# socsim Recipes (for AI coding agents)

Copy-paste, task-oriented recipes for building implementations — mostly paper
replications — **on top of** the socsim Rust library. Each recipe is grounded in
real, working code in this repo. For a per-crate API overview, see
[`capability-map.md`](capability-map.md); this file is the end-to-end "how do I
actually do X" companion.

**Conventions used below**

- Crate **package** names are hyphenated (`socsim-core`); Rust **import** paths
  use underscores (`socsim_core`).
- All git deps point at this repo, `branch = "main"`. Pin the exact commit via
  the consuming crate's `Cargo.lock`.
- Determinism is a first-class concern in replications: derive every RNG stream
  from one root seed and pass seeds explicitly (never rely on "current" RNG).

---

## Recipe 1 — New replication crate skeleton

**When to use.** Starting a fresh paper replication. The house layout is a Cargo
workspace whose `simulation/` member is a Rust crate with a `lib.rs` (modules,
unit-testable) plus a `main.rs` (clap subcommands). A sibling `tools/` (Python /
uv) usually does plotting — out of scope here.

**Dependencies.** Two dep sets exist depending on the model class. Pick one.

*Spatial / ABM library mode* (deterministic, no LLM — the Hegselmann-Krause
replication uses exactly this set):

```toml
# simulation/Cargo.toml
[package]
name    = "mypaper-simulation"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "mypaper"
path = "src/main.rs"

[lib]
name = "mypaper_simulation"
path = "src/lib.rs"

[dependencies]
rand  = { version = "0.8", features = ["small_rng"] }
csv   = "1.3"
serde = { version = "1.0", features = ["derive"] }
clap  = { version = "4.5", features = ["derive"] }

socsim-core       = { git = "https://github.com/akitenkrad/rs-social-simulation-tools", branch = "main", package = "socsim-core" }
socsim-engine     = { git = "https://github.com/akitenkrad/rs-social-simulation-tools", branch = "main", package = "socsim-engine" }
socsim-mechanisms = { git = "https://github.com/akitenkrad/rs-social-simulation-tools", branch = "main", package = "socsim-mechanisms" }
socsim-results    = { git = "https://github.com/akitenkrad/rs-social-simulation-tools", branch = "main", package = "socsim-results" }
# add socsim-net for graph worlds, socsim-grid for lattices, socsim-metrics for stats.
```

*Engine-free LLM-social mode* (agents decide via an LLM; the gao2023 /
chuang2024 / ren2024 replications use exactly this set):

```toml
[dependencies]
rand  = { version = "0.8" }
serde = { version = "1.0", features = ["derive"] }
clap  = { version = "4.5", features = ["derive"] }

socsim-core    = { git = "https://github.com/akitenkrad/rs-social-simulation-tools", branch = "main", package = "socsim-core" }
socsim-engine  = { git = "https://github.com/akitenkrad/rs-social-simulation-tools", branch = "main", package = "socsim-engine" }
socsim-net     = { git = "https://github.com/akitenkrad/rs-social-simulation-tools", branch = "main", package = "socsim-net" }
socsim-llm     = { git = "https://github.com/akitenkrad/rs-social-simulation-tools", branch = "main", package = "socsim-llm", features = ["live"] }
socsim-results = { git = "https://github.com/akitenkrad/rs-social-simulation-tools", branch = "main", package = "socsim-results" }
```

`socsim-llm`'s `live` feature pulls in the Ollama + OpenAI HTTP backends
(`FallbackClient`). `socsim-llm` owns all HTTP — you do **not** add `reqwest` /
`ureq` yourself.

**Code.** `main.rs` shape — the house pattern is clap subcommands `run` / `sweep`
(add `analyze` if you post-process a results dir). Grounded in
`replications/hegselmann2005/simulation/src/main.rs`.

```rust
use clap::{Parser, Subcommand};
use socsim_results::{refresh_latest_symlink, timestamp, write_csv, write_json};

#[derive(Parser, Debug)]
#[command(name = "mypaper", about = "Author (Year) — replication")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Single condition, single seed.
    Run(RunArgs),
    /// Parameter sweep / sensitivity analysis.
    Sweep(SweepArgs),
}

#[derive(Parser, Debug)]
struct RunArgs {
    #[arg(long, default_value_t = 625)] n: usize,
    #[arg(long, default_value_t = 0.15)] eps: f64,
    #[arg(long, default_value_t = 100)] max_iterations: usize,
    /// Omit for a random seed; pass it for reproducible runs.
    #[arg(long)] seed: Option<u64>,
    #[arg(long, default_value = "results")] output_dir: String,
}

#[derive(Parser, Debug)]
struct SweepArgs {
    #[arg(long, default_value_t = 0.0)] eps_min: f64,
    #[arg(long, default_value_t = 0.40)] eps_max: f64,
    #[arg(long, default_value_t = 0.01)] eps_step: f64,
    #[arg(long, default_value_t = 50)] runs: usize,
    #[arg(long)] seed: Option<u64>,
    #[arg(long, default_value = "results")] output_dir: String,
}

fn main() {
    match Cli::parse().command {
        Commands::Run(args) => run_cmd(args),
        Commands::Sweep(args) => sweep_cmd(args),
    }
}

fn run_cmd(_args: RunArgs) { /* build world + mechanisms, run, write_csv/write_json */ }
fn sweep_cmd(_args: SweepArgs) { /* loop conditions × seeds, aggregate */ }
```

**Gotchas.**
- Keep the **model** (world + mechanisms + paper-specific metrics) in your
  replication crate; keep **reusable machinery** (engine loop, generic
  mechanisms, RNG, stats, output) in socsim. If a mechanism is paper-specific,
  `impl Mechanism` locally (Recipe 2); if it is a textbook model already in
  `socsim-mechanisms`, reuse it.
- `socsim-marl` pulls in `burn`; only add it (behind a `marl` feature) if you
  need learned policies.
- A workspace-root `Cargo.toml` (`[workspace] members = ["simulation"]`) sits
  above `simulation/`; the deps above live in `simulation/Cargo.toml`.

---

## Recipe 2 — Implement a custom `Mechanism`

**When to use.** Your paper's update rule is not one of the stock mechanisms in
`socsim-mechanisms`. You translate it to one `impl Mechanism`, picking the phase
it runs in.

**Dependencies.** `socsim-core` (the trait + capability traits), `rand` (for the
`Rng` extension methods on `ctx.rng`).

**Code.** Grounded in `crates/socsim-mechanisms/src/opinion.rs`
(`HegselmannKrauseMechanism`) and `src/contagion.rs` (`SiContagionMechanism`).

The trait you implement (verbatim from `crates/socsim-core/src/lib.rs`), generic
over your world `W: WorldState`:

```rust
pub trait Mechanism<W: WorldState> {
    fn name(&self) -> &str;
    fn phases(&self) -> &'static [Phase];
    fn apply(&mut self, phase: Phase, ctx: &mut StepContext<'_, W>) -> Result<()>;
}
```

The 6 phases run in this fixed order each step
(`Phase::ORDER`): `PreStep, Environment, Decision, Interaction, Reward,
PostStep`. `StepContext` is how you reach the world, RNG, recorder and
activation order:

```rust
pub struct StepContext<'a, W: WorldState> {
    pub world:       &'a mut W,
    pub clock:       SimClock,            // copy of the step clock
    pub rng:         &'a mut SimRng,
    pub recorder:    &'a mut dyn Recorder,
    pub agent_order: &'a [AgentId],       // scheduler-decided activation order
    pub scratch:     &'a mut Blackboard,  // step-scoped scratchpad, cleared each step
    pub stop:        &'a mut bool,        // ctx.request_stop() to halt the run
}
```

A synchronous bounded-confidence-style mechanism (snapshot all opinions, compute
new values, then batch write-back — order-independent so it pairs with the
`SequentialScheduler`):

```rust
use rand::Rng; // brings gen_range / gen onto ctx.rng
use socsim_core::{Mechanism, Neighbors, Phase, Result, ScalarOpinions, StepContext};

pub struct MyBoundedConfidence {
    pub epsilon: f64,
}

impl<W: ScalarOpinions + Neighbors> Mechanism<W> for MyBoundedConfidence {
    fn name(&self) -> &str { "my_bounded_confidence" }

    // Pairwise / synchronous opinion updates belong in the Interaction phase.
    fn phases(&self) -> &'static [Phase] { &[Phase::Interaction] }

    fn apply(&mut self, _phase: Phase, ctx: &mut StepContext<'_, W>) -> Result<()> {
        let ids = ctx.world.agent_ids();
        // Snapshot BEFORE mutating, so updates are synchronous.
        let prev: Vec<f64> = ids.iter().map(|&id| ctx.world.opinion(id)).collect();

        let mut next = prev.clone();
        for (idx, &id) in ids.iter().enumerate() {
            let xi = prev[idx];
            // Confidence set = self + neighbours within epsilon.
            let mut acc = xi;
            let mut count = 1.0_f64;
            for nb in ctx.world.neighbors_of(id) {
                let xj = ctx.world.opinion(nb);
                if (xj - xi).abs() <= self.epsilon {
                    acc += xj;
                    count += 1.0;
                }
            }
            next[idx] = acc / count;
        }

        for (idx, &id) in ids.iter().enumerate() {
            ctx.world.set_opinion(id, next[idx]); // batch write-back
        }
        Ok(())
    }
}
```

