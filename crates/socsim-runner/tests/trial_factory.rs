//! Tests for `Trial`, `TrialFactory`, `RegistryTrialFactory`, `run_trial` and
//! `run_trials`.
//!
//! 1. `RegistryTrialFactory` over the `hr-lifecycle` pack reproduces
//!    `run_once` exactly.
//! 2. A factory can put `[[mechanism]]` params into the initial world.
//! 3. The engine seed chosen by the factory drives the trajectory, while
//!    `RunResult::seed` stays the caller's seed.
//! 4. `run_trials` returns identical results in parallel and sequential mode,
//!    sorted by seed, with a deterministic error choice — for a world that is
//!    not `Send`.

mod common;

use socsim_config::{ModulePack, Registry, Scenario};
use socsim_core::{SimRng, SocsimError};
use socsim_packs::hr_lifecycle::{HrLifecyclePack, HrWorld};
use socsim_runner::{
    run_once, run_seeds, run_trial, run_trials, scheduler_from_scenario, RegistryTrialFactory,
    Trial, TrialFactory, WorldFactory,
};

use common::{
    assert_same_result, assert_same_results, config_message, register_test, world_factory,
    EngineSeed, LevelWorld, LocalWorld, ScenarioFactory, TestWorld,
};

// ── fixtures ──────────────────────────────────────────────────────────────────

const BASELINE_TOML: &str = include_str!("../../../scenarios/hr_lifecycle_baseline.toml");

fn hr_world_factory() -> WorldFactory<HrWorld> {
    Box::new(|params, seed| {
        let n_teams = params.get_u64("n_teams", 5) as usize;
        let team_size = params.get_u64("team_size_initial", 8) as usize;
        let ws_k = params.get_u64("network_k", 4) as usize;
        let ws_beta = params.get_f64("network_beta", 0.1);
        let mut rng = SimRng::from_seed(seed);
        Ok(HrWorld::new(n_teams, team_size, ws_k, ws_beta, &mut rng))
    })
}

fn hr_register(reg: &mut Registry<HrWorld>) {
    HrLifecyclePack.register(reg);
}

/// `record_level` reads `init` (the factory puts it into the world) and
/// `draw` samples the engine RNG.
const LEVEL_TOML: &str = r#"
[simulation]
name        = "level"
module_pack = "test"
t_max       = 6
seed        = 0
scheduler   = "sequential"

[[mechanism]]
name = "record_level"
[mechanism.params]
init = 2.5

[[mechanism]]
name = "draw"

[output]
log_path = "unused.jsonl"
"#;

/// The same mechanisms for the registry-based (legacy) path.
const REGISTRY_TOML: &str = r#"
[simulation]
name        = "registry"
module_pack = "test"
t_max       = 4
seed        = 0
scheduler   = "random_activation"

[world]
level = 1.25

[[mechanism]]
name = "record_level"

[[mechanism]]
name = "draw"

[output]
log_path = "unused.jsonl"
"#;

fn values(r: &socsim_runner::RunResult, key: &str) -> Vec<f64> {
    r.series[key].iter().map(|&(_, v)| v).collect()
}

/// Compile-time proof that `LocalWorld` is not `Send` (the `assert_not_impl`
/// trick: for a `Send` type both impls apply and the inferred parameter is
/// ambiguous, so this would not compile).  It must name the concrete type; in
/// a generic function only the first impl is ever selectable.
trait AmbiguousIfSend<A> {
    fn some_item() {}
}
impl<T: ?Sized> AmbiguousIfSend<()> for T {}
impl<T: ?Sized + Send> AmbiguousIfSend<u8> for T {}

fn assert_local_world_is_not_send() {
    <LocalWorld as AmbiguousIfSend<_>>::some_item();
}

// ── (1) RegistryTrialFactory reproduces run_once ──────────────────────────────

