//! Simulation engine for `socsim`.
//!
//! Provides:
//!
//! - [`SequentialScheduler`] — activates agents in sorted `AgentId` order.
//! - [`RandomActivationScheduler`] — shuffles agents each step using the RNG.
//! - [`Simulation`] — drives the 6-phase execution loop.
//! - [`SimulationBuilder`] — fluent builder with sensible defaults.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use socsim_core::{
    AgentId, Blackboard, Mechanism, Phase, Recorder, Result, Scheduler, SimRng, SocsimError,
    StepContext, WorldState,
};
use socsim_log::InMemoryRecorder;

// ── SequentialScheduler ───────────────────────────────────────────────────────

/// Activates agents in ascending [`AgentId`] order every step.
///
/// Deterministic and order-stable; useful when interaction order must not vary.
pub struct SequentialScheduler;

impl<W: WorldState> Scheduler<W> for SequentialScheduler {
    fn activation_order(&mut self, world: &W, _rng: &mut SimRng) -> Vec<AgentId> {
        let mut ids = world.agent_ids();
        ids.sort();
        ids
    }
}

// ── RandomActivationScheduler ─────────────────────────────────────────────────

/// Shuffles the agent activation order each step using the simulation RNG.
///
/// Standard ABM scheduler that avoids systematic first-mover advantages.
pub struct RandomActivationScheduler;

impl<W: WorldState> Scheduler<W> for RandomActivationScheduler {
    fn activation_order(&mut self, world: &W, rng: &mut SimRng) -> Vec<AgentId> {
        use rand::seq::SliceRandom;
        let mut ids = world.agent_ids();
        ids.shuffle(rng);
        ids
    }
}

// ── Simulation ────────────────────────────────────────────────────────────────

/// The main simulation driver.
///
/// Owns the world, all mechanisms, the scheduler, the RNG, and the recorder.
/// Advance the simulation with [`Simulation::step`] (one step) or
/// [`Simulation::run`] (run to completion).
///
/// Construct via [`SimulationBuilder`].
pub struct Simulation<W: WorldState> {
    world: W,
    mechanisms: Vec<Box<dyn Mechanism<W>>>,
    scheduler: Box<dyn Scheduler<W>>,
    rng: SimRng,
    recorder: Box<dyn Recorder>,
    /// Step-scoped scratch space (cleared at the start of every step).
    scratch: Blackboard,
    /// Set to `true` when a mechanism calls
    /// [`StepContext::request_stop`](socsim_core::StepContext::request_stop).
    stop_requested: bool,
}

impl<W: WorldState> Simulation<W> {
    /// Execute one simulation step.
    ///
    /// Order of operations:
    /// 1. Tick the clock (`t += 1`).
    /// 2. Ask the scheduler for the agent activation order.
    /// 3. For each phase in [`Phase::ORDER`], invoke every mechanism that
    ///    registered that phase (in insertion order).
    pub fn step(&mut self) -> Result<()> {
        // Advance time first so mechanisms observe the new `t`.
        self.world.clock_mut().tick();

        // Snapshot the clock *after* ticking so ctx.clock reflects the
        // current step.  This is a Copy so it doesn't conflict with the
        // `&mut world` borrow below.
        let clock_snapshot = *self.world.clock();

        // Clear step-scoped scratch so values from the previous step don't
        // leak into this one.  Values written this step remain readable by the
        // driver until the next `step()` call.
        self.scratch.clear();

        // Determine activation order.  We need a shared borrow of world here,
        // which is fine because we drop it before taking the mutable borrow
        // inside the phase loop.
        let order = self.scheduler.activation_order(&self.world, &mut self.rng);

        // Execute phases.
        for &phase in &Phase::ORDER {
            for mech in &mut self.mechanisms {
                if mech.phases().contains(&phase) {
                    let mut ctx = StepContext {
                        world: &mut self.world,
                        clock: clock_snapshot,
                        rng: &mut self.rng,
                        recorder: self.recorder.as_mut(),
                        agent_order: &order,
                        scratch: &mut self.scratch,
                        stop: &mut self.stop_requested,
                    };
                    mech.apply(phase, &mut ctx)?;
                }
            }
        }

        Ok(())
    }

    /// Run the simulation to completion.
    ///
    /// Stops when **either** the clock reaches `t_max`
    /// ([`SimClock::is_done`](socsim_core::SimClock::is_done)) **or** a
    /// mechanism has requested a stop via
    /// [`StepContext::request_stop`](socsim_core::StepContext::request_stop).
    pub fn run(&mut self) -> Result<()> {
        while !self.world.clock().is_done() && !self.stop_requested {
            self.step()?;
        }
        Ok(())
    }

