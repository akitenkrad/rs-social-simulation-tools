**English** | [日本語](README.ja.md)

# socsim-tools

`socsim-tools` is the shared Python library for command-line routing,
experiment-settings rendering, results I/O, and normal-form-game plots used by
socsim replications. Replications can adopt it as a git subdirectory dependency
and keep only their model-specific configuration and plots locally.

## Helpers

| Module | Helpers | Purpose |
|---|---|---|
| `socsim_tools.cli` | `build_dispatcher(prog, description, subcommands)` | build an argparse subparser and argv router |
| `socsim_tools.settings` | `render_run_metadata` / `render_run_config` / `show_experiment_settings_main` | render experiment settings and the shared LLM metadata block |
| `socsim_tools.io` | `resolve_results_dir` / `load_json` / `load_config` / `load_run_metadata` / `load_metrics` | find and load `results/{timestamp}` and `results/latest` |
| `socsim_tools.games` | `load_game_spec` / `load_solutions`; theory, plotting, styling, saving, and CLI subcommands in the package's submodules | read the versioned `socsim-games` export contract and draw payoff matrices, best-response arrows, and replicator fields |

`render_run_metadata` emits the same LLM metadata block in every replication:
`llm_model`, `llm_endpoint`, `llm_temperature`, `llm_seed`, `total_calls`,
`cache_hits`, and `cache_hit_rate`. The replication-specific part is the
`field_labels` mapping passed to `render_run_config`.

## Dependencies

The core dispatcher, settings helpers, and config/metadata readers use only the
Python standard library. Install an extra only for the corresponding feature:

```bash
pip install "socsim-tools[io]"       # pandas for load_metrics
pip install "socsim-tools[games]"    # matplotlib, numpy, and scipy
```

Importing `socsim_tools` or `socsim_tools.games` does not import the numerical
or plotting dependencies. `load_game_spec` and `load_solutions` remain usable
without the `games` extra; import `socsim_tools.games.theory`, `.plots`, or
`.style` only when needed.

The game CLI reads the `game_spec.json` and optional `solutions.json` written by
the Rust `socsim-games` `export` feature:

```bash
socsim-tools plot-game \
  --game-spec exported/battle_of_sexes/game_spec.json \
  --solutions exported/battle_of_sexes/solutions.json \
  --output-dir figures

socsim-tools plot-simplex \
  --game-spec exported/rock_paper_scissors/game_spec.json \
  --solutions exported/rock_paper_scissors/solutions.json \
  --output-dir figures
```

## Adopting it in a replication

Add the git subdirectory dependency to `tools/pyproject.toml`:

```toml
[project]
dependencies = [
    "socsim-tools[games]",
    # ... replication-specific dependencies
]

[tool.uv.sources]
socsim-tools = { git = "https://github.com/akitenkrad/rs-social-simulation-tools", subdirectory = "tools/socsim_tools" }
```

Then replace copied dispatcher and settings code with the shared helpers:

```python
# <pkg>_tools/cli.py
from socsim_tools.cli import build_dispatcher

main = build_dispatcher(
    prog="chuang-tools",
    description="Chuang et al. (2024) visualisation and analysis tools",
    subcommands={
        "visualize": ("Visualize one run", "chuang_tools.visualize:main"),
        "visualize-sweep": (
            "Visualize a sweep",
            "chuang_tools.visualize_sweep:main",
        ),
        "show-experiment-settings": (
            "Show experiment settings",
            "chuang_tools.show_experiment_settings:main",
        ),
    },
)
```

```python
# <pkg>_tools/show_experiment_settings.py
from socsim_tools.settings import show_experiment_settings_main

FIELD_LABELS = {
    "n_agents": "Number of agents N ",
    "topic": "Topic              ",
    # ... padded labels specific to the replication
}


def main(argv=None):
    return show_experiment_settings_main(
        argv,
        field_labels=FIELD_LABELS,
        prog="chuang-tools show-experiment-settings",
    )
```

Pass labels padded so their right-hand colons align, as in the existing tools.
If a replication needs compound rows such as `Taylor α_π/α_u`, share only
`render_run_metadata` and keep its settings table in the replication.
