use thiserror::Error;

use crate::game::MAX_PROFILES;
use crate::{FiniteGame, MixedStrategy};

/// Equilibria of anonymous games in count form.
pub mod anonymous;
pub mod dominance;
/// Evolutionary stability of symmetric mixed strategies.
pub mod ess;
pub mod lemke_howson;
mod linalg;
pub mod minimax;
pub mod pure;
pub mod qre;
/// Replicator dynamics of symmetric two-role games: integration and fixed points.
pub mod replicator;
pub mod support;

pub use anonymous::{pure_nash_counts, CountProfile};
pub use dominance::{iterated_strict_dominance, DominanceResult, Elimination};
pub use ess::{ess_verdict, EssVerdict};
pub use lemke_howson::{lemke_howson, LemkeHowsonOptions, LemkeHowsonResult};
pub use minimax::{maximin_security_level, zero_sum_minimax, MinimaxSolution, SecurityLevel};
pub use pure::{pure_nash, PureNash};
pub use qre::{logit_qre, QreOptions, QreSolution};
pub use replicator::{
    replicator_fixed_points, replicator_integrate, AffineComponent, FixedPoint, FixedPointSet,
    ReplicatorOptions, ReplicatorRun,
};
pub use support::{support_enumeration, EquilibriumSet, SupportFinding};

pub(crate) use linalg::{solve_least_squares, LeastSquaresSolution};

use crate::mixed::MixedError;

/// A low-level linear-programming failure.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum LpFailure {
    #[error("linear program is infeasible")]
    Infeasible,
    #[error("linear program is unbounded")]
    Unbounded,
    #[error("linear program was interrupted")]
    Interrupted,
    #[error("linear program did not reach an optimal solution")]
    NotOptimal,
    #[error("linear-programming backend failed: {0}")]
    Internal(String),
}

/// An error solving or validating a finite game.
#[derive(Clone, Debug, PartialEq, Error)]
#[non_exhaustive]
pub enum SolveError {
    #[error("game is too large: {profiles} profiles")]
    TooLarge { profiles: u128 },
    #[error("unsupported number of roles: {n_roles}")]
    UnsupportedRoles { n_roles: usize },
    #[error("role {role} has no actions")]
    EmptyActions { role: usize },
    #[error("game has a non-finite payoff")]
    NonFinitePayoff,
    #[error("game is not zero-sum")]
    NotZeroSum,
    #[error("label {label} is invalid for {n_labels} labels")]
    InvalidLabel { label: usize, n_labels: usize },
    #[error("maximum pivot limit reached: {limit}")]
    MaxPivots { limit: usize },
    #[error(transparent)]
    Lp(#[from] LpFailure),
    #[error("solution verification failed: duality gap {duality_gap}, regret {regret}")]
    VerificationFailed { duality_gap: f64, regret: f64 },
    #[error("solver did not converge: residual {residual}, iterations {iterations}")]
    NotConverged { residual: f64, iterations: usize },
    #[error("invalid solver options: {0}")]
    InvalidOptions(String),
    #[error(transparent)]
    Mixed(#[from] MixedError),
}

/// A verified equilibrium represented as one mixed strategy per role.
#[derive(Clone, Debug, PartialEq)]
pub struct Equilibrium {
    pub strategies: Vec<MixedStrategy>,
    pub payoffs: Vec<f64>,
    pub regret: f64,
}

impl Equilibrium {
    /// Returns true when every strategy puts probability one on exactly one action.
    pub fn is_pure(&self) -> bool {
        !self.strategies.is_empty()
            && self.strategies.iter().all(|strategy| {
                strategy.probs().iter().filter(|&&prob| prob == 1.0).count() == 1
                    && strategy
                        .probs()
                        .iter()
                        .all(|&prob| prob == 0.0 || prob == 1.0)
            })
    }
}

/// Computes the payoff-scaled comparison tolerance for a finite game.
pub fn tolerance<G: FiniteGame>(game: &G) -> Result<f64, SolveError> {
    Ok(inspect_game(game)?.tolerance)
}

pub(crate) struct GameInfo {
    pub counts: Vec<usize>,
    pub tolerance: f64,
}

pub(crate) fn inspect_game<G: FiniteGame>(game: &G) -> Result<GameInfo, SolveError> {
    let n_roles = game.n_roles();
    if n_roles == 0 {
        return Err(SolveError::UnsupportedRoles { n_roles });
    }

    let mut profiles = 1_u128;
    let mut counts = Vec::with_capacity(n_roles);
    for role in 0..n_roles {
        let count = game.n_actions(role);
        if count == 0 {
            return Err(SolveError::EmptyActions { role });
        }
        #[allow(clippy::manual_saturating_arithmetic)]
        {
            profiles = profiles.checked_mul(count as u128).unwrap_or(u128::MAX);
        }
        counts.push(count);
    }
    if profiles > MAX_PROFILES {
        return Err(SolveError::TooLarge { profiles });
    }

    let mut minimum = f64::INFINITY;
    let mut maximum = f64::NEG_INFINITY;
    let mut profile = vec![0; n_roles];
    loop {
        for role in 0..n_roles {
            let payoff = game.payoff(role, &profile);
            if !payoff.is_finite() {
                return Err(SolveError::NonFinitePayoff);
            }
            minimum = minimum.min(payoff);
            maximum = maximum.max(payoff);
        }
        if !advance_profile(&mut profile, &counts) {
            break;
        }
    }

    let range = maximum - minimum;
    let tolerance = if range.is_finite() {
        1e-10 * range.max(1.0)
    } else {
        (maximum * 1e-10 - minimum * 1e-10).max(1e-10)
    };
    Ok(GameInfo { counts, tolerance })
}

pub(crate) fn advance_profile(profile: &mut [usize], counts: &[usize]) -> bool {
    for role in (0..profile.len()).rev() {
        profile[role] += 1;
        if profile[role] < counts[role] {
            return true;
        }
        profile[role] = 0;
    }
    false
}
