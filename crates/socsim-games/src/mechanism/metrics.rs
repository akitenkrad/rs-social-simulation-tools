use std::borrow::Cow;
use std::marker::PhantomData;

use socsim_core::{Mechanism, Phase, SocsimError, StepContext};

use crate::world::InternalMetricsState;
use crate::{
    metric_action_ids, mixed, solve::support_enumeration, ExperimentState, FiniteGame, GameError,
    GameWorld, MetricScheme, MixedStrategy, PolicyTable,
};

/// Selects whether metric descriptions refer to actions before or after a
/// population update.
///
/// This option changes only the text returned by `describe()`; it does not
/// change any recorded value. Use [`PayoffTiming::AfterUpdate`] for an
/// asynchronous population step, whose final record is recomputed from the
/// actions held after every update of the step.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum PayoffTiming {
    /// Describe the actions and payoffs played before population dynamics.
    #[default]
    BeforeUpdate,
    /// Describe play recomputed from the final actions after in-place updates.
    AfterUpdate,
}

/// Selects the source used to construct per-role mixed strategies.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum MixedSource {
    /// Use the current step's empirical action distribution for each role.
    #[default]
    Played,
    /// Use the empirical action distribution accumulated from the first step.
    PlayedCumulative,
    /// Average post-learning policy reports over the current step's seats.
    Reported,
}

/// Selects how an equilibrium comparison target is obtained.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum EquilibriumTarget {
    /// Do not compute equilibrium distance.
    #[default]
    None,
    /// Solve the complete two-player equilibrium set during construction.
    Solve,
}

/// Options for per-step game metrics.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct MetricsOptions {
    cooperate_label: Option<String>,
    regret: Option<bool>,
    mixed_source: MixedSource,
    equilibrium: EquilibriumTarget,
    stop_tol: Option<f64>,
    stop_window: u32,
    population_share: bool,
    payoff_timing: PayoffTiming,
    #[cfg(feature = "metrics")]
    action_entropy: bool,
}

impl MetricsOptions {
    /// Constructs the default metric options.
    pub fn new() -> Self {
        Self {
            cooperate_label: None,
            regret: None,
            mixed_source: MixedSource::Played,
            equilibrium: EquilibriumTarget::None,
            stop_tol: None,
            stop_window: 0,
            population_share: false,
            payoff_timing: PayoffTiming::BeforeUpdate,
            #[cfg(feature = "metrics")]
            action_entropy: true,
        }
    }

    /// Enables `game.coop_rate` using the uniquely matching action in each role.
    pub fn cooperate_label(mut self, label: impl Into<String>) -> Self {
        self.cooperate_label = Some(label.into());
        self
    }

    /// Enables or disables `game.regret` explicitly.
    ///
    /// When not specified, regret is enabled for two-player games and disabled
    /// for games with any other role count.
    pub fn regret(mut self, on: bool) -> Self {
        self.regret = Some(on);
        self
    }

    /// Selects the mixed-strategy source used by strategy metrics.
    pub fn mixed_source(mut self, source: MixedSource) -> Self {
        self.mixed_source = source;
        self
    }

    /// Selects the equilibrium target used by `game.eq_distance`.
    pub fn equilibrium(mut self, target: EquilibriumTarget) -> Self {
        self.equilibrium = target;
        self
    }

    /// Requests early stopping after `window` consecutive sufficiently small
    /// changes in the played action distribution.
    ///
    /// `tol` must be finite and greater than zero, and `window` must be at
    /// least one. The values are validated by [`GameMetrics::new`].
    pub fn stop(mut self, tol: f64, window: u32) -> Self {
        self.stop_tol = Some(tol);
        self.stop_window = window;
        self
    }

    /// Enables or disables `game.population_share.a<k>`, the share of all
    /// agents holding action `k` at `PostStep`.
    ///
    /// The default is `false`. When population dynamics are assembled, the
    /// held actions at `PostStep` are the updated actions used in the next
    /// step, while `played_share` describes the actions played before the
    /// update. Every role must have the same number of actions.
    pub fn population_share(mut self, on: bool) -> Self {
        self.population_share = on;
        self
    }

    /// Selects the timing described for action and payoff columns.
    ///
    /// This changes only [`GameMetrics::describe`], not recorded values. The
    /// default is [`PayoffTiming::BeforeUpdate`].
    pub fn payoff_timing(mut self, timing: PayoffTiming) -> Self {
        self.payoff_timing = timing;
        self
    }

    /// Enables or disables Shannon entropy of the pooled played-action shares.
    #[cfg(feature = "metrics")]
    pub fn action_entropy(mut self, on: bool) -> Self {
        self.action_entropy = on;
        self
    }
}

impl Default for MetricsOptions {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug)]
struct ActionColumn {
    name: String,
    role: Option<usize>,
}

