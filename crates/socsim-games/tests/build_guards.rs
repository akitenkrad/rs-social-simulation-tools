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
