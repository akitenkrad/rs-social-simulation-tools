//! Asynchronous population updates recompute local payoffs after every write
//! and record one coherent post-update state at the end of each step.

use std::any::Any;
use std::collections::BTreeMap;
use std::sync::Arc;

use rand::seq::SliceRandom;
use socsim_core::{derive_seed, AgentId, Recorder, SimRng};
use socsim_engine::{
    RandomActivationScheduler, SequentialScheduler, Simulation, SimulationBuilder, Snapshot,
};
use socsim_games::catalog::{PrisonersDilemma, PublicGoodsFinite};
use socsim_games::continuous::PublicGoodsContinuous;
use socsim_games::{
    assemble_async, assemble_with, build_world, counting_play, AssignGroup, AssignKind, Assignment,
    AsyncActivation, AsyncPopulationStep, ContinuousMetrics, ContinuousMetricsOptions,
    DecisionScope, EncounterRef, FermiUpdate, FiniteGame, Fixed, GameError, GameMetrics, GameWorld,
    GridBoundary, GridNeighborhood, GridNeighbors, GridSpec, ImitateBest, InitialActions,
    MetricsOptions, NetworkEdges, NoDynamics, NormalFormOutcome, OnInvalidAction, OutcomeContext,
    OutcomeRule, Pairwise, PayoffAggregation, PayoffTiming, PlayOptions, ProportionalImitation,
    RandomPairs, RuleBook, StageGame, Topology, Update, WholePopulation,
};

const SIDE: usize = 99;
const CENTER: usize = 49;
const COOPERATE: usize = 0;
const DEFECT: usize = 1;

fn index(row: usize, col: usize) -> usize {
    row * SIDE + col
}

fn held(n_agents: usize) -> Assignment {
    Assignment::Ordered(vec![AssignGroup {
        kind: AssignKind::Held,
        count: n_agents,
    }])
}

#[derive(Default)]
struct TestRecorder {
    rows: Vec<(u64, String, u64)>,
    events: Vec<(u64, String, serde_json::Value)>,
}

impl Recorder for TestRecorder {
    fn record_metric(&mut self, t: u64, key: &str, value: f64) {
        self.rows.push((t, key.to_owned(), value.to_bits()));
    }

    fn record_event(&mut self, t: u64, kind: &str, payload: serde_json::Value) {
        self.events.push((t, kind.to_owned(), payload));
    }

    fn record_row(&mut self, t: u64, table: &str, row: &[(&str, f64)]) {
        self.rows.extend(
            row.iter()
                .map(|(column, value)| (t, format!("{table}.{column}"), value.to_bits())),
        );
    }

    fn as_any(&self) -> Option<&dyn Any> {
        Some(self)
    }
}

fn recorder<G: StageGame>(simulation: &Simulation<GameWorld<G>>) -> &TestRecorder {
    simulation
        .recorder()
        .as_any()
        .unwrap()
        .downcast_ref::<TestRecorder>()
        .unwrap()
}

fn finite_finish<G: FiniteGame>(
    builder: SimulationBuilder<GameWorld<G>>,
    game: &G,
) -> Simulation<GameWorld<G>> {
    let metrics = GameMetrics::new(game, MetricsOptions::new().population_share(true)).unwrap();
    builder
        .add_mechanism(Box::new(metrics))
        .recorder(Box::new(TestRecorder::default()))
        .build()
}

fn nowak_world(
    side: usize,
    steps: u64,
    seed: u64,
    initial: Vec<usize>,
) -> GameWorld<PrisonersDilemma> {
    let rules = RuleBook::new();
    build_world(
        PrisonersDilemma::weak(1.85).unwrap(),
        side * side,
        (),
        InitialActions::PerAgent(initial),
        &rules,
        &held(side * side),
        2,
        steps,
        seed,
    )
    .unwrap()
    .with_topology(
        Topology::grid(GridSpec {
            rows: side,
            cols: side,
            boundary: GridBoundary::Fixed,
            neighborhood: GridNeighbors::Moore,
        })
        .unwrap(),
    )
    .unwrap()
}

