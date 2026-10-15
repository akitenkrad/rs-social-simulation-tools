use std::sync::Arc;

use socsim_core::{AgentId, SimRng};
use socsim_games::{
    assemble, assemble_with, build_world, counting_play, ArenaWriter, AssignGroup, AssignKind,
    Assignment, Automaton, AutomatonKind, Credit, DecisionScope, EncounterRef, FictitiousPlay,
    Fixed, GameError, InitialActions, MatchContext, Matching, MatchingState, MatrixGame, Mixed,
    NoDynamics, NormalFormOutcome, OutcomeContext, OutcomeRule, Pairwise, PlayOptions, RothErev,
    RoundRobin, RuleBook, RuleError,
};

fn game() -> MatrixGame {
    MatrixGame::zero_sum(&[vec![1.0, -1.0], vec![-1.0, 1.0]]).unwrap()
}

fn rules() -> RuleBook<MatrixGame, ()> {
    let mut rules = RuleBook::new();
    rules.register("fixed", Arc::new(Fixed::new(0))).unwrap();
    rules
}

fn world(n_agents: usize, rules: &RuleBook<MatrixGame, ()>) -> socsim_games::GameWorld<MatrixGame> {
    build_world(
        game(),
        n_agents,
        (),
        InitialActions::Same(0),
        rules,
        &Assignment::Ordered(vec![AssignGroup {
            kind: AssignKind::Rule("fixed".to_owned()),
            count: n_agents,
        }]),
        2,
        1,
        7,
    )
    .unwrap()
}

fn assemble_round_robin(
    n_agents: usize,
    both_orders: bool,
    scope: DecisionScope,
) -> Result<socsim_engine::SimulationBuilder<socsim_games::GameWorld<MatrixGame>>, GameError> {
    let rules = rules();
    assemble(
        world(n_agents, &rules),
        RoundRobin::new().both_orders(both_orders),
        rules,
        NormalFormOutcome,
        PlayOptions::new().decision_scope(scope),
        7,
    )
}

#[test]
fn per_step_rejects_round_robin_agents_that_hold_different_roles() {
    assert!(matches!(
        assemble_round_robin(3, false, DecisionScope::PerStep),
        Err(GameError::InvalidMatching(_))
    ));
    assert!(matches!(
        assemble_round_robin(2, true, DecisionScope::PerStep),
        Err(GameError::InvalidMatching(_))
    ));
}

#[test]
fn finer_scopes_accept_round_robin_agents_that_hold_different_roles() {
    for scope in [DecisionScope::PerAgentRole, DecisionScope::PerEncounter] {
        assert!(assemble_round_robin(3, false, scope).is_ok());
        assert!(assemble_round_robin(2, true, scope).is_ok());
    }
}

#[derive(Clone, Copy, Debug)]
struct LateAmbiguity;

impl Matching<MatrixGame, ()> for LateAmbiguity {
    fn name(&self) -> &str {
        "late_ambiguity"
    }

    fn check(&self, _ctx: &MatchContext<'_, MatrixGame, ()>) -> Result<(), GameError> {
        Ok(())
    }

    fn encounters_into(
        &self,
        _ctx: &MatchContext<'_, MatrixGame, ()>,
        _state: &mut MatchingState,
        _rng: &mut SimRng,
        out: &mut ArenaWriter<'_, usize>,
    ) -> socsim_core::Result<()> {
        out.push(&[AgentId(0), AgentId(1)], Credit::All)?;
        out.push(&[AgentId(2), AgentId(0)], Credit::All)?;
        Ok(())
    }
}

#[test]
fn per_step_reports_ambiguous_role_when_matching_cannot_guard_at_build_time() {
    let rules = rules();
    let mut simulation = assemble(
        world(3, &rules),
        LateAmbiguity,
        rules,
        NormalFormOutcome,
        PlayOptions::new(),
        7,
    )
    .unwrap()
    .build();

    let error = simulation.step().unwrap_err();
    assert_eq!(
        error.to_string(),
        "mechanism error: agent 0 holds more than one role in a per-step decision"
    );
}

#[test]
fn per_step_rejects_ambiguous_roles_for_held_agents_too() {
    let rules = RuleBook::new();
    let world = build_world(
        game(),
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
        7,
    )
    .unwrap();
    let mut simulation = assemble(
        world,
        LateAmbiguity,
        rules,
        NormalFormOutcome,
        PlayOptions::new(),
        7,
    )
    .unwrap()
    .build();

    let error = simulation.step().unwrap_err();
    assert_eq!(
        error.to_string(),
        "mechanism error: agent 0 holds more than one role in a per-step decision"
    );
}

#[test]
fn round_robin_rejects_self_play() {
    let rules = rules();
    let result = assemble(
        world(2, &rules),
        RoundRobin::new().self_play(true),
        rules,
        NormalFormOutcome,
        PlayOptions::new().decision_scope(DecisionScope::PerEncounter),
        7,
    );
    assert!(matches!(result, Err(GameError::InvalidMatching(_))));
}

fn one_rule_assignment(kind: &str) -> Assignment {
    Assignment::Ordered(vec![AssignGroup {
        kind: AssignKind::Rule(kind.to_owned()),
        count: 1,
    }])
}

fn assert_unsupported<T>(result: Result<T, GameError>) {
    assert!(matches!(
        result,
        Err(GameError::Rule(RuleError::Unsupported(_)))
    ));
}

#[test]
fn fictitious_play_rejects_a_three_role_game_while_building_the_world() {
    let game = MatrixGame::new(vec![2, 2, 2], vec![vec![1.0; 8]; 3]).unwrap();
    let mut rules = RuleBook::new();
    rules
        .register("fictitious", Arc::new(FictitiousPlay::new()))
        .unwrap();

    assert_unsupported(build_world(
        game,
        1,
        (),
        InitialActions::Same(0),
        &rules,
        &one_rule_assignment("fictitious"),
        1,
        1,
        7,
    ));
}

#[test]
fn roth_erev_rejects_non_positive_game_payoffs_while_building_the_world() {
    let game = MatrixGame::bimatrix(
        &[vec![3.0, 0.0], vec![5.0, 1.0]],
        &[vec![3.0, 5.0], vec![-1.0, 1.0]],
    )
    .unwrap();
    let mut rules = RuleBook::new();
    rules
        .register("roth_erev", Arc::new(RothErev::new(0.1, 0.2, 1.0)))
        .unwrap();

    assert_unsupported(build_world(
        game,
        1,
        (),
        InitialActions::Same(0),
        &rules,
        &one_rule_assignment("roth_erev"),
        1,
        1,
        7,
    ));
}

#[test]
fn automaton_rejects_a_three_action_game_while_building_the_world() {
    let payoffs = vec![vec![1.0; 3]; 3];
    let game = MatrixGame::bimatrix(&payoffs, &payoffs).unwrap();
    let mut rules = RuleBook::new();
    rules
        .register("automaton", Arc::new(Automaton::new(AutomatonKind::Tft, 0)))
        .unwrap();

    assert_unsupported(build_world(
        game,
        1,
        (),
        InitialActions::Same(0),
        &rules,
        &one_rule_assignment("automaton"),
        1,
        1,
        7,
    ));
}

#[test]
fn mixed_rejects_probabilities_that_do_not_sum_to_one() {
    let mut rules = RuleBook::new();
    rules
        .register("mixed", Arc::new(Mixed::new(vec![0.25, 0.25])))
        .unwrap();

    assert_unsupported(build_world(
        game(),
        1,
        (),
        InitialActions::Same(0),
        &rules,
        &one_rule_assignment("mixed"),
        1,
        1,
        7,
    ));
}

#[test]
fn mixed_rejects_a_probability_length_that_differs_from_any_role() {
    let mut rules = RuleBook::new();
    rules
        .register("mixed", Arc::new(Mixed::new(vec![1.0])))
        .unwrap();

    assert_unsupported(build_world(
        game(),
        1,
        (),
        InitialActions::Same(0),
        &rules,
        &one_rule_assignment("mixed"),
        1,
        1,
        7,
    ));
}

#[cfg(feature = "grid")]
#[test]
fn with_topology_rejects_invalid_grid_dimensions_and_population_size() {
    let invalid = socsim_games::GridSpec {
        rows: 0,
        cols: 2,
        boundary: socsim_games::GridBoundary::Fixed,
        neighborhood: socsim_games::GridNeighbors::Moore,
    };
    assert!(matches!(
        socsim_games::Topology::grid(invalid),
        Err(GameError::Topology(_))
    ));

    let spec = socsim_games::GridSpec {
        rows: 2,
        cols: 2,
        boundary: socsim_games::GridBoundary::Toroidal,
        neighborhood: socsim_games::GridNeighbors::Moore,
    };
    assert!(matches!(
        world(3, &rules()).with_topology(socsim_games::Topology::grid(spec).unwrap()),
        Err(GameError::Topology(_))
    ));
}

#[cfg(feature = "net")]
#[test]
fn with_topology_rejects_network_node_mismatches_and_self_loops() {
    let mut missing = socsim_net::SocialNetwork::empty();
    missing.add_node(AgentId(0));
    missing.add_node(AgentId(2));
    assert!(matches!(
        world(3, &rules()).with_topology(socsim_games::Topology::network(missing)),
        Err(GameError::Topology(_))
    ));

    let mut extra = socsim_net::SocialNetwork::empty();
    for id in [AgentId(0), AgentId(1), AgentId(2), AgentId(3)] {
        extra.add_node(id);
    }
    assert!(matches!(
        world(3, &rules()).with_topology(socsim_games::Topology::network(extra)),
        Err(GameError::Topology(_))
    ));

    let mut self_loop = socsim_net::DiSocialNetwork::empty();
    for id in [AgentId(0), AgentId(1), AgentId(2)] {
        self_loop.add_node(id);
    }
    self_loop.add_edge(AgentId(0), AgentId(0));
    assert!(matches!(
        world(3, &rules()).with_topology(socsim_games::Topology::directed_network(self_loop)),
        Err(GameError::Topology(_))
    ));
}

#[derive(Clone, Copy, Debug)]
struct CustomOutcome;

impl OutcomeRule<socsim_games::catalog::PublicGoodsFinite, ()> for CustomOutcome {
    fn name(&self) -> &str {
        "custom"
    }

    fn outcome(
        &self,
        _ctx: &OutcomeContext<'_, socsim_games::catalog::PublicGoodsFinite, ()>,
        _encounter: EncounterRef<'_>,
        _actions: &[usize],
        payoffs: &mut [f64],
    ) -> socsim_core::Result<()> {
        payoffs.fill(99.0);
        Ok(())
    }
}

fn public_goods_world() -> (
    socsim_games::GameWorld<socsim_games::catalog::PublicGoodsFinite>,
    RuleBook<socsim_games::catalog::PublicGoodsFinite, ()>,
) {
    let game = socsim_games::catalog::PublicGoodsFinite::new(3, 2.0, 1.0).unwrap();
    let rules = RuleBook::new();
    let world = build_world(
        game,
        3,
        (),
        InitialActions::PerAgent(vec![0, 1, 1]),
        &rules,
        &Assignment::Ordered(vec![AssignGroup {
            kind: AssignKind::Held,
            count: 3,
        }]),
        1,
        1,
        101,
    )
    .unwrap();
    (world, rules)
}

