use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use serde_json::json;
use socsim_core::{derive_seed, AgentId, Mechanism, Phase, SimRng, SocsimError, StepContext};
use socsim_llm::{CallMetadata, LlmConfig};

use super::manifest::verify_manifest;
use super::{
    BackendFactory, BackendIdentity, CacheKeyV1, ExecutorOptions, HasLlmManifest, LlmExecutor,
    LlmReply, LlmRuleBook, ParseFlags, ReplayMode, Resolution, ResolveContext, ResponseRecord,
    ResponseStore,
};
use crate::{
    DecisionContext, DecisionScope, DecisionUnit, ExperimentState, GameError, GameWorld,
    KindCatalog, LearnContext, MatchContext, Matching, OnInvalidAction, OutcomeRule, PlayOptions,
    PolicyTable, RuleBook, RuleError, StageGame,
};

pub struct LlmSetup<G: StageGame, X: ExperimentState> {
    pub(crate) book: LlmRuleBook<G, X>,
    store: ResponseStore,
    backend: BackendFactory,
    identity: BackendIdentity,
    executor: LlmExecutor,
    options: ExecutorOptions,
    config: LlmConfig,
}

impl<G: StageGame, X: ExperimentState> LlmSetup<G, X> {
    pub fn new(
        book: LlmRuleBook<G, X>,
        store: ResponseStore,
        backend: BackendFactory,
        identity: BackendIdentity,
    ) -> Self {
        Self {
            book,
            store,
            backend,
            identity,
            executor: LlmExecutor::default(),
            options: ExecutorOptions::default(),
            config: LlmConfig::deterministic(),
        }
    }

    pub fn executor(mut self, executor: LlmExecutor) -> Self {
        self.executor = executor;
        self
    }

    pub fn options(mut self, options: ExecutorOptions) -> Self {
        self.options = options;
        self
    }

    pub fn config(mut self, config: LlmConfig) -> Self {
        self.config = config;
        self
    }
}

/// Decision/Reward mechanism for kinds registered in an [`LlmRuleBook`].
pub struct LlmDriver<G: StageGame, X: ExperimentState> {
    rules: LlmRuleBook<G, X>,
    store: ResponseStore,
    backend: BackendFactory,
    identity: BackendIdentity,
    executor: LlmExecutor,
    options: ExecutorOptions,
    config: LlmConfig,
}

impl<G: StageGame, X: ExperimentState> LlmDriver<G, X> {
    pub(crate) fn new(setup: LlmSetup<G, X>) -> Self {
        Self {
            rules: setup.book,
            store: setup.store,
            backend: setup.backend,
            identity: setup.identity,
            executor: setup.executor,
            options: setup.options,
            config: setup.config,
        }
    }

    fn mechanism_error(error: impl std::fmt::Display) -> SocsimError {
        SocsimError::Mechanism(error.to_string())
    }
}

struct PendingDecision {
    agent: AgentId,
    agent_index: usize,
    seat_indices: Vec<usize>,
    role: usize,
    state_slot: usize,
    kind: String,
    request: super::DecisionRequest,
}

struct UniqueRequest {
    key: CacheKeyV1,
    config: LlmConfig,
    members: Vec<usize>,
}

type LogicalReply = (CacheKeyV1, LlmReply, bool, u64);

#[derive(Default)]
struct StepRates {
    attempts: usize,
    parse_failures: usize,
    clamped: usize,
    value_was_string: usize,
    lenient: usize,
    cache_hits: usize,
    retried: BTreeSet<AgentId>,
    gave_up: BTreeSet<AgentId>,
}

