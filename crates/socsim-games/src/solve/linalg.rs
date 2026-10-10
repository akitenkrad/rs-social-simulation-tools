const RANK_RELATIVE_TOLERANCE: f64 = 1e-10;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LeastSquaresSolution {
    pub(crate) solution: Vec<f64>,
    pub(crate) rank: usize,
    pub(crate) condition_estimate: f64,
    pub(crate) residual: f64,
    pub(crate) nullspace: Vec<Vec<f64>>,
}

pub(crate) fn solve_least_squares(matrix: &[Vec<f64>], rhs: &[f64]) -> LeastSquaresSolution {
    let rows = matrix.len();
    let columns = matrix.first().map_or(0, Vec::len);
    assert_eq!(rhs.len(), rows, "right-hand side has the wrong dimension");
    assert!(
        matrix.iter().all(|row| row.len() == columns),
        "matrix is ragged"
    );

    let mut qr = matrix.to_vec();
    let mut transformed_rhs = rhs.to_vec();
    let mut permutation = (0..columns).collect::<Vec<_>>();
    let steps = rows.min(columns);

    for step in 0..steps {
        let pivot = (step..columns)
            .max_by(|&left, &right| {
                column_norm(&qr, step, left).total_cmp(&column_norm(&qr, step, right))
            })
            .expect("the pivot range is non-empty");
        if pivot != step {
            for row in &mut qr {
                row.swap(step, pivot);
            }
            permutation.swap(step, pivot);
        }

        let norm = column_norm(&qr, step, step);
        if norm == 0.0 {
            continue;
        }
        let diagonal = if qr[step][step] >= 0.0 { -norm } else { norm };
        let mut reflector = (step..rows).map(|row| qr[row][step]).collect::<Vec<_>>();
        reflector[0] -= diagonal;
        let denominator = reflector.iter().map(|value| value * value).sum::<f64>();
        if denominator == 0.0 {
            continue;
        }
        let scale = 2.0 / denominator;

        #[allow(clippy::needless_range_loop)]
        for column in step..columns {
            let projection = reflector
                .iter()
                .enumerate()
                .map(|(offset, value)| value * qr[step + offset][column])
                .sum::<f64>()
                * scale;
            for (offset, value) in reflector.iter().enumerate() {
                qr[step + offset][column] -= projection * value;
            }
        }
        let rhs_projection = reflector
            .iter()
            .enumerate()
            .map(|(offset, value)| value * transformed_rhs[step + offset])
            .sum::<f64>()
            * scale;
        for (offset, value) in reflector.iter().enumerate() {
            transformed_rhs[step + offset] -= rhs_projection * value;
        }

        qr[step][step] = diagonal;
        for row in qr.iter_mut().skip(step + 1) {
            row[step] = 0.0;
        }
    }

    let rank = numerical_rank(&qr, steps);
    let condition_estimate = if rank == 0 {
        f64::INFINITY
    } else {
        qr[0][0].abs() / qr[rank - 1][rank - 1].abs()
    };
    let mut permuted_solution = vec![0.0; columns];
    solve_leading_triangle(&qr, rank, &transformed_rhs[..rank], &mut permuted_solution);
    let solution = undo_permutation(&permuted_solution, &permutation);

    let mut nullspace = Vec::with_capacity(columns.saturating_sub(rank));
    for free in rank..columns {
        let mut direction = vec![0.0; columns];
        direction[free] = 1.0;
        for row in (0..rank).rev() {
            let tail = ((row + 1)..columns)
                .map(|column| qr[row][column] * direction[column])
                .sum::<f64>();
            direction[row] = -tail / qr[row][row];
        }
        nullspace.push(undo_permutation(&direction, &permutation));
    }

    LeastSquaresSolution {
        residual: residual_norm(matrix, rhs, &solution),
        solution,
        rank,
        condition_estimate,
        nullspace,
    }
}

fn column_norm(matrix: &[Vec<f64>], start_row: usize, column: usize) -> f64 {
    matrix
        .iter()
        .skip(start_row)
        .fold(0.0, |norm, row| norm.hypot(row[column]))
}

fn numerical_rank(qr: &[Vec<f64>], steps: usize) -> usize {
    if steps == 0 || qr[0][0] == 0.0 {
        return 0;
    }
    let threshold = RANK_RELATIVE_TOLERANCE * qr[0][0].abs();
    (0..steps)
        .take_while(|&index| qr[index][index].abs() > threshold)
        .count()
}

