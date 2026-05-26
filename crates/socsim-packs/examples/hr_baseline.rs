//! HR lifecycle baseline example.
//!
//! Demonstrates the 9-stage model running for T=60 steps and prints the key
//! metric series: `org_performance`, `avg_tenure`, `turnover_rate`, and
//! `knowledge_stock`.
//!
//! Run with:
//! ```bash
//! cargo run -p socsim-packs --example hr_baseline
//! ```

use std::sync::{Arc, Mutex};

use socsim_config::{ModulePack, Params};
use socsim_core::Recorder;
use socsim_engine::{RandomActivationScheduler, SimulationBuilder};
use socsim_packs::hr_lifecycle::{HrLifecyclePack, HrWorld};
use socsim_log::InMemoryRecorder;

// ── SharedRecorder ─────────────────────────────────────────────────────────
struct SharedRecorder(Arc<Mutex<InMemoryRecorder>>);

impl SharedRecorder {
    fn new() -> (Self, Arc<Mutex<InMemoryRecorder>>) {
        let inner = Arc::new(Mutex::new(InMemoryRecorder::new()));
        (Self(Arc::clone(&inner)), inner)
    }
}

impl Recorder for SharedRecorder {
    fn record_metric(&mut self, t: u64, key: &str, value: f64) {
        self.0.lock().unwrap().record_metric(t, key, value);
    }
    fn record_event(&mut self, t: u64, kind: &str, payload: serde_json::Value) {
        self.0.lock().unwrap().record_event(t, kind, payload);
    }
}

fn main() {
    const T_MAX: u64 = 60;
    const SEED: u64 = 42;
    const N_TEAMS: usize = 5;
    const TEAM_SIZE: usize = 8;

    println!("=== HR Lifecycle ABM — Baseline Run ===");
    println!(
        "Teams: {N_TEAMS}  |  Initial team size: {TEAM_SIZE}  |  T_max: {T_MAX}  |  Seed: {SEED}"
    );
    println!();

    let mut rng = socsim_core::SimRng::from_seed(SEED);
    let mut world = HrWorld::new(N_TEAMS, TEAM_SIZE, 4, 0.1, &mut rng);
    world.clock = socsim_core::SimClock::new(T_MAX);

    println!(
        "Initial employees: {}  |  Base mean θ: {:.4}",
        world.employee_count(),
        world.base_mean_theta
    );
    println!();

    // Register all mechanisms.
    let mut reg = socsim_config::Registry::new();
    HrLifecyclePack.register(&mut reg);

    let p = Params::empty();
    let mechanism_names = [
        "learning_curve",
        "peer_effect",
        "ocb",
        "fit",
        "turnover",
        "knowledge_loss",
        "toxic_spread",
        "hiring",
        "socialization",
        "org_performance",
    ];

    let (shared_rec, rec_handle) = SharedRecorder::new();
    let mut builder = SimulationBuilder::new(world)
        .scheduler(Box::new(RandomActivationScheduler))
        .seed(SEED)
        .recorder(Box::new(shared_rec));

    for name in &mechanism_names {
        let m = reg.build(name, &p).expect("mechanism registered");
        builder = builder.add_mechanism(m);
    }

    let mut sim = builder.build();
    sim.run().expect("simulation completed without error");

    // Access the recorder through the Arc handle.
    let rec = rec_handle.lock().unwrap();

    // Collect metric rows per key, indexed by t.
    let mut perf_series: Vec<(u64, f64)> = rec
        .metrics()
        .iter()
        .filter(|r| r.key == "org_performance")
        .map(|r| (r.t, r.value))
        .collect();
    perf_series.sort_by_key(|r| r.0);

    let mut tenure_series: Vec<(u64, f64)> = rec
        .metrics()
        .iter()
        .filter(|r| r.key == "avg_tenure")
        .map(|r| (r.t, r.value))
        .collect();
    tenure_series.sort_by_key(|r| r.0);

    let mut turn_series: Vec<(u64, f64)> = rec
        .metrics()
        .iter()
        .filter(|r| r.key == "turnover_rate")
        .map(|r| (r.t, r.value))
        .collect();
    turn_series.sort_by_key(|r| r.0);

    let mut know_series: Vec<(u64, f64)> = rec
        .metrics()
        .iter()
        .filter(|r| r.key == "knowledge_stock")
        .map(|r| (r.t, r.value))
        .collect();
    know_series.sort_by_key(|r| r.0);

    println!(
        "{:>4}  {:>14}  {:>12}  {:>14}  {:>16}",
        "t", "org_performance", "avg_tenure", "turnover_rate", "knowledge_stock"
    );
    println!("{}", "-".repeat(70));

    for (i, &(t, perf)) in perf_series.iter().enumerate() {
        let tenure = tenure_series.get(i).map(|r| r.1).unwrap_or(f64::NAN);
        let turn = turn_series.get(i).map(|r| r.1).unwrap_or(f64::NAN);
        let know = know_series.get(i).map(|r| r.1).unwrap_or(f64::NAN);
        println!("{t:>4}  {perf:>14.4}  {tenure:>12.2}  {turn:>14.4}  {know:>16.2}");
    }

    println!();

    let final_emp = sim.world().employee_count();
    let total_events = rec.events().len();
    let turnover_events = rec.events().iter().filter(|e| e.kind == "turnover").count();
    let hiring_events = rec.events().iter().filter(|e| e.kind == "hiring").count();

    println!("Final employee count : {final_emp}");
    println!("Total events recorded: {total_events}");
    println!("  – turnover events  : {turnover_events}");
    println!("  – hiring events    : {hiring_events}");
}
