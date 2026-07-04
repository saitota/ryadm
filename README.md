# radm - Rust (Yet) Another Dotfiles Manager

**radm** is a byte-compatible Rust rewrite of [yadm][] 3.5.0. It's a drop-in
replacement: same commands, options, exit codes, output, and config files — a
native binary instead of a bash script, for faster startup and no cross-platform
bash/awk quirks.

[![License][license-badge]][license-link]

## Features

* Based on [Git][], with its full feature set
* System-specific [alternate][feature-alternates] and [templated][feature-templates] files
* [Encryption][feature-encryption] via [GnuPG][], [OpenSSL][], [transcrypt][], or [git-crypt][]
* [Bootstrapping][feature-bootstrap] and [hooks][feature-hooks]

radm implements yadm's interface, so [yadm's docs][website-link] apply directly.

## Compatibility

radm targets byte-identical behavior with yadm 3.5.0: same paths
(`$XDG_CONFIG_HOME/yadm`, `$XDG_DATA_HOME/yadm`), config keys (`yadm.*`,
`local.*`), hook variables, template syntax, and per-command stdout/stderr/exit
codes. Output still says "yadm" so existing scripts keep working. An existing
yadm setup works with radm immediately — no migration.

The only difference: `radm version` prints a `radm version 1.0.0` line so you
can tell which binary you're running; the `yadm version 3.5.0` line is preserved.

## Install

Needs a Rust toolchain (edition 2021, Rust 1.74+).

```sh
cargo install --path .   # or: task install
```

Prebuilt macOS arm64 binaries are on the [releases page][releases-link].

## Quick tour

    radm init                         # or: radm clone <url>
    radm add <file> && radm commit

    echo '.ssh/id_rsa' > ~/.config/yadm/encrypt
    radm encrypt                      # radm decrypt to restore

    radm add path/file.cfg##os.Linux  # per-OS alternates
    radm add path/file.cfg##os.Darwin

## Development

Everything goes through [Task][]; run `task` to list all tasks.

| Task | Description |
|------|-------------|
| `task build` / `build:release` | Build debug / release binary |
| `task fmt` / `fmt:check` | Format / check formatting |
| `task lint` | clippy with warnings denied |
| `task test` | Unit and integration tests |
| `task test:compat` | Differential tests against the original bash yadm |
| `task ci` | Everything CI runs |
| `task install` / `release` | Install / publish a release |

`task test:compat` runs identical scenarios against radm and the original bash
yadm (pinned at `bbb58e6`, extracted from git history), diffing stdout, stderr,
exit codes, and filesystem state. Plain `cargo test` skips it.

## Platform support

Developed and tested on **macOS on Apple Silicon**. Other Unix-like platforms
should work (radm has zero runtime Rust deps and shells out to `git`,
`gpg`/`openssl`, and template engines like yadm), but are untested.

## License & attribution

[GPL-3.0-or-later][license-link], same as yadm. A derivative work of [yadm][],
copyright (C) 2015-2024 Tim Byrne, (C) 2025 Erik Flodin. If you use radm,
consider starring yadm too.

[Git]: https://git-scm.com/
[GnuPG]: https://gnupg.org/
[OpenSSL]: https://www.openssl.org/
[Task]: https://taskfile.dev/
[feature-alternates]: https://yadm.io/docs/alternates
[feature-bootstrap]: https://yadm.io/docs/bootstrap
[feature-hooks]: https://yadm.io/docs/hooks
[feature-encryption]: https://yadm.io/docs/encryption
[feature-templates]: https://yadm.io/docs/templates
[git-crypt]: https://github.com/AGWA/git-crypt
[license-badge]: https://img.shields.io/badge/license-GPL--3.0--or--later-blue
[license-link]: https://github.com/saitota/radm/blob/main/LICENSE
[releases-link]: https://github.com/saitota/radm/releases
[transcrypt]: https://github.com/elasticdog/transcrypt
[website-link]: https://yadm.io/
[yadm]: https://github.com/yadm-dev/yadm
