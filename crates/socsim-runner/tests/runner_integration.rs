//! Integration tests for `socsim-runner` with the `hr-lifecycle` pack.
//!
//! Tests verify:
//! 1. Scenario parse/validate round-trip.
//! 2. `run_once` produces sane HR baseline metrics (turnover, knowledge, perf).
//! 3. `run_once` is deterministic for the same seed.
//! 4. `run_seeds` parallel mode produces identical per-seed results to
//!    sequential mode.

use std::collections::HashMap;

use socsim_config::{ModulePack, Registry, Scenario};
use socsim_core::SimRng;
use socsim_packs::hr_lifecycle::{HrLifecyclePack, HrWorld};
use socsim_runner::{run_once, run_seeds, WorldFactory};

// ── Fixtures ──────────────────────────────────────────────────────────────────

const BASELINE_TOML: &str = include_str!("../../../scenarios/hr_lifecycle_baseline.toml");

fn make_factory() -> WorldFactory<HrWorld> {
    Box::new(|params, seed| {
        let n_teams = params.get_u64("n_teams", 5) as usize;
        let team_size = params.get_u64("team_size_initial", 8) as usize;
        let ws_k = params.get_u64("network_k", 4) as usize;
        let ws_beta = params.get_f64("network_beta", 0.1);
        let mut rng = SimRng::from_seed(seed);
        let world = HrWorld::new(n_teams, team_size, ws_k, ws_beta, &mut rng);
        Ok(world)
    })
}

