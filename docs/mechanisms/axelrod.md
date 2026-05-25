**English** | [日本語](axelrod.ja.md)

# Axelrod culture (`axelrod`)

> On each random encounter a site copies one differing cultural feature from a
> neighbour with probability equal to their feature-overlap similarity, so similar
> neighbours converge and dissimilar ones stay apart.
> **Phase:** Interaction. **Source:** Axelrod (1997). **Kind:** cultural dissemination (culture vectors, event-driven).

[← Back to the mechanism catalog](../mechanisms.md)

## 1. Overview

`axelrod` is the **cultural-dissemination** member of the general
`socsim-social-dynamics` pack. Each agent holds a fixed-length categorical *culture
vector* of `F` features. The mechanism is **event-driven**: each step it runs
`events_per_step` micro-events. In each event it draws a site `s` uniformly and a
random neighbour `nb`, computes their similarity `sim = (#matching features) / F`,
and — if `0 < sim < 1` — with probability `sim` copies one randomly-chosen *differing*
feature from `nb` into `s`.

The two boundary cases do nothing: at `sim = 0` the agents share no feature value, so
they never interact; at `sim = 1` they are identical, so there is nothing to copy.
This homophily-with-influence dynamic produces **stable cultural regions** —
contiguous blocks of identical agents separated by boundaries where neighbours share
nothing. The free helper `is_absorbing` tests the absorbing state (every adjacent
pair has `sim ∈ {0, 1}`), i.e. no further event can change any agent.

The mechanism is **library-only**: it operates over any world implementing the
`CultureVectors` and `Neighbors` capability traits from `socsim-core`. There is **no
`ModulePack`** for it (no scenario-TOML registration); construct it directly and add
it to a `SimulationBuilder`.

## 2. Theory & source

Axelrod (1997) asked why cultural differences persist rather than everyone converging
to a single culture. His model combines two intuitive forces: **homophily** (agents
interact more readily with similar others) and **social influence** (interaction makes
agents more alike). Counter-intuitively, their interplay can lock the population into
several distinct, stable cultures rather than a single global one.

Each agent has a culture vector of `F` features, each taking one of `q` categorical
trait values. For a site `s` and neighbour `nb`, the cultural similarity is

$$\mathrm{sim}(s, nb) = \frac{1}{F}\bigl|\{\, k : \text{feature}(s, k) = \text{feature}(nb, k) \,\}\bigr| \in [0, 1].$$

One interaction event proceeds as: with probability `sim` (homophily/influence), pick
one feature `k` on which `s` and `nb` differ, uniformly at random, and set
`feature(s, k) ← feature(nb, k)`. The cases `sim ∈ {0, 1}` are inert. The
implementation is ported verbatim from the `wang2025` reference's `classical_event`.

## 3. Data flow

![axelrod data flow](../assets/mech-axelrod.svg)

For each of `events_per_step` events the mechanism draws a site `s` and a random
neighbour `nb` from `neighbors_of(s)`, reads their `feature` values to compute `sim`,
and — with probability `sim`, when `0 < sim < 1` — writes one copied feature via
`set_feature`. No other state is touched.

## 4. Position in the 6-phase loop

Runs in **Interaction**, the phase where agents influence one another. Cultural
copying *is* the interaction.

- The update is **asynchronous and sequential within the step**: each of the
  `events_per_step` events reads and writes the live culture vectors, so a later event
  in the same step sees the effect of an earlier one.
