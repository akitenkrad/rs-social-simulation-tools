**English** | [日本語](toxic-spread.ja.md)

# Toxic spread (`toxic_spread`)

> Toxic employees infect non-toxic neighbours through network edges with an
> empirically calibrated contagion probability.
> **Phase:** Interaction. **Source:** Housman & Minor (2015). **Kind:** empirical (p_spread).

[← Back to the mechanism catalog](../mechanisms.md)

## 1. Overview

`toxic_spread` models workplace toxicity as a social contagion process: each
toxic employee can convert adjacent non-toxic colleagues into toxic employees
through repeated negative interactions. The mechanism propagates toxicity along
the Watts–Strogatz social network that connects employees, applying a
per-edge infection probability calibrated to the empirical rate reported by
Housman & Minor (2015).

Toxicity affects the organisation indirectly: toxic employees depress
satisfaction in their neighbourhood (through fit and satisfaction dynamics) and
increase turnover probability, so `toxic_spread` is an important amplifier of
workforce instability. The baseline prevalence `P_TOXIC = 0.04` is set at
hiring time; `toxic_spread` allows this fraction to grow over time if not
checked.

## 2. Theory & source

Housman & Minor (2015) quantify the cost of toxic workers in a large services
firm, finding both direct productivity losses and strong peer-contagion effects.
socsim maps the contagion to a simple network-diffusion model: for each toxic
employee (sorted by `AgentId`), each non-toxic neighbour (sorted by `AgentId`)
independently becomes toxic with probability $p_{\text{spread}}$:

$$P(\text{non-toxic neighbour becomes toxic}) = p_{\text{spread}}$$

Infection decisions are collected first, then applied together, so a newly
infected employee does not become a source within the same step.

- `p_spread` ($p_{\text{spread}} = 0.46$) — empirical per-edge monthly contagion
  probability (Housman & Minor 2015).
- The network default is Watts–Strogatz (`k = 4`, $\beta = 0.1$), giving each
  employee roughly four neighbours.

## 3. Data flow

![toxic_spread data flow](../assets/mech-toxic-spread.svg)

The mechanism reads `Employee.is_toxic` and the network adjacency list, samples
`ctx.rng` once per susceptible edge, and writes `Employee.is_toxic = true` for
newly infected employees. No other state is touched.

## 4. Position in the 6-phase loop

Runs in **Interaction**, the fourth phase, alongside `peer_effect` and `ocb`.
The ordering within Interaction matters only in that `toxic_spread` should be
declared **before** any mechanism that reads `is_toxic` within the same
Interaction phase. In the default pack, no Interaction-phase mechanism reads
`is_toxic`, so declaration order is flexible.

Placing it in Interaction is appropriate because toxicity propagates through
direct social contact — the same conceptual frame as peer productivity spillovers.

## 5. State read/write contract

| Field | Read | Write | Notes |
|---|:--:|:--:|---|
| `Employee.is_toxic` | ✓ | ✓ | Source of contagion; written `true` for newly infected. |
| `HrWorld.network` (adjacency) | ✓ | | Watts–Strogatz; neighbours looked up per toxic employee. |

## 6. Dependencies & ordering constraints

- **Upstream:** `hiring` (Decision) sets `is_toxic` on new hires at the
  population baseline `P_TOXIC = 0.04`. No same-step dependency beyond the
  network being initialised.
- **Downstream:** no mechanism reads `is_toxic` within the same step's
  remaining phases. The effects of toxicity surface through `fit` (which
  updates `satisfaction`) and `turnover` (which consumes `satisfaction` and
  `embeddedness`) in the **Decision** phase of the **next** step, so toxicity
  propagated this step takes effect from the next step onwards.

## 7. Parameters

| Param key | Default | Kind | Source |
|---|---|---|---|
| `p_spread` | `0.46` | empirical (per-edge monthly contagion rate) | Housman & Minor (2015) |

`P_TOXIC = 0.04` (baseline prevalence at hire) is set by `hiring`, not by this
mechanism; adjust it via the `hiring` mechanism's `p_toxic` parameter.

## 8. How to apply

### Scenario TOML

```toml
[[mechanism]]
name  = "toxic_spread"
phase = "interaction"
[mechanism.params]
p_spread = 0.46
```

### Library mode

```rust
use socsim_config::{Registry, Params, ModulePack};
use socsim_packs::hr_lifecycle::{HrLifecyclePack, HrWorld};
use socsim_engine::{RandomActivationScheduler, SimulationBuilder};

let mut reg: Registry<HrWorld> = Registry::new();
HrLifecyclePack.register(&mut reg);

let ts = reg.build("toxic_spread", &Params::empty())?;
let mut sim = SimulationBuilder::new(world)
    .scheduler(Box::new(RandomActivationScheduler))
    .seed(42)
    .add_mechanism(ts)
    .build();
sim.run()?;
```

## 9. Determinism & RNG

Draws randomness from `ctx.rng` — one `gen::<f64>()` call per susceptible
(toxic-source, non-toxic-neighbour) edge. To ensure bit-reproducibility:

1. Toxic source employees are collected and **sorted by AgentId** before
   iteration.
2. For each source, its neighbours are **sorted by AgentId** before the RNG is
   consulted.

This lexicographic ordering makes the RNG consumption sequence identical
regardless of the underlying map iteration order, so the run is reproducible
given the same seed.

### Relationship to the general `si_contagion` kernel

`toxic_spread` is an SI-variant and conceptually overlaps with the general
[`si_contagion`](si-contagion.md) mechanism in `socsim-mechanisms`, but it is
**deliberately not** built on the shared kernel, because the two have
incompatible RNG-draw structures and unifying them would change this
empirically calibrated mechanism's seeded trajectory:

- **Iteration pivot.** `toxic_spread` iterates *source-first* (each toxic
  employee → its neighbours); `si_contagion` iterates *target-first* (each
  inactive agent → its active neighbours).
- **Break semantics.** `toxic_spread` draws one Bernoulli per
  (toxic-source, non-toxic-neighbour) edge and **never breaks** — a neighbour
  adjacent to *k* toxic sources consumes *k* draws. `si_contagion` **breaks on
  first success**, so a target consumes at most one draw per active neighbour
  up to the first hit.
- **Order basis.** `toxic_spread` orders by sorted `AgentId`; `si_contagion`
  uses the scheduler's `ctx.agent_order`.

These differences mean a faithful delegation would alter the number and order
of RNG draws, breaking the deterministic seeded tests. `toxic_spread` therefore
remains an HR-local mechanism; it is recorded as a *future candidate* should the
kernel ever grow a source-first, no-break variant. `HrWorld` does not implement
the `BinaryState` / `Neighbors` capability traits for the same reason.

## 10. Expected behaviour

With `P_TOXIC = 0.04` and `p_spread = 0.46`, toxic prevalence should rise from
its initial 4 % toward a higher equilibrium that depends on network structure
and the rate at which toxic employees are turned over. On a Watts–Strogatz
network with `k = 4`, the mechanism typically causes a slow upward drift in
toxicity over 24–48 steps before stabilising (or causing a turnover cascade
that purges toxic nodes). Disabling `hiring` replacement while running
`toxic_spread` will eventually infect the majority of the network; re-enabling
hiring with `p_toxic = 0.04` gradually dilutes the toxic fraction.

## 11. References

- Housman, M., & Minor, D. (2015). Toxic workers. *Harvard Business School
  Working Paper* 16-057.
