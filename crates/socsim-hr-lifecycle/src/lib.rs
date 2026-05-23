//! Reference research module: 9-stage employee lifecycle ABM.
//!
//! Implements the HR lifecycle model from the design document §9, covering:
//!
//! 1. `hiring` — selection validity, new-hire onboarding
//! 2. `socialization` — onboarding quality mediates retention
//! 3. `learning_curve` — productivity ramp `θ(1 − e^{−λ·tenure})`
//! 4. `peer_effect` — effective productivity scaled by team mean θ
//! 5. `ocb` — organisational citizenship behaviour adds to team knowledge
//! 6. `fit` — P-O/P-J fit drives satisfaction and turnover intent
//! 7. `turnover` — network-contagion quit process (Krackhardt cascade)
//! 8. `knowledge_loss` — tacit knowledge drain on departure
//! 9. `toxic_spread` — toxic worker infection along network edges
//! 10. `org_performance` — aggregate metrics (Reward phase)
//!
//! # Calibration
//!
//! All default parameter values are collected as `pub const` in the
//! [`calibration`] module together with the original source citations.
//!
//! # Usage
//!
//! ```rust,no_run
//! use socsim_hr_lifecycle::{HrWorld, HrLifecyclePack};
//! use socsim_config::{Registry, ModulePack, Params};
//!
//! let mut rng = socsim_core::SimRng::from_seed(42);
//! let world = HrWorld::new(5, 8, 4, 0.1, &mut rng);
//!
//! let mut reg = Registry::new();
//! HrLifecyclePack.register(&mut reg);
//!
//! let p = Params::empty();
//! let mechanisms: Vec<_> = [
//!     "learning_curve", "peer_effect", "ocb", "fit",
//!     "turnover", "knowledge_loss", "toxic_spread",
//!     "hiring", "socialization", "org_performance",
//! ]
//! .iter()
//! .map(|name| reg.build(name, &p).unwrap())
//! .collect();
//! ```

pub mod calibration;
#[cfg(feature = "marl")]
pub mod marl;
mod mechanisms;
mod world;

pub use mechanisms::HrLifecyclePack;
pub use world::{Employee, HrWorld, Team};