#[test]
fn registry_factory_matches_run_once_on_hr_lifecycle() {
    let scenario = Scenario::parse(BASELINE_TOML).unwrap();
    let legacy_factory = hr_world_factory();
    let factory = RegistryTrialFactory::new(hr_world_factory(), hr_register);

    for seed in [0u64, 7, 42] {
        let legacy = run_once(&scenario, &legacy_factory, &hr_register, seed).unwrap();
        let new = run_trial(&scenario, &factory, seed).unwrap();
        assert_eq!(new.seed, seed);
        assert!(new.event_count > 0, "hr-lifecycle records events");
        assert_same_result(&new, &legacy);
    }
}

#[test]
fn registry_factory_run_trials_matches_run_seeds_on_hr_lifecycle() {
    let scenario = Scenario::parse(BASELINE_TOML).unwrap();
    let legacy_factory = hr_world_factory();
    let factory = RegistryTrialFactory::new(hr_world_factory(), hr_register);
    let seeds = [42u64, 0, 7];

    for parallel in [false, true] {
        let legacy = run_seeds(&scenario, &legacy_factory, &hr_register, seeds, parallel).unwrap();
        let new = run_trials(&scenario, &factory, &seeds, parallel).unwrap();
        assert_same_results(&new, &legacy);
    }
}

#[test]
fn registry_factory_matches_run_once_on_test_world() {
    let scenario = Scenario::parse(REGISTRY_TOML).unwrap();
    let legacy_factory = world_factory::<TestWorld>();
    let factory = RegistryTrialFactory::new(world_factory::<TestWorld>(), register_test);

    for seed in [0u64, 1, 99] {
        let legacy = run_once(&scenario, &legacy_factory, &register_test, seed).unwrap();
        let new = run_trial(&scenario, &factory, seed).unwrap();
        assert_eq!(values(&new, "level"), vec![1.25; 4]);
        assert_same_result(&new, &legacy);
    }
}

// ── RegistryTrialFactory::validate ────────────────────────────────────────────

#[test]
fn registry_factory_validate_accepts_valid_scenario() {
    let scenario = Scenario::parse(REGISTRY_TOML).unwrap();
    let factory = RegistryTrialFactory::new(world_factory::<TestWorld>(), register_test);
    factory.validate(&scenario).expect("valid scenario");
}

#[test]
fn registry_factory_validate_rejects_unregistered_mechanism() {
    let text = REGISTRY_TOML.replace("name = \"draw\"", "name = \"drew\"");
    let scenario = Scenario::parse(&text).unwrap();
    let factory = RegistryTrialFactory::new(world_factory::<TestWorld>(), register_test);

    let msg = config_message(factory.validate(&scenario));
    assert!(msg.contains("drew"), "{msg}");
    let msg = config_message(run_trial(&scenario, &factory, 0));
    assert!(msg.contains("drew"), "{msg}");

    // The legacy path does not validate; it fails while building instead.
    let legacy = run_once(&scenario, &world_factory::<TestWorld>(), &register_test, 0);
    assert!(matches!(legacy, Err(SocsimError::UnknownMechanism(ref n)) if n == "drew"));
}

#[test]
fn registry_factory_validate_rejects_bad_scheduler() {
    let text = REGISTRY_TOML.replace("random_activation", "bogus");
    let scenario = Scenario::parse(&text).unwrap();
    let factory = RegistryTrialFactory::new(world_factory::<TestWorld>(), register_test);
    let msg = config_message(factory.validate(&scenario));
    assert!(msg.contains("bogus"), "{msg}");
}

#[test]
fn registry_factory_t_max_zero_is_an_error_but_run_once_runs_zero_steps() {
    let text = REGISTRY_TOML.replace("t_max       = 4", "t_max       = 0");
    let scenario = Scenario::parse(&text).unwrap();
    let factory = RegistryTrialFactory::new(world_factory::<TestWorld>(), register_test);

    let msg = config_message(run_trial(&scenario, &factory, 0));
    assert!(msg.contains("t_max"), "{msg}");

    let legacy = run_once(&scenario, &world_factory::<TestWorld>(), &register_test, 0).unwrap();
    assert!(legacy.series.is_empty());
    assert_eq!(legacy.event_count, 0);
}

// ── scheduler_from_scenario ───────────────────────────────────────────────────

