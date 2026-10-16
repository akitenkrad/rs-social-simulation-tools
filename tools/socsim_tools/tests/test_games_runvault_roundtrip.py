"""Roundtrip a real socsim-games run through runvault's on-disk format."""

from __future__ import annotations

import os
from pathlib import Path

import pytest

from socsim_tools.games.records import load_metric_series, load_step_actions

RUST_FIXTURES = (
    Path(__file__).resolve().parents[3] / "crates/socsim-games/tests/fixtures/viz"
)
RUN_ENV = "SOCSIM_GAMES_VIZ_RUN"


def test_real_runvault_run_roundtrips_like_the_socsim_jsonl_fixture():
    run_value = os.environ.get(RUN_ENV)
    if run_value is None:
        pytest.skip(f"{RUN_ENV} is not set")

    run = Path(run_value)
    fixture = RUST_FIXTURES / "ipd/seed_0.jsonl"

    actual_metrics = load_metric_series([run], "game.played_share.")
    expected_metrics = load_metric_series([fixture], "game.played_share.")
    assert actual_metrics.series[0].axis_name == "round"
    assert actual_metrics.series[0].t == expected_metrics.series[0].t
    assert actual_metrics.series[0].columns == expected_metrics.series[0].columns
    assert actual_metrics.labels == {
        "game.played_share.a0": "C",
        "game.played_share.a1": "D",
    }
    assert actual_metrics.spec == expected_metrics.spec

    assert load_step_actions(run, repo_id="socsim-games-viz") == load_step_actions(
        fixture
    )
