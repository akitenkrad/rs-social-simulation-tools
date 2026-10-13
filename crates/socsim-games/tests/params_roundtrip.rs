//! Reading scenarios: `[world]`, `[world.game]`, `[world.population]`, and the
//! `[[mechanism]]` entries, with every key type-checked and every unknown key
//! rejected.

use std::sync::Arc;

use socsim_config::{Params, Scenario};
use socsim_core::SocsimError;
use socsim_games::catalog::{
    BattleOfSexes, ElFarol, HawkDove, MinorityGame, PrisonersDilemma, PublicGoodsFinite,
    RockPaperScissors,
};
use socsim_games::continuous::{BertrandDifferentiated, Cournot, PublicGoodsContinuous};
use socsim_games::{
    build_world, uniform_initial_actions, AssignKind, Assignment, ContinuousSpec, DecisionRule,
    FiniteGame, FiniteSpec, GameError, GameSimFactory, GameWorld, InitialActions, MatrixGame,
    NormalFormGame, RandomPairs, RuleBook, RuleRegistry, ScenarioConfig, StageGame, UniformRandom,
};
use socsim_runner::{run_trial_sweep, SweepAxis, Trial, TrialFactory};

const HEADER: &str = r#"
[simulation]
name = "roundtrip"
module_pack = "normal-form"
t_max = 4
seed = 11
scheduler = "sequential"

[output]
log_path = "runs/{name}_{seed}.jsonl"
"#;

const MECHANISMS: &str = r#"
[[mechanism]]
name = "game_match"
[mechanism.params]
matching = "random_pairs"

[[mechanism]]
name = "game_decide"

[[mechanism]]
name = "game_play"
"#;

fn held(n_agents: usize) -> String {
    format!("[world.population]\n[[world.population.group]]\nrule = \"held\"\ncount = {n_agents}\n")
}

fn hawk_dove_game() -> &'static str {
    "[world.game]\nkind = \"hawk_dove\"\nv = 2.0\nc = 3.0\n"
}

fn scenario(world: &str, mechanisms: &str) -> Scenario {
    let text = format!("{HEADER}\n[world]\n{world}\n{mechanisms}");
    Scenario::parse(&text).unwrap_or_else(|error| panic!("{error}\n{text}"))
}

fn read_finite(scenario: &Scenario) -> Result<ScenarioConfig<FiniteSpec>, GameError> {
    ScenarioConfig::read(scenario, &RuleRegistry::finite_builtins())
}

fn read_continuous(scenario: &Scenario) -> Result<ScenarioConfig<ContinuousSpec>, GameError> {
    ScenarioConfig::read(scenario, &RuleRegistry::continuous_builtins())
}

/// The world of a finite scenario: `n_agents`, `history_len`, a game, and one
/// held group, followed by `extra` lines of `[world]` keys.
fn finite_world(n_agents: usize, game: &str, extra: &str) -> String {
    format!(
        "n_agents = {n_agents}\nhistory_len = 2\n{extra}\n{game}\n{}",
        held(n_agents)
    )
}

#[track_caller]
fn assert_config_error<T>(result: Result<T, GameError>, path: &str) {
    match result {
        Err(GameError::Config(message)) => assert!(
            message.contains(path),
            "expected an error about {path:?}, got {message:?}"
        ),
        Err(other) => panic!("expected a configuration error about {path:?}, got {other:?}"),
        Ok(_) => panic!("expected a configuration error about {path:?}, got Ok"),
    }
}

fn counts_of(assignment: &Assignment) -> Vec<(String, usize)> {
    let groups = match assignment {
        Assignment::Ordered(groups) | Assignment::Shuffled(groups) => groups,
        _ => panic!("unexpected assignment"),
    };
    groups
        .iter()
        .map(|group| {
            let kind = match &group.kind {
                AssignKind::Rule(name) => name.clone(),
                AssignKind::Held => "held".to_owned(),
                _ => panic!("unexpected kind"),
            };
            (kind, group.count)
        })
        .collect()
}

// ── [world.game] ─────────────────────────────────────────────────────────────

const EXAMPLE_MATRIX: &str = r#"
[world.game]
kind    = "matrix"
labels  = [["C", "D"], ["C", "D"]]
payoffs = [
  [[3.0, 3.0], [0.0, 5.0]],
  [[5.0, 0.0], [1.0, 1.0]],
]
"#;

#[test]
fn a_matrix_game_reads_as_the_bimatrix_with_labels() {
    let config = read_finite(&scenario(&finite_world(2, EXAMPLE_MATRIX, ""), MECHANISMS)).unwrap();
    let expected = MatrixGame::bimatrix(
        &[vec![3.0, 0.0], vec![5.0, 1.0]],
        &[vec![3.0, 5.0], vec![0.0, 1.0]],
    )
    .unwrap()
    .with_labels(vec![
        vec!["C".to_owned(), "D".to_owned()],
        vec!["C".to_owned(), "D".to_owned()],
    ])
    .unwrap();
    assert_eq!(config.game(), &FiniteSpec::Matrix(expected));
}

