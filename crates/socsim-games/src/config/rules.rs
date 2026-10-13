//! Decision rules chosen by name: [`RuleRegistry`] and [`RuleCtor`].

use std::collections::BTreeMap;
use std::sync::Arc;

use socsim_config::Params;
use socsim_core::SimRng;

use super::reader::{choice_value, config_error, label_value, real_value, reals_value};
use crate::learn::automaton::AutomatonState;
use crate::learn::fictitious::FictitiousBelief;
use crate::learn::logit::LogitState;
use crate::learn::roth_erev::RothErevState;
use crate::{
    Automaton, AutomatonKind, AutomatonPolicyState, BeliefState, ContinuousGame, ContinuousState,
    DampedBestResponse, DecisionContext, DecisionRule, EncounterOutcome, EstimateMode,
    EstimateState, ExperimentState, FictitiousPlay, FiniteGame, GameError, GradientAscent,
    LearnContext, LogitChoice, MyopicBestResponse, PropensityState, RothErev, RuleError,
    RuleInitContext, Slot, StageGame, Stateless, TftState, TitForTat, UniformRandom,
};

/// Builds a decision rule from the parameters of the `game_decide` mechanism
/// and the game.
///
/// The game is passed so that a constructor can turn an action label such as
/// `coop_label = "C"` into an action index. A constructor reports invalid
/// parameters as [`GameError`]; the built-in constructors use
/// [`GameError::Config`] with the path of the key, for example
/// `mechanism[game_decide].logit_lambda: ...`.
pub type RuleCtor<G, X> =
    Box<dyn Fn(&Params, &G) -> Result<Arc<dyn DecisionRule<G, X>>, GameError> + Send + Sync>;

/// The path prefix of the parameters of the `game_decide` mechanism.
pub(crate) const DECIDE_PATH: &str = "mechanism[game_decide]";

/// The rule name that marks a group of agents without a rule.
pub(crate) const HELD: &str = "held";

struct RuleEntry<G: StageGame, X: ExperimentState> {
    keys: Vec<String>,
    ctor: RuleCtor<G, X>,
}

/// A name-to-constructor map for decision rules, used to read the rules of a
/// scenario.
///
/// A scenario names the rule of every group of agents in
/// `[[world.population.group]]` and gives the rules' parameters in the
/// `params` of the `game_decide` mechanism. Each rule is registered with the
/// parameter keys it reads; those keys are accepted in `game_decide`, and any
/// other key there is an unknown-key error. A rule is constructed once per
/// trial and shared, through an `Arc`, by every agent of its kind; rule state
/// lives in the world.
///
/// [`finite_builtins`](Self::finite_builtins) and
/// [`continuous_builtins`](Self::continuous_builtins) register the built-in
/// rules under their scenario names. The name `held` is reserved for groups
/// without a rule.
///
/// # Built-in rules and their keys
///
/// Finite games:
///
/// | Name | Keys |
/// |---|---|
/// | `uniform_random` | none |
/// | `tit_for_tat` | `coop_label` (required) |
/// | `myopic_best_response` | `myopic_tie_break` (`"lowest_index"` or `"random"`, default `"lowest_index"`), `myopic_first_action` (label, default action 0) |
/// | `fictitious_play` | `fp_tie_break` (`"lowest_index"` or `"random"`, default `"lowest_index"`), `fp_initial_counts` (numbers, one per opponent action, default all zero) |
/// | `logit` | `logit_lambda` (required), `logit_estimate` (`"empirical_frequency"` or `"last_opponent"`, default `"empirical_frequency"`) |
/// | `roth_erev` | `roth_erev_phi`, `roth_erev_epsilon`, `roth_erev_initial_propensity` (all required) |
/// | `all_c`, `all_d`, `grim`, `wsls` | `coop_label` (required) |
/// | `gtft` | `coop_label`, `gtft_generosity` (both required) |
///
/// Continuous games:
///
/// | Name | Keys |
/// |---|---|
/// | `damped_best_response` | `br_damping` (required) |
/// | `gradient` | `grad_step` (required) |
///
/// Labels name actions of role 0.
pub struct RuleRegistry<G: StageGame, X: ExperimentState> {
    rules: BTreeMap<String, RuleEntry<G, X>>,
}

impl<G: StageGame, X: ExperimentState> Default for RuleRegistry<G, X> {
    fn default() -> Self {
        Self::new()
    }
}

