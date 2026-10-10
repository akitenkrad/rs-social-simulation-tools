use thiserror::Error;

use crate::{ActionError, MatrixError, RuleError};

/// An error detected while constructing a game simulation.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum GameError {
    #[error(transparent)]
    Rule(#[from] RuleError),
    #[error(transparent)]
    Action(#[from] ActionError),
    #[error(transparent)]
    Matrix(#[from] MatrixError),
    #[error("agent count mismatch: expected {expected}, got {actual}")]
    AgentCount { expected: usize, actual: usize },
    #[error("history_len must be greater than zero")]
    InvalidHistoryLength,
    #[error("only two-role encounters are supported, got {n_roles}")]
    UnsupportedRoles { n_roles: usize },
    #[error("invalid matching: {0}")]
    InvalidMatching(String),
    #[error("policy table kinds do not match the kind catalog")]
    InvalidKinds {
        expected: Vec<String>,
        actual: Vec<String>,
    },
    #[error("LLM replay manifest mismatch: {0}")]
    ManifestMismatch(String),
}