If your rule is **stochastic** and order-sensitive (e.g. SI contagion or
Deffuant pair-picking), visit agents in `ctx.agent_order` and draw with the RNG:

```rust
let order: Vec<AgentId> = if ctx.agent_order.is_empty() {
    ctx.world.agent_ids()
} else {
    ctx.agent_order.to_vec()
};
for &id in &order {
    for nb in ctx.world.neighbors_of(id) {
        if ctx.world.is_active(nb) && ctx.rng.gen::<f64>() < self.beta {
            // ...infect id...
            break;
        }
    }
}
// ctx.request_stop();  // e.g. on saturation / convergence
```

Your world must implement `WorldState` plus the capability traits your mechanism
bounds on (`ScalarOpinions`, `Neighbors`, `BinaryState`, `CultureVectors`,
`ActivationThreshold`, `GroupMembership`, all re-exported from `socsim_core`).
See Recipe 3 for a minimal world.

**Gotchas.**
- **Phase choice matters.** Environment = world-driven changes; Decision =
  per-agent choices (the LLM call goes here, Recipe 8); Interaction =
  agent-agent updates; Reward = payoff/learning; Pre/PostStep = bookkeeping &
  metrics. Within a step, all mechanisms in an earlier phase run before any in a
  later phase.
- **Synchronous vs sequential.** If your update reads neighbours, decide
  explicitly: snapshot-then-write-back (synchronous, activation-order
  independent) or in-place (sequential, order matters — then rely on
  `ctx.agent_order`).
- **Determinism.** Draw only from `ctx.rng`; never `rand::thread_rng()`. Two
  identical seeds must produce identical runs.
- `apply` is `&mut self`, so a mechanism may own state (e.g. an LLM client +
  metadata collector, Recipe 8). Keep it RNG-free unless it genuinely samples.

---

## Recipe 3 — Build & run a simulation in library mode

**When to use.** You have a world + mechanism(s) and want to run them: one seed,
then many seeds, then a parameter sweep.

**Dependencies.** `socsim-core`, `socsim-engine`, `socsim-mechanisms` (if reusing
stock mechanisms). For the multi-seed / sweep helpers also add `socsim-runner`,
`socsim-config`, `socsim-log`.

**Code — single seed, run N steps.** Grounded in
`crates/socsim-engine/examples/opinion_dynamics.rs` and the
`SimulationBuilder` API in `crates/socsim-engine/src/lib.rs`.

```rust
use rand::Rng;
use socsim_core::{
    derive_seed, AgentId, Neighbors, ScalarOpinions, SimClock, SimRng, WorldState,
};
use socsim_engine::{RandomActivationScheduler, SimulationBuilder};
use socsim_mechanisms::{HegselmannKrauseMechanism, MeanOperator};

struct MyWorld {
    clock: SimClock,
    opinions: Vec<f64>,
}

impl WorldState for MyWorld {
    fn agent_ids(&self) -> Vec<AgentId> {
        (0..self.opinions.len() as u64).map(AgentId).collect()
    }
    fn clock(&self) -> &SimClock { &self.clock }
    fn clock_mut(&mut self) -> &mut SimClock { &mut self.clock }
}
impl ScalarOpinions for MyWorld {
    fn opinion(&self, id: AgentId) -> f64 { self.opinions[id.0 as usize] }
    fn set_opinion(&mut self, id: AgentId, v: f64) { self.opinions[id.0 as usize] = v; }
}
impl Neighbors for MyWorld {
    fn neighbors_of(&self, id: AgentId) -> Vec<AgentId> {
        (0..self.opinions.len() as u64).map(AgentId).filter(|&j| j != id).collect()
    }
}

fn build(seed: u64, n: usize, n_steps: u64, eps: f64) -> socsim_engine::Simulation<MyWorld> {
    // Separate RNG streams for init vs engine — both derived from one root seed.
    let mut init = SimRng::from_seed(derive_seed(seed, &[0]));
    let world = MyWorld {
        clock: SimClock::new(n_steps),                 // run stops at t_max = n_steps
        opinions: (0..n).map(|_| init.gen::<f64>()).collect(),
    };
    SimulationBuilder::new(world)
        .seed(derive_seed(seed, &[1]))                 // engine / scheduler RNG stream
        .scheduler(Box::new(RandomActivationScheduler)) // default is SequentialScheduler
        .add_mechanism(Box::new(HegselmannKrauseMechanism::new(eps, MeanOperator::Arithmetic)))
        .build()                                        // default recorder = NullRecorder
}

fn main() -> socsim_core::Result<()> {
    let mut sim = build(42, 100, 50, 0.2);
    sim.run()?;                                         // runs to t_max
    // Or observe each step:
    // sim.run_observed(|report| { /* report.t, report.world, report.stopped */ })?;
    println!("final t = {}", sim.world().clock().t());
    Ok(())
}
```

`SimulationBuilder` chain (from `crates/socsim-engine/src/lib.rs`):
`new(world)` → `.seed(u64)` → `.scheduler(Box<dyn Scheduler<W>>)` →
`.add_mechanism(Box<dyn Mechanism<W>>)` (call repeatedly) →
`.recorder(Box<dyn Recorder>)` → `.build()`. Drive it with `sim.run()`,
`sim.run_until(pred)`, `sim.run_observed(|report| …)`, or manual
`sim.step()` in a loop.

**Code — multi-seed and parameter sweep via `socsim-runner`.** Grounded in
`crates/socsim-runner/src/lib.rs`. Note: the runner is **scenario / Registry
driven** — it builds mechanisms by name from a `Scenario` config, not from a raw
`Vec<Box<dyn Mechanism>>`. You supply a world-factory closure and a registration
closure.

```rust
use socsim_config::{Params, Registry, Scenario};
use socsim_runner::{run_seeds, run_sweep, summarize, SweepAxis, WorldFactory};

// world_factory: builds a fresh world per (params, seed).
let world_factory: WorldFactory<MyWorld> =
    Box::new(|_params: &Params, seed: u64| {
        let mut init = SimRng::from_seed(derive_seed(seed, &[0]));
        Ok(MyWorld {
            clock: SimClock::new(50),
            opinions: (0..100).map(|_| init.gen::<f64>()).collect(),
        })
    });

// register: tells the runner how to build each mechanism named in the scenario.
let register = |reg: &mut Registry<MyWorld>| {
    reg.register("hegselmann_krause", |_p: &Params| {
        Ok(Box::new(HegselmannKrauseMechanism::new(0.2, MeanOperator::Arithmetic)) as _)
    });
};

// scenario: typically loaded from a TOML file; see crates/socsim-config.
let scenario: Scenario = /* Scenario::from_toml_str(...)? */ todo!();

// Multi-seed: run seeds 0..50 in parallel, then aggregate mean/std/min/max.
let results = run_seeds(&scenario, &world_factory, &register, 0..50, /*parallel=*/ true)?;
let summary = summarize(&results);   // Summary { metrics: Vec<MetricStats> }
println!("{}", summary.to_csv());

// Parameter sweep: vary "<mechanism>.<param>" across values, seeds per point.
let axes = [SweepAxis { param_key: "hegselmann_krause.epsilon".into(),
                        values: vec![0.05, 0.10, 0.15, 0.20] }];
let points = run_sweep(&scenario, &axes, &world_factory, &register,
                       (0..20).collect(), /*parallel=*/ true)?;
for p in &points { /* p.params: Vec<(String,f64)>, p.summary: Summary */ }
```

**Gotchas.**
- For a quick study you do **not** need the runner — just loop
  `SimulationBuilder` per seed yourself (build → `run()` → read
  `sim.world()`/recorder). The runner adds value when you want scenario configs,
  parallel seeds, and aggregated summaries. There is no "list of boxed
  mechanisms + seeds" entry point; the runner requires a `Scenario`.
- `run_seeds` / `run_sweep` return results **sorted by seed** and parallelise
  with rayon when `parallel = true`. `WorldState: Send` is required for
  parallel.
- Reset the clock per run: the runner sets
  `*world.clock_mut() = SimClock::new(scenario.simulation.t_max)` itself; if you
  hand-loop, set `SimClock::new(n_steps)` in the world factory.
