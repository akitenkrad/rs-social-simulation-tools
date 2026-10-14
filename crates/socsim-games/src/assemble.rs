#[cfg(feature = "engine")]
use std::any::TypeId;
use std::collections::BTreeMap;

#[cfg(feature = "engine")]
use std::collections::BTreeSet;

use std::any::Any;
use std::fmt::Debug;

use rand::seq::SliceRandom;
use rand::Rng;
use socsim_core::{derive_seed, AgentId, SimClock, SimRng};

use crate::{
    ContinuousGame, DecisionScope, ExperimentState, GameError, GameWorld, InternalAggregateTable,
    InternalPairHistory, InternalPolicyReports, InternalStepHistory, KindCatalog, MatchingState,
    PlayArena, PolicyTable, RuleBook, RuleError, RuleInitContext, StageGame, Topology,
};

#[cfg(feature = "engine")]
use crate::{AggregateGame, MatchContext, Matching, OutcomeRule};

/// The held actions every agent starts with.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum InitialActions<A> {
    /// Every agent holds the same action.
    Same(A),
    /// Agent `i` holds the `i`-th action; the length must equal the number of
    /// agents.
    PerAgent(Vec<A>),
    /// Exact shares of the population, as `(action, share)` pairs.
    ///
    /// The number of agents holding each action is fixed by the largest
    /// remainder method: every action first receives `floor(n * share)`
    /// agents, and the agents still unassigned go one each to the actions
    /// with the largest fractional parts, ties going to the pair listed
    /// first. The actions are then laid out in the listed order and shuffled
    /// with the random stream `derive_seed(seed, &[0, 2])`, which is
    /// independent of the stream `[0]` used by [`Assignment::Shuffled`].
    ///
    /// Every share must be finite and non-negative, the shares must sum to one
    /// within `1e-9`, every action must be valid for every role, and an action
    /// may be listed only once. Two actions are the same when they compare
    /// equal (`usize` and `Vec<f64>` actions) or, for other action types,
    /// when their `Debug` representations coincide. A violation is reported
    /// as [`GameError::Config`] (or as [`GameError::Action`] for an invalid
    /// action).
    Shares(Vec<(A, f64)>),
}

/// Tolerance of the sum of initial-action shares around one.
const SHARE_SUM_TOLERANCE: f64 = 1e-9;

fn same_action<A: Debug + 'static>(a: &A, b: &A) -> bool {
    let (left, right) = (a as &dyn Any, b as &dyn Any);
    if let (Some(left), Some(right)) = (left.downcast_ref::<usize>(), right.downcast_ref::<usize>())
    {
        return left == right;
    }
    if let (Some(left), Some(right)) = (
        left.downcast_ref::<Vec<f64>>(),
        right.downcast_ref::<Vec<f64>>(),
    ) {
        return left == right;
    }
    format!("{a:?}") == format!("{b:?}")
}

