use std::any::Any;
use std::sync::Arc;

use serde_json::{json, Value};
use socsim_core::Recorder;
use socsim_engine::Simulation;
use socsim_games::catalog::{PrisonersDilemma, PublicGoodsFinite};
use socsim_games::continuous::Cournot;
use socsim_games::{
    assemble, assemble_async, build_world, counting_play, AssignGroup, AssignKind, Assignment,
    AsyncPopulationStep, ContinuousMetrics, ContinuousMetricsOptions, DecisionScope, FixedPlayers,
    GameMetrics, GameWorld, InitialActions, MatrixGame, MetricsOptions, NormalFormOutcome,
    PayoffTiming, PlayOptions, ProportionalImitation, RandomPairs, RoundRobin, RuleBook, TitForTat,
    UniformRandom, Update, WholePopulation,
};

#[derive(Clone, Debug, PartialEq)]
struct MetricRow {
    t: u64,
    key: String,
    value: f64,
}

#[derive(Clone, Debug, PartialEq)]
struct EventRow {
    t: u64,
    kind: String,
    payload: Value,
}

#[derive(Default)]
struct CaptureRecorder {
    metrics: Vec<MetricRow>,
    events: Vec<EventRow>,
}

impl Recorder for CaptureRecorder {
    fn record_metric(&mut self, t: u64, key: &str, value: f64) {
        self.metrics.push(MetricRow {
            t,
            key: key.to_owned(),
            value,
        });
    }

    fn record_event(&mut self, t: u64, kind: &str, payload: Value) {
        self.events.push(EventRow {
            t,
            kind: kind.to_owned(),
            payload,
        });
    }

    fn record_row(&mut self, t: u64, table: &str, row: &[(&str, f64)]) {
        self.metrics
            .extend(row.iter().map(|(column, value)| MetricRow {
                t,
                key: format!("{table}.{column}"),
                value: *value,
            }));
    }

    fn as_any(&self) -> Option<&dyn Any> {
        Some(self)
    }
}

fn capture<G, X>(simulation: &Simulation<GameWorld<G, X>>) -> &CaptureRecorder
where
    G: socsim_games::StageGame,
    X: socsim_games::ExperimentState,
{
    simulation
        .recorder()
        .as_any()
        .and_then(|recorder| recorder.downcast_ref::<CaptureRecorder>())
        .unwrap()
}

fn held(count: usize) -> Assignment {
    Assignment::Ordered(vec![AssignGroup {
        kind: AssignKind::Held,
        count,
    }])
}

fn prisoners_dilemma() -> PrisonersDilemma {
    PrisonersDilemma::new(5.0, 3.0, 1.0, 0.0).unwrap()
}

fn ipd_simulation(
    seed: u64,
    t_max: u64,
    options: MetricsOptions,
) -> Simulation<GameWorld<PrisonersDilemma>> {
    let game = prisoners_dilemma();
    let metrics = GameMetrics::new(&game, options).unwrap();
    let mut rules = RuleBook::new();
    rules
        .register("tit_for_tat", Arc::new(TitForTat::new(0)))
        .unwrap();
    rules
        .register("uniform_random", Arc::new(UniformRandom))
        .unwrap();
    let world = build_world(
        game,
        2,
        (),
        InitialActions::Same(0),
        &rules,
        &Assignment::Ordered(vec![
            AssignGroup {
                kind: AssignKind::Rule("tit_for_tat".to_owned()),
                count: 1,
            },
            AssignGroup {
                kind: AssignKind::Rule("uniform_random".to_owned()),
                count: 1,
            },
        ]),
        1,
        t_max,
        seed,
    )
    .unwrap();
    assemble(
        world,
        FixedPlayers::pairs(&[(0, 1)]),
        rules,
        NormalFormOutcome,
        PlayOptions::new().materialize(true),
        seed,
    )
    .unwrap()
    .add_mechanism(Box::new(metrics))
    .recorder(Box::new(CaptureRecorder::default()))
    .build()
}