fn register(reg: &mut Registry<HrWorld>) {
    HrLifecyclePack.register(reg);
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[test]
fn scenario_parse_and_validate_round_trip() {
    let scenario = Scenario::parse(BASELINE_TOML).expect("baseline TOML must parse");

    assert_eq!(scenario.simulation.module_pack, "hr-lifecycle");
    assert_eq!(scenario.simulation.t_max, 60);
    assert_eq!(scenario.simulation.scheduler, "random_activation");
    assert_eq!(scenario.mechanisms.len(), 10);

    // Validate against the pack's registry.
    let mut reg: Registry<HrWorld> = Registry::new();
    HrLifecyclePack.register(&mut reg);
    let names: Vec<&str> = reg.names().into_iter().collect();
    scenario
        .validate(&names)
        .expect("baseline scenario must validate");
}

#[test]
fn run_once_produces_sane_hr_baseline() {
    let scenario = Scenario::parse(BASELINE_TOML).unwrap();
    let factory = make_factory();
    let result = run_once(&scenario, &factory, &register, 42).expect("run should succeed");

    // Must record exactly t_max steps for each metric.
    let t_max = scenario.simulation.t_max as usize;
    let perf_series = result
        .series
        .get("org_performance")
        .expect("org_performance must be recorded");
    assert_eq!(
        perf_series.len(),
        t_max,
        "org_performance series must have t_max={t_max} entries"
    );

    // Org performance must be positive after warmup (at t=30+).
    let late_perf: Vec<f64> = perf_series
        .iter()
        .filter(|&&(t, _)| t >= 30)
        .map(|&(_, v)| v)
        .collect();
    assert!(!late_perf.is_empty());
    let avg_late_perf: f64 = late_perf.iter().sum::<f64>() / late_perf.len() as f64;
    assert!(
        avg_late_perf > 5.0,
        "avg late org_performance={avg_late_perf} should be > 5"
    );

    // Turnover rate must be in [0, 0.5] (sane range).
    let turnover_series = result
        .series
        .get("turnover_rate")
        .expect("turnover_rate must be recorded");
    for &(_, v) in turnover_series {
        assert!(
            (0.0..=0.5).contains(&v),
            "turnover_rate={v} out of [0, 0.5]"
        );
    }

    // Knowledge stock must remain positive.
    let ks_series = result
        .series
        .get("knowledge_stock")
        .expect("knowledge_stock must be recorded");
    for &(_, v) in ks_series {
        assert!(v >= 0.0, "knowledge_stock={v} must be non-negative");
    }

    // Average tenure must increase over time (general trend).
    let tenure_series = result
        .series
        .get("avg_tenure")
        .expect("avg_tenure must be recorded");
    let first_tenure = tenure_series.first().map(|&(_, v)| v).unwrap_or(0.0);
    let last_tenure = tenure_series.last().map(|&(_, v)| v).unwrap_or(0.0);
    assert!(
        last_tenure > first_tenure,
        "avg_tenure should increase: {first_tenure} → {last_tenure}"
    );
}

#[test]
fn run_once_is_deterministic_for_same_seed() {
    let scenario = Scenario::parse(BASELINE_TOML).unwrap();
    let factory = make_factory();

    let r1 = run_once(&scenario, &factory, &register, 99).unwrap();
    let r2 = run_once(&scenario, &factory, &register, 99).unwrap();

    // Final metrics must be identical.
    let keys = [
        "org_performance",
        "avg_tenure",
        "knowledge_stock",
        "turnover_rate",
    ];
    for key in keys {
        let v1 = r1.final_metrics[key];
        let v2 = r2.final_metrics[key];
        assert!(
            (v1 - v2).abs() < 1e-12,
            "metric {key}: seed-99 run 1={v1} ≠ run 2={v2}"
        );
    }
}

#[test]
fn run_once_different_seeds_differ() {
    let scenario = Scenario::parse(BASELINE_TOML).unwrap();
    let factory = make_factory();

    let r0 = run_once(&scenario, &factory, &register, 0).unwrap();
    let r1 = run_once(&scenario, &factory, &register, 1).unwrap();

    // At least one final metric should differ between seeds.
    let keys = ["org_performance", "avg_tenure", "knowledge_stock"];
    let any_differ = keys
        .iter()
        .any(|&k| (r0.final_metrics[k] - r1.final_metrics[k]).abs() > 1e-9);
    assert!(
        any_differ,
        "different seeds should produce different results"
    );
}

#[test]
fn run_seeds_parallel_equals_sequential() {
    let scenario = Scenario::parse(BASELINE_TOML).unwrap();
    let factory = make_factory();

    let seeds: Vec<u64> = (0..4).collect();

    let seq = run_seeds(&scenario, &factory, &register, seeds.clone(), false)
        .expect("sequential run should succeed");
    let par = run_seeds(&scenario, &factory, &register, seeds.clone(), true)
        .expect("parallel run should succeed");

    assert_eq!(seq.len(), par.len(), "same number of results");

    for (s, p) in seq.iter().zip(par.iter()) {
        assert_eq!(s.seed, p.seed, "seeds must match");
        let keys = [
            "org_performance",
            "avg_tenure",
            "knowledge_stock",
            "turnover_rate",
        ];
        for key in keys {
            let sv = s.final_metrics.get(key).copied().unwrap_or(0.0);
            let pv = p.final_metrics.get(key).copied().unwrap_or(0.0);
            assert!(
                (sv - pv).abs() < 1e-12,
                "seed={} metric={key}: sequential={sv} parallel={pv}",
                s.seed
            );
        }
    }
}

#[test]
fn run_once_populates_events_with_payloads() {
    // hr-lifecycle records `hiring` and `turnover` events via Recorder; this
    // test guards against regressions where events get counted but dropped
    // from `RunResult` (so the CLI JSONL log loses them).
    let scenario = Scenario::parse(BASELINE_TOML).unwrap();
    let factory = make_factory();
    let result = run_once(&scenario, &factory, &register, 7).unwrap();

    assert!(
        !result.events.is_empty(),
        "hr-lifecycle baseline must record at least one event"
    );
    assert_eq!(
        result.events.len(),
        result.event_count,
        "event_count must equal events.len() for compatibility"
    );

    // Payloads must be retained — not just kinds — so downstream JSONL
    // consumers can analyse them.
    let hiring = result.events.iter().find(|e| e.kind == "hiring");
    let turnover = result.events.iter().find(|e| e.kind == "turnover");
    assert!(hiring.is_some(), "should record at least one `hiring` event");
    assert!(
        turnover.is_some(),
        "should record at least one `turnover` event"
    );
    if let Some(ev) = hiring {
        assert!(
            ev.payload.get("agent_id").is_some(),
            "hiring payload should carry agent_id, got {}",
            ev.payload
        );
    }
    if let Some(ev) = turnover {
        assert!(
            ev.payload.get("agent_id").is_some(),
            "turnover payload should carry agent_id"
        );
    }

    // Events must round-trip through serde so `RunResult` can be JSON-logged.
    let json = serde_json::to_string(&result).expect("RunResult serialises");
    let back: socsim_runner::RunResult =
        serde_json::from_str(&json).expect("RunResult round-trips");
    assert_eq!(back.events.len(), result.events.len());
    assert_eq!(back.events[0].kind, result.events[0].kind);
}

#[test]
fn summarize_hr_results_sane() {
    let scenario = Scenario::parse(BASELINE_TOML).unwrap();
    let factory = make_factory();

    let results = run_seeds(&scenario, &factory, &register, 0..5, false).unwrap();
    let summary = socsim_runner::summarize(&results);

    // Should have 4 metrics.
    assert_eq!(summary.metrics.len(), 4);

    let by_key: HashMap<&str, &socsim_runner::MetricStats> = summary
        .metrics
        .iter()
        .map(|m| (m.key.as_str(), m))
        .collect();

    // org_performance: mean > 5, std >= 0
    let perf = by_key["org_performance"];
    assert!(perf.mean > 5.0, "mean org_performance={}", perf.mean);
    assert!(perf.std >= 0.0);
    assert!(perf.min <= perf.mean);
    assert!(perf.max >= perf.mean);

    // turnover_rate: mean in [0, 0.5]
    let tr = by_key["turnover_rate"];
    assert!((0.0..=0.5).contains(&tr.mean), "mean turnover={}", tr.mean);

    // knowledge_stock: positive
    let ks = by_key["knowledge_stock"];
    assert!(ks.mean > 0.0, "mean knowledge_stock={}", ks.mean);

    // CSV has correct header
    let csv = summary.to_csv();
    assert!(csv.starts_with("key,mean,std,min,max,n\n"));
}
