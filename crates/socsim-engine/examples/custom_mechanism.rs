//! Demonstrates how to write and register a custom `Mechanism`.
//!
//! This example mirrors the integration test in `tests/custom_mechanism.rs`
//! but with explanatory `println!` output so you can follow along.
//!
//! Run with:
//!   cargo run -p socsim-engine --example custom_mechanism

use socsim_config::{ModulePack, Params, Registry};
use socsim_core::{AgentId, Mechanism, Phase, Recorder, Result, SimClock, StepContext, WorldState};
use socsim_engine::{RandomActivationScheduler, SimulationBuilder};

// ─────────────────────────────────────────────────────────────────────────────
// Step 1 — Define your WorldState
//
// A WorldState holds all shared simulation state: agents, the clock, and any
// domain data your mechanisms need to read/write.
// ─────────────────────────────────────────────────────────────────────────────

struct CounterWorld {
    clock: SimClock,
    agents: Vec<AgentId>,
    /// A running total that our GrowthMechanism will increment each step.
    pub value: f64,
}

impl CounterWorld {
    fn new(t_max: u64) -> Self {
        Self {
            clock: SimClock::new(t_max),
            agents: vec![AgentId(0)],
            value: 0.0,
        }
    }
}

impl WorldState for CounterWorld {
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

// ─────────────────────────────────────────────────────────────────────────────
// Step 2 — Implement a custom Mechanism
//
// A Mechanism is the unit of research logic.  It declares which Phase(s) it
// runs in and receives a `StepContext` with mutable world access, the RNG, and
// the recorder.
// ─────────────────────────────────────────────────────────────────────────────

/// Adds a fixed `rate` to `world.value` every `Environment` phase.
struct GrowthMechanism {
    rate: f64,
}

impl Mechanism<CounterWorld> for GrowthMechanism {
    fn name(&self) -> &str {
        "growth"
    }

    fn phases(&self) -> &'static [Phase] {
        // This mechanism only participates in the Environment phase.
        &[Phase::Environment]
    }

    fn apply(&mut self, _phase: Phase, ctx: &mut StepContext<'_, CounterWorld>) -> Result<()> {
        ctx.world.value += self.rate;
        // Record the current value as a metric so we can inspect the time series.
        ctx.recorder
            .record_metric(ctx.clock.t(), "value", ctx.world.value);
        Ok(())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Step 3 — Bundle mechanisms into a ModulePack (optional but recommended)
//
// A ModulePack groups related mechanisms and registers them all at once.
// This is the socsim equivalent of a PyTorch nn.Module library.
// ─────────────────────────────────────────────────────────────────────────────

struct DemoPack;

impl ModulePack<CounterWorld> for DemoPack {
    fn pack_name(&self) -> &str {
        "demo"
    }

    fn register(&self, reg: &mut Registry<CounterWorld>) {
        // Each closure is a constructor: reads params, returns a boxed Mechanism.
        reg.register("growth", |params| {
            let rate = params.get_f64("rate", 1.0); // default rate = 1.0
            Ok(Box::new(GrowthMechanism { rate }))
        });
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Step 4 — Wire everything together
// ─────────────────────────────────────────────────────────────────────────────

/// Simple in-process recorder that prints each metric line.
struct PrintingRecorder;

impl Recorder for PrintingRecorder {
    fn record_metric(&mut self, t: u64, key: &str, value: f64) {
        println!("  [t={t:3}] metric  {key} = {value}");
    }
    fn record_event(&mut self, t: u64, kind: &str, payload: serde_json::Value) {
        println!("  [t={t:3}] event   {kind} {payload}");
    }
}

fn main() -> std::process::ExitCode {
    println!("=== socsim custom_mechanism example ===\n");

    // ── 4a. Create a Registry and register via the ModulePack ─────────────────
    let mut reg: Registry<CounterWorld> = Registry::new();
    let pack = DemoPack;
    pack.register(&mut reg);

    println!("Registered mechanisms: {:?}", {
        let mut names = reg.names();
        names.sort();
        names
    });

    // ── 4b. Build the mechanism from the registry using default params ─────────
    let params = Params::empty(); // or Params::from(toml_table)
    let growth = match reg.build("growth", &params) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("Error building mechanism: {e}");
            return std::process::ExitCode::FAILURE;
        }
    };
    println!("Built mechanism: '{}'", growth.name());

    // ── 4c. Assemble the Simulation via SimulationBuilder ─────────────────────
    let world = CounterWorld::new(10); // run for 10 steps
    let mut sim = SimulationBuilder::new(world)
        .add_mechanism(growth)
        .scheduler(Box::new(RandomActivationScheduler))
        .seed(42) // fixed seed → deterministic
        .recorder(Box::new(PrintingRecorder))
        .build();

    println!("\nRunning simulation (T=10, seed=42) …\n");

    // ── 4d. Run to completion ─────────────────────────────────────────────────
    if let Err(e) = sim.run() {
        eprintln!("Simulation error: {e}");
        return std::process::ExitCode::FAILURE;
    }

    println!("\nFinal value: {}", sim.world().value);

    // ── 4e. Verify determinism: re-run with same seed, must give same result ───
    println!("\n--- Determinism check (same seed = same result) ---");
    let mut reg2: Registry<CounterWorld> = Registry::new();
    DemoPack.register(&mut reg2);
    let growth2 = reg2.build("growth", &Params::empty()).unwrap();
    let world2 = CounterWorld::new(10);
    let mut sim2 = SimulationBuilder::new(world2)
        .add_mechanism(growth2)
        .scheduler(Box::new(RandomActivationScheduler))
        .seed(42)
        .recorder(Box::new(PrintingRecorder))
        .build();
    sim2.run().unwrap();

    let v1 = sim.world().value;
    let v2 = sim2.world().value;
    if (v1 - v2).abs() < 1e-12 {
        println!("\nDeterminism OK: both runs produced value = {v1}");
    } else {
        eprintln!("DETERMINISM FAILURE: {v1} != {v2}");
        return std::process::ExitCode::FAILURE;
    }

    std::process::ExitCode::SUCCESS
}
