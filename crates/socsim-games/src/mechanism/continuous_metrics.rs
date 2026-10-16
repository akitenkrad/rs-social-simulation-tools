use std::borrow::Cow;
use std::marker::PhantomData;

use socsim_core::{Mechanism, Phase, Recorder, SocsimError, StepContext};

use crate::game::continuous::check_bounds;
use crate::outcome::kernel::for_each_implicit;
use crate::world::InternalMetricsState;
use crate::{
    best_response_iteration, BrIterationOptions, BrStatus, ContinuousGame, Credit, ExperimentState,
    GameError, GameWorld, PayoffTiming,
};

/// Selects the equilibrium used by `game.eq_distance` for a continuous game.
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub enum ContinuousEquilibrium {
    /// Do not compute an equilibrium distance.
    #[default]
    None,
    /// Solve one equilibrium during construction with
    /// [`best_response_iteration`](crate::best_response_iteration), starting
    /// from the midpoint of every role's bounds with the default options.
    ///
    /// Construction fails unless the iteration converges. The iteration
    /// returns a single equilibrium; for games with several equilibria use
    /// [`ContinuousEquilibrium::Given`].
    Solve,
    /// Use the given profile, one action per role.
    Given(Vec<Vec<f64>>),
}

/// Options for per-step metrics of a continuous game.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct ContinuousMetricsOptions {
    equilibrium: ContinuousEquilibrium,
    stop_tol: Option<f64>,
    stop_window: u32,
    payoff_timing: PayoffTiming,
    log_plays: bool,
}

impl ContinuousMetricsOptions {
    /// Constructs the default options: no equilibrium distance and no early
    /// stopping.
    pub fn new() -> Self {
        Self {
            equilibrium: ContinuousEquilibrium::None,
            stop_tol: None,
            stop_window: 0,
            payoff_timing: PayoffTiming::BeforeUpdate,
            log_plays: false,
        }
    }

    /// Selects the equilibrium used by `game.eq_distance`.
    pub fn equilibrium(mut self, equilibrium: ContinuousEquilibrium) -> Self {
        self.equilibrium = equilibrium;
        self
    }

    /// Requests early stopping after `window` consecutive steps in which no
    /// component of any agent's played action changed by `tol` or more.
    ///
    /// The compared vector concatenates, in agent-ID order, the action each
    /// agent played in the step. An agent that did not play keeps its previous
    /// value. `tol` must be finite and greater than zero, and `window` must be
    /// at least one. The values are validated by [`ContinuousMetrics::new`].
    pub fn stop(mut self, tol: f64, window: u32) -> Self {
        self.stop_tol = Some(tol);
        self.stop_window = window;
        self
    }

    /// Selects the timing described for action and payoff columns.
    ///
    /// This changes only [`ContinuousMetrics::describe`], not recorded values.
    /// The default is [`PayoffTiming::BeforeUpdate`].
    pub fn payoff_timing(mut self, timing: PayoffTiming) -> Self {
        self.payoff_timing = timing;
        self
    }

    /// Enables or disables one `game.play` event per materialized encounter.
    ///
    /// Continuous actions are recorded as arrays of numbers. This requires the
    /// materialized play path; the first processed step returns an error when
    /// the simulation uses the implicit path. The default is `false`.
    pub fn log_plays(mut self, on: bool) -> Self {
        self.log_plays = on;
        self
    }
}

impl Default for ContinuousMetricsOptions {
    fn default() -> Self {
        Self::new()
    }
}

