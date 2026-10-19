//! Scenario-level counting play agrees with the ordinary play path.

use std::any::Any;

use socsim_config::Scenario;
use socsim_core::{derive_seed, Recorder, SocsimError, WorldState};
use socsim_engine::{Simulation, SimulationBuilder};
use socsim_games::catalog::{ElFarol, HawkDove, MinorityGame, PublicGoodsFinite};
use socsim_games::{
    game_family, AssignGroup, AssignKind, Assignment, ContinuousSpec, FermiUpdate, FiniteSpec,
    GameError, GameFamily, GameSimFactory, GameTrialFactory, GameWorld, GridBoundary,
    GridNeighborhood, GridNeighbors, GridSpec, InitialActions, Matching, MetricsOptions, PlayPath,
    PopulationDynamics, RandomGroups, RandomPairs, RuleRegistry, StageGame, Topology,
    UniformRandom, Update, WholePopulation, SCENARIO_MECHANISM_NAMES,
};
use socsim_runner::{Trial, TrialFactory};

fn parse(text: &str) -> Scenario {
    Scenario::parse(text).unwrap()
}

fn scenario(game: &str, counting: &str) -> Scenario {
    parse(&format!(
        r#"
[simulation]
name = "counting"
module_pack = "normal-form"
t_max = 1
seed = 0
scheduler = "sequential"

[world]
n_agents = 2
history_len = 1

[world.game]
{game}

[world.population]
[[world.population.group]]
rule = "uniform_random"
count = 2

[[mechanism]]
name = "game_match"
[mechanism.params]
matching = "random_pairs"

[[mechanism]]
name = "game_decide"

[[mechanism]]
name = "game_play"
[mechanism.params]
{counting}

[output]
log_path = "runs/{{name}}_{{seed}}.jsonl"
"#
    ))
}

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

#[derive(Debug, PartialEq)]
struct Trajectory {
    worlds: Vec<serde_json::Value>,
    rows: Rows,
    materialized: Vec<bool>,
}

fn run<G>(trial: Trial<GameWorld<G>>, steps: u64) -> Trajectory
where
    G: StageGame + Clone + serde::Serialize,
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
    let mut materialized = Vec::new();
    while !simulation.stop_requested() && simulation.world().clock().t() < steps {
        simulation.step().unwrap();
        materialized.push(simulation.world().plays().n_encounters() != 0);
        let mut world = serde_json::to_value(simulation.world()).unwrap();
        world.as_object_mut().unwrap().remove("game");
        worlds.push(world);
    }
    let rows = simulation
        .recorder()
        .as_any()
        .and_then(|recorder| recorder.downcast_ref::<RowRecorder>())
        .unwrap()
        .rows
        .clone();
    Trajectory {
        worlds,
        rows,
        materialized,
    }
}

fn finite_scenario(
    world: &str,
    population: &str,
    matching: &str,
    play: &str,
    dynamics: &str,
    metrics: &str,
    counting: bool,
) -> Scenario {
    parse(&format!(
        r#"
[simulation]
name = "spec_counting"
module_pack = "normal-form"
t_max = 10
seed = 0
scheduler = "sequential"

[world]
{world}

[world.population]
{population}

[[mechanism]]
name = "game_match"
[mechanism.params]
{matching}

[[mechanism]]
name = "game_decide"

[[mechanism]]
name = "game_play"
[mechanism.params]
counting = {counting}
{play}

{dynamics}
[[mechanism]]
name = "game_metrics"
[mechanism.params]
{metrics}

[output]
log_path = "runs/{{name}}_{{seed}}.jsonl"
"#
    ))
}

const SEEDS: [u64; 2] = [0, 7];

