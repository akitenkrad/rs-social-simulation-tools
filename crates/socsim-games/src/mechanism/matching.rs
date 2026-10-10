use std::marker::PhantomData;

use socsim_core::{Mechanism, Phase, StepContext};

use crate::{ArenaWriter, ExperimentState, GameWorld, MatchContext, Matching, StageGame};

pub(crate) struct MatchMechanism<G, X, M>
where
    G: StageGame,
    X: ExperimentState,
    M: Matching<G, X>,
{
    matching: M,
    marker: PhantomData<fn() -> (G, X)>,
}

impl<G, X, M> MatchMechanism<G, X, M>
where
    G: StageGame,
    X: ExperimentState,
    M: Matching<G, X>,
{
    pub(crate) fn new(matching: M) -> Self {
        Self {
            matching,
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
            }
            Phase::Decision => {
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
