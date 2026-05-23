//! 2D lattice, neighborhoods and spatial indexing for the `socsim` platform.
//!
//! This crate provides reusable spatial abstractions for agent-based models
//! laid out on a regular 2D grid:
//!
//! - [`Neighborhood`] — Moore (8) vs. Von Neumann (4) adjacency.
//! - [`Boundary`] — fixed (clipping) vs. toroidal (wrap-around) edges.
//! - [`Metric`] — Chebyshev / Manhattan / Euclidean distance functions.
//! - [`Grid`] — a `rows × cols` lattice with boundary-aware neighbor and
//!   distance queries.
//! - [`GridIndex`] — an occupancy / spatial index layered over a [`Grid`] that
//!   maps [`AgentId`]s to cells and back.
//! - [`CellGrid`] — a generic, row-major per-cell state container over a
//!   [`Grid`], the primitive for cellular-automata / lattice-attribute models.
//! - [`Adjacency`] — a precomputed CSR neighbor table for hot lattice loops.
//! - [`GridError`] — error type returned by mutating [`GridIndex`] operations.
//!
//! For performance-sensitive code the crate also offers non-allocating neighbor
//! queries: [`Grid::neighbors_into`] / [`Grid::neighbors_radius_into`] reuse a
//! caller-supplied buffer, [`Grid::neighbors_iter`] yields radius-1 neighbors
//! without a heap allocation, and [`Grid::adjacency`] precomputes the whole
//! neighbor table once for repeated lookups.
//!
//! All neighbor and cell listings are returned in a deterministic order
//! (sorted / row-major) so that simulations built on this crate are
//! reproducible regardless of internal iteration order.

use std::collections::BTreeMap;

use socsim_core::AgentId;

// ── Neighborhood ───────────────────────────────────────────────────────────────

/// The adjacency pattern used when enumerating the neighbors of a cell.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Neighborhood {
    /// All 8 surrounding cells (orthogonal + diagonal) at radius 1.
    Moore,
    /// The 4 orthogonally adjacent cells (no diagonals) at radius 1.
    VonNeumann,
}

// ── Boundary ───────────────────────────────────────────────────────────────────

/// How the grid behaves at its edges.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Boundary {
    /// Cells beyond the edge do not exist; neighbor queries clip them away.
    Fixed,
    /// The grid is periodic: moving off one edge wraps to the opposite edge.
    Toroidal,
}

// ── Metric ─────────────────────────────────────────────────────────────────────

/// Distance metrics defined over integer grid coordinates.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Metric {
    /// Chebyshev (L∞) distance — `max(|dr|, |dc|)`.  Pairs with [`Neighborhood::Moore`].
    Chebyshev,
    /// Manhattan (L1) distance — `|dr| + |dc|`.  Pairs with [`Neighborhood::VonNeumann`].
    Manhattan,
    /// Euclidean (L2) distance — `sqrt(dr² + dc²)`.
    Euclidean,
}

impl Metric {
    /// Non-wrapping distance between two cells `a` and `b`.
    ///
    /// This treats coordinates as living on an infinite plane; it does **not**
    /// account for toroidal wrap-around.  For wrap-aware distances use
    /// [`Grid::distance`], which delegates here for fixed grids.
    pub fn distance(&self, a: (usize, usize), b: (usize, usize)) -> f64 {
        let dr = abs_diff(a.0, b.0);
        let dc = abs_diff(a.1, b.1);
        self.combine(dr, dc)
    }

    /// Combine the per-axis absolute differences into a scalar distance.
    fn combine(&self, dr: usize, dc: usize) -> f64 {
        match self {
            Metric::Chebyshev => dr.max(dc) as f64,
            Metric::Manhattan => (dr + dc) as f64,
            Metric::Euclidean => (((dr * dr) + (dc * dc)) as f64).sqrt(),
        }
    }
}

/// Absolute difference between two `usize` values without overflow.
fn abs_diff(a: usize, b: usize) -> usize {
    a.abs_diff(b)
}

// ── Grid ─────────────────────────────────────────────────────────────────────

/// A `rows × cols` rectangular lattice with a fixed [`Boundary`] behavior.
///
/// The grid itself stores no occupancy data — it is a pure geometric helper
/// answering neighbor and distance queries.  Layer a [`GridIndex`] on top to
/// track which [`AgentId`] sits in which cell.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Grid {
    rows: usize,
    cols: usize,
    boundary: Boundary,
}

impl Grid {
    /// Create a new `rows × cols` grid with the given boundary behavior.
    pub fn new(rows: usize, cols: usize, boundary: Boundary) -> Self {
        Self {
            rows,
            cols,
            boundary,
        }
    }

    /// Number of rows.
    pub fn rows(&self) -> usize {
        self.rows
    }

    /// Number of columns.
    pub fn cols(&self) -> usize {
        self.cols
    }

    /// The boundary behavior of this grid.
    pub fn boundary(&self) -> Boundary {
        self.boundary
    }

    /// Total number of cells (`rows * cols`).
    pub fn len(&self) -> usize {
        self.rows * self.cols
    }

    /// Returns `true` if the grid has no cells.
    pub fn is_empty(&self) -> bool {
        self.rows == 0 || self.cols == 0
    }

    /// Returns `true` if `(r, c)` lies inside the grid.
    ///
    /// Accepts signed coordinates so callers can probe one step off the edge
    /// without underflowing `usize`.
    pub fn in_bounds(&self, r: isize, c: isize) -> bool {
        r >= 0 && c >= 0 && (r as usize) < self.rows && (c as usize) < self.cols
    }

    /// Resolve a signed candidate coordinate to a valid cell, honoring the
    /// boundary.
    ///
    /// - Under [`Boundary::Fixed`]: returns `None` when out of bounds.
    /// - Under [`Boundary::Toroidal`]: wraps into range and returns `Some`.
    fn resolve(&self, r: isize, c: isize) -> Option<(usize, usize)> {
        if self.rows == 0 || self.cols == 0 {
            return None;
        }
        match self.boundary {
            Boundary::Fixed => {
                if self.in_bounds(r, c) {
                    Some((r as usize, c as usize))
                } else {
                    None
                }
            }
            Boundary::Toroidal => {
                let rr = wrap(r, self.rows);
                let cc = wrap(c, self.cols);
                Some((rr, cc))
            }
        }
    }

