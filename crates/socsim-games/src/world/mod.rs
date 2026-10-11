mod aggregate_table;
mod arena;
mod history;
mod metrics_state;
mod pair_history;
mod policy;
mod reports;
mod state;
mod topology;

pub use arena::{ArenaWriter, Credit, EncounterId, EncounterRef, PlayArena};
pub use history::{HistoryView, PeerHistory, StepRecord};
pub use pair_history::PairView;
pub use policy::{PolicyTable, RuleKindId};
pub use state::{
    AutomatonPolicyState, BeliefState, BuiltinPolicyState, EstimateState, ExperimentState,
    PropensityState, Slot, Stateless, TftState,
};
#[cfg(feature = "net")]
pub use topology::NetworkTopology;
pub use topology::Topology;
#[cfg(feature = "grid")]
pub use topology::{GridBoundary, GridNeighbors, GridSpec, GridTopology};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use socsim_core::{AgentId, Neighbors, SimClock, WorldState};

use crate::{matching::MatchingState, StageGame};

use self::history::StepHistory;
use self::metrics_state::MetricsState;
use self::pair_history::PairHistory;
use self::reports::PolicyReports;
use aggregate_table::AggregateTable;

/// Complete mutable state of a game simulation.
///
/// Values in `X` are included in snapshots. `X` must therefore contain only
/// state that can be restored, rather than external clients or file handles.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(bound(
    serialize = "G: Serialize, G::Action: Serialize, X: Serialize, X::PolicyState: Serialize",
    deserialize = "G: DeserializeOwned, G::Action: DeserializeOwned, X: DeserializeOwned, X::PolicyState: DeserializeOwned"
))]
pub struct GameWorld<G: StageGame, X: ExperimentState = ()> {
    pub(crate) clock: SimClock,
    pub(crate) ids: Vec<AgentId>,
    pub(crate) game: G,
    pub(crate) topology: Topology,
    pub(crate) held: Vec<G::Action>,
    pub(crate) policies: PolicyTable<X::PolicyState>,
    pub(crate) matching_state: MatchingState,
    pub(crate) plays: PlayArena<G::Action>,
    pub(crate) payoff: Vec<f64>,
    pub(crate) n_played: Vec<u32>,
    #[serde(skip)]
    pub(crate) played_counts: Option<PlayedActionCounts>,
    pub(crate) step_history: StepHistory<G::Action>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) pair_history: Option<PairHistory<G::Action>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) aggregate: Option<AggregateTable>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) reports: Option<PolicyReports>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) metrics_state: Option<MetricsState>,
    pub(crate) x: X,
    pub(crate) decision_root: u64,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct PlayedActionCounts {
    by_role: Vec<Vec<usize>>,
    total: usize,
}

impl PlayedActionCounts {
    pub(crate) fn new(n_roles: usize) -> Self {
        Self {
            by_role: vec![Vec::new(); n_roles],
            total: 0,
        }
    }

    pub(crate) fn increment(&mut self, role: usize, action: usize) -> socsim_core::Result<()> {
        let row = self.by_role.get_mut(role).ok_or_else(|| {
            socsim_core::SocsimError::Mechanism(format!("played-action count has no role {role}"))
        })?;
        if row.len() <= action {
            row.resize(action + 1, 0);
        }
        row[action] = row[action].checked_add(1).ok_or_else(|| {
            socsim_core::SocsimError::Mechanism(format!(
                "played-action count overflowed for role {role} action {action}"
            ))
        })?;
        self.total = self.total.checked_add(1).ok_or_else(|| {
            socsim_core::SocsimError::Mechanism("played seat count overflowed".to_owned())
        })?;
        Ok(())
    }

    pub(crate) fn by_role(&self) -> &[Vec<usize>] {
        &self.by_role
    }

    pub(crate) fn total(&self) -> usize {
        self.total
    }
}

impl<G: StageGame, X: ExperimentState> GameWorld<G, X> {
    /// Replaces the world's topology after checking it against the agents.
    ///
    /// A network must have exactly the world's agent IDs as vertices and no
    /// self-loops; a grid must have exactly one cell per agent. Otherwise this
    /// returns [`GameError::Topology`](crate::GameError::Topology) and the world is
    /// dropped. A freshly built world has [`Topology::None`].
    pub fn with_topology(mut self, topology: Topology) -> Result<Self, crate::GameError> {
        topology.validate_agents(&self.ids)?;
        self.topology = topology;
        Ok(self)
    }

    pub fn game(&self) -> &G {
        &self.game
    }

    pub fn x(&self) -> &X {
        &self.x
    }

    pub fn x_mut(&mut self) -> &mut X {
        &mut self.x
    }

    pub fn held(&self) -> &[G::Action] {
        &self.held
    }

    pub fn policies(&self) -> &PolicyTable<X::PolicyState> {
        &self.policies
    }

    pub fn plays(&self) -> &PlayArena<G::Action> {
        &self.plays
    }

    pub fn payoffs(&self) -> &[f64] {
        &self.payoff
    }

    pub fn n_played(&self) -> &[u32] {
        &self.n_played
    }

    pub fn history(&self, agent: AgentId) -> Option<HistoryView<'_, StepRecord<G::Action>>> {
        self.step_history.view(agent)
    }

    /// Returns the latest mixed strategy reported for an agent and role.
    ///
    /// Reports are written at the end of `Reward`, after every native rule has
    /// completed learning. They therefore describe the mixed strategy used by
    /// the next step's decision, not the strategy that generated the current
    /// step's action.
    pub fn reported_mixed(&self, agent: AgentId, role: usize) -> Option<&[f64]> {
        let agent = usize::try_from(agent.0).ok()?;
        self.reports.as_ref()?.get(agent, role)
    }
}

impl<G: StageGame, X: ExperimentState> Neighbors for GameWorld<G, X> {
    fn neighbors_of(&self, _id: AgentId) -> Vec<AgentId> {
        match &self.topology {
            Topology::None => Vec::new(),
            #[cfg(feature = "net")]
            Topology::Network(network) => network.neighbors(_id).to_vec(),
            #[cfg(feature = "grid")]
            Topology::Grid(grid) => {
                let Ok(index) = usize::try_from(_id.0) else {
                    return Vec::new();
                };
                if index >= grid.adjacency().len() {
                    return Vec::new();
                }
                grid.adjacency()
                    .neighbors(index)
                    .iter()
                    .map(|&neighbor| AgentId(neighbor as u64))
                    .collect()
            }
        }
    }
}

impl<G: StageGame, X: ExperimentState> WorldState for GameWorld<G, X> {
    fn agent_ids(&self) -> Vec<AgentId> {
        self.ids.clone()
    }

    fn clock(&self) -> &SimClock {
        &self.clock
    }

    fn clock_mut(&mut self) -> &mut SimClock {
        &mut self.clock
    }
}

pub(crate) use aggregate_table::AggregateTable as InternalAggregateTable;
pub(crate) use history::StepHistory as InternalStepHistory;
pub(crate) use metrics_state::MetricsState as InternalMetricsState;
pub(crate) use pair_history::PairHistory as InternalPairHistory;
pub(crate) use reports::PolicyReports as InternalPolicyReports;
pub(crate) use PlayedActionCounts as InternalPlayedActionCounts;
