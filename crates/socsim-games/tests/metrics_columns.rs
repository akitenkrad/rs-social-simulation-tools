use socsim_games::{metric_action_ids, MatrixGame, MetricScheme};

fn matrix_game(action_counts: Vec<usize>) -> MatrixGame {
    let profiles = action_counts.iter().product();
    MatrixGame::new(
        action_counts.clone(),
        vec![vec![0.0; profiles]; action_counts.len()],
    )
    .unwrap()
}

fn has_valid_slug_syntax(value: &str) -> bool {
    value.len() <= 64
        && value
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        && value.bytes().skip(1).all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
        })
}

#[test]
fn uppercase_symmetric_labels_use_pooled_index_ids() {
    let game = matrix_game(vec![2, 2])
        .with_labels(vec![vec!["C".into(), "D".into()]; 2])
        .unwrap();

    let ids = metric_action_ids(&game);

    assert_eq!(ids.scheme(), MetricScheme::Pooled);
    assert_eq!(
        ids.actions()
            .iter()
            .map(|action| action.id.as_str())
            .collect::<Vec<_>>(),
        ["a0", "a1"]
    );
    assert!(has_valid_slug_syntax("game.played_share.a0"));
}

#[test]
fn role_specific_labels_and_counts_use_role_then_action_order() {
    let game = matrix_game(vec![2, 3])
        .with_labels(vec![
            vec!["left-a".into(), "left-b".into()],
            vec!["right-a".into(), "right-b".into(), "right-c".into()],
        ])
        .unwrap();

    let ids = metric_action_ids(&game);

    assert_eq!(ids.scheme(), MetricScheme::PerRole);
    assert_eq!(
        ids.actions()
            .iter()
            .map(|action| action.id.as_str())
            .collect::<Vec<_>>(),
        ["r0-a0", "r0-a1", "r1-a0", "r1-a1", "r1-a2"]
    );
}

#[test]
fn labels_that_only_differ_by_case_do_not_collide() {
    let game = matrix_game(vec![2])
        .with_labels(vec![vec!["C".into(), "c".into()]])
        .unwrap();

    let ids = metric_action_ids(&game);

    assert_eq!(ids.actions()[0].id, "a0");
    assert_eq!(ids.actions()[1].id, "a1");
    assert_ne!(ids.actions()[0].id, ids.actions()[1].id);
}

#[test]
fn equal_unlabelled_action_sets_are_pooled() {
    let ids = metric_action_ids(&matrix_game(vec![3, 3]));

    assert_eq!(ids.scheme(), MetricScheme::Pooled);
    assert_eq!(ids.actions().len(), 3);
    assert!(ids.actions().iter().all(|action| action.role.is_none()));
}

#[test]
fn unequal_action_counts_are_per_role() {
    let ids = metric_action_ids(&matrix_game(vec![1, 2]));

    assert_eq!(ids.scheme(), MetricScheme::PerRole);
    assert_eq!(
        ids.actions()
            .iter()
            .map(|action| (action.role, action.action))
            .collect::<Vec<_>>(),
        [(Some(0), 0), (Some(1), 0), (Some(1), 1)]
    );
}

#[test]
fn id_looks_up_pooled_and_per_role_actions() {
    let pooled = metric_action_ids(&matrix_game(vec![2, 2]));
    assert_eq!(pooled.id(0, 1), Some("a1"));
    assert_eq!(pooled.id(1, 1), Some("a1"));
    assert_eq!(pooled.id(0, 2), None);

    let per_role = metric_action_ids(&matrix_game(vec![1, 2]));
    assert_eq!(per_role.id(0, 0), Some("r0-a0"));
    assert_eq!(per_role.id(1, 1), Some("r1-a1"));
    assert_eq!(per_role.id(0, 1), None);
    assert_eq!(per_role.id(2, 0), None);
}

#[test]
fn every_played_share_metric_name_has_valid_slug_syntax() {
    for game in [
        matrix_game(vec![2, 2])
            .with_labels(vec![vec!["C".into(), "D".into()]; 2])
            .unwrap(),
        matrix_game(vec![2, 3])
            .with_labels(vec![
                vec!["UPPER".into(), "lower".into()],
                vec!["x".into(), "y".into(), "z".into()],
            ])
            .unwrap(),
    ] {
        for action in metric_action_ids(&game).actions() {
            assert!(has_valid_slug_syntax(&format!(
                "game.played_share.{}",
                action.id
            )));
        }
    }
}

mod mechanism_metrics {
    use super::has_valid_slug_syntax;
    use std::any::Any;
    use std::borrow::Cow;
    use std::sync::Arc;

    use socsim_core::{AgentId, Recorder, SimRng, SocsimError};
    use socsim_engine::Simulation;
    use socsim_games::{
        assemble, assemble_with_aggregates, build_world, ActionError, AggregateGame,
        AggregateReduce, AggregateSpec, ArenaWriter, AssignGroup, AssignKind, Assignment,
        Automaton, AutomatonKind, DecisionContext, DecisionRule, DecisionScope, EquilibriumTarget,
        ExperimentState, FictitiousPlay, FiniteGame, Fixed, FixedPlayers, GameError, GameMetrics,
        GameWorld, InitialActions, LogitChoice, MatchContext, Matching, MatchingState, MatrixGame,
        MetricsOptions, Mixed, MixedSource, NormalFormGame, NormalFormOutcome, PayoffAggregation,
        PlayOptions, RothErev, RoundRobin, RuleBook, RuleError, RuleInitContext, StageGame,
        TitForTat, UniformRandom,
    };

    #[derive(Default)]
    struct MetricRecorder {
        metrics: Vec<(u64, String, f64)>,
    }

    impl Recorder for MetricRecorder {
        fn record_metric(&mut self, t: u64, key: &str, value: f64) {
            self.metrics.push((t, key.to_owned(), value));
        }

        fn record_event(&mut self, _t: u64, _kind: &str, _payload: serde_json::Value) {}

        fn record_row(&mut self, t: u64, table: &str, row: &[(&str, f64)]) {
            self.metrics.extend(
                row.iter()
                    .map(|(column, value)| (t, format!("{table}.{column}"), *value)),
            );
        }

        fn as_any(&self) -> Option<&dyn Any> {
            Some(self)
        }
    }

    fn prisoners_dilemma() -> MatrixGame {
        MatrixGame::bimatrix(
            &[vec![3.0, 0.0], vec![5.0, 1.0]],
            &[vec![3.0, 5.0], vec![0.0, 1.0]],
        )
        .unwrap()
        .with_labels(vec![vec!["C".into(), "D".into()]; 2])
        .unwrap()
    }

    fn held_assignment(n_agents: usize) -> Assignment {
        Assignment::Ordered(vec![AssignGroup {
            kind: AssignKind::Held,
            count: n_agents,
        }])
    }

