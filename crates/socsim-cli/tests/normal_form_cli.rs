use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

use socsim_config::Scenario;

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "socsim_cli_{tag}_{}_{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or(0)
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("create integration-test temp directory");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn socsim<I, S>(cwd: &Path, args: I) -> Output
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    Command::new(env!("CARGO_BIN_EXE_socsim"))
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("run socsim binary")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "command failed\nstdout:\n{}\nstderr:\n{}",
        stdout(output),
        stderr(output)
    );
}

fn assert_exit_one(output: &Output) {
    assert_eq!(
        output.status.code(),
        Some(1),
        "unexpected status\nstdout:\n{}\nstderr:\n{}",
        stdout(output),
        stderr(output)
    );
}

fn write_scenario(dir: &Path, name: &str, mut scenario: Scenario) -> PathBuf {
    scenario.output.log_path = dir
        .join("logs/{name}_{seed}.jsonl")
        .to_string_lossy()
        .into_owned();
    let path = dir.join(name);
    fs::write(
        &path,
        toml::to_string(&scenario).expect("serialize scenario"),
    )
    .expect("write scenario in temp directory");
    path
}

fn small_nowak_may(dir: &Path, name: &str) -> PathBuf {
    let mut scenario = Scenario::parse(include_str!(
        "../../../scenarios/normal_form_nowak_may.toml"
    ))
    .expect("bundled Nowak-May scenario parses");
    scenario.simulation.t_max = 5;
    scenario
        .world
        .0
        .insert("grid_rows".to_owned(), toml::Value::Integer(10));
    scenario
        .world
        .0
        .insert("grid_cols".to_owned(), toml::Value::Integer(10));
    write_scenario(dir, name, scenario)
}

fn small_normal_form(dir: &Path, name: &str, metrics: Vec<String>) -> PathBuf {
    let mut scenario = Scenario::parse(include_str!("../../../scenarios/normal_form_ipd.toml"))
        .expect("bundled normal-form scenario parses");
    scenario.simulation.t_max = 2;
    scenario.output.metrics = metrics;
    write_scenario(dir, name, scenario)
}

fn series_header(output: &Output) -> Vec<String> {
    stdout(output)
        .lines()
        .find(|line| line.split_whitespace().next() == Some("t"))
        .expect("series header")
        .split_whitespace()
        .map(str::to_owned)
        .collect()
}

fn summary_metric_keys(output: &Output) -> Vec<String> {
    stdout(output)
        .lines()
        .filter_map(|line| {
            let key = line.split_whitespace().next()?;
            key.starts_with("game.").then(|| key.to_owned())
        })
        .collect()
}

fn assert_no_files_with_extensions(root: &Path, extensions: &[&str]) {
    if !root.exists() {
        return;
    }
    let mut pending = vec![root.to_path_buf()];
    while let Some(path) = pending.pop() {
        for entry in fs::read_dir(path).expect("read test output directory") {
            let path = entry.expect("read directory entry").path();
            if path.is_dir() {
                pending.push(path);
            } else if let Some(extension) = path.extension().and_then(OsStr::to_str) {
                assert!(
                    !extensions.contains(&extension),
                    "unexpected output file: {}",
                    path.display()
                );
            }
        }
    }
}

fn assert_no_files(root: &Path) {
    if !root.exists() {
        return;
    }
    let mut pending = vec![root.to_path_buf()];
    while let Some(path) = pending.pop() {
        for entry in fs::read_dir(path).expect("read test output directory") {
            let path = entry.expect("read directory entry").path();
            assert!(path.is_dir(), "unexpected output file: {}", path.display());
            pending.push(path);
        }
    }
}

#[test]
fn list_includes_normal_form_pack_and_sorted_mechanisms() {
    let dir = TempDir::new("normal_form_list");

    let packs = socsim(dir.path(), ["list", "packs"]);
    assert_success(&packs);
    assert!(stdout(&packs).lines().any(|line| line == "  normal-form"));

    let mechanisms = socsim(dir.path(), ["list", "mechanisms"]);
    assert_success(&mechanisms);
    let text = stdout(&mechanisms);
    let lines: Vec<&str> = text.lines().collect();
    let start = lines
        .iter()
        .position(|line| *line == "  [normal-form]")
        .expect("normal-form mechanism section");
    assert_eq!(
        &lines[start + 1..start + 10],
        &[
            "    continuation_probability",
            "    fermi_update",
            "    game_decide",
            "    game_match",
            "    game_metrics",
            "    game_play",
            "    imitate_best",
            "    moran_update",
            "    replicator_update",
        ]
    );
}

