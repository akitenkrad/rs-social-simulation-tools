//! Reading `[world.game]`, and the game-specific parts of a scenario.

use std::convert::Infallible;
use std::fmt::Debug;

use super::reader::{array_value, config_error, is_label, real_value, ParamReader};
use super::rules::{action_labels, resolve_label};
use super::ScenarioConfig;
use crate::catalog::{
    BattleOfSexes, ElFarol, HawkDove, MatchingPennies, MinorityGame, PrisonersDilemma,
    PublicGoodsFinite, RockPaperScissors, StagHunt,
};
use crate::continuous::{BertrandDifferentiated, Cournot, PublicGoodsContinuous};
use crate::GameWorld;
use crate::{
    uniform_initial_actions, ContinuousEquilibrium, ContinuousMetrics, ContinuousMetricsOptions,
    ContinuousSpec, EquilibriumTarget, ExperimentState, FiniteGame, FiniteSpec, GameError,
    GameMetrics, GameSimFactory, InitialActions, MatrixError, MatrixGame, MetricsOptions,
    MixedSource, ProportionalImitation, StageGame, Update,
};
use socsim_config::Scenario;
use socsim_runner::Trial;

/// Tolerance of the sum of shares around one.
pub(crate) const SHARE_SUM_TOLERANCE: f64 = 1e-9;

const FINITE_KINDS: &[&str] = &[
    "matrix",
    "prisoners_dilemma",
    "battle_of_sexes",
    "matching_pennies",
    "rock_paper_scissors",
    "stag_hunt",
    "hawk_dove",
    "el_farol",
    "minority",
    "public_goods_finite",
];

const CONTINUOUS_KINDS: &[&str] = &[
    "cournot",
    "bertrand_differentiated",
    "public_goods_continuous",
];

/// Whether a scenario describes a finite or continuous game.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum GameFamily {
    /// A game with discrete actions.
    Finite,
    /// A game with vector-valued continuous actions.
    Continuous,
}

/// Returns the family selected by `world.game.kind`.
///
/// Missing keys, wrong value types, and unknown kinds use the same
/// configuration errors as [`ScenarioConfig::read`].
pub fn game_family(scenario: &Scenario) -> Result<GameFamily, GameError> {
    let mut world = ParamReader::new(&scenario.world.0, "world");
    let game = world.subtable("game")?;
    let mut game = world.required("game", game)?;
    let kind = read_kind(&mut game)?;
    if FINITE_KINDS.contains(&kind) {
        Ok(GameFamily::Finite)
    } else if CONTINUOUS_KINDS.contains(&kind) {
        Ok(GameFamily::Continuous)
    } else {
        let mut kinds = Vec::with_capacity(FINITE_KINDS.len() + CONTINUOUS_KINDS.len());
        kinds.extend_from_slice(FINITE_KINDS);
        kinds.extend_from_slice(CONTINUOUS_KINDS);
        Err(game.error(
            "kind",
            format!(
                "unknown game kind {kind:?} (expected one of {})",
                super::reader::quoted(&kinds)
            ),
        ))
    }
}

/// A game type that can be read from a scenario: [`FiniteSpec`] or
/// [`ContinuousSpec`].
///
/// The trait is sealed. It carries what differs between finite and
/// continuous games when a scenario is read: the `kind` values accepted in
/// `[world.game]`, how initial actions are given in `[world.population]`, the
/// population dynamics that apply, and the per-step metrics.
pub trait ScenarioGame: sealed::SpecGame {}

impl ScenarioGame for FiniteSpec {}

impl ScenarioGame for ContinuousSpec {}

/// The initial held actions of a scenario, before a seed is chosen.
pub(crate) enum InitialSpec<G: StageGame> {
    /// Actions given by the scenario.
    Given(InitialActions<G::Action>),
    /// Actions drawn from the random stream of the seed.
    Drawn(fn(&G, usize, u64) -> Vec<G::Action>),
}

