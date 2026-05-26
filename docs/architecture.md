**English** | [日本語](architecture.ja.md)

# Architecture

---

## Crate workspace

The workspace contains fourteen crates organised in three layers:

![Crate dependency graph](assets/arch-crates.svg)

```
socsim-cli          ← binary (entry point)
    └── socsim-runner      ← multi-seed runs, sweeps, summarise
            ├── socsim-engine      ← Simulation, SimulationBuilder, schedulers
            │       └── socsim-log         ← InMemoryRecorder, JsonlRecorder, CsvRecorder
            ├── socsim-config      ← Params, Registry, ModulePack, Scenario loader
            │       └── socsim-core        ← traits (Mechanism, WorldState, …), AgentId, Phase, Blackboard
            ├── socsim-packs        ← bundled CLI packs: hr-lifecycle (10 mechanisms) + opinion-dynamics world; CliPacks behind `pack-hr-lifecycle` / `pack-opinion-dynamics` (both default on)
            │       ├── socsim-net         ← SocialNetwork (ER, WS, BA generators)
            │       └── socsim-mechanisms  ← opinion-dynamics mechanisms (HK, Deffuant, …) used by the opinion pack
            ├── socsim-grid        ← Grid, GridIndex, neighbourhoods, distances (spatial models)
            ├── socsim-marl        ← learnable (MARL) policies: Policy, PolicyMechanism, MarlTrainer (burn; library-only)
            └── socsim-rng         ← SimRng (ChaCha20), derive_seed

socsim-mechanisms ← general social-dynamics crate: HegselmannKrauseMechanism, DeffuantMechanism, SocialJudgementMechanism, LorenzMechanism, SiContagionMechanism, ThresholdContagionMechanism, AxelrodMechanism, GroupConformityMechanism, MeanOperator (→ socsim-core only; library-only)
socsim-llm      ← optional LLM-agent layer: LlmClient, CachingClient, build_live_client (no socsim deps; feature-gated; library-only)
socsim-results  ← leaf output helpers: timestamp, create_run_dir, write_csv/json, refresh_latest_symlink (no socsim deps; library-only)
```

Dependency rules:

- `socsim-core` and `socsim-rng` have **no internal dependencies** — they are the foundation.
- `socsim-config` depends on `socsim-core` but **not** on `socsim-engine` (avoiding a cycle).
- `socsim-engine` depends on `socsim-core`, `socsim-log`, and `socsim-config`.
- `socsim-runner` depends on all of the above and adds `rayon` for parallelism.
- `socsim-cli` wires everything together into the `socsim` binary. It is **world-polymorphic**: command handlers operate through an object-safe, world-erased `CliPack` trait (`name` / `starter_toml` / `mechanism_names` / `run_seeds` / `run_sweep`, all returning the world-agnostic `RunResult` / `SweepPoint`), and each registered pack monomorphizes the generic `socsim-runner` functions for its own world type internally. The binary therefore names **no** concrete world type, and packs are looked up by name via a registry. The bundled worlds now live in **`socsim-packs`** — the crate that bundles the hr-lifecycle and opinion-dynamics packs (each a `CliPack` gated behind `pack-hr-lifecycle` / `pack-opinion-dynamics`, an `optional` dependency) — not in the CLI itself; additional packs slot in beside them without touching the run/sweep/validate/list pipeline.
- `socsim-packs`, `socsim-net`, and `socsim-grid` sit beside the engine layer and are orthogonal to it; `socsim-grid` depends only on `socsim-core`. `socsim-packs` depends on `socsim-net` (HR/opinion networks) and `socsim-mechanisms` (the opinion-dynamics mechanisms).
- `socsim-marl` (Phase 6) depends on `socsim-engine` and `socsim-core`. It is **library-only** — not part of the `socsim` binary — and pulls in the `burn` neural-network framework, so the `socsim-packs` hr-lifecycle integration gates it behind a `marl` feature.
- `socsim-llm` is an **orthogonal, optional** layer beside the engine. It has **no `socsim-*` dependencies** (only `serde`/`serde_json`/`thiserror`, plus `ureq` behind features) and is **library-only**. Its live provider backends are feature-gated (`ollama`, `openai`, and `live` = both); the default build pulls in no networking. It is used by the `Decision` phase of LLM-driven models.
- `socsim-results` is a **leaf crate** with **no `socsim-*` dependencies** (only `std` plus `serde`/`serde_json`/`csv`/`chrono`). It provides the output boilerplate for the lightweight library mode and never drags in `socsim-log`/`-config`/`-runner`.
- `socsim-mechanisms` is an **orthogonal, optional** crate beside the engine. It depends on **`socsim-core` only** (for the `ScalarOpinions` / `BinaryState` / `CultureVectors` / `Neighbors` capability traits) and is **library-only** — no `ModulePack`, not wired into the `socsim` binary. It is the **general mechanism catalog**: reusable, domain-agnostic building blocks organised into four Cargo **feature families** (all on by default — `opinion-dynamics`, `contagion`, `cultural`, `group-dynamics`), eight mechanisms in total: opinion dynamics (the bounded-confidence `HegselmannKrauseMechanism` and `DeffuantMechanism`, the `SocialJudgementMechanism`, and the `LorenzMechanism`, with the A/G/H/P/R `MeanOperator` family), network contagion (`SiContagionMechanism`, `ThresholdContagionMechanism`), cultural dissemination (`AxelrodMechanism`), and group dynamics (`GroupConformityMechanism`) — distinct from the scenario-specific packs bundled in the `socsim-packs` crate (which depends on it for its opinion-dynamics pack).

