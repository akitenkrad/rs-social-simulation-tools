use socsim_core::{AgentId, SimRng};

use crate::{
    learn::{
        automaton::{validate_two_action_game, AutomatonKind, AutomatonState},
        fictitious::FictitiousBelief,
        logit::{EstimateMode, LogitState},
        resolve_tie,
        roth_erev::RothErevState,
        sample_probabilities, LearnError, TieBreak,
    },
    AutomatonPolicyState, BeliefState, DecisionContext, DecisionRule, DecisionScope,
    EncounterOutcome, EstimateState, ExperimentState, FiniteGame, LearnContext, PropensityState,
    RuleError, RuleInitContext, Slot, Stateless,
};

use super::pure_strategy;

const PROBABILITY_TOLERANCE: f64 = 1.0e-12;

fn unsupported(error: LearnError) -> RuleError {
    RuleError::Unsupported(error.to_string())
}

fn assigned_role<G: FiniteGame, X: ExperimentState>(
    ctx: &DecisionContext<'_, G, X>,
    rule: &str,
) -> Result<usize, RuleError> {
    ctx.role()
        .ok_or_else(|| RuleError::Unsupported(format!("{rule} requires an assigned role")))
}

fn get_or_try_insert<T, E>(
    entries: &mut Vec<(AgentId, T)>,
    opponent: AgentId,
    make: impl FnOnce() -> Result<T, E>,
) -> Result<&mut T, E> {
    let index = match entries.binary_search_by_key(&opponent, |(agent, _)| *agent) {
        Ok(index) => return Ok(&mut entries[index].1),
        Err(index) => index,
    };
    entries.insert(index, (opponent, make()?));
    Ok(&mut entries[index].1)
}

fn decision_opponent<G: FiniteGame, X: ExperimentState>(
    ctx: &DecisionContext<'_, G, X>,
    rule: &str,
) -> Result<AgentId, RuleError> {
    ctx.opponents()
        .first()
        .copied()
        .ok_or_else(|| RuleError::Unsupported(format!("{rule} requires an opponent")))
}

fn outcome_opponent<A>(
    outcome: &EncounterOutcome<'_, A>,
    rule: &str,
) -> Result<(usize, AgentId), RuleError> {
    outcome
        .seats
        .iter()
        .copied()
        .enumerate()
        .find(|(role, _)| *role != outcome.role)
        .ok_or_else(|| RuleError::Unsupported(format!("{rule} requires an opponent")))
}

fn outcome_action(outcome: &EncounterOutcome<'_, usize>, role: usize) -> Result<usize, RuleError> {
    outcome
        .actions
        .get(role)
        .and_then(Option::as_ref)
        .copied()
        .ok_or_else(|| {
            RuleError::InvalidAction(format!("encounter role {role} has no realized action"))
        })
}

/// A fixed mixed strategy shared by every role of a finite game.
#[derive(Clone, Debug)]
pub struct Mixed {
    probabilities: Vec<f64>,
}

impl Mixed {
    /// Constructs a rule from action probabilities in action-index order.
    pub fn new(q: Vec<f64>) -> Self {
        Self { probabilities: q }
    }
}

impl<G, X> DecisionRule<G, X> for Mixed
where
    G: FiniteGame,
    X: ExperimentState,
    X::PolicyState: Slot<Stateless>,
{
    fn name(&self) -> &str {
        "mixed"
    }

    fn init_state(&self, _ctx: &RuleInitContext, game: &G) -> Result<X::PolicyState, RuleError> {
        for (index, &value) in self.probabilities.iter().enumerate() {
            if !value.is_finite() || value < 0.0 {
                return Err(unsupported(LearnError::InvalidProbability { index, value }));
            }
        }
        let total: f64 = self.probabilities.iter().sum();
        if !total.is_finite() || (total - 1.0).abs() > PROBABILITY_TOLERANCE {
            return Err(RuleError::Unsupported(format!(
                "mixed probabilities must sum to one within {PROBABILITY_TOLERANCE}, got {total}"
            )));
        }
        for role in 0..game.n_roles() {
            let expected = game.n_actions(role);
            if self.probabilities.len() != expected {
                return Err(unsupported(LearnError::LengthMismatch {
                    what: "mixed probabilities",
                    expected,
                    actual: self.probabilities.len(),
                }));
            }
        }
        Ok(<X::PolicyState as Slot<Stateless>>::from_slot(Stateless))
    }

    fn decide(
        &self,
        state: &mut X::PolicyState,
        _ctx: &DecisionContext<'_, G, X>,
        rng: &mut SimRng,
    ) -> Result<usize, RuleError> {
        state.slot().ok_or(RuleError::MissingSlot("Stateless"))?;
        sample_probabilities(&self.probabilities, rng).map_err(unsupported)
    }

    fn mixed(&self, state: &X::PolicyState, role: usize, game: &G) -> Option<Vec<f64>> {
        <X::PolicyState as Slot<Stateless>>::slot(state)?;
        if role >= game.n_roles() || self.probabilities.len() != game.n_actions(role) {
            return None;
        }
        Some(self.probabilities.clone())
    }
}

