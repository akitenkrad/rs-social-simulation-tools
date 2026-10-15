**English** | [日本語](replicator-update.ja.md)

# Proportional imitation (`replicator_update`)

> Every held agent compares itself with one random partner and copies a better-paid partner with a probability proportional to the payoff difference, all agents at once.
> **Phase:** Reward. **Source:** Schlag (1998). **Kind:** stochastic imitation.

[← Back to the mechanism catalog](../mechanisms.md)

## 1. Overview

`replicator_update` is one of the four population dynamics of
[`socsim-games`](../games.md#population-dynamics). It is registered by
`assemble_with` when its `dynamics` argument is
`ProportionalImitation::new(beta, update)?`, and it works on a world whose
agents are all **held**. After the step has been played, each agent $i$

1. draws one comparison partner $j$ — uniformly among its topology neighbours,
   or, in a world without topology, among all other agents;
2. adopts $j$'s held action with probability $\beta\,[\pi_j - \pi_i]_+$ — never
   when $j$ earned less;
3. — for every agent at once — writes the result to `held`.

The mechanism is named after the dynamics it approximates: in a large
well-mixed population the expected change of the action shares under this rule
is a discrete form of the replicator dynamics.

## 2. Theory & source

Schlag (1998) derives proportional imitation as the improving imitation rule of
a boundedly rational agent that observes one other agent: switch to the
observed action with a probability proportional to how much better it did,

$$P(i \leftarrow j) = \beta\,\big[\pi_j - \pi_i\big]_+ ,\qquad [x]_+ = \max(x, 0)$$

- $\pi_i$ — agent $i$'s payoff of the step (`world.payoffs()`, after
  `PayoffAggregation`).
- $\beta > 0$ — the imitation rate.

The expected share $x_k$ of action $k$ then changes in proportion to
$x_k\,(\bar\pi_k - \bar\pi)$, the replicator equation of Taylor & Jonker (1978)
in discrete time. For the probability to stay at most one for every payoff
pair, $\beta$ must satisfy

$$\beta \le \frac{1}{\Delta\pi}$$

where $\Delta\pi$ is the width of the attainable **per-step** payoff range.
`assemble_with` computes $\Delta\pi$ and rejects a larger $\beta$ (see
[Conditions checked at assembly](../games.md#conditions-checked-at-assembly));
at run time the probability is additionally capped at one.

## 3. Data flow

![replicator_update data flow](../assets/mech-replicator-update.svg)

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
  payoffs.
- **A bounded payoff range:** the play path must be
  `Pairwise::new(NormalFormOutcome)` (range over every action profile, at most
  10⁶) or `counting_play()` (range over every count vector). A user-defined
  `OutcomeRule` cannot be bounded and is rejected.
- **Encounter bounds under `Sum`:** with `PayoffAggregation::Sum` the matching
  must report `encounter_bounds` (every population matching does;
  `FixedPlayers`, `RoundRobin` and custom matchings by default do not). Use
  `PayoffAggregation::Mean` otherwise.
- **Held agents only** and a **common action space**, as for every dynamic
  (`GameError::Population`).
- **Finite games only:** `ProportionalImitation` requires a `FiniteGame`; with
  a [continuous game](../games.md#population-dynamics-with-continuous-actions)
  the call does not compile, because the bound $\beta \le 1/\Delta\pi$ needs
  the payoff range over every action profile. Use `fermi_update`,
  `imitate_best` or `moran_update` for continuous actions.
- **Downstream:** the next step plays the new held actions;
  [`game_metrics`](game-metrics.md) with
  `MetricsOptions::population_share(true)` records the updated shares.

## 7. Parameters

| Parameter | Default | Kind | Notes |
|---|---|---|---|
| `beta: f64` | — (required) | tunable | The imitation rate $\beta$. `ProportionalImitation::new` requires it finite and `> 0`; `assemble_with` requires $\beta \le 1/\Delta\pi$ (`GameError::Population`). |
| `update: Update` | — (required) | structural | `Update::Synchronous` here; it has no default so that adding schedules cannot change an existing configuration. `Update::Asynchronous` is rejected by `assemble_with` and is used only through `assemble_async`, which runs this rule as [`async_population_step`](async-population-step.md). |

How $\Delta\pi$ follows from the one-encounter range $[p_{\min}, p_{\max}]$
and the matching's `encounter_bounds` `min..max`:

| Aggregation | Per-step range |
|---|---|
| `Sum` | spanned by $c \cdot p$ for $c \in \{\texttt{min}, \texttt{max}\}$, $p \in \{p_{\min}, p_{\max}\}$ |
| `Mean` | $[p_{\min}, p_{\max}]$ |
| either, with `min = 0` | the range above extended to include $0$ |

For hawk–dove with $V = 2$, $C = 4$ ($[-1, 2]$ per encounter):
`RandomPairs` gives $\Delta\pi = 3$ and $\beta \le 1/3$; a Moore lattice with
self-play (9 encounters) gives $[-9, 18]$ and $\beta \le 1/27$ under `Sum`.

## 8. How to apply

Library-only. Pass the configuration as the `dynamics` argument of
`assemble_with`:

### Library mode

```rust
use socsim_games::catalog::HawkDove;
use socsim_games::{
    assemble_with, NormalFormOutcome, Pairwise, PlayOptions, ProportionalImitation, RandomPairs,
    Update,
};

let game = HawkDove::new(2.0, 4.0)?;                       // one encounter pays in [-1, 2]
let world = build_world(game, n, (), initial, &rules, &held, 1, steps, seed)?;
let mut sim = assemble_with(
    world,
    RandomPairs::new(),
    rules,                                                 // an empty RuleBook: every agent is held
    Pairwise::new(NormalFormOutcome),
    ProportionalImitation::new(1.0 / 3.0, Update::Synchronous)?, // beta = 1 / 3
    PlayOptions::new(),
    seed,
)?
.build();
```

The complete programs are the hawk–dove example of
[`docs/games.md` § Population dynamics](../games.md#population-dynamics) and
`crates/socsim-games/examples/hawk_dove_large.rs`.

## 9. Determinism & RNG

Draws from the engine stream (`derive_seed(seed, &[1])`, the step RNG). Agents
are visited in ascending `AgentId` order; each draws one partner with
`gen_range` and then one uniform `f64`, and adopts the partner's action when
the draw is below $\min(1, \beta\,[\pi_j - \pi_i]_+)$. An agent without
neighbours draws nothing. The mechanism keeps no state between steps, so a
snapshot resumes it exactly.

## 10. Expected behaviour

Well-mixed hawk–dove with $V = 2$, $C = 4$, `RandomPairs`, $\beta = 1/3$:

- the hawk share moves to the evolutionarily stable share $V/C = 0.5$ and
  fluctuates around it. With $N = 10^4$ agents, 30 % initial hawks and 20
  seeds, the 95 % $t$ interval of the per-seed means of
  `game.population_share.a0` over steps 201–1000 lies inside $[0.45, 0.55]$
  (the seeds average 0.49986);
- `hawk_dove_large` with $10^6$ agents starting at 0.5 ends ten steps later at
  a hawk share of 0.499876;
- requesting $\beta = 0.5$ fails at assembly with
  `replicator_update requires beta <= 1 / 3 (the per-step payoff range), got 0.5`.

## 11. References

- Schlag, K. H. (1998). Why imitate, and if so, how? A boundedly rational approach to multi-armed bandits. *Journal of Economic Theory*, 78(1), 130–156.
- Taylor, P. D., & Jonker, L. B. (1978). Evolutionary stable strategies and game dynamics. *Mathematical Biosciences*, 40(1–2), 145–156.
