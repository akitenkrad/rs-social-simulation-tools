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
    /// A primitive defined only for a fixed number of roles received another size.
    #[error("learning rule requires {expected} roles, but the game has {actual}")]
    UnsupportedRoleCount {
        /// Number of roles required by the primitive.
        expected: usize,
        /// Number of roles declared by the game.
        actual: usize,
    },
    /// A role index lies outside the game's role range.
    #[error("role {role} is out of range for a game with {n_roles} roles")]
    InvalidRole {
        /// Rejected zero-based role index.
        role: usize,
        /// Number of roles declared by the game.
        n_roles: usize,
    },
    /// A role's action count does not match a learning primitive's fixed size.
    #[error("role {role} must have {expected} actions, but has {actual}")]
    InvalidActionCount {
        /// Zero-based role with the incompatible action set.
        role: usize,
        /// Action count required by the primitive.
        expected: usize,
        /// Action count declared by the game.
        actual: usize,
    },
    /// An input vector has a different length from its corresponding action set.
    #[error("{what} has length {actual}, expected {expected}")]
    LengthMismatch {
        /// Name of the vector being validated.
        what: &'static str,
        /// Required number of entries.
        expected: usize,
        /// Supplied number of entries.
        actual: usize,
    },
    /// A fictitious-play prior contains a negative or non-finite count.
    #[error("initial count at index {index} must be finite and non-negative, got {value}")]
    InvalidInitialCount {
        /// Zero-based entry containing the invalid count.
        index: usize,
        /// Rejected count value.
        value: f64,
    },
    /// A named learning parameter is outside its documented domain.
    #[error("parameter {parameter} has invalid value {value}")]
    InvalidParameter {
        /// Stable parameter name used in diagnostics.
        parameter: &'static str,
        /// Rejected parameter value.
        value: f64,
    },
    /// A Roth--Erev game payoff violates the strict-positivity prerequisite.
    #[error(
        "game payoff must be strictly positive at role {role}, profile {profile:?}; got {payoff}"
    )]
    NonPositivePayoff {
        /// Role whose payoff violates the positivity requirement.
        role: usize,
        /// Complete finite-action profile at which validation failed.
        profile: Vec<usize>,
        /// Non-positive or non-finite payoff returned by the game.
        payoff: f64,
    },
    /// A realized reinforcement payoff is not finite and strictly positive.
    #[error("realized payoff must be finite and strictly positive, got {payoff}")]
    InvalidRealizedPayoff {
        /// Rejected payoff value.
        payoff: f64,
    },
    /// A zero-based action index lies outside its finite action space.
    #[error("action {action} is out of range for {n_actions} actions")]
    InvalidAction {
        /// Rejected action index.
        action: usize,
        /// Number of available actions.
        n_actions: usize,
    },
    /// A binary repeated-game automaton received an index other than 0 or 1.
    #[error("cooperation action must be 0 or 1, got {coop}")]
    InvalidCooperationAction {
        /// Rejected cooperation-action index.
        coop: usize,
    },
    /// A sampling weight is negative or non-finite.
    #[error("probability at index {index} must be finite and non-negative, got {value}")]
    InvalidProbability {
        /// Zero-based entry containing the invalid weight.
        index: usize,
        /// Rejected probability value.
        value: f64,
    },
    /// A probability vector is empty or every entry is zero.
    #[error("probability vector is empty or has no positive mass")]
    EmptyProbabilityMass,
    /// An integer observation count exceeded its representable range.
    #[error("a learned count overflowed")]
    CountOverflow,
    /// An intermediate learning calculation produced NaN or infinity.
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
