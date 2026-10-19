use socsim_core::{AgentId, SimRng};

use crate::{
    ArenaWriter, Credit, ExperimentState, GameError, MatchContext, Matching, MatchingState,
    StageGame,
};

#[derive(Clone, Debug)]
/// Fixed disjoint pairs for a two-role game.
///
/// Assembly rejects unknown agents, repeated agents, and games with any role
/// count other than two. Agents omitted from the pairs sit out every step.
pub struct FixedPlayers {
    pairs: Vec<[AgentId; 2]>,
}

impl FixedPlayers {
    /// Creates fixed pairs from explicit [`AgentId`] seats.
    pub fn new(pairs: Vec<[AgentId; 2]>) -> Self {
        Self { pairs }
    }

    /// Creates fixed pairs from numeric agent IDs.
    pub fn pairs(pairs: &[(u64, u64)]) -> Self {
        Self::new(
            pairs
                .iter()
                .map(|&(left, right)| [AgentId(left), AgentId(right)])
                .collect(),
        )
    }
}

impl<G: StageGame, X: ExperimentState> Matching<G, X> for FixedPlayers {
    fn name(&self) -> &str {
        "fixed_players"
    }

    fn check(&self, ctx: &MatchContext<'_, G, X>) -> Result<(), GameError> {
        if ctx.game().n_roles() != 2 {
            return Err(GameError::InvalidMatching(
                "FixedPlayers requires exactly two roles".to_owned(),
            ));
        }
        let mut seen = vec![false; ctx.ids().len()];
        for pair in &self.pairs {
            for agent in pair {
                let Some(slot) = usize::try_from(agent.0)
                    .ok()
                    .and_then(|index| seen.get_mut(index))
                else {
                    return Err(GameError::InvalidMatching(format!(
                        "unknown agent {}",
                        agent.0
                    )));
                };
                if *slot {
                    return Err(GameError::InvalidMatching(format!(
                        "agent {} appears more than once",
                        agent.0
                    )));
                }
                *slot = true;
            }
        }
        Ok(())
    }

    fn encounters_into(
        &self,
        _ctx: &MatchContext<'_, G, X>,
        _state: &mut MatchingState,
        _rng: &mut SimRng,
        out: &mut ArenaWriter<'_, G::Action>,
    ) -> socsim_core::Result<()> {
        for pair in &self.pairs {
            out.push(pair, Credit::All)?;
        }
        Ok(())
    }
}
