"""Accessible plotting colors, local rcParams, and deterministic saving."""

from __future__ import annotations

import os
from collections.abc import Sequence
from contextlib import AbstractContextManager
from pathlib import Path
from typing import TYPE_CHECKING

import matplotlib as mpl

if TYPE_CHECKING:
    from matplotlib.figure import Figure

    from socsim_tools.games.spec import GameSpec

OKABE_ITO = (
    "#0072B2",
    "#D55E00",
    "#009E73",
    "#E69F00",
    "#56B4E9",
    "#CC79A7",
    "#F0E442",
    "#000000",
)
ROLE_COLORS = ("#0072B2", "#D55E00")
CONTINUOUS_CMAP = "cividis"
MISSING_COLOR = "#DDDDDD"


def style_context(font: str | None = None) -> AbstractContextManager[None]:
    """Return a local matplotlib style context for socsim game figures.

    The context sets DejaVu Sans by default, a 10-point base font, 1.5-point
    lines, and a 6.4-by-4.8-inch figure. It does not change the global backend
    or SVG save-time settings.
    """

    return mpl.rc_context(
        rc={
            "font.family": font or "DejaVu Sans",
            "font.size": 10.0,
            "lines.linewidth": 1.5,
            "figure.figsize": (6.4, 4.8),
        }
    )


def save_figure(
    fig: Figure,
    path_stem: str | os.PathLike[str],
    formats: Sequence[str] = ("png", "svg"),
    dpi: int = 150,
) -> list[Path]:
    """Save a figure in each requested format and return the written paths.

    Parent directories are created. PNG output omits the matplotlib software
    metadata. SVG output omits the date and is saved with fixed hash IDs and
    path-embedded fonts. No tight bounding box is applied.
    """

    stem = Path(path_stem)
    stem.parent.mkdir(parents=True, exist_ok=True)
    written = []
    with mpl.rc_context(
        rc={"svg.hashsalt": "socsim-games", "svg.fonttype": "path"}
    ):
        for requested in formats:
            if not isinstance(requested, str) or not requested:
                raise ValueError("formats must contain non-empty strings")
            file_format = requested.removeprefix(".").lower()
            if not file_format:
                raise ValueError("formats must contain non-empty format names")
            path = Path(f"{stem}.{file_format}")
            metadata = None
            if file_format == "svg":
                metadata = {"Date": None}
            elif file_format == "png":
                metadata = {"Software": None}
            fig.savefig(path, format=file_format, dpi=dpi, metadata=metadata)
            written.append(path)
    return written


def _action_colors(
    spec: GameSpec, palette: Sequence[str] | None = None
) -> dict[str, str]:
    """Build the stable action-label color mapping used by game plots."""

    labels = []
    for role_labels in spec.action_labels or []:
        for label in role_labels:
            if label not in labels:
                labels.append(label)
    colors = OKABE_ITO if palette is None else tuple(palette)
    if len(labels) > len(colors):
        if palette is None:
            raise ValueError(
                "more than eight action labels require an explicit palette"
            )
        raise ValueError("the explicit palette has fewer colors than action labels")
    return dict(zip(labels, colors))


__all__ = [
    "CONTINUOUS_CMAP",
    "MISSING_COLOR",
    "OKABE_ITO",
    "ROLE_COLORS",
    "save_figure",
    "style_context",
]
