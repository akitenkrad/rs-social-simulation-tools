use std::error::Error;

use serde::Serialize;
use socsim_games::catalog::{
    BattleOfSexes, HawkDove, MatchingPennies, PrisonersDilemma, RockPaperScissors,
};
use socsim_games::continuous::Cournot;
use socsim_games::{GameSpec, MatrixGame};

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
