//! Reading `[world]` and `[world.population]`.

use super::game::{sealed::SpecGame, InitialSpec, SHARE_SUM_TOLERANCE};
use super::reader::{config_error, count_value, real_value, ParamReader};
use super::rules::{RuleRegistry, HELD};
use crate::{AssignGroup, AssignKind, Assignment, ExperimentState, GameError, Topology};

const GRID_KEYS: &[&str] = &["grid_rows", "grid_cols", "boundary", "neighborhood"];

/// The keys of each network model, after `network_model`.
const NETWORK_KEYS: &[(&str, &str)] = &[
    ("network_p", "erdos_renyi"),
    ("network_k", "watts_strogatz"),
    ("network_beta", "watts_strogatz"),
    ("network_m", "barabasi_albert"),
];

/// A random network to generate for each seed.
#[cfg(feature = "net")]
#[derive(Clone, Copy, Debug)]
pub(crate) enum NetworkSpec {
    ErdosRenyi { p: f64 },
    WattsStrogatz { k: usize, beta: f64 },
    BarabasiAlbert { m: usize },
}

/// The topology of a scenario, before a seed is chosen.
#[derive(Clone, Debug)]
pub(crate) enum TopologySpec {
    None,
    /// A grid, built when the scenario is read.
    #[cfg(feature = "grid")]
    Grid(Topology),
    /// A network generated from the stream `derive_seed(seed, &[0, 1])`.
    #[cfg(feature = "net")]
    Network(NetworkSpec),
}

impl TopologySpec {
    /// The topology for `seed`, or `None` without a topology.
    pub(crate) fn resolve(&self, n_agents: usize, seed: u64) -> Option<Topology> {
        let _ = (n_agents, seed);
        match self {
            Self::None => None,
            #[cfg(feature = "grid")]
            Self::Grid(topology) => Some(topology.clone()),
            #[cfg(feature = "net")]
            Self::Network(network) => {
                use socsim_core::{derive_seed, AgentId, SimRng};
                use socsim_net::SocialNetwork;

                let ids: Vec<AgentId> = (0..n_agents as u64).map(AgentId).collect();
                let mut rng = SimRng::from_seed(derive_seed(seed, &[0, 1]));
                let network = match *network {
                    NetworkSpec::ErdosRenyi { p } => SocialNetwork::erdos_renyi(&ids, p, &mut rng),
                    NetworkSpec::WattsStrogatz { k, beta } => {
                        SocialNetwork::watts_strogatz(&ids, k, beta, &mut rng)
                    }
                    NetworkSpec::BarabasiAlbert { m } => {
                        SocialNetwork::barabasi_albert(&ids, m, &mut rng)
                    }
                };
                Some(Topology::network(network))
            }
        }
    }
}

/// What `[world]` configures, apart from the game and the population.
pub(crate) struct WorldConfig {
    pub(crate) n_agents: usize,
    pub(crate) history_len: usize,
    pub(crate) topology: TopologySpec,
}

/// Reads the keys of `[world]` other than `game` and `population`.
pub(crate) fn read_world(reader: &mut ParamReader<'_>) -> Result<WorldConfig, GameError> {
    let n_agents = reader.count_usize("n_agents")?;
    let history_len = reader.require_count_usize("history_len")?;
    if history_len == 0 {
        return Err(reader.error("history_len", "must be greater than zero"));
    }
    let topology = reader
        .choice("topology", &["none", "grid", "network"])?
        .unwrap_or("none");
    if topology != "grid" {
        if let Some(key) = GRID_KEYS.iter().find(|key| reader.has(key)) {
            return Err(reader.error(key, "this key is only used with topology = \"grid\""));
        }
    }
    if topology != "network" {
        let network_key = std::iter::once("network_model")
            .chain(NETWORK_KEYS.iter().map(|(key, _)| *key))
            .find(|key| reader.has(key));
        if let Some(key) = network_key {
            return Err(reader.error(key, "this key is only used with topology = \"network\""));
        }
    }
    let (n_agents, topology) = match topology {
        "grid" => read_grid(reader, n_agents)?,
        "network" => {
            let n_agents = reader.required("n_agents", n_agents)?;
            (n_agents, read_network(reader, n_agents)?)
        }
        _ => (reader.required("n_agents", n_agents)?, TopologySpec::None),
    };
    Ok(WorldConfig {
        n_agents,
        history_len,
        topology,
    })
}