#[test]
fn a_three_player_matrix_is_indexed_by_the_actions_of_each_role() {
    let game = r#"
[world.game]
kind = "matrix"
labels = [["a", "b"], ["a", "b"], ["a", "b"]]
payoffs = [
  [[[0, 1, 2], [3, 4, 5]], [[6, 7, 8], [9, 10, 11]]],
  [[[12, 13, 14], [15, 16, 17]], [[18, 19, 20], [21, 22, 23]]],
]
"#;
    let mechanisms = MECHANISMS.replace("random_pairs", "whole_population");
    let config = read_finite(&scenario(&finite_world(3, game, ""), &mechanisms)).unwrap();
    let game = config.game();
    assert_eq!(game.n_roles(), 3);
    // payoffs[1][0][1] is the profile [1, 0, 1].
    for role in 0..3 {
        assert_eq!(game.payoff(role, &[1, 0, 1]), (15 + role) as f64);
    }
    assert_eq!(game.action_label(2, 1), "b");
}

#[test]
fn malformed_matrices_are_rejected() {
    let ragged = "[world.game]\nkind = \"matrix\"\nlabels = [[\"C\", \"D\"], [\"C\", \"D\"]]\npayoffs = [[[3, 3]], [[5, 0], [1, 1]]]\n";
    assert_config_error(
        read_finite(&scenario(&finite_world(2, ragged, ""), MECHANISMS)),
        "world.game.payoffs[0]",
    );
    let non_ascii = "[world.game]\nkind = \"matrix\"\nlabels = [[\"協力\", \"D\"], [\"C\", \"D\"]]\npayoffs = [[[3, 3], [0, 5]], [[5, 0], [1, 1]]]\n";
    assert_config_error(
        read_finite(&scenario(&finite_world(2, non_ascii, ""), MECHANISMS)),
        "world.game.labels[0][0]",
    );
    let duplicate = "[world.game]\nkind = \"matrix\"\nlabels = [[\"C\", \"C\"], [\"C\", \"D\"]]\npayoffs = [[[3, 3], [0, 5]], [[5, 0], [1, 1]]]\n";
    assert_config_error(
        read_finite(&scenario(&finite_world(2, duplicate, ""), MECHANISMS)),
        "world.game.labels[0][1]",
    );
    let non_finite = "[world.game]\nkind = \"matrix\"\nlabels = [[\"C\", \"D\"], [\"C\", \"D\"]]\npayoffs = [[[3, nan], [0, 5]], [[5, 0], [1, 1]]]\n";
    assert_config_error(
        read_finite(&scenario(&finite_world(2, non_finite, ""), MECHANISMS)),
        "world.game.payoffs[0][0][1]",
    );
    let labels: Vec<String> = (0..1001).map(|a| format!("\"a{a}\"")).collect();
    let row = format!("[{}]", labels.join(", "));
    let huge = format!("[world.game]\nkind = \"matrix\"\nlabels = [{row}, {row}]\npayoffs = []\n");
    match read_finite(&scenario(&finite_world(2, &huge, ""), MECHANISMS)) {
        Err(GameError::Matrix(_)) => {}
        other => panic!("expected a matrix size error, got {:?}", other.err()),
    }
}

#[test]
fn built_in_games_read_from_their_keys() {
    let cases: Vec<(String, usize, FiniteSpec)> = vec![
        (
            "kind = \"prisoners_dilemma\"\ntemptation = 2\nreward = 1.0\npunishment = 0\nsucker = -1.0".to_owned(),
            2,
            FiniteSpec::PrisonersDilemmaGame(PrisonersDilemma::new(2.0, 1.0, 0.0, -1.0).unwrap()),
        ),
        (
            "kind = \"battle_of_sexes\"".to_owned(),
            2,
            FiniteSpec::BattleOfSexesGame(BattleOfSexes::default()),
        ),
        (
            "kind = \"battle_of_sexes\"\na = [[3, 0], [0, 2]]\nb = [[2, 0], [0, 3]]".to_owned(),
            2,
            FiniteSpec::BattleOfSexesGame(
                BattleOfSexes::new([[3.0, 0.0], [0.0, 2.0]], [[2.0, 0.0], [0.0, 3.0]]).unwrap(),
            ),
        ),
        (
            "kind = \"rock_paper_scissors\"\nwin = 2\nloss = 1".to_owned(),
            2,
            FiniteSpec::RockPaperScissorsGame(RockPaperScissors::new(2.0, 1.0).unwrap()),
        ),
        (
            "kind = \"hawk_dove\"\nv = 2\nc = 3".to_owned(),
            2,
            FiniteSpec::HawkDoveGame(HawkDove::new(2.0, 3.0).unwrap()),
        ),
        (
            "kind = \"el_farol\"\ncapacity = 3".to_owned(),
            5,
            FiniteSpec::ElFarolGame(ElFarol::new(5, 3).unwrap()),
        ),
        (
            "kind = \"minority\"".to_owned(),
            5,
            FiniteSpec::Minority(MinorityGame::new(5).unwrap()),
        ),
        (
            "kind = \"public_goods_finite\"\nk = 3\nr = 1.5\ncost = 1".to_owned(),
            6,
            FiniteSpec::PublicGoodsFiniteGame(PublicGoodsFinite::new(3, 1.5, 1.0).unwrap()),
        ),
    ];
    for (game, n_agents, expected) in cases {
        let game = format!("[world.game]\n{game}\n");
        let config = read_finite(&scenario(&finite_world(n_agents, &game, ""), MECHANISMS))
            .unwrap_or_else(|error| panic!("{game}: {error}"));
        assert_eq!(config.game(), &expected, "{game}");
    }
}