#[test]
fn init_validate_and_run_normal_form_scenario() {
    let dir = TempDir::new("normal_form_run");

    let init = socsim(
        dir.path(),
        ["init", "--module-pack", "normal-form", "-o", "x.toml"],
    );
    assert_success(&init);
    let validate = socsim(dir.path(), ["validate", "x.toml"]);
    assert_success(&validate);
    let run = socsim(dir.path(), ["run", "x.toml", "--seeds", "0..3"]);
    assert_success(&run);

    for seed in 0..3 {
        assert!(dir
            .path()
            .join(format!("runs/ipd_tft_vs_fp_{seed}.jsonl"))
            .is_file());
    }
    assert!(!dir.path().join("game_spec.json").exists());
    assert!(!dir.path().join("solutions.json").exists());
}

#[test]
fn every_compiled_pack_starter_validates() {
    let dir = TempDir::new("all_starters_validate");
    for pack in [
        "hr-lifecycle",
        "opinion-dynamics",
        "organizational-silence",
        "normal-form",
    ] {
        let name = format!("{pack}.toml");
        let init = socsim(
            dir.path(),
            ["init", "--module-pack", pack, "-o", name.as_str()],
        );
        assert_success(&init);
        let validate = socsim(dir.path(), ["validate", name.as_str()]);
        assert_success(&validate);
    }
}

#[test]
fn every_bundled_scenario_for_compiled_features_validates() {
    let dir = TempDir::new("all_scenarios_validate");
    let scenarios_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scenarios");
    let mut scenarios: Vec<PathBuf> = fs::read_dir(scenarios_dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension() == Some(OsStr::new("toml")))
        .filter(|path| {
            cfg!(feature = "pack-organizational-silence-llm")
                || path.file_name() != Some(OsStr::new("org_silence_llm.toml"))
        })
        .collect();
    scenarios.sort();

    for scenario in scenarios {
        let validate = socsim(dir.path(), ["validate", scenario.to_str().unwrap()]);
        assert_success(&validate);
    }
}

#[cfg(feature = "pack-organizational-silence-llm")]
#[test]
fn llm_bundled_scenario_validates_when_its_feature_is_compiled() {
    let dir = TempDir::new("llm_scenario_validate");
    let scenario =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scenarios/org_silence_llm.toml");
    let validate = socsim(dir.path(), ["validate", scenario.to_str().unwrap()]);
    assert_success(&validate);
}

#[test]
fn run_selects_metrics_in_requested_order_for_single_and_multiple_seeds() {
    let dir = TempDir::new("normal_form_selected_metrics");
    let selected = vec![
        "game.regret".to_owned(),
        "game.mean_payoff".to_owned(),
        "game.played_share.a0".to_owned(),
        "game.regret".to_owned(),
    ];
    let scenario_path = small_normal_form(dir.path(), "selected.toml", selected);

    let single = socsim(dir.path(), ["run", scenario_path.to_str().unwrap()]);
    assert_success(&single);
    assert_eq!(
        series_header(&single),
        [
            "t",
            "game.regret",
            "game.mean_payoff",
            "game.played_share.a0"
        ]
    );

    let multiple = socsim(
        dir.path(),
        ["run", scenario_path.to_str().unwrap(), "--seeds", "0..2"],
    );
    assert_success(&multiple);
    assert_eq!(
        summary_metric_keys(&multiple),
        ["game.regret", "game.mean_payoff", "game.played_share.a0"]
    );
}

#[test]
fn run_with_empty_metrics_keeps_all_series_columns() {
    let dir = TempDir::new("normal_form_all_metrics");
    let scenario_path = small_normal_form(dir.path(), "all.toml", Vec::new());

    let output = socsim(dir.path(), ["run", scenario_path.to_str().unwrap()]);
    assert_success(&output);
    let header = series_header(&output);
    assert!(header.len() > 4, "expected all metric columns: {header:?}");
    assert!(header.iter().any(|key| key == "game.payoff_sd"));
    assert!(header.iter().any(|key| key == "game.played_share.a1"));

    let multiple = socsim(
        dir.path(),
        ["run", scenario_path.to_str().unwrap(), "--seeds", "0..2"],
    );
    assert_success(&multiple);
    let keys = summary_metric_keys(&multiple);
    assert!(keys.len() > 3, "expected all summary metrics: {keys:?}");
    assert!(keys.iter().any(|key| key == "game.payoff_sd"));
    assert!(keys.iter().any(|key| key == "game.played_share.a1"));
}

