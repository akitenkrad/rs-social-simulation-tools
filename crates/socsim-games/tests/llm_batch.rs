use std::any::Any;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use socsim_core::{derive_seed, Recorder};
use socsim_engine::Simulation;
use socsim_games::llm::{
    assemble_with_llm, BackendFactory, BackendIdentity, CountsObs, CountsRenderer, LabelParser,
    LlmPolicy, LlmReplayManifest, LlmRuleBook, LlmSetup, ReplayMode, ResponseStore, WithLlm,
};
use socsim_games::{
    build_world_with, AssignGroup, AssignKind, Assignment, Fixed, FixedPlayers, GameWorld,
    InitialActions, KindCatalog, MatrixGame, NormalFormOutcome, PlayOptions, RuleBook,
};
use socsim_llm::mock::ScriptedClient;
use socsim_llm::LlmConfig;

#[derive(Debug)]
struct Event {
    kind: String,
    payload: serde_json::Value,
}

#[derive(Default)]
struct CaptureRecorder {
    rows: Vec<(u64, String, BTreeMap<String, f64>)>,
    events: Vec<Event>,
}

impl Recorder for CaptureRecorder {
    fn record_metric(&mut self, _t: u64, _key: &str, _value: f64) {}

    fn record_event(&mut self, _t: u64, kind: &str, payload: serde_json::Value) {
        self.events.push(Event {
            kind: kind.to_owned(),
            payload,
        });
    }

    fn record_row(&mut self, t: u64, table: &str, row: &[(&str, f64)]) {
        self.rows.push((
            t,
            table.to_owned(),
            row.iter()
                .map(|(key, value)| ((*key).to_owned(), *value))
                .collect(),
        ));
    }

    fn as_any(&self) -> Option<&dyn Any> {
        Some(self)
    }
}

type X = WithLlm<()>;
type World = GameWorld<MatrixGame, X>;

fn game() -> MatrixGame {
    MatrixGame::bimatrix(
        &[vec![3.0, 0.0], vec![5.0, 1.0]],
        &[vec![3.0, 5.0], vec![0.0, 1.0]],
    )
    .unwrap()
    .with_labels(vec![
        vec!["a0".to_owned(), "a1".to_owned()],
        vec!["a0".to_owned(), "a1".to_owned()],
    ])
    .unwrap()
}

fn identity() -> BackendIdentity {
    BackendIdentity {
        model: "scripted".to_owned(),
        endpoint: "mock://scripted".to_owned(),
    }
}

fn build_simulation(temperature: f32, seed: u64) -> (Simulation<World>, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let observed_calls = Arc::clone(&calls);
    let backend: BackendFactory = Arc::new(move || {
        let observed_calls = Arc::clone(&observed_calls);
        Ok(Box::new(ScriptedClient::new("scripted", move |_| {
            observed_calls.fetch_add(1, Ordering::SeqCst);
            r#"{"action":"a1","reason":"scripted"}"#.to_owned()
        })))
    });

    let mut rules: RuleBook<MatrixGame, X> = RuleBook::new();
    rules.register("fixed", Arc::new(Fixed::new(0))).unwrap();
    let mut llm_rules: LlmRuleBook<MatrixGame, X> = LlmRuleBook::new();
    llm_rules
        .register(
            "llm",
            Arc::new(LlmPolicy::new(
                CountsObs,
                CountsRenderer::default(),
                LabelParser::strict(),
            )),
        )
        .unwrap();

    let catalog = KindCatalog::new()
        .with_rules(&rules)
        .unwrap()
        .with_llm(&llm_rules)
        .unwrap();
    let store = ResponseStore::in_memory("batch-store", ReplayMode::RecordMisses);
    let backend_identity = identity();
    let x = WithLlm {
        inner: (),
        llm: LlmReplayManifest::for_store(&store, backend_identity.clone()),
    };
    let world = build_world_with(
        game(),
        4,
        x,
        InitialActions::Same(0),
        &catalog,
        &Assignment::Ordered(vec![
            AssignGroup {
                kind: AssignKind::Rule("llm".to_owned()),
                count: 1,
            },
            AssignGroup {
                kind: AssignKind::Rule("fixed".to_owned()),
                count: 1,
            },
            AssignGroup {
                kind: AssignKind::Rule("llm".to_owned()),
                count: 1,
            },
            AssignGroup {
                kind: AssignKind::Rule("fixed".to_owned()),
                count: 1,
            },
        ]),
        2,
        1,
        seed,
    )
    .unwrap();
    let setup = LlmSetup::new(llm_rules, store, backend, backend_identity)
        .config(LlmConfig::deterministic().with_temperature(temperature));
    let simulation = assemble_with_llm(
        world,
        FixedPlayers::pairs(&[(0, 1), (2, 3)]),
        rules,
        setup,
        NormalFormOutcome,
        PlayOptions::new(),
        seed,
    )
    .unwrap()
    .recorder(Box::new(CaptureRecorder::default()))
    .build();
    (simulation, calls)
}