#[test]
fn counting_play_is_a_separate_path_from_user_defined_outcome_rules() {
    let (world, rules) = public_goods_world();
    let mut custom = assemble_with(
        world,
        socsim_games::WholePopulation::new(),
        rules,
        Pairwise::new(CustomOutcome),
        NoDynamics,
        PlayOptions::new().materialize(true),
        101,
    )
    .unwrap()
    .build();
    custom.step().unwrap();
    assert_eq!(custom.world().payoffs(), &[99.0, 99.0, 99.0]);

    let (world, rules) = public_goods_world();
    let mut counting = assemble_with(
        world,
        socsim_games::WholePopulation::new(),
        rules,
        counting_play(),
        NoDynamics,
        PlayOptions::new().materialize(true),
        101,
    )
    .unwrap()
    .build();
    counting.step().unwrap();
    assert_ne!(counting.world().payoffs(), custom.world().payoffs());
}

fn held_public_goods_world(
    rules: &RuleBook<socsim_games::catalog::PublicGoodsFinite, ()>,
    assignment: Assignment,
) -> socsim_games::GameWorld<socsim_games::catalog::PublicGoodsFinite> {
    build_world(
        socsim_games::catalog::PublicGoodsFinite::new(3, 2.0, 1.0).unwrap(),
        3,
        (),
        InitialActions::PerAgent(vec![0, 1, 1]),
        rules,
        &assignment,
        1,
        1,
        103,
    )
    .unwrap()
}

fn all_held(n_agents: usize) -> Assignment {
    Assignment::Ordered(vec![AssignGroup {
        kind: AssignKind::Held,
        count: n_agents,
    }])
}

fn arena_len_after_one_step<G: socsim_games::StageGame>(
    builder: Result<socsim_engine::SimulationBuilder<socsim_games::GameWorld<G>>, GameError>,
) -> usize {
    let mut simulation = builder.unwrap().build();
    simulation.step().unwrap();
    simulation.world().plays().n_encounters()
}

#[test]
fn assemble_selects_the_implicit_path_unless_materialization_is_requested() {
    let run = |options: PlayOptions<usize>| {
        let rules = RuleBook::new();
        arena_len_after_one_step(assemble(
            held_public_goods_world(&rules, all_held(3)),
            socsim_games::WholePopulation::new(),
            rules,
            NormalFormOutcome,
            options,
            103,
        ))
    };
    assert_eq!(run(PlayOptions::new()), 0);
    assert_eq!(run(PlayOptions::new().materialize(false)), 0);
    assert_eq!(run(PlayOptions::new().materialize(true)), 1);
}

#[test]
fn assemble_with_aggregates_honours_the_materialize_option() {
    let run = |materialize: bool| {
        let game = socsim_games::catalog::MinorityGame::new(3).unwrap();
        let rules = RuleBook::new();
        let world = build_world(
            game,
            3,
            (),
            InitialActions::PerAgent(vec![0, 1, 1]),
            &rules,
            &all_held(3),
            1,
            1,
            107,
        )
        .unwrap();
        arena_len_after_one_step(socsim_games::assemble_with_aggregates(
            world,
            socsim_games::WholePopulation::new(),
            rules,
            NormalFormOutcome,
            PlayOptions::new().materialize(materialize),
            107,
            2,
        ))
    };
    assert_eq!(run(false), 0);
    assert_eq!(run(true), 1);
}

#[test]
fn an_active_rule_forces_the_materialized_path() {
    let mut rules = RuleBook::new();
    rules.register("fixed", Arc::new(Fixed::new(0))).unwrap();
    let world = held_public_goods_world(
        &rules,
        Assignment::Ordered(vec![
            AssignGroup {
                kind: AssignKind::Rule("fixed".to_owned()),
                count: 1,
            },
            AssignGroup {
                kind: AssignKind::Held,
                count: 2,
            },
        ]),
    );
    assert_eq!(
        arena_len_after_one_step(assemble(
            world,
            socsim_games::WholePopulation::new(),
            rules,
            NormalFormOutcome,
            PlayOptions::new(),
            103,
        )),
        1
    );
}

#[test]
fn a_user_defined_outcome_rule_forces_the_materialized_path() {
    let rules = RuleBook::new();
    let world = held_public_goods_world(&rules, all_held(3));
    let mut simulation = assemble(
        world,
        socsim_games::WholePopulation::new(),
        rules,
        CustomOutcome,
        PlayOptions::new(),
        103,
    )
    .unwrap()
    .build();
    simulation.step().unwrap();
    assert_eq!(simulation.world().plays().n_encounters(), 1);
    assert_eq!(simulation.world().payoffs(), &[99.0, 99.0, 99.0]);
}

#[test]
fn a_matching_without_an_implicit_pattern_forces_the_materialized_path() {
    let rules = RuleBook::new();
    let world = build_world(
        game(),
        2,
        (),
        InitialActions::Same(0),
        &rules,
        &all_held(2),
        1,
        1,
        109,
    )
    .unwrap();
    assert_eq!(
        arena_len_after_one_step(assemble(
            world,
            socsim_games::FixedPlayers::pairs(&[(0, 1)]),
            rules,
            NormalFormOutcome,
            PlayOptions::new(),
            109,
        )),
        1
    );
}

#[test]
fn finer_decision_scopes_force_the_materialized_path_even_when_all_agents_are_held() {
    for scope in [DecisionScope::PerAgentRole, DecisionScope::PerEncounter] {
        let rules = RuleBook::new();
        let world = held_public_goods_world(&rules, all_held(3));
        let mut simulation = assemble(
            world,
            socsim_games::WholePopulation::new(),
            rules,
            NormalFormOutcome,
            PlayOptions::new().decision_scope(scope),
            103,
        )
        .unwrap()
        .build();
        simulation.step().unwrap();
        assert_eq!(simulation.world().plays().n_encounters(), 1, "{scope:?}");
        // Held agents carry no per-step action under these scopes.
        let record = simulation
            .world()
            .history(AgentId(0))
            .unwrap()
            .last()
            .unwrap()
            .clone();
        assert_eq!(record.action, None, "{scope:?}");
        assert_eq!(record.n_played, 1, "{scope:?}");
    }
}

fn hawk_dove_world(
    n_agents: usize,
    assignment: Assignment,
    rules: &RuleBook<socsim_games::catalog::HawkDove, ()>,
) -> socsim_games::GameWorld<socsim_games::catalog::HawkDove> {
    build_world(
        socsim_games::catalog::HawkDove::new(2.0, 4.0).unwrap(),
        n_agents,
        (),
        InitialActions::Same(1),
        rules,
        &assignment,
        1,
        4,
        211,
    )
    .unwrap()
}

fn assemble_hawk_dove<D>(
    dynamics: D,
    opts: PlayOptions<usize>,
) -> Result<
    socsim_engine::SimulationBuilder<socsim_games::GameWorld<socsim_games::catalog::HawkDove>>,
    GameError,
>
where
    D: socsim_games::PopulationDynamics<socsim_games::catalog::HawkDove, ()>,
{
    let rules = RuleBook::new();
    assemble_with(
        hawk_dove_world(10, all_held(10), &rules),
        socsim_games::RandomPairs::new(),
        rules,
        Pairwise::new(NormalFormOutcome),
        dynamics,
        opts,
        211,
    )
}

fn is_population_error<T>(result: Result<T, GameError>) -> bool {
    matches!(result, Err(GameError::Population(_)))
}

#[test]
fn population_dynamics_reject_any_agent_with_an_active_rule() {
    let mut rules = RuleBook::new();
    rules.register("fixed", Arc::new(Fixed::new(0))).unwrap();
    let assignment = Assignment::Ordered(vec![
        AssignGroup {
            kind: AssignKind::Held,
            count: 9,
        },
        AssignGroup {
            kind: AssignKind::Rule("fixed".to_owned()),
            count: 1,
        },
    ]);
    let build = |dynamics: &dyn Fn(
        socsim_games::GameWorld<socsim_games::catalog::HawkDove>,
        RuleBook<socsim_games::catalog::HawkDove, ()>,
    ) -> bool| {
        dynamics(
            hawk_dove_world(10, assignment.clone(), &rules),
            rules.clone(),
        )
    };
    let update = socsim_games::Update::Synchronous;
    assert!(build(&|world, rules| is_population_error(assemble_with(
        world,
        socsim_games::RandomPairs::new(),
        rules,
        Pairwise::new(NormalFormOutcome),
        socsim_games::FermiUpdate::new(1.0, update).unwrap(),
        PlayOptions::new(),
        211,
    ))));
    assert!(build(&|world, rules| is_population_error(assemble_with(
        world,
        socsim_games::RandomPairs::new(),
        rules,
        Pairwise::new(NormalFormOutcome),
        socsim_games::ProportionalImitation::new(0.25, update).unwrap(),
        PlayOptions::new(),
        211,
    ))));
    assert!(build(&|world, rules| is_population_error(assemble_with(
        world,
        socsim_games::RandomPairs::new(),
        rules,
        Pairwise::new(NormalFormOutcome),
        socsim_games::MoranUpdate::new(socsim_games::Fitness::Exponential { w: 1.0 }, 0.0, update)
            .unwrap(),
        PlayOptions::new(),
        211,
    ))));
    // The same world without the active agent is accepted.
    assert!(assemble_hawk_dove(
        socsim_games::FermiUpdate::new(1.0, update).unwrap(),
        PlayOptions::new()
    )
    .is_ok());
}

#[test]
fn proportional_imitation_rejects_beta_above_the_inverse_payoff_range() {
    let update = socsim_games::Update::Synchronous;
    // Hawk-dove with V = 2 and C = 4 has payoffs in [-1, 2]; RandomPairs with
    // an even population gives every agent exactly one encounter.
    assert!(assemble_hawk_dove(
        socsim_games::ProportionalImitation::new(1.0 / 3.0, update).unwrap(),
        PlayOptions::new(),
    )
    .is_ok());
    assert!(is_population_error(assemble_hawk_dove(
        socsim_games::ProportionalImitation::new(0.34, update).unwrap(),
        PlayOptions::new(),
    )));
    assert!(matches!(
        socsim_games::ProportionalImitation::new(0.0, update),
        Err(GameError::Population(_))
    ));
    assert!(matches!(
        socsim_games::ProportionalImitation::new(f64::NAN, update),
        Err(GameError::Population(_))
    ));
}

