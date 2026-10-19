use thiserror::Error;

use crate::FiniteGame;

const DISTRIBUTION_TOLERANCE: f64 = 1e-9;

/// An error involving a mixed-strategy profile.
#[derive(Clone, Debug, PartialEq, Error)]
#[non_exhaustive]
pub enum MixedError {
    /// A probability vector is empty, non-finite, negative, or does not sum to one.
    #[error("values do not form a probability distribution")]
    NotADistribution,
    /// A role's probability vector has the wrong dimension.
    #[error("role {role} has dimension {actual}, expected {expected}")]
    Dimension {
        /// Zero-based role with the mismatched distribution.
        role: usize,
        /// Number of actions declared for that role.
        expected: usize,
        /// Number of supplied probabilities.
        actual: usize,
    },
    /// The mixed profile has the wrong number of roles.
    #[error("mixed profile has {actual} roles, expected {expected}")]
    RoleCount {
        /// Number of roles declared by the game.
        expected: usize,
        /// Number of supplied role strategies, or the minimum implied by a bad role index.
        actual: usize,
    },
}

/// A validated probability distribution over one role's finite actions.
#[derive(Clone, Debug, PartialEq)]
pub struct MixedStrategy(Vec<f64>);

impl MixedStrategy {
    /// Validates and stores a probability vector without normalizing it.
    pub fn new(probs: Vec<f64>) -> Result<Self, MixedError> {
        if probs.is_empty()
            || probs.iter().any(|prob| !prob.is_finite() || *prob < 0.0)
            || (probs.iter().sum::<f64>() - 1.0).abs() > DISTRIBUTION_TOLERANCE
        {
            return Err(MixedError::NotADistribution);
        }
        Ok(Self(probs))
    }

    /// Constructs the uniform distribution over `n` actions.
    pub fn uniform(n: usize) -> Result<Self, MixedError> {
        if n == 0 {
            return Err(MixedError::NotADistribution);
        }
        Self::new(vec![1.0 / n as f64; n])
    }

    /// Constructs a pure strategy over `n` actions.
    pub fn pure(n: usize, action: usize) -> Result<Self, MixedError> {
        if n == 0 || action >= n {
            return Err(MixedError::NotADistribution);
        }
        let mut probs = vec![0.0; n];
        probs[action] = 1.0;
        Ok(Self(probs))
    }

    /// Returns the probabilities in action-index order.
    pub fn probs(&self) -> &[f64] {
        &self.0
    }

    /// Returns the number of actions in the distribution.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns whether this distribution contains no actions.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Computes `F_i(sigma)`, the expected payoff of one role.
pub fn expected_payoff<G: FiniteGame>(
    game: &G,
    role: usize,
    strategies: &[MixedStrategy],
) -> Result<f64, MixedError> {
    validate_profile(game, strategies)?;
    if role >= game.n_roles() {
        return Err(MixedError::RoleCount {
            expected: game.n_roles(),
            actual: role.saturating_add(1),
        });
    }

    let action_values = action_payoffs_validated(game, role, strategies);
    Ok(action_values
        .iter()
        .zip(strategies[role].probs())
        .map(|(payoff, probability)| payoff * probability)
        .sum())
}

/// Computes `F_i(s_i, sigma_-i)` for every pure action of one role.
pub fn action_payoffs<G: FiniteGame>(
    game: &G,
    role: usize,
    strategies: &[MixedStrategy],
) -> Result<Vec<f64>, MixedError> {
    validate_profile(game, strategies)?;
    if role >= game.n_roles() {
        return Err(MixedError::RoleCount {
            expected: game.n_roles(),
            actual: role.saturating_add(1),
        });
    }
    Ok(action_payoffs_validated(game, role, strategies))
}

/// Computes regret separately for every role.
pub fn regrets<G: FiniteGame>(
    game: &G,
    strategies: &[MixedStrategy],
) -> Result<Vec<f64>, MixedError> {
    validate_profile(game, strategies)?;
    let mut result = Vec::with_capacity(game.n_roles());
    for role in 0..game.n_roles() {
        let action_values = action_payoffs_validated(game, role, strategies);
        let expected = action_values
            .iter()
            .zip(strategies[role].probs())
            .map(|(payoff, probability)| payoff * probability)
            .sum::<f64>();
        let best = action_values
            .iter()
            .copied()
            .fold(f64::NEG_INFINITY, f64::max);
        result.push(best - expected);
    }
    Ok(result)
}

/// Computes the maximum regret over all roles.
pub fn regret<G: FiniteGame>(game: &G, strategies: &[MixedStrategy]) -> Result<f64, MixedError> {
    let values = regrets(game, strategies)?;
    values
        .into_iter()
        .reduce(f64::max)
        .ok_or(MixedError::RoleCount {
            expected: 1,
            actual: 0,
        })
}

fn validate_profile<G: FiniteGame>(
    game: &G,
    strategies: &[MixedStrategy],
) -> Result<(), MixedError> {
    let expected_roles = game.n_roles();
    if expected_roles == 0 || strategies.len() != expected_roles {
        return Err(MixedError::RoleCount {
            expected: expected_roles,
            actual: strategies.len(),
        });
    }
    for (role, strategy) in strategies.iter().enumerate() {
        let expected = game.n_actions(role);
        if strategy.len() != expected {
            return Err(MixedError::Dimension {
                role,
                expected,
                actual: strategy.len(),
            });
        }
    }
    Ok(())
}

fn action_payoffs_validated<G: FiniteGame>(
    game: &G,
    role: usize,
    strategies: &[MixedStrategy],
) -> Vec<f64> {
    let mut result = vec![0.0; game.n_actions(role)];
    let counts = (0..game.n_roles())
        .map(|index| game.n_actions(index))
        .collect::<Vec<_>>();
    let mut profile = vec![0; game.n_roles()];

    loop {
        let probability = profile
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != role)
            .map(|(index, action)| strategies[index].probs()[*action])
            .product::<f64>();
        if probability != 0.0 {
            result[profile[role]] += probability * game.payoff(role, &profile);
        }
        if !advance_profile(&mut profile, &counts) {
            break;
        }
    }
    result
}

fn advance_profile(profile: &mut [usize], counts: &[usize]) -> bool {
    for role in (0..profile.len()).rev() {
        profile[role] += 1;
        if profile[role] < counts[role] {
            return true;
        }
        profile[role] = 0;
    }
    false
}