/// A best response to previously observed opponent actions.
///
/// The observation used depends on the decision scope:
///
/// - [`DecisionScope::PerStep`] uses each current opponent's action from the
///   previous step.
/// - [`DecisionScope::PerEncounter`] uses the latest pair-history action of
///   that encounter's opponent.
/// - [`DecisionScope::PerAgentRole`] best responds to the uniform empirical
///   distribution over the latest pair-history actions of the current role's
///   opponents that have a retained record.
///
/// If the applicable history has no usable action, the configured first action
/// is returned. Fine-grained scopes support only two-player games.
#[derive(Clone, Copy, Debug)]
pub struct MyopicBestResponse {
    tie_break: TieBreak,
    first_action: usize,
}

impl MyopicBestResponse {
    /// Constructs a rule that initially chooses action zero and breaks ties by
    /// the lowest action index.
    pub fn new() -> Self {
        Self {
            tie_break: TieBreak::LowestIndex,
            first_action: 0,
        }
    }

    /// Chooses the lowest action index when several best responses tie.
    pub fn tie_break_lowest(mut self) -> Self {
        self.tie_break = TieBreak::LowestIndex;
        self
    }

    /// Chooses uniformly among tied best responses.
    pub fn tie_break_random(mut self) -> Self {
        self.tie_break = TieBreak::Random;
        self
    }

    /// Sets the action returned when the applicable history has no action.
    pub fn first_action(mut self, action: usize) -> Self {
        self.first_action = action;
        self
    }

    fn previous_step_profile<G, X>(
        &self,
        game: &G,
        ctx: &DecisionContext<'_, G, X>,
        role: usize,
    ) -> Option<Vec<usize>>
    where
        G: FiniteGame,
        X: ExperimentState,
    {
        let mut profile = vec![0; game.n_roles()];
        if let Some(encounter) = ctx.encounter() {
            if encounter.seats.len() != game.n_roles() {
                return None;
            }
            for (other_role, &opponent) in encounter.seats.iter().enumerate() {
                if other_role == role {
                    continue;
                }
                profile[other_role] = *ctx.opponent_history(opponent)?.last()??;
            }
        } else {
            if ctx.opponents().len() != game.n_roles().saturating_sub(1) {
                return None;
            }
            let mut opponents = ctx.opponents().iter().copied();
            for (other_role, slot) in profile.iter_mut().enumerate() {
                if other_role == role {
                    continue;
                }
                let opponent = opponents.next()?;
                *slot = *ctx.opponent_history(opponent)?.last()??;
            }
        }
        Some(profile)
    }

    fn pair_opponent_action<G, X>(
        ctx: &DecisionContext<'_, G, X>,
        opponent: AgentId,
    ) -> Option<usize>
    where
        G: FiniteGame,
        X: ExperimentState,
    {
        ctx.pair_history(opponent)
            .and_then(|history| history.iter().last())
            .map(|(_, _, opponent_action)| *opponent_action)
    }

    fn best_response_to_opponent_actions<G: FiniteGame>(
        &self,
        game: &G,
        role: usize,
        opponent_actions: &[usize],
        rng: &mut SimRng,
    ) -> Result<usize, RuleError> {
        let mut best_value = f64::NEG_INFINITY;
        let mut best_actions = Vec::new();
        for action in 0..game.n_actions(role) {
            let value = opponent_actions
                .iter()
                .map(|&opponent_action| {
                    let profile = if role == 0 {
                        [action, opponent_action]
                    } else {
                        [opponent_action, action]
                    };
                    game.payoff(role, &profile)
                })
                .sum::<f64>();
            if !value.is_finite() {
                return Err(unsupported(LearnError::NonFiniteCalculation));
            }
            if value > best_value {
                best_value = value;
                best_actions.clear();
                best_actions.push(action);
            } else if value == best_value {
                best_actions.push(action);
            }
        }
        resolve_tie(&best_actions, self.tie_break, rng).map_err(unsupported)
    }
}