    /// Run until the clock is done, a stop is requested, **or** `predicate`
    /// returns `true` when evaluated against the world after a step.
    ///
    /// The predicate is checked *after* each step, so the simulation always
    /// advances at least one step before it can terminate via the predicate.
    /// This is the idiomatic way to stop on convergence:
    ///
    /// ```ignore
    /// sim.run_until(|w| w.is_converged())?;
    /// ```
    pub fn run_until<F>(&mut self, predicate: F) -> Result<()>
    where
        F: Fn(&W) -> bool,
    {
        while !self.world.clock().is_done() && !self.stop_requested {
            self.step()?;
            if predicate(&self.world) {
                break;
            }
        }
        Ok(())
    }

    /// Returns `true` if a mechanism has requested the run to stop.
    pub fn stop_requested(&self) -> bool {
        self.stop_requested
    }

    /// Shared reference to the step-scoped scratch space.  Most useful right
    /// after [`Simulation::step`] to read values a mechanism left for the driver.
    pub fn scratch(&self) -> &Blackboard {
        &self.scratch
    }

    /// Shared reference to the world state.
    pub fn world(&self) -> &W {
        &self.world
    }

    /// Shared reference to the recorder.
    pub fn recorder(&self) -> &dyn Recorder {
        self.recorder.as_ref()
    }

    /// Mutable reference to the recorder, e.g. to downcast it for inspection.
    pub fn recorder_mut(&mut self) -> &mut dyn Recorder {
        self.recorder.as_mut()
    }
}

// ── Snapshot ──────────────────────────────────────────────────────────────────

/// On-disk format version for [`Snapshot`].  Bumped on any breaking change to
/// the snapshot layout; [`Snapshot::load`] rejects mismatched versions.
pub const SNAPSHOT_VERSION: u32 = 1;

/// A serialisable capture of a simulation's **mutable state** — the analogue of
/// a PyTorch `state_dict` (§6.1).
///
/// It holds the world (which owns the [`SimClock`](socsim_core::SimClock)), the
/// exact RNG stream position, and the early-stop flag.  It deliberately does
/// **not** capture mechanisms, the scheduler, or the recorder: those are *code*
/// (the model architecture), supplied when the simulation is rebuilt.  Restoring
/// a snapshot into a [`Simulation`] wired with the same mechanisms reproduces
/// the run bit-identically from the saved step onward.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Snapshot<W> {
    /// Format version, checked on [`Snapshot::load`].
    pub version: u32,
    /// Captured world state (includes the clock).
    pub world: W,
    /// Exact RNG stream state (seed + word position).
    pub rng: SimRng,
    /// Whether a mechanism had requested an early stop.
    pub stop_requested: bool,
}

impl<W: WorldState + Clone> Simulation<W> {
    /// Capture the current mutable state as an in-memory [`Snapshot`].
    ///
    /// Clones the world and RNG; the simulation is left untouched and can keep
    /// running.  Requires `W: Clone`.
    pub fn snapshot(&self) -> Snapshot<W> {
        Snapshot {
            version: SNAPSHOT_VERSION,
            world: self.world.clone(),
            rng: self.rng.clone(),
            stop_requested: self.stop_requested,
        }
    }
}

impl<W: WorldState> Simulation<W> {
    /// Overwrite this simulation's state with `snapshot`'s.
    ///
    /// Replaces the world, RNG stream, and stop flag, and clears the step-scoped
    /// scratch.  Mechanisms, scheduler, and recorder are kept as-is — restore
    /// into a simulation built with the **same** mechanisms to resume exactly.
    pub fn restore(&mut self, snapshot: Snapshot<W>) {
        self.world = snapshot.world;
        self.rng = snapshot.rng;
        self.stop_requested = snapshot.stop_requested;
        self.scratch.clear();
    }
}

impl<W: Serialize> Snapshot<W> {
    /// Serialise this snapshot to a pretty-printed JSON file.
    pub fn save<P: AsRef<std::path::Path>>(&self, path: P) -> Result<()> {
        let file = std::fs::File::create(path)
            .map_err(|e| SocsimError::Snapshot(format!("create: {e}")))?;
        serde_json::to_writer_pretty(std::io::BufWriter::new(file), self)
            .map_err(|e| SocsimError::Snapshot(format!("serialise: {e}")))
    }
}

impl<W: DeserializeOwned> Snapshot<W> {
    /// Load a snapshot from a JSON file, rejecting a mismatched
    /// [`SNAPSHOT_VERSION`].
    pub fn load<P: AsRef<std::path::Path>>(path: P) -> Result<Self> {
        let file =
            std::fs::File::open(path).map_err(|e| SocsimError::Snapshot(format!("open: {e}")))?;
        let snap: Snapshot<W> = serde_json::from_reader(std::io::BufReader::new(file))
            .map_err(|e| SocsimError::Snapshot(format!("deserialise: {e}")))?;
        if snap.version != SNAPSHOT_VERSION {
            return Err(SocsimError::Snapshot(format!(
                "version mismatch: file is v{}, expected v{SNAPSHOT_VERSION}",
                snap.version
            )));
        }
        Ok(snap)
    }
}