/// Records per-step action, payoff, aggregate, and equilibrium-distance
/// metrics of a continuous game.
///
/// This is the continuous counterpart of [`GameMetrics`](crate::GameMetrics)
/// and registers under the same mechanism name, `game_metrics`, so its columns
/// are written to the `game` table. Add it to an assembled simulation with
/// `SimulationBuilder::add_mechanism`. It observes the world in `PostStep` and
/// changes only its own convergence state.
///
/// When every role has the same number of dimensions, the action columns are
/// `mean_action.<d>` and `action_sd.<d>`, pooled over all seats of the step.
/// Otherwise they are reported per role as `mean_action.r<r>-<d>` and
/// `action_sd.r<r>-<d>`. A column whose value is undefined in a step (for
/// example because no seat was played) is omitted from that step's row.
///
/// The per-role columns are intended for games whose action dimensions differ
/// by role. A world for such a game cannot currently be constructed through
/// the supported construction paths (`build_world`, `assemble*`, or
/// scenarios), because initial and fallback actions must be valid for every
/// role. The per-role columns are therefore not recorded through those paths.
/// Worlds that bypass the checks through deserialization or
/// `Simulation::restore` are unsupported.
///
/// The columns are the same on the materialized and the implicit play path
/// (see [`PlayOptions::materialize`](crate::PlayOptions::materialize)).
#[derive(Clone, Debug)]
pub struct ContinuousMetrics<G: ContinuousGame> {
    dims: Vec<usize>,
    pooled: bool,
    mean_columns: Vec<String>,
    sd_columns: Vec<String>,
    equilibrium: Option<Vec<Vec<f64>>>,
    stop_tol: Option<f64>,
    stop_window: u32,
    payoff_timing: PayoffTiming,
    log_plays: bool,
    marker: PhantomData<fn() -> G>,
}

impl<G: ContinuousGame> ContinuousMetrics<G> {
    /// Constructs metrics for the role and dimension shape of `game`.
    ///
    /// Returns [`GameError::Metrics`] when the bounds of `game` are invalid,
    /// when the stopping options are invalid, when a given equilibrium does
    /// not match the dimensions of the game or is not finite, or when solving
    /// the equilibrium does not converge.
    pub fn new(game: &G, options: ContinuousMetricsOptions) -> Result<Self, GameError> {
        check_bounds(game).map_err(|violation| {
            GameError::Metrics(format!("invalid continuous game bounds: {violation}"))
        })?;
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
        let dims: Vec<usize> = (0..game.n_roles())
            .map(|role| game.bounds(role).len())
            .collect();
        let pooled = dims.windows(2).all(|pair| pair[0] == pair[1]);
        let (mean_columns, sd_columns) = if pooled {
            (
                (0..dims[0]).map(|d| format!("mean_action.{d}")).collect(),
                (0..dims[0]).map(|d| format!("action_sd.{d}")).collect(),
            )
        } else {
            let mut means = Vec::new();
            let mut sds = Vec::new();
            for (role, &dim) in dims.iter().enumerate() {
                for d in 0..dim {
                    means.push(format!("mean_action.r{role}-{d}"));
                    sds.push(format!("action_sd.r{role}-{d}"));
                }
            }
            (means, sds)
        };
        let equilibrium = match options.equilibrium {
            ContinuousEquilibrium::None => None,
            ContinuousEquilibrium::Solve => Some(Self::solve(game)?),
            ContinuousEquilibrium::Given(profile) => {
                if profile.len() != dims.len() {
                    return Err(GameError::Metrics(format!(
                        "the given equilibrium has {} roles, expected {}",
                        profile.len(),
                        dims.len()
                    )));
                }
                for (role, (action, &dim)) in profile.iter().zip(&dims).enumerate() {
                    if action.len() != dim {
                        return Err(GameError::Metrics(format!(
                            "the given equilibrium action for role {role} has {} components, expected {dim}",
                            action.len()
                        )));
                    }
                    if action.iter().any(|value| !value.is_finite()) {
                        return Err(GameError::Metrics(format!(
                            "the given equilibrium action for role {role} is not finite"
                        )));
                    }
                }
                Some(profile)
            }
        };
        Ok(Self {
            dims,
            pooled,
            mean_columns,
            sd_columns,
            equilibrium,
            stop_tol: options.stop_tol,
            stop_window: options.stop_window,
            payoff_timing: options.payoff_timing,
            log_plays: options.log_plays,
            marker: PhantomData,
        })
    }

