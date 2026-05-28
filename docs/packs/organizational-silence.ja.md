[English](organizational-silence.md) | **日本語**

# `organizational-silence` パック

> **階層型ネットワーク上の組織的沈黙（organizational silence）**：従業員は異質な沈黙動機を持ち，恐怖・上司の開放性・周囲の沈黙圧力のもとで，懸念を voice するか silent にとどまるかを決定します — 沈黙の風土（climate of silence）の創発（Morrison & Milliken 2000）．
> **ワールド：** `SilenceWorld`．**メカニズム：** 10個（加えてオプションの LLM 駆動 `voice_decision`）．**Cargo フィーチャ：** `pack-organizational-silence`（デフォルトで有効）．`pack-organizational-silence-llm` を追加すると LLM 変種が加わります．**時間単位：** 1ステップ ≈ 1か月．

[← パックカタログに戻る](../packs.ja.md)

## 1. 概要

`organizational-silence` パックは，**沈黙の風土（climate of silence）** — 大多数の従業員が現状に内心で同意していないにもかかわらず公には黙り続ける状態（Morrison & Milliken 2000） — に，組織がいかに陥り，そこからいかに脱出するかをモデル化します．これは [`opinion-dynamics`](opinion-dynamics.ja.md) パックの社会組織版です．いずれもネットワーク上の創発的な集団行動を観察しますが，opinion dynamics が単一スカラー意見を集約するのに対し，本パックは恐怖（fear，Kish-Gephart et al. 2009），知覚された心理的安全性（psych_safety，Edmondson 1999），暗黙の発言観（implicit voice theory，Detert & Edmondson 2011），そして3つの沈黙動機（**acquiescent**：諦め型，**defensive**：恐怖型，**prosocial**：他者保護型 — Van Dyne, Ang & Botero 2003）のいずれか，をエージェントごとに持つより豊かな状態を追跡します．加えて上司シグナルと確率的な報復（retaliation）イベントを伴う階層的な組織の上で動作します．

このモデルが素朴な意見ダイナミクスネットワークと異なる点は2つあります．

- **階層と上司シグナル．** 従業員はチームに所属し，各チームは開放性 `u_k ∈ [-1, 1]` を持つ上司が率います．`u_k` は，従業員が voice を決断する際に読む，最も近接した手がかりです．パックは `supervisor_homogeneity` というノブも提供し，組織全体で同一の上司から一様分散した上司までを補間できます — Sohn (2022) が指摘した「同質な上司シグナルは沈黙の大域的創発を起こりやすくする」という効果を直接検証できます．
- **閾値カスケードとスパイラル知覚．** Granovetter / Kuran (1995) の選好偽装カスケードは，隣接 voice 比率が個別の閾値を超えた瞬間に多数の silent dissenter を一気に voice へ反転させます．一方，Noelle-Neumann (1974) の沈黙のスパイラルは逆方向に作用し，局所的に沈黙が密な場所では心理的安全性を侵食します．

パックは **LLM／ルールの切り替え**を組み込みのアブレーションとして提供します．シナリオ TOML 内のメカニズム名を1つ変更するだけで，Decision フェーズが較正済みロジスティックルールと LLM 駆動の voice 決定（`voice_decision`）の間で切り替わります．ワールドは，モデルが較正対象としている4つのマクロ変数 — 課題顕在性 σ(t)，沈黙の風土 C(t)，voice volume V(t)，組織業績 Π(t) — を保持しているため，Milliken 2003 の「~85% ever silent」や Detert & Edmondson 2011 の「IVT 高位群で ~50% が沈黙」を再現するには，標準のメトリクス系列を読み取るだけで足ります．

## 2. ワールド：`SilenceWorld`

`SilenceWorld` は，すべてのメカニズムが読み書きする共有状態を保持します．

