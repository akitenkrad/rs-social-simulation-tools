//! Synchronous population dynamics for held agents.
//!
//! Every dynamic in this module runs in [`Phase::Reward`] after the play step
//! has written each agent's aggregated payoff. Payoffs are never recomputed:
//! the dynamics read [`GameWorld::payoffs`] as produced by the step's
//! interaction, so `PostStep` observers see payoffs and play records from the
//! actions used before the update and `held` actions after it.

use std::cmp::Ordering;
use std::marker::PhantomData;

use rand::Rng;
use socsim_core::{Mechanism, Phase, SimRng, SocsimError, StepContext};

use crate::{
    ExperimentState, FiniteGame, GameError, GameWorld, NormalFormGame, PolicyTable, StageGame,
    Topology,
};

/// Update schedule of a population dynamic.
///
/// The schedule has no default value so that adding further schedules does not
/// change the meaning of an existing configuration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Update {
    /// Every agent's next action is computed from the same pre-update state
    /// and all actions are written together at the end of the step.
    Synchronous,
}

/// Tie-breaking policy of [`ImitateBest`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum ImitationTie {
    /// Keep the agent's own action when its payoff is among the highest;
    /// otherwise adopt the lowest action index among the best neighbors.
    #[default]
    KeepOwn,
    /// Adopt the lowest action index among the agent and its neighbors that
    /// share the highest payoff.
    LowestAction,
}

/// Imitate-the-best dynamic.
///
/// Each agent adopts the action of the highest-paid agent among itself and
/// its topology neighbors. Ties are broken by action index, never by neighbor
/// position, so the rule commutes with lattice symmetries. It consumes no
/// randomness and requires a grid or network topology.
#[derive(Clone, Copy, Debug)]
pub struct ImitateBest {
    update: Update,
    tie_break: ImitationTie,
}

impl ImitateBest {
    /// Constructs the dynamic with [`ImitationTie::KeepOwn`].
    pub fn new(update: Update) -> Self {
        Self {
            update,
            tie_break: ImitationTie::KeepOwn,
        }
    }

    /// Selects the tie-breaking policy.
    pub fn tie_break(mut self, tie_break: ImitationTie) -> Self {
        self.tie_break = tie_break;
        self
    }

    /// Returns the update schedule.
    pub fn update(&self) -> Update {
        self.update
    }
}

/// Pairwise-comparison dynamic with the Fermi function.
///
/// Agent `i` draws one comparison partner `j` and adopts `j`'s action with
/// probability `1 / (1 + exp(-(pi_j - pi_i) / K))`. The probability is
/// evaluated as `1 / (1 + exp(-z))` for `z >= 0` and `exp(z) / (1 + exp(z))`
/// for `z < 0`, where `z = (pi_j - pi_i) / K`, so the exponential never
/// overflows.
#[derive(Clone, Copy, Debug)]
pub struct FermiUpdate {
    k: f64,
    update: Update,
}

impl FermiUpdate {
    /// Constructs the dynamic with noise `k`, which must be finite and positive.
    pub fn new(k: f64, update: Update) -> Result<Self, GameError> {
        if !(k.is_finite() && k > 0.0) {
            return Err(GameError::Population(format!(
                "fermi_update requires a finite K greater than zero, got {k}"
            )));
        }
        Ok(Self { k, update })
    }

    /// Returns the noise parameter `K`.
    pub fn k(&self) -> f64 {
        self.k
    }

    /// Returns the update schedule.
    pub fn update(&self) -> Update {
        self.update
    }
}

/// Proportional imitation dynamic.
///
/// Agent `i` draws one comparison partner `j` and adopts `j`'s action with
/// probability `beta * max(pi_j - pi_i, 0)`. Assembly rejects `beta` above
/// `1 / range`, where `range` is the width of the attainable per-step payoff
/// interval, so the probability never exceeds one.
#[derive(Clone, Copy, Debug)]
pub struct ProportionalImitation {
    beta: f64,
    update: Update,
}

