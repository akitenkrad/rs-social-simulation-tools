use socsim_config::Scenario;
use socsim_packs::normal_form::{NormalFormPack, NORMAL_FORM_STARTER};
use socsim_runner::SweepAxis;

const IPD: &str = include_str!("../../../scenarios/normal_form_ipd.toml");
const NOWAK_MAY: &str = include_str!("../../../scenarios/normal_form_nowak_may.toml");
const COURNOT: &str = include_str!("../../../scenarios/normal_form_cournot.toml");

type Mutation = fn(&mut toml::Value);

struct InvalidCase {
    name: &'static str,
    base: &'static str,
    mutate: Mutation,
    expected: &'static str,
    rejects_unknown_key: bool,
}

fn scenario(value: &toml::Value) -> Scenario {
    Scenario::parse(&toml::to_string(value).expect("the mutated TOML should serialize"))
        .expect("the mutated TOML should remain a Scenario")
}

fn parsed(text: &str) -> toml::Value {
    toml::from_str(text).expect("the fixture should be valid TOML")
}

fn mechanisms(value: &mut toml::Value) -> &mut Vec<toml::Value> {
    value["mechanism"]
        .as_array_mut()
        .expect("mechanism should be an array")
}

fn mechanism<'a>(value: &'a mut toml::Value, name: &str) -> &'a mut toml::value::Table {
    mechanisms(value)
        .iter_mut()
        .find_map(|entry| {
            let table = entry.as_table_mut()?;
            (table.get("name")?.as_str()? == name).then_some(table)
        })
        .unwrap_or_else(|| panic!("missing mechanism {name}"))
}

fn params<'a>(value: &'a mut toml::Value, name: &str) -> &'a mut toml::value::Table {
    mechanism(value, name)
        .get_mut("params")
        .and_then(toml::Value::as_table_mut)
        .unwrap_or_else(|| panic!("missing params for mechanism {name}"))
}

fn groups(value: &mut toml::Value) -> &mut Vec<toml::Value> {
    value["world"]["population"]["group"]
        .as_array_mut()
        .expect("population.group should be an array")
}

fn mechanism_entry(text: &str) -> toml::Value {
    parsed(text)["mechanism"]
        .as_array()
        .expect("mechanism fragment should be an array")[0]
        .clone()
}

fn set_game(value: &mut toml::Value, text: &str) {
    let replacement = parsed(text)["game"].clone();
    value["world"]
        .as_table_mut()
        .expect("world should be a table")
        .insert("game".to_owned(), replacement);
}

fn set_matching(value: &mut toml::Value, matching: &str) {
    let params = params(value, "game_match");
    params.clear();
    params.insert(
        "matching".to_owned(),
        toml::Value::String(matching.to_owned()),
    );
}

fn shrink_grid(value: &mut toml::Value) {
    value["world"]["grid_rows"] = toml::Value::Integer(10);
    value["world"]["grid_cols"] = toml::Value::Integer(10);
}

fn continuous_with_finite_rule(value: &mut toml::Value) {
    let group = groups(value)[0].as_table_mut().unwrap();
    group.insert(
        "rule".to_owned(),
        toml::Value::String("fictitious_play".to_owned()),
    );
}

fn continuous_with_replicator(value: &mut toml::Value) {
    mechanisms(value).push(mechanism_entry(
        r#"
[[mechanism]]
name = "replicator_update"
[mechanism.params]
update = "synchronous"
beta = 0.01
"#,
    ));
}

fn finite_with_continuous_rule(value: &mut toml::Value) {
    let group = groups(value)[1].as_table_mut().unwrap();
    group.insert(
        "rule".to_owned(),
        toml::Value::String("damped_best_response".to_owned()),
    );
}

fn nonanonymous_counting(value: &mut toml::Value) {
    params(value, "game_play").insert("counting".to_owned(), toml::Value::Boolean(true));
}

fn continuous_counting_false(value: &mut toml::Value) {
    params(value, "game_play").insert("counting".to_owned(), toml::Value::Boolean(false));
}

