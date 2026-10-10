**English** | [日本語](game-decide.ja.md)

# Game decision (`game_decide`)

> Each seated agent's decision rule chooses an action; after play, the rules learn.
> **Phase:** Decision + Reward. **Source:** game-theoretic simulation (decision rules). **Kind:** rule-driven.

[← Back to the mechanism catalog](../mechanisms.md)

## 1. Overview

`game_decide` (the `RuleDriver` type) is the second of the three
[`socsim-games`](../games.md) mechanisms. It drives every agent's
`DecisionRule`:

- in **Decision** it asks each seated agent for an action — through its rule,
  or, for a `Held` agent, from its held action — checks the action with
  `StageGame::check_action`, stores it in the agent's seat, and updates the
  agent's held action;
- in **Reward** it calls `learn_step` for every rule-driven agent, with this
  step's record and encounter outcomes.

The rule objects themselves are shared code. Every agent's rule state lives in
the world's `PolicyTable`, so the mechanism holds no state that matters for a
resumed run.

`game_decide` drives only the kinds of its `RuleBook`. Which driver owns a kind
is fixed by the `KindCatalog`: with the `llm` feature, kinds registered in an
`LlmRuleBook` belong to [`game_llm_decide`](game-llm-decide.md), and
`game_decide` skips those agents in both Decision and Reward, leaving their
seats for `game_llm_decide`, which `assemble_with_llm` registers right after
`game_decide`.

## 2. Theory & source

A decision rule is a behavioural strategy for the repeated game: a map from
what the agent knows to an action, plus an update of its internal state.

$$a_i^t = \text{decide}_{k(i)}\big(s_i,\ \text{view}_i^t;\ \text{rng}\big), \qquad s_i \leftarrow \text{learn}_{k(i)}\big(s_i,\ \text{outcome}_i^t\big)$$

- $k(i)$ — the rule kind assigned to agent $i$ (`PolicyTable.kind`).
- $s_i$ — agent $i$'s rule state (`X::PolicyState`).
- $\text{view}_i^t$ — the `DecisionContext`: own role and encounter, current
  opponents, own history, opponents' action histories, and `X::view_for(i)`.