#[test]
fn proportional_imitation_includes_idle_agents_and_requires_bounded_encounters() {
    let update = socsim_games::Update::Synchronous;
    let rules = RuleBook::new();
    // With an odd population one agent idles with payoff zero, so the range
    // stays [-1, 2]; beta = 1/3 is still accepted.
    assert!(assemble_with(
        hawk_dove_world(9, all_held(9), &rules),
        socsim_games::RandomPairs::new(),
        rules.clone(),
        Pairwise::new(NormalFormOutcome),
        socsim_games::ProportionalImitation::new(1.0 / 3.0, update).unwrap(),
        PlayOptions::new(),
        211,
    )
    .is_ok());

    // A matching that reports no encounter counts cannot bound summed payoffs.
    let fixed = || socsim_games::FixedPlayers::pairs(&[(0, 1), (2, 3)]);
    assert!(is_population_error(assemble_with(
        hawk_dove_world(4, all_held(4), &rules),
        fixed(),
        rules.clone(),
        Pairwise::new(NormalFormOutcome),
        socsim_games::ProportionalImitation::new(0.25, update).unwrap(),
        PlayOptions::new(),
        211,
    )));
    // Mean aggregation includes zero for possibly idle agents: [-1, 2].
    assert!(assemble_with(
        hawk_dove_world(4, all_held(4), &rules),
        fixed(),
        rules,
        Pairwise::new(NormalFormOutcome),
        socsim_games::ProportionalImitation::new(1.0 / 3.0, update).unwrap(),
        PlayOptions::new().payoff_aggregation(socsim_games::PayoffAggregation::Mean),
        211,
    )
    .is_ok());
}

#[test]
fn payoff_bounded_dynamics_reject_user_defined_outcome_rules_and_scan_counting_payoffs() {
    let update = socsim_games::Update::Synchronous;
    let (world, rules) = public_goods_world();
    assert!(is_population_error(assemble_with(
        world,
        socsim_games::WholePopulation::new(),
        rules,
        Pairwise::new(CustomOutcome),
        socsim_games::ProportionalImitation::new(0.01, update).unwrap(),
        PlayOptions::new(),
        101,
    )));

    // Public goods with k = 3, r = 2, cost = 1 has payoffs in [-1/3, 4/3].
    let (world, rules) = public_goods_world();
    assert!(assemble_with(
        world,
        socsim_games::WholePopulation::new(),
        rules,
        counting_play(),
        socsim_games::ProportionalImitation::new(0.6, update).unwrap(),
        PlayOptions::new(),
        101,
    )
    .is_ok());
    let (world, rules) = public_goods_world();
    assert!(is_population_error(assemble_with(
        world,
        socsim_games::WholePopulation::new(),
        rules,
        counting_play(),
        socsim_games::ProportionalImitation::new(0.61, update).unwrap(),
        PlayOptions::new(),
        101,
    )));
}

#[test]
fn moran_rejects_asynchronous_update_without_pointing_to_assemble_async() {
    let moran = socsim_games::MoranUpdate::new(
        socsim_games::Fitness::Exponential { w: 1.0 },
        0.0,
        socsim_games::Update::Asynchronous,
    )
    .unwrap();
    match assemble_hawk_dove(moran, PlayOptions::new()) {
        Err(GameError::Population(message)) => assert_eq!(
            message,
            "moran_update supports only Update::Synchronous; it has no asynchronous form"
        ),
        other => panic!("expected a population error, got {:?}", other.err()),
    }
}

#[test]
fn linear_moran_fitness_must_be_positive_at_the_lowest_attainable_payoff() {
    let update = socsim_games::Update::Synchronous;
    let moran = |w| {
        socsim_games::MoranUpdate::new(socsim_games::Fitness::Linear { w }, 0.0, update).unwrap()
    };
    // The lowest hawk-dove payoff is -1: 1 - w + w * (-1) = 1 - 2w.
    assert!(is_population_error(assemble_hawk_dove(
        moran(1.0),
        PlayOptions::new()
    )));
    assert!(is_population_error(assemble_hawk_dove(
        moran(0.5),
        PlayOptions::new()
    )));
    assert!(assemble_hawk_dove(moran(0.4), PlayOptions::new()).is_ok());
    assert!(assemble_hawk_dove(
        socsim_games::MoranUpdate::new(socsim_games::Fitness::Exponential { w: 5.0 }, 0.0, update)
            .unwrap(),
        PlayOptions::new()
    )
    .is_ok());

    for (fitness, mutation) in [
        (socsim_games::Fitness::Linear { w: -0.1 }, 0.0),
        (socsim_games::Fitness::Linear { w: 1.1 }, 0.0),
        (socsim_games::Fitness::Exponential { w: f64::NAN }, 0.0),
        (socsim_games::Fitness::Linear { w: 0.5 }, -0.1),
        (socsim_games::Fitness::Linear { w: 0.5 }, 1.5),
    ] {
        assert!(matches!(
            socsim_games::MoranUpdate::new(fitness, mutation, update),
            Err(GameError::Population(_))
        ));
    }
}

#[test]
fn copy_dynamics_reject_games_whose_roles_have_different_action_counts() {
    let update = socsim_games::Update::Synchronous;
    // MatrixGame declares CommonActionSpace unconditionally; assembly checks
    // the action counts at run time.
    let game = MatrixGame::bimatrix(
        &[vec![1.0, 0.0, 2.0], vec![0.0, 1.0, 0.0]],
        &[vec![1.0, 0.0, 2.0], vec![0.0, 1.0, 0.0]],
    )
    .unwrap();
    let rules = RuleBook::new();
    let world = build_world(
        game,
        4,
        (),
        InitialActions::Same(0),
        &rules,
        &all_held(4),
        1,
        1,
        7,
    )
    .unwrap();
    assert!(is_population_error(assemble_with(
        world,
        socsim_games::RandomPairs::new(),
        rules,
        Pairwise::new(NormalFormOutcome),
        socsim_games::FermiUpdate::new(1.0, update).unwrap(),
        PlayOptions::new(),
        7,
    )));

    let rectangular = MatrixGame::bimatrix(&[vec![1.0, 0.0, 2.0]], &[vec![1.0, 0.0, 2.0]]).unwrap();
    assert!(matches!(
        socsim_games::GameMetrics::new(
            &rectangular,
            socsim_games::MetricsOptions::new().population_share(true)
        ),
        Err(GameError::Metrics(_))
    ));
    assert!(
        socsim_games::GameMetrics::new(&rectangular, socsim_games::MetricsOptions::new()).is_ok()
    );
}

#[test]
fn fermi_update_requires_a_finite_positive_noise() {
    let update = socsim_games::Update::Synchronous;
    for k in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(matches!(
            socsim_games::FermiUpdate::new(k, update),
            Err(GameError::Population(_))
        ));
    }
    assert!(socsim_games::FermiUpdate::new(0.1, update).is_ok());
}

#[test]
fn population_dynamics_take_the_update_schedule_as_a_required_argument() {
    // The update schedule has no default: every constructor takes it
    // explicitly, so an unspecified schedule is a compile-time error.
    let update = socsim_games::Update::Synchronous;
    assert_eq!(socsim_games::ImitateBest::new(update).update(), update);
    assert_eq!(
        socsim_games::FermiUpdate::new(1.0, update)
            .unwrap()
            .update(),
        update
    );
    assert_eq!(
        socsim_games::ProportionalImitation::new(0.1, update)
            .unwrap()
            .update(),
        update
    );
    assert_eq!(
        socsim_games::MoranUpdate::new(socsim_games::Fitness::Linear { w: 0.5 }, 0.0, update)
            .unwrap()
            .update(),
        update
    );
}

#[test]
fn imitate_best_requires_a_topology() {
    assert!(is_population_error(assemble_hawk_dove(
        socsim_games::ImitateBest::new(socsim_games::Update::Synchronous),
        PlayOptions::new()
    )));
}

#[cfg(feature = "grid")]
#[test]
fn moran_update_rejects_a_world_with_topology() {
    let rules = RuleBook::new();
    let world = hawk_dove_world(9, all_held(9), &rules)
        .with_topology(
            socsim_games::Topology::grid(socsim_games::GridSpec {
                rows: 3,
                cols: 3,
                boundary: socsim_games::GridBoundary::Toroidal,
                neighborhood: socsim_games::GridNeighbors::Moore,
            })
            .unwrap(),
        )
        .unwrap();
    assert!(is_population_error(assemble_with(
        world,
        socsim_games::RandomPairs::new(),
        rules,
        Pairwise::new(NormalFormOutcome),
        socsim_games::MoranUpdate::new(
            socsim_games::Fitness::Exponential { w: 1.0 },
            0.0,
            socsim_games::Update::Synchronous
        )
        .unwrap(),
        PlayOptions::new(),
        211,
    )));
}

// Continuous built-in games.

use socsim_games::catalog::CatalogError;
use socsim_games::continuous::{BertrandDifferentiated, Cournot, PublicGoodsContinuous};

fn is_condition<T: std::fmt::Debug>(result: Result<T, CatalogError>) -> bool {
    matches!(result, Err(CatalogError::Condition(_)))
}

fn is_non_finite<T: std::fmt::Debug>(result: Result<T, CatalogError>) -> bool {
    matches!(result, Err(CatalogError::NonFinite))
}

#[test]
fn cournot_constructor_rejects_invalid_parameters() {
    assert!(Cournot::new(2, 100.0, 1.0, 10.0, 100.0).is_ok());
    assert!(Cournot::new(2, 100.0, 1.0, 0.0, 100.0).is_ok());
    assert!(is_condition(Cournot::new(2, 100.0, 0.0, 10.0, 100.0)));
    assert!(is_condition(Cournot::new(2, 100.0, -1.0, 10.0, 100.0)));
    assert!(is_condition(Cournot::new(2, 10.0, 1.0, 10.0, 100.0)));
    assert!(is_condition(Cournot::new(2, 5.0, 1.0, 10.0, 100.0)));
    assert!(is_condition(Cournot::new(2, 100.0, 1.0, -1.0, 100.0)));
    assert!(is_condition(Cournot::new(2, 100.0, 1.0, 10.0, 0.0)));
    assert!(is_condition(Cournot::new(2, 100.0, 1.0, 10.0, -1.0)));
    assert!(is_condition(Cournot::new(1, 100.0, 1.0, 10.0, 100.0)));
    assert!(is_condition(Cournot::new(0, 100.0, 1.0, 10.0, 100.0)));
    assert!(is_non_finite(Cournot::new(2, f64::NAN, 1.0, 10.0, 100.0)));
    assert!(is_non_finite(Cournot::new(
        2,
        100.0,
        f64::INFINITY,
        10.0,
        100.0
    )));
    assert!(is_non_finite(Cournot::new(2, 100.0, 1.0, f64::NAN, 100.0)));
    assert!(is_non_finite(Cournot::new(
        2,
        100.0,
        1.0,
        10.0,
        f64::INFINITY
    )));
}

