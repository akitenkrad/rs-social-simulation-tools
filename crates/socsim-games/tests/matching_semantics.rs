use serde_json::Value;
use socsim_games::{
    assemble, build_world, AssignGroup, AssignKind, Assignment, Credit, DecisionScope, EncounterId,
    InitialActions, MatrixGame, NormalFormOutcome, PayoffAggregation, PlayOptions, RoundRobin,
    RuleBook,
};

fn prisoners_dilemma() -> MatrixGame {
    MatrixGame::bimatrix(
        &[vec![3.0, 0.0], vec![5.0, 1.0]],
        &[vec![3.0, 5.0], vec![0.0, 1.0]],
    )
    .unwrap()
}

fn run(
    actions: Vec<usize>,
    both_orders: bool,
    aggregation: PayoffAggregation,
    steps: u64,
) -> socsim_engine::Simulation<socsim_games::GameWorld<MatrixGame>> {
    let n_agents = actions.len();
    let rules = RuleBook::new();
    let world = build_world(
        prisoners_dilemma(),
        n_agents,
        (),
        InitialActions::PerAgent(actions),
        &rules,
        &Assignment::Ordered(vec![AssignGroup {
            kind: AssignKind::Held,
            count: n_agents,
        }]),
        4,
        steps,
        31,
    )
    .unwrap();
    let mut simulation = assemble(
        world,
        RoundRobin::new().both_orders(both_orders),
        rules,
        NormalFormOutcome,
        PlayOptions::new()
            .payoff_aggregation(aggregation)
            .decision_scope(DecisionScope::PerEncounter),
        31,
    )
    .unwrap()
    .build();
    for _ in 0..steps {
        simulation.step().unwrap();
    }
    simulation
}

fn seats(
    simulation: &socsim_engine::Simulation<socsim_games::GameWorld<MatrixGame>>,
) -> Vec<[u64; 2]> {
    let arena = simulation.world().plays();
    (0..arena.n_encounters())
        .map(|index| {
            let encounter = arena.encounter(EncounterId(index as u32)).unwrap();
            assert_eq!(encounter.id, EncounterId(index as u32));
            assert_eq!(encounter.credit, Credit::All);
            [encounter.seats[0].0, encounter.seats[1].0]
        })
        .collect()
}

fn cursor(simulation: &socsim_engine::Simulation<socsim_games::GameWorld<MatrixGame>>) -> u64 {
    let value: Value = serde_json::to_value(simulation.world()).unwrap();
    value["matching_state"]["RoundRobin"]["cursor"]
        .as_u64()
        .unwrap()
}

#[test]
fn round_robin_lists_each_pair_once_in_lexicographic_order() {
    let simulation = run(vec![0, 1, 0, 1], false, PayoffAggregation::Sum, 1);
    assert_eq!(
        seats(&simulation),
        vec![[0, 1], [0, 2], [0, 3], [1, 2], [1, 3], [2, 3]]
    );
    assert_eq!(cursor(&simulation), 1);
}

#[test]
fn round_robin_places_reverse_order_immediately_after_each_pair() {
    let simulation = run(vec![0, 1, 0], true, PayoffAggregation::Sum, 1);
    assert_eq!(
        seats(&simulation),
        vec![[0, 1], [1, 0], [0, 2], [2, 0], [1, 2], [2, 1]]
    );
    assert_eq!(simulation.world().payoffs(), &[6.0, 20.0, 6.0]);
    assert_eq!(simulation.world().n_played(), &[4, 4, 4]);
}

#[test]
fn round_robin_sum_and_mean_match_hand_calculation_and_cursor_advances() {
    let sum = run(vec![0, 1, 0], false, PayoffAggregation::Sum, 2);
    assert_eq!(sum.world().payoffs(), &[3.0, 10.0, 3.0]);
    assert_eq!(sum.world().n_played(), &[2, 2, 2]);
    assert_eq!(cursor(&sum), 2);

    let mean = run(vec![0, 1, 0], false, PayoffAggregation::Mean, 1);
    assert_eq!(mean.world().payoffs(), &[1.5, 5.0, 1.5]);
    assert_eq!(mean.world().n_played(), &[2, 2, 2]);
}