Built-in rules: `Fixed` (a constant action), `UniformRandom` (uniform over the
role's actions) and `TitForTat` (Axelrod, 1984: cooperate first, then copy the
opponent's last action). A rule is distinct from a `socsim_marl::Policy`: a
`DecisionRule` is a game-layer strategy over `G::Action` with its state stored
in the world, not a neural policy over an observation vector.

## 3. Data flow

![game_decide data flow](../assets/mech-game-decide.svg)

Decision reads the seats written by `game_match`, each agent's rule kind and
state, held actions, history and `X`, and writes the chosen actions into the
seats, the held actions and the rule states (`decide` may mutate its state).
Reward reads the step's records and encounter outcomes written by `game_play`
and updates the rule states.

## 4. Position in the 6-phase loop

Registers **Decision** and **Reward**.

- **Decision** — after `game_match` in the same phase (registration order), so
  the encounters exist. Agents are visited in the scheduler's activation order
  (`ctx.agent_order`); unseated agents and agents of LLM kinds are skipped.
  With `assemble_with_llm`, `game_llm_decide` runs next in the same phase and
  fills the LLM agents' seats.
- **Reward** — after `game_play` (Interaction) has filled payoffs and appended
  the step history, so `learn_step` sees the complete step. Agents are visited
  in sorted `AgentId` order; `Held` agents and agents of LLM kinds are skipped
  (`game_llm_decide`'s Reward runs after this one).

## 5. State read/write contract

| Field | Read | Write | Notes |
|---|:--:|:--:|---|
| `PlayArena.seats` | ✓ | | Decision: which agents act, and in which role. |
| `PlayArena.actions` | | ✓ | Decision: the checked action for each seat. |
| `GameWorld.held` | ✓ | ✓ | Decision: a `Held` agent plays it; a rule-driven agent's action replaces it. |
| `PolicyTable` (`kinds`, `slot`, `kind_of`, `states`) | ✓ | ✓ | `decide` and `learn_step` receive `&mut` to the agent's state. |
| step history, `game`, `x`, `ids`, `clock.t` | ✓ | | Through `DecisionContext` / `LearnContext`. |
| `PlayArena.payoffs` / `credit` | ✓ | | Reward: through `LearnContext::outcomes()`. |
| Recorder event `game.invalid_action` | | ✓ | Only with `OnInvalidAction::Fallback`. |

## 6. Dependencies & ordering constraints

- **Upstream:** `game_match` (Decision) must have written the encounters;
  `game_play` (Interaction) must have written payoffs and the step history
  before Reward.
- **Downstream:** `game_play` requires every seat to hold an action; seats of
  LLM agents are filled by `game_llm_decide`, registered after `game_decide`.
- **Information structure:** the default view shows only opponents in the
  agent's own encounter (`opponent_history` returns `None` for anyone else);
  the whole world is readable only through `omniscient()`. During Decision,
  `held` read through `omniscient()` may depend on activation order.

## 7. Parameters

| Parameter | Default | Kind | Notes |
|---|---|---|---|
| `rules: RuleBook<G, X>` | — | structural | Passed to `assemble`; kinds registered by name. |
| `opts.on_invalid_action` | `OnInvalidAction::Stop` | policy | `Stop`: an invalid action fails the step. `Fallback(map)`: replace by the kind's fallback action and record `game.invalid_action`. |

The `Fallback` map covers this mechanism's kinds only; LLM kinds are not
listed there (their fallback is the `RetryProtocol` of the LLM rule).
With `Fallback`, `assemble` requires a fallback for every rule kind in use and
checks each fallback with `check_action` for every role. The event payload is
`{ "agent", "kind", "role", "action" }` (the rejected action, `Debug`-formatted).

## 8. How to apply

Library-only; `assemble` constructs `game_decide` from `rules` and `opts`.

### Library mode

```rust
use std::collections::BTreeMap;
use std::sync::Arc;
use socsim_games::{
    assemble, FixedPlayers, NormalFormOutcome, OnInvalidAction, PlayOptions, RuleBook,
    TitForTat, UniformRandom,
};

let mut rules = RuleBook::new();
rules.register("tit_for_tat", Arc::new(TitForTat::new(0)))?;
rules.register("uniform_random", Arc::new(UniformRandom))?;
// build_world(game, n, x, initial_held, &rules, &assignment, history_len, t_max, seed)?

let opts = PlayOptions::new().on_invalid_action(OnInvalidAction::Fallback(BTreeMap::from([
    ("tit_for_tat".to_owned(), 0),
    ("uniform_random".to_owned(), 0),
])));
let sim = assemble(world, FixedPlayers::pairs(&[(0, 1)]), rules, NormalFormOutcome, opts, seed)?
    .build();
```

## 9. Determinism & RNG

`decide` receives the engine step RNG (`ctx.rng`, from `derive_seed(seed, &[1])`);
`UniformRandom` draws from it, `Fixed` and `TitForTat` do not. Rule-state
initialisation (in `build_world`) uses `derive_seed(seed, &[3, agent, generation])`,
and `derive_seed(seed, &[2])` is stored as the world's `decision_root`; both
labels are reserved. Learning visits agents in sorted `AgentId` order, and
decisions follow the scheduler's order, so a run is reproducible for a given
seed — and exactly resumable from a snapshot, because all rule state is in the
world.

## 10. Expected behaviour

In the iterated prisoner's dilemma `TitForTat` versus `Fixed(D)`, `TitForTat`
cooperates in the first step and defects from the second step on; over 100
rounds the scores are 99 and 104 (`examples/ipd_pair.rs`). A rule that returns
an out-of-range action stops the step under `Stop`, and is replaced (with one
`game.invalid_action` event per occurrence) under `Fallback`.

## 11. References

- Axelrod, R. (1984). *The Evolution of Cooperation*. Basic Books.