#[test]
fn bertrand_constructor_rejects_invalid_parameters() {
    assert!(BertrandDifferentiated::new(3, 10.0, 2.0, 1.0, 1.0, 100.0).is_ok());
    assert!(BertrandDifferentiated::new(3, 10.0, 2.0, 0.0, 0.0, 100.0).is_ok());
    assert!(is_condition(BertrandDifferentiated::new(
        3, 10.0, 0.0, 1.0, 1.0, 100.0
    )));
    assert!(is_condition(BertrandDifferentiated::new(
        3, 10.0, -2.0, 1.0, 1.0, 100.0
    )));
    assert!(is_condition(BertrandDifferentiated::new(
        3, 10.0, 2.0, -0.5, 1.0, 100.0
    )));
    // 2b = (n - 1) d is rejected; 2b > (n - 1) d is required.
    assert!(is_condition(BertrandDifferentiated::new(
        3, 10.0, 2.0, 2.0, 1.0, 100.0
    )));
    assert!(is_condition(BertrandDifferentiated::new(
        5, 10.0, 2.0, 1.5, 1.0, 100.0
    )));
    assert!(is_condition(BertrandDifferentiated::new(
        3, 10.0, 2.0, 1.0, -1.0, 100.0
    )));
    assert!(is_condition(BertrandDifferentiated::new(
        3, 10.0, 2.0, 1.0, 1.0, 0.0
    )));
    assert!(is_condition(BertrandDifferentiated::new(
        1, 10.0, 2.0, 1.0, 1.0, 100.0
    )));
    assert!(is_non_finite(BertrandDifferentiated::new(
        3,
        f64::NAN,
        2.0,
        1.0,
        1.0,
        100.0
    )));
    assert!(is_non_finite(BertrandDifferentiated::new(
        3,
        10.0,
        2.0,
        f64::INFINITY,
        1.0,
        100.0
    )));
    assert!(is_non_finite(BertrandDifferentiated::new(
        3,
        10.0,
        2.0,
        1.0,
        1.0,
        f64::NEG_INFINITY
    )));
}

#[test]
fn public_goods_constructors_reject_invalid_parameters() {
    assert!(PublicGoodsContinuous::new(4, 10.0, 0.0).is_ok());
    assert!(PublicGoodsContinuous::new(4, 10.0, 6.0).is_ok());
    assert!(is_condition(PublicGoodsContinuous::new(1, 10.0, 2.0)));
    assert!(is_condition(PublicGoodsContinuous::new(4, 0.0, 2.0)));
    assert!(is_condition(PublicGoodsContinuous::new(4, -1.0, 2.0)));
    assert!(is_condition(PublicGoodsContinuous::new(4, 10.0, -0.5)));
    assert!(is_non_finite(PublicGoodsContinuous::new(4, f64::NAN, 2.0)));
    assert!(is_non_finite(PublicGoodsContinuous::new(
        4,
        10.0,
        f64::INFINITY
    )));

    assert!(PublicGoodsContinuous::social_dilemma(4, 10.0, 2.0).is_ok());
    for r in [0.5, 1.0, 4.0, 6.0] {
        assert!(
            is_condition(PublicGoodsContinuous::social_dilemma(4, 10.0, r)),
            "r = {r}"
        );
    }
    assert!(is_condition(PublicGoodsContinuous::social_dilemma(
        1, 10.0, 2.0
    )));
}

#[test]
fn deserialization_cannot_bypass_the_continuous_game_checks() {
    let game = Cournot::new(3, 100.0, 1.1, 10.0, 100.0).unwrap();
    let json = serde_json::to_string(&game).unwrap();
    assert_eq!(serde_json::from_str::<Cournot>(&json).unwrap(), game);
    assert!(serde_json::from_str::<Cournot>(
        r#"{"n": 3, "a": 100.0, "b": -1.0, "c": 10.0, "q_max": 100.0}"#
    )
    .is_err());
    assert!(serde_json::from_str::<Cournot>(
        r#"{"n": 1, "a": 100.0, "b": 1.0, "c": 10.0, "q_max": 100.0}"#
    )
    .is_err());

    let game = BertrandDifferentiated::new(3, 10.0, 2.0, 1.0, 1.0, 100.0).unwrap();
    let json = serde_json::to_string(&game).unwrap();
    assert_eq!(
        serde_json::from_str::<BertrandDifferentiated>(&json).unwrap(),
        game
    );
    assert!(serde_json::from_str::<BertrandDifferentiated>(
        r#"{"n": 3, "a": 10.0, "b": 2.0, "d": 2.0, "c": 1.0, "p_max": 100.0}"#
    )
    .is_err());

    let game = PublicGoodsContinuous::new(4, 10.0, 2.5).unwrap();
    let json = serde_json::to_string(&game).unwrap();
    assert_eq!(
        serde_json::from_str::<PublicGoodsContinuous>(&json).unwrap(),
        game
    );
    assert!(serde_json::from_str::<PublicGoodsContinuous>(
        r#"{"k": 4, "endowment": 0.0, "r": 2.0}"#
    )
    .is_err());
    assert!(serde_json::from_str::<PublicGoodsContinuous>(
        r#"{"k": 1, "endowment": 10.0, "r": 2.0}"#
    )
    .is_err());
}

// Continuous-game rules.

use socsim_games::{
    check_continuous_action, ActionError, BestResponseOracle, BrResult, BrStatus, ContinuousGame,
    DampedBestResponse, GradientAscent, NormalFormGame, NumericBr, StageGame,
};

#[test]
fn continuous_rule_constructors_reject_invalid_parameters() {
    assert!(DampedBestResponse::<Cournot>::new(1.0).is_ok());
    assert!(DampedBestResponse::<Cournot>::new(0.5).is_ok());
    for alpha in [0.0, -0.5, 1.5, f64::NAN, f64::INFINITY] {
        assert!(
            matches!(
                DampedBestResponse::<Cournot>::new(alpha),
                Err(RuleError::Unsupported(_))
            ),
            "alpha = {alpha}"
        );
    }
    assert!(GradientAscent::new(0.3).is_ok());
    for step in [0.0, -0.1, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(
            matches!(GradientAscent::new(step), Err(RuleError::Unsupported(_))),
            "step = {step}"
        );
    }
}

/// A continuous game with configurable dimension, bounds, and numeric
/// best-response declaration, and no analytic best response.
struct NoBestResponse {
    bounds: Vec<(f64, f64)>,
    numeric: NumericBr,
}

impl StageGame for NoBestResponse {
    type Action = Vec<f64>;

    fn n_roles(&self) -> usize {
        2
    }

    fn check_action(&self, role: usize, action: &Vec<f64>) -> Result<(), ActionError> {
        check_continuous_action(self, role, action)
    }
}

impl NormalFormGame for NoBestResponse {
    fn payoff(&self, role: usize, profile: &[Vec<f64>]) -> f64 {
        -profile[role].iter().map(|x| x * x).sum::<f64>()
    }
}

impl ContinuousGame for NoBestResponse {
    fn bounds(&self, _role: usize) -> &[(f64, f64)] {
        &self.bounds
    }

    fn numeric_br(&self) -> NumericBr {
        self.numeric
    }
}

struct ZeroOracle;

impl BestResponseOracle<NoBestResponse> for ZeroOracle {
    fn best_response(&self, game: &NoBestResponse, role: usize, _profile: &[Vec<f64>]) -> BrResult {
        BrResult {
            action: vec![0.0; game.bounds(role).len()],
            residual: 0.0,
            status: BrStatus::Converged,
        }
    }
}

fn build_continuous<G: ContinuousGame>(
    game: G,
    rule: Arc<dyn socsim_games::DecisionRule<G, ()>>,
    initial: Vec<f64>,
) -> Result<socsim_games::GameWorld<G>, GameError> {
    let mut rules = RuleBook::new();
    rules.register("rule", rule).unwrap();
    build_world(
        game,
        2,
        (),
        InitialActions::Same(initial),
        &rules,
        &Assignment::Ordered(vec![AssignGroup {
            kind: AssignKind::Rule("rule".to_owned()),
            count: 2,
        }]),
        2,
        1,
        7,
    )
}

#[test]
fn damped_best_response_without_a_best_response_path_is_rejected_at_build_time() {
    let cases = [
        // No analytic best response and no numerical search.
        (vec![(0.0, 1.0)], NumericBr::Unavailable),
        // The numerical search is one-dimensional only.
        (vec![(0.0, 1.0), (0.0, 1.0)], NumericBr::Unimodal1D),
    ];
    for (bounds, numeric) in cases {
        let initial = vec![0.5; bounds.len()];
        let game = NoBestResponse {
            bounds: bounds.clone(),
            numeric,
        };
        let rule = Arc::new(DampedBestResponse::<NoBestResponse>::new(0.5).unwrap());
        assert!(
            matches!(
                build_continuous(game, rule, initial.clone()),
                Err(GameError::Rule(RuleError::Unsupported(_)))
            ),
            "{bounds:?} {numeric:?}"
        );

        // An oracle supplies the missing path.
        let game = NoBestResponse { bounds, numeric };
        let rule = Arc::new(
            DampedBestResponse::<NoBestResponse>::new(0.5)
                .unwrap()
                .with_oracle(Arc::new(ZeroOracle)),
        );
        assert!(build_continuous(game, rule, initial).is_ok());
    }

    // A one-dimensional game declared unimodal can use the numerical search.
    let game = NoBestResponse {
        bounds: vec![(0.0, 1.0)],
        numeric: NumericBr::Unimodal1D,
    };
    let rule = Arc::new(DampedBestResponse::<NoBestResponse>::new(0.5).unwrap());
    assert!(build_continuous(game, rule, vec![0.5]).is_ok());
}

#[test]
fn continuous_rules_reject_invalid_bounds_at_build_time() {
    for bounds in [vec![(0.0, f64::NAN)], vec![(1.0, 1.0)], vec![(1.0, 0.0)]] {
        let game = NoBestResponse {
            bounds: bounds.clone(),
            numeric: NumericBr::Unavailable,
        };
        let initial = socsim_games::RuleInitContext {
            agent: AgentId(0),
            kind: socsim_games::RuleKindId(0),
            generation: 0,
            init_seed: 0,
        };
        let rule = GradientAscent::new(0.1).unwrap();
        assert!(
            matches!(
                <GradientAscent as socsim_games::DecisionRule<NoBestResponse, ()>>::init_state(
                    &rule, &initial, &game
                ),
                Err(RuleError::Unsupported(_))
            ),
            "{bounds:?}"
        );
        let rule = DampedBestResponse::<NoBestResponse>::new(0.5)
            .unwrap()
            .with_oracle(Arc::new(ZeroOracle));
        assert!(
            matches!(
                <DampedBestResponse<NoBestResponse> as socsim_games::DecisionRule<
                    NoBestResponse,
                    (),
                >>::init_state(&rule, &initial, &game),
                Err(RuleError::Unsupported(_))
            ),
            "{bounds:?}"
        );
    }
}

// Continuous metrics.

#[test]
fn continuous_metrics_rejects_invalid_stop_options() {
    use socsim_games::{ContinuousMetrics, ContinuousMetricsOptions};

    let game = Cournot::new(3, 100.0, 1.0, 10.0, 100.0).unwrap();
    assert!(
        ContinuousMetrics::new(&game, ContinuousMetricsOptions::new().stop(1.0e-9, 10)).is_ok()
    );
    for (tol, window) in [
        (0.0, 1),
        (-1.0e-9, 1),
        (f64::NAN, 1),
        (f64::INFINITY, 1),
        (1.0e-9, 0),
    ] {
        assert!(
            matches!(
                ContinuousMetrics::new(&game, ContinuousMetricsOptions::new().stop(tol, window)),
                Err(GameError::Metrics(_))
            ),
            "{tol} {window}"
        );
    }
}

