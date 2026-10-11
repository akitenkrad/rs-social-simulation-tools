use std::collections::{BTreeMap, BTreeSet};

use socsim_core::{AgentId, Mechanism, Phase, Recorder, SocsimError, StepContext};

use crate::{
    DecisionContext, DecisionScope, DecisionUnit, ExperimentState, GameWorld, LearnContext,
    OnInvalidAction, PolicyTable, RuleBook, RuleError, StageGame,
};

pub struct RuleDriver<G: StageGame, X: ExperimentState> {
    rules: RuleBook<G, X>,
    on_invalid_action: OnInvalidAction<G::Action>,
    decision_scope: DecisionScope,
    report_mixed: bool,
}

impl<G: StageGame, X: ExperimentState> RuleDriver<G, X> {
    pub(crate) fn new(
        rules: RuleBook<G, X>,
        on_invalid_action: OnInvalidAction<G::Action>,
        decision_scope: DecisionScope,
        report_mixed: bool,
    ) -> Self {
        Self {
            rules,
            on_invalid_action,
            decision_scope,
            report_mixed,
        }
    }

    fn mechanism_error(error: impl std::fmt::Display) -> SocsimError {
        SocsimError::Mechanism(error.to_string())
    }

    #[allow(clippy::too_many_arguments)]
    fn validate_action(
        &self,
        t: u64,
        agent: AgentId,
        role: usize,
        kind: &str,
        mut action: G::Action,
        game: &G,
        recorder: &mut dyn Recorder,
    ) -> socsim_core::Result<G::Action> {
        if let Err(error) = game.check_action(role, &action) {
            match &self.on_invalid_action {
                OnInvalidAction::Stop => {
                    return Err(Self::mechanism_error(RuleError::InvalidAction(
                        error.to_string(),
                    )));
                }
                OnInvalidAction::Fallback(fallbacks) => {
                    let replacement = fallbacks.get(kind).ok_or_else(|| {
                        Self::mechanism_error(RuleError::Unsupported(format!(
                            "missing fallback action for rule kind {kind}"
                        )))
                    })?;
                    game.check_action(role, replacement)
                        .map_err(|fallback_error| {
                            Self::mechanism_error(RuleError::InvalidAction(
                                fallback_error.to_string(),
                            ))
                        })?;
                    recorder.record_event(
                        t,
                        "game.invalid_action",
                        serde_json::json!({
                            "agent": agent.0,
                            "kind": kind,
                            "role": role,
                            "action": format!("{action:?}"),
                        }),
                    );
                    action = replacement.clone();
                }
            }
        }
        Ok(action)
    }

