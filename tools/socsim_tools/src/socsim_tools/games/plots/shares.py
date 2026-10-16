"""Aggregate and plot recorded game metric shares across runs."""

from __future__ import annotations

from collections.abc import Sequence
import math
import warnings

import matplotlib as mpl
from matplotlib.figure import Figure
from matplotlib.lines import Line2D
import numpy as np

from socsim_tools.games.aggregate import Aggregated, aggregate_series
from socsim_tools.games.records import SeriesTable
from socsim_tools.games.spec import GameSpec, Solution
from socsim_tools.games.style import OKABE_ITO, _action_colors, style_context
from socsim_tools.games.theory import TheoryLine, theory_lines


_LINESTYLES = ("--", ":", "-.", (0, (5, 2, 1, 2)))
_PROPORTION_FAMILIES = {
    "game.played_share.",
    "game.population_share.",
    "game.coop_rate",
}
_PROPORTION_MARGIN = 0.03


def plot_share_series(
    table: SeriesTable,
    spec: GameSpec | None = None,
    solutions: Sequence[Solution] | None = None,
    *,
    family: str = "game.played_share.",
    stat: str = "mean",
    ci: str = "t",
    level: float = 0.95,
    align: str = "intersection",
    show_replicates: bool = False,
    max_points: int = 2000,
    rolling: int | None = None,
    theory: str | Sequence[str] = "all",
    labels: str | Sequence[str] = "all",
    log_x: bool = False,
    include_unconverged: bool = False,
    ax=None,
    palette: Sequence[str] | None = None,
    font: str | None = None,
) -> Figure:
    """Plot aggregate metric series with intervals and theoretical values.

    ``include_unconverged`` is an explicit opt-in for non-converged continuous
    solutions. Such lines are annotated with ``not converged``. The returned
    figure is not saved.
    """

    if type(max_points) is not int or max_points < 1:
        raise ValueError("max_points must be a positive integer")
    if rolling is not None and (type(rolling) is not int or rolling < 1):
        raise ValueError("rolling must be a positive integer or None")
    if not isinstance(family, str) or not family:
        raise ValueError("family must be a non-empty string")
    resolved_spec = spec if spec is not None else table.spec
    if spec is not None and table.spec is not None and spec != table.spec:
        raise ValueError("game specification does not match the metric sources")
    selected_labels = _selected_labels(labels, table)
    label_text = [table.labels[name] for name in selected_labels]
    if resolved_spec is not None:
        label_text.extend(resolved_spec.role_labels)
    _warn_non_ascii(label_text, font)

    aggregated = aggregate_series(table, stat=stat, ci=ci, level=level, align=align)
    selected_theory = _select_solutions(solutions or [], theory)
    mapped_theory = (
        theory_lines(
            resolved_spec,
            selected_theory,
            family,
            include_unconverged=include_unconverged,
        )
        if resolved_spec is not None and selected_theory
        else []
    )

    if ax is None:
        with style_context(font=font):
            figure = Figure(figsize=tuple(mpl.rcParams["figure.figsize"]))
            if align == "available":
                axes = figure.subplots(
                    2,
                    1,
                    sharex=True,
                    gridspec_kw={"height_ratios": (4, 1), "hspace": 0.08},
                )
                axis, count_axis = axes
            else:
                axis = figure.subplots()
                count_axis = None
            _draw_share_series(
                axis,
                count_axis,
                table,
                aggregated,
                resolved_spec,
                mapped_theory,
                selected_labels,
                family,
                stat,
                ci,
                level,
                align,
                show_replicates,
                max_points,
                rolling,
                log_x,
                palette,
                font,
            )
            return figure
    count_axis = (
        ax.inset_axes([0.0, -0.32, 1.0, 0.18]) if align == "available" else None
    )
    _draw_share_series(
        ax,
        count_axis,
        table,
        aggregated,
        resolved_spec,
        mapped_theory,
        selected_labels,
        family,
        stat,
        ci,
        level,
        align,
        show_replicates,
        max_points,
        rolling,
        log_x,
        palette,
        font,
    )
    return ax.figure