/// Matching pennies on [0, 1] with discontinuous best responses: role 0 wants
/// to match role 1's side of 1/2 and role 1 wants to mismatch, so damped
/// best-response iteration circles the midpoint without converging.
struct ContinuousPennies;

const PENNIES_BOUNDS: [(f64, f64); 1] = [(0.0, 1.0)];

impl StageGame for ContinuousPennies {
    type Action = Vec<f64>;

    fn n_roles(&self) -> usize {
        2
    }

    fn check_action(&self, role: usize, action: &Vec<f64>) -> Result<(), ActionError> {
        check_continuous_action(self, role, action)
    }
}

impl NormalFormGame for ContinuousPennies {
    fn payoff(&self, role: usize, profile: &[Vec<f64>]) -> f64 {
        let target = self.best_response(role, profile).unwrap()[0];
        -(profile[role][0] - target).abs()
    }
}

impl ContinuousGame for ContinuousPennies {
    fn bounds(&self, _role: usize) -> &[(f64, f64)] {
        &PENNIES_BOUNDS
    }

    fn best_response(&self, role: usize, profile: &[Vec<f64>]) -> Option<Vec<f64>> {
        let other_high = profile[1 - role][0] >= 0.5;
        let high = if role == 0 { other_high } else { !other_high };
        Some(vec![if high { 1.0 } else { 0.0 }])
    }
}

#[test]
fn continuous_metrics_rejects_an_equilibrium_that_does_not_converge() {
    use socsim_games::{ContinuousEquilibrium, ContinuousMetrics, ContinuousMetricsOptions};

    let solve = || ContinuousMetricsOptions::new().equilibrium(ContinuousEquilibrium::Solve);
    let game = Cournot::new(3, 100.0, 1.0, 10.0, 100.0).unwrap();
    assert!(ContinuousMetrics::new(&game, solve()).is_ok());

    // The iteration runs but never reaches the tolerance.
    assert!(matches!(
        ContinuousMetrics::new(&ContinuousPennies, solve()),
        Err(GameError::Metrics(_))
    ));

    // No analytic, oracle, or numerical best response is available.
    let game = NoBestResponse {
        bounds: vec![(0.0, 1.0)],
        numeric: NumericBr::Unavailable,
    };
    assert!(matches!(
        ContinuousMetrics::new(&game, solve()),
        Err(GameError::Metrics(_))
    ));
}

// Population dynamics on continuous games.

fn held_cournot_world(n_agents: usize) -> socsim_games::GameWorld<Cournot> {
    build_world(
        Cournot::new(2, 100.0, 1.0, 10.0, 100.0).unwrap(),
        n_agents,
        (),
        InitialActions::Same(vec![20.0]),
        &RuleBook::new(),
        &all_held(n_agents),
        1,
        4,
        223,
    )
    .unwrap()
}

fn assemble_cournot<D>(
    dynamics: D,
) -> Result<socsim_engine::SimulationBuilder<socsim_games::GameWorld<Cournot>>, GameError>
where
    D: socsim_games::PopulationDynamics<Cournot, ()>,
{
    assemble_with(
        held_cournot_world(6),
        socsim_games::RandomPairs::new(),
        RuleBook::new(),
        Pairwise::new(NormalFormOutcome),
        dynamics,
        PlayOptions::new(),
        223,
    )
}

#[test]
fn gaussian_mutation_requires_a_finite_positive_standard_deviation() {
    let update = socsim_games::Update::Synchronous;
    let moran = || {
        socsim_games::MoranUpdate::new(socsim_games::Fitness::Exponential { w: 0.1 }, 0.2, update)
            .unwrap()
    };
    for sd in [0.0, -0.5, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(
            matches!(moran().gaussian_mutation(sd), Err(GameError::Population(_))),
            "{sd}"
        );
    }
    assert_eq!(
        moran().gaussian_mutation(0.5).unwrap().gaussian_sd(),
        Some(0.5)
    );
    assert_eq!(moran().gaussian_sd(), None);
}

#[test]
fn continuous_moran_requires_a_standard_deviation_when_it_mutates() {
    let update = socsim_games::Update::Synchronous;
    let exponential = socsim_games::Fitness::Exponential { w: 0.1 };
    // A positive mutation probability without a standard deviation.
    assert!(is_population_error(assemble_cournot(
        socsim_games::MoranUpdate::new(exponential, 0.2, update).unwrap()
    )));
    // With a standard deviation, or without mutation, assembly succeeds.
    assert!(assemble_cournot(
        socsim_games::MoranUpdate::new(exponential, 0.2, update)
            .unwrap()
            .gaussian_mutation(0.5)
            .unwrap()
    )
    .is_ok());
    assert!(
        assemble_cournot(socsim_games::MoranUpdate::new(exponential, 0.0, update).unwrap()).is_ok()
    );
    assert!(assemble_cournot(
        socsim_games::MoranUpdate::new(exponential, 0.0, update)
            .unwrap()
            .gaussian_mutation(0.5)
            .unwrap()
    )
    .is_ok());
}

#[test]
fn finite_moran_rejects_a_gaussian_standard_deviation() {
    let update = socsim_games::Update::Synchronous;
    for mutation in [0.0, 0.2] {
        assert!(is_population_error(assemble_hawk_dove(
            socsim_games::MoranUpdate::new(
                socsim_games::Fitness::Exponential { w: 0.1 },
                mutation,
                update
            )
            .unwrap()
            .gaussian_mutation(0.5)
            .unwrap(),
            PlayOptions::new()
        )));
    }
}

#[test]
fn continuous_moran_rejects_linear_fitness_and_accepts_exponential_fitness() {
    let update = socsim_games::Update::Synchronous;
    for w in [0.0, 0.1, 1.0] {
        assert!(
            is_population_error(assemble_cournot(
                socsim_games::MoranUpdate::new(socsim_games::Fitness::Linear { w }, 0.0, update)
                    .unwrap()
            )),
            "{w}"
        );
    }
    assert!(assemble_cournot(
        socsim_games::MoranUpdate::new(socsim_games::Fitness::Exponential { w: 0.1 }, 0.0, update)
            .unwrap()
    )
    .is_ok());
    // Fermi updating needs no payoff range.
    assert!(assemble_cournot(socsim_games::FermiUpdate::new(1.0, update).unwrap()).is_ok());
}

/// A two-role continuous game whose roles may have different bounds. It
/// declares a common action space, which assembly checks at run time, and it
/// accepts every action so that the world can be built.
struct RoleBounds {
    role0: Vec<(f64, f64)>,
    role1: Vec<(f64, f64)>,
}

impl StageGame for RoleBounds {
    type Action = Vec<f64>;

    fn n_roles(&self) -> usize {
        2
    }

    fn check_action(&self, _role: usize, _action: &Vec<f64>) -> Result<(), ActionError> {
        Ok(())
    }
}

impl NormalFormGame for RoleBounds {
    fn payoff(&self, role: usize, profile: &[Vec<f64>]) -> f64 {
        profile[role].iter().sum()
    }
}

impl ContinuousGame for RoleBounds {
    fn bounds(&self, role: usize) -> &[(f64, f64)] {
        if role == 0 {
            &self.role0
        } else {
            &self.role1
        }
    }
}

impl socsim_games::CommonActionSpace for RoleBounds {}

fn role_bounds_world(game: RoleBounds) -> socsim_games::GameWorld<RoleBounds> {
    build_world(
        game,
        4,
        (),
        InitialActions::Same(vec![0.5]),
        &RuleBook::new(),
        &all_held(4),
        1,
        4,
        227,
    )
    .unwrap()
}

#[test]
fn copy_dynamics_reject_continuous_games_whose_roles_have_different_bounds() {
    let update = socsim_games::Update::Synchronous;
    let cases = [
        // Different dimensions.
        (vec![(0.0, 1.0)], vec![(0.0, 1.0), (0.0, 1.0)]),
        // Different lower end point.
        (vec![(0.0, 1.0)], vec![(0.25, 1.0)]),
        // Different upper end point.
        (vec![(0.0, 1.0)], vec![(0.0, 2.0)]),
    ];
    for (role0, role1) in cases {
        let game = || RoleBounds {
            role0: role0.clone(),
            role1: role1.clone(),
        };
        let fermi = assemble_with(
            role_bounds_world(game()),
            socsim_games::RandomPairs::new(),
            RuleBook::new(),
            Pairwise::new(NormalFormOutcome),
            socsim_games::FermiUpdate::new(1.0, update).unwrap(),
            PlayOptions::new(),
            227,
        );
        assert!(is_population_error(fermi), "{role0:?} {role1:?}");
        let moran = assemble_with(
            role_bounds_world(game()),
            socsim_games::RandomPairs::new(),
            RuleBook::new(),
            Pairwise::new(NormalFormOutcome),
            socsim_games::MoranUpdate::new(
                socsim_games::Fitness::Exponential { w: 0.1 },
                0.0,
                update,
            )
            .unwrap(),
            PlayOptions::new(),
            227,
        );
        assert!(is_population_error(moran), "{role0:?} {role1:?}");
    }

    // Identical bounds pass the check.
    let same = RoleBounds {
        role0: vec![(0.0, 1.0)],
        role1: vec![(0.0, 1.0)],
    };
    assert!(assemble_with(
        role_bounds_world(same),
        socsim_games::RandomPairs::new(),
        RuleBook::new(),
        Pairwise::new(NormalFormOutcome),
        socsim_games::FermiUpdate::new(1.0, update).unwrap(),
        PlayOptions::new(),
        227,
    )
    .is_ok());

    // Invalid bounds are rejected before they are compared.
    let invalid = RoleBounds {
        role0: vec![(1.0, 1.0)],
        role1: vec![(1.0, 1.0)],
    };
    assert!(is_population_error(assemble_with(
        role_bounds_world(invalid),
        socsim_games::RandomPairs::new(),
        RuleBook::new(),
        Pairwise::new(NormalFormOutcome),
        socsim_games::FermiUpdate::new(1.0, update).unwrap(),
        PlayOptions::new(),
        227,
    )));
}

#[cfg(feature = "grid")]
#[test]
fn imitate_best_accepts_a_continuous_game_on_a_grid_and_checks_its_bounds() {
    let update = socsim_games::Update::Synchronous;
    let grid = || {
        socsim_games::Topology::grid(socsim_games::GridSpec {
            rows: 3,
            cols: 3,
            boundary: socsim_games::GridBoundary::Toroidal,
            neighborhood: socsim_games::GridNeighbors::Moore,
        })
        .unwrap()
    };
    let world = build_world(
        PublicGoodsContinuous::new(2, 10.0, 1.5).unwrap(),
        9,
        (),
        InitialActions::Same(vec![5.0]),
        &RuleBook::new(),
        &all_held(9),
        1,
        4,
        229,
    )
    .unwrap()
    .with_topology(grid())
    .unwrap();
    assert!(assemble_with(
        world,
        socsim_games::GridNeighborhood::new(),
        RuleBook::new(),
        Pairwise::new(NormalFormOutcome),
        socsim_games::ImitateBest::new(update),
        PlayOptions::new(),
        229,
    )
    .is_ok());

    let world = build_world(
        RoleBounds {
            role0: vec![(0.0, 1.0)],
            role1: vec![(0.0, 2.0)],
        },
        9,
        (),
        InitialActions::Same(vec![0.5]),
        &RuleBook::new(),
        &all_held(9),
        1,
        4,
        229,
    )
    .unwrap()
    .with_topology(grid())
    .unwrap();
    assert!(is_population_error(assemble_with(
        world,
        socsim_games::GridNeighborhood::new(),
        RuleBook::new(),
        Pairwise::new(NormalFormOutcome),
        socsim_games::ImitateBest::new(update),
        PlayOptions::new(),
        229,
    )));
}

