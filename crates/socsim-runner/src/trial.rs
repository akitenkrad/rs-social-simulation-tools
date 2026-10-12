//! Trials built from a whole [`Scenario`]: [`Trial`], [`TrialFactory`],
//! [`RegistryTrialFactory`], [`run_trial`] and [`run_trials`].
//!
//! The legacy entry points ([`run_once`](crate::run_once) and friends) and the
//! factory-based ones share two private functions: the registry-based trial
//! assembly and the execution kernel (clock reset → engine → in-memory
//! recorder → [`RunResult`] extraction), so both paths run a trial in exactly
//! the same way.

use std::collections::HashMap;

use rayon::prelude::*;

use socsim_config::{Registry, Scenario};
use socsim_core::{Mechanism, Result, Scheduler, SimClock, SocsimError, WorldState};
use socsim_engine::{RandomActivationScheduler, SequentialScheduler, SimulationBuilder};
use socsim_log::{EventRow, InMemoryRecorder};

use crate::{RunResult, WorldFactory};

// ── Trial ─────────────────────────────────────────────────────────────────────

/// The parts of one simulation trial, produced by a [`TrialFactory`].
///
/// The struct is `#[non_exhaustive]`: its fields can be read anywhere, but code
/// outside this crate must build it with [`Trial::new`], so fields can be added
/// later without breaking implementors.
///
/// The runner always attaches its own in-memory recorder when executing a
/// trial (that is how a [`RunResult`] is collected); a trial carries no
/// recorder of its own.
#[non_exhaustive]
pub struct Trial<W: WorldState> {
    /// The initial world.  Its clock is reset to
    /// `SimClock::new(scenario.simulation.t_max)` before the run, so the
    /// factory does not need to set it.
    pub world: W,

    /// Mechanisms in registration order, which is also the execution order
    /// within a phase.
    pub mechanisms: Vec<Box<dyn Mechanism<W>>>,

    /// The activation scheduler (see [`scheduler_from_scenario`] for the
    /// scenario's `scheduler` string).
    pub scheduler: Box<dyn Scheduler<W>>,

    /// The seed handed to the engine (`SimulationBuilder::seed`), i.e. the
    /// root of the engine RNG stream that mechanisms draw from via
    /// `StepContext::rng`.
    ///
    /// This is chosen by the factory and may differ from the trial seed the
    /// caller passed in.  [`RunResult::seed`] always reports the caller's
    /// seed, never this value.
    pub engine_seed: u64,
}

impl<W: WorldState> Trial<W> {
    /// Assemble a trial from its parts.
    pub fn new(
        world: W,
        mechanisms: Vec<Box<dyn Mechanism<W>>>,
        scheduler: Box<dyn Scheduler<W>>,
        engine_seed: u64,
    ) -> Self {
        Self {
            world,
            mechanisms,
            scheduler,
            engine_seed,
        }
    }
}

// ── TrialFactory ──────────────────────────────────────────────────────────────

/// Builds every part of a trial from the whole effective [`Scenario`] and a
/// trial seed.
///
/// Unlike a [`WorldFactory`], which sees only `[world]`, a trial factory can
/// read `[world]` and every `[[mechanism]]` entry at once, so the initial world
/// may depend on mechanism parameters.  It also chooses the engine seed
/// ([`Trial::engine_seed`]).
///
/// The trait is object safe; the runner functions take `&dyn TrialFactory<W>`.
/// A factory is shared between worker threads (`Send + Sync`), but the trial it
/// builds never leaves the thread that runs it.
pub trait TrialFactory<W: WorldState>: Send + Sync {
    /// Check the whole scenario (pack-specific combination rules) before any
    /// trial is built.  The default accepts every scenario.
    fn validate(&self, scenario: &Scenario) -> Result<()> {
        let _ = scenario;
        Ok(())
    }

    /// Build the world, mechanisms, scheduler and engine seed for `seed`.
    fn build_trial(&self, scenario: &Scenario, seed: u64) -> Result<Trial<W>>;
}

// ── RegistryTrialFactory ──────────────────────────────────────────────────────

