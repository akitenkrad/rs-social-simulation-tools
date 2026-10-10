//! Roth--Erev reinforcement learning for finite games.

use rand::RngCore;
use serde::{Deserialize, Serialize};

use super::{sample_probabilities, LearnError};
use crate::FiniteGame;

/// One learner's Roth--Erev propensities and fixed update parameters.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RothErevState {
    propensities: Vec<f64>,
    phi: f64,
    epsilon: f64,
}

impl RothErevState {
    /// Validates the game and constructs equal positive propensities.
    pub fn new<G: FiniteGame>(
        game: &G,
        role: usize,
        initial_propensity: f64,
        phi: f64,
        epsilon: f64,
    ) -> Result<Self, LearnError> {
        validate_parameter("initial_propensity", initial_propensity, |value| {
            value > 0.0
        })?;
        validate_parameter("phi", phi, |value| (0.0..1.0).contains(&value))?;
        validate_parameter("epsilon", epsilon, |value| (0.0..1.0).contains(&value))?;
        validate_role(game, role)?;
        validate_positive_payoffs(game)?;
        Ok(Self {
            propensities: vec![initial_propensity; game.n_actions(role)],
            phi,
            epsilon,
        })
    }

    /// Returns the current unnormalized propensities.
    pub fn propensities(&self) -> &[f64] {
        &self.propensities
    }

    /// Returns action probabilities proportional to current propensities.
    pub fn selection_probabilities(&self) -> Result<Vec<f64>, LearnError> {
        let maximum = self
            .propensities
            .iter()
            .copied()
            .fold(f64::NEG_INFINITY, f64::max);
        if !maximum.is_finite() || maximum <= 0.0 {
            return Err(LearnError::EmptyProbabilityMass);
        }
        let scaled: Vec<_> = self
            .propensities
            .iter()
            .map(|value| value / maximum)
            .collect();
        let total: f64 = scaled.iter().sum();
        if !total.is_finite() || total <= 0.0 {
            return Err(LearnError::EmptyProbabilityMass);
        }
        Ok(scaled.into_iter().map(|value| value / total).collect())
    }

    /// Samples an action from the current propensity-proportional distribution.
    pub fn choose_action(&self, rng: &mut dyn RngCore) -> Result<usize, LearnError> {
        sample_probabilities(&self.selection_probabilities()?, rng)
    }

    /// Reads this role's chosen action and payoff from a valid game profile,
    /// then applies the Roth--Erev update.
    pub fn update_from_profile<G: FiniteGame>(
        &mut self,
        game: &G,
        role: usize,
        profile: &[usize],
    ) -> Result<(), LearnError> {
        validate_role(game, role)?;
        if profile.len() != game.n_roles() {
            return Err(LearnError::LengthMismatch {
                what: "action profile",
                expected: game.n_roles(),
                actual: profile.len(),
            });
        }
        for (profile_role, &action) in profile.iter().enumerate() {
            let n_actions = game.n_actions(profile_role);
            if action >= n_actions {
                return Err(LearnError::InvalidAction { action, n_actions });
            }
        }
        self.update(profile[role], game.payoff(role, profile))
    }

    /// Applies `q_j <- (1 - phi) q_j + E_j` for a realized positive payoff.
    ///
    /// For one available action, the entire payoff reinforces that action
    /// because there is no alternative action to receive experimentation mass.
    pub fn update(&mut self, chosen_action: usize, payoff: f64) -> Result<(), LearnError> {
        let n_actions = self.propensities.len();
        if chosen_action >= n_actions {
            return Err(LearnError::InvalidAction {
                action: chosen_action,
                n_actions,
            });
        }
        if !payoff.is_finite() || payoff <= 0.0 {
            return Err(LearnError::InvalidRealizedPayoff { payoff });
        }

        let other_experience = if n_actions > 1 {
            self.epsilon * payoff / (n_actions - 1) as f64
        } else {
            0.0
        };
        let mut next = Vec::with_capacity(n_actions);
        for (action, &propensity) in self.propensities.iter().enumerate() {
            let experience = if n_actions == 1 {
                payoff
            } else if action == chosen_action {
                (1.0 - self.epsilon) * payoff
            } else {
                other_experience
            };
            let value = (1.0 - self.phi) * propensity + experience;
            if !value.is_finite() || value <= 0.0 {
                return Err(LearnError::NonFiniteCalculation);
            }
            next.push(value);
        }
        self.propensities = next;
        Ok(())
    }
}

fn validate_parameter(
    parameter: &'static str,
    value: f64,
    predicate: impl FnOnce(f64) -> bool,
) -> Result<(), LearnError> {
    if !value.is_finite() || !predicate(value) {
        return Err(LearnError::InvalidParameter { parameter, value });
    }
    Ok(())
}

fn validate_role(game: &impl FiniteGame, role: usize) -> Result<(), LearnError> {
    let n_roles = game.n_roles();
    if role >= n_roles {
        return Err(LearnError::InvalidRole { role, n_roles });
    }
    Ok(())
}

fn validate_positive_payoffs(game: &impl FiniteGame) -> Result<(), LearnError> {
    let n_roles = game.n_roles();
    let mut profile = vec![0; n_roles];
    loop {
        for role in 0..n_roles {
            let payoff = game.payoff(role, &profile);
            if !payoff.is_finite() || payoff <= 0.0 {
                return Err(LearnError::NonPositivePayoff {
                    role,
                    profile,
                    payoff,
                });
            }
        }

        let mut position = n_roles;
        loop {
            if position == 0 {
                return Ok(());
            }
            position -= 1;
            profile[position] += 1;
            if profile[position] < game.n_actions(position) {
                break;
            }
            profile[position] = 0;
        }
    }
}
