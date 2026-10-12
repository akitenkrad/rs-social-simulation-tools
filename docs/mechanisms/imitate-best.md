**English** | [日本語](imitate-best.ja.md)

# Imitate the best (`imitate_best`)

> Every held agent adopts the action of the highest-paid agent among itself and its neighbours, all agents at once.
> **Phase:** Reward. **Source:** Nowak & May (1992). **Kind:** deterministic imitation.

[← Back to the mechanism catalog](../mechanisms.md)

## 1. Overview

`imitate_best` is one of the four population dynamics of
[`socsim-games`](../games.md#population-dynamics). It is registered by
`assemble_with` when its `dynamics` argument is `ImitateBest::new(update)`, and
it works on a world whose agents are all **held** (no decision rule) and which
has a grid or network [topology](../games.md#topology-gameworldwith_topology).
After the step has been played, each agent

1. looks at its own payoff and at the payoffs of its topology neighbours;
2. adopts the held action of the agent with the highest payoff among them,
   breaking ties by action index (see the parameters);
3. — for every agent at once — writes the new action to `held`, which the
   agent plays in the next step.

It is the update rule of the spatial prisoner's dilemma: with
`GridNeighborhood::new().include_self(true)` on a Moore lattice and the weak
prisoner's dilemma, it reproduces Nowak & May's "evolutionary kaleidoscope".

## 2. Theory & source

Nowak & May (1992) place cooperators and defectors on a square lattice. Each
cell plays the prisoner's dilemma with $R = 1$, $T = b$, $S = P = 0$ against
its eight neighbours and itself, and in the next generation every cell is
occupied by the strategy of the highest-scoring cell in its neighbourhood, all
cells updating simultaneously and deterministically:

$$a_i^{t+1} = a_{j^*}^{t}, \qquad j^* \in \arg\max_{j \in \{i\} \cup N(i)} \pi_j^{t}$$

- $N(i)$ — the neighbours of agent $i$ in the topology (sorted, without $i$).
- $\pi_j^t$ — agent $j$'s payoff of step $t$ (`world.payoffs()`, after
  `PayoffAggregation`).
- $a_j^t$ — agent $j$'s held action during step $t$.

When several agents share the highest payoff, the choice is made by **action
index**, never by position, so the update commutes with every symmetry of the
lattice: a symmetric configuration stays symmetric. With self-play on a Moore
lattice a cooperator earns an integer $m \in \{1, \dots, 9\}$ and a defector
$k\,b$ with $k \in \{0, \dots, 8\}$, so the synchronous evolution changes only
when $b$ crosses a ratio $m/k$. No such ratio lies in $(1.8, 2)$: every $b$ in
that interval gives the same evolution, and from a random start the
cooperator share fluctuates around $0.318$ (Nowak & May, 1992).

## 3. Data flow

![imitate_best data flow](../assets/mech-imitate-best.svg)

Reads the step's payoffs, the held actions and the topology's neighbour lists.
Writes the held actions, through a buffer that is swapped in once every agent
has been evaluated.

## 4. Position in the 6-phase loop

Registers **Reward**. `assemble_with` adds it after `game_decide`, so it runs
after any learning and before mechanisms you add to the builder. The payoffs
it reads were written in Interaction by `game_play`; at PostStep
`world.held()` therefore already holds the next step's actions while
`world.payoffs()` and `world.plays()` still describe the actions just played
(see [What `PostStep` sees](../games.md#what-poststep-sees)).

## 5. State read/write contract

| Field | Read | Write | Notes |
|---|:--:|:--:|---|
| `GameWorld.payoff` | ✓ | | One payoff per agent; a non-finite payoff stops the step. |
| `GameWorld.held` | ✓ | ✓ | All next actions are computed from the pre-update `held`, then written at once (`Update::Synchronous`). |
| `GameWorld.topology` | ✓ | | Grid or network neighbours, sorted and deduplicated; on a directed network, the out-neighbours. |
| `PolicyTable.slot` | ✓ | | Checked once at assembly: every agent must be held. |

It writes nothing else: no history, no play arena, no payoff.

## 6. Dependencies & ordering constraints

- **Upstream:** `game_play` (Interaction) must have written this step's
  payoffs. Any matching works; the classic configuration is
  `GridNeighborhood::new().include_self(true)`.
- **Topology:** a grid or network topology is required
  (`GameWorld::with_topology`); a world without topology is rejected by
  `assemble_with` (`GameError::Population`). The neighbours used for imitation
  come from the topology, not from the matching.
- **Held agents only:** a world in which some agent has a decision rule is
  rejected (`GameError::Population`).
- **Common action space:** the game must implement `CommonActionSpace`
  (compile time) and every role must have the same number of actions
  (`GameError::Population`).
- **Continuous games:** a `ContinuousGame` works as well; the agent copies the
  whole action vector, and every role must have the same intervals
  (`GameError::Population`). See
  [Population dynamics with continuous actions](../games.md#population-dynamics-with-continuous-actions).
- **Downstream:** the next step's `game_decide` seats the held agents with
  their new actions; [`game_metrics`](game-metrics.md) with
  `MetricsOptions::population_share(true)` records the updated shares.

## 7. Parameters

| Parameter | Default | Kind | Notes |
|---|---|---|---|
| `update: Update` | — (required) | structural | `Update::Synchronous`, the only schedule; it has no default so that adding schedules cannot change an existing configuration. |
| `tie_break(ImitationTie)` | `KeepOwn` | structural | `KeepOwn`: keep the own action when the own payoff is among the highest, else adopt the lowest action index among the best neighbours. `LowestAction`: adopt the lowest action index among the agent and its neighbours that share the highest payoff. In a continuous game "the lowest action" is the lexicographically smallest action vector (components compared with `f64::total_cmp`, a shorter vector first). |

## 8. How to apply

Library-only. Pass the configuration as the `dynamics` argument of
`assemble_with`, on a world with a topology:

### Library mode

```rust
use socsim_games::{
    assemble_with, GridBoundary, GridNeighborhood, GridNeighbors, GridSpec, ImitateBest,
    ImitationTie, NormalFormOutcome, Pairwise, PlayOptions, Topology, Update,
};

let world = build_world(game, side * side, (), initial, &rules, &held, 1, steps, seed)?
    .with_topology(Topology::grid(GridSpec {
        rows: side,
        cols: side,
        boundary: GridBoundary::Toroidal,
        neighborhood: GridNeighbors::Moore,
    })?)?;
let mut sim = assemble_with(
    world,
    GridNeighborhood::new().include_self(true),
    rules,                                            // an empty RuleBook: every agent is held
    Pairwise::new(NormalFormOutcome),
    ImitateBest::new(Update::Synchronous).tie_break(ImitationTie::KeepOwn),
    PlayOptions::new(),
    seed,
)?
.build();
```

The complete program is `crates/socsim-games/examples/nowak_may.rs`
(`--features core,engine,grid`); see
[`docs/games.md` § Examples](../games.md#examples-nowak_may-and-hawk_dove_large).

## 9. Determinism & RNG

Draws **no** randomness. Agents are evaluated in ascending `AgentId` order from
the same pre-update state, and ties are broken by action index, so the result
depends only on the payoffs, the held actions and the topology. The mechanism
keeps no state between steps (its buffer is scratch space), so a snapshot
resumes it exactly.

## 10. Expected behaviour

On a 99 × 99 lattice with fixed boundaries, Moore neighbours and self-play,
`PrisonersDilemma::weak(1.85)` and a single defector in the centre:

- after the first step the centre's payoff is $8b = 14.8$ (eight cooperating
  neighbours, computed from the pre-update actions) and the defectors form a
  3 × 3 block;
- the number of defectors runs 9, 25, 41, 81, 81, 109, … and is 5 165 after 60
  generations;
- `held` and `payoffs()` stay symmetric under horizontal and vertical
  reflection and transposition in every generation.

On a 100 × 100 torus from 10 % random defectors, the cooperator share falls to
about 0.19 within four generations and then fluctuates around 0.32 (example
`nowak_may`).

## 11. References

- Nowak, M. A., & May, R. M. (1992). Evolutionary games and spatial chaos. *Nature*, 359(6398), 826–829.
