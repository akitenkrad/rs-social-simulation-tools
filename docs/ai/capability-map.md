# socsim Capability Map (for AI coding agents)

`socsim` (`rs-social-simulation-tools`) is a composable, **seed-deterministic** agent-based social-simulation platform in Rust, organised as an **eighteen-crate** workspace. This page is the curated **capability map** for an AI agent building an implementation — usually an academic-paper replication — **on top of** socsim consumed as a git dependency.

Three sources, three jobs:

- **This map** = the curated, task-oriented index: which crate to reach for, the exact import paths and signatures you actually call, and the rules you must not break.
- **`cargo doc -p <crate>`** = the full, authoritative API (everything; this map keeps only the load-bearing surface).
- **The human docs in `docs/*.md`** (`architecture.md`, `library.md`, `design.md`, `packs.md`, `mechanisms.md`, `cli.md`) = the prose, worked examples, and rationale.

> Companion file: **`docs/ai/recipes.md`** — copy-paste templates (`Cargo.toml` + `main.rs` skeletons per replication shape). Read this map for *what exists*; read recipes for *how to wire it*.

All git snippets below use:
`git = "https://github.com/akitenkrad/rs-social-simulation-tools"`, `branch = "main"`, `package = "<crate>"`.

---

## Mental model (read this first)

**One world, many mechanisms, six phases per tick.**

- A **`WorldState`** owns all shared state (agent roster + `SimClock` + your domain data). You implement it.
- A **`Mechanism<W>`** is one composable unit of research logic (the analogue of a neural-net layer). It declares which **`Phase`**s it runs in and mutates the world inside `apply`. You implement these.
- Each tick runs the **fixed 6-phase loop**, in this exact order (`socsim_core::Phase::ORDER`):

  ```
  PreStep → Environment → Decision → Interaction → Reward → PostStep
  ```

  Per phase, every mechanism that registered that phase fires in **insertion order**. (Verified: `crates/socsim-core/src/lib.rs` `Phase::ORDER`.)
- A **`Simulation<W>`** (built by **`SimulationBuilder<W>`**) drives the loop. A **`Scheduler<W>`** picks each step's agent activation order; a seeded **`SimRng`** (ChaCha20) supplies all randomness; a **`Recorder`** sinks metrics/events (default `NullRecorder` = no-op).

**Two usage modes (both first-class):**

| Mode | What you write | Crates you pull in | Used by |
|---|---|---|---|
| **(a) Library mode** | your own `main.rs` + clap CLI, `WorldState`, `Mechanism`s; drive `SimulationBuilder` directly | only the crates you need (`socsim-core`/`-engine`, optionally `-grid`/`-net`/`-mechanisms`/…) | **most replications** |
| **(b) CLI mode** | a `Scenario` TOML + a `ModulePack`; run via the `socsim` binary | the `socsim` binary + a pack | bundled packs, sweepable scenario files |

**LLM-social replications** that have no spatial/ABM engine deliberately do **not** pull in `socsim-core`/`-engine`/`-net`; they depend on `socsim-llm` (+ `-results`/`-survey`/`-reproduce`/`-metrics`) only. **Spatial/ABM** replications use library mode with `socsim-core`/`-engine`/`-grid`/`-net`.

**Two-layer determinism.** The socsim core is **seed-deterministic and LLM-free**: same seed + same code ⇒ identical trajectory. The optional **`socsim-llm`** layer is made *cache-pseudo-deterministic* on top — LLM nondeterminism is confined to a prompt cache and, by convention, to the **`Decision`** phase (an LLM call is a synchronous `complete` inline in `Mechanism::apply`).

Minimal library-mode shape (signatures verified against `socsim-core` / `socsim-engine`):

```rust,ignore
use socsim_core::{AgentId, Mechanism, Phase, Result, SimClock, StepContext, WorldState};
use socsim_engine::{RandomActivationScheduler, SimulationBuilder};

let mut sim = SimulationBuilder::new(my_world)   // SimulationBuilder::new(world: W)
    .scheduler(Box::new(RandomActivationScheduler))
    .seed(42)                                     // default seed is 0
    .add_mechanism(Box::new(MyMechanism::new()))
    .build();
sim.run()?;                                       // or run_until / run_observed / step
let final_state = sim.world();
```

---

## Capability → crate table