    /// Radius-1 neighbors of `(r, c)` under the given neighborhood.
    ///
    /// Equivalent to [`Grid::neighbors_radius`] with `radius = 1`.  The
    /// returned list is sorted (row-major) and never includes the center.
    ///
    /// This allocates a fresh `Vec` per call; for hot loops prefer
    /// [`Grid::neighbors_into`] (reuse a buffer), [`Grid::neighbors_iter`]
    /// (no heap allocation), or [`Grid::adjacency`] (precomputed table).
    pub fn neighbors(&self, r: usize, c: usize, nbhd: Neighborhood) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        self.neighbors_into(r, c, nbhd, &mut out);
        out
    }

    /// Like [`Grid::neighbors`], but writes into a caller-supplied buffer.
    ///
    /// `out` is cleared and then filled with the same neighbors in the same
    /// order (sorted row-major, center excluded, boundary-aware).  Reusing a
    /// single `Vec` across many calls avoids a per-call heap allocation.
    pub fn neighbors_into(
        &self,
        r: usize,
        c: usize,
        nbhd: Neighborhood,
        out: &mut Vec<(usize, usize)>,
    ) {
        self.neighbors_radius_into(r, c, nbhd, 1, out);
    }

    /// All cells within `radius` of `(r, c)` under the given neighborhood.
    ///
    /// - [`Neighborhood::Moore`] yields the Chebyshev ball (a square) of the
    ///   given radius: `max(|dr|, |dc|) <= radius`.
    /// - [`Neighborhood::VonNeumann`] yields the Manhattan ball (a diamond):
    ///   `|dr| + |dc| <= radius`.
    ///
    /// The center cell is never included.  Out-of-range cells are clipped under
    /// [`Boundary::Fixed`] and wrapped under [`Boundary::Toroidal`].  On small
    /// toroidal grids a single offset may resolve to the same cell as another;
    /// such duplicates (and any accidental wrap onto the center) are removed.
    /// The result is sorted row-major for determinism.
    pub fn neighbors_radius(
        &self,
        r: usize,
        c: usize,
        nbhd: Neighborhood,
        radius: usize,
    ) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        self.neighbors_radius_into(r, c, nbhd, radius, &mut out);
        out
    }

    /// Like [`Grid::neighbors_radius`], but writes into a caller-supplied
    /// buffer.
    ///
    /// `out` is cleared and then filled with the same cells in the same order
    /// (sorted row-major, center excluded, boundary-aware, deduplicated).
    /// Reusing a single `Vec` across many calls avoids a per-call heap
    /// allocation.
    pub fn neighbors_radius_into(
        &self,
        r: usize,
        c: usize,
        nbhd: Neighborhood,
        radius: usize,
        out: &mut Vec<(usize, usize)>,
    ) {
        out.clear();
        if radius == 0 || self.is_empty() {
            return;
        }

        let rad = radius as isize;

        for dr in -rad..=rad {
            for dc in -rad..=rad {
                if dr == 0 && dc == 0 {
                    continue; // never include the center offset
                }
                // Shape test: square (Moore) vs. diamond (Von Neumann).
                let in_shape = match nbhd {
                    Neighborhood::Moore => dr.unsigned_abs().max(dc.unsigned_abs()) <= radius,
                    Neighborhood::VonNeumann => dr.unsigned_abs() + dc.unsigned_abs() <= radius,
                };
                if !in_shape {
                    continue;
                }
                if let Some(cell) = self.resolve(r as isize + dr, c as isize + dc) {
                    // Guard against a toroidal wrap landing back on the center.
                    if cell != (r, c) {
                        out.push(cell);
                    }
                }
            }
        }

        out.sort_unstable();
        out.dedup();
    }

    /// A non-allocating iterator over the radius-1 neighbors of `(r, c)`.
    ///
    /// Yields the same cells in the same order as [`Grid::neighbors`] (sorted
    /// row-major, center excluded, boundary-aware, deduplicated) but without
    /// allocating a heap `Vec`.
    ///
    /// # Scope
    ///
    /// This is **radius-1 only** — a radius-1 [`Neighborhood::Moore`] cell has
    /// at most 8 neighbors and Von Neumann at most 4, so the candidates are
    /// computed into a fixed-size stack array and the iterator borrows from it.
    /// For larger radii use [`Grid::neighbors_radius`] /
    /// [`Grid::neighbors_radius_into`], which handle the general case.
    pub fn neighbors_iter(
        &self,
        r: usize,
        c: usize,
        nbhd: Neighborhood,
    ) -> impl Iterator<Item = (usize, usize)> + '_ {
        NeighborsIter::new(self, r, c, nbhd)
    }

    /// Wrap-aware distance between two cells under the given [`Metric`].
    ///
    /// Under [`Boundary::Fixed`] this is identical to [`Metric::distance`].
    /// Under [`Boundary::Toroidal`] each axis takes the shorter of the direct
    /// and wrapped separation, so e.g. on a 10-wide toroidal grid columns `0`
    /// and `9` are Manhattan distance `1` apart.
    pub fn distance(&self, metric: Metric, a: (usize, usize), b: (usize, usize)) -> f64 {
        match self.boundary {
            Boundary::Fixed => metric.distance(a, b),
            Boundary::Toroidal => {
                let dr = toroidal_diff(a.0, b.0, self.rows);
                let dc = toroidal_diff(a.1, b.1, self.cols);
                metric.combine(dr, dc)
            }
        }
    }

    /// Precompute the radius-1 adjacency table for the whole grid.
    ///
    /// Equivalent to [`Grid::adjacency_radius`] with `radius = 1`.
    pub fn adjacency(&self, nbhd: Neighborhood) -> Adjacency {
        self.adjacency_radius(nbhd, 1)
    }

    /// Precompute the radius-`radius` adjacency table for the whole grid.
    ///
    /// Returns an [`Adjacency`] in CSR (compressed sparse row) layout keyed by
    /// flat cell index `idx = r * cols + c`.  Building it once and reusing
    /// [`Adjacency::neighbors`] is the recommended pattern for hot lattice /
    /// cellular-automata loops, where the same neighbor sets are queried every
    /// tick: it amortizes the offset enumeration, shape test and sort done by
    /// [`Grid::neighbors_radius`] into a single up-front pass and then answers
    /// each lookup with an O(1) slice.
    ///
    /// Each cell's neighbor list preserves the deterministic sorted row-major
    /// order of [`Grid::neighbors_radius`] (now expressed as flat indices).
    pub fn adjacency_radius(&self, nbhd: Neighborhood, radius: usize) -> Adjacency {
        let n = self.len();
        let cols = self.cols;
        let mut offsets = Vec::with_capacity(n + 1);
        let mut flat = Vec::new();
        offsets.push(0usize);

        // Reused scratch buffer so we don't allocate per cell.
        let mut buf: Vec<(usize, usize)> = Vec::new();
        for idx in 0..n {
            let r = idx / cols;
            let c = idx % cols;
            self.neighbors_radius_into(r, c, nbhd, radius, &mut buf);
            for &(nr, nc) in buf.iter() {
                flat.push(nr * cols + nc);
            }
            offsets.push(flat.len());
        }

        Adjacency { offsets, flat }
    }
}

