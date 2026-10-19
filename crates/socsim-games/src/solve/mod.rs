//! Verified solvers and evolutionary analyses for finite and continuous games.
//!
//! See the [solver guide](https://github.com/akitenkrad/rs-social-simulation-tools/blob/main/docs/games.md#solving-finite-games).

use thiserror::Error;

use crate::game::MAX_PROFILES;
use crate::{FiniteGame, MixedStrategy};

/// Equilibria of anonymous games in count form.
pub mod anonymous;
/// Damped best-response iteration for games with continuous actions.
pub mod continuous;
/// Iterated elimination of strictly dominated pure actions.
pub mod dominance;
/// Evolutionary stability of symmetric mixed strategies.
pub mod ess;
/// Lemke--Howson pivoting for two-player bimatrix games.
pub mod lemke_howson;
mod linalg;
/// Linear-programming solvers for two-player zero-sum games.
pub mod minimax;
/// Exhaustive pure-strategy Nash equilibrium enumeration.
pub mod pure;
/// Logit quantal-response equilibrium iteration.
pub mod qre;
/// Replicator dynamics of symmetric two-role games: integration and fixed points.
pub mod replicator;
/// Support enumeration for mixed equilibria of two-player finite games.
pub mod support;

pub use anonymous::{pure_nash_counts, CountProfile};
pub use continuous::{
    best_response_iteration, best_response_step, golden_section_best_response, BestResponseOracle,
    BrIteration, BrIterationOptions, BrResult, BrStatus, GoldenSectionOptions,
};
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
    /// Constraints admit no feasible point.
    #[error("linear program is infeasible")]
    Infeasible,
    /// Objective can improve without a finite bound.
    #[error("linear program is unbounded")]
    Unbounded,
    /// Backend terminated before producing a solution.
    #[error("linear program was interrupted")]
    Interrupted,
    /// Backend stopped without certifying optimality.
    #[error("linear program did not reach an optimal solution")]
    NotOptimal,
    /// Backend reported another implementation-specific failure.
    #[error("linear-programming backend failed: {0}")]
    Internal(String),
}

/// An error solving or validating a finite game.
#[derive(Clone, Debug, PartialEq, Error)]
#[non_exhaustive]
pub enum SolveError {
    /// Profile or search space exceeds the solver's explicit safety limit.
    #[error("game is too large: {profiles} profiles")]
    TooLarge {
        /// Number of profiles or support pairs that would be examined.
        profiles: u128,
    },
    /// Solver is defined for a different number of roles.
    #[error("unsupported number of roles: {n_roles}")]
    UnsupportedRoles {
        /// Number of roles declared by the game.
        n_roles: usize,
    },
    /// A role has no action and therefore no strategy simplex.
    #[error("role {role} has no actions")]
    EmptyActions {
        /// Zero-based role with an empty action set.
        role: usize,
    },
    /// At least one enumerated payoff is NaN or infinite.
    #[error("game has a non-finite payoff")]
    NonFinitePayoff,
    /// A requested minimax solve failed the zero-sum payoff check.
    #[error("game is not zero-sum")]
    NotZeroSum,
    /// A tableau label lies outside the combined action-label range.
    #[error("label {label} is invalid for {n_labels} labels")]
    InvalidLabel {
        /// Rejected zero-based label.
        label: usize,
        /// Total number of valid labels.
        n_labels: usize,
    },
    /// A pivoting algorithm reached its configured iteration cap.
    #[error("maximum pivot limit reached: {limit}")]
    MaxPivots {
        /// Maximum number of permitted pivots.
        limit: usize,
    },
    /// Linear-programming backend failure.
    #[error(transparent)]
    Lp(#[from] LpFailure),
    /// A candidate failed the solver's final numerical certificate.
    #[error("solution verification failed: duality gap {duality_gap}, regret {regret}")]
    VerificationFailed {
        /// Difference between primal and dual objectives.
        duality_gap: f64,
        /// Maximum profitable unilateral deviation.
        regret: f64,
    },
    /// An iterative method exhausted its budget before meeting tolerance.
    #[error("solver did not converge: residual {residual}, iterations {iterations}")]
    NotConverged {
        /// Final convergence residual.
        residual: f64,
        /// Number of completed iterations.
        iterations: usize,
    },
    /// Solver parameters are non-finite, out of range, or mutually inconsistent.
    #[error("invalid solver options: {0}")]
    InvalidOptions(String),
    /// Invalid mixed-strategy construction or arithmetic.
    #[error(transparent)]
    Mixed(#[from] MixedError),
    /// No converged best response could be obtained for `role`: the game has
    /// no analytic best response, no oracle was given, and a numerical search
    /// is not available, or the chosen method did not converge.
    #[error("no converged best response is available for role {role}")]
    NoBestResponse {
        /// Zero-based role for which no converged response was available.
        role: usize,
    },
    /// The game violates a contract that cannot be expressed in its type, for
    /// example invalid bounds of a continuous game.
    #[error("invalid game: {0}")]
    InvalidGame(String),
}

/// A verified equilibrium represented as one mixed strategy per role.
#[derive(Clone, Debug, PartialEq)]
pub struct Equilibrium {
    /// One normalized mixed strategy per role, in role order.
    pub strategies: Vec<MixedStrategy>,
    /// Expected payoff of each role at the reported strategy profile.
    pub payoffs: Vec<f64>,
    /// Largest profitable unilateral deviation; bounded by solver tolerance.
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