impl Default for MyopicBestResponse {
    fn default() -> Self {
        Self::new()
    }
}

impl<G, X> DecisionRule<G, X> for MyopicBestResponse
where
    G: FiniteGame,
    X: ExperimentState,
    X::PolicyState: Slot<Stateless>,
{
    fn name(&self) -> &str {
        "myopic_best_response"
    }

    fn init_state(&self, _ctx: &RuleInitContext, game: &G) -> Result<X::PolicyState, RuleError> {
        for role in 0..game.n_roles() {
            if self.first_action >= game.n_actions(role) {
                return Err(RuleError::Unsupported(format!(
                    "initial action {} is invalid for role {role}",
                    self.first_action
                )));
            }
        }
        Ok(<X::PolicyState as Slot<Stateless>>::from_slot(Stateless))
    }

    fn decide(
        &self,
        state: &mut X::PolicyState,
        ctx: &DecisionContext<'_, G, X>,
        rng: &mut SimRng,
    ) -> Result<usize, RuleError> {
        state.slot().ok_or(RuleError::MissingSlot("Stateless"))?;
        let role = assigned_role(ctx, "MyopicBestResponse")?;
        match ctx.scope() {
            DecisionScope::PerStep => {
                let Some(mut profile) = self.previous_step_profile(ctx.game(), ctx, role) else {
                    return Ok(self.first_action);
                };

                let mut best_value = f64::NEG_INFINITY;
                let mut best_actions = Vec::new();
                for action in 0..ctx.game().n_actions(role) {
                    profile[role] = action;
                    let value = ctx.game().payoff(role, &profile);
                    if !value.is_finite() {
                        return Err(unsupported(LearnError::NonFiniteCalculation));
                    }
                    if value > best_value {
                        best_value = value;
                        best_actions.clear();
                        best_actions.push(action);
                    } else if value == best_value {
                        best_actions.push(action);
                    }
                }
                resolve_tie(&best_actions, self.tie_break, rng).map_err(unsupported)
            }
            scope @ (DecisionScope::PerAgentRole | DecisionScope::PerEncounter) => {
                if ctx.game().n_roles() != 2 {
                    return Err(RuleError::Unsupported(format!(
                        "MyopicBestResponse requires exactly two roles for {scope:?} decisions"
                    )));
                }
                let opponent_actions = match scope {
                    DecisionScope::PerEncounter => {
                        let [opponent] = ctx.opponents() else {
                            return Err(RuleError::Unsupported(
                                "MyopicBestResponse requires exactly one opponent per encounter"
                                    .to_owned(),
                            ));
                        };
                        Self::pair_opponent_action(ctx, *opponent)
                            .into_iter()
                            .collect::<Vec<_>>()
                    }
                    DecisionScope::PerAgentRole => ctx
                        .opponents()
                        .iter()
                        .filter_map(|&opponent| Self::pair_opponent_action(ctx, opponent))
                        .collect(),
                    DecisionScope::PerStep => unreachable!("scope was matched above"),
                };
                if opponent_actions.is_empty() {
                    return Ok(self.first_action);
                }
                self.best_response_to_opponent_actions(ctx.game(), role, &opponent_actions, rng)
            }
        }
    }

    fn needs_pair_history(&self) -> bool {
        true
    }
}

/// Simultaneous fictitious play with fixed pseudo-counts and tie handling.
#[derive(Clone, Debug)]
pub struct FictitiousPlay {
    initial_counts: Option<Vec<f64>>,
    tie_break: TieBreak,
}

impl FictitiousPlay {
    /// Constructs zero-pseudo-count fictitious play with lowest-index ties.
    pub fn new() -> Self {
        Self {
            initial_counts: None,
            tie_break: TieBreak::LowestIndex,
        }
    }

    /// Chooses the lowest action index when best responses tie.
    pub fn tie_break_lowest(mut self) -> Self {
        self.tie_break = TieBreak::LowestIndex;
        self
    }

    /// Chooses uniformly among tied best responses.
    pub fn tie_break_random(mut self) -> Self {
        self.tie_break = TieBreak::Random;
        self
    }

