//! Learning primitives for finite games.

use rand::{Rng, RngCore};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub mod automaton;
pub mod fictitious;
pub mod logit;
pub mod roth_erev;

/// An error produced by a game-learning primitive.
#[derive(Clone, Debug, PartialEq, Error)]
#[non_exhaustive]
pub enum LearnError {
    #[error("learning rule requires {expected} roles, but the game has {actual}")]
    UnsupportedRoleCount { expected: usize, actual: usize },
    #[error("role {role} is out of range for a game with {n_roles} roles")]
    InvalidRole { role: usize, n_roles: usize },
    #[error("role {role} must have {expected} actions, but has {actual}")]
    InvalidActionCount {
        role: usize,
        expected: usize,
        actual: usize,
    },
    #[error("{what} has length {actual}, expected {expected}")]
    LengthMismatch {
        what: &'static str,
        expected: usize,
        actual: usize,
    },
    #[error("initial count at index {index} must be finite and non-negative, got {value}")]
    InvalidInitialCount { index: usize, value: f64 },
    #[error("parameter {parameter} has invalid value {value}")]
    InvalidParameter { parameter: &'static str, value: f64 },
    #[error(
        "game payoff must be strictly positive at role {role}, profile {profile:?}; got {payoff}"
    )]
    NonPositivePayoff {
        role: usize,
        profile: Vec<usize>,
        payoff: f64,
    },
    #[error("realized payoff must be finite and strictly positive, got {payoff}")]
    InvalidRealizedPayoff { payoff: f64 },
    #[error("action {action} is out of range for {n_actions} actions")]
    InvalidAction { action: usize, n_actions: usize },
    #[error("cooperation action must be 0 or 1, got {coop}")]
    InvalidCooperationAction { coop: usize },
    #[error("probability at index {index} must be finite and non-negative, got {value}")]
    InvalidProbability { index: usize, value: f64 },
    #[error("probability vector is empty or has no positive mass")]
    EmptyProbabilityMass,
    #[error("a learned count overflowed")]
    CountOverflow,
    #[error("a payoff calculation produced a non-finite value")]
    NonFiniteCalculation,
}

/// Policy used when several actions have exactly equal values.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TieBreak {
    /// Choose the lowest action index without consuming randomness.
    #[default]
    LowestIndex,
    /// Draw uniformly from the tied indices.
    Random,
}

/// Samples an index by drawing `rng.gen::<f64>()` once and visiting cumulative
/// probabilities in ascending index order.
///
/// If floating-point rounding leaves the draw beyond the cumulative sum, the
/// last index with positive probability is returned. Invalid or empty mass is
/// reported as [`LearnError`].
pub fn sample_probabilities(
    probabilities: &[f64],
    rng: &mut dyn RngCore,
) -> Result<usize, LearnError> {
    let mut last_positive = None;
    for (index, &value) in probabilities.iter().enumerate() {
        if !value.is_finite() || value < 0.0 {
            return Err(LearnError::InvalidProbability { index, value });
        }
        if value > 0.0 {
            last_positive = Some(index);
        }
    }
    let last_positive = last_positive.ok_or(LearnError::EmptyProbabilityMass)?;
    let draw = rng.gen::<f64>();
    let mut cumulative = 0.0;
    for (index, &value) in probabilities.iter().enumerate() {
        cumulative += value;
        if draw < cumulative {
            return Ok(index);
        }
    }
    Ok(last_positive)
}

/// Resolves candidates that were collected in ascending action-index order.
///
/// A single candidate never consumes randomness. [`TieBreak::LowestIndex`]
/// chooses the first candidate. [`TieBreak::Random`] uses
/// `rng.gen_range(0..k)` exactly once for `k > 1` tied candidates.
pub fn resolve_tie(
    candidates: &[usize],
    policy: TieBreak,
    rng: &mut dyn RngCore,
) -> Result<usize, LearnError> {
    let Some(&first) = candidates.first() else {
        return Err(LearnError::EmptyProbabilityMass);
    };
    if candidates.len() == 1 || policy == TieBreak::LowestIndex {
        return Ok(first);
    }
    Ok(candidates[rng.gen_range(0..candidates.len())])
}

pub(crate) fn validate_two_player<G: crate::FiniteGame>(game: &G) -> Result<(), LearnError> {
    let actual = game.n_roles();
    if actual != 2 {
        return Err(LearnError::UnsupportedRoleCount {
            expected: 2,
            actual,
        });
    }
    Ok(())
}

pub(crate) fn opponent_role<G: crate::FiniteGame>(
    game: &G,
    role: usize,
) -> Result<usize, LearnError> {
    validate_two_player(game)?;
    if role >= 2 {
        return Err(LearnError::InvalidRole { role, n_roles: 2 });
    }
    Ok(1 - role)
}

pub(crate) fn payoff_against(
    game: &impl crate::FiniteGame,
    role: usize,
    own_action: usize,
    opponent_action: usize,
) -> f64 {
    let profile = if role == 0 {
        [own_action, opponent_action]
    } else {
        [opponent_action, own_action]
    };
    game.payoff(role, &profile)
}
