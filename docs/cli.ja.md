[English](cli.md) | **日本語**

# CLIリファレンス

`socsim` バイナリは6つのサブコマンドを提供します．`socsim --help` または `socsim <COMMAND> --help` でフラグを確認できます．

```
socsim <COMMAND>

Commands:
  init       Generate a starter scenario TOML for a module pack
  run        Run a scenario (single seed or multi-seed)
  validate   Validate a scenario TOML against the pack's registry
  list       List available module packs or mechanisms
  sweep      Run a grid parameter sweep
  summarize  Re-aggregate existing JSONL run logs into a summary
  help       Print this message or the help of the given subcommand(s)
```

![CLI workflow](assets/cli-workflow.svg)

---

## init

モジュールパック向けのスターターシナリオTOMLを生成します．

```
socsim init --module-pack <MODULE_PACK> --out <OUT>
```

| フラグ | 説明 |
|---|---|
| `--module-pack` | パック名（例：`hr-lifecycle`） |
| `-o, --out` | 出力ファイルパス |

**例**

```sh
socsim init --module-pack hr-lifecycle --out scenarios/my_scenario.toml
```

出力：

```
Wrote starter scenario to 'scenarios/my_scenario.toml'
```

---

## run

シナリオTOMLを1つのシードまたは複数のシードで実行します．

```
socsim run [OPTIONS] <SCENARIO>
```

| フラグ | デフォルト | 説明 |
|---|---|---|
| `--seeds <A..B>` | シナリオのseed | シードの範囲（上限は排他） |
| `--parallel` | false | Rayonを使ったシードの並列実行 |

**単一シード実行**（TOMLに記載のシードを使用）：

```sh
socsim run scenarios/hr_lifecycle_baseline.toml
```

出力：

```
Running 'hr_lifecycle_baseline' (pack=hr-lifecycle, t_max=60, seeds=[42], parallel=false)

Seed 42 — 82 events recorded

t             avg_tenure   knowledge_stock   org_performance     turnover_rate
10                9.1000           53.9517           32.1462            0.0000
20               14.6000           62.4468           35.7133            0.0000
30               21.5500           72.5042           40.4270            0.0250
40               25.9000           78.4727           40.2186            0.0000
50               30.0750           85.3493           40.8007            0.0000
60               35.6250           92.3841           41.8100            0.0000
```

**マルチシード実行**では，ステップ系列の代わりにシード間サマリーテーブルが出力されます：

```sh
socsim run scenarios/hr_lifecycle_baseline.toml --seeds 0..3
```

出力：

```
Running 'hr_lifecycle_baseline' (pack=hr-lifecycle, t_max=60, seeds=[0, 1, 2], parallel=false)

Cross-seed summary (3 seeds):

metric                      mean         std         min         max      n
------------------------------------------------------------------------
avg_tenure               35.8000      0.5319     35.3750     36.5500      3
knowledge_stock          92.6772      1.2340     91.1426     94.1641      3
org_performance          42.8467      1.4574     40.7856     43.8800      3
turnover_rate             0.0083      0.0118      0.0000      0.0250      3
```