impl ProportionalImitation {
    /// Constructs the dynamic with rate `beta`, which must be finite and
    /// positive. The upper bound is checked during assembly.
    pub fn new(beta: f64, update: Update) -> Result<Self, GameError> {
        if !(beta.is_finite() && beta > 0.0) {
            return Err(GameError::Population(format!(
                "replicator_update requires a finite beta greater than zero, got {beta}"
            )));
        }
        Ok(Self { beta, update })
    }

    /// Returns the imitation rate.
    pub fn beta(&self) -> f64 {
        self.beta
    }

    /// Returns the update schedule.
    pub fn update(&self) -> Update {
        self.update
    }
}

/// Map from an agent's step payoff to its reproductive fitness.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub enum Fitness {
    /// `1 - w + w * payoff`, with `w` in `[0, 1]`. Assembly checks that the
    /// lowest attainable payoff gives a positive fitness.
    Linear {
        /// Selection strength, in `[0, 1]`; `0` makes fitness constant.
        w: f64,
    },
    /// `exp(w * payoff)`, with finite `w`.
    Exponential {
        /// Selection strength; any finite value.
        w: f64,
    },
}

impl Fitness {
    fn value(self, payoff: f64) -> f64 {
        match self {
            Self::Linear { w } => 1.0 - w + w * payoff,
            Self::Exponential { w } => (w * payoff).exp(),
        }
    }
}

/// Moran birth-death process.
///
/// Each step performs exactly one birth-death event: a parent is drawn from
/// all agents with probability proportional to fitness, the agent that dies
/// is drawn uniformly from all agents (the parent included), and the dying
/// agent's held action becomes the parent's action, or with probability
/// `mutation` a uniformly drawn action. It requires a world without topology.
#[derive(Clone, Copy, Debug)]
pub struct MoranUpdate {
    fitness: Fitness,
    mutation: f64,
    update: Update,
}

impl MoranUpdate {
    /// Constructs the process. `mutation` must lie in `[0, 1]`.
    pub fn new(fitness: Fitness, mutation: f64, update: Update) -> Result<Self, GameError> {
        match fitness {
            Fitness::Linear { w } => {
                if !(w.is_finite() && (0.0..=1.0).contains(&w)) {
                    return Err(GameError::Population(format!(
                        "linear fitness requires w in [0, 1], got {w}"
                    )));
                }
            }
            Fitness::Exponential { w } => {
                if !w.is_finite() {
                    return Err(GameError::Population(format!(
                        "exponential fitness requires a finite w, got {w}"
                    )));
                }
            }
        }
        if !(mutation.is_finite() && (0.0..=1.0).contains(&mutation)) {
            return Err(GameError::Population(format!(
                "moran_update requires a mutation probability in [0, 1], got {mutation}"
            )));
        }
        Ok(Self {
            fitness,
            mutation,
            update,
        })
    }

    /// Returns the fitness map.
    pub fn fitness(&self) -> Fitness {
        self.fitness
    }

    /// Returns the mutation probability.
    pub fn mutation(&self) -> f64 {
        self.mutation
    }

    /// Returns the update schedule.
    pub fn update(&self) -> Update {
        self.update
    }
}

#[allow(private_interfaces)]
pub(crate) mod sealed {
    use super::*;

    /// Action-type dispatch for dynamics that copy actions between agents.
    ///
    /// Implemented only inside this crate. Finite games use `usize`.
    pub trait ActionSpace<G: StageGame<Action = Self>>: Sized + Clone {
        /// Checks that every role shares the same action space.
        fn check_common_space(game: &G) -> Result<(), GameError>;

        /// Draws an action uniformly from the common action space.
        fn uniform(game: &G, rng: &mut SimRng) -> Self;

        /// Returns a mutant of `parent`.
        fn mutate(game: &G, parent: &Self, rng: &mut SimRng) -> Self;

        /// Returns the payoff range over every role and action profile.
        fn payoff_range(game: &G) -> Result<(f64, f64), GameError>;