    fn empty_rules<G: StageGame>() -> RuleBook<G, ()> {
        RuleBook::new()
    }

    fn metric_recorder<G, X>(simulation: &Simulation<GameWorld<G, X>>) -> &MetricRecorder
    where
        G: StageGame,
        X: ExperimentState,
    {
        simulation
            .recorder()
            .as_any()
            .unwrap()
            .downcast_ref::<MetricRecorder>()
            .unwrap()
    }

    fn metric_value<G, X>(
        simulation: &Simulation<GameWorld<G, X>>,
        t: u64,
        name: &str,
    ) -> Option<f64>
    where
        G: StageGame,
        X: ExperimentState,
    {
        metric_recorder(simulation)
            .metrics
            .iter()
            .find(|(recorded_t, recorded_name, _)| *recorded_t == t && recorded_name == name)
            .map(|(_, _, value)| *value)
    }

    fn held_pd_simulation(options: MetricsOptions) -> Simulation<GameWorld<MatrixGame>> {
        let game = prisoners_dilemma();
        let metrics = GameMetrics::new(&game, options).unwrap();
        let rules = empty_rules();
        let world = build_world(
            game,
            2,
            (),
            InitialActions::PerAgent(vec![0, 1]),
            &rules,
            &held_assignment(2),
            2,
            2,
            7,
        )
        .unwrap();
        assemble(
            world,
            FixedPlayers::pairs(&[(0, 1)]),
            rules,
            NormalFormOutcome,
            PlayOptions::new(),
            7,
        )
        .unwrap()
        .add_mechanism(Box::new(metrics))
        .recorder(Box::new(MetricRecorder::default()))
        .build()
    }

    #[test]
    fn records_pooled_played_share_and_population_payoff_statistics() {
        let mut simulation = held_pd_simulation(MetricsOptions::new());

        simulation.step().unwrap();

        assert_eq!(
            metric_value(&simulation, 1, "game.played_share.a0"),
            Some(0.5)
        );
        assert_eq!(
            metric_value(&simulation, 1, "game.played_share.a1"),
            Some(0.5)
        );
        assert_eq!(metric_value(&simulation, 1, "game.mean_payoff"), Some(2.5));
        let payoff_sd = metric_value(&simulation, 1, "game.payoff_sd").unwrap();
        assert!((payoff_sd - 2.5).abs() < 1e-12);
        assert!(metric_recorder(&simulation)
            .metrics
            .iter()
            .all(|(_, name, value)| has_valid_slug_syntax(name) && value.is_finite()));
    }

    #[test]
    fn cooperation_rate_is_opt_in_and_missing_labels_are_rejected() {
        let mut without = held_pd_simulation(MetricsOptions::new());
        without.step().unwrap();
        assert_eq!(metric_value(&without, 1, "game.coop_rate"), None);

        let mut with = held_pd_simulation(MetricsOptions::new().cooperate_label("C"));
        with.step().unwrap();
        assert_eq!(
            metric_value(&with, 1, "game.coop_rate"),
            metric_value(&with, 1, "game.played_share.a0")
        );

        let error = GameMetrics::new(
            &prisoners_dilemma(),
            MetricsOptions::new().cooperate_label("missing"),
        )
        .unwrap_err();
        assert!(matches!(error, GameError::Metrics(_)));
    }

    struct RoleAction {
        actions: Vec<usize>,
    }

    impl DecisionRule<MatrixGame, ()> for RoleAction {
        fn name(&self) -> &str {
            "role_action"
        }

        fn init_state(
            &self,
            _ctx: &RuleInitContext,
            _game: &MatrixGame,
        ) -> Result<<() as ExperimentState>::PolicyState, RuleError> {
            Ok(socsim_games::BuiltinPolicyState::Stateless(
                socsim_games::Stateless,
            ))
        }

        fn decide(
            &self,
            _state: &mut <() as ExperimentState>::PolicyState,
            ctx: &DecisionContext<'_, MatrixGame, ()>,
            _rng: &mut SimRng,
        ) -> Result<usize, RuleError> {
            Ok(self.actions[ctx.role().unwrap()])
        }
    }

    #[test]
    fn records_role_specific_shares_for_a_two_by_three_game() {
        let game = MatrixGame::new(vec![2, 3], vec![vec![0.0; 6], vec![0.0; 6]])
            .unwrap()
            .with_labels(vec![
                vec!["left-a".into(), "left-b".into()],
                vec!["right-a".into(), "right-b".into(), "right-c".into()],
            ])
            .unwrap();
        let metrics = GameMetrics::new(&game, MetricsOptions::new()).unwrap();
        let mut rules = RuleBook::new();
        rules
            .register(
                "role_action",
                Arc::new(RoleAction {
                    actions: vec![0, 2],
                }),
            )
            .unwrap();
        let world = build_world(
            game,
            2,
            (),
            InitialActions::Same(0),
            &rules,
            &Assignment::Ordered(vec![AssignGroup {
                kind: AssignKind::Rule("role_action".into()),
                count: 2,
            }]),
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
            PlayOptions::new().decision_scope(DecisionScope::PerAgentRole),
            11,
        )
        .unwrap()
        .add_mechanism(Box::new(metrics))
        .recorder(Box::new(MetricRecorder::default()))
        .build();

        simulation.step().unwrap();

        for (name, expected) in [
            ("game.played_share.r0-a0", 1.0),
            ("game.played_share.r0-a1", 0.0),
            ("game.played_share.r1-a0", 0.0),
            ("game.played_share.r1-a1", 0.0),
            ("game.played_share.r1-a2", 1.0),
        ] {
            assert_eq!(metric_value(&simulation, 1, name), Some(expected));
            assert!(has_valid_slug_syntax(name));
        }
    }

    #[test]
    fn round_robin_per_encounter_counts_every_arena_seat() {
        let game = prisoners_dilemma();
        let metrics = GameMetrics::new(&game, MetricsOptions::new()).unwrap();
        let rules = empty_rules();
        let world = build_world(
            game,
            3,
            (),
            InitialActions::PerAgent(vec![0, 1, 0]),
            &rules,
            &held_assignment(3),
            1,
            1,
            13,
        )
        .unwrap();
        let mut simulation = assemble(
            world,
            RoundRobin::new(),
            rules,
            NormalFormOutcome,
            PlayOptions::new().decision_scope(DecisionScope::PerEncounter),
            13,
        )
        .unwrap()
        .add_mechanism(Box::new(metrics))
        .recorder(Box::new(MetricRecorder::default()))
        .build();

        simulation.step().unwrap();

        assert_eq!(
            metric_value(&simulation, 1, "game.played_share.a0"),
            Some(4.0 / 6.0)
        );
        assert_eq!(
            metric_value(&simulation, 1, "game.played_share.a1"),
            Some(2.0 / 6.0)
        );
    }

