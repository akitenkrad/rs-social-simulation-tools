"""Replicator vector fields, fixed points, and trajectories on a simplex."""

from __future__ import annotations

from collections.abc import Sequence
import math
import warnings

import matplotlib as mpl
from matplotlib.figure import Figure
from matplotlib.patches import Polygon
import numpy as np
from numpy.typing import ArrayLike, NDArray

from socsim_tools.games.spec import (
    GameSpec,
    InconclusiveSolution,
    NashSolution,
    PopulationComponent,
    PopulationSolution,
    Solution,
)
from socsim_tools.games.style import CONTINUOUS_CMAP, style_context
from socsim_tools.games.theory import replicator_field, to_barycentric_xy


_SQRT_THREE = math.sqrt(3.0)
_VERTICES = np.array([[0.0, 0.0], [1.0, 0.0], [0.5, _SQRT_THREE / 2.0]])


def plot_simplex(
    spec: GameSpec,
    solutions: Sequence[Solution] | None = None,
    trajectories: Sequence[ArrayLike] | ArrayLike | None = None,
    *,
    ax=None,
    n_grid: int = 15,
    normalize: bool = True,
    color_by_speed: bool = True,
    streamlines: bool = False,
    orbits: Sequence[ArrayLike] | None = None,
    orbit_t_max: float = 50.0,
    labels: Sequence[str] | None = None,
    font: str | None = None,
) -> Figure:
    """Plot a three-action symmetric game's replicator dynamics.

    ``trajectories`` accepts one or more arrays of simplex points. ``orbits``
    accepts initial points and integrates deterministic display trajectories
    with fourth-order Runge--Kutta. The returned figure is not saved.

    Raises:
        ValueError: If the game is not two-role, symmetric, and three-action,
            or if a point is outside the probability simplex.
    """

    payoff_matrix = _validate_simplex_game(spec)
    if type(n_grid) is not int or n_grid < 1:
        raise ValueError("n_grid must be a positive integer")
    vertex_labels = list(labels) if labels is not None else list(spec.action_labels[0])
    if len(vertex_labels) != 3:
        raise ValueError("labels must contain exactly three entries")
    all_labels = [*spec.role_labels, *vertex_labels]
    all_labels.extend(solution.id for solution in solutions or [])
    _warn_non_ascii(all_labels, font)

    if ax is None:
        with style_context(font=font):
            figure = Figure(figsize=tuple(mpl.rcParams["figure.figsize"]))
            axis = figure.subplots()
            _draw_simplex(
                axis,
                spec,
                payoff_matrix,
                solutions,
                trajectories,
                n_grid,
                normalize,
                color_by_speed,
                streamlines,
                orbits,
                orbit_t_max,
                vertex_labels,
                font,
            )
            return figure
    _draw_simplex(
        ax,
        spec,
        payoff_matrix,
        solutions,
        trajectories,
        n_grid,
        normalize,
        color_by_speed,
        streamlines,
        orbits,
        orbit_t_max,
        vertex_labels,
        font,
    )
    return ax.figure


def _draw_simplex(
    ax,
    spec: GameSpec,
    payoff_matrix: NDArray[np.float64],
    solutions: Sequence[Solution] | None,
    trajectories: Sequence[ArrayLike] | ArrayLike | None,
    n_grid: int,
    normalize: bool,
    color_by_speed: bool,
    streamlines: bool,
    orbits: Sequence[ArrayLike] | None,
    orbit_t_max: float,
    labels: list[str],
    font: str | None,
) -> None:
    del payoff_matrix
    text_options = {} if font is None else {"fontfamily": font}
    border = Polygon(_VERTICES, closed=True, fill=False, edgecolor="black", linewidth=1.5)
    ax.add_patch(border)

    if streamlines:
        _draw_streamlines(ax, spec, n_grid, color_by_speed)
    else:
        _draw_quiver(ax, spec, n_grid, normalize, color_by_speed)
    _draw_fixed_points(ax, solutions)
    _draw_trajectories(ax, trajectories)
    _draw_orbits(ax, spec, orbits, orbit_t_max)

    offsets = [(-0.04, -0.045, "right"), (0.04, -0.045, "left"), (0.0, 0.045, "center")]
    for (x, y), label, (dx, dy, align) in zip(_VERTICES, labels, offsets):
        ax.text(x + dx, y + dy, label, ha=align, va="center", **text_options)
    ax.set_xlim(-0.14, 1.14)
    ax.set_ylim(-0.12, _SQRT_THREE / 2.0 + 0.13)
    ax.set_aspect("equal", adjustable="box")
    ax.axis("off")
    ax.set_title(f"Replicator dynamics: {spec.name.replace('_', ' ').title()}", **text_options)


