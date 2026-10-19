//! Information-scoped views passed to decision and learning rules.
//!
//! See the [information-structure guide](https://github.com/akitenkrad/rs-social-simulation-tools/blob/main/docs/games.md#information-structure-default-views-view_for-and-omniscient).

use serde::{Deserialize, Serialize};
use socsim_core::AgentId;

use crate::{
    world::{InternalAggregateTable, InternalPairHistory, InternalStepHistory},
    Credit, EncounterId, EncounterRef, ExperimentState, HistoryView, PairView, PeerHistory,
    PlayArena, StageGame, StepRecord, Topology,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
/// Granularity at which each agent chooses actions during a step.
pub enum DecisionScope {
    /// Choose once and reuse the action for all roles and encounters in the step.
    #[default]
    PerStep,
    /// Choose once for each distinct role occupied during the step.
    PerAgentRole,
    /// Choose independently for every encounter.
    PerEncounter,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
/// Identity of one concrete decision within a step.
pub enum DecisionUnit {
    /// The agent's single decision for the whole step.
    Step,
    /// A decision shared by all encounters in the contained zero-based role.
    AgentRole(usize),
    /// A decision local to the contained encounter.
    Encounter(EncounterId),
}

/// Read-only, information-scoped input supplied to [`crate::DecisionRule::decide`].
///
/// Ordinary accessors expose only the focal agent, current opponents, public
/// history, and the experiment's private view. [`omniscient`](Self::omniscient)
/// is an explicit escape hatch for models that require global state.
pub struct DecisionContext<'a, G: StageGame, X: ExperimentState> {
    pub(crate) t: u64,
    pub(crate) me: AgentId,
    pub(crate) unit: DecisionUnit,
    pub(crate) role: Option<usize>,
    pub(crate) encounter: Option<EncounterRef<'a>>,
    pub(crate) opponents: Vec<AgentId>,
    pub(crate) ids: &'a [AgentId],
    pub(crate) game: &'a G,
    pub(crate) held: &'a [G::Action],
    pub(crate) history: &'a InternalStepHistory<G::Action>,
    pub(crate) pair_history: Option<&'a InternalPairHistory<G::Action>>,
    pub(crate) aggregate: Option<&'a InternalAggregateTable>,
    pub(crate) x: &'a X,
}

impl<'a, G: StageGame, X: ExperimentState> DecisionContext<'a, G, X> {
    /// Returns the current simulation step.
    ///
    /// The first step is `1`: the engine advances the clock before mechanisms run.
    pub fn t(&self) -> u64 {
        self.t
    }

    /// Returns the focal agent making this decision.
    pub fn me(&self) -> AgentId {
        self.me
    }

    /// Returns the concrete unit for which an action is being chosen.
    pub fn unit(&self) -> DecisionUnit {
        self.unit
    }

    /// Returns the configured granularity represented by this context.
    pub fn scope(&self) -> DecisionScope {
        match self.unit {
            DecisionUnit::Step => DecisionScope::PerStep,
            DecisionUnit::AgentRole(_) => DecisionScope::PerAgentRole,
            DecisionUnit::Encounter(_) => DecisionScope::PerEncounter,
        }
    }

    /// Returns the unique role for this decision, if one exists.
    pub fn role(&self) -> Option<usize> {
        self.role
    }

    /// Returns the encounter metadata for a per-encounter decision.
    pub fn encounter(&self) -> Option<EncounterRef<'a>> {
        self.encounter
    }

    /// Returns current opponents in encounter traversal order without duplicates.
    pub fn opponents(&self) -> &[AgentId] {
        &self.opponents
    }

    /// Returns the focal agent's retained step records, oldest first.
    pub fn my_history(&self) -> HistoryView<'a, StepRecord<G::Action>> {
        self.history
            .view(self.me)
            .expect("DecisionContext is constructed only for a known agent")
    }

    /// Returns one current opponent's history, or `None` for any other agent.
    pub fn opponent_history(&self, opponent: AgentId) -> Option<PeerHistory<'a, G::Action>> {
        self.opponents
            .contains(&opponent)
            .then(|| self.history.view(opponent))
            .flatten()
            .map(PeerHistory::new)
    }

    /// Returns this agent's own held action before this decision: its initial
    /// action, or the action it last chose.
    pub fn my_held(&self) -> &'a G::Action {
        self.held
            .get(self.me.0 as usize)
            .expect("DecisionContext is constructed only for a known agent")
    }

    /// Returns the held action of a peer in this decision unit.
    pub fn peer_held(&self, peer: AgentId) -> Option<&'a G::Action> {
        self.opponents
            .contains(&peer)
            .then(|| self.held.get(peer.0 as usize))
            .flatten()
    }

    /// Returns pair-specific history for an opponent in this decision unit.
    ///
    /// `None` means that pair history was not enabled for this world, or that
    /// `other` is not among [`Self::opponents`]. If pair history is enabled and
    /// `other` is a current opponent, a pair that has not played before returns
    /// an empty view.
    pub fn pair_history(&self, other: AgentId) -> Option<PairView<'a, G::Action>> {
        self.opponents
            .contains(&other)
            .then(|| {
                self.pair_history
                    .map(|history| history.view(self.me, other))
            })
            .flatten()
    }

    /// Returns the public aggregate history from completed prior steps.
    ///
    /// `None` means aggregate collection was not enabled for this simulation.
    pub fn aggregate_history(&self) -> Option<AggregateView<'a>> {
        self.aggregate.map(AggregateView::new)
    }

    /// Returns the immutable stage game.
    pub fn game(&self) -> &'a G {
        self.game
    }

    /// Returns the experiment-defined private view for the focal agent.
    pub fn view(&self) -> X::View<'_> {
        self.x.view_for(self.me)
    }

    /// Returns unrestricted world data.
    ///
    /// Calling this removes the information-structure guarantee of the default
    /// view. During Decision, `held` may also depend on activation order because
    /// agents activated earlier have already updated their held action.
    pub fn omniscient(&self) -> OmniscientContext<'a, G, X> {
        OmniscientContext {
            ids: self.ids,
            held: self.held,
            history: self.history,
            pair_history: self.pair_history,
            x: self.x,
            marker: std::marker::PhantomData,
        }
    }
}

