[English](README.md) | **日本語**

# socsim-tools

`socsim-tools` は，socsim の replication が使う CLI の振り分け，実験設定の表示，
結果の I/O，戦略形ゲームの描画を集めた共有 Python ライブラリです．各 replication は
git subdirectory 依存として取り込み，モデル固有の設定と図だけを手元に残せます．

## ヘルパ

| モジュール | ヘルパ | 役割 |
|---|---|---|
| `socsim_tools.cli` | `build_dispatcher(prog, description, subcommands)` | argparse の subparser と argv の振り分けを作る |
| `socsim_tools.settings` | `render_run_metadata` / `render_run_config` / `show_experiment_settings_main` | 実験設定と共通の LLM metadata ブロックを整形する |
| `socsim_tools.io` | `resolve_results_dir` / `load_json` / `load_config` / `load_run_metadata` / `load_metrics` | `results/{timestamp}` と `results/latest` を探して読む |
| `socsim_tools.games` | `load_game_spec` / `load_solutions`．理論値，描画，style，保存，CLI はサブモジュール | 版付きの `socsim-games` の書き出し契約を読み，利得行列，最適応答の矢印，レプリケータ場を描く |

`render_run_metadata` が出す LLM metadata のブロックは，すべての replication で同じです：
`llm_model`，`llm_endpoint`，`llm_temperature`，`llm_seed`，`total_calls`，
`cache_hits`，`cache_hit_rate`．replication ごとに変わるのは，`render_run_config` に渡す
`field_labels` の対応です．

## 依存

コアの dispatcher，settings ヘルパ，config/metadata の読込は Python の標準ライブラリだけで
動きます．必要な機能の extra だけを入れます．

```bash
pip install "socsim-tools[io]"       # load_metrics 用の pandas
pip install "socsim-tools[games]"    # matplotlib，numpy，scipy
```

`socsim_tools` または `socsim_tools.games` を import しても，数値計算や描画の依存は
import されません．`load_game_spec` と `load_solutions` は `games` extra が無くても使えます．
必要になったときだけ `socsim_tools.games.theory`，`.plots`，`.style` を import します．

ゲーム用 CLI は，Rust の `socsim-games` の feature `export` が書いた `game_spec.json` と任意の
`solutions.json` を読みます．

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

## replication への取り込み

`tools/pyproject.toml` に git subdirectory 依存を追加します．

```toml
[project]
dependencies = [
    "socsim-tools[games]",
    # ... replication 固有の依存
]

[tool.uv.sources]
socsim-tools = { git = "https://github.com/akitenkrad/rs-social-simulation-tools", subdirectory = "tools/socsim_tools" }
```

そのうえで，コピーしていた dispatcher と settings のコードを共有ヘルパに置き換えます．

```python
# <pkg>_tools/cli.py
from socsim_tools.cli import build_dispatcher

main = build_dispatcher(
    prog="chuang-tools",
    description="Chuang et al. (2024) 可視化・分析ツール",
    subcommands={
        "visualize": ("単一実行結果の可視化", "chuang_tools.visualize:main"),
        "visualize-sweep": (
            "スイープ結果の可視化",
            "chuang_tools.visualize_sweep:main",
        ),
        "show-experiment-settings": (
            "実行設定の表示",
            "chuang_tools.show_experiment_settings:main",
        ),
    },
)
```

```python
# <pkg>_tools/show_experiment_settings.py
from socsim_tools.settings import show_experiment_settings_main

FIELD_LABELS = {
    "n_agents": "エージェント数 N ",
    "topic": "トピック         ",
    # ... replication 固有のラベル
}


def main(argv=None):
    return show_experiment_settings_main(
        argv,
        field_labels=FIELD_LABELS,
        prog="chuang-tools show-experiment-settings",
    )
```

右側のコロンの桁を揃えるため，既存ツールと同じく空白で padding したラベルを渡します．
`Taylor α_π/α_u` のような複合行が必要な replication では，`render_run_metadata` だけを共有し，
設定の表は replication 側に残します．
