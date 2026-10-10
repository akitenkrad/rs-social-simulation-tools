use socsim_games::catalog::{
    BattleOfSexes, CatalogError, MatchingPennies, PrisonersDilemma, RockPaperScissors, StagHunt,
};
use socsim_games::{FiniteGame, NormalFormGame};

fn assert_same_game<G: FiniteGame>(game: &G, matrix: &impl FiniteGame) {
    assert_eq!(matrix.n_roles(), game.n_roles());
    assert_eq!(matrix.is_symmetric(), game.is_symmetric());
    for role in 0..game.n_roles() {
        assert_eq!(matrix.n_actions(role), game.n_actions(role));
        for action in 0..game.n_actions(role) {
            assert_eq!(
                matrix.action_label(role, action),
                game.action_label(role, action)
            );
        }
    }

    let mut profile = vec![0; game.n_roles()];
    loop {
        for role in 0..game.n_roles() {
            assert_eq!(matrix.payoff(role, &profile), game.payoff(role, &profile));
        }
        let mut advanced = false;
        for role in (0..game.n_roles()).rev() {
            profile[role] += 1;
            if profile[role] < game.n_actions(role) {
                advanced = true;
                break;
            }
            profile[role] = 0;
        }
        if !advanced {
            break;
        }
    }
}

#[test]
fn prisoners_dilemma_checks_boundaries_and_non_finite_values() {
    let game = PrisonersDilemma::new(5.0, 3.0, 1.0, 1.0).unwrap();
    assert_eq!(game.payoff(0, &[0, 0]), 3.0);
    assert_eq!(game.payoff(0, &[0, 1]), 1.0);
    assert_eq!(game.payoff(0, &[1, 0]), 5.0);
    assert_eq!(game.payoff(0, &[1, 1]), 1.0);

    for invalid in [
        PrisonersDilemma::new(3.0, 3.0, 1.0, 0.0),
        PrisonersDilemma::new(4.0, 3.0, 3.0, 0.0),
        PrisonersDilemma::new(4.0, 3.0, 1.0, 2.0),
    ] {
        assert!(matches!(invalid, Err(CatalogError::Condition(_))));
    }
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(
            PrisonersDilemma::new(value, 3.0, 1.0, 0.0),
            Err(CatalogError::NonFinite)
        );
    }
    assert!(PrisonersDilemma::weak(1.0).is_err());
    assert!(PrisonersDilemma::weak(f64::NAN).is_err());
    assert_eq!(PrisonersDilemma::weak(1.5).unwrap().payoff(0, &[0, 0]), 1.0);
}

#[test]
fn textbook_defaults_have_expected_payoffs() {
    let bos = BattleOfSexes::default();
    assert_eq!(bos.payoff(0, &[0, 0]), 2.0);
    assert_eq!(bos.payoff(1, &[0, 0]), 1.0);
    assert_eq!(bos.payoff(0, &[1, 1]), 1.0);
    assert_eq!(bos.payoff(1, &[1, 1]), 2.0);

    let matching = MatchingPennies::default();
    assert_eq!(matching.payoff(0, &[0, 0]), -1.0);
    assert_eq!(matching.payoff(1, &[0, 0]), 1.0);
    assert_eq!(matching.payoff(0, &[0, 1]), 1.0);

    let stag = StagHunt::default();
    assert_eq!(stag.payoff(0, &[0, 0]), 9.0);
    assert_eq!(stag.payoff(1, &[0, 1]), 8.0);
    assert_eq!(stag.payoff(0, &[1, 1]), 7.0);

    let rps = RockPaperScissors::default();
    let expected = [[0.0, -1.0, 1.0], [1.0, 0.0, -1.0], [-1.0, 1.0, 0.0]];
    for (row, row_payoffs) in expected.iter().enumerate() {
        for (column, &payoff) in row_payoffs.iter().enumerate() {
            assert_eq!(rps.payoff(0, &[row, column]), payoff);
            assert_eq!(rps.payoff(1, &[row, column]), expected[column][row]);
        }
    }
}

