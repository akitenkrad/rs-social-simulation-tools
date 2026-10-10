#[cfg_attr(not(feature = "engine"), allow(dead_code))]
mod decide;
#[cfg_attr(not(feature = "engine"), allow(dead_code))]
mod matching;
#[cfg_attr(not(feature = "engine"), allow(dead_code))]
mod play;

pub use decide::RuleDriver;
#[cfg(feature = "engine")]
pub(crate) use matching::MatchMechanism;
#[cfg(feature = "engine")]
pub(crate) use play::PlayMechanism;
