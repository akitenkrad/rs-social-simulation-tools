"""Read and validate versioned socsim game specifications and solutions.

This module intentionally uses only the Python standard library so exported
artifacts can be inspected without installing the plotting extra.
"""

from __future__ import annotations

import json
import math
import os
from dataclasses import dataclass
from pathlib import Path
from typing import Any, TypeAlias

GAME_SPEC_SCHEMA = "socsim-games.game-spec"
SOLUTIONS_SCHEMA = "socsim-games.solutions"
SCHEMA_VERSION = 1
PROBABILITY_TOLERANCE = 1e-9


@dataclass(frozen=True)
class MetricAction:
    """A stable metric identifier and its corresponding action."""

    id: str
    role: int | None
    action: int


@dataclass(frozen=True)
class GameSpec:
    """A validated finite or continuous game definition."""

    kind: str
    name: str
    n_roles: int
    role_labels: list[str]
    action_labels: list[list[str]] | None
    payoffs: list[list[float]] | None
    shape: list[int] | None
    symmetric: bool | None
    metric_scheme: str | None
    metric_actions: list[MetricAction]
    bounds: list[list[list[float]]] | None
    dimension_labels: list[list[str]] | None
    params: Any | None


@dataclass(frozen=True)
class NashSolution:
    """A Nash equilibrium exported by a finite-game solver."""

    id: str
    solver: str
    strategies: list[list[float]]
    payoffs: list[float]
    regret: float
    pure: bool


@dataclass(frozen=True)
class QreSolution:
    """A logit quantal-response equilibrium."""

    id: str
    solver: str
    lambda_: float
    strategies: list[list[float]]
    residual: float


@dataclass(frozen=True)
class PopulationSolution:
    """An isolated population fixed point."""

    id: str
    solver: str
    shares: list[float]
    ess: str | None


@dataclass(frozen=True)
class PopulationComponent:
    """An affine component of population fixed points."""

    id: str
    solver: str
    base: list[float]
    directions: list[list[float]]


@dataclass(frozen=True)
class ContinuousSolution:
    """A continuous-game action profile."""

    id: str
    solver: str
    actions: list[list[float]]
    residual: float
    converged: bool


@dataclass(frozen=True)
class InconclusiveSolution:
    """A finite-game support that a solver could not resolve."""

    id: str
    solver: str
    support: list[list[int]]
    reason: str


Solution: TypeAlias = (
    NashSolution
    | QreSolution
    | PopulationSolution
    | PopulationComponent
    | ContinuousSolution
    | InconclusiveSolution
)


def load_game_spec(path: str | os.PathLike[str]) -> GameSpec:
    """Load and validate a schema-version-1 ``game_spec.json`` file.

    Raises:
        ValueError: If the schema, version, keys, types, or dimensions are
            inconsistent with the exported artifact contract.
    """

    data = _load_json_object(path, "game_spec.json")
    _require_schema(data, GAME_SPEC_SCHEMA, "game_spec.json")
    kind = _string(data.get("kind"), "game_spec.json.kind")
    if kind == "finite":
        return _load_finite_spec(data)
    if kind == "continuous":
        return _load_continuous_spec(data)
    raise ValueError(f"game_spec.json: unknown kind {kind!r}")


def load_solutions(
    path: str | os.PathLike[str], spec: GameSpec
) -> list[Solution]:
    """Load solutions and verify that their game copy matches ``spec``.

    Raises:
        ValueError: If the schema, version, keys, solution shapes, or copied
            game identity are invalid.
    """

    if not isinstance(spec, GameSpec):
        raise TypeError("spec must be a GameSpec")
    data = _load_json_object(path, "solutions.json")
    _require_schema(data, SOLUTIONS_SCHEMA, "solutions.json")
    _exact_keys(
        data,
        {"schema", "version", "game", "solutions"},
        set(),
        "solutions.json",
    )
    game = _object(data["game"], "solutions.json.game")
    _validate_game_copy(game, spec)
    raw_solutions = _list(data["solutions"], "solutions.json.solutions")
    solutions: list[Solution] = []
    ids: set[str] = set()
    for index, raw in enumerate(raw_solutions):
        context = f"solutions.json.solutions[{index}]"
        item = _object(raw, context)
        kind = _string(item.get("kind"), f"{context}.kind")
        solution = _load_solution(item, kind, spec, context)
        if solution.id in ids:
            raise ValueError(
                f"solutions.json: duplicate solution id {solution.id!r}"
            )
        ids.add(solution.id)
        solutions.append(solution)
    return solutions


