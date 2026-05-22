//! Simulation engine for `socsim`.
//!
//! Provides:
//!
//! - [`SequentialScheduler`] — activates agents in sorted `AgentId` order.
//! - [`RandomActivationScheduler`] — shuffles agents each step using the RNG.
//! - [`Simulation`] — drives the 6-phase execution loop.
//! - [`SimulationBuilder`] — fluent builder with sensible defaults.

use socsim_core::{
    AgentId, Mechanism, Phase, Recorder, Result, Scheduler, SimRng, StepContext, WorldState,
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
                    };
                    mech.apply(phase, &mut ctx)?;
                }
            }
        }

        Ok(())
    }

    /// Run the simulation to completion (until `world.clock().is_done()`).
    pub fn run(&mut self) -> Result<()> {
        while !self.world.clock().is_done() {
            self.step()?;
        }
        Ok(())
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
}