/// Resolves [`InitialActions::Shares`] into one action per agent.
fn resolve_shares<G: StageGame>(
    game: &G,
    shares: Vec<(G::Action, f64)>,
    n_agents: usize,
    seed: u64,
) -> Result<Vec<G::Action>, GameError> {
    if shares.is_empty() {
        return Err(GameError::Config(
            "initial action shares: at least one action is required".to_owned(),
        ));
    }
    for (index, (_, share)) in shares.iter().enumerate() {
        if !share.is_finite() || *share < 0.0 {
            return Err(GameError::Config(format!(
                "initial action shares[{index}]: share {share} must be finite and non-negative"
            )));
        }
    }
    let total: f64 = shares.iter().map(|(_, share)| share).sum();
    if (total - 1.0).abs() > SHARE_SUM_TOLERANCE {
        return Err(GameError::Config(format!(
            "initial action shares: shares sum to {total}, expected 1"
        )));
    }
    for (action, _) in &shares {
        for role in 0..game.n_roles() {
            game.check_action(role, action)?;
        }
    }
    for (index, (action, _)) in shares.iter().enumerate() {
        if let Some(first) = shares[..index]
            .iter()
            .position(|(earlier, _)| same_action(earlier, action))
        {
            return Err(GameError::Config(format!(
                "initial action shares[{index}]: action {action:?} is already listed at index {first}"
            )));
        }
    }

    let quotas: Vec<f64> = shares
        .iter()
        .map(|(_, share)| n_agents as f64 * share)
        .collect();
    let mut counts: Vec<usize> = quotas.iter().map(|quota| quota.floor() as usize).collect();
    let floored: usize = counts.iter().sum();
    // The shares sum to 1 within 1e-9, so the floored counts can overshoot
    // `n_agents`, or leave more agents than entries to hand out, only when
    // `n_agents` is around 10^9 or larger. These checks keep that case an
    // error instead of a panic.
    let remaining = n_agents.checked_sub(floored).ok_or_else(|| {
        GameError::Config(format!(
            "initial action shares: shares assign {floored} agents, more than {n_agents}"
        ))
    })?;
    let mut order: Vec<usize> = (0..shares.len()).collect();
    // A stable sort keeps the listed order among equal fractional parts.
    order.sort_by(|&left, &right| {
        let left = quotas[left] - quotas[left].floor();
        let right = quotas[right] - quotas[right].floor();
        right.total_cmp(&left)
    });
    if remaining > order.len() {
        return Err(GameError::Config(format!(
            "initial action shares: {remaining} agents left after rounding down, more than {} actions",
            order.len()
        )));
    }
    for &index in order.iter().take(remaining) {
        counts[index] += 1;
    }

    let mut actions = Vec::with_capacity(n_agents);
    for ((action, _), count) in shares.into_iter().zip(counts) {
        actions.extend(std::iter::repeat_n(action, count));
    }
    let mut rng = SimRng::from_seed(derive_seed(seed, &[0, 2]));
    actions.shuffle(&mut rng);
    Ok(actions)
}

/// Draws independent uniform initial actions for a continuous game.
///
/// Every component of every agent's action is drawn uniformly from the closed
/// interval of the corresponding bound of role 0, in agent order and, within
/// an agent, in component order, from the random stream
/// `derive_seed(seed, &[0, 2])`. Pass the result to
/// [`InitialActions::PerAgent`]. The stream is the one used by
/// [`InitialActions::Shares`], and is independent of the stream `[0]` used by
/// [`Assignment::Shuffled`].
///
/// # Panics
///
/// Panics if the game has no role 0.
pub fn uniform_initial_actions<G: ContinuousGame>(
    game: &G,
    n_agents: usize,
    seed: u64,
) -> Vec<Vec<f64>> {
    let bounds = game.bounds(0);
    let mut rng = SimRng::from_seed(derive_seed(seed, &[0, 2]));
    (0..n_agents)
        .map(|_| {
            bounds
                .iter()
                .map(|&(lo, hi)| rng.gen_range(lo..=hi))
                .collect()
        })
        .collect()
}

#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum Assignment {
    Ordered(Vec<AssignGroup>),
    Shuffled(Vec<AssignGroup>),
}

#[derive(Clone, Debug)]
pub struct AssignGroup {
    pub kind: AssignKind,
    pub count: usize,
}

#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum AssignKind {
    Rule(String),
    Held,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum PayoffAggregation {
    #[default]
    Sum,
    Mean,
}

#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum OnInvalidAction<A> {
    Stop,
    Fallback(BTreeMap<String, A>),
}

// Kept manual by contract so this remains explicit about not requiring `A: Default`.
#[allow(clippy::derivable_impls)]
impl<A> Default for OnInvalidAction<A> {
    fn default() -> Self {
        Self::Stop
    }
}

#[derive(Clone, Debug)]
pub struct PlayOptions<A> {
    pub(crate) aggregation: PayoffAggregation,
    pub(crate) on_invalid_action: OnInvalidAction<A>,
    pub(crate) decision_scope: DecisionScope,
    pub(crate) report_mixed: bool,
    pub(crate) materialize: bool,
}

impl<A> Default for PlayOptions<A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A> PlayOptions<A> {
    pub fn new() -> Self {
        Self {
            aggregation: PayoffAggregation::Sum,
            on_invalid_action: OnInvalidAction::Stop,
            decision_scope: DecisionScope::PerStep,
            report_mixed: false,
            materialize: false,
        }
    }

    pub fn payoff_aggregation(mut self, aggregation: PayoffAggregation) -> Self {
        self.aggregation = aggregation;
        self
    }