| フィールド | 型 | モデル化対象 |
|---|---|---|
| `clock` | `SimClock` | シミュレーションクロック |
| `employees` | `BTreeMap<AgentId, Employee>` | 在職中のロスター（決定性のため id でソート） |
| `teams` | `Vec<Team>` | チームごとの上司開放性と `knowledge_stock` |
| `network` | `SocialNetwork` | [Watts–Strogatz](../architecture.ja.md) スモールワールドの結合グラフ |
| `issue_salience` | `f64` | σ(t) ∈ [0, 1]：当該課題が現時点でどれだけ可視・深刻であるか |
| `climate_of_silence` | `f64` | C(t)：`Silence` ∧ `private_concern < 0` であるエージェントの割合 |
| `voice_volume` | `f64` | V(t)：現在 `Voice` であるエージェントの割合 |
| `org_performance` | `f64` | Π(t) = `knowledge_stock` · (1 − C(t)) |
| `retaliation_this_step` | `Vec<AgentId>` | 一時的：このステップで報復を受けたエージェント → `fear_appraisal` と `psafety_update` が消費 |

各 **`Employee`** は，10個のメカニズムが作用する行動状態を保持します．`level`（1 = 現場 … L = 経営層），`tenure`（月数），所属 `team` インデックス，`private_concern` ∈ [-1, 1]（負値 ⇒ 現状に批判的），現在の公的 `expression` ∈ {`Voice`, `Silence`, `Neutral`}，経験的トレイトのスカラー `fear`（Kish-Gephart 2009），`psych_safety`（Edmondson 1999），`ivt_strength`（Detert & Edmondson 2011），カスケードのゲートとなる個別の `voice_threshold`（Kuran 1995），沈黙時に割り当てられる `silence_motive` ∈ {`Acquiescent`, `Defensive`, `Prosocial`}（Van Dyne 2003），そしてフェーズ間でスパイラル効果を運ぶ毎ステップのスナップショット `neighbor_silence_ratio` ρ_i です．各 **`Team`** は上司の `supervisor_openness` と，`org_learning` が更新する `knowledge_stock` を保持します．

ワールドは，シード付き [`SimRng`](../architecture.ja.md) から `SilenceWorld::new(n_teams, team_size, n_levels, ws_k, ws_beta, supervisor_homogeneity, &mut rng)` によって構築されるため，あるシードは常に同じ初期組織を生成します．各メカニズム内の決定性は [`hr-lifecycle`](hr-lifecycle.ja.md) パックと同じ `AgentId でソート` パターンに従います．RNG から抽出する，または `f64` を累積するエージェントごとの反復はいずれも，事前に候補集合をソートし，`BTreeMap` の反復もキー順です．

### 階層と上司均質性

`supervisor_homogeneity` パラメータ η ∈ [0, 1] は，リーダーシップの均一性に関する2つの極を補間します．

- **η = 1** — すべてのチームが同じベースライン上司開放性（≈ 0，共通平均）を割り当てられます．すべての上司が同じシグナルを送るため，沈黙のスパイラルは一風変わった逸脱チームに妨げられることがありません．
- **η = 0** — 上司開放性は `[-1, 1]` で一様にばらつき，組織には敵対的・中立的・開放的な上司が混在します．

中間の η は両者を線形に混ぜます．このパラメータは「同質な上司シグナルは沈黙の大域的創発を起こりやすくする」という Sohn (2022) の発見を直接調整するノブです．高い η ではスパイラルを破壊しうる少数の外れ値が消え，低い η では開放的な上司の部分集団が voice を生かし続けます．

## 3. 10個のメカニズム

パックは6つの socsim フェーズにまたがる10個のルールベースメカニズムを登録します．`pack-organizational-silence-llm` フィーチャが有効な場合，11個目のオプション `voice_decision` が `voice_decision_rule` を置き換えます（§3.1）．

