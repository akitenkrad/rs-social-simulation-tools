//! Integration test for the HR lifecycle ABM.
//!
//! Asserts:
//! - The simulation runs T=60 steps without error.
//! - Employee count stays > 0 and bounded by a reasonable maximum.
//! - All team knowledge stocks remain finite and non-negative.
//! - Metrics are recorded for every step.
//! - Same seed ⇒ identical `org_performance` series (determinism).

use std::sync::{Arc, Mutex};

use socsim_config::{ModulePack, Params};
use socsim_core::Recorder;
use socsim_engine::{RandomActivationScheduler, SimulationBuilder};
use socsim_hr_lifecycle::{HrLifecyclePack, HrWorld};
use socsim_log::{InMemoryRecorder, MetricRow};

const T_MAX: u64 = 60;
const SEED: u64 = 42;
const N_TEAMS: usize = 5;
const TEAM_SIZE: usize = 8;

// ── SharedRecorder ─────────────────────────────────────────────────────────
// A thin wrapper around Arc<Mutex<InMemoryRecorder>> that implements Recorder
// so we can hand it to SimulationBuilder and also inspect it after the run.

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

// ── helpers ────────────────────────────────────────────────────────────────

fn build_world(seed: u64) -> HrWorld {
    let mut rng = socsim_core::SimRng::from_seed(seed);
    let mut world = HrWorld::new(N_TEAMS, TEAM_SIZE, 4, 0.1, &mut rng);
    world.clock = socsim_core::SimClock::new(T_MAX);
    world
}

fn mechanism_names() -> &'static [&'static str] {
    &[
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
    ]
}

/// Run a full baseline simulation for `seed` and return the recorded metric
/// series for `key`, ordered by time step.
fn run_metric_series(seed: u64, key: &str) -> Vec<f64> {
    let world = build_world(seed);

    let mut reg = socsim_config::Registry::new();
    HrLifecyclePack.register(&mut reg);

    let p = Params::empty();
    let (shared_rec, rec_handle) = SharedRecorder::new();

    let mut builder = SimulationBuilder::new(world)
        .scheduler(Box::new(RandomActivationScheduler))
        .seed(seed)
        .recorder(Box::new(shared_rec));

    for name in mechanism_names() {
        let m = reg.build(name, &p).expect("mechanism registered");
        builder = builder.add_mechanism(m);
    }

    let mut sim = builder.build();
    sim.run().expect("simulation should complete");

    let rec = rec_handle.lock().unwrap();
    let mut rows: Vec<(u64, f64)> = rec
        .metrics()
        .iter()
        .filter(|r| r.key == key)
        .map(|r| (r.t, r.value))
        .collect();
    rows.sort_by_key(|(t, _)| *t);
    rows.into_iter().map(|(_, v)| v).collect()
}

/// Mean of a slice (0.0 if empty).
fn mean(xs: &[f64]) -> f64 {
    if xs.is_empty() {
        0.0
    } else {
        xs.iter().sum::<f64>() / xs.len() as f64
    }
}

/// Number of months to skip before assessing steady-state dynamics.
const WARMUP: usize = 6;

// ── tests ──────────────────────────────────────────────────────────────────

#[test]
fn lifecycle_runs_without_error() {
    let world = build_world(SEED);

    let mut reg = socsim_config::Registry::new();
    HrLifecyclePack.register(&mut reg);

    let p = Params::empty();
    let mut builder = SimulationBuilder::new(world)
        .scheduler(Box::new(RandomActivationScheduler))
        .seed(SEED)
        .recorder(Box::new(InMemoryRecorder::new()));

    for name in mechanism_names() {
        let m = reg.build(name, &p).expect("mechanism registered");
        builder = builder.add_mechanism(m);
    }

    let mut sim = builder.build();
    sim.run().expect("simulation should complete without error");

    // Employee count > 0.
    assert!(
        sim.world().employee_count() > 0,
        "all employees quit — population collapsed"
    );

    // Employee count bounded (≤ initial + generous hiring slack).
    let max_expected = N_TEAMS * TEAM_SIZE * 3;
    assert!(
        sim.world().employee_count() <= max_expected,
        "employee count exploded: {}",
        sim.world().employee_count()
    );

    // All team knowledge stocks finite and non-negative.
    for (i, team) in sim.world().teams.iter().enumerate() {
        assert!(
            team.knowledge_stock.is_finite(),
            "team {i} knowledge_stock is not finite"
        );
        assert!(
            team.knowledge_stock >= 0.0,
            "team {i} knowledge_stock is negative: {}",
            team.knowledge_stock
        );
    }
}

#[test]
fn metrics_recorded_every_step() {
    let world = build_world(SEED);

    let mut reg = socsim_config::Registry::new();
    HrLifecyclePack.register(&mut reg);

    let p = Params::empty();
    let (shared_rec, rec_handle) = SharedRecorder::new();

    let mut builder = SimulationBuilder::new(world)
        .scheduler(Box::new(RandomActivationScheduler))
        .seed(SEED)
        .recorder(Box::new(shared_rec));

    for name in mechanism_names() {
        let m = reg.build(name, &p).expect("mechanism registered");
        builder = builder.add_mechanism(m);
    }

    let mut sim = builder.build();
    sim.run().expect("simulation should complete");

    let rec = rec_handle.lock().unwrap();

    // There should be exactly T_MAX metric rows for org_performance.
    let perf_rows: Vec<&MetricRow> = rec
        .metrics()
        .iter()
        .filter(|r| r.key == "org_performance")
        .collect();

    assert_eq!(
        perf_rows.len() as u64,
        T_MAX,
        "expected one org_performance metric per step, got {}",
        perf_rows.len()
    );

    // All values should be finite.
    for row in rec.metrics() {
        assert!(
            row.value.is_finite(),
            "metric '{}' at t={} is not finite: {}",
            row.key,
            row.t,
            row.value
        );
    }
}