/// Read-only chronological access to public game aggregates.
#[derive(Clone, Copy, Debug)]
pub struct AggregateView<'a> {
    table: &'a InternalAggregateTable,
}

impl<'a> AggregateView<'a> {
    pub(crate) fn new(table: &'a InternalAggregateTable) -> Self {
        Self { table }
    }

    /// Returns aggregate names in column order.
    pub fn names(&self) -> &'a [String] {
        self.table.names()
    }

    /// Returns the number of retained steps.
    pub fn len(&self) -> usize {
        self.table.filled()
    }

    /// Returns `true` when no completed step is retained.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Iterates over `(t, values)` from oldest to newest.
    pub fn iter(&self) -> impl Iterator<Item = (u64, &'a [Option<f64>])> + 'a {
        self.table.iter()
    }

    /// Returns the newest retained row.
    pub fn last(&self) -> Option<(u64, &'a [Option<f64>])> {
        self.table.last()
    }

    /// Iterates over one named column from oldest to newest.
    pub fn column(&self, name: &str) -> Option<impl Iterator<Item = Option<f64>> + 'a> {
        let index = self
            .table
            .names()
            .iter()
            .position(|candidate| candidate == name)?;
        Some(self.table.iter().map(move |(_, values)| values[index]))
    }
}

/// Read-only step result supplied to decision rules during learning.
pub struct LearnContext<'a, G: StageGame, X: ExperimentState> {
    pub(crate) t: u64,
    pub(crate) me: AgentId,
    pub(crate) scope: DecisionScope,
    pub(crate) record: &'a StepRecord<G::Action>,
    pub(crate) ids: &'a [AgentId],
    pub(crate) game: &'a G,
    pub(crate) held: &'a [G::Action],
    pub(crate) history: &'a InternalStepHistory<G::Action>,
    pub(crate) plays: &'a PlayArena<G::Action>,
    pub(crate) x: &'a X,
}

impl<'a, G: StageGame, X: ExperimentState> LearnContext<'a, G, X> {
    /// Returns the simulation step that just completed.
    ///
    /// The first completed step is `1`: the engine advances the clock before mechanisms run.
    pub fn t(&self) -> u64 {
        self.t
    }

    /// Returns the focal agent whose state is being updated.
    pub fn me(&self) -> AgentId {
        self.me
    }

    /// Returns the decision granularity used during the completed step.
    pub fn scope(&self) -> DecisionScope {
        self.scope
    }

