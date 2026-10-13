use std::collections::HashMap;

use socsim_config::Scenario;
use socsim_core::WorldState;
use socsim_engine::SimulationBuilder;
use socsim_games::catalog::{HawkDove, PrisonersDilemma};
use socsim_games::continuous::Cournot;
use socsim_games::{
    uniform_initial_actions, AssignGroup, AssignKind, Assignment, ContinuousEquilibrium,
    ContinuousMetricsOptions, ContinuousSpec, DampedBestResponse, DecisionScope, EquilibriumTarget,
    FiniteSpec, FixedPlayers, GameSimFactory, GridBoundary, GridNeighborhood, GridNeighbors,
    GridSpec, ImitateBest, InitialActions, MatrixGame, MetricsOptions, PayoffAggregation,
    PlayOptions, RandomPairs, TitForTat, Topology, UniformRandom, Update, WholePopulation,
};
use socsim_log::InMemoryRecorder;
use socsim_packs::normal_form::NormalFormPack;

const IPD: &str = include_str!("../../../scenarios/normal_form_ipd.toml");
const NOWAK_MAY: &str = include_str!("../../../scenarios/normal_form_nowak_may.toml");
const COURNOT: &str = include_str!("../../../scenarios/normal_form_cournot.toml");
const SEEDS: [u64; 2] = [0, 3];

type Series = HashMap<String, Vec<(u64, f64)>>;

fn shortened(text: &str, t_max: u64, grid_side: Option<i64>) -> Scenario {
    let mut value: toml::Value = toml::from_str(text).expect("fixture should be valid TOML");
    value["simulation"]["t_max"] = toml::Value::Integer(t_max as i64);
    if let Some(side) = grid_side {
        value["world"]["grid_rows"] = toml::Value::Integer(side);
        value["world"]["grid_cols"] = toml::Value::Integer(side);
    }
    Scenario::parse(&toml::to_string(&value).unwrap()).unwrap()
}

fn run_library<W: WorldState>(builder: SimulationBuilder<W>) -> Series {
    let mut simulation = builder.recorder(Box::new(InMemoryRecorder::new())).build();
    simulation.run().expect("library simulation should run");
    let recorder = simulation
        .recorder()
        .as_any()
        .and_then(|value| value.downcast_ref::<InMemoryRecorder>())
        .expect("the in-memory recorder should be available");
    let mut series = Series::new();
    for row in recorder.metrics() {
        series
            .entry(row.key.clone())
            .or_default()
            .push((row.t, row.value));
    }
    series
}

fn assert_equivalent(name: &str, scenario: &Scenario, library: impl Fn(u64) -> Series) {
    let results = NormalFormPack
        .run_trials(scenario, &SEEDS, false)
        .unwrap_or_else(|error| panic!("{name} pack run failed: {error}"));
    assert_eq!(results.len(), SEEDS.len(), "{name}");
    for seed in SEEDS {
        let from_pack = &results
            .iter()
            .find(|result| result.seed == seed)
            .unwrap_or_else(|| panic!("{name}: result for seed {seed} is missing"))
            .series;
        let from_library = library(seed);
        assert_eq!(
            from_pack, &from_library,
            "{name}: exact series mismatch for seed {seed}"
        );
    }
}

