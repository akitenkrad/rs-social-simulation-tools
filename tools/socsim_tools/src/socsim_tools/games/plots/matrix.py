"""Payoff-matrix and best-response-arrow plots for finite games."""

from __future__ import annotations

from collections.abc import Sequence
import math
import warnings

import matplotlib as mpl
from matplotlib.figure import Figure
from matplotlib.patches import FancyArrowPatch, Rectangle

from socsim_tools.games.spec import GameSpec, NashSolution, Solution
from socsim_tools.games.style import ROLE_COLORS, style_context
from socsim_tools.games.theory import best_responses_2x2, pure_nash


def plot_payoff_matrix(
    spec: GameSpec,
    solutions: Sequence[Solution] | None = None,
    *,
    ax=None,
    fmt: str = "{:.3g}",
    underline_best_responses: bool = True,
    highlight_nash: bool = True,
    show_mixed: bool | None = None,
    max_actions: int = 10,
    font: str | None = None,
) -> Figure:
    """Plot a two-role finite game as a bimatrix.

    Role-0 and role-1 payoffs use distinct colors and positions. Best-response
    underlines and pure-Nash boxes carry stable ``gid`` values for inspection.
    The returned figure is not saved.

    Raises:
        ValueError: If ``spec`` is not a two-role finite game or an action
            count exceeds ``max_actions``.
    """

    rows, columns, payoffs = _bimatrix(spec)
    if type(max_actions) is not int or max_actions < 1:
        raise ValueError("max_actions must be a positive integer")
    if rows > max_actions or columns > max_actions:
        raise ValueError(
            f"game action count exceeds max_actions={max_actions}"
        )
    _warn_non_ascii(_plot_labels(spec, solutions), font)

    if ax is None:
        with style_context(font=font):
            figure = Figure(figsize=tuple(mpl.rcParams["figure.figsize"]))
            axis = figure.subplots()
            _draw_payoff_matrix(
                axis,
                spec,
                payoffs,
                solutions,
                fmt,
                underline_best_responses,
                highlight_nash,
                show_mixed,
                font,
            )
            return figure
    _draw_payoff_matrix(
        ax,
        spec,
        payoffs,
        solutions,
        fmt,
        underline_best_responses,
        highlight_nash,
        show_mixed,
        font,
    )
    return ax.figure


def plot_best_response_2x2(
    spec: GameSpec,
    *,
    ax=None,
    show_payoffs: bool = False,
    fmt: str = "{:.3g}",
    font: str | None = None,
) -> Figure:
    """Plot the directed unilateral-deviation graph of a 2-by-2 game.

    Vertical edges are role-0 deviations and horizontal edges are role-1
    deviations. Ties are drawn as dashed lines without arrowheads. The
    returned figure is not saved.
    """

    rows, columns, payoffs = _bimatrix(spec)
    if (rows, columns) != (2, 2):
        raise ValueError("plot_best_response_2x2 requires a finite 2-by-2 game")
    _warn_non_ascii(_plot_labels(spec, None), font)
    if ax is None:
        with style_context(font=font):
            figure = Figure(figsize=tuple(mpl.rcParams["figure.figsize"]))
            axis = figure.subplots()
            _draw_best_response_arrows(
                axis, spec, payoffs, show_payoffs, fmt, font
            )
            return figure
    _draw_best_response_arrows(ax, spec, payoffs, show_payoffs, fmt, font)
    return ax.figure


