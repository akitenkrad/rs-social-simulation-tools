"""Per-agent action rasters with optional LLM decision marks."""

from __future__ import annotations

from collections.abc import Sequence
import warnings

import matplotlib as mpl
from matplotlib.colors import BoundaryNorm, ListedColormap
from matplotlib.figure import Figure
from matplotlib.patches import Patch
import numpy as np

from socsim_tools.games.records import ActionLog, LlmCall
from socsim_tools.games.spec import GameSpec
from socsim_tools.games.style import MISSING_COLOR, _action_colors, style_context


def plot_action_raster(
    log: ActionLog,
    spec: GameSpec,
    llm_calls: Sequence[LlmCall] | None = None,
    *,
    t_range: tuple[int | None, int | None] | None = None,
    agents: Sequence[int] | None = None,
    sort_by: str = "id",
    show_payoffs: bool = False,
    max_cells: int = 2_000_000,
    llm_marks: bool | None = None,
    ax=None,
    palette: Sequence[str] | None = None,
    font: str | None = None,
) -> Figure:
    """Plot one action cell per displayed agent and recorded round.

    The optional ``t_range`` uses inclusive endpoints. LLM marks are enabled
    automatically when calls are supplied; ``llm_marks=False`` suppresses them.
    """

    if spec.kind != "finite" or spec.action_labels is None:
        raise ValueError("plot_action_raster requires a finite game specification")
    if type(max_cells) is not int or max_cells < 1:
        raise ValueError("max_cells must be a positive integer")
    if sort_by not in {"id", "kind"}:
        raise ValueError("sort_by must be 'id' or 'kind'")
    selected_times = _selected_times(log, t_range)
    selected_agents = _selected_agents(log, agents, sort_by)
    if len(selected_times) * len(selected_agents) > max_cells:
        raise ValueError(
            f"action raster exceeds max_cells={max_cells}: "
            f"{len(selected_times) * len(selected_agents)} cells"
        )
    colors = _action_colors(spec, palette)
    _warn_non_ascii([*colors, *log.kinds], font)
    calls = list(llm_calls or [])
    draw_marks = bool(calls) if llm_marks is None else llm_marks

    if ax is None:
        with style_context(font=font):
            figure = Figure(figsize=tuple(mpl.rcParams["figure.figsize"]))
            if show_payoffs:
                axes = figure.subplots(
                    2,
                    1,
                    sharex=True,
                    gridspec_kw={"height_ratios": (4, 1), "hspace": 0.08},
                )
                axis, payoff_axis = axes
            else:
                axis = figure.subplots()
                payoff_axis = None
            figure.subplots_adjust(left=0.28, right=0.78, bottom=0.12, top=0.88)
            _draw_action_raster(
                axis,
                payoff_axis,
                log,
                spec,
                calls,
                selected_times,
                selected_agents,
                colors,
                draw_marks,
                font,
            )
            return figure
    payoff_axis = ax.inset_axes([0.0, -0.32, 1.0, 0.18]) if show_payoffs else None
    _draw_action_raster(
        ax,
        payoff_axis,
        log,
        spec,
        calls,
        selected_times,
        selected_agents,
        colors,
        draw_marks,
        font,
    )
    return ax.figure


