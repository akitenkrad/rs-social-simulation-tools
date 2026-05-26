[English](00-getting-started.md) | **日本語**

# T0 — はじめに

**作るもの：** Rustコードは書きません — `socsim` CLIをひととおり動かします：同梱の2つのシナリオを実行し，利用可能なものを一覧表示し，新しいシナリオの雛形を生成し，パラメータスイープを実行します．
**所要時間：** 15分．

## 前提

- Rustツールチェーン（`cargo`） — バイナリのビルドに1度だけ使います．
- Rustの知識は不要です．コマンドを実行するだけです．

バイナリを1度ビルドします：

```sh
cargo build --release
```

これで `target/release/socsim` が生成されます．以下のコマンドはこのバイナリがパス上にある前提です（または `./target/release/socsim` で呼び出してください）．

## ステップ

### 1. HRライフサイクルのベースラインを実行する

socsimは [`scenarios/`](../../scenarios) に2つのすぐ実行できるシナリオを同梱しています．較正済みのHRライフサイクルを実行します：

```sh
socsim run scenarios/hr_lifecycle_baseline.toml
```

ステップごとのメトリクス表が出力されます．各列はシナリオが要求したメトリクス（`[output] metrics`）です：

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

ここで2つの概念が登場します．**シナリオ** は，**モジュールパック**（ここでは `hr-lifecycle`），シード，ステップ数，メカニズムのリストを指定する `.toml` ファイルです．**メトリクス** は実行が各ステップで記録する数値系列です．

### 2. 利用可能なもの：パックとメカニズム

*パック* はCLIが実行できる，名前付きのメカニズムの束です．一覧表示します：

```sh
socsim list packs
```

```
Available module packs:
  hr-lifecycle
  opinion-dynamics
```

各パックが登録するメカニズム：

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
  [opinion-dynamics]
    convergence
    deffuant
    hegselmann_krause
    lorenz
    opinion_metrics
    social_judgement
```

シナリオの `[[mechanism]]` ブロックは，そのパックのメカニズムしか指定できません．

### 3. opinion-dynamics シナリオを実行しクラスタが減るのを見る

```sh
socsim run scenarios/opinion_dynamics_baseline.toml
```

```
Running 'opinion_dynamics_baseline' (pack=opinion-dynamics, t_max=60, seeds=[42], parallel=false)

Seed 42 — 0 events recorded

t               clusters         max_delta              mean            spread          variance
10               22.0000            0.1238            0.5092            0.9769            0.0360
20               18.0000            0.0331            0.5088            0.9769            0.0268
30               15.0000            0.0127            0.5094            0.9769            0.0243
40               12.0000            0.0049            0.5097            0.9769            0.0235
50               12.0000            0.0021            0.5098            0.9769            0.0233
60               12.0000            0.0010            0.5098            0.9769            0.0232
```

`clusters` 列が減っていく（22 → 12）のを見てください：有界信頼の下では，意見が十分に近いエージェントが収束するため，異なる意見クラスタが時間とともに統合されます．`max_delta` 列がゼロへ縮むのは，系が落ち着いていることを示します．

### 4. `init` で新しいシナリオの雛形を作る

シナリオをゼロから書く必要はありません — `init` が任意のパックの雛形を出力します：

```sh
socsim init --module-pack opinion-dynamics --out scenarios/my_opinion.toml
```

```
Wrote starter scenario to 'scenarios/my_opinion.toml'
```

ファイルを開き，`epsilon`（信頼半径）を `0.4` に上げてから `socsim run scenarios/my_opinion.toml` を実行してみてください — `epsilon` が大きいほどエージェントは単一クラスタ（完全な合意）へ向かいます．

### 5. パラメータをスイープする

「パラメータPを変えると結果Xはどう変わるか」を問うには `sweep` を使います．パラメータ値の直積を，各シード範囲にわたって実行します：

```sh
socsim sweep scenarios/hr_lifecycle_baseline.toml \
    --param "toxic_spread.p_spread=0.2,0.7" \
    --seeds 0..2
```

```
Sweeping 'hr_lifecycle_baseline' over 1 axes × 2 seeds
  toxic_spread.p_spread = [0.2, 0.7]
  combo 0: toxic_spread.p_spread=0.2000
metric                      mean         std         min         max      n
------------------------------------------------------------------------
avg_tenure               35.3000      6.2000     29.1000     41.5000      2
knowledge_stock          91.5906      5.6123     85.9783     97.2030      2
org_performance          40.0188      2.2088     37.8100     42.2276      2
turnover_rate             0.0125      0.0125      0.0000      0.0250      2
  combo 1: toxic_spread.p_spread=0.7000
...
Wrote 2 CSV files to 'runs/sweep'
```

各組合せのシード横断サマリが出力され，`runs/sweep/` 以下にCSVとしても書き出されます．

## 実行する

ここまで使った4つのコマンドを順に：

```sh
socsim run scenarios/hr_lifecycle_baseline.toml
socsim list packs
socsim run scenarios/opinion_dynamics_baseline.toml
socsim init --module-pack opinion-dynamics --out scenarios/my_opinion.toml
socsim sweep scenarios/hr_lifecycle_baseline.toml --param "toxic_spread.p_spread=0.2,0.7" --seeds 0..2
```

## 学んだこと

- **シナリオ**（`.toml`）は **モジュールパック** を選び，その **メカニズム** を構成します．**メトリクス** は記録されるステップごとの系列です．
- `socsim list packs` / `list mechanisms` で構成可能なものが分かります．
- 有界信頼の意見は収束します — 時間とともにクラスタが減ります．
- `socsim init` はシナリオの雛形を作り，`socsim sweep` はパラメータが結果をどう動かすかを調べます．
- 各サブコマンドの全フラグは [CLIリファレンス](../cli.ja.md) にあります．

## 次へ

[T1 — 最初のモデル](01-first-model.ja.md)：TOMLをやめ，`WorldState` と `Mechanism` を1つずつRustでモデルを作ります．
