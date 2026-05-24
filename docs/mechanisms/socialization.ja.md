[English](socialization.md) | **日本語**

# 社会化 (`socialization`)

> 新規採用者はランダムなサポート抽出を受け取り，個人–組織適合度と組み合わせることで初期社会化スコアが決まり，組織埋め込み度に早期ブーストが与えられます．
> **フェーズ:** PostStep．**出典:** オンボーディングモデル（キャリブレーション）．**種別:** キャリブレーション．

[← Mechanism カタログに戻る](../mechanisms.ja.md)

## 1. 概要

`socialization` は同一ステップで採用された従業員のオンボーディングメカニズムです．すべてのDecisionフェーズおよびInteractionフェーズのメカニズムが終了した後，**PostStep** で実行され，同ステップで `hiring` が生成した `new_hires_this_step` リスト内のすべてのエージェントを処理します．

各新規採用者についてランダムな組織サポートレベルを抽出し，採用者の個人–組織適合度とブレンドして `[0, 1]` の `socialization` スコアを生成し，そのスコアを使って `embeddedness` に小さな上方ナッジを与えます．すべての新規採用者を処理したら，メカニズムは `new_hires_this_step` をクリアし，次のステップに向けて空にします．

このメカニズムは意図的にパラメータフリーです．固定係数（0.5/0.5ブレンド，0.1の埋め込み度増分，`U[0.4, 1.0)` のサポート範囲）は，キャリア初期のサポートが組織や役割によって異なるものの常に少なくとも適度にポジティブであり，1ヶ月のオンボーディングだけでも埋め込み度が控えめで有界な量だけ動くという仮定を符号化しています．

## 2. 理論と出典

社会化の計算式は，内的適合度と受け取ったサポートという2つの成分を統合インデックスにブレンドします：

$$\text{support} \sim \mathcal{U}[0.4, 1.0)$$

$$\text{socialization} = \operatorname{clip}_{[0,1]}\!\left(0.5\,\text{po\_fit} + 0.5\,\text{support}\right)$$

$$\text{embeddedness} \leftarrow \operatorname{clip}_{[0,1]}\!\left(\text{embeddedness} + 0.1\,\text{socialization}\right)$$

- `po_fit` — 新規採用者の個人–組織適合度．構築時に割り当てられ固定されます．$\text{po\_fit}$ が高い従業員ほど早く統合されます．
- $\text{support}$ — $\mathcal{U}[0.4, 1.0)$ の一様抽出．下限0.4は組織が常に少なくとも何らかの基準オンボーディングを提供するという仮定を反映し，上限1.0未満はサポートが完璧になることはないことを意味します．
- 等重みブレンド（0.5/0.5）により，2つの成分が社会化に対称的な影響を持ちます．
- 0.1の埋め込み度増分は，1ステップあたりの小さなナッジで，単一の大きな初期化ではなく，シミュレーションの自然なダイナミクス（ネットワーク成長，在職期間など）を通じた後続ステップで累積します．
- すべての値は有効範囲に収めるため $[0, 1]$ にクランプされます．

この特定の関数形式に対する公表された引用文献はありません；これは `turnover` のロジスティック離職モデルとWatts–Strogazネットワークと組み合わせたときに現実的なオンボーディングダイナミクスを生み出すように設計されたキャリブレーション上の選択です．

## 3. データフロー

![socialization data flow](../assets/mech-socialization.svg)

`socialization` は `new_hires_this_step`（`hiring` が生成）と各新規採用者の `po_fit` および `embeddedness` を読み取ります．`socialization` と増分された `embeddedness` を書き戻し，その後 `new_hires_this_step` をクリアします．

## 4. 6フェーズループにおける位置

第6フェーズかつ最終フェーズである **PostStep** で実行されます．これにより以下が保証されます：

1. `hiring`（Decision）が新規従業員を挿入し `new_hires_this_step` を生成してから `socialization` がそれを読み取ります．
2. Interactionフェーズのメカニズム（`peer_effect`，`ocb`，`toxic_spread`）が既存のロスターで実行済みです．新規採用者は最初のステップのInteractionには参加しません——まず社会化を受け取り，次のステップ以降から完全なInteractionに参加します．
3. `knowledge_loss` も PostStep で実行されます．両方がアクティブな場合，それらは互いに素な状態（`new_hires_this_step` 対 `departed_this_step`）に作用するため，両者の順序は問いません．

## 5. 状態読み書きコントラクト