    fn decide(&self, step: &mut StepContext<'_, GameWorld<G, X>>) -> socsim_core::Result<()> {
        let GameWorld {
            clock,
            ids,
            game,
            held,
            policies,
            plays,
            step_history,
            pair_history,
            aggregate,
            x,
            ..
        } = step.world;

        let mut seats_by_agent = vec![Vec::new(); ids.len()];
        for (seat_index, &agent) in plays.seats.iter().enumerate() {
            let agent_index = usize::try_from(agent.0)
                .map_err(|_| SocsimError::Mechanism("agent index exceeds usize".to_owned()))?;
            let seats = seats_by_agent
                .get_mut(agent_index)
                .ok_or_else(|| SocsimError::Mechanism(format!("unknown agent {}", agent.0)))?;
            seats.push(seat_index);
        }

        for &agent in step.agent_order {
            let agent_index = usize::try_from(agent.0)
                .map_err(|_| SocsimError::Mechanism("agent index exceeds usize".to_owned()))?;
            let seat_indices = seats_by_agent
                .get(agent_index)
                .ok_or_else(|| SocsimError::Mechanism(format!("unknown agent {}", agent.0)))?;
            if seat_indices.is_empty() {
                continue;
            }
            let state_slot = *policies
                .slot
                .get(agent_index)
                .ok_or_else(|| SocsimError::Mechanism(format!("unknown agent {}", agent.0)))?;

            if state_slot == PolicyTable::<X::PolicyState>::HELD {
                if self.decision_scope == DecisionScope::PerStep
                    && seat_indices
                        .iter()
                        .map(|seat_index| seat_index % plays.n_roles)
                        .collect::<BTreeSet<_>>()
                        .len()
                        > 1
                {
                    return Err(Self::mechanism_error(RuleError::AmbiguousRole {
                        agent: agent.0,
                    }));
                }
                for &seat_index in seat_indices {
                    plays.actions[seat_index] = Some(held[agent_index].clone());
                }
                continue;
            }

            let state_index = state_slot as usize;
            let kind_id = policies.kind_of[state_index];
            let kind = &policies.kinds[kind_id.0 as usize];
            let Some(rule) = self.rules.get_optional(kind) else {
                continue;
            };
            if self.decision_scope == DecisionScope::PerStep
                && seat_indices
                    .iter()
                    .map(|seat_index| seat_index % plays.n_roles)
                    .collect::<BTreeSet<_>>()
                    .len()
                    > 1
            {
                return Err(Self::mechanism_error(RuleError::AmbiguousRole {
                    agent: agent.0,
                }));
            }
            let state = &mut policies.states[state_index];

            match self.decision_scope {
                DecisionScope::PerStep => {
                    let &seat_index = seat_indices
                        .last()
                        .expect("non-empty seat lists were checked above");
                    let encounter_index = seat_index / plays.n_roles;
                    let start = encounter_index * plays.n_roles;
                    let end = start + plays.n_roles;
                    let role = seat_index % plays.n_roles;
                    let opponents = plays.seats[start..end]
                        .iter()
                        .copied()
                        .filter(|seat| *seat != agent)
                        .collect();
                    let context = DecisionContext {
                        t: clock.t(),
                        me: agent,
                        unit: DecisionUnit::Step,
                        role: Some(role),
                        encounter: plays.encounter(crate::EncounterId(encounter_index as u32)),
                        opponents,
                        ids,
                        game,
                        held,
                        history: step_history,
                        pair_history: pair_history.as_ref(),
                        aggregate: aggregate.as_ref(),
                        x,
                    };
                    let action = rule
                        .decide(state, &context, step.rng)
                        .map_err(Self::mechanism_error)?;
                    let action = self.validate_action(
                        clock.t(),
                        agent,
                        role,
                        kind,
                        action,
                        game,
                        step.recorder,
                    )?;
                    held[agent_index] = action.clone();
                    for &seat_index in seat_indices {
                        plays.actions[seat_index] = Some(action.clone());
                    }
                }
                DecisionScope::PerAgentRole => {
                    let mut seats_by_role = BTreeMap::<usize, Vec<usize>>::new();
                    for &seat_index in seat_indices {
                        seats_by_role
                            .entry(seat_index % plays.n_roles)
                            .or_default()
                            .push(seat_index);
                    }
                    for (role, role_seats) in seats_by_role {
                        let mut opponent_set = BTreeSet::new();
                        for &seat_index in &role_seats {
                            let encounter_index = seat_index / plays.n_roles;
                            let start = encounter_index * plays.n_roles;
                            let end = start + plays.n_roles;
                            opponent_set.extend(
                                plays.seats[start..end]
                                    .iter()
                                    .copied()
                                    .filter(|seat| *seat != agent),
                            );
                        }
                        let context = DecisionContext {
                            t: clock.t(),
                            me: agent,
                            unit: DecisionUnit::AgentRole(role),
                            role: Some(role),
                            encounter: None,
                            opponents: opponent_set.into_iter().collect(),
                            ids,
                            game,
                            held,
                            history: step_history,
                            pair_history: pair_history.as_ref(),
                            aggregate: aggregate.as_ref(),
                            x,
                        };
                        let action = rule
                            .decide(state, &context, step.rng)
                            .map_err(Self::mechanism_error)?;
                        let action = self.validate_action(
                            clock.t(),
                            agent,
                            role,
                            kind,
                            action,
                            game,
                            step.recorder,
                        )?;
                        for seat_index in role_seats {
                            plays.actions[seat_index] = Some(action.clone());
                        }
                    }
                }
                DecisionScope::PerEncounter => {
                    for &seat_index in seat_indices {
                        let encounter_index = seat_index / plays.n_roles;
                        let start = encounter_index * plays.n_roles;
                        let end = start + plays.n_roles;
                        let role = seat_index % plays.n_roles;
                        let opponents = plays.seats[start..end]
                            .iter()
                            .enumerate()
                            .filter(|(other_role, _)| *other_role != role)
                            .map(|(_, &opponent)| opponent)
                            .collect();
                        let encounter_id = crate::EncounterId(encounter_index as u32);
                        let context = DecisionContext {
                            t: clock.t(),
                            me: agent,
                            unit: DecisionUnit::Encounter(encounter_id),
                            role: Some(role),
                            encounter: plays.encounter(encounter_id),
                            opponents,
                            ids,
                            game,
                            held,
                            history: step_history,
                            pair_history: pair_history.as_ref(),
                            aggregate: aggregate.as_ref(),
                            x,
                        };
                        let action = rule
                            .decide(state, &context, step.rng)
                            .map_err(Self::mechanism_error)?;
                        let action = self.validate_action(
                            clock.t(),
                            agent,
                            role,
                            kind,
                            action,
                            game,
                            step.recorder,
                        )?;
                        plays.actions[seat_index] = Some(action);
                    }
                }
            }
        }
        Ok(())
    }