fn hand_payoffs(held: &[usize]) -> Vec<f64> {
    let payoff = |own: usize, other: usize| match (own, other) {
        (COOPERATE, COOPERATE) => 1.0,
        (DEFECT, COOPERATE) => 1.85,
        _ => 0.0,
    };
    let mut out = vec![0.0; held.len()];
    for row in 0..SIDE {
        for col in 0..SIDE {
            let own = held[index(row, col)];
            for dr in -1_i64..=1 {
                for dc in -1_i64..=1 {
                    let r = row as i64 + dr;
                    let c = col as i64 + dc;
                    if r >= 0 && c >= 0 && r < SIDE as i64 && c < SIDE as i64 {
                        out[index(row, col)] += payoff(own, held[index(r as usize, c as usize)]);
                    }
                }
            }
        }
    }
    out
}

fn assert_not_dihedral(values: &[usize]) {
    let asymmetric = (0..SIDE).any(|row| {
        (0..SIDE).any(|col| {
            let value = values[index(row, col)];
            value != values[index(row, SIDE - 1 - col)]
                || value != values[index(SIDE - 1 - row, col)]
                || value != values[index(col, row)]
        })
    });
    assert!(
        asymmetric,
        "the first asynchronous generation stayed dihedral"
    );
}

#[test]
fn sequential_agent_order_breaks_symmetry_at_the_derived_boundary() {
    let game = PrisonersDilemma::weak(1.85).unwrap();
    let rules = RuleBook::new();
    let mut initial = vec![COOPERATE; SIDE * SIDE];
    initial[index(CENTER, CENTER)] = DEFECT;
    let world = build_world(
        game,
        SIDE * SIDE,
        (),
        InitialActions::PerAgent(initial),
        &rules,
        &held(SIDE * SIDE),
        2,
        1,
        92,
    )
    .unwrap()
    .with_topology(
        Topology::grid(GridSpec {
            rows: SIDE,
            cols: SIDE,
            boundary: GridBoundary::Fixed,
            neighborhood: GridNeighbors::Moore,
        })
        .unwrap(),
    )
    .unwrap();
    let step = AsyncPopulationStep::new(ImitateBest::new(Update::Asynchronous))
        .activation(AsyncActivation::AgentOrder);
    let mut simulation = assemble_async(
        world,
        GridNeighborhood::new().include_self(true),
        Pairwise::new(NormalFormOutcome),
        step,
        PlayOptions::new(),
        92,
    )
    .unwrap()
    .scheduler(Box::new(SequentialScheduler))
    .build();

    simulation.step().unwrap();
    let row = &simulation.world().held()[index(48, 0)..=index(48, 98)];
    assert!(row[..48].iter().all(|&action| action == COOPERATE));
    assert!(row[48..].iter().all(|&action| action == DEFECT));
}

#[test]
fn records_final_actions_and_matches_the_implicit_no_dynamics_path_bit_for_bit() {
    let mut initial = vec![COOPERATE; SIDE * SIDE];
    initial[index(CENTER, CENTER)] = DEFECT;
    let game = PrisonersDilemma::weak(1.85).unwrap();
    let builder = assemble_async(
        nowak_world(SIDE, 10, 92, initial),
        GridNeighborhood::new().include_self(true),
        Pairwise::new(NormalFormOutcome),
        AsyncPopulationStep::new(ImitateBest::new(Update::Asynchronous)),
        PlayOptions::new(),
        92,
    )
    .unwrap()
    .scheduler(Box::new(RandomActivationScheduler));
    let mut asynchronous = finite_finish(builder, &game);

    for generation in 1..=10 {
        asynchronous.step().unwrap();
        let world = asynchronous.world();
        let expected = hand_payoffs(world.held());
        for (agent, (&actual, &expected)) in world.payoffs().iter().zip(&expected).enumerate() {
            assert!(
                (actual - expected).abs() <= 1e-12,
                "agent {agent} payoff {actual} differs from {expected} in generation {generation}"
            );
            let record = world
                .history(AgentId(agent as u64))
                .unwrap()
                .last()
                .unwrap();
            assert_eq!(record.action, Some(world.held()[agent]));
        }

        let rules = RuleBook::new();
        let reference_world = build_world(
            game.clone(),
            SIDE * SIDE,
            (),
            InitialActions::PerAgent(world.held().to_vec()),
            &rules,
            &held(SIDE * SIDE),
            2,
            1,
            92,
        )
        .unwrap()
        .with_topology(
            Topology::grid(GridSpec {
                rows: SIDE,
                cols: SIDE,
                boundary: GridBoundary::Fixed,
                neighborhood: GridNeighbors::Moore,
            })
            .unwrap(),
        )
        .unwrap();
        let reference_builder = assemble_with(
            reference_world,
            GridNeighborhood::new().include_self(true),
            rules,
            Pairwise::new(NormalFormOutcome),
            NoDynamics,
            PlayOptions::new(),
            92,
        )
        .unwrap();
        let mut reference = finite_finish(reference_builder, &game);
        reference.step().unwrap();

        assert_eq!(world.payoffs(), reference.world().payoffs());
        assert_eq!(world.n_played(), reference.world().n_played());
        for agent in 0..SIDE * SIDE {
            assert_eq!(
                world.history(AgentId(agent as u64)).unwrap().last(),
                reference
                    .world()
                    .history(AgentId(agent as u64))
                    .unwrap()
                    .last()
            );
        }
        let async_played: Vec<_> = recorder(&asynchronous)
            .rows
            .iter()
            .filter(|(t, key, _)| *t == generation && key.starts_with("game.played_share."))
            .map(|(_, key, value)| (key.clone(), *value))
            .collect();
        let reference_played: Vec<_> = recorder(&reference)
            .rows
            .iter()
            .filter(|(_, key, _)| key.starts_with("game.played_share."))
            .map(|(_, key, value)| (key.clone(), *value))
            .collect();
        assert_eq!(async_played, reference_played);
        assert_eq!(world.plays().n_encounters(), 0);
    }
}