#[test]
fn determinism_same_seed_same_performance_series() {
    fn run_and_collect(seed: u64) -> Vec<f64> {
        let world = build_world(seed);

        let mut reg = socsim_config::Registry::new();
        HrLifecyclePack.register(&mut reg);

        let p = Params::empty();
        let (shared_rec, rec_handle) = SharedRecorder::new();

        let mut builder = SimulationBuilder::new(world)
            .scheduler(Box::new(RandomActivationScheduler))
            .seed(seed)
            .recorder(Box::new(shared_rec));

        for name in mechanism_names() {
            let m = reg.build(name, &p).unwrap();
            builder = builder.add_mechanism(m);
        }

        let mut sim = builder.build();
        sim.run().unwrap();

        let rec = rec_handle.lock().unwrap();
        rec.metrics()
            .iter()
            .filter(|r| r.key == "org_performance")
            .map(|r| r.value)
            .collect()
    }

    let series_a = run_and_collect(42);
    let series_b = run_and_collect(42);

    assert_eq!(series_a.len(), series_b.len(), "length mismatch");
    for (i, (a, b)) in series_a.iter().zip(series_b.iter()).enumerate() {
        assert_eq!(
            a.to_bits(),
            b.to_bits(),
            "org_performance differs at step {i}: {a} vs {b}"
        );
    }
}

// ── sanity-check tests (design §11.4) ────────────────────────────────────────

#[test]
fn sanity_turnover_rate_is_low_after_warmup() {
    let turnover = run_metric_series(SEED, "turnover_rate");
    assert_eq!(turnover.len() as u64, T_MAX);

    // Mean per-step monthly turnover hazard after warmup should be well under
    // 5%/month (target ~1–2%/month).
    let post_warmup = &turnover[WARMUP..];
    let avg = mean(post_warmup);
    assert!(
        avg < 0.05,
        "post-warmup mean turnover_rate too high: {avg:.4} (target ~0.01–0.02, must be < 0.05)"
    );
    // It should also be non-trivially positive (people do leave).
    assert!(
        avg > 0.0,
        "no turnover at all after warmup — model is inert"
    );
}

#[test]
fn sanity_avg_tenure_trends_up() {
    let tenure = run_metric_series(SEED, "avg_tenure");
    assert_eq!(tenure.len() as u64, T_MAX);

    // Compare an early window to a late window: retention should accumulate
    // tenure, so the late-window mean must exceed the early-window mean.
    let early = mean(&tenure[WARMUP..WARMUP + 12]);
    let late = mean(&tenure[tenure.len() - 12..]);
    assert!(
        late > early,
        "avg_tenure did not trend up: early={early:.2} late={late:.2}"
    );
    // And it should reach a meaningful level (tens of months) by the end.
    assert!(
        late > 15.0,
        "avg_tenure stayed low: late-window mean = {late:.2} months (expected > 15)"
    );
}

#[test]
fn sanity_knowledge_stock_stable_and_finite() {
    let knowledge = run_metric_series(SEED, "knowledge_stock");
    assert_eq!(knowledge.len() as u64, T_MAX);

    // Initial stock = N_TEAMS * TEAM_SIZE (seeded in HrWorld::new).
    let initial = (N_TEAMS * TEAM_SIZE) as f64;
    let floor = 0.10 * initial; // must not collapse below 10% of initial
    let ceiling = 100.0 * initial; // must not explode

    for (i, &k) in knowledge.iter().enumerate() {
        assert!(k.is_finite(), "knowledge_stock at t={i} is not finite: {k}");
        assert!(
            k > floor,
            "knowledge_stock collapsed at t={i}: {k:.2} <= floor {floor:.2}"
        );
        assert!(
            k < ceiling,
            "knowledge_stock exploded at t={i}: {k:.2} >= ceiling {ceiling:.2}"
        );
    }
}

#[test]
fn sanity_headcount_stays_bounded() {
    // Headcount is not a recorded metric; reconstruct it from the run.
    let world = build_world(SEED);
    let mut reg = socsim_config::Registry::new();
    HrLifecyclePack.register(&mut reg);
    let p = Params::empty();

    let mut builder = SimulationBuilder::new(world)
        .scheduler(Box::new(RandomActivationScheduler))
        .seed(SEED)
        .recorder(Box::new(InMemoryRecorder::new()));
    for name in mechanism_names() {
        builder = builder.add_mechanism(reg.build(name, &p).unwrap());
    }
    let mut sim = builder.build();
    sim.run().unwrap();

    let n0 = (N_TEAMS * TEAM_SIZE) as f64;
    let final_n = sim.world().employee_count() as f64;
    assert!(
        final_n >= 0.5 * n0 && final_n <= 1.5 * n0,
        "final headcount {final_n} outside [0.5·N0, 1.5·N0] = [{}, {}]",
        0.5 * n0,
        1.5 * n0
    );
}

#[test]
fn sanity_org_performance_positive_after_warmup() {
    // With positive ability scale, the aggregate should be a positive,
    // interpretable quantity once the learning curve has ramped up.
    let perf = run_metric_series(SEED, "org_performance");
    let post_warmup = &perf[WARMUP..];
    let avg = mean(post_warmup);
    assert!(
        avg > 0.0,
        "post-warmup mean org_performance not positive: {avg:.4}"
    );
}
