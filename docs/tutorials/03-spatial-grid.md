**English** | [日本語](03-spatial-grid.ja.md)

# T3 — A spatial grid model

**What you'll build:** an event-driven voter cellular automaton on a toroidal lattice — many micro-events per tick, O(1) neighbour lookups, stopping at consensus — plus how to read a spatial metric off the grid.
**Estimated time:** 40 minutes.

## Prerequisites

- [T1 — Your first model](01-first-model.md) (`WorldState`, `Mechanism`, `run_observed`, seeds).
- T2 is *not* required (this is the lattice sibling of the network model).

Backing example, CI-compiled: [`crates/socsim-engine/examples/cellular_automata.rs`](../../crates/socsim-engine/examples/cellular_automata.rs). Open it alongside this page.

## Steps

### 1. A lattice world: `CellGrid` + precomputed `Adjacency`

`socsim-grid` gives you 2D space. Two pieces matter here: `CellGrid<T>` stores a value `T` for **every** cell (here a `u8` opinion), and `Adjacency` is a **precomputed** neighbour table you build once so the hot per-step loop does O(1) lookups with no allocation:

```rust
struct VoterWorld {
    clock: SimClock,
    /// Per-cell opinion, row-major over the grid.
    cells: CellGrid<u8>,
    /// Precomputed CSR neighbour table (flat row-major indices).
    adjacency: Adjacency,
}

impl VoterWorld {
    fn new(rows: usize, cols: usize, n_opinions: u8, rng: &mut socsim_core::SimRng) -> Self {
        let grid = Grid::new(rows, cols, Boundary::Toroidal);
        // Precompute the Moore (8-neighbour) adjacency once; reused every tick.
        let adjacency = grid.adjacency(Neighborhood::Moore);
        let cells = CellGrid::from_fn(grid, |_r, _c| rng.gen_range(0..n_opinions));
        Self { clock: SimClock::new(0), cells, adjacency }
    }
}
```

`Boundary::Toroidal` makes the lattice wrap (no edges); `Neighborhood::Moore` is the 8-cell neighbourhood. Building `adjacency` once up front is the key spatial idiom — see [Library API, non-allocating neighbour queries](../library.md#non-allocating-neighbour-queries).

A `CellGrid` world has no per-agent roster — the "agents" are cells driven en masse from one mechanism — so `agent_ids` returns empty:

```rust
impl WorldState for VoterWorld {
    fn agent_ids(&self) -> Vec<AgentId> {
        Vec::new()
    }
    // clock / clock_mut as usual
}
```

### 2. Many micro-events inside one tick

A voter model has no natural "step": it just keeps firing single-cell updates (pick a cell, copy a random neighbour's opinion). The idiom is to **batch many micro-events into one engine tick** rather than giving each event its own step:

```rust
fn apply(&mut self, _phase: Phase, ctx: &mut StepContext<'_, VoterWorld>) -> Result<()> {
    let n = ctx.world.cells.len();
    if n == 0 { return Ok(()); }

    // Batch of micro-events, all driven by ctx.rng for reproducibility.
    for _ in 0..self.events_per_step {
        let idx = ctx.rng.gen_range(0..n);
        let nbrs = ctx.world.adjacency.neighbors(idx);   // O(1) borrowed &[usize]
        if nbrs.is_empty() { continue; }
        let nbr = nbrs[ctx.rng.gen_range(0..nbrs.len())];
        let opinion = *ctx.world.cells.get_idx(nbr).expect("in-range");
        if let Some(cell) = ctx.world.cells.get_idx_mut(idx) { *cell = opinion; }
    }

    if ctx.world.distinct_opinions() <= 1 { ctx.request_stop(); }  // consensus
    Ok(())
}
```

Two things to note. `ctx.rng` drives both the cell pick and the neighbour pick, so the whole run is reproducible from the seed. And `adjacency.neighbors(idx)` returns a borrowed slice — no allocation per event, which matters when you fire hundreds of events per tick. The mechanism runs in `Phase::Interaction` and asks the engine to stop once the lattice is uniform (the absorbing state).

### 3. Observe a per-step metric with `run_observed`

`run_observed` calls your closure once per executed step with a `StepReport` reflecting state *after* that step — the ergonomic way to collect a convergence curve without hand-rolling a `step()` loop:

```rust
sim.run_observed(|report| {
    let distinct = report.world.distinct_opinions();
    if report.t <= 5 || report.t % 10 == 0 || report.stopped {
        println!("  {:>3}  {}", report.t, distinct);
    }
})
.expect("simulation completed");
```

`distinct_opinions()` here is a tiny local metric (count of distinct cell values). For **labelled spatial structure** — "how segregated is the lattice" — `socsim-metrics` ships ready-made spatial metrics you can read straight off a `Grid` via a label-accessor closure:

```rust,ignore
use socsim_metrics::spatial::{local_similarity, segregation_index};
use socsim_grid::Neighborhood;

// label(r, c) -> Some(category) for an occupied cell, None for vacant.
let s = segregation_index(&grid, Neighborhood::Moore, |r, c| Some(cells.get(r, c)?.clone()));
// `s` → 1.0 under perfect segregation; near the population share under a random layout.
```

`segregation_index` is the mean `local_similarity` (fraction of like neighbours) over all occupied cells — the standard Schelling measure. It's a pure read-only function, so it never perturbs the run. (The voter example uses the simpler `distinct_opinions` because it tracks *consensus*, not *segregation*; swap in `segregation_index` when your cells carry categorical labels and you care about spatial sorting.)

### 4. Seeds and the safety cap

The example uses the same `&[0]` world-init / `&[1]` engine split as T2, and sets a `t_max` safety cap (`500`) that consensus normally beats:

```rust
let root = 7u64;
let mut init_rng = socsim_core::SimRng::from_seed(socsim_core::derive_seed(root, &[0]));
let mut world = VoterWorld::new(16, 16, 4, &mut init_rng);
world.clock = SimClock::new(500);

let mut sim = SimulationBuilder::new(world)
    .seed(socsim_core::derive_seed(root, &[1]))
    .add_mechanism(Box::new(VoterModel { events_per_step: 256 }))
    .build();
```

## Run it

```sh
cargo run -p socsim-engine --example cellular_automata
```

```
=== socsim cellular_automata (voter model) ===
16x16 toroidal lattice, 4 opinions, 256 events/tick

  t   distinct opinions
  ---------------------
    1  4
   50  3
  110  2
  ...
  390  2
  396  1

reached consensus at t = 396 (distinct = 1)
```

The four initial opinions collapse to one (consensus) at `t = 396`, beating the `500` cap. Same seed → same trajectory every run.

## What you learned

- `socsim-grid` provides 2D space: `CellGrid<T>` for per-cell state and `Adjacency` for a **precomputed** O(1) neighbour table — build it once, reuse it every tick.
- The **batch-events-in-one-tick** idiom maps an asynchronous event model onto the engine's discrete loop; `ctx.rng` drives every random choice so the run stays reproducible.
- `run_observed` gives you a clean per-step observation hook (`StepReport`).
- `socsim-metrics::spatial` (`segregation_index`, `local_similarity`) reads spatial structure off a `Grid` via a label closure — a pure, reproducibility-safe metric.

See the [Library API grid section](../library.md#spatial-models-with-socsim-grid) for the full `socsim-grid` surface.

## Next

[T4 — An LLM-driven agent](04-llm-agent.md): put a language model inside one phase while keeping the run deterministic.