// ── NeighborsIter ────────────────────────────────────────────────────────────

/// Non-allocating iterator backing [`Grid::neighbors_iter`].
///
/// Radius-1 Moore neighborhoods yield at most 8 cells and Von Neumann at most
/// 4, so the (already ordered, deduplicated) candidates are materialized into a
/// fixed `[(usize, usize); 8]` array on the stack — no heap allocation — and
/// the iterator walks that array.
struct NeighborsIter {
    buf: [(usize, usize); 8],
    len: usize,
    pos: usize,
}

impl NeighborsIter {
    fn new(grid: &Grid, r: usize, c: usize, nbhd: Neighborhood) -> Self {
        let mut buf = [(0usize, 0usize); 8];
        let mut len = 0usize;

        if !grid.is_empty() {
            // Enumerate the 3x3 offsets in row-major order so the result is
            // already sorted before deduplication, matching neighbors_into.
            let mut tmp: [(usize, usize); 8] = [(0, 0); 8];
            let mut tmp_len = 0usize;
            for dr in -1isize..=1 {
                for dc in -1isize..=1 {
                    if dr == 0 && dc == 0 {
                        continue;
                    }
                    let in_shape = match nbhd {
                        Neighborhood::Moore => true,
                        Neighborhood::VonNeumann => {
                            dr.unsigned_abs() + dc.unsigned_abs() <= 1
                        }
                    };
                    if !in_shape {
                        continue;
                    }
                    if let Some(cell) = grid.resolve(r as isize + dr, c as isize + dc) {
                        if cell != (r, c) {
                            tmp[tmp_len] = cell;
                            tmp_len += 1;
                        }
                    }
                }
            }
            // Offsets are visited row-major, so tmp[0..tmp_len] is already
            // sorted.  Toroidal grids may produce duplicates; drop adjacent
            // repeats (equivalent to Vec::dedup on sorted data).
            tmp[..tmp_len].sort_unstable();
            for i in 0..tmp_len {
                if i == 0 || tmp[i] != tmp[i - 1] {
                    buf[len] = tmp[i];
                    len += 1;
                }
            }
        }

        Self { buf, len, pos: 0 }
    }
}

impl Iterator for NeighborsIter {
    type Item = (usize, usize);

    fn next(&mut self) -> Option<Self::Item> {
        if self.pos < self.len {
            let v = self.buf[self.pos];
            self.pos += 1;
            Some(v)
        } else {
            None
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let rem = self.len - self.pos;
        (rem, Some(rem))
    }
}

impl ExactSizeIterator for NeighborsIter {}

// ── Adjacency ──────────────────────────────────────────────────────────────────

/// A precomputed neighbor table for a [`Grid`], in CSR (compressed sparse row)
/// layout.
///
/// Cells are addressed by their flat row-major index `idx = r * cols + c`, and
/// each cell's neighbors are likewise stored as flat indices.  `offsets` has
/// length `n_cells + 1`; the neighbors of cell `idx` are
/// `flat[offsets[idx] .. offsets[idx + 1]]`.
///
/// Build one with [`Grid::adjacency`] / [`Grid::adjacency_radius`].  This is the
/// recommended structure for hot loops (lattice / cellular-automata models)
/// that query the same neighbor sets every tick, replacing repeated allocating
/// [`Grid::neighbors`] calls with O(1) slice lookups.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Adjacency {
    /// CSR row offsets; length is `n_cells + 1`.
    offsets: Vec<usize>,
    /// Flattened neighbor cell indices (`r * cols + c`) for every cell, in CSR
    /// order.
    flat: Vec<usize>,
}

impl Adjacency {
    /// The flat neighbor indices of cell `idx` (`idx = r * cols + c`).
    ///
    /// Returned as an O(1) borrowed slice, in the deterministic sorted
    /// row-major order produced by [`Grid::neighbors_radius`].
    ///
    /// # Panics
    /// Panics if `idx` is out of range (`idx >= self.len()`).
    pub fn neighbors(&self, idx: usize) -> &[usize] {
        let start = self.offsets[idx];
        let end = self.offsets[idx + 1];
        &self.flat[start..end]
    }

    /// The number of cells the table covers.
    pub fn len(&self) -> usize {
        self.offsets.len().saturating_sub(1)
    }

    /// Returns `true` if the table covers no cells.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Wrap a signed coordinate into `[0, len)` (Euclidean modulo).
fn wrap(v: isize, len: usize) -> usize {
    let len_i = len as isize;
    (((v % len_i) + len_i) % len_i) as usize
}

/// Shortest separation between `a` and `b` on a single toroidal axis of size
/// `len`.
fn toroidal_diff(a: usize, b: usize, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    let d = abs_diff(a, b);
    d.min(len - d)
}

// ── GridError ──────────────────────────────────────────────────────────────────

/// Errors returned by mutating [`GridIndex`] operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GridError {
    /// A target coordinate lies outside the grid.
    OutOfBounds {
        /// The offending row.
        r: usize,
        /// The offending column.
        c: usize,
        /// The grid row count.
        rows: usize,
        /// The grid column count.
        cols: usize,
    },
    /// The target cell is already occupied by another agent.
    CellOccupied {
        /// The contested row.
        r: usize,
        /// The contested column.
        c: usize,
        /// The agent currently sitting in the cell.
        occupant: AgentId,
    },
    /// The referenced agent is not present in the index.
    UnknownAgent(AgentId),
}

impl std::fmt::Display for GridError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GridError::OutOfBounds { r, c, rows, cols } => write!(
                f,
                "cell ({r}, {c}) is out of bounds for a {rows}x{cols} grid"
            ),
            GridError::CellOccupied { r, c, occupant } => {
                write!(f, "cell ({r}, {c}) is already occupied by {occupant:?}")
            }
            GridError::UnknownAgent(id) => write!(f, "unknown agent: {id:?}"),
        }
    }
}