| I want to… | Use crate | Key entry points (import) | Notes |
|---|---|---|---|
| Define a world + mechanism | `socsim-core` | `socsim_core::{WorldState, Mechanism, Phase, StepContext, AgentId, SimClock, Result, Recorder, Blackboard}` | Engine-free foundation; pick the `Phase` deliberately. |
| Build & run a simulation | `socsim-engine` | `socsim_engine::{SimulationBuilder, Simulation, SequentialScheduler, RandomActivationScheduler}` | `run` / `run_until` / `run_observed` / `step`. |
| Seeded / derived randomness | `socsim-rng` (re-exported by core) | `socsim_core::{SimRng, derive_seed}` | All randomness goes through this. |
| Multi-seed runs + sweeps + summaries | `socsim-runner` | `socsim_runner::{run_once, run_seeds, run_sweep, summarize, RunResult, Summary, SweepAxis, SweepPoint, WorldFactory}` | Needs a `Scenario` + `Registry`; rayon-parallel. CLI-mode oriented. |
| Social-network topology (ER / WS / BA) | `socsim-net` | `socsim_net::{SocialNetwork, DiSocialNetwork, WeightedNetwork}` → `erdos_renyi` / `watts_strogatz` / `barabasi_albert` | Generators take `&mut SimRng`. |
| Spatial 2-D grid / lattice | `socsim-grid` | `socsim_grid::{Grid, GridIndex, CellGrid, Adjacency, Boundary, Neighborhood, Metric}` | `GridIndex` = agent↔cell; `CellGrid<T>` = per-cell state. |
| Opinion / contagion / cultural / group mechanisms | `socsim-mechanisms` | `socsim_mechanisms::{HegselmannKrauseMechanism, DeffuantMechanism, SocialJudgementMechanism, LorenzMechanism, SiContagionMechanism, ThresholdContagionMechanism, PerAgentThresholdContagionMechanism, AxelrodMechanism, GroupConformityMechanism, MeanOperator}` | Reusable catalog over `socsim-core` capability traits; 4 feature families (all default-on). |
| LLM-agent decisions | `socsim-llm` | `socsim_llm::{LlmClient, LlmConfig, LlmResponse, build_live_client_from_settings, wrap_client, LlmSettings, LiveClient, extract_first_choice, MetadataCollector}` | Confine to `Decision` phase; `features=["live"]` for networking. |
| Survey microdata recode | `socsim-survey` | `socsim_survey::{SurveySchema, DemoVar, ValMap, AgeBins, OutcomeMap, recode_row, demo_label, actual_outcome, estimate_distributions}` | Generic engine only — schemas live in `socsim-datasets`. |
| Dataset schemas + registry + download | `socsim-datasets` | `socsim_datasets::{all, by_key, DatasetMeta, DataFile, Source}`; `socsim_datasets::anes::{anes_2012, anes_2016, anes_2020}`; `socsim_datasets::ces::ces_2022`; `acquire::{fetch, raw_to_csv}` (feature) | Never vendors data; ANES is license-gated (`Source::Manual`); CES 2022 is CC0/Dataverse (auto). |
| Reproduction (paper-anchor PASS/off) | `socsim-reproduce` | `socsim_reproduce::{Anchor, AnchorStatus, compare_anchor, build_rows, write_reproduce_summary, write_paper_anchors, find_latest}` | Ships mechanics, **no** anchor values — you supply `&[Anchor]` + lookup. |
| Reusable metrics (stats / distribution / agreement) | `socsim-metrics` | `socsim_metrics::stats::{mean, variance, gini, …}`, `::distribution::{kl_divergence, chi_square_homogeneity, …}`, `::agreement::{cohen_kappa, tetrachoric, …}`, `::opinion::{MetricsMechanism, …}` (feature `core`) | Default build = zero socsim deps. Keep paper-specific metrics local. |
| Output dirs + CSV/JSON + `latest` symlink | `socsim-results` | `socsim_results::{timestamp, create_run_dir, refresh_latest_symlink, write_csv, write_json, WriteError}` | Leaf crate; the `results/<ts>/` convention without a `Recorder`. |
| Metric / event logging (Recorders) | `socsim-log` | `socsim_log::{InMemoryRecorder, JsonlRecorder, CsvRecorder}` | Only needed when you use a `Recorder`; library models can skip it (`NullRecorder` default). |
| Learnable Decision-phase policies (MARL) | `socsim-marl` | `socsim_marl::{Policy, DiscretePolicyNet, NetConfig, PolicyMechanism, MarlTrainer, TrainConfig, ObsEncoder, ActionApplier, RewardFn, TrajectoryBuffer}` | Pulls in `burn`; library-only. |
| Bundled CLI packs (HR / opinion / silence) | `socsim-packs` + `socsim-config` | `socsim_packs::hr_lifecycle::{HrWorld, HrLifecyclePack}`; `socsim_config::{ModulePack, Registry, Params, Scenario}` | CLI-mode building blocks; usable as a library too. |
| Run / sweep / summarise via CLI | `socsim-cli` (binary `socsim`) | `socsim run|sweep|summarize|validate|list|init|datasets` | World-polymorphic; packs selected by name. |

---

## Per-crate digest

> **Layer key:** *foundation* (no internal deps) · *engine spine* (the CLI/run stack) · *orthogonal + optional* (beside the engine) · *library-only leaves* (engine-free helpers).

### Foundation

#### `socsim-core`
- **Purpose:** the four composable traits + core types every other crate builds on.
- **Engine-free?** Yes. **Depends-on:** `socsim-rng` (re-exported).
- **Git dep:**
  ```toml
  socsim-core = { git = "https://github.com/akitenkrad/rs-social-simulation-tools", branch = "main", package = "socsim-core" }
  ```
- **Feature flags:** none.
- **Key public API** (all `socsim_core::…`, from `crates/socsim-core/src/lib.rs`):
  - `AgentId(pub u64)` — `Copy + Ord + Hash + serde`.
  - `SimClock::new(t_max: u64)` · `.t() -> u64` · `.t_max()` · `.is_done() -> bool` · `.tick()`.
  - `enum Phase { PreStep, Environment, Decision, Interaction, Reward, PostStep }`; `Phase::ORDER: [Phase; 6]`.
  - `trait WorldState: 'static` — `fn agent_ids(&self) -> Vec<AgentId>`; `fn clock(&self) -> &SimClock`; `fn clock_mut(&mut self) -> &mut SimClock`.
  - `trait Mechanism<W: WorldState>` — `fn name(&self) -> &str`; `fn phases(&self) -> &'static [Phase]`; `fn apply(&mut self, phase: Phase, ctx: &mut StepContext<'_, W>) -> Result<()>`.
  - `trait Scheduler<W: WorldState>` — `fn activation_order(&mut self, world: &W, rng: &mut SimRng) -> Vec<AgentId>`.
  - `trait Recorder` — `record_metric(&mut self, t: u64, key: &str, value: f64)`; `record_event(&mut self, t, kind: &str, payload: serde_json::Value)`; `record_row(&mut self, t, table: &str, row: &[(&str, f64)])` (default fans out to `record_metric`); `as_any(&self) -> Option<&dyn Any>`.
  - `struct NullRecorder` (the engine default sink).
  - `struct StepContext<'a, W>` pub fields: `world: &'a mut W`, `clock: SimClock` (a value copy), `rng: &'a mut SimRng`, `recorder: &'a mut dyn Recorder`, `agent_order: &'a [AgentId]`, `scratch: &'a mut Blackboard`, `stop: &'a mut bool`; method `request_stop(&mut self)`.
  - `struct Blackboard` — step-scoped, type-erased: `insert<T>(&mut self, key: &'static str, value: T)`, `get::<T>(key)`, `get_mut::<T>(key)`, `clear()`. Cleared by the engine at the start of every step.
  - `enum SocsimError` + `type Result<T> = std::result::Result<T, SocsimError>`.
  - **Capability traits** (in `socsim_core::opinion`, re-exported at crate root) — opt-in bounds that let `socsim-mechanisms`/`socsim-metrics` operate on any world:
    - `ScalarOpinions` — `opinion(id) -> f64` / `set_opinion(id, value)`.
    - `Neighbors` — `neighbors_of(id) -> Vec<AgentId>` (the influence set).
    - `BinaryState` — `is_active(id) -> bool` / `set_active(id, bool)`.
    - `CultureVectors` — `n_features() -> usize` / `feature(id, f) -> u32` / `set_feature(id, f, value)`.
    - `ActivationThreshold` — `activation_threshold(id) -> f64` (per-agent θ_i).
    - `GroupMembership` (+ `type GroupId = u64`) — partition structure for group dynamics.
