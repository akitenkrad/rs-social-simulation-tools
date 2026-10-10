use std::sync::Arc;

use socsim_core::{AgentId, SimRng};
use socsim_games::{
    assemble, build_world, ArenaWriter, AssignGroup, AssignKind, Assignment, Automaton,
    AutomatonKind, Credit, DecisionScope, FictitiousPlay, Fixed, GameError, InitialActions,
    MatchContext, Matching, MatchingState, MatrixGame, Mixed, NormalFormOutcome, PlayOptions,
    RothErev, RoundRobin, RuleBook, RuleError,
};

fn game() -> MatrixGame {
    MatrixGame::zero_sum(&[vec![1.0, -1.0], vec![-1.0, 1.0]]).unwrap()
}

fn rules() -> RuleBook<MatrixGame, ()> {
    let mut rules = RuleBook::new();
    rules.register("fixed", Arc::new(Fixed::new(0))).unwrap();
    rules
}

fn world(n_agents: usize, rules: &RuleBook<MatrixGame, ()>) -> socsim_games::GameWorld<MatrixGame> {
    build_world(
        game(),
        n_agents,
        (),
        InitialActions::Same(0),
        rules,
        &Assignment::Ordered(vec![AssignGroup {
            kind: AssignKind::Rule("fixed".to_owned()),
            count: n_agents,
        }]),
        2,
        1,
        7,
    )
    .unwrap()
}

fn assemble_round_robin(
    n_agents: usize,
    both_orders: bool,
    scope: DecisionScope,
) -> Result<socsim_engine::SimulationBuilder<socsim_games::GameWorld<MatrixGame>>, GameError> {
    let rules = rules();
    assemble(
        world(n_agents, &rules),
        RoundRobin::new().both_orders(both_orders),
        rules,
        NormalFormOutcome,
        PlayOptions::new().decision_scope(scope),
        7,
    )
}

#[test]
fn per_step_rejects_round_robin_agents_that_hold_different_roles() {
    assert!(matches!(
        assemble_round_robin(3, false, DecisionScope::PerStep),
        Err(GameError::InvalidMatching(_))
    ));
    assert!(matches!(
        assemble_round_robin(2, true, DecisionScope::PerStep),
        Err(GameError::InvalidMatching(_))
    ));
}

#[test]
fn finer_scopes_accept_round_robin_agents_that_hold_different_roles() {
    for scope in [DecisionScope::PerAgentRole, DecisionScope::PerEncounter] {
        assert!(assemble_round_robin(3, false, scope).is_ok());
        assert!(assemble_round_robin(2, true, scope).is_ok());
    }
}

#[derive(Clone, Copy, Debug)]
struct LateAmbiguity;

impl Matching<MatrixGame, ()> for LateAmbiguity {
    fn name(&self) -> &str {
        "late_ambiguity"
    }

    fn check(&self, _ctx: &MatchContext<'_, MatrixGame, ()>) -> Result<(), GameError> {
        Ok(())
    }

    fn encounters_into(
        &self,
        _ctx: &MatchContext<'_, MatrixGame, ()>,
        _state: &mut MatchingState,
        _rng: &mut SimRng,
        out: &mut ArenaWriter<'_, usize>,
    ) -> socsim_core::Result<()> {
        out.push(&[AgentId(0), AgentId(1)], Credit::All)?;
        out.push(&[AgentId(2), AgentId(0)], Credit::All)?;
        Ok(())
    }
}

#[test]
fn per_step_reports_ambiguous_role_when_matching_cannot_guard_at_build_time() {
    let rules = rules();
    let mut simulation = assemble(
        world(3, &rules),
        LateAmbiguity,
        rules,
        NormalFormOutcome,
        PlayOptions::new(),
        7,
    )
    .unwrap()
    .build();

    let error = simulation.step().unwrap_err();
    assert_eq!(
        error.to_string(),
        "mechanism error: agent 0 holds more than one role in a per-step decision"
    );
}