#[cfg(feature = "llm")]
#[test]
fn number_parser_rejects_a_multi_dimensional_continuous_game() {
    use socsim_games::llm::{ActionParser, NumberParser};

    let validate = |game: &NoBestResponse, parser: &NumberParser| {
        <NumberParser as ActionParser<NoBestResponse>>::validate(parser, game)
    };
    let plane = NoBestResponse {
        bounds: vec![(0.0, 1.0), (0.0, 1.0)],
        numeric: NumericBr::Unavailable,
    };
    assert!(matches!(
        validate(&plane, &NumberParser::new("x")),
        Err(RuleError::Unsupported(_))
    ));
    assert!(matches!(
        validate(&plane, &NumberParser::new("x").bounds(0.0, 1.0)),
        Err(RuleError::Unsupported(_))
    ));

    let line = NoBestResponse {
        bounds: vec![(0.0, 1.0)],
        numeric: NumericBr::Unavailable,
    };
    assert!(validate(&line, &NumberParser::new("x")).is_ok());
    assert!(validate(&line, &NumberParser::new("x").bounds(0.25, 0.75)).is_ok());
    for (lo, hi) in [(-0.5, 0.5), (0.5, 1.5), (0.75, 0.25), (f64::NAN, 0.5)] {
        assert!(
            matches!(
                validate(&line, &NumberParser::new("x").bounds(lo, hi)),
                Err(RuleError::Unsupported(_))
            ),
            "[{lo}, {hi}]"
        );
    }
}

fn held_world(
    initial: InitialActions<usize>,
    n_agents: usize,
    seed: u64,
) -> Result<socsim_games::GameWorld<MatrixGame>, GameError> {
    let rules: RuleBook<MatrixGame, ()> = RuleBook::new();
    build_world(
        game(),
        n_agents,
        (),
        initial,
        &rules,
        &Assignment::Ordered(vec![AssignGroup {
            kind: AssignKind::Held,
            count: n_agents,
        }]),
        2,
        1,
        seed,
    )
}

fn action_counts(world: &socsim_games::GameWorld<MatrixGame>) -> [usize; 2] {
    let mut counts = [0; 2];
    for &action in world.held() {
        counts[action] += 1;
    }
    counts
}

#[test]
fn initial_shares_fix_counts_by_the_largest_remainder_method() {
    let world = held_world(InitialActions::Shares(vec![(0, 0.9), (1, 0.1)]), 100, 1).unwrap();
    assert_eq!(action_counts(&world), [90, 10]);
    // Equal remainders of one half: the action listed first takes the agent.
    let world = held_world(InitialActions::Shares(vec![(0, 0.95), (1, 0.05)]), 10, 1).unwrap();
    assert_eq!(action_counts(&world), [10, 0]);
    let world = held_world(InitialActions::Shares(vec![(0, 0.5), (1, 0.5)]), 7, 1).unwrap();
    assert_eq!(action_counts(&world), [4, 3]);
    let world = held_world(InitialActions::Shares(vec![(1, 0.5), (0, 0.5)]), 7, 1).unwrap();
    assert_eq!(action_counts(&world), [3, 4]);
}

#[test]
fn initial_shares_equal_listed_actions_shuffled_with_stream_0_2() {
    use rand::seq::SliceRandom;
    use socsim_core::derive_seed;

    for seed in [0, 1, 42] {
        let shares =
            held_world(InitialActions::Shares(vec![(1, 0.3), (0, 0.7)]), 20, seed).unwrap();
        let mut listed = [vec![1; 6], vec![0; 14]].concat();
        let mut rng = SimRng::from_seed(derive_seed(seed, &[0, 2]));
        listed.shuffle(&mut rng);
        let per_agent = held_world(InitialActions::PerAgent(listed), 20, seed).unwrap();
        assert_eq!(
            serde_json::to_value(&shares).unwrap(),
            serde_json::to_value(&per_agent).unwrap()
        );
    }
}

#[test]
fn initial_shares_do_not_depend_on_the_assignment_order() {
    let mut rules = RuleBook::new();
    rules.register("fixed", Arc::new(Fixed::new(0))).unwrap();
    let groups = vec![
        AssignGroup {
            kind: AssignKind::Rule("fixed".to_owned()),
            count: 4,
        },
        AssignGroup {
            kind: AssignKind::Held,
            count: 6,
        },
    ];
    let build = |assign: Assignment| {
        build_world(
            game(),
            10,
            (),
            InitialActions::Shares(vec![(0, 0.5), (1, 0.5)]),
            &rules,
            &assign,
            2,
            1,
            9,
        )
        .unwrap()
        .held()
        .to_vec()
    };
    assert_eq!(
        build(Assignment::Ordered(groups.clone())),
        build(Assignment::Shuffled(groups))
    );
}

#[test]
fn initial_shares_reject_invalid_shares() {
    let config_error = |shares: Vec<(usize, f64)>| {
        matches!(
            held_world(InitialActions::Shares(shares), 10, 1),
            Err(GameError::Config(_))
        )
    };
    assert!(config_error(vec![]));
    assert!(config_error(vec![(0, 0.5), (0, 0.5)]));
    assert!(config_error(vec![(0, 1.2), (1, -0.2)]));
    assert!(config_error(vec![(0, f64::NAN), (1, 0.5)]));
    assert!(config_error(vec![(0, f64::INFINITY)]));
    assert!(config_error(vec![(0, 0.7), (1, 0.4)]));
    assert!(config_error(vec![(0, 0.5), (1, 0.4)]));
    assert!(matches!(
        held_world(InitialActions::Shares(vec![(0, 0.5), (2, 0.5)]), 10, 1),
        Err(GameError::Action(_))
    ));
    // Within the tolerance of the sum, and a zero share, are accepted.
    assert!(held_world(
        InitialActions::Shares(vec![(0, 0.9), (1, 0.1 + 5e-10)]),
        10,
        1
    )
    .is_ok());
    assert!(held_world(InitialActions::Shares(vec![(0, 1.0), (1, 0.0)]), 10, 1).is_ok());
}

/// The run-time checks of `GameSimFactory::build`. Every check is the one the
/// assembly functions perform; the factory adds only the checks of its own
/// settings (missing required values).
#[cfg(feature = "factory")]
mod factory_guards {
    use socsim_games::catalog::{CatalogError, HawkDove, PrisonersDilemma};
    use socsim_games::continuous::Cournot;
    use socsim_games::{
        AssignGroup, AssignKind, Assignment, DecisionScope, FermiUpdate, FictitiousPlay, Fitness,
        Fixed, GameError, GameSimFactory, InitialActions, MatrixGame, MoranUpdate, PlayOptions,
        ProportionalImitation, RandomPairs, RoundRobin, RuleError, TitForTat, Update,
        WholePopulation,
    };

    fn assert_population_error<T>(result: Result<T, GameError>, reason: &str) {
        match result {
            Err(GameError::Population(message)) => assert!(
                message.contains(reason),
                "expected a population error about {reason:?}, got {message:?}"
            ),
            Err(other) => panic!("expected a population error, got {other:?}"),
            Ok(_) => panic!("expected a population error, got Ok"),
        }
    }

    fn assert_config_error<T>(result: Result<T, GameError>, setting: &str) {
        match result {
            Err(GameError::Config(message)) => assert!(
                message.starts_with(setting),
                "expected a message starting with {setting:?}, got {message:?}"
            ),
            Err(other) => panic!("expected a configuration error, got {other:?}"),
            Ok(_) => panic!("expected a configuration error, got Ok"),
        }
    }

    fn hawk_dove() -> HawkDove {
        HawkDove::new(2.0, 4.0).unwrap()
    }

    fn one_tit_for_tat(n_agents: usize) -> Assignment {
        Assignment::Ordered(vec![
            AssignGroup {
                kind: AssignKind::Held,
                count: n_agents - 1,
            },
            AssignGroup {
                kind: AssignKind::Rule("tit_for_tat".to_owned()),
                count: 1,
            },
        ])
    }

    // (1) Population dynamics with an agent on an active rule.
    #[test]
    fn population_dynamics_reject_a_group_with_an_active_rule() {
        let factory = GameSimFactory::new(hawk_dove())
            .agents(10)
            .matching(RandomPairs::new())
            .rule("tit_for_tat", TitForTat::new(0))
            .assign(one_tit_for_tat(10))
            .dynamics(FermiUpdate::new(1.0, Update::Synchronous).unwrap())
            .history_len(1)
            .t_max(4);
        assert_population_error(factory.build(7), "applies only to held agents");

        let held = GameSimFactory::new(hawk_dove())
            .agents(10)
            .matching(RandomPairs::new())
            .dynamics(FermiUpdate::new(1.0, Update::Synchronous).unwrap())
            .history_len(1)
            .t_max(4);
        assert!(held.build(7).is_ok());
    }

    #[cfg(feature = "grid")]
    #[test]
    fn imitate_best_on_a_grid_rejects_a_tit_for_tat_group() {
        use socsim_games::{
            GridBoundary, GridNeighborhood, GridNeighbors, GridSpec, ImitateBest, Topology,
        };
        let factory = GameSimFactory::new(PrisonersDilemma::weak(1.85).unwrap())
            .agents(9)
            .topology(
                Topology::grid(GridSpec {
                    rows: 3,
                    cols: 3,
                    boundary: GridBoundary::Toroidal,
                    neighborhood: GridNeighbors::Moore,
                })
                .unwrap(),
            )
            .matching(GridNeighborhood::new().include_self(true))
            .rule("tit_for_tat", TitForTat::new(0))
            .assign(one_tit_for_tat(9))
            .dynamics(ImitateBest::new(Update::Synchronous))
            .history_len(1)
            .t_max(4);
        assert_population_error(factory.build(1), "applies only to held agents");
    }

    // (2) Copying dynamics on a game whose roles have different action counts.
    fn rectangular_matrix() -> MatrixGame {
        MatrixGame::bimatrix(
            &[vec![1.0, 0.0, 2.0], vec![0.0, 1.0, 0.0]],
            &[vec![1.0, 0.0, 2.0], vec![0.0, 1.0, 0.0]],
        )
        .unwrap()
    }

