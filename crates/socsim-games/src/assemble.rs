use std::collections::BTreeMap;

#[cfg(feature = "engine")]
use std::collections::BTreeSet;

use rand::seq::SliceRandom;
use socsim_core::{derive_seed, AgentId, SimClock, SimRng};

use crate::{
    ExperimentState, GameError, GameWorld, InternalStepHistory, MatchingState, PlayArena,
    PolicyTable, RuleBook, RuleInitContext, StageGame, Topology,
};

#[cfg(feature = "engine")]
use crate::{MatchContext, Matching, OutcomeRule};

#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum InitialActions<A> {
    Same(A),
    PerAgent(Vec<A>),
}

#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum Assignment {
    Ordered(Vec<AssignGroup>),
    Shuffled(Vec<AssignGroup>),
}

#[derive(Clone, Debug)]
pub struct AssignGroup {
    pub kind: AssignKind,
    pub count: usize,
}

#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum AssignKind {
    Rule(String),
    Held,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum PayoffAggregation {
    #[default]
    Sum,
    Mean,
}

#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum OnInvalidAction<A> {
    Stop,
    Fallback(BTreeMap<String, A>),
}

// Kept manual by contract so this remains explicit about not requiring `A: Default`.
#[allow(clippy::derivable_impls)]
impl<A> Default for OnInvalidAction<A> {
    fn default() -> Self {
        Self::Stop
    }
}

#[derive(Clone, Debug)]
pub struct PlayOptions<A> {
    pub(crate) aggregation: PayoffAggregation,
    pub(crate) on_invalid_action: OnInvalidAction<A>,
}

impl<A> Default for PlayOptions<A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A> PlayOptions<A> {
    pub fn new() -> Self {
        Self {
            aggregation: PayoffAggregation::Sum,
            on_invalid_action: OnInvalidAction::Stop,
        }
    }

    pub fn payoff_aggregation(mut self, aggregation: PayoffAggregation) -> Self {
        self.aggregation = aggregation;
        self
    }

    pub fn on_invalid_action(mut self, policy: OnInvalidAction<A>) -> Self {
        self.on_invalid_action = policy;
        self
    }
}

#[allow(clippy::too_many_arguments)]
pub fn build_world<G, X>(
    game: G,
    n_agents: usize,
    x: X,
    initial_held: InitialActions<G::Action>,
    rules: &RuleBook<G, X>,
    assign: &Assignment,
    history_len: usize,
    t_max: u64,
    seed: u64,
) -> Result<GameWorld<G, X>, GameError>
where
    G: StageGame,
    X: ExperimentState,
{
    if history_len == 0 {
        return Err(GameError::InvalidHistoryLength);
    }

    let held = match initial_held {
        InitialActions::Same(action) => vec![action; n_agents],
        InitialActions::PerAgent(actions) => {
            if actions.len() != n_agents {
                return Err(GameError::AgentCount {
                    expected: n_agents,
                    actual: actions.len(),
                });
            }
            actions
        }
    };

    for action in &held {
        for role in 0..game.n_roles() {
            game.check_action(role, action)?;
        }
    }

    let (groups, shuffled) = match assign {
        Assignment::Ordered(groups) => (groups, false),
        Assignment::Shuffled(groups) => (groups, true),
    };
    let assigned_count = groups.iter().map(|group| group.count).sum::<usize>();
    if assigned_count != n_agents {
        return Err(GameError::AgentCount {
            expected: n_agents,
            actual: assigned_count,
        });
    }
    let mut assignments = Vec::with_capacity(n_agents);
    for group in groups {
        assignments.extend((0..group.count).map(|_| group.kind.clone()));
    }
    if shuffled {
        let mut rng = SimRng::from_seed(derive_seed(seed, &[0]));
        assignments.shuffle(&mut rng);
    }

    let ids: Vec<_> = (0..n_agents as u64).map(AgentId).collect();
    let kinds = rules.kinds();
    let mut slot = Vec::with_capacity(n_agents);
    let mut kind_of = Vec::new();
    let mut states = Vec::new();
    for (index, assignment) in assignments.into_iter().enumerate() {
        match assignment {
            AssignKind::Held => slot.push(PolicyTable::<X::PolicyState>::HELD),
            AssignKind::Rule(kind) => {
                let rule = rules.get(&kind)?;
                let kind_id = rules.kind_id(&kind)?;
                let state_index =
                    u32::try_from(states.len()).map_err(|_| GameError::AgentCount {
                        expected: u32::MAX as usize,
                        actual: n_agents,
                    })?;
                let agent = AgentId(index as u64);
                let generation = 0;
                let context = RuleInitContext {
                    agent,
                    kind: kind_id,
                    generation,
                    init_seed: derive_seed(seed, &[3, agent.0, u64::from(generation)]),
                };
                states.push(rule.init_state(&context, &game)?);
                kind_of.push(kind_id);
                slot.push(state_index);
            }
        }
    }

    Ok(GameWorld {
        clock: SimClock::new(t_max),
        ids,
        topology: Topology::None,
        held,
        policies: PolicyTable::new(kinds, slot, kind_of, states),
        matching_state: MatchingState::None,
        plays: PlayArena::new(game.n_roles()),
        payoff: vec![0.0; n_agents],
        n_played: vec![0; n_agents],
        step_history: InternalStepHistory::new(n_agents, history_len),
        game,
        x,
        decision_root: derive_seed(seed, &[2]),
    })
}