---

## The 6-phase tick loop

Each discrete time step executes six phases in a fixed order defined by `Phase::ORDER`:

```
PreStep → Environment → Decision → Interaction → Reward → PostStep
```

The engine's `Simulation::step` method:

1. Ticks the clock (`t += 1`).
2. Asks the `Scheduler` for the agent activation order.
3. For each phase in `Phase::ORDER`, invokes every mechanism that registered that phase, in insertion order.

The activation order computed in step 2 is passed to all phases as `StepContext::agent_order`, ensuring that mechanisms in the same step see the same ordering.

A mechanism registers its phases by returning a `'static` slice from `Mechanism::phases`. It will be called once per registered phase per step. The typical assignment of phases by the HR lifecycle mechanisms is:

| Mechanism | Phase |
|---|---|
| `learning_curve` | Environment |
| `peer_effect` | Interaction |
| `ocb` | Interaction |
| `fit` | Decision |
| `turnover` | Decision |
| `hiring` | Decision |
| `knowledge_loss` | PostStep |
| `socialization` | PostStep |
| `toxic_spread` | Interaction |
| `org_performance` | Reward |

### Event-driven / sub-tick models

The fixed tick loop does **not** restrict socsim to one-action-per-agent-per-tick models. Event-driven, sub-tick dynamics (Gillespie-style reactions, voter models, contact-process contagion) are supported via a simple idiom: **batch many micro-events inside a single `Mechanism::apply` and map those events onto one tick.** One `apply()` call performs `events_per_step` random single-cell/agent updates (all drawn from `ctx.rng`), so the engine tick becomes the observation/checkpoint cadence while the per-event update semantics are preserved. A mechanism can call `ctx.request_stop()` when the model reaches an absorbing state. See `crates/socsim-engine/examples/cellular_automata.rs` for a worked lattice voter model.

---

## Two usage paths: scenario-CLI vs. library mode

socsim is usable two ways, and **both are first-class**:

![Two usage paths: scenario-CLI vs. library mode](assets/arch-usage-paths.svg)