def _draw_quiver(
    ax, spec: GameSpec, n_grid: int, normalize: bool, color_by_speed: bool
) -> None:
    points = np.array(
        [
            [i / n_grid, j / n_grid, (n_grid - i - j) / n_grid]
            for i in range(n_grid + 1)
            for j in range(n_grid - i + 1)
        ],
        dtype=float,
    )
    derivative = replicator_field(spec, points)
    xy = to_barycentric_xy(points)
    delta = to_barycentric_xy(derivative)
    speed = np.linalg.norm(derivative, axis=1)
    if normalize:
        nonzero = speed > 1e-12
        delta = np.divide(
            delta,
            np.linalg.norm(delta, axis=1, keepdims=True),
            out=np.zeros_like(delta),
            where=nonzero[:, np.newaxis],
        )
    arguments = [xy[:, 0], xy[:, 1], delta[:, 0], delta[:, 1]]
    options = {
        "angles": "xy",
        "scale_units": "xy",
        "scale": 18.0 if normalize else 1.0,
        "width": 0.004,
        "pivot": "mid",
        "zorder": 1,
    }
    if color_by_speed:
        arguments.append(speed)
        options["cmap"] = CONTINUOUS_CMAP
    else:
        options["color"] = "#444444"
    quiver = ax.quiver(*arguments, **options)
    quiver.set_gid("vector-field")
    if color_by_speed:
        ax.figure.colorbar(quiver, ax=ax, label="Speed", fraction=0.045, pad=0.05)


def _draw_streamlines(
    ax, spec: GameSpec, n_grid: int, color_by_speed: bool
) -> None:
    resolution = max(25, n_grid * 3)
    xs = np.linspace(0.0, 1.0, resolution)
    ys = np.linspace(0.0, _SQRT_THREE / 2.0, resolution)
    x_grid, y_grid = np.meshgrid(xs, ys)
    x_two = 2.0 * y_grid / _SQRT_THREE
    x_one = x_grid - y_grid / _SQRT_THREE
    x_zero = 1.0 - x_one - x_two
    inside = (x_zero >= 0.0) & (x_one >= 0.0) & (x_two >= 0.0)
    points = np.column_stack([x_zero.ravel(), x_one.ravel(), x_two.ravel()])
    derivative = replicator_field(spec, points)
    delta = to_barycentric_xy(derivative)
    u = np.ma.array(delta[:, 0].reshape(x_grid.shape), mask=~inside)
    v = np.ma.array(delta[:, 1].reshape(y_grid.shape), mask=~inside)
    speed = np.linalg.norm(derivative, axis=1).reshape(x_grid.shape)
    color = np.ma.array(speed, mask=~inside) if color_by_speed else "#444444"
    stream = ax.streamplot(
        xs,
        ys,
        u,
        v,
        color=color,
        cmap=CONTINUOUS_CMAP if color_by_speed else None,
        density=1.0,
        linewidth=1.2,
        arrowsize=0.8,
    )
    stream.lines.set_gid("streamlines")
    stream.arrows.set_gid("streamline-arrows")
    if color_by_speed:
        maximum = float(np.max(speed[inside])) if np.any(inside) else 1.0
        norm = mpl.colors.Normalize(vmin=0.0, vmax=maximum or 1.0)
        scalar = mpl.cm.ScalarMappable(norm=norm, cmap=CONTINUOUS_CMAP)
        ax.figure.colorbar(scalar, ax=ax, label="Speed", fraction=0.045, pad=0.05)


