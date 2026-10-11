use std::marker::PhantomData;

use socsim_core::{Mechanism, Phase, StepContext};

use crate::{
    ArenaWriter, DecisionScope, ExperimentState, GameWorld, MatchContext, Matching, StageGame,
};

pub(crate) struct MatchMechanism<G, X, M>
where
    G: StageGame,
    X: ExperimentState,
    M: Matching<G, X>,
{
    matching: M,
    decision_scope: DecisionScope,
    implicit: bool,
    marker: PhantomData<fn() -> (G, X)>,
}

impl<G, X, M> MatchMechanism<G, X, M>
where
    G: StageGame,
    X: ExperimentState,
    M: Matching<G, X>,
{
    pub(crate) fn new(matching: M, decision_scope: DecisionScope, implicit: bool) -> Self {
        Self {
            matching,
            decision_scope,
            implicit,
            marker: PhantomData,
        }
    }
}

impl<G, X, M> Mechanism<GameWorld<G, X>> for MatchMechanism<G, X, M>
where
    G: StageGame,
    X: ExperimentState,
    M: Matching<G, X> + 'static,
{
    fn name(&self) -> &str {
        "game_match"
    }

    fn phases(&self) -> &'static [Phase] {
        &[Phase::PreStep, Phase::Decision]
    }

    fn apply(
        &mut self,
        phase: Phase,
        step: &mut StepContext<'_, GameWorld<G, X>>,
    ) -> socsim_core::Result<()> {
        match phase {
            Phase::PreStep => {
                step.world.plays.clear();
                step.world.payoff.fill(0.0);
                step.world.n_played.fill(0);
                step.world.played_counts = None;
            }
            Phase::Decision => {
                if self.implicit {
                    return Ok(());
                }
                let GameWorld {
                    clock,
                    ids,
                    game,
                    topology,
                    held,
                    matching_state,
                    plays,
                    step_history,
                    x,
                    ..
                } = step.world;
                let context = MatchContext {
                    t: clock.t(),
                    decision_scope: self.decision_scope,
                    ids,
                    game,
                    topology,
                    held,
                    history: step_history,
                    x,
                };
                let mut writer = ArenaWriter::new(plays);
                self.matching
                    .encounters_into(&context, matching_state, step.rng, &mut writer)?;
            }
            _ => {}
        }
        Ok(())
    }
}
