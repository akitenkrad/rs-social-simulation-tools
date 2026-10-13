//! Reading `[[mechanism]]`, and the parts chosen at run time.
//!
//! A scenario chooses the matching, the play path, and the population dynamic
//! by name, so their types are not known at compile time. [`AnyMatching`],
//! [`AnyPlay`], and [`AnyDynamics`] wrap the built-in parts in one enum each.
//! Every method of the wrapped part is forwarded, including the ones with a
//! default body, so a part chosen here consumes the same random numbers and
//! selects the same play path (materialized or implicit) as the same part
//! chosen by type.

use std::convert::Infallible;

use socsim_config::MechanismEntry;
use socsim_core::{Mechanism, SimRng};

use super::game::{read_update, sealed::SpecGame};
use super::reader::{config_error, quoted, ParamReader};
use super::rules::{RuleRegistry, DECIDE_PATH};
use crate::assemble::sealed::{DynamicsBuild, DynamicsCheck, PlayBuild};
use crate::mechanism::{AggregateCollector, KernelRunner, PayoffRangeSource};
use crate::{
    AggregateGame, ArenaWriter, DecisionScope, EncounterBounds, ExperimentState, FermiUpdate,
    Fitness, FixedPlayers, GameError, GameWorld, ImitateBest, ImitationTie, ImplicitPattern,
    MatchContext, Matching, MatchingState, MoranUpdate, NoDynamics, NormalFormOutcome, Pairwise,
    PayoffAggregation, PlayOptions, RandomGroups, RandomPairs, RoundRobin, StageGame,
    WholePopulation, WithAggregates,
};

/// A built-in matching chosen at run time.
#[derive(Clone, Debug)]
pub(crate) enum AnyMatching {
    FixedPlayers(FixedPlayers),
    RoundRobin(RoundRobin),
    RandomPairs(RandomPairs),
    RandomGroups(RandomGroups),
    WholePopulation(WholePopulation),
    #[cfg(feature = "grid")]
    GridNeighborhood(crate::GridNeighborhood),
    #[cfg(feature = "net")]
    NetworkEdges(crate::NetworkEdges),
}

macro_rules! any_matching {
    ($matching:expr, $inner:ident => $body:expr) => {
        match $matching {
            AnyMatching::FixedPlayers($inner) => $body,
            AnyMatching::RoundRobin($inner) => $body,
            AnyMatching::RandomPairs($inner) => $body,
            AnyMatching::RandomGroups($inner) => $body,
            AnyMatching::WholePopulation($inner) => $body,
            #[cfg(feature = "grid")]
            AnyMatching::GridNeighborhood($inner) => $body,
            #[cfg(feature = "net")]
            AnyMatching::NetworkEdges($inner) => $body,
        }
    };
}

impl<G: StageGame, X: ExperimentState> Matching<G, X> for AnyMatching {
    fn name(&self) -> &str {
        any_matching!(self, inner => Matching::<G, X>::name(inner))
    }

    fn check(&self, ctx: &MatchContext<'_, G, X>) -> Result<(), GameError> {
        any_matching!(self, inner => inner.check(ctx))
    }

    fn supports_roles(&self, n_roles: usize) -> bool {
        any_matching!(self, inner => Matching::<G, X>::supports_roles(inner, n_roles))
    }

    fn encounter_bounds(&self, ctx: &MatchContext<'_, G, X>) -> Option<EncounterBounds> {
        any_matching!(self, inner => inner.encounter_bounds(ctx))
    }

    fn allows_multi_role_per_step(&self) -> bool {
        any_matching!(self, inner => Matching::<G, X>::allows_multi_role_per_step(inner))
    }

    fn implicit(&self) -> Option<ImplicitPattern> {
        any_matching!(self, inner => Matching::<G, X>::implicit(inner))
    }

    fn init_state(&self, ctx: &MatchContext<'_, G, X>) -> MatchingState {
        any_matching!(self, inner => inner.init_state(ctx))
    }

