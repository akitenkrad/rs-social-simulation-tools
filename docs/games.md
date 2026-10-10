**English** | [日本語](games.ja.md)

# Normal-form games (`socsim-games`)

`socsim-games` runs game-theoretic simulations on the socsim engine: agents are
seated into encounters of a **stage game**, each one chooses an action through a
**decision rule**, an **outcome rule** turns the action profile into payoffs, and
the rules learn from what happened — once per step, inside the ordinary
[6-phase tick loop](architecture.md#the-6-phase-tick-loop).

The crate is library-only (no `ModulePack`, not wired into the `socsim` binary).
This page covers the building blocks, where their state lives, what a rule is
allowed to see, and how to assemble a runnable simulation.

---

## Cargo features

| Feature | Adds | socsim dependencies |
|---|---|---|
| *(default)* | the stage-game traits (`StageGame`, `NormalFormGame`, `FiniteGame`) and `MatrixGame` | **none** — the default build depends on no `socsim-*` crate |
| `core` | the world (`GameWorld`), rules, outcome rules, matching, views, `build_world` | `socsim-core` |
| `engine` | `assemble` (returns a `SimulationBuilder`); implies `core` | `socsim-core`, `socsim-engine` |

The crate depends on neither `socsim-llm`, `socsim-config` nor `socsim-runner`.
A simulation needs both features:

```toml
socsim-games = { git = "https://github.com/akitenkrad/rs-social-simulation-tools", branch = "main", package = "socsim-games", features = ["engine"] }
socsim-core   = { git = "https://github.com/akitenkrad/rs-social-simulation-tools", branch = "main", package = "socsim-core" }
socsim-engine = { git = "https://github.com/akitenkrad/rs-social-simulation-tools", branch = "main", package = "socsim-engine" }
```

---

## Minimal example: iterated prisoner's dilemma

`Tit-for-Tat` against an unconditional defector for 100 rounds
(`crates/socsim-games/examples/ipd_pair.rs`):

```rust
use std::sync::Arc;

use socsim_games::{
    assemble, build_world, AssignGroup, AssignKind, Assignment, Fixed, FixedPlayers,
    InitialActions, MatrixGame, NormalFormOutcome, PlayOptions, RuleBook, TitForTat,
};

const ROUNDS: usize = 100;

fn main() {
    // Row player's and column player's payoffs; action 0 = C, 1 = D.
    let game = MatrixGame::bimatrix(
        &[vec![3.0, 0.0], vec![5.0, 1.0]],
        &[vec![3.0, 5.0], vec![0.0, 1.0]],
    )
    .expect("the prisoner's dilemma matrix is valid");

    let mut rules = RuleBook::new();
    rules.register("tit_for_tat", Arc::new(TitForTat::new(0))).unwrap();
    rules.register("fixed_defect", Arc::new(Fixed::new(1))).unwrap();

    let seed = 1;
    let world = build_world(
        game,
        2,                          // n_agents
        (),                         // experiment state X
        InitialActions::Same(0),    // initial held action
        &rules,
        &Assignment::Ordered(vec![
            AssignGroup { kind: AssignKind::Rule("tit_for_tat".to_owned()), count: 1 },
            AssignGroup { kind: AssignKind::Rule("fixed_defect".to_owned()), count: 1 },
        ]),
        ROUNDS,                     // history_len
        ROUNDS as u64,              // t_max
        seed,
    )
    .expect("the world configuration is valid");

    let mut simulation = assemble(
        world,
        FixedPlayers::pairs(&[(0, 1)]),
        rules,
        NormalFormOutcome,
        PlayOptions::new(),
        seed,
    )
    .expect("the simulation configuration is valid")
    .build();

    let mut scores = [0.0, 0.0];
    for _ in 0..ROUNDS {
        simulation.step().unwrap();
        for (score, payoff) in scores.iter_mut().zip(simulation.world().payoffs()) {
            *score += payoff;
        }
    }
    println!("TFT score: {}, Fixed(D) score: {}", scores[0], scores[1]);
}
```

```sh
cargo run -p socsim-games --example ipd_pair --features core,engine
# TFT score: 99, Fixed(D) score: 104
```

`GameWorld::payoffs()` holds **this step's** payoffs (it is reset every step), so
cumulative scores are summed by the caller.

---

## Stage games

A stage game defines the roles of one encounter and the actions available to
each role. Three traits form a ladder:

| Trait | Adds | Methods |
|---|---|---|
| `StageGame` | roles and action validity | `type Action`, `n_roles()`, `check_action(role, &action) -> Result<(), ActionError>` |
| `NormalFormGame: StageGame` | a payoff for every role and profile | `payoff(role, profile)`, `payoffs_into(profile, out)` (default loops over `payoff`) |
| `FiniteGame: NormalFormGame<Action = usize>` | finitely many indexed actions | `n_actions(role)`, `action_label(role, action)` (default `"a{action}"`), `is_symmetric()` (default `false`) |

Rules and outcome rules state the rung they need as a trait bound —
`UniformRandom` and `TitForTat` require `FiniteGame`, `NormalFormOutcome`
requires `NormalFormGame` — so a mismatch is a compile error, not a runtime one.

### `MatrixGame`

`MatrixGame` is the built-in `FiniteGame`: one row-major payoff tensor per role,
actions are zero-based indices.

| Constructor | Use |
|---|---|
| `MatrixGame::new(n_actions, payoffs)` | general n-role tensor; `payoffs[role]` has one entry per profile |
| `MatrixGame::bimatrix(a, b)` | two roles; `a[i][j]` / `b[i][j]` are the row / column player's payoffs |
| `MatrixGame::zero_sum(a)` | two roles; the column player receives `-a[i][j]` |
| `.with_labels(labels)` | per-role action labels for `action_label` |

Construction fails with `MatrixError` on a shape mismatch (`Shape`), a
non-finite payoff (`NonFinite`), a label table of the wrong shape (`Labels`), or
an oversized game (`TooLarge`: more than 10⁶ profiles or 10⁷ payoff elements).
`is_symmetric()` is computed once: two roles, equal action counts, and
`payoff(0, (i, j)) == payoff(1, (j, i))`. `MatrixGame` serialises with serde, so
it is part of a snapshot.

The simulation layer currently assembles **two-role** encounters only:
`assemble` rejects any other `n_roles()` with `GameError::UnsupportedRoles`.

---

## Decision rules

A `DecisionRule<G, X>` chooses one agent's action and optionally learns from the
step's outcome:

```rust
pub trait DecisionRule<G: StageGame, X: ExperimentState>: Send + Sync {
    fn name(&self) -> &str;
    fn init_state(&self, ctx: &RuleInitContext, game: &G) -> Result<X::PolicyState, RuleError>;
    fn decide(
        &self,
        state: &mut X::PolicyState,
        ctx: &DecisionContext<'_, G, X>,
        rng: &mut SimRng,
    ) -> Result<G::Action, RuleError>;
    fn learn_step(
        &self,
        state: &mut X::PolicyState,
        ctx: &LearnContext<'_, G, X>,
    ) -> Result<(), RuleError> { Ok(()) }
}
```

Note the receivers: `decide` and `learn_step` take `&self`. A rule object is
shared code; everything it remembers about an agent lives in the
`state: &mut X::PolicyState` it is handed (see
[where rule state lives](#where-rule-state-lives-and-why-it-matters-for-snapshots)).

Built-in rules:

| Rule | Bound | Behaviour | State slot |
|---|---|---|---|
| `Fixed::new(action)` | any `StageGame` | always plays `action` (validated for every role at init) | `Stateless` |
| `UniformRandom` | `FiniteGame` | uniform draw over the role's actions from `rng` | `Stateless` |
| `TitForTat::new(coop)` | `FiniteGame`, two roles, two actions per role | plays `coop` first, then the opponent's last action | `TftState { last_opponent }` |

Rules are registered by **kind name** in a `RuleBook`
(`register(kind, Arc<dyn DecisionRule<G, X>>)`; a duplicate kind is
`RuleError::DuplicateKind`). Kind ids are assigned in sorted kind-name order, so
they do not depend on registration order.

### Assigning rules to agents

`build_world` takes an `Assignment` of `AssignGroup { kind, count }`:

- `AssignKind::Rule(kind)` — the agents run the registered rule `kind`; each gets
  its own state from `init_state`.
- `AssignKind::Held` — the agents have **no** rule and no state slot; they keep
  playing their held action (from `InitialActions`).
- `Assignment::Ordered(groups)` assigns agent ids in group order;
  `Assignment::Shuffled(groups)` shuffles the assignment with the world-init
  stream.

The counts must sum to `n_agents` (`GameError::AgentCount`).

### `RuleInitContext`

`init_state` receives `RuleInitContext { agent, kind, generation, init_seed }`.
`init_seed` is `derive_seed(seed, &[3, agent, generation])` — a per-agent
stream for randomised initial state (`generation` is `0` for agents created by
`build_world`). Draw any initial randomness from `SimRng::from_seed(init_seed)`.

---

## Outcome rules

An `OutcomeRule<G, X>` writes one payoff per role for one encounter:

```rust
pub trait OutcomeRule<G: StageGame, X: ExperimentState>: Send + Sync {
    fn name(&self) -> &str;
    fn outcome(
        &self,
        ctx: &OutcomeContext<'_, G, X>,
        encounter: EncounterRef<'_>,
        actions: &[G::Action],
        payoffs: &mut [f64],
    ) -> socsim_core::Result<()>;
}
```

`NormalFormOutcome` (for any `NormalFormGame`) simply calls
`game.payoffs_into(actions, payoffs)`. Write your own outcome rule when payoffs
depend on more than the action profile — `OutcomeContext` exposes `t()`,
`game()`, `x()`, `held(agent)` and `topology()`, and `EncounterRef` gives the
encounter's `id`, `seats` (agent per role) and `credit`. For example, payoffs
scaled by a reputation kept in `X`:

```rust
impl OutcomeRule<MatrixGame, ReputationState> for ReputationDiscount {
    fn name(&self) -> &str { "reputation_discount" }
    fn outcome(
        &self,
        ctx: &OutcomeContext<'_, MatrixGame, ReputationState>,
        encounter: EncounterRef<'_>,
        actions: &[usize],
        payoffs: &mut [f64],
    ) -> socsim_core::Result<()> {
        ctx.game().payoffs_into(actions, payoffs);
        for (role, payoff) in payoffs.iter_mut().enumerate() {
            *payoff *= ctx.x().reputation[encounter.seats[role].0 as usize];
        }
        Ok(())
    }
}
```

Encounters must use `Credit::All` (every seat is credited with its payoff);
other credit modes are not yet provided and stop the step with an error.

---

## Experiment state `X`

`GameWorld<G, X>` is generic over an **experiment state** `X: ExperimentState` —
the place for everything model-specific: reputations, signals, budgets, a custom
matching's bookkeeping. `X = ()` is the default and uses the built-in rule
states.

```rust
pub trait ExperimentState: Send + Sync + 'static {
    type PolicyState: Clone + Send + Sync + Debug + 'static;
    type View<'a> where Self: 'a;
    fn view_for(&self, me: AgentId) -> Self::View<'_>;
}
```

- **`PolicyState`** — the per-agent rule state type. One value per rule-driven
  agent is stored in the world.
- **`View<'a>` / `view_for(me)`** — what an agent may see of `X` by default (see
  [information structure](#information-structure-default-views-view_for-and-omniscient)).

### `PolicyState` and `Slot`

Built-in rules do not name your `PolicyState`; they reach their own state
through the `Slot<T>` trait (`slot`, `slot_mut`, `from_slot`). `Fixed` and
`UniformRandom` need `Slot<Stateless>`, `TitForTat` needs `Slot<TftState>`.
`BuiltinPolicyState` (the `PolicyState` of `X = ()`) implements both. To mix
built-in and custom rules, wrap it:

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
enum MyPolicyState {
    Builtin(BuiltinPolicyState),
    Discriminator { threshold: f64, seen: u32 },
}

impl Slot<TftState> for MyPolicyState {
    fn slot(&self) -> Option<&TftState> {
        match self { Self::Builtin(s) => s.slot(), _ => None }
    }
    fn slot_mut(&mut self) -> Option<&mut TftState> {
        match self { Self::Builtin(s) => s.slot_mut(), _ => None }
    }
    fn from_slot(value: TftState) -> Self {
        Self::Builtin(<BuiltinPolicyState as Slot<TftState>>::from_slot(value))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ReputationState { reputation: Vec<f64> }

impl ExperimentState for ReputationState {
    type PolicyState = MyPolicyState;
    type View<'a> = &'a [f64];
    fn view_for(&self, _me: AgentId) -> Self::View<'_> { &self.reputation }
}
```

Using a built-in rule whose slot your `PolicyState` does not implement is a
compile error. If a slot is present in the type but the stored variant differs,
the rule returns `RuleError::MissingSlot`.

To change `X` during a run, add your own `Mechanism<GameWorld<G, X>>` to the
builder and write through `GameWorld::x_mut()`.

---

## Where rule state lives, and why it matters for snapshots

All state that a resumed run needs is stored **in the world**:

| State | Field (read accessor) |
|---|---|
| per-agent rule state | `PolicyTable` (`world.policies()`, `.state(agent)`, `.kind(agent)`) |
| held actions | `world.held()` |
| this step's encounters, actions, payoffs | `PlayArena` (`world.plays()`) |
| this step's payoff and play count per agent | `world.payoffs()`, `world.n_played()` |
| per-agent step history (ring of `history_len`) | `world.history(agent)` |
| the experiment state | `world.x()` |

The three mechanisms (`game_match`, `game_decide`, `game_play`), the
`RuleBook`, the `Matching` and the `OutcomeRule` hold **no** state that affects a
resumed run — they are code, supplied again when the simulation is rebuilt. That
is what makes the engine's [snapshot & resume](architecture.md#snapshots-save--resume)
exact for game simulations:

```rust
let snapshot = partial.snapshot();          // GameWorld + RNG position
let mut resumed = build_simulation(/* any seed */);
resumed.restore(snapshot);                  // continues bit-identically
```

`GameWorld` is `Serialize`/`Deserialize` when `G`, `G::Action`, `X` and
`X::PolicyState` are. Consequently, keep only restorable data in `X` and in
`PolicyState` — no network clients, file handles or caches — and never keep
mutable per-run data in a rule, matching or outcome struct: it would not be
saved and a resumed run would diverge.

---

## Information structure: default views, `view_for`, and `omniscient()`

A rule sees the world through a context, not through `GameWorld` directly. The
default methods give the information structure of a standard repeated game:

**`DecisionContext`** (passed to `decide`):

| Method | Returns |
|---|---|
| `t()`, `me()`, `unit()` | step, own id, decision unit (`DecisionUnit::Step`) |
| `role()`, `encounter()` | own role and encounter this step |
| `opponents()` | the other agents **in the same encounter** |
| `my_history()` | own past `StepRecord`s (action, payoff, `n_played`) |
| `opponent_history(id)` | the actions of `id` — **`None` unless `id` is a current opponent** |
| `game()` | the stage game |
| `view()` | `X::view_for(me)` |
| `omniscient()` | unrestricted data (see below) |

**`LearnContext`** (passed to `learn_step`): `t()`, `me()`, `record()` (own
record for this step), `my_history()`, `outcomes()` (own encounters this step,
in `EncounterId` order — each `EncounterOutcome` has `role`, `seats`, `actions`,
`payoffs`, `credit`), `game()`, `view()`, `omniscient()`.

`view_for(me)` is where you decide what `X` reveals: return only `me`'s private
signal for an incomplete-information game, or a public reputation vector for an
indirect-reciprocity model.

**`omniscient()`** returns an `OmniscientContext` with every agent's id, held
action and history, and the full `X`. Calling it **removes the information
structure guarantee** — use it only for rules that are meant to be omniscient.
During `Decision`, `held` may also reflect activation order, because agents that
acted earlier in the step have already updated their held action.

---

## Matching

A `Matching<G, X>` seats agents into encounters at the start of `Decision`:

```rust
pub trait Matching<G: StageGame, X: ExperimentState>: Send + Sync {
    fn name(&self) -> &str;
    fn check(&self, ctx: &MatchContext<'_, G, X>) -> Result<(), GameError>;
    fn init_state(&self, ctx: &MatchContext<'_, G, X>) -> MatchingState { MatchingState::None }
    fn encounters_into(
        &self,
        ctx: &MatchContext<'_, G, X>,
        state: &mut MatchingState,
        rng: &mut SimRng,
        out: &mut ArenaWriter<'_, G::Action>,
    ) -> socsim_core::Result<()>;
}
```

`encounters_into` pushes one encounter per `out.push(&seats, Credit::All)`
(`seats` has one agent per role). `check` runs once in `assemble` to validate the
matching against the world. The built-in `FixedPlayers::pairs(&[(0, 1), …])`
plays the same pairs every step and rejects unknown agents, an agent listed
twice, and games without exactly two roles. An agent that is not seated in a
step does not decide or play that step.

A custom matching keeps any state it needs **in `X`** (updated by your own
mechanism), not in its own struct — `MatchingState` currently has only `None`,
and state in the matching struct is not part of a snapshot.

---

## One step

`assemble` registers three mechanisms, in this order:

| Mechanism | Phases | Does |
|---|---|---|
| [`game_match`](mechanisms/game-match.md) | PreStep, Decision | PreStep: clears the play arena, `payoff`, `n_played`. Decision: asks the `Matching` for this step's encounters. |
| [`game_decide`](mechanisms/game-decide.md) | Decision, Reward | Decision: each seated agent's rule `decide`s (or its held action is used); the action is checked with `check_action`. Reward: every rule-driven agent's `learn_step`. |
| [`game_play`](mechanisms/game-play.md) | Interaction | checks every seat is filled, computes payoffs with the `OutcomeRule`, aggregates them per agent with `PayoffAggregation`, and appends a `StepRecord` for every agent to the step history. |

Because mechanisms in the same phase run in registration order, `game_match`
always seats agents before `game_decide` asks them for actions. Per step:

```
PreStep      game_match   reset arena / payoff / n_played
Decision     game_match   Matching → encounters
             game_decide  decide + check_action → actions, held
Interaction  game_play    OutcomeRule → payoffs → aggregation → history
Reward       game_decide  learn_step (rule state)
```

Mechanisms you add after `assemble` (`.add_mechanism(...)` on the returned
builder) run after the game mechanisms within each phase — a `Reward`-phase
mechanism therefore sees this step's payoffs.

---

## Building: `build_world` and `assemble`

```rust
pub fn build_world<G: StageGame, X: ExperimentState>(
    game: G,
    n_agents: usize,
    x: X,
    initial_held: InitialActions<G::Action>,   // Same(a) | PerAgent(vec)
    rules: &RuleBook<G, X>,
    assign: &Assignment,
    history_len: usize,
    t_max: u64,
    seed: u64,
) -> Result<GameWorld<G, X>, GameError>;

pub fn assemble<G, X, M: Matching<G, X>, O: OutcomeRule<G, X>>(
    world: GameWorld<G, X>,
    matching: M,
    rules: RuleBook<G, X>,
    outcome: O,
    opts: PlayOptions<G::Action>,
    seed: u64,
) -> Result<socsim_engine::SimulationBuilder<GameWorld<G, X>>, GameError>;
```

`build_world` checks: `history_len > 0` (`InvalidHistoryLength`); the
assignment counts and `PerAgent` length equal `n_agents` (`AgentCount`); every
initial action passes `check_action` for every role; every assigned kind is in
the `RuleBook` (`RuleError::UnknownKind`); every `init_state` succeeds.

`assemble` checks: the game has two roles (`UnsupportedRoles`); every rule kind
in the world is in the `RuleBook`; with `OnInvalidAction::Fallback`, every active
kind has a fallback action that is valid for every role; and `Matching::check`.
It sets the engine seed to `derive_seed(seed, &[1])` and returns the builder, so
you can still add a scheduler, a recorder or extra mechanisms before `build()`.
Pass the **same** `seed` to both functions. Calling `.seed(...)` on the returned
builder overrides the engine stream.

### `PlayOptions`

```rust
PlayOptions::new()                                     // Sum + Stop
    .payoff_aggregation(PayoffAggregation::Mean)        // Sum (default) | Mean
    .on_invalid_action(OnInvalidAction::Stop)           // Stop (default) | Fallback(map)
```

- **`PayoffAggregation`** — how an agent's payoffs from several encounters in
  one step combine into `payoffs()[agent]`: `Sum` (default) or `Mean` (divided by
  `n_played`; agents that did not play keep `0`).
- **`OnInvalidAction`** — what happens when a rule returns an action that fails
  `check_action` for its role:
  - `Stop` (default): the step fails with an error.
  - `Fallback(BTreeMap<kind, action>)`: the action is replaced by the kind's
    fallback, and an event `game.invalid_action` is recorded with payload
    `{ "agent", "kind", "role", "action" }` (the rejected action, `Debug`-formatted).

---

## Determinism & RNG streams

Every stream derives from the one `seed` passed to `build_world` and `assemble`:

| Label | Stream |
|---|---|
| `derive_seed(seed, &[0])` | world initialisation (`Assignment::Shuffled`) |
| `derive_seed(seed, &[1])` | the engine (scheduler, `Matching` and `decide` draws via the step RNG) |
| `derive_seed(seed, &[2])` | `decision_root`, stored in the world and reserved for decision-time streams |
| `derive_seed(seed, &[3, agent, generation])` | rule-state initialisation (`RuleInitContext::init_seed`) |

Labels `[2]` and `[3]` are therefore reserved in a game simulation — use `[4]`
and above for your own streams (see the
[RNG labelling convention](library.md#rng-stream-labeling-convention)). Agents
learn in sorted `AgentId` order and decide in the scheduler's activation order;
with the same seed and the same code a run is reproducible step for step.
