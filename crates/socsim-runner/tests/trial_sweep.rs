//! Tests for the checked sweep `run_trial_sweep`, and for the unchanged
//! behaviour of the legacy `run_sweep`.

mod common;

use socsim_config::Scenario;
use socsim_core::SocsimError;
use socsim_runner::{
    run_seeds, run_sweep, run_trial_sweep, run_trials, summarize, CheckedSweepPoint, Summary,
    SweepAxis,
};

use common::{
    config_message, register_test, world_factory, EngineSeed, LocalWorld, ScenarioFactory,
    TestWorld,
};

// ── fixtures ──────────────────────────────────────────────────────────────────

const SWEEP_TOML: &str = r#"
[simulation]
name        = "sweep"
module_pack = "test"
t_max       = 3
seed        = 0
scheduler   = "sequential"

[world]
alpha = 1.5
n     = 3
flag  = true
label = "x"

[world.game]
temptation = 1.6

[[mechanism]]
name = "record_level"
[mechanism.params]
init = 2.5
k    = 2

[[mechanism]]
name = "record_level"
[mechanism.params]
init = 2.5
k    = 2

[[mechanism]]
name = "draw"

[output]
log_path = "unused.jsonl"
"#;

fn scenario() -> Scenario {
    Scenario::parse(SWEEP_TOML).unwrap()
}

fn axis(key: &str, values: &[f64]) -> SweepAxis {
    SweepAxis {
        param_key: key.to_owned(),
        values: values.to_vec(),
    }
}

fn factory() -> ScenarioFactory<LocalWorld> {
    ScenarioFactory::new(EngineSeed::Derived)
}

fn sweep(
    axes: &[SweepAxis],
    f: &ScenarioFactory<LocalWorld>,
) -> socsim_core::Result<Vec<CheckedSweepPoint>> {
    run_trial_sweep(&scenario(), axes, f, &[0, 1], true)
}

fn as_value(s: &Scenario) -> toml::Value {
    toml::Value::try_from(s).unwrap()
}

fn summary_json(s: &Summary) -> serde_json::Value {
    serde_json::to_value(s).unwrap()
}

fn mean(s: &Summary, key: &str) -> f64 {
    s.metrics.iter().find(|m| m.key == key).unwrap().mean
}

// ── successful sweeps ─────────────────────────────────────────────────────────

#[test]
fn checked_sweep_patches_world_and_mechanisms() {
    let base = scenario();
    let f = factory();
    let axes = [
        axis("world.game.temptation", &[1.6, 1.9]),
        axis("world.n", &[2.0, 4.0]),
        axis("record_level.init", &[0.5]),
        axis("record_level.unseen", &[1.0]),
    ];
    let points = run_trial_sweep(&base, &axes, &f, &[0, 1], true).unwrap();

    assert_eq!(points.len(), 4);
    let expected_grid = [(1.6, 2), (1.6, 4), (1.9, 2), (1.9, 4)];
    for (point, &(temptation, n)) in points.iter().zip(&expected_grid) {
        // `params` follows the axis order; the last axis varies fastest.
        let keys: Vec<&str> = point.params.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(
            keys,
            vec![
                "world.game.temptation",
                "world.n",
                "record_level.init",
                "record_level.unseen"
            ]
        );
        assert_eq!(point.params[0].1, temptation);
        assert_eq!(point.params[1].1, n as f64);

        // The patch is checked on the effective scenario itself.
        let world = &point.effective.world.0;
        assert_eq!(world["game"]["temptation"], toml::Value::Float(temptation));
        assert_eq!(world["n"], toml::Value::Integer(n));
        assert_eq!(world["alpha"], toml::Value::Float(1.5));
        assert_eq!(world["flag"], toml::Value::Boolean(true));
        assert_eq!(world["label"], toml::Value::String("x".to_owned()));

        let record_levels: Vec<_> = point
            .effective
            .mechanisms
            .iter()
            .filter(|m| m.name == "record_level")
            .collect();
        assert_eq!(record_levels.len(), 2);
        for entry in record_levels {
            assert_eq!(entry.params.0["init"], toml::Value::Float(0.5));
            // A param absent from the scenario is added as a float.
            assert_eq!(entry.params.0["unseen"], toml::Value::Float(1.0));
            assert_eq!(entry.params.0["k"], toml::Value::Integer(2));
        }
        let draw = point
            .effective
            .mechanisms
            .iter()
            .find(|m| m.name == "draw")
            .unwrap();
        assert!(draw.params.0.is_empty());

        // The factory read the patched `init` into the world.
        assert_eq!(mean(&point.summary, "level"), 0.5);
        assert_eq!(point.summary.metrics[0].n, 2);
    }

    // The base scenario is not mutated.
    assert_eq!(as_value(&base), as_value(&scenario()));
    // One validation per grid point, one build per (point, seed).
    assert_eq!(f.validations(), 4);
    assert_eq!(f.builds(), 8);
}