#[cfg(feature = "engine")]
pub fn assemble<G, X, M, O>(
    mut world: GameWorld<G, X>,
    matching: M,
    rules: RuleBook<G, X>,
    outcome: O,
    opts: PlayOptions<G::Action>,
    seed: u64,
) -> Result<socsim_engine::SimulationBuilder<GameWorld<G, X>>, GameError>
where
    G: StageGame,
    X: ExperimentState,
    M: Matching<G, X> + 'static,
    O: OutcomeRule<G, X> + 'static,
{
    if world.game.n_roles() != 2 {
        return Err(GameError::UnsupportedRoles {
            n_roles: world.game.n_roles(),
        });
    }

    let mut active_kinds = BTreeSet::new();
    for &state_slot in &world.policies.slot {
        if state_slot == PolicyTable::<X::PolicyState>::HELD {
            continue;
        }
        let state_index = state_slot as usize;
        let kind_id = world.policies.kind_of.get(state_index).ok_or_else(|| {
            GameError::Rule(crate::RuleError::UnknownKind(state_index.to_string()))
        })?;
        let kind = world
            .policies
            .kinds
            .get(kind_id.0 as usize)
            .ok_or_else(|| GameError::Rule(crate::RuleError::UnknownKind(kind_id.0.to_string())))?;
        rules.get(kind)?;
        active_kinds.insert(kind.as_str());
    }

    if let OnInvalidAction::Fallback(fallbacks) = &opts.on_invalid_action {
        for &kind in &active_kinds {
            if !fallbacks.contains_key(kind) {
                return Err(crate::RuleError::Unsupported(format!(
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
        ids: &world.ids,
        game: &world.game,
        topology: &world.topology,
        held: &world.held,
        history: &world.step_history,
        x: &world.x,
    };
    matching.check(&context)?;
    world.matching_state = matching.init_state(&context);

    let builder = socsim_engine::SimulationBuilder::new(world)
        .seed(derive_seed(seed, &[1]))
        .add_mechanism(Box::new(crate::mechanism::MatchMechanism::<G, X, M>::new(
            matching,
        )))
        .add_mechanism(Box::new(crate::RuleDriver::new(
            rules,
            opts.on_invalid_action,
        )))
        .add_mechanism(Box::new(crate::mechanism::PlayMechanism::<G, X, O>::new(
            outcome,
            opts.aggregation,
        )));
    Ok(builder)
}
