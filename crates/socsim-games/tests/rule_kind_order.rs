use std::sync::Arc;

use socsim_games::{
    assemble, build_world, AssignGroup, AssignKind, Assignment, Fixed, FixedPlayers, GameWorld,
    InitialActions, MatrixGame, NormalFormOutcome, PlayOptions, RuleBook, UniformRandom,
};

fn prisoners_dilemma() -> MatrixGame {
    MatrixGame::bimatrix(
        &[vec![3.0, 0.0], vec![5.0, 1.0]],
        &[vec![3.0, 5.0], vec![0.0, 1.0]],
    )
    .unwrap()
}

fn trajectory(register_random_first: bool) -> Vec<String> {
    let seed = 91;
    let mut rules: RuleBook<MatrixGame, ()> = RuleBook::new();
    if register_random_first {
        rules.register("z_random", Arc::new(UniformRandom)).unwrap();
        rules.register("a_fixed", Arc::new(Fixed::new(0))).unwrap();
    } else {
        rules.register("a_fixed", Arc::new(Fixed::new(0))).unwrap();
        rules.register("z_random", Arc::new(UniformRandom)).unwrap();
    }

    let world = build_world(
        prisoners_dilemma(),
        2,
        (),
        InitialActions::Same(0),
        &rules,
        &Assignment::Ordered(vec![
            AssignGroup {
                kind: AssignKind::Rule("z_random".into()),
                count: 1,
            },
            AssignGroup {
                kind: AssignKind::Rule("a_fixed".into()),
                count: 1,
            },
        ]),
        8,
        12,
        seed,
    )
    .unwrap();
    let mut simulation = assemble(
        world,
        FixedPlayers::pairs(&[(0, 1)]),
        rules,
        NormalFormOutcome,
        PlayOptions::new(),
        seed,
    )
    .unwrap()
    .build();

    (0..12)
        .map(|_| {
            simulation.step().unwrap();
            serde_json::to_string(simulation.world()).unwrap()
        })
        .collect()
}

#[test]
fn registration_order_does_not_change_kind_ids_or_trajectory() {
    let left = trajectory(false);
    let right = trajectory(true);
    assert_eq!(left, right);

    let world: GameWorld<MatrixGame, ()> = serde_json::from_str(left.last().unwrap()).unwrap();
    assert_eq!(
        world.policies().kind(socsim_core::AgentId(0)),
        Some("z_random")
    );
    assert_eq!(
        world.policies().kind(socsim_core::AgentId(1)),
        Some("a_fixed")
    );
}

/// The repeated prisoner's dilemma scenario with the keys of every table
/// written in the given order (`reversed` reverses them). The arrays, and so
/// the order of the groups, are the same in both.
#[cfg(feature = "config")]
fn scenario_text(reversed: bool) -> String {
    fn table(header: &str, keys: &[&str], reversed: bool) -> String {
        let mut keys = keys.to_vec();
        if reversed {
            keys.reverse();
        }
        format!("{header}\n{}\n\n", keys.join("\n"))
    }
    [
        table(
            "[simulation]",
            &[
                "name = \"ipd\"",
                "module_pack = \"normal-form\"",
                "t_max = 40",
                "seed = 3",
                "scheduler = \"sequential\"",
            ],
            reversed,
        ),
        table("[world]", &["n_agents = 2", "history_len = 40"], reversed),
        table(
            "[world.game]",
            &[
                "kind = \"matrix\"",
                "labels = [[\"C\", \"D\"], [\"C\", \"D\"]]",
                "payoffs = [[[3.0, 3.0], [0.0, 5.0]], [[5.0, 0.0], [1.0, 1.0]]]",
            ],
            reversed,
        ),
        table(
            "[world.population]",
            &["assign = \"ordered\"", "initial_shares = { D = 0.5 }"],
            reversed,
        ),
        "[[world.population.group]]\nrule = \"tit_for_tat\"\ncount = 1\n[[world.population.group]]\nrule = \"fictitious_play\"\ncount = 1\n\n".to_owned(),
        table(
            "[[mechanism]]\nname = \"game_match\"\n[mechanism.params]",
            &["matching = \"fixed_players\"", "pairs = [[0, 1]]"],
            reversed,
        ),
        table(
            "[[mechanism]]\nname = \"game_decide\"\n[mechanism.params]",
            &[
                "decision_scope = \"per_step\"",
                "coop_label = \"C\"",
                "fp_tie_break = \"random\"",
                "fp_initial_counts = [1.0, 2.0]",
            ],
            reversed,
        ),
        table(
            "[[mechanism]]\nname = \"game_play\"\n[mechanism.params]",
            &["payoff_aggregation = \"sum\"", "materialize = true"],
            reversed,
        ),
        table(
            "[[mechanism]]\nname = \"game_metrics\"\n[mechanism.params]",
            &[
                "cooperate_label = \"C\"",
                "equilibrium = \"solve\"",
                "regret = true",
            ],
            reversed,
        ),
        table(
            "[output]",
            &["log_path = \"runs/{name}_{seed}.jsonl\""],
            reversed,
        ),
    ]
    .concat()
}

#[cfg(feature = "config")]
fn scenario_trajectory(reversed: bool) -> Vec<String> {
    use socsim_engine::SimulationBuilder;
    use socsim_games::{FiniteSpec, GameTrialFactory, RuleRegistry};
    use socsim_runner::TrialFactory;

    let scenario = socsim_config::Scenario::parse(&scenario_text(reversed)).unwrap();
    let factory: GameTrialFactory<FiniteSpec> =
        GameTrialFactory::new(RuleRegistry::finite_builtins());
    factory.validate(&scenario).unwrap();
    let trial = factory.build_trial(&scenario, 17).unwrap();
    let mut simulation = trial
        .mechanisms
        .into_iter()
        .fold(SimulationBuilder::new(trial.world), |builder, mechanism| {
            builder.add_mechanism(mechanism)
        })
        .scheduler(trial.scheduler)
        .seed(trial.engine_seed)
        .build();
    (0..40)
        .map(|_| {
            simulation.step().unwrap();
            serde_json::to_string(simulation.world()).unwrap()
        })
        .collect()
}

#[cfg(feature = "config")]
#[test]
fn scenario_key_order_does_not_change_the_trajectory() {
    let written = scenario_trajectory(false);
    let reversed = scenario_trajectory(true);
    assert_eq!(written, reversed);

    let world: GameWorld<socsim_games::FiniteSpec, ()> =
        serde_json::from_str(written.last().unwrap()).unwrap();
    assert_eq!(
        world.policies().kind(socsim_core::AgentId(0)),
        Some("tit_for_tat")
    );
    assert_eq!(
        world.policies().kind(socsim_core::AgentId(1)),
        Some("fictitious_play")
    );
}
