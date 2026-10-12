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

// Continuous games.

use socsim_games::continuous::Cournot;
use socsim_games::{
    check_continuous_action, ActionError, ContinuousGame, Fixed, NormalFormGame, StageGame,
};

/// A two-dimensional continuous game, to observe the order of the checks.
struct Box2;

impl StageGame for Box2 {
    type Action = Vec<f64>;

    fn n_roles(&self) -> usize {
        2
    }

    fn check_action(&self, role: usize, action: &Vec<f64>) -> Result<(), ActionError> {
        check_continuous_action(self, role, action)
    }
}

impl NormalFormGame for Box2 {
    fn payoff(&self, role: usize, profile: &[Vec<f64>]) -> f64 {
        profile[role].iter().sum()
    }
}

impl ContinuousGame for Box2 {
    fn bounds(&self, _role: usize) -> &[(f64, f64)] {
        &[(0.0, 1.0), (-1.0, 1.0)]
    }
}

#[test]
fn continuous_action_checks_return_the_first_violation_in_a_fixed_order() {
    let game = Cournot::new(2, 100.0, 1.0, 10.0, 100.0).unwrap();
    assert_eq!(game.check_action(0, &vec![0.0]), Ok(()));
    assert_eq!(game.check_action(1, &vec![100.0]), Ok(()));
    assert_eq!(
        game.check_action(2, &vec![f64::NAN]),
        Err(ActionError::InvalidRole { role: 2 })
    );
    assert_eq!(
        game.check_action(0, &vec![]),
        Err(ActionError::DimensionMismatch {
            role: 0,
            expected: 1,
            actual: 0
        })
    );
    assert_eq!(
        game.check_action(1, &vec![f64::NAN, 1.0]),
        Err(ActionError::DimensionMismatch {
            role: 1,
            expected: 1,
            actual: 2
        })
    );
    assert_eq!(
        game.check_action(0, &vec![f64::INFINITY]),
        Err(ActionError::NonFiniteComponent { role: 0, dim: 0 })
    );
    assert_eq!(
        game.check_action(0, &vec![100.5]),
        Err(ActionError::OutOfBounds { role: 0, dim: 0 })
    );
    assert_eq!(
        game.check_action(1, &vec![-1e-12]),
        Err(ActionError::OutOfBounds { role: 1, dim: 0 })
    );

    // Finiteness of every component is checked before any range.
    assert_eq!(
        check_continuous_action(&Box2, 0, &[2.0, f64::NAN]),
        Err(ActionError::NonFiniteComponent { role: 0, dim: 1 })
    );
    assert_eq!(
        check_continuous_action(&Box2, 1, &[0.5, -1.5]),
        Err(ActionError::OutOfBounds { role: 1, dim: 1 })
    );
    assert_eq!(check_continuous_action(&Box2, 1, &[1.0, -1.0]), Ok(()));
}

/// Returns a fixed continuous action without checking it at build time, so the
/// per-step check of the decision mechanism is exercised.
struct InvalidContinuousRule(Vec<f64>);

impl DecisionRule<Cournot, ()> for InvalidContinuousRule {
    fn name(&self) -> &str {
        "invalid_continuous"
    }

    fn init_state(
        &self,
        _ctx: &RuleInitContext,
        _game: &Cournot,
    ) -> Result<<() as socsim_games::ExperimentState>::PolicyState, RuleError> {
        Ok(<_ as Slot<Stateless>>::from_slot(Stateless))
    }

    fn decide(
        &self,
        _state: &mut <() as socsim_games::ExperimentState>::PolicyState,
        _ctx: &DecisionContext<'_, Cournot, ()>,
        _rng: &mut SimRng,
    ) -> Result<Vec<f64>, RuleError> {
        Ok(self.0.clone())
    }
}

fn cournot_game() -> Cournot {
    Cournot::new(2, 100.0, 1.0, 10.0, 100.0).unwrap()
}

