use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::NormalFormGame;

const MAX_AGGREGATE_NAME_LEN: usize = 49;

/// How aggregate values are reduced across observations.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum AggregateReduce {
    /// Add the observed values.
    Sum,
    /// Compute the arithmetic mean of the observed values.
    Mean,
}

/// An error constructing an [`AggregateSpec`].
#[derive(Clone, Debug, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum AggregateSpecError {
    /// The aggregate name is empty.
    #[error("aggregate name must not be empty")]
    EmptyName,
    /// The aggregate name starts with an unsupported character.
    #[error("aggregate name must start with an ASCII lowercase letter or digit")]
    InvalidStart,
    /// The aggregate name contains an unsupported character.
    #[error("aggregate name contains an invalid character at byte {index}")]
    InvalidCharacter { index: usize },
    /// The aggregate name exceeds the maximum length.
    #[error("aggregate name is {length} bytes long; maximum is 49")]
    TooLong { length: usize },
}

#[derive(Deserialize)]
struct AggregateSpecRepr {
    name: String,
    reduce: AggregateReduce,
}

/// The name and reduction method for one game aggregate.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "AggregateSpecRepr")]
pub struct AggregateSpec {
    name: String,
    reduce: AggregateReduce,
}

impl TryFrom<AggregateSpecRepr> for AggregateSpec {
    type Error = AggregateSpecError;

    fn try_from(value: AggregateSpecRepr) -> Result<Self, Self::Error> {
        Self::new(value.name, value.reduce)
    }
}

impl AggregateSpec {
    /// Constructs a validated aggregate specification.
    pub fn new(
        name: impl Into<String>,
        reduce: AggregateReduce,
    ) -> Result<Self, AggregateSpecError> {
        let name = name.into();
        validate_name(&name)?;
        Ok(Self { name, reduce })
    }

    /// Returns the single-segment aggregate name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the reduction method.
    pub fn reduce(&self) -> AggregateReduce {
        self.reduce
    }
}

fn validate_name(name: &str) -> Result<(), AggregateSpecError> {
    let Some(first) = name.bytes().next() else {
        return Err(AggregateSpecError::EmptyName);
    };
    if name.len() > MAX_AGGREGATE_NAME_LEN {
        return Err(AggregateSpecError::TooLong { length: name.len() });
    }
    if !(first.is_ascii_lowercase() || first.is_ascii_digit()) {
        return Err(AggregateSpecError::InvalidStart);
    }
    if let Some((index, _)) = name.bytes().enumerate().skip(1).find(|(_, byte)| {
        !(byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-'))
    }) {
        return Err(AggregateSpecError::InvalidCharacter { index });
    }
    Ok(())
}

/// A normal-form game that exposes additional profile-level aggregate values.
pub trait AggregateGame: NormalFormGame {
    /// Returns the specifications for values written by [`Self::aggregate_into`].
    fn aggregate_spec(&self) -> &[AggregateSpec];

    /// Writes aggregate values for an action profile.
    ///
    /// `out` must have length `self.aggregate_spec().len()`, and every value
    /// written to `out` must be finite.
    fn aggregate_into(&self, profile: &[Self::Action], out: &mut [f64]);
}
