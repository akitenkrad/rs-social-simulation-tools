[English](game-decide.md) | **日本語**

# ゲームの決定（`game_decide`）

> 席に着いた各エージェントの決定規則が行動を選び，対戦の後に規則が学習します．
> **フェーズ：** Decision ＋ Reward．**出典：** ゲーム理論のシミュレーション（決定規則）．**種別：** rule-driven．

[← Mechanism カタログに戻る](../mechanisms.ja.md)

## 1. 概要

`game_decide`（型は `RuleDriver`）は，[`socsim-games`](../games.ja.md) の 3 つの Mechanism の 2 つ目です．
すべてのエージェントの `DecisionRule` を動かします．

- **Decision** では，席に着いた各エージェントに行動を尋ねます（規則を通して．`Held` のエージェントは保持行動から）．
  行動を `StageGame::check_action` で検査し，エージェントの席に書きます．
  エージェントが何回決定するかは**決定の粒度**（`PlayOptions::decision_scope`）で決まります．
  1 ステップに 1 回（`PerStep`．既定．保持行動も更新する），そのステップに持つ役割ごとに 1 回（`PerAgentRole`），
  席ごとに 1 回（`PerEncounter`）のいずれかです．
- **Reward** では，規則で動く全エージェントについて，そのステップの記録と対戦の結果を渡し，
  （規則が求めるなら）そのエージェントの対戦ごとに `learn_encounter` を呼び，その後で `learn_step` を呼びます．
- `PlayOptions::report_mixed(true)` なら，**Reward の終わり**（全エージェントの学習の後）に，規則で動く各エージェントの規則に
  全役割の混合戦略を尋ね（`DecisionRule::mixed`），報告を World に保存します．`GameWorld::reported_mixed(agent, role)` で読めます．

規則のオブジェクトそのものは共有されるコードです．各エージェントの規則の状態は World の `PolicyTable` にあるので，
この Mechanism は再開後の実行に影響する状態を持ちません．

