"""Cross-language checks for finite-game theory helpers."""

from __future__ import annotations

import json
from pathlib import Path

import numpy as np
import pytest

from socsim_tools.games.spec import load_game_spec
from socsim_tools.games.theory import (
    best_responses_2x2,
    pure_nash,
    replicator_field,
    to_barycentric_xy,
)

FIXTURES = (
    Path(__file__).resolve().parents[3]
    / "crates/socsim-games/tests/fixtures/viz"
)


@pytest.mark.parametrize(
    ("game", "expected"),
    [
        ("asym_2x3", [(0, 2)]),
        ("battle_of_sexes", [(0, 0), (1, 1)]),
        ("hawk_dove", [(0, 1), (1, 0)]),
        ("matching_pennies", []),
        ("prisoners_dilemma", [(1, 1)]),
        (
            "replicator_component",
            [(0, 0), (0, 1), (1, 0), (1, 1), (2, 2)],
        ),
        ("rock_paper_scissors", []),
        ("matrix_3p", [(1, 1, 1)]),
        ("weak_prisoners_dilemma", [(0, 1), (1, 0), (1, 1)]),
    ],
)
def test_pure_nash_matches_rust_games(game, expected):
    spec = load_game_spec(FIXTURES / game / "game_spec.json")
    assert pure_nash(spec) == expected


def test_pure_nash_rejects_continuous_games():
    spec = load_game_spec(FIXTURES / "cournot/game_spec.json")
    with pytest.raises(ValueError, match="finite"):
        pure_nash(spec)


def test_best_responses_include_ties_with_rust_tolerance():
    spec = load_game_spec(FIXTURES / "weak_prisoners_dilemma/game_spec.json")
    row, column = best_responses_2x2(spec)
    np.testing.assert_array_equal(row, [[False, True], [True, True]])
    np.testing.assert_array_equal(column, [[False, True], [True, True]])
    assert row.dtype == np.bool_
    assert column.dtype == np.bool_


def test_replicator_field_matches_every_rust_fixture_vector():
    fixture = json.loads(
        (FIXTURES / "replicator_vectors.json").read_text(encoding="utf-8")
    )
    assert fixture["schema"] == "socsim-games.replicator-vectors"
    assert fixture["version"] == 1
    maximum_error = 0.0
    for game in fixture["games"]:
        spec = load_game_spec(FIXTURES / game["game"] / "game_spec.json")
        actual = replicator_field(spec, game["points"])
        expected = np.asarray(game["derivatives"], dtype=float)
        maximum_error = max(maximum_error, float(np.max(np.abs(actual - expected))))
        np.testing.assert_allclose(actual, expected, rtol=0.0, atol=1e-12)
    assert maximum_error <= 1e-12


def test_replicator_field_matches_known_values_and_single_point_shape():
    rps = load_game_spec(FIXTURES / "rock_paper_scissors/game_spec.json")
    np.testing.assert_array_equal(
        replicator_field(rps, [0.5, 0.5, 0.0]), [-0.25, 0.25, 0.0]
    )
    np.testing.assert_allclose(
        replicator_field(rps, [1.0 / 3.0] * 3), 0.0, rtol=0.0, atol=1e-16
    )
    hawk_dove = load_game_spec(FIXTURES / "hawk_dove/game_spec.json")
    np.testing.assert_array_equal(replicator_field(hawk_dove, [0.5, 0.5]), [0, 0])


def test_barycentric_mapping_uses_documented_vertex_order():
    points = np.eye(3)
    expected = np.array([[0.0, 0.0], [1.0, 0.0], [0.5, np.sqrt(3.0) / 2.0]])
    np.testing.assert_allclose(to_barycentric_xy(points), expected, atol=1e-15)
    np.testing.assert_allclose(
        to_barycentric_xy([1.0 / 3.0] * 3),
        [0.5, np.sqrt(3.0) / 6.0],
        atol=1e-15,
    )