def _load_finite_spec(data: dict[str, Any]) -> GameSpec:
    required = {
        "schema",
        "version",
        "kind",
        "name",
        "n_roles",
        "role_labels",
        "action_labels",
        "payoffs",
        "symmetric",
        "metric_scheme",
        "metric_actions",
    }
    _exact_keys(data, required, {"params"}, "game_spec.json")
    name, n_roles, role_labels = _common_spec_fields(data)
    action_labels = _label_matrix(
        data["action_labels"], n_roles, "game_spec.json.action_labels"
    )
    payoffs_data = _object(data["payoffs"], "game_spec.json.payoffs")
    _exact_keys(
        payoffs_data,
        {"layout", "shape", "values"},
        set(),
        "game_spec.json.payoffs",
    )
    layout = _string(payoffs_data["layout"], "game_spec.json.payoffs.layout")
    if layout != "row_major_role0_outer":
        raise ValueError(
            "game_spec.json.payoffs.layout must be 'row_major_role0_outer'"
        )
    shape = _integer_list(payoffs_data["shape"], "game_spec.json.payoffs.shape")
    if len(shape) != n_roles:
        raise ValueError(
            "game_spec.json.payoffs.shape must have one entry per role"
        )
    for role, (labels, count) in enumerate(zip(action_labels, shape)):
        if len(labels) != count:
            raise ValueError(
                f"game_spec.json.action_labels[{role}] has length {len(labels)}, "
                f"expected {count} from shape"
            )
    profile_count = math.prod(shape)
    raw_values = _list(payoffs_data["values"], "game_spec.json.payoffs.values")
    if len(raw_values) != n_roles:
        raise ValueError(
            "game_spec.json.payoffs.values must have one row per role"
        )
    payoffs = []
    for role, row in enumerate(raw_values):
        values = _number_list(row, f"game_spec.json.payoffs.values[{role}]")
        if len(values) != profile_count:
            raise ValueError(
                f"game_spec.json.payoffs.values[{role}] has length {len(values)}, "
                f"expected {profile_count} from shape"
            )
        payoffs.append(values)

    symmetric = data["symmetric"]
    if type(symmetric) is not bool:
        raise ValueError("game_spec.json.symmetric must be a boolean")
    metric_scheme = _string(
        data["metric_scheme"], "game_spec.json.metric_scheme"
    )
    if metric_scheme not in {"pooled", "per_role"}:
        raise ValueError(
            "game_spec.json.metric_scheme must be 'pooled' or 'per_role'"
        )
    raw_actions = _list(
        data["metric_actions"], "game_spec.json.metric_actions"
    )
    metric_actions = [
        _load_metric_action(value, index, metric_scheme, shape)
        for index, value in enumerate(raw_actions)
    ]
    ids = [action.id for action in metric_actions]
    if len(ids) != len(set(ids)):
        raise ValueError("game_spec.json.metric_actions contains duplicate ids")
    _validate_json_finite(data.get("params"), "game_spec.json.params")
    return GameSpec(
        kind="finite",
        name=name,
        n_roles=n_roles,
        role_labels=role_labels,
        action_labels=action_labels,
        payoffs=payoffs,
        shape=shape,
        symmetric=symmetric,
        metric_scheme=metric_scheme,
        metric_actions=metric_actions,
        bounds=None,
        dimension_labels=None,
        params=data.get("params"),
    )


def _load_continuous_spec(data: dict[str, Any]) -> GameSpec:
    required = {
        "schema",
        "version",
        "kind",
        "name",
        "n_roles",
        "role_labels",
        "bounds",
        "dimension_labels",
    }
    _exact_keys(data, required, {"params"}, "game_spec.json")
    name, n_roles, role_labels = _common_spec_fields(data)
    if n_roles < 1:
        raise ValueError("game_spec.json.n_roles must be at least 1")
    raw_bounds = _list(data["bounds"], "game_spec.json.bounds")
    if len(raw_bounds) != n_roles:
        raise ValueError("game_spec.json.bounds must have one row per role")
    bounds: list[list[list[float]]] = []
    for role, raw_role_bounds in enumerate(raw_bounds):
        entries = _list(raw_role_bounds, f"game_spec.json.bounds[{role}]")
        if not entries:
            raise ValueError(
                f"game_spec.json.bounds[{role}] must contain at least one dimension"
            )
        role_bounds = []
        for dim, raw_bound in enumerate(entries):
            context = f"game_spec.json.bounds[{role}][{dim}]"
            pair = _list(raw_bound, context)
            if len(pair) != 2:
                raise ValueError(f"{context} must contain [lo, hi]")
            lo = _number(pair[0], f"{context}[0]")
            hi = _number(pair[1], f"{context}[1]")
            if lo >= hi:
                raise ValueError(f"{context} must satisfy lo < hi")
            role_bounds.append([lo, hi])
        bounds.append(role_bounds)
    dimension_labels = _label_matrix(
        data["dimension_labels"], n_roles, "game_spec.json.dimension_labels"
    )
    for role, (labels, role_bounds) in enumerate(zip(dimension_labels, bounds)):
        if len(labels) != len(role_bounds):
            raise ValueError(
                f"game_spec.json.dimension_labels[{role}] has length {len(labels)}, "
                f"expected {len(role_bounds)} from bounds"
            )
    _validate_json_finite(data.get("params"), "game_spec.json.params")
    return GameSpec(
        kind="continuous",
        name=name,
        n_roles=n_roles,
        role_labels=role_labels,
        action_labels=None,
        payoffs=None,
        shape=None,
        symmetric=None,
        metric_scheme=None,
        metric_actions=[],
        bounds=bounds,
        dimension_labels=dimension_labels,
        params=data.get("params"),
    )


