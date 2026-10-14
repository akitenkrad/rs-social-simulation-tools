use std::any::Any;
use std::collections::BTreeMap;
use std::sync::Arc;

use socsim_core::Recorder;
use socsim_games::{
    assemble, build_world, check_continuous_action, ActionError, AssignGroup, AssignKind,
    Assignment, ContinuousGame, ContinuousMetrics, ContinuousMetricsOptions, FixedPlayers,
    GameError, GradientAscent, InitialActions, NormalFormGame, NormalFormOutcome, OnInvalidAction,
    PlayOptions, RuleBook, StageGame,
};

const ONE_DIMENSION: [(f64, f64); 1] = [(0.0, 1.0)];
const TWO_DIMENSIONS: [(f64, f64); 2] = [(0.0, 1.0), (0.0, 1.0)];

#[derive(Clone, Copy, Debug)]
struct MixedDimensions;

impl StageGame for MixedDimensions {
    type Action = Vec<f64>;

    fn n_roles(&self) -> usize {
        2
    }

    fn check_action(&self, role: usize, action: &Vec<f64>) -> Result<(), ActionError> {
        check_continuous_action(self, role, action)
    }
}

impl NormalFormGame for MixedDimensions {
    fn payoff(&self, role: usize, profile: &[Vec<f64>]) -> f64 {
        profile[role].iter().sum()
    }
}

impl ContinuousGame for MixedDimensions {
    fn bounds(&self, role: usize) -> &[(f64, f64)] {
        if role == 0 {
            &ONE_DIMENSION
        } else {
            &TWO_DIMENSIONS
        }
    }
}

fn held_assignment(n_agents: usize) -> Assignment {
    Assignment::Ordered(vec![AssignGroup {
        kind: AssignKind::Held,
        count: n_agents,
    }])
}

fn rule_assignment(n_agents: usize) -> Assignment {
    Assignment::Ordered(vec![AssignGroup {
        kind: AssignKind::Rule("gradient".to_owned()),
        count: n_agents,
    }])
}

fn gradient_rules<G: ContinuousGame>() -> RuleBook<G, ()> {
    let mut rules = RuleBook::new();
    rules
        .register("gradient", Arc::new(GradientAscent::new(0.1).unwrap()))
        .unwrap();
    rules
}

fn assert_build_world_rejects(initial: InitialActions<Vec<f64>>) {
    let held_rules: RuleBook<MixedDimensions, ()> = RuleBook::new();
    let held_result = build_world(
        MixedDimensions,
        2,
        (),
        initial.clone(),
        &held_rules,
        &held_assignment(2),
        1,
        1,
        7,
    );
    assert!(matches!(
        held_result,
        Err(GameError::Action(ActionError::DimensionMismatch { .. }))
    ));

    let rules = gradient_rules();
    let rule_result = build_world(
        MixedDimensions,
        2,
        (),
        initial,
        &rules,
        &rule_assignment(2),
        1,
        1,
        7,
    );
    assert!(matches!(
        rule_result,
        Err(GameError::Action(ActionError::DimensionMismatch { .. }))
    ));
}

#[test]
fn build_world_rejects_one_dimensional_same_action_for_held_and_rule_agents() {
    assert_build_world_rejects(InitialActions::Same(vec![0.5]));
}

#[test]
fn build_world_rejects_two_dimensional_same_action_for_held_and_rule_agents() {
    assert_build_world_rejects(InitialActions::Same(vec![0.5, 0.5]));
}

#[test]
fn build_world_rejects_role_shaped_per_agent_actions_for_held_and_rule_agents() {
    assert_build_world_rejects(InitialActions::PerAgent(vec![vec![0.5], vec![0.5, 0.5]]));
}

#[test]
fn assemble_rejects_fallback_action_for_mixed_role_dimensions() {
    let rules = gradient_rules();
    let world = build_world(
        MixedDimensions,
        0,
        (),
        InitialActions::PerAgent(Vec::new()),
        &rules,
        &Assignment::Ordered(Vec::new()),
        1,
        1,
        7,
    )
    .unwrap();
    let fallbacks = BTreeMap::from([("gradient".to_owned(), vec![0.5])]);
    let result = assemble(
        world,
        FixedPlayers::pairs(&[]),
        rules,
        NormalFormOutcome,
        PlayOptions::new().on_invalid_action(OnInvalidAction::Fallback(fallbacks)),
        7,
    );
    assert!(matches!(
        result,
        Err(GameError::Action(ActionError::DimensionMismatch { .. }))
    ));
}

const LOWER_BOUNDS: [(f64, f64); 1] = [(0.0, 2.0)];
const UPPER_BOUNDS: [(f64, f64); 1] = [(1.0, 3.0)];

#[derive(Clone, Copy, Debug)]
struct SameDimensionsDifferentBounds;

impl StageGame for SameDimensionsDifferentBounds {
    type Action = Vec<f64>;

    fn n_roles(&self) -> usize {
        2
    }

    fn check_action(&self, role: usize, action: &Vec<f64>) -> Result<(), ActionError> {
        check_continuous_action(self, role, action)
    }
}

impl NormalFormGame for SameDimensionsDifferentBounds {
    fn payoff(&self, role: usize, profile: &[Vec<f64>]) -> f64 {
        profile[role][0]
    }
}

impl ContinuousGame for SameDimensionsDifferentBounds {
    fn bounds(&self, role: usize) -> &[(f64, f64)] {
        if role == 0 {
            &LOWER_BOUNDS
        } else {
            &UPPER_BOUNDS
        }
    }
}

#[derive(Default)]
struct ColumnRecorder {
    rows: Vec<(String, f64)>,
}

impl Recorder for ColumnRecorder {
    fn record_metric(&mut self, _t: u64, key: &str, value: f64) {
        self.rows.push((key.to_owned(), value));
    }

    fn record_event(&mut self, _t: u64, _kind: &str, _payload: serde_json::Value) {}

    fn record_row(&mut self, _t: u64, table: &str, row: &[(&str, f64)]) {
        self.rows.extend(
            row.iter()
                .map(|(column, value)| (format!("{table}.{column}"), *value)),
        );
    }

    fn as_any(&self) -> Option<&dyn Any> {
        Some(self)
    }
}

#[test]
fn equal_dimensions_with_role_specific_bounds_record_pooled_action_columns() {
    let game = SameDimensionsDifferentBounds;
    let metrics = ContinuousMetrics::new(&game, ContinuousMetricsOptions::new()).unwrap();
    let rules = RuleBook::new();
    let world = build_world(
        game,
        2,
        (),
        InitialActions::Same(vec![1.5]),
        &rules,
        &held_assignment(2),
        1,
        1,
        11,
    )
    .unwrap();
    let mut simulation = assemble(
        world,
        FixedPlayers::pairs(&[(0, 1)]),
        rules,
        NormalFormOutcome,
        PlayOptions::new(),
        11,
    )
    .unwrap()
    .add_mechanism(Box::new(metrics))
    .recorder(Box::new(ColumnRecorder::default()))
    .build();

    simulation.run().unwrap();

    let rows = &simulation
        .recorder()
        .as_any()
        .unwrap()
        .downcast_ref::<ColumnRecorder>()
        .unwrap()
        .rows;
    assert!(rows
        .iter()
        .any(|(column, value)| column == "game.mean_action.0" && *value == 1.5));
    assert!(rows
        .iter()
        .any(|(column, value)| column == "game.action_sd.0" && *value == 0.0));
}
