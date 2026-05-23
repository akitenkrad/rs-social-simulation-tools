//! Event-driven cellular automaton — a voter-model lattice on a `CellGrid`.
//!
//! This is the lightweight, lattice-flavoured counterpart to `engine_only.rs`.
//! It shows how to express a *sub-tick / event-driven* model on top of the
//! engine's discrete six-phase loop, and stitches together the spatial and
//! observation APIs added on this branch:
//!
//! - [`socsim_grid::CellGrid`] — per-cell mutable state (here a `u8` opinion per
//!   cell), the primitive for cellular-automata / lattice-attribute models.
//! - [`socsim_grid::Grid::adjacency`] — a **precomputed** CSR neighbour table
//!   ([`socsim_grid::Adjacency`]) built once up front, so the hot per-step loop
//!   does O(1) slice lookups with **no per-step allocation** (instead of calling
//!   the allocating `Grid::neighbors` every event).
//! - The **batch-events-inside-one-mechanism** idiom: a Gillespie / async voter
//!   model fires *many* micro-events between observable ticks. We don't give
//!   each event its own engine step; instead one [`Mechanism::apply`] on
//!   [`Phase::Interaction`] performs a whole batch of `events_per_step` random
//!   single-cell updates, mapping `events_per_step` events onto one engine tick.
//!   `ctx.rng` drives both the cell pick and the neighbour pick, so the run is
//!   fully reproducible from the seed.
//! - [`socsim_core::StepContext::request_stop`] — the mechanism asks the engine
//!   to stop once the lattice is uniform (every cell agrees), the model's
//!   absorbing state.
//! - [`socsim_engine::Simulation::run_observed`] — the ergonomic per-step loop:
//!   we collect the number of distinct opinions after each step from the
//!   [`socsim_engine::StepReport`] without hand-rolling a `step()` + `scratch()`
//!   loop.
//!
//! The model uses the default [`socsim_core::NullRecorder`] — no `socsim-log`
//! dependency is needed for a self-contained library model like this.
//!
//! Run with:
//!   cargo run -p socsim-engine --example cellular_automata

use rand::Rng;

use socsim_core::{AgentId, Mechanism, Phase, Result, SimClock, StepContext, WorldState};
use socsim_engine::SimulationBuilder;
use socsim_grid::{Adjacency, Boundary, CellGrid, Grid, Neighborhood};

// ── World: a lattice of opinions ────────────────────────────────────────────
//
// Every cell holds a small integer "opinion". The voter model repeatedly picks
// a random cell and copies a random neighbour's opinion; the lattice drifts
// toward consensus. We keep the per-cell state in a `CellGrid<u8>` and the
// neighbour table in a precomputed `Adjacency` so the mechanism never allocates.

struct VoterWorld {
    clock: SimClock,
    /// Per-cell opinion, row-major over the grid.
    cells: CellGrid<u8>,
    /// Precomputed CSR neighbour table (flat row-major indices).
    adjacency: Adjacency,
}

impl VoterWorld {
    /// A `rows × cols` toroidal lattice seeded with `n_opinions` random opinions.
    fn new(rows: usize, cols: usize, n_opinions: u8, rng: &mut socsim_core::SimRng) -> Self {
        let grid = Grid::new(rows, cols, Boundary::Toroidal);
        // Precompute the Moore (8-neighbour) adjacency once; reused every tick.
        let adjacency = grid.adjacency(Neighborhood::Moore);
        let cells = CellGrid::from_fn(grid, |_r, _c| rng.gen_range(0..n_opinions));
        Self {
            clock: SimClock::new(0), // t_max set by the caller below
            cells,
            adjacency,
        }
    }

    /// Number of distinct opinions currently on the lattice. Equals 1 at
    /// consensus (the absorbing state we stop on).
    fn distinct_opinions(&self) -> usize {
        let mut seen = [false; 256];
        let mut count = 0;
        for &v in self.cells.cells() {
            if !seen[v as usize] {
                seen[v as usize] = true;
                count += 1;
            }
        }
        count
    }
}