- `TODO(verify):` the exact `Scenario` constructor / field shape lives in
  `crates/socsim-config/src/scenario.rs` (`scenario.simulation.t_max`,
  `scenario.simulation.scheduler`, `scenario.mechanisms[].name`). Confirm there
  before authoring a scenario by hand.

---

## Recipe 4 — Survey microdata recode

**When to use.** You need to turn raw survey microdata (e.g. ANES) into recoded
demographic categories + an actual-outcome label, and estimate marginal
distributions — typically to seed an LLM-agent population or to validate against
ground truth.

**Dependencies.**

```toml
socsim-survey   = { git = "https://github.com/akitenkrad/rs-social-simulation-tools", branch = "main", package = "socsim-survey" }
socsim-datasets = { git = "https://github.com/akitenkrad/rs-social-simulation-tools", branch = "main", package = "socsim-datasets" }
```

`socsim-survey` is schema/recode logic (no built-in schemas); `socsim-datasets`
ships the concrete ANES/CES schema builders (`anes::anes_2020()`,
`ces::ces_2022()`, etc.) and the dataset registry.

**Code.** Grounded in `crates/socsim-datasets/tests/anes_recode.rs` and the
`socsim-survey` public API.

```rust
use socsim_datasets::anes::{anes_2020, OUTCOME_DEM, OUTCOME_REP};
use socsim_survey::{
    actual_outcome, demo_label, estimate_distributions, load_named_records, recode_row,
    Record, SurveySchema,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Get a concrete schema (8 demo vars + outcome map for the 2020 study).
    let schema: SurveySchema = anes_2020();

    // 2. Load raw microdata: a header-bearing CSV/TAB -> Vec<Record>.
    //    Record = HashMap<String, String> (column name -> raw cell value).
    let records: Vec<Record> = load_named_records("data/anes_2020.csv")?;

    // 3. Recode one row into canonical demographic labels.
    if let Some(rec) = records.first() {
        let row = recode_row(rec, &schema);          // RecodedRow { attrs: HashMap<..> }
        if row.is_complete(&schema) {
            println!("race = {}", row.attrs["race"]);     // e.g. "white"
            println!("age  = {}", row.attrs["age"]);      // e.g. "40-49"
        }
        // Single-variable label without a full recode:
        let party = demo_label(rec, &schema, "party_id"); // Option<String>
        println!("party_id = {party:?}");

        // Actual vote outcome for this respondent (one of OUTCOME_DEM/OUTCOME_REP).
        match actual_outcome(rec, &schema) {
            Some(o) if o == OUTCOME_DEM => println!("voted {OUTCOME_DEM}"),
            Some(o) if o == OUTCOME_REP => println!("voted {OUTCOME_REP}"),
            _ => println!("no recorded outcome"),
        }
    }

    // 4. Estimate marginal distributions over the whole sample.
    let dists = estimate_distributions(&records, &schema);
    if let Some(race) = dists.demo("race") {           // CategoryDist { labels, probs }
        for (lbl, p) in race.labels.iter().zip(&race.probs) {
            println!("{lbl}: {p:.3}");
        }
    }
    let dem_rate = dists.outcome.rate_of(OUTCOME_DEM);  // OutcomeDistribution
    println!("Dem vote share = {dem_rate:.3}  (n = {})", dists.outcome.total());

    Ok(())
}
```

Signatures (from `crates/socsim-survey/src/{schema,distribution}.rs` and
`lib.rs`), all over `Record = HashMap<String,String>` + `SurveySchema`:

```rust
pub fn recode_row(rec: &Record, schema: &SurveySchema) -> RecodedRow
pub fn demo_label(rec: &Record, schema: &SurveySchema, var_key: &str) -> Option<String>
pub fn actual_outcome(rec: &Record, schema: &SurveySchema) -> Option<&'static str>
pub fn estimate_distributions(records: &[Record], schema: &SurveySchema) -> Distributions
pub fn load_named_records<P: AsRef<std::path::Path>>(path: P) -> Result<Vec<Record>, csv::Error>
```

**Gotchas.**
- ANES microdata is **license-gated** and not shipped — download it yourself
  (Recipe 5) into `data/` before `load_named_records` will find it. CES 2022 is
  CC0 1.0 (public domain) on the Harvard Dataverse and **auto-downloadable** via
  `fetch` (no account / terms), so it lands in `data/` without a manual step.
- `load_named_records` always parses with a `,` delimiter, even for `.tab`
  files. `TODO(verify):` this function has no test/call-site in-repo (only its
  definition at `crates/socsim-survey/src/lib.rs`), so treat its CSV contract as
  documented-but-unexercised — confirm against your actual file's delimiter.
- `recode_row(...).is_complete(&schema)` is the gate for "all demo vars present";
  filter incomplete rows out before building agent populations.
- The schema builders live in `socsim-datasets` (`anes_2012/2016/2020`, plus
  `anes(year) -> Option<SurveySchema>`), **not** in `socsim-survey`.

---

## Recipe 5 — Acquire a dataset

**When to use.** You need the raw microdata file on disk. socsim knows the
registry (keys, citations, checksums). CES 2022 is CC0 on the Harvard Dataverse
and auto-downloads; the ANES Time Series sources are license-gated and must be
fetched manually.

**Dependencies.** For the in-code API, depend on `socsim-datasets` with the
**`acquire`** feature (the library feature is `acquire`; the CLI exposes the same
under `datasets-acquire`):

```toml
socsim-datasets = { git = "https://github.com/akitenkrad/rs-social-simulation-tools", branch = "main", package = "socsim-datasets", features = ["acquire"] }
```

**Code — CLI** (grounded in `crates/socsim-cli/src/datasets.rs`):

```sh
# list / show need no extra feature:
cargo run -p socsim-cli -- datasets list
cargo run -p socsim-cli -- datasets show anes-2020

# fetch downloads (or prints manual instructions); gated behind datasets-acquire:
cargo run -p socsim-cli --features datasets-acquire -- datasets fetch ces-2022 --dest data/
cargo run -p socsim-cli --features datasets-acquire -- datasets fetch anes-2020 --dest data/ --force
```

`fetch` always resolves the key (an unknown key errors with the valid keys,
regardless of features); only the **network download** is gated — building
without `datasets-acquire` makes `fetch` bail with
`"this build has no acquisition support; rebuild the CLI with --features
datasets-acquire"`. `--dest` defaults to `data`.

**Code — in-code API** (grounded in `crates/socsim-datasets/src/acquire.rs`):

```rust
use socsim_datasets::{acquire::{self, FetchOpts}, by_key, all, Source};

fn main() -> anyhow::Result<()> {
    // List the registry: ANES 2012/2016/2020, CES 2022.
    for meta in all() {
        println!("{:12}  {}", meta.key, meta.name);
    }

    let meta = by_key("anes-2020").expect("known key");

    // The ANES Time Series datasets are Source::Manual, so fetch reports manual
    // instructions for them (it does NOT silently fail — it bails with the
    // instructions_url). CES 2022 is Source::Dataverse (CC0) and downloads directly.
    if meta.files.iter().all(|f| matches!(f.source, Source::Manual { .. })) {
        eprintln!("{} is license-gated; download from {}", meta.key, meta.source_url);
    }

    let opts = FetchOpts {
        dest: std::path::PathBuf::from("data"),
        force: false,
        ..FetchOpts::default()          // token defaults to env DATAVERSE_TOKEN
    };
    let written: Vec<std::path::PathBuf> = acquire::fetch(meta, &opts)?;
    println!("on disk: {written:?}");
    Ok(())
}
```

`FetchOpts` (from `acquire.rs`):

```rust
pub struct FetchOpts {
    pub dest:  PathBuf,        // default PathBuf::from("data")
    pub token: Option<String>, // default = env DATAVERSE_TOKEN
    pub force: bool,           // default false (skip verified cache hits)
}
pub fn fetch(meta: &DatasetMeta, opts: &FetchOpts) -> anyhow::Result<Vec<PathBuf>>;
```

**Gotchas.**
- **Every dataset currently in the registry is `Source::Manual`** (ANES is gated
  behind the ANES data center; `sha256` is `None`). `fetch` will report the
  manual instructions URL rather than download — plan for a human step.
- The library feature is `acquire`; the CLI feature is `datasets-acquire`
  (forwards to `socsim-datasets/acquire`). Don't mix them up in `Cargo.toml`.
- `fetch` verifies `sha256` + `expect_rows` when present, uses an atomic
  temp-then-rename cache, and skips verified cache hits unless `force`.
- `socsim datasets show <KEY>` prints a copy-paste `fetch` line for that key.

---

## Recipe 6 — Reusable metrics

**When to use.** You need standard stats / distribution-distance / inter-rater
agreement numbers (segregation index, Gini, KL, Wasserstein, Cohen's kappa,
two-proportion test, ICC, …) without reimplementing them.