def _common_spec_fields(
    data: dict[str, Any],
) -> tuple[str, int, list[str]]:
    name = _string(data["name"], "game_spec.json.name")
    if not name:
        raise ValueError("game_spec.json.name must not be empty")
    n_roles = _integer(data["n_roles"], "game_spec.json.n_roles")
    role_labels = _string_list(data["role_labels"], "game_spec.json.role_labels")
    if len(role_labels) != n_roles:
        raise ValueError(
            "game_spec.json.role_labels must have one label per role"
        )
    if any(not label for label in role_labels):
        raise ValueError("game_spec.json.role_labels must not contain empty labels")
    return name, n_roles, role_labels


def _load_metric_action(
    value: Any, index: int, scheme: str, shape: list[int]
) -> MetricAction:
    context = f"game_spec.json.metric_actions[{index}]"
    data = _object(value, context)
    _exact_keys(data, {"id", "role", "action"}, set(), context)
    identifier = _string(data["id"], f"{context}.id")
    if not identifier:
        raise ValueError(f"{context}.id must not be empty")
    role_value = data["role"]
    if role_value is None:
        role = None
    else:
        role = _integer(role_value, f"{context}.role")
    action = _integer(data["action"], f"{context}.action")
    if scheme == "pooled":
        if role is not None:
            raise ValueError(f"{context}.role must be null for pooled metrics")
        role_for_bounds = 0
    else:
        if role is None or role >= len(shape):
            raise ValueError(f"{context}.role must identify a valid role")
        role_for_bounds = role
    if action >= shape[role_for_bounds]:
        raise ValueError(f"{context}.action is outside the role's action range")
    return MetricAction(id=identifier, role=role, action=action)


def _validate_game_copy(game: dict[str, Any], spec: GameSpec) -> None:
    if spec.kind == "finite":
        _exact_keys(
            game,
            {"name", "n_roles", "action_labels"},
            set(),
            "solutions.json.game",
        )
        expected = {
            "name": spec.name,
            "n_roles": spec.n_roles,
            "action_labels": spec.action_labels,
        }
    else:
        _exact_keys(
            game,
            {"name", "n_roles", "dimension_labels"},
            set(),
            "solutions.json.game",
        )
        expected = {
            "name": spec.name,
            "n_roles": spec.n_roles,
            "dimension_labels": spec.dimension_labels,
        }
    if game != expected:
        raise ValueError(
            "solutions.json.game does not match the supplied GameSpec"
        )


