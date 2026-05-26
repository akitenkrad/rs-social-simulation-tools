**English** | [日本語](group-conformity.ja.md)

# Group conformity (`group_conformity`)

> Every agent synchronously moves a fraction α of the way toward the mean opinion
> of its own group.
> **Phase:** Interaction. **Source:** DeGroot (1974). **Kind:** theory (within-group averaging).

[← Back to the mechanism catalog](../mechanisms.md)

## 1. Overview

`group_conformity` is the **group-dynamics** opinion mechanism shipped by the
general `socsim-mechanisms` crate. Once per step it performs a **synchronous**
update: it snapshots every agent's scalar opinion, computes the *mean opinion of
each group*, and for each agent `i` nudges that opinion a fraction α of the way
toward the mean of `i`'s own group. An agent is therefore pulled only toward the
agents it *shares a group with* — the group partition is the influence set, and
conformity to the group's average is the dynamic.

Because every new opinion is computed from the *same* start-of-step snapshot (all
group means are taken first, then every opinion is written), the result is
independent of agent activation order — the simultaneous (parallel) DeGroot
consensus update, restricted to within-group influence. With `α = 1` each agent
jumps straight onto its group mean in a single step; with a small α the groups
relax toward their means gradually. Crucially, the averaging step **preserves
each group's mean exactly**, so disjoint groups converge *independently* to their
own (conserved) means and never mix.

The mechanism is **library-only**: it operates over any world that implements the
`GroupMembership` and `ScalarOpinions` capability traits from `socsim-core`.
There is **no `ModulePack`** for it (it ships no scenario-TOML registration);
construct it directly and add it to a `SimulationBuilder`.

## 2. Theory & source

DeGroot (1974, "Reaching a Consensus") modelled a group of individuals who
repeatedly revise their opinion to a **weighted average** of the opinions in their
influence set; under mild connectivity conditions the process converges to a
consensus. `group_conformity` is this averaging-consensus rule with the influence
set fixed to be *the agent's own group* — i.e. a model of **within-group
conformity**: members assimilate toward their group's prevailing opinion.

Write the group of agent `i` as $g = g(i)$ and that group's member set as
$M_g = \{\, j : g(j) = g \,\}$. The group's mean opinion at opinion profile $x$ is

$$\mu_g(x) = \frac{1}{|M_g|} \sum_{j \in M_g} x_j ,$$

and the synchronous conformity update for each agent is

$$x_i' = x_i + \alpha\,\bigl(\mu_{g(i)}(x) - x_i\bigr), \qquad \alpha \in [0, 1].$$

This is a relaxation toward the group mean by a fraction α. Two structural
properties follow directly:

$$
\begin{aligned}
\text{(conservation)}\quad & \frac{1}{|M_g|}\sum_{i \in M_g} x_i' = \mu_g(x) && \text{(the group mean is unchanged each step),}\\
\text{(contraction)}\quad & \max_{i,j \in M_g} |x_i' - x_j'| = (1-\alpha)\,\max_{i,j \in M_g} |x_i - x_j| && \text{(within-group spread shrinks by } 1-\alpha).
\end{aligned}
$$

For $0 < \alpha \le 1$ every group therefore converges geometrically to its own
mean, and because $\mu_g$ depends only on members of $g$, distinct groups evolve
**independently** — there is no cross-group coupling. Setting one single group
recovers a global DeGroot consensus on the population mean; the special case
$\alpha = 1$ snaps each member onto the group mean in one step.

## 3. Data flow

![group_conformity data flow](../assets/mech-group-conformity.svg)

The mechanism reads `opinion(i)`, `group_of(i)`, and each group's members
(`group_members(g)` over `groups()`) from a start-of-step snapshot, computes every
group mean, nudges each agent toward its own group's mean by α, and batch-writes
the new opinions back via `set_opinion`. No other state is touched.

## 4. Position in the 6-phase loop

Runs in **Interaction**, the phase where agents influence one another. Conformity
to the group *is* the interaction here, so this is its natural home.

- It reads a snapshot of all opinions taken at the start of its `apply` call,
  computes every group's mean from that snapshot, then writes every agent's new
  opinion in a single batch — making the update synchronous (simultaneous) and
  independent of the scheduler's activation order.
- Each agent's own opinion is part of its group's mean (it is one of the
  members), matching the DeGroot averaging definition.

Because it both reads and writes only the scalar opinion, two opinion-mutating
mechanisms in the same Interaction phase would compose sequentially; in a pure
group-conformity run this is normally the only opinion updater.

## 5. State read/write contract

