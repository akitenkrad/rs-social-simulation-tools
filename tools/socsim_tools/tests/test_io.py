"""resolve_results_dir / load_json / load_config / load_run_metadata テスト．"""

from __future__ import annotations

import json
import os
from pathlib import Path

import pytest

from socsim_tools.io import (
    load_config,
    load_json,
    load_run_metadata,
    resolve_results_dir,
)


def test_resolve_explicit_dir(tmp_path, monkeypatch):
    monkeypatch.chdir(tmp_path)
    target = tmp_path / "results" / "20260101_000000"
    target.mkdir(parents=True)
    resolved = resolve_results_dir("results/20260101_000000")
    assert resolved == Path(os.path.realpath(target))


def test_resolve_latest_symlink(tmp_path, monkeypatch):
    monkeypatch.chdir(tmp_path)
    base = tmp_path / "results"
    real = base / "20260101_120000"
    real.mkdir(parents=True)
    latest = base / "latest"
    latest.symlink_to(real)
    resolved = resolve_results_dir(None)
    assert resolved == Path(os.path.realpath(real))


def test_resolve_newest_by_name(tmp_path, monkeypatch):
    monkeypatch.chdir(tmp_path)
    base = tmp_path / "results"
    (base / "20260101_000000").mkdir(parents=True)
    (base / "20260301_000000").mkdir(parents=True)
    (base / "20260201_000000").mkdir(parents=True)
    resolved = resolve_results_dir(None)
    assert resolved.name == "20260301_000000"


def test_load_json_roundtrip(tmp_path):
    p = tmp_path / "x.json"
    p.write_text(json.dumps({"a": 1, "b": "x"}), encoding="utf-8")
    assert load_json(p) == {"a": 1, "b": "x"}


def test_load_config_run(tmp_path):
    rd = tmp_path / "run1"
    rd.mkdir()
    (rd / "config.json").write_text(json.dumps({"n_agents": 4}), encoding="utf-8")
    cfg, src = load_config(rd)
    assert cfg == {"n_agents": 4}
    assert src == rd / "config.json"


def test_load_config_sweep(tmp_path):
    rd = tmp_path / "sweep1"
    rd.mkdir()
    (rd / "sweep_config.json").write_text(json.dumps({"runs": 3}), encoding="utf-8")
    cfg, src = load_config(rd)
    assert cfg == {"runs": 3}
    assert src.name == "sweep_config.json"


def test_load_config_missing(tmp_path):
    rd = tmp_path / "empty"
    rd.mkdir()
    with pytest.raises(FileNotFoundError):
        load_config(rd)


def test_load_run_metadata_variants(tmp_path):
    rd = tmp_path / "m"
    rd.mkdir()
    assert load_run_metadata(rd) is None
    (rd / "llm_meta.json").write_text(json.dumps({"llm_model": "x"}), encoding="utf-8")
    assert load_run_metadata(rd) == {"llm_model": "x"}
    # run_metadata.json takes priority
    (rd / "run_metadata.json").write_text(
        json.dumps({"llm_model": "y"}), encoding="utf-8"
    )
    assert load_run_metadata(rd) == {"llm_model": "y"}
