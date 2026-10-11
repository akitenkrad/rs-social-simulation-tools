//! Simultaneous fictitious play for two-player finite games.

use rand::RngCore;
use serde::{Deserialize, Serialize};

use super::{opponent_role, payoff_against, resolve_tie, LearnError, TieBreak};
use crate::FiniteGame;

/// One learner's belief about an opponent's actions.
///
/// `counts` contains both the fixed initial pseudo-counts and observations.
/// `empirical_counts` contains observations only.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FictitiousBelief {
    counts: Vec<f64>,
    empirical_counts: Vec<u64>,
    tie_break: TieBreak,
}

impl FictitiousBelief {
    /// Constructs a belief with fixed initial pseudo-counts and tie policy.
    pub fn new<G: FiniteGame>(
        game: &G,
        role: usize,
        initial_counts: Vec<f64>,
        tie_break: TieBreak,
    ) -> Result<Self, LearnError> {
        let opponent = opponent_role(game, role)?;
        let expected = game.n_actions(opponent);
        if initial_counts.len() != expected {
            return Err(LearnError::LengthMismatch {
                what: "initial counts",
                expected,
                actual: initial_counts.len(),
            });
        }
        for (index, &value) in initial_counts.iter().enumerate() {
            if !value.is_finite() || value < 0.0 {
                return Err(LearnError::InvalidInitialCount { index, value });
            }
        }
        Ok(Self {
            counts: initial_counts,
            empirical_counts: vec![0; expected],
            tie_break,
        })
    }

    /// Constructs a belief with zero pseudo-counts and lowest-index tie breaks.
    pub fn with_defaults<G: FiniteGame>(game: &G, role: usize) -> Result<Self, LearnError> {
        let opponent = opponent_role(game, role)?;
        Self::new(
            game,
            role,
            vec![0.0; game.n_actions(opponent)],
            TieBreak::LowestIndex,
        )
    }

    /// Returns unnormalized belief counts, including initial pseudo-counts.
    pub fn counts(&self) -> &[f64] {
        &self.counts
    }

    /// Returns observed action frequencies, excluding initial pseudo-counts.
    pub fn empirical_counts(&self) -> &[u64] {
        &self.empirical_counts
    }

    /// Returns the construction-time tie policy.
    pub fn tie_break(&self) -> TieBreak {
        self.tie_break
    }

    /// Computes a best response to the unnormalized belief counts.
    ///
    /// Ties are detected with exact `f64` equality.
    pub fn best_response<G: FiniteGame>(
        &self,
        game: &G,
        role: usize,
        rng: &mut dyn RngCore,
    ) -> Result<usize, LearnError> {
        let opponent = opponent_role(game, role)?;
        let expected = game.n_actions(opponent);
        if self.counts.len() != expected {
            return Err(LearnError::LengthMismatch {
                what: "belief counts",
                expected,
                actual: self.counts.len(),
            });
        }

        let mut best_value = f64::NEG_INFINITY;
        let mut best_actions = Vec::new();
        for action in 0..game.n_actions(role) {
            let mut value = 0.0;
            for (opponent_action, &count) in self.counts.iter().enumerate() {
                if count == 0.0 {
                    continue;
                }
                value += count * payoff_against(game, role, action, opponent_action);
            }
            if !value.is_finite() {
                return Err(LearnError::NonFiniteCalculation);
            }
            if value > best_value {
                best_value = value;
                best_actions.clear();
                best_actions.push(action);
            } else if value == best_value {
                best_actions.push(action);
            }
        }
        resolve_tie(&best_actions, self.tie_break, rng)
    }

    /// Returns all best responses to the unnormalized belief counts in action
    /// index order.
    ///
    /// Ties are detected with exact `f64` equality.
    pub fn best_responses<G: FiniteGame>(
        &self,
        game: &G,
        role: usize,
    ) -> Result<Vec<usize>, LearnError> {
        let opponent = opponent_role(game, role)?;
        let expected = game.n_actions(opponent);
        if self.counts.len() != expected {
            return Err(LearnError::LengthMismatch {
                what: "belief counts",
                expected,
                actual: self.counts.len(),
            });
        }

        let mut best_value = f64::NEG_INFINITY;
        let mut best_actions = Vec::new();
        for action in 0..game.n_actions(role) {
            let mut value = 0.0;
            for (opponent_action, &count) in self.counts.iter().enumerate() {
                if count == 0.0 {
                    continue;
                }
                value += count * payoff_against(game, role, action, opponent_action);
            }
            if !value.is_finite() {
                return Err(LearnError::NonFiniteCalculation);
            }
            if value > best_value {
                best_value = value;
                best_actions.clear();
                best_actions.push(action);
            } else if value == best_value {
                best_actions.push(action);
            }
        }
        if best_actions.is_empty() {
            return Err(LearnError::EmptyProbabilityMass);
        }
        Ok(best_actions)
    }

    /// Adds one opponent action to belief and empirical counts.
    pub fn observe(&mut self, opponent_action: usize) -> Result<(), LearnError> {
        if opponent_action >= self.counts.len() {
            return Err(LearnError::InvalidAction {
                action: opponent_action,
                n_actions: self.counts.len(),
            });
        }
        let next_empirical = self.empirical_counts[opponent_action]
            .checked_add(1)
            .ok_or(LearnError::CountOverflow)?;
        let next_count = self.counts[opponent_action] + 1.0;
        if !next_count.is_finite() {
            return Err(LearnError::CountOverflow);
        }
        self.counts[opponent_action] = next_count;
        self.empirical_counts[opponent_action] = next_empirical;
        Ok(())
    }
}

/// Performs one simultaneous fictitious-play step.
///
/// Both best responses are computed from counts through the previous step.
/// Only after both actions have been selected are the current observations
/// added. No alternating-update operation is provided.
pub fn simultaneous_step<G: FiniteGame>(
    game: &G,
    role_zero: &mut FictitiousBelief,
    role_one: &mut FictitiousBelief,
    rng: &mut dyn RngCore,
) -> Result<(usize, usize), LearnError> {
    let action_zero = role_zero.best_response(game, 0, rng)?;
    let action_one = role_one.best_response(game, 1, rng)?;
    role_zero.observe(action_one)?;
    role_one.observe(action_zero)?;
    Ok((action_zero, action_one))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MatrixGame;

    #[test]
    fn best_responses_returns_every_exact_tie_in_action_order() {
        let game = MatrixGame::bimatrix(
            &[vec![1.0, 1.0], vec![1.0, 1.0]],
            &[vec![0.0, 0.0], vec![0.0, 0.0]],
        )
        .unwrap();
        let belief = FictitiousBelief::new(&game, 0, vec![1.0, 1.0], TieBreak::Random).unwrap();

        assert_eq!(belief.best_responses(&game, 0).unwrap(), vec![0, 1]);
    }
}
