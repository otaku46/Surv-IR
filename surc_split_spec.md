surc split 仕様 v1.0
目的

単一の巨大な Surv IR（TOML）ファイルを、package + 複数ファイルの Surv Project へ分割する。

出力は：

surv.toml（manifest）

package ディレクトリ配下の .toml（各ファイルは package/namespace/import/require ヘッダ付き）

1. CLI
1.1 基本形
surc split <input_ir.toml> --config <split_config.toml>

1.2 オプション(今後の拡張案)

--dry-run
生成はせず、分割計画と依存の要約のみ出す

--no-check
分割後の surc project-check を実行しない

--plan <path>
分割計画を JSON で書き出す（後でGUI/CIに使える）

--overwrite
出力先に同名ファイルがある場合に上書きする（デフォルトはエラー）

2. split_config.toml フォーマット
2.1 トップレベル
[split]
output_dir   = "design"      # プロジェクト出力ルート（相対/絶対）
manifest     = "surv.toml"   # 生成する manifest 名
project_name = "my-project"  # surv.toml の project.name
ir_root      = "design"      # surv.toml の packages.root の基準（省略時 output_dir）

# 任意: 共有シンボル方針など
[split.behavior]
shared_symbols = "copy"      # copy | hoist | error
hoist_target_package = "common"
run_project_check = true     # 省略時 true（--no-check で false）

2.2 package 定義

split.packages.<pkg> を複数持てる。

[split.packages.backend]
root      = "design/backend"
namespace = "app.api"
depends   = ["common"]          # surv.toml の packages.<pkg>.depends に反映（任意）

modules = [
  { mod = "mod.user_api",  file = "user.toml"  },
  { mod = "mod.order_api", file = "order.toml" },
]

modules エントリ仕様

mod: 入力IR内に存在する [mod.<name>] の参照（mod.user_api 形式）

file: package root からの相対パス（user.toml / foo/bar.toml など）

3. 入力IRの前提

入力ファイルは Surv IR として parse 可能であること（meta/schema/func/mod の既存仕様に従う）

すべての参照（例: mod.schemas 内の schema.*、mod.funcs 内の func.*）が解決できること
※ 解決できない場合 split は失敗（プロジェクト化以前に壊れているため）

4. 分割の意味論（何を各ファイルへ入れるか）
4.1 「ターゲットmod」の定義

split_config の modules に列挙された mod を ターゲットmod と呼ぶ。

4.2 各出力ファイルの内容

出力ファイルは次を含む：

ファイルヘッダ（トップレベルキー）

package = "<pkg>"

namespace = "<pkg.namespace>"

import = [...]（必要なら）

require = [...]（必要なら）

IRセクション（TOMLテーブル）

ターゲット mod（[mod.*]）

その mod が成立するために必要な schema/func の 依存閉包（下記）

5. 依存閉包（Dependency Closure）仕様

ターゲット mod M の閉包 Closure(M) は以下の集合の合併として定義する。

5.1 mod 直参照

M.schemas に列挙された schema.*

M.funcs に列挙された func.*

M.pipeline に列挙された func.*（チェーン形式も展開後）

5.2 func 参照

func F を含めるとき、以下を含める：

F.input の schema.*

F.output の schema.*

（将来 effect_read/write があるならそれも closure に含めるのが自然。v1.0 では “存在すれば読む” でOK）

5.3 schema 参照

schema S を含めるとき、以下を再帰的に含める（存在すれば）：

kind=edge の from/to

kind=boundary/context の over

kind=space の base（採用しているなら）

5.4 収束条件

上記を参照が増えなくなるまで再帰し、固定点で止める。

6. package / file への配置ルール
6.1 ターゲットmodの所属

ターゲット mod は split_config の指定どおり その package のファイルに配置。

6.2 依存閉包で出てきた schema/func の配置

基本方針は behavior.shared_symbols に従う。

(A) shared_symbols = "copy"（デフォルト）

schema/func を、必要とする各ファイルへ 複製して入れる

ただし警告 W_SHARED_SYMBOL_COPIED を出す（重複シンボル名）

(B) shared_symbols = "hoist"

複数ファイルから参照される schema/func は hoist_target_package の下に集約ファイルを作り、そこへ移す
例：design/common/_shared.toml

参照側は import（必要なら alias）で解決する

(C) shared_symbols = "error"

共有が発生した時点でエラー（分割計画が曖昧、という扱い）

実装コスト最小は (A)。(B) はプロジェクトが育った後に効く。

7. require / import の推論
7.1 require 推論（mod依存）

出力ファイル内の各ターゲット mod M について：

M の “外部mod参照” を収集し、ファイルヘッダ require に列挙する

