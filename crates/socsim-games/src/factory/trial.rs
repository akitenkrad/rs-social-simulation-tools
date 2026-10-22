//! Trials read from a scenario: [`GameTrialFactory`] (feature `config`).

use std::sync::Arc;

use socsim_config::Scenario;
use socsim_core::SocsimError;
use socsim_mechanisms::ContinuationProbability;
use socsim_runner::{scheduler_from_scenario, Trial, TrialFactory};

use crate::config::{RuleRegistry, ScenarioConfig, ScenarioGame, DYNAMICS};
use crate::{ExperimentState, GameError, GameWorld};

/// Builds the experiment state of a trial from its scenario.
type MakeState<X> = Box<dyn Fn(&Scenario) -> Result<X, GameError> + Send + Sync>;

struct Shared<G: ScenarioGame, X: ExperimentState> {
    registry: RuleRegistry<G, X>,
    make_x: MakeState<X>,
}

/// A [`TrialFactory`] that builds game simulations from whole scenarios.
///
/// `G` is [`FiniteSpec`](crate::FiniteSpec) or [`ContinuousSpec`](crate::ContinuousSpec): the game, the population, the
/// matching, the rules, the play path, the population dynamic, and the
/// metrics and optional continuation probability are all read from the scenario (see the
/// [`config` module](crate::config) for the keys). The factory itself holds
/// only the [`RuleRegistry`] that turns rule names into rules, behind an
/// `Arc`, so cloning it is cheap and one factory serves every worker thread.
///
/// # One procedure for both entry points
///
/// [`build_trial`](TrialFactory::build_trial) reads the scenario into the
/// configuration of a [`GameSimFactory`](crate::GameSimFactory) and calls its
/// [`build_trial`](crate::GameSimFactory::build_trial). A scenario and the
/// same configuration written with the [`GameSimFactory`](crate::GameSimFactory)
/// setting methods therefore build the same game world and game mechanisms
/// through the same procedure. If the scenario lists
/// `continuation_probability`, the factory appends that generic PostStep
/// mechanism after the game metrics.
/// The rules are constructed once per trial and shared by every agent of
/// their kind; their state lives in the world.
///
/// # Seeds
///
/// The world is built from the trial seed passed to `build_trial`, with the
/// streams of the [`config` module](crate::config#random-streams): `[0]` for
/// shuffled groups, `[0, 1]` for a random network, `[0, 2]` for initial
/// action shares and uniform initial actions, `[2]` for decisions, and
/// `[3, agent, generation]` for rule states. The trial's
/// [`engine_seed`](Trial::engine_seed) is `derive_seed(seed, &[1])`, and its
/// scheduler is the one named by `scenario.simulation.scheduler`. An optional
/// `continuation_probability` mechanism derives a separate stream from the
/// raw trial seed and its fixed label; it never consumes the engine stream.
///
/// # Validation
///
/// [`validate`](TrialFactory::validate) reads every key of the scenario and
/// checks the combination of `[[mechanism]]` entries, including that a
/// population dynamic is listed after `game_decide`. It then constructs every
/// registered rule that no agent follows but whose own parameters appear in
/// `game_decide` (a key that several rules share, such as `coop_label`, does
/// not count), and finally builds a trial for `scenario.simulation.seed` and
/// discards it, which runs every check of the assembly. Some of those checks
/// depend on the seed: on a random network, the payoff range that bounds the
/// `beta` of `replicator_update` depends on the degrees of the generated
/// network. A scenario that passes `validate` is therefore always read
/// successfully by `build_trial`, but may still fail to build for another
/// seed; it never runs unchecked.
///
/// Errors are [`SocsimError::Config`] messages that start with the path of the
/// key or the name of the mechanism at fault, for example
/// `world.game.temptation: ...`, `mechanism[game_decide].logit_lambda: ...`,
/// or `mechanism[replicator_update]: ...`.
pub struct GameTrialFactory<G: ScenarioGame, X: ExperimentState = ()> {
    shared: Arc<Shared<G, X>>,
}

impl<G: ScenarioGame, X: ExperimentState> Clone for GameTrialFactory<G, X> {
    fn clone(&self) -> Self {
        Self {
            shared: Arc::clone(&self.shared),
        }
    }
}

impl<G: ScenarioGame> GameTrialFactory<G> {
    /// A factory without experiment state that reads rule names with
    /// `registry`, for example
    /// `GameTrialFactory::<FiniteSpec>::new(RuleRegistry::finite_builtins())`.
    pub fn new(registry: RuleRegistry<G, ()>) -> Self {
        Self::with_state(registry, |_| Ok(()))
    }
}

impl<G: ScenarioGame, X: ExperimentState> GameTrialFactory<G, X> {
    /// A factory whose trials start from the experiment state that `make_x`
    /// builds from the scenario; `make_x` runs once per built trial.
    pub fn with_state(
        registry: RuleRegistry<G, X>,
        make_x: impl Fn(&Scenario) -> Result<X, GameError> + Send + Sync + 'static,
    ) -> Self {
        Self {
            shared: Arc::new(Shared {
                registry,
                make_x: Box::new(make_x),
            }),
        }
    }
}