- This is the event-based idiom — many micro-events (encounters) batched into a single
  `apply` call mapped onto one engine tick (see the
  [event-driven / sub-tick](../architecture.md#event-driven--sub-tick-models)
  pattern). The engine tick is the observation cadence; `events_per_step` sets how
  many copy events happen per observation.
- There is no built-in stop logic; pair it with a driver- or world-side
  absorbing-state check (the `is_absorbing` helper) if a stop is desired.

## 5. State read/write contract

| Field | Read | Write | Notes |
|---|:--:|:--:|---|
| `feature(i, k)` (`CultureVectors`) | ✓ | ✓ | Read live each event to compute `sim`; one differing feature of `s` overwritten on a successful copy. |
| `n_features()` (`CultureVectors`) | ✓ | | The vector length `F`, used as the similarity denominator. |
| `neighbors_of(s)` (`Neighbors`) | ✓ | | Candidate interaction partners for the drawn site `s`. |

## 6. Dependencies & ordering constraints

- **Upstream:** none. It needs only a world implementing `CultureVectors +
  Neighbors`; the topology (lattice, network, complete graph) is the world's concern
  via `neighbors_of`, and the initial culture vectors are the world's responsibility.
- **Downstream:** none required. The mechanism has no convergence/stop logic of its
  own; the free helper `is_absorbing(world)` lets a driver or PostStep check detect
  the absorbing state and stop the run.

## 7. Parameters

| Param | Type | Default | Meaning |
|---|---|---|---|
| `events_per_step` | `usize` | `1` | Number of micro-events (encounters) per engine tick. Larger → faster dynamics per observation. |

The feature count `F` and trait alphabet `q` are properties of the world (via
`n_features` and the trait values it stores), not of the mechanism. There is no
ModulePack and therefore no scenario-TOML param block; the single field is a
constructor argument.

## 8. How to apply

This mechanism is **library-mode only** — there is no scenario-TOML registration.
Provide a world implementing `CultureVectors + Neighbors`, construct the mechanism,
and add it to a `SimulationBuilder`.

```rust
use socsim_core::{AgentId, CultureVectors, Neighbors, WorldState, SimClock};
use socsim_social_dynamics::{AxelrodMechanism, is_absorbing};
use socsim_engine::{SequentialScheduler, SimulationBuilder};

// A world carrying an F-feature culture vector per agent (e.g. over a lattice).
struct CultureWorld { clock: SimClock, f: usize, traits: Vec<Vec<u32>> }

impl WorldState for CultureWorld {
    fn agent_ids(&self) -> Vec<AgentId> {
        (0..self.traits.len() as u64).map(AgentId).collect()
    }
    fn clock(&self) -> &SimClock { &self.clock }
    fn clock_mut(&mut self) -> &mut SimClock { &mut self.clock }
}
impl CultureVectors for CultureWorld {
    fn n_features(&self) -> usize { self.f }
    fn feature(&self, id: AgentId, k: usize) -> u32 { self.traits[id.0 as usize][k] }
    fn set_feature(&mut self, id: AgentId, k: usize, v: u32) {
        self.traits[id.0 as usize][k] = v;
    }
}
impl Neighbors for CultureWorld {
    fn neighbors_of(&self, id: AgentId) -> Vec<AgentId> {
        self.agent_ids().into_iter().filter(|&j| j != id).collect()
    }
}

// `n_sites` micro-events per step (one sweep over the population).
let n_sites = world.agent_ids().len();
let axelrod = AxelrodMechanism::new(n_sites);

let mut sim = SimulationBuilder::new(world) // world: CultureVectors + Neighbors
    .scheduler(Box::new(SequentialScheduler))
    .seed(42)
    .add_mechanism(axelrod)
    .build();
sim.run()?;
```

To stop on the absorbing state, check `is_absorbing(sim.world())` from the driver loop
(or in a PostStep mechanism) and break once it returns `true`.

## 9. Determinism & RNG

**Stochastic**: every event draws the site `s`, the neighbour `nb`, the
interaction trial (with probability `sim`), and the copied feature from `ctx.rng`, so
the trajectory depends on the RNG stream. Because all randomness flows through
`ctx.rng`, a fixed seed yields a fully reproducible run. The process is absorbing —
once `is_absorbing` holds, no further event changes anything.

## 10. Expected behaviour

The number of surviving cultures is governed by `F` and `q` (world properties)
relative to the topology:

- **Few features / many traits** (high initial diversity, low overlap): neighbours
  rarely share enough to interact, so the population freezes into **many small
  cultural regions** — Axelrod's persistence of diversity.
- **Many features / few traits**: high overlap lets influence propagate, driving the
  population toward **a single dominant culture** (monoculture).

The dynamics always end in an absorbing configuration of contiguous identical regions
separated by zero-similarity boundaries; the cultural map then never changes again.

## 11. References

- Axelrod, R. (1997). The dissemination of culture: A model with local convergence
  and global polarization. *Journal of Conflict Resolution*, 41(2), 203–226.
- Castellano, C., Marsili, M., & Vespignani, A. (2000). Nonequilibrium phase
  transition in a model for social influence. *Physical Review Letters*, 85(16),
  3536–3539.
