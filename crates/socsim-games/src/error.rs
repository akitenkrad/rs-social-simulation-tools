use thiserror::Error;

use crate::catalog::CatalogError;
use crate::{ActionError, MatrixError, RuleError};

/// An error detected while constructing a game simulation.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum GameError {
    /// Decision-rule registration, initialization, or execution failed.
    #[error(transparent)]
    Rule(#[from] RuleError),
    /// A held, initial, fallback, or chosen action is invalid for its role.
    #[error(transparent)]
    Action(#[from] ActionError),
    /// A matrix game has invalid dimensions, values, or labels.
    #[error(transparent)]
    Matrix(#[from] MatrixError),
    /// Assignment, action, or topology data does not match the population size.
    #[error("agent count mismatch: expected {expected}, got {actual}")]
    AgentCount {
        /// Required number of agent-indexed entries.
        expected: usize,
        /// Supplied number of entries.
        actual: usize,
    },
    /// Step history cannot use a zero-length ring.
    #[error("history_len must be greater than zero")]
    InvalidHistoryLength,
    /// Public aggregate history cannot use a zero-length ring.
    #[error("aggregate_history_len must be greater than zero")]
    InvalidAggregateHistoryLength,
    /// A component supports a different number of stage-game roles.
    #[error("only two-role encounters are supported, got {n_roles}")]
    UnsupportedRoles {
        /// Number of roles declared by the game.
        n_roles: usize,
    },
    /// Matching configuration is inconsistent with agents, roles, or decision scope.
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
    /// Metrics options cannot be evaluated for the selected game or world.
    #[error("invalid game metrics configuration: {0}")]
    Metrics(String),
    /// Policy-table interned names differ from the assembled rule catalog.
    #[error("policy table kinds do not match the kind catalog")]
    InvalidKinds {
        /// Canonical kind names supplied by the assembled catalog.
        expected: Vec<String>,
        /// Canonical kind names persisted in the world.
        actual: Vec<String>,
    },
    /// Snapshot state was created with another response store or backend identity.
    #[error("LLM replay manifest mismatch: {0}")]
    ManifestMismatch(String),
    /// A configuration value is missing, malformed, or inconsistent, for
    /// example an initial-action share that is negative, or a scenario key of
    /// the wrong type. The message starts with the offending key or setting.
    #[error("invalid scenario: {0}")]
    Config(String),
    /// A built-in game rejected its parameters.
    #[error(transparent)]
    Catalog(#[from] CatalogError),
}
