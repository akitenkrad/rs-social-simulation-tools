use std::borrow::Cow;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use socsim_games::{
    write_artifacts, write_game_spec, write_solutions, ActionError, EssLabel, ExportError,
    FiniteGame, GameSpec, NormalFormGame, SolutionSet, StageGame,
};

#[path = "support/viz_games.rs"]
mod viz_games;

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

const REGEN_ENV: &str = "SOCSIM_GAMES_REGEN_FIXTURES";

#[derive(Clone)]
struct SyntheticFinite {
    counts: Vec<usize>,
    labels: Vec<Vec<String>>,
    payoff: f64,
}

impl StageGame for SyntheticFinite {
    type Action = usize;

    fn n_roles(&self) -> usize {
        self.counts.len()
    }

    fn check_action(&self, role: usize, action: &usize) -> Result<(), ActionError> {
        if role >= self.counts.len() {
            return Err(ActionError::InvalidRole { role });
        }
        if *action >= self.counts[role] {
            return Err(ActionError::InvalidAction {
                role,
                action: *action,
            });
        }
        Ok(())
    }
}

impl NormalFormGame for SyntheticFinite {
    fn payoff(&self, _role: usize, _profile: &[usize]) -> f64 {
        self.payoff
    }
}

impl FiniteGame for SyntheticFinite {
    fn n_actions(&self, role: usize) -> usize {
        self.counts[role]
    }

    fn action_label(&self, role: usize, action: usize) -> Cow<'_, str> {
        self.labels
            .get(role)
            .and_then(|labels| labels.get(action))
            .map_or_else(
                || Cow::Owned(format!("a{action}")),
                |label| Cow::Borrowed(label.as_str()),
            )
    }
}

fn one_action_game(payoff: f64, label: &str) -> SyntheticFinite {
    SyntheticFinite {
        counts: vec![1],
        labels: vec![vec![label.into()]],
        payoff,
    }
}

fn read_json(path: &Path) -> TestResult<Value> {
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

fn fixture_path(game: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/viz")
        .join(game)
        .join("game_spec.json")
}

#[test]
fn ten_game_specs_match_fixtures_byte_for_byte() -> TestResult {
    for (game, spec) in viz_games::game_specs()? {
        let directory = tempfile::tempdir()?;
        let actual_path = directory.path().join("game_spec.json");
        write_game_spec(&spec, &actual_path)?;
        let actual = fs::read(&actual_path)?;
        let fixture = fixture_path(game);

        if std::env::var(REGEN_ENV).as_deref() == Ok("1") {
            fs::create_dir_all(fixture.parent().expect("fixture has a parent"))?;
            fs::write(&fixture, &actual)?;
        } else {
            let expected = fs::read(&fixture)?;
            assert_eq!(actual, expected, "fixture differs for {game}");
        }
    }
    Ok(())
}

#[test]
fn finite_spec_preserves_layout_labels_metrics_and_params() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("game_spec.json");
    let spec = viz_games::prisoners_dilemma()?.1;
    write_game_spec(&spec, &path)?;
    let value = read_json(&path)?;

    assert_eq!(value["schema"], "socsim-games.game-spec");
    assert_eq!(value["version"], 1);
    assert_eq!(value["kind"], "finite");
    assert_eq!(value["name"], "prisoners_dilemma");
    assert_eq!(value["n_roles"], 2);
    assert_eq!(value["role_labels"], json!(["role0", "role1"]));
    assert_eq!(value["action_labels"], json!([["C", "D"], ["C", "D"]]));
    assert_eq!(value["payoffs"]["layout"], "row_major_role0_outer");
    assert_eq!(value["payoffs"]["shape"], json!([2, 2]));
    assert_eq!(
        value["payoffs"]["values"],
        json!([[3.0, 0.0, 5.0, 1.0], [3.0, 5.0, 0.0, 1.0]])
    );
    assert_eq!(value["symmetric"], true);
    assert_eq!(value["metric_scheme"], "pooled");
    assert_eq!(
        value["metric_actions"],
        json!([
            {"id": "a0", "role": null, "action": 0},
            {"id": "a1", "role": null, "action": 1}
        ])
    );
    assert_eq!(
        value["params"],
        json!({"temptation": 5.0, "reward": 3.0, "punishment": 1.0, "sucker": 0.0})
    );
    assert!(value.get("bounds").is_none());
    assert!(value.get("dimension_labels").is_none());
    Ok(())
}

