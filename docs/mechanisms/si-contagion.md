**English** | [日本語](si-contagion.ja.md)

# SI contagion (`si_contagion`)

> Each step, every active (infected) neighbour independently infects an inactive
> agent with probability β; newly-infected agents are activated in a synchronous
> round.
> **Phase:** Interaction. **Source:** SI epidemic model. **Kind:** network contagion (binary state, β).

[← Back to the mechanism catalog](../mechanisms.md)

## 1. Overview

`si_contagion` is the per-edge **SI (Susceptible–Infected)** member of the
network-contagion family in the general `socsim-social-dynamics` pack. Each agent
carries a binary *active / infected* flag. Once per step the mechanism performs a
**synchronous round**: it snapshots the active set at the start of the step, and for
every inactive agent it draws one independent Bernoulli(β) trial per *active*
neighbour (read from the snapshot). The agent becomes infected if **any** of those
trials succeeds. Newly-infected agents are batch-activated, so an agent infected
mid-round does not become a source until the next round.

Because the round is evaluated against the start-of-step snapshot and written back as
a batch, the result is independent of agent activation order. There is no recovery
state (SI, not SIR): infection is monotone, so the active set only grows. The
mechanism calls `request_stop` on **saturation** — a round in which no new agent is
infected, or everyone is already active.

The mechanism is **library-only**: it operates over any world implementing the
`BinaryState` and `Neighbors` capability traits from `socsim-core`. There is **no
`ModulePack`** for it (no scenario-TOML registration); construct it directly and add
it to a `SimulationBuilder`.

## 2. Theory & source

The SI model is the simplest compartmental epidemic model: agents are either
*susceptible* (inactive) or *infected* (active), and a susceptible agent becomes
infected through contact with infected neighbours, with no return to the susceptible
state. On a network, infection is mediated per edge: each contact with an infected
neighbour is an independent transmission opportunity with probability β.

For an inactive agent `i` whose neighbour set $N(i)$ has active members
$A(i) = \{\, j \in N(i) : j \text{ active} \,\}$ at the start of the step, the
probability that `i` becomes infected this round is

$$P(i \text{ infected}) = 1 - (1 - \beta)^{|A(i)|},$$

i.e. `i` escapes infection only if *every* independent per-edge trial fails. Equivalently,
`i` is infected iff at least one Bernoulli(β) trial — one per active neighbour — succeeds.
β is clamped to $[0, 1]$. The implementation is ported from the `granovetter1973`
reference's SI branch.

## 3. Data flow

![si_contagion data flow](../assets/mech-si-contagion.svg)

The mechanism reads `is_active(i)` and `neighbors_of(i)` from a start-of-step
snapshot of the active set, draws one Bernoulli(β) trial per active neighbour of each
inactive agent, collects the newly-infected agents, and batch-writes them via
`set_active(i, true)`. No other state is touched.

## 4. Position in the 6-phase loop

Runs in **Interaction**, the phase where agents influence one another. Disease
transmission along edges *is* the interaction here.

- It reads a snapshot of the active set taken at the start of its `apply` call, then
  activates every newly-infected agent in a single batch — making the round
  synchronous and independent of the scheduler's activation order.
- An agent activated this round is not yet a source: the snapshot fixes the source
  set for the whole round, so contagion advances one ring per step.
- On **saturation** (no new infection, or everyone active) it calls
  `ctx.request_stop`, matching the `granovetter1973` reference's convergence rule.

## 5. State read/write contract

| Field | Read | Write | Notes |
|---|:--:|:--:|---|
| `is_active(i)` (`BinaryState`) | ✓ | ✓ | Snapshotted at step start; inactive agents flipped to active on infection. |
| `neighbors_of(i)` (`Neighbors`) | ✓ | | Contact set; only the *active* members (from the snapshot) are infection sources. |

## 6. Dependencies & ordering constraints