#[test]
fn checked_sweep_summary_is_summarize_of_run_trials() {
    let f = factory();
    let axes = [axis("record_level.init", &[0.5, 3.0])];
    let points = sweep(&axes, &f).unwrap();
    for point in &points {
        let results = run_trials(&point.effective, &f, &[0, 1], false).unwrap();
        assert_eq!(
            summary_json(&point.summary),
            summary_json(&summarize(&results))
        );
    }
    assert_eq!(mean(&points[1].summary, "level"), 3.0);
}

#[test]
fn checked_sweep_writes_integral_values_into_integer_params() {
    let f = factory();
    let points = sweep(&[axis("record_level.k", &[3.0, -4.0])], &f).unwrap();
    let ks: Vec<toml::Value> = points
        .iter()
        .flat_map(|p| {
            p.effective
                .mechanisms
                .iter()
                .filter(|m| m.name == "record_level")
                .map(|m| m.params.0["k"].clone())
                .collect::<Vec<_>>()
        })
        .collect();
    assert_eq!(
        ks,
        vec![
            toml::Value::Integer(3),
            toml::Value::Integer(3),
            toml::Value::Integer(-4),
            toml::Value::Integer(-4)
        ]
    );
}

#[test]
fn checked_sweep_without_axes_runs_the_base_scenario_once() {
    let f = factory();
    let points = sweep(&[], &f).unwrap();
    assert_eq!(points.len(), 1);
    assert!(points[0].params.is_empty());
    assert_eq!(as_value(&points[0].effective), as_value(&scenario()));
    assert_eq!(mean(&points[0].summary, "level"), 2.5);
}

#[test]
fn checked_sweep_point_serialises_with_effective_scenario() {
    let f = factory();
    let points = sweep(&[axis("world.n", &[5.0])], &f).unwrap();
    let json = serde_json::to_value(&points[0]).unwrap();
    assert_eq!(json["effective"]["world"]["n"], serde_json::json!(5));
    let back: CheckedSweepPoint = serde_json::from_value(json).unwrap();
    assert_eq!(as_value(&back.effective), as_value(&points[0].effective));
    // The effective scenario writes back to TOML and parses again.
    let text = toml::to_string(&back.effective).unwrap();
    let reparsed = Scenario::parse(&text).unwrap();
    assert_eq!(reparsed.world.0["n"], toml::Value::Integer(5));
}

#[test]
fn checked_sweep_world_mechanism_name_is_not_a_mechanism_key() {
    // A mechanism may be called `world`; a `world.` key still addresses the
    // `[world]` table, never that mechanism.
    let text = SWEEP_TOML.replace(
        "[[mechanism]]\nname = \"draw\"",
        "[[mechanism]]\nname = \"world\"\n[mechanism.params]\nalpha = 9.0",
    );
    let base = Scenario::parse(&text).unwrap();
    let f = ScenarioFactory::<LocalWorld>::new(EngineSeed::Derived);
    let points = run_trial_sweep(&base, &[axis("world.alpha", &[0.25])], &f, &[], false).unwrap();
    let effective = &points[0].effective;
    assert_eq!(effective.world.0["alpha"], toml::Value::Float(0.25));
    let world_mech = effective
        .mechanisms
        .iter()
        .find(|m| m.name == "world")
        .unwrap();
    assert_eq!(world_mech.params.0["alpha"], toml::Value::Float(9.0));
}

