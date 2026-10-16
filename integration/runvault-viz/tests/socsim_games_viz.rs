use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use runvault::{Origin, Run, RunOptions};
use runvault_socsim::RunvaultRecorder;
use serde_json::Value;
use socsim_games::catalog::PrisonersDilemma;
use socsim_games::{write_artifacts, GameMetrics, MetricsOptions};

#[path = "../../../crates/socsim-games/tests/support/viz_ipd.rs"]
mod viz_ipd;

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

const METRICS_HEADER: &str = "run_uid,step,step_unit,scope,name,value";
const EVENT_SCHEMAS: [&str; 3] = [
    "x.socsim-games-viz.game.roster",
    "x.socsim-games-viz.game.step_actions",
    "x.socsim-games-viz.game.play",
];
const RESERVED_EVENT_FIELDS: [&str; 9] = [
    "schema", "run_uid", "ts", "unit_id", "t", "t_unit", "outcome", "censored", "budget",
];

fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn configured_metrics() -> TestResult<GameMetrics<PrisonersDilemma>> {
    let game = PrisonersDilemma::new(5.0, 3.0, 1.0, 0.0)?;
    Ok(GameMetrics::new(
        &game,
        MetricsOptions::new()
            .cooperate_label("C")
            .population_share(true)
            .log_actions(true)
            .log_plays(true),
    )?)
}

fn runvault_declaration(metrics: &GameMetrics<PrisonersDilemma>) -> TestResult<String> {
    let mut declaration = String::new();
    for (name, meaning) in metrics.describe() {
        let name = serde_json::to_string(&name)?;
        let meaning = serde_json::to_string(&meaning)?;
        if name.contains('{') {
            declaration.push_str("[[metric_patterns]]\n");
            declaration.push_str(&format!("pattern = {name}\nmeaning = {meaning}\n\n"));
        } else {
            declaration.push_str(&format!("[metrics.{name}]\nmeaning = {meaning}\n\n"));
        }
    }
    Ok(declaration)
}

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/socsim-games/tests/fixtures/viz/ipd")
}

fn jsonl_values(path: &Path) -> TestResult<Vec<Value>> {
    fs::read_to_string(path)?
        .lines()
        .enumerate()
        .map(|(index, line)| {
            serde_json::from_str(line).map_err(|error| {
                invalid_data(format!(
                    "{}:{} is not valid JSON: {error}",
                    path.display(),
                    index + 1
                ))
                .into()
            })
        })
        .collect()
}

fn fixture_metrics(path: &Path) -> TestResult<BTreeMap<(u64, String), f64>> {
    let mut metrics = BTreeMap::new();
    for value in jsonl_values(path)? {
        if value.get("type").and_then(Value::as_str) != Some("metric") {
            continue;
        }
        let t = value
            .get("t")
            .and_then(Value::as_u64)
            .ok_or_else(|| invalid_data("fixture metric has no integer t"))?;
        let name = value
            .get("key")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid_data("fixture metric has no key"))?
            .to_owned();
        let number = value
            .get("value")
            .and_then(Value::as_f64)
            .ok_or_else(|| invalid_data("fixture metric has no numeric value"))?;
        if metrics.insert((t, name), number).is_some() {
            return Err(invalid_data("fixture contains a duplicate metric").into());
        }
    }
    Ok(metrics)
}

fn run_metrics(path: &Path) -> TestResult<BTreeMap<(u64, String), f64>> {
    let text = fs::read_to_string(path)?;
    let mut lines = text.lines();
    let header = lines
        .next()
        .ok_or_else(|| invalid_data("metrics.csv is empty"))?;
    assert_eq!(header, METRICS_HEADER);

    let mut metrics = BTreeMap::new();
    for (index, line) in lines.enumerate() {
        let fields: Vec<_> = line.split(',').collect();
        if fields.len() != 6 {
            return Err(invalid_data(format!(
                "metrics.csv:{} has {} fields",
                index + 2,
                fields.len()
            ))
            .into());
        }
        assert_eq!(fields[2], "round");
        assert_eq!(fields[3], "run");
        let t = fields[1].parse()?;
        let value = fields[5].parse()?;
        if metrics.insert((t, fields[4].to_owned()), value).is_some() {
            return Err(invalid_data("metrics.csv contains a duplicate metric").into());
        }
    }
    Ok(metrics)
}