    pub fn on_invalid_action(mut self, policy: OnInvalidAction<A>) -> Self {
        self.on_invalid_action = policy;
        self
    }

    pub fn decision_scope(mut self, scope: DecisionScope) -> Self {
        self.decision_scope = scope;
        self
    }

    /// Enables or disables post-learning mixed-strategy reports.
    pub fn report_mixed(mut self, on: bool) -> Self {
        self.report_mixed = on;
        self
    }

    /// Forces construction of the complete encounter arena.
    ///
    /// The default is `false`. With `false`, the play step is evaluated by
    /// traversing the topology directly, without filling
    /// [`GameWorld::plays`](crate::GameWorld::plays), when every one of these
    /// conditions holds; otherwise the materialized path is used:
    ///
    /// - every agent is held (no agent is assigned an active rule);
    /// - the decision scope is [`DecisionScope::PerStep`];
    /// - the outcome rule is [`NormalFormOutcome`](crate::NormalFormOutcome)
    ///   (or the play path is [`Counting`]), not a user-defined
    ///   [`OutcomeRule`];
    /// - [`Matching::implicit`] returns `Some`.
    ///
    /// Both paths produce the same payoffs, play counts, step records, and
    /// metric columns. Setting this to `true` always uses the materialized
    /// path. A grid neighborhood with self-play contains about nine
    /// encounters per agent, so the materialized path can use substantially
    /// more memory than the implicit path.
    pub fn materialize(mut self, materialize: bool) -> Self {
        self.materialize = materialize;
        self
    }
}

/// Play path that evaluates every encounter with an [`OutcomeRule`].
///
/// This is the play path used by [`assemble`].
#[cfg(feature = "engine")]
#[derive(Clone, Debug)]
pub struct Pairwise<O> {
    outcome: O,
}

#[cfg(feature = "engine")]
impl<O> Pairwise<O> {
    /// Wraps an outcome rule as a play path.
    pub fn new(outcome: O) -> Self {
        Self { outcome }
    }

    /// Records public aggregates, keeping `history_len` rows for decisions.
    pub fn with_aggregates(self, history_len: usize) -> WithAggregates<Self> {
        WithAggregates {
            inner: self,
            history_len,
        }
    }
}

/// Play path for anonymous games that computes payoffs from action counts.
///
/// Within each encounter, the payoff of each distinct action is computed once
/// from the counts of the other seats. It yields the same payoffs as
/// [`Pairwise`] with [`NormalFormOutcome`](crate::NormalFormOutcome) and
/// cannot be combined with a user-defined outcome rule. Construct it with
/// [`counting_play`].
#[cfg(feature = "engine")]
#[derive(Clone, Copy, Debug, Default)]
pub struct Counting;

#[cfg(feature = "engine")]
impl Counting {
    /// Records public aggregates, keeping `history_len` rows for decisions.
    pub fn with_aggregates(self, history_len: usize) -> WithAggregates<Self> {
        WithAggregates {
            inner: self,
            history_len,
        }
    }
}

/// Returns the [`Counting`] play path.
#[cfg(feature = "engine")]
pub fn counting_play() -> Counting {
    Counting
}

/// Play path that also records the game's public aggregate values.
///
/// Aggregates are reduced into one row per step, on both the materialized
/// and the implicit path. Construct it with `with_aggregates` on
/// [`Pairwise`] or [`Counting`].
#[cfg(feature = "engine")]
#[derive(Clone, Debug)]
pub struct WithAggregates<P> {
    inner: P,
    history_len: usize,
}

/// Population dynamics that leave held actions unchanged.
#[cfg(feature = "engine")]
#[derive(Clone, Copy, Debug, Default)]
pub struct NoDynamics;

#[cfg(feature = "engine")]
#[allow(private_interfaces)]
pub(crate) mod sealed {
    use super::*;
    use crate::mechanism::PayoffRangeSource;

