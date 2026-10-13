//! `GameTrialFactory` builds trials from scenarios.
//!
//! (a) `[[mechanism]]` parameters reach the initial world (initial fictitious
//! counts); (b) `round_robin` starts from its matching state and
//! `both_orders` doubles the encounters; (c) the engine seed is
//! `derive_seed(seed, &[1])`; (d) a scenario and the same configuration
//! written with `GameSimFactory` give the same world after every step and the
//! same metric rows; (e) the trial runner runs a scenario trial; (f) a check
//! that depends on the generated network splits seeds into `Err` and `Ok`,
//! and `validate` reports the result for `simulation.seed`.

use std::any::Any;

use socsim_config::Scenario;
use socsim_core::{derive_seed, AgentId, Recorder, SimRng, SocsimError, WorldState};
use socsim_engine::{Simulation, SimulationBuilder};
use socsim_games::catalog::{HawkDove, PrisonersDilemma, PublicGoodsFinite, StagHunt};
use socsim_games::continuous::Cournot;
use socsim_games::{
    uniform_initial_actions, AssignGroup, AssignKind, Assignment, Automaton, AutomatonKind,
    BuiltinPolicyState, ContinuousEquilibrium, ContinuousMetricsOptions, ContinuousSpec,
    DampedBestResponse, DecisionScope, EquilibriumTarget, ExperimentState, FermiUpdate,
    FictitiousPlay, FiniteSpec, Fitness, FixedPlayers, GameError, GameSimFactory, GameTrialFactory,
    GameWorld, GridBoundary, GridNeighborhood, GridNeighbors, GridSpec, ImitateBest,
    InitialActions, MatchingState, MatrixGame, MetricsOptions, MoranUpdate, NetworkEdges,
    PayoffAggregation, PlayOptions, ProportionalImitation, RandomGroups, RandomPairs, RoundRobin,
    RuleRegistry, SchedulerChoice, StageGame, TitForTat, Topology, UniformRandom, Update,
    WholePopulation,
};
use socsim_net::SocialNetwork;
use socsim_runner::{run_trial, Trial, TrialFactory};

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

/// The world after every step and every recorded row.
#[derive(Debug, PartialEq)]
struct Trajectory {
    worlds: Vec<String>,
    rows: Rows,
}

/// Runs a trial the way a trial runner does (its parts, its scheduler, and
/// its engine seed) until `steps` steps have run or a mechanism requests a
/// stop.
fn run<G>(trial: Trial<GameWorld<G>>, steps: u64) -> Trajectory
where
    G: StageGame + serde::Serialize,
    G::Action: serde::Serialize,
{
    let mut simulation: Simulation<GameWorld<G>> = trial
        .mechanisms
        .into_iter()
        .fold(SimulationBuilder::new(trial.world), |builder, mechanism| {
            builder.add_mechanism(mechanism)
        })
        .scheduler(trial.scheduler)
        .seed(trial.engine_seed)
        .recorder(Box::new(RowRecorder::default()))
        .build();
    let mut worlds = Vec::new();
    while !simulation.stop_requested() && simulation.world().clock().t() < steps {
        simulation.step().unwrap();
        worlds.push(serde_json::to_string(simulation.world()).unwrap());
    }
    let rows = simulation
        .recorder()
        .as_any()
        .and_then(|recorder| recorder.downcast_ref::<RowRecorder>())
        .map(|recorder| recorder.rows.clone())
        .unwrap_or_default();
    Trajectory { worlds, rows }
}

fn finite_factory() -> GameTrialFactory<FiniteSpec> {
    GameTrialFactory::new(RuleRegistry::finite_builtins())
}

fn continuous_factory() -> GameTrialFactory<ContinuousSpec> {
    GameTrialFactory::new(RuleRegistry::continuous_builtins())
}

fn parse(text: &str) -> Scenario {
    Scenario::parse(text).unwrap()
}

fn config_message<T>(result: socsim_core::Result<T>) -> String {
    match result {
        Err(SocsimError::Config(message)) => message,
        Err(other) => panic!("expected a configuration error, got {other:?}"),
        Ok(_) => panic!("expected a configuration error, got Ok"),
    }
}

