//! Bundled socsim CLI packs.
//!
//! This crate aggregates the two packs the `socsim` CLI ships with, keeping the
//! CLI binary (`socsim-cli`) thin: each pack here is self-contained, bundling
//! its world type, its mechanisms, a registration function, and a starter
//! scenario TOML.
//!
//! - [`hr_lifecycle`] — the 9-stage employee-lifecycle reference module
//!   (`HrWorld`, `HrLifecyclePack`, calibration constants, and the optional
//!   `marl` learnable-turnover integration behind the `marl` feature).
//! - [`opinion`] — the bounded-confidence opinion-dynamics world (`OpinionWorld`,
//!   `OpinionMetricsMechanism`, a [`opinion::register`] wiring the
//!   `socsim-mechanisms` opinion mechanisms).
//!
//! Each module is gated behind a Cargo feature of the same conceptual name so
//! downstream binaries can compile in only the packs they need.

#[cfg(feature = "hr-lifecycle")]
pub mod hr_lifecycle;
#[cfg(feature = "opinion-dynamics")]
pub mod opinion;
#[cfg(feature = "organizational-silence")]
pub mod organizational_silence;