fn three_action_game(value: &mut toml::Value) {
    set_game(
        value,
        r#"
[game]
kind = "rock_paper_scissors"
win = 1.0
loss = 1.0
"#,
    );
    params(value, "game_decide")
        .insert("coop_label".to_owned(), toml::Value::String("R".to_owned()));
    params(value, "game_metrics").insert(
        "cooperate_label".to_owned(),
        toml::Value::String("R".to_owned()),
    );
}

fn three_action_tit_for_tat(value: &mut toml::Value) {
    three_action_game(value);
}

fn three_action_grim(value: &mut toml::Value) {
    three_action_game(value);
    groups(value)[0]
        .as_table_mut()
        .unwrap()
        .insert("rule".to_owned(), toml::Value::String("grim".to_owned()));
}

fn three_role_fictitious_play(value: &mut toml::Value) {
    value["world"]["n_agents"] = toml::Value::Integer(3);
    set_game(
        value,
        r#"
[game]
kind = "matrix"
labels = [["A", "B"], ["A", "B"], ["A", "B"]]
payoffs = [
  [
    [[1.0, 1.0, 1.0], [0.0, 0.0, 0.0]],
    [[0.0, 0.0, 0.0], [1.0, 1.0, 1.0]],
  ],
  [
    [[0.0, 0.0, 0.0], [1.0, 1.0, 1.0]],
    [[1.0, 1.0, 1.0], [0.0, 0.0, 0.0]],
  ],
]
"#,
    );
    let population = value["world"]["population"].as_table_mut().unwrap();
    population.insert(
        "group".to_owned(),
        toml::Value::Array(vec![toml::Value::Table(toml::Table::from_iter([
            (
                "rule".to_owned(),
                toml::Value::String("fictitious_play".to_owned()),
            ),
            ("count".to_owned(), toml::Value::Integer(3)),
        ]))]),
    );
    set_matching(value, "whole_population");
}

fn grid_matching_without_grid(value: &mut toml::Value) {
    shrink_grid(value);
    let world = value["world"].as_table_mut().unwrap();
    for key in [
        "topology",
        "grid_rows",
        "grid_cols",
        "boundary",
        "neighborhood",
    ] {
        world.remove(key);
    }
    world.insert("n_agents".to_owned(), toml::Value::Integer(100));
}

fn network_matching_without_topology(value: &mut toml::Value) {
    set_matching(value, "network_edges");
}

fn dynamic_without_update(value: &mut toml::Value) {
    shrink_grid(value);
    params(value, "imitate_best").remove("update");
}

fn asynchronous_dynamic(value: &mut toml::Value) {
    shrink_grid(value);
    params(value, "imitate_best").insert(
        "update".to_owned(),
        toml::Value::String("asynchronous".to_owned()),
    );
}

fn dynamic_with_active_rule(value: &mut toml::Value) {
    shrink_grid(value);
    groups(value)[0]
        .as_table_mut()
        .unwrap()
        .insert("share".to_owned(), toml::Value::Float(0.5));
    groups(value).push(toml::Value::Table(toml::Table::from_iter([
        (
            "rule".to_owned(),
            toml::Value::String("tit_for_tat".to_owned()),
        ),
        ("share".to_owned(), toml::Value::Float(0.5)),
    ])));
    mechanism(value, "game_decide").insert(
        "params".to_owned(),
        toml::Value::Table(toml::Table::from_iter([(
            "coop_label".to_owned(),
            toml::Value::String("C".to_owned()),
        )])),
    );
}

fn dynamic_with_different_action_spaces(value: &mut toml::Value) {
    shrink_grid(value);
    set_game(
        value,
        r#"
[game]
kind = "matrix"
labels = [["C", "D", "E"], ["C", "D"]]
payoffs = [
  [[3.0, 3.0], [0.0, 5.0]],
  [[5.0, 0.0], [1.0, 1.0]],
  [[2.0, 2.0], [2.0, 2.0]],
]
"#,
    );
    let population = value["world"]["population"].as_table_mut().unwrap();
    population.remove("initial_shares");
    population.insert(
        "initial_action".to_owned(),
        toml::Value::String("C".to_owned()),
    );
}

