"""Contract tests for metric-series and event readers."""

from __future__ import annotations

import csv
import json
from dataclasses import FrozenInstanceError
from pathlib import Path

import pytest

from socsim_tools.games.records import (
    ActionLog,
    LlmCall,
    MetricSeries,
    SeriesTable,
    load_llm_calls,
    load_metric_series,
    load_step_actions,
)

RUST_FIXTURES = (
    Path(__file__).resolve().parents[3] / "crates/socsim-games/tests/fixtures/viz"
)
RUNVAULT_FIXTURE = Path(__file__).parent / "fixtures/runvault_run"


def _write_jsonl(path: Path, rows: list[object]) -> Path:
    path.write_text(
        "".join(json.dumps(row) + "\n" for row in rows),
        encoding="utf-8",
    )
    return path


def test_load_metric_series_reads_jsonl_sorts_and_labels_columns():
    sources = [RUST_FIXTURES / f"ipd/seed_{seed}.jsonl" for seed in range(5)]
    table = load_metric_series(sources, "game.played_share.")

    assert isinstance(table, SeriesTable)
    assert len(table.series) == 5
    assert all(isinstance(series, MetricSeries) for series in table.series)
    first = table.series[0]
    assert first.source == sources[0]
    assert first.axis_name == "t"
    assert first.t == list(range(1, 51))
    assert list(first.columns) == [
        "game.played_share.a0",
        "game.played_share.a1",
    ]
    assert first.columns["game.played_share.a0"][:3] == [0.5, 0.0, 0.5]
    assert table.labels == {
        "game.played_share.a0": "C",
        "game.played_share.a1": "D",
    }
    with pytest.raises(FrozenInstanceError):
        first.axis_name = "round"  # type: ignore[misc]


def test_load_metric_series_reads_runvault_long_form_like_jsonl():
    runvault = load_metric_series([RUNVAULT_FIXTURE], "game.played_share.")
    jsonl = load_metric_series(
        [RUST_FIXTURES / "ipd/seed_0.jsonl"], "game.played_share."
    )

    actual = runvault.series[0]
    expected = jsonl.series[0]
    assert actual.axis_name == "round"
    assert actual.t == expected.t[:3]
    assert actual.columns == {
        name: values[:3] for name, values in expected.columns.items()
    }
    assert runvault.labels == jsonl.labels
    assert runvault.spec == jsonl.spec
    assert runvault.spec is not None


def test_load_metric_series_rejects_duplicates_and_misaligned_columns(tmp_path):
    duplicate = [
        {"type": "metric", "t": 2, "key": "game.x.a0", "value": 0.2},
        {"type": "metric", "t": 1, "key": "game.x.a0", "value": 0.1},
        {"type": "metric", "t": 1, "key": "game.x.a0", "value": 0.3},
    ]
    with pytest.raises(ValueError, match="duplicate"):
        load_metric_series(
            [_write_jsonl(tmp_path / "duplicate.jsonl", duplicate)], "game.x."
        )

    missing = [
        {"type": "metric", "t": 1, "key": "game.x.a0", "value": 0.1},
        {"type": "metric", "t": 2, "key": "game.x.a0", "value": 0.2},
        {"type": "metric", "t": 1, "key": "game.x.a1", "value": 0.9},
    ]
    with pytest.raises(ValueError, match="same time points"):
        load_metric_series(
            [_write_jsonl(tmp_path / "missing.jsonl", missing)], "game.x."
        )


def test_load_metric_series_without_spec_uses_metric_id_verbatim(tmp_path):
    source = _write_jsonl(
        tmp_path / "metrics.jsonl",
        [{"type": "metric", "t": 1, "key": "game.coop_rate", "value": 0.5}],
    )
    with pytest.warns(UserWarning, match="game_spec.json"):
        table = load_metric_series([source], "game.coop_rate")
    assert table.labels == {"game.coop_rate": "game.coop_rate"}
    assert table.spec is None


def test_load_metric_series_rejects_wide_and_mixed_runvault_metadata(tmp_path):
    run = tmp_path / "run"
    run.mkdir()
    with (run / "metrics.csv").open("w", newline="", encoding="utf-8") as output:
        csv.writer(output).writerows([["step", "game.x.a0"], [1, 0.5]])
    with pytest.raises(ValueError, match="header"):
        load_metric_series([run], "game.x.")

    with (run / "metrics.csv").open("w", newline="", encoding="utf-8") as output:
        csv.writer(output).writerows(
            [
                ["run_uid", "step", "step_unit", "scope", "name", "value"],
                ["r", 1, "round", "run", "game.x.a0", 0.5],
                ["r", 2, "generation", "agent", "game.x.a0", 0.6],
            ]
        )
    with pytest.raises(ValueError, match="step_unit"):
        load_metric_series([run], "game.x.")