- See `cargo doc -p socsim-core` for the rest.

#### `socsim-rng`
- **Purpose:** seeded, reproducible ChaCha20 RNG + label-based seed derivation.
- **Engine-free?** Yes. **Depends-on:** none.
- **Git dep:** usually unnecessary directly — `socsim-core` re-exports `SimRng` and `derive_seed`. If needed:
  ```toml
  socsim-rng = { git = "https://github.com/akitenkrad/rs-social-simulation-tools", branch = "main", package = "socsim-rng" }
  ```
- **Feature flags:** none.
- **Key public API** (`crates/socsim-rng/src/lib.rs`):
  - `SimRng::from_seed(seed: u64) -> SimRng` — the root RNG (impls `rand::RngCore`, `Clone`, serde).
  - `SimRng::derive(&self, label: &[u64]) -> SimRng` — child RNG from a label, without mutating `self`.
  - `derive_seed(root: u64, parts: &[u64]) -> u64` — the FNV-1a mix used by `derive`. Convention: `derive_seed(root, &[0])` = world init, `derive_seed(root, &[1])` = engine/scheduler.

### Engine spine

#### `socsim-engine`
- **Purpose:** the `Simulation` driver, `SimulationBuilder`, schedulers, snapshots.
- **Engine-free?** No (it *is* the engine). **Depends-on:** `socsim-core` (runtime). *(`socsim-config`/`-grid`/`-log`/`-net` are dev-only deps for examples.)*
- **Git dep:**
  ```toml
  socsim-engine = { git = "https://github.com/akitenkrad/rs-social-simulation-tools", branch = "main", package = "socsim-engine" }
  ```
- **Feature flags:** none.
- **Key public API** (`crates/socsim-engine/src/lib.rs`):
  - `SimulationBuilder::new(world: W) -> Self` → `.add_mechanism(Box<dyn Mechanism<W>>)` · `.scheduler(Box<dyn Scheduler<W>>)` · `.seed(u64)` · `.recorder(Box<dyn Recorder>)` · `.build() -> Simulation<W>`. **Defaults:** scheduler `SequentialScheduler`, seed `0`, recorder `NullRecorder`. (Verified.)
  - `Simulation<W>`: `run()` (until clock done **or** stop requested); `run_until<F: Fn(&W) -> bool>(predicate)`; `run_observed<F: FnMut(StepReport<'_, W>)>(observe)`; `step()`; `step_reported() -> Result<StepReport<'_, W>>`; `stop_requested() -> bool`; `world() -> &W`; `scratch() -> &Blackboard`; `recorder() -> &dyn Recorder`; `recorder_mut()`; `snapshot() -> Snapshot<W>` (`where W: Clone`); `restore(Snapshot<W>)`.
  - `StepReport<'a, W>` pub fields: `t: u64`, `stopped: bool`, `world: &'a W`, `scratch: &'a Blackboard`.
  - `Snapshot<W>` (`Serialize`/`Deserialize`): `save(path)` (`W: Serialize`), `Snapshot::load(path)` (`W: DeserializeOwned`); `SNAPSHOT_VERSION`.
  - `SequentialScheduler` (sorted `AgentId`) · `RandomActivationScheduler` (shuffled each step). These are the **only two** schedulers. (Verified.)

#### `socsim-config`
- **Purpose:** `Params` (typed TOML), `Registry` (mechanism factory), `ModulePack` trait, `Scenario` loader.
- **Engine-free?** Yes (deliberately, to avoid an engine→config→engine cycle). **Depends-on:** `socsim-core`.
- **Git dep:**
  ```toml
  socsim-config = { git = "https://github.com/akitenkrad/rs-social-simulation-tools", branch = "main", package = "socsim-config" }
  ```
- **Feature flags:** none.
- **Key public API** (`crates/socsim-config/src/lib.rs`, `scenario.rs`):
  - `Params::empty()`; `get_f64(key, default)` / `get_u64` / `get_i64` / `get_bool` / `get_str(key, default: &str) -> &str`. (Note: `get_i64` exists; there is no `get_string`.) `impl From<toml::Table>`.
  - `Registry<W>::new()`; `register<F: Fn(&Params) -> Result<Box<dyn Mechanism<W>>> + 'static>(&mut self, name: &str, ctor: F)`; `build(name, &Params) -> Result<Box<dyn Mechanism<W>>>`; `names() -> Vec<&str>`.
  - `trait ModulePack<W>` — `pack_name(&self) -> &str`; `register(&self, reg: &mut Registry<W>)`.
  - `Scenario` (`from_path(&Path)` / `parse(text)` / `impl FromStr`; `validate(&self, registry_names: &[&str])`); sections `SimulationSection { name, module_pack, t_max, seed, scheduler }`, `world: RawParams`, `mechanisms: Vec<MechanismEntry>`, `output: OutputSection`.

#### `socsim-log`
- **Purpose:** concrete `Recorder` implementations.
- **Engine-free?** Yes. **Depends-on:** `socsim-core`.
- **Git dep:**
  ```toml
  socsim-log = { git = "https://github.com/akitenkrad/rs-social-simulation-tools", branch = "main", package = "socsim-log" }
  ```
