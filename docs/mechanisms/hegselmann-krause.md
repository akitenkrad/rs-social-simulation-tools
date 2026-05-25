**English** | [日本語](hegselmann-krause.ja.md)

# Hegselmann–Krause (`hegselmann_krause`)

> Every agent synchronously moves to the (chosen) mean of all opinions within a
> confidence bound ε of its own.
> **Phase:** Interaction. **Source:** Hegselmann & Krause (2002, 2005). **Kind:** bounded-confidence (ε, mean).

[← Back to the mechanism catalog](../mechanisms.md)

## 1. Overview

`hegselmann_krause` (HK) is one of the two **bounded-confidence (BC)** opinion-dynamics
mechanisms shipped by the general `socsim-mechanisms` crate. Once per step it
performs a **synchronous** update: it snapshots every agent's scalar opinion, and
for each agent `i` it recomputes that opinion as the (chosen) *mean* of the opinions
that lie within a symmetric tolerance ε of `x_i` — its *confidence set*. Opinions
further than ε away are simply ignored, so an agent is only ever pulled toward those
it already broadly agrees with.

Because every new opinion is computed from the *same* start-of-step snapshot, the
result is independent of agent activation order — this is the simultaneous (parallel)
update of the canonical HK model. With a wide ε the population collapses to a single
consensus; with a narrow ε it fragments into several stable opinion clusters.

The mechanism is **library-only**: it operates over any world that implements the
`ScalarOpinions` and `Neighbors` capability traits from `socsim-core`. There
is **no `ModulePack`** for it (it ships no scenario-TOML registration); construct it
directly and add it to a `SimulationBuilder`.

## 2. Theory & source

Hegselmann & Krause (2002) introduced bounded confidence as a model of *continuous*
opinion dynamics: agent `i` is influenced only by the agents whose opinions fall
within a confidence radius ε, and updates to the **arithmetic mean** of that set.
Writing the confidence (influence) set of `i` at opinion profile $x$ as

$$I(i, x) = \{\, j \in N(i) \cup \{i\} \;:\; |x_i - x_j| \le \varepsilon \,\},$$

the synchronous update for the base (arithmetic) model is

$$x_i' = \frac{1}{|I(i, x)|} \sum_{j \in I(i, x)} x_j .$$

The 2005 paper generalises HK along the axis of *which kind of mean* aggregates the
confidence set, replacing the arithmetic mean with a family of averaging operators.
socsim exposes this as the [`MeanOperator`] enum with five members:

$$
\begin{aligned}
A &= P_1 = \tfrac{1}{m}\textstyle\sum_j x_j && \text{(arithmetic)}\\
G &= P_0 = \Bigl(\textstyle\prod_j x_j\Bigr)^{1/m} && \text{(geometric)}\\
H &= P_{-1} = m \big/ \textstyle\sum_j \tfrac{1}{x_j} && \text{(harmonic)}\\
P_p &= \Bigl(\tfrac{1}{m}\textstyle\sum_j x_j^{\,p}\Bigr)^{1/p},\; p \neq 0 && \text{(power / Hölder)}\\
R &= \mathrm{Uniform}\bigl(\min S,\; \max S\bigr) && \text{(random)}
\end{aligned}
$$

where $m = |I(i,x)|$ and $S$ is the confidence-set multiset. For strictly positive
inputs these means satisfy the **systematic inequality** of the paper:

$$P_{-\infty} = \min \;\le\; H = P_{-1} \;\le\; G = P_0 \;\le\; A = P_1 \;\le\; P_p \;\le\; P_{+\infty} = \max \quad (p \ge 1).$$

Geometric and harmonic means are undefined at zero, so they require opinions in an
open positive interval. The averaging math is ported verbatim (math-identical) from
the `hegselmann2005` replication's `means.rs`.

## 3. Data flow

![hegselmann_krause data flow](../assets/mech-hegselmann-krause.svg)

The mechanism reads `opinion(i)` and `neighbors_of(i)` from a start-of-step
snapshot, filters neighbours to the confidence set, aggregates with the configured
mean, and batch-writes the new opinions back via `set_opinion`. No other state is
touched.

## 4. Position in the 6-phase loop

Runs in **Interaction**, the phase where agents influence one another. Opinion
change *is* the interaction here, so this is its natural home.

- It reads a snapshot of all opinions taken at the start of its `apply` call, then
  writes every agent's new opinion in a single batch — making the update synchronous
  (simultaneous) and independent of the scheduler's activation order.
- Self is always included in its own confidence set (`{i}` is unconditionally added),
  matching the canonical HK definition.

Because it both reads and writes only the scalar opinion, two opinion-mutating
mechanisms in the same Interaction phase would compose sequentially; in the BC
literature HK is normally the *only* opinion updater in a run.

## 5. State read/write contract