impl<G: StageGame> Clone for InitialSpec<G> {
    fn clone(&self) -> Self {
        match self {
            Self::Given(actions) => Self::Given(actions.clone()),
            Self::Drawn(draw) => Self::Drawn(*draw),
        }
    }
}

impl<G: StageGame> InitialSpec<G> {
    /// The initial actions for `seed`.
    pub(crate) fn resolve(
        &self,
        game: &G,
        n_agents: usize,
        seed: u64,
    ) -> InitialActions<G::Action> {
        match self {
            Self::Given(actions) => actions.clone(),
            Self::Drawn(draw) => InitialActions::PerAgent(draw(game, n_agents, seed)),
        }
    }
}

#[allow(private_interfaces)]
pub(crate) mod sealed {
    use super::*;

    /// The game-specific reading of a scenario.
    pub trait SpecGame: StageGame + Clone + Debug {
        /// The count-based play path, or [`Infallible`] when unavailable.
        type Counting: Clone + Debug + Send + Sync + 'static;
        /// The `replicator_update` dynamic, or [`Infallible`] when the game
        /// does not support it.
        type Replicator: Clone + Debug + Send + Sync + 'static;
        /// The options of the `game_metrics` mechanism.
        type Metrics: Clone + Debug + Send + Sync + 'static;

        /// Reads `[world.game]` for a population of `n_agents`.
        fn read_game(reader: &mut ParamReader<'_>, n_agents: usize) -> Result<Self, GameError>;

        /// Reads the initial-action keys of `[world.population]`.
        fn read_initial(
            reader: &mut ParamReader<'_>,
            game: &Self,
        ) -> Result<InitialSpec<Self>, GameError>;

        /// Reads the optional count-based play path of `game_play`.
        fn read_counting(
            reader: &mut ParamReader<'_>,
            game: &Self,
        ) -> Result<Option<Self::Counting>, GameError>;

        /// Reads the `replicator_update` mechanism (`update` is already read).
        fn read_replicator(
            reader: &mut ParamReader<'_>,
            update: Update,
        ) -> Result<Self::Replicator, GameError>;

        /// Reads the `game_metrics` mechanism.
        fn read_metrics(reader: &mut ParamReader<'_>) -> Result<Self::Metrics, GameError>;

        /// Builds the configured metrics mechanism and returns its column
        /// patterns without constructing or running a simulation.
        fn metric_patterns(game: &Self, metrics: &Self::Metrics) -> Result<Vec<String>, GameError>;

        /// Sets the metrics of a factory.
        fn attach_metrics<X: ExperimentState, M, P, D>(
            factory: GameSimFactory<Self, X, M, P, D>,
            metrics: &Self::Metrics,
        ) -> GameSimFactory<Self, X, M, P, D>;

        /// Builds a trial for `seed` from a configuration read from a
        /// scenario, through [`GameSimFactory::build_trial`].
        fn build_trial<X: ExperimentState + Clone>(
            config: &ScenarioConfig<Self, X>,
            x: X,
            seed: u64,
        ) -> Result<Trial<GameWorld<Self, X>>, GameError>
        where
            Self: ScenarioGame;
    }
}

/// The error for a `kind` that is not one of `own`.
fn kind_error(
    reader: &ParamReader<'_>,
    kind: &str,
    own: &[&str],
    own_family: &str,
    other: &[&str],
    other_family: &str,
) -> GameError {
    if other.contains(&kind) {
        reader.error(
            "kind",
            format!("{kind:?} is a {other_family} game, but a {own_family} game is expected"),
        )
    } else {
        reader.error(
            "kind",
            format!(
                "unknown game kind {kind:?} (expected one of {})",
                super::reader::quoted(own)
            ),
        )
    }
}

/// Reads the required `kind`.
fn read_kind<'a>(reader: &mut ParamReader<'a>) -> Result<&'a str, GameError> {
    let kind = reader.text("kind")?;
    reader.required("kind", kind)
}