- **Scenario-TOML / CLI path** — `ModulePack` → `Registry` → scenario `.toml` → `socsim-runner` → `socsim` binary. Best for new projects, reproducible scenario files, and parameter sweeps.
- **Library mode** — depend on just `socsim-core` / `socsim-engine` (and optionally `socsim-grid`), build the world yourself, add mechanisms directly to `SimulationBuilder`, drive it with `run` / `run_until` / `run_observed`, and bring your own recorder (or none — the default is `NullRecorder`, so the engine forces no `socsim-log` dependency). Best for embedding the engine in an existing tool, custom output schemas, and self-contained lattice/CA models.

The two paths share the same engine and determinism guarantees; choose per project rather than per platform. See the [library guide](library.md#lightweight-engine-only-usage-no-toml--runner) for the trade-off table.

---

## Deterministic RNG

`socsim-rng` wraps `rand_chacha::ChaCha20Rng` to provide reproducible streams. The key API:

- `SimRng::from_seed(seed: u64)` — create the root RNG.
- `SimRng::derive(&[u64])` — derive a child RNG from a label (agent ID, phase index, etc.) without mutating the parent. Uses a FNV-1a–style hash mix.

The engine seeds the root RNG from the scenario's `seed` field. The same seed always produces the same agent trajectories, regardless of machine architecture or Rust version.

Agents and team aggregates are always iterated in sorted `AgentId` order to eliminate hash-map iteration non-determinism.

---

## Snapshots: save & resume

A simulation's **mutable state** can be captured and restored — the analogue of a PyTorch `state_dict` (state) versus model architecture (code). `Snapshot<W>` holds the world (which owns the `SimClock`), the exact `SimRng` stream position (serialised via `rand_chacha`'s `serde1`), and the early-stop flag. It deliberately omits mechanisms, the scheduler, and the recorder: those are *code*, supplied when the simulation is rebuilt.

- `Simulation::snapshot()` / `restore(snapshot)` — in-memory capture/restore (`snapshot()` requires `W: Clone`).
- `Snapshot::save(path)` / `Snapshot::load(path)` — JSON persistence, version-checked via `SNAPSHOT_VERSION`.

Restoring a snapshot into a simulation wired with the **same** mechanisms reproduces the run bit-identically from the saved step onward — verified by tests that resume into a *different-seed* simulation and match an uninterrupted run. The bound is opt-in (`impl` blocks gated on `W: Serialize` / `DeserializeOwned`), so the `WorldState` trait is unchanged and non-serde worlds simply lack these methods. `SocialNetwork` serialises as a `{nodes, edges}` pair (petgraph `NodeIndex`es are rebuilt, not persisted), keeping snapshots stable across petgraph versions.

---

## Learnable policies (MARL, Phase 6)

