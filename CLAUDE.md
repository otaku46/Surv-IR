# Project Workflow

- プロジェクト開始時は `./docs` にあるプロジェクト spec を読む。
- 実装時は、spec から Surv IR 形式でまず設計を記述する。
- 設計の記述方法は `/users/masatakashioji/prog/surv/surc/Spec-Surv-IR-v1.1.md` に従う。
  - 設計変更後は `surc check surv.toml` を実行する。
  - Behavior IR を変更した場合は `surc verify surv.toml` も実行する。
- 実装順を決めるときは `surc plan surv.toml` を使う。
- 実装と設計に乖離が出た場合、git commit する段階で `surc diff` を使い、`surv.toml` にまとめる。
