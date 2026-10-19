use std::path::PathBuf;

use thiserror::Error;

/// An error while validating or writing exported game artifacts.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ExportError {
    /// A field contains a NaN or infinite value.
    #[error("field `{field}` contains a non-finite value")]
    NonFinite {
        /// Dot-like field path containing the invalid number.
        field: String,
    },
    /// A finite payoff tensor exceeds the export size limit.
    #[error("payoff tensor has {elements} elements, exceeding the limit of {limit}")]
    TooLarge {
        /// Number of elements the artifact would contain.
        elements: u128,
        /// Maximum number of elements accepted by the export format.
        limit: u128,
    },
    /// A collection has a shape that does not match the game definition.
    #[error("invalid shape: {0}")]
    Shape(String),
    /// A required name or label is empty.
    #[error("field `{field}` contains an empty label")]
    EmptyLabel {
        /// Field path containing the empty label.
        field: String,
    },
    /// A solution identifier has already been added to its set.
    #[error("solution id `{0}` is duplicated")]
    DuplicateId(String),
    /// A probability vector is invalid.
    #[error("invalid probability vector for solution `{id}`, role {role}: {detail}")]
    Probability {
        /// Stable identifier of the invalid solution.
        id: String,
        /// Zero-based role whose distribution is invalid.
        role: usize,
        /// Specific normalization, sign, or dimension failure.
        detail: String,
    },
    /// A solution or artifact does not match its game definition.
    #[error("game mismatch: {0}")]
    Mismatch(String),
    /// A filesystem operation failed.
    #[error("I/O error for `{path}`: {message}", path = path.display())]
    Io {
        /// Artifact path involved in the failed operation.
        path: PathBuf,
        /// Underlying I/O error text.
        message: String,
    },
    /// JSON serialization failed.
    #[error("JSON serialization failed: {0}")]
    Json(String),
}