#[test]
fn random_agent_order_breaks_dihedral_symmetry_for_fixed_seeds() {
    for seed in [92, 7] {
        let mut initial = vec![COOPERATE; SIDE * SIDE];
        initial[index(CENTER, CENTER)] = DEFECT;
        let mut simulation = assemble_async(
            nowak_world(SIDE, 1, seed, initial),
            GridNeighborhood::new().include_self(true),
            Pairwise::new(NormalFormOutcome),
            AsyncPopulationStep::new(ImitateBest::new(Update::Asynchronous)),
            PlayOptions::new(),
            seed,
        )
        .unwrap()
        .scheduler(Box::new(RandomActivationScheduler))
        .build();
        simulation.step().unwrap();
        assert_not_dihedral(simulation.world().held());
    }
}

fn initial_actions(
    n_agents: usize,
    count: usize,
    focal: usize,
    other: usize,
    seed: u64,
) -> Vec<usize> {
    let mut actions = vec![other; n_agents];
    actions[..count].fill(focal);
    actions.shuffle(&mut SimRng::from_seed(derive_seed(seed, &[0, 2])));
    actions
}

fn random_regular(n_agents: usize, degree: usize, seed: u64) -> socsim_net::SocialNetwork {
    let mut rng = SimRng::from_seed(derive_seed(seed, &[0, 1]));
    loop {
        let mut stubs: Vec<usize> = (0..n_agents)
            .flat_map(|agent| std::iter::repeat_n(agent, degree))
            .collect();
        stubs.shuffle(&mut rng);
        let mut edges = std::collections::BTreeSet::new();
        let simple = stubs.as_chunks::<2>().0.iter().all(|pair| {
            let (a, b) = (pair[0].min(pair[1]), pair[0].max(pair[1]));
            a != b && edges.insert((a, b))
        });
        if !simple {
            continue;
        }
        let mut network = socsim_net::SocialNetwork::empty();
        for agent in 0..n_agents {
            network.add_node(AgentId(agent as u64));
        }
        for pair in stubs.as_chunks::<2>().0 {
            network.add_edge(AgentId(pair[0] as u64), AgentId(pair[1] as u64));
        }
        return network;
    }
}

fn async_nowak_may(seed: u64) -> Simulation<GameWorld<PrisonersDilemma>> {
    let side = 50;
    let n_agents = side * side;
    let game = PrisonersDilemma::weak(1.85).unwrap();
    let rules = RuleBook::new();
    let world = build_world(
        game.clone(),
        n_agents,
        (),
        InitialActions::PerAgent(initial_actions(n_agents, n_agents / 10, 1, 0, seed)),
        &rules,
        &held(n_agents),
        2,
        30,
        seed,
    )
    .unwrap()
    .with_topology(
        Topology::grid(GridSpec {
            rows: side,
            cols: side,
            boundary: GridBoundary::Toroidal,
            neighborhood: GridNeighbors::Moore,
        })
        .unwrap(),
    )
    .unwrap();
    let builder = assemble_async(
        world,
        GridNeighborhood::new().include_self(true),
        Pairwise::new(NormalFormOutcome),
        AsyncPopulationStep::new(ImitateBest::new(Update::Asynchronous)),
        PlayOptions::new(),
        seed,
    )
    .unwrap()
    .scheduler(Box::new(RandomActivationScheduler));
    finite_finish(builder, &game)
}

