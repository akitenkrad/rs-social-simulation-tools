[English](voice-decision-rule.md) | **日本語**

# Voice 決定（ルールおよび LLM）（`voice_decision_rule`，`voice_decision`）

> 各エージェントは，心理的安全性，上司開放性，課題顕在性，fear，
> 暗黙の発言観，隣接沈黙比をブレンドした較正済みロジスティックを介して
> Voice か Silence かを決定します．Silence の場合，さらに Van Dyne, Ang & Botero (2003) に従って
> Acquiescent，Defensive，Prosocial の3つの動機のうち1つを割り当てます．
> フィーチャゲートされた LLM 兄弟（`voice_decision`）はロジスティックを構造化 JSON プロンプトで置き換え，
> パース失敗時にはルール側のデフォルトにフォールバックします．
> **フェーズ：** Decision．**出典：** Noelle-Neumann (1974)；Van Dyne et al. (2003)；Detert & Edmondson (2011)；Kuran (1995)；Edmondson (1999)．**種別：** mixed（経験的係数＋理論構造）．

[← Mechanism カタログに戻る](../mechanisms.ja.md)

## 1. 概要

`voice_decision_rule` は組織的沈黙パックの中心となるメカニズムです．
モデルが voice の予測子として扱うすべてのエージェント／チームごとのスカラを消費し，
それらを単一のロジスティックに通し，エージェントごとにベルヌーイを1回引き，
結果の `Expression::Voice` または `Expression::Silence` を各従業員に書き込みます．
Silence 抽出時には，さらにエージェントを3つの silence 動機のいずれかに分類します — 
Van Dyne, Ang & Botero (2003) の類型 — これにより `org_performance` の下流の `motive_mix` イベントが
時間にわたる silence の構成を追跡できます．

ルールの構造は設計書の §4.3 決定式を反映していますが，実装は2点で設計と乖離しており，本ページが
それを公式に文書化します．

1. **Prosocial 動機**は強度スコアではなく状態述語
   （`supervisor_openness > 0` AND `private_concern < 0`）で検出されます．
   Prosocial silence とは「上司は開かれているのに，他者を守るために批判的見解を控える」状況です — 
   連続量の最大値ではなく，特徴づけられた状況です．
2. `BETA_CLIMATE` 項は **エージェントごとのスナップショット**
   `Employee.neighbor_silence_ratio` から $\rho_i$ を読み取り，これは前ステップの
   Interaction フェーズ末で `silence_spiral` が書き込んだものです．
   このスナップショットが，ティック内のメカニズムスタックを首尾一貫した複数ステップのスパイラルに変換する運搬役です．
   フィールドは `pub(crate)` なので，外部コードが誤って同期を崩すことはできません．

正準名 `voice_decision` の下に登録された2つ目のメカニズムが，差し替え可能な LLM 代替を提供します．
書き込みコントラクトは共有しますが，ロジスティックを構造化プロンプトで置き換えます — §2.1 を参照．

## 2. 理論と出典

ルールは6つの予測子を単一のロジットに混合し，
$p = \operatorname{logistic}(\text{logit})$ のベルヌーイ$(p)$ を抽出し，Silence の場合は小さな分類器を回します．

$$\text{logit}_i = \beta_0 + \beta_\psi \cdot \psi_i + \beta_u \cdot u_{k(i)} + \beta_\sigma \cdot \sigma - \beta_f \cdot f_i - \beta_\iota \cdot \iota_i - \beta_C \cdot \rho_i$$

$$p_i = \operatorname{logistic}(\text{logit}_i) = \frac{1}{1 + e^{-\text{logit}_i}}, \qquad X_i \sim \operatorname{Bernoulli}(p_i)$$

$$\text{Expression}_i = \begin{cases} \text{Voice} & X_i = 1 \\ \text{Silence} & X_i = 0 \end{cases}$$

予測子とその引用：

