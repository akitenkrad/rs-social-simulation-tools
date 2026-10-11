[English](game-metrics.md) | **日本語**

# ゲームの指標（`game_metrics`）

> ステップごとのゲームの統計 — 行動の割合，利得，協力，後悔，均衡までの距離，公開集計値 — を 1 行記録し，対戦が落ち着いたら実行を終えられます．
> **フェーズ：** PostStep．**出典：** ゲーム理論の要約統計．**種別：** observation．

[← Mechanism カタログに戻る](../mechanisms.ja.md)

## 1. 概要

`game_metrics`（`GameMetrics<G>`，`G: FiniteGame`）は [`socsim-games`](../games.ja.md) の観察用の Mechanism です．
ほかの `game_*` と違って `assemble` は**登録しません**．`GameMetrics::new(&game, MetricsOptions)` で作り，ビルダーに足します．
毎ステップの後に次を行います．

1. 対戦の表にあるそのステップの席を行動ごとに数え，`game.played_share.<id>` を書きます
   （協力のラベルを設定していれば `game.coop_rate` も，feature `metrics` では `game.action_entropy` も書きます）．
2. 対戦したエージェントについて `game.mean_payoff` と `game.payoff_sd` を書きます．
3. `assemble_with_aggregates` で組み立てたときは，そのステップの公開集計値の行を `game.aggregate.<name>` として写します．
4. 設定した `MixedSource` から役割ごとに 1 つの混合戦略を作り，`game.regret` と，`EquilibriumTarget::Solve` なら `game.eq_distance` を書きます．
5. 収束のカウンターを更新し，割合が `window` ステップ安定したら実行の終了を求めます．

