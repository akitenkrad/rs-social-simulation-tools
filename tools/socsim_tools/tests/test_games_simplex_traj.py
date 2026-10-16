"""Recorded-series trajectory tests for the simplex plot."""

from __future__ import annotations

import os
from pathlib import Path

import matplotlib
from matplotlib.testing.compare import compare_images
import numpy as np
import pytest

from socsim_tools.games.plots.simplex import plot_simplex
from socsim_tools.games.records import load_metric_series
from socsim_tools.games.spec import load_game_spec, load_solutions
from socsim_tools.games.style import save_figure, style_context
from socsim_tools.games.theory import to_barycentric_xy


FIXTURES = (
    Path(__file__).resolve().parents[3] / "crates/socsim-games/tests/fixtures/viz"
)
LOCAL_FIXTURES = Path(__file__).parent / "fixtures"
BASELINES = Path(__file__).parent / "baseline/games"


def _game():
    spec = load_game_spec(FIXTURES / "rock_paper_scissors/game_spec.json")
    solutions = load_solutions(FIXTURES / "rock_paper_scissors/solutions.json", spec)
    return spec, solutions


def _artist(figure, gid: str):
    matches = [artist for artist in figure.findobj() if artist.get_gid() == gid]
    assert len(matches) == 1
    return matches[0]


def _assert_baseline(tmp_path: Path, name: str, figure) -> None:
    version_path = BASELINES / "VERSION"
    assert version_path.read_text(encoding="utf-8") == "3.11.2\n"
    assert matplotlib.__version__ == "3.11.2"
    expected = BASELINES / f"{name}.png"
    if os.environ.get("SOCSIM_TOOLS_REGEN_BASELINES") == "1":
        figure.savefig(expected, dpi=100, metadata={"Software": None})
    assert expected.exists(), (
        f"baseline image {expected} is missing; set "
        "SOCSIM_TOOLS_REGEN_BASELINES=1 to create it intentionally"
    )
    actual = tmp_path / f"{name}.png"
    figure.savefig(actual, dpi=100, metadata={"Software": None})
    assert compare_images(str(expected), str(actual), tol=1.0) is None


def test_simplex_reads_series_table_in_metric_action_order():
    spec, solutions = _game()
    table = load_metric_series(
        [LOCAL_FIXTURES / "population_share_rps.jsonl"],
        "game.population_share.",
    )
    figure = plot_simplex(spec, solutions, trajectories=table)
    trajectory = _artist(figure, "traj/0")
    expected = to_barycentric_xy(
        [[0.5, 0.25, 0.25], [0.25, 0.5, 0.25], [0.25, 0.25, 0.5]]
    )
    np.testing.assert_allclose(trajectory.get_xdata(), expected[:, 0], atol=1e-12)
    np.testing.assert_allclose(trajectory.get_ydata(), expected[:, 1], atol=1e-12)


def test_simplex_series_table_rejects_missing_columns_and_invalid_sum():
    spec, _ = _game()
    missing = load_metric_series(
        [LOCAL_FIXTURES / "population_share_rps.jsonl"],
        "game.population_share.a0",
    )
    with pytest.raises(ValueError, match="three metric columns"):
        plot_simplex(spec, trajectories=missing)

    invalid = load_metric_series(
        [LOCAL_FIXTURES / "population_share_rps_invalid.jsonl"],
        "game.population_share.",
    )
    with pytest.raises(ValueError, match="sum to 1"):
        plot_simplex(spec, trajectories=invalid)


def test_simplex_series_table_honors_trajectory_family():
    spec, _ = _game()
    table = load_metric_series(
        [LOCAL_FIXTURES / "population_share_rps.jsonl"],
        "game.population_share.",
    )
    with pytest.raises(ValueError, match="three metric columns"):
        plot_simplex(
            spec,
            trajectories=table,
            trajectory_family="game.played_share.",
        )


def test_simplex_recorded_trajectory_image_regression(tmp_path):
    with style_context():
        spec, solutions = _game()
        table = load_metric_series(
            [LOCAL_FIXTURES / "population_share_rps.jsonl"],
            "game.population_share.",
        )
        figure = plot_simplex(spec, solutions, trajectories=table)
    _assert_baseline(tmp_path, "simplex_trajectory_rock_paper_scissors", figure)
    second = plot_simplex(spec, solutions, trajectories=table)
    first_svg = save_figure(figure, tmp_path / "first/simplex", formats=("svg",))[0]
    second_svg = save_figure(second, tmp_path / "second/simplex", formats=("svg",))[0]
    assert first_svg.read_bytes() == second_svg.read_bytes()
