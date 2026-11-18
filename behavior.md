# **Surv Behavior IR — 要件定義 v0.1 Draft**

Proposal

2026-11-18

Surv IR v1.1 互換

## **1. 概要**

**Behavior IR** は、Surv IRで定義されたシステムの振る舞いを、状態・状態遷移・制約・時間的性質として宣言的に記述するための中間表現である。

Surv IRがシステムの静的構造を記述するのに対し、Behavior IRはその構造上で起こり得る動的な振る舞いを記述する。

### **1.1 設計上の位置付け**

**Surv Project Manifest**

シンボル解決・依存関係・検証設定

**Structural IR**

What exists?

Schema

Func

Mod

Pipeline

既存のSurv IR v1.1

**Behavior IR**

What can happen?

State

Action

Invariant

Temporal Property

今回追加する仕様

統合・検証・トレーサビリティ

**Verification / Implementation**

TLA+ / TLC / Apalache / Rust / TypeScript

Behavior IRはTLA+の代替を目指すものではない。TLA+などの形式手法に変換可能な、より高水準の設計記述を提供する。

ただし、TLA+の全表現能力を再現することも目的としない。

### **1.2 主要目標**

1. **独立性** — 既存のSurv IR仕様とファイルを変更せずに導入できる。
2. **参照可能性** — Schema、Func、Modを既存の名前解決規則で参照できる。
3. **形式性** — 状態と遷移に曖昧さのない意味論を与える。
4. **検証可能性** — 到達可能性、不変条件、デッドロックなどを機械的に検査できる。
5. **並行性** — 処理の順序が固定されないシステムを記述できる。
6. **実装非依存性** — 言語・フレームワーク・データベースに依存しない。
7. **段階的導入** — 単純な状態機械から始め、分散システムの検証まで拡張できる。

### **1.3 非目標**

初期版では次を対象外とする。

- 汎用プログラミング言語の実装
- 実際の業務処理の自動生成
- 任意のTLA+仕様との完全な相互変換
- 実装コードの正しさの自動証明
- 実時間性能や物理的時間制約の厳密な検証
- 無限状態空間における一般的な性質の決定

特に最後の点は重要である。有限モデルにおける検証結果と、現実のシステム全体の正しさは区別しなければならない。

## **2. 基本設計方針**

### **2.1 Structural IRとBehavior IRの関係**

両者は上下関係ではなく、**同じシステムを異なる観点から記述する二つの射影**とする。

| 観点    | Structural IR | Behavior IR         |
| ----- | ------------- | ------------------- |
| データ   | Schema        | State variable      |
| 操作    | Func          | Action              |
| 組織化   | Mod           | Behavior            |
| 処理順序  | Pipeline      | Transition relation |
| 制約    | 型・参照整合性       | Invariant           |
| 時間的性質 | 原則対象外         | Safety / Liveness   |
| 実装対応  | `impl.*`      | Actionと実装の対応        |

ここで重要なのは、**FuncとActionを同一視しない**ことである。

例えば `func.processPayment` は実装上の関数を抽象化するが、`action.paymentSucceeded` はシステムに観測される一つの原子的な状態変化を記述する。

一つのFuncが複数のActionに対応する場合も、複数のFuncが一つのActionを実現する場合もある。

### **2.2 ファイル構成**

```
project/
├── surv.toml
│
├── design/
│   ├── order.toml
│   ├── payment.toml
│   └── inventory.toml
│
└── behavior/
    ├── order.behavior.toml
    ├── payment.behavior.toml
    └── checkout.behavior.toml
```

Behavior IRは `.behavior.toml` を標準拡張子とする案を採用する。

- 1ファイルに複数のBehaviorを記述できる。
- 1つのBehaviorは複数のStructural IRファイルを参照できる。
- 同一のSchemaやFuncを複数のBehaviorが共有できる。
- Behavior間の参照・合成は、初期版では限定する。

### **2.3 設計の最小単位**

Behavior IRの中心概念は `behavior` とする。

一つのBehaviorは次の組で定義される。

\\[ B=(V,I,A,P,F) \\]

