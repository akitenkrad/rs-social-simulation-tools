"""socsim_tools.cli — 各 replication の `tools/` が共有する CLI dispatcher ビルダ．

既存の `<pkg>_tools/cli.py` は ~95%% 同一の argparse subparser + argv ルーティング
ボイラープレートだった．``build_dispatcher`` はそれを 1 行で構築できるようにする::

    from socsim_tools.cli import build_dispatcher

    main = build_dispatcher(
        prog="chuang-tools",
        description="Chuang et al. (2024) LLM 意見力学 可視化・分析ツール",
        subcommands={
            "visualize": ("単一実行結果の可視化", "chuang_tools.visualize:main"),
            "visualize-sweep": ("スイープ結果の可視化", "chuang_tools.visualize_sweep:main"),
            "show-experiment-settings": (
                "実行設定の表示", "chuang_tools.show_experiment_settings:main"
            ),
        },
    )

サブコマンドに続く引数は対応モジュールの ``main(rest)`` がそのまま受け取る．
サブコマンドレベルの ``--help`` は，そのモジュール自身のヘルプに委ねられる
(subparser を ``add_help=False`` で登録するため)．
"""

from __future__ import annotations

import argparse
import importlib
import sys
from typing import Callable


def _resolve_target(target: str) -> Callable[[list[str]], object]:
    """``"module.path:func"`` 形式の import ターゲットを呼び出し可能オブジェクトに解決する．"""
    if ":" not in target:
        raise ValueError(
            f"サブコマンドのターゲットは 'module.path:func' 形式である必要があります: {target!r}"
        )
    module_path, func_name = target.split(":", 1)
    module = importlib.import_module(module_path)
    return getattr(module, func_name)


def build_dispatcher(
    prog: str,
    description: str,
    subcommands: dict[str, tuple[str, str]],
) -> Callable[[list[str] | None], None]:
    """共有 CLI dispatcher の ``main(argv)`` を構築して返す．

    Args:
        prog: プログラム名 (例 ``"chuang-tools"``)．
        description: トップレベル parser の説明文．
        subcommands: ``name -> (help_text, "module.path:func")``．``func`` は
            ``main(rest: list[str])`` シグネチャの呼び出し可能オブジェクトを指す
            import ターゲット (遅延 import される)．

    Returns:
        ``main(argv: list[str] | None = None) -> None``．argv 未指定なら
        ``sys.argv[1:]`` を使う．先頭が ``-h/--help`` または空なら argparse の
        ヘルプを表示し，それ以外は ``argv[0]`` を対応サブコマンドへルーティングして
        ``func(argv[1:])`` を呼ぶ．未知のサブコマンドは argparse のエラーに委ねる．
    """

    def main(argv: list[str] | None = None) -> None:
        parser = argparse.ArgumentParser(prog=prog, description=description)
        subparsers = parser.add_subparsers(dest="command", required=True)
        for name, (help_text, _target) in subcommands.items():
            subparsers.add_parser(name, help=help_text, add_help=False)

        argv = sys.argv[1:] if argv is None else argv
        if not argv or argv[0] in {"-h", "--help"}:
            parser.parse_args(argv)
            return

        command = argv[0]
        rest = argv[1:]
        if command in subcommands:
            _help_text, target = subcommands[command]
            run_main = _resolve_target(target)
            run_main(rest)
        else:
            # 未知のコマンドは argparse のエラーメッセージに委ねる
            parser.parse_args(argv)

    return main


def _demo_main(argv: list[str] | None = None) -> None:
    """``socsim-tools`` スタンドアロン CLI (デモ用)．

    本パッケージは主に各 replication の ``cli.py`` から import して使うライブラリ．
    このエントリポイントはパッケージ情報の表示のみを行う．
    """
    print(
        "socsim-tools: shared dispatcher + settings renderer + results I/O for "
        "socsim replication tools.\n"
        "主にライブラリとして利用します: "
        "from socsim_tools.cli import build_dispatcher"
    )


if __name__ == "__main__":
    _demo_main()
