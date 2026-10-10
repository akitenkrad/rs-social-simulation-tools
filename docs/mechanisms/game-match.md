**English** | [日本語](game-match.ja.md)

# Game matching (`game_match`)

> Resets the per-step play state and seats agents into this step's encounters.
> **Phase:** PreStep + Decision. **Source:** game-theoretic simulation (matching protocol). **Kind:** structural.

[← Back to the mechanism catalog](../mechanisms.md)

## 1. Overview

`game_match` is the first of the three mechanisms that
[`socsim-games`](../games.md) registers for a normal-form game simulation. It
owns the *who plays whom* part of a step:

- in **PreStep** it clears the previous step's encounters (`PlayArena`) and the
  per-agent `payoff` and `n_played` counters;
- at the start of **Decision** it hands the world to the configured `Matching`,
  which writes this step's encounters — one seat per role — into the play arena.

It does not choose actions or compute payoffs; `game_decide` and `game_play`
do that from the encounters it writes.

## 2. Theory & source

In a repeated or evolutionary game simulation, the *matching protocol* decides
which agents meet in each round: fixed partners (a repeated two-player game),
random pairing, round-robin tournaments, or neighbours on a network. Keeping the
protocol separate from the stage game and from the decision rules lets the same
game and rules run under different interaction structures.

The `Matching<G, X>` trait expresses one protocol:

$$\text{encounters}_t = \mathcal{M}\big(t,\ \text{ids},\ \text{game},\ \text{held},\ \text{history},\ X;\ \text{rng}\big)$$

where each encounter is a tuple of agents (one per role) with a credit mode.
The built-in `FixedPlayers` returns the same pairs every step, which is the
standard repeated two-player game.

## 3. Data flow

![game_match data flow](../assets/mech-game-match.svg)

PreStep empties the per-step fields. Decision reads the matching context
(`t`, agent ids, game, topology, held actions, step history, `X`) and the
`MatchingState`, and writes the encounters (`seats`, `credit`, empty action and
payoff slots) into the `PlayArena`.

## 4. Position in the 6-phase loop

Registers **PreStep** and **Decision**.

- **PreStep** runs before anything else in the step, so every later phase sees
  an empty arena and zeroed counters.
- **Decision**: `assemble` registers `game_match` before `game_decide`, and
  mechanisms in the same phase run in registration order — so encounters exist
  before any agent is asked for an action.

## 5. State read/write contract

| Field | Read | Write | Notes |
|---|:--:|:--:|---|
| `GameWorld.plays` (`PlayArena`) | | ✓ | PreStep: cleared. Decision: encounters appended through `ArenaWriter::push`. |
| `GameWorld.payoff` | | ✓ | PreStep: filled with `0.0`. |
| `GameWorld.n_played` | | ✓ | PreStep: filled with `0`. |
| `GameWorld.matching_state` | ✓ | ✓ | Passed mutably to `encounters_into` (currently `MatchingState::None`). |
| `clock.t`, `ids`, `game`, `topology`, `held`, step history, `x` | ✓ | | Read-only through `MatchContext`. |

## 6. Dependencies & ordering constraints

- **Upstream:** none within the step. Held actions and history come from earlier
  steps (`game_decide`, `game_play`).
- **Downstream:** `game_decide` (Decision) reads the seats to decide which
  agents act and in which role; `game_play` (Interaction) evaluates every
  encounter. Agents that are not seated do not act or play this step.
- Keep `game_match` registered first; `assemble` does this for you.

## 7. Parameters

| Parameter | Default | Kind | Notes |
|---|---|---|---|
| `matching: M` (`M: Matching<G, X>`) | — | structural | Passed to `assemble`. Built-in: `FixedPlayers::pairs(&[(a, b), …])` / `FixedPlayers::new(vec![[AgentId; 2], …])`. |

`FixedPlayers::check` (run once in `assemble`) rejects an unknown agent, an agent
listed in more than one pair, and a game without exactly two roles
(`GameError::InvalidMatching`).

A custom `Matching` keeps any state it needs in the experiment state `X`, not in
its own struct — the struct is code and is not part of a snapshot.

## 8. How to apply

`game_match` is library-only (no scenario-TOML registration) and is not
constructed directly: `assemble` creates it from the `matching` argument.

### Library mode

```rust
use socsim_games::{assemble, FixedPlayers, NormalFormOutcome, PlayOptions};

let mut sim = assemble(
    world,                              // from build_world(...)
    FixedPlayers::pairs(&[(0, 1), (2, 3)]),
    rules,
    NormalFormOutcome,
    PlayOptions::new(),
    seed,
)?
.build();
sim.step()?;
let arena = sim.world().plays();        // this step's encounters
```

## 9. Determinism & RNG

PreStep draws no randomness. In Decision, `encounters_into` receives the engine
step RNG (`ctx.rng`, seeded from `derive_seed(seed, &[1])`); `FixedPlayers` does
not draw from it. A randomised custom matching must draw only from that RNG so
that a run — and a resumed run — stays reproducible.

## 10. Expected behaviour

With `FixedPlayers::pairs(&[(0, 1)])`, `world().plays().n_encounters()` is `1`
after every step and both agents have `n_played == 1`. With pairs that do not
cover every agent, the uncovered agents have `n_played == 0` and a `None`
action in that step's `StepRecord`.

## 11. References

- Axelrod, R. (1984). *The Evolution of Cooperation*. Basic Books.
