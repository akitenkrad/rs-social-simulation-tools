"""build_dispatcher のルーティング / --help テスト (ネットワーク不要)．"""

from __future__ import annotations

import sys
import types

import pytest

from socsim_tools.cli import build_dispatcher


def _install_fake_module(monkeypatch, name: str, recorder: list):
    mod = types.ModuleType(name)

    def fake_main(rest):
        recorder.append(rest)

    mod.main = fake_main  # type: ignore[attr-defined]
    monkeypatch.setitem(sys.modules, name, mod)


def test_routes_to_target_main(monkeypatch):
    calls: list = []
    _install_fake_module(monkeypatch, "fake_pkg_viz", calls)
    main = build_dispatcher(
        prog="x-tools",
        description="desc",
        subcommands={"visualize": ("viz help", "fake_pkg_viz:main")},
    )

    main(["visualize", "--foo", "bar"])
    assert calls == [["--foo", "bar"]]


def test_help_does_not_route(monkeypatch, capsys):
    calls: list = []
    _install_fake_module(monkeypatch, "fake_pkg_help", calls)
    main = build_dispatcher(
        prog="x-tools",
        description="desc",
        subcommands={"visualize": ("viz help", "fake_pkg_help:main")},
    )

    # -h delegates to argparse, which prints help and exits(0) without routing
    with pytest.raises(SystemExit) as exc:
        main(["-h"])
    assert exc.value.code == 0
    out = capsys.readouterr().out
    assert "x-tools" in out
    assert calls == []


def test_empty_argv_delegates_to_argparse(monkeypatch, capsys):
    main = build_dispatcher(
        prog="x-tools",
        description="desc",
        subcommands={"visualize": ("viz help", "fake_pkg_empty:main")},
    )
    # empty argv: required subcommand missing -> argparse errors (exit 2)
    with pytest.raises(SystemExit) as exc:
        main([])
    assert exc.value.code == 2
    err = capsys.readouterr().err
    assert "x-tools" in err


def test_unknown_subcommand_errors(monkeypatch):
    main = build_dispatcher(
        prog="x-tools",
        description="desc",
        subcommands={"visualize": ("viz help", "fake_pkg_unknown:main")},
    )
    with pytest.raises(SystemExit):
        main(["nope"])


def test_bad_target_raises(monkeypatch):
    calls: list = []
    _install_fake_module(monkeypatch, "fake_pkg_bad", calls)
    main = build_dispatcher(
        prog="x-tools",
        description="desc",
        subcommands={"visualize": ("viz help", "no_colon_target")},
    )
    with pytest.raises(ValueError):
        main(["visualize"])