fn events_of<'a>(recorder: &'a CaptureRecorder, kind: &str) -> Vec<&'a EventRow> {
    recorder
        .events
        .iter()
        .filter(|event| event.kind == kind)
        .collect()
}

fn event_actions(event: &EventRow) -> &Vec<Value> {
    event.payload["actions"].as_array().unwrap()
}

fn reserved_payload_keys() -> [&'static str; 9] {
    [
        "schema", "run_uid", "ts", "unit_id", "t", "t_unit", "outcome", "censored", "budget",
    ]
}

#[test]
fn options_reject_zero_sampling_interval_and_agent_limit() {
    let game = prisoners_dilemma();

    let every = GameMetrics::new(&game, MetricsOptions::new().log_actions_every(0))
        .unwrap_err()
        .to_string();
    let maximum = GameMetrics::new(&game, MetricsOptions::new().log_actions_max_agents(0))
        .unwrap_err()
        .to_string();

    assert_eq!(
        every,
        "invalid game metrics configuration: log_actions_every must be greater than zero"
    );
    assert_eq!(
        maximum,
        "invalid game metrics configuration: log_actions_max_agents must be greater than zero"
    );
}

#[test]
fn ipd_records_roster_step_actions_and_materialized_plays() {
    let mut simulation = ipd_simulation(
        0,
        50,
        MetricsOptions::new().log_actions(true).log_plays(true),
    );

    for expected_t in 1..=50 {
        simulation.step().unwrap();
        let recorder = capture(&simulation);
        let step = events_of(recorder, "game.step_actions")
            .last()
            .copied()
            .unwrap();
        assert_eq!(step.t, expected_t);

        let plays = simulation.world().plays();
        assert_eq!(plays.n_encounters(), 1);
        let played = plays.actions(socsim_games::EncounterId(0)).unwrap();
        assert_eq!(
            event_actions(step),
            &json!(played).as_array().unwrap().clone()
        );
    }

    let recorder = capture(&simulation);
    let roster = events_of(recorder, "game.roster");
    let actions = events_of(recorder, "game.step_actions");
    let plays = events_of(recorder, "game.play");
    assert_eq!(roster.len(), 1);
    assert_eq!(actions.len(), 50);
    assert_eq!(plays.len(), 50);
    assert_eq!(recorder.events[0].kind, "game.roster");
    assert_eq!(recorder.events[1].kind, "game.step_actions");
    assert_eq!(
        roster[0].payload,
        json!({
            "agents": [0, 1],
            "kinds": ["tit_for_tat", "uniform_random"],
            "decision_scope": "per_step",
        })
    );
    assert_eq!(plays[0].payload["encounter"], 0);
    assert_eq!(plays[0].payload["seats"], json!([0, 1]));
    assert_eq!(plays[0].payload["credit"], "all");

    for event in &recorder.events {
        let object = event.payload.as_object().unwrap();
        for key in reserved_payload_keys() {
            assert!(!object.contains_key(key), "reserved payload key {key}");
        }
    }
}

#[test]
fn random_pairs_marks_one_idle_agent_each_step() {
    let game = prisoners_dilemma();
    let metrics = GameMetrics::new(&game, MetricsOptions::new().log_actions(true)).unwrap();
    let mut rules = RuleBook::new();
    rules
        .register("uniform_random", Arc::new(UniformRandom))
        .unwrap();
    let world = build_world(
        game,
        5,
        (),
        InitialActions::Same(0),
        &rules,
        &Assignment::Ordered(vec![AssignGroup {
            kind: AssignKind::Rule("uniform_random".to_owned()),
            count: 5,
        }]),
        1,
        10,
        17,
    )
    .unwrap();
    let mut simulation = assemble(
        world,
        RandomPairs::new(),
        rules,
        NormalFormOutcome,
        PlayOptions::new(),
        17,
    )
    .unwrap()
    .add_mechanism(Box::new(metrics))
    .recorder(Box::new(CaptureRecorder::default()))
    .build();

    simulation.run().unwrap();

    for event in events_of(capture(&simulation), "game.step_actions") {
        let actions = event.payload["actions"].as_array().unwrap();
        let n_played = event.payload["n_played"].as_array().unwrap();
        assert_eq!(actions.iter().filter(|action| action.is_null()).count(), 1);
        assert_eq!(
            n_played.iter().filter(|count| **count == json!(0)).count(),
            1
        );
        assert_eq!(
            actions
                .iter()
                .zip(n_played)
                .filter(|(action, count)| action.is_null() && **count == json!(0))
                .count(),
            1
        );
    }
}

