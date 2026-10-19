//! Stage-game interfaces and reusable game representations.
//!
//! Implement a trait from the [`StageGame`] hierarchy to make a new game
//! available to rules and solvers. Simulation assembly validates actions with
//! [`StageGame::check_action`] before outcome evaluation.
//!
//! See the [stage-games guide](https://github.com/akitenkrad/rs-social-simulation-tools/blob/main/docs/games.md#stage-games).

use std::borrow::Cow;
use std::fmt::Debug;

use thiserror::Error;

mod aggregate;
/// Built-in finite stage games and population games.
pub mod catalog;
pub mod continuous;
mod matrix;
mod metric_ids;
pub mod spec;

pub use aggregate::{AggregateGame, AggregateReduce, AggregateSpec, AggregateSpecError};
pub use continuous::check_continuous_action;
pub(crate) use matrix::MAX_PROFILES;
pub use matrix::{MatrixError, MatrixGame};
pub use metric_ids::{metric_action_ids, MetricAction, MetricActionIds, MetricScheme};
pub use spec::{ContinuousSpec, FiniteSpec};

/// An invalid action supplied to a stage game.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum ActionError {
    /// The requested role is not in `0..StageGame::n_roles()`.
    #[error("role {role} is out of range")]
    InvalidRole {
        /// The rejected zero-based role index.
        role: usize,
    },
    /// A discrete action is outside the action space of its role.
    #[error("action {action} is invalid for role {role}")]
    InvalidAction {
        /// The zero-based role whose action was checked.
        role: usize,
        /// The rejected zero-based action index.
        action: usize,
    },
    /// A continuous action has a different number of components than the
    /// bounds of its role.
    #[error("action for role {role} has {actual} components, expected {expected}")]
    DimensionMismatch {
        /// The zero-based role whose action was checked.
        role: usize,
        /// Number of components declared by the role's bounds.
        expected: usize,
        /// Number of components supplied by the caller.
        actual: usize,
    },
    /// A component of a continuous action is NaN or infinite.
    #[error("component {dim} of the action for role {role} is not finite")]
    NonFiniteComponent {
        /// The zero-based role whose action was checked.
        role: usize,
        /// The zero-based component containing a non-finite value.
        dim: usize,
    },
    /// A component of a continuous action lies outside the closed bounds of
    /// its role.
    #[error("component {dim} of the action for role {role} is outside its bounds")]
    OutOfBounds {
        /// The zero-based role whose action was checked.
        role: usize,
        /// The zero-based component outside its closed interval.
        dim: usize,
    },
}

/// Defines the roles and legal actions of one encounter.
///
/// Game authors implement this base trait. Decision rules and world builders
/// call it to validate every held or newly chosen action. Role indices are
/// zero-based in `0..n_roles()`; an implementation must reject every other
/// role rather than indexing unchecked state.
///
/// See the [stage-games guide](https://github.com/akitenkrad/rs-social-simulation-tools/blob/main/docs/games.md#stage-games).
pub trait StageGame: Send + Sync + 'static {
    /// Value chosen by one role, such as a finite action index or real vector.
    type Action: Clone + Send + Sync + Debug + 'static;

    /// Returns the fixed number of seats in each encounter.
    ///
    /// Implementations must return a stable value for their lifetime.
    fn n_roles(&self) -> usize;

    /// Validates `action` for the zero-based `role`.
    ///
    /// # Errors
    ///
    /// Returns [`ActionError::InvalidRole`] for an unknown role and an
    /// action-specific variant when the value is outside that role's action
    /// space or otherwise malformed.
    fn check_action(&self, role: usize, action: &Self::Action) -> Result<(), ActionError>;
}

/// Assigns a payoff to every role and complete action profile.
///
/// Game authors implement this after [`StageGame`]. Outcome rules and solvers
/// call it only with a valid role and a profile of exactly [`n_roles`](StageGame::n_roles)
/// actions that have passed [`StageGame::check_action`]. Returned payoffs must
/// be finite whenever the game is used by simulation metrics or solvers.
pub trait NormalFormGame: StageGame {
    /// Computes the payoff of one role for a complete valid profile.
    ///
    /// The payoff unit is defined by the game. Implementations may panic for a
    /// role or profile that violates the trait contract.
    fn payoff(&self, role: usize, profile: &[Self::Action]) -> f64;

    /// Writes all role payoffs for a complete valid profile into `out`.
    ///
    /// The default calls [`payoff`](Self::payoff) for each role. Callers provide
    /// at least [`n_roles`](StageGame::n_roles) output slots; implementations
    /// overriding this method must preserve role order.
    fn payoffs_into(&self, profile: &[Self::Action], out: &mut [f64]) {
        for (role, value) in out.iter_mut().enumerate().take(self.n_roles()) {
            *value = self.payoff(role, profile);
        }
    }
}