#[test]
fn constructors_reject_games_outside_the_named_catalog_shape() {
    let bos_a = [[2.0, -1.0], [-1.0, 1.0]];
    let bos_b = [[1.0, -1.0], [-1.0, 2.0]];
    assert!(BattleOfSexes::new(bos_a, bos_b).is_ok());
    assert!(BattleOfSexes::new([[1.0, -1.0], [-1.0, 2.0]], bos_b).is_err());

    let matching_a = [[-1.0, 1.0], [1.0, -1.0]];
    let matching_b = [[1.0, -1.0], [-1.0, 1.0]];
    assert!(MatchingPennies::new(matching_a, matching_b).is_ok());
    assert!(MatchingPennies::new(matching_a, matching_a).is_err());
    assert!(MatchingPennies::new([[1.0, 0.0], [0.0, -1.0]], [[-1.0, 0.0], [0.0, 1.0]]).is_err());

    let stag_a = [[9.0, 0.0], [8.0, 7.0]];
    let stag_b = [[9.0, 8.0], [0.0, 7.0]];
    assert!(StagHunt::new(stag_a, stag_b).is_ok());
    assert!(StagHunt::new([[7.0, 0.0], [8.0, 9.0]], stag_b).is_err());

    assert!(RockPaperScissors::new(0.0, 1.0).is_err());
    assert!(RockPaperScissors::new(1.0, 0.0).is_err());
    assert!(RockPaperScissors::new(f64::INFINITY, 1.0).is_err());

    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(
            BattleOfSexes::new([[2.0, value], [-1.0, 1.0]], bos_b),
            Err(CatalogError::NonFinite)
        );
        assert_eq!(
            MatchingPennies::new(matching_a, [[1.0, -1.0], [value, 1.0]]),
            Err(CatalogError::NonFinite)
        );
        assert_eq!(
            StagHunt::new(stag_a, [[9.0, 8.0], [0.0, value]]),
            Err(CatalogError::NonFinite)
        );
    }
}

#[test]
fn conversions_preserve_all_payoffs_symmetry_and_ascii_labels() {
    let pd = PrisonersDilemma::new(5.0, 3.0, 1.0, 0.0).unwrap();
    assert_same_game(&pd, &pd.to_matrix());
    assert_eq!(pd.action_label(0, 0), "C");
    assert_eq!(pd.action_label(1, 1), "D");

    let bos = BattleOfSexes::default();
    assert_same_game(&bos, &bos.to_matrix());
    assert_eq!(bos.action_label(0, 0), "B");
    assert_eq!(bos.action_label(1, 1), "V");

    let matching = MatchingPennies::default();
    assert_same_game(&matching, &matching.to_matrix());
    assert_eq!(matching.action_label(0, 0), "H");
    assert_eq!(matching.action_label(1, 1), "T");

    let asymmetric_stag =
        StagHunt::new([[9.0, 0.0], [8.0, 7.0]], [[10.0, 8.0], [0.0, 6.0]]).unwrap();
    assert!(!asymmetric_stag.is_symmetric());
    assert_same_game(&asymmetric_stag, &asymmetric_stag.to_matrix());
    assert_eq!(asymmetric_stag.action_label(0, 0), "stag");
    assert_eq!(asymmetric_stag.action_label(1, 1), "hare");

    let rps = RockPaperScissors::new(2.0, 3.0).unwrap();
    assert_same_game(&rps, &rps.to_matrix());
    assert_eq!(rps.action_label(0, 0), "R");
    assert_eq!(rps.action_label(0, 1), "P");
    assert_eq!(rps.action_label(0, 2), "S");
}

#[test]
fn serde_round_trip_runs_validation_on_deserialization() {
    let pd = PrisonersDilemma::new(5.0, 3.0, 1.0, 0.0).unwrap();
    let bos = BattleOfSexes::default();
    let matching = MatchingPennies::default();
    let stag = StagHunt::default();
    let rps = RockPaperScissors::default();
    assert_eq!(
        serde_json::from_value::<PrisonersDilemma>(serde_json::to_value(&pd).unwrap()).unwrap(),
        pd
    );
    assert_eq!(
        serde_json::from_value::<BattleOfSexes>(serde_json::to_value(&bos).unwrap()).unwrap(),
        bos
    );
    assert_eq!(
        serde_json::from_value::<MatchingPennies>(serde_json::to_value(&matching).unwrap())
            .unwrap(),
        matching
    );
    assert_eq!(
        serde_json::from_value::<StagHunt>(serde_json::to_value(&stag).unwrap()).unwrap(),
        stag
    );
    assert_eq!(
        serde_json::from_value::<RockPaperScissors>(serde_json::to_value(&rps).unwrap()).unwrap(),
        rps
    );
    assert!(serde_json::from_str::<PrisonersDilemma>(
        r#"{"temptation":3.0,"reward":3.0,"punishment":1.0,"sucker":0.0}"#
    )
    .is_err());
    assert!(
        serde_json::from_str::<BattleOfSexes>(r#"{"a":[[1,-1],[-1,2]],"b":[[1,-1],[-1,2]]}"#)
            .is_err()
    );
    assert!(
        serde_json::from_str::<MatchingPennies>(r#"{"a":[[1,0],[0,-1]],"b":[[-1,0],[0,1]]}"#)
            .is_err()
    );
    assert!(serde_json::from_str::<StagHunt>(r#"{"a":[[7,0],[8,9]],"b":[[9,8],[0,7]]}"#).is_err());
    assert!(serde_json::from_str::<RockPaperScissors>(r#"{"win":0.0,"loss":1.0}"#).is_err());
}
