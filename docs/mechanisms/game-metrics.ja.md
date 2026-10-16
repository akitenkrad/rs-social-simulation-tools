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
3. 集計値つきで組み立てたときは，そのステップの公開集計値の行を `game.aggregate.<name>` として写します．
   `population_share(true)` なら，行動 `k` を保持する全エージェントの割合 `game.population_share.a<k>` を書きます．
4. 設定した `MixedSource` から役割ごとに 1 つの混合戦略を作り，`game.regret` と，`EquilibriumTarget::Solve` なら `game.eq_distance` を書きます．
5. 収束のカウンターを更新し，割合が `window` ステップ安定したら実行の終了を求めます．

そのステップで値が定義されない列は行から省き，NaN を記録することはありません．
列の詳しい説明は [`docs/games.ja.md` の「指標」](../games.ja.md#指標) にあります．

**連続ゲーム．** `GameMetrics` は `FiniteGame` を要求します．[連続ゲーム](../games.ja.md#連続ゲーム) では，同じ Mechanism が `ContinuousMetrics<G>`（`G: ContinuousGame`）の形をとり，`ContinuousMetrics::new(&game, ContinuousMetricsOptions)` で作ります．
同じ名前 `game_metrics` で登録され，PostStep で動き，同じ表 `"game"` に書くので，このページで両方の形を扱います．
連続の形は毎ステップの後に次を行います．

1. そのステップの席にわたる行動の各成分の平均と母標準偏差 `game.mean_action.<d>`，`game.action_sd.<d>` を書きます（役割によって成分の数が違うときは役割ごとに `game.mean_action.r<r>-<d>`，`game.action_sd.r<r>-<d>`）．
2. 有限の形と同じく `game.mean_payoff`，`game.payoff_sd`，`game.aggregate.<name>` を書きます．
3. 均衡を設定していれば `game.eq_distance` を書きます．
4. **エージェントごと** の収束のカウンターを更新し，どのエージェントの行動も `window` ステップ続けて `tol` 動かなければ実行の終了を求めます．

割合，`coop_rate`，`regret`，`action_entropy`，`population_share` は書きません．

役割ごとの列は今は記録されません．役割によって成分の数が違う連続ゲームは World に組めないからです．`build_world` はすべての初期行動を，組み立てはすべての fallback の行動を，すべての役割について検査します．そのため役割ごとの列は `describe()` にだけ現れます．これらの検査を通らずに作った World（たとえば `GameWorld` を逆直列化したものや，`Simulation::restore` で読み込んだもの）には対応していません．

## 2. 理論と出典

ステップ $t$ の席を $S_t$，席 $s$ の行動を $a_s$，その役割を $r(s)$ とすると，打たれた行動の割合は次のとおりです．

$$p_t(k) = \frac{\lvert\{s \in S_t : a_s = k\}\rvert}{\lvert S_t\rvert} \ \ (\text{pooled ids}), \qquad p_t(r, k) = \frac{\lvert\{s \in S_t : r(s) = r,\ a_s = k\}\rvert}{\lvert\{s \in S_t : r(s) = r\}\rvert} \ \ (\text{per-role ids})$$

役割ごとに 1 つの混合戦略からなる組 $\sigma = (\sigma_r)_r$ について，$F_r$ を役割 $r$ の期待利得とすると，

$$\text{regret}(\sigma) = \max_r \Big[\max_a F_r(a, \sigma_{-r}) - F_r(\sigma)\Big], \qquad \text{eq\_distance}(\sigma) = \min_{\sigma^* \in E} \sum_r \sum_a \big\lvert \sigma_r(a) - \sigma^*_r(a)\big\rvert$$

- 後悔はちょうど Nash 均衡（Nash, 1951）で 0 になります．ゲームにおける学習の文献で広く使われる，一方的な逸脱による利得の増分です
  （例：Hart & Mas-Colell, 2000）．
- $E$ は 2 人ゲームの完全な均衡の集合で，サポート列挙で 1 回だけ求めます．
- `game.action_entropy` は Shannon（1948）のエントロピー $-\sum_k p_t(k) \ln p_t(k)$ で，単位は nat です．

エージェントの集合を $A$，PostStep でのエージェント $i$ の保持行動を $h_i$ とすると，集団の割合は次のとおりです．

$$q_t(k) = \frac{\lvert\{i \in A : h_i = k\}\rvert}{\lvert A\rvert}$$

これは $p_t$ と 2 点で違います．席ではなくエージェントを数えること，そして [集団動学](../games.ja.md#集団動学) のもとでは，
$p_t$ がそのステップで打った行動を表すのに対し，$q_t$ はそのステップの更新**後**の行動（次のステップの行動）を表すことです．

**連続の形．** 席 $s$ の行動ベクトルを $x_s$，役割 $r$ の席を $S_t^r$，設定した均衡を $x^*$ とすると，

$$\bar x_{t,d} = \frac{1}{\lvert S_t\rvert} \sum_{s \in S_t} x_{s,d}, \qquad \text{action\_sd}_{t,d} = \sqrt{\frac{1}{\lvert S_t\rvert} \sum_{s \in S_t} \big(x_{s,d} - \bar x_{t,d}\big)^2}, \qquad \text{eq\_distance}_t = \sum_r \sum_d \Big\lvert \frac{1}{\lvert S_t^r\rvert} \sum_{s \in S_t^r} x_{s,d} - x^*_{r,d} \Big\rvert$$

です．`eq_distance` は役割ごとの平均行動をその役割の均衡の行動と比べるので，すべての役割の席の平均が均衡に一致するときにちょうど 0 になります．
収束による停止は，各エージェントがそのステップで出した行動を `AgentId` 順につないだベクトル $v_t$ をステップごとに比べます（対戦しなかったエージェントは前の値を保ちます）．
$\max_k \lvert v_t(k) - v_{t-1}(k)\rvert < \texttt{tol}$ の間カウンターが増えます．
エージェントごとの基準なので，平均が落ち着いていても個々が動いていれば実行は止まりません．

## 3. データフロー

![game_metrics data flow](../assets/mech-game-metrics.svg)

対戦の表（`seats`，`actions`）— 実体化しない経路では `game_play` がそのステップについて保持した役割ごとの打った行動の数 —，保持行動（`population_share` のため），エージェントごとの `payoff` と `n_played`，最後の集計値の行，`MixedSource::Reported` なら混合戦略の報告を読みます．
Recorder の表 `"game"` に 1 行を書き，`stop` か `MixedSource::PlayedCumulative` を設定したときだけ，World に自分の `MetricsState` を書きます．任意のイベント記録は，同じ Recorder に `game.roster`，`game.step_actions`，`game.play` を書きます．

## 4. 6フェーズループにおける位置

**PostStep** に登録されます．その時点で `game_play`（Interaction）が利得と集計値の行を埋め，
`game_decide`（Reward）が学習後の報告を書いているので，終わったステップを見ます．
ゲームの Mechanism はその出力を読みません．PostStep でこれより後に足した Mechanism は，これより後に動きます．

## 5. 状態の読み書きコントラクト

| フィールド | 読み | 書き | 備考 |
|---|:--:|:--:|---|
| `PlayArena.seats`，`actions` | ✓ | | 席の役割は席の番号を役割数で割った余り．行動はすべて `Some` で，その役割の範囲内でなければならない． |
| 役割ごとの打った行動の数 | ✓ | | ステップが実体化しない経路で動いたとき（対戦の表は空），対戦の表の代わりに読む．割合，`coop_rate`，後悔，累積の数は同じになる． |
| `GameWorld.held` | ✓ | | `population_share(true)` のときだけ．共通の行動の集合の外にある保持行動はエラー． |
| `GameWorld.payoff`，`n_played` | ✓ | | `mean_payoff` / `payoff_sd` に入るのは `n_played > 0` のエージェントだけ． |
| `game` | ✓ | | 役割数と行動数を `new` に渡したゲームと照合する．利得は後悔に使う． |
| 集計値の表 | ✓ | | 最後の行だけ．その `t` がこのステップのときだけ． |
| 混合戦略の報告，`PolicyTable` のスロット | ✓ | | `MixedSource::Reported` のときだけ．報告はこのステップのものでなければならない．`Held` の席は純粋戦略として数える． |
| `MetricsState { prev, below, cumulative }` | ✓ | ✓ | `stop` か `PlayedCumulative` を設定したとき，最初のステップで作る．World のフィールド `metrics_state`（それ以外では World の JSON から省かれる）．snapshot と再開を越えて残る． |
| ステップの履歴，`held` | ✓ | | 連続の形で `stop` のときだけ．各エージェントがそのステップで出した行動（`PerEncounter` では最初の席の行動），なければ前の値，最初のステップでは保持行動． |
| Recorder の表 `"game"` | | ✓ | ステップごとに `record_row` を 1 回． |
| Recorder のイベント `game.roster` | | ✓ | `log_actions(true)` のとき，この Mechanism が最初に処理する PostStep で 1 回：`{ agents, kinds, decision_scope }`．snapshot から再開して新しい Mechanism を作るともう一度出る．reader は同じ roster の重複を受け入れる． |
| Recorder のイベント `game.step_actions` | | ✓ | `log_actions(true)` かつ `t % log_actions_every == 0` のとき：`{ actions, roles, payoffs, n_played }`．配列はすべて roster 順で，`actions` と `roles` には `null` が入り得る．`async_population_step` では，全更新後の行動と，その行動から計算し直した利得になる． |
| Recorder のイベント `game.play` | | ✓ | `log_plays(true)` のとき，実体化した対戦ごとに 1 回：`{ encounter, seats, actions, payoffs, credit }`．有限の行動は index，連続の行動は配列． |
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
- **記録の検査：** 0 の間隔と人口上限は `GameMetrics::new` が拒否します．人口の大きさ，`PerStep` の行動の意味，役割の曖昧さ，実体化した対戦経路を要する検査は，最初の PostStep でイベントや指標の行を書く前に走ります．したがって，`log_plays` は暗黙の経路では失敗し，`async_population_step` では使えません．
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
| `population_share(on)` | 無効 | observation | `game.population_share.a<k>`（`k` を保持するエージェント ÷ 全エージェント．集団動学の後）を足す．すべての役割の行動数が等しくなければならない（`GameError::Metrics`）． |
| `log_actions(on)` | 無効 | observation | `game.roster` と，間引いた `game.step_actions` を出す．エージェントを標本抽出せず，シミュレーションの軌跡を変えない． |
| `log_actions_every(every)` | `1` | observation | `t % every == 0` のとき `game.step_actions` を出す．`every ≥ 1`． |
| `log_actions_max_agents(maximum)` | `1000` | guard | 人口が上限を超えると行動の記録を拒否する．`maximum ≥ 1`． |
| `log_plays(on)` | 無効 | observation | 実体化した対戦ごとに `game.play` を 1 つ出す．`PlayOptions::materialize(true)` が必要で，`async_population_step` では使えない． |

連続の形は `ContinuousMetricsOptions` を受けます．

| パラメータ | 既定値 | 種別 | 備考 |
|---|---|---|---|
| `equilibrium(ContinuousEquilibrium)` | `None` | observation | `Solve`：`new` の中で `best_response_iteration` を 1 回，区間の中点から既定のオプションで．`Converged` でなければ拒否．見つける均衡は 1 つ．`Given(profile)`：役割ごとに長さの合った有限の行動．どちらも `game.eq_distance` を足す． |
| `stop(tol, window)` | なし | control | どのエージェントの出した行動のどの成分も `tol` 以上変わらないステップが `window` 回続いたら停止．`tol` は有限で `> 0`，`window ≥ 1`． |
| `log_plays(on)` | 無効 | observation | 実体化した対戦ごとに `game.play` を 1 つ出す．連続の行動は配列になる．`PlayOptions::materialize(true)` が必要． |

区間やオプションが不正なとき，`Given` の形が合わないとき，解が収束しないときは，`ContinuousMetrics::new` が `GameError::Metrics` を返します．

行動の記録は大きくなり得ます．$N$ 人の `game.step_actions` 1 件は，およそ $(20N + 20)$ バイトが目安です．$N = 50$ の実測では記録ステップあたり 0.76〜1.08 KB で，$10^5$ ステップでは 76〜108 MB，指標の行も含めると約 140 MB でした．2 人・50 ステップの例では，`game.step_actions` が 1 行 135〜136 バイト，`game.play` が対戦ごとに 133〜134 バイト，`game.roster` が 140 バイトで，ファイル全体は 551 行，42,761 バイトでした．`log_plays` は対戦ごとに 1 行を足します．長い実行では `log_actions_every` を大きくして間引いてください．

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

連続ゲームでは次のようにします．

```rust
use socsim_games::{ContinuousEquilibrium, ContinuousMetrics, ContinuousMetricsOptions};

let metrics = ContinuousMetrics::new(     // before `game` moves into build_world
    &game,
    ContinuousMetricsOptions::new()
        .equilibrium(ContinuousEquilibrium::Solve)
        .stop(1e-9, 10),
)?;
let mut sim = assemble(world, WholePopulation::new(), rules, NormalFormOutcome, PlayOptions::new(), seed)?
    .add_mechanism(Box::new(metrics))
    .build();
```
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

3 社のクールノー（$P = 100 - Q$，単位費用 10，$q^* = 22.5$）が `WholePopulation` の下で `[0, 50, 100]` から $\alpha = 0.5$ の `DampedBestResponse` で学習し，集計値つきで組み立て，`ContinuousEquilibrium::Solve`（`[[22.5], [22.5], [22.5]]` を見つける）と `stop(1e-9, 10)` を設定すると，

- `t = 1`：`game.mean_action.0 = 50`，`game.action_sd.0 ≈ 40.82`，`game.mean_payoff = -3000`，`game.aggregate.total_output = 150`，`game.aggregate.price = -50`，`game.eq_distance = 127.5`（$22.5 + 27.5 + 77.5$．各役割の自分の席を 22.5 と比べる）．
- `t = 2`：行動は `[0, 25, 60]`，`game.mean_action.0 ≈ 28.33`，`game.eq_distance = 62.5`．
- 実行は `t = 92` で止まり，`game.mean_action.0 = 22.5`，`game.mean_payoff = 506.25`，`game.eq_distance ≈ 3.6e-10` です（`examples/cournot.rs` も同じ停止を表示します）．

[`docs/games.ja.md` の「集団動学」](../games.ja.md#集団動学) のタカ・ハトの例（Held の 1 000 人，初期のタカ 300 人，`RandomPairs`，比例模倣）で `population_share(true)` にすると，
`t = 1` には `game.played_share.a0 = 0.3`（対戦したタカ）と `game.population_share.a0 = 0.319`（最初の更新の後のタカ）が記録されます．

## 11. 参考文献

- Nash, J. (1951). Non-cooperative games. *Annals of Mathematics*, 54(2), 286–295.
- Hart, S., & Mas-Colell, A. (2000). A simple adaptive procedure leading to correlated equilibrium. *Econometrica*, 68(5), 1127–1150.
- Shannon, C. E. (1948). A mathematical theory of communication. *Bell System Technical Journal*, 27(3), 379–423.
