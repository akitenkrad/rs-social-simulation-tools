use socsim_config::Scenario;
use socsim_core::SocsimError;
use socsim_games::{FiniteSpec, GameTrialFactory, RuleRegistry};
use socsim_runner::{run_trial, TrialFactory};

fn scenario_with_continuation_params(params: Option<&str>) -> Scenario {
    let continuation = params
        .map(|params| {
            format!(
                "[[mechanism]]\nname = \"continuation_probability\"\n[mechanism.params]\n{params}\n"
            )
        })
        .unwrap_or_default();
    Scenario::parse(&format!(
        r#"
[simulation]
name = "stochastic_ipd"
module_pack = "normal-form"
t_max = 100
seed = 42
scheduler = "sequential"

[world]
n_agents = 2
history_len = 100

[world.game]
kind = "matrix"
labels = [["C", "D"], ["C", "D"]]
payoffs = [
  [[3.0, 3.0], [0.0, 5.0]],
  [[5.0, 0.0], [1.0, 1.0]],
]

[world.population]
[[world.population.group]]
rule = "uniform_random"
count = 2

[[mechanism]]
name = "game_match"
[mechanism.params]
matching = "fixed_players"
pairs = [[0, 1]]

[[mechanism]]
name = "game_decide"

[[mechanism]]
name = "game_play"

[[mechanism]]
name = "game_metrics"
[mechanism.params]
cooperate_label = "C"

{continuation}
[output]
log_path = "runs/{{name}}_{{seed}}.jsonl"
"#
    ))
    .unwrap()
}

fn scenario(delta: Option<&str>) -> Scenario {
    scenario_with_continuation_params(delta.map(|delta| format!("delta = {delta}")).as_deref())
}

fn factory() -> GameTrialFactory<FiniteSpec> {
    GameTrialFactory::new(RuleRegistry::finite_builtins())
}

fn config_message<T>(result: socsim_core::Result<T>) -> String {
    match result {
        Err(SocsimError::Config(message)) => message,
        Err(other) => panic!("expected a configuration error, got {other:?}"),
        Ok(_) => panic!("expected a configuration error, got Ok"),
    }
}

#[test]
fn validate_rejects_invalid_continuation_probabilities() {
    for (delta, reason) in [
        ("-0.1", "[0, 1]"),
        ("1.1", "[0, 1]"),
        ("nan", "finite"),
        ("\"0.9\"", "number"),
    ] {
        let message = config_message(factory().validate(&scenario(Some(delta))));
        assert!(
            message.starts_with("mechanism[continuation_probability].delta"),
            "{message}"
        );
        assert!(message.contains(reason), "{message}");
    }
}

#[test]
fn validate_requires_delta_and_rejects_unknown_keys() {
    let missing = scenario_with_continuation_params(Some(""));
    let message = config_message(factory().validate(&missing));
    assert!(
        message.starts_with("mechanism[continuation_probability].delta"),
        "{message}"
    );

    let unknown = scenario_with_continuation_params(Some("delta = 0.9\nprobability = 0.9"));
    let message = config_message(factory().validate(&unknown));
    assert!(
        message.starts_with("mechanism[continuation_probability].probability"),
        "{message}"
    );
    assert!(message.contains("unknown key"), "{message}");
}

#[test]
fn stochastic_ipd_matches_the_run_without_stopping_through_the_stop_step() {
    let with_continuation = scenario(Some("0.9"));
    let without_continuation = scenario(None);
    let factory = factory();
    let (seed, shortened) = (0..1_000)
        .find_map(|seed| {
            let result = run_trial(&with_continuation, &factory, seed).unwrap();
            let stop = result
                .events
                .iter()
                .find(|event| event.kind == "continuation_stop")?;
            (stop.t < 100).then_some((seed, result))
        })
        .expect("a seed that stops before t_max");
    let full = run_trial(&without_continuation, &factory, seed).unwrap();
    let stop_t = shortened
        .events
        .iter()
        .find(|event| event.kind == "continuation_stop")
        .unwrap()
        .t;

    assert!(!shortened.series.is_empty());
    assert_eq!(shortened.series.len(), full.series.len());
    for (key, shortened_values) in &shortened.series {
        let full_values = &full.series[key];
        assert_eq!(
            shortened_values,
            &full_values[..shortened_values.len()],
            "metric {key} diverged before stop at t={stop_t}"
        );
        assert_eq!(shortened_values.last().unwrap().0, stop_t);
    }
}

#[test]
fn same_seed_produces_byte_identical_run_results() {
    let scenario = scenario(Some("0.9"));
    let factory = factory();
    let first = run_trial(&scenario, &factory, 73).unwrap();
    let second = run_trial(&scenario, &factory, 73).unwrap();

    assert_eq!(
        serde_json::to_vec(&first).unwrap(),
        serde_json::to_vec(&second).unwrap()
    );
    let event = first
        .events
        .iter()
        .find(|event| event.kind == "continuation_stop")
        .expect("the selected seed stops before t_max");
    assert_eq!(event.payload["delta"], 0.9);
    assert!(event.payload["u"].as_f64().unwrap() >= 0.9);
}
