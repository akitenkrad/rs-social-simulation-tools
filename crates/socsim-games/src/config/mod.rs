//! Reading game simulations from scenario files (feature `config`).
//!
//! A scenario ([`socsim_config::Scenario`]) describes a game simulation in
//! four places: `[world]` holds the population size, the history length, and
//! the topology; `[world.game]` holds the stage game; `[world.population]`
//! holds the rule of every group of agents and their initial actions; and the
//! `[[mechanism]]` entries hold the matching, the rule parameters, the play
//! options, the population dynamic, and the metrics.
//! [`ScenarioConfig::read`] reads all of them for a game type implementing
//! [`ScenarioGame`] ([`FiniteSpec`](crate::FiniteSpec) or
//! [`ContinuousSpec`](crate::ContinuousSpec)) and a [`RuleRegistry`].
//!
//! # Reading rules
//!
//! Every value is read with its type checked; no value of the wrong type falls
//! back to a default. Every key that is not listed below is an error. Errors
//! are [`GameError::Config`] messages that start
//! with the path of the key, such as `world.game.temptation: ...`,
//! `world.population.group[1].share: ...`, or
//! `mechanism[game_decide].logit_lambda: ...`. A built-in game that rejects
//! its parameters reports [`GameError::Catalog`].
//!
//! - A **number** is a TOML integer or a finite TOML float; `temptation = 2`
//!   reads as `2.0`.
//! - A **count** is accepted only when it is a TOML integer or a TOML float
//!   with no fractional part, finite, non-negative, and within the range of
//!   the type it is read into (`u64`, `u32`, or `usize`). So `16` and `16.0`
//!   are accepted, while `-1`, `1.5`, `1e30`, `nan`, `"16"`, and `true` are
//!   rejected. Sweeps that write integers back as floats therefore read back
//!   correctly.
//! - A **label** is an action label made of ASCII letters, digits, `_`, and
//!   `-`. A label refers to an action of role 0.
//!
//! # Keys
//!
//! `[world]`:
//!
//! | Key | Type | Default |
//! |---|---|---|
//! | `n_agents` | count | required (may be left out on a grid) |
//! | `history_len` | count, at least 1 | required |
//! | `topology` | `"none"`, `"grid"`, or `"network"` | `"none"` |
//! | `grid_rows`, `grid_cols` | count | required on a grid; `n_agents` must equal their product |
//! | `boundary` | `"fixed"` or `"toroidal"` | required on a grid |
//! | `neighborhood` | `"moore"` or `"von_neumann"` | required on a grid |
//! | `network_model` | `"erdos_renyi"`, `"watts_strogatz"`, or `"barabasi_albert"` | required on a network |
//! | `network_p` | number in `[0, 1]` | required by `erdos_renyi` |
//! | `network_k`, `network_beta` | even count with `2 <= k <= n_agents - 1`, number in `[0, 1]` | required by `watts_strogatz` |
//! | `network_m` | count with `1 <= m <= n_agents - 1` | required by `barabasi_albert` |
//!
//! A grid needs the `grid` feature and a network the `net` feature.
//!
//! `[world.game]` has a required `kind`:
//!
//! | `kind` | Family | Keys |
//! |---|---|---|
//! | `matrix` | finite | `labels` (`labels[role][action]`), `payoffs` (nested by the action of each role, innermost the payoffs of all roles) |
//! | `prisoners_dilemma` | finite | `temptation`, `reward`, `punishment`, `sucker` (all required) |
//! | `battle_of_sexes`, `matching_pennies`, `stag_hunt` | finite | `a`, `b` (2x2; both, or neither for the default game) |
//! | `rock_paper_scissors` | finite | `win`, `loss` (both, or neither for the default game) |
//! | `hawk_dove` | finite | `v`, `c` |
//! | `el_farol` | finite | `capacity`; the players are `n_agents` |
//! | `minority` | finite | none; the players are `n_agents` |
//! | `public_goods_finite` | finite | `k`, `r`, `cost` |
//! | `cournot` | continuous | `a`, `b`, `c`, `q_max`; the firms are `n_agents` |
//! | `bertrand_differentiated` | continuous | `a`, `b`, `d`, `c`, `p_max`; the firms are `n_agents` |
//! | `public_goods_continuous` | continuous | `k`, `endowment`, `r`, `social_dilemma` (default `false`) |
//!
//! For the games whose players are `n_agents`, writing `n` is an error. A
//! matrix game is written as
//!
//! ```toml
//! [world.game]
//! kind    = "matrix"
//! labels  = [["C", "D"], ["C", "D"]]
//! payoffs = [
//!   [[3.0, 3.0], [0.0, 5.0]],   # payoffs[a0][a1] = [payoff of role 0, payoff of role 1]
//!   [[5.0, 0.0], [1.0, 1.0]],
//! ]
//! ```
//!
//! `[world.population]`:
//!
//! | Key | Type | Default |
//! |---|---|---|
//! | `assign` | `"ordered"` or `"shuffled"` | `"ordered"` |
//! | `[[world.population.group]]` | one or more tables with `rule` and either `count` or `share` | required |
//! | `initial_shares` | table from label to number (finite games) | |
//! | `initial_uniform` | boolean (continuous games) | `false` |
//! | `initial_action` | label (finite games) or array of numbers (continuous games) | action 0 for finite games |
//!
//! A group's `rule` is a name registered in the [`RuleRegistry`], or `held`
//! for agents without a rule; a rule may have one group only. Every group uses
//! `count` (summing to `n_agents`) or every group uses `share` (summing to 1
//! within `1e-9`). Shares become counts by the largest remainder method, ties
//! going to the group listed first. Groups are laid out in the listed order,
//! or shuffled with `assign = "shuffled"`.
//!
//! At most one of the three initial-action keys may be given. In
//! `initial_shares`, at most one action may be left out; it receives the rest
//! (`1 - sum`), and shares above 1 in total are an error. A continuous game
//! needs `initial_uniform = true` or `initial_action`.
//!
//! `[[mechanism]]` entries, identified by `name` (`phase` is ignored):
//!
//! | Name | Keys |
//! |---|---|
//! | `game_match` (required) | `matching`: `"fixed_players"` (with `pairs`, an array of agent-id pairs), `"round_robin"` (with `both_orders`, default `false`), `"random_pairs"`, `"random_groups"` (with `k`), `"whole_population"`, `"grid_neighborhood"` (with `include_self`, default `false`), or `"network_edges"` |
//! | `game_decide` (required) | `decision_scope` (`"per_step"`, `"per_agent_role"`, or `"per_encounter"`; default `"per_step"`), `report_mixed` (default `false`), and the keys of the registered rules |
//! | `game_play` (required) | `payoff_aggregation` (`"sum"` or `"mean"`; default `"sum"`), `aggregate_history_len` (count; records the game's aggregates when given), `materialize` (default `false`), `counting` (finite anonymous games; default `false`) |
//! | `imitate_best` | `update` (`"synchronous"`, required), `tie_break` (`"keep_own"` or `"lowest_action"`; default `"keep_own"`) |
//! | `fermi_update` | `update`, `k` (both required) |
//! | `replicator_update` (finite games) | `update`, `beta` (both required) |
//! | `moran_update` | `update`, `fitness` (`"linear"` or `"exponential"`), `w` (all required), `mutation` (default 0), `gaussian_sd` |
//! | `game_metrics`, finite games | `cooperate_label`, `regret`, `mixed_source` (`"played"`, `"played_cumulative"`, or `"reported"`), `equilibrium` (`"none"` or `"solve"`), `stop_tol` and `stop_window` (together), `population_share`, `action_entropy` (`metrics` feature) |
//! | `game_metrics`, continuous games | `equilibrium` (`"none"` or `"solve"`), `stop_tol` and `stop_window` (together) |
//! | `continuation_probability` | `delta`, a required number in `[0, 1]` |
//!
//! Each name may appear once, at most one population dynamic may be listed,
//! and a population dynamic must be listed after `game_decide`. The order of
//! the entries does not change the simulation: the mechanisms always run as
//! `game_match`, `game_decide`, the population dynamic, `game_play`,
//! `game_metrics`, and `continuation_probability`.
//!
//! # Random streams
//!
//! Everything random in a simulation read from a scenario is derived from the
//! trial seed: `derive_seed(seed, &[0])` shuffles the groups
//! (`assign = "shuffled"`), `derive_seed(seed, &[0, 1])` generates the
//! network, `derive_seed(seed, &[0, 2])` lays out `initial_shares` and draws
//! `initial_uniform` actions, `derive_seed(seed, &[2])` is the decision root,
//! `derive_seed(seed, &[3, agent, generation])` initializes rule states, and
//! `derive_seed(seed, &[1])` seeds the engine. When present,
//! `continuation_probability` derives its private stream from `seed` and the
//! fixed label `0x636f6e74696e7565` (the bytes `continue`).

