"""Read metric series and game events without optional dependencies."""

from __future__ import annotations

import csv
import json
import math
import os
import warnings
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Iterable

from socsim_tools.games.spec import GameSpec, load_game_spec

RUNVAULT_METRICS_HEADER = (
    "run_uid",
    "step",
    "step_unit",
    "scope",
    "name",
    "value",
)
RUNVAULT_EVENT_FIELDS = {
    "schema",
    "run_uid",
    "ts",
    "unit_id",
    "t",
    "t_unit",
    "outcome",
    "censored",
    "budget",
}


@dataclass(frozen=True)
class MetricSeries:
    """One source's aligned metric columns."""

    source: Path
    t: list[int]
    axis_name: str
    columns: dict[str, list[float]]


@dataclass(frozen=True)
class SeriesTable:
    """Metric series from one or more compatible sources."""

    series: list[MetricSeries]
    labels: dict[str, str]
    spec: GameSpec | None = None


@dataclass(frozen=True)
class ActionLog:
    """Per-step actions aligned to a required agent roster."""

    agents: list[int]
    kinds: list[str]
    t: list[int]
    actions: list[list[int | None]]
    roles: list[list[int | None]]
    payoffs: list[list[float]]
    n_played: list[list[int]]


@dataclass(frozen=True)
class LlmCall:
    """The fields used to mark an LLM decision attempt."""

    agent: int
    t: int
    resolution: str
    attempt: int


def load_metric_series(
    sources: Iterable[str | os.PathLike[str]], prefix: str
) -> SeriesTable:
    """Read matching metrics from runvault runs or socsim JSONL files.

    Every source is sorted by time. Duplicate ``(time, name)`` pairs and
    columns with different time points are rejected.
    """

    if not isinstance(prefix, str) or not prefix:
        raise ValueError("prefix must be a non-empty string")
    paths = [Path(source) for source in sources]
    if not paths:
        raise ValueError("sources must contain at least one path")

    series: list[MetricSeries] = []
    specs: list[GameSpec | None] = []
    for path in paths:
        if path.is_dir() and (path / "metrics.csv").is_file():
            current = _load_runvault_metrics(path, prefix)
            spec_path = path / "artifacts/socsim_games/game_spec.json"
        elif path.is_file() and path.suffix == ".jsonl":
            current = _load_jsonl_metrics(path, prefix)
            spec_path = path.parent / "game_spec.json"
        else:
            raise ValueError(
                f"unsupported metric source {path}: expected a run directory "
                "with metrics.csv or a .jsonl file"
            )
        series.append(current)
        specs.append(load_game_spec(spec_path) if spec_path.is_file() else None)

    found_specs = [spec for spec in specs if spec is not None]
    if found_specs and any(spec != found_specs[0] for spec in found_specs[1:]):
        raise ValueError("game specifications do not match across sources")

    column_sets = [set(item.columns) for item in series]
    if any(columns != column_sets[0] for columns in column_sets[1:]):
        raise ValueError("metric column names do not match across sources")
    if any(spec is None for spec in specs):
        warnings.warn(
            "game_spec.json is missing for at least one source; compatibility "
            "was checked using metric column names only",
            UserWarning,
            stacklevel=2,
        )

    spec = found_specs[0] if found_specs else None
    labels = {name: _metric_label(name, prefix, spec) for name in series[0].columns}
    return SeriesTable(series=series, labels=labels, spec=spec)


def load_step_actions(
    source: str | os.PathLike[str], repo_id: str | None = None
) -> ActionLog:
    """Read a roster and its per-step action records from one source."""

    events, runvault = _load_events(Path(source))
    roster: tuple[list[int], list[str]] | None = None
    roster_payload: dict[str, Any] | None = None
    steps: list[
        tuple[int, list[int | None], list[int | None], list[float], list[int]]
    ] = []
    seen_t: set[int] = set()

    for kind, raw_t, payload, context in events:
        if _event_matches(kind, "game.roster", repo_id, runvault):
            candidate = _parse_roster(payload)
            if roster_payload is not None and payload != roster_payload:
                raise ValueError("conflicting roster events")
            roster = candidate
            roster_payload = payload
        elif _event_matches(kind, "game.step_actions", repo_id, runvault):
            t = _integer(raw_t, f"{context}: event t")
            if t in seen_t:
                raise ValueError(f"duplicate game.step_actions event at t={t}")
            seen_t.add(t)
            steps.append((t, *_parse_step_actions(payload)))

    if roster is None:
        raise ValueError("game.step_actions requires a game.roster event")

    agents, kinds = roster
    expected = len(agents)
    for t, actions, roles, payoffs, n_played in steps:
        fields = {
            "actions": actions,
            "roles": roles,
            "payoffs": payoffs,
            "n_played": n_played,
        }
        for name, values in fields.items():
            if len(values) != expected:
                raise ValueError(
                    f"{name} at t={t} has length {len(values)}, "
                    f"but the roster has {expected} agents"
                )

    steps.sort(key=lambda row: row[0])
    return ActionLog(
        agents=agents,
        kinds=kinds,
        t=[row[0] for row in steps],
        actions=[row[1] for row in steps],
        roles=[row[2] for row in steps],
        payoffs=[row[3] for row in steps],
        n_played=[row[4] for row in steps],
    )


