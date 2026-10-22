[English](normal-form.md) | **日本語**

# `normal-form` パック

> シナリオファイルだけで組み立てる**戦略形ゲームのシミュレーション**です．段階ゲーム，決定規則に従うエージェントか行動を保持するだけのエージェントからなる集団，Matching，利得，任意の集団動学，ステップごとのゲームの指標を備え，繰り返し囚人のジレンマ，格子やネットワーク上の空間ゲーム，連続行動の寡占を Rust を書かずに走らせます．
> **ワールド：** `GameWorld<FiniteSpec>` か `GameWorld<ContinuousSpec>`（`[world.game] kind` で選ぶ）．**メカニズム：** [`socsim-games`](../games.ja.md) の8つのメカニズムと汎用の `continuation_probability`．**Cargo フィーチャ：** `pack-normal-form`（デフォルトで有効）．

[← パックカタログに戻る](../packs.ja.md)

## 1. 概要

`normal-form` パックは [`socsim-games`](../games.ja.md) クレートを `socsim` バイナリから使えるようにします．各ステップで，エージェントは Matching によって対戦に割り当てられ，決定規則（しっぺ返し，仮想プレイ，ロジット選択，Roth–Erev，繰り返しゲームのオートマトン，減衰付き最適応答など）で行動を選ぶか保持している行動をそのまま使い，段階ゲームの利得を受け取って学習します．集団動学を書いた場合は，さらに互いを模倣したり置き換えたりします．ゲーム，集団，トポロジー，規則のパラメータはすべてシナリオから読みます．

