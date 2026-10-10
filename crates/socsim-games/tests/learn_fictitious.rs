use rand::rngs::StdRng;
use rand::SeedableRng;
use socsim_games::learn::fictitious::{simultaneous_step, FictitiousBelief};
use socsim_games::learn::{LearnError, TieBreak};
use socsim_games::MatrixGame;

fn matching_pennies() -> MatrixGame {
    MatrixGame::bimatrix(
        &[vec![1.0, -1.0], vec![-1.0, 1.0]],
        &[vec![-1.0, 1.0], vec![1.0, -1.0]],
    )
    .unwrap()
}

#[test]
fn robinson_bound_holds_for_simultaneous_fictitious_play() {
    const T: usize = 100_000;
    let game = matching_pennies();
    let mut row = FictitiousBelief::with_defaults(&game, 0).unwrap();
    let mut column = FictitiousBelief::with_defaults(&game, 1).unwrap();
    let mut rng = StdRng::seed_from_u64(17);

    for _ in 0..T {
        simultaneous_step(&game, &mut row, &mut column, &mut rng).unwrap();
    }

    let x = row.empirical_counts();
    let y = column.empirical_counts();
    assert_eq!(x.iter().sum::<u64>(), T as u64);
    assert_eq!(y.iter().sum::<u64>(), T as u64);

    let x0 = i64::try_from(x[0]).unwrap();
    let x1 = i64::try_from(x[1]).unwrap();
    let y0 = i64::try_from(y[0]).unwrap();
    let y1 = i64::try_from(y[1]).unwrap();
    let ay = [y0 - y1, -y0 + y1];
    let xa = [x0 - x1, -x0 + x1];
    let gap = i128::from(*ay.iter().max().unwrap() - *xa.iter().min().unwrap());

    eprintln!("G(10^5)={gap}");
    assert!(gap * gap <= 128_i128 * i128::try_from(T).unwrap());
}

#[test]
fn beliefs_include_priors_but_empirical_counts_do_not() {
    let game = matching_pennies();
    let mut belief =
        FictitiousBelief::new(&game, 0, vec![2.5, 0.5], TieBreak::LowestIndex).unwrap();

    assert_eq!(belief.counts(), &[2.5, 0.5]);
    assert_eq!(belief.empirical_counts(), &[0, 0]);
    belief.observe(1).unwrap();
    assert_eq!(belief.counts(), &[2.5, 1.5]);
    assert_eq!(belief.empirical_counts(), &[0, 1]);
}

#[test]
fn construction_rejects_invalid_priors_and_unsupported_games() {
    let game = matching_pennies();
    assert!(matches!(
        FictitiousBelief::new(&game, 0, vec![0.0], TieBreak::LowestIndex),
        Err(LearnError::LengthMismatch { .. })
    ));
    assert!(matches!(
        FictitiousBelief::new(&game, 0, vec![0.0, -1.0], TieBreak::LowestIndex),
        Err(LearnError::InvalidInitialCount { .. })
    ));
    assert!(matches!(
        FictitiousBelief::new(&game, 0, vec![0.0, f64::INFINITY], TieBreak::LowestIndex),
        Err(LearnError::InvalidInitialCount { .. })
    ));

    let three_player =
        MatrixGame::new(vec![1, 1, 1], vec![vec![1.0], vec![1.0], vec![1.0]]).unwrap();
    assert!(matches!(
        FictitiousBelief::with_defaults(&three_player, 0),
        Err(LearnError::UnsupportedRoleCount {
            expected: 2,
            actual: 3
        })
    ));
}

#[test]
fn exact_ties_use_the_configured_policy() {
    let game = matching_pennies();
    let lowest = FictitiousBelief::with_defaults(&game, 0).unwrap();
    let mut rng = StdRng::seed_from_u64(3);
    assert_eq!(lowest.best_response(&game, 0, &mut rng).unwrap(), 0);

    let random = FictitiousBelief::new(&game, 0, vec![0.0; 2], TieBreak::Random).unwrap();
    let actions: Vec<_> = (0..32)
        .map(|_| random.best_response(&game, 0, &mut rng).unwrap())
        .collect();
    assert!(actions.contains(&0));
    assert!(actions.contains(&1));
}