fn async_network_fermi(seed: u64) -> Simulation<GameWorld<PrisonersDilemma>> {
    let n_agents = 200;
    let game = PrisonersDilemma::weak(1.5).unwrap();
    let rules = RuleBook::new();
    let world = build_world(
        game.clone(),
        n_agents,
        (),
        InitialActions::PerAgent(initial_actions(n_agents, n_agents / 2, 1, 0, seed)),
        &rules,
        &held(n_agents),
        2,
        20,
        seed,
    )
    .unwrap()
    .with_topology(Topology::network(random_regular(n_agents, 4, seed)))
    .unwrap();
    let builder = assemble_async(
        world,
        NetworkEdges::new(),
        Pairwise::new(NormalFormOutcome),
        AsyncPopulationStep::new(FermiUpdate::new(0.1, Update::Asynchronous).unwrap())
            .activation(AsyncActivation::UniformWithReplacement),
        PlayOptions::new(),
        seed,
    )
    .unwrap();
    finite_finish(builder, &game)
}

fn async_public_goods(seed: u64) -> Simulation<GameWorld<PublicGoodsFinite>> {
    let n_agents = 8;
    let game = PublicGoodsFinite::new(n_agents, 3.0, 1.0).unwrap();
    let rules = RuleBook::new();
    let world = build_world(
        game.clone(),
        n_agents,
        (),
        InitialActions::PerAgent(initial_actions(n_agents, n_agents / 2, 0, 1, seed)),
        &rules,
        &held(n_agents),
        2,
        30,
        seed,
    )
    .unwrap();
    let builder = assemble_async(
        world,
        WholePopulation::new(),
        counting_play(),
        AsyncPopulationStep::new(
            ProportionalImitation::new(1.0 / 3.25, Update::Asynchronous).unwrap(),
        )
        .activation(AsyncActivation::UniformWithReplacement),
        PlayOptions::new(),
        seed,
    )
    .unwrap();
    finite_finish(builder, &game)
}

fn run_steps<G>(simulation: &mut Simulation<GameWorld<G>>, count: u64) -> Vec<String>
where
    G: FiniteGame + serde::Serialize,
{
    (0..count)
        .map(|_| {
            simulation.step().unwrap();
            serde_json::to_string(simulation.world()).unwrap()
        })
        .collect()
}

fn assert_deterministic_and_resume<G>(
    build: impl Fn(u64) -> Simulation<GameWorld<G>>,
    steps: u64,
    split: u64,
) where
    G: FiniteGame + Clone + serde::Serialize + serde::de::DeserializeOwned,
{
    let seed = 20261011;
    let mut uninterrupted = build(seed);
    let expected = run_steps(&mut uninterrupted, steps);
    let expected_rows = recorder(&uninterrupted).rows.clone();

    let mut repeated = build(seed);
    assert_eq!(run_steps(&mut repeated, steps), expected);
    assert_eq!(recorder(&repeated).rows, expected_rows);

    let rows_after_split: Vec<_> = expected_rows
        .iter()
        .filter(|(t, _, _)| *t > split)
        .cloned()
        .collect();
    assert!(!rows_after_split.is_empty());

    let mut partial = build(seed);
    run_steps(&mut partial, split);
    let mut resumed = build(seed ^ 0x5eed);
    resumed.restore(partial.snapshot());
    assert_eq!(
        run_steps(&mut resumed, steps - split),
        expected[split as usize..]
    );
    assert_eq!(recorder(&resumed).rows, rows_after_split);

    let mut partial = build(seed);
    run_steps(&mut partial, split);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("snapshot.json");
    partial.snapshot().save(&path).unwrap();
    let snapshot = Snapshot::<GameWorld<G>>::load(&path).unwrap();
    let mut resumed = build(seed ^ 0xfeed);
    resumed.restore(snapshot);
    assert_eq!(
        run_steps(&mut resumed, steps - split),
        expected[split as usize..]
    );
    assert_eq!(recorder(&resumed).rows, rows_after_split);
}

