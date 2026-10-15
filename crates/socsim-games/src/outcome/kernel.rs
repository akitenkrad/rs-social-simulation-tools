use std::any::Any;

use socsim_core::{AgentId, SocsimError};

use crate::{
    AnonymousGame, Credit, EncounterId, EncounterRef, ExperimentState, ImplicitPattern,
    InternalPlayedActionCounts, OutcomeContext, OutcomeRule, PayoffAggregation, PlayArena,
    StageGame, Topology,
};

pub(crate) struct PayoffAccum<'a> {
    payoff: &'a mut [f64],
    n_played: &'a mut [u32],
    played_counts: Option<&'a mut InternalPlayedActionCounts>,
}

impl<'a> PayoffAccum<'a> {
    pub(crate) fn new(
        payoff: &'a mut [f64],
        n_played: &'a mut [u32],
        played_counts: Option<&'a mut InternalPlayedActionCounts>,
    ) -> Self {
        Self {
            payoff,
            n_played,
            played_counts,
        }
    }

    fn apply<A: 'static>(
        &mut self,
        seats: &[AgentId],
        credit: Credit,
        actions: &[A],
        payoffs: &[f64],
    ) -> socsim_core::Result<()> {
        if let Some(counts) = &mut self.played_counts {
            for (role, action) in actions.iter().enumerate() {
                if let Some(action) = (action as &dyn Any).downcast_ref::<usize>() {
                    counts.increment(role, *action)?;
                }
            }
        }
        let credited_roles = match credit {
            Credit::All => 0..seats.len(),
            Credit::Center => 0..1,
        };
        for role in credited_roles {
            let index = usize::try_from(seats[role].0)
                .map_err(|_| SocsimError::Mechanism("agent index exceeds usize".to_owned()))?;
            let total = self.payoff.get_mut(index).ok_or_else(|| {
                SocsimError::Mechanism(format!("unknown agent {}", seats[role].0))
            })?;
            *total += payoffs[role];
            let count = self.n_played.get_mut(index).ok_or_else(|| {
                SocsimError::Mechanism(format!("unknown agent {}", seats[role].0))
            })?;
            *count = count.checked_add(1).ok_or_else(|| {
                SocsimError::Mechanism(format!("play count overflowed for agent {}", seats[role].0))
            })?;
        }
        Ok(())
    }
}

pub(crate) fn play_pairwise<G, X, O>(
    context: &OutcomeContext<'_, G, X>,
    outcome: &O,
    arena: &mut PlayArena<G::Action>,
    acc: &mut PayoffAccum<'_>,
) -> socsim_core::Result<()>
where
    G: StageGame,
    X: ExperimentState,
    O: OutcomeRule<G, X>,
{
    // Scratch buffers are reused across encounters so that the loop does not
    // allocate per encounter.
    let mut actions = Vec::with_capacity(arena.n_roles);
    let mut payoffs = vec![0.0; arena.n_roles];
    for encounter_index in 0..arena.n_encounters() {
        let start = encounter_index * arena.n_roles;
        let end = start + arena.n_roles;
        fill_actions(&mut actions, &arena.actions[start..end])?;
        let credit = arena.credit[encounter_index];
        let seats = &arena.seats[start..end];
        let encounter = EncounterRef {
            id: EncounterId(encounter_index as u32),
            seats,
            credit,
        };
        payoffs.fill(0.0);
        outcome.outcome(context, encounter, &actions, &mut payoffs)?;
        arena.payoffs[start..end].copy_from_slice(&payoffs);
        acc.apply(seats, credit, &actions, &payoffs)?;
    }
    Ok(())
}

/// Copies one encounter's filled action seats into `out`, reusing its buffer.
pub(crate) fn fill_actions<A: Clone>(
    out: &mut Vec<A>,
    seats: &[Option<A>],
) -> socsim_core::Result<()> {
    out.clear();
    for action in seats {
        out.push(
            action
                .clone()
                .ok_or_else(|| SocsimError::Mechanism("unfilled action seat".to_owned()))?,
        );
    }
    Ok(())
}

/// Reusable buffers of the counting kernel.
struct CountingScratch {
    counts: Vec<usize>,
    by_action: Vec<f64>,
    payoffs: Vec<f64>,
}

