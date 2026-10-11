use std::borrow::Cow;

use socsim_games::catalog::{
    BattleOfSexes, CatalogError, ElFarol, HawkDove, MatchingPennies, MinorityGame,
    PrisonersDilemma, PublicGoodsFinite, RockPaperScissors, StagHunt,
};
use socsim_games::solve::{pure_nash_counts, CountProfile, SolveError};
use socsim_games::{
    ActionError, AggregateGame, AnonymousGame, CommonActionSpace, FiniteGame, MatrixGame,
    NormalFormGame, StageGame,
};

fn assert_common_action_space<G: CommonActionSpace>() {}

fn assert_close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() <= 1e-12,
        "expected {expected}, got {actual}"
    );
}

#[test]
fn common_action_space_is_public_for_builtin_and_matrix_games() {
    assert_common_action_space::<MatrixGame>();
    assert_common_action_space::<PrisonersDilemma>();
    assert_common_action_space::<BattleOfSexes>();
    assert_common_action_space::<MatchingPennies>();
    assert_common_action_space::<StagHunt>();
    assert_common_action_space::<RockPaperScissors>();
    assert_common_action_space::<HawkDove>();
    assert_common_action_space::<ElFarol>();
    assert_common_action_space::<MinorityGame>();
    assert_common_action_space::<PublicGoodsFinite>();
}

#[test]
fn hawk_dove_matches_the_catalog_matrix_and_ess_share() {
    let game = HawkDove::new(2.0, 4.0).unwrap();
    assert_eq!(game.n_roles(), 2);
    assert_eq!(game.action_label(0, 0), "H");
    assert_eq!(game.action_label(1, 1), "D");
    assert_eq!(game.payoff_by_counts(0, &[1, 0]), -1.0);
    assert_eq!(game.payoff_by_counts(0, &[0, 1]), 2.0);
    assert_eq!(game.payoff_by_counts(1, &[1, 0]), 0.0);
    assert_eq!(game.payoff_by_counts(1, &[0, 1]), 1.0);
    assert_eq!(game.hawk_share_ess(), 0.5);
    assert_eq!(HawkDove::new(2.0, 2.0).unwrap().hawk_share_ess(), 1.0);
    assert_eq!(HawkDove::new(3.0, 2.0).unwrap().hawk_share_ess(), 1.0);

    let matrix = game.to_matrix();
    for row in 0..2 {
        for column in 0..2 {
            let profile = [row, column];
            assert_eq!(matrix.payoff(0, &profile), game.payoff(0, &profile));
            assert_eq!(matrix.payoff(1, &profile), game.payoff(1, &profile));
        }
    }
}

#[test]
fn aggregate_games_use_action_zero_attendance_and_d77_payoffs() {
    let el_farol = ElFarol::new(100, 60).unwrap();
    let mut at_capacity = vec![0; 60];
    at_capacity.extend(vec![1; 40]);
    assert_eq!(el_farol.payoff(0, &at_capacity), 1.0);
    assert_eq!(el_farol.payoff(60, &at_capacity), 0.0);
    assert_eq!(el_farol.payoff_by_counts(0, &[59, 40]), 1.0);
    assert_eq!(el_farol.payoff_by_counts(0, &[60, 39]), -1.0);
    assert_eq!(el_farol.action_label(0, 0), "attend");
    assert_eq!(el_farol.action_label(99, 1), "stay");
    assert_eq!(el_farol.aggregate_spec()[0].name(), "attendance");
    let mut aggregate = [0.0];
    el_farol.aggregate_into(&at_capacity, &mut aggregate);
    assert_eq!(aggregate, [60.0]);

    let minority = MinorityGame::new(5).unwrap();
    let split = [0, 0, 1, 1, 1];
    assert_eq!(minority.payoff(0, &split), 1.0);
    assert_eq!(minority.payoff(2, &split), 0.0);
    assert_eq!(minority.payoff_by_counts(0, &[1, 3]), 1.0);
    assert_eq!(minority.payoff_by_counts(1, &[2, 2]), 0.0);
    assert_eq!(minority.aggregate_spec()[0].name(), "attendance");
    minority.aggregate_into(&split, &mut aggregate);
    assert_eq!(aggregate, [2.0]);
}

#[test]
fn public_goods_uses_finite_group_payoffs() {
    let game = PublicGoodsFinite::new(5, 3.0, 1.0).unwrap();
    let profile = [0, 0, 1, 1, 1];
    assert_close(game.payoff(0, &profile), 0.2);
    assert_close(game.payoff(2, &profile), 1.2);
    assert_close(game.payoff_by_counts(0, &[1, 3]), 0.2);
    assert_close(game.payoff_by_counts(1, &[2, 2]), 1.2);
    assert_close(
        game.payoff_by_counts(0, &[1, 3]) - game.payoff_by_counts(1, &[1, 3]),
        -0.4,
    );
    assert_eq!(game.action_label(0, 0), "C");
    assert_eq!(game.action_label(4, 1), "D");
}

