use rand::seq::SliceRandom;
use socsim_core::SimRng;

use crate::{
    ArenaWriter, Credit, EncounterBounds, ExperimentState, GameError, MatchContext, Matching,
    MatchingState, StageGame,
};

/// Shuffles the agents every step and pairs them off.
///
/// Requires a two-role game. Each pair is one encounter, seated in shuffled
/// order, and both agents receive their payoff. With an odd number of agents,
/// one randomly chosen agent sits out each step, so the encounter bounds are
/// `0..=1` instead of `1..=1`.
#[derive(Clone, Copy, Debug, Default)]
pub struct RandomPairs;

impl RandomPairs {
    /// Constructs the matching.
    pub fn new() -> Self {
        Self
    }
}

impl<G: StageGame, X: ExperimentState> Matching<G, X> for RandomPairs {
    fn name(&self) -> &str {
        "random_pairs"
    }

    fn check(&self, ctx: &MatchContext<'_, G, X>) -> Result<(), GameError> {
        if ctx.game().n_roles() != 2 {
            return Err(GameError::InvalidMatching(
                "RandomPairs requires exactly two roles".to_owned(),
            ));
        }
        Ok(())
    }

    fn encounter_bounds(&self, ctx: &MatchContext<'_, G, X>) -> Option<EncounterBounds> {
        Some(if ctx.ids().len().is_multiple_of(2) {
            EncounterBounds { min: 1, max: 1 }
        } else {
            EncounterBounds { min: 0, max: 1 }
        })
    }

    fn encounters_into(
        &self,
        ctx: &MatchContext<'_, G, X>,
        _state: &mut MatchingState,
        rng: &mut SimRng,
        out: &mut ArenaWriter<'_, G::Action>,
    ) -> socsim_core::Result<()> {
        let mut ids = ctx.ids().to_vec();
        ids.shuffle(rng);
        for pair in ids.as_chunks::<2>().0 {
            out.push(pair, Credit::All)?;
        }
        Ok(())
    }
}
