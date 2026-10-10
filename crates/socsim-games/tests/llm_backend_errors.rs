use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use socsim_core::SocsimError;
use socsim_engine::Simulation;
use socsim_games::llm::{
    assemble_with_llm, BackendFactory, BackendIdentity, CountsObs, CountsRenderer, ExecutorOptions,
    LabelParser, LlmExecutor, LlmPolicy, LlmReplayManifest, LlmRuleBook, LlmSetup, ReplayMode,
    ResponseStore, WithLlm,
};
use socsim_games::{
    build_world_with, AssignGroup, AssignKind, Assignment, FixedPlayers, GameWorld, InitialActions,
    KindCatalog, MatrixGame, NormalFormOutcome, PlayOptions, RuleBook,
};
use socsim_llm::{LlmClient, LlmConfig, LlmError, LlmResponse};

#[derive(Clone, Copy)]
enum Behavior {
    Fail,
    Delay(Duration),
}

struct ErrorClient {
    behavior: Behavior,
    calls: Arc<AtomicUsize>,
}

impl LlmClient for ErrorClient {
    fn model(&self) -> &str {
        "error-client"
    }
    fn endpoint(&self) -> &str {
        "mock://error"
    }
    fn complete(&self, _prompt: &str, _config: &LlmConfig) -> Result<LlmResponse, LlmError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        match self.behavior {
            Behavior::Fail => Err(test_error("always fails")),
            Behavior::Delay(delay) => {
                thread::sleep(delay);
                Err(test_error("late failure"))
            }
        }
    }
}

fn test_error(message: &str) -> LlmError {
    LlmError::Transport {
        endpoint: "mock://error".to_owned(),
        message: message.to_owned(),
    }
}

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

fn build_simulation(
    behavior: Behavior,
    calls: Arc<AtomicUsize>,
    executor: LlmExecutor,
    options: ExecutorOptions,
) -> Simulation<World> {
    let backend: BackendFactory = Arc::new(move || {
        Ok(Box::new(ErrorClient {
            behavior,
            calls: Arc::clone(&calls),
        }))
    });
    let identity = BackendIdentity {
        model: "error-client".to_owned(),
        endpoint: "mock://error".to_owned(),
    };
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
    let store = ResponseStore::in_memory("backend-errors", ReplayMode::Live);
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
        29,
    )
    .unwrap();
    let setup = LlmSetup::new(llm_rules, store, backend, identity)
        .executor(executor)
        .options(options);
    assemble_with_llm(
        world,
        FixedPlayers::pairs(&[(0, 1)]),
        rules,
        setup,
        NormalFormOutcome,
        PlayOptions::new(),
        29,
    )
    .unwrap()
    .build()
}

fn assert_backend_mechanism_error(error: SocsimError) {
    match error {
        SocsimError::Mechanism(message) => {
            assert!(message.contains("LLM backend error"), "{message}")
        }
        other => panic!("expected mechanism error, got {other:?}"),
    }
}

#[test]
fn backend_retry_exhaustion_stops_the_driver_with_rule_error_backend() {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut options = ExecutorOptions::default();
    options.max_backend_retries = 3;
    options.backoff_base = Duration::ZERO;
    let mut simulation = build_simulation(
        Behavior::Fail,
        Arc::clone(&calls),
        LlmExecutor::Sequential,
        options,
    );
    assert_backend_mechanism_error(simulation.step().unwrap_err());
    assert_eq!(calls.load(Ordering::SeqCst), 4);
}

#[test]
fn threaded_timeout_stops_the_driver_without_resending_the_request() {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut options = ExecutorOptions::default();
    options.timeout = Duration::from_millis(20);
    options.max_backend_retries = 3;
    options.backoff_base = Duration::ZERO;
    let mut simulation = build_simulation(
        Behavior::Delay(Duration::from_millis(200)),
        Arc::clone(&calls),
        LlmExecutor::Threaded { workers: 1 },
        options,
    );
    let started = Instant::now();
    assert_backend_mechanism_error(simulation.step().unwrap_err());
    assert!(started.elapsed() < Duration::from_millis(150));
    thread::sleep(Duration::from_millis(250));
    assert_eq!(calls.load(Ordering::SeqCst), 1, "timeout must not retry");
}