fn dynamic_before_decide(value: &mut toml::Value) {
    shrink_grid(value);
    let entries = mechanisms(value);
    let dynamic_index = entries
        .iter()
        .position(|entry| entry["name"].as_str() == Some("imitate_best"))
        .unwrap();
    let dynamic = entries.remove(dynamic_index);
    let decide_index = entries
        .iter()
        .position(|entry| entry["name"].as_str() == Some("game_decide"))
        .unwrap();
    entries.insert(decide_index, dynamic);
}

fn per_step_round_robin(value: &mut toml::Value) {
    value["world"]["n_agents"] = toml::Value::Integer(3);
    groups(value)[1]
        .as_table_mut()
        .unwrap()
        .insert("count".to_owned(), toml::Value::Integer(2));
    set_matching(value, "round_robin");
}

fn mixed_count_and_share(value: &mut toml::Value) {
    let second = groups(value)[1].as_table_mut().unwrap();
    second.remove("count");
    second.insert("share".to_owned(), toml::Value::Float(0.5));
}

fn shares_above_one(value: &mut toml::Value) {
    shrink_grid(value);
    value["world"]["population"]["initial_shares"] =
        parsed("x = { C = 0.7, D = 0.4 }")["x"].clone();
}

fn non_ascii_label(value: &mut toml::Value) {
    value["world"]["game"]["labels"][0][0] = toml::Value::String("協力".to_owned());
}

fn invalid_prisoners_dilemma(value: &mut toml::Value) {
    shrink_grid(value);
    value["world"]["game"]["temptation"] = toml::Value::Float(0.9);
}

fn replicator_beta_too_high(value: &mut toml::Value) {
    shrink_grid(value);
    let entry = mechanism(value, "imitate_best");
    entry.insert(
        "name".to_owned(),
        toml::Value::String("replicator_update".to_owned()),
    );
    let params = entry["params"].as_table_mut().unwrap();
    params.clear();
    params.insert(
        "update".to_owned(),
        toml::Value::String("synchronous".to_owned()),
    );
    params.insert("beta".to_owned(), toml::Value::Float(1.0));
}

fn no_change(_: &mut toml::Value) {}

fn add_linear_fitness(value: &mut toml::Value) {
    params(value, "moran_update").insert(
        "fitness".to_owned(),
        toml::Value::String("linear".to_owned()),
    );
}

fn unknown_decide_key(value: &mut toml::Value) {
    params(value, "game_decide").insert("logit_lamda".to_owned(), toml::Value::Float(1.0));
}

fn unknown_mechanism(value: &mut toml::Value) {
    mechanism(value, "game_decide").insert(
        "name".to_owned(),
        toml::Value::String("game_decid".to_owned()),
    );
}

const MORAN_WITHOUT_FITNESS: &str = r#"
[simulation]
name = "moran_without_fitness"
module_pack = "normal-form"
t_max = 4
seed = 2
scheduler = "sequential"

[world]
topology = "none"
n_agents = 10
history_len = 1

[world.game]
kind = "prisoners_dilemma"
temptation = 1.85
reward = 1.0
punishment = 0.0
sucker = -10.0

[world.population]
initial_shares = { C = 0.5 }
[[world.population.group]]
rule = "held"
share = 1.0

[[mechanism]]
name = "game_match"
[mechanism.params]
matching = "random_pairs"

[[mechanism]]
name = "game_decide"

[[mechanism]]
name = "game_play"

[[mechanism]]
name = "moran_update"
[mechanism.params]
update = "synchronous"
w = 0.5

[output]
log_path = "unused.jsonl"
"#;

