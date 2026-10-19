//! Trial management, multi-seed runs, and parameter sweeps for `socsim`.
//!
//! This crate is generic over any concrete world `W: WorldState` and provides:
//!
//! - [`WorldFactory`] — a type alias for a closure that builds a world from
//!   `[world]` params and a seed.
//! - [`run_once`] — run a single scenario trial for one seed, returning a
//!   [`RunResult`] containing time series, final metrics, and event count.
//! - [`run_seeds`] — run the same scenario over many seeds, optionally in
//!   parallel via `rayon`.
//! - [`summarize`] — aggregate a slice of [`RunResult`]s into per-metric
//!   mean/std/min/max statistics.
//! - [`run_sweep`] — grid-sweep over a parameter space, reusing [`run_seeds`]
//!   for each combination.
//!
//! Trials built from the whole scenario:
//!
//! - [`Trial`] — the parts of one trial (world, mechanisms, scheduler and
//!   engine seed), built with [`Trial::new`].
//! - [`TrialFactory`] — builds a [`Trial`] from the whole effective
//!   [`Scenario`] and a seed, so the initial world may depend on
//!   `[[mechanism]]` params and the factory chooses the engine seed.
//! - [`RegistryTrialFactory`] — wraps an existing ([`WorldFactory`], register)
//!   pair as a [`TrialFactory`] that reproduces [`run_once`].
//! - [`scheduler_from_scenario`] — the scheduler named by the scenario.
//! - [`run_trial`] / [`run_trials`] — run one trial / many seeds (optionally
//!   in parallel) through a factory.
//! - [`run_trial_sweep`] — a checked grid sweep that can patch `[world]`
//!   paths, rejects unknown keys and type mismatches, validates every grid
//!   point before running, and returns the effective scenario of each point
//!   as a [`CheckedSweepPoint`].
//!
//! Every entry point runs a trial through the same execution kernel (clock
//! reset to `t_max`, engine, in-memory recorder, result extraction).

use std::collections::HashMap;

use rayon::prelude::*;
use serde::{ser::SerializeMap, Deserialize, Serialize, Serializer};

use socsim_config::{Registry, Scenario};
use socsim_core::{Result, WorldState};
use socsim_log::EventRow;

mod checked_sweep;
mod trial;

pub use checked_sweep::{run_trial_sweep, CheckedSweepPoint};
pub use trial::{
    run_trial, run_trials, scheduler_from_scenario, RegistryTrialFactory, Trial, TrialFactory,
};

// ── WorldFactory ──────────────────────────────────────────────────────────────

/// A closure that constructs a concrete world `W` from the `[world]` parameter
/// bag and a per-trial seed.
///
/// The factory is responsible for seeding any internal RNG with `seed`.
pub type WorldFactory<W> = Box<dyn Fn(&socsim_config::Params, u64) -> Result<W> + Send + Sync>;

// ── RunResult ─────────────────────────────────────────────────────────────────

/// The outcome of a single simulation trial.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunResult {
    /// Seed used for this trial: the trial seed passed by the caller (for
    /// factory-built trials this is not [`Trial::engine_seed`]).
    pub seed: u64,

    /// Time series for each metric key: `key → Vec<(t, value)>`.
    #[serde(serialize_with = "serialize_string_map_sorted")]
    pub series: HashMap<String, Vec<(u64, f64)>>,

    /// Final (last-recorded) value for each metric key.
    #[serde(serialize_with = "serialize_string_map_sorted")]
    pub final_metrics: HashMap<String, f64>,

    /// All events recorded during the run, in the order the mechanisms emitted
    /// them.  Consumers that previously only inspected `event_count` keep
    /// working unchanged; callers that need the payloads (e.g. CLI JSONL log
    /// writers, downstream analysis) can iterate this directly.
    #[serde(default)]
    pub events: Vec<EventRow>,

    /// Total event count recorded during the run (equals `events.len()`,
    /// retained for API compatibility).
    pub event_count: usize,
}