    /// Sets unnormalized initial pseudo-counts for opponent actions.
    pub fn initial_counts(mut self, counts: Vec<f64>) -> Self {
        self.initial_counts = Some(counts);
        self
    }

    fn initial<G: FiniteGame>(&self, game: &G, role: usize) -> Result<FictitiousBelief, RuleError> {
        let opponent = match role {
            0 => 1,
            1 => 0,
            _ => {
                return Err(unsupported(LearnError::InvalidRole {
                    role,
                    n_roles: game.n_roles(),
                }))
            }
        };
        let counts = self
            .initial_counts
            .clone()
            .unwrap_or_else(|| vec![0.0; game.n_actions(opponent)]);
        FictitiousBelief::new(game, role, counts, self.tie_break).map_err(unsupported)
    }

    fn observe(
        belief: &mut FictitiousBelief,
        outcome: &EncounterOutcome<'_, usize>,
    ) -> Result<(), RuleError> {
        let opponent_role = 1_usize.checked_sub(outcome.role).ok_or_else(|| {
            RuleError::Unsupported("FictitiousPlay requires two roles".to_owned())
        })?;
        belief
            .observe(outcome_action(outcome, opponent_role)?)
            .map_err(unsupported)
    }
}

impl Default for FictitiousPlay {
    fn default() -> Self {
        Self::new()
    }
}

impl<G, X> DecisionRule<G, X> for FictitiousPlay
where
    G: FiniteGame,
    X: ExperimentState,
    X::PolicyState: Slot<BeliefState>,
{
    fn name(&self) -> &str {
        "fictitious_play"
    }

    fn init_state(&self, _ctx: &RuleInitContext, game: &G) -> Result<X::PolicyState, RuleError> {
        if game.n_roles() != 2 {
            return Err(unsupported(LearnError::UnsupportedRoleCount {
                expected: 2,
                actual: game.n_roles(),
            }));
        }
        let global = self.initial(game, 0)?;
        self.initial(game, 1)?;
        Ok(<X::PolicyState as Slot<BeliefState>>::from_slot(
            BeliefState::new(global),
        ))
    }

    fn decide(
        &self,
        state: &mut X::PolicyState,
        ctx: &DecisionContext<'_, G, X>,
        rng: &mut SimRng,
    ) -> Result<usize, RuleError> {
        let role = assigned_role(ctx, "FictitiousPlay")?;
        let state = state
            .slot_mut()
            .ok_or(RuleError::MissingSlot("BeliefState"))?;
        let belief = if ctx.scope() == DecisionScope::PerEncounter {
            let opponent = decision_opponent(ctx, "FictitiousPlay")?;
            get_or_try_insert(&mut state.by_opponent, opponent, || {
                self.initial(ctx.game(), role)
            })?
        } else {
            &mut state.global
        };
        belief
            .best_response(ctx.game(), role, rng)
            .map_err(unsupported)
    }

    fn learn_step(
        &self,
        state: &mut X::PolicyState,
        ctx: &LearnContext<'_, G, X>,
    ) -> Result<(), RuleError> {
        if ctx.scope() == DecisionScope::PerEncounter {
            return Ok(());
        }
        let state = state
            .slot_mut()
            .ok_or(RuleError::MissingSlot("BeliefState"))?;
        for outcome in ctx.outcomes() {
            Self::observe(&mut state.global, &outcome)?;
        }
        Ok(())
    }

    fn learn_encounter(
        &self,
        state: &mut X::PolicyState,
        outcome: &EncounterOutcome<'_, usize>,
        ctx: &LearnContext<'_, G, X>,
    ) -> Result<(), RuleError> {
        if ctx.scope() != DecisionScope::PerEncounter {
            return Ok(());
        }
        let (_, opponent) = outcome_opponent(outcome, "FictitiousPlay")?;
        let state = state
            .slot_mut()
            .ok_or(RuleError::MissingSlot("BeliefState"))?;
        let belief = get_or_try_insert(&mut state.by_opponent, opponent, || {
            self.initial(ctx.game(), outcome.role)
        })?;
        Self::observe(belief, outcome)
    }

    fn needs_encounter_learning(&self) -> bool {
        true
    }

    fn mixed(&self, state: &X::PolicyState, role: usize, game: &G) -> Option<Vec<f64>> {
        let state = <X::PolicyState as Slot<BeliefState>>::slot(state)?;
        if role >= game.n_roles() {
            return None;
        }
        let best = state.global.best_responses(game, role).ok()?;
        match state.global.tie_break() {
            TieBreak::LowestIndex => pure_strategy(game.n_actions(role), *best.first()?),
            TieBreak::Random => {
                let mut probabilities = vec![0.0; game.n_actions(role)];
                let probability = 1.0 / best.len() as f64;
                for action in best {
                    *probabilities.get_mut(action)? = probability;
                }
                Some(probabilities)
            }
        }
    }
}

