//! Core traits and types for the `socsim` social-simulation platform.
//!
//! This crate defines the fundamental abstractions that all other `socsim`
//! crates build on:
//!
//! - [`AgentId`] — opaque agent identifier.
//! - [`SimClock`] — discrete-time counter.
//! - [`Phase`] / [`Phase::ORDER`] — the six-phase execution loop.
//! - [`WorldState`], [`Mechanism`], [`Scheduler`], [`Recorder`] — the four
//!   composable traits researchers implement.
//! - [`StepContext`] — context bundle passed into every [`Mechanism::apply`]
//!   call.
//! - [`SocsimError`] / [`Result`] — unified error type.
//!
//! `socsim-rng`'s [`SimRng`] is re-exported here so downstream crates only
//! need to depend on `socsim-core`.

pub use socsim_rng::SimRng;

// ── AgentId ──────────────────────────────────────────────────────────────────

/// Opaque, cheaply-copyable identifier for a simulation agent.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct AgentId(pub u64);

// ── SimClock ─────────────────────────────────────────────────────────────────

/// Discrete-time clock.
///
/// Passed **by value** into [`StepContext`] so that mechanisms can read the
/// current time without holding a shared reference to `world`, which would
/// prevent the mutable `world` borrow inside the same context.
#[derive(Clone, Copy, Debug)]
pub struct SimClock {
    t: u64,
    t_max: u64,
}

impl SimClock {
    /// Create a new clock that will run from `t = 0` to `t = t_max` (exclusive).
    pub fn new(t_max: u64) -> Self {
        Self { t: 0, t_max }
    }

    /// Current simulation time step.
    pub fn t(&self) -> u64 {
        self.t
    }

    /// Maximum simulation time (exclusive upper bound).
    pub fn t_max(&self) -> u64 {
        self.t_max
    }

    /// Returns `true` when the simulation has reached or passed `t_max`.
    pub fn is_done(&self) -> bool {
        self.t >= self.t_max
    }

    /// Advance time by one step.
    pub fn tick(&mut self) {
        self.t += 1;
    }
}

// ── Phase ─────────────────────────────────────────────────────────────────────

/// The six execution phases of one simulation step.
///
/// Mechanisms register the phases they participate in; the engine calls them
/// in [`Phase::ORDER`] within each step.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Phase {
    /// Setup / bookkeeping before the main phases.
    PreStep,
    /// Global environment update (e.g. exogenous shocks, resource replenishment).
    Environment,
    /// Agents decide their next action.
    Decision,
    /// Agents interact with each other (peer effects, network diffusion).
    Interaction,
    /// Rewards are computed and applied.
    Reward,
    /// Cleanup / logging after all agents have acted.
    PostStep,
}

impl Phase {
    /// Canonical execution order for one step.
    pub const ORDER: [Phase; 6] = [
        Phase::PreStep,
        Phase::Environment,
        Phase::Decision,
        Phase::Interaction,
        Phase::Reward,
        Phase::PostStep,
    ];
}

// ── Error / Result ────────────────────────────────────────────────────────────

/// Unified error type for all `socsim` crates.
#[derive(Debug, thiserror::Error)]
pub enum SocsimError {
    /// A configuration or parameter error.
    #[error("config error: {0}")]
    Config(String),

    /// An error raised inside a [`Mechanism::apply`] call.
    #[error("mechanism error: {0}")]
    Mechanism(String),

    /// Attempt to look up a mechanism that has not been registered.
    #[error("unknown mechanism: '{0}'")]
    UnknownMechanism(String),
}

/// Convenience alias used throughout the `socsim` workspace.
pub type Result<T> = std::result::Result<T, SocsimError>;

// ── WorldState ────────────────────────────────────────────────────────────────

/// Shared environment state.
///
/// Researchers implement this trait to define the concrete world their
/// simulation operates on.  The world owns the [`SimClock`] and the agent
/// roster; everything else is domain-specific.
pub trait WorldState: 'static {
    /// Returns all agent identifiers currently active in the simulation.
    fn agent_ids(&self) -> Vec<AgentId>;

    /// Returns a shared reference to the simulation clock.
    fn clock(&self) -> &SimClock;

    /// Returns a mutable reference to the simulation clock.
    fn clock_mut(&mut self) -> &mut SimClock;
}

// ── Recorder ─────────────────────────────────────────────────────────────────

/// Sink for metrics and events recorded during a simulation run.
///
/// Implement this trait to route output to memory (for tests), JSONL files,
/// databases, etc.
pub trait Recorder {
    /// Record a scalar metric at time `t` under `key`.
    fn record_metric(&mut self, t: u64, key: &str, value: f64);

    /// Record a structured event at time `t`.
    fn record_event(&mut self, t: u64, kind: &str, payload: serde_json::Value);

    /// Optional downcast support.  Returns `Some(&dyn std::any::Any)` when the
    /// concrete type supports it; `None` by default.
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        None
    }
}

// ── StepContext ───────────────────────────────────────────────────────────────

/// Execution context passed to every [`Mechanism::apply`] call.
///
/// `clock` is a **copy** of the world's current clock so that mechanisms can
/// read the time without holding a second borrow on `world`.
pub struct StepContext<'a, W: WorldState> {
    /// Mutable access to the shared world state.
    pub world: &'a mut W,
    /// Snapshot of the clock at the start of this step (value copy).
    pub clock: SimClock,
    /// Per-step RNG derived from the root seed.
    pub rng: &'a mut SimRng,
    /// Metric / event recorder.
    pub recorder: &'a mut dyn Recorder,
    /// Activation order for this step as decided by the [`Scheduler`].
    pub agent_order: &'a [AgentId],
    /// Step-scoped scratch space, cleared by the engine at the start of every
    /// step.  Use it to pass transient values between mechanisms (or out to the
    /// driver) without polluting [`WorldState`] with per-step bookkeeping.
    pub scratch: &'a mut Blackboard,
    /// Stop flag.  A mechanism calls [`StepContext::request_stop`] to ask the
    /// engine to terminate the run after the current step completes.
    pub stop: &'a mut bool,
}