impl std::error::Error for GridError {}

// ── GridIndex ──────────────────────────────────────────────────────────────────

/// An occupancy / spatial index layered over a [`Grid`].
///
/// Maintains a bidirectional mapping between [`AgentId`]s and cells:
///
/// - a row-major `Vec<Option<AgentId>>` for O(1) "who is in this cell?" lookups,
/// - a [`BTreeMap`] for O(log n), sorted "where is this agent?" lookups.
///
/// At most one agent may occupy a cell at a time.
pub struct GridIndex {
    grid: Grid,
    /// Row-major occupancy: `cells[r * cols + c]`.
    cells: Vec<Option<AgentId>>,
    /// Position of every placed agent.  Sorted by `AgentId` for determinism.
    positions: BTreeMap<AgentId, (usize, usize)>,
}

impl GridIndex {
    /// Create an empty index over `grid` (all cells vacant).
    pub fn new(grid: Grid) -> Self {
        let n = grid.len();
        Self {
            grid,
            cells: vec![None; n],
            positions: BTreeMap::new(),
        }
    }

    /// Borrow the underlying [`Grid`].
    pub fn grid(&self) -> &Grid {
        &self.grid
    }

    /// Linear row-major index for `(r, c)`.
    fn idx(&self, r: usize, c: usize) -> usize {
        r * self.grid.cols + c
    }

    /// Validate that `(r, c)` is in bounds, returning a [`GridError`] otherwise.
    fn check_bounds(&self, r: usize, c: usize) -> std::result::Result<(), GridError> {
        if r < self.grid.rows && c < self.grid.cols {
            Ok(())
        } else {
            Err(GridError::OutOfBounds {
                r,
                c,
                rows: self.grid.rows,
                cols: self.grid.cols,
            })
        }
    }

    /// Place `id` at `(r, c)`.
    ///
    /// # Errors
    /// - [`GridError::OutOfBounds`] if the cell is outside the grid.
    /// - [`GridError::CellOccupied`] if another agent already sits there.
    ///
    /// If `id` was already placed elsewhere, this still places it at `(r, c)`
    /// only when the target is free; the old cell is **not** vacated (use
    /// [`GridIndex::move_to`] to relocate an existing agent).
    pub fn place(&mut self, id: AgentId, r: usize, c: usize) -> std::result::Result<(), GridError> {
        self.check_bounds(r, c)?;
        let i = self.idx(r, c);
        if let Some(occupant) = self.cells[i] {
            return Err(GridError::CellOccupied { r, c, occupant });
        }
        self.cells[i] = Some(id);
        self.positions.insert(id, (r, c));
        Ok(())
    }

    /// The agent occupying `(r, c)`, if any.
    ///
    /// Returns `None` for vacant or out-of-bounds cells.
    pub fn occupant(&self, r: usize, c: usize) -> Option<AgentId> {
        if r < self.grid.rows && c < self.grid.cols {
            self.cells[self.idx(r, c)]
        } else {
            None
        }
    }

    /// The cell occupied by `id`, if it is placed.
    pub fn position(&self, id: AgentId) -> Option<(usize, usize)> {
        self.positions.get(&id).copied()
    }

    /// Move `id` from its current cell to `(r, c)`.
    ///
    /// Vacates the old cell and fills the new one.
    ///
    /// # Errors
    /// - [`GridError::UnknownAgent`] if `id` is not currently placed.
    /// - [`GridError::OutOfBounds`] if the target is outside the grid.
    /// - [`GridError::CellOccupied`] if a *different* agent holds the target.
    ///   Moving an agent onto its own current cell is a no-op success.
    pub fn move_to(
        &mut self,
        id: AgentId,
        r: usize,
        c: usize,
    ) -> std::result::Result<(), GridError> {
        let old = self
            .positions
            .get(&id)
            .copied()
            .ok_or(GridError::UnknownAgent(id))?;
        self.check_bounds(r, c)?;

        let target = self.idx(r, c);
        if let Some(occupant) = self.cells[target] {
            if occupant != id {
                return Err(GridError::CellOccupied { r, c, occupant });
            }
            // Same agent, same cell → no-op.
            return Ok(());
        }

        let old_i = self.idx(old.0, old.1);
        self.cells[old_i] = None;
        self.cells[target] = Some(id);
        self.positions.insert(id, (r, c));
        Ok(())
    }

    /// All vacant cells, in row-major order.
    pub fn vacant_cells(&self) -> Vec<(usize, usize)> {
        let cols = self.grid.cols;
        self.cells
            .iter()
            .enumerate()
            .filter(|(_, occ)| occ.is_none())
            .map(|(i, _)| (i / cols, i % cols))
            .collect()
    }

    /// All placed agents, sorted by [`AgentId`].
    pub fn agent_ids(&self) -> Vec<AgentId> {
        // BTreeMap iterates in key order, so this is already sorted.
        self.positions.keys().copied().collect()
    }

    /// The nearest vacant cell to `from` under `metric`, wrap-aware via the
    /// grid.
    ///
    /// Ties are broken deterministically by row-major order (the first such
    /// cell encountered when scanning rows then columns).  Returns `None` if
    /// the grid is fully occupied (or empty).
    pub fn nearest_vacant(&self, from: (usize, usize), metric: Metric) -> Option<(usize, usize)> {
        let mut best: Option<((usize, usize), f64)> = None;
        for cell in self.vacant_cells() {
            // vacant_cells() is row-major, so the first cell at a given distance
            // wins the tie naturally with a strict `<` comparison.
            let d = self.grid.distance(metric, from, cell);
            match best {
                Some((_, bd)) if d < bd => best = Some((cell, d)),
                None => best = Some((cell, d)),
                _ => {}
            }
        }
        best.map(|(cell, _)| cell)
    }

    /// The agents occupying the radius-1 neighbors of `(r, c)`.
    ///
    /// Returned in the deterministic (sorted, row-major) neighbor order of the
    /// underlying grid; vacant neighbor cells are skipped.
    pub fn occupant_neighbors(&self, r: usize, c: usize, nbhd: Neighborhood) -> Vec<AgentId> {
        self.grid
            .neighbors(r, c, nbhd)
            .into_iter()
            .filter_map(|(nr, nc)| self.occupant(nr, nc))
            .collect()
    }
}

