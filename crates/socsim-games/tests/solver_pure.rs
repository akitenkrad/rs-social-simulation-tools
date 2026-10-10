use std::borrow::Cow;

use socsim_games::catalog::{
    BattleOfSexes, MatchingPennies, PrisonersDilemma, RockPaperScissors, StagHunt,
};
use socsim_games::mixed::{action_payoffs, expected_payoff, regret, regrets, MixedError};
use socsim_games::solve::{pure_nash, tolerance, Equilibrium, SolveError};
use socsim_games::{ActionError, FiniteGame, MatrixGame, MixedStrategy, NormalFormGame, StageGame};

fn profiles<G: FiniteGame>(game: &G) -> Vec<Vec<usize>> {
    pure_nash(game)
        .unwrap()
        .into_iter()
        .map(|equilibrium| {
            assert!(equilibrium.regret <= tolerance(game).unwrap());
            equilibrium.profile
        })
        .collect()
}

#[test]
fn catalog_games_have_exactly_the_expected_pure_equilibria() {
    assert_eq!(
        profiles(&PrisonersDilemma::new(5.0, 3.0, 1.0, 0.0).unwrap()),
        vec![vec![1, 1]]
    );
    assert_eq!(
        profiles(&BattleOfSexes::default()),
        vec![vec![0, 0], vec![1, 1]]
    );
    assert!(profiles(&MatchingPennies::default()).is_empty());
    assert!(profiles(&RockPaperScissors::default()).is_empty());
    assert_eq!(profiles(&StagHunt::default()), vec![vec![0, 0], vec![1, 1]]);
}

#[test]
fn three_player_game_is_fully_enumerated_in_lexicographic_order() {
    let game = MatrixGame::new(
        vec![2, 2, 2],
        vec![
            vec![1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0],
            vec![1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0],
            vec![1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0],
        ],
    )
    .unwrap();
    assert_eq!(profiles(&game), vec![vec![0, 0, 0], vec![1, 1, 1]]);
}

#[test]
fn mixed_strategy_validates_and_computes_n_player_quantities() {
    assert!(MixedStrategy::new(vec![]).is_err());
    assert!(MixedStrategy::new(vec![f64::NAN, 1.0]).is_err());
    assert!(MixedStrategy::new(vec![-0.1, 1.1]).is_err());
    assert!(MixedStrategy::new(vec![0.4, 0.4]).is_err());
    let unnormalized = MixedStrategy::new(vec![0.5, 0.500_000_000_5]).unwrap();
    assert_eq!(unnormalized.probs(), &[0.5, 0.500_000_000_5]);
    assert!(MixedStrategy::uniform(0).is_err());
    assert!(MixedStrategy::pure(2, 2).is_err());

    let game = MatchingPennies::default();
    let strategies = [
        MixedStrategy::uniform(2).unwrap(),
        MixedStrategy::uniform(2).unwrap(),
    ];
    assert_eq!(expected_payoff(&game, 0, &strategies).unwrap(), 0.0);
    assert_eq!(
        action_payoffs(&game, 0, &strategies).unwrap(),
        vec![0.0, 0.0]
    );
    assert_eq!(regrets(&game, &strategies).unwrap(), vec![0.0, 0.0]);
    assert_eq!(regret(&game, &strategies).unwrap(), 0.0);

    let three_player = MatrixGame::new(
        vec![2, 2, 2],
        vec![(0..8).map(f64::from).collect(), vec![0.0; 8], vec![0.0; 8]],
    )
    .unwrap();
    let three_uniform = [
        MixedStrategy::uniform(2).unwrap(),
        MixedStrategy::uniform(2).unwrap(),
        MixedStrategy::uniform(2).unwrap(),
    ];
    assert_eq!(
        action_payoffs(&three_player, 0, &three_uniform).unwrap(),
        vec![1.5, 5.5]
    );
    assert_eq!(
        expected_payoff(&three_player, 0, &three_uniform).unwrap(),
        3.5
    );
    assert_eq!(
        regrets(&three_player, &three_uniform).unwrap(),
        vec![2.0, 0.0, 0.0]
    );

    assert!(matches!(
        regret(&game, &strategies[..1]),
        Err(MixedError::RoleCount {
            expected: 2,
            actual: 1
        })
    ));
    assert!(matches!(
        regret(
            &game,
            &[
                MixedStrategy::pure(2, 0).unwrap(),
                MixedStrategy::pure(1, 0).unwrap()
            ]
        ),
        Err(MixedError::Dimension {
            role: 1,
            expected: 2,
            actual: 1
        })
    ));
}