        /// Orders actions for deterministic tie-breaking.
        fn tie_cmp(left: &Self, right: &Self) -> Ordering;
    }

    impl<G: FiniteGame> ActionSpace<G> for usize {
        fn check_common_space(game: &G) -> Result<(), GameError> {
            crate::game::check_common_action_space(game)
        }

        fn uniform(game: &G, rng: &mut SimRng) -> Self {
            rng.gen_range(0..game.n_actions(0))
        }

        fn mutate(game: &G, _parent: &Self, rng: &mut SimRng) -> Self {
            Self::uniform(game, rng)
        }

        fn payoff_range(game: &G) -> Result<(f64, f64), GameError> {
            finite_payoff_range(game)
        }

        fn tie_cmp(left: &Self, right: &Self) -> Ordering {
            left.cmp(right)
        }
    }
}

pub(crate) use sealed::ActionSpace;

/// Source of the single-encounter payoff range used by assembly checks.
pub(crate) enum PayoffRangeSource<G> {
    /// Payoffs come from the game's normal form.
    Game,
    /// Payoffs come from an anonymous game's count-based payoff.
    Counts(fn(&G) -> Result<(f64, f64), GameError>),
    /// Payoffs come from a user-defined outcome rule.
    Unknown,
}

pub(crate) fn finite_payoff_range<G: FiniteGame>(game: &G) -> Result<(f64, f64), GameError> {
    let n_roles = game.n_roles();
    if n_roles == 0 {
        return Err(GameError::Population(
            "the game has no roles to compute a payoff range".to_owned(),
        ));
    }
    let mut counts = Vec::with_capacity(n_roles);
    let mut profiles = 1_u128;
    for role in 0..n_roles {
        let count = game.n_actions(role);
        if count == 0 {
            return Err(GameError::Population(format!("role {role} has no actions")));
        }
        profiles = profiles.saturating_mul(count as u128);
        counts.push(count);
    }
    if profiles > crate::game::MAX_PROFILES {
        return Err(GameError::Population(format!(
            "the payoff range requires enumerating {profiles} action profiles, which exceeds the limit"
        )));
    }
    let mut minimum = f64::INFINITY;
    let mut maximum = f64::NEG_INFINITY;
    let mut profile = vec![0; n_roles];
    loop {
        for role in 0..n_roles {
            let payoff = game.payoff(role, &profile);
            if !payoff.is_finite() {
                return Err(GameError::Population(
                    "the game has a non-finite payoff".to_owned(),
                ));
            }
            minimum = minimum.min(payoff);
            maximum = maximum.max(payoff);
        }
        if !crate::solve::advance_profile(&mut profile, &counts) {
            break;
        }
    }
    Ok((minimum, maximum))
}

#[cfg_attr(not(feature = "engine"), allow(dead_code))]
pub(crate) fn anonymous_payoff_range<G: crate::AnonymousGame>(
    game: &G,
) -> Result<(f64, f64), GameError> {
    crate::game::check_common_action_space(game)?;
    let n_roles = game.n_roles();
    let n_actions = game.n_actions(0);
    if n_roles == 0 || n_actions == 0 {
        return Err(GameError::Population(
            "the game has no roles or no actions".to_owned(),
        ));
    }
    let others = n_roles - 1;
    let mut minimum = f64::INFINITY;
    let mut maximum = f64::NEG_INFINITY;
    let mut counts = vec![0_usize; n_actions];
    counts[n_actions - 1] = others;
    let mut visited = 0_u128;
    loop {
        visited += 1;
        if visited > crate::game::MAX_PROFILES {
            return Err(GameError::Population(
                "the payoff range requires enumerating too many count vectors".to_owned(),
            ));
        }
        for action in 0..n_actions {
            let payoff = game.payoff_by_counts(action, &counts);
            if !payoff.is_finite() {
                return Err(GameError::Population(
                    "the game has a non-finite payoff".to_owned(),
                ));
            }
            minimum = minimum.min(payoff);
            maximum = maximum.max(payoff);
        }
        if !next_composition(&mut counts) {
            break;
        }
    }
    Ok((minimum, maximum))
}