fn invalid_continuous_cases() -> [(Vec<f64>, &'static str); 3] {
    [
        (
            vec![150.0],
            "component 0 of the action for role 0 is outside its bounds",
        ),
        (
            vec![f64::NAN],
            "component 0 of the action for role 0 is not finite",
        ),
        (
            vec![1.0, 2.0],
            "action for role 0 has 2 components, expected 1",
        ),
    ]
}

#[test]
fn fixed_rule_with_an_invalid_continuous_action_is_rejected_while_building_the_world() {
    for (action, expected) in invalid_continuous_cases() {
        let mut rules: RuleBook<Cournot, ()> = RuleBook::new();
        rules.register("bad", Arc::new(Fixed::new(action))).unwrap();
        let result = build_world(
            cournot_game(),
            2,
            (),
            InitialActions::PerAgent(vec![vec![10.0], vec![20.0]]),
            &rules,
            &assignment(),
            1,
            1,
            7,
        );
        match result {
            Err(socsim_games::GameError::Rule(RuleError::InvalidAction(message))) => {
                assert_eq!(message, expected);
            }
            other => panic!("expected an invalid action, got {other:?}"),
        }
    }
}

#[test]
fn invalid_initial_continuous_actions_are_rejected_while_building_the_world() {
    let mut rules: RuleBook<Cournot, ()> = RuleBook::new();
    rules
        .register("bad", Arc::new(Fixed::new(vec![10.0])))
        .unwrap();
    let result = build_world(
        cournot_game(),
        2,
        (),
        InitialActions::PerAgent(vec![vec![10.0], vec![f64::INFINITY]]),
        &rules,
        &assignment(),
        1,
        1,
        7,
    );
    assert!(matches!(
        result,
        Err(socsim_games::GameError::Action(
            ActionError::NonFiniteComponent { role: 0, dim: 0 }
        ))
    ));
}

fn cournot_simulation(
    action: Vec<f64>,
    options: PlayOptions<Vec<f64>>,
) -> socsim_engine::Simulation<socsim_games::GameWorld<Cournot>> {
    let mut rules = RuleBook::new();
    rules
        .register("bad", Arc::new(InvalidContinuousRule(action)))
        .unwrap();
    let world = build_world(
        cournot_game(),
        2,
        (),
        InitialActions::PerAgent(vec![vec![10.0], vec![20.0]]),
        &rules,
        &assignment(),
        1,
        1,
        7,
    )
    .unwrap();
    assemble(
        world,
        FixedPlayers::pairs(&[(0, 1)]),
        rules,
        NormalFormOutcome,
        options,
        7,
    )
    .unwrap()
    .recorder(Box::new(EventRecorder::default()))
    .build()
}

#[test]
fn a_rule_returning_an_invalid_continuous_action_stops_the_step() {
    for (action, expected) in invalid_continuous_cases() {
        let mut simulation = cournot_simulation(action, PlayOptions::new());
        match simulation.step().unwrap_err() {
            SocsimError::Mechanism(message) => {
                assert!(message.contains("rule returned an invalid action"));
                assert!(message.contains(expected), "{message}");
            }
            other => panic!("expected mechanism error, got {other:?}"),
        }
    }
}

#[test]
fn fallback_replaces_an_invalid_continuous_action() {
    let fallback = BTreeMap::from([("bad".to_owned(), vec![30.0])]);
    let mut simulation = cournot_simulation(
        vec![-5.0],
        PlayOptions::new().on_invalid_action(OnInvalidAction::Fallback(fallback)),
    );
    simulation.step().unwrap();
    assert_eq!(
        simulation
            .world()
            .plays()
            .actions(socsim_games::EncounterId(0))
            .unwrap(),
        &[Some(vec![30.0]), Some(vec![20.0])]
    );
    // P = 100 - 50 = 50; profits 30 * 40 and 20 * 40.
    assert_eq!(simulation.world().payoffs(), &[1200.0, 800.0]);

    let recorder = simulation
        .recorder()
        .as_any()
        .unwrap()
        .downcast_ref::<EventRecorder>()
        .unwrap();
    assert_eq!(recorder.events.len(), 1);
    assert_eq!(recorder.events[0].kind, "game.invalid_action");
    assert_eq!(recorder.events[0].payload["role"], serde_json::json!(0));
}
