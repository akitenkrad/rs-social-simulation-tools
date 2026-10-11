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
    #[error("aggregate_history_len must be greater than zero")]
    InvalidAggregateHistoryLength,
    #[error("only two-role encounters are supported, got {n_roles}")]
    UnsupportedRoles { n_roles: usize },
    #[error("invalid matching: {0}")]
    InvalidMatching(String),
    /// The topology does not fit the world, or its shape is invalid.
    #[error("invalid topology: {0}")]
    Topology(String),
    /// A population dynamic or an anonymous game was configured inconsistently,
    /// for example a parameter outside its range or a payoff bound that cannot be
    /// established.
    #[error("invalid population dynamics: {0}")]
    Population(String),
    #[error("invalid game metrics configuration: {0}")]
    Metrics(String),
    #[error("policy table kinds do not match the kind catalog")]
    InvalidKinds {
        expected: Vec<String>,
        actual: Vec<String>,
    },
    #[error("LLM replay manifest mismatch: {0}")]
    ManifestMismatch(String),
}