impl CountingScratch {
    fn new(n_actions: usize) -> Self {
        Self {
            counts: vec![0; n_actions],
            by_action: vec![0.0; n_actions],
            payoffs: Vec::new(),
        }
    }

    /// Fills `self.payoffs` with the payoff of every seat of one encounter.
    fn compute<G: AnonymousGame>(
        &mut self,
        game: &G,
        actions: &[usize],
    ) -> socsim_core::Result<()> {
        let n_actions = game.n_actions(0);
        self.counts.clear();
        self.counts.resize(n_actions, 0);
        for &action in actions {
            let count = self.counts.get_mut(action).ok_or_else(|| {
                SocsimError::Mechanism(format!("counting kernel found invalid action {action}"))
            })?;
            *count += 1;
        }
        self.by_action.clear();
        self.by_action.resize(n_actions, 0.0);
        for action in 0..n_actions {
            if self.counts[action] == 0 {
                continue;
            }
            self.counts[action] -= 1;
            self.by_action[action] = game.payoff_by_counts(action, &self.counts);
            self.counts[action] += 1;
        }
        self.payoffs.clear();
        self.payoffs
            .extend(actions.iter().map(|&action| self.by_action[action]));
        Ok(())
    }
}

pub(crate) fn play_counting<G: AnonymousGame>(
    game: &G,
    arena: &mut PlayArena<usize>,
    acc: &mut PayoffAccum<'_>,
) -> socsim_core::Result<()> {
    let mut scratch = CountingScratch::new(game.n_actions(0));
    let mut actions = Vec::with_capacity(arena.n_roles);
    for encounter_index in 0..arena.n_encounters() {
        let start = encounter_index * arena.n_roles;
        let end = start + arena.n_roles;
        fill_actions(&mut actions, &arena.actions[start..end])?;
        scratch.compute(game, &actions)?;
        arena.payoffs[start..end].copy_from_slice(&scratch.payoffs);
        acc.apply(
            &arena.seats[start..end],
            arena.credit[encounter_index],
            &actions,
            &scratch.payoffs,
        )?;
    }
    Ok(())
}

pub(crate) fn for_each_implicit<A, F>(
    pattern: ImplicitPattern,
    _topology: &Topology,
    ids: &[AgentId],
    held: &[A],
    mut visit: F,
) -> socsim_core::Result<()>
where
    A: Clone,
    F: FnMut(u32, &[AgentId], Credit, &[A]) -> socsim_core::Result<()>,
{
    let mut id = 0_u32;
    let mut actions: Vec<A> = Vec::new();
    let mut emit = |seats: &[AgentId], credit: Credit| -> socsim_core::Result<()> {
        actions.clear();
        for agent in seats {
            actions.push(
                held.get(agent.0 as usize)
                    .cloned()
                    .ok_or_else(|| SocsimError::Mechanism(format!("unknown agent {}", agent.0)))?,
            );
        }
        visit(id, seats, credit, &actions)?;
        id = id
            .checked_add(1)
            .ok_or_else(|| SocsimError::Mechanism("encounter count exceeds u32::MAX".to_owned()))?;
        Ok(())
    };
    match pattern {
        #[cfg(feature = "grid")]
        ImplicitPattern::GridCenter { include_self } => {
            let Topology::Grid(grid) = _topology else {
                return Err(SocsimError::Mechanism(
                    "implicit grid matching requires a grid topology".to_owned(),
                ));
            };
            for index in 0..grid.adjacency().len() {
                let center = AgentId(index as u64);
                for &neighbor in grid.adjacency().neighbors(index) {
                    emit(&[center, AgentId(neighbor as u64)], Credit::Center)?;
                }
                if include_self {
                    emit(&[center, center], Credit::Center)?;
                }
            }
        }
        #[cfg(not(feature = "grid"))]
        ImplicitPattern::GridCenter { .. } => {
            return Err(SocsimError::Mechanism(
                "implicit grid matching requires the grid feature".to_owned(),
            ));
        }
        #[cfg(feature = "net")]
        ImplicitPattern::NetworkEdges => {
            let Topology::Network(network) = _topology else {
                return Err(SocsimError::Mechanism(
                    "implicit network matching requires a network topology".to_owned(),
                ));
            };
            for &(source, target) in network.edges() {
                emit(&[source, target], Credit::All)?;
            }
        }
        #[cfg(not(feature = "net"))]
        ImplicitPattern::NetworkEdges => {
            return Err(SocsimError::Mechanism(
                "implicit network matching requires the net feature".to_owned(),
            ));
        }
        ImplicitPattern::WholePopulation => {
            let mut seats = ids.to_vec();
            seats.sort_unstable();
            emit(&seats, Credit::All)?;
        }
    }
    Ok(())
}