`socsim-marl` makes the `Decision` phase learnable: a `PolicyMechanism` wraps a `Policy` (implemented by `DiscretePolicyNet`, a small `burn` MLP trained with REINFORCE) and slots into the same six-phase loop as any other mechanism — the engine needs no changes. `ObsEncoder`/`ActionApplier`/`RewardFn` bridge a concrete world to the flat feature/action space, a `TrajectoryBuffer` collects episodes, and `MarlTrainer` runs the outer learn loop. Weights are seeded from `SimRng` and all tensor math runs on CPU, so a frozen policy stays bit-reproducible. See the [library guide](library.md#learnable-policies-marl) for usage.

---

## LLM layer (socsim-llm)

`socsim-llm` is the optional layer for LLM-driven agents. The socsim core is **deterministic and LLM-free**, so this crate confines all model non-determinism to one place and *pseudo-determinises* it — a deliberate **two-layer determinism** design: the socsim core is seed-deterministic, and the LLM layer is made *cache-pseudo-deterministic* on top. By convention LLM calls are confined to the `Decision` phase of a mechanism (an LLM call is just a synchronous `complete` inline in `Mechanism::apply`).

![LLM layer: two-layer determinism](assets/arch-llm-layer.svg)

Everything is built on one provider-agnostic trait:

```rust,ignore
pub trait LlmClient {
    fn model(&self) -> &str;
    fn endpoint(&self) -> &str;
    fn complete(&self, prompt: &str, config: &LlmConfig) -> Result<LlmResponse, LlmError>;
}
```

The production stack is assembled in one call behind the `live` feature:

```rust,ignore
let client: CachingClient<Box<dyn LlmClient>> =
    socsim_llm::build_live_client(cache_path /* Option<&Path> */)?;
```

`build_live_client` composes **Ollama-first → OpenAI-fallback → type-erased → caching** from environment variables:

- **Ollama** (primary) via `OLLAMA_HOST` (default `http://localhost:11434`) and `OLLAMA_MODEL` (default `llama3.1`).
- **OpenAI** (best-effort fallback) via `OPENAI_API_KEY` and `OPENAI_MODEL` (default `gpt-4o-mini`); if `OPENAI_API_KEY` is unset a placeholder is built and only errors if Ollama itself fails (so an Ollama-only setup works).
- The backend is type-erased to `Box<dyn LlmClient>` so the same concrete return type covers both the production stack and an injected mock.

Construction is **lazy** — no network call happens until `CachingClient::complete` is invoked on a cache miss.

Pseudo-determinism comes from two pieces:

- **`PromptCache`** — a `hash(prompt + model)`-keyed (`cache_key`) prompt → response cache, either in-memory (`PromptCache::in_memory`) or JSON-file-backed (`PromptCache::open`, atomic save). `LlmConfig::deterministic()` sets `temperature = 0` and a fixed `seed`; combined with a warm cache, a re-run replays identical responses, turning a noisy model into a reproducible oracle.
- **`MetadataCollector`** / **`RunMetadata`** — `CallMetadata` records model / endpoint / temperature / seed / `cache_hit` for every call; `MetadataCollector::summary()` rolls these up into a serialisable `RunMetadata` (model, endpoint, generation settings, total calls, cache hits, cache-hit rate) that the replications persist (e.g. `llm_meta.json`).

For deterministic tests there is `mock::ScriptedClient` — a network-free `LlmClient` that answers via a closure — which slots into `CachingClient` exactly like a live backend.

This crate is **library-only** and **not** wired into the `socsim` binary; the lightweight replications depend on it directly via a git dependency.

---

## Result output helpers (socsim-results)

`socsim-results` factors out the output boilerplate the lightweight library-mode replications all hand-roll. Those replications ship their own `main.rs` + clap CLI and write outputs directly (no `Recorder`/`Scenario` machinery), so this crate is a dependency-light **leaf crate** — `std` plus `serde`/`serde_json`/`csv`/`chrono`, and **no `socsim-*` dependency**, so pulling it in never drags in `socsim-log`/`-config`/`-runner`.

![Result output convention](assets/arch-results.svg)

It provides the shared `results/` output convention:

- `timestamp()` — current local time as a `YYYYMMDD_HHMMSS` stamp.
- `create_run_dir(base)` — make a timestamped run directory `base/<timestamp>`; `ensure_dir(path)` is the idempotent `mkdir -p`.
- `refresh_latest_symlink(base, target)` — (re)point `base/latest` at the newest run (Unix symlink; best-effort no-op elsewhere).
- `write_csv(rows, path)` / `write_json(value, path)` — serde-backed CSV/JSON writers (the JSON writer is how the LLM `RunMetadata` from `socsim-llm` is persisted), returning a `WriteError` that wraps the I/O / CSV / JSON failure sources.

It is domain-agnostic by design: it offers only generic serialization primitives, so domain types (such as `socsim-llm`'s `RunMetadata`) live in their owning crates and are written here via `write_json`.

---

## Social network layer

`socsim-net` provides `SocialNetwork` — a thin, undirected-graph wrapper around `petgraph::UnGraph<AgentId, ()>` with an `AgentId → NodeIndex` map for O(1) lookups. Three random-graph generators are included, all accepting a `&mut SimRng`:

| Generator | Model |
|---|---|
| `SocialNetwork::erdos_renyi(ids, p, rng)` | Erdős–Rényi G(n,p) |
| `SocialNetwork::watts_strogatz(ids, k, beta, rng)` | Watts–Strogatz small-world |
| `SocialNetwork::barabasi_albert(ids, m, rng)` | Barabási–Albert preferential attachment |

The HR lifecycle baseline uses `watts_strogatz(k=4, beta=0.1)` to model a small-world inter-employee network. The `toxic_spread` and `turnover` mechanisms query neighbour lists at each step.

---

## Calibration philosophy

The HR lifecycle module separates two categories of parameters:

### Empirical correlations (ρ)

These are **fixed influence strengths** drawn directly from published meta-analyses. They represent the direction and relative magnitude of an effect as documented in the literature. Researchers should not modify them unless replacing the underlying citation.

| Constant | Value | Source |
|---|---|---|
| `RHO_SI` | 0.51 | Schmidt & Hunter (1998) — structured-interview validity |
| `ALPHA_PEER` | 0.17 | Mas & Moretti (2009) — peer-productivity multiplier |
| `P_TOXIC` | 0.04 | Housman & Minor (2015) — baseline toxic-worker prevalence |
| `P_SPREAD` | 0.46 | Housman & Minor (2015) — toxic-behaviour contagion probability |
| `PHI_TACIT` | 0.85 | Nonaka (1994) — tacit-to-total knowledge ratio |
| `RHO_PJ` | 0.20 | Kristof-Brown et al. (2005) — PJ-fit correlation |
| `RHO_PO` | 0.07 | Kristof-Brown et al. (2005) — PO-fit correlation |
| `RHO_PO_TURN` | −0.35 | Kristof-Brown et al. (2005) — PO-fit vs turnover intent |
| `LAMBDA_LEARN` | 0.15 | Bahk & Gort (1993) — learning-curve growth rate |

### Monthly-dynamics scale parameters (tunable)

These are **calibration controls** that govern the pace and magnitude of the simulation's monthly dynamics. They have no direct empirical counterpart but are tuned so the model produces plausible trajectories (e.g. ~15–22%/year voluntary turnover, a knowledge stock that grows gradually without diverging).

| Constant | Value | Governs |
|---|---|---|
| `BASE_MONTHLY_QUIT_HAZARD` | 0.008 | Baseline ~0.8%/month quit probability |
| `BASE_QUIT_LOGIT` | −4.82 | Logit intercept (`logit(0.008)`) |
| `QUIT_EMBED_SENS` | 1.0 | Sensitivity of quit logit to (1 − embeddedness) |
| `QUIT_SAT_SENS` | 0.8 | Sensitivity of quit logit to (1 − satisfaction) |
| `QUIT_CASCADE_BUMP` | 0.30 | Per-quit-neighbour additive logit bump (Krackhardt cascade) |
| `ALPHA_K` | 0.30 | OCB inflow coefficient into team knowledge stock |
| `BETA_LOSS` | 1.0 | Knowledge-loss exponent on tenure (in years) |
| `KAPPA_LOSS` | 0.40 | Knowledge-loss magnitude coefficient |
| `THETA_MEAN` | 1.0 | Mean true ability θ at hire |
| `THETA_SD` | 0.2 | Standard deviation of θ |

All calibration constants live in `crates/socsim-packs/src/hr_lifecycle/calibration.rs` with doc-comments citing their sources.

---

## Scenario TOML schema

A scenario TOML has four sections:

```toml
[simulation]   # name, module_pack, t_max, seed, scheduler
[world]        # free-form params forwarded to the world factory
[[mechanism]]  # ordered array; one entry per mechanism to compose
[output]       # log_path template and metric keys
```

The `[[mechanism]]` array is **order-preserving**: composition order equals declaration order. Within each `Phase`, mechanisms fire in the order they appear in the scenario file.

The `output.log_path` template supports `{name}` and `{seed}` substitutions.

Two schedulers are available: `sequential` (sorted `AgentId` order, fully deterministic) and `random_activation` (shuffled each step using the simulation RNG).
