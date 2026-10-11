[English](games.md) | **日本語**

# 戦略形ゲーム（`socsim-games`）

`socsim-games` は socsim のエンジン上でゲーム理論のシミュレーションを走らせる crate です．
エージェントは**段階ゲーム**の対戦の席に着き，各自が**決定規則**で行動を選び，
**利得規則**が行動の組を利得に変え，規則は起きたことから学習します．
これを毎ステップ，通常の [6フェーズティックループ](architecture.ja.md#6フェーズティックループ) の中で行います．

この crate はライブラリ専用です（`ModulePack` を持たず，`socsim` バイナリには組み込まれていません）．
このページでは，部品，その状態の置き場所，規則が見てよい情報，実行可能なシミュレーションの組み立て方，
組込みの学習則と決定の粒度（[学習則と決定の粒度](#学習則と決定の粒度) を参照），
ステップごとの指標を記録する Mechanism `game_metrics`（[指標](#指標) を参照）を説明します．
シミュレーションとは別に，古典的なゲームと有限ゲームのソルバー（Nash 均衡，支配，ミニマックス，ロジット QRE）も提供します（[有限ゲームを解く](#有限ゲームを解く) を参照）．

---

## Cargo feature

| feature | 追加されるもの | socsim への依存 |
|---|---|---|
| *（既定）* | 段階ゲームのトレイト（`StageGame`，`NormalFormGame`，`FiniteGame`，`AggregateGame`），`MatrixGame`，組込みのゲーム（`catalog`），混合戦略（`mixed`），ソルバー（`solve`），学習則の計算（`learn`），`AggregateSpec` と指標用の行動 ID（`metric_action_ids`） | **なし** — 既定のビルドはどの `socsim-*` crate にも依存しません（ソルバーの線形計画のバックエンドは純 Rust の `microlp` crate） |
| `core` | World（`GameWorld`），規則（学習則を含む），利得規則，Matching，ビュー，`build_world`，指標の Mechanism `GameMetrics` | `socsim-core` |
| `engine` | `assemble` と `assemble_with_aggregates`（どちらも `SimulationBuilder` を返す）．`core` を含意 | `socsim-core`，`socsim-engine` |
| `metrics` | `GameMetrics` の列 `game.action_entropy`（[指標](#指標) を参照）．`core` を含意 | `socsim-core`，`socsim-metrics`（既定の feature を切ったもの．依存の無い `stats` モジュールだけ） |
| `llm` | LLM の決定規則，応答ストア，再生の manifest（[LLM による決定](#llm-による決定) を参照）．`core` を含意．Mechanism `game_llm_decide` と `assemble_with_llm` には `engine` も要る | `socsim-core`，`socsim-llm`（既定の feature だけ．ネットワークなし） |
| `live` | `live_backend_factory()`（キャッシュなしの Ollama → OpenAI のバックエンド）．`llm` を含意 | `llm` と同じ＋`socsim-llm/live` |

`llm` を有効にしなければ，この crate は `socsim-llm` に依存せず，`metrics` を有効にしなければ `socsim-metrics` に依存しません．`socsim-config` と `socsim-runner` には常に依存しません．
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

規則と利得規則は，必要な段をトレイト境界として宣言します．
`UniformRandom` と `TitForTat` は `FiniteGame` を，`NormalFormOutcome` は `NormalFormGame` を要求するので，
組み合わせの誤りは実行時ではなくコンパイル時のエラーになります．

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

シミュレーション層が現在組み立てるのは **2 役割**の対戦だけです．
それ以外の `n_roles()` は `assemble` が `GameError::UnsupportedRoles` で拒否します．

### 組込みのゲーム：`catalog`

`socsim_games::catalog` は古典的な 2 人ゲームを 5 つ提供します．どれも `FiniteGame` を実装しているので，
`MatrixGame` と同じように `build_world`，組込みの規則，[ソルバー](#有限ゲームを解く) に渡せます．
`to_matrix()` は同じラベルを持つ等価な `MatrixGame` に変換します．
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

対戦は `Credit::All`（全席がそれぞれの利得を受け取る）でなければなりません．
それ以外の credit の方式はまだ提供されておらず，ステップがエラーで止まります．

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
`Slot<PropensityState>`（`RothErev`），`Slot<AutomatonPolicyState>`（`Automaton`）を要求します．
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
| 実験状態 | `world.x()` |

3 つの Mechanism（`game_match`，`game_decide`，`game_play`）と `RuleBook`，`Matching`，`OutcomeRule` は，
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

`encounters_into` は `out.push(&seats, Credit::All)` ごとに対戦を 1 つ積みます（`seats` は役割ごとに 1 エージェント）．
`check` は `assemble` の中で一度だけ走り，Matching が World と合っているかを検査します．
使われている [決定の粒度](#決定の粒度) は `ctx.decision_scope()` でわかります．
組込みの `FixedPlayers::pairs(&[(0, 1), …])` は毎ステップ同じ組を対戦させ，
未知のエージェント，2 回現れるエージェント，役割がちょうど 2 でないゲームを拒否します．
組込みの [`RoundRobin`](#roundrobin) は，毎ステップすべての 2 人の組を対戦させます．
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
PostStep     game_metrics (optional, added by you) → one "game" row of metrics
```

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
    initial_held: InitialActions<G::Action>,   // Same(a) | PerAgent(vec)
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
```

`build_world` の検査：`history_len > 0`（`InvalidHistoryLength`），割り当ての `count` の和と `PerAgent` の長さが `n_agents` に一致（`AgentCount`），
初期行動がすべての役割で `check_action` を通る，割り当てた種類がすべて `RuleBook` にある（`RuleError::UnknownKind`），各 `init_state` が成功する．

`assemble` の検査：ゲームが 2 役割である（`UnsupportedRoles`），World にある規則の種類がすべて `RuleBook` にある，
`OnInvalidAction::Fallback` のとき使われている種類すべてに全役割で妥当な代替行動がある，
`report_mixed(true)` と `DecisionScope::PerEncounter` を組み合わせていない（`GameError::Metrics`），`Matching::check`．
`assemble_with_aggregates` は同じ検査に加えて `aggregate_history_len > 0` を求めます（`InvalidAggregateHistoryLength`）．
これは `assemble` に [公開集計値](#公開集計値aggregategame) の表を足したものです．
エンジンのシードを `derive_seed(seed, &[1])` に設定してビルダーを返すので，`build()` の前にスケジューラー，Recorder，追加の Mechanism を足せます．
2 つの関数には**同じ** `seed` を渡してください．返されたビルダーで `.seed(...)` を呼ぶと，エンジンのストリームが上書きされます．

### `PlayOptions`

```rust
PlayOptions::new()                                     // Sum + Stop + PerStep
    .payoff_aggregation(PayoffAggregation::Mean)        // Sum (default) | Mean
    .on_invalid_action(OnInvalidAction::Stop)           // Stop (default) | Fallback(map)
    .decision_scope(DecisionScope::PerEncounter)        // PerStep (default) | PerAgentRole | PerEncounter
    .report_mixed(true)                                 // false (default) | true
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
  `MatrixGame` 用の `ActionParser` ではありません．

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

## 指標

`GameMetrics` は読むだけの Mechanism（名前は `game_metrics`）で，ゲームのステップごとの統計 — 行動の割合，利得の平均とばらつき，
協力率，後悔，均衡までの距離，公開集計値 — を Recorder の表 `"game"` に 1 行ずつ記録します．
`assemble` は登録しないので，ゲームから作って，返されたビルダーに足します．
**PostStep** でゲームの全 Mechanism の後に動くので，終わったステップを見ます．
乱数を引かず，モデルの状態を変えません．World に書くのは，収束による停止と累積の情報源のための自分の記録だけです
（[Mechanism のページ](mechanisms/game-metrics.ja.md) を参照）．

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
| `game.aggregate.<name>` | `assemble_with_aggregates` で組み立て，かつ集約が定義される | そのステップの集約済みの集計値（[公開集計値](#公開集計値aggregategame) を参照） |
| `game.regret` | 後悔が有効で，混合戦略の組が定義される | $\max_r \big[\max_a F_r(a, \sigma_{-r}) - F_r(\sigma)\big]$（`mixed::regret`） |
| `game.eq_distance` | `EquilibriumTarget::Solve` で，組が定義される | 解いた均衡に対する $\min_{\sigma^*} \sum_r \sum_a \lvert \sigma_r(a) - \sigma^*_r(a)\rvert$ |

1 行の中の列の順は，行動の割合，`coop_rate`，`action_entropy`，`mean_payoff`，`payoff_sd`，集計値，`regret`，`eq_distance` です．

**定義されない列は行から省き，NaN として書くことはありません．** 対戦の無いステップでは割合，協力率，エントロピーの列を記録せず，
誰も対戦しなかったステップでは利得の列を記録しません．有限でない値（計算した値，または読んだ利得）が出たときは，行を記録する前にステップを Mechanism のエラーで止めるので，
`game_metrics` が記録や World の自分の状態に NaN や無限大を書くことはありません．
ただし行をそのまま保持する Recorder は行を埋めることがあります．`socsim_log::CsvRecorder` は表に一度でも現れた列をすべて残し，
行に無い列には NaN を書きます．列が初めて現れる前に記録した行も NaN で埋めます．
`llm` では `game_llm_decide` も同じ表 `"game"` に自分の行を書くので，その表には 1 ステップに 2 行が入り，それぞれ相手の列が NaN になります．Mechanism は毎ステップ，World のゲームが作ったときと同じ役割数と行動数を持つことも検査します．

`played_share` が数えるのは**対戦の表の席**であって，エージェントではありません．3 つの対戦の席に着いたエージェントは 3 回数えます．
そのため，`held` を書かない粒度も含めて，どの[決定の粒度](#決定の粒度)でも定義されます．
集団の割合 — 各戦略を持つエージェントの割合 — は別の量で，`game_metrics` には含まれません．

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

`assemble` の代わりに `assemble_with_aggregates(.., seed, aggregate_history_len)` で組み立てます．
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
`game.action_entropy`，そして `game.aggregate.{name}`（組み立て方に依存するので常に並ぶ）です．
[runvault](https://github.com/akitenkrad/rs-runvault) で実行を記録するリポジトリは，これを `runvault.toml` に写せます．
`{…}` の区切りを含むパターンは `[[metric_patterns]]` に，そのままの名前は `[metrics."<name>"]` に書きます．
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

`unit`，`direction`，パターンの `axes` が欲しければ手で足してください．この crate 自体は runvault に依存しません．

---

## 決定論性と乱数ストリーム

すべてのストリームは，`build_world` と `assemble` に渡す 1 つの `seed` から派生します．

| ラベル | ストリーム |
|---|---|
| `derive_seed(seed, &[0])` | World の初期化（`Assignment::Shuffled`） |
| `derive_seed(seed, &[1])` | エンジン（スケジューラー，およびステップの RNG を通した `Matching` と `decide` の乱数） |
| `derive_seed(seed, &[2])` | `decision_root`．World に保存される．`game_llm_decide` は生成のシードとフォールバックのストリームをここから導く（[シード](#game_llm_decide-の-decision-の流れ) を参照） |
| `derive_seed(seed, &[3, agent, generation])` | 規則の状態の初期化（`RuleInitContext::init_seed`） |

したがってゲームのシミュレーションではラベル `[2]` と `[3]` が予約済みです．独自のストリームには `[4]` 以上を使ってください
（[RNG ストリームのラベル付け規約](library.ja.md#rngストリームのラベル付け規約) を参照）．
学習は `AgentId` の昇順で，決定はスケジューラーの活性化の順（1 人のエージェントの中では決定の単位の順）で行われます．
組込みの学習則が引くのは `decide` に渡されるエンジンのストリームだけです（[乱数](#乱数) を参照）．
同じ seed と同じコードなら，実行はステップごとに再現されます．
`game_metrics` と混合戦略の報告は乱数を引かず，規則の状態，保持行動，対戦の表に触れないので，足しても実行の軌跡は変わりません．
集計値の表も乱数を引きません．

---

## 有限ゲームを解く

シミュレーションとは別に，この crate は任意の `FiniteGame`（`MatrixGame`，[組込みのゲーム](#組込みのゲームcatalog)，自作の型）の
均衡とそれに関連する解概念を直接計算します．混合戦略は `socsim_games::mixed` に，ソルバーは `socsim_games::solve` にあります．
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
