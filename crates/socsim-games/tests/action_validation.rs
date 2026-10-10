use std::any::Any;
use std::collections::BTreeMap;
use std::sync::Arc;

use socsim_core::{Recorder, SimRng, SocsimError};
use socsim_games::{
    assemble, build_world, AssignGroup, AssignKind, Assignment, DecisionContext, DecisionRule,
    FixedPlayers, InitialActions, MatrixGame, NormalFormOutcome, OnInvalidAction, PlayOptions,
    RuleBook, RuleError, RuleInitContext, Slot, Stateless,
};

#[derive(Debug)]
struct Event {
    kind: String,
    payload: serde_json::Value,
}

#[derive(Default)]
struct EventRecorder {
    events: Vec<Event>,
}

impl Recorder for EventRecorder {
    fn record_metric(&mut self, _t: u64, _key: &str, _value: f64) {}

    fn record_event(&mut self, _t: u64, kind: &str, payload: serde_json::Value) {
        self.events.push(Event {
            kind: kind.to_owned(),
            payload,
        });
    }

    fn as_any(&self) -> Option<&dyn Any> {
        Some(self)
    }
}

struct InvalidRule;

impl DecisionRule<MatrixGame, ()> for InvalidRule {
    fn name(&self) -> &str {
        "invalid"
    }

    fn init_state(
        &self,
        _ctx: &RuleInitContext,
        _game: &MatrixGame,
    ) -> Result<<() as socsim_games::ExperimentState>::PolicyState, RuleError> {
        Ok(<_ as Slot<Stateless>>::from_slot(Stateless))
    }

    fn decide(
        &self,
        _state: &mut <() as socsim_games::ExperimentState>::PolicyState,
        _ctx: &DecisionContext<'_, MatrixGame, ()>,
        _rng: &mut SimRng,
    ) -> Result<usize, RuleError> {
        Ok(2)
    }
}

fn prisoners_dilemma() -> MatrixGame {
    MatrixGame::bimatrix(
        &[vec![3.0, 0.0], vec![5.0, 1.0]],
        &[vec![3.0, 5.0], vec![0.0, 1.0]],
    )
    .unwrap()
}

fn rules() -> RuleBook<MatrixGame, ()> {
    let mut rules = RuleBook::new();
    rules.register("bad", Arc::new(InvalidRule)).unwrap();
    rules
}

fn assignment() -> Assignment {
    Assignment::Ordered(vec![
        AssignGroup {
            kind: AssignKind::Rule("bad".into()),
            count: 1,
        },
        AssignGroup {
            kind: AssignKind::Held,
            count: 1,
        },
    ])
}

#[test]
fn invalid_rule_action_stops_the_step_by_default() {
    let rules = rules();
    let world = build_world(
        prisoners_dilemma(),
        2,
        (),
        InitialActions::PerAgent(vec![0, 1]),
        &rules,
        &assignment(),
        1,
        1,
        7,
    )
    .unwrap();
    let mut simulation = assemble(
        world,
        FixedPlayers::pairs(&[(0, 1)]),
        rules,
        NormalFormOutcome,
        PlayOptions::new(),
        7,
    )
    .unwrap()
    .build();

    let error = simulation.step().unwrap_err();
    match error {
        SocsimError::Mechanism(message) => {
            assert!(message.contains("rule returned an invalid action"));
            assert!(message.contains("action 2 is invalid for role 0"));
        }
        other => panic!("expected mechanism error, got {other:?}"),
    }
}

#[test]
fn fallback_replaces_invalid_action_and_records_structured_event() {
    let rules = rules();
    let world = build_world(
        prisoners_dilemma(),
        2,
        (),
        InitialActions::PerAgent(vec![0, 1]),
        &rules,
        &assignment(),
        1,
        1,
        7,
    )
    .unwrap();
    let fallback = BTreeMap::from([("bad".to_owned(), 0)]);
    let mut simulation = assemble(
        world,
        FixedPlayers::pairs(&[(0, 1)]),
        rules,
        NormalFormOutcome,
        PlayOptions::new().on_invalid_action(OnInvalidAction::Fallback(fallback)),
        7,
    )
    .unwrap()
    .recorder(Box::new(EventRecorder::default()))
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
    assert_eq!(simulation.world().payoffs(), &[0.0, 5.0]);

    let recorder = simulation
        .recorder()
        .as_any()
        .unwrap()
        .downcast_ref::<EventRecorder>()
        .unwrap();
    assert_eq!(recorder.events.len(), 1);
    let event = &recorder.events[0];
    assert_eq!(event.kind, "game.invalid_action");
    assert_eq!(event.payload["agent"], serde_json::json!(0));
    assert_eq!(event.payload["kind"], serde_json::json!("bad"));
    assert_eq!(event.payload["role"], serde_json::json!(0));
    assert_eq!(event.payload["action"], serde_json::json!("2"));
}
