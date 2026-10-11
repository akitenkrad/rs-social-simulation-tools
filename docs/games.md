**English** | [日本語](games.ja.md)

# Normal-form games (`socsim-games`)

`socsim-games` runs game-theoretic simulations on the socsim engine: agents are
seated into encounters of a **stage game**, each one chooses an action through a
**decision rule**, an **outcome rule** turns the action profile into payoffs, and
the rules learn from what happened — once per step, inside the ordinary
[6-phase tick loop](architecture.md#the-6-phase-tick-loop).

The crate is library-only (no `ModulePack`, not wired into the `socsim` binary).
This page covers the building blocks, where their state lives, what a rule is
allowed to see, how to assemble a runnable simulation, and the built-in learning
rules and decision scopes (see
[Learning rules and decision scope](#learning-rules-and-decision-scope)), and the
per-step metrics mechanism `game_metrics` (see [Metrics](#metrics)). Separately from the
simulation, the crate ships classic games and solvers for finite games — Nash
equilibria, dominance, minimax and logit QRE (see
[Solving finite games](#solving-finite-games)).

---

## Cargo features

| Feature | Adds | socsim dependencies |
|---|---|---|
| *(default)* | the stage-game traits (`StageGame`, `NormalFormGame`, `FiniteGame`, `AggregateGame`), `MatrixGame`, the built-in games (`catalog`), mixed strategies (`mixed`), the solvers (`solve`), the learning-rule cores (`learn`), `AggregateSpec` and the metric action ids (`metric_action_ids`) | **none** — the default build depends on no `socsim-*` crate (the solvers' LP backend is the pure-Rust `microlp` crate) |
| `core` | the world (`GameWorld`), rules (including the learning rules), outcome rules, matching, views, `build_world`, the metrics mechanism `GameMetrics` | `socsim-core` |
| `engine` | `assemble` and `assemble_with_aggregates` (both return a `SimulationBuilder`); implies `core` | `socsim-core`, `socsim-engine` |
| `metrics` | the `game.action_entropy` column of `GameMetrics` (see [Metrics](#metrics)); implies `core` | `socsim-core`, `socsim-metrics` (default features off — only its dependency-free `stats` module) |
| `llm` | LLM-backed decision rules, the response store and the replay manifest (see [LLM decisions](#llm-decisions-feature-llm)); implies `core`. The `game_llm_decide` mechanism and `assemble_with_llm` also need `engine` | `socsim-core`, `socsim-llm` (default features only — no networking) |
| `live` | `live_backend_factory()` (the cache-free Ollama → OpenAI backend); implies `llm` | as `llm`, plus `socsim-llm/live` |

Without `llm`, the crate does not depend on `socsim-llm`, and without `metrics`
it does not depend on `socsim-metrics`; it never depends on
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

### Built-in games: `catalog`

`socsim_games::catalog` provides five classic two-player games. Each one
implements `FiniteGame`, so it plugs into `build_world`, the built-in rules and
the [solvers](#solving-finite-games) exactly like a `MatrixGame`; `to_matrix()`
converts it to an equivalent `MatrixGame` with the same labels. Action labels
are ASCII, and where the game has a cooperative side (`C`, `stag`) it is action
`0`.

| Type | Actions `0, 1, …` | Constructors | Checked at construction |
|---|---|---|---|
| `PrisonersDilemma` | `C`, `D` | `new(T, R, P, S)`; `weak(b)` = `new(b, 1, 0, 0)` | `T > R > P ≥ S` (`P = S` is allowed, so `weak(b)` needs `b > 1`) |
| `BattleOfSexes` | `B`, `V` | `new(a, b)`; `Default`: `a = [[2, -1], [-1, 1]]`, `b = [[1, -1], [-1, 2]]` | `(B, B)` and `(V, V)` are strict equilibria; role 0 strictly prefers `(B, B)`, role 1 strictly prefers `(V, V)` |
| `MatchingPennies` | `H`, `T` | `new(a, b)`; `Default`: `a = [[-1, 1], [1, -1]]`, `b = -a` | zero-sum (`b == -a` entry by entry) and no pure equilibrium |
| `StagHunt` | `stag`, `hare` | `new(a, b)`; `Default`: `a = [[9, 0], [8, 7]]`, `b = [[9, 8], [0, 7]]` | `(stag, stag)` and `(hare, hare)` are strict equilibria, and `(stag, stag)` gives each role strictly more than `(hare, hare)` |
| `RockPaperScissors` | `R`, `P`, `S` | `new(win, loss)`; `Default`: `win = loss = 1` | `win > 0` and `loss > 0` |

`a[i][j]` / `b[i][j]` are role 0's / role 1's payoffs when role 0 plays `i` and
role 1 plays `j`, as in `MatrixGame::bimatrix`. The prisoner's dilemma gives
role 0 `[[R, S], [T, P]]` and role 1 the transpose. It deliberately has no
`Default`: there is no single standard set of `T, R, P, S`, so the payoffs are
always written out. In the default matching pennies, role 0 wins when the
coins differ. In rock–paper–scissors the winner receives `win`, the loser
`-loss` and a tie `0`; with the default `win = loss = 1` this is the standard
antisymmetric zero-sum form (with `win ≠ loss` the game is not zero-sum).

Every constructor returns `Result<_, CatalogError>`: `NonFinite` for a NaN or
infinite payoff, `Condition(_)` when the payoffs lack the game's defining
shape. The types serialise with serde, and deserialisation runs the same
checks, so an invalid game cannot be loaded from a snapshot or a file either.

```rust
use socsim_games::catalog::{PrisonersDilemma, RockPaperScissors};
use socsim_games::{FiniteGame, NormalFormGame};

fn main() {
    // T > R > P >= S is checked when the game is built.
    let pd = PrisonersDilemma::new(5.0, 3.0, 1.0, 0.0).unwrap();
    assert!(PrisonersDilemma::new(3.0, 3.0, 1.0, 0.0).is_err()); // T = R

    println!("{} vs {}: {}", pd.action_label(0, 0), pd.action_label(1, 1), pd.payoff(0, &[0, 1]));
    // C vs D: 0

    let rps = RockPaperScissors::default(); // win = 1, loss = 1
    let matrix = rps.to_matrix();           // the same payoffs as a MatrixGame
    println!("{} beats {}: {}", matrix.action_label(0, 1), matrix.action_label(1, 0), matrix.payoff(0, &[1, 0]));
    // P beats R: 1
}
```

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
    fn learn_encounter(
        &self,
        state: &mut X::PolicyState,
        outcome: &EncounterOutcome<'_, G::Action>,
        ctx: &LearnContext<'_, G, X>,
    ) -> Result<(), RuleError> { Ok(()) }
    fn needs_encounter_learning(&self) -> bool { false }
    fn needs_pair_history(&self) -> bool { false }
    fn mixed(&self, state: &X::PolicyState, role: usize, game: &G) -> Option<Vec<f64>> { None }
}
```

`learn_encounter`, `needs_encounter_learning` and `needs_pair_history` are
optional; they matter when an agent plays several encounters per step (see
[Per-encounter learning](#per-encounter-learning) and
[Pair history](#pair-history)). `mixed` is optional too: it returns the mixed
strategy the rule would play next in `role`, read from its state without
changing it or drawing randomness, and is called only when the simulation
reports mixed strategies (see
[Reporting mixed strategies](#reporting-mixed-strategies)).

Note the receivers: `decide` and the learning methods take `&self`. A rule object is
shared code; everything it remembers about an agent lives in the
`state: &mut X::PolicyState` it is handed (see
[where rule state lives](#where-rule-state-lives-and-why-it-matters-for-snapshots)).

Built-in rules:

| Rule | Bound | Behaviour | State slot |
|---|---|---|---|
| `Fixed::new(action)` | any `StageGame` | always plays `action` (validated for every role at init) | `Stateless` |
| `UniformRandom` | `FiniteGame` | uniform draw over the role's actions from `rng` | `Stateless` |
| `TitForTat::new(coop)` | `FiniteGame`, two roles, two actions per role | plays `coop` first, then the opponent's last action | `TftState { last_opponent }` |

Further built-in rules — `Mixed` (a fixed mixed strategy),
`MyopicBestResponse`, `FictitiousPlay`, `LogitChoice`, `RothErev` and the
repeated-game automata `Automaton` — are described in
[Learning rules and decision scope](#learning-rules-and-decision-scope).

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
through the `Slot<T>` trait (`slot`, `slot_mut`, `from_slot`). `Fixed`,
`UniformRandom`, `Mixed` and `MyopicBestResponse` need `Slot<Stateless>`,
`TitForTat` needs `Slot<TftState>`, and the learning rules need
`Slot<BeliefState>` (`FictitiousPlay`), `Slot<EstimateState>` (`LogitChoice`),
`Slot<PropensityState>` (`RothErev`) or `Slot<AutomatonPolicyState>`
(`Automaton`). `BuiltinPolicyState` (the `PolicyState` of `X = ()`) implements
all of them. To mix
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
| per-pair encounter history (ring of `history_len` per pair; only when a rule asks for it) | through `DecisionContext::pair_history` (see [Pair history](#pair-history)) |
| the matching's state (`MatchingState`, e.g. the `RoundRobin` cursor) | serialised with the world |
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
| `t()`, `me()`, `unit()`, `scope()` | step, own id, decision unit (`DecisionUnit::Step` by default) and its `DecisionScope` |
| `role()`, `encounter()` | own role and encounter for this decision (`encounter()` is `None` under `PerAgentRole`) |
| `opponents()` | the other agents **in the same encounter** (under `PerAgentRole`, in all encounters of that role) |
| `my_history()` | own past `StepRecord`s (action, payoff, `n_played`) |
| `opponent_history(id)` | the actions of `id` — **`None` unless `id` is a current opponent** |
| `pair_history(id)` | this agent's past encounters with `id` — `None` unless the pair history is enabled and `id` is a current opponent |
| `game()` | the stage game |
| `view()` | `X::view_for(me)` |
| `omniscient()` | unrestricted data (see below) |

How these change with the decision scope is listed under
[Decision scope](#decision-scope).

**`LearnContext`** (passed to `learn_step` and `learn_encounter`): `t()`, `me()`,
`scope()`, `record()` (own record for this step), `my_history()`, `outcomes()` (own encounters this step,
in `EncounterId` order — each `EncounterOutcome` has `role`, `seats`, `actions`,
`payoffs`, `credit`), `game()`, `view()`, `omniscient()`.

`view_for(me)` is where you decide what `X` reveals: return only `me`'s private
signal for an incomplete-information game, or a public reputation vector for an
indirect-reciprocity model.

**`omniscient()`** returns an `OmniscientContext` with every agent's id, held
action and history, the full `X`, and — from a `DecisionContext`, when enabled —
the history of any pair of agents (`pair_history(a, b)`). Calling it **removes the information
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
matching against the world; `ctx.decision_scope()` tells it the
[decision scope](#decision-scope) in use. The built-in
`FixedPlayers::pairs(&[(0, 1), …])` plays the same pairs every step and rejects
unknown agents, an agent listed twice, and games without exactly two roles. The
built-in [`RoundRobin`](#roundrobin) seats every pair of agents every step. An
agent that is not seated in a step does not decide or play that step.

A custom matching keeps any state it needs **in `X`** (updated by your own
mechanism), not in its own struct — `MatchingState` is a closed enum (`None`, and
`RoundRobin { cursor }` for the built-in round-robin), and state in the matching
struct is not part of a snapshot.

---

## One step

`assemble` registers three mechanisms, in this order:

| Mechanism | Phases | Does |
|---|---|---|
| [`game_match`](mechanisms/game-match.md) | PreStep, Decision | PreStep: clears the play arena, `payoff`, `n_played`. Decision: asks the `Matching` for this step's encounters. |
| [`game_decide`](mechanisms/game-decide.md) | Decision, Reward | Decision: each seated agent's rule `decide`s once per decision unit (or its held action is used); the action is checked with `check_action`. Reward: every rule-driven agent's `learn_encounter` (per encounter, when the rule asks for it) and then `learn_step`; with `PlayOptions::report_mixed(true)`, finally each rule's post-learning mixed strategy (see [Reporting mixed strategies](#reporting-mixed-strategies)). |
| [`game_play`](mechanisms/game-play.md) | Interaction | checks every seat is filled, computes payoffs with the `OutcomeRule`, appends the encounters to the pair history (when enabled), aggregates payoffs per agent with `PayoffAggregation`, and appends a `StepRecord` for every agent to the step history; under `assemble_with_aggregates`, finally reduces the game's public aggregates into one row (see [Public aggregates](#public-aggregates-aggregategame)). |

Because mechanisms in the same phase run in registration order, `game_match`
always seats agents before `game_decide` asks them for actions. Per step:

```
PreStep      game_match   reset arena / payoff / n_played
Decision     game_match   Matching → encounters
             game_decide  decide + check_action → actions, held
Interaction  game_play    OutcomeRule → payoffs → pair history → aggregation → history
                          (→ aggregate row, under assemble_with_aggregates)
Reward       game_decide  learn_encounter (per encounter) + learn_step (rule state)
                          (→ post-learning mixed reports, with report_mixed)
PostStep     game_metrics (optional, added by you) → one "game" row of metrics
```

With the `llm` feature, `assemble_with_llm` registers a fourth mechanism,
[`game_llm_decide`](mechanisms/game-llm-decide.md) (Decision, Reward), between
`game_decide` and `game_play`: it fills the seats of agents whose kind is an LLM
rule (see [LLM decisions](#llm-decisions-feature-llm)).

Mechanisms you add after `assemble` (`.add_mechanism(...)` on the returned
builder) run after the game mechanisms within each phase — a `Reward`-phase
mechanism therefore sees this step's payoffs. The metrics mechanism
[`game_metrics`](mechanisms/game-metrics.md) is added this way; it runs in
PostStep, after everything above, and reads the completed step (see
[Metrics](#metrics)).

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

pub fn assemble_with_aggregates<G: AggregateGame, X, M: Matching<G, X>, O: OutcomeRule<G, X>>(
    world: GameWorld<G, X>,
    matching: M,
    rules: RuleBook<G, X>,
    outcome: O,
    opts: PlayOptions<G::Action>,
    seed: u64,
    aggregate_history_len: usize,
) -> Result<socsim_engine::SimulationBuilder<GameWorld<G, X>>, GameError>;
```

`build_world` checks: `history_len > 0` (`InvalidHistoryLength`); the
assignment counts and `PerAgent` length equal `n_agents` (`AgentCount`); every
initial action passes `check_action` for every role; every assigned kind is in
the `RuleBook` (`RuleError::UnknownKind`); every `init_state` succeeds.

`assemble` checks: the game has two roles (`UnsupportedRoles`); every rule kind
in the world is in the `RuleBook`; with `OnInvalidAction::Fallback`, every active
kind has a fallback action that is valid for every role; `report_mixed(true)`
is not combined with `DecisionScope::PerEncounter` (`GameError::Metrics`); and
`Matching::check`. `assemble_with_aggregates` does the same and additionally
requires `aggregate_history_len > 0` (`InvalidAggregateHistoryLength`); it is
`assemble` plus the public aggregate table of
[Public aggregates](#public-aggregates-aggregategame).
It sets the engine seed to `derive_seed(seed, &[1])` and returns the builder, so
you can still add a scheduler, a recorder or extra mechanisms before `build()`.
Pass the **same** `seed` to both functions. Calling `.seed(...)` on the returned
builder overrides the engine stream.

### `PlayOptions`

```rust
PlayOptions::new()                                     // Sum + Stop + PerStep
    .payoff_aggregation(PayoffAggregation::Mean)        // Sum (default) | Mean
    .on_invalid_action(OnInvalidAction::Stop)           // Stop (default) | Fallback(map)
    .decision_scope(DecisionScope::PerEncounter)        // PerStep (default) | PerAgentRole | PerEncounter
    .report_mixed(true)                                 // false (default) | true
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
- **`DecisionScope`** — how many decisions an agent makes when it sits in several
  encounters in one step; see [Decision scope](#decision-scope).
- **`report_mixed`** — when `true`, `game_decide` stores every rule-driven
  agent's post-learning mixed strategy at the end of each Reward phase, readable
  through `GameWorld::reported_mixed`; see
  [Reporting mixed strategies](#reporting-mixed-strategies). Rejected together
  with `DecisionScope::PerEncounter`.

---

## Learning rules and decision scope

Besides `Fixed`, `UniformRandom` and `TitForTat`, the crate ships classic
learning rules and repeated-game automata, a round-robin matching, and a
**decision scope** that says how many decisions an agent makes when it sits in
several encounters in one step. The learning arithmetic itself lives in
`socsim_games::learn` (fictitious play, Roth–Erev, logit choice, automata),
which is part of the default build and depends on no `socsim-*` crate; the
`DecisionRule` wrappers below (feature `core`) store that state in the world.

### The rule table

Every check in the last column runs in `init_state`, so a misconfigured rule is
rejected by `build_world` (as `GameError::Rule(RuleError::Unsupported(_))`)
before the simulation starts.

| Rule | Constructor | State slot (`BuiltinPolicyState` variant) | Valid when (otherwise rejected at build time) |
|---|---|---|---|
| `Mixed` | `Mixed::new(q)` — a fixed mixed strategy in action-index order | `Stateless` | every `q[i]` finite and `≥ 0`; `Σ q = 1` within `1e-12`; `q.len()` equals the action count of **every** role |
| `MyopicBestResponse` | `MyopicBestResponse::new()`, `.tie_break_lowest()` (default) / `.tie_break_random()`, `.first_action(a)` (default `0`) | `Stateless` | `first_action` is a valid action for every role |
| `FictitiousPlay` | `FictitiousPlay::new()`, `.initial_counts(vec)` (default all zero), `.tie_break_lowest()` (default) / `.tie_break_random()` | `BeliefState` (`Belief`) | exactly two roles; initial counts finite and `≥ 0`, one per opponent action for both roles |
| `LogitChoice` | `LogitChoice::new(lambda)`, `.estimate(EstimateMode::LastOpponent \| EmpiricalFrequency)` (default `EmpiricalFrequency`) | `EstimateState` (`Estimate`) | exactly two roles; `lambda` finite and `≥ 0` |
| `RothErev` | `RothErev::new(phi, epsilon, initial_propensity)` | `PropensityState` (`Propensity`) | `0 ≤ phi < 1`, `0 ≤ epsilon < 1`, `initial_propensity > 0` (all finite); **every** payoff of every role and profile is finite and `> 0` |
| `Automaton` | `Automaton::new(kind, coop)`, `kind` ∈ `AutomatonKind::{AllC, AllD, Grim, Wsls, Tft, Gtft { generosity }}` | `AutomatonPolicyState` (`Automaton`) | every role has exactly two actions; `coop` is `0` or `1`; GTFT `generosity` in `[0, 1]` |
| `TitForTat` (unchanged) | `TitForTat::new(coop)` | `TftState` (`TitForTat`) | two roles, two actions each, `coop` valid |

`BuiltinPolicyState` (the `PolicyState` of `X = ()`) implements `Slot<_>` for
every slot in the table. With your own `PolicyState`, implement `Slot<T>` for
the slots of the rules you register, as in
[`PolicyState` and `Slot`](#policystate-and-slot). The four state types expose
`global()` and `by_opponent()` (a slice of `(AgentId, state)` in ascending
`AgentId` order) for inspection through `world.policies().state(agent)`.

`TitForTat` keeps working exactly as before: one `last_opponent` per agent,
overwritten by each of the agent's encounters in `EncounterId` order. For a
tit-for-tat that remembers each opponent separately, use
`Automaton::new(AutomatonKind::Tft, coop)` under `DecisionScope::PerEncounter`.

`MyopicBestResponse` best-responds to the opponents' most recent actions. What
it reads depends on the [decision scope](#decision-scope):

- `PerStep` — each current opponent's action in the previous step, from the
  step history.
- `PerEncounter` — the last action recorded in the [pair history](#pair-history)
  for this encounter's opponent.
- `PerAgentRole` — for each current opponent of this role that has a record,
  the last action in the pair history; the rule maximises the expected payoff
  against the uniform mix of those actions.

While no applicable action exists (the first step, an opponent never met, or
an opponent that did not play in the previous step under `PerStep`) it plays
`first_action`. The rule sets `needs_pair_history() == true`, so assigning it
creates the pair history. Under `PerAgentRole` and `PerEncounter` it supports
two-player games only; any other game stops the step with
`RuleError::Unsupported`.

### Fictitious play

Each agent holds a **belief**: one count per opponent action. `counts()` is the
unnormalised vector that the rule uses — the initial (fictitious) counts plus
one per observed opponent action; `empirical_counts()` holds the observations
only. The rule plays a best response to the counts, i.e. it maximises
`Σ_j counts[j] · u(a, j)`; with all-zero counts every action ties. Ties are
detected by exact `f64` equality and resolved by `TieBreak::LowestIndex`
(default, no randomness) or `TieBreak::Random` (one `gen_range` over the tied
actions). Fictitious play is defined for **two-player** games only.

Updating is **simultaneous**: every agent decides from the counts through the
previous step, and the observations are added only after the step has been
played (in Reward). No alternating variant is offered, because the two update
orders are different dynamics. In Shapley's 3×3 game, simultaneous fictitious
play cycles forever through six action pairs, each run of a pair about 1.47
times as long as the one before; letting the second player respond to the first
player's already-updated count instead collapses play into a three-step cycle
in which each pair lasts a single step.

The core can be run without a simulation (default build; add `rand = "0.8"`
for the RNG):

```rust
use rand::{rngs::StdRng, SeedableRng};
use socsim_games::learn::fictitious::{simultaneous_step, FictitiousBelief};
use socsim_games::MatrixGame;

fn main() {
    // Shapley's 3×3 game.
    let game = MatrixGame::bimatrix(
        &[vec![1.0, 0.0, 0.0], vec![0.0, 1.0, 0.0], vec![0.0, 0.0, 1.0]],
        &[vec![0.0, 0.0, 1.0], vec![1.0, 0.0, 0.0], vec![0.0, 1.0, 0.0]],
    )
    .unwrap();
    let mut row = FictitiousBelief::with_defaults(&game, 0).unwrap();
    let mut col = FictitiousBelief::with_defaults(&game, 1).unwrap();
    let mut rng = StdRng::seed_from_u64(0); // unused with lowest-index ties

    // Lengths of the runs of identical action pairs.
    let mut runs: Vec<((usize, usize), u32)> = Vec::new();
    for _ in 0..200 {
        let pair = simultaneous_step(&game, &mut row, &mut col, &mut rng).unwrap();
        match runs.last_mut() {
            Some((last, n)) if *last == pair => *n += 1,
            _ => runs.push((pair, 1)),
        }
    }
    let first: Vec<_> = runs.iter().take(9).collect();
    println!("{first:?}");
    // [((0, 0), 1), ((0, 2), 2), ((2, 2), 3), ((2, 1), 5), ((1, 1), 8),
    //  ((1, 0), 12), ((0, 0), 18), ((0, 2), 27), ((2, 2), 40)]
}
```

### Roth–Erev reinforcement

Each agent holds one propensity `q_j > 0` per own action (all equal to
`initial_propensity` at the start) and chooses action `j` with probability
`q_j / Σ_k q_k`. After playing action `k` and receiving payoff `x`, every
propensity is updated with the three-parameter rule

```
q_j ← (1 − φ) · q_j + E_j,     E_j = (1 − ε) · x        if j = k
                                E_j = ε · x / (N − 1)    otherwise
```

where `φ` (`phi`) is forgetting, `ε` (`epsilon`) experimentation and `N` the
number of own actions (with `N = 1` the whole payoff goes to the single action).
There is no reference point: payoffs are used as they are, which is why the
game's payoffs must all be strictly positive (checked at build time), and a
realised payoff that is not finite and positive — possible with a custom
outcome rule — stops the step with an error. To use a game with zero or
negative payoffs, shift its payoffs before building it.

### Logit choice

`LogitChoice` estimates each own action's payoff from the opponent's past
actions and plays action `a` with probability proportional to
`exp(λ · û(a))`. The estimate is either the payoff against the opponent's most
recent action (`EstimateMode::LastOpponent`) or the expected payoff against the
opponent's empirical action frequencies (`EstimateMode::EmpiricalFrequency`,
the default). Before any observation all estimates are zero, so the choice is
uniform; `λ = 0` is always uniform. The softmax subtracts the largest estimate
before exponentiating, so large `λ` does not overflow. Logit choice is defined
for **two-player** games only.

```rust
use rand::{rngs::StdRng, SeedableRng};
use socsim_games::learn::logit::{EstimateMode, LogitState};
use socsim_games::learn::roth_erev::RothErevState;
use socsim_games::MatrixGame;

fn main() {
    // A prisoner's dilemma shifted so every payoff is strictly positive.
    let game = MatrixGame::bimatrix(
        &[vec![4.0, 1.0], vec![6.0, 2.0]],
        &[vec![4.0, 6.0], vec![1.0, 2.0]],
    )
    .unwrap();

    // Roth–Erev: initial propensity 1, phi = 0.1, epsilon = 0.2.
    let mut re = RothErevState::new(&game, 0, 1.0, 0.1, 0.2).unwrap();
    re.update(0, 3.0).unwrap(); // chose action 0, earned 3
    let q = re.propensities();
    let p = re.selection_probabilities().unwrap();
    println!("propensities [{:.1}, {:.1}]", q[0], q[1]);
    println!("probabilities [{:.4}, {:.4}]", p[0], p[1]);
    // propensities [3.3, 1.5]
    // probabilities [0.6875, 0.3125]

    // Logit: uniform before any observation, then exp(lambda * estimate).
    let mut logit = LogitState::new(&game, 0, 1.0, EstimateMode::LastOpponent).unwrap();
    println!("before {:?}", logit.selection_probabilities(&game, 0).unwrap());
    logit.observe(0).unwrap(); // the opponent played C
    let p = logit.selection_probabilities(&game, 0).unwrap();
    println!("after  [{:.4}, {:.4}]", p[0], p[1]);
    // before [0.5, 0.5]
    // after  [0.1192, 0.8808]

    let mut rng = StdRng::seed_from_u64(7);
    let action = logit.choose_action(&game, 0, &mut rng).unwrap();
    assert!(action < 2);
}
```

(`0.9 · 1 + 0.8 · 3 = 3.3` and `0.9 · 1 + 0.2 · 3 = 1.5`; after the opponent
played `C`, the estimates are `4` and `6`, so `p(C) = 1 / (1 + e²)`.)

### Repeated-game automata

`Automaton::new(kind, coop)` plays a fixed repeated-game strategy in a game
where every role has exactly two actions. `coop` is the index of the
cooperative action (`C`); the other index is `D`. All automata start with `C`
except `AllD`.

| `AutomatonKind` | Next action |
|---|---|
| `AllC` / `AllD` | always `C` / always `D` |
| `Grim` | `C` until the opponent has played `D` once, then `D` forever |
| `Tft` | the opponent's previous action |
| `Wsls` (win-stay, lose-shift) | `(C, C) → C`, `(C, D) → D`, `(D, C) → D`, `(D, D) → C`, where `(own, opponent)` is the previous step |
| `Gtft { generosity }` | `C` after the opponent's `C`; after the opponent's `D`, `C` with probability `generosity`, else `D` |

### Random numbers

Learning rules draw only from the engine stream that `game_decide` passes to
`decide`; `learn_encounter` and `learn_step` take no RNG. The stream is consumed
in the scheduler's activation order and, within one agent, in decision-unit
order (see [Decision scope](#decision-scope)). Sampling is fixed: a rule that
samples from a probability vector (`Mixed`, `LogitChoice`, `RothErev`, GTFT)
draws one uniform `f64` and walks the cumulative probabilities in
action-index order (if rounding leaves the draw past the total, the last action
with positive probability is taken). Random tie-breaking draws one `gen_range`
over the tied actions, and a single best action consumes nothing.
`Fixed`, `TitForTat`, the deterministic automata, lowest-index ties, and GTFT at
`generosity` `0` or `1` consume no randomness.

### Decision scope

`PlayOptions::decision_scope(scope)` chooses what one decision covers. It
matters as soon as a matching seats an agent in several encounters in a step.

| | `DecisionScope::PerStep` (default) | `DecisionScope::PerAgentRole` | `DecisionScope::PerEncounter` |
|---|---|---|---|
| decisions per agent per step | one, written to all its seats | one per role it holds that step, in ascending role order | one per seat, in `EncounterId` order |
| `ctx.unit()` / `ctx.scope()` | `DecisionUnit::Step` / `PerStep` | `DecisionUnit::AgentRole(role)` / `PerAgentRole` | `DecisionUnit::Encounter(id)` / `PerEncounter` |
| `ctx.role()` | the role (the same in all its seats) | that role | the seat's role |
| `ctx.encounter()` | its last encounter (highest `EncounterId`) | `None` | that encounter |
| `ctx.opponents()` | the other agents of that encounter | every opponent in that role's encounters, deduplicated, ascending | the other agents of that encounter |
| writes `held` | yes | no | no |
| `StepRecord::action` | the action | `None` | `None` |

Under `PerAgentRole` and `PerEncounter` an agent can play different actions in
one step, so there is no single action to hold or record: `held` keeps its
previous value and the step history records `action: None` (payoff and
`n_played` are recorded as usual). Rules that need the actions of past
encounters under these scopes read them from `LearnContext::outcomes()` or from
the [pair history](#pair-history). `Held` agents play their held action in all
their seats under every scope.

`PerStep` requires every agent to hold **one role** per step. A matching that
can tell up front checks this in `Matching::check` through
`MatchContext::decision_scope()`: `RoundRobin` returns
`GameError::InvalidMatching` from `assemble` under `PerStep` with three or more
agents or with `both_orders(true)`. For a matching that cannot be checked in
advance, `game_decide` stops the step with a mechanism error built from
`RuleError::AmbiguousRole { agent }` (`agent 0 holds more than one role in a
per-step decision`) when an agent holds two different roles in a `PerStep`
step — `Held` agents included.

```rust
use std::sync::Arc;

use socsim_games::{
    assemble, build_world, AssignGroup, AssignKind, Assignment, Automaton, AutomatonKind,
    DecisionScope, InitialActions, MatrixGame, NormalFormOutcome, PlayOptions, RoundRobin,
    RuleBook,
};

fn main() {
    let game = MatrixGame::bimatrix(
        &[vec![3.0, 0.0], vec![5.0, 1.0]],
        &[vec![3.0, 5.0], vec![0.0, 1.0]],
    )
    .unwrap();
    let mut rules = RuleBook::new();
    rules.register("tft", Arc::new(Automaton::new(AutomatonKind::Tft, 0))).unwrap();
    let assign = Assignment::Ordered(vec![AssignGroup {
        kind: AssignKind::Rule("tft".to_owned()),
        count: 3,
    }]);

    for scope in [DecisionScope::PerStep, DecisionScope::PerAgentRole, DecisionScope::PerEncounter] {
        let world = build_world(game.clone(), 3, (), InitialActions::Same(0), &rules, &assign, 10, 10, 1)
            .unwrap();
        // With three agents, agent 1 sits in role 1 of (0, 1) and role 0 of (1, 2).
        let result = assemble(
            world,
            RoundRobin::new(),
            rules.clone(),
            NormalFormOutcome,
            PlayOptions::new().decision_scope(scope),
            1,
        );
        match result {
            Ok(_) => println!("{scope:?}: ok"),
            Err(error) => println!("{scope:?}: {error}"),
        }
    }
    // PerStep: invalid matching: RoundRobin assigns more than one role to an agent in a per-step decision
    // PerAgentRole: ok
    // PerEncounter: ok
}
```

LLM decisions support `PerStep` only: `assemble_with_llm` rejects the other
scopes with `RuleError::Unsupported`.

### `RoundRobin`

`RoundRobin::new()` seats **every pair** of agents once per step. Agents are
sorted by `AgentId` and the pairs are pushed in lexicographic order — for four
agents `(0, 1), (0, 2), (0, 3), (1, 2), (1, 3), (2, 3)` — with the lower id in
role 0. `.both_orders(true)` also pushes the reversed pair immediately after
each pair (`(0, 1), (1, 0), (0, 2), (2, 0), …`), so every agent plays both roles
against every opponent. Self-play is not supported: `.self_play(true)` is
rejected by `assemble` with `GameError::InvalidMatching`, as is a game without
exactly two roles. The matching draws no randomness.

Its state is `MatchingState::RoundRobin { cursor }`, stored in the world (and
therefore in a snapshot): `cursor` counts the rounds played and grows by one
per step.

With `n` agents each agent plays `n − 1` encounters per step (`2(n − 1)` with
both orders), so `PayoffAggregation` matters: `Sum` adds the encounter payoffs
and `Mean` divides by `n_played`. Three agents playing `C`, `D`, `C` in the
prisoner's dilemma above receive `[3, 10, 3]` under `Sum` and `[1.5, 5, 1.5]`
under `Mean`.

### Per-encounter learning

`DecisionRule` has three hooks besides `learn_step`:

| Method | Default | Meaning |
|---|---|---|
| `learn_encounter(state, outcome, ctx)` | no-op | learn from one encounter |
| `needs_encounter_learning()` | `false` | when `true`, `game_decide` calls `learn_encounter` |
| `needs_pair_history()` | `false` | when `true` for any assigned kind, `build_world` creates the [pair history](#pair-history) |

In Reward, `game_decide` visits rule-driven agents in ascending `AgentId`
order. For each agent whose rule returns `needs_encounter_learning() == true`
it first calls `learn_encounter` once per encounter of that agent, in
`EncounterId` order, and then calls `learn_step`. `LearnContext::scope()`
reports the decision scope.

`FictitiousPlay`, `LogitChoice`, `RothErev` and `Automaton` use this to switch
their state with the scope. Under `PerEncounter` they keep **one state per
opponent** (`by_opponent()`), created on the first decision or observation
against that opponent and initialised like a fresh rule; `learn_encounter`
updates the state of that encounter's opponent and `learn_step` does nothing.
Under `PerStep` and `PerAgentRole` they keep a **single global state**
(`global()`); `learn_encounter` does nothing and `learn_step` folds all of the
step's encounters into it, in `EncounterId` order. The global state is shaped
for role 0 (for example, a fictitious-play belief has one count per role-1
action), and an agent that holds role 1 uses the same state; a per-opponent
state is shaped for the role in which the agent first meets that opponent.
This makes no difference when both roles have the same number of actions;
otherwise a decision in the other role can fail.

### Pair history

The pair history keeps, for each unordered pair of agents, the last
`history_len` encounters between them — the step and both actions. It costs
memory, so `build_world` creates it only when some assigned rule kind returns
`needs_pair_history() == true`. Among the built-in rules only
`MyopicBestResponse` does; custom rules can ask for it too. When it is not
created, the `pair_history` field is absent
from the world's JSON (and from a snapshot).

- `DecisionContext::pair_history(other)` returns a `PairView` only for a
  **current opponent** of the decision unit. It returns `None` when the pair
  history is not enabled or `other` is not in `ctx.opponents()`; a current
  opponent that this agent has never met gives an **empty** view.
- `PairView::iter()` yields `(t, my_action, other_action)` from oldest to
  newest, oriented to the agent that asked; `len()` and `is_empty()` count the
  retained encounters. With `both_orders(true)` a pair meets twice per step and
  both encounters are recorded, in `EncounterId` order.
- `ctx.omniscient().pair_history(a, b)` reads any pair of agents in the world
  (taken from a `DecisionContext`; the `OmniscientContext` of a
  `LearnContext` returns `None`).
- `game_play` appends this step's two-seat encounters after it has computed
  payoffs, so a decision never sees its own step.

A rule that reads the pair history, in a round-robin under `PerEncounter`:

```rust
use std::sync::Arc;

use socsim_core::SimRng;
use socsim_games::{
    assemble, build_world, AssignGroup, AssignKind, Assignment, Automaton, AutomatonKind,
    BuiltinPolicyState, DecisionContext, DecisionRule, DecisionScope, InitialActions, MatrixGame,
    NormalFormOutcome, PlayOptions, RoundRobin, RuleBook, RuleError, RuleInitContext, Slot,
    Stateless,
};

/// Tit-for-tat that reads the pair history instead of keeping its own state.
struct PairTitForTat;

impl DecisionRule<MatrixGame, ()> for PairTitForTat {
    fn name(&self) -> &str {
        "pair_tit_for_tat"
    }
    fn init_state(&self, _: &RuleInitContext, _: &MatrixGame) -> Result<BuiltinPolicyState, RuleError> {
        Ok(<BuiltinPolicyState as Slot<Stateless>>::from_slot(Stateless))
    }
    fn decide(
        &self,
        _state: &mut BuiltinPolicyState,
        ctx: &DecisionContext<'_, MatrixGame, ()>,
        _rng: &mut SimRng,
    ) -> Result<usize, RuleError> {
        let opponent = ctx.opponents()[0];
        let pair = ctx.pair_history(opponent).expect("pair history is enabled");
        // (t, my action, their action), oldest first; cooperate on first meeting.
        Ok(pair.iter().last().map_or(0, |(_, _, theirs)| *theirs))
    }
    fn needs_pair_history(&self) -> bool {
        true // asks build_world to create the pair history
    }
}

fn main() {
    let game = MatrixGame::bimatrix(
        &[vec![3.0, 0.0], vec![5.0, 1.0]],
        &[vec![3.0, 5.0], vec![0.0, 1.0]],
    )
    .unwrap();
    let mut rules = RuleBook::new();
    rules.register("pair_tft", Arc::new(PairTitForTat)).unwrap();
    rules.register("all_d", Arc::new(Automaton::new(AutomatonKind::AllD, 0))).unwrap();
    rules.register("all_c", Arc::new(Automaton::new(AutomatonKind::AllC, 0))).unwrap();
    let assign = Assignment::Ordered(
        ["pair_tft", "all_d", "all_c"]
            .iter()
            .map(|kind| AssignGroup { kind: AssignKind::Rule((*kind).to_owned()), count: 1 })
            .collect(),
    );
    let world = build_world(game, 3, (), InitialActions::Same(0), &rules, &assign, 10, 10, 1).unwrap();
    let mut simulation = assemble(
        world,
        RoundRobin::new(),
        rules,
        NormalFormOutcome,
        PlayOptions::new().decision_scope(DecisionScope::PerEncounter),
        1,
    )
    .unwrap()
    .build();

    let mut totals = [0.0; 3];
    for _ in 0..10 {
        simulation.step().unwrap();
        for (total, payoff) in totals.iter_mut().zip(simulation.world().payoffs()) {
            *total += payoff;
        }
    }
    println!("{totals:?}");
    // [39.0, 64.0, 30.0]
}
```

Over 10 rounds the pair tit-for-tat is exploited once by `all_d` (`0 + 9 · 1`)
and cooperates throughout with `all_c` (`10 · 3`).

### Worked example: a round-robin IPD tournament

Five automata play the iterated prisoner's dilemma against each other for 200
rounds (`crates/socsim-games/examples/ipd_tournament.rs`, shortened here).
`PerEncounter` gives each agent one automaton per opponent, so tit-for-tat
punishes the defector without punishing anyone else:

```rust
use std::sync::Arc;

use socsim_games::{
    assemble, build_world, AssignGroup, AssignKind, Assignment, Automaton, AutomatonKind,
    DecisionScope, InitialActions, MatrixGame, NormalFormOutcome, PayoffAggregation,
    PlayOptions, RoundRobin, RuleBook,
};

const ROUNDS: usize = 200;

fn main() {
    // Prisoner's dilemma, action 0 = C, 1 = D.
    let game = MatrixGame::bimatrix(
        &[vec![3.0, 0.0], vec![5.0, 1.0]],
        &[vec![3.0, 5.0], vec![0.0, 1.0]],
    )
    .unwrap();

    let strategies = [
        ("all_c", AutomatonKind::AllC),
        ("all_d", AutomatonKind::AllD),
        ("tft", AutomatonKind::Tft),
        ("grim", AutomatonKind::Grim),
        ("wsls", AutomatonKind::Wsls),
    ];
    let mut rules = RuleBook::new();
    for (kind, automaton) in &strategies {
        rules.register(kind, Arc::new(Automaton::new(automaton.clone(), 0))).unwrap();
    }

    let seed = 1;
    let world = build_world(
        game,
        strategies.len(), // agent i runs strategies[i]
        (),
        InitialActions::Same(0),
        &rules,
        &Assignment::Ordered(
            strategies
                .iter()
                .map(|(kind, _)| AssignGroup { kind: AssignKind::Rule((*kind).to_owned()), count: 1 })
                .collect(),
        ),
        ROUNDS,
        ROUNDS as u64,
        seed,
    )
    .unwrap();

    let mut simulation = assemble(
        world,
        RoundRobin::new(), // every pair once per step: 10 encounters for 5 agents
        rules,
        NormalFormOutcome,
        PlayOptions::new()
            .decision_scope(DecisionScope::PerEncounter) // one decision per seat
            .payoff_aggregation(PayoffAggregation::Sum),
        seed,
    )
    .unwrap()
    .build();

    let mut totals = vec![0.0; strategies.len()];
    for _ in 0..ROUNDS {
        simulation.step().unwrap();
        for (total, payoff) in totals.iter_mut().zip(simulation.world().payoffs()) {
            *total += payoff;
        }
    }
    for ((kind, _), total) in strategies.iter().zip(&totals) {
        println!("{kind}: {total}");
    }
    // all_c: 1800
    // all_d: 2008
    // tft: 1999
    // grim: 1999
    // wsls: 1900
}
```

```sh
cargo run -p socsim-games --example ipd_tournament --features core,engine
```

Against `all_d`, tit-for-tat loses the first round and then both defect:
`0 + 199 · 1 = 199` for tit-for-tat and `5 + 199 · 1 = 204` for `all_d` (the
full example also prints this pair). Against each of the other three
cooperators it earns `200 · 3 = 600`, hence `1999`. `all_d` comes out ahead
because it exploits every opponent at least once: tit-for-tat and grim in the
first round, `wsls` every other round (win-stay, lose-shift alternates between
`C` and `D` against a defector) and `all_c` every round.

---

## LLM decisions (feature `llm`)

With the `llm` feature, an agent's action can come from a language model. LLM
rules are not `DecisionRule`s: they are driven by a separate mechanism,
[`game_llm_decide`](mechanisms/game-llm-decide.md) (`LlmDriver`), which collects
every LLM decision of the step, sends them as one batch, and only then fixes the
actions. Immediate rules (`game_decide`) and LLM rules (`game_llm_decide`) can be
mixed in the same world — each kind is owned by exactly one of the two drivers.
LLM decisions support only the default
[decision scope](#decision-scope) `PerStep`: `assemble_with_llm` rejects
`PerAgentRole` and `PerEncounter` with `RuleError::Unsupported`.

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

## Metrics

`GameMetrics` is a read-only mechanism, named `game_metrics`, that records one
row of per-step game statistics — action shares, payoff mean and spread,
cooperation rate, regret, distance to equilibrium and public aggregates — to the
recorder's table `"game"`. It is not registered by `assemble`: build it from the
game and add it to the returned builder. It runs in **PostStep**, after every
game mechanism, so it sees the completed step. It draws no randomness and changes
no model state; the only thing it writes into the world is its own
bookkeeping for the convergence stop and the cumulative source (see
[the mechanism page](mechanisms/game-metrics.md)).

### Adding `game_metrics`

`GameMetrics::new(&game, MetricsOptions)` takes the game by reference — build it
before the game moves into `build_world` — and returns
`Result<GameMetrics<G>, GameError>` (`G: FiniteGame`). The options are a builder:

| `MetricsOptions` method | Default | Effect |
|---|---|---|
| `new()` / `default()` | — | the defaults below |
| `cooperate_label(label)` | off | adds `game.coop_rate`; `label` must match **exactly one** action label in **every** role, else `GameError::Metrics` |
| `regret(on)` | on for two-role games, off otherwise | adds or removes `game.regret` |
| `mixed_source(MixedSource)` | `Played` | which mixed-strategy profile `game.regret` and `game.eq_distance` evaluate |
| `equilibrium(EquilibriumTarget)` | `None` | `Solve` adds `game.eq_distance` |
| `stop(tol, window)` | off | requests the end of the run once the action shares stop changing; `tol` finite and `> 0`, `window ≥ 1`, else `GameError::Metrics` |
| `action_entropy(on)` | on | feature `metrics` only: adds or removes `game.action_entropy` |

A two-agent prisoner's dilemma, Tit-for-Tat against an unconditional defector:

```rust
use std::sync::Arc;

use socsim_games::{
    assemble, build_world, AssignGroup, AssignKind, Assignment, EquilibriumTarget, Fixed,
    FixedPlayers, GameMetrics, InitialActions, MatrixGame, MetricsOptions, NormalFormOutcome,
    PlayOptions, RuleBook, TitForTat,
};
use socsim_log::InMemoryRecorder;

fn main() {
    let game = MatrixGame::bimatrix(
        &[vec![3.0, 0.0], vec![5.0, 1.0]],
        &[vec![3.0, 5.0], vec![0.0, 1.0]],
    )
    .unwrap()
    .with_labels(vec![vec!["C".into(), "D".into()]; 2])
    .unwrap();

    // Built from the game before the game moves into the world.
    let metrics = GameMetrics::new(
        &game,
        MetricsOptions::new()
            .cooperate_label("C")                    // adds game.coop_rate
            .equilibrium(EquilibriumTarget::Solve)   // adds game.eq_distance
            .stop(1e-9, 3),                          // stop after 3 unchanged steps
    )
    .unwrap();

    let mut rules = RuleBook::new();
    rules.register("tit_for_tat", Arc::new(TitForTat::new(0))).unwrap();
    rules.register("fixed_defect", Arc::new(Fixed::new(1))).unwrap();

    let seed = 1;
    let world = build_world(
        game, 2, (), InitialActions::Same(0), &rules,
        &Assignment::Ordered(vec![
            AssignGroup { kind: AssignKind::Rule("tit_for_tat".into()), count: 1 },
            AssignGroup { kind: AssignKind::Rule("fixed_defect".into()), count: 1 },
        ]),
        100, 100, seed,
    )
    .unwrap();

    let mut sim = assemble(world, FixedPlayers::pairs(&[(0, 1)]), rules,
                           NormalFormOutcome, PlayOptions::new(), seed)
        .unwrap()
        .add_mechanism(Box::new(metrics))           // runs in PostStep
        .recorder(Box::new(InMemoryRecorder::new()))
        .build();
    sim.run().unwrap();                             // ends early: the stop rule fired

    let recorder = sim.recorder().as_any().unwrap()
        .downcast_ref::<InMemoryRecorder>().unwrap();
    for row in recorder.metrics().iter().filter(|row| row.t <= 2) {
        println!("t={} {} = {}", row.t, row.key, row.value);
    }
    println!("stopped at t = {}", recorder.metrics().last().unwrap().t);
}
```

(`socsim-log` provides `InMemoryRecorder`; any `Recorder` works.) Output:

```text
t=1 game.played_share.a0 = 0.5
t=1 game.played_share.a1 = 0.5
t=1 game.coop_rate = 0.5
t=1 game.mean_payoff = 2.5
t=1 game.payoff_sd = 2.5
t=1 game.regret = 1
t=1 game.eq_distance = 2
t=2 game.played_share.a0 = 0
t=2 game.played_share.a1 = 1
t=2 game.coop_rate = 0
t=2 game.mean_payoff = 1
t=2 game.payoff_sd = 0
t=2 game.regret = 0
t=2 game.eq_distance = 0
stopped at t = 5
```

At `t = 1` Tit-for-Tat cooperates and the defector defects: half the seats play
`C`, the payoffs are 0 and 5, the cooperator could have gained 1 by defecting
(regret 1), and the profile `(C, D)` is at L1 distance 2 from the unique
equilibrium `(D, D)`. From `t = 2` both defect. With feature `metrics` the same
program also prints `game.action_entropy` (`0.6931471805599453` = ln 2 at
`t = 1`, `0` at `t = 2`) after `game.coop_rate`.

### Columns

Every column is written with `record_row(t, "game", …)`, so a recorder that fans
rows out (the default) sees the keys `game.<column>`:

| Column | Appears when | Value |
|---|---|---|
| `game.played_share.<id>` | the step has at least one seat | pooled ids: seats choosing that action ÷ all seats; per-role ids: seats of that role choosing that action ÷ seats of that role |
| `game.coop_rate` | `cooperate_label` is set and the step has a seat | seats whose action is the cooperation action of the seat's role ÷ all seats |
| `game.action_entropy` | feature `metrics`, `action_entropy(true)` (the default), pooled ids, and the step has a seat | Shannon entropy $-\sum_k p_k \ln p_k$ of the pooled `played_share` values, in nats |
| `game.mean_payoff` | at least one agent played (`n_played() > 0`) | mean of `payoffs()` (after `PayoffAggregation`) over the agents that played |
| `game.payoff_sd` | as `mean_payoff` | population standard deviation (divided by $n$) of the same payoffs |
| `game.aggregate.<name>` | the simulation was assembled with `assemble_with_aggregates` and the reduction is defined | the step's reduced aggregate (see [Public aggregates](#public-aggregates-aggregategame)) |
| `game.regret` | regret is on and the mixed-strategy profile is defined | $\max_r \big[\max_a F_r(a, \sigma_{-r}) - F_r(\sigma)\big]$ (`mixed::regret`) |
| `game.eq_distance` | `EquilibriumTarget::Solve` and the profile is defined | $\min_{\sigma^*} \sum_r \sum_a \lvert \sigma_r(a) - \sigma^*_r(a)\rvert$ over the solved equilibria |

Within a row the columns come in the order played shares, `coop_rate`,
`action_entropy`, `mean_payoff`, `payoff_sd`, aggregates, `regret`,
`eq_distance`.

**An undefined column is left out of the row, never written as NaN.** A step
without encounters records no share, cooperation or entropy column; a step in
which no agent played records no payoff columns. Any value that is not finite
— computed, or a played payoff it reads — stops the step with a mechanism error
before the row is recorded, so `game_metrics` never writes NaN or infinity to
the records or to its state in the world. A recorder that keeps rows natively
may still pad them: `socsim_log::CsvRecorder` fixes a table's columns from its
first row, writes NaN for a column missing from a later row and drops a column
that first appears later (with `llm`, `game_llm_decide` writes its rows to the
same table `"game"`). Prefer a recorder that fans rows out per key when columns
can come and go. The mechanism also checks
every step that the world's game still has the role and action counts it was
built for.

`played_share` counts **seats in the play arena**, not agents: an agent seated in
three encounters counts three times. That is why it is defined under every
[decision scope](#decision-scope), including the scopes that do not write `held`.
A population share — the fraction of agents holding each strategy — is a
different quantity and is not part of `game_metrics`.

### Action ids: `metric_action_ids`

The `<id>` in `game.played_share.<id>` comes from action **indices**, never from
labels, so a column name is a valid metric name whatever the labels contain.
`metric_action_ids(&game)` (default build) returns the ids in column order:

- **pooled** (`MetricScheme::Pooled`) — every role has the same number of
  actions and the same label at each index as role 0: one id per index,
  `a0`, `a1`, …, shared by all roles;
- **per role** (`MetricScheme::PerRole`) — otherwise: `r<role>-a<index>`, role
  by role.

```rust
use socsim_games::{metric_action_ids, GameMetrics, MatrixGame, MetricsOptions};

fn main() {
    let pd = MatrixGame::bimatrix(
        &[vec![3.0, 0.0], vec![5.0, 1.0]],
        &[vec![3.0, 5.0], vec![0.0, 1.0]],
    )
    .unwrap()
    .with_labels(vec![vec!["C".into(), "D".into()]; 2])
    .unwrap();
    // Two roles with different action sets.
    let asymmetric = MatrixGame::new(vec![2, 3], vec![vec![0.0; 6], vec![0.0; 6]])
        .unwrap()
        .with_labels(vec![
            vec!["Up".into(), "Down".into()],
            vec!["L".into(), "M".into(), "R".into()],
        ])
        .unwrap();

    for game in [&pd, &asymmetric] {
        let ids = metric_action_ids(game);
        let listed: Vec<_> = ids.actions().iter().map(|a| a.id.as_str()).collect();
        println!("{:?}: {}", ids.scheme(), listed.join(", "));
    }
    println!("{:?}", metric_action_ids(&asymmetric).id(1, 2));
    println!();

    let metrics = GameMetrics::new(&pd, MetricsOptions::new().cooperate_label("C")).unwrap();
    for (pattern, meaning) in metrics.describe() {
        if pattern.contains('{') {
            println!("[[metric_patterns]]\npattern = {pattern:?}\nmeaning = {meaning:?}\n");
        } else {
            println!("[metrics.{pattern:?}]\nmeaning = {meaning:?}\n");
        }
    }
}
```

The first three lines of the output:

```text
Pooled: a0, a1
PerRole: r0-a0, r0-a1, r1-a0, r1-a1, r1-a2
Some("r1-a2")
```

Each `MetricAction` carries `id`, `role` (`None` when pooled) and `action`; use
the game's `action_label(role, action)` for display. The rest of the output is
used in [Describing the columns](#describing-the-columns-describe).

### Regret, equilibrium distance and `MixedSource`

`game.regret` and `game.eq_distance` evaluate one mixed strategy per role, the
profile $\sigma$. `MetricsOptions::mixed_source` chooses where it comes from:

| `MixedSource` | $\sigma_r$ | Defined when |
|---|---|---|
| `Played` (default) | the share of this step's role-$r$ seats choosing each action | every role has a seat this step |
| `PlayedCumulative` | the same shares over **all** seats since the first step `game_metrics` observed | every role has been played at least once (so also on a later step without encounters) |
| `Reported` | the average, over this step's role-$r$ seats, of the seated agent's reported mix (see [Reporting mixed strategies](#reporting-mixed-strategies)); a `Held` agent's seat counts as the pure strategy it played | every role has a seat this step |

The `PlayedCumulative` counts are kept in the world, so a snapshot carries them
and a resumed run continues the same column. `Reported` needs
`PlayOptions::report_mixed(true)`: without it the first step fails with a
mechanism error, as it does when a seated rule-driven agent has no report (its
rule's `mixed` returned `None`) or a report of the wrong length.

`EquilibriumTarget::Solve` runs [`support_enumeration`](#support_enumeration)
**once, in `GameMetrics::new`**, and keeps the equilibria. It requires a
two-role game, and `new` returns `GameError::Metrics` when the solver fails,
when the equilibrium set is not complete (`is_complete()` is false — a
degenerate game) or when it is empty. `game.eq_distance` is the summed L1
distance to the nearest of those equilibria, between 0 and $2 \times$ the number
of roles.

### Reporting mixed strategies

`PlayOptions::report_mixed(true)` makes `game_decide` call
`DecisionRule::mixed(state, role, game)` at the **end of each Reward phase**,
after every agent has learned, for every rule-driven agent (ascending
`AgentId`) and every role, and store the result in the world.
`GameWorld::reported_mixed(agent, role) -> Option<&[f64]>` reads it.

The reported mix is therefore the **post-learning** mix — the one the next
step's decision will use — not the mix that produced this step's action. A
report is `None` for a `Held` agent, for a kind owned by another driver (an LLM
kind), when the rule returns `None`, and when a returned value is not finite;
the table is cleared at the start of each report, so a value never carries
over from an earlier step.

| Built-in rule | `mixed` reports |
|---|---|
| `Fixed` | `None` |
| `UniformRandom` | $1/n$ on each of the role's $n$ actions |
| `Mixed` | its probabilities (`None` if their length differs from the role's action count) |
| `TitForTat` | the pure strategy on the opponent's last action (`coop` before any) |
| `MyopicBestResponse` | `None` |
| `FictitiousPlay` | the best responses to its belief counts: with `tie_break_lowest()` the pure strategy on the lowest one, with `tie_break_random()` equal weight on every exactly tied one |
| `LogitChoice` | its logit selection probabilities |
| `RothErev` | its selection probabilities |
| `Automaton` | the next action's probabilities — pure, except GTFT after an opponent defection: `generosity` on `coop`, the rest on the other action |
| your own rule | `None` unless you implement `mixed` |

The learning rules report from their single per-agent (`global`) state.
Because `DecisionScope::PerEncounter` keeps one state per opponent, `assemble`
rejects `report_mixed(true)` under that scope (`GameError::Metrics`).
`assemble_with_llm` accepts `report_mixed`; its LLM kinds report `None`.

```rust
use std::sync::Arc;

use socsim_core::AgentId;
use socsim_games::{
    assemble, build_world, AssignGroup, AssignKind, Assignment, FictitiousPlay, FixedPlayers,
    GameMetrics, InitialActions, MatrixGame, MetricsOptions, MixedSource, NormalFormOutcome,
    PlayOptions, RuleBook,
};
use socsim_log::InMemoryRecorder;

fn main() {
    // Matching pennies: role 0 wins on a match, role 1 on a mismatch.
    let game = MatrixGame::zero_sum(&[vec![1.0, -1.0], vec![-1.0, 1.0]]).unwrap();
    let metrics = GameMetrics::new(
        &game,
        MetricsOptions::new().mixed_source(MixedSource::Reported),
    )
    .unwrap();

    let mut rules = RuleBook::new();
    rules
        .register("fictitious", Arc::new(FictitiousPlay::new().tie_break_random()))
        .unwrap();

    let seed = 1;
    let world = build_world(
        game, 2, (), InitialActions::Same(1), &rules,
        &Assignment::Ordered(vec![
            AssignGroup { kind: AssignKind::Rule("fictitious".into()), count: 1 },
            AssignGroup { kind: AssignKind::Held, count: 1 },   // always plays action 1
        ]),
        10, 10, seed,
    )
    .unwrap();

    let mut sim = assemble(world, FixedPlayers::pairs(&[(0, 1)]), rules,
                           NormalFormOutcome, PlayOptions::new().report_mixed(true), seed)
        .unwrap()
        .add_mechanism(Box::new(metrics))
        .recorder(Box::new(InMemoryRecorder::new()))
        .build();

    // Before any Reward phase there is no report.
    println!("before t=1: {:?}", sim.world().reported_mixed(AgentId(0), 0));
    sim.step().unwrap();
    println!("played at t=1: {}", sim.world().held()[0]);
    println!("reported after t=1: {:?}", sim.world().reported_mixed(AgentId(0), 0));
    println!("held agent: {:?}", sim.world().reported_mixed(AgentId(1), 1));

    let recorder = sim.recorder().as_any().unwrap()
        .downcast_ref::<InMemoryRecorder>().unwrap();
    for row in recorder.metrics().iter().filter(|row| row.key == "game.regret") {
        println!("t={} {} = {}", row.t, row.key, row.value);
    }
}
```

```text
before t=1: None
played at t=1: 0
reported after t=1: Some([0.0, 1.0])
held agent: None
t=1 game.regret = 2
```

With no observation yet, fictitious play broke the tie at random and played
`0`; having seen the opponent play `1`, it now reports the pure best response
`1`. The regret uses that report for role 0 and the held agent's played action
for role 1: role 1 loses on a match and would gain 2 by switching.

### Public aggregates: `AggregateGame`

A game can publish values computed from each encounter's action profile —
"was this encounter mutual cooperation", "how many contributed" — that every
rule may read as public information. Implement `AggregateGame`
(default build) on the game:

```rust
pub trait AggregateGame: NormalFormGame {
    fn aggregate_spec(&self) -> &[AggregateSpec];
    /// `out.len() == self.aggregate_spec().len()`; every written value must be finite.
    fn aggregate_into(&self, profile: &[Self::Action], out: &mut [f64]);
}
```

`AggregateSpec::new(name, AggregateReduce::{Sum, Mean})` validates the name: 1–49
bytes, the first an ASCII lowercase letter or digit, the rest lowercase
letters, digits, `_` or `-` (`AggregateSpecError::{EmptyName, InvalidStart,
InvalidCharacter { index }, TooLong { length }}`; deserialisation re-runs the
check). A name is therefore one segment of the metric name
`game.aggregate.<name>`.

Assemble with `assemble_with_aggregates(.., seed, aggregate_history_len)`
instead of `assemble`. At the end of Interaction, `game_play` calls
`aggregate_into` once per encounter (`EncounterId` order) and reduces the
values over the step's encounters — `Sum` adds them (`0` for a step without
encounters), `Mean` averages them (undefined for a step without encounters) —
into one row keyed by `t`. The last `aggregate_history_len` rows are kept in the
world, so snapshots carry them. A non-finite value stops the step.

Rules read the rows through `DecisionContext::aggregate_history()`, which
returns `None` unless the simulation was assembled with aggregates. The rows
are written in Interaction and decisions are made in Decision, so a decision
sees **only completed previous steps** (none at `t = 1`). `AggregateView`
offers `names()`, `len()`, `is_empty()`, `iter()` (`(t, &[Option<f64>])`,
oldest first), `last()` and `column(name)`; an undefined value is `None`.
`game_metrics` records the current step's row as `game.aggregate.<name>`.
`assemble_with_llm` has no aggregate variant: under LLM assembly
`aggregate_history()` is `None` for every rule.

```rust
use std::borrow::Cow;
use std::sync::Arc;

use socsim_core::SimRng;
use socsim_games::{
    assemble_with_aggregates, build_world, ActionError, AggregateGame, AggregateReduce,
    AggregateSpec, AssignGroup, AssignKind, Assignment, BuiltinPolicyState, DecisionContext,
    DecisionRule, FiniteGame, Fixed, FixedPlayers, GameMetrics, InitialActions, MatrixGame,
    MetricsOptions, NormalFormGame, NormalFormOutcome, PlayOptions, RuleBook, RuleError,
    RuleInitContext, StageGame, Stateless,
};
use socsim_log::InMemoryRecorder;

/// The prisoner's dilemma plus two public, profile-level values.
#[derive(Clone, Debug)]
struct PdWithAggregates {
    pd: MatrixGame,
    specs: Vec<AggregateSpec>,
}

impl StageGame for PdWithAggregates {
    type Action = usize;
    fn n_roles(&self) -> usize { self.pd.n_roles() }
    fn check_action(&self, role: usize, action: &usize) -> Result<(), ActionError> {
        self.pd.check_action(role, action)
    }
}
impl NormalFormGame for PdWithAggregates {
    fn payoff(&self, role: usize, profile: &[usize]) -> f64 { self.pd.payoff(role, profile) }
}
impl FiniteGame for PdWithAggregates {
    fn n_actions(&self, role: usize) -> usize { self.pd.n_actions(role) }
    fn action_label(&self, role: usize, action: usize) -> Cow<'_, str> {
        self.pd.action_label(role, action)
    }
}
impl AggregateGame for PdWithAggregates {
    fn aggregate_spec(&self) -> &[AggregateSpec] { &self.specs }
    fn aggregate_into(&self, profile: &[usize], out: &mut [f64]) {
        out[0] = f64::from(u8::from(profile.iter().all(|&a| a == 0)));   // mutual_coop
        out[1] = profile.iter().filter(|&&a| a == 1).count() as f64;    // defections
    }
}

/// Cooperates while more than half of last step's encounters were mutual cooperation.
struct FollowTheCrowd;

impl DecisionRule<PdWithAggregates, ()> for FollowTheCrowd {
    fn name(&self) -> &str { "follow_the_crowd" }
    fn init_state(&self, _: &RuleInitContext, _: &PdWithAggregates)
        -> Result<BuiltinPolicyState, RuleError> {
        Ok(BuiltinPolicyState::Stateless(Stateless))
    }
    fn decide(&self, _: &mut BuiltinPolicyState,
              ctx: &DecisionContext<'_, PdWithAggregates, ()>,
              _: &mut SimRng) -> Result<usize, RuleError> {
        let last = ctx.aggregate_history()                 // None without aggregates
            .and_then(|history| history.last())            // previous step only
            .and_then(|(_, values)| values[0]);            // mutual_coop
        Ok(match last {
            Some(share) if share <= 0.5 => 1,
            _ => 0,                                        // first step: cooperate
        })
    }
}

fn main() {
    let pd = MatrixGame::bimatrix(
        &[vec![3.0, 0.0], vec![5.0, 1.0]],
        &[vec![3.0, 5.0], vec![0.0, 1.0]],
    )
    .unwrap()
    .with_labels(vec![vec!["C".into(), "D".into()]; 2])
    .unwrap();
    let game = PdWithAggregates {
        pd,
        specs: vec![
            AggregateSpec::new("mutual_coop", AggregateReduce::Mean).unwrap(),
            AggregateSpec::new("defections", AggregateReduce::Sum).unwrap(),
        ],
    };
    let metrics = GameMetrics::new(&game, MetricsOptions::new()).unwrap();

    let mut rules: RuleBook<PdWithAggregates, ()> = RuleBook::new();
    rules.register("follow_the_crowd", Arc::new(FollowTheCrowd)).unwrap();
    rules.register("fixed_defect", Arc::new(Fixed::new(1))).unwrap();

    let seed = 5;
    let world = build_world(
        game, 4, (), InitialActions::Same(0), &rules,
        &Assignment::Ordered(vec![
            AssignGroup { kind: AssignKind::Rule("follow_the_crowd".into()), count: 3 },
            AssignGroup { kind: AssignKind::Rule("fixed_defect".into()), count: 1 },
        ]),
        10, 3, seed,
    )
    .unwrap();

    let mut sim = assemble_with_aggregates(
        world, FixedPlayers::pairs(&[(0, 1), (2, 3)]), rules, NormalFormOutcome,
        PlayOptions::new(), seed,
        8,                                                 // aggregate_history_len
    )
    .unwrap()
    .add_mechanism(Box::new(metrics))
    .recorder(Box::new(InMemoryRecorder::new()))
    .build();
    sim.run().unwrap();

    let recorder = sim.recorder().as_any().unwrap()
        .downcast_ref::<InMemoryRecorder>().unwrap();
    for row in recorder.metrics().iter().filter(|row| row.key.starts_with("game.aggregate.")) {
        println!("t={} {} = {}", row.t, row.key, row.value);
    }
}
```

```text
t=1 game.aggregate.mutual_coop = 0.5
t=1 game.aggregate.defections = 1
t=2 game.aggregate.mutual_coop = 0
t=2 game.aggregate.defections = 4
t=3 game.aggregate.mutual_coop = 0
t=3 game.aggregate.defections = 4
```

At `t = 1` the pair `(0, 1)` cooperates mutually and the pair `(2, 3)` does not,
so `mutual_coop` is `0.5`; at `t = 2` the three followers read that row, find it
not above one half, and defect.

### Convergence stop

`MetricsOptions::stop(tol, window)` ends the run once the played shares have
stopped moving. After each step that has at least one seat, `game_metrics`
compares the step's `played_share` values with those of the **previous step that
had seats**:

- on the first such step there is nothing to compare with, and the counter is `0`;
- otherwise, if $\max_k \lvert s_k(t) - s_k(\text{prev})\rvert < \texttt{tol}$
  the counter grows by one, else it is reset to `0`;
- a step without seats resets the counter to `0` and keeps the previous shares.

When the counter reaches `window`, `game_metrics` records the step's row and
then requests a stop (`StepContext::request_stop`), and the run ends after that
step. The earliest possible stop is therefore at step `window + 1` — in the
first example `stop(1e-9, 3)` ends the run at `t = 5`. The counter and the
previous shares are kept in the world, so a resumed run stops at the same step.

### Feature `metrics`: action entropy

With the Cargo feature `metrics`, `game_metrics` also writes
`game.action_entropy`: the Shannon entropy in nats of the pooled `played_share`
values, computed with `socsim_metrics::stats::shannon_entropy`. It is on by
default under the feature (`MetricsOptions::action_entropy(false)` turns it off)
and appears only for games with pooled ids — the entropy of per-role shares
would mix two distributions. The feature adds nothing else.

### Describing the columns: `describe`

`GameMetrics::describe()` returns `(pattern, meaning)` pairs for the columns
this configuration can write: `game.played_share.{action}`, `game.mean_payoff`,
`game.payoff_sd`, then `game.coop_rate`, `game.regret`, `game.eq_distance` and
`game.action_entropy` when enabled, and `game.aggregate.{name}` (always listed,
since it depends on how the simulation is assembled). A repository that records
runs with [runvault](https://github.com/akitenkrad/rs-runvault) can copy them
into its `runvault.toml`: a pattern with a `{…}` segment is a
`[[metric_patterns]]` entry, a plain name a `[metrics."<name>"]` entry. The
example program in [Action ids](#action-ids-metric_action_ids) prints exactly
that:

```toml
[[metric_patterns]]
pattern = "game.played_share.{action}"
meaning = "Share of all played seats choosing `{action}`. `{action}` is a metric_id; display labels come from the game's metric actions. Actions are pooled across roles."

[metrics."game.mean_payoff"]
meaning = "Mean aggregated payoff among agents who played at least once in the step."

[metrics."game.payoff_sd"]
meaning = "Population standard deviation of aggregated payoff among agents who played at least once in the step."

[metrics."game.coop_rate"]
meaning = "Share of all played seats choosing the configured cooperation label for that seat's role."

[metrics."game.regret"]
meaning = "Maximum unilateral-deviation regret of the configured mixed-strategy profile."

[[metric_patterns]]
pattern = "game.aggregate.{name}"
meaning = "Public game aggregate for the completed step; present only when the simulation was assembled with aggregates."
```

Add `unit`, `direction` and the pattern's `axes` by hand if you want them. The
crate itself does not depend on runvault.

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
learn in sorted `AgentId` order and decide in the scheduler's activation order
(and, within one agent, in decision-unit order); the built-in learning rules
draw only from the engine stream passed to `decide` (see
[Random numbers](#random-numbers)). With the same seed and the same code a run
is reproducible step for step. `game_metrics` and the mixed-strategy reports draw
no randomness and leave the policy states, held actions and plays untouched, so
adding them does not change a run's trajectory; the aggregate table draws no
randomness either.

---

## Solving finite games

Independently of the simulation, the crate computes equilibria and related
solution concepts of any `FiniteGame` — a `MatrixGame`, a
[built-in game](#built-in-games-catalog) or your own type. Mixed strategies
live in `socsim_games::mixed`, the solvers in `socsim_games::solve`. Both are in
the default build: no feature flag and no `socsim-*` crate are needed (the
linear programs are solved with the pure-Rust `microlp` crate).

### Mixed strategies

A `MixedStrategy` is a validated probability vector over one role's actions,
in action-index order:

| Constructor | Result |
|---|---|
| `MixedStrategy::new(probs)` | the vector as given (it is not renormalised); `MixedError::NotADistribution` if it is empty, has a NaN, infinite or negative entry, or its sum differs from 1 by more than `1e-9` |
| `MixedStrategy::uniform(n)` | `1/n` on each of `n` actions |
| `MixedStrategy::pure(n, action)` | probability 1 on `action` |

A mixed profile is a slice with one `MixedStrategy` per role. These functions
evaluate it for any number of roles, enumerating every action profile:

| Function | Returns |
|---|---|
| `mixed::expected_payoff(&game, role, &profile)` | the role's expected payoff `F_i(σ)` |
| `mixed::action_payoffs(&game, role, &profile)` | `F_i(s_i, σ_-i)` for every pure action `s_i` of the role |
| `mixed::regrets(&game, &profile)` | per role: best pure-action payoff minus expected payoff |
| `mixed::regret(&game, &profile)` | the maximum of `regrets` |

A profile with the wrong number of roles or a vector of the wrong length is a
`MixedError::{RoleCount, Dimension}`.

Mixed strategies are a solver-side concept only. In a simulation a decision
rule always returns one `G::Action` — an action index for a `FiniteGame` — and
a `MixedStrategy` is never an action. A rule that is meant to play a mix draws
an action from it with its own RNG. A rule can, however, *report* the mix it would
play next (see [Reporting mixed strategies](#reporting-mixed-strategies)).

### Tolerance and verification

All comparisons in the solvers use one tolerance per game, scaled by its
payoffs:

```text
tol = 1e-10 × max(1, max payoff − min payoff)    (over all roles and all profiles)
```

`solve::tolerance(&game)` returns it. A solver does not trust its method on its
own: every solver that returns an equilibrium recomputes the regret of the
profile it found against the original game, attaches it (the `regret` field),
and returns the result only when that regret is at most `tol`. Before solving,
every solver checks the game: at least one role, no role without actions, at
most 10⁶ action profiles (`SolveError::TooLarge`) and only finite payoffs
(`SolveError::NonFinitePayoff`).

Mixed equilibria are returned as `Equilibrium { strategies, payoffs, regret }`:
one `MixedStrategy` and one expected payoff per role, plus the verified maximum
regret. `is_pure()` is true when every strategy puts probability 1 on a single
action.

### Solvers at a glance

| Function | Players | Returns | Limits |
|---|---|---|---|
| `pure_nash(&game)` | n | `Vec<PureNash>`: every pure equilibrium | enumerates all profiles (≤ 10⁶) |
| `iterated_strict_dominance(&game)` | n | `DominanceResult`: surviving actions and the elimination log | pure strict dominance only |
| `support_enumeration(&game)` | 2 | `EquilibriumSet`: all equilibria of a nondegenerate game, plus `findings` | ≤ 2²⁰ support pairs (up to 10 × 10) |
| `lemke_howson(&game, &options)` | 2 | `LemkeHowsonResult`: one equilibrium | which one depends on the dropped label |
| `zero_sum_minimax(&game)` | 2, zero-sum | `MinimaxSolution`: value and optimal strategies | LP; primal and dual both solved and checked |
| `maximin_security_level(&game, role)` | 2 | `SecurityLevel`: one role's maximin guarantee | LP; primal and dual both solved and checked |
| `logit_qre(&game, &options)` | n | `QreSolution`: one logit QRE at the given λ | local fixed-point iteration, not branch tracking |

Errors are `SolveError`: `UnsupportedRoles` for a role count a solver does not
accept, `NotZeroSum`, `InvalidLabel`, `MaxPivots`, `Lp(LpFailure)`,
`VerificationFailed { duality_gap, regret }`, `NotConverged { residual,
iterations }`, `InvalidOptions` and `Mixed(MixedError)`, besides the game
checks above.

### `pure_nash`

Visits every action profile in lexicographic order (role 0's action is the
most significant index, the last role's varies fastest) and keeps each profile
in which no role gains more than `tol` by a unilateral deviation. Returns
`PureNash { profile, payoffs, regret }` in that order. Because the enumeration
is exhaustive, the list is every pure equilibrium of the game (up to `tol`); the
cost is the number of profiles times the total number of actions, and games
above 10⁶ profiles are rejected.

### `iterated_strict_dominance`

In each round, for every role at once, removes every surviving action that is
strictly dominated by another surviving **pure** action of the same role —
worse by more than `tol` against every surviving combination of the other
roles' actions — and repeats until a round removes nothing. Returns
`DominanceResult { surviving, eliminated }`: per role the surviving action
indices in ascending order, and one `Elimination { round, role, action }` per
removed action (rounds start at 1).

Elimination by strict dominance never removes a Nash equilibrium, and its end
result does not depend on the order of removal. Two common variants are left
out on purpose:

- **Weak dominance** is not used: its result can depend on the order of
  removal, and it can remove equilibria.
- **Domination by a mixed strategy** is not checked: an action that is
  dominated only by a mix of other actions survives here. Detecting it needs a
  linear program per action. The surviving set can therefore be larger than
  under the textbook definition that allows mixed dominators.

### `support_enumeration`

Two players only. For every pair of non-empty supports (role 0's as the outer
key, role 1's as the inner, each in lexicographic order of ascending action
indices) it solves the indifference equations, rejects the support when a
probability is negative or an action outside the support does better, and
verifies the regret of the candidate. Returns `EquilibriumSet { equilibria,
findings }`; an equilibrium found again within `tol` keeps its first
occurrence.

**When it finds all equilibria.** In a nondegenerate game every equilibrium is
the unique solution of the equations on its support, so every equilibrium is
found (and their number is odd). In a degenerate game a support's system can be
rank-deficient or ill-conditioned; such a support is not guessed at but
reported in `findings`:

| `SupportFinding` | Meaning |
|---|---|
| `Degenerate { support, reason }` | rank-deficient system with no verified point |
| `Component { support, base, directions }` | a verified point `base` on a positive-dimensional set of equilibria, with null-space `directions` (each flattened as role 0's probabilities followed by role 1's); `base` is also added to `equilibria` |
| `Inconclusive { support, reason }` | the system is ill-conditioned (condition estimate above 10¹²) or non-finite, or the final verification failed |

No point is ever reported as an equilibrium without passing the regret check.
`is_complete()` is true when `findings` is empty: every support was decided,
and for a nondegenerate game `equilibria` is then the full set.

**Limits.** Exactly two roles (`UnsupportedRoles` otherwise). The number of
support pairs `(2^m − 1)(2^n − 1)` must be at most 2²⁰, so 10 × 10 is accepted
and 11 × 10 is not (`TooLarge`, whose `profiles` field then carries the
support-pair count). The arithmetic is floating point — there is no exact
rational arithmetic — so a nearly degenerate game is decided by `tol`.

### `lemke_howson`

Two players only. Follows one complementary-pivoting path and returns exactly
one equilibrium, `LemkeHowsonResult { equilibrium, pivots }`.

| `LemkeHowsonOptions` | Default | Meaning |
|---|---|---|
| `dropped_label` | `0` | the label dropped at the start; labels `0..m` are role 0's actions, `m..m+n` role 1's, for an `m × n` game |
| `max_pivots` | `100_000` | stop with `SolveError::MaxPivots { limit }` after this many pivots |

Set them with `LemkeHowsonOptions::default().with_dropped_label(k).with_max_pivots(n)`;
a label `≥ m + n` is `InvalidLabel`. Before pivoting, each player's payoffs are
shifted by a constant so that every entry is strictly positive (this does not
change the equilibria); the result is verified against the original, unshifted
game. Ties in the minimum-ratio test, which occur in degenerate games, are
broken by a lexicographic rule on the current inverse basis; if even that rule
ties, the solver stops with `Lp(LpFailure::Internal)`.

**What to expect.** In a nondegenerate game every dropped label leads to an
equilibrium. Which one depends on the label, and some equilibria are not
reached from any label: the method may return a pure equilibrium even when a
mixed one exists (in the Battle of the Sexes below, every label ends at a pure
equilibrium). To list all equilibria, use `support_enumeration`.

### `zero_sum_minimax` and `maximin_security_level`

Both solve a pair of linear programs with `microlp`: the primal (the
maximising role's mixed strategy and the payoff it can guarantee) and the dual
(the opponent's mixed strategy and the payoff it can hold the first role to).

`zero_sum_minimax(&game)` requires two roles and a zero-sum game
(`|u_0 + u_1| ≤ tol` at every profile; otherwise `NotZeroSum`). By the minimax
theorem every finite two-player zero-sum game has a value, and the primal and
dual optima both equal it. Before returning, the solver checks that the duality
gap `|primal − dual|`, the regret of the strategy pair, the gap between role
0's expected payoff and the value, and the gap between each role's best
pure-action payoff and the value (`+value` for role 0, `-value` for role 1) are
all at most `tol`; otherwise it returns `VerificationFailed { duality_gap,
regret }`. It returns `MinimaxSolution { value, row, col, duality_gap, regret }`,
with `value` from role 0's point of view. When the optimal strategies are not
unique, which optimal pair is returned is up to the LP solver.

`maximin_security_level(&game, role)` accepts any two-player game, zero-sum or
not. It returns `SecurityLevel { level, strategy, duality_gap }`: the largest
payoff `level` that `role` can guarantee against every pure action of the
opponent, and a mixed strategy achieving it. The solver checks the duality gap
and that the strategy actually guarantees `level` against every opponent action
(within `tol`). A role other than 0 or 1 is `InvalidOptions`. In a general-sum
game this is a guarantee, not an equilibrium strategy.

If the LP backend reports the problem infeasible or unbounded, is interrupted,
stops at a feasible but not optimal point, or fails internally, the solver
returns `SolveError::Lp(LpFailure::{Infeasible, Unbounded, Interrupted,
NotOptimal, Internal})` instead of a result.

### `logit_qre`

Finds a logit quantal response equilibrium at one precision λ for any number of
players: a profile in which each role's strategy is the softmax of
`λ × action_payoffs` against the others. Starting from `init` (or the uniform
profile), it repeats the damped update

```text
σ_i ← (1 − damping) σ_i + damping × softmax(λ F_i(·, σ_-i))
```

until the fixed-point residual (the largest absolute difference between `σ`
and its logit response) is at most `tol`.

| `QreOptions` | Default | Meaning |
|---|---|---|
| `lambda` | (argument of `QreOptions::new(lambda)`) | precision; finite and `≥ 0`; `0` gives the uniform profile whatever `init` is |
| `init` | `None` (uniform) | starting profile, set with `.with_init(profile)` |
| `damping` | `0.1` | in `(0, 1]`, set with `.with_damping(d)` |
| `tol` | `1e-12` | finite and `> 0`, set with `.with_tol(t)` |
| `max_iter` | `100_000` | positive, set with `.with_max_iter(n)` |

Invalid options are `InvalidOptions`; an `init` of the wrong shape is a
`MixedError`. The residual is recomputed on the profile that is returned, and
`QreSolution { lambda, strategies, residual, regrets, iterations }` is returned
only when it is at most `tol`; otherwise the result is `NotConverged { residual,
iterations }`. There is no fallback to the last iterate.

**This is a local search, not principal-branch tracking.** A game can have
several logit QREs at the same λ, and the iteration converges to whichever
fixed point it reaches from `init`; different starting profiles can land on
different QRE branches (see the stag hunt below). For a large λ the damped
iteration may fail to settle; lower the damping, or raise λ in steps and pass
each solution as the `init` of the next. That usually follows one branch, but
it is not guaranteed to pass through folds or bifurcations of the QRE
correspondence.

A logit QRE is not a Nash equilibrium at finite λ. `regrets` holds the
ordinary (unregularised) regret of each role at the returned profile; at an
exact logit QRE it is at most `ln |S_i| / λ`, where `|S_i|` is the role's
number of actions, so it shrinks as λ grows. The solver reports this regret but
does not use it as an acceptance test.

### Examples

**Battle of the Sexes: all equilibria vs. one path.** Support enumeration finds
the two pure equilibria and the mixed one; Lemke–Howson reaches only the pure
ones, whichever label is dropped:

```rust
use socsim_games::catalog::BattleOfSexes;
use socsim_games::solve::{
    lemke_howson, pure_nash, support_enumeration, tolerance, LemkeHowsonOptions,
};

fn main() {
    // Role 0 prefers (B, B) with 2, role 1 prefers (V, V) with 2;
    // miscoordination gives both -1.
    let game = BattleOfSexes::default();
    println!("tol = {:e}", tolerance(&game).unwrap());

    let pure: Vec<_> = pure_nash(&game).unwrap().into_iter().map(|e| e.profile).collect();
    println!("pure: {pure:?}");

    let all = support_enumeration(&game).unwrap();
    println!("complete: {}", all.is_complete());
    for eq in &all.equilibria {
        println!(
            "  {:.4?} {:.4?} payoffs {:.4?}",
            eq.strategies[0].probs(),
            eq.strategies[1].probs(),
            eq.payoffs
        );
    }

    // Labels 0..2 are role 0's actions, 2..4 are role 1's.
    for label in 0..4 {
        let found = lemke_howson(&game, &LemkeHowsonOptions::default().with_dropped_label(label))
            .unwrap();
        println!(
            "  label {label}: {:?} {:?} ({} pivots)",
            found.equilibrium.strategies[0].probs(),
            found.equilibrium.strategies[1].probs(),
            found.pivots
        );
    }
}
```

```text
tol = 3e-10
pure: [[0, 0], [1, 1]]
complete: true
  [1.0000, 0.0000] [1.0000, 0.0000] payoffs [2.0000, 1.0000]
  [0.6000, 0.4000] [0.4000, 0.6000] payoffs [0.2000, 0.2000]
  [0.0000, 1.0000] [0.0000, 1.0000] payoffs [1.0000, 2.0000]
  label 0: [1.0, 0.0] [1.0, 0.0] (2 pivots)
  label 1: [0.0, 1.0] [0.0, 1.0] (2 pivots)
  label 2: [1.0, 0.0] [1.0, 0.0] (2 pivots)
  label 3: [0.0, 1.0] [0.0, 1.0] (2 pivots)
```

The payoff range is 2 − (−1) = 3, hence `tol = 3e-10`. In the mixed
equilibrium each role plays its preferred action with probability 0.6 and
expects 0.2.

**Rock–Paper–Scissors: minimax.** The value is 0 and both optimal strategies
are uniform. The Battle of the Sexes is not zero-sum, but each role still has a
security level:

```rust
use socsim_games::catalog::{BattleOfSexes, RockPaperScissors};
use socsim_games::solve::{maximin_security_level, zero_sum_minimax};

fn main() {
    let rps = RockPaperScissors::default();
    let solution = zero_sum_minimax(&rps).unwrap();
    println!("value {:.6}", solution.value);
    println!("row {:.6?}", solution.row.probs());
    println!("col {:.6?}", solution.col.probs());
    println!("gap {:e}, regret {:e}", solution.duality_gap, solution.regret);

    // A general-sum game has no minimax value, but each role still has a
    // maximin guarantee.
    let bos = BattleOfSexes::default();
    assert!(zero_sum_minimax(&bos).is_err()); // NotZeroSum
    let guard = maximin_security_level(&bos, 0).unwrap();
    println!("BoS role 0 guarantees {:.6} with {:.6?}", guard.level, guard.strategy.probs());
}
```

```text
value 0.000000
row [0.333333, 0.333333, 0.333333]
col [0.333333, 0.333333, 0.333333]
gap 0e0, regret 5.551115123125783e-17
BoS role 0 guarantees 0.200000 with [0.400000, 0.600000]
```

The exact low-order digits of `regret` come from floating-point rounding; what
the solver guarantees is `regret ≤ tol`.

**Stag hunt: QRE branches.** Raising λ from the uniform start follows the
branch towards `(hare, hare)`; at λ = 5 a start at `(stag, stag)` lands on a
different QRE close to `(stag, stag)`:

```rust
use socsim_games::catalog::StagHunt;
use socsim_games::solve::{logit_qre, QreOptions};
use socsim_games::MixedStrategy;

fn main() {
    let game = StagHunt::default();

    // Raise lambda step by step, starting each solve from the previous one.
    let mut previous = None;
    for lambda in [0.5, 1.0, 2.0, 5.0] {
        let mut options = QreOptions::new(lambda);
        if let Some(init) = previous.take() {
            options = options.with_init(init);
        }
        let qre = logit_qre(&game, &options).unwrap();
        println!(
            "lambda {lambda}: P(stag) = {:.4}, regrets {:.4?}",
            qre.strategies[0].probs()[0],
            qre.regrets
        );
        previous = Some(qre.strategies);
    }

    // The same lambda from a different start lands on a different fixed point.
    let stag = MixedStrategy::pure(2, 0).unwrap();
    let qre = logit_qre(&game, &QreOptions::new(5.0).with_init(vec![stag.clone(), stag])).unwrap();
    println!("lambda 5 from (stag, stag): P(stag) = {:.4}", qre.strategies[0].probs()[0]);
}
```

```text
lambda 0.5: P(stag) = 0.0334, regrets [0.2246, 0.2246]
lambda 1: P(stag) = 0.0009, regrets [0.0064, 0.0064]
lambda 2: P(stag) = 0.0000, regrets [0.0000, 0.0000]
lambda 5: P(stag) = 0.0000, regrets [0.0000, 0.0000]
lambda 5 from (stag, stag): P(stag) = 0.9901
```

Each regret stays below `ln 2 / λ` (about 1.39 at λ = 0.5).
