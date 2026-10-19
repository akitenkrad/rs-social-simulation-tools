#![cfg(all(feature = "engine", feature = "llm"))]

use std::sync::{Arc, Mutex};

use socsim_core::{AgentId, SimRng};
use socsim_games::llm::{ObservationProvider, OwnHistoryObs};
use socsim_games::{
    assemble, build_world, ArenaWriter, AssignGroup, AssignKind, Assignment, Credit,
    DecisionContext, DecisionRule, EncounterRef, ExperimentState, InitialActions, LearnContext,
    MatchContext, Matching, MatchingState, MatrixGame, NormalFormOutcome, OutcomeContext,
    OutcomeRule, PlayOptions, RuleBook, RuleError, RuleInitContext,
};

#[derive(Clone, Debug, Default)]
struct ObservedPolicyState {
    decision_t: Option<u64>,
    learn_t: Option<u64>,
    own_history_t: Option<u64>,
}

#[derive(Clone, Debug)]
struct TimeExperiment;

impl ExperimentState for TimeExperiment {
    type PolicyState = ObservedPolicyState;
    type View<'a> = ();

    fn view_for(&self, _me: AgentId) -> Self::View<'_> {}
}

#[derive(Clone, Copy, Debug)]
struct TimeRule;

impl DecisionRule<MatrixGame, TimeExperiment> for TimeRule {
    fn name(&self) -> &str {
        "time"
    }

    fn init_state(
        &self,
        _ctx: &RuleInitContext,
        _game: &MatrixGame,
    ) -> Result<ObservedPolicyState, RuleError> {
        Ok(ObservedPolicyState::default())
    }

    fn decide(
        &self,
        state: &mut ObservedPolicyState,
        ctx: &DecisionContext<'_, MatrixGame, TimeExperiment>,
        _rng: &mut SimRng,
    ) -> Result<usize, RuleError> {
        state.decision_t = Some(ctx.t());
        state.own_history_t = Some(OwnHistoryObs::new(1).observe(ctx).t);
        Ok(0)
    }

    fn learn_step(
        &self,
        state: &mut ObservedPolicyState,
        ctx: &LearnContext<'_, MatrixGame, TimeExperiment>,
    ) -> Result<(), RuleError> {
        state.learn_t = Some(ctx.t());
        Ok(())
    }
}

#[derive(Clone, Debug)]
struct TimeMatching {
    observed_t: Arc<Mutex<Option<u64>>>,
}

impl Matching<MatrixGame, TimeExperiment> for TimeMatching {
    fn name(&self) -> &str {
        "time"
    }

    fn check(
        &self,
        _ctx: &MatchContext<'_, MatrixGame, TimeExperiment>,
    ) -> Result<(), socsim_games::GameError> {
        Ok(())
    }

    fn encounters_into(
        &self,
        ctx: &MatchContext<'_, MatrixGame, TimeExperiment>,
        _state: &mut MatchingState,
        _rng: &mut SimRng,
        out: &mut ArenaWriter<'_, usize>,
    ) -> socsim_core::Result<()> {
        *self.observed_t.lock().unwrap() = Some(ctx.t());
        out.push(&[AgentId(0), AgentId(1)], Credit::All)?;
        Ok(())
    }
}

#[derive(Clone, Debug)]
struct TimeOutcome {
    observed_t: Arc<Mutex<Option<u64>>>,
}

impl OutcomeRule<MatrixGame, TimeExperiment> for TimeOutcome {
    fn name(&self) -> &str {
        "time"
    }

    fn outcome(
        &self,
        ctx: &OutcomeContext<'_, MatrixGame, TimeExperiment>,
        encounter: EncounterRef<'_>,
        actions: &[usize],
        payoffs: &mut [f64],
    ) -> socsim_core::Result<()> {
        *self.observed_t.lock().unwrap() = Some(ctx.t());
        NormalFormOutcome.outcome(ctx, encounter, actions, payoffs)
    }
}

#[derive(Clone, Copy, Debug)]
struct FirstStepTimes {
    decision: u64,
    learn: u64,
    matching: u64,
    outcome: u64,
    own_history: u64,
}

fn observe_first_step_times() -> FirstStepTimes {
    let mut rules = RuleBook::new();
    rules.register("time", Arc::new(TimeRule)).unwrap();
    let world = build_world(
        MatrixGame::zero_sum(&[vec![1.0, -1.0], vec![-1.0, 1.0]]).unwrap(),
        2,
        TimeExperiment,
        InitialActions::Same(0),
        &rules,
        &Assignment::Ordered(vec![AssignGroup {
            kind: AssignKind::Rule("time".to_owned()),
            count: 2,
        }]),
        1,
        1,
        11,
    )
    .unwrap();
    let matching_t = Arc::new(Mutex::new(None));
    let outcome_t = Arc::new(Mutex::new(None));
    let mut simulation = assemble(
        world,
        TimeMatching {
            observed_t: matching_t.clone(),
        },
        rules,
        TimeOutcome {
            observed_t: outcome_t.clone(),
        },
        PlayOptions::new(),
        11,
    )
    .unwrap()
    .build();

    simulation.step().unwrap();

    let state = simulation.world().policies().state(AgentId(0)).unwrap();
    let matching = matching_t.lock().unwrap().unwrap();
    let outcome = outcome_t.lock().unwrap().unwrap();
    FirstStepTimes {
        decision: state.decision_t.unwrap(),
        learn: state.learn_t.unwrap(),
        matching,
        outcome,
        own_history: state.own_history_t.unwrap(),
    }
}

#[test]
fn decision_context_t_is_one_based() {
    assert_eq!(observe_first_step_times().decision, 1);
}

#[test]
fn learn_context_t_is_one_based() {
    assert_eq!(observe_first_step_times().learn, 1);
}

#[test]
fn match_context_t_is_one_based() {
    assert_eq!(observe_first_step_times().matching, 1);
}

#[test]
fn own_history_t_is_one_based() {
    assert_eq!(observe_first_step_times().own_history, 1);
}

#[test]
fn outcome_context_t_is_one_based() {
    assert_eq!(observe_first_step_times().outcome, 1);
}