/// Advances `counts` to the next composition of the same total, returning
/// `false` after the last one.
fn next_composition(counts: &mut [usize]) -> bool {
    let last = counts.len() - 1;
    // Find the rightmost position before `last` that can receive one unit
    // from the tail.
    let tail = counts[last];
    if tail > 0 && last > 0 {
        counts[last] = 0;
        counts[last - 1] += 1;
        counts[last] = tail - 1;
        return true;
    }
    // Tail is empty: move everything after the rightmost non-zero entry.
    let Some(position) = (0..last).rev().find(|&index| counts[index] > 0) else {
        return false;
    };
    if position == 0 {
        return false;
    }
    let moved = counts[position];
    counts[position] = 0;
    counts[position - 1] += 1;
    counts[last] = moved - 1;
    true
}

/// Converts a single-encounter payoff range into the attainable per-step
/// payoff range for the configured aggregation and encounter counts.
#[cfg_attr(not(feature = "engine"), allow(dead_code))]
pub(crate) fn step_payoff_range(
    single: (f64, f64),
    bounds: Option<crate::EncounterBounds>,
    aggregation: crate::PayoffAggregation,
) -> Result<(f64, f64), GameError> {
    let (p_min, p_max) = single;
    let include_zero = |(low, high): (f64, f64), c_min: u32| {
        if c_min == 0 {
            (low.min(0.0), high.max(0.0))
        } else {
            (low, high)
        }
    };
    match aggregation {
        crate::PayoffAggregation::Sum => {
            let Some(bounds) = bounds else {
                return Err(GameError::Population(
                    "the matching does not report encounter counts; summed payoffs cannot be bounded, use PayoffAggregation::Mean".to_owned(),
                ));
            };
            let mut low = f64::INFINITY;
            let mut high = f64::NEG_INFINITY;
            for count in [bounds.min, bounds.max] {
                for payoff in [p_min, p_max] {
                    let value = f64::from(count) * payoff;
                    low = low.min(value);
                    high = high.max(value);
                }
            }
            Ok(include_zero((low, high), bounds.min))
        }
        crate::PayoffAggregation::Mean => {
            let c_min = bounds.map_or(0, |bounds| bounds.min);
            Ok(include_zero((p_min, p_max), c_min))
        }
    }
}

#[cfg_attr(not(feature = "engine"), allow(dead_code))]
pub(crate) fn check_held_only<G: StageGame, X: ExperimentState>(
    world: &GameWorld<G, X>,
    name: &str,
) -> Result<(), GameError> {
    if world
        .policies
        .slot
        .iter()
        .any(|&slot| slot != PolicyTable::<X::PolicyState>::HELD)
    {
        return Err(GameError::Population(format!(
            "{name} applies only to held agents, but an active rule is assigned"
        )));
    }
    Ok(())
}

/// Draws one comparison partner for agent `index`, or `None` when the agent
/// has no partner. A partner is never the agent itself.
fn comparison_partner(
    topology: &Topology,
    n_agents: usize,
    index: usize,
    rng: &mut SimRng,
) -> Option<usize> {
    match topology {
        Topology::None => {
            if n_agents < 2 {
                return None;
            }
            let draw = rng.gen_range(0..n_agents - 1);
            Some(if draw >= index { draw + 1 } else { draw })
        }
        #[cfg(feature = "net")]
        Topology::Network(network) => {
            let neighbors = network.neighbors(socsim_core::AgentId(index as u64));
            if neighbors.is_empty() {
                return None;
            }
            Some(neighbors[rng.gen_range(0..neighbors.len())].0 as usize)
        }
        #[cfg(feature = "grid")]
        Topology::Grid(grid) => {
            let neighbors = grid.adjacency().neighbors(index);
            if neighbors.is_empty() {
                return None;
            }
            Some(neighbors[rng.gen_range(0..neighbors.len())])
        }
    }
}