#[test]
fn asynchronous_configurations_are_deterministic_and_resume_exactly() {
    assert_deterministic_and_resume(async_nowak_may, 30, 11);
    assert_deterministic_and_resume(async_network_fermi, 20, 7);
    assert_deterministic_and_resume(async_public_goods, 30, 13);
}

fn grid_pd_world(
    rules: &RuleBook<PrisonersDilemma, ()>,
    assignment: &Assignment,
) -> GameWorld<PrisonersDilemma> {
    build_world(
        PrisonersDilemma::weak(1.85).unwrap(),
        4,
        (),
        InitialActions::PerAgent(vec![0, 0, 0, 1]),
        rules,
        assignment,
        2,
        3,
        19,
    )
    .unwrap()
    .with_topology(
        Topology::grid(GridSpec {
            rows: 2,
            cols: 2,
            boundary: GridBoundary::Toroidal,
            neighborhood: GridNeighbors::Moore,
        })
        .unwrap(),
    )
    .unwrap()
}

fn assert_population_error<T>(result: Result<T, GameError>, expected: &str) {
    match result {
        Err(GameError::Population(message)) => assert_eq!(message, expected),
        _ => panic!("expected GameError::Population({expected:?})"),
    }
}

#[derive(Clone, Copy, Debug)]
struct CustomOutcome;

impl OutcomeRule<PrisonersDilemma, ()> for CustomOutcome {
    fn name(&self) -> &str {
        "custom"
    }

    fn outcome(
        &self,
        _ctx: &OutcomeContext<'_, PrisonersDilemma, ()>,
        _encounter: EncounterRef<'_>,
        _actions: &[usize],
        payoffs: &mut [f64],
    ) -> socsim_core::Result<()> {
        payoffs.fill(0.0);
        Ok(())
    }
}

