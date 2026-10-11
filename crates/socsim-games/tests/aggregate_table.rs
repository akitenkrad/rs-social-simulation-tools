use socsim_games::{
    ActionError, AggregateGame, AggregateReduce, AggregateSpec, FiniteGame, NormalFormGame,
    StageGame,
};

#[test]
fn aggregate_spec_accepts_valid_names_and_boundaries() {
    for name in ["count", "mean_value", "group-2", "0count", &"a".repeat(49)] {
        let spec = AggregateSpec::new(name, AggregateReduce::Sum).unwrap();
        assert_eq!(spec.name(), name);
        assert_eq!(spec.reduce(), AggregateReduce::Sum);
    }
}

#[test]
fn aggregate_spec_rejects_invalid_names() {
    for name in ["", "Count", "count.mean", "_count", &"a".repeat(50)] {
        assert!(AggregateSpec::new(name, AggregateReduce::Mean).is_err());
    }
}

#[test]
fn aggregate_spec_serde_round_trip_preserves_values() {
    let spec = AggregateSpec::new("zero_count", AggregateReduce::Mean).unwrap();
    let json = serde_json::to_string(&spec).unwrap();
    let decoded = serde_json::from_str::<AggregateSpec>(&json).unwrap();

    assert_eq!(decoded, spec);
}

#[test]
fn aggregate_spec_deserialization_cannot_bypass_name_validation() {
    let valid = AggregateSpec::new("valid", AggregateReduce::Sum).unwrap();
    let mut invalid = serde_json::to_value(valid).unwrap();

    for name in ["", "Upper", "has.dot", "_leading", &"x".repeat(50)] {
        invalid["name"] = serde_json::Value::String(name.to_owned());
        assert!(serde_json::from_value::<AggregateSpec>(invalid.clone()).is_err());
    }
}

#[derive(Debug)]
struct ZeroCountGame {
    aggregate_spec: Vec<AggregateSpec>,
}

impl ZeroCountGame {
    fn new() -> Self {
        Self {
            aggregate_spec: vec![AggregateSpec::new("zero_count", AggregateReduce::Sum).unwrap()],
        }
    }
}

impl StageGame for ZeroCountGame {
    type Action = usize;

    fn n_roles(&self) -> usize {
        2
    }

    fn check_action(&self, role: usize, action: &usize) -> Result<(), ActionError> {
        if role >= self.n_roles() {
            return Err(ActionError::InvalidRole { role });
        }
        if *action >= 2 {
            return Err(ActionError::InvalidAction {
                role,
                action: *action,
            });
        }
        Ok(())
    }
}

impl NormalFormGame for ZeroCountGame {
    fn payoff(&self, _role: usize, _profile: &[Self::Action]) -> f64 {
        0.0
    }
}

impl FiniteGame for ZeroCountGame {
    fn n_actions(&self, _role: usize) -> usize {
        2
    }
}

impl AggregateGame for ZeroCountGame {
    fn aggregate_spec(&self) -> &[AggregateSpec] {
        &self.aggregate_spec
    }

    fn aggregate_into(&self, profile: &[Self::Action], out: &mut [f64]) {
        out[0] = profile.iter().filter(|&&action| action == 0).count() as f64;
    }
}

#[test]
fn aggregate_game_can_write_profile_values_into_the_output_slice() {
    let game = ZeroCountGame::new();
    let mut out = [f64::NAN];

    game.aggregate_into(&[0, 1], &mut out);

    assert_eq!(game.aggregate_spec()[0].name(), "zero_count");
    assert_eq!(out, [1.0]);
}

#[cfg(feature = "engine")]
mod engine {
    use std::sync::Arc;

    use socsim_core::{AgentId, SimRng};
    use socsim_engine::{Simulation, Snapshot};
    use socsim_games::{
        assemble, assemble_with_aggregates, build_world, ActionError, AggregateGame,
        AggregateReduce, AggregateSpec, ArenaWriter, AssignGroup, AssignKind, Assignment, Credit,
        DecisionContext, DecisionRule, DecisionScope, ExperimentState, GameError, GameWorld,
        InitialActions, MatchContext, Matching, MatchingState, NormalFormGame, NormalFormOutcome,
        PlayOptions, RuleBook, RuleError, RuleInitContext, StageGame,
    };

    #[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
    struct TestGame {
        specs: Vec<AggregateSpec>,
        non_finite: bool,
    }

    impl TestGame {
        fn new() -> Self {
            Self {
                specs: vec![
                    AggregateSpec::new("zero_count", AggregateReduce::Sum).unwrap(),
                    AggregateSpec::new("payoff_sum", AggregateReduce::Mean).unwrap(),
                ],
                non_finite: false,
            }
        }

        fn non_finite() -> Self {
            Self {
                non_finite: true,
                ..Self::new()
            }
        }
    }

    impl StageGame for TestGame {
        type Action = usize;

        fn n_roles(&self) -> usize {
            2
        }

        fn check_action(&self, role: usize, action: &usize) -> Result<(), ActionError> {
            if role >= self.n_roles() {
                return Err(ActionError::InvalidRole { role });
            }
            if *action >= 2 {
                return Err(ActionError::InvalidAction {
                    role,
                    action: *action,
                });
            }
            Ok(())
        }
    }