mod game;
mod mechanisms;
mod reader;
mod rules;
mod world;

pub use game::{game_family, GameFamily, ScenarioGame};
pub use mechanisms::SCENARIO_MECHANISM_NAMES;
pub use rules::{RuleCtor, RuleRegistry};

use std::sync::Arc;

use socsim_config::{Params, Scenario};

use crate::{
    Assignment, DecisionRule, ExperimentState, GameError, GameSimFactory, InitialActions,
    PlayOptions, PlayPath, PopulationDynamics, SchedulerChoice, Topology,
};
use game::{sealed::SpecGame, InitialSpec};
pub(crate) use mechanisms::DYNAMICS;
use mechanisms::{read_mechanisms, AnyDynamics, AnyMatching, AnyPlay};
use reader::ParamReader;
use rules::{SharedRule, DECIDE_PATH};
use world::{read_population, read_scheduler, read_world, TopologySpec};

/// A game simulation read from a scenario, before a seed is chosen.
///
/// [`read`](Self::read) checks every key of the scenario (see the
/// [module documentation](self)) and constructs the game and the rules of the
/// groups. The parts that depend on the seed, a random network and random
/// initial actions, are drawn later from the seed of each trial.
pub struct ScenarioConfig<G: ScenarioGame, X: ExperimentState = ()> {
    game: G,
    n_agents: usize,
    history_len: usize,
    t_max: u64,
    scheduler: SchedulerChoice,
    topology: TopologySpec,
    assignment: Assignment,
    initial: InitialSpec<G>,
    rules: Vec<(String, Arc<dyn DecisionRule<G, X>>)>,
    /// The parameters of `game_decide`, kept to check the rules no agent
    /// follows.
    decide: Params,
    matching: AnyMatching,
    play_options: PlayOptions<G::Action>,
    aggregate_history_len: Option<usize>,
    counting: Option<<G as SpecGame>::Counting>,
    dynamics: AnyDynamics<<G as SpecGame>::Replicator>,
    metrics: Option<<G as SpecGame>::Metrics>,
    continuation_probability: Option<f64>,
}