/// Adapts an existing ([`WorldFactory`], register) pair to a [`TrialFactory`].
///
/// [`build_trial`](TrialFactory::build_trial) builds the parts in the same
/// order as [`run_once`](crate::run_once): the world from `[world]`, a fresh
/// [`Registry`] filled by `register`, one mechanism per `[[mechanism]]` entry
/// in scenario order, then the scheduler.  The registry is created anew on
/// every call (a registry is neither `Send` nor `Sync`).
///
/// # Seeds
///
/// For compatibility with the existing packs, the **raw** trial seed is passed
/// both to the world factory and to the engine (`engine_seed == seed`), so
/// [`run_trial`] reproduces [`run_once`](crate::run_once) bit for bit on any
/// scenario that passes validation.  New factories should instead keep the
/// world-initialisation and engine streams apart, e.g. by seeding the world
/// from `derive_seed(seed, &[0])` and returning
/// `engine_seed = derive_seed(seed, &[1])`.
///
/// # Validation
///
/// [`validate`](TrialFactory::validate) runs [`Scenario::validate`] with the
/// names registered by `register`: `t_max > 0`, a known scheduler string, and
/// every mechanism name registered.  [`run_once`](crate::run_once) performs no
/// validation, so a scenario with `t_max = 0` runs zero steps there but is an
/// error through [`run_trial`].
pub struct RegistryTrialFactory<W: WorldState> {
    world_factory: WorldFactory<W>,
    register: RegisterFn<W>,
}

/// An owned, thread-shareable registry-populating closure.
type RegisterFn<W> = Box<dyn Fn(&mut Registry<W>) + Send + Sync>;

impl<W: WorldState> RegistryTrialFactory<W> {
    /// Wrap an owned world factory and a registry-populating closure.
    pub fn new(
        world_factory: WorldFactory<W>,
        register: impl Fn(&mut Registry<W>) + Send + Sync + 'static,
    ) -> Self {
        Self {
            world_factory,
            register: Box::new(register),
        }
    }
}

impl<W: WorldState> TrialFactory<W> for RegistryTrialFactory<W> {
    fn validate(&self, scenario: &Scenario) -> Result<()> {
        let mut registry: Registry<W> = Registry::new();
        (self.register)(&mut registry);
        let names = registry.names();
        scenario.validate(&names)
    }

    fn build_trial(&self, scenario: &Scenario, seed: u64) -> Result<Trial<W>> {
        build_registry_trial(scenario, &self.world_factory, &*self.register, seed)
    }
}

// ── scheduler_from_scenario ───────────────────────────────────────────────────

/// Select the scheduler named by `scenario.simulation.scheduler`:
/// `"sequential"` or `"random_activation"`.
///
/// # Errors
///
/// Any other string yields `SocsimError::Config("unknown scheduler '<s>'")`,
/// the same error [`run_once`](crate::run_once) returns.
pub fn scheduler_from_scenario<W: WorldState>(
    scenario: &Scenario,
) -> Result<Box<dyn Scheduler<W>>> {
    match scenario.simulation.scheduler.as_str() {
        "random_activation" => Ok(Box::new(RandomActivationScheduler)),
        "sequential" => Ok(Box::new(SequentialScheduler)),
        other => Err(SocsimError::Config(format!("unknown scheduler '{other}'"))),
    }
}

// ── shared assembly and kernel (private) ──────────────────────────────────────

/// Registry-based trial assembly shared by [`run_once`](crate::run_once) and
/// [`RegistryTrialFactory`]: world → registry → mechanisms (scenario order) →
/// scheduler.  The raw `seed` feeds both the world factory and the engine.
/// The clock is not touched here; [`execute_trial`] resets it.
pub(crate) fn build_registry_trial<W: WorldState>(
    scenario: &Scenario,
    world_factory: &WorldFactory<W>,
    register: &dyn Fn(&mut Registry<W>),
    seed: u64,
) -> Result<Trial<W>> {
    let world_params = scenario.world.to_params();
    let world = world_factory(&world_params, seed)?;

    let mut registry: Registry<W> = Registry::new();
    register(&mut registry);

    let mut mechanisms: Vec<Box<dyn Mechanism<W>>> = Vec::with_capacity(scenario.mechanisms.len());
    for entry in &scenario.mechanisms {
        let params = entry.params.to_params();
        mechanisms.push(registry.build(&entry.name, &params)?);
    }

    let scheduler = scheduler_from_scenario(scenario)?;

    Ok(Trial::new(world, mechanisms, scheduler, seed))
}