def _draw_payoff_matrix(
    ax,
    spec: GameSpec,
    payoffs: list[list[list[float]]],
    solutions: Sequence[Solution] | None,
    fmt: str,
    underline: bool,
    highlight: bool,
    show_mixed: bool | None,
    font: str | None,
) -> None:
    assert spec.shape is not None and spec.action_labels is not None
    rows, columns = spec.shape
    text_options = _font_options(font)
    row_best, column_best = _best_response_masks(spec, rows, columns)
    equilibria = set(pure_nash(spec))
    _warn_if_pure_solutions_disagree(spec, solutions, equilibria)

    for i in range(rows):
        for j in range(columns):
            ax.add_patch(
                Rectangle(
                    (j, i),
                    1,
                    1,
                    facecolor="white",
                    edgecolor="#777777",
                    linewidth=1.0,
                )
            )
            ax.text(
                j + 0.47,
                i + 0.52,
                fmt.format(payoffs[0][i][j]),
                color=ROLE_COLORS[0],
                ha="right",
                va="center",
                **text_options,
            )
            ax.text(j + 0.50, i + 0.52, ",", ha="center", va="center", **text_options)
            ax.text(
                j + 0.53,
                i + 0.52,
                fmt.format(payoffs[1][i][j]),
                color=ROLE_COLORS[1],
                ha="left",
                va="center",
                **text_options,
            )
            if underline and row_best[i][j]:
                line = ax.plot(
                    [j + 0.18, j + 0.46],
                    [i + 0.68, i + 0.68],
                    color=ROLE_COLORS[0],
                    linewidth=1.5,
                )[0]
                line.set_gid(f"br/0/{i}/{j}")
            if underline and column_best[i][j]:
                line = ax.plot(
                    [j + 0.54, j + 0.82],
                    [i + 0.68, i + 0.68],
                    color=ROLE_COLORS[1],
                    linewidth=1.5,
                )[0]
                line.set_gid(f"br/1/{i}/{j}")
            if highlight and (i, j) in equilibria:
                box = Rectangle(
                    (j + 0.025, i + 0.025),
                    0.95,
                    0.95,
                    fill=False,
                    edgecolor="black",
                    linewidth=3.0,
                )
                box.set_gid(f"ne/{i}/{j}")
                ax.add_patch(box)
                ax.text(
                    j + 0.92,
                    i + 0.13,
                    "NE",
                    ha="right",
                    va="top",
                    fontsize="small",
                    fontweight="bold",
                    **text_options,
                )

    for j, label in enumerate(spec.action_labels[1]):
        ax.text(j + 0.5, -0.18, label, ha="center", va="bottom", **text_options)
    for i, label in enumerate(spec.action_labels[0]):
        ax.text(-0.12, i + 0.5, label, ha="right", va="center", **text_options)
    ax.text(
        columns / 2,
        -0.55,
        spec.role_labels[1],
        ha="center",
        va="bottom",
        color=ROLE_COLORS[1],
        **text_options,
    )
    ax.text(
        -0.62,
        rows / 2,
        spec.role_labels[0],
        ha="center",
        va="center",
        rotation=90,
        color=ROLE_COLORS[0],
        **text_options,
    )

    mixed = [
        solution
        for solution in solutions or []
        if isinstance(solution, NashSolution) and not solution.pure
    ]
    should_show_mixed = solutions is not None if show_mixed is None else show_mixed
    annotation_lines = (
        _mixed_nash_lines(spec, mixed) if should_show_mixed else []
    )
    if annotation_lines:
        ax.text(
            0,
            rows + 0.35,
            "\n".join(annotation_lines),
            ha="left",
            va="top",
            fontsize="small",
            linespacing=1.05,
            **text_options,
        )

    lower = rows + 0.65
    if annotation_lines:
        lower += 0.4 * (len(annotation_lines) - 1)
    ax.set_xlim(-0.8, columns + 0.15)
    ax.set_ylim(lower, -0.7)
    ax.set_aspect("equal", adjustable="box")
    ax.axis("off")
    ax.set_title(spec.name.replace("_", " ").title(), **text_options)