/// The scenario of the repeated prisoner's dilemma, tit for tat against
/// fictitious play; `decide_extra` is appended to the `game_decide` params.
fn ipd(decide_extra: &str) -> String {
    format!(
        r#"
[simulation]
name        = "ipd_tft_vs_fp"
module_pack = "normal-form"
t_max       = 200
seed        = 42
scheduler   = "sequential"

[world]
n_agents    = 2
history_len = 200

[world.game]
kind    = "matrix"
labels  = [["C", "D"], ["C", "D"]]
payoffs = [
  [[3.0, 3.0], [0.0, 5.0]],
  [[5.0, 0.0], [1.0, 1.0]],
]

[world.population]
assign = "ordered"
[[world.population.group]]
rule  = "tit_for_tat"
count = 1
[[world.population.group]]
rule  = "fictitious_play"
count = 1

[[mechanism]]
name  = "game_match"
[mechanism.params]
matching = "fixed_players"
pairs    = [[0, 1]]

[[mechanism]]
name  = "game_decide"
[mechanism.params]
decision_scope = "per_step"
coop_label     = "C"
fp_tie_break   = "lowest_index"
{decide_extra}

[[mechanism]]
name  = "game_play"
[mechanism.params]
payoff_aggregation = "sum"

[[mechanism]]
name  = "game_metrics"
[mechanism.params]
cooperate_label = "C"
equilibrium     = "solve"

[output]
log_path = "runs/{{name}}_{{seed}}.jsonl"
"#
    )
}

/// The initial fictitious counts of agent 1 right after the trial is built.
fn initial_counts(scenario: &Scenario) -> Vec<f64> {
    let trial = finite_factory().build_trial(scenario, 5).unwrap();
    match trial.world.policies().state(AgentId(1)) {
        Some(BuiltinPolicyState::Belief(belief)) => belief.global().counts().to_vec(),
        other => panic!("expected a fictitious-play belief, got {other:?}"),
    }
}

// ── (a) initial fictitious counts ───────────────────────────────────────────

#[test]
fn a_fictitious_play_starts_from_the_initial_counts_of_game_decide() {
    let with_counts = parse(&ipd("fp_initial_counts = [2.0, 0.0]"));
    assert_eq!(initial_counts(&with_counts), vec![2.0, 0.0]);
    let without = parse(&ipd(""));
    assert_eq!(initial_counts(&without), vec![0.0, 0.0]);

    for counts in ["[1.0]", "[-1.0, 0.0]"] {
        let scenario = parse(&ipd(&format!("fp_initial_counts = {counts}")));
        let factory = finite_factory();
        let message = config_message(factory.build_trial(&scenario, 5));
        assert!(
            message.starts_with("mechanism[game_decide].fp_initial_counts"),
            "{message}"
        );
        let message = config_message(factory.validate(&scenario));
        assert!(
            message.starts_with("mechanism[game_decide].fp_initial_counts"),
            "{message}"
        );
    }
}

// ── (b) round robin ─────────────────────────────────────────────────────────

fn round_robin(both_orders: bool) -> Scenario {
    parse(&format!(
        r#"
[simulation]
name = "round_robin"
module_pack = "normal-form"
t_max = 4
seed = 1
scheduler = "sequential"

[world]
n_agents = 3
history_len = 4

[world.game]
kind = "hawk_dove"
v = 2
c = 3

[world.population]
[[world.population.group]]
rule = "uniform_random"
count = 3

[[mechanism]]
name = "game_match"
[mechanism.params]
matching = "round_robin"
both_orders = {both_orders}

[[mechanism]]
name = "game_decide"
[mechanism.params]
decision_scope = "per_encounter"

[[mechanism]]
name = "game_play"

[output]
log_path = "runs/{{name}}_{{seed}}.jsonl"
"#
    ))
}

#[test]
fn b_round_robin_starts_at_cursor_zero_and_both_orders_double_the_encounters() {
    for (both_orders, encounters) in [(false, 3), (true, 6)] {
        let trial = finite_factory()
            .build_trial(&round_robin(both_orders), 11)
            .unwrap();
        let world = serde_json::to_value(&trial.world).unwrap();
        assert_eq!(
            world["matching_state"],
            serde_json::to_value(MatchingState::RoundRobin { cursor: 0 }).unwrap()
        );
        let mut simulation = trial
            .mechanisms
            .into_iter()
            .fold(SimulationBuilder::new(trial.world), |builder, mechanism| {
                builder.add_mechanism(mechanism)
            })
            .scheduler(trial.scheduler)
            .seed(trial.engine_seed)
            .build();
        simulation.step().unwrap();
        assert_eq!(simulation.world().plays().n_encounters(), encounters);
    }
}

