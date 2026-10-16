**English** | [日本語](game-metrics.ja.md)

# Game metrics (`game_metrics`)

> Records one row of per-step game statistics — action shares, payoffs, cooperation, regret, distance to equilibrium, public aggregates — and can end the run when play settles.
> **Phase:** PostStep. **Source:** game-theoretic summary statistics. **Kind:** observation.

[← Back to the mechanism catalog](../mechanisms.md)

## 1. Overview

`game_metrics` (`GameMetrics<G>`, `G: FiniteGame`) is the observation mechanism
of [`socsim-games`](../games.md). Unlike the other `game_*` mechanisms it is
**not** registered by `assemble`: you build it with
`GameMetrics::new(&game, MetricsOptions)` and add it to the builder. After every
step it

1. counts this step's seats in the play arena per action and writes
   `game.played_share.<id>` (and `game.coop_rate` when a cooperation label is
   configured; `game.action_entropy` under feature `metrics`);
2. writes `game.mean_payoff` and `game.payoff_sd` over the agents that played;
3. copies this step's public aggregate row as `game.aggregate.<name>` when the
   simulation was assembled with aggregates, and, with
   `population_share(true)`, writes `game.population_share.a<k>`, the share of
   all agents holding action `k`;
4. builds one mixed strategy per role from the configured `MixedSource` and
   writes `game.regret` and, with `EquilibriumTarget::Solve`, `game.eq_distance`;
5. updates its convergence counter and, once the shares have been stable for
   `window` steps, requests the end of the run.