impl<G: ScenarioGame, X: ExperimentState> ScenarioConfig<G, X> {
    /// Reads `scenario`, constructing the rules of the groups from `registry`.
    ///
    /// Each rule assigned to at least one agent is constructed once from the
    /// parameters of `game_decide`. The simulation's `t_max` and `scheduler`
    /// (`"sequential"` or `"random_activation"`) come from `[simulation]`.
    pub fn read(scenario: &Scenario, registry: &RuleRegistry<G, X>) -> Result<Self, GameError> {
        let mut world = ParamReader::new(&scenario.world.0, "world");
        let world_config = read_world(&mut world)?;
        let n_agents = world_config.n_agents;

        let game_reader = world.subtable("game")?;
        let mut game_reader = world.required("game", game_reader)?;
        let game = G::read_game(&mut game_reader, n_agents)?;
        game_reader.finish()?;

        let mechanisms = read_mechanisms(&scenario.mechanisms, registry, &game)?;

        let population_reader = world.subtable("population")?;
        let mut population_reader = world.required("population", population_reader)?;
        let population = read_population(&mut population_reader, &game, n_agents, registry)?;
        population_reader.finish()?;
        world.finish()?;

        let params = Params::from(mechanisms.decide.clone());
        let mut rules = Vec::with_capacity(population.rules.len());
        for name in population.rules {
            let rule = build_rule(registry, &name, &params, &game)?;
            rules.push((name, rule));
        }

        Ok(Self {
            game,
            n_agents,
            history_len: world_config.history_len,
            t_max: scenario.simulation.t_max,
            scheduler: read_scheduler(&scenario.simulation.scheduler)?,
            topology: world_config.topology,
            assignment: population.assignment,
            initial: population.initial,
            rules,
            decide: params,
            matching: mechanisms.matching,
            play_options: mechanisms.play_options,
            aggregate_history_len: mechanisms.aggregate_history_len,
            counting: mechanisms.counting,
            dynamics: mechanisms.dynamics,
            metrics: mechanisms.metrics,
            continuation_probability: mechanisms.continuation_probability,
        })
    }