    #[test]
    fn copying_dynamics_reject_roles_with_different_action_counts() {
        let factory = GameSimFactory::new(rectangular_matrix())
            .agents(4)
            .matching(RandomPairs::new())
            .dynamics(FermiUpdate::new(1.0, Update::Synchronous).unwrap())
            .history_len(1)
            .t_max(1);
        assert_population_error(factory.build(7), "every role must share one action space");
    }

    #[cfg(feature = "grid")]
    #[test]
    fn imitate_best_on_a_grid_rejects_roles_with_different_action_counts() {
        use socsim_games::{
            GridBoundary, GridNeighborhood, GridNeighbors, GridSpec, ImitateBest, Topology,
        };
        let factory = GameSimFactory::new(rectangular_matrix())
            .agents(9)
            .topology(
                Topology::grid(GridSpec {
                    rows: 3,
                    cols: 3,
                    boundary: GridBoundary::Toroidal,
                    neighborhood: GridNeighbors::Moore,
                })
                .unwrap(),
            )
            .matching(GridNeighborhood::new())
            .dynamics(ImitateBest::new(Update::Synchronous))
            .history_len(1)
            .t_max(1);
        assert_population_error(factory.build(1), "every role must share one action space");
    }

    // (3) A per-step decision with an agent that holds several roles.
    #[test]
    fn per_step_rejects_round_robin_with_three_agents() {
        let factory = || {
            GameSimFactory::new(hawk_dove())
                .agents(3)
                .matching(RoundRobin::new())
                .rule("fixed", Fixed::new(0))
                .history_len(1)
                .t_max(1)
        };
        assert!(matches!(
            factory().build(7),
            Err(GameError::InvalidMatching(_))
        ));
        assert!(factory()
            .play_options(PlayOptions::new().decision_scope(DecisionScope::PerEncounter))
            .build(7)
            .is_ok());
    }

    // (4) Fictitious play needs a two-role game.
    #[test]
    fn fictitious_play_rejects_a_three_role_game() {
        let game = MatrixGame::new(vec![2, 2, 2], vec![vec![1.0; 8]; 3]).unwrap();
        let factory = GameSimFactory::new(game)
            .agents(3)
            .matching(WholePopulation::new())
            .rule("fictitious_play", FictitiousPlay::new())
            .history_len(1)
            .t_max(1);
        assert!(matches!(
            factory.build(7),
            Err(GameError::Rule(RuleError::Unsupported(_)))
        ));
    }

    // (5) Parameters that the payoff range does not allow.
    #[test]
    fn proportional_imitation_rejects_beta_above_the_inverse_payoff_width() {
        // Hawk-dove with V = 2 and C = 4 has payoffs in [-1, 2]: width 3, and
        // random pairs give every agent of an even population one encounter.
        let factory = |beta: f64| {
            GameSimFactory::new(hawk_dove())
                .agents(10)
                .matching(RandomPairs::new())
                .dynamics(ProportionalImitation::new(beta, Update::Synchronous).unwrap())
                .history_len(1)
                .t_max(1)
        };
        assert!(factory(1.0 / 3.0).build(7).is_ok());
        assert!(matches!(
            factory(2.0 / 3.0).build(7),
            Err(GameError::Population(_))
        ));
    }

    #[test]
    fn linear_moran_fitness_rejects_a_non_positive_fitness() {
        // The lowest payoff is the sucker's payoff -10: 1 - w + w * (-10) =
        // -4.5 with w = 0.5.
        let game = PrisonersDilemma::new(5.0, 3.0, 1.0, -10.0).unwrap();
        let factory = |w: f64| {
            GameSimFactory::new(game.clone())
                .agents(10)
                .matching(RandomPairs::new())
                .dynamics(
                    MoranUpdate::new(Fitness::Linear { w }, 0.0, Update::Synchronous).unwrap(),
                )
                .history_len(1)
                .t_max(1)
        };
        assert!(matches!(
            factory(0.5).build(7),
            Err(GameError::Population(_))
        ));
        assert!(factory(0.05).build(7).is_ok());
    }

    // (7) Duplicate and missing rule kinds.
    #[test]
    fn a_rule_kind_registered_twice_is_rejected() {
        let factory = GameSimFactory::new(hawk_dove())
            .agents(2)
            .matching(RandomPairs::new())
            .rule("fixed", Fixed::new(0))
            .rule("fixed", Fixed::new(1))
            .assign(Assignment::Ordered(vec![AssignGroup {
                kind: AssignKind::Rule("fixed".to_owned()),
                count: 2,
            }]))
            .history_len(1)
            .t_max(1);
        assert!(matches!(
            factory.build(7),
            Err(GameError::Rule(RuleError::DuplicateKind(kind))) if kind == "fixed"
        ));
    }

    #[test]
    fn a_group_of_an_unregistered_rule_kind_is_rejected() {
        let factory = GameSimFactory::new(hawk_dove())
            .agents(2)
            .matching(RandomPairs::new())
            .rule("fixed", Fixed::new(0))
            .assign(Assignment::Ordered(vec![
                AssignGroup {
                    kind: AssignKind::Rule("fixed".to_owned()),
                    count: 1,
                },
                AssignGroup {
                    kind: AssignKind::Rule("ghost".to_owned()),
                    count: 1,
                },
            ]))
            .history_len(1)
            .t_max(1);
        assert!(matches!(
            factory.build(7),
            Err(GameError::Rule(RuleError::UnknownKind(kind))) if kind == "ghost"
        ));
    }

    // (8) A built-in game checks its parameters when it is constructed.
    #[test]
    fn a_built_in_game_constructor_error_converts_into_a_catalog_error() {
        fn build() -> Result<(), GameError> {
            let factory = GameSimFactory::new(PrisonersDilemma::new(1.0, 2.0, 3.0, 4.0)?)
                .agents(2)
                .matching(RandomPairs::new())
                .history_len(1)
                .t_max(1);
            factory.build(7).map(|_| ())
        }
        assert!(matches!(
            build(),
            Err(GameError::Catalog(CatalogError::Condition(_)))
        ));
    }

    // The factory's own settings.
    #[test]
    fn missing_required_settings_are_configuration_errors() {
        let base = || GameSimFactory::new(hawk_dove()).matching(RandomPairs::new());
        assert_config_error(base().history_len(1).t_max(1).build(7), "agents");
        assert_config_error(base().agents(2).t_max(1).build(7), "history_len");
        assert_config_error(base().agents(2).history_len(1).build(7), "t_max");
        assert_config_error(
            base()
                .agents(2)
                .history_len(1)
                .t_max(1)
                .rule("hawk", Fixed::new(0))
                .rule("dove", Fixed::new(1))
                .build(7),
            "assign",
        );
        assert!(matches!(
            base().agents(2).history_len(0).t_max(1).build(7),
            Err(GameError::InvalidHistoryLength)
        ));
    }

    #[test]
    fn a_continuous_game_requires_initial_held_actions() {
        let factory = GameSimFactory::new(Cournot::new(2, 100.0, 1.0, 10.0, 100.0).unwrap())
            .agents(2)
            .matching(WholePopulation::new())
            .history_len(1)
            .t_max(1);
        assert_config_error(factory.build(7), "initial_held");
        assert!(factory
            .initial_held(InitialActions::Same(vec![10.0]))
            .build(7)
            .is_ok());
    }

    #[test]
    fn invalid_initial_shares_are_configuration_errors() {
        let factory = |shares: Vec<(usize, f64)>| {
            GameSimFactory::new(hawk_dove())
                .agents(10)
                .matching(RandomPairs::new())
                .initial_held(InitialActions::Shares(shares))
                .history_len(1)
                .t_max(1)
                .build(7)
        };
        assert!(matches!(
            factory(vec![(0, 0.5), (0, 0.5)]),
            Err(GameError::Config(_))
        ));
        assert!(matches!(
            factory(vec![(0, 0.7), (1, 0.4)]),
            Err(GameError::Config(_))
        ));
        assert!(matches!(
            factory(vec![(0, 0.5), (2, 0.5)]),
            Err(GameError::Action(_))
        ));
    }
}

#[cfg(feature = "config")]
mod config_guards {
    use socsim_config::Scenario;
    use socsim_games::{FiniteSpec, GameError, RuleRegistry, ScenarioConfig};

    fn read(world: &str, mechanisms: &str) -> Result<ScenarioConfig<FiniteSpec>, GameError> {
        let text = format!(
            "[simulation]\nname = \"guards\"\nmodule_pack = \"normal-form\"\nt_max = 4\nseed = 1\nscheduler = \"sequential\"\n\n[world]\n{world}\n{mechanisms}\n[output]\nlog_path = \"runs/{{name}}_{{seed}}.jsonl\"\n"
        );
        ScenarioConfig::read(
            &Scenario::parse(&text).unwrap(),
            &RuleRegistry::finite_builtins(),
        )
    }

    const HAWK_DOVE: &str = "n_agents = 4\nhistory_len = 2\n[world.game]\nkind = \"hawk_dove\"\nv = 2\nc = 3\n[world.population]\n[[world.population.group]]\nrule = \"held\"\ncount = 4\n";

    fn entry(name: &str, params: &str) -> String {
        format!("[[mechanism]]\nname = \"{name}\"\n[mechanism.params]\n{params}\n\n")
    }

    fn config_message<T>(result: Result<T, GameError>) -> String {
        match result {
            Err(GameError::Config(message)) => message,
            Err(other) => panic!("expected a configuration error, got {other:?}"),
            Ok(_) => panic!("expected a configuration error, got Ok"),
        }
    }

    // (6) A population dynamic listed before `game_decide`.
    #[test]
    fn a_population_dynamic_listed_before_game_decide_is_rejected() {
        let matching = entry("game_match", "matching = \"random_pairs\"");
        let fermi = entry("fermi_update", "update = \"synchronous\"\nk = 0.5");
        let decide = entry("game_decide", "");
        let play = entry("game_play", "");
        let message = config_message(read(HAWK_DOVE, &format!("{matching}{fermi}{decide}{play}")));
        assert!(message.starts_with("mechanism[fermi_update]"), "{message}");
        assert!(read(HAWK_DOVE, &format!("{matching}{decide}{fermi}{play}")).is_ok());
    }

    // (8) A built-in game whose parameters violate its conditions.
    #[test]
    fn a_prisoners_dilemma_with_invalid_payoffs_is_rejected() {
        let world = "n_agents = 4\nhistory_len = 2\n[world.game]\nkind = \"prisoners_dilemma\"\ntemptation = 1\nreward = 2\npunishment = 3\nsucker = 4\n[world.population]\n[[world.population.group]]\nrule = \"held\"\ncount = 4\n";
        let mechanisms = format!(
            "{}{}{}",
            entry("game_match", "matching = \"random_pairs\""),
            entry("game_decide", ""),
            entry("game_play", "")
        );
        assert!(matches!(
            read(world, &mechanisms),
            Err(GameError::Catalog(_))
        ));
    }

