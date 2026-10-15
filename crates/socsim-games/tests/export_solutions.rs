use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use socsim_games::solve::{
    support_enumeration, BrIteration, BrStatus, EquilibriumSet, PureNash, SupportFinding,
};
use socsim_games::{write_artifacts, write_solutions, ExportError, SolutionSet};

#[path = "support/viz_games.rs"]
mod viz_games;

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

const REGEN_ENV: &str = "SOCSIM_GAMES_REGEN_FIXTURES";

fn fixture_path(game: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/viz")
        .join(game)
        .join("solutions.json")
}

fn serialize(solutions: &SolutionSet) -> TestResult<Value> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("solutions.json");
    write_solutions(solutions, &path)?;
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

#[test]
fn eight_solution_sets_match_solver_generated_fixtures_byte_for_byte() -> TestResult {
    for (game, _spec, solutions) in viz_games::solution_artifacts()? {
        let directory = tempfile::tempdir()?;
        let actual_path = directory.path().join("solutions.json");
        write_solutions(&solutions, &actual_path)?;
        let actual = fs::read(&actual_path)?;
        let fixture = fixture_path(game);

        if std::env::var(REGEN_ENV).as_deref() == Ok("1") {
            fs::write(&fixture, &actual)?;
        } else {
            assert_eq!(actual, fs::read(&fixture)?, "fixture differs for {game}");
        }
    }
    Ok(())
}

#[test]
fn conversion_methods_use_contract_solver_names_ids_and_values() -> TestResult {
    let (_, pd) = viz_games::prisoners_dilemma_artifacts()?;
    let pd = serialize(&pd)?;
    assert_eq!(
        pd["solutions"]
            .as_array()
            .expect("solutions is an array")
            .iter()
            .map(|solution| solution["id"].as_str().expect("id is a string"))
            .collect::<Vec<_>>(),
        ["pure-0", "se-0", "rep-0", "rep-1"]
    );
    assert_eq!(pd["solutions"][0]["solver"], "pure_nash");
    assert_eq!(pd["solutions"][1]["solver"], "support_enumeration");
    assert_eq!(pd["solutions"][2]["solver"], "replicator_fixed_points");
    assert_eq!(pd["solutions"][2]["shares"], json!([1.0, 0.0]));
    assert_eq!(pd["solutions"][2]["ess"], "false");
    assert_eq!(pd["solutions"][3]["shares"], json!([0.0, 1.0]));
    assert_eq!(pd["solutions"][3]["ess"], "true");

    let (_, matching) = viz_games::matching_pennies_artifacts()?;
    let matching = serialize(&matching)?;
    assert_eq!(matching["solutions"][1]["solver"], "zero_sum_minimax");
    assert_eq!(matching["solutions"][1]["payoffs"], json!([0.0, -0.0]));
    assert_eq!(matching["solutions"][2]["solver"], "logit_qre");
    assert_eq!(matching["solutions"][2]["lambda"], 1.0);
    assert_eq!(
        matching["solutions"][2]["strategies"],
        json!([[0.5, 0.5], [0.5, 0.5]])
    );

    let (_, cournot) = viz_games::cournot_artifacts()?;
    let cournot = serialize(&cournot)?;
    assert_eq!(cournot["solutions"][0]["solver"], "best_response_iteration");
    assert_eq!(
        cournot["solutions"][0]["actions"],
        json!([[22.5], [22.5], [22.5]])
    );
    assert_eq!(cournot["solutions"][0]["converged"], true);
    Ok(())
}

#[test]
fn equilibrium_set_findings_keep_their_index_spaces() -> TestResult {
    let (game, spec) = viz_games::battle_of_sexes()?;
    let solved = support_enumeration(&game)?;
    let base = solved.equilibria[0].clone();
    let set = EquilibriumSet {
        equilibria: vec![base.clone()],
        findings: vec![
            SupportFinding::Degenerate {
                support: vec![vec![0], vec![0]],
                reason: "degenerate".into(),
            },
            SupportFinding::Component {
                support: vec![vec![0, 1], vec![0, 1]],
                base,
                directions: vec![vec![1.0, -1.0, 1.0, -1.0]],
            },
            SupportFinding::Inconclusive {
                support: vec![vec![1], vec![1]],
                reason: "conditioning".into(),
            },
        ],
    };
    let mut solutions = SolutionSet::new(&spec);
    solutions.push_equilibrium_set("se", &set)?;
    let value = serialize(&solutions)?;
    let ids = value["solutions"]
        .as_array()
        .expect("solutions is an array")
        .iter()
        .map(|solution| solution["id"].as_str().expect("id is a string"))
        .collect::<Vec<_>>();
    assert_eq!(ids, ["se-0", "se-f0", "se-c1", "se-f1", "se-f2"]);
    assert_eq!(value["solutions"][1]["reason"], "degenerate");
    assert_eq!(value["solutions"][2]["kind"], "nash");
    assert_eq!(value["solutions"][3]["reason"], "component");
    assert_eq!(value["solutions"][4]["reason"], "conditioning");
    Ok(())
}