#[test]
fn per_step_rejects_ambiguous_roles_for_held_agents_too() {
    let rules = RuleBook::new();
    let world = build_world(
        game(),
        3,
        (),
        InitialActions::Same(0),
        &rules,
        &Assignment::Ordered(vec![AssignGroup {
            kind: AssignKind::Held,
            count: 3,
        }]),
        1,
        1,
        7,
    )
    .unwrap();
    let mut simulation = assemble(
        world,
        LateAmbiguity,
        rules,
        NormalFormOutcome,
        PlayOptions::new(),
        7,
    )
    .unwrap()
    .build();

    let error = simulation.step().unwrap_err();
    assert_eq!(
        error.to_string(),
        "mechanism error: agent 0 holds more than one role in a per-step decision"
    );
}

#[test]
fn round_robin_rejects_self_play() {
    let rules = rules();
    let result = assemble(
        world(2, &rules),
        RoundRobin::new().self_play(true),
        rules,
        NormalFormOutcome,
        PlayOptions::new().decision_scope(DecisionScope::PerEncounter),
        7,
    );
    assert!(matches!(result, Err(GameError::InvalidMatching(_))));
}

fn one_rule_assignment(kind: &str) -> Assignment {
    Assignment::Ordered(vec![AssignGroup {
        kind: AssignKind::Rule(kind.to_owned()),
        count: 1,
    }])
}

fn assert_unsupported<T>(result: Result<T, GameError>) {
    assert!(matches!(
        result,
        Err(GameError::Rule(RuleError::Unsupported(_)))
    ));
}

#[test]
fn fictitious_play_rejects_a_three_role_game_while_building_the_world() {
    let game = MatrixGame::new(vec![2, 2, 2], vec![vec![1.0; 8]; 3]).unwrap();
    let mut rules = RuleBook::new();
    rules
        .register("fictitious", Arc::new(FictitiousPlay::new()))
        .unwrap();

    assert_unsupported(build_world(
        game,
        1,
        (),
        InitialActions::Same(0),
        &rules,
        &one_rule_assignment("fictitious"),
        1,
        1,
        7,
    ));
}

#[test]
fn roth_erev_rejects_non_positive_game_payoffs_while_building_the_world() {
    let game = MatrixGame::bimatrix(
        &[vec![3.0, 0.0], vec![5.0, 1.0]],
        &[vec![3.0, 5.0], vec![-1.0, 1.0]],
    )
    .unwrap();
    let mut rules = RuleBook::new();
    rules
        .register("roth_erev", Arc::new(RothErev::new(0.1, 0.2, 1.0)))
        .unwrap();

    assert_unsupported(build_world(
        game,
        1,
        (),
        InitialActions::Same(0),
        &rules,
        &one_rule_assignment("roth_erev"),
        1,
        1,
        7,
    ));
}

#[test]
fn automaton_rejects_a_three_action_game_while_building_the_world() {
    let payoffs = vec![vec![1.0; 3]; 3];
    let game = MatrixGame::bimatrix(&payoffs, &payoffs).unwrap();
    let mut rules = RuleBook::new();
    rules
        .register("automaton", Arc::new(Automaton::new(AutomatonKind::Tft, 0)))
        .unwrap();

    assert_unsupported(build_world(
        game,
        1,
        (),
        InitialActions::Same(0),
        &rules,
        &one_rule_assignment("automaton"),
        1,
        1,
        7,
    ));
}

#[test]
fn mixed_rejects_probabilities_that_do_not_sum_to_one() {
    let mut rules = RuleBook::new();
    rules
        .register("mixed", Arc::new(Mixed::new(vec![0.25, 0.25])))
        .unwrap();

    assert_unsupported(build_world(
        game(),
        1,
        (),
        InitialActions::Same(0),
        &rules,
        &one_rule_assignment("mixed"),
        1,
        1,
        7,
    ));
}

#[test]
fn mixed_rejects_a_probability_length_that_differs_from_any_role() {
    let mut rules = RuleBook::new();
    rules
        .register("mixed", Arc::new(Mixed::new(vec![1.0])))
        .unwrap();

    assert_unsupported(build_world(
        game(),
        1,
        (),
        InitialActions::Same(0),
        &rules,
        &one_rule_assignment("mixed"),
        1,
        1,
        7,
    ));
}