def _draw_share_series(
    ax,
    count_ax,
    table: SeriesTable,
    aggregated: Aggregated,
    spec: GameSpec | None,
    mapped_theory: list[TheoryLine],
    selected_labels: list[str],
    family: str,
    stat: str,
    ci: str,
    level: float,
    align: str,
    show_replicates: bool,
    max_points: int,
    rolling: int | None,
    log_x: bool,
    palette: Sequence[str] | None,
    font: str | None,
) -> None:
    text_options = {} if font is None else {"fontfamily": font}
    colors = _metric_colors(spec, aggregated, palette)
    if log_x and any(value <= 0 for value in aggregated.t):
        raise ValueError("log_x requires positive time values")

    if show_replicates:
        for series in table.series:
            indices = _downsample_indices(len(series.t), max_points)
            for name in selected_labels:
                ax.plot(
                    np.asarray(series.t)[indices],
                    np.asarray(series.columns[name], dtype=float)[indices],
                    color=colors[name],
                    linewidth=0.7,
                    alpha=0.2,
                    zorder=1,
                )

    x = np.asarray(aggregated.t, dtype=float)
    indices = _downsample_indices(len(x), max_points)
    x_display = x[indices]
    direct_labels: list[tuple[float, str, str]] = []
    for name in selected_labels:
        center = _rolling(np.asarray(aggregated.center[name], dtype=float), rolling)
        line = ax.plot(
            x_display,
            center[indices],
            color=colors[name],
            label=aggregated.labels[name],
            zorder=3,
        )[0]
        metric_id = _metric_id(name, family)
        line.set_gid(f"series/{metric_id}")
        direct_labels.append((float(center[-1]), aggregated.labels[name], colors[name]))
        if aggregated.lower is not None and aggregated.upper is not None:
            lower = _optional_array(aggregated.lower[name])
            upper = _optional_array(aggregated.upper[name])
            lower = _rolling(lower, rolling)
            upper = _rolling(upper, rolling)
            band = ax.fill_between(
                x_display,
                lower[indices],
                upper[indices],
                color=colors[name],
                alpha=0.18,
                linewidth=0.0,
                zorder=2,
            )
            band.set_gid(f"band/{metric_id}")

    theory_annotations: list[tuple[float, str, str]] = []
    for theory_line in mapped_theory:
        name = _metric_name(family, theory_line.metric_id)
        if name not in selected_labels:
            continue
        label = theory_line.solution_id
        if not theory_line.converged:
            label += " (not converged)"
        artist = ax.axhline(
            theory_line.value,
            color=colors[name],
            linestyle=_LINESTYLES[theory_line.linestyle_index % len(_LINESTYLES)],
            linewidth=1.0,
            alpha=0.9,
            zorder=2,
        )
        artist.set_gid(f"theory/{theory_line.solution_id}/{theory_line.metric_id}")
        theory_annotations.append((theory_line.value, label, colors[name]))

    left, right = float(x[0]), float(x[-1])
    if log_x:
        ax.set_xscale("log")
        factor = (right / left) ** 0.08 if right > left else 1.3
        label_x = right * factor
        theory_x = right * factor**2
        ax.set_xlim(left, right * factor**4)
    else:
        span = max(right - left, 1.0)
        label_x = right + span * 0.025
        theory_x = right + span * 0.13
        ax.set_xlim(left, right + span * 0.34)
    if family in _PROPORTION_FAMILIES:
        label_lower, label_upper = 0.02, 0.98
    else:
        values = [value for value, _, _ in direct_labels]
        label_lower, label_upper = min(values), max(values)
        padding = max(label_upper - label_lower, 1.0) * 0.05
        label_lower -= padding
        label_upper += padding
    label_positions = _spread_label_positions(
        [value for value, _, _ in direct_labels],
        label_lower,
        label_upper,
    )
    for (value, label, color), label_y in zip(direct_labels, label_positions):
        ax.plot(
            [right, label_x],
            [value, label_y],
            color=color,
            linewidth=0.7,
            zorder=3,
        )
        ax.text(
            label_x,
            label_y,
            label,
            color=color,
            va="center",
            fontsize="small",
            **text_options,
        )
    grouped_theory: dict[tuple[float, str], list[str]] = {}
    for value, label, color in theory_annotations:
        grouped_theory.setdefault((value, color), []).append(label)
    for (value, color), grouped_labels in grouped_theory.items():
        if family in _PROPORTION_FAMILIES and value <= 0.05:
            vertical_alignment = "bottom"
            offset = 2.0
        elif family in _PROPORTION_FAMILIES and value >= 0.95:
            vertical_alignment = "top"
            offset = -2.0
        else:
            vertical_alignment = "center"
            offset = 0.0
        ax.annotate(
            " / ".join(grouped_labels),
            xy=(theory_x, value),
            xytext=(0.0, offset),
            textcoords="offset points",
            color=color,
            va=vertical_alignment,
            fontsize="x-small",
            **text_options,
        )

    if family in _PROPORTION_FAMILIES:
        ax.set_ylim(-_PROPORTION_MARGIN, 1.0 + _PROPORTION_MARGIN)
        ax.set_yticks(np.linspace(0.0, 1.0, 6))
        ax.set_ylabel("Share", **text_options)
    else:
        ax.set_ylabel("Value", **text_options)
    if count_ax is None:
        ax.set_xlabel(aggregated.axis_name.title(), **text_options)
    title = family.removeprefix("game.").rstrip(".").replace("_", " ").title()
    subtitle = _subtitle(len(table.series), aggregated.n_t, stat, ci, level)
    ax.set_title(f"{title}\n{subtitle}", **text_options)
    ax.grid(axis="y", color="#DDDDDD", linewidth=0.7)
    handles, legend_labels = ax.get_legend_handles_labels()
    if align == "available":
        handles.append(Line2D([], [], color="none"))
        legend_labels.append("Available replicates may be survivor-biased")
    ax.legend(handles, legend_labels, loc="upper left", frameon=False)

    if count_ax is not None:
        count_ax.step(aggregated.t, aggregated.n_t, where="mid", color="#666666")
        count_ax.set_ylabel("n", rotation=0, labelpad=10, **text_options)
        count_ax.set_xlabel(aggregated.axis_name.title(), **text_options)
        count_ax.set_ylim(0, max(aggregated.n_t) + 0.5)


