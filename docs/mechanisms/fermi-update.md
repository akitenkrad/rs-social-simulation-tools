**English** | [日本語](fermi-update.ja.md)

# Fermi update (`fermi_update`)

> Every held agent compares itself with one random partner and adopts the partner's action with a probability given by the Fermi function of the payoff difference, all agents at once.
> **Phase:** Reward. **Source:** Szabó & Tőke (1998). **Kind:** stochastic imitation.

[← Back to the mechanism catalog](../mechanisms.md)

## 1. Overview

`fermi_update` is one of the four population dynamics of
[`socsim-games`](../games.md#population-dynamics). It is registered by
`assemble_with` when its `dynamics` argument is `FermiUpdate::new(k, update)?`,
and it works on a world whose agents are all **held**. After the step has been
played, each agent $i$

1. draws one comparison partner $j$ — uniformly among its topology neighbours,
   or, in a world without topology, among all other agents;
2. adopts $j$'s held action with probability
   $W = 1/(1 + \exp[-(\pi_j - \pi_i)/K])$, and otherwise keeps its own;
3. — for every agent at once — writes the result to `held`.

Unlike [`imitate_best`](imitate-best.md), a worse-paid partner is copied too,
with a probability below one half; the noise $K$ controls how sharply the
payoff difference decides.

## 2. Theory & source

Szabó & Tőke (1998) introduced the pairwise comparison in which player $x$
adopts the strategy of a neighbour $y$ with probability

$$W(\pi_x \to \pi_y) = \frac{1}{1 + \exp\!\big[-(\pi_y - \pi_x)/K\big]}$$

- $\pi_i$ — agent $i$'s payoff of the step (`world.payoffs()`, after
  `PayoffAggregation`).
- $K > 0$ — the noise of the comparison. $\beta = 1/K$ is the intensity of
  selection (Traulsen, Nowak & Pacheco, 2006): $K \to 0$ approaches the
  deterministic rule "copy a better partner", a large $K$ copying with
  probability one half regardless of payoffs.

$W$ lies in $(0, 1)$ for every payoff pair, so no bound on $K$ is needed beyond
$K$ being finite and positive. The implementation evaluates $W$ with
$z = (\pi_j - \pi_i)/K$ as $1/(1 + e^{-z})$ for $z \ge 0$ and
$e^{z}/(1 + e^{z})$ for $z < 0$, so the exponential never overflows. At equal
payoffs $W = 1/2$.

## 3. Data flow

![fermi_update data flow](../assets/mech-fermi-update.svg)

Reads the step's payoffs, the held actions and, when present, the topology's
neighbour lists. Draws a partner and a uniform number per agent from the step
RNG. Writes the held actions, through a buffer that is swapped in once every
agent has been evaluated.

## 4. Position in the 6-phase loop

