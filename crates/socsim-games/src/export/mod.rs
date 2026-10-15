//! Explicit JSON export for game definitions and solver results.
//!
//! The module writes versioned `game_spec.json` and `solutions.json` files for
//! visualization and other external tooling. Constructors validate shapes,
//! labels, probabilities, and all floating-point values before serialization.
//! Files are written atomically through a unique temporary file followed by a
//! rename. This crate never writes these artifacts implicitly: callers must
//! invoke one of the writing functions.

mod convert;
mod error;
mod solutions;
mod spec;
mod write;

pub use error::ExportError;
pub use solutions::{EssLabel, SolutionSet};
pub use spec::GameSpec;
pub use write::{write_artifacts, write_game_spec, write_solutions};
