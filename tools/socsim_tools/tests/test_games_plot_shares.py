"""Structural and image-regression tests for metric share plots."""

from __future__ import annotations

from dataclasses import replace
import os
from pathlib import Path

import matplotlib
from matplotlib.backends.backend_agg import FigureCanvasAgg
from matplotlib.figure import Figure
from matplotlib.testing.compare import compare_images
import numpy as np

from socsim_tools import cli as root_cli
from socsim_tools.games.cli import plot_shares_main
from socsim_tools.games.plots.shares import plot_share_series
from socsim_tools.games.records import MetricSeries, SeriesTable, load_metric_series
from socsim_tools.games.spec import load_game_spec, load_solutions
from socsim_tools.games.style import save_figure, style_context


FIXTURES = (
    Path(__file__).resolve().parents[3] / "crates/socsim-games/tests/fixtures/viz"
)
BASELINES = Path(__file__).parent / "baseline/games"


def _ipd_table():
    return load_metric_series(
        [FIXTURES / "ipd" / f"seed_{seed}.jsonl" for seed in range(5)],
        "game.played_share.",
    )


def _artist(figure: Figure, gid: str):
    matches = [artist for artist in figure.findobj() if artist.get_gid() == gid]
    assert len(matches) == 1
    return matches[0]


def _assert_baseline(tmp_path: Path, name: str, figure: Figure) -> None:
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


def test_share_plot_draws_centers_bands_labels_and_exact_theory_lines():
    table = _ipd_table()
    assert table.spec is not None
    solutions = load_solutions(FIXTURES / "ipd/solutions.json", table.spec)
    figure = plot_share_series(table, table.spec, solutions)

    for metric_id in ("a0", "a1"):
        center = _artist(figure, f"series/{metric_id}")
        band = _artist(figure, f"band/{metric_id}")
        assert len(center.get_xdata()) == 50
        assert band is not None
    theory = _artist(figure, "theory/pure-0/a1")
    np.testing.assert_allclose(theory.get_ydata(), [1.0, 1.0], atol=1e-12)
    assert figure.axes[0].get_ylim() == (-0.03, 1.03)
    assert "n = 5 seeds, mean ± 95% t-CI" in figure.axes[0].get_title()
    assert [text.get_text() for text in figure.axes[0].get_legend().get_texts()] == [
        "C",
        "D",
    ]


def test_share_plot_available_alignment_has_counts_and_survivor_warning():
    table = _ipd_table()
    shortened = replace(table.series[1], t=table.series[1].t[:-1])
    shortened = replace(
        shortened,
        columns={name: values[:-1] for name, values in shortened.columns.items()},
    )
    available = replace(table, series=[table.series[0], shortened])
    figure = plot_share_series(available, align="available")
    assert len(figure.axes) == 2
    assert (
        np.testing.assert_array_equal(figure.axes[1].lines[0].get_ydata()[-2:], [2, 1])
        is None
    )
    legend = " ".join(
        text.get_text() for text in figure.axes[0].get_legend().get_texts()
    )
    assert "survivor-biased" in legend


def test_share_plot_places_per_role_theory_on_its_exact_metric():
    spec = load_game_spec(FIXTURES / "asym_2x3/game_spec.json")
    solutions = load_solutions(FIXTURES / "asym_2x3/solutions.json", spec)
    columns = {
        f"game.played_share.{action.id}": [0.5, 0.5] for action in spec.metric_actions
    }
    labels = {
        f"game.played_share.{action.id}": spec.action_labels[action.role][action.action]
        for action in spec.metric_actions
    }
    table = SeriesTable(
        series=[
            MetricSeries(
                source=Path("manual"),
                t=[1, 2],
                axis_name="t",
                columns=columns,
            )
        ],
        labels=labels,
        spec=spec,
    )
    figure = plot_share_series(table, spec, solutions, ci="none")
    line = _artist(figure, "theory/pure-0/r1-a2")
    np.testing.assert_allclose(line.get_ydata(), [1.0, 1.0], atol=1e-12)