    fn learn(&self, step: &mut StepContext<'_, GameWorld<G, X>>) -> socsim_core::Result<()> {
        let GameWorld {
            clock,
            ids,
            game,
            held,
            policies,
            plays,
            step_history,
            reports,
            x,
            ..
        } = step.world;

        let mut agents = ids.clone();
        agents.sort_unstable();
        for agent in agents {
            let agent_index = agent.0 as usize;
            let state_slot = policies.slot[agent_index];
            if state_slot == PolicyTable::<X::PolicyState>::HELD {
                continue;
            }
            let kind_id = policies.kind_of[state_slot as usize];
            let kind = &policies.kinds[kind_id.0 as usize];
            let Some(rule) = self.rules.get_optional(kind) else {
                continue;
            };
            let state = &mut policies.states[state_slot as usize];
            let record = step_history
                .view(agent)
                .and_then(|history| history.last())
                .ok_or_else(|| {
                    SocsimError::Mechanism(format!(
                        "missing current step record for agent {}",
                        agent.0
                    ))
                })?;
            let context = LearnContext {
                t: clock.t(),
                me: agent,
                scope: self.decision_scope,
                record,
                ids,
                game,
                held,
                history: step_history,
                plays,
                x,
            };
            if rule.needs_encounter_learning() {
                for outcome in context.outcomes() {
                    rule.learn_encounter(state, &outcome, &context)
                        .map_err(Self::mechanism_error)?;
                }
            }
            rule.learn_step(state, &context)
                .map_err(Self::mechanism_error)?;
        }

        if self.report_mixed {
            let reports = reports.as_mut().ok_or_else(|| {
                SocsimError::Mechanism(
                    "mixed-strategy reporting is enabled but the report table is missing"
                        .to_owned(),
                )
            })?;
            reports.reset(clock.t());
            let mut agents = ids.clone();
            agents.sort_unstable();
            for agent in agents {
                let agent_index = usize::try_from(agent.0)
                    .map_err(|_| SocsimError::Mechanism("agent index exceeds usize".to_owned()))?;
                let state_slot = policies.slot[agent_index];
                if state_slot == PolicyTable::<X::PolicyState>::HELD {
                    continue;
                }
                let state_index = state_slot as usize;
                let kind_id = policies.kind_of[state_index];
                let kind = &policies.kinds[kind_id.0 as usize];
                let Some(rule) = self.rules.get_optional(kind) else {
                    continue;
                };
                let state = &policies.states[state_index];
                for role in 0..game.n_roles() {
                    let mixed = rule
                        .mixed(state, role, game)
                        .filter(|values| values.iter().all(|value| value.is_finite()));
                    if !reports.set(agent_index, role, mixed) {
                        return Err(SocsimError::Mechanism(format!(
                            "mixed-strategy report index is invalid for agent {} role {role}",
                            agent.0
                        )));
                    }
                }
            }
        }
        Ok(())
    }
}

impl<G: StageGame, X: ExperimentState> Mechanism<GameWorld<G, X>> for RuleDriver<G, X> {
    fn name(&self) -> &str {
        "game_decide"
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
