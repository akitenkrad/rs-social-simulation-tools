//! An LLM-backed firm in a three-firm Cournot oligopoly.
//!
//! Firm 0 asks a scripted model for its quantity; firms 1 and 2 play a damped
//! best response. With the LLM firm fixed at `L`, the other two firms converge
//! to `(a - c - b L) / (3 b)`.

use std::any::Any;
use std::sync::Arc;

use socsim_core::{AgentId, Recorder, SimRng};
use socsim_engine::Simulation;
use socsim_games::continuous::Cournot;
use socsim_games::llm::{
    assemble_with_llm, ActionParser, BackendFactory, BackendIdentity, ConversationTurn,
    DescribeGame, DescribedRenderer, GameRenderer, LlmConversation, LlmPolicy, LlmReplayManifest,
    LlmRuleBook, LlmSetup, NumberParser, OwnHistory, OwnHistoryObs, ParseOutcome, ReplayMode,
    ResponseStore, ViolationKind, WithLlm,
};
use socsim_games::{
    build_world_with, AssignGroup, AssignKind, Assignment, DampedBestResponse, DecisionRule,
    FixedPlayers, GameError, GameWorld, InitialActions, KindCatalog, NormalFormOutcome,
    PlayOptions, RuleBook, Slot, StepRecord, WholePopulation,
};
use socsim_llm::mock::ScriptedClient;

type X = WithLlm<()>;
type World = GameWorld<Cournot, X>;

const STEPS: u64 = 200;
const SEED: u64 = 2027;

#[derive(Default)]
struct CaptureRecorder {
    events: Vec<(u64, serde_json::Value)>,
    rows: Vec<(u64, String, u64)>,
}

impl Recorder for CaptureRecorder {
    fn record_metric(&mut self, _t: u64, _key: &str, _value: f64) {}