/// Rejects `n` for the games whose player count is `world.n_agents`.
fn players_from_population(reader: &ParamReader<'_>, kind: &str) -> Result<(), GameError> {
    if reader.has("n") {
        return Err(reader.error(
            "n",
            format!("the number of players of {kind:?} is world.n_agents and cannot be set here"),
        ));
    }
    Ok(())
}

/// Reads a 2x2 payoff matrix.
fn matrix2(reader: &mut ParamReader<'_>, key: &str) -> Result<Option<[[f64; 2]; 2]>, GameError> {
    let path = reader.key_path(key);
    let Some(value) = reader.value(key) else {
        return Ok(None);
    };
    let rows = array_value(&path, value, |row_path, row| {
        array_value(row_path, row, real_value)
    })?;
    let shape_error = || config_error(&path, "expected a 2x2 array of numbers");
    let [first, second]: [Vec<f64>; 2] = rows.try_into().map_err(|_| shape_error())?;
    let first: [f64; 2] = first.try_into().map_err(|_| shape_error())?;
    let second: [f64; 2] = second.try_into().map_err(|_| shape_error())?;
    Ok(Some([first, second]))
}

/// Reads the two 2x2 matrices `a` and `b`: both, or neither for the default.
fn bimatrix2<T: Default>(
    reader: &mut ParamReader<'_>,
    build: impl FnOnce([[f64; 2]; 2], [[f64; 2]; 2]) -> Result<T, crate::catalog::CatalogError>,
) -> Result<T, GameError> {
    match (matrix2(reader, "a")?, matrix2(reader, "b")?) {
        (Some(a), Some(b)) => Ok(build(a, b)?),
        (None, None) => Ok(T::default()),
        (Some(_), None) => Err(reader.error("b", "give both a and b, or neither")),
        (None, Some(_)) => Err(reader.error("a", "give both a and b, or neither")),
    }
}

/// Reads `kind = "matrix"`: `labels[role][action]` and the nested `payoffs`.
fn read_matrix(reader: &mut ParamReader<'_>) -> Result<MatrixGame, GameError> {
    let labels_path = reader.key_path("labels");
    let labels_value = reader.value("labels");
    let labels_value = reader.required("labels", labels_value)?;
    let labels: Vec<Vec<String>> = array_value(&labels_path, labels_value, |role_path, role| {
        let role_labels = array_value(role_path, role, |label_path, label| {
            let label = super::reader::label_value(label_path, label)?;
            Ok(label.to_owned())
        })?;
        for (index, label) in role_labels.iter().enumerate() {
            if role_labels[..index].contains(label) {
                return Err(config_error(
                    &format!("{role_path}[{index}]"),
                    format!("duplicate label {label:?}"),
                ));
            }
        }
        if role_labels.is_empty() {
            return Err(config_error(role_path, "a role needs at least one action"));
        }
        Ok(role_labels)
    })?;
    if labels.is_empty() {
        return Err(config_error(&labels_path, "a game needs at least one role"));
    }
    let n_actions: Vec<usize> = labels.iter().map(Vec::len).collect();
    // `MatrixGame::new` checks the size before the payoffs, so an empty payoff
    // table reports a game that is too large without reading the payoffs.
    if let Err(error @ MatrixError::TooLarge { .. }) =
        MatrixGame::new(n_actions.clone(), Vec::new())
    {
        return Err(error.into());
    }

    let payoffs_path = reader.key_path("payoffs");
    let payoffs_value = reader.value("payoffs");
    let payoffs_value = reader.required("payoffs", payoffs_value)?;
    let mut payoffs = vec![Vec::new(); n_actions.len()];
    read_payoffs(&payoffs_path, payoffs_value, &n_actions, 0, &mut payoffs)?;
    Ok(MatrixGame::new(n_actions, payoffs)?.with_labels(labels)?)
}

