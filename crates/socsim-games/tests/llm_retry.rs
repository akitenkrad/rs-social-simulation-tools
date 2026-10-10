use std::any::Any;
use std::collections::BTreeMap;
use std::sync::Arc;

use socsim_core::Recorder;
use socsim_engine::Simulation;
use socsim_games::llm::{
    assemble_with_llm, BackendFactory, BackendIdentity, ConversationTurn, CountsObs,
    CountsRenderer, Fallback, LabelParser, LlmConversation, LlmPolicy, LlmReplayManifest,
    LlmRuleBook, LlmSetup, ReplayMode, ResponseStore, RetryProtocol, WithLlm,
};
use socsim_games::{
    build_world_with, AssignGroup, AssignKind, Assignment, Fixed, FixedPlayers, GameWorld,
    InitialActions, KindCatalog, MatrixGame, NormalFormOutcome, PlayOptions, RuleBook, Slot,
    UniformRandom,
};
use socsim_llm::mock::ScriptedClient;

#[derive(Default)]
struct CaptureRecorder {
    rows: Vec<BTreeMap<String, f64>>,
    events: Vec<serde_json::Value>,
}

impl Recorder for CaptureRecorder {
    fn record_metric(&mut self, _t: u64, _key: &str, _value: f64) {}
    fn record_event(&mut self, _t: u64, kind: &str, payload: serde_json::Value) {
        if kind == "game.llm_call" {
            self.events.push(payload);
        }
    }
    fn record_row(&mut self, _t: u64, table: &str, row: &[(&str, f64)]) {
        if table == "game" {
            self.rows.push(
                row.iter()
                    .map(|(key, value)| ((*key).to_owned(), *value))
                    .collect(),
            );
        }
    }
    fn as_any(&self) -> Option<&dyn Any> {
        Some(self)
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

fn identity() -> BackendIdentity {
    BackendIdentity {
        model: "scripted".to_owned(),
        endpoint: "mock://scripted".to_owned(),
    }
}

fn invalid_backend() -> BackendFactory {
    Arc::new(|| {
        Ok(Box::new(ScriptedClient::constant(
            "scripted",
            "not valid JSON",
        )))
    })
}

fn policy(
    fallback: Fallback<usize>,
) -> LlmPolicy<MatrixGame, CountsObs, CountsRenderer, LabelParser> {
    LlmPolicy::new(CountsObs, CountsRenderer::default(), LabelParser::strict()).with_retry(
        RetryProtocol {
            max_retries: 1,
            fallback,
        },
    )
}

#[test]
fn retries_form_a_second_batch_then_give_up_and_record_the_conversation() {
    let seed = 97;
    let mut rules: RuleBook<MatrixGame, X> = RuleBook::new();
    rules.register("fixed", Arc::new(Fixed::new(0))).unwrap();
    let mut llm_rules: LlmRuleBook<MatrixGame, X> = LlmRuleBook::new();
    llm_rules
        .register("llm", Arc::new(policy(Fallback::Previous)))
        .unwrap();
    let catalog = KindCatalog::new()
        .with_rules(&rules)
        .unwrap()
        .with_llm(&llm_rules)
        .unwrap();
    let store = ResponseStore::in_memory("retry", ReplayMode::Live);
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
    let setup = LlmSetup::new(llm_rules, store, invalid_backend(), backend_identity);
    let mut simulation = assemble_with_llm(
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

    simulation.step().unwrap();
    let recorder = simulation
        .recorder()
        .as_any()
        .unwrap()
        .downcast_ref::<CaptureRecorder>()
        .unwrap();
    let attempts: Vec<_> = recorder
        .events
        .iter()
        .map(|event| event["attempt"].clone())
        .collect();
    assert_eq!(attempts, vec![0, 0, 1, 1]);
    let resolutions: Vec<_> = recorder
        .events
        .iter()
        .map(|event| event["resolution"].as_str().unwrap())
        .collect();
    assert_eq!(resolutions, vec!["retry", "retry", "giveup", "giveup"]);
    assert_eq!(recorder.rows[0]["llm.retry_rate"], 1.0);
    assert_eq!(recorder.rows[0]["llm.giveup_rate"], 1.0);

    let state = simulation
        .world()
        .policies()
        .state(socsim_core::AgentId(0))
        .unwrap();
    let conversation = <_ as Slot<LlmConversation>>::slot(state).unwrap();
    assert_eq!(conversation.turns.len(), 1);
    assert_eq!(
        conversation.turns.front(),
        Some(&ConversationTurn {
            t: 1,
            action: "0".to_owned(),
            attempts: 2,
            gave_up: true,
            payoff: Some(1.0),
        })
    );
}

fn random_opponent_trajectory(fallback: Fallback<usize>) -> Vec<usize> {
    let seed = 101;
    let mut rules: RuleBook<MatrixGame, X> = RuleBook::new();
    rules.register("random", Arc::new(UniformRandom)).unwrap();
    let mut llm_rules: LlmRuleBook<MatrixGame, X> = LlmRuleBook::new();
    llm_rules
        .register("llm", Arc::new(policy(fallback)))
        .unwrap();
    let catalog = KindCatalog::new()
        .with_rules(&rules)
        .unwrap()
        .with_llm(&llm_rules)
        .unwrap();
    let store = ResponseStore::in_memory("rng", ReplayMode::Live);
    let backend_identity = identity();
    let world = build_world_with(
        game(),
        2,
        WithLlm {
            inner: (),
            llm: LlmReplayManifest::for_store(&store, backend_identity.clone()),
        },
        InitialActions::Same(0),
        &catalog,
        &Assignment::Ordered(vec![
            AssignGroup {
                kind: AssignKind::Rule("llm".to_owned()),
                count: 1,
            },
            AssignGroup {
                kind: AssignKind::Rule("random".to_owned()),
                count: 1,
            },
        ]),
        2,
        16,
        seed,
    )
    .unwrap();
    let setup = LlmSetup::new(llm_rules, store, invalid_backend(), backend_identity);
    let mut simulation: Simulation<World> = assemble_with_llm(
        world,
        FixedPlayers::pairs(&[(0, 1)]),
        rules,
        setup,
        NormalFormOutcome,
        PlayOptions::new(),
        seed,
    )
    .unwrap()
    .build();
    (0..16)
        .map(|_| {
            simulation.step().unwrap();
            simulation.world().held()[1]
        })
        .collect()
}

#[test]
fn uniform_fallback_uses_a_derived_rng_without_consuming_the_engine_stream() {
    assert_eq!(
        random_opponent_trajectory(Fallback::UniformRandom),
        random_opponent_trajectory(Fallback::Specified(0))
    );
}