impl<W: WorldState> StepContext<'_, W> {
    /// Request that the simulation stop after the current step finishes.
    ///
    /// All remaining mechanisms in the current step still run; the engine
    /// checks the flag once the step completes (see
    /// [`Simulation::run`](../socsim_engine/struct.Simulation.html#method.run)).
    pub fn request_stop(&mut self) {
        *self.stop = true;
    }
}

// ── Blackboard ────────────────────────────────────────────────────────────────

/// Step-scoped, type-erased key/value store handed to mechanisms via
/// [`StepContext::scratch`].
///
/// The engine clears it at the start of every step, so values written during a
/// step are visible to later mechanisms in the same step and to the driver
/// immediately after [`Simulation::step`](../socsim_engine/struct.Simulation.html#method.step)
/// returns — but not on the next step.
#[derive(Default)]
pub struct Blackboard {
    map: std::collections::HashMap<&'static str, Box<dyn std::any::Any>>,
}

impl Blackboard {
    /// Create an empty blackboard.
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a value under `key`, replacing any previous value.
    pub fn insert<T: std::any::Any>(&mut self, key: &'static str, value: T) {
        self.map.insert(key, Box::new(value));
    }

    /// Borrow the value stored under `key`, if present and of type `T`.
    pub fn get<T: std::any::Any>(&self, key: &'static str) -> Option<&T> {
        self.map.get(key).and_then(|b| b.downcast_ref::<T>())
    }

    /// Mutably borrow the value stored under `key`, if present and of type `T`.
    pub fn get_mut<T: std::any::Any>(&mut self, key: &'static str) -> Option<&mut T> {
        self.map.get_mut(key).and_then(|b| b.downcast_mut::<T>())
    }

    /// Remove all entries.  Called by the engine at the start of each step.
    pub fn clear(&mut self) {
        self.map.clear();
    }
}

// ── Mechanism ─────────────────────────────────────────────────────────────────

/// A composable transformation unit — the analogue of a PyTorch layer.
///
/// Each mechanism registers one or more [`Phase`]s it participates in.  The
/// engine calls [`Mechanism::apply`] once per registered phase, in
/// [`Phase::ORDER`], passing a fully populated [`StepContext`].
///
/// # Example
/// ```ignore
/// struct GrowthMechanism { rate: f64 }
///
/// impl Mechanism<MyWorld> for GrowthMechanism {
///     fn name(&self) -> &str { "growth" }
///     fn phases(&self) -> &'static [Phase] { &[Phase::Environment] }
///     fn apply(&mut self, _phase: Phase, ctx: &mut StepContext<'_, MyWorld>) -> Result<()> {
///         ctx.world.value += self.rate;
///         Ok(())
///     }
/// }
/// ```
pub trait Mechanism<W: WorldState> {
    /// Human-readable name used for logging and registry lookup.
    fn name(&self) -> &str;

    /// The phases this mechanism participates in.  Must be a `'static` slice
    /// so no allocation is needed on the hot path.
    fn phases(&self) -> &'static [Phase];

    /// Apply the mechanism's logic for `phase`.
    ///
    /// Called once per registered phase per step, in [`Phase::ORDER`].
    fn apply(&mut self, phase: Phase, ctx: &mut StepContext<'_, W>) -> Result<()>;
}

// ── Scheduler ─────────────────────────────────────────────────────────────────

/// Determines the agent activation order for each step.
///
/// The choice of scheduler affects simulation outcomes for interaction-heavy
/// mechanisms (e.g. contagion, peer effects).
pub trait Scheduler<W: WorldState> {
    /// Return the ordered list of agents to activate this step.
    fn activation_order(&mut self, world: &W, rng: &mut SimRng) -> Vec<AgentId>;
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_ticks_and_detects_done() {
        let mut c = SimClock::new(3);
        assert!(!c.is_done());
        c.tick();
        c.tick();
        c.tick();
        assert!(c.is_done());
    }

    #[test]
    fn clock_is_copy() {
        let c = SimClock::new(10);
        let copy = c; // Copy trait — original still usable
        assert_eq!(c.t(), copy.t());
    }

    #[test]
    fn phase_order_has_six_elements() {
        assert_eq!(Phase::ORDER.len(), 6);
    }

    #[test]
    fn agent_id_ord() {
        let a = AgentId(1);
        let b = AgentId(2);
        assert!(a < b);
    }

    #[test]
    fn blackboard_round_trips_typed_values() {
        let mut bb = Blackboard::new();
        bb.insert("count", 42u32);
        bb.insert("label", "hello".to_string());
        assert_eq!(bb.get::<u32>("count"), Some(&42));
        assert_eq!(bb.get::<String>("label").map(String::as_str), Some("hello"));
        // Wrong type → None.
        assert_eq!(bb.get::<i64>("count"), None);
        // Missing key → None.
        assert_eq!(bb.get::<u32>("missing"), None);
    }

    #[test]
    fn blackboard_clear_removes_all() {
        let mut bb = Blackboard::new();
        bb.insert("x", 1u8);
        bb.clear();
        assert_eq!(bb.get::<u8>("x"), None);
    }
}
