use std::borrow::Cow;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::{ActionError, CommonActionSpace, FiniteGame, NormalFormGame, StageGame};

pub(crate) const MAX_PROFILES: u128 = 1_000_000;
const MAX_PAYOFF_ELEMENTS: u128 = 10_000_000;

/// An error constructing a [`MatrixGame`].
#[derive(Clone, Debug, PartialEq, Error)]
#[non_exhaustive]
pub enum MatrixError {
    /// Role counts and row-major payoff tensors have inconsistent dimensions.
    #[error("payoff shape does not match the action counts")]
    Shape,
    /// The profile count or total payoff elements exceed safety limits.
    #[error("matrix too large: {profiles} profiles, {elements} payoff elements")]
    TooLarge {
        /// Product of every role's action count.
        profiles: u128,
        /// Profile count multiplied by the number of roles.
        elements: u128,
    },
    /// At least one payoff is NaN or infinite.
    #[error("non-finite payoff")]
    NonFinite,
    /// Optional action labels do not provide exactly one label per action.
    #[error("label table does not match the action counts")]
    Labels,
}

/// A finite game represented by one row-major payoff tensor per role.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MatrixGame {
    n_actions: Vec<usize>,
    payoffs: Vec<Vec<f64>>,
    labels: Option<Vec<Vec<String>>>,
    symmetric: bool,
}

impl MatrixGame {
    /// Constructs a validated role-major payoff tensor.
    ///
    /// `payoffs[role]` is row-major over complete profiles, with the last role
    /// varying fastest.
    ///
    /// # Errors
    ///
    /// Returns [`MatrixError`] for empty action spaces, mismatched tensor
    /// dimensions, excessive size, or non-finite payoffs.
    pub fn new(n_actions: Vec<usize>, payoffs: Vec<Vec<f64>>) -> Result<Self, MatrixError> {
        if n_actions.is_empty() || n_actions.contains(&0) {
            return Err(MatrixError::Shape);
        }

        let (profiles, profiles_u128) = profile_count(&n_actions)?;
        let elements = profiles_u128.saturating_mul(n_actions.len() as u128);
        if profiles_u128 > MAX_PROFILES || elements > MAX_PAYOFF_ELEMENTS {
            return Err(MatrixError::TooLarge {
                profiles: profiles_u128,
                elements,
            });
        }

        if payoffs.len() != n_actions.len()
            || payoffs
                .iter()
                .any(|role_payoffs| role_payoffs.len() != profiles)
        {
            return Err(MatrixError::Shape);
        }
        if payoffs.iter().flatten().any(|payoff| !payoff.is_finite()) {
            return Err(MatrixError::NonFinite);
        }

        let symmetric = is_symmetric_matrix(&n_actions, &payoffs);
        Ok(Self {
            n_actions,
            payoffs,
            labels: None,
            symmetric,
        })
    }

    /// Constructs a two-role game from equally shaped row and column payoff matrices.
    ///
    /// # Errors
    ///
    /// Returns [`MatrixError::Shape`] unless both matrices are nonempty,
    /// rectangular, and equally shaped; other validation errors come from [`new`](Self::new).
    pub fn bimatrix(a: &[Vec<f64>], b: &[Vec<f64>]) -> Result<Self, MatrixError> {
        let (rows, columns) = rectangular_shape(a).ok_or(MatrixError::Shape)?;
        if rectangular_shape(b) != Some((rows, columns)) {
            return Err(MatrixError::Shape);
        }

        Self::new(
            vec![rows, columns],
            vec![
                a.iter().flatten().copied().collect(),
                b.iter().flatten().copied().collect(),
            ],
        )
    }

    /// Constructs a two-role zero-sum game whose role-1 payoffs are `-a`.
    ///
    /// # Errors
    ///
    /// Returns [`MatrixError::Shape`] for a non-rectangular or empty matrix and
    /// otherwise forwards [`new`](Self::new) validation errors.
    pub fn zero_sum(a: &[Vec<f64>]) -> Result<Self, MatrixError> {
        let (rows, columns) = rectangular_shape(a).ok_or(MatrixError::Shape)?;
        let role_0: Vec<f64> = a.iter().flatten().copied().collect();
        let role_1 = role_0.iter().map(|payoff| -*payoff).collect();
        Self::new(vec![rows, columns], vec![role_0, role_1])
    }

    /// Replaces generated action labels with one label per role and action.
    ///
    /// # Errors
    ///
    /// Returns [`MatrixError::Labels`] if either the role count or an action
    /// label count differs from the game's action spaces.
    pub fn with_labels(mut self, labels: Vec<Vec<String>>) -> Result<Self, MatrixError> {
        if labels.len() != self.n_actions.len()
            || labels
                .iter()
                .zip(&self.n_actions)
                .any(|(role_labels, &count)| role_labels.len() != count)
        {
            return Err(MatrixError::Labels);
        }
        self.labels = Some(labels);
        Ok(self)
    }

    fn profile_index(&self, profile: &[usize]) -> usize {
        profile
            .iter()
            .zip(&self.n_actions)
            .fold(0, |index, (&action, &count)| index * count + action)
    }
}

impl StageGame for MatrixGame {
    type Action = usize;

    fn n_roles(&self) -> usize {
        self.n_actions.len()
    }

    fn check_action(&self, role: usize, action: &usize) -> Result<(), ActionError> {
        let Some(&count) = self.n_actions.get(role) else {
            return Err(ActionError::InvalidRole { role });
        };
        if *action >= count {
            return Err(ActionError::InvalidAction {
                role,
                action: *action,
            });
        }
        Ok(())
    }
}

impl NormalFormGame for MatrixGame {
    fn payoff(&self, role: usize, profile: &[usize]) -> f64 {
        self.payoffs[role][self.profile_index(profile)]
    }
}

impl FiniteGame for MatrixGame {
    fn n_actions(&self, role: usize) -> usize {
        self.n_actions[role]
    }

    fn action_label(&self, role: usize, action: usize) -> Cow<'_, str> {
        match &self.labels {
            Some(labels) => Cow::Borrowed(&labels[role][action]),
            None => Cow::Owned(format!("a{action}")),
        }
    }

    fn is_symmetric(&self) -> bool {
        self.symmetric
    }
}

impl CommonActionSpace for MatrixGame {}

fn profile_count(n_actions: &[usize]) -> Result<(usize, u128), MatrixError> {
    let mut profiles = 1_usize;
    let mut profiles_u128 = 1_u128;
    for &count in n_actions {
        profiles_u128 = profiles_u128.saturating_mul(count as u128);
        let Some(next) = profiles.checked_mul(count) else {
            return Err(MatrixError::TooLarge {
                profiles: profiles_u128,
                elements: profiles_u128.saturating_mul(n_actions.len() as u128),
            });
        };
        profiles = next;
    }
    Ok((profiles, profiles_u128))
}

fn rectangular_shape(matrix: &[Vec<f64>]) -> Option<(usize, usize)> {
    let columns = matrix.first()?.len();
    if columns == 0 || matrix.iter().any(|row| row.len() != columns) {
        return None;
    }
    Some((matrix.len(), columns))
}

fn is_symmetric_matrix(n_actions: &[usize], payoffs: &[Vec<f64>]) -> bool {
    if n_actions.len() != 2 || n_actions[0] != n_actions[1] {
        return false;
    }

    let count = n_actions[0];
    (0..count).all(|row| {
        (0..count)
            .all(|column| payoffs[0][row * count + column] == payoffs[1][column * count + row])
    })
}