- **Feature flags:** none.
- **Key public API** (`crates/socsim-log/src/lib.rs`):
  - `InMemoryRecorder::new()` — `metrics() -> &[MetricRow]`, `events() -> &[EventRow]`; supports `as_any` downcast.
  - `JsonlRecorder<W: Write>::new(sink)` — one JSON line per record; `take_error()`. *(Does not override `record_row` and does not support `as_any` downcast.)*
  - `CsvRecorder::new()` — `record_row`-aware; `set_columns(table, &[&str])` (pin header order), `table_csv(table) -> Option<String>`, `metrics_csv() -> String`, `tables()`, `events()`; supports `as_any` downcast.
  - Row types: `MetricRow { t, key, value }`, `EventRow { t, kind, payload }`.

#### `socsim-runner`
- **Purpose:** single/multi-seed runs, cross-seed summaries, parameter sweeps (rayon-parallel).
- **Engine-free?** No. **Depends-on:** `socsim-core`, `socsim-config`, `socsim-engine`, `socsim-log`.
- **Git dep:**
  ```toml
  socsim-runner = { git = "https://github.com/akitenkrad/rs-social-simulation-tools", branch = "main", package = "socsim-runner" }
  ```
- **Feature flags:** none.
- **Key public API** (`crates/socsim-runner/src/lib.rs`) — note American spelling **`summarize`**:
  - `type WorldFactory<W> = Box<dyn Fn(&Params, u64) -> Result<W> + Send + Sync>`.
  - `run_once(scenario, world_factory, register, seed) -> Result<RunResult>`.
  - `run_seeds(scenario, world_factory, register, seeds, parallel: bool) -> Result<Vec<RunResult>>`.
  - `summarize(results: &[RunResult]) -> Summary` (→ `Summary::to_csv()` / `to_json()`).
  - `run_sweep(scenario, axes: &[SweepAxis], world_factory, register, seeds: Vec<u64>, parallel) -> Result<Vec<SweepPoint>>`.
  - Data: `RunResult { seed, series, final_metrics, events, event_count }`, `MetricStats`, `Summary { metrics }`, `SweepAxis { param_key, values }`, `SweepPoint { params, summary }`.

#### `socsim-cli` (binary `socsim`)
- **Purpose:** the world-polymorphic `socsim` binary wrapping packs + runner.
- **Engine-free?** No. **Depends-on:** `socsim-core`/`-config`/`-engine`/`-log`/`-runner`/`-datasets`; `socsim-packs` (optional).
- **Consume as:** the **binary** (`cargo build --release` → `target/release/socsim`), not a library dep. Replications generally use library mode instead.
- **Feature flags** (default on: `pack-hr-lifecycle`, `pack-opinion-dynamics`, `pack-organizational-silence`):
  | Feature | Gates |
  |---|---|
  | `pack-hr-lifecycle` / `pack-opinion-dynamics` / `pack-organizational-silence` | the three bundled `CliPack`s |
  | `pack-organizational-silence-llm` | opt-in LLM voice-decision in the silence pack |
  | `datasets-acquire` | enables `socsim-datasets/acquire` for the `datasets` subcommand |
- **Subcommands:** `init --module-pack <PACK> -o <PATH>` · `run <scenario> [--seeds A..B] [--parallel]` · `validate <scenario>` · `list <packs|mechanisms>` · `sweep <scenario> --param MECH.PARAM=v1,v2 [--seeds 0..5] [-o runs/sweep] [--parallel]` · `summarize <path> [--format csv|json]` · `datasets <subcommand>`.

### Orthogonal + optional

#### `socsim-net`
- **Purpose:** `AgentId`-keyed graph (petgraph) with reproducible random generators.
- **Engine-free?** Yes (orthogonal to the engine). **Depends-on:** `socsim-core`.
- **Git dep:**
  ```toml
  socsim-net = { git = "https://github.com/akitenkrad/rs-social-simulation-tools", branch = "main", package = "socsim-net" }
  ```
- **Feature flags:** none.
- **Key public API** (`crates/socsim-net/src/lib.rs`):
  - Type aliases: `SocialNetwork = Network<(), Undirected>` · `DiSocialNetwork = Network<(), Directed>` · `WeightedNetwork<E> = Network<E, Undirected>` · `DiWeightedNetwork<E>`.
  - Generators (`&mut SimRng`): `SocialNetwork::erdos_renyi(ids: &[AgentId], p: f64, rng)` · `::watts_strogatz(ids, k: usize, beta: f64, rng)` · `::barabasi_albert(ids, m: usize, rng)` · `::empty()`. Directed: `erdos_renyi_directed`, `barabasi_albert_directed`, `.to_directed(p_mutual, rng)`.
  - Queries: `neighbors(id) -> Vec<AgentId>` · `neighbors_into(id, &mut Vec)` · `neighbors_iter(id)` · `degree(id)` · `node_count()` · `edge_count()` · `contains(id)` · `connected_components() -> usize` · `component_membership()` · `edges()`.
  - Mutation: `add_node` / `add_edge(a, b)` (unweighted) / `add_edge_weighted(a, b, w)` / `remove_node` / `remove_edge`.
  - Directed-only: `out_neighbors` / `in_neighbors` / `out_degree` / `in_degree`.

#### `socsim-grid`
- **Purpose:** 2-D lattice, neighbourhoods, distances, occupancy + per-cell state.
- **Engine-free?** Yes. **Depends-on:** `socsim-core`.
- **Git dep:**
  ```toml
  socsim-grid = { git = "https://github.com/akitenkrad/rs-social-simulation-tools", branch = "main", package = "socsim-grid" }
  ```
