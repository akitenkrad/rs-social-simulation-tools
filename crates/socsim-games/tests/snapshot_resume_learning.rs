use std::sync::Arc;

use socsim_core::{AgentId, SimRng};
use socsim_engine::{Simulation, Snapshot};
use socsim_games::{
    assemble, build_world, AssignGroup, AssignKind, Assignment, Automaton, AutomatonKind,
    BuiltinPolicyState, DecisionContext, DecisionRule, DecisionScope, EstimateMode, FictitiousPlay,
    GameWorld, InitialActions, LogitChoice, MatrixGame, NormalFormOutcome, PlayOptions, RothErev,
    RoundRobin, RuleBook, RuleError, RuleInitContext, Slot, Stateless,
};

type World = GameWorld<MatrixGame>;

#[derive(Clone, Copy, Debug)]
struct PairHistoryProbe;

impl DecisionRule<MatrixGame, ()> for PairHistoryProbe {
    fn name(&self) -> &str {
        "pair_history_probe"
    }

    fn init_state(
        &self,
        _ctx: &RuleInitContext,
        _game: &MatrixGame,
    ) -> Result<BuiltinPolicyState, RuleError> {
        Ok(<BuiltinPolicyState as Slot<Stateless>>::from_slot(
            Stateless,
        ))
    }

    fn decide(
        &self,
        state: &mut BuiltinPolicyState,
        ctx: &DecisionContext<'_, MatrixGame, ()>,
        _rng: &mut SimRng,
    ) -> Result<usize, RuleError> {
        <BuiltinPolicyState as Slot<Stateless>>::slot(state)
            .ok_or(RuleError::MissingSlot("Stateless"))?;
        let opponent = ctx.opponents()[0];
        let pair = ctx
            .pair_history(opponent)
            .ok_or_else(|| RuleError::Unsupported("pair history is required".to_owned()))?;
        Ok(pair.len() % 2)
    }

    fn needs_pair_history(&self) -> bool {
        true
    }
}

fn positive_prisoners_dilemma() -> MatrixGame {
    MatrixGame::bimatrix(
        &[vec![4.0, 1.0], vec![6.0, 2.0]],
        &[vec![4.0, 6.0], vec![1.0, 2.0]],
    )
    .unwrap()
}

fn rules() -> RuleBook<MatrixGame, ()> {
    let mut rules = RuleBook::new();
    rules
        .register(
            "fictitious",
            Arc::new(
                FictitiousPlay::new()
                    .initial_counts(vec![0.5, 0.5])
                    .tie_break_random(),
            ),
        )
        .unwrap();
    rules
        .register("roth_erev", Arc::new(RothErev::new(0.1, 0.25, 1.0)))
        .unwrap();
    rules
        .register(
            "logit",
            Arc::new(LogitChoice::new(0.8).estimate(EstimateMode::LastOpponent)),
        )
        .unwrap();
    rules
        .register(
            "automaton",
            Arc::new(Automaton::new(AutomatonKind::Gtft { generosity: 0.4 }, 0)),
        )
        .unwrap();
    rules
        .register("pair_history_probe", Arc::new(PairHistoryProbe))
        .unwrap();
    rules
}

fn build_simulation(seed: u64, t_max: u64) -> Simulation<World> {
    let rules = rules();
    let world = build_world(
        positive_prisoners_dilemma(),
        5,
        (),
        InitialActions::Same(0),
        &rules,
        &Assignment::Ordered(
            [
                "fictitious",
                "roth_erev",
                "logit",
                "automaton",
                "pair_history_probe",
            ]
            .into_iter()
            .map(|kind| AssignGroup {
                kind: AssignKind::Rule(kind.to_owned()),
                count: 1,
            })
            .collect(),
        ),
        t_max as usize,
        t_max,
        seed,
    )
    .unwrap();
    assemble(
        world,
        RoundRobin::new(),
        rules,
        NormalFormOutcome,
        PlayOptions::new().decision_scope(DecisionScope::PerEncounter),
        seed,
    )
    .unwrap()
    .build()
}

fn run_steps(simulation: &mut Simulation<World>, count: u64) -> Vec<String> {
    (0..count)
        .map(|_| {
            simulation.step().unwrap();
            serde_json::to_string(simulation.world()).unwrap()
        })
        .collect()
}

#[test]
fn learning_pair_history_and_round_robin_state_resume_exactly() {
    const STEPS: u64 = 12;
    const SPLIT: u64 = 5;

    let mut uninterrupted = build_simulation(211, STEPS);
    let expected = run_steps(&mut uninterrupted, STEPS);

    let mut partial = build_simulation(211, STEPS);
    run_steps(&mut partial, SPLIT);
    let policies = partial.world().policies();
    let belief_opponents = match policies.state(AgentId(0)).unwrap() {
        BuiltinPolicyState::Belief(state) => state
            .by_opponent()
            .iter()
            .map(|(agent, _)| agent.0)
            .collect::<Vec<_>>(),
        state => panic!("expected belief state, got {state:?}"),
    };
    let propensity_opponents = match policies.state(AgentId(1)).unwrap() {
        BuiltinPolicyState::Propensity(state) => state
            .by_opponent()
            .iter()
            .map(|(agent, _)| agent.0)
            .collect::<Vec<_>>(),
        state => panic!("expected propensity state, got {state:?}"),
    };
    let estimate_opponents = match policies.state(AgentId(2)).unwrap() {
        BuiltinPolicyState::Estimate(state) => state
            .by_opponent()
            .iter()
            .map(|(agent, _)| agent.0)
            .collect::<Vec<_>>(),
        state => panic!("expected estimate state, got {state:?}"),
    };
    let automaton_opponents = match policies.state(AgentId(3)).unwrap() {
        BuiltinPolicyState::Automaton(state) => state
            .by_opponent()
            .iter()
            .map(|(agent, _)| agent.0)
            .collect::<Vec<_>>(),
        state => panic!("expected automaton state, got {state:?}"),
    };
    assert_eq!(belief_opponents, vec![1, 2, 3, 4]);
    assert_eq!(propensity_opponents, vec![0, 2, 3, 4]);
    assert_eq!(estimate_opponents, vec![0, 1, 3, 4]);
    assert_eq!(automaton_opponents, vec![0, 1, 2, 4]);

    let encoded = serde_json::to_string(&partial.snapshot()).unwrap();
    let snapshot: Snapshot<World> = serde_json::from_str(&encoded).unwrap();
    let snapshot_json = serde_json::to_value(&snapshot.world).unwrap();
    assert!(snapshot_json.get("pair_history").is_some());
    assert!(snapshot_json.get("matching_state").is_some());

    let mut resumed = build_simulation(999, STEPS);
    resumed.restore(snapshot);
    let actual = run_steps(&mut resumed, STEPS - SPLIT);

    assert_eq!(actual, expected[SPLIT as usize..]);
    assert_eq!(
        resumed.world().policies().kind(AgentId(4)),
        Some("pair_history_probe")
    );
}
