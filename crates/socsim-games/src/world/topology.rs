use serde::{Deserialize, Serialize};

use crate::GameError;

#[cfg(feature = "grid")]
use socsim_grid::{Adjacency, Boundary, Grid, Neighborhood};
#[cfg(feature = "net")]
use socsim_net::{DiSocialNetwork, SocialNetwork};

/// Spatial structure that matchings and population dynamics read.
///
/// Attach one to a world with [`GameWorld::with_topology`](crate::GameWorld::with_topology),
/// which checks it against the world's agents. Serialization stores only the
/// network or the `GridSpec`; neighbor tables are rebuilt on deserialization.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(try_from = "TopologyDto", into = "TopologyDto")]
#[non_exhaustive]
pub enum Topology {
    /// No spatial structure; the default of a new world.
    None,
    /// A social network whose vertices are the world's agent IDs.
    #[cfg(feature = "net")]
    Network(NetworkTopology),
    /// A rectangular lattice with one agent per cell.
    #[cfg(feature = "grid")]
    Grid(GridTopology),
}

impl Topology {
    /// Wraps an undirected network.
    ///
    /// Each edge is one undirected link; neighbors are the adjacent vertices. The
    /// vertex set is checked against the world's agents only when the topology is
    /// attached with [`GameWorld::with_topology`](crate::GameWorld::with_topology).
    #[cfg(feature = "net")]
    pub fn network(network: SocialNetwork) -> Self {
        Self::Network(NetworkTopology::undirected(network))
    }

    /// Wraps a directed network.
    ///
    /// Each arc is kept with its direction: an agent's neighbors are its
    /// out-neighbors, and [`NetworkEdges`](crate::NetworkEdges) seats the arc's
    /// source in role 0 and its target in role 1. The vertex set is checked only
    /// when the topology is attached with
    /// [`GameWorld::with_topology`](crate::GameWorld::with_topology).
    #[cfg(feature = "net")]
    pub fn directed_network(network: DiSocialNetwork) -> Self {
        Self::Network(NetworkTopology::directed(network))
    }

    /// Builds a grid topology and precomputes its neighbor table.
    ///
    /// Returns [`GameError::Topology`] when `rows` or `cols` is zero or the cell
    /// count overflows `usize`. The agent count is checked later by
    /// [`GameWorld::with_topology`](crate::GameWorld::with_topology).
    #[cfg(feature = "grid")]
    pub fn grid(spec: GridSpec) -> Result<Self, GameError> {
        Ok(Self::Grid(GridTopology::new(spec)?))
    }

    pub(crate) fn validate_agents(&self, _ids: &[socsim_core::AgentId]) -> Result<(), GameError> {
        match self {
            Self::None => Ok(()),
            #[cfg(feature = "net")]
            Self::Network(network) => network.validate_agents(_ids),
            #[cfg(feature = "grid")]
            Self::Grid(grid) => {
                let cells = grid.spec.cell_count()?;
                if cells != _ids.len() {
                    return Err(GameError::Topology(format!(
                        "grid has {cells} cells but the world has {} agents",
                        _ids.len()
                    )));
                }
                Ok(())
            }
        }
    }
}

/// A network topology with precomputed, sorted neighbor lists.
///
/// Built by [`Topology::network`] or [`Topology::directed_network`]. Duplicate
/// edges are collapsed, and neighbor lists and the edge list are sorted by agent
/// ID, so traversal order does not depend on insertion order. A network with a
/// self-loop, or whose vertices differ from the world's agent IDs, is rejected
/// by [`GameWorld::with_topology`](crate::GameWorld::with_topology).
#[cfg(feature = "net")]
#[derive(Clone, Debug)]
pub struct NetworkTopology {
    network: NetworkKind,
    offsets: Vec<usize>,
    neighbors: Vec<socsim_core::AgentId>,
    edges: Vec<(socsim_core::AgentId, socsim_core::AgentId)>,
}