    pub trait PlayBuild<G: StageGame, X: ExperimentState>: Send + Sync + Sized + 'static {
        fn permits_implicit(&self) -> bool;
        fn into_local_runner(self) -> Option<Box<dyn crate::mechanism::KernelRunner<G, X>>> {
            None
        }
        fn prepare(
            &self,
            _world: &mut GameWorld<G, X>,
        ) -> Result<Option<crate::mechanism::AggregateCollector<G>>, GameError> {
            Ok(None)
        }
        fn into_runner(self) -> Box<dyn crate::mechanism::KernelRunner<G, X>>;
        fn payoff_source(&self) -> PayoffRangeSource<G>;
    }

    impl<G, X, O> PlayBuild<G, X> for Pairwise<O>
    where
        G: StageGame,
        X: ExperimentState,
        O: OutcomeRule<G, X> + 'static,
    {
        fn permits_implicit(&self) -> bool {
            TypeId::of::<O>() == TypeId::of::<crate::NormalFormOutcome>()
        }

        fn into_runner(self) -> Box<dyn crate::mechanism::KernelRunner<G, X>> {
            crate::mechanism::pairwise_runner(self.outcome)
        }

        fn into_local_runner(self) -> Option<Box<dyn crate::mechanism::KernelRunner<G, X>>> {
            (TypeId::of::<O>() == TypeId::of::<crate::NormalFormOutcome>())
                .then(|| crate::mechanism::pairwise_runner(self.outcome))
        }

        fn payoff_source(&self) -> PayoffRangeSource<G> {
            if TypeId::of::<O>() == TypeId::of::<crate::NormalFormOutcome>() {
                PayoffRangeSource::Game
            } else {
                PayoffRangeSource::Unknown
            }
        }
    }

    impl<G, X> PlayBuild<G, X> for Counting
    where
        G: crate::AnonymousGame,
        X: ExperimentState,
    {
        fn permits_implicit(&self) -> bool {
            true
        }

        fn into_runner(self) -> Box<dyn crate::mechanism::KernelRunner<G, X>> {
            crate::mechanism::counting_runner()
        }

        fn into_local_runner(self) -> Option<Box<dyn crate::mechanism::KernelRunner<G, X>>> {
            Some(crate::mechanism::counting_runner())
        }

        fn payoff_source(&self) -> PayoffRangeSource<G> {
            PayoffRangeSource::Counts(crate::mechanism::anonymous_payoff_range::<G>)
        }
    }

    impl<G, X, P> PlayBuild<G, X> for WithAggregates<P>
    where
        G: AggregateGame,
        X: ExperimentState,
        P: PlayBuild<G, X>,
    {
        fn permits_implicit(&self) -> bool {
            self.inner.permits_implicit()
        }

        fn into_local_runner(self) -> Option<Box<dyn crate::mechanism::KernelRunner<G, X>>> {
            self.inner.into_local_runner()
        }

        fn prepare(
            &self,
            world: &mut GameWorld<G, X>,
        ) -> Result<Option<crate::mechanism::AggregateCollector<G>>, GameError> {
            if self.history_len == 0 {
                return Err(GameError::InvalidAggregateHistoryLength);
            }
            let names = world
                .game
                .aggregate_spec()
                .iter()
                .map(|spec| spec.name().to_owned())
                .collect();
            let reductions = world
                .game
                .aggregate_spec()
                .iter()
                .map(|spec| spec.reduce())
                .collect();
            world.aggregate = Some(InternalAggregateTable::new(names, self.history_len));
            Ok(Some(crate::mechanism::AggregateCollector::new(
                reductions,
                aggregate_into::<G>,
            )))
        }

        fn into_runner(self) -> Box<dyn crate::mechanism::KernelRunner<G, X>> {
            self.inner.into_runner()
        }

        fn payoff_source(&self) -> PayoffRangeSource<G> {
            self.inner.payoff_source()
        }
    }

    /// Values available to population dynamics while the simulation is
    /// assembled.
    pub struct DynamicsCheck<'a, G: StageGame, X: ExperimentState> {
        pub(crate) world: &'a GameWorld<G, X>,
        pub(crate) encounter_bounds: Option<crate::EncounterBounds>,
        pub(crate) aggregation: PayoffAggregation,
        pub(crate) payoff_source: PayoffRangeSource<G>,
    }

    pub trait DynamicsBuild<G: StageGame, X: ExperimentState>:
        Send + Sync + Sized + 'static
    {
        fn prepare(&self, _check: &DynamicsCheck<'_, G, X>) -> Result<(), GameError> {
            Ok(())
        }

        fn into_mechanism(self) -> Option<Box<dyn socsim_core::Mechanism<GameWorld<G, X>>>>;
    }

    impl<G: StageGame, X: ExperimentState> DynamicsBuild<G, X> for NoDynamics {
        fn into_mechanism(self) -> Option<Box<dyn socsim_core::Mechanism<GameWorld<G, X>>>> {
            None
        }
    }
}

