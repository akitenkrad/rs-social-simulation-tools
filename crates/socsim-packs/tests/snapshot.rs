//! Snapshot save/resume on the reference HR lifecycle module (design §10, Phase 6).
//!
//! Runs the full mechanism stack with a stochastic scheduler, snapshots
//! mid-run, round-trips through JSON, restores into a freshly built simulation
//! (same mechanisms, *different* seed), and verifies the continuation matches an
//! uninterrupted run bit-for-bit — exercising the `HrWorld` + `SocialNetwork`
//! serde path end to end.

use socsim_config::{ModulePack, Params, Registry};
use socsim_core::{Mechanism, SimClock, SimRng, WorldState};
use socsim_engine::{RandomActivationScheduler, Simulation, SimulationBuilder, Snapshot};
use socsim_packs::hr_lifecycle::{HrLifecyclePack, HrWorld};

const N_TEAMS: usize = 3;
const TEAM_SIZE: usize = 6;
const T_MAX: u64 = 24;

/// Mechanisms in a fixed Phase-consistent order (insertion order matters only
/// within a phase, but we keep one list so both runs are identical).
fn mechanisms() -> Vec<Box<dyn Mechanism<HrWorld>>> {
    let mut reg: Registry<HrWorld> = Registry::new();
    HrLifecyclePack.register(&mut reg);
    let p = Params::empty();
    [
        "learning_curve",
        "peer_effect",
        "ocb",
        "toxic_spread",
        "fit",
        "hiring",
        "turnover",
        "socialization",
        "knowledge_loss",
        "org_performance",
    ]
    .iter()
    .map(|name| reg.build(name, &p).unwrap())
    .collect()
}

fn build(seed: u64) -> Simulation<HrWorld> {
    let mut rng = SimRng::from_seed(seed ^ 0x5151);
    let mut world = HrWorld::new(N_TEAMS, TEAM_SIZE, 4, 0.1, &mut rng);
    world.clock = SimClock::new(T_MAX);

    let mut builder = SimulationBuilder::new(world)
        .scheduler(Box::new(RandomActivationScheduler))
        .seed(seed);
    for m in mechanisms() {
        builder = builder.add_mechanism(m);
    }
    builder.build()
}

#[test]
fn hr_resume_from_snapshot_matches_uninterrupted_run() {
    // Reference: full 24-month run.
    let mut full = build(7);
    full.run().unwrap();
    let full_perf = full.world().org_performance;
    let full_count = full.world().employee_count();
    let full_knowledge = full.world().total_knowledge_stock();

    // Interrupted: 12 steps → snapshot → JSON round-trip.
    let mut a = build(7);
    for _ in 0..12 {
        a.step().unwrap();
    }
    let snap = a.snapshot();
    assert_eq!(snap.world.clock().t(), 12);
    let json = serde_json::to_string(&snap).unwrap();
    let snap2: Snapshot<HrWorld> = serde_json::from_str(&json).unwrap();

    // Resume into a sim built with a different seed; restore must override it.
    let mut b = build(999);
    b.restore(snap2);
    b.run().unwrap();

    assert_eq!(b.world().org_performance, full_perf, "org_performance");
    assert_eq!(b.world().employee_count(), full_count, "headcount");
    assert_eq!(
        b.world().total_knowledge_stock(),
        full_knowledge,
        "knowledge_stock"
    );
}
