use socsim_core::{AgentId, SimRng};

use crate::{
    ArenaWriter, Credit, EncounterBounds, ExperimentState, GameError, ImplicitPattern,
    MatchContext, Matching, MatchingState, StageGame, Topology,
};

/// Lets every grid cell play each of its neighbors each step.
///
/// Requires a two-role game and a [`Topology::Grid`](crate::Topology::Grid). For
/// each cell, the cell sits in role 0 and the neighbor in role 1, and only the
/// center is credited ([`Credit::Center`](crate::Credit::Center)). Each agent
/// therefore receives exactly one payoff per neighbor (plus one for self-play),
/// while appearing as role 1 in its neighbors' encounters.
#[derive(Clone, Copy, Debug, Default)]
pub struct GridNeighborhood {
    include_self: bool,
}

impl GridNeighborhood {
    /// Constructs the matching without self-play.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds an encounter of each cell against itself when `true`.
    ///
    /// The default is `false`. Self-play adds one credited encounter per agent, as
    /// in Nowak and May's spatial prisoner's dilemma.
    pub fn include_self(mut self, include_self: bool) -> Self {
        self.include_self = include_self;
        self
    }
}

impl<G: StageGame, X: ExperimentState> Matching<G, X> for GridNeighborhood {
    fn name(&self) -> &str {
        "grid_neighborhood"
    }

    fn check(&self, ctx: &MatchContext<'_, G, X>) -> Result<(), GameError> {
        if ctx.game().n_roles() != 2 {
            return Err(GameError::InvalidMatching(
                "GridNeighborhood requires exactly two roles".to_owned(),
            ));
        }
        if !matches!(ctx.topology(), Topology::Grid(_)) {
            return Err(GameError::InvalidMatching(
                "GridNeighborhood requires a grid topology".to_owned(),
            ));
        }
        Ok(())
    }

    fn encounter_bounds(&self, ctx: &MatchContext<'_, G, X>) -> Option<EncounterBounds> {
        let Topology::Grid(grid) = ctx.topology() else {
            return None;
        };
        let self_count = usize::from(self.include_self);
        let mut degrees = (0..grid.adjacency().len())
            .map(|index| grid.adjacency().neighbors(index).len() + self_count);
        let first = degrees.next().unwrap_or(self_count);
        let (min, max) = degrees.fold((first, first), |(min, max), degree| {
            (min.min(degree), max.max(degree))
        });
        Some(EncounterBounds {
            min: u32::try_from(min).ok()?,
            max: u32::try_from(max).ok()?,
        })
    }

    fn allows_multi_role_per_step(&self) -> bool {
        true
    }

    fn implicit(&self) -> Option<ImplicitPattern> {
        Some(ImplicitPattern::GridCenter {
            include_self: self.include_self,
        })
    }

    fn encounters_into(
        &self,
        ctx: &MatchContext<'_, G, X>,
        _state: &mut MatchingState,
        _rng: &mut SimRng,
        out: &mut ArenaWriter<'_, G::Action>,
    ) -> socsim_core::Result<()> {
        let Topology::Grid(grid) = ctx.topology() else {
            return Err(socsim_core::SocsimError::Mechanism(
                "GridNeighborhood requires a grid topology".to_owned(),
            ));
        };
        for index in 0..grid.adjacency().len() {
            let center = AgentId(index as u64);
            for &neighbor in grid.adjacency().neighbors(index) {
                out.push(&[center, AgentId(neighbor as u64)], Credit::Center)?;
            }
            if self.include_self {
                out.push(&[center, center], Credit::Center)?;
            }
        }
        Ok(())
    }
}
