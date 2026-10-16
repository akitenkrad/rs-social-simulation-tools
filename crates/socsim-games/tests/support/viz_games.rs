#![allow(dead_code)]

use std::error::Error;

use serde::Serialize;
use socsim_games::catalog::{
    BattleOfSexes, HawkDove, MatchingPennies, PrisonersDilemma, RockPaperScissors,
};
use socsim_games::continuous::Cournot;
use socsim_games::solve::{
    lemke_howson, logit_qre, pure_nash, support_enumeration, zero_sum_minimax, LemkeHowsonOptions,
    QreOptions,
};
use socsim_games::{
    best_response_iteration, ess_verdict, replicator_fixed_points, BrIterationOptions, FiniteGame,
    GameSpec, MatrixGame, SolutionSet,
};

type TestResult<T> = Result<T, Box<dyn Error>>;

fn with_serialized_params<T: Serialize>(spec: GameSpec, game: &T) -> TestResult<GameSpec> {
    Ok(spec.with_params(serde_json::to_value(game)?))
}

pub fn prisoners_dilemma() -> TestResult<(PrisonersDilemma, GameSpec)> {
    let game = PrisonersDilemma::new(5.0, 3.0, 1.0, 0.0)?;
    let spec = GameSpec::from_finite(&game, "prisoners_dilemma", None)?;
    let spec = with_serialized_params(spec, &game)?;
    Ok((game, spec))
}

pub fn battle_of_sexes() -> TestResult<(BattleOfSexes, GameSpec)> {
    let game = BattleOfSexes::default();
    let spec = GameSpec::from_finite(&game, "battle_of_sexes", None)?;
    let spec = with_serialized_params(spec, &game)?;
    Ok((game, spec))
}

pub fn matching_pennies() -> TestResult<(MatchingPennies, GameSpec)> {
    let game = MatchingPennies::default();
    let spec = GameSpec::from_finite(&game, "matching_pennies", None)?;
    let spec = with_serialized_params(spec, &game)?;
    Ok((game, spec))
}

pub fn rock_paper_scissors() -> TestResult<(RockPaperScissors, GameSpec)> {
    let game = RockPaperScissors::default();
    let spec = GameSpec::from_finite(&game, "rock_paper_scissors", None)?;
    let spec = with_serialized_params(spec, &game)?;
    Ok((game, spec))
}

pub fn hawk_dove() -> TestResult<(HawkDove, GameSpec)> {
    let game = HawkDove::new(2.0, 4.0)?;
    let spec = GameSpec::from_finite(&game, "hawk_dove", None)?;
    let spec = with_serialized_params(spec, &game)?;
    Ok((game, spec))
}

pub fn asym_2x3() -> TestResult<(MatrixGame, GameSpec)> {
    let game = MatrixGame::bimatrix(
        &[vec![2.0, 0.0, 3.0], vec![1.0, 1.0, 1.0]],
        &[vec![0.0, 1.0, 2.0], vec![1.0, 0.0, 0.0]],
    )?
    .with_labels(vec![
        vec!["U".into(), "D".into()],
        vec!["L".into(), "C".into(), "R".into()],
    ])?;
    let spec = GameSpec::from_finite(&game, "asym_2x3", None)?;
    Ok((game, spec))
}

pub fn matrix_3p() -> TestResult<(MatrixGame, GameSpec)> {
    let payoffs = (0..3)
        .map(|role| (0..8).map(|k| (100 * role + k) as f64).collect())
        .collect();
    let game = MatrixGame::new(vec![2, 2, 2], payoffs)?;
    let spec = GameSpec::from_finite(&game, "matrix_3p", None)?;
    Ok((game, spec))
}

pub fn cournot() -> TestResult<(Cournot, GameSpec)> {
    let game = Cournot::new(3, 100.0, 1.0, 10.0, 100.0)?;
    let spec = GameSpec::from_continuous(&game, "cournot", None, None)?;
    let spec = with_serialized_params(spec, &game)?;
    Ok((game, spec))
}

pub fn replicator_component() -> TestResult<(MatrixGame, GameSpec)> {
    let a = [vec![2.0, 2.0, 0.0], vec![2.0, 2.0, 0.0], vec![1.0; 3]];
    let b = [
        vec![2.0, 2.0, 1.0],
        vec![2.0, 2.0, 1.0],
        vec![0.0, 0.0, 1.0],
    ];
    let game = MatrixGame::bimatrix(&a, &b)?;
    let spec = GameSpec::from_finite(&game, "replicator_component", None)?;
    Ok((game, spec))
}

pub fn weak_prisoners_dilemma() -> TestResult<(PrisonersDilemma, GameSpec)> {
    let game = PrisonersDilemma::weak(1.85)?;
    let spec = GameSpec::from_finite(&game, "weak_prisoners_dilemma", None)?;
    let spec = with_serialized_params(spec, &game)?;
    Ok((game, spec))
}

pub fn game_specs() -> TestResult<Vec<(&'static str, GameSpec)>> {
    Ok(vec![
        ("prisoners_dilemma", prisoners_dilemma()?.1),
        ("battle_of_sexes", battle_of_sexes()?.1),
        ("matching_pennies", matching_pennies()?.1),
        ("rock_paper_scissors", rock_paper_scissors()?.1),
        ("hawk_dove", hawk_dove()?.1),
        ("asym_2x3", asym_2x3()?.1),
        ("matrix_3p", matrix_3p()?.1),
        ("cournot", cournot()?.1),
        ("replicator_component", replicator_component()?.1),
        ("weak_prisoners_dilemma", weak_prisoners_dilemma()?.1),
    ])
}

