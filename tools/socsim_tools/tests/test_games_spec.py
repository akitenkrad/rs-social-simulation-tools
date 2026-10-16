"""Tests for the versioned game-artifact reader."""

from __future__ import annotations

import json
from dataclasses import FrozenInstanceError
from pathlib import Path

import pytest

from socsim_tools.games.spec import (
    ContinuousSolution,
    GameSpec,
    InconclusiveSolution,
    NashSolution,
    PopulationComponent,
    PopulationSolution,
    QreSolution,
    load_game_spec,
    load_solutions,
)

FIXTURES = (
    Path(__file__).resolve().parents[3]
    / "crates/socsim-games/tests/fixtures/viz"
)


def _write_json(tmp_path: Path, name: str, value: object) -> Path:
    path = tmp_path / name
    path.write_text(json.dumps(value), encoding="utf-8")
    return path


def _fixture_json(relative: str) -> dict:
    return json.loads((FIXTURES / relative).read_text(encoding="utf-8"))


def test_loads_all_game_specs_with_rust_float_values():
    paths = sorted(FIXTURES.glob("*/game_spec.json"))
    specs = {path.parent.name: load_game_spec(path) for path in paths}
    assert len(specs) == 10
    assert all(isinstance(spec, GameSpec) for spec in specs.values())

    matrix = specs["matrix_3p"]
    assert matrix.kind == "finite"
    assert matrix.shape == [2, 2, 2]
    assert matrix.payoffs[2] == [float(value) for value in range(200, 208)]
    assert matrix.action_labels == [["a0", "a1"]] * 3
    assert matrix.bounds is None

    cournot = specs["cournot"]
    assert cournot.kind == "continuous"
    assert cournot.bounds == [[[0.0, 100.0]]] * 3
    assert cournot.dimension_labels == [["x0"]] * 3
    assert cournot.action_labels is None
    assert cournot.payoffs is None

    with pytest.raises(FrozenInstanceError):
        matrix.name = "changed"  # type: ignore[misc]


def test_loads_every_solution_kind_from_rust_fixtures():
    loaded = []
    for path in sorted(FIXTURES.glob("*/solutions.json")):
        spec = load_game_spec(path.with_name("game_spec.json"))
        loaded.extend(load_solutions(path, spec))

    assert len(list(FIXTURES.glob("*/solutions.json"))) == 8
    assert any(isinstance(value, NashSolution) for value in loaded)
    assert any(isinstance(value, QreSolution) for value in loaded)
    assert any(isinstance(value, PopulationSolution) for value in loaded)
    assert any(isinstance(value, PopulationComponent) for value in loaded)
    assert any(isinstance(value, ContinuousSolution) for value in loaded)
    assert any(isinstance(value, InconclusiveSolution) for value in loaded)

    mixed = next(value for value in loaded if value.id == "se-1")
    assert isinstance(mixed, NashSolution)
    assert mixed.strategies[0] == [0.6, 0.4000000000000001]
    qre = next(value for value in loaded if isinstance(value, QreSolution))
    assert qre.lambda_ == 1.0


@pytest.mark.parametrize(
    ("change", "message"),
    [
        (lambda data: data.update(schema="wrong"), "schema"),
        (lambda data: data.update(version=2), "version"),
        (lambda data: data.update(kind="unknown"), "kind"),
        (lambda data: data.update(extra=True), "unexpected keys"),
        (
            lambda data: data["payoffs"].update(layout="column_major"),
            "layout",
        ),
        (lambda data: data["payoffs"].update(shape=[2, 3]), "shape"),
        (lambda data: data["metric_actions"][0].update(extra=True), "unexpected"),
    ],
)
def test_rejects_invalid_game_spec(tmp_path, change, message):
    data = _fixture_json("prisoners_dilemma/game_spec.json")
    change(data)
    with pytest.raises(ValueError, match=message):
        load_game_spec(_write_json(tmp_path, "game_spec.json", data))


def test_rejects_nonfinite_and_wrong_types_in_game_spec(tmp_path):
    data = _fixture_json("prisoners_dilemma/game_spec.json")
    data["payoffs"]["values"][0][0] = float("nan")
    with pytest.raises(ValueError, match="finite"):
        load_game_spec(_write_json(tmp_path, "nan.json", data))

    data = _fixture_json("prisoners_dilemma/game_spec.json")
    data["n_roles"] = True
    with pytest.raises(ValueError, match="n_roles"):
        load_game_spec(_write_json(tmp_path, "bool.json", data))

    data = _fixture_json("prisoners_dilemma/game_spec.json")
    data["params"]["nested"] = [float("inf")]
    with pytest.raises(ValueError, match="params.*finite"):
        load_game_spec(_write_json(tmp_path, "params.json", data))


def test_non_ascii_labels_are_preserved(tmp_path):
    data = _fixture_json("prisoners_dilemma/game_spec.json")
    data["role_labels"] = ["行", "列"]
    data["action_labels"] = [["協力", "裏切り"], ["協力", "裏切り"]]
    spec = load_game_spec(_write_json(tmp_path, "unicode.json", data))
    assert spec.role_labels == ["行", "列"]
    assert spec.action_labels[0] == ["協力", "裏切り"]


@pytest.mark.parametrize(
    ("change", "message"),
    [
        (lambda data: data.update(schema="wrong"), "schema"),
        (lambda data: data.update(version=0), "version"),
        (lambda data: data.update(extra=True), "unexpected keys"),
        (lambda data: data["game"].update(name="other"), "does not match"),
        (
            lambda data: data["solutions"][0].update(kind="unknown"),
            "unknown solution kind",
        ),
        (
            lambda data: data["solutions"][0].update(extra=True),
            "unexpected keys",
        ),
        (
            lambda data: data["solutions"][0].update(strategies=[[1.0], [1.0]]),
            "strategies",
        ),
    ],
)
def test_rejects_invalid_solutions(tmp_path, change, message):
    spec = load_game_spec(FIXTURES / "prisoners_dilemma/game_spec.json")
    data = _fixture_json("prisoners_dilemma/solutions.json")
    change(data)
    with pytest.raises(ValueError, match=message):
        load_solutions(_write_json(tmp_path, "solutions.json", data), spec)


def test_rejects_duplicate_solution_ids(tmp_path):
    spec = load_game_spec(FIXTURES / "prisoners_dilemma/game_spec.json")
    data = _fixture_json("prisoners_dilemma/solutions.json")
    data["solutions"][1]["id"] = data["solutions"][0]["id"]
    with pytest.raises(ValueError, match="duplicate solution id"):
        load_solutions(_write_json(tmp_path, "solutions.json", data), spec)