    impl NormalFormGame for TestGame {
        fn payoff(&self, role: usize, profile: &[Self::Action]) -> f64 {
            if profile[role] == 0 {
                2.0
            } else {
                -1.0
            }
        }
    }

    impl AggregateGame for TestGame {
        fn aggregate_spec(&self) -> &[AggregateSpec] {
            &self.specs
        }

        fn aggregate_into(&self, profile: &[Self::Action], out: &mut [f64]) {
            out[0] = if self.non_finite {
                f64::NAN
            } else {
                profile.iter().filter(|&&action| action == 0).count() as f64
            };
            out[1] = (0..self.n_roles())
                .map(|role| self.payoff(role, profile))
                .sum();
        }
    }

    #[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
    struct AggregateObservation {
        t: u64,
        available: bool,
        names: Vec<String>,
        rows: Vec<(u64, Vec<Option<f64>>)>,
        zero_column: Option<Vec<Option<f64>>>,
        last: Option<(u64, Vec<Option<f64>>)>,
        missing_column: bool,
    }

    #[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
    struct ProbeState {
        action: usize,
        observations: Vec<AggregateObservation>,
    }

    #[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
    struct TestExperiment;

    impl ExperimentState for TestExperiment {
        type PolicyState = ProbeState;
        type View<'a> = ();

        fn view_for(&self, _me: AgentId) -> Self::View<'_> {}
    }

    struct AggregateProbe;

    impl DecisionRule<TestGame, TestExperiment> for AggregateProbe {
        fn name(&self) -> &str {
            "aggregate_probe"
        }

        fn init_state(
            &self,
            ctx: &RuleInitContext,
            _game: &TestGame,
        ) -> Result<ProbeState, RuleError> {
            Ok(ProbeState {
                action: (ctx.agent.0 % 2) as usize,
                observations: Vec::new(),
            })
        }

        fn decide(
            &self,
            state: &mut ProbeState,
            ctx: &DecisionContext<'_, TestGame, TestExperiment>,
            _rng: &mut SimRng,
        ) -> Result<usize, RuleError> {
            if state.observations.last().map(|row| row.t) != Some(ctx.t()) {
                let observation = match ctx.aggregate_history() {
                    Some(history) => AggregateObservation {
                        t: ctx.t(),
                        available: true,
                        names: history.names().to_vec(),
                        rows: history
                            .iter()
                            .map(|(t, values)| (t, values.to_vec()))
                            .collect(),
                        zero_column: history.column("zero_count").map(Iterator::collect),
                        last: history.last().map(|(t, values)| (t, values.to_vec())),
                        missing_column: history.column("missing").is_none(),
                    },
                    None => AggregateObservation {
                        t: ctx.t(),
                        ..AggregateObservation::default()
                    },
                };
                state.observations.push(observation);
            }
            Ok(state.action)
        }
    }

    type World = GameWorld<TestGame, TestExperiment>;

    fn rules() -> RuleBook<TestGame, TestExperiment> {
        let mut rules = RuleBook::new();
        rules.register("probe", Arc::new(AggregateProbe)).unwrap();
        rules
    }

    fn world(game: TestGame, n_agents: usize, t_max: u64, seed: u64) -> World {
        let rules = rules();
        build_world(
            game,
            n_agents,
            TestExperiment,
            InitialActions::Same(0),
            &rules,
            &Assignment::Ordered(vec![AssignGroup {
                kind: AssignKind::Rule("probe".to_owned()),
                count: n_agents,
            }]),
            8,
            t_max,
            seed,
        )
        .unwrap()
    }

    fn aggregate_simulation(history_len: usize, t_max: u64, seed: u64) -> Simulation<World> {
        assemble_with_aggregates(
            world(TestGame::new(), 3, t_max, seed),
            socsim_games::RoundRobin::new(),
            rules(),
            NormalFormOutcome,
            PlayOptions::new().decision_scope(DecisionScope::PerEncounter),
            seed,
            history_len,
        )
        .unwrap()
        .build()
    }

    fn agent_zero(simulation: &Simulation<World>) -> &ProbeState {
        simulation.world().policies().state(AgentId(0)).unwrap()
    }

    #[test]
    fn sums_means_and_public_view_match_hand_calculation() {
        let mut simulation = aggregate_simulation(4, 3, 41);
        simulation.step().unwrap();
        simulation.step().unwrap();

        let observations = &agent_zero(&simulation).observations;
        assert_eq!(observations[0].t, 1);
        assert!(observations[0].available);
        assert!(observations[0].rows.is_empty());
        assert!(observations[0].last.is_none());

        assert_eq!(observations[1].t, 2);
        assert_eq!(observations[1].names, ["zero_count", "payoff_sum"]);
        assert_eq!(observations[1].rows, [(1, vec![Some(4.0), Some(2.0)])]);
        assert_eq!(observations[1].zero_column, Some(vec![Some(4.0)]));
        assert_eq!(observations[1].last, observations[1].rows.last().cloned());
        assert!(observations[1].missing_column);
    }