/// Records per-step action, payoff, cooperation, and public aggregate metrics.
///
/// Add this mechanism to an assembled simulation with
/// `SimulationBuilder::add_mechanism`. It observes the world in `PostStep` and
/// does not modify model state.
#[derive(Clone, Debug)]
pub struct GameMetrics<G: FiniteGame> {
    expected_actions: Vec<usize>,
    action_columns: Vec<ActionColumn>,
    scheme: MetricScheme,
    role_offsets: Vec<usize>,
    cooperate_actions: Option<Vec<usize>>,
    regret: bool,
    mixed_source: MixedSource,
    equilibria: Option<Vec<Vec<Vec<f64>>>>,
    stop_tol: Option<f64>,
    stop_window: u32,
    population_columns: Option<Vec<String>>,
    payoff_timing: PayoffTiming,
    #[cfg(feature = "metrics")]
    action_entropy: bool,
    marker: PhantomData<fn() -> G>,
}

impl<G: FiniteGame> GameMetrics<G> {
    /// Constructs metrics for the role and action shape of `game`.
    ///
    /// A configured cooperation label must match exactly one action in every
    /// role.
    pub fn new(game: &G, options: MetricsOptions) -> Result<Self, GameError> {
        if let Some(tol) = options.stop_tol {
            if !tol.is_finite() || tol <= 0.0 {
                return Err(GameError::Metrics(
                    "stop_tol must be finite and greater than zero".to_owned(),
                ));
            }
            if options.stop_window == 0 {
                return Err(GameError::Metrics(
                    "stop_window must be greater than zero".to_owned(),
                ));
            }
        }
        let expected_actions: Vec<_> = (0..game.n_roles())
            .map(|role| game.n_actions(role))
            .collect();
        let metric_ids = metric_action_ids(game);
        let scheme = metric_ids.scheme();
        let action_columns = metric_ids
            .actions()
            .iter()
            .map(|entry| ActionColumn {
                name: format!("played_share.{}", entry.id),
                role: entry.role,
            })
            .collect();
        let mut offset = 0;
        let role_offsets = expected_actions
            .iter()
            .map(|&count| {
                let current = offset;
                offset += count;
                current
            })
            .collect();
        let cooperate_actions = options
            .cooperate_label
            .as_deref()
            .map(|label| Self::resolve_cooperation_actions(game, label))
            .transpose()?;
        let regret = options.regret.unwrap_or(game.n_roles() == 2);
        let equilibria = match options.equilibrium {
            EquilibriumTarget::None => None,
            EquilibriumTarget::Solve => {
                if game.n_roles() != 2 {
                    return Err(GameError::Metrics(format!(
                        "equilibrium solving requires exactly two roles, got {}",
                        game.n_roles()
                    )));
                }
                let solved = support_enumeration(game).map_err(|error| {
                    GameError::Metrics(format!("failed to solve equilibria: {error}"))
                })?;
                if !solved.is_complete() {
                    return Err(GameError::Metrics(
                        "equilibrium solving produced incomplete findings".to_owned(),
                    ));
                }
                if solved.equilibria.is_empty() {
                    return Err(GameError::Metrics(
                        "equilibrium solving found no equilibria".to_owned(),
                    ));
                }
                Some(
                    solved
                        .equilibria
                        .into_iter()
                        .map(|equilibrium| {
                            equilibrium
                                .strategies
                                .into_iter()
                                .map(|strategy| strategy.probs().to_vec())
                                .collect()
                        })
                        .collect(),
                )
            }
        };

        let population_columns = if options.population_share {
            crate::game::check_common_action_space(game).map_err(|_| {
                GameError::Metrics(
                    "population_share requires every role to have the same number of actions"
                        .to_owned(),
                )
            })?;
            Some(
                (0..game.n_actions(0))
                    .map(|action| format!("population_share.a{action}"))
                    .collect(),
            )
        } else {
            None
        };

        Ok(Self {
            expected_actions,
            action_columns,
            scheme,
            role_offsets,
            cooperate_actions,
            regret,
            mixed_source: options.mixed_source,
            equilibria,
            stop_tol: options.stop_tol,
            stop_window: options.stop_window,
            population_columns,
            payoff_timing: options.payoff_timing,
            #[cfg(feature = "metrics")]
            action_entropy: options.action_entropy,
            marker: PhantomData,
        })
    }

    fn resolve_cooperation_actions(game: &G, label: &str) -> Result<Vec<usize>, GameError> {
        if game.n_roles() == 0 {
            return Err(GameError::Metrics(
                "a cooperation label requires at least one game role".to_owned(),
            ));
        }
        (0..game.n_roles())
            .map(|role| {
                let matches: Vec<_> = (0..game.n_actions(role))
                    .filter(|&action| game.action_label(role, action) == label)
                    .collect();
                if matches.len() != 1 {
                    return Err(GameError::Metrics(format!(
                        "cooperation label {label:?} must match exactly one action for role {role}, found {}",
                        matches.len()
                    )));
                }
                Ok(matches[0])
            })
            .collect()
    }