fn llm_events(recorder: &CaptureRecorder) -> Vec<&serde_json::Value> {
    recorder
        .events
        .iter()
        .filter(|event| event.kind == "game.llm_call")
        .map(|event| &event.payload)
        .collect()
}

#[test]
fn deterministic_requests_are_deduplicated_and_mixed_drivers_fill_their_own_seats() {
    let seed = 73;
    let (mut simulation, calls) = build_simulation(0.0, seed);
    simulation.step().unwrap();

    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(simulation.world().held(), &[1, 0, 1, 0]);
    assert_eq!(
        simulation
            .world()
            .plays()
            .actions(socsim_games::EncounterId(0))
            .unwrap(),
        &[Some(1), Some(0)]
    );
    assert_eq!(
        simulation
            .world()
            .plays()
            .actions(socsim_games::EncounterId(1))
            .unwrap(),
        &[Some(1), Some(0)]
    );

    let recorder = simulation
        .recorder()
        .as_any()
        .unwrap()
        .downcast_ref::<CaptureRecorder>()
        .unwrap();
    let events = llm_events(recorder);
    assert_eq!(events.len(), 2);
    assert_eq!(events[0]["agent"], serde_json::json!(0));
    assert_eq!(events[1]["agent"], serde_json::json!(2));
    assert_eq!(events[0]["deduplicated"], serde_json::json!(false));
    assert_eq!(events[1]["deduplicated"], serde_json::json!(true));
    assert_eq!(events[0]["key"], events[1]["key"]);
    assert_eq!(
        events[0]["seed"],
        serde_json::json!(derive_seed(derive_seed(seed, &[2]), &[0]))
    );

    for event in events {
        for key in [
            "agent",
            "kind",
            "unit",
            "unit_index",
            "attempt",
            "resolution",
            "system",
            "prompt",
            "reply",
            "thinking",
            "seed",
            "usage",
            "cache_hit",
            "deduplicated",
            "key",
            "flags",
        ] {
            assert!(event.get(key).is_some(), "missing event field {key}");
        }
        assert_eq!(event["unit"], serde_json::json!("step"));
        assert_eq!(event["unit_index"], serde_json::json!(0));
        assert_eq!(event["attempt"], serde_json::json!(0));
        assert_eq!(event["resolution"], serde_json::json!("accept"));
    }

    assert_eq!(recorder.rows.len(), 1);
    let (t, table, row) = &recorder.rows[0];
    assert_eq!((*t, table.as_str()), (1, "game"));
    for key in [
        "llm.parse_fail_rate",
        "llm.retry_rate",
        "llm.clamp_rate",
        "llm.value_was_string_rate",
        "llm.lenient_rate",
        "llm.cache_hit_rate",
        "llm.giveup_rate",
    ] {
        assert!(row.contains_key(key), "missing rate {key}");
    }
    assert_eq!(row["llm.retry_rate"], 0.0);
    assert_eq!(row["llm.giveup_rate"], 0.0);
}

#[test]
fn positive_temperature_uses_agent_specific_keys_and_does_not_deduplicate() {
    let seed = 79;
    let (mut simulation, calls) = build_simulation(0.7, seed);
    simulation.step().unwrap();

    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let recorder = simulation
        .recorder()
        .as_any()
        .unwrap()
        .downcast_ref::<CaptureRecorder>()
        .unwrap();
    let events = llm_events(recorder);
    assert_eq!(events.len(), 2);
    assert_ne!(events[0]["key"], events[1]["key"]);
    assert_eq!(events[0]["deduplicated"], serde_json::json!(false));
    assert_eq!(events[1]["deduplicated"], serde_json::json!(false));
    assert_eq!(
        events[0]["seed"],
        serde_json::json!(derive_seed(derive_seed(seed, &[2]), &[1, 0, 0, 0, 0]))
    );
    assert_eq!(
        events[1]["seed"],
        serde_json::json!(derive_seed(derive_seed(seed, &[2]), &[1, 2, 0, 0, 0]))
    );
}
