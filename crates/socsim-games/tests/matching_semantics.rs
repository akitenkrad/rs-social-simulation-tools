use serde_json::Value;
use std::any::Any;

use socsim_core::{AgentId, Recorder};
use socsim_games::{
    assemble, assemble_with, build_world, counting_play, AssignGroup, AssignKind, Assignment,
    Credit, DecisionScope, EncounterId, GameMetrics, InitialActions, MatrixGame, MetricsOptions,
    NoDynamics, NormalFormOutcome, Pairwise, PayoffAggregation, PlayOptions, PlayPath, RoundRobin,
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

#[test]
fn random_pairs_leave_exactly_one_uniformly_selected_agent_idle_when_odd() {
    let n_agents = 5;
    let rules = RuleBook::new();
    let world = build_world(
        prisoners_dilemma(),
        n_agents,
        (),
        InitialActions::Same(0),
        &rules,
        &Assignment::Ordered(vec![AssignGroup {
            kind: AssignKind::Held,
            count: n_agents,
        }]),
        1,
        1,
        41,
    )
    .unwrap();
    let mut simulation = assemble(
        world,
        socsim_games::RandomPairs::new(),
        rules,
        NormalFormOutcome,
        PlayOptions::new(),
        41,
    )
    .unwrap()
    .build();

    simulation.step().unwrap();

    let arena = simulation.world().plays();
    assert_eq!(arena.n_encounters(), 2);
    let mut seated = Vec::new();
    for encounter_index in 0..arena.n_encounters() {
        let encounter = arena
            .encounter(EncounterId(encounter_index as u32))
            .unwrap();
        assert_eq!(encounter.credit, Credit::All);
        seated.extend(encounter.seats.iter().map(|agent| agent.0));
    }
    seated.sort_unstable();
    seated.dedup();
    assert_eq!(seated.len(), 4);
    assert_eq!(
        simulation
            .world()
            .n_played()
            .iter()
            .filter(|&&count| count == 0)
            .count(),
        1
    );
}

#[test]
fn random_groups_reject_small_groups_and_leave_only_the_remainder_idle() {
    assert!(socsim_games::RandomGroups::new(0).is_err());
    assert!(socsim_games::RandomGroups::new(1).is_err());

    let rules = RuleBook::new();
    let wrong_roles = build_world(
        prisoners_dilemma(),
        3,
        (),
        InitialActions::Same(0),
        &rules,
        &Assignment::Ordered(vec![AssignGroup {
            kind: AssignKind::Held,
            count: 3,
        }]),
        1,
        1,
        43,
    )
    .unwrap();
    assert!(matches!(
        assemble(
            wrong_roles,
            socsim_games::RandomGroups::new(3).unwrap(),
            rules,
            NormalFormOutcome,
            PlayOptions::new(),
            43,
        ),
        Err(socsim_games::GameError::UnsupportedRoles { n_roles: 2 })
    ));

    let n_agents = 8;
    let game = MatrixGame::new(vec![1, 1, 1], vec![vec![1.0]; 3]).unwrap();
    let rules = RuleBook::new();
    let world = build_world(
        game,
        n_agents,
        (),
        InitialActions::Same(0),
        &rules,
        &Assignment::Ordered(vec![AssignGroup {
            kind: AssignKind::Held,
            count: n_agents,
        }]),
        1,
        1,
        43,
    )
    .unwrap();
    let mut simulation = assemble(
        world,
        socsim_games::RandomGroups::new(3).unwrap(),
        rules,
        NormalFormOutcome,
        PlayOptions::new(),
        43,
    )
    .unwrap()
    .build();

    simulation.step().unwrap();

    assert_eq!(simulation.world().plays().n_encounters(), 2);
    assert_eq!(
        simulation
            .world()
            .n_played()
            .iter()
            .filter(|&&count| count == 0)
            .count(),
        2
    );
    assert!(simulation
        .world()
        .n_played()
        .iter()
        .all(|&count| count <= 1));
}

#[test]
fn random_groups_choose_the_idle_agent_uniformly_over_many_steps() {
    let n_agents = 10;
    let steps = 30_000;
    let game = MatrixGame::new(vec![1, 1, 1], vec![vec![1.0]; 3]).unwrap();
    let rules = RuleBook::new();
    let world = build_world(
        game,
        n_agents,
        (),
        InitialActions::Same(0),
        &rules,
        &Assignment::Ordered(vec![AssignGroup {
            kind: AssignKind::Held,
            count: n_agents,
        }]),
        1,
        steps,
        47,
    )
    .unwrap();
    let mut simulation = assemble(
        world,
        socsim_games::RandomGroups::new(3).unwrap(),
        rules,
        NormalFormOutcome,
        PlayOptions::new(),
        47,
    )
    .unwrap()
    .build();
    let mut idle = vec![0_u64; n_agents];

    for _ in 0..steps {
        simulation.step().unwrap();
        for (count, &played) in idle.iter_mut().zip(simulation.world().n_played()) {
            *count += u64::from(played == 0);
        }
    }

    let expected = steps as f64 * 0.1;
    let four_sigma = 4.0 * (steps as f64 * 0.1 * 0.9).sqrt();
    for count in idle {
        assert!((count as f64 - expected).abs() <= four_sigma);
    }
}

#[test]
fn whole_population_uses_sorted_agent_ids_and_checks_the_role_count() {
    let game = MatrixGame::new(vec![1, 1, 1], vec![vec![1.0]; 3]).unwrap();
    let rules = RuleBook::new();
    let world = build_world(
        game.clone(),
        3,
        (),
        InitialActions::Same(0),
        &rules,
        &Assignment::Ordered(vec![AssignGroup {
            kind: AssignKind::Held,
            count: 3,
        }]),
        1,
        1,
        53,
    )
    .unwrap();
    let mut simulation = assemble(
        world,
        socsim_games::WholePopulation::new(),
        rules,
        NormalFormOutcome,
        PlayOptions::new().materialize(true),
        53,
    )
    .unwrap()
    .build();
    simulation.step().unwrap();
    assert_eq!(
        simulation
            .world()
            .plays()
            .encounter(EncounterId(0))
            .unwrap()
            .seats
            .iter()
            .map(|agent| agent.0)
            .collect::<Vec<_>>(),
        vec![0, 1, 2]
    );

    let rules = RuleBook::new();
    let mismatch = build_world(
        game,
        4,
        (),
        InitialActions::Same(0),
        &rules,
        &Assignment::Ordered(vec![AssignGroup {
            kind: AssignKind::Held,
            count: 4,
        }]),
        1,
        1,
        53,
    )
    .unwrap();
    assert!(matches!(
        assemble(
            mismatch,
            socsim_games::WholePopulation::new(),
            rules,
            NormalFormOutcome,
            PlayOptions::new(),
            53,
        ),
        Err(socsim_games::GameError::InvalidMatching(_))
    ));
}

#[test]
fn center_credit_applies_only_role_zero_payoff_even_for_self_play() {
    #[derive(Clone, Copy, Debug)]
    struct CenterMatching;

    impl socsim_games::Matching<MatrixGame, ()> for CenterMatching {
        fn name(&self) -> &str {
            "center"
        }

        fn check(
            &self,
            _ctx: &socsim_games::MatchContext<'_, MatrixGame, ()>,
        ) -> Result<(), socsim_games::GameError> {
            Ok(())
        }

        fn allows_multi_role_per_step(&self) -> bool {
            true
        }

        fn encounters_into(
            &self,
            _ctx: &socsim_games::MatchContext<'_, MatrixGame, ()>,
            _state: &mut socsim_games::MatchingState,
            _rng: &mut socsim_core::SimRng,
            out: &mut socsim_games::ArenaWriter<'_, usize>,
        ) -> socsim_core::Result<()> {
            out.push(&[AgentId(0), AgentId(1)], Credit::Center)?;
            out.push(&[AgentId(2), AgentId(2)], Credit::Center)?;
            Ok(())
        }
    }

    let rules = RuleBook::new();
    let world = build_world(
        prisoners_dilemma(),
        3,
        (),
        InitialActions::PerAgent(vec![1, 0, 0]),
        &rules,
        &Assignment::Ordered(vec![AssignGroup {
            kind: AssignKind::Held,
            count: 3,
        }]),
        2,
        1,
        59,
    )
    .unwrap();
    let mut simulation = assemble(
        world,
        CenterMatching,
        rules,
        NormalFormOutcome,
        PlayOptions::new(),
        59,
    )
    .unwrap()
    .build();

    simulation.step().unwrap();

    assert_eq!(simulation.world().payoffs(), &[5.0, 0.0, 3.0]);
    assert_eq!(simulation.world().n_played(), &[1, 0, 1]);
    assert_eq!(
        simulation.world().plays().payoffs(EncounterId(0)).unwrap(),
        &[5.0, 0.0]
    );
    assert_eq!(
        simulation.world().plays().payoffs(EncounterId(1)).unwrap(),
        &[3.0, 3.0]
    );
    assert_eq!(
        simulation
            .world()
            .history(AgentId(2))
            .unwrap()
            .last()
            .unwrap()
            .action,
        Some(0)
    );
}

#[test]
fn opted_in_multi_role_per_step_uses_all_opponents_and_validates_every_role() {
    #[derive(Clone, Copy, Debug)]
    struct MultiRoleMatching;

    impl socsim_games::Matching<MatrixGame, ()> for MultiRoleMatching {
        fn name(&self) -> &str {
            "multi_role"
        }

        fn check(
            &self,
            _ctx: &socsim_games::MatchContext<'_, MatrixGame, ()>,
        ) -> Result<(), socsim_games::GameError> {
            Ok(())
        }

        fn allows_multi_role_per_step(&self) -> bool {
            true
        }

        fn encounters_into(
            &self,
            _ctx: &socsim_games::MatchContext<'_, MatrixGame, ()>,
            _state: &mut socsim_games::MatchingState,
            _rng: &mut socsim_core::SimRng,
            out: &mut socsim_games::ArenaWriter<'_, usize>,
        ) -> socsim_core::Result<()> {
            // Agent 0's last seat is role 0. The asymmetric case below makes
            // action 1 valid there but invalid in the earlier role-1 seat, so
            // this test distinguishes all-role validation from validating only
            // the last seat's role.
            out.push(&[AgentId(1), AgentId(0)], Credit::All)?;
            out.push(&[AgentId(0), AgentId(2)], Credit::All)?;
            Ok(())
        }
    }

    #[derive(Clone, Copy, Debug)]
    struct ProbeRule {
        action: usize,
    }

    impl socsim_games::DecisionRule<MatrixGame, ()> for ProbeRule {
        fn name(&self) -> &str {
            "probe"
        }

        fn init_state(
            &self,
            _ctx: &socsim_games::RuleInitContext,
            _game: &MatrixGame,
        ) -> Result<socsim_games::BuiltinPolicyState, socsim_games::RuleError> {
            Ok(<socsim_games::BuiltinPolicyState as socsim_games::Slot<
                socsim_games::Stateless,
            >>::from_slot(socsim_games::Stateless))
        }

        fn decide(
            &self,
            _state: &mut socsim_games::BuiltinPolicyState,
            ctx: &socsim_games::DecisionContext<'_, MatrixGame, ()>,
            _rng: &mut socsim_core::SimRng,
        ) -> Result<usize, socsim_games::RuleError> {
            if ctx.opponents() != [AgentId(1), AgentId(2)]
                || ctx.peer_held(AgentId(1)) != Some(&0)
                || ctx.peer_held(AgentId(2)) != Some(&0)
                || ctx.peer_held(AgentId(0)).is_some()
            {
                return Err(socsim_games::RuleError::Unsupported(
                    "multi-role per-step view did not include exactly the current peers".to_owned(),
                ));
            }
            Ok(self.action)
        }
    }

    fn run(action: usize, game: MatrixGame) -> socsim_core::Result<()> {
        let mut rules = RuleBook::new();
        rules
            .register("probe", std::sync::Arc::new(ProbeRule { action }))
            .unwrap();
        let world = build_world(
            game,
            3,
            (),
            InitialActions::PerAgent(vec![0, 0, 0]),
            &rules,
            &Assignment::Ordered(vec![
                AssignGroup {
                    kind: AssignKind::Rule("probe".to_owned()),
                    count: 1,
                },
                AssignGroup {
                    kind: AssignKind::Held,
                    count: 2,
                },
            ]),
            1,
            1,
            61,
        )
        .unwrap();
        let mut simulation = assemble(
            world,
            MultiRoleMatching,
            rules,
            NormalFormOutcome,
            PlayOptions::new(),
            61,
        )
        .unwrap()
        .build();
        simulation.step()
    }

    run(0, prisoners_dilemma()).unwrap();

    let asymmetric = MatrixGame::new(vec![2, 1], vec![vec![0.0, 0.0]; 2]).unwrap();
    let error = run(1, asymmetric).unwrap_err();
    assert!(error
        .to_string()
        .contains("rule returned an invalid action"));
}

#[derive(Clone, Copy, Debug)]
struct BoundsProbe<M> {
    inner: M,
    expected: socsim_games::EncounterBounds,
}

impl<M> socsim_games::Matching<MatrixGame, ()> for BoundsProbe<M>
where
    M: socsim_games::Matching<MatrixGame, ()>,
{
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn check(
        &self,
        ctx: &socsim_games::MatchContext<'_, MatrixGame, ()>,
    ) -> Result<(), socsim_games::GameError> {
        self.inner.check(ctx)?;
        assert_eq!(self.inner.encounter_bounds(ctx), Some(self.expected));
        Ok(())
    }

    fn supports_roles(&self, n_roles: usize) -> bool {
        self.inner.supports_roles(n_roles)
    }

    fn encounter_bounds(
        &self,
        ctx: &socsim_games::MatchContext<'_, MatrixGame, ()>,
    ) -> Option<socsim_games::EncounterBounds> {
        self.inner.encounter_bounds(ctx)
    }

    fn allows_multi_role_per_step(&self) -> bool {
        self.inner.allows_multi_role_per_step()
    }

    fn init_state(
        &self,
        ctx: &socsim_games::MatchContext<'_, MatrixGame, ()>,
    ) -> socsim_games::MatchingState {
        self.inner.init_state(ctx)
    }

    fn encounters_into(
        &self,
        ctx: &socsim_games::MatchContext<'_, MatrixGame, ()>,
        state: &mut socsim_games::MatchingState,
        rng: &mut socsim_core::SimRng,
        out: &mut socsim_games::ArenaWriter<'_, usize>,
    ) -> socsim_core::Result<()> {
        self.inner.encounters_into(ctx, state, rng, out)
    }
}

fn assert_encounter_bounds<M>(
    game: MatrixGame,
    n_agents: usize,
    matching: M,
    expected: socsim_games::EncounterBounds,
) where
    M: socsim_games::Matching<MatrixGame, ()> + 'static,
{
    let rules = RuleBook::new();
    let world = build_world(
        game,
        n_agents,
        (),
        InitialActions::Same(0),
        &rules,
        &Assignment::Ordered(vec![AssignGroup {
            kind: AssignKind::Held,
            count: n_agents,
        }]),
        1,
        1,
        67,
    )
    .unwrap();
    assemble(
        world,
        BoundsProbe {
            inner: matching,
            expected,
        },
        rules,
        NormalFormOutcome,
        PlayOptions::new(),
        67,
    )
    .unwrap();
}

#[test]
fn population_matchings_report_their_encounter_bounds() {
    let pair_game = prisoners_dilemma();
    assert_encounter_bounds(
        pair_game.clone(),
        4,
        socsim_games::RandomPairs::new(),
        socsim_games::EncounterBounds { min: 1, max: 1 },
    );
    assert_encounter_bounds(
        pair_game,
        5,
        socsim_games::RandomPairs::new(),
        socsim_games::EncounterBounds { min: 0, max: 1 },
    );

    let group_game = MatrixGame::new(vec![1, 1, 1], vec![vec![1.0]; 3]).unwrap();
    assert_encounter_bounds(
        group_game.clone(),
        6,
        socsim_games::RandomGroups::new(3).unwrap(),
        socsim_games::EncounterBounds { min: 1, max: 1 },
    );
    assert_encounter_bounds(
        group_game.clone(),
        7,
        socsim_games::RandomGroups::new(3).unwrap(),
        socsim_games::EncounterBounds { min: 0, max: 1 },
    );
    assert_encounter_bounds(
        group_game,
        3,
        socsim_games::WholePopulation::new(),
        socsim_games::EncounterBounds { min: 1, max: 1 },
    );
}

#[test]
fn topology_none_keeps_its_legacy_json_shape() {
    assert_eq!(
        serde_json::to_string(&socsim_games::Topology::None).unwrap(),
        "\"None\""
    );
}

#[cfg(feature = "net")]
fn held_world(n_agents: usize) -> socsim_games::GameWorld<MatrixGame> {
    let rules = RuleBook::new();
    build_world(
        prisoners_dilemma(),
        n_agents,
        (),
        InitialActions::Same(0),
        &rules,
        &Assignment::Ordered(vec![AssignGroup {
            kind: AssignKind::Held,
            count: n_agents,
        }]),
        1,
        2,
        71,
    )
    .unwrap()
}

#[cfg(feature = "net")]
fn run_network_edges(
    world: socsim_games::GameWorld<MatrixGame>,
) -> (Vec<([AgentId; 2], Credit)>, socsim_games::EncounterBounds) {
    #[derive(Clone, Debug)]
    struct NetworkBoundsProbe {
        observed: std::sync::Arc<std::sync::Mutex<Option<socsim_games::EncounterBounds>>>,
    }

    impl socsim_games::Matching<MatrixGame, ()> for NetworkBoundsProbe {
        fn name(&self) -> &str {
            "network_bounds_probe"
        }

        fn check(
            &self,
            ctx: &socsim_games::MatchContext<'_, MatrixGame, ()>,
        ) -> Result<(), socsim_games::GameError> {
            let inner = socsim_games::NetworkEdges::new();
            inner.check(ctx)?;
            *self.observed.lock().unwrap() = inner.encounter_bounds(ctx);
            Ok(())
        }

        fn encounter_bounds(
            &self,
            ctx: &socsim_games::MatchContext<'_, MatrixGame, ()>,
        ) -> Option<socsim_games::EncounterBounds> {
            socsim_games::NetworkEdges::new().encounter_bounds(ctx)
        }

        fn allows_multi_role_per_step(&self) -> bool {
            true
        }

        fn encounters_into(
            &self,
            ctx: &socsim_games::MatchContext<'_, MatrixGame, ()>,
            state: &mut socsim_games::MatchingState,
            rng: &mut socsim_core::SimRng,
            out: &mut socsim_games::ArenaWriter<'_, usize>,
        ) -> socsim_core::Result<()> {
            socsim_games::NetworkEdges::new().encounters_into(ctx, state, rng, out)
        }
    }

    let observed = std::sync::Arc::new(std::sync::Mutex::new(None));
    let rules = RuleBook::new();
    let mut simulation = assemble(
        world,
        NetworkBoundsProbe {
            observed: observed.clone(),
        },
        rules,
        NormalFormOutcome,
        PlayOptions::new(),
        71,
    )
    .unwrap()
    .build();
    simulation.step().unwrap();
    let arena = simulation.world().plays();
    let encounters = (0..arena.n_encounters())
        .map(|index| {
            let encounter = arena.encounter(EncounterId(index as u32)).unwrap();
            ([encounter.seats[0], encounter.seats[1]], encounter.credit)
        })
        .collect();
    let bounds = observed.lock().unwrap().unwrap();
    (encounters, bounds)
}

#[cfg(feature = "net")]
#[test]
fn network_edges_are_sorted_and_stable_across_snapshot_round_trips() {
    let mut network = socsim_net::SocialNetwork::empty();
    for id in [AgentId(3), AgentId(1), AgentId(0), AgentId(2)] {
        network.add_node(id);
    }
    network.add_edge(AgentId(2), AgentId(3));
    network.add_edge(AgentId(0), AgentId(2));
    network.add_edge(AgentId(1), AgentId(3));
    assert!(network.remove_edge(AgentId(0), AgentId(2)));
    network.add_edge(AgentId(0), AgentId(1));

    let world = held_world(4)
        .with_topology(socsim_games::Topology::network(network))
        .unwrap();
    let restored = serde_json::from_str(&serde_json::to_string(&world).unwrap()).unwrap();

    let expected = vec![
        ([AgentId(0), AgentId(1)], Credit::All),
        ([AgentId(1), AgentId(3)], Credit::All),
        ([AgentId(2), AgentId(3)], Credit::All),
    ];
    let expected_bounds = socsim_games::EncounterBounds { min: 1, max: 2 };
    assert_eq!(
        run_network_edges(world),
        (expected.clone(), expected_bounds)
    );
    assert_eq!(run_network_edges(restored), (expected, expected_bounds));
}

#[cfg(feature = "net")]
#[test]
fn directed_network_edges_preserve_arc_direction_and_use_out_neighbors() {
    let mut network = socsim_net::DiSocialNetwork::empty();
    for id in [AgentId(2), AgentId(0), AgentId(1)] {
        network.add_node(id);
    }
    network.add_edge(AgentId(2), AgentId(0));
    network.add_edge(AgentId(1), AgentId(0));
    network.add_edge(AgentId(0), AgentId(1));

    let world = held_world(3)
        .with_topology(socsim_games::Topology::directed_network(network))
        .unwrap();
    assert_eq!(
        socsim_core::Neighbors::neighbors_of(&world, AgentId(0)),
        vec![AgentId(1)]
    );
    assert_eq!(
        socsim_core::Neighbors::neighbors_of(&world, AgentId(1)),
        vec![AgentId(0)]
    );
    assert_eq!(
        socsim_core::Neighbors::neighbors_of(&world, AgentId(2)),
        vec![AgentId(0)]
    );

    let expected = vec![
        ([AgentId(0), AgentId(1)], Credit::All),
        ([AgentId(1), AgentId(0)], Credit::All),
        ([AgentId(2), AgentId(0)], Credit::All),
    ];
    assert_eq!(
        run_network_edges(world),
        (expected, socsim_games::EncounterBounds { min: 1, max: 3 })
    );
}

#[cfg(feature = "grid")]
#[test]
fn grid_neighborhood_uses_sorted_adjacency_center_credit_and_optional_self_play() {
    let spec = socsim_games::GridSpec {
        rows: 2,
        cols: 2,
        boundary: socsim_games::GridBoundary::Fixed,
        neighborhood: socsim_games::GridNeighbors::VonNeumann,
    };
    let topology = socsim_games::Topology::grid(spec).unwrap();
    let topology: socsim_games::Topology =
        serde_json::from_str(&serde_json::to_string(&topology).unwrap()).unwrap();
    let rules = RuleBook::new();
    let world = build_world(
        prisoners_dilemma(),
        4,
        (),
        InitialActions::Same(0),
        &rules,
        &Assignment::Ordered(vec![AssignGroup {
            kind: AssignKind::Held,
            count: 4,
        }]),
        1,
        1,
        73,
    )
    .unwrap()
    .with_topology(topology)
    .unwrap();
    assert_eq!(
        socsim_core::Neighbors::neighbors_of(&world, AgentId(0)),
        vec![AgentId(1), AgentId(2)]
    );

    let mut simulation = assemble(
        world,
        socsim_games::GridNeighborhood::new().include_self(true),
        rules,
        NormalFormOutcome,
        PlayOptions::new().materialize(true),
        73,
    )
    .unwrap()
    .build();
    simulation.step().unwrap();

    let arena = simulation.world().plays();
    assert_eq!(arena.n_encounters(), 12);
    let encounters = (0..arena.n_encounters())
        .map(|index| arena.encounter(EncounterId(index as u32)).unwrap())
        .collect::<Vec<_>>();
    assert!(encounters
        .iter()
        .all(|encounter| encounter.credit == Credit::Center));
    for center in 0..4_u64 {
        assert_eq!(
            encounters
                .iter()
                .filter(|encounter| encounter.seats[0] == AgentId(center))
                .count(),
            3
        );
        assert!(encounters.iter().any(|encounter| {
            encounter.seats[0] == AgentId(center) && encounter.seats[1] == AgentId(center)
        }));
    }
    assert_eq!(simulation.world().n_played(), &[3, 3, 3, 3]);
}

#[cfg(all(feature = "net", feature = "grid"))]
#[test]
fn topology_specific_matchings_reject_the_wrong_topology() {
    let rules = RuleBook::new();
    assert!(matches!(
        assemble(
            held_world(2),
            socsim_games::NetworkEdges::new(),
            rules,
            NormalFormOutcome,
            PlayOptions::new(),
            79,
        ),
        Err(socsim_games::GameError::InvalidMatching(_))
    ));

    let rules = RuleBook::new();
    assert!(matches!(
        assemble(
            held_world(2),
            socsim_games::GridNeighborhood::new(),
            rules,
            NormalFormOutcome,
            PlayOptions::new(),
            79,
        ),
        Err(socsim_games::GameError::InvalidMatching(_))
    ));
}

fn held_assignment(n_agents: usize) -> Assignment {
    Assignment::Ordered(vec![AssignGroup {
        kind: AssignKind::Held,
        count: n_agents,
    }])
}

fn run_public_goods<P>(
    play: P,
) -> socsim_engine::Simulation<socsim_games::GameWorld<socsim_games::catalog::PublicGoodsFinite>>
where
    P: socsim_games::PlayPath<socsim_games::catalog::PublicGoodsFinite, ()>,
{
    let game = socsim_games::catalog::PublicGoodsFinite::new(4, 2.0, 1.0).unwrap();
    let rules = RuleBook::new();
    let world = build_world(
        game,
        4,
        (),
        InitialActions::PerAgent(vec![0, 1, 0, 1]),
        &rules,
        &held_assignment(4),
        2,
        1,
        83,
    )
    .unwrap();
    let mut simulation = assemble_with(
        world,
        socsim_games::WholePopulation::new(),
        rules,
        play,
        NoDynamics,
        PlayOptions::new().materialize(true),
        83,
    )
    .unwrap()
    .build();
    simulation.step().unwrap();
    simulation
}

#[test]
fn counting_and_pairwise_kernels_agree_for_public_goods() {
    let pairwise = run_public_goods(Pairwise::new(NormalFormOutcome));
    let counting = run_public_goods(counting_play());

    assert_eq!(counting.world().payoffs(), pairwise.world().payoffs());
    assert_eq!(counting.world().n_played(), pairwise.world().n_played());
    assert_eq!(counting.world().payoffs(), &[0.0, 1.0, 0.0, 1.0]);
    assert_eq!(counting.world().n_played(), &[1, 1, 1, 1]);
}

#[cfg(feature = "grid")]
fn run_grid(materialize: bool) -> socsim_engine::Simulation<socsim_games::GameWorld<MatrixGame>> {
    let rules = RuleBook::new();
    let world = build_world(
        prisoners_dilemma(),
        4,
        (),
        InitialActions::PerAgent(vec![0, 1, 0, 1]),
        &rules,
        &held_assignment(4),
        2,
        1,
        89,
    )
    .unwrap()
    .with_topology(
        socsim_games::Topology::grid(socsim_games::GridSpec {
            rows: 2,
            cols: 2,
            boundary: socsim_games::GridBoundary::Fixed,
            neighborhood: socsim_games::GridNeighbors::VonNeumann,
        })
        .unwrap(),
    )
    .unwrap();
    let mut simulation = assemble_with(
        world,
        socsim_games::GridNeighborhood::new().include_self(true),
        rules,
        Pairwise::new(NormalFormOutcome),
        NoDynamics,
        PlayOptions::new().materialize(materialize),
        89,
    )
    .unwrap()
    .build();
    simulation.step().unwrap();
    simulation
}

#[cfg(feature = "grid")]
#[test]
fn implicit_grid_matches_materialized_payoffs_and_counts_without_building_an_arena() {
    let materialized = run_grid(true);
    let implicit = run_grid(false);

    assert_eq!(implicit.world().payoffs(), materialized.world().payoffs());
    assert_eq!(implicit.world().n_played(), materialized.world().n_played());
    assert_eq!(implicit.world().plays().n_encounters(), 0);
    assert_eq!(materialized.world().plays().n_encounters(), 12);
}

#[derive(Default)]
struct MetricRecorder {
    metrics: Vec<(u64, String, f64)>,
}

impl Recorder for MetricRecorder {
    fn record_metric(&mut self, t: u64, key: &str, value: f64) {
        self.metrics.push((t, key.to_owned(), value));
    }

    fn record_event(&mut self, _t: u64, _kind: &str, _payload: serde_json::Value) {}

    fn record_row(&mut self, t: u64, table: &str, row: &[(&str, f64)]) {
        self.metrics.extend(
            row.iter()
                .map(|(column, value)| (t, format!("{table}.{column}"), *value)),
        );
    }

    fn as_any(&self) -> Option<&dyn Any> {
        Some(self)
    }
}

fn aggregate_metric<P>(play: P) -> (usize, f64, f64)
where
    P: PlayPath<socsim_games::catalog::MinorityGame, ()>,
{
    let game = socsim_games::catalog::MinorityGame::new(5).unwrap();
    let metrics = GameMetrics::new(&game, MetricsOptions::new()).unwrap();
    let rules = RuleBook::new();
    let world = build_world(
        game,
        5,
        (),
        InitialActions::PerAgent(vec![0, 0, 1, 1, 1]),
        &rules,
        &held_assignment(5),
        2,
        1,
        97,
    )
    .unwrap();
    let mut simulation = assemble_with(
        world,
        socsim_games::WholePopulation::new(),
        rules,
        play,
        NoDynamics,
        PlayOptions::new(),
        97,
    )
    .unwrap()
    .add_mechanism(Box::new(metrics))
    .recorder(Box::new(MetricRecorder::default()))
    .build();
    simulation.step().unwrap();
    let arena_len = simulation.world().plays().n_encounters();
    let recorder = simulation
        .recorder()
        .as_any()
        .unwrap()
        .downcast_ref::<MetricRecorder>()
        .unwrap();
    let metric = |name: &str| {
        recorder
            .metrics
            .iter()
            .find(|(_, key, _)| key == name)
            .map(|(_, _, value)| *value)
            .unwrap()
    };
    (
        arena_len,
        metric("game.aggregate.attendance"),
        metric("game.played_share.a0"),
    )
}

#[test]
fn counting_and_implicit_pairwise_paths_record_identical_aggregates_and_played_share() {
    let pairwise = aggregate_metric(Pairwise::new(NormalFormOutcome).with_aggregates(2));
    let counting = aggregate_metric(counting_play().with_aggregates(2));

    assert_eq!(pairwise, (0, 2.0, 0.4));
    assert_eq!(counting, pairwise);
}

fn world_json_without_plays<G>(world: &socsim_games::GameWorld<G>) -> Value
where
    G: socsim_games::StageGame + serde::Serialize,
    G::Action: serde::Serialize,
{
    let mut value = serde_json::to_value(world).unwrap();
    value.as_object_mut().unwrap().remove("plays").unwrap();
    value
}

/// Asserts that the materialized and implicit runs left identical observable
/// state: payoffs, play counts, every agent's step records, and the world JSON
/// without the play arena. The implicit run must not have filled the arena.
fn assert_paths_agree<G>(
    materialized: &socsim_engine::Simulation<socsim_games::GameWorld<G>>,
    implicit: &socsim_engine::Simulation<socsim_games::GameWorld<G>>,
) where
    G: socsim_games::StageGame + serde::Serialize,
    G::Action: serde::Serialize + PartialEq,
{
    assert!(materialized.world().plays().n_encounters() > 0);
    assert_eq!(implicit.world().plays().n_encounters(), 0);
    assert_eq!(implicit.world().payoffs(), materialized.world().payoffs());
    assert_eq!(implicit.world().n_played(), materialized.world().n_played());
    for index in 0..materialized.world().payoffs().len() {
        let agent = AgentId(index as u64);
        let expected: Vec<_> = materialized
            .world()
            .history(agent)
            .unwrap()
            .iter()
            .cloned()
            .collect();
        let actual: Vec<_> = implicit
            .world()
            .history(agent)
            .unwrap()
            .iter()
            .cloned()
            .collect();
        assert!(!expected.is_empty());
        assert_eq!(actual, expected, "step records differ for agent {index}");
    }
    assert_eq!(
        world_json_without_plays(implicit.world()),
        world_json_without_plays(materialized.world())
    );
}

const PATH_STEPS: u64 = 3;

fn run_steps<G: socsim_games::StageGame>(
    builder: socsim_engine::SimulationBuilder<socsim_games::GameWorld<G>>,
) -> socsim_engine::Simulation<socsim_games::GameWorld<G>> {
    let mut simulation = builder.build();
    for _ in 0..PATH_STEPS {
        simulation.step().unwrap();
    }
    simulation
}

#[cfg(feature = "grid")]
fn moore_grid_world(n_side: usize) -> socsim_games::GameWorld<MatrixGame> {
    let n_agents = n_side * n_side;
    let rules = RuleBook::new();
    build_world(
        prisoners_dilemma(),
        n_agents,
        (),
        InitialActions::PerAgent((0..n_agents).map(|i| usize::from(i % 3 == 0)).collect()),
        &rules,
        &held_assignment(n_agents),
        4,
        PATH_STEPS,
        113,
    )
    .unwrap()
    .with_topology(
        socsim_games::Topology::grid(socsim_games::GridSpec {
            rows: n_side,
            cols: n_side,
            boundary: socsim_games::GridBoundary::Toroidal,
            neighborhood: socsim_games::GridNeighbors::Moore,
        })
        .unwrap(),
    )
    .unwrap()
}

#[cfg(feature = "grid")]
#[test]
fn moore_grid_with_self_play_agrees_between_materialized_and_implicit_paths() {
    let run = |materialize: bool| {
        let rules = RuleBook::new();
        run_steps(
            assemble(
                moore_grid_world(5),
                socsim_games::GridNeighborhood::new().include_self(true),
                rules,
                NormalFormOutcome,
                PlayOptions::new().materialize(materialize),
                113,
            )
            .unwrap(),
        )
    };
    let materialized = run(true);
    let implicit = run(false);
    // 25 centers, each with 8 Moore neighbors and one self-play encounter.
    assert_eq!(materialized.world().plays().n_encounters(), 25 * 9);
    assert_paths_agree(&materialized, &implicit);
}

#[cfg(feature = "net")]
#[test]
fn undirected_network_edges_agree_between_materialized_and_implicit_paths() {
    let n_agents = 6;
    let mut network = socsim_net::SocialNetwork::empty();
    for id in (0..n_agents as u64).rev() {
        network.add_node(AgentId(id));
    }
    for (a, b) in [(4, 5), (0, 1), (2, 0), (1, 3), (3, 4), (5, 2), (1, 4)] {
        network.add_edge(AgentId(a), AgentId(b));
    }
    let run = |materialize: bool| {
        let rules = RuleBook::new();
        let world = build_world(
            prisoners_dilemma(),
            n_agents,
            (),
            InitialActions::PerAgent(vec![0, 1, 1, 0, 1, 0]),
            &rules,
            &held_assignment(n_agents),
            4,
            PATH_STEPS,
            127,
        )
        .unwrap()
        .with_topology(socsim_games::Topology::network(network.clone()))
        .unwrap();
        run_steps(
            assemble(
                world,
                socsim_games::NetworkEdges::new(),
                rules,
                NormalFormOutcome,
                PlayOptions::new().materialize(materialize),
                127,
            )
            .unwrap(),
        )
    };
    let materialized = run(true);
    let implicit = run(false);
    assert_eq!(materialized.world().plays().n_encounters(), 7);
    assert_paths_agree(&materialized, &implicit);
}

#[test]
fn whole_population_agrees_between_materialized_and_implicit_paths() {
    let run = |materialize: bool| {
        let game = socsim_games::catalog::PublicGoodsFinite::new(4, 2.0, 1.0).unwrap();
        let rules = RuleBook::new();
        let world = build_world(
            game,
            4,
            (),
            InitialActions::PerAgent(vec![1, 0, 1, 1]),
            &rules,
            &held_assignment(4),
            4,
            PATH_STEPS,
            131,
        )
        .unwrap();
        run_steps(
            assemble(
                world,
                socsim_games::WholePopulation::new(),
                rules,
                NormalFormOutcome,
                PlayOptions::new().materialize(materialize),
                131,
            )
            .unwrap(),
        )
    };
    let materialized = run(true);
    let implicit = run(false);
    assert_eq!(materialized.world().plays().n_encounters(), 1);
    assert_paths_agree(&materialized, &implicit);
}

#[cfg(all(feature = "grid", feature = "metrics"))]
fn grid_game_metrics(
    materialize: bool,
    source: socsim_games::MixedSource,
) -> (usize, Vec<(u64, String, f64)>) {
    let n_side = 5;
    let n_agents = n_side * n_side;
    let game = socsim_games::catalog::PrisonersDilemma::weak(1.6).unwrap();
    let metrics = GameMetrics::new(
        &game,
        MetricsOptions::new()
            .cooperate_label("C")
            .regret(true)
            .mixed_source(source),
    )
    .unwrap();
    let rules = RuleBook::new();
    let world = build_world(
        game,
        n_agents,
        (),
        InitialActions::PerAgent((0..n_agents).map(|i| usize::from(i % 4 == 1)).collect()),
        &rules,
        &held_assignment(n_agents),
        4,
        PATH_STEPS,
        137,
    )
    .unwrap()
    .with_topology(
        socsim_games::Topology::grid(socsim_games::GridSpec {
            rows: n_side,
            cols: n_side,
            boundary: socsim_games::GridBoundary::Toroidal,
            neighborhood: socsim_games::GridNeighbors::Moore,
        })
        .unwrap(),
    )
    .unwrap();
    let mut simulation = assemble(
        world,
        socsim_games::GridNeighborhood::new().include_self(true),
        rules,
        NormalFormOutcome,
        PlayOptions::new().materialize(materialize),
        137,
    )
    .unwrap()
    .add_mechanism(Box::new(metrics))
    .recorder(Box::new(MetricRecorder::default()))
    .build();
    for _ in 0..PATH_STEPS {
        simulation.step().unwrap();
    }
    let arena_len = simulation.world().plays().n_encounters();
    let recorder = simulation
        .recorder()
        .as_any()
        .unwrap()
        .downcast_ref::<MetricRecorder>()
        .unwrap();
    (arena_len, recorder.metrics.clone())
}

#[cfg(all(feature = "grid", feature = "metrics"))]
#[test]
fn game_metrics_columns_agree_between_materialized_and_implicit_grid_paths() {
    for source in [
        socsim_games::MixedSource::Played,
        socsim_games::MixedSource::PlayedCumulative,
    ] {
        let (materialized_len, materialized) = grid_game_metrics(true, source);
        let (implicit_len, implicit) = grid_game_metrics(false, source);
        assert!(materialized_len > 0);
        assert_eq!(implicit_len, 0);
        for t in 1..=PATH_STEPS {
            for column in [
                "game.played_share.a0",
                "game.played_share.a1",
                "game.coop_rate",
                "game.regret",
            ] {
                assert!(
                    implicit
                        .iter()
                        .any(|(step, key, _)| *step == t && key == column),
                    "{column} is missing at step {t} for {source:?}"
                );
            }
        }
        assert_eq!(implicit, materialized, "metric rows differ for {source:?}");
    }
}

fn run_minority_groups<P>(
    play: P,
) -> socsim_engine::Simulation<socsim_games::GameWorld<socsim_games::catalog::MinorityGame>>
where
    P: PlayPath<socsim_games::catalog::MinorityGame, ()>,
{
    let game = socsim_games::catalog::MinorityGame::new(3).unwrap();
    let rules = RuleBook::new();
    let world = build_world(
        game,
        7,
        (),
        InitialActions::PerAgent(vec![0, 1, 1, 0, 0, 1, 1]),
        &rules,
        &held_assignment(7),
        4,
        PATH_STEPS,
        139,
    )
    .unwrap();
    run_steps(
        assemble_with(
            world,
            socsim_games::RandomGroups::new(3).unwrap(),
            rules,
            play,
            NoDynamics,
            PlayOptions::new(),
            139,
        )
        .unwrap(),
    )
}

#[test]
fn counting_and_pairwise_kernels_agree_on_payoffs_and_aggregates_for_random_groups() {
    let pairwise = run_minority_groups(Pairwise::new(NormalFormOutcome).with_aggregates(4));
    let counting = run_minority_groups(counting_play().with_aggregates(4));

    let arena = pairwise.world().plays();
    assert_eq!(arena.n_encounters(), 2);
    assert_eq!(counting.world().plays().n_encounters(), 2);
    for index in 0..arena.n_encounters() {
        let id = EncounterId(index as u32);
        assert_eq!(
            counting.world().plays().encounter(id).unwrap().seats,
            arena.encounter(id).unwrap().seats
        );
        assert_eq!(
            counting.world().plays().payoffs(id).unwrap(),
            arena.payoffs(id).unwrap()
        );
    }
    assert_eq!(counting.world().payoffs(), pairwise.world().payoffs());
    assert_eq!(counting.world().n_played(), pairwise.world().n_played());
    // Some group must have had a winning minority during the run, so the
    // comparison is not between two all-zero payoff vectors.
    assert!((0..7_u64).any(|agent| {
        pairwise
            .world()
            .history(AgentId(agent))
            .unwrap()
            .iter()
            .any(|record| record.payoff > 0.0)
    }));
    for agent in 0..7_u64 {
        let expected: Vec<_> = pairwise
            .world()
            .history(AgentId(agent))
            .unwrap()
            .iter()
            .cloned()
            .collect();
        let actual: Vec<_> = counting
            .world()
            .history(AgentId(agent))
            .unwrap()
            .iter()
            .cloned()
            .collect();
        assert_eq!(actual, expected);
    }

    let pairwise_json = serde_json::to_value(pairwise.world()).unwrap();
    let counting_json = serde_json::to_value(counting.world()).unwrap();
    let aggregate = &pairwise_json["aggregate"];
    assert!(!aggregate.is_null());
    assert_eq!(&counting_json["aggregate"], aggregate);
    assert_eq!(counting_json, pairwise_json);
}

#[test]
fn counting_and_pairwise_kernels_write_the_same_aggregate_table_when_materialized() {
    let run = |counting: bool| {
        let game = socsim_games::catalog::MinorityGame::new(5).unwrap();
        let rules = RuleBook::new();
        let world = build_world(
            game,
            5,
            (),
            InitialActions::PerAgent(vec![1, 0, 1, 0, 0]),
            &rules,
            &held_assignment(5),
            4,
            PATH_STEPS,
            149,
        )
        .unwrap();
        let options = PlayOptions::new().materialize(true);
        let builder = if counting {
            assemble_with(
                world,
                socsim_games::WholePopulation::new(),
                rules,
                counting_play().with_aggregates(4),
                NoDynamics,
                options,
                149,
            )
        } else {
            assemble_with(
                world,
                socsim_games::WholePopulation::new(),
                rules,
                Pairwise::new(NormalFormOutcome).with_aggregates(4),
                NoDynamics,
                options,
                149,
            )
        };
        run_steps(builder.unwrap())
    };
    let pairwise = run(false);
    let counting = run(true);
    assert_eq!(pairwise.world().plays().n_encounters(), 1);
    assert_eq!(counting.world().plays().n_encounters(), 1);
    let pairwise_json = serde_json::to_value(pairwise.world()).unwrap();
    let counting_json = serde_json::to_value(counting.world()).unwrap();
    assert!(!pairwise_json["aggregate"].is_null());
    assert_eq!(counting_json["aggregate"], pairwise_json["aggregate"]);
    assert_eq!(counting.world().payoffs(), pairwise.world().payoffs());
}