#[test]
fn continuous_games_take_their_players_from_n_agents() {
    let world = |game: &str| {
        format!(
            "n_agents = 3\nhistory_len = 4\n[world.game]\n{game}\n[world.population]\ninitial_uniform = true\n[[world.population.group]]\nrule = \"held\"\ncount = 3\n"
        )
    };
    let cases = [
        (
            "kind = \"cournot\"\na = 100\nb = 1.1\nc = 10\nq_max = 100",
            ContinuousSpec::CournotGame(Cournot::new(3, 100.0, 1.1, 10.0, 100.0).unwrap()),
        ),
        (
            "kind = \"bertrand_differentiated\"\na = 10\nb = 2\nd = 0.5\nc = 1\np_max = 10",
            ContinuousSpec::BertrandDifferentiatedGame(
                BertrandDifferentiated::new(3, 10.0, 2.0, 0.5, 1.0, 10.0).unwrap(),
            ),
        ),
        (
            "kind = \"public_goods_continuous\"\nk = 3\nendowment = 10\nr = 2\nsocial_dilemma = true",
            ContinuousSpec::PublicGoodsContinuousGame(
                PublicGoodsContinuous::social_dilemma(3, 10.0, 2.0).unwrap(),
            ),
        ),
    ];
    for (game, expected) in cases {
        let config = read_continuous(&scenario(&world(game), MECHANISMS))
            .unwrap_or_else(|error| panic!("{game}: {error}"));
        assert_eq!(config.game(), &expected, "{game}");
    }

    assert_config_error(
        read_continuous(&scenario(
            &world("kind = \"cournot\"\nn = 3\na = 100\nb = 1.1\nc = 10\nq_max = 100"),
            MECHANISMS,
        )),
        "world.game.n",
    );
    for (game, n_agents) in [
        ("kind = \"el_farol\"\nn = 5\ncapacity = 3", 5),
        ("kind = \"minority\"\nn = 5", 5),
    ] {
        let game = format!("[world.game]\n{game}\n");
        assert_config_error(
            read_finite(&scenario(&finite_world(n_agents, &game, ""), MECHANISMS)),
            "world.game.n",
        );
    }
}

#[test]
fn game_kinds_and_constructor_conditions_are_checked() {
    let finite = |game: &str| {
        read_finite(&scenario(
            &finite_world(2, &format!("[world.game]\n{game}\n"), ""),
            MECHANISMS,
        ))
    };
    match finite(
        "kind = \"prisoners_dilemma\"\ntemptation = 1\nreward = 2\npunishment = 3\nsucker = 4",
    ) {
        Err(GameError::Catalog(_)) => {}
        other => panic!("expected a catalog error, got {:?}", other.err()),
    }
    assert_config_error(
        finite("kind = \"hawk_dov\"\nv = 2\nc = 3"),
        "world.game.kind",
    );
    assert_config_error(
        finite("kind = \"cournot\"\na = 100\nb = 1\nc = 10\nq_max = 100"),
        "world.game.kind",
    );
    assert_config_error(
        finite("kind = \"prisoners_dilemma\"\ntemptation = 2\nreward = 1\npunishment = 0"),
        "world.game.sucker",
    );
    assert_config_error(
        finite("kind = \"battle_of_sexes\"\na = [[3, 0], [0, 2]]"),
        "world.game.b",
    );
    assert_config_error(
        finite("kind = \"prisoners_dilemma\"\ntemptaton = 2\ntemptation = 2\nreward = 1\npunishment = 0\nsucker = 0"),
        "world.game.temptaton",
    );

    let continuous = read_continuous(&scenario(
        "n_agents = 2\nhistory_len = 2\n[world.game]\nkind = \"hawk_dove\"\nv = 2\nc = 3\n[world.population]\ninitial_uniform = true\n[[world.population.group]]\nrule = \"held\"\ncount = 2\n",
        MECHANISMS,
    ));
    assert_config_error(continuous, "world.game.kind");
}

// ── numbers and counts ───────────────────────────────────────────────────────

#[test]
fn counts_accept_integral_values_and_reject_everything_else() {
    let history = |value: &str| {
        read_finite(&scenario(
            &format!(
                "n_agents = 2\nhistory_len = {value}\n{}{}",
                hawk_dove_game(),
                held(2)
            ),
            MECHANISMS,
        ))
    };
    assert_eq!(history("16").unwrap().history_len(), 16);
    assert_eq!(history("16.0").unwrap().history_len(), 16);
    for bad in ["-1", "1.5", "1e30", "nan", "inf", "\"16\"", "true"] {
        assert_config_error(history(bad), "world.history_len");
    }

    let window = |value: &str| {
        let mechanisms = format!(
            "{MECHANISMS}\n[[mechanism]]\nname = \"game_metrics\"\n[mechanism.params]\nstop_tol = 1e-6\nstop_window = {value}\n"
        );
        read_finite(&scenario(
            &finite_world(2, hawk_dove_game(), ""),
            &mechanisms,
        ))
    };
    assert!(window("4294967295").is_ok());
    assert_config_error(window("4294967296"), "mechanism[game_metrics].stop_window");
}