#[cfg(feature = "net")]
#[derive(Clone, Debug)]
enum NetworkKind {
    Undirected(SocialNetwork),
    Directed(DiSocialNetwork),
}

#[cfg(feature = "net")]
impl NetworkTopology {
    fn undirected(network: SocialNetwork) -> Self {
        Self::from_kind(NetworkKind::Undirected(network))
    }

    fn directed(network: DiSocialNetwork) -> Self {
        Self::from_kind(NetworkKind::Directed(network))
    }

    fn from_kind(network: NetworkKind) -> Self {
        let node_count = match &network {
            NetworkKind::Undirected(network) => network.node_count(),
            NetworkKind::Directed(network) => network.node_count(),
        };
        let mut offsets = Vec::with_capacity(node_count + 1);
        let mut neighbors = Vec::new();
        offsets.push(0);
        for index in 0..node_count {
            let id = socsim_core::AgentId(index as u64);
            let mut row = match &network {
                NetworkKind::Undirected(network) => network.neighbors(id),
                NetworkKind::Directed(network) => network.out_neighbors(id),
            };
            row.sort_unstable();
            row.dedup();
            neighbors.extend(row);
            offsets.push(neighbors.len());
        }
        let mut edges = match &network {
            NetworkKind::Undirected(network) => network.edges().collect::<Vec<_>>(),
            NetworkKind::Directed(network) => network.edges().collect::<Vec<_>>(),
        };
        edges.sort_unstable();
        edges.dedup();
        Self {
            network,
            offsets,
            neighbors,
            edges,
        }
    }

    pub(crate) fn edges(&self) -> &[(socsim_core::AgentId, socsim_core::AgentId)] {
        &self.edges
    }

    pub(crate) fn neighbors(&self, id: socsim_core::AgentId) -> &[socsim_core::AgentId] {
        let Ok(index) = usize::try_from(id.0) else {
            return &[];
        };
        let Some((&start, &end)) = self.offsets.get(index).zip(self.offsets.get(index + 1)) else {
            return &[];
        };
        &self.neighbors[start..end]
    }

    fn validate_agents(&self, ids: &[socsim_core::AgentId]) -> Result<(), GameError> {
        let node_count = match &self.network {
            NetworkKind::Undirected(network) => network.node_count(),
            NetworkKind::Directed(network) => network.node_count(),
        };
        let contains = |id| match &self.network {
            NetworkKind::Undirected(network) => network.contains(id),
            NetworkKind::Directed(network) => network.contains(id),
        };
        if node_count != ids.len() || ids.iter().copied().any(|id| !contains(id)) {
            return Err(GameError::Topology(
                "network vertices must equal the world's agent IDs".to_owned(),
            ));
        }
        if self.edges.iter().any(|(source, target)| source == target) {
            return Err(GameError::Topology(
                "network self-loops are not supported".to_owned(),
            ));
        }
        Ok(())
    }

    fn into_dto(self) -> NetworkTopologyDto {
        match self.network {
            NetworkKind::Undirected(network) => NetworkTopologyDto::Undirected(network),
            NetworkKind::Directed(network) => NetworkTopologyDto::Directed(network),
        }
    }
}

/// Edge behavior of a [`GridSpec`].
#[cfg(feature = "grid")]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GridBoundary {
    /// Cells on the border have fewer neighbors; nothing wraps around.
    Fixed,
    /// Opposite edges are joined, so every cell has the same number of neighbors.
    Toroidal,
}

#[cfg(feature = "grid")]
impl From<GridBoundary> for Boundary {
    fn from(value: GridBoundary) -> Self {
        match value {
            GridBoundary::Fixed => Self::Fixed,
            GridBoundary::Toroidal => Self::Toroidal,
        }
    }
}