/// Visits every topology neighbor of agent `index` in sorted order.
fn for_each_neighbor(topology: &Topology, index: usize, mut visit: impl FnMut(usize)) {
    match topology {
        Topology::None => {
            let _ = (index, &mut visit);
        }
        #[cfg(feature = "net")]
        Topology::Network(network) => {
            for neighbor in network.neighbors(socsim_core::AgentId(index as u64)) {
                visit(neighbor.0 as usize);
            }
        }
        #[cfg(feature = "grid")]
        Topology::Grid(grid) => {
            for &neighbor in grid.adjacency().neighbors(index) {
                visit(neighbor);
            }
        }
    }
}

fn fermi_probability(z: f64) -> f64 {
    if z >= 0.0 {
        1.0 / (1.0 + (-z).exp())
    } else {
        let e = z.exp();
        e / (1.0 + e)
    }
}

fn check_payoffs(payoffs: &[f64], name: &str) -> socsim_core::Result<()> {
    if let Some(index) = payoffs.iter().position(|payoff| !payoff.is_finite()) {
        return Err(SocsimError::Mechanism(format!(
            "{name} found a non-finite payoff for agent {index}"
        )));
    }
    Ok(())
}

/// Per-agent rule of a copying dynamic. Returns the agent whose action is
/// adopted, or `None` to keep the agent's own action.
pub(crate) trait CopyKernel<G: StageGame>: Send + Sync + 'static {
    fn name(&self) -> &'static str;

    fn source(
        &self,
        game: &G,
        topology: &Topology,
        held: &[G::Action],
        payoffs: &[f64],
        index: usize,
        rng: &mut SimRng,
    ) -> Option<usize>;
}

pub(crate) struct ImitateBestKernel<G> {
    tie_break: ImitationTie,
    marker: PhantomData<fn() -> G>,
}

impl<G> CopyKernel<G> for ImitateBestKernel<G>
where
    G: NormalFormGame,
    G::Action: ActionSpace<G>,
{
    fn name(&self) -> &'static str {
        "imitate_best"
    }

    fn source(
        &self,
        _game: &G,
        topology: &Topology,
        held: &[G::Action],
        payoffs: &[f64],
        index: usize,
        _rng: &mut SimRng,
    ) -> Option<usize> {
        let mut best = payoffs[index];
        for_each_neighbor(topology, index, |neighbor| {
            best = best.max(payoffs[neighbor]);
        });
        let own_is_best = payoffs[index] == best;
        if own_is_best && self.tie_break == ImitationTie::KeepOwn {
            return None;
        }
        let mut chosen: Option<usize> = own_is_best.then_some(index);
        for_each_neighbor(topology, index, |neighbor| {
            if payoffs[neighbor] != best {
                return;
            }
            chosen = Some(match chosen {
                Some(current)
                    if <G::Action as ActionSpace<G>>::tie_cmp(&held[current], &held[neighbor])
                        != Ordering::Greater =>
                {
                    current
                }
                _ => neighbor,
            });
        });
        chosen.filter(|&source| source != index)
    }
}

pub(crate) struct FermiKernel {
    k: f64,
}

impl<G: NormalFormGame> CopyKernel<G> for FermiKernel {
    fn name(&self) -> &'static str {
        "fermi_update"
    }

    fn source(
        &self,
        _game: &G,
        topology: &Topology,
        _held: &[G::Action],
        payoffs: &[f64],
        index: usize,
        rng: &mut SimRng,
    ) -> Option<usize> {
        let partner = comparison_partner(topology, payoffs.len(), index, rng)?;
        let draw = rng.gen::<f64>();
        let z = (payoffs[partner] - payoffs[index]) / self.k;
        (draw < fermi_probability(z)).then_some(partner)
    }
}

pub(crate) struct ProportionalKernel {
    beta: f64,
}

