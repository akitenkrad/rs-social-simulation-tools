//! Logit action choice from recent or empirical opponent behavior.

use rand::RngCore;
use serde::{Deserialize, Serialize};

use super::{opponent_role, payoff_against, sample_probabilities, LearnError};
use crate::FiniteGame;

/// Opponent-payoff estimate used for logit choice.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EstimateMode {
    /// Use only the most recently observed opponent action.
    LastOpponent,
    /// Use all observed opponent actions at their empirical frequencies.
    #[default]
    EmpiricalFrequency,
}

/// One learner's opponent observations and fixed logit configuration.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LogitState {
    empirical_counts: Vec<u64>,
    last_opponent: Option<usize>,
    lambda: f64,
    mode: EstimateMode,
}

impl LogitState {
    /// Constructs an empty estimate for one role in a two-player game.
    pub fn new<G: FiniteGame>(
        game: &G,
        role: usize,
        lambda: f64,
        mode: EstimateMode,
    ) -> Result<Self, LearnError> {
        if !lambda.is_finite() || lambda < 0.0 {
            return Err(LearnError::InvalidParameter {
                parameter: "lambda",
                value: lambda,
            });
        }
        let opponent = opponent_role(game, role)?;
        Ok(Self {
            empirical_counts: vec![0; game.n_actions(opponent)],
            last_opponent: None,
            lambda,
            mode,
        })
    }

    /// Constructs an empirical-frequency estimate, the default mode.
    pub fn with_default_mode<G: FiniteGame>(
        game: &G,
        role: usize,
        lambda: f64,
    ) -> Result<Self, LearnError> {
        Self::new(game, role, lambda, EstimateMode::default())
    }

    /// Returns observed opponent frequencies.
    pub fn empirical_counts(&self) -> &[u64] {
        &self.empirical_counts
    }

    /// Returns the most recently observed opponent action.
    pub fn last_opponent(&self) -> Option<usize> {
        self.last_opponent
    }

    /// Records one opponent action.
    pub fn observe(&mut self, opponent_action: usize) -> Result<(), LearnError> {
        if opponent_action >= self.empirical_counts.len() {
            return Err(LearnError::InvalidAction {
                action: opponent_action,
                n_actions: self.empirical_counts.len(),
            });
        }
        let next = self.empirical_counts[opponent_action]
            .checked_add(1)
            .ok_or(LearnError::CountOverflow)?;
        self.empirical_counts[opponent_action] = next;
        self.last_opponent = Some(opponent_action);
        Ok(())
    }

    /// Estimates every own action's payoff under the configured mode.
    ///
    /// With no applicable observation, all estimates are zero; logit choice is
    /// therefore uniform.
    pub fn expected_payoffs<G: FiniteGame>(
        &self,
        game: &G,
        role: usize,
    ) -> Result<Vec<f64>, LearnError> {
        let opponent = opponent_role(game, role)?;
        let expected = game.n_actions(opponent);
        if self.empirical_counts.len() != expected {
            return Err(LearnError::LengthMismatch {
                what: "empirical counts",
                expected,
                actual: self.empirical_counts.len(),
            });
        }

        let mut estimates = vec![0.0; game.n_actions(role)];
        match self.mode {
            EstimateMode::LastOpponent => {
                if let Some(opponent_action) = self.last_opponent {
                    for (action, estimate) in estimates.iter_mut().enumerate() {
                        *estimate = payoff_against(game, role, action, opponent_action);
                    }
                }
            }
            EstimateMode::EmpiricalFrequency => {
                let total: u64 = self
                    .empirical_counts
                    .iter()
                    .try_fold(0_u64, |sum, &count| {
                        sum.checked_add(count).ok_or(LearnError::CountOverflow)
                    })?;
                if total > 0 {
                    for (action, estimate) in estimates.iter_mut().enumerate() {
                        let weighted_sum: f64 = self
                            .empirical_counts
                            .iter()
                            .enumerate()
                            .map(|(opponent_action, &count)| {
                                count as f64 * payoff_against(game, role, action, opponent_action)
                            })
                            .sum();
                        *estimate = weighted_sum / total as f64;
                    }
                }
            }
        }
        if estimates.iter().any(|value| !value.is_finite()) {
            return Err(LearnError::NonFiniteCalculation);
        }
        Ok(estimates)
    }

    /// Computes stable logit probabilities proportional to
    /// `exp(lambda * estimated_payoff)`.
    pub fn selection_probabilities<G: FiniteGame>(
        &self,
        game: &G,
        role: usize,
    ) -> Result<Vec<f64>, LearnError> {
        let estimates = self.expected_payoffs(game, role)?;
        if estimates.is_empty() {
            return Err(LearnError::EmptyProbabilityMass);
        }
        if self.lambda == 0.0 {
            return Ok(vec![1.0 / estimates.len() as f64; estimates.len()]);
        }
        let maximum = estimates.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let weights: Vec<_> = estimates
            .iter()
            .map(|estimate| (self.lambda * (estimate - maximum)).exp())
            .collect();
        let total: f64 = weights.iter().sum();
        if !total.is_finite() || total <= 0.0 {
            return Err(LearnError::EmptyProbabilityMass);
        }
        Ok(weights.into_iter().map(|weight| weight / total).collect())
    }

    /// Samples an action from the current stable logit probabilities.
    pub fn choose_action<G: FiniteGame>(
        &self,
        game: &G,
        role: usize,
        rng: &mut dyn RngCore,
    ) -> Result<usize, LearnError> {
        sample_probabilities(&self.selection_probabilities(game, role)?, rng)
    }
}