#[test]
fn numbers_accept_integers_and_reject_other_types() {
    let temptation = |value: &str| {
        let game = format!(
            "[world.game]\nkind = \"prisoners_dilemma\"\ntemptation = {value}\nreward = 1\npunishment = 0\nsucker = 0\n"
        );
        read_finite(&scenario(&finite_world(2, &game, ""), MECHANISMS))
    };
    assert_eq!(
        temptation("2").unwrap().game(),
        &FiniteSpec::PrisonersDilemmaGame(PrisonersDilemma::new(2.0, 1.0, 0.0, 0.0).unwrap())
    );
    for bad in ["\"x\"", "true", "nan", "inf"] {
        assert_config_error(temptation(bad), "world.game.temptation");
    }
}

// ── [world] ──────────────────────────────────────────────────────────────────

#[cfg(feature = "net")]
#[test]
fn network_parameters_are_checked_against_their_ranges() {
    let network = |keys: &str| {
        read_finite(&scenario(
            &finite_world(
                10,
                hawk_dove_game(),
                &format!("topology = \"network\"\n{keys}"),
            ),
            MECHANISMS,
        ))
    };
    for p in ["0.0", "1.0"] {
        assert!(network(&format!("network_model = \"erdos_renyi\"\nnetwork_p = {p}")).is_ok());
    }
    for p in ["-0.1", "1.1"] {
        assert_config_error(
            network(&format!("network_model = \"erdos_renyi\"\nnetwork_p = {p}")),
            "world.network_p",
        );
    }
    let watts_strogatz = |k: &str, beta: &str| {
        network(&format!(
            "network_model = \"watts_strogatz\"\nnetwork_k = {k}\nnetwork_beta = {beta}"
        ))
    };
    for k in ["2", "8"] {
        assert!(watts_strogatz(k, "0.5").is_ok(), "k = {k}");
    }
    for k in ["0", "3", "10"] {
        assert_config_error(watts_strogatz(k, "0.5"), "world.network_k");
    }
    for beta in ["0.0", "1.0"] {
        assert!(watts_strogatz("4", beta).is_ok(), "beta = {beta}");
    }
    for beta in ["-0.1", "1.1"] {
        assert_config_error(watts_strogatz("4", beta), "world.network_beta");
    }
    let barabasi_albert = |m: &str| {
        network(&format!(
            "network_model = \"barabasi_albert\"\nnetwork_m = {m}"
        ))
    };
    for m in ["1", "9"] {
        assert!(barabasi_albert(m).is_ok(), "m = {m}");
    }
    for m in ["0", "10"] {
        assert_config_error(barabasi_albert(m), "world.network_m");
    }
    for (old, path) in [
        (
            "network = \"erdos_renyi\"\nnetwork_model = \"erdos_renyi\"\nnetwork_p = 0.5",
            "world.network",
        ),
        (
            "network_model = \"erdos_renyi\"\nnetwork_p = 0.5\np = 0.5",
            "world.p",
        ),
        (
            "network_model = \"watts_strogatz\"\nnetwork_k = 4\nnetwork_beta = 0.5\nk = 4",
            "world.k",
        ),
        (
            "network_model = \"watts_strogatz\"\nnetwork_k = 4\nnetwork_beta = 0.5\nbeta = 0.5",
            "world.beta",
        ),
        (
            "network_model = \"barabasi_albert\"\nnetwork_m = 2\nm = 2",
            "world.m",
        ),
    ] {
        assert_config_error(network(old), path);
    }
}

#[cfg(feature = "net")]
#[test]
fn a_network_is_generated_from_stream_0_1() {
    use socsim_core::{derive_seed, AgentId, SimRng};
    use socsim_games::Topology;
    use socsim_net::SocialNetwork;

    let config = read_finite(&scenario(
        &finite_world(
            10,
            hawk_dove_game(),
            "topology = \"network\"\nnetwork_model = \"watts_strogatz\"\nnetwork_k = 4\nnetwork_beta = 0.3",
        ),
        MECHANISMS,
    ))
    .unwrap();
    let ids: Vec<AgentId> = (0..10).map(AgentId).collect();
    for seed in [0, 5] {
        let mut rng = SimRng::from_seed(derive_seed(seed, &[0, 1]));
        let expected = Topology::network(SocialNetwork::watts_strogatz(&ids, 4, 0.3, &mut rng));
        assert_eq!(
            serde_json::to_value(config.topology(seed).unwrap()).unwrap(),
            serde_json::to_value(expected).unwrap()
        );
    }
}

