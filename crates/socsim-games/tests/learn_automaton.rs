use rand::rngs::StdRng;
use rand::SeedableRng;
use socsim_games::learn::automaton::{validate_two_action_game, AutomatonKind, AutomatonState};
use socsim_games::learn::LearnError;
use socsim_games::{MatrixGame, NormalFormGame};

fn pd() -> MatrixGame {
    MatrixGame::bimatrix(
        &[vec![3.0, 0.0], vec![5.0, 1.0]],
        &[vec![3.0, 5.0], vec![0.0, 1.0]],
    )
    .unwrap()
}

fn play_pair(left_kind: AutomatonKind, right_kind: AutomatonKind, rounds: usize) -> (f64, f64) {
    let game = pd();
    let mut left = AutomatonState::new(left_kind, 0).unwrap();
    let mut right = AutomatonState::new(right_kind, 0).unwrap();
    let mut rng = StdRng::seed_from_u64(41);
    let mut scores = (0.0, 0.0);
    for _ in 0..rounds {
        let left_action = left.next_action(&mut rng).unwrap();
        let right_action = right.next_action(&mut rng).unwrap();
        let profile = [left_action, right_action];
        scores.0 += game.payoff(0, &profile);
        scores.1 += game.payoff(1, &profile);
        left.observe(left_action, right_action).unwrap();
        right.observe(right_action, left_action).unwrap();
    }
    scores
}

#[test]
fn tft_against_alld_has_the_exact_repeated_pd_scores() {
    for rounds in [1_usize, 1_000] {
        let scores = play_pair(AutomatonKind::Tft, AutomatonKind::AllD, rounds);
        assert_eq!(scores.0, 0.0 + (rounds - 1) as f64);
        assert_eq!(scores.1, 5.0 + (rounds - 1) as f64);
    }
}

#[test]
fn tft_pair_cooperates_in_every_period() {
    let rounds = 1_000;
    assert_eq!(
        play_pair(AutomatonKind::Tft, AutomatonKind::Tft, rounds),
        (3.0 * rounds as f64, 3.0 * rounds as f64)
    );
}

#[test]
fn wsls_transition_table_matches_win_stay_lose_shift() {
    let mut rng = StdRng::seed_from_u64(1);
    for ((own, opponent), expected) in [((0, 0), 0), ((0, 1), 1), ((1, 0), 1), ((1, 1), 0)] {
        let mut state = AutomatonState::new(AutomatonKind::Wsls, 0).unwrap();
        state.observe(own, opponent).unwrap();
        assert_eq!(state.next_action(&mut rng).unwrap(), expected);
    }
}

#[test]
fn grim_allc_and_gtft_have_their_defining_transitions() {
    let mut rng = StdRng::seed_from_u64(9);
    let mut grim = AutomatonState::new(AutomatonKind::Grim, 0).unwrap();
    assert_eq!(grim.next_action(&mut rng).unwrap(), 0);
    grim.observe(0, 1).unwrap();
    assert_eq!(grim.next_action(&mut rng).unwrap(), 1);
    grim.observe(1, 0).unwrap();
    assert_eq!(grim.next_action(&mut rng).unwrap(), 1);

    let mut allc = AutomatonState::new(AutomatonKind::AllC, 0).unwrap();
    allc.observe(0, 1).unwrap();
    assert_eq!(allc.next_action(&mut rng).unwrap(), 0);

    let mut gtft_zero = AutomatonState::new(AutomatonKind::Gtft { generosity: 0.0 }, 0).unwrap();
    gtft_zero.observe(0, 1).unwrap();
    assert_eq!(gtft_zero.next_action(&mut rng).unwrap(), 1);

    let mut gtft_one = AutomatonState::new(AutomatonKind::Gtft { generosity: 1.0 }, 0).unwrap();
    gtft_one.observe(0, 1).unwrap();
    assert_eq!(gtft_one.next_action(&mut rng).unwrap(), 0);
}

#[test]
fn automata_validate_binary_games_cooperation_and_generosity() {
    validate_two_action_game(&pd()).unwrap();
    let non_binary = MatrixGame::bimatrix(
        &[vec![1.0, 1.0, 1.0], vec![1.0, 1.0, 1.0]],
        &[vec![1.0, 1.0, 1.0], vec![1.0, 1.0, 1.0]],
    )
    .unwrap();
    assert!(matches!(
        validate_two_action_game(&non_binary),
        Err(LearnError::InvalidActionCount { .. })
    ));
    assert!(matches!(
        AutomatonState::new(AutomatonKind::Tft, 2),
        Err(LearnError::InvalidCooperationAction { coop: 2 })
    ));
    for generosity in [-0.1, 1.1, f64::NAN] {
        assert!(matches!(
            AutomatonState::new(AutomatonKind::Gtft { generosity }, 0),
            Err(LearnError::InvalidParameter { .. })
        ));
    }
}