fn assert_same_counting_trajectory<G, M, P, D>(
    ordinary: &Scenario,
    counting: &Scenario,
    library: impl Fn(u64) -> GameSimFactory<G, (), M, P, D>,
    materialized: bool,
) where
    G: StageGame + Clone + serde::Serialize,
    G::Action: serde::Serialize,
    M: Matching<G, ()> + Clone + 'static,
    P: PlayPath<G, ()> + Clone,
    D: PopulationDynamics<G, ()> + Clone,
{
    let factory = GameTrialFactory::<FiniteSpec>::new(RuleRegistry::finite_builtins());
    factory.validate(ordinary).unwrap();
    factory.validate(counting).unwrap();
    for seed in SEEDS {
        let ordinary_run = run(factory.build_trial(ordinary, seed).unwrap(), 10);
        let counting_run = run(factory.build_trial(counting, seed).unwrap(), 10);
        let library_trial = library(seed).build_trial(seed).unwrap();
        assert_eq!(library_trial.engine_seed, derive_seed(seed, &[1]));
        let library_run = run(library_trial, 10);
        assert_eq!(
            counting_run, ordinary_run,
            "scenario paths differ for seed {seed}"
        );
        assert_eq!(
            counting_run, library_run,
            "library path differs for seed {seed}"
        );
        assert!(
            counting_run
                .materialized
                .iter()
                .all(|actual| *actual == materialized),
            "unexpected materialization path for seed {seed}: {:?}",
            counting_run.materialized
        );
    }
}

fn assigned(rule: &str, count: usize) -> Assignment {
    Assignment::Ordered(vec![AssignGroup {
        kind: AssignKind::Rule(rule.to_owned()),
        count,
    }])
}

fn held(count: usize) -> Assignment {
    Assignment::Ordered(vec![AssignGroup {
        kind: AssignKind::Held,
        count,
    }])
}

#[test]
fn public_scenario_metadata_is_exact() {
    assert_eq!(
        SCENARIO_MECHANISM_NAMES,
        [
            "game_match",
            "game_decide",
            "game_play",
            "imitate_best",
            "fermi_update",
            "replicator_update",
            "moran_update",
            "game_metrics",
            "continuation_probability",
        ]
    );
    assert_eq!(
        game_family(&scenario(
            "kind = \"hawk_dove\"\nv = 2\nc = 3",
            "counting = true"
        ))
        .unwrap(),
        GameFamily::Finite
    );
    assert_eq!(
        game_family(&scenario(
            "kind = \"cournot\"\na = 100\nb = 1\nc = 10\nq_max = 100",
            ""
        ))
        .unwrap(),
        GameFamily::Continuous
    );
}

fn family_error(world: &str) -> String {
    let scenario = parse(&format!(
        r#"
[simulation]
name = "family_error"
module_pack = "normal-form"
t_max = 1
seed = 0
scheduler = "sequential"

[world]
{world}

[output]
log_path = "runs/{{name}}_{{seed}}.jsonl"
"#
    ));
    match game_family(&scenario) {
        Err(GameError::Config(message)) => message,
        Err(other) => panic!("expected a configuration error, got {other:?}"),
        Ok(family) => panic!("expected an error, got {family:?}"),
    }
}

#[test]
fn game_family_uses_the_scenario_reader_error_contract() {
    assert_eq!(
        family_error("n_agents = 2"),
        "world.game: this key is required"
    );
    assert_eq!(
        family_error("n_agents = 2\n[world.game]"),
        "world.game.kind: this key is required"
    );
    assert_eq!(
        family_error("n_agents = 2\ngame = 3"),
        "world.game: expected a table, found integer"
    );
    assert_eq!(
        family_error("n_agents = 2\n[world.game]\nkind = 3"),
        "world.game.kind: expected a string, found integer"
    );
    assert_eq!(
        family_error("n_agents = 2\n[world.game]\nkind = \"unknown\""),
        concat!(
            "world.game.kind: unknown game kind \"unknown\" (expected one of ",
            "\"matrix\", \"prisoners_dilemma\", \"battle_of_sexes\", ",
            "\"matching_pennies\", \"rock_paper_scissors\", \"stag_hunt\", ",
            "\"hawk_dove\", \"el_farol\", \"minority\", \"public_goods_finite\", ",
            "\"cournot\", \"bertrand_differentiated\", \"public_goods_continuous\")"
        )
    );
}