- **Feature flags:** none.
- **Key public API** (`crates/socsim-grid/src/lib.rs`):
  - `enum Boundary { Fixed, Toroidal }` · `enum Neighborhood { Moore, VonNeumann }` · `enum Metric { Chebyshev, Manhattan, Euclidean }`.
  - `Grid::new(rows: usize, cols: usize, boundary: Boundary)`; `neighbors(r, c, nbhd) -> Vec<(usize,usize)>` (+ `_into` / `_radius` / `_radius_into` / `_iter`); `distance(metric, a, b)`; `adjacency(nbhd) -> Adjacency` / `adjacency_radius(nbhd, radius)`.
  - `GridIndex::new(grid)`; `place(id, r, c) -> Result<(), GridError>`; `move_to(id, r, c)`; `nearest_vacant(from: (usize,usize), metric) -> Option<(usize,usize)>`; `occupant_neighbors(r, c, nbhd) -> Vec<AgentId>`; `vacant_cells()`; `agent_ids()`.
  - `CellGrid<T>::new(grid, fill)` (`T: Clone`) / `::from_fn(grid, |r, c| …)`; `get_idx(idx)` / `get_idx_mut(idx)` (flat row-major, matches `Adjacency`); `cells()` / `cells_mut()`; `neighbors(r, c, nbhd)` / `neighbor_values`.
  - `Adjacency::neighbors(idx: usize) -> &[usize]` (CSR, O(1), precompute once for hot loops).

#### `socsim-mechanisms`
- **Purpose:** the **general, reusable mechanism catalog** (domain-agnostic building blocks).
- **Engine-free?** Yes. Library-only (no `ModulePack`, not in the binary). **Depends-on:** `socsim-core` only (operates over its capability traits).
- **Git dep:**
  ```toml
  socsim-mechanisms = { git = "https://github.com/akitenkrad/rs-social-simulation-tools", branch = "main", package = "socsim-mechanisms" }
  # default features: opinion-dynamics, contagion, cultural, group-dynamics (all on)
  ```
- **Feature flags** (four families, **all default-on**): `opinion-dynamics` (HK/Deffuant/SocialJudgement/Lorenz + `MeanOperator`) · `contagion` (SI + threshold) · `cultural` (Axelrod) · `group-dynamics` (GroupConformity). Disable with `default-features = false` then re-enable selectively.
- **Key public API** (`crates/socsim-mechanisms/src/…`) — constructor + required capability bound:
  | Mechanism | Constructor | Requires `Mechanism<W>` where `W:` |
  |---|---|---|
  | `HegselmannKrauseMechanism` | `new(epsilon: f64, mean: MeanOperator)` (also `with_asymmetric(eps_l, eps_r, mean)`, `Default`) | `ScalarOpinions + Neighbors` |
  | `DeffuantMechanism` | `new(epsilon: f64, mu: f64, pairs_per_step: usize)` | `ScalarOpinions + Neighbors` |
  | `SocialJudgementMechanism` | `new(epsilon, alpha, rejection, repulsion: f64)` | `ScalarOpinions + Neighbors` |
  | `LorenzMechanism` | `new(epsilon, alpha, repulsion: f64)` | `ScalarOpinions + Neighbors` |
  | `SiContagionMechanism` | `new(beta: f64)` | `BinaryState + Neighbors` |
  | `ThresholdContagionMechanism` | `new(theta: f64)` | `BinaryState + Neighbors` |
  | `PerAgentThresholdContagionMechanism` | `new()` | `BinaryState + Neighbors + ActivationThreshold` |
  | `AxelrodMechanism` | `new(events_per_step: usize)` | `CultureVectors + Neighbors` |
  | `GroupConformityMechanism` | `new(alpha: f64)` | `GroupMembership + ScalarOpinions` |
  - `MeanOperator` is an **enum**, not a struct: `MeanOperator::{Arithmetic, Geometric, Harmonic, Power(f64), Random}` (`Default = Arithmetic`). It is the averaging strategy passed into `HegselmannKrauseMechanism::new`.
  - Free Δ functions (for hybrid mechanisms that reuse the update math): `bounded_confidence_update(a_i, messages: &[f64], epsilon, alpha) -> f64`, `hk_update(…)`, `social_judgement_update(a_i, messages, epsilon, alpha, rejection, repulsion)`, `lorenz_update(a_i, messages, epsilon, alpha, repulsion)`. Initial profile helper: `regular_profile(n: usize) -> Vec<f64>`.
  - ⚠️ The free `hk_update` / `bounded_confidence_update` use a **different formula** than the like-named *mechanisms* (only `SocialJudgementMechanism` / `LorenzMechanism` route through the free fns), and their confidence window is **strict `<`** vs the mechanisms' inclusive `<=` — they differ exactly at `|diff| == ε`. Prefer the mechanism unless you specifically need the bare Δ.

#### `socsim-llm`
- **Purpose:** optional LLM-agent layer; provider-agnostic client + prompt cache + run metadata.
- **Engine-free?** Yes — **no `socsim-*` deps** (`serde`/`serde_json`/`thiserror`, `ureq` behind features). Library-only.
- **Git dep:**
  ```toml
  socsim-llm = { git = "https://github.com/akitenkrad/rs-social-simulation-tools", branch = "main", package = "socsim-llm", features = ["live"] }
  ```