def _draw_action_raster(
    ax,
    payoff_ax,
    log: ActionLog,
    spec: GameSpec,
    calls: list[LlmCall],
    selected_times: list[int],
    selected_agents: list[int],
    colors: dict[str, str],
    draw_marks: bool,
    font: str | None,
) -> None:
    text_options = {} if font is None else {"fontfamily": font}
    time_positions = [log.t.index(value) for value in selected_times]
    agent_positions = [log.agents.index(value) for value in selected_agents]
    labels = list(colors)
    codes = np.zeros((len(selected_agents), len(selected_times)), dtype=int)
    for row, agent_index in enumerate(agent_positions):
        for column, time_index in enumerate(time_positions):
            action = log.actions[time_index][agent_index]
            role = log.roles[time_index][agent_index]
            if action is None:
                continue
            role_index = 0 if role is None else role
            if role_index < 0 or role_index >= len(spec.action_labels):
                raise ValueError("recorded role is outside the game specification")
            role_labels = spec.action_labels[role_index]
            if action < 0 or action >= len(role_labels):
                raise ValueError("recorded action is outside the game specification")
            codes[row, column] = labels.index(role_labels[action]) + 1

    cmap = ListedColormap([MISSING_COLOR, *(colors[label] for label in labels)])
    norm = BoundaryNorm(np.arange(-0.5, len(labels) + 1.5), cmap.N)
    image = ax.imshow(
        codes,
        cmap=cmap,
        norm=norm,
        interpolation="nearest",
        aspect="auto",
        extent=(
            selected_times[0] - 0.5,
            selected_times[-1] + 0.5,
            len(selected_agents) - 0.5,
            -0.5,
        ),
    )
    image.set_gid("action-raster")
    agent_to_row = {agent: row for row, agent in enumerate(selected_agents)}
    kind_by_agent = dict(zip(log.agents, log.kinds))
    ax.set_yticks(range(len(selected_agents)))
    ax.set_yticklabels(
        [f"{agent}: {kind_by_agent[agent]}" for agent in selected_agents],
        **text_options,
    )
    if payoff_ax is None:
        ax.set_xlabel("Round", **text_options)
    ax.set_ylabel("Agent", **text_options)
    ax.set_title(f"Actions: {spec.name.replace('_', ' ').title()}", **text_options)
    handles = [Patch(facecolor=colors[label], label=label) for label in labels]
    handles.append(Patch(facecolor=MISSING_COLOR, label="not played"))
    ax.legend(
        handles=handles,
        loc="upper left",
        bbox_to_anchor=(1.01, 1.0),
        borderaxespad=0.0,
        frameon=False,
    )

    if draw_marks:
        last = {(call.t, call.agent): call for call in calls}
        visible_times = set(selected_times)
        for (t, agent), call in last.items():
            if t not in visible_times or agent not in agent_to_row:
                continue
            if call.resolution == "giveup":
                marker, size = "x", 7
            elif call.resolution == "accept" and call.attempt >= 1:
                marker, size = "o", 3
            else:
                continue
            artist = ax.plot(
                [t],
                [agent_to_row[agent]],
                color="black",
                marker=marker,
                markersize=size,
                linestyle="none",
                zorder=4,
            )[0]
            artist.set_gid(f"llm/{call.resolution}/{t}/{agent}")

    if payoff_ax is not None:
        for agent, agent_index in zip(selected_agents, agent_positions):
            payoff_ax.plot(
                selected_times,
                [log.payoffs[index][agent_index] for index in time_positions],
                linewidth=0.8,
                label=str(agent),
            )
        payoff_ax.set_ylabel("Payoff", **text_options)
        payoff_ax.set_xlabel("Round", **text_options)
        payoff_ax.grid(axis="y", color="#DDDDDD", linewidth=0.7)


def _selected_times(
    log: ActionLog, t_range: tuple[int | None, int | None] | None
) -> list[int]:
    if not log.t:
        raise ValueError("action log contains no recorded rounds")
    if t_range is None:
        return list(log.t)
    start, end = t_range
    if start is not None and end is not None and start > end:
        raise ValueError("t_range start must not exceed its end")
    selected = [
        t for t in log.t if (start is None or t >= start) and (end is None or t <= end)
    ]
    if not selected:
        raise ValueError("t_range selects no recorded rounds")
    return selected


def _selected_agents(
    log: ActionLog, agents: Sequence[int] | None, sort_by: str
) -> list[int]:
    requested = list(log.agents) if agents is None else list(agents)
    if not requested:
        raise ValueError("agents must select at least one agent")
    if len(requested) != len(set(requested)):
        raise ValueError("agents must not contain duplicates")
    unknown = [agent for agent in requested if agent not in log.agents]
    if unknown:
        raise ValueError(f"unknown agents: {', '.join(map(str, unknown))}")
    kinds = dict(zip(log.agents, log.kinds))
    if sort_by == "id":
        return sorted(requested)
    return sorted(requested, key=lambda agent: (kinds[agent], agent))


def _warn_non_ascii(labels: Sequence[str], font: str | None) -> None:
    if font is None and any(not label.isascii() for label in labels):
        warnings.warn(
            "non-ASCII plot labels may require a font; pass font=... to the "
            "plotting function or style_context",
            UserWarning,
            stacklevel=3,
        )


__all__ = ["plot_action_raster"]
