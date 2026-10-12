//! Test worlds, mechanisms and factories shared by `trial_factory.rs` and
//! `trial_sweep.rs`.

#![allow(dead_code)]

use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use socsim_config::{Registry, Scenario};
use socsim_core::{
    derive_seed, AgentId, Mechanism, Phase, Result, Scheduler, SimClock, SocsimError, StepContext,
    WorldState,
};
use socsim_engine::RandomActivationScheduler;
use socsim_runner::{scheduler_from_scenario, RunResult, Trial, TrialFactory, WorldFactory};

/// Number of agents in every test world.
pub const N_AGENTS: u64 = 8;

// ── worlds ────────────────────────────────────────────────────────────────────

/// A test world whose only state is a scalar `level`.
pub trait LevelWorld: WorldState + Sized {
    fn with_level(level: f64) -> Self;
    fn level(&self) -> f64;
}

/// A `Send` test world (usable with the legacy `run_seeds` / `run_sweep`).
pub struct TestWorld {
    clock: SimClock,
    agents: Vec<AgentId>,
    level: f64,
}

impl WorldState for TestWorld {
    fn agent_ids(&self) -> Vec<AgentId> {
        self.agents.clone()
    }
    fn clock(&self) -> &SimClock {
        &self.clock
    }
    fn clock_mut(&mut self) -> &mut SimClock {
        &mut self.clock
    }
}

impl LevelWorld for TestWorld {
    fn with_level(level: f64) -> Self {
        Self {
            clock: SimClock::new(u64::MAX),
            agents: (0..N_AGENTS).map(AgentId).collect(),
            level,
        }
    }
    fn level(&self) -> f64 {
        self.level
    }
}

/// A test world that is **not** `Send` (it holds an `Rc`).
pub struct LocalWorld {
    clock: SimClock,
    agents: Vec<AgentId>,
    level: f64,
    rc: Rc<()>,
}

impl WorldState for LocalWorld {
    fn agent_ids(&self) -> Vec<AgentId> {
        self.agents.clone()
    }
    fn clock(&self) -> &SimClock {
        &self.clock
    }
    fn clock_mut(&mut self) -> &mut SimClock {
        &mut self.clock
    }
}

impl LevelWorld for LocalWorld {
    fn with_level(level: f64) -> Self {
        Self {
            clock: SimClock::new(u64::MAX),
            agents: (0..N_AGENTS).map(AgentId).collect(),
            level,
            rc: Rc::new(()),
        }
    }
    fn level(&self) -> f64 {
        let _ = Rc::strong_count(&self.rc);
        self.level
    }
}

// ── mechanisms ────────────────────────────────────────────────────────────────

/// Records `level + offset` under the metric `level` every step.
pub struct RecordLevel {
    pub offset: f64,
}

impl<W: LevelWorld> Mechanism<W> for RecordLevel {
    fn name(&self) -> &str {
        "record_level"
    }
    fn phases(&self) -> &'static [Phase] {
        &[Phase::PostStep]
    }
    fn apply(&mut self, _phase: Phase, ctx: &mut StepContext<'_, W>) -> Result<()> {
        let v = ctx.world.level() + self.offset;
        ctx.recorder.record_metric(ctx.clock.t(), "level", v);
        Ok(())
    }
}

/// Draws a uniform number in `[0, 1)` from the engine RNG (`ctx.rng`) and
/// records it under the metric `draw` every step.
///
/// The draw is a random permutation of the agents, obtained through the public
/// `Scheduler` trait (the runner has no direct `rand` dependency), read as the
/// base-`N_AGENTS` fraction `0.p0 p1 p2 …`.  Its trajectory is a function of
/// the engine seed only.
pub struct Draw;

impl<W: LevelWorld> Mechanism<W> for Draw {
    fn name(&self) -> &str {
        "draw"
    }
    fn phases(&self) -> &'static [Phase] {
        &[Phase::Decision]
    }
    fn apply(&mut self, _phase: Phase, ctx: &mut StepContext<'_, W>) -> Result<()> {
        let mut shuffler = RandomActivationScheduler;
        let perm = shuffler.activation_order(&*ctx.world, ctx.rng);
        let base = N_AGENTS as f64;
        let mut scale = 1.0 / base;
        let mut u = 0.0;
        for id in perm {
            u += id.0 as f64 * scale;
            scale /= base;
        }
        ctx.recorder.record_metric(ctx.clock.t(), "draw", u);
        Ok(())
    }
}

fn record_level_from(params: &socsim_config::Params) -> RecordLevel {
    RecordLevel {
        offset: params.get_f64("offset", 0.0),
    }
}

/// Registry for the legacy API: `record_level` and `draw`.
pub fn register_test<W: LevelWorld>(reg: &mut Registry<W>) {
    reg.register("record_level", |p| Ok(Box::new(record_level_from(p))));
    reg.register("draw", |_p| Ok(Box::new(Draw)));
}

/// World factory for the legacy API: `level` comes from `[world] level`.
pub fn world_factory<W: LevelWorld>() -> WorldFactory<W> {
    Box::new(|params, _seed| Ok(W::with_level(params.get_f64("level", 0.0))))
}