def _mixed_nash_lines(
    spec: GameSpec, solutions: Sequence[NashSolution]
) -> list[str]:
    assert spec.action_labels is not None
    lines: list[str] = []
    for solution_index, solution in enumerate(solutions):
        if solution_index:
            lines.append("")
        lines.append(f"Mixed NE ({solution.id})")
        for role, strategy in enumerate(solution.strategies):
            entries = [
                f"{label} {probability:.3f}"
                for label, probability in zip(
                    spec.action_labels[role], strategy
                )
            ]
            chunks = [
                entries[index : index + 3]
                for index in range(0, len(entries), 3)
            ]
            for chunk_index, chunk in enumerate(chunks):
                prefix = f"{spec.role_labels[role]} (" if chunk_index == 0 else "  "
                suffix = ")" if chunk_index == len(chunks) - 1 else ","
                lines.append(prefix + ", ".join(chunk) + suffix)
    return lines


def _draw_best_response_arrows(
    ax,
    spec: GameSpec,
    payoffs: list[list[list[float]]],
    show_payoffs: bool,
    fmt: str,
    font: str | None,
) -> None:
    assert spec.action_labels is not None
    tolerance = _payoff_tolerance(spec)
    text_options = _font_options(font)
    positions = {(i, j): (float(j), float(1 - i)) for i in range(2) for j in range(2)}
    outgoing: set[tuple[int, int]] = set()

    edges = []
    for j in range(2):
        edges.append((0, (0, j), (1, j), payoffs[0][0][j], payoffs[0][1][j]))
    for i in range(2):
        edges.append((1, (i, 0), (i, 1), payoffs[1][i][0], payoffs[1][i][1]))

    for role, first, second, first_payoff, second_payoff in edges:
        difference = second_payoff - first_payoff
        if abs(difference) <= tolerance:
            line = ax.plot(
                [positions[first][0], positions[second][0]],
                [positions[first][1], positions[second][1]],
                linestyle="--",
                color=ROLE_COLORS[role],
                linewidth=1.5,
                zorder=1,
            )[0]
            line.set_gid(f"tie/{role}/{_profile_id(first)}/{_profile_id(second)}")
            continue
        source, target = (first, second) if difference > 0 else (second, first)
        outgoing.add(source)
        arrow = FancyArrowPatch(
            positions[source],
            positions[target],
            arrowstyle="-|>",
            mutation_scale=16,
            shrinkA=14,
            shrinkB=14,
            linewidth=1.8,
            color=ROLE_COLORS[role],
            zorder=2,
        )
        arrow.set_gid(
            f"arrow/{role}/{_profile_id(source)}/{_profile_id(target)}"
        )
        ax.add_patch(arrow)

    for profile, (x, y) in positions.items():
        sink = profile not in outgoing
        marker = ax.plot(
            [x],
            [y],
            marker="o",
            markersize=11,
            markerfacecolor="black" if sink else "white",
            markeredgecolor="black",
            linestyle="none",
            zorder=3,
        )[0]
        marker.set_gid(
            f"sink/{profile[0]}/{profile[1]}"
            if sink
            else f"profile/{profile[0]}/{profile[1]}"
        )
        labels = (
            spec.action_labels[0][profile[0]],
            spec.action_labels[1][profile[1]],
        )
        ax.text(
            x,
            y + (0.13 if profile[0] == 0 else -0.13),
            f"({labels[0]}, {labels[1]})",
            ha="center",
            va="bottom" if profile[0] == 0 else "top",
            **text_options,
        )
        if show_payoffs:
            ax.text(
                x,
                y + (0.25 if profile[0] == 0 else -0.25),
                f"({fmt.format(payoffs[0][profile[0]][profile[1]])}, "
                f"{fmt.format(payoffs[1][profile[0]][profile[1]])})",
                ha="center",
                va="bottom" if profile[0] == 0 else "top",
                fontsize="small",
                **text_options,
            )

    ax.text(-0.13, 0.5, spec.role_labels[0], rotation=90, ha="center", va="center", color=ROLE_COLORS[0], **text_options)
    ax.text(0.5, -0.14, spec.role_labels[1], ha="center", va="center", color=ROLE_COLORS[1], **text_options)
    ax.set_xlim(-0.35, 1.35)
    ax.set_ylim(-0.35, 1.35)
    ax.set_aspect("equal", adjustable="box")
    ax.axis("off")
    ax.set_title(f"Best responses: {spec.name.replace('_', ' ').title()}", **text_options)


