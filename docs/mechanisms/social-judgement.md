**English** | [日本語](social-judgement.ja.md)

# Social Judgement (`social_judgement`)

> Each agent assimilates neighbour opinions that fall inside its acceptance region
> (ε) and is repelled by those in its rejection region, producing polarisation.
> **Phase:** Interaction. **Source:** Social Judgement Theory. **Kind:** opinion dynamics (assimilation–contrast).

[← Back to the mechanism catalog](../mechanisms.md)

## 1. Overview

`social_judgement` is the **assimilation–contrast** member of the opinion-dynamics
family in the general `socsim-mechanisms` crate. Each agent carries a scalar
opinion in `[-1, 1]`. Once per step it performs a **synchronous** update: it snapshots
every agent's opinion, and for each agent `i` it classifies every neighbour message
`m_j = x_j` by the signed gap `diff = m_j − x_i` into three regions:

- **acceptance** (`|diff| < ε`): assimilate — move *toward* the message by `α · diff`;
- **rejection** (`|diff| > rejection`): repel — move *away* from the message by
  `repulsion · sign(diff)`;
- **non-commitment** (`ε ≤ |diff| ≤ rejection`): no contribution.

The per-agent delta is the **mean** over the contributing neighbours; the new opinion
`x_i + Δ` is clamped to `[-1, 1]` and batch-written. The repulsion term is what
drives **polarisation**: messages that are too far away push the agent in the opposite
direction, so opposing groups can diverge to the extremes.

The mechanism is **library-only**: it operates over any world implementing the
`ScalarOpinions` and `Neighbors` capability traits from `socsim-core`. There is **no
`ModulePack`** for it (no scenario-TOML registration); construct it directly and add
it to a `SimulationBuilder`.

## 2. Theory & source

Social Judgement Theory (Sherif & Hovland) models persuasion via three latitudes
around an attitude: a **latitude of acceptance** (messages close enough to assimilate
toward), a **latitude of rejection** (messages far enough to be contrasted *away*
from), and a **latitude of non-commitment** in between (no effect). Messages inside
acceptance produce assimilation; messages inside rejection produce a contrast effect
that pushes the attitude further from the message.

socsim renders this as a per-step opinion update. For agent `i` with opinion $x_i$
and neighbour messages $\{m_j\}$, the delta is the mean of the per-message
contributions:

$$
\Delta_i = \frac{1}{|C_i|} \sum_{j \in C_i} \delta_{ij},
\qquad
\delta_{ij} =
\begin{cases}
\alpha\,(m_j - x_i) & |m_j - x_i| < \varepsilon \quad \text{(accept)}\\
-\,\rho_{\text{rep}}\,\operatorname{sign}(m_j - x_i) & |m_j - x_i| > r \quad \text{(reject)}\\
0 & \text{otherwise (non-commitment)}
\end{cases}
$$

where $C_i$ is the set of contributing neighbours (those in the acceptance or
rejection region), $\varepsilon$ is the acceptance half-width, $r$ the rejection
threshold, $\alpha$ the assimilation rate, and $\rho_{\text{rep}}$ the repulsion
strength. The new opinion $x_i' = \operatorname{clamp}_{[-1, 1]}(x_i + \Delta_i)$.
The math is ported verbatim from the `mou2024` reference's `sj_update`.

## 3. Data flow

![social_judgement data flow](../assets/mech-social-judgement.svg)

The mechanism reads `opinion(i)` and the neighbour opinions (`neighbors_of(i)` →
`opinion(j)`, used as messages `m_j`) from a start-of-step snapshot, classifies each
message into accept / reject / non-commit, averages the contributions, and
batch-writes the clamped new opinions via `set_opinion`. No other state is touched.

## 4. Position in the 6-phase loop

Runs in **Interaction**, the phase where agents influence one another. Opinion change
*is* the interaction here.

- It reads a snapshot of all opinions taken at the start of its `apply` call, then
  writes every agent's new opinion in a single batch — making the update synchronous
  (simultaneous) and independent of the scheduler's activation order.
- Self is excluded from the message set (a neighbour `j == i` is skipped); only
  neighbour opinions act as messages.

