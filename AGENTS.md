# AGENTS.md

## プロジェクト

ryadm — yadm 3.5.0 をバイト互換のまま置き換える Rust 実装（Rust Yet Another Dotfiles Manager）。ドロップイン置換で、出力・終了コード・パス・設定キー・フック環境変数・テンプレート構文まで yadm 3.5.0 と同一。git / gpg / openssl 等へシェルアウトする方式を維持する。

名前は `ryadm`（旧 `radm` から改名済み。`radm` とは呼ばない）。

## 互換性の原則

- 本家 yadm 互換インターフェースを壊さない: 設定ディレクトリ `~/.config/yadm`、環境変数 `YADM_*`、config キー `yadm.*`、CLI フラグ `--yadm-*`、`yadm version 3.5.0` 表示は不変
- 実行時挙動を変える変更は差分互換テストで担保する

## リポジトリ

- リモートは `saitota/ryadm`（upstream ではない）。PR は saitota/ryadm に作る
- デフォルトブランチは `main`（`develop` ではない）
- ブランチ名 `{JIRA_KEY}_{description}`
- PR 作成時 `--assignee @me` を付ける。PR Description にコードの内容を書かない

## 開発

すべて [Task](https://taskfile.dev/) 経由。`task` でタスク一覧。

- `task ci` — CI と同じゲート（fmt / clippy / build / test / compat）
- `task test:compat` — git 履歴に固定した本家 bash yadm と ryadm を同一シナリオで実行し、stdout / stderr / 終了コード / FS 状態を突き合わせる
- `task build` / `task test` / `task install` / `task release`

## コード構成

- `src/main.rs` — エントリ、引数ディスパッチ
- `src/cmd/` — サブコマンド（clone / init / enter / upgrade / misc）
- `src/context.rs` `src/paths.rs` — パス・グローバル状態（GIT_DIR/CWD）。暗黙結合を明示化するコメントあり
- `src/git.rs` `src/encrypt.rs` `src/hooks.rs` `src/template.rs` `src/alt.rs` `src/exclude.rs` — 各機能
- `src/util.rs` — 共通ヘルパ（`glob_prefix` 等）

## 環境

- Rust edition 2021 / rust-version 1.74+、ランタイム Rust 依存ゼロ
- 開発・テストは macOS (Apple Silicon)。他 Unix 系は未検証
- CI は GitHub Actions (macos-14)。Actions は SHA ピン留め、権限 `contents: read` 最小化

## ライセンス

yadm の派生物で GPL-3.0-or-later。
