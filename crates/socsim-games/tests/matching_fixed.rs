use socsim_games::{
    assemble, build_world, AssignGroup, AssignKind, Assignment, Credit, FixedPlayers,
    InitialActions, MatrixGame, NormalFormOutcome, PayoffAggregation, PlayOptions, RuleBook,
};

fn prisoners_dilemma() -> MatrixGame {
    MatrixGame::bimatrix(
        &[vec![3.0, 0.0], vec![5.0, 1.0]],
        &[vec![3.0, 5.0], vec![0.0, 1.0]],
    )
    .unwrap()
}

fn run(
    aggregation: PayoffAggregation,
) -> socsim_engine::Simulation<socsim_games::GameWorld<MatrixGame>> {
    let rules = RuleBook::new();
    let world = build_world(
        prisoners_dilemma(),
        5,
        (),
        InitialActions::PerAgent(vec![1, 1, 0, 0, 1]),
        &rules,
        &Assignment::Ordered(vec![AssignGroup {
            kind: AssignKind::Held,
            count: 5,
        }]),
        3,
        1,
        19,
    )
    .unwrap();
    let mut simulation = assemble(
        world,
        FixedPlayers::pairs(&[(2, 1), (4, 0)]),
        rules,
        NormalFormOutcome,
        PlayOptions::new().payoff_aggregation(aggregation),
        19,
    )
    .unwrap()
    .build();
    simulation.step().unwrap();
    simulation
}

#[test]
fn fixed_players_preserves_pair_and_seat_order() {
    let simulation = run(PayoffAggregation::Sum);
    let arena = simulation.world().plays();

    let first = arena.encounter(socsim_games::EncounterId(0)).unwrap();
    assert_eq!(
        first.seats,
        &[socsim_core::AgentId(2), socsim_core::AgentId(1)]
    );
    assert_eq!(first.credit, Credit::All);
    assert_eq!(arena.actions(first.id).unwrap(), &[Some(0), Some(1)]);

    let second = arena.encounter(socsim_games::EncounterId(1)).unwrap();
    assert_eq!(
        second.seats,
        &[socsim_core::AgentId(4), socsim_core::AgentId(0)]
    );
    assert_eq!(arena.actions(second.id).unwrap(), &[Some(1), Some(1)]);
}

#[test]
fn resting_players_and_sum_or_mean_aggregation_match_hand_calculation() {
    for aggregation in [PayoffAggregation::Sum, PayoffAggregation::Mean] {
        let simulation = run(aggregation);
        assert_eq!(simulation.world().payoffs(), &[1.0, 5.0, 0.0, 0.0, 1.0]);
        assert_eq!(simulation.world().n_played(), &[1, 1, 1, 0, 1]);

        let resting = simulation
            .world()
            .history(socsim_core::AgentId(3))
            .unwrap()
            .last()
            .unwrap();
        assert_eq!(resting.action, None);
        assert_eq!(resting.payoff, 0.0);
        assert_eq!(resting.n_played, 0);
    }
}