fn invalid_cases() -> Vec<InvalidCase> {
    vec![
        InvalidCase { name: "continuous game with finite rule", base: COURNOT, mutate: continuous_with_finite_rule, expected: "world.population.group[0].rule", rejects_unknown_key: false },
        InvalidCase { name: "continuous game with replicator", base: COURNOT, mutate: continuous_with_replicator, expected: "mechanism[replicator_update]", rejects_unknown_key: false },
        InvalidCase { name: "finite game with continuous rule", base: IPD, mutate: finite_with_continuous_rule, expected: "world.population.group[1].rule", rejects_unknown_key: false },
        InvalidCase { name: "nonanonymous counting true", base: IPD, mutate: nonanonymous_counting, expected: "mechanism[game_play].counting", rejects_unknown_key: true },
        InvalidCase { name: "continuous counting false", base: COURNOT, mutate: continuous_counting_false, expected: "mechanism[game_play].counting", rejects_unknown_key: true },
        InvalidCase { name: "three-action tit for tat", base: IPD, mutate: three_action_tit_for_tat, expected: "mechanism[game_decide]", rejects_unknown_key: false },
        InvalidCase { name: "three-action grim", base: IPD, mutate: three_action_grim, expected: "mechanism[game_decide]", rejects_unknown_key: false },
        InvalidCase { name: "three-role fictitious play", base: IPD, mutate: three_role_fictitious_play, expected: "mechanism[game_decide]", rejects_unknown_key: false },
        InvalidCase { name: "grid matching without grid", base: NOWAK_MAY, mutate: grid_matching_without_grid, expected: "mechanism[game_match]", rejects_unknown_key: false },
        InvalidCase { name: "network matching without topology", base: IPD, mutate: network_matching_without_topology, expected: "mechanism[game_match]", rejects_unknown_key: false },
        InvalidCase { name: "dynamic without update", base: NOWAK_MAY, mutate: dynamic_without_update, expected: "mechanism[imitate_best].update", rejects_unknown_key: false },
        InvalidCase { name: "asynchronous dynamic", base: NOWAK_MAY, mutate: asynchronous_dynamic, expected: "mechanism[imitate_best].update: expected one of \"synchronous\", found \"asynchronous\"", rejects_unknown_key: false },
        InvalidCase { name: "dynamic with active rule", base: NOWAK_MAY, mutate: dynamic_with_active_rule, expected: "mechanism[imitate_best]", rejects_unknown_key: false },
        InvalidCase { name: "dynamic with differing action spaces", base: NOWAK_MAY, mutate: dynamic_with_different_action_spaces, expected: "mechanism[imitate_best]", rejects_unknown_key: false },
        InvalidCase { name: "dynamic before decide", base: NOWAK_MAY, mutate: dynamic_before_decide, expected: "mechanism[imitate_best]", rejects_unknown_key: false },
        InvalidCase { name: "per-step round robin with three agents", base: IPD, mutate: per_step_round_robin, expected: "mechanism[game_match]", rejects_unknown_key: false },
        InvalidCase { name: "mixed count and share", base: IPD, mutate: mixed_count_and_share, expected: "world.population.group[1]", rejects_unknown_key: false },
        InvalidCase { name: "initial shares above one", base: NOWAK_MAY, mutate: shares_above_one, expected: "world.population.initial_shares", rejects_unknown_key: false },
        InvalidCase { name: "non-ASCII action label", base: IPD, mutate: non_ascii_label, expected: "world.game.labels[0][0]", rejects_unknown_key: false },
        InvalidCase { name: "invalid prisoners dilemma ordering", base: NOWAK_MAY, mutate: invalid_prisoners_dilemma, expected: "world.game", rejects_unknown_key: false },
        InvalidCase { name: "replicator beta too high", base: NOWAK_MAY, mutate: replicator_beta_too_high, expected: "mechanism[replicator_update]", rejects_unknown_key: false },
        InvalidCase { name: "moran without fitness", base: MORAN_WITHOUT_FITNESS, mutate: no_change, expected: "mechanism[moran_update].fitness", rejects_unknown_key: false },
        InvalidCase { name: "moran linear fitness can become negative", base: MORAN_WITHOUT_FITNESS, mutate: add_linear_fitness, expected: "mechanism[moran_update]", rejects_unknown_key: false },
        InvalidCase { name: "unknown decision key", base: IPD, mutate: unknown_decide_key, expected: "mechanism[game_decide].logit_lamda", rejects_unknown_key: false },
        InvalidCase { name: "unknown mechanism", base: IPD, mutate: unknown_mechanism, expected: "mechanism[game_decid]", rejects_unknown_key: false },
    ]
}

