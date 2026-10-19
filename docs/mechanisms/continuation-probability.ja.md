[English](continuation-probability.md) | **日本語**

# 継続確率（`continuation_probability`）

> 各ステップの完了後，確率 δ で次へ進み，確率 1 − δ で停止します．
> **フェーズ：** PostStep．**出典：** 無期限繰り返しゲーム／確率的寿命の標準的な規約．
> **種別：** 確率的な実行制御．

[← Mechanism カタログに戻る](../mechanisms.ja.md)

## 1. 概要

`ContinuationProbability` は `socsim-mechanisms` の汎用メカニズムで，どの
`WorldState` にも使えます．各ステップの完了後に一様乱数 $u$ を 1 つ引き，
$u \geq \delta$ なら `StepContext::request_stop()` を呼びます．そのため現在の
ステップは必ず最後まで実行され，次のステップへ進む確率は正確に δ です．

確率による停止までに完了するステップ数は，$1,2,\ldots$ 上の幾何分布に従います．

$$
P(T=t) = \delta^{t-1}(1-\delta),
\qquad E[T] = \frac{1}{1-\delta}\quad (0 \leq \delta < 1).
$$

時計の `t_max` は引き続き上限なので，実際の実行ステップ数は `min(T, t_max)` です．
δ = 0 ではステップ 1 の後で停止し，δ = 1 では停止を要求しません．

## 2. 6フェーズループにおける位置

**PostStep** で動きます．したがって Environment，Decision，Interaction，Reward に
おける現在のステップの処理はすべて完了してから，次のラウンドがあるかを決めます．
同じ PostStep のメカニズムは追加順に動きます．停止したステップも記録するには，
`game_metrics` などの記録用メカニズムより後に追加してください．normal-form の
Scenario factory はこの順序で自動的に追加します．

## 3. 状態の読み書きコントラクト

| 状態 | 読み取り | 書き込み | 備考 |
|---|:--:|:--:|---|
| `ctx.clock.t()` | ✓ | | 専用ストリームの位置を決め，イベントの時刻に使う． |
| 停止フラグ | | ✓ | $u \geq \delta$ のときだけ `request_stop()` で設定する． |
| Recorder | | ✓ | 停止したステップに `continuation_stop` を記録する． |
| World のドメイン状態 | | | 読み書きしない． |
| `ctx.rng` | | | 消費しない． |

イベントの payload は `{"delta": <δ>, "u": <乱数>}` です．イベントの時刻 `t` が
停止した完了済みステップを表します．

## 4. パラメータ

| パラメータ | 型 | 既定値 | 意味 |
|---|---|---|---|
| `delta` | `[0, 1]` の有限な `f64` | なし | 現在のステップの後に次がある確率． |
| `seed` | `u64` | なし | 専用ストリームを導く実行のルート seed． |

`ContinuationProbability::new(delta, seed)` は，範囲外，NaN，無限大に対して
設定エラーを返します．

## 5. ライブラリでの利用

ほかのストリームを導くときと同じ実行のルート seed を渡します．エンジンには従来どおり
派生した seed を渡せます．

```rust,ignore
use socsim_core::derive_seed;
use socsim_engine::SimulationBuilder;
use socsim_mechanisms::ContinuationProbability;

let seed = 42;
let continuation = ContinuationProbability::new(0.9, seed)?;
let mut simulation = SimulationBuilder::new(world)
    .seed(derive_seed(seed, &[1]))
    .add_mechanism(Box::new(domain_mechanism))
    .add_mechanism(Box::new(continuation))
    .build();
simulation.run()?;
```

## 6. normal-form Scenario での利用

Scenario TOML から使えるのは `normal-form` パックだけです．

```toml
[[mechanism]]
name = "continuation_probability"
[mechanism.params]
delta = 0.9
```

これは独自のイベントと乱数ストリームを持つ，任意に追加できる PostStep メカニズムです．
そのため `[simulation]` のキーではなく `[[mechanism]]` の項目にしています．ほかの
パックには登録しません．

## 7. 決定論性と乱数ストリームの分離

ルート seed と固定の 64 bit ラベル `0x636f6e74696e7565`（`continue` のバイト列）から
専用の `SimRng` を導きます．完了したステップごとに一様乱数を正確に 1 つ引き，
`ctx.rng` は使いません．したがって同じ seed では，このメカニズムを足した実行と
足さない実行の World の軌跡および通常の記録は，停止したステップまでビット単位で一致します．

## 8. snapshot

エンジンの snapshot は World の時計，共有 RNG，停止フラグを保存しますが，Mechanism
オブジェクトは保存しません．そこで専用 RNG と消費済み乱数の個数を `clock.t()` に同期します．
新しく組み直したメカニズムは保存時点まで専用ストリームを進め，古い snapshot へ戻した場合は
派生 seed からストリームを作り直して同じ位置まで進めます．元の実行と同じルート seed を使い，
各ステップでこのメカニズムを 1 回動かしてください．

## 9. 繰り返しゲームと未来の影

毎ラウンドの利得列を考えると，ラウンド $t$ に到達する確率は $\delta^{t-1}$ です．
したがって期待される生涯利得は，通常の δ 割引による繰り返しゲームの利得と同じ重みを持ちます．
δ が大きいほど将来の結果の重みが増え，「未来の影」が長くなります．

## 10. テストで確かめること

同じ seed で停止ステップが一致すること，δ = 0 と δ = 1，無効な確率，幾何分布の平均寿命，
専用ストリームによる前方一致，snapshot からの再開を検査します．前方一致には確率的なゲーム規則を
使うと，共有ストリームを誤って消費した場合の差をすぐ検出できます．