| Field | Read | Write | Notes |
|---|:--:|:--:|---|
| `opinion(i)` (`ScalarOpinions`) | ✓ | ✓ | Snapshotted at step start; overwritten with the confidence-set mean. |
| `neighbors_of(i)` (`Neighbors`) | ✓ | | Influence pool before ε-filtering; self is added by the mechanism. |

## 6. Dependencies & ordering constraints

- **Upstream:** none. It needs only a world implementing `ScalarOpinions +
  Neighbors`; the topology (complete graph, ring, network, lattice) is the
  world's concern via `neighbors_of`.
- **Downstream:** an optional [`ConvergenceMechanism`] (PostStep) can stop the run
  once `max|Δx| < tol`; the free helper `max_abs_delta(prev, curr)` exposes the same
  test for driver-side loops. Convergence detection is meaningful only for the
  deterministic means (A/G/H/P) — the `Random` mean need not reach a fixed point.

## 7. Parameters

| Param | Type | Default | Meaning |
|---|---|---|---|
| `epsilon` (ε) | `f64` | `0.2` | Symmetric confidence bound. Larger ε → fewer, larger clusters (→ consensus). |
| `mean` | `MeanOperator` | `Arithmetic` | Averaging operator over the confidence set: `Arithmetic` (A), `Geometric` (G), `Harmonic` (H), `Power(p)` (P_p), `Random` (R). |

There is no ModulePack and therefore no scenario-TOML param block; both fields are
constructor arguments.

## 8. How to apply

This mechanism is **library-mode only** — there is no scenario-TOML registration.
Provide a world implementing `ScalarOpinions + Neighbors`, construct the
mechanism, and add it to a `SimulationBuilder`.

```rust
use socsim_core::{AgentId, ScalarOpinions, Neighbors, WorldState, SimClock};
use socsim_mechanisms::{HegselmannKrauseMechanism, MeanOperator};
use socsim_engine::{SequentialScheduler, SimulationBuilder};

// A world carrying one scalar opinion per agent (e.g. over a complete graph).
struct OpinionWorld { clock: SimClock, opinions: Vec<f64> }

impl WorldState for OpinionWorld {
    fn agent_ids(&self) -> Vec<AgentId> {
        (0..self.opinions.len() as u64).map(AgentId).collect()
    }
    fn clock(&self) -> &SimClock { &self.clock }
    fn clock_mut(&mut self) -> &mut SimClock { &mut self.clock }
}
impl ScalarOpinions for OpinionWorld {
    fn opinion(&self, id: AgentId) -> f64 { self.opinions[id.0 as usize] }
    fn set_opinion(&mut self, id: AgentId, v: f64) { self.opinions[id.0 as usize] = v; }
}
impl Neighbors for OpinionWorld {
    fn neighbors_of(&self, id: AgentId) -> Vec<AgentId> {
        self.agent_ids().into_iter().filter(|&j| j != id).collect()
    }
}

// ε = 0.2 with the arithmetic mean — the canonical HK setting.
let hk = HegselmannKrauseMechanism::new(0.2, MeanOperator::Arithmetic);

let mut sim = SimulationBuilder::new(world)
    .scheduler(Box::new(SequentialScheduler))
    .seed(42)
    .add_mechanism(hk)
    .build();
sim.run()?;
```

Swap the operator to explore the 2005 generalisation, e.g.
`HegselmannKrauseMechanism::new(0.2, MeanOperator::Power(2.0))`. To stop on
convergence, also add a `ConvergenceMechanism::new(1e-9)`.

## 9. Determinism & RNG

**Deterministic** for the arithmetic, geometric, harmonic, and power means
(A/G/H/P): the update reads a fixed snapshot and writes a fixed batch, so the result
is order-independent and reproducible for a given world state — it does not touch
`ctx.rng`. The sole exception is `MeanOperator::Random` (R), which draws a uniform
sample from the confidence set's range via `ctx.rng`; with a fixed seed even that run
is reproducible, but it need not converge to a fixed point.

## 10. Expected behaviour

The dynamics are governed by ε relative to the spread of initial opinions:

- **Large ε** (≳ the opinion range): every agent stays in everyone's confidence set,
  so the arithmetic mean drives the whole population to a single **consensus**.
- **Small ε**: distant opinions never enter each other's confidence sets, so the
  population freezes into **multiple stable clusters** (fragmentation / polarisation),
  with the number of clusters growing as ε shrinks.

Choosing a non-arithmetic mean shifts the fixed points along the systematic
inequality (e.g. the harmonic mean biases clusters downward relative to the
arithmetic mean) without changing this qualitative consensus-vs-fragmentation
picture.

## 11. References

- Hegselmann, R., & Krause, U. (2002). Opinion dynamics and bounded confidence:
  models, analysis and simulation. *Journal of Artificial Societies and Social
  Simulation*, 5(3).
- Hegselmann, R., & Krause, U. (2005). Opinion dynamics driven by various ways of
  averaging. *Computational Economics*, 25(4), 381–405.
