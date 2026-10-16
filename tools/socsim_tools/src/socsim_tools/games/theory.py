"""Numerical theory helpers for exported normal-form games."""

from __future__ import annotations

import itertools
import math
from collections.abc import Sequence

import numpy as np
from numpy.typing import ArrayLike, NDArray

from socsim_tools.games.spec import GameSpec


def pure_nash(spec: GameSpec) -> list[tuple[int, ...]]:
    """Return pure Nash profiles in row-major profile order.

    Payoff comparisons use the same payoff-scaled tolerance as the Rust
    finite-game solvers.
    """

    payoffs, shape = _finite_payoff_tensors(spec)
    tolerance = _payoff_tolerance(spec)
    equilibria = []
    for profile in itertools.product(*(range(count) for count in shape)):
        is_equilibrium = True
        for role, count in enumerate(shape):
            current = payoffs[role][profile]
            for action in range(count):
                deviation = list(profile)
                deviation[role] = action
                if payoffs[role][tuple(deviation)] - current > tolerance:
                    is_equilibrium = False
                    break
            if not is_equilibrium:
                break
        if is_equilibrium:
            equilibria.append(profile)
    return equilibria


def best_responses_2x2(
    spec: GameSpec,
) -> tuple[NDArray[np.bool_], NDArray[np.bool_]]:
    """Return role-0 and role-1 best-response masks for a 2-by-2 game.

    Ties within the Rust solver tolerance are marked as best responses for
    every tied action.
    """

    payoffs, shape = _finite_payoff_tensors(spec)
    if shape != [2, 2] or spec.n_roles != 2:
        raise ValueError("best_responses_2x2 requires a finite 2-by-2 game")
    tolerance = _payoff_tolerance(spec)
    role_zero = payoffs[0] >= np.max(payoffs[0], axis=0) - tolerance
    role_one = payoffs[1] >= np.max(payoffs[1], axis=1)[:, np.newaxis] - tolerance
    return role_zero, role_one


def replicator_field(spec: GameSpec, points: ArrayLike) -> NDArray[np.float64]:
    """Evaluate the replicator equation at one point or an array of points.

    The payoff matrix is the role-0 matrix ``A`` and the returned derivative
    is ``x_i * ((A x)_i - x.T @ A @ x)``.
    """

    payoffs, shape = _finite_payoff_tensors(spec)
    if spec.n_roles != 2 or len(shape) != 2 or shape[0] != shape[1]:
        raise ValueError(
            "replicator_field requires a two-role game with one common action count"
        )
    values = np.asarray(points, dtype=float)
    single = values.ndim == 1
    if single:
        values = values[np.newaxis, :]
    if values.ndim != 2 or values.shape[1] != shape[0]:
        raise ValueError(
            f"points must have shape (n, {shape[0]}) or ({shape[0]},)"
        )
    if not np.all(np.isfinite(values)):
        raise ValueError("points must contain only finite values")
    fitness = values @ payoffs[0].T
    mean_fitness = np.sum(values * fitness, axis=1, keepdims=True)
    derivative = values * (fitness - mean_fitness)
    return derivative[0] if single else derivative


def to_barycentric_xy(points: ArrayLike) -> NDArray[np.float64]:
    """Map simplex coordinates to an equilateral triangle.

    Vertex 0 is the lower left, vertex 1 the lower right, and vertex 2 the
    upper vertex.
    """

    values = np.asarray(points, dtype=float)
    single = values.ndim == 1
    if single:
        values = values[np.newaxis, :]
    if values.ndim != 2 or values.shape[1] != 3:
        raise ValueError("barycentric points must have shape (n, 3) or (3,)")
    if not np.all(np.isfinite(values)):
        raise ValueError("barycentric points must contain only finite values")
    vertices = np.array(
        [[0.0, 0.0], [1.0, 0.0], [0.5, math.sqrt(3.0) / 2.0]],
        dtype=float,
    )
    mapped = values @ vertices
    return mapped[0] if single else mapped


def _finite_payoff_tensors(
    spec: GameSpec,
) -> tuple[list[NDArray[np.float64]], list[int]]:
    if spec.kind != "finite" or spec.payoffs is None or spec.shape is None:
        raise ValueError("this operation requires a finite game specification")
    shape = list(spec.shape)
    expected = math.prod(shape)
    if len(spec.payoffs) != spec.n_roles or any(
        len(values) != expected for values in spec.payoffs
    ):
        raise ValueError("the payoff tensor shape is inconsistent with the game")
    return [np.asarray(values, dtype=float).reshape(shape) for values in spec.payoffs], shape


def _payoff_tolerance(spec: GameSpec) -> float:
    if spec.payoffs is None:
        raise ValueError("payoff tolerance requires a finite game specification")
    values: Sequence[float] = [value for row in spec.payoffs for value in row]
    if not values:
        raise ValueError("payoff tolerance requires at least one payoff")
    minimum = min(values)
    maximum = max(values)
    payoff_range = maximum - minimum
    if math.isfinite(payoff_range):
        return 1e-10 * max(1.0, payoff_range)
    return max(maximum * 1e-10 - minimum * 1e-10, 1e-10)


__all__ = [
    "best_responses_2x2",
    "pure_nash",
    "replicator_field",
    "to_barycentric_xy",
]