fn role_specific_game() -> MatrixGame {
    MatrixGame::bimatrix(
        &[vec![2.0, 0.0], vec![1.0, 1.0]],
        &[vec![1.0, 2.0], vec![0.0, 1.0]],
    )
    .unwrap()
    .with_labels(vec![
        vec!["up".to_owned(), "down".to_owned()],
        vec!["left".to_owned(), "right".to_owned()],
    ])
    .unwrap()
}

#[test]
fn per_role_actions_record_a_unique_role_and_finer_scopes_are_rejected() {
    let game = role_specific_game();
    let rules = RuleBook::new();
    let world = build_world(
        game.clone(),
        2,
        (),
        InitialActions::PerAgent(vec![0, 1]),
        &rules,
        &held(2),
        1,
        1,
        19,
    )
    .unwrap();
    let metrics = GameMetrics::new(&game, MetricsOptions::new().log_actions(true)).unwrap();
    let mut fixed = assemble(
        world,
        FixedPlayers::pairs(&[(0, 1)]),
        rules,
        NormalFormOutcome,
        PlayOptions::new(),
        19,
    )
    .unwrap()
    .add_mechanism(Box::new(metrics))
    .recorder(Box::new(CaptureRecorder::default()))
    .build();
    fixed.step().unwrap();
    assert_eq!(
        events_of(capture(&fixed), "game.step_actions")[0].payload["roles"],
        json!([0, 1])
    );

    for scope in [DecisionScope::PerAgentRole, DecisionScope::PerEncounter] {
        let rules = RuleBook::new();
        let world = build_world(
            game.clone(),
            3,
            (),
            InitialActions::PerAgent(vec![0, 1, 0]),
            &rules,
            &held(3),
            1,
            1,
            23,
        )
        .unwrap();
        let metrics = GameMetrics::new(&game, MetricsOptions::new().log_actions(true)).unwrap();
        let mut simulation = assemble(
            world,
            RoundRobin::new(),
            rules,
            NormalFormOutcome,
            PlayOptions::new().decision_scope(scope),
            23,
        )
        .unwrap()
        .add_mechanism(Box::new(metrics))
        .recorder(Box::new(CaptureRecorder::default()))
        .build();

        let error = simulation.step().unwrap_err().to_string();
        assert!(error.contains("log_actions requires DecisionScope::PerStep"));
        assert!(error.contains("log_plays(true)"));
        assert!(capture(&simulation).events.is_empty());
        assert!(capture(&simulation).metrics.is_empty());
    }
}

#[test]
fn maximum_agent_check_precedes_all_output() {
    let game = prisoners_dilemma();
    let rules = RuleBook::new();
    let world = build_world(
        game.clone(),
        5,
        (),
        InitialActions::Same(0),
        &rules,
        &held(5),
        1,
        1,
        29,
    )
    .unwrap();
    let metrics = GameMetrics::new(
        &game,
        MetricsOptions::new()
            .log_actions(true)
            .log_actions_max_agents(4),
    )
    .unwrap();
    let mut simulation = assemble(
        world,
        RandomPairs::new(),
        rules,
        NormalFormOutcome,
        PlayOptions::new(),
        29,
    )
    .unwrap()
    .add_mechanism(Box::new(metrics))
    .recorder(Box::new(CaptureRecorder::default()))
    .build();

    let error = simulation.step().unwrap_err().to_string();
    assert!(error.contains("log_actions_max_agents"));
    assert!(capture(&simulation).events.is_empty());
    assert!(capture(&simulation).metrics.is_empty());
}

