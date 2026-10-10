**English** | [日本語](game-llm-decide.ja.md)

# Game LLM decision (`game_llm_decide`)

> Every seated agent whose kind is an LLM rule renders a request; the step's requests are sent as one batch (store first, backend for misses); the replies are parsed into checked actions, with retries and fallbacks.
> **Phase:** Decision + Reward. **Source:** LLM agents in repeated games. **Kind:** LLM-driven.

[← Back to the mechanism catalog](../mechanisms.md)

## 1. Overview

`game_llm_decide` (the `LlmDriver` type, feature `llm` + `engine`) is the
[`socsim-games`](../games.md) mechanism that lets a language model choose an
agent's action. `assemble_with_llm` registers it after `game_decide` and before
`game_play`. It drives the `LlmDecisionRule`s of an `LlmRuleBook` in two stages:

- in **Decision** it calls `prepare` for every seated agent whose kind it owns,
  sends the step's requests as one batch — deduplicated, looked up in the
  `ResponseStore`, and only the misses executed by the `LlmExecutor` — then calls
  `resolve` on each reply. Violations are retried in a further batch or replaced
  by a fallback; every final action is checked with `StageGame::check_action`
  and written to the agent's seat and held action;
- in **Reward** it calls `learn_step` for every agent of its kinds (the built-in
  `LlmPolicy` writes the step's payoff into its conversation record). The model
  is never called in Reward.

Which kinds the mechanism owns is fixed by the `KindCatalog`: kinds of the
`LlmRuleBook` belong to `game_llm_decide`, kinds of the `RuleBook` to
[`game_decide`](game-decide.md). Each driver skips the other's agents, so LLM
agents and rule-driven agents share one world.

## 2. Theory & source

An LLM agent's behavioural strategy is the composition of an observation, a
prompt renderer, the model, and a parser:

$$a_i^t = \text{parse}\Big(\text{LLM}\big(\text{render}(o_i^t);\ \theta,\ \sigma_i^t\big)\Big), \qquad o_i^t = \text{observe}(\text{view}_i^t)$$

- $o_i^t$ — the observation built only from the agent's `DecisionContext`
  (`OwnHistoryObs`: own recent records and current opponents' last actions;
  `CountsObs`: counts of opponents' last actions).
- $\theta$ — generation conditions (`LlmConfig`: temperature, `max_tokens`, …).
- $\sigma_i^t$ — the generation seed, derived from the decision unit (§9).
- $\text{parse}$ — `LabelParser` (exact label in a JSON reply, or lenient
  first-mention matching); a violation triggers a retry with feedback, then a
  fallback (previous action, uniform draw, or a specified action).

The model is treated as a stochastic oracle whose answers are **recorded and
replayed**: a request is identified by its complete key (backend, system,
prompt, temperature, seed, …), so a run against a fixed record is reproducible.
Using LLMs as simulated agents in economic and repeated games follows Horton
(2023) and Akata et al. (2023).

## 3. Data flow

![game_llm_decide data flow](../assets/mech-game-llm-decide.svg)

Decision reads the seats written by `game_match`, each agent's kind and
`LlmConversation`, held actions, history and `X` (through the observation), and
the `ResponseStore`; it writes the checked actions into the seats and held
actions, appends a turn to each agent's conversation, appends new records to the
store's shard (`RecordMisses`), and records one `game.llm_call` event per attempt
and one `game` row of `llm.*` rates. Reward reads the step's records written by
`game_play` and updates the conversations.

## 4. Position in the 6-phase loop

Registers **Decision** and **Reward**.

- **Decision** — after `game_match` (encounters exist) and `game_decide`
  (rule-driven seats are filled) in registration order, and before `game_play`
  (Interaction), which requires every seat to hold an action. Requests are
  prepared in the scheduler's activation order (`ctx.agent_order`); unseated and
  `Held` agents are skipped. Replies are resolved in the same order, after the
  whole batch has returned; actions are written once every decision is final.
- **Reward** — after `game_play` has filled payoffs and appended the step
  history, and after `game_decide`'s `learn_step`. Agents are visited in sorted
  `AgentId` order; agents of other kinds and `Held` agents are skipped.

## 5. State read/write contract

| Field | Read | Write | Notes |
|---|:--:|:--:|---|
| `PlayArena.seats` | ✓ | | Decision: which agents act, and in which role. |
| `PlayArena.actions` | | ✓ | Decision: the checked action for each seat of an LLM agent. |
| `GameWorld.held` | ✓ | ✓ | Decision: `ResolveContext::previous` (for `Fallback::Previous`); replaced by the final action. |
| `PolicyTable` (`kinds`, `slot`, `kind_of`, `states`) | ✓ | ✓ | `prepare` reads the state; `resolve` and `learn_step` receive `&mut` (the `LlmConversation`). |
| step history, `game`, `x`, `ids`, `clock.t` | ✓ | | Through `DecisionContext` / `LearnContext`. |
| `GameWorld.decision_root` | ✓ | | Root of the generation seeds and fallback streams. |
| `X` manifest (`HasLlmManifest`) | ✓ | | Compared with the store and backend identity on every Decision. |
| `ResponseStore` (outside the world) | ✓ | ✓ | Looked up on the main thread; misses appended to this run's shard under `RecordMisses`. |
| Recorder row `game` (`llm.*`) | | ✓ | Once per Decision with at least one LLM decision. |
| Recorder event `game.llm_call` | | ✓ | One per attempt, including retries and deduplicated attempts. |

## 6. Dependencies & ordering constraints

- **Upstream:** `game_match` (Decision) must have written the encounters;
  `game_play` (Interaction) must have written payoffs and the step history
  before Reward.
- **Downstream:** `game_play` requires every seat to hold an action — the LLM
  agents' seats are filled only by this mechanism.
- **Kinds:** the world must be built with a `KindCatalog` containing both books
  (`build_world_with`) and assembled with `assemble_with_llm`, which checks the
  world's kind list against that catalog (`GameError::InvalidKinds`). A kind in
  both books is `RuleError::DuplicateKind`.
- **Experiment state:** `X: HasLlmManifest` (use `WithLlm<X>`), and
  `X::PolicyState: Slot<LlmConversation>` for `LlmPolicy`.
- **Information structure:** observations are built from the `DecisionContext`
  only (no `omniscient()`), so the default view — opponents in the agent's own
  encounter plus `X::view_for(me)` — applies.
- **Two roles only:** `assemble_with_llm` rejects other games with
  `GameError::UnsupportedRoles`.

## 7. Parameters

| Parameter | Default | Kind | Notes |
|---|---|---|---|
| `LlmSetup::new(book, store, backend, identity)` | — | structural | `LlmRuleBook`, `ResponseStore`, `BackendFactory` (cache-free; built inside each thread), `BackendIdentity { model, endpoint }`. |
| `.config(LlmConfig)` | `LlmConfig::deterministic()` | generation | Base of every request; `seed` is always overwritten, `system` whenever the request has one. |
| `.executor(LlmExecutor)` | `Sequential` | execution | `Threaded { workers }` runs calls on worker threads through a bounded queue. |
| `.options(ExecutorOptions)` | see below | execution | `#[non_exhaustive]`: mutate `ExecutorOptions::default()`. |
| `timeout` | 120 s | execution | Per call, `Threaded` only. A timed-out call is not resent; the step fails. |
| `max_backend_retries` | 3 | execution | Resends after a backend error; then the step fails with `RuleError::Backend`. |
| `backoff_base` | 1 s | execution | The k-th resend waits `backoff_base × 2^(k-1)`. |
| `queue_per_worker` | 2 | execution | Queue capacity = `workers × queue_per_worker`. |
| store `ReplayMode` | — | replay | `ReplayOnly` (miss = error), `RecordMisses` (miss → backend → shard), `Live` (store unused). |
| `RetryProtocol { max_retries, fallback }` (per `LlmPolicy`) | `1`, `Fallback::Previous` | policy | Retries with feedback while `attempt < max_retries`; then `Previous`, `UniformRandom` or `Specified(a)`. |
| `LabelParser` (per `LlmPolicy`) | — | policy | `strict()` (exact label in JSON; any labels) or `lenient()` (first-mention fallback; ASCII `[A-Za-z0-9_-]+` labels only, checked at init). `require_reason`, `with_synonyms`. |
| `with_max_turns(n)` (per `LlmPolicy`) | 16 | policy | Length of the `LlmConversation` record. |

`OnInvalidAction` does not apply to this mechanism: its `Fallback` map may list
`RuleBook` kinds only. An invalid final action from an LLM kind stops the step
with `RuleError::InvalidAction`.

## 8. How to apply

Library-only; `assemble_with_llm` constructs `game_llm_decide` from the
`LlmSetup`. A complete, runnable program is in
[normal-form games › LLM decisions](../games.md#llm-decisions-feature-llm).

### Library mode

```rust
use std::sync::Arc;
use socsim_games::llm::{
    assemble_with_llm, BackendFactory, BackendIdentity, CountsObs, CountsRenderer, LabelParser,
    LlmExecutor, LlmPolicy, LlmReplayManifest, LlmRuleBook, LlmSetup, ReplayMode, ResponseStore,
    WithLlm,
};
use socsim_games::{build_world_with, FixedPlayers, KindCatalog, NormalFormOutcome, PlayOptions};

let mut llm_rules = LlmRuleBook::new();
llm_rules.register(
    "llm",
    Arc::new(LlmPolicy::new(CountsObs, CountsRenderer::default(), LabelParser::strict())),
)?;
let catalog = KindCatalog::new().with_rules(&rules)?.with_llm(&llm_rules)?;

let store = ResponseStore::open(dir, ReplayMode::RecordMisses, "run", seed)?;
let x = WithLlm { inner: (), llm: LlmReplayManifest::for_store(&store, identity.clone()) };
// build_world_with(game, n, x, initial_held, &catalog, &assignment, history_len, t_max, seed)?

let setup = LlmSetup::new(llm_rules, store, backend, identity)
    .executor(LlmExecutor::Threaded { workers: 4 });
let sim = assemble_with_llm(
    world, FixedPlayers::pairs(&[(0, 1)]), rules, setup, NormalFormOutcome, PlayOptions::new(), seed,
)?
.build();
```

In tests, `backend` is a `BackendFactory` closure that returns
`socsim_llm::mock::ScriptedClient`; with feature `live`,
`socsim_games::llm::live_backend_factory()` returns the cache-free
Ollama → OpenAI backend.

## 9. Determinism & RNG

`game_llm_decide` **does not draw from the engine's step RNG** (`ctx.rng`). Every
stream it needs is derived from the world's `decision_root`
(`derive_seed(seed, &[2])`) and the decision unit:

| Use | Seed |
|---|---|
| generation seed, `temperature == 0` | `derive_seed(decision_root, &[0])` (the same for every request, so identical prompts share one key and one call) |
| generation seed, otherwise | `derive_seed(decision_root, &[t, agent, 0, 0, attempt])` |
| `Fallback::UniformRandom` | `derive_seed(decision_root, &[t, agent, 0, 0, u64::MAX])` |

Only the main thread looks up and writes the store, and replies are resolved in
decision order, so the result does not depend on activation timing or on the
executor: `Sequential` and `Threaded { workers }` produce identical worlds.

Reproducibility rests on the responses. A run is reproducible from a fixed
record (`ReplayOnly`). A snapshot resumes exactly **only** under `ReplayOnly`, or
under `RecordMisses` when every request after the resume hits the store
(`cache_hit`; `llm.cache_hit_rate == 1`); under `Live` nothing is guaranteed.
The world carries an `LlmReplayManifest { store_id, base_digest, mode, backend }`
in `WithLlm<X>`; the driver compares it with its store and backend identity on
every Decision and stops on a mismatch, and `verify_snapshot` performs the
same check before `restore`. `ResponseStore::merge_shards` is never called
automatically, and it changes `base_digest`.

## 10. Expected behaviour

- With a scripted model that always answers `{"action": "D", …}` against
  `TitForTat` in the prisoner's dilemma, the held actions are `[1, 0]` after the
  first step and `[1, 1]` from the second step on.
- At temperature 0, two LLM agents in the same situation (`CountsObs`) produce
  one backend call per step; both attempts are recorded as `game.llm_call`
  events with the same `key`, the second with `deduplicated: true`.
- A reply that violates the format is retried with feedback; when retries run
  out, the fallback is used and `llm.giveup_rate` counts the agent.
- A miss under `ReplayOnly`, an exhausted backend retry budget, or a timeout
  under `Threaded` stops the step with a mechanism error.

## 11. References

- Horton, J. J. (2023). *Large Language Models as Simulated Economic Agents: What Can We Learn from Homo Silicus?* NBER Working Paper 31122.
- Akata, E., Schulz, L., Coda-Forno, J., Oh, S. J., Bethge, M., & Schulz, E. (2023). *Playing repeated games with large language models*. Nature Human Behaviour (2025), doi:10.1038/s41562-025-02172-y. Preprint arXiv:2305.16867 (2023).
