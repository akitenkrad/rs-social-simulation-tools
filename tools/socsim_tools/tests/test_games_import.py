"""Import and style-contract tests for ``socsim_tools.games``."""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path


def test_core_games_imports_do_not_load_optional_dependencies():
    code = (
        "import socsim_tools, socsim_tools.games, socsim_tools.games.spec, "
        "socsim_tools.games.records, sys; "
        "print(json.dumps(sorted(m for m in "
        "('matplotlib', 'numpy', 'scipy') if m in sys.modules)))"
    )
    completed = subprocess.run(
        [sys.executable, "-c", "import json; " + code],
        check=True,
        capture_output=True,
        text=True,
    )
    assert json.loads(completed.stdout) == []


def test_records_and_aggregate_public_functions_import_from_their_modules():
    from socsim_tools.games.aggregate import aggregate_series
    from socsim_tools.games.records import (
        load_llm_calls,
        load_metric_series,
        load_step_actions,
    )

    assert callable(load_metric_series)
    assert callable(load_step_actions)
    assert callable(load_llm_calls)
    assert callable(aggregate_series)


def test_games_package_reexports_only_spec_loaders():
    import socsim_tools.games as games

    assert games.__all__ == ["load_game_spec", "load_solutions"]
    assert callable(games.load_game_spec)
    assert callable(games.load_solutions)


def test_style_context_sets_and_restores_rcparams():
    import matplotlib as mpl

    from socsim_tools.games.style import style_context

    original = {
        key: mpl.rcParams[key]
        for key in (
            "font.family",
            "font.size",
            "lines.linewidth",
            "figure.figsize",
            "svg.hashsalt",
            "svg.fonttype",
        )
    }
    with style_context():
        assert mpl.rcParams["font.family"] == ["DejaVu Sans"]
        assert mpl.rcParams["font.size"] == 10.0
        assert mpl.rcParams["lines.linewidth"] == 1.5
        assert mpl.rcParams["figure.figsize"] == [6.4, 4.8]
        assert mpl.rcParams["svg.hashsalt"] == original["svg.hashsalt"]
        assert mpl.rcParams["svg.fonttype"] == original["svg.fonttype"]
    for key, value in original.items():
        assert mpl.rcParams[key] == value

    with style_context(font="Hiragino Sans"):
        assert mpl.rcParams["font.family"] == ["Hiragino Sans"]


def test_action_colors_follow_label_order_and_require_large_palette():
    from socsim_tools.games.spec import load_game_spec
    from socsim_tools.games.style import (
        CONTINUOUS_CMAP,
        MISSING_COLOR,
        OKABE_ITO,
        ROLE_COLORS,
        _action_colors,
    )

    assert OKABE_ITO == (
        "#0072B2",
        "#D55E00",
        "#009E73",
        "#E69F00",
        "#56B4E9",
        "#CC79A7",
        "#F0E442",
        "#000000",
    )
    assert ROLE_COLORS == ("#0072B2", "#D55E00")
    assert CONTINUOUS_CMAP == "cividis"
    assert MISSING_COLOR == "#DDDDDD"

    fixture_root = (
        Path(__file__).resolve().parents[3]
        / "crates/socsim-games/tests/fixtures/viz"
    )
    spec = load_game_spec(fixture_root / "asym_2x3/game_spec.json")
    colors = _action_colors(spec)
    assert list(colors) == ["U", "D", "L", "C", "R"]
    assert list(colors.values()) == list(OKABE_ITO[:5])

    labels = [[f"a{i}" for i in range(9)], [f"a{i}" for i in range(9)]]
    large = spec.__class__(
        kind="finite",
        name="large",
        n_roles=2,
        role_labels=["row", "column"],
        action_labels=labels,
        payoffs=[[], []],
        shape=[9, 9],
        symmetric=True,
        metric_scheme="pooled",
        metric_actions=[],
        bounds=None,
        dimension_labels=None,
        params=None,
    )
    try:
        _action_colors(large)
    except ValueError as exc:
        assert "palette" in str(exc)
    else:  # pragma: no cover - assertion aid
        raise AssertionError("nine labels must require an explicit palette")

    explicit = [f"C{i}" for i in range(9)]
    assert list(_action_colors(large, explicit).values()) == explicit


def test_save_figure_writes_requested_formats_without_global_svg_changes(tmp_path):
    import matplotlib as mpl
    from matplotlib.figure import Figure

    from socsim_tools.games.style import save_figure

    original_hashsalt = mpl.rcParams["svg.hashsalt"]
    original_fonttype = mpl.rcParams["svg.fonttype"]
    figure = Figure()
    axis = figure.subplots()
    axis.plot([0.0, 1.0], [1.0, 0.0])

    paths = save_figure(figure, tmp_path / "nested/figure")
    assert paths == [
        tmp_path / "nested/figure.png",
        tmp_path / "nested/figure.svg",
    ]
    assert all(path.is_file() for path in paths)
    assert "<dc:date>" not in paths[1].read_text(encoding="utf-8")
    assert mpl.rcParams["svg.hashsalt"] == original_hashsalt
    assert mpl.rcParams["svg.fonttype"] == original_fonttype


def test_save_figure_svg_is_byte_identical_across_processes(tmp_path):
    script = """
from matplotlib.figure import Figure
from socsim_tools.games.style import save_figure
import sys
fig = Figure()
ax = fig.subplots()
ax.plot([0.0, 1.0], [1.0, 0.0])
save_figure(fig, sys.argv[1], formats=("svg",))
"""
    stems = [tmp_path / "first/figure", tmp_path / "second/figure"]
    for stem in stems:
        subprocess.run([sys.executable, "-c", script, str(stem)], check=True)
    assert stems[0].with_suffix(".svg").read_bytes() == stems[1].with_suffix(
        ".svg"
    ).read_bytes()