impl<G: StageGame> CopyKernel<G> for ProportionalKernel {
    fn name(&self) -> &'static str {
        "replicator_update"
    }

    fn source(
        &self,
        _game: &G,
        topology: &Topology,
        _held: &[G::Action],
        payoffs: &[f64],
        index: usize,
        rng: &mut SimRng,
    ) -> Option<usize> {
        let partner = comparison_partner(topology, payoffs.len(), index, rng)?;
        let draw = rng.gen::<f64>();
        let probability = (self.beta * (payoffs[partner] - payoffs[index]).max(0.0)).min(1.0);
        (draw < probability).then_some(partner)
    }
}

/// Synchronous copying dynamic: every next action is computed into a buffer
/// from the pre-update state, then all actions are written at once.
pub(crate) struct CopyMechanism<G: StageGame, X, K> {
    kernel: K,
    next: Vec<G::Action>,
    marker: PhantomData<fn() -> X>,
}

impl<G: StageGame, X, K> CopyMechanism<G, X, K> {
    fn new(kernel: K) -> Self {
        Self {
            kernel,
            next: Vec::new(),
            marker: PhantomData,
        }
    }
}

impl<G, X, K> Mechanism<GameWorld<G, X>> for CopyMechanism<G, X, K>
where
    G: StageGame,
    X: ExperimentState,
    K: CopyKernel<G>,
{
    fn name(&self) -> &str {
        self.kernel.name()
    }

    fn phases(&self) -> &'static [Phase] {
        &[Phase::Reward]
    }

    fn apply(
        &mut self,
        _phase: Phase,
        step: &mut StepContext<'_, GameWorld<G, X>>,
    ) -> socsim_core::Result<()> {
        let world = &mut *step.world;
        let n_agents = world.held.len();
        if world.payoff.len() != n_agents {
            return Err(SocsimError::Mechanism(format!(
                "{} expected {n_agents} payoffs, got {}",
                self.kernel.name(),
                world.payoff.len()
            )));
        }
        check_payoffs(&world.payoff, self.kernel.name())?;
        self.next.clear();
        for index in 0..n_agents {
            let source = self.kernel.source(
                &world.game,
                &world.topology,
                &world.held,
                &world.payoff,
                index,
                step.rng,
            );
            self.next.push(world.held[source.unwrap_or(index)].clone());
        }
        std::mem::swap(&mut world.held, &mut self.next);
        Ok(())
    }
}

/// Moran birth-death mechanism. It writes exactly one held action per step.
pub(crate) struct MoranMechanism<G, X> {
    fitness: Fitness,
    mutation: f64,
    weights: Vec<f64>,
    marker: PhantomData<fn() -> (G, X)>,
}

impl<G, X> Mechanism<GameWorld<G, X>> for MoranMechanism<G, X>
where
    G: NormalFormGame,
    G::Action: ActionSpace<G>,
    X: ExperimentState,
{
    fn name(&self) -> &str {
        "moran_update"
    }

    fn phases(&self) -> &'static [Phase] {
        &[Phase::Reward]
    }

    fn apply(
        &mut self,
        _phase: Phase,
        step: &mut StepContext<'_, GameWorld<G, X>>,
    ) -> socsim_core::Result<()> {
        let world = &mut *step.world;
        let n_agents = world.held.len();
        if n_agents == 0 {
            return Ok(());
        }
        check_payoffs(&world.payoff, "moran_update")?;
        self.weights.clear();
        let mut total = 0.0;
        for (index, &payoff) in world.payoff.iter().enumerate() {
            let weight = self.fitness.value(payoff);
            if !weight.is_finite() || weight < 0.0 {
                return Err(SocsimError::Mechanism(format!(
                    "moran_update computed an invalid fitness {weight} for agent {index}"
                )));
            }
            total += weight;
            self.weights.push(weight);
        }
        if !(total.is_finite() && total > 0.0) {
            return Err(SocsimError::Mechanism(format!(
                "moran_update requires a positive finite total fitness, got {total}"
            )));
        }

        let target = step.rng.gen::<f64>() * total;
        let mut cumulative = 0.0;
        let mut parent = None;
        let mut last_positive = 0;
        for (index, &weight) in self.weights.iter().enumerate() {
            if weight > 0.0 {
                last_positive = index;
            }
            cumulative += weight;
            if parent.is_none() && target < cumulative {
                parent = Some(index);
            }
        }
        let parent = parent.unwrap_or(last_positive);
        let death = step.rng.gen_range(0..n_agents);
        let mutate = step.rng.gen::<f64>() < self.mutation;
        let mutant =
            <G::Action as ActionSpace<G>>::mutate(&world.game, &world.held[parent], step.rng);
        world.held[death] = if mutate {
            mutant
        } else {
            world.held[parent].clone()
        };
        Ok(())
    }
}

