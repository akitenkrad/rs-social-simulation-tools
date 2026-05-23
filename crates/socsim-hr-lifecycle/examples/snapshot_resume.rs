//! Pause an HR lifecycle run, save it to disk, then resume from the file
//! (design §10, Phase 6 — World snapshot save/resume).
//!
//! Run with:
//! ```bash
//! cargo run -p socsim-hr-lifecycle --example snapshot_resume
//! ```

use socsim_config::{ModulePack, Params, Registry};
use socsim_core::{Mechanism, SimClock, SimRng};
use socsim_engine::{RandomActivationScheduler, Simulation, SimulationBuilder, Snapshot};
use socsim_hr_lifecycle::{HrLifecyclePack, HrWorld};

const T_MAX: u64 = 24;
const PAUSE_AT: u64 = 12;

fn mechanisms() -> Vec<Box<dyn Mechanism<HrWorld>>> {
    let mut reg: Registry<HrWorld> = Registry::new();
    HrLifecyclePack.register(&mut reg);
    let p = Params::empty();
    [
        "learning_curve", "peer_effect", "ocb", "toxic_spread", "fit", "hiring", "turnover",
        "socialization", "knowledge_loss", "org_performance",
    ]
    .iter()
    .map(|name| reg.build(name, &p).unwrap())
    .collect()
}

fn build(seed: u64) -> Simulation<HrWorld> {
    let mut rng = SimRng::from_seed(seed ^ 0x5151);
    let mut world = HrWorld::new(3, 6, 4, 0.1, &mut rng);
    world.clock = SimClock::new(T_MAX);
    let mut builder = SimulationBuilder::new(world)
        .scheduler(Box::new(RandomActivationScheduler))
        .seed(seed);
    for m in mechanisms() {
        builder = builder.add_mechanism(m);
    }
    builder.build()
}

fn main() {
    let path = std::env::temp_dir().join("socsim_hr_snapshot.json");

    // ── Phase 1: run halfway, then save to disk ────────────────────────────────
    let mut sim = build(7);
    for _ in 0..PAUSE_AT {
        sim.step().unwrap();
    }
    println!("=== Paused at month {PAUSE_AT} ===");
    println!(
        "  headcount={}  org_performance={:.2}  knowledge={:.2}",
        sim.world().employee_count(),
        sim.world().org_performance,
        sim.world().total_knowledge_stock(),
    );
    sim.snapshot().save(&path).unwrap();
    println!("  saved snapshot → {}", path.display());

    // ── Phase 2: a *fresh* process-like simulation loads and resumes ───────────
    // Built with a different seed to show the snapshot — not this seed — drives
    // the continuation.
    let loaded: Snapshot<HrWorld> = Snapshot::load(&path).unwrap();
    let mut resumed = build(999);
    resumed.restore(loaded);
    resumed.run().unwrap();
    let _ = std::fs::remove_file(&path);

    println!("\n=== Resumed to month {T_MAX} ===");
    println!(
        "  headcount={}  org_performance={:.2}  knowledge={:.2}",
        resumed.world().employee_count(),
        resumed.world().org_performance,
        resumed.world().total_knowledge_stock(),
    );

    // Cross-check against an uninterrupted run.
    let mut full = build(7);
    full.run().unwrap();
    let identical = full.world().org_performance == resumed.world().org_performance
        && full.world().employee_count() == resumed.world().employee_count();
    println!(
        "\nResumed run matches uninterrupted run: {}",
        if identical { "YES (bit-identical)" } else { "NO" }
    );
}