そのステップで値が定義されない列は行から省き，NaN を記録することはありません．
列の詳しい説明は [`docs/games.ja.md` の「指標」](../games.ja.md#指標) にあります．

## 2. 理論と出典

ステップ $t$ の席を $S_t$，席 $s$ の行動を $a_s$，その役割を $r(s)$ とすると，打たれた行動の割合は次のとおりです．

$$p_t(k) = \frac{\lvert\{s \in S_t : a_s = k\}\rvert}{\lvert S_t\rvert} \ \ (\text{pooled ids}), \qquad p_t(r, k) = \frac{\lvert\{s \in S_t : r(s) = r,\ a_s = k\}\rvert}{\lvert\{s \in S_t : r(s) = r\}\rvert} \ \ (\text{per-role ids})$$

役割ごとに 1 つの混合戦略からなる組 $\sigma = (\sigma_r)_r$ について，$F_r$ を役割 $r$ の期待利得とすると，

$$\text{regret}(\sigma) = \max_r \Big[\max_a F_r(a, \sigma_{-r}) - F_r(\sigma)\Big], \qquad \text{eq\_distance}(\sigma) = \min_{\sigma^* \in E} \sum_r \sum_a \big\lvert \sigma_r(a) - \sigma^*_r(a)\big\rvert$$

- 後悔はちょうど Nash 均衡（Nash, 1951）で 0 になります．ゲームにおける学習の文献で広く使われる，一方的な逸脱による利得の増分です
  （例：Hart & Mas-Colell, 2000）．
- $E$ は 2 人ゲームの完全な均衡の集合で，サポート列挙で 1 回だけ求めます．
- `game.action_entropy` は Shannon（1948）のエントロピー $-\sum_k p_t(k) \ln p_t(k)$ で，単位は nat です．

## 3. データフロー

![game_metrics data flow](../assets/mech-game-metrics.svg)

対戦の表（`seats`，`actions`），エージェントごとの `payoff` と `n_played`，最後の集計値の行，`MixedSource::Reported` なら混合戦略の報告を読みます．
Recorder の表 `"game"` に 1 行を書き，`stop` か `MixedSource::PlayedCumulative` を設定したときだけ，World に自分の `MetricsState` を書きます．

## 4. 6フェーズループにおける位置

**PostStep** に登録されます．その時点で `game_play`（Interaction）が利得と集計値の行を埋め，
`game_decide`（Reward）が学習後の報告を書いているので，終わったステップを見ます．
ゲームの Mechanism はその出力を読みません．PostStep でこれより後に足した Mechanism は，これより後に動きます．

## 5. 状態の読み書きコントラクト

| フィールド | 読み | 書き | 備考 |
|---|:--:|:--:|---|
| `PlayArena.seats`，`actions` | ✓ | | 席の役割は席の番号を役割数で割った余り．行動はすべて `Some` で，その役割の範囲内でなければならない． |
| `GameWorld.payoff`，`n_played` | ✓ | | `mean_payoff` / `payoff_sd` に入るのは `n_played > 0` のエージェントだけ． |
| `game` | ✓ | | 役割数と行動数を `new` に渡したゲームと照合する．利得は後悔に使う． |
| 集計値の表 | ✓ | | 最後の行だけ．その `t` がこのステップのときだけ． |
| 混合戦略の報告，`PolicyTable` のスロット | ✓ | | `MixedSource::Reported` のときだけ．報告はこのステップのものでなければならない．`Held` の席は純粋戦略として数える． |
| `MetricsState { prev, below, cumulative }` | ✓ | ✓ | `stop` か `PlayedCumulative` を設定したとき，最初のステップで作る．World のフィールド `metrics_state`（それ以外では World の JSON から省かれる）．snapshot と再開を越えて残る． |
| Recorder の表 `"game"` | | ✓ | ステップごとに `record_row` を 1 回． |
| 停止の要求 | | ✓ | 収束のカウンターが `window` に達したとき `StepContext::request_stop`． |

規則の状態，保持行動，対戦，利得は書きません．`game_metrics` を足しても実行の軌跡は変わりません．

## 6. 依存関係と順序制約

- **上流：** `game_match` / `game_decide` / `game_play` が動いていること（どの `assemble*` 関数でもよい）．
  `game.aggregate.<name>` には `assemble_with_aggregates` が要ります．`MixedSource::Reported` には `PlayOptions::report_mixed(true)` が要り
  （無ければ最初のステップが失敗します），席に着いた規則で動くエージェントがすべて混合を報告しなければなりません
  （`Fixed`，`MyopicBestResponse`，LLM の種類，`mixed` を持たない規則は報告しません）．
- **同じゲーム：** World に入れたゲームから作ってください．役割数と行動数は毎ステップ照合します（違えば Mechanism のエラー）．
  `eq_distance` の均衡は `new` に渡したゲームから求めます．
- **シミュレーションに 1 つ：** World の `MetricsState` は 1 つなので，`game_metrics` は 1 つだけ足してください．
- **下流：** ゲームの Mechanism はその出力を読みません．`stop` では，停止を求めたステップの後でエンジンが実行を終えます．

## 7. パラメータ

| パラメータ | 既定値 | 種別 | 備考 |
|---|---|---|---|
| `cooperate_label(label)` | なし | observation | `game.coop_rate` を足す．全役割でちょうど 1 つの行動のラベルに一致しなければならない（`GameError::Metrics`）． |
| `regret(on)` | ゲームが 2 役割のときだけ有効 | observation | `game.regret`． |
| `mixed_source(MixedSource)` | `Played` | observation | `Played`（このステップの席），`PlayedCumulative`（最初のステップ以降の全席．World に保持），`Reported`（学習後の報告の席で重み付けした平均）． |
| `equilibrium(EquilibriumTarget)` | `None` | observation | `Solve`：`new` の中でサポート列挙を 1 回．2 役割のみ．ソルバーの失敗，均衡の集合が不完全または空なら拒否．`game.eq_distance` を足す． |
| `stop(tol, window)` | なし | control | 席のあるステップで割合の最大変化が `< tol` のものが `window` 回続いたら停止．`tol` は有限で `> 0`，`window ≥ 1`． |
| `action_entropy(on)` | 有効 | observation | feature `metrics` のときだけ．共通 ID のときだけ． |

## 8. 適用方法

ライブラリ専用です．Mechanism を作り，`assemble`（または `assemble_with_aggregates` / `assemble_with_llm`）が返すビルダーに足します．

### ライブラリモード

```rust
use socsim_games::{EquilibriumTarget, GameMetrics, MetricsOptions, MixedSource};

let metrics = GameMetrics::new(           // before `game` moves into build_world
    &game,
    MetricsOptions::new()
        .cooperate_label("C")
        .mixed_source(MixedSource::PlayedCumulative)
        .equilibrium(EquilibriumTarget::Solve)
        .stop(1e-6, 20),
)?;
let columns = metrics.describe();         // (pattern, meaning) pairs for runvault.toml
let world = build_world(game, 2, (), InitialActions::Same(0), &rules, &Assignment::Ordered(vec![
    AssignGroup { kind: AssignKind::Rule("tit_for_tat".into()), count: 1 },
    AssignGroup { kind: AssignKind::Rule("fixed_defect".into()), count: 1 },
]), 100, 1000, seed)?;
let mut sim = assemble(world, matching, rules, NormalFormOutcome, PlayOptions::new(), seed)?
    .add_mechanism(Box::new(metrics))     // PostStep, after the game mechanisms
    .recorder(Box::new(recorder))
    .build();
sim.run()?;                               // may end before t_max via the stop rule
```

Tit-for-Tat と裏切る相手では，この実行は `t = 22` で終わります（割合は `t = 2` から変わらず，`t = 3` から 20 ステップ安定する）．
`describe()` はこの設定の列を並べます．runvault の宣言へ写す方法は
[`docs/games.ja.md` の「列の説明」](../games.ja.md#列の説明describe) を参照してください．

## 9. 決定論性と RNG

乱数を**引かず**，モデルの状態を書かないので，`game_metrics` を足した実行は，足さない同じ実行と全く同じ軌跡をたどります．
行は終わったステップと World の一部である `MetricsState` だけで決まるので，再開した実行は通しの実行と同じ値を記録し，同じステップで止まります．

## 10. 期待される動作

$(T, R, P, S) = (5, 3, 1, 0)$ の囚人のジレンマで，Tit-for-Tat と常に裏切る相手を，`cooperate_label("C")`，`EquilibriumTarget::Solve`，`stop(1e-9, 3)` で走らせると，

- `t = 1`：`game.played_share.a0 = 0.5`，`game.played_share.a1 = 0.5`，`game.coop_rate = 0.5`，`game.mean_payoff = 2.5`，`game.payoff_sd = 2.5`，
  `game.regret = 1`，`game.eq_distance = 2`．
- `t ≥ 2`：`played_share.a1 = 1`，`coop_rate = 0`，`mean_payoff = 1`，`payoff_sd = 0`，`regret = 0`，`eq_distance = 0`．
- 実行は `t = 5` で止まります．

## 11. 参考文献

- Nash, J. (1951). Non-cooperative games. *Annals of Mathematics*, 54(2), 286–295.
- Hart, S., & Mas-Colell, A. (2000). A simple adaptive procedure leading to correlated equilibrium. *Econometrica*, 68(5), 1127–1150.
- Shannon, C. E. (1948). A mathematical theory of communication. *Bell System Technical Journal*, 27(3), 379–423.