// ── (c) engine seed ─────────────────────────────────────────────────────────

#[test]
fn c_the_engine_seed_is_derived_from_the_trial_seed() {
    let scenario = parse(&ipd(""));
    for seed in [0, 7] {
        let trial = finite_factory().build_trial(&scenario, seed).unwrap();
        assert_eq!(trial.engine_seed, derive_seed(seed, &[1]));
    }
}

// ── (d) the scenario and the library build the same trajectory ─────────────
//
// | Configuration                 | Matching            | Dynamic           | Play path      |
// |-------------------------------|---------------------|-------------------|----------------|
// | example (a), repeated PD      | `fixed_players`     | none              | pairwise       |
// | example (b), 10x10 grid       | `grid_neighborhood` | `imitate_best`    | pairwise       |
// | example (c), Cournot          | `whole_population`  | none              | with aggregates|
// | Watts-Strogatz network        | `network_edges`     | `replicator_update` | pairwise     |
// | public goods in groups        | `random_groups`     | none              | pairwise       |
// | hawk-dove in random pairs     | `random_pairs`      | `fermi_update`    | pairwise       |
// | stag hunt without topology    | `fixed_players`     | `moran_update`    | pairwise       |
// | round robin                   | `round_robin`       | none              | pairwise       |

const SEEDS: [u64; 2] = [0, 3];

/// Builds both sides for every seed and compares their trajectories.
fn assert_same_trajectory<G, M, P, D>(
    scenario: &Scenario,
    trial_factory: &GameTrialFactory<G>,
    library: impl Fn(u64) -> GameSimFactory<G, (), M, P, D>,
    steps: u64,
) where
    G: socsim_games::ScenarioGame + serde::Serialize,
    G::Action: serde::Serialize,
    GameTrialFactory<G>: TrialFactory<GameWorld<G>>,
    M: socsim_games::Matching<G, ()> + Clone + 'static,
    P: socsim_games::PlayPath<G, ()> + Clone,
    D: socsim_games::PopulationDynamics<G, ()> + Clone,
{
    trial_factory.validate(scenario).unwrap();
    for seed in SEEDS {
        let from_scenario = trial_factory.build_trial(scenario, seed).unwrap();
        assert_eq!(from_scenario.engine_seed, derive_seed(seed, &[1]));
        let from_scenario = run(from_scenario, steps);
        let from_library = run(library(seed).build_trial(seed).unwrap(), steps);
        assert!(!from_library.worlds.is_empty());
        assert_eq!(from_scenario.worlds.len(), from_library.worlds.len());
        for (step, (left, right)) in from_scenario
            .worlds
            .iter()
            .zip(&from_library.worlds)
            .enumerate()
        {
            assert_eq!(left, right, "the worlds differ after step {}", step + 1);
        }
        assert_eq!(from_scenario.rows, from_library.rows);
    }
}

fn labelled_prisoners_dilemma() -> FiniteSpec {
    let labels = vec![vec!["C".to_owned(), "D".to_owned()]; 2];
    FiniteSpec::Matrix(
        MatrixGame::bimatrix(
            &[vec![3.0, 0.0], vec![5.0, 1.0]],
            &[vec![3.0, 5.0], vec![0.0, 1.0]],
        )
        .unwrap()
        .with_labels(labels)
        .unwrap(),
    )
}

fn ordered(groups: &[(&str, usize)]) -> Vec<AssignGroup> {
    groups
        .iter()
        .map(|(rule, count)| AssignGroup {
            kind: AssignKind::Rule((*rule).to_owned()),
            count: *count,
        })
        .collect()
}