各実行後，シナリオTOMLの `output.log_path` で指定されたJSONLログファイルが書き出されます（後述の[JSONL出力形式](#jsonl出力形式)を参照）．

---

## validate

シナリオTOML内のすべてのメカニズム名が指定のパックに登録されているか，またスケジューラーと `t_max` が有効かを確認します．

```
socsim validate <SCENARIO>
```

**例**

```sh
socsim validate scenarios/hr_lifecycle_baseline.toml
# OK — scenario 'scenarios/hr_lifecycle_baseline.toml' is valid.
```

---

## list

利用可能なモジュールパック，または各パック内のメカニズムを一覧表示します．

```
socsim list <WHAT>
```

`<WHAT>` は `packs` または `mechanisms` のいずれかです．

**例**

```sh
socsim list packs
```

```
Available module packs:
  hr-lifecycle
```

```sh
socsim list mechanisms
```

```
Mechanisms by pack:
  [hr-lifecycle]
    fit
    hiring
    knowledge_loss
    learning_curve
    ocb
    org_performance
    peer_effect
    socialization
    toxic_spread
    turnover
```

---

## sweep

1つ以上のパラメータ軸のデカルト積に対してグリッドパラメータスイープを実行します．

```
socsim sweep [OPTIONS] <SCENARIO>
```

| フラグ | デフォルト | 説明 |
|---|---|---|
| `--param <MECH.PARAM=V1,V2,...>` | — | スイープ軸（多次元スイープは繰り返し指定） |
| `--seeds <A..B>` | `0..5` | 各組み合わせのシード範囲 |
| `-o, --out <DIR>` | `runs/sweep` | 組み合わせごとのCSVの出力ディレクトリ |
| `--parallel` | false | 各組み合わせ内のシードを並列実行 |

**例** — `toxic_spread.p_spread` が結果に与える影響を調査：

```sh
socsim sweep scenarios/hr_lifecycle_baseline.toml \
    --param "toxic_spread.p_spread=0.2,0.46,0.7" \
    --seeds 0..3
```

出力（抜粋）：

```
Sweeping 'hr_lifecycle_baseline' over 1 axes × 3 seeds
  toxic_spread.p_spread = [0.2, 0.46, 0.7]
  combo 0: toxic_spread.p_spread=0.2000
metric                      mean         std         min         max      n
------------------------------------------------------------------------
avg_tenure               35.3250      5.0624     29.1000     41.5000      3
...
  combo 2: toxic_spread.p_spread=0.7000
...
Wrote 3 CSV files to 'runs/sweep'
```

各CSVファイルは `combo_<N>_<param>=<value>.csv` という名前で，`key,mean,std,min,max,n` の列を持ちます．

**多次元スイープ** — 軸ごとに `--param` を追加：

```sh
socsim sweep scenarios/hr_lifecycle_baseline.toml \
    --param "peer_effect.alpha_peer=0.1,0.17,0.3" \
    --param "turnover.quit_cascade_bump=0.1,0.3,0.5" \
    --seeds 0..10 --parallel
```

---

## summarize

シミュレーションを再実行せずに，既存のJSONLログファイルをサマリー統計に集計し直します．

```
socsim summarize [OPTIONS] <PATH>
```

| フラグ | デフォルト | 説明 |
|---|---|---|
| `--format <csv\|json>` | `csv` | 出力形式 |

`<PATH>` は単一の `.jsonl` ファイルまたはディレクトリです．ディレクトリの場合は `*.jsonl` ファイルを非再帰的にスキャンします．

**例**

```sh
socsim summarize runs/hr_lifecycle_baseline_42.jsonl
```

```
key,mean,std,min,max,n
avg_tenure,35.625000,0.000000,35.625000,35.625000,1
knowledge_stock,92.384145,0.000000,92.384145,92.384145,1
org_performance,41.810021,0.000000,41.810021,41.810021,1
turnover_rate,0.000000,0.000000,0.000000,0.000000,1
```

```sh
socsim summarize runs/ --format json
```

---

## JSONL出力形式

各 `socsim run` はシードごとに1つのJSONLファイルを書き出します．パスはシナリオTOMLの `output.log_path` で制御され，2つの置換トークンをサポートします：

| トークン | 置換内容 |
|---|---|
| `{name}` | `simulation.name` |
| `{seed}` | そのトライアルの整数シード値 |

テンプレート例：`"runs/{name}_{seed}.jsonl"` → `runs/hr_lifecycle_baseline_42.jsonl`

JSONLファイルの各行は独立したJSONオブジェクトです．2種類のレコードが出力されます：

**メトリクスレコード**

```json
{"type":"metric","t":1,"key":"turnover_rate","value":0.0}
```

**イベントレコード**

```json
{"type":"event","t":3,"kind":"turnover","payload":{"agent":7,"team":2}}
```

各タイプのフィールドは全レコードで統一されており，`payload` はメカニズム固有の内容になります．
