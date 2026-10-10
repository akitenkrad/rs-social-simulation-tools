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
| `llm` | LLM-backed decision rules, the response store and the replay manifest (see [LLM decisions](#llm-decisions-feature-llm)); implies `core`. The `game_llm_decide` mechanism and `assemble_with_llm` also need `engine` | `socsim-core`, `socsim-llm` (default features only — no networking) |
| `live` | `live_backend_factory()` (the cache-free Ollama → OpenAI backend); implies `llm` | as `llm`, plus `socsim-llm/live` |

Without `llm`, the crate does not depend on `socsim-llm`; it never depends on
`socsim-config` or `socsim-runner`. A simulation needs `core` and `engine`:

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

With the `llm` feature, `assemble_with_llm` registers a fourth mechanism,
[`game_llm_decide`](mechanisms/game-llm-decide.md) (Decision, Reward), between
`game_decide` and `game_play`: it fills the seats of agents whose kind is an LLM
rule (see [LLM decisions](#llm-decisions-feature-llm)).

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

## LLM decisions (feature `llm`)

With the `llm` feature, an agent's action can come from a language model. LLM
rules are not `DecisionRule`s: they are driven by a separate mechanism,
[`game_llm_decide`](mechanisms/game-llm-decide.md) (`LlmDriver`), which collects
every LLM decision of the step, sends them as one batch, and only then fixes the
actions. Immediate rules (`game_decide`) and LLM rules (`game_llm_decide`) can be
mixed in the same world — each kind is owned by exactly one of the two drivers.

```toml
socsim-games  = { git = "https://github.com/akitenkrad/rs-social-simulation-tools", branch = "main", package = "socsim-games", features = ["llm", "engine"] }
socsim-core   = { git = "https://github.com/akitenkrad/rs-social-simulation-tools", branch = "main", package = "socsim-core" }
socsim-engine = { git = "https://github.com/akitenkrad/rs-social-simulation-tools", branch = "main", package = "socsim-engine" }
socsim-llm    = { git = "https://github.com/akitenkrad/rs-social-simulation-tools", branch = "main", package = "socsim-llm" }
```

Everything below lives in `socsim_games::llm`. Add the `live` feature (to
`socsim-games`) only when you want the built-in Ollama → OpenAI backend; tests
and offline replays need no networking.

### Two-stage decisions: `prepare` → batch → `resolve`

```rust
pub trait LlmDecisionRule<G: StageGame, X: ExperimentState>: Send + Sync {
    fn name(&self) -> &str;
    fn init_state(&self, ctx: &RuleInitContext, game: &G) -> Result<X::PolicyState, RuleError>;
    /// Stage 1: render the request. Pure; no RNG; the state is read-only.
    fn prepare(
        &self,
        state: &X::PolicyState,
        ctx: &DecisionContext<'_, G, X>,
    ) -> Result<DecisionRequest, RuleError>;
    /// Stage 2: turn the reply into an action, a retry, or a fallback.
    fn resolve(
        &self,
        state: &mut X::PolicyState,
        req: &DecisionRequest,
        reply: &LlmReply,
        rctx: &mut ResolveContext<'_, G>,
    ) -> Resolution<G::Action>;
    /// Reward phase. Never calls the model.
    fn learn_step(
        &self,
        state: &mut X::PolicyState,
        ctx: &LearnContext<'_, G, X>,
    ) -> Result<(), RuleError> { Ok(()) }
}
```

- `prepare` returns a `DecisionRequest { system, prompt, attempt }` (`attempt`
  starts at 0). The generation conditions — temperature, seed, `max_tokens`,
  `think`, … — are not chosen by the rule; the driver attaches them from the
  `LlmConfig` in `LlmSetup` and from the decision unit.
- `resolve` receives the `LlmReply { text, thinking, usage, cache_hit }` and a
  `ResolveContext { game, role, previous, fallback_rng, t }`, where `previous`
  is the agent's held action from the previous step and `fallback_rng` is a
  stream derived from the decision unit. It returns
  `Resolution::Accept { action, flags }`, `Resolution::Retry(next_request)` or
  `Resolution::GiveUp { action, fallback, violation }`.
- The model is called **only** inside the driver's Decision phase.
  `learn_step` (Reward) only updates the rule's state.

### Kinds across two drivers: `KindCatalog` and `build_world_with`

`build_world` only knows a `RuleBook`. When some kinds are LLM rules, describe
every kind with a `KindCatalog` and build the world with `build_world_with`
(same arguments as `build_world`, with `&catalog` in place of `&rules`):

```rust
let catalog = KindCatalog::new()
    .with_rules(&rules)?      // kinds owned by game_decide
    .with_llm(&llm_rules)?;   // kinds owned by game_llm_decide
```

- A kind registered by both books is `RuleError::DuplicateKind`.
- Kind ids follow the sorted union of all kind names, so they do not depend on
  which driver owns a kind or on registration order. `catalog.kinds()`,
  `catalog.kind_id(kind)` and `catalog.driver_name(kind)` (`"game_decide"` or
  `"game_llm_decide"`) expose the result.
- `build_world(.., &rules, ..)` is exactly
  `build_world_with(.., &KindCatalog::from_rules(&rules), ..)`.
- `assemble` checks that the world's kind list equals its `RuleBook`'s, and
  `assemble_with_llm` that it equals the catalog of `rules` + `llm_rules`; a
  difference is `GameError::InvalidKinds { expected, actual }`. A world with LLM
  kinds is therefore assembled with `assemble_with_llm`.
- Each driver skips the kinds it does not own: `game_decide` leaves the seats of
  LLM agents empty, and `game_llm_decide` fills them.

### Example: an LLM agent against Tit-for-Tat

The backend here is `ScriptedClient`, so the program runs offline. Replace the
factory with `live_backend_factory()` (feature `live`) to call a real model.

```rust
use std::sync::Arc;

use socsim_games::llm::{
    assemble_with_llm, BackendFactory, BackendIdentity, LabelParser, LlmPolicy,
    LlmReplayManifest, LlmRuleBook, LlmSetup, MarkdownTableRenderer, OwnHistoryObs, ReplayMode,
    ResponseStore, WithLlm,
};
use socsim_games::{
    build_world_with, AssignGroup, AssignKind, Assignment, FixedPlayers, InitialActions,
    KindCatalog, MatrixGame, NormalFormOutcome, PlayOptions, RuleBook, TitForTat,
};
use socsim_llm::mock::ScriptedClient;

type X = WithLlm<()>; // the replay manifest lives in the experiment state

fn main() {
    // Prisoner's dilemma with labels the model answers with.
    let game = MatrixGame::bimatrix(
        &[vec![3.0, 0.0], vec![5.0, 1.0]],
        &[vec![3.0, 5.0], vec![0.0, 1.0]],
    )
    .unwrap()
    .with_labels(vec![
        vec!["C".to_owned(), "D".to_owned()],
        vec!["C".to_owned(), "D".to_owned()],
    ])
    .unwrap();

    // Immediate rules (game_decide) and LLM rules (game_llm_decide).
    let mut rules: RuleBook<MatrixGame, X> = RuleBook::new();
    rules.register("tit_for_tat", Arc::new(TitForTat::new(0))).unwrap();
    let mut llm_rules: LlmRuleBook<MatrixGame, X> = LlmRuleBook::new();
    llm_rules
        .register(
            "llm",
            Arc::new(LlmPolicy::new(
                OwnHistoryObs::new(5),
                MarkdownTableRenderer::default(),
                LabelParser::strict(),
            )),
        )
        .unwrap();

    // One catalog of every kind, across both drivers.
    let catalog = KindCatalog::new()
        .with_rules(&rules)
        .unwrap()
        .with_llm(&llm_rules)
        .unwrap();

    // Response store + backend. The backend factory is called once per worker.
    let store = ResponseStore::in_memory("ipd-llm", ReplayMode::RecordMisses);
    let identity = BackendIdentity {
        model: "scripted".to_owned(),
        endpoint: "mock://scripted".to_owned(),
    };
    let backend: BackendFactory = Arc::new(|| {
        Ok(Box::new(ScriptedClient::constant(
            "scripted",
            r#"{"action": "D", "reason": "defect"}"#,
        )))
    });

    let seed = 7;
    let rounds = 5;
    let x = WithLlm {
        inner: (),
        llm: LlmReplayManifest::for_store(&store, identity.clone()),
    };
    let world = build_world_with(
        game,
        2,
        x,
        InitialActions::Same(0),
        &catalog,
        &Assignment::Ordered(vec![
            AssignGroup { kind: AssignKind::Rule("llm".to_owned()), count: 1 },
            AssignGroup { kind: AssignKind::Rule("tit_for_tat".to_owned()), count: 1 },
        ]),
        rounds,
        rounds as u64,
        seed,
    )
    .unwrap();

    let setup = LlmSetup::new(llm_rules, store, backend, identity);
    let mut simulation = assemble_with_llm(
        world,
        FixedPlayers::pairs(&[(0, 1)]),
        rules,
        setup,
        NormalFormOutcome,
        PlayOptions::new(),
        seed,
    )
    .unwrap()
    .build();

    for _ in 0..rounds {
        simulation.step().unwrap();
        println!("held = {:?}", simulation.world().held());
    }
}
```

```text
held = [1, 0]
held = [1, 1]
held = [1, 1]
held = [1, 1]
held = [1, 1]
```

`assemble_with_llm(world, matching, rules, llm, outcome, opts, seed)` takes the
same arguments as `assemble` plus the `LlmSetup`, requires
`X: HasLlmManifest` (see [manifest](#manifest-and-what-a-resume-guarantees)),
and registers `game_match` → `game_decide` → `game_llm_decide` → `game_play`.
`OnInvalidAction::Fallback` applies to `game_decide` kinds only: list only
`RuleBook` kinds in its map (an LLM kind there is `RuleError::UnknownKind`); LLM
kinds fall back through their `RetryProtocol`.

### Building blocks of `LlmPolicy`

`LlmPolicy::new(obs, renderer, parser)` is the built-in `LlmDecisionRule`. It
is one generic type over three parts — what the agent observes, how that
becomes a prompt, and how the reply becomes an action:

```rust
LlmPolicy::new(OwnHistoryObs::new(5), MarkdownTableRenderer::default(), LabelParser::strict())
    .with_retry(RetryProtocol { max_retries: 2, fallback: Fallback::Previous })  // default: 1, Previous
    .with_max_turns(32)                                                          // default: 16
```

Its state is an `LlmConversation { max_turns, turns }` in `X::PolicyState`, so
the policy requires `X::PolicyState: Slot<LlmConversation>`
(`BuiltinPolicyState` has an `Llm` variant under `llm`). Each decision appends a
`ConversationTurn { t, action, attempts, gave_up, payoff }` (`action` is the
`Debug` form; `payoff` is filled in Reward); the oldest turn is dropped beyond
`max_turns`. The record is for analysis — it is not replayed into the prompt.

**Observations** (`ObservationProvider<G, X>`, built from the `DecisionContext`
only — the information structure of [default views](#information-structure-default-views-view_for-and-omniscient)
holds):

| Provider | `Obs` | Content |
|---|---|---|
| `OwnHistoryObs::new(window)` | `OwnHistory<G::Action>` | `t`, `role`, own last `window` `StepRecord`s (oldest first), and the last action of each current opponent (`None` if it has none) |
| `CountsObs` (`G: FiniteGame`) | `ActionCounts` | `role`, own last action, and how many current opponents last played each action. It has no `t`, so agents in the same situation produce the same prompt and share one call |

**Renderers** (`GameRenderer<G, Obs>`: `rules` → the system prompt, `turn` → the
prompt, `feedback` → the text appended on a retry):

| Renderer | Pairs with | System prompt |
|---|---|---|
| `MarkdownTableRenderer` (`G: FiniteGame`) | `OwnHistoryObs` | the payoff table from `action_label` (a bimatrix for two roles, one table per role otherwise) and the reply format `{"action": "<action label>", "reason": "<reason>"}` |
| `CountsRenderer` (`G: FiniteGame`) | `CountsObs` | the role, the available labels, and the same reply format |

**Parsers** (`ActionParser<G>`):

- `LabelParser::strict()` — the reply must be a JSON object, bare or in a single
  Markdown fence (```` ``` ```` or ```` ```json ````), whose `"action"` string
  equals one of the role's labels exactly. Anything else is a violation.
- `LabelParser::lenient()` — tries the strict match first; if it fails, finds
  the first label mentioned anywhere in the reply
  (`socsim_llm::extract_first_choice`: case-insensitive, on word boundaries,
  earliest wins) and sets `ParseFlags::lenient_match`.
  `.with_synonyms(label, &[..])` adds words that count as that label.
  **Lenient matching requires every label of every role to be ASCII
  `[A-Za-z0-9_-]+`**; this is checked when rule state is initialised, so
  `build_world_with` fails with `RuleError::Unsupported` otherwise. Strict
  matching accepts any label.
- `.require_reason(true)` additionally requires a string `"reason"` field; a
  missing reason is a violation even in lenient mode.
- `NumberParser::new(field).bounds(lo, hi).out_of_range(OutOfRange::Clamp)` is
  a standalone helper (`parse_number(raw) -> ParseOutcome<f64>`) for numeric
  answers: a JSON number or a numeric string (`value_was_string`), finite only;
  out of range is `Violation` (default) or clamped (`clamped`). It is not an
  `ActionParser` for `MatrixGame`.

Each accepted reply carries `ParseFlags { fenced, value_was_string, clamped,
lenient_match }`; each rejected one a `Violation { kind, message }` with
`ViolationKind::{Format, Label, Range, InvalidAction}`.

**Retries and fallbacks** — `RetryProtocol { max_retries, fallback }`. While
`attempt < max_retries`, a violation produces a retry whose prompt is the
previous prompt plus the renderer's `feedback`; all retries of a step are sent
together as the next batch. After that the policy gives up with the fallback:

| `Fallback` | Action |
|---|---|
| `Previous` (default) | the agent's held action from the previous step |
| `UniformRandom` | a uniform draw by the parser, from `fallback_rng` (derived from the decision unit; never the engine stream) |
| `Specified(action)` | `action` |

The driver checks every final action with `check_action`; an invalid one stops
the step with `RuleError::InvalidAction`.

### One Decision step of `game_llm_decide`

1. **Prepare** — for every seated agent whose kind is an LLM kind, in the
   scheduler's activation order, call `prepare`.
2. **Deduplicate** — requests with the same cache key (below) are sent once.
3. **Look up** — on the main thread, look each request up in the
   `ResponseStore` (not in `Live` mode). A miss under `ReplayOnly` stops the step.
4. **Execute** — send only the misses to the `LlmExecutor`.
5. **Resolve** — call `resolve` in decision order; collect `Retry` requests and
   repeat 2–5 for them as one more batch; `GiveUp` fixes the fallback.
6. **Write back** — write every fixed action to its seat and to `held`.

**Seeds.** With `temperature == 0` every request uses the constant seed
`derive_seed(decision_root, &[0])`, so identical prompts share one key, one call
and one stored record. With any other temperature, the seed is
`derive_seed(decision_root, &[t, agent, 0, 0, attempt])` (one per decision and
attempt), so requests from different agents generally differ. The fallback
stream is `derive_seed(decision_root, &[t, agent, 0, 0, u64::MAX])`. The driver
never draws from the engine's step RNG, so the order of activation and the
number of workers do not change the result.

**Setup.**

```rust
let setup = LlmSetup::new(llm_rules, store, backend, identity)
    .executor(LlmExecutor::Threaded { workers: 4 })   // default: Sequential
    .options(options)                                 // default: ExecutorOptions::default()
    .config(LlmConfig::deterministic());              // default; the seed is always overwritten
```

`config` is the base of every request's `LlmConfig`; the driver overwrites its
`seed`, and its `system` whenever the request has one (the built-in policy always
does).

### Response store

`ResponseStore` replaces `socsim_llm::PromptCache` for game simulations. Each
request is identified by a versioned key:

```rust
pub struct CacheKeyV1 {
    pub version: u32,                 // 1
    pub backend: BackendIdentity,     // { model, endpoint }
    pub system: Option<String>,
    pub prompt: String,
    pub temperature_bits: u32,        // f32::to_bits, so 0.2 and 0.8 differ (and 0.0 / -0.0 too)
    pub seed: u64,
    pub omit_seed: bool,
    pub max_tokens: Option<u32>,
    pub think: Option<bool>,
    pub allow_blank: bool,
}
```

The lookup key is the 64-bit FNV-1a of `canonical_json()`; the full key is
stored with each `ResponseRecord { key, text, thinking, usage,
backend_metadata }` and compared on every hit, so a digest collision is
detected (`StoreError::KeyCollision`) rather than served. A hit restores the
thinking trace and token usage, so a replayed call is recorded exactly like a
live one except for `cache_hit`.

| Constructor | Storage |
|---|---|
| `ResponseStore::in_memory(store_id, mode)` | memory only (tests) |
| `ResponseStore::open(dir, mode, run_tag, seed)` | `dir/store_id` (created on first open, then reused), `dir/base.jsonl` (read-only; may be absent) and this run's shard `dir/shards/<run_tag>-<seed>.jsonl` (read, and appended to) |

| `ReplayMode` | Store lookup | On a miss | Writes |
|---|---|---|---|
| `ReplayOnly` | yes | the step fails | none |
| `RecordMisses` | yes | the backend is called | the new record, to this run's shard |
| `Live` | no | (every request goes to the backend) | none |

**Shards are merged only when you ask.** `open` reads the base and its own
shard, not other runs' shards. Call `ResponseStore::merge_shards(dir)` after
the runs that recorded (for example after a multi-seed sweep) to fold every
shard into `base.jsonl` (written to a unique temporary file and renamed) and
delete the shards; it returns a `MergeReport { records_added,
duplicate_records, shards_merged }`. Two records with the same key and
different content are `StoreError::Conflict`. Merging changes the base digest,
so a manifest written before the merge no longer matches the store.

### Executor and backends

```rust
pub type BackendFactory = Arc<dyn Fn() -> Result<Box<dyn LlmClient>, LlmError> + Send + Sync>;
```

The executor builds its backend from the factory **in the thread that uses it**
(an `LlmClient` is not `Send`): once per batch for `Sequential`, once per worker
for `Threaded`, and not at all for a batch in which every request hit the store. The backend must not cache — the response store is the cache.
`live_backend_factory()` (feature `live`) wraps `socsim_llm::build_live_backend`;
in tests return a `socsim_llm::mock::ScriptedClient`.

| `LlmExecutor` | Runs calls on |
|---|---|
| `Sequential` (default) | the main thread, one at a time |
| `Threaded { workers }` | `workers` threads, fed through a bounded queue; only the main thread writes to the store |

| `ExecutorOptions` field | Default | Meaning |
|---|---|---|
| `timeout` | 120 s | per call; **`Threaded` only** (a `Sequential` call cannot be interrupted). A timed-out call is not resent and fails the step |
| `max_backend_retries` | 3 | resends after a backend error; when exhausted the step fails with `RuleError::Backend` |
| `backoff_base` | 1 s | the k-th resend waits `backoff_base × 2^(k-1)` |
| `queue_per_worker` | 2 | queue capacity = `workers × queue_per_worker` |

`ExecutorOptions` is `#[non_exhaustive]`: start from `ExecutorOptions::default()`
and assign fields. `workers == 0` or `queue_per_worker == 0` is a backend error.
Because seeds come from the decision unit and resolution follows the decision
order, `Sequential` and `Threaded` produce identical worlds.

### Manifest and what a resume guarantees

A real model can answer the same request differently, so a snapshot resumes
exactly only if the resumed run sees the same responses. The world therefore
records which responses it was run against: `WithLlm<X> { inner, llm }` wraps
your experiment state (delegating `PolicyState` and `view_for` to `inner`) and
carries an `LlmReplayManifest { store_id, base_digest, mode, backend }`, built
with `LlmReplayManifest::for_store(&store, identity)`. `assemble_with_llm`
requires `X: HasLlmManifest`, which `WithLlm<X>` implements.

- On every Decision the driver compares the world's manifest with its own
  store (`store_id`, `base_digest`, `mode`) and `BackendIdentity`; any
  difference stops the step. A snapshot restored into a simulation that has
  already run is therefore checked on its next Decision as well.
- `verify_snapshot(&snapshot, &store, &identity)` performs the same check
  before you call `restore` and returns `GameError::ManifestMismatch`.
- A resumed run matches the uninterrupted run **only** under `ReplayOnly`, or
  under `RecordMisses` when every request after the resume hit the store
  (`cache_hit` in `game.llm_call`, `llm.cache_hit_rate == 1`). Under `Live`
  nothing is guaranteed. The mode is part of the manifest, so resume with the
  mode the snapshot was taken with.

```rust
// Record, then fold the shards into the base.
let store = ResponseStore::open(dir, ReplayMode::RecordMisses, "record", seed)?;
// ... build with this store and run ...
ResponseStore::merge_shards(dir)?;

// Replay offline, snapshot, and resume against the same store.
let store = ResponseStore::open(dir, ReplayMode::ReplayOnly, "replay", seed)?;
// ... build, run, let snapshot = simulation.snapshot(); ...
let store = ResponseStore::open(dir, ReplayMode::ReplayOnly, "resume", seed)?;
verify_snapshot(&snapshot, &store, &identity)?;
// ... build with this store, then resumed.restore(snapshot) ...
```

### What is recorded

At the end of each Decision with at least one LLM decision, the driver writes a
row to table `"game"` (`record_row`):

| Column | Denominator |
|---|---|
| `llm.parse_fail_rate` | attempts (logical calls, retries included) |
| `llm.clamp_rate`, `llm.value_was_string_rate`, `llm.lenient_rate` | attempts |
| `llm.cache_hit_rate` | attempts |
| `llm.retry_rate` | decisions (agents that needed at least one retry) |
| `llm.giveup_rate` | decisions (agents that fell back) |

Every attempt — including one that shared a deduplicated call — is recorded as
an event `game.llm_call` with payload:

| Key | Value |
|---|---|
| `agent`, `kind` | agent index and rule kind |
| `unit`, `unit_index` | decision unit (`"step"`) and its index (`0`) |
| `attempt`, `resolution` | 0-based attempt; `"accept"`, `"retry"` or `"giveup"` |
| `system`, `prompt` | the request |
| `reply`, `thinking`, `usage` | the response, the thinking trace and token usage (also on a store hit) |
| `seed` | the generation seed |
| `cache_hit` | `true` if served by the store |
| `deduplicated` | `true` if this attempt reused another agent's call in the same batch |
| `key` | the request key's digest, 16 lowercase hex digits |
| `flags` | `ParseFlags` (all `false` for `retry` / `giveup`) |

Steps without any LLM decision record nothing.

---

## Determinism & RNG streams

Every stream derives from the one `seed` passed to `build_world` and `assemble`:

| Label | Stream |
|---|---|
| `derive_seed(seed, &[0])` | world initialisation (`Assignment::Shuffled`) |
| `derive_seed(seed, &[1])` | the engine (scheduler, `Matching` and `decide` draws via the step RNG) |
| `derive_seed(seed, &[2])` | `decision_root`, stored in the world; `game_llm_decide` derives its generation seeds and fallback streams from it (see [Seeds](#one-decision-step-of-game_llm_decide)) |
| `derive_seed(seed, &[3, agent, generation])` | rule-state initialisation (`RuleInitContext::init_seed`) |

Labels `[2]` and `[3]` are therefore reserved in a game simulation — use `[4]`
and above for your own streams (see the
[RNG labelling convention](library.md#rng-stream-labeling-convention)). Agents
learn in sorted `AgentId` order and decide in the scheduler's activation order;
with the same seed and the same code a run is reproducible step for step.
