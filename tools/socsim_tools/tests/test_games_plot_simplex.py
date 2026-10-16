"""Structural and image-regression tests for simplex plots."""

from __future__ import annotations

import os
from dataclasses import replace
from pathlib import Path
import warnings

import matplotlib
from matplotlib.figure import Figure
from matplotlib.quiver import Quiver
from matplotlib.testing.compare import compare_images
import numpy as np
import pytest

from socsim_tools.games.cli import plot_simplex_main
from socsim_tools.games.plots.simplex import plot_simplex
from socsim_tools.games.spec import PopulationComponent, load_game_spec, load_solutions
from socsim_tools.games.style import style_context
from socsim_tools.games.theory import to_barycentric_xy


FIXTURES = (
    Path(__file__).resolve().parents[3]
    / "crates/socsim-games/tests/fixtures/viz"
)
BASELINES = Path(__file__).parent / "baseline/games"


def _game(name: str):
    spec = load_game_spec(FIXTURES / name / "game_spec.json")
    solution_path = FIXTURES / name / "solutions.json"
    solutions = load_solutions(solution_path, spec) if solution_path.exists() else None
    return spec, solutions


def _artist(figure: Figure, gid: str):
    matches = [artist for artist in figure.findobj() if artist.get_gid() == gid]
    assert len(matches) == 1
    return matches[0]


def _assert_baseline(tmp_path: Path, name: str, figure: Figure) -> None:
    version_path = BASELINES / "VERSION"
    assert version_path.exists(), "baseline VERSION is missing"
    assert version_path.read_text(encoding="utf-8") == "3.11.2\n"
    assert matplotlib.__version__ == "3.11.2", (
        "image baselines require matplotlib 3.11.2; regenerate them when upgrading"
    )
    expected = BASELINES / f"{name}.png"
    if os.environ.get("SOCSIM_TOOLS_REGEN_BASELINES") == "1":
        expected.parent.mkdir(parents=True, exist_ok=True)
        figure.savefig(expected, dpi=100, metadata={"Software": None})
    assert expected.exists(), (
        f"baseline image {expected} is missing; set "
        "SOCSIM_TOOLS_REGEN_BASELINES=1 to create it intentionally"
    )
    actual = tmp_path / f"{name}.png"
    figure.savefig(actual, dpi=100, metadata={"Software": None})
    assert compare_images(str(expected), str(actual), tol=1.0) is None


def test_simplex_has_all_grid_arrows_and_the_rps_interior_fixed_point():
    spec, solutions = _game("rock_paper_scissors")
    figure = plot_simplex(spec, solutions, n_grid=15)
    quivers = [artist for artist in figure.findobj(Quiver)]
    assert len(quivers) == 1
    assert quivers[0].N == 136

    interior = _artist(figure, "fixed/rep-3")
    expected = to_barycentric_xy([1.0 / 3.0] * 3)
    np.testing.assert_allclose(interior.get_xdata(), [expected[0]], atol=1e-12)
    np.testing.assert_allclose(interior.get_ydata(), [expected[1]], atol=1e-12)


def test_simplex_draws_one_dimensional_components_and_skips_inconclusive():
    spec, solutions = _game("replicator_component")
    figure = plot_simplex(spec, solutions)
    assert _artist(figure, "fixed/rep-c0") is not None
    assert _artist(figure, "fixed/rep-c1") is not None

    rps, rps_solutions = _game("rock_paper_scissors")
    with warnings.catch_warnings(record=True) as captured:
        warnings.simplefilter("always")
        rps_figure = plot_simplex(rps, rps_solutions)
    assert captured == []
    assert not any(
        (artist.get_gid() or "").startswith("fixed/rep-f")
        for artist in rps_figure.findobj()
    )

    assert solutions is not None
    two_dimensional = PopulationComponent(
        id="plane",
        solver="test",
        base=[1.0, 0.0, 0.0],
        directions=[[1.0, -1.0, 0.0], [0.0, 1.0, -1.0]],
    )
    with pytest.warns(UserWarning, match="two or more dimensions"):
        skipped = plot_simplex(spec, [*solutions, two_dimensional])
    assert not any(
        artist.get_gid() == "fixed/plane" for artist in skipped.findobj()
    )


def test_simplex_trajectories_orbits_and_probability_validation():
    spec, solutions = _game("rock_paper_scissors")
    trajectory = np.array(
        [[0.8, 0.1, 0.1], [0.7, 0.2, 0.1], [0.6, 0.3, 0.1]]
    )
    figure = plot_simplex(
        spec,
        solutions,
        trajectories=[trajectory],
        orbits=[[0.6, 0.3, 0.1]],
        orbit_t_max=0.2,
    )
    assert _artist(figure, "traj/0") is not None
    assert _artist(figure, "traj/orbit-0") is not None

    second = np.array([[0.2, 0.5, 0.3], [0.3, 0.4, 0.3]])
    ragged = plot_simplex(spec, trajectories=[trajectory, second])
    assert _artist(ragged, "traj/0") is not None
    assert _artist(ragged, "traj/1") is not None

    with pytest.raises(ValueError, match="sum to 1"):
        plot_simplex(spec, trajectories=[[[0.2, 0.2, 0.2]]])


def test_simplex_checks_shape_symmetry_grid_and_streamlines():
    asym, _ = _game("asym_2x3")
    with pytest.raises(ValueError, match="three actions"):
        plot_simplex(asym)

    spec, _ = _game("rock_paper_scissors")
    assert spec.payoffs is not None
    nonsymmetric = replace(
        spec,
        payoffs=[spec.payoffs[0], [*spec.payoffs[1][:-1], 0.25]],
    )
    with pytest.raises(ValueError, match="symmetric"):
        plot_simplex(nonsymmetric)
    with pytest.raises(ValueError, match="symmetric"):
        plot_simplex(replace(spec, symmetric=False))
    with pytest.raises(ValueError, match="n_grid"):
        plot_simplex(spec, n_grid=0)

    stream = plot_simplex(spec, streamlines=True, n_grid=8)
    assert any(collection.get_gid() == "streamlines" for collection in stream.axes[0].collections)


def test_simplex_uses_existing_axes_and_warns_for_non_ascii_labels():
    spec, solutions = _game("rock_paper_scissors")
    figure = Figure(figsize=(3.0, 2.0))
    axis = figure.subplots()
    returned = plot_simplex(spec, solutions, ax=axis, color_by_speed=False)
    assert returned is figure
    assert figure.get_size_inches().tolist() == [3.0, 2.0]

    localized = replace(
        spec,
        action_labels=[["石", "紙", "鋏"], ["石", "紙", "鋏"]],
    )
    with pytest.warns(UserWarning, match="font"):
        assert isinstance(plot_simplex(localized), Figure)


def test_plot_simplex_cli_writes_selected_format_and_parses_orbit(tmp_path):
    output = tmp_path / "figures"
    assert (
        plot_simplex_main(
            [
                "--game-spec",
                str(FIXTURES / "rock_paper_scissors/game_spec.json"),
                "--solutions",
                str(FIXTURES / "rock_paper_scissors/solutions.json"),
                "--output-dir",
                str(output),
                "--format",
                "png",
                "--orbit",
                "0.6,0.3,0.1",
            ]
        )
        == 0
    )
    assert [path.name for path in output.iterdir()] == ["simplex.png"]


def test_simplex_image_regression(tmp_path):
    with style_context():
        spec, solutions = _game("rock_paper_scissors")
        figure = plot_simplex(spec, solutions)
    _assert_baseline(tmp_path, "simplex_rock_paper_scissors", figure)