#[cfg(feature = "grid")]
#[test]
fn a_grid_gives_the_number_of_agents() {
    let world = |extra: &str| {
        format!(
            "history_len = 1\ntopology = \"grid\"\ngrid_rows = 3\ngrid_cols = 4\nboundary = \"toroidal\"\nneighborhood = \"moore\"\n{extra}\n{}{}",
            hawk_dove_game(),
            held(12)
        )
    };
    let mechanisms = MECHANISMS.replace("random_pairs", "grid_neighborhood");
    assert_eq!(
        read_finite(&scenario(&world(""), &mechanisms))
            .unwrap()
            .n_agents(),
        12
    );
    assert!(read_finite(&scenario(&world("n_agents = 12"), &mechanisms)).is_ok());
    assert_config_error(
        read_finite(&scenario(&world("n_agents = 10"), &mechanisms)),
        "world.n_agents",
    );
    assert_config_error(
        read_finite(&scenario(
            &finite_world(4, hawk_dove_game(), "grid_rows = 2"),
            MECHANISMS,
        )),
        "world.grid_rows",
    );
}

// ── [world.population] ───────────────────────────────────────────────────────

fn letters_registry() -> RuleRegistry<FiniteSpec, ()> {
    let mut registry = RuleRegistry::new();
    for name in ["a", "b", "c"] {
        registry
            .register(name, &[], |_, _| {
                let rule: Arc<dyn DecisionRule<FiniteSpec, ()>> = Arc::new(UniformRandom);
                Ok(rule)
            })
            .unwrap();
    }
    registry
}

fn groups(
    n_agents: usize,
    groups: &[(&str, &str)],
) -> Result<ScenarioConfig<FiniteSpec>, GameError> {
    let mut world = format!(
        "n_agents = {n_agents}\nhistory_len = 2\n{}[world.population]\n",
        hawk_dove_game()
    );
    for (rule, size) in groups {
        world.push_str(&format!(
            "[[world.population.group]]\nrule = \"{rule}\"\n{size}\n"
        ));
    }
    ScenarioConfig::read(&scenario(&world, MECHANISMS), &letters_registry())
}

#[test]
fn group_shares_use_the_largest_remainder_with_ties_in_listed_order() {
    let third = "share = 0.3333333333333333";
    let config = groups(10, &[("a", third), ("b", third), ("c", third)]).unwrap();
    assert_eq!(
        counts_of(config.assignment()),
        [
            ("a".to_owned(), 4),
            ("b".to_owned(), 3),
            ("c".to_owned(), 3)
        ]
    );
    let config = groups(10, &[("c", third), ("b", third), ("a", third)]).unwrap();
    assert_eq!(
        counts_of(config.assignment()),
        [
            ("c".to_owned(), 4),
            ("b".to_owned(), 3),
            ("a".to_owned(), 3)
        ]
    );
    let config = groups(7, &[("b", "share = 0.5"), ("a", "share = 0.5")]).unwrap();
    assert_eq!(
        counts_of(config.assignment()),
        [("b".to_owned(), 4), ("a".to_owned(), 3)]
    );
    assert!(matches!(config.assignment(), Assignment::Ordered(_)));
}

#[test]
fn group_counts_and_shares_are_checked() {
    let config = groups(5, &[("a", "count = 2"), ("held", "count = 3.0")]).unwrap();
    assert_eq!(
        counts_of(config.assignment()),
        [("a".to_owned(), 2), ("held".to_owned(), 3)]
    );
    assert_config_error(
        groups(5, &[("a", "count = 2"), ("b", "share = 0.6")]),
        "world.population.group[1]",
    );
    assert_config_error(
        groups(5, &[("a", "count = 2"), ("b", "count = 2")]),
        "world.population.group",
    );
    assert_config_error(
        groups(5, &[("a", "share = 0.5"), ("b", "share = 0.6")]),
        "world.population.group",
    );
    assert_config_error(
        groups(5, &[("a", "count = 2"), ("a", "count = 3")]),
        "world.population.group[1].rule",
    );
    assert_config_error(
        groups(5, &[("a", "count = 2"), ("d", "count = 3")]),
        "world.population.group[1].rule",
    );
    assert_config_error(
        groups(5, &[("a", "count = 2\nshare = 0.4"), ("b", "count = 3")]),
        "world.population.group[0]",
    );
    assert_config_error(
        groups(5, &[("a", "count = -2"), ("b", "count = 7")]),
        "world.population.group[0].count",
    );
    assert_config_error(
        groups(5, &[("a", "count = 5\nweight = 1")]),
        "world.population.group[0].weight",
    );
}

#[test]
fn shuffled_assignments_are_read() {
    let world = format!(
        "n_agents = 4\nhistory_len = 2\n{}[world.population]\nassign = \"shuffled\"\n[[world.population.group]]\nrule = \"held\"\ncount = 4\n",
        hawk_dove_game()
    );
    let config = read_finite(&scenario(&world, MECHANISMS)).unwrap();
    assert!(matches!(config.assignment(), Assignment::Shuffled(_)));
}

fn shares_world(n_agents: usize, game: &str, shares: &str) -> String {
    format!(
        "n_agents = {n_agents}\nhistory_len = 2\n{game}\n[world.population]\ninitial_shares = {shares}\n[[world.population.group]]\nrule = \"held\"\ncount = {n_agents}\n"
    )
}

