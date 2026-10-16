"""Aggregate compatible game metric series across simulation runs."""

from __future__ import annotations

import math
import warnings
from dataclasses import dataclass

import numpy as np
from scipy import stats

from socsim_tools.games.records import SeriesTable


@dataclass(frozen=True)
class Aggregated:
    """A center line, optional interval, and replicate counts by time."""

    t: list[int]
    axis_name: str
    center: dict[str, list[float]]
    lower: dict[str, list[float | None]] | None
    upper: dict[str, list[float | None]] | None
    n_t: list[int]
    labels: dict[str, str]


def aggregate_series(
    table: SeriesTable,
    stat: str = "mean",
    ci: str = "t",
    level: float = 0.95,
    align: str = "intersection",
) -> Aggregated:
    """Aggregate series with a common set of metric columns.

    Bootstrap intervals use one ``numpy.random.default_rng(0)`` per call.
    In ascending time order, the function draws
    ``rng.integers(0, n_t, size=(2000, n_t))``, computes the requested
    statistic row by row, and takes the bounds with the default linear method
    of ``numpy.percentile``. The generator is not reset between time points.
    """

    if stat not in {"mean", "median"}:
        raise ValueError("stat must be 'mean' or 'median'")
    if ci not in {"t", "bootstrap", "quantile", "none"}:
        raise ValueError("ci must be 't', 'bootstrap', 'quantile', or 'none'")
    if stat == "median" and ci == "t":
        raise ValueError("median cannot be combined with a t interval")
    if align not in {"intersection", "available"}:
        raise ValueError("align must be 'intersection' or 'available'")
    if not isinstance(level, (int, float)) or isinstance(level, bool):
        raise ValueError("level must be a number between zero and one")
    if not 0.0 < float(level) < 1.0:
        raise ValueError("level must be between zero and one")
    if not table.series:
        raise ValueError("table must contain at least one series")

    axis_name = table.series[0].axis_name
    names = list(table.series[0].columns)
    expected_names = set(names)
    time_maps: list[dict[int, dict[str, float]]] = []
    time_sets: list[set[int]] = []
    for series in table.series:
        if series.axis_name != axis_name:
            raise ValueError("all series must use the same axis name")
        if set(series.columns) != expected_names:
            raise ValueError("all series must use the same metric columns")
        if len(series.t) != len(set(series.t)):
            raise ValueError(f"series {series.source} contains duplicate times")
        if any(len(values) != len(series.t) for values in series.columns.values()):
            raise ValueError(
                f"series {series.source} columns must align with its time values"
            )
        mapping = {
            t: {name: float(series.columns[name][index]) for name in names}
            for index, t in enumerate(series.t)
        }
        time_maps.append(mapping)
        time_sets.append(set(series.t))

    if align == "intersection":
        selected = set.intersection(*time_sets)
    else:
        selected = set.union(*time_sets)
    times = sorted(selected)
    if not times:
        raise ValueError("the selected alignment contains no time points")

    center = {name: [] for name in names}
    lower = None if ci == "none" else {name: [] for name in names}
    upper = None if ci == "none" else {name: [] for name in names}
    n_t: list[int] = []
    tail = (1.0 - float(level)) * 50.0
    rng = np.random.default_rng(0) if ci == "bootstrap" else None
    warned_single = False

    for t in times:
        rows = [mapping[t] for mapping in time_maps if t in mapping]
        count = len(rows)
        n_t.append(count)
        sample_indices = None
        if rng is not None:
            sample_indices = rng.integers(0, count, size=(2000, count))
        if ci != "none" and count == 1 and not warned_single:
            warnings.warn(
                "confidence bands are omitted where only one replicate is available",
                UserWarning,
                stacklevel=2,
            )
            warned_single = True

        for name in names:
            values = np.asarray([row[name] for row in rows], dtype=float)
            if not np.all(np.isfinite(values)):
                raise ValueError("series values must be finite")
            estimate = _statistic(values, stat)
            center[name].append(estimate)
            if lower is None or upper is None:
                continue
            if count == 1:
                lower[name].append(None)
                upper[name].append(None)
                continue
            lo, hi = _interval(
                values,
                estimate,
                stat,
                ci,
                float(level),
                tail,
                sample_indices,
            )
            lower[name].append(lo)
            upper[name].append(hi)

    return Aggregated(
        t=times,
        axis_name=axis_name,
        center=center,
        lower=lower,
        upper=upper,
        n_t=n_t,
        labels=dict(table.labels),
    )


def _statistic(values: np.ndarray, stat: str) -> float:
    if stat == "mean":
        return float(np.mean(values))
    return float(np.median(values))


def _interval(
    values: np.ndarray,
    estimate: float,
    stat: str,
    ci: str,
    level: float,
    tail: float,
    sample_indices: np.ndarray | None,
) -> tuple[float, float]:
    if ci == "t":
        critical = float(stats.t.ppf((1.0 + level) / 2.0, len(values) - 1))
        standard_error = float(np.std(values, ddof=1)) / math.sqrt(len(values))
        margin = critical * standard_error
        return estimate - margin, estimate + margin
    if ci == "quantile":
        bounds = np.percentile(values, [tail, 100.0 - tail])
        return float(bounds[0]), float(bounds[1])
    if ci == "bootstrap":
        if sample_indices is None:  # pragma: no cover - internal invariant
            raise RuntimeError("bootstrap sample indices were not initialized")
        samples = values[sample_indices]
        if stat == "mean":
            statistics = np.mean(samples, axis=1)
        else:
            statistics = np.median(samples, axis=1)
        bounds = np.percentile(statistics, [tail, 100.0 - tail])
        return float(bounds[0]), float(bounds[1])
    raise RuntimeError(f"unsupported interval kind {ci!r}")


__all__ = ["Aggregated", "aggregate_series"]
