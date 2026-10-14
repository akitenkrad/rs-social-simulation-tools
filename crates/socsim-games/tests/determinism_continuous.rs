//! Determinism and metric columns of continuous games.
//!
//! Configuration (1) is a Cournot oligopoly whose firms all play a damped best
//! response against the whole population. Configuration (2) is a continuous
//! public-goods game played in random groups of four by ten gradient-ascent
//! agents, so two agents sit out every step.

use std::any::Any;
use std::sync::Arc;

use rand::Rng;
use socsim_core::{derive_seed, Recorder, SimRng};
use socsim_engine::Simulation;
use socsim_games::continuous::{Cournot, PublicGoodsContinuous};
use socsim_games::{
    assemble_with_aggregates, build_world, AssignGroup, AssignKind, Assignment,
    ContinuousEquilibrium, ContinuousGame, ContinuousMetrics, ContinuousMetricsOptions,
    DampedBestResponse, DecisionRule, GameError, GameWorld, GradientAscent, InitialActions,
    NormalFormOutcome, PlayOptions, RandomGroups, RuleBook, WholePopulation,
};

#[path = "support/metric_patterns.rs"]
mod metric_patterns;

const STEPS: u64 = 60;

type Rows = Vec<(u64, String, u64)>;

#[derive(Default)]
struct RowRecorder {
    rows: Rows,
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

fn everyone(kind: &str, n_agents: usize) -> Assignment {
    Assignment::Ordered(vec![AssignGroup {
        kind: AssignKind::Rule(kind.to_owned()),
        count: n_agents,
    }])
}

fn cournot_game() -> Cournot {
    Cournot::new(3, 100.0, 1.1, 10.0, 100.0).unwrap()
}

fn cournot_options() -> ContinuousMetricsOptions {
    ContinuousMetricsOptions::new()
        .equilibrium(ContinuousEquilibrium::Solve)
        .stop(1.0e-4, 5)
}

/// Configuration (1).
fn cournot_simulation(
    seed: u64,
    options: ContinuousMetricsOptions,
) -> Simulation<GameWorld<Cournot>> {
    let game = cournot_game();
    let metrics = ContinuousMetrics::new(&game, options).unwrap();
    let mut rules = RuleBook::new();
    let rule: Arc<dyn DecisionRule<Cournot, ()>> =
        Arc::new(DampedBestResponse::<Cournot>::new(0.5).unwrap());
    rules.register("damped", rule).unwrap();
    let world = build_world(
        game,
        3,
        (),
        InitialActions::PerAgent(vec![vec![0.0], vec![50.0], vec![100.0]]),
        &rules,
        &everyone("damped", 3),
        4,
        STEPS,
        seed,
    )
    .unwrap();
    assemble_with_aggregates(
        world,
        WholePopulation::new(),
        rules,
        NormalFormOutcome,
        PlayOptions::new(),
        seed,
        4,
    )
    .unwrap()
    .add_mechanism(Box::new(metrics))
    .recorder(Box::new(RowRecorder::default()))
    .build()
}

fn public_goods_game() -> PublicGoodsContinuous {
    PublicGoodsContinuous::new(4, 10.0, 2.5).unwrap()
}

/// Initial contributions drawn uniformly from `[0, e]` with a stream derived
/// from the seed.
fn public_goods_initial(seed: u64) -> Vec<Vec<f64>> {
    let mut rng = SimRng::from_seed(derive_seed(seed, &[0, 2]));
    (0..10).map(|_| vec![rng.gen_range(0.0..=10.0)]).collect()
}

/// Configuration (2).
fn public_goods_simulation(seed: u64) -> Simulation<GameWorld<PublicGoodsContinuous>> {
    let game = public_goods_game();
    let metrics = ContinuousMetrics::new(
        &game,
        ContinuousMetricsOptions::new()
            .equilibrium(ContinuousEquilibrium::Given(vec![vec![0.0]; 4]))
            .stop(1.0e-9, 3),
    )
    .unwrap();
    let mut rules = RuleBook::new();
    let rule: Arc<dyn DecisionRule<PublicGoodsContinuous, ()>> =
        Arc::new(GradientAscent::new(0.3).unwrap());
    rules.register("gradient", rule).unwrap();
    let world = build_world(
        game,
        10,
        (),
        InitialActions::PerAgent(public_goods_initial(seed)),
        &rules,
        &everyone("gradient", 10),
        4,
        STEPS,
        seed,
    )
    .unwrap();
    assemble_with_aggregates(
        world,
        RandomGroups::new(4).unwrap(),
        rules,
        NormalFormOutcome,
        PlayOptions::new(),
        seed,
        4,
    )
    .unwrap()
    .add_mechanism(Box::new(metrics))
    .recorder(Box::new(RowRecorder::default()))
    .build()
}

fn rows<G: ContinuousGame>(simulation: &Simulation<GameWorld<G>>) -> Rows {
    simulation
        .recorder()
        .as_any()
        .unwrap()
        .downcast_ref::<RowRecorder>()
        .unwrap()
        .rows
        .clone()
}

fn value(rows: &Rows, t: u64, key: &str) -> Option<f64> {
    rows.iter()
        .find(|(step, column, _)| *step == t && column == key)
        .map(|(_, _, bits)| f64::from_bits(*bits))
}

/// Runs the simulation and returns every step's world as JSON.
fn run<G>(simulation: &mut Simulation<GameWorld<G>>) -> Vec<String>
where
    G: ContinuousGame + serde::Serialize,
{
    let mut worlds = Vec::new();
    while !simulation.stop_requested()
        && socsim_core::WorldState::clock(simulation.world()).t() < STEPS
    {
        simulation.step().unwrap();
        worlds.push(serde_json::to_string(simulation.world()).unwrap());
    }
    worlds
}

#[test]
fn cournot_damped_best_response_is_deterministic() {
    let mut first = cournot_simulation(41, cournot_options());
    let mut second = cournot_simulation(41, cournot_options());
    let first_worlds = run(&mut first);
    let second_worlds = run(&mut second);

    assert!(!first_worlds.is_empty());
    assert_eq!(first_worlds, second_worlds);
    assert_eq!(rows(&first), rows(&second));
    assert_eq!(first.stop_requested(), second.stop_requested());
}

#[test]
fn public_goods_gradient_ascent_in_random_groups_is_deterministic() {
    let mut first = public_goods_simulation(43);
    let mut second = public_goods_simulation(43);
    let first_worlds = run(&mut first);
    let second_worlds = run(&mut second);

    assert_eq!(first_worlds.len(), STEPS as usize);
    assert_eq!(first_worlds, second_worlds);
    assert_eq!(rows(&first), rows(&second));
}

#[test]
fn cournot_metric_columns_match_the_first_step_by_hand() {
    let mut simulation = cournot_simulation(47, cournot_options());
    simulation.step().unwrap();
    let rows = rows(&simulation);
    let game = cournot_game();
    let q_star = game.equilibrium_quantity();
    let tolerance = 1.0e-9;

    // The first step plays the initial actions [0, 50, 100].
    let mean = value(&rows, 1, "game.mean_action.0").unwrap();
    assert!((mean - 50.0).abs() <= tolerance, "{mean}");
    let sd = value(&rows, 1, "game.action_sd.0").unwrap();
    let expected_sd = (5000.0_f64 / 3.0).sqrt();
    assert!((sd - expected_sd).abs() <= tolerance, "{sd}");

    let total = 150.0;
    let price = 100.0 - 1.1 * total;
    let payoffs = [
        0.0 * (price - 10.0),
        50.0 * (price - 10.0),
        100.0 * (price - 10.0),
    ];
    let mean_payoff = payoffs.iter().sum::<f64>() / 3.0;
    let payoff_sd = (payoffs
        .iter()
        .map(|payoff| (payoff - mean_payoff).powi(2))
        .sum::<f64>()
        / 3.0)
        .sqrt();
    let actual = value(&rows, 1, "game.mean_payoff").unwrap();
    assert!((actual - mean_payoff).abs() <= tolerance, "{actual}");
    let actual = value(&rows, 1, "game.payoff_sd").unwrap();
    assert!((actual - payoff_sd).abs() <= tolerance, "{actual}");

    let actual = value(&rows, 1, "game.aggregate.total_output").unwrap();
    assert!((actual - total).abs() <= tolerance, "{actual}");
    let actual = value(&rows, 1, "game.aggregate.price").unwrap();
    assert!((actual - price).abs() <= tolerance, "{actual}");

    // The solved equilibrium is the analytic one, and the distance sums the
    // per-role L1 distances.
    let distance = value(&rows, 1, "game.eq_distance").unwrap();
    let expected = (0.0 - q_star).abs() + (50.0 - q_star).abs() + (100.0 - q_star).abs();
    assert!(
        (distance - expected).abs() <= tolerance,
        "{distance} vs {expected}"
    );
}

#[test]
fn solved_equilibrium_is_the_analytic_cournot_quantity() {
    let game = cournot_game();
    let metrics = ContinuousMetrics::new(
        &game,
        ContinuousMetricsOptions::new().equilibrium(ContinuousEquilibrium::Solve),
    )
    .unwrap();
    let q_star = game.equilibrium_quantity();
    for action in metrics.equilibrium().unwrap() {
        assert_eq!(action.len(), 1);
        assert!((action[0] - q_star).abs() <= 1.0e-8, "{}", action[0]);
    }
}

#[test]
fn public_goods_columns_cover_the_played_seats_only() {
    let mut simulation = public_goods_simulation(53);
    simulation.step().unwrap();
    let rows = rows(&simulation);
    // Two random groups of four play; the eight seats define the columns.
    let mean = value(&rows, 1, "game.mean_action.0").unwrap();
    let total = value(&rows, 1, "game.aggregate.total_contribution").unwrap();
    assert!((mean * 8.0 - total).abs() <= 1.0e-9, "{mean} {total}");
    assert!(value(&rows, 1, "game.action_sd.0").is_some());
    assert!(value(&rows, 1, "game.mean_payoff").is_some());
    assert!(value(&rows, 1, "game.payoff_sd").is_some());
    let distance = value(&rows, 1, "game.eq_distance").unwrap();
    assert!(distance >= 0.0);
    assert_eq!(
        simulation
            .world()
            .n_played()
            .iter()
            .filter(|&&n| n > 0)
            .count(),
        8
    );
}

#[test]
fn convergence_is_judged_per_agent_not_on_the_mean_action() {
    let mut simulation = cournot_simulation(59, cournot_options());
    run(&mut simulation);
    assert!(
        simulation.stop_requested(),
        "the run must stop before t_max"
    );
    let rows = rows(&simulation);
    let stop = socsim_core::WorldState::clock(simulation.world()).t();
    assert!(stop < STEPS);

    // The criterion on the mean action alone would have fired earlier: the
    // damped simultaneous iteration settles the mean long before the firms.
    let means: Vec<f64> = (1..=stop)
        .map(|t| value(&rows, t, "game.mean_action.0").unwrap())
        .collect();
    let mut below = 0;
    let mut mean_stop = None;
    for t in 1..means.len() {
        if (means[t] - means[t - 1]).abs() < 1.0e-4 {
            below += 1;
        } else {
            below = 0;
        }
        if below >= 5 {
            mean_stop = Some(t as u64 + 1);
            break;
        }
    }
    let mean_stop = mean_stop.expect("the mean action settles before the firms do");
    assert!(mean_stop < stop, "mean {mean_stop} vs agents {stop}");

    // At the stop, the last per-agent changes are below the tolerance.
    let held = simulation.world().held();
    let q_star = cournot_game().equilibrium_quantity();
    for action in held {
        assert!((action[0] - q_star).abs() < 1.0e-2, "{}", action[0]);
    }
}

#[test]
fn invalid_stop_options_are_rejected() {
    let game = cournot_game();
    for (tol, window) in [
        (0.0, 1),
        (-1.0, 1),
        (f64::NAN, 1),
        (f64::INFINITY, 1),
        (1.0e-6, 0),
    ] {
        let result =
            ContinuousMetrics::new(&game, ContinuousMetricsOptions::new().stop(tol, window));
        assert!(
            matches!(result, Err(GameError::Metrics(_))),
            "{tol} {window}"
        );
    }
}

#[test]
fn given_equilibrium_must_match_the_game_dimensions() {
    let game = cournot_game();
    for profile in [
        vec![vec![1.0]; 2],
        vec![vec![1.0], vec![1.0], vec![1.0, 2.0]],
        vec![vec![1.0], vec![f64::NAN], vec![1.0]],
    ] {
        let result = ContinuousMetrics::new(
            &game,
            ContinuousMetricsOptions::new().equilibrium(ContinuousEquilibrium::Given(profile)),
        );
        assert!(matches!(result, Err(GameError::Metrics(_))));
    }
}

#[test]
fn describe_lists_the_continuous_columns() {
    let game = cournot_game();
    let metrics = ContinuousMetrics::new(
        &game,
        ContinuousMetricsOptions::new().equilibrium(ContinuousEquilibrium::Solve),
    )
    .unwrap();
    let names: Vec<_> = metrics
        .describe()
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert_eq!(
        names,
        [
            "game.mean_action.{d}",
            "game.action_sd.{d}",
            "game.mean_payoff",
            "game.payoff_sd",
            "game.aggregate.{name}",
            "game.eq_distance",
        ]
    );
}

/// Two roles with different dimensions: role 0 picks one number, role 1 a pair.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct Mixed1And2;

const BOUNDS_1: [(f64, f64); 1] = [(0.0, 1.0)];
const BOUNDS_2: [(f64, f64); 2] = [(0.0, 1.0), (0.0, 1.0)];

impl socsim_games::StageGame for Mixed1And2 {
    type Action = Vec<f64>;