// ── factory ───────────────────────────────────────────────────────────────────

/// How a [`ScenarioFactory`] chooses the engine seed.
#[derive(Clone, Copy, Debug)]
pub enum EngineSeed {
    /// The trial seed itself.
    Raw,
    /// `derive_seed(seed, &[1])`.
    Derived,
    /// A constant, independent of the trial seed.
    Fixed(u64),
}

type Validator = Box<dyn Fn(&Scenario) -> Result<()> + Send + Sync>;

/// A trial factory that reads the whole scenario: the world's initial `level`
/// is the `init` param of the first `[[mechanism]] name = "record_level"`
/// entry, so it comes from mechanism params rather than `[world]`.
pub struct ScenarioFactory<W> {
    pub engine_seed: EngineSeed,
    pub validator: Option<Validator>,
    pub fail_seeds: Vec<u64>,
    pub builds: Arc<AtomicUsize>,
    pub validations: Arc<AtomicUsize>,
    _world: PhantomData<fn() -> W>,
}

impl<W> ScenarioFactory<W> {
    pub fn new(engine_seed: EngineSeed) -> Self {
        Self {
            engine_seed,
            validator: None,
            fail_seeds: Vec::new(),
            builds: Arc::new(AtomicUsize::new(0)),
            validations: Arc::new(AtomicUsize::new(0)),
            _world: PhantomData,
        }
    }

    pub fn with_validator(
        mut self,
        f: impl Fn(&Scenario) -> Result<()> + Send + Sync + 'static,
    ) -> Self {
        self.validator = Some(Box::new(f));
        self
    }

    pub fn failing_on(mut self, seeds: &[u64]) -> Self {
        self.fail_seeds = seeds.to_vec();
        self
    }

    pub fn builds(&self) -> usize {
        self.builds.load(Ordering::SeqCst)
    }

    pub fn validations(&self) -> usize {
        self.validations.load(Ordering::SeqCst)
    }
}

impl<W: LevelWorld> TrialFactory<W> for ScenarioFactory<W> {
    fn validate(&self, scenario: &Scenario) -> Result<()> {
        self.validations.fetch_add(1, Ordering::SeqCst);
        match &self.validator {
            Some(f) => f(scenario),
            None => Ok(()),
        }
    }

    fn build_trial(&self, scenario: &Scenario, seed: u64) -> Result<Trial<W>> {
        self.builds.fetch_add(1, Ordering::SeqCst);
        if self.fail_seeds.contains(&seed) {
            return Err(SocsimError::Mechanism(format!(
                "injected failure for seed {seed}"
            )));
        }

        let init = scenario
            .mechanisms
            .iter()
            .find(|m| m.name == "record_level")
            .map(|m| m.params.to_params().get_f64("init", 0.0))
            .unwrap_or(0.0);
        let world = W::with_level(init);

        let mut mechanisms: Vec<Box<dyn Mechanism<W>>> = Vec::new();
        for entry in &scenario.mechanisms {
            match entry.name.as_str() {
                "record_level" => {
                    mechanisms.push(Box::new(record_level_from(&entry.params.to_params())))
                }
                "draw" => mechanisms.push(Box::new(Draw)),
                other => return Err(SocsimError::UnknownMechanism(other.to_owned())),
            }
        }

        let engine_seed = match self.engine_seed {
            EngineSeed::Raw => seed,
            EngineSeed::Derived => derive_seed(seed, &[1]),
            EngineSeed::Fixed(s) => s,
        };

        Ok(Trial::new(
            world,
            mechanisms,
            scheduler_from_scenario(scenario)?,
            engine_seed,
        ))
    }
}

// ── comparisons ───────────────────────────────────────────────────────────────

/// Exact comparison of two run results: `seed`, `series`, `final_metrics` and
/// `event_count` with `==`, `events` through `serde_json::to_value`.  No
/// tolerance.
pub fn assert_same_result(a: &RunResult, b: &RunResult) {
    assert_eq!(a.seed, b.seed, "seed");
    assert_eq!(a.series, b.series, "series (seed {})", a.seed);
    assert_eq!(
        a.final_metrics, b.final_metrics,
        "final_metrics (seed {})",
        a.seed
    );
    assert_eq!(
        a.event_count, b.event_count,
        "event_count (seed {})",
        a.seed
    );
    assert_eq!(
        serde_json::to_value(&a.events).unwrap(),
        serde_json::to_value(&b.events).unwrap(),
        "events (seed {})",
        a.seed
    );
}

/// Exact comparison of two lists of run results, in order.
pub fn assert_same_results(a: &[RunResult], b: &[RunResult]) {
    assert_eq!(a.len(), b.len(), "number of results");
    for (x, y) in a.iter().zip(b) {
        assert_same_result(x, y);
    }
}

/// Unwrap a `SocsimError::Config` message, panicking on anything else.
pub fn config_message<T: std::fmt::Debug>(r: Result<T>) -> String {
    match r {
        Err(SocsimError::Config(msg)) => msg,
        other => panic!("expected SocsimError::Config, got {other:?}"),
    }
}
