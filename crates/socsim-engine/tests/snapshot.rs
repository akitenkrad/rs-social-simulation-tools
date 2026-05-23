//! World snapshot save/resume reproducibility (design §10, Phase 6).
//!
//! A snapshot captures the world + the exact RNG stream + clock.  Restoring it
//! into a freshly built simulation (same mechanisms, *different* seed) must
//! reproduce the rest of the run bit-identically — proving the snapshot, not the
//! new simulation's seed, drives the continuation.

use rand::Rng;
use serde::{Deserialize, Serialize};
use socsim_core::{AgentId, Mechanism, Phase, Result, SimClock, StepContext, WorldState};
use socsim_engine::{
    SequentialScheduler, Simulation, SimulationBuilder, Snapshot, SNAPSHOT_VERSION,
};

// ── a serde-able world that accumulates RNG draws ────────────────────────────

#[derive(Clone, Serialize, Deserialize)]
struct AccWorld {
    clock: SimClock,
    agents: Vec<AgentId>,
    acc: f64,
}

impl AccWorld {
    fn new(t_max: u64, n: u64) -> Self {
        Self {
            clock: SimClock::new(t_max),
            agents: (0..n).map(AgentId).collect(),
            acc: 0.0,
        }
    }
}

impl WorldState for AccWorld {
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

/// Draws one RNG value per agent each step and accumulates it — so both the
/// world (`acc`) and the RNG stream position affect the outcome.
struct RngAccum;

impl Mechanism<AccWorld> for RngAccum {
    fn name(&self) -> &str {
        "rng_accum"
    }
    fn phases(&self) -> &'static [Phase] {
        &[Phase::Environment]
    }
    fn apply(&mut self, _phase: Phase, ctx: &mut StepContext<'_, AccWorld>) -> Result<()> {
        let n = ctx.world.agents.len();
        for _ in 0..n {
            ctx.world.acc += ctx.rng.gen::<f64>();
        }
        Ok(())
    }
}

fn build(seed: u64) -> Simulation<AccWorld> {
    SimulationBuilder::new(AccWorld::new(20, 3))
        .scheduler(Box::new(SequentialScheduler))
        .seed(seed)
        .add_mechanism(Box::new(RngAccum))
        .build()
}

/// Snapshot mid-run, round-trip through JSON, restore into a *different-seed*
/// simulation, and finish — the result must equal the uninterrupted run.
#[test]
fn resume_from_snapshot_matches_uninterrupted_run() {
    // Reference: run all 20 steps straight through.
    let mut full = build(7);
    full.run().unwrap();
    let full_acc = full.world().acc;

    // Interrupted: run 10 steps, snapshot, JSON round-trip.
    let mut a = build(7);
    for _ in 0..10 {
        a.step().unwrap();
    }
    let snap = a.snapshot();
    assert_eq!(snap.world.clock().t(), 10);
    let json = serde_json::to_string(&snap).unwrap();
    let snap2: Snapshot<AccWorld> = serde_json::from_str(&json).unwrap();

    // Resume into a sim built with a *different* seed; restore must override it.
    let mut b = build(999);
    b.restore(snap2);
    b.run().unwrap();

    assert_eq!(
        full_acc,
        b.world().acc,
        "resumed run must bit-match the uninterrupted run"
    );
}

/// `Snapshot::save` / `Snapshot::load` round-trip through a real file.
#[test]
fn snapshot_file_round_trip() {
    let mut sim = build(42);
    for _ in 0..5 {
        sim.step().unwrap();
    }
    let snap = sim.snapshot();

    let path = std::env::temp_dir().join(format!("socsim_snap_{}.json", std::process::id()));
    snap.save(&path).unwrap();
    let loaded: Snapshot<AccWorld> = Snapshot::load(&path).unwrap();
    let _ = std::fs::remove_file(&path);

    assert_eq!(loaded.world.acc, snap.world.acc);
    assert_eq!(loaded.world.clock().t(), 5);

    // Restoring the loaded snapshot continues identically to the live sim.
    let mut resumed = build(0);
    resumed.restore(loaded);
    resumed.run().unwrap();
    sim.run().unwrap();
    assert_eq!(resumed.world().acc, sim.world().acc);
}

/// A version mismatch is rejected by `Snapshot::load`.
#[test]
fn load_rejects_version_mismatch() {
    // Take a valid snapshot, then bump its version to a future value.
    let mut snap = build(1).snapshot();
    snap.version = SNAPSHOT_VERSION + 1;
    let path = std::env::temp_dir().join(format!("socsim_snap_bad_{}.json", std::process::id()));
    snap.save(&path).unwrap();
    let res: Result<Snapshot<AccWorld>> = Snapshot::load(&path);
    let _ = std::fs::remove_file(&path);
    assert!(res.is_err(), "future snapshot version must be rejected");
}