impl<G, X> LlmDriver<G, X>
where
    G: StageGame,
    X: HasLlmManifest,
{
    /// Checks the world's manifest against this driver's store and backend on
    /// every Decision, so a snapshot restored into an already-running
    /// simulation is checked too (the engine's `restore` does not call
    /// mechanisms).
    fn verify_world_manifest(&self, world: &GameWorld<G, X>) -> Result<(), SocsimError> {
        verify_manifest(world.x.llm_manifest(), &self.store, &self.identity)
            .map_err(Self::mechanism_error)
    }

    fn prepare(
        &self,
        step: &StepContext<'_, GameWorld<G, X>>,
    ) -> Result<Vec<PendingDecision>, SocsimError> {
        let world = &*step.world;
        let mut seats_by_agent = vec![Vec::new(); world.ids.len()];
        for (seat_index, &agent) in world.plays.seats.iter().enumerate() {
            let index = usize::try_from(agent.0)
                .map_err(|_| Self::mechanism_error("agent index exceeds usize"))?;
            let seats = seats_by_agent
                .get_mut(index)
                .ok_or_else(|| Self::mechanism_error(format!("unknown agent {}", agent.0)))?;
            seats.push(seat_index);
        }

        let mut pending = Vec::new();
        for &agent in step.agent_order {
            let agent_index = usize::try_from(agent.0)
                .map_err(|_| Self::mechanism_error("agent index exceeds usize"))?;
            let seat_indices = seats_by_agent
                .get(agent_index)
                .ok_or_else(|| Self::mechanism_error(format!("unknown agent {}", agent.0)))?;
            if seat_indices.is_empty() {
                continue;
            }
            let state_slot = *world
                .policies
                .slot
                .get(agent_index)
                .ok_or_else(|| Self::mechanism_error(format!("unknown agent {}", agent.0)))?;
            if state_slot == PolicyTable::<X::PolicyState>::HELD {
                continue;
            }
            let state_slot = state_slot as usize;
            let kind_id = world.policies.kind_of[state_slot];
            let kind = &world.policies.kinds[kind_id.0 as usize];
            let Some(rule) = self.rules.rules.get(kind) else {
                continue;
            };
            let roles = seat_indices
                .iter()
                .map(|seat_index| seat_index % world.plays.n_roles)
                .collect::<BTreeSet<_>>();
            if roles.len() > 1 {
                return Err(Self::mechanism_error(RuleError::AmbiguousRole {
                    agent: agent.0,
                }));
            }
            let &seat_index = seat_indices
                .last()
                .expect("non-empty seat lists were checked above");
            let encounter_index = seat_index / world.plays.n_roles;
            let start = encounter_index * world.plays.n_roles;
            let end = start + world.plays.n_roles;
            let role = seat_index % world.plays.n_roles;
            let opponents = world.plays.seats[start..end]
                .iter()
                .copied()
                .filter(|seat| *seat != agent)
                .collect();
            let context = DecisionContext {
                t: world.clock.t(),
                me: agent,
                unit: DecisionUnit::Step,
                role: Some(role),
                encounter: world
                    .plays
                    .encounter(crate::EncounterId(encounter_index as u32)),
                opponents,
                ids: &world.ids,
                game: &world.game,
                held: &world.held,
                history: &world.step_history,
                pair_history: world.pair_history.as_ref(),
                x: &world.x,
            };
            let request = rule
                .prepare(&world.policies.states[state_slot], &context)
                .map_err(Self::mechanism_error)?;
            pending.push(PendingDecision {
                agent,
                agent_index,
                seat_indices: seat_indices.clone(),
                role,
                state_slot,
                kind: kind.clone(),
                request,
            });
        }
        Ok(pending)
    }

    fn seed_for(&self, root: u64, t: u64, decision: &PendingDecision) -> u64 {
        if self.config.temperature == 0.0 {
            derive_seed(root, &[0])
        } else {
            derive_seed(
                root,
                &[
                    t,
                    decision.agent.0,
                    0,
                    0,
                    u64::from(decision.request.attempt),
                ],
            )
        }
    }

    fn key_and_config(
        &self,
        root: u64,
        t: u64,
        decision: &PendingDecision,
    ) -> (CacheKeyV1, LlmConfig) {
        let seed = self.seed_for(root, t, decision);
        let mut config = self.config.clone().with_seed(seed);
        if let Some(system) = &decision.request.system {
            config.system = Some(system.clone());
        }
        let key = CacheKeyV1 {
            version: 1,
            backend: self.identity.clone(),
            system: config.system.clone(),
            prompt: decision.request.prompt.clone(),
            temperature_bits: config.temperature.to_bits(),
            seed,
            omit_seed: config.omit_seed,
            max_tokens: config.max_tokens,
            think: config.think,
            allow_blank: config.allow_blank,
        };
        (key, config)
    }

    fn obtain_replies(
        &mut self,
        root: u64,
        t: u64,
        pending: &[PendingDecision],
    ) -> Result<Vec<LogicalReply>, SocsimError> {
        let mut canonical_to_unique = BTreeMap::<String, usize>::new();
        let mut unique = Vec::<UniqueRequest>::new();
        let mut member_unique = Vec::with_capacity(pending.len());
        for (member, decision) in pending.iter().enumerate() {
            let (key, config) = self.key_and_config(root, t, decision);
            let canonical = key.canonical_json();
            let unique_index = if let Some(&index) = canonical_to_unique.get(&canonical) {
                unique[index].members.push(member);
                index
            } else {
                let index = unique.len();
                canonical_to_unique.insert(canonical, index);
                unique.push(UniqueRequest {
                    key,
                    config,
                    members: vec![member],
                });
                index
            };
            member_unique.push(unique_index);
        }

        let mut replies: Vec<Option<LlmReply>> = vec![None; unique.len()];
        let mut misses = BTreeMap::new();
        for (index, request) in unique.iter().enumerate() {
            let stored = (self.store.mode() != ReplayMode::Live)
                .then(|| self.store.get(&request.key))
                .flatten();
            if let Some(record) = stored {
                replies[index] = Some(LlmReply {
                    text: record.text.clone(),
                    thinking: record.thinking.clone(),
                    usage: record.usage,
                    cache_hit: true,
                });
            } else if self.store.mode() == ReplayMode::ReplayOnly {
                return Err(Self::mechanism_error(RuleError::Backend(format!(
                    "ReplayOnly response store has no record for key {:016x}",
                    request.key.digest()
                ))));
            } else {
                misses.insert(
                    index,
                    (
                        request.key.clone(),
                        request.config.clone(),
                        request.key.prompt.clone(),
                    ),
                );
            }
        }

        for (index, result) in self.executor.execute(&self.backend, misses, &self.options) {
            let reply = result.map_err(Self::mechanism_error)?;
            if self.store.mode() == ReplayMode::RecordMisses {
                let request = &unique[index];
                self.store
                    .insert(ResponseRecord {
                        key: request.key.clone(),
                        text: reply.text.clone(),
                        thinking: reply.thinking.clone(),
                        usage: reply.usage,
                        backend_metadata: Some(CallMetadata {
                            model: self.identity.model.clone(),
                            endpoint: self.identity.endpoint.clone(),
                            temperature: request.config.temperature,
                            seed: request.config.seed,
                            cache_hit: false,
                            usage: reply.usage,
                        }),
                    })
                    .map_err(Self::mechanism_error)?;
            }
            replies[index] = Some(reply);
        }

        let logical = member_unique
            .into_iter()
            .enumerate()
            .map(|(member, unique_index)| {
                let request = &unique[unique_index];
                (
                    request.key.clone(),
                    replies[unique_index]
                        .clone()
                        .expect("each unique request is filled by the store or executor"),
                    request.members.first().copied() != Some(member),
                    request.config.seed,
                )
            })
            .collect();
        Ok(logical)
    }

    fn decide(&mut self, step: &mut StepContext<'_, GameWorld<G, X>>) -> socsim_core::Result<()> {
        self.verify_world_manifest(step.world)?;
        let mut pending = self.prepare(step)?;
        if pending.is_empty() {
            return Ok(());
        }
        let decision_count = pending.len();
        let t = step.world.clock.t();
        let root = step.world.decision_root;
        let mut rates = StepRates::default();
        let mut finalized = Vec::with_capacity(decision_count);

        while !pending.is_empty() {
            let logical_replies = self.obtain_replies(root, t, &pending)?;
            let mut retry = Vec::new();
            for (mut decision, (key, reply, deduplicated, seed)) in
                pending.into_iter().zip(logical_replies)
            {
                rates.attempts += 1;
                if reply.cache_hit {
                    rates.cache_hits += 1;
                }
                let rule = Arc::clone(
                    self.rules
                        .rules
                        .get(&decision.kind)
                        .expect("prepared kinds remain registered"),
                );
                let fallback_seed = derive_seed(root, &[t, decision.agent.0, 0, 0, u64::MAX]);
                let mut fallback_rng = SimRng::from_seed(fallback_seed);
                let previous = step.world.held[decision.agent_index].clone();
                let mut resolve_context = ResolveContext {
                    game: &step.world.game,
                    role: Some(decision.role),
                    previous: &previous,
                    fallback_rng: &mut fallback_rng,
                    t,
                };
                let resolution = rule.resolve(
                    &mut step.world.policies.states[decision.state_slot],
                    &decision.request,
                    &reply,
                    &mut resolve_context,
                );

                let event_attempt = decision.request.attempt;
                let event_system = key.system.clone();
                let event_prompt = decision.request.prompt.clone();
                let event_kind = decision.kind.clone();
                let mut next_request = None;

                let (resolution_name, flags, action) = match resolution {
                    Resolution::Accept { action, flags } => {
                        rates.clamped += usize::from(flags.clamped);
                        rates.value_was_string += usize::from(flags.value_was_string);
                        rates.lenient += usize::from(flags.lenient_match);
                        ("accept", flags, Some(action))
                    }
                    Resolution::Retry(next) => {
                        rates.parse_failures += 1;
                        rates.retried.insert(decision.agent);
                        next_request = Some(next);
                        ("retry", ParseFlags::default(), None)
                    }
                    Resolution::GiveUp { action, .. } => {
                        rates.parse_failures += 1;
                        rates.gave_up.insert(decision.agent);
                        ("giveup", ParseFlags::default(), Some(action))
                    }
                };

                step.recorder.record_event(
                    t,
                    "game.llm_call",
                    json!({
                        "agent": decision.agent.0,
                        "kind": event_kind,
                        "unit": "step",
                        "unit_index": 0,
                        "attempt": event_attempt,
                        "resolution": resolution_name,
                        "system": event_system,
                        "prompt": event_prompt,
                        "reply": reply.text,
                        "thinking": reply.thinking,
                        "seed": seed,
                        "usage": reply.usage,
                        "cache_hit": reply.cache_hit,
                        "deduplicated": deduplicated,
                        "key": format!("{:016x}", key.digest()),
                        "flags": flags,
                    }),
                );

                if let Some(next) = next_request {
                    decision.request = next;
                    retry.push(decision);
                    continue;
                }
                if let Some(action) = action {
                    step.world
                        .game
                        .check_action(decision.role, &action)
                        .map_err(|error| {
                            Self::mechanism_error(RuleError::InvalidAction(error.to_string()))
                        })?;
                    finalized.push((decision.agent_index, decision.seat_indices, action));
                }
            }
            pending = retry;
        }

        for (agent_index, seat_indices, action) in finalized {
            step.world.held[agent_index] = action.clone();
            for seat_index in seat_indices {
                step.world.plays.actions[seat_index] = Some(action.clone());
            }
        }

        let attempts = rates.attempts as f64;
        let decisions = decision_count as f64;
        step.recorder.record_row(
            t,
            "game",
            &[
                (
                    "llm.parse_fail_rate",
                    rates.parse_failures as f64 / attempts,
                ),
                ("llm.retry_rate", rates.retried.len() as f64 / decisions),
                ("llm.clamp_rate", rates.clamped as f64 / attempts),
                (
                    "llm.value_was_string_rate",
                    rates.value_was_string as f64 / attempts,
                ),
                ("llm.lenient_rate", rates.lenient as f64 / attempts),
                ("llm.cache_hit_rate", rates.cache_hits as f64 / attempts),
                ("llm.giveup_rate", rates.gave_up.len() as f64 / decisions),
            ],
        );
        Ok(())
    }

    fn learn(&self, step: &mut StepContext<'_, GameWorld<G, X>>) -> socsim_core::Result<()> {
        let world = &mut *step.world;
        for &agent in &world.ids {
            let agent_index = agent.0 as usize;
            let state_slot = world.policies.slot[agent_index];
            if state_slot == PolicyTable::<X::PolicyState>::HELD {
                continue;
            }
            let state_slot = state_slot as usize;
            let kind_id = world.policies.kind_of[state_slot];
            let kind = &world.policies.kinds[kind_id.0 as usize];
            let Some(rule) = self.rules.rules.get(kind) else {
                continue;
            };
            let record = world
                .step_history
                .view(agent)
                .and_then(|history| history.last())
                .ok_or_else(|| {
                    Self::mechanism_error(format!(
                        "missing current step record for agent {}",
                        agent.0
                    ))
                })?;
            let context = LearnContext {
                t: world.clock.t(),
                me: agent,
                scope: DecisionScope::PerStep,
                record,
                ids: &world.ids,
                game: &world.game,
                held: &world.held,
                history: &world.step_history,
                plays: &world.plays,
                x: &world.x,
            };
            rule.learn_step(&mut world.policies.states[state_slot], &context)
                .map_err(Self::mechanism_error)?;
        }
        Ok(())
    }
}