| 記号      | 意味            |
| ------- | ------------- |
| \\(V\\) | 状態変数の集合       |
| \\(I\\) | 初期状態を定義する述語   |
| \\(A\\) | 許可された状態遷移の集合  |
| \\(P\\) | 検証対象の性質       |
| \\(F\\) | 公平性など実行に関する仮定 |

このうちMVPでは \\(V,I,A\\) と、不変条件を中心とする \\(P\\) を必須の中核とする。公平性 \\(F\\) は次の段階で導入する。

## **3. 機能要件**

優先度はP0（MVP必須）、P1（初期拡張）、P2（将来）とする。

| ID     | 機能              | 優先度 |
| ------ | --------------- | --- |
| BR-001 | Behaviorの宣言と管理  | P0  |
| BR-002 | 状態変数と型の定義       | P0  |
| BR-003 | 初期状態の定義         | P0  |
| BR-004 | Actionによる状態遷移   | P0  |
| BR-005 | Guard（実行条件）     | P0  |
| BR-006 | Invariant（不変条件） | P0  |
| BR-007 | Survシンボル参照      | P0  |
| BR-008 | 有限状態モデルの探索      | P0  |
| BR-009 | 反例トレース出力        | P0  |
| BR-010 | TLA+への変換        | P1  |
| BR-011 | 並行実行・非決定性       | P0  |
| BR-012 | Liveness（活性）    | P1  |
| BR-013 | Fairness（公平性）   | P1  |
| BR-014 | Behaviorの合成     | P1  |
| BR-015 | 実装とのドリフト検出      | P1  |
| BR-016 | 時間・リトライ・障害モデル   | P2  |
| BR-017 | Refinement検証    | P2  |

### **3.1 Behavior**

Behaviorは、一つの独立した状態遷移系を表す。

```
[behavior.order_lifecycle]
description = "Order lifecycle"
owner = "mod.order_api"
```

`owner` はSurv IRのModへの参照である。

要件：

- Behavior名はプロジェクト内で一意に解決される。
- `owner` は任意だが、指定した場合は実在するModを参照する。
- Behaviorは複数の状態変数、Action、Propertyを所有できる。
- Behavior自体は実装関数でも実行プロセスでもない。

### **3.2 State Variable**

状態は、現在のシステムの状態を表す変数の集合として定義する。

```
[behavior.order_lifecycle]
description = "Order lifecycle"
owner = "mod.order_api"

[behavior.order_lifecycle.vars]
order_status = "enum(pending, paid, cancelled)"
payment_count = "int"
```

要件：

- Primitive、Enum、Record、有限集合などの型を扱う。
- 変数は時間とともに値が変化する。
- 状態変数には初期値または初期値制約が必要。
- Structural IRのSchemaへの参照も許可する。
- 検証時には状態空間を有限に制限できること。

重要なのは、**StateとSchemaは異なる**という点である。

`schema.Order` はデータ構造を定義する。一方、`order_status` はある時点でのシステムの値を表す。

したがって、Schemaに含まれるフィールドをそのまま状態として扱うことを必須にしない。

### **3.3 Initial State**

初期状態を定義する。

```
[behavior.order_lifecycle.init]
order_status = "pending"
payment_count = 0
```

初期状態は一つの具体値である必要はない。将来は複数の初期状態を許す述語もサポートする。

例えば、モデルが複数の注文を表す場合、各注文の状態が `pending` または `cancelled` のいずれかである、という初期条件も考えられる。

### **3.4 Action**

Actionは、ある状態から別の状態への許可された変化を定義する。

```
[behavior.order_lifecycle.action.pay]
guard = "order_status == 'pending'"
update = {order_status = "'paid'", payment_count = "payment_count + 1"}
```

意味は次のとおり。

