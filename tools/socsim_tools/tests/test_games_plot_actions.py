"""Structural and image-regression tests for action raster plots."""

from __future__ import annotations

import os
from pathlib import Path

import matplotlib
from matplotlib.backends.backend_agg import FigureCanvasAgg
from matplotlib.colors import to_rgba
from matplotlib.testing.compare import compare_images
import numpy as np
import pytest

from socsim_tools.games.cli import plot_actions_main
from socsim_tools.games.plots.actions import plot_action_raster
from socsim_tools.games.records import load_llm_calls, load_step_actions
from socsim_tools.games.spec import load_game_spec
from socsim_tools.games.style import (
    MISSING_COLOR,
    OKABE_ITO,
    save_figure,
    style_context,
)


FIXTURES = (
    Path(__file__).resolve().parents[3] / "crates/socsim-games/tests/fixtures/viz"
)
LOCAL_FIXTURES = Path(__file__).parent / "fixtures"
BASELINES = Path(__file__).parent / "baseline/games"


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


def test_action_raster_colors_cells_and_places_last_llm_resolution():
    source = FIXTURES / "ipd/seed_0.jsonl"
    spec = load_game_spec(FIXTURES / "ipd/game_spec.json")
    log = load_step_actions(source)
    calls = load_llm_calls(LOCAL_FIXTURES / "llm_calls_ipd.jsonl")
    figure = plot_action_raster(log, spec, calls)

    image = figure.axes[0].images[0]
    rgba = image.cmap(image.norm(image.get_array()))
    assert np.allclose(rgba[0, 0], to_rgba(OKABE_ITO[0]))
    assert np.allclose(rgba[1, 0], to_rgba(OKABE_ITO[1]))
    assert _artist(figure, "llm/giveup/3/1").get_marker() == "x"
    accepted = _artist(figure, "llm/accept/7/1")
    assert accepted.get_marker() == "o"
    np.testing.assert_allclose(accepted.get_xdata(), [7])
    np.testing.assert_allclose(accepted.get_ydata(), [1])
    assert not any(artist.get_gid() == "llm/retry/7/1" for artist in figure.findobj())
    assert not any(artist.get_gid() == "llm/accept/12/0" for artist in figure.findobj())


def test_action_raster_filters_agents_adds_payoffs_and_enforces_max_cells():
    source = FIXTURES / "ipd/seed_0.jsonl"
    spec = load_game_spec(FIXTURES / "ipd/game_spec.json")
    log = load_step_actions(source)
    figure = plot_action_raster(
        log,
        spec,
        t_range=(3, 7),
        agents=[1],
        show_payoffs=True,
    )
    assert len(figure.axes) == 2
    assert figure.axes[0].images[0].get_array().shape == (1, 5)
    assert len(figure.axes[1].lines) == 1

    with pytest.raises(ValueError, match="max_cells"):
        plot_action_raster(log, spec, max_cells=99)


def test_action_raster_uses_missing_color_for_an_unplayed_agent():
    spec = load_game_spec(FIXTURES / "ipd/game_spec.json")
    log = load_step_actions(FIXTURES / "ipd/seed_0.jsonl")
    actions = [row.copy() for row in log.actions]
    actions[0][0] = None
    from dataclasses import replace

    figure = plot_action_raster(replace(log, actions=actions), spec)
    image = figure.axes[0].images[0]
    rgba = image.cmap(image.norm(image.get_array()))
    assert np.allclose(rgba[0, 0], to_rgba(MISSING_COLOR))


def test_action_raster_labels_and_legend_stay_inside_the_figure():
    spec = load_game_spec(FIXTURES / "ipd/game_spec.json")
    log = load_step_actions(FIXTURES / "ipd/seed_0.jsonl")
    figure = plot_action_raster(log, spec)
    FigureCanvasAgg(figure)
    figure.canvas.draw()
    renderer = figure.canvas.get_renderer()
    labels = [
        figure.axes[0].xaxis.label,
        figure.axes[0].yaxis.label,
        *figure.axes[0].get_yticklabels(),
        *figure.axes[0].get_legend().get_texts(),
    ]
    for label in labels:
        extent = label.get_window_extent(renderer)
        assert extent.x0 >= figure.bbox.x0
        assert extent.y0 >= figure.bbox.y0
        assert extent.x1 <= figure.bbox.x1
        assert extent.y1 <= figure.bbox.y1


def test_plot_actions_cli_writes_both_formats(tmp_path):
    output = tmp_path / "figures"
    assert (
        plot_actions_main(
            [
                "--source",
                str(FIXTURES / "ipd/seed_0.jsonl"),
                "--llm-calls",
                str(LOCAL_FIXTURES / "llm_calls_ipd.jsonl"),
                "--game-spec",
                str(FIXTURES / "ipd/game_spec.json"),
                "--output-dir",
                str(output),
            ]
        )
        == 0
    )
    assert sorted(path.name for path in output.iterdir()) == [
        "actions.png",
        "actions.svg",
    ]


def test_action_raster_image_regression(tmp_path):
    with style_context():
        spec = load_game_spec(FIXTURES / "ipd/game_spec.json")
        log = load_step_actions(FIXTURES / "ipd/seed_0.jsonl")
        calls = load_llm_calls(LOCAL_FIXTURES / "llm_calls_ipd.jsonl")
        figure = plot_action_raster(log, spec, calls)
    _assert_baseline(tmp_path, "action_raster_ipd", figure)
    second = plot_action_raster(log, spec, calls)
    first_svg = save_figure(figure, tmp_path / "first/actions", formats=("svg",))[0]
    second_svg = save_figure(second, tmp_path / "second/actions", formats=("svg",))[0]
    assert first_svg.read_bytes() == second_svg.read_bytes()
