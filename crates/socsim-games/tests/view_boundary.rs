use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use socsim_core::{AgentId, SimRng};
use socsim_games::{
    assemble, build_world, AssignGroup, AssignKind, Assignment, DecisionContext, DecisionRule,
    EncounterId, ExperimentState, Fixed, FixedPlayers, InitialActions, LearnContext, MatrixGame,
    NormalFormOutcome, PlayOptions, RuleBook, RuleError, RuleInitContext,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PrivateView {
    mine: u8,
}

#[derive(Clone, Debug)]
struct PrivateExperiment {
    private_signals: Vec<u8>,
}

#[derive(Clone, Debug, Default)]
struct ProbeState {
    mine: Option<u8>,
    opponent_visible: Option<bool>,
    other_encounter_hidden: Option<bool>,
    resting_hidden: Option<bool>,
    outcomes: Vec<OutcomeSnapshot>,
}

#[derive(Clone, Debug, PartialEq)]
struct OutcomeSnapshot {
    id: EncounterId,
    role: usize,
    seats: Vec<AgentId>,
    actions: Vec<Option<usize>>,
    payoffs: Vec<f64>,
}

impl ExperimentState for PrivateExperiment {
    type PolicyState = ProbeState;
    type View<'a> = PrivateView;

    fn view_for(&self, me: AgentId) -> Self::View<'_> {
        PrivateView {
            mine: self.private_signals[me.0 as usize],
        }
    }
}

struct ProbeRule;

impl DecisionRule<MatrixGame, PrivateExperiment> for ProbeRule {
    fn name(&self) -> &str {
        "probe"
    }

    fn init_state(
        &self,
        _ctx: &RuleInitContext,
        _game: &MatrixGame,
    ) -> Result<ProbeState, RuleError> {
        Ok(ProbeState::default())
    }

    fn decide(
        &self,
        state: &mut ProbeState,
        ctx: &DecisionContext<'_, MatrixGame, PrivateExperiment>,
        _rng: &mut SimRng,
    ) -> Result<usize, RuleError> {
        state.mine = Some(ctx.view().mine);
        let opponent = ctx.opponents()[0];
        state.opponent_visible = Some(ctx.opponent_history(opponent).is_some());
        state.other_encounter_hidden = Some(ctx.opponent_history(AgentId(2)).is_none());
        state.resting_hidden = Some(ctx.opponent_history(AgentId(4)).is_none());
        Ok(0)
    }

    fn learn_step(
        &self,
        state: &mut ProbeState,
        ctx: &LearnContext<'_, MatrixGame, PrivateExperiment>,
    ) -> Result<(), RuleError> {
        state.outcomes = ctx
            .outcomes()
            .map(|outcome| OutcomeSnapshot {
                id: outcome.id,
                role: outcome.role,
                seats: outcome.seats.to_vec(),
                actions: outcome.actions.to_vec(),
                payoffs: outcome.payoffs.to_vec(),
            })
            .collect();
        Ok(())
    }
}

fn prisoners_dilemma() -> MatrixGame {
    MatrixGame::bimatrix(
        &[vec![3.0, 0.0], vec![5.0, 1.0]],
        &[vec![3.0, 5.0], vec![0.0, 1.0]],
    )
    .unwrap()
}

#[test]
fn default_view_reveals_only_the_current_agent_and_current_opponent() {
    let mut rules = RuleBook::new();
    rules.register("probe", Arc::new(ProbeRule)).unwrap();
    let world = build_world(
        prisoners_dilemma(),
        5,
        PrivateExperiment {
            private_signals: vec![10, 11, 12, 13, 14],
        },
        InitialActions::Same(0),
        &rules,
        &Assignment::Ordered(vec![AssignGroup {
            kind: AssignKind::Rule("probe".into()),
            count: 5,
        }]),
        2,
        1,
        23,
    )
    .unwrap();
    let mut simulation = assemble(
        world,
        FixedPlayers::pairs(&[(0, 1), (2, 3)]),
        rules,
        NormalFormOutcome,
        PlayOptions::new(),
        23,
    )
    .unwrap()
    .build();
    simulation.step().unwrap();

    let state = simulation.world().policies().state(AgentId(0)).unwrap();
    assert_eq!(state.mine, Some(10));
    assert_eq!(state.opponent_visible, Some(true));
    assert_eq!(state.other_encounter_hidden, Some(true));
    assert_eq!(state.resting_hidden, Some(true));
}

#[test]
fn learn_outcomes_are_own_encounters_with_option_actions_and_stable_ids() {
    let mut rules = RuleBook::new();
    rules.register("probe", Arc::new(ProbeRule)).unwrap();
    let world = build_world(
        prisoners_dilemma(),
        5,
        PrivateExperiment {
            private_signals: vec![10, 11, 12, 13, 14],
        },
        InitialActions::Same(0),
        &rules,
        &Assignment::Ordered(vec![AssignGroup {
            kind: AssignKind::Rule("probe".into()),
            count: 5,
        }]),
        2,
        1,
        23,
    )
    .unwrap();
    let mut simulation = assemble(
        world,
        FixedPlayers::pairs(&[(2, 3), (0, 1)]),
        rules,
        NormalFormOutcome,
        PlayOptions::new(),
        23,
    )
    .unwrap()
    .build();
    simulation.step().unwrap();

    let zero = &simulation
        .world()
        .policies()
        .state(AgentId(0))
        .unwrap()
        .outcomes;
    assert_eq!(zero.len(), 1);
    assert_eq!(zero[0].id, EncounterId(1));
    assert_eq!(zero[0].role, 0);
    assert_eq!(zero[0].seats, vec![AgentId(0), AgentId(1)]);
    assert_eq!(zero[0].actions, vec![Some(0), Some(0)]);
    assert_eq!(zero[0].payoffs, vec![3.0, 3.0]);

    let resting = &simulation
        .world()
        .policies()
        .state(AgentId(4))
        .unwrap()
        .outcomes;
    assert!(resting.is_empty());
}

#[test]
fn unit_experiment_state_builds_without_user_defined_view_for() {
    let mut rules = RuleBook::new();
    rules.register("fixed", Arc::new(Fixed::new(0))).unwrap();
    let world = build_world(
        prisoners_dilemma(),
        2,
        (),
        InitialActions::Same(0),
        &rules,
        &Assignment::Ordered(vec![AssignGroup {
            kind: AssignKind::Rule("fixed".into()),
            count: 2,
        }]),
        1,
        1,
        5,
    )
    .unwrap();
    let mut simulation = assemble(
        world,
        FixedPlayers::pairs(&[(0, 1)]),
        rules,
        NormalFormOutcome,
        PlayOptions::new(),
        5,
    )
    .unwrap()
    .build();
    simulation.step().unwrap();
    assert_eq!(simulation.world().payoffs(), &[3.0, 3.0]);
}

#[test]
fn builtin_rules_do_not_call_omniscient_context() {
    let rule_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/rule");
    let mut source = String::new();
    for entry in fs::read_dir(rule_dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|extension| extension == "rs") {
            source.push_str(&fs::read_to_string(path).unwrap());
        }
    }
    assert!(
        !source.contains(".omniscient("),
        "built-in rules must stay inside the default information view"
    );
}