#[cfg(feature = "engine")]
mod build {
    use super::*;
    use crate::assemble::sealed::{DynamicsBuild, DynamicsCheck};
    use crate::CommonActionSpace;

    type Boxed<G, X> = Box<dyn Mechanism<GameWorld<G, X>>>;

    fn require_synchronous(update: Update) -> Result<(), GameError> {
        match update {
            Update::Synchronous => Ok(()),
        }
    }

    fn single_range<G, X>(
        check: &DynamicsCheck<'_, G, X>,
        name: &str,
        from_game: impl FnOnce(&G) -> Result<(f64, f64), GameError>,
    ) -> Result<(f64, f64), GameError>
    where
        G: StageGame,
        X: ExperimentState,
    {
        match &check.payoff_source {
            PayoffRangeSource::Game => from_game(&check.world.game),
            PayoffRangeSource::Counts(range) => range(&check.world.game),
            PayoffRangeSource::Unknown => Err(GameError::Population(format!(
                "{name} cannot bound payoffs produced by a user-defined outcome rule"
            ))),
        }
    }

    impl<G, X> DynamicsBuild<G, X> for ImitateBest
    where
        G: NormalFormGame + CommonActionSpace,
        G::Action: ActionSpace<G>,
        X: ExperimentState,
    {
        fn prepare(&self, check: &DynamicsCheck<'_, G, X>) -> Result<(), GameError> {
            require_synchronous(self.update)?;
            check_held_only(check.world, "imitate_best")?;
            <G::Action as ActionSpace<G>>::check_common_space(&check.world.game)?;
            if matches!(check.world.topology, Topology::None) {
                return Err(GameError::Population(
                    "imitate_best requires a grid or network topology".to_owned(),
                ));
            }
            Ok(())
        }

        fn into_mechanism(self) -> Option<Boxed<G, X>> {
            Some(Box::new(CopyMechanism::<G, X, _>::new(
                ImitateBestKernel::<G> {
                    tie_break: self.tie_break,
                    marker: PhantomData,
                },
            )))
        }
    }

    impl<G, X> DynamicsBuild<G, X> for FermiUpdate
    where
        G: NormalFormGame + CommonActionSpace,
        G::Action: ActionSpace<G>,
        X: ExperimentState,
    {
        fn prepare(&self, check: &DynamicsCheck<'_, G, X>) -> Result<(), GameError> {
            require_synchronous(self.update)?;
            check_held_only(check.world, "fermi_update")?;
            <G::Action as ActionSpace<G>>::check_common_space(&check.world.game)
        }

        fn into_mechanism(self) -> Option<Boxed<G, X>> {
            Some(Box::new(CopyMechanism::<G, X, _>::new(FermiKernel {
                k: self.k,
            })))
        }
    }

