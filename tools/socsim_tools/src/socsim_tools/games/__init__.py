"""Lightweight readers for exported socsim game artifacts.

Importing this package does not import the optional numerical or plotting
dependencies. Import :mod:`socsim_tools.games.theory` or
:mod:`socsim_tools.games.style` explicitly when those facilities are needed.
"""

from socsim_tools.games.spec import load_game_spec, load_solutions

__all__ = ["load_game_spec", "load_solutions"]