fn public_goods_simulation(
    materialize: bool,
    options: MetricsOptions,
) -> Simulation<GameWorld<PublicGoodsFinite>> {
    let game = PublicGoodsFinite::new(4, 2.0, 1.0).unwrap();
    let metrics = GameMetrics::new(&game, options).unwrap();
    let rules = RuleBook::new();
    let world = build_world(
        game,
        4,
        (),
        InitialActions::PerAgent(vec![0, 1, 0, 1]),
        &rules,
        &held(4),
        1,
        2,
        31,
    )
    .unwrap();
    assemble(
        world,
        WholePopulation::new(),
        rules,
        NormalFormOutcome,
        PlayOptions::new().materialize(materialize),
        31,
    )
    .unwrap()
    .add_mechanism(Box::new(metrics))
    .recorder(Box::new(CaptureRecorder::default()))
    .build()
}

#[test]
fn implicit_and_materialized_step_actions_match_and_plays_require_materialization() {
    let options = || MetricsOptions::new().log_actions(true);
    let mut implicit = public_goods_simulation(false, options());
    let mut materialized = public_goods_simulation(true, options());
    implicit.step().unwrap();
    materialized.step().unwrap();
    assert_eq!(
        events_of(capture(&implicit), "game.step_actions")[0].payload,
        events_of(capture(&materialized), "game.step_actions")[0].payload
    );

    let mut materialized_plays = public_goods_simulation(
        true,
        MetricsOptions::new().log_actions(true).log_plays(true),
    );
    materialized_plays.step().unwrap();
    let plays = events_of(capture(&materialized_plays), "game.play");
    assert_eq!(plays.len(), 1);
    assert_eq!(plays[0].payload["seats"], json!([0, 1, 2, 3]));
    assert_eq!(plays[0].payload["actions"], json!([0, 1, 0, 1]));
    assert_eq!(plays[0].payload["credit"], "all");

    let mut invalid = public_goods_simulation(false, MetricsOptions::new().log_plays(true));
    let error = invalid.step().unwrap_err().to_string();
    assert!(error.contains("requires the materialized play path"));
    assert!(error.contains("PlayOptions::materialize(true)"));
    assert!(error.contains("not available with `async_population_step`"));
    assert!(capture(&invalid).events.is_empty());
    assert!(capture(&invalid).metrics.is_empty());
}

#[test]
fn action_sampling_uses_t_modulo_interval_but_roster_is_immediate() {
    let mut simulation = ipd_simulation(
        37,
        10,
        MetricsOptions::new().log_actions(true).log_actions_every(3),
    );
    simulation.run().unwrap();

    let recorder = capture(&simulation);
    assert_eq!(
        events_of(recorder, "game.roster")
            .iter()
            .map(|event| event.t)
            .collect::<Vec<_>>(),
        [1]
    );
    assert_eq!(
        events_of(recorder, "game.step_actions")
            .iter()
            .map(|event| event.t)
            .collect::<Vec<_>>(),
        [3, 6, 9]
    );
}

#[test]
fn snapshot_resume_reemits_roster_and_continues_action_times_and_values() {
    let options = || MetricsOptions::new().log_actions(true);
    let mut uninterrupted = ipd_simulation(41, 10, options());
    uninterrupted.run().unwrap();
    let expected: Vec<_> = events_of(capture(&uninterrupted), "game.step_actions")
        .into_iter()
        .filter(|event| event.t > 5)
        .cloned()
        .collect();

    let mut partial = ipd_simulation(41, 10, options());
    for _ in 0..5 {
        partial.step().unwrap();
    }
    let snapshot = partial.snapshot();
    let world_json = serde_json::to_value(partial.world()).unwrap();
    assert!(world_json.get("roster_logged").is_none());

    let mut resumed = ipd_simulation(999, 10, options());
    resumed.restore(snapshot);
    resumed.run().unwrap();

    let recorder = capture(&resumed);
    assert_eq!(events_of(recorder, "game.roster").len(), 1);
    assert_eq!(events_of(recorder, "game.roster")[0].t, 6);
    assert_eq!(
        events_of(recorder, "game.step_actions")
            .into_iter()
            .cloned()
            .collect::<Vec<_>>(),
        expected
    );
}