def _bimatrix(spec: GameSpec) -> tuple[int, int, list[list[list[float]]]]:
    if (
        spec.kind != "finite"
        or spec.n_roles != 2
        or spec.shape is None
        or spec.payoffs is None
        or spec.action_labels is None
    ):
        raise ValueError("payoff-matrix plots require a finite two-role game")
    rows, columns = spec.shape
    if len(spec.shape) != 2:
        raise ValueError("payoff-matrix plots require a finite two-role game")
    expected = rows * columns
    if len(spec.payoffs) != 2 or any(len(row) != expected for row in spec.payoffs):
        raise ValueError("the payoff tensor shape is inconsistent with the game")
    payoffs = [
        [values[i * columns : (i + 1) * columns] for i in range(rows)]
        for values in spec.payoffs
    ]
    return rows, columns, payoffs


def _best_response_masks(
    spec: GameSpec, rows: int, columns: int
) -> tuple[list[list[bool]], list[list[bool]]]:
    if (rows, columns) == (2, 2):
        role_zero, role_one = best_responses_2x2(spec)
        return role_zero.tolist(), role_one.tolist()
    _, _, payoffs = _bimatrix(spec)
    tolerance = _payoff_tolerance(spec)
    role_zero = [[False] * columns for _ in range(rows)]
    role_one = [[False] * columns for _ in range(rows)]
    for j in range(columns):
        best = max(payoffs[0][i][j] for i in range(rows))
        for i in range(rows):
            role_zero[i][j] = payoffs[0][i][j] >= best - tolerance
    for i in range(rows):
        best = max(payoffs[1][i])
        for j in range(columns):
            role_one[i][j] = payoffs[1][i][j] >= best - tolerance
    return role_zero, role_one


def _warn_if_pure_solutions_disagree(
    spec: GameSpec,
    solutions: Sequence[Solution] | None,
    expected: set[tuple[int, ...]],
) -> None:
    if solutions is None:
        return
    exported = set()
    for solution in solutions:
        if not isinstance(solution, NashSolution) or not solution.pure:
            continue
        profile = tuple(strategy.index(1.0) for strategy in solution.strategies)
        exported.add(profile)
    if exported != expected:
        warnings.warn(
            "exported pure Nash solutions disagree with equilibria computed "
            f"from game {spec.name!r}",
            UserWarning,
            stacklevel=3,
        )


def _payoff_tolerance(spec: GameSpec) -> float:
    assert spec.payoffs is not None
    values = [value for role in spec.payoffs for value in role]
    minimum, maximum = min(values), max(values)
    payoff_range = maximum - minimum
    if math.isfinite(payoff_range):
        return 1e-10 * max(1.0, payoff_range)
    return max(maximum * 1e-10 - minimum * 1e-10, 1e-10)


def _plot_labels(
    spec: GameSpec, solutions: Sequence[Solution] | None
) -> list[str]:
    labels = [*spec.role_labels]
    for row in spec.action_labels or []:
        labels.extend(row)
    labels.extend(solution.id for solution in solutions or [])
    return labels


def _warn_non_ascii(labels: Sequence[str], font: str | None) -> None:
    if font is None and any(not label.isascii() for label in labels):
        warnings.warn(
            "non-ASCII plot labels may require a font; pass font=... to the "
            "plotting function or style_context",
            UserWarning,
            stacklevel=3,
        )


def _font_options(font: str | None) -> dict[str, str]:
    return {} if font is None else {"fontfamily": font}


def _profile_id(profile: tuple[int, int]) -> str:
    return f"{profile[0]},{profile[1]}"


__all__ = ["plot_best_response_2x2", "plot_payoff_matrix"]