    fn n_roles(&self) -> usize {
        2
    }

    fn check_action(
        &self,
        role: usize,
        action: &Vec<f64>,
    ) -> Result<(), socsim_games::ActionError> {
        socsim_games::check_continuous_action(self, role, action)
    }
}

impl socsim_games::NormalFormGame for Mixed1And2 {
    fn payoff(&self, role: usize, profile: &[Vec<f64>]) -> f64 {
        profile[role].iter().sum()
    }
}

impl ContinuousGame for Mixed1And2 {
    fn bounds(&self, role: usize) -> &[(f64, f64)] {
        if role == 0 {
            &BOUNDS_1
        } else {
            &BOUNDS_2
        }
    }
}

#[test]
fn roles_with_different_dimensions_are_reported_per_role() {
    let game = Mixed1And2;
    let metrics = ContinuousMetrics::new(&game, ContinuousMetricsOptions::new()).unwrap();
    // That supported construction paths cannot reach these columns is covered
    // by the regression tests in `continuous_role_dimensions.rs`.
    let names: Vec<_> = metrics
        .describe()
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert_eq!(names[0], "game.mean_action.{role_component}");
    assert_eq!(names[1], "game.action_sd.{role_component}");
}

#[test]
fn describe_patterns_cover_every_pooled_column_recorded() {
    let metrics = ContinuousMetrics::new(&cournot_game(), cournot_options()).unwrap();
    let described = metrics.describe();
    let mut simulation = cournot_simulation(3, cournot_options());
    run(&mut simulation);
    let rows = rows(&simulation);
    metric_patterns::assert_patterns_cover(
        &described,
        rows.iter().map(|(_, name, _)| name.as_str()),
    );
}

#[test]
fn describe_patterns_cover_the_per_role_column_names() {
    let metrics = ContinuousMetrics::new(
        &Mixed1And2,
        ContinuousMetricsOptions::new().equilibrium(ContinuousEquilibrium::Given(vec![
            vec![0.5],
            vec![0.5, 0.5],
        ])),
    )
    .unwrap();
    // That supported construction paths cannot reach these columns is covered
    // by the regression tests in `continuous_role_dimensions.rs`. The names
    // below are the per-role form for role 0 with one component and role 1 with
    // two.
    let names = [
        "game.mean_action.r0-0",
        "game.mean_action.r1-0",
        "game.mean_action.r1-1",
        "game.action_sd.r0-0",
        "game.action_sd.r1-0",
        "game.action_sd.r1-1",
        "game.mean_payoff",
        "game.payoff_sd",
        "game.aggregate.total",
        "game.eq_distance",
    ];
    metric_patterns::assert_patterns_cover(&metrics.describe(), names);
}

// Implicit and materialized play paths give the same metric rows.

/// Held agents only, so the implicit path is taken unless the arena is
/// materialized.
fn held_world<G: ContinuousGame>(game: G, initial: Vec<Vec<f64>>) -> GameWorld<G> {
    let n_agents = initial.len();
    build_world(
        game,
        n_agents,
        (),
        InitialActions::PerAgent(initial),
        &RuleBook::new(),
        &Assignment::Ordered(vec![AssignGroup {
            kind: AssignKind::Held,
            count: n_agents,
        }]),
        2,
        3,
        67,
    )
    .unwrap()
}

/// Runs three steps on the implicit or the materialized path and returns the
/// metric rows and the number of arena encounters after the last step.
fn path_rows<G, M>(
    world: GameWorld<G>,
    matching: M,
    equilibrium: Vec<Vec<f64>>,
    materialize: bool,
) -> (Rows, usize)
where
    G: ContinuousGame + socsim_games::AggregateGame,
    M: socsim_games::Matching<G, ()> + 'static,
{
    let metrics = ContinuousMetrics::new(
        world.game(),
        ContinuousMetricsOptions::new()
            .equilibrium(ContinuousEquilibrium::Given(equilibrium))
            .stop(1.0e-9, 1),
    )
    .unwrap();
    let mut simulation = assemble_with_aggregates(
        world,
        matching,
        RuleBook::new(),
        NormalFormOutcome,
        PlayOptions::new().materialize(materialize),
        67,
        2,
    )
    .unwrap()
    .add_mechanism(Box::new(metrics))
    .recorder(Box::new(RowRecorder::default()))
    .build();
    while socsim_core::WorldState::clock(simulation.world()).t() < 3 {
        simulation.step().unwrap();
    }
    let encounters = simulation.world().plays().n_encounters();
    (rows(&simulation), encounters)
}

fn assert_paths_agree<G, M>(
    world: impl Fn() -> GameWorld<G>,
    matching: impl Fn() -> M,
    equilibrium: Vec<Vec<f64>>,
) where
    G: ContinuousGame + socsim_games::AggregateGame,
    M: socsim_games::Matching<G, ()> + 'static,
{
    let (implicit, implicit_encounters) =
        path_rows(world(), matching(), equilibrium.clone(), false);
    let (materialized, materialized_encounters) = path_rows(world(), matching(), equilibrium, true);
    assert_eq!(implicit_encounters, 0, "the implicit path must be taken");
    assert!(materialized_encounters > 0);
    for column in [
        "game.mean_action.0",
        "game.action_sd.0",
        "game.mean_payoff",
        "game.payoff_sd",
        "game.eq_distance",
    ] {
        assert!(
            implicit.iter().any(|(t, key, _)| *t == 3 && key == column),
            "{column}"
        );
    }
    assert!(implicit
        .iter()
        .any(|(_, key, _)| key.starts_with("game.aggregate.")));
    assert_eq!(implicit, materialized);
}

/// Contributions that are not exactly representable in binary and differ per
/// agent.
#[cfg(any(feature = "grid", feature = "net"))]
fn contributions(n_agents: usize) -> Vec<Vec<f64>> {
    (0..n_agents)
        .map(|agent| vec![(agent as f64 * 1.37 + 0.1) % 10.0])
        .collect()
}

#[test]
fn whole_population_implicit_and_materialized_paths_agree() {
    assert_paths_agree(
        || held_world(cournot_game(), vec![vec![0.1], vec![50.3], vec![99.7]]),
        WholePopulation::new,
        vec![vec![90.0 / 4.4]; 3],
    );
}

#[cfg(feature = "grid")]
#[test]
fn grid_neighborhood_implicit_and_materialized_paths_agree() {
    use socsim_games::{GridBoundary, GridNeighborhood, GridNeighbors, GridSpec, Topology};

    for include_self in [false, true] {
        for boundary in [GridBoundary::Toroidal, GridBoundary::Fixed] {
            let world = || {
                held_world(
                    PublicGoodsContinuous::new(2, 10.0, 1.5).unwrap(),
                    contributions(16),
                )
                .with_topology(
                    Topology::grid(GridSpec {
                        rows: 4,
                        cols: 4,
                        boundary,
                        neighborhood: GridNeighbors::Moore,
                    })
                    .unwrap(),
                )
                .unwrap()
            };
            assert_paths_agree(
                world,
                || GridNeighborhood::new().include_self(include_self),
                vec![vec![0.0]; 2],
            );
        }
    }
}

#[cfg(feature = "net")]
#[test]
fn network_edges_implicit_and_materialized_paths_agree() {
    use socsim_core::AgentId;
    use socsim_games::{NetworkEdges, Topology};

    let world = || {
        let mut network = socsim_net::SocialNetwork::empty();
        for id in 0..8 {
            network.add_node(AgentId(id));
        }
        for (a, b) in [
            (0, 1),
            (1, 2),
            (2, 3),
            (3, 4),
            (4, 5),
            (5, 6),
            (6, 7),
            (7, 0),
            (0, 4),
            (2, 6),
            (5, 1),
        ] {
            network.add_edge(AgentId(a), AgentId(b));
        }
        held_world(
            PublicGoodsContinuous::new(2, 10.0, 1.5).unwrap(),
            contributions(8),
        )
        .with_topology(Topology::network(network))
        .unwrap()
    };
    assert_paths_agree(world, NetworkEdges::new, vec![vec![0.0]; 2]);
}

// Configuration (3): configuration (2) with held agents and a Moran process
// with Gaussian mutation, so that the population dynamic consumes randomness.

fn moran_public_goods_simulation(seed: u64) -> Simulation<GameWorld<PublicGoodsContinuous>> {
    let game = public_goods_game();
    let metrics = ContinuousMetrics::new(
        &game,
        ContinuousMetricsOptions::new()
            .equilibrium(ContinuousEquilibrium::Given(vec![vec![0.0]; 4]))
            .stop(1.0e-9, 3),
    )
    .unwrap();
    let rules = RuleBook::new();
    let world = build_world(
        game,
        10,
        (),
        InitialActions::PerAgent(public_goods_initial(seed)),
        &rules,
        &Assignment::Ordered(vec![AssignGroup {
            kind: AssignKind::Held,
            count: 10,
        }]),
        4,
        STEPS,
        seed,
    )
    .unwrap();
    let moran = socsim_games::MoranUpdate::new(
        socsim_games::Fitness::Exponential { w: 0.1 },
        0.2,
        socsim_games::Update::Synchronous,
    )
    .unwrap()
    .gaussian_mutation(0.5)
    .unwrap();
    socsim_games::assemble_with(
        world,
        RandomGroups::new(4).unwrap(),
        rules,
        socsim_games::Pairwise::new(NormalFormOutcome).with_aggregates(4),
        moran,
        PlayOptions::new(),
        seed,
    )
    .unwrap()
    .add_mechanism(Box::new(metrics))
    .recorder(Box::new(RowRecorder::default()))
    .build()
}

#[test]
fn public_goods_moran_with_gaussian_mutation_is_deterministic() {
    let mut first = moran_public_goods_simulation(61);
    let mut second = moran_public_goods_simulation(61);
    let first_worlds = run(&mut first);
    let second_worlds = run(&mut second);

    assert!(!first_worlds.is_empty());
    assert_eq!(first_worlds, second_worlds);
    assert_eq!(rows(&first), rows(&second));
    assert_eq!(first.stop_requested(), second.stop_requested());
    assert_eq!(
        serde_json::to_string(&first.snapshot().rng).unwrap(),
        serde_json::to_string(&second.snapshot().rng).unwrap()
    );

    // Mutation produced actions that are not among the initial ones, and
    // every action stays inside [0, e].
    let initial = public_goods_initial(61);
    let held = first.world().held();
    assert!(held.iter().any(|action| !initial.contains(action)));
    assert!(held
        .iter()
        .all(|action| action.len() == 1 && (0.0..=10.0).contains(&action[0])));

    // A different seed gives a different trajectory.
    let mut other = moran_public_goods_simulation(62);
    assert_ne!(run(&mut other), first_worlds);
}

#[test]
fn uniform_initial_actions_match_the_public_goods_helper() {
    let game = public_goods_game();
    for seed in [0, 3, 7, 41] {
        assert_eq!(
            socsim_games::uniform_initial_actions(&game, 10, seed),
            public_goods_initial(seed)
        );
    }
}
