"""Structural and image-regression tests for finite-game matrix plots."""

from __future__ import annotations

import os
from dataclasses import replace
from pathlib import Path
import subprocess
import sys

import matplotlib
from matplotlib.backends.backend_agg import FigureCanvasAgg
from matplotlib.figure import Figure
from matplotlib.testing.compare import compare_images
import pytest

from socsim_tools import cli as root_cli
from socsim_tools.games.cli import plot_game_main
from socsim_tools.games.plots.matrix import (
    plot_best_response_2x2,
    plot_payoff_matrix,
)
from socsim_tools.games.spec import load_game_spec, load_solutions
from socsim_tools.games.style import save_figure, style_context


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


def _gids(figure: Figure, prefix: str) -> set[str]:
    return {
        gid
        for artist in figure.findobj()
        if (gid := artist.get_gid()) is not None and gid.startswith(prefix)
    }


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


@pytest.mark.parametrize(
    ("game", "expected"),
    [
        ("prisoners_dilemma", {"ne/1/1"}),
        ("battle_of_sexes", {"ne/0/0", "ne/1/1"}),
        ("matching_pennies", set()),
    ],
)
def test_payoff_matrix_marks_python_pure_nash_and_best_responses(game, expected):
    spec, solutions = _game(game)
    figure = plot_payoff_matrix(spec, solutions)
    assert isinstance(figure, Figure)
    assert _gids(figure, "ne/") == expected
    assert _gids(figure, "br/0/")
    assert _gids(figure, "br/1/")


def test_payoff_matrix_lists_mixed_nash_and_checks_limits():
    spec, solutions = _game("battle_of_sexes")
    figure = plot_payoff_matrix(spec, solutions)
    text = "\n".join(item.get_text() for item in figure.axes[0].texts)
    assert "Mixed NE (se-1)" in text
    assert "role0 (B 0.600, V 0.400)" in text

    three_role, _ = _game("matrix_3p")
    with pytest.raises(ValueError, match="two-role"):
        plot_payoff_matrix(three_role)
    asym, _ = _game("asym_2x3")
    with pytest.raises(ValueError, match="max_actions"):
        plot_payoff_matrix(asym, max_actions=2)


def test_mixed_nash_annotations_stay_inside_figure():
    for game in ("battle_of_sexes", "rock_paper_scissors"):
        spec, solutions = _game(game)
        figure = plot_payoff_matrix(spec, solutions)
        annotations = [
            item
            for item in figure.axes[0].texts
            if item.get_text().startswith("Mixed NE")
        ]
        assert annotations
        FigureCanvasAgg(figure)

        for dpi in (100, 150):
            figure.set_dpi(dpi)
            figure.canvas.draw()
            renderer = figure.canvas.get_renderer()
            for annotation in annotations:
                extent = annotation.get_window_extent(renderer)
                assert extent.x0 >= figure.bbox.x0
                assert extent.y0 >= figure.bbox.y0
                assert extent.x1 <= figure.bbox.x1
                assert extent.y1 <= figure.bbox.y1


def test_payoff_matrix_warns_when_exported_pure_nash_disagrees():
    spec, _ = _game("prisoners_dilemma")
    with pytest.warns(UserWarning, match="pure Nash"):
        plot_payoff_matrix(spec, [])


def test_best_response_arrows_have_roles_directions_ties_and_one_sink():
    spec, _ = _game("prisoners_dilemma")
    figure = plot_best_response_2x2(spec)
    arrows = _gids(figure, "arrow/")
    assert len(arrows) == 4
    assert {gid.split("/")[1] for gid in arrows} == {"0", "1"}
    sinks = [
        artist.get_gid()
        for artist in figure.findobj()
        if artist.get_gid() == "sink/1/1"
    ]
    assert sinks == ["sink/1/1"]

    weak, _ = _game("weak_prisoners_dilemma")
    tied = plot_best_response_2x2(weak)
    assert _gids(tied, "tie/") == {
        "tie/0/0,1/1,1",
        "tie/1/1,0/1,1",
    }


def test_plots_use_existing_axes_without_changing_global_style():
    spec, solutions = _game("battle_of_sexes")
    original_family = list(matplotlib.rcParams["font.family"])
    figure = Figure(figsize=(3.0, 2.0))
    axis = figure.subplots()
    returned = plot_payoff_matrix(spec, solutions, ax=axis)
    assert returned is figure
    assert figure.get_size_inches().tolist() == [3.0, 2.0]
    assert matplotlib.rcParams["font.family"] == original_family