#[test]
fn async_assembly_rejects_each_unsupported_configuration() {
    let mut active_rules = RuleBook::new();
    active_rules
        .register("fixed", Arc::new(Fixed::new(0)))
        .unwrap();
    let active = Assignment::Ordered(vec![
        AssignGroup {
            kind: AssignKind::Rule("fixed".to_owned()),
            count: 1,
        },
        AssignGroup {
            kind: AssignKind::Held,
            count: 3,
        },
    ]);
    assert_population_error(
        assemble_async(
            grid_pd_world(&active_rules, &active),
            GridNeighborhood::new().include_self(true),
            Pairwise::new(NormalFormOutcome),
            AsyncPopulationStep::new(ImitateBest::new(Update::Asynchronous)),
            PlayOptions::new(),
            19,
        ),
        "async_population_step applies only to held agents, but an active rule is assigned",
    );

    let rules = RuleBook::new();
    let all_held = held(4);
    assert_population_error(
        assemble_async(
            grid_pd_world(&rules, &all_held),
            RandomPairs::new(),
            Pairwise::new(NormalFormOutcome),
            AsyncPopulationStep::new(FermiUpdate::new(0.1, Update::Asynchronous).unwrap()),
            PlayOptions::new(),
            19,
        ),
        "async_population_step requires a matching with an implicit pattern (grid neighborhood, network edges or whole population); matchings that change partners every step are not supported",
    );
    assert_population_error(
        assemble_async(
            grid_pd_world(&rules, &all_held),
            GridNeighborhood::new().include_self(true),
            Pairwise::new(NormalFormOutcome),
            AsyncPopulationStep::new(ImitateBest::new(Update::Asynchronous)),
            PlayOptions::new().decision_scope(DecisionScope::PerAgentRole),
            19,
        ),
        "async_population_step requires DecisionScope::PerStep",
    );
    assert_population_error(
        assemble_async(
            grid_pd_world(&rules, &all_held),
            GridNeighborhood::new().include_self(true),
            Pairwise::new(NormalFormOutcome),
            AsyncPopulationStep::new(ImitateBest::new(Update::Asynchronous)),
            PlayOptions::new().materialize(true),
            19,
        ),
        "async_population_step does not materialize encounters; PlayOptions::materialize(true) is not supported",
    );
    assert_population_error(
        assemble_async(
            grid_pd_world(&rules, &all_held),
            GridNeighborhood::new().include_self(true),
            Pairwise::new(NormalFormOutcome),
            AsyncPopulationStep::new(ImitateBest::new(Update::Asynchronous)),
            PlayOptions::new().report_mixed(true),
            19,
        ),
        "async_population_step does not support report_mixed",
    );
    assert_population_error(
        assemble_async(
            grid_pd_world(&rules, &all_held),
            GridNeighborhood::new().include_self(true),
            Pairwise::new(CustomOutcome),
            AsyncPopulationStep::new(ImitateBest::new(Update::Asynchronous)),
            PlayOptions::new(),
            19,
        ),
        "async_population_step requires the NormalFormOutcome or counting play path",
    );
    assert_population_error(
        assemble_async(
            grid_pd_world(&rules, &all_held),
            GridNeighborhood::new().include_self(true),
            Pairwise::new(NormalFormOutcome),
            AsyncPopulationStep::new(ImitateBest::new(Update::Asynchronous)),
            PlayOptions::new().on_invalid_action(OnInvalidAction::Fallback(BTreeMap::new())),
            19,
        ),
        "async_population_step does not use on_invalid_action; fallback is not supported",
    );
    assert_population_error(
        assemble_async(
            grid_pd_world(&rules, &all_held),
            GridNeighborhood::new().include_self(true),
            Pairwise::new(NormalFormOutcome),
            AsyncPopulationStep::new(ImitateBest::new(Update::Synchronous)),
            PlayOptions::new(),
            19,
        ),
        "assemble_async requires Update::Asynchronous, got Synchronous",
    );

    let game = PublicGoodsFinite::new(8, 3.0, 1.0).unwrap();
    let pg_rules = RuleBook::new();
    let pg_world = build_world(
        game,
        8,
        (),
        InitialActions::PerAgent(vec![0, 1, 0, 1, 0, 1, 0, 1]),
        &pg_rules,
        &held(8),
        2,
        1,
        19,
    )
    .unwrap();
    assert_population_error(
        assemble_async(
            pg_world,
            WholePopulation::new(),
            counting_play(),
            AsyncPopulationStep::new(
                ProportionalImitation::new(1.0, Update::Asynchronous).unwrap(),
            ),
            PlayOptions::new(),
            19,
        ),
        "replicator_update requires beta <= 1 / 3.25 (the per-step payoff range), got 1",
    );

    let pair_rules = RuleBook::new();
    let no_topology = build_world(
        PrisonersDilemma::weak(1.85).unwrap(),
        2,
        (),
        InitialActions::PerAgent(vec![0, 1]),
        &pair_rules,
        &held(2),
        2,
        1,
        19,
    )
    .unwrap();
    assert_population_error(
        assemble_async(
            no_topology,
            WholePopulation::new(),
            Pairwise::new(NormalFormOutcome),
            AsyncPopulationStep::new(ImitateBest::new(Update::Asynchronous)),
            PlayOptions::new(),
            19,
        ),
        "imitate_best requires a grid or network topology",
    );

    let sync_rules = RuleBook::new();
    let sync_world = build_world(
        PrisonersDilemma::weak(1.85).unwrap(),
        2,
        (),
        InitialActions::PerAgent(vec![0, 1]),
        &sync_rules,
        &held(2),
        2,
        1,
        19,
    )
    .unwrap();
    assert_population_error(
        assemble_with(
            sync_world,
            RandomPairs::new(),
            sync_rules,
            Pairwise::new(NormalFormOutcome),
            FermiUpdate::new(0.1, Update::Asynchronous).unwrap(),
            PlayOptions::new(),
            19,
        ),
        "fermi_update with Update::Asynchronous must be assembled with assemble_async",
    );
}

#[test]
fn async_assembly_preserves_matching_error_types() {
    let rules = RuleBook::new();
    let three_role = PublicGoodsFinite::new(3, 2.0, 1.0).unwrap();
    let world = build_world(
        three_role,
        3,
        (),
        InitialActions::Same(0),
        &rules,
        &held(3),
        2,
        1,
        4,
    )
    .unwrap();
    assert!(matches!(
        assemble_async(
            world,
            RandomPairs::new(),
            Pairwise::new(NormalFormOutcome),
            AsyncPopulationStep::new(FermiUpdate::new(0.1, Update::Asynchronous).unwrap()),
            PlayOptions::new(),
            4,
        ),
        Err(GameError::UnsupportedRoles { n_roles: 3 })
    ));

    let rules = RuleBook::new();
    let world = build_world(
        PrisonersDilemma::weak(1.85).unwrap(),
        3,
        (),
        InitialActions::Same(0),
        &rules,
        &held(3),
        2,
        1,
        4,
    )
    .unwrap();
    assert!(matches!(
        assemble_async(
            world,
            WholePopulation::new(),
            Pairwise::new(NormalFormOutcome),
            AsyncPopulationStep::new(FermiUpdate::new(0.1, Update::Asynchronous).unwrap()),
            PlayOptions::new(),
            4,
        ),
        Err(GameError::InvalidMatching(_))
    ));
}

