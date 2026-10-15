[English](games.md) | **日本語**

# 戦略形ゲーム（`socsim-games`）

`socsim-games` は socsim のエンジン上でゲーム理論のシミュレーションを走らせる crate です．
エージェントは**段階ゲーム**の対戦の席に着き，各自が**決定規則**で行動を選び，
**利得規則**が行動の組を利得に変え，規則は起きたことから学習します．
これを毎ステップ，通常の [6フェーズティックループ](architecture.ja.md#6フェーズティックループ) の中で行います．

この crate はライブラリ専用です（`ModulePack` を持たず，`socsim` バイナリには組み込まれていません）．
このページでは，部品，その状態の置き場所，規則が見てよい情報，実行可能なシミュレーションの組み立て方，
組込みの学習則と決定の粒度（[学習則と決定の粒度](#学習則と決定の粒度) を参照），
格子・ネットワーク・よく混ざった集団の上で模倣や繁殖をするエージェントの集団（[集団動学](#集団動学) を参照），
ステップごとの指標を記録する Mechanism `game_metrics`（[指標](#指標) を参照）を説明します．
シミュレーションとは別に，古典的なゲームと有限ゲームのソルバー（Nash 均衡，支配，ミニマックス，ロジット QRE，匿名ゲームの均衡，レプリケータ動学，進化的安定性）も提供します（[有限ゲームを解く](#有限ゲームを解く) を参照）．
実数の行動を持つゲーム（クールノー競争，ベルトラン競争，連続の公共財）には，専用の組込みのゲーム，最適応答反復のソルバー，決定規則，指標があります（[連続ゲーム](#連続ゲーム) を参照）．
シミュレーションは，型で検査される構築器 `GameSimFactory` で組むこと（[`GameSimFactory` による組み立て](#gamesimfactory-による組み立て) を参照）も，`GameTrialFactory` で Scenario ファイルから丸ごと読むこと（[Scenario ファイル](#scenario-ファイル) を参照）もできます．

---

## Cargo feature

| feature | 追加されるもの | socsim への依存 |
|---|---|---|
| *（既定）* | 段階ゲームのトレイト（`StageGame`，`NormalFormGame`，`FiniteGame`，`ContinuousGame`，`AggregateGame`，`AnonymousGame`，`CommonActionSpace`，`DescribeGame`），`MatrixGame`，組込みのゲーム（`catalog`，連続の行動の `continuous`），混合戦略（`mixed`），ソルバー（`solve`．`pure_nash_counts`，レプリケータ動学，`ess_verdict`，連続の `best_response_iteration` を含む），学習則の計算（`learn`），`AggregateSpec`，指標用の行動 ID（`metric_action_ids`），`FiniteSpec` / `ContinuousSpec`（実行時に選ぶ組込みのゲーム．Scenario が使う） | **なし** — 既定のビルドはどの `socsim-*` crate にも依存しません（ソルバーの線形計画のバックエンドは純 Rust の `microlp` crate） |
| `core` | World（`GameWorld`，`Topology`），規則（学習則と連続の規則 `DampedBestResponse` / `GradientAscent` を含む），利得規則，Matching（`FixedPlayers`，`RoundRobin`，`RandomPairs`，`RandomGroups`，`WholePopulation`），ビュー，`build_world`（`InitialActions::Shares` を含む），`uniform_initial_actions`，集団動学の設定の型（`ImitateBest`，`FermiUpdate`，`ProportionalImitation`，`MoranUpdate`，`Update`），指標の Mechanism `GameMetrics` とその連続版 `ContinuousMetrics`（`PayoffTiming` を含む） | `socsim-core` |
| `engine` | `assemble`，`assemble_with_aggregates`，`assemble_with`，`assemble_async`（どれも `SimulationBuilder` を返す），対戦の評価経路（`Pairwise`，`counting_play()`），集団動学の Mechanism，`AsyncPopulationStep` と `AsyncActivation`．`core` を含意 | `socsim-core`，`socsim-engine` |
| `net` | `Topology::network` / `Topology::directed_network` と Matching `NetworkEdges`（[トポロジー](#トポロジーgameworldwith_topology) を参照）．`core` を含意 | `socsim-core`，`socsim-net` |
| `grid` | `Topology::grid`（`GridSpec`）と Matching `GridNeighborhood`．`core` を含意 | `socsim-core`，`socsim-grid` |
| `metrics` | `GameMetrics` の列 `game.action_entropy`（[指標](#指標) を参照）．`core` を含意 | `socsim-core`，`socsim-metrics`（既定の feature を切ったもの．依存の無い `stats` モジュールだけ） |
| `llm` | LLM の決定規則，応答ストア，再生の manifest（[LLM による決定](#llm-による決定) を参照）．`core` を含意．Mechanism `game_llm_decide` と `assemble_with_llm` には `engine` も要る | `socsim-core`，`socsim-llm`（既定の feature だけ．ネットワークなし） |
| `live` | `live_backend_factory()`（キャッシュなしの Ollama → OpenAI のバックエンド）．`llm` を含意 | `llm` と同じ＋`socsim-llm/live` |
| `factory` | `GameSimFactory`，`SchedulerChoice`，`NoMatching`（[`GameSimFactory` による組み立て](#gamesimfactory-による組み立て) を参照）．`engine` を含意 | `socsim-core`，`socsim-engine`，`socsim-runner`，`socsim-config` |
| `config` | `GameTrialFactory`，`RuleRegistry`，`RuleCtor`，`ScenarioConfig`，`ScenarioGame` — Scenario ファイルから読むシミュレーション（[Scenario ファイル](#scenario-ファイル) を参照）．`factory` を含意 | `factory` と同じ＋`toml` crate |

`llm` を有効にしなければ，この crate は `socsim-llm` に依存せず，`metrics` を有効にしなければ `socsim-metrics` に依存せず，`net` / `grid` を有効にしなければ `socsim-net` / `socsim-grid` に依存せず，`factory` を有効にしなければ `socsim-config` と `socsim-runner` に依存しません．
シミュレーションを走らせるには `core` と `engine` が要ります．

```toml
socsim-games = { git = "https://github.com/akitenkrad/rs-social-simulation-tools", branch = "main", package = "socsim-games", features = ["engine"] }
socsim-core   = { git = "https://github.com/akitenkrad/rs-social-simulation-tools", branch = "main", package = "socsim-core" }
socsim-engine = { git = "https://github.com/akitenkrad/rs-social-simulation-tools", branch = "main", package = "socsim-engine" }
```

---

## 最小の例：繰り返し囚人のジレンマ

`Tit-for-Tat` と無条件の裏切り者を 100 ラウンド対戦させます
（`crates/socsim-games/examples/ipd_pair.rs`）．

```rust
use std::sync::Arc;

use socsim_games::{
    assemble, build_world, AssignGroup, AssignKind, Assignment, Fixed, FixedPlayers,
    InitialActions, MatrixGame, NormalFormOutcome, PlayOptions, RuleBook, TitForTat,
};

const ROUNDS: usize = 100;

fn main() {
    // Row player's and column player's payoffs; action 0 = C, 1 = D.
    let game = MatrixGame::bimatrix(
        &[vec![3.0, 0.0], vec![5.0, 1.0]],
        &[vec![3.0, 5.0], vec![0.0, 1.0]],
    )
    .expect("the prisoner's dilemma matrix is valid");

    let mut rules = RuleBook::new();
    rules.register("tit_for_tat", Arc::new(TitForTat::new(0))).unwrap();
    rules.register("fixed_defect", Arc::new(Fixed::new(1))).unwrap();

    let seed = 1;
    let world = build_world(
        game,
        2,                          // n_agents
        (),                         // experiment state X
        InitialActions::Same(0),    // initial held action
        &rules,
        &Assignment::Ordered(vec![
            AssignGroup { kind: AssignKind::Rule("tit_for_tat".to_owned()), count: 1 },
            AssignGroup { kind: AssignKind::Rule("fixed_defect".to_owned()), count: 1 },
        ]),
        ROUNDS,                     // history_len
        ROUNDS as u64,              // t_max
        seed,
    )
    .expect("the world configuration is valid");

    let mut simulation = assemble(
        world,
        FixedPlayers::pairs(&[(0, 1)]),
        rules,
        NormalFormOutcome,
        PlayOptions::new(),
        seed,
    )
    .expect("the simulation configuration is valid")
    .build();

    let mut scores = [0.0, 0.0];
    for _ in 0..ROUNDS {
        simulation.step().unwrap();
        for (score, payoff) in scores.iter_mut().zip(simulation.world().payoffs()) {
            *score += payoff;
        }
    }
    println!("TFT score: {}, Fixed(D) score: {}", scores[0], scores[1]);
}
```

```sh
cargo run -p socsim-games --example ipd_pair --features core,engine
# TFT score: 99, Fixed(D) score: 104
```

`GameWorld::payoffs()` が持つのは**そのステップの**利得です（毎ステップ空にされます）．
累積の得点は呼び出し側で足し合わせます．

---

## 段階ゲーム

段階ゲームは，1 回の対戦の役割と，各役割が取れる行動を定めます．3 つのトレイトが段をなします．

| トレイト | 追加されるもの | メソッド |
|---|---|---|
| `StageGame` | 役割と行動の妥当性 | `type Action`，`n_roles()`，`check_action(role, &action) -> Result<(), ActionError>` |
| `NormalFormGame: StageGame` | 全役割・全行動の組に対する利得 | `payoff(role, profile)`，`payoffs_into(profile, out)`（既定の実装は `payoff` を役割ごとに呼ぶ） |
| `FiniteGame: NormalFormGame<Action = usize>` | 番号付きの有限個の行動 | `n_actions(role)`，`action_label(role, action)`（既定は `"a{action}"`），`is_symmetric()`（既定は `false`） |

`FiniteGame` と並んで，`ContinuousGame: NormalFormGame<Action = Vec<f64>>` が，行動が箱（`bounds(role)`）の中の実数ベクトルであるゲームを表します（[連続ゲーム](#連続ゲーム) を参照）．

規則と利得規則は，必要な段をトレイト境界として宣言します．
`UniformRandom` と `TitForTat` は `FiniteGame` を，`DampedBestResponse` と `GradientAscent` は `ContinuousGame` を，`NormalFormOutcome` は `NormalFormGame` を要求するので，
組み合わせの誤りは実行時ではなくコンパイル時のエラーになります．

### 匿名ゲームと `CommonActionSpace`

集団で行うゲームを表すトレイトがさらに 2 つあります（既定のビルド）．

| トレイト | 意味 | メソッド |
|---|---|---|
| `AnonymousGame: FiniteGame` | プレイヤーの利得は，自分の行動と，ほかのプレイヤーが各行動を**何人**選んだかだけで決まり，誰が選んだかには依らない | `payoff_by_counts(a, counts_others)`：`counts_others[k]` は行動 `k` を選んだ**ほかの**プレイヤーの人数．和は `n_roles() - 1` |
| `CommonActionSpace: StageGame` | 目印．すべての役割が同じ行動の集合から選ぶので，役割が違っても行動をエージェントからエージェントへ写せる | なし |

`payoff_by_counts` はどの行動の組でも `payoff` と一致しなければなりません．
人数で数える評価経路（[`counting_play()`](#シミュレーションの中の匿名ゲームcounting_play)）とソルバー [`pure_nash_counts`](#pure_nash_counts) は，
行動の組を列挙する代わりにこれを評価します．
`CommonActionSpace` は，エージェントの間で行動を写す動学（`imitate_best`，`fermi_update`，`replicator_update`，`moran_update`）が要求します．
これを実装しないゲームと組み合わせるとコンパイルエラーになります．
目印は約束にすぎないので，組み立ての時にも実行時の検査をします．`FiniteGame` ではすべての役割の行動数が等しく，`ContinuousGame` ではすべての役割の成分の数が等しく区間も同一でなければなりません（違えば `GameError::Population`）．
`MatrixGame` と組込みのゲームは有限・連続を問わずすべて `CommonActionSpace` を実装し，catalog の集団用の 4 つのゲームは `AnonymousGame` も実装します．

### `MatrixGame`

`MatrixGame` は組込みの `FiniteGame` です．役割ごとに行優先の利得テンソルを 1 つ持ち，行動は 0 始まりの番号です．

| コンストラクタ | 用途 |
|---|---|
| `MatrixGame::new(n_actions, payoffs)` | 一般の n 役割のテンソル．`payoffs[role]` は行動の組ごとに 1 要素 |
| `MatrixGame::bimatrix(a, b)` | 2 役割．`a[i][j]` / `b[i][j]` が行プレイヤー / 列プレイヤーの利得 |
| `MatrixGame::zero_sum(a)` | 2 役割．列プレイヤーは `-a[i][j]` を受け取る |
| `.with_labels(labels)` | `action_label` 用の役割ごとの行動ラベル |

形の不一致（`Shape`），有限でない利得（`NonFinite`），形の合わないラベル表（`Labels`），
大きすぎるゲーム（`TooLarge`：行動の組が 10⁶ を超えるか，利得の要素が 10⁷ を超える）のとき，
構築は `MatrixError` で失敗します．`is_symmetric()` は構築時に一度だけ計算されます
（2 役割，行動の数が等しく，`payoff(0, (i, j)) == payoff(1, (j, i))`）．
`MatrixGame` は serde で直列化できるので，snapshot に含まれます．

1 つの対戦の役割の数は Matching が決めます（`Matching::supports_roles`）．
`FixedPlayers`，`RoundRobin`，`RandomPairs`，`NetworkEdges`，`GridNeighborhood` は 2 役割，`RandomGroups::new(k)` は `k` 役割，
`WholePopulation` はエージェント 1 人につき 1 役割の対戦を組みます．
Matching が扱えない `n_roles()` のゲームは，`assemble` が `GameError::UnsupportedRoles` で拒否します．
LLM による決定の `assemble_with_llm` も同じです．

### 組込みのゲーム：`catalog`

`socsim_games::catalog` は古典的な 2 人ゲームを 5 つと，集団用のゲームを 4 つ提供します．どれも `FiniteGame` を実装しているので，
`MatrixGame` と同じように `build_world`，組込みの規則，[ソルバー](#有限ゲームを解く) に渡せます．
2 人ゲームでは，`to_matrix()` が同じラベルを持つ等価な `MatrixGame` に変換します．
行動のラベルは ASCII で，協力側を持つゲーム（`C`，`stag`）ではそれが行動 `0` です．

| 型 | 行動 `0, 1, …` | コンストラクタ | 構築時に確かめる条件 |
|---|---|---|---|
| `PrisonersDilemma` | `C`，`D` | `new(T, R, P, S)`；`weak(b)` ＝ `new(b, 1, 0, 0)` | `T > R > P ≥ S`（`P = S` を許すので，`weak(b)` には `b > 1` が要る） |
| `BattleOfSexes` | `B`，`V` | `new(a, b)`；`Default`：`a = [[2, -1], [-1, 1]]`，`b = [[1, -1], [-1, 2]]` | `(B, B)` と `(V, V)` が狭義の均衡で，役割 0 は `(B, B)` を，役割 1 は `(V, V)` を狭義に好む |
| `MatchingPennies` | `H`，`T` | `new(a, b)`；`Default`：`a = [[-1, 1], [1, -1]]`，`b = -a` | ゼロ和（要素ごとに `b == -a`）で，純粋戦略の均衡を持たない |
| `StagHunt` | `stag`，`hare` | `new(a, b)`；`Default`：`a = [[9, 0], [8, 7]]`，`b = [[9, 8], [0, 7]]` | `(stag, stag)` と `(hare, hare)` が狭義の均衡で，`(stag, stag)` はどちらの役割にも `(hare, hare)` より狭義に大きい利得を与える |
| `RockPaperScissors` | `R`，`P`，`S` | `new(win, loss)`；`Default`：`win = loss = 1` | `win > 0` かつ `loss > 0` |

`a[i][j]` / `b[i][j]` は，役割 0 が `i`，役割 1 が `j` を選んだときの役割 0 / 役割 1 の利得です（`MatrixGame::bimatrix` と同じ）．
囚人のジレンマは役割 0 に `[[R, S], [T, P]]` を，役割 1 にその転置を与えます．
標準の `T, R, P, S` は一通りには決まらないので，囚人のジレンマには意図して `Default` を用意していません．利得は必ず書き下します．
既定のマッチング・ペニーでは，硬貨の面が異なるときに役割 0 が勝ちます．
じゃんけんでは勝者が `win`，敗者が `-loss`，引き分けが `0` を受け取ります．
既定の `win = loss = 1` では標準の反対称なゼロ和の形になります（`win ≠ loss` のときはゼロ和ではありません）．

集団用のゲームは対称で，`AnonymousGame` と `CommonActionSpace` を実装し，行動を 2 つ持ちます．

| 型 | 役割 | 行動 `0, 1` | 利得 | コンストラクタと検査 |
|---|---|---|---|---|
| `HawkDove` | 2 | `H`，`D` | $HH = (V-C)/2$，$HD = V$，$DH = 0$，$DD = V/2$（行プレイヤー） | `new(v, c)`：`V > 0`，`C > 0`．`hawk_share_ess()` は $V < C$ なら $V/C$，そうでなければ `1` を返す．`to_matrix()` を持つ |
| `ElFarol` | `n` | `attend`，`stay` | 出席は，出席者数 $A$（自分を含む）が `capacity` 以下なら $+1$，超えれば $-1$．欠席は $0$ | `new(n, capacity)`：`n ≥ 2`，`0 < capacity < n` |
| `MinorityGame` | `n`（奇数） | `a0`，`a1` | 少数派の側のプレイヤーは $+1$，多数派は $0$ | `new(n)`：`n ≥ 3` かつ奇数（同数は起きない） |
| `PublicGoodsFinite` | `k` | `C`，`D` | 群れの拠出者数を $m$（自分を含む）として，$P_C = r\,c\,m/k - c$，$P_D = r\,c\,m/k$ | `new(k, r, cost)`：`k ≥ 2`，`r > 0`，`cost > 0`（`1 < r < k` は求めない） |

タカ・ハトの利得表は Maynard Smith（1982）のものです．$V < C$ のとき，進化的に安定なタカの割合は $V/C$ です．
`ElFarol` は Arthur（1994）のバー問題に従います．原典の $N = 100$ と閾値 60 は定性的な記述で，
$\pm 1$ の利得と，出席者がちょうど `capacity` 人のときの扱い（$+1$）はこの crate の選択です．純粋戦略の均衡は，出席者数が `capacity` に等しい配置だけです．
`MinorityGame` は Challet & Zhang（1997）のゲームです（勝者は 1 点，敗者は 0 点）．
`PublicGoodsFinite` は Hauert, De Monte, Hofbauer & Sigmund（2002）の利得を使います．$r < k$ では拠出しないことが支配戦略です．
`ElFarol` と `MinorityGame` は `AggregateGame` も実装し，公開集計値 `attendance`（`Sum`）を 1 つ持ちます．対戦で行動 `0` を選んだプレイヤーの人数です．
少数派ゲームでは，`attendance` $< n/2$ なら少数派の側は `0`，そうでなければ `1` です．

どのコンストラクタも `Result<_, CatalogError>` を返します．利得に NaN や無限大があれば `NonFinite`，
利得がそのゲームを定める形をしていなければ `Condition(_)` です．
これらの型は serde で直列化でき，逆直列化でも同じ検査が走るので，不正なゲームを snapshot やファイルから読み込むこともできません．

```rust
use socsim_games::catalog::{PrisonersDilemma, RockPaperScissors};
use socsim_games::{FiniteGame, NormalFormGame};

fn main() {
    // T > R > P >= S is checked when the game is built.
    let pd = PrisonersDilemma::new(5.0, 3.0, 1.0, 0.0).unwrap();
    assert!(PrisonersDilemma::new(3.0, 3.0, 1.0, 0.0).is_err()); // T = R

    println!("{} vs {}: {}", pd.action_label(0, 0), pd.action_label(1, 1), pd.payoff(0, &[0, 1]));
    // C vs D: 0

    let rps = RockPaperScissors::default(); // win = 1, loss = 1
    let matrix = rps.to_matrix();           // the same payoffs as a MatrixGame
    println!("{} beats {}: {}", matrix.action_label(0, 1), matrix.action_label(1, 0), matrix.payoff(0, &[1, 0]));
    // P beats R: 1
}
```

---

## 決定規則

`DecisionRule<G, X>` は 1 エージェントの行動を選び，必要ならそのステップの結果から学習します．

```rust
pub trait DecisionRule<G: StageGame, X: ExperimentState>: Send + Sync {
    fn name(&self) -> &str;
    fn init_state(&self, ctx: &RuleInitContext, game: &G) -> Result<X::PolicyState, RuleError>;
    fn decide(
        &self,
        state: &mut X::PolicyState,
        ctx: &DecisionContext<'_, G, X>,
        rng: &mut SimRng,
    ) -> Result<G::Action, RuleError>;
    fn learn_step(
        &self,
        state: &mut X::PolicyState,
        ctx: &LearnContext<'_, G, X>,
    ) -> Result<(), RuleError> { Ok(()) }
    fn learn_encounter(
        &self,
        state: &mut X::PolicyState,
        outcome: &EncounterOutcome<'_, G::Action>,
        ctx: &LearnContext<'_, G, X>,
    ) -> Result<(), RuleError> { Ok(()) }
    fn needs_encounter_learning(&self) -> bool { false }
    fn needs_pair_history(&self) -> bool { false }
    fn mixed(&self, state: &X::PolicyState, role: usize, game: &G) -> Option<Vec<f64>> { None }
}
```

`learn_encounter`，`needs_encounter_learning`，`needs_pair_history` は省略できます．
エージェントが 1 ステップに複数の対戦をするときに意味を持ちます（[対戦ごとの学習](#対戦ごとの学習) と [対の履歴](#対の履歴) を参照）．
`mixed` も省略できます．規則が `role` で次に打つ混合戦略を，状態を変えず乱数も引かずに状態から読んで返すもので，
シミュレーションが混合戦略を報告するときだけ呼ばれます（[混合戦略の報告](#混合戦略の報告) を参照）．

`decide` と学習のメソッドの受け手が `&self` であることに注意してください．規則のオブジェクトは共有されるコードであり，
エージェントについて覚えておくことはすべて，渡される `state: &mut X::PolicyState` に置きます
（[規則の状態と snapshot](#規則の状態と-snapshot) を参照）．

組込みの規則：

| 規則 | 境界 | 振る舞い | 状態のスロット |
|---|---|---|---|
| `Fixed::new(action)` | 任意の `StageGame` | 常に `action` を出す（初期化時に全役割で検証） | `Stateless` |
| `UniformRandom` | `FiniteGame` | 役割の行動から `rng` で一様に引く | `Stateless` |
| `TitForTat::new(coop)` | `FiniteGame`，2 役割，各役割 2 行動 | 初回は `coop`，以後は相手の直前の行動 | `TftState { last_opponent }` |

ほかの組込みの規則 — `Mixed`（固定の混合戦略），`MyopicBestResponse`，`FictitiousPlay`，`LogitChoice`，`RothErev`，
繰り返しゲームのオートマトン `Automaton` — は [学習則と決定の粒度](#学習則と決定の粒度) で説明します．
連続ゲームでは `Fixed::new(vec![..])` が一定の行動ベクトルを出し，`DampedBestResponse` と `GradientAscent` は [連続ゲーム](#決定規則dampedbestresponse-と-gradientascent) で説明します．

規則は `RuleBook` に**種類の名前**で登録します（`register(kind, Arc<dyn DecisionRule<G, X>>)`．
重複した種類は `RuleError::DuplicateKind`）．種類の番号は種類名の昇順で振られるので，登録の順序には依存しません．

### 規則をエージェントに割り当てる

`build_world` は `AssignGroup { kind, count }` の列である `Assignment` を受け取ります．

- `AssignKind::Rule(kind)` — そのエージェントは登録済みの規則 `kind` で動き，各自が `init_state` で自分の状態を得ます．
- `AssignKind::Held` — そのエージェントは規則も状態のスロットも**持たず**，保持している行動（`InitialActions` の値）を出し続けます．
- `Assignment::Ordered(groups)` はグループの順にエージェントの番号を割り当て，
  `Assignment::Shuffled(groups)` は World 初期化のストリームで割り当てを並べ替えます．

`count` の和は `n_agents` に一致しなければなりません（`GameError::AgentCount`）．

### `RuleInitContext`

`init_state` は `RuleInitContext { agent, kind, generation, init_seed }` を受け取ります．
`init_seed` は `derive_seed(seed, &[3, agent, generation])` で，状態の初期値を乱数で決めるためのエージェントごとのストリームです
（`build_world` が作るエージェントの `generation` は `0`）．
初期化の乱数は `SimRng::from_seed(init_seed)` から引いてください．

---

## 利得規則

`OutcomeRule<G, X>` は 1 つの対戦について役割ごとの利得を書き込みます．

```rust
pub trait OutcomeRule<G: StageGame, X: ExperimentState>: Send + Sync {
    fn name(&self) -> &str;
    fn outcome(
        &self,
        ctx: &OutcomeContext<'_, G, X>,
        encounter: EncounterRef<'_>,
        actions: &[G::Action],
        payoffs: &mut [f64],
    ) -> socsim_core::Result<()>;
}
```

`NormalFormOutcome`（任意の `NormalFormGame` 用）は `game.payoffs_into(actions, payoffs)` を呼ぶだけです．
利得が行動の組以外にも依存するときは独自の利得規則を書きます．
`OutcomeContext` は `t()`，`game()`，`x()`，`held(agent)`，`topology()` を，
`EncounterRef` は対戦の `id`，`seats`（役割ごとのエージェント），`credit` を与えます．
例えば `X` に置いた評判で利得を割り引く場合：

```rust
impl OutcomeRule<MatrixGame, ReputationState> for ReputationDiscount {
    fn name(&self) -> &str { "reputation_discount" }
    fn outcome(
        &self,
        ctx: &OutcomeContext<'_, MatrixGame, ReputationState>,
        encounter: EncounterRef<'_>,
        actions: &[usize],
        payoffs: &mut [f64],
    ) -> socsim_core::Result<()> {
        ctx.game().payoffs_into(actions, payoffs);
        for (role, payoff) in payoffs.iter_mut().enumerate() {
            *payoff *= ctx.x().reputation[encounter.seats[role].0 as usize];
        }
        Ok(())
    }
}
```

対戦の `credit` は，どの席が利得を受け取るかを表します．

- `Credit::All` — どの席も，自分の利得をエージェントの `payoff` に足し，`n_played` を 1 増やします．`GridNeighborhood` 以外の Matching はすべてこれを使います．
- `Credit::Center` — 席 0（«中心»）だけが利得を受け取ります．ほかの席は対戦に加わりますが，その利得はエージェントに足されません．
  `GridNeighborhood` がこれを使います．各エージェントは近傍 1 人につき 1 回，中心として対戦するので，
  近傍 1 人につきちょうど 1 回（自己対戦があればもう 1 回）利得を受け取り，同じ組で 2 回受け取ることはありません．

credit が何であっても，対戦の表は全席の利得を保持し，`game_metrics` の行動の割合は全席を数えます．

---

## 実験状態 `X`

`GameWorld<G, X>` は**実験状態** `X: ExperimentState` について総称的です．
`X` はモデル固有のもの — 評判，シグナル，予算，独自の Matching の記録 — をすべて置く場所です．
既定は `X = ()` で，組込みの規則の状態を使います．

```rust
pub trait ExperimentState: Send + Sync + 'static {
    type PolicyState: Clone + Send + Sync + Debug + 'static;
    type View<'a> where Self: 'a;
    fn view_for(&self, me: AgentId) -> Self::View<'_>;
}
```

- **`PolicyState`** — エージェントごとの規則の状態の型．規則で動くエージェント 1 人につき 1 つの値が World に保存されます．
- **`View<'a>` / `view_for(me)`** — エージェントが既定で `X` から見てよいもの（[情報構造](#情報構造) を参照）．

### `PolicyState` と `Slot`

組込みの規則は利用者の `PolicyState` の型を知りません．`Slot<T>` トレイト（`slot`，`slot_mut`，`from_slot`）を通して自分の状態に届きます．
`Fixed`，`UniformRandom`，`Mixed`，`MyopicBestResponse` は `Slot<Stateless>` を，`TitForTat` は `Slot<TftState>` を要求し，
学習則は `Slot<BeliefState>`（`FictitiousPlay`），`Slot<EstimateState>`（`LogitChoice`），
`Slot<PropensityState>`（`RothErev`），`Slot<AutomatonPolicyState>`（`Automaton`）を，連続の規則 `DampedBestResponse` と `GradientAscent` は `Slot<ContinuousState>` を要求します．
`BuiltinPolicyState`（`X = ()` の `PolicyState`）はこれらをすべて実装しています．
組込みの規則と独自の規則を混ぜるには，これを包みます．

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
enum MyPolicyState {
    Builtin(BuiltinPolicyState),
    Discriminator { threshold: f64, seen: u32 },
}

impl Slot<TftState> for MyPolicyState {
    fn slot(&self) -> Option<&TftState> {
        match self { Self::Builtin(s) => s.slot(), _ => None }
    }
    fn slot_mut(&mut self) -> Option<&mut TftState> {
        match self { Self::Builtin(s) => s.slot_mut(), _ => None }
    }
    fn from_slot(value: TftState) -> Self {
        Self::Builtin(<BuiltinPolicyState as Slot<TftState>>::from_slot(value))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ReputationState { reputation: Vec<f64> }

impl ExperimentState for ReputationState {
    type PolicyState = MyPolicyState;
    type View<'a> = &'a [f64];
    fn view_for(&self, _me: AgentId) -> Self::View<'_> { &self.reputation }
}
```

`PolicyState` が実装していないスロットを要求する組込みの規則を使うと，コンパイルエラーになります．
型としてはスロットがあっても保存されている値の variant が違う場合，規則は `RuleError::MissingSlot` を返します．

実行中に `X` を変えるには，独自の `Mechanism<GameWorld<G, X>>` をビルダーに足し，`GameWorld::x_mut()` から書き込みます．

---

## 規則の状態と snapshot

再開に要る状態は，すべて **World の中**にあります．

| 状態 | フィールド（読み出し） |
|---|---|
| エージェントごとの規則の状態 | `PolicyTable`（`world.policies()`，`.state(agent)`，`.kind(agent)`） |
| 保持している行動 | `world.held()` |
| そのステップの対戦・行動・利得 | `PlayArena`（`world.plays()`） |
| そのステップのエージェントごとの利得と対戦数 | `world.payoffs()`，`world.n_played()` |
| エージェントごとのステップ履歴（長さ `history_len` の環） | `world.history(agent)` |
| 組ごとの対戦の履歴（組ごとに長さ `history_len` の環．規則が求めたときだけ） | `DecisionContext::pair_history` から（[対の履歴](#対の履歴) を参照） |
| Matching の状態（`MatchingState`．例えば `RoundRobin` の cursor） | World とともに直列化される |
| トポロジー（格子かネットワーク） | `GameWorld::with_topology` で設定する．World とともに直列化される（格子は `GridSpec` として） |
| 実験状態 | `world.x()` |

3 つの Mechanism（`game_match`，`game_decide`，`game_play`），集団動学，`RuleBook`，`Matching`，`OutcomeRule` は，
再開後の実行に影響する状態を**持ちません**．これらはコードであり，シミュレーションを組み直すときに改めて渡します．
このため，エンジンの [snapshot と再開](architecture.ja.md#スナップショット保存と再開) はゲームのシミュレーションでも正確です．

```rust
let snapshot = partial.snapshot();          // GameWorld + RNG position
let mut resumed = build_simulation(/* any seed */);
resumed.restore(snapshot);                  // continues bit-identically
```

`GameWorld` は `G`，`G::Action`，`X`，`X::PolicyState` が `Serialize`/`Deserialize` のときに直列化できます．
したがって `X` と `PolicyState` には復元できるデータだけを置いてください（ネットワークのクライアント，ファイルハンドル，キャッシュは置かない）．
また，実行ごとに変わるデータを規則・Matching・利得規則の構造体に持たせないでください．保存されないので，再開した実行が食い違います．

---

## 情報構造

規則は `GameWorld` を直接ではなく，コンテキストを通して見ます．既定のメソッドは標準的な繰り返しゲームの情報構造を与えます．

**`DecisionContext`**（`decide` に渡される）：

| メソッド | 返すもの |
|---|---|
| `t()`，`me()`，`unit()`，`scope()` | ステップ，自分の id，決定の単位（既定では `DecisionUnit::Step`）とその `DecisionScope` |
| `role()`，`encounter()` | その決定での自分の役割と対戦（`PerAgentRole` では `encounter()` は `None`） |
| `opponents()` | **同じ対戦の**ほかのエージェント（`PerAgentRole` では，その役割で着いた対戦すべての相手） |
| `my_history()` | 自分の過去の `StepRecord`（行動，利得，`n_played`） |
| `my_held()` | この決定の前の自分の保持行動．初期行動か，最後に選んだ行動 |
| `opponent_history(id)` | `id` の行動の履歴．**`id` が現在の相手でなければ `None`** |
| `pair_history(id)` | `id` との過去の対戦．対の履歴が有効で，かつ `id` が現在の相手でなければ `None` |
| `game()` | 段階ゲーム |
| `view()` | `X::view_for(me)` |
| `omniscient()` | 制限のないデータ（後述） |

これらが決定の粒度によってどう変わるかは [決定の粒度](#決定の粒度) にまとめてあります．

**`LearnContext`**（`learn_step` と `learn_encounter` に渡される）：`t()`，`me()`，`scope()`，`record()`（このステップの自分の記録），`my_history()`，
`outcomes()`（このステップの自分の対戦を `EncounterId` の順に．各 `EncounterOutcome` は `role`，`seats`，`actions`，`payoffs`，`credit` を持つ），
`game()`，`view()`，`omniscient()`．

`X` が何を見せるかは `view_for(me)` で決めます．不完備情報のゲームなら `me` の私的シグナルだけを，
間接互恵のモデルなら公開の評判のベクトルを返す，といった具合です．

**`omniscient()`** は `OmniscientContext` を返し，全エージェントの id・保持している行動・履歴，`X` 全体，
そして `DecisionContext` から得た場合は（有効なら）任意の 2 人の組の履歴（`pair_history(a, b)`）を読めます．
これを呼ぶと**情報構造の保証がなくなります**．全知であることを意図した規則にだけ使ってください．
また Decision の間は，同じステップで先に行動したエージェントがすでに保持行動を更新しているため，
`held` が活性化の順序に依存することがあります．

---

## Matching

`Matching<G, X>` は Decision の先頭でエージェントを対戦の席に着けます．

```rust
pub trait Matching<G: StageGame, X: ExperimentState>: Send + Sync {
    fn name(&self) -> &str;
    fn check(&self, ctx: &MatchContext<'_, G, X>) -> Result<(), GameError>;
    fn supports_roles(&self, n_roles: usize) -> bool { n_roles == 2 }
    fn encounter_bounds(&self, ctx: &MatchContext<'_, G, X>) -> Option<EncounterBounds> { None }
    fn allows_multi_role_per_step(&self) -> bool { false }
    fn implicit(&self) -> Option<ImplicitPattern> { None }
    fn init_state(&self, ctx: &MatchContext<'_, G, X>) -> MatchingState { MatchingState::None }
    fn encounters_into(
        &self,
        ctx: &MatchContext<'_, G, X>,
        state: &mut MatchingState,
        rng: &mut SimRng,
        out: &mut ArenaWriter<'_, G::Action>,
    ) -> socsim_core::Result<()>;
}
```

`encounters_into` は `out.push(&seats, credit)` ごとに対戦を 1 つ積みます
（`seats` は役割ごとに 1 エージェント．`credit` は `Credit::All` か `Credit::Center`．[利得規則](#利得規則) を参照）．
省略できる 4 つのメソッドは，Matching の性質を `assemble` に伝えます．

| メソッド | 既定 | 意味 |
|---|---|---|
| `supports_roles(n)` | `n == 2` | 席を組める役割の数．それ以外は `UnsupportedRoles` で拒否される |
| `encounter_bounds(ctx)` | `None` | 1 ステップにエージェントが利得を受け取る対戦の数の最小と最大（`EncounterBounds { min, max }`）．1 ステップの利得の範囲を求めるのに使う（[組み立て時の検査](#組み立て時の検査) を参照） |
| `allows_multi_role_per_step()` | `false` | `true` なら，`DecisionScope::PerStep` でも 1 ステップに複数の役割を持てる．そのエージェントは 1 回だけ決定し，その行動をすべての席で使う |
| `implicit()` | `None` | `game_play` が対戦の表を作らずにたどれる型（`ImplicitPattern`）（[対戦を実体化しない評価](#対戦を実体化しない評価) を参照） |

`check` は `assemble` の中で一度だけ走り，Matching が World と合っているかを検査します．
使われている [決定の粒度](#決定の粒度) は `ctx.decision_scope()` でわかります．
組込みの `FixedPlayers::pairs(&[(0, 1), …])` は毎ステップ同じ組を対戦させ，
未知のエージェント，2 回現れるエージェント，役割がちょうど 2 でないゲームを拒否します．
組込みの [`RoundRobin`](#roundrobin) は，毎ステップすべての 2 人の組を対戦させます．
集団用の Matching — `RandomPairs`，`RandomGroups`，`WholePopulation`，`NetworkEdges`，`GridNeighborhood` — は [集団用の Matching](#集団用の-matching) で説明します．
あるステップで席に着かなかったエージェントは，そのステップでは決定も対戦もしません．

独自の Matching が要する状態は，Matching の構造体ではなく **`X` に**置きます（独自の Mechanism で更新します）．
`MatchingState` は閉じた enum で（`None` と，組込みの総当たり用の `RoundRobin { cursor }`），Matching の構造体の中の状態は snapshot に含まれないからです．

---

## 1 ステップの流れ

`assemble` は 3 つの Mechanism をこの順に登録します．

| Mechanism | フェーズ | 行うこと |
|---|---|---|
| [`game_match`](mechanisms/game-match.ja.md) | PreStep，Decision | PreStep：対戦の表，`payoff`，`n_played` を空にする．Decision：そのステップの対戦を `Matching` に組ませる． |
| [`game_decide`](mechanisms/game-decide.ja.md) | Decision，Reward | Decision：席に着いた各エージェントの規則が決定の単位ごとに `decide` する（Held なら保持行動を使う）．行動は `check_action` で検査する．Reward：規則で動く全エージェントの `learn_encounter`（規則が求めるとき，対戦ごとに）と，その後の `learn_step`．`PlayOptions::report_mixed(true)` なら，最後に各規則の学習後の混合戦略を記録する（[混合戦略の報告](#混合戦略の報告) を参照）． |
| [`game_play`](mechanisms/game-play.ja.md) | Interaction | 全席が埋まっていることを検査し，`OutcomeRule` で利得を計算し，（有効なら）対戦を対の履歴に追記し，`PayoffAggregation` でエージェントごとに集計し，全エージェントの `StepRecord` をステップ履歴に追記する．`assemble_with_aggregates` で組み立てたときは，最後にゲームの公開集計値を 1 行にまとめる（[公開集計値](#公開集計値aggregategame) を参照）． |

同じフェーズの Mechanism は登録順に動くので，`game_match` が席を決めてから `game_decide` が行動を尋ねます．1 ステップは次のとおりです．

```
PreStep      game_match   reset arena / payoff / n_played
Decision     game_match   Matching → encounters
             game_decide  decide + check_action → actions, held
Interaction  game_play    OutcomeRule → payoffs → pair history → aggregation → history
                          (→ aggregate row, under assemble_with_aggregates)
Reward       game_decide  learn_encounter (per encounter) + learn_step (rule state)
                          (→ post-learning mixed reports, with report_mixed)
             dynamics     (assemble_with only) imitation / selection → held
PostStep     game_metrics (optional, added by you) → one "game" row of metrics
```

`assemble_with` は 4 つ目の Mechanism として集団動学（`imitate_best`，`fermi_update`，`replicator_update`，`moran_update` のどれか）を登録できます．
これは Reward で `game_decide` の後に動くので，学習はいつも模倣や選択より先です．[集団動学](#集団動学) を参照してください．
[実体化しない経路](#対戦を実体化しない評価) を使うときは，`game_match` は Decision で対戦を書かず，
`game_play` は対戦の表の代わりに Matching の型をたどります．

feature `llm` では，`assemble_with_llm` が 4 つ目の Mechanism [`game_llm_decide`](mechanisms/game-llm-decide.ja.md)（Decision，Reward）を
`game_decide` と `game_play` の間に登録します．これは種類が LLM の規則であるエージェントの席を埋めます（[LLM による決定](#llm-による決定) を参照）．

`assemble` の後に足した Mechanism（返されたビルダーの `.add_mechanism(...)`）は，各フェーズでゲームの Mechanism の後に動きます．
したがって Reward フェーズの Mechanism はそのステップの利得を見られます．
指標の Mechanism [`game_metrics`](mechanisms/game-metrics.ja.md) もこの方法で足します．
これは PostStep で上のすべての後に動き，終わったステップを読みます（[指標](#指標) を参照）．

---

## 組み立て：`build_world` と `assemble`

```rust
pub fn build_world<G: StageGame, X: ExperimentState>(
    game: G,
    n_agents: usize,
    x: X,
    initial_held: InitialActions<G::Action>,   // Same(a) | PerAgent(vec) | Shares(vec)
    rules: &RuleBook<G, X>,
    assign: &Assignment,
    history_len: usize,
    t_max: u64,
    seed: u64,
) -> Result<GameWorld<G, X>, GameError>;

pub fn assemble<G, X, M: Matching<G, X>, O: OutcomeRule<G, X>>(
    world: GameWorld<G, X>,
    matching: M,
    rules: RuleBook<G, X>,
    outcome: O,
    opts: PlayOptions<G::Action>,
    seed: u64,
) -> Result<socsim_engine::SimulationBuilder<GameWorld<G, X>>, GameError>;

pub fn assemble_with_aggregates<G: AggregateGame, X, M: Matching<G, X>, O: OutcomeRule<G, X>>(
    world: GameWorld<G, X>,
    matching: M,
    rules: RuleBook<G, X>,
    outcome: O,
    opts: PlayOptions<G::Action>,
    seed: u64,
    aggregate_history_len: usize,
) -> Result<socsim_engine::SimulationBuilder<GameWorld<G, X>>, GameError>;

pub fn assemble_with<G, X, M: Matching<G, X>, P: PlayPath<G, X>, D: PopulationDynamics<G, X>>(
    world: GameWorld<G, X>,
    matching: M,
    rules: RuleBook<G, X>,
    play: P,        // Pairwise::new(outcome) | counting_play(), optionally .with_aggregates(n)
    dynamics: D,    // NoDynamics | ImitateBest | FermiUpdate | ProportionalImitation | MoranUpdate
    opts: PlayOptions<G::Action>,
    seed: u64,
) -> Result<socsim_engine::SimulationBuilder<GameWorld<G, X>>, GameError>;
```

`assemble(world, matching, rules, outcome, opts, seed)` は
`assemble_with(world, matching, rules, Pairwise::new(outcome), NoDynamics, opts, seed)` と同じで，
`assemble_with_aggregates(.., seed, n)` は評価経路を `Pairwise::new(outcome).with_aggregates(n)` にしたものです．
`PlayPath` と `PopulationDynamics` は封印されています．評価経路は `Pairwise<O>`（どの対戦も `OutcomeRule` で評価する），
`counting_play()` が返す `Counting`（匿名ゲーム．[シミュレーションの中の匿名ゲーム](#シミュレーションの中の匿名ゲームcounting_play) を参照），
およびそれらを `.with_aggregates(history_len)` で包んだものです．動学は `NoDynamics` と [集団動学](#集団動学) の 4 つの型です．

`build_world` の検査：`history_len > 0`（`InvalidHistoryLength`），割り当ての `count` の和と `PerAgent` の長さが `n_agents` に一致（`AgentCount`），
初期行動がすべての役割で `check_action` を通る，割り当てた種類がすべて `RuleBook` にある（`RuleError::UnknownKind`），各 `init_state` が成功する．

`assemble` の検査：Matching がゲームの役割の数を扱える（`UnsupportedRoles`），World にある規則の種類がすべて `RuleBook` にある，
`OnInvalidAction::Fallback` のとき使われている種類すべてに全役割で妥当な代替行動がある，
`report_mixed(true)` と `DecisionScope::PerEncounter` を組み合わせていない（`GameError::Metrics`），`Matching::check`．
`assemble_with_aggregates` は同じ検査に加えて `aggregate_history_len > 0` を求めます（`InvalidAggregateHistoryLength`）．
これは `assemble` に [公開集計値](#公開集計値aggregategame) の表を足したものです．
`assemble_with` も同じ検査をし，集団動学を渡したときは [組み立て時の検査](#組み立て時の検査) も行います（エラーは `GameError::Population`）．
どの関数もエンジンのシードを `derive_seed(seed, &[1])` に設定してビルダーを返すので，`build()` の前にスケジューラー，Recorder，追加の Mechanism を足せます．
2 つの関数には**同じ** `seed` を渡してください．返されたビルダーで `.seed(...)` を呼ぶと，エンジンのストリームが上書きされます．

### `PlayOptions`

```rust
PlayOptions::new()                                     // Sum + Stop + PerStep
    .payoff_aggregation(PayoffAggregation::Mean)        // Sum (default) | Mean
    .on_invalid_action(OnInvalidAction::Stop)           // Stop (default) | Fallback(map)
    .decision_scope(DecisionScope::PerEncounter)        // PerStep (default) | PerAgentRole | PerEncounter
    .report_mixed(true)                                 // false (default) | true
    .materialize(true)                                  // false (default) | true
```

- **`PayoffAggregation`** — 1 ステップに複数の対戦があるとき，エージェントの利得を `payoffs()[agent]` にどうまとめるか．
  `Sum`（既定）か `Mean`（`n_played` で割る．対戦しなかったエージェントは `0` のまま）．
- **`OnInvalidAction`** — 規則が自分の役割で `check_action` を通らない行動を返したときの扱い．
  - `Stop`（既定）：ステップがエラーで失敗します．
  - `Fallback(BTreeMap<kind, action>)`：行動を種類ごとの代替行動に差し替え，イベント `game.invalid_action` を
    payload `{ "agent", "kind", "role", "action" }`（拒否された行動を `Debug` 形式で）とともに記録します．
- **`DecisionScope`** — 1 ステップに複数の対戦の席に着いたエージェントが何回決定するか．[決定の粒度](#決定の粒度) を参照．
- **`report_mixed`** — `true` なら，`game_decide` が各 Reward フェーズの終わりに，規則で動く全エージェントの学習後の混合戦略を記録します．
  `GameWorld::reported_mixed` で読めます．[混合戦略の報告](#混合戦略の報告) を参照．`DecisionScope::PerEncounter` との組み合わせは拒否されます．
- **`materialize`** — `true` なら，対戦の表を作らずに評価できるステップでも，`game_play` に対戦の表を作らせます．
  [対戦を実体化しない評価](#対戦を実体化しない評価) を参照．

### 初期行動：`InitialActions` と `uniform_initial_actions`

```rust
InitialActions::Same(0)                            // every agent holds action 0
InitialActions::PerAgent(vec![0, 1, 1])            // agent i holds the i-th action
InitialActions::Shares(vec![(0, 0.9), (1, 0.1)])   // exact shares, laid out at random
```

`Shares` は，各行動をとるエージェントの人数を最大剰余法で決めます．
まず各行動に `floor(n * share)` 人を割り当て，残ったエージェントを端数の大きい行動から 1 人ずつ割り当てます．端数が同じなら，先に書いた組が取ります．
エージェントが 100 人で上の比なら，ちょうど 90 人が行動 0 をとります．
そのうえで行動を**書いた順**に並べ，`derive_seed(seed, &[0, 2])` で並べ替えます．このストリームは `Assignment::Shuffled` が使う `[0]` とは独立です．
各比は有限かつ非負で，比の和は $10^{-9}$ の許容差で 1 に一致しなければなりません．各行動はすべての役割について `check_action` を通らなければならず，同じ行動を 2 回書くことはできません．
これに反すると `GameError::Config` になります（行動が不正なときは `GameError::Action`）．

並べ方は書いた順から始まるので，同じ seed でも，組の順によってどのエージェントがどの行動をとるかが変わります．
例 `nowak_may` は裏切り者を先に書き，同じストリームで並べ替えます．したがって `Shares(vec![(1, 0.1), (0, 0.9)])` はその初期の格子を正確に再現し，`Shares(vec![(0, 0.9), (1, 0.1)])` は同じ seed で別の格子になります．
Scenario の `initial_shares` は常に行動を添字の順に並べる（[`[world.population]`](#worldpopulation) を参照）ので，Scenario の経路も例の配置を再現しません．
どちらの配置が正しいということはありません．比べるときはセルごとではなく，seed をまたいで比べてください．

連続ゲームには，割り当てる行動の添字がありません．
`uniform_initial_actions(&game, n_agents, seed)` は，各エージェントの行動の各成分を，役割 0 の範囲の閉区間から一様に引きます．エージェントの順，1 人の中では成分の順に，同じストリーム `[0, 2]` から引きます．
結果は `InitialActions::PerAgent` に渡します．

```rust
let initial = uniform_initial_actions(&game, 3, seed);   // Vec<Vec<f64>>, one action per agent
let world = build_world(game, 3, (), InitialActions::PerAgent(initial), &rules, &assign, 4, 100, seed)?;
```

---

## 学習則と決定の粒度

`Fixed`，`UniformRandom`，`TitForTat` のほかに，この crate は古典的な学習則と繰り返しゲームのオートマトン，総当たりの Matching，
そして**決定の粒度**（decision scope）を提供します．決定の粒度は，1 ステップに複数の対戦の席に着いたエージェントが何回決定するかを定めます．
学習の計算そのものは `socsim_games::learn`（仮想プレイ，Roth–Erev，ロジット選択，オートマトン）にあり，
これは既定のビルドに含まれ，どの `socsim-*` crate にも依存しません．以下の `DecisionRule` の包み（feature `core`）が，その状態を World に置きます．

### 規則の一覧

最後の列の検査はすべて `init_state` の中で行われます．したがって設定を誤った規則は，シミュレーションが始まる前に
`build_world` が（`GameError::Rule(RuleError::Unsupported(_))` として）拒否します．

| 規則 | 構築 | 状態のスロット（`BuiltinPolicyState` の variant） | 有効な条件（満たさなければ構築時に拒否） |
|---|---|---|---|
| `Mixed` | `Mixed::new(q)` — 行動の番号順に並べた固定の混合戦略 | `Stateless` | 各 `q[i]` が有限で `≥ 0`．`Σ q = 1`（許容誤差 `1e-12`）．`q.len()` が**すべての**役割の行動数に等しい |
| `MyopicBestResponse` | `MyopicBestResponse::new()`，`.tie_break_lowest()`（既定）/ `.tie_break_random()`，`.first_action(a)`（既定 `0`） | `Stateless` | `first_action` がすべての役割で妥当な行動 |
| `FictitiousPlay` | `FictitiousPlay::new()`，`.initial_counts(vec)`（既定はすべて 0），`.tie_break_lowest()`（既定）/ `.tie_break_random()` | `BeliefState`（`Belief`） | 役割がちょうど 2．初期カウントは有限で `≥ 0`，両方の役割について相手の行動 1 つにつき 1 つ |
| `LogitChoice` | `LogitChoice::new(lambda)`，`.estimate(EstimateMode::LastOpponent \| EmpiricalFrequency)`（既定 `EmpiricalFrequency`） | `EstimateState`（`Estimate`） | 役割がちょうど 2．`lambda` が有限で `≥ 0` |
| `RothErev` | `RothErev::new(phi, epsilon, initial_propensity)` | `PropensityState`（`Propensity`） | `0 ≤ phi < 1`，`0 ≤ epsilon < 1`，`initial_propensity > 0`（いずれも有限）．すべての役割・すべての行動の組で利得が有限かつ `> 0` |
| `Automaton` | `Automaton::new(kind, coop)`，`kind` は `AutomatonKind::{AllC, AllD, Grim, Wsls, Tft, Gtft { generosity }}` のいずれか | `AutomatonPolicyState`（`Automaton`） | すべての役割の行動数がちょうど 2．`coop` が `0` か `1`．GTFT の `generosity` が `[0, 1]` |
| `TitForTat`（従来どおり） | `TitForTat::new(coop)` | `TftState`（`TitForTat`） | 2 役割，各役割 2 行動，`coop` が妥当 |

`BuiltinPolicyState`（`X = ()` の `PolicyState`）は，表のすべてのスロットについて `Slot<_>` を実装しています．
独自の `PolicyState` を使うときは，[`PolicyState` と `Slot`](#policystate-と-slot) のように，登録する規則のスロットについて `Slot<T>` を実装してください．
4 つの状態の型は `global()` と `by_opponent()`（`(AgentId, 状態)` の列．`AgentId` の昇順）を持ち，`world.policies().state(agent)` から中身を確かめられます．

`TitForTat` の動作は従来どおりです．エージェントごとに `last_opponent` を 1 つだけ持ち，そのエージェントの対戦ごとに `EncounterId` の順で上書きします．
相手ごとに別々に覚える tit-for-tat が要るときは，`DecisionScope::PerEncounter` のもとで `Automaton::new(AutomatonKind::Tft, coop)` を使ってください．

`MyopicBestResponse` は，相手の直近の行動に対して最適応答します．何を読むかは [決定の粒度](#決定の粒度) によって変わります．

- `PerStep` — 現在の各相手が直前のステップに出した行動．ステップ履歴から読みます．
- `PerEncounter` — その対戦の相手について [対の履歴](#対の履歴) に記録された最後の行動．
- `PerAgentRole` — その役割の現在の相手のうち記録のある相手それぞれについて，対の履歴の最後の行動．
  規則は，それらの行動の一様な混合に対する期待利得を最大にします．

使える行動がないうち（最初のステップ，一度も対戦していない相手，`PerStep` で直前のステップに対戦しなかった相手）は `first_action` を出します．
この規則は `needs_pair_history() == true` を返すので，割り当てると対の履歴が作られます．
`PerAgentRole` と `PerEncounter` では 2 人ゲームだけに対応し，それ以外のゲームでは `RuleError::Unsupported` でステップが止まります．

### 仮想プレイ

各エージェントは**信念**，つまり相手の行動ごとのカウントを持ちます．
`counts()` は規則が使う正規化しないベクトルで，初期の（仮想の）カウントに，観測した相手の行動 1 回につき 1 を足したものです．
`empirical_counts()` は観測だけを数えます．規則はカウントへの最適応答，すなわち `Σ_j counts[j] · u(a, j)` を最大にする行動を出します．
カウントがすべて 0 なら，すべての行動が同点です．同点は `f64` の完全一致で判定し，`TieBreak::LowestIndex`（既定．乱数を使わない）か
`TieBreak::Random`（同点の行動について `gen_range` を 1 回）で解決します．仮想プレイは **2 人ゲーム**に限ります．

更新は**同時**です．各エージェントは直前のステップまでのカウントで決定し，観測はそのステップの対戦が終わってから（Reward で）足されます．
交互に更新する版は用意していません．2 つの更新の順序は別の力学になるからです．
Shapley の 3×3 のゲームでは，同時更新の仮想プレイは 6 つの行動の組を永遠に巡り，同じ組が続く長さは前の組の約 1.47 倍ずつ伸びていきます．
一方，2 人目が 1 人目の更新済みのカウントに応答するように交互に更新すると，各組が 1 ステップずつしか続かない 3 ステップの短い周期に落ち込みます．

この計算はシミュレーションなしでも動かせます（既定のビルド．乱数のために `rand = "0.8"` を足す）．

```rust
use rand::{rngs::StdRng, SeedableRng};
use socsim_games::learn::fictitious::{simultaneous_step, FictitiousBelief};
use socsim_games::MatrixGame;

fn main() {
    // Shapley's 3×3 game.
    let game = MatrixGame::bimatrix(
        &[vec![1.0, 0.0, 0.0], vec![0.0, 1.0, 0.0], vec![0.0, 0.0, 1.0]],
        &[vec![0.0, 0.0, 1.0], vec![1.0, 0.0, 0.0], vec![0.0, 1.0, 0.0]],
    )
    .unwrap();
    let mut row = FictitiousBelief::with_defaults(&game, 0).unwrap();
    let mut col = FictitiousBelief::with_defaults(&game, 1).unwrap();
    let mut rng = StdRng::seed_from_u64(0); // unused with lowest-index ties

    // Lengths of the runs of identical action pairs.
    let mut runs: Vec<((usize, usize), u32)> = Vec::new();
    for _ in 0..200 {
        let pair = simultaneous_step(&game, &mut row, &mut col, &mut rng).unwrap();
        match runs.last_mut() {
            Some((last, n)) if *last == pair => *n += 1,
            _ => runs.push((pair, 1)),
        }
    }
    let first: Vec<_> = runs.iter().take(9).collect();
    println!("{first:?}");
    // [((0, 0), 1), ((0, 2), 2), ((2, 2), 3), ((2, 1), 5), ((1, 1), 8),
    //  ((1, 0), 12), ((0, 0), 18), ((0, 2), 27), ((2, 2), 40)]
}
```

### Roth–Erev の強化学習

各エージェントは自分の行動ごとに傾向 `q_j > 0` を持ち（初期値はすべて `initial_propensity`），行動 `j` を確率 `q_j / Σ_k q_k` で選びます．
行動 `k` を出して利得 `x` を得たら，3 パラメータの規則ですべての傾向を更新します．

```
q_j ← (1 − φ) · q_j + E_j,     E_j = (1 − ε) · x        if j = k
                                E_j = ε · x / (N − 1)    otherwise
```

`φ`（`phi`）は忘却，`ε`（`epsilon`）は試行，`N` は自分の行動数です（`N = 1` なら利得はすべて唯一の行動に入る）．
参照点は差し引きません．利得をそのまま使うので，ゲームの利得はすべて正でなければならず（構築時に検査），
有限かつ正でない実現利得（独自の利得規則なら起こりうる）はステップをエラーで止めます．
0 や負の利得を持つゲームを使うときは，構築する前に利得をずらしてください．

### ロジット選択

`LogitChoice` は相手の過去の行動から自分の各行動の利得を見積もり，行動 `a` を `exp(λ · û(a))` に比例する確率で出します．
見積もりは，相手の直近の行動に対する利得（`EstimateMode::LastOpponent`）か，
相手の行動の経験頻度に対する期待利得（`EstimateMode::EmpiricalFrequency`．既定）です．
観測がひとつもないうちは見積もりがすべて 0 なので，選択は一様になります．`λ = 0` なら常に一様です．
softmax は指数をとる前に最大の見積もりを引くので，`λ` が大きくてもあふれません．ロジット選択は **2 人ゲーム**に限ります．

```rust
use rand::{rngs::StdRng, SeedableRng};
use socsim_games::learn::logit::{EstimateMode, LogitState};
use socsim_games::learn::roth_erev::RothErevState;
use socsim_games::MatrixGame;

fn main() {
    // A prisoner's dilemma shifted so every payoff is strictly positive.
    let game = MatrixGame::bimatrix(
        &[vec![4.0, 1.0], vec![6.0, 2.0]],
        &[vec![4.0, 6.0], vec![1.0, 2.0]],
    )
    .unwrap();

    // Roth–Erev: initial propensity 1, phi = 0.1, epsilon = 0.2.
    let mut re = RothErevState::new(&game, 0, 1.0, 0.1, 0.2).unwrap();
    re.update(0, 3.0).unwrap(); // chose action 0, earned 3
    let q = re.propensities();
    let p = re.selection_probabilities().unwrap();
    println!("propensities [{:.1}, {:.1}]", q[0], q[1]);
    println!("probabilities [{:.4}, {:.4}]", p[0], p[1]);
    // propensities [3.3, 1.5]
    // probabilities [0.6875, 0.3125]

    // Logit: uniform before any observation, then exp(lambda * estimate).
    let mut logit = LogitState::new(&game, 0, 1.0, EstimateMode::LastOpponent).unwrap();
    println!("before {:?}", logit.selection_probabilities(&game, 0).unwrap());
    logit.observe(0).unwrap(); // the opponent played C
    let p = logit.selection_probabilities(&game, 0).unwrap();
    println!("after  [{:.4}, {:.4}]", p[0], p[1]);
    // before [0.5, 0.5]
    // after  [0.1192, 0.8808]

    let mut rng = StdRng::seed_from_u64(7);
    let action = logit.choose_action(&game, 0, &mut rng).unwrap();
    assert!(action < 2);
}
```

（`0.9 · 1 + 0.8 · 3 = 3.3`，`0.9 · 1 + 0.2 · 3 = 1.5`．相手が `C` を出した後の見積もりは `4` と `6` なので，`p(C) = 1 / (1 + e²)` です．）

### 繰り返しゲームのオートマトン

`Automaton::new(kind, coop)` は，すべての役割の行動数がちょうど 2 のゲームで，決まった繰り返しゲームの戦略を出します．
`coop` は協力の行動（`C`）の番号で，もう一方の番号が `D` です．`AllD` を除き，どのオートマトンも最初は `C` を出します．

| `AutomatonKind` | 次の行動 |
|---|---|
| `AllC` / `AllD` | 常に `C` / 常に `D` |
| `Grim` | 相手が一度でも `D` を出すまでは `C`，以後はずっと `D` |
| `Tft` | 相手の直前の行動 |
| `Wsls`（win-stay, lose-shift） | `(C, C) → C`，`(C, D) → D`，`(D, C) → D`，`(D, D) → C`．`(自分, 相手)` は直前のステップの行動 |
| `Gtft { generosity }` | 相手の `C` の後は `C`．相手の `D` の後は確率 `generosity` で `C`，それ以外は `D` |

### 乱数

学習則が引くのは，`game_decide` が `decide` に渡すエンジンのストリームだけです．`learn_encounter` と `learn_step` は RNG を受け取りません．
ストリームはスケジューラーの活性化の順に，1 人のエージェントの中では決定の単位の順に消費されます（[決定の粒度](#決定の粒度) を参照）．
引き方は固定です．確率ベクトルから引く規則（`Mixed`，`LogitChoice`，`RothErev`，GTFT）は一様な `f64` を 1 回引き，
行動の番号順に累積確率をたどります（丸め誤差で引いた値が合計を超えたときは，確率が正である最後の行動をとる）．
同点の乱択は同点の行動について `gen_range` を 1 回引き，最適な行動が 1 つだけなら何も消費しません．
`Fixed`，`TitForTat`，決定的なオートマトン，番号の小さい方を選ぶ同点処理，`generosity` が `0` か `1` の GTFT は乱数を消費しません．

### 決定の粒度

`PlayOptions::decision_scope(scope)` で，1 回の決定が何をカバーするかを選びます．
Matching があるエージェントを 1 ステップに複数の対戦の席に着けると，この違いが効いてきます．

| | `DecisionScope::PerStep`（既定） | `DecisionScope::PerAgentRole` | `DecisionScope::PerEncounter` |
|---|---|---|---|
| 1 ステップあたりの決定の回数 | 1 回．その行動をすべての席に書く | そのステップに持つ役割ごとに 1 回（役割の昇順） | 席ごとに 1 回（`EncounterId` の順） |
| `ctx.unit()` / `ctx.scope()` | `DecisionUnit::Step` / `PerStep` | `DecisionUnit::AgentRole(role)` / `PerAgentRole` | `DecisionUnit::Encounter(id)` / `PerEncounter` |
| `ctx.role()` | その役割（すべての席で同じ） | その役割 | その席の役割 |
| `ctx.encounter()` | 最後の対戦（`EncounterId` が最大のもの） | `None` | その対戦 |
| `ctx.opponents()` | その対戦のほかのエージェント | その役割で着いた対戦すべての相手（重複を除き昇順） | その対戦のほかのエージェント |
| `held` を書くか | 書く | 書かない | 書かない |
| `StepRecord::action` | その行動 | `None` | `None` |

`PerAgentRole` と `PerEncounter` では，エージェントが 1 ステップの中で違う行動を出しうるので，保持する行動や記録する行動が 1 つに決まりません．
そのため `held` は前の値のままで，ステップ履歴には `action: None` が記録されます（利得と `n_played` は通常どおり記録される）．
これらの粒度で過去の対戦の行動が要る規則は，`LearnContext::outcomes()` か [対の履歴](#対の履歴) から読みます．
`Held` のエージェントは，どの粒度でも保持行動をすべての席に出します．

`PerStep` では，各エージェントが 1 ステップに持つ役割が **1 つ**でなければなりません．
前もって判定できる Matching は，`MatchContext::decision_scope()` を見て `Matching::check` でこれを検査します．
`RoundRobin` は，`PerStep` のもとでエージェントが 3 人以上のとき，または `both_orders(true)` のとき，`assemble` から `GameError::InvalidMatching` を返します．
前もって判定できない Matching では，`PerStep` のステップでエージェントが 2 つの違う役割を持つと，
`game_decide` が `RuleError::AmbiguousRole { agent }` から作った Mechanism のエラー（`agent 0 holds more than one role in a per-step decision`）でステップを止めます．
`Held` のエージェントも対象です．

```rust
use std::sync::Arc;

use socsim_games::{
    assemble, build_world, AssignGroup, AssignKind, Assignment, Automaton, AutomatonKind,
    DecisionScope, InitialActions, MatrixGame, NormalFormOutcome, PlayOptions, RoundRobin,
    RuleBook,
};

fn main() {
    let game = MatrixGame::bimatrix(
        &[vec![3.0, 0.0], vec![5.0, 1.0]],
        &[vec![3.0, 5.0], vec![0.0, 1.0]],
    )
    .unwrap();
    let mut rules = RuleBook::new();
    rules.register("tft", Arc::new(Automaton::new(AutomatonKind::Tft, 0))).unwrap();
    let assign = Assignment::Ordered(vec![AssignGroup {
        kind: AssignKind::Rule("tft".to_owned()),
        count: 3,
    }]);

    for scope in [DecisionScope::PerStep, DecisionScope::PerAgentRole, DecisionScope::PerEncounter] {
        let world = build_world(game.clone(), 3, (), InitialActions::Same(0), &rules, &assign, 10, 10, 1)
            .unwrap();
        // With three agents, agent 1 sits in role 1 of (0, 1) and role 0 of (1, 2).
        let result = assemble(
            world,
            RoundRobin::new(),
            rules.clone(),
            NormalFormOutcome,
            PlayOptions::new().decision_scope(scope),
            1,
        );
        match result {
            Ok(_) => println!("{scope:?}: ok"),
            Err(error) => println!("{scope:?}: {error}"),
        }
    }
    // PerStep: invalid matching: RoundRobin assigns more than one role to an agent in a per-step decision
    // PerAgentRole: ok
    // PerEncounter: ok
}
```

LLM による決定は `PerStep` だけに対応します．`assemble_with_llm` はほかの粒度を `RuleError::Unsupported` で拒否します．

### `RoundRobin`

`RoundRobin::new()` は，エージェントの**すべての組**を毎ステップ 1 回ずつ対戦させます．
エージェントを `AgentId` で並べ，組を辞書式の順に積みます（4 人なら `(0, 1), (0, 2), (0, 3), (1, 2), (1, 3), (2, 3)`）．役割 0 には番号の小さい方が着きます．
`.both_orders(true)` は各組の直後に逆順の組も積むので（`(0, 1), (1, 0), (0, 2), (2, 0), …`），各エージェントはどの相手とも両方の役割で対戦します．
自分自身との対戦には対応していません．`.self_play(true)` は，役割がちょうど 2 でないゲームと同じく，`assemble` が `GameError::InvalidMatching` で拒否します．
この Matching は乱数を引きません．

状態は `MatchingState::RoundRobin { cursor }` で，World に（したがって snapshot に）保存されます．
`cursor` は行ったラウンドの数で，毎ステップ 1 ずつ増えます．

エージェントが `n` 人なら，各エージェントは 1 ステップに `n − 1` 回（両方の順なら `2(n − 1)` 回）対戦するので，`PayoffAggregation` が効いてきます．
`Sum` は対戦ごとの利得を足し，`Mean` は `n_played` で割ります．
上の囚人のジレンマで 3 人が `C`，`D`，`C` を出すと，`Sum` では `[3, 10, 3]`，`Mean` では `[1.5, 5, 1.5]` を受け取ります．

### 対戦ごとの学習

`DecisionRule` には `learn_step` のほかに 3 つのフックがあります．

| メソッド | 既定 | 意味 |
|---|---|---|
| `learn_encounter(state, outcome, ctx)` | 何もしない | 1 つの対戦から学習する |
| `needs_encounter_learning()` | `false` | `true` なら `game_decide` が `learn_encounter` を呼ぶ |
| `needs_pair_history()` | `false` | 割り当てたどれかの種類で `true` なら，`build_world` が [対の履歴](#対の履歴) を作る |

Reward では，`game_decide` が規則で動くエージェントを `AgentId` の昇順に訪れます．
規則が `needs_encounter_learning() == true` を返すエージェントについては，まずそのエージェントの対戦ごとに `EncounterId` の順で `learn_encounter` を 1 回ずつ呼び，
その後で `learn_step` を呼びます．`LearnContext::scope()` は決定の粒度を返します．

`FictitiousPlay`，`LogitChoice`，`RothErev`，`Automaton` はこれを使って，粒度に応じて状態を切り替えます．
`PerEncounter` では**相手ごとに状態を 1 つ**持ちます（`by_opponent()`）．状態はその相手との最初の決定か観測のときに作られ，新しい規則と同じように初期化されます．
`learn_encounter` はその対戦の相手の状態を更新し，`learn_step` は何もしません．
`PerStep` と `PerAgentRole` では**全体で 1 つの状態**を持ちます（`global()`）．`learn_encounter` は何もせず，`learn_step` がそのステップの対戦を `EncounterId` の順にすべて取り込みます．
全体の状態は役割 0 に合わせた形をしており（例えば仮想プレイの信念は役割 1 の行動ごとに 1 つのカウントを持つ），役割 1 を持つエージェントも同じ状態を使います．
相手ごとの状態は，その相手と最初に出会ったときの役割に合わせた形になります．
2 つの役割の行動数が同じなら違いはありません．違う場合は，もう一方の役割での決定が失敗することがあります．

### 対の履歴

対の履歴は，エージェントの順序なしの組ごとに，その 2 人の直近 `history_len` 回の対戦（ステップと双方の行動）を保持します．
メモリを使うので，`build_world` は割り当てたどれかの規則の種類が `needs_pair_history() == true` を返すときだけこれを作ります．
組込みの規則でこれを返すのは `MyopicBestResponse` だけで，独自の規則も求められます．作らなかった場合，World の JSON（と snapshot）に `pair_history` の項目は現れません．

- `DecisionContext::pair_history(other)` は，その決定の単位の**現在の相手**についてだけ `PairView` を返します．
  対の履歴が有効でないとき，または `other` が `ctx.opponents()` にないときは `None` です．
  現在の相手でもまだ一度も対戦していなければ，**空の**ビューが返ります．
- `PairView::iter()` は `(t, 自分の行動, 相手の行動)` を古い順に返します．向きは尋ねたエージェントに合わせます．
  `len()` と `is_empty()` は保持している対戦の数を数えます．`both_orders(true)` では 1 ステップに 2 回対戦し，両方が `EncounterId` の順に記録されます．
- `ctx.omniscient().pair_history(a, b)` は World の任意の 2 人の組を読みます（`DecisionContext` から得た場合．`LearnContext` の `OmniscientContext` では `None`）．
- `game_play` は利得を計算した後で，そのステップの 2 席の対戦を追記します．したがって決定から自分のステップが見えることはありません．

総当たりで `PerEncounter` のもと，対の履歴を読む規則：

```rust
use std::sync::Arc;

use socsim_core::SimRng;
use socsim_games::{
    assemble, build_world, AssignGroup, AssignKind, Assignment, Automaton, AutomatonKind,
    BuiltinPolicyState, DecisionContext, DecisionRule, DecisionScope, InitialActions, MatrixGame,
    NormalFormOutcome, PlayOptions, RoundRobin, RuleBook, RuleError, RuleInitContext, Slot,
    Stateless,
};

/// Tit-for-tat that reads the pair history instead of keeping its own state.
struct PairTitForTat;

impl DecisionRule<MatrixGame, ()> for PairTitForTat {
    fn name(&self) -> &str {
        "pair_tit_for_tat"
    }
    fn init_state(&self, _: &RuleInitContext, _: &MatrixGame) -> Result<BuiltinPolicyState, RuleError> {
        Ok(<BuiltinPolicyState as Slot<Stateless>>::from_slot(Stateless))
    }
    fn decide(
        &self,
        _state: &mut BuiltinPolicyState,
        ctx: &DecisionContext<'_, MatrixGame, ()>,
        _rng: &mut SimRng,
    ) -> Result<usize, RuleError> {
        let opponent = ctx.opponents()[0];
        let pair = ctx.pair_history(opponent).expect("pair history is enabled");
        // (t, my action, their action), oldest first; cooperate on first meeting.
        Ok(pair.iter().last().map_or(0, |(_, _, theirs)| *theirs))
    }
    fn needs_pair_history(&self) -> bool {
        true // asks build_world to create the pair history
    }
}

fn main() {
    let game = MatrixGame::bimatrix(
        &[vec![3.0, 0.0], vec![5.0, 1.0]],
        &[vec![3.0, 5.0], vec![0.0, 1.0]],
    )
    .unwrap();
    let mut rules = RuleBook::new();
    rules.register("pair_tft", Arc::new(PairTitForTat)).unwrap();
    rules.register("all_d", Arc::new(Automaton::new(AutomatonKind::AllD, 0))).unwrap();
    rules.register("all_c", Arc::new(Automaton::new(AutomatonKind::AllC, 0))).unwrap();
    let assign = Assignment::Ordered(
        ["pair_tft", "all_d", "all_c"]
            .iter()
            .map(|kind| AssignGroup { kind: AssignKind::Rule((*kind).to_owned()), count: 1 })
            .collect(),
    );
    let world = build_world(game, 3, (), InitialActions::Same(0), &rules, &assign, 10, 10, 1).unwrap();
    let mut simulation = assemble(
        world,
        RoundRobin::new(),
        rules,
        NormalFormOutcome,
        PlayOptions::new().decision_scope(DecisionScope::PerEncounter),
        1,
    )
    .unwrap()
    .build();

    let mut totals = [0.0; 3];
    for _ in 0..10 {
        simulation.step().unwrap();
        for (total, payoff) in totals.iter_mut().zip(simulation.world().payoffs()) {
            *total += payoff;
        }
    }
    println!("{totals:?}");
    // [39.0, 64.0, 30.0]
}
```

10 ラウンドで，対の履歴を読む tit-for-tat は `all_d` に一度だけ搾取され（`0 + 9 · 1`），`all_c` とはずっと協力します（`10 · 3`）．

### 例：総当たりの繰り返し囚人のジレンマ

5 つのオートマトンが 200 ラウンドの繰り返し囚人のジレンマで総当たりします（`crates/socsim-games/examples/ipd_tournament.rs` を短くしたもの）．
`PerEncounter` では各エージェントが相手ごとにオートマトンを 1 つずつ持つので，tit-for-tat は裏切り者だけを罰し，ほかの相手は罰しません．

```rust
use std::sync::Arc;

use socsim_games::{
    assemble, build_world, AssignGroup, AssignKind, Assignment, Automaton, AutomatonKind,
    DecisionScope, InitialActions, MatrixGame, NormalFormOutcome, PayoffAggregation,
    PlayOptions, RoundRobin, RuleBook,
};

const ROUNDS: usize = 200;

fn main() {
    // Prisoner's dilemma, action 0 = C, 1 = D.
    let game = MatrixGame::bimatrix(
        &[vec![3.0, 0.0], vec![5.0, 1.0]],
        &[vec![3.0, 5.0], vec![0.0, 1.0]],
    )
    .unwrap();

    let strategies = [
        ("all_c", AutomatonKind::AllC),
        ("all_d", AutomatonKind::AllD),
        ("tft", AutomatonKind::Tft),
        ("grim", AutomatonKind::Grim),
        ("wsls", AutomatonKind::Wsls),
    ];
    let mut rules = RuleBook::new();
    for (kind, automaton) in &strategies {
        rules.register(kind, Arc::new(Automaton::new(automaton.clone(), 0))).unwrap();
    }

    let seed = 1;
    let world = build_world(
        game,
        strategies.len(), // agent i runs strategies[i]
        (),
        InitialActions::Same(0),
        &rules,
        &Assignment::Ordered(
            strategies
                .iter()
                .map(|(kind, _)| AssignGroup { kind: AssignKind::Rule((*kind).to_owned()), count: 1 })
                .collect(),
        ),
        ROUNDS,
        ROUNDS as u64,
        seed,
    )
    .unwrap();

    let mut simulation = assemble(
        world,
        RoundRobin::new(), // every pair once per step: 10 encounters for 5 agents
        rules,
        NormalFormOutcome,
        PlayOptions::new()
            .decision_scope(DecisionScope::PerEncounter) // one decision per seat
            .payoff_aggregation(PayoffAggregation::Sum),
        seed,
    )
    .unwrap()
    .build();

    let mut totals = vec![0.0; strategies.len()];
    for _ in 0..ROUNDS {
        simulation.step().unwrap();
        for (total, payoff) in totals.iter_mut().zip(simulation.world().payoffs()) {
            *total += payoff;
        }
    }
    for ((kind, _), total) in strategies.iter().zip(&totals) {
        println!("{kind}: {total}");
    }
    // all_c: 1800
    // all_d: 2008
    // tft: 1999
    // grim: 1999
    // wsls: 1900
}
```

```sh
cargo run -p socsim-games --example ipd_tournament --features core,engine
```

`all_d` に対して tit-for-tat は最初のラウンドで負け，その後は双方が裏切ります．
tit-for-tat は `0 + 199 · 1 = 199`，`all_d` は `5 + 199 · 1 = 204` です（完全な例はこの組の得点も表示する）．
ほかの 3 つの協力的な相手からはそれぞれ `200 · 3 = 600` を得るので，合計は `1999` です．
`all_d` が上回るのは，すべての相手を少なくとも一度は搾取するからです．tit-for-tat と grim は最初のラウンドで，
`wsls` は 1 ラウンドおきに（win-stay, lose-shift は裏切り者に対して `C` と `D` を交互に出す），`all_c` は毎ラウンド搾取されます．

---

## LLM による決定

feature `llm` を有効にすると，エージェントの行動を言語モデルに決めさせられます．
LLM の規則は `DecisionRule` ではありません．別の Mechanism [`game_llm_decide`](mechanisms/game-llm-decide.ja.md)（型は `LlmDriver`）が動かします．
この Mechanism はそのステップの LLM の決定をすべて集めて 1 つのバッチとして送り，その後で行動を確定します．
即決の規則（`game_decide`）と LLM の規則（`game_llm_decide`）は同じ World に混ぜられます．各種類はどちらか一方の Driver だけが担当します．
LLM による決定は，既定の [決定の粒度](#決定の粒度) `PerStep` だけに対応します．`assemble_with_llm` は `PerAgentRole` と `PerEncounter` を `RuleError::Unsupported` で拒否します．

```toml
socsim-games  = { git = "https://github.com/akitenkrad/rs-social-simulation-tools", branch = "main", package = "socsim-games", features = ["llm", "engine"] }
socsim-core   = { git = "https://github.com/akitenkrad/rs-social-simulation-tools", branch = "main", package = "socsim-core" }
socsim-engine = { git = "https://github.com/akitenkrad/rs-social-simulation-tools", branch = "main", package = "socsim-engine" }
socsim-llm    = { git = "https://github.com/akitenkrad/rs-social-simulation-tools", branch = "main", package = "socsim-llm" }
```

以下の型はすべて `socsim_games::llm` にあります．
組込みの Ollama → OpenAI のバックエンドを使うときだけ，`socsim-games` に feature `live` を足します．テストとオフラインの再生にネットワークは要りません．

### 二段階の決定：`prepare` → バッチ → `resolve`

```rust
pub trait LlmDecisionRule<G: StageGame, X: ExperimentState>: Send + Sync {
    fn name(&self) -> &str;
    fn init_state(&self, ctx: &RuleInitContext, game: &G) -> Result<X::PolicyState, RuleError>;
    /// 第 1 段：要求を描画する．純粋で，乱数を使わず，状態は読むだけ．
    fn prepare(
        &self,
        state: &X::PolicyState,
        ctx: &DecisionContext<'_, G, X>,
    ) -> Result<DecisionRequest, RuleError>;
    /// 第 2 段：応答を行動・再試行・フォールバックのいずれかにする．
    fn resolve(
        &self,
        state: &mut X::PolicyState,
        req: &DecisionRequest,
        reply: &LlmReply,
        rctx: &mut ResolveContext<'_, G>,
    ) -> Resolution<G::Action>;
    /// Reward フェーズ．モデルは呼ばない．
    fn learn_step(
        &self,
        state: &mut X::PolicyState,
        ctx: &LearnContext<'_, G, X>,
    ) -> Result<(), RuleError> { Ok(()) }
}
```

- `prepare` は `DecisionRequest { system, prompt, attempt }` を返します（`attempt` は 0 から）．
  生成の条件（temperature，seed，`max_tokens`，`think` など）は規則が決めるのではなく，
  Driver が `LlmSetup` の `LlmConfig` と決定単位から付けます．
- `resolve` は `LlmReply { text, thinking, usage, cache_hit }` と `ResolveContext { game, role, previous, fallback_rng, t }` を受け取ります．
  `previous` は前のステップのエージェントの保持行動，`fallback_rng` は決定単位から導いたストリームです．
  戻り値は `Resolution::Accept { action, flags }`，`Resolution::Retry(次の要求)`，`Resolution::GiveUp { action, fallback, violation }` のいずれかです．
- モデルを呼ぶのは Driver の Decision フェーズの中**だけ**です．`learn_step`（Reward）は規則の状態を更新するだけです．

### 2 つの Driver にまたがる種類：`KindCatalog` と `build_world_with`

`build_world` が知っているのは `RuleBook` だけです．LLM の規則の種類があるときは，すべての種類を `KindCatalog` で表し，
`build_world_with` で World を作ります（引数は `build_world` と同じで，`&rules` の代わりに `&catalog` を渡す）．

```rust
let catalog = KindCatalog::new()
    .with_rules(&rules)?      // game_decide が担当する種類
    .with_llm(&llm_rules)?;   // game_llm_decide が担当する種類
```

- 両方の表に登録した種類は `RuleError::DuplicateKind` です．
- 種類 ID は全種類名の和集合の辞書順で振るので，どの Driver が担当するかにも登録順にもよりません．
  結果は `catalog.kinds()`，`catalog.kind_id(kind)`，`catalog.driver_name(kind)`（`"game_decide"` か `"game_llm_decide"`）で見られます．
- `build_world(.., &rules, ..)` は `build_world_with(.., &KindCatalog::from_rules(&rules), ..)` と同じです．
- `assemble` は World の種類の一覧が自分の `RuleBook` と一致することを，`assemble_with_llm` は `rules` ＋ `llm_rules` の
  カタログと一致することを検査します．違えば `GameError::InvalidKinds { expected, actual }` です．
  したがって LLM の種類を含む World は `assemble_with_llm` で組み立てます．
- 各 Driver は担当でない種類を飛ばします．`game_decide` は LLM のエージェントの席を空けたままにし，`game_llm_decide` がそれを埋めます．

### 例：LLM のエージェント対 Tit-for-Tat

バックエンドは `ScriptedClient` なので，このプログラムはオフラインで動きます．
実際のモデルを呼ぶには，構築関数を `live_backend_factory()`（feature `live`）に替えます．

```rust
use std::sync::Arc;

use socsim_games::llm::{
    assemble_with_llm, BackendFactory, BackendIdentity, LabelParser, LlmPolicy,
    LlmReplayManifest, LlmRuleBook, LlmSetup, MarkdownTableRenderer, OwnHistoryObs, ReplayMode,
    ResponseStore, WithLlm,
};
use socsim_games::{
    build_world_with, AssignGroup, AssignKind, Assignment, FixedPlayers, InitialActions,
    KindCatalog, MatrixGame, NormalFormOutcome, PlayOptions, RuleBook, TitForTat,
};
use socsim_llm::mock::ScriptedClient;

type X = WithLlm<()>; // 再生の manifest は実験状態に置く

fn main() {
    // モデルが答えるラベルを付けた囚人のジレンマ．
    let game = MatrixGame::bimatrix(
        &[vec![3.0, 0.0], vec![5.0, 1.0]],
        &[vec![3.0, 5.0], vec![0.0, 1.0]],
    )
    .unwrap()
    .with_labels(vec![
        vec!["C".to_owned(), "D".to_owned()],
        vec!["C".to_owned(), "D".to_owned()],
    ])
    .unwrap();

    // 即決の規則 (game_decide) と LLM の規則 (game_llm_decide)．
    let mut rules: RuleBook<MatrixGame, X> = RuleBook::new();
    rules.register("tit_for_tat", Arc::new(TitForTat::new(0))).unwrap();
    let mut llm_rules: LlmRuleBook<MatrixGame, X> = LlmRuleBook::new();
    llm_rules
        .register(
            "llm",
            Arc::new(LlmPolicy::new(
                OwnHistoryObs::new(5),
                MarkdownTableRenderer::default(),
                LabelParser::strict(),
            )),
        )
        .unwrap();

    // 両方の Driver の全種類を 1 つのカタログに．
    let catalog = KindCatalog::new()
        .with_rules(&rules)
        .unwrap()
        .with_llm(&llm_rules)
        .unwrap();

    // 応答ストアとバックエンド．構築関数はワーカーごとに 1 回呼ばれる．
    let store = ResponseStore::in_memory("ipd-llm", ReplayMode::RecordMisses);
    let identity = BackendIdentity {
        model: "scripted".to_owned(),
        endpoint: "mock://scripted".to_owned(),
    };
    let backend: BackendFactory = Arc::new(|| {
        Ok(Box::new(ScriptedClient::constant(
            "scripted",
            r#"{"action": "D", "reason": "defect"}"#,
        )))
    });

    let seed = 7;
    let rounds = 5;
    let x = WithLlm {
        inner: (),
        llm: LlmReplayManifest::for_store(&store, identity.clone()),
    };
    let world = build_world_with(
        game,
        2,
        x,
        InitialActions::Same(0),
        &catalog,
        &Assignment::Ordered(vec![
            AssignGroup { kind: AssignKind::Rule("llm".to_owned()), count: 1 },
            AssignGroup { kind: AssignKind::Rule("tit_for_tat".to_owned()), count: 1 },
        ]),
        rounds,
        rounds as u64,
        seed,
    )
    .unwrap();

    let setup = LlmSetup::new(llm_rules, store, backend, identity);
    let mut simulation = assemble_with_llm(
        world,
        FixedPlayers::pairs(&[(0, 1)]),
        rules,
        setup,
        NormalFormOutcome,
        PlayOptions::new(),
        seed,
    )
    .unwrap()
    .build();

    for _ in 0..rounds {
        simulation.step().unwrap();
        println!("held = {:?}", simulation.world().held());
    }
}
```

```text
held = [1, 0]
held = [1, 1]
held = [1, 1]
held = [1, 1]
held = [1, 1]
```

`assemble_with_llm(world, matching, rules, llm, outcome, opts, seed)` は `assemble` と同じ引数に `LlmSetup` を足したもので，
`X: HasLlmManifest` を要求し（[manifest](#manifest-と再開の保証) を参照），`game_match` → `game_decide` → `game_llm_decide` → `game_play` の順に登録します．
`OnInvalidAction::Fallback` は `game_decide` の種類だけに効きます．その表には `RuleBook` の種類だけを書きます（LLM の種類を書くと `RuleError::UnknownKind`）．
LLM の種類のフォールバックは `RetryProtocol` が受け持ちます．

### `LlmPolicy` の部品

`LlmPolicy::new(obs, renderer, parser)` は組込みの `LlmDecisionRule` です．
エージェントが何を観測するか，それをどうプロンプトにするか，応答をどう行動にするか，の 3 つの部品を型引数にした 1 つの型です．

```rust
LlmPolicy::new(OwnHistoryObs::new(5), MarkdownTableRenderer::default(), LabelParser::strict())
    .with_retry(RetryProtocol { max_retries: 2, fallback: Fallback::Previous })  // 既定: 1，Previous
    .with_max_turns(32)                                                          // 既定: 16
```

状態は `X::PolicyState` の中の `LlmConversation { max_turns, turns }` なので，この規則は `X::PolicyState: Slot<LlmConversation>` を要求します
（`BuiltinPolicyState` は feature `llm` のとき `Llm` の変種を持つ）．
決定のたびに `ConversationTurn { t, action, attempts, gave_up, payoff }` を追記し（`action` は `Debug` の表記，`payoff` は Reward で埋まる），
`max_turns` を超えたら古いものから捨てます．この記録は解析用で，プロンプトには再生しません．

**観測**（`ObservationProvider<G, X>`．`DecisionContext` だけから作るので，[情報構造](#情報構造) の既定のビューが保たれる）：

| 提供者 | `Obs` | 中身 |
|---|---|---|
| `OwnHistoryObs::new(window)` | `OwnHistory<G::Action>` | `t`，`role`，自分の直近 `window` 件の `StepRecord`（古い順），現在の各相手の直前の行動（無ければ `None`） |
| `CountsObs`（`G: FiniteGame`） | `ActionCounts` | `role`，自分の直前の行動，現在の相手のうち直前に各行動を選んだ人数．`t` を持たないので，同じ状況のエージェントは同じプロンプトになり，呼出しを 1 回で共有する |

**レンダラ**（`GameRenderer<G, Obs>`：`rules` → system プロンプト，`turn` → プロンプト，`feedback` → 再試行で末尾に足す文）：

| レンダラ | 組み合わせる観測 | system プロンプト |
|---|---|---|
| `MarkdownTableRenderer`（`G: FiniteGame`） | `OwnHistoryObs` | `action_label` から作る利得表（2 役割なら双行列，それ以外は役割ごとの表）と，応答の形式 `{"action": "<action label>", "reason": "<reason>"}` |
| `CountsRenderer`（`G: FiniteGame`） | `CountsObs` | 役割，選べるラベル，同じ応答の形式 |

**パーサ**（`ActionParser<G>`）：

- `LabelParser::strict()` — 応答は JSON のオブジェクト（そのままか，Markdown の単一のフェンス ```` ``` ```` / ```` ```json ```` で囲んだもの）で，
  `"action"` の文字列が役割のラベルのどれかと完全に一致しなければなりません．それ以外は違反です．
- `LabelParser::lenient()` — まず厳密な照合を試し，失敗したら応答のどこかで最初に現れたラベルを探し
  （`socsim_llm::extract_first_choice`：大文字小文字を区別せず，語の境界で，最も早いものが勝つ），`ParseFlags::lenient_match` を立てます．
  `.with_synonyms(label, &[..])` でそのラベルとみなす語を足せます．
  **緩い照合は，全役割の全ラベルが ASCII の `[A-Za-z0-9_-]+` であることを要求します**．
  これは規則の状態を初期化するときに検査するので，満たさなければ `build_world_with` が `RuleError::Unsupported` で失敗します．厳密な照合はどんなラベルも受け付けます．
- `.require_reason(true)` は文字列の `"reason"` も要求します．理由が無ければ，緩い照合でも違反です．
- `NumberParser::new(field).bounds(lo, hi).out_of_range(OutOfRange::Clamp)` は数値の応答のための単体の補助です（`parse_number(raw) -> ParseOutcome<f64>`）．
  JSON の数値か数字の文字列（`value_was_string`）で，有限の値だけを受け付けます．範囲外は `Violation`（既定）か切り詰め（`clamped`）です．
  `MatrixGame` 用の `ActionParser` ではありませんが，1 次元の行動のすべての `ContinuousGame` 用の `ActionParser` であり，`DescribedRenderer` と組にして使います（[連続ゲームでの LLM による決定](#連続ゲームでの-llm-による決定) を参照）．

受理した応答は `ParseFlags { fenced, value_was_string, clamped, lenient_match }` を，拒否した応答は
`Violation { kind, message }`（`ViolationKind::{Format, Label, Range, InvalidAction}`）を伴います．

**再試行とフォールバック** — `RetryProtocol { max_retries, fallback }`．`attempt < max_retries` の間は，違反のたびに
前のプロンプトにレンダラの `feedback` を足した再試行を作ります．そのステップの再試行はまとめて次のバッチとして送ります．
それを過ぎるとフォールバックで諦めます．

| `Fallback` | 行動 |
|---|---|
| `Previous`（既定） | 前のステップのエージェントの保持行動 |
| `UniformRandom` | パーサが `fallback_rng`（決定単位から導く．エンジンのストリームは使わない）で一様に引く |
| `Specified(action)` | `action` |

Driver は確定した行動をすべて `check_action` で検査し，不正なら `RuleError::InvalidAction` でステップを止めます．

### `game_llm_decide` の Decision の流れ

1. **準備** — 種類が LLM の規則である席に着いたエージェントについて，スケジューラーの活性化の順に `prepare` を呼ぶ．
2. **重複除去** — キャッシュのキー（後述）が同じ要求は 1 回だけ送る．
3. **照会** — 主スレッドで各要求を `ResponseStore` から引く（`Live` を除く）．`ReplayOnly` で見つからなければステップを止める．
4. **実行** — 見つからなかった要求だけを `LlmExecutor` に渡す．
5. **確定** — 決定の順に `resolve` を呼ぶ．`Retry` の要求を集め，もう 1 つのバッチとして 2〜5 を繰り返す．`GiveUp` はフォールバックで確定する．
6. **戻し** — 確定した行動を席と `held` に書く．

**シード．** `temperature == 0` のとき，すべての要求は定数のシード `derive_seed(decision_root, &[0])` を使います．
同じプロンプトはキー・呼出し・保存した記録を 1 つに共有します．
それ以外の temperature では，シードは `derive_seed(decision_root, &[t, agent, 0, 0, attempt])`（決定と試行ごとに 1 つ）なので，
エージェントが違えば要求も普通は違います．フォールバックのストリームは `derive_seed(decision_root, &[t, agent, 0, 0, u64::MAX])` です．
Driver はエンジンのステップの乱数を引かないので，活性化の順やワーカーの数で結果は変わりません．

**構成．**

```rust
let setup = LlmSetup::new(llm_rules, store, backend, identity)
    .executor(LlmExecutor::Threaded { workers: 4 })   // 既定: Sequential
    .options(options)                                 // 既定: ExecutorOptions::default()
    .config(LlmConfig::deterministic());              // 既定．seed は常に上書きされる
```

`config` は各要求の `LlmConfig` の基になります．Driver は `seed` を上書きし，要求が system を持つとき（組込みの規則は常に持つ）は `system` も上書きします．

### 応答ストア

ゲームのシミュレーションでは `socsim_llm::PromptCache` の代わりに `ResponseStore` を使います．各要求は版付きのキーで識別します．

```rust
pub struct CacheKeyV1 {
    pub version: u32,                 // 1
    pub backend: BackendIdentity,     // { model, endpoint }
    pub system: Option<String>,
    pub prompt: String,
    pub temperature_bits: u32,        // f32::to_bits．0.2 と 0.8 は別のキー (0.0 と -0.0 も別)
    pub seed: u64,
    pub omit_seed: bool,
    pub max_tokens: Option<u32>,
    pub think: Option<bool>,
    pub allow_blank: bool,
}
```

引くときのキーは `canonical_json()` の 64 bit FNV-1a です．キー全体を各 `ResponseRecord { key, text, thinking, usage, backend_metadata }` と一緒に保存し，
当たるたびに比べるので，ダイジェストの衝突は応答として返さず検出します（`StoreError::KeyCollision`）．
当たったときも思考とトークン数を復元するので，再生した呼出しは `cache_hit` 以外は実際の呼出しと同じ形で記録されます．

| 構築 | 置き場所 |
|---|---|
| `ResponseStore::in_memory(store_id, mode)` | メモリだけ（テスト用） |
| `ResponseStore::open(dir, mode, run_tag, seed)` | `dir/store_id`（最初に開いたときに作り，以後はそれを使う），`dir/base.jsonl`（読取り専用．無くてもよい），この実行のシャード `dir/shards/<run_tag>-<seed>.jsonl`（読み，追記する） |

| `ReplayMode` | ストアの照会 | 見つからないとき | 書込み |
|---|---|---|---|
| `ReplayOnly` | する | ステップが失敗する | なし |
| `RecordMisses` | する | バックエンドを呼ぶ | 新しい記録をこの実行のシャードへ |
| `Live` | しない | （すべての要求がバックエンドへ行く） | なし |

**シャードは頼んだときだけ統合されます．** `open` が読むのは基底と自分のシャードだけで，他の実行のシャードは読みません．
記録した実行が終わった後（たとえば複数シードの掃引の後）に `ResponseStore::merge_shards(dir)` を呼ぶと，
すべてのシャードを `base.jsonl` に取り込み（一意な一時ファイルに書いて rename する），シャードを消して，
`MergeReport { records_added, duplicate_records, shards_merged }` を返します．
同じキーで中身の違う記録があれば `StoreError::Conflict` です．統合すると基底のダイジェストが変わるので，統合の前に書いた manifest はストアと一致しなくなります．

### 実行器とバックエンド

```rust
pub type BackendFactory = Arc<dyn Fn() -> Result<Box<dyn LlmClient>, LlmError> + Send + Sync>;
```

実行器はバックエンドを構築関数から**それを使うスレッドの中で**作ります（`LlmClient` は `Send` ではないため）．
`Sequential` ではバッチごとに 1 回，`Threaded` ではワーカーごとに 1 回です．ストアに当たらなかった要求が 1 件もないバッチでは作りません．
バックエンドはキャッシュを持ってはいけません（キャッシュは応答ストアです）．
`live_backend_factory()`（feature `live`）は `socsim_llm::build_live_backend` を包みます．テストでは `socsim_llm::mock::ScriptedClient` を返します．

| `LlmExecutor` | 呼出しを実行する場所 |
|---|---|
| `Sequential`（既定） | 主スレッドで 1 つずつ |
| `Threaded { workers }` | 有界のキューで配った `workers` 本のスレッド．ストアへ書くのは主スレッドだけ |

| `ExecutorOptions` のフィールド | 既定値 | 意味 |
|---|---|---|
| `timeout` | 120 秒 | 呼出しごと．**`Threaded` だけ**に効く（`Sequential` の呼出しは中断できない）．時間切れの呼出しは再送せず，ステップが失敗する |
| `max_backend_retries` | 3 | バックエンドのエラーの後の再送の回数．使い切るとステップが `RuleError::Backend` で失敗する |
| `backoff_base` | 1 秒 | k 回目の再送の前に `backoff_base × 2^(k-1)` 待つ |
| `queue_per_worker` | 2 | キューの容量 = `workers × queue_per_worker` |

`ExecutorOptions` は `#[non_exhaustive]` です．`ExecutorOptions::default()` から始めてフィールドに代入します．
`workers == 0` や `queue_per_worker == 0` はバックエンドのエラーです．
シードが決定単位で決まり，確定が決定の順に行われるので，`Sequential` と `Threaded` は同じ World を作ります．

### manifest と再開の保証

実際のモデルは同じ要求に違う応答を返しうるので，snapshot から正確に再開できるのは，再開後の実行が同じ応答を見る場合だけです．
そこで World は，どの応答に対して実行したかを記録します．`WithLlm<X> { inner, llm }` は実験状態を包み（`PolicyState` と `view_for` は `inner` に委ねる），
`LlmReplayManifest::for_store(&store, identity)` で作る `LlmReplayManifest { store_id, base_digest, mode, backend }` を持ちます．
`assemble_with_llm` は `X: HasLlmManifest` を要求し，`WithLlm<X>` がそれを実装します．

- Driver は毎回の Decision で，World の manifest を自分のストア（`store_id`，`base_digest`，`mode`）と `BackendIdentity` に照らします．
  違いがあればステップを止めます．走っている途中のシミュレーションに restore した場合も，次の Decision で照合されます．
- `verify_snapshot(&snapshot, &store, &identity)` は同じ照合を `restore` の前に行い，`GameError::ManifestMismatch` を返します．
- 再開した実行が通しの実行と一致するのは，`ReplayOnly` の場合，または `RecordMisses` で再開後の全要求がストアに当たった場合
  （`game.llm_call` の `cache_hit`，`llm.cache_hit_rate == 1`）**だけ**です．`Live` では何も保証しません．
  モードは manifest の一部なので，snapshot を取ったときと同じモードで再開します．

```rust
// 記録し，シャードを基底に取り込む．
let store = ResponseStore::open(dir, ReplayMode::RecordMisses, "record", seed)?;
// ... このストアで組み立てて走らせる ...
ResponseStore::merge_shards(dir)?;

// オフラインで再生し，snapshot を取り，同じストアに対して再開する．
let store = ResponseStore::open(dir, ReplayMode::ReplayOnly, "replay", seed)?;
// ... 組み立て，走らせ，let snapshot = simulation.snapshot(); ...
let store = ResponseStore::open(dir, ReplayMode::ReplayOnly, "resume", seed)?;
verify_snapshot(&snapshot, &store, &identity)?;
// ... このストアで組み立て，resumed.restore(snapshot) ...
```

### 記録されるもの

LLM の決定が 1 つ以上あった Decision の最後に，Driver は表 `"game"` に 1 行を書きます（`record_row`）．

| 列 | 分母 |
|---|---|
| `llm.parse_fail_rate` | 試行の数（論理的な呼出し．再試行を含む） |
| `llm.clamp_rate`，`llm.value_was_string_rate`，`llm.lenient_rate` | 試行の数 |
| `llm.cache_hit_rate` | 試行の数 |
| `llm.retry_rate` | 決定の数（1 回以上再試行したエージェント） |
| `llm.giveup_rate` | 決定の数（フォールバックしたエージェント） |

試行はすべて（重複除去で呼出しを共有したものも）イベント `game.llm_call` として記録します．payload は次のとおりです．

| キー | 値 |
|---|---|
| `agent`，`kind` | エージェントの添字と規則の種類 |
| `unit`，`unit_index` | 決定単位（`"step"`）とその添字（`0`） |
| `attempt`，`resolution` | 0 起点の試行番号．`"accept"`，`"retry"`，`"giveup"` |
| `system`，`prompt` | 要求 |
| `reply`，`thinking`，`usage` | 応答，思考，トークン数（ストアに当たったときも） |
| `seed` | 生成のシード |
| `cache_hit` | ストアから返したなら `true` |
| `deduplicated` | 同じバッチの他のエージェントの呼出しを使い回したなら `true` |
| `key` | 要求のキーのダイジェスト（小文字 16 桁の 16 進） |
| `flags` | `ParseFlags`（`retry` / `giveup` ではすべて `false`） |

LLM の決定が無いステップでは何も記録しません．

---

## 集団動学

進化ゲーム理論のエージェントは，ゲームについて推論しません．
各自が 1 つの戦略を持ってそれを打ち，多く稼いだ戦略が広がります．広がり方は，より稼いだエージェントの模倣か，適応度に比例した繁殖です．
`socsim-games` はこれを，**Held** のエージェント（決定規則を持たないエージェント．`AssignKind::Held`）と 1 つの**集団動学**で表します．
集団動学は，Reward でそのステップの利得から保持行動を書き換える Mechanism です．
`assemble_with` の引数 `dynamics` に渡します．
エージェントが 1 人ずつ行動を変える更新にするときは，代わりに `assemble_async` で組み立てます（[非同期更新](#非同期更新) を参照）．

よく混ざったタカ・ハトの集団の例です．1 000 人が毎ステップ無作為に組になり，比例模倣で更新します．
タカ 30 % から始めると，タカの割合は進化的に安定な割合 $V/C = 0.5$ へ向かいます．

```rust
use socsim_games::catalog::HawkDove;
use socsim_games::{
    assemble_with, build_world, AssignGroup, AssignKind, Assignment, GameMetrics, InitialActions,
    MetricsOptions, NormalFormOutcome, Pairwise, PlayOptions, ProportionalImitation, RandomPairs,
    RuleBook, Update,
};
use socsim_log::InMemoryRecorder;

fn main() {
    let n = 1_000;
    let seed = 7;
    // Action 0 = hawk, 1 = dove. With V = 2, C = 4 one encounter pays in [-1, 2].
    let game = HawkDove::new(2.0, 4.0).unwrap();
    let metrics = GameMetrics::new(&game, MetricsOptions::new().population_share(true)).unwrap();

    let rules = RuleBook::new(); // no rules: every agent is Held
    let mut initial = vec![1; n];
    initial[..300].fill(0); // 300 hawks, 700 doves
    let held = Assignment::Ordered(vec![AssignGroup { kind: AssignKind::Held, count: n }]);
    let world = build_world(game, n, (), InitialActions::PerAgent(initial), &rules, &held, 1, 200, seed)
        .unwrap();

    let mut sim = assemble_with(
        world,
        RandomPairs::new(),                // n is even: every agent plays exactly once
        rules,
        Pairwise::new(NormalFormOutcome),  // the play path of `assemble`
        ProportionalImitation::new(1.0 / 3.0, Update::Synchronous).unwrap(), // beta = 1 / (2 - (-1))
        PlayOptions::new(),
        seed,
    )
    .unwrap()
    .add_mechanism(Box::new(metrics))
    .recorder(Box::new(InMemoryRecorder::new()))
    .build();
    sim.run().unwrap();

    let recorder = sim.recorder().as_any().unwrap().downcast_ref::<InMemoryRecorder>().unwrap();
    for row in recorder.metrics().iter().filter(|row| {
        [1, 50, 200].contains(&row.t)
            && (row.key == "game.played_share.a0" || row.key == "game.population_share.a0")
    }) {
        println!("t={} {} = {}", row.t, row.key, row.value);
    }
}
```

```text
t=1 game.played_share.a0 = 0.3
t=1 game.population_share.a0 = 0.319
t=50 game.played_share.a0 = 0.497
t=50 game.population_share.a0 = 0.498
t=200 game.played_share.a0 = 0.54
t=200 game.population_share.a0 = 0.511
```

`t = 1` では初期の 300 人のタカが対戦し（`played_share.a0 = 0.3`），最初の更新の後には 319 人がタカを保持しています（`population_share.a0`）．
ここでは全員がちょうど 1 回ずつ対戦するので，ステップ `t` の `played_share` はステップ `t − 1` の `population_share` に等しくなります．
有限の集団なので，割合は $V/C$ の周りで揺れ続けます．

### 4 つの動学

| Mechanism | 設定（`assemble_with` の引数） | エージェント $i$ の規則 | トポロジー | 乱数 |
|---|---|---|---|---|
| [`imitate_best`](mechanisms/imitate-best.ja.md) | `ImitateBest::new(update)`，`.tie_break(ImitationTie::{KeepOwn, LowestAction})` | $i$ と近傍の中で最も稼いだエージェントの行動を採る | 格子かネットワークが必須 | なし |
| [`fermi_update`](mechanisms/fermi-update.ja.md) | `FermiUpdate::new(k, update)?` | 相手 $j$ を 1 人引き，確率 $1/(1 + \exp[-(\pi_j - \pi_i)/K])$ で $j$ の行動を採る | 任意 | 相手＋一様乱数 1 つ |
| [`replicator_update`](mechanisms/replicator-update.ja.md) | `ProportionalImitation::new(beta, update)?` | 相手 $j$ を 1 人引き，確率 $\beta\,[\pi_j - \pi_i]_+$ で $j$ の行動を採る | 任意 | 相手＋一様乱数 1 つ |
| [`moran_update`](mechanisms/moran-update.ja.md) | `MoranUpdate::new(fitness, mutation, update)?` | 1 ステップに出生と死亡を 1 回：適応度に比例して選んだ親が，一様に選んだ 1 人を置き換える | 持てない | 1 ステップに 4 回 |

$\pi_i$ はエージェント $i$ のそのステップの利得で，`PayoffAggregation` を適用した後の値（`world.payoffs()` の値）です．動学は利得を計算し直しません．

- **`imitate_best`**（Nowak & May, 1992）．同点は近傍の位置ではなく行動の添字で破るので，規則は格子の対称性と可換です．
  `ImitationTie::KeepOwn`（既定）は，自分の利得が最高に並ぶなら自分の行動を保ち，そうでなければ最高の近傍のうち最小の行動の添字を採ります．
  `LowestAction` は，最高の利得に並ぶ自分と近傍のうち最小の行動の添字を採ります．
- **`fermi_update`**（Szabó & Tőke, 1998）．相手はエージェントの近傍から，トポロジーが無ければほかの全エージェントから一様に引きます．自分自身を引くことはありません．
  $K > 0$ は雑音です．$K \to 0$ ではより稼いだ相手を決定的に真似る規則に近づき，$K$ が大きいと無作為な写しに近づきます．
  確率は $z = (\pi_j - \pi_i)/K$ として，$z \ge 0$ なら $1/(1 + e^{-z})$，$z < 0$ なら $e^{z}/(1 + e^{z})$ で計算するので，溢れることはありません．
- **`replicator_update`** は Schlag（1998）の比例模倣の規則です．より稼いだ相手は利得の差に比例した確率で写し，稼ぎの少ない相手は決して写しません．
  大きなよく混ざった集団では，その期待変化はレプリケータ動学の離散版になります．Mechanism の名前はここから来ています．相手は `fermi_update` と同じように引きます．
- **`moran_update`**（Moran, 1958；Nowak, Sasaki, Taylor & Fudenberg, 2004；Traulsen, Shoresh & Nowak, 2008）．
  親は全エージェントから適応度に比例した確率で引き，死ぬエージェントは全エージェント（親を含む）から一様に引きます．
  死ぬエージェントの保持行動は親の行動になります．ただし確率 `mutation` で，一様に引いた行動になります．
  適応度は `Fitness::Linear { w }`（$1 - w + w\,\pi$，$w \in [0, 1]$）か `Fitness::Exponential { w }`（$e^{w\pi}$，$w$ は有限）です．
  居住者の適応度 $1$ に対して 1 人の変異体の適応度が一定の $r$ なら，変異体が $N$ 人の集団を占める確率は $\rho = (1 - 1/r)/(1 - 1/r^N)$，中立なら $1/N$ です．

近傍を持たないエージェントは行動を保ち，乱数を引きません．

### 同期更新

どのコンストラクタも更新の方式を必須の引数として受け取ります．方式は `Update::Synchronous` か `Update::Asynchronous` で，既定値はありません．
方式を後から足しても，既存の設定の意味が変わらないようにするためです．
`assemble_with` は `Synchronous` だけを受け付け，`Asynchronous` で作った動学を拒否します．

```text
invalid population dynamics: fermi_update with Update::Asynchronous must be assembled with assemble_async
```

`Synchronous` では，全エージェントの次の行動を**同じ更新前の状態**から計算して一時バッファに書き，ステップの終わりにまとめて確定します．
ステップの中で，ほかのエージェントの新しい行動が見えることはありません．
同じ規則を 1 人ずつその場で書き換える形で適用すると，結果がエージェントの順に依存し，対称な格子の配置の対称性も崩れます．
それは別のモデルで，次の [非同期更新](#非同期更新) で扱います．
`moran_update` は 1 ステップに保持行動をちょうど 1 つ書きます．親と死ぬエージェントはどちらも更新前の状態から引きます．

### 非同期更新

Huberman & Glance（1993）は，Nowak & May の空間囚人のジレンマで協力者の塊が残るのは，全セルが同じ瞬間に更新するからだと示しました．
セルが 1 つずつ無作為な順に更新すると，1 人の裏切り者が格子全体を占めることがあります．
`assemble_async` はこのモデルを組み立てます．`RuleBook` は受け取らず（全エージェントが Held），動学も別の引数にはしません．
`Update::Asynchronous` で作った 3 つの写しの動学のどれかを `AsyncPopulationStep` で包んで渡します．

```rust
use socsim_games::catalog::PrisonersDilemma;
use socsim_games::{
    assemble_async, build_world, AssignGroup, AssignKind, Assignment, AsyncActivation,
    AsyncPopulationStep, FermiUpdate, GameMetrics, GridBoundary, GridNeighborhood, GridNeighbors,
    GridSpec, InitialActions, MetricsOptions, NormalFormOutcome, Pairwise, PayoffTiming,
    PlayOptions, RuleBook, Topology, Update,
};
use socsim_log::InMemoryRecorder;

fn main() {
    let side = 50;
    let n = side * side;
    let seed = 5;
    let game = PrisonersDilemma::weak(1.3).unwrap();
    let metrics = GameMetrics::new(
        &game,
        MetricsOptions::new()
            .population_share(true)
            .payoff_timing(PayoffTiming::AfterUpdate), // describe() says "after every update"
    )
    .unwrap();

    let rules = RuleBook::new(); // no rules: every agent is Held
    let held = Assignment::Ordered(vec![AssignGroup { kind: AssignKind::Held, count: n }]);
    let initial = InitialActions::Shares(vec![(0, 0.5), (1, 0.5)]); // half C, half D
    let world = build_world(game, n, (), initial, &rules, &held, 1, 100, seed)
        .unwrap()
        .with_topology(
            Topology::grid(GridSpec {
                rows: side,
                cols: side,
                boundary: GridBoundary::Toroidal,
                neighborhood: GridNeighbors::Moore,
            })
            .unwrap(),
        )
        .unwrap();

    let mut sim = assemble_async(
        world,
        GridNeighborhood::new().include_self(true), // an implicit pattern
        Pairwise::new(NormalFormOutcome),
        AsyncPopulationStep::new(FermiUpdate::new(0.1, Update::Asynchronous).unwrap())
            .activation(AsyncActivation::UniformWithReplacement), // N draws with replacement
        PlayOptions::new(),
        seed,
    )
    .unwrap()
    .add_mechanism(Box::new(metrics))
    .recorder(Box::new(InMemoryRecorder::new()))
    .build();
    sim.run().unwrap();

    let recorder = sim.recorder().as_any().unwrap().downcast_ref::<InMemoryRecorder>().unwrap();
    for row in recorder.metrics().iter().filter(|row| {
        [1, 10, 100].contains(&row.t)
            && (row.key == "game.played_share.a0" || row.key == "game.population_share.a0")
    }) {
        println!("t={} {} = {}", row.t, row.key, row.value);
    }
}
```

```text
t=1 game.played_share.a0 = 0.4008
t=1 game.population_share.a0 = 0.4008
t=10 game.played_share.a0 = 0.2756
t=10 game.population_share.a0 = 0.2756
t=100 game.played_share.a0 = 0.3848
t=100 game.population_share.a0 = 0.3848
```

ここでは 2 つの列がどのステップでも等しくなります．上の同期のタカ・ハトの例とは違います（[`PostStep` で見えるもの](#poststep-で見えるもの) を参照）．

**1 回に 1 人ずつ．** `assemble_async` は，`game_match`，`game_decide`，`game_play` と動学の代わりに，[`async_population_step`](mechanisms/async-population-step.ja.md) という Mechanism を 1 つだけ登録します．
PreStep では `game_match` と同じようにそのステップの対戦の状態を消します（対戦の表を空にし，利得と対戦数を 0 にします）．
Reward ではエージェントを 1 人ずつ活性化し，活性化したエージェント $i$ ごとに次を行います．

1. 規則が読む利得を，その時点の `held` から計算し直します．$i$ とそのトポロジーの近傍の利得で，`WholePopulation` では全エージェントの利得です．書く先は World ではなく Mechanism の作業領域です．
2. $i$ に `imitate_best`，`fermi_update`，`replicator_update` の規則を当て，$i$ の新しい行動をただちに `held` に書きます．次に活性化するエージェントは，もうその行動を相手に対戦します．
3. `log_updates(true)` のときは，$i$ の行動が変わったらイベント `game.async_update` を記録します．

最後の活性化の後，最終的な `held` からステップ全体をもう一度評価し，利得，対戦数，ステップ履歴，集計値の行を書きます．
書くコードは `game_play` の [対戦を実体化しない経路](#対戦を実体化しない評価) と同じで，`world.plays()` は空のままです．
局所の利得はその経路と同じ順に足すので，規則が見る利得は全体を計算し直した利得とビット単位で同じです．

**活性化の順：`AsyncActivation`．**

| 変種 | 1 ステップの活性化の回数 | 活性化するエージェント | 乱数 |
|---|---|---|---|
| `AgentOrder`（既定） | $N$ | 全エージェントを 1 回ずつ，スケジューラーの順（`StepContext::agent_order`）に．順次のスケジューラーでは `AgentId` の昇順，`RandomActivationScheduler` では毎ステップ新しい無作為な置換 | スケジューラーの置換 |
| `UniformWithReplacement` | $N$ | 活性化のたびに $N$ 人から一様に 1 人を引く．同じエージェントが 2 回引かれることも，1 回も引かれないこともある | 活性化の前に `gen_range(0..N)` を 1 回 |

Huberman & Glance の更新は復元抽出です．各ミクロステップで無作為に選んだ 1 人が更新し，$N$ ミクロステップの 1 世代でエージェント 1 人あたり平均 1 回の更新になります．
したがって再現には `UniformWithReplacement` を使います．
`UniformWithReplacement` はスケジューラーの順を使いません．`RandomActivationScheduler` は使われない置換を引くだけなので，順次のスケジューラー（既定）のままにしてください．
順次のスケジューラーでの `AgentOrder` は添字の順の決定的な掃引で，それだけで対称な格子の対称性を崩します．

**記録の意味．** `Synchronous` では，そのステップの利得，`played_share`，`coop_rate`，ステップ履歴は，更新**前**に打った行動を表します．
`async_population_step` では，それらは**そのステップの全更新の後**に保持している行動で対戦し直した結果を表します．
値を作るコードは同じでも意味が違い，`game_metrics` はどの組み立てを観測しているかを知りません．
`MetricsOptions::payoff_timing(PayoffTiming::AfterUpdate)`（連続ゲームでは `ContinuousMetricsOptions::payoff_timing`）で伝えてください．
これは `describe()` が `game.played_share.{action}`，`game.mean_payoff`，`game.payoff_sd`，`game.coop_rate`（連続では `game.mean_action.*`，`game.action_sd.*`，`game.mean_payoff`，`game.payoff_sd`）について返す意味だけを変え，記録する値は変えません．
既定は `PayoffTiming::BeforeUpdate` です．`assemble_async` はこれを変えませんが，[`GameSimFactory::async_dynamics`](#型状態) は `AfterUpdate` を自動で付けます．
`game.population_share.a<k>` の意味はどちらでも同じで，ステップの終わりに保持している行動です．

**受け付ける構成．** `assemble_async` は次の順に検査し，最初に違反したところで失敗します．

| 条件 | エラー |
|---|---|
| Matching がゲームの役割の数に対応している | `GameError::UnsupportedRoles` |
| 全エージェントが Held | `async_population_step applies only to held agents, but an active rule is assigned` |
| Matching が対戦を実体化しないパターンを持つ（`GridNeighborhood`，`NetworkEdges`，`WholePopulation`） | `async_population_step requires a matching with an implicit pattern (grid neighborhood, network edges or whole population); matchings that change partners every step are not supported` |
| `DecisionScope::PerStep` | `async_population_step requires DecisionScope::PerStep` |
| `materialize(false)` | `async_population_step does not materialize encounters; PlayOptions::materialize(true) is not supported` |
| `report_mixed(false)` | `async_population_step does not support report_mixed` |
| 対戦の評価経路が `Pairwise::new(NormalFormOutcome)` か `counting_play()`（集計値の有無は問わない） | `async_population_step requires the NormalFormOutcome or counting play path` |
| `OnInvalidAction::Stop` | `async_population_step does not use on_invalid_action; fallback is not supported` |
| Matching 自身の検査（例えば `GridNeighborhood` は格子を要する） | Matching のエラーのまま |
| 動学を `Update::Asynchronous` で作った | `assemble_async requires Update::Asynchronous, got Synchronous` |
| 動学の [組み立て時の検査](#組み立て時の検査)：共通の行動の集合，`imitate_best` のトポロジー，$\beta$ の上限 | `assemble_with` と同じ文言 |

Matching のもの以外はどれも `GameError::Population` で，表示には接頭辞 `invalid population dynamics: ` が付きます．
`MoranUpdate` は `AsyncPopulationStep` に合いません（渡すとコンパイルできません）．
モランの 1 ステップは全員の適応度から引く 1 回の出生と死亡で，«活性化したエージェントの新しい行動» という単位を持たないからです．
毎ステップ相手が変わる Matching（`RandomPairs`，`RandomGroups`）も対象外です．ステップの途中で相手が変わると，規則が読む利得を定義し直す必要があるからです．
`ImitateBest` と `FermiUpdate` は連続ゲームも受け付けます．
非同期更新はライブラリ専用です．Scenario ファイルと `normal-form` パックは `update = "synchronous"` だけを受け付けます．

`assemble_async` は `derive_seed(seed, &[1])` を seed にした `SimulationBuilder` を返します．スケジューラー，Recorder，`game_metrics` は `assemble_with` と同じく呼び出し側が足します．
`log_updates(true)` では，変わった行動ごとにイベント `game.async_update` を記録します．payload は
`{"agent": <AgentId>, "order": <そのステップでの活性化の通し番号．0 始まり>, "from": <行動>, "to": <行動>}` です（有限の行動は添字，連続の行動は数の配列）．

**例：`huberman_glance`．** `crates/socsim-games/examples/huberman_glance.rs` は Huberman & Glance の Fig. 1 を走らせます．
固定境界の 99 × 99 の格子，Moore 近傍と自己対戦，`PrisonersDilemma::weak(1.85)`，中央に裏切り者が 1 人，`KeepOwn` の `ImitateBest` という構成です．
世代ごとの協力者の割合（`game.population_share.a0`）を CSV で出します．`--update async`（`UniformWithReplacement`．既定）と `--update sync`（`assemble_with`．Nowak–May の対照）を選べます．

```sh
cargo run -q -p socsim-games --features core,engine,grid --example huberman_glance -- --update async --steps 217 --seed 1
```

```text
generation,cooperator_share
1,0.9969390878481788
2,0.987756351392715
3,0.9732680338740944
...
118,0.0008162432404856647
119,0.00010203040506070809
120,0
...
217,0
```

```sh
cargo run -q -p socsim-games --features core,engine,grid --example huberman_glance -- --update sync --steps 217 --seed 1
```

```text
generation,cooperator_share
1,0.9990817263544536
2,0.9974492398734823
3,0.995816753392511
...
216,0.37751249872461995
217,0.39995918783797574
```

非同期では，第 120 世代から全員が裏切ります（seed 1）．seed 0〜4 では，全員が裏切りになるのはそれぞれ第 119，120，79，129，134 世代です．
同期では裏切り者が対称な模様で広がり，第 217 世代に 3 920 人の協力者が残ります．
**この結果は $b > 1.8$ に限ります．** $b \le 1.8$ では，非同期に更新しても協力が残ります．
$b = 1.85$ は，Nowak & May の空間カオスの区間 $1.8 < b < 2$ にあります．

### `PostStep` で見えるもの

動学は Reward で，`game_play` が利得を書いた後，`game_decide` の学習の後に動きます．したがって PostStep では次のようになります．

| 読むもの | 表すもの |
|---|---|
| `world.payoffs()`，`n_played()`，`plays()`，ステップ履歴 | このステップで打った行動（更新**前**） |
| `world.held()` | 更新**後**の行動．次のステップでエージェントが打つ行動 |

`game_metrics` は両方を報告します．`game.played_share.<id>` はこのステップの席（打った行動）を数え，
`MetricsOptions::population_share(true)` で有効にする `game.population_share.a<k>` は，更新後に行動 `k` を保持する**全エージェント**の割合です．
2 つは中身（更新の前か後か）も分母（席かエージェントか）も違います．9 つの対戦の席に着いたエージェントは `played_share` では 9 回，`population_share` では 1 回数え，
どの席にも着かなかったエージェントは `population_share` でだけ数えます．[列](#列) を参照してください．

`async_population_step` の後では時点は 1 つだけです．`payoffs()`，`n_played()`，ステップ履歴，`held()` はどれもそのステップの全更新の後に保持している行動を表し，`plays()` は空です．
このとき `played_share` と `population_share` は同じ行動から数え，違うのは分母（席かエージェントか）だけです．
全エージェントの席の数が同じ構成（`WholePopulation`，トーラス，正則なネットワーク）では 2 つの列は等しくなります（[非同期更新](#非同期更新) の例）．
固定境界の格子では縁のエージェントの席が少ないので，2 つはわずかに違います．
その差を更新の効果と読まないでください．

### 組み立て時の検査

動学がきちんと定まらない設定は，`assemble_with` が `GameError::Population` で拒否します
（有限でない値や範囲外の値は，コンストラクタが同じエラーで先に拒否します）．

- **Held のエージェントだけ．** 全エージェントが `AssignKind::Held` でなければなりません．決定規則を持つエージェントは Decision で `held` を上書きするので，動学は規則と混ざることを拒みます．
- **共通の行動の集合．** ゲームが `CommonActionSpace` を実装していなければならず（実装していなければコンパイルできません），すべての役割の行動数が等しくなければなりません．
- **トポロジー．** `imitate_best` は格子かネットワークのトポロジーを要し，`moran_update` はトポロジーの無い World を要します．`fermi_update` と `replicator_update` はどちらでも動きます．
- **連続ゲーム．** `imitate_best`，`fermi_update`，`moran_update` は `ContinuousGame`（すべての役割の区間が同一のもの）も受け付けます．`replicator_update` は有限ゲームだけです．`moran_update` は連続ゲームでは `Fitness::Linear` を拒否し，`mutation > 0` ならガウスの突然変異の幅を要求します（[連続の行動での集団動学](#連続の行動での集団動学) を参照）．
- **`replicator_update` の上限．** 採用の確率 $\beta\,[\pi_j - \pi_i]_+$ が 1 を超えてはならないので，$\beta \le 1/\Delta\pi$ でなければなりません．$\Delta\pi$ は，1 ステップに取りうる利得の値域の幅です．
- **`moran_update` の線形の適応度の正値性．** `Fitness::Linear { w }` では，1 ステップに取りうる最小の利得 $\pi_{\min}$ で $1 - w + w\,\pi_{\min}$ が正でなければなりません．`Fitness::Exponential` は常に正です．

1 ステップに取りうる値域は，1 回の対戦の利得の値域 $[p_{\min}, p_{\max}]$ と，Matching の `encounter_bounds`（エージェントが利得を受け取る対戦の数の `min` と `max`）から求めます．

- 1 回の対戦の値域は，`Pairwise::new(NormalFormOutcome)` ではゲームのすべての行動の組（10⁶ 以下）から，`counting_play()` ではすべての人数ベクトルから求めます．
  利用者定義の `OutcomeRule` では値域が求まらないので，`replicator_update` と，`Linear` の適応度の `moran_update` は拒否されます．
- `PayoffAggregation::Sum` では，$c \in \{\texttt{min}, \texttt{max}\}$ と $p \in \{p_{\min}, p_{\max}\}$ の積 $c \cdot p$ を覆う範囲です．
  対戦の数を報告しない Matching（`FixedPlayers`，`RoundRobin`，既定のままの独自の Matching）は拒否されます．`PayoffAggregation::Mean` を使ってください．
- `Mean` では $[p_{\min}, p_{\max}]$ です．
- `min` が `0` のとき（対戦しないエージェントがありうるとき．例えば奇数人の `RandomPairs`）は，値域に `0` を加えます．

$V = 2$，$C = 4$ のタカ・ハトでは，1 回の対戦の利得は $[-1, 2]$ に入ります．
`RandomPairs` では各エージェントが利得を受け取る対戦は高々 1 回なので，$\Delta\pi = 3$，$\beta \le 1/3$ です．
Moore 近傍で自己対戦つきの `GridNeighborhood` では，各エージェントが 9 回の対戦で利得を受け取るので，`Sum` では $[-9, 18]$ で $\beta \le 1/27$，`Mean` では $\beta \le 1/3$ のままです．
上の例で $\beta = 0.5$ を求めると，次のように失敗します．

```text
invalid population dynamics: replicator_update requires beta <= 1 / 3 (the per-step payoff range), got 0.5
```

実行時には，有限でない利得や，`moran_update` の適応度の合計が正でないことが，ステップを Mechanism のエラーで止めます．

### 集団用の Matching

| Matching | 役割 | 1 ステップの対戦 | Credit | `encounter_bounds` | 実体化しない経路 | 乱数 |
|---|---|---|---|---|---|---|
| `RandomPairs::new()` | 2 | エージェントを並べ替え，隣り合う 2 人を組にする．奇数人なら 1 人が休む | `All` | `1..1`，奇数の $n$ では `0..1` | なし | 並べ替え 1 回 |
| `RandomGroups::new(k)?` | `k`（`k ≥ 2`） | エージェントを並べ替え，隣り合う `k` 人を群れにする．最後の $n \bmod k$ 人が休む | `All` | `1..1`，`k` が $n$ を割り切らなければ `0..1` | なし | 並べ替え 1 回 |
| `WholePopulation::new()` | エージェントごとに 1 つ | 全エージェントで 1 つの対戦．席は `AgentId` の順 | `All` | `1..1` | あり | なし |
| `NetworkEdges::new()`（feature `net`） | 2 | 辺 `(source, target)` ごとに 1 つの対戦．辺は整列した順 | `All` | エージェントに接する辺の数の最小と最大 | あり | なし |
| `GridNeighborhood::new().include_self(b)`（feature `grid`） | 2 | セルを番号の順に，近傍ごとに対戦 `[center, neighbour]` を 1 つ．`include_self(true)` ならその後に `[center, center]` | `Center` | エージェントの近傍の数（自己対戦があれば ＋1） | あり | なし |

`k < 2` の `RandomGroups::new(k)` と，ゲームの役割の数がエージェントの人数と違う `WholePopulation` は `GameError::InvalidMatching` です．
`NetworkEdges` と `GridNeighborhood` は対応するトポロジーを要し（無ければ `InvalidMatching`），1 ステップに 1 人を複数の役割の席に着けるので，
`allows_multi_role_per_step()` を宣言します．規則で動くエージェントは 1 ステップに 1 回だけ決定し，その行動をすべての席で使います．
有向ネットワークでは，`NetworkEdges` は弧ごとに 1 回対戦させ，両端に利得を与えます．動学の近傍は出近傍です．

Moore 近傍で `include_self(true)` の `GridNeighborhood` は，Nowak & May（1992）の相互作用です．
各エージェントは 8 つの近傍と自分自身を相手に段階ゲームを行い，自分が中心の対戦ごとに 1 回利得を受け取ります．

### トポロジー：`GameWorld::with_topology`

`build_world` で作った World はトポロジーを持ちません（`Topology::None`）．組み立ての前に付けます．

```rust
let world = build_world(game, side * side, (), initial, &rules, &held, 1, steps, seed)?
    .with_topology(Topology::grid(GridSpec {
        rows: side,
        cols: side,
        boundary: GridBoundary::Toroidal,          // or Fixed
        neighborhood: GridNeighbors::Moore,        // or VonNeumann
    })?)?;
```

| コンストラクタ | feature | エージェント $i$ の近傍 |
|---|---|---|
| `Topology::grid(GridSpec { rows, cols, boundary, neighborhood })` | `grid` | セル $i$ に隣接するセル．セル $i$ は `i / cols` 行 `i % cols` 列 |
| `Topology::network(SocialNetwork)` | `net` | 無向ネットワークでの近傍 |
| `Topology::directed_network(DiSocialNetwork)` | `net` | 出近傍 |

格子のセルがエージェントと同数でないとき，またはネットワークの頂点が World の `AgentId` とちょうど一致しないか自己ループを持つとき，`with_topology` は `GameError::Topology` を返します．
近傍の並びは整列し重複を除くので，辺を足した順は結果に影響しません．
トポロジーは World の一部で，snapshot にも入ります．格子は `GridSpec` として保存し，読み込むときに作り直します．
Mechanism は `MatchContext::topology()` と `OutcomeContext::topology()` で読み，`GameWorld` は `socsim_core::Neighbors` を実装します．
ネットワークは `NetworkEdges` と，`moran_update` 以外の動学に合います．

```rust
let ids: Vec<AgentId> = (0..n as u64).map(AgentId).collect();
let network = SocialNetwork::watts_strogatz(
    &ids, 4, 0.1, &mut SimRng::from_seed(derive_seed(seed, &[4])), // labels [0]..[3] are taken
);
let world = build_world(PrisonersDilemma::weak(1.5)?, n, (), initial, &rules, &held, 1, 50, seed)?
    .with_topology(Topology::network(network))?;
let mut sim = assemble_with(
    world,
    NetworkEdges::new(),                                   // one encounter per edge
    rules,
    Pairwise::new(NormalFormOutcome),
    FermiUpdate::new(0.1, Update::Synchronous)?,           // a random neighbour per agent
    PlayOptions::new().payoff_aggregation(PayoffAggregation::Mean),
    seed,
)?
.build();
```

### シミュレーションの中の匿名ゲーム：`counting_play()`

`AnonymousGame` を実装したゲームでは，評価経路として `Pairwise::new(NormalFormOutcome)` の代わりに `counting_play()` を使えます．
各対戦で行動を 1 回数え，各行動の利得をほかの席の人数から 1 回ずつ計算します．席ごとに戦略形の利得を評価する代わりです．
利得は `Pairwise::new(NormalFormOutcome)` と同じで，1 つの対戦に多くのエージェントが着くとき（`RandomGroups`，`WholePopulation`）に効きます．
利用者定義の利得規則とは組み合わせられず，匿名でないゲームで `counting_play()` を使うとコンパイルできません．
`counting_play().with_aggregates(n)` は，`assemble_with_aggregates` と同じようにゲームの公開集計値も記録します．

100 人で 1 つの対戦をするエル・ファロル・バーを，フェルミ則で動かします．

```rust
use socsim_games::catalog::ElFarol;
use socsim_games::{
    assemble_with, build_world, counting_play, AssignGroup, AssignKind, Assignment, FermiUpdate,
    GameMetrics, InitialActions, MetricsOptions, PlayOptions, RuleBook, Update, WholePopulation,
};
use socsim_log::InMemoryRecorder;

fn main() {
    let n = 100;
    let seed = 3;
    // One encounter seats all 100 agents; attending pays +1 up to 60 attendees, -1 above.
    let game = ElFarol::new(n, 60).unwrap();
    let metrics = GameMetrics::new(&game, MetricsOptions::new().population_share(true)).unwrap();
    let rules = RuleBook::new();
    let held = Assignment::Ordered(vec![AssignGroup { kind: AssignKind::Held, count: n }]);
    let initial = (0..n).map(|i| usize::from(i >= 80)).collect(); // 80 attend, 20 stay home
    let world = build_world(game, n, (), InitialActions::PerAgent(initial), &rules, &held, 1, 100, seed)
        .unwrap();

    let mut sim = assemble_with(
        world,
        WholePopulation::new(),
        rules,
        counting_play().with_aggregates(8),                     // payoffs from counts + `attendance`
        FermiUpdate::new(0.1, Update::Synchronous).unwrap(),
        PlayOptions::new(),
        seed,
    )
    .unwrap()
    .add_mechanism(Box::new(metrics))
    .recorder(Box::new(InMemoryRecorder::new()))
    .build();
    sim.run().unwrap();

    let recorder = sim.recorder().as_any().unwrap().downcast_ref::<InMemoryRecorder>().unwrap();
    for row in recorder.metrics().iter().filter(|row| {
        [1, 2, 3, 100].contains(&row.t)
            && (row.key == "game.aggregate.attendance" || row.key == "game.population_share.a0")
    }) {
        println!("t={} {} = {}", row.t, row.key, row.value);
    }
    println!("arena encounters at the end: {}", sim.world().plays().n_encounters());
}
```

```text
t=1 game.aggregate.attendance = 80
t=1 game.population_share.a0 = 0.6
t=2 game.aggregate.attendance = 60
t=2 game.population_share.a0 = 0.86
t=3 game.aggregate.attendance = 86
t=3 game.population_share.a0 = 0.72
t=100 game.aggregate.attendance = 49
t=100 game.population_share.a0 = 0.76
arena encounters at the end: 0
```

ステップ `t` の `population_share.a0` は，ステップ `t + 1` の出席者数です．割合は更新の後に記録するからです．
`t = 1` ではバーが混み（80 > 60），出席者はみな $-1$ を得て，欠席者を相手に引いた出席者が切り替えます．
`t = 2` ではちょうど 60 人が出席し，出席者はみな $+1$ を得て，欠席者が切り替えて戻ります．
全員が一斉に更新するので，出席者数は定員を行き過ぎ続けます．バー問題の揺れの同期版です．
最後の行は，この実行が対戦の表を一度も作らなかったことを示します（次の節を参照）．

### 対戦を実体化しない評価

既定（`PlayOptions::materialize(false)`）では，次の条件が**すべて**成り立つとき，`game_play` は `GameWorld::plays` を埋めずに Matching の型を直接たどってステップを評価します．

- 全エージェントが Held である（規則を割り当てたエージェントがいない）．
- 決定の粒度が `DecisionScope::PerStep` である．
- 評価経路が `Pairwise::new(NormalFormOutcome)` か `counting_play()`（集計値の有無は問わない）で，利用者定義の `OutcomeRule` ではない．
- `Matching::implicit()` が型を返す．`WholePopulation`，`NetworkEdges`，`GridNeighborhood` は返し，`RandomPairs`，`RandomGroups`，`FixedPlayers`，`RoundRobin` は返さない．

そうでないとき，または `PlayOptions::materialize(true)` のときは，先に対戦を対戦の表に書きます．
どちらの経路でも，利得，対戦数，ステップの記録，集計値の行，指標の列は同じです．
実体化しない経路では，`game_match` は対戦を書かず，`world.plays()` は空のままで，`game_metrics` は `game_play` がそのステップについて保持する役割ごとの打った行動の数を読みます．
自己対戦つきの Moore 近傍では 1 人あたりおよそ 9 つの対戦があるので，大きな格子では対戦の表が World のほかの部分よりはるかに多くのメモリを使うことがあります．実体化しない経路はそれを確保しません．
`world.plays()` を自分で読みたいときは `materialize(true)` にしてください．
`implicit()` で型を返す独自の Matching は，実体化しない経路では自分の `encounters_into` ではなくその型で評価されるので，まったく同じ対戦を表すものでなければなりません．

### 例：`nowak_may` と `hawk_dove_large`

`crates/socsim-games/examples/` の 2 つのプログラムは，集団動学を大きな規模で走らせます．

**`nowak_may`** — 100 × 100 のトーラス上の Nowak & May（1992）の空間囚人のジレンマです．
自己対戦つきの Moore 近傍（`GridNeighborhood`），$b = 1.85$ の弱い囚人のジレンマ（`PrisonersDilemma::weak(1.85)`），`KeepOwn` の `ImitateBest`，
seed から無作為に配置した初期の裏切り者 10 % という構成です．
各世代の後の協力者の割合（`game.population_share.a0`）を CSV で出力します．

```sh
cargo run --release -p socsim-games --features core,engine,grid --example nowak_may -- --steps 200 --seed 1
```

```text
generation,cooperator_share
1,0.3805
2,0.2485
3,0.1984
...
200,0.3242
```

最初は裏切り者が侵入し（第 4 世代までに割合は約 0.19 に下がる），その後協力者の塊が持ち直して割合が揺れます．
この実行の第 21〜200 世代では 0.27 と 0.36 の間にあり，平均は 0.320 です．
Nowak & May は，$1.8 < b < 2$ では割合が 0.318 の周りで揺れると報告しています．この区間の中では，同期の発展は $b$ の値に依りません．
同じ seed の 2 回の実行は同じ出力を出します．

この例は初期の格子を自分で並べます．1,000 人の裏切り者を先に書き，`derive_seed(seed, &[0, 2])` で並べ替えます．
同じ配置は `InitialActions::Shares(vec![(1, 0.1), (0, 0.9)])`（裏切り者を先に書く）で得られます．協力者を先に書いた `Shares` と Scenario の `initial_shares` は，同じ seed で別の格子になります（[初期行動](#初期行動initialactions-と-uniform_initial_actions) を参照）．

**`hawk_dove_large`** — よく混ざったタカ・ハトの集団（$V = 2$，$C = 4$）を，`RandomPairs` と $\beta = 1/3$ の `ProportionalImitation` で動かします．初期はタカが半数です．
ステップの間に保持行動だけを持つ（`history_len = 1`，指標の Mechanism なし）ので，メモリは集団の大きさに比例して増えます．要約を 1 行出力します．

```sh
cargo run --release -p socsim-games --features core,engine --example hawk_dove_large
# n=10000 steps=10 seed=1 hawk_share_initial=0.5 hawk_share_final=0.4932 mean_payoff_last_step=0.5156
cargo run --release -p socsim-games --features core,engine --example hawk_dove_large -- --n 1000000 --steps 10
# n=1000000 steps=10 seed=1 hawk_share_initial=0.5 hawk_share_final=0.499876 mean_payoff_last_step=0.500256
```

集団は安定な割合 $V/C = 0.5$ から始まり，標本の揺れを除いてそこに留まります．揺れは $n$ が大きいほど小さくなります．この割合での平均利得は $0.5$ です．

---

## 指標

`GameMetrics`（`FiniteGame` 用）は読むだけの Mechanism（名前は `game_metrics`）で，ゲームのステップごとの統計 — 行動の割合，利得の平均とばらつき，
協力率，後悔，均衡までの距離，公開集計値 — を Recorder の表 `"game"` に 1 行ずつ記録します．
`assemble` は登録しないので，ゲームから作って，返されたビルダーに足します．
**PostStep** でゲームの全 Mechanism の後に動くので，終わったステップを見ます．
乱数を引かず，モデルの状態を変えません．World に書くのは，収束による停止と累積の情報源のための自分の記録だけです
（[Mechanism のページ](mechanisms/game-metrics.ja.md) を参照）．
連続ゲームでは同じ Mechanism の連続版 `ContinuousMetrics` を使い，割合の代わりに行動の平均とばらつきを記録します（[指標：`ContinuousMetrics`](#指標continuousmetrics) を参照）．

### `game_metrics` を足す

`GameMetrics::new(&game, MetricsOptions)` はゲームを参照で受け取り（ゲームが `build_world` に移る前に作ります），
`Result<GameMetrics<G>, GameError>` を返します（`G: FiniteGame`）．オプションはビルダーです．

| `MetricsOptions` のメソッド | 既定 | 効果 |
|---|---|---|
| `new()` / `default()` | — | 以下の既定値 |
| `cooperate_label(label)` | なし | `game.coop_rate` を足す．`label` は**全役割**でちょうど **1 つ**の行動のラベルに一致しなければならず，そうでなければ `GameError::Metrics` |
| `regret(on)` | 2 役割のゲームでは有効，それ以外では無効 | `game.regret` を足す／除く |
| `mixed_source(MixedSource)` | `Played` | `game.regret` と `game.eq_distance` が評価する混合戦略の組をどこから作るか |
| `equilibrium(EquilibriumTarget)` | `None` | `Solve` で `game.eq_distance` を足す |
| `stop(tol, window)` | なし | 行動の割合が動かなくなったら実行の終了を求める．`tol` は有限で `> 0`，`window ≥ 1`．そうでなければ `GameError::Metrics` |
| `action_entropy(on)` | 有効 | feature `metrics` のときだけ．`game.action_entropy` を足す／除く |
| `population_share(on)` | 無効 | `game.population_share.a<k>` を足す．すべての役割の行動数が等しくなければ `GameError::Metrics` |
| `payoff_timing(PayoffTiming)` | `BeforeUpdate` | `describe()` が返す意味だけを変える．`AfterUpdate` は，打った行動の割合，利得，協力率がそのステップの全更新の後に保持している行動から来ることを述べる（[非同期更新](#非同期更新) を参照） |

2 エージェントの囚人のジレンマで，Tit-for-Tat と常に裏切る相手を対戦させます．

```rust
use std::sync::Arc;

use socsim_games::{
    assemble, build_world, AssignGroup, AssignKind, Assignment, EquilibriumTarget, Fixed,
    FixedPlayers, GameMetrics, InitialActions, MatrixGame, MetricsOptions, NormalFormOutcome,
    PlayOptions, RuleBook, TitForTat,
};
use socsim_log::InMemoryRecorder;

fn main() {
    let game = MatrixGame::bimatrix(
        &[vec![3.0, 0.0], vec![5.0, 1.0]],
        &[vec![3.0, 5.0], vec![0.0, 1.0]],
    )
    .unwrap()
    .with_labels(vec![vec!["C".into(), "D".into()]; 2])
    .unwrap();

    // Built from the game before the game moves into the world.
    let metrics = GameMetrics::new(
        &game,
        MetricsOptions::new()
            .cooperate_label("C")                    // adds game.coop_rate
            .equilibrium(EquilibriumTarget::Solve)   // adds game.eq_distance
            .stop(1e-9, 3),                          // stop after 3 unchanged steps
    )
    .unwrap();

    let mut rules = RuleBook::new();
    rules.register("tit_for_tat", Arc::new(TitForTat::new(0))).unwrap();
    rules.register("fixed_defect", Arc::new(Fixed::new(1))).unwrap();

    let seed = 1;
    let world = build_world(
        game, 2, (), InitialActions::Same(0), &rules,
        &Assignment::Ordered(vec![
            AssignGroup { kind: AssignKind::Rule("tit_for_tat".into()), count: 1 },
            AssignGroup { kind: AssignKind::Rule("fixed_defect".into()), count: 1 },
        ]),
        100, 100, seed,
    )
    .unwrap();

    let mut sim = assemble(world, FixedPlayers::pairs(&[(0, 1)]), rules,
                           NormalFormOutcome, PlayOptions::new(), seed)
        .unwrap()
        .add_mechanism(Box::new(metrics))           // runs in PostStep
        .recorder(Box::new(InMemoryRecorder::new()))
        .build();
    sim.run().unwrap();                             // ends early: the stop rule fired

    let recorder = sim.recorder().as_any().unwrap()
        .downcast_ref::<InMemoryRecorder>().unwrap();
    for row in recorder.metrics().iter().filter(|row| row.t <= 2) {
        println!("t={} {} = {}", row.t, row.key, row.value);
    }
    println!("stopped at t = {}", recorder.metrics().last().unwrap().t);
}
```

（`InMemoryRecorder` は `socsim-log` にあります．どの `Recorder` でも構いません．）出力：

```text
t=1 game.played_share.a0 = 0.5
t=1 game.played_share.a1 = 0.5
t=1 game.coop_rate = 0.5
t=1 game.mean_payoff = 2.5
t=1 game.payoff_sd = 2.5
t=1 game.regret = 1
t=1 game.eq_distance = 2
t=2 game.played_share.a0 = 0
t=2 game.played_share.a1 = 1
t=2 game.coop_rate = 0
t=2 game.mean_payoff = 1
t=2 game.payoff_sd = 0
t=2 game.regret = 0
t=2 game.eq_distance = 0
stopped at t = 5
```

`t = 1` では Tit-for-Tat が協力し，相手が裏切ります．席の半分が `C` を打ち，利得は 0 と 5，協力した側は裏切れば 1 多く得られ（後悔 1），
組 `(C, D)` は唯一の均衡 `(D, D)` から L1 距離 2 にあります．`t = 2` からは両者が裏切ります．
feature `metrics` を有効にすると，同じプログラムは `game.coop_rate` の後に `game.action_entropy` も出力します
（`t = 1` で `0.6931471805599453` = ln 2，`t = 2` で `0`）．

### 列

どの列も `record_row(t, "game", …)` で書くので，行を展開する Recorder（既定の振る舞い）にはキー `game.<列>` として届きます．

| 列 | 現れる条件 | 値 |
|---|---|---|
| `game.played_share.<id>` | そのステップに席が 1 つ以上ある | 共通 ID：その行動を選んだ席 ÷ 全席．役割別 ID：その役割の席のうちその行動を選んだ席 ÷ その役割の席 |
| `game.coop_rate` | `cooperate_label` を設定し，ステップに席がある | 席の役割の協力行動を選んだ席 ÷ 全席 |
| `game.action_entropy` | feature `metrics`，`action_entropy(true)`（既定），共通 ID，かつステップに席がある | 共通の `played_share` の値の Shannon エントロピー $-\sum_k p_k \ln p_k$（単位は nat） |
| `game.mean_payoff` | 1 人以上が対戦した（`n_played() > 0`） | 対戦したエージェントの `payoffs()`（`PayoffAggregation` の後）の平均 |
| `game.payoff_sd` | `mean_payoff` と同じ | 同じ利得の母標準偏差（$n$ で割る） |
| `game.aggregate.<name>` | 集計値つきで組み立て（`assemble_with_aggregates`，または `.with_aggregates(n)` を付けた評価経路），かつ集約が定義される | そのステップの集約済みの集計値（[公開集計値](#公開集計値aggregategame) を参照） |
| `game.population_share.a<k>` | `population_share(true)` で，World にエージェントが 1 人以上いる | PostStep で `held` が `k` のエージェント ÷ 全エージェント．[集団動学](#集団動学) の後の値，つまり次のステップの行動 |
| `game.regret` | 後悔が有効で，混合戦略の組が定義される | $\max_r \big[\max_a F_r(a, \sigma_{-r}) - F_r(\sigma)\big]$（`mixed::regret`） |
| `game.eq_distance` | `EquilibriumTarget::Solve` で，組が定義される | 解いた均衡に対する $\min_{\sigma^*} \sum_r \sum_a \lvert \sigma_r(a) - \sigma^*_r(a)\rvert$ |

1 行の中の列の順は，行動の割合，`coop_rate`，`action_entropy`，`mean_payoff`，`payoff_sd`，集計値，集団の割合，`regret`，`eq_distance` です．

**定義されない列は行から省き，NaN として書くことはありません．** 対戦の無いステップでは割合，協力率，エントロピーの列を記録せず，
誰も対戦しなかったステップでは利得の列を記録しません．有限でない値（計算した値，または読んだ利得）が出たときは，行を記録する前にステップを Mechanism のエラーで止めるので，
`game_metrics` が記録や World の自分の状態に NaN や無限大を書くことはありません．
ただし行をそのまま保持する Recorder は行を埋めることがあります．`socsim_log::CsvRecorder` は表に一度でも現れた列をすべて残し，
行に無い列には NaN を書きます．列が初めて現れる前に記録した行も NaN で埋めます．
`llm` では `game_llm_decide` も同じ表 `"game"` に自分の行を書くので，その表には 1 ステップに 2 行が入り，それぞれ相手の列が NaN になります．Mechanism は毎ステップ，World のゲームが作ったときと同じ役割数と行動数を持つことも検査します．

`played_share` が数えるのは**席**であって，エージェントではありません．3 つの対戦の席に着いたエージェントは 3 回数えます．
そのため，`held` を書かない粒度も含めて，どの[決定の粒度](#決定の粒度)でも定義されます．
[実体化しない経路](#対戦を実体化しない評価) では対戦の表を使わずに席を数え，結果は同じです．
集団の割合 `game.population_share.a<k>` は別の量で，ステップの終わりに行動 `k` を保持している**エージェント**の割合です．
集団動学があるとき，これは更新の後の行動を表し，`played_share` は更新の前に打った行動を表します（[`PostStep` で見えるもの](#poststep-で見えるもの) を参照）．
集団の割合の ID は，ラベルに依らず常に `a<k>` です．

### 行動 ID：`metric_action_ids`

`game.played_share.<id>` の `<id>` は行動の**番号**から作り，ラベルからは作りません．そのためラベルに何が入っていても，列名は妥当な指標名になります．
`metric_action_ids(&game)`（既定のビルド）が ID を列の順に返します．

- **共通**（`MetricScheme::Pooled`）— 全役割の行動数が等しく，各番号のラベルが役割 0 と一致するとき．番号ごとに 1 つの ID
  `a0`，`a1`，… を全役割で共有します．
- **役割別**（`MetricScheme::PerRole`）— それ以外．役割ごとに `r<役割>-a<番号>` です．

```rust
use socsim_games::{metric_action_ids, GameMetrics, MatrixGame, MetricsOptions};

fn main() {
    let pd = MatrixGame::bimatrix(
        &[vec![3.0, 0.0], vec![5.0, 1.0]],
        &[vec![3.0, 5.0], vec![0.0, 1.0]],
    )
    .unwrap()
    .with_labels(vec![vec!["C".into(), "D".into()]; 2])
    .unwrap();
    // Two roles with different action sets.
    let asymmetric = MatrixGame::new(vec![2, 3], vec![vec![0.0; 6], vec![0.0; 6]])
        .unwrap()
        .with_labels(vec![
            vec!["Up".into(), "Down".into()],
            vec!["L".into(), "M".into(), "R".into()],
        ])
        .unwrap();

    for game in [&pd, &asymmetric] {
        let ids = metric_action_ids(game);
        let listed: Vec<_> = ids.actions().iter().map(|a| a.id.as_str()).collect();
        println!("{:?}: {}", ids.scheme(), listed.join(", "));
    }
    println!("{:?}", metric_action_ids(&asymmetric).id(1, 2));
    println!();

    let metrics = GameMetrics::new(&pd, MetricsOptions::new().cooperate_label("C")).unwrap();
    for (pattern, meaning) in metrics.describe() {
        if pattern.contains('{') {
            println!("[[metric_patterns]]\npattern = {pattern:?}\nmeaning = {meaning:?}\n");
        } else {
            println!("[metrics.{pattern:?}]\nmeaning = {meaning:?}\n");
        }
    }
}
```

出力の最初の 3 行：

```text
Pooled: a0, a1
PerRole: r0-a0, r0-a1, r1-a0, r1-a1, r1-a2
Some("r1-a2")
```

各 `MetricAction` は `id`，`role`（共通なら `None`），`action` を持ちます．表示にはゲームの `action_label(role, action)` を使ってください．
出力の残りは [列の説明](#列の説明describe) で使います．

### 後悔，均衡までの距離と `MixedSource`

`game.regret` と `game.eq_distance` は，役割ごとに 1 つの混合戦略からなる組 $\sigma$ を評価します．
`MetricsOptions::mixed_source` がその出どころを選びます．

| `MixedSource` | $\sigma_r$ | 定義される条件 |
|---|---|---|
| `Played`（既定） | このステップの役割 $r$ の席のうち各行動を選んだ割合 | このステップに全役割の席がある |
| `PlayedCumulative` | 同じ割合を，`game_metrics` が最初に見たステップ以降の**全**席で数えたもの | 全役割が 1 回以上打たれている（したがって，後の対戦の無いステップでも定義される） |
| `Reported` | このステップの役割 $r$ の席について，席に着いたエージェントが報告した混合（[混合戦略の報告](#混合戦略の報告) を参照）を平均したもの．`Held` のエージェントの席は打った行動の純粋戦略として数える | このステップに全役割の席がある |

`PlayedCumulative` の数は World に保持されるので，snapshot が持ち運び，再開した実行は同じ列を続けます．
`Reported` には `PlayOptions::report_mixed(true)` が要ります．無ければ最初のステップが Mechanism のエラーで失敗します．
席に着いた規則で動くエージェントに報告が無い（規則の `mixed` が `None` を返した）ときや，報告の長さが違うときも同じです．

`EquilibriumTarget::Solve` は [`support_enumeration`](#support_enumeration) を **`GameMetrics::new` の中で 1 回だけ**走らせ，均衡を保持します．
2 役割のゲームが必要で，ソルバーが失敗したとき，均衡の集合が完全でない（`is_complete()` が偽．退化したゲーム）とき，空のときに，
`new` は `GameError::Metrics` を返します．`game.eq_distance` はそれらの均衡のうち最も近いものまでの L1 距離の和で，0 から $2 \times$ 役割数の間の値です．

### 混合戦略の報告

`PlayOptions::report_mixed(true)` にすると，`game_decide` は**各 Reward フェーズの終わり**，全エージェントの学習が済んだ後に，
規則で動く全エージェント（`AgentId` の昇順）の全役割について `DecisionRule::mixed(state, role, game)` を呼び，結果を World に保存します．
`GameWorld::reported_mixed(agent, role) -> Option<&[f64]>` で読めます．

したがって報告される混合は**学習後**の混合 — 次のステップの決定が使うもの — であって，このステップの行動を生んだ混合ではありません．
`Held` のエージェント，別の Driver が持つ種類（LLM の種類），規則が `None` を返したとき，返した値が有限でないときは，報告は `None` です．
報告の表は毎回の報告の初めに空にするので，前のステップの値が残ることはありません．

| 組込みの規則 | `mixed` が報告するもの |
|---|---|
| `Fixed` | `None` |
| `UniformRandom` | 役割の $n$ 個の行動それぞれに $1/n$ |
| `Mixed` | 自分の確率（その長さが役割の行動数と違えば `None`） |
| `TitForTat` | 相手の直前の行動の純粋戦略（まだ無ければ `coop`） |
| `MyopicBestResponse` | `None` |
| `FictitiousPlay` | 信念の数に対する最適応答．`tie_break_lowest()` なら最も小さい番号の純粋戦略，`tie_break_random()` なら厳密に同点のものすべてに等しい重み |
| `LogitChoice` | ロジット選択の確率 |
| `RothErev` | 選択の確率 |
| `Automaton` | 次の行動の確率．純粋戦略．ただし GTFT で相手が裏切った後は `coop` に `generosity`，残りをもう一方の行動に |
| 自作の規則 | `mixed` を実装しなければ `None` |

学習則は，エージェントごとに 1 つの状態（`global`）から報告します．
`DecisionScope::PerEncounter` は相手ごとに状態を持つので，`assemble` はこの粒度での `report_mixed(true)` を拒否します（`GameError::Metrics`）．
`assemble_with_llm` は `report_mixed` を受け付けますが，LLM の種類は `None` を報告します．

```rust
use std::sync::Arc;

use socsim_core::AgentId;
use socsim_games::{
    assemble, build_world, AssignGroup, AssignKind, Assignment, FictitiousPlay, FixedPlayers,
    GameMetrics, InitialActions, MatrixGame, MetricsOptions, MixedSource, NormalFormOutcome,
    PlayOptions, RuleBook,
};
use socsim_log::InMemoryRecorder;

fn main() {
    // Matching pennies: role 0 wins on a match, role 1 on a mismatch.
    let game = MatrixGame::zero_sum(&[vec![1.0, -1.0], vec![-1.0, 1.0]]).unwrap();
    let metrics = GameMetrics::new(
        &game,
        MetricsOptions::new().mixed_source(MixedSource::Reported),
    )
    .unwrap();

    let mut rules = RuleBook::new();
    rules
        .register("fictitious", Arc::new(FictitiousPlay::new().tie_break_random()))
        .unwrap();

    let seed = 1;
    let world = build_world(
        game, 2, (), InitialActions::Same(1), &rules,
        &Assignment::Ordered(vec![
            AssignGroup { kind: AssignKind::Rule("fictitious".into()), count: 1 },
            AssignGroup { kind: AssignKind::Held, count: 1 },   // always plays action 1
        ]),
        10, 10, seed,
    )
    .unwrap();

    let mut sim = assemble(world, FixedPlayers::pairs(&[(0, 1)]), rules,
                           NormalFormOutcome, PlayOptions::new().report_mixed(true), seed)
        .unwrap()
        .add_mechanism(Box::new(metrics))
        .recorder(Box::new(InMemoryRecorder::new()))
        .build();

    // Before any Reward phase there is no report.
    println!("before t=1: {:?}", sim.world().reported_mixed(AgentId(0), 0));
    sim.step().unwrap();
    println!("played at t=1: {}", sim.world().held()[0]);
    println!("reported after t=1: {:?}", sim.world().reported_mixed(AgentId(0), 0));
    println!("held agent: {:?}", sim.world().reported_mixed(AgentId(1), 1));

    let recorder = sim.recorder().as_any().unwrap()
        .downcast_ref::<InMemoryRecorder>().unwrap();
    for row in recorder.metrics().iter().filter(|row| row.key == "game.regret") {
        println!("t={} {} = {}", row.t, row.key, row.value);
    }
}
```

```text
before t=1: None
played at t=1: 0
reported after t=1: Some([0.0, 1.0])
held agent: None
t=1 game.regret = 2
```

まだ何も観察していないので，仮想プレイは同点を乱数で崩して `0` を打ちました．相手が `1` を打つのを見た後は，最適応答の純粋戦略 `1` を報告します．
後悔は役割 0 にこの報告を，役割 1 に保持行動のエージェントが打った行動を使います．役割 1 は一致すると負けるので，切り替えれば 2 多く得られます．

### 公開集計値：`AggregateGame`

ゲームは，各対戦の行動の組から計算した値 — 「この対戦は相互協力だったか」「何人が拠出したか」 — を，
どの規則も読める公開情報として出せます．ゲームに `AggregateGame`（既定のビルド）を実装します．

```rust
pub trait AggregateGame: NormalFormGame {
    fn aggregate_spec(&self) -> &[AggregateSpec];
    /// `out.len() == self.aggregate_spec().len()`; every written value must be finite.
    fn aggregate_into(&self, profile: &[Self::Action], out: &mut [f64]);
}
```

`AggregateSpec::new(name, AggregateReduce::{Sum, Mean})` は名前を検査します．1〜49 バイトで，先頭は ASCII の小文字か数字，
残りは小文字，数字，`_`，`-` です（`AggregateSpecError::{EmptyName, InvalidStart, InvalidCharacter { index }, TooLong { length }}`．
デシリアライズでも同じ検査が走ります）．したがって名前は指標名 `game.aggregate.<name>` の 1 区切りになります．

`assemble` の代わりに `assemble_with_aggregates(.., seed, aggregate_history_len)` で組み立てます
（`assemble_with` では評価経路を包みます：`Pairwise::new(outcome).with_aggregates(n)` か `counting_play().with_aggregates(n)`）．
Interaction の終わりに `game_play` が対戦ごとに 1 回（`EncounterId` の順）`aggregate_into` を呼び，値をそのステップの対戦にわたって集約し，
`t` をキーとする 1 行にします．`Sum` は和（対戦の無いステップでは `0`），`Mean` は平均（対戦の無いステップでは定義されない）です．
最後の `aggregate_history_len` 行が World に保持されるので，snapshot が持ち運びます．有限でない値はステップを止めます．

規則は `DecisionContext::aggregate_history()` で行を読みます．集計値つきで組み立てていなければ `None` です．
行は Interaction で書かれ，決定は Decision で行われるので，決定に見えるのは**終わった前のステップだけ**です（`t = 1` では何も無い）．
`AggregateView` は `names()`，`len()`，`is_empty()`，`iter()`（`(t, &[Option<f64>])`，古い順），`last()`，`column(name)` を持ち，
定義されない値は `None` です．`game_metrics` はそのステップの行を `game.aggregate.<name>` として記録します．
`assemble_with_llm` には集計値つきの版が無く，LLM の組み立てではどの規則でも `aggregate_history()` は `None` です．

```rust
use std::borrow::Cow;
use std::sync::Arc;

use socsim_core::SimRng;
use socsim_games::{
    assemble_with_aggregates, build_world, ActionError, AggregateGame, AggregateReduce,
    AggregateSpec, AssignGroup, AssignKind, Assignment, BuiltinPolicyState, DecisionContext,
    DecisionRule, FiniteGame, Fixed, FixedPlayers, GameMetrics, InitialActions, MatrixGame,
    MetricsOptions, NormalFormGame, NormalFormOutcome, PlayOptions, RuleBook, RuleError,
    RuleInitContext, StageGame, Stateless,
};
use socsim_log::InMemoryRecorder;

/// The prisoner's dilemma plus two public, profile-level values.
#[derive(Clone, Debug)]
struct PdWithAggregates {
    pd: MatrixGame,
    specs: Vec<AggregateSpec>,
}

impl StageGame for PdWithAggregates {
    type Action = usize;
    fn n_roles(&self) -> usize { self.pd.n_roles() }
    fn check_action(&self, role: usize, action: &usize) -> Result<(), ActionError> {
        self.pd.check_action(role, action)
    }
}
impl NormalFormGame for PdWithAggregates {
    fn payoff(&self, role: usize, profile: &[usize]) -> f64 { self.pd.payoff(role, profile) }
}
impl FiniteGame for PdWithAggregates {
    fn n_actions(&self, role: usize) -> usize { self.pd.n_actions(role) }
    fn action_label(&self, role: usize, action: usize) -> Cow<'_, str> {
        self.pd.action_label(role, action)
    }
}
impl AggregateGame for PdWithAggregates {
    fn aggregate_spec(&self) -> &[AggregateSpec] { &self.specs }
    fn aggregate_into(&self, profile: &[usize], out: &mut [f64]) {
        out[0] = f64::from(u8::from(profile.iter().all(|&a| a == 0)));   // mutual_coop
        out[1] = profile.iter().filter(|&&a| a == 1).count() as f64;    // defections
    }
}

/// Cooperates while more than half of last step's encounters were mutual cooperation.
struct FollowTheCrowd;

impl DecisionRule<PdWithAggregates, ()> for FollowTheCrowd {
    fn name(&self) -> &str { "follow_the_crowd" }
    fn init_state(&self, _: &RuleInitContext, _: &PdWithAggregates)
        -> Result<BuiltinPolicyState, RuleError> {
        Ok(BuiltinPolicyState::Stateless(Stateless))
    }
    fn decide(&self, _: &mut BuiltinPolicyState,
              ctx: &DecisionContext<'_, PdWithAggregates, ()>,
              _: &mut SimRng) -> Result<usize, RuleError> {
        let last = ctx.aggregate_history()                 // None without aggregates
            .and_then(|history| history.last())            // previous step only
            .and_then(|(_, values)| values[0]);            // mutual_coop
        Ok(match last {
            Some(share) if share <= 0.5 => 1,
            _ => 0,                                        // first step: cooperate
        })
    }
}

fn main() {
    let pd = MatrixGame::bimatrix(
        &[vec![3.0, 0.0], vec![5.0, 1.0]],
        &[vec![3.0, 5.0], vec![0.0, 1.0]],
    )
    .unwrap()
    .with_labels(vec![vec!["C".into(), "D".into()]; 2])
    .unwrap();
    let game = PdWithAggregates {
        pd,
        specs: vec![
            AggregateSpec::new("mutual_coop", AggregateReduce::Mean).unwrap(),
            AggregateSpec::new("defections", AggregateReduce::Sum).unwrap(),
        ],
    };
    let metrics = GameMetrics::new(&game, MetricsOptions::new()).unwrap();

    let mut rules: RuleBook<PdWithAggregates, ()> = RuleBook::new();
    rules.register("follow_the_crowd", Arc::new(FollowTheCrowd)).unwrap();
    rules.register("fixed_defect", Arc::new(Fixed::new(1))).unwrap();

    let seed = 5;
    let world = build_world(
        game, 4, (), InitialActions::Same(0), &rules,
        &Assignment::Ordered(vec![
            AssignGroup { kind: AssignKind::Rule("follow_the_crowd".into()), count: 3 },
            AssignGroup { kind: AssignKind::Rule("fixed_defect".into()), count: 1 },
        ]),
        10, 3, seed,
    )
    .unwrap();

    let mut sim = assemble_with_aggregates(
        world, FixedPlayers::pairs(&[(0, 1), (2, 3)]), rules, NormalFormOutcome,
        PlayOptions::new(), seed,
        8,                                                 // aggregate_history_len
    )
    .unwrap()
    .add_mechanism(Box::new(metrics))
    .recorder(Box::new(InMemoryRecorder::new()))
    .build();
    sim.run().unwrap();

    let recorder = sim.recorder().as_any().unwrap()
        .downcast_ref::<InMemoryRecorder>().unwrap();
    for row in recorder.metrics().iter().filter(|row| row.key.starts_with("game.aggregate.")) {
        println!("t={} {} = {}", row.t, row.key, row.value);
    }
}
```

```text
t=1 game.aggregate.mutual_coop = 0.5
t=1 game.aggregate.defections = 1
t=2 game.aggregate.mutual_coop = 0
t=2 game.aggregate.defections = 4
t=3 game.aggregate.mutual_coop = 0
t=3 game.aggregate.defections = 4
```

`t = 1` では対 `(0, 1)` が相互協力し，対 `(2, 3)` はしないので `mutual_coop` は `0.5` です．
`t = 2` では 3 人の追従者がこの行を読み，半分を超えていないので裏切ります．

### 収束による停止

`MetricsOptions::stop(tol, window)` は，打たれた行動の割合が動かなくなったら実行を終えます．
席が 1 つ以上あるステップの後に，`game_metrics` はそのステップの `played_share` の値を，**席があった直前のステップ**の値と比べます．

- そのような最初のステップには比べる相手が無く，カウンターは `0` です．
- それ以外では，$\max_k \lvert s_k(t) - s_k(\text{prev})\rvert < \texttt{tol}$ ならカウンターを 1 増やし，そうでなければ `0` に戻します．
- 席の無いステップはカウンターを `0` に戻し，直前の割合はそのまま残します．

カウンターが `window` に達すると，`game_metrics` はそのステップの行を記録してから停止を求め（`StepContext::request_stop`），
実行はそのステップの後で終わります．したがって最も早い停止はステップ `window + 1` です．最初の例の `stop(1e-9, 3)` は `t = 5` で実行を終えます．
カウンターと直前の割合は World に保持されるので，再開した実行も同じステップで止まります．

### feature `metrics`：行動のエントロピー

Cargo feature `metrics` を有効にすると，`game_metrics` は `game.action_entropy` も書きます．
共通の `played_share` の値の Shannon エントロピー（単位は nat）で，`socsim_metrics::stats::shannon_entropy` で計算します．
feature のもとでは既定で有効で（`MetricsOptions::action_entropy(false)` で切る），共通 ID のゲームでだけ現れます．
役割別の割合のエントロピーは 2 つの分布を混ぜてしまうからです．この feature はほかに何も足しません．

### 列の説明：`describe`

`GameMetrics::describe()` は，この設定が書きうる列について `(pattern, meaning)` の組を返します．
`game.played_share.{action}`，`game.mean_payoff`，`game.payoff_sd`，有効なら `game.coop_rate`，`game.regret`，`game.eq_distance`，
`game.action_entropy`，`game.population_share.{action}`（列は `game.population_share.a<k>`），そして `game.aggregate.{name}`（組み立て方に依存するので常に並ぶ）です．
パターンでは，`.` で区切った 1 つの区切り全体を `{…}` と書いたものが列名の区切りちょうど 1 つに当たります．`{…}` と文字が 1 つの区切りに混ざることはありません．
そのため [runvault](https://github.com/akitenkrad/rs-runvault) で実行を記録するリポジトリは，これをそのまま `runvault.toml` に写せます．
`{…}` の区切りを含むパターンは `[[metric_patterns]]` に，そのままの名前は `[metrics."<name>"]` に書きます．
`ContinuousMetrics::describe()` も同じ規則に従うので，役割ごとの列 `game.mean_action.r<r>-<d>` は `game.mean_action.{role_component}` と説明されます（今組み立てられる World ではこの列は記録されません．[指標：`ContinuousMetrics`](#指標continuousmetrics) を参照）．
[行動 ID](#行動-idmetric_action_ids) の例のプログラムがちょうどそれを出力します．

```toml
[[metric_patterns]]
pattern = "game.played_share.{action}"
meaning = "Share of all played seats choosing `{action}`. `{action}` is a metric_id; display labels come from the game's metric actions. Actions are pooled across roles."

[metrics."game.mean_payoff"]
meaning = "Mean aggregated payoff among agents who played at least once in the step."

[metrics."game.payoff_sd"]
meaning = "Population standard deviation of aggregated payoff among agents who played at least once in the step."

[metrics."game.coop_rate"]
meaning = "Share of all played seats choosing the configured cooperation label for that seat's role."

[metrics."game.regret"]
meaning = "Maximum unilateral-deviation regret of the configured mixed-strategy profile."

[[metric_patterns]]
pattern = "game.aggregate.{name}"
meaning = "Public game aggregate for the completed step; present only when the simulation was assembled with aggregates."
```

`MetricsOptions::payoff_timing(PayoffTiming::AfterUpdate)` では，パターンは同じままで，`game.played_share.{action}`，`game.mean_payoff`，`game.payoff_sd`，`game.coop_rate` の意味が «the actions held after every update of the step (asynchronous update)» から来ることを述べる文になります．
`assemble_async` で組み立てたシミュレーションに使ってください（[非同期更新](#非同期更新) を参照）．

`unit`，`direction`，パターンの `axes` が欲しければ手で足してください．この crate 自体は runvault に依存しません．

---

## 決定論性と乱数ストリーム

すべてのストリームは，`build_world` と `assemble` に渡す 1 つの `seed`（または `GameSimFactory::build` に渡す seed，`GameTrialFactory` の試行の seed）から派生します．

| ラベル | ストリーム |
|---|---|
| `derive_seed(seed, &[0])` | World の初期化（`Assignment::Shuffled`） |
| `derive_seed(seed, &[0, 1])` | Scenario のランダムネットワーク（`topology = "network"`）．`GameSimFactory::topology` に渡すネットワークを自分で生成するときにも，このストリームを推奨する |
| `derive_seed(seed, &[0, 2])` | `InitialActions::Shares` の配置と `uniform_initial_actions` の乱数 |
| `derive_seed(seed, &[1])` | エンジン（スケジューラー，およびステップの RNG を通した `Matching`，`decide`，集団動学の乱数） |
| `derive_seed(seed, &[2])` | `decision_root`．World に保存される．`game_llm_decide` は生成のシードとフォールバックのストリームをここから導く（[シード](#game_llm_decide-の-decision-の流れ) を参照） |
| `derive_seed(seed, &[3, agent, generation])` | 規則の状態の初期化（`RuleInitContext::init_seed`） |

したがってゲームのシミュレーションではラベル `[2]` と `[3]` が予約済みです．独自のストリームには `[4]` 以上を使ってください
（[RNG ストリームのラベル付け規約](library.ja.md#rngストリームのラベル付け規約) を参照）．
学習は `AgentId` の昇順で，決定はスケジューラーの活性化の順（1 人のエージェントの中では決定の単位の順）で行われます．
組込みの学習則が引くのは `decide` に渡されるエンジンのストリームだけです（[乱数](#乱数) を参照）．
同じ seed と同じコードなら，実行はステップごとに再現されます．
`game_metrics` と混合戦略の報告は乱数を引かず，規則の状態，保持行動，対戦の表に触れないので，足しても実行の軌跡は変わりません．
集計値の表も乱数を引きません．

集団動学も同じエンジンのストリームから，Reward で学習の後に，`AgentId` の昇順にエージェントを訪れて引きます．
`fermi_update` と `replicator_update` は，エージェントごとに相手を 1 回（`gen_range`），続いて一様な `f64` を 1 回引きます．近傍を持たないエージェントは引きません．
`imitate_best` は何も引きません．
`moran_update` は 1 ステップに 4 回引きます．親のための一様な `f64`，死ぬエージェントのための `gen_range`，変異するかを決める一様な `f64`，変異体のための一様な行動です（連続ゲームでは，ガウスの変異体のために成分ごとに一様な `f64` を 2 つ）．
変異体の行動は，変異が起きないときも引きます．`DampedBestResponse` と `GradientAscent` は何も引きません．
`RandomPairs` と `RandomGroups` は Decision で 1 ステップに 1 回並べ替えを引き，ほかの組込みの Matching は何も引きません．
動学は World の外に状態を持たないので，snapshot から正確に再開できます．

`async_population_step` も同じエンジンのストリームから，Reward で活性化を 1 回ずつ順に行いながら引きます．
`AsyncActivation::UniformWithReplacement` では，活性化のたびにまずエージェントを引き（`gen_range(0..N)`），続いて規則の乱数を引きます（上と同じく，`fermi_update` と `replicator_update` は相手と `f64` を 1 つ，`imitate_best` は何も引きません）．$N$ 人を先にまとめて引くことはしません．
`AgentOrder` では規則の乱数だけを引き，スケジューラーの順に従います．
作業領域の利得は活性化のたびに計算し直すので，これも snapshot から正確に再開できます．

---

## 有限ゲームを解く

シミュレーションとは別に，この crate は任意の `FiniteGame`（`MatrixGame`，[組込みのゲーム](#組込みのゲームcatalog)，自作の型）の
均衡とそれに関連する解概念を直接計算します．連続ゲームの均衡は [最適応答反復](#最適応答反復) で求めます．混合戦略は `socsim_games::mixed` に，ソルバーは `socsim_games::solve` にあります．
どちらも既定のビルドに含まれ，feature も `socsim-*` crate も要りません（線形計画は純 Rust の `microlp` crate で解きます）．

### 混合戦略

`MixedStrategy` は 1 つの役割の行動上の確率ベクトルで，検査済みのものです．並びは行動の番号順です．

| コンストラクタ | 結果 |
|---|---|
| `MixedStrategy::new(probs)` | 与えたベクトルそのもの（正規化し直さない）．空のとき，NaN・無限大・負の要素があるとき，和と 1 の差が `1e-9` を超えるときは `MixedError::NotADistribution` |
| `MixedStrategy::uniform(n)` | `n` 個の行動それぞれに `1/n` |
| `MixedStrategy::pure(n, action)` | `action` に確率 1 |

混合戦略の組は，役割ごとに 1 つの `MixedStrategy` を並べたスライスです．次の関数は役割の数を問わずこれを評価します（行動の組をすべて列挙します）．

| 関数 | 返すもの |
|---|---|
| `mixed::expected_payoff(&game, role, &profile)` | その役割の期待利得 `F_i(σ)` |
| `mixed::action_payoffs(&game, role, &profile)` | その役割の各純粋行動 `s_i` に対する `F_i(s_i, σ_-i)` |
| `mixed::regrets(&game, &profile)` | 役割ごとの後悔（最良の純粋行動の利得 − 期待利得） |
| `mixed::regret(&game, &profile)` | `regrets` の最大値 |

役割の数が合わない組や長さの合わないベクトルは `MixedError::{RoleCount, Dimension}` になります．

混合戦略はソルバー側だけの概念です．シミュレーションでは決定規則は常に 1 つの `G::Action`（`FiniteGame` なら行動の番号）を返し，
`MixedStrategy` が行動になることはありません．混合を打つ規則は，自分の RNG でそこから行動を 1 つ引きます．
ただし規則は，次に打つ混合を*報告*できます（[混合戦略の報告](#混合戦略の報告) を参照）．

### 許容誤差と検証

ソルバー内の比較はすべて，ゲームごとに利得の幅で尺度を合わせた 1 つの許容誤差を使います．

```text
tol = 1e-10 × max(1, 利得の最大値 − 利得の最小値)    （全役割・全行動の組にわたって）
```

`solve::tolerance(&game)` がこれを返します．ソルバーは手法そのものを信用しません．
均衡を返すソルバーはどれも，見つけた組の後悔を元のゲームで計算し直して添え（`regret` フィールド），
後悔が `tol` 以下のときに限って結果を返します．
解く前には，どのソルバーもゲームを検査します．役割が 1 つ以上あること，行動のない役割がないこと，
行動の組が 10⁶ 以下であること（`SolveError::TooLarge`），利得がすべて有限であること（`SolveError::NonFinitePayoff`）です．

混合戦略の均衡は `Equilibrium { strategies, payoffs, regret }` で返ります．役割ごとの `MixedStrategy` と期待利得，
そして検証済みの最大の後悔です．`is_pure()` は，すべての戦略が 1 つの行動に確率 1 を置くとき真です．

### ソルバーの一覧

| 関数 | プレイヤー | 返すもの | 制限 |
|---|---|---|---|
| `pure_nash(&game)` | n 人 | `Vec<PureNash>`：純粋戦略の均衡すべて | 行動の組をすべて列挙（10⁶ 以下） |
| `iterated_strict_dominance(&game)` | n 人 | `DominanceResult`：残った行動と除去の記録 | 純粋戦略による強支配だけ |
| `support_enumeration(&game)` | 2 人 | `EquilibriumSet`：非退化なゲームの均衡すべてと `findings` | 台の組が 2²⁰ 以下（10 × 10 まで） |
| `lemke_howson(&game, &options)` | 2 人 | `LemkeHowsonResult`：均衡 1 つ | どれが返るかは落とすラベルで決まる |
| `zero_sum_minimax(&game)` | 2 人，ゼロ和 | `MinimaxSolution`：ゲームの値と最適戦略 | 線形計画．主問題と双対問題を両方解いて検査する |
| `maximin_security_level(&game, role)` | 2 人 | `SecurityLevel`：1 つの役割のマキシミン保証水準 | 線形計画．主問題と双対問題を両方解いて検査する |
| `logit_qre(&game, &options)` | n 人 | `QreSolution`：与えた λ でのロジット QRE 1 つ | 局所的な不動点反復であり，分枝の追跡ではない |
| `pure_nash_counts(&game)` | n 人，匿名 | `Vec<CountProfile>`：純粋戦略の均衡すべてを人数ベクトルで | `G: AnonymousGame`．人数ベクトルは 10⁶ 以下 |
| `replicator_integrate(&game, &x0, &options)` | 2 人，対称 | `ReplicatorRun`：レプリケータ動学の軌道の終点 | 刻み一定の古典的な RK4 |
| `replicator_fixed_points(&game)` | 2 人，対称 | `FixedPointSet`：単体のすべての面の静止点 | 行動は 20 以下 |
| `ess_verdict(&game, &resident)` | 2 人，対称 | `EssVerdict::{True, False, Inconclusive}` | 接空間の二次形式で判定．僅差では `Inconclusive` |

エラーは `SolveError` です．上のゲームの検査のほか，ソルバーが受け付けない役割の数の `UnsupportedRoles`，
`NotZeroSum`，`InvalidLabel`，`MaxPivots`，`Lp(LpFailure)`，`VerificationFailed { duality_gap, regret }`，
`NotConverged { residual, iterations }`，`InvalidOptions`，`Mixed(MixedError)` があります．

### `pure_nash`

行動の組を辞書式の順（役割 0 の行動がもっとも上位の桁，最後の役割の行動がもっとも速く変わる）にすべて訪れ，
どの役割も一方的な逸脱で `tol` を超えて得をしない組を残します．
`PureNash { profile, payoffs, regret }` をこの順で返します．列挙は網羅的なので，この一覧はゲームの純粋戦略の均衡のすべてです（`tol` の範囲で）．
費用は行動の組の数×行動の総数で，行動の組が 10⁶ を超えるゲームは拒否します．

### `iterated_strict_dominance`

各ラウンドで，すべての役割について同時に，同じ役割の別の残っている**純粋**行動に強支配される行動
（残っている他の役割の行動のどの組み合わせに対しても `tol` を超えて劣る行動）をすべて取り除き，
何も取り除かれなくなるまで繰り返します．`DominanceResult { surviving, eliminated }` を返します．
`surviving` は役割ごとの残った行動の番号（昇順），`eliminated` は取り除いた行動ごとの `Elimination { round, role, action }`（ラウンドは 1 から数える）です．

強支配による除去は Nash 均衡を取り除かず，最終結果は取り除く順序に依存しません．よく使われる 2 つの変種は意図して入れていません．

- **弱支配**は使いません．結果が取り除く順序に依存しうるうえ，均衡を取り除くことがあるからです．
- **混合戦略による支配**は調べません．他の行動の混合にだけ支配される行動は，ここでは残ります．
  これを検出するには行動ごとに線形計画を解く必要があります．そのため，混合戦略による支配を認める教科書の定義より，残る集合が大きくなることがあります．

### `support_enumeration`

2 人ゲームだけを扱います．空でない台の組（役割 0 の台を外側，役割 1 の台を内側のキーとし，それぞれ昇順の行動番号の辞書式順）をすべて調べ，
無差別条件の方程式を解き，負の確率が出たときや台の外の行動のほうが良いときはその台を退け，候補の後悔を検証します．
`EquilibriumSet { equilibria, findings }` を返します．`tol` の範囲で同じ均衡がもう一度見つかったときは，最初のものを残します．

**すべての均衡が見つかる条件．** 非退化なゲームでは，各均衡はその台での方程式のただ 1 つの解なので，均衡はすべて見つかります（その数は奇数です）．
退化したゲームでは，台の方程式の階数が落ちたり条件が悪かったりします．そうした台は推測で埋めず，`findings` に報告します．

| `SupportFinding` | 意味 |
|---|---|
| `Degenerate { support, reason }` | 階数が落ちていて，検証済みの点がない |
| `Component { support, base, directions }` | 正の次元を持つ均衡の集合の上の検証済みの点 `base` と，零空間の方向 `directions`（それぞれ役割 0 の確率，続けて役割 1 の確率と平坦に並べたもの）．`base` は `equilibria` にも加える |
| `Inconclusive { support, reason }` | 方程式の条件が悪い（条件数の推定が 10¹² を超える）か有限でない，または最後の検証に失敗した |

後悔の検査を通らない点を均衡として報告することはありません．`is_complete()` は `findings` が空のとき真です．
このときすべての台に決着がついており，非退化なゲームなら `equilibria` が均衡の全体です．

**制限．** 役割はちょうど 2 つです（それ以外は `UnsupportedRoles`）．台の組の数 `(2^m − 1)(2^n − 1)` は 2²⁰ 以下でなければならず，
10 × 10 は受け付け，11 × 10 は受け付けません（`TooLarge`．このとき `profiles` フィールドには台の組の数が入ります）．
計算は浮動小数点で行い，厳密な有理数演算はしないので，退化に近いゲームの判定は `tol` で決まります．

### `lemke_howson`

2 人ゲームだけを扱います．相補ピボットの経路を 1 本たどり，均衡をちょうど 1 つ返します（`LemkeHowsonResult { equilibrium, pivots }`）．

| `LemkeHowsonOptions` | 既定値 | 意味 |
|---|---|---|
| `dropped_label` | `0` | 最初に落とすラベル．`m × n` のゲームでは，ラベル `0..m` が役割 0 の行動，`m..m+n` が役割 1 の行動 |
| `max_pivots` | `100_000` | ピボットがこの回数に達したら `SolveError::MaxPivots { limit }` で止める |

`LemkeHowsonOptions::default().with_dropped_label(k).with_max_pivots(n)` で設定します．`m + n` 以上のラベルは `InvalidLabel` です．
ピボットの前に，各プレイヤーの利得を定数だけずらしてすべての要素を正にします（均衡は変わりません）．結果は，ずらす前の元のゲームで検証します．
退化したゲームで起きる最小比テストの同点は，その時点の基底の逆行列による辞書式の規則で解消します．その規則でもなお同点なら，`Lp(LpFailure::Internal)` で止まります．

**何が返るか．** 非退化なゲームでは，どのラベルを落としても均衡に着きます．どの均衡に着くかはラベルで決まり，どのラベルからも着かない均衡もあります．
混合戦略の均衡があっても純粋戦略の均衡が返ることがあります（下の男女の争いでは，どのラベルでも純粋戦略の均衡に着きます）．
均衡をすべて列挙するには `support_enumeration` を使ってください．

### `zero_sum_minimax` と `maximin_security_level`

どちらも `microlp` で線形計画の組を解きます．主問題（最大化する役割の混合戦略と，それで保証できる利得）と，
双対問題（相手の混合戦略と，それで最初の役割を抑え込める利得）です．

`zero_sum_minimax(&game)` は 2 つの役割とゼロ和（すべての行動の組で `|u_0 + u_1| ≤ tol`．そうでなければ `NotZeroSum`）を要求します．
ミニマックス定理により有限の 2 人ゼロ和ゲームはどれも値を持ち，主問題と双対問題の最適値はどちらもそれに等しくなります．
返す前に，双対ギャップ `|主問題 − 双対問題|`，戦略の組の後悔，役割 0 の期待利得と値の差，
各役割の最良の純粋行動の利得と値の差（役割 0 は `+value`，役割 1 は `-value`）がすべて `tol` 以下であることを確かめ，
そうでなければ `VerificationFailed { duality_gap, regret }` を返します．
返すのは `MinimaxSolution { value, row, col, duality_gap, regret }` で，`value` は役割 0 から見た値です．
最適戦略が一意でないとき，どの最適な組が返るかは線形計画ソルバー次第です．

`maximin_security_level(&game, role)` はゼロ和かどうかを問わず任意の 2 人ゲームを受け付けます．
`SecurityLevel { level, strategy, duality_gap }` を返します．`level` は `role` が相手のどの純粋行動に対しても保証できる最大の利得で，
`strategy` はそれを達成する混合戦略です．双対ギャップと，その戦略が相手のどの行動に対しても実際に `level` を保証すること（`tol` の範囲で）を確かめます．
0 と 1 以外の役割は `InvalidOptions` です．一般和ゲームでは，これは保証であって均衡戦略ではありません．

線形計画のバックエンドが実行不能・非有界を報告したとき，中断されたとき，最適でない実行可能解で止まったとき，内部で失敗したときは，
結果ではなく `SolveError::Lp(LpFailure::{Infeasible, Unbounded, Interrupted, NotOptimal, Internal})` を返します．

### `logit_qre`

1 つの精度 λ でのロジット量的応答均衡（QRE）を，プレイヤーの数を問わず求めます．
各役割の戦略が，他の役割に対する `λ × action_payoffs` のソフトマックスに一致する組です．
`init`（なければ一様な組）から始めて，減衰つきの更新

```text
σ_i ← (1 − damping) σ_i + damping × softmax(λ F_i(·, σ_-i))
```

を，不動点の残差（`σ` とそのロジット応答の差の絶対値の最大）が `tol` 以下になるまで繰り返します．

| `QreOptions` | 既定値 | 意味 |
|---|---|---|
| `lambda` | （`QreOptions::new(lambda)` の引数） | 精度．有限で `0` 以上．`0` なら `init` によらず一様な組になる |
| `init` | `None`（一様） | 初期の組．`.with_init(profile)` で設定 |
| `damping` | `0.1` | `(0, 1]` の範囲．`.with_damping(d)` で設定 |
| `tol` | `1e-12` | 有限で正．`.with_tol(t)` で設定 |
| `max_iter` | `100_000` | 正．`.with_max_iter(n)` で設定 |

不正なオプションは `InvalidOptions`，形の合わない `init` は `MixedError` です．残差は返す組で計算し直し，
それが `tol` 以下のときに限って `QreSolution { lambda, strategies, residual, regrets, iterations }` を返します．
そうでなければ結果は `NotConverged { residual, iterations }` です．最後の反復値で代用することはありません．

**これは局所的な探索であって，主分枝の追跡ではありません．** 同じ λ でもゲームが複数のロジット QRE を持つことがあり，
反復は `init` からたどり着いた不動点に収束します．初期値が違えば別の QRE の分枝に着くことがあります（下の鹿狩りを参照）．
λ が大きいと減衰つきの反復が落ち着かないことがあります．そのときは減衰を小さくするか，λ を段階的に上げ，各解を次の `init` に渡してください．
こうすると多くの場合 1 本の分枝をたどれますが，QRE の対応の折り返しや分岐を越えられる保証はありません．

有限の λ ではロジット QRE は Nash 均衡ではありません．`regrets` は返した組での各役割の通常の（正則化しない）後悔です．
厳密なロジット QRE ではこれは `ln |S_i| / λ` 以下（`|S_i|` はその役割の行動の数）なので，λ が大きくなるほど小さくなります．
ソルバーはこの後悔を報告しますが，受け入れの判定には使いません．

### `pure_nash_counts`

[匿名ゲーム](#匿名ゲームと-commonactionspace) では各行動を選ぶプレイヤーの人数だけが意味を持つので，均衡は行動の組ではなく**人数ベクトル**になります．
`pure_nash_counts(&game)` は，`n_roles()` 人のプレイヤーを共通の行動の集合に分けるすべての人数ベクトル（$m^n$ 個の行動の組ではなく $\binom{n + m - 1}{m - 1}$ 個の組成）を調べ，
どのプレイヤーも行動を変えて `tol` より多く得をしないものを残します．利得は `payoff_by_counts` で評価します．
`CountProfile { counts }` を `counts` の辞書式の順で返します．
すべての役割の行動数が等しくなければならず（違えば `InvalidOptions`），人数ベクトルが 10⁶ を超えると `TooLarge` です．
`ElFarol::new(100, 60)` の均衡は `[60, 40]`（出席者数が定員に等しい）だけで，`MinorityGame::new(n)` の均衡は `[(n−1)/2, (n+1)/2]` と `[(n+1)/2, (n−1)/2]` の 2 つです．

### `replicator_integrate` と `replicator_fixed_points`

どちらも**対称な 2 人ゲーム**（`is_symmetric()`．そうでなければ `UnsupportedRoles` か `InvalidOptions`）を，役割 0 の利得行列 $A$ と Taylor & Jonker（1978）のレプリケータ動学で扱います．

$$\dot x_i = x_i\,\big((Ax)_i - x^\top A x\big)$$

`replicator_integrate(&game, &x0, &ReplicatorOptions)` は，混合戦略 `x0` から古典的な 4 次の Runge–Kutta 法と一定の刻み `dt` で積分します．
各刻みの後，0 より下で `tol` 以内の成分は 0 にし，状態を単体に正規化し直します（それより大きな負の成分や有限でない状態は `NotConverged`）．
$\lVert \dot x \rVert_\infty <$ `tol` になった時点で止まり（`converged: true`），`t` が `t_max` に達したときも止まります（`converged: false`）．
`ReplicatorRun { state, t, steps, converged }` を返します．

| `ReplicatorOptions` | 既定 | 意味 |
|---|---|---|
| `dt` | `0.01` | 刻み．有限で `> 0`．`.with_dt(dt)` で設定 |
| `t_max` | `10_000` | 時間の上限．有限で `≥ 0`．`.with_t_max(t)` で設定 |
| `tol` | `1e-12` | $\lVert \dot x \rVert_\infty$ に対する停止の閾値．有限で `> 0`．`.with_tol(t)` で設定 |

`replicator_fixed_points(&game)` は動学の静止点を列挙します．すべてのサポート（単体のすべての面）について，サポート内の行動の無差別条件 $(Ax)_i = (Ax)_j$ を解きます．
`FixedPointSet { isolated, components, inconclusive }` を返します．
`isolated` は，面の内部に一意の解を持つ `FixedPoint { support, point }` です．
`components` は，`base` を通り `directions` に沿う正の次元の静止点の族 `AffineComponent { support, base, directions }` です（零ゲームでは単体全体が 1 つの族になります）．
`inconclusive` は，方程式が悪条件（条件数の推定が 10¹² 超）だったか判定できなかったサポートの一覧です．
より大きな族に含まれる族と，族の上にある孤立点は除きます．
静止点は Nash 均衡とは限りません．どのゲームでも，純粋戦略はすべて静止点です．行動が 20 を超えると `TooLarge` です．

### `ess_verdict`

`ess_verdict(&game, &resident)` は，対称な 2 人ゲームの混合戦略 `resident` が Maynard Smith（1974）の意味の進化的に安定な戦略（ESS）かを判定します．
$p$ が ESS であるとは，$p$ と違うどの変異体 $q$ についても，$E(p, p) > E(q, p)$ か，$E(p, p) = E(q, p)$ かつ $E(p, q) > E(q, q)$ が成り立つことです．
`EssVerdict` を返します．

- `resident` に対して `resident` より良い純粋戦略があれば `False` です（対称な Nash 均衡ではない）．
- そうでなければ，効くのは `resident` への最適応答から作った変異体です．最適応答がちょうどサポートの行動に一致するときは，その面の接空間での利得の二次形式の符号で決めます．
  対称部分の固有値がすべて `−tol` より下なら `True`，`tol` より上の固有値があれば `False`，二次形式が消えるとき（標準のじゃんけんの一様混合など）は `False` です．
- サポートの外にも最適応答があるとき（最適応答が 3 つ以下）は，変異体の向きがなす錐の上で二次形式を調べます．
- 判定を決める固有値や向きの値が 0 から `tol` 以内のとき，またはサポートの外に最適応答があり最適応答が 3 つを超えるときは `Inconclusive` です．

`tol` はゲームの [許容誤差](#許容誤差と検証) です．
`HawkDove::new(2.0, 4.0)` では混合 $(1/2, 1/2)$ が `True`，純粋なハトが `False` で，`HawkDove::new(2.0, 2.0)` では純粋なタカが `True` です．

### 例

**男女の争い：すべての均衡と 1 本の経路．** 台の列挙は 2 つの純粋戦略の均衡と混合戦略の均衡を見つけます．
Lemke–Howson は，どのラベルを落としても純粋戦略の均衡にしか着きません．

```rust
use socsim_games::catalog::BattleOfSexes;
use socsim_games::solve::{
    lemke_howson, pure_nash, support_enumeration, tolerance, LemkeHowsonOptions,
};

fn main() {
    // Role 0 prefers (B, B) with 2, role 1 prefers (V, V) with 2;
    // miscoordination gives both -1.
    let game = BattleOfSexes::default();
    println!("tol = {:e}", tolerance(&game).unwrap());

    let pure: Vec<_> = pure_nash(&game).unwrap().into_iter().map(|e| e.profile).collect();
    println!("pure: {pure:?}");

    let all = support_enumeration(&game).unwrap();
    println!("complete: {}", all.is_complete());
    for eq in &all.equilibria {
        println!(
            "  {:.4?} {:.4?} payoffs {:.4?}",
            eq.strategies[0].probs(),
            eq.strategies[1].probs(),
            eq.payoffs
        );
    }

    // Labels 0..2 are role 0's actions, 2..4 are role 1's.
    for label in 0..4 {
        let found = lemke_howson(&game, &LemkeHowsonOptions::default().with_dropped_label(label))
            .unwrap();
        println!(
            "  label {label}: {:?} {:?} ({} pivots)",
            found.equilibrium.strategies[0].probs(),
            found.equilibrium.strategies[1].probs(),
            found.pivots
        );
    }
}
```

```text
tol = 3e-10
pure: [[0, 0], [1, 1]]
complete: true
  [1.0000, 0.0000] [1.0000, 0.0000] payoffs [2.0000, 1.0000]
  [0.6000, 0.4000] [0.4000, 0.6000] payoffs [0.2000, 0.2000]
  [0.0000, 1.0000] [0.0000, 1.0000] payoffs [1.0000, 2.0000]
  label 0: [1.0, 0.0] [1.0, 0.0] (2 pivots)
  label 1: [0.0, 1.0] [0.0, 1.0] (2 pivots)
  label 2: [1.0, 0.0] [1.0, 0.0] (2 pivots)
  label 3: [0.0, 1.0] [0.0, 1.0] (2 pivots)
```

利得の幅は 2 − (−1) = 3 なので `tol = 3e-10` です．混合戦略の均衡では，各役割が自分の好む行動を確率 0.6 で選び，期待利得は 0.2 です．

**じゃんけん：ミニマックス．** 値は 0 で，最適戦略はどちらも一様です．男女の争いはゼロ和ではありませんが，各役割は保証水準を持ちます．

```rust
use socsim_games::catalog::{BattleOfSexes, RockPaperScissors};
use socsim_games::solve::{maximin_security_level, zero_sum_minimax};

fn main() {
    let rps = RockPaperScissors::default();
    let solution = zero_sum_minimax(&rps).unwrap();
    println!("value {:.6}", solution.value);
    println!("row {:.6?}", solution.row.probs());
    println!("col {:.6?}", solution.col.probs());
    println!("gap {:e}, regret {:e}", solution.duality_gap, solution.regret);

    // A general-sum game has no minimax value, but each role still has a
    // maximin guarantee.
    let bos = BattleOfSexes::default();
    assert!(zero_sum_minimax(&bos).is_err()); // NotZeroSum
    let guard = maximin_security_level(&bos, 0).unwrap();
    println!("BoS role 0 guarantees {:.6} with {:.6?}", guard.level, guard.strategy.probs());
}
```

```text
value 0.000000
row [0.333333, 0.333333, 0.333333]
col [0.333333, 0.333333, 0.333333]
gap 0e0, regret 5.551115123125783e-17
BoS role 0 guarantees 0.200000 with [0.400000, 0.600000]
```

`regret` の下位の桁は浮動小数点の丸めによるものです．ソルバーが保証するのは `regret ≤ tol` です．

**鹿狩り：QRE の分枝．** 一様な初期値から λ を上げていくと `(hare, hare)` へ向かう分枝をたどります．
λ = 5 で `(stag, stag)` から始めると，`(stag, stag)` に近い別の QRE に着きます．

```rust
use socsim_games::catalog::StagHunt;
use socsim_games::solve::{logit_qre, QreOptions};
use socsim_games::MixedStrategy;

fn main() {
    let game = StagHunt::default();

    // Raise lambda step by step, starting each solve from the previous one.
    let mut previous = None;
    for lambda in [0.5, 1.0, 2.0, 5.0] {
        let mut options = QreOptions::new(lambda);
        if let Some(init) = previous.take() {
            options = options.with_init(init);
        }
        let qre = logit_qre(&game, &options).unwrap();
        println!(
            "lambda {lambda}: P(stag) = {:.4}, regrets {:.4?}",
            qre.strategies[0].probs()[0],
            qre.regrets
        );
        previous = Some(qre.strategies);
    }

    // The same lambda from a different start lands on a different fixed point.
    let stag = MixedStrategy::pure(2, 0).unwrap();
    let qre = logit_qre(&game, &QreOptions::new(5.0).with_init(vec![stag.clone(), stag])).unwrap();
    println!("lambda 5 from (stag, stag): P(stag) = {:.4}", qre.strategies[0].probs()[0]);
}
```

```text
lambda 0.5: P(stag) = 0.0334, regrets [0.2246, 0.2246]
lambda 1: P(stag) = 0.0009, regrets [0.0064, 0.0064]
lambda 2: P(stag) = 0.0000, regrets [0.0000, 0.0000]
lambda 5: P(stag) = 0.0000, regrets [0.0000, 0.0000]
lambda 5 from (stag, stag): P(stag) = 0.9901
```

どの後悔も `ln 2 / λ`（λ = 0.5 で約 1.39）を下回っています．

**集団のゲーム：人数の均衡，レプリケータ動学，ESS．**
匿名ゲームの均衡は人数ベクトルで返ります．タカ・ハトでは，タカ 10 % からのレプリケータ動学が安定な割合 $V/C$ に達します．これは唯一の内部の静止点で，ESS です．

```rust
use socsim_games::catalog::{ElFarol, HawkDove, MinorityGame, PublicGoodsFinite};
use socsim_games::solve::{
    ess_verdict, pure_nash_counts, replicator_fixed_points, replicator_integrate,
    ReplicatorOptions,
};
use socsim_games::MixedStrategy;

fn main() {
    // Anonymous games: equilibria as count vectors (number of players per action).
    let bar = ElFarol::new(100, 60).unwrap();
    let minority = MinorityGame::new(5).unwrap();
    let pgg = PublicGoodsFinite::new(5, 3.0, 1.0).unwrap();
    for (name, counts) in [
        ("El Farol (n = 100, capacity 60)", pure_nash_counts(&bar).unwrap()),
        ("minority game (n = 5)", pure_nash_counts(&minority).unwrap()),
        ("public goods (k = 5, r = 3)", pure_nash_counts(&pgg).unwrap()),
    ] {
        let counts: Vec<_> = counts.into_iter().map(|profile| profile.counts).collect();
        println!("{name}: {counts:?}");
    }

    // Hawk-dove with V = 2, C = 4: the stable hawk share is V / C = 0.5.
    let hd = HawkDove::new(2.0, 4.0).unwrap();
    println!("hawk_share_ess = {}", hd.hawk_share_ess());
    let x0 = MixedStrategy::new(vec![0.1, 0.9]).unwrap();
    let run = replicator_integrate(&hd, &x0, &ReplicatorOptions::new()).unwrap();
    println!(
        "from x0 = 0.1: x = {:.10} at t = {:.2} (converged: {})",
        run.state.probs()[0],
        run.t,
        run.converged
    );
    for point in replicator_fixed_points(&hd).unwrap().isolated {
        println!("rest point {:.4?} on support {:?}", point.point.probs(), point.support);
    }
    let mixed = MixedStrategy::new(vec![0.5, 0.5]).unwrap();
    let dove = MixedStrategy::pure(2, 1).unwrap();
    println!(
        "ESS: (0.5, 0.5) {:?}, pure dove {:?}",
        ess_verdict(&hd, &mixed).unwrap(),
        ess_verdict(&hd, &dove).unwrap()
    );
    let hawk = MixedStrategy::pure(2, 0).unwrap();
    println!("V = C = 2, pure hawk: {:?}", ess_verdict(&HawkDove::new(2.0, 2.0).unwrap(), &hawk).unwrap());
}
```

```text
El Farol (n = 100, capacity 60): [[60, 40]]
minority game (n = 5): [[2, 3], [3, 2]]
public goods (k = 5, r = 3): [[0, 5]]
hawk_share_ess = 0.5
from x0 = 0.1: x = 0.5000000000 at t = 53.07 (converged: true)
rest point [1.0000, 0.0000] on support [0]
rest point [0.0000, 1.0000] on support [1]
rest point [0.5000, 0.5000] on support [0, 1]
ESS: (0.5, 0.5) True, pure dove False
V = C = 2, pure hawk: True
```

$r = 3 < k = 5$ の公共財ゲームでは拠出しないことが支配戦略なので，均衡は誰も拠出しない配置だけです（`[0, 5]`：`C` が 0 人，`D` が 5 人）．
タカ・ハトでは $\dot x = x(1 - x)(1 - 2x)$ で，積分は $\lvert \dot x \rvert < 10^{-12}$ になった時点で止まります．
2 つの純粋な静止点は安定ではなく，純粋なハトは均衡ですらありません．
$V = C$ では安定な状態は純粋なタカです（$E(H, H) = E(D, H) = 0$ かつ $E(H, D) = V > E(D, D) = V/2$）．

---

## 連続ゲーム

連続ゲームでは，各役割が箱の中の実数ベクトル（生産量，価格，拠出額など）を選びます．
この crate は連続ゲームを有限ゲームと同じ部品の組で扱います．

| 部品 | 場所 | feature |
|---|---|---|
| トレイト `ContinuousGame` と `check_continuous_action` | crate の根 | 既定 |
| 組込みのゲーム 3 つ：`Cournot`，`BertrandDifferentiated`，`PublicGoodsContinuous` | `socsim_games::continuous` | 既定 |
| 減衰付きの最適応答反復ソルバーと黄金分割による最適応答 | `socsim_games::solve`（crate の根からも再エクスポート） | 既定 |
| 決定規則 `DampedBestResponse` と `GradientAscent` | crate の根 | `core` |
| `game_metrics` の連続版 `ContinuousMetrics` | crate の根 | `core` |
| `imitate_best`，`fermi_update`，`moran_update`（ガウスの突然変異つき）での連続の行動 | crate の根 | `core` / `engine` |
| LLM による決定：`ActionParser` としての `NumberParser`，`DescribedRenderer` | `socsim_games::llm` | `llm` |

`build_world`，Matching，`NormalFormOutcome`，`assemble`，`assemble_with_aggregates`，`assemble_with` はそのまま使えます．行動の型が `Vec<f64>` になるだけです．

### `ContinuousGame`

```rust
pub trait ContinuousGame: NormalFormGame<Action = Vec<f64>> {
    fn bounds(&self, role: usize) -> &[(f64, f64)];
    fn best_response(&self, role: usize, profile: &[Vec<f64>]) -> Option<Vec<f64>> { None }
    fn numeric_br(&self) -> NumericBr { NumericBr::Unavailable }
    fn gradient(&self, role: usize, profile: &[Vec<f64>]) -> Option<Vec<f64>> { None }
}
```

| メソッド | 意味 |
|---|---|
| `bounds(role)` | 役割の行動の成分ごとの閉区間 `(lo, hi)`．成分は 1 つ以上で，端点は有限，どの成分でも `lo < hi` でなければならない |
| `best_response(role, profile)` | 解析的な最適応答があれば返す．`profile` は**全役割**の行動を役割の番号順に並べたもの．`profile[role]` は読んではならず，結果はほかの役割の行動だけで決まる．結果は `bounds(role)` の中にある |
| `numeric_br()` | `NumericBr::Unimodal1D` は «どの役割の利得も 1 次元の自分の行動について単峰» という宣言で，解析的な最適応答が無いときに黄金分割の探索を許す．既定の `Unavailable` は許さない |
| `gradient(role, profile)` | 自分の行動についての利得の解析勾配（長さは `bounds(role).len()`）．`None` なら `GradientAscent` は数値微分を使う |

`best_response` が «ほかの役割» ではなく全員分の行動の組を受けるのは，ほかの役割だけを詰めた短いスライスでは，非対称なゲームで役割の番号がずれるからです．

**行動の検査．** `StageGame::check_action` は `check_continuous_action(self, role, action)` で実装します．
次の順に検査し，最初の違反を返します．

| 検査 | `ActionError` |
|---|---|
| `role < n_roles()` | `InvalidRole { role }` |
| `action.len() == bounds(role).len()` | `DimensionMismatch { role, expected, actual }` |
| すべての成分が有限 | `NonFiniteComponent { role, dim }` |
| すべての成分が自分の閉区間の中 | `OutOfBounds { role, dim }` |

エラーが持つのは添字だけで，違反した `f64` の値は持ちません．
`game_decide` は有限ゲームとまったく同じく，すべての決定に `check_action` を適用します．
NaN や区間の外の値を返す規則は，`OnInvalidAction::Stop` ではステップを止め，`Fallback` では置き換えられます．
`build_world` はすべての初期行動をすべての役割について検査し，`Fixed::new(vec![..])` は規則の状態の初期化のときに自分の行動を検査します（`RuleError::InvalidAction`）．

区間そのものは使う箇所で検査します．ソルバーは `SolveError::InvalidGame`，規則は `RuleError::Unsupported`，`ContinuousMetrics` は `GameError::Metrics`，集団動学は `GameError::Population`，`NumberParser` は `RuleError::Unsupported` を返します．

### 組込みの連続ゲーム

`socsim_games::continuous` は 1 次元の行動を持つゲームを 3 つ提供します．

| 型 | 役割 $i$ の行動 | 役割 $i$ の利得 | 構築時の検査 |
|---|---|---|---|
| `Cournot::new(n, a, b, c, q_max)` | 生産量 $q_i \in [0, q_{\max}]$ | $q_i\,(P - c)$．$P = a - bQ$，$Q = \sum_j q_j$ | `n ≥ 2`，`b > 0`，`a > c ≥ 0`，`q_max > 0` |
| `BertrandDifferentiated::new(n, a, b, d, c, p_max)` | 価格 $p_i \in [0, p_{\max}]$ | $(p_i - c)\,(a - b\,p_i + d \sum_{j \ne i} p_j)$ | `n ≥ 2`，`b > 0`，`d ≥ 0`，`2b > (n − 1) d`，`c ≥ 0`，`p_max > 0` |
| `PublicGoodsContinuous::new(k, endowment, r)` | 拠出額 $x_i \in [0, e]$ | $e - x_i + (r/k) \sum_j x_j$ | `k ≥ 2`，`endowment > 0`，`r ≥ 0` |

パラメータはすべて有限でなければならず（`CatalogError::NonFinite`），それ以外の違反は `CatalogError::Condition(_)` です．
`PublicGoodsContinuous::social_dilemma(k, e, r)` はさらに $1 < r < k$ を要求します．何も拠出しないことが支配戦略で，全額の拠出が社会的に最適になる範囲です．
`Cournot` の価格も `BertrandDifferentiated` の需要も 0 で切り捨てません．

理論値とその成立条件は次のとおりです．

| メソッド | 値 |
|---|---|
| `Cournot::equilibrium_quantity()` | 一意な対称均衡 $q^* = \min\{(a - c)/((n + 1)\,b),\ q_{\max}\}$ |
| `BertrandDifferentiated::equilibrium_price()` | $p^* = (a + bc)/(2b - (n - 1)\,d)$ として，$p^* < c$ なら `None`（均衡の需要が負になり，モデルの範囲の外），そうでなければ $\min\{p^*, p_{\max}\}$ |
| `PublicGoodsContinuous::equilibrium_contribution()` | $r < k$ なら `0`，$r > k$ なら全額（どちらも支配戦略）．$r = k$ ではどの拠出額も最適応答なので `None` |
| `PublicGoodsContinuous::social_optimum_contribution()` | 総利得 $k e + (r - 1) \sum_j x_j$ を最大にする拠出額．$r > 1$ なら全額，$r < 1$ なら `0`，$r = 1$ なら `None` |

どのゲームも解析的な最適応答を持ちます．制約の無い最適応答を区間へ射影したもので，クールノーは $\mathrm{clamp}\big((a - c - b\,Q_{-i})/(2b),\ 0,\ q_{\max}\big)$，ベルトランは $\mathrm{clamp}\big((a + bc + d \sum_{j \ne i} p_j)/(2b),\ 0,\ p_{\max}\big)$，公共財は $r \le k$ なら `0`，$r > k$ なら全額です．
どれも解析勾配を持ち，`NumericBr::Unimodal1D` を宣言し，`CommonActionSpace`，`DescribeGame`（プロンプト用の自然言語の説明），`AggregateGame` を実装します．

| ゲーム | 公開集計値（対戦をまたぐ集約） |
|---|---|
| `Cournot` | `total_output`（`Sum`），`price`（`Mean`） |
| `BertrandDifferentiated` | `mean_price`（`Mean`），`total_demand`（`Sum`） |
| `PublicGoodsContinuous` | `total_contribution`（`Sum`） |

型は serde で直列化でき，読込みの時にも構築子の検査を行います．

### 最適応答反復

`best_response_iteration(&game, &x0, &options)` は，任意の `ContinuousGame` の均衡を，**同時手番**の減衰付き最適応答で探します．
すべての役割が同じ行動の組に応答し，

$$x \leftarrow \mathrm{Proj}_{\text{bounds}}\big((1 - \alpha)\,x + \alpha\,\mathrm{BR}(x)\big)$$

と更新します．

減衰の無い同時手番の最適応答（$\alpha = 1$）は収束するとは限りません．
線形のクールノー競争では，内点の均衡は 3 社で中立安定，4 社以上で不安定です（Theocharis, 1960）．
反復は区間へ射影されるので発散はせず，周期軌道になります．3 社が $q = 20$ から始めると，減衰の無い写像は 25 と 20 を永遠に交互にとります．
対称な方向では，減衰付きの写像は偏差を $1 - \alpha (n + 1)/2$ 倍するので，線形クールノーの内点では $\alpha < 4/(n + 1)$ のときに限って収束します．
そこでソルバーは既定で減衰をかけ，試行が失敗するたびに $\alpha$ を半分にして `x0` からやり直します．

| `BrIterationOptions` のメソッド | 既定値 | 意味 |
|---|---|---|
| `new()` / `default()` | — | 以下の既定値 |
| `damping(alpha)` | `0.5` | 最初の $\alpha$．有限で $(0, 1]$ の中 |
| `adaptive(on)` | `true` | 収束しなかった試行の後に $\alpha$ を半分にし，`x0` からやり直す．`false` なら最初の $\alpha$ だけを試す |
| `min_damping(m)` | $2^{-10}$ | 試す最小の $\alpha$．有限で正，`damping` 以下 |
| `tol(tol)` | 開始時に決める | $\max \lvert \mathrm{BR}(x) - x \rvert \le \texttt{tol} \cdot \max(1, w)$（$w$ はどの役割のものも含めて最も広い区間の幅）で受け入れる．指定しなければ，`x0` でいずれかの役割の最適応答が黄金分割の探索から来るときは `1e-7`，そうでなければ `1e-12` |
| `max_iter(n)` | `10_000` | $\alpha$ 1 つあたりの減衰付き更新の回数．正でなければならない |
| `oracle(&dyn BestResponseOracle<G>)` | なし | 解析的な最適応答を持たない役割の最適応答 |

結果は `BrIteration { profile, residual, status, damping, iterations }` です．
`residual` は `profile` での $\max \lvert \mathrm{BR}(x) - x \rvert$，`damping` は最後の試行の $\alpha$，`iterations` は全試行の減衰付き更新の回数の合計です．
`status` は，残差が許容誤差に達したら `BrStatus::Converged`，どの $\alpha$ でも達しなければ `BrStatus::MaxIter` で，そのときの `profile` は最後の反復値にすぎません．
**均衡として使ってよいのは `Converged` の行動の組だけです．**

**最適応答の出どころ．** 役割ごとに，次の順に使います．ゲームの解析的な `best_response`，oracle，黄金分割の探索（ゲームが `NumericBr::Unimodal1D` を宣言し，役割の行動が 1 成分のとき）．
どれも無ければソルバーは `SolveError::NoBestResponse { role }` を返します．選んだ最適応答が `Converged` でない，成分の数が違う，有限でない，のどれかのときも同じです．
多次元のゲームを座標ごとに探索することはしません．解析的な最適応答か oracle が要ります．
`BestResponseOracle<G>`（`Send + Sync`）は `best_response(&self, game, role, profile) -> BrResult` を実装します．`profile[role]` を読んではならず，`Converged` でない結果は失敗として扱われます．

**数値の最適応答の精度．** `golden_section_best_response(&game, role, &profile, &GoldenSectionOptions)` は，役割の区間の上で括弧を，幅が `1e-12 · max(1, hi − lo)`（`tol_width`）以下になるか 200 回縮めるまで（`max_iter`．状態は `MaxIter`）縮めます．
その後，区間の両端，最後の括弧の両端と中点の利得を比べて最良の点を返します（同点なら小さい行動）．区間の端にある最適応答はちょうどその値で返ります．
`BrResult::residual` は最後の括弧の幅です．
最大値の位置は利得の値の比較で決めており，利得は最大値の近くで 2 次まで平らなので，括弧をどれだけ狭めても位置は $\sqrt{\varepsilon}\,\lvert x \rvert$ 程度（$\varepsilon$ は `f64` の計算機イプシロン，$\sqrt{\varepsilon} \approx 1.5 \times 10^{-8}$）までしか定まりません．
反復の既定の許容誤差がこの経路で相対 `1e-7` になるのはこのためで，`1e-12` を求めると `MaxIter` に終わります．

ほかの失敗：区間が不正なら `SolveError::InvalidGame`，オプションが不正か，`x0` の役割の数が違うか行動が不正なら `SolveError::InvalidOptions` です．
`best_response_step(&game, &profile, damping, oracle)` は同時手番の更新を 1 回だけ行い（`damping = 1` なら減衰の無い写像），同じ条件で `NoBestResponse` を返します．

```rust
use socsim_games::continuous::{BertrandDifferentiated, Cournot, PublicGoodsContinuous};
use socsim_games::{best_response_iteration, best_response_step, BrIterationOptions};

fn main() {
    // P = 100 - Q, unit cost 10, quantities in [0, 100]: q* = 90 / (n + 1).
    for n in [3, 10] {
        let game = Cournot::new(n, 100.0, 1.0, 10.0, 100.0).unwrap();
        let x0 = vec![vec![0.0]; n];
        let run = best_response_iteration(&game, &x0, &BrIterationOptions::new()).unwrap();
        println!(
            "n = {n}: q* = {:.6}, found {:.6}, {:?}, damping {}, residual {:.1e}",
            game.equilibrium_quantity(),
            run.profile[0][0],
            run.status,
            run.damping,
            run.residual
        );
    }

    // Undamped simultaneous best responses cycle for three firms.
    let game = Cournot::new(3, 100.0, 1.0, 10.0, 100.0).unwrap();
    let mut x = vec![vec![20.0], vec![20.0], vec![20.0]];
    for t in 1..=4 {
        x = best_response_step(&game, &x, 1.0, None).unwrap();
        println!("undamped step {t}: {:?}", x.iter().map(|a| a[0]).collect::<Vec<_>>());
    }

    let bertrand = BertrandDifferentiated::new(2, 10.0, 2.0, 1.0, 2.0, 100.0).unwrap();
    println!("Bertrand p* = {:?}", bertrand.equilibrium_price());
    let pgg = PublicGoodsContinuous::social_dilemma(4, 20.0, 1.6).unwrap();
    println!(
        "public goods: equilibrium {:?}, social optimum {:?}",
        pgg.equilibrium_contribution(),
        pgg.social_optimum_contribution()
    );
}
```

```text
n = 3: q* = 22.500000, found 22.500000, Converged, damping 0.5, residual 0.0e0
n = 10: q* = 8.181818, found 8.181818, Converged, damping 0.25, residual 8.4e-11
undamped step 1: [25.0, 25.0, 25.0]
undamped step 2: [20.0, 20.0, 20.0]
undamped step 3: [25.0, 25.0, 25.0]
undamped step 4: [20.0, 20.0, 20.0]
Bertrand p* = Some(4.666666666666667)
public goods: equilibrium Some(0.0), social optimum Some(20.0)
```

10 社では $\alpha = 0.5$ が $4/11$ を超えるので最初の試行が失敗し，$\alpha = 0.25$ で収束します．

解析的な最適応答を持たない利用者定義のゲームは，黄金分割の探索と，緩めた既定の許容誤差で解かれます．

```rust
use socsim_games::{
    best_response_iteration, check_continuous_action, ActionError, BrIterationOptions,
    ContinuousGame, NormalFormGame, NumericBr, StageGame,
};

/// Two players, x_i in [0, 10], u_i = x_i (10 - x_i - x_j): no analytic best response.
struct Duopoly {
    bounds: [(f64, f64); 1],
}

impl StageGame for Duopoly {
    type Action = Vec<f64>;
    fn n_roles(&self) -> usize { 2 }
    fn check_action(&self, role: usize, action: &Vec<f64>) -> Result<(), ActionError> {
        check_continuous_action(self, role, action)
    }
}

impl NormalFormGame for Duopoly {
    fn payoff(&self, role: usize, profile: &[Vec<f64>]) -> f64 {
        let (own, other) = (profile[role][0], profile[1 - role][0]);
        own * (10.0 - own - other)
    }
}

impl ContinuousGame for Duopoly {
    fn bounds(&self, _role: usize) -> &[(f64, f64)] { &self.bounds }
    fn numeric_br(&self) -> NumericBr { NumericBr::Unimodal1D } // concave in the own action
}

fn main() {
    let game = Duopoly { bounds: [(0.0, 10.0)] };
    let run = best_response_iteration(&game, &[vec![0.0], vec![0.0]], &BrIterationOptions::new())
        .unwrap();
    println!("{:?} x = {:.7} residual {:.1e}", run.status, run.profile[0][0], run.residual);
    let strict = BrIterationOptions::new().tol(1e-12).adaptive(false);
    let run = best_response_iteration(&game, &[vec![0.0], vec![0.0]], &strict).unwrap();
    println!("tol 1e-12: {:?} residual {:.1e}", run.status, run.residual);
}
```

```text
Converged x = 3.3333331 residual 2.9e-7
tol 1e-12: MaxIter residual 2.0e-8
```

均衡は $x^* = 10/3$ です．数値の最適応答は $\sqrt{\varepsilon}\,\lvert x \rvert$ 程度の残差を残し，既定の許容誤差（`1e-7` × 区間の幅 10）はこれを受け入れ，`1e-12` は受け入れません．

### 決定規則：`DampedBestResponse` と `GradientAscent`

| 規則（`name()`） | 構築子 | 役割 $r$ のエージェント $i$ の更新 | 状態のスロット |
|---|---|---|---|
| `DampedBestResponse<G>`（`damped_best_response`） | `DampedBestResponse::<G>::new(alpha)?`（$\alpha$ は有限で $(0, 1]$ の中），`.with_oracle(Arc<dyn BestResponseOracle<G>>)` | $x_i \leftarrow \mathrm{Proj}\big((1 - \alpha)\,x_i + \alpha\,\mathrm{BR}_r(x_{-i})\big)$ | `ContinuousState`（`Continuous`） |
| `GradientAscent`（`gradient`） | `GradientAscent::new(step)?`（有限で `> 0`） | $x_i \leftarrow \mathrm{Proj}\big(x_i + \texttt{step} \cdot \nabla_{x_i} u_r(x_i, x_{-i})\big)$ | `ContinuousState`（`Continuous`） |

どちらも `G: ContinuousGame`（有限ゲームではコンパイルできない）と `X::PolicyState: Slot<ContinuousState>` を要求します．
`BuiltinPolicyState` は変種 `Continuous(ContinuousState { last })` を持ちます．
`alpha()` と `step()` はパラメータを返し，不正な値は構築子が `RuleError::Unsupported` で拒否します．
どちらの規則も乱数を引きません．

**同時手番．** どちらの規則も **前のステップ** の行動に応答します．
ほかの役割の行動は，その席のエージェントが最後に出した行動で，ステップの履歴（`opponent_history(j)`）から読みます．
ステップを休んだ相手は，履歴の窓（`history_len`）の中で最後に出した行動で表します．
規則は相手の保持行動を読みません．`game_decide` は各エージェントの決定をその場で `held` に書くので，Decision の間に保持行動を読むと，そのステップの行動と前のステップの行動が混ざります．
それは逐次手番（Gauss–Seidel 型）の更新で，結果がエージェントの処理順に依存します．
Theocharis の不安定性は同時手番（Jacobi 型）の反復の性質であり，規則が再現するのはこちらです．
$\alpha$ の `DampedBestResponse` は，`best_response_step(&game, &x, alpha, None)` とちょうど同じ更新をします．
$x_i$ はそのエージェントが最後に出した行動（`ContinuousState::last`）で，まだ出していなければ保持行動です．エージェントが休んだステップでは `last` は変わりません．

**最初のステップ．** ほかの役割の行動がまだ分からない間（最初のステップのように，履歴の窓に出された行動が無いとき）は，自分の保持行動（`DecisionContext::my_held()`）をそのまま出します．
したがってステップ 1 は初期の行動の組 $x_0$ を出し，ステップ $k$ はソルバーの写像の反復値 $x_{k-1}$ を出します．
下の例では企業は `[0, 50, 100]` から始め，ステップ 1 はそのまま，ステップ 2 は `[0, 25, 60]` を出します．

**席．** ほかの役割はエージェントの対戦から取るので，Matching は役割ごとに 1 人を座らせなければなりません．2 役割なら `FixedPlayers`，エージェントごとに 1 役割なら `WholePopulation`，`k` 役割なら `RandomGroups::new(k)` です．
対戦の無い粒度（`PerAgentRole`）では相手を順に使い，相手の数が役割の数と合わなければ `RuleError::Unsupported` でステップが止まります．

**検査．** `init_state`（したがって `build_world`）は不正な区間を `RuleError::Unsupported` で拒否します．
oracle が無ければ，`DampedBestResponse` はさらに，すべての役割が区間の中点の組で解析的な最適応答を持つか，`NumericBr::Unimodal1D` の 1 次元の行動であることを要求します．
実行時には，最適応答が得られないか `Converged` でなければ `RuleError::BestResponse(_)`，解析勾配の長さが違えば `RuleError::Unsupported`，勾配の成分が NaN か無限大なら `RuleError::NonFiniteGradient { dim }` でステップが止まります．

**数値微分．** `gradient` が `None` を返すと，`GradientAscent` は各成分 $d$ で刻み $h = 10^{-6} \max(1, \lvert x_d \rvert)$ の中心差分を使います．
区間の端の近くでは区間の中に収まる片側差分に切り替え（$x_d - h < lo$ なら前進差分，$x_d + h > hi$ なら後退差分），幅が $2h$ より狭い成分では区間の幅の半分を刻みにします．
利得を区間の外で評価することはありません．
中心差分の誤差は $O(h^2)$ ですが，片側差分の誤差は $\tfrac{1}{2} \lvert u'' \rvert h$ のオーダーで，$h$ が $\lvert x \rvert$ に比例するので誤差も $\lvert x \rvert$ に比例して増えます．
クールノー（$u'' = -2b$）では誤差は $b\,h$ で，$b = 1$，$q = 50$ ではおよそ $5 \times 10^{-5}$ です．
反復値が区間の端にあり精度が要るときは `gradient` を実装してください．

線形クールノーの内点では，同時手番の勾配上昇は $\texttt{step} < 2/((n + 1)\,b)$ のときに限って収束します．3 社，$b = 1$ なら，`0.2` は 22.5 に収束し，`0.6` は収束しません．

**例：`cournot`．** `examples/cournot.rs` は，3 社のクールノー（$a = 100$，$b = 1$，$c = 10$，$q \in [0, 100]$．したがって $q^* = 22.5$）を，`[0, 50, 100]` から $\alpha = 0.5$ の `DampedBestResponse` で学習させます．
Matching は `WholePopulation` で，`ContinuousMetrics` が，どの生産量も 10 ステップ続けて `1e-9` 以上変わらなくなった時点で実行を止めます．

```rust
let game = Cournot::new(3, 100.0, 1.0, 10.0, 100.0)?;
let metrics = ContinuousMetrics::new(&game, ContinuousMetricsOptions::new().stop(1e-9, 10))?;

let mut rules = RuleBook::new();
let rule: Arc<dyn DecisionRule<Cournot, ()>> = Arc::new(DampedBestResponse::<Cournot>::new(0.5)?);
rules.register("damped_best_response", rule)?;

let world = build_world(
    game, 3, (),
    InitialActions::PerAgent(vec![vec![0.0], vec![50.0], vec![100.0]]),
    &rules,
    &Assignment::Ordered(vec![AssignGroup {
        kind: AssignKind::Rule("damped_best_response".to_owned()),
        count: 3,
    }]),
    1, 500, seed,
)?;
let mut simulation = assemble(world, WholePopulation::new(), rules, NormalFormOutcome, PlayOptions::new(), seed)?
    .add_mechanism(Box::new(metrics))
    .build();
simulation.run()?;
```

```sh
cargo run -p socsim-games --features core,engine --example cournot
```

```text
equilibrium quantity: 22.5000000000
stopped after 92 of 500 steps
last metrics row: t = 92, mean quantity 22.5000000000
firm 0: 22.4999999998
firm 1: 22.5000000000
firm 2: 22.5000000002
max |q_i - q*|: 1.803e-10
```

例は，実行が `t_max` より前に止まること，`game_metrics` が毎ステップ `game.mean_action.0` を 1 つ書いたこと，どの企業も $q^*$ から `1e-9` 以内で終わることを assert で確かめます．
ステップ 1 は初期の行動の組を繰り返すので，ステップ 92 での停止は `best_response_step` の 91 回目の反復値に当たります．

### 指標：`ContinuousMetrics`

`GameMetrics` は `G: FiniteGame` を要求します．連続ゲームには `ContinuousMetrics<G: ContinuousGame>`（feature `core`）を使います．
Mechanism の名前（`game_metrics`），フェーズ（PostStep），表（`"game"`）は同じで，ビルダーへの足し方も同じです．
別の Mechanism ではなく，`game_metrics` の連続版です．

```rust
let metrics = ContinuousMetrics::new(           // before `game` moves into build_world
    &game,
    ContinuousMetricsOptions::new()
        .equilibrium(ContinuousEquilibrium::Solve)
        .stop(1e-9, 10),
)?;
```

| `ContinuousMetricsOptions` のメソッド | 既定値 | 効果 |
|---|---|---|
| `new()` / `default()` | — | 均衡までの距離なし，停止なし |
| `equilibrium(ContinuousEquilibrium)` | `None` | `Solve`：`new` の中で `best_response_iteration` を 1 回，区間の中点から既定のオプションで走らせ，`Converged` でなければ失敗する．`Given(profile)`：この行動の組（役割ごとに長さの合った有限の行動）を使う．どちらも `game.eq_distance` を足す |
| `stop(tol, window)` | なし | どのエージェントの行動も `window` ステップ続けて `tol` 以上変わらなければ実行を終える．`tol` は有限で `> 0`，`window ≥ 1` |
| `payoff_timing(PayoffTiming)` | `BeforeUpdate` | `MetricsOptions` と同じ．`AfterUpdate` は `describe()` が返す `mean_action`，`action_sd`，`mean_payoff`，`payoff_sd` の意味だけを変える |

`ContinuousMetrics::new` は，区間かオプションが不正なとき，与えた行動の組の形が合わないとき，解が収束しないときに `GameError::Metrics` を返します．
`Solve` が見つける均衡は 1 つです．均衡が複数あるゲームでは，意図した均衡を `Given` で渡してください．
`equilibrium()` は使っている行動の組を，`describe()` は設定した列の `(pattern, meaning)` の組を返します．

| 列 | 現れる条件 | 値 |
|---|---|---|
| `game.mean_action.<d>` | すべての役割の成分の数が等しく，ステップに席がある | ステップで出されたすべての席にわたる成分 `d` の平均（役割をまとめる） |
| `game.action_sd.<d>` | `mean_action.<d>` と同じ | 同じ値の母標準偏差 |
| `game.mean_action.r<r>-<d>`，`game.action_sd.r<r>-<d>` | 役割によって成分の数が違い，役割 `r` に席がある | 同じものを役割 `r` の席にわたって |
| `game.mean_payoff`，`game.payoff_sd` | 少なくとも 1 人が対戦した | `GameMetrics` と同じ |
| `game.aggregate.<name>` | 集計値つきで組み立てた | `GameMetrics` と同じ |
| `game.eq_distance` | 均衡を設定し，すべての役割に席がある | $\sum_r \sum_d \lvert \bar x_{r,d} - x^*_{r,d} \rvert$．ステップでの各役割の平均行動と均衡との L1 距離 |

列の順は `mean_action.*`，`action_sd.*`，`mean_payoff`，`payoff_sd`，`aggregate.*`，`eq_distance` です．
有限ゲームと同じく，値の定まらない列は行に含めず，有限でない値はステップを止めます．
`GameMetrics` の割合，`coop_rate`，`regret`，`action_entropy`，`population_share` に当たる連続の列はありません．

**役割ごとの列は今は記録されません．** 役割によって成分の数が違う連続ゲームは World に組めないからです．`build_world` はすべての初期行動を，組み立てはすべての fallback の行動を，すべての役割について検査します．そのため役割ごとの列は `describe()` にだけ現れます．これらの検査を通らずに作った World（たとえば `GameWorld` を逆直列化したものや，`Simulation::restore` で読み込んだもの）には対応していません．

**収束による停止はエージェントごとです．** `game_metrics` は毎ステップ，各エージェントがそのステップで出した行動を `AgentId` 順につないだベクトルを作ります．対戦しなかったエージェントは前の値を保ちます（最初のステップでは保持行動）．
前のステップからどの成分も `tol` 以上変わっていなければカウンタを 1 増やし，そうでなければ 0 に戻します．`window` に達したら，その行を記録してから停止を要求します．
割合で判定する有限ゲームの停止と違い，平均行動が落ち着いていても個々のエージェントが動いている集団は止まり**ません**．
最も早い停止はステップ `window + 1` で，カウンタと前のベクトルは World（`MetricsState`）に置かれるので，再開した実行も同じステップで止まります．

列は [暗黙の評価経路](#対戦を実体化しない評価)（全員が Held の `WholePopulation`，`GridNeighborhood`，`NetworkEdges`）でも同じです．
評価のステップがたどったパターンで席をたどるので，値は `materialize(true)` のときとビット単位で一致します．

### 連続の行動での集団動学

`imitate_best`，`fermi_update`，`moran_update` はどんな型の行動でも写せるので，その設定の型は有限ゲームのほか `ContinuousGame + CommonActionSpace` も受け付けます．
`replicator_update`（`ProportionalImitation`）は有限ゲームだけです（連続ゲームではコンパイルできません）．上限 $\beta \le 1/\Delta\pi$ に，全行動の組にわたる利得の範囲が要るからです．

| 動学 | 連続の行動では |
|---|---|
| `imitate_best` | 最も稼いだエージェントの行動ベクトルを写す．有限の規則が «最小の行動番号» で同点を解くところを，連続の規則は **辞書式に最小の行動ベクトル**（成分は `f64::total_cmp` で比べ，長さが違えば短い方が先）で解く |
| `fermi_update` | 相手の行動ベクトルをそのまま写す |
| `moran_update` | `Fitness::Linear` は拒否される（正値性の検査に全行動の組にわたる利得の範囲が要る）．`Fitness::Exponential` を使う．突然変異体は，親の行動の各成分に独立なガウス雑音を加え，区間へ切り詰めたもの |

連続の行動の `imitate_best` と `fermi_update` も非同期に更新できます（[非同期更新](#非同期更新) を参照）．

ガウスの突然変異はモラン過程に設定します．

```rust
let moran = MoranUpdate::new(Fitness::Exponential { w: 0.05 }, 0.1, Update::Synchronous)?
    .gaussian_mutation(2.0)?; // mutant = parent + N(0, 2^2) per component, clamped to the bounds
```

`gaussian_mutation(sd)` は有限で `sd > 0` を要求し（そうでなければ `GameError::Population`），`gaussian_sd()` がその値を返します．
組み立ての時に，`mutation > 0` で `sd` の無い連続ゲームと，`sd` を設定した有限ゲームは `GameError::Population` で拒否されます．`mutation = 0` の連続ゲームには `sd` は要りません．
連続ゲームの共通の行動空間とは，すべての役割の成分の数が等しく，区間も同一であることです．そうでなければ組み立ては `GameError::Population` で失敗します．
線形の適応度で連続ゲームのモラン過程を組むと，次のエラーになります．

```text
invalid population dynamics: linear fitness requires a payoff range, which cannot be enumerated for a continuous game; use Fitness::Exponential
```

1 ステップで引く乱数の個数は固定です．連続ゲームの `moran_update` は，親（`f64`），死ぬエージェント（`gen_range`），突然変異の判定（`f64`）を引き，続いて Box–Muller 変換のために成分ごとに一様な `f64` を 2 つ引きます．突然変異が起きないときや `sd` が無いときも必ず引きます．

### 連続ゲームでの LLM による決定

`llm` feature を有効にすると，LLM の企業が規則で動く企業と対戦できます．

| 部品 | 連続の形 |
|---|---|
| 観測 | `OwnHistoryObs::new(window)` — `OwnHistory<Vec<f64>>` |
| レンダラ | `DescribedRenderer::new(field)`（`G: ContinuousGame + DescribeGame`）．システムプロンプトは，見出し（既定は `Game rules`．`.with_heading(..)` で変える），`You are role r.`，ゲームの `describe(role)`，許される区間，返答の形式 `{"<field>": <number>, "reason": "<reason>"}` からなる．各ターンには自分の行動と利得，相手の最後の行動を並べる |
| パーサ | `NumberParser::new(field)` はすべての `ContinuousGame` について `ActionParser<G>` である |

`NumberParser` は 1 つの数を読み，1 次元の行動にします．
`.bounds(lo, hi)` を指定しなければ値を役割のゲームの区間で検査し，指定すれば自分の範囲で検査します．自分の範囲はゲームの区間の中になければなりません（規則の状態の初期化のときに走る `validate` が，そうでなければ `RuleError::Unsupported` を返します）．
`OutOfRange::Clamp` は違反を報告する代わりに切り詰めます．
2 成分以上の行動は `validate` が拒否し，ゲームに無い役割への返答は `ViolationKind::Range` の違反です．
`Fallback::UniformRandom` は同じ区間から一様に引きます．
レンダラとパーサには同じフィールド名を渡してください．
2 成分以上の行動についてのレンダラの文言は仮のものです．そうした返答を読む組込みのパーサはまだありません．

`DescribeGame`（`describe(role) -> String`）は crate の根（既定のビルド）で定義されているので，ゲームは `llm` feature 無しで実装できます．`socsim_games::llm::DescribeGame` はその再エクスポートです．
`assemble_with_llm` は Matching が対応する役割の数ならいくつでも受け付けるので，`WholePopulation` の下の 3 社も組めます．

```rust
let mut rules: RuleBook<Cournot, WithLlm<()>> = RuleBook::new();
let damped: Arc<dyn DecisionRule<Cournot, WithLlm<()>>> =
    Arc::new(DampedBestResponse::<Cournot>::new(0.5)?);
rules.register("damped", damped)?;
let mut llm_rules: LlmRuleBook<Cournot, WithLlm<()>> = LlmRuleBook::new();
llm_rules.register(
    "llm",
    Arc::new(LlmPolicy::new(
        OwnHistoryObs::new(4),
        DescribedRenderer::new("quantity"), // the reply field ...
        NumberParser::new("quantity"),      // ... is the field the parser reads
    )),
)?;
// Firm 0 is the LLM firm, firms 1 and 2 play the damped best response;
// build the world with build_world_with and a KindCatalog of both books, then
// assemble_with_llm(world, WholePopulation::new(), rules, setup, NormalFormOutcome, opts, seed).
```

企業 0 のシステムプロンプトは次のようになります．

```text
# Game rules

You are role 0.

Cournot competition among 3 firms. Each firm chooses a quantity in [0, 100]. The market price is 100 - 1 * Q, where Q is the total quantity of all firms; the price can become negative. A firm producing quantity q earns q * (price - 10). You are firm 0.

Your action is a single number between 0 and 100 (inclusive).

Reply with JSON: {"quantity": <number>, "reason": "<reason>"}.
```

常に 20 と答えるモデルなら，企業 1 と 2 は $(90 - 20)/3 \approx 23.33$ に落ち着きます．

### 連続ゲームの snapshot

連続の行動は World に残り続けます（`held`，ステップの履歴，`ContinuousState::last`，`game_metrics` の収束判定のベクトル）．
そのため JSON を経由する snapshot は，どの `f64` もビット単位で元に戻さなければなりません．
`serde_json` の既定はそれを保証しません．おおよそ 11 個に 1 個の値が最下位の桁で 1 だけずれて戻り，再開した実行は通しの実行から離れていきます．
そこで `socsim-games` は `serde_json` の feature `float_roundtrip` を有効にしています．
Cargo は feature を統合するので，`socsim-games` を `core` つきで使うビルドでは，どの crate も浮動小数を正確に読みます．
snapshot を別の直列化で保存する場合も，この性質を保ってください．

### 現時点の制限

- 数値の最適応答は，`NumericBr::Unimodal1D` を宣言した 1 次元の行動だけを扱います．多次元のゲームには解析的な `best_response` か `BestResponseOracle` が要ります．
- 役割によって成分の数が違うゲームはソルバーでは扱えますが，まだ World に組めません．`build_world` がすべての初期行動を，組み立てがすべての fallback の行動を，すべての役割について検査するからです．そのため `ContinuousMetrics` の役割ごとの列 `game.mean_action.r<r>-<d>`，`game.action_sd.r<r>-<d>` は今は記録されません．これらの検査を通らずに作った World（たとえば `GameWorld` を逆直列化したものや，`Simulation::restore` で読み込んだもの）には対応していません．
- `NumberParser` は 1 次元の行動だけを扱います．
- `replicator_update` は有限ゲームだけで，`moran_update` は連続ゲームでは `Fitness::Linear` を拒否します．

---

## `GameSimFactory` による組み立て

`GameSimFactory`（feature `factory`）は，`build_world_with` と `assemble_with` を再利用できる 1 つの構成にまとめます．
型がシミュレーションの型を変える部品を記録するので，Matching を設定し忘れるとコンパイルエラーになり，組み立て関数が検出する不整合はすべて `build` が `Result` で報告します．
factory が持つのは構成であってシミュレーションではありません．`build`，`into_builder`，`build_trial` は `&self` を取り，同じ構成と seed からは常に同じ World，同じ Mechanism，同じエンジンの seed ができます．

```rust
use socsim_core::WorldState;
use socsim_games::catalog::PrisonersDilemma;
use socsim_games::{
    AssignGroup, AssignKind, Assignment, FictitiousPlay, FixedPlayers, GameSimFactory,
    MetricsOptions, TitForTat,
};
use socsim_log::InMemoryRecorder;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let factory = GameSimFactory::new(PrisonersDilemma::new(5.0, 3.0, 1.0, 0.0)?) // T, R, P, S
        .agents(2)
        .matching(FixedPlayers::pairs(&[(0, 1)])) // the type state now has a matching
        .rule("tit_for_tat", TitForTat::new(0)) // cooperate = action 0 ("C")
        .rule("fictitious_play", FictitiousPlay::new())
        .assign(Assignment::Ordered(vec![
            AssignGroup { kind: AssignKind::Rule("tit_for_tat".into()), count: 1 },
            AssignGroup { kind: AssignKind::Rule("fictitious_play".into()), count: 1 },
        ]))
        .history_len(200)
        .t_max(200)
        .metrics(MetricsOptions::new().cooperate_label("C"));

    // `build` takes `&self`: one factory, any number of simulations.
    for seed in [1, 2] {
        let mut sim = factory.build(seed)?;
        sim.run()?;
        println!("seed {seed}: t = {}, held = {:?}", sim.world().clock().t(), sim.world().held());
    }

    // A recorder is attached to the builder, not to the factory.
    let mut sim = factory
        .into_builder(1)?
        .recorder(Box::new(InMemoryRecorder::new()))
        .build();
    sim.run()?;
    let recorder = sim
        .recorder()
        .as_any()
        .and_then(|r| r.downcast_ref::<InMemoryRecorder>())
        .unwrap();
    let coop: Vec<f64> = recorder
        .metrics()
        .iter()
        .filter(|row| row.key == "game.coop_rate")
        .map(|row| row.value)
        .collect();
    println!("coop_rate: first {:?}, last {:?}, rows {}", coop.first(), coop.last(), coop.len());
    Ok(())
}
```

```text
seed 1: t = 200, held = [1, 1]
seed 2: t = 200, held = [1, 1]
coop_rate: first Some(1.0), last Some(0.0), rows 200
```

### 型状態

`GameSimFactory<G, X = (), M = NoMatching, P = Pairwise<NormalFormOutcome>, D = NoDynamics>` は，ゲーム `G`，実験状態 `X`，Matching `M`，対戦の評価経路 `P`，集団動学 `D` を記録します．
`GameSimFactory::new(game)` は `X = ()` から始まり，`GameSimFactory::with_state(game, x)` は利用者の実験状態から始まります．factory から作るシミュレーションは，どれも `x` の複製から始まります．
型を変えるメソッドは 6 つです．

| メソッド | 置き換えるもの | 要求 |
|---|---|---|
| `.matching(m)` | `M`（`NoMatching` から） | `m: Matching<G, X> + Clone + 'static` |
| `.outcome(o)` | `P` を `Pairwise<O>` に | `o: OutcomeRule<G, X> + Clone + 'static` |
| `.counting_play()` | `P` を `Counting` に | `G: AnonymousGame` |
| `.aggregates(len)` | `P` を `WithAggregates<P>` に | `G: AggregateGame`．呼んだ後は `outcome` と `counting_play` を呼べなくなるので，集計値を誤って落とすことはない |
| `.dynamics(d)` | `D` | `d: PopulationDynamics<G, X> + Clone`（`ProportionalImitation` は有限ゲームだけ） |
| `.async_dynamics(step)` | `D` を `AsyncPopulationStep<D2>` に | `step` は `Update::Asynchronous` で作った `ImitateBest`，`FermiUpdate`，`ProportionalImitation` を包む（[非同期更新](#非同期更新) を参照） |

`build`，`into_builder`，`build_trial` は `M` が `Matching` を実装したときにだけ存在するので，`.matching(..)` を忘れるとコンパイルできません．
また，どの組み立てもゲームと実験状態の複製から始まるので，`G: Clone` と `X: Clone` も要求します．
ほかの制約もコンパイラーが確かめます．`.rule(name, r)` は `r: DecisionRule<G, X>` を要求し（連続ゲームで仮想プレイを使うとコンパイルできない），`.metrics(..)` は `G: FiniteGame`，`.continuous_metrics(..)` は `G: ContinuousGame` を要求します．

`.dynamics(..)` と `.async_dynamics(..)` は互いに置き換えます．後から呼んだ方が factory の組み立て方を決めます．
`.async_dynamics(step)` の後の `build`，`into_builder`，`build_trial` は `assemble_async` の検査を行い，`game_match`，`game_decide`，動学，`game_play` の代わりに `async_population_step` だけを登録します．
登録した規則は使わず，全エージェントが Held でなければなりません．
factory は指標の利得の時点も `PayoffTiming::AfterUpdate` にします．`.metrics(..)` や `.continuous_metrics(..)` のオプションで時点を指定していても上書きし，同期の経路では `BeforeUpdate` にします．
Scenario から非同期更新を選ぶことはできません．

```rust
use socsim_core::WorldState;
use socsim_games::catalog::PrisonersDilemma;
use socsim_games::{
    AsyncActivation, AsyncPopulationStep, GameSimFactory, GridBoundary, GridNeighborhood,
    GridNeighbors, GridSpec, ImitateBest, InitialActions, MetricsOptions, Topology, Update,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let side = 99;
    let mut initial = vec![0; side * side]; // action 0 = C
    initial[49 * side + 49] = 1; // one D in the centre
    let factory = GameSimFactory::new(PrisonersDilemma::weak(1.85)?)
        .agents(side * side) // no rule registered: every agent is held
        .matching(GridNeighborhood::new().include_self(true))
        .topology(Topology::grid(GridSpec {
            rows: side,
            cols: side,
            boundary: GridBoundary::Fixed,
            neighborhood: GridNeighbors::Moore,
        })?)
        .initial_held(InitialActions::PerAgent(initial))
        .history_len(1)
        .t_max(217)
        .async_dynamics(
            AsyncPopulationStep::new(ImitateBest::new(Update::Asynchronous))
                .activation(AsyncActivation::UniformWithReplacement),
        )
        .metrics(MetricsOptions::new().population_share(true)); // AfterUpdate is set by the factory

    let mut sim = factory.build(1)?;
    sim.run()?;
    let cooperators = sim.world().held().iter().filter(|&&a| a == 0).count();
    println!("t = {}, cooperators = {cooperators}", sim.world().clock().t());
    Ok(())
}
```

```text
t = 217, cooperators = 0
```

### 設定と既定値

| 設定 | 既定値 |
|---|---|
| `agents(n)` | 必須 |
| `matching(m)` | 必須（型状態） |
| `history_len(n)` | 必須．正の値 |
| `t_max(n)` | 必須 |
| `rule(name, r)` | なし．各規則は `Arc` に包まれ，factory から作るすべてのシミュレーションで共有される（規則の状態は World にある）．同じ名前を 2 回登録すると `build` が `RuleError::DuplicateKind` で失敗する |
| `assign(Assignment)` | 規則が 0 個なら全員 Held，1 個なら全員その規則，2 個以上なら必須 |
| `initial_held(InitialActions)` | 行動が添字のとき（有限ゲームはすべて）`InitialActions::Same(0)`．それ以外（連続ゲーム）は必須 |
| `topology(Topology)` | なし．エージェント `i` がノードまたはセル `i` |
| `play_options(PlayOptions)` | `PlayOptions::new()` |
| `outcome(o)` か `counting_play()`，続けて任意で `aggregates(len)` | 集計値なしの `Pairwise<NormalFormOutcome>` |
| `dynamics(d)` か `async_dynamics(step)` | `NoDynamics`．どちらも後の呼び出しが前の呼び出しを置き換える |
| `metrics(MetricsOptions)`（有限）/ `continuous_metrics(ContinuousMetricsOptions)`（連続） | 指標なし．後の呼び出しが前の呼び出しを置き換える．指標の Mechanism をさらに足すときは `into_builder` を通す．利得の時点は factory が決める（`async_dynamics` の後は `AfterUpdate`，それ以外は `BeforeUpdate`） |
| `scheduler(SchedulerChoice)` | `SchedulerChoice::Sequential`（もう一つの選択肢は `RandomActivation`） |

必須の値が無いと，`build` は `GameError::Config` を返します．文言は設定の名前を含みます（例：`invalid scenario: history_len: the history length is required`）．
ネットワークのトポロジーは，`.topology(..)` に渡す前に利用者が生成します．ストリームには，Scenario が使うのと同じ `derive_seed(seed, &[0, 1])` を使ってください．

### `build`，`into_builder`，`build_trial`

| メソッド | 返すもの | 用途 |
|---|---|---|
| `build(seed)` | エンジンの null recorder つきの `Simulation<GameWorld<G, X>>` | そのまま走らせる |
| `into_builder(seed)` | すべての Mechanism，スケジューラー，エンジンの seed を設定済みの `SimulationBuilder<GameWorld<G, X>>` | recorder，Environment や Reward の Mechanism，2 つ目の指標の Mechanism を足してから `.build()` を呼ぶ |
| `build_trial(seed)` | `socsim_runner::Trial<GameWorld<G, X>>` | 利用者の `TrialFactory` から返す |

3 つとも同じ手順を通ります．
World を `build_world_with` でゲーム・登録した規則・割当て・初期行動から作り，トポロジーを `GameWorld::with_topology` で付け，
Matching・規則・対戦の評価経路・動学を `assemble_with` と同じコードで検査して Mechanism `game_match`，`game_decide`，動学（あれば），`game_play` にし，指定があれば `game_metrics` を足します．
エンジンの seed は `derive_seed(seed, &[1])` です．
メソッドは `&self` を取るので，多数の seed を回すときは seed ごとに `build(seed)` を呼べばよく，複数のスレッドから呼んでもかまいません．

1 つの recorder を同じ factory から作る複数のシミュレーションで共有することはできないので，factory は recorder を持ちません．
`factory.into_builder(seed)?.recorder(Box::new(recorder)).build()` のように builder に付けてください．

`build_trial` を使うと，`socsim_runner` 用に書く factory から構成全体を再利用できます．
試行を `run_trial`，`run_trials`，`run_trial_sweep` で走らせるときは World の時計が `scenario.simulation.t_max` に置き直されるので，factory 自身の `t_max` はそこでは使われません．

```rust
use socsim_config::Scenario;
use socsim_core::SocsimError;
use socsim_games::catalog::PrisonersDilemma;
use socsim_games::{FixedPlayers, GameSimFactory, GameWorld, TitForTat};
use socsim_runner::{Trial, TrialFactory};

/// Two tit-for-tat players; one rule, so every agent follows it.
struct TftPairs {
    factory: GameSimFactory<PrisonersDilemma, (), FixedPlayers>,
}

impl TrialFactory<GameWorld<PrisonersDilemma>> for TftPairs {
    fn build_trial(
        &self,
        _scenario: &Scenario,
        seed: u64,
    ) -> socsim_core::Result<Trial<GameWorld<PrisonersDilemma>>> {
        self.factory
            .build_trial(seed)
            .map_err(|error| SocsimError::Config(error.to_string()))
    }
}

fn tft_pairs() -> Result<TftPairs, socsim_games::GameError> {
    Ok(TftPairs {
        factory: GameSimFactory::new(PrisonersDilemma::new(5.0, 3.0, 1.0, 0.0)?)
            .agents(2)
            .matching(FixedPlayers::pairs(&[(0, 1)]))
            .rule("tit_for_tat", TitForTat::new(0))
            .history_len(10)
            .t_max(10),
    })
}
```

ゲーム・集団・Mechanism も Scenario から読むときは `GameTrialFactory` を使います（[Scenario ファイル](#scenario-ファイル) を参照）．

### 実行時の検査

`build`（と `into_builder`，`build_trial`）は，組み立て関数が検出する不整合をすべて `Err` で返します．

| 検査 | エラー |
|---|---|
| 必須の設定が無い．規則を 2 つ以上登録したのに `assign` が無い | `GameError::Config` |
| 集団動学があるのに，決定規則に従うエージェントがいる | `GameError::Population` |
| `async_dynamics` の後で，`assemble_async` が拒否する構成（[非同期更新](#非同期更新) を参照） | `assemble_async` のエラー |
| 役割によって行動の数が違う（連続ゲームでは区間が違う）ゲームに集団動学を付けた | `GameError::Population` |
| `replicator_update` の $\beta$ が $1/\Delta\pi_{\max}$ を超える．`moran_update` の `Fitness::Linear` で適応度が正でない | `GameError::Population` |
| 1 ステップに複数の役割を持つエージェントがいるのにステップごとの決定にした（例：3 人以上の `RoundRobin` を `PerStep` で） | Matching が組み立て時に判断できれば `GameError::InvalidMatching`．できなければそのステップが `RuleError::AmbiguousRole` で失敗する |
| 規則がゲームに対応していない（役割が 2 つでないゲームでの仮想プレイやロジット選択，正でない利得のある Roth–Erev，行動が 2 つでないゲームでのオートマトン） | `GameError::Rule(RuleError::Unsupported(..))` |
| 同じ規則の名前を 2 回登録した．割当てが登録していない規則を指す | `GameError::Rule(RuleError::DuplicateKind(..))` / `GameError::Rule(RuleError::UnknownKind(..))` |
| 組込みのゲームが構築時にパラメータを拒否した | `CatalogError`．`?` で `GameError::Catalog` になる |

例：

```text
invalid population dynamics: imitate_best applies only to held agents, but an active rule is assigned
invalid population dynamics: replicator_update requires beta <= 1 / 3 (the per-step payoff range), got 1
rule cannot be used with this game: learning rule requires 2 roles, but the game has 3
```

Mechanism の順は factory が決めるので，ライブラリモードで集団動学が `game_decide` より前に走ることはありません．
Scenario でその順に書くと拒否されます（[`[[mechanism]]` の項目](#mechanism-の項目) を参照）．

---

## Scenario ファイル

feature `config` の `GameTrialFactory` は，シミュレーション全体 — ゲーム，集団，トポロジー，Matching，規則，対戦のオプション，集団動学，指標 — を `socsim_config::Scenario` から組み立てます．
`socsim_runner::TrialFactory` を実装するので，`run_trial`，`run_trials`，`run_trial_sweep` でゲームの Scenario をそのまま走らせられます（[シナリオ全体から試行を組み立てる](library.ja.md#シナリオ全体から試行を組み立てるtrialfactorysocsim-runner) を参照）．
`GameTrialFactory::build_trial` は Scenario を `GameSimFactory` の構成に読み込み，その `build_trial` を呼びます．
したがって 2 つの入口は同じ手順を通り，Scenario と，同じ構成を `GameSimFactory` の設定メソッドで書いたものは，各ステップで同じ World と同じ指標の行を作ります．

```toml
[simulation]
name        = "ipd_tft_vs_fp"
module_pack = "library"      # not read by GameTrialFactory; "normal-form" to run it with the socsim CLI
t_max       = 200
seed        = 42
scheduler   = "sequential"

[world]
n_agents    = 2
history_len = 200

[world.game]
kind       = "prisoners_dilemma"
temptation = 5.0
reward     = 3.0
punishment = 1.0
sucker     = 0.0

[world.population]
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
coop_label   = "C"            # read by tit_for_tat
fp_tie_break = "lowest_index" # read by fictitious_play

[[mechanism]]
name = "game_play"

[[mechanism]]
name = "game_metrics"
[mechanism.params]
cooperate_label = "C"

[output]
log_path = "runs/{name}_{seed}.jsonl"   # not read by GameTrialFactory
```

```rust
use socsim_config::Scenario;
use socsim_games::{FiniteSpec, GameTrialFactory, RuleRegistry};
use socsim_runner::{run_trial, run_trials, TrialFactory};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let scenario = Scenario::from_path(std::path::Path::new("ipd.toml"))?;
    let factory = GameTrialFactory::<FiniteSpec>::new(RuleRegistry::finite_builtins());

    factory.validate(&scenario)?; // run_trial validates as well
    let result = run_trial(&scenario, &factory, scenario.simulation.seed)?;
    println!("coop_rate at t = 1: {:?}", result.series["game.coop_rate"].first());
    println!("final coop_rate: {}", result.final_metrics["game.coop_rate"]);

    let many = run_trials(&scenario, &factory, &[1, 2, 3], true)?;
    println!("{:?}", many.iter().map(|r| r.seed).collect::<Vec<_>>());
    Ok(())
}
```

```text
coop_rate at t = 1: Some((1, 1.0))
final coop_rate: 0
[1, 2, 3]
```

`G` は，有限ゲームなら `FiniteSpec`，連続ゲームなら `ContinuousSpec` で，それぞれ規則のレジストリを持ちます（`RuleRegistry::finite_builtins()` / `RuleRegistry::continuous_builtins()`）．

`GameTrialFactory` は `module_pack` を読まないので，ライブラリモードではどの名前でも構いません．同じファイルを `socsim` CLI で走らせるには `module_pack = "normal-form"` と書きます．[`normal-form` パック](packs/normal-form.ja.md)が `[world.game] kind` から `FiniteSpec` か `ContinuousSpec` を選び，組込みの規則でこのファクトリを走らせます（`socsim validate`，`run`，検証付きの `sweep`．[CLI リファレンス](cli.ja.md#検証付きスイープnormal-form)を参照）．

### 書く場所

`Scenario` が持つ節は `[simulation]`，`[world]`，`[[mechanism]]`，`[output]` の 4 つだけで，socsim-config は**それ以外の最上位のテーブルをエラーにせずに無視します**．`[game]` や `[matching]` のテーブルは黙って捨てられます．
各部品は次の場所に書いてください．

| 部品 | 場所 |
|---|---|
| ステップ数，seed，スケジューラー | `[simulation]` の `t_max`，`seed`（`validate` が使う），`scheduler`（`"sequential"` か `"random_activation"`） |
| 集団の大きさ，履歴の長さ，トポロジー | `[world]` |
| 段階ゲーム | `[world.game]` |
| エージェントの組ごとの規則，初期行動 | `[world.population]` |
| Matching | `[[mechanism]] name = "game_match"` の `params` |
| 決定の粒度と規則のパラメータ | `game_decide` の `params` |
| 利得の集計と集計値 | `game_play` の `params` |
| 集団動学 | `imitate_best`，`fermi_update`，`replicator_update`，`moran_update` のどれかの `params` |
| 指標 | `game_metrics` の `params` |

`[simulation]` の `name`，`module_pack` と `[output]` は `Scenario` の書式として必須ですが，`GameTrialFactory` は読みません．

### 値：数・個数・ラベル

値はすべて型を確かめて読みます．`socsim_config::Params` のゲッター（[ステップ3](library.ja.md#ステップ3--modulepack-としてまとめる推奨) を参照）と違い，**型の違う値が既定値に戻ることはありません**．下に挙げていないキーはすべてエラーです．

- **数**（number）は TOML の整数か有限の浮動小数です．`temptation = 2` は `2.0` として読みます．
- **個数**（count）は，TOML の整数か小数部の無い浮動小数で，有限で，非負で，読み込む型（`u64`，`u32`，`usize`）の範囲に収まるときだけ受理します．
  `16` と `16.0` は受理し，`-1`，`1.5`，`1e30`，`nan`，`"16"`，`true` は拒否します．スイープが個数を浮動小数で書き戻しても，正しく読み戻せます．
- **ラベル**（label）は ASCII の英数字・`_`・`-` からなる行動のラベルで，役割 0 の行動を指します．

`run_trial_sweep` は既存の整数を整数のまま保ちます．Scenario に `temptation = 5` と書くと，`world.game.temptation` を `4.5` に振るスイープは失敗します．
振るつもりの数は浮動小数で書いてください（`temptation = 5.0`）．

### `[world]`

| キー | 型 | 既定値 | 備考 |
|---|---|---|---|
| `n_agents` | 個数 | 必須（格子では省略可） | 格子なら `grid_rows * grid_cols` と一致しなければならない |
| `history_len` | 個数 | 必須 | 1 以上 |
| `topology` | `"none"`，`"grid"`，`"network"` | `"none"` | `"grid"` には feature `grid`，`"network"` には feature `net` が要る |
| `grid_rows`，`grid_cols` | 個数 | 格子では必須 | |
| `boundary` | `"fixed"`，`"toroidal"` | 格子では必須 | |
| `neighborhood` | `"moore"`，`"von_neumann"` | 格子では必須 | |
| `network_model` | `"erdos_renyi"`，`"watts_strogatz"`，`"barabasi_albert"` | ネットワークでは必須 | 試行ごとに `derive_seed(seed, &[0, 1])` から生成する |
| `network_p` | $[0, 1]$ の数 | `erdos_renyi` で必須 | |
| `network_k`，`network_beta` | $2 \le k \le$ `n_agents` $- 1$ の偶数の個数，$[0, 1]$ の数 | `watts_strogatz` で必須 | |
| `network_m` | $1 \le m \le$ `n_agents` $- 1$ の個数 | `barabasi_albert` で必須 | |

対応する `topology` の無い格子・ネットワークのキーと，別の `network_model` のネットワークのキーはエラーです．
`socsim-net` のネットワークの生成関数は範囲外の値を黙って丸めますが，読込器はそれを先に拒否します．

### `[world.game]`

`kind` は必須です．

| `kind` | 種別 | キー |
|---|---|---|
| `matrix` | 有限 | `labels`（`labels[role][action]`），`payoffs`（どちらも必須） |
| `prisoners_dilemma` | 有限 | `temptation`，`reward`，`punishment`，`sucker`（4 つとも必須．既定のゲームは無い） |
| `battle_of_sexes`，`matching_pennies`，`stag_hunt` | 有限 | `a`，`b`（数の 2 × 2 の配列．両方書くか，両方省略して既定のゲームにする） |
| `rock_paper_scissors` | 有限 | `win`，`loss`（両方書くか，両方省略して既定のゲームにする） |
| `hawk_dove` | 有限 | `v`，`c`（必須） |
| `el_farol` | 有限 | `capacity`（個数．必須）．プレイヤーは `n_agents` 人 |
| `minority` | 有限 | なし．プレイヤーは `n_agents` 人 |
| `public_goods_finite` | 有限 | `k`（個数），`r`，`cost`（すべて必須） |
| `cournot` | 連続 | `a`，`b`，`c`，`q_max`（すべて必須）．企業は `n_agents` 社 |
| `bertrand_differentiated` | 連続 | `a`，`b`，`d`，`c`，`p_max`（すべて必須）．企業は `n_agents` 社 |
| `public_goods_continuous` | 連続 | `k`（個数），`endowment`，`r`（すべて必須），`social_dilemma`（真偽値．既定 `false`．`true` なら $1 < r < k$ も確かめる） |

パラメータは組込みのゲームの条件を満たさなければなりません（[組込みのゲーム](#組込みのゲームcatalog) と [組込みの連続ゲーム](#組込みの連続ゲーム) を参照）．違反は `world.game` で報告されます．
`GameTrialFactory<ContinuousSpec>` に有限の `kind` を書く（その逆も）とエラーです．
行列ゲームの役割の数は任意で，`labels` の要素ごとに 1 つです．`payoffs` は各役割の行動の順に入れ子にし，最も内側に全役割の利得を置きます．

```toml
[world.game]
kind    = "matrix"
labels  = [["C", "D"], ["C", "D"]]
payoffs = [
  [[3.0, 3.0], [0.0, 5.0]],   # payoffs[a0][a1] = [payoff of role 0, payoff of role 1]
  [[5.0, 0.0], [1.0, 1.0]],
]
```

`el_farol`，`minority`，`cournot`，`bertrand_differentiated` のプレイヤーの数は `[world] n_agents` で，`[world.game]` に `n` を書くとエラーです．全エージェントが 1 回の対戦に入ります（`matching = "whole_population"`）．
これらのゲームを集団の一部ずつの組で行う構成（`random_groups(k)` × `minority`）は Scenario では書けません．`GameSimFactory` で組んでください．
`public_goods_finite` と `public_goods_continuous` の `k` は組の大きさで，`matching = "random_groups"` の `k` と一致しなければなりません．

### `[world.population]`

| キー | 型 | 既定値 | 備考 |
|---|---|---|---|
| `assign` | `"ordered"`，`"shuffled"` | `"ordered"` | `"shuffled"` は `derive_seed(seed, &[0])` で並べ替える |
| `[[world.population.group]]` | テーブルの配列．1 つ以上 | 必須 | 各要素は `rule` と，`count`（個数）か `share`（数）のどちらか一方を持つ |
| `initial_shares` | ラベルから数へのテーブル | | 有限ゲームだけ |
| `initial_uniform` | 真偽値 | `false` | 連続ゲームだけ |
| `initial_action` | ラベル（有限）か数の配列（連続） | 有限ゲームでは行動 0 | |

**組**．`rule` は規則のレジストリに登録した名前か，規則を持たないエージェントを表す `held` です．1 つの規則には組を 1 つしか書けません．
全部の組が `count` を使う（和は `n_agents`）か，全部の組が `share` を使う（和は $10^{-9}$ の許容差で 1）かのどちらかで，混ぜるとエラーです．
比は最大剰余法で人数にし，端数が同じなら先に書いた組が取ります．
`assign = "ordered"` では，組は書いた順にエージェントの ID を取ります（TOML の配列は順序を保ちますが，テーブルのキーは保ちません）．

**初期行動**．`initial_shares`，`initial_uniform`，`initial_action` のうち書けるのは高々 1 つです．
どれも無ければ，有限ゲームでは全員が行動 0 をとり，連続ゲームではエラーです．

- `initial_shares = { D = 0.1 }`（有限）：ラベルを 1 つだけ省略でき，省略したラベルが残り（$1 - \sum$）を受け取ります．書いた比の和が 1 を超えるとエラーです．
  これにより，2 行動の Scenario で `world.population.initial_shares.D` だけを振れます．
  人数は正確に決め（最大剰余法．端数が同じなら添字の小さい行動が取る），行動を**添字の順**に並べてから `derive_seed(seed, &[0, 2])` で並べ替えます．組を行動の順に並べた `InitialActions::Shares` と同じです．
  例 `nowak_may` は裏切り者を先に並べるので，同じ seed でも Scenario の経路とは初期の格子が違います（[初期行動](#初期行動initialactions-と-uniform_initial_actions) を参照）．
- `initial_uniform = true`（連続）：各エージェントの行動の各成分を，役割 0 の区間から `uniform_initial_actions` で一様に引きます（ストリーム `[0, 2]`）．
- `initial_action`：全員に同じラベル（有限）か同じ行動のベクトル（連続）を与えます．

### `[[mechanism]]` の項目

各項目は `name` で区別し，`phase` は無視します．

- `game_match`，`game_decide`，`game_play` は必須です．集団動学（`imitate_best`，`fermi_update`，`replicator_update`，`moran_update`）は高々 1 つ，`game_metrics` は高々 1 つ足せます．
- 同じ名前は 1 回しか書けず，未知の名前はエラーです．
- 集団動学は `game_decide` より**後**に書かなければなりません．
- それ以外の点で項目の順は関係ありません．Mechanism は常に `game_match`，`game_decide`，動学，`game_play`，`game_metrics` の順に走ります．

| Mechanism | キー | 型 | 既定値 |
|---|---|---|---|
| `game_match` | `matching` | `"fixed_players"`，`"round_robin"`，`"random_pairs"`，`"random_groups"`，`"whole_population"`，`"grid_neighborhood"`（feature `grid`），`"network_edges"`（feature `net`） | 必須 |
| | `pairs` | エージェントの ID の組の配列（`[[0, 1], [2, 3]]`） | `fixed_players` で必須 |
| | `both_orders` | 真偽値 | `false`．`round_robin` だけ |
| | `k` | 個数 | `random_groups` で必須 |
| | `include_self` | 真偽値 | `false`．`grid_neighborhood` だけ |
| `game_decide` | `decision_scope` | `"per_step"`，`"per_agent_role"`，`"per_encounter"` | `"per_step"` |
| | `report_mixed` | 真偽値 | `false` |
| | 登録した規則のキー | 次の表を参照 | |
| `game_play` | `payoff_aggregation` | `"sum"`，`"mean"` | `"sum"` |
| | `aggregate_history_len` | 個数 | なし．書くとゲームの公開集計値を記録する（`WithAggregates`） |
| | `materialize` | 真偽値 | `false` |
| | `counting` | 真偽値 | `false`．`true` なら `counting_play()` で評価する（利得は同じ）．受け付けるのは匿名の有限ゲーム `hawk_dove`，`el_farol`，`minority`，`public_goods_finite` だけで，連続ゲームでは `false` でもキーを拒否する |
| `imitate_best` | `update` | `"synchronous"` | 必須 |
| | `tie_break` | `"keep_own"`，`"lowest_action"` | `"keep_own"` |
| `fermi_update` | `update`，`k` | `"synchronous"`，数 | どちらも必須 |
| `replicator_update` | `update`，`beta` | `"synchronous"`，数 | どちらも必須．有限ゲームだけ |
| `moran_update` | `update`，`fitness`，`w` | `"synchronous"`，`"linear"` か `"exponential"`，数 | すべて必須 |
| | `mutation` | 数 | 0 |
| | `gaussian_sd` | 数 | なし．連続ゲームで `mutation > 0` なら必要で，有限ゲームでは書けない |
| `game_metrics`（有限） | `cooperate_label`（ラベル），`regret`（真偽値），`mixed_source`（`"played"`，`"played_cumulative"`，`"reported"`），`equilibrium`（`"none"`，`"solve"`），`stop_tol`（数）と `stop_window`（個数．`u32`）は両方書くか両方省略，`population_share`（真偽値），`action_entropy`（真偽値．feature `metrics`） | | `MetricsOptions::new()` の既定値 |
| `game_metrics`（連続） | `equilibrium`（`"none"`，`"solve"`），`stop_tol` と `stop_window` は両方書くか両方省略 | | `ContinuousMetricsOptions::new()` の既定値 |

別の Matching のキー（例：`matching = "round_robin"` での `pairs`）はエラーです．

規則のパラメータは，接頭辞を付けた名前で `game_decide` に書きます．組込みの規則と，それが読むキーは次のとおりです．

| 規則（`rule = ..`） | 種別 | `game_decide` のキー |
|---|---|---|
| `uniform_random` | 有限 | なし |
| `tit_for_tat` | 有限 | `coop_label`（必須） |
| `myopic_best_response` | 有限 | `myopic_tie_break`（`"lowest_index"` か `"random"`．既定 `"lowest_index"`），`myopic_first_action`（ラベル．既定は行動 0） |
| `fictitious_play` | 有限 | `fp_tie_break`（`"lowest_index"` か `"random"`．既定 `"lowest_index"`），`fp_initial_counts`（相手の行動ごとに 1 つの数．既定はすべて 0） |
| `logit` | 有限 | `logit_lambda`（必須），`logit_estimate`（`"empirical_frequency"` か `"last_opponent"`．既定 `"empirical_frequency"`） |
| `roth_erev` | 有限 | `roth_erev_phi`，`roth_erev_epsilon`，`roth_erev_initial_propensity`（すべて必須） |
| `all_c`，`all_d`，`grim`，`wsls` | 有限 | `coop_label`（必須） |
| `gtft` | 有限 | `coop_label`，`gtft_generosity`（どちらも必須） |
| `damped_best_response` | 連続 | `br_damping`（必須） |
| `gradient` | 連続 | `grad_step`（必須） |

規則は試行ごとに 1 回作り，その組の全エージェントで共有します．
`fixed`，`mixed` と LLM の規則は Scenario では使えません．連続ゲームの Scenario に有限の規則の名前を書くと，未知の規則になります．

### エラー

どのエラーも，直すべき場所を名指しします．
読込のエラーは，先頭にキーのパスを付けた `GameError::Config` の文言です．`GameTrialFactory` はそれを `SocsimError::Config` としてそのまま渡し，ほかの組み立てのエラーには位置を先頭に付けます．

```text
world.history_len: expected an integer, found 1.5
world.population.group[0].count: expected a non-negative integer, found -1
world.game.temptation: expected a number, found string
mechanism[game_decide].fp_tiebreak: unknown key (known keys: coop_label, decision_scope, fp_initial_counts, fp_tie_break, gtft_generosity, logit_estimate, logit_lambda, myopic_first_action, myopic_tie_break, report_mixed, roth_erev_epsilon, roth_erev_initial_propensity, roth_erev_phi)
world.game: catalog condition failed: T > R > P >= S
mechanism[imitate_best]: a population dynamic must be listed after game_decide
mechanism[replicator_update]: invalid population dynamics: replicator_update requires beta <= 1 / 16.650000000000002 (the per-step payoff range), got 1
```

（`SocsimError` の `Display` を通すと，どの文言にも `config error: ` が前に付きます．）組み立てのエラーの位置は，エラーの種類で決まります．

| 組み立てのエラー | 位置 |
|---|---|
| `Catalog`，`Matrix` | `world.game` |
| `AgentCount`，`InvalidHistoryLength` | `world` |
| `Topology` | `world.topology` |
| `Action` | `world.population` |
| `InvalidKinds` | `world.population.group` |
| `InvalidAggregateHistoryLength` | `mechanism[game_play].aggregate_history_len` |
| `UnsupportedRoles`，`InvalidMatching` | `mechanism[game_match]` |
| `Rule`，`ManifestMismatch` | `mechanism[game_decide]` |
| `Metrics` | `mechanism[game_metrics]` |
| `Population` | `mechanism[<Scenario に書いた動学の名前>]` |

### 乱数ストリーム

Scenario から読んだシミュレーションの乱数は，すべて `build_trial` に渡す試行の seed から派生します．

| ストリーム | 引くもの |
|---|---|
| `derive_seed(seed, &[0])` | `assign = "shuffled"` の並べ替え |
| `derive_seed(seed, &[0, 1])` | `topology = "network"` のランダムネットワーク |
| `derive_seed(seed, &[0, 2])` | `initial_shares` の配置と `initial_uniform` の行動 |
| `derive_seed(seed, &[1])` | エンジン（`Trial::engine_seed`） |
| `derive_seed(seed, &[2])` | 決定の根 |
| `derive_seed(seed, &[3, agent, generation])` | 規則の状態の初期値 |

これは `GameSimFactory` と `build_world` / `assemble` のストリーム（[決定論性と乱数ストリーム](#決定論性と乱数ストリーム) を参照）と同じで，そのため 2 つの入口の軌跡が一致します．

### `GameTrialFactory`

| 項目 | 意味 |
|---|---|
| `GameTrialFactory::<FiniteSpec>::new(RuleRegistry::finite_builtins())` | 実験状態の無い（`X = ()`）factory．連続ゲームでは `ContinuousSpec` と `RuleRegistry::continuous_builtins()` |
| `GameTrialFactory::with_state(registry, make_x)` | `make_x: Fn(&Scenario) -> Result<X, GameError>` が作る実験状態から試行を始める factory．`make_x` は試行を作るたびに 1 回呼ばれる |
| `TrialFactory::build_trial(&scenario, seed)` | Scenario を読んで `seed` の試行を作る．World は `seed` から，Mechanism は決まった順，スケジューラーは `[simulation] scheduler`，`engine_seed = derive_seed(seed, &[1])` |
| `TrialFactory::validate(&scenario)` | [`validate`](#validate) を参照 |

factory が持つのは `Arc` に包んだ規則のレジストリだけなので，複製は軽く，1 つの factory をすべてのワーカースレッドで使えます．
スイープでは点ごとに Scenario が変わるので，factory は試行ごとに Scenario を読み直します．
`TrialFactory` の実装は `G: ScenarioGame` と `X: ExperimentState + Clone` について generic です．`ScenarioGame` は封印されていて，実装するのは `FiniteSpec` と `ContinuousSpec` だけです．

`FiniteSpec` と `ContinuousSpec`（既定のビルド）は組込みのゲームを包む enum です — `FiniteSpec::Matrix(MatrixGame)`，`FiniteSpec::PrisonersDilemmaGame(PrisonersDilemma)`，…，`ContinuousSpec::CournotGame(Cournot)`，…．
ゲームのメソッドはすべて包んだゲームに委譲するので，`kind` で選んだゲームはそのゲーム自身とまったく同じに振る舞います．
serde の既定のタグで直列化されます（例：`{"HawkDoveGame": {"v": 2.0, "c": 3.0}}`）．
`AnonymousGame` を実装しないので，Scenario は常に対ごとの評価経路を使います．
コンパイラーは，ゲームのトレイトの実装が足りないというエラーで，これらを実装例として挙げません．

### `validate`

`validate(&scenario)` は次を行います．

1. Scenario のすべてのキーを，上の検査とともに読む．
2. `[[mechanism]]` の項目の並びを確かめる（集団動学が `game_decide` より後にあることを含む）．
3. どのエージェントも従っていないが，**その規則だけ**が読むパラメータが `game_decide` に書かれている規則を構築する．どの組も `logit` を使っていなくても，`logit_lambda` 無しで `logit_estimate` を書けば失敗する．一方，複数の規則が共有するキー（`coop_label`）を書いても，使っていない規則が必須になることはない．
4. `scenario.simulation.seed` の試行を 1 つ作って捨てる．これで組み立ての実行時の検査がすべて走る．

`run_trial`，`run_trials`，`run_trial_sweep` は，試行を走らせる前に `validate` を呼びます．
検査の中には seed に依存するものがあります．ランダムネットワークでは，`replicator_update` の `beta` の上限を決める利得の範囲が，生成したネットワークの次数で決まります．
したがって `validate` を通った Scenario は `build_trial` で必ず読めますが，別の seed では組み立てに失敗することがあります．その場合もエラーになり，黙って走ることはありません．

### 利用者の規則：`RuleRegistry::register`

`RuleRegistry<G, X>` は規則の名前を構築関数に対応づけます．
利用者の規則は，それが読む `game_decide` のキーとともに登録します．登録したキーは受理され，それ以外のキーは引き続き未知のキーのエラーになります．

```rust
use std::sync::Arc;

use socsim_config::{Params, Scenario};
use socsim_games::{
    DecisionRule, FiniteGame, FiniteSpec, Fixed, GameError, GameTrialFactory, RuleRegistry,
};

fn registry() -> Result<RuleRegistry<FiniteSpec, ()>, Box<dyn std::error::Error>> {
    let mut registry = RuleRegistry::<FiniteSpec, ()>::finite_builtins();
    registry.register("fixed", &["fixed_label"], |params: &Params, game: &FiniteSpec| {
        let path = "mechanism[game_decide].fixed_label";
        let label = params
            .get_value("fixed_label")
            .and_then(|value| value.as_str())
            .ok_or_else(|| GameError::Config(format!("{path}: a label is required")))?;
        let action = (0..game.n_actions(0))
            .find(|&a| game.action_label(0, a) == label)
            .ok_or_else(|| GameError::Config(format!("{path}: no action {label:?}")))?;
        let rule: Arc<dyn DecisionRule<FiniteSpec, ()>> = Arc::new(Fixed::new(action));
        Ok(rule)
    })?;
    Ok(registry)
}
```

`game_decide` に `rule = "fixed"` と `fixed_label = "D"` を書くと，その組のエージェントは常に裏切ります．
綴りを誤った `fixed_lable` は `mechanism[game_decide].fixed_lable: unknown key (known keys: ..., fixed_label, ...)` として拒否されます．

- 構築関数は `game_decide` の `params` のテーブル全体（`socsim_config::Params`）とゲームを受け取ります．`Params` の型付きのゲッターは型の違う値に既定値を返すので，値は `Params::get_value` で読み，型の違う値は自分で拒否してください．エラーの文言は `mechanism[game_decide].<key>` で始めてください．
- `register` は，登録済みの名前には `RuleError::DuplicateKind` で，予約名 `held` には `RuleError::Unsupported` で失敗します．`names()` は名前を辞書順に返します．
- 構築関数の型は `RuleCtor<G, X> = Box<dyn Fn(&Params, &G) -> Result<Arc<dyn DecisionRule<G, X>>, GameError> + Send + Sync>` です．

### Scenario を自分のコードで読む：`ScenarioConfig`

`ScenarioConfig::<G, X>::read(&scenario, &registry)` は，何も組み立てずに Scenario を読んで検査するので，ライブラリモードで一部を変えて使えます．
ゲーム（`game()`），`n_agents()`，`history_len()`，`assignment()` と，seed を与えた `initial_actions(seed)`，`topology(seed)` を返します（ランダムネットワークと一様の初期行動はその seed から引きます）．
規則と Mechanism は返しません．集団に規則があるときは `.rule(..)` で登録し直してください．

```rust
use socsim_config::Scenario;
use socsim_games::{
    FermiUpdate, FiniteSpec, GameSimFactory, GridNeighborhood, RuleRegistry, ScenarioConfig,
    Update,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let scenario = Scenario::from_path(std::path::Path::new("spatial.toml"))?;
    let config = ScenarioConfig::<FiniteSpec>::read(&scenario, &RuleRegistry::finite_builtins())?;
    let seed = 3;
    let mut factory = GameSimFactory::new(config.game().clone())
        .agents(config.n_agents())
        .history_len(config.history_len())
        .t_max(scenario.simulation.t_max)
        .assign(config.assignment().clone())
        .initial_held(config.initial_actions(seed))
        .matching(GridNeighborhood::new().include_self(true))
        .dynamics(FermiUpdate::new(0.1, Update::Synchronous)?); // not the scenario's dynamic
    if let Some(topology) = config.topology(seed) {
        factory = factory.topology(topology);
    }
    factory.build(seed)?.run()?;
    Ok(())
}
```

### ほかの Scenario

格子上の集団 — 初期の裏切り者が 10 % の空間囚人のジレンマです．
ゲームを `FiniteSpec::PrisonersDilemmaGame(PrisonersDilemma::weak(1.85)?)` とし，`Topology::grid(..)`，`GridNeighborhood::new().include_self(true)`，`InitialActions::Shares(vec![(0, 0.9), (1, 0.1)])`，`ImitateBest::new(Update::Synchronous)`，`MetricsOptions::new().population_share(true)` を設定した `GameSimFactory` と，ステップごとに同じ World と指標の行を作ります．

```toml
[simulation]
name        = "spatial_pd"
module_pack = "library"
t_max       = 50
seed        = 1
scheduler   = "sequential"

[world]
history_len  = 1
topology     = "grid"
grid_rows    = 100
grid_cols    = 100
boundary     = "toroidal"
neighborhood = "moore"

[world.game]
kind       = "prisoners_dilemma"
temptation = 1.85
reward     = 1.0
punishment = 0.0
sucker     = 0.0

[world.population]
initial_shares = { D = 0.1 }        # C receives the rest, 0.9
[[world.population.group]]
rule  = "held"
share = 1.0

[[mechanism]]
name = "game_match"
[mechanism.params]
matching     = "grid_neighborhood"
include_self = true

[[mechanism]]
name = "game_decide"

[[mechanism]]
name = "imitate_best"
[mechanism.params]
update = "synchronous"

[[mechanism]]
name = "game_play"

[[mechanism]]
name = "game_metrics"
[mechanism.params]
population_share = true

[output]
log_path = "runs/{name}_{seed}.jsonl"
```

減衰つきの最適応答で一様な初期の生産量から始める 3 社のクールノー競争です．`GameTrialFactory::<ContinuousSpec>::new(RuleRegistry::continuous_builtins())` で読みます．

```toml
[simulation]
name        = "cournot3"
module_pack = "library"
t_max       = 500
seed        = 7
scheduler   = "sequential"

[world]
n_agents    = 3                   # the three firms
history_len = 4

[world.game]
kind  = "cournot"
a     = 100.0
b     = 1.0
c     = 10.0
q_max = 100.0

[world.population]
initial_uniform = true            # each firm uniform on [0, q_max], stream [0, 2]
[[world.population.group]]
rule  = "damped_best_response"
count = 3

[[mechanism]]
name = "game_match"
[mechanism.params]
matching = "whole_population"

[[mechanism]]
name = "game_decide"
[mechanism.params]
br_damping = 0.5

[[mechanism]]
name = "game_play"
[mechanism.params]
aggregate_history_len = 4         # records total_output and price

[[mechanism]]
name = "game_metrics"
[mechanism.params]
equilibrium = "solve"
stop_tol    = 1e-9
stop_window = 10

[output]
log_path = "runs/{name}_{seed}.jsonl"
```

`run_trial(&scenario, &factory, 7)` では，収束による停止で 500 ステップのうち 90 ステップで止まり，`game.mean_action.0 = 22.5`，`game.aggregate.total_output = 67.5`，`game.aggregate.price = 32.5` になります．

### 制限

- Scenario で選べるのは，組込みのゲーム・規則・Matching・動学です．利用者定義のゲーム・利得規則・Matching，`OnInvalidAction::Fallback`，規則 `fixed` と `mixed`，LLM の規則はライブラリモード（`GameSimFactory`）でだけ使えます．利用者の決定規則は `RuleRegistry::register` で登録できます．数え上げの評価経路は `game_play` の `counting = true` で使えます（匿名の有限ゲームだけ）．
- factory を `GameTrialFactory::with_state` で作らない限り，実験状態は `()` です．
- 集団動学は `update = "synchronous"` だけを受け付けます．
- `minority` や `el_farol` と `random_groups(k)` の組合せは書けません（プレイヤーは `n_agents` 人）．
- ネットワークは無向です（`Topology::network`）．
- 未知の最上位のテーブルは socsim-config が無視し，報告しません．