    fn encounters_into(
        &self,
        ctx: &MatchContext<'_, G, X>,
        state: &mut MatchingState,
        rng: &mut SimRng,
        out: &mut ArenaWriter<'_, G::Action>,
    ) -> socsim_core::Result<()> {
        any_matching!(self, inner => inner.encounters_into(ctx, state, rng, out))
    }
}

/// A play path chosen at run time: pairwise play with the normal-form
/// outcome, with or without public aggregates.
#[derive(Clone, Debug)]
pub(crate) enum AnyPlay {
    Pairwise(Pairwise<NormalFormOutcome>),
    WithAggregates(WithAggregates<Pairwise<NormalFormOutcome>>),
}

impl AnyPlay {
    /// The play path that records aggregates when `aggregate_history_len` is
    /// set.
    pub(crate) fn new(aggregate_history_len: Option<usize>) -> Self {
        let pairwise = Pairwise::new(NormalFormOutcome);
        match aggregate_history_len {
            Some(len) => Self::WithAggregates(pairwise.with_aggregates(len)),
            None => Self::Pairwise(pairwise),
        }
    }
}

impl<G: AggregateGame, X: ExperimentState> PlayBuild<G, X> for AnyPlay {
    fn permits_implicit(&self) -> bool {
        match self {
            Self::Pairwise(inner) => PlayBuild::<G, X>::permits_implicit(inner),
            Self::WithAggregates(inner) => PlayBuild::<G, X>::permits_implicit(inner),
        }
    }

    fn prepare(
        &self,
        world: &mut GameWorld<G, X>,
    ) -> Result<Option<AggregateCollector<G>>, GameError> {
        match self {
            Self::Pairwise(inner) => inner.prepare(world),
            Self::WithAggregates(inner) => inner.prepare(world),
        }
    }

    fn into_runner(self) -> Box<dyn KernelRunner<G, X>> {
        match self {
            Self::Pairwise(inner) => inner.into_runner(),
            Self::WithAggregates(inner) => inner.into_runner(),
        }
    }

    fn payoff_source(&self) -> PayoffRangeSource<G> {
        match self {
            Self::Pairwise(inner) => PlayBuild::<G, X>::payoff_source(inner),
            Self::WithAggregates(inner) => PlayBuild::<G, X>::payoff_source(inner),
        }
    }
}

