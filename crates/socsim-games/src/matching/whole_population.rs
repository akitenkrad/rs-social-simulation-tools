use socsim_core::SimRng;

use crate::{
    ArenaWriter, Credit, EncounterBounds, ExperimentState, GameError, ImplicitPattern,
    MatchContext, Matching, MatchingState, StageGame,
};

/// Places every agent in one encounter each step.
///
/// The game must have exactly one role per agent; agents are seated in
/// ascending ID order, so agent `i` plays role `i` when IDs are `0..n`. Every
/// agent receives its payoff once per step. Suited to anonymous games such as
/// the El Farol bar or the minority game.
#[derive(Clone, Copy, Debug, Default)]
pub struct WholePopulation;

impl WholePopulation {
    /// Constructs the matching.
    pub fn new() -> Self {
        Self
    }
}

impl<G: StageGame, X: ExperimentState> Matching<G, X> for WholePopulation {
    fn name(&self) -> &str {
        "whole_population"
    }

    fn check(&self, ctx: &MatchContext<'_, G, X>) -> Result<(), GameError> {
        if ctx.game().n_roles() != ctx.ids().len() {
            return Err(GameError::InvalidMatching(format!(
                "WholePopulation requires one role per agent, got {} roles and {} agents",
                ctx.game().n_roles(),
                ctx.ids().len()
            )));
        }
        Ok(())
    }

    fn supports_roles(&self, _n_roles: usize) -> bool {
        true
    }

    fn encounter_bounds(&self, _ctx: &MatchContext<'_, G, X>) -> Option<EncounterBounds> {
        Some(EncounterBounds { min: 1, max: 1 })
    }

    fn implicit(&self) -> Option<ImplicitPattern> {
        Some(ImplicitPattern::WholePopulation)
    }

    fn encounters_into(
        &self,
        ctx: &MatchContext<'_, G, X>,
        _state: &mut MatchingState,
        _rng: &mut SimRng,
        out: &mut ArenaWriter<'_, G::Action>,
    ) -> socsim_core::Result<()> {
        let mut ids = ctx.ids().to_vec();
        ids.sort_unstable();
        out.push(&ids, Credit::All)?;
        Ok(())
    }
}