/// Reads the payoffs nested `depth` levels deep, appending each profile's
/// payoffs in row-major order.
fn read_payoffs(
    path: &str,
    value: &toml::Value,
    n_actions: &[usize],
    depth: usize,
    payoffs: &mut [Vec<f64>],
) -> Result<(), GameError> {
    let n_roles = n_actions.len();
    let Some(items) = value.as_array() else {
        return Err(config_error(
            path,
            format!("expected an array, found {}", value.type_str()),
        ));
    };
    if depth == n_roles {
        if items.len() != n_roles {
            return Err(config_error(
                path,
                format!(
                    "expected {n_roles} payoffs (one per role), found {}",
                    items.len()
                ),
            ));
        }
        for (role, item) in items.iter().enumerate() {
            payoffs[role].push(real_value(&format!("{path}[{role}]"), item)?);
        }
        return Ok(());
    }
    if items.len() != n_actions[depth] {
        return Err(config_error(
            path,
            format!(
                "expected {} entries (the actions of role {depth}), found {}",
                n_actions[depth],
                items.len()
            ),
        ));
    }
    for (index, item) in items.iter().enumerate() {
        read_payoffs(
            &format!("{path}[{index}]"),
            item,
            n_actions,
            depth + 1,
            payoffs,
        )?;
    }
    Ok(())
}

/// Reads `update`, the schedule shared by every population dynamic.
pub(crate) fn read_update(reader: &mut ParamReader<'_>) -> Result<Update, GameError> {
    reader.require_choice("update", &["synchronous"])?;
    Ok(Update::Synchronous)
}

/// Reads `stop_tol` and `stop_window`, which are given together or not at all.
fn read_stop(reader: &mut ParamReader<'_>) -> Result<Option<(f64, u32)>, GameError> {
    match (reader.real("stop_tol")?, reader.count_u32("stop_window")?) {
        (Some(tol), Some(window)) => Ok(Some((tol, window))),
        (None, None) => Ok(None),
        (Some(_), None) => Err(reader.error(
            "stop_window",
            "give both stop_tol and stop_window, or neither",
        )),
        (None, Some(_)) => {
            Err(reader.error("stop_tol", "give both stop_tol and stop_window, or neither"))
        }
    }
}

/// Counts the initial-action keys present in `[world.population]`.
fn initial_keys(reader: &ParamReader<'_>) -> Result<(), GameError> {
    let present: Vec<&str> = ["initial_shares", "initial_uniform", "initial_action"]
        .into_iter()
        .filter(|key| reader.has(key))
        .collect();
    if present.len() > 1 {
        return Err(reader.table_error(format!(
            "give at most one of initial_shares, initial_uniform, and initial_action (found {})",
            present.join(", ")
        )));
    }
    Ok(())
}

/// Reads `initial_shares`: action label to share, with at most one label left
/// out, which receives the rest.
fn read_initial_shares<G: FiniteGame>(
    reader: &mut ParamReader<'_>,
    game: &G,
) -> Result<Option<Vec<(usize, f64)>>, GameError> {
    let Some(table) = reader.subtable("initial_shares")? else {
        return Ok(None);
    };
    let n_actions = game.n_actions(0);
    let mut shares: Vec<Option<f64>> = vec![None; n_actions];
    for (label, value) in table.table() {
        let path = table.key_path(label);
        if !is_label(label) {
            return Err(config_error(
                &path,
                format!("{label:?} is not an action label (ASCII letters, digits, '_' and '-')"),
            ));
        }
        let action = resolve_label(game, &path, label)?;
        let share = real_value(&path, value)?;
        if share < 0.0 {
            return Err(config_error(
                &path,
                format!("a share must be non-negative, got {share}"),
            ));
        }
        shares[action] = Some(share);
    }
    let missing: Vec<usize> = (0..n_actions).filter(|&a| shares[a].is_none()).collect();
    let listed: f64 = shares.iter().flatten().sum();
    match missing.as_slice() {
        [] => {
            if (listed - 1.0).abs() > SHARE_SUM_TOLERANCE {
                return Err(table.table_error(format!("shares sum to {listed}, expected 1")));
            }
        }
        [rest] => {
            let remainder = 1.0 - listed;
            if remainder < -SHARE_SUM_TOLERANCE {
                return Err(table.table_error(format!("shares sum to {listed}, more than 1")));
            }
            shares[*rest] = Some(remainder.max(0.0));
        }
        _ => {
            return Err(table.table_error(format!(
                "at most one action may be left out, but {} are (actions: {})",
                missing.len(),
                action_labels(game)
            )))
        }
    }
    Ok(Some(
        shares
            .into_iter()
            .enumerate()
            .map(|(action, share)| (action, share.unwrap_or(0.0)))
            .collect(),
    ))
}

