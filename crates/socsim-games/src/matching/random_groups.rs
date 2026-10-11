use rand::seq::SliceRandom;
use socsim_core::SimRng;

use crate::{
    ArenaWriter, Credit, EncounterBounds, ExperimentState, GameError, MatchContext, Matching,
    MatchingState, StageGame,
};

/// Shuffles the agents every step and splits them into groups of `k`.
///
/// The game must have exactly `k` roles; each group is one encounter seated in
/// shuffled order, and every member receives its payoff. When the agent count
/// is not a multiple of `k`, the leftover agents sit out that step.
#[derive(Clone, Copy, Debug)]
pub struct RandomGroups {
    k: usize,
}

impl RandomGroups {
    /// Constructs the matching for groups of `k` agents.
    ///
    /// Returns [`GameError::InvalidMatching`] when `k < 2`.
    pub fn new(k: usize) -> Result<Self, GameError> {
        if k < 2 {
            return Err(GameError::InvalidMatching(
                "RandomGroups group size must be at least two".to_owned(),
            ));
        }
        Ok(Self { k })
    }
}

impl<G: StageGame, X: ExperimentState> Matching<G, X> for RandomGroups {
    fn name(&self) -> &str {
        "random_groups"
    }

    fn check(&self, ctx: &MatchContext<'_, G, X>) -> Result<(), GameError> {
        if ctx.game().n_roles() != self.k {
            return Err(GameError::InvalidMatching(format!(
                "RandomGroups group size {} does not match {} game roles",
                self.k,
                ctx.game().n_roles()
            )));
        }
        Ok(())
    }

    fn supports_roles(&self, n_roles: usize) -> bool {
        n_roles == self.k
    }

    fn encounter_bounds(&self, ctx: &MatchContext<'_, G, X>) -> Option<EncounterBounds> {
        Some(if ctx.ids().len().is_multiple_of(self.k) {
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
        for group in ids.chunks_exact(self.k) {
            out.push(group, Credit::All)?;
        }
        Ok(())
    }
}