    fn solve(game: &G) -> Result<Vec<Vec<f64>>, GameError> {
        let x0: Vec<Vec<f64>> = (0..game.n_roles())
            .map(|role| {
                game.bounds(role)
                    .iter()
                    .map(|&(lo, hi)| lo + (hi - lo) / 2.0)
                    .collect()
            })
            .collect();
        let solved =
            best_response_iteration(game, &x0, &BrIterationOptions::new()).map_err(|error| {
                GameError::Metrics(format!("failed to solve the equilibrium: {error}"))
            })?;
        if solved.status != BrStatus::Converged {
            return Err(GameError::Metrics(format!(
                "best-response iteration did not converge: residual {}, damping {}",
                solved.residual, solved.damping
            )));
        }
        Ok(solved.profile)
    }

    fn validate_play_recording<X: ExperimentState>(
        &self,
        world: &GameWorld<G, X>,
    ) -> socsim_core::Result<()> {
        if self.log_plays && world.played_counts.is_some() && world.plays.seats.is_empty() {
            return Err(SocsimError::Mechanism(
                "game_metrics log_plays requires the materialized play path; set `PlayOptions::materialize(true)` (not available with `async_population_step`)"
                    .to_owned(),
            ));
        }
        Ok(())
    }

    fn record_plays<X: ExperimentState>(
        &self,
        world: &GameWorld<G, X>,
        t: u64,
        recorder: &mut dyn Recorder,
    ) -> socsim_core::Result<()> {
        for encounter_index in 0..world.plays.n_encounters() {
            let encounter_id = u32::try_from(encounter_index).map_err(|_| {
                SocsimError::Mechanism("game_metrics encounter index exceeds u32::MAX".to_owned())
            })?;
            let id = crate::EncounterId(encounter_id);
            let encounter = world.plays.encounter(id).ok_or_else(|| {
                SocsimError::Mechanism(format!(
                    "game_metrics could not read encounter {encounter_index}"
                ))
            })?;
            let actions = world.plays.actions(id).ok_or_else(|| {
                SocsimError::Mechanism(format!(
                    "game_metrics could not read actions for encounter {encounter_index}"
                ))
            })?;
            let payoffs = world.plays.payoffs(id).ok_or_else(|| {
                SocsimError::Mechanism(format!(
                    "game_metrics could not read payoffs for encounter {encounter_index}"
                ))
            })?;
            let seats: Vec<_> = encounter.seats.iter().map(|agent| agent.0).collect();
            let credit = match encounter.credit {
                Credit::All => "all",
                Credit::Center => "center",
            };
            recorder.record_event(
                t,
                "game.play",
                serde_json::json!({
                    "encounter": encounter.id.0,
                    "seats": seats,
                    "actions": actions,
                    "payoffs": payoffs,
                    "credit": credit,
                }),
            );
        }
        Ok(())
    }

    /// Returns the equilibrium profile used by `game.eq_distance`, if any.
    pub fn equilibrium(&self) -> Option<&[Vec<f64>]> {
        self.equilibrium.as_deref()
    }