// ── rejected axes ─────────────────────────────────────────────────────────────

/// Every axis here is rejected with a `Config` error naming its key, and no
/// trial is built.
#[test]
fn checked_sweep_rejects_bad_axes() {
    let cases: Vec<(&str, Vec<f64>, &[&str])> = vec![
        (
            "world.game.temptaton",
            vec![1.0],
            &["temptaton", "[world.game]"],
        ),
        ("world.missing.x", vec![1.0], &["missing", "[world]"]),
        ("world.n", vec![2.5], &["integer", "2.5"]),
        ("world.flag", vec![1.0], &["boolean"]),
        ("world.label", vec![1.0], &["string"]),
        ("world.game", vec![1.0], &["table"]),
        ("world.alpha.x", vec![1.0], &["alpha", "not a table"]),
        ("nosuch.x", vec![1.0], &["nosuch", "not in the scenario"]),
        ("alpha", vec![1.0], &[]),
        ("world.", vec![1.0], &[]),
        ("world..x", vec![1.0], &[]),
        (".x", vec![1.0], &[]),
        ("record_level.", vec![1.0], &[]),
        ("record_level.a.b", vec![1.0], &["a.b"]),
        ("world.alpha", vec![f64::NAN], &["NaN", "not finite"]),
        ("world.alpha", vec![f64::INFINITY], &["not finite"]),
        ("record_level.unseen", vec![f64::NAN], &["not finite"]),
        ("record_level.k", vec![1.5], &["integer", "1.5"]),
        ("world.n", vec![1e19], &["integer"]),
    ];

    for (key, values, fragments) in cases {
        let f = factory();
        let msg = config_message(sweep(&[axis(key, &values)], &f));
        assert!(
            msg.contains(&format!("sweep axis '{key}'")),
            "{key}: message does not name the axis: {msg}"
        );
        for fragment in fragments {
            assert!(
                msg.contains(fragment),
                "{key}: missing '{fragment}' in: {msg}"
            );
        }
        assert_eq!(f.builds(), 0, "{key}: no trial may run");
        assert_eq!(f.validations(), 0, "{key}: rejected before validation");
    }
}

#[test]
fn checked_sweep_rejects_a_bad_value_in_a_later_combination() {
    // The first combination is fine; the second is not.  Nothing runs.
    let f = factory();
    let msg = config_message(sweep(&[axis("world.n", &[2.0, 2.5])], &f));
    assert!(msg.contains("sweep axis 'world.n'"), "{msg}");
    assert_eq!(f.builds(), 0);
}

#[test]
fn checked_sweep_rejects_duplicate_axes() {
    let f = factory();
    let axes = [axis("world.n", &[2.0]), axis("world.n", &[4.0])];
    let msg = config_message(sweep(&axes, &f));
    assert!(msg.contains("sweep axis 'world.n'"), "{msg}");
    assert!(msg.contains("duplicate"), "{msg}");
    assert_eq!(f.builds(), 0);
}

#[test]
fn checked_sweep_rejects_empty_axis() {
    let f = factory();
    let axes = [axis("world.n", &[2.0]), axis("record_level.init", &[])];
    let msg = config_message(sweep(&axes, &f));
    assert!(msg.contains("sweep axis 'record_level.init'"), "{msg}");
    assert!(msg.contains("no values"), "{msg}");
    assert_eq!(f.builds(), 0);
}

// ── validation of every combination ───────────────────────────────────────────

