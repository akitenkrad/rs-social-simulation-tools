use std::path::PathBuf;

use thiserror::Error;

/// An error while validating or writing exported game artifacts.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ExportError {
    /// A field contains a NaN or infinite value.
    #[error("field `{field}` contains a non-finite value")]
    NonFinite { field: String },
    /// A finite payoff tensor exceeds the export size limit.
    #[error("payoff tensor has {elements} elements, exceeding the limit of {limit}")]
    TooLarge { elements: u128, limit: u128 },
    /// A collection has a shape that does not match the game definition.
    #[error("invalid shape: {0}")]
    Shape(String),
    /// A required name or label is empty.
    #[error("field `{field}` contains an empty label")]
    EmptyLabel { field: String },
    /// A solution identifier has already been added to its set.
    #[error("solution id `{0}` is duplicated")]
    DuplicateId(String),
    /// A probability vector is invalid.
    #[error("invalid probability vector for solution `{id}`, role {role}: {detail}")]
    Probability {
        id: String,
        role: usize,
        detail: String,
    },
    /// A solution or artifact does not match its game definition.
    #[error("game mismatch: {0}")]
    Mismatch(String),
    /// A filesystem operation failed.
    #[error("I/O error for `{path}`: {message}", path = path.display())]
    Io { path: PathBuf, message: String },
    /// JSON serialization failed.
    #[error("JSON serialization failed: {0}")]
    Json(String),
}
