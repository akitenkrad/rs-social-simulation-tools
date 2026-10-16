"""Command-line entry points for exported-game plots."""

from __future__ import annotations

import argparse
from pathlib import Path
import sys

from socsim_tools.games.plots.actions import plot_action_raster
from socsim_tools.games.plots.matrix import (
    plot_best_response_2x2,
    plot_payoff_matrix,
)
from socsim_tools.games.plots.shares import plot_share_series
from socsim_tools.games.plots.simplex import plot_simplex
from socsim_tools.games.records import (
    load_llm_calls,
    load_metric_series,
    load_step_actions,
)
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


def plot_shares_main(argv: list[str] | None = None) -> int:
    """Run the ``plot-shares`` command and return zero on success."""

    parser = argparse.ArgumentParser(
        description="Plot aggregate metric shares across simulation runs."
    )
    parser.add_argument(
        "--source",
        action="append",
        required=True,
        help="run directory or socsim JSONL file; repeat for multiple runs",
    )
    parser.add_argument(
        "--family",
        default="game.played_share.",
        help="metric family prefix (default: game.played_share.)",
    )
    parser.add_argument("--stat", choices=("mean", "median"), default="mean")
    parser.add_argument(
        "--ci",
        choices=("t", "bootstrap", "quantile", "none"),
        default="t",
    )
    parser.add_argument("--level", type=float, default=0.95)
    parser.add_argument(
        "--align", choices=("intersection", "available"), default="intersection"
    )
    parser.add_argument(
        "--theory",
        action="append",
        help="solution id to draw; repeat as needed (default: all)",
    )
    parser.add_argument(
        "--repo-id",
        help="repository id for namespaced runvault records",
    )
    _add_common_arguments(parser)
    args = parser.parse_args(argv)
    try:
        spec = load_game_spec(args.game_spec)
        solutions = (
            load_solutions(args.solutions, spec) if args.solutions else None
        )
        table = load_metric_series(args.source, args.family)
        if table.spec is not None and table.spec != spec:
            raise ValueError("game specification does not match the metric sources")
        figure = plot_share_series(
            table,
            spec,
            solutions,
            family=args.family,
            stat=args.stat,
            ci=args.ci,
            level=args.level,
            align=args.align,
            theory=args.theory or "all",
        )
        family_name = args.family.rstrip(".").rsplit(".", 1)[-1]
        save_figure(
            figure,
            Path(args.output_dir) / f"shares_{family_name}",
            formats=_formats(args.formats),
        )
    except (FileNotFoundError, ValueError) as error:
        _known_failure(error)
    return 0


def plot_actions_main(argv: list[str] | None = None) -> int:
    """Run the ``plot-actions`` command and return zero on success."""

    parser = argparse.ArgumentParser(
        description="Plot per-agent actions across recorded rounds."
    )
    parser.add_argument("--source", required=True, help="run directory or JSONL file")
    parser.add_argument(
        "--llm-calls",
        help="optional separate run directory or JSONL file with LLM calls",
    )
    parser.add_argument(
        "--t-range",
        metavar="A:B",
        help="inclusive round range; either endpoint may be omitted",
    )
    parser.add_argument(
        "--agents",
        help="comma-separated agent ids",
    )
    parser.add_argument(
        "--repo-id",
        help="repository id for namespaced runvault events",
    )
    _add_common_arguments(parser)
    args = parser.parse_args(argv)
    try:
        spec = load_game_spec(args.game_spec)
        if args.solutions:
            load_solutions(args.solutions, spec)
        log = load_step_actions(args.source, repo_id=args.repo_id)
        calls = load_llm_calls(
            args.llm_calls or args.source,
            repo_id=args.repo_id,
        )
        figure = plot_action_raster(
            log,
            spec,
            calls,
            t_range=_parse_t_range(args.t_range),
            agents=_parse_agents(args.agents),
        )
        save_figure(
            figure,
            Path(args.output_dir) / "actions",
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


def _parse_t_range(value: str | None) -> tuple[int | None, int | None] | None:
    if value is None:
        return None
    pieces = value.split(":")
    if len(pieces) != 2:
        raise ValueError("--t-range must have the form A:B")
    try:
        bounds = tuple(int(piece) if piece else None for piece in pieces)
    except ValueError as error:
        raise ValueError("--t-range must have the form A:B") from error
    if bounds == (None, None):
        raise ValueError("--t-range must include at least one endpoint")
    return bounds


def _parse_agents(value: str | None) -> list[int] | None:
    if value is None:
        return None
    pieces = value.split(",")
    if not pieces or any(not piece for piece in pieces):
        raise ValueError("--agents must contain comma-separated integer ids")
    try:
        return [int(piece) for piece in pieces]
    except ValueError as error:
        raise ValueError(
            "--agents must contain comma-separated integer ids"
        ) from error


def _known_failure(error: Exception) -> None:
    print(f"error: {error}", file=sys.stderr)
    raise SystemExit(1)


__all__ = [
    "plot_actions_main",
    "plot_game_main",
    "plot_shares_main",
    "plot_simplex_main",
]