def _load_solution(
    data: dict[str, Any], kind: str, spec: GameSpec, context: str
) -> Solution:
    if kind == "nash":
        _require_finite(spec, context, kind)
        _exact_keys(
            data,
            {
                "id",
                "solver",
                "kind",
                "strategies",
                "payoffs",
                "regret",
                "pure",
            },
            set(),
            context,
        )
        identifier, solver = _solution_identity(data, context)
        strategies = _strategies(data["strategies"], spec, f"{context}.strategies")
        payoffs = _number_list(data["payoffs"], f"{context}.payoffs")
        if len(payoffs) != spec.n_roles:
            raise ValueError(f"{context}.payoffs must have one value per role")
        pure = data["pure"]
        if type(pure) is not bool:
            raise ValueError(f"{context}.pure must be a boolean")
        actual_pure = all(
            probability in {0.0, 1.0}
            for strategy in strategies
            for probability in strategy
        )
        if pure != actual_pure:
            raise ValueError(f"{context}.pure does not match the strategies")
        return NashSolution(
            id=identifier,
            solver=solver,
            strategies=strategies,
            payoffs=payoffs,
            regret=_number(data["regret"], f"{context}.regret"),
            pure=pure,
        )
    if kind == "qre":
        _require_finite(spec, context, kind)
        _exact_keys(
            data,
            {"id", "solver", "kind", "lambda", "strategies", "residual"},
            set(),
            context,
        )
        identifier, solver = _solution_identity(data, context)
        return QreSolution(
            id=identifier,
            solver=solver,
            lambda_=_number(data["lambda"], f"{context}.lambda"),
            strategies=_strategies(
                data["strategies"], spec, f"{context}.strategies"
            ),
            residual=_number(data["residual"], f"{context}.residual"),
        )
    if kind == "population":
        count = _common_action_count(spec, context, kind)
        _exact_keys(
            data,
            {"id", "solver", "kind", "shares", "ess"},
            set(),
            context,
        )
        identifier, solver = _solution_identity(data, context)
        shares = _probability(data["shares"], count, f"{context}.shares")
        ess = data["ess"]
        if ess not in {"true", "false", "inconclusive", None}:
            raise ValueError(
                f"{context}.ess must be 'true', 'false', 'inconclusive', or null"
            )
        return PopulationSolution(identifier, solver, shares, ess)
    if kind == "population_component":
        count = _common_action_count(spec, context, kind)
        _exact_keys(
            data,
            {"id", "solver", "kind", "base", "directions"},
            set(),
            context,
        )
        identifier, solver = _solution_identity(data, context)
        base = _probability(data["base"], count, f"{context}.base")
        raw_directions = _list(data["directions"], f"{context}.directions")
        directions = []
        for index, raw in enumerate(raw_directions):
            values = _number_list(raw, f"{context}.directions[{index}]")
            if len(values) != count:
                raise ValueError(
                    f"{context}.directions[{index}] has length {len(values)}, "
                    f"expected {count}"
                )
            directions.append(values)
        return PopulationComponent(identifier, solver, base, directions)
    if kind == "continuous":
        if spec.kind != "continuous" or spec.dimension_labels is None:
            raise ValueError(f"{context} of kind 'continuous' requires a continuous game")
        _exact_keys(
            data,
            {"id", "solver", "kind", "actions", "residual", "converged"},
            set(),
            context,
        )
        identifier, solver = _solution_identity(data, context)
        raw_actions = _list(data["actions"], f"{context}.actions")
        if len(raw_actions) != spec.n_roles:
            raise ValueError(f"{context}.actions must have one row per role")
        actions = []
        for role, (raw, labels) in enumerate(zip(raw_actions, spec.dimension_labels)):
            values = _number_list(raw, f"{context}.actions[{role}]")
            if len(values) != len(labels):
                raise ValueError(
                    f"{context}.actions[{role}] has length {len(values)}, "
                    f"expected {len(labels)}"
                )
            actions.append(values)
        converged = data["converged"]
        if type(converged) is not bool:
            raise ValueError(f"{context}.converged must be a boolean")
        return ContinuousSolution(
            identifier,
            solver,
            actions,
            _number(data["residual"], f"{context}.residual"),
            converged,
        )
    if kind == "inconclusive":
        _require_finite(spec, context, kind)
        _exact_keys(
            data,
            {"id", "solver", "kind", "support", "reason"},
            set(),
            context,
        )
        identifier, solver = _solution_identity(data, context)
        raw_support = _list(data["support"], f"{context}.support")
        if len(raw_support) != spec.n_roles:
            raise ValueError(f"{context}.support must have one row per role")
        assert spec.shape is not None
        support = []
        for role, (raw, count) in enumerate(zip(raw_support, spec.shape)):
            actions = _integer_list(raw, f"{context}.support[{role}]")
            if any(action >= count for action in actions):
                raise ValueError(
                    f"{context}.support[{role}] contains an out-of-range action"
                )
            support.append(actions)
        return InconclusiveSolution(
            identifier,
            solver,
            support,
            _string(data["reason"], f"{context}.reason"),
        )
    raise ValueError(f"{context}: unknown solution kind {kind!r}")


def _solution_identity(data: dict[str, Any], context: str) -> tuple[str, str]:
    return (
        _string(data["id"], f"{context}.id"),
        _string(data["solver"], f"{context}.solver"),
    )


def _strategies(value: Any, spec: GameSpec, context: str) -> list[list[float]]:
    _require_finite(spec, context, "strategy")
    assert spec.shape is not None
    raw_strategies = _list(value, context)
    if len(raw_strategies) != spec.n_roles:
        raise ValueError(f"{context} must have one strategy per role")
    return [
        _probability(raw, count, f"{context}[{role}]")
        for role, (raw, count) in enumerate(zip(raw_strategies, spec.shape))
    ]


