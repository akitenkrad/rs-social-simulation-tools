use std::marker::PhantomData;

use socsim_core::{Mechanism, Phase, SocsimError, StepContext};

use crate::{
    AggregateReduce, Credit, DecisionScope, EncounterId, EncounterRef, ExperimentState, GameWorld,
    OutcomeContext, OutcomeRule, PayoffAggregation, StageGame, StepRecord,
};

type AggregateFn<G> = fn(&G, &[<G as StageGame>::Action], &mut [f64]);

struct AggregateCollector<G: StageGame> {
    reductions: Vec<AggregateReduce>,
    aggregate_into: AggregateFn<G>,
}

pub(crate) struct PlayMechanism<G, X, O>
where
    G: StageGame,
    X: ExperimentState,
    O: OutcomeRule<G, X>,
{
    outcome: O,
    aggregation: PayoffAggregation,
    decision_scope: DecisionScope,
    aggregate: Option<AggregateCollector<G>>,
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
            aggregate: None,
            marker: PhantomData,
        }
    }

    pub(crate) fn with_aggregates(
        outcome: O,
        aggregation: PayoffAggregation,
        decision_scope: DecisionScope,
        reductions: Vec<AggregateReduce>,
        aggregate_into: AggregateFn<G>,
    ) -> Self {
        Self {
            outcome,
            aggregation,
            decision_scope,
            aggregate: Some(AggregateCollector {
                reductions,
                aggregate_into,
            }),
            marker: PhantomData,
        }
    }

    fn record_aggregates(&self, world: &mut GameWorld<G, X>) -> socsim_core::Result<()> {
        let Some(aggregate) = &self.aggregate else {
            return Ok(());
        };
        let mut sums = vec![0.0; aggregate.reductions.len()];
        let encounter_count = world.plays.n_encounters();
        for encounter_index in 0..encounter_count {
            let start = encounter_index * world.plays.n_roles;
            let end = start + world.plays.n_roles;
            let actions = world.plays.actions[start..end]
                .iter()
                .cloned()
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| SocsimError::Mechanism("unfilled action seat".to_owned()))?;
            let mut values = vec![f64::NAN; aggregate.reductions.len()];
            (aggregate.aggregate_into)(&world.game, &actions, &mut values);
            for (index, value) in values.into_iter().enumerate() {
                if !value.is_finite() {
                    return Err(SocsimError::Mechanism(format!(
                        "non-finite aggregate value at index {index} for encounter {encounter_index}"
                    )));
                }
                sums[index] += value;
                if !sums[index].is_finite() {
                    return Err(SocsimError::Mechanism(format!(
                        "non-finite aggregate value after reduction at index {index}"
                    )));
                }
            }
        }
        let values = aggregate
            .reductions
            .iter()
            .zip(sums)
            .map(|(reduction, sum)| match reduction {
                AggregateReduce::Sum => Some(sum),
                AggregateReduce::Mean => {
                    (encounter_count != 0).then(|| sum / encounter_count as f64)
                }
            })
            .collect();
        world
            .aggregate
            .as_mut()
            .ok_or_else(|| SocsimError::Mechanism("aggregate table is not initialized".to_owned()))?
            .push(world.clock.t(), values);
        Ok(())
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
        self.record_aggregates(world)?;
        Ok(())
    }
}