#[cfg(feature = "grid")]
fn read_grid(
    reader: &mut ParamReader<'_>,
    n_agents: Option<usize>,
) -> Result<(usize, TopologySpec), GameError> {
    use crate::{GridBoundary, GridNeighbors, GridSpec};

    let rows = reader.require_count_usize("grid_rows")?;
    let cols = reader.require_count_usize("grid_cols")?;
    let boundary = match reader.require_choice("boundary", &["fixed", "toroidal"])? {
        "fixed" => GridBoundary::Fixed,
        _ => GridBoundary::Toroidal,
    };
    let neighborhood = match reader.require_choice("neighborhood", &["moore", "von_neumann"])? {
        "moore" => GridNeighbors::Moore,
        _ => GridNeighbors::VonNeumann,
    };
    let cells = rows
        .checked_mul(cols)
        .ok_or_else(|| reader.error("grid_cols", "grid_rows * grid_cols overflows"))?;
    if let Some(n_agents) = n_agents {
        if n_agents != cells {
            return Err(reader.error(
                "n_agents",
                format!("a {rows} x {cols} grid has {cells} agents, not {n_agents}"),
            ));
        }
    }
    let topology = Topology::grid(GridSpec {
        rows,
        cols,
        boundary,
        neighborhood,
    })
    .map_err(|error| reader.table_error(error))?;
    Ok((cells, TopologySpec::Grid(topology)))
}

#[cfg(not(feature = "grid"))]
fn read_grid(
    reader: &mut ParamReader<'_>,
    _n_agents: Option<usize>,
) -> Result<(usize, TopologySpec), GameError> {
    Err(reader.error(
        "topology",
        "topology = \"grid\" requires the `grid` feature",
    ))
}

#[cfg(feature = "net")]
fn read_network(reader: &mut ParamReader<'_>, n_agents: usize) -> Result<TopologySpec, GameError> {
    let model = reader.require_choice(
        "network_model",
        &["erdos_renyi", "watts_strogatz", "barabasi_albert"],
    )?;
    for (key, owner) in NETWORK_KEYS {
        if *owner != model && reader.has(key) {
            return Err(reader.error(
                key,
                format!("this key is only used with network_model = {owner:?}"),
            ));
        }
    }
    let unit = |reader: &mut ParamReader<'_>, key: &str| -> Result<f64, GameError> {
        let value = reader.require_real(key)?;
        if !(0.0..=1.0).contains(&value) {
            return Err(reader.error(key, format!("must lie in [0, 1], got {value}")));
        }
        Ok(value)
    };
    let largest = n_agents.saturating_sub(1);
    let spec = match model {
        "erdos_renyi" => NetworkSpec::ErdosRenyi {
            p: unit(reader, "network_p")?,
        },
        "watts_strogatz" => {
            let k = reader.require_count_usize("network_k")?;
            if k % 2 != 0 || k < 2 || k > largest {
                return Err(reader.error(
                    "network_k",
                    format!("must be even with 2 <= k <= n_agents - 1 = {largest}, got {k}"),
                ));
            }
            NetworkSpec::WattsStrogatz {
                k,
                beta: unit(reader, "network_beta")?,
            }
        }
        _ => {
            let m = reader.require_count_usize("network_m")?;
            if m < 1 || m > largest {
                return Err(reader.error(
                    "network_m",
                    format!("must satisfy 1 <= m <= n_agents - 1 = {largest}, got {m}"),
                ));
            }
            NetworkSpec::BarabasiAlbert { m }
        }
    };
    Ok(TopologySpec::Network(spec))
}

#[cfg(not(feature = "net"))]
fn read_network(reader: &mut ParamReader<'_>, _n_agents: usize) -> Result<TopologySpec, GameError> {
    Err(reader.error(
        "topology",
        "topology = \"network\" requires the `net` feature",
    ))
}

/// What `[world.population]` configures.
pub(crate) struct PopulationConfig<G: SpecGame> {
    pub(crate) assignment: Assignment,
    /// The rules assigned to at least one agent, in the order of the groups.
    pub(crate) rules: Vec<String>,
    pub(crate) initial: InitialSpec<G>,
}

/// The size of a group: a count or a share of the population.
enum GroupSize {
    Count(usize),
    Share(f64),
}