#[test]
fn three_role_payoffs_follow_role_zero_outer_order() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("game_spec.json");
    write_game_spec(&viz_games::matrix_3p()?.1, &path)?;
    let value = read_json(&path)?;

    assert_eq!(value["payoffs"]["shape"], json!([2, 2, 2]));
    assert_eq!(
        value["payoffs"]["values"],
        json!([
            [0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0],
            [100.0, 101.0, 102.0, 103.0, 104.0, 105.0, 106.0, 107.0],
            [200.0, 201.0, 202.0, 203.0, 204.0, 205.0, 206.0, 207.0]
        ])
    );
    assert_eq!(value["symmetric"], false);
    assert!(value.get("params").is_none());
    Ok(())
}

#[test]
fn finite_spec_rejects_invalid_values_and_labels() {
    let non_finite = one_action_game(f64::NAN, "a");
    let error = GameSpec::from_finite(&non_finite, "bad", None).unwrap_err();
    assert!(matches!(error, ExportError::NonFinite { .. }));
    assert!(error.to_string().contains("payoffs[0][0]"));

    let too_large = SyntheticFinite {
        counts: vec![500_001, 1],
        labels: Vec::new(),
        payoff: 0.0,
    };
    let error = GameSpec::from_finite(&too_large, "large", None).unwrap_err();
    assert!(matches!(
        error,
        ExportError::TooLarge {
            elements: 1_000_002,
            limit: 1_000_000
        }
    ));

    let game = one_action_game(0.0, "a");
    let error = GameSpec::from_finite(&game, "game", Some(vec![])).unwrap_err();
    assert!(matches!(error, ExportError::Shape(_)));
    assert!(error.to_string().contains("role_labels"));

    let error = GameSpec::from_finite(&game, "game", Some(vec![String::new()])).unwrap_err();
    assert!(matches!(error, ExportError::EmptyLabel { .. }));
    assert!(error.to_string().contains("role_labels[0]"));

    let empty_action = one_action_game(0.0, "");
    let error = GameSpec::from_finite(&empty_action, "game", None).unwrap_err();
    assert!(matches!(error, ExportError::EmptyLabel { .. }));
    assert!(error.to_string().contains("action_labels[0][0]"));

    let error = GameSpec::from_finite(&game, "", None).unwrap_err();
    assert!(matches!(error, ExportError::EmptyLabel { .. }));
    assert!(error.to_string().contains("name"));
}

#[test]
fn continuous_spec_uses_strict_bounds_and_matching_label_shapes() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("game_spec.json");
    let spec = viz_games::cournot()?.1.with_role_labels(vec![
        "firm0".into(),
        "firm1".into(),
        "firm2".into(),
    ])?;
    write_game_spec(&spec, &path)?;
    let value = read_json(&path)?;

    assert_eq!(value["kind"], "continuous");
    assert_eq!(value["role_labels"], json!(["firm0", "firm1", "firm2"]));
    assert_eq!(
        value["bounds"],
        json!([[[0.0, 100.0]], [[0.0, 100.0]], [[0.0, 100.0]]])
    );
    assert_eq!(value["dimension_labels"], json!([["x0"], ["x0"], ["x0"]]));
    assert_eq!(
        value["params"],
        json!({"n": 3, "a": 100.0, "b": 1.0, "c": 10.0, "q_max": 100.0})
    );
    assert!(value.get("action_labels").is_none());
    assert!(value.get("payoffs").is_none());
    assert!(value.get("symmetric").is_none());
    assert!(value.get("metric_scheme").is_none());
    assert!(value.get("metric_actions").is_none());
    Ok(())
}

