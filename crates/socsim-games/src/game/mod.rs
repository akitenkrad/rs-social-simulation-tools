use std::borrow::Cow;
use std::fmt::Debug;

use thiserror::Error;

mod aggregate;
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
    #[error("role {role} is out of range")]
    InvalidRole { role: usize },
    #[error("action {action} is invalid for role {role}")]
    InvalidAction { role: usize, action: usize },
    /// A continuous action has a different number of components than the
    /// bounds of its role.
    #[error("action for role {role} has {actual} components, expected {expected}")]
    DimensionMismatch {
        role: usize,
        expected: usize,
        actual: usize,
    },
    /// A component of a continuous action is NaN or infinite.
    #[error("component {dim} of the action for role {role} is not finite")]
    NonFiniteComponent { role: usize, dim: usize },
    /// A component of a continuous action lies outside the closed bounds of
    /// its role.
    #[error("component {dim} of the action for role {role} is outside its bounds")]
    OutOfBounds { role: usize, dim: usize },
}

/// A stage game defines its roles and the actions available to each role.
pub trait StageGame: Send + Sync + 'static {
    type Action: Clone + Send + Sync + Debug + 'static;

    fn n_roles(&self) -> usize;

    fn check_action(&self, role: usize, action: &Self::Action) -> Result<(), ActionError>;
}

/// A normal-form game assigns a payoff to every role and action profile.
pub trait NormalFormGame: StageGame {
    fn payoff(&self, role: usize, profile: &[Self::Action]) -> f64;

    fn payoffs_into(&self, profile: &[Self::Action], out: &mut [f64]) {
        for (role, value) in out.iter_mut().enumerate().take(self.n_roles()) {
            *value = self.payoff(role, profile);
        }
    }
}

/// A finite normal-form game whose actions are zero-based indices.
pub trait FiniteGame: NormalFormGame<Action = usize> {
    fn n_actions(&self, role: usize) -> usize;

    fn action_label(&self, _role: usize, action: usize) -> Cow<'_, str> {
        Cow::Owned(format!("a{action}"))
    }

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
