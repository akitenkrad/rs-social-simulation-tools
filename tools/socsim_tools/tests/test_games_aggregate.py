"""Tests for cross-run aggregation and solution-to-series mappings."""

from __future__ import annotations

import json
import math
import warnings
from dataclasses import replace
from pathlib import Path

import numpy as np
import pytest

from socsim_tools.games.aggregate import Aggregated, aggregate_series
from socsim_tools.games.records import MetricSeries, SeriesTable, load_metric_series
from socsim_tools.games.spec import load_game_spec, load_solutions
from socsim_tools.games.theory import TheoryLine, theory_lines

FIXTURES = (
    Path(__file__).resolve().parents[3] / "crates/socsim-games/tests/fixtures/viz"
)


def _table(values: list[float]) -> SeriesTable:
    return SeriesTable(
        series=[
            MetricSeries(
                source=Path(f"seed_{index}.jsonl"),
                t=[1],
                axis_name="t",
                columns={"game.x.a0": [value]},
            )
            for index, value in enumerate(values)
        ],
        labels={"game.x.a0": "a0"},
    )


def test_mean_and_student_t_interval_match_hand_calculation():
    result = aggregate_series(
        _table([0.2, 0.4, 0.9]),
        stat="mean",
        ci="t",
        level=0.95,
        align="intersection",
    )
    assert isinstance(result, Aggregated)
    critical = 4.302652729749462
    margin = critical * math.sqrt(0.13 / 3.0)
    assert result.t == [1]
    assert result.center["game.x.a0"][0] == pytest.approx(0.5, abs=1e-15)
    assert result.lower["game.x.a0"][0] == pytest.approx(0.5 - margin, abs=1e-12)
    assert result.upper["game.x.a0"][0] == pytest.approx(0.5 + margin, abs=1e-12)
    assert result.n_t == [3]


def test_alignment_available_returns_union_and_counts():
    table = SeriesTable(
        series=[
            MetricSeries(Path("a"), [1, 2], "round", {"game.x.a0": [0.1, 0.2]}),
            MetricSeries(Path("b"), [2, 3], "round", {"game.x.a0": [0.4, 0.6]}),
        ],
        labels={"game.x.a0": "a0"},
    )
    with pytest.warns(UserWarning, match="one replicate"):
        available = aggregate_series(table, "mean", "t", 0.95, "available")
    assert available.t == [1, 2, 3]
    assert available.center["game.x.a0"] == pytest.approx([0.1, 0.3, 0.6])
    assert available.n_t == [1, 2, 1]
    assert available.lower["game.x.a0"][0] is None
    assert available.upper["game.x.a0"][2] is None

    intersection = aggregate_series(table, "mean", "none", 0.95, "intersection")
    assert intersection.t == [2]
    assert intersection.center["game.x.a0"] == pytest.approx([0.3])
    assert intersection.lower is None
    assert intersection.upper is None


def test_median_quantile_and_bootstrap_are_supported_and_deterministic():
    table = _table([0.2, 0.4, 0.9])
    quantile = aggregate_series(table, "median", "quantile", 0.8, "intersection")
    assert quantile.center["game.x.a0"] == [0.4]
    assert quantile.lower["game.x.a0"] == pytest.approx([0.24])
    assert quantile.upper["game.x.a0"] == pytest.approx([0.8])

    first = aggregate_series(table, "mean", "bootstrap", 0.95, "intersection")
    second = aggregate_series(table, "mean", "bootstrap", 0.95, "intersection")
    assert first == second
    rng = np.random.default_rng(0)
    values = np.asarray([0.2, 0.4, 0.9])
    samples = values[rng.integers(0, 3, size=(2000, 3))]
    expected = np.percentile(np.mean(samples, axis=1), [2.5, 97.5])
    assert first.lower["game.x.a0"] == pytest.approx([expected[0]])
    assert first.upper["game.x.a0"] == pytest.approx([expected[1]])

    with pytest.raises(ValueError, match="median.*t"):
        aggregate_series(table, "median", "t", 0.95, "intersection")