const PD: &str = "[world.game]\nkind = \"prisoners_dilemma\"\ntemptation = 1.85\nreward = 1\npunishment = 0\nsucker = 0\n";

#[test]
fn initial_shares_give_the_rest_to_the_action_left_out() {
    let config = read_finite(&scenario(&shares_world(100, PD, "{ C = 0.9 }"), MECHANISMS)).unwrap();
    let actions = config.initial_actions(1);
    let InitialActions::Shares(shares) = &actions else {
        panic!("expected shares, got {actions:?}");
    };
    assert_eq!(shares, &vec![(0, 0.9), (1, 1.0 - 0.9)]);

    let world = build_world(
        config.game().clone(),
        100,
        (),
        actions,
        &RuleBook::new(),
        config.assignment(),
        2,
        1,
        1,
    )
    .unwrap();
    let json = serde_json::to_value(&world).unwrap();
    let held: Vec<usize> = serde_json::from_value(json["held"].clone()).unwrap();
    assert_eq!(held.iter().filter(|&&action| action == 0).count(), 90);
    assert_eq!(held.iter().filter(|&&action| action == 1).count(), 10);
}

#[test]
fn invalid_initial_shares_are_rejected() {
    assert_config_error(
        read_finite(&scenario(
            &shares_world(100, PD, "{ C = 0.7, D = 0.4 }"),
            MECHANISMS,
        )),
        "world.population.initial_shares",
    );
    let rps = "[world.game]\nkind = \"rock_paper_scissors\"\n";
    assert_config_error(
        read_finite(&scenario(
            &shares_world(100, rps, "{ R = 0.5 }"),
            MECHANISMS,
        )),
        "world.population.initial_shares",
    );
    assert_config_error(
        read_finite(&scenario(&shares_world(100, PD, "{ X = 0.5 }"), MECHANISMS)),
        "world.population.initial_shares.X",
    );
    assert_config_error(
        read_finite(&scenario(
            &shares_world(100, PD, "{ C = -0.1 }"),
            MECHANISMS,
        )),
        "world.population.initial_shares.C",
    );
}

#[test]
fn initial_actions_default_to_action_zero_and_accept_one_key() {
    let config = read_finite(&scenario(&finite_world(2, PD, ""), MECHANISMS)).unwrap();
    assert!(matches!(config.initial_actions(0), InitialActions::Same(0)));

    let world = format!(
        "n_agents = 2\nhistory_len = 2\n{PD}\n[world.population]\ninitial_action = \"D\"\n[[world.population.group]]\nrule = \"held\"\ncount = 2\n"
    );
    let config = read_finite(&scenario(&world, MECHANISMS)).unwrap();
    assert!(matches!(config.initial_actions(0), InitialActions::Same(1)));

    let world = format!(
        "n_agents = 2\nhistory_len = 2\n{PD}\n[world.population]\ninitial_action = \"D\"\ninitial_shares = {{ C = 0.5 }}\n[[world.population.group]]\nrule = \"held\"\ncount = 2\n"
    );
    assert_config_error(
        read_finite(&scenario(&world, MECHANISMS)),
        "world.population",
    );

    let world = format!(
        "n_agents = 2\nhistory_len = 2\n{PD}\n[world.population]\ninitial_uniform = true\n[[world.population.group]]\nrule = \"held\"\ncount = 2\n"
    );
    assert_config_error(
        read_finite(&scenario(&world, MECHANISMS)),
        "world.population.initial_uniform",
    );
}

#[test]
fn uniform_initial_actions_are_drawn_from_stream_0_2() {
    let world = "n_agents = 10\nhistory_len = 2\n[world.game]\nkind = \"public_goods_continuous\"\nk = 4\nendowment = 10\nr = 1.6\n[world.population]\ninitial_uniform = true\n[[world.population.group]]\nrule = \"held\"\ncount = 10\n";
    let config = read_continuous(&scenario(world, MECHANISMS)).unwrap();
    for seed in [0, 3] {
        let InitialActions::PerAgent(actions) = config.initial_actions(seed) else {
            panic!("expected per-agent actions");
        };
        assert_eq!(actions, uniform_initial_actions(config.game(), 10, seed));
    }

    let missing = "n_agents = 3\nhistory_len = 2\n[world.game]\nkind = \"cournot\"\na = 100\nb = 1\nc = 10\nq_max = 100\n[world.population]\n[[world.population.group]]\nrule = \"held\"\ncount = 3\n";
    assert_config_error(
        read_continuous(&scenario(missing, MECHANISMS)),
        "world.population",
    );
    let out_of_bounds = "n_agents = 3\nhistory_len = 2\n[world.game]\nkind = \"cournot\"\na = 100\nb = 1\nc = 10\nq_max = 100\n[world.population]\ninitial_action = [101.0]\n[[world.population.group]]\nrule = \"held\"\ncount = 3\n";
    assert_config_error(
        read_continuous(&scenario(out_of_bounds, MECHANISMS)),
        "world.population.initial_action",
    );
}

