use std::sync::Arc;

use socsim_games::{
    assemble, build_world, AssignGroup, AssignKind, Assignment, Fixed, FixedPlayers,
    InitialActions, MatrixGame, NormalFormOutcome, PlayOptions, RuleBook, TitForTat,
};

const ROUNDS: usize = 100;
const TEMPTATION: f64 = 5.0;
const REWARD: f64 = 3.0;
const PUNISHMENT: f64 = 1.0;
const SUCKER: f64 = 0.0;

fn main() {
    let game = MatrixGame::bimatrix(
        &[vec![REWARD, SUCKER], vec![TEMPTATION, PUNISHMENT]],
        &[vec![REWARD, TEMPTATION], vec![SUCKER, PUNISHMENT]],
    )
    .expect("the prisoner's dilemma matrix is valid");

    let mut rules = RuleBook::new();
    rules
        .register("tit_for_tat", Arc::new(TitForTat::new(0)))
        .expect("rule kind is unique");
    rules
        .register("fixed_defect", Arc::new(Fixed::new(1)))
        .expect("rule kind is unique");

    let seed = 1;
    let world = build_world(
        game,
        2,
        (),
        InitialActions::Same(0),
        &rules,
        &Assignment::Ordered(vec![
            AssignGroup {
                kind: AssignKind::Rule("tit_for_tat".to_owned()),
                count: 1,
            },
            AssignGroup {
                kind: AssignKind::Rule("fixed_defect".to_owned()),
                count: 1,
            },
        ]),
        ROUNDS,
        ROUNDS as u64,
        seed,
    )
    .expect("the world configuration is valid");
    let mut simulation = assemble(
        world,
        FixedPlayers::pairs(&[(0, 1)]),
        rules,
        NormalFormOutcome,
        PlayOptions::new(),
        seed,
    )
    .expect("the simulation configuration is valid")
    .build();

    let mut scores = [0.0, 0.0];
    for _ in 0..ROUNDS {
        simulation.step().expect("the game step succeeds");
        for (score, payoff) in scores.iter_mut().zip(simulation.world().payoffs()) {
            *score += payoff;
        }
    }

    let expected_tft = SUCKER + (ROUNDS as f64 - 1.0) * PUNISHMENT;
    let expected_defect = TEMPTATION + (ROUNDS as f64 - 1.0) * PUNISHMENT;
    assert_eq!(scores[0], expected_tft);
    assert_eq!(scores[1], expected_defect);
    println!("TFT score: {}, Fixed(D) score: {}", scores[0], scores[1]);
}