fn labelled_prisoners_dilemma() -> FiniteSpec {
    FiniteSpec::Matrix(
        MatrixGame::bimatrix(
            &[vec![3.0, 0.0], vec![5.0, 1.0]],
            &[vec![3.0, 5.0], vec![0.0, 1.0]],
        )
        .unwrap()
        .with_labels(vec![vec!["C".to_owned(), "D".to_owned()]; 2])
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
fn repeated_prisoners_dilemma_matches_the_library_builder() {
    const T_MAX: u64 = 50;
    let scenario = shortened(IPD, T_MAX, None);
    assert_equivalent("ipd", &scenario, |seed| {
        let factory = GameSimFactory::new(labelled_prisoners_dilemma())
            .agents(2)
            .matching(FixedPlayers::pairs(&[(0, 1)]))
            .rule("fictitious_play", socsim_games::FictitiousPlay::new())
            .rule("tit_for_tat", TitForTat::new(0))
            .assign(Assignment::Ordered(ordered(&[
                ("tit_for_tat", 1),
                ("fictitious_play", 1),
            ])))
            .history_len(200)
            .t_max(T_MAX)
            .play_options(
                PlayOptions::new()
                    .payoff_aggregation(PayoffAggregation::Sum)
                    .decision_scope(DecisionScope::PerStep),
            )
            .metrics(
                MetricsOptions::new()
                    .cooperate_label("C")
                    .equilibrium(EquilibriumTarget::Solve),
            );
        run_library(factory.into_builder(seed).unwrap())
    });
}

#[test]
fn nowak_may_grid_matches_the_library_builder() {
    const T_MAX: u64 = 20;
    let scenario = shortened(NOWAK_MAY, T_MAX, Some(10));
    assert_equivalent("nowak_may", &scenario, |seed| {
        let game = PrisonersDilemma::new(1.85, 1.0, 0.0, 0.0).unwrap();
        let factory = GameSimFactory::new(FiniteSpec::PrisonersDilemmaGame(game))
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
            .initial_held(InitialActions::Shares(vec![(0, 0.9), (1, 0.1)]))
            .dynamics(ImitateBest::new(Update::Synchronous))
            .play_options(PlayOptions::new().payoff_aggregation(PayoffAggregation::Sum))
            .history_len(1)
            .t_max(T_MAX)
            .metrics(
                MetricsOptions::new()
                    .cooperate_label("C")
                    .population_share(true),
            );
        run_library(factory.into_builder(seed).unwrap())
    });
}

#[test]
fn cournot_matches_the_library_builder_until_convergence_or_t_max() {
    const T_MAX: u64 = 60;
    let scenario = shortened(COURNOT, T_MAX, None);
    assert_equivalent("cournot", &scenario, |seed| {
        let game = ContinuousSpec::CournotGame(Cournot::new(3, 100.0, 1.0, 10.0, 100.0).unwrap());
        let initial = uniform_initial_actions(&game, 3, seed);
        let factory = GameSimFactory::new(game)
            .agents(3)
            .matching(WholePopulation::new())
            .rule(
                "damped_best_response",
                DampedBestResponse::new(0.5).unwrap(),
            )
            .initial_held(InitialActions::PerAgent(initial))
            .history_len(16)
            .t_max(T_MAX)
            .aggregates(16)
            .continuous_metrics(
                ContinuousMetricsOptions::new()
                    .equilibrium(ContinuousEquilibrium::Solve)
                    .stop(1e-9, 10),
            );
        run_library(factory.into_builder(seed).unwrap())
    });
}

const COUNTING_HAWK_DOVE: &str = r#"
[simulation]
name = "hawk_dove_counting"
module_pack = "normal-form"
t_max = 12
seed = 0
scheduler = "sequential"

[world]
n_agents = 10
history_len = 10

[world.game]
kind = "hawk_dove"
v = 2.0
c = 3.0

[world.population]
assign = "ordered"
[[world.population.group]]
rule = "uniform_random"
count = 10

[[mechanism]]
name = "game_match"
[mechanism.params]
matching = "random_pairs"

[[mechanism]]
name = "game_decide"

[[mechanism]]
name = "game_play"
[mechanism.params]
counting = true

[[mechanism]]
name = "game_metrics"

[output]
log_path = "unused.jsonl"
"#;

#[test]
fn counting_hawk_dove_matches_the_library_builder() {
    const T_MAX: u64 = 12;
    let scenario = Scenario::parse(COUNTING_HAWK_DOVE).unwrap();
    assert_equivalent("hawk_dove_counting", &scenario, |seed| {
        let game = HawkDove::new(2.0, 3.0).unwrap();
        let factory = GameSimFactory::new(game)
            .agents(10)
            .matching(RandomPairs::new())
            .rule("uniform_random", UniformRandom)
            .assign(Assignment::Ordered(ordered(&[("uniform_random", 10)])))
            .history_len(10)
            .t_max(T_MAX)
            .counting_play()
            .metrics(MetricsOptions::new());
        run_library(factory.into_builder(seed).unwrap())
    });
}
