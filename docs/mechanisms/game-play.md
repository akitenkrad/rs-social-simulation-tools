**English** | [日本語](game-play.ja.md)

# Game play (`game_play`)

> Turns each encounter's action profile into payoffs, aggregates them per agent, and records the step.
> **Phase:** Interaction. **Source:** normal-form game payoffs. **Kind:** structural.

[← Back to the mechanism catalog](../mechanisms.md)

## 1. Overview

`game_play` is the third of the three [`socsim-games`](../games.md)
mechanisms. For every encounter written by `game_match` and filled by
`game_decide`, it

1. checks that every seat holds an action (an unfilled seat is an error);
2. computes one payoff per role with the configured `OutcomeRule`;
3. adds each payoff to the seated agent's `payoff` and increments its
   `n_played`, then applies the `PayoffAggregation` (`Sum` or `Mean`);
4. appends one `StepRecord { action, payoff, n_played }` per agent to the step
   history.

## 2. Theory & source

In a normal-form game each role $r$ receives $u_r(a_1, \dots, a_n)$ for the
action profile $a$. When an agent plays several encounters in one step, its
step payoff is their sum or mean:

$$\pi_i^t = \sum_{e \ni i} u_{r_e(i)}\big(a^{e}\big) \quad (\texttt{Sum}), \qquad \pi_i^t = \frac{1}{n_i^t}\sum_{e \ni i} u_{r_e(i)}\big(a^{e}\big) \quad (\texttt{Mean})$$

- $e \ni i$ — the encounters agent $i$ is seated in this step; $r_e(i)$ its role in $e$.
- $n_i^t$ (`n_played`) — the number of those encounters.

`NormalFormOutcome` evaluates $u$ with `NormalFormGame::payoffs_into`. A custom
`OutcomeRule` may also use `X`, held actions, the topology and the encounter
(e.g. reputation-weighted payoffs).

## 3. Data flow

![game_play data flow](../assets/mech-game-play.svg)

Reads the encounters (`seats`, `credit`, `actions`) and, through
`OutcomeContext`, the game, `X`, held actions and topology. Writes the per-seat
payoffs, the per-agent `payoff` and `n_played`, and the step history.

## 4. Position in the 6-phase loop

Registers **Interaction** — after Decision (all actions are chosen) and before
Reward (rules learn from the outcome; your own Reward-phase mechanisms can read
`world.payoffs()`).

## 5. State read/write contract

| Field | Read | Write | Notes |
|---|:--:|:--:|---|
| `PlayArena.seats`, `credit`, `actions` | ✓ | | Every action must be `Some`. Only `Credit::All` is accepted. |
| `PlayArena.payoffs` | | ✓ | One payoff per seat. |
| `GameWorld.payoff` | ✓ | ✓ | Accumulated per agent, then divided by `n_played` under `Mean`. |
| `GameWorld.n_played` | ✓ | ✓ | Incremented once per encounter the agent is seated in. |
| step history (`StepRecord`) | | ✓ | One record per agent; `action` is `None` for an unseated agent. Ring of length `history_len`. |
| `game`, `x`, `held`, `topology`, `clock.t` | ✓ | | Through `OutcomeContext`. |

## 6. Dependencies & ordering constraints

- **Upstream:** `game_match` (PreStep) zeroes `payoff` / `n_played`; without it
  payoffs would accumulate across steps. `game_decide` (Decision) must fill
  every seat.
- **Downstream:** `game_decide` (Reward) reads the new step record and
  encounter outcomes for `learn_step`; `DecisionContext::my_history` and
  `opponent_history` in the next step read the history written here.

## 7. Parameters

| Parameter | Default | Kind | Notes |
|---|---|---|---|
| `outcome: O` (`O: OutcomeRule<G, X>`) | — | structural | Passed to `assemble`. Built-in: `NormalFormOutcome` (requires `G: NormalFormGame`). |
| `opts.payoff_aggregation` | `PayoffAggregation::Sum` | structural | `Sum` or `Mean`; agents that played nothing keep `0` under `Mean`. |

## 8. How to apply

Library-only; `assemble` constructs `game_play` from `outcome` and `opts`.

### Library mode

```rust
use socsim_games::{assemble, FixedPlayers, NormalFormOutcome, PayoffAggregation, PlayOptions};

let opts = PlayOptions::new().payoff_aggregation(PayoffAggregation::Mean);
let mut sim = assemble(world, FixedPlayers::pairs(&[(0, 1)]), rules, NormalFormOutcome, opts, seed)?
    .build();
sim.step()?;
let step_payoffs = sim.world().payoffs();          // reset every step
let last = sim.world().history(socsim_core::AgentId(0)).and_then(|h| h.last().cloned());
```

## 9. Determinism & RNG

Draws **no** randomness. Encounters are evaluated in `EncounterId` order and
records are written in sorted `AgentId` order, so the result is fully
determined by the actions chosen in Decision.

## 10. Expected behaviour

In the prisoner's dilemma with $(T, R, P, S) = (5, 3, 1, 0)$, a step in which
agent 0 cooperates and agent 1 defects gives `payoffs() == [0.0, 5.0]` and
`n_played() == [1, 1]`.

## 11. References

- von Neumann, J., & Morgenstern, O. (1944). *Theory of Games and Economic Behavior*. Princeton University Press.