    /// The game of `[world.game]`.
    pub fn game(&self) -> &G {
        &self.game
    }

    /// The number of agents.
    pub fn n_agents(&self) -> usize {
        self.n_agents
    }

    /// The number of past steps kept for decisions.
    pub fn history_len(&self) -> usize {
        self.history_len
    }

    /// The assignment of rule kinds to agents.
    pub fn assignment(&self) -> &Assignment {
        &self.assignment
    }

    /// The initial held actions for `seed`.
    ///
    /// Uniform initial actions are drawn from `derive_seed(seed, &[0, 2])`;
    /// shares are returned as [`InitialActions::Shares`] and laid out from the
    /// same stream when the world is built.
    pub fn initial_actions(&self, seed: u64) -> InitialActions<G::Action> {
        self.initial.resolve(&self.game, self.n_agents, seed)
    }

    /// The topology for `seed`, or `None` without a topology. A network is
    /// generated from `derive_seed(seed, &[0, 1])`.
    pub fn topology(&self, seed: u64) -> Option<Topology> {
        self.topology.resolve(self.n_agents, seed)
    }

    /// The probability that a completed step is followed by another step.
    ///
    /// `None` means the scenario has no `continuation_probability` mechanism.
    pub fn continuation_probability(&self) -> Option<f64> {
        self.continuation_probability
    }

    /// A [`GameSimFactory`] configured as the scenario describes, for `seed`
    /// (which fixes the network and the uniform initial actions).
    ///
    /// The play path chosen by the scenario (with or without the game's
    /// aggregates) is passed whole, so the factory builds through exactly the
    /// procedure that the public setting methods configure.
    pub(crate) fn factory(
        &self,
        x: X,
        seed: u64,
    ) -> GameSimFactory<
        G,
        X,
        AnyMatching,
        AnyPlay<<G as SpecGame>::Counting>,
        AnyDynamics<<G as SpecGame>::Replicator>,
    >
    where
        AnyPlay<<G as SpecGame>::Counting>: PlayPath<G, X>,
        AnyDynamics<<G as SpecGame>::Replicator>: PopulationDynamics<G, X>,
    {
        let mut factory = GameSimFactory::with_state(self.game.clone(), x)
            .agents(self.n_agents)
            .matching(self.matching.clone())
            .history_len(self.history_len)
            .t_max(self.t_max)
            .scheduler(self.scheduler)
            .assign(self.assignment.clone())
            .initial_held(self.initial_actions(seed))
            .play_options(self.play_options.clone())
            .play_path(AnyPlay::new(
                self.counting.clone(),
                self.aggregate_history_len,
            ))
            .dynamics(self.dynamics.clone());
        if let Some(topology) = self.topology(seed) {
            factory = factory.topology(topology);
        }
        for (name, rule) in &self.rules {
            factory = factory.rule(name.clone(), SharedRule(Arc::clone(rule)));
        }
        match &self.metrics {
            Some(metrics) => G::attach_metrics(factory, metrics),
            None => factory,
        }
    }

    /// Constructs every registered rule that no agent follows but whose own
    /// parameters appear in `game_decide`, and reports the first error.
    ///
    /// A rule counts as written when `game_decide` contains a key that the
    /// rule declares and no other registered rule declares; a key shared by
    /// several rules (such as `coop_label`) does not make an unused rule
    /// required.
    pub(crate) fn check_unassigned_rules(
        &self,
        registry: &RuleRegistry<G, X>,
    ) -> Result<(), GameError> {
        for name in registry.names() {
            if self.rules.iter().any(|(assigned, _)| assigned == name) {
                continue;
            }
            let written = registry
                .exclusive_keys(name)
                .any(|key| self.decide.get_value(key).is_some());
            if written {
                build_rule(registry, name, &self.decide, &self.game)?;
            }
        }
        Ok(())
    }
}