// ── CellGrid ───────────────────────────────────────────────────────────────────

/// A generic, row-major per-cell state container layered over a [`Grid`].
///
/// Where [`GridIndex`] tracks *which agent* occupies each cell, `CellGrid<T>`
/// stores an arbitrary value `T` for *every* cell.  It is the primitive for
/// cellular-automata and lattice-attribute models — e.g. each cell holding a
/// strategy, opinion, or counter — and combines the [`Grid`]'s boundary-aware
/// neighbor queries with direct mutable access to the underlying `Vec<T>`.
///
/// Cells are stored row-major: cell `(r, c)` lives at flat index
/// `r * cols + c`, matching [`Grid`] and [`Adjacency`].
#[derive(Clone, Debug)]
pub struct CellGrid<T> {
    grid: Grid,
    /// Row-major per-cell state: `cells[r * cols + c]`.
    cells: Vec<T>,
}

impl<T: Clone> CellGrid<T> {
    /// Create a `CellGrid` over `grid` with every cell initialized to
    /// `fill.clone()`.
    pub fn new(grid: Grid, fill: T) -> Self {
        let n = grid.len();
        Self {
            grid,
            cells: vec![fill; n],
        }
    }
}

impl<T> CellGrid<T> {
    /// Create a `CellGrid` over `grid`, computing each cell's value from its
    /// coordinates via `f(r, c)`.
    ///
    /// Cells are visited in row-major order.
    pub fn from_fn(grid: Grid, mut f: impl FnMut(usize, usize) -> T) -> Self {
        let cols = grid.cols();
        let n = grid.len();
        let mut cells = Vec::with_capacity(n);
        for idx in 0..n {
            cells.push(f(idx / cols, idx % cols));
        }
        Self { grid, cells }
    }

    /// Borrow the underlying [`Grid`].
    pub fn grid(&self) -> &Grid {
        &self.grid
    }

    /// Number of rows.
    pub fn rows(&self) -> usize {
        self.grid.rows()
    }

    /// Number of columns.
    pub fn cols(&self) -> usize {
        self.grid.cols()
    }

    /// Total number of cells (`rows * cols`).
    pub fn len(&self) -> usize {
        self.cells.len()
    }

    /// Returns `true` if the grid has no cells.
    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// Flat row-major index for `(r, c)` (`r * cols + c`).
    ///
    /// Does not bounds-check; callers passing untrusted coordinates should use
    /// [`CellGrid::get`] / [`CellGrid::get_mut`] instead.
    pub fn index(&self, r: usize, c: usize) -> usize {
        r * self.grid.cols() + c
    }

    /// Borrow the value at `(r, c)`, or `None` if out of bounds.
    pub fn get(&self, r: usize, c: usize) -> Option<&T> {
        if r < self.grid.rows() && c < self.grid.cols() {
            self.cells.get(self.index(r, c))
        } else {
            None
        }
    }

    /// Mutably borrow the value at `(r, c)`, or `None` if out of bounds.
    pub fn get_mut(&mut self, r: usize, c: usize) -> Option<&mut T> {
        if r < self.grid.rows() && c < self.grid.cols() {
            let i = self.index(r, c);
            self.cells.get_mut(i)
        } else {
            None
        }
    }

    /// Borrow the value at flat index `idx`, or `None` if out of range.
    pub fn get_idx(&self, idx: usize) -> Option<&T> {
        self.cells.get(idx)
    }

    /// Mutably borrow the value at flat index `idx`, or `None` if out of range.
    pub fn get_idx_mut(&mut self, idx: usize) -> Option<&mut T> {
        self.cells.get_mut(idx)
    }

    /// Borrow all cell values as a row-major slice.
    pub fn cells(&self) -> &[T] {
        &self.cells
    }

    /// Mutably borrow all cell values as a row-major slice.
    pub fn cells_mut(&mut self) -> &mut [T] {
        &mut self.cells
    }

    /// The radius-1 neighbor coordinates of `(r, c)` (delegates to the
    /// underlying [`Grid::neighbors`]).
    pub fn neighbors(&self, r: usize, c: usize, nbhd: Neighborhood) -> Vec<(usize, usize)> {
        self.grid.neighbors(r, c, nbhd)
    }

