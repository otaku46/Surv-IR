# gj の Attested 判定を検証する

このモデルは証拠 1 件と、保証内容だけが異なる成功記録 2 件を使う。選んだ記録の対象を、attestation の成果物、attestor、保証種別、保証内容と照合する。正しい記録なら `Attested`、別件の記録なら `RecordedOnly` に到達する。

```sh
surc check examples/evidence/surv.toml
surc verify examples/evidence/surv.toml --format json > /tmp/evidence.json
python3 examples/evidence/replay_gj.py --gj /path/to/gj --goal bound_is_attested < /tmp/evidence.json
python3 examples/evidence/replay_gj.py --gj /path/to/gj --goal unrelated_is_recorded_only < /tmp/evidence.json
```

変異を試すには、モデルを別のディレクトリにコピーし、評価アクションから `and v.obligation.target.content == a.content` の行を削除する。不変条件には同じ照合を残す。

```sh
surc verify /path/to/mutant.behavior.toml --manifest examples/evidence/surv.toml --format json > /tmp/evidence-mutant.json
python3 examples/evidence/replay_gj.py --gj /path/to/gj < /tmp/evidence-mutant.json
```

変異モデルは反例で `Attested` を返し、gj での再生は `RecordedOnly` を返す。両コマンドはこの場合に終了コード 1 になる。

アダプターは記号の成果物と ID から一時台帳を組み立て、gj の `_evidence_standing` を呼ぶ。gj 1.2 では、検証対象の成果物ハッシュ、attestor、保証種別、保証内容からテスト用 attestation を組み立て、その ID を `AttestationVerification` に記録する。旧形式の gj では同じ項目から capsule が照合する subject を作る。この試験の射程は、その対応づけを施した台帳フィクスチャでの判定である。

## SMT と Lean で検査する

PATH に Z3 があれば、同じモデルと変異モデルに `--backend smt` を指定できる。作成した JSON は同じ Python アダプターで再生できる。

```sh
surc verify examples/evidence/surv.toml --backend smt --format json > /tmp/evidence-smt.json
python3 examples/evidence/replay_gj.py --gj /path/to/gj --goal bound_is_attested < /tmp/evidence-smt.json
```

既定の Lean 出力には、不変条件と両方の到達ゴールの証明が含まれる。

```sh
surc codegen lean examples/evidence/surv.toml > /tmp/Evidence.lean
lean /tmp/Evidence.lean
```

生成される不変条件の定理は `Surv_evidence.certified_invariant_0_attested_has_bound_verification`。照合を外した変異モデルでは、探索の停止理由が `violation_found` になり、未証明の不変条件について証明義務が出力される。

対話的に証明するには `--proof obligations` で生成し、コメントにある `InvariantObligations` の例を読み込む側のファイルに移す。`aux` を定義して、初期状態・遷移による保存・不変条件への含意を埋めると、`prove_invariant_0_attested_has_bound_verification` を適用できる。[AttestedProof.lean](AttestedProof.lean) は、状態集合への所属を補助不変条件として使い、三つの義務を証明する例。

```sh
work=$(mktemp -d)
surc codegen lean examples/evidence/surv.toml --proof obligations > "$work/Evidence.lean"
cp examples/evidence/AttestedProof.lean "$work/"
(
  cd "$work"
  lean -o Evidence.olean Evidence.lean &&
    LEAN_PATH="$work" lean AttestedProof.lean
)
```