// ── SimulationBuilder ─────────────────────────────────────────────────────────

/// Fluent builder for [`Simulation`].
///
/// # Defaults
///
/// | Option | Default |
/// |---|---|
/// | scheduler | [`SequentialScheduler`] |
/// | seed | `0` |
/// | recorder | [`InMemoryRecorder`] |
///
/// # Example
///
/// ```ignore
/// let sim = SimulationBuilder::new(my_world)
///     .add_mechanism(Box::new(GrowthMechanism { rate: 0.1 }))
///     .scheduler(Box::new(RandomActivationScheduler))
///     .seed(42)
///     .build();
/// ```
pub struct SimulationBuilder<W: WorldState> {
    world: W,
    mechanisms: Vec<Box<dyn Mechanism<W>>>,
    scheduler: Option<Box<dyn Scheduler<W>>>,
    seed: u64,
    recorder: Option<Box<dyn Recorder>>,
}

impl<W: WorldState> SimulationBuilder<W> {
    /// Create a builder for `world`.
    pub fn new(world: W) -> Self {
        Self {
            world,
            mechanisms: Vec::new(),
            scheduler: None,
            seed: 0,
            recorder: None,
        }
    }

    /// Append a mechanism.  Mechanisms are invoked in insertion order within
    /// each phase.
    pub fn add_mechanism(mut self, m: Box<dyn Mechanism<W>>) -> Self {
        self.mechanisms.push(m);
        self
    }

    /// Override the default [`SequentialScheduler`].
    pub fn scheduler(mut self, s: Box<dyn Scheduler<W>>) -> Self {
        self.scheduler = Some(s);
        self
    }

    /// Set the root RNG seed (default: `0`).
    pub fn seed(mut self, seed: u64) -> Self {
        self.seed = seed;
        self
    }

    /// Override the default [`InMemoryRecorder`].
    pub fn recorder(mut self, r: Box<dyn Recorder>) -> Self {
        self.recorder = Some(r);
        self
    }

    /// Consume the builder and produce a [`Simulation`].
    pub fn build(self) -> Simulation<W> {
        Simulation {
            world: self.world,
            mechanisms: self.mechanisms,
            scheduler: self
                .scheduler
                .unwrap_or_else(|| Box::new(SequentialScheduler)),
            rng: SimRng::from_seed(self.seed),
            recorder: self
                .recorder
                .unwrap_or_else(|| Box::new(InMemoryRecorder::new())),
            scratch: Blackboard::new(),
            stop_requested: false,
        }
    }
}

// ── unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use socsim_core::{SimClock, SocsimError};
    use socsim_log::InMemoryRecorder;

    // ── minimal test world ────────────────────────────────────────────────────

    struct SimpleWorld {
        clock: SimClock,
        agents: Vec<AgentId>,
        counter: u32,
    }

    impl SimpleWorld {
        fn new(t_max: u64, n: u64) -> Self {
            Self {
                clock: SimClock::new(t_max),
                agents: (0..n).map(AgentId).collect(),
                counter: 0,
            }
        }
    }

    impl WorldState for SimpleWorld {
        fn agent_ids(&self) -> Vec<AgentId> {
            self.agents.clone()
        }
        fn clock(&self) -> &SimClock {
            &self.clock
        }
        fn clock_mut(&mut self) -> &mut SimClock {
            &mut self.clock
        }
    }

    // ── a mechanism that counts apply() calls ─────────────────────────────────

    struct CountMechanism;

    impl Mechanism<SimpleWorld> for CountMechanism {
        fn name(&self) -> &str {
            "counter"
        }
        fn phases(&self) -> &'static [Phase] {
            &[Phase::Environment]
        }
        fn apply(&mut self, _phase: Phase, ctx: &mut StepContext<'_, SimpleWorld>) -> Result<()> {
            ctx.world.counter += 1;
            Ok(())
        }
    }

    #[test]
    fn run_increments_counter_once_per_step() {
        let world = SimpleWorld::new(5, 3);
        let mut sim = SimulationBuilder::new(world)
            .add_mechanism(Box::new(CountMechanism))
            .build();
        sim.run().unwrap();
        assert_eq!(sim.world().counter, 5);
    }

    #[test]
    fn sequential_scheduler_sorts_agent_ids() {
        let world = SimpleWorld::new(1, 4);
        let mut sched = SequentialScheduler;
        let mut rng = SimRng::from_seed(0);
        let order = sched.activation_order(&world, &mut rng);
        assert_eq!(order, vec![AgentId(0), AgentId(1), AgentId(2), AgentId(3)]);
    }

    #[test]
    fn random_scheduler_produces_same_order_for_same_seed() {
        let world = SimpleWorld::new(1, 5);
        let mut sched_a = RandomActivationScheduler;
        let mut sched_b = RandomActivationScheduler;
        let mut rng_a = SimRng::from_seed(99);
        let mut rng_b = SimRng::from_seed(99);
        let order_a = sched_a.activation_order(&world, &mut rng_a);
        let order_b = sched_b.activation_order(&world, &mut rng_b);
        assert_eq!(order_a, order_b);
    }

    // ── a mechanism that propagates an error ──────────────────────────────────

    struct ErrorMechanism;

    impl Mechanism<SimpleWorld> for ErrorMechanism {
        fn name(&self) -> &str {
            "error"
        }
        fn phases(&self) -> &'static [Phase] {
            &[Phase::PreStep]
        }
        fn apply(&mut self, _phase: Phase, _ctx: &mut StepContext<'_, SimpleWorld>) -> Result<()> {
            Err(SocsimError::Mechanism("intentional".to_owned()))
        }
    }

    #[test]
    fn step_propagates_mechanism_error() {
        let world = SimpleWorld::new(5, 1);
        let mut sim = SimulationBuilder::new(world)
            .add_mechanism(Box::new(ErrorMechanism))
            .build();
        assert!(sim.step().is_err());
    }

    #[test]
    fn recorder_accessible_after_run() {
        let world = SimpleWorld::new(3, 1);
        let rec = InMemoryRecorder::new();
        let mut sim = SimulationBuilder::new(world)
            .recorder(Box::new(rec))
            .build();
        sim.run().unwrap();
        // Just check the accessor compiles and doesn't panic.
        let _ = sim.recorder();
    }

    // ── #1: early stop ────────────────────────────────────────────────────────

    /// A mechanism that requests a stop once `world.counter` reaches a target.
    struct StopAtMechanism {
        target: u32,
    }

    impl Mechanism<SimpleWorld> for StopAtMechanism {
        fn name(&self) -> &str {
            "stop_at"
        }
        fn phases(&self) -> &'static [Phase] {
            &[Phase::PostStep]
        }
        fn apply(&mut self, _phase: Phase, ctx: &mut StepContext<'_, SimpleWorld>) -> Result<()> {
            if ctx.world.counter >= self.target {
                ctx.request_stop();
            }
            Ok(())
        }
    }

    #[test]
    fn request_stop_halts_run_before_t_max() {
        // t_max = 100 but we stop as soon as counter hits 3.
        let world = SimpleWorld::new(100, 1);
        let mut sim = SimulationBuilder::new(world)
            .add_mechanism(Box::new(CountMechanism)) // counter += 1 each step
            .add_mechanism(Box::new(StopAtMechanism { target: 3 }))
            .build();
        sim.run().unwrap();
        assert!(sim.stop_requested());
        assert_eq!(sim.world().counter, 3);
        assert!(sim.world().clock().t() < sim.world().clock().t_max());
    }

    #[test]
    fn run_until_stops_on_predicate() {
        let world = SimpleWorld::new(100, 1);
        let mut sim = SimulationBuilder::new(world)
            .add_mechanism(Box::new(CountMechanism))
            .build();
        sim.run_until(|w| w.counter >= 5).unwrap();
        assert_eq!(sim.world().counter, 5);
        assert!(sim.world().clock().t() < 100);
    }

    // ── #6: step-scoped scratch ────────────────────────────────────────────────

    /// Writes a transient value into the blackboard each step.
    struct ScratchWriter;

    impl Mechanism<SimpleWorld> for ScratchWriter {
        fn name(&self) -> &str {
            "scratch_writer"
        }
        fn phases(&self) -> &'static [Phase] {
            &[Phase::Decision]
        }
        fn apply(&mut self, _phase: Phase, ctx: &mut StepContext<'_, SimpleWorld>) -> Result<()> {
            let t = ctx.clock.t();
            ctx.scratch.insert("last_t", t);
            Ok(())
        }
    }

    #[test]
    fn scratch_is_readable_by_driver_after_step_and_cleared_next_step() {
        let world = SimpleWorld::new(10, 1);
        let mut sim = SimulationBuilder::new(world)
            .add_mechanism(Box::new(ScratchWriter))
            .build();

        sim.step().unwrap();
        assert_eq!(sim.scratch().get::<u64>("last_t"), Some(&1));

        sim.step().unwrap();
        // Cleared at the start of step 2, then re-written with the new t.
        assert_eq!(sim.scratch().get::<u64>("last_t"), Some(&2));
    }
}