#[test]
fn d_example_a_repeated_prisoners_dilemma() {
    let scenario = parse(&ipd(""));
    assert_same_trajectory(
        &scenario,
        &finite_factory(),
        |_| {
            GameSimFactory::new(labelled_prisoners_dilemma())
                .agents(2)
                .matching(FixedPlayers::pairs(&[(0, 1)]))
                .rule("fictitious_play", FictitiousPlay::new())
                .rule("tit_for_tat", TitForTat::new(0))
                .assign(Assignment::Ordered(ordered(&[
                    ("tit_for_tat", 1),
                    ("fictitious_play", 1),
                ])))
                .history_len(200)
                .t_max(200)
                .play_options(
                    PlayOptions::new()
                        .payoff_aggregation(PayoffAggregation::Sum)
                        .decision_scope(DecisionScope::PerStep),
                )
                .metrics(
                    MetricsOptions::new()
                        .cooperate_label("C")
                        .equilibrium(EquilibriumTarget::Solve),
                )
        },
        50,
    );
}

const SPATIAL: &str = r#"
[simulation]
name        = "nowak_may_b185"
module_pack = "normal-form"
t_max       = 200
seed        = 1
scheduler   = "sequential"

[world]
topology     = "grid"
grid_rows    = 10
grid_cols    = 10
boundary     = "toroidal"
neighborhood = "moore"
history_len  = 1

[world.game]
kind       = "prisoners_dilemma"
temptation = 1.85
reward     = 1.0
punishment = 0.0
sucker     = 0.0

[world.population]
initial_shares = { C = 0.9 }
[[world.population.group]]
rule  = "held"
share = 1.0

[[mechanism]]
name = "game_match"
[mechanism.params]
matching     = "grid_neighborhood"
include_self = true

[[mechanism]]
name = "game_decide"

[[mechanism]]
name = "game_play"
[mechanism.params]
payoff_aggregation = "sum"

[[mechanism]]
name = "imitate_best"
[mechanism.params]
update    = "synchronous"
tie_break = "keep_own"

[[mechanism]]
name = "game_metrics"
[mechanism.params]
cooperate_label = "C"
population_share = true

[output]
log_path = "runs/{name}_{seed}.jsonl"
"#;

#[test]
fn d_example_b_spatial_prisoners_dilemma_on_a_grid() {
    assert_same_trajectory(
        &parse(SPATIAL),
        &finite_factory(),
        |_| {
            let game = PrisonersDilemma::new(1.85, 1.0, 0.0, 0.0).unwrap();
            GameSimFactory::new(FiniteSpec::PrisonersDilemmaGame(game))
                .agents(100)
                .topology(
                    Topology::grid(GridSpec {
                        rows: 10,
                        cols: 10,
                        boundary: GridBoundary::Toroidal,
                        neighborhood: GridNeighbors::Moore,
                    })
                    .unwrap(),
                )
                .matching(GridNeighborhood::new().include_self(true))
                .initial_held(InitialActions::Shares(vec![(0, 0.9), (1, 1.0 - 0.9)]))
                .dynamics(ImitateBest::new(Update::Synchronous))
                .play_options(PlayOptions::new().payoff_aggregation(PayoffAggregation::Sum))
                .history_len(1)
                .t_max(200)
                .metrics(
                    MetricsOptions::new()
                        .cooperate_label("C")
                        .population_share(true),
                )
        },
        20,
    );
}

const COURNOT: &str = r#"
[simulation]
name        = "cournot_n3"
module_pack = "normal-form"
t_max       = 500
seed        = 7
scheduler   = "sequential"

[world]
n_agents    = 3
history_len = 16

[world.game]
kind  = "cournot"
a     = 100.0
b     = 1.0
c     = 10.0
q_max = 100.0

[world.population]
initial_uniform = true
[[world.population.group]]
rule  = "damped_best_response"
count = 3

[[mechanism]]
name = "game_match"
[mechanism.params]
matching = "whole_population"

[[mechanism]]
name = "game_decide"
[mechanism.params]
br_damping = 0.5

[[mechanism]]
name = "game_play"
[mechanism.params]
aggregate_history_len = 16

[[mechanism]]
name = "game_metrics"
[mechanism.params]
equilibrium = "solve"
stop_tol    = 1e-9
stop_window = 10

[output]
log_path = "runs/{name}_{seed}.jsonl"
"#;