fn normalized_fixture_events(path: &Path) -> TestResult<Vec<Value>> {
    jsonl_values(path)?
        .into_iter()
        .filter(|value| value.get("type").and_then(Value::as_str) == Some("event"))
        .map(|value| {
            let t = value
                .get("t")
                .cloned()
                .ok_or_else(|| invalid_data("fixture event has no t"))?;
            let kind = value
                .get("kind")
                .and_then(Value::as_str)
                .ok_or_else(|| invalid_data("fixture event has no kind"))?;
            let payload = value
                .get("payload")
                .and_then(Value::as_object)
                .ok_or_else(|| invalid_data("fixture event payload is not an object"))?;
            let mut normalized = payload.clone();
            normalized.insert(
                "schema".to_owned(),
                Value::String(format!("x.socsim-games-viz.{kind}")),
            );
            normalized.insert("t".to_owned(), t);
            Ok(Value::Object(normalized))
        })
        .collect()
}

fn normalized_run_events(path: &Path) -> TestResult<Vec<Value>> {
    jsonl_values(path)?
        .into_iter()
        .map(|value| {
            let mut object = value
                .as_object()
                .cloned()
                .ok_or_else(|| invalid_data("runvault event is not an object"))?;
            for field in RESERVED_EVENT_FIELDS {
                if field != "schema" && field != "t" {
                    object.remove(field);
                }
            }
            Ok(Value::Object(object))
        })
        .collect()
}

fn copy_tree(source: &Path, destination: &Path) -> TestResult {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let kind = entry.file_type()?;
        if kind.is_dir() {
            copy_tree(&source_path, &destination_path)?;
        } else if kind.is_file() {
            fs::copy(&source_path, &destination_path)?;
        } else {
            return Err(invalid_data(format!(
                "cannot copy non-file run entry {}",
                source_path.display()
            ))
            .into());
        }
    }
    Ok(())
}

#[test]
fn real_runvault_run_matches_the_ipd_fixture() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let repo_root = temporary.path().join("repo");
    fs::create_dir_all(&repo_root)?;

    let declaration = runvault_declaration(&configured_metrics()?)?;
    fs::write(repo_root.join("runvault.toml"), &declaration)?;
    println!("generated runvault.toml:\n{declaration}");

    let run = Run::start(
        RunOptions::new("socsim-games-viz", "test")
            .repo_id("socsim-games-viz")
            .domain("simulation")
            .master_seed(0)
            .origin(Origin::Manual)
            .scratch(true)
            .results_root(temporary.path().join("results"))
            .repo_root(&repo_root),
    )?;
    let recorder = RunvaultRecorder::new(run).step_unit("round");
    let handle = recorder.handle();
    let run_dir = handle
        .dir()
        .ok_or_else(|| invalid_data("run directory is unavailable before finish"))?;
    assert!(run_dir
        .components()
        .any(|part| part.as_os_str() == "_scratch"));

    let mut simulation = viz_ipd::build_simulation(0, Box::new(recorder))?;
    simulation.run()?;

    let (spec, solutions) = viz_ipd::artifacts()?;
    write_artifacts(&run_dir.join("artifacts"), &spec, Some(&solutions))?;
    let finished_dir = handle.finish()?;
    assert_eq!(finished_dir, run_dir);

    let fixture = fixture_dir();
    let fixture_log = fixture.join("seed_0.jsonl");
    let expected_metrics = fixture_metrics(&fixture_log)?;
    let actual_metrics = run_metrics(&run_dir.join("metrics.csv"))?;
    assert_eq!(actual_metrics, expected_metrics);
    assert_eq!(actual_metrics.len(), viz_ipd::ROUNDS as usize * 9);

    let actual_events = normalized_run_events(&run_dir.join("events.jsonl"))?;
    let expected_events = normalized_fixture_events(&fixture_log)?;
    assert_eq!(actual_events, expected_events);
    let schemas: BTreeSet<_> = actual_events
        .iter()
        .filter_map(|event| event.get("schema").and_then(Value::as_str))
        .collect();
    assert_eq!(schemas, EVENT_SCHEMAS.into_iter().collect());

    let artifacts = run_dir.join("artifacts/socsim_games");
    assert_eq!(
        fs::read(artifacts.join("game_spec.json"))?,
        fs::read(fixture.join("game_spec.json"))?
    );
    assert_eq!(
        fs::read(artifacts.join("solutions.json"))?,
        fs::read(fixture.join("solutions.json"))?
    );

    if let Some(destination) =
        std::env::var_os("SOCSIM_GAMES_VIZ_RUN_OUT").filter(|value| !value.is_empty())
    {
        copy_tree(&run_dir, Path::new(&destination))?;
    }
    Ok(())
}
