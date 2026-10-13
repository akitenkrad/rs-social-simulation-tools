**English** | [日本語](packs.ja.md)

# Module pack catalog

A **module pack** is socsim's unit of *a complete model*. Where a
[mechanism](mechanisms.md) is one piece of research logic, a pack bundles
everything needed to run a whole world: a concrete **world type** (the state
every agent shares), the **mechanisms** that move it forward, a **registration**
function that wires those mechanisms into a [`Registry`](library.md) by name, and
a **starter scenario** TOML you can run immediately.

The `socsim` CLI is **world-polymorphic**: a scenario selects a pack by name
(`[simulation] module_pack = "..."`), and the binary dispatches to that pack's
world type without ever naming a concrete world itself. Four packs ship today.

![Module packs overview](assets/packs-overview.svg)

## The four bundled packs

| Pack | World | What it models | Mechanisms | Page |
|---|---|---|---|---|
| [`hr-lifecycle`](packs/hr-lifecycle.md) | `HrWorld` | An employee-lifecycle organisation: hiring, learning, peer effects, satisfaction, turnover cascades, knowledge loss | ten, calibrated against published empirical findings | [→ hr-lifecycle](packs/hr-lifecycle.md) |
| [`opinion-dynamics`](packs/opinion-dynamics.md) | `OpinionWorld` | Bounded-confidence opinion formation on a social network (consensus, clustering, polarisation) | the `socsim-mechanisms` opinion family, reused via capability traits | [→ opinion-dynamics](packs/opinion-dynamics.md) |
| [`organizational-silence`](packs/organizational-silence.md) | `SilenceWorld` | Hierarchical employees on a small-world network: heterogeneous silence motives, fear, psychological safety, supervisor openness, retaliation events, threshold cascades | ten (plus an LLM variant), calibrated against organizational-behaviour findings | [→ organizational-silence](packs/organizational-silence.md) |
| [`normal-form`](packs/normal-form.md) | `GameWorld` (finite or continuous, chosen by `[world.game] kind`) | Normal-form (strategic-form) games: repeated play between rule-driven agents, population dynamics on lattices and networks, continuous-action oligopolies — configured entirely in the scenario | the eight [`socsim-games`](games.md) mechanisms (`game_match`, `game_decide`, `game_play`, four population dynamics, `game_metrics`) | [→ normal-form](packs/normal-form.md) |

List them at any time from the CLI:

```sh
socsim list packs        # pack names
socsim list mechanisms   # mechanisms grouped by pack
```

## How a pack is structured

Every pack is defined by two traits and a starter TOML:

1. **`ModulePack<W>`** ([`socsim-config`](library.md)) — the research-facing
   interface. It has a `pack_name()` and a `register(&mut Registry<W>)` method
   that adds every mechanism constructor by name. This is what library users
   call directly to activate an entire body of work in one line.