| フィールド | 読み取り | 書き込み | 備考 |
|---|:--:|:--:|---|
| `HrWorld.new_hires_this_step` | ✓ | ✓ | 新規採用者のイテレーションに使用；終了時にクリアされます． |
| `Employee.po_fit` | ✓ | | 個人–組織適合度，採用時に固定． |
| `Employee.embeddedness` | ✓ | ✓ | `0.1 · socialization` だけインクリメントされ，`[0, 1]` にクランプ． |
| `Employee.socialization` | | ✓ | `clamp01(0.5·po_fit + 0.5·support)` に設定． |

## 6. 依存関係と順序制約

**必ず後に実行すべきもの：**
- `hiring`（Decision）——`hiring` が `new_hires_this_step` を生成します；これがないとリストは空で `socialization` はno-opになります．実際には，`hiring` が登録されていない場合 `new_hires_this_step` は生成されないため，`socialization` は安全に省略できます．

**同ステップ内に下流の依存先はありません．** 更新された `socialization` と `embeddedness` の値は*次の*ステップで `turnover` と `fit` によって初めて読み取られます．

**共有状態の引き継ぎ：**

| 生産者 | フィールド | 消費者 |
|---|---|---|
| `hiring` | `new_hires_this_step` | `socialization` |
| `socialization` | `new_hires_this_step` のクリア | （次ステップ用のクリーンな状態） |
| `socialization` | `Employee.embeddedness` | `turnover`（次ステップ） |
| `socialization` | `Employee.socialization` | `fit`（次ステップ，間接的） |

## 7. パラメータ

`socialization` には**設定可能なパラメータがありません**．すべての係数はコンパイル定数です：

| 定数 | 値 | 役割 |
|---|---|---|
| サポート下限 | `0.4` | 最小組織サポート |
| サポート上限 | `1.0`（排他） | 最大組織サポート |
| 適合度重み | `0.5` | サポートとの等重みブレンド |
| サポート重み | `0.5` | 適合度との等重みブレンド |
| 埋め込み度増分 | `0.1` | 社会化でスケールされたステップあたりのナッジ |

## 8. 使い方

### シナリオTOML

```toml
[[mechanism]]
name  = "hiring"
phase = "decision"
[mechanism.params]
rho_si  = 0.51
p_toxic = 0.04

[[mechanism]]
name  = "socialization"
phase = "post_step"
```

`socialization` は `[mechanism.params]` ブロックを必要としません．TOML内で `hiring` の後に記述する必要がありますが，両者は異なるフェーズで実行されるため，順序制約はエンジンによって自動的に守られます．

### ライブラリモード

```rust
use socsim_config::{Registry, Params, ModulePack};
use socsim_hr_lifecycle::{HrLifecyclePack, HrWorld};
use socsim_engine::{RandomActivationScheduler, SimulationBuilder};

let mut reg: Registry<HrWorld> = Registry::new();
HrLifecyclePack.register(&mut reg);

let socialization = reg.build("socialization", &Params::empty())?;
let mut sim = SimulationBuilder::new(world)
    .scheduler(Box::new(RandomActivationScheduler))
    .seed(42)
    .add_mechanism(socialization)
    .build();
sim.run()?;
```

## 9. 決定論性とRNG

`socialization` は `ctx.rng` から引数を取得します——新規採用者1人につき1回の `gen_range(0.4..1.0_f64)` 呼び出しです．ステップあたりの新規採用者数は `hiring` によって決まり（それ自体が与えられたシードに対して決定論的），`new_hires_this_step` は順序付きリストなので，同じシードによる実行では同じシーケンスでサポート抽出が再現されます．

## 10. 期待される動作

ベースラインシナリオ（`hiring` と `turnover` がアクティブ）では：

- 各新規採用者は `po_fit` に依存したフロアとランダムなサポートブーストで `socialization` を開始します．$\text{po\_fit} = 0.8$，$\text{support} = 0.7$ の採用者は $\text{socialization} = \operatorname{clip}_{[0,1]}(0.5 \times 0.8 + 0.5 \times 0.7) = 0.75$ を受け取ります．
- 対応する `embeddedness` バンプ $0.1 \times 0.75 = 0.075$ は小さいながらも意味があります：*次の*ステップで `turnover` における離職確率を約 $0.075 \times \text{quit\_embed\_sens}$ ロジット単位だけ低下させ，即時再離職の可能性を減らします．
- `socialization` がない場合，新規採用者は `embeddedness = 0` から始まり，1ヶ月目の離職確率が著しく高くなり，離職率に非現実的な「入社初日後悔」スパイクが発生します．
- サポート範囲の変更（例：低サポート組織向けの `[0.1, 0.5)`）にはコードレベルの定数変更が必要です；これはパラメータフリー設計の既知の制限です．

## 11. 参考文献

外部引用なし．関数形式は socsim-hr-lifecycle モデル内部のキャリブレーション上の選択です．
