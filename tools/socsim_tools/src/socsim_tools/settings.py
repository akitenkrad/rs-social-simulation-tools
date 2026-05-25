"""socsim_tools.settings — 実験設定 (config / run_metadata) の整形表示．

各 replication の `show_experiment_settings.py` が共有する renderer 群．
出力フォーマット (``=`` × 70 / ``-`` × 70 区切り，日本語ラベル) は既存の
chuang2024 / li2024 の出力と **バイト等価** になるよう設計してある．
replication 固有の差異は ``render_run_config`` に渡す ``field_labels`` (config の
キー → 表示ラベル) のみ．run_metadata ブロックは全 replication で一様．
"""

from __future__ import annotations

import argparse
import json
import os
import sys
from typing import Any

from socsim_tools.io import load_config, load_run_metadata, resolve_results_dir


def render_run_metadata(meta: dict[str, Any]) -> str:
    """LLM 実行メタデータの一様ブロックを整形する．

    対象キー (全 replication で共通): ``llm_model`` / ``llm_endpoint`` /
    ``llm_temperature`` / ``llm_seed`` / ``total_calls`` / ``cache_hits`` /
    ``cache_hit_rate`` (0..1 → %.1f%% 表示)．任意で ``determinism_note``．

    出力は chuang2024 / li2024 の ``render_run_metadata`` とバイト等価．
    """
    lines: list[str] = []
    lines.append("")
    lines.append("LLM 実行メタデータ (run_metadata.json)")
    lines.append("-" * 70)
    lines.append(f"モデル           : {meta.get('llm_model', '-')}")
    lines.append(f"endpoint         : {meta.get('llm_endpoint', '-')}")
    lines.append(f"温度             : {meta.get('llm_temperature', '-')}")
    lines.append(f"seed             : {meta.get('llm_seed', '-')}")
    lines.append(f"呼び出し総数     : {meta.get('total_calls', '-')}")
    lines.append(f"cache-hit        : {meta.get('cache_hits', '-')}")
    rate = meta.get("cache_hit_rate")
    if rate is not None:
        lines.append(f"cache-hit 率     : {rate * 100:.1f}%")
    note = meta.get("determinism_note")
    if note:
        lines.append("-" * 70)
        lines.append(f"注記: {note}")
    lines.append("=" * 70)
    return "\n".join(lines)


def render_run_config(
    cfg: dict[str, Any],
    source: str | os.PathLike[str],
    field_labels: dict[str, str],
    *,
    title: str = "実行設定 (run)",
) -> str:
    """汎用の設定テーブルを整形する．

    ``field_labels`` は cfg のキー → 表示ラベルの順序付き dict．これが
    replication ごとの唯一の差異．ラベルは右側コロンの位置を揃えるため
    呼び出し側で空白パディング済みの文字列を渡す想定 (既存実装と同様)．
    複合行 (例: ``"Taylor α_π/α_u   : {alpha_pi} / {alpha_u}"``) のような
    特殊整形が必要な場合はこの関数を使わず replication 側で直接組む．
    """
    lines: list[str] = []
    lines.append("=" * 70)
    lines.append(title)
    lines.append("=" * 70)
    lines.append(f"設定ファイル: {source}")
    lines.append("-" * 70)
    for key, label in field_labels.items():
        lines.append(f"{label}: {cfg.get(key, '-')}")
    lines.append("=" * 70)
    return "\n".join(lines)


def show_experiment_settings_main(
    argv: list[str] | None = None,
    *,
    field_labels: dict[str, str],
    title: str = "実行設定 (run)",
    prog: str = "socsim-tools show-experiment-settings",
) -> int:
    """各 replication が呼び出せる drop-in な ``main()``．

    ``--results-dir`` / ``--json`` を解析し，ディレクトリを解決して
    ``render_run_config`` + ``render_run_metadata`` を出力する (または ``--json``)．
    ``field_labels`` はそのまま ``render_run_config`` に渡す．
    """
    parser = argparse.ArgumentParser(
        prog=prog,
        description="実行結果ディレクトリの設定 (config / run_metadata) を表示する．",
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument(
        "--results-dir",
        "--results_dir",
        default="results/latest",
        help="実行結果ディレクトリ (default: results/latest)",
    )
    parser.add_argument(
        "--json",
        action="store_true",
        help="表ではなく JSON 形式で出力する．",
    )
    args = parser.parse_args(argv)

    results_dir = resolve_results_dir(args.results_dir)
    if not results_dir.exists():
        print(f"エラー: ディレクトリが存在しません: {results_dir}", file=sys.stderr)
        return 1

    try:
        cfg, cfg_path = load_config(results_dir)
    except FileNotFoundError as exc:
        print(f"エラー: {exc}", file=sys.stderr)
        return 1
    meta = load_run_metadata(results_dir)

    if args.json:
        payload = {"source": str(cfg_path), "config": cfg, "run_metadata": meta}
        print(json.dumps(payload, indent=2, ensure_ascii=False))
    else:
        print(render_run_config(cfg, cfg_path, field_labels, title=title))
        if meta is not None:
            print(render_run_metadata(meta))
    return 0