fn logging_simulation(log_updates: bool) -> Simulation<GameWorld<PrisonersDilemma>> {
    let side = 5;
    let n_agents = side * side;
    let mut initial = vec![COOPERATE; n_agents];
    initial[(side / 2) * side + side / 2] = DEFECT;
    let game = PrisonersDilemma::weak(1.85).unwrap();
    let rules = RuleBook::new();
    let world = build_world(
        game,
        n_agents,
        (),
        InitialActions::PerAgent(initial),
        &rules,
        &held(n_agents),
        2,
        3,
        31,
    )
    .unwrap()
    .with_topology(
        Topology::grid(GridSpec {
            rows: side,
            cols: side,
            boundary: GridBoundary::Toroidal,
            neighborhood: GridNeighbors::Moore,
        })
        .unwrap(),
    )
    .unwrap();
    assemble_async(
        world,
        GridNeighborhood::new().include_self(true),
        Pairwise::new(NormalFormOutcome),
        AsyncPopulationStep::new(ImitateBest::new(Update::Asynchronous)).log_updates(log_updates),
        PlayOptions::new(),
        31,
    )
    .unwrap()
    .scheduler(Box::new(SequentialScheduler))
    .recorder(Box::new(TestRecorder::default()))
    .build()
}

#[test]
fn update_events_are_opt_in_and_include_only_changed_actions() {
    let defaults = AsyncPopulationStep::new(ImitateBest::new(Update::Asynchronous));
    assert_eq!(defaults.activation_mode(), AsyncActivation::AgentOrder);
    assert!(!defaults.updates_logged());

    let mut logged = logging_simulation(true);
    let mut changed = 0;
    for _ in 0..3 {
        let before = logged.world().held().to_vec();
        logged.step().unwrap();
        changed += before
            .iter()
            .zip(logged.world().held())
            .filter(|(before, after)| before != after)
            .count();
    }
    let events = &recorder(&logged).events;
    assert!(changed > 0);
    assert_eq!(events.len(), changed);
    for (t, kind, payload) in events {
        assert!((1..=3).contains(t));
        assert_eq!(kind, "game.async_update");
        assert!(payload["agent"].is_u64());
        assert!(payload["order"].is_u64());
        assert!(payload["from"].is_u64());
        assert!(payload["to"].is_u64());
        assert_ne!(payload["from"], payload["to"]);
    }

    let mut silent = logging_simulation(false);
    for _ in 0..3 {
        silent.step().unwrap();
    }
    assert!(recorder(&silent).events.is_empty());
}

fn continuous_simulation(seed: u64) -> Simulation<GameWorld<PublicGoodsContinuous>> {
    let n_agents = 6;
    let game = PublicGoodsContinuous::new(n_agents, 10.0, 2.5).unwrap();
    let rules = RuleBook::new();
    let world = build_world(
        game,
        n_agents,
        (),
        InitialActions::PerAgent(vec![
            vec![0.0],
            vec![2.0],
            vec![4.0],
            vec![6.0],
            vec![8.0],
            vec![10.0],
        ]),
        &rules,
        &held(n_agents),
        2,
        20,
        seed,
    )
    .unwrap();
    assemble_async(
        world,
        WholePopulation::new(),
        Pairwise::new(NormalFormOutcome),
        AsyncPopulationStep::new(FermiUpdate::new(0.5, Update::Asynchronous).unwrap()),
        PlayOptions::new().payoff_aggregation(PayoffAggregation::Sum),
        seed,
    )
    .unwrap()
    .build()
}

#[test]
fn continuous_fermi_update_is_deterministic() {
    let mut first = continuous_simulation(73);
    let mut second = continuous_simulation(73);
    for _ in 0..20 {
        first.step().unwrap();
        second.step().unwrap();
        assert_eq!(
            serde_json::to_string(first.world()).unwrap(),
            serde_json::to_string(second.world()).unwrap()
        );
    }
}

