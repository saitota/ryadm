# AGENTS.md

## Project

ryadm — a byte-compatible Rust rewrite of yadm 3.5.0 (Rust Yet Another Dotfiles Manager). Drop-in replacement: identical output, exit codes, paths, config keys, hook env vars, and template syntax. Shells out to git / gpg / openssl as yadm does.

Name is `ryadm` (renamed from `radm`; don't call it `radm`).

## Compatibility

Never break the yadm-compatible interface: config dir `~/.config/yadm`, env vars `YADM_*`, config keys `yadm.*`, and CLI flags `--yadm-*` stay unchanged. Any runtime-behavior change must be covered by the diff-compat test. (Exception: `version` deliberately drops yadm's `bash version` / `yadm version 3.5.0` lines in favour of `ryadm version`.)

## Repo

- Remote is `saitota/ryadm` (not upstream). Open PRs against saitota/ryadm.
- Default branch is `main` (not `develop`).
- Branch name: `{JIRA_KEY}_{description}`.
- Add `--assignee @me` on PR create. Don't describe code details in the PR description.

## Development

Everything runs through [Task](https://taskfile.dev/); run `task` to list.

- `task ci` — same gate as CI (fmt / clippy / build / test / compat)
- `task test:compat` — runs upstream bash yadm (pinned in git history) and ryadm on the same scenarios, diffing stdout / stderr / exit code / FS state
- `task build` / `task test` / `task install` / `task release`

## Layout

- `src/main.rs` — entry, arg dispatch
- `src/cmd/` — subcommands (clone / init / enter / upgrade / misc)
- `src/context.rs`, `src/paths.rs` — paths and global state (GIT_DIR/CWD)
- `src/git.rs`, `src/encrypt.rs`, `src/hooks.rs`, `src/template.rs`, `src/alt.rs`, `src/exclude.rs` — features
- `src/util.rs` — shared helpers (e.g. `glob_prefix`)