    /// Describes the metric column patterns emitted by this mechanism.
    ///
    /// Each pattern is a dotted template in which a segment written wholly as
    /// `{name}` stands for exactly one segment of the column name, and every
    /// other segment is literal. Braces never share a segment with literal
    /// text, so the patterns can be copied unchanged into the
    /// `[[metric_patterns]]` of a runvault `runvault.toml`.
    ///
    /// The per-role patterns describe columns for games whose action
    /// dimensions differ by role. A world for such a game cannot currently be
    /// constructed through the supported construction paths (`build_world`,
    /// `assemble*`, or scenarios), so the columns are not recorded there.
    /// Worlds that bypass the initial- and fallback-action checks through
    /// deserialization or `Simulation::restore` are unsupported.
    pub fn describe(&self) -> Vec<(String, String)> {
        let (mean, sd) = match (self.pooled, self.payoff_timing) {
            (true, PayoffTiming::BeforeUpdate) => (
                (
                    "game.mean_action.{d}",
                    "Mean of component d of the actions over all seats played in the step, pooled across roles.",
                ),
                (
                    "game.action_sd.{d}",
                    "Population standard deviation of component d of the actions over all seats played in the step, pooled across roles.",
                ),
            ),
            (false, PayoffTiming::BeforeUpdate) => (
                (
                    "game.mean_action.{role_component}",
                    "Mean of component d of the actions over the seats of role r played in the step. `{role_component}` is `r<r>-<d>`.",
                ),
                (
                    "game.action_sd.{role_component}",
                    "Population standard deviation of component d of the actions over the seats of role r played in the step. `{role_component}` is `r<r>-<d>`.",
                ),
            ),
            (true, PayoffTiming::AfterUpdate) => (
                (
                    "game.mean_action.{d}",
                    "Mean of component d of the actions held after every update of the step (asynchronous update), over all played seats and pooled across roles.",
                ),
                (
                    "game.action_sd.{d}",
                    "Population standard deviation of component d of the actions held after every update of the step (asynchronous update), over all played seats and pooled across roles.",
                ),
            ),
            (false, PayoffTiming::AfterUpdate) => (
                (
                    "game.mean_action.{role_component}",
                    "Mean of component d of the actions held after every update of the step (asynchronous update), over the played seats of role r. `{role_component}` is `r<r>-<d>`.",
                ),
                (
                    "game.action_sd.{role_component}",
                    "Population standard deviation of component d of the actions held after every update of the step (asynchronous update), over the played seats of role r. `{role_component}` is `r<r>-<d>`.",
                ),
            ),
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
            (mean.0.to_owned(), mean.1.to_owned()),
            (sd.0.to_owned(), sd.1.to_owned()),
            (
                "game.mean_payoff".to_owned(),
                mean_payoff.to_owned(),
            ),
            (
                "game.payoff_sd".to_owned(),
                payoff_sd.to_owned(),
            ),
            (
                "game.aggregate.{name}".to_owned(),
                "Public game aggregate for the completed step; present only when the simulation was assembled with aggregates."
                    .to_owned(),
            ),
        ];
        if self.equilibrium.is_some() {
            descriptions.push((
                "game.eq_distance".to_owned(),
                "Sum over roles of the L1 distance between the mean action of the role's seats in the step and the equilibrium action of the role."
                    .to_owned(),
            ));
        }
        descriptions
    }

    fn check_shape<X: ExperimentState>(&self, world: &GameWorld<G, X>) -> socsim_core::Result<()> {
        if world.game.n_roles() != self.dims.len() {
            return Err(SocsimError::Mechanism(format!(
                "game_metrics expected {} roles, got {}",
                self.dims.len(),
                world.game.n_roles()
            )));
        }
        for (role, &expected) in self.dims.iter().enumerate() {
            let actual = world.game.bounds(role).len();
            if actual != expected {
                return Err(SocsimError::Mechanism(format!(
                    "game_metrics expected {expected} dimensions for role {role}, got {actual}"
                )));
            }
        }
        if world.plays.n_roles != self.dims.len() {
            return Err(SocsimError::Mechanism(format!(
                "game_metrics expected an arena with {} roles, got {}",
                self.dims.len(),
                world.plays.n_roles
            )));
        }
        Ok(())
    }