impl<G: StageGame, X: ExperimentState> RuleRegistry<G, X> {
    /// An empty registry.
    pub fn new() -> Self {
        Self {
            rules: BTreeMap::new(),
        }
    }

    /// Registers the constructor `ctor` under `name`.
    ///
    /// `keys` are the parameter keys of `game_decide` that the rule reads;
    /// they are accepted when a scenario is read, and a key that no
    /// registered rule declares is an unknown-key error. Several rules may
    /// declare the same key.
    ///
    /// Registering a name twice fails with [`RuleError::DuplicateKind`]; the
    /// reserved name `held` fails with [`RuleError::Unsupported`].
    pub fn register<F>(&mut self, name: &str, keys: &[&str], ctor: F) -> Result<(), RuleError>
    where
        F: Fn(&Params, &G) -> Result<Arc<dyn DecisionRule<G, X>>, GameError>
            + Send
            + Sync
            + 'static,
    {
        if name == HELD {
            return Err(RuleError::Unsupported(format!(
                "the rule name {HELD:?} is reserved for groups without a rule"
            )));
        }
        if self.rules.contains_key(name) {
            return Err(RuleError::DuplicateKind(name.to_owned()));
        }
        self.rules.insert(
            name.to_owned(),
            RuleEntry {
                keys: keys.iter().map(|key| (*key).to_owned()).collect(),
                ctor: Box::new(ctor),
            },
        );
        Ok(())
    }

    /// The registered names, in lexicographic order.
    pub fn names(&self) -> Vec<&str> {
        self.rules.keys().map(String::as_str).collect()
    }

    /// Whether `name` is registered.
    pub(crate) fn contains(&self, name: &str) -> bool {
        self.rules.contains_key(name)
    }

    /// Every key declared by any registered rule.
    pub(crate) fn keys(&self) -> impl Iterator<Item = &str> {
        self.rules
            .values()
            .flat_map(|entry| entry.keys.iter().map(String::as_str))
    }

    /// The keys that the rule registered under `name` declares and no other
    /// registered rule declares.
    pub(crate) fn exclusive_keys<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a str> {
        let own = self.rules.get(name).map(|entry| entry.keys.as_slice());
        own.unwrap_or_default()
            .iter()
            .map(String::as_str)
            .filter(move |key| {
                self.rules
                    .iter()
                    .all(|(other, entry)| other == name || !entry.keys.iter().any(|k| k == key))
            })
    }

    /// Constructs the rule registered under `name`.
    pub(crate) fn build(
        &self,
        name: &str,
        params: &Params,
        game: &G,
    ) -> Result<Arc<dyn DecisionRule<G, X>>, GameError> {
        let entry = self
            .rules
            .get(name)
            .ok_or_else(|| RuleError::UnknownKind(name.to_owned()))?;
        (entry.ctor)(params, game)
    }

    /// Registers a built-in rule; built-in names never collide.
    fn builtin<F>(&mut self, name: &str, keys: &[&str], ctor: F)
    where
        F: Fn(&Params, &G) -> Result<Arc<dyn DecisionRule<G, X>>, GameError>
            + Send
            + Sync
            + 'static,
    {
        let registered = self.register(name, keys, ctor);
        debug_assert!(registered.is_ok(), "built-in rule {name} registered twice");
    }
}