/// How the play step turns encounters into payoffs.
///
/// Sealed: implemented only by [`Pairwise`], [`Counting`], and
/// [`WithAggregates`]. Pass one to [`assemble_with`].
#[cfg(feature = "engine")]
pub trait PlayPath<G: StageGame, X: ExperimentState>: sealed::PlayBuild<G, X> {}

#[cfg(feature = "engine")]
impl<G, X, P> PlayPath<G, X> for P
where
    G: StageGame,
    X: ExperimentState,
    P: sealed::PlayBuild<G, X>,
{
}

/// A population dynamic that rewrites held actions from payoffs.
///
/// Sealed: implemented only by [`NoDynamics`],
/// [`ImitateBest`](crate::ImitateBest), [`FermiUpdate`](crate::FermiUpdate),
/// [`ProportionalImitation`](crate::ProportionalImitation), and
/// [`MoranUpdate`](crate::MoranUpdate). Pass one to [`assemble_with`]; its
/// parameters are checked against the world and the matching at assembly.
#[cfg(feature = "engine")]
pub trait PopulationDynamics<G: StageGame, X: ExperimentState>:
    sealed::DynamicsBuild<G, X>
{
}

#[cfg(feature = "engine")]
impl<G, X, D> PopulationDynamics<G, X> for D
where
    G: StageGame,
    X: ExperimentState,
    D: sealed::DynamicsBuild<G, X>,
{
}

/// Builds a game world with decision-rule kinds taken from `rules`.
///
/// Every initial held action must pass [`StageGame::check_action`] for every
/// role, including actions held by agents assigned a decision rule. Therefore,
/// this function cannot build a world for a continuous game whose action
/// dimensions differ by role.
#[allow(clippy::too_many_arguments)]
pub fn build_world<G, X>(
    game: G,
    n_agents: usize,
    x: X,
    initial_held: InitialActions<G::Action>,
    rules: &RuleBook<G, X>,
    assign: &Assignment,
    history_len: usize,
    t_max: u64,
    seed: u64,
) -> Result<GameWorld<G, X>, GameError>
where
    G: StageGame,
    X: ExperimentState,
{
    build_world_with(
        game,
        n_agents,
        x,
        initial_held,
        &KindCatalog::from_rules(rules),
        assign,
        history_len,
        t_max,
        seed,
    )
}

