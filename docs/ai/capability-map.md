# socsim Capability Map (for AI coding agents)

`socsim` (`rs-social-simulation-tools`) is a composable, **seed-deterministic** agent-based social-simulation platform in Rust, organised as a **nineteen-crate** workspace. This page is the curated **capability map** for an AI agent building an implementation — usually an academic-paper replication — **on top of** socsim consumed as a git dependency.

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
| LLM-agent decisions | `socsim-llm` | `socsim_llm::{LlmClient, LlmConfig, LlmResponse, build_live_client_from_settings, build_live_backend, wrap_client, LlmSettings, LiveClient, extract_first_choice, parse_json_object, MetadataCollector}` | Confine to `Decision` phase; `features=["live"]` for networking. In a normal-form game use `socsim-games` feature `llm` instead of a hand-written mechanism. |
| Survey microdata recode | `socsim-survey` | `socsim_survey::{SurveySchema, DemoVar, ValMap, AgeBins, OutcomeMap, recode_row, demo_label, actual_outcome, estimate_distributions}` | Generic engine only — schemas live in `socsim-datasets`. |
| Dataset schemas + registry + download | `socsim-datasets` | `socsim_datasets::{all, by_key, DatasetMeta, DataFile, Source}`; `socsim_datasets::anes::{anes_2012, anes_2016, anes_2020}`; `socsim_datasets::ces::ces_2022`; `acquire::{fetch, raw_to_csv}` (feature) | Never vendors data; ANES is license-gated (`Source::Manual`); CES 2022 is CC0/Dataverse (auto). |
| Reproduction (paper-anchor PASS/off) | `socsim-reproduce` | `socsim_reproduce::{Anchor, AnchorStatus, compare_anchor, build_rows, write_reproduce_summary, write_paper_anchors, find_latest}` | Ships mechanics, **no** anchor values — you supply `&[Anchor]` + lookup. |
| Reusable metrics (stats / distribution / agreement) | `socsim-metrics` | `socsim_metrics::stats::{mean, variance, gini, …}`, `::distribution::{kl_divergence, chi_square_homogeneity, …}`, `::agreement::{cohen_kappa, tetrachoric, …}`, `::opinion::{MetricsMechanism, …}` (feature `core`) | Default build = zero socsim deps. Keep paper-specific metrics local. |
| Output dirs + CSV/JSON + `latest` symlink | `socsim-results` | `socsim_results::{timestamp, create_run_dir, refresh_latest_symlink, write_csv, write_json, WriteError}` | Leaf crate; the `results/<ts>/` convention without a `Recorder`. |
| Metric / event logging (Recorders) | `socsim-log` | `socsim_log::{InMemoryRecorder, JsonlRecorder, CsvRecorder}` | Only needed when you use a `Recorder`; library models can skip it (`NullRecorder` default). |
| Normal-form game simulation (stage game + decision rules + matching + payoffs) and finite-game solvers (Nash / dominance / minimax / logit QRE) | `socsim-games` (features `core` / `engine`; `llm` / `live` for LLM agents; solvers in the default build) | `socsim_games::{MatrixGame, StageGame, NormalFormGame, FiniteGame, DecisionRule, RuleBook, Fixed, UniformRandom, TitForTat, OutcomeRule, NormalFormOutcome, Matching, FixedPlayers, ExperimentState, Slot, GameWorld, build_world, build_world_with, KindCatalog, assemble, PlayOptions, OnInvalidAction, PayoffAggregation}`; LLM: `socsim_games::llm::{LlmRuleBook, LlmPolicy, LlmSetup, WithLlm, ResponseStore, assemble_with_llm}`; games: `socsim_games::catalog::{PrisonersDilemma, BattleOfSexes, MatchingPennies, StagHunt, RockPaperScissors}`; solvers: `socsim_games::MixedStrategy`, `socsim_games::solve::{pure_nash, iterated_strict_dominance, support_enumeration, lemke_howson, zero_sum_minimax, maximin_security_level, logit_qre, tolerance}` | Library-only; default build = zero socsim deps (solvers add only `microlp`); `socsim-llm` only with feature `llm`. Rule state lives in the world → exact snapshot resume. Every returned equilibrium is regret-verified against `tol`. |
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
- **Feature flags** (default = **none**, no networking): `ollama` / `openai` (each adds `ureq`) · `live = ["ollama", "openai"]` (enables `build_live_client*`, `build_live_backend` + the fallback stack).
- **Key public API** (`crates/socsim-llm/src/…`):
  - `trait LlmClient` — `model(&self) -> &str`; `endpoint(&self) -> &str`; `complete(&self, prompt: &str, config: &LlmConfig) -> Result<LlmResponse, LlmError>`; `complete_with_logprobs(&self, prompt, config)` (default impl → `LlmError::Unsupported`; overridden by live backends).
  - `LlmConfig` fields: `temperature: f32`, `seed: u64`, `max_tokens: Option<u32>`, `system: Option<String>`, `omit_seed: bool`, `allow_blank: bool`, `top_logprobs: Option<u32>`. Ctors: `deterministic()` (temp 0, fixed seed) / `sampling(temperature)`; builders `with_seed` / `with_temperature` / `with_max_tokens` / `with_system` / `omit_seed` / `allow_blank` / `with_top_logprobs`.
  - `LlmResponse { text: String, metadata: CallMetadata, logprobs: Option<Vec<TokenLogprob>> }`; `TokenLogprob { token, bytes, logprob }`.
  - `enum LlmError { Transport, Backend, Config, EmptyResponse { endpoint, model }, AllBackendsFailed, Unsupported { endpoint, operation } }`. **Blank (whitespace-only) completions are rejected by default** as `EmptyResponse` — opt out per call via `LlmConfig::allow_blank()`.
  - Caching: `CachingClient<C>::new(inner, cache)` (its `complete` takes `&mut self`); `SharedCachingClient<C>` (interior-mutable, itself `impl LlmClient`, so injectable as `&dyn LlmClient`); `PromptCache::in_memory()` / `::open(path)`; free `cache_key(prompt, model) -> String`. ⚠️ The key is **only `prompt` + `model`** — it does **not** include `system`, `seed` or `temperature` (nor `max_tokens` / `think`), so two calls that differ only in those share one cache entry. A hit also returns the text only (`thinking: None`, no usage). Keep those fixed per cache, or use a store with a complete key (`socsim_games::llm::ResponseStore` / `CacheKeyV1` for game simulations).
  - Harness (prefer this over a hand-rolled `llm.rs`): `LlmSettings { temperature: f32, seed: u64, cache_path: Option<String> }`; `type LiveClient = CachingClient<Box<dyn LlmClient>>`; `build_live_client_from_settings(&LlmSettings) -> Result<LiveClient, LlmError>` (feature `live`); `wrap_client(backend, PromptCache) -> LiveClient` (inject a mock/test backend); `llm_config(&LlmSettings) -> LlmConfig`. Shared variants: `wrap_client_shared`, `build_shared_live_client_from_settings`. Lower-level: `build_live_client(cache_path: Option<&Path>)`.
  - Cache-free backend: `build_live_backend() -> Result<Box<dyn LlmClient>, LlmError>` (feature `live`) — the same Ollama-first → OpenAI-fallback stack as `build_live_client`, **without** a `PromptCache`; recording responses is the caller's job. Construction is lazy (no network until `complete`). Use it when something else owns the cache (e.g. `socsim_games::llm::live_backend_factory()`).
  - Parsing (`socsim_llm::parse`, always compiled, no game concepts):
    - `extract_first_choice<'a>(text: &str, vocab: &[(&'a str, &[&str])]) -> Option<&'a str>` (case-insensitive, word-boundary, earliest-then-longest match; assumes an ASCII label vocabulary).
    - `single_fenced_payload(raw) -> Option<String>` — the inner text of **exactly one** Markdown fence (opening line ```` ``` ```` or ```` ```json ```` (case-insensitive), closing ```` ``` ````, ≥ 1 inner line; surrounding whitespace ignored). `None` for prose around the fence, an unclosed fence, another language tag, or a nested / second fence. `is_single_fenced(raw) -> bool` applies the same rules.
    - `json_payload_text(raw) -> (String, bool)` — the fenced inner text and `true`, else `raw.trim()` and `false` (malformed fences are not stripped).
    - `parse_json_object(raw) -> Result<JsonObjectPayload { object: serde_json::Map, fenced }, JsonPayloadError>` — `JsonPayloadError::InvalidJson(String)` / `NotAnObject` (arrays, scalars, `null`).
    - `parse_integer_value(value: Option<&serde_json::Value>) -> Option<(u64, bool)>` — a non-negative JSON integer → `(n, false)`; a string of ASCII digits only (trimmed, leading zeros ok) → `(n, true)`. `None` for missing, negative, fractional, `null`, signed / decimal strings (`"3.0"`), non-ASCII digits, or overflow of `u64`. Narrower ranges (e.g. `u32::try_from`) are the caller's job.
    - `json_field_was_string(raw, field) -> bool` — whether the object's `field` was encoded as a JSON string (fence stripped first; invalid / non-object / missing → `false`).
  - Metadata: `MetadataCollector::new()` → `record(CallMetadata)` → `summary() -> RunMetadata` (persist via `socsim_results::write_json`). `CallMetadata { model, endpoint, temperature, seed, cache_hit, usage }`.
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

#### `socsim-games`
- **Purpose:** normal-form (strategic-form) game simulations: stage games, per-agent decision rules, matching protocols, outcome (payoff) rules, and an experiment-state slot `X`, assembled into a socsim `Simulation`. Independently of the simulation: built-in classic games and solvers for finite games (pure Nash, iterated strict dominance, support enumeration, Lemke–Howson, zero-sum minimax / maximin, logit QRE).
- **Engine-free?** Default build: yes — **zero `socsim-*` deps** (the game traits, `MatrixGame`, `catalog`, `mixed` and `solve`; the solvers add one external dependency, the pure-Rust LP crate `microlp`). Feature `core` → `socsim-core`; feature `engine` → `socsim-core` + `socsim-engine`; feature `llm` → `socsim-llm` (default features, no networking) + `sha2`; feature `live` → `socsim-llm/live`. `core` / `engine` never depend on `socsim-llm`; the crate never depends on `socsim-config` or `socsim-runner`. Library-only (no `ModulePack`, not in the binary).
- **Git dep:**
  ```toml
  socsim-games = { git = "https://github.com/akitenkrad/rs-social-simulation-tools", branch = "main", package = "socsim-games", features = ["engine"] }
  ```
- **Feature flags:** default = none (game traits, `MatrixGame`, `catalog`, `mixed`, `solve`) · `core` (world, rules, outcome, matching, views, `build_world`, `build_world_with`, `KindCatalog`; adds `serde_json`) · `engine` (implies `core`; adds `assemble`) · `llm` (implies `core`; the `socsim_games::llm` module — LLM rules, response store, executor, manifest; `game_llm_decide` / `assemble_with_llm` / `verify_snapshot` additionally need `engine`; test with `--features llm,engine`) · `live` (implies `llm`; adds `llm::live_backend_factory()` wrapping `socsim_llm::build_live_backend`).
- **Key public API** (`crates/socsim-games/src/…`):
  - Games: `trait StageGame { type Action; fn n_roles(&self) -> usize; fn check_action(&self, role, &Action) -> Result<(), ActionError>; }`; `trait NormalFormGame: StageGame { fn payoff(&self, role, profile: &[Action]) -> f64; fn payoffs_into(&self, profile, out: &mut [f64]); }`; `trait FiniteGame: NormalFormGame<Action = usize> { fn n_actions(&self, role) -> usize; fn action_label(..) -> Cow<str>; fn is_symmetric(&self) -> bool; }`. `MatrixGame::{new(n_actions, payoffs), bimatrix(&a, &b), zero_sum(&a), with_labels(labels)}` → `Result<_, MatrixError>` (`Shape` / `TooLarge` / `NonFinite` / `Labels`).
  - Rules: `trait DecisionRule<G, X> { fn name(&self) -> &str; fn init_state(&self, &RuleInitContext, &G) -> Result<X::PolicyState, RuleError>; fn decide(&self, state: &mut X::PolicyState, &DecisionContext<G, X>, &mut SimRng) -> Result<G::Action, RuleError>; fn learn_step(&self, state, &LearnContext<G, X>) -> Result<(), RuleError> /* default no-op */; }`. Built-ins: `Fixed::new(action)` (needs `Slot<Stateless>`), `UniformRandom` (`FiniteGame`, `Slot<Stateless>`), `TitForTat::new(coop)` (`FiniteGame`, 2 roles × 2 actions, `Slot<TftState>`). `RuleBook::new()` + `register(kind: &str, Arc<dyn DecisionRule<G, X>>)`; kind ids follow sorted kind-name order. `RuleInitContext { agent, kind, generation, init_seed }`.
  - Experiment state: `trait ExperimentState { type PolicyState: Clone + Send + Sync + Debug; type View<'a>; fn view_for(&self, me: AgentId) -> Self::View<'_>; }`; `impl ExperimentState for ()` (`PolicyState = BuiltinPolicyState`, `View = ()`). `trait Slot<T> { slot, slot_mut, from_slot }`; `BuiltinPolicyState::{Stateless, TitForTat}` implements `Slot<Stateless>` + `Slot<TftState>`.
  - Outcome: `trait OutcomeRule<G, X> { fn name(&self) -> &str; fn outcome(&self, &OutcomeContext<G, X>, EncounterRef, actions: &[G::Action], payoffs: &mut [f64]) -> socsim_core::Result<()>; }`; built-in `NormalFormOutcome` (`G: NormalFormGame`). `OutcomeContext::{t, game, x, held(agent), topology}`.
  - Matching: `trait Matching<G, X> { fn name; fn check(&self, &MatchContext) -> Result<(), GameError>; fn init_state(..) -> MatchingState /* None */; fn encounters_into(&self, &MatchContext, &mut MatchingState, &mut SimRng, &mut ArenaWriter<G::Action>) -> socsim_core::Result<()>; }`; `ArenaWriter::push(&[AgentId], Credit) -> Result<EncounterId>`; built-in `FixedPlayers::pairs(&[(u64, u64)])` / `FixedPlayers::new(Vec<[AgentId; 2]>)`.
  - Views: `DecisionContext::{t, me, unit, role, encounter, opponents, my_history, opponent_history(id) /* None unless id is a current opponent */, game, view, omniscient}`; `LearnContext::{t, me, record, my_history, outcomes, game, view, omniscient}`; `OmniscientContext::{ids, held(agent), history(agent), x}`.
  - World: `GameWorld<G, X = ()>` (`WorldState`; serde when `G`, `G::Action`, `X`, `X::PolicyState` are) — accessors `game()`, `x()`, `x_mut()`, `held()`, `policies()` (`PolicyTable::{state(agent), kind(agent)}`), `plays()` (`PlayArena`), `payoffs()` / `n_played()` (**this step only**), `history(agent)` (`HistoryView<StepRecord { action: Option<A>, payoff, n_played }>`).
  - Kinds: `KindCatalog::new().with_rules(&RuleBook)?.with_llm(&LlmRuleBook)?` (feature `llm` for `with_llm`) / `KindCatalog::from_rules(&rules)` — the single registry of every kind across drivers; a kind in two books → `RuleError::DuplicateKind`; kind ids = sorted union of names; `kinds()`, `kind_id(kind) -> Option<RuleKindId>`, `driver_name(kind) -> Option<&'static str>` (`"game_decide"` / `"game_llm_decide"`). `build_world_with(game, n_agents, x, initial_held, &catalog, &assign, history_len, t_max, seed)`; `build_world(.., &rules, ..)` ≡ `build_world_with(.., &KindCatalog::from_rules(&rules), ..)`. `assemble` rejects a world whose kind list ≠ its `RuleBook`'s (`GameError::InvalidKinds { expected, actual }`); `game_decide` skips kinds it does not own.
  - Assembly: `build_world(game, n_agents, x, InitialActions::{Same(a), PerAgent(vec)}, &rules, &Assignment::{Ordered, Shuffled}(vec![AssignGroup { kind: AssignKind::{Rule(String), Held}, count }]), history_len, t_max, seed) -> Result<GameWorld<G, X>, GameError>`; `assemble(world, matching, rules, outcome, PlayOptions, seed) -> Result<SimulationBuilder<GameWorld<G, X>>, GameError>` (feature `engine`; sets `.seed(derive_seed(seed, &[1]))`, registers `game_match`, `game_decide`, `game_play` in that order). `PlayOptions::new().payoff_aggregation(PayoffAggregation::{Sum, Mean}).on_invalid_action(OnInvalidAction::{Stop, Fallback(BTreeMap<String, A>)})`.
  - Mechanisms (registered by `assemble`): `game_match` (PreStep: clear arena/`payoff`/`n_played`; Decision: `Matching::encounters_into`), `game_decide` = `RuleDriver` (Decision: `decide` + `check_action` per seated agent in activation order; Reward: `learn_step` per rule-driven agent in sorted `AgentId` order), `game_play` (Interaction: all seats filled → `OutcomeRule` → `PayoffAggregation` → append `StepRecord` per agent). Event `game.invalid_action` (`{agent, kind, role, action}`) under `Fallback`.
  - LLM (feature `llm`, module `socsim_games::llm`; see `docs/games.md` § LLM decisions and `docs/mechanisms/game-llm-decide.md`):
    - `trait LlmDecisionRule<G, X> { name; init_state; prepare(&self, &X::PolicyState, &DecisionContext) -> Result<DecisionRequest, RuleError> /* pure, no RNG */; resolve(&self, &mut X::PolicyState, &DecisionRequest, &LlmReply, &mut ResolveContext<G>) -> Resolution<G::Action>; learn_step /* no LLM call */ }`; `DecisionRequest { system, prompt, attempt }` (`#[non_exhaustive]`); `LlmReply { text, thinking, usage, cache_hit }`; `ResolveContext { game, role, previous, fallback_rng, t }`; `Resolution::{Accept { action, flags }, Retry(DecisionRequest), GiveUp { action, fallback: FallbackKind, violation }}`. `LlmRuleBook::new()` + `register(kind, Arc<dyn LlmDecisionRule<G, X>>)`.
    - `LlmPolicy::new(obs, renderer, parser).with_retry(RetryProtocol { max_retries /* 1 */, fallback: Fallback::{Previous /* default */, UniformRandom, Specified(a)} }).with_max_turns(n /* 16 */)` — needs `X::PolicyState: Slot<LlmConversation>` (`BuiltinPolicyState::Llm` under `llm`). Observations: `OwnHistoryObs::new(window)` → `OwnHistory`, `CountsObs` → `ActionCounts` (no `t`, so identical situations dedupe). Renderers: `MarkdownTableRenderer` (with `OwnHistoryObs`), `CountsRenderer` (with `CountsObs`); trait `GameRenderer<G, Obs> { rules, turn, feedback }`. Parsers: `LabelParser::{strict(), lenient()}.require_reason(bool).with_synonyms(label, &[..])` (lenient ⇒ every label must be ASCII `[A-Za-z0-9_-]+`, checked at `init_state` → `RuleError::Unsupported`); `NumberParser::new(field).bounds(lo, hi).out_of_range(OutOfRange::{Violation /* default */, Clamp}).parse_number(raw)`; `ParseFlags { fenced, value_was_string, clamped, lenient_match }`.
    - Store: `ResponseStore::in_memory(id, mode)` / `::open(dir, mode, run_tag, seed)` (`dir/store_id`, read-only `dir/base.jsonl`, own shard `dir/shards/<run_tag>-<seed>.jsonl`); `ReplayMode::{ReplayOnly /* miss = error */, RecordMisses /* miss → backend → own shard */, Live /* store unused */}`; `ResponseStore::merge_shards(dir) -> MergeReport` (explicit; never automatic; changes `base_digest`); key `CacheKeyV1 { version, backend: BackendIdentity { model, endpoint }, system, prompt, temperature_bits, seed, omit_seed, max_tokens, think, allow_blank }` (FNV-1a of canonical JSON; full key compared on hit); `StoreError::{Io, Json, Conflict, KeyCollision}`.
    - Execution: `type BackendFactory = Arc<dyn Fn() -> Result<Box<dyn LlmClient>, LlmError> + Send + Sync>` (cache-free; built inside the thread that uses it); `LlmExecutor::{Sequential /* default */, Threaded { workers }}`; `ExecutorOptions { timeout: 120 s /* Threaded only */, max_backend_retries: 3, backoff_base: 1 s, queue_per_worker: 2 }` (`#[non_exhaustive]` — mutate a `default()`).
    - Manifest: `WithLlm<X> { inner, llm: LlmReplayManifest { store_id, base_digest, mode, backend } }` (`LlmReplayManifest::for_store(&store, identity)`), `trait HasLlmManifest`; `verify_snapshot(&Snapshot<GameWorld<G, X>>, &store, &identity) -> Result<(), GameError>`.
    - Assembly: `LlmSetup::new(llm_rules, store, backend, identity).executor(..).options(..).config(LlmConfig /* default deterministic(); seed overwritten */)`; `assemble_with_llm(world, matching, rules, llm_setup, outcome, opts, seed)` (feature `engine`; `X: HasLlmManifest`; registers `game_match` → `game_decide` → `game_llm_decide` → `game_play`; `OnInvalidAction::Fallback` covers `RuleBook` kinds only).
    - Recording: row `"game"` with `llm.{parse_fail,retry,clamp,value_was_string,lenient,cache_hit,giveup}_rate`; event `game.llm_call` per attempt (`agent, kind, unit, unit_index, attempt, resolution, system, prompt, reply, thinking, seed, usage, cache_hit, deduplicated, key, flags`).
  - Built-in games (default build, `socsim_games::catalog`; all `FiniteGame` with `type Action = usize`, 2 roles, ASCII labels, `to_matrix() -> MatrixGame`; serde `Deserialize` re-runs the constructor checks): `PrisonersDilemma::new(T, R, P, S)` (`T > R > P >= S`, `P == S` allowed; **no `Default`**) / `::weak(b)` = `new(b, 1, 0, 0)` (needs `b > 1`), actions `C`,`D`; `BattleOfSexes::new(a, b)` (`B`,`V`; default `a = [[2,-1],[-1,1]]`, `b = [[1,-1],[-1,2]]`); `MatchingPennies::new(a, b)` (`H`,`T`; `b == -a`, no pure NE; default `a = [[-1,1],[1,-1]]`, i.e. role 0 wins on a mismatch); `StagHunt::new(a, b)` (`stag`,`hare`; two strict NE, `(stag, stag)` payoff-dominant; default `a = [[9,0],[8,7]]`, `b = [[9,8],[0,7]]`); `RockPaperScissors::new(win, loss)` (`R`,`P`,`S`; `win, loss > 0`; default 1, 1 = antisymmetric zero-sum; `win != loss` is not zero-sum). Cooperative side (`C`, `stag`) is action 0. Errors `CatalogError::{NonFinite, Condition(String)}`.
  - Mixed strategies (default build, `socsim_games::mixed`; re-exported `socsim_games::MixedStrategy`): `MixedStrategy::{new(probs) /* validates: non-empty, finite, >= 0, |sum - 1| <= 1e-9; not renormalised */, uniform(n), pure(n, action), probs(), len()}`; `expected_payoff(&g, role, &[MixedStrategy])`, `action_payoffs(&g, role, &profile) -> Vec<f64>`, `regrets(&g, &profile) -> Vec<f64>`, `regret(&g, &profile) -> f64` (n roles; `MixedError::{NotADistribution, Dimension, RoleCount}`). **Solver-side only — a `MixedStrategy` is never a `G::Action` in a simulation.**
  - Solvers (default build, `socsim_games::solve`; all take `&G: FiniteGame`): `tolerance(&g) -> f64` = `1e-10 * max(1, payoff range over all roles and profiles)`. Every solver first rejects 0 roles, an empty action set, > 10^6 profiles (`TooLarge`) and non-finite payoffs. Every equilibrium-returning solver recomputes the regret on the original game and returns only if `regret <= tol`. `Equilibrium { strategies: Vec<MixedStrategy>, payoffs, regret }` + `is_pure()`.

    | Function | Players | Returns | Limits / guarantees |
    |---|---|---|---|
    | `pure_nash(&g)` | n | `Vec<PureNash { profile, payoffs, regret }>` — all pure NE, lexicographic profile order (role 0 most significant) | exhaustive; ≤ 10^6 profiles |
    | `iterated_strict_dominance(&g)` | n | `DominanceResult { surviving: Vec<Vec<usize>>, eliminated: Vec<Elimination { round /* 1-based */, role, action }> }` | simultaneous rounds; **pure strict dominance only** (margin > `tol`); no weak dominance (order-dependent, can drop NE), no domination by mixed strategies (would need an LP per action) |
    | `support_enumeration(&g)` | 2 | `EquilibriumSet { equilibria, findings: Vec<SupportFinding::{Degenerate, Component { base, directions }, Inconclusive}> }`; `is_complete()` = no findings | all NE for nondegenerate games; degenerate / ill-conditioned (cond > 1e12) supports go to `findings`, never fabricated (`Component.base` is verified and also in `equilibria`); `(2^m-1)(2^n-1) <= 2^20` (10×10 ok, 11×10 → `TooLarge { profiles: <pair count> }`); f64, no exact rationals |
    | `lemke_howson(&g, &LemkeHowsonOptions)` | 2 | `LemkeHowsonResult { equilibrium, pivots }` — exactly one NE | `dropped_label` (default 0; `0..m` role 0, `m..m+n` role 1; `>= m+n` → `InvalidLabel`) decides which NE; may return a pure NE when a mixed one exists; per-player positive payoff shift; lexicographic min-ratio test (unresolvable tie → `Lp(Internal)`); `max_pivots` default 100_000 → `MaxPivots { limit }` |
    | `zero_sum_minimax(&g)` | 2, zero-sum (`\|u0+u1\| <= tol` else `NotZeroSum`) | `MinimaxSolution { value /* role 0 view */, row, col, duality_gap, regret }` | primal + dual LP via `microlp`; checks duality gap, regret, value residuals ≤ `tol` else `VerificationFailed { duality_gap, regret }`; LP infeasible / unbounded / interrupted / not optimal / backend error → `Lp(LpFailure::{Infeasible, Unbounded, Interrupted, NotOptimal, Internal})` |
    | `maximin_security_level(&g, role)` | 2 (any payoffs) | `SecurityLevel { level, strategy, duality_gap }` | same LP pair + checks on `role`'s own matrix; `role >= 2` → `InvalidOptions`; a guarantee, not an equilibrium strategy |
    | `logit_qre(&g, &QreOptions)` | n | `QreSolution { lambda, strategies, residual, regrets, iterations }` | damped fixed-point iteration from `init` (default uniform); defaults `damping 0.1`, `tol 1e-12`, `max_iter 100_000`; returns only if recomputed residual ≤ `tol`, else `NotConverged { residual, iterations }`; **local, not principal-branch tracking** (different `init` → possibly different QRE); for large λ lower damping or step λ up passing the previous solution as `init`; `lambda == 0` → uniform; ordinary regret ≤ `ln\|S_i\|/λ` at an exact QRE (reported, not enforced) |

    `QreOptions::new(lambda).with_init(..).with_damping(..).with_tol(..).with_max_iter(..)`; `LemkeHowsonOptions::default().with_dropped_label(..).with_max_pivots(..)` (both `#[non_exhaustive]`). Errors: `SolveError::{TooLarge, UnsupportedRoles, EmptyActions, NonFinitePayoff, NotZeroSum, InvalidLabel, MaxPivots, Lp(LpFailure), VerificationFailed, NotConverged, InvalidOptions, Mixed(MixedError)}`. See `docs/games.md` § Solving finite games.
  - Errors: `GameError::{Rule, Action, Matrix, AgentCount, InvalidHistoryLength, UnsupportedRoles, InvalidMatching, InvalidKinds, ManifestMismatch}`; `RuleError::{MissingSlot, UnknownKind, DuplicateKind, InvalidAction, Unsupported, Backend}`.
- **Limits today:** encounters must have exactly **two roles** (`assemble` / `assemble_with_llm` → `UnsupportedRoles`); only `Credit::All` is played; `MatchingState` has only `None`; `DecisionUnit::Step` only. LLM rules for continuous games, LLM rules in scenario TOML / the CLI, and replaying past turns into the prompt are not provided. Solvers: `support_enumeration` / `lemke_howson` / `zero_sum_minimax` / `maximin_security_level` are 2-player only; `support_enumeration` is capped at 2^20 support pairs (≤ 10×10) and uses f64 (no exact rational arithmetic); dominance is pure-strict only; `logit_qre` finds a local fixed point at one λ (no homotopy / branch tracking); no solver for extensive-form or continuous games. Example: `crates/socsim-games/examples/ipd_pair.rs`; LLM usage: `crates/socsim-games/tests/llm_*.rs`.

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
- **RNG stream convention:** `derive_seed(root, &[0])` = world/network/agent init, `derive_seed(root, &[1])` = the engine/scheduler (passed to `SimulationBuilder::seed`). Reserve `&[2]`, `&[3]`, … for further independent streams. **In a `socsim-games` simulation `&[2]` (`decision_root`) and `&[3, agent, generation]` (rule-state init) are taken** — start your own labels at `&[4]`.
- **Sorted `AgentId`.** `WorldState::agent_ids` should return IDs in sorted order; never let hash-map iteration order influence results.
- **LLM nondeterminism stays in `socsim-llm`.** A warm `PromptCache` + `LlmConfig::deterministic()` (temperature 0) is what makes an LLM run pseudo-deterministic on top of the seed-deterministic core. Keep model calls out of every phase except `Decision`. In `socsim-games` the same role is played by `ResponseStore` (`ReplayOnly` / `RecordMisses`) plus the manifest in `WithLlm<X>`.

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
- **`cache_key` covers `prompt` + `model` only.** `PromptCache` does not distinguish `system`, `seed`, `temperature`, `max_tokens` or `think`, and a hit drops `thinking` / usage. Do not vary those across calls that share one cache; for game simulations use `socsim_games::llm::ResponseStore`, whose `CacheKeyV1` includes them.
- **Parse with the shared helpers** (`parse_json_object`, `single_fenced_payload`, `parse_integer_value`, `json_field_was_string`, `extract_first_choice`) instead of re-implementing fence stripping or digit-string checks per replication.
- `build_live_backend` has **no cache**: wrap it (`wrap_client`) or record responses yourself; never put a caching client behind `socsim_games::llm::BackendFactory`.

**`socsim-games` specifics**
- **State needed to resume lives in the world; mechanisms are stateless.** Every agent's rule state is in `GameWorld`'s `PolicyTable`, the matching state in `MatchingState`, and everything model-specific in `X`. `game_match` / `game_decide` / `game_play`, the `RuleBook`, the `Matching` and the `OutcomeRule` are code re-supplied on rebuild. Never keep mutable per-run data in a rule, matching or outcome struct (`decide`/`learn_step` take `&self` for this reason) — it would not be in the snapshot and a resumed run would diverge. Keep `X` and `PolicyState` serde-restorable (no clients, handles, caches).
- **RNG streams `[2]` and `[3]` are reserved** (`derive_seed(seed, &[2])` = `decision_root`, `derive_seed(seed, &[3, agent, generation])` = `RuleInitContext::init_seed`), on top of the standard `[0]` (world init, `Assignment::Shuffled`) and `[1]` (engine; `decide` and `Matching` draw from the step RNG). Pass the same `seed` to `build_world` and `assemble`.
- **`DecisionRule` ≠ `socsim_marl::Policy`.** A `DecisionRule<G, X>` is a game-layer strategy over `G::Action` whose per-agent state is stored in the world and which is driven by `game_decide`; `socsim_marl::Policy` is a neural policy over an `f32` observation vector driven by `PolicyMechanism`. Do not wrap one as the other implicitly.
- **The core does not depend on `socsim-llm`** (nor `socsim-config` / `socsim-runner`). `socsim-llm` enters only through the optional `llm` feature; do not add it (or those other crates) to `socsim-games`' `core` / `engine` features, and do not reference `socsim_games::llm` types from `core` code (`GameWorld` knows nothing about LLMs — the manifest lives in `X` via `WithLlm<X>`).
- **LLM invariants (`llm` feature).**
  - The model is called **only inside `game_llm_decide`'s Decision phase** (`LlmDriver`). `prepare` is a pure render (no RNG, read-only state); `learn_step` (Reward) never calls the model.
  - **Seeds come from the decision unit, never from the engine stream.** Generation seed: `derive_seed(decision_root, &[0])` at temperature 0, else `derive_seed(decision_root, &[t, agent, 0, 0, attempt])`; fallback RNG: `derive_seed(decision_root, &[t, agent, 0, 0, u64::MAX])`. The driver does not touch `ctx.rng`, so activation order and worker count cannot change results (`Sequential` ≡ `Threaded`).
  - **Only the main thread writes to the `ResponseStore`.** Worker threads build their own cache-free backend from the `BackendFactory` and return replies; resolution happens in decision order.
  - **Every kind belongs to exactly one driver** (`KindCatalog`); build worlds with LLM kinds via `build_world_with` and assemble them via `assemble_with_llm` (plain `assemble` → `InvalidKinds`).
  - **Resume is exact only for `ReplayOnly`, or `RecordMisses` when every post-resume request hits the store** — never for `Live`. Check with `verify_snapshot` before `restore`; the driver also re-checks the manifest on every Decision, so a restore into an already-running simulation is caught. `merge_shards` is never called for you, and it changes `base_digest`.
- **Information structure:** a rule's default view is limited to opponents in **its own encounter** (`opponents()`, `opponent_history(id)` → `None` for anyone else) plus `X::view_for(me)`. The whole world is readable **only** through `ctx.omniscient()`, which drops the guarantee (and, during Decision, `held` may reflect activation order). Put any agent-visible extra information behind `view_for`.
- **User-defined matching state goes in `X`.** `MatchingState` is a closed enum (`None`); a custom `Matching` reads its bookkeeping from `ctx.x()` and your own mechanism updates it via `GameWorld::x_mut()`.
- Mechanisms you add after `assemble` run after the game mechanisms within each phase; a Reward-phase mechanism sees this step's `payoffs()`. `payoffs()` / `n_played()` are reset every PreStep — accumulate across steps yourself.

**`socsim-datasets` specifics**
- The crate **never vendors data**. The raw ANES Time-Series microdata is license-gated (needs a free electionstudies.org account + data-use agreement), so it is `Source::Manual` and is not auto-downloaded. CES 2022 Common Content is CC0 1.0 (public domain) on the Harvard Dataverse, so it is `Source::Dataverse` and **is** auto-downloaded. `fetch()` downloads only non-`Manual` sources into a consuming repo's gitignored `data/`.

---

## Where to go next

- **`docs/ai/recipes.md`** — copy-paste `Cargo.toml` + `main.rs` templates per replication shape (engine-free LLM-social, spatial/ABM library mode, survey + reproduce).
- **`cargo doc -p <crate> --open`** — the full, authoritative API for any crate above.
- **Human docs** (prose + rationale + worked examples): the index in the repo `README.md`, then `docs/architecture.md`, `docs/library.md`, `docs/design.md`, `docs/packs.md`, `docs/mechanisms.md`, `docs/cli.md`, `docs/games.md` (normal-form games).
- **Worked examples in-tree:** `crates/socsim-engine/examples/{engine_only,opinion_dynamics,cellular_automata,snapshot_resume}.rs`; `crates/socsim-packs/examples/hr_baseline.rs`; `crates/socsim-games/examples/ipd_pair.rs` (`--features core,engine`).

---

## Keeping this current

This map is a curated index, so it drifts unless maintained. Adding a **crate**, a **feature flag**, or a **mechanism** to socsim must update **both** this capability map and `docs/ai/recipes.md` (new crate → a per-crate digest row + a capability-table row; new feature → its feature-flags entry; new mechanism → a catalog row + the relevant capability bound). This extends the repo's existing practice that every new mechanism ships with matching docs.