#[allow(private_interfaces)]
impl sealed::SpecGame for FiniteSpec {
    type Counting = super::mechanisms::SpecPlay;
    type Replicator = ProportionalImitation;
    type Metrics = MetricsOptions;

    fn read_game(reader: &mut ParamReader<'_>, n_agents: usize) -> Result<Self, GameError> {
        let kind = read_kind(reader)?;
        Ok(match kind {
            "matrix" => Self::Matrix(read_matrix(reader)?),
            "prisoners_dilemma" => {
                let temptation = reader.require_real("temptation")?;
                let reward = reader.require_real("reward")?;
                let punishment = reader.require_real("punishment")?;
                let sucker = reader.require_real("sucker")?;
                Self::PrisonersDilemmaGame(PrisonersDilemma::new(
                    temptation, reward, punishment, sucker,
                )?)
            }
            "battle_of_sexes" => Self::BattleOfSexesGame(bimatrix2(reader, BattleOfSexes::new)?),
            "matching_pennies" => {
                Self::MatchingPenniesGame(bimatrix2(reader, MatchingPennies::new)?)
            }
            "stag_hunt" => Self::StagHuntGame(bimatrix2(reader, StagHunt::new)?),
            "rock_paper_scissors" => {
                let game = match (reader.real("win")?, reader.real("loss")?) {
                    (Some(win), Some(loss)) => RockPaperScissors::new(win, loss)?,
                    (None, None) => RockPaperScissors::default(),
                    (Some(_), None) => {
                        return Err(reader.error("loss", "give both win and loss, or neither"))
                    }
                    (None, Some(_)) => {
                        return Err(reader.error("win", "give both win and loss, or neither"))
                    }
                };
                Self::RockPaperScissorsGame(game)
            }
            "hawk_dove" => {
                let v = reader.require_real("v")?;
                let c = reader.require_real("c")?;
                Self::HawkDoveGame(HawkDove::new(v, c)?)
            }
            "el_farol" => {
                players_from_population(reader, kind)?;
                let capacity = reader.require_count_usize("capacity")?;
                Self::ElFarolGame(ElFarol::new(n_agents, capacity)?)
            }
            "minority" => {
                players_from_population(reader, kind)?;
                Self::Minority(MinorityGame::new(n_agents)?)
            }
            "public_goods_finite" => {
                let k = reader.require_count_usize("k")?;
                let r = reader.require_real("r")?;
                let cost = reader.require_real("cost")?;
                Self::PublicGoodsFiniteGame(PublicGoodsFinite::new(k, r, cost)?)
            }
            other => {
                return Err(kind_error(
                    reader,
                    other,
                    FINITE_KINDS,
                    "finite",
                    CONTINUOUS_KINDS,
                    "continuous",
                ))
            }
        })
    }

    fn read_initial(
        reader: &mut ParamReader<'_>,
        game: &Self,
    ) -> Result<InitialSpec<Self>, GameError> {
        initial_keys(reader)?;
        if reader.has("initial_uniform") {
            reader.value("initial_uniform");
            return Err(reader.error(
                "initial_uniform",
                "uniform initial actions are only for continuous games",
            ));
        }
        if let Some(shares) = read_initial_shares(reader, game)? {
            return Ok(InitialSpec::Given(InitialActions::Shares(shares)));
        }
        if let Some(label) = reader.label("initial_action")? {
            let action = resolve_label(game, &reader.key_path("initial_action"), label)?;
            return Ok(InitialSpec::Given(InitialActions::Same(action)));
        }
        Ok(InitialSpec::Given(InitialActions::Same(0)))
    }