2. **`CliPack`** ([`socsim-cli`](cli.md)) — an object-safe, *world-erased*
   wrapper. It exposes `name()`, `starter_toml()`, `mechanism_names()`,
   `run_seeds()`, and `run_sweep()`, all returning world-agnostic result types,
   plus `run_sweep_rows()`, which `socsim sweep` calls. `run_sweep_rows()` has a
   default implementation that wraps `run_sweep()` (each `SweepPoint` becomes a
   `SweepRow` with no effective scenario), so a pack only overrides it when it
   can report the patched scenario of each combination.
   Each `CliPack` owns its concrete world internally and monomorphises the
   generic [`socsim-runner`](architecture.md) functions for it, so the CLI
   binary stays free of any one domain model. It also has `validate()`, which
   checks pack-specific rules; its default accepts everything, and `socsim`
   calls it after the generic scenario check in `validate`, `run` and `sweep`
   (see [CLI reference § validate](cli.md#validate)).

**A pack without a `Registry`.** The `normal-form` pack does not fit the
`ModulePack<W>` shape: it implements no `ModulePack` and registers nothing. Its
`NormalFormPack` reads `[world.game] kind` to choose a finite or a continuous
game, and hands the **whole scenario** to the `socsim-games` trial factory
(`GameTrialFactory`), which builds the world, the population, the matching,
the rules, the population dynamic and the metrics from it — see
[normal-form games § Scenario files](games.md#scenario-files). Its `CliPack`
therefore checks the whole scenario in `validate()` (every key, every
combination, one dry build) and runs trials and checked sweeps through
`socsim_runner::run_trials` / `run_trial_sweep`, so a sweep can patch
`world.` paths and reports each combination's effective scenario.

All bundled packs are gated behind a Cargo feature of the same name
(`pack-hr-lifecycle`, `pack-opinion-dynamics`, `pack-organizational-silence`,
`pack-normal-form`), all on by default, so a downstream binary can compile in only the packs it
needs. The `organizational-silence` pack additionally offers an
`pack-organizational-silence-llm` feature that adds the LLM-driven
`voice_decision` mechanism alongside the rule-based `voice_decision_rule`. See the
[architecture overview](architecture.md#two-usage-paths-scenario-cli-vs-library-mode)
for how the pack layer sits between scenarios and the engine, and the
[T5 — A scenario pack](tutorials/05-scenario-pack.md) tutorial for a hands-on
build of a pack from scratch.

## Two ways to drive a pack

| Path | Entry point | Best for |
|---|---|---|
| **Scenario / CLI** | `module_pack = "..."` in a `.toml`, then `socsim run` | reproducible scenario files, parameter sweeps, no recompile |
| **Library** | `Pack.register(&mut reg)` + `SimulationBuilder` in Rust | custom drivers, embedding socsim in a larger program |

For `normal-form` the library path is `NormalFormPack` (the same entry point
the CLI uses) or the `socsim-games` builders, not `register`.

Each pack page below documents both paths, the world's data model, the
mechanism composition across the [6-phase tick loop](architecture.md#the-6-phase-tick-loop),
the starter scenario, and the metrics it records.

## Pages

- [**hr-lifecycle**](packs/hr-lifecycle.md) — the calibrated employee-lifecycle reference module (`HrWorld`, ten mechanisms).
- [**opinion-dynamics**](packs/opinion-dynamics.md) — bounded-confidence opinion dynamics (`OpinionWorld`).
- [**organizational-silence**](packs/organizational-silence.md) — climate-of-silence emergence on a hierarchical network (`SilenceWorld`, ten rule-based mechanisms + an optional LLM variant).
- [**normal-form**](packs/normal-form.md) — normal-form game simulations built from the scenario alone (`GameWorld`, the eight `socsim-games` mechanisms, finite and continuous games).

## Adding a new pack

The CLI registry is designed so new packs slot in beside the existing four
without touching the run/sweep/validate/list pipeline:

1. Implement a concrete `World` + its mechanisms (or reuse the
   [`socsim-mechanisms`](mechanisms.md) catalog via capability traits, as the
   opinion pack does).
2. Implement `ModulePack<W>` for it (`pack_name` + `register`).
3. Implement a `struct FooCliPack;` that `impl CliPack`, behind a
   `pack-foo` Cargo feature. Override `validate()` if the pack has rules
   that the generic check (mechanism names, scheduler, `t_max`) cannot see.
4. Push it into `packs()` in `crates/socsim-cli/src/packs.rs`.

A pack that builds whole trials from the scenario, like `normal-form`, skips
step 2: it implements `socsim_runner::TrialFactory` (or reuses one) and calls
`run_trials` / `run_trial_sweep` from its `CliPack`.

The [T5 tutorial](tutorials/05-scenario-pack.md) walks through steps 1–2 end to
end.

## See also

- [Mechanism catalog](mechanisms.md) — the individual mechanisms a pack composes.
- [CLI reference](cli.md) — `init`, `run`, `validate`, `list`, `sweep`, `summarize`.
- [Architecture](architecture.md) — crate graph, the 6-phase tick loop, calibration philosophy.
- [Use cases & recipes](usecases.md) — runnable workflows for each bundled pack.