def _metric_colors(
    spec: GameSpec | None,
    aggregated: Aggregated,
    palette: Sequence[str] | None,
) -> dict[str, str]:
    chosen = tuple(OKABE_ITO if palette is None else palette)
    if spec is not None:
        by_label = _action_colors(spec, palette)
        result = {
            name: by_label[label]
            for name, label in aggregated.labels.items()
            if label in by_label
        }
    else:
        result = {}
    missing = [name for name in aggregated.center if name not in result]
    if len(missing) > len(chosen):
        raise ValueError("the palette has fewer colors than metric series")
    result.update({name: chosen[index] for index, name in enumerate(missing)})
    return result


def _selected_labels(labels: str | Sequence[str], table: SeriesTable) -> list[str]:
    names = list(table.series[0].columns) if table.series else []
    if labels == "all":
        return names
    if isinstance(labels, str):
        requested = [labels]
    else:
        requested = list(labels)
    unknown = [name for name in requested if name not in names]
    if unknown:
        raise ValueError(f"unknown metric labels: {', '.join(unknown)}")
    if not requested:
        raise ValueError("labels must select at least one metric")
    return requested


def _select_solutions(
    solutions: Sequence[Solution], theory: str | Sequence[str]
) -> list[Solution]:
    if theory == "all":
        return list(solutions)
    requested = [theory] if isinstance(theory, str) else list(theory)
    if "all" in requested:
        return list(solutions)
    by_id = {solution.id: solution for solution in solutions}
    unknown = [identifier for identifier in requested if identifier not in by_id]
    if unknown:
        raise ValueError(f"unknown theory solution ids: {', '.join(unknown)}")
    return [by_id[identifier] for identifier in requested]


def _metric_name(family: str, metric_id: str) -> str:
    return family if family == "game.coop_rate" else f"{family}{metric_id}"


def _metric_id(name: str, family: str) -> str:
    if family == "game.coop_rate":
        return name.removeprefix("game.")
    return name.removeprefix(family)


def _downsample_indices(length: int, maximum: int) -> np.ndarray:
    if length <= maximum:
        return np.arange(length)
    return np.unique(np.linspace(0, length - 1, maximum, dtype=int))


def _optional_array(values: Sequence[float | None]) -> np.ndarray:
    return np.asarray([math.nan if value is None else value for value in values])


def _rolling(values: np.ndarray, window: int | None) -> np.ndarray:
    if window is None or window == 1:
        return values
    result = np.empty_like(values, dtype=float)
    for index in range(len(values)):
        start = max(0, index - window + 1)
        current = values[start : index + 1]
        result[index] = np.nan if np.all(np.isnan(current)) else np.nanmean(current)
    return result


def _spread_label_positions(
    values: Sequence[float], lower: float, upper: float
) -> list[float]:
    if len(values) < 2:
        return list(values)
    span = max(upper - lower, 1e-12)
    gap = 0.05 * span
    order = sorted(range(len(values)), key=lambda index: values[index])
    positions = [float(value) for value in values]
    for previous, current in zip(order, order[1:]):
        positions[current] = max(positions[current], positions[previous] + gap)
    overflow = positions[order[-1]] - upper
    if overflow > 0.0:
        for index in order:
            positions[index] -= overflow
    for current, following in zip(reversed(order[:-1]), reversed(order[1:])):
        positions[current] = min(positions[current], positions[following] - gap)
    underflow = lower - positions[order[0]]
    if underflow > 0.0:
        for index in order:
            positions[index] += underflow
    return positions


def _subtitle(
    n_series: int, n_t: Sequence[int], stat: str, ci: str, level: float
) -> str:
    if min(n_t) == max(n_t):
        count = f"n = {n_series} seeds"
    else:
        count = f"n = {min(n_t)}–{max(n_t)} available seeds"
    percent = f"{100.0 * level:g}%"
    if ci == "none":
        method = stat
    elif ci == "t":
        method = f"{stat} ± {percent} t-CI"
    elif ci == "bootstrap":
        method = f"{stat} ± {percent} bootstrap CI"
    else:
        method = f"{stat}, {percent} quantile range"
    return f"{count}, {method}"


def _warn_non_ascii(labels: Sequence[str], font: str | None) -> None:
    if font is None and any(not label.isascii() for label in labels):
        warnings.warn(
            "non-ASCII plot labels may require a font; pass font=... to the "
            "plotting function or style_context",
            UserWarning,
            stacklevel=3,
        )


__all__ = ["plot_share_series"]