/// Logit choice based on either recent or empirical opponent behavior.
#[derive(Clone, Copy, Debug)]
pub struct LogitChoice {
    lambda: f64,
    estimate: EstimateMode,
}

impl LogitChoice {
    /// Constructs empirical-frequency logit choice with inverse temperature
    /// `lambda`.
    pub fn new(lambda: f64) -> Self {
        Self {
            lambda,
            estimate: EstimateMode::EmpiricalFrequency,
        }
    }

    /// Selects how opponent behavior is estimated.
    pub fn estimate(mut self, estimate: EstimateMode) -> Self {
        self.estimate = estimate;
        self
    }

    fn initial<G: FiniteGame>(&self, game: &G, role: usize) -> Result<LogitState, RuleError> {
        LogitState::new(game, role, self.lambda, self.estimate).map_err(unsupported)
    }

    fn observe(
        estimate: &mut LogitState,
        outcome: &EncounterOutcome<'_, usize>,
    ) -> Result<(), RuleError> {
        let opponent_role = 1_usize
            .checked_sub(outcome.role)
            .ok_or_else(|| RuleError::Unsupported("LogitChoice requires two roles".to_owned()))?;
        estimate
            .observe(outcome_action(outcome, opponent_role)?)
            .map_err(unsupported)
    }
}

impl<G, X> DecisionRule<G, X> for LogitChoice
where
    G: FiniteGame,
    X: ExperimentState,
    X::PolicyState: Slot<EstimateState>,
{
    fn name(&self) -> &str {
        "logit"
    }

    fn init_state(&self, _ctx: &RuleInitContext, game: &G) -> Result<X::PolicyState, RuleError> {
        let global = self.initial(game, 0)?;
        for role in 1..game.n_roles() {
            self.initial(game, role)?;
        }
        Ok(<X::PolicyState as Slot<EstimateState>>::from_slot(
            EstimateState::new(global),
        ))
    }

    fn decide(
        &self,
        state: &mut X::PolicyState,
        ctx: &DecisionContext<'_, G, X>,
        rng: &mut SimRng,
    ) -> Result<usize, RuleError> {
        let role = assigned_role(ctx, "LogitChoice")?;
        let state = state
            .slot_mut()
            .ok_or(RuleError::MissingSlot("EstimateState"))?;
        let estimate = if ctx.scope() == DecisionScope::PerEncounter {
            let opponent = decision_opponent(ctx, "LogitChoice")?;
            get_or_try_insert(&mut state.by_opponent, opponent, || {
                self.initial(ctx.game(), role)
            })?
        } else {
            &mut state.global
        };
        estimate
            .choose_action(ctx.game(), role, rng)
            .map_err(unsupported)
    }

    fn learn_step(
        &self,
        state: &mut X::PolicyState,
        ctx: &LearnContext<'_, G, X>,
    ) -> Result<(), RuleError> {
        if ctx.scope() == DecisionScope::PerEncounter {
            return Ok(());
        }
        let state = state
            .slot_mut()
            .ok_or(RuleError::MissingSlot("EstimateState"))?;
        for outcome in ctx.outcomes() {
            Self::observe(&mut state.global, &outcome)?;
        }
        Ok(())
    }

    fn learn_encounter(
        &self,
        state: &mut X::PolicyState,
        outcome: &EncounterOutcome<'_, usize>,
        ctx: &LearnContext<'_, G, X>,
    ) -> Result<(), RuleError> {
        if ctx.scope() != DecisionScope::PerEncounter {
            return Ok(());
        }
        let (_, opponent) = outcome_opponent(outcome, "LogitChoice")?;
        let state = state
            .slot_mut()
            .ok_or(RuleError::MissingSlot("EstimateState"))?;
        let estimate = get_or_try_insert(&mut state.by_opponent, opponent, || {
            self.initial(ctx.game(), outcome.role)
        })?;
        Self::observe(estimate, outcome)
    }

    fn needs_encounter_learning(&self) -> bool {
        true
    }

    fn mixed(&self, state: &X::PolicyState, role: usize, game: &G) -> Option<Vec<f64>> {
        let state = <X::PolicyState as Slot<EstimateState>>::slot(state)?;
        if role >= game.n_roles() {
            return None;
        }
        let probabilities = state.global.selection_probabilities(game, role).ok()?;
        (probabilities.len() == game.n_actions(role)).then_some(probabilities)
    }
}