| メカニズム | フェーズ | 種別 | 役割 |
|---|---|---|---|
| [`issue_salience`](../mechanisms/issue-salience.ja.md) | Environment | scenario-driven | σ(t) を更新．実行途中のトリガーイベント用のステップ関数ショック（`shock_t`，`shock_delta`）に対応． |
| [`retaliation_event`](../mechanisms/retaliation-event.ja.md) | Environment | stochastic | 毎ステップ確率 `p_retaliate` で，最近の voicer（フォールバックは任意のエージェント）を1名選び，本人と隣接者を「報復された」と印付け，`retaliation` イベントを記録（Kish-Gephart 2009）． |
| [`fear_appraisal`](../mechanisms/fear-appraisal.ja.md) | Decision | empirical | このステップの報復集合と上司開放性から各従業員の fear を更新．穏やかな風土ではステップごとの小さな減衰によりベースラインへ回帰（Kish-Gephart 2009）． |
| [`voice_decision_rule`](../mechanisms/voice-decision-rule.ja.md) | Decision | mixed | ルールベースのロジスティック voice／silence 決定．silence の場合は支配的な抑制要因に応じて動機 ∈ {`acquiescent`, `defensive`, `prosocial`} を割り当て（Van Dyne 2003）．LLM 変種 `voice_decision` はページを共有（§2.1）． |
| [`silence_spiral`](../mechanisms/silence-spiral.ja.md) | Interaction | empirical | 各従業員の隣接沈黙比 ρ_i をスナップショットし，`epsilon · ρ · 0.05` だけ psych_safety を下方に押す．ρ_i のスナップショットは次ステップへのスパイラル効果の運搬役（Noelle-Neumann 1974）． |
| [`prefalse_cascade`](../mechanisms/prefalse-cascade.ja.md) | Interaction | mixed | 反復的な voice 反転カスケード．`private_concern < 0` の silent エージェントは，隣接 voice 比が個別の `voice_threshold` を超えると Voice に反転し，不動点まで繰り返す．反転総数が母集団の `cascade_threshold`（デフォルト 5%）を超えると `cascade` イベントを記録（Kuran 1995 / Granovetter 1978）． |
| [`org_performance`](../mechanisms/org-performance.ja.md) | Reward | aggregation | マクロ集約を再計算し，`silence_rate`，`climate_of_silence`，`voice_volume`，`knowledge_stock`，`org_performance`，`opinion_clusters`，`n_employees` を記録．現在 silent なエージェントの (acquiescent, defensive, prosocial) 内訳を `motive_mix` イベントとして発火．（注：リファレンスページは hr-lifecycle のボディを文書化．silence ボディは本行と下記の注で要約．） |
| [`psafety_update`](../mechanisms/psafety-update.ja.md) | PostStep | empirical | voice したエージェントには `psafety_learn` 分 ψ を上げ，報復されたエージェントには `psafety_learn` 分 ψ を下げる（Edmondson 1999）． |
| [`climate_silence`](../mechanisms/climate-silence.ja.md) | PostStep | aggregation | C(t) を再計算し，カスケードや他の Reward／PostStep 変更を反映したステップ終了時点の世界に対応する値を公開． |
| [`org_learning`](../mechanisms/org-learning.ja.md) | PostStep | optional intervention | Argyris (1977) ダブルループ学習．少なくとも1名が voice し*かつ* σ(t) > `salience_floor` の場合，各 voicer のチームに `learning_rate` 分の `knowledge_stock` 増加．それ以外では全 stock が `decay_rate`（≈ 1%/step）で減衰し，沈黙の風土で更新されない暗黙知を表現． |

> **`org_performance` の注意．** リファレンスページ
> [`mechanisms/org-performance.ja.md`](../mechanisms/org-performance.ja.md) は
> hr-lifecycle のボディ（生産性合計，在職期間平均，離職率，チーム平均 θ 再計算）を文書化しています．
> **organizational-silence パック**が同名で登録するボディは別物です：
> `silence_rate`，`climate_of_silence`，`voice_volume`，`knowledge_stock`，
> `org_performance`，`opinion_clusters`，`n_employees` を記録し，
> 毎ステップ `motive_mix` イベントを発火します．2つのボディが共存できるのは，
> それぞれのパックが自分の World 型に対する `Registry<W>` へ `org_performance` を登録するからです．

完全な方程式・パラメータの既定値・引用は [`crates/socsim-packs/src/organizational_silence/mechanisms.rs`](../../crates/socsim-packs/src/organizational_silence/mechanisms.rs) と [`calibration.rs`](../../crates/socsim-packs/src/organizational_silence/calibration.rs) にあります．

### 3.1 LLM 変種 `voice_decision`

パックは2つ目の voice 決定メカニズムを，正準名 `voice_decision` の下に登録します（ルールベース版は `voice_decision_rule` を保持しているため，両者はレジストリ内で共存できます）．CLI レベルでは `pack-organizational-silence-llm`，`socsim-packs` クレートレベルでは `organizational-silence-llm` のフィーチャでゲートされており，いずれかを有効にすると `socsim list mechanisms` に追加メカニズムが現れます．