#[test]
fn conversions_reuse_flat_constructor_validation() -> TestResult {
    let (game, spec) = viz_games::battle_of_sexes()?;
    let mut equilibrium = support_enumeration(&game)?.equilibria.remove(0);
    equilibrium.payoffs.pop();

    let mut converted = SolutionSet::new(&spec);
    let converted_error = converted
        .push_equilibrium("bad", "support_enumeration", &equilibrium)
        .unwrap_err();

    let mut flat = SolutionSet::new(&spec);
    let flat_error = flat
        .push_nash(
            "bad",
            "support_enumeration",
            equilibrium
                .strategies
                .iter()
                .map(|strategy| strategy.probs().to_vec())
                .collect(),
            equilibrium.payoffs,
            equilibrium.regret,
        )
        .unwrap_err();
    assert_eq!(converted_error.to_string(), flat_error.to_string());
    Ok(())
}

#[test]
fn pure_nash_conversion_checks_empty_role_and_action_boundaries() -> TestResult {
    let (_, spec) = viz_games::prisoners_dilemma()?;
    let mut solutions = SolutionSet::new(&spec);
    solutions.push_pure_nash_all("empty", &[])?;
    assert_eq!(serialize(&solutions)?["solutions"], json!([]));

    let missing_role = solutions
        .push_pure_nash_all(
            "missing-role",
            &[PureNash {
                profile: vec![0],
                payoffs: vec![0.0, 0.0],
                regret: 0.0,
            }],
        )
        .unwrap_err();
    assert!(matches!(missing_role, ExportError::Mismatch(_)));

    let action_at_upper_bound = solutions
        .push_pure_nash_all(
            "upper-bound",
            &[PureNash {
                profile: vec![2, 0],
                payoffs: vec![0.0, 0.0],
                regret: 0.0,
            }],
        )
        .unwrap_err();
    assert!(matches!(action_at_upper_bound, ExportError::Mismatch(_)));
    assert_eq!(serialize(&solutions)?["solutions"], json!([]));
    Ok(())
}

#[test]
fn flat_constructors_reject_bad_probability_mismatched_game_and_duplicate_id() -> TestResult {
    let (_, spec) = viz_games::prisoners_dilemma()?;
    let mut solutions = SolutionSet::new(&spec);
    let bad_sum = solutions
        .push_population("bad-sum", "manual", vec![0.4, 0.4], None)
        .unwrap_err();
    assert!(matches!(bad_sum, ExportError::Probability { .. }));

    solutions.push_population("duplicate", "manual", vec![0.5, 0.5], None)?;
    let duplicate = solutions
        .push_population("duplicate", "manual", vec![0.5, 0.5], None)
        .unwrap_err();
    assert!(matches!(duplicate, ExportError::DuplicateId(_)));

    let (_, other_spec) = viz_games::battle_of_sexes()?;
    let directory = tempfile::tempdir()?;
    let mismatch = write_artifacts(directory.path(), &other_spec, Some(&solutions)).unwrap_err();
    assert!(matches!(mismatch, ExportError::Mismatch(_)));
    assert!(!directory.path().join("socsim_games").exists());
    Ok(())
}

#[test]
fn fixed_point_ess_length_and_br_status_are_checked() -> TestResult {
    let (game, spec) = viz_games::hawk_dove()?;
    let fixed = socsim_games::replicator_fixed_points(&game)?;
    let mut solutions = SolutionSet::new(&spec);
    let error = solutions
        .push_fixed_point_set("rep", &fixed, &[])
        .unwrap_err();
    assert!(matches!(error, ExportError::Mismatch(_)));

    let continuous_spec = viz_games::cournot()?.1;
    let mut continuous = SolutionSet::new(&continuous_spec);
    for (id, status, converged) in [
        ("converged", BrStatus::Converged, true),
        ("max-iter", BrStatus::MaxIter, false),
        ("failed", BrStatus::Failed, false),
    ] {
        continuous.push_br_iteration(
            id,
            &BrIteration {
                profile: vec![vec![22.5]; 3],
                residual: 0.0,
                status,
                damping: 0.5,
                iterations: 2,
            },
        )?;
        let value = serialize(&continuous)?;
        let last = value["solutions"]
            .as_array()
            .expect("solutions is an array")
            .last()
            .expect("a solution was inserted");
        assert_eq!(last["converged"], converged);
    }
    Ok(())
}