/// The `replicator_update` slot of [`AnyDynamics`]: the dynamic itself for a
/// finite game, and [`Infallible`] for a game without it.
pub(crate) trait ReplicatorSlot<G: StageGame, X: ExperimentState>:
    Clone + std::fmt::Debug + Send + Sync + 'static
{
    fn prepare(&self, check: &DynamicsCheck<'_, G, X>) -> Result<(), GameError>;
    fn into_mechanism(self) -> Option<Box<dyn Mechanism<GameWorld<G, X>>>>;
}

impl<G, X> ReplicatorSlot<G, X> for crate::ProportionalImitation
where
    G: StageGame,
    X: ExperimentState,
    crate::ProportionalImitation: DynamicsBuild<G, X>,
{
    fn prepare(&self, check: &DynamicsCheck<'_, G, X>) -> Result<(), GameError> {
        DynamicsBuild::prepare(self, check)
    }

    fn into_mechanism(self) -> Option<Box<dyn Mechanism<GameWorld<G, X>>>> {
        DynamicsBuild::into_mechanism(self)
    }
}

impl<G: StageGame, X: ExperimentState> ReplicatorSlot<G, X> for Infallible {
    fn prepare(&self, _check: &DynamicsCheck<'_, G, X>) -> Result<(), GameError> {
        match *self {}
    }

    fn into_mechanism(self) -> Option<Box<dyn Mechanism<GameWorld<G, X>>>> {
        match self {}
    }
}

/// A population dynamic chosen at run time. `R` is the `replicator_update`
/// slot of the game (see [`ReplicatorSlot`]).
#[derive(Clone, Debug)]
pub(crate) enum AnyDynamics<R> {
    None(NoDynamics),
    ImitateBest(ImitateBest),
    Fermi(FermiUpdate),
    Replicator(R),
    Moran(MoranUpdate),
}

impl<G, X, R> DynamicsBuild<G, X> for AnyDynamics<R>
where
    G: StageGame,
    X: ExperimentState,
    ImitateBest: DynamicsBuild<G, X>,
    FermiUpdate: DynamicsBuild<G, X>,
    MoranUpdate: DynamicsBuild<G, X>,
    R: ReplicatorSlot<G, X>,
{
    fn prepare(&self, check: &DynamicsCheck<'_, G, X>) -> Result<(), GameError> {
        match self {
            Self::None(inner) => DynamicsBuild::<G, X>::prepare(inner, check),
            Self::ImitateBest(inner) => DynamicsBuild::<G, X>::prepare(inner, check),
            Self::Fermi(inner) => DynamicsBuild::<G, X>::prepare(inner, check),
            Self::Replicator(inner) => ReplicatorSlot::<G, X>::prepare(inner, check),
            Self::Moran(inner) => DynamicsBuild::<G, X>::prepare(inner, check),
        }
    }

    fn into_mechanism(self) -> Option<Box<dyn Mechanism<GameWorld<G, X>>>> {
        match self {
            Self::None(inner) => DynamicsBuild::<G, X>::into_mechanism(inner),
            Self::ImitateBest(inner) => DynamicsBuild::<G, X>::into_mechanism(inner),
            Self::Fermi(inner) => DynamicsBuild::<G, X>::into_mechanism(inner),
            Self::Replicator(inner) => ReplicatorSlot::<G, X>::into_mechanism(inner),
            Self::Moran(inner) => DynamicsBuild::<G, X>::into_mechanism(inner),
        }
    }
}

/// The mechanism names a scenario may list.
const MECHANISMS: &[&str] = &[
    "game_match",
    "game_decide",
    "game_play",
    "imitate_best",
    "fermi_update",
    "replicator_update",
    "moran_update",
    "game_metrics",
];

const DYNAMICS: &[&str] = &[
    "imitate_best",
    "fermi_update",
    "replicator_update",
    "moran_update",
];

const MATCHINGS: &[&str] = &[
    "fixed_players",
    "round_robin",
    "random_pairs",
    "random_groups",
    "whole_population",
    "grid_neighborhood",
    "network_edges",
];

/// The keys of `game_decide` that are not rule parameters.
const DECIDE_KEYS: &[&str] = &["decision_scope", "report_mixed"];

/// What the `[[mechanism]]` entries of a scenario configure.
pub(crate) struct MechanismConfig<G: SpecGame> {
    pub(crate) matching: AnyMatching,
    pub(crate) play_options: PlayOptions<G::Action>,
    pub(crate) aggregate_history_len: Option<usize>,
    pub(crate) dynamics: AnyDynamics<G::Replicator>,
    pub(crate) metrics: Option<G::Metrics>,
    /// The parameters of `game_decide`, passed to the rule constructors.
    pub(crate) decide: toml::Table,
}

fn entry_path(name: &str) -> String {
    format!("mechanism[{name}]")
}

/// Reads the `[[mechanism]]` entries.
///
/// The entries `game_match`, `game_decide`, and `game_play` are required; at
/// most one population dynamic and at most one `game_metrics` may follow. A
/// population dynamic must be listed after `game_decide`. The order of the
/// entries does not change the order of the assembled mechanisms.
pub(crate) fn read_mechanisms<G: SpecGame, X: ExperimentState>(
    entries: &[MechanismEntry],
    registry: &RuleRegistry<G, X>,
) -> Result<MechanismConfig<G>, GameError> {
    let mut seen: Vec<&str> = Vec::new();
    for entry in entries {
        let name = entry.name.as_str();
        if !MECHANISMS.contains(&name) {
            return Err(config_error(
                &entry_path(name),
                format!("unknown mechanism (expected one of {})", quoted(MECHANISMS)),
            ));
        }
        if seen.contains(&name) {
            return Err(config_error(&entry_path(name), "listed more than once"));
        }
        if DYNAMICS.contains(&name) {
            if let Some(other) = seen.iter().find(|seen| DYNAMICS.contains(seen)) {
                return Err(config_error(
                    &entry_path(name),
                    format!("at most one population dynamic may be listed, found {other} too"),
                ));
            }
        }
        seen.push(name);
    }
    let position = |name: &str| seen.iter().position(|seen| *seen == name);
    let table = |name: &str| -> Result<&toml::Table, GameError> {
        entries
            .iter()
            .find(|entry| entry.name == name)
            .map(|entry| &entry.params.0)
            .ok_or_else(|| config_error(&entry_path(name), "this mechanism is required"))
    };
    let match_table = table("game_match")?;
    let decide = table("game_decide")?;
    let play_table = table("game_play")?;
    if let (Some(dynamic), Some(decide_at)) = (
        seen.iter().position(|seen| DYNAMICS.contains(seen)),
        position("game_decide"),
    ) {
        if dynamic < decide_at {
            return Err(config_error(
                &entry_path(seen[dynamic]),
                "a population dynamic must be listed after game_decide",
            ));
        }
    }
    let reader = |name: &str| {
        entries
            .iter()
            .find(|entry| entry.name == name)
            .map(|entry| ParamReader::new(&entry.params.0, entry_path(name)))
    };

    let matching = read_matching(ParamReader::new(match_table, entry_path("game_match")))?;

    let mut decide_reader = ParamReader::new(decide, DECIDE_PATH);
    let scope = match decide_reader.choice(
        "decision_scope",
        &["per_step", "per_agent_role", "per_encounter"],
    )? {
        Some("per_agent_role") => DecisionScope::PerAgentRole,
        Some("per_encounter") => DecisionScope::PerEncounter,
        _ => DecisionScope::PerStep,
    };
    let report_mixed = decide_reader.flag("report_mixed")?.unwrap_or(false);
    decide_reader.accept(DECIDE_KEYS.iter().copied());
    decide_reader.accept(registry.keys());
    decide_reader.finish()?;

    let mut play_reader = ParamReader::new(play_table, entry_path("game_play"));
    let aggregation = match play_reader.choice("payoff_aggregation", &["sum", "mean"])? {
        Some("mean") => PayoffAggregation::Mean,
        _ => PayoffAggregation::Sum,
    };
    let aggregate_history_len = play_reader.count_usize("aggregate_history_len")?;
    let materialize = play_reader.flag("materialize")?.unwrap_or(false);
    play_reader.finish()?;

    let mut dynamics = AnyDynamics::None(NoDynamics);
    for name in DYNAMICS {
        let Some(mut dynamic_reader) = reader(name) else {
            continue;
        };
        dynamics = read_dynamics::<G>(name, &mut dynamic_reader)?;
        dynamic_reader.finish()?;
    }

    let metrics = match reader("game_metrics") {
        Some(mut metrics_reader) => {
            let metrics = G::read_metrics(&mut metrics_reader)?;
            metrics_reader.finish()?;
            Some(metrics)
        }
        None => None,
    };

    Ok(MechanismConfig {
        matching,
        play_options: PlayOptions::new()
            .payoff_aggregation(aggregation)
            .decision_scope(scope)
            .report_mixed(report_mixed)
            .materialize(materialize),
        aggregate_history_len,
        dynamics,
        metrics,
        decide: decide.clone(),
    })
}

/// The matching that each optional key of `game_match` belongs to.
const MATCHING_KEYS: &[(&str, &str)] = &[
    ("pairs", "fixed_players"),
    ("both_orders", "round_robin"),
    ("k", "random_groups"),
    ("include_self", "grid_neighborhood"),
];

fn read_matching(mut reader: ParamReader<'_>) -> Result<AnyMatching, GameError> {
    let kind = reader.require_choice("matching", MATCHINGS)?;
    for (key, owner) in MATCHING_KEYS {
        if *owner != kind && reader.has(key) {
            return Err(reader.error(
                key,
                format!("this key is only used with matching = {owner:?}"),
            ));
        }
    }
    let matching = match kind {
        "fixed_players" => {
            let path = reader.key_path("pairs");
            let value = reader.value("pairs");
            let value = reader.required("pairs", value)?;
            let pairs = super::reader::array_value(&path, value, |pair_path, pair| {
                let ids =
                    super::reader::array_value(pair_path, pair, super::reader::count_value::<u64>)?;
                match ids.as_slice() {
                    [a, b] => Ok((*a, *b)),
                    _ => Err(config_error(pair_path, "a pair has exactly two agent ids")),
                }
            })?;
            AnyMatching::FixedPlayers(FixedPlayers::pairs(&pairs))
        }
        "round_robin" => AnyMatching::RoundRobin(
            RoundRobin::new().both_orders(reader.flag("both_orders")?.unwrap_or(false)),
        ),
        "random_pairs" => AnyMatching::RandomPairs(RandomPairs::new()),
        "random_groups" => {
            let k = reader.require_count_usize("k")?;
            AnyMatching::RandomGroups(
                RandomGroups::new(k).map_err(|error| reader.error("k", error))?,
            )
        }
        "whole_population" => AnyMatching::WholePopulation(WholePopulation::new()),
        "grid_neighborhood" => {
            #[cfg(feature = "grid")]
            {
                let include_self = reader.flag("include_self")?.unwrap_or(false);
                AnyMatching::GridNeighborhood(
                    crate::GridNeighborhood::new().include_self(include_self),
                )
            }
            #[cfg(not(feature = "grid"))]
            {
                return Err(reader.error(
                    "matching",
                    "matching = \"grid_neighborhood\" requires the `grid` feature",
                ));
            }
        }
        _ => {
            #[cfg(feature = "net")]
            {
                AnyMatching::NetworkEdges(crate::NetworkEdges::new())
            }
            #[cfg(not(feature = "net"))]
            {
                return Err(reader.error(
                    "matching",
                    "matching = \"network_edges\" requires the `net` feature",
                ));
            }
        }
    };
    reader.finish()?;
    Ok(matching)
}

fn read_dynamics<G: SpecGame>(
    name: &str,
    reader: &mut ParamReader<'_>,
) -> Result<AnyDynamics<G::Replicator>, GameError> {
    let update = read_update(reader)?;
    Ok(match name {
        "imitate_best" => {
            let tie_break = match reader.choice("tie_break", &["keep_own", "lowest_action"])? {
                Some("lowest_action") => ImitationTie::LowestAction,
                _ => ImitationTie::KeepOwn,
            };
            AnyDynamics::ImitateBest(ImitateBest::new(update).tie_break(tie_break))
        }
        "fermi_update" => {
            let k = reader.require_real("k")?;
            AnyDynamics::Fermi(
                FermiUpdate::new(k, update).map_err(|error| reader.error("k", error))?,
            )
        }
        "replicator_update" => AnyDynamics::Replicator(G::read_replicator(reader, update)?),
        _ => {
            let fitness = reader.require_choice("fitness", &["linear", "exponential"])?;
            let w = reader.require_real("w")?;
            let fitness = match fitness {
                "linear" => Fitness::Linear { w },
                _ => Fitness::Exponential { w },
            };
            let mutation = reader.real("mutation")?.unwrap_or(0.0);
            let mut moran = MoranUpdate::new(fitness, mutation, update)
                .map_err(|error| reader.table_error(error))?;
            if let Some(sd) = reader.real("gaussian_sd")? {
                moran = moran
                    .gaussian_mutation(sd)
                    .map_err(|error| reader.error("gaussian_sd", error))?;
            }
            AnyDynamics::Moran(moran)
        }
    })
}