def test_load_step_actions_reads_socsim_and_runvault_equivalently():
    jsonl = load_step_actions(RUST_FIXTURES / "ipd/seed_0.jsonl")
    runvault = load_step_actions(RUNVAULT_FIXTURE, repo_id="socsim-games-viz")

    assert isinstance(jsonl, ActionLog)
    assert jsonl.agents == [0, 1]
    assert jsonl.kinds == ["tit_for_tat", "uniform_random"]
    assert jsonl.t == list(range(1, 51))
    assert jsonl.actions[:3] == [[0, 1], [1, 1], [1, 0]]
    assert jsonl.roles[:3] == [[None, None]] * 3
    assert jsonl.payoffs[:3] == [[0.0, 5.0], [1.0, 1.0], [5.0, 0.0]]
    assert jsonl.n_played[:3] == [[1, 1]] * 3
    assert runvault == ActionLog(
        agents=jsonl.agents,
        kinds=jsonl.kinds,
        t=jsonl.t[:3],
        actions=jsonl.actions[:3],
        roles=jsonl.roles[:3],
        payoffs=jsonl.payoffs[:3],
        n_played=jsonl.n_played[:3],
    )


def test_load_step_actions_handles_roster_restarts_and_rejects_bad_logs(tmp_path):
    roster = {
        "type": "event",
        "t": 1,
        "kind": "game.roster",
        "payload": {
            "agents": [0, 1],
            "kinds": ["held", "held"],
            "decision_scope": "per_step",
        },
    }
    step = {
        "type": "event",
        "t": 1,
        "kind": "game.step_actions",
        "payload": {
            "actions": [0, 1],
            "roles": [None, None],
            "payoffs": [1.0, 2.0],
            "n_played": [1, 1],
        },
    }
    valid = _write_jsonl(tmp_path / "valid.jsonl", [roster, step, roster])
    assert load_step_actions(valid).actions == [[0, 1]]

    changed = json.loads(json.dumps(roster))
    changed["payload"]["agents"] = [0, 2]
    with pytest.raises(ValueError, match="conflicting roster"):
        load_step_actions(
            _write_jsonl(tmp_path / "changed.jsonl", [roster, changed, step])
        )
    with pytest.raises(ValueError, match="roster"):
        load_step_actions(_write_jsonl(tmp_path / "missing.jsonl", [step]))

    short = json.loads(json.dumps(step))
    short["payload"]["actions"] = [0]
    with pytest.raises(ValueError, match="actions.*roster"):
        load_step_actions(_write_jsonl(tmp_path / "short.jsonl", [roster, short]))


def test_load_llm_calls_reads_nested_and_flat_events(tmp_path):
    payloads = [
        {"agent": 1, "resolution": "retry", "attempt": 0},
        {"agent": 1, "resolution": "accept", "attempt": 1},
        {"agent": 0, "resolution": "giveup", "attempt": 2},
    ]
    nested = [
        {
            "type": "event",
            "t": t,
            "kind": "game.llm_call",
            "payload": payload,
        }
        for t, payload in zip((7, 7, 3), payloads)
    ]
    nested_path = _write_jsonl(tmp_path / "llm.jsonl", nested)
    assert load_llm_calls(nested_path) == [
        LlmCall(agent=0, t=3, resolution="giveup", attempt=2),
        LlmCall(agent=1, t=7, resolution="retry", attempt=0),
        LlmCall(agent=1, t=7, resolution="accept", attempt=1),
    ]

    run = tmp_path / "run"
    run.mkdir()
    flattened = [
        {
            **payload,
            "schema": "x.repo.game.llm_call",
            "run_uid": "r",
            "ts": f"2026-10-16T00:00:0{index}Z",
            "t": t,
            "t_unit": "round",
        }
        for index, (t, payload) in enumerate(zip((7, 7, 3), payloads))
    ]
    _write_jsonl(run / "events.jsonl", flattened)
    assert load_llm_calls(run, repo_id="repo") == load_llm_calls(nested_path)


def test_event_readers_ignore_unrelated_runvault_events_without_time(tmp_path):
    run = tmp_path / "run"
    run.mkdir()
    _write_jsonl(
        run / "events.jsonl",
        [
            {"schema": "x.repo.other", "run_uid": "r", "ts": "now"},
            {
                "agent": 2,
                "attempt": 0,
                "resolution": "accept",
                "schema": "x.repo.game.llm_call",
                "run_uid": "r",
                "t": 4,
                "t_unit": "round",
                "ts": "later",
            },
        ],
    )
    assert load_llm_calls(run, repo_id="repo") == [
        LlmCall(agent=2, t=4, resolution="accept", attempt=0)
    ]