| Field | Read | Write | Notes |
|---|:--:|:--:|---|
| `opinion(i)` (`ScalarOpinions`) | ✓ | ✓ | Snapshotted at step start; overwritten with `x_i + α·(μ_g − x_i)`. |
| `group_of(i)` (`GroupMembership`) | ✓ | | Selects which group's mean agent `i` conforms to. |
| `group_members(g)` (`GroupMembership`) | ✓ | | Members aggregated into the group mean `μ_g`. |
| `groups()` (`GroupMembership`) | ✓ | | Enumerates the groups whose means are computed. |

## 6. Dependencies & ordering constraints

- **Upstream:** none. It needs only a world implementing `GroupMembership +
  ScalarOpinions`; the partition (team index, community label, spatial block, …)
  is the world's concern via `group_of` / `group_members` / `groups`.
- **Downstream:** an optional [`ConvergenceMechanism`] (PostStep) can stop the run
  once `max|Δx| < tol`; the free helper `max_abs_delta(prev, curr)` exposes the
  same test for driver-side loops. Convergence detection is meaningful here
  because the update is deterministic and contracting for `0 < α ≤ 1`.

The three `GroupMembership` accessors must be mutually consistent — `group_of` of
any member returned by `group_members(g)` is `g`, and every group an agent maps to
appears in `groups()`.

## 7. Parameters

| Param | Type | Default | Meaning |
|---|---|---|---|
| `alpha` (α) | `f64` | `0.3` | Conformity rate, clamped to `[0, 1]`. Fraction of the gap to the group mean closed each step: `0` freezes opinions, `1` snaps each agent onto its group mean per step. |

There is no ModulePack and therefore no scenario-TOML param block; `alpha` is a
constructor argument and is clamped to `[0, 1]` at construction.

## 8. How to apply

This mechanism is **library-mode only** — there is no scenario-TOML registration.
Provide a world implementing `GroupMembership + ScalarOpinions`, construct the
mechanism, and add it to a `SimulationBuilder`.

```rust
use socsim_core::{AgentId, GroupId, GroupMembership, ScalarOpinions, WorldState, SimClock};
use socsim_mechanisms::GroupConformityMechanism;
use socsim_engine::{SequentialScheduler, SimulationBuilder};

// A world carrying one scalar opinion per agent and a fixed group partition.
struct GroupWorld { clock: SimClock, opinions: Vec<f64>, group: Vec<GroupId> }

impl WorldState for GroupWorld {
    fn agent_ids(&self) -> Vec<AgentId> {
        (0..self.opinions.len() as u64).map(AgentId).collect()
    }
    fn clock(&self) -> &SimClock { &self.clock }
    fn clock_mut(&mut self) -> &mut SimClock { &mut self.clock }
}
impl ScalarOpinions for GroupWorld {
    fn opinion(&self, id: AgentId) -> f64 { self.opinions[id.0 as usize] }
    fn set_opinion(&mut self, id: AgentId, v: f64) { self.opinions[id.0 as usize] = v; }
}
impl GroupMembership for GroupWorld {
    fn group_of(&self, id: AgentId) -> GroupId { self.group[id.0 as usize] }
    fn group_members(&self, g: GroupId) -> Vec<AgentId> {
        self.group.iter().enumerate()
            .filter(|&(_, &gg)| gg == g)
            .map(|(i, _)| AgentId(i as u64)).collect()
    }
    fn groups(&self) -> Vec<GroupId> {
        let mut gs = self.group.clone(); gs.sort_unstable(); gs.dedup(); gs
    }
}

// α = 0.3 — a moderate per-step pull toward the group mean.
let gc = GroupConformityMechanism::new(0.3);

let mut sim = SimulationBuilder::new(world)
    .scheduler(Box::new(SequentialScheduler))
    .seed(42)
    .add_mechanism(gc)
    .build();
sim.run()?;
```

Raise α toward `1.0` for faster within-group consensus. To stop on convergence,
also add a `ConvergenceMechanism::new(1e-9)`.

## 9. Determinism & RNG

**Deterministic.** The update reads a fixed start-of-step snapshot, computes group
means by summing members in sorted `AgentId` order, and writes a fixed batch, so
the result is order-independent and reproducible for a given world state — it does
**not** touch `ctx.rng`. The same initial state therefore yields the same
trajectory on every run.

## 10. Expected behaviour

The dynamics are governed by α and the group partition:

- **Within a group**, opinions converge geometrically (spread × `(1−α)` per step)
  to the group's mean, which is **conserved** by the averaging step. With `α = 1`
  the group reaches consensus in a single step.
- **Across groups**, evolution is **independent**: each group converges to its own
  mean and no opinion ever crosses a group boundary. A single all-encompassing
  group recovers a global consensus on the population mean.

## 11. References

- DeGroot, M. H. (1974). Reaching a consensus. *Journal of the American
  Statistical Association*, 69(345), 118–121.