def test_non_ascii_labels_warn_without_font_and_continue():
    spec, _ = _game("prisoners_dilemma")
    localized = replace(
        spec,
        role_labels=["行", "列"],
        action_labels=[["協力", "裏切り"], ["協力", "裏切り"]],
    )
    with pytest.warns(UserWarning, match="font"):
        assert isinstance(plot_payoff_matrix(localized), Figure)


def test_plot_game_cli_writes_both_formats_and_reports_known_failures(tmp_path, capsys):
    spec_path = FIXTURES / "battle_of_sexes/game_spec.json"
    solutions_path = FIXTURES / "battle_of_sexes/solutions.json"
    output = tmp_path / "figures"
    assert (
        plot_game_main(
            [
                "--game-spec",
                str(spec_path),
                "--solutions",
                str(solutions_path),
                "--output-dir",
                str(output),
            ]
        )
        == 0
    )
    assert sorted(path.name for path in output.iterdir()) == [
        "payoff_matrix.png",
        "payoff_matrix.svg",
    ]

    with pytest.raises(SystemExit) as exc:
        plot_game_main(
            [
                "--game-spec",
                str(tmp_path / "missing.json"),
                "--output-dir",
                str(output),
            ]
        )
    assert exc.value.code == 1
    assert "missing.json" in capsys.readouterr().err


def test_root_cli_routes_and_keeps_no_argument_message(tmp_path, capsys):
    assert root_cli._demo_main([]) is None
    expected = (
        "socsim-tools: shared dispatcher + settings renderer + results I/O for "
        "socsim replication tools.\n"
        "主にライブラリとして利用します: "
        "from socsim_tools.cli import build_dispatcher\n"
    )
    assert capsys.readouterr().out == expected

    spec_path = FIXTURES / "prisoners_dilemma/game_spec.json"
    output = tmp_path / "arrows"
    root_cli._demo_main(
        [
            "plot-game",
            "--kind",
            "br-arrows",
            "--game-spec",
            str(spec_path),
            "--output-dir",
            str(output),
            "--format",
            "svg",
        ]
    )
    assert [path.name for path in output.iterdir()] == ["best_response_2x2.svg"]


def test_root_cli_explains_missing_plotting_extra(monkeypatch, capsys):
    def missing(_target):
        raise ImportError("matplotlib is unavailable")

    monkeypatch.setattr(root_cli, "_resolve_target", missing)
    with pytest.raises(SystemExit) as exc:
        root_cli._demo_main(["plot-game"])
    assert exc.value.code == 1
    assert 'pip install "socsim-tools[games]"' in capsys.readouterr().err


def test_plot_game_argument_errors_use_argparse_exit_code_two():
    with pytest.raises(SystemExit) as exc:
        plot_game_main([])
    assert exc.value.code == 2


def test_save_figure_does_not_crop_and_matrix_svg_is_deterministic(tmp_path):
    spec, solutions = _game("battle_of_sexes")
    first = plot_payoff_matrix(spec, solutions)
    second = plot_payoff_matrix(spec, solutions)
    paths_a = save_figure(first, tmp_path / "a/figure")
    paths_b = save_figure(second, tmp_path / "b/figure")
    assert [path.suffix for path in paths_a] == [".png", ".svg"]
    assert paths_a[1].read_bytes() == paths_b[1].read_bytes()
    assert first.get_size_inches().tolist() == [6.4, 4.8]


def test_matrix_svg_is_byte_identical_across_processes(tmp_path):
    script = f"""
from pathlib import Path
from socsim_tools.games.plots.matrix import plot_payoff_matrix
from socsim_tools.games.spec import load_game_spec, load_solutions
from socsim_tools.games.style import save_figure
root = Path({str(FIXTURES)!r})
spec = load_game_spec(root / 'battle_of_sexes/game_spec.json')
solutions = load_solutions(root / 'battle_of_sexes/solutions.json', spec)
save_figure(plot_payoff_matrix(spec, solutions), Path(__import__('sys').argv[1]), formats=('svg',))
"""
    stems = [tmp_path / "process-a/figure", tmp_path / "process-b/figure"]
    for stem in stems:
        subprocess.run([sys.executable, "-c", script, str(stem)], check=True)
    assert stems[0].with_suffix(".svg").read_bytes() == stems[1].with_suffix(
        ".svg"
    ).read_bytes()


def test_payoff_matrix_image_regression(tmp_path):
    with style_context():
        spec, solutions = _game("battle_of_sexes")
        matrix = plot_payoff_matrix(spec, solutions)
    _assert_baseline(tmp_path, "payoff_matrix_battle_of_sexes", matrix)


def test_best_response_image_regression(tmp_path):
    with style_context():
        spec, _ = _game("prisoners_dilemma")
        arrows = plot_best_response_2x2(spec)
    _assert_baseline(tmp_path, "best_response_2x2_prisoners_dilemma", arrows)