#[test]
fn continuous_spec_rejects_invalid_bounds_and_labels() {
    for (bounds, field) in [
        (vec![], "bounds"),
        (vec![vec![]], "bounds[0]"),
        (vec![vec![(1.0, 1.0)]], "bounds[0][0]"),
        (vec![vec![(2.0, 1.0)]], "bounds[0][0]"),
        (vec![vec![(0.0, f64::INFINITY)]], "bounds[0][0]"),
    ] {
        let error = GameSpec::continuous("continuous", bounds, None).unwrap_err();
        assert!(error.to_string().contains(field), "{error}");
    }

    let error = GameSpec::continuous(
        "continuous",
        vec![vec![(0.0, 1.0)]],
        Some(vec![vec!["x".into(), "y".into()]]),
    )
    .unwrap_err();
    assert!(matches!(error, ExportError::Shape(_)));
    assert!(error.to_string().contains("dimension_labels[0]"));

    let error = GameSpec::continuous(
        "continuous",
        vec![vec![(0.0, 1.0)]],
        Some(vec![vec![String::new()]]),
    )
    .unwrap_err();
    assert!(matches!(error, ExportError::EmptyLabel { .. }));
    assert!(error.to_string().contains("dimension_labels[0][0]"));

    let spec = GameSpec::continuous("continuous", vec![vec![(0.0, 1.0)]], None).unwrap();
    let error = spec.with_role_labels(vec![]).unwrap_err();
    assert!(matches!(error, ExportError::Shape(_)));
    assert!(error.to_string().contains("role_labels"));
}

#[test]
fn solution_set_serializes_all_flat_solution_kinds() -> TestResult {
    let spec = viz_games::prisoners_dilemma()?.1;
    let mut solutions = SolutionSet::new(&spec);
    solutions.push_nash(
        "nash-pure",
        "manual",
        vec![vec![1.0, 0.0], vec![0.0, 1.0]],
        vec![0.0, 5.0],
        0.0,
    )?;
    solutions.push_nash(
        "nash-near",
        "manual",
        vec![vec![1.0 - f64::EPSILON, f64::EPSILON], vec![0.0, 1.0]],
        vec![0.0, 5.0],
        0.0,
    )?;
    solutions.push_qre("qre", 1.0, vec![vec![0.5, 0.5], vec![0.5, 0.5]], 0.0)?;
    solutions.push_population(
        "population",
        "manual",
        vec![0.25, 0.75],
        Some(EssLabel::Inconclusive),
    )?;
    solutions.push_population_component(
        "component",
        "manual",
        vec![0.5, 0.5],
        vec![vec![1.0, -1.0]],
    )?;
    solutions.push_inconclusive(
        "finding",
        "manual",
        vec![vec![0], vec![1]],
        "degenerate support",
    )?;

    let directory = tempfile::tempdir()?;
    let path = directory.path().join("solutions.json");
    write_solutions(&solutions, &path)?;
    let value = read_json(&path)?;

    assert_eq!(value["schema"], "socsim-games.solutions");
    assert_eq!(value["version"], 1);
    assert_eq!(value["game"]["name"], "prisoners_dilemma");
    assert_eq!(value["solutions"][0]["kind"], "nash");
    assert_eq!(value["solutions"][0]["pure"], true);
    assert_eq!(value["solutions"][1]["pure"], false);
    assert_eq!(value["solutions"][2]["kind"], "qre");
    assert_eq!(value["solutions"][2]["solver"], "logit_qre");
    assert_eq!(value["solutions"][3]["ess"], "inconclusive");
    assert_eq!(value["solutions"][4]["kind"], "population_component");
    assert_eq!(value["solutions"][5]["kind"], "inconclusive");
    Ok(())
}

#[test]
fn ess_labels_and_absence_use_the_contract_strings_and_null() -> TestResult {
    let spec = viz_games::prisoners_dilemma()?.1;
    let mut solutions = SolutionSet::new(&spec);
    for (id, ess) in [
        ("true", Some(EssLabel::True)),
        ("false", Some(EssLabel::False)),
        ("inconclusive", Some(EssLabel::Inconclusive)),
        ("none", None),
    ] {
        solutions.push_population(id, "manual", vec![0.5, 0.5], ess)?;
    }

    let directory = tempfile::tempdir()?;
    let path = directory.path().join("solutions.json");
    write_solutions(&solutions, &path)?;
    let value = read_json(&path)?;
    assert_eq!(value["solutions"][0]["ess"], "true");
    assert_eq!(value["solutions"][1]["ess"], "false");
    assert_eq!(value["solutions"][2]["ess"], "inconclusive");
    assert_eq!(value["solutions"][3]["ess"], Value::Null);
    Ok(())
}