impl<G, X> Mechanism<GameWorld<G, X>> for LlmDriver<G, X>
where
    G: StageGame,
    X: HasLlmManifest,
{
    fn name(&self) -> &str {
        "game_llm_decide"
    }

    fn phases(&self) -> &'static [Phase] {
        &[Phase::Decision, Phase::Reward]
    }

    fn apply(
        &mut self,
        phase: Phase,
        step: &mut StepContext<'_, GameWorld<G, X>>,
    ) -> socsim_core::Result<()> {
        match phase {
            Phase::Decision => self.decide(step),
            Phase::Reward => self.learn(step),
            _ => Ok(()),
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn assemble_with_llm<G, X, M, O>(
    mut world: GameWorld<G, X>,
    matching: M,
    rules: RuleBook<G, X>,
    llm: LlmSetup<G, X>,
    outcome: O,
    opts: PlayOptions<G::Action>,
    seed: u64,
) -> Result<socsim_engine::SimulationBuilder<GameWorld<G, X>>, GameError>
where
    G: StageGame,
    X: HasLlmManifest,
    M: Matching<G, X> + 'static,
    O: OutcomeRule<G, X> + 'static,
{
    if opts.decision_scope != DecisionScope::PerStep {
        return Err(
            RuleError::Unsupported("LLM rules support only per-step decisions".to_owned()).into(),
        );
    }
    if world.game.n_roles() != 2 {
        return Err(GameError::UnsupportedRoles {
            n_roles: world.game.n_roles(),
        });
    }

    let catalog = KindCatalog::new().with_rules(&rules)?.with_llm(&llm.book)?;
    let expected: Vec<_> = catalog.kinds().map(str::to_owned).collect();
    let actual = world.policies.kinds.clone();
    if actual != expected {
        return Err(GameError::InvalidKinds { expected, actual });
    }

    let mut active_rule_kinds = BTreeSet::new();
    for &state_slot in &world.policies.slot {
        if state_slot == PolicyTable::<X::PolicyState>::HELD {
            continue;
        }
        let state_index = state_slot as usize;
        let kind_id = world
            .policies
            .kind_of
            .get(state_index)
            .ok_or_else(|| GameError::Rule(RuleError::UnknownKind(state_index.to_string())))?;
        let kind = world
            .policies
            .kinds
            .get(kind_id.0 as usize)
            .ok_or_else(|| GameError::Rule(RuleError::UnknownKind(kind_id.0.to_string())))?;
        if rules.get_optional(kind).is_some() {
            active_rule_kinds.insert(kind.as_str());
        }
    }

    if let OnInvalidAction::Fallback(fallbacks) = &opts.on_invalid_action {
        for &kind in &active_rule_kinds {
            if !fallbacks.contains_key(kind) {
                return Err(RuleError::Unsupported(format!(
                    "missing fallback action for rule kind {kind}"
                ))
                .into());
            }
        }
        for (kind, action) in fallbacks {
            rules.get(kind)?;
            for role in 0..world.game.n_roles() {
                world.game.check_action(role, action)?;
            }
        }
    }

    let context = MatchContext {
        t: world.clock.t(),
        decision_scope: opts.decision_scope,
        ids: &world.ids,
        game: &world.game,
        topology: &world.topology,
        held: &world.held,
        history: &world.step_history,
        x: &world.x,
    };
    matching.check(&context)?;
    world.matching_state = matching.init_state(&context);

    Ok(socsim_engine::SimulationBuilder::new(world)
        .seed(derive_seed(seed, &[1]))
        .add_mechanism(Box::new(crate::mechanism::MatchMechanism::<G, X, M>::new(
            matching,
            opts.decision_scope,
        )))
        .add_mechanism(Box::new(crate::RuleDriver::new(
            rules,
            opts.on_invalid_action,
            opts.decision_scope,
        )))
        .add_mechanism(Box::new(LlmDriver::new(llm)))
        .add_mechanism(Box::new(crate::mechanism::PlayMechanism::<G, X, O>::new(
            outcome,
            opts.aggregation,
            opts.decision_scope,
        ))))
}