#[test]
fn logging_does_not_change_world_or_metric_rows() {
    let mut baseline = ipd_simulation(43, 5, MetricsOptions::new());
    let mut logged = ipd_simulation(
        43,
        5,
        MetricsOptions::new().log_actions(true).log_plays(true),
    );

    for _ in 0..5 {
        baseline.step().unwrap();
        logged.step().unwrap();
        assert_eq!(
            serde_json::to_value(baseline.world()).unwrap(),
            serde_json::to_value(logged.world()).unwrap()
        );
        assert_eq!(capture(&baseline).metrics, capture(&logged).metrics);
    }
}

#[test]
fn continuous_metrics_records_vector_actions_for_materialized_plays() {
    let game = Cournot::new(3, 100.0, 1.0, 10.0, 100.0).unwrap();
    let metrics =
        ContinuousMetrics::new(&game, ContinuousMetricsOptions::new().log_plays(true)).unwrap();
    let rules = RuleBook::new();
    let world = build_world(
        game,
        3,
        (),
        InitialActions::PerAgent(vec![vec![0.0], vec![50.0], vec![100.0]]),
        &rules,
        &held(3),
        1,
        1,
        47,
    )
    .unwrap();
    let mut simulation = assemble(
        world,
        WholePopulation::new(),
        rules,
        NormalFormOutcome,
        PlayOptions::new().materialize(true),
        47,
    )
    .unwrap()
    .add_mechanism(Box::new(metrics))
    .recorder(Box::new(CaptureRecorder::default()))
    .build();

    simulation.step().unwrap();

    let plays = events_of(capture(&simulation), "game.play");
    assert_eq!(plays.len(), 1);
    assert_eq!(plays[0].payload["actions"], json!([[0.0], [50.0], [100.0]]));
    assert_eq!(plays[0].payload["seats"], json!([0, 1, 2]));
}

fn async_public_goods(options: MetricsOptions) -> Simulation<GameWorld<PublicGoodsFinite>> {
    let game = PublicGoodsFinite::new(4, 2.0, 1.0).unwrap();
    let metrics = GameMetrics::new(&game, options).unwrap();
    let rules = RuleBook::new();
    let world = build_world(
        game,
        4,
        (),
        InitialActions::PerAgent(vec![0, 1, 0, 1]),
        &rules,
        &held(4),
        1,
        1,
        53,
    )
    .unwrap();
    assemble_async(
        world,
        WholePopulation::new(),
        counting_play(),
        AsyncPopulationStep::new(ProportionalImitation::new(0.1, Update::Asynchronous).unwrap()),
        PlayOptions::new(),
        53,
    )
    .unwrap()
    .add_mechanism(Box::new(metrics))
    .recorder(Box::new(CaptureRecorder::default()))
    .build()
}

#[test]
fn asynchronous_actions_and_payoffs_are_the_final_updated_values() {
    let mut simulation = async_public_goods(
        MetricsOptions::new()
            .payoff_timing(PayoffTiming::AfterUpdate)
            .log_actions(true),
    );
    simulation.step().unwrap();

    let step = events_of(capture(&simulation), "game.step_actions")[0];
    assert_eq!(step.payload["actions"], json!(simulation.world().held()));
    assert_eq!(step.payload["payoffs"], json!(simulation.world().payoffs()));

    let mut invalid = async_public_goods(MetricsOptions::new().log_plays(true));
    let error = invalid.step().unwrap_err().to_string();
    assert!(error.contains("requires the materialized play path"));
    assert!(error.contains("PlayOptions::materialize(true)"));
    assert!(error.contains("not available with `async_population_step`"));
}
