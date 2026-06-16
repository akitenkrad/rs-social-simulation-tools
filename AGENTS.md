# AGENTS.md — building on socsim

This file orients **AI coding agents** that will build or extend an implementation
(most often an academic-paper **replication**) on top of **socsim**
(`rs-social-simulation-tools`), a composable agent-based social-simulation
platform in Rust. Humans should start from [`README.md`](README.md); agents
should start here.

## Start here (read in this order)

1. **[`docs/ai/capability-map.md`](docs/ai/capability-map.md)** — the curated map:
   the mental model, a *capability → crate* table, a per-crate API digest
   (exact types/traits/functions, import paths, feature flags, git-dep snippets),
   and the invariants you must not violate. Read this first.
2. **[`docs/ai/recipes.md`](docs/ai/recipes.md)** — copy-paste, end-to-end task
   templates (new replication skeleton, custom `Mechanism`, run/sweep, survey
   recode, dataset acquisition, metrics, reproduction harness, LLM decision,
   output/logging).
3. **`cargo doc -p <crate> --open`** — the full, authoritative API for any crate.
4. The prose human docs (`docs/architecture.md`, `docs/library.md`,
   `docs/cli.md`, `docs/packs.md`, `docs/design.md`) for rationale and depth.

The two `docs/ai/*` files are the distilled map; `cargo doc` is the source of
truth for signatures. When they disagree, trust the code.

## 30-second mental model

- A `Simulation` drives ordered, phase-tagged **`Mechanism`** layers that read and
  write a shared **`WorldState`**; a `Scheduler` and a seeded `SimRng` (ChaCha20)
  feed in; a `Recorder` emits metrics and events.
- The **six-phase tick loop** is the fixed per-step order in which mechanisms act:
  `PreStep → Environment → Decision → Interaction → Reward → PostStep`.
- **Two usage modes.** *Library mode* — depend on just the crates you need, write
  your own `main.rs` + `Mechanism`s (most replications). *CLI mode* — author a
  `Scenario` TOML and run the `socsim` binary against a bundled ModulePack.
- **Two-layer determinism.** The core is seed-deterministic and LLM-free; the
  optional `socsim-llm` layer is made cache-pseudo-deterministic on top, with LLM
  calls confined to the `Decision` phase.

## Depending on socsim

Replications consume socsim crates as git dependencies — pull in only what you
need (engine-free LLM-social replications deliberately omit core/engine/net):

```toml
socsim-core = { git = "https://github.com/akitenkrad/rs-social-simulation-tools", branch = "main", package = "socsim-core" }
socsim-llm  = { git = "https://github.com/akitenkrad/rs-social-simulation-tools", branch = "main", package = "socsim-llm", features = ["live"] }
```

## Non-negotiable rules

- **Determinism:** all randomness flows through `SimRng` / `derive_seed` — never
  ad-hoc RNG. Keep LLM nondeterminism inside `socsim-llm`'s cache layer and the
  `Decision` phase.
- **Engine-free boundaries:** `socsim-survey`, `socsim-results`, `socsim-reproduce`,
  `socsim-datasets`, and default-build `socsim-metrics` pull in **no** engine
  crates. Do not add engine dependencies to them.
- **Keep paper-specific code OUT of socsim-\*.** Canonical, reusable building
  blocks belong in socsim; a metric, schema, or anchor whose meaning is
  model-specific stays in the replication crate. (`socsim-metrics` ships only
  canonical statistics; `socsim-reproduce` ships no anchor values; `socsim-datasets`
  never vendors data.)
- **Phase semantics:** put logic in the phase that matches its role; do read-only
  recording in `PostStep`.

## The 18 crates at a glance

Foundation: `socsim-core` (traits, `WorldState`, `Mechanism`, `Phase`, `AgentId`),
`socsim-rng` (`SimRng`, `derive_seed`). Engine spine: `socsim-config`,
`socsim-log`, `socsim-engine`, `socsim-runner` (multi-seed + sweeps),
`socsim-cli` (`socsim` binary). Orthogonal/optional: `socsim-net` (ER/WS/BA
networks), `socsim-grid` (spatial), `socsim-packs` (bundled CLI packs),
`socsim-mechanisms` (opinion/contagion/cultural/group catalog), `socsim-marl`
(learnable policies), `socsim-llm` (LLM agents). Library-only data/IO:
`socsim-results` (run dirs, CSV/JSON), `socsim-metrics` (stats/distribution/
agreement), `socsim-survey` (config-driven recode engine), `socsim-datasets`
(dataset schemas + registry + optional acquisition), `socsim-reproduce`
(paper-anchor PASS/off harness). See `docs/ai/capability-map.md` for the full
digest.

## Keeping these docs current

Adding a crate, a Cargo feature, a mechanism, or a public API surface must update
`docs/ai/capability-map.md` and, where it introduces a new task pattern,
`docs/ai/recipes.md`. This extends the repo's existing practice that every new
mechanism ships with matching docs.