fn solve_leading_triangle(triangle: &[Vec<f64>], rank: usize, rhs: &[f64], solution: &mut [f64]) {
    for row in (0..rank).rev() {
        let tail = ((row + 1)..rank)
            .map(|column| triangle[row][column] * solution[column])
            .sum::<f64>();
        solution[row] = (rhs[row] - tail) / triangle[row][row];
    }
}

fn undo_permutation(permuted: &[f64], permutation: &[usize]) -> Vec<f64> {
    let mut original = vec![0.0; permuted.len()];
    for (column, &original_column) in permutation.iter().enumerate() {
        original[original_column] = permuted[column];
    }
    original
}

fn residual_norm(matrix: &[Vec<f64>], rhs: &[f64], solution: &[f64]) -> f64 {
    matrix.iter().zip(rhs).fold(0.0, |norm, (row, target)| {
        let actual = row
            .iter()
            .zip(solution)
            .map(|(coefficient, value)| coefficient * value)
            .sum::<f64>();
        norm.hypot(actual - target)
    })
}

#[cfg(test)]
mod tests {
    use super::solve_least_squares;

    const EPSILON: f64 = 1e-10;

    fn multiply(matrix: &[Vec<f64>], vector: &[f64]) -> Vec<f64> {
        matrix
            .iter()
            .map(|row| row.iter().zip(vector).map(|(a, x)| a * x).sum())
            .collect()
    }

    fn assert_close(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() <= EPSILON,
            "expected {expected}, got {actual}"
        );
    }

    #[test]
    fn solves_a_full_rank_square_system() {
        let matrix = vec![vec![2.0, 1.0], vec![1.0, -1.0]];
        let solved = solve_least_squares(&matrix, &[5.0, 1.0]);

        assert_eq!(solved.rank, 2);
        assert!(solved.condition_estimate >= 1.0);
        assert_close(solved.solution[0], 2.0);
        assert_close(solved.solution[1], 1.0);
        assert_close(solved.residual, 0.0);
        assert!(solved.nullspace.is_empty());
    }

    #[test]
    fn estimates_condition_from_the_retained_diagonal_range() {
        let matrix = vec![vec![4.0, 0.0], vec![0.0, 1.0]];
        let solved = solve_least_squares(&matrix, &[0.0, 0.0]);

        assert_eq!(solved.rank, 2);
        assert_close(solved.condition_estimate, 4.0);
    }

    #[test]
    fn returns_the_overdetermined_least_squares_solution_and_residual() {
        let matrix = vec![vec![1.0, 0.0], vec![0.0, 1.0], vec![1.0, 1.0]];
        let solved = solve_least_squares(&matrix, &[1.0, 2.0, 2.0]);

        assert_eq!(solved.rank, 2);
        assert_close(solved.solution[0], 2.0 / 3.0);
        assert_close(solved.solution[1], 5.0 / 3.0);
        assert_close(solved.residual, (1.0_f64 / 3.0).sqrt());
    }

    #[test]
    fn reports_rank_deficiency_and_a_nullspace_basis() {
        let matrix = vec![vec![1.0, 2.0, 3.0], vec![2.0, 4.0, 6.0]];
        let solved = solve_least_squares(&matrix, &[1.0, 2.0]);

        assert_eq!(solved.rank, 1);
        assert_eq!(solved.nullspace.len(), 2);
        assert_close(solved.residual, 0.0);
        for direction in &solved.nullspace {
            for value in multiply(&matrix, direction) {
                assert_close(value, 0.0);
            }
        }
    }

    #[test]
    fn handles_a_wide_full_row_rank_matrix() {
        let matrix = vec![vec![1.0, 0.0, 1.0], vec![0.0, 1.0, 1.0]];
        let solved = solve_least_squares(&matrix, &[2.0, 3.0]);

        assert_eq!(solved.rank, 2);
        assert_eq!(solved.nullspace.len(), 1);
        assert_close(solved.residual, 0.0);
        for (actual, expected) in multiply(&matrix, &solved.solution).iter().zip([2.0, 3.0]) {
            assert_close(*actual, expected);
        }
        for value in multiply(&matrix, &solved.nullspace[0]) {
            assert_close(value, 0.0);
        }
    }
}
