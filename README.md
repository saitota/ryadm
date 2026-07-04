# radm - Rust (Yet) Another Dotfiles Manager

**radm** is a byte-compatible Rust rewrite of [yadm][] 3.5.0. It's a drop-in
replacement: same commands, options, exit codes, output, and config files — a
native binary instead of a bash script, for faster startup and no cross-platform
bash/awk quirks.

[![License][license-badge]][license-link]

## Features

* Git-based, with its full feature set
* System-specific alternate and templated files
* Encryption via GnuPG, OpenSSL, transcrypt, or git-crypt
* Bootstrapping and hooks

radm implements yadm's interface, so [yadm's docs][website-link] apply directly.

## Install

Prebuilt binary (macOS arm64) — fetch the latest release and install to
`~/.local/bin` in one go (needs [`gh`][gh]):

```sh
mkdir -p ~/.local/bin && gh release download --repo saitota/radm \
  --pattern '*aarch64-apple-darwin.tar.gz' -O - | tar xz -C ~/.local/bin radm
```

Or build from source (needs a Rust toolchain, edition 2021, Rust 1.74+):

```sh
cargo install --path .   # or: task install
```

(No releases are published yet; build from source for now.)

## Quick tour

    radm init                         # or: radm clone <url>
    radm add <file> && radm commit

    echo '.ssh/id_rsa' > ~/.config/yadm/encrypt
    radm encrypt                      # radm decrypt to restore

    radm add path/file.cfg##os.Linux  # per-OS alternates
    radm add path/file.cfg##os.Darwin

## Commands

| Command | Description |
|---------|-------------|
| `init [-f]` | Initialize an empty repository |
| `clone <url> [-f]` | Clone an existing repository |
| `config <name> <value>` | Configure a setting |
| `list [-a]` | List tracked files |
| `alt` | Create links for alternates |
| `bootstrap` | Execute `$HOME/.config/yadm/bootstrap` |
| `encrypt` / `decrypt [-l]` | Encrypt / decrypt files |
| `perms` | Fix perms for private files |
| `enter [COMMAND]` | Run a sub-shell with GIT variables set |
| `git-crypt [OPTIONS]` | Run git-crypt against the repo |
| `transcrypt [OPTIONS]` | Run transcrypt against the repo |
| `version` / `help` | Print version / usage |

Any Git command or alias also works as a `<command>`, operating on radm's repo
and the work tree (usually `$HOME`).

Global options (before the command) override paths for one invocation:
`-Y`/`--yadm-dir`, `--yadm-data`, `--yadm-repo`, `--yadm-config`,
`--yadm-encrypt`, `--yadm-archive`, `--yadm-bootstrap`.

## Development

Everything goes through [Task][]; run `task` to list all tasks. `task ci` runs
what CI runs; `task test:compat` diffs radm against the original bash yadm
(pinned in git history), asserting identical stdout, stderr, exit codes, and
filesystem state.

## Platform support

Developed and tested on **macOS on Apple Silicon**. Other Unix-like platforms
should work (radm has zero runtime Rust deps and shells out to `git`,
`gpg`/`openssl`, and template engines like yadm), but are untested.

## License & attribution

radm is a derivative work of [yadm][] and is distributed under the same license,
[GPL-3.0-or-later][license-link]. Original yadm copyright (C) 2015-2024 Tim
Byrne, (C) 2025 Erik Flodin.

[Task]: https://taskfile.dev/
[gh]: https://cli.github.com/
[license-badge]: https://img.shields.io/badge/license-GPL--3.0--or--later-blue
[license-link]: https://github.com/saitota/radm/blob/main/LICENSE
[website-link]: https://yadm.io/
[yadm]: https://github.com/yadm-dev/yadm
[yadm]: https://github.com/yadm-dev/yadm