    /// Describes the metric column patterns emitted by this mechanism.
    ///
    /// Each pattern is a dotted template in which a segment written wholly as
    /// `{name}` stands for exactly one segment of the column name, and every
    /// other segment is literal. Braces never share a segment with literal
    /// text, so the patterns can be copied unchanged into the
    /// `[[metric_patterns]]` of a runvault `runvault.toml`.
    pub fn describe(&self) -> Vec<(String, String)> {
        let played_share = match (self.scheme, self.payoff_timing) {
            (MetricScheme::Pooled, PayoffTiming::BeforeUpdate) => {
                "Share of all played seats choosing `{action}`. `{action}` is a metric_id; display labels come from the game's metric actions. Actions are pooled across roles."
            }
            (MetricScheme::PerRole, PayoffTiming::BeforeUpdate) => {
                "Share of played seats in the indicated role choosing `{action}`. `{action}` is a metric_id; display labels come from the game's metric actions. Actions are reported per role."
            }
            (MetricScheme::Pooled, PayoffTiming::AfterUpdate) => {
                "Share of all played seats choosing `{action}` from the actions held after every update of the step (asynchronous update). `{action}` is a metric_id; display labels come from the game's metric actions. Actions are pooled across roles."
            }
            (MetricScheme::PerRole, PayoffTiming::AfterUpdate) => {
                "Share of played seats in the indicated role choosing `{action}` from the actions held after every update of the step (asynchronous update). `{action}` is a metric_id; display labels come from the game's metric actions. Actions are reported per role."
            }
        };
        let (mean_payoff, payoff_sd) = match self.payoff_timing {
            PayoffTiming::BeforeUpdate => (
                "Mean aggregated payoff among agents who played at least once in the step.",
                "Population standard deviation of aggregated payoff among agents who played at least once in the step.",
            ),
            PayoffTiming::AfterUpdate => (
                "Mean aggregated payoff from play using the actions held after every update of the step (asynchronous update), among agents who played at least once.",
                "Population standard deviation of aggregated payoff from play using the actions held after every update of the step (asynchronous update), among agents who played at least once.",
            ),
        };
        let mut descriptions = vec![
            (
                "game.played_share.{action}".to_owned(),
                played_share.to_owned(),
            ),
            ("game.mean_payoff".to_owned(), mean_payoff.to_owned()),
            ("game.payoff_sd".to_owned(), payoff_sd.to_owned()),
        ];
        if self.cooperate_actions.is_some() {
            let cooperation = match self.payoff_timing {
                PayoffTiming::BeforeUpdate => {
                    "Share of all played seats choosing the configured cooperation label for that seat's role."
                }
                PayoffTiming::AfterUpdate => {
                    "Share of all played seats choosing the configured cooperation label from the actions held after every update of the step (asynchronous update), for that seat's role."
                }
            };
            descriptions.push(("game.coop_rate".to_owned(), cooperation.to_owned()));
        }
        if self.regret {
            descriptions.push((
                "game.regret".to_owned(),
                "Maximum unilateral-deviation regret of the configured mixed-strategy profile."
                    .to_owned(),
            ));
        }
        if self.equilibria.is_some() {
            descriptions.push((
                "game.eq_distance".to_owned(),
                "Minimum summed L1 distance from the configured mixed-strategy profile to the solved equilibrium set."
                    .to_owned(),
            ));
        }
        #[cfg(feature = "metrics")]
        if self.action_entropy && self.scheme == MetricScheme::Pooled {
            descriptions.push((
                "game.action_entropy".to_owned(),
                "Shannon entropy in nats of the pooled played-action shares.".to_owned(),
            ));
        }
        if self.population_columns.is_some() {
            descriptions.push((
                "game.population_share.{action}".to_owned(),
                "Share of all agents holding the indicated action at the end of the step, after any population dynamics; the denominator includes every agent. `{action}` is `a<k>` for action index k, the same in every role."
                    .to_owned(),
            ));
        }
        descriptions.push((
            "game.aggregate.{name}".to_owned(),
            "Public game aggregate for the completed step; present only when the simulation was assembled with aggregates."
                .to_owned(),
        ));
        descriptions
    }