**Dependencies.** Default features are **zero-dep** (only `std`):

```toml
socsim-metrics = { git = "https://github.com/akitenkrad/rs-social-simulation-tools", branch = "main", package = "socsim-metrics" }
```

Optional feature adapters (from `crates/socsim-metrics/Cargo.toml`): `core` adds
capability-trait extractors + a `MetricsMechanism<W>` (records in `PostStep`,
never mutates the world); `network` (implies `core`, adds `socsim-net`) adds
network-structure / cascade metrics; `spatial` (implies `core`, adds
`socsim-grid`) adds spatial-segregation metrics. The three base modules below
need none of these.

**Code.** Grounded in `crates/socsim-metrics/src/{stats,distribution,agreement}.rs`.

```rust
use socsim_metrics::stats::{mean, variance, gini, shannon_entropy, polarization};
use socsim_metrics::distribution::{kl_divergence, chi_square_homogeneity, wasserstein_1d, nemd};
use socsim_metrics::agreement::{cohen_kappa, cramers_v, icc, prop_test};

// stats — over &[f64] (population variance; empty/degenerate -> neutral, never NaN)
let m  = mean(&[1.0, 2.0, 3.0]);              // 2.0
let v  = variance(&[1.0, 2.0, 3.0]);          // population (/N)
let g  = gini(&[1.0, 2.0, 3.0]);
let h  = shannon_entropy(&[0.5, 0.5]);        // nats
let pol = polarization(&[0.1, 0.9, 0.2]);     // == std_dev

// distribution — distances between (auto-normalised) histograms
let kl = kl_divergence(&[0.7, 0.3], &[0.5, 0.5]);                 // nats
let (chi, p) = chi_square_homogeneity(&[30.0, 10.0], &[20.0, 20.0]); // (stat, p), df = len-1
let w1 = wasserstein_1d(&[1.0, 0.0, 0.0], &[0.0, 0.0, 1.0]);     // 2.0
let nm = nemd(&[1.0, 0.0, 0.0], &[0.0, 0.0, 1.0], /*range=*/ 2.0); // W1 / range = 1.0

// agreement — 2x2 confusion cells passed row-major (n00, n01, n10, n11)
let kappa = cohen_kappa(20.0, 5.0, 10.0, 15.0);                  // 0.40
let v_assoc = cramers_v(&[vec![10.0, 5.0], vec![3.0, 12.0]]);    // r×c row-major
let icc_min = icc(&[1.0, 2.0, 3.0], &[1.1, 2.1, 2.9]);          // = average_icc(..).min()
let (chi2, pval) = prop_test(6617, 10721, 7463, 12191);         // Yates-corrected
```

Selected signatures (verbatim):

```rust
// stats.rs — also: std_dev, spread, min_max, hhi, simpson_diversity,
//   distinct_clusters(values, tol), bimodality_coefficient, extremeness(xs, center),
//   max_abs_delta(prev, curr), mean_abs_delta(prev, curr), num_distinct(&[u32]), largest_share(&[u32])
pub fn mean(xs: &[f64]) -> f64
pub fn variance(xs: &[f64]) -> f64
pub fn gini(xs: &[f64]) -> f64
pub fn shannon_entropy(weights: &[f64]) -> f64
pub fn polarization(xs: &[f64]) -> f64

// distribution.rs — also: chi_square_sf, mean_diff(p,q,range), sd_diff(p,q,range, SIGNED)
pub fn kl_divergence(p: &[f64], q: &[f64]) -> f64
pub fn chi_square_homogeneity(observed: &[f64], expected: &[f64]) -> (f64, f64)
pub fn wasserstein_1d(p: &[f64], q: &[f64]) -> f64
pub fn nemd(p: &[f64], q: &[f64], range: f64) -> f64

// agreement.rs — also: tetrachoric, average_icc -> AverageIcc{icc1k,icc2k,icc3k}, prop_agree
pub fn cohen_kappa(n00: f64, n01: f64, n10: f64, n11: f64) -> f64
pub fn cramers_v(counts: &[Vec<f64>]) -> f64
pub fn icc(human: &[f64], model: &[f64]) -> f64
pub fn prop_test(x1: u64, n1: u64, x2: u64, n2: u64) -> (f64, f64)   // (chi_squared, p_value)
```

**Gotchas.**
- `stats` and `distribution` return a neutral `0.0` (or `(0.0, 1.0)` for
  chi-square) on empty/degenerate input — never panic, never NaN.
- `agreement` is the opposite: degenerate cases return `f64::NAN`
  (`cohen_kappa`, `cramers_v`, `prop_agree`, `tetrachoric`, `icc`). Guard with
  `is_nan()`.
- `average_icc` / `icc` **panic** if the two slices differ in length — check
  lengths first.
- `agreement` is bit-compatible with the `argyle2023` `common::stats` port; use
  it instead of re-deriving kappa/ICC.

---

## Recipe 7 — Reproduction harness

**When to use.** You have already produced observed numbers (a finished run's
metrics) and want to compare them against the paper's reference values
(table/figure anchors) and emit a PASS/off summary. This is the **validation**
step — it does **not** re-run generation.

**Dependencies.**

```toml
socsim-reproduce = { git = "https://github.com/akitenkrad/rs-social-simulation-tools", branch = "main", package = "socsim-reproduce" }
```

**Code.** Grounded in `crates/socsim-reproduce/src/lib.rs` and
`crates/socsim-reproduce/tests/harness.rs`. The crate ships **no anchor values** —
each paper supplies its own `&[Anchor]` plus an observation-lookup closure.

```rust
use socsim_reproduce::{
    build_rows, compare_anchor, write_paper_anchors, write_reproduce_summary,
    Anchor, AnchorStatus,
};

// 1. Anchors come from the PAPER (this repo ships none). One row per claim.
static ANCHORS: &[Anchor] = &[
    Anchor {
        study: "MyPaper", table_or_fig: "Table 1", condition: "overall",
        metric: "biden_rate", paper_value: 0.5743, tolerance: 0.02,
        upper_bound: false, note: "10-iteration mean Biden rate",
    },
    Anchor {
        study: "MyPaper", table_or_fig: "Table 1", condition: "overall",
        metric: "mean_kl", paper_value: 0.0004, tolerance: 0.001,
        upper_bound: true,  note: "mean KL; gate is an upper bound",
    },
];

// 2. Observation lookup: map each anchor.metric to your OBSERVED value (or None).
//    These come from a finished run's cached metrics — NOT recomputed here.
fn observe(observed_biden: f64, observed_kl: f64) -> impl Fn(&Anchor) -> Option<f64> {
    move |a: &Anchor| match a.metric {
        "biden_rate" => Some(observed_biden),
        "mean_kl"    => Some(observed_kl),
        _            => None,    // None -> AnchorStatus::NoData / "NO_DATA"
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let lookup = observe(0.5743, 0.0004);

    // Single-anchor check:
    assert_eq!(compare_anchor(&ANCHORS[1], Some(0.0004)), AnchorStatus::Pass);

    // Full table -> rows -> CSV.
    let rows = build_rows(ANCHORS, &lookup);
    write_reproduce_summary(&rows, "results/latest/reproduce_summary.csv")?;
    write_paper_anchors(ANCHORS, "results/latest/paper_anchors.csv")?;
    Ok(())
}
```

Signatures (verbatim):

```rust
pub struct Anchor {            // Debug, Clone, Copy
    pub study: &'static str,
    pub table_or_fig: &'static str,
    pub condition: &'static str,
    pub metric: &'static str,
    pub paper_value: f64,
    pub tolerance: f64,
    pub upper_bound: bool,     // true => "observed < paper_value" gate (strict)
    pub note: &'static str,
}
pub enum AnchorStatus { Pass, Off, NoData }   // .tag() -> "PASS" / "off" / "NO_DATA"

pub fn compare_anchor(anchor: &Anchor, observed: Option<f64>) -> AnchorStatus
pub fn build_rows<F: Fn(&Anchor) -> Option<f64>>(anchors: &[Anchor], observed: F) -> Vec<ReproduceRow>
pub fn write_reproduce_summary(rows: &[ReproduceRow], path: impl AsRef<Path>) -> Result<(), WriteError>
pub fn write_paper_anchors(anchors: &[Anchor], path: impl AsRef<Path>) -> Result<(), WriteError>
```

`reproduce_summary.csv` columns:
`study,table_or_fig,condition,metric,paper_value,observed_value,tolerance,status`.
Missing observation → empty `observed_value` + `status=NO_DATA`.

**Gotchas.**
- The harness **does not re-run generation** and depends only on
  `socsim-results` + `serde`. Produce your observed metrics in the run/sweep
  step, then validate here — keep the two phases separate.
