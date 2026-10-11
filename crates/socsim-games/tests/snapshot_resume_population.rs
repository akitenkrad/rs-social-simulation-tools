//! A simulation with population dynamics restored from a snapshot into a newly
//! assembled simulation continues exactly like the uninterrupted run,
//! including grid adjacency and network enumeration order rebuilt on load.

use std::any::Any;

use rand::seq::SliceRandom;
use socsim_core::{derive_seed, AgentId, Recorder, SimRng};
use socsim_engine::{Simulation, SimulationBuilder, Snapshot};
use socsim_games::catalog::{HawkDove, PrisonersDilemma};
use socsim_games::{
    assemble_with, build_world, AssignGroup, AssignKind, Assignment, FermiUpdate, FiniteGame,
    Fitness, GameMetrics, GameWorld, GridBoundary, GridNeighborhood, GridNeighbors, GridSpec,
    ImitateBest, InitialActions, MetricsOptions, MoranUpdate, NetworkEdges, NormalFormOutcome,
    Pairwise, PlayOptions, ProportionalImitation, RandomPairs, RuleBook, Topology, Update,
};

#[derive(Default)]
struct RowRecorder {
    rows: Vec<(u64, String, u64)>,
}

impl Recorder for RowRecorder {
    fn record_metric(&mut self, t: u64, key: &str, value: f64) {
        self.rows.push((t, key.to_owned(), value.to_bits()));
    }