    fn check_shape<X: ExperimentState>(&self, world: &GameWorld<G, X>) -> socsim_core::Result<()> {
        if world.game.n_roles() != self.expected_actions.len() {
            return Err(SocsimError::Mechanism(format!(
                "game_metrics expected {} roles, got {}",
                self.expected_actions.len(),
                world.game.n_roles()
            )));
        }
        for (role, &expected) in self.expected_actions.iter().enumerate() {
            let actual = world.game.n_actions(role);
            if actual != expected {
                return Err(SocsimError::Mechanism(format!(
                    "game_metrics expected {expected} actions for role {role}, got {actual}"
                )));
            }
        }
        if world.plays.n_roles != self.expected_actions.len() {
            return Err(SocsimError::Mechanism(format!(
                "game_metrics expected an arena with {} roles, got {}",
                self.expected_actions.len(),
                world.plays.n_roles
            )));
        }
        Ok(())
    }

    /// Returns the per-role played-action counts recorded by the implicit
    /// play path, when the step was evaluated without a play arena.
    fn implicit_counts<'w, X: ExperimentState>(
        &self,
        world: &'w GameWorld<G, X>,
    ) -> socsim_core::Result<Option<&'w [Vec<usize>]>> {
        if !world.plays.actions.is_empty() {
            return Ok(None);
        }
        let Some(played) = world.played_counts.as_ref() else {
            return Ok(None);
        };
        if played.by_role().len() != self.expected_actions.len()
            || played
                .by_role()
                .iter()
                .zip(&self.expected_actions)
                .any(|(counts, &expected)| counts.len() > expected)
        {
            return Err(SocsimError::Mechanism(
                "game_metrics found inconsistent implicit played-action counts".to_owned(),
            ));
        }
        Ok(Some(played.by_role()))
    }

    fn push_action_metrics<'a, X: ExperimentState>(
        &'a self,
        world: &GameWorld<G, X>,
        row: &mut Vec<(Cow<'a, str>, f64)>,
    ) -> socsim_core::Result<Option<Vec<f64>>> {
        let seat_count = world.plays.actions.len();
        if seat_count == 0 {
            let Some(played) = world.played_counts.as_ref() else {
                return Ok(None);
            };
            if played.by_role().len() != self.expected_actions.len()
                || played
                    .by_role()
                    .iter()
                    .zip(&self.expected_actions)
                    .any(|(counts, &expected)| counts.len() > expected)
            {
                return Err(SocsimError::Mechanism(
                    "game_metrics found inconsistent implicit played-action counts".to_owned(),
                ));
            }
            if played.total() == 0 {
                return Ok(None);
            }
            let role_seats: Vec<_> = played
                .by_role()
                .iter()
                .map(|counts| counts.iter().sum::<usize>())
                .collect();
            let mut counts = vec![0_usize; self.action_columns.len()];
            for (role, role_counts) in played.by_role().iter().enumerate() {
                for (action, &count) in role_counts.iter().enumerate() {
                    let index = match self.scheme {
                        MetricScheme::Pooled => action,
                        MetricScheme::PerRole => self.role_offsets[role] + action,
                    };
                    counts[index] += count;
                }
            }
            let mut shares = Vec::with_capacity(self.action_columns.len());
            for (column, &count) in self.action_columns.iter().zip(&counts) {
                let denominator = match column.role {
                    Some(role) => role_seats[role],
                    None => played.total(),
                };
                if denominator != 0 {
                    let share = count as f64 / denominator as f64;
                    row.push((Cow::Borrowed(column.name.as_str()), share));
                    shares.push(share);
                }
            }
            if let Some(cooperate_actions) = &self.cooperate_actions {
                let cooperation_count: usize = played
                    .by_role()
                    .iter()
                    .zip(cooperate_actions)
                    .map(|(counts, &action)| counts.get(action).copied().unwrap_or(0))
                    .sum();
                row.push((
                    Cow::Borrowed("coop_rate"),
                    cooperation_count as f64 / played.total() as f64,
                ));
            }
            return Ok(Some(shares));
        }
        if world.plays.seats.len() != seat_count
            || self.expected_actions.is_empty()
            || !seat_count.is_multiple_of(self.expected_actions.len())
        {
            return Err(SocsimError::Mechanism(
                "game_metrics found an inconsistent play arena".to_owned(),
            ));
        }

        let mut counts = vec![0_usize; self.action_columns.len()];
        let mut role_seats = vec![0_usize; self.expected_actions.len()];
        let mut cooperation_count = 0_usize;
        for (seat, action) in world.plays.actions.iter().enumerate() {
            let role = seat % self.expected_actions.len();
            let action = action.as_ref().ok_or_else(|| {
                SocsimError::Mechanism(format!(
                    "game_metrics found an unfilled action at seat {seat}"
                ))
            })?;
            if *action >= self.expected_actions[role] {
                return Err(SocsimError::Mechanism(format!(
                    "game_metrics found action {action} outside role {role}"
                )));
            }
            role_seats[role] += 1;
            let index = match self.scheme {
                MetricScheme::Pooled => *action,
                MetricScheme::PerRole => self.role_offsets[role] + *action,
            };
            counts[index] += 1;
            if self
                .cooperate_actions
                .as_ref()
                .is_some_and(|actions| actions[role] == *action)
            {
                cooperation_count += 1;
            }
        }

        let mut shares = Vec::with_capacity(self.action_columns.len());
        for (column, &count) in self.action_columns.iter().zip(&counts) {
            let denominator = match column.role {
                Some(role) => role_seats[role],
                None => seat_count,
            };
            if denominator != 0 {
                let share = count as f64 / denominator as f64;
                row.push((Cow::Borrowed(column.name.as_str()), share));
                shares.push(share);
            }
        }
        if self.cooperate_actions.is_some() {
            row.push((
                Cow::Borrowed("coop_rate"),
                cooperation_count as f64 / seat_count as f64,
            ));
        }
        Ok(Some(shares))
    }

    fn push_payoff_metrics<'a, X: ExperimentState>(
        &self,
        world: &GameWorld<G, X>,
        row: &mut Vec<(Cow<'a, str>, f64)>,
    ) -> socsim_core::Result<()> {
        if world.payoff.len() != world.n_played.len() {
            return Err(SocsimError::Mechanism(
                "game_metrics found inconsistent payoff state".to_owned(),
            ));
        }
        let payoffs: Vec<_> = world
            .payoff
            .iter()
            .zip(&world.n_played)
            .filter_map(|(&payoff, &n_played)| (n_played > 0).then_some(payoff))
            .collect();
        if payoffs.is_empty() {
            return Ok(());
        }
        if payoffs.iter().any(|value| !value.is_finite()) {
            return Err(SocsimError::Mechanism(
                "game_metrics found a non-finite played payoff".to_owned(),
            ));
        }
        let sum = payoffs.iter().try_fold(0.0, |sum, value| {
            let next = sum + value;
            next.is_finite().then_some(next)
        });
        let Some(sum) = sum else {
            return Err(SocsimError::Mechanism(
                "game_metrics payoff sum is non-finite".to_owned(),
            ));
        };
        let mean = sum / payoffs.len() as f64;
        let squared_deviations = payoffs.iter().try_fold(0.0, |sum, value| {
            let deviation = value - mean;
            let next = sum + deviation * deviation;
            next.is_finite().then_some(next)
        });
        let Some(squared_deviations) = squared_deviations else {
            return Err(SocsimError::Mechanism(
                "game_metrics payoff variance is non-finite".to_owned(),
            ));
        };
        let payoff_sd = (squared_deviations / payoffs.len() as f64).sqrt();
        row.push((Cow::Borrowed("mean_payoff"), mean));
        row.push((Cow::Borrowed("payoff_sd"), payoff_sd));
        Ok(())
    }

    fn push_population_metrics<'a, X: ExperimentState>(
        &'a self,
        world: &GameWorld<G, X>,
        row: &mut Vec<(Cow<'a, str>, f64)>,
    ) -> socsim_core::Result<()> {
        let Some(columns) = &self.population_columns else {
            return Ok(());
        };
        if world.held.is_empty() {
            return Ok(());
        }
        let mut counts = vec![0_usize; columns.len()];
        for (index, &action) in world.held.iter().enumerate() {
            let count = counts.get_mut(action).ok_or_else(|| {
                SocsimError::Mechanism(format!(
                    "game_metrics found held action {action} out of range for agent {index}"
                ))
            })?;
            *count += 1;
        }
        let total = world.held.len() as f64;
        for (column, count) in columns.iter().zip(counts) {
            row.push((Cow::Borrowed(column.as_str()), count as f64 / total));
        }
        Ok(())
    }

    fn push_aggregate_metrics<X: ExperimentState>(
        &self,
        world: &GameWorld<G, X>,
        t: u64,
        row: &mut Vec<(Cow<'_, str>, f64)>,
    ) -> socsim_core::Result<()> {
        let Some(table) = world.aggregate.as_ref() else {
            return Ok(());
        };
        let Some((aggregate_t, values)) = table.last() else {
            return Ok(());
        };
        if aggregate_t != t {
            return Ok(());
        }
        if table.names().len() != values.len() {
            return Err(SocsimError::Mechanism(
                "game_metrics found an inconsistent aggregate row".to_owned(),
            ));
        }
        for (name, value) in table.names().iter().zip(values) {
            let Some(value) = value else {
                continue;
            };
            if !value.is_finite() {
                return Err(SocsimError::Mechanism(format!(
                    "game_metrics found a non-finite aggregate {name:?}"
                )));
            }
            row.push((Cow::Owned(format!("aggregate.{name}")), *value));
        }
        Ok(())
    }

    fn check_report_table<X: ExperimentState>(
        &self,
        world: &GameWorld<G, X>,
        t: u64,
    ) -> socsim_core::Result<()> {
        if self.mixed_source != MixedSource::Reported {
            return Ok(());
        }
        let reports = world.reports.as_ref().ok_or_else(|| {
            SocsimError::Mechanism(
                "game_metrics requires mixed-strategy reports, but the report table is missing"
                    .to_owned(),
            )
        })?;
        if reports.t() != t {
            return Err(SocsimError::Mechanism(format!(
                "game_metrics expected mixed-strategy reports for step {t}, found step {}",
                reports.t()
            )));
        }
        Ok(())
    }

    fn ensure_metrics_state<X: ExperimentState>(
        &self,
        world: &mut GameWorld<G, X>,
    ) -> socsim_core::Result<()> {
        let needs_cumulative = self.mixed_source == MixedSource::PlayedCumulative;
        if self.stop_tol.is_none() && !needs_cumulative {
            return Ok(());
        }
        if world.metrics_state.is_none() {
            let cumulative = needs_cumulative.then(|| {
                self.expected_actions
                    .iter()
                    .map(|&count| vec![0; count])
                    .collect()
            });
            world.metrics_state = Some(InternalMetricsState::new(cumulative));
        }
        let state = world.metrics_state.as_ref().ok_or_else(|| {
            SocsimError::Mechanism("game_metrics state was not initialized".to_owned())
        })?;
        if let Some(prev) = &state.prev {
            if prev.len() != self.action_columns.len() {
                return Err(SocsimError::Mechanism(format!(
                    "game_metrics convergence state has {} columns, expected {}",
                    prev.len(),
                    self.action_columns.len()
                )));
            }
        }
        if needs_cumulative {
            let cumulative = state.cumulative.as_ref().ok_or_else(|| {
                SocsimError::Mechanism(
                    "game_metrics cumulative state was not initialized".to_owned(),
                )
            })?;
            if cumulative.len() != self.expected_actions.len()
                || cumulative
                    .iter()
                    .zip(&self.expected_actions)
                    .any(|(counts, &expected)| counts.len() != expected)
            {
                return Err(SocsimError::Mechanism(
                    "game_metrics cumulative state has the wrong action shape".to_owned(),
                ));
            }
        }
        Ok(())
    }

    fn update_cumulative<X: ExperimentState>(
        &self,
        world: &mut GameWorld<G, X>,
    ) -> socsim_core::Result<()> {
        if self.mixed_source != MixedSource::PlayedCumulative {
            return Ok(());
        }
        let implicit = self.implicit_counts(world)?.map(<[Vec<usize>]>::to_vec);
        let cumulative = world
            .metrics_state
            .as_mut()
            .and_then(|state| state.cumulative.as_mut())
            .ok_or_else(|| {
                SocsimError::Mechanism(
                    "game_metrics cumulative state was not initialized".to_owned(),
                )
            })?;
        if let Some(by_role) = implicit {
            for (role, (totals, counts)) in cumulative.iter_mut().zip(&by_role).enumerate() {
                for (action, (total, &count)) in totals.iter_mut().zip(counts).enumerate() {
                    *total = total.checked_add(count as u64).ok_or_else(|| {
                        SocsimError::Mechanism(format!(
                            "game_metrics cumulative count overflowed for role {role} action {action}"
                        ))
                    })?;
                }
            }
            return Ok(());
        }
        for (seat, action) in world.plays.actions.iter().enumerate() {
            let role = seat % self.expected_actions.len();
            let action = action.as_ref().ok_or_else(|| {
                SocsimError::Mechanism(format!(
                    "game_metrics found an unfilled action at seat {seat}"
                ))
            })?;
            let count = cumulative
                .get_mut(role)
                .and_then(|counts| counts.get_mut(*action))
                .ok_or_else(|| {
                    SocsimError::Mechanism(format!(
                        "game_metrics found action {action} outside role {role}"
                    ))
                })?;
            *count = count.checked_add(1).ok_or_else(|| {
                SocsimError::Mechanism(format!(
                    "game_metrics cumulative count overflowed for role {role} action {action}"
                ))
            })?;
        }
        Ok(())
    }

    fn update_convergence<X: ExperimentState>(
        &self,
        world: &mut GameWorld<G, X>,
        shares: Option<&[f64]>,
    ) -> socsim_core::Result<bool> {
        let Some(tol) = self.stop_tol else {
            return Ok(false);
        };
        let state = world.metrics_state.as_mut().ok_or_else(|| {
            SocsimError::Mechanism("game_metrics state was not initialized".to_owned())
        })?;
        let Some(shares) = shares else {
            state.below = 0;
            return Ok(false);
        };
        if shares.len() != self.action_columns.len() {
            return Err(SocsimError::Mechanism(format!(
                "game_metrics produced {} played-share columns, expected {}",
                shares.len(),
                self.action_columns.len()
            )));
        }
        state.below = match &state.prev {
            Some(prev) => {
                let max_change = prev
                    .iter()
                    .zip(shares)
                    .map(|(previous, current)| (current - previous).abs())
                    .fold(0.0_f64, f64::max);
                if max_change < tol {
                    state.below.saturating_add(1)
                } else {
                    0
                }
            }
            None => 0,
        };
        state.prev = Some(shares.to_vec());
        Ok(state.below >= self.stop_window)
    }

    fn cumulative_profile<X: ExperimentState>(
        &self,
        world: &GameWorld<G, X>,
    ) -> socsim_core::Result<Option<Vec<MixedStrategy>>> {
        let cumulative = world
            .metrics_state
            .as_ref()
            .and_then(|state| state.cumulative.as_ref())
            .ok_or_else(|| {
                SocsimError::Mechanism(
                    "game_metrics cumulative state was not initialized".to_owned(),
                )
            })?;
        let mut profile = Vec::with_capacity(cumulative.len());
        for (role, counts) in cumulative.iter().enumerate() {
            let total: f64 = counts.iter().map(|&count| count as f64).sum();
            if total == 0.0 {
                return Ok(None);
            }
            let probabilities = counts.iter().map(|&count| count as f64 / total).collect();
            let strategy = MixedStrategy::new(probabilities).map_err(|error| {
                SocsimError::Mechanism(format!(
                    "game_metrics produced an invalid cumulative mixed strategy for role {role}: {error}"
                ))
            })?;
            profile.push(strategy);
        }
        Ok(Some(profile))
    }

    fn mixed_profile<X: ExperimentState>(
        &self,
        world: &GameWorld<G, X>,
    ) -> socsim_core::Result<Option<Vec<MixedStrategy>>> {
        let seat_count = world.plays.actions.len();
        if world.plays.seats.len() != seat_count
            || self.expected_actions.is_empty()
            || !seat_count.is_multiple_of(self.expected_actions.len())
        {
            return Err(SocsimError::Mechanism(
                "game_metrics found an inconsistent play arena".to_owned(),
            ));
        }
        if self.mixed_source == MixedSource::PlayedCumulative {
            return self.cumulative_profile(world);
        }
        if seat_count == 0 {
            // The implicit play path only runs when every agent is held, so a
            // reported profile equals the played one there as well.
            let Some(by_role) = self.implicit_counts(world)? else {
                return Ok(None);
            };
            let mut profile = Vec::with_capacity(self.expected_actions.len());
            for (role, (counts, &n_actions)) in
                by_role.iter().zip(&self.expected_actions).enumerate()
            {
                let seats: usize = counts.iter().sum();
                if seats == 0 {
                    return Ok(None);
                }
                let mut probabilities = vec![0.0; n_actions];
                for (probability, &count) in probabilities.iter_mut().zip(counts) {
                    *probability = count as f64 / seats as f64;
                }
                let strategy = MixedStrategy::new(probabilities).map_err(|error| {
                    SocsimError::Mechanism(format!(
                        "game_metrics produced an invalid mixed strategy for role {role}: {error}"
                    ))
                })?;
                profile.push(strategy);
            }
            return Ok(Some(profile));
        }

        let mut sums = self
            .expected_actions
            .iter()
            .map(|&count| vec![0.0; count])
            .collect::<Vec<_>>();
        let mut role_seats = vec![0_usize; self.expected_actions.len()];
        for (seat, (&agent, action)) in world
            .plays
            .seats
            .iter()
            .zip(&world.plays.actions)
            .enumerate()
        {
            let role = seat % self.expected_actions.len();
            let action = action.as_ref().ok_or_else(|| {
                SocsimError::Mechanism(format!(
                    "game_metrics found an unfilled action at seat {seat}"
                ))
            })?;
            if *action >= self.expected_actions[role] {
                return Err(SocsimError::Mechanism(format!(
                    "game_metrics found action {action} outside role {role}"
                )));
            }
            role_seats[role] += 1;
            match self.mixed_source {
                MixedSource::Played => sums[role][*action] += 1.0,
                MixedSource::PlayedCumulative => unreachable!(
                    "the cumulative mixed-strategy source is handled before current-step seats"
                ),
                MixedSource::Reported => {
                    let agent_index = usize::try_from(agent.0).map_err(|_| {
                        SocsimError::Mechanism("agent index exceeds usize".to_owned())
                    })?;
                    let state_slot = world.policies.slot.get(agent_index).ok_or_else(|| {
                        SocsimError::Mechanism(format!("unknown agent {}", agent.0))
                    })?;
                    if *state_slot == PolicyTable::<X::PolicyState>::HELD {
                        sums[role][*action] += 1.0;
                        continue;
                    }
                    let values = world.reported_mixed(agent, role).ok_or_else(|| {
                        SocsimError::Mechanism(format!(
                            "game_metrics found no mixed-strategy report for agent {} role {role}",
                            agent.0
                        ))
                    })?;
                    let strategy = MixedStrategy::new(values.to_vec()).map_err(|error| {
                        SocsimError::Mechanism(format!(
                            "game_metrics found an invalid mixed-strategy report for agent {} role {role}: {error}",
                            agent.0
                        ))
                    })?;
                    if strategy.len() != self.expected_actions[role] {
                        return Err(SocsimError::Mechanism(format!(
                            "game_metrics found a mixed-strategy report with dimension {} for agent {} role {role}, expected {}",
                            strategy.len(),
                            agent.0,
                            self.expected_actions[role]
                        )));
                    }
                    for (sum, probability) in
                        sums[role].iter_mut().zip(strategy.probs().iter().copied())
                    {
                        *sum += probability;
                    }
                }
            }
        }
        if role_seats.contains(&0) {
            return Ok(None);
        }

        let mut profile = Vec::with_capacity(self.expected_actions.len());
        for (role, (mut probabilities, seats)) in sums.into_iter().zip(role_seats).enumerate() {
            for probability in &mut probabilities {
                *probability /= seats as f64;
            }
            let strategy = MixedStrategy::new(probabilities).map_err(|error| {
                SocsimError::Mechanism(format!(
                    "game_metrics produced an invalid mixed strategy for role {role}: {error}"
                ))
            })?;
            if strategy.len() != self.expected_actions[role] {
                return Err(SocsimError::Mechanism(format!(
                    "game_metrics produced dimension {} for role {role}, expected {}",
                    strategy.len(),
                    self.expected_actions[role]
                )));
            }
            profile.push(strategy);
        }
        Ok(Some(profile))
    }

    fn push_strategy_metrics<'a, X: ExperimentState>(
        &'a self,
        world: &GameWorld<G, X>,
        row: &mut Vec<(Cow<'a, str>, f64)>,
    ) -> socsim_core::Result<()> {
        if !self.regret && self.equilibria.is_none() {
            return Ok(());
        }
        let Some(profile) = self.mixed_profile(world)? else {
            return Ok(());
        };
        if self.regret {
            let value = mixed::regret(&world.game, &profile).map_err(|error| {
                SocsimError::Mechanism(format!("game_metrics could not compute regret: {error}"))
            })?;
            row.push((Cow::Borrowed("regret"), value));
        }
        if let Some(equilibria) = &self.equilibria {
            let distance = equilibria
                .iter()
                .map(|equilibrium| {
                    profile
                        .iter()
                        .zip(equilibrium)
                        .flat_map(|(strategy, target)| strategy.probs().iter().zip(target))
                        .map(|(probability, target)| (probability - target).abs())
                        .sum::<f64>()
                })
                .fold(f64::INFINITY, f64::min);
            row.push((Cow::Borrowed("eq_distance"), distance));
        }
        Ok(())
    }
}