def _probability(value: Any, count: int, context: str) -> list[float]:
    probabilities = _number_list(value, context)
    if len(probabilities) != count:
        raise ValueError(
            f"{context} has length {len(probabilities)}, expected {count}"
        )
    if any(probability < 0.0 for probability in probabilities):
        raise ValueError(f"{context} contains a negative probability")
    total = sum(probabilities)
    if abs(total - 1.0) > PROBABILITY_TOLERANCE:
        raise ValueError(
            f"{context} sums to {total}, expected 1 within {PROBABILITY_TOLERANCE}"
        )
    return probabilities


def _require_finite(spec: GameSpec, context: str, kind: str) -> None:
    if spec.kind != "finite":
        raise ValueError(f"{context} of kind {kind!r} requires a finite game")


def _common_action_count(spec: GameSpec, context: str, kind: str) -> int:
    _require_finite(spec, context, kind)
    assert spec.shape is not None
    if not spec.shape or any(count != spec.shape[0] for count in spec.shape):
        raise ValueError(f"{context} requires one common action count")
    return spec.shape[0]


def _require_schema(data: dict[str, Any], schema: str, context: str) -> None:
    if data.get("schema") != schema:
        raise ValueError(f"{context}: schema must be {schema!r}")
    version = data.get("version")
    if type(version) is not int or version != SCHEMA_VERSION:
        raise ValueError(f"{context}: version must be {SCHEMA_VERSION}")


def _load_json_object(path: str | os.PathLike[str], context: str) -> dict[str, Any]:
    with Path(path).open(encoding="utf-8") as source:
        return _object(json.load(source), context)


def _exact_keys(
    data: dict[str, Any], required: set[str], optional: set[str], context: str
) -> None:
    missing = sorted(required - data.keys())
    unexpected = sorted(data.keys() - required - optional)
    if missing:
        raise ValueError(f"{context}: missing keys: {', '.join(missing)}")
    if unexpected:
        raise ValueError(f"{context}: unexpected keys: {', '.join(unexpected)}")


def _object(value: Any, context: str) -> dict[str, Any]:
    if type(value) is not dict:
        raise ValueError(f"{context} must be an object")
    return value


def _list(value: Any, context: str) -> list[Any]:
    if type(value) is not list:
        raise ValueError(f"{context} must be an array")
    return value


def _string(value: Any, context: str) -> str:
    if type(value) is not str:
        raise ValueError(f"{context} must be a string")
    return value


def _integer(value: Any, context: str) -> int:
    if type(value) is not int or value < 0:
        raise ValueError(f"{context} must be a non-negative integer")
    return value


def _number(value: Any, context: str) -> float:
    if type(value) not in {int, float}:
        raise ValueError(f"{context} must be a number")
    result = float(value)
    if not math.isfinite(result):
        raise ValueError(f"{context} must be finite")
    return result


def _validate_json_finite(value: Any, context: str) -> None:
    if type(value) is float and not math.isfinite(value):
        raise ValueError(f"{context} must contain only finite numbers")
    if type(value) is list:
        for index, item in enumerate(value):
            _validate_json_finite(item, f"{context}[{index}]")
    elif type(value) is dict:
        for key, item in value.items():
            _validate_json_finite(item, f"{context}.{key}")


def _string_list(value: Any, context: str) -> list[str]:
    return [_string(item, f"{context}[{index}]") for index, item in enumerate(_list(value, context))]


def _integer_list(value: Any, context: str) -> list[int]:
    return [_integer(item, f"{context}[{index}]") for index, item in enumerate(_list(value, context))]


def _number_list(value: Any, context: str) -> list[float]:
    return [_number(item, f"{context}[{index}]") for index, item in enumerate(_list(value, context))]


def _label_matrix(value: Any, n_roles: int, context: str) -> list[list[str]]:
    rows = _list(value, context)
    if len(rows) != n_roles:
        raise ValueError(f"{context} must have one row per role")
    result = []
    for role, row in enumerate(rows):
        labels = _string_list(row, f"{context}[{role}]")
        if any(not label for label in labels):
            raise ValueError(f"{context}[{role}] must not contain empty labels")
        result.append(labels)
    return result


__all__ = [
    "ContinuousSolution",
    "GameSpec",
    "InconclusiveSolution",
    "MetricAction",
    "NashSolution",
    "PopulationComponent",
    "PopulationSolution",
    "QreSolution",
    "Solution",
    "load_game_spec",
    "load_solutions",
]
