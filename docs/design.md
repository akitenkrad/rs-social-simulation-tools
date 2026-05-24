**English** | [日本語](design.ja.md)

# Design overview

This page is the conceptual entry point to socsim: what it is, the ideas it is
built on, the roles of its core types, and — in detail — how every piece behaves
during one step of the six-phase loop. For the reference-level material (crate
dependency graph, calibration philosophy, scenario schema) see the
[Architecture](architecture.md) page; for the mechanisms that ship with the tool
see the [Mechanism catalog](mechanisms.md).

## 1. What socsim is

`socsim` is a composable, agent-based social-simulation engine. A simulation is
a **shared world** that a stack of **mechanisms** transforms, step by step, in a
fixed six-phase order, with all randomness flowing from a single seeded RNG.

The guiding analogy is a neural network: mechanisms compose like layers, the
world is the tensor they read and write, and the engine is the runtime that runs
them in order. You assemble a model by *stacking mechanisms*, not by editing the
engine.

![socsim at a glance](assets/design-overview.svg)

The result is an **ABM + RL-style tick loop**: discrete time advances one step at
a time, every agent and global effect is expressed as a mechanism slotted into a
phase, and the whole run is deterministic for a given seed.

## 2. Design philosophy

**Composition over modification.** Domain logic lives in mechanisms. Adding a new
behaviour means writing one more `Mechanism` and slotting it into a phase — the
engine, the world trait, and every other mechanism stay untouched. This is what
"composes like neural-net layers" means in practice.

