"""socsim_tools.io — results ディレクトリの探索・ロードヘルパ．

各 replication の `results/{timestamp}/` レイアウトを横断的に扱う:
  - `results/latest` シンボリックリンク (実体に解決)
  - 明示的な `--results-dir`
  - 引数なしのときの「最新タイムスタンプディレクトリ」自動選択
config.json / sweep_config.json / run_metadata.json (または llm_meta.json) /
metrics.csv のロードを提供する．

`pandas` は `load_metrics` 内で遅延 import するため，metrics をロードしない限り
依存として不要 (コア dispatcher / settings レンダラは stdlib のみで動く)．
"""

from __future__ import annotations

import json
import os
from pathlib import Path
from typing import Any


def resolve_results_dir(results_dir: str | None, base: str = "results") -> Path:
    """results ディレクトリを絶対パス (symlink は実体) に解決する．

    解決順:
      1. ``results_dir`` が明示指定されていれば，それを (相対なら cwd 基準で) 解決．
      2. 未指定 (None) のとき，``base/latest`` が存在すればそれを解決．
      3. それも無ければ ``base/`` 直下の最も新しい (名前順で最大の) サブディレクトリを返す．

    いずれも `os.path.realpath` で実体パスへ正規化して返す．
    候補が一つも無い場合は ``base/latest`` の (未解決) パスを返す
    (呼び出し側が存在チェックでエラー表示できるよう)．
    """
    if results_dir is not None:
        p = Path(results_dir)
        if not p.is_absolute():
            candidates = [Path.cwd() / results_dir, p]
            for c in candidates:
                if c.exists():
                    p = c
                    break
            else:
                p = candidates[0]
        return Path(os.path.realpath(p))

    base_path = Path(base)
    latest = base_path / "latest"
    if latest.exists():
        return Path(os.path.realpath(latest))

    if base_path.is_dir():
        subdirs = sorted(
            (d for d in base_path.iterdir() if d.is_dir()),
            key=lambda d: d.name,
        )
        if subdirs:
            return Path(os.path.realpath(subdirs[-1]))

    return Path(os.path.realpath(latest))


def load_json(path: str | os.PathLike[str]) -> dict[str, Any]:
    """JSON ファイルを dict としてロードする．"""
    with Path(path).open(encoding="utf-8") as f:
        return json.load(f)


def load_config(results_dir: str | os.PathLike[str]) -> tuple[dict[str, Any], Path]:
    """config.json (run/benchmark) か sweep_config.json (sweep) をロードする．

    Returns ``(config_dict, source_path)``．どちらも無ければ ``FileNotFoundError``．
    """
    rd = Path(results_dir)
    run_cfg = rd / "config.json"
    sweep_cfg = rd / "sweep_config.json"
    if run_cfg.exists():
        return load_json(run_cfg), run_cfg
    if sweep_cfg.exists():
        return load_json(sweep_cfg), sweep_cfg
    raise FileNotFoundError(
        f"設定ファイルが見つかりません: {rd}\n"
        f"  期待されるファイル: config.json (run) または sweep_config.json (sweep)"
    )


def load_run_metadata(results_dir: str | os.PathLike[str]) -> dict[str, Any] | None:
    """LLM 実行メタデータをロードする (存在しなければ None)．

    ``run_metadata.json`` を優先し，無ければ ``llm_meta.json`` を探す
    (replication 間でファイル名が揺れるため両対応)．
    """
    rd = Path(results_dir)
    for name in ("run_metadata.json", "llm_meta.json"):
        path = rd / name
        if path.exists():
            return load_json(path)
    return None


def load_metrics(results_dir: str | os.PathLike[str]):
    """metrics.csv を ``pandas.DataFrame`` としてロードする．

    ``pandas`` をこの関数内で遅延 import するため，metrics を使わない限り
    pandas は不要．未インストールなら分かりやすい ``ImportError`` を投げる．
    metrics.csv が無ければ ``FileNotFoundError``．
    """
    try:
        import pandas as pd
    except ImportError as exc:  # pragma: no cover - 環境依存
        raise ImportError(
            "load_metrics には pandas が必要です．"
            ' `pip install "socsim-tools[io]"` でインストールしてください．'
        ) from exc

    path = Path(results_dir) / "metrics.csv"
    if not path.exists():
        raise FileNotFoundError(f"metrics.csv が見つかりません: {path}")
    return pd.read_csv(path)