/// Builds a game world with decision-rule kinds taken from `catalog`.
///
/// Every initial held action must pass [`StageGame::check_action`] for every
/// role, including actions held by agents assigned a decision rule. Therefore,
/// this function cannot build a world for a continuous game whose action
/// dimensions differ by role.
#[allow(clippy::too_many_arguments)]
pub fn build_world_with<G, X>(
    game: G,
    n_agents: usize,
    x: X,
    initial_held: InitialActions<G::Action>,
    catalog: &KindCatalog<G, X>,
    assign: &Assignment,
    history_len: usize,
    t_max: u64,
    seed: u64,
) -> Result<GameWorld<G, X>, GameError>
where
    G: StageGame,
    X: ExperimentState,
{
    if history_len == 0 {
        return Err(GameError::InvalidHistoryLength);
    }

    let held = match initial_held {
        InitialActions::Same(action) => vec![action; n_agents],
        InitialActions::PerAgent(actions) => {
            if actions.len() != n_agents {
                return Err(GameError::AgentCount {
                    expected: n_agents,
                    actual: actions.len(),
                });
            }
            actions
        }
        InitialActions::Shares(shares) => resolve_shares(&game, shares, n_agents, seed)?,
    };

    for action in &held {
        for role in 0..game.n_roles() {
            game.check_action(role, action)?;
        }
    }

    let (groups, shuffled) = match assign {
        Assignment::Ordered(groups) => (groups, false),
        Assignment::Shuffled(groups) => (groups, true),
    };
    let assigned_count = groups.iter().map(|group| group.count).sum::<usize>();
    if assigned_count != n_agents {
        return Err(GameError::AgentCount {
            expected: n_agents,
            actual: assigned_count,
        });
    }
    let mut assignments = Vec::with_capacity(n_agents);
    for group in groups {
        assignments.extend((0..group.count).map(|_| group.kind.clone()));
    }
    if shuffled {
        let mut rng = SimRng::from_seed(derive_seed(seed, &[0]));
        assignments.shuffle(&mut rng);
    }

    let needs_pair_history = assignments.iter().try_fold(false, |needed, assignment| {
        if needed {
            return Ok(true);
        }
        match assignment {
            AssignKind::Rule(kind) => catalog.needs_pair_history(kind),
            AssignKind::Held => Ok(false),
        }
    })?;

    let ids: Vec<_> = (0..n_agents as u64).map(AgentId).collect();
    let kinds = catalog.kinds().map(str::to_owned).collect();
    let mut slot = Vec::with_capacity(n_agents);
    let mut kind_of = Vec::new();
    let mut states = Vec::new();
    for (index, assignment) in assignments.into_iter().enumerate() {
        match assignment {
            AssignKind::Held => slot.push(PolicyTable::<X::PolicyState>::HELD),
            AssignKind::Rule(kind) => {
                let kind_id = catalog
                    .kind_id(&kind)
                    .ok_or_else(|| RuleError::UnknownKind(kind.clone()))?;
                let state_index =
                    u32::try_from(states.len()).map_err(|_| GameError::AgentCount {
                        expected: u32::MAX as usize,
                        actual: n_agents,
                    })?;
                let agent = AgentId(index as u64);
                let generation = 0;
                let context = RuleInitContext {
                    agent,
                    kind: kind_id,
                    generation,
                    init_seed: derive_seed(seed, &[3, agent.0, u64::from(generation)]),
                };
                states.push(catalog.init_state(&kind, &context, &game)?);
                kind_of.push(kind_id);
                slot.push(state_index);
            }
        }
    }

    Ok(GameWorld {
        clock: SimClock::new(t_max),
        ids,
        topology: Topology::None,
        held,
        policies: PolicyTable::new(kinds, slot, kind_of, states),
        matching_state: MatchingState::None,
        plays: PlayArena::new(game.n_roles()),
        payoff: vec![0.0; n_agents],
        n_played: vec![0; n_agents],
        played_counts: None,
        step_history: InternalStepHistory::new(n_agents, history_len),
        pair_history: needs_pair_history.then(|| InternalPairHistory::new(history_len)),
        aggregate: None,
        reports: None,
        metrics_state: None,
        game,
        x,
        decision_root: derive_seed(seed, &[2]),
    })
}

#[cfg(feature = "engine")]
pub fn assemble<G, X, M, O>(
    world: GameWorld<G, X>,
    matching: M,
    rules: RuleBook<G, X>,
    outcome: O,
    opts: PlayOptions<G::Action>,
    seed: u64,
) -> Result<socsim_engine::SimulationBuilder<GameWorld<G, X>>, GameError>
where
    G: StageGame,
    X: ExperimentState,
    M: Matching<G, X> + 'static,
    O: OutcomeRule<G, X> + 'static,
{
    assemble_with(
        world,
        matching,
        rules,
        Pairwise::new(outcome),
        NoDynamics,
        opts,
        seed,
    )
}

#[cfg(feature = "engine")]
#[allow(clippy::too_many_arguments)]
/// Assembles a game simulation that retains public aggregate values.
///
/// Aggregate values are computed once per encounter and reduced into one row
/// per completed step. `aggregate_history_len` is the maximum number of rows
/// exposed through [`crate::DecisionContext::aggregate_history`].
pub fn assemble_with_aggregates<G, X, M, O>(
    world: GameWorld<G, X>,
    matching: M,
    rules: RuleBook<G, X>,
    outcome: O,
    opts: PlayOptions<G::Action>,
    seed: u64,
    aggregate_history_len: usize,
) -> Result<socsim_engine::SimulationBuilder<GameWorld<G, X>>, GameError>
where
    G: AggregateGame,
    X: ExperimentState,
    M: Matching<G, X> + 'static,
    O: OutcomeRule<G, X> + 'static,
{
    assemble_with(
        world,
        matching,
        rules,
        Pairwise::new(outcome).with_aggregates(aggregate_history_len),
        NoDynamics,
        opts,
        seed,
    )
}