Because it both reads and writes only the scalar opinion, two opinion-mutating
mechanisms in the same Interaction phase would compose sequentially.

## 5. State read/write contract

| Field | Read | Write | Notes |
|---|:--:|:--:|---|
| `opinion(i)` (`ScalarOpinions`) | ✓ | ✓ | Snapshotted at step start; overwritten with `clamp(x_i + Δ)`. |
| `neighbors_of(i)` (`Neighbors`) | ✓ | | Source of the messages `m_j = x_j` (self excluded). |

## 6. Dependencies & ordering constraints

- **Upstream:** none. It needs only a world implementing `ScalarOpinions +
  Neighbors`; the topology (complete graph, ring, network, lattice) is the world's
  concern via `neighbors_of`.
- **Downstream:** an optional [`ConvergenceMechanism`] (PostStep) and the
  `max_abs_delta` helper apply, but note that the repulsion term can drive opinions to
  the clamp boundaries and produce a polarised standoff rather than a single fixed
  point — a step budget is usually the clearer stopping rule.

## 7. Parameters

| Param | Type | Default | Meaning |
|---|---|---|---|
| `epsilon` (ε) | `f64` | `0.4` | Acceptance half-width: `|diff| < ε` ⇒ assimilate. |
| `alpha` (α) | `f64` | `0.5` | Assimilation rate applied to the in-region gap. |
| `rejection` (r) | `f64` | `0.8` | Rejection threshold: `|diff| > rejection` ⇒ repel. |
| `repulsion` | `f64` | `0.2` | Repulsion strength (magnitude of the away-from-message push). |

These are tunable behavioural scales, not empirical correlations. There is no
ModulePack and therefore no scenario-TOML param block; all four fields are constructor
arguments.

## 8. How to apply

This mechanism is **library-mode only** — there is no scenario-TOML registration.
Provide a world implementing `ScalarOpinions + Neighbors`, construct the mechanism,
and add it to a `SimulationBuilder`. (The world boilerplate is identical to the
[Hegselmann–Krause example](hegselmann-krause.md#8-how-to-apply).)

```rust
use socsim_mechanisms::SocialJudgementMechanism;
use socsim_engine::{SequentialScheduler, SimulationBuilder};

// ε = 0.4 acceptance, α = 0.5 assimilation, rejection = 0.8, repulsion = 0.2.
let sj = SocialJudgementMechanism::new(0.4, 0.5, 0.8, 0.2);

let mut sim = SimulationBuilder::new(world) // world: ScalarOpinions + Neighbors
    .scheduler(Box::new(SequentialScheduler))
    .seed(42)
    .add_mechanism(sj)
    .build();
sim.run()?;
```

Raise `repulsion` (or shrink the non-commitment band by lowering `rejection`) to
strengthen polarisation; lower it toward 0 to recover a pure assimilation dynamic.

## 9. Determinism & RNG

**Deterministic**: the update reads a fixed snapshot and writes a fixed batch, so the
result is order-independent and reproducible for a given world state — it does not
touch `ctx.rng`. (Any stochasticity, e.g. random initial opinions, lives in the
world, not the mechanism.)

## 10. Expected behaviour

The regime is set by the width of the regions relative to the opinion spread:

- **Wide acceptance, weak repulsion** (large ε, small `repulsion`): assimilation
  dominates and the population converges, much like a bounded-confidence model.
- **Narrow acceptance, strong repulsion** (small ε, large `repulsion`, low
  `rejection`): the rejection term dominates for distant pairs, pushing groups apart
  until they polarise to the `[-1, 1]` extremes — opinions at the boundaries are
  pinned by the clamp.

The non-commitment band acts as a buffer: messages there neither attract nor repel,
slowing both convergence and polarisation.

## 11. References

- Sherif, M., & Hovland, C. I. (1961). *Social Judgment: Assimilation and Contrast
  Effects in Communication and Attitude Change*. Yale University Press.
- Mou, X., et al. (2024). Opinion-dynamics agent-based models with assimilation,
  reinforcement, and polarisation mechanisms (the `mou2024` reference port).