fn serialize_string_map_sorted<S, V>(
    map: &HashMap<String, V>,
    serializer: S,
) -> std::result::Result<S::Ok, S::Error>
where
    S: Serializer,
    V: Serialize,
{
    let mut entries: Vec<_> = map.iter().collect();
    entries.sort_by_key(|(key, _)| *key);

    let mut ordered = serializer.serialize_map(Some(entries.len()))?;
    for (key, value) in entries {
        ordered.serialize_entry(key, value)?;
    }
    ordered.end()
}

// ── run_once ──────────────────────────────────────────────────────────────────

/// Run a single scenario trial for `seed`.
///
/// # Arguments
///
/// * `scenario` — parsed and validated scenario.
/// * `world_factory` — closure that constructs `W` from `[world]` params and
///   the trial seed.
/// * `register` — closure that populates an empty `Registry<W>` with the
///   pack's mechanisms.
/// * `seed` — RNG seed for this trial.
///
/// # Returns
///
/// A [`RunResult`] populated from the
/// [`InMemoryRecorder`](socsim_log::InMemoryRecorder) collected during the
/// run.
///
/// The scenario is not validated here.  The raw `seed` is passed both to
/// `world_factory` and to the engine; [`RegistryTrialFactory`] with
/// [`run_trial`] reproduces this function's results for valid scenarios.
pub fn run_once<W>(
    scenario: &Scenario,
    world_factory: &WorldFactory<W>,
    register: &dyn Fn(&mut Registry<W>),
    seed: u64,
) -> Result<RunResult>
where
    W: WorldState,
{
    let trial = trial::build_registry_trial(scenario, world_factory, register, seed)?;
    trial::execute_trial(scenario, trial, seed)
}

// ── run_seeds ─────────────────────────────────────────────────────────────────

/// Run the scenario over a collection of seeds.
///
/// When `parallel` is `true`, trials are executed concurrently using `rayon`;
/// each trial is independently seeded and deterministic.  Results are sorted
/// by seed before returning.
///
/// # Errors
///
/// Returns the first error encountered (in parallel mode, the first to fail
/// in processing order).
pub fn run_seeds<W>(
    scenario: &Scenario,
    world_factory: &WorldFactory<W>,
    register: &(dyn Fn(&mut Registry<W>) + Sync),
    seeds: impl IntoIterator<Item = u64>,
    parallel: bool,
) -> Result<Vec<RunResult>>
where
    W: WorldState + Send,
{
    let seeds: Vec<u64> = seeds.into_iter().collect();

    let mut results: Vec<RunResult> = if parallel {
        seeds
            .par_iter()
            .map(|&s| run_once(scenario, world_factory, register, s))
            .collect::<Result<Vec<_>>>()?
    } else {
        seeds
            .iter()
            .map(|&s| run_once(scenario, world_factory, register, s))
            .collect::<Result<Vec<_>>>()?
    };

    results.sort_by_key(|r| r.seed);
    Ok(results)
}

// ── MetricStats / Summary ─────────────────────────────────────────────────────

/// Aggregate statistics for one metric across a set of trials.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricStats {
    /// Metric key.
    pub key: String,
    /// Mean of the final value across all trials.
    pub mean: f64,
    /// Standard deviation.
    pub std: f64,
    /// Minimum value.
    pub min: f64,
    /// Maximum value.
    pub max: f64,
    /// Number of trials included.
    pub n: usize,
}

/// Cross-seed summary: per-metric statistics of the final recorded value.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Summary {
    /// Per-metric aggregated statistics, sorted alphabetically by key.
    pub metrics: Vec<MetricStats>,
}

impl Summary {
    /// Produce a CSV string with columns: `key,mean,std,min,max,n`.
    pub fn to_csv(&self) -> String {
        let mut out = String::from("key,mean,std,min,max,n\n");
        for m in &self.metrics {
            out.push_str(&format!(
                "{},{:.6},{:.6},{:.6},{:.6},{}\n",
                m.key, m.mean, m.std, m.min, m.max, m.n
            ));
        }
        out
    }

