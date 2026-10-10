use std::collections::BTreeSet;
use std::sync::Arc;

use socsim_core::{AgentId, SimRng};
use socsim_games::{
    assemble, build_world, ArenaWriter, AssignGroup, AssignKind, Assignment, Automaton,
    AutomatonKind, Credit, DecisionContext, DecisionRule, DecisionScope, DecisionUnit,
    EncounterOutcome, Fixed, InitialActions, LearnContext, MatchContext, Matching, MatchingState,
    MatrixGame, MyopicBestResponse, NormalFormOutcome, PayoffAggregation, PlayOptions, RoundRobin,
    RuleBook, RuleError, RuleInitContext,
};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct TraceState {
    decisions: Vec<DecisionTrace>,
    learning: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DecisionTrace {
    scope: DecisionScope,
    unit: DecisionUnit,
    role: Option<usize>,
    encounter: Option<u32>,
    opponents: Vec<u64>,
    has_opponent_history: bool,
}

#[derive(Clone, Debug)]
struct TraceExperiment;

impl socsim_games::ExperimentState for TraceExperiment {
    type PolicyState = TraceState;
    type View<'a> = ();

    fn view_for(&self, _me: AgentId) -> Self::View<'_> {}
}

#[derive(Clone, Copy, Debug)]
struct TraceRule;

impl DecisionRule<MatrixGame, TraceExperiment> for TraceRule {
    fn name(&self) -> &str {
        "trace"
    }

    fn init_state(
        &self,
        _ctx: &RuleInitContext,
        _game: &MatrixGame,
    ) -> Result<TraceState, RuleError> {
        Ok(TraceState::default())
    }

    fn decide(
        &self,
        state: &mut TraceState,
        ctx: &DecisionContext<'_, MatrixGame, TraceExperiment>,
        _rng: &mut SimRng,
    ) -> Result<usize, RuleError> {
        let opponents = ctx
            .opponents()
            .iter()
            .map(|agent| agent.0)
            .collect::<Vec<_>>();
        state.decisions.push(DecisionTrace {
            scope: ctx.scope(),
            unit: ctx.unit(),
            role: ctx.role(),
            encounter: ctx.encounter().map(|encounter| encounter.id.0),
            has_opponent_history: ctx
                .opponents()
                .iter()
                .all(|&opponent| ctx.opponent_history(opponent).is_some()),
            opponents,
        });
        Ok(ctx.role().unwrap_or(0))
    }

    fn learn_encounter(
        &self,
        state: &mut TraceState,
        outcome: &EncounterOutcome<'_, usize>,
        ctx: &LearnContext<'_, MatrixGame, TraceExperiment>,
    ) -> Result<(), RuleError> {
        state
            .learning
            .push(format!("encounter:{}:{:?}", outcome.id.0, ctx.scope()));
        Ok(())
    }

    fn needs_encounter_learning(&self) -> bool {
        true
    }

    fn needs_pair_history(&self) -> bool {
        true
    }

    fn learn_step(
        &self,
        state: &mut TraceState,
        ctx: &LearnContext<'_, MatrixGame, TraceExperiment>,
    ) -> Result<(), RuleError> {
        state.learning.push(format!("step:{:?}", ctx.scope()));
        Ok(())
    }
}

fn game() -> MatrixGame {
    MatrixGame::zero_sum(&[vec![1.0, -1.0], vec![-1.0, 1.0]]).unwrap()
}

fn trace_rules() -> RuleBook<MatrixGame, TraceExperiment> {
    let mut rules = RuleBook::new();
    rules.register("trace", Arc::new(TraceRule)).unwrap();
    rules
}

fn trace_world(
    n_agents: usize,
    rules: &RuleBook<MatrixGame, TraceExperiment>,
) -> socsim_games::GameWorld<MatrixGame, TraceExperiment> {
    build_world(
        game(),
        n_agents,
        TraceExperiment,
        InitialActions::Same(0),
        rules,
        &Assignment::Ordered(vec![AssignGroup {
            kind: AssignKind::Rule("trace".to_owned()),
            count: n_agents,
        }]),
        2,
        1,
        17,
    )
    .unwrap()
}

fn state(
    simulation: &socsim_engine::Simulation<socsim_games::GameWorld<MatrixGame, TraceExperiment>>,
    agent: u64,
) -> &TraceState {
    simulation.world().policies().state(AgentId(agent)).unwrap()
}

#[test]
fn round_robin_decision_context_matches_each_fine_scope() {
    for (scope, expected_counts) in [
        (DecisionScope::PerAgentRole, [1, 2, 1]),
        (DecisionScope::PerEncounter, [2, 2, 2]),
    ] {
        let rules = trace_rules();
        let mut simulation = assemble(
            trace_world(3, &rules),
            RoundRobin::new(),
            rules,
            NormalFormOutcome,
            PlayOptions::new().decision_scope(scope),
            17,
        )
        .unwrap()
        .build();
        simulation.step().unwrap();

        for (agent, expected) in expected_counts.into_iter().enumerate() {
            let state = state(&simulation, agent as u64);
            assert_eq!(state.decisions.len(), expected);
            assert!(state.decisions.iter().all(|trace| trace.scope == scope));
            assert!(state
                .decisions
                .iter()
                .all(|trace| trace.has_opponent_history));
            assert_eq!(state.learning.last(), Some(&format!("step:{scope:?}")));
            let encounter_ids = state
                .learning
                .iter()
                .filter_map(|event| event.strip_prefix("encounter:"))
                .map(|event| event.split(':').next().unwrap().parse::<u32>().unwrap())
                .collect::<Vec<_>>();
            assert!(encounter_ids.windows(2).all(|ids| ids[0] < ids[1]));
        }

        if scope == DecisionScope::PerAgentRole {
            assert_eq!(
                state(&simulation, 0).decisions[0],
                DecisionTrace {
                    scope,
                    unit: DecisionUnit::AgentRole(0),
                    role: Some(0),
                    encounter: None,
                    opponents: vec![1, 2],
                    has_opponent_history: true,
                }
            );
            assert_eq!(
                state(&simulation, 1)
                    .decisions
                    .iter()
                    .map(|trace| trace.role.unwrap())
                    .collect::<Vec<_>>(),
                vec![0, 1]
            );
        } else {
            assert_eq!(
                state(&simulation, 0)
                    .decisions
                    .iter()
                    .map(|trace| (
                        trace.unit,
                        trace.role,
                        trace.encounter,
                        trace.opponents.clone()
                    ))
                    .collect::<Vec<_>>(),
                vec![
                    (
                        DecisionUnit::Encounter(socsim_games::EncounterId(0)),
                        Some(0),
                        Some(0),
                        vec![1]
                    ),
                    (
                        DecisionUnit::Encounter(socsim_games::EncounterId(1)),
                        Some(0),
                        Some(1),
                        vec![2]
                    ),
                ]
            );
        }

        for agent in 0..3 {
            assert_eq!(
                simulation
                    .world()
                    .history(AgentId(agent))
                    .unwrap()
                    .last()
                    .unwrap()
                    .action,
                None
            );
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct SameRoleMultipleSeats;

impl Matching<MatrixGame, TraceExperiment> for SameRoleMultipleSeats {
    fn name(&self) -> &str {
        "same_role_multiple_seats"
    }

    fn check(
        &self,
        _ctx: &MatchContext<'_, MatrixGame, TraceExperiment>,
    ) -> Result<(), socsim_games::GameError> {
        Ok(())
    }

    fn encounters_into(
        &self,
        _ctx: &MatchContext<'_, MatrixGame, TraceExperiment>,
        _state: &mut MatchingState,
        _rng: &mut SimRng,
        out: &mut ArenaWriter<'_, usize>,
    ) -> socsim_core::Result<()> {
        out.push(&[AgentId(0), AgentId(1)], Credit::All)?;
        out.push(&[AgentId(0), AgentId(2)], Credit::All)?;
        Ok(())
    }
}

#[test]
fn per_step_decides_once_and_fills_every_seat_with_the_same_action() {
    let rules = trace_rules();
    let mut simulation = assemble(
        trace_world(3, &rules),
        SameRoleMultipleSeats,
        rules,
        NormalFormOutcome,
        PlayOptions::new(),
        17,
    )
    .unwrap()
    .build();
    simulation.step().unwrap();

    assert_eq!(state(&simulation, 0).decisions.len(), 1);
    assert_eq!(
        state(&simulation, 0).decisions[0].scope,
        DecisionScope::PerStep
    );
    assert_eq!(state(&simulation, 0).decisions[0].unit, DecisionUnit::Step);
    assert_eq!(
        simulation
            .world()
            .plays()
            .actions(socsim_games::EncounterId(0)),
        Some(&[Some(0), Some(1)][..])
    );
    assert_eq!(
        simulation
            .world()
            .plays()
            .actions(socsim_games::EncounterId(1)),
        Some(&[Some(0), Some(1)][..])
    );
    assert_eq!(simulation.world().held(), &[0, 1, 1]);
    assert_eq!(
        simulation
            .world()
            .history(AgentId(0))
            .unwrap()
            .last()
            .unwrap()
            .action,
        Some(0)
    );
    assert_eq!(
        state(&simulation, 0).learning,
        vec![
            "encounter:0:PerStep".to_owned(),
            "encounter:1:PerStep".to_owned(),
            "step:PerStep".to_owned(),
        ]
    );
}

#[test]
fn held_agents_fill_all_round_robin_seats_at_fine_scopes() {
    for scope in [DecisionScope::PerAgentRole, DecisionScope::PerEncounter] {
        let rules: RuleBook<MatrixGame, ()> = RuleBook::new();
        let world = build_world(
            game(),
            3,
            (),
            InitialActions::PerAgent(vec![0, 1, 0]),
            &rules,
            &Assignment::Ordered(vec![AssignGroup {
                kind: AssignKind::Held,
                count: 3,
            }]),
            1,
            1,
            17,
        )
        .unwrap();
        let mut simulation = assemble(
            world,
            RoundRobin::new(),
            rules,
            NormalFormOutcome,
            PlayOptions::new().decision_scope(scope),
            17,
        )
        .unwrap()
        .build();
        simulation.step().unwrap();

        let actions = (0..simulation.world().plays().n_encounters())
            .flat_map(|index| {
                simulation
                    .world()
                    .plays()
                    .actions(socsim_games::EncounterId(index as u32))
                    .unwrap()
                    .iter()
            })
            .collect::<Vec<_>>();
        assert!(actions.iter().all(|action| action.is_some()));
        assert_eq!(
            actions
                .into_iter()
                .flatten()
                .copied()
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([0, 1])
        );
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct PairTraceState {
    observations: Vec<PairObservation>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct PairObservation {
    t: u64,
    opponent: u64,
    pair: Vec<(u64, usize, usize)>,
    non_opponent_visible: bool,
    omniscient_one_two: Option<Vec<(u64, usize, usize)>>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct PairTraceExperiment;

impl socsim_games::ExperimentState for PairTraceExperiment {
    type PolicyState = PairTraceState;
    type View<'a> = ();

    fn view_for(&self, _me: AgentId) -> Self::View<'_> {}
}

#[derive(Clone, Copy, Debug)]
struct PairTraceRule;

impl DecisionRule<MatrixGame, PairTraceExperiment> for PairTraceRule {
    fn name(&self) -> &str {
        "pair_trace"
    }

    fn init_state(
        &self,
        _ctx: &RuleInitContext,
        _game: &MatrixGame,
    ) -> Result<PairTraceState, RuleError> {
        Ok(PairTraceState::default())
    }

    fn decide(
        &self,
        state: &mut PairTraceState,
        ctx: &DecisionContext<'_, MatrixGame, PairTraceExperiment>,
        _rng: &mut SimRng,
    ) -> Result<usize, RuleError> {
        let opponent = ctx.opponents()[0];
        let pair = ctx
            .pair_history(opponent)
            .expect("the current opponent's pair history is visible")
            .iter()
            .map(|(t, mine, theirs)| (t, *mine, *theirs))
            .collect();
        let non_opponent = (0..3)
            .map(AgentId)
            .find(|&agent| agent != ctx.me() && agent != opponent)
            .expect("three-agent tests always have a non-opponent");
        let omniscient_one_two =
            ctx.omniscient()
                .pair_history(AgentId(1), AgentId(2))
                .map(|history| {
                    history
                        .iter()
                        .map(|(t, one, two)| (t, *one, *two))
                        .collect()
                });
        state.observations.push(PairObservation {
            t: ctx.t(),
            opponent: opponent.0,
            pair,
            non_opponent_visible: ctx.pair_history(non_opponent).is_some(),
            omniscient_one_two,
        });
        Ok((ctx.me().0 as usize + ctx.t() as usize + ctx.role().unwrap()) % 2)
    }

    fn needs_pair_history(&self) -> bool {
        true
    }
}

type PairWorld = socsim_games::GameWorld<MatrixGame, PairTraceExperiment>;

fn pair_rules() -> RuleBook<MatrixGame, PairTraceExperiment> {
    let mut rules = RuleBook::new();
    rules
        .register("pair_trace", Arc::new(PairTraceRule))
        .unwrap();
    rules
}

fn pair_world(history_len: usize, t_max: u64) -> PairWorld {
    let rules = pair_rules();
    build_world(
        game(),
        3,
        PairTraceExperiment,
        InitialActions::Same(0),
        &rules,
        &Assignment::Ordered(vec![AssignGroup {
            kind: AssignKind::Rule("pair_trace".to_owned()),
            count: 3,
        }]),
        history_len,
        t_max,
        29,
    )
    .unwrap()
}

fn pair_simulation(history_len: usize, t_max: u64) -> socsim_engine::Simulation<PairWorld> {
    let rules = pair_rules();
    assemble(
        pair_world(history_len, t_max),
        RoundRobin::new(),
        rules,
        NormalFormOutcome,
        PlayOptions::new().decision_scope(DecisionScope::PerEncounter),
        29,
    )
    .unwrap()
    .build()
}

fn pair_state(simulation: &socsim_engine::Simulation<PairWorld>, agent: u64) -> &PairTraceState {
    simulation.world().policies().state(AgentId(agent)).unwrap()
}

fn expected_pair_record(me: u64, other: u64, t: u64) -> (u64, usize, usize) {
    let (lower, upper) = if me < other { (me, other) } else { (other, me) };
    let lower_action = (lower as usize + t as usize) % 2;
    let upper_action = (upper as usize + t as usize + 1) % 2;
    if me == lower {
        (t, lower_action, upper_action)
    } else {
        (t, upper_action, lower_action)
    }
}

#[test]
fn round_robin_pair_history_is_separate_bounded_oldest_first_and_omniscient() {
    let mut simulation = pair_simulation(2, 4);
    for _ in 0..4 {
        simulation.step().unwrap();
    }

    for me in 0..3 {
        let observations = &pair_state(&simulation, me).observations;
        assert!(observations.iter().all(|observation| observation
            .pair
            .iter()
            .all(|record| record.0 < observation.t)));
        assert!(observations
            .iter()
            .filter(|observation| observation.t == 1)
            .all(|observation| observation.pair.is_empty()));
        for other in 0..3 {
            if other == me {
                continue;
            }
            let latest = observations
                .iter()
                .find(|observation| observation.t == 4 && observation.opponent == other)
                .unwrap();
            assert_eq!(
                latest.pair,
                vec![
                    expected_pair_record(me, other, 2),
                    expected_pair_record(me, other, 3),
                ]
            );
        }
    }

    let omniscient = pair_state(&simulation, 0)
        .observations
        .iter()
        .find(|observation| observation.t == 4)
        .unwrap()
        .omniscient_one_two
        .as_ref()
        .unwrap();
    assert_eq!(
        omniscient,
        &vec![expected_pair_record(1, 2, 2), expected_pair_record(1, 2, 3),]
    );
}

#[test]
fn round_robin_both_orders_records_two_encounters_in_encounter_id_order() {
    let rules = pair_rules();
    let mut simulation = assemble(
        pair_world(4, 2),
        RoundRobin::new().both_orders(true),
        rules,
        NormalFormOutcome,
        PlayOptions::new().decision_scope(DecisionScope::PerEncounter),
        29,
    )
    .unwrap()
    .build();
    simulation.step().unwrap();
    simulation.step().unwrap();

    let pair = pair_state(&simulation, 0)
        .observations
        .iter()
        .find(|observation| observation.t == 2 && observation.opponent == 1)
        .unwrap()
        .pair
        .clone();
    assert_eq!(pair, vec![(1, 1, 1), (1, 0, 0)]);
}

#[derive(Clone, Copy, Debug)]
struct AlternatingOpponent;

impl Matching<MatrixGame, PairTraceExperiment> for AlternatingOpponent {
    fn name(&self) -> &str {
        "alternating_opponent"
    }

    fn check(
        &self,
        _ctx: &MatchContext<'_, MatrixGame, PairTraceExperiment>,
    ) -> Result<(), socsim_games::GameError> {
        Ok(())
    }

    fn encounters_into(
        &self,
        ctx: &MatchContext<'_, MatrixGame, PairTraceExperiment>,
        _state: &mut MatchingState,
        _rng: &mut SimRng,
        out: &mut ArenaWriter<'_, usize>,
    ) -> socsim_core::Result<()> {
        let opponent = if ctx.t().is_multiple_of(2) { 2 } else { 1 };
        out.push(&[AgentId(0), AgentId(opponent)], Credit::All)?;
        Ok(())
    }
}

#[test]
fn pair_history_hides_a_past_opponent_who_is_not_in_the_current_encounter() {
    let rules = pair_rules();
    let mut simulation = assemble(
        pair_world(4, 2),
        AlternatingOpponent,
        rules,
        NormalFormOutcome,
        PlayOptions::new().decision_scope(DecisionScope::PerEncounter),
        29,
    )
    .unwrap()
    .build();
    simulation.step().unwrap();
    simulation.step().unwrap();

    let observation = pair_state(&simulation, 0)
        .observations
        .iter()
        .find(|observation| observation.t == 2)
        .unwrap();
    assert_eq!(observation.opponent, 2);
    assert!(observation.pair.is_empty());
    assert!(!observation.non_opponent_visible);
    assert_eq!(
        observation
            .omniscient_one_two
            .as_ref()
            .expect("known arbitrary pairs have a visible, possibly empty view"),
        &Vec::new()
    );
}

#[test]
fn world_omits_pair_history_from_json_when_no_assigned_rule_needs_it() {
    let rules: RuleBook<MatrixGame, ()> = RuleBook::new();
    let world = build_world(
        game(),
        2,
        (),
        InitialActions::Same(0),
        &rules,
        &Assignment::Ordered(vec![AssignGroup {
            kind: AssignKind::Held,
            count: 2,
        }]),
        2,
        1,
        31,
    )
    .unwrap();
    let json = serde_json::to_value(world).unwrap();
    assert!(json.get("pair_history").is_none());
}

#[test]
fn snapshot_restore_preserves_pair_history_trajectory() {
    let mut uninterrupted = pair_simulation(3, 5);
    let expected = (0..5)
        .map(|_| {
            uninterrupted.step().unwrap();
            serde_json::to_string(uninterrupted.world()).unwrap()
        })
        .collect::<Vec<_>>();

    let mut partial = pair_simulation(3, 5);
    partial.step().unwrap();
    partial.step().unwrap();
    let encoded = serde_json::to_string(&partial.snapshot()).unwrap();
    let snapshot: socsim_engine::Snapshot<PairWorld> = serde_json::from_str(&encoded).unwrap();
    assert!(serde_json::to_value(&snapshot.world)
        .unwrap()
        .get("pair_history")
        .is_some());

    let mut resumed = pair_simulation(3, 5);
    resumed.restore(snapshot);
    let actual = (0..3)
        .map(|_| {
            resumed.step().unwrap();
            serde_json::to_string(resumed.world()).unwrap()
        })
        .collect::<Vec<_>>();
    assert_eq!(actual, expected[2..]);
}

#[test]
fn per_encounter_tft_keeps_separate_state_for_each_round_robin_opponent() {
    const STEPS: usize = 4;
    let game = MatrixGame::bimatrix(
        &[vec![3.0, 0.0], vec![5.0, 1.0]],
        &[vec![3.0, 5.0], vec![0.0, 1.0]],
    )
    .unwrap();
    let mut rules = RuleBook::new();
    rules
        .register("tft", Arc::new(Automaton::new(AutomatonKind::Tft, 0)))
        .unwrap();
    rules
        .register("all_d", Arc::new(Automaton::new(AutomatonKind::AllD, 0)))
        .unwrap();
    rules
        .register("all_c", Arc::new(Automaton::new(AutomatonKind::AllC, 0)))
        .unwrap();
    let world = build_world(
        game,
        4,
        (),
        InitialActions::Same(0),
        &rules,
        &Assignment::Ordered(vec![
            AssignGroup {
                kind: AssignKind::Rule("tft".to_owned()),
                count: 1,
            },
            AssignGroup {
                kind: AssignKind::Rule("all_d".to_owned()),
                count: 1,
            },
            AssignGroup {
                kind: AssignKind::Rule("all_c".to_owned()),
                count: 2,
            },
        ]),
        STEPS,
        STEPS as u64,
        37,
    )
    .unwrap();
    let mut simulation = assemble(
        world,
        RoundRobin::new(),
        rules,
        NormalFormOutcome,
        PlayOptions::new()
            .decision_scope(DecisionScope::PerEncounter)
            .payoff_aggregation(PayoffAggregation::Sum),
        37,
    )
    .unwrap()
    .build();

    let mut cumulative = [0.0; 4];
    for _ in 0..STEPS {
        simulation.step().unwrap();
        for (total, payoff) in cumulative.iter_mut().zip(simulation.world().payoffs()) {
            *total += payoff;
        }
    }

    // Agent 0 (TFT) defects after the first round only against agent 1
    // (ALLD), while continuing to cooperate with agents 2 and 3 (ALLC).
    assert_eq!(cumulative, [27.0, 48.0, 24.0, 24.0]);
}

fn coordination_game() -> MatrixGame {
    MatrixGame::bimatrix(
        &[vec![2.0, 0.0], vec![0.0, 2.0]],
        &[vec![2.0, 0.0], vec![0.0, 2.0]],
    )
    .unwrap()
}

fn myopic_round_robin(
    scope: DecisionScope,
    first_action: usize,
    fixed_zero: usize,
    fixed_one: usize,
    t_max: u64,
) -> socsim_engine::Simulation<socsim_games::GameWorld<MatrixGame>> {
    let mut rules = RuleBook::new();
    rules
        .register(
            "myopic",
            Arc::new(MyopicBestResponse::new().first_action(first_action)),
        )
        .unwrap();
    rules
        .register("fixed_zero", Arc::new(Fixed::new(0)))
        .unwrap();
    rules
        .register("fixed_one", Arc::new(Fixed::new(1)))
        .unwrap();

    let n_agents = 1 + fixed_zero + fixed_one;
    let world = build_world(
        coordination_game(),
        n_agents,
        (),
        InitialActions::Same(0),
        &rules,
        &Assignment::Ordered(vec![
            AssignGroup {
                kind: AssignKind::Rule("myopic".to_owned()),
                count: 1,
            },
            AssignGroup {
                kind: AssignKind::Rule("fixed_zero".to_owned()),
                count: fixed_zero,
            },
            AssignGroup {
                kind: AssignKind::Rule("fixed_one".to_owned()),
                count: fixed_one,
            },
        ]),
        t_max as usize,
        t_max,
        41,
    )
    .unwrap();
    assemble(
        world,
        RoundRobin::new(),
        rules,
        NormalFormOutcome,
        PlayOptions::new().decision_scope(scope),
        41,
    )
    .unwrap()
    .build()
}

#[test]
fn per_encounter_myopic_responds_to_each_opponents_last_pair_action() {
    let mut simulation = myopic_round_robin(DecisionScope::PerEncounter, 0, 1, 1, 2);

    simulation.step().unwrap();
    assert_eq!(
        simulation
            .world()
            .plays()
            .actions(socsim_games::EncounterId(0)),
        Some(&[Some(0), Some(0)][..])
    );
    assert_eq!(
        simulation
            .world()
            .plays()
            .actions(socsim_games::EncounterId(1)),
        Some(&[Some(0), Some(1)][..])
    );

    simulation.step().unwrap();
    assert_eq!(
        simulation
            .world()
            .plays()
            .actions(socsim_games::EncounterId(0)),
        Some(&[Some(0), Some(0)][..])
    );
    assert_eq!(
        simulation
            .world()
            .plays()
            .actions(socsim_games::EncounterId(1)),
        Some(&[Some(1), Some(1)][..])
    );
}

#[test]
fn per_agent_role_myopic_best_responds_to_the_opponent_action_mixture() {
    let mut simulation = myopic_round_robin(DecisionScope::PerAgentRole, 0, 1, 2, 2);

    simulation.step().unwrap();
    for encounter in 0..3 {
        assert_eq!(
            simulation
                .world()
                .plays()
                .actions(socsim_games::EncounterId(encounter))
                .unwrap()[0],
            Some(0)
        );
    }

    simulation.step().unwrap();
    for encounter in 0..3 {
        assert_eq!(
            simulation
                .world()
                .plays()
                .actions(socsim_games::EncounterId(encounter))
                .unwrap()[0],
            Some(1)
        );
    }
}

#[test]
fn per_step_myopic_still_uses_the_opponents_previous_step_action() {
    let mut simulation = myopic_round_robin(DecisionScope::PerStep, 0, 0, 1, 2);

    simulation.step().unwrap();
    assert_eq!(
        simulation
            .world()
            .plays()
            .actions(socsim_games::EncounterId(0)),
        Some(&[Some(0), Some(1)][..])
    );

    simulation.step().unwrap();
    assert_eq!(
        simulation
            .world()
            .plays()
            .actions(socsim_games::EncounterId(0)),
        Some(&[Some(1), Some(1)][..])
    );
}

#[cfg(feature = "llm")]
mod llm_guards {
    use super::*;
    use socsim_games::llm::{
        assemble_with_llm, BackendFactory, BackendIdentity, CountsObs, CountsRenderer, LabelParser,
        LlmPolicy, LlmReplayManifest, LlmRuleBook, LlmSetup, ReplayMode, ResponseStore, WithLlm,
    };
    use socsim_games::{build_world_with, GameError, KindCatalog};
    use socsim_llm::mock::ScriptedClient;

    type LlmExperiment = WithLlm<()>;

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
                r#"{"action":"a1","reason":"multiple seats"}"#,
            )))
        })
    }

    fn llm_rules() -> LlmRuleBook<MatrixGame, LlmExperiment> {
        let mut rules = LlmRuleBook::new();
        rules
            .register(
                "llm",
                Arc::new(LlmPolicy::new(
                    CountsObs,
                    CountsRenderer::default(),
                    LabelParser::strict(),
                )),
            )
            .unwrap();
        rules
    }

    #[derive(Clone, Copy, Debug)]
    struct LlmMultipleSeats;

    impl Matching<MatrixGame, LlmExperiment> for LlmMultipleSeats {
        fn name(&self) -> &str {
            "llm_multiple_seats"
        }

        fn check(
            &self,
            _ctx: &MatchContext<'_, MatrixGame, LlmExperiment>,
        ) -> Result<(), GameError> {
            Ok(())
        }

        fn encounters_into(
            &self,
            _ctx: &MatchContext<'_, MatrixGame, LlmExperiment>,
            _state: &mut MatchingState,
            _rng: &mut SimRng,
            out: &mut ArenaWriter<'_, usize>,
        ) -> socsim_core::Result<()> {
            out.push(&[AgentId(0), AgentId(1)], Credit::All)?;
            out.push(&[AgentId(0), AgentId(2)], Credit::All)?;
            Ok(())
        }
    }

    fn setup_world() -> (
        socsim_games::GameWorld<MatrixGame, LlmExperiment>,
        RuleBook<MatrixGame, LlmExperiment>,
        LlmSetup<MatrixGame, LlmExperiment>,
    ) {
        let regular = RuleBook::new();
        let llm = llm_rules();
        let catalog = KindCatalog::new().with_llm(&llm).unwrap();
        let store = ResponseStore::in_memory("decision-scope", ReplayMode::Live);
        let backend_identity = identity();
        let world = build_world_with(
            game()
                .with_labels(vec![
                    vec!["a0".to_owned(), "a1".to_owned()],
                    vec!["a0".to_owned(), "a1".to_owned()],
                ])
                .unwrap(),
            3,
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
                    count: 2,
                },
            ]),
            1,
            1,
            23,
        )
        .unwrap();
        (
            world,
            regular,
            LlmSetup::new(llm, store, backend(), backend_identity),
        )
    }

    #[test]
    fn llm_per_step_fills_every_seat_owned_by_an_agent() {
        let (world, regular, llm) = setup_world();
        let mut simulation = assemble_with_llm(
            world,
            LlmMultipleSeats,
            regular,
            llm,
            NormalFormOutcome,
            PlayOptions::new(),
            23,
        )
        .unwrap()
        .build();

        simulation.step().unwrap();
        assert_eq!(
            simulation
                .world()
                .plays()
                .actions(socsim_games::EncounterId(0)),
            Some(&[Some(1), Some(0)][..])
        );
        assert_eq!(
            simulation
                .world()
                .plays()
                .actions(socsim_games::EncounterId(1)),
            Some(&[Some(1), Some(0)][..])
        );
    }

    #[test]
    fn assemble_with_llm_rejects_finer_decision_scopes() {
        for scope in [DecisionScope::PerAgentRole, DecisionScope::PerEncounter] {
            let (world, regular, llm) = setup_world();
            let result = assemble_with_llm(
                world,
                LlmMultipleSeats,
                regular,
                llm,
                NormalFormOutcome,
                PlayOptions::new().decision_scope(scope),
                23,
            );
            assert!(matches!(
                result,
                Err(GameError::Rule(RuleError::Unsupported(_)))
            ));
        }
    }
}
