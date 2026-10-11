[English](game-play.md) | **日本語**

# ゲームの対戦（`game_play`）

> 各対戦の行動の組を利得に変え，エージェントごとに集計し，そのステップを記録します．
> **フェーズ：** Interaction．**出典：** 戦略形ゲームの利得．**種別：** structural．

[← Mechanism カタログに戻る](../mechanisms.ja.md)

## 1. 概要

`game_play` は，[`socsim-games`](../games.ja.md) の 3 つの Mechanism の 3 つ目です．
`game_match` が書き `game_decide` が埋めた各対戦について，次を行います．

1. すべての席に行動があることを検査します（埋まっていない席はエラー）．
2. 設定された `OutcomeRule` で役割ごとの利得を計算します．
3. 各利得を席のエージェントの `payoff` に足して `n_played` を 1 増やします．
4. World に対の履歴があるとき（割り当てたどれかの規則が `needs_pair_history() == true` を返したとき），そのステップの 2 席の対戦をすべて `EncounterId` の順に追記します．
5. `PayoffAggregation`（`Sum` か `Mean`）を適用します．
6. エージェントごとに `StepRecord { action, payoff, n_played }` を 1 つステップ履歴に追記します．
   `DecisionScope::PerAgentRole` と `PerEncounter` では，エージェントが 1 ステップの中で違う行動を出しうるので，`action` は `None` です．
7. `assemble_with_aggregates`（`G: AggregateGame`）で組み立てたときは，対戦ごとに 1 回（`EncounterId` の順）`aggregate_into` を呼び，
   各値をそのステップの対戦にわたって集約し（`Sum`，または `Mean`．対戦の無いステップでは `Mean` は定義されない），
   `t` をキーとする 1 行を World の集計値の表（長さ `aggregate_history_len` の環）に追記します．

利得は，`assemble_with` に渡す評価経路が選ぶ 2 つの計算のどちらかから出ます．
`Pairwise` は対戦ごとに `OutcomeRule` を評価し（`assemble` の経路），`Counting`（`counting_play()`．`AnonymousGame` 用）は各対戦の行動を 1 回数えて，
各行動の利得をほかの席の人数から計算します．`NormalFormOutcome` ではどちらも同じ利得になります．
`Credit::Center` の対戦では席 0 だけが利得を受け取ります（手順 3）．`GridNeighborhood` がこれを使います．

