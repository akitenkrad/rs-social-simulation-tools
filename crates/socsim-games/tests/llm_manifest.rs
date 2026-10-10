use std::fs;
use std::sync::Arc;

use socsim_core::{SocsimError, WorldState};
use socsim_engine::{Simulation, Snapshot};
use socsim_games::llm::{
    assemble_with_llm, verify_snapshot, BackendFactory, BackendIdentity, CountsObs, CountsRenderer,
    LabelParser, LlmPolicy, LlmReplayManifest, LlmRuleBook, LlmSetup, ReplayMode, ResponseStore,
    WithLlm,
};
use socsim_games::{
    build_world_with, AssignGroup, AssignKind, Assignment, FixedPlayers, GameWorld, InitialActions,
    KindCatalog, MatrixGame, NormalFormOutcome, PlayOptions, RuleBook,
};
use socsim_llm::mock::ScriptedClient;

type X = WithLlm<()>;
type World = GameWorld<MatrixGame, X>;

fn game() -> MatrixGame {
    MatrixGame::zero_sum(&[vec![1.0, -1.0], vec![-1.0, 1.0]])
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

fn backend() -> BackendFactory {
    Arc::new(|| {
        Ok(Box::new(ScriptedClient::constant(
            "scripted",
            r#"{"action":"a0","reason":"replay"}"#,
        )))
    })
}

fn build_simulation(store: ResponseStore, identity: BackendIdentity) -> Simulation<World> {
    let rules: RuleBook<MatrixGame, X> = RuleBook::new();
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
    let world = build_world_with(
        game(),
        2,
        WithLlm {
            inner: (),
            llm: LlmReplayManifest::for_store(&store, identity.clone()),
        },
        InitialActions::Same(0),
        &catalog,
        &Assignment::Ordered(vec![
            AssignGroup {
                kind: AssignKind::Rule("llm".to_owned()),
                count: 1,
            },
            AssignGroup {
                kind: AssignKind::Held,
                count: 1,
            },
        ]),
        1,
        1,
        61,
    )
    .unwrap();
    let setup = LlmSetup::new(llm_rules, store, backend(), identity);
    assemble_with_llm(
        world,
        FixedPlayers::pairs(&[(0, 1)]),
        rules,
        setup,
        NormalFormOutcome,
        PlayOptions::new(),
        61,
    )
    .unwrap()
    .build()
}

fn assert_manifest_mismatch(error: socsim_games::GameError) {
    assert!(
        matches!(error, socsim_games::GameError::ManifestMismatch(_)),
        "unexpected error: {error}"
    );
}

#[test]
fn with_llm_world_satisfies_engine_bounds_and_round_trips_through_snapshot_json() {
    fn assert_world<W: WorldState + Send + 'static>() {}
    assert_world::<World>();

    let simulation = build_simulation(
        ResponseStore::in_memory("same", ReplayMode::ReplayOnly),
        identity(),
    );
    let value = serde_json::to_value(simulation.snapshot()).unwrap();
    let _: Snapshot<World> = serde_json::from_value(value).unwrap();
}

#[test]
fn verify_snapshot_checks_store_id_base_digest_mode_and_backend_identity() {
    let directory_a = tempfile::tempdir().unwrap();
    let directory_b = tempfile::tempdir().unwrap();
    fs::write(directory_a.path().join("store_id"), "shared").unwrap();
    fs::write(directory_b.path().join("store_id"), "shared").unwrap();
    fs::write(directory_b.path().join("base.jsonl"), "\n").unwrap();
    let source_store =
        ResponseStore::open(directory_a.path(), ReplayMode::ReplayOnly, "source", 1).unwrap();
    let simulation = build_simulation(source_store, identity());
    let snapshot = simulation.snapshot();

    let same = ResponseStore::open(directory_a.path(), ReplayMode::ReplayOnly, "same", 2).unwrap();
    verify_snapshot(&snapshot, &same, &identity()).unwrap();

    let wrong_id = ResponseStore::in_memory("other", ReplayMode::ReplayOnly);
    assert_manifest_mismatch(verify_snapshot(&snapshot, &wrong_id, &identity()).unwrap_err());
    let wrong_base =
        ResponseStore::open(directory_b.path(), ReplayMode::ReplayOnly, "wrong-base", 3).unwrap();
    assert_manifest_mismatch(verify_snapshot(&snapshot, &wrong_base, &identity()).unwrap_err());
    let wrong_mode = ResponseStore::open(
        directory_a.path(),
        ReplayMode::RecordMisses,
        "wrong-mode",
        4,
    )
    .unwrap();
    assert_manifest_mismatch(verify_snapshot(&snapshot, &wrong_mode, &identity()).unwrap_err());
    let wrong_backend = BackendIdentity {
        model: "other".to_owned(),
        endpoint: "mock://other".to_owned(),
    };
    assert_manifest_mismatch(verify_snapshot(&snapshot, &same, &wrong_backend).unwrap_err());
}

#[test]
fn restored_manifest_is_checked_on_the_first_decision_and_replay_miss_stops() {
    let source = build_simulation(
        ResponseStore::in_memory("source", ReplayMode::ReplayOnly),
        identity(),
    );
    let snapshot = source.snapshot();
    let mut wrong_driver = build_simulation(
        ResponseStore::in_memory("other", ReplayMode::ReplayOnly),
        identity(),
    );
    wrong_driver.restore(snapshot);
    match wrong_driver.step().unwrap_err() {
        SocsimError::Mechanism(message) => assert!(message.contains("manifest"), "{message}"),
        other => panic!("expected mechanism error, got {other:?}"),
    }

    let mut missing = build_simulation(
        ResponseStore::in_memory("missing", ReplayMode::ReplayOnly),
        identity(),
    );
    match missing.step().unwrap_err() {
        SocsimError::Mechanism(message) => assert!(message.contains("ReplayOnly"), "{message}"),
        other => panic!("expected mechanism error, got {other:?}"),
    }
}

#[test]
fn restoring_into_a_running_simulation_is_checked_on_the_next_decision() {
    let mut running = build_simulation(
        ResponseStore::in_memory("running", ReplayMode::RecordMisses),
        identity(),
    );
    running.step().unwrap();

    let other = build_simulation(
        ResponseStore::in_memory("other", ReplayMode::RecordMisses),
        identity(),
    );
    running.restore(other.snapshot());
    match running.step().unwrap_err() {
        SocsimError::Mechanism(message) => assert!(message.contains("manifest"), "{message}"),
        other => panic!("expected mechanism error, got {other:?}"),
    }
}