#[test]
fn equilibrium_reports_purity_from_its_strategy_probabilities() {
    let pure = Equilibrium {
        strategies: vec![
            MixedStrategy::pure(2, 0).unwrap(),
            MixedStrategy::pure(2, 1).unwrap(),
        ],
        payoffs: vec![0.0, 0.0],
        regret: 0.0,
    };
    assert!(pure.is_pure());

    let mixed = Equilibrium {
        strategies: vec![MixedStrategy::uniform(2).unwrap()],
        payoffs: vec![0.0],
        regret: 0.0,
    };
    assert!(!mixed.is_pure());
    assert!(!Equilibrium {
        strategies: vec![],
        payoffs: vec![],
        regret: 0.0,
    }
    .is_pure());
}

#[derive(Clone, Debug)]
struct ShapeGame(Vec<usize>);

impl StageGame for ShapeGame {
    type Action = usize;

    fn n_roles(&self) -> usize {
        self.0.len()
    }

    fn check_action(&self, role: usize, action: &usize) -> Result<(), ActionError> {
        let Some(&count) = self.0.get(role) else {
            return Err(ActionError::InvalidRole { role });
        };
        if *action >= count {
            return Err(ActionError::InvalidAction {
                role,
                action: *action,
            });
        }
        Ok(())
    }
}

impl NormalFormGame for ShapeGame {
    fn payoff(&self, _role: usize, _profile: &[usize]) -> f64 {
        0.0
    }
}

impl FiniteGame for ShapeGame {
    fn n_actions(&self, role: usize) -> usize {
        self.0[role]
    }
}

#[derive(Clone, Debug)]
struct HugeGame;

impl StageGame for HugeGame {
    type Action = usize;

    fn n_roles(&self) -> usize {
        2
    }

    fn check_action(&self, role: usize, action: &usize) -> Result<(), ActionError> {
        if role >= 2 {
            return Err(ActionError::InvalidRole { role });
        }
        if *action >= self.n_actions(role) {
            return Err(ActionError::InvalidAction {
                role,
                action: *action,
            });
        }
        Ok(())
    }
}

impl NormalFormGame for HugeGame {
    fn payoff(&self, _role: usize, _profile: &[usize]) -> f64 {
        0.0
    }
}

impl FiniteGame for HugeGame {
    fn n_actions(&self, _role: usize) -> usize {
        1_001
    }
    fn action_label(&self, _role: usize, action: usize) -> Cow<'_, str> {
        Cow::Owned(action.to_string())
    }
}

#[test]
fn solver_rejects_oversized_and_non_finite_games() {
    assert_eq!(
        pure_nash(&HugeGame),
        Err(SolveError::TooLarge {
            profiles: 1_002_001
        })
    );

    #[derive(Clone, Debug)]
    struct NonFinite;
    impl StageGame for NonFinite {
        type Action = usize;
        fn n_roles(&self) -> usize {
            1
        }
        fn check_action(&self, _role: usize, _action: &usize) -> Result<(), ActionError> {
            Ok(())
        }
    }
    impl NormalFormGame for NonFinite {
        fn payoff(&self, _role: usize, _profile: &[usize]) -> f64 {
            f64::NAN
        }
    }
    impl FiniteGame for NonFinite {
        fn n_actions(&self, _role: usize) -> usize {
            1
        }
    }
    assert_eq!(tolerance(&NonFinite), Err(SolveError::NonFinitePayoff));
    assert_eq!(
        pure_nash(&ShapeGame(vec![])),
        Err(SolveError::UnsupportedRoles { n_roles: 0 })
    );
    assert_eq!(
        pure_nash(&ShapeGame(vec![2, 0])),
        Err(SolveError::EmptyActions { role: 1 })
    );
    assert_eq!(
        pure_nash(&ShapeGame(vec![usize::MAX; 3])),
        Err(SolveError::TooLarge {
            profiles: u128::MAX
        })
    );
}