#[derive(Debug, Default)]
pub(crate) struct LocalEncounterIds {
    #[cfg(feature = "grid")]
    grid_starts: Option<Vec<u32>>,
}

impl LocalEncounterIds {
    pub(crate) fn new(pattern: ImplicitPattern, _topology: &Topology) -> socsim_core::Result<Self> {
        match pattern {
            #[cfg(feature = "grid")]
            ImplicitPattern::GridCenter { include_self } => {
                let Topology::Grid(grid) = _topology else {
                    return Err(SocsimError::Mechanism(
                        "implicit grid matching requires a grid topology".to_owned(),
                    ));
                };
                let mut starts = Vec::with_capacity(grid.adjacency().len());
                let mut next = 0_u32;
                for index in 0..grid.adjacency().len() {
                    starts.push(next);
                    let count = grid
                        .adjacency()
                        .neighbors(index)
                        .len()
                        .checked_add(usize::from(include_self))
                        .ok_or_else(|| {
                            SocsimError::Mechanism("encounter count exceeds usize::MAX".to_owned())
                        })?;
                    let count = u32::try_from(count).map_err(|_| {
                        SocsimError::Mechanism("encounter count exceeds u32::MAX".to_owned())
                    })?;
                    next = next.checked_add(count).ok_or_else(|| {
                        SocsimError::Mechanism("encounter count exceeds u32::MAX".to_owned())
                    })?;
                }
                Ok(Self {
                    grid_starts: Some(starts),
                })
            }
            #[cfg(not(feature = "grid"))]
            ImplicitPattern::GridCenter { .. } => Err(SocsimError::Mechanism(
                "implicit grid matching requires the grid feature".to_owned(),
            )),
            _ => Ok(Self::default()),
        }
    }

    #[cfg(feature = "grid")]
    fn grid_start(&self, index: usize) -> socsim_core::Result<u32> {
        self.grid_starts
            .as_ref()
            .and_then(|starts| starts.get(index))
            .copied()
            .ok_or_else(|| {
                SocsimError::Mechanism(format!(
                    "local encounter id is not initialized for agent {index}"
                ))
            })
    }
}

fn for_each_local_implicit<A, F>(
    pattern: ImplicitPattern,
    _topology: &Topology,
    ids: &[AgentId],
    held: &[A],
    _encounter_ids: &LocalEncounterIds,
    index: usize,
    mut visit: F,
) -> socsim_core::Result<()>
where
    A: Clone,
    F: FnMut(u32, &[AgentId], Credit, &[A], usize) -> socsim_core::Result<()>,
{
    let agent = AgentId(index as u64);
    let mut actions = Vec::new();
    let mut emit = |id: u32,
                    seats: &[AgentId],
                    credit: Credit,
                    credited_role: usize|
     -> socsim_core::Result<()> {
        actions.clear();
        for seat in seats {
            actions.push(
                held.get(seat.0 as usize)
                    .cloned()
                    .ok_or_else(|| SocsimError::Mechanism(format!("unknown agent {}", seat.0)))?,
            );
        }
        visit(id, seats, credit, &actions, credited_role)
    };
    match pattern {
        #[cfg(feature = "grid")]
        ImplicitPattern::GridCenter { include_self } => {
            let Topology::Grid(grid) = _topology else {
                return Err(SocsimError::Mechanism(
                    "implicit grid matching requires a grid topology".to_owned(),
                ));
            };
            let Some(neighbors) =
                (index < grid.adjacency().len()).then(|| grid.adjacency().neighbors(index))
            else {
                return Err(SocsimError::Mechanism(format!("unknown agent {}", agent.0)));
            };
            let mut id = _encounter_ids.grid_start(index)?;
            for &neighbor in neighbors {
                emit(id, &[agent, AgentId(neighbor as u64)], Credit::Center, 0)?;
                id = id.checked_add(1).ok_or_else(|| {
                    SocsimError::Mechanism("encounter count exceeds u32::MAX".to_owned())
                })?;
            }
            if include_self {
                emit(id, &[agent, agent], Credit::Center, 0)?;
            }
        }
        #[cfg(not(feature = "grid"))]
        ImplicitPattern::GridCenter { .. } => {
            return Err(SocsimError::Mechanism(
                "implicit grid matching requires the grid feature".to_owned(),
            ));
        }
        #[cfg(feature = "net")]
        ImplicitPattern::NetworkEdges => {
            let Topology::Network(network) = _topology else {
                return Err(SocsimError::Mechanism(
                    "implicit network matching requires a network topology".to_owned(),
                ));
            };
            for &edge_index in network.incident_edges(agent) {
                let &(source, target) = network.edges().get(edge_index).ok_or_else(|| {
                    SocsimError::Mechanism("incident edge index is out of bounds".to_owned())
                })?;
                let role = usize::from(target == agent);
                emit(
                    u32::try_from(edge_index).map_err(|_| {
                        SocsimError::Mechanism("encounter count exceeds u32::MAX".to_owned())
                    })?,
                    &[source, target],
                    Credit::All,
                    role,
                )?;
            }
        }
        #[cfg(not(feature = "net"))]
        ImplicitPattern::NetworkEdges => {
            return Err(SocsimError::Mechanism(
                "implicit network matching requires the net feature".to_owned(),
            ));
        }
        ImplicitPattern::WholePopulation => {
            let mut seats = ids.to_vec();
            seats.sort_unstable();
            let role = seats
                .iter()
                .position(|&seat| seat == agent)
                .ok_or_else(|| SocsimError::Mechanism(format!("unknown agent {}", agent.0)))?;
            emit(0, &seats, Credit::All, role)?;
        }
    }
    Ok(())
}