    // (7) A group that names a rule that is not registered, and unknown keys.
    #[test]
    fn unregistered_rules_and_unknown_keys_are_rejected() {
        let mechanisms = format!(
            "{}{}{}",
            entry("game_match", "matching = \"random_pairs\""),
            entry("game_decide", ""),
            entry("game_play", "")
        );
        let world = HAWK_DOVE.replace("rule = \"held\"", "rule = \"tft\"");
        let message = config_message(read(&world, &mechanisms));
        assert!(
            message.starts_with("world.population.group[0].rule"),
            "{message}"
        );
        let world = HAWK_DOVE.replace("v = 2", "v = 2\nvv = 1");
        let message = config_message(read(&world, &mechanisms));
        assert!(message.starts_with("world.game.vv"), "{message}");
    }
}

/// The run-time checks reached from a scenario: `GameTrialFactory::validate`
/// reads the scenario, checks the order of `[[mechanism]]`, and builds a trial
/// for `simulation.seed`, so every check of the assembly reports a
/// configuration error that names the key or the mechanism at fault.
#[cfg(feature = "config")]
mod trial_guards {
    use socsim_config::Scenario;
    use socsim_core::SocsimError;
    use socsim_games::{FiniteSpec, GameTrialFactory, RuleRegistry};
    use socsim_runner::TrialFactory;

    fn scenario(world: &str, population: &str, mechanisms: &[(&str, &str)]) -> Scenario {
        let mechanisms: String = mechanisms
            .iter()
            .map(|(name, params)| {
                format!("[[mechanism]]\nname = \"{name}\"\n[mechanism.params]\n{params}\n\n")
            })
            .collect();
        Scenario::parse(&format!(
            "[simulation]\nname = \"guards\"\nmodule_pack = \"normal-form\"\nt_max = 4\nseed = 1\nscheduler = \"sequential\"\n\n[world]\n{world}\n\n[world.population]\n{population}\n\n{mechanisms}[output]\nlog_path = \"runs/{{name}}_{{seed}}.jsonl\"\n"
        ))
        .unwrap()
    }

    fn validate(scenario: &Scenario) -> socsim_core::Result<()> {
        let factory: GameTrialFactory<FiniteSpec> =
            GameTrialFactory::new(RuleRegistry::finite_builtins());
        factory.validate(scenario)
    }

    /// Checks that `scenario` is rejected with a message that starts with
    /// `location` and mentions `reason`.
    fn assert_rejected(scenario: &Scenario, location: &str, reason: &str) {
        match validate(scenario) {
            Err(SocsimError::Config(message)) => {
                assert!(message.starts_with(location), "{message}");
                assert!(message.contains(reason), "{message}");
            }
            Err(other) => panic!("expected a configuration error, got {other:?}"),
            Ok(()) => panic!("expected {location} to be rejected"),
        }
    }

    const HAWK_DOVE: &str = "[world.game]\nkind = \"hawk_dove\"\nv = 2\nc = 4";
    const HELD: &str = "[[world.population.group]]\nrule = \"held\"\ncount = 10";
    const TEN: &str = "n_agents = 10\nhistory_len = 1";

    // (1) Population dynamics with an agent on an active rule.
    #[test]
    fn a_population_dynamic_rejects_a_group_with_an_active_rule() {
        let population = "[[world.population.group]]\nrule = \"held\"\ncount = 9\n[[world.population.group]]\nrule = \"tit_for_tat\"\ncount = 1";
        let mechanisms = [
            ("game_match", "matching = \"random_pairs\""),
            ("game_decide", "coop_label = \"H\""),
            ("game_play", ""),
            ("fermi_update", "update = \"synchronous\"\nk = 1.0"),
        ];
        let world = format!("{TEN}\n{HAWK_DOVE}");
        assert_rejected(
            &scenario(&world, population, &mechanisms),
            "mechanism[fermi_update]",
            "applies only to held agents",
        );
        validate(&scenario(&world, HELD, &mechanisms)).unwrap();
    }

    #[cfg(feature = "grid")]
    #[test]
    fn imitate_best_on_a_grid_rejects_a_tit_for_tat_group() {
        let world = format!("topology = \"grid\"\ngrid_rows = 2\ngrid_cols = 5\nboundary = \"toroidal\"\nneighborhood = \"moore\"\nhistory_len = 1\n{HAWK_DOVE}");
        let population = "[[world.population.group]]\nrule = \"held\"\ncount = 9\n[[world.population.group]]\nrule = \"tit_for_tat\"\ncount = 1";
        let mechanisms = [
            ("game_match", "matching = \"grid_neighborhood\""),
            ("game_decide", "coop_label = \"H\""),
            ("game_play", ""),
            ("imitate_best", "update = \"synchronous\""),
        ];
        assert_rejected(
            &scenario(&world, population, &mechanisms),
            "mechanism[imitate_best]",
            "applies only to held agents",
        );
    }

    // (2) Roles that do not share one action space, on a grid so that the
    // common-space check is the one that fails.
    #[cfg(feature = "grid")]
    #[test]
    fn imitate_best_rejects_roles_with_different_action_counts() {
        let world = "topology = \"grid\"\ngrid_rows = 2\ngrid_cols = 5\nboundary = \"toroidal\"\nneighborhood = \"moore\"\nhistory_len = 1\n[world.game]\nkind = \"matrix\"\nlabels = [[\"a\", \"b\"], [\"x\", \"y\", \"z\"]]\npayoffs = [[[1, 1], [0, 0], [0, 0]], [[0, 0], [1, 1], [0, 0]]]";
        let mechanisms = [
            ("game_match", "matching = \"grid_neighborhood\""),
            ("game_decide", ""),
            ("game_play", ""),
            ("imitate_best", "update = \"synchronous\""),
        ];
        assert_rejected(
            &scenario(world, HELD, &mechanisms),
            "mechanism[imitate_best]",
            "share one action space",
        );
    }

    // (3) A per-step decision with an agent holding several roles.
    #[test]
    fn per_step_rejects_round_robin_with_three_agents() {
        let world = format!("n_agents = 3\nhistory_len = 1\n{HAWK_DOVE}");
        let population = "[[world.population.group]]\nrule = \"uniform_random\"\ncount = 3";
        let mechanisms = |scope: &'static str| {
            [
                ("game_match", "matching = \"round_robin\""),
                ("game_decide", scope),
                ("game_play", ""),
            ]
        };
        assert_rejected(
            &scenario(
                &world,
                population,
                &mechanisms("decision_scope = \"per_step\""),
            ),
            "mechanism[game_match]",
            "per-step decision",
        );
        validate(&scenario(
            &world,
            population,
            &mechanisms("decision_scope = \"per_encounter\""),
        ))
        .unwrap();
    }

    // (4) Fictitious play on a game with three roles.
    #[test]
    fn fictitious_play_rejects_a_three_role_game() {
        let world = "n_agents = 3\nhistory_len = 1\n[world.game]\nkind = \"matrix\"\nlabels = [[\"a\", \"b\"], [\"a\", \"b\"], [\"a\", \"b\"]]\npayoffs = [[[[1, 1, 1], [0, 0, 0]], [[0, 0, 0], [1, 1, 1]]], [[[1, 1, 1], [0, 0, 0]], [[0, 0, 0], [1, 1, 1]]]]";
        let population = "[[world.population.group]]\nrule = \"fictitious_play\"\ncount = 3";
        let mechanisms = [
            ("game_match", "matching = \"whole_population\""),
            ("game_decide", ""),
            ("game_play", ""),
        ];
        assert_rejected(
            &scenario(world, population, &mechanisms),
            "mechanism[game_decide]",
            "requires 2 roles",
        );
    }

    // (5) Dynamic parameters that the payoff range does not allow. Hawk-dove
    // with v = 2, c = 4 pays between -1 and 2 per encounter, a width of 3,
    // so beta may be at most 1/3 with one encounter per step.
    #[test]
    fn replicator_update_rejects_beta_above_the_inverse_payoff_width() {
        let world = format!("{TEN}\n{HAWK_DOVE}");
        let mechanisms = |beta: &'static str| {
            [
                ("game_match", "matching = \"random_pairs\""),
                ("game_decide", ""),
                ("game_play", ""),
                ("replicator_update", beta),
            ]
        };
        assert_rejected(
            &scenario(
                &world,
                HELD,
                &mechanisms("update = \"synchronous\"\nbeta = 0.5"),
            ),
            "mechanism[replicator_update]",
            "beta",
        );
        validate(&scenario(
            &world,
            HELD,
            &mechanisms("update = \"synchronous\"\nbeta = 0.3"),
        ))
        .unwrap();
    }

    // Linear Moran fitness 1 - w + w * payoff with w = 0.5 and a lowest
    // payoff of -10 is negative.
    #[test]
    fn linear_moran_fitness_rejects_a_non_positive_fitness() {
        let world = "n_agents = 10\nhistory_len = 1\n[world.game]\nkind = \"prisoners_dilemma\"\ntemptation = 5\nreward = 3\npunishment = 1\nsucker = -10";
        let mechanisms = [
            ("game_match", "matching = \"random_pairs\""),
            ("game_decide", ""),
            ("game_play", ""),
            (
                "moran_update",
                "update = \"synchronous\"\nfitness = \"linear\"\nw = 0.5",
            ),
        ];
        assert_rejected(
            &scenario(world, HELD, &mechanisms),
            "mechanism[moran_update]",
            "must be positive",
        );
    }

    // (6) A population dynamic listed before `game_decide`, reported by
    // `validate`.
    #[test]
    fn a_population_dynamic_listed_before_game_decide_is_rejected() {
        let world = format!("{TEN}\n{HAWK_DOVE}");
        let before = [
            ("game_match", "matching = \"random_pairs\""),
            ("fermi_update", "update = \"synchronous\"\nk = 1.0"),
            ("game_decide", ""),
            ("game_play", ""),
        ];
        assert_rejected(
            &scenario(&world, HELD, &before),
            "mechanism[fermi_update]",
            "after game_decide",
        );
        let after = [before[0], before[2], before[1], before[3]];
        validate(&scenario(&world, HELD, &after)).unwrap();
    }

    // (7) A group of a rule kind that is not registered.
    #[test]
    fn a_group_of_an_unregistered_rule_is_rejected() {
        let world = format!("{TEN}\n{HAWK_DOVE}");
        let population = "[[world.population.group]]\nrule = \"tft\"\ncount = 10";
        let mechanisms = [
            ("game_match", "matching = \"random_pairs\""),
            ("game_decide", ""),
            ("game_play", ""),
        ];
        assert_rejected(
            &scenario(&world, population, &mechanisms),
            "world.population.group[0].rule",
            "tft",
        );
    }

    // (8) A built-in game whose parameters violate its conditions.
    #[test]
    fn a_prisoners_dilemma_with_invalid_payoffs_is_rejected() {
        let world = "n_agents = 10\nhistory_len = 1\n[world.game]\nkind = \"prisoners_dilemma\"\ntemptation = 1\nreward = 2\npunishment = 3\nsucker = 4";
        let mechanisms = [
            ("game_match", "matching = \"random_pairs\""),
            ("game_decide", ""),
            ("game_play", ""),
        ];
        assert_rejected(
            &scenario(world, HELD, &mechanisms),
            "world.game",
            "T > R > P >= S",
        );
    }
}