    #[test]
    fn aggregate_ring_discards_old_rows_and_iterates_oldest_first() {
        let mut simulation = aggregate_simulation(2, 4, 43);
        for _ in 0..4 {
            simulation.step().unwrap();
        }

        let last = agent_zero(&simulation).observations.last().unwrap();
        assert_eq!(last.t, 4);
        assert_eq!(
            last.rows,
            [
                (2, vec![Some(4.0), Some(2.0)]),
                (3, vec![Some(4.0), Some(2.0)]),
            ]
        );
        assert_eq!(last.zero_column, Some(vec![Some(4.0), Some(4.0)]));
    }

    #[test]
    fn plain_assemble_exposes_no_aggregate_history_or_snapshot_field() {
        let seed = 47;
        let mut simulation = assemble(
            world(TestGame::new(), 2, 1, seed),
            socsim_games::FixedPlayers::pairs(&[(0, 1)]),
            rules(),
            NormalFormOutcome,
            PlayOptions::new(),
            seed,
        )
        .unwrap()
        .build();
        simulation.step().unwrap();

        let observation = &agent_zero(&simulation).observations[0];
        assert!(!observation.available);
        assert!(serde_json::to_value(simulation.world())
            .unwrap()
            .get("aggregate")
            .is_none());
    }

    #[test]
    fn zero_aggregate_history_length_is_rejected() {
        let seed = 53;
        let result = assemble_with_aggregates(
            world(TestGame::new(), 2, 1, seed),
            socsim_games::FixedPlayers::pairs(&[(0, 1)]),
            rules(),
            NormalFormOutcome,
            PlayOptions::new(),
            seed,
            0,
        );

        assert!(matches!(
            result,
            Err(GameError::InvalidAggregateHistoryLength)
        ));
    }

    #[test]
    fn aggregate_snapshot_restore_matches_uninterrupted_execution() {
        let mut uninterrupted = aggregate_simulation(3, 5, 59);
        let expected: Vec<_> = (0..5)
            .map(|_| {
                uninterrupted.step().unwrap();
                serde_json::to_string(uninterrupted.world()).unwrap()
            })
            .collect();

        let mut partial = aggregate_simulation(3, 5, 59);
        partial.step().unwrap();
        partial.step().unwrap();
        let encoded = serde_json::to_string(&partial.snapshot()).unwrap();
        let snapshot: Snapshot<World> = serde_json::from_str(&encoded).unwrap();

        let mut resumed = aggregate_simulation(3, 5, 999);
        resumed.restore(snapshot);
        let actual: Vec<_> = (0..3)
            .map(|_| {
                resumed.step().unwrap();
                serde_json::to_string(resumed.world()).unwrap()
            })
            .collect();

        assert_eq!(actual, expected[2..]);
    }

    #[derive(Clone, Copy, Debug)]
    struct DelayedPair;

    impl Matching<TestGame, TestExperiment> for DelayedPair {
        fn name(&self) -> &str {
            "delayed_pair"
        }

        fn check(
            &self,
            _ctx: &MatchContext<'_, TestGame, TestExperiment>,
        ) -> Result<(), GameError> {
            Ok(())
        }

        fn init_state(&self, _ctx: &MatchContext<'_, TestGame, TestExperiment>) -> MatchingState {
            MatchingState::None
        }

        fn encounters_into(
            &self,
            ctx: &MatchContext<'_, TestGame, TestExperiment>,
            _state: &mut MatchingState,
            _rng: &mut SimRng,
            out: &mut ArenaWriter<'_, usize>,
        ) -> socsim_core::Result<()> {
            if ctx.t() >= 2 {
                out.push(&[AgentId(0), AgentId(1)], Credit::All)?;
            }
            Ok(())
        }
    }

    #[test]
    fn empty_step_records_zero_for_sum_and_none_for_mean() {
        let seed = 61;
        let mut simulation = assemble_with_aggregates(
            world(TestGame::new(), 2, 2, seed),
            DelayedPair,
            rules(),
            NormalFormOutcome,
            PlayOptions::new(),
            seed,
            2,
        )
        .unwrap()
        .build();
        simulation.step().unwrap();
        simulation.step().unwrap();

        let observation = &agent_zero(&simulation).observations[0];
        assert_eq!(observation.t, 2);
        assert_eq!(observation.rows, [(1, vec![Some(0.0), None])]);
    }

    #[test]
    fn non_finite_aggregate_stops_without_writing_it_to_the_world() {
        let seed = 67;
        let mut simulation = assemble_with_aggregates(
            world(TestGame::non_finite(), 2, 1, seed),
            socsim_games::FixedPlayers::pairs(&[(0, 1)]),
            rules(),
            NormalFormOutcome,
            PlayOptions::new(),
            seed,
            2,
        )
        .unwrap()
        .build();

        let error = simulation.step().unwrap_err();
        assert!(error.to_string().contains("non-finite aggregate value"));
        let json = serde_json::to_value(simulation.world()).unwrap();
        assert_eq!(json["aggregate"]["filled"], 0);
        assert_eq!(json["aggregate"]["rows"], serde_json::json!([]));
    }
}