Registers **Reward**, after `game_decide` (so after any learning). The
payoffs it reads were written in Interaction by `game_play`; at PostStep
`world.held()` holds the next step's actions while `world.payoffs()` still
describes the step just played (see
[What `PostStep` sees](../games.md#what-poststep-sees)).

## 5. State read/write contract

| Field | Read | Write | Notes |
|---|:--:|:--:|---|
| `GameWorld.payoff` | ✓ | | One payoff per agent; a non-finite payoff stops the step. |
| `GameWorld.held` | ✓ | ✓ | All next actions are computed from the pre-update `held` and written at once (`Update::Synchronous`). |
| `GameWorld.topology` | ✓ | | Partners are neighbours (out-neighbours on a directed network); with `Topology::None`, any other agent. |
| `PolicyTable.slot` | ✓ | | Checked once at assembly: every agent must be held. |

## 6. Dependencies & ordering constraints

- **Upstream:** `game_play` (Interaction) must have written this step's
  payoffs. Any matching and any topology (or none) works.
- **Held agents only:** a world in which some agent has a decision rule is
  rejected by `assemble_with` (`GameError::Population`).
- **Common action space:** the game must implement `CommonActionSpace`
  (compile time) and every role must have the same number of actions
  (`GameError::Population`).
- **Continuous games:** a `ContinuousGame` works as well; the partner's action
  vector is copied unchanged, and every role must have the same intervals
  (`GameError::Population`). See
  [Population dynamics with continuous actions](../games.md#population-dynamics-with-continuous-actions).
- **Partners and interaction partners are separate.** The comparison partner
  is drawn from the topology, not from this step's encounters; under
  `RandomPairs` in a world without topology an agent may compare itself with an
  agent it did not meet.
- **Downstream:** the next step plays the new held actions;
  [`game_metrics`](game-metrics.md) with
  `MetricsOptions::population_share(true)` records the updated shares.

## 7. Parameters

| Parameter | Default | Kind | Notes |
|---|---|---|---|
| `k: f64` | — (required) | tunable | The noise $K$. Must be finite and `> 0`, else `FermiUpdate::new` returns `GameError::Population`. No upper bound. |
| `update: Update` | — (required) | structural | `Update::Synchronous` here; it has no default so that adding schedules cannot change an existing configuration. `Update::Asynchronous` is rejected by `assemble_with` and is used only through `assemble_async`, which runs this rule as [`async_population_step`](async-population-step.md). |

## 8. How to apply

Library-only. Pass the configuration as the `dynamics` argument of
`assemble_with`:

### Library mode

```rust
use socsim_games::{
    assemble_with, FermiUpdate, NetworkEdges, NormalFormOutcome, Pairwise, PayoffAggregation,
    PlayOptions, Topology, Update,
};

let world = build_world(game, n, (), initial, &rules, &held, 1, steps, seed)?
    .with_topology(Topology::network(network))?;          // feature `net`
let mut sim = assemble_with(
    world,
    NetworkEdges::new(),                                   // one encounter per edge
    rules,                                                 // an empty RuleBook: every agent is held
    Pairwise::new(NormalFormOutcome),
    FermiUpdate::new(0.1, Update::Synchronous)?,           // K = 0.1
    PlayOptions::new().payoff_aggregation(PayoffAggregation::Mean),
    seed,
)?
.build();
```

In a well-mixed population, drop the topology and use a matching such as
`RandomPairs::new()` or `WholePopulation::new()`; see the El Farol example in
[`docs/games.md` § Anonymous games in a simulation](../games.md#anonymous-games-in-a-simulation-counting_play).

## 9. Determinism & RNG

Draws from the engine stream (`derive_seed(seed, &[1])`, the step RNG). Agents
are visited in ascending `AgentId` order; each draws one partner with
`gen_range` and then one uniform `f64`, and adopts the partner's action when
the draw is below $W$. An agent without neighbours draws nothing. The mechanism
keeps no state between steps, so a snapshot resumes it exactly, and two runs
with the same seed are identical step for step.

## 10. Expected behaviour

- With equal payoffs every agent copies its partner with probability exactly
  $1/2$; with $\pi_j - \pi_i = K \ln 3$ the probability is $3/4$.
- In the El Farol example of [`docs/games.md`](../games.md#anonymous-games-in-a-simulation-counting_play)
  (100 agents in one encounter, $K = 0.1$, 80 initial attendees), attendance is
  80, 60, 86 in the first three steps: attendees who earn $-1$ copy a
  stay-at-home partner with probability $1/(1 + e^{-10}) \approx 1$, and
  stay-at-homes copy an attendee earning $+1$ in the next step.
- On a random 4-regular network of 200 agents with
  `PrisonersDilemma::weak(1.5)` and $K = 0.1$, two runs with the same seed give
  identical `held`, payoffs and metric rows after every step, and a run resumed
  from a snapshot continues identically.

## 11. References

- Szabó, G., & Tőke, C. (1998). Evolutionary prisoner's dilemma game on a square lattice. *Physical Review E*, 58(1), 69–73.
- Traulsen, A., Nowak, M. A., & Pacheco, J. M. (2006). Stochastic dynamics of invasion and fixation. *Physical Review E*, 74(1), 011909.
