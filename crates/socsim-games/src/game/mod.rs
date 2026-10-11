use std::borrow::Cow;
use std::fmt::Debug;

use thiserror::Error;

mod aggregate;
pub mod catalog;
mod matrix;
mod metric_ids;

pub use aggregate::{AggregateGame, AggregateReduce, AggregateSpec, AggregateSpecError};
pub(crate) use matrix::MAX_PROFILES;
pub use matrix::{MatrixError, MatrixGame};
pub use metric_ids::{metric_action_ids, MetricAction, MetricActionIds, MetricScheme};

/// An invalid action supplied to a stage game.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum ActionError {
    #[error("role {role} is out of range")]
    InvalidRole { role: usize },
    #[error("action {action} is invalid for role {role}")]
    InvalidAction { role: usize, action: usize },
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