/// Neighborhood shape of a [`GridSpec`] (radius 1).
#[cfg(feature = "grid")]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GridNeighbors {
    /// The eight surrounding cells, including diagonals.
    Moore,
    /// The four orthogonally adjacent cells.
    VonNeumann,
}

#[cfg(feature = "grid")]
impl From<GridNeighbors> for Neighborhood {
    fn from(value: GridNeighbors) -> Self {
        match value {
            GridNeighbors::Moore => Self::Moore,
            GridNeighbors::VonNeumann => Self::VonNeumann,
        }
    }
}

/// Shape of a grid topology.
///
/// Cell `(r, c)` holds the agent with ID `r * cols + c` (row-major), so a world
/// on this grid must have exactly `rows * cols` agents.
#[cfg(feature = "grid")]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GridSpec {
    /// Number of rows; must be greater than zero.
    pub rows: usize,
    /// Number of columns; must be greater than zero.
    pub cols: usize,
    /// Whether the edges wrap around.
    pub boundary: GridBoundary,
    /// Which surrounding cells count as neighbors.
    pub neighborhood: GridNeighbors,
}

#[cfg(feature = "grid")]
impl GridSpec {
    fn cell_count(self) -> Result<usize, GameError> {
        if self.rows == 0 || self.cols == 0 {
            return Err(GameError::Topology(
                "grid rows and columns must be greater than zero".to_owned(),
            ));
        }
        self.rows
            .checked_mul(self.cols)
            .ok_or_else(|| GameError::Topology("grid cell count overflow".to_owned()))
    }
}

/// A grid topology with a precomputed neighbor table.
///
/// Built by [`Topology::grid`]. Agent `i` occupies the cell with row-major index
/// `i`; neighbor lists are in row-major order and never contain the cell itself.
#[cfg(feature = "grid")]
#[derive(Clone, Debug)]
pub struct GridTopology {
    spec: GridSpec,
    adjacency: Adjacency,
}

#[cfg(feature = "grid")]
impl GridTopology {
    fn new(spec: GridSpec) -> Result<Self, GameError> {
        spec.cell_count()?;
        let grid = Grid::new(spec.rows, spec.cols, spec.boundary.into());
        let adjacency = grid.adjacency(spec.neighborhood.into());
        Ok(Self { spec, adjacency })
    }

    /// Returns the shape this topology was built from.
    pub fn spec(&self) -> GridSpec {
        self.spec
    }

    pub(crate) fn adjacency(&self) -> &Adjacency {
        &self.adjacency
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
enum TopologyDto {
    None,
    #[cfg(feature = "net")]
    Network(NetworkTopologyDto),
    #[cfg(feature = "grid")]
    Grid(GridSpec),
}

#[cfg(feature = "net")]
#[derive(Clone, Debug, Serialize, Deserialize)]
enum NetworkTopologyDto {
    Undirected(SocialNetwork),
    Directed(DiSocialNetwork),
}

impl TryFrom<TopologyDto> for Topology {
    type Error = GameError;

    fn try_from(value: TopologyDto) -> Result<Self, Self::Error> {
        match value {
            TopologyDto::None => Ok(Self::None),
            #[cfg(feature = "net")]
            TopologyDto::Network(NetworkTopologyDto::Undirected(network)) => {
                Ok(Self::network(network))
            }
            #[cfg(feature = "net")]
            TopologyDto::Network(NetworkTopologyDto::Directed(network)) => {
                Ok(Self::directed_network(network))
            }
            #[cfg(feature = "grid")]
            TopologyDto::Grid(spec) => Self::grid(spec),
        }
    }
}

impl From<Topology> for TopologyDto {
    fn from(value: Topology) -> Self {
        match value {
            Topology::None => Self::None,
            #[cfg(feature = "net")]
            Topology::Network(network) => Self::Network(network.into_dto()),
            #[cfg(feature = "grid")]
            Topology::Grid(grid) => Self::Grid(grid.spec),
        }
    }
}