/// Reads `[world.population]`.
pub(crate) fn read_population<G: SpecGame, X: ExperimentState>(
    reader: &mut ParamReader<'_>,
    game: &G,
    n_agents: usize,
    registry: &RuleRegistry<G, X>,
) -> Result<PopulationConfig<G>, GameError> {
    let shuffled = reader.choice("assign", &["ordered", "shuffled"])? == Some("shuffled");

    let groups_path = reader.key_path("group");
    let groups_value = reader.value("group");
    let groups_value = reader.required("group", groups_value)?;
    let items = groups_value.as_array().ok_or_else(|| {
        config_error(
            &groups_path,
            format!(
                "expected an array of tables ([[world.population.group]]), found {}",
                groups_value.type_str()
            ),
        )
    })?;
    if items.is_empty() {
        return Err(config_error(&groups_path, "at least one group is required"));
    }
    let mut groups: Vec<(String, GroupSize)> = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let path = format!("{groups_path}[{index}]");
        let table = item.as_table().ok_or_else(|| {
            config_error(
                &path,
                format!("expected a table, found {}", item.type_str()),
            )
        })?;
        let mut group = ParamReader::new(table, path.clone());
        let rule = group.text("rule")?;
        let rule = group.required("rule", rule)?;
        if rule != HELD && !registry.contains(rule) {
            return Err(group.error(
                "rule",
                format!(
                    "unknown rule {rule:?} (registered: {}; {HELD:?} means no rule)",
                    registry.names().join(", ")
                ),
            ));
        }
        if let Some(first) = groups.iter().position(|(name, _)| name == rule) {
            return Err(group.error(
                "rule",
                format!("rule {rule:?} already has a group at {groups_path}[{first}]"),
            ));
        }
        let size = match (group.value("count"), group.value("share")) {
            (Some(count), None) => GroupSize::Count(count_value(&group.key_path("count"), count)?),
            (None, Some(share)) => {
                let share_path = group.key_path("share");
                let share = real_value(&share_path, share)?;
                if share < 0.0 {
                    return Err(config_error(
                        &share_path,
                        format!("a share must be non-negative, got {share}"),
                    ));
                }
                GroupSize::Share(share)
            }
            (Some(_), Some(_)) => {
                return Err(group.table_error("give either count or share, not both"))
            }
            (None, None) => return Err(group.table_error("give count or share")),
        };
        if let Some((_, first)) = groups.first() {
            if matches!(first, GroupSize::Count(_)) != matches!(size, GroupSize::Count(_)) {
                return Err(group.table_error("every group must use the same of count and share"));
            }
        }
        group.finish()?;
        groups.push((rule.to_owned(), size));
    }

    let counts = group_counts(&groups_path, &groups, n_agents)?;
    let assign_groups: Vec<AssignGroup> = groups
        .iter()
        .zip(&counts)
        .map(|((rule, _), &count)| AssignGroup {
            kind: if rule == HELD {
                AssignKind::Held
            } else {
                AssignKind::Rule(rule.clone())
            },
            count,
        })
        .collect();
    let rules = groups
        .iter()
        .zip(&counts)
        .filter(|((rule, _), &count)| rule != HELD && count > 0)
        .map(|((rule, _), _)| rule.clone())
        .collect();
    let assignment = if shuffled {
        Assignment::Shuffled(assign_groups)
    } else {
        Assignment::Ordered(assign_groups)
    };

    let initial = G::read_initial(reader, game)?;
    Ok(PopulationConfig {
        assignment,
        rules,
        initial,
    })
}

/// The number of agents of each group. Shares are turned into counts by the
/// largest remainder method, ties going to the group listed first.
fn group_counts(
    path: &str,
    groups: &[(String, GroupSize)],
    n_agents: usize,
) -> Result<Vec<usize>, GameError> {
    let mut counts = Vec::with_capacity(groups.len());
    let mut quotas = Vec::with_capacity(groups.len());
    for (_, size) in groups {
        match size {
            GroupSize::Count(count) => counts.push(*count),
            GroupSize::Share(share) => quotas.push(n_agents as f64 * share),
        }
    }
    if quotas.is_empty() {
        let total: usize = counts.iter().sum();
        if total != n_agents {
            return Err(config_error(
                path,
                format!("group counts sum to {total}, but world.n_agents is {n_agents}"),
            ));
        }
        return Ok(counts);
    }

    let total: f64 = groups
        .iter()
        .map(|(_, size)| match size {
            GroupSize::Share(share) => *share,
            GroupSize::Count(_) => 0.0,
        })
        .sum();
    if (total - 1.0).abs() > SHARE_SUM_TOLERANCE {
        return Err(config_error(
            path,
            format!("group shares sum to {total}, expected 1"),
        ));
    }
    let mut counts: Vec<usize> = quotas.iter().map(|quota| quota.floor() as usize).collect();
    let floored: usize = counts.iter().sum();
    // The shares sum to 1 within 1e-9, so the floored counts can overshoot
    // `n_agents`, or leave more agents than entries to hand out, only when
    // `n_agents` is around 10^9 or larger. These checks keep that case an
    // error instead of a panic.
    let remaining = n_agents.checked_sub(floored).ok_or_else(|| {
        config_error(
            path,
            format!("group shares assign {floored} agents, more than world.n_agents = {n_agents}"),
        )
    })?;
    if remaining > counts.len() {
        return Err(config_error(
            path,
            format!(
                "{remaining} agents are left after rounding down, more than the {} groups",
                counts.len()
            ),
        ));
    }
    let mut order: Vec<usize> = (0..quotas.len()).collect();
    // A stable sort keeps the listed order among equal fractional parts.
    order.sort_by(|&left, &right| {
        let left = quotas[left] - quotas[left].floor();
        let right = quotas[right] - quotas[right].floor();
        right.total_cmp(&left)
    });
    for &index in order.iter().take(remaining) {
        counts[index] += 1;
    }
    Ok(counts)
}

/// Reads the `scheduler` of `[simulation]`.
pub(crate) fn read_scheduler(scheduler: &str) -> Result<crate::SchedulerChoice, GameError> {
    let value = toml::Value::String(scheduler.to_owned());
    match super::reader::choice_value(
        "simulation.scheduler",
        &value,
        &["sequential", "random_activation"],
    )? {
        "random_activation" => Ok(crate::SchedulerChoice::RandomActivation),
        _ => Ok(crate::SchedulerChoice::Sequential),
    }
}