    fn read_counting(
        reader: &mut ParamReader<'_>,
        game: &Self,
    ) -> Result<Option<Self::Counting>, GameError> {
        if reader.flag("counting")?.unwrap_or(false) {
            match game {
                Self::HawkDoveGame(_)
                | Self::ElFarolGame(_)
                | Self::Minority(_)
                | Self::PublicGoodsFiniteGame(_) => Ok(Some(super::mechanisms::SpecPlay)),
                _ => Err(reader.error("counting", "counting play requires an anonymous game")),
            }
        } else {
            Ok(None)
        }
    }

    fn read_replicator(
        reader: &mut ParamReader<'_>,
        update: Update,
    ) -> Result<Self::Replicator, GameError> {
        let beta = reader.require_real("beta")?;
        ProportionalImitation::new(beta, update).map_err(|error| reader.error("beta", error))
    }

    fn read_metrics(reader: &mut ParamReader<'_>) -> Result<Self::Metrics, GameError> {
        let mut options = MetricsOptions::new();
        if let Some(label) = reader.label("cooperate_label")? {
            options = options.cooperate_label(label);
        }
        if let Some(regret) = reader.flag("regret")? {
            options = options.regret(regret);
        }
        if let Some(source) =
            reader.choice("mixed_source", &["played", "played_cumulative", "reported"])?
        {
            options = options.mixed_source(match source {
                "played" => MixedSource::Played,
                "played_cumulative" => MixedSource::PlayedCumulative,
                _ => MixedSource::Reported,
            });
        }
        if let Some(target) = reader.choice("equilibrium", &["none", "solve"])? {
            options = options.equilibrium(match target {
                "solve" => EquilibriumTarget::Solve,
                _ => EquilibriumTarget::None,
            });
        }
        if let Some((tol, window)) = read_stop(reader)? {
            options = options.stop(tol, window);
        }
        if let Some(on) = reader.flag("population_share")? {
            options = options.population_share(on);
        }
        let entropy = reader.flag("action_entropy")?;
        #[cfg(feature = "metrics")]
        if let Some(on) = entropy {
            options = options.action_entropy(on);
        }
        #[cfg(not(feature = "metrics"))]
        if entropy.is_some() {
            return Err(reader.error(
                "action_entropy",
                "the action entropy requires the `metrics` feature",
            ));
        }
        Ok(options)
    }

    fn metric_patterns(game: &Self, metrics: &Self::Metrics) -> Result<Vec<String>, GameError> {
        Ok(GameMetrics::new(game, metrics.clone())?
            .describe()
            .into_iter()
            .map(|(pattern, _)| pattern)
            .collect())
    }

    fn attach_metrics<X: ExperimentState, M, P, D>(
        factory: GameSimFactory<Self, X, M, P, D>,
        metrics: &Self::Metrics,
    ) -> GameSimFactory<Self, X, M, P, D> {
        factory.metrics(metrics.clone())
    }

    fn build_trial<X: ExperimentState + Clone>(
        config: &ScenarioConfig<Self, X>,
        x: X,
        seed: u64,
    ) -> Result<Trial<GameWorld<Self, X>>, GameError> {
        config.factory(x, seed).build_trial(seed)
    }
}

#[allow(private_interfaces)]
impl sealed::SpecGame for ContinuousSpec {
    type Counting = Infallible;
    type Replicator = Infallible;
    type Metrics = ContinuousMetricsOptions;