#[test]
fn d_example_c_cournot_oligopoly_with_aggregates() {
    assert_same_trajectory(
        &parse(COURNOT),
        &continuous_factory(),
        |seed| {
            let game =
                ContinuousSpec::CournotGame(Cournot::new(3, 100.0, 1.0, 10.0, 100.0).unwrap());
            let initial = uniform_initial_actions(&game, 3, seed);
            GameSimFactory::new(game)
                .agents(3)
                .matching(WholePopulation::new())
                .rule(
                    "damped_best_response",
                    DampedBestResponse::new(0.5).unwrap(),
                )
                .initial_held(InitialActions::PerAgent(initial))
                .history_len(16)
                .t_max(500)
                .aggregates(16)
                .continuous_metrics(
                    ContinuousMetricsOptions::new()
                        .equilibrium(ContinuousEquilibrium::Solve)
                        .stop(1e-9, 10),
                )
        },
        60,
    );
}

/// A small scenario: `world`, `population`, and `mechanisms` are spliced in.
fn small(scheduler: &str, world: &str, population: &str, mechanisms: &str) -> Scenario {
    parse(&format!(
        "[simulation]\nname = \"small\"\nmodule_pack = \"normal-form\"\nt_max = 8\nseed = 2\nscheduler = \"{scheduler}\"\n\n[world]\n{world}\n\n[world.population]\n{population}\n\n{mechanisms}\n[output]\nlog_path = \"runs/{{name}}_{{seed}}.jsonl\"\n"
    ))
}

fn entry(name: &str, params: &str) -> String {
    format!("[[mechanism]]\nname = \"{name}\"\n[mechanism.params]\n{params}\n\n")
}

const SMALL_STEPS: u64 = 8;

#[test]
fn d_watts_strogatz_network_with_replicator_update() {
    let scenario = small(
        "sequential",
        "n_agents = 12\nhistory_len = 2\ntopology = \"network\"\nnetwork_model = \"watts_strogatz\"\nnetwork_k = 4\nnetwork_beta = 0.2\n[world.game]\nkind = \"prisoners_dilemma\"\ntemptation = 1.5\nreward = 1\npunishment = 0\nsucker = 0",
        "initial_shares = { C = 0.5 }\n[[world.population.group]]\nrule = \"held\"\ncount = 12",
        &[
            entry("game_match", "matching = \"network_edges\""),
            entry("game_decide", ""),
            entry("game_play", ""),
            entry("replicator_update", "update = \"synchronous\"\nbeta = 0.05"),
            entry("game_metrics", "population_share = true"),
        ]
        .concat(),
    );
    assert_same_trajectory(
        &scenario,
        &finite_factory(),
        |seed| {
            let ids: Vec<AgentId> = (0..12).map(AgentId).collect();
            let mut rng = SimRng::from_seed(derive_seed(seed, &[0, 1]));
            let network = SocialNetwork::watts_strogatz(&ids, 4, 0.2, &mut rng);
            let game = PrisonersDilemma::new(1.5, 1.0, 0.0, 0.0).unwrap();
            GameSimFactory::new(FiniteSpec::PrisonersDilemmaGame(game))
                .agents(12)
                .topology(Topology::network(network))
                .matching(NetworkEdges::new())
                .initial_held(InitialActions::Shares(vec![(0, 0.5), (1, 1.0 - 0.5)]))
                .dynamics(ProportionalImitation::new(0.05, Update::Synchronous).unwrap())
                .history_len(2)
                .t_max(8)
                .metrics(MetricsOptions::new().population_share(true))
        },
        SMALL_STEPS,
    );
}

#[test]
fn d_public_goods_in_random_groups() {
    let scenario = small(
        "sequential",
        "n_agents = 12\nhistory_len = 3\n[world.game]\nkind = \"public_goods_finite\"\nk = 3\nr = 2.0\ncost = 1.0",
        "assign = \"shuffled\"\n[[world.population.group]]\nrule = \"uniform_random\"\ncount = 6\n[[world.population.group]]\nrule = \"all_d\"\ncount = 6",
        &[
            entry("game_match", "matching = \"random_groups\"\nk = 3"),
            entry(
                "game_decide",
                "coop_label = \"C\"",
            ),
            entry("game_play", ""),
            entry("game_metrics", "cooperate_label = \"C\""),
        ]
        .concat(),
    );
    assert_same_trajectory(
        &scenario,
        &finite_factory(),
        |_| {
            let game = PublicGoodsFinite::new(3, 2.0, 1.0).unwrap();
            GameSimFactory::new(FiniteSpec::PublicGoodsFiniteGame(game))
                .agents(12)
                .matching(RandomGroups::new(3).unwrap())
                .rule("uniform_random", UniformRandom)
                .rule("all_d", Automaton::new(AutomatonKind::AllD, 0))
                .assign(Assignment::Shuffled(ordered(&[
                    ("uniform_random", 6),
                    ("all_d", 6),
                ])))
                .history_len(3)
                .t_max(8)
                .metrics(MetricsOptions::new().cooperate_label("C"))
        },
        SMALL_STEPS,
    );
}