本番では LLM メカニズムは [`socsim-llm`](../architecture.ja.md#llm層socsim-llm) の `LiveClient` に接続されます．`LiveClient` は他の socsim LLM メカニズムと同様に，環境変数から **Ollama-first → OpenAI-fallback → キャッシュ**で組み立てられます．呼び出しごとの `LlmConfig` は `temperature = 0` とシナリオの seed を使用し，JSON ファイル背景の `PromptCache` がウォームな再実行を決定論的なオラクルに変えます．テストでは代わりに `from_client` コンストラクタ経由で `socsim_llm::mock::ScriptedClient` を注入します．

メカニズムは，level，tenure，fear，psych_safety，ivt_strength，neighbor silence ratio，supervisor_openness，retaliated_this_step，issue_salience を含む構造化されたペルソナ＋文脈のプロンプトを組み立て，モデルに1行 JSON オブジェクトでの応答を求めます．

```json
{"decision": "VOICE"|"SILENCE", "motive": "acquiescent"|"defensive"|"prosocial"|null, "rationale": "..."}
```

パーサは大文字小文字に寛容で，パース失敗時には (`Silence`, `Defensive`) にフォールバックするため，1つの不適切な呼び出しが実行全体を中断することはありません．

ルールと LLM の切り替えは，本パックの主要なアブレーションの軸です．シナリオ TOML 内のメカニズム名を1つ変えるだけです（`voice_decision_rule` ↔ `voice_decision`）．設計上の注意点も同様に残ります．モデルは ψ プロンプトの文言に敏感であり，毎ティック追加で N 回の LLM 呼び出しが発生し（エージェント1名につき1回，キャッシュヒットを除く），キャッシュヒット率はプロンプト文脈の離散化粒度に依存します．本番スイープには Ollama ローカル，検証サブセットにはフロンティアモデル実行を推奨します．

## 4. パイプラインとメトリクス

モデルの1ティックは標準の [6フェーズループ](../architecture.ja.md#6フェーズティックループ)を辿ります．スターターシナリオは10個のメカニズムを次のように配置します．

- **Environment** — `issue_salience` が σ(t) を更新．`retaliation_event` が発火しうる場合は `retaliation_this_step` バッファを準備．
- **Decision** — `fear_appraisal` が報復バッファを読んで fear を更新し，続いて `voice_decision_rule`（LLM フィーチャ下では `voice_decision`）が `ctx.agent_order` 順に各エージェントについて Bernoulli(p) を引き，silence 時には動機を割り当てる．
- **Interaction** — `silence_spiral` が ρ_i をスナップショットして ψ を押し下げ，続けて `prefalse_cascade` が `private_concern < 0` の silent エージェント上で不動点まで実行．
- **Reward** — `org_performance` が集約を再計算し，全メトリクスとそのステップの `motive_mix` イベントを記録．
- **PostStep** — `psafety_update` が voice／報復経験から ψ を調整し，`climate_silence` が C(t) を再公開し，`org_learning` が知識の増分または減衰を適用．

`org_performance` は毎ステップ7個のメトリクスを記録し，3種類の名前付きイベントを発火します．

| メトリクス／イベント | 意味 |
|---|---|
| `silence_rate` | 現在 `Silence` である従業員の割合． |
| `climate_of_silence` | C(t)：`Silence` ∧ `private_concern < 0` の割合． |
| `voice_volume` | V(t)：現在 `Voice` の割合． |
| `ever_silent_fraction` | 実行中に少なくとも1回 `Silence` を経験したエージェントの累積割合（ステップごとの `silence_rate` 系列から計算）． |
| `knowledge_stock` | 全チームにわたる `team.knowledge_stock` の合計． |
| `org_performance` | Π(t) = `knowledge_stock` · (1 − C(t))． |
| `opinion_clusters` | 許容値 `cluster_tol`（デフォルト 0.05）以内の別個な `private_concern` クラスタ数． |
| `n_employees` | 現在の在職人員数． |
| `retaliation`（event） | `retaliation_event` が発火．ペイロード `{target, n_affected}`． |
| `cascade`（event） | 反転総数が `cascade_threshold` を超えたときに `prefalse_cascade` が発火．ペイロード `{size, fraction}`． |
| `motive_mix`（event） | 毎ステップ `org_performance` が発火．現在 silent なエージェントに対するペイロード `{acquiescent, defensive, prosocial, no_motive}`． |

PR #52 以降，これらのイベントはステップごとのメトリクスと並んで JSONL 実行ログ（`type:"event"` レコード）にも現れるため，単一の `*.jsonl` ファイルが実行全体の再現可能な記録になります．

## 5. キャリブレーションアンカー

主要な経験的アンカーは [`crates/socsim-packs/src/organizational_silence/calibration.rs`](../../crates/socsim-packs/src/organizational_silence/calibration.rs) に `pub const` として置かれ，インライン引用が添えられています．これらは voice 決定のロジスティック係数，エージェントごとの初期状態の事前分布スケール，更新率，そしてベースライン実行が再現すべき2つの*キャリブレーションターゲット*に分けられます．

| アンカー | 値 | 出典／役割 |
|---|---|---|
| `BETA_PSAFETY` | `1.2` | Edmondson (1999) — ψ → voice 係数 |
| `BETA_FEAR` | `1.5` | Kish-Gephart et al. (2009) — fear → silence（減算） |
| `BETA_IVT` | `0.8` | Detert & Edmondson (2011) — 暗黙の発言観 → silence |
| `BETA_SUP` | `1.0` | Detert & Burris (2007) / Morrison (2014) — 上司開放性 → voice |
| `BETA_SALIENCE` | `1.0` | Morrison (2014) — salience → voice |
| `BETA_CLIMATE` | `1.5` | Noelle-Neumann (1974) / Sohn (2022) — 沈黙のスパイラル（減算） |
| `BETA_0` | `-0.5` | calibration scale — 平均的なエージェントが軽く silent へ偏るよう調整した切片 |
| `F_MEAN`, `F_SD` | `0.4`, `0.2` | Kish-Gephart et al. (2009) — 初期 fear 事前分布 `N(0.4, 0.2)` |
| `PSAFETY_MEAN`, `PSAFETY_SD` | `0.5`, `0.2` | Edmondson (1999) — 初期 ψ 事前分布 |
| `THETA_VOICE_MEAN`, `THETA_VOICE_SD` | `0.4`, `0.15` | Kuran (1995) — voice 閾値の事前分布 |
| `P_RETALIATE` | `0.05` | Kish-Gephart et al. (2009) — ステップごとの報復確率 |
| `FEAR_SENSITIVITY` | `0.4` | calibration scale — 報復ごとの fear バンプ |
| `EPSILON_SPIRAL` | `0.25` | Noelle-Neumann (1974) — スパイラル知覚の大きさ |
| `PSAFETY_LEARN` | `0.1` | Edmondson (1999) — ψ 学習率 |
| `EVER_SILENT_TARGET` | `0.85` | Milliken, Morrison & Hewlin (2003) — **ターゲット**：6か月窓で1つ以上の課題に沈黙 |
| `HICO_SILENCE_TARGET` | `0.50` | Detert & Edmondson (2011) — **ターゲット**：HiCo（IVT 高位）従業員の沈黙率 |

「calibration scale」と注記された定数は，上記2つの経験的ターゲットを再現するために選んだチューナブルなノブです．socsim パック全般の経験的／チューナブル分割の考え方は[アーキテクチャページ](../architecture.ja.md#キャリブレーション哲学)を参照してください．

## 6. 適用方法

### シナリオ / CLI

スターターシナリオを生成して実行します．

```sh
socsim init --module-pack organizational-silence --out scenarios/os.toml
socsim run scenarios/os.toml
```

同梱の `scenarios/org_silence_baseline.toml` は，5チーム × 8従業員 × 3階層の組織を60ステップ（月）にわたって実行し，月24で salience ショックを発火させるルールベースの voice 決定です．

```toml
[simulation]
name        = "org_silence_baseline"
module_pack = "organizational-silence"
t_max       = 60
seed        = 42
scheduler   = "random_activation"

[world]
n_teams                = 5
team_size_initial      = 8
n_levels               = 3
network_model          = "watts_strogatz"
network_k              = 6
network_beta           = 0.1
supervisor_homogeneity = 0.5

[[mechanism]]
name  = "issue_salience"
phase = "environment"
[mechanism.params]
sigma_base  = 0.3
shock_t     = 24
shock_delta = 0.4

[[mechanism]]
name  = "retaliation_event"
phase = "environment"
[mechanism.params]
p_retaliate = 0.05            # kish-gephart:2009

[[mechanism]]
name  = "fear_appraisal"
phase = "decision"
[mechanism.params]
fear_sensitivity = 0.4

[[mechanism]]
name  = "voice_decision_rule"
phase = "decision"
[mechanism.params]
beta_0        = -0.5
beta_psafety  = 1.2           # edmondson:1999
beta_fear     = 1.5           # kish-gephart:2009
beta_ivt      = 0.8           # detert:2011
beta_sup      = 1.0
beta_salience = 1.0
beta_climate  = 1.5           # noelle-neumann:1974

[[mechanism]]
name  = "silence_spiral"
phase = "interaction"
[mechanism.params]
epsilon = 0.25                # noelle-neumann:1974

[[mechanism]]
name  = "prefalse_cascade"
phase = "interaction"
[mechanism.params]
cascade_threshold = 0.05      # kuran:1995

[[mechanism]]
name  = "org_performance"
phase = "reward"
[mechanism.params]
cluster_tol = 0.05

[[mechanism]]
name  = "psafety_update"
phase = "post_step"
[mechanism.params]
psafety_learn = 0.1           # edmondson:1999

[[mechanism]]
name  = "climate_silence"
phase = "post_step"

[[mechanism]]
name  = "org_learning"
phase = "post_step"
[mechanism.params]
learning_rate  = 0.05
decay_rate     = 0.01
salience_floor = 0.3

[output]
log_path = "runs/{name}_{seed}.jsonl"
metrics  = ["silence_rate", "climate_of_silence", "voice_volume", "knowledge_stock", "org_performance", "opinion_clusters"]
```

LLM 変種に切り替えるには，同梱の LLM シナリオを実行し，LLM フィーチャを有効にして CLI を再ビルドします．

```sh
cargo build --release -p socsim-cli --features pack-organizational-silence-llm
socsim run scenarios/org_silence_llm.toml
```

LLM シナリオは同じワールドとメカニズムスタックを使用しますが，`voice_decision_rule` を `voice_decision` に置き換え，`temperature = 0` と `cache_path` を持つ `[llm]` ブロックを追加してウォームキャッシュからの再現性を確保します．これによりパラダイム横断比較は，同一シードでの2シナリオの差分にすぎなくなります．

### ライブラリ

```rust
use socsim_config::{ModulePack, Params, Registry};
use socsim_core::{SimClock, SimRng};
use socsim_engine::{RandomActivationScheduler, SimulationBuilder};
use socsim_packs::organizational_silence::{
    OrganizationalSilencePack, SilenceWorld,
};

let mut rng = SimRng::from_seed(42);
let mut world = SilenceWorld::new(5, 8, 3, 6, 0.1, 0.5, &mut rng);
world.clock = SimClock::new(60);

let mut reg: Registry<SilenceWorld> = Registry::new();
OrganizationalSilencePack.register(&mut reg);

let mut builder = SimulationBuilder::new(world)
    .scheduler(Box::new(RandomActivationScheduler))
    .seed(42);
for name in [
    "issue_salience", "retaliation_event",
    "fear_appraisal", "voice_decision_rule",
    "silence_spiral", "prefalse_cascade",
    "org_performance",
    "psafety_update", "climate_silence", "org_learning",
] {
    builder = builder.add_mechanism(reg.build(name, &Params::empty())?);
}
let mut sim = builder.build();
sim.run()?;
```

ソースから LLM 変種をビルドする場合はフィーチャを明示してください．

```sh
cargo run -p socsim-cli --features pack-organizational-silence-llm \
    -- run scenarios/org_silence_llm.toml
```

## 7. 関連項目

- [Mechanism カタログ](../mechanisms.ja.md) — スパイラル／カスケード／fear-appraisal メカニズムの隣に位置する，より広いカタログ．
- [hr-lifecycle パック](hr-lifecycle.ja.md) — もう1つの組織系パック（労働力進化，10個の較正済みメカニズム）．
- [opinion-dynamics パック](opinion-dynamics.ja.md) — 「ネットワーク上の創発」を扱う兄弟パック．
- [T5 — シナリオパック](../tutorials/05-scenario-pack.ja.md) — パックをゼロから構築．
- [ユースケース＆レシピ](../usecases.ja.md) · [CLI リファレンス](../cli.ja.md) · [アーキテクチャ](../architecture.ja.md)