    fn read_game(reader: &mut ParamReader<'_>, n_agents: usize) -> Result<Self, GameError> {
        let kind = read_kind(reader)?;
        Ok(match kind {
            "cournot" => {
                players_from_population(reader, kind)?;
                let a = reader.require_real("a")?;
                let b = reader.require_real("b")?;
                let c = reader.require_real("c")?;
                let q_max = reader.require_real("q_max")?;
                Self::CournotGame(Cournot::new(n_agents, a, b, c, q_max)?)
            }
            "bertrand_differentiated" => {
                players_from_population(reader, kind)?;
                let a = reader.require_real("a")?;
                let b = reader.require_real("b")?;
                let d = reader.require_real("d")?;
                let c = reader.require_real("c")?;
                let p_max = reader.require_real("p_max")?;
                Self::BertrandDifferentiatedGame(BertrandDifferentiated::new(
                    n_agents, a, b, d, c, p_max,
                )?)
            }
            "public_goods_continuous" => {
                let k = reader.require_count_usize("k")?;
                let endowment = reader.require_real("endowment")?;
                let r = reader.require_real("r")?;
                let game = if reader.flag("social_dilemma")?.unwrap_or(false) {
                    PublicGoodsContinuous::social_dilemma(k, endowment, r)?
                } else {
                    PublicGoodsContinuous::new(k, endowment, r)?
                };
                Self::PublicGoodsContinuousGame(game)
            }
            other => {
                return Err(kind_error(
                    reader,
                    other,
                    CONTINUOUS_KINDS,
                    "continuous",
                    FINITE_KINDS,
                    "finite",
                ))
            }
        })
    }

    fn read_initial(
        reader: &mut ParamReader<'_>,
        game: &Self,
    ) -> Result<InitialSpec<Self>, GameError> {
        initial_keys(reader)?;
        if reader.has("initial_shares") {
            reader.value("initial_shares");
            return Err(reader.error(
                "initial_shares",
                "initial action shares are only for finite games",
            ));
        }
        if let Some(action) = reader.reals("initial_action")? {
            let path = reader.key_path("initial_action");
            for role in 0..game.n_roles() {
                game.check_action(role, &action)
                    .map_err(|error| config_error(&path, error))?;
            }
            return Ok(InitialSpec::Given(InitialActions::Same(action)));
        }
        if reader.flag("initial_uniform")? == Some(true) {
            return Ok(InitialSpec::Drawn(uniform_initial_actions::<Self>));
        }
        Err(reader.table_error(
            "a continuous game needs initial actions: set initial_uniform = true or initial_action",
        ))
    }

    fn read_counting(
        reader: &mut ParamReader<'_>,
        _game: &Self,
    ) -> Result<Option<Self::Counting>, GameError> {
        if reader.has("counting") {
            reader.value("counting");
            Err(reader.error("counting", "counting play requires a finite game"))
        } else {
            Ok(None)
        }
    }

    fn read_replicator(
        reader: &mut ParamReader<'_>,
        _update: Update,
    ) -> Result<Self::Replicator, GameError> {
        Err(reader.table_error("replicator_update requires a finite game"))
    }

    fn read_metrics(reader: &mut ParamReader<'_>) -> Result<Self::Metrics, GameError> {
        let mut options = ContinuousMetricsOptions::new();
        if let Some(target) = reader.choice("equilibrium", &["none", "solve"])? {
            options = options.equilibrium(match target {
                "solve" => ContinuousEquilibrium::Solve,
                _ => ContinuousEquilibrium::None,
            });
        }
        if let Some((tol, window)) = read_stop(reader)? {
            options = options.stop(tol, window);
        }
        Ok(options)
    }

    fn metric_patterns(game: &Self, metrics: &Self::Metrics) -> Result<Vec<String>, GameError> {
        Ok(ContinuousMetrics::new(game, metrics.clone())?
            .describe()
            .into_iter()
            .map(|(pattern, _)| pattern)
            .collect())
    }

    fn attach_metrics<X: ExperimentState, M, P, D>(
        factory: GameSimFactory<Self, X, M, P, D>,
        metrics: &Self::Metrics,
    ) -> GameSimFactory<Self, X, M, P, D> {
        factory.continuous_metrics(metrics.clone())
    }

    fn build_trial<X: ExperimentState + Clone>(
        config: &ScenarioConfig<Self, X>,
        x: X,
        seed: u64,
    ) -> Result<Trial<GameWorld<Self, X>>, GameError> {
        config.factory(x, seed).build_trial(seed)
    }
}
