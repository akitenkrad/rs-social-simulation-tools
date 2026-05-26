//! Spatial segregation metrics (feature `spatial`).
//!
//! Schelling-style read-only summaries over a [`Grid`].  Both functions are
//! generic via a **label accessor closure** `label(r, c) -> Option<L>` that the
//! caller supplies: it returns the categorical label of the occupant of cell
//! `(r, c)`, or `None` for a vacant cell.  This keeps the metrics decoupled
//! from any concrete world — pass a closure that reads your `GridIndex` /
//! `CellGrid` / agent struct.

use socsim_grid::{Grid, Neighborhood};

/// **Local similarity** of the cell `(r, c)`: the fraction of its *occupied*
/// neighbours that carry the **same** label as `(r, c)`.
///
/// Uses the radius-1 `nbhd` neighbourhood and the grid's boundary rules.
/// Vacant neighbours (those whose `label` returns `None`) are excluded from
/// both the numerator and the denominator.
///
/// Returns `None` when `(r, c)` is itself vacant, or when it has **no occupied
/// neighbours** (the ratio is undefined there).  Otherwise a value in
/// `[0.0, 1.0]`.
pub fn local_similarity<L, F>(
    grid: &Grid,
    r: usize,
    c: usize,
    nbhd: Neighborhood,
    label: &F,
) -> Option<f64>
where
    L: PartialEq,
    F: Fn(usize, usize) -> Option<L>,
{
    let own = label(r, c)?;
    let mut same = 0usize;
    let mut occupied = 0usize;
    for (nr, nc) in grid.neighbors(r, c, nbhd) {
        if let Some(other) = label(nr, nc) {
            occupied += 1;
            if other == own {
                same += 1;
            }
        }
    }
    if occupied == 0 {
        None
    } else {
        Some(same as f64 / occupied as f64)
    }
}

/// **Segregation index**: the mean [`local_similarity`] over all occupied cells
/// that have at least one occupied neighbour.
///
/// ```text
/// S = ( Σ_{occupied cell i with >=1 occupied neighbour} same_i / occupied_i ) / M
/// ```
///
/// where `same_i` is the count of `i`'s neighbours sharing `i`'s label,
/// `occupied_i` is `i`'s count of occupied neighbours, and `M` is the number of
/// such cells.  This is the standard Schelling "average fraction of like
/// neighbours": `S → 1` under perfect segregation and `S` near the population
/// share of `i`'s label under a random layout.
///
/// Labels are supplied by the accessor `label(r, c) -> Option<L>` (`None` =
/// vacant).  Cells that are vacant, or occupied but with no occupied
/// neighbours (fully isolated), are excluded.  Returns `0.0` when no cell
/// qualifies (empty / fully vacant / fully isolated grid).
pub fn segregation_index<L, F>(grid: &Grid, nbhd: Neighborhood, label: F) -> f64
where
    L: PartialEq,
    F: Fn(usize, usize) -> Option<L>,
{
    let mut sum = 0.0;
    let mut count = 0usize;
    for r in 0..grid.rows() {
        for c in 0..grid.cols() {
            if let Some(sim) = local_similarity(grid, r, c, nbhd, &label) {
                sum += sim;
                count += 1;
            }
        }
    }
    if count == 0 {
        0.0
    } else {
        sum / count as f64
    }
}

#[cfg(test)]
mod tests {
    use socsim_grid::{Boundary, Grid, Neighborhood};

    use super::*;

    /// A 2x2 grid split into two columns of distinct labels (perfect
    /// segregation under Von Neumann adjacency):
    ///   A A
    ///   B B
    fn split_label(r: usize, _c: usize) -> Option<u8> {
        Some(if r == 0 { b'A' } else { b'B' })
    }

    #[test]
    fn perfectly_segregated_rows() {
        let g = Grid::new(2, 2, Boundary::Fixed);
        // Each cell's Von Neumann neighbours: the cell beside it (same label)
        // and the cell below/above it (different label) → similarity 0.5 each.
        // Actually for 2x2: (0,0) neighbours (0,1)[A] and (1,0)[B] → 1/2.
        let s = segregation_index(&g, Neighborhood::VonNeumann, split_label);
        assert!((s - 0.5).abs() < 1e-12, "got {s}");
    }

    #[test]
    fn fully_uniform_is_one() {
        // Every occupied cell has only same-label neighbours.
        let g = Grid::new(3, 3, Boundary::Fixed);
        let s = segregation_index(&g, Neighborhood::Moore, |_, _| Some(7u8));
        assert!((s - 1.0).abs() < 1e-12, "got {s}");
    }

    #[test]
    fn checkerboard_is_zero() {
        // Alternating labels: every Von Neumann neighbour differs → similarity 0.
        let g = Grid::new(4, 4, Boundary::Fixed);
        let s = segregation_index(&g, Neighborhood::VonNeumann, |r, c| {
            Some(((r + c) % 2) as u8)
        });
        assert!(s.abs() < 1e-12, "got {s}");
    }

    #[test]
    fn vacant_grid_is_zero() {
        let g = Grid::new(3, 3, Boundary::Fixed);
        let s = segregation_index(&g, Neighborhood::Moore, |_, _| None::<u8>);
        assert!(s.abs() < 1e-12);
    }

    #[test]
    fn local_similarity_excludes_vacant_and_isolated() {
        let g = Grid::new(3, 3, Boundary::Fixed);
        // Only (0,0) and (0,1) occupied (same label), rest vacant.
        let label = |r: usize, c: usize| {
            if (r, c) == (0, 0) || (r, c) == (0, 1) {
                Some(1u8)
            } else {
                None
            }
        };
        // (0,0): one occupied neighbour (0,1), same label → 1.0.
        assert_eq!(
            local_similarity(&g, 0, 0, Neighborhood::VonNeumann, &label),
            Some(1.0)
        );
        // (1,1) is vacant → None.
        assert_eq!(
            local_similarity(&g, 1, 1, Neighborhood::VonNeumann, &label),
            None
        );
        // (2,2) is vacant → None.
        assert_eq!(
            local_similarity(&g, 2, 2, Neighborhood::Moore, &label),
            None
        );
    }
}