    fn record_event(&mut self, _t: u64, _kind: &str, _payload: serde_json::Value) {}

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

fn held(n_agents: usize) -> Assignment {
    Assignment::Ordered(vec![AssignGroup {
        kind: AssignKind::Held,
        count: n_agents,
    }])
}

/// Places exactly `count` agents with action `focal` uniformly at random.
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

/// Builds a uniformly random simple `degree`-regular graph by the pairing model.
fn random_regular(n_agents: usize, degree: usize, seed: u64) -> socsim_net::SocialNetwork {
    let mut rng = SimRng::from_seed(derive_seed(seed, &[0, 1]));
    loop {
        let mut stubs: Vec<usize> = (0..n_agents)
            .flat_map(|agent| std::iter::repeat_n(agent, degree))
            .collect();
        stubs.shuffle(&mut rng);
        let mut edges = std::collections::BTreeSet::new();
        let simple = stubs.chunks_exact(2).all(|pair| {
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
        // Insert edges in pairing order, not sorted order.
        for pair in stubs.chunks_exact(2) {
            network.add_edge(AgentId(pair[0] as u64), AgentId(pair[1] as u64));
        }
        return network;
    }
}

fn finish<G: FiniteGame>(
    builder: SimulationBuilder<GameWorld<G>>,
    game: &G,
) -> Simulation<GameWorld<G>> {
    let metrics = GameMetrics::new(game, MetricsOptions::new().population_share(true)).unwrap();
    builder
        .add_mechanism(Box::new(metrics))
        .recorder(Box::new(RowRecorder::default()))
        .build()
}

/// Nowak–May: 50 x 50 torus, Moore neighbors with self-play, weak prisoner's
/// dilemma with b = 1.85, 10% initial defectors, imitate-the-best.
fn nowak_may(seed: u64) -> Simulation<GameWorld<PrisonersDilemma>> {
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
    let builder = assemble_with(
        world,
        GridNeighborhood::new().include_self(true),
        rules,
        Pairwise::new(NormalFormOutcome),
        ImitateBest::new(Update::Synchronous),
        PlayOptions::new(),
        seed,
    )
    .unwrap();
    finish(builder, &game)
}

/// Well-mixed hawk-dove with proportional imitation.
fn hawk_dove(seed: u64) -> Simulation<GameWorld<HawkDove>> {
    let n_agents = 1_000;
    let game = HawkDove::new(2.0, 4.0).unwrap();
    let rules = RuleBook::new();
    let world = build_world(
        game.clone(),
        n_agents,
        (),
        InitialActions::PerAgent(initial_actions(n_agents, 300, 0, 1, seed)),
        &rules,
        &held(n_agents),
        2,
        40,
        seed,
    )
    .unwrap();
    let builder = assemble_with(
        world,
        RandomPairs::new(),
        rules,
        Pairwise::new(NormalFormOutcome),
        ProportionalImitation::new(1.0 / 3.0, Update::Synchronous).unwrap(),
        PlayOptions::new(),
        seed,
    )
    .unwrap();
    finish(builder, &game)
}

/// Random 4-regular network of 200 vertices, weak prisoner's dilemma with
/// b = 1.5, Fermi update with K = 0.1.
fn network_fermi(seed: u64) -> Simulation<GameWorld<PrisonersDilemma>> {
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
    let builder = assemble_with(
        world,
        NetworkEdges::new(),
        rules,
        Pairwise::new(NormalFormOutcome),
        FermiUpdate::new(0.1, Update::Synchronous).unwrap(),
        PlayOptions::new(),
        seed,
    )
    .unwrap();
    finish(builder, &game)
}

/// Moran process with exponential fitness and mutation.
fn moran(seed: u64) -> Simulation<GameWorld<HawkDove>> {
    let n_agents = 50;
    let game = HawkDove::new(2.0, 4.0).unwrap();
    let rules = RuleBook::new();
    let world = build_world(
        game.clone(),
        n_agents,
        (),
        InitialActions::PerAgent(initial_actions(n_agents, 10, 0, 1, seed)),
        &rules,
        &held(n_agents),
        2,
        60,
        seed,
    )
    .unwrap();
    let builder = assemble_with(
        world,
        RandomPairs::new(),
        rules,
        Pairwise::new(NormalFormOutcome),
        MoranUpdate::new(Fitness::Exponential { w: 1.0 }, 0.05, Update::Synchronous).unwrap(),
        PlayOptions::new(),
        seed,
    )
    .unwrap();
    finish(builder, &game)
}

fn rows<G: socsim_games::StageGame>(
    simulation: &Simulation<GameWorld<G>>,
) -> Vec<(u64, String, u64)> {
    simulation
        .recorder()
        .as_any()
        .unwrap()
        .downcast_ref::<RowRecorder>()
        .unwrap()
        .rows
        .clone()
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

fn assert_resume<G>(build: impl Fn(u64) -> Simulation<GameWorld<G>>, steps: u64, split: u64)
where
    G: FiniteGame + Clone + serde::Serialize + serde::de::DeserializeOwned,
{
    let seed = 20261011;
    let mut uninterrupted = build(seed);
    let expected = run_steps(&mut uninterrupted, steps);
    let expected_rows: Vec<_> = rows(&uninterrupted)
        .into_iter()
        // Rows are labelled with the 1-based step number.
        .filter(|(t, _, _)| *t > split)
        .collect();
    assert!(!expected_rows.is_empty());

    // In-memory snapshot restored into a simulation assembled with another seed.
    let mut partial = build(seed);
    run_steps(&mut partial, split);
    let mut resumed = build(seed ^ 0x5eed);
    resumed.restore(partial.snapshot());
    let actual = run_steps(&mut resumed, steps - split);
    assert_eq!(actual, expected[split as usize..]);
    assert_eq!(rows(&resumed), expected_rows);

    // Snapshot saved to and loaded from JSON, which rebuilds the topology caches.
    let mut partial = build(seed);
    run_steps(&mut partial, split);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("snapshot.json");
    partial.snapshot().save(&path).unwrap();
    let snapshot = Snapshot::<GameWorld<G>>::load(&path).unwrap();
    let mut resumed = build(seed ^ 0xfeed);
    resumed.restore(snapshot);
    let actual = run_steps(&mut resumed, steps - split);
    assert_eq!(actual, expected[split as usize..]);
    assert_eq!(rows(&resumed), expected_rows);
}

#[test]
fn nowak_may_resumes_from_a_snapshot() {
    assert_resume(nowak_may, 30, 11);
}

#[test]
fn random_pairs_hawk_dove_resumes_from_a_snapshot() {
    assert_resume(hawk_dove, 40, 17);
}

#[test]
fn network_fermi_update_resumes_from_a_snapshot() {
    assert_resume(network_fermi, 20, 7);
}

#[test]
fn moran_update_resumes_from_a_snapshot() {
    assert_resume(moran, 60, 23);
}
