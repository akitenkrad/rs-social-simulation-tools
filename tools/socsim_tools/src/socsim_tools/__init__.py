"""socsim_tools — socsim replication 群の `tools/` パッケージが共有する基盤．

各 replication の `tools/` にコピーされていた ~95%% 同一のボイラープレート
(CLI dispatcher + 実験設定レンダラ + results I/O) を 1 箇所に集約する．

公開ヘルパ:
    - ``build_dispatcher``                 : 共有 CLI dispatcher (cli)
    - ``resolve_results_dir`` / ``load_*`` : results ディレクトリ探索・ロード (io)
    - ``render_run_metadata``              : 一様な LLM メタデータブロック (settings)
    - ``render_run_config``                : 汎用設定テーブル (settings)
    - ``show_experiment_settings_main``    : drop-in な show-experiment-settings main
"""

from __future__ import annotations

from socsim_tools.cli import build_dispatcher
from socsim_tools.io import (
    load_config,
    load_json,
    load_metrics,
    load_run_metadata,
    resolve_results_dir,
)
from socsim_tools.settings import (
    render_run_config,
    render_run_metadata,
    show_experiment_settings_main,
)

__all__ = [
    "build_dispatcher",
    "resolve_results_dir",
    "load_json",
    "load_config",
    "load_run_metadata",
    "load_metrics",
    "render_run_metadata",
    "render_run_config",
    "show_experiment_settings_main",
]
