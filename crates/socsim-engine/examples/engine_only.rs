//! Engine-only usage — no `ModulePack`, no scenario TOML, no `socsim-runner`.
//!
//! This is the lightweight path for embedding the engine in a tool that
//! already has its own configuration and output format (e.g. when porting an
//! existing project onto socsim). It exercises the features that make that
//! ergonomic:
//!
//! - [`Simulation::run_until`] — stop on convergence instead of running to
//!   `t_max`.
//! - [`StepContext::request_stop`] — a mechanism asking the run to end.
//! - [`StepContext::scratch`] — passing a per-step value out to the driver.
//! - [`Recorder::record_row`] + [`CsvRecorder`] — wide tabular output you can
//!   write with your own schema.
//!
//! Run with:
//!   cargo run -p socsim-engine --example engine_only

use std::collections::BTreeMap;

use socsim_core::{AgentId, Mechanism, Phase, Result, SimClock, StepContext, WorldState};
use socsim_engine::{SequentialScheduler, SimulationBuilder};
use socsim_log::CsvRecorder;

// ── A tiny "cooling" world ──────────────────────────────────────────────────
//
// Each agent holds some heat; the model converges once every agent has cooled
// to zero. No grid, no network — just enough state to show the control flow.

struct CoolingWorld {
    clock: SimClock,
    heat: BTreeMap<AgentId, f64>,
}

impl CoolingWorld {
    fn new(n: u64, t_max: u64) -> Self {
        let heat = (0..n).map(|i| (AgentId(i), (i + 2) as f64)).collect();
        Self {
            clock: SimClock::new(t_max),
            heat,
        }
    }

    /// Convergence criterion the driver polls via `run_until`.
    fn is_converged(&self) -> bool {
        self.heat.values().all(|h| *h <= 0.0)
    }
}

impl WorldState for CoolingWorld {
    fn agent_ids(&self) -> Vec<AgentId> {
        // BTreeMap keys are already sorted — matches the determinism convention.
        self.heat.keys().copied().collect()
    }
    fn clock(&self) -> &SimClock {
        &self.clock
    }
    fn clock_mut(&mut self) -> &mut SimClock {
        &mut self.clock
    }
}

// ── A mechanism that cools agents and reports progress ──────────────────────

struct CoolingMechanism {
    rate: f64,
}

impl Mechanism<CoolingWorld> for CoolingMechanism {
    fn name(&self) -> &str {
        "cooling"
    }
    fn phases(&self) -> &'static [Phase] {
        &[Phase::Decision]
    }
    fn apply(&mut self, _phase: Phase, ctx: &mut StepContext<'_, CoolingWorld>) -> Result<()> {
        let mut active = 0usize;
        let mut total = 0.0;
        for id in ctx.agent_order {
            if let Some(h) = ctx.world.heat.get_mut(id) {
                if *h > 0.0 {
                    *h = (*h - self.rate).max(0.0);
                    active += 1;
                }
                total += *h;
            }
        }

        // Hand the step's active count to the driver via step-scoped scratch.
        ctx.scratch.insert("active", active);

        // Wide tabular row — your own column schema.
        ctx.recorder.record_row(
            ctx.clock.t(),
            "cooling",
            &[("active", active as f64), ("total_heat", total)],
        );

        // Also demonstrate a mechanism-initiated stop (redundant with the
        // driver's run_until predicate, but shows the API).
        if active == 0 {
            ctx.request_stop();
        }
        Ok(())
    }
}

fn main() {
    let world = CoolingWorld::new(5, 1_000); // t_max is a safety cap we never hit
    let mut sim = SimulationBuilder::new(world)
        .scheduler(Box::new(SequentialScheduler))
        .seed(42)
        .recorder(Box::new(CsvRecorder::new()))
        .add_mechanism(Box::new(CoolingMechanism { rate: 1.0 }))
        .build();

    // Drive it ourselves and stop on convergence — not at t_max.
    sim.run_until(|w| w.is_converged())
        .expect("simulation completed");

    let last_active = sim.scratch().get::<usize>("active").copied().unwrap_or(0);
    println!(
        "converged at t = {} (t_max = {}), stop_requested = {}, last active = {}",
        sim.world().clock().t(),
        sim.world().clock().t_max(),
        sim.stop_requested(),
        last_active,
    );
    println!();

    // Emit our own CSV from the CsvRecorder — no JSONL, no runner.
    let rec = sim
        .recorder()
        .as_any()
        .and_then(|a| a.downcast_ref::<CsvRecorder>())
        .expect("recorder is a CsvRecorder");
    print!("{}", rec.table_csv("cooling").expect("table exists"));
}