/// The execution kernel shared by every runner entry point.
///
/// Resets the world clock to `t_max`, builds the engine with
/// `trial.engine_seed`, adds the mechanisms in order, sets the scheduler,
/// attaches an [`InMemoryRecorder`], runs to completion and extracts the
/// [`RunResult`].  `seed` is the caller's trial seed and is what
/// [`RunResult::seed`] reports.
pub(crate) fn execute_trial<W: WorldState>(
    scenario: &Scenario,
    trial: Trial<W>,
    seed: u64,
) -> Result<RunResult> {
    let Trial {
        mut world,
        mechanisms,
        scheduler,
        engine_seed,
    } = trial;

    // World factories (e.g. HrWorld::new) may set t_max = u64::MAX, so the
    // clock is overridden with the scenario's t_max (time also resets to 0).
    *world.clock_mut() = SimClock::new(scenario.simulation.t_max);

    let mut builder = SimulationBuilder::new(world).seed(engine_seed);
    for mech in mechanisms {
        builder = builder.add_mechanism(mech);
    }
    builder = builder.scheduler(scheduler);

    // Use InMemoryRecorder so we can inspect results after the run.
    builder = builder.recorder(Box::new(InMemoryRecorder::new()));

    let mut sim = builder.build();
    sim.run()?;

    // Downcast recorder to extract data.
    let rec = sim
        .recorder()
        .as_any()
        .and_then(|a| a.downcast_ref::<InMemoryRecorder>())
        .ok_or_else(|| SocsimError::Mechanism("recorder downcast failed".to_owned()))?;

    let mut series: HashMap<String, Vec<(u64, f64)>> = HashMap::new();
    for row in rec.metrics() {
        series
            .entry(row.key.clone())
            .or_default()
            .push((row.t, row.value));
    }

    let mut final_metrics: HashMap<String, f64> = HashMap::new();
    for (key, ts) in &series {
        if let Some(&(_, last_val)) = ts.last() {
            final_metrics.insert(key.clone(), last_val);
        }
    }

    let events: Vec<EventRow> = rec.events().to_vec();
    let event_count = events.len();

    Ok(RunResult {
        seed,
        series,
        final_metrics,
        events,
        event_count,
    })
}

// ── run_trial / run_trials ────────────────────────────────────────────────────

/// Run one trial: [`TrialFactory::validate`] → [`TrialFactory::build_trial`] →
/// execute (clock reset to `t_max`, in-memory recorder, run, extract).
///
/// [`RunResult::seed`] is `seed`, not the factory's engine seed.
pub fn run_trial<W: WorldState>(
    scenario: &Scenario,
    factory: &dyn TrialFactory<W>,
    seed: u64,
) -> Result<RunResult> {
    factory.validate(scenario)?;
    let trial = factory.build_trial(scenario, seed)?;
    execute_trial(scenario, trial, seed)
}

/// Run one trial per seed, sequentially or in parallel with `rayon`.
///
/// [`TrialFactory::validate`] is called once, before any trial runs.  Each
/// trial is then built by [`TrialFactory::build_trial`] **on the thread that
/// runs it**; a [`Trial`] never crosses threads, so `W` need not be `Send`.
///
/// # Ordering
///
/// Results are stably sorted by seed value, as in
/// [`run_seeds`](crate::run_seeds); duplicated seeds keep their input order.
/// Sequential and parallel runs return identical results.
///
/// # Errors
///
/// A validation failure is returned as is.  If any trial fails, every trial
/// still runs, and the error of the failed seed that comes **first in the
/// input order** is returned (deterministic even in parallel mode), as
/// `SocsimError::Config("trial seed <s>: <error>")`.
pub fn run_trials<W: WorldState>(
    scenario: &Scenario,
    factory: &dyn TrialFactory<W>,
    seeds: &[u64],
    parallel: bool,
) -> Result<Vec<RunResult>> {
    factory.validate(scenario)?;
    run_validated_trials(scenario, factory, seeds, parallel)
}

/// [`run_trials`] without the validation call (the caller has validated).
pub(crate) fn run_validated_trials<W: WorldState>(
    scenario: &Scenario,
    factory: &dyn TrialFactory<W>,
    seeds: &[u64],
    parallel: bool,
) -> Result<Vec<RunResult>> {
    let run_one = |seed: u64| -> Result<RunResult> {
        let trial = factory.build_trial(scenario, seed)?;
        execute_trial(scenario, trial, seed)
    };

    let outcomes: Vec<Result<RunResult>> = if parallel {
        seeds.par_iter().map(|&s| run_one(s)).collect()
    } else {
        seeds.iter().map(|&s| run_one(s)).collect()
    };

    let mut results = Vec::with_capacity(seeds.len());
    for (&seed, outcome) in seeds.iter().zip(outcomes) {
        match outcome {
            Ok(r) => results.push(r),
            Err(e) => return Err(SocsimError::Config(format!("trial seed {seed}: {e}"))),
        }
    }

    results.sort_by_key(|r| r.seed);
    Ok(results)
}