#[test]
fn scheduler_from_scenario_known_strings() {
    for name in ["sequential", "random_activation"] {
        let text = REGISTRY_TOML.replace("random_activation", name);
        let scenario = Scenario::parse(&text).unwrap();
        assert!(scheduler_from_scenario::<TestWorld>(&scenario).is_ok());
    }
}

#[test]
fn scheduler_from_scenario_unknown_string_matches_run_once_error() {
    let text = REGISTRY_TOML.replace("random_activation", "bogus");
    let scenario = Scenario::parse(&text).unwrap();

    let msg = config_message(scheduler_from_scenario::<TestWorld>(&scenario).map(|_| ()));
    assert_eq!(msg, "unknown scheduler 'bogus'");

    let legacy = run_once(&scenario, &world_factory::<TestWorld>(), &register_test, 0);
    assert_eq!(config_message(legacy), "unknown scheduler 'bogus'");
}

// ── (2) mechanism params into the initial world ───────────────────────────────

#[test]
fn factory_puts_mechanism_params_into_world() {
    let factory = ScenarioFactory::<LocalWorld>::new(EngineSeed::Derived);

    let scenario = Scenario::parse(LEVEL_TOML).unwrap();
    let r = run_trial(&scenario, &factory, 0).unwrap();
    assert_eq!(values(&r, "level"), vec![2.5; 6]);
    assert_eq!(r.final_metrics["level"], 2.5);

    let scenario = Scenario::parse(&LEVEL_TOML.replace("init = 2.5", "init = 7.0")).unwrap();
    let r = run_trial(&scenario, &factory, 0).unwrap();
    assert_eq!(values(&r, "level"), vec![7.0; 6]);
    assert_eq!(r.final_metrics["level"], 7.0);
}

// ── (3) the factory's engine seed drives the trajectory ───────────────────────

#[test]
fn engine_seed_from_factory_drives_trajectory() {
    let scenario = Scenario::parse(LEVEL_TOML).unwrap();
    let derived = ScenarioFactory::<LocalWorld>::new(EngineSeed::Derived);
    let fixed = ScenarioFactory::<LocalWorld>::new(EngineSeed::Fixed(12345));

    let a0 = run_trial(&scenario, &derived, 0).unwrap();
    let a1 = run_trial(&scenario, &derived, 1).unwrap();
    let b0 = run_trial(&scenario, &fixed, 0).unwrap();
    let b1 = run_trial(&scenario, &fixed, 1).unwrap();

    // RunResult::seed is the caller's seed, never the engine seed.
    assert_eq!((a0.seed, a1.seed, b0.seed, b1.seed), (0, 1, 0, 1));

    let draws = |r: &socsim_runner::RunResult| values(r, "draw");
    assert_eq!(draws(&a0).len(), 6);
    assert!(draws(&a0).iter().all(|u| (0.0..1.0).contains(u)));

    // Same engine seed (12345) → same trajectory, whatever the trial seed.
    assert_eq!(draws(&b0), draws(&b1));
    assert_eq!(b0.series, b1.series);
    // Different engine seeds → different trajectories.
    assert_ne!(draws(&a0), draws(&a1));
    assert_ne!(draws(&a0), draws(&b0));
}

#[test]
fn raw_engine_seed_reproduces_registry_path() {
    // A factory returning the raw seed walks the same engine stream as the
    // registry-based path (same mechanisms, same scheduler).
    let scenario = Scenario::parse(LEVEL_TOML).unwrap();
    let raw = ScenarioFactory::<TestWorld>::new(EngineSeed::Raw);
    let legacy = run_once(&scenario, &world_factory::<TestWorld>(), &register_test, 5).unwrap();
    let new = run_trial(&scenario, &raw, 5).unwrap();
    assert_eq!(values(&new, "draw"), values(&legacy, "draw"));
}

// ── (4) run_trials ────────────────────────────────────────────────────────────