def load_llm_calls(
    source: str | os.PathLike[str], repo_id: str | None = None
) -> list[LlmCall]:
    """Read LLM call attempts in time and input order."""

    events, runvault = _load_events(Path(source))
    calls: list[tuple[int, LlmCall]] = []
    for order, (kind, raw_t, payload, context) in enumerate(events):
        if not _event_matches(kind, "game.llm_call", repo_id, runvault):
            continue
        t = _integer(raw_t, f"{context}: event t")
        agent = _integer(payload.get("agent"), "game.llm_call.agent")
        attempt = _integer(payload.get("attempt"), "game.llm_call.attempt")
        if agent < 0 or attempt < 0:
            raise ValueError("game.llm_call agent and attempt must be non-negative")
        resolution = payload.get("resolution")
        if resolution not in {"accept", "retry", "giveup"}:
            raise ValueError(
                "game.llm_call.resolution must be 'accept', 'retry', or 'giveup'"
            )
        calls.append(
            (
                order,
                LlmCall(
                    agent=agent,
                    t=t,
                    resolution=resolution,
                    attempt=attempt,
                ),
            )
        )
    calls.sort(key=lambda item: (item[1].t, item[0]))
    return [call for _, call in calls]


def _load_runvault_metrics(path: Path, prefix: str) -> MetricSeries:
    rows: list[dict[str, str]] = []
    with (path / "metrics.csv").open(newline="", encoding="utf-8") as source:
        reader = csv.DictReader(source)
        if tuple(reader.fieldnames or ()) != RUNVAULT_METRICS_HEADER:
            raise ValueError(
                "runvault metrics.csv header must be "
                + ",".join(RUNVAULT_METRICS_HEADER)
            )
        rows.extend(reader)

    if any(None in row or any(value is None for value in row.values()) for row in rows):
        raise ValueError("runvault metrics.csv contains a malformed row")

    step_units = {row["step_unit"] for row in rows}
    if len(step_units) != 1:
        raise ValueError("runvault metrics.csv must contain one step_unit")
    scopes = {row["scope"] for row in rows}
    if len(scopes) != 1:
        raise ValueError("runvault metrics.csv must contain one scope")
    return _build_metric_series(
        path,
        next(iter(step_units)),
        (
            (
                _integer_text(row["step"], "metrics.csv step"),
                row["name"],
                _float_text(row["value"], "metrics.csv value"),
            )
            for row in rows
            if row["name"].startswith(prefix)
        ),
    )


def _load_jsonl_metrics(path: Path, prefix: str) -> MetricSeries:
    records: list[tuple[int, str, float]] = []
    for line_number, value in _jsonl_objects(path):
        if value.get("type") != "metric":
            continue
        key = value.get("key")
        if not isinstance(key, str):
            raise ValueError(f"{path}:{line_number}: metric key must be a string")
        if not key.startswith(prefix):
            continue
        records.append(
            (
                _integer(value.get("t"), f"{path}:{line_number}: metric t"),
                key,
                _number(
                    value.get("value"),
                    f"{path}:{line_number}: metric value",
                ),
            )
        )
    return _build_metric_series(path, "t", records)


def _build_metric_series(
    source: Path,
    axis_name: str,
    records: Iterable[tuple[int, str, float]],
) -> MetricSeries:
    by_name: dict[str, dict[int, float]] = {}
    for t, name, value in records:
        values = by_name.setdefault(name, {})
        if t in values:
            raise ValueError(f"duplicate metric ({t}, {name!r}) in {source}")
        values[t] = value
    if not by_name:
        raise ValueError(f"no metrics with the requested prefix in {source}")

    names = sorted(by_name)
    times = set(by_name[names[0]])
    if any(set(by_name[name]) != times for name in names[1:]):
        raise ValueError(f"metric columns in {source} must use the same time points")
    ordered_times = sorted(times)
    columns = {name: [by_name[name][t] for t in ordered_times] for name in names}
    return MetricSeries(
        source=source,
        t=ordered_times,
        axis_name=axis_name,
        columns=columns,
    )


def _metric_label(name: str, prefix: str, spec: GameSpec | None) -> str:
    metric_id = name[len(prefix) :] if prefix.endswith(".") else name
    if spec is None or spec.action_labels is None:
        return metric_id
    by_id = {action.id: action for action in spec.metric_actions}
    action = by_id.get(metric_id)
    if action is None:
        return metric_id
    role = 0 if action.role is None else action.role
    return spec.action_labels[role][action.action]