impl<G, X> Mechanism<GameWorld<G, X>> for GameMetrics<G>
where
    G: FiniteGame,
    X: ExperimentState,
{
    fn name(&self) -> &str {
        "game_metrics"
    }

    fn phases(&self) -> &'static [Phase] {
        &[Phase::PostStep]
    }

    fn apply(
        &mut self,
        phase: Phase,
        ctx: &mut StepContext<'_, GameWorld<G, X>>,
    ) -> socsim_core::Result<()> {
        if phase != Phase::PostStep {
            return Ok(());
        }
        self.check_shape(ctx.world)?;

        let t = ctx.clock.t();
        self.check_report_table(ctx.world, t)?;
        let mut owned_row = Vec::new();
        let played_shares = self.push_action_metrics(ctx.world, &mut owned_row)?;
        #[cfg(feature = "metrics")]
        if self.action_entropy && self.scheme == MetricScheme::Pooled {
            if let Some(shares) = &played_shares {
                owned_row.push((
                    Cow::Borrowed("action_entropy"),
                    socsim_metrics::stats::shannon_entropy(shares),
                ));
            }
        }
        self.push_payoff_metrics(ctx.world, &mut owned_row)?;
        self.push_aggregate_metrics(ctx.world, t, &mut owned_row)?;
        self.push_population_metrics(ctx.world, &mut owned_row)?;
        self.ensure_metrics_state(ctx.world)?;
        self.update_cumulative(ctx.world)?;
        self.push_strategy_metrics(ctx.world, &mut owned_row)?;
        if owned_row.iter().any(|(_, value)| !value.is_finite()) {
            return Err(SocsimError::Mechanism(
                "game_metrics produced a non-finite metric".to_owned(),
            ));
        }
        let should_stop = self.update_convergence(ctx.world, played_shares.as_deref())?;
        let row: Vec<_> = owned_row
            .iter()
            .map(|(column, value)| (column.as_ref(), *value))
            .collect();
        ctx.recorder.record_row(t, "game", &row);
        if should_stop {
            ctx.request_stop();
        }
        Ok(())
    }
}