#[test]
fn d_hawk_dove_in_random_pairs_with_fermi_update() {
    let scenario = small(
        "random_activation",
        "n_agents = 10\nhistory_len = 1\n[world.game]\nkind = \"hawk_dove\"\nv = 2\nc = 3",
        "initial_shares = { H = 0.3 }\n[[world.population.group]]\nrule = \"held\"\ncount = 10",
        &[
            entry("game_match", "matching = \"random_pairs\""),
            entry("game_decide", ""),
            entry("game_play", "payoff_aggregation = \"mean\""),
            entry("fermi_update", "update = \"synchronous\"\nk = 0.5"),
            entry("game_metrics", "population_share = true"),
        ]
        .concat(),
    );
    assert_same_trajectory(
        &scenario,
        &finite_factory(),
        |_| {
            let game = HawkDove::new(2.0, 3.0).unwrap();
            GameSimFactory::new(FiniteSpec::HawkDoveGame(game))
                .agents(10)
                .matching(RandomPairs::new())
                .initial_held(InitialActions::Shares(vec![(0, 0.3), (1, 1.0 - 0.3)]))
                .dynamics(FermiUpdate::new(0.5, Update::Synchronous).unwrap())
                .play_options(PlayOptions::new().payoff_aggregation(PayoffAggregation::Mean))
                .history_len(1)
                .t_max(8)
                .scheduler(SchedulerChoice::RandomActivation)
                .metrics(MetricsOptions::new().population_share(true))
        },
        SMALL_STEPS,
    );
}

#[test]
fn d_stag_hunt_without_topology_with_moran_update() {
    let scenario = small(
        "sequential",
        "n_agents = 10\nhistory_len = 1\n[world.game]\nkind = \"stag_hunt\"",
        "initial_shares = { stag = 0.6 }\n[[world.population.group]]\nrule = \"held\"\ncount = 10",
        &[
            entry(
                "game_match",
                "matching = \"fixed_players\"\npairs = [[0, 1], [2, 3], [4, 5], [6, 7], [8, 9]]",
            ),
            entry("game_decide", ""),
            entry("game_play", ""),
            entry(
                "moran_update",
                "update = \"synchronous\"\nfitness = \"exponential\"\nw = 0.5\nmutation = 0.05",
            ),
            entry("game_metrics", "population_share = true"),
        ]
        .concat(),
    );
    assert_same_trajectory(
        &scenario,
        &finite_factory(),
        |_| {
            GameSimFactory::new(FiniteSpec::StagHuntGame(StagHunt::default()))
                .agents(10)
                .matching(FixedPlayers::pairs(&[
                    (0, 1),
                    (2, 3),
                    (4, 5),
                    (6, 7),
                    (8, 9),
                ]))
                .initial_held(InitialActions::Shares(vec![(0, 0.6), (1, 1.0 - 0.6)]))
                .dynamics(
                    MoranUpdate::new(Fitness::Exponential { w: 0.5 }, 0.05, Update::Synchronous)
                        .unwrap(),
                )
                .history_len(1)
                .t_max(8)
                .metrics(MetricsOptions::new().population_share(true))
        },
        SMALL_STEPS,
    );
}

