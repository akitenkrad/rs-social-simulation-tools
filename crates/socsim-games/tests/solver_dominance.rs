use socsim_games::catalog::{BattleOfSexes, PrisonersDilemma};
use socsim_games::solve::{iterated_strict_dominance, Elimination};
use socsim_games::MatrixGame;

#[test]
fn prisoners_dilemma_leaves_only_defection() {
    let result =
        iterated_strict_dominance(&PrisonersDilemma::new(5.0, 3.0, 1.0, 0.0).unwrap()).unwrap();
    assert_eq!(result.surviving, vec![vec![1], vec![1]]);
    assert_eq!(
        result.eliminated,
        vec![
            Elimination {
                round: 1,
                role: 0,
                action: 0
            },
            Elimination {
                round: 1,
                role: 1,
                action: 0
            },
        ]
    );
}

#[test]
fn elimination_uses_simultaneous_one_based_rounds_until_fixed_point() {
    // In round 1, role 1's action 2 is dominated by action 1. Role 0's
    // action 0 is only dominated by action 1 after that removal.
    let game = MatrixGame::bimatrix(
        &[
            vec![1.0, 1.0, 3.0],
            vec![2.0, 2.0, 0.0],
            vec![0.0, 0.0, -1.0],
        ],
        &[
            vec![1.0, 2.0, 0.0],
            vec![1.0, 2.0, 0.0],
            vec![1.0, 2.0, 0.0],
        ],
    )
    .unwrap();
    let result = iterated_strict_dominance(&game).unwrap();
    assert_eq!(result.surviving, vec![vec![1], vec![1]]);
    assert_eq!(
        result.eliminated,
        vec![
            Elimination {
                round: 1,
                role: 0,
                action: 2
            },
            Elimination {
                round: 1,
                role: 1,
                action: 0
            },
            Elimination {
                round: 1,
                role: 1,
                action: 2
            },
            Elimination {
                round: 2,
                role: 0,
                action: 0
            },
        ]
    );
}

#[test]
fn no_strict_dominance_and_weak_dominance_remove_nothing() {
    let bos = iterated_strict_dominance(&BattleOfSexes::default()).unwrap();
    assert_eq!(bos.surviving, vec![vec![0, 1], vec![0, 1]]);
    assert!(bos.eliminated.is_empty());

    let weak_only = MatrixGame::bimatrix(
        &[vec![1.0, 0.0], vec![1.0, 1.0]],
        &[vec![0.0, 0.0], vec![0.0, 0.0]],
    )
    .unwrap();
    let result = iterated_strict_dominance(&weak_only).unwrap();
    assert_eq!(result.surviving, vec![vec![0, 1], vec![0, 1]]);
    assert!(result.eliminated.is_empty());
}