\\[ Pay(s,s') \equiv \begin{cases} s.status = pending\\\ s'.status = paid\\\ s'.paymentCount = s.paymentCount+1 \end{cases} \\]

ここでは、Actionに記述された変更は**原子的な1回の状態遷移**として扱う。

また、変更対象として指定されなかった変数は変化しない。

この規則を明示しないと、TLA+へ変換したときに意味が曖昧になる。

Actionの要件：

- `guard` が真である状態でのみ実行可能。
- `update` は変更後の値を定義する。
- 複数のActionが有効なとき、実行されるActionは非決定的に選択される。
- Actionの記述順は実行順序を意味しない。
- すべての右辺は更新前の状態で評価する。
- 更新不能な状態では当該Actionを実行できない。

### **3.5 Invariant**

Invariantは、すべての到達可能な状態で成立すべき条件である。

```
[behavior.order_lifecycle.invariant.no_double_payment]
expr = "payment_count <= 1"
severity = "error"
```

例えば、同じ注文に二度支払いが発生してはいけないという業務上の制約を記述できる。

要件：

- Invariantは状態に対する純粋な述語とする。
- 副作用を持つ演算は許可しない。
- 初期状態とすべての到達可能状態で評価する。
- 違反が見つかれば、そこへ至る反例を返す。

`severity` は報告上の分類であり、真偽の意味を変えない。

### **3.6 Temporal Property**

Invariantだけでは「何かが起きないこと」は検証できても、「いつか何かが起きること」は十分に表現できない。

そこでP1では時間的性質を導入する。

例：

```
[behavior.order_lifecycle.property.eventually_resolved]
kind = "liveness"
expr = "eventually(order_status != 'pending')"
```

これは、注文がいつか必ず `pending` 以外になることを要求する。

ただし、これが成立するためには、処理がいつまでも先送りされないという公平性の仮定が必要になる場合がある。

TLA+では不変条件のほかに時間論理の性質や公平性も扱えるため、Behavior IRもこれらを区別する。

[image](https://www.google.com/s2/favicons?domain=https://dl.tlapl.us\&sz=32)

dl.tlapl.us+1



## **4. MVP構文案**

以下は **Behavior IR v0.1の提案構文**であり、現時点で実装済みの仕様ではない。

### **4.1 完全な例：注文・決済モデル**

```

# order.behavior.toml
package = "backend"
namespace = "app.orders"

[meta]
ir = "surv-behavior"
version = "0.1.0"

[behavior.order_lifecycle]
description = "Order and payment lifecycle"
owner = "mod.order_api"

[behavior.order_lifecycle.vars]
order_status = "enum(pending, paid, cancelled)"
payment_count = "int"

[behavior.order_lifecycle.init]
order_status = "pending"
payment_count = 0

[behavior.order_lifecycle.action.pay]
ref = "func.processPayment"
guard = "order_status == 'pending'"
update = {
  order_status = "'paid'",
  payment_count = "payment_count + 1"
}

[behavior.order_lifecycle.action.cancel]
ref = "func.cancelOrder"
guard = "order_status == 'pending'"
update = {order_status = "'cancelled'"}

[behavior.order_lifecycle.invariant.no_double_payment]
expr = "payment_count <= 1"

[behavior.order_lifecycle.invariant.no_paid_cancellation]
expr = "not (order_status == 'cancelled' and payment_count > 0)"

[behavior.order_lifecycle.check]
max_states = 10000

```

この例では `pay` と `cancel` のどちらも初期状態から実行可能である。

このモデルの到達可能な状態は3つ。paidとcancelledは終端状態として扱われる。

### **4.2 インタラクティブな実行例**

次のモデルは、二重決済を防ぐGuardを外した場合に何が起こるかを示す。

**状態遷移シミュレーター**

説明用モデル

Guardを有効にする（pending時のみ支払い可能）

Order status

## **pending**

Payment count

## **0**

PayCancel Reset

Execution trace

pending

Invariant holds for this trace

これは手動のトレース操作であり、全状態探索ではない。実際のモデルチェッカーは実行可能な分岐を系統的に探索する。

Guardを外せば支払いが繰り返され、Invariantに違反する。これは、**型が正しく、関数の入出力も整合しているのに、システム全体としては誤った振る舞いを許容してしまう**という問題の典型例である。

## **5. 形式意味論**

Behavior IRを単なる状態遷移図の記述言語で終わらせないために、最小限の数学的意味論を確定する。

### **5.1 状態と遷移**

状態変数集合を \\(V\\)、状態空間を \\(S\\) とする。

\\[ S=\prod\_{v\in V}Domain(v) \\]

各Actionは状態間の関係として定義する。

\\[ A_i\subseteq S\times S \\]

すべてのActionを統合した遷移関係は、

\\[ Next(s,s')=\bigvee\_{i=1}^{n}A_i(s,s') \\]

となる。

したがって、Behaviorは単なる関数のリストではなく、**非決定的な遷移関係**を定義している。

これにより、複数のActionが同時に実行可能な状態でも、特定の実行順序を暗黙に強制しない。

### **5.2 実行トレース**

実行トレースは状態の列である。

\\[ \sigma=s_0,s_1,s_2,\ldots \\]

ただし、

\\[ I(s_0)\land \forall i,\ Next(s_i,s\_{i+1}) \\]

を満たすものとする。

ここではMVPの有限トレースを定義している。TLA+への変換では、状態が変化しないステップ（stuttering）を許す無限振る舞いの意味論に対応させる。

### **5.3 SafetyとLiveness**

| 性質           | 問い            | 例                   |
| ------------ | ------------- | ------------------- |
| Safety       | 悪いことが起きないか    | 二重決済が発生しない          |
| Liveness     | 良いことがいつか起きるか  | 受理された注文が最終的に処理される   |
| Reachability | ある状態に到達できるか   | cancelledになることができるか |
| Deadlock     | 遷移不能な状態が存在するか | 処理途中で停止していないか       |

ここで、完了状態から遷移できないことは必ずしもエラーではない。

そのため、MVPでも `terminal` 状態の指定、またはデッドロック検査から除外する条件を定義する必要がある。

なお、TLA+のstutteringを許す意味論とCLI上のデッドロック検出は区別する。

## **6. 式言語の要件**

これは仕様上、特に重要な決定事項である。

先ほどの例では、

```
guard = "order_status == 'pending'"
```

と記述した。

しかし、これは**PythonでもJavaScriptでもない、Behavior IR固有の式言語**として定義しなければならない。

任意のコードを評価してしまうと、言語非依存性や形式検証可能性を失う。

### **6.1 MVPの演算子**

| 種類    | 演算子                              |
| ----- | -------------------------------- |
| 論理    | `and`, `or`, `not`               |
| 比較    | `==`, `!=`, `<`, `<=`, `>`, `>=` |
| 算術    | `+`, `-`, `*`                    |
| グループ化 | `(expr)`                         |
| 参照    | `variable`, `record.field`       |

MVPでは任意の関数呼び出し、I/O、動的コード実行を禁止する。

### **6.2 内部AST**

TOMLの文字列を直接TLA+に置換するのではなく、式パーサーで型付きASTに変換する。

```
"payment_count <= 1"
         │
         ▼
       Parser
         │
         ▼
    BinaryExpr(<=)
      ├── Var(payment_count)
      └── IntLiteral(1)
         │
         ▼
     Type Checker
         │
         ▼
    Semantic IR
         │
    ┌────┴────┐
    ▼         ▼
 Simulator   TLA+ Generator
```

このSemantic IRこそが、検証バックエンドから独立した意味論の中心となる。

### **6.3 型検査**

最低限、次の検証を実施する。

- 未宣言変数の参照禁止
- 型の異なる値の不正比較の禁止
- 更新先変数の型整合性
- Guard・Invariantの結果がBooleanであること
- Actionの更新式が純粋であること
- 同一Action内で同じ変数への重複代入を禁止

また、整数の意味論を明示する。検証上の整数を数学的整数とするなら、実装言語のオーバーフロー挙動とは異なる可能性があるため、数値範囲や表現方法の制約をモデル設定として持つ。

## **7. Surv IRとの統合要件**

### **7.1 既存の名前解決を再利用する**

Behavior IR独自の名前解決器は実装しない。

既存の `ProjectChecker` が提供するシンボルテーブルを共有する。

```
[behavior.order_lifecycle]
owner = "mod.order_api"

[behavior.order_lifecycle.action.pay]
ref = "func.processPayment"
```

ただし、ここで以下の区別が必要となる。

- `owner` はBehaviorとModの所属関係
- `ref` はActionとFuncの設計上の対応関係
- `guard`、`update` は実際の状態遷移の意味論

**Func参照が存在することは、そのFuncの実装がActionの意味論を満たすことを保証しない。**

これは将来の実装検証との境界になる。

### **7.2 Manifest拡張**

既存の `surv.toml` に任意の設定を追加する。

```
[paths]
ir_root = "design"
behavior_root = "behavior"

[verification]
backend = "internal"
default_max_states = 100000
```

既存プロジェクトに `behavior_root` がない場合も問題なく動作する。

### **7.3 名前空間の整合性**

現行のSurv IR v1.1には、名前解決で整理すべき細部が残っている。

例えば、

- `app.orders.schema.Order`
- `schema.Order`
- `Order`

という表記のどれを正規形式とし、どれを短縮参照とするか。

Behavior IRが加わると名前衝突の可能性も増すため、**内部の正規シンボルIDは一つに統一する**ことを要求する。

また、既存仕様の `require` はモジュール依存、`import` はシンボル別名という意味を維持する。Behavior側の参照を追加しても、この二つを混同しない。

## **8. 検証エンジンの要件**

### **8.1 段階的な検証構成**

**Behavior IR Parser**

TOML → AST

**Type Checker / Resolver**

シンボル解決・静的検証

**Normalized Behavioral Model**

形式意味論を持つ内部AST

**Internal Explorer**

v0.1

有限状態探索・反例

**TLA+ Backend**

P1

TLC / Apalache

**Verification Report**

結果・探索範囲・反例トレース

TLCは状態を明示的に列挙するモデルチェッカーで、Apalacheは論理制約を利用するシンボリックモデルチェッカーである。後者の通常の有界検査では、指定された探索長内での反例探索と、全実行に関する証明を区別する必要がある。

[image](https://www.google.com/s2/favicons?domain=https://github.com\&sz=32)

GitHub+1



### **8.2 検証結果**

CLI出力には、少なくとも以下の状態を用意する。

| 結果        | 意味              |
| --------- | --------------- |
| `PASS`    | 指定した検証範囲内で性質成立  |
| `FAIL`    | 反例が見つかった        |
| `UNKNOWN` | 探索未完了・資源制限・判定不能 |
| `ERROR`   | 仕様または変換のエラー     |

`PASS` は有限状態空間を網羅した場合と、有界探索のみの場合で保証の範囲が異なるため、検証方式と探索範囲を必ず併記する。

### **8.3 反例トレース**

```
FAIL: no_double_payment

Initial:
  order_status = pending
  payment_count = 0

Step 1: pay
  order_status = paid
  payment_count = 1

Step 2: pay
  order_status = paid
  payment_count = 2

Violation:
  payment_count <= 1
```

出力形式はJSONもサポートし、後のHTML可視化やエディタ連携に利用できるようにする。

## **9. 非機能要件**

| 分類     | 要件                                  |
| ------ | ----------------------------------- |
| 互換性    | Surv IR v1.1を破壊しない                  |
| 可読性    | 人間が小規模モデルを直接編集できる                   |
| 決定性    | 同一モデル・同一設定に対して同一の探索結果を再現可能          |
| 安全性    | 式評価で任意のコードを実行しない                    |
| 性能     | 状態数・探索時間・メモリ使用量の上限を設定できる            |
| 診断性    | ファイル名・行番号・シンボル・違反条件を報告する            |
| 拡張性    | バックエンドを追加してもBehavior IRの基本意味論を変更しない |
| テスト可能性 | パーサー・型検査・遷移意味論・バックエンド変換を個別にテストできる   |

性能については、初期から大規模分散システムの完全探索を保証しない。

また、同一入力に対する再現性は確保するが、探索順序の一致まで保証する必要はない。例えばBFSを採用する内部探索器であれば、最短反例の探索を基本方針としてよい。

## **10. 開発フェーズ**

**Phase 1 — Behavior Core（v0.1）**

状態・Guard・Action・Invariant・参照解決・有限状態探索・反例出力。TLA+なしでも動作する最小実装。

**Phase 2 — Formal Verification（v0.2）**

TLA+生成、TLC連携、時間的性質、Fairness、検証モデルの有限化設定。

**Phase 3 — Composition（v0.3）**

複数Behaviorの合成、共有状態、プロセス別のAction、非同期メッセージ、障害注入。

**Phase 4 — Refinement & Implementation（v0.4以降）**

抽象モデルと詳細モデルの対応検査、実装トレースとの照合、テストケース生成、ドリフト検出の拡張。

## **11. MVPの受け入れ基準**

v0.1は以下が成立した時点で完了とみなす。

- 既存Surv IR v1.1のプロジェクトをそのまま読み込める。
- `.behavior.toml` から状態変数、初期状態、Action、Invariantを解析できる。
- 型違反、未定義変数、未解決のSurv参照を検出できる。
- 遷移の有効・無効をGuardに従って正しく判定できる。
- 複数Actionの非決定的な分岐を探索できる。
- すべての到達可能状態でInvariantを評価できる。
- 違反が発生した場合に再現可能な反例を出力できる。
- 探索資源制限に達した場合に`PASS`を返さない。
- 終端状態と意図しないデッドロックを区別できる。
- 正常モデルと異常モデルの両方で自動テストが通る。

## **12. 仕様確定前に解決すべき設計問題**

ここまでの要件から、実装前に決めるべき論点がいくつかある。

### **12.1 状態変数をどこまでSchemaに結びつけるか**

現在の提案では、

```
[behavior.order.vars]
status = "enum(pending, paid, cancelled)"
```

のように独立して定義する。

しかし既存の `schema.Order` にも `status` フィールドがあるなら、二重定義となる可能性がある。

ここでは、**Schemaは値の構造、State Variableは時間変化する値**と分離した上で、型の再利用を可能にするのがよい。

### **12.2 実装関数の粒度とActionの粒度**

SurvのFuncが大きな処理単位を表している場合、それを単一の原子的Actionに対応させると、内部の並行性や中間状態が消えてしまう。

例えば決済処理は次のように分解できる。

```
paymentRequested
       ↓
paymentAuthorized
       ↓
paymentCaptured
       ↓
paymentRecorded
```

このどこを原子的な境界とみなすかは、モデルの重要な設計判断となる。

### **12.3 Pipelineの意味論を変更する必要があるか**

私は**既存のPipelineには手を加えない**方針を提案する。

Pipelineは設計上の処理フローを示し、Behaviorは実際に許された状態遷移を示す。

両者の整合性を確認する機能は将来追加できるが、初期版でPipelineから自動的に状態遷移を導出しない。

### **12.4 TOML構文の制約**

TOMLでは `[section]` を開始した後、後続のキーはそのセクションに属する。したがって、既存Surv IRの説明にある `import` や `require` も、トップレベルに置く場合は最初のテーブル宣言より前に記述する必要がある。

さらに、本提案で示した次の複数行形式はTOMLとして不正になる。

```
update = {
  order_status = "'paid'",
  payment_count = "payment_count + 1"
}
```

正式仕様では、例えば次のようにする。

```
[behavior.order.action.pay.update]
order_status = "'paid'"
payment_count = "payment_count + 1"
```

この形式なら通常のTOMLパーサーで問題なく解析できる。

## **13. 最終的な設計判断**

ここで一つ、Behavior IRの将来を左右する選択がある。

**Behavior IRを「状態機械の定義ファイル」に留めるか、それとも「実行可能な形式仕様」にするか。**

私の提案は後者である。

単に状態名と遷移を列挙するだけなら、既存のState Machine DSLや状態遷移図とあまり変わらない。

しかし、状態変数、遷移関係、初期状態、不変条件に形式意味論を与えれば、Behavior IRは、設計者が記述したシステムの振る舞いについて機械的に反例を発見できる言語になる。

そのうえで、Surv IR全体の構造は次のように整理できる。

\\[ \boxed{ \text{Surv Project} = \text{Structure} + \text{Behavior} + \text{Verification} } \\]

これは仕様としての構成であって、各層の整合性が自動的に証明されるという意味ではない。

さらに長期的には、**Behavior IRをTLA+の単なるフロントエンドにするのではなく、Surv独自の意味論を持つ中間表現として確立する**ことを推奨する。

そうすることで、TLA+だけでなく、SMTソルバー、テストケース生成器、シミュレーター、実装トレース解析器など、複数のバックエンドへ展開できる。

最初の実装目標としては、**小規模な状態遷移系をTOMLで記述し、**`surc verify`**&#x20;によって不変条件の反例を発見できること**が最も適切だと考える。これだけで、既存のStructural IRとは明確に異なる実用価値を実証できる。