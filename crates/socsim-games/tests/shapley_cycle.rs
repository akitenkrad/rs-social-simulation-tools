use rand::rngs::StdRng;
use rand::SeedableRng;
use socsim_games::learn::fictitious::{simultaneous_step, FictitiousBelief};
use socsim_games::MatrixGame;

#[test]
fn shapley_fictitious_play_cycle_has_the_original_asymptotics() {
    const T: usize = 100_000;
    const CYCLE: [(usize, usize); 6] = [(0, 0), (0, 2), (2, 2), (2, 1), (1, 1), (1, 0)];

    let game = MatrixGame::bimatrix(
        &[
            vec![1.0, 0.0, 0.0],
            vec![0.0, 1.0, 0.0],
            vec![0.0, 0.0, 1.0],
        ],
        &[
            vec![0.0, 0.0, 1.0],
            vec![1.0, 0.0, 0.0],
            vec![0.0, 1.0, 0.0],
        ],
    )
    .unwrap();
    let mut row = FictitiousBelief::with_defaults(&game, 0).unwrap();
    let mut column = FictitiousBelief::with_defaults(&game, 1).unwrap();
    let mut rng = StdRng::seed_from_u64(0);
    let mut completed_runs = Vec::<((usize, usize), u64)>::new();
    let mut current_pair = None;
    let mut current_len = 0_u64;
    let mut min_distance = f64::INFINITY;

    for t in 1..=T {
        let pair = simultaneous_step(&game, &mut row, &mut column, &mut rng).unwrap();
        if current_pair == Some(pair) {
            current_len += 1;
        } else {
            if let Some(previous) = current_pair.replace(pair) {
                completed_runs.push((previous, current_len));
            }
            current_len = 1;
        }

        if t >= T / 2 {
            let max_deviation_numerator = row
                .empirical_counts()
                .iter()
                .chain(column.empirical_counts())
                .map(|&count| (3_i128 * i128::from(count) - i128::try_from(t).unwrap()).abs())
                .max()
                .unwrap();
            assert!(1000 * max_deviation_numerator >= 258 * i128::try_from(t).unwrap());
            let distance = max_deviation_numerator as f64 / (3.0 * t as f64);
            min_distance = min_distance.min(distance);
        }
    }

    for (index, (pair, _)) in completed_runs.iter().enumerate() {
        assert_eq!(*pair, CYCLE[index % CYCLE.len()]);
    }
    assert!(completed_runs.windows(2).all(|runs| runs[0].1 < runs[1].1));

    let last = completed_runs.len() - 1;
    let run_ratio = completed_runs[last].1 as f64 / completed_runs[last - 6].1 as f64;
    let mut theta = 1.465_f64;
    for _ in 0..16 {
        theta -= (theta.powi(3) - theta.powi(2) - 1.0) / (3.0 * theta.powi(2) - 2.0 * theta);
    }
    let theta_six = theta.powi(6);
    eprintln!("last run ratio={run_ratio}, min d(t)={min_distance}");
    assert!((run_ratio - theta_six).abs() <= 0.01 * theta_six);
}