/// Constructs the rule `name`, locating any error in `game_decide`.
fn build_rule<G: ScenarioGame, X: ExperimentState>(
    registry: &RuleRegistry<G, X>,
    name: &str,
    params: &Params,
    game: &G,
) -> Result<Arc<dyn DecisionRule<G, X>>, GameError> {
    registry
        .build(name, params, game)
        .map_err(|error| match error {
            GameError::Config(message) if message.starts_with(DECIDE_PATH) => {
                GameError::Config(message)
            }
            other => GameError::Config(format!("{DECIDE_PATH}: rule {name:?}: {other}")),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ContinuousSpec, FiniteSpec};

    const IPD: &str = r#"
[simulation]
name = "ipd"
module_pack = "normal-form"
t_max = 20
seed = 42
scheduler = "sequential"

[world]
n_agents = 2
history_len = 20

[world.game]
kind = "matrix"
labels = [["C", "D"], ["C", "D"]]
payoffs = [
  [[3.0, 3.0], [0.0, 5.0]],
  [[5.0, 0.0], [1.0, 1.0]],
]

[world.population]
[[world.population.group]]
rule = "tit_for_tat"
count = 1
[[world.population.group]]
rule = "fictitious_play"
count = 1

[[mechanism]]
name = "game_match"
[mechanism.params]
matching = "fixed_players"
pairs = [[0, 1]]

[[mechanism]]
name = "game_decide"
[mechanism.params]
coop_label = "C"
fp_tie_break = "lowest_index"

[[mechanism]]
name = "game_play"

[[mechanism]]
name = "game_metrics"
[mechanism.params]
cooperate_label = "C"

[output]
log_path = "runs/{name}_{seed}.jsonl"
"#;

    const COURNOT: &str = r#"
[simulation]
name = "cournot"
module_pack = "normal-form"
t_max = 30
seed = 7
scheduler = "random_activation"

[world]
n_agents = 3
history_len = 4

[world.game]
kind = "cournot"
a = 100
b = 1.0
c = 10.0
q_max = 100.0

[world.population]
initial_uniform = true
[[world.population.group]]
rule = "damped_best_response"
count = 3

[[mechanism]]
name = "game_match"
[mechanism.params]
matching = "whole_population"

[[mechanism]]
name = "game_decide"
[mechanism.params]
br_damping = 0.5

[[mechanism]]
name = "game_play"
[mechanism.params]
aggregate_history_len = 4

[[mechanism]]
name = "game_metrics"
[mechanism.params]
equilibrium = "solve"

[output]
log_path = "runs/{name}_{seed}.jsonl"
"#;

    #[test]
    fn a_finite_scenario_builds_and_runs() {
        let scenario = Scenario::parse(IPD).unwrap();
        let config =
            ScenarioConfig::<FiniteSpec>::read(&scenario, &RuleRegistry::finite_builtins())
                .unwrap();
        assert_eq!(config.rules.len(), 2);
        assert_eq!(config.aggregate_history_len, None);
        let mut simulation = config.factory((), 5).build(5).unwrap();
        simulation.run().unwrap();
        assert_eq!(simulation.world().clock.t(), 20);
    }

    #[test]
    fn a_continuous_scenario_builds_and_runs_with_aggregates() {
        let scenario = Scenario::parse(COURNOT).unwrap();
        let config =
            ScenarioConfig::<ContinuousSpec>::read(&scenario, &RuleRegistry::continuous_builtins())
                .unwrap();
        assert_eq!(config.scheduler, SchedulerChoice::RandomActivation);
        assert_eq!(config.aggregate_history_len, Some(4));
        let mut simulation = config.factory((), 9).build(9).unwrap();
        assert!(simulation.world().aggregate.is_some());
        simulation.run().unwrap();
        assert!(simulation.world().clock.t() > 0);
    }
}
