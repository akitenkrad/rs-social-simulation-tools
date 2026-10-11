#[cfg_attr(not(feature = "engine"), allow(dead_code))]
mod decide;
#[cfg_attr(not(feature = "engine"), allow(dead_code))]
mod matching;
mod metrics;
#[cfg_attr(not(feature = "engine"), allow(dead_code))]
mod play;

pub use decide::RuleDriver;
#[cfg(feature = "engine")]
pub(crate) use matching::MatchMechanism;
pub use metrics::{EquilibriumTarget, GameMetrics, MetricsOptions, MixedSource};
#[cfg(feature = "engine")]
pub(crate) use play::PlayMechanism;