#[test]
fn d_round_robin_with_both_orders() {
    let scenario = small(
        "sequential",
        "n_agents = 4\nhistory_len = 4\n[world.game]\nkind = \"prisoners_dilemma\"\ntemptation = 5\nreward = 3\npunishment = 1\nsucker = 0",
        "[[world.population.group]]\nrule = \"tit_for_tat\"\ncount = 2\n[[world.population.group]]\nrule = \"uniform_random\"\ncount = 2",
        &[
            entry("game_match", "matching = \"round_robin\"\nboth_orders = true"),
            entry(
                "game_decide",
                "decision_scope = \"per_encounter\"\ncoop_label = \"C\"",
            ),
            entry("game_play", ""),
        ]
        .concat(),
    );
    assert_same_trajectory(
        &scenario,
        &finite_factory(),
        |_| {
            let game = PrisonersDilemma::new(5.0, 3.0, 1.0, 0.0).unwrap();
            GameSimFactory::new(FiniteSpec::PrisonersDilemmaGame(game))
                .agents(4)
                .matching(RoundRobin::new().both_orders(true))
                .rule("tit_for_tat", TitForTat::new(0))
                .rule("uniform_random", UniformRandom)
                .assign(Assignment::Ordered(ordered(&[
                    ("tit_for_tat", 2),
                    ("uniform_random", 2),
                ])))
                .play_options(PlayOptions::new().decision_scope(DecisionScope::PerEncounter))
                .history_len(4)
                .t_max(8)
        },
        SMALL_STEPS,
    );
}

// ── (e) the trial runner ────────────────────────────────────────────────────

#[test]
fn e_the_trial_runner_runs_a_scenario_trial() {
    let mut scenario = parse(&ipd(""));
    scenario.simulation.t_max = 30;
    let result = run_trial(&scenario, &finite_factory(), 4).unwrap();
    assert_eq!(result.seed, 4);
    let mean_payoff = result
        .series
        .get("game.mean_payoff")
        .expect("the metrics record the mean payoff");
    assert_eq!(mean_payoff.len(), 30);
}

// ── (f) a seed-dependent check ──────────────────────────────────────────────

/// An Erdős–Rényi network whose largest degree decides whether `beta` is
/// within `1 / (temptation * largest degree)`: `beta = 1 / (1.5 * 7)` holds
/// when no agent has more than seven neighbours.
fn degree_bound(seed: u64) -> Scenario {
    let mut scenario = small(
        "sequential",
        "n_agents = 20\nhistory_len = 1\ntopology = \"network\"\nnetwork_model = \"erdos_renyi\"\nnetwork_p = 0.25\n[world.game]\nkind = \"prisoners_dilemma\"\ntemptation = 1.5\nreward = 1\npunishment = 0\nsucker = 0",
        "[[world.population.group]]\nrule = \"held\"\ncount = 20",
        &[
            entry("game_match", "matching = \"network_edges\""),
            entry("game_decide", ""),
            entry("game_play", ""),
            entry(
                "replicator_update",
                &format!("update = \"synchronous\"\nbeta = {}", 1.0 / (1.5 * 7.0)),
            ),
        ]
        .concat(),
    );
    scenario.simulation.seed = seed;
    scenario
}

/// The largest degree of the network that the scenario generates for `seed`.
fn largest_degree(seed: u64) -> usize {
    let ids: Vec<AgentId> = (0..20).map(AgentId).collect();
    let mut rng = SimRng::from_seed(derive_seed(seed, &[0, 1]));
    let network = SocialNetwork::erdos_renyi(&ids, 0.25, &mut rng);
    let mut degrees = vec![0_usize; 20];
    for (source, target) in network.edges() {
        degrees[source.0 as usize] += 1;
        degrees[target.0 as usize] += 1;
    }
    degrees.into_iter().max().unwrap()
}

const SEED_WITHIN: u64 = 1;
const SEED_ABOVE: u64 = 0;

#[test]
fn f_a_seed_dependent_check_splits_seeds_and_validate_uses_the_simulation_seed() {
    assert!(largest_degree(SEED_WITHIN) <= 7);
    assert!(largest_degree(SEED_ABOVE) > 7);

    let factory = finite_factory();
    let scenario = degree_bound(SEED_WITHIN);
    assert!(factory.build_trial(&scenario, SEED_WITHIN).is_ok());
    let message = config_message(factory.build_trial(&scenario, SEED_ABOVE));
    assert!(
        message.starts_with("mechanism[replicator_update]"),
        "{message}"
    );

    factory.validate(&degree_bound(SEED_WITHIN)).unwrap();
    let message = config_message(factory.validate(&degree_bound(SEED_ABOVE)));
    assert!(
        message.starts_with("mechanism[replicator_update]"),
        "{message}"
    );
}

