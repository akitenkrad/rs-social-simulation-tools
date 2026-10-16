"""Command-line entry points for exported-game plots."""

from __future__ import annotations

import argparse
from pathlib import Path
import sys

from socsim_tools.games.plots.matrix import (
    plot_best_response_2x2,
    plot_payoff_matrix,
)
from socsim_tools.games.plots.simplex import plot_simplex
from socsim_tools.games.spec import load_game_spec, load_solutions
from socsim_tools.games.style import save_figure


def plot_game_main(argv: list[str] | None = None) -> int:
    """Run the ``plot-game`` command and return zero on success."""

    parser = argparse.ArgumentParser(description="Plot an exported finite game.")
    _add_common_arguments(parser)
    parser.add_argument(
        "--kind",
        choices=("matrix", "br-arrows"),
        default="matrix",
        help="plot kind (default: matrix)",
    )
    args = parser.parse_args(argv)
    try:
        spec = load_game_spec(args.game_spec)
        solutions = (
            load_solutions(args.solutions, spec) if args.solutions else None
        )
        if args.kind == "matrix":
            figure = plot_payoff_matrix(spec, solutions)
            stem = "payoff_matrix"
        else:
            figure = plot_best_response_2x2(spec)
            stem = "best_response_2x2"
        save_figure(
            figure,
            Path(args.output_dir) / stem,
            formats=_formats(args.formats),
        )
    except (FileNotFoundError, ValueError) as error:
        _known_failure(error)
    return 0


def plot_simplex_main(argv: list[str] | None = None) -> int:
    """Run the ``plot-simplex`` command and return zero on success."""

    parser = argparse.ArgumentParser(
        description="Plot replicator dynamics on a three-action simplex."
    )
    _add_common_arguments(parser)
    parser.add_argument(
        "--n-grid",
        type=int,
        default=15,
        help="simplex grid subdivisions (default: 15)",
    )
    parser.add_argument(
        "--orbit",
        action="append",
        default=[],
        metavar="X0,X1,X2",
        help="initial point for an RK4 orbit; repeat for multiple orbits",
    )
    args = parser.parse_args(argv)
    try:
        spec = load_game_spec(args.game_spec)
        solutions = (
            load_solutions(args.solutions, spec) if args.solutions else None
        )
        orbits = [_parse_orbit(value) for value in args.orbit]
        figure = plot_simplex(
            spec,
            solutions,
            n_grid=args.n_grid,
            orbits=orbits or None,
        )
        save_figure(
            figure,
            Path(args.output_dir) / "simplex",
            formats=_formats(args.formats),
        )
    except (FileNotFoundError, ValueError) as error:
        _known_failure(error)
    return 0


def _add_common_arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("--game-spec", required=True, help="path to game_spec.json")
    parser.add_argument("--solutions", help="optional path to solutions.json")
    parser.add_argument(
        "--output-dir",
        required=True,
        help="directory for generated image files",
    )
    parser.add_argument(
        "--format",
        dest="formats",
        action="append",
        help="output format; repeat as needed (default: png and svg)",
    )


def _formats(values: list[str] | None) -> tuple[str, ...]:
    return tuple(values) if values else ("png", "svg")


def _parse_orbit(value: str) -> list[float]:
    pieces = value.split(",")
    if len(pieces) != 3:
        raise ValueError("--orbit must contain three comma-separated numbers")
    try:
        return [float(piece) for piece in pieces]
    except ValueError as error:
        raise ValueError(
            "--orbit must contain three comma-separated numbers"
        ) from error


def _known_failure(error: Exception) -> None:
    print(f"error: {error}", file=sys.stderr)
    raise SystemExit(1)


__all__ = ["plot_game_main", "plot_simplex_main"]