#[test]
fn solution_set_validates_shape_probability_finiteness_kind_and_ids() -> TestResult {
    let finite = viz_games::prisoners_dilemma()?.1;

    let mut solutions = SolutionSet::new(&finite);
    let error = solutions
        .push_nash(
            "bad-shape",
            "manual",
            vec![vec![1.0, 0.0]],
            vec![0.0, 0.0],
            0.0,
        )
        .unwrap_err();
    assert!(matches!(error, ExportError::Mismatch(_)));
    assert!(error.to_string().contains("bad-shape"));

    let error = solutions
        .push_qre(
            "bad-probability",
            1.0,
            vec![vec![1.1, -0.1], vec![0.5, 0.5]],
            0.0,
        )
        .unwrap_err();
    assert!(matches!(error, ExportError::Probability { role: 0, .. }));
    assert!(error.to_string().contains("bad-probability"));

    let error = solutions
        .push_population("bad-sum", "manual", vec![0.4, 0.4], None)
        .unwrap_err();
    assert!(matches!(error, ExportError::Probability { .. }));

    let error = solutions
        .push_inconclusive(
            "bad-support",
            "manual",
            vec![vec![0], vec![2]],
            "invalid action",
        )
        .unwrap_err();
    assert!(matches!(error, ExportError::Mismatch(_)));
    assert!(error.to_string().contains("bad-support"));

    solutions.push_population("duplicate", "manual", vec![0.5, 0.5], None)?;
    let error = solutions
        .push_population("duplicate", "manual", vec![0.5, 0.5], None)
        .unwrap_err();
    assert!(matches!(error, ExportError::DuplicateId(_)));
    assert!(error.to_string().contains("duplicate"));

    let continuous = viz_games::cournot()?.1;
    let mut continuous_solutions = SolutionSet::new(&continuous);
    let error = continuous_solutions
        .push_nash("finite-on-continuous", "manual", vec![], vec![], 0.0)
        .unwrap_err();
    assert!(matches!(error, ExportError::Mismatch(_)));
    assert!(error.to_string().contains("finite-on-continuous"));

    continuous_solutions.push_continuous(
        "continuous",
        "manual",
        vec![vec![22.5], vec![22.5], vec![22.5]],
        0.0,
        true,
    )?;
    let error = continuous_solutions
        .push_continuous(
            "bad-dimension",
            "manual",
            vec![vec![], vec![22.5], vec![22.5]],
            0.0,
            false,
        )
        .unwrap_err();
    assert!(matches!(error, ExportError::Mismatch(_)));
    assert!(error.to_string().contains("bad-dimension"));

    let error = continuous_solutions
        .push_continuous(
            "non-finite",
            "manual",
            vec![vec![f64::NAN], vec![22.5], vec![22.5]],
            0.0,
            false,
        )
        .unwrap_err();
    assert!(matches!(error, ExportError::NonFinite { .. }));
    assert!(error.to_string().contains("non-finite"));
    Ok(())
}

#[test]
fn population_requires_a_common_action_count() -> TestResult {
    let spec = viz_games::asym_2x3()?.1;
    let mut solutions = SolutionSet::new(&spec);
    let error = solutions
        .push_population("population", "manual", vec![0.5, 0.5], None)
        .unwrap_err();
    assert!(matches!(error, ExportError::Mismatch(_)));
    assert!(error.to_string().contains("population"));
    Ok(())
}

#[test]
fn write_functions_use_explicit_paths_and_leave_no_temporary_files() -> TestResult {
    let spec = viz_games::prisoners_dilemma()?.1.with_params(json!({
        "display_only": [1, 2, 3]
    }));
    let directory = tempfile::tempdir()?;
    let missing_parent = directory.path().join("missing/game_spec.json");
    let error = write_game_spec(&spec, &missing_parent).unwrap_err();
    assert!(matches!(error, ExportError::Io { .. }));
    assert!(error.to_string().contains("game_spec.json"));
    assert!(!missing_parent.exists());

    let artifact_root = directory.path().join("artifacts");
    fs::create_dir(&artifact_root)?;
    let solutions = SolutionSet::new(&spec);
    write_artifacts(&artifact_root, &spec, Some(&solutions))?;
    let output_dir = artifact_root.join("socsim_games");
    let game_path = output_dir.join("game_spec.json");
    assert_eq!(
        read_json(&game_path)?["params"]["display_only"],
        json!([1, 2, 3])
    );
    assert_eq!(
        read_json(&output_dir.join("solutions.json"))?["solutions"],
        json!([])
    );

    write_artifacts(&artifact_root, &spec, Some(&solutions))?;
    let mut names = fs::read_dir(&output_dir)?
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect::<Result<Vec<_>, _>>()?;
    names.sort();
    assert_eq!(names, ["game_spec.json", "solutions.json"]);
    Ok(())
}