- **Upstream:** none. It needs only a world implementing `BinaryState + Neighbors`;
  the topology (complete graph, ring, network, lattice) is the world's concern via
  `neighbors_of`, and the initial seed set is the world's responsibility.
- **Downstream:** none required — the mechanism self-terminates the run via
  `request_stop` on saturation. The active set is monotone, so no convergence
  helper is needed.

## 7. Parameters

| Param | Type | Default | Meaning |
|---|---|---|---|
| `beta` (β) | `f64` | `0.5` | Per-edge infection probability, clamped to `[0, 1]`. Larger β → faster, wider spread. |

There is no ModulePack and therefore no scenario-TOML param block; the single field
is a constructor argument.

## 8. How to apply

This mechanism is **library-mode only** — there is no scenario-TOML registration.
Provide a world implementing `BinaryState + Neighbors`, construct the mechanism, and
add it to a `SimulationBuilder`.

```rust
use socsim_core::{AgentId, BinaryState, Neighbors, WorldState, SimClock};
use socsim_social_dynamics::SiContagionMechanism;
use socsim_engine::{SequentialScheduler, SimulationBuilder};

// A world carrying one active/infected flag per agent (e.g. over a network).
struct ContagionWorld { clock: SimClock, active: Vec<bool> }

impl WorldState for ContagionWorld {
    fn agent_ids(&self) -> Vec<AgentId> {
        (0..self.active.len() as u64).map(AgentId).collect()
    }
    fn clock(&self) -> &SimClock { &self.clock }
    fn clock_mut(&mut self) -> &mut SimClock { &mut self.clock }
}
impl BinaryState for ContagionWorld {
    fn is_active(&self, id: AgentId) -> bool { self.active[id.0 as usize] }
    fn set_active(&mut self, id: AgentId, v: bool) { self.active[id.0 as usize] = v; }
}
impl Neighbors for ContagionWorld {
    fn neighbors_of(&self, id: AgentId) -> Vec<AgentId> {
        self.agent_ids().into_iter().filter(|&j| j != id).collect()
    }
}

// β = 0.3 per-edge infection probability.
let si = SiContagionMechanism::new(0.3);

let mut sim = SimulationBuilder::new(world) // world: BinaryState + Neighbors
    .scheduler(Box::new(SequentialScheduler))
    .seed(42)
    .add_mechanism(si)
    .build();
sim.run()?;
```

Seed the initial infected set in the world before running. Lower β for a slower,
more stochastic cascade; raise it toward 1 for near-deterministic flooding.

## 9. Determinism & RNG

**Stochastic**: each per-edge transmission is a Bernoulli(β) trial drawn from
`ctx.rng`, so the trajectory depends on the RNG stream. Because all randomness flows
through `ctx.rng`, a fixed seed yields a fully reproducible run. The active set is
monotone, so the run always reaches saturation in finite steps. Inactive agents are
visited in the scheduler's activation order (`ctx.agent_order`), so the mechanism
reproduces a `RandomActivationScheduler`-driven run exactly.

## 10. Expected behaviour

The dynamics are governed by β relative to the network topology:

- **Large β** (≳ the percolation threshold): infection floods the giant component
  within a few rounds, reaching near-total saturation.
- **Small β**: many per-edge trials fail, so spread is slow and may stall in pockets;
  whether a large outbreak occurs depends on whether β exceeds the network's
  percolation threshold.

Because there is no recovery, the active fraction is non-decreasing and the run ends
at a fixed point (saturation) rather than oscillating.

## 11. References

- Kermack, W. O., & McKendrick, A. G. (1927). A contribution to the mathematical
  theory of epidemics. *Proceedings of the Royal Society A*, 115(772), 700–721.
- Pastor-Satorras, R., Castellano, C., Van Mieghem, P., & Vespignani, A. (2015).
  Epidemic processes in complex networks. *Reviews of Modern Physics*, 87(3), 925–979.
