use std::marker::PhantomData;

use socsim_core::{Mechanism, Phase, SocsimError, StepContext};

use crate::outcome::kernel::{
    aggregate_mean, fill_actions, for_each_implicit, local_payoff_counting, local_payoff_pairwise,
    play_counting, play_implicit_counting, play_implicit_pairwise, play_pairwise,
    LocalEncounterIds, PayoffAccum,
};
use crate::{
    AggregateReduce, AnonymousGame, DecisionScope, ExperimentState, GameWorld, ImplicitPattern,
    OutcomeContext, OutcomeRule, PayoffAggregation, StageGame, StepRecord,
};

pub(crate) type AggregateFn<G> = fn(&G, &[<G as StageGame>::Action], &mut [f64]);

pub(crate) struct AggregateCollector<G: StageGame> {
    reductions: Vec<AggregateReduce>,
    aggregate_into: AggregateFn<G>,
}

impl<G: StageGame> AggregateCollector<G> {
    pub(crate) fn new(reductions: Vec<AggregateReduce>, aggregate_into: AggregateFn<G>) -> Self {
        Self {
            reductions,
            aggregate_into,
        }
    }

    fn record<X: ExperimentState>(
        &self,
        world: &mut GameWorld<G, X>,
        implicit: Option<ImplicitPattern>,
    ) -> socsim_core::Result<()> {
        let mut sums = vec![0.0; self.reductions.len()];
        let mut encounter_count = 0_usize;
        let mut values = vec![f64::NAN; self.reductions.len()];
        let mut add_profile = |profile: &[G::Action]| -> socsim_core::Result<()> {
            values.fill(f64::NAN);
            (self.aggregate_into)(&world.game, profile, &mut values);
            for (index, &value) in values.iter().enumerate() {
                if !value.is_finite() {
                    return Err(SocsimError::Mechanism(format!(
                        "non-finite aggregate value at index {index} for encounter {encounter_count}"
                    )));
                }
                sums[index] += value;
                if !sums[index].is_finite() {
                    return Err(SocsimError::Mechanism(format!(
                        "non-finite aggregate value after reduction at index {index}"
                    )));
                }
            }
            encounter_count += 1;
            Ok(())
        };
        if let Some(pattern) = implicit {
            for_each_implicit(
                pattern,
                &world.topology,
                &world.ids,
                &world.held,
                |_id, _seats, _credit, actions| add_profile(actions),
            )?;
        } else {
            let mut actions = Vec::with_capacity(world.plays.n_roles);
            for encounter_index in 0..world.plays.n_encounters() {
                let start = encounter_index * world.plays.n_roles;
                let end = start + world.plays.n_roles;
                fill_actions(&mut actions, &world.plays.actions[start..end])?;
                add_profile(&actions)?;
            }
        }
        let values = self
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

pub(crate) trait KernelRunner<G: StageGame, X: ExperimentState>: Send + Sync {
    fn materialized(
        &self,
        context: &OutcomeContext<'_, G, X>,
        arena: &mut crate::PlayArena<G::Action>,
        accum: &mut PayoffAccum<'_>,
    ) -> socsim_core::Result<()>;

    fn implicit(
        &self,
        pattern: ImplicitPattern,
        context: &OutcomeContext<'_, G, X>,
        ids: &[socsim_core::AgentId],
        accum: &mut PayoffAccum<'_>,
    ) -> socsim_core::Result<()>;

    #[allow(dead_code)]
    fn local_payoff(
        &self,
        _pattern: ImplicitPattern,
        _context: &OutcomeContext<'_, G, X>,
        _ids: &[socsim_core::AgentId],
        _encounter_ids: &LocalEncounterIds,
        _index: usize,
        _aggregation: PayoffAggregation,
    ) -> Option<socsim_core::Result<(f64, u32)>> {
        None
    }

    #[allow(dead_code)]
    fn whole_population_payoffs(
        &self,
        _context: &OutcomeContext<'_, G, X>,
        _ids: &[socsim_core::AgentId],
        _aggregation: PayoffAggregation,
        _payoff: &mut [f64],
        _n_played: &mut [u32],
    ) -> Option<socsim_core::Result<()>> {
        None
    }
}

#[allow(dead_code)]
fn run_whole_population<G, X, R>(
    runner: &R,
    context: &OutcomeContext<'_, G, X>,
    ids: &[socsim_core::AgentId],
    aggregation: PayoffAggregation,
    payoff: &mut [f64],
    n_played: &mut [u32],
) -> socsim_core::Result<()>
where
    G: StageGame,
    X: ExperimentState,
    R: KernelRunner<G, X> + ?Sized,
{
    payoff.fill(0.0);
    n_played.fill(0);
    runner.implicit(
        ImplicitPattern::WholePopulation,
        context,
        ids,
        &mut PayoffAccum::new(payoff, n_played, None),
    )?;
    aggregate_mean(aggregation, payoff, n_played);
    Ok(())
}

pub(crate) struct PairwiseRunner<O>(pub(crate) O);

impl<G, X, O> KernelRunner<G, X> for PairwiseRunner<O>
where
    G: StageGame,
    X: ExperimentState,
    O: OutcomeRule<G, X>,
{
    fn materialized(
        &self,
        context: &OutcomeContext<'_, G, X>,
        arena: &mut crate::PlayArena<G::Action>,
        accum: &mut PayoffAccum<'_>,
    ) -> socsim_core::Result<()> {
        play_pairwise(context, &self.0, arena, accum)
    }

    fn implicit(
        &self,
        pattern: ImplicitPattern,
        context: &OutcomeContext<'_, G, X>,
        ids: &[socsim_core::AgentId],
        accum: &mut PayoffAccum<'_>,
    ) -> socsim_core::Result<()> {
        play_implicit_pairwise(pattern, context, ids, &self.0, accum)
    }

    fn local_payoff(
        &self,
        pattern: ImplicitPattern,
        context: &OutcomeContext<'_, G, X>,
        ids: &[socsim_core::AgentId],
        encounter_ids: &LocalEncounterIds,
        index: usize,
        aggregation: PayoffAggregation,
    ) -> Option<socsim_core::Result<(f64, u32)>> {
        Some(local_payoff_pairwise(
            pattern,
            context,
            ids,
            &self.0,
            encounter_ids,
            index,
            aggregation,
        ))
    }

    fn whole_population_payoffs(
        &self,
        context: &OutcomeContext<'_, G, X>,
        ids: &[socsim_core::AgentId],
        aggregation: PayoffAggregation,
        payoff: &mut [f64],
        n_played: &mut [u32],
    ) -> Option<socsim_core::Result<()>> {
        Some(run_whole_population(
            self,
            context,
            ids,
            aggregation,
            payoff,
            n_played,
        ))
    }
}

pub(crate) struct CountingRunner;

impl<G, X> KernelRunner<G, X> for CountingRunner
where
    G: AnonymousGame,
    X: ExperimentState,
{
    fn materialized(
        &self,
        context: &OutcomeContext<'_, G, X>,
        arena: &mut crate::PlayArena<usize>,
        accum: &mut PayoffAccum<'_>,
    ) -> socsim_core::Result<()> {
        play_counting(context.game(), arena, accum)
    }

    fn implicit(
        &self,
        pattern: ImplicitPattern,
        context: &OutcomeContext<'_, G, X>,
        ids: &[socsim_core::AgentId],
        accum: &mut PayoffAccum<'_>,
    ) -> socsim_core::Result<()> {
        play_implicit_counting(
            pattern,
            context.game(),
            context.topology(),
            ids,
            context.held,
            accum,
        )
    }

    fn local_payoff(
        &self,
        pattern: ImplicitPattern,
        context: &OutcomeContext<'_, G, X>,
        ids: &[socsim_core::AgentId],
        encounter_ids: &LocalEncounterIds,
        index: usize,
        aggregation: PayoffAggregation,
    ) -> Option<socsim_core::Result<(f64, u32)>> {
        Some(local_payoff_counting(
            pattern,
            context.game(),
            context.topology(),
            ids,
            context.held,
            encounter_ids,
            index,
            aggregation,
        ))
    }

    fn whole_population_payoffs(
        &self,
        context: &OutcomeContext<'_, G, X>,
        ids: &[socsim_core::AgentId],
        aggregation: PayoffAggregation,
        payoff: &mut [f64],
        n_played: &mut [u32],
    ) -> Option<socsim_core::Result<()>> {
        Some(run_whole_population(
            self,
            context,
            ids,
            aggregation,
            payoff,
            n_played,
        ))
    }
}

pub(crate) struct PlayMechanism<G, X>
where
    G: StageGame,
    X: ExperimentState,
{
    runner: Box<dyn KernelRunner<G, X>>,
    aggregation: PayoffAggregation,
    decision_scope: DecisionScope,
    implicit: Option<ImplicitPattern>,
    aggregate: Option<AggregateCollector<G>>,
    marker: PhantomData<fn() -> (G, X)>,
}

impl<G, X> PlayMechanism<G, X>
where
    G: StageGame,
    X: ExperimentState,
{
    pub(crate) fn new(
        runner: Box<dyn KernelRunner<G, X>>,
        aggregation: PayoffAggregation,
        decision_scope: DecisionScope,
        implicit: Option<ImplicitPattern>,
        aggregate: Option<AggregateCollector<G>>,
    ) -> Self {
        Self {
            runner,
            aggregation,
            decision_scope,
            implicit,
            aggregate,
            marker: PhantomData,
        }
    }
}

pub(crate) fn record_implicit_step<G, X>(
    world: &mut GameWorld<G, X>,
    runner: &dyn KernelRunner<G, X>,
    pattern: ImplicitPattern,
    aggregation: PayoffAggregation,
    aggregate: Option<&AggregateCollector<G>>,
) -> socsim_core::Result<()>
where
    G: StageGame,
    X: ExperimentState,
{
    world.played_counts = Some(crate::InternalPlayedActionCounts::new(
        world.game.n_roles(),
        Some(pattern),
    ));
    let context = OutcomeContext {
        t: world.clock.t(),
        game: &world.game,
        x: &world.x,
        held: &world.held,
        topology: &world.topology,
    };
    runner.implicit(
        pattern,
        &context,
        &world.ids,
        &mut PayoffAccum::new(
            &mut world.payoff,
            &mut world.n_played,
            world.played_counts.as_mut(),
        ),
    )?;
    aggregate_mean(aggregation, &mut world.payoff, &world.n_played);

    let mut records = world.step_history.next_slot_mut();
    for (row, &agent) in world.ids.iter().enumerate() {
        let index = agent.0 as usize;
        *records.get(row) = StepRecord {
            action: None,
            payoff: world.payoff[index],
            n_played: world.n_played[index],
        };
    }
    for (&agent, action) in world.ids.iter().zip(&world.held) {
        if world.n_played[agent.0 as usize] > 0 {
            records.get(agent.0 as usize).action = Some(action.clone());
        }
    }
    records.commit();
    if let Some(aggregate) = aggregate {
        aggregate.record(world, Some(pattern))?;
    }
    Ok(())
}

impl<G, X> Mechanism<GameWorld<G, X>> for PlayMechanism<G, X>
where
    G: StageGame,
    X: ExperimentState,
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
        if let Some(pattern) = self.implicit {
            return record_implicit_step(
                world,
                self.runner.as_ref(),
                pattern,
                self.aggregation,
                self.aggregate.as_ref(),
            );
        }
        let context = OutcomeContext {
            t: world.clock.t(),
            game: &world.game,
            x: &world.x,
            held: &world.held,
            topology: &world.topology,
        };
        let mut accum = PayoffAccum::new(
            &mut world.payoff,
            &mut world.n_played,
            world.played_counts.as_mut(),
        );
        self.runner
            .materialized(&context, &mut world.plays, &mut accum)?;

        if let Some(pair_history) = &mut world.pair_history {
            for encounter_index in 0..world.plays.n_encounters() {
                let start = encounter_index * world.plays.n_roles;
                let end = start + world.plays.n_roles;
                let seats = &world.plays.seats[start..end];
                if seats.len() != 2 {
                    continue;
                }
                let (Some(first), Some(second)) =
                    (&world.plays.actions[start], &world.plays.actions[start + 1])
                else {
                    return Err(SocsimError::Mechanism("unfilled action seat".to_owned()));
                };
                pair_history.push(
                    world.clock.t(),
                    seats[0],
                    seats[1],
                    first.clone(),
                    second.clone(),
                );
            }
        }

        aggregate_mean(self.aggregation, &mut world.payoff, &world.n_played);

        // The records of this step are written in place into the next history
        // slot, without a per-step buffer.
        let mut records = world.step_history.next_slot_mut();
        for (row, &agent) in world.ids.iter().enumerate() {
            let index = agent.0 as usize;
            *records.get(row) = StepRecord {
                action: None,
                payoff: world.payoff[index],
                n_played: world.n_played[index],
            };
        }
        if self.decision_scope == DecisionScope::PerStep {
            for (&agent, action) in world.plays.seats.iter().zip(&world.plays.actions) {
                let record = records.get(agent.0 as usize);
                if record.action.is_none() {
                    record.action.clone_from(action);
                }
            }
        }
        records.commit();
        if let Some(aggregate) = &self.aggregate {
            aggregate.record(world, None)?;
        }
        Ok(())
    }
}

pub(crate) fn pairwise_runner<G, X, O>(outcome: O) -> Box<dyn KernelRunner<G, X>>
where
    G: StageGame,
    X: ExperimentState,
    O: OutcomeRule<G, X> + 'static,
{
    Box::new(PairwiseRunner(outcome))
}

pub(crate) fn counting_runner<G, X>() -> Box<dyn KernelRunner<G, X>>
where
    G: AnonymousGame,
    X: ExperimentState,
{
    Box::new(CountingRunner)
}