    /// Serialize to a pretty-printed JSON string.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }
}

/// Aggregate a slice of [`RunResult`]s into per-metric statistics.
///
/// Only metrics that appear in at least one result are included.
/// The returned [`Summary::metrics`] vector is sorted alphabetically by key.
pub fn summarize(results: &[RunResult]) -> Summary {
    let mut by_key: HashMap<String, Vec<f64>> = HashMap::new();
    for r in results {
        for (k, v) in &r.final_metrics {
            by_key.entry(k.clone()).or_default().push(*v);
        }
    }

    let mut stats: Vec<MetricStats> = by_key
        .into_iter()
        .map(|(key, vals)| {
            let n = vals.len();
            let mean = vals.iter().sum::<f64>() / n as f64;
            let variance = vals.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n as f64;
            let std = variance.sqrt();
            let min = vals.iter().cloned().fold(f64::INFINITY, f64::min);
            let max = vals.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            MetricStats {
                key,
                mean,
                std,
                min,
                max,
                n,
            }
        })
        .collect();

    stats.sort_by(|a, b| a.key.cmp(&b.key));
    Summary { metrics: stats }
}

// ── SweepAxis / SweepPoint / run_sweep ───────────────────────────────────────

/// One axis of a parameter sweep.
///
/// `param_key` is `"<mechanism_name>.<param_name>"`, e.g.
/// `"peer_effect.alpha_peer"`.  At sweep time the matching mechanism entry in
/// the scenario is cloned and the param overridden.
#[derive(Debug, Clone)]
pub struct SweepAxis {
    /// `"<mechanism_name>.<param_name>"`.
    pub param_key: String,
    /// Values to iterate over.
    pub values: Vec<f64>,
}

/// The result of one grid point in a parameter sweep.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SweepPoint {
    /// Parameter values used for this grid point (in sweep axis order).
    pub params: Vec<(String, f64)>,
    /// Aggregated summary across all seeds for this grid combination.
    pub summary: Summary,
}

/// Run a grid sweep over the Cartesian product of the given [`SweepAxis`]es.
///
/// For each combination the matching mechanism entries in the scenario are
/// patched, then [`run_seeds`] is called with the provided seeds.
///
/// # Arguments
///
/// * `scenario` — base scenario (not mutated).
/// * `axes` — sweep axes.
/// * `world_factory` / `register` — forwarded to [`run_seeds`].
/// * `seeds` — seed list for each combo.
/// * `parallel` — whether to run seeds in parallel within each combo.
pub fn run_sweep<W>(
    scenario: &Scenario,
    axes: &[SweepAxis],
    world_factory: &WorldFactory<W>,
    register: &(dyn Fn(&mut Registry<W>) + Sync),
    seeds: Vec<u64>,
    parallel: bool,
) -> Result<Vec<SweepPoint>>
where
    W: WorldState + Send,
{
    let combos = cartesian_product(axes);

    let mut points = Vec::new();
    for combo in combos {
        let mut patched = scenario.clone();
        for (key, value) in &combo {
            if let Some(dot) = key.find('.') {
                let mech_name = &key[..dot];
                let param_name = &key[dot + 1..];
                for entry in &mut patched.mechanisms {
                    if entry.name == mech_name {
                        entry.params.set_f64(param_name, *value);
                    }
                }
            }
        }

        let results = run_seeds(&patched, world_factory, register, seeds.clone(), parallel)?;
        let summary = summarize(&results);
        points.push(SweepPoint {
            params: combo,
            summary,
        });
    }

    Ok(points)
}