- **Feature flags** (default = **none**, no networking): `ollama` / `openai` (each adds `ureq`) · `live = ["ollama", "openai"]` (enables `build_live_client*` + the fallback stack).
- **Key public API** (`crates/socsim-llm/src/…`):
  - `trait LlmClient` — `model(&self) -> &str`; `endpoint(&self) -> &str`; `complete(&self, prompt: &str, config: &LlmConfig) -> Result<LlmResponse, LlmError>`; `complete_with_logprobs(&self, prompt, config)` (default impl → `LlmError::Unsupported`; overridden by live backends).
  - `LlmConfig` fields: `temperature: f32`, `seed: u64`, `max_tokens: Option<u32>`, `system: Option<String>`, `omit_seed: bool`, `allow_blank: bool`, `top_logprobs: Option<u32>`. Ctors: `deterministic()` (temp 0, fixed seed) / `sampling(temperature)`; builders `with_seed` / `with_temperature` / `with_max_tokens` / `with_system` / `omit_seed` / `allow_blank` / `with_top_logprobs`.
  - `LlmResponse { text: String, metadata: CallMetadata, logprobs: Option<Vec<TokenLogprob>> }`; `TokenLogprob { token, bytes, logprob }`.
  - `enum LlmError { Transport, Backend, Config, EmptyResponse { endpoint, model }, AllBackendsFailed, Unsupported { endpoint, operation } }`. **Blank (whitespace-only) completions are rejected by default** as `EmptyResponse` — opt out per call via `LlmConfig::allow_blank()`.
  - Caching: `CachingClient<C>::new(inner, cache)` (its `complete` takes `&mut self`); `SharedCachingClient<C>` (interior-mutable, itself `impl LlmClient`, so injectable as `&dyn LlmClient`); `PromptCache::in_memory()` / `::open(path)`; free `cache_key(prompt, model) -> String`.
  - Harness (prefer this over a hand-rolled `llm.rs`): `LlmSettings { temperature: f32, seed: u64, cache_path: Option<String> }`; `type LiveClient = CachingClient<Box<dyn LlmClient>>`; `build_live_client_from_settings(&LlmSettings) -> Result<LiveClient, LlmError>` (feature `live`); `wrap_client(backend, PromptCache) -> LiveClient` (inject a mock/test backend); `llm_config(&LlmSettings) -> LlmConfig`. Shared variants: `wrap_client_shared`, `build_shared_live_client_from_settings`. Lower-level: `build_live_client(cache_path: Option<&Path>)`.
  - Parsing: `extract_first_choice<'a>(text: &str, vocab: &[(&'a str, &[&str])]) -> Option<&'a str>` (word-boundary, earliest-then-longest match; always compiled).
  - Metadata: `MetadataCollector::new()` → `record(CallMetadata)` → `summary() -> RunMetadata` (persist via `socsim_results::write_json`). `CallMetadata { model, endpoint, temperature, seed, cache_hit }`.
  - Test backend: `mock::ScriptedClient::new(model, |prompt| …)` / `::constant(model, reply)`; `mock::AlwaysFailClient`.

#### `socsim-marl`
- **Purpose:** learnable `Decision`-phase policies (REINFORCE on a `burn` MLP).
- **Engine-free?** No. **Depends-on:** `socsim-core`, `socsim-engine`, `socsim-log` (+ `burn`). Library-only.
- **Git dep:**
  ```toml
  socsim-marl = { git = "https://github.com/akitenkrad/rs-social-simulation-tools", branch = "main", package = "socsim-marl" }
  ```
- **Feature flags:** none.
- **Key public API** (`crates/socsim-marl/src/…`):
  - `trait Policy` — `act(&self, obs: &[f32]) -> usize`; `sample(&self, obs, rng) -> usize`; `update(&mut self, episodes) -> Result<f32>`; `obs_dim()` / `n_actions()`.
  - `trait ObsEncoder<W>` (`obs_dim`, `encode(&self, world, agent) -> Option<Vec<f32>>`); `trait ActionApplier<W>` (`n_actions`, `apply(&self, world, agent, action: usize, rng)`); `trait RewardFn<W>` (`reward(&self, world, agent) -> f32`).
  - `DiscretePolicyNet::new(cfg: NetConfig, rng: &mut SimRng) -> Result<Self>`; `NetConfig::new(obs_dim, n_actions)` (fields `obs_dim`, `hidden`, `n_actions`, `lr`, `gamma`).
  - `PolicyMechanism::collecting(policy: Rc<RefCell<P>>, encoder, applier, buffer)` / `::inference(policy, encoder, applier)` (frozen, greedy, RNG-free).
  - `MarlTrainer::new(policy)` → `train(&TrainConfig, env_factory, &dyn RewardFn<W>) -> Result<Vec<EpisodeStat>>`; `TrainConfig { episodes: usize, seed: u64 }`; `TrajectoryBuffer`.

### Library-only leaves (engine-free helpers)

#### `socsim-results`
- **Purpose:** the `results/<timestamp>/` + `latest`-symlink output convention, no `Recorder` needed.
- **Engine-free?** Yes — **no `socsim-*` deps** (`std`/`serde`/`serde_json`/`csv`/`chrono`).
- **Git dep:**
  ```toml
  socsim-results = { git = "https://github.com/akitenkrad/rs-social-simulation-tools", branch = "main", package = "socsim-results" }
  ```
- **Feature flags:** none.
- **Key public API** (`crates/socsim-results/src/lib.rs`):
  - `timestamp() -> String` (`"YYYYMMDD_HHMMSS"`); `ensure_dir(path)`; `create_run_dir(base) -> io::Result<PathBuf>` (`base/<ts>`); `refresh_latest_symlink(base, target: &str)`.
  - `write_csv<T: Serialize>(rows: &[T], path) -> Result<(), WriteError>`; `write_json<T: Serialize>(value: &T, path)`; `enum WriteError { Io, Csv, Json }`.

#### `socsim-metrics`
- **Purpose:** reusable, read-only observation metrics (dispersion, inequality, diversity, distribution-comparison, rater-agreement) + opt-in world adapters.
- **Engine-free?** **By default, yes** — `default-features = false` (the default `[features]` set is empty) compiles only the zero-dep `stats`/`distribution`/`agreement` modules. **Depends-on:** `socsim-core`/`-net`/`-grid` *only behind features*.
- **Git dep:**
  ```toml
  # zero socsim deps (stats/distribution/agreement only):
  socsim-metrics = { git = "https://github.com/akitenkrad/rs-social-simulation-tools", branch = "main", package = "socsim-metrics" }
  # opinion-world adapters:
  # socsim-metrics = { …, package = "socsim-metrics", features = ["core"] }
  ```
- **Feature flags** (default = none): `core` (→ `socsim-core`; opinion extractors + `MetricsMechanism<W>`) · `network` (→ `socsim-net`; implies `core`) · `spatial` (→ `socsim-grid`; implies `core`).
- **Key public API** (`crates/socsim-metrics/src/…`):
  - `stats::{mean, variance, std_dev, spread, min_max, gini, shannon_entropy, hhi, simpson_diversity, distinct_clusters(values, tol), bimodality_coefficient, extremeness(xs, center), polarization, max_abs_delta, mean_abs_delta, num_distinct, largest_share}` (over `&[f64]` / `&[u32]`).
  - `distribution::{kl_divergence(p, q), chi_square_homogeneity(observed, expected) -> (stat, p), chi_square_sf(x, df), wasserstein_1d(p, q), nemd(p, q, range), mean_diff(p, q, range), sd_diff(p, q, range)}`.
  - `agreement::{tetrachoric(n00, n01, n10, n11), cohen_kappa(…), icc, average_icc, cramers_v(&[Vec<f64>]), prop_agree, prop_test(x1, n1, x2, n2), bvn_cdf, std_normal_cdf, std_normal_inverse_cdf}`.
  - **feature `core`** (`opinion` module): `opinion_mean` / `opinion_variance` / `opinion_spread` / `opinion_clusters(&W, tol)` / … over `W: ScalarOpinions`; `MetricsMechanism<W>::new().with(name, |w| …)` — records each entry via `record_metric` in `PostStep` (no calibration impact).