def test_source_specs_must_match_and_missing_specs_warn(tmp_path):
    rows = [
        {"type": "metric", "t": 1, "key": "game.played_share.a0", "value": 1.0},
        {"type": "metric", "t": 1, "key": "game.played_share.a1", "value": 0.0},
    ]
    sources = []
    for name in ("first", "second"):
        directory = tmp_path / name
        directory.mkdir()
        source = directory / "seed.jsonl"
        source.write_text(
            "".join(json.dumps(row) + "\n" for row in rows), encoding="utf-8"
        )
        sources.append(source)
    first_spec = json.loads(
        (FIXTURES / "ipd/game_spec.json").read_text(encoding="utf-8")
    )
    second_spec = json.loads(json.dumps(first_spec))
    second_spec["name"] = "different"
    (sources[0].parent / "game_spec.json").write_text(
        json.dumps(first_spec), encoding="utf-8"
    )
    (sources[1].parent / "game_spec.json").write_text(
        json.dumps(second_spec), encoding="utf-8"
    )
    with pytest.raises(ValueError, match="game specifications do not match"):
        load_metric_series(sources, "game.played_share.")

    (sources[1].parent / "game_spec.json").unlink()
    with pytest.warns(UserWarning, match="game_spec.json"):
        table = load_metric_series(sources, "game.played_share.")
    assert set(table.labels.values()) == {"C", "D"}


@pytest.mark.parametrize(
    ("game", "family", "solution_id", "metric_id", "expected"),
    [
        ("hawk_dove", "game.population_share.", "rep-2", "a0", 0.5),
        ("battle_of_sexes", "game.played_share.", "se-0", "a0", 1.0),
        ("battle_of_sexes", "game.played_share.", "se-1", "a0", 0.5),
        ("battle_of_sexes", "game.played_share.", "se-2", "a0", 0.0),
        ("matching_pennies", "game.played_share.", "se-0", "a0", 0.5),
        ("rock_paper_scissors", "game.played_share.", "se-0", "a0", 1.0 / 3.0),
        ("asym_2x3", "game.played_share.", "pure-0", "r1-a2", 1.0),
        ("cournot", "game.mean_action.", "br", "0", 22.5),
    ],
)
def test_theory_lines_match_exported_solutions(
    game, family, solution_id, metric_id, expected
):
    spec = load_game_spec(FIXTURES / game / "game_spec.json")
    solutions = load_solutions(FIXTURES / game / "solutions.json", spec)
    with warnings.catch_warnings():
        warnings.simplefilter("ignore", UserWarning)
        lines = theory_lines(spec, solutions, family)
    matches = [
        line
        for line in lines
        if line.solution_id == solution_id and line.metric_id == metric_id
    ]
    assert len(matches) == 1
    assert isinstance(matches[0], TheoryLine)
    assert abs(matches[0].value - expected) <= 1e-12
    assert matches[0].converged is True


def test_theory_lines_skip_inconclusive_and_warn_for_unmappable():
    rps = load_game_spec(FIXTURES / "rock_paper_scissors/game_spec.json")
    rps_solutions = load_solutions(FIXTURES / "rock_paper_scissors/solutions.json", rps)
    with warnings.catch_warnings(record=True) as caught:
        warnings.simplefilter("always")
        lines = theory_lines(rps, rps_solutions, "game.population_share.")
    assert all(not line.solution_id.startswith("rep-f") for line in lines)
    assert caught == []

    component = load_game_spec(FIXTURES / "replicator_component/game_spec.json")
    component_solutions = load_solutions(
        FIXTURES / "replicator_component/solutions.json", component
    )
    with pytest.warns(UserWarning, match="rep-c0"):
        theory_lines(component, component_solutions, "game.population_share.")


def test_theory_lines_excludes_unconverged_continuous_solution_by_default():
    spec = load_game_spec(FIXTURES / "cournot/game_spec.json")
    solutions = load_solutions(FIXTURES / "cournot/solutions.json", spec)
    unconverged = replace(solutions[0], converged=False)
    with warnings.catch_warnings(record=True) as caught:
        warnings.simplefilter("always")
        lines = theory_lines(spec, [unconverged], "game.mean_action.")
    assert lines == []
    assert caught == []


def test_theory_lines_includes_unconverged_continuous_solution_when_requested():
    spec = load_game_spec(FIXTURES / "cournot/game_spec.json")
    solutions = load_solutions(FIXTURES / "cournot/solutions.json", spec)
    unconverged = replace(solutions[0], converged=False)
    lines = theory_lines(
        spec,
        [unconverged],
        "game.mean_action.",
        include_unconverged=True,
    )
    matches = [line for line in lines if line.metric_id == "0"]
    assert len(matches) == 1
    assert matches[0].value == 22.5
    assert matches[0].converged is False
