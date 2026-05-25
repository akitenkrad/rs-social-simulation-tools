**English** | [日本語](deffuant.ja.md)

# Deffuant (`deffuant`)

> On each random pairwise encounter, two agents within a confidence bound ε move
> toward each other by a fraction μ of the gap between them.
> **Phase:** Interaction. **Source:** Deffuant, Neau, Amblard & Weisbuch (2000). **Kind:** bounded-confidence (ε, μ).

[← Back to the mechanism catalog](../mechanisms.md)

## 1. Overview

`deffuant` is the **pairwise / event-based** member of the bounded-confidence (BC)
family in the general `socsim-social-dynamics` pack. Where Hegselmann–Krause updates
*every* agent simultaneously against its whole confidence set, Deffuant updates *two*
agents at a time: each step it performs `pairs_per_step` random encounters, drawing an
agent `i` and a random neighbour `j`. If their opinions lie within a symmetric
tolerance ε of each other, both step a fraction μ of the way toward the other; if
they are further than ε apart, nothing happens.

Repeated over many encounters this reproduces the same qualitative regimes as HK — a
wide ε drives the population to consensus, a narrow ε freezes it into multiple
clusters — but stochastically and asynchronously rather than in lockstep.

The mechanism is **library-only**: it operates over any world implementing the
`ScalarOpinions` and `Neighbors` capability traits from `socsim-core`. There
is **no `ModulePack`** for it (no scenario-TOML registration); construct it directly
and add it to a `SimulationBuilder`.

## 2. Theory & source

Deffuant, Neau, Amblard & Weisbuch (2000) proposed a *pairwise* model of continuous
opinion mixing under bounded confidence. At each interaction a random pair $(i, j)$
is selected; if the opinions are close enough they partially converge, otherwise they
do not interact. With confidence bound ε and convergence rate μ, the update on an
interacting pair is

$$
\begin{aligned}
x_i' &= x_i + \mu\,(x_j - x_i),\\
x_j' &= x_j + \mu\,(x_i - x_j),
\end{aligned}
\qquad \text{applied iff } |x_i - x_j| \le \varepsilon .
$$

Both deltas use the **pre-update** values $x_i, x_j$, so the pair contracts
symmetrically: the pair's mean $\tfrac{1}{2}(x_i + x_j)$ is conserved, and the gap
between them shrinks by a factor $(1 - 2\mu)$. The rate μ therefore lives in
$(0, 0.5]$: at $\mu = 0.5$ the pair jumps straight to its mean; smaller μ blends more
gradually. socsim's update is the `bc` rule of the `mou2024` reference applied
pairwise (math-identical port).

## 3. Data flow

![deffuant data flow](../assets/mech-deffuant.svg)

For each of `pairs_per_step` encounters the mechanism draws `i` (from the scheduler's
activation order, or the world roster as a fallback) and a random neighbour `j` from
`neighbors_of(i)` (excluding `i`), reads `opinion(i)` and `opinion(j)`, and — if
they are within ε — writes both new opinions via `set_opinion`. No other state is
touched.

## 4. Position in the 6-phase loop

Runs in **Interaction**, the phase where agents influence one another. Opinion
exchange *is* the interaction.

- Unlike HK, the update is **asynchronous and sequential within the step**: each of
  the `pairs_per_step` encounters reads and writes the live opinions, so a later
  encounter in the same step sees the effect of an earlier one.
- This is the BC literature's standard event-based idiom — many micro-events
  (encounters) batched into a single `apply` call mapped onto one engine tick (see
  the [event-driven / sub-tick](../architecture.md#event-driven--sub-tick-models)
  pattern). The engine tick is the observation cadence; `pairs_per_step` sets how
  many pair updates happen per observation.

## 5. State read/write contract

| Field | Read | Write | Notes |
|---|:--:|:--:|---|
| `opinion(i)`, `opinion(j)` (`ScalarOpinions`) | ✓ | ✓ | Read live each encounter; both overwritten iff `|x_i − x_j| ≤ ε`. |
| `neighbors_of(i)` (`Neighbors`) | ✓ | | Candidate partners for `i` (self excluded). |

## 6. Dependencies & ordering constraints

- **Upstream:** none. It needs only a world implementing `ScalarOpinions +
  Neighbors`; the topology (complete graph, ring, network, lattice) is the
  world's concern via `neighbors_of`.
- **Downstream:** the optional [`ConvergenceMechanism`] (PostStep) and the
  `max_abs_delta` helper apply, but note that a stochastic update need not reach an
  exact fixed point, so a convergence test may stop early or never — prefer a step
  budget for Deffuant.

## 7. Parameters

| Param | Type | Default | Meaning |
|---|---|---|---|
| `epsilon` (ε) | `f64` | `0.2` | Symmetric confidence bound. Larger ε → fewer, larger clusters (→ consensus). |
| `mu` (μ) | `f64` | `0.5` | Convergence rate in `(0, 0.5]`; each agent moves a fraction μ of the gap. |
| `pairs_per_step` | `usize` | `1` | Number of random pairwise encounters per step. |

There is no ModulePack and therefore no scenario-TOML param block; all three fields
are constructor arguments.

## 8. How to apply

This mechanism is **library-mode only** — there is no scenario-TOML registration.
Provide a world implementing `ScalarOpinions + Neighbors`, construct the
mechanism, and add it to a `SimulationBuilder`. (The world boilerplate is identical
to the [Hegselmann–Krause example](hegselmann-krause.md#8-how-to-apply).)

```rust
use socsim_social_dynamics::DeffuantMechanism;
use socsim_engine::{RandomActivationScheduler, SimulationBuilder};

// ε = 0.2, μ = 0.5, 50 pairwise encounters per step.
let deffuant = DeffuantMechanism::new(0.2, 0.5, 50);

let mut sim = SimulationBuilder::new(world) // world: ScalarOpinions + Neighbors
    .scheduler(Box::new(RandomActivationScheduler))
    .seed(42)
    .add_mechanism(deffuant)
    .build();
sim.run()?;
```

Increase `pairs_per_step` to advance the dynamics faster per tick; lower `mu` for
more gradual blending.

## 9. Determinism & RNG

**Stochastic**: every encounter draws the agent `i` and the partner `j` from
`ctx.rng`, so the trajectory depends on the RNG stream. Because all randomness flows
through `ctx.rng`, a fixed seed yields a fully reproducible run (verified by a
determinism test). Unlike the deterministic HK means, repeated stochastic encounters
need not settle to an exact fixed point, so pairing it with a `ConvergenceMechanism`
is discouraged.

## 10. Expected behaviour

As with HK, ε relative to the spread of initial opinions sets the regime:

- **Large ε** (≳ the opinion range): essentially every drawn pair interacts, and the
  μ-contractions drive the population to a single **consensus**; the global mean is
  conserved (symmetric exchange) and the spread is non-increasing.
- **Small ε**: distant opinions never interact, so the population settles into
  **multiple clusters** (fragmentation / polarisation), with more clusters as ε
  shrinks.

Compared with HK at the same ε, Deffuant's asynchronous pairwise mixing tends to
reach the same cluster structure but along a noisier, slower trajectory.

## 11. References

- Deffuant, G., Neau, D., Amblard, F., & Weisbuch, G. (2000). Mixing beliefs among
  interacting agents. *Advances in Complex Systems*, 3(01n04), 87–98.