#[test]
fn sweep_selects_metrics_in_stdout_and_csv_order() {
    let dir = TempDir::new("normal_form_sweep_selected_metrics");
    let scenario_path = small_normal_form(
        dir.path(),
        "sweep.toml",
        vec!["game.regret".to_owned(), "game.mean_payoff".to_owned()],
    );
    let out_dir = dir.path().join("out");

    let output = socsim(
        dir.path(),
        [
            "sweep",
            scenario_path.to_str().unwrap(),
            "--param",
            "world.history_len=2,3",
            "--seeds",
            "0..2",
            "-o",
            out_dir.to_str().unwrap(),
        ],
    );
    assert_success(&output);
    assert_eq!(
        summary_metric_keys(&output),
        [
            "game.regret",
            "game.mean_payoff",
            "game.regret",
            "game.mean_payoff"
        ]
    );

    for entry in fs::read_dir(&out_dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension() != Some(OsStr::new("csv")) {
            continue;
        }
        let text = fs::read_to_string(path).unwrap();
        let keys: Vec<&str> = text
            .lines()
            .skip(1)
            .filter_map(|line| line.split(',').next())
            .collect();
        assert_eq!(keys, ["game.regret", "game.mean_payoff"]);
    }
}

#[test]
fn validate_rejects_unknown_normal_form_metric_with_patterns() {
    let dir = TempDir::new("normal_form_unknown_metric");
    let scenario_path = small_normal_form(dir.path(), "unknown.toml", vec!["game.nope".to_owned()]);

    let output = socsim(dir.path(), ["validate", scenario_path.to_str().unwrap()]);
    assert_exit_one(&output);
    let error = stderr(&output);
    assert!(error.contains("game.nope"), "{error}");
    assert!(error.contains("game.played_share.{action}"), "{error}");
    assert!(error.contains("game.mean_payoff"), "{error}");
}

#[test]
fn pattern_match_without_a_recorded_metric_fails_run_and_sweep_after_execution() {
    let dir = TempDir::new("normal_form_absent_metric");
    let scenario_path = small_normal_form(
        dir.path(),
        "absent.toml",
        vec!["game.played_share.a9".to_owned()],
    );

    let validate = socsim(dir.path(), ["validate", scenario_path.to_str().unwrap()]);
    assert_success(&validate);

    let run = socsim(dir.path(), ["run", scenario_path.to_str().unwrap()]);
    assert_exit_one(&run);
    let run_error = stderr(&run);
    assert!(run_error.contains("game.played_share.a9"), "{run_error}");
    assert!(run_error.contains("game.played_share.a0"), "{run_error}");
    assert!(dir.path().join("logs/ipd_tft_vs_fp_42.jsonl").is_file());

    let out_dir = dir.path().join("out");
    let sweep = socsim(
        dir.path(),
        [
            "sweep",
            scenario_path.to_str().unwrap(),
            "--param",
            "world.history_len=2",
            "--seeds",
            "0..1",
            "-o",
            out_dir.to_str().unwrap(),
        ],
    );
    assert_exit_one(&sweep);
    let sweep_error = stderr(&sweep);
    assert!(
        sweep_error.contains("game.played_share.a9"),
        "{sweep_error}"
    );
    assert!(
        sweep_error.contains("game.played_share.a0"),
        "{sweep_error}"
    );
    assert!(!out_dir.exists());
}