ほかの3つのパックと違い，このパックは **`Registry` を持たず**，`ModulePack` も実装しません．`NormalFormPack` は `[world.game] kind` を読み，`socsim-games` の有限ゲーム用か連続ゲーム用の試行ファクトリ（組込みの規則を持つ `GameTrialFactory<FiniteSpec>` か `GameTrialFactory<ContinuousSpec>`）を選んで，シナリオ全体を渡します．ゲーム部分はライブラリモードのビルダー `GameSimFactory` と同じ手順で組み立てるので，シナリオと，同じゲーム設定を Rust で書いたものは，各ステップで同じワールドと同じ指標の行を与えます．任意の `continuation_probability` はその後に追加され，ライブラリモードで汎用メカニズムをビルダーへ足す場合と同じです．キーの完全な一覧は[戦略形ゲーム § Scenario ファイル](../games.ja.md#scenario-ファイル)にあり，このページではパックが加えるものを扱います．

パックには3つのシナリオが同梱されています．

| ファイル | ゲーム | 示すもの |
|---|---|---|
| `scenarios/normal_form_ipd.toml`（`socsim init` の雛形と同じ） | 繰り返し囚人のジレンマ，2エージェント | しっぺ返し対仮想プレイ，ナッシュ均衡への距離と後悔の列 |
| `scenarios/normal_form_nowak_may.toml` | 100 × 100 のトーラス状 Moore 格子上の空間囚人のジレンマ | 行動を保持するエージェントの Nowak–May の模倣動学，`b = 1.85` |
| `scenarios/normal_form_cournot.toml` | クールノー寡占，3社 | `q* = 22.5` に収束する減衰付き最適応答，集計値と早期停止 |

## 2. ワールド：`GameWorld`

![GameWorld, the two game families and where the scenario keys go](../assets/pack-normal-form-world.svg)

`[world.game] kind` がゲームの族を選び，それに伴ってワールドの型，書ける規則，記録される指標が決まります．

| 族 | `kind` | 行動 | 組込みの規則（`rule = ...`） |
|---|---|---|---|
| 有限 | `matrix`，`prisoners_dilemma`，`battle_of_sexes`，`matching_pennies`，`rock_paper_scissors`，`stag_hunt`，`hawk_dove`，`el_farol`，`minority`，`public_goods_finite` | ASCII のラベルを持つ行動の番号 | `uniform_random`，`tit_for_tat`，`myopic_best_response`，`fictitious_play`，`logit`，`roth_erev`，`all_c`，`all_d`，`grim`，`wsls`，`gtft` |
| 連続 | `cournot`，`bertrand_differentiated`，`public_goods_continuous` | 箱の中のベクトル | `damped_best_response`，`gradient` |

どちらの族でも，現在の行動を保持するだけのエージェント（集団動学が作用するエージェント）には `rule = "held"` と書けます．もう一方の族の規則は未知の規則になります．

`GameWorld` は再開に要る状態をすべて持ちます．保持している行動，エージェントごとの規則の状態，そのステップの対戦と利得，`history_len` ステップ分のエージェントごとの履歴，Matching の状態，トポロジー，集計値の表です．メカニズムはコードだけです．[戦略形ゲーム § 規則の状態と snapshot](../games.ja.md#規則の状態と-snapshot)を参照してください．

シナリオは次の4か所でこれを埋めます．

- **`[world]`** — `n_agents`（格子では省く），`history_len`，そしてトポロジー：`"none"`（既定），`"grid"`（`grid_rows`，`grid_cols`，`boundary`，`neighborhood`），`"network"`（`network_model` とそのパラメータ）．
- **`[world.game]`** — `kind` とゲーム固有のキー（囚人のジレンマなら `temptation`，`reward`，`punishment`，`sucker`，行列なら `labels` と `payoffs`，クールノーなら `a`，`b`，`c`，`q_max` など）．プレイヤー数が `n_agents` であるゲーム（`el_farol`，`minority`，`cournot`，`bertrand_differentiated`）は，その数を `[world]` から取ります．
- **`[world.population]`** — 規則ごとに1つの `[[world.population.group]]`（それぞれ `count` か `share` を持つ）．並べた順に配置するか（`assign = "ordered"`），シャッフルします．初期行動（`initial_shares`，`initial_action`，連続ゲームでは `initial_uniform`）もここに書きます．
- **`[[mechanism]]`** — メカニズムごとに1項目．パラメータは `[mechanism.params]` に書きます（次節）．

## 3. パックのメカニズム

`socsim list mechanisms` は，このパックが受け付ける9つの名前を表示します．

| メカニズム | フェーズ | 挙動 | 必須か | ページ |
|---|---|---|---|---|
| `game_match` | PreStep，Decision | ステップを初期化し，エージェントを対戦に割り当てる（`matching = "fixed_players"`，`"round_robin"`，`"random_pairs"`，`"random_groups"`，`"whole_population"`，`"grid_neighborhood"`，`"network_edges"`） | 必須 | [→](../mechanisms/game-match.ja.md) |
| `game_decide` | Decision，Reward | 規則に従うエージェントが行動を選び，対戦の後に規則が学習する．規則のパラメータ（`coop_label`，`logit_lambda`，`br_damping` など）を持つ | 必須 | [→](../mechanisms/game-decide.ja.md) |
| `game_play` | Interaction | 対戦を評価して利得を書く．任意でゲームの集計値を記録する（`aggregate_history_len`）か，数え上げの経路を使う（`counting`） | 必須 | [→](../mechanisms/game-play.ja.md) |
| `imitate_best` | Reward | 行動を保持するエージェントが，自分と近傍のうち最も稼いだ行動をまねる | 集団動学は1つまで | [→](../mechanisms/imitate-best.ja.md) |
| `fermi_update` | Reward | フェルミ則による対比較．雑音は `k` | 集団動学は1つまで | [→](../mechanisms/fermi-update.ja.md) |
| `replicator_update` | Reward | 係数 `beta` の比例模倣（有限ゲームだけ） | 集団動学は1つまで | [→](../mechanisms/replicator-update.ja.md) |
| `moran_update` | Reward | ステップごとに1回の出生・死亡．適応度は `"linear"` か `"exponential"` | 集団動学は1つまで | [→](../mechanisms/moran-update.ja.md) |
| `game_metrics` | PostStep | ステップごとのゲームの指標を記録する（§4） | 任意 | [→](../mechanisms/game-metrics.ja.md) |
| `continuation_probability` | PostStep | 完了したステップの後に確率 `delta` で継続する．停止時は `continuation_stop` を記録して停止を要求する | 任意 | [→](../mechanisms/continuation-probability.ja.md) |

`[[mechanism]]` の項目の順は実行を変えません．メカニズムは常に `game_match`，`game_decide`，集団動学，`game_play`，`game_metrics`，`continuation_probability` の順に走ります．順に関する規則は1つだけで，集団動学は `game_decide` より**後に書く**必要があります．`phase` のキーは無視されます．集団動学は行動を保持するエージェントにだけ作用し，`update = "synchronous"` だけを受け付けます．

このパックは LLM の決定規則，`fixed` と `mixed` の規則，利用者定義のゲーム・利得規則・Matching を提供しません．これらには[ライブラリモード](../games.ja.md)が要ります．

## 4. パイプラインとメトリクス

![normal-form mechanism pipeline](../assets/pack-normal-form-pipeline.svg)

`game_metrics` はステップごとに1行を記録します．どの列が出るかはゲームとオプションで決まります．その実行で定義されない列は書かれず，NaN として書かれることはありません．

| 族 | メトリクス | 意味 |
|---|---|---|
| 有限 | `game.played_share.a<k>` | そのステップの対戦のうち行動 `k` を使った割合（`a0` は最初のラベル，たとえば `C`） |
| 有限 | `game.mean_payoff`，`game.payoff_sd` | そのステップの利得の平均と標準偏差 |
| 有限 | `game.coop_rate` | 協力した対戦の割合（`cooperate_label` が要る） |
| 有限 | `game.regret` | 実際に使われた行動の混合に対する後悔の平均（既定では2役割のゲームで有効．`regret = false` で止める） |
| 有限 | `game.eq_distance` | ナッシュ均衡への距離（`equilibrium = "solve"`，2役割のゲーム） |
| 有限 | `game.action_entropy` | 使われた行動のエントロピー |
| 有限 | `game.population_share.a<k>` | 行動 `k` を保持しているエージェントの割合（`population_share = true`） |
| 両方 | `game.aggregate.<name>` | ゲームの公開の集計値（`game_play` の `aggregate_history_len`）．クールノーなら `price` と `total_output` |
| 連続 | `game.mean_action.<d>`，`game.action_sd.<d>` | 行動の成分 `d` の平均と標準偏差 |
| 連続 | `game.eq_distance`，`game.mean_payoff`，`game.payoff_sd` | 最適応答反復で求めた均衡への距離（`equilibrium = "solve"`）と利得の統計 |

`stop_tol` と `stop_window` を書くと，`game_metrics` は行動が変わらなくなった時点で実行を早く終えます（クールノーは500ステップのうち90ステップで止まります）．厳密な定義は [`game_metrics` のページ](../mechanisms/game-metrics.ja.md)にあります．

**`[output] metrics` は CLI の列を選びます．** リストが空でなければ，単一シードの時系列，複数シードの要約，sweep の表と CSV は，記載したキーだけを記載順で表示します．重複キーは最初の1回だけを使います．JSONL ログには，従来どおり記録された全指標を書きます．リストが空または省略されていれば，全指標を表示します．`normal-form` では，設定済みの `game_metrics` が示すどのパターンにも一致しないキーを実行前に拒否します．さらに全パックで，どの結果にも現れなかった要求キーを実行後に拒否します．`run` の場合，この検査は JSONL ログを書いた後に行います．したがって，`game.played_share.a9` のようにパターンには一致するものの実際には存在しない行動は，記録済みキーとの実行後検査でエラーになります．

ゲームのメカニズムは既定ではイベントを出しません（`Seed 42 — 0 events recorded`）．
`continuation_probability` が停止させた場合は，停止したステップに `continuation_stop` を1件記録し，
payload に `delta` と一様乱数 `u` を入れます．

### 数え上げの経路：`counting`

**匿名ゲーム**（`hawk_dove`，`el_farol`，`minority`，`public_goods_finite`）では，利得は自分の行動とほかのプレイヤーの行動の個数だけで決まります．`game_play` に `counting = true` と書くと，対戦を行動の組ごとではなく個数から評価します．

```toml
[[mechanism]]
name = "game_play"
[mechanism.params]
counting = true
```

利得，指標，ワールドは `counting = false` のときと同じで，変わるのは対戦ごとの計算量だけです．`materialize` や `aggregate_history_len` と組み合わせられます．ほかの有限ゲームと連続ゲーム（`counting = false` でも）では拒否されます．

```text
mechanism[game_play].counting: counting play requires an anonymous game
mechanism[game_play].counting: counting play requires a finite game
```

## 5. 適用方法

### シナリオ / CLI

```sh
socsim init --module-pack normal-form --out scenarios/ipd.toml
socsim validate scenarios/ipd.toml
socsim run scenarios/ipd.toml
```

雛形（`scenarios/normal_form_ipd.toml` と同じ）は，繰り返し囚人のジレンマでしっぺ返しと仮想プレイを対戦させます．

```toml
[simulation]
name        = "ipd_tft_vs_fp"
module_pack = "normal-form"
t_max       = 200
seed        = 42
scheduler   = "sequential"

[world]
n_agents    = 2
history_len = 200

[world.game]
kind    = "matrix"
labels  = [["C", "D"], ["C", "D"]]
payoffs = [
  [[3.0, 3.0], [0.0, 5.0]],
  [[5.0, 0.0], [1.0, 1.0]],
]

[world.population]
assign = "ordered"                            # Assign to AgentId 0, 1, ... in group order.
[[world.population.group]]
rule  = "tit_for_tat"
count = 1
[[world.population.group]]
rule  = "fictitious_play"
count = 1

[[mechanism]]
name = "game_match"
[mechanism.params]
matching = "fixed_players"
pairs    = [[0, 1]]

[[mechanism]]
name = "game_decide"
[mechanism.params]
decision_scope = "per_step"
coop_label     = "C"                          # Cooperative action for automata.
fp_tie_break   = "lowest_index"

[[mechanism]]
name = "game_play"
[mechanism.params]
payoff_aggregation = "sum"

[[mechanism]]
name = "game_metrics"
[mechanism.params]
cooperate_label = "C"
equilibrium     = "solve"                     # Solve the small game for Nash-equilibrium distance.

[output]
log_path = "runs/{name}_{seed}.jsonl"
metrics  = ["game.mean_payoff", "game.played_share.a0", "game.regret"]   # a0 is action 0, labelled C.
```

```
Running 'ipd_tft_vs_fp' (pack=normal-form, t_max=200, seeds=[42], parallel=false)

Seed 42 — 0 events recorded

t       game.action_entropy    game.coop_rate  game.eq_distance  game.mean_payoff    game.payoff_sd  game.played_share.a0  game.played_share.a1       game.regret
10                0.0000            0.0000            0.0000            1.0000            0.0000            0.0000            1.0000            0.0000
20                0.0000            0.0000            0.0000            1.0000            0.0000            0.0000            1.0000            0.0000
...
200               0.0000            0.0000            0.0000            1.0000            0.0000            0.0000            1.0000            0.0000
```

仮想プレイは相手の行動の経験頻度に最適応答し，1回限りの囚人のジレンマではどんな信念に対しても裏切りが最適応答です．ステップ1では2人とも協力し（`played_share.a0 = 1`），ステップ2で仮想プレイが裏切り，ステップ3からしっぺ返しが裏切りで応じます．そのためステップ3以降，2人はナッシュ均衡にとどまります（`eq_distance = 0`，利得はそれぞれ1）．

繰り返しゲームにラウンドごとの継続確率 δ を与えるには，次の任意メカニズムを足します．
`game_metrics` の後に動くため，停止したラウンドもログに含まれます．

```toml
[[mechanism]]
name = "continuation_probability"
[mechanism.params]
delta = 0.9
```

`delta` は必須で，有限かつ `[0, 1]` の範囲でなければなりません．試行の seed から導いた
専用 RNG を使うため，停止までの Matching，決定，エンジンの乱数列をずらしません．

ほかの2つの同梱シナリオも同じように走ります．その出力は[ユースケースのページ](../usecases.ja.md#8-シナリオから戦略形ゲームを走らせる)にあります．規則・ゲーム・Matching を変えるにはシナリオを書き換えます．キーの一覧は[戦略形ゲーム § Scenario ファイル](../games.ja.md#scenario-ファイル)にあります．

### スイープ

`socsim sweep` はこのパックでは検証付きのスイープになります．軸には `world.` のパスか `<mechanism>.<param>` を書け，試行を1つも走らせる前にすべての組合せを検証し，組合せごとの実効シナリオを CSV の隣に `combo_<i>_<label>.scenario.toml` として書きます．

```sh
socsim sweep scenarios/normal_form_nowak_may.toml \
    --param "world.game.temptation=1.6,1.7,1.8,1.9" --seeds 0..10 --parallel
```

| 軸 | 意味 |
|---|---|
| `world.game.<param>` | 組込みゲームのパラメータ（`temptation`，`v`，`c`，`a`，`r` など）．シナリオに書いたキーだけ |
| `world.population.initial_shares.<label>` | 初期の構成比．シナリオに書いたラベルだけ |
| `world.<key>` | ほかの数値の `[world]` のキー．たとえば `world.history_len`（整数のキーは整数値だけ） |
| `game_decide.logit_lambda` | ロジット規則の合理性 $\lambda$ |
| `game_decide.roth_erev_phi`，`game_decide.roth_erev_epsilon` | Roth–Erev の忘却率と実験率 |
| `game_decide.br_damping`，`game_decide.grad_step` | 連続ゲームの規則の減衰係数と学習率 |
| `fermi_update.k` | フェルミ則の雑音 $K$ |
| `moran_update.w`，`moran_update.mutation` | 選択の強さと突然変異率 |
| `replicator_update.beta` | 比例模倣の係数 $\beta$（書き換えの後にも上限を検査する） |
| `continuation_probability.delta` | 次のステップへ進む確率 δ |

文字列と真偽値（`kind`，`matching`，`update`，`counting` など）はスイープできません．条件ごとに別のシナリオファイルを置いてください．知っておくべきことが3つあります．

1. **スイープする値は浮動小数で書きます．** 基準のシナリオの整数は整数のまま書き戻されるので，`temptation = 2` を `1.6` でスイープすると `sweep axis 'world.game.temptation': existing value is an integer; 1.6 is not an integral value representable as i64` で失敗します．`temptation = 2.0` と書いてください．
2. **`world.population.initial_shares.<label>` は基準のシナリオに書いたラベルだけをスイープできます．** 省いたラベル（残りを受け取るもの）には書き換えるキーがありません：`no key 'D' in [world.population.initial_shares]`．
3. **メカニズムのパラメータが無ければ，拒否されずに足されます．** どのエージェントも使っていないパラメータ（IPD のシナリオの `game_decide.logit_lambda`．`logit` に従うエージェントはいない）をスイープしても，検証は通り，何も変わりません．そのキーを読む規則がどれかのエージェントに割り当てられていることを確かめてください．

`world.` のパスの綴り間違い，未知のメカニズム名，数値でない対象は，試行を走らせる前にスイープを止めます．文言は [CLI リファレンス § sweep](../cli.ja.md#sweep) にあります．

### `validate` が拒否するもの

`socsim validate`，`run`，`sweep` はいずれも最初にシナリオ全体を検査します．すべてのキーと型，ゲーム・規則・Matching・集団動学の組合せ，そしてシナリオの `seed` での1回の乾いた構築です．文言は直すべきキーのパスかメカニズムで始まります（CLI は `validation failed for '<file>'` と `config error: ` の下に表示します）．拒否される組合せは次のとおりです．

| 組合せ | 文言の例 |
|---|---|
| 連続ゲームに有限ゲーム専用の規則，またはその逆 | `world.population.group[0].rule: unknown rule "fictitious_play" (registered: damped_best_response, gradient; "held" means no rule)` |
| 連続ゲームに `replicator_update` | `mechanism[replicator_update]: replicator_update requires a finite game` |
| 匿名でないゲームに `counting = true`，連続ゲームに `counting`（`false` でも） | `mechanism[game_play].counting: counting play requires an anonymous game` |
| 3つ以上の行動を持つゲームにオートマトンか `tit_for_tat` | `mechanism[game_decide]: rule cannot be used with this game: TitForTat requires two actions for role 0, got 3` |
| 3つ以上の役割を持つゲームに `fictitious_play`（か `logit`） | `mechanism[game_decide]: rule cannot be used with this game: learning rule requires 2 roles, but the game has 3` |
| `topology = "grid"` でない `grid_neighborhood`，ネットワークの無い `network_edges` | `mechanism[game_match]: invalid matching: GridNeighborhood requires a grid topology` |
| `update` の無い集団動学，`update = "asynchronous"` の集団動学 | `mechanism[imitate_best].update: expected one of "synchronous", found "asynchronous"` |
| 決定規則を持つ group がある集団動学 | `mechanism[imitate_best]: invalid population dynamics: imitate_best applies only to held agents, but an active rule is assigned` |
| 役割ごとに行動の集合が違うゲームでの集団動学 | `mechanism[imitate_best]: invalid population dynamics: every role must share one action space, but role 0 has 3 actions and role 1 has 2` |
| `game_decide` より前に書いた集団動学 | `mechanism[imitate_best]: a population dynamic must be listed after game_decide` |
| `decision_scope = "per_step"` で，3エージェント以上（か `both_orders = true`）の `round_robin` | `mechanism[game_match]: invalid matching: RoundRobin assigns more than one role to an agent in a per-step decision` |
| `count` と `share` を混ぜた group，和が1にならない share，1を超える `initial_shares` | `world.population.initial_shares: shares sum to 1.1, expected 1` |
| ASCII の `[A-Za-z0-9_-]+` でないラベル | `world.game.labels[0][0]: "協力" is not an action label (ASCII letters, digits, '_' and '-')` |
| 組込みゲームの成立条件（たとえば $T > R > P \ge S$） | `world.game: catalog condition failed: T > R > P >= S` |
| 利得の幅から決まる上限を超える `replicator_update.beta` | `mechanism[replicator_update]: invalid population dynamics: replicator_update requires beta <= 1 / 16.650000000000002 (the per-step payoff range), got 1` |
| `fitness` の無い `moran_update`，負になりうる `"linear"` の適応度 | `mechanism[moran_update].fitness: this key is required` |
| `continuation_probability.delta` が無い，有限でない，または範囲外 | `mechanism[continuation_probability].delta: expected a continuation probability in [0, 1], found 1.1` |
| メカニズムの params の未知のキー | `mechanism[game_decide].logit_lamda: unknown key (known keys: …)` |
| 未知のメカニズム名 | `mechanism 'game_decid' is not registered in the 'normal-form' pack; available: [fermi_update, game_decide, …]` |

検査は `scenario.simulation.seed` で行います．seed に依存する検査（ランダムなネットワーク上の `replicator_update` の上限）は，ほかの seed で失敗することがあります（エラーになります）．構築のエラーの位置の一覧は[戦略形ゲーム § エラー](../games.ja.md#エラー)にあります．

### ライブラリ

同じ入口は `socsim-packs` の Rust の API（フィーチャ `normal-form`，デフォルトで有効）としても使えます（このコードは `socsim-config`，`socsim-runner`，`toml` も使います）．

```rust
use socsim_config::Scenario;
use socsim_packs::normal_form::NormalFormPack;
use socsim_runner::{summarize, SweepAxis};

let scenario = Scenario::from_path("scenarios/normal_form_cournot.toml".as_ref())?;
let pack = NormalFormPack;
pack.validate(&scenario)?; // the same check `socsim validate` runs

let results = pack.run_trials(&scenario, &[0, 1, 2], false)?;
println!("{}", summarize(&results).to_csv());

let axis = SweepAxis { param_key: "game_decide.br_damping".into(), values: vec![0.3, 0.7] };
for point in pack.run_sweep(&scenario, &[axis], &[0, 1], false)? {
    println!("{:?}", point.params);
    println!("{}", toml::to_string(&point.effective)?); // the patched scenario
}
```

`NORMAL_FORM_STARTER` は雛形のシナリオを持ちます．ゲームのシミュレーションを TOML ではなく Rust で組みたい場合や，シナリオでは書けないもの（利用者定義のゲーム・利得規則・Matching，LLM の規則，独自の実験状態）を使う場合は，`socsim-games` を直接使います．`GameSimFactory`，または独自の `RuleRegistry` を渡した `GameTrialFactory` です．[戦略形ゲーム](../games.ja.md)を参照してください．

## 6. 関連項目

- [戦略形ゲーム](../games.ja.md) — `socsim-games` のガイド．[シナリオのキーの一覧](../games.ja.md#scenario-ファイル)を含む．
- [Mechanism カタログ](../mechanisms.ja.md) — `game_match`，`game_decide`，`game_play`，`game_metrics`，`continuation_probability` と4つの集団動学．
- [CLI リファレンス](../cli.ja.md) — `validate`，`run`，検証付きの `sweep` と実効シナリオのファイル．
- [ユースケース＆レシピ](../usecases.ja.md#8-シナリオから戦略形ゲームを走らせる) — 同梱の3つのシナリオとその出力．
- [モジュールパックカタログ](../packs.ja.md) · [アーキテクチャ](../architecture.ja.md)