// ── [[mechanism]] and rules ──────────────────────────────────────────────────

fn decide_scenario(rule: &str, params: &str) -> Scenario {
    let world = format!(
        "n_agents = 2\nhistory_len = 2\n{EXAMPLE_MATRIX}\n[world.population]\n[[world.population.group]]\nrule = \"{rule}\"\ncount = 2\n"
    );
    let mechanisms = format!(
        "[[mechanism]]\nname = \"game_match\"\n[mechanism.params]\nmatching = \"fixed_players\"\npairs = [[0, 1]]\n\n[[mechanism]]\nname = \"game_decide\"\n[mechanism.params]\n{params}\n\n[[mechanism]]\nname = \"game_play\"\n"
    );
    scenario(&world, &mechanisms)
}

#[test]
fn rule_keys_declared_at_registration_are_accepted() {
    let mut registry = RuleRegistry::<FiniteSpec, ()>::finite_builtins();
    registry
        .register(
            "my_rule",
            &["my_alpha"],
            |params: &Params, _: &FiniteSpec| {
                assert!(params.get_value("my_alpha").is_some());
                let rule: Arc<dyn DecisionRule<FiniteSpec, ()>> = Arc::new(UniformRandom);
                Ok(rule)
            },
        )
        .unwrap();
    assert!(ScenarioConfig::read(&decide_scenario("my_rule", "my_alpha = 0.5"), &registry).is_ok());
    let rejected = ScenarioConfig::read(
        &decide_scenario("my_rule", "my_alpha = 0.5\nmy_beta = 1.0"),
        &registry,
    );
    assert_config_error(rejected, "mechanism[game_decide].my_beta");

    let duplicate = registry.register("my_rule", &[], |_: &Params, _: &FiniteSpec| {
        let rule: Arc<dyn DecisionRule<FiniteSpec, ()>> = Arc::new(UniformRandom);
        Ok(rule)
    });
    assert!(duplicate.is_err());
    assert!(registry.names().contains(&"my_rule"));
}

#[test]
fn unknown_rule_parameters_are_reported_with_their_path() {
    assert_config_error(
        read_finite(&decide_scenario(
            "logit",
            "logit_lamda = 2.0\nlogit_lambda = 2.0",
        )),
        "mechanism[game_decide].logit_lamda",
    );
    assert_config_error(
        read_finite(&decide_scenario("logit", "")),
        "mechanism[game_decide].logit_lambda",
    );
    assert!(read_finite(&decide_scenario("logit", "logit_lambda = 2")).is_ok());
    assert_config_error(
        read_finite(&decide_scenario("tit_for_tat", "coop_label = \"X\"")),
        "mechanism[game_decide].coop_label",
    );
    assert!(read_finite(&decide_scenario("tit_for_tat", "coop_label = \"C\"")).is_ok());
    assert_config_error(
        read_finite(&decide_scenario(
            "uniform_random",
            "decision_scope = \"per_turn\"",
        )),
        "mechanism[game_decide].decision_scope",
    );
}

#[test]
fn fictitious_play_initial_counts_are_checked() {
    assert!(read_finite(&decide_scenario(
        "fictitious_play",
        "fp_initial_counts = [2.0, 0.0]"
    ))
    .is_ok());
    for bad in ["[1.0]", "[-1.0, 0.0]"] {
        assert_config_error(
            read_finite(&decide_scenario(
                "fictitious_play",
                &format!("fp_initial_counts = {bad}"),
            )),
            "mechanism[game_decide].fp_initial_counts",
        );
    }
}

#[test]
fn continuous_rules_are_not_finite_rules() {
    let world = "n_agents = 3\nhistory_len = 2\n[world.game]\nkind = \"cournot\"\na = 100\nb = 1\nc = 10\nq_max = 100\n[world.population]\ninitial_uniform = true\n[[world.population.group]]\nrule = \"fictitious_play\"\ncount = 3\n";
    let mechanisms = MECHANISMS.replace("random_pairs", "whole_population");
    assert_config_error(
        read_continuous(&scenario(world, &mechanisms)),
        "world.population.group[0].rule",
    );
}

fn with_entries(entries: &[(&str, &str)]) -> Result<ScenarioConfig<FiniteSpec>, GameError> {
    let mut mechanisms = String::new();
    for (name, params) in entries {
        mechanisms.push_str(&format!(
            "[[mechanism]]\nname = \"{name}\"\n[mechanism.params]\n{params}\n\n"
        ));
    }
    read_finite(&scenario(
        &finite_world(4, hawk_dove_game(), ""),
        &mechanisms,
    ))
}