    /// The values stored at the radius-1 neighbors of `(r, c)`.
    ///
    /// Returned in the deterministic (sorted, row-major) neighbor order of the
    /// underlying grid.
    pub fn neighbor_values(&self, r: usize, c: usize, nbhd: Neighborhood) -> Vec<&T> {
        self.grid
            .neighbors(r, c, nbhd)
            .into_iter()
            .filter_map(|(nr, nc)| self.get(nr, nc))
            .collect()
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── neighbors (radius 1) ────────────────────────────────────────────────────

    #[test]
    fn moore_interior_has_eight() {
        let g = Grid::new(5, 5, Boundary::Fixed);
        assert_eq!(g.neighbors(2, 2, Neighborhood::Moore).len(), 8);
    }

    #[test]
    fn moore_corner_fixed_has_three() {
        let g = Grid::new(5, 5, Boundary::Fixed);
        assert_eq!(g.neighbors(0, 0, Neighborhood::Moore).len(), 3);
    }

    #[test]
    fn moore_corner_toroidal_has_eight() {
        let g = Grid::new(5, 5, Boundary::Toroidal);
        assert_eq!(g.neighbors(0, 0, Neighborhood::Moore).len(), 8);
    }

    #[test]
    fn von_neumann_interior_has_four() {
        let g = Grid::new(5, 5, Boundary::Fixed);
        assert_eq!(g.neighbors(2, 2, Neighborhood::VonNeumann).len(), 4);
    }

    #[test]
    fn von_neumann_corner_fixed_has_two() {
        let g = Grid::new(5, 5, Boundary::Fixed);
        assert_eq!(g.neighbors(0, 0, Neighborhood::VonNeumann).len(), 2);
    }

    #[test]
    fn von_neumann_corner_toroidal_has_four() {
        let g = Grid::new(5, 5, Boundary::Toroidal);
        assert_eq!(g.neighbors(0, 0, Neighborhood::VonNeumann).len(), 4);
    }

    #[test]
    fn neighbors_are_sorted() {
        let g = Grid::new(5, 5, Boundary::Fixed);
        let n = g.neighbors(2, 2, Neighborhood::Moore);
        let mut sorted = n.clone();
        sorted.sort_unstable();
        assert_eq!(n, sorted);
    }

    #[test]
    fn neighbors_never_include_center() {
        let g = Grid::new(5, 5, Boundary::Toroidal);
        for nbhd in [Neighborhood::Moore, Neighborhood::VonNeumann] {
            assert!(!g.neighbors(2, 2, nbhd).contains(&(2, 2)));
            assert!(!g
                .neighbors_radius(1, 1, nbhd, 3)
                .contains(&(1, 1)));
        }
    }

    // ── neighbors_radius ─────────────────────────────────────────────────────────

    #[test]
    fn moore_radius_two_interior_has_24() {
        let g = Grid::new(9, 9, Boundary::Fixed);
        assert_eq!(
            g.neighbors_radius(4, 4, Neighborhood::Moore, 2).len(),
            24 // 5x5 square minus the center
        );
    }

    #[test]
    fn von_neumann_radius_two_interior_has_12() {
        let g = Grid::new(9, 9, Boundary::Fixed);
        assert_eq!(
            g.neighbors_radius(4, 4, Neighborhood::VonNeumann, 2).len(),
            12 // diamond of radius 2 minus the center
        );
    }

    #[test]
    fn radius_zero_is_empty() {
        let g = Grid::new(5, 5, Boundary::Fixed);
        assert!(g.neighbors_radius(2, 2, Neighborhood::Moore, 0).is_empty());
    }

    // ── toroidal wrap ────────────────────────────────────────────────────────────

    #[test]
    fn toroidal_neighbor_of_origin_wraps() {
        let g = Grid::new(6, 6, Boundary::Toroidal);
        let n = g.neighbors(0, 0, Neighborhood::Moore);
        // Wrapping should reach the last row and last column.
        assert!(n.contains(&(5, 0)));
        assert!(n.contains(&(0, 5)));
        assert!(n.contains(&(5, 5)));
    }

    #[test]
    fn toroidal_two_by_two_no_duplicates() {
        let g = Grid::new(2, 2, Boundary::Toroidal);
        let n = g.neighbors(0, 0, Neighborhood::Moore);
        let mut deduped = n.clone();
        deduped.sort_unstable();
        deduped.dedup();
        assert_eq!(n.len(), deduped.len(), "neighbors must be unique");
        // On a 2x2 torus the only other cells are (0,1), (1,0), (1,1).
        assert_eq!(n, vec![(0, 1), (1, 0), (1, 1)]);
    }

    // ── distances ────────────────────────────────────────────────────────────────

    #[test]
    fn metric_basic_values() {
        assert_eq!(Metric::Chebyshev.distance((0, 0), (3, 4)), 4.0);
        assert_eq!(Metric::Manhattan.distance((0, 0), (3, 4)), 7.0);
        assert_eq!(Metric::Euclidean.distance((0, 0), (3, 4)), 5.0);
    }

    #[test]
    fn fixed_grid_distance_matches_metric() {
        let g = Grid::new(10, 10, Boundary::Fixed);
        assert_eq!(g.distance(Metric::Manhattan, (0, 0), (0, 9)), 9.0);
    }

    #[test]
    fn toroidal_distance_wraps_short() {
        let g = Grid::new(10, 10, Boundary::Toroidal);
        // Columns 0 and 9 are adjacent across the wrap.
        assert_eq!(g.distance(Metric::Manhattan, (0, 0), (0, 9)), 1.0);
        assert_eq!(g.distance(Metric::Chebyshev, (0, 0), (0, 9)), 1.0);
        assert_eq!(g.distance(Metric::Euclidean, (0, 0), (0, 9)), 1.0);
    }

    #[test]
    fn toroidal_distance_both_axes() {
        let g = Grid::new(10, 10, Boundary::Toroidal);
        // Row diff: min(9, 1) = 1; col diff: min(9, 1) = 1.
        assert_eq!(g.distance(Metric::Manhattan, (0, 0), (9, 9)), 2.0);
    }

    // ── GridIndex ────────────────────────────────────────────────────────────────

    #[test]
    fn place_and_query() {
        let mut idx = GridIndex::new(Grid::new(4, 4, Boundary::Fixed));
        idx.place(AgentId(1), 1, 2).unwrap();
        assert_eq!(idx.occupant(1, 2), Some(AgentId(1)));
        assert_eq!(idx.position(AgentId(1)), Some((1, 2)));
        assert_eq!(idx.occupant(0, 0), None);
        assert_eq!(idx.position(AgentId(2)), None);
    }

    #[test]
    fn place_on_occupied_errors() {
        let mut idx = GridIndex::new(Grid::new(4, 4, Boundary::Fixed));
        idx.place(AgentId(1), 1, 1).unwrap();
        let err = idx.place(AgentId(2), 1, 1).unwrap_err();
        assert_eq!(
            err,
            GridError::CellOccupied {
                r: 1,
                c: 1,
                occupant: AgentId(1)
            }
        );
    }

    #[test]
    fn place_out_of_bounds_errors() {
        let mut idx = GridIndex::new(Grid::new(4, 4, Boundary::Fixed));
        let err = idx.place(AgentId(1), 4, 0).unwrap_err();
        assert_eq!(
            err,
            GridError::OutOfBounds {
                r: 4,
                c: 0,
                rows: 4,
                cols: 4
            }
        );
    }

    #[test]
    fn move_to_happy_path() {
        let mut idx = GridIndex::new(Grid::new(4, 4, Boundary::Fixed));
        idx.place(AgentId(1), 0, 0).unwrap();
        idx.move_to(AgentId(1), 3, 3).unwrap();
        assert_eq!(idx.occupant(0, 0), None);
        assert_eq!(idx.occupant(3, 3), Some(AgentId(1)));
        assert_eq!(idx.position(AgentId(1)), Some((3, 3)));
    }

    #[test]
    fn move_to_self_is_noop() {
        let mut idx = GridIndex::new(Grid::new(4, 4, Boundary::Fixed));
        idx.place(AgentId(1), 2, 2).unwrap();
        idx.move_to(AgentId(1), 2, 2).unwrap();
        assert_eq!(idx.position(AgentId(1)), Some((2, 2)));
    }

    #[test]
    fn move_to_out_of_bounds_errors() {
        let mut idx = GridIndex::new(Grid::new(4, 4, Boundary::Fixed));
        idx.place(AgentId(1), 0, 0).unwrap();
        let err = idx.move_to(AgentId(1), 9, 9).unwrap_err();
        assert_eq!(
            err,
            GridError::OutOfBounds {
                r: 9,
                c: 9,
                rows: 4,
                cols: 4
            }
        );
        // The agent did not move.
        assert_eq!(idx.position(AgentId(1)), Some((0, 0)));
    }

    #[test]
    fn move_to_occupied_errors() {
        let mut idx = GridIndex::new(Grid::new(4, 4, Boundary::Fixed));
        idx.place(AgentId(1), 0, 0).unwrap();
        idx.place(AgentId(2), 1, 1).unwrap();
        let err = idx.move_to(AgentId(1), 1, 1).unwrap_err();
        assert_eq!(
            err,
            GridError::CellOccupied {
                r: 1,
                c: 1,
                occupant: AgentId(2)
            }
        );
    }

    #[test]
    fn move_unknown_agent_errors() {
        let mut idx = GridIndex::new(Grid::new(4, 4, Boundary::Fixed));
        let err = idx.move_to(AgentId(7), 0, 0).unwrap_err();
        assert_eq!(err, GridError::UnknownAgent(AgentId(7)));
    }

    #[test]
    fn vacant_cells_count_after_placements() {
        let mut idx = GridIndex::new(Grid::new(3, 3, Boundary::Fixed));
        assert_eq!(idx.vacant_cells().len(), 9);
        idx.place(AgentId(1), 0, 0).unwrap();
        idx.place(AgentId(2), 1, 1).unwrap();
        assert_eq!(idx.vacant_cells().len(), 7);
        assert!(!idx.vacant_cells().contains(&(0, 0)));
        assert!(!idx.vacant_cells().contains(&(1, 1)));
    }

    #[test]
    fn vacant_cells_are_row_major() {
        let idx = GridIndex::new(Grid::new(2, 2, Boundary::Fixed));
        assert_eq!(idx.vacant_cells(), vec![(0, 0), (0, 1), (1, 0), (1, 1)]);
    }

    #[test]
    fn agent_ids_are_sorted() {
        let mut idx = GridIndex::new(Grid::new(4, 4, Boundary::Fixed));
        idx.place(AgentId(3), 0, 0).unwrap();
        idx.place(AgentId(1), 0, 1).unwrap();
        idx.place(AgentId(2), 0, 2).unwrap();
        assert_eq!(idx.agent_ids(), vec![AgentId(1), AgentId(2), AgentId(3)]);
    }

    #[test]
    fn nearest_vacant_tie_break_row_major() {
        let mut idx = GridIndex::new(Grid::new(3, 3, Boundary::Fixed));
        // Occupy the center and all four orthogonal neighbors, leaving only the
        // four corners vacant — all at Manhattan distance 2 from the center.
        idx.place(AgentId(1), 1, 1).unwrap();
        idx.place(AgentId(2), 0, 1).unwrap();
        idx.place(AgentId(3), 1, 0).unwrap();
        idx.place(AgentId(4), 1, 2).unwrap();
        idx.place(AgentId(5), 2, 1).unwrap();
        // Remaining vacant: (0,0),(0,2),(2,0),(2,2) — all distance 2.
        // Deterministic tie-break by row-major order → (0, 0).
        assert_eq!(
            idx.nearest_vacant((1, 1), Metric::Manhattan),
            Some((0, 0))
        );
    }

    #[test]
    fn nearest_vacant_picks_adjacent_over_far() {
        let mut idx = GridIndex::new(Grid::new(5, 5, Boundary::Fixed));
        // Occupy the whole grid except (4, 4) and (2, 3).
        for r in 0..5 {
            for c in 0..5 {
                if (r, c) != (4, 4) && (r, c) != (2, 3) {
                    idx.place(AgentId((r * 5 + c) as u64), r, c).unwrap();
                }
            }
        }
        // From (2,2): (2,3) is distance 1, (4,4) is distance 4.
        assert_eq!(idx.nearest_vacant((2, 2), Metric::Manhattan), Some((2, 3)));
    }

    #[test]
    fn nearest_vacant_full_grid_is_none() {
        let mut idx = GridIndex::new(Grid::new(2, 2, Boundary::Fixed));
        idx.place(AgentId(0), 0, 0).unwrap();
        idx.place(AgentId(1), 0, 1).unwrap();
        idx.place(AgentId(2), 1, 0).unwrap();
        idx.place(AgentId(3), 1, 1).unwrap();
        assert_eq!(idx.nearest_vacant((0, 0), Metric::Manhattan), None);
    }

    #[test]
    fn occupant_neighbors_lists_adjacent_agents() {
        let mut idx = GridIndex::new(Grid::new(3, 3, Boundary::Fixed));
        idx.place(AgentId(1), 0, 1).unwrap();
        idx.place(AgentId(2), 1, 0).unwrap();
        idx.place(AgentId(3), 2, 2).unwrap(); // not a Von Neumann neighbor of center
        let mut got = idx.occupant_neighbors(1, 1, Neighborhood::VonNeumann);
        got.sort_unstable();
        assert_eq!(got, vec![AgentId(1), AgentId(2)]);
    }

    #[test]
    fn grid_basic_accessors() {
        let g = Grid::new(3, 4, Boundary::Toroidal);
        assert_eq!(g.rows(), 3);
        assert_eq!(g.cols(), 4);
        assert_eq!(g.len(), 12);
        assert!(!g.is_empty());
        assert_eq!(g.boundary(), Boundary::Toroidal);
        assert!(g.in_bounds(2, 3));
        assert!(!g.in_bounds(3, 0));
        assert!(!g.in_bounds(-1, 0));
        assert!(Grid::new(0, 5, Boundary::Fixed).is_empty());
    }

    // ── neighbors_into / neighbors_iter (issue #11) ──────────────────────────────

    #[test]
    fn neighbors_into_matches_neighbors() {
        // Corner, edge, and interior cells under both boundaries and both
        // neighborhoods must produce identical results to the allocating API.
        let cells = [(0, 0), (0, 2), (2, 0), (4, 4), (2, 2)];
        for boundary in [Boundary::Fixed, Boundary::Toroidal] {
            let g = Grid::new(5, 5, boundary);
            for nbhd in [Neighborhood::Moore, Neighborhood::VonNeumann] {
                let mut buf = Vec::new();
                for &(r, c) in &cells {
                    g.neighbors_into(r, c, nbhd, &mut buf);
                    assert_eq!(buf, g.neighbors(r, c, nbhd), "cell {:?} {:?} {:?}", (r, c), boundary, nbhd);
                }
            }
        }
    }

    #[test]
    fn neighbors_radius_into_matches_and_reuses_buffer() {
        let g = Grid::new(7, 7, Boundary::Toroidal);
        let mut buf = vec![(99, 99); 50]; // pre-dirtied buffer
        g.neighbors_radius_into(3, 3, Neighborhood::Moore, 2, &mut buf);
        assert_eq!(buf, g.neighbors_radius(3, 3, Neighborhood::Moore, 2));
        // Radius 0 clears the buffer.
        g.neighbors_radius_into(3, 3, Neighborhood::Moore, 0, &mut buf);
        assert!(buf.is_empty());
    }

    #[test]
    fn neighbors_iter_matches_neighbors() {
        let cells = [(0, 0), (0, 2), (2, 0), (4, 4), (2, 2)];
        for boundary in [Boundary::Fixed, Boundary::Toroidal] {
            let g = Grid::new(5, 5, boundary);
            for nbhd in [Neighborhood::Moore, Neighborhood::VonNeumann] {
                for &(r, c) in &cells {
                    let got: Vec<(usize, usize)> = g.neighbors_iter(r, c, nbhd).collect();
                    assert_eq!(got, g.neighbors(r, c, nbhd), "cell {:?} {:?} {:?}", (r, c), boundary, nbhd);
                }
            }
        }
    }

    #[test]
    fn neighbors_iter_dedups_on_small_torus() {
        let g = Grid::new(2, 2, Boundary::Toroidal);
        let got: Vec<(usize, usize)> = g.neighbors_iter(0, 0, Neighborhood::Moore).collect();
        assert_eq!(got, vec![(0, 1), (1, 0), (1, 1)]);
    }

    // ── Adjacency (issue #11) ────────────────────────────────────────────────────

    #[test]
    fn adjacency_matches_grid_neighbors_every_cell() {
        for boundary in [Boundary::Fixed, Boundary::Toroidal] {
            let g = Grid::new(4, 5, boundary);
            let cols = g.cols();
            for nbhd in [Neighborhood::Moore, Neighborhood::VonNeumann] {
                let adj = g.adjacency(nbhd);
                assert_eq!(adj.len(), g.len());
                assert!(!adj.is_empty());
                for r in 0..g.rows() {
                    for c in 0..g.cols() {
                        let idx = r * cols + c;
                        let expected: Vec<usize> = g
                            .neighbors(r, c, nbhd)
                            .into_iter()
                            .map(|(nr, nc)| nr * cols + nc)
                            .collect();
                        assert_eq!(adj.neighbors(idx), expected.as_slice());
                    }
                }
            }
        }
    }

    #[test]
    fn adjacency_radius_matches_grid_neighbors_radius() {
        let g = Grid::new(6, 6, Boundary::Fixed);
        let cols = g.cols();
        let adj = g.adjacency_radius(Neighborhood::VonNeumann, 2);
        for r in 0..g.rows() {
            for c in 0..g.cols() {
                let idx = r * cols + c;
                let expected: Vec<usize> = g
                    .neighbors_radius(r, c, Neighborhood::VonNeumann, 2)
                    .into_iter()
                    .map(|(nr, nc)| nr * cols + nc)
                    .collect();
                assert_eq!(adj.neighbors(idx), expected.as_slice());
            }
        }
    }

    // ── CellGrid (issue #12) ─────────────────────────────────────────────────────

    #[test]
    fn cell_grid_new_fills_all_cells() {
        let cg = CellGrid::new(Grid::new(3, 4, Boundary::Fixed), 7u32);
        assert_eq!(cg.rows(), 3);
        assert_eq!(cg.cols(), 4);
        assert_eq!(cg.len(), 12);
        assert!(!cg.is_empty());
        assert!(cg.cells().iter().all(|&v| v == 7));
        assert_eq!(cg.grid().boundary(), Boundary::Fixed);
    }

    #[test]
    fn cell_grid_from_fn_uses_coordinates() {
        let cg = CellGrid::from_fn(Grid::new(2, 3, Boundary::Fixed), |r, c| r * 10 + c);
        assert_eq!(cg.get(0, 0), Some(&0));
        assert_eq!(cg.get(0, 2), Some(&2));
        assert_eq!(cg.get(1, 0), Some(&10));
        assert_eq!(cg.get(1, 2), Some(&12));
        assert_eq!(cg.cells(), &[0, 1, 2, 10, 11, 12]);
    }

    #[test]
    fn cell_grid_get_mut_round_trip() {
        let mut cg = CellGrid::new(Grid::new(3, 3, Boundary::Fixed), 0i32);
        *cg.get_mut(1, 2).unwrap() = 42;
        assert_eq!(cg.get(1, 2), Some(&42));
        *cg.get_idx_mut(0).unwrap() = -5;
        assert_eq!(cg.get_idx(0), Some(&-5));
        // Out-of-bounds queries return None.
        assert_eq!(cg.get(3, 0), None);
        assert_eq!(cg.get_mut(0, 3), None);
        assert_eq!(cg.get_idx(99), None);
    }

    #[test]
    fn cell_grid_index_mapping() {
        let cg = CellGrid::new(Grid::new(3, 4, Boundary::Fixed), 0u8);
        assert_eq!(cg.index(0, 0), 0);
        assert_eq!(cg.index(0, 3), 3);
        assert_eq!(cg.index(2, 1), 9);
    }

    #[test]
    fn cell_grid_neighbor_values() {
        // Each cell holds its flat index; neighbor_values at the center should
        // line up with grid.neighbors mapped to those values.
        let g = Grid::new(3, 3, Boundary::Fixed);
        let cg = CellGrid::from_fn(g, |r, c| r * 3 + c);
        let expected: Vec<usize> = g
            .neighbors(1, 1, Neighborhood::VonNeumann)
            .into_iter()
            .map(|(r, c)| r * 3 + c)
            .collect();
        let got: Vec<usize> = cg
            .neighbor_values(1, 1, Neighborhood::VonNeumann)
            .into_iter()
            .copied()
            .collect();
        assert_eq!(got, expected);
        // Delegated neighbors() matches the grid's.
        assert_eq!(
            cg.neighbors(0, 0, Neighborhood::Moore),
            g.neighbors(0, 0, Neighborhood::Moore)
        );
    }
}