#[test]
fn run_trials_parallel_equals_sequential_and_sorts_by_seed() {
    assert_local_world_is_not_send();

    let scenario = Scenario::parse(LEVEL_TOML).unwrap();
    let factory = ScenarioFactory::<LocalWorld>::new(EngineSeed::Derived);
    let seeds = [3u64, 1, 2, 1];

    let seq = run_trials(&scenario, &factory, &seeds, false).unwrap();
    let par = run_trials(&scenario, &factory, &seeds, true).unwrap();

    let order: Vec<u64> = seq.iter().map(|r| r.seed).collect();
    assert_eq!(order, vec![1, 1, 2, 3]);
    assert_same_results(&seq, &par);

    // Each result equals the single-trial run for its seed.
    for r in &seq {
        assert_same_result(r, &run_trial(&scenario, &factory, r.seed).unwrap());
    }
}

#[test]
fn run_trials_validates_once_and_builds_per_seed() {
    let scenario = Scenario::parse(LEVEL_TOML).unwrap();
    let factory = ScenarioFactory::<LocalWorld>::new(EngineSeed::Derived);

    run_trials(&scenario, &factory, &[0, 1, 2, 3], true).unwrap();
    assert_eq!(factory.validations(), 1);
    assert_eq!(factory.builds(), 4);

    run_trial(&scenario, &factory, 9).unwrap();
    assert_eq!(factory.validations(), 2);
    assert_eq!(factory.builds(), 5);
}

#[test]
fn run_trials_validation_failure_runs_nothing() {
    let scenario = Scenario::parse(LEVEL_TOML).unwrap();
    let factory = ScenarioFactory::<LocalWorld>::new(EngineSeed::Derived)
        .with_validator(|_| Err(SocsimError::Config("rejected by factory".to_owned())));

    let msg = config_message(run_trials(&scenario, &factory, &[0, 1], true));
    assert!(msg.contains("rejected by factory"), "{msg}");
    assert_eq!(factory.builds(), 0);
}

#[test]
fn run_trials_returns_first_failure_in_input_order() {
    let scenario = Scenario::parse(LEVEL_TOML).unwrap();
    for parallel in [false, true] {
        let factory = ScenarioFactory::<LocalWorld>::new(EngineSeed::Derived).failing_on(&[2, 3]);
        let msg = config_message(run_trials(&scenario, &factory, &[1, 3, 2], parallel));
        assert!(msg.contains("trial seed 3"), "parallel={parallel}: {msg}");
        assert!(msg.contains("injected failure for seed 3"), "{msg}");
        // Every trial still ran.
        assert_eq!(factory.builds(), 3);
    }
}

// ── TrialFactory surface ──────────────────────────────────────────────────────

/// A factory relying on the default `validate`.
struct MinimalFactory;

impl TrialFactory<LocalWorld> for MinimalFactory {
    fn build_trial(
        &self,
        scenario: &Scenario,
        seed: u64,
    ) -> socsim_core::Result<Trial<LocalWorld>> {
        let scheduler = scheduler_from_scenario(scenario)?;
        let trial = Trial::new(
            LocalWorld::with_level(4.0),
            vec![Box::new(common::RecordLevel { offset: 0.5 })],
            scheduler,
            seed,
        );
        assert_eq!(trial.engine_seed, seed);
        assert_eq!(trial.mechanisms.len(), 1);
        Ok(trial)
    }
}

#[test]
fn default_validate_accepts_everything_and_factories_are_object_safe() {
    // t_max = 0 passes the default validate and runs zero steps.
    let zero = Scenario::parse(&LEVEL_TOML.replace("t_max       = 6", "t_max       = 0")).unwrap();
    let factory: &dyn TrialFactory<LocalWorld> = &MinimalFactory;
    factory.validate(&zero).unwrap();
    let r = run_trial(&zero, factory, 3).unwrap();
    assert!(r.series.is_empty());
    assert_eq!(r.seed, 3);

    let scenario = Scenario::parse(LEVEL_TOML).unwrap();
    let boxed: Box<dyn TrialFactory<LocalWorld>> = Box::new(MinimalFactory);
    let rs = run_trials(&scenario, boxed.as_ref(), &[2, 0], true).unwrap();
    assert_eq!(rs.iter().map(|r| r.seed).collect::<Vec<_>>(), vec![0, 2]);
    assert_eq!(values(&rs[0], "level"), vec![4.5; 6]);
}