全エージェントが Held で，粒度が `PerStep` で，評価経路が `Pairwise::new(NormalFormOutcome)` か `counting_play()` で，
Matching が `Matching::implicit()` で型を返す（`WholePopulation`，`NetworkEdges`，`GridNeighborhood`）ときは，
`PlayOptions::materialize(true)` でない限り，`game_play` は対戦の表の代わりにその型をたどってステップを評価します．
利得，対戦数，ステップの記録，集計値の行，指標の列はどちらの経路でも同じです．
実体化しない経路では対戦の表は空のままで，代わりに役割ごとの打った行動の数を `game_metrics` のために保持します
（[`docs/games.ja.md` の「対戦を実体化しない評価」](../games.ja.md#対戦を実体化しない評価) を参照）．

## 2. 理論と出典

戦略形ゲームでは，各役割 $r$ が行動の組 $a$ に対して $u_r(a_1, \dots, a_n)$ を受け取ります．
1 ステップに複数の対戦をしたエージェントのステップの利得は，それらの和か平均です．

$$\pi_i^t = \sum_{e \ni i} u_{r_e(i)}\big(a^{e}\big) \quad (\texttt{Sum}), \qquad \pi_i^t = \frac{1}{n_i^t}\sum_{e \ni i} u_{r_e(i)}\big(a^{e}\big) \quad (\texttt{Mean})$$

- $e \ni i$ — そのステップでエージェント $i$ が席に着いた対戦．$r_e(i)$ は $e$ における役割．
- $n_i^t$（`n_played`）— その対戦の数．

`NormalFormOutcome` は `NormalFormGame::payoffs_into` で $u$ を評価します．
独自の `OutcomeRule` は `X`，保持行動，トポロジー，対戦も使えます（例：評判で重み付けした利得）．

## 3. データフロー

![game_play data flow](../assets/mech-game-play.svg)

対戦（`seats`，`credit`，`actions`）— 実体化しない経路ではトポロジーの上の Matching の型と保持行動 — と，`OutcomeContext` を通してゲーム・`X`・保持行動・トポロジーを読みます．
席ごとの利得，エージェントごとの `payoff` と `n_played`，（有効なら）対の履歴，ステップ履歴，`assemble_with_aggregates` なら集計値の行を書きます．

## 4. 6フェーズループにおける位置

**Interaction** に登録されます．Decision（全行動が選ばれた後）より後，Reward（規則が結果から学習する．
利用者の Reward フェーズの Mechanism も `world.payoffs()` を読める）より前です．

## 5. 状態の読み書きコントラクト

| フィールド | 読み | 書き | 備考 |
|---|:--:|:--:|---|
| `PlayArena.seats`，`credit`，`actions` | ✓ | | 行動はすべて `Some` でなければならない．`Credit::All` は全席に，`Credit::Center` は席 0 にだけ利得を与える．実体化しない経路では読まない． |
| `PlayArena.payoffs` | | ✓ | 席ごとに 1 つの利得（利得を受け取る席かどうかに依らない）．実体化しない経路では空． |
| 役割ごとの打った行動の数 | | ✓ | 実体化しない経路だけ．そのステップで各役割の席が各行動を選んだ数．`game_metrics` が読む．直列化しない（毎ステップ作り直す）． |
| `GameWorld.payoff` | ✓ | ✓ | エージェントごとに足し合わせ，`Mean` のときは `n_played` で割る． |
| `GameWorld.n_played` | ✓ | ✓ | エージェントの席が利得を受け取る対戦ごとに 1 増やす． |
| 対の履歴 | | ✓ | `build_world` が作ったとき（規則が求めたとき）だけ．2 席の対戦ごとに `(t, 双方の行動)` を 1 件，`EncounterId` の順に，利得の計算の後で書く．組ごとに長さ `history_len` の環． |
| ステップ履歴（`StepRecord`） | | ✓ | エージェントごとに 1 件．席のないエージェントの `action` は `None`．`PerAgentRole` / `PerEncounter` ではすべてのエージェントで `None`．長さ `history_len` の環． |
| 集計値の表 | | ✓ | `assemble_with_aggregates` のときだけ．ステップ履歴の後に 1 行 `(t, values)`，`AggregateSpec` ごとに `Option<f64>` を 1 つ．有限でない値はステップを失敗させる．World のフィールド `aggregate`（それ以外では World の JSON から省かれる）．長さ `aggregate_history_len` の環． |
| `game`，`x`，`held`，`topology`，`clock.t` | ✓ | | `OutcomeContext` を通して． |

## 6. 依存関係と順序制約

- **上流：** `game_match`（PreStep）が `payoff` / `n_played` を 0 にします．これがないと利得がステップをまたいで積み上がります．
  `game_decide`（Decision）がすべての席を埋めていなければなりません．
- **下流：** `game_decide`（Reward）は `learn_encounter` / `learn_step` のために新しいステップの記録と対戦の結果を読みます．
  次のステップの `DecisionContext::my_history`，`opponent_history`，`pair_history` は，ここで書いた履歴を読みます．
  対の履歴は Interaction で書かれるので，決定から自分のステップが見えることはありません．
  集計値の表も同じで，`DecisionContext::aggregate_history()` に見えるのは終わった前のステップだけです．
  [`game_metrics`](game-metrics.ja.md)（PostStep）はこのステップの集計値の行を `game.aggregate.<name>` として記録します．
- **決定の粒度：** `assemble` は `PlayOptions::decision_scope` を `game_play` にも渡します．`StepRecord::action` を記録するかどうかはこれで決まります．

## 7. パラメータ

| パラメータ | 既定値 | 種別 | 備考 |
|---|---|---|---|
| `outcome: O`（`O: OutcomeRule<G, X>`） | — | structural | `assemble` に渡すか，`Pairwise::new(outcome)` として `assemble_with` に渡す．組込み：`NormalFormOutcome`（`G: NormalFormGame` を要求）． |
| `counting_play()` | — | structural | `assemble_with` の評価経路 `Counting`（`G: AnonymousGame` を要求）．利得は `Pairwise::new(NormalFormOutcome)` と同じで，利用者定義の利得規則は受け取れない． |
| `opts.materialize` | `false` | structural | `true` なら常に対戦の表を作る．`false` なら条件が揃えば実体化しない経路を使う． |
| `opts.payoff_aggregation` | `PayoffAggregation::Sum` | structural | `Sum` か `Mean`．`Mean` では対戦しなかったエージェントは `0` のまま． |
| `opts.decision_scope` | `DecisionScope::PerStep` | structural | `PerStep`：`StepRecord::action` はエージェントの行動．`PerAgentRole` / `PerEncounter`：`None`． |
| `aggregate_history_len` | — | structural | `assemble_with_aggregates` のときだけ．`aggregate_history()` のために保持する行数．`> 0` でなければならない（`GameError::InvalidAggregateHistoryLength`）．名前と集約の方法は `AggregateGame::aggregate_spec()` から． |

## 8. 適用方法

ライブラリ専用です．`assemble` が `outcome` と `opts` から `game_play` を構築します．
`assemble_with` は評価経路（`Pairwise::new(outcome)` か `counting_play()`．どちらも `.with_aggregates(n)` を付けられる）から構築します．
`assemble_with_aggregates(.., seed, aggregate_history_len)` は集計値を集める `game_play` を構築します
（[`docs/games.ja.md` の「公開集計値」](../games.ja.md#公開集計値aggregategame) を参照）．

### ライブラリモード

```rust
use socsim_games::{assemble, FixedPlayers, NormalFormOutcome, PayoffAggregation, PlayOptions};

let opts = PlayOptions::new().payoff_aggregation(PayoffAggregation::Mean);
let mut sim = assemble(world, FixedPlayers::pairs(&[(0, 1)]), rules, NormalFormOutcome, opts, seed)?
    .build();
sim.step()?;
let step_payoffs = sim.world().payoffs();          // reset every step
let last = sim.world().history(socsim_core::AgentId(0)).and_then(|h| h.last().cloned());
```

## 9. 決定論性と RNG

乱数は**引きません**．対戦は `EncounterId` の順に（実体化しない経路では型をたどる順，つまり Matching が対戦を積んだはずの順に）評価し，記録は `AgentId` の昇順に（対の履歴の記録は `EncounterId` の順に）書くので，
結果は Decision で選ばれた行動だけで完全に決まります．

## 10. 期待される動作

$(T, R, P, S) = (5, 3, 1, 0)$ の囚人のジレンマで，エージェント 0 が協力しエージェント 1 が裏切ったステップでは，
`payoffs() == [0.0, 5.0]`，`n_played() == [1, 1]` になります．

## 11. 参考文献

- von Neumann, J., & Morgenstern, O. (1944). *Theory of Games and Economic Behavior*. Princeton University Press.
