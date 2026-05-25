"""render_run_metadata (バイト等価) / render_run_config / main の動作テスト．"""

from __future__ import annotations

import json

from socsim_tools.settings import (
    render_run_config,
    render_run_metadata,
    show_experiment_settings_main,
)

SAMPLE_META = {
    "llm_model": "llama3.2:latest",
    "llm_endpoint": "http://localhost:11434",
    "llm_temperature": 0.0,
    "llm_seed": 42,
    "total_calls": 12,
    "cache_hits": 12,
    "cache_hit_rate": 1.0,
}


def _reference_render_run_metadata(meta: dict) -> str:
    """chuang2024 の render_run_metadata を逐語コピーした参照実装 (回帰固定)．"""
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


def test_render_run_metadata_byte_identical():
    assert render_run_metadata(SAMPLE_META) == _reference_render_run_metadata(SAMPLE_META)


def test_render_run_metadata_key_lines():
    out = render_run_metadata(SAMPLE_META)
    assert "LLM 実行メタデータ (run_metadata.json)" in out
    assert "モデル           : llama3.2:latest" in out
    assert "cache-hit 率     : 100.0%" in out
    assert out.endswith("=" * 70)


def test_render_run_metadata_with_note():
    meta = dict(SAMPLE_META, determinism_note="二層擬似決定論")
    out = render_run_metadata(meta)
    assert "注記: 二層擬似決定論" in out


def test_render_run_config_uses_field_labels():
    cfg = {"n_agents": 4, "topic": "AI"}
    labels = {"n_agents": "エージェント数 N ", "topic": "トピック         "}
    out = render_run_config(cfg, "results/x/config.json", labels)
    assert "実行設定 (run)" in out
    assert "エージェント数 N : 4" in out
    assert "トピック         : AI" in out
    assert "設定ファイル: results/x/config.json" in out
    # missing keys not in labels are not rendered
    assert "framing" not in out


def test_render_run_config_missing_value_dash():
    out = render_run_config({}, "src", {"k": "ラベル"})
    assert "ラベル: -" in out


def test_show_experiment_settings_main_text(tmp_path, monkeypatch, capsys):
    monkeypatch.chdir(tmp_path)
    rd = tmp_path / "results" / "run1"
    rd.mkdir(parents=True)
    (rd / "config.json").write_text(json.dumps({"n_agents": 4}), encoding="utf-8")
    (rd / "run_metadata.json").write_text(json.dumps(SAMPLE_META), encoding="utf-8")

    rc = show_experiment_settings_main(
        ["--results-dir", "results/run1"],
        field_labels={"n_agents": "エージェント数 N "},
    )
    assert rc == 0
    out = capsys.readouterr().out
    assert "エージェント数 N : 4" in out
    assert "cache-hit 率     : 100.0%" in out


def test_show_experiment_settings_main_json(tmp_path, monkeypatch, capsys):
    monkeypatch.chdir(tmp_path)
    rd = tmp_path / "results" / "run1"
    rd.mkdir(parents=True)
    (rd / "config.json").write_text(json.dumps({"n_agents": 4}), encoding="utf-8")

    rc = show_experiment_settings_main(
        ["--results-dir", "results/run1", "--json"],
        field_labels={"n_agents": "N"},
    )
    assert rc == 0
    payload = json.loads(capsys.readouterr().out)
    assert payload["config"] == {"n_agents": 4}
    assert payload["run_metadata"] is None


def test_show_experiment_settings_main_missing_dir(monkeypatch, capsys):
    rc = show_experiment_settings_main(
        ["--results-dir", "/nonexistent/path/xyz"],
        field_labels={"k": "ラベル"},
    )
    assert rc == 1