// A `CellGrid`-based world has no per-agent roster; the "agents" are cells, and
// we drive them en masse from one mechanism rather than via the scheduler.
impl WorldState for VoterWorld {
    fn agent_ids(&self) -> Vec<AgentId> {
        Vec::new()
    }
    fn clock(&self) -> &SimClock {
        &self.clock
    }
    fn clock_mut(&mut self) -> &mut SimClock {
        &mut self.clock
    }
}

// ── Mechanism: batch many voter events into one tick ─────────────────────────
//
// This is the event→tick mapping. A pure event-driven (Gillespie-style) voter
// model has no natural notion of a global "step": it just keeps firing single
// updates. We batch `events_per_step` of those micro-events inside ONE
// `apply()` call, so one engine tick == `events_per_step` voter events. This
// keeps observables (and stop checks) at a sane cadence while preserving the
// asynchronous, one-cell-at-a-time update semantics.

struct VoterModel {
    events_per_step: usize,
}

impl Mechanism<VoterWorld> for VoterModel {
    fn name(&self) -> &str {
        "voter"
    }

    fn phases(&self) -> &'static [Phase] {
        &[Phase::Interaction]
    }

    fn apply(&mut self, _phase: Phase, ctx: &mut StepContext<'_, VoterWorld>) -> Result<()> {
        let n = ctx.world.cells.len();
        if n == 0 {
            return Ok(());
        }

        // Batch of micro-events, all driven by ctx.rng for reproducibility.
        for _ in 0..self.events_per_step {
            // Pick a random cell and a random one of its (precomputed) neighbours,
            // then copy the neighbour's opinion. O(1) slice lookup, no allocation.
            let idx = ctx.rng.gen_range(0..n);
            let nbrs = ctx.world.adjacency.neighbors(idx);
            if nbrs.is_empty() {
                continue;
            }
            let nbr = nbrs[ctx.rng.gen_range(0..nbrs.len())];
            let opinion = *ctx
                .world
                .cells
                .get_idx(nbr)
                .expect("adjacency yields in-range indices");
            if let Some(cell) = ctx.world.cells.get_idx_mut(idx) {
                *cell = opinion;
            }
        }

        // Absorbing state: once everyone agrees, ask the engine to stop.
        if ctx.world.distinct_opinions() <= 1 {
            ctx.request_stop();
        }
        Ok(())
    }
}

fn main() {
    // One root seed; derive labelled child streams (issue #16 convention):
    //   [0] → world initialisation, [1] → engine / scheduler.
    let root = 7u64;
    let mut init_rng = socsim_core::SimRng::from_seed(socsim_core::derive_seed(root, &[0]));

    let mut world = VoterWorld::new(16, 16, 4, &mut init_rng);
    world.clock = SimClock::new(500); // safety cap; consensus usually arrives first

    let mut sim = SimulationBuilder::new(world)
        // Default recorder is NullRecorder — no socsim-log dependency needed.
        .seed(socsim_core::derive_seed(root, &[1]))
        .add_mechanism(Box::new(VoterModel {
            events_per_step: 256,
        }))
        .build();

    println!("=== socsim cellular_automata (voter model) ===");
    println!("16x16 toroidal lattice, 4 opinions, 256 events/tick\n");
    println!("  t   distinct opinions");
    println!("  ---------------------");

    // run_observed: collect a per-step metric (distinct opinions) ergonomically.
    sim.run_observed(|report| {
        let distinct = report.world.distinct_opinions();
        if report.t <= 5 || report.t % 10 == 0 || report.stopped {
            println!("  {:>3}  {}", report.t, distinct);
        }
    })
    .expect("simulation completed");

    let final_distinct = sim.world().distinct_opinions();
    println!();
    if sim.stop_requested() {
        println!(
            "reached consensus at t = {} (distinct = {})",
            sim.world().clock().t(),
            final_distinct,
        );
    } else {
        println!(
            "hit safety cap t_max = {} without consensus (distinct = {})",
            sim.world().clock().t_max(),
            final_distinct,
        );
    }
}
