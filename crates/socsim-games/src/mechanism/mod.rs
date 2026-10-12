mod continuous_metrics;
#[cfg_attr(not(feature = "engine"), allow(dead_code))]
mod decide;
#[cfg_attr(not(feature = "engine"), allow(dead_code))]
mod matching;
mod metrics;
#[cfg_attr(not(feature = "engine"), allow(dead_code))]
mod play;
#[cfg_attr(not(feature = "engine"), allow(dead_code))]
mod population;

pub use continuous_metrics::{ContinuousEquilibrium, ContinuousMetrics, ContinuousMetricsOptions};
pub use decide::RuleDriver;
#[cfg(feature = "engine")]
pub(crate) use matching::MatchMechanism;
pub use metrics::{EquilibriumTarget, GameMetrics, MetricsOptions, MixedSource};
#[cfg(feature = "engine")]
pub(crate) use play::{
    counting_runner, pairwise_runner, AggregateCollector, KernelRunner, PlayMechanism,
};
#[cfg(feature = "engine")]
pub(crate) use population::{anonymous_payoff_range, PayoffRangeSource};
pub use population::{
    FermiUpdate, Fitness, ImitateBest, ImitationTie, MoranUpdate, ProportionalImitation, Update,
};