/// Build the Cartesian product of sweep axis values.
fn cartesian_product(axes: &[SweepAxis]) -> Vec<Vec<(String, f64)>> {
    if axes.is_empty() {
        return vec![vec![]];
    }
    let mut result: Vec<Vec<(String, f64)>> = vec![vec![]];
    for axis in axes {
        let mut next = Vec::new();
        for row in &result {
            for &v in &axis.values {
                let mut new_row = row.clone();
                new_row.push((axis.param_key.clone(), v));
                next.push(new_row);
            }
        }
        result = next;
    }
    result
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cartesian_product_empty() {
        let combos = cartesian_product(&[]);
        assert_eq!(combos.len(), 1);
        assert!(combos[0].is_empty());
    }

    #[test]
    fn cartesian_product_single_axis() {
        let axes = vec![SweepAxis {
            param_key: "x.a".to_owned(),
            values: vec![1.0, 2.0, 3.0],
        }];
        let combos = cartesian_product(&axes);
        assert_eq!(combos.len(), 3);
    }

    #[test]
    fn cartesian_product_two_axes() {
        let axes = vec![
            SweepAxis {
                param_key: "x.a".to_owned(),
                values: vec![1.0, 2.0],
            },
            SweepAxis {
                param_key: "y.b".to_owned(),
                values: vec![10.0, 20.0],
            },
        ];
        let combos = cartesian_product(&axes);
        assert_eq!(combos.len(), 4);
    }

    #[test]
    fn summarize_single_result() {
        let mut final_metrics = HashMap::new();
        final_metrics.insert("score".to_owned(), 3.0);
        let results = vec![RunResult {
            seed: 0,
            series: HashMap::new(),
            final_metrics,
            events: Vec::new(),
            event_count: 0,
        }];
        let s = summarize(&results);
        assert_eq!(s.metrics.len(), 1);
        assert_eq!(s.metrics[0].key, "score");
        assert!((s.metrics[0].mean - 3.0).abs() < 1e-9);
    }

    #[test]
    fn summarize_multiple_results() {
        let make_result = |seed: u64, val: f64| {
            let mut fm = HashMap::new();
            fm.insert("x".to_owned(), val);
            RunResult {
                seed,
                series: HashMap::new(),
                final_metrics: fm,
                events: Vec::new(),
                event_count: 0,
            }
        };
        let results = vec![make_result(0, 1.0), make_result(1, 3.0)];
        let s = summarize(&results);
        assert_eq!(s.metrics[0].mean, 2.0);
        assert!((s.metrics[0].min - 1.0).abs() < 1e-9);
        assert!((s.metrics[0].max - 3.0).abs() < 1e-9);
    }

    #[test]
    fn summary_to_csv_headers() {
        let s = Summary {
            metrics: vec![MetricStats {
                key: "foo".to_owned(),
                mean: 1.0,
                std: 0.0,
                min: 1.0,
                max: 1.0,
                n: 1,
            }],
        };
        let csv = s.to_csv();
        assert!(csv.starts_with("key,mean,std,min,max,n\n"));
        assert!(csv.contains("foo,"));
    }

    #[test]
    fn run_result_json_maps_are_sorted_and_byte_identical() {
        fn equivalent_result() -> RunResult {
            let mut series = HashMap::new();
            let mut final_metrics = HashMap::new();
            for (index, key) in [
                "zeta", "alpha", "theta", "beta", "eta", "gamma", "iota", "delta", "kappa",
                "epsilon", "lambda", "mu",
            ]
            .into_iter()
            .enumerate()
            {
                let value = index as f64;
                series.insert(key.to_owned(), vec![(0, value)]);
                final_metrics.insert(key.to_owned(), value);
            }
            RunResult {
                seed: 7,
                series,
                final_metrics,
                events: Vec::new(),
                event_count: 0,
            }
        }

        let first = serde_json::to_string(&equivalent_result()).unwrap();
        let second = serde_json::to_string(&equivalent_result()).unwrap();

        assert_eq!(first, second);
        assert!(first.contains(r#""series":{"alpha":[[0,1.0]],"beta":[[0,3.0]],"delta":[[0,7.0]]"#));
        assert!(first.contains(r#""final_metrics":{"alpha":1.0,"beta":3.0,"delta":7.0"#));
    }
}