def _draw_fixed_points(ax, solutions: Sequence[Solution] | None) -> None:
    for solution in solutions or []:
        if isinstance(solution, InconclusiveSolution):
            continue
        if isinstance(solution, PopulationSolution):
            point = to_barycentric_xy(solution.shares)
            if solution.ess == "true":
                marker, face, label = "o", "black", None
            elif solution.ess == "false":
                marker, face, label = "o", "white", None
            else:
                marker, face, label = "s", "white", "?"
            artist = ax.plot(
                [point[0]],
                [point[1]],
                marker=marker,
                markersize=9,
                markerfacecolor=face,
                markeredgecolor="black",
                linestyle="none",
                zorder=5,
            )[0]
            artist.set_gid(f"fixed/{solution.id}")
            if label is not None:
                ax.text(point[0], point[1], label, ha="center", va="center", fontsize="small", zorder=6)
        elif isinstance(solution, PopulationComponent):
            if len(solution.directions) >= 2:
                warnings.warn(
                    f"population component {solution.id!r} has two or more dimensions and will not be drawn",
                    UserWarning,
                    stacklevel=3,
                )
                continue
            if len(solution.directions) == 1:
                endpoints = _component_endpoints(solution.base, solution.directions[0])
                if endpoints is not None:
                    xy = to_barycentric_xy(endpoints)
                    line = ax.plot(xy[:, 0], xy[:, 1], color="#777777", linewidth=4.0, zorder=3)[0]
                    line.set_gid(f"fixed/{solution.id}")
        elif isinstance(solution, NashSolution) and _is_symmetric_nash(solution):
            point = to_barycentric_xy(solution.strategies[0])
            artist = ax.plot(
                [point[0]],
                [point[1]],
                marker="D",
                markersize=8,
                markerfacecolor="white",
                markeredgecolor="black",
                linestyle="none",
                zorder=4,
            )[0]
            artist.set_gid(f"fixed/{solution.id}")


def _draw_trajectories(
    ax, trajectories: Sequence[ArrayLike] | ArrayLike | None
) -> None:
    if trajectories is None:
        return
    arrays = _trajectory_arrays(trajectories)
    for index, values in enumerate(arrays):
        points = _probability_points(values, f"trajectories[{index}]")
        xy = to_barycentric_xy(points)
        line = ax.plot(xy[:, 0], xy[:, 1], color="black", linewidth=1.0, alpha=0.6, zorder=3)[0]
        line.set_gid(f"traj/{index}")
        ax.plot([xy[0, 0]], [xy[0, 1]], marker="o", markerfacecolor="white", markeredgecolor="black", linestyle="none", zorder=4)
        ax.plot([xy[-1, 0]], [xy[-1, 1]], marker="s", markerfacecolor="black", markeredgecolor="black", linestyle="none", zorder=4)


def _draw_orbits(
    ax,
    spec: GameSpec,
    orbits: Sequence[ArrayLike] | None,
    orbit_t_max: float,
) -> None:
    if orbits is None:
        return
    if not math.isfinite(orbit_t_max) or orbit_t_max <= 0.0:
        raise ValueError("orbit_t_max must be finite and positive")
    step = min(0.05, orbit_t_max / 10.0)
    count = max(1, int(math.ceil(orbit_t_max / step)))
    step = orbit_t_max / count
    for index, initial in enumerate(orbits):
        point = _probability_points(np.asarray(initial, dtype=float), f"orbits[{index}]")[0]
        values = [point.copy()]
        for _ in range(count):
            k1 = replicator_field(spec, point)
            k2 = replicator_field(spec, point + 0.5 * step * k1)
            k3 = replicator_field(spec, point + 0.5 * step * k2)
            k4 = replicator_field(spec, point + step * k3)
            point = point + step * (k1 + 2 * k2 + 2 * k3 + k4) / 6.0
            point = np.clip(point, 0.0, None)
            point /= np.sum(point)
            values.append(point.copy())
        xy = to_barycentric_xy(np.asarray(values))
        line = ax.plot(xy[:, 0], xy[:, 1], color="#777777", linewidth=1.2, zorder=2)[0]
        line.set_gid(f"traj/orbit-{index}")