impl<G, X> RuleRegistry<G, X>
where
    G: FiniteGame,
    X: ExperimentState,
    X::PolicyState: Slot<Stateless>
        + Slot<TftState>
        + Slot<BeliefState>
        + Slot<EstimateState>
        + Slot<PropensityState>
        + Slot<AutomatonPolicyState>,
{
    /// A registry with the built-in rules for finite games (see the table on
    /// [`RuleRegistry`]).
    pub fn finite_builtins() -> Self {
        let mut registry = Self::new();
        registry.builtin("uniform_random", &[], |_, _| Ok(Arc::new(UniformRandom)));
        registry.builtin("tit_for_tat", &["coop_label"], |params, game| {
            Ok(Arc::new(TitForTat::new(coop_action(params, game)?)))
        });
        registry.builtin(
            "myopic_best_response",
            &["myopic_tie_break", "myopic_first_action"],
            |params, game| {
                let mut rule = MyopicBestResponse::new();
                if tie_break_random(params, "myopic_tie_break")? {
                    rule = rule.tie_break_random();
                }
                if let Some(action) = optional_action(params, game, "myopic_first_action")? {
                    rule = rule.first_action(action);
                }
                Ok(Arc::new(rule))
            },
        );
        registry.builtin(
            "fictitious_play",
            &["fp_tie_break", "fp_initial_counts"],
            |params, game| {
                let mut rule = FictitiousPlay::new();
                if tie_break_random(params, "fp_tie_break")? {
                    rule = rule.tie_break_random();
                }
                if let Some(value) = params.get_value("fp_initial_counts") {
                    let path = decide_path("fp_initial_counts");
                    let counts = reals_value(&path, value)?;
                    // The same checks as when a state is initialized, so that a
                    // rule no agent follows still rejects invalid counts.
                    if game.n_roles() == 2 {
                        for role in 0..2 {
                            FictitiousBelief::new(game, role, counts.clone(), Default::default())
                                .map_err(|error| config_error(&path, error))?;
                        }
                    }
                    rule = rule.initial_counts(counts);
                }
                Ok(Arc::new(rule))
            },
        );
        registry.builtin(
            "logit",
            &["logit_lambda", "logit_estimate"],
            |params, game| {
                let lambda = required_real(params, "logit_lambda")?;
                let estimate = match optional_choice(
                    params,
                    "logit_estimate",
                    &["empirical_frequency", "last_opponent"],
                )? {
                    Some("last_opponent") => EstimateMode::LastOpponent,
                    _ => EstimateMode::EmpiricalFrequency,
                };
                if game.n_roles() == 2 {
                    LogitState::new(game, 0, lambda, estimate)
                        .map_err(|error| config_error(&decide_path("logit_lambda"), error))?;
                }
                Ok(Arc::new(LogitChoice::new(lambda).estimate(estimate)))
            },
        );
        registry.builtin(
            "roth_erev",
            &[
                "roth_erev_phi",
                "roth_erev_epsilon",
                "roth_erev_initial_propensity",
            ],
            |params, game| {
                let phi = required_real(params, "roth_erev_phi")?;
                let epsilon = required_real(params, "roth_erev_epsilon")?;
                let initial = required_real(params, "roth_erev_initial_propensity")?;
                RothErevState::new(game, 0, initial, phi, epsilon)
                    .map_err(|error| config_error(DECIDE_PATH, format!("roth_erev: {error}")))?;
                Ok(Arc::new(RothErev::new(phi, epsilon, initial)))
            },
        );
        for (name, kind) in [
            ("all_c", AutomatonKind::AllC),
            ("all_d", AutomatonKind::AllD),
            ("grim", AutomatonKind::Grim),
            ("wsls", AutomatonKind::Wsls),
        ] {
            registry.builtin(name, &["coop_label"], move |params, game| {
                Ok(Arc::new(Automaton::new(
                    kind.clone(),
                    coop_action(params, game)?,
                )))
            });
        }
        registry.builtin(
            "gtft",
            &["coop_label", "gtft_generosity"],
            |params, game| {
                let coop = coop_action(params, game)?;
                let generosity = required_real(params, "gtft_generosity")?;
                let kind = AutomatonKind::Gtft { generosity };
                // Checks the generosity alone; the cooperation action is checked
                // against the game when a state is initialized.
                AutomatonState::new(kind.clone(), 0)
                    .map_err(|error| config_error(&decide_path("gtft_generosity"), error))?;
                Ok(Arc::new(Automaton::new(kind, coop)))
            },
        );
        registry
    }
}

impl<G, X> RuleRegistry<G, X>
where
    G: ContinuousGame,
    X: ExperimentState,
    X::PolicyState: Slot<ContinuousState>,
{
    /// A registry with the built-in rules for continuous games (see the table
    /// on [`RuleRegistry`]).
    pub fn continuous_builtins() -> Self {
        let mut registry = Self::new();
        registry.builtin("damped_best_response", &["br_damping"], |params, _| {
            let alpha = required_real(params, "br_damping")?;
            let rule = DampedBestResponse::<G>::new(alpha)
                .map_err(|error| config_error(&decide_path("br_damping"), error))?;
            Ok(Arc::new(rule))
        });
        registry.builtin("gradient", &["grad_step"], |params, _| {
            let step = required_real(params, "grad_step")?;
            let rule = GradientAscent::new(step)
                .map_err(|error| config_error(&decide_path("grad_step"), error))?;
            Ok(Arc::new(rule))
        });
        registry
    }
}