A column whose value is undefined in a step is left out of the row; nothing is
ever recorded as NaN. The full column reference is in
[`docs/games.md` § Metrics](../games.md#metrics).

**Continuous games.** `GameMetrics` requires a `FiniteGame`. For a
[continuous game](../games.md#continuous-games) the same mechanism comes as
`ContinuousMetrics<G>` (`G: ContinuousGame`), built with
`ContinuousMetrics::new(&game, ContinuousMetricsOptions)`. It registers under
the same name, `game_metrics`, runs in PostStep and writes to the same table
`"game"`, so this page covers both forms. After every step the continuous
form

1. writes the mean and population standard deviation of every action
   component over the step's seats, `game.mean_action.<d>` and
   `game.action_sd.<d>` (per role, `game.mean_action.r<r>-<d>` and
   `game.action_sd.r<r>-<d>`, when the roles have different numbers of
   components);
2. writes `game.mean_payoff`, `game.payoff_sd` and `game.aggregate.<name>` as
   the finite form does;
3. with an equilibrium configured, writes `game.eq_distance`;
4. updates a **per-agent** convergence counter and, once no agent's action
   has moved by `tol` for `window` steps, requests the end of the run.

It writes no shares, `coop_rate`, `regret`, `action_entropy` or
`population_share`.

The per-role columns are not recorded today. A continuous game whose roles have
different numbers of components cannot be built into a world: `build_world`
checks every initial action against every role, and assembly checks every
fallback action against every role. The per-role columns therefore appear only
in `describe()`. A world that bypasses these checks, for example one obtained by
deserializing a `GameWorld` or loaded with `Simulation::restore`, is not
supported.

## 2. Theory & source

With $S_t$ the seats of step $t$, $a_s$ the action in seat $s$ and $r(s)$ its
role, the played shares are

$$p_t(k) = \frac{\lvert\{s \in S_t : a_s = k\}\rvert}{\lvert S_t\rvert} \ \ (\text{pooled ids}), \qquad p_t(r, k) = \frac{\lvert\{s \in S_t : r(s) = r,\ a_s = k\}\rvert}{\lvert\{s \in S_t : r(s) = r\}\rvert} \ \ (\text{per-role ids})$$

For a profile $\sigma = (\sigma_r)_r$ of one mixed strategy per role, with
$F_r$ role $r$'s expected payoff,

$$\text{regret}(\sigma) = \max_r \Big[\max_a F_r(a, \sigma_{-r}) - F_r(\sigma)\Big], \qquad \text{eq\_distance}(\sigma) = \min_{\sigma^* \in E} \sum_r \sum_a \big\lvert \sigma_r(a) - \sigma^*_r(a)\big\rvert$$

- Regret is zero exactly at a Nash equilibrium (Nash, 1951); it is the
  unilateral-deviation gain used throughout the learning-in-games literature
  (e.g. Hart & Mas-Colell, 2000).
- $E$ is the complete equilibrium set of a two-player game, found once by
  support enumeration.
- `game.action_entropy` is the Shannon (1948) entropy $-\sum_k p_t(k) \ln p_t(k)$
  in nats.

With $A$ the agents and $h_i$ agent $i$'s held action at PostStep, the
population share is

$$q_t(k) = \frac{\lvert\{i \in A : h_i = k\}\rvert}{\lvert A\rvert}$$

It differs from $p_t$ twice: it counts agents rather than seats, and under
[population dynamics](../games.md#population-dynamics) it describes the
actions **after** the step's update (those of the next step), while $p_t$
describes the actions played in the step.

**Continuous form.** With $x_s$ the action vector in seat $s$, $S_t^r$ the
seats of role $r$ and $x^*$ the configured equilibrium,

$$\bar x_{t,d} = \frac{1}{\lvert S_t\rvert} \sum_{s \in S_t} x_{s,d}, \qquad \text{action\_sd}_{t,d} = \sqrt{\frac{1}{\lvert S_t\rvert} \sum_{s \in S_t} \big(x_{s,d} - \bar x_{t,d}\big)^2}, \qquad \text{eq\_distance}_t = \sum_r \sum_d \Big\lvert \frac{1}{\lvert S_t^r\rvert} \sum_{s \in S_t^r} x_{s,d} - x^*_{r,d} \Big\rvert$$

`eq_distance` compares each role's mean action with that role's equilibrium
action, so it is zero exactly when every role's seats average to the
equilibrium. The convergence stop compares, step by step, the vector $v_t$
that concatenates every agent's played action in `AgentId` order (an agent
that did not play keeps its previous entry): the counter grows while
$\max_k \lvert v_t(k) - v_{t-1}(k)\rvert < \texttt{tol}$. It is a per-agent
criterion — a steady mean with moving individuals does not stop the run.

## 3. Data flow

![game_metrics data flow](../assets/mech-game-metrics.svg)

Reads the play arena (`seats`, `actions`) — or, on the implicit play path, the
per-role counts of the played actions that `game_play` kept for the step — the
held actions (for `population_share`), the per-agent `payoff` and
`n_played`, the last aggregate row and, for `MixedSource::Reported`, the
mixed-strategy reports. Writes one row to the recorder's table `"game"` and,
only when `stop` or `MixedSource::PlayedCumulative` is configured, its own
`MetricsState` in the world. Optional event logging writes `game.roster`,
`game.step_actions` and `game.play` through the same recorder.

## 4. Position in the 6-phase loop

Registers **PostStep**. By then `game_play` (Interaction) has filled the
payoffs and the aggregate row and `game_decide` (Reward) has written the
post-learning reports, so the mechanism sees the completed step. No game
mechanism reads its output; a mechanism you add after it in PostStep runs
after it.

## 5. State read/write contract

| Field | Read | Write | Notes |
|---|:--:|:--:|---|
| `PlayArena.seats`, `actions` | ✓ | | A seat's role is its index modulo the role count. Every action must be `Some` and inside its role. |
| per-role played-action counts | ✓ | | Read instead of the arena when the step ran on the implicit path (the arena is empty). Same shares, `coop_rate`, regret and cumulative counts. |
| `GameWorld.held` | ✓ | | Only with `population_share(true)`; a held action outside the common action set is an error. |
| `GameWorld.payoff`, `n_played` | ✓ | | Only agents with `n_played > 0` enter `mean_payoff` / `payoff_sd`. |
| `game` | ✓ | | Role and action counts are checked against the game passed to `new`; payoffs are used for regret. |
| aggregate table | ✓ | | Only the last row, and only if its `t` is this step's. |
| mixed-strategy reports, `PolicyTable` slots | ✓ | | `MixedSource::Reported` only. The reports must be this step's; `Held` seats count as pure strategies. |
| `MetricsState { prev, below, cumulative }` | ✓ | ✓ | Created on the first step when `stop` or `PlayedCumulative` is set; field `metrics_state` of the world (omitted from the world JSON otherwise). Survives snapshot/resume. |
| step history, `held` | ✓ | | Continuous form with `stop` only: each agent's played action of the step (the first seat's action under `PerEncounter`), or the previous entry, or — on the first step — the held action. |
| recorder table `"game"` | | ✓ | One `record_row` per step. |
| recorder event `game.roster` | | ✓ | With `log_actions(true)`, once on the first PostStep processed by this mechanism: `{ agents, kinds, decision_scope }`. A newly constructed mechanism emits it again after snapshot restore; readers accept an identical duplicate roster. |
| recorder event `game.step_actions` | | ✓ | With `log_actions(true)`, when `t % log_actions_every == 0`: `{ actions, roles, payoffs, n_played }`, all arrays in roster order. `actions` and `roles` may contain `null`. With `async_population_step`, actions and payoffs are the final values recomputed after all in-place updates. |
| recorder event `game.play` | | ✓ | With `log_plays(true)`, once per materialized encounter: `{ encounter, seats, actions, payoffs, credit }`. Finite actions are indices; continuous actions are arrays. |
| stop request | | ✓ | `StepContext::request_stop` when the convergence counter reaches `window`. |

Policy states, held actions, plays and payoffs are never written: adding
`game_metrics` does not change the trajectory of a run.

## 6. Dependencies & ordering constraints

- **Upstream:** `game_match` / `game_decide` / `game_play` must have run (any
  `assemble*` function). `game.aggregate.<name>` needs
  `assemble_with_aggregates`; `MixedSource::Reported` needs
  `PlayOptions::report_mixed(true)` (otherwise the first step fails), and every
  seated rule-driven agent must report a mix (`Fixed`, `MyopicBestResponse`,
  LLM kinds and rules without `mixed` do not).
- **Same game:** build it from the game you put into the world. Role and action
  counts are checked every step (mechanism error on a mismatch); the
  equilibria for `eq_distance` come from the game given to `new`.
- **One per simulation:** the world has a single `MetricsState`, so add one
  `game_metrics`.
- **Logging checks:** zero sampling intervals or population limits are rejected
  by `GameMetrics::new`. Checks that need the world — population size,
  `PerStep` action semantics, role ambiguity and a materialized play path — run
  before any event or metric row on the first PostStep. `log_plays` therefore
  fails on an implicit path and is unavailable with `async_population_step`.
- **Downstream:** no game mechanism reads its output. With `stop`, the engine
  ends the run after the step in which the stop was requested.

## 7. Parameters

| Parameter | Default | Kind | Notes |
|---|---|---|---|
| `cooperate_label(label)` | off | observation | Adds `game.coop_rate`. Must match exactly one action label in every role (`GameError::Metrics`). |
| `regret(on)` | on iff the game has two roles | observation | `game.regret`. |
| `mixed_source(MixedSource)` | `Played` | observation | `Played` (this step's seats), `PlayedCumulative` (all seats since the first step; kept in the world), `Reported` (seat-weighted average of post-learning reports). |
| `equilibrium(EquilibriumTarget)` | `None` | observation | `Solve`: support enumeration once in `new`; two roles only; rejected if the solver fails or the equilibrium set is incomplete or empty. Adds `game.eq_distance`. |
| `stop(tol, window)` | off | control | Stop after `window` consecutive seated steps whose largest share change is `< tol`. `tol` finite `> 0`, `window ≥ 1`. |
| `action_entropy(on)` | on | observation | Feature `metrics` only; pooled ids only. |
| `population_share(on)` | off | observation | Adds `game.population_share.a<k>` (agents holding `k` ÷ all agents, after any population dynamics). Every role must have the same number of actions (`GameError::Metrics`). |
| `log_actions(on)` | off | observation | Emits `game.roster` and sampled `game.step_actions` events. It does not sample agents or change the simulation trajectory. |
| `log_actions_every(every)` | `1` | observation | Emits `game.step_actions` when `t % every == 0`; `every ≥ 1`. |
| `log_actions_max_agents(maximum)` | `1000` | guard | Rejects action logging when the population is larger; `maximum ≥ 1`. |
| `log_plays(on)` | off | observation | Emits one `game.play` per materialized encounter. Requires `PlayOptions::materialize(true)` and is unavailable with `async_population_step`. |

The continuous form takes `ContinuousMetricsOptions`:

| Parameter | Default | Kind | Notes |
|---|---|---|---|
| `equilibrium(ContinuousEquilibrium)` | `None` | observation | `Solve`: `best_response_iteration` once in `new`, from the interval midpoints with the default options; rejected unless `Converged`; finds one equilibrium. `Given(profile)`: one finite action of the right length per role. Either adds `game.eq_distance`. |
| `stop(tol, window)` | off | control | Stop after `window` consecutive steps in which no component of any agent's played action changes by `tol` or more. `tol` finite `> 0`, `window ≥ 1`. |
| `log_plays(on)` | off | observation | Emits one `game.play` per materialized encounter, with continuous actions as arrays. Requires `PlayOptions::materialize(true)`. |

Invalid bounds or options, a mismatched `Given` profile and a solve that does
not converge are `GameError::Metrics` from `ContinuousMetrics::new`.

Action records are intentionally verbose. A useful estimate for one
`game.step_actions` event is about $(20N + 20)$ bytes for $N$ agents. Measured
records at $N = 50$ were 0.76--1.08 KB per recorded step: 76--108 MB over
$10^5$ recorded steps, or about 140 MB when the metric rows are included. In
the two-agent, 50-step example, `game.step_actions` was 135--136 bytes per
line, `game.play` 133--134 bytes per encounter and `game.roster` 140 bytes;
the complete 551-line file was 42,761 bytes. `log_plays` adds one line per
encounter. Increase `log_actions_every` to subsample long runs.

## 8. How to apply

Library-only. Build the mechanism, then add it to the builder returned by
`assemble` (or `assemble_with_aggregates` / `assemble_with_llm`):

### Library mode

```rust
use socsim_games::{EquilibriumTarget, GameMetrics, MetricsOptions, MixedSource};

let metrics = GameMetrics::new(           // before `game` moves into build_world
    &game,
    MetricsOptions::new()
        .cooperate_label("C")
        .mixed_source(MixedSource::PlayedCumulative)
        .equilibrium(EquilibriumTarget::Solve)
        .stop(1e-6, 20),
)?;
let columns = metrics.describe();         // (pattern, meaning) pairs for runvault.toml
let world = build_world(game, 2, (), InitialActions::Same(0), &rules, &Assignment::Ordered(vec![
    AssignGroup { kind: AssignKind::Rule("tit_for_tat".into()), count: 1 },
    AssignGroup { kind: AssignKind::Rule("fixed_defect".into()), count: 1 },
]), 100, 1000, seed)?;
let mut sim = assemble(world, matching, rules, NormalFormOutcome, PlayOptions::new(), seed)?
    .add_mechanism(Box::new(metrics))     // PostStep, after the game mechanisms
    .recorder(Box::new(recorder))
    .build();
sim.run()?;                               // may end before t_max via the stop rule
```

With Tit-for-Tat against a defector this run ends at `t = 22` (shares unchanged
from `t = 2`, twenty stable steps from `t = 3`).

For a continuous game:

```rust
use socsim_games::{ContinuousEquilibrium, ContinuousMetrics, ContinuousMetricsOptions};

let metrics = ContinuousMetrics::new(     // before `game` moves into build_world
    &game,
    ContinuousMetricsOptions::new()
        .equilibrium(ContinuousEquilibrium::Solve)
        .stop(1e-9, 10),
)?;
let mut sim = assemble(world, WholePopulation::new(), rules, NormalFormOutcome, PlayOptions::new(), seed)?
    .add_mechanism(Box::new(metrics))
    .build();
``` `describe()` lists the
columns of this configuration; see
[`docs/games.md` § Describing the columns](../games.md#describing-the-columns-describe)
for copying them into a runvault declaration.

## 9. Determinism & RNG

Draws **no** randomness and writes no model state, so a run with
`game_metrics` follows exactly the trajectory of the same run without it. The
row depends only on the completed step and on `MetricsState`, which is part of
the world; a resumed run therefore records the same values and stops at the
same step as an uninterrupted one.

## 10. Expected behaviour

In the prisoner's dilemma with $(T, R, P, S) = (5, 3, 1, 0)$, Tit-for-Tat
against an unconditional defector with `cooperate_label("C")`,
`EquilibriumTarget::Solve` and `stop(1e-9, 3)`:

- `t = 1`: `game.played_share.a0 = 0.5`, `game.played_share.a1 = 0.5`,
  `game.coop_rate = 0.5`, `game.mean_payoff = 2.5`, `game.payoff_sd = 2.5`,
  `game.regret = 1`, `game.eq_distance = 2`;
- `t ≥ 2`: `played_share.a1 = 1`, `coop_rate = 0`, `mean_payoff = 1`,
  `payoff_sd = 0`, `regret = 0`, `eq_distance = 0`;
- the run stops at `t = 5`.

Three Cournot firms ($P = 100 - Q$, unit cost 10, $q^* = 22.5$) learning by
`DampedBestResponse` with $\alpha = 0.5$ from `[0, 50, 100]` under
`WholePopulation`, assembled with aggregates, with
`ContinuousEquilibrium::Solve` (which finds `[[22.5], [22.5], [22.5]]`) and
`stop(1e-9, 10)`:

- `t = 1`: `game.mean_action.0 = 50`, `game.action_sd.0 ≈ 40.82`,
  `game.mean_payoff = -3000`, `game.aggregate.total_output = 150`,
  `game.aggregate.price = -50`, `game.eq_distance = 127.5`
  ($22.5 + 27.5 + 77.5$: each role's own seat against 22.5);
- `t = 2`: actions `[0, 25, 60]`, `game.mean_action.0 ≈ 28.33`,
  `game.eq_distance = 62.5`;
- the run stops at `t = 92` with `game.mean_action.0 = 22.5`,
  `game.mean_payoff = 506.25` and `game.eq_distance ≈ 3.6e-10`
  (`examples/cournot.rs` prints the same stop).

In the hawk–dove example of
[`docs/games.md` § Population dynamics](../games.md#population-dynamics)
(1 000 held agents, 300 initial hawks, `RandomPairs`, proportional imitation)
with `population_share(true)`, `t = 1` records `game.played_share.a0 = 0.3`
(the hawks that played) and `game.population_share.a0 = 0.319` (the hawks after
the first update).

## 11. References

- Nash, J. (1951). Non-cooperative games. *Annals of Mathematics*, 54(2), 286–295.
- Hart, S., & Mas-Colell, A. (2000). A simple adaptive procedure leading to correlated equilibrium. *Econometrica*, 68(5), 1127–1150.
- Shannon, C. E. (1948). A mathematical theory of communication. *Bell System Technical Journal*, 27(3), 379–423.