#[test]
fn constructors_and_deserialization_enforce_catalog_conditions() {
    for value in [0.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(HawkDove::new(value, 1.0).is_err());
        assert!(HawkDove::new(1.0, value).is_err());
        assert!(PublicGoodsFinite::new(5, value, 1.0).is_err());
        assert!(PublicGoodsFinite::new(5, 1.0, value).is_err());
    }
    assert!(matches!(
        ElFarol::new(1, 0),
        Err(CatalogError::Condition(_))
    ));
    assert!(matches!(
        ElFarol::new(10, 0),
        Err(CatalogError::Condition(_))
    ));
    assert!(matches!(
        ElFarol::new(10, 10),
        Err(CatalogError::Condition(_))
    ));
    assert!(matches!(
        MinorityGame::new(1),
        Err(CatalogError::Condition(_))
    ));
    assert!(matches!(
        MinorityGame::new(4),
        Err(CatalogError::Condition(_))
    ));
    assert!(matches!(
        PublicGoodsFinite::new(1, 3.0, 1.0),
        Err(CatalogError::Condition(_))
    ));

    let hawk_dove = HawkDove::new(2.0, 4.0).unwrap();
    let el_farol = ElFarol::new(100, 60).unwrap();
    let minority = MinorityGame::new(5).unwrap();
    let public_goods = PublicGoodsFinite::new(5, 3.0, 1.0).unwrap();
    assert_eq!(
        serde_json::from_value::<HawkDove>(serde_json::to_value(&hawk_dove).unwrap()).unwrap(),
        hawk_dove
    );
    assert_eq!(
        serde_json::from_value::<ElFarol>(serde_json::to_value(&el_farol).unwrap()).unwrap(),
        el_farol
    );
    assert_eq!(
        serde_json::from_value::<MinorityGame>(serde_json::to_value(&minority).unwrap()).unwrap(),
        minority
    );
    assert_eq!(
        serde_json::from_value::<PublicGoodsFinite>(serde_json::to_value(&public_goods).unwrap())
            .unwrap(),
        public_goods
    );
    assert!(serde_json::from_str::<HawkDove>(r#"{"v":0.0,"c":4.0}"#).is_err());
    assert!(serde_json::from_str::<ElFarol>(r#"{"n":100,"capacity":100}"#).is_err());
    assert!(serde_json::from_str::<MinorityGame>(r#"{"n":4}"#).is_err());
    assert!(serde_json::from_str::<PublicGoodsFinite>(r#"{"k":5,"r":0.0,"cost":1.0}"#).is_err());
}

#[test]
fn pure_nash_counts_match_the_complete_d77_sets_in_lexicographic_order() {
    assert_eq!(
        pure_nash_counts(&HawkDove::new(2.0, 4.0).unwrap()).unwrap(),
        vec![CountProfile { counts: vec![1, 1] }]
    );
    assert_eq!(
        pure_nash_counts(&ElFarol::new(100, 60).unwrap()).unwrap(),
        vec![CountProfile {
            counts: vec![60, 40]
        }]
    );
    assert_eq!(
        pure_nash_counts(&MinorityGame::new(5).unwrap()).unwrap(),
        vec![
            CountProfile { counts: vec![2, 3] },
            CountProfile { counts: vec![3, 2] }
        ]
    );
    assert_eq!(
        pure_nash_counts(&PublicGoodsFinite::new(5, 3.0, 1.0).unwrap()).unwrap(),
        vec![CountProfile { counts: vec![0, 5] }]
    );
}

#[derive(Clone, Debug)]
struct LargeAnonymous;

impl StageGame for LargeAnonymous {
    type Action = usize;

    fn n_roles(&self) -> usize {
        2_000
    }

    fn check_action(&self, role: usize, action: &usize) -> Result<(), ActionError> {
        if role >= self.n_roles() {
            return Err(ActionError::InvalidRole { role });
        }
        if *action >= 3 {
            return Err(ActionError::InvalidAction {
                role,
                action: *action,
            });
        }
        Ok(())
    }
}

impl NormalFormGame for LargeAnonymous {
    fn payoff(&self, _role: usize, _profile: &[usize]) -> f64 {
        0.0
    }
}

impl FiniteGame for LargeAnonymous {
    fn n_actions(&self, _role: usize) -> usize {
        3
    }

    fn action_label(&self, _role: usize, action: usize) -> Cow<'_, str> {
        Cow::Owned(format!("a{action}"))
    }
}

impl AnonymousGame for LargeAnonymous {
    fn payoff_by_counts(&self, _a: usize, _counts_others: &[usize]) -> f64 {
        0.0
    }
}

#[test]
fn anonymous_solver_rejects_more_than_one_million_count_profiles() {
    assert_eq!(
        pure_nash_counts(&LargeAnonymous),
        Err(SolveError::TooLarge {
            profiles: 2_003_001
        })
    );
}
