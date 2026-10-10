use std::collections::BTreeMap;
use std::sync::Arc;

use socsim_games::llm::{
    assemble_with_llm, BackendFactory, BackendIdentity, CountsObs, CountsRenderer, LabelParser,
    LlmPolicy, LlmReplayManifest, LlmRuleBook, LlmSetup, ReplayMode, ResponseStore, WithLlm,
};
use socsim_games::{
    build_world_with, AssignGroup, AssignKind, Assignment, Fixed, FixedPlayers, GameError,
    InitialActions, KindCatalog, MatrixGame, NormalFormOutcome, OnInvalidAction, PlayOptions,
    RuleBook, RuleError,
};
use socsim_llm::mock::ScriptedClient;

type X = WithLlm<()>;

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
            r#"{"action":"a1","reason":"partition"}"#,
        )))
    })
}

fn llm_policy() -> LlmPolicy<MatrixGame, CountsObs, CountsRenderer, LabelParser> {
    LlmPolicy::new(CountsObs, CountsRenderer::default(), LabelParser::strict())
}

#[test]
fn a_kind_cannot_be_owned_by_both_rule_drivers() {
    let mut rules: RuleBook<MatrixGame, X> = RuleBook::new();
    rules.register("same", Arc::new(Fixed::new(0))).unwrap();
    let mut llm_rules: LlmRuleBook<MatrixGame, X> = LlmRuleBook::new();
    llm_rules.register("same", Arc::new(llm_policy())).unwrap();

    assert!(matches!(
        KindCatalog::new().with_rules(&rules).unwrap().with_llm(&llm_rules),
        Err(RuleError::DuplicateKind(kind)) if kind == "same"
    ));
}

#[test]
fn assemble_rejects_a_world_kind_with_no_driver_owner() {
    let rules: RuleBook<MatrixGame, X> = RuleBook::new();
    let mut world_llm: LlmRuleBook<MatrixGame, X> = LlmRuleBook::new();
    world_llm.register("llm", Arc::new(llm_policy())).unwrap();
    let catalog = KindCatalog::new().with_llm(&world_llm).unwrap();
    let store = ResponseStore::in_memory("missing-owner", ReplayMode::Live);
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
                kind: AssignKind::Held,
                count: 1,
            },
        ]),
        1,
        1,
        7,
    )
    .unwrap();
    let empty_llm = LlmRuleBook::new();
    let setup = LlmSetup::new(empty_llm, store, backend(), backend_identity);
    let error = assemble_with_llm(
        world,
        FixedPlayers::pairs(&[(0, 1)]),
        rules,
        setup,
        NormalFormOutcome,
        PlayOptions::new(),
        7,
    )
    .err()
    .expect("missing owner must fail during assembly");
    assert!(matches!(error, GameError::InvalidKinds { .. }));
}

#[test]
fn each_driver_fills_only_its_assigned_seats() {
    let mut rules: RuleBook<MatrixGame, X> = RuleBook::new();
    rules.register("fixed", Arc::new(Fixed::new(0))).unwrap();
    let mut llm_rules: LlmRuleBook<MatrixGame, X> = LlmRuleBook::new();
    llm_rules.register("llm", Arc::new(llm_policy())).unwrap();
    let catalog = KindCatalog::new()
        .with_rules(&rules)
        .unwrap()
        .with_llm(&llm_rules)
        .unwrap();
    let store = ResponseStore::in_memory("partition", ReplayMode::Live);
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
                kind: AssignKind::Rule("fixed".to_owned()),
                count: 1,
            },
            AssignGroup {
                kind: AssignKind::Rule("llm".to_owned()),
                count: 1,
            },
        ]),
        1,
        1,
        11,
    )
    .unwrap();
    let setup = LlmSetup::new(llm_rules, store, backend(), backend_identity);
    let mut simulation = assemble_with_llm(
        world,
        FixedPlayers::pairs(&[(0, 1)]),
        rules,
        setup,
        NormalFormOutcome,
        PlayOptions::new().on_invalid_action(OnInvalidAction::Fallback(BTreeMap::from([(
            "fixed".to_owned(),
            0,
        )]))),
        11,
    )
    .unwrap()
    .build();

    simulation.step().unwrap();
    assert_eq!(
        simulation
            .world()
            .plays()
            .actions(socsim_games::EncounterId(0))
            .unwrap(),
        &[Some(0), Some(1)]
    );
    assert_eq!(simulation.world().held(), &[0, 1]);
}