fn push_replicator_solutions<G: FiniteGame>(
    solutions: &mut SolutionSet,
    game: &G,
) -> TestResult<()> {
    let fixed = replicator_fixed_points(game)?;
    let ess = fixed
        .isolated
        .iter()
        .map(|point| ess_verdict(game, &point.point).map(Some))
        .collect::<Result<Vec<_>, _>>()?;
    solutions.push_fixed_point_set("rep", &fixed, &ess)?;
    Ok(())
}

pub fn prisoners_dilemma_artifacts() -> TestResult<(GameSpec, SolutionSet)> {
    let (game, spec) = prisoners_dilemma()?;
    let mut solutions = SolutionSet::new(&spec);
    solutions.push_pure_nash_all("pure", &pure_nash(&game)?)?;
    solutions.push_equilibrium_set("se", &support_enumeration(&game)?)?;
    push_replicator_solutions(&mut solutions, &game)?;
    Ok((spec, solutions))
}

pub fn battle_of_sexes_artifacts() -> TestResult<(GameSpec, SolutionSet)> {
    let (game, spec) = battle_of_sexes()?;
    let mut solutions = SolutionSet::new(&spec);
    solutions.push_pure_nash_all("pure", &pure_nash(&game)?)?;
    solutions.push_equilibrium_set("se", &support_enumeration(&game)?)?;
    solutions.push_lemke_howson("lh", &lemke_howson(&game, &LemkeHowsonOptions::default())?)?;
    Ok((spec, solutions))
}

pub fn matching_pennies_artifacts() -> TestResult<(GameSpec, SolutionSet)> {
    let (game, spec) = matching_pennies()?;
    let mut solutions = SolutionSet::new(&spec);
    solutions.push_equilibrium_set("se", &support_enumeration(&game)?)?;
    solutions.push_minimax("minimax", &zero_sum_minimax(&game)?)?;
    solutions.push_qre_solution("qre", &logit_qre(&game, &QreOptions::new(1.0))?)?;
    Ok((spec, solutions))
}

pub fn rock_paper_scissors_artifacts() -> TestResult<(GameSpec, SolutionSet)> {
    let (game, spec) = rock_paper_scissors()?;
    let mut solutions = SolutionSet::new(&spec);
    // RPS support enumeration uses the OS libm's hypot, so this fixture would not
    // be byte-identical across platforms.
    solutions.push_minimax("minimax", &zero_sum_minimax(&game)?)?;
    push_replicator_solutions(&mut solutions, &game)?;
    Ok((spec, solutions))
}

pub fn hawk_dove_artifacts() -> TestResult<(GameSpec, SolutionSet)> {
    let (game, spec) = hawk_dove()?;
    let mut solutions = SolutionSet::new(&spec);
    solutions.push_pure_nash_all("pure", &pure_nash(&game)?)?;
    solutions.push_equilibrium_set("se", &support_enumeration(&game)?)?;
    push_replicator_solutions(&mut solutions, &game)?;
    Ok((spec, solutions))
}

pub fn asym_2x3_artifacts() -> TestResult<(GameSpec, SolutionSet)> {
    let (game, spec) = asym_2x3()?;
    let mut solutions = SolutionSet::new(&spec);
    solutions.push_pure_nash_all("pure", &pure_nash(&game)?)?;
    solutions.push_equilibrium_set("se", &support_enumeration(&game)?)?;
    solutions.push_lemke_howson("lh", &lemke_howson(&game, &LemkeHowsonOptions::default())?)?;
    Ok((spec, solutions))
}

pub fn cournot_artifacts() -> TestResult<(GameSpec, SolutionSet)> {
    let (game, spec) = cournot()?;
    let iteration = best_response_iteration(
        &game,
        &[vec![50.0], vec![50.0], vec![50.0]],
        &BrIterationOptions::default(),
    )?;
    let mut solutions = SolutionSet::new(&spec);
    solutions.push_br_iteration("br", &iteration)?;
    Ok((spec, solutions))
}

pub fn replicator_component_artifacts() -> TestResult<(GameSpec, SolutionSet)> {
    let (game, spec) = replicator_component()?;
    let mut solutions = SolutionSet::new(&spec);
    push_replicator_solutions(&mut solutions, &game)?;
    Ok((spec, solutions))
}

pub fn solution_artifacts() -> TestResult<Vec<(&'static str, GameSpec, SolutionSet)>> {
    let (pd_spec, pd_solutions) = prisoners_dilemma_artifacts()?;
    let (bos_spec, bos_solutions) = battle_of_sexes_artifacts()?;
    let (mp_spec, mp_solutions) = matching_pennies_artifacts()?;
    let (rps_spec, rps_solutions) = rock_paper_scissors_artifacts()?;
    let (hd_spec, hd_solutions) = hawk_dove_artifacts()?;
    let (asym_spec, asym_solutions) = asym_2x3_artifacts()?;
    let (cournot_spec, cournot_solutions) = cournot_artifacts()?;
    let (component_spec, component_solutions) = replicator_component_artifacts()?;
    Ok(vec![
        ("prisoners_dilemma", pd_spec, pd_solutions),
        ("battle_of_sexes", bos_spec, bos_solutions),
        ("matching_pennies", mp_spec, mp_solutions),
        ("rock_paper_scissors", rps_spec, rps_solutions),
        ("hawk_dove", hd_spec, hd_solutions),
        ("asym_2x3", asym_spec, asym_solutions),
        ("cournot", cournot_spec, cournot_solutions),
        ("replicator_component", component_spec, component_solutions),
    ])
}