#[test]
fn counting_is_rejected_at_read_time_for_non_anonymous_and_continuous_games() {
    let finite = scenario(
        "kind = \"prisoners_dilemma\"\ntemptation = 5\nreward = 3\npunishment = 1\nsucker = 0",
        "counting = true",
    );
    let error = GameTrialFactory::<FiniteSpec>::new(RuleRegistry::finite_builtins())
        .validate(&finite)
        .unwrap_err()
        .to_string();
    assert!(error.contains("mechanism[game_play].counting"), "{error}");
    assert!(!error.contains("unknown key"), "{error}");

    let continuous = scenario(
        "kind = \"cournot\"\na = 100\nb = 1\nc = 10\nq_max = 100",
        "counting = false",
    );
    let error = GameTrialFactory::<ContinuousSpec>::new(RuleRegistry::continuous_builtins())
        .validate(&continuous)
        .unwrap_err()
        .to_string();
    assert!(error.contains("mechanism[game_play].counting"), "{error}");
    assert!(!error.contains("unknown key"), "{error}");
}

#[test]
fn asynchronous_update_keeps_the_existing_reader_error() {
    let scenario = finite_scenario(
        "n_agents = 2\nhistory_len = 1\n[world.game]\nkind = \"hawk_dove\"\nv = 2\nc = 3",
        "[[world.population.group]]\nrule = \"held\"\ncount = 2",
        "matching = \"random_pairs\"",
        "",
        "[[mechanism]]\nname = \"imitate_best\"\n[mechanism.params]\nupdate = \"asynchronous\"\n",
        "",
        false,
    );
    let error = GameTrialFactory::<FiniteSpec>::new(RuleRegistry::finite_builtins())
        .validate(&scenario)
        .unwrap_err();
    match error {
        SocsimError::Config(message) => assert_eq!(
            message,
            "mechanism[imitate_best].update: expected one of \"synchronous\", found \"asynchronous\""
        ),
        other => panic!("expected a configuration error, got {other:?}"),
    }
}

#[test]
fn hawk_dove_random_pairs_matches_all_three_paths() {
    let make = |counting| {
        finite_scenario(
            "n_agents = 10\nhistory_len = 10\n[world.game]\nkind = \"hawk_dove\"\nv = 2\nc = 3",
            "[[world.population.group]]\nrule = \"uniform_random\"\ncount = 10",
            "matching = \"random_pairs\"",
            "",
            "",
            "",
            counting,
        )
    };
    assert_same_counting_trajectory(
        &make(false),
        &make(true),
        |_| {
            GameSimFactory::new(HawkDove::new(2.0, 3.0).unwrap())
                .agents(10)
                .matching(RandomPairs::new())
                .rule("uniform_random", UniformRandom)
                .assign(assigned("uniform_random", 10))
                .history_len(10)
                .t_max(10)
                .counting_play()
                .metrics(MetricsOptions::new())
        },
        true,
    );
}

#[test]
fn minority_whole_population_with_aggregates_matches_all_three_paths() {
    let make = |counting| {
        finite_scenario(
            "n_agents = 5\nhistory_len = 10\n[world.game]\nkind = \"minority\"",
            "[[world.population.group]]\nrule = \"uniform_random\"\ncount = 5",
            "matching = \"whole_population\"",
            "aggregate_history_len = 4",
            "",
            "",
            counting,
        )
    };
    assert_same_counting_trajectory(
        &make(false),
        &make(true),
        |_| {
            GameSimFactory::new(MinorityGame::new(5).unwrap())
                .agents(5)
                .matching(WholePopulation::new())
                .rule("uniform_random", UniformRandom)
                .assign(assigned("uniform_random", 5))
                .history_len(10)
                .t_max(10)
                .counting_play()
                .aggregates(4)
                .metrics(MetricsOptions::new())
        },
        true,
    );
}

#[test]
fn el_farol_whole_population_matches_all_three_paths() {
    let make = |counting| {
        finite_scenario(
            "n_agents = 5\nhistory_len = 10\n[world.game]\nkind = \"el_farol\"\ncapacity = 3",
            "[[world.population.group]]\nrule = \"uniform_random\"\ncount = 5",
            "matching = \"whole_population\"",
            "",
            "",
            "",
            counting,
        )
    };
    assert_same_counting_trajectory(
        &make(false),
        &make(true),
        |_| {
            GameSimFactory::new(ElFarol::new(5, 3).unwrap())
                .agents(5)
                .matching(WholePopulation::new())
                .rule("uniform_random", UniformRandom)
                .assign(assigned("uniform_random", 5))
                .history_len(10)
                .t_max(10)
                .counting_play()
                .metrics(MetricsOptions::new())
        },
        true,
    );
}

