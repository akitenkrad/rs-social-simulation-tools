//! Reusable **observation metrics** for the `socsim` platform.
//!
//! Models across the workspace repeatedly reimplement the same summary
//! statistics — mean, variance, Gini, polarization, opinion clusters, cascade
//! sizes, spatial segregation.  This crate gathers them into one place so they
//! can be shared rather than copied.
//!
//! # Metrics are read-only / derived
//!
//! Every function here is a **pure observation**: it reads world / opinion /
//! network / grid state and returns a derived number.  Nothing here owns an
//! RNG, mutates simulation state, or participates in the update rule of a
//! model.  Metrics therefore have **no calibration impact** — swapping a
//! model's hand-rolled `variance` for [`stats::variance`] cannot change its
//! trajectory, only how that trajectory is *summarised*.  The one mechanism
//! type exposed here, [`MetricsMechanism`](opinion::MetricsMechanism), runs in
//! the `PostStep` phase and only *records* via the [`Recorder`] — it never
//! touches the world.
//!
//! # Feature tiers
//!
//! The crate is organised so that pulling in metrics never forces a dependency
//! you do not use:
//!
//! | Feature   | Module                       | Pulls in       | What you get |
//! |-----------|------------------------------|----------------|--------------|
//! | *(none)*  | [`stats`], [`distribution`], [`agreement`] | nothing | pure numeric primitives over `&[f64]` / `&[u32]`, KL divergence & chi-square homogeneity, ordinal distribution-matching (Wasserstein/NEMD/MD/SDD), and contingency-table agreement (tetrachoric, κ, ICC, Cramér's V, prop-test) |
//! | `core`    | [`opinion`]   | `socsim-core`  | capability-trait extractors + `MetricsMechanism<W>` |
//! | `network` | [`network`]   | + `socsim-net` | degree / clustering / component / cascade metrics |
//! | `spatial` | [`spatial`]   | + `socsim-grid`| Schelling-style segregation metrics |
//!
//! `network` and `spatial` both imply `core`.  Building with **no features**
//! compiles only [`stats`], which has **zero dependencies**.
//!
//! [`Recorder`]: socsim_core::Recorder

pub mod agreement;
pub mod distribution;
pub mod stats;

#[cfg(feature = "core")]
pub mod opinion;

#[cfg(feature = "network")]
pub mod network;

#[cfg(feature = "spatial")]
pub mod spatial;
