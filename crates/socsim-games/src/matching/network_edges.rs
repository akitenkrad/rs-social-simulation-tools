use socsim_core::SimRng;

use crate::{
    ArenaWriter, Credit, EncounterBounds, ExperimentState, GameError, ImplicitPattern,
    MatchContext, Matching, MatchingState, StageGame, Topology,
};

/// Plays one two-role encounter per network edge each step.
///
/// Requires a two-role game and a [`Topology::Network`](crate::Topology::Network).
/// Edges are visited in sorted order; an undirected edge seats the smaller ID in
/// role 0, and a directed arc seats its source in role 0. Both endpoints receive
/// their payoff, so an agent plays once per incident edge and may hold
/// different roles within a step.
#[derive(Clone, Copy, Debug, Default)]
pub struct NetworkEdges;

impl NetworkEdges {
    /// Constructs the matching.
    pub fn new() -> Self {
        Self
    }
}

impl<G: StageGame, X: ExperimentState> Matching<G, X> for NetworkEdges {
    fn name(&self) -> &str {
        "network_edges"
    }

    fn check(&self, ctx: &MatchContext<'_, G, X>) -> Result<(), GameError> {
        if ctx.game().n_roles() != 2 {
            return Err(GameError::InvalidMatching(
                "NetworkEdges requires exactly two roles".to_owned(),
            ));
        }
        if !matches!(ctx.topology(), Topology::Network(_)) {
            return Err(GameError::InvalidMatching(
                "NetworkEdges requires a network topology".to_owned(),
            ));
        }
        Ok(())
    }

    fn encounter_bounds(&self, ctx: &MatchContext<'_, G, X>) -> Option<EncounterBounds> {
        let Topology::Network(network) = ctx.topology() else {
            return None;
        };
        let mut counts = vec![0_usize; ctx.ids().len()];
        for &(source, target) in network.edges() {
            *counts.get_mut(usize::try_from(source.0).ok()?)? += 1;
            *counts.get_mut(usize::try_from(target.0).ok()?)? += 1;
        }
        let min = u32::try_from(counts.iter().copied().min().unwrap_or(0)).ok()?;
        let max = u32::try_from(counts.iter().copied().max().unwrap_or(0)).ok()?;
        Some(EncounterBounds { min, max })
    }

    fn allows_multi_role_per_step(&self) -> bool {
        true
    }

    fn implicit(&self) -> Option<ImplicitPattern> {
        Some(ImplicitPattern::NetworkEdges)
    }

    fn encounters_into(
        &self,
        ctx: &MatchContext<'_, G, X>,
        _state: &mut MatchingState,
        _rng: &mut SimRng,
        out: &mut ArenaWriter<'_, G::Action>,
    ) -> socsim_core::Result<()> {
        let Topology::Network(network) = ctx.topology() else {
            return Err(socsim_core::SocsimError::Mechanism(
                "NetworkEdges requires a network topology".to_owned(),
            ));
        };
        for &(source, target) in network.edges() {
            out.push(&[source, target], Credit::All)?;
        }
        Ok(())
    }
}