/// Roth--Erev reinforcement learning with recency and experimentation.
#[derive(Clone, Copy, Debug)]
pub struct RothErev {
    phi: f64,
    epsilon: f64,
    initial_propensity: f64,
}

impl RothErev {
    /// Stores Roth--Erev parameters for validation during state initialization.
    pub fn new(phi: f64, epsilon: f64, initial_propensity: f64) -> Self {
        Self {
            phi,
            epsilon,
            initial_propensity,
        }
    }

    fn initial<G: FiniteGame>(&self, game: &G, role: usize) -> Result<RothErevState, RuleError> {
        RothErevState::new(game, role, self.initial_propensity, self.phi, self.epsilon)
            .map_err(unsupported)
    }

    fn observe(
        propensity: &mut RothErevState,
        outcome: &EncounterOutcome<'_, usize>,
    ) -> Result<(), RuleError> {
        let own_action = outcome_action(outcome, outcome.role)?;
        let payoff = outcome.payoffs.get(outcome.role).copied().ok_or_else(|| {
            RuleError::Unsupported(format!(
                "encounter has no realized payoff for role {}",
                outcome.role
            ))
        })?;
        propensity.update(own_action, payoff).map_err(unsupported)
    }
}

impl<G, X> DecisionRule<G, X> for RothErev
where
    G: FiniteGame,
    X: ExperimentState,
    X::PolicyState: Slot<PropensityState>,
{
    fn name(&self) -> &str {
        "roth_erev"
    }

    fn init_state(&self, _ctx: &RuleInitContext, game: &G) -> Result<X::PolicyState, RuleError> {
        let global = self.initial(game, 0)?;
        for role in 1..game.n_roles() {
            self.initial(game, role)?;
        }
        Ok(<X::PolicyState as Slot<PropensityState>>::from_slot(
            PropensityState::new(global),
        ))
    }

    fn decide(
        &self,
        state: &mut X::PolicyState,
        ctx: &DecisionContext<'_, G, X>,
        rng: &mut SimRng,
    ) -> Result<usize, RuleError> {
        let role = assigned_role(ctx, "RothErev")?;
        let state = state
            .slot_mut()
            .ok_or(RuleError::MissingSlot("PropensityState"))?;
        let propensity = if ctx.scope() == DecisionScope::PerEncounter {
            let opponent = decision_opponent(ctx, "RothErev")?;
            get_or_try_insert(&mut state.by_opponent, opponent, || {
                self.initial(ctx.game(), role)
            })?
        } else {
            &mut state.global
        };
        propensity.choose_action(rng).map_err(unsupported)
    }

    fn learn_step(
        &self,
        state: &mut X::PolicyState,
        ctx: &LearnContext<'_, G, X>,
    ) -> Result<(), RuleError> {
        if ctx.scope() == DecisionScope::PerEncounter {
            return Ok(());
        }
        let state = state
            .slot_mut()
            .ok_or(RuleError::MissingSlot("PropensityState"))?;
        for outcome in ctx.outcomes() {
            Self::observe(&mut state.global, &outcome)?;
        }
        Ok(())
    }

    fn learn_encounter(
        &self,
        state: &mut X::PolicyState,
        outcome: &EncounterOutcome<'_, usize>,
        ctx: &LearnContext<'_, G, X>,
    ) -> Result<(), RuleError> {
        if ctx.scope() != DecisionScope::PerEncounter {
            return Ok(());
        }
        let (_, opponent) = outcome_opponent(outcome, "RothErev")?;
        let state = state
            .slot_mut()
            .ok_or(RuleError::MissingSlot("PropensityState"))?;
        let propensity = get_or_try_insert(&mut state.by_opponent, opponent, || {
            self.initial(ctx.game(), outcome.role)
        })?;
        Self::observe(propensity, outcome)
    }

    fn needs_encounter_learning(&self) -> bool {
        true
    }

    fn mixed(&self, state: &X::PolicyState, role: usize, game: &G) -> Option<Vec<f64>> {
        let state = <X::PolicyState as Slot<PropensityState>>::slot(state)?;
        if role >= game.n_roles() || state.global.propensities().len() != game.n_actions(role) {
            return None;
        }
        state.global.selection_probabilities().ok()
    }
}