def _load_events(
    path: Path,
) -> tuple[list[tuple[str, Any, dict[str, Any], str]], bool]:
    if path.is_dir():
        event_path = path / "events.jsonl"
        runvault = True
    elif path.is_file() and path.suffix == ".jsonl":
        event_path = path
        runvault = False
    else:
        raise ValueError(
            f"unsupported event source {path}: expected a run directory or .jsonl file"
        )
    if not event_path.is_file():
        raise ValueError(f"event source does not contain {event_path.name}")

    events: list[tuple[str, Any, dict[str, Any], str]] = []
    for line_number, value in _jsonl_objects(event_path):
        if runvault:
            kind = value.get("schema")
            if not isinstance(kind, str):
                raise ValueError(
                    f"{event_path}:{line_number}: event schema must be a string"
                )
            payload = {
                key: item
                for key, item in value.items()
                if key not in RUNVAULT_EVENT_FIELDS
            }
        else:
            if value.get("type") != "event":
                continue
            kind = value.get("kind")
            if not isinstance(kind, str):
                raise ValueError(
                    f"{event_path}:{line_number}: event kind must be a string"
                )
            payload = value.get("payload")
            if not isinstance(payload, dict):
                raise ValueError(
                    f"{event_path}:{line_number}: event payload must be an object"
                )
        events.append((kind, value.get("t"), payload, f"{event_path}:{line_number}"))
    return events, runvault


def _event_matches(
    actual: str, expected: str, repo_id: str | None, runvault: bool
) -> bool:
    if not runvault:
        return actual == expected
    if repo_id is not None:
        return actual == f"x.{repo_id}.{expected}"
    return actual == expected or actual.endswith(f".{expected}")


def _parse_roster(payload: dict[str, Any]) -> tuple[list[int], list[str]]:
    agents = _integer_list(payload.get("agents"), "game.roster.agents")
    kinds = _string_list(payload.get("kinds"), "game.roster.kinds")
    if len(agents) != len(kinds):
        raise ValueError("game.roster agents and kinds must have the same length")
    if len(agents) != len(set(agents)) or any(agent < 0 for agent in agents):
        raise ValueError("game.roster agents must be unique non-negative integers")
    if payload.get("decision_scope") != "per_step":
        raise ValueError("game.roster decision_scope must be 'per_step'")
    return agents, kinds


def _parse_step_actions(
    payload: dict[str, Any],
) -> tuple[list[int | None], list[int | None], list[float], list[int]]:
    actions = _optional_integer_list(
        payload.get("actions"), "game.step_actions.actions"
    )
    roles = _optional_integer_list(payload.get("roles"), "game.step_actions.roles")
    payoffs = _number_list(payload.get("payoffs"), "game.step_actions.payoffs")
    n_played = _integer_list(payload.get("n_played"), "game.step_actions.n_played")
    if any(value is not None and value < 0 for value in actions + roles):
        raise ValueError("game.step_actions actions and roles must be non-negative")
    if any(value < 0 for value in n_played):
        raise ValueError("game.step_actions n_played must be non-negative")
    return actions, roles, payoffs, n_played


def _jsonl_objects(path: Path) -> Iterable[tuple[int, dict[str, Any]]]:
    with path.open(encoding="utf-8") as source:
        for line_number, line in enumerate(source, start=1):
            if not line.strip():
                continue
            try:
                value = json.loads(line)
            except json.JSONDecodeError as error:
                raise ValueError(f"{path}:{line_number}: invalid JSON") from error
            if not isinstance(value, dict):
                raise ValueError(f"{path}:{line_number}: expected a JSON object")
            yield line_number, value


def _integer(value: Any, context: str) -> int:
    if type(value) is not int:
        raise ValueError(f"{context} must be an integer")
    return value


def _number(value: Any, context: str) -> float:
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise ValueError(f"{context} must be a number")
    result = float(value)
    if not math.isfinite(result):
        raise ValueError(f"{context} must be finite")
    return result


def _integer_text(value: str, context: str) -> int:
    try:
        result = int(value)
    except ValueError as error:
        raise ValueError(f"{context} must be an integer") from error
    return result


def _float_text(value: str, context: str) -> float:
    try:
        result = float(value)
    except ValueError as error:
        raise ValueError(f"{context} must be a number") from error
    if not math.isfinite(result):
        raise ValueError(f"{context} must be finite")
    return result


def _list(value: Any, context: str) -> list[Any]:
    if not isinstance(value, list):
        raise ValueError(f"{context} must be a list")
    return value


def _integer_list(value: Any, context: str) -> list[int]:
    return [
        _integer(item, f"{context}[{index}]")
        for index, item in enumerate(_list(value, context))
    ]


def _optional_integer_list(value: Any, context: str) -> list[int | None]:
    result: list[int | None] = []
    for index, item in enumerate(_list(value, context)):
        result.append(None if item is None else _integer(item, f"{context}[{index}]"))
    return result


def _number_list(value: Any, context: str) -> list[float]:
    return [
        _number(item, f"{context}[{index}]")
        for index, item in enumerate(_list(value, context))
    ]


def _string_list(value: Any, context: str) -> list[str]:
    result = _list(value, context)
    if any(not isinstance(item, str) for item in result):
        raise ValueError(f"{context} must contain only strings")
    return result


__all__ = [
    "ActionLog",
    "LlmCall",
    "MetricSeries",
    "SeriesTable",
    "load_llm_calls",
    "load_metric_series",
    "load_step_actions",
]
