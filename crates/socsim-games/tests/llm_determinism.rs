use std::sync::Arc;

use socsim_engine::Simulation;
use socsim_games::llm::{
    assemble_with_llm, BackendFactory, BackendIdentity, CountsObs, CountsRenderer, LabelParser,
    LlmExecutor, LlmPolicy, LlmReplayManifest, LlmRuleBook, LlmSetup, ReplayMode, ResponseStore,
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
            r#"{"action":"a1","reason":"deterministic"}"#,
        )))
    })
}

fn build_simulation(
    store: ResponseStore,
    executor: LlmExecutor,
    seed: u64,
    t_max: u64,
) -> Simulation<World> {
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
    let backend_identity = identity();
    let world = build_world_with(
        game(),
        4,
        WithLlm {
            inner: (),
            llm: LlmReplayManifest::for_store(&store, backend_identity.clone()),
        },
        InitialActions::Same(0),
        &catalog,
        &Assignment::Ordered(vec![AssignGroup {
            kind: AssignKind::Rule("llm".to_owned()),
            count: 4,
        }]),
        4,
        t_max,
        seed,
    )
    .unwrap();
    let setup = LlmSetup::new(llm_rules, store, backend(), backend_identity).executor(executor);
    assemble_with_llm(
        world,
        FixedPlayers::pairs(&[(0, 1), (2, 3)]),
        rules,
        setup,
        NormalFormOutcome,
        PlayOptions::new(),
        seed,
    )
    .unwrap()
    .build()
}

fn run_steps(simulation: &mut Simulation<World>, count: u64) -> Vec<String> {
    (0..count)
        .map(|_| {
            simulation.step().unwrap();
            serde_json::to_string(simulation.world()).unwrap()
        })
        .collect()
}

#[test]
fn sequential_and_four_worker_execution_have_identical_worlds_each_step() {
    let mut sequential = build_simulation(
        ResponseStore::in_memory("determinism", ReplayMode::Live),
        LlmExecutor::Sequential,
        41,
        6,
    );
    let mut threaded = build_simulation(
        ResponseStore::in_memory("determinism", ReplayMode::Live),
        LlmExecutor::Threaded { workers: 4 },
        41,
        6,
    );
    assert_eq!(run_steps(&mut sequential, 6), run_steps(&mut threaded, 6));
}

#[test]
fn replay_only_snapshot_restore_matches_an_uninterrupted_replay() {
    let directory = tempfile::tempdir().unwrap();
    let mut prime = build_simulation(
        ResponseStore::open(directory.path(), ReplayMode::RecordMisses, "prime", 53).unwrap(),
        LlmExecutor::Threaded { workers: 4 },
        53,
        6,
    );
    run_steps(&mut prime, 6);
    ResponseStore::merge_shards(directory.path()).unwrap();

    let mut uninterrupted = build_simulation(
        ResponseStore::open(directory.path(), ReplayMode::ReplayOnly, "full", 53).unwrap(),
        LlmExecutor::Sequential,
        53,
        6,
    );
    let expected = run_steps(&mut uninterrupted, 6);

    let mut partial = build_simulation(
        ResponseStore::open(directory.path(), ReplayMode::ReplayOnly, "partial", 53).unwrap(),
        LlmExecutor::Sequential,
        53,
        6,
    );
    run_steps(&mut partial, 2);
    let snapshot = partial.snapshot();
    let mut resumed = build_simulation(
        ResponseStore::open(directory.path(), ReplayMode::ReplayOnly, "resumed", 0).unwrap(),
        LlmExecutor::Threaded { workers: 4 },
        0,
        6,
    );
    resumed.restore(snapshot);
    assert_eq!(run_steps(&mut resumed, 4), expected[2..]);
}