- **Anchor values belong to the paper, encoded in your replication crate** — do
  not look for built-in anchors; there are none.
- `upper_bound: true` uses a strict `<`: an observation equal to the bound is
  `off`, not `Pass`.
- Use `find_latest(results_root, predicate)` to locate the most recent run dir
  to read observed values from.

---

## Recipe 8 — LLM-agent decision

**When to use.** Agents decide via an LLM (the "engine-free LLM-social" model
class). The LLM call is confined to one mechanism in the **Decision** phase.

**Dependencies.**

```toml
socsim-core = { git = "...", branch = "main", package = "socsim-core" }
socsim-engine = { git = "...", branch = "main", package = "socsim-engine" }
socsim-llm  = { git = "...", branch = "main", package = "socsim-llm", features = ["live"] }
```

The `live` feature enables the Ollama + OpenAI backends and the
`build_live_client*` constructors. Without it you can still build deterministic
runs against `mock::ScriptedClient`.

**Code.** Grounded in `crates/socsim-llm/examples/tutorial_llm_agent.rs` and the
`socsim-llm` client API. The mechanism **owns** its client + a metadata
collector; `apply` is `&mut self`:

```rust
use socsim_core::{
    AgentId, Mechanism, Phase, Result, SocsimError, StepContext, WorldState,
};
use socsim_llm::{
    llm_config, wrap_client, LiveClient, LlmConfig, LlmSettings, MetadataCollector, PromptCache,
};
use socsim_llm::mock::ScriptedClient;

struct LlmDecision {
    client: LiveClient,        // = CachingClient<Box<dyn LlmClient>>
    settings: LlmSettings,
    collector: MetadataCollector,
}

impl<W: WorldState> Mechanism<W> for LlmDecision {
    fn name(&self) -> &str { "llm_decision" }
    fn phases(&self) -> &'static [Phase] { &[Phase::Decision] }   // confine the call here

    fn apply(&mut self, _phase: Phase, ctx: &mut StepContext<'_, W>) -> Result<()> {
        let cfg: LlmConfig = llm_config(&self.settings);          // deterministic + seed
        for id in ctx.world.agent_ids() {
            let prompt = build_prompt(ctx.world, id);
            // CachingClient::complete is &mut self (it may write to the cache).
            let resp = self.client
                .complete(&prompt, &cfg)
                .map_err(|e| SocsimError::Mechanism(e.to_string()))?;
            self.collector.record(resp.metadata);
            let choice = resp.text.trim().to_string();
            // ...apply `choice` to ctx.world...
            let _ = (id, choice);
        }
        Ok(())
    }
}

fn build_prompt<W: WorldState>(_world: &W, _id: AgentId) -> String { String::new() }

fn main() {
    let settings = LlmSettings { temperature: 0.0, seed: 42, cache_path: None };

    // Live backend (Ollama primary, OpenAI fallback) when feature "live" is on:
    // let client = socsim_llm::build_live_client_from_settings(&settings).expect("live");

    // Deterministic mock for tests / CI:
    let backend = ScriptedClient::new("mock", |_prompt: &str| "yes".to_string());
    let client: LiveClient = wrap_client(backend, PromptCache::in_memory());

    let mech = LlmDecision { client, settings, collector: MetadataCollector::new() };
    let _ = mech; // ...add to SimulationBuilder and run as in Recipe 3...
}
```

`LlmConfig` knobs (from `crates/socsim-llm/src/client.rs`; defaults =
`LlmConfig::deterministic()`):

```rust
pub struct LlmConfig {
    pub temperature: f32,          // 0.0
    pub seed: u64,                 // 0
    pub max_tokens: Option<u32>,   // None
    pub system: Option<String>,    // None  -> with_system(..) prepends a system message
    pub omit_seed: bool,           // false -> true: don't send `seed` to the backend
    pub allow_blank: bool,         // false -> blank/whitespace completion is REJECTED
    pub top_logprobs: Option<u32>, // None  -> backend default 20 (for complete_with_logprobs)
}
```

For logprob-based choices use `complete_with_logprobs` (live builds), e.g.
`cfg.with_top_logprobs(5)`; the response carries
`Option<Vec<TokenLogprob{token, bytes, logprob}>>`. Note
`complete_with_logprobs` does **not** consult or populate the cache.

**Gotchas.**
- **Confine the call to `Phase::Decision`.** No LLM call in Interaction/Reward.
- **Blank rejection is the default** (`allow_blank = false`): an empty/whitespace
  completion returns `LlmError::EmptyResponse`. Opt in with
  `cfg.allow_blank()` only if a blank answer is meaningful. Non-blank text is
  returned **untrimmed** — `.trim()` it yourself.