pub(crate) fn aggregate_mean(aggregation: PayoffAggregation, payoff: &mut [f64], n_played: &[u32]) {
    if aggregation == PayoffAggregation::Mean {
        for (payoff, &count) in payoff.iter_mut().zip(n_played) {
            if count != 0 {
                *payoff /= f64::from(count);
            }
        }
    }
}

pub(crate) fn local_payoff_pairwise<G, X, O>(
    pattern: ImplicitPattern,
    context: &OutcomeContext<'_, G, X>,
    ids: &[AgentId],
    outcome: &O,
    encounter_ids: &LocalEncounterIds,
    index: usize,
    aggregation: PayoffAggregation,
) -> socsim_core::Result<(f64, u32)>
where
    G: StageGame,
    X: ExperimentState,
    O: OutcomeRule<G, X>,
{
    let mut total = 0.0;
    let mut count = 0_u32;
    let mut payoffs = Vec::new();
    for_each_local_implicit(
        pattern,
        context.topology(),
        ids,
        context.held,
        encounter_ids,
        index,
        |id, seats, credit, actions, role| {
            payoffs.clear();
            payoffs.resize(seats.len(), 0.0);
            outcome.outcome(
                context,
                EncounterRef {
                    id: EncounterId(id),
                    seats,
                    credit,
                },
                actions,
                &mut payoffs,
            )?;
            total += payoffs[role];
            count = count.checked_add(1).ok_or_else(|| {
                SocsimError::Mechanism(format!("play count overflowed for agent {index}"))
            })?;
            Ok(())
        },
    )?;
    if aggregation == PayoffAggregation::Mean && count != 0 {
        total /= f64::from(count);
    }
    Ok((total, count))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn local_payoff_counting<G: AnonymousGame>(
    pattern: ImplicitPattern,
    game: &G,
    topology: &Topology,
    ids: &[AgentId],
    held: &[usize],
    encounter_ids: &LocalEncounterIds,
    index: usize,
    aggregation: PayoffAggregation,
) -> socsim_core::Result<(f64, u32)> {
    let mut total = 0.0;
    let mut count = 0_u32;
    let mut scratch = CountingScratch::new(game.n_actions(0));
    for_each_local_implicit(
        pattern,
        topology,
        ids,
        held,
        encounter_ids,
        index,
        |_id, _seats, _credit, actions, role| {
            scratch.compute(game, actions)?;
            total += scratch.payoffs[role];
            count = count.checked_add(1).ok_or_else(|| {
                SocsimError::Mechanism(format!("play count overflowed for agent {index}"))
            })?;
            Ok(())
        },
    )?;
    if aggregation == PayoffAggregation::Mean && count != 0 {
        total /= f64::from(count);
    }
    Ok((total, count))
}

pub(crate) fn play_implicit_pairwise<G, X, O>(
    pattern: ImplicitPattern,
    context: &OutcomeContext<'_, G, X>,
    ids: &[AgentId],
    outcome: &O,
    acc: &mut PayoffAccum<'_>,
) -> socsim_core::Result<()>
where
    G: StageGame,
    X: ExperimentState,
    O: OutcomeRule<G, X>,
{
    let mut payoffs = Vec::new();
    for_each_implicit(
        pattern,
        context.topology(),
        ids,
        context.held,
        |id, seats, credit, actions| {
            let encounter = EncounterRef {
                id: EncounterId(id),
                seats,
                credit,
            };
            payoffs.clear();
            payoffs.resize(seats.len(), 0.0);
            outcome.outcome(context, encounter, actions, &mut payoffs)?;
            acc.apply(seats, credit, actions, &payoffs)
        },
    )
}

pub(crate) fn play_implicit_counting<G: AnonymousGame>(
    pattern: ImplicitPattern,
    game: &G,
    topology: &Topology,
    ids: &[AgentId],
    held: &[usize],
    acc: &mut PayoffAccum<'_>,
) -> socsim_core::Result<()> {
    let mut scratch = CountingScratch::new(game.n_actions(0));
    for_each_implicit(
        pattern,
        topology,
        ids,
        held,
        |_id, seats, credit, actions| {
            scratch.compute(game, actions)?;
            acc.apply(seats, credit, actions, &scratch.payoffs)
        },
    )
}

#[cfg(all(test, feature = "grid", feature = "net"))]
mod tests {
    use rand::Rng;
    use socsim_core::{AgentId, SimRng};
    use socsim_net::{DiSocialNetwork, SocialNetwork};

    use super::*;
    use crate::catalog::PublicGoodsFinite;
    use crate::{
        GridBoundary, GridNeighbors, GridSpec, NormalFormOutcome, PayoffAggregation, Topology,
    };

    fn random_held(n_agents: usize, seed: u64) -> Vec<usize> {
        let mut rng = SimRng::from_seed(seed);
        (0..n_agents).map(|_| rng.gen_range(0..2)).collect()
    }

    fn assert_local_matches_full(
        game: &PublicGoodsFinite,
        topology: &Topology,
        pattern: ImplicitPattern,
        held: &[usize],
    ) {
        let ids: Vec<_> = (0..held.len()).map(|index| AgentId(index as u64)).collect();
        let x = ();
        let context = OutcomeContext {
            t: 1,
            game,
            x: &x,
            held,
            topology,
        };
        let encounter_ids = LocalEncounterIds::new(pattern, topology).unwrap();

        for aggregation in [PayoffAggregation::Sum, PayoffAggregation::Mean] {
            let mut pairwise_payoff = vec![0.0; held.len()];
            let mut pairwise_played = vec![0; held.len()];
            play_implicit_pairwise(
                pattern,
                &context,
                &ids,
                &NormalFormOutcome,
                &mut PayoffAccum::new(&mut pairwise_payoff, &mut pairwise_played, None),
            )
            .unwrap();
            aggregate_mean(aggregation, &mut pairwise_payoff, &pairwise_played);

            let mut counting_payoff = vec![0.0; held.len()];
            let mut counting_played = vec![0; held.len()];
            play_implicit_counting(
                pattern,
                game,
                topology,
                &ids,
                held,
                &mut PayoffAccum::new(&mut counting_payoff, &mut counting_played, None),
            )
            .unwrap();
            aggregate_mean(aggregation, &mut counting_payoff, &counting_played);

            for index in 0..held.len() {
                assert_eq!(
                    local_payoff_pairwise(
                        pattern,
                        &context,
                        &ids,
                        &NormalFormOutcome,
                        &encounter_ids,
                        index,
                        aggregation,
                    )
                    .unwrap(),
                    (pairwise_payoff[index], pairwise_played[index]),
                    "pairwise agent {index}, aggregation {aggregation:?}"
                );
                assert_eq!(
                    local_payoff_counting(
                        pattern,
                        game,
                        topology,
                        &ids,
                        held,
                        &encounter_ids,
                        index,
                        aggregation,
                    )
                    .unwrap(),
                    (counting_payoff[index], counting_played[index]),
                    "counting agent {index}, aggregation {aggregation:?}"
                );
            }
        }
    }

    fn assert_grid(boundary: GridBoundary, neighborhood: GridNeighbors, include_self: bool) {
        let rows = 4;
        let cols = 5;
        let topology = Topology::grid(GridSpec {
            rows,
            cols,
            boundary,
            neighborhood,
        })
        .unwrap();
        let held = random_held(rows * cols, 17);
        assert_local_matches_full(
            &PublicGoodsFinite::new(2, 1.7, 0.9).unwrap(),
            &topology,
            ImplicitPattern::GridCenter { include_self },
            &held,
        );
    }

    #[test]
    fn local_payoff_matches_full_for_fixed_moore_grid_with_self_play() {
        assert_grid(GridBoundary::Fixed, GridNeighbors::Moore, true);
    }

    #[test]
    fn local_payoff_matches_full_for_toroidal_moore_grid_with_self_play() {
        assert_grid(GridBoundary::Toroidal, GridNeighbors::Moore, true);
    }

    #[test]
    fn local_payoff_matches_full_for_von_neumann_grid_without_self_play() {
        assert_grid(GridBoundary::Fixed, GridNeighbors::VonNeumann, false);
    }

    #[test]
    fn local_payoff_matches_full_for_undirected_network() {
        let n_agents = 7;
        let mut network = SocialNetwork::empty();
        for index in 0..n_agents {
            network.add_node(AgentId(index as u64));
        }
        for (source, target) in [(4, 1), (0, 4), (6, 2), (1, 3), (5, 0), (2, 5)] {
            network.add_edge(AgentId(source), AgentId(target));
        }
        assert_local_matches_full(
            &PublicGoodsFinite::new(2, 1.7, 0.9).unwrap(),
            &Topology::network(network),
            ImplicitPattern::NetworkEdges,
            &random_held(n_agents, 23),
        );
    }

    #[test]
    fn local_payoff_matches_full_for_directed_network_including_incoming_arcs() {
        let n_agents = 7;
        let mut network = DiSocialNetwork::empty();
        for index in 0..n_agents {
            network.add_node(AgentId(index as u64));
        }
        for (source, target) in [(4, 1), (0, 4), (6, 2), (1, 3), (5, 0), (2, 0)] {
            network.add_edge(AgentId(source), AgentId(target));
        }
        assert_local_matches_full(
            &PublicGoodsFinite::new(2, 1.7, 0.9).unwrap(),
            &Topology::directed_network(network),
            ImplicitPattern::NetworkEdges,
            &random_held(n_agents, 29),
        );
    }

    #[test]
    fn local_payoff_matches_full_for_whole_population() {
        let n_agents = 8;
        assert_local_matches_full(
            &PublicGoodsFinite::new(n_agents, 3.0, 1.0).unwrap(),
            &Topology::None,
            ImplicitPattern::WholePopulation,
            &random_held(n_agents, 31),
        );
    }

    #[test]
    fn grid_local_encounter_ids_match_full_traversal() {
        let topology = Topology::grid(GridSpec {
            rows: 4,
            cols: 5,
            boundary: GridBoundary::Fixed,
            neighborhood: GridNeighbors::Moore,
        })
        .unwrap();
        let pattern = ImplicitPattern::GridCenter { include_self: true };
        let held = random_held(20, 37);
        let ids: Vec<_> = (0..held.len()).map(|index| AgentId(index as u64)).collect();
        let encounter_ids = LocalEncounterIds::new(pattern, &topology).unwrap();

        let mut full = vec![Vec::new(); held.len()];
        for_each_implicit(
            pattern,
            &topology,
            &ids,
            &held,
            |id, seats, _credit, _actions| {
                full[seats[0].0 as usize].push(id);
                Ok(())
            },
        )
        .unwrap();

        for (index, expected) in full.iter().enumerate() {
            let mut local = Vec::new();
            for_each_local_implicit(
                pattern,
                &topology,
                &ids,
                &held,
                &encounter_ids,
                index,
                |id, _seats, _credit, _actions, _role| {
                    local.push(id);
                    Ok(())
                },
            )
            .unwrap();
            assert_eq!(&local, expected, "agent {index}");
        }
    }
}