#### `socsim-survey`
- **Purpose:** generic, config-driven survey microdata recode engine (the schema *is* the extension API).
- **Engine-free?** Yes — **no `socsim-*` deps** (`csv`/`serde`). Library-only.
- **Git dep:**
  ```toml
  socsim-survey = { git = "https://github.com/akitenkrad/rs-social-simulation-tools", branch = "main", package = "socsim-survey" }
  ```
- **Feature flags:** `default = []` only. (The built-in ANES schemas have **moved to `socsim-datasets`** — there is no `anes` feature here.)
- **Key public API** (`crates/socsim-survey/src/…`):
  - `SurveySchema::builder(name)` → `.var(DemoVar)` · `.outcome(OutcomeMap)` · `.build()`; `var(key) -> Option<&DemoVar>`; `var_keys()`.
  - `DemoVar::valmap(key, column, ValMap)` / `::age(key, column, AgeBins)`; `ValMap::new(&[(i64, &'static str)])`; `AgeBins::new(&[(i64, i64, &'static str)])` / `::anes_decade()`; `OutcomeMap::new(column, &[(i64, &'static str)])`.
  - Free fns over `type Record = HashMap<String, String>`: `recode_row(rec, schema) -> RecodedRow`; `demo_label(rec, schema, var_key) -> Option<String>`; `actual_outcome(rec, schema) -> Option<&'static str>`; `estimate_distributions(records: &[Record], schema) -> Distributions`. Loader: `load_named_records(path)`.

#### `socsim-datasets`
- **Purpose:** the dataset-specific side of survey replications — schemas + machine-readable registry + optional acquisition. **Never vendors data.**
- **Engine-free?** Yes. **Depends-on:** `socsim-survey` only (for the schema types).
- **Git dep:**
  ```toml
  socsim-datasets = { git = "https://github.com/akitenkrad/rs-social-simulation-tools", branch = "main", package = "socsim-datasets" }
  # with downloader: features = ["acquire"]
  ```
- **Feature flags** (default = none): `acquire = ["dep:ureq", "dep:sha2", "dep:anyhow", "dep:csv", "dep:tempfile"]` — adds `fetch()` + `raw_to_csv()`.
- **Key public API** (`crates/socsim-datasets/src/…`):
  - Registry: `all() -> Vec<&'static DatasetMeta>` (ANES 2012/2016/2020 + CES 2022); `by_key(key) -> Option<&'static DatasetMeta>`.
  - `DatasetMeta { key, name, doi, source_url, citation, license, files }`; `DataFile { logical_name, source, sha256, expect_rows }`; `enum Source { Dataverse { base, file_id }, Url { url }, Manual { instructions_url } }`; `Source::download_url(&self) -> Option<String>` (`None` for `Manual`).
  - Schemas: `anes::{anes_2012, anes_2016, anes_2020}() -> SurveySchema` (+ `anes(year)`, `meta(year)`); `ces::ces_2022() -> SurveySchema` (+ `ces(year)`, `meta(year)`) — native CES codings (`race` 8-cat, `gender4` 4-cat, `ideo5` 5-point with code 6 "Not sure" unmapped) + a fixed-coded policy outcome `CC22_332a` (1=support, 2=oppose).
  - feature `acquire`: `acquire::fetch(meta: &DatasetMeta, opts: &FetchOpts) -> anyhow::Result<Vec<PathBuf>>` (download into gitignored `data/`, atomic-write, verify `sha256` + rows, skip cache hits); `acquire::raw_to_csv(input, output, delimiter, strip, expect_rows)`; `FetchOpts { dest, token, force }`.
  - License-gated files (raw ANES Time Series microdata) are declared `Source::Manual` and are **not** auto-downloaded. CES 2022 is CC0 1.0 (public domain) and is a `Source::Dataverse` — auto-downloadable, no account / terms.

#### `socsim-reproduce`
- **Purpose:** paper-anchor PASS/off reproduction harness — reads cached observations, classifies against the paper's reference values.
- **Engine-free?** Yes. **Depends-on:** `socsim-results` only (for CSV I/O).
- **Git dep:**
  ```toml
  socsim-reproduce = { git = "https://github.com/akitenkrad/rs-social-simulation-tools", branch = "main", package = "socsim-reproduce" }
  ```
- **Feature flags:** none.
- **Key public API** (`crates/socsim-reproduce/src/lib.rs`):
  - `Anchor { study, table_or_fig, condition, metric, paper_value: f64, tolerance: f64, upper_bound: bool, note }` (Copy).
  - `enum AnchorStatus { Pass, Off, NoData }` (`tag() -> "PASS" | "off" | "NO_DATA"`).
  - `compare_anchor(&Anchor, observed: Option<f64>) -> AnchorStatus`; `build_rows<F: Fn(&Anchor) -> Option<f64>>(anchors, observed) -> Vec<ReproduceRow>`.
  - `write_reproduce_summary(&[ReproduceRow], path)`; `write_paper_anchors(&[Anchor], path)`; `find_latest(results_root, predicate) -> io::Result<Option<PathBuf>>`.
  - **Ships no anchor values** — your replication supplies its own `&[Anchor]` slice + an observation-lookup closure.

#### `socsim-packs`
- **Purpose:** bundled CLI packs (world + mechanisms + registration + starter TOML).
- **Engine-free?** No. **Depends-on:** `socsim-core`/`-net`/`-config`/`-mechanisms`/`-metrics` (+ optional `-marl`/`-llm`).
- **Git dep:**
  ```toml
  socsim-packs = { git = "https://github.com/akitenkrad/rs-social-simulation-tools", branch = "main", package = "socsim-packs" }
  ```