    impl<G, X> DynamicsBuild<G, X> for ProportionalImitation
    where
        G: FiniteGame + CommonActionSpace,
        X: ExperimentState,
    {
        fn prepare(&self, check: &DynamicsCheck<'_, G, X>) -> Result<(), GameError> {
            require_synchronous(self.update)?;
            check_held_only(check.world, "replicator_update")?;
            crate::game::check_common_action_space(&check.world.game)?;
            let single = single_range(check, "replicator_update", finite_payoff_range)?;
            let (low, high) = step_payoff_range(single, check.encounter_bounds, check.aggregation)?;
            let width = high - low;
            if !width.is_finite() {
                return Err(GameError::Population(
                    "the per-step payoff range is not finite".to_owned(),
                ));
            }
            if self.beta * width > 1.0 + 4.0 * f64::EPSILON {
                return Err(GameError::Population(format!(
                    "replicator_update requires beta <= 1 / {width} (the per-step payoff range), got {}",
                    self.beta
                )));
            }
            Ok(())
        }

        fn into_mechanism(self) -> Option<Boxed<G, X>> {
            Some(Box::new(CopyMechanism::<G, X, _>::new(
                ProportionalKernel { beta: self.beta },
            )))
        }
    }

    impl<G, X> DynamicsBuild<G, X> for MoranUpdate
    where
        G: NormalFormGame + CommonActionSpace,
        G::Action: ActionSpace<G>,
        X: ExperimentState,
    {
        fn prepare(&self, check: &DynamicsCheck<'_, G, X>) -> Result<(), GameError> {
            require_synchronous(self.update)?;
            check_held_only(check.world, "moran_update")?;
            <G::Action as ActionSpace<G>>::check_common_space(&check.world.game)?;
            if !matches!(check.world.topology, Topology::None) {
                return Err(GameError::Population(
                    "moran_update requires a world without topology".to_owned(),
                ));
            }
            if let Fitness::Linear { w } = self.fitness {
                let single = single_range(
                    check,
                    "moran_update",
                    <G::Action as ActionSpace<G>>::payoff_range,
                )?;
                let (low, _) =
                    step_payoff_range(single, check.encounter_bounds, check.aggregation)?;
                let fitness = 1.0 - w + w * low;
                if !(fitness.is_finite() && fitness > 0.0) {
                    return Err(GameError::Population(format!(
                        "linear fitness 1 - w + w * {low} = {fitness} must be positive for the lowest attainable payoff"
                    )));
                }
            }
            Ok(())
        }

        fn into_mechanism(self) -> Option<Boxed<G, X>> {
            Some(Box::new(MoranMechanism::<G, X> {
                fitness: self.fitness,
                mutation: self.mutation,
                weights: Vec::new(),
                marker: PhantomData,
            }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compositions_cover_every_count_vector_once() {
        let mut counts = vec![0, 0, 3];
        let mut seen = vec![counts.clone()];
        while next_composition(&mut counts) {
            seen.push(counts.clone());
        }
        assert_eq!(seen.len(), 10);
        let mut sorted = seen.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), 10);
        assert!(seen.iter().all(|row| row.iter().sum::<usize>() == 3));
    }

    #[test]
    fn fermi_probability_is_stable_for_large_arguments() {
        assert_eq!(fermi_probability(0.0), 0.5);
        assert!(fermi_probability(1e6) <= 1.0);
        assert!(fermi_probability(-1e6) >= 0.0);
        assert!((fermi_probability(2.0) + fermi_probability(-2.0) - 1.0).abs() < 1e-15);
    }

    #[test]
    fn step_payoff_range_follows_aggregation_and_idle_agents() {
        use crate::{EncounterBounds, PayoffAggregation};
        let bounds = |min, max| Some(EncounterBounds { min, max });
        assert_eq!(
            step_payoff_range((-1.0, 2.0), bounds(1, 1), PayoffAggregation::Sum).unwrap(),
            (-1.0, 2.0)
        );
        assert_eq!(
            step_payoff_range((1.0, 2.0), bounds(0, 1), PayoffAggregation::Sum).unwrap(),
            (0.0, 2.0)
        );
        assert_eq!(
            step_payoff_range((-1.0, 2.0), bounds(8, 9), PayoffAggregation::Sum).unwrap(),
            (-9.0, 18.0)
        );
        assert_eq!(
            step_payoff_range((1.0, 2.0), None, PayoffAggregation::Mean).unwrap(),
            (0.0, 2.0)
        );
        assert!(step_payoff_range((1.0, 2.0), None, PayoffAggregation::Sum).is_err());
    }
}