#[test]
fn checked_sweep_validates_every_combination_before_running() {
    let f = factory().with_validator(|s: &Scenario| {
        let t = s.world.0["game"]["temptation"].as_float().unwrap();
        if t > 1.8 {
            Err(SocsimError::Config(format!("temptation {t} is too large")))
        } else {
            Ok(())
        }
    });
    let msg = config_message(sweep(&[axis("world.game.temptation", &[1.6, 1.9])], &f));
    assert!(msg.contains("sweep combination"), "{msg}");
    assert!(msg.contains("world.game.temptation=1.9"), "{msg}");
    assert!(msg.contains("temptation 1.9 is too large"), "{msg}");
    // Both combinations were validated; no trial ran, not even for 1.6.
    assert_eq!(f.validations(), 2);
    assert_eq!(f.builds(), 0);
}

#[test]
fn checked_sweep_reports_trial_failures_with_the_seed() {
    let f = factory().failing_on(&[1]);
    let msg = config_message(sweep(&[axis("world.n", &[2.0])], &f));
    assert!(msg.contains("trial seed 1"), "{msg}");
}

// ── the legacy run_sweep is unchanged ─────────────────────────────────────────

const LEGACY_TOML: &str = r#"
[simulation]
name        = "legacy"
module_pack = "test"
t_max       = 3
seed        = 0
scheduler   = "random_activation"

[world]
level = 1.0

[[mechanism]]
name = "record_level"
[mechanism.params]
offset = 2

[[mechanism]]
name = "draw"

[output]
log_path = "unused.jsonl"
"#;

#[test]
fn legacy_run_sweep_still_ignores_unknown_keys() {
    let base = Scenario::parse(LEGACY_TOML).unwrap();
    let wf = world_factory::<TestWorld>();
    let seeds = vec![0u64, 1, 2];

    let reference =
        summarize(&run_seeds(&base, &wf, &register_test, seeds.clone(), false).unwrap());

    // A key without '.', an unknown mechanism, and a `world.` key (which the
    // legacy sweep treats as the mechanism name "world") are all ignored.
    let axes = [
        axis("alpha", &[1.0, 2.0]),
        axis("nosuch.x", &[1.0]),
        axis("world.level", &[5.0]),
    ];
    let points = run_sweep(&base, &axes, &wf, &register_test, seeds, false).unwrap();
    assert_eq!(points.len(), 2);
    for point in &points {
        assert_eq!(point.params.len(), 3);
        assert_eq!(summary_json(&point.summary), summary_json(&reference));
    }
    // offset = 2 is an integer, which `get_f64` does not read: level 1.0 only.
    assert_eq!(mean(&reference, "level"), 1.0);
}

#[test]
fn legacy_run_sweep_still_writes_floats() {
    let base = Scenario::parse(LEGACY_TOML).unwrap();
    let wf = world_factory::<TestWorld>();

    // The legacy sweep overwrites the integer `offset = 2` with Float(3.0)
    // via `set_f64`, which `get_f64` then reads: level 1.0 + 3.0.
    let points = run_sweep(
        &base,
        &[axis("record_level.offset", &[3.0])],
        &wf,
        &register_test,
        vec![0],
        false,
    )
    .unwrap();
    assert_eq!(mean(&points[0].summary, "level"), 4.0);

    // The checked sweep keeps the integer type instead.
    let f = factory();
    let checked = run_trial_sweep(
        &base,
        &[axis("record_level.offset", &[3.0])],
        &f,
        &[0],
        false,
    )
    .unwrap();
    let entry = &checked[0].effective.mechanisms[0];
    assert_eq!(entry.params.0["offset"], toml::Value::Integer(3));
}

#[test]
fn legacy_run_sweep_with_empty_axis_returns_no_points() {
    let base = Scenario::parse(LEGACY_TOML).unwrap();
    let wf = world_factory::<TestWorld>();
    let points = run_sweep(
        &base,
        &[axis("record_level.offset", &[])],
        &wf,
        &register_test,
        vec![0],
        false,
    )
    .unwrap();
    assert!(points.is_empty());
}