#[test]
fn unknown_metric_without_patterns_fails_after_writing_full_jsonl() {
    let dir = TempDir::new("opinion_unknown_metric");
    let mut scenario = Scenario::parse(include_str!(
        "../../../scenarios/opinion_dynamics_baseline.toml"
    ))
    .expect("bundled opinion scenario parses");
    scenario.simulation.t_max = 1;
    scenario.output.metrics = vec!["opinion.nope".to_owned()];
    let log_path = dir.path().join(format!(
        "logs/{}_{}.jsonl",
        scenario.simulation.name, scenario.simulation.seed
    ));
    let scenario_path = write_scenario(dir.path(), "opinion.toml", scenario);

    let output = socsim(dir.path(), ["run", scenario_path.to_str().unwrap()]);
    assert_exit_one(&output);
    let error = stderr(&output);
    assert!(error.contains("opinion.nope"), "{error}");
    assert!(error.contains("clusters"), "{error}");
    let log = fs::read_to_string(&log_path).expect("JSONL is written before validation fails");
    assert!(log.contains(r#""key":"clusters""#), "{log}");
    assert!(log.contains(r#""key":"variance""#), "{log}");
}

#[test]
fn sweep_writes_two_effective_scenarios_with_patched_float_values() {
    let dir = TempDir::new("normal_form_sweep");
    let scenario_path = small_nowak_may(dir.path(), "nowak_may.toml");
    let out_dir = dir.path().join("out");

    let output = socsim(
        dir.path(),
        [
            "sweep",
            scenario_path.to_str().unwrap(),
            "--param",
            "world.game.temptation=1.6,1.9",
            "--seeds",
            "0..2",
            "-o",
            out_dir.to_str().unwrap(),
        ],
    );
    assert_success(&output);

    let expected = [
        ("combo_0000_world.game.temptation=1.6000.scenario.toml", 1.6),
        ("combo_0001_world.game.temptation=1.9000.scenario.toml", 1.9),
    ];
    for (name, value) in expected {
        let text = fs::read_to_string(out_dir.join(name)).expect("effective scenario exists");
        let scenario = Scenario::parse(&text).expect("effective scenario parses");
        assert_eq!(
            scenario.world.0["game"].as_table().unwrap()["temptation"],
            toml::Value::Float(value)
        );
    }
    let scenario_files = fs::read_dir(&out_dir)
        .unwrap()
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            path.file_name()
                .and_then(OsStr::to_str)
                .filter(|name| name.ends_with(".scenario.toml"))
                .map(str::to_owned)
        })
        .count();
    assert_eq!(scenario_files, 2);
    assert!(!dir.path().join("game_spec.json").exists());
    assert!(!dir.path().join("solutions.json").exists());
}

#[test]
fn sweep_rejects_unknown_world_path_and_mechanism_without_files() {
    let dir = TempDir::new("normal_form_bad_sweep_axes");
    let scenario_path = small_nowak_may(dir.path(), "nowak_may.toml");

    for (index, axis) in [
        "world.game.temptaton=1.6,1.9",
        "game_decid.logit_lambda=1.0,2.0",
    ]
    .into_iter()
    .enumerate()
    {
        let out_dir = dir.path().join(format!("out_{index}"));
        let output = socsim(
            dir.path(),
            [
                "sweep",
                scenario_path.to_str().unwrap(),
                "--param",
                axis,
                "--seeds",
                "0..2",
                "-o",
                out_dir.to_str().unwrap(),
            ],
        );
        assert_exit_one(&output);
        assert_no_files(&out_dir);
    }
}

#[test]
fn run_validate_and_sweep_reject_each_validation_layer_before_writing() {
    let dir = TempDir::new("normal_form_validation_layers");
    let base_path = small_nowak_may(dir.path(), "base.toml");
    let base_text = fs::read_to_string(&base_path).unwrap();

    let cases = [
        (
            "zero.toml",
            base_text.replacen("t_max = 5", "t_max = 0", 1),
            "simulation.t_max must be > 0",
        ),
        (
            "typo.toml",
            base_text.replacen(
                "temptation = 1.85",
                "temptation = 1.85\ntemptaton = 1.85",
                1,
            ),
            "world.game.temptaton",
        ),
    ];

    for (index, (name, text, expected_error)) in cases.into_iter().enumerate() {
        let scenario_path = dir.path().join(name);
        fs::write(&scenario_path, text).unwrap();

        let validate = socsim(dir.path(), ["validate", scenario_path.to_str().unwrap()]);
        assert_exit_one(&validate);
        assert!(stderr(&validate).contains("validation failed for"));
        assert!(
            stderr(&validate).contains(expected_error),
            "validate stderr did not contain {expected_error:?}:\n{}",
            stderr(&validate)
        );

        let run = socsim(dir.path(), ["run", scenario_path.to_str().unwrap()]);
        assert_exit_one(&run);
        assert!(stderr(&run).contains("validation failed for"));
        assert!(stderr(&run).contains(expected_error));
        assert!(!stdout(&run).contains("Running"));

        let out_dir = dir.path().join(format!("invalid_out_{index}"));
        let sweep = socsim(
            dir.path(),
            [
                "sweep",
                scenario_path.to_str().unwrap(),
                "--param",
                "world.game.temptation=1.6,1.9",
                "--seeds",
                "0..2",
                "-o",
                out_dir.to_str().unwrap(),
            ],
        );
        assert_exit_one(&sweep);
        assert!(stderr(&sweep).contains("validation failed for"));
        assert!(stderr(&sweep).contains(expected_error));
        assert!(!stdout(&sweep).contains("Sweeping"));
        assert_no_files_with_extensions(&out_dir, &["csv", "toml"]);
    }

    assert_no_files_with_extensions(dir.path(), &["jsonl", "csv"]);
}
