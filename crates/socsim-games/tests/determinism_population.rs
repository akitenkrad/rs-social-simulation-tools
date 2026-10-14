//! Population dynamics are deterministic for a fixed seed: every metric row and
//! the world after every step agree between two runs.

use std::any::Any;

use rand::seq::SliceRandom;
use socsim_core::{derive_seed, AgentId, Recorder, SimRng};
use socsim_engine::{Simulation, SimulationBuilder};
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
        // Insert edges in pairing order, not sorted order.
        for pair in stubs.as_chunks::<2>().0 {
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

struct Trace {
    worlds: Vec<String>,
    rows: Vec<(u64, String, u64)>,
}

fn trace<G>(mut simulation: Simulation<GameWorld<G>>, steps: u64) -> Trace
where
    G: FiniteGame + serde::Serialize,
{
    let worlds = (0..steps)
        .map(|_| {
            simulation.step().unwrap();
            serde_json::to_string(simulation.world()).unwrap()
        })
        .collect();
    let rows = simulation
        .recorder()
        .as_any()
        .unwrap()
        .downcast_ref::<RowRecorder>()
        .unwrap()
        .rows
        .clone();
    Trace { worlds, rows }
}

fn assert_deterministic<G>(build: impl Fn(u64) -> Simulation<GameWorld<G>>, steps: u64)
where
    G: FiniteGame + serde::Serialize,
{
    let first = trace(build(20261011), steps);
    let second = trace(build(20261011), steps);
    assert_eq!(first.worlds.len(), steps as usize);
    assert!(first
        .rows
        .iter()
        .any(|(_, column, _)| column == "game.population_share.a0"));
    assert_eq!(first.rows, second.rows);
    assert_eq!(first.worlds, second.worlds);

    let other = trace(build(7), steps);
    assert_ne!(first.worlds, other.worlds);
}

#[test]
fn nowak_may_is_deterministic() {
    assert_deterministic(nowak_may, 30);
}

#[test]
fn random_pairs_hawk_dove_is_deterministic() {
    assert_deterministic(hawk_dove, 40);
}

#[test]
fn network_fermi_update_is_deterministic() {
    assert_deterministic(network_fermi, 20);
}

#[test]
fn moran_update_is_deterministic() {
    assert_deterministic(moran, 60);
}

#[test]
fn population_share_reports_post_update_held_actions() {
    let mut simulation = hawk_dove(3);
    let mut held_shares = Vec::new();
    let mut played_shares = Vec::new();
    for _ in 0..10 {
        let before = simulation.world().held().to_vec();
        simulation.step().unwrap();
        let world = simulation.world();
        let count = |held: &[usize]| held.iter().filter(|&&a| a == 0).count() as f64;
        held_shares.push(count(world.held()) / world.held().len() as f64);
        played_shares.push(count(&before) / before.len() as f64);
    }
    let rows = &simulation
        .recorder()
        .as_any()
        .unwrap()
        .downcast_ref::<RowRecorder>()
        .unwrap()
        .rows;
    let column = |name: &str| -> Vec<f64> {
        rows.iter()
            .filter(|(_, column, _)| column == name)
            .map(|(_, _, bits)| f64::from_bits(*bits))
            .collect()
    };
    assert_eq!(column("game.population_share.a0"), held_shares);
    assert_eq!(column("game.played_share.a0"), played_shares);
    assert_ne!(held_shares, played_shares);
}
