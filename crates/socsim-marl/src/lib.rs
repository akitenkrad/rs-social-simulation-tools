//! Multi-agent reinforcement learning for `socsim` (design §14.1).
//!
//! This crate makes the `Decision` phase of the six-phase loop *learnable*.
//! Because the tick loop is already RL-shaped (observe → act → reward), a
//! learning policy slots in as just another [`Mechanism`](socsim_core::Mechanism)
//! — the engine needs no changes.
//!
//! # Pieces
//!
//! | Concept | Type |
//! |---|---|
//! | Learnable policy | [`Policy`] (impl: [`DiscretePolicyNet`], a burn MLP) |
//! | World → features | [`ObsEncoder`] |
//! | Action → world | [`ActionApplier`] |
//! | Per-agent reward | [`RewardFn`] |
//! | Policy as a phase | [`PolicyMechanism`] |
//! | Trajectory storage | [`TrajectoryBuffer`] + [`Transition`] |
//! | Outer learning loop | [`MarlTrainer`] |
//!
//! # Determinism
//!
//! Network weights are initialised from a [`SimRng`](socsim_core::SimRng),
//! actions are sampled with the simulation RNG, and all tensor math runs on the
//! CPU.  A frozen policy used via [`PolicyMechanism::inference`] consumes no RNG
//! and is bit-reproducible; training with a fixed seed reproduces exactly.
//!
//! # Example
//!
//! ```ignore
//! let net = Rc::new(RefCell::new(DiscretePolicyNet::new(cfg, &mut rng)?));
//! let mut trainer = MarlTrainer::new(net);
//! let stats = trainer.train(&TrainConfig { episodes: 50, seed: 0 },
//!     |policy, buffer, seed| {
//!         let world = MyWorld::new(/* … */);
//!         SimulationBuilder::new(world)
//!             .seed(seed)
//!             .add_mechanism(Box::new(PolicyMechanism::collecting(
//!                 policy, MyEncoder, MyApplier, buffer)))
//!             .build()
//!     },
//!     &MyReward)?;
//! ```

mod buffer;
mod mechanism;
mod net;
mod policy;
mod trainer;

pub use buffer::TrajectoryBuffer;
pub use mechanism::PolicyMechanism;
pub use net::{DiscretePolicyNet, NetConfig};
pub use policy::{ActionApplier, ObsEncoder, Policy, RewardFn, Transition};
pub use trainer::{EpisodeStat, MarlTrainer, TrainConfig};