impl<G: ScenarioGame, X: ExperimentState + Clone> GameTrialFactory<G, X> {
    /// Reads the scenario through the same configuration path as a trial and
    /// returns the metric column patterns described by `game_metrics`.
    pub fn metric_patterns(&self, scenario: &Scenario) -> socsim_core::Result<Vec<String>> {
        let config = ScenarioConfig::read(scenario, &self.shared.registry)
            .map_err(|error| scenario_error(scenario, error))?;
        config
            .metric_patterns()
            .map_err(|error| scenario_error(scenario, error))
    }

    /// Reads `scenario` and builds a trial for `seed` from it.
    fn trial(&self, scenario: &Scenario, seed: u64) -> Result<Trial<GameWorld<G, X>>, GameError> {
        let config = ScenarioConfig::read(scenario, &self.shared.registry)?;
        self.trial_from(&config, scenario, seed)
    }

    /// Builds a trial for `seed` from a configuration read from `scenario`.
    fn trial_from(
        &self,
        config: &ScenarioConfig<G, X>,
        scenario: &Scenario,
        seed: u64,
    ) -> Result<Trial<GameWorld<G, X>>, GameError> {
        let x = (self.shared.make_x)(scenario)?;
        let mut trial = G::build_trial(config, x, seed)?;
        if let Some(delta) = config.continuation_probability() {
            let continuation = ContinuationProbability::new(delta, seed).map_err(|error| {
                GameError::Config(format!(
                    "mechanism[continuation_probability].delta: {error}"
                ))
            })?;
            trial.mechanisms.push(Box::new(continuation));
        }
        Ok(trial)
    }

    /// Reads `scenario`, constructs the unassigned rules whose parameters it
    /// writes, and builds a trial for `scenario.simulation.seed`.
    fn check(&self, scenario: &Scenario) -> Result<(), GameError> {
        let config = ScenarioConfig::read(scenario, &self.shared.registry)?;
        config.check_unassigned_rules(&self.shared.registry)?;
        self.trial_from(&config, scenario, scenario.simulation.seed)?;
        Ok(())
    }
}

impl<G: ScenarioGame, X: ExperimentState + Clone> TrialFactory<GameWorld<G, X>>
    for GameTrialFactory<G, X>
{
    /// Reads and checks the whole scenario, then builds one trial for
    /// `scenario.simulation.seed` and discards it (see [`GameTrialFactory`]).
    fn validate(&self, scenario: &Scenario) -> socsim_core::Result<()> {
        self.check(scenario)
            .map_err(|error| scenario_error(scenario, error))
    }

    /// Builds the trial for `seed`: the world from `seed`, the mechanisms in
    /// the order `game_match`, `game_decide`, the population dynamic,
    /// `game_play`, `game_metrics`, and `continuation_probability`, the
    /// scenario's scheduler, and the engine seed `derive_seed(seed, &[1])`.
    fn build_trial(
        &self,
        scenario: &Scenario,
        seed: u64,
    ) -> socsim_core::Result<Trial<GameWorld<G, X>>> {
        let trial = self
            .trial(scenario, seed)
            .map_err(|error| scenario_error(scenario, error))?;
        Ok(Trial::new(
            trial.world,
            trial.mechanisms,
            scheduler_from_scenario(scenario)?,
            trial.engine_seed,
        ))
    }
}

/// Turns a build error into a configuration error that starts with the path
/// of the key or the name of the mechanism at fault.
fn scenario_error(scenario: &Scenario, error: GameError) -> SocsimError {
    let location = match &error {
        GameError::Config(message) => return SocsimError::Config(message.clone()),
        GameError::Catalog(_) | GameError::Matrix(_) => "world.game".to_owned(),
        GameError::AgentCount { .. } | GameError::InvalidHistoryLength => "world".to_owned(),
        GameError::Topology(_) => "world.topology".to_owned(),
        GameError::Action(_) => "world.population".to_owned(),
        GameError::InvalidKinds { .. } => "world.population.group".to_owned(),
        GameError::InvalidAggregateHistoryLength => {
            "mechanism[game_play].aggregate_history_len".to_owned()
        }
        GameError::UnsupportedRoles { .. } | GameError::InvalidMatching(_) => {
            "mechanism[game_match]".to_owned()
        }
        GameError::Rule(_) | GameError::ManifestMismatch(_) => "mechanism[game_decide]".to_owned(),
        GameError::Metrics(_) => "mechanism[game_metrics]".to_owned(),
        GameError::Population(_) => {
            let name = scenario
                .mechanisms
                .iter()
                .map(|entry| entry.name.as_str())
                .find(|name| DYNAMICS.contains(name))
                .unwrap_or("population dynamic");
            format!("mechanism[{name}]")
        }
    };
    SocsimError::Config(format!("{location}: {error}"))
}