fn decide_path(key: &str) -> String {
    format!("{DECIDE_PATH}.{key}")
}

fn required_real(params: &Params, key: &str) -> Result<f64, GameError> {
    let path = decide_path(key);
    let value = params
        .get_value(key)
        .ok_or_else(|| config_error(&path, "this key is required by the rule"))?;
    real_value(&path, value)
}

fn optional_choice<'a>(
    params: &'a Params,
    key: &str,
    choices: &[&str],
) -> Result<Option<&'a str>, GameError> {
    params
        .get_value(key)
        .map(|value| choice_value(&decide_path(key), value, choices))
        .transpose()
}

fn tie_break_random(params: &Params, key: &str) -> Result<bool, GameError> {
    Ok(optional_choice(params, key, &["lowest_index", "random"])? == Some("random"))
}

/// The index of the role-0 action labelled `label`.
pub(crate) fn action_index<G: FiniteGame>(game: &G, label: &str) -> Option<usize> {
    (0..game.n_actions(0)).find(|&action| game.action_label(0, action) == label)
}

/// The role-0 labels of `game`, for error messages.
pub(crate) fn action_labels<G: FiniteGame>(game: &G) -> String {
    (0..game.n_actions(0))
        .map(|action| format!("{:?}", game.action_label(0, action)))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Resolves the label at `path` to an action index of `game`.
pub(crate) fn resolve_label<G: FiniteGame>(
    game: &G,
    path: &str,
    label: &str,
) -> Result<usize, GameError> {
    action_index(game, label).ok_or_else(|| {
        config_error(
            path,
            format!(
                "{label:?} is not an action of the game (actions: {})",
                action_labels(game)
            ),
        )
    })
}

fn optional_action<G: FiniteGame>(
    params: &Params,
    game: &G,
    key: &str,
) -> Result<Option<usize>, GameError> {
    let path = decide_path(key);
    params
        .get_value(key)
        .map(|value| resolve_label(game, &path, label_value(&path, value)?))
        .transpose()
}

fn coop_action<G: FiniteGame>(params: &Params, game: &G) -> Result<usize, GameError> {
    optional_action(params, game, "coop_label")?.ok_or_else(|| {
        config_error(
            &decide_path("coop_label"),
            "this key is required by the rule",
        )
    })
}

/// A rule shared through an `Arc`, registered with a factory that wraps every
/// rule in its own `Arc`.
///
/// Every method forwards to the shared rule, including the ones with a default
/// body, so the wrapper decides, learns, and reports exactly as the rule does.
pub(crate) struct SharedRule<G: StageGame, X: ExperimentState>(
    pub(crate) Arc<dyn DecisionRule<G, X>>,
);

impl<G: StageGame, X: ExperimentState> DecisionRule<G, X> for SharedRule<G, X> {
    fn name(&self) -> &str {
        self.0.name()
    }

    fn init_state(&self, ctx: &RuleInitContext, game: &G) -> Result<X::PolicyState, RuleError> {
        self.0.init_state(ctx, game)
    }

    fn decide(
        &self,
        state: &mut X::PolicyState,
        ctx: &DecisionContext<'_, G, X>,
        rng: &mut SimRng,
    ) -> Result<G::Action, RuleError> {
        self.0.decide(state, ctx, rng)
    }

    fn learn_step(
        &self,
        state: &mut X::PolicyState,
        ctx: &LearnContext<'_, G, X>,
    ) -> Result<(), RuleError> {
        self.0.learn_step(state, ctx)
    }

    fn learn_encounter(
        &self,
        state: &mut X::PolicyState,
        outcome: &EncounterOutcome<'_, G::Action>,
        ctx: &LearnContext<'_, G, X>,
    ) -> Result<(), RuleError> {
        self.0.learn_encounter(state, outcome, ctx)
    }

    fn needs_encounter_learning(&self) -> bool {
        self.0.needs_encounter_learning()
    }

    fn needs_pair_history(&self) -> bool {
        self.0.needs_pair_history()
    }

    fn mixed(&self, state: &X::PolicyState, role: usize, game: &G) -> Option<Vec<f64>> {
        self.0.mixed(state, role, game)
    }
}