    fn record_event(&mut self, t: u64, kind: &str, payload: serde_json::Value) {
        if kind == "game.llm_call" {
            self.events.push((t, payload));
        }
    }

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

fn game() -> Cournot {
    Cournot::new(3, 100.0, 1.0, 10.0, 100.0).unwrap()
}

fn identity() -> BackendIdentity {
    BackendIdentity {
        model: "scripted".to_owned(),
        endpoint: "mock://scripted".to_owned(),
    }
}

fn backend(reply: &'static str) -> BackendFactory {
    Arc::new(move || Ok(Box::new(ScriptedClient::constant("scripted", reply))))
}

type Policy = LlmPolicy<Cournot, OwnHistoryObs, DescribedRenderer, NumberParser>;

fn policy() -> Policy {
    LlmPolicy::new(
        OwnHistoryObs::new(4),
        DescribedRenderer::new("quantity"),
        NumberParser::new("quantity"),
    )
}

fn catalog_and_books() -> (
    RuleBook<Cournot, X>,
    LlmRuleBook<Cournot, X>,
    KindCatalog<Cournot, X>,
) {
    let mut rules: RuleBook<Cournot, X> = RuleBook::new();
    let damped: Arc<dyn DecisionRule<Cournot, X>> =
        Arc::new(DampedBestResponse::<Cournot>::new(0.5).unwrap());
    rules.register("damped", damped).unwrap();
    let mut llm_rules: LlmRuleBook<Cournot, X> = LlmRuleBook::new();
    llm_rules.register("llm", Arc::new(policy())).unwrap();
    let catalog = KindCatalog::new()
        .with_rules(&rules)
        .unwrap()
        .with_llm(&llm_rules)
        .unwrap();
    (rules, llm_rules, catalog)
}

fn world(store: &ResponseStore, catalog: &KindCatalog<Cournot, X>, t_max: u64) -> World {
    build_world_with(
        game(),
        3,
        WithLlm {
            inner: (),
            llm: LlmReplayManifest::for_store(store, identity()),
        },
        InitialActions::PerAgent(vec![vec![20.0], vec![0.0], vec![0.0]]),
        catalog,
        &Assignment::Ordered(vec![
            AssignGroup {
                kind: AssignKind::Rule("llm".to_owned()),
                count: 1,
            },
            AssignGroup {
                kind: AssignKind::Rule("damped".to_owned()),
                count: 2,
            },
        ]),
        4,
        t_max,
        SEED,
    )
    .unwrap()
}

fn simulation(reply: &'static str, t_max: u64) -> Simulation<World> {
    let (rules, llm_rules, catalog) = catalog_and_books();
    let store = ResponseStore::in_memory("llm-continuous", ReplayMode::Live);
    let world = world(&store, &catalog, t_max);
    let setup = LlmSetup::new(llm_rules, store, backend(reply), identity());
    assemble_with_llm(
        world,
        WholePopulation::new(),
        rules,
        setup,
        NormalFormOutcome,
        PlayOptions::new(),
        SEED,
    )
    .unwrap()
    .recorder(Box::new(CaptureRecorder::default()))
    .build()
}

fn recorder(simulation: &Simulation<World>) -> &CaptureRecorder {
    simulation
        .recorder()
        .as_any()
        .unwrap()
        .downcast_ref::<CaptureRecorder>()
        .unwrap()
}

/// Runs `STEPS` steps and returns every step's world as JSON.
fn run(simulation: &mut Simulation<World>) -> Vec<String> {
    (0..STEPS)
        .map(|_| {
            simulation.step().unwrap();
            serde_json::to_string(simulation.world()).unwrap()
        })
        .collect()
}

#[test]
fn llm_firm_runs_deterministically_with_damped_best_response_firms() {
    let mut first = simulation(r#"{"quantity": 20}"#, STEPS);
    let mut second = simulation(r#"{"quantity": 20}"#, STEPS);
    let first_worlds = run(&mut first);
    let second_worlds = run(&mut second);

    // (a) Every step's world is identical across two runs.
    assert_eq!(first_worlds.len(), STEPS as usize);
    assert_eq!(first_worlds, second_worlds);
    assert_eq!(recorder(&first).events, recorder(&second).events);
    assert_eq!(recorder(&first).rows, recorder(&second).rows);

    // (b) With the LLM firm at 20, the other firms reach (100 - 10 - 20) / 3.
    let held = first.world().held();
    assert_eq!(held[0], vec![20.0]);
    let target = 70.0 / 3.0;
    for firm in [1, 2] {
        assert_eq!(held[firm].len(), 1);
        assert!(
            (held[firm][0] - target).abs() <= 1.0e-9,
            "firm {firm}: {:?}",
            held[firm]
        );
    }

    // (c) One accepted LLM decision is recorded per step, for firm 0.
    let events = &recorder(&first).events;
    assert_eq!(events.len(), STEPS as usize);
    for (index, (t, event)) in events.iter().enumerate() {
        assert_eq!(*t, index as u64 + 1);
        assert_eq!(event["agent"], 0);
        assert_eq!(event["attempt"], 0);
        assert_eq!(event["resolution"], "accept");
    }
}

#[test]
fn described_renderer_builds_the_prompt_from_the_description_and_bounds() {
    let mut simulation = simulation(r#"{"quantity": 20}"#, 2);
    simulation.step().unwrap();
    simulation.step().unwrap();
    let events = &recorder(&simulation).events;
    assert_eq!(events.len(), 2);

    let system = events[0].1["system"].as_str().unwrap();
    assert!(system.contains("You are role 0."), "{system}");
    assert!(system.contains(&game().describe(Some(0))), "{system}");
    assert!(
        system.contains("Your action is a single number between 0 and 100 (inclusive)."),
        "{system}"
    );
    assert!(
        system.ends_with(r#"Reply with JSON: {"quantity": <number>, "reason": "<reason>"}."#),
        "{system}"
    );
    assert_eq!(events[1].1["system"], events[0].1["system"]);

    assert_eq!(
        events[0].1["prompt"],
        "t=1\nmy history (oldest first):\n- none\nopponents' last actions: none, none"
    );
    // Step 1 played (20, 0, 0): price 80, firm 0 earns 20 * (80 - 10).
    assert_eq!(
        events[1].1["prompt"],
        "t=2\nmy history (oldest first):\n- action: 20; payoff: 1400; encounters: 1\n\
         opponents' last actions: 0, 0"
    );
}

#[test]
fn described_renderer_renders_multi_component_actions_as_lists() {
    let observation = OwnHistory {
        t: 3,
        role: Some(0),
        mine: vec![StepRecord {
            action: Some(vec![1.5, 0.0]),
            payoff: -2.0,
            n_played: 1,
        }],
        opponents_last: vec![Some(vec![2.0, 3.0]), None],
    };
    let rendered = <DescribedRenderer as GameRenderer<Cournot, OwnHistory<Vec<f64>>>>::turn(
        &DescribedRenderer::new("quantity"),
        &observation,
        &game(),
    );
    assert_eq!(
        rendered,
        "t=3\nmy history (oldest first):\n- action: [1.5, 0]; payoff: -2; encounters: 1\n\
         opponents' last actions: [2, 3], none"
    );
}

#[test]
fn out_of_range_quantity_is_retried_then_falls_back_to_the_previous_action() {
    let mut simulation = simulation(r#"{"quantity": 150}"#, 2);
    simulation.step().unwrap();

    let events = &recorder(&simulation).events;
    let resolutions: Vec<_> = events
        .iter()
        .map(|(_, event)| event["resolution"].as_str().unwrap())
        .collect();
    assert_eq!(resolutions, vec!["retry", "giveup"]);
    let attempts: Vec<_> = events
        .iter()
        .map(|(_, event)| event["attempt"].clone())
        .collect();
    assert_eq!(attempts, vec![0, 1]);
    // The retry prompt carries the range violation back to the model.
    let retry_prompt = events[1].1["prompt"].as_str().unwrap();
    assert!(
        retry_prompt.starts_with(events[0].1["prompt"].as_str().unwrap()),
        "{retry_prompt}"
    );
    assert!(retry_prompt.contains("violated Range"), "{retry_prompt}");

    let rows = &recorder(&simulation).rows;
    let column = |key: &str| {
        rows.iter()
            .find(|(t, column, _)| *t == 1 && column == key)
            .map(|(_, _, bits)| f64::from_bits(*bits))
            .unwrap()
    };
    assert_eq!(column("game.llm.retry_rate"), 1.0);
    assert_eq!(column("game.llm.giveup_rate"), 1.0);

    // The default fallback keeps the previous action.
    assert_eq!(simulation.world().held()[0], vec![20.0]);
    let state = simulation.world().policies().state(AgentId(0)).unwrap();
    let conversation = <_ as Slot<LlmConversation>>::slot(state).unwrap();
    assert_eq!(
        conversation.turns.back(),
        Some(&ConversationTurn {
            t: 1,
            action: "[20.0]".to_owned(),
            attempts: 2,
            gave_up: true,
            payoff: Some(20.0 * (80.0 - 10.0)),
        })
    );
}

#[test]
fn number_parser_reads_the_game_bounds_unless_it_has_its_own() {
    let game = game();
    let parse = |parser: &NumberParser, raw: &str| {
        <NumberParser as ActionParser<Cournot>>::parse(parser, raw, &game, Some(1))
    };
    let range_violation = |outcome: ParseOutcome<Vec<f64>>| match outcome {
        ParseOutcome::Violation(violation) => violation.kind == ViolationKind::Range,
        ParseOutcome::Ok { .. } => false,
    };

    let parser = NumberParser::new("quantity");
    assert!(matches!(
        parse(&parser, r#"{"quantity": 22.5}"#),
        ParseOutcome::Ok { action, .. } if action == vec![22.5]
    ));
    assert!(range_violation(parse(&parser, r#"{"quantity": 100.5}"#)));
    assert!(range_violation(parse(&parser, r#"{"quantity": -1}"#)));

    let narrow = NumberParser::new("quantity").bounds(10.0, 30.0);
    assert!(<NumberParser as ActionParser<Cournot>>::validate(&narrow, &game).is_ok());
    assert!(range_violation(parse(&narrow, r#"{"quantity": 40}"#)));

    let wide = NumberParser::new("quantity").bounds(-1.0, 30.0);
    assert!(<NumberParser as ActionParser<Cournot>>::validate(&wide, &game).is_err());
    assert!(<NumberParser as ActionParser<Cournot>>::validate(&parser, &game).is_ok());

    let mut rng = SimRng::from_seed(5);
    for _ in 0..100 {
        let sample = <NumberParser as ActionParser<Cournot>>::sample_uniform(
            &parser,
            &game,
            Some(0),
            &mut rng,
        );
        assert_eq!(sample.len(), 1);
        assert!((0.0..=100.0).contains(&sample[0]), "{sample:?}");
        let sample = <NumberParser as ActionParser<Cournot>>::sample_uniform(
            &narrow,
            &game,
            Some(0),
            &mut rng,
        );
        assert!((10.0..=30.0).contains(&sample[0]), "{sample:?}");
    }
}

#[test]
fn fixed_players_still_reject_a_three_role_game_under_llm_assembly() {
    let (rules, llm_rules, catalog) = catalog_and_books();
    let store = ResponseStore::in_memory("llm-continuous", ReplayMode::Live);
    let world = world(&store, &catalog, 1);
    let setup = LlmSetup::new(llm_rules, store, backend(r#"{"quantity": 20}"#), identity());
    let result = assemble_with_llm(
        world,
        FixedPlayers::pairs(&[(0, 1)]),
        rules,
        setup,
        NormalFormOutcome,
        PlayOptions::new(),
        SEED,
    );
    assert!(matches!(
        result,
        Err(GameError::UnsupportedRoles { n_roles: 3 })
    ));
}