    /// Returns the seats played in the step as `(role, action)` pairs.
    ///
    /// On the materialized path the seats come from the play arena. On the
    /// implicit path the pattern recorded by the play step is traversed with
    /// the same function the play step uses, so the encounters, seats, and
    /// roles (the seat's position in its encounter) are identical.
    fn seats<'w, X: ExperimentState>(
        &self,
        world: &'w GameWorld<G, X>,
    ) -> socsim_core::Result<Vec<(usize, Cow<'w, [f64]>)>> {
        let n_roles = self.dims.len();
        let mut seats = Vec::new();
        if world.plays.actions.is_empty() {
            let Some(played) = world.played_counts.as_ref() else {
                return Ok(seats);
            };
            let Some(pattern) = played.pattern() else {
                return Ok(seats);
            };
            for_each_implicit(
                pattern,
                &world.topology,
                &world.ids,
                &world.held,
                |_id, encounter_seats, _credit, actions| {
                    if encounter_seats.len() != n_roles || actions.len() != n_roles {
                        return Err(SocsimError::Mechanism(format!(
                            "game_metrics found an implicit encounter with {} seats, expected {n_roles}",
                            encounter_seats.len()
                        )));
                    }
                    for (role, action) in actions.iter().enumerate() {
                        seats.push((role, Cow::Owned(action.clone())));
                    }
                    Ok(())
                },
            )?;
        } else {
            if world.plays.seats.len() != world.plays.actions.len()
                || !world.plays.actions.len().is_multiple_of(n_roles)
            {
                return Err(SocsimError::Mechanism(
                    "game_metrics found an inconsistent play arena".to_owned(),
                ));
            }
            for (seat, action) in world.plays.actions.iter().enumerate() {
                let action = action.as_ref().ok_or_else(|| {
                    SocsimError::Mechanism(format!(
                        "game_metrics found an unfilled action at seat {seat}"
                    ))
                })?;
                seats.push((seat % n_roles, Cow::Borrowed(action.as_slice())));
            }
        }
        for (role, action) in &seats {
            if action.len() != self.dims[*role] {
                return Err(SocsimError::Mechanism(format!(
                    "game_metrics found an action with {} components for role {role}, expected {}",
                    action.len(),
                    self.dims[*role]
                )));
            }
        }
        Ok(seats)
    }

    fn push_action_metrics<'a>(
        &'a self,
        seats: &[(usize, &[f64])],
        row: &mut Vec<(Cow<'a, str>, f64)>,
    ) -> Option<Vec<Vec<f64>>> {
        let n_roles = self.dims.len();
        let mut role_sums: Vec<Vec<f64>> = self.dims.iter().map(|&dim| vec![0.0; dim]).collect();
        let mut role_seats = vec![0_usize; n_roles];
        for &(role, action) in seats {
            role_seats[role] += 1;
            for (sum, value) in role_sums[role].iter_mut().zip(action) {
                *sum += value;
            }
        }
        if self.pooled {
            let total = seats.len();
            if total != 0 {
                let dim = self.dims[0];
                let means: Vec<f64> = (0..dim)
                    .map(|d| role_sums.iter().map(|sums| sums[d]).sum::<f64>() / total as f64)
                    .collect();
                let mut squares = vec![0.0; dim];
                for &(_, action) in seats {
                    for ((square, value), mean) in squares.iter_mut().zip(action).zip(&means) {
                        let deviation = value - mean;
                        *square += deviation * deviation;
                    }
                }
                for (column, &mean) in self.mean_columns.iter().zip(&means) {
                    row.push((Cow::Borrowed(column.as_str()), mean));
                }
                for (column, square) in self.sd_columns.iter().zip(squares) {
                    row.push((
                        Cow::Borrowed(column.as_str()),
                        (square / total as f64).sqrt(),
                    ));
                }
            }
        } else {
            let role_means: Vec<Option<Vec<f64>>> = role_sums
                .iter()
                .zip(&role_seats)
                .map(|(sums, &count)| {
                    (count != 0).then(|| sums.iter().map(|sum| sum / count as f64).collect())
                })
                .collect();
            let mut role_squares: Vec<Vec<f64>> =
                self.dims.iter().map(|&dim| vec![0.0; dim]).collect();
            for &(role, action) in seats {
                let Some(means) = &role_means[role] else {
                    continue;
                };
                for ((square, value), mean) in role_squares[role].iter_mut().zip(action).zip(means)
                {
                    let deviation = value - mean;
                    *square += deviation * deviation;
                }
            }
            let mut offset = 0;
            let mut sd_values = Vec::new();
            for (role, &dim) in self.dims.iter().enumerate() {
                if let Some(means) = &role_means[role] {
                    for (d, &mean) in means.iter().enumerate() {
                        row.push((Cow::Borrowed(self.mean_columns[offset + d].as_str()), mean));
                        sd_values.push((
                            offset + d,
                            (role_squares[role][d] / role_seats[role] as f64).sqrt(),
                        ));
                    }
                }
                offset += dim;
            }
            for (index, value) in sd_values {
                row.push((Cow::Borrowed(self.sd_columns[index].as_str()), value));
            }
        }
        // Per-role means for the equilibrium distance; undefined when some
        // role has no played seat.
        if role_seats.contains(&0) {
            return None;
        }
        Some(
            role_sums
                .into_iter()
                .zip(role_seats)
                .map(|(sums, count)| sums.into_iter().map(|sum| sum / count as f64).collect())
                .collect(),
        )
    }

    fn push_payoff_metrics<X: ExperimentState>(
        &self,
        world: &GameWorld<G, X>,
        row: &mut Vec<(Cow<'_, str>, f64)>,
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
        row.push((Cow::Borrowed("mean_payoff"), mean));
        row.push((
            Cow::Borrowed("payoff_sd"),
            (squared_deviations / payoffs.len() as f64).sqrt(),
        ));
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

    fn eq_distance(&self, role_means: &[Vec<f64>]) -> Option<f64> {
        let equilibrium = self.equilibrium.as_ref()?;
        Some(
            role_means
                .iter()
                .zip(equilibrium)
                .flat_map(|(mean, target)| mean.iter().zip(target))
                .map(|(value, target)| (value - target).abs())
                .sum(),
        )
    }

    /// Width of one agent's slot in the convergence vector.
    fn slot_width(&self) -> usize {
        self.dims.iter().copied().max().unwrap_or(0)
    }

    /// Builds the per-agent action vector of the step in agent-ID order.
    ///
    /// Each agent occupies `slot_width` components; an action with fewer
    /// components is padded with zeros. An agent that did not play keeps its
    /// previous slot, or its held action when there is no previous vector.
    fn agent_vector<X: ExperimentState>(
        &self,
        world: &GameWorld<G, X>,
        prev: Option<&[f64]>,
    ) -> socsim_core::Result<Vec<f64>> {
        let width = self.slot_width();
        let n_agents = world.ids.len();
        // First seat of each agent in the arena; used when the step record
        // does not carry an action (per-encounter decisions).
        let mut first_seat: Vec<Option<&Vec<f64>>> = vec![None; n_agents];
        for (agent, action) in world.plays.seats.iter().zip(&world.plays.actions) {
            let index = usize::try_from(agent.0)
                .map_err(|_| SocsimError::Mechanism("agent index exceeds usize".to_owned()))?;
            if let (Some(slot), Some(action)) = (first_seat.get_mut(index), action.as_ref()) {
                if slot.is_none() {
                    *slot = Some(action);
                }
            }
        }
        let mut vector = vec![0.0; n_agents * width];
        for (row, &agent) in world.ids.iter().enumerate() {
            let index = usize::try_from(agent.0)
                .map_err(|_| SocsimError::Mechanism("agent index exceeds usize".to_owned()))?;
            let recorded = world
                .step_history
                .view(agent)
                .and_then(|view| view.last())
                .and_then(|record| record.action.as_ref());
            let played = recorded.or_else(|| first_seat.get(index).copied().flatten());
            let slot = &mut vector[row * width..(row + 1) * width];
            let source: &[f64] = match (played, prev) {
                (Some(action), _) => action,
                (None, Some(prev)) => &prev[row * width..(row + 1) * width],
                (None, None) => world
                    .held
                    .get(index)
                    .ok_or_else(|| SocsimError::Mechanism(format!("unknown agent {}", agent.0)))?,
            };
            if source.len() > width {
                return Err(SocsimError::Mechanism(format!(
                    "game_metrics found an action with {} components for agent {}, expected at most {width}",
                    source.len(),
                    agent.0
                )));
            }
            slot[..source.len()].copy_from_slice(source);
        }
        Ok(vector)
    }

    fn update_convergence<X: ExperimentState>(
        &self,
        world: &mut GameWorld<G, X>,
    ) -> socsim_core::Result<bool> {
        let Some(tol) = self.stop_tol else {
            return Ok(false);
        };
        let expected = world.ids.len() * self.slot_width();
        if world.metrics_state.is_none() {
            world.metrics_state = Some(InternalMetricsState::new(None));
        }
        let prev = world
            .metrics_state
            .as_ref()
            .and_then(|state| state.prev.as_ref());
        if let Some(prev) = prev {
            if prev.len() != expected {
                return Err(SocsimError::Mechanism(format!(
                    "game_metrics convergence state has {} components, expected {expected}",
                    prev.len()
                )));
            }
        }
        let current = self.agent_vector(world, prev.map(Vec::as_slice))?;
        if current.iter().any(|value| !value.is_finite()) {
            return Err(SocsimError::Mechanism(
                "game_metrics found a non-finite played action".to_owned(),
            ));
        }
        let state = world.metrics_state.as_mut().ok_or_else(|| {
            SocsimError::Mechanism("game_metrics state was not initialized".to_owned())
        })?;
        state.below = match &state.prev {
            Some(prev) => {
                let max_change = prev
                    .iter()
                    .zip(&current)
                    .map(|(previous, value)| (value - previous).abs())
                    .fold(0.0_f64, f64::max);
                if max_change < tol {
                    state.below.saturating_add(1)
                } else {
                    0
                }
            }
            None => 0,
        };
        state.prev = Some(current);
        Ok(state.below >= self.stop_window)
    }
}

impl<G, X> Mechanism<GameWorld<G, X>> for ContinuousMetrics<G>
where
    G: ContinuousGame,
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
        self.validate_play_recording(ctx.world)?;
        let t = ctx.clock.t();
        let mut owned_row = Vec::new();
        let seats = self.seats(ctx.world)?;
        let seats: Vec<(usize, &[f64])> = seats
            .iter()
            .map(|(role, action)| (*role, action.as_ref()))
            .collect();
        let role_means = self.push_action_metrics(&seats, &mut owned_row);
        self.push_payoff_metrics(ctx.world, &mut owned_row)?;
        self.push_aggregate_metrics(ctx.world, t, &mut owned_row)?;
        if let Some(distance) = role_means
            .as_deref()
            .and_then(|means| self.eq_distance(means))
        {
            owned_row.push((Cow::Borrowed("eq_distance"), distance));
        }
        if owned_row.iter().any(|(_, value)| !value.is_finite()) {
            return Err(SocsimError::Mechanism(
                "game_metrics produced a non-finite metric".to_owned(),
            ));
        }
        let should_stop = self.update_convergence(ctx.world)?;
        let row: Vec<_> = owned_row
            .iter()
            .map(|(column, value)| (column.as_ref(), *value))
            .collect();
        if self.log_plays {
            self.record_plays(ctx.world, t, &mut *ctx.recorder)?;
        }
        ctx.recorder.record_row(t, "game", &row);
        if should_stop {
            ctx.request_stop();
        }
        Ok(())
    }
}