#[cfg(feature = "engine")]
fn aggregate_into<G: AggregateGame>(game: &G, profile: &[G::Action], out: &mut [f64]) {
    game.aggregate_into(profile, out);
}

/// Assembles a game simulation with an explicit play path and population dynamic.
///
/// [`assemble`] and [`assemble_with_aggregates`] are shorthands for this with
/// [`Pairwise`] and [`NoDynamics`]. Assembly validates the matching (role count,
/// topology), the rule kinds, fallback actions, and the dynamic's parameters,
/// which may depend on the matching's
/// [`encounter_bounds`](crate::Matching::encounter_bounds) and on
/// [`PlayOptions::payoff_aggregation`]. Each step matches, decides, and plays;
/// the population dynamic then rewrites held actions from that step's payoffs.
#[cfg(feature = "engine")]
#[allow(clippy::too_many_arguments)]
pub fn assemble_with<G, X, M, P, D>(
    world: GameWorld<G, X>,
    matching: M,
    rules: RuleBook<G, X>,
    play: P,
    dynamics: D,
    opts: PlayOptions<G::Action>,
    seed: u64,
) -> Result<socsim_engine::SimulationBuilder<GameWorld<G, X>>, GameError>
where
    G: StageGame,
    X: ExperimentState,
    M: Matching<G, X> + 'static,
    P: PlayPath<G, X>,
    D: PopulationDynamics<G, X>,
{
    assemble_internal(world, matching, rules, play, dynamics, opts, seed)
}

#[cfg(feature = "engine")]
#[allow(clippy::too_many_arguments)]
fn assemble_internal<G, X, M, P, D>(
    world: GameWorld<G, X>,
    matching: M,
    rules: RuleBook<G, X>,
    play: P,
    dynamics: D,
    opts: PlayOptions<G::Action>,
    seed: u64,
) -> Result<socsim_engine::SimulationBuilder<GameWorld<G, X>>, GameError>
where
    G: StageGame,
    X: ExperimentState,
    M: Matching<G, X> + 'static,
    P: PlayPath<G, X>,
    D: PopulationDynamics<G, X>,
{
    let parts = assemble_parts(world, matching, rules, play, dynamics, opts, seed)?;
    let builder = parts
        .mechanisms
        .into_iter()
        .fold(
            socsim_engine::SimulationBuilder::new(parts.world),
            |builder, mechanism| builder.add_mechanism(mechanism),
        )
        .seed(parts.engine_seed);
    Ok(builder)
}

/// The parts of an assembled game simulation, before they are handed to a
/// [`socsim_engine::SimulationBuilder`].
#[cfg(feature = "engine")]
pub(crate) struct AssembledParts<G: StageGame, X: ExperimentState> {
    /// The world, with its matching state, reports, and aggregate table set.
    pub(crate) world: GameWorld<G, X>,
    /// The mechanisms in registration order: `game_match`, `game_decide`, the
    /// population dynamic (if any), then `game_play`.
    pub(crate) mechanisms: Vec<Box<dyn socsim_core::Mechanism<GameWorld<G, X>>>>,
    /// The engine seed, `derive_seed(seed, &[1])`.
    pub(crate) engine_seed: u64,
}