- $\psi_i$（`Employee.psych_safety`，Edmondson 1999）— 知覚された心理的安全性．
  正符号：安全性が高いほど voice の確率が上昇．
- $u_{k(i)}$（`Team.supervisor_openness`，Detert & Burris 2007 / Morrison 2014）— 上司の開放性シグナル．
  正符号：開かれた上司は voice の確率を上げる．
- $\sigma$（`SilenceWorld.issue_salience`，Morrison 2014）— 現在の課題顕在性．
  正符号：顕在性の高い課題は沈黙を保ちにくい．
- $f_i$（`Employee.fear`，Kish-Gephart et al. 2009）— 発言への恐怖．
  **減算**：fear が高いほど voice の確率が低下．
- $\iota_i$（`Employee.ivt_strength`，Detert & Edmondson 2011）— 暗黙の発言観の強さ．
  **減算**：「発言は危険」を内面化したエージェントは状況的 fear とは独立に沈黙する．
- $\rho_i$（`Employee.neighbor_silence_ratio`，Noelle-Neumann 1974）— *前*ステップの Interaction フェーズで
  取得したエージェントの局所沈黙比のスナップショット．
  **減算**：沈黙した局所多数派はエージェントの voice 意欲を侵食する — 沈黙のスパイラル．

デフォルト係数は [`calibration.rs`](../../crates/socsim-packs/src/organizational_silence/calibration.rs) にあり，
パックページの[キャリブレーションアンカー](../packs/organizational-silence.ja.md#5-キャリブレーションアンカー)表に要約されています．

### Silence 時の動機分類

$X_i = 0$ のとき，メカニズムは3つの抑制要因を固定順序でランク付けして `silence_motive` を割り当てます．
分類器はユニットテスト可能なように純粋関数（`classify_motive`）です．

1. **まず Prosocial silence．** `supervisor_openness > 0` *かつ* `private_concern < 0` であれば，
   動機 = `Prosocial`．これは Van Dyne et al. (2003) の「保護的留保」のケースです：
   エージェントは批判的見解を持ち*かつ*上司は聞く用意があるにもかかわらず沈黙する — 
   個人的脅威ではなく他者を守るための留保と解釈されます．
2. **それ以外は，fear と IVT の比較．同点は fear に倒れる．** $f_i \ge \iota_i$ であれば，
   動機 = `Defensive`（fear 主導）．それ以外は動機 = `Acquiescent`（「どうせ変わらない」— 諦め主導）．

順序が重要です．fear と IVT が共に高く，上司が開放的で批判的懸念を持つエージェントは，
`Defensive` ではなく `Prosocial` に分類されます — 状況的述語が強度比較より優先されます．

### 2.1 LLM 変種（`voice_decision`）

パックは正準名 `voice_decision` の下にフィーチャゲートされた LLM 変種を同梱します．
両方のメカニズムは単一のレジストリ内で共存でき，シナリオ TOML が名前で選択します．
ソース：[`crates/socsim-packs/src/organizational_silence/mechanisms_llm.rs`](../../crates/socsim-packs/src/organizational_silence/mechanisms_llm.rs)．

LLM 変種は次のフィーチャでゲートされます．

- `socsim-packs` クレートの `organizational-silence-llm` フィーチャ
- `socsim-cli` クレートの `pack-organizational-silence-llm` フィーチャ

どちらかが有効な場合，パックの `register` メソッドが `voice_decision_rule` と並んで
`voice_decision` メカニズムを追加で挿入します．

`ctx.agent_order` の各エージェントについて，メカニズムは次を行います．

1. **プロンプトを構築**：エージェントの level，tenure，fear，psych_safety，ivt_strength，
   隣接沈黙比，上司開放性，`retaliated_this_step` フラグ，現在の $\sigma$ から構築．
   プロンプトはモデルに1行 JSON オブジェクトを返すよう要求します．
2. **`LiveClient::complete` を呼ぶ**：決定論的な `LlmConfig`（temperature 0，シナリオ seed）で呼び出します．
   live client はパックの `LlmSettings` から組み立てられ，ローカル Ollama → OpenAI フォールバックへ
   ルーティングされ，キャッシュヒットは JSON ファイルバックの `PromptCache` から提供されます．
3. **応答をパース**：`serde_json` で `{decision, motive, rationale}` 形に変換．
   パーサは大文字小文字に寛容で，パース失敗時には `(Silence, Defensive)` にフォールバックするため，
   1つの不適切な応答が実行を中断することはありません．

応答の形：

```json
{"decision": "VOICE"|"SILENCE", "motive": "acquiescent"|"defensive"|"prosocial"|null, "rationale": "..."}
```

ウォームな `PromptCache` は LLM 変種を決定論的なオラクルに変えます — 再実行はキャッシュにヒットし
ネットワークラウンドトリップなしに返ります．テストは `from_client` コンストラクタを介して
`socsim_llm::mock::ScriptedClient` を注入できます．ルール変種と同じ書き込みコントラクトが適用されます：
Voice の場合は動機がクリア（`None`）され，Silence の場合はパース済みの動機が書き込まれます．

## 3. データフロー

ルールは各従業員から $\psi$，$f$，$\iota$，`team`，および前ステップの $\rho$ スナップショットを読み取り，
さらに `Team.supervisor_openness` と `SilenceWorld.issue_salience` を読み取ります．
従業員を変更する前にチーム開放性を `Vec<f64>` にスナップショットし（借用チェッカーの要件），
次に `ctx.agent_order` を反復してロジットを計算し，ベルヌーイを抽出し，
新しい `Expression` を書き込みます — Silence の場合は `silence_motive` も書き込みます．
LLM 変種は同じ I/O 形状を持ちますが，エージェントごとにロジットを評価する代わりに
`LiveClient::complete` を呼び出します．

## 4. 6フェーズループにおける位置

3番目のフェーズである **Decision** で実行されます．Decision 内では同梱シナリオが
`fear_appraisal` を先に，`voice_decision_rule`（または `voice_decision`）を次に宣言するため，
ロジットが $f_i$ を読む時点でエージェントの fear はこのステップの報復バッファから更新済みです．
上司開放性項と $\sigma$ 項はワールドフィールドから読み取るので，
それらに書き込む他の Decision メカニズムも先に走る必要がありますが，
同梱シナリオでは Environment 以降，両者とも触れられません．

このメカニズムは silence 評価スタック（顕在性，報復，fear，IVT，$\rho$）と
ステップ内 Interaction フェーズ（`silence_spiral`，`prefalse_cascade`）との主要な受け渡し点です．
どちらも本メカニズムが書き込む `Expression` と `silence_motive` を読み取ります．

## 5. 状態の読み書きコントラクト

| フィールド | 読み取り | 書き込み | 備考 |
|---|:--:|:--:|---|
| `ctx.agent_order` | ✓ | | 決定論的反復順序．id ごとに RNG 抽出を1回ずつ実行． |
| `ctx.rng` | ✓ | | エージェントごとに `gen::<f64>()` のベルヌーイ抽出を1回行う． |
| `SilenceWorld.issue_salience` | ✓ | | ロジット内の $\sigma$ として読む． |
| `Team.supervisor_openness` | ✓ | | 従業員を変更する前に `Vec<f64>` にスナップショット． |
| `Employee.psych_safety` | ✓ | | ロジット内の $\psi_i$． |
| `Employee.fear` | ✓ | | ロジット内の $f_i$（減算）． |
| `Employee.ivt_strength` | ✓ | | ロジット内の $\iota_i$（減算）． |
| `Employee.team` | ✓ | | 開放性スナップショットへのインデックス． |
| `Employee.neighbor_silence_ratio` | ✓ | | ロジット内の $\rho_i$（減算）．前ステップの `silence_spiral` が設定． |
| `Employee.private_concern` | ✓ | | `classify_motive` が Prosocial 分岐で使用． |
| `Employee.expression` | | ✓ | `Voice` または `Silence` に設定． |
| `Employee.silence_motive` | | ✓ | Voice 時はクリア．Silence 時は `classify_motive` が設定． |

## 6. 依存関係と順序制約

- **上流（同ステップ）：**
  - `fear_appraisal`（Decision）は本メカニズムより先に実行されている必要があります．
    $f_i$ がこのステップの報復バッファから新鮮であるためです．
  - `issue_salience`（Environment）は Decision 開始前に $\sigma$ を書き込みます．
- **上流（前ステップ）：**
  - `silence_spiral`（Interaction）は前ステップ末で `Employee.neighbor_silence_ratio` を書き込みました — 
    このステップのロジットへスパイラル効果を運ぶ運搬役です．
- **下流（同ステップ）：**
  - `silence_spiral`（Interaction）は次ステップの $\rho$ スナップショット計算のために
    新たに書き込まれた各エージェントの `Expression` を読みます．
  - `prefalse_cascade`（Interaction）は `Expression == Silence` かつ `private_concern < 0` のエージェントのみを反転します．
    両方とも本メカニズムが書き込みます．
  - `org_performance`（Reward）は `motive_mix` イベントのために `silence_motive` を集計します．

## 7. パラメータ

| パラメータキー | デフォルト | 種別 | 出典 |
|---|---|---|---|
| `beta_0` | `-0.5` | calibration scale（チューナブル） | calibration — わずかに負の切片 |
| `beta_psafety` | `1.2` | empirical | Edmondson (1999) |
| `beta_fear` | `1.5` | empirical | Kish-Gephart et al. (2009) |
| `beta_ivt` | `0.8` | empirical | Detert & Edmondson (2011) |
| `beta_sup` | `1.0` | empirical | Detert & Burris (2007) / Morrison (2014) |
| `beta_salience` | `1.0` | empirical | Morrison (2014) |
| `beta_climate` | `1.5` | empirical | Noelle-Neumann (1974) / Sohn (2022) |

デフォルトは [`calibration.rs`](../../crates/socsim-packs/src/organizational_silence/calibration.rs) に
`BETA_0`，`BETA_PSAFETY`，`BETA_FEAR`，`BETA_IVT`，`BETA_SUP`，
`BETA_SALIENCE`，`BETA_CLIMATE` として存在します．

LLM 変種 `voice_decision` は別のパラメータ集合を認識します．

| パラメータキー | デフォルト | 種別 | 出典 |
|---|---|---|---|
| `cache_path` | `"runs/silence_cache.json"` | path | LLM プロンプトキャッシュファイル |
| `seed` | `42` | u64 | `LlmConfig` に渡す |
| `temperature` | `0.0` | f64 | `LlmConfig` に渡す |

## 8. 適用方法

### シナリオ TOML — ルール

```toml
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
```

### シナリオ TOML — LLM 変種

LLM フィーチャを有効にして CLI をビルドし，メカニズム名を `voice_decision` に切り替えます．

```sh
cargo build --release -p socsim-cli --features pack-organizational-silence-llm
```

```toml
[llm]
decision_mode = "llm"
temperature   = 0.0
seed          = 42
cache_path    = "runs/silence_cache.json"

[[mechanism]]
name  = "voice_decision"
phase = "decision"
[mechanism.params]
cache_path = "runs/silence_cache.json"
seed       = 42
```

### ライブラリモード

```rust
use socsim_config::{Registry, Params, ModulePack};
use socsim_packs::organizational_silence::{OrganizationalSilencePack, SilenceWorld};
use socsim_engine::{RandomActivationScheduler, SimulationBuilder};

let mut reg: Registry<SilenceWorld> = Registry::new();
OrganizationalSilencePack.register(&mut reg);

let m = reg.build("voice_decision_rule", &Params::empty())?;
let mut sim = SimulationBuilder::new(world)
    .scheduler(Box::new(RandomActivationScheduler))
    .seed(42)
    .add_mechanism(m)
    .build();
sim.run()?;
```

## 9. 決定論性と RNG

ルールは `ctx.agent_order`（スケジューラから提供される順序．それ自体がシードされている）を反復し，
`ctx.rng.gen::<f64>()` でエージェントごとに正確に1回ベルヌーイを抽出します．
反復順序が固定であり，チーム開放性の読み取りスナップショットが変更前に行われるため，
同じワールド状態とシードに対する2つの実行はビット同一の `Expression` / `silence_motive` ベクトルを生成します．

LLM 変種は，(a) `temperature = 0` とウォームな `PromptCache`（毎回キャッシュにヒットし同じ応答を返す），または
(b) テスト時の `ScriptedClient` のもとで決定論的です．ウォームキャッシュなしのライブ実行は，
バックエンドが `temperature = 0` を尊重するかに依存します．
キャッシュヒット率は `MetadataCollector::cache_hit_rate()` で報告されるため，
研究者は出力を再現可能と見なす前に実行がキャッシュから提供されたことを検証できます．

## 10. 期待される動作

ベースラインシナリオ（シード 0，60 ステップ）でルールは次を生成します．

- 実行全体で silence rate は 0.20〜0.55．voice volume はその補数．
- ショック前の climate of silence は 5〜10 % 程度に落ち着き，$\sigma$ ショックが $t = 24$ で発火した後，
  境界エージェントが Voice 側へ傾くことでさらに下がる．
- 動機構成は `Acquiescent` が支配的（fear も IVT も高くない場合，最も一般的な silence は
  「どうせ変わらない」）．報復下では `Defensive` コホートが小規模に存在し，
  開かれた上司と批判的私的懸念が共存するときに Prosocial silence が時折現れる．
- 多くのステップでカスケードイベントが発火（ルールは silence-with-critical-concern エージェントを
  十分に生み出すため，カスケードの 5 % 反転質量閾値を超えやすい）．

フロンティアモデルでの `voice_decision`（LLM）への切り替えは，定性的に類似した軌跡を生成しますが，
動機構成はモデルが最も口に出しやすい動機側に偏ります — 通常は高 `fear` で `Defensive`，
高 `ivt_strength` で `Acquiescent`．ルールは較正済みベースラインのままであり，LLM 変種は比較対象です．

## 11. 参考文献

- Detert, J. R., & Burris, E. R. (2007). Leadership behavior and employee
  voice: Is the door really open? *Academy of Management Journal*, 50(4),
  869–884.
- Detert, J. R., & Edmondson, A. C. (2011). Implicit voice theories:
  Taken-for-granted rules of self-censorship at work. *Academy of Management
  Journal*, 54(3), 461–488.
- Edmondson, A. C. (1999). Psychological safety and learning behavior in
  work teams. *Administrative Science Quarterly*, 44(2), 350–383.
- Kish-Gephart, J. J., Detert, J. R., Treviño, L. K., & Edmondson, A. C.
  (2009). Silenced by fear: The nature, sources, and consequences of fear at
  work. *Research in Organizational Behavior*, 29, 163–193.
- Kuran, T. (1995). *Private Truths, Public Lies: The Social Consequences
  of Preference Falsification*. Harvard University Press.
- Morrison, E. W. (2014). Employee voice and silence. *Annual Review of
  Organizational Psychology and Organizational Behavior*, 1(1), 173–197.
- Noelle-Neumann, E. (1974). The spiral of silence: A theory of public
  opinion. *Journal of Communication*, 24(2), 43–51.
- Van Dyne, L., Ang, S., & Botero, I. C. (2003). Conceptualizing employee
  silence and employee voice as multidimensional constructs. *Journal of
  Management Studies*, 40(6), 1359–1392.