    #[derive(Clone, Debug)]
    struct AggregateState {
        action: usize,
        observed: Option<f64>,
    }

    #[derive(Debug)]
    struct AggregateExperiment;

    impl ExperimentState for AggregateExperiment {
        type PolicyState = AggregateState;
        type View<'a> = ();

        fn view_for(&self, _me: AgentId) -> Self::View<'_> {}
    }

    #[derive(Debug)]
    struct CountGame {
        specs: Vec<AggregateSpec>,
    }

    impl CountGame {
        fn new() -> Self {
            Self {
                specs: vec![AggregateSpec::new("zero_count", AggregateReduce::Sum).unwrap()],
            }
        }
    }

    impl StageGame for CountGame {
        type Action = usize;

        fn n_roles(&self) -> usize {
            2
        }

        fn check_action(&self, role: usize, action: &usize) -> Result<(), ActionError> {
            if role >= 2 {
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

    impl NormalFormGame for CountGame {
        fn payoff(&self, role: usize, profile: &[usize]) -> f64 {
            profile[role] as f64
        }
    }

    impl FiniteGame for CountGame {
        fn n_actions(&self, _role: usize) -> usize {
            2
        }

        fn action_label(&self, _role: usize, action: usize) -> Cow<'_, str> {
            Cow::Owned(format!("a{action}"))
        }
    }

    impl AggregateGame for CountGame {
        fn aggregate_spec(&self) -> &[AggregateSpec] {
            &self.specs
        }

        fn aggregate_into(&self, profile: &[usize], out: &mut [f64]) {
            out[0] = profile.iter().filter(|&&action| action == 0).count() as f64;
        }
    }

    struct AggregateProbe;

    impl DecisionRule<CountGame, AggregateExperiment> for AggregateProbe {
        fn name(&self) -> &str {
            "aggregate_probe"
        }

        fn init_state(
            &self,
            ctx: &RuleInitContext,
            _game: &CountGame,
        ) -> Result<AggregateState, RuleError> {
            Ok(AggregateState {
                action: ctx.agent.0 as usize,
                observed: None,
            })
        }

        fn decide(
            &self,
            state: &mut AggregateState,
            ctx: &DecisionContext<'_, CountGame, AggregateExperiment>,
            _rng: &mut SimRng,
        ) -> Result<usize, RuleError> {
            state.observed = ctx
                .aggregate_history()
                .and_then(|history| history.last())
                .and_then(|(_, values)| values[0]);
            Ok(state.action)
        }
    }

    fn aggregate_rules() -> RuleBook<CountGame, AggregateExperiment> {
        let mut rules = RuleBook::new();
        rules
            .register("aggregate_probe", Arc::new(AggregateProbe))
            .unwrap();
        rules
    }

    fn aggregate_world(
        game: CountGame,
        rules: &RuleBook<CountGame, AggregateExperiment>,
    ) -> GameWorld<CountGame, AggregateExperiment> {
        build_world(
            game,
            2,
            AggregateExperiment,
            InitialActions::Same(0),
            rules,
            &Assignment::Ordered(vec![AssignGroup {
                kind: AssignKind::Rule("aggregate_probe".into()),
                count: 2,
            }]),
            2,
            2,
            17,
        )
        .unwrap()
    }

    #[test]
    fn aggregate_metrics_match_the_public_aggregate_view_and_plain_assemble_omits_them() {
        let game = CountGame::new();
        let metrics = GameMetrics::new(&game, MetricsOptions::new()).unwrap();
        let rules = aggregate_rules();
        let mut with_aggregates = assemble_with_aggregates(
            aggregate_world(game, &rules),
            FixedPlayers::pairs(&[(0, 1)]),
            rules,
            NormalFormOutcome,
            PlayOptions::new(),
            17,
            2,
        )
        .unwrap()
        .add_mechanism(Box::new(metrics))
        .recorder(Box::new(MetricRecorder::default()))
        .build();
        with_aggregates.step().unwrap();
        with_aggregates.step().unwrap();

        let view_value = with_aggregates
            .world()
            .policies()
            .state(AgentId(0))
            .unwrap()
            .observed
            .unwrap();
        assert_eq!(
            metric_value(&with_aggregates, 1, "game.aggregate.zero_count"),
            Some(view_value)
        );

        let plain_game = CountGame::new();
        let plain_metrics = GameMetrics::new(&plain_game, MetricsOptions::new()).unwrap();
        let plain_rules = aggregate_rules();
        let mut plain = assemble(
            aggregate_world(plain_game, &plain_rules),
            FixedPlayers::pairs(&[(0, 1)]),
            plain_rules,
            NormalFormOutcome,
            PlayOptions::new(),
            17,
        )
        .unwrap()
        .add_mechanism(Box::new(plain_metrics))
        .recorder(Box::new(MetricRecorder::default()))
        .build();
        plain.step().unwrap();
        assert!(metric_recorder(&plain)
            .metrics
            .iter()
            .all(|(_, name, _)| !name.starts_with("game.aggregate.")));
    }

    #[derive(Clone, Copy, Debug)]
    struct EmptyMatching;

    impl Matching<MatrixGame, ()> for EmptyMatching {
        fn name(&self) -> &str {
            "empty"
        }

        fn check(&self, _ctx: &MatchContext<'_, MatrixGame, ()>) -> Result<(), GameError> {
            Ok(())
        }

        fn encounters_into(
            &self,
            _ctx: &MatchContext<'_, MatrixGame, ()>,
            _state: &mut MatchingState,
            _rng: &mut SimRng,
            _out: &mut ArenaWriter<'_, usize>,
        ) -> socsim_core::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn undefined_columns_are_omitted_on_an_empty_step() {
        let game = prisoners_dilemma();
        let metrics = GameMetrics::new(&game, MetricsOptions::new().cooperate_label("C")).unwrap();
        let rules = empty_rules();
        let world = build_world(
            game,
            2,
            (),
            InitialActions::Same(0),
            &rules,
            &held_assignment(2),
            1,
            1,
            19,
        )
        .unwrap();
        let mut simulation = assemble(
            world,
            EmptyMatching,
            rules,
            NormalFormOutcome,
            PlayOptions::new(),
            19,
        )
        .unwrap()
        .add_mechanism(Box::new(metrics))
        .recorder(Box::new(MetricRecorder::default()))
        .build();

        simulation.step().unwrap();

        assert!(metric_recorder(&simulation).metrics.is_empty());
    }

    #[test]
    fn describe_lists_enabled_column_patterns_and_the_naming_scheme() {
        let game = prisoners_dilemma();
        let without = GameMetrics::new(&game, MetricsOptions::new()).unwrap();
        let with = GameMetrics::new(&game, MetricsOptions::new().cooperate_label("C")).unwrap();

        let without = without.describe();
        let with = with.describe();
        for expected in [
            "game.played_share.{action}",
            "game.mean_payoff",
            "game.payoff_sd",
            "game.aggregate.{name}",
        ] {
            assert!(without.iter().any(|(name, _)| name == expected));
        }
        assert!(!without.iter().any(|(name, _)| name == "game.coop_rate"));
        assert!(with.iter().any(|(name, _)| name == "game.coop_rate"));
        let played_share = with
            .iter()
            .find(|(name, _)| name == "game.played_share.{action}")
            .unwrap();
        assert!(played_share.1.contains("metric_id"));
        assert!(played_share.1.contains("pooled"));
    }

    #[test]
    fn adding_metrics_does_not_change_the_world_trajectory() {
        fn simulation(with_metrics: bool) -> Simulation<GameWorld<MatrixGame>> {
            let game = prisoners_dilemma();
            let metrics = GameMetrics::new(&game, MetricsOptions::new()).unwrap();
            let rules = empty_rules();
            let world = build_world(
                game,
                2,
                (),
                InitialActions::PerAgent(vec![0, 1]),
                &rules,
                &held_assignment(2),
                3,
                3,
                23,
            )
            .unwrap();
            let builder = assemble(
                world,
                FixedPlayers::pairs(&[(0, 1)]),
                rules,
                NormalFormOutcome,
                PlayOptions::new().payoff_aggregation(PayoffAggregation::Mean),
                23,
            )
            .unwrap();
            if with_metrics {
                builder.add_mechanism(Box::new(metrics)).build()
            } else {
                builder.build()
            }
        }

        let mut baseline = simulation(false);
        let mut instrumented = simulation(true);
        for _ in 0..3 {
            baseline.step().unwrap();
            instrumented.step().unwrap();
            assert_eq!(
                serde_json::to_value(baseline.world()).unwrap(),
                serde_json::to_value(instrumented.world()).unwrap()
            );
        }
    }

    #[test]
    fn applying_metrics_to_a_different_game_shape_returns_a_mechanism_error() {
        let metrics = GameMetrics::new(&prisoners_dilemma(), MetricsOptions::new()).unwrap();
        let game = MatrixGame::new(vec![3, 3], vec![vec![0.0; 9], vec![0.0; 9]]).unwrap();
        let rules = empty_rules();
        let world = build_world(
            game,
            2,
            (),
            InitialActions::Same(0),
            &rules,
            &held_assignment(2),
            1,
            1,
            29,
        )
        .unwrap();
        let mut simulation = assemble(
            world,
            FixedPlayers::pairs(&[(0, 1)]),
            rules,
            NormalFormOutcome,
            PlayOptions::new(),
            29,
        )
        .unwrap()
        .add_mechanism(Box::new(metrics))
        .build();

        let error = simulation.step().unwrap_err();
        assert!(matches!(error, SocsimError::Mechanism(_)));
    }

    #[test]
    fn non_finite_metric_calculation_stops_before_recording_a_partial_row() {
        let game = MatrixGame::new(vec![1, 1], vec![vec![1.0e308], vec![1.0e308]]).unwrap();
        let metrics = GameMetrics::new(&game, MetricsOptions::new()).unwrap();
        let rules = empty_rules();
        let world = build_world(
            game,
            2,
            (),
            InitialActions::Same(0),
            &rules,
            &held_assignment(2),
            1,
            1,
            31,
        )
        .unwrap();
        let mut simulation = assemble(
            world,
            FixedPlayers::pairs(&[(0, 1)]),
            rules,
            NormalFormOutcome,
            PlayOptions::new(),
            31,
        )
        .unwrap()
        .add_mechanism(Box::new(metrics))
        .recorder(Box::new(MetricRecorder::default()))
        .build();

        let error = simulation.step().unwrap_err();
        assert!(matches!(error, SocsimError::Mechanism(_)));
        assert!(metric_recorder(&simulation).metrics.is_empty());
        assert!(simulation
            .world()
            .payoffs()
            .iter()
            .all(|value| value.is_finite()));
    }

    fn matching_pennies() -> MatrixGame {
        MatrixGame::zero_sum(&[vec![1.0, -1.0], vec![-1.0, 1.0]]).unwrap()
    }

    fn constant_game(payoff: f64) -> MatrixGame {
        MatrixGame::new(vec![2, 2], vec![vec![payoff; 4], vec![payoff; 4]]).unwrap()
    }

    fn assert_close(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 1e-12,
            "expected {expected}, got {actual}"
        );
    }

    #[test]
    fn played_regret_and_equilibrium_distance_match_matching_pennies_by_hand() {
        let game = matching_pennies();
        let metrics = GameMetrics::new(
            &game,
            MetricsOptions::new().equilibrium(EquilibriumTarget::Solve),
        )
        .unwrap();
        let rules = empty_rules();
        let world = build_world(
            game,
            2,
            (),
            InitialActions::Same(0),
            &rules,
            &held_assignment(2),
            3,
            3,
            37,
        )
        .unwrap();
        let mut simulation = assemble(
            world,
            FixedPlayers::pairs(&[(0, 1)]),
            rules,
            NormalFormOutcome,
            PlayOptions::new(),
            37,
        )
        .unwrap()
        .add_mechanism(Box::new(metrics))
        .recorder(Box::new(MetricRecorder::default()))
        .build();

        for t in 1..=3 {
            simulation.step().unwrap();
            assert_close(metric_value(&simulation, t, "game.regret").unwrap(), 2.0);
            assert_close(
                metric_value(&simulation, t, "game.eq_distance").unwrap(),
                2.0,
            );
        }
    }

    #[derive(Clone, Debug)]
    struct RepeatedPairs(Vec<[AgentId; 2]>);

    impl Matching<MatrixGame, ()> for RepeatedPairs {
        fn name(&self) -> &str {
            "repeated_pairs"
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
            for pair in &self.0 {
                out.push(pair, socsim_games::Credit::All)?;
            }
            Ok(())
        }
    }

    #[test]
    fn reported_regret_uses_seat_weighted_average() {
        let game = matching_pennies();
        let metrics = GameMetrics::new(
            &game,
            MetricsOptions::new().mixed_source(MixedSource::Reported),
        )
        .unwrap();
        let mut rules = RuleBook::new();
        rules.register("uniform", Arc::new(UniformRandom)).unwrap();
        rules
            .register("pure", Arc::new(Mixed::new(vec![1.0, 0.0])))
            .unwrap();
        rules
            .register("biased", Arc::new(Mixed::new(vec![0.75, 0.25])))
            .unwrap();
        let world = build_world(
            game,
            4,
            (),
            InitialActions::Same(0),
            &rules,
            &Assignment::Ordered(vec![
                AssignGroup {
                    kind: AssignKind::Rule("uniform".into()),
                    count: 1,
                },
                AssignGroup {
                    kind: AssignKind::Rule("pure".into()),
                    count: 2,
                },
                AssignGroup {
                    kind: AssignKind::Rule("biased".into()),
                    count: 1,
                },
            ]),
            1,
            1,
            41,
        )
        .unwrap();
        let mut simulation = assemble(
            world,
            RepeatedPairs(vec![
                [AgentId(0), AgentId(3)],
                [AgentId(1), AgentId(3)],
                [AgentId(2), AgentId(3)],
            ]),
            rules,
            NormalFormOutcome,
            PlayOptions::new().report_mixed(true),
            41,
        )
        .unwrap()
        .add_mechanism(Box::new(metrics))
        .recorder(Box::new(MetricRecorder::default()))
        .build();

        simulation.step().unwrap();

        assert_close(metric_value(&simulation, 1, "game.regret").unwrap(), 1.0);
    }

    #[test]
    fn solve_rejects_incomplete_and_non_two_player_equilibrium_sets() {
        let degenerate = MatrixGame::bimatrix(
            &[vec![1.0, 0.0], vec![1.0, 0.0]],
            &[vec![1.0, 0.0], vec![1.0, 0.0]],
        )
        .unwrap();
        assert!(matches!(
            GameMetrics::new(
                &degenerate,
                MetricsOptions::new().equilibrium(EquilibriumTarget::Solve)
            ),
            Err(GameError::Metrics(_))
        ));

        let three_player = MatrixGame::new(vec![2, 2, 2], vec![vec![0.0; 8]; 3]).unwrap();
        assert!(matches!(
            GameMetrics::new(
                &three_player,
                MetricsOptions::new().equilibrium(EquilibriumTarget::Solve)
            ),
            Err(GameError::Metrics(_))
        ));
    }

    fn reported_pair(
        game: MatrixGame,
        rules: RuleBook<MatrixGame, ()>,
        assignment: Assignment,
        initial: InitialActions<usize>,
        seed: u64,
    ) -> Simulation<GameWorld<MatrixGame>> {
        let world = build_world(game, 2, (), initial, &rules, &assignment, 1, 1, seed).unwrap();
        assemble(
            world,
            FixedPlayers::pairs(&[(0, 1)]),
            rules,
            NormalFormOutcome,
            PlayOptions::new().report_mixed(true),
            seed,
        )
        .unwrap()
        .build()
    }

    #[test]
    fn stateless_builtin_rules_report_their_mixed_strategies() {
        let mut rules = RuleBook::new();
        rules.register("uniform", Arc::new(UniformRandom)).unwrap();
        rules
            .register("mixed", Arc::new(Mixed::new(vec![0.25, 0.75])))
            .unwrap();
        let mut simulation = reported_pair(
            matching_pennies(),
            rules,
            Assignment::Ordered(vec![
                AssignGroup {
                    kind: AssignKind::Rule("uniform".into()),
                    count: 1,
                },
                AssignGroup {
                    kind: AssignKind::Rule("mixed".into()),
                    count: 1,
                },
            ]),
            InitialActions::Same(0),
            43,
        );

        simulation.step().unwrap();

        assert_eq!(
            simulation.world().reported_mixed(AgentId(0), 0),
            Some([0.5, 0.5].as_slice())
        );
        assert_eq!(
            simulation.world().reported_mixed(AgentId(0), 1),
            Some([0.5, 0.5].as_slice())
        );
        assert_eq!(
            simulation.world().reported_mixed(AgentId(1), 0),
            Some([0.25, 0.75].as_slice())
        );
    }

    #[test]
    fn history_and_belief_rules_report_after_learning() {
        let mut tft_rules = RuleBook::new();
        tft_rules
            .register("tft", Arc::new(TitForTat::new(0)))
            .unwrap();
        let mut tft = reported_pair(
            prisoners_dilemma(),
            tft_rules,
            Assignment::Ordered(vec![
                AssignGroup {
                    kind: AssignKind::Rule("tft".into()),
                    count: 1,
                },
                AssignGroup {
                    kind: AssignKind::Held,
                    count: 1,
                },
            ]),
            InitialActions::PerAgent(vec![0, 1]),
            47,
        );
        tft.step().unwrap();
        assert_eq!(
            tft.world().reported_mixed(AgentId(0), 0),
            Some([0.0, 1.0].as_slice())
        );

        let game = constant_game(0.0);
        let mut belief_rules = RuleBook::new();
        belief_rules
            .register("lowest", Arc::new(FictitiousPlay::new().tie_break_lowest()))
            .unwrap();
        belief_rules
            .register("random", Arc::new(FictitiousPlay::new().tie_break_random()))
            .unwrap();
        let mut beliefs = reported_pair(
            game,
            belief_rules,
            Assignment::Ordered(vec![
                AssignGroup {
                    kind: AssignKind::Rule("lowest".into()),
                    count: 1,
                },
                AssignGroup {
                    kind: AssignKind::Rule("random".into()),
                    count: 1,
                },
            ]),
            InitialActions::Same(0),
            53,
        );
        beliefs.step().unwrap();
        assert_eq!(
            beliefs.world().reported_mixed(AgentId(0), 0),
            Some([1.0, 0.0].as_slice())
        );
        assert_eq!(
            beliefs.world().reported_mixed(AgentId(1), 1),
            Some([0.5, 0.5].as_slice())
        );
    }

    #[test]
    fn probabilistic_learning_rules_report_current_selection_probabilities() {
        let mut rules = RuleBook::new();
        rules
            .register("logit", Arc::new(LogitChoice::new(0.0)))
            .unwrap();
        rules
            .register("roth", Arc::new(RothErev::new(0.1, 0.5, 1.0)))
            .unwrap();
        let mut simulation = reported_pair(
            constant_game(1.0),
            rules,
            Assignment::Ordered(vec![
                AssignGroup {
                    kind: AssignKind::Rule("logit".into()),
                    count: 1,
                },
                AssignGroup {
                    kind: AssignKind::Rule("roth".into()),
                    count: 1,
                },
            ]),
            InitialActions::Same(0),
            59,
        );

        simulation.step().unwrap();

        assert_eq!(
            simulation.world().reported_mixed(AgentId(0), 0),
            Some([0.5, 0.5].as_slice())
        );
        assert_eq!(
            simulation.world().reported_mixed(AgentId(1), 1),
            Some([0.5, 0.5].as_slice())
        );
    }

    #[test]
    fn automata_report_next_action_probabilities_including_gtft() {
        let mut rules = RuleBook::new();
        rules
            .register(
                "gtft",
                Arc::new(Automaton::new(AutomatonKind::Gtft { generosity: 0.25 }, 0)),
            )
            .unwrap();
        rules
            .register("all_d", Arc::new(Automaton::new(AutomatonKind::AllD, 0)))
            .unwrap();
        let mut simulation = reported_pair(
            prisoners_dilemma(),
            rules,
            Assignment::Ordered(vec![
                AssignGroup {
                    kind: AssignKind::Rule("gtft".into()),
                    count: 1,
                },
                AssignGroup {
                    kind: AssignKind::Rule("all_d".into()),
                    count: 1,
                },
            ]),
            InitialActions::Same(0),
            61,
        );

        simulation.step().unwrap();

        assert_eq!(
            simulation.world().reported_mixed(AgentId(0), 0),
            Some([0.25, 0.75].as_slice())
        );
        assert_eq!(
            simulation.world().reported_mixed(AgentId(1), 1),
            Some([0.0, 1.0].as_slice())
        );
    }

    #[test]
    fn global_learning_state_dimension_mismatch_reports_none() {
        let game = MatrixGame::new(vec![2, 3], vec![vec![0.0; 6], vec![0.0; 6]]).unwrap();
        let mut rules = RuleBook::new();
        rules
            .register("fictitious", Arc::new(FictitiousPlay::new()))
            .unwrap();
        let mut simulation = reported_pair(
            game,
            rules,
            Assignment::Ordered(vec![
                AssignGroup {
                    kind: AssignKind::Rule("fictitious".into()),
                    count: 1,
                },
                AssignGroup {
                    kind: AssignKind::Held,
                    count: 1,
                },
            ]),
            InitialActions::Same(0),
            67,
        );

        simulation.step().unwrap();

        assert!(simulation.world().reported_mixed(AgentId(0), 0).is_some());
        assert_eq!(simulation.world().reported_mixed(AgentId(0), 1), None);
    }

    #[test]
    fn reported_source_accepts_held_seats_as_pure_strategies() {
        let game = matching_pennies();
        let metrics = GameMetrics::new(
            &game,
            MetricsOptions::new().mixed_source(MixedSource::Reported),
        )
        .unwrap();
        let mut rules = RuleBook::new();
        rules.register("uniform", Arc::new(UniformRandom)).unwrap();
        let world = build_world(
            game,
            2,
            (),
            InitialActions::Same(0),
            &rules,
            &Assignment::Ordered(vec![
                AssignGroup {
                    kind: AssignKind::Held,
                    count: 1,
                },
                AssignGroup {
                    kind: AssignKind::Rule("uniform".into()),
                    count: 1,
                },
            ]),
            1,
            1,
            71,
        )
        .unwrap();
        let mut simulation = assemble(
            world,
            FixedPlayers::pairs(&[(0, 1)]),
            rules,
            NormalFormOutcome,
            PlayOptions::new().report_mixed(true),
            71,
        )
        .unwrap()
        .add_mechanism(Box::new(metrics))
        .recorder(Box::new(MetricRecorder::default()))
        .build();

        simulation.step().unwrap();

        assert_close(metric_value(&simulation, 1, "game.regret").unwrap(), 1.0);
    }

    struct ReportRule {
        values: Vec<f64>,
    }

    impl DecisionRule<MatrixGame, ()> for ReportRule {
        fn name(&self) -> &str {
            "report_rule"
        }

        fn init_state(
            &self,
            _ctx: &RuleInitContext,
            _game: &MatrixGame,
        ) -> Result<<() as ExperimentState>::PolicyState, RuleError> {
            Ok(socsim_games::BuiltinPolicyState::Stateless(
                socsim_games::Stateless,
            ))
        }

        fn decide(
            &self,
            _state: &mut <() as ExperimentState>::PolicyState,
            _ctx: &DecisionContext<'_, MatrixGame, ()>,
            _rng: &mut SimRng,
        ) -> Result<usize, RuleError> {
            Ok(0)
        }

        fn mixed(
            &self,
            _state: &<() as ExperimentState>::PolicyState,
            _role: usize,
            _game: &MatrixGame,
        ) -> Option<Vec<f64>> {
            Some(self.values.clone())
        }
    }

    #[test]
    fn non_finite_reports_are_not_stored_and_wrong_dimensions_are_rejected_by_metrics() {
        let mut nan_rules = RuleBook::new();
        nan_rules
            .register(
                "bad",
                Arc::new(ReportRule {
                    values: vec![f64::NAN, 0.0],
                }),
            )
            .unwrap();
        let mut nan = reported_pair(
            matching_pennies(),
            nan_rules,
            Assignment::Ordered(vec![
                AssignGroup {
                    kind: AssignKind::Rule("bad".into()),
                    count: 1,
                },
                AssignGroup {
                    kind: AssignKind::Held,
                    count: 1,
                },
            ]),
            InitialActions::Same(0),
            72,
        );
        nan.step().unwrap();
        assert_eq!(nan.world().reported_mixed(AgentId(0), 0), None);
        serde_json::to_value(nan.world()).unwrap();

        let game = matching_pennies();
        let metrics = GameMetrics::new(
            &game,
            MetricsOptions::new().mixed_source(MixedSource::Reported),
        )
        .unwrap();
        let mut short_rules = RuleBook::new();
        short_rules
            .register("bad", Arc::new(ReportRule { values: vec![1.0] }))
            .unwrap();
        let world = build_world(
            game,
            2,
            (),
            InitialActions::Same(0),
            &short_rules,
            &Assignment::Ordered(vec![
                AssignGroup {
                    kind: AssignKind::Rule("bad".into()),
                    count: 1,
                },
                AssignGroup {
                    kind: AssignKind::Held,
                    count: 1,
                },
            ]),
            1,
            1,
            74,
        )
        .unwrap();
        let mut short = assemble(
            world,
            FixedPlayers::pairs(&[(0, 1)]),
            short_rules,
            NormalFormOutcome,
            PlayOptions::new().report_mixed(true),
            74,
        )
        .unwrap()
        .add_mechanism(Box::new(metrics))
        .build();
        assert!(matches!(short.step(), Err(SocsimError::Mechanism(_))));
    }

    #[test]
    fn invalid_reported_source_combinations_fail_at_the_required_boundary() {
        let rules = empty_rules();
        let world = build_world(
            matching_pennies(),
            2,
            (),
            InitialActions::Same(0),
            &rules,
            &held_assignment(2),
            1,
            1,
            73,
        )
        .unwrap();
        assert!(assemble(
            world,
            FixedPlayers::pairs(&[(0, 1)]),
            rules,
            NormalFormOutcome,
            PlayOptions::new()
                .decision_scope(DecisionScope::PerEncounter)
                .report_mixed(true),
            73,
        )
        .is_err());

        let game = matching_pennies();
        let metrics = GameMetrics::new(
            &game,
            MetricsOptions::new().mixed_source(MixedSource::Reported),
        )
        .unwrap();
        let rules = empty_rules();
        let world = build_world(
            game,
            2,
            (),
            InitialActions::Same(0),
            &rules,
            &held_assignment(2),
            1,
            1,
            79,
        )
        .unwrap();
        let mut missing_reports = assemble(
            world,
            FixedPlayers::pairs(&[(0, 1)]),
            rules,
            NormalFormOutcome,
            PlayOptions::new(),
            79,
        )
        .unwrap()
        .add_mechanism(Box::new(metrics))
        .build();
        assert!(matches!(
            missing_reports.step(),
            Err(SocsimError::Mechanism(_))
        ));

        let game = matching_pennies();
        let metrics = GameMetrics::new(
            &game,
            MetricsOptions::new().mixed_source(MixedSource::Reported),
        )
        .unwrap();
        let mut fixed_rules = RuleBook::new();
        fixed_rules
            .register("fixed", Arc::new(Fixed::new(0)))
            .unwrap();
        let world = build_world(
            game,
            2,
            (),
            InitialActions::Same(0),
            &fixed_rules,
            &Assignment::Ordered(vec![
                AssignGroup {
                    kind: AssignKind::Rule("fixed".into()),
                    count: 1,
                },
                AssignGroup {
                    kind: AssignKind::Held,
                    count: 1,
                },
            ]),
            1,
            1,
            83,
        )
        .unwrap();
        let mut missing_value = assemble(
            world,
            FixedPlayers::pairs(&[(0, 1)]),
            fixed_rules,
            NormalFormOutcome,
            PlayOptions::new().report_mixed(true),
            83,
        )
        .unwrap()
        .add_mechanism(Box::new(metrics))
        .build();
        assert!(matches!(
            missing_value.step(),
            Err(SocsimError::Mechanism(_))
        ));
    }

    #[test]
    fn reporting_and_metrics_do_not_change_the_model_trajectory() {
        fn simulation(report: bool, metrics: bool) -> Simulation<GameWorld<MatrixGame>> {
            let game = matching_pennies();
            let mechanism = GameMetrics::new(&game, MetricsOptions::new()).unwrap();
            let mut rules = RuleBook::new();
            rules.register("uniform", Arc::new(UniformRandom)).unwrap();
            let world = build_world(
                game,
                2,
                (),
                InitialActions::Same(0),
                &rules,
                &Assignment::Ordered(vec![AssignGroup {
                    kind: AssignKind::Rule("uniform".into()),
                    count: 2,
                }]),
                3,
                3,
                89,
            )
            .unwrap();
            let builder = assemble(
                world,
                FixedPlayers::pairs(&[(0, 1)]),
                rules,
                NormalFormOutcome,
                PlayOptions::new().report_mixed(report),
                89,
            )
            .unwrap();
            if metrics {
                builder.add_mechanism(Box::new(mechanism)).build()
            } else {
                builder.build()
            }
        }

        fn without_reports<G, X>(world: &GameWorld<G, X>) -> serde_json::Value
        where
            G: StageGame + serde::Serialize,
            G::Action: serde::Serialize,
            X: ExperimentState + serde::Serialize,
            X::PolicyState: serde::Serialize,
        {
            let mut value = serde_json::to_value(world).unwrap();
            value.as_object_mut().unwrap().remove("reports");
            value
        }

        let mut baseline = simulation(false, false);
        let mut reported = simulation(true, false);
        let mut instrumented = simulation(true, true);
        for _ in 0..3 {
            baseline.step().unwrap();
            reported.step().unwrap();
            instrumented.step().unwrap();
            assert_eq!(
                without_reports(baseline.world()),
                without_reports(reported.world())
            );
            assert_eq!(
                without_reports(baseline.world()),
                without_reports(instrumented.world())
            );
            assert!(serde_json::to_value(baseline.world())
                .unwrap()
                .get("reports")
                .is_none());
        }
    }

    #[test]
    fn describe_lists_only_enabled_strategy_metrics() {
        let game = matching_pennies();
        let default = GameMetrics::new(&game, MetricsOptions::new())
            .unwrap()
            .describe();
        let disabled = GameMetrics::new(&game, MetricsOptions::new().regret(false))
            .unwrap()
            .describe();
        let solved = GameMetrics::new(
            &game,
            MetricsOptions::new().equilibrium(EquilibriumTarget::Solve),
        )
        .unwrap()
        .describe();
        assert!(default.iter().any(|(name, _)| name == "game.regret"));
        assert!(!disabled.iter().any(|(name, _)| name == "game.regret"));
        assert!(!default.iter().any(|(name, _)| name == "game.eq_distance"));
        assert!(solved.iter().any(|(name, _)| name == "game.eq_distance"));

        let three_player = MatrixGame::new(vec![2, 2, 2], vec![vec![0.0; 8]; 3]).unwrap();
        let automatic = GameMetrics::new(&three_player, MetricsOptions::new())
            .unwrap()
            .describe();
        let explicit = GameMetrics::new(&three_player, MetricsOptions::new().regret(true))
            .unwrap()
            .describe();
        assert!(!automatic.iter().any(|(name, _)| name == "game.regret"));
        assert!(explicit.iter().any(|(name, _)| name == "game.regret"));
    }

    struct CumulativeSchedule;

    impl DecisionRule<MatrixGame, ()> for CumulativeSchedule {
        fn name(&self) -> &str {
            "cumulative_schedule"
        }

        fn init_state(
            &self,
            _ctx: &RuleInitContext,
            _game: &MatrixGame,
        ) -> Result<<() as ExperimentState>::PolicyState, RuleError> {
            Ok(socsim_games::BuiltinPolicyState::Stateless(
                socsim_games::Stateless,
            ))
        }

        fn decide(
            &self,
            _state: &mut <() as ExperimentState>::PolicyState,
            ctx: &DecisionContext<'_, MatrixGame, ()>,
            _rng: &mut SimRng,
        ) -> Result<usize, RuleError> {
            Ok(usize::from(ctx.role() == Some(0) && ctx.t() >= 2))
        }
    }

    fn cumulative_simulation(seed: u64) -> Simulation<GameWorld<MatrixGame>> {
        let game = matching_pennies();
        let metrics = GameMetrics::new(
            &game,
            MetricsOptions::new().mixed_source(MixedSource::PlayedCumulative),
        )
        .unwrap();
        let mut rules = RuleBook::new();
        rules
            .register("cumulative_schedule", Arc::new(CumulativeSchedule))
            .unwrap();
        let world = build_world(
            game,
            2,
            (),
            InitialActions::Same(0),
            &rules,
            &Assignment::Ordered(vec![AssignGroup {
                kind: AssignKind::Rule("cumulative_schedule".into()),
                count: 2,
            }]),
            3,
            3,
            seed,
        )
        .unwrap();
        assemble(
            world,
            FixedPlayers::pairs(&[(0, 1)]),
            rules,
            NormalFormOutcome,
            PlayOptions::new().decision_scope(DecisionScope::PerAgentRole),
            seed,
        )
        .unwrap()
        .add_mechanism(Box::new(metrics))
        .recorder(Box::new(MetricRecorder::default()))
        .build()
    }

    #[test]
    fn played_cumulative_regret_uses_all_steps_and_survives_snapshot() {
        let mut uninterrupted = cumulative_simulation(101);
        uninterrupted.run().unwrap();
        assert_close(metric_value(&uninterrupted, 1, "game.regret").unwrap(), 2.0);
        assert_close(metric_value(&uninterrupted, 2, "game.regret").unwrap(), 1.0);

        let mut partial = cumulative_simulation(101);
        partial.step().unwrap();
        let snapshot = partial.snapshot();
        let mut resumed = cumulative_simulation(999);
        resumed.restore(snapshot);
        resumed.run().unwrap();

        for t in 2..=3 {
            assert_close(
                metric_value(&resumed, t, "game.regret").unwrap(),
                metric_value(&uninterrupted, t, "game.regret").unwrap(),
            );
        }
    }

    #[test]
    fn invalid_stop_options_are_rejected_during_metrics_construction() {
        let game = matching_pennies();
        for tol in [0.0, -1.0, f64::NAN] {
            assert!(matches!(
                GameMetrics::new(&game, MetricsOptions::new().stop(tol, 1)),
                Err(GameError::Metrics(_))
            ));
        }
        assert!(matches!(
            GameMetrics::new(&game, MetricsOptions::new().stop(0.1, 0)),
            Err(GameError::Metrics(_))
        ));
    }

    #[cfg(feature = "metrics")]
    #[test]
    fn action_entropy_is_pooled_opt_out_and_omitted_for_per_role_games() {
        let mut pooled = held_pd_simulation(MetricsOptions::new());
        pooled.step().unwrap();
        assert_close(
            metric_value(&pooled, 1, "game.action_entropy").unwrap(),
            std::f64::consts::LN_2,
        );
        assert!(
            GameMetrics::new(&prisoners_dilemma(), MetricsOptions::new())
                .unwrap()
                .describe()
                .iter()
                .any(|(name, _)| name == "game.action_entropy")
        );
        assert!(!GameMetrics::new(
            &prisoners_dilemma(),
            MetricsOptions::new().action_entropy(false),
        )
        .unwrap()
        .describe()
        .iter()
        .any(|(name, _)| name == "game.action_entropy"));

        let game = MatrixGame::new(vec![2, 3], vec![vec![0.0; 6], vec![0.0; 6]])
            .unwrap()
            .with_labels(vec![
                vec!["left-a".into(), "left-b".into()],
                vec!["right-a".into(), "right-b".into(), "right-c".into()],
            ])
            .unwrap();
        let metrics = GameMetrics::new(&game, MetricsOptions::new()).unwrap();
        assert!(!metrics
            .describe()
            .iter()
            .any(|(name, _)| name == "game.action_entropy"));
        let rules = empty_rules();
        let world = build_world(
            game,
            2,
            (),
            InitialActions::Same(0),
            &rules,
            &held_assignment(2),
            1,
            1,
            103,
        )
        .unwrap();
        let mut per_role = assemble(
            world,
            FixedPlayers::pairs(&[(0, 1)]),
            rules,
            NormalFormOutcome,
            PlayOptions::new(),
            103,
        )
        .unwrap()
        .add_mechanism(Box::new(metrics))
        .recorder(Box::new(MetricRecorder::default()))
        .build();
        per_role.step().unwrap();
        assert_eq!(metric_value(&per_role, 1, "game.action_entropy"), None);
    }

    #[cfg(feature = "llm")]
    #[test]
    fn llm_assembly_creates_reports_but_llm_kinds_remain_none() {
        use socsim_games::llm::{
            assemble_with_llm, BackendFactory, BackendIdentity, CountsObs, CountsRenderer,
            LabelParser, LlmPolicy, LlmReplayManifest, LlmRuleBook, LlmSetup, ReplayMode,
            ResponseStore, WithLlm,
        };
        use socsim_games::{build_world_with, KindCatalog};
        use socsim_llm::mock::ScriptedClient;

        type LlmX = WithLlm<()>;

        let game = matching_pennies()
            .with_labels(vec![
                vec!["a0".into(), "a1".into()],
                vec!["a0".into(), "a1".into()],
            ])
            .unwrap();
        let rules: RuleBook<MatrixGame, LlmX> = RuleBook::new();
        let mut llm_rules: LlmRuleBook<MatrixGame, LlmX> = LlmRuleBook::new();
        llm_rules
            .register(
                "llm",
                Arc::new(LlmPolicy::new(
                    CountsObs,
                    CountsRenderer::default(),
                    LabelParser::strict(),
                )),
            )
            .unwrap();
        let catalog = KindCatalog::new()
            .with_rules(&rules)
            .unwrap()
            .with_llm(&llm_rules)
            .unwrap();
        let identity = BackendIdentity {
            model: "scripted".into(),
            endpoint: "mock://scripted".into(),
        };
        let store = ResponseStore::in_memory("mixed-reports", ReplayMode::Live);
        let world = build_world_with(
            game,
            2,
            WithLlm {
                inner: (),
                llm: LlmReplayManifest::for_store(&store, identity.clone()),
            },
            InitialActions::Same(0),
            &catalog,
            &Assignment::Ordered(vec![
                AssignGroup {
                    kind: AssignKind::Rule("llm".into()),
                    count: 1,
                },
                AssignGroup {
                    kind: AssignKind::Held,
                    count: 1,
                },
            ]),
            1,
            1,
            97,
        )
        .unwrap();
        let backend: BackendFactory = Arc::new(|| {
            Ok(Box::new(ScriptedClient::constant(
                "scripted",
                r#"{"action":"a0","reason":"test"}"#,
            )))
        });
        let setup = LlmSetup::new(llm_rules, store, backend, identity);
        let mut simulation = assemble_with_llm(
            world,
            FixedPlayers::pairs(&[(0, 1)]),
            rules,
            setup,
            NormalFormOutcome,
            PlayOptions::new().report_mixed(true),
            97,
        )
        .unwrap()
        .build();

        simulation.step().unwrap();

        assert_eq!(simulation.world().reported_mixed(AgentId(0), 0), None);
        assert_eq!(simulation.world().reported_mixed(AgentId(0), 1), None);
    }
}
