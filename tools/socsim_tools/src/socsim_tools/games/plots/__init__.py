"""Plots for exported normal-form game definitions and solutions."""

from socsim_tools.games.plots.matrix import (
    plot_best_response_2x2,
    plot_payoff_matrix,
)
from socsim_tools.games.plots.actions import plot_action_raster
from socsim_tools.games.plots.shares import plot_share_series
from socsim_tools.games.plots.simplex import plot_simplex

__all__ = [
    "plot_action_raster",
    "plot_best_response_2x2",
    "plot_payoff_matrix",
    "plot_share_series",
    "plot_simplex",
]
