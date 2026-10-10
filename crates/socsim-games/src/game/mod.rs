use std::borrow::Cow;
use std::fmt::Debug;

use thiserror::Error;

mod matrix;

pub use matrix::{MatrixError, MatrixGame};

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