/// A built-in repeated-game automaton with configurable cooperation action.
#[derive(Clone, Debug)]
pub struct Automaton {
    kind: AutomatonKind,
    coop: usize,
}

impl Automaton {
    /// Constructs an automaton rule.
    pub fn new(kind: AutomatonKind, coop: usize) -> Self {
        Self { kind, coop }
    }

    fn initial(&self) -> Result<AutomatonState, RuleError> {
        AutomatonState::new(self.kind.clone(), self.coop).map_err(unsupported)
    }

    fn observe(
        automaton: &mut AutomatonState,
        outcome: &EncounterOutcome<'_, usize>,
    ) -> Result<(), RuleError> {
        let (opponent_role, _) = outcome_opponent(outcome, "Automaton")?;
        let own_action = outcome_action(outcome, outcome.role)?;
        let opponent_action = outcome_action(outcome, opponent_role)?;
        automaton
            .observe(own_action, opponent_action)
            .map_err(unsupported)
    }
}

impl<G, X> DecisionRule<G, X> for Automaton
where
    G: FiniteGame,
    X: ExperimentState,
    X::PolicyState: Slot<AutomatonPolicyState>,
{
    fn name(&self) -> &str {
        "automaton"
    }

    fn init_state(&self, _ctx: &RuleInitContext, game: &G) -> Result<X::PolicyState, RuleError> {
        validate_two_action_game(game).map_err(unsupported)?;
        let global = self.initial()?;
        Ok(<X::PolicyState as Slot<AutomatonPolicyState>>::from_slot(
            AutomatonPolicyState::new(global),
        ))
    }

    fn decide(
        &self,
        state: &mut X::PolicyState,
        ctx: &DecisionContext<'_, G, X>,
        rng: &mut SimRng,
    ) -> Result<usize, RuleError> {
        assigned_role(ctx, "Automaton")?;
        let state = state
            .slot_mut()
            .ok_or(RuleError::MissingSlot("AutomatonPolicyState"))?;
        let automaton = if ctx.scope() == DecisionScope::PerEncounter {
            let opponent = decision_opponent(ctx, "Automaton")?;
            get_or_try_insert(&mut state.by_opponent, opponent, || self.initial())?
        } else {
            &mut state.global
        };
        automaton.next_action(rng).map_err(unsupported)
    }

    fn learn_step(
        &self,
        state: &mut X::PolicyState,
        ctx: &LearnContext<'_, G, X>,
    ) -> Result<(), RuleError> {
        if ctx.scope() == DecisionScope::PerEncounter {
            return Ok(());
        }
        let state = state
            .slot_mut()
            .ok_or(RuleError::MissingSlot("AutomatonPolicyState"))?;
        for outcome in ctx.outcomes() {
            Self::observe(&mut state.global, &outcome)?;
        }
        Ok(())
    }

    fn learn_encounter(
        &self,
        state: &mut X::PolicyState,
        outcome: &EncounterOutcome<'_, usize>,
        ctx: &LearnContext<'_, G, X>,
    ) -> Result<(), RuleError> {
        if ctx.scope() != DecisionScope::PerEncounter {
            return Ok(());
        }
        let (_, opponent) = outcome_opponent(outcome, "Automaton")?;
        let state = state
            .slot_mut()
            .ok_or(RuleError::MissingSlot("AutomatonPolicyState"))?;
        let automaton = get_or_try_insert(&mut state.by_opponent, opponent, || self.initial())?;
        Self::observe(automaton, outcome)
    }

    fn needs_encounter_learning(&self) -> bool {
        true
    }

    fn mixed(&self, state: &X::PolicyState, role: usize, game: &G) -> Option<Vec<f64>> {
        let state = <X::PolicyState as Slot<AutomatonPolicyState>>::slot(state)?;
        if role >= game.n_roles() || game.n_actions(role) != 2 {
            return None;
        }
        state.global.next_action_probabilities().ok()
    }
}
