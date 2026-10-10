use std::marker::PhantomData;

use socsim_core::{Mechanism, Phase, SocsimError, StepContext};

use crate::{
    Credit, DecisionScope, EncounterId, EncounterRef, ExperimentState, GameWorld, OutcomeContext,
    OutcomeRule, PayoffAggregation, StageGame, StepRecord,
};

pub(crate) struct PlayMechanism<G, X, O>
where
    G: StageGame,
    X: ExperimentState,
    O: OutcomeRule<G, X>,
{
    outcome: O,
    aggregation: PayoffAggregation,
    decision_scope: DecisionScope,
    marker: PhantomData<fn() -> (G, X)>,
}

impl<G, X, O> PlayMechanism<G, X, O>
where
    G: StageGame,
    X: ExperimentState,
    O: OutcomeRule<G, X>,
{
    pub(crate) fn new(
        outcome: O,
        aggregation: PayoffAggregation,
        decision_scope: DecisionScope,
    ) -> Self {
        Self {
            outcome,
            aggregation,
            decision_scope,
            marker: PhantomData,
        }
    }
}

impl<G, X, O> Mechanism<GameWorld<G, X>> for PlayMechanism<G, X, O>
where
    G: StageGame,
    X: ExperimentState,
    O: OutcomeRule<G, X> + 'static,
{
    fn name(&self) -> &str {
        "game_play"
    }

    fn phases(&self) -> &'static [Phase] {
        &[Phase::Interaction]
    }

    fn apply(
        &mut self,
        _phase: Phase,
        step: &mut StepContext<'_, GameWorld<G, X>>,
    ) -> socsim_core::Result<()> {
        let world = &mut *step.world;
        for encounter_index in 0..world.plays.n_encounters() {
            let start = encounter_index * world.plays.n_roles;
            let end = start + world.plays.n_roles;
            let actions = world.plays.actions[start..end]
                .iter()
                .cloned()
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| SocsimError::Mechanism("unfilled action seat".to_owned()))?;
            let credit = world.plays.credit[encounter_index];
            if credit != Credit::All {
                return Err(SocsimError::Mechanism(
                    "Credit::Center is not supported yet".to_owned(),
                ));
            }
            let seats = &world.plays.seats[start..end];
            let encounter = EncounterRef {
                id: EncounterId(encounter_index as u32),
                seats,
                credit,
            };
            let context = OutcomeContext {
                t: world.clock.t(),
                game: &world.game,
                x: &world.x,
                held: &world.held,
                topology: &world.topology,
            };
            let mut payoffs = vec![0.0; world.plays.n_roles];
            self.outcome
                .outcome(&context, encounter, &actions, &mut payoffs)?;
            world.plays.payoffs[start..end].copy_from_slice(&payoffs);
            for (role, payoff) in payoffs.into_iter().enumerate() {
                let agent = seats[role];
                let index = agent.0 as usize;
                world.payoff[index] += payoff;
                world.n_played[index] += 1;
            }
        }

        if let Some(pair_history) = &mut world.pair_history {
            for encounter_index in 0..world.plays.n_encounters() {
                let start = encounter_index * world.plays.n_roles;
                let end = start + world.plays.n_roles;
                let seats = &world.plays.seats[start..end];
                if seats.len() != 2 {
                    continue;
                }
                let actions = world.plays.actions[start..end]
                    .iter()
                    .cloned()
                    .collect::<Option<Vec<_>>>()
                    .ok_or_else(|| SocsimError::Mechanism("unfilled action seat".to_owned()))?;
                pair_history.push(
                    world.clock.t(),
                    seats[0],
                    seats[1],
                    actions[0].clone(),
                    actions[1].clone(),
                );
            }
        }

        if self.aggregation == PayoffAggregation::Mean {
            for (payoff, &count) in world.payoff.iter_mut().zip(&world.n_played) {
                if count != 0 {
                    *payoff /= f64::from(count);
                }
            }
        }

        let records: Vec<_> = world
            .ids
            .iter()
            .map(|&agent| {
                let index = agent.0 as usize;
                let action = (self.decision_scope == DecisionScope::PerStep)
                    .then(|| {
                        world
                            .plays
                            .seats
                            .iter()
                            .position(|seat| *seat == agent)
                            .and_then(|seat| world.plays.actions[seat].clone())
                    })
                    .flatten();
                StepRecord {
                    action,
                    payoff: world.payoff[index],
                    n_played: world.n_played[index],
                }
            })
            .collect();
        world.step_history.push_step(&records);
        Ok(())
    }
}