/// A normal-form game whose actions are zero-based finite indices.
///
/// Implementations define a nonempty action set for every valid role. Rules
/// and finite-game solvers use the returned counts to enumerate actions.
pub trait FiniteGame: NormalFormGame<Action = usize> {
    /// Returns the number of legal actions for `role`.
    ///
    /// Valid actions are exactly `0..n_actions(role)`.
    fn n_actions(&self, role: usize) -> usize;

    /// Returns a display label for a valid action of a role.
    ///
    /// The default is `a{action}` and does not inspect `role`.
    fn action_label(&self, _role: usize, action: usize) -> Cow<'_, str> {
        Cow::Owned(format!("a{action}"))
    }

    /// Reports whether permuting roles leaves action spaces and payoffs unchanged.
    ///
    /// The default is `false`. Solvers may use `true` to choose a symmetric
    /// representation, so implementations must not claim symmetry merely
    /// because all roles have the same number of actions.
    fn is_symmetric(&self) -> bool {
        false
    }
}

/// How a best response may be computed numerically when a continuous game does
/// not provide an analytic one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum NumericBr {
    /// No numerical best response is available.
    Unavailable,
    /// The action space is one-dimensional and every role's payoff is unimodal
    /// in its own action, so a bracketing search over the bounds finds the
    /// global best response.
    Unimodal1D,
}

/// A normal-form game whose actions are finite real vectors inside a box.
///
/// The action of `role` has `bounds(role).len()` components, and component `d`
/// must lie in the closed interval `bounds(role)[d]`. Implementations should
/// use [`check_continuous_action`] in [`StageGame::check_action`] so that every
/// continuous game rejects malformed actions in the same way.
///
/// The bounds must have at least one dimension, finite end points, and
/// `lo < hi` in every dimension.
pub trait ContinuousGame: NormalFormGame<Action = Vec<f64>> {
    /// Returns the box constraint of `role`, one `(lo, hi)` pair per component.
    fn bounds(&self, role: usize) -> &[(f64, f64)];

    /// Returns an analytic best response of `role`, if the game has one.
    ///
    /// `profile` holds the actions of all roles, indexed by role. The entry
    /// `profile[role]` is ignored: an implementation must not read it, so the
    /// result depends only on the other roles' actions. The returned action
    /// lies inside `bounds(role)`. The default returns `None`.
    fn best_response(&self, _role: usize, _profile: &[Vec<f64>]) -> Option<Vec<f64>> {
        None
    }

    /// Declares whether a numerical best response may be used when no analytic
    /// one is available. The default is [`NumericBr::Unavailable`].
    fn numeric_br(&self) -> NumericBr {
        NumericBr::Unavailable
    }

    /// Returns the analytic gradient of the payoff of `role` with respect to
    /// its own action, if the game has one.
    ///
    /// `profile` holds the actions of all roles, indexed by role, and the
    /// result has length `bounds(role).len()`. The default returns `None`, in
    /// which case callers fall back to numerical differentiation.
    fn gradient(&self, _role: usize, _profile: &[Vec<f64>]) -> Option<Vec<f64>> {
        None
    }
}

/// A game that can describe its rules in natural language, for example in the
/// rules section of a prompt.
pub trait DescribeGame {
    /// Describes the game, from the point of view of `role` when given.
    fn describe(&self, role: Option<usize>) -> String;
}

/// A finite game whose payoffs depend only on an action and the other players'
/// action counts.
pub trait AnonymousGame: FiniteGame {
    /// Returns the payoff for action `a` given the other players' action counts.
    ///
    /// `counts_others` counts players other than the focal player, and its
    /// entries sum to `n_roles() - 1`.
    fn payoff_by_counts(&self, a: usize, counts_others: &[usize]) -> f64;
}

/// Marker for games whose roles share one common action space.
pub trait CommonActionSpace: StageGame {}

/// Checks that every role of a finite game has the same number of actions.
#[cfg(feature = "core")]
pub(crate) fn check_common_action_space<G: FiniteGame>(game: &G) -> Result<(), crate::GameError> {
    let n_roles = game.n_roles();
    if n_roles == 0 {
        return Err(crate::GameError::Population(
            "a common action space requires at least one role".to_owned(),
        ));
    }
    let first = game.n_actions(0);
    for role in 1..n_roles {
        let count = game.n_actions(role);
        if count != first {
            return Err(crate::GameError::Population(format!(
                "every role must share one action space, but role 0 has {first} actions and role {role} has {count}"
            )));
        }
    }
    if first == 0 {
        return Err(crate::GameError::Population(
            "the common action space is empty".to_owned(),
        ));
    }
    Ok(())
}