#[test]
fn public_goods_random_groups_materializes_and_matches_all_three_paths() {
    let make = |counting| {
        finite_scenario(
            "n_agents = 8\nhistory_len = 10\n[world.game]\nkind = \"public_goods_finite\"\nk = 4\nr = 3\ncost = 1",
            "initial_shares = { C = 0.5 }\n[[world.population.group]]\nrule = \"held\"\ncount = 8",
            "matching = \"random_groups\"\nk = 4",
            "",
            "[[mechanism]]\nname = \"fermi_update\"\n[mechanism.params]\nupdate = \"synchronous\"\nk = 0.5\n",
            "population_share = true",
            counting,
        )
    };
    assert_same_counting_trajectory(
        &make(false),
        &make(true),
        |_| {
            GameSimFactory::new(PublicGoodsFinite::new(4, 3.0, 1.0).unwrap())
                .agents(8)
                .matching(RandomGroups::new(4).unwrap())
                .assign(held(8))
                .initial_held(InitialActions::Shares(vec![(0, 0.5), (1, 0.5)]))
                .dynamics(FermiUpdate::new(0.5, Update::Synchronous).unwrap())
                .history_len(10)
                .t_max(10)
                .counting_play()
                .metrics(MetricsOptions::new().population_share(true))
        },
        true,
    );
}

#[test]
fn hawk_dove_grid_uses_implicit_path_and_matches_all_three_paths() {
    let make = |counting| {
        finite_scenario(
            "topology = \"grid\"\ngrid_rows = 5\ngrid_cols = 5\nboundary = \"toroidal\"\nneighborhood = \"moore\"\nhistory_len = 10\n[world.game]\nkind = \"hawk_dove\"\nv = 2\nc = 3",
            "initial_shares = { H = 0.5 }\n[[world.population.group]]\nrule = \"held\"\ncount = 25",
            "matching = \"grid_neighborhood\"",
            "",
            "[[mechanism]]\nname = \"fermi_update\"\n[mechanism.params]\nupdate = \"synchronous\"\nk = 0.5\n",
            "population_share = true",
            counting,
        )
    };
    assert_same_counting_trajectory(
        &make(false),
        &make(true),
        |_| {
            GameSimFactory::new(HawkDove::new(2.0, 3.0).unwrap())
                .agents(25)
                .topology(
                    Topology::grid(GridSpec {
                        rows: 5,
                        cols: 5,
                        boundary: GridBoundary::Toroidal,
                        neighborhood: GridNeighbors::Moore,
                    })
                    .unwrap(),
                )
                .matching(GridNeighborhood::new())
                .assign(held(25))
                .initial_held(InitialActions::Shares(vec![(0, 0.5), (1, 0.5)]))
                .dynamics(FermiUpdate::new(0.5, Update::Synchronous).unwrap())
                .history_len(10)
                .t_max(10)
                .counting_play()
                .metrics(MetricsOptions::new().population_share(true))
        },
        false,
    );
}

#[test]
fn minority_held_with_aggregates_uses_implicit_path_and_matches_all_three_paths() {
    let make = |counting| {
        finite_scenario(
            "n_agents = 5\nhistory_len = 10\n[world.game]\nkind = \"minority\"",
            "initial_shares = { a0 = 0.4 }\n[[world.population.group]]\nrule = \"held\"\ncount = 5",
            "matching = \"whole_population\"",
            "aggregate_history_len = 4",
            "[[mechanism]]\nname = \"fermi_update\"\n[mechanism.params]\nupdate = \"synchronous\"\nk = 0.5\n",
            "population_share = true",
            counting,
        )
    };
    assert_same_counting_trajectory(
        &make(false),
        &make(true),
        |_| {
            GameSimFactory::new(MinorityGame::new(5).unwrap())
                .agents(5)
                .matching(WholePopulation::new())
                .assign(held(5))
                .initial_held(InitialActions::Shares(vec![(0, 0.4), (1, 0.6)]))
                .dynamics(FermiUpdate::new(0.5, Update::Synchronous).unwrap())
                .history_len(10)
                .t_max(10)
                .counting_play()
                .aggregates(4)
                .metrics(MetricsOptions::new().population_share(true))
        },
        false,
    );
}