- **Two determinism layers**, both must be pinned: the engine seed
  (`SimulationBuilder::seed`) and the LLM layer (the prompt cache +
  `llm_config`'s `seed`/`temperature`). A populated `PromptCache` makes a rerun
  fully reproducible offline (cache hit → backend not contacted,
  `metadata.cache_hit = true`, `endpoint = "cache"`).
- Use `wrap_client_shared` / `SharedCachingClient` (interior-mutable, implements
  `LlmClient` so it works behind `Box<dyn LlmClient>`) when several mechanisms
  share one cache. The client trait is **not** `Send`/`Sync` — single-threaded
  by design; do not parallelise seeds across threads while sharing a client.
- Live backend env: `OLLAMA_HOST` (default `http://localhost:11434`),
  `OLLAMA_MODEL` (default `llama3.1`); `OPENAI_API_KEY` / `OPENAI_MODEL` for the
  fallback. Construction is lazy — no network call until the first cache miss.
- `TODO(verify):` exact `ScriptedClient::new` / `OllamaClient::from_env` /
  `OpenAiClient::new` signatures live in `crates/socsim-llm/src/{mock,ollama,
  openai}.rs`; the usages above match the example/tests but open those files if
  you construct a backend directly.

### Recipe 8b — LLM agents in a normal-form game (`socsim-games`, feature `llm`)

**When to use.** The agents play a stage game (Recipe 10) and some or all of
them choose actions with an LLM. Do **not** hand-write a Decision mechanism:
`socsim-games` ships `game_llm_decide`, which batches and deduplicates the
step's LLM calls, replays them from a response store, runs them on worker
threads if asked, and records every call — while immediate rules
(`TitForTat`, …) keep running in `game_decide` in the same world.

**Dependencies.**

```toml
socsim-games  = { git = "...", branch = "main", package = "socsim-games", features = ["llm", "engine"] }  # + "live" for the real backend
socsim-core   = { git = "...", branch = "main", package = "socsim-core" }
socsim-engine = { git = "...", branch = "main", package = "socsim-engine" }
socsim-llm    = { git = "...", branch = "main", package = "socsim-llm" }   # LlmConfig, mock::ScriptedClient
```

**Steps.**
1. Experiment state: wrap yours in `WithLlm<X>` (`X = ()` → `type X = WithLlm<()>`). It carries the
   `LlmReplayManifest`; `assemble_with_llm` requires `X: HasLlmManifest`. Your `X::PolicyState`
   must implement `Slot<LlmConversation>` (`BuiltinPolicyState` does).
2. Rules: immediate kinds in a `RuleBook`; LLM kinds in an `LlmRuleBook`, usually
   `LlmPolicy::new(obs, renderer, parser)` — `OwnHistoryObs::new(k)` + `MarkdownTableRenderer`, or
   `CountsObs` + `CountsRenderer` (dedupes well); `LabelParser::strict()` (any labels) or
   `::lenient()` (ASCII `[A-Za-z0-9_-]+` labels only). Retries/fallback via `.with_retry(RetryProtocol { .. })`.
3. Kinds: `KindCatalog::new().with_rules(&rules)?.with_llm(&llm_rules)?`, then
   `build_world_with(game, n, x, initial, &catalog, &assignment, history_len, t_max, seed)`.
4. Setup: `ResponseStore` (`in_memory` for tests, `open(dir, mode, run_tag, seed)` for runs),
   `BackendIdentity { model, endpoint }`, a `BackendFactory` (tests: a closure returning
   `ScriptedClient`; live: `socsim_games::llm::live_backend_factory()`), then
   `LlmSetup::new(llm_rules, store, backend, identity)` (+ `.executor(..)`, `.options(..)`, `.config(..)`).
5. Assemble: `assemble_with_llm(world, matching, rules, setup, outcome, opts, seed)?.build()`.

**Code (offline, `ScriptedClient`).**

```rust
use std::sync::Arc;
use socsim_games::llm::{
    assemble_with_llm, BackendFactory, BackendIdentity, CountsObs, CountsRenderer, LabelParser,
    LlmPolicy, LlmReplayManifest, LlmRuleBook, LlmSetup, ReplayMode, ResponseStore, WithLlm,
};
use socsim_games::{
    build_world_with, AssignGroup, AssignKind, Assignment, FixedPlayers, InitialActions,
    KindCatalog, MatrixGame, NormalFormOutcome, PlayOptions, RuleBook, TitForTat,
};
use socsim_llm::mock::ScriptedClient;

type X = WithLlm<()>;

fn main() {
    let game = MatrixGame::bimatrix(
        &[vec![3.0, 0.0], vec![5.0, 1.0]],
        &[vec![3.0, 5.0], vec![0.0, 1.0]],
    ).unwrap()
    .with_labels(vec![vec!["C".into(), "D".into()], vec!["C".into(), "D".into()]]).unwrap();

    let mut rules: RuleBook<MatrixGame, X> = RuleBook::new();
    rules.register("tit_for_tat", Arc::new(TitForTat::new(0))).unwrap();
    let mut llm_rules: LlmRuleBook<MatrixGame, X> = LlmRuleBook::new();
    llm_rules.register("llm", Arc::new(LlmPolicy::new(
        CountsObs, CountsRenderer::default(), LabelParser::strict(),
    ))).unwrap();
    let catalog = KindCatalog::new().with_rules(&rules).unwrap().with_llm(&llm_rules).unwrap();

    let store = ResponseStore::in_memory("test", ReplayMode::RecordMisses);
    let identity = BackendIdentity { model: "scripted".into(), endpoint: "mock://scripted".into() };
    // Tests: inject a factory that returns ScriptedClient (built inside each worker thread).
    let backend: BackendFactory = Arc::new(|| Ok(Box::new(
        ScriptedClient::constant("scripted", r#"{"action":"C","reason":"cooperate"}"#),
    )));

    let seed = 3;
    let x = WithLlm { inner: (), llm: LlmReplayManifest::for_store(&store, identity.clone()) };
    let world = build_world_with(
        game, 4, x, InitialActions::Same(0), &catalog,
        &Assignment::Ordered(vec![
            AssignGroup { kind: AssignKind::Rule("llm".into()), count: 2 },
            AssignGroup { kind: AssignKind::Rule("tit_for_tat".into()), count: 2 },
        ]),
        10, 10, seed,
    ).unwrap();

    let setup = LlmSetup::new(llm_rules, store, backend, identity);
    let mut sim = assemble_with_llm(
        world, FixedPlayers::pairs(&[(0, 2), (1, 3)]), rules, setup,
        NormalFormOutcome, PlayOptions::new(), seed,
    ).unwrap().build();
    for _ in 0..10 { sim.step().unwrap(); }
    assert_eq!(sim.world().held(), &[0, 0, 0, 0]);
}
```

**Gotchas.**
- `build_world` / `assemble` do not know LLM kinds: use `build_world_with` + `assemble_with_llm`
  (plain `assemble` on such a world → `GameError::InvalidKinds`). A kind in both books →
  `RuleError::DuplicateKind`.
- `OnInvalidAction::Fallback` maps `RuleBook` kinds only; LLM kinds fall back through `RetryProtocol`
  (`Previous` by default). A final invalid action stops the step.
- The `BackendFactory` must return a **cache-free** client (the store is the cache); it is called inside
  each worker thread, once per batch / worker (not at all when every request hit the store). Never wrap it in `CachingClient`.
- At temperature 0 all requests share one seed, so identical prompts are one call (good for
  `CountsObs`). At temperature > 0 each agent/attempt has its own seed.
- `ReplayOnly` fails the step on a miss; `RecordMisses` appends misses to this run's shard; `Live`
  ignores the store. **Call `ResponseStore::merge_shards(dir)` yourself** after recording runs —
  `open` reads only the base and its own shard. Merging changes `base_digest`.
- Resume is exact only under `ReplayOnly`, or `RecordMisses` with every post-resume request a hit.
  `verify_snapshot(&snap, &store, &identity)?` before `restore`, with the same `mode`; the driver
  also re-checks the manifest on every Decision.
- `ExecutorOptions` is `#[non_exhaustive]` — `let mut o = ExecutorOptions::default(); o.timeout = ..;`.
  `timeout` only works with `LlmExecutor::Threaded { workers }`.
- Metrics: per-step row `"game"` (`llm.*_rate`) and one `game.llm_call` event per attempt.
- Full reference: [`docs/games.md` § LLM decisions](../games.md#llm-decisions-feature-llm) and
  [`docs/mechanisms/game-llm-decide.md`](../mechanisms/game-llm-decide.md).

---

## Recipe 9 — Output & logging

**When to use.** Persist a run to a timestamped directory with a `latest`
pointer, and/or stream structured per-step events to JSONL.

**Dependencies.**

```toml
socsim-results = { git = "...", branch = "main", package = "socsim-results" }
socsim-log     = { git = "...", branch = "main", package = "socsim-log" }   # only if you record events
```

**Code — timestamped run dir + CSV/JSON.** Grounded in
`crates/socsim-results/src/lib.rs` (this is exactly how the replication binaries
write results):

```rust
use serde::Serialize;
use socsim_results::{create_run_dir, refresh_latest_symlink, timestamp, write_csv, write_json};

#[derive(Serialize)]
struct MetricRow { step: u64, value: f64 }

fn main() -> std::io::Result<()> {
    let ts = timestamp();                          // "YYYYMMDD_HHMMSS"
    let run_dir = create_run_dir("results")?;      // results/<ts>/  (mkdir -p)

    let rows = vec![MetricRow { step: 0, value: 1.0 }, MetricRow { step: 1, value: 0.8 }];
    write_csv(&rows, run_dir.join("metrics.csv")).unwrap();              // header from field names
    write_json(&serde_json::json!({ "seed": 42 }), run_dir.join("config.json")).unwrap(); // pretty

    // results/latest -> <ts>  (unix symlink; no-op on non-unix)
    refresh_latest_symlink("results", &ts)?;
    Ok(())
}
```

There is **no run-dir struct** — `create_run_dir` returns a bare `PathBuf`.
`refresh_latest_symlink`'s second arg is the symlink **target** (the timestamp),
so reuse the same `ts` you created the dir with.

**Code — JSONL event recorder.** Grounded in `crates/socsim-log/src/lib.rs`.
`JsonlRecorder` implements `socsim_core::Recorder`, so you hand it to the builder
and mechanisms record through `ctx.recorder`:

```rust
use socsim_log::JsonlRecorder;

let file = std::fs::File::create("results/latest/events.jsonl").unwrap();
let recorder = JsonlRecorder::new(file);   // any W: std::io::Write
// SimulationBuilder::new(world).recorder(Box::new(recorder)) ... .build();

// Inside a mechanism's apply():
// ctx.recorder.record_metric(ctx.clock.t(), "consensus", 0.93);
// ctx.recorder.record_event(ctx.clock.t(), "turnover",
//     serde_json::json!({ "agent": 7, "team": 2 }));
```

JSONL schema (one object per line):

```json
{"type":"metric","t":1,"key":"consensus","value":0.93}
{"type":"event","t":1,"kind":"turnover","payload":{"agent":7,"team":2}}
```

**Gotchas.**
- `JsonlRecorder` **swallows write errors** (the `Recorder` trait is infallible);
  call `recorder.take_error()` after the run to surface any I/O failure.
- `write_csv` derives the header from the first record's serde field names, so
  every row must be the same `Serialize` struct; `write_json` is pretty-printed.
- `refresh_latest_symlink` is a no-op on non-unix platforms — don't rely on the
  `latest` symlink existing on Windows.
- Alternatives in `socsim-log`: `InMemoryRecorder` (`metrics()` / `events()`
  accessors — used by `socsim-runner`) and `CsvRecorder` (tabular rows). Pick
  JSONL for streaming, in-memory for tests, CSV for tabular output.

---

## Recipe 10 — Normal-form game simulation

**When to use.** Agents repeatedly play a stage game (prisoner's dilemma,
coordination, zero-sum, …): each step they are seated into encounters, choose
actions with per-agent decision rules, receive payoffs, and learn. Use
`socsim-games` instead of hand-writing a world + mechanisms.

**Dependencies.**

```toml
socsim-games  = { git = "...", branch = "main", package = "socsim-games", features = ["engine"] }
socsim-core   = { git = "...", branch = "main", package = "socsim-core" }
socsim-engine = { git = "...", branch = "main", package = "socsim-engine" }
serde = { version = "1", features = ["derive"] }   # only for a custom X / PolicyState
```

**Code — built-in rules, `X = ()`.** Grounded in
`crates/socsim-games/examples/ipd_pair.rs`:

```rust
use std::sync::Arc;
use socsim_games::{
    assemble, build_world, AssignGroup, AssignKind, Assignment, Fixed, FixedPlayers,
    InitialActions, MatrixGame, NormalFormOutcome, PlayOptions, RuleBook, TitForTat,
};

fn main() {
    // action 0 = C, 1 = D; a = row player, b = column player
    let game = MatrixGame::bimatrix(
        &[vec![3.0, 0.0], vec![5.0, 1.0]],
        &[vec![3.0, 5.0], vec![0.0, 1.0]],
    ).unwrap();

    let mut rules = RuleBook::new();
    rules.register("tit_for_tat", Arc::new(TitForTat::new(0))).unwrap();
    rules.register("fixed_defect", Arc::new(Fixed::new(1))).unwrap();

    let seed = 1;
    let rounds = 100;
    let world = build_world(
        game, 2, (), InitialActions::Same(0), &rules,
        &Assignment::Ordered(vec![
            AssignGroup { kind: AssignKind::Rule("tit_for_tat".into()), count: 1 },
            AssignGroup { kind: AssignKind::Rule("fixed_defect".into()), count: 1 },
        ]),
        rounds,          // history_len (ring buffer per agent)
        rounds as u64,   // t_max
        seed,
    ).unwrap();

    let mut sim = assemble(world, FixedPlayers::pairs(&[(0, 1)]), rules,
                           NormalFormOutcome, PlayOptions::new(), seed)
        .unwrap()         // -> SimulationBuilder; add .scheduler/.recorder/.add_mechanism here
        .build();

    let mut scores = [0.0, 0.0];
    for _ in 0..rounds {
        sim.step().unwrap();
        for (s, p) in scores.iter_mut().zip(sim.world().payoffs()) { *s += p; } // per-step payoffs
    }
    assert_eq!(scores, [99.0, 104.0]);
}
```

**Code — a custom rule with model state in `X`.** Grounded in
`crates/socsim-games/tests/experiment_state.rs`:

```rust
use serde::{Deserialize, Serialize};
use socsim_core::{AgentId, SimRng};
use socsim_games::{
    BuiltinPolicyState, DecisionContext, DecisionRule, ExperimentState, MatrixGame,
    RuleError, RuleInitContext, Slot, TftState,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ReputationState { reputation: Vec<f64> }          // model state lives in X

#[derive(Clone, Debug, Serialize, Deserialize)]
enum MyPolicyState { Builtin(BuiltinPolicyState), Discriminator { threshold: f64 } }

impl ExperimentState for ReputationState {
    type PolicyState = MyPolicyState;
    type View<'a> = &'a [f64];                           // what rules may see of X
    fn view_for(&self, _me: AgentId) -> Self::View<'_> { &self.reputation }
}

// Needed only if TitForTat is also registered with this X.
impl Slot<TftState> for MyPolicyState {
    fn slot(&self) -> Option<&TftState> { match self { Self::Builtin(s) => s.slot(), _ => None } }
    fn slot_mut(&mut self) -> Option<&mut TftState> { match self { Self::Builtin(s) => s.slot_mut(), _ => None } }
    fn from_slot(v: TftState) -> Self { Self::Builtin(<BuiltinPolicyState as Slot<TftState>>::from_slot(v)) }
}

struct Discriminator;
impl DecisionRule<MatrixGame, ReputationState> for Discriminator {
    fn name(&self) -> &str { "discriminator" }
    fn init_state(&self, _: &RuleInitContext, _: &MatrixGame) -> Result<MyPolicyState, RuleError> {
        Ok(MyPolicyState::Discriminator { threshold: 0.5 })
    }
    fn decide(&self, state: &mut MyPolicyState,
              ctx: &DecisionContext<'_, MatrixGame, ReputationState>,
              _rng: &mut SimRng) -> Result<usize, RuleError> {
        let MyPolicyState::Discriminator { threshold } = state else {
            return Err(RuleError::MissingSlot("Discriminator"));
        };
        let opponent = ctx.opponents()[0];                // same-encounter opponent only
        Ok(usize::from(ctx.view()[opponent.0 as usize] < *threshold))
    }
}
// Update X from your own Mechanism<GameWorld<MatrixGame, ReputationState>>
// (e.g. in Reward) via ctx.world.x_mut(), added to the builder after assemble().
```

**Snapshot / resume.** `Simulation::snapshot()` captures the whole
`GameWorld` (rule states included) plus the RNG position; rebuild with the same
rules / matching / outcome and `restore(snapshot)` to continue bit-identically.

**Gotchas.**
- Pass the **same `seed`** to `build_world` and `assemble`. Streams: `[0]` world
  init (`Assignment::Shuffled`), `[1]` engine, `[2]` `decision_root`, `[3, agent,
  generation]` rule-state init — use `[4]+` for your own.
- Keep rules, matchings and outcome rules stateless (`&self`); per-agent memory
  goes in `X::PolicyState`, everything else in `X`. Custom matching state goes in
  `X` too.
- The matching decides the role count (`Matching::supports_roles`): the built-in
  pair matchings take **two roles**, `RandomGroups::new(k)` takes `k`,
  `WholePopulation` one per agent (`GameError::UnsupportedRoles` otherwise;
  LLM assembly stays two-role). `GridNeighborhood` uses `Credit::Center` (only
  seat 0 is credited).
- `world().payoffs()` / `n_played()` are **per step** (reset in PreStep);
  `PayoffAggregation::Mean` divides by `n_played`.
- A rule returning an invalid action stops the step by default; use
  `OnInvalidAction::Fallback(map)` (one entry per rule kind in use) to replace
  it and record a `game.invalid_action` event.
- Rules see only their own encounter's opponents by default; reading
  `ctx.omniscient()` deliberately gives up that information structure.
- **Several encounters per agent (tournaments).** With `RoundRobin::new()`
  every pair plays every step, so with three or more agents (or
  `both_orders(true)`) an agent sits in several encounters, in both roles. The
  default `DecisionScope::PerStep` (one decision per agent per step) is then
  rejected by `assemble` (`GameError::InvalidMatching`); pass
  `PlayOptions::new().decision_scope(DecisionScope::PerEncounter)` (one
  decision per seat; the built-in learning rules and `Automaton` then keep one
  state per opponent) or `PerAgentRole`. Under those scopes `held` is not
  updated and `StepRecord::action` is `None`. Worked example:
  `crates/socsim-games/examples/ipd_tournament.rs`; details in
  [`docs/games.md` § Learning rules and decision scope](../games.md#learning-rules-and-decision-scope).
- **Metrics are opt-in and added by you.** `assemble` does not register
  `game_metrics`: build `GameMetrics::new(&game, MetricsOptions::new()...)`
  **before** `game` moves into `build_world`, then `.add_mechanism(Box::new(m))`
  on the builder (it runs in PostStep). Columns land in table `"game"` as
  `game.played_share.<id>`, `game.mean_payoff`, `game.payoff_sd`, … — an
  undefined column is **missing** from the row, never NaN, so readers must
  handle absent keys (`socsim_log::CsvRecorder` keeps every column a table has
  seen and pads gaps — including rows before a late column appeared — with
  NaN; with `llm`, `game_llm_decide` adds its own row to `"game"` each step). `<id>` is `a<k>` (pooled) or `r<role>-a<k>` (per role),
  built from action indices — never parse labels out of column names; use
  `metric_action_ids(&game)` + `action_label`.
- `played_share` counts **seats**, not agents (an agent in three encounters
  counts three times); `MetricsOptions::population_share(true)` adds
  `game.population_share.a<k>` (agents holding `k` at PostStep).
- `game.regret` is on by default for two-role games; `game.eq_distance` needs
  `EquilibriumTarget::Solve`, which solves once in `new` and fails there for
  non-two-player or degenerate games (`GameError::Metrics`).
- `MixedSource::Reported` needs `PlayOptions::report_mixed(true)` and a report
  for every seated rule-driven agent — `Fixed`, `MyopicBestResponse`, LLM kinds
  and custom rules without `mixed` report `None` and make the step fail.
  `report_mixed(true)` with `DecisionScope::PerEncounter` is rejected by
  `assemble`. A report written at step `t` is the **post-learning** mix for
  step `t + 1`.
- Public per-encounter values (`AggregateGame`) are collected only by
  `assemble_with_aggregates(.., seed, aggregate_history_len)`; rules read
  previous steps through `ctx.aggregate_history()` (`None` under plain
  `assemble` or `assemble_with_llm`).
- `MetricsOptions::stop(tol, window)` ends `sim.run()` early (earliest at
  `t = window + 1`); check `sim.stop_requested()` / the last recorded `t`
  rather than assuming `t_max` steps ran. `game.action_entropy` exists only with
  the `metrics` feature (and pooled ids).
- `GameMetrics::describe()` gives `(pattern, meaning)` pairs to paste into a
  runvault `runvault.toml` (`[[metric_patterns]]` / `[metrics."…"]`); do not add
  a runvault dependency to the crate.
- See also [`docs/games.md`](../games.md) (§ Metrics) and the `game_match` /
  `game_decide` / `game_play` / `game_metrics` pages under `docs/mechanisms/`.
- LLM-driven players: see [Recipe 8b — LLM agents in a normal-form game](#recipe-8b--llm-agents-in-a-normal-form-game-socsim-games-feature-llm).
- Imitation / selection without decision rules (spatial or well-mixed
  evolutionary games): see [Recipe 10b — Population dynamics](#recipe-10b--population-dynamics-socsim-games).

---

### Recipe 10b — Population dynamics (`socsim-games`)

**When to use.** Evolutionary game models in which agents do not choose by a
rule but hold a strategy that spreads by imitation or reproduction: spatial
games on a lattice (Nowak & May), games on networks (Fermi / pairwise
comparison), well-mixed proportional imitation, or the Moran process. Every
agent is `AssignKind::Held`; one population dynamic is passed to
`assemble_with` and rewrites `held` in Reward from the step's payoffs.

**Dependencies.** Recipe 10's, plus `features = ["engine", "grid"]` for a
lattice or `["engine", "net"]` (+ `socsim-net` for `SocialNetwork`) for a
network; add `"metrics"` only if you want `game.action_entropy`.

**Code — well-mixed hawk–dove with proportional imitation.** Grounded in
`crates/socsim-games/examples/hawk_dove_large.rs` and `tests/hawk_dove_abm.rs`
(`socsim-log` provides `InMemoryRecorder`):

```rust
use socsim_games::catalog::HawkDove;
use socsim_games::{
    assemble_with, build_world, AssignGroup, AssignKind, Assignment, GameMetrics, InitialActions,
    MetricsOptions, NormalFormOutcome, Pairwise, PlayOptions, ProportionalImitation, RandomPairs,
    RuleBook, Update,
};
use socsim_log::InMemoryRecorder;

fn main() {
    let n = 1_000;
    let seed = 7;
    // Action 0 = hawk, 1 = dove. With V = 2, C = 4 one encounter pays in [-1, 2].
    let game = HawkDove::new(2.0, 4.0).unwrap();
    let metrics = GameMetrics::new(&game, MetricsOptions::new().population_share(true)).unwrap();

    let rules = RuleBook::new(); // no rules: every agent is Held
    let mut initial = vec![1; n];
    initial[..300].fill(0); // 300 hawks, 700 doves
    let held = Assignment::Ordered(vec![AssignGroup { kind: AssignKind::Held, count: n }]);
    let world = build_world(game, n, (), InitialActions::PerAgent(initial), &rules, &held, 1, 200, seed)
        .unwrap();

    let mut sim = assemble_with(
        world,
        RandomPairs::new(),                // n is even: every agent plays exactly once
        rules,
        Pairwise::new(NormalFormOutcome),  // the play path of `assemble`
        ProportionalImitation::new(1.0 / 3.0, Update::Synchronous).unwrap(), // beta = 1 / (2 - (-1))
        PlayOptions::new(),
        seed,
    )
    .unwrap()
    .add_mechanism(Box::new(metrics))
    .recorder(Box::new(InMemoryRecorder::new()))
    .build();
    sim.run().unwrap();

    let recorder = sim.recorder().as_any().unwrap().downcast_ref::<InMemoryRecorder>().unwrap();
    for row in recorder.metrics().iter().filter(|row| {
        [1, 50, 200].contains(&row.t)
            && (row.key == "game.played_share.a0" || row.key == "game.population_share.a0")
    }) {
        println!("t={} {} = {}", row.t, row.key, row.value);
    }
}
```

Prints `population_share.a0` 0.319 at `t = 1` and ≈ 0.5 (= V/C) by `t = 50`.

**Code — lattice (Nowak–May).** Grounded in `examples/nowak_may.rs`; attach the
topology after `build_world`, then use `GridNeighborhood` + `ImitateBest`:

```rust
let world = build_world(game, side * side, (), initial, &rules, &held, 1, steps, seed)?
    .with_topology(Topology::grid(GridSpec {
        rows: side,
        cols: side,
        boundary: GridBoundary::Toroidal,          // or Fixed
        neighborhood: GridNeighbors::Moore,        // or VonNeumann
    })?)?;
```

```rust
let mut sim = assemble_with(
    world,
    GridNeighborhood::new().include_self(true),          // Moore + self-play, Credit::Center
    rules,                                               // empty RuleBook
    Pairwise::new(NormalFormOutcome),
    ImitateBest::new(Update::Synchronous),               // ties: KeepOwn
    PlayOptions::new(),
    seed,
)?
.build();
```

**Choosing the pieces.**

| Want | Matching | Dynamic | Topology |
|---|---|---|---|
| Spatial PD (Nowak & May 1992) | `GridNeighborhood::new().include_self(true)` | `ImitateBest` | `Topology::grid` (required) |
| Games on networks | `NetworkEdges::new()` | `FermiUpdate::new(k, ..)?` | `Topology::network` |
| Well-mixed replicator-like | `RandomPairs::new()` / `RandomGroups::new(k)?` | `ProportionalImitation::new(beta, ..)?` | none |
| Fixation probabilities | `RandomPairs::new()` | `MoranUpdate::new(Fitness::Linear { w }, mutation, ..)?` | none (required) |
| n-player anonymous game (El Farol, minority, public goods) | `WholePopulation::new()` / `RandomGroups::new(k)?` + play `counting_play()` | any copy dynamic except `ImitateBest` without topology | optional |

**Gotchas.**
- `Update::Synchronous` is a required constructor argument (no default).
  Everything is synchronous: next actions come from the pre-update state.
- Any agent with a decision rule makes `assemble_with` fail
  (`GameError::Population`). Mixing learning rules with imitation is not
  supported.
- `ProportionalImitation`: `beta ≤ 1 / Δπ`, Δπ = width of the **per-step**
  payoff range; with `PayoffAggregation::Sum` this scales with the number of
  encounters (Moore + self-play: ×9), and matchings without
  `encounter_bounds` (`FixedPlayers`, `RoundRobin`, custom) are rejected under
  `Sum` — use `Mean`. A user-defined `OutcomeRule` cannot be bounded.
  `MoranUpdate` with `Fitness::Linear` needs `1 − w + w·π_min > 0` (same range).
- `ImitateBest` needs a grid/network; `MoranUpdate` refuses any topology.
- At PostStep `payoffs()` are from the actions **played**, `held()` are the
  **updated** actions; `played_share(t) ≠ population_share(t)`.
- With all agents held, `PerStep`, `Pairwise<NormalFormOutcome>` /
  `counting_play()` and `WholePopulation` / `NetworkEdges` /
  `GridNeighborhood`, the step runs without the play arena: `world.plays()` is
  empty. Pass `PlayOptions::new().materialize(true)` if you need it; results
  are identical either way.
- Random initial placements: shuffle with your own stream
  (`derive_seed(seed, &[0, 2])` in the examples, or `[4]+`), never with the
  engine stream.
- Solvers for comparison (default build): `solve::pure_nash_counts` (anonymous
  games), `solve::replicator_integrate` / `replicator_fixed_points`,
  `solve::ess_verdict`, `HawkDove::hawk_share_ess()`.
- Run the examples: `cargo run --release -p socsim-games --features
  core,engine,grid --example nowak_may -- --steps 200 --seed 1` (CSV
  `generation,cooperator_share`) and `cargo run --release -p socsim-games
  --features core,engine --example hawk_dove_large -- --n 1000000 --steps 10`.
  Details: [`docs/games.md` § Population dynamics](../games.md#population-dynamics)
  and `docs/mechanisms/{imitate-best,fermi-update,replicator-update,moran-update}.md`.

---

## Choosing library mode vs CLI mode

| | **Library mode** | **CLI mode** (`socsim` binary) |
|---|---|---|
| You write | Your own `main.rs` + `impl WorldState`/`Mechanism` | A TOML `Scenario` referencing registered mechanisms |
| Mechanisms | Stock + your custom ones, by type | Built by name from a `Registry` / `ModulePack` |
| Best for | **Paper replications** with a bespoke world, custom metrics, custom output | Quick experiments over packaged worlds & sweeps without writing Rust |
| Determinism | You wire seeds (`derive_seed` + `SimulationBuilder::seed`) | Scenario `seed` / `--seeds` flag |
| Output | You call `socsim-results` / `socsim-log` directly | CLI writes `runs/` dirs for you |

**Rule of thumb for replications:** use **library mode**. Paper models almost
always need a bespoke `WorldState`, paper-specific metrics, and paper-specific
output layout — all of which are most naturally expressed as Rust in your own
crate. Reach for the CLI / `socsim-runner` scenario path only when your model is
already expressible as a registered pack and you just want sweeps for free.

For the per-crate capability overview that complements these task recipes, see
[`capability-map.md`](capability-map.md).
