use socsim_core::{Mechanism, Phase, SocsimError, StepContext};

use crate::{
    DecisionContext, DecisionUnit, ExperimentState, GameWorld, LearnContext, OnInvalidAction,
    PolicyTable, RuleBook, RuleError, StageGame,
};

pub struct RuleDriver<G: StageGame, X: ExperimentState> {
    rules: RuleBook<G, X>,
    on_invalid_action: OnInvalidAction<G::Action>,
}

impl<G: StageGame, X: ExperimentState> RuleDriver<G, X> {
    pub(crate) fn new(
        rules: RuleBook<G, X>,
        on_invalid_action: OnInvalidAction<G::Action>,
    ) -> Self {
        Self {
            rules,
            on_invalid_action,
        }
    }

    fn mechanism_error(error: impl std::fmt::Display) -> SocsimError {
        SocsimError::Mechanism(error.to_string())
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
            x,
            ..
        } = step.world;

        let mut seat_by_agent = vec![None; ids.len()];
        for (seat_index, &agent) in plays.seats.iter().enumerate() {
            let agent_index = usize::try_from(agent.0)
                .map_err(|_| SocsimError::Mechanism("agent index exceeds usize".to_owned()))?;
            let seat = seat_by_agent
                .get_mut(agent_index)
                .ok_or_else(|| SocsimError::Mechanism(format!("unknown agent {}", agent.0)))?;
            *seat = Some(seat_index);
        }

        for &agent in step.agent_order {
            let agent_index = usize::try_from(agent.0)
                .map_err(|_| SocsimError::Mechanism("agent index exceeds usize".to_owned()))?;
            let Some(seat_index) = seat_by_agent.get(agent_index).copied().flatten() else {
                continue;
            };
            let encounter_index = seat_index / plays.n_roles;
            let encounter_start = encounter_index * plays.n_roles;
            let encounter_end = encounter_start + plays.n_roles;
            let role = seat_index % plays.n_roles;
            let state_slot = *policies
                .slot
                .get(agent_index)
                .ok_or_else(|| SocsimError::Mechanism(format!("unknown agent {}", agent.0)))?;

            let action = if state_slot == PolicyTable::<X::PolicyState>::HELD {
                held[agent_index].clone()
            } else {
                let kind_id = policies.kind_of[state_slot as usize];
                let kind = &policies.kinds[kind_id.0 as usize];
                let Some(rule) = self.rules.get_optional(kind) else {
                    continue;
                };
                let opponents = plays.seats[encounter_start..encounter_end]
                    .iter()
                    .copied()
                    .filter(|seat| *seat != agent)
                    .collect();
                let state = &mut policies.states[state_slot as usize];
                let mut action = {
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
                        x,
                    };
                    rule.decide(state, &context, step.rng)
                        .map_err(Self::mechanism_error)?
                };
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
                            step.recorder.record_event(
                                clock.t(),
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
                held[agent_index] = action.clone();
                action
            };
            plays.actions[seat_index] = Some(action);
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
            x,
            ..
        } = step.world;

        for &agent in ids.iter() {
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
                record,
                ids,
                game,
                held,
                history: step_history,
                plays,
                x,
            };
            rule.learn_step(state, &context)
                .map_err(Self::mechanism_error)?;
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