    /// Returns the focal agent's aggregate record for the completed step.
    pub fn record(&self) -> &'a StepRecord<G::Action> {
        self.record
    }

    /// Returns the focal agent's retained records including the completed step.
    pub fn my_history(&self) -> HistoryView<'a, StepRecord<G::Action>> {
        self.history
            .view(self.me)
            .expect("LearnContext is constructed only for a known agent")
    }

    /// Returns this agent's encounter outcomes in ascending [`EncounterId`]
    /// order.
    ///
    /// The play mechanism has already verified every seat, so every entry of
    /// [`EncounterOutcome::actions`] is `Some` at this point.
    pub fn outcomes(&self) -> impl Iterator<Item = EncounterOutcome<'a, G::Action>> + '_ {
        (0..self.plays.n_encounters()).filter_map(move |encounter_index| {
            let start = encounter_index * self.plays.n_roles;
            let end = start + self.plays.n_roles;
            let seats = &self.plays.seats[start..end];
            let role = seats.iter().position(|&agent| agent == self.me)?;
            Some(EncounterOutcome {
                id: EncounterId(encounter_index as u32),
                role,
                seats,
                actions: &self.plays.actions[start..end],
                payoffs: &self.plays.payoffs[start..end],
                credit: self.plays.credit[encounter_index],
            })
        })
    }

    /// Returns the immutable stage game.
    pub fn game(&self) -> &'a G {
        self.game
    }

    /// Returns the experiment-defined private view for the focal agent.
    pub fn view(&self) -> X::View<'_> {
        self.x.view_for(self.me)
    }

    /// Returns unrestricted world data. This removes the default information
    /// structure guarantee; see [`DecisionContext::omniscient`].
    pub fn omniscient(&self) -> OmniscientContext<'a, G, X> {
        OmniscientContext {
            ids: self.ids,
            held: self.held,
            history: self.history,
            pair_history: None,
            x: self.x,
            marker: std::marker::PhantomData,
        }
    }
}

/// Explicit unrestricted read-only view of game-world state.
///
/// Obtaining this view opts a rule out of the default information-structure
/// boundary. It still exposes no mutation methods.
pub struct OmniscientContext<'a, G: StageGame, X: ExperimentState> {
    ids: &'a [AgentId],
    held: &'a [G::Action],
    history: &'a InternalStepHistory<G::Action>,
    pair_history: Option<&'a InternalPairHistory<G::Action>>,
    x: &'a X,
    marker: std::marker::PhantomData<G>,
}

impl<'a, G: StageGame, X: ExperimentState> OmniscientContext<'a, G, X> {
    /// Returns all agent identifiers in stable world order.
    pub fn ids(&self) -> &'a [AgentId] {
        self.ids
    }

    /// Returns any agent's currently held action, or `None` for an unknown ID.
    pub fn held(&self, agent: AgentId) -> Option<&'a G::Action> {
        self.held.get(agent.0 as usize)
    }

    /// Returns any known agent's retained step history.
    pub fn history(&self, agent: AgentId) -> Option<HistoryView<'a, StepRecord<G::Action>>> {
        self.history.view(agent)
    }

    /// Returns the history of `(a, b)`, oriented as `a`'s actions followed by
    /// `b`'s actions.
    ///
    /// `None` means pair history was not enabled, at least one agent is not in
    /// this world, or the context that created this view does not expose pair
    /// history. A known pair that has not played returns an empty view.
    pub fn pair_history(&self, a: AgentId, b: AgentId) -> Option<PairView<'a, G::Action>> {
        (self.ids.contains(&a) && self.ids.contains(&b))
            .then(|| self.pair_history.map(|history| history.view(a, b)))
            .flatten()
    }

    /// Returns unrestricted experiment state.
    pub fn x(&self) -> &'a X {
        self.x
    }
}

#[derive(Clone, Copy, Debug)]
/// One completed encounter as observed by a participating agent during learning.
pub struct EncounterOutcome<'a, A> {
    /// Stable encounter index within the completed step.
    pub id: EncounterId,
    /// Zero-based seat occupied by the focal agent.
    pub role: usize,
    /// Agent identifiers indexed by role.
    pub seats: &'a [AgentId],
    /// Actions indexed by role; every entry is `Some` after successful play.
    pub actions: &'a [Option<A>],
    /// Realized payoffs indexed by role.
    pub payoffs: &'a [f64],
    /// Which seats receive these payoffs in their step aggregates.
    pub credit: Credit,
}

#[allow(dead_code)]
fn _topology_surface(_: &Topology) {}