fn assert_expected_error(case: &InvalidCase, surface: &str, message: &str) {
    eprintln!("{} ({surface}): {message}", case.name);
    assert!(
        message.contains(case.expected),
        "{} ({surface}) should contain {:?}, got {message:?}",
        case.name,
        case.expected
    );
    if case.rejects_unknown_key {
        assert!(
            !message.contains("unknown key"),
            "{} ({surface}) treated counting as an unknown key: {message}",
            case.name
        );
    }
    if case.name == "asynchronous dynamic" {
        assert_eq!(
            message,
            "config error: mechanism[imitate_best].update: expected one of \"synchronous\", found \"asynchronous\"",
            "the asynchronous error must not suggest a mechanism that is not available"
        );
    }
}

#[test]
fn bundled_scenarios_validate_and_run_for_two_seeds() {
    let pack = NormalFormPack;
    for (name, text) in [("ipd", IPD), ("nowak_may", NOWAK_MAY), ("cournot", COURNOT)] {
        let scenario = Scenario::parse(text).unwrap_or_else(|error| panic!("{name}: {error}"));
        pack.validate(&scenario)
            .unwrap_or_else(|error| panic!("{name} validate: {error}"));
        let results = pack
            .run_trials(&scenario, &[0, 1], false)
            .unwrap_or_else(|error| panic!("{name} run_trials: {error}"));
        assert_eq!(results.len(), 2, "{name}");
    }
}

#[test]
fn starter_is_the_bundled_ipd_scenario_byte_for_byte() {
    assert_eq!(NORMAL_FORM_STARTER.as_bytes(), IPD.as_bytes());
}

#[test]
fn mechanism_names_are_exactly_the_non_llm_scenario_mechanisms() {
    assert_eq!(
        NormalFormPack.mechanism_names(),
        [
            "game_match",
            "game_decide",
            "game_play",
            "imitate_best",
            "fermi_update",
            "replicator_update",
            "moran_update",
            "game_metrics",
        ]
    );
}

#[test]
fn checked_sweep_delegates_world_paths_and_preserves_floats() {
    let mut value = parsed(NOWAK_MAY);
    shrink_grid(&mut value);
    value["simulation"]["t_max"] = toml::Value::Integer(2);
    let scenario = scenario(&value);
    let points = NormalFormPack
        .run_sweep(
            &scenario,
            &[SweepAxis {
                param_key: "world.game.temptation".to_owned(),
                values: vec![1.6, 1.9],
            }],
            &[0, 1],
            false,
        )
        .expect("the checked world-path sweep should run");
    assert_eq!(points.len(), 2);
    for (point, expected) in points.iter().zip([1.6, 1.9]) {
        assert_eq!(
            point.effective.world.0["game"]["temptation"],
            toml::Value::Float(expected)
        );
    }
}

#[test]
fn invalid_combinations_fail_on_validate_and_run_trials() {
    let pack = NormalFormPack;
    for case in invalid_cases() {
        let mut value = parsed(case.base);
        (case.mutate)(&mut value);
        let scenario = scenario(&value);

        let validate_error = pack
            .validate(&scenario)
            .expect_err("validate should reject the invalid combination")
            .to_string();
        assert_expected_error(&case, "validate", &validate_error);

        let run_error = pack
            .run_trials(&scenario, &[0], false)
            .expect_err("run_trials should reject the invalid combination")
            .to_string();
        assert_expected_error(&case, "run_trials", &run_error);
    }
}