**State and code are separate.** The *state* of a run is the world (which owns
the clock), the RNG stream position, and the stop flag. The *code* is the set of
mechanisms, the scheduler, and the recorder. A [`Snapshot`](#7-state-vs-code-snapshots--determinism)
captures only the state — exactly like a PyTorch `state_dict` versus the model
architecture — so a run can be saved and resumed bit-identically.

**Determinism by construction.** Reproducibility is not a feature you switch on;
it is structural. All randomness derives from one seeded ChaCha20 stream;
agents and aggregates are always iterated in sorted `AgentId` order so
floating-point sums and RNG draws are stable; the clock is passed by value so
reading time never perturbs anything; and the activation order is an explicit
`Scheduler` decision rather than incidental hash-map order.

**A small trait surface.** There are only four traits to implement —
`WorldState`, `Mechanism`, `Scheduler`, `Recorder` — and most projects only ever
write the first two. Everything else is concrete engine machinery.

**Two first-class usage paths.** The same engine and determinism guarantees back
both the scenario-TOML/CLI path (declare mechanisms in a `.toml`, run with the
`socsim` binary) and library mode (build the world in Rust and drive
`Simulation` directly). Neither is a second-class citizen.

**The six-phase loop is a coordination contract.** Mechanisms never call each
other. Instead they agree on *when* their effect happens by declaring a phase.
That fixed ordering — `PreStep → Environment → Decision → Interaction → Reward →
PostStep` — is the only protocol they need to compose predictably.

## 3. The cast of characters

socsim is built from four traits you implement and a handful of concrete types
the engine provides.

![Core abstractions and ownership](assets/design-abstractions.svg)

| Type | Kind | Role | Analogy |
|---|---|---|---|
| `WorldState` | trait (you impl) | The shared, mutable environment: the agent roster, the `SimClock`, and all domain data. | The tensor / model state |
| `Mechanism<W>` | trait (you impl) | One unit of research logic. Declares the phase(s) it runs in and does its work in `apply`. | A network layer |
| `Scheduler<W>` | trait (you impl, or use a built-in) | Decides the agent activation order each step. | The data-loader order |
| `Recorder` | trait (you impl, or use a built-in) | Sink for metrics and events. `NullRecorder` is the no-op default. | A logger / metrics writer |
| `AgentId` | struct | Opaque `Copy` agent id; `Ord` so iteration is deterministic. | A row index |
| `SimClock` | struct | `Copy` discrete-time counter: `t`, `t_max`, `is_done`, `tick`. | The epoch counter |
| `Phase` | enum | The six phases; `Phase::ORDER` is their fixed execution order. | The forward-pass schedule |
| `StepContext<'a, W>` | struct | The bundle of borrows handed to every `apply` call. | A layer's forward `ctx` |
| `Blackboard` | struct | Step-scoped, type-erased scratch for passing transient values between mechanisms. | Per-step activations cache |
| `SimRng` | struct | Seeded ChaCha20 stream; `derive` makes labelled child streams. | The seeded generator |
| `Simulation<W>` | struct | The driver. Owns the world, mechanisms, scheduler, RNG, recorder, scratch, and stop flag. | The training loop / runtime |
| `SimulationBuilder` | struct | Fluent constructor with sane defaults (sequential scheduler, seed 0, `NullRecorder`). | The model builder |
| `Snapshot<W>` | struct | Serialisable capture of mutable state for save/resume. | `state_dict` |

The ownership is simple: a `Simulation` **owns** one of each collaborator, and on
every mechanism call it lends them out — temporarily and safely — through a
`StepContext`. Mechanisms borrow; they never own engine state.

## 4. The six-phase execution model

Every step runs the same six phases in the same order. The phases are a
*semantic* schedule — they say *when* a kind of effect belongs, not *what* the
effect is:

- **PreStep** — setup / bookkeeping before the main phases.
- **Environment** — global, exogenous updates (resource replenishment, shocks).
- **Decision** — agents choose actions (and, in this module, hiring/quitting).
- **Interaction** — agent-to-agent effects (peer effects, network diffusion).
- **Reward** — payoffs computed and aggregated; metrics recorded.
- **PostStep** — cleanup / logging after everyone has acted.

Here is exactly what `Simulation::step()` does:

![Anatomy of one step](assets/design-step-sequence.svg)

1. **Tick the clock** (`t += 1`) *first*, so mechanisms observe the new time.
2. **Copy the clock** into a value snapshot — because `StepContext` hands out a
   `&mut world`, the clock is passed by value so reading `t` needs no second
   borrow of the world.
3. **Clear the scratch** blackboard, so values from the previous step cannot leak
   into this one.
4. **Ask the scheduler** for the activation order once. The same `agent_order` is
   shared by all phases this step, so every mechanism sees a consistent ordering.
5. **Run the nested loop.** The outer loop walks `Phase::ORDER`; the inner loop
   walks the mechanisms in **insertion order** (which equals scenario declaration
   order). A mechanism runs only in a phase it registered via `phases()`; a
   mechanism that registers several phases is invoked once in each. For each such
   call the engine builds a fresh `StepContext` and calls `apply(phase, ctx)`.
6. **Return.** The run loop then decides whether to step again.

Mapped onto the reference HR-lifecycle mechanisms, one step flows like this:

| Phase | Mechanisms that act | What happens |
|---|---|---|
| PreStep | — | (free for bookkeeping; used by the MARL variant) |
| Environment | `learning_curve` | tenure ages; individual productivity is refreshed |
| Decision | `fit`, `turnover`, `hiring` | satisfaction updates; quits resolved; vacancies filled |
| Interaction | `peer_effect`, `ocb`, `toxic_spread` | team effects, knowledge inflow, contagion |
| Reward | `org_performance` | productivity summed; step metrics recorded; team means recomputed |
| PostStep | `knowledge_loss`, `socialization` | departing tacit knowledge drained; new hires onboarded |

Because the order is fixed, mechanisms can rely on hand-offs without ever
referencing one another: `turnover` fills `departed_this_step` in Decision, and
`knowledge_loss` consumes it in PostStep; `org_performance` recomputes each
team's mean ability in Reward so `peer_effect` reads a current value on the next
step. See the [Mechanism catalog](mechanisms.md) for each mechanism's contract.

## 5. Inside a mechanism call: `StepContext`

A mechanism's `apply` is small and focused because everything it can touch
arrives in one bundle:

![Anatomy of StepContext](assets/design-stepcontext.svg)

- `world: &mut W` — read and mutate the shared state.
- `clock: SimClock` — a copy; read the current `t` freely.
- `rng: &mut SimRng` — the *only* sanctioned source of randomness, so runs stay
  reproducible.
- `recorder: &mut dyn Recorder` — emit metrics and events.
- `agent_order: &[AgentId]` — this step's activation order from the scheduler.
- `scratch: &mut Blackboard` — step-scoped, type-erased space to pass transient
  values to a later mechanism in the same step (or out to the driver) without
  polluting `WorldState`.
- `stop: &mut bool` — call `request_stop()` to end the run after the current step
  completes.

This is what keeps mechanisms composable: they are pure functions of "the world
plus this context", with no hidden global state and no direct knowledge of each
other.

## 6. Driving the simulation

`Simulation` exposes a small driving API on top of `step()`:

- `run()` — step until the clock `is_done()` (`t ≥ t_max`) **or** a mechanism
  requested a stop.
- `run_until(predicate)` — also stop when `predicate(&world)` becomes true.
- `run_observed(observe)` — call `observe(StepReport)` after each step to collect
  convergence curves or live metrics without hand-rolling a `step()` + read loop.
- `step()` / `step_reported()` — advance exactly one step when you want to drive
  the loop yourself.

A stop requested mid-step is honoured *after* the step finishes: the remaining
mechanisms in the current step still run, then the loop exits. `StepReport`
bundles the post-step clock time, the stop flag, and shared references to the
world and scratch.

```rust
use socsim_config::{Registry, Params, ModulePack};
use socsim_hr_lifecycle::{HrLifecyclePack, HrWorld};
use socsim_engine::{RandomActivationScheduler, SimulationBuilder};

let mut reg: Registry<HrWorld> = Registry::new();
HrLifecyclePack.register(&mut reg);

let mut builder = SimulationBuilder::new(world)
    .scheduler(Box::new(RandomActivationScheduler))
    .seed(42);
for name in ["learning_curve", "fit", "turnover", "hiring",
             "peer_effect", "ocb", "toxic_spread",
             "org_performance", "knowledge_loss", "socialization"] {
    builder = builder.add_mechanism(reg.build(name, &Params::empty())?);
}
let mut sim = builder.build();
sim.run()?;
```

See the [Library API](library.md) for building a custom world and mechanisms, and
the [CLI reference](cli.md) for driving the same engine from a scenario file.

## 7. State vs code: snapshots & determinism

A `Snapshot<W>` is socsim's save/resume primitive, and it embodies the
state-versus-code split:

![State vs code](assets/design-state-vs-code.svg)

It captures **state only** — the world (which owns the `SimClock`), the exact
`SimRng` stream position, and the stop flag. It deliberately omits the
**code** — mechanisms, scheduler, recorder — because those are supplied again
when you rebuild the `Simulation`. Restore a snapshot into a simulation wired
with the *same* mechanisms and the run continues bit-identically from the saved
step onward; `Snapshot::save` / `load` persist it as version-checked JSON.

This only works because the whole engine is deterministic: one seeded ChaCha20
stream (with labelled `derive` children), sorted-`AgentId` iteration for stable
floating-point order, a value-copied clock, and an explicit scheduler. Same
seed, same code, same trajectory — on any machine.

## 8. Where to go next

| Page | What it adds |
|---|---|
| [Architecture](architecture.md) | Crate dependency graph, calibration philosophy, scenario TOML schema, snapshots, MARL |
| [Mechanism catalog](mechanisms.md) | Every mechanism: theory, sources, diagrams, phase positioning |
| [Library API](library.md) | Implement your own `WorldState` and `Mechanism`; drive the engine as a library |
| [CLI reference](cli.md) | Run, sweep, and summarise scenarios from the command line |