// ── validate ────────────────────────────────────────────────────────────────

#[test]
fn validate_accepts_the_examples() {
    finite_factory().validate(&parse(&ipd(""))).unwrap();
    finite_factory().validate(&parse(SPATIAL)).unwrap();
    continuous_factory().validate(&parse(COURNOT)).unwrap();
}

#[test]
fn validate_constructs_unassigned_rules_whose_own_keys_are_written() {
    // `coop_label` is shared by several rules, so it does not make the unused
    // `gtft` required; `gtft_generosity` is gtft's own key and does.
    let factory = finite_factory();
    let scenario = parse(&ipd("gtft_generosity = 1.5"));
    let message = config_message(factory.validate(&scenario));
    assert!(
        message.starts_with("mechanism[game_decide].gtft_generosity"),
        "{message}"
    );
    assert!(factory.build_trial(&scenario, 1).is_ok());
    factory
        .validate(&parse(&ipd("gtft_generosity = 0.1")))
        .unwrap();

    // `logit_estimate` belongs to `logit` alone, so `logit` is constructed
    // and its required `logit_lambda` is missing.
    let message =
        config_message(factory.validate(&parse(&ipd("logit_estimate = \"last_opponent\""))));
    assert!(
        message.starts_with("mechanism[game_decide].logit_lambda"),
        "{message}"
    );
}

#[test]
fn validate_reports_errors_with_the_path_of_the_key() {
    let factory = finite_factory();
    let text = ipd("").replace("history_len = 200", "history_len = 200\nhistory_lenn = 1");
    let message = config_message(factory.validate(&parse(&text)));
    assert!(message.starts_with("world.history_lenn"), "{message}");
    let message = config_message(factory.build_trial(&parse(&text), 1));
    assert!(message.starts_with("world.history_lenn"), "{message}");

    // A continuous kind in the finite factory.
    let message = config_message(factory.validate(&parse(COURNOT)));
    assert!(message.starts_with("world.game.kind"), "{message}");
}

// ── experiment state ────────────────────────────────────────────────────────

#[derive(Clone, Debug, serde::Serialize)]
struct Tag(u64);

impl ExperimentState for Tag {
    type PolicyState = BuiltinPolicyState;
    type View<'a> = ();

    fn view_for(&self, _me: AgentId) -> Self::View<'_> {}
}

#[test]
fn with_state_builds_the_experiment_state_from_the_scenario() {
    let factory: GameTrialFactory<FiniteSpec, Tag> =
        GameTrialFactory::with_state(RuleRegistry::finite_builtins(), |scenario| {
            Ok(Tag(scenario.simulation.seed))
        });
    let trial = factory.build_trial(&parse(&ipd("")), 9).unwrap();
    assert_eq!(trial.world.x().0, 42);

    let failing: GameTrialFactory<FiniteSpec, Tag> =
        GameTrialFactory::with_state(RuleRegistry::finite_builtins(), |_| {
            Err(GameError::Config("experiment: no state".to_owned()))
        });
    let message = config_message(failing.build_trial(&parse(&ipd("")), 9));
    assert!(message.starts_with("experiment"), "{message}");
}

#[test]
fn the_factory_is_shared_between_threads() {
    fn assert_send_sync<T: Send + Sync + Clone>() {}
    assert_send_sync::<GameTrialFactory<FiniteSpec>>();
    assert_send_sync::<GameTrialFactory<ContinuousSpec>>();
}

/// Calls the trial factory methods for any scenario game, without bounds
/// beyond `G: ScenarioGame`.
fn validate_and_build<G: socsim_games::ScenarioGame>(
    factory: GameTrialFactory<G>,
    scenario: &Scenario,
) -> socsim_core::Result<u64> {
    factory.validate(scenario)?;
    Ok(factory.build_trial(scenario, 6)?.engine_seed)
}

#[test]
fn the_factory_is_a_trial_factory_for_any_scenario_game() {
    assert_eq!(
        validate_and_build(finite_factory(), &parse(&ipd(""))).unwrap(),
        derive_seed(6, &[1])
    );
    assert_eq!(
        validate_and_build(continuous_factory(), &parse(COURNOT)).unwrap(),
        derive_seed(6, &[1])
    );
}