def _validate_simplex_game(spec: GameSpec) -> NDArray[np.float64]:
    if (
        spec.kind != "finite"
        or spec.n_roles != 2
        or spec.shape != [3, 3]
        or spec.payoffs is None
        or spec.action_labels is None
    ):
        raise ValueError(
            "plot_simplex requires a finite two-role game with three actions per role"
        )
    if spec.symmetric is not True:
        raise ValueError("plot_simplex requires a symmetric game")
    first = np.asarray(spec.payoffs[0], dtype=float).reshape(3, 3)
    second = np.asarray(spec.payoffs[1], dtype=float).reshape(3, 3)
    values = np.concatenate([first.ravel(), second.ravel()])
    payoff_range = float(np.max(values) - np.min(values))
    tolerance = 1e-10 * max(1.0, payoff_range)
    if not np.allclose(second, first.T, rtol=0.0, atol=tolerance):
        raise ValueError("plot_simplex requires a symmetric game with B = A.T")
    return first


def _trajectory_arrays(
    trajectories: Sequence[ArrayLike] | ArrayLike,
) -> list[NDArray[np.float64]]:
    try:
        values = np.asarray(trajectories, dtype=float)
    except ValueError:
        return [np.asarray(item, dtype=float) for item in trajectories]
    if values.ndim == 2 and values.shape[1] == 3:
        return [values]
    if values.ndim == 3 and values.shape[2] == 3:
        return [values[index] for index in range(values.shape[0])]
    raise ValueError("trajectories must contain arrays with shape (n, 3)")


def _probability_points(values: NDArray[np.float64], context: str) -> NDArray[np.float64]:
    if values.ndim == 1:
        values = values[np.newaxis, :]
    if values.ndim != 2 or values.shape[1] != 3 or values.shape[0] == 0:
        raise ValueError(f"{context} must have shape (n, 3)")
    if not np.all(np.isfinite(values)):
        raise ValueError(f"{context} must contain only finite values")
    if np.any(values < -1e-12):
        raise ValueError(f"{context} must contain non-negative probabilities")
    if np.any(np.abs(np.sum(values, axis=1) - 1.0) > 1e-9):
        raise ValueError(f"{context} points must sum to 1 within 1e-9")
    return values


def _component_endpoints(
    base: Sequence[float], direction: Sequence[float]
) -> NDArray[np.float64] | None:
    base_array = np.asarray(base, dtype=float)
    direction_array = np.asarray(direction, dtype=float)
    if base_array.shape != (3,) or direction_array.shape != (3,):
        return None
    lower, upper = -math.inf, math.inf
    for value, delta in zip(base_array, direction_array):
        if delta > 1e-15:
            lower = max(lower, -value / delta)
        elif delta < -1e-15:
            upper = min(upper, -value / delta)
    if not math.isfinite(lower) or not math.isfinite(upper) or lower > upper:
        return None
    endpoints = np.vstack(
        [base_array + lower * direction_array, base_array + upper * direction_array]
    )
    endpoints[np.abs(endpoints) < 1e-14] = 0.0
    endpoints = np.clip(endpoints, 0.0, None)
    endpoints /= np.sum(endpoints, axis=1, keepdims=True)
    return endpoints


def _is_symmetric_nash(solution: NashSolution) -> bool:
    first = np.asarray(solution.strategies[0], dtype=float)
    return all(
        np.allclose(first, strategy, rtol=0.0, atol=1e-9)
        for strategy in solution.strategies[1:]
    )


def _warn_non_ascii(labels: Sequence[str], font: str | None) -> None:
    if font is None and any(not label.isascii() for label in labels):
        warnings.warn(
            "non-ASCII plot labels may require a font; pass font=... to the "
            "plotting function or style_context",
            UserWarning,
            stacklevel=3,
        )


__all__ = ["plot_simplex"]