/// Validates the configuration and builds the parts of a game simulation.
///
/// This is the single assembly procedure behind [`assemble_with`] and its
/// shorthands; the public functions only wrap the parts in a builder.
#[cfg(feature = "engine")]
#[allow(clippy::too_many_arguments)]
pub(crate) fn assemble_parts<G, X, M, P, D>(
    mut world: GameWorld<G, X>,
    matching: M,
    rules: RuleBook<G, X>,
    play: P,
    dynamics: D,
    opts: PlayOptions<G::Action>,
    seed: u64,
) -> Result<AssembledParts<G, X>, GameError>
where
    G: StageGame,
    X: ExperimentState,
    M: Matching<G, X> + 'static,
    P: PlayPath<G, X>,
    D: PopulationDynamics<G, X>,
{
    if !matching.supports_roles(world.game.n_roles()) {
        return Err(GameError::UnsupportedRoles {
            n_roles: world.game.n_roles(),
        });
    }
    if opts.report_mixed && opts.decision_scope == DecisionScope::PerEncounter {
        return Err(GameError::Metrics(
            "mixed-strategy reporting does not support per-encounter decisions".to_owned(),
        ));
    }
    world.reports = if opts.report_mixed {
        Some(
            InternalPolicyReports::new(world.clock.t(), world.ids.len(), world.game.n_roles())
                .ok_or_else(|| {
                    GameError::Metrics("mixed-strategy report table is too large".to_owned())
                })?,
        )
    } else {
        None
    };

    let expected: Vec<_> = KindCatalog::from_rules(&rules)
        .kinds()
        .map(str::to_owned)
        .collect();
    let actual = world.policies.kinds.clone();
    if actual != expected {
        return Err(GameError::InvalidKinds { expected, actual });
    }

    let mut active_kinds = BTreeSet::new();
    for &state_slot in &world.policies.slot {
        if state_slot == PolicyTable::<X::PolicyState>::HELD {
            continue;
        }
        let state_index = state_slot as usize;
        let kind_id = world.policies.kind_of.get(state_index).ok_or_else(|| {
            GameError::Rule(crate::RuleError::UnknownKind(state_index.to_string()))
        })?;
        let kind = world
            .policies
            .kinds
            .get(kind_id.0 as usize)
            .ok_or_else(|| GameError::Rule(crate::RuleError::UnknownKind(kind_id.0.to_string())))?;
        rules.get(kind)?;
        active_kinds.insert(kind.as_str());
    }

    if let OnInvalidAction::Fallback(fallbacks) = &opts.on_invalid_action {
        for &kind in &active_kinds {
            if !fallbacks.contains_key(kind) {
                return Err(crate::RuleError::Unsupported(format!(
                    "missing fallback action for rule kind {kind}"
                ))
                .into());
            }
        }
        for (kind, action) in fallbacks {
            rules.get(kind)?;
            for role in 0..world.game.n_roles() {
                world.game.check_action(role, action)?;
            }
        }
    }
    let allows_multi_role_per_step = matching.allows_multi_role_per_step();
    let context = MatchContext {
        t: world.clock.t(),
        decision_scope: opts.decision_scope,
        ids: &world.ids,
        game: &world.game,
        topology: &world.topology,
        held: &world.held,
        history: &world.step_history,
        x: &world.x,
    };
    matching.check(&context)?;
    let encounter_bounds = matching.encounter_bounds(&context);
    world.matching_state = matching.init_state(&context);
    sealed::DynamicsBuild::prepare(
        &dynamics,
        &sealed::DynamicsCheck {
            world: &world,
            encounter_bounds,
            aggregation: opts.aggregation,
            payoff_source: sealed::PlayBuild::payoff_source(&play),
        },
    )?;
    let implicit = (!opts.materialize
        && opts.decision_scope == DecisionScope::PerStep
        && world
            .policies
            .slot
            .iter()
            .all(|&slot| slot == PolicyTable::<X::PolicyState>::HELD)
        && sealed::PlayBuild::permits_implicit(&play))
    .then(|| matching.implicit())
    .flatten();
    let aggregate = sealed::PlayBuild::prepare(&play, &mut world)?;
    let runner = sealed::PlayBuild::into_runner(play);
    let play = Box::new(crate::mechanism::PlayMechanism::<G, X>::new(
        runner,
        opts.aggregation,
        opts.decision_scope,
        implicit,
        aggregate,
    ));
    let mut mechanisms: Vec<Box<dyn socsim_core::Mechanism<GameWorld<G, X>>>> = vec![
        Box::new(crate::mechanism::MatchMechanism::<G, X, M>::new(
            matching,
            opts.decision_scope,
            implicit.is_some(),
        )),
        Box::new(crate::RuleDriver::new(
            rules,
            opts.on_invalid_action,
            opts.decision_scope,
            opts.report_mixed,
            allows_multi_role_per_step,
        )),
    ];
    // Population dynamics run in `Reward` after `game_decide`, so learning
    // always precedes imitation and selection.
    if let Some(mechanism) = sealed::DynamicsBuild::into_mechanism(dynamics) {
        mechanisms.push(mechanism);
    }
    mechanisms.push(play);
    Ok(AssembledParts {
        world,
        mechanisms,
        engine_seed: derive_seed(seed, &[1]),
    })
}