[連続ゲーム](../games.ja.md#連続ゲーム)（`G: ContinuousGame`，行動は `Vec<f64>`）では，同じ検査が，成分の数が役割の区間と違う行動，NaN や無限大の成分を持つ行動，区間の外にある行動を拒否します（`ActionError::{DimensionMismatch, NonFiniteComponent, OutOfBounds}`．組込みの連続ゲームは `check_action` を `check_continuous_action` で実装しています）．
ほかの不正な行動と同じく，`OnInvalidAction::Stop` ではステップを止め，`Fallback` では置き換えられます．

`game_decide` が動かすのは自分の `RuleBook` の種類だけです．どの Driver が種類を担当するかは `KindCatalog` で決まります．
feature `llm` では，`LlmRuleBook` に登録した種類は [`game_llm_decide`](game-llm-decide.ja.md) の担当で，
`game_decide` はそれらのエージェントを Decision でも Reward でも飛ばします．その席は，`assemble_with_llm` が `game_decide` のすぐ後に登録する `game_llm_decide` が埋めます．

## 2. 理論と出典

決定規則は，繰り返しゲームにおける行動戦略です．エージェントが知っていることから行動への写像と，内部状態の更新からなります．

$$a_i^t = \text{decide}_{k(i)}\big(s_i,\ \text{view}_i^t;\ \text{rng}\big), \qquad s_i \leftarrow \text{learn}_{k(i)}\big(s_i,\ \text{outcome}_i^t\big)$$

- $k(i)$ — エージェント $i$ に割り当てた規則の種類（`PolicyTable.kind`）．
- $s_i$ — エージェント $i$ の規則の状態（`X::PolicyState`）．
- $\text{view}_i^t$ — `DecisionContext`：自分の役割と対戦，現在の相手，自分の履歴，相手の行動の履歴，`X::view_for(i)`．

組込みの規則は `Fixed`（一定の行動），`UniformRandom`（役割の行動から一様），
`TitForTat`（Axelrod, 1984：初回は協力し，以後は相手の直前の行動をまねる），`Mixed`（固定の混合戦略），
`MyopicBestResponse`（相手の直前の行動への最適応答），
`FictitiousPlay`（Brown, 1951．同時更新，2 人ゲーム．Shapley (1964) の例では収束せずに巡回する），
`LogitChoice`（相手の直近の行動か経験頻度へのロジット応答），
`RothErev`（Erev & Roth (1998) の 3 パラメータの強化学習．忘却 `φ` と試行 `ε` を持つ），
`Automaton`（ALLC，ALLD，GRIM，TFT，WSLS — Nowak & Sigmund, 1993 — と寛容な TFT）です．
学習の計算は `socsim_games::learn` にあります（[学習則と決定の粒度](../games.ja.md#学習則と決定の粒度) を参照）．

決定の粒度が `PerStep` より細かいとき，`decide` は $(t, i)$ ごとではなく決定の単位ごと —
`PerAgentRole` では $(t, i, \text{role})$，`PerEncounter` では $(t, i, \text{encounter})$ — に 1 回呼ばれます．
学習の更新は，どの粒度でも，対戦ごとの呼び出し（求める規則だけ）とその後のステップごとの呼び出しからなります．

$$s_i \leftarrow \text{learn}^{\text{step}}_{k(i)}\Big(\text{learn}^{\text{enc}}_{k(i)}\big(\cdots \text{learn}^{\text{enc}}_{k(i)}(s_i,\ o_{i,1}^t) \cdots,\ o_{i,m}^t\big)\Big)$$

ここで $o_{i,1}^t, \dots, o_{i,m}^t$ は `EncounterId` の順に並べたエージェント $i$ の対戦の結果，
$\text{learn}^{\text{enc}}$ は `learn_encounter`，$\text{learn}^{\text{step}}$ は `learn_step` です．

連続ゲームの 2 つの規則は **同時手番** で動きます．$x_{-i}^{t-1}$ を前のステップでほかの役割が出した行動（ステップの履歴から読み，保持行動からは読まない），$x_i$ をエージェント $i$ が最後に出した行動として，

$$\texttt{DampedBestResponse}:\ x_i \leftarrow \mathrm{Proj}\big((1 - \alpha)\,x_i + \alpha\,\mathrm{BR}_r(x_{-i}^{t-1})\big), \qquad \texttt{GradientAscent}:\ x_i \leftarrow \mathrm{Proj}\big(x_i + \eta\,\nabla_{x_i} u_r(x_i, x_{-i}^{t-1})\big)$$

です．$\mathrm{Proj}$ は各成分を役割の区間へ切り詰めます．
減衰 $\alpha \in (0, 1]$ があるのは，減衰の無い同時手番の最適応答が収束するとは限らないからです．線形のクールノー競争では，内点の均衡は 3 社で中立安定，4 社以上で不安定です（Theocharis, 1960）．
前のステップを読むので，更新はエージェントの処理順に依らない Jacobi 型の反復になり，`DampedBestResponse` はソルバーの `best_response_step` とちょうど同じ更新をします．
最初のステップ（ほかの役割がまだ出していないとき）は，自分の保持行動（`DecisionContext::my_held()`）をそのまま出します．
最適応答はゲームの解析的な `best_response`，注入した `BestResponseOracle`，`NumericBr::Unimodal1D` を宣言した 1 次元の行動での黄金分割の探索のいずれかから，勾配はゲームの `gradient` か，刻み $h = 10^{-6} \max(1, \lvert x_d \rvert)$ の中心差分（区間の端では片側差分なので，誤差は $\lvert x \rvert$ とともに増える）から得ます．
詳細は [`docs/games.ja.md` の連続ゲーム](../games.ja.md#決定規則dampedbestresponse-と-gradientascent) にあります．

規則は `socsim_marl::Policy` とは別物です．`DecisionRule` は `G::Action` 上のゲーム層の戦略で状態を World に置くものであり，
観測ベクトル上のニューラルな方策ではありません．

## 3. データフロー

![game_decide data flow](../assets/mech-game-decide.svg)

Decision は，`game_match` が書いた席，各エージェントの規則の種類と状態，保持行動，履歴，（有効なら）対の履歴，`X` を読み，
選んだ行動を席に，保持行動（`PerStep` のときだけ）と規則の状態を World に書きます（`decide` は状態を変更してよい）．
Reward は，`game_play` が書いたそのステップの記録と対戦の結果を読み，規則の状態を更新します．
`report_mixed` なら，その後で混合戦略の報告を書きます．

## 4. 6フェーズループにおける位置

**Decision** と **Reward** に登録されます．

- **Decision** — 同じフェーズで `game_match` の後（登録順）に動くので，対戦ができています．
  エージェントはスケジューラーの活性化の順（`ctx.agent_order`）に訪れ，席のないエージェントと LLM の種類のエージェントは飛ばします．
  1 人のエージェントの中では，決定は決定の単位の順に行います．1 ステップに 1 回，役割の昇順に役割ごとに 1 回（`PerAgentRole`），
  `EncounterId` の順に席ごとに 1 回（`PerEncounter`）のいずれかです．
  `assemble_with_llm` では，同じフェーズで次に `game_llm_decide` が動き，LLM のエージェントの席を埋めます．
- **Reward** — `game_play`（Interaction）が利得とステップ履歴を書いた後なので，学習はステップ全体を見ます．
  エージェントは `AgentId` の昇順に訪れ，`Held` のエージェントと LLM の種類のエージェントは飛ばします（`game_llm_decide` の Reward はこの後に動く）．
  規則が `needs_encounter_learning() == true` を返すエージェントについては，そのエージェントの対戦ごとに `EncounterId` の順で `learn_encounter` を呼び，
  その後で `learn_step` を呼びます．組込みの学習則は，`PerEncounter` では `learn_encounter` で相手ごとの状態を，
  ほかの粒度では `learn_step` で全体で 1 つの状態を更新します．
  `report_mixed` では，全エージェントの学習の後に，やはり `AgentId` の昇順で報告を書きます．
  ステップ $t$ の報告は，ステップ $t+1$ の決定が使う**学習後**の混合であって，ステップ $t$ の行動を生んだ混合ではありません．

## 5. 状態の読み書きコントラクト

| フィールド | 読み | 書き | 備考 |
|---|:--:|:--:|---|
| `PlayArena.seats` | ✓ | | Decision：行動するエージェントとその役割． |
| `PlayArena.actions` | | ✓ | Decision：各席の検査済みの行動． |
| `GameWorld.held` | ✓ | ✓ | Decision：`Held` のエージェントはこれをすべての席に出す．`PerStep` では規則で動くエージェントの行動がこれを置き換える（`PerAgentRole` / `PerEncounter` では置き換えない）． |
| `PolicyTable`（`kinds`，`slot`，`kind_of`，`states`） | ✓ | ✓ | `decide` と `learn_step` はエージェントの状態への `&mut` を受け取る． |
| ステップ履歴，`game`，`x`，`ids`，`clock.t` | ✓ | | `DecisionContext` / `LearnContext` を通して． |
| 対の履歴 | ✓ | | Decision：`DecisionContext::pair_history`（現在の相手）と `omniscient()` を通して．規則が `needs_pair_history() == true` を返すときだけ存在する．書くのは `game_play`． |
| `PlayArena.payoffs` / `credit` | ✓ | | Reward：`LearnContext::outcomes()` を通して． |
| Recorder のイベント `game.invalid_action` | | ✓ | `OnInvalidAction::Fallback` のときだけ． |
| 混合戦略の報告 | | ✓ | Reward．`report_mixed(true)` のときだけ．表を空にして `t` を記し，規則で動くエージェントと役割ごとに `DecisionRule::mixed` の `Option<Vec<f64>>` を 1 つ書く．`Held` のエージェント，別の Driver の種類，`None` を返す規則，有限でない値では `None`．World のフィールド `reports`（無効なら World の JSON から省かれる）．snapshot と再開を越えて残る． |
| 集計値の表 | ✓ | | Decision：`DecisionContext::aggregate_history()` を通して（前のステップだけ）．`assemble_with_aggregates` のときだけある．`game_play` が書く． |

## 6. 依存関係と順序制約

- **上流：** `game_match`（Decision）が対戦を書いていること．Reward の前に `game_play`（Interaction）が利得とステップ履歴を書いていること．
- **下流：** `game_play` はすべての席に行動があることを要求します．LLM のエージェントの席は，`game_decide` の後に登録される `game_llm_decide` が埋めます．
  `MixedSource::Reported` の [`game_metrics`](game-metrics.ja.md) は，PostStep でこのステップの混合戦略の報告を読みます．
- **混合戦略の報告と決定の粒度：** `report_mixed(true)` と `DecisionScope::PerEncounter` の組み合わせは `assemble` が拒否します（`GameError::Metrics`）．
  この粒度では学習則が相手ごとに状態を持ちますが，`mixed` は全体で 1 つの状態を読むからです．
- **決定の粒度と Matching：** `PerStep` では，各エージェントが 1 ステップに持つ役割は 1 つでなければなりません．
  `RoundRobin` は，エージェントを 2 つの役割に着けることになる `PerStep` を `assemble` で拒否します．
  それ以外の場合は `game_decide` が `RuleError::AmbiguousRole` でステップを止めます（`Held` のエージェントも対象）．
  同じ粒度が `game_match`（`MatchContext::decision_scope` を通して）と `game_play` にも渡されます．
- **情報構造：** 既定のビューで見えるのは，自分の対戦の相手だけです（それ以外の相手に対して `opponent_history` と `pair_history` は `None` を返す）．
  World 全体は `omniscient()` からだけ読めます．Decision の間に `omniscient()` から読む `held` は，活性化の順序に依存することがあります．

## 7. パラメータ

| パラメータ | 既定値 | 種別 | 備考 |
|---|---|---|---|
| `rules: RuleBook<G, X>` | — | structural | `assemble` に渡す．種類は名前で登録する． |
| `opts.on_invalid_action` | `OnInvalidAction::Stop` | policy | `Stop`：不正な行動でステップが失敗する．`Fallback(map)`：種類ごとの代替行動に差し替え，`game.invalid_action` を記録する． |
| `opts.decision_scope` | `DecisionScope::PerStep` | structural | `PerStep` / `PerAgentRole` / `PerEncounter`．`game_match` と `game_play` にも渡される．LLM による決定は `PerStep` だけに対応する． |
| `opts.report_mixed` | `false` | observation | Reward の終わりに学習後の混合戦略の報告を書く．`PerEncounter` とは併用できない．組込みの各規則が何を報告するかは [`docs/games.ja.md` の「混合戦略の報告」](../games.ja.md#混合戦略の報告) を参照． |

`Fallback` の表が対象にするのはこの Mechanism の種類だけです．LLM の種類は書きません（そのフォールバックは LLM の規則の `RetryProtocol` が持つ）．
`Fallback` のとき，`assemble` は使われているすべての規則の種類に代替行動があることを要求し，各代替行動を全役割で `check_action` により検査します．
イベントの payload は `{ "agent", "kind", "role", "action" }`（拒否された行動を `Debug` 形式で）です．

## 8. 適用方法

ライブラリ専用です．`assemble` が `rules` と `opts` から `game_decide` を構築します．

### ライブラリモード

```rust
use std::collections::BTreeMap;
use std::sync::Arc;
use socsim_games::{
    assemble, FixedPlayers, NormalFormOutcome, OnInvalidAction, PlayOptions, RuleBook,
    TitForTat, UniformRandom,
};

let mut rules = RuleBook::new();
rules.register("tit_for_tat", Arc::new(TitForTat::new(0)))?;
rules.register("uniform_random", Arc::new(UniformRandom))?;
// build_world(game, n, x, initial_held, &rules, &assignment, history_len, t_max, seed)?

let opts = PlayOptions::new().on_invalid_action(OnInvalidAction::Fallback(BTreeMap::from([
    ("tit_for_tat".to_owned(), 0),
    ("uniform_random".to_owned(), 0),
])));
let sim = assemble(world, FixedPlayers::pairs(&[(0, 1)]), rules, NormalFormOutcome, opts, seed)?
    .build();
```

各エージェントが相手ごとに学習の状態を持つ総当たりにするには，Matching に `RoundRobin::new()` を，
オプションに `PlayOptions::new().decision_scope(DecisionScope::PerEncounter)` を渡します．
そのまま動く総当たり戦は [例](../games.ja.md#例総当たりの繰り返し囚人のジレンマ)（`examples/ipd_tournament.rs`）にあります．

## 9. 決定論性と RNG

`decide` はエンジンのステップの RNG（`ctx.rng`，`derive_seed(seed, &[1])` から）を受け取ります．
`UniformRandom` はこれを引き，`Fixed`，`TitForTat`，`DampedBestResponse`，`GradientAscent` は引きません．
ストリームは活性化の順に，1 人のエージェントの中では決定の単位の順に消費されます．
`Mixed`，`LogitChoice`，`RothErev`，GTFT（相手の裏切りの後で `0 < generosity < 1` のとき）は決定ごとに一様な `f64` を 1 回引き，
行動の番号順に累積確率をたどります．同点の乱択（`FictitiousPlay`，`MyopicBestResponse`）は同点の行動について `gen_range` を 1 回引きます．
`learn_encounter` と `learn_step` は RNG を受け取りません．`mixed` も受け取りません．
報告は乱数を引かず規則の状態も変えないので，実行を変えません．
規則の状態の初期化（`build_world` の中）は `derive_seed(seed, &[3, agent, generation])` を使い，
`derive_seed(seed, &[2])` は World の `decision_root` として保存されます．どちらのラベルも予約済みです．
学習は `AgentId` の昇順，決定はスケジューラーの順で行うので，同じ seed なら実行は再現されます．
規則の状態がすべて World にあるので，snapshot からの再開も正確です．

## 10. 期待される動作

繰り返し囚人のジレンマで `TitForTat` と `Fixed(D)` を対戦させると，`TitForTat` は最初のステップで協力し，2 ステップ目から裏切ります．
100 ラウンドの得点は 99 と 104 です（`examples/ipd_pair.rs`）．
範囲外の行動を返す規則は，`Stop` ではステップを止め，`Fallback` では（発生ごとに `game.invalid_action` を 1 件記録して）差し替えられます．

3 社のクールノー（$P = 100 - Q$，単位費用 10，$q \in [0, 100]$．したがって $q^* = 22.5$）が `WholePopulation` の下で `[0, 50, 100]` から $\alpha = 0.5$ の `DampedBestResponse` で学習すると，ステップ 1 は `[0, 50, 100]`，ステップ 2 は `[0, 25, 60]` を出します．
どの生産量も 10 ステップ続けて `1e-9` 動かなくなったら `game_metrics` が止める設定では，実行は 92 ステップで終わり，どの企業も 22.5 から `2e-10` 以内にあります（`examples/cournot.rs`）．

ALLC，ALLD，TFT，GRIM，WSLS のオートマトンを `PerEncounter` で 200 ラウンド総当たりさせると（`examples/ipd_tournament.rs`），
合計は 1800，2008，1999，1999，1900 です．TFT は ALLD に最初のラウンドだけ負け（199 対 204），ほかの相手とは協力し続けます．

## 11. 参考文献

- Axelrod, R. (1984). *The Evolution of Cooperation*. Basic Books.
- Brown, G. W. (1951). Iterative solution of games by fictitious play. In T. C. Koopmans (Ed.), *Activity Analysis of Production and Allocation* (pp. 374–376). Wiley.
- Erev, I., & Roth, A. E. (1998). Predicting how people play games: Reinforcement learning in experimental games with unique, mixed strategy equilibria. *American Economic Review*, 88(4), 848–881.
- Nowak, M., & Sigmund, K. (1993). A strategy of win-stay, lose-shift that outperforms tit-for-tat in the Prisoner's Dilemma game. *Nature*, 364, 56–58.
- Shapley, L. S. (1964). Some topics in two-person games. In M. Dresher, L. S. Shapley, & A. W. Tucker (Eds.), *Advances in Game Theory* (pp. 1–28). Princeton University Press.
- Theocharis, R. D. (1960). On the stability of the Cournot solution on the oligopoly problem. *The Review of Economic Studies*, 27(2), 133–134.