def test_share_plot_only_labels_unconverged_theory_when_explicitly_included():
    spec = load_game_spec(FIXTURES / "cournot/game_spec.json")
    solution = load_solutions(FIXTURES / "cournot/solutions.json", spec)[0]
    unconverged = replace(solution, converged=False)
    table = load_metric_series([FIXTURES / "ipd/seed_0.jsonl"], "game.mean_payoff")
    metric = table.series[0]
    continuous_table = replace(
        table,
        series=[
            replace(
                metric,
                columns={"game.mean_action.0": metric.columns["game.mean_payoff"]},
            )
        ],
        labels={"game.mean_action.0": "x0"},
        spec=spec,
    )

    hidden = plot_share_series(
        continuous_table,
        spec,
        [unconverged],
        family="game.mean_action.",
    )
    assert not any(
        (artist.get_gid() or "").startswith("theory/") for artist in hidden.findobj()
    )

    shown = plot_share_series(
        continuous_table,
        spec,
        [unconverged],
        family="game.mean_action.",
        include_unconverged=True,
    )
    assert _artist(shown, "theory/br/0") is not None
    assert "not converged" in "\n".join(text.get_text() for text in shown.axes[0].texts)


def test_share_annotations_stay_inside_the_figure():
    table = _ipd_table()
    assert table.spec is not None
    solutions = load_solutions(FIXTURES / "ipd/solutions.json", table.spec)
    figure = plot_share_series(table, table.spec, solutions)
    FigureCanvasAgg(figure)
    figure.canvas.draw()
    renderer = figure.canvas.get_renderer()
    direct = [text for text in figure.axes[0].texts if text.get_text() in {"C", "D"}]
    assert len(direct) == 2
    assert (
        not direct[0]
        .get_window_extent(renderer)
        .overlaps(direct[1].get_window_extent(renderer))
    )
    for annotation in figure.axes[0].texts:
        extent = annotation.get_window_extent(renderer)
        assert extent.x0 >= figure.bbox.x0
        assert extent.y0 >= figure.bbox.y0
        assert extent.x1 <= figure.bbox.x1
        assert extent.y1 <= figure.bbox.y1


def test_share_boundary_theory_lines_and_annotations_clear_axes_spines():
    table = _ipd_table()
    assert table.spec is not None
    solutions = load_solutions(FIXTURES / "ipd/solutions.json", table.spec)
    figure = plot_share_series(table, table.spec, solutions)
    FigureCanvasAgg(figure)
    figure.canvas.draw()
    renderer = figure.canvas.get_renderer()
    axis = figure.axes[0]

    lower, upper = axis.get_ylim()
    assert lower < 0.0
    assert upper > 1.0
    np.testing.assert_allclose(axis.get_yticks(), np.linspace(0.0, 1.0, 6))

    annotations = [text for text in axis.texts if text.get_text() == "pure-0 / se-0"]
    assert len(annotations) == 2
    axes_bbox = axis.get_window_extent(renderer)
    for annotation in annotations:
        extent = annotation.get_window_extent(renderer)
        assert extent.x0 >= axes_bbox.x0
        assert extent.y0 > axes_bbox.y0
        assert extent.x1 <= axes_bbox.x1
        assert extent.y1 < axes_bbox.y1


def test_plot_shares_cli_and_root_dispatch_write_selected_formats(tmp_path):
    output = tmp_path / "figures"
    args = [
        "--source",
        str(FIXTURES / "ipd/seed_0.jsonl"),
        "--game-spec",
        str(FIXTURES / "ipd/game_spec.json"),
        "--solutions",
        str(FIXTURES / "ipd/solutions.json"),
        "--output-dir",
        str(output),
        "--format",
        "svg",
    ]
    assert plot_shares_main(args) == 0
    assert [path.name for path in output.iterdir()] == ["shares_played_share.svg"]

    routed = tmp_path / "routed"
    root_cli._demo_main(
        ["plot-shares", *args[:-4], "--output-dir", str(routed), "--format", "png"]
    )
    assert [path.name for path in routed.iterdir()] == ["shares_played_share.png"]


def test_share_series_image_regression(tmp_path):
    with style_context():
        table = _ipd_table()
        assert table.spec is not None
        solutions = load_solutions(FIXTURES / "ipd/solutions.json", table.spec)
        figure = plot_share_series(table, table.spec, solutions)
    _assert_baseline(tmp_path, "share_series_ipd", figure)
    second = plot_share_series(table, table.spec, solutions)
    first_svg = save_figure(figure, tmp_path / "first/share", formats=("svg",))[0]
    second_svg = save_figure(second, tmp_path / "second/share", formats=("svg",))[0]
    assert first_svg.read_bytes() == second_svg.read_bytes()