- **Feature flags** (default on: `hr-lifecycle`, `opinion-dynamics`, `organizational-silence`; note: **no `pack-` prefix** at this crate level — the `pack-` prefix exists only in `socsim-cli`): `organizational-silence-llm` (adds the LLM voice-decision mechanism, pulls `socsim-llm`) · `marl` (pulls `socsim-marl`).
- **Key public API** (`crates/socsim-packs/src/…`):
  - `hr_lifecycle::{HrWorld::new(n_teams, team_size, ws_k, ws_beta, rng), HrLifecyclePack, Employee, Team, HR_LIFECYCLE_STARTER}` (`HrLifecyclePack: ModulePack<HrWorld>`).
  - `opinion::{OpinionWorld::new(&Params, seed), OpinionMetricsMechanism, register, OPINION_DYNAMICS_STARTER}`.
  - `organizational_silence::{SilenceWorld, OrganizationalSilencePack, Expression, Motive, …}`.

---

## Invariants & anti-patterns

**Determinism**
- **All randomness via `SimRng`** seeded from the run seed. Never introduce an ad-hoc RNG (no `Math.random`-style global, no `rand::thread_rng()` in model code). Derive independent streams with `SimRng::derive(&[…])` / `derive_seed(root, &[…])`.
- **RNG stream convention:** `derive_seed(root, &[0])` = world/network/agent init, `derive_seed(root, &[1])` = the engine/scheduler (passed to `SimulationBuilder::seed`). Reserve `&[2]`, `&[3]`, … for further independent streams.
- **Sorted `AgentId`.** `WorldState::agent_ids` should return IDs in sorted order; never let hash-map iteration order influence results.
- **LLM nondeterminism stays in `socsim-llm`.** A warm `PromptCache` + `LlmConfig::deterministic()` (temperature 0) is what makes an LLM run pseudo-deterministic on top of the seed-deterministic core. Keep model calls out of every phase except `Decision`.

**Engine-free boundaries** — do not regress these. `socsim-survey`, `socsim-results`, `socsim-reproduce`, `socsim-metrics` (default features), and `socsim-datasets` pull in **no engine crates**. Do not add `socsim-engine`/`-runner`/`-log` deps to these to keep LLM/survey replications lightweight. `socsim-config` is intentionally engine-free (avoids an engine↔config cycle).

**Keep paper-specific code OUT of socsim-\*.** Canonical, reusable things belong in socsim; anything with a *model-specific meaning* stays in the replication crate:
- A statistic with a paper-specific definition (e.g. a polarization measure defined as a product of extreme-opinion fractions, or a domain-event cascade aggregation) stays local — `socsim-metrics`' own guidance is *"keep paper-specific metrics local"* (reuse it only for canonical stats like `mean`/`variance`/`gini`).
- A survey recode is data, not code: encode it as a `SurveySchema`, not a hard-coded function.
- `socsim-reproduce` ships **no anchor values** on purpose — the paper's reference numbers live in the paper's crate as its own `&[Anchor]`.

**Phase semantics** — put logic in the phase whose meaning matches:
- `PreStep` — reset per-step counters / bookkeeping.
- `Environment` — exogenous shocks, resource replenishment, learning curves.
- `Decision` — agent decisions (incl. the **single** allowed home for LLM `complete` calls).
- `Interaction` — peer effects, network diffusion, contagion.
- `Reward` — compute/apply rewards, record aggregate metrics.
- `PostStep` — **read-only recording / convergence checks** (e.g. `MetricsMechanism` records here; `ctx.request_stop()` on convergence). Treat `PostStep` as the observation point, not a place to mutate dynamics.
- Snapshot (synchronous) vs asynchronous updates change the dynamics — snapshot the eligible set at step start for synchronous semantics; act on whoever is currently eligible for asynchronous. Choose deliberately.

**`socsim-llm` specifics**
- Blank (whitespace-only) completions are **rejected by default** as `LlmError::EmptyResponse { endpoint, model }` (a reasoning model can spend its whole budget on a hidden trace). Opt out per call via `LlmConfig::allow_blank()`.
- Token-level logprobs come from `complete_with_logprobs` (default impl returns `LlmError::Unsupported`; live backends override it).
- Prefer the harness (`build_live_client_from_settings` / `wrap_client` / `llm_config`) over a per-model `llm.rs`.

**`socsim-datasets` specifics**
- The crate **never vendors data**. The raw ANES Time-Series microdata is license-gated (needs a free electionstudies.org account + data-use agreement), so it is `Source::Manual` and is not auto-downloaded. CES 2022 Common Content is CC0 1.0 (public domain) on the Harvard Dataverse, so it is `Source::Dataverse` and **is** auto-downloaded. `fetch()` downloads only non-`Manual` sources into a consuming repo's gitignored `data/`.

---

## Where to go next

- **`docs/ai/recipes.md`** — copy-paste `Cargo.toml` + `main.rs` templates per replication shape (engine-free LLM-social, spatial/ABM library mode, survey + reproduce).
- **`cargo doc -p <crate> --open`** — the full, authoritative API for any crate above.
- **Human docs** (prose + rationale + worked examples): the index in the repo `README.md`, then `docs/architecture.md`, `docs/library.md`, `docs/design.md`, `docs/packs.md`, `docs/mechanisms.md`, `docs/cli.md`.
- **Worked examples in-tree:** `crates/socsim-engine/examples/{engine_only,opinion_dynamics,cellular_automata,snapshot_resume}.rs`; `crates/socsim-packs/examples/hr_baseline.rs`.

---

## Keeping this current

This map is a curated index, so it drifts unless maintained. Adding a **crate**, a **feature flag**, or a **mechanism** to socsim must update **both** this capability map and `docs/ai/recipes.md` (new crate → a per-crate digest row + a capability-table row; new feature → its feature-flags entry; new mechanism → a catalog row + the relevant capability bound). This extends the repo's existing practice that every new mechanism ships with matching docs.