v1.0 での外部mod参照ソース（最低限）：

入力IR側で require が既にあるなら、それを引き継ぐ

（拡張）将来的に schema/func が “所属mod” を持つ場合は、参照から mod依存を逆算できる

現段階の Surv は「schema/func がどの mod に属するか」を必ずしも持っていないので、require の完全自動推論は難しい。
v1.0 では「元IRの require を引き継ぐ + package跨ぎなら FQ 化」くらいが現実的。

package跨ぎ require の正規形

同 package: mod.foo

別 package: mod.<pkg>.<foo>

（例：mod.auth.login_api）

7.2 import 推論（名前解決）

shared_symbols=copy の場合は基本 import 不要（同一ファイル内に複製されるため）

hoist の場合は、参照先 package を import に追加する
衝突がある場合は alias 必須（次の節）

8. 名前衝突（schema/func の同名）検出

衝突例：

backend と auth が両方 schema.user を持つ

8.1 split の基本動作

shared_symbols=copy：同一ファイル内で衝突したら エラー（TOML上表現できない）

hoist：import する package 間で衝突したら alias が必要

8.2 alias（v1.0での表現）

TOMLを汚さない範囲で、文字列形式で定義：

import = ["auth", "users as u"]


参照は（将来 checker が解決できる形として）：

schema.user：自 package をまず見る

auth.schema.user：auth package

u.schema.user：users package

9. 生成されるファイルのフォーマット規則
9.1 出力の安定性（diff耐性）

同じ入力・同じ config なら、出力は常に同一になるようにする。

推奨の並び順：

ヘッダ（package/namespace/import/require）

[meta]（入力にあれば必要なら写す。基本は出力しなくてもよい）

[schema.*]（キー名でソート）

[func.*]（キー名でソート）

[mod.*]（キー名でソート）

9.2 元ファイルのコメント

v1.0 では コメントの保存はしない（実装コストが高い）。
必要なら将来 --preserve-comments を追加。

10. エラー/警告（Diagnostics）

最低限これだけあると運用が回る。

Errors

E_CONFIG_PARSE: split_config.toml が読めない

E_CONFIG_INVALID: 必須キー不足、packages定義不整合

E_INPUT_PARSE: 入力IRがparse不能

E_MOD_NOT_FOUND: config で指定した mod が入力IRに存在しない

E_DUP_OUTPUT: 同じ出力先が複数モジュールに割り当てられている

E_NAME_CONFLICT: 同一ファイル内で schema/func/mod 名が衝突

E_SHARED_SYMBOL: shared_symbols=error で共有が発生

E_WRITE_CONFLICT: 出力ファイルが存在し overwrite なし

Warnings

W_SHARED_SYMBOL_COPIED: shared_symbols=copy により複製が発生

W_UNUSED_SYMBOL_DROPPED: 入力IRにあるがどのターゲットにも到達しない定義を捨てた

W_REQUIRE_INCOMPLETE: require 自動推論が完全ではない（元requireが薄い等）

11. 出力例（あなたの案に沿った形）
11.1 生成される構造
my-project/
├── surv.toml
└── design/
    ├── backend/
    │   ├── user.toml
    │   └── order.toml
    ├── frontend/
    │   └── ui.toml
    └── common/
        └── auth.toml

11.2 user.toml（例）
package   = "backend"
namespace = "app.api"
require   = ["mod.common.auth"]   # 必要なら FQ

[schema.User]
kind = "node"
role = "entity"
fields = { id="uuid", name="string" }

[func.createUser]
intent = "Create new user"
input  = ["schema.User"]
output = ["schema.User"]

[mod.user_api]
purpose  = "User management API"
schemas  = ["schema.User"]
funcs    = ["func.createUser"]
pipeline = ["func.createUser"]

11.3 surv.toml（例）
[project]
name = "my-project"

[paths]
ir_root = "design"

[packages.backend]
root = "design/backend"
namespace = "app.api"
depends = ["common"]

[packages.frontend]
root = "design/frontend"
namespace = "app.ui"
depends = ["backend"]

[packages.common]
root = "design/common"
namespace = "app.common"

12. 仕様上の注意（正直な限界）

schema/func がどの mod に属するかを Surv がまだ強く持っていないなら、require の完全推論はできない。
→ v1.0 は「元 require の引き継ぎ + 必要なら FQ 化」が現実的。
→ 将来 owner_mod / declared_in みたいなメタ情報があれば推論が一気に強くなる。

hoist は便利だが、import/alias/名前解決 が Checker 側で確立している前提が強い。
→ 最初は copy で固め、プロジェクトが育ってから hoist を実装するのが安全。