fn descriptions_by_pattern(descriptions: Vec<(String, String)>) -> BTreeMap<String, String> {
    descriptions.into_iter().collect()
}

#[test]
fn payoff_timing_changes_only_post_update_descriptions() {
    const POST_UPDATE: &str =
        "the actions held after every update of the step (asynchronous update)";

    let finite_game = PrisonersDilemma::weak(1.85).unwrap();
    let finite_default = GameMetrics::new(&finite_game, MetricsOptions::new().cooperate_label("C"))
        .unwrap()
        .describe();
    let finite_before = GameMetrics::new(
        &finite_game,
        MetricsOptions::new()
            .cooperate_label("C")
            .payoff_timing(PayoffTiming::BeforeUpdate),
    )
    .unwrap()
    .describe();
    assert_eq!(finite_default, finite_before);

    let finite_default = descriptions_by_pattern(finite_default);
    assert_eq!(
        finite_default["game.played_share.{action}"],
        "Share of all played seats choosing `{action}`. `{action}` is a metric_id; display labels come from the game's metric actions. Actions are pooled across roles."
    );
    assert_eq!(
        finite_default["game.mean_payoff"],
        "Mean aggregated payoff among agents who played at least once in the step."
    );
    assert_eq!(
        finite_default["game.payoff_sd"],
        "Population standard deviation of aggregated payoff among agents who played at least once in the step."
    );
    assert_eq!(
        finite_default["game.coop_rate"],
        "Share of all played seats choosing the configured cooperation label for that seat's role."
    );

    let finite_after = descriptions_by_pattern(
        GameMetrics::new(
            &finite_game,
            MetricsOptions::new()
                .cooperate_label("C")
                .payoff_timing(PayoffTiming::AfterUpdate),
        )
        .unwrap()
        .describe(),
    );
    for pattern in [
        "game.played_share.{action}",
        "game.mean_payoff",
        "game.payoff_sd",
        "game.coop_rate",
    ] {
        assert!(finite_after[pattern].contains(POST_UPDATE));
        assert_ne!(finite_after[pattern], finite_default[pattern]);
    }
    for (pattern, description) in &finite_default {
        if ![
            "game.played_share.{action}",
            "game.mean_payoff",
            "game.payoff_sd",
            "game.coop_rate",
        ]
        .contains(&pattern.as_str())
        {
            assert_eq!(finite_after[pattern], *description);
        }
    }

    let continuous_game = PublicGoodsContinuous::new(6, 10.0, 2.5).unwrap();
    let continuous_default =
        ContinuousMetrics::new(&continuous_game, ContinuousMetricsOptions::new())
            .unwrap()
            .describe();
    let continuous_before = ContinuousMetrics::new(
        &continuous_game,
        ContinuousMetricsOptions::new().payoff_timing(PayoffTiming::BeforeUpdate),
    )
    .unwrap()
    .describe();
    assert_eq!(continuous_default, continuous_before);

    let continuous_default = descriptions_by_pattern(continuous_default);
    assert_eq!(
        continuous_default["game.mean_action.{d}"],
        "Mean of component d of the actions over all seats played in the step, pooled across roles."
    );
    assert_eq!(
        continuous_default["game.action_sd.{d}"],
        "Population standard deviation of component d of the actions over all seats played in the step, pooled across roles."
    );

    let continuous_after = descriptions_by_pattern(
        ContinuousMetrics::new(
            &continuous_game,
            ContinuousMetricsOptions::new().payoff_timing(PayoffTiming::AfterUpdate),
        )
        .unwrap()
        .describe(),
    );
    for pattern in [
        "game.mean_action.{d}",
        "game.action_sd.{d}",
        "game.mean_payoff",
        "game.payoff_sd",
    ] {
        assert!(continuous_after[pattern].contains(POST_UPDATE));
        assert_ne!(continuous_after[pattern], continuous_default[pattern]);
    }
    for (pattern, description) in &continuous_default {
        if ![
            "game.mean_action.{d}",
            "game.action_sd.{d}",
            "game.mean_payoff",
            "game.payoff_sd",
        ]
        .contains(&pattern.as_str())
        {
            assert_eq!(continuous_after[pattern], *description);
        }
    }
}