#[test]
fn mechanism_entries_are_checked_as_a_whole() {
    let matching = ("game_match", "matching = \"random_pairs\"");
    let decide = ("game_decide", "");
    let play = ("game_play", "");
    let fermi = ("fermi_update", "update = \"synchronous\"\nk = 0.5");
    assert!(with_entries(&[matching, decide, fermi, play]).is_ok());
    assert!(with_entries(&[play, fermi, decide, matching]).is_err());
    assert_config_error(
        with_entries(&[matching, fermi, decide, play]),
        "mechanism[fermi_update]",
    );
    assert_config_error(with_entries(&[matching, decide]), "mechanism[game_play]");
    assert_config_error(
        with_entries(&[matching, decide, play, play]),
        "mechanism[game_play]",
    );
    assert_config_error(
        with_entries(&[matching, decide, play, ("game_reward", "")]),
        "mechanism[game_reward]",
    );
    assert_config_error(
        with_entries(&[
            matching,
            decide,
            fermi,
            (
                "moran_update",
                "update = \"synchronous\"\nfitness = \"exponential\"\nw = 1",
            ),
            play,
        ]),
        "mechanism[moran_update]",
    );
    assert_config_error(
        with_entries(&[
            matching,
            decide,
            ("imitate_best", "update = \"asynchronous\""),
            play,
        ]),
        "mechanism[imitate_best].update",
    );
    assert_config_error(
        with_entries(&[
            ("game_match", "matching = \"random_pairs\"\nk = 2"),
            decide,
            play,
        ]),
        "mechanism[game_match].k",
    );
    assert_config_error(
        with_entries(&[
            matching,
            decide,
            ("game_play", "aggregate_history_len = 1.5"),
        ]),
        "mechanism[game_play].aggregate_history_len",
    );
    assert_config_error(
        with_entries(&[matching, decide, play, ("game_metrics", "stop_tol = 1e-6")]),
        "mechanism[game_metrics].stop_window",
    );
}

#[test]
fn replicator_update_is_only_for_finite_games() {
    let world = "n_agents = 4\nhistory_len = 2\n[world.game]\nkind = \"public_goods_continuous\"\nk = 2\nendowment = 10\nr = 1.5\n[world.population]\ninitial_uniform = true\n[[world.population.group]]\nrule = \"held\"\ncount = 4\n";
    let mechanisms = format!(
        "{MECHANISMS}\n[[mechanism]]\nname = \"replicator_update\"\n[mechanism.params]\nupdate = \"synchronous\"\nbeta = 0.1\n"
    );
    assert_config_error(
        read_continuous(&scenario(world, &mechanisms)),
        "mechanism[replicator_update]",
    );
}

// ── sweeps ───────────────────────────────────────────────────────────────────

/// A trial factory that reads every scenario it is given, and builds a held
/// population of the scenario's game on random pairs.
struct ReadingFactory;

fn to_socsim(error: GameError) -> SocsimError {
    SocsimError::Config(error.to_string())
}

impl TrialFactory<GameWorld<FiniteSpec>> for ReadingFactory {
    fn validate(&self, scenario: &Scenario) -> socsim_core::Result<()> {
        read_finite(scenario).map(|_| ()).map_err(to_socsim)
    }

    fn build_trial(
        &self,
        scenario: &Scenario,
        seed: u64,
    ) -> socsim_core::Result<Trial<GameWorld<FiniteSpec>>> {
        let config = read_finite(scenario).map_err(to_socsim)?;
        GameSimFactory::new(config.game().clone())
            .agents(config.n_agents())
            .matching(RandomPairs::new())
            .initial_held(config.initial_actions(seed))
            .history_len(config.history_len())
            .t_max(scenario.simulation.t_max)
            .build_trial(seed)
            .map_err(to_socsim)
    }
}

#[test]
fn values_written_back_by_a_checked_sweep_read_back() {
    let world = format!(
        "n_agents = 4\nhistory_len = 2\n{PD}\n[world.population]\ninitial_shares = {{ C = 0.5 }}\n[[world.population.group]]\nrule = \"held\"\nshare = 1.0\n"
    );
    let base = scenario(&world, MECHANISMS);
    let axes = [
        SweepAxis {
            param_key: "world.n_agents".to_owned(),
            values: vec![4.0, 6.0],
        },
        SweepAxis {
            param_key: "world.game.temptation".to_owned(),
            values: vec![2.0],
        },
        SweepAxis {
            param_key: "world.population.initial_shares.C".to_owned(),
            values: vec![0.25],
        },
        SweepAxis {
            param_key: "game_play.aggregate_history_len".to_owned(),
            values: vec![3.0],
        },
    ];
    let points = run_trial_sweep(&base, &axes, &ReadingFactory, &[1, 2], false).unwrap();
    assert_eq!(points.len(), 2);
    for (point, n_agents) in points.iter().zip([4, 6]) {
        let world = &point.effective.world.0;
        assert!(world["n_agents"].is_integer());
        let play = &point.effective.mechanisms[2].params.0;
        assert!(play["aggregate_history_len"].is_float());

        let config = read_finite(&point.effective).unwrap();
        assert_eq!(config.n_agents(), n_agents);
        assert_eq!(
            config.game(),
            &FiniteSpec::PrisonersDilemmaGame(PrisonersDilemma::new(2.0, 1.0, 0.0, 0.0).unwrap())
        );
        let InitialActions::Shares(shares) = config.initial_actions(0) else {
            panic!("expected shares");
        };
        assert_eq!(shares, vec![(0, 0.25), (1, 1.0 - 0.25)]);
        assert_eq!(
            counts_of(config.assignment()),
            [("held".to_owned(), n_agents)]
        );
    }
}
