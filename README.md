# radm - Rust (Yet) Another Dotfiles Manager

**radm** is a byte-compatible Rust rewrite of [yadm][] 3.5.0, the dotfiles
manager. It is a drop-in replacement: same commands, same options, same exit
codes, same output, same config files — just a native Rust binary instead of
a bash script.

[![License][license-badge]][license-link]

## Why

yadm is a single, well-tested bash script that has managed dotfiles for years
across many machines. radm exists to keep that exact interface and behavior
while replacing the implementation with a compiled Rust binary: faster
startup, no bash/awk version quirks across platforms, and a codebase that is
easier to statically analyze and test — without asking anyone to change a
single dotfile, config key, or script that already depends on yadm.

* Based on [Git][], with the full range of Git's features
* Supports system-specific [alternate][feature-alternates] files or
  [templated][feature-templates] files
* [Encryption][feature-encryption] of private data using [GnuPG][],
  [OpenSSL][], [transcrypt][], or [git-crypt][]
* Customizable initialization ([bootstrapping][feature-bootstrap])
* Customizable [hooks][feature-hooks] for before and after any operation

Complete feature documentation, usage, and examples for the underlying
interface can be found on the [yadm.io][website-link] website — radm
implements that same interface, so yadm's docs apply directly.

## Compatibility contract

radm is not "inspired by" yadm or "mostly compatible" — it targets **byte
identical** behavior with yadm 3.5.0:

* Same paths: `$XDG_CONFIG_HOME/yadm` (or `$HOME/.config/yadm`) and
  `$XDG_DATA_HOME/yadm` (or `$HOME/.local/share/yadm`). radm does not
  introduce its own directories.
* Same config keys: `yadm.*` and `local.*`, read and written with the
  existing `$HOME/.config/yadm/config` file — an existing yadm setup keeps
  working unchanged.
* Same hook environment variables (`YADM_HOOK_COMMAND`, `YADM_HOOK_DATA`,
  `YADM_HOOK_DIR`, `YADM_HOOK_EXIT`, `YADM_HOOK_FULL_COMMAND`,
  `YADM_HOOK_REPO`, `YADM_HOOK_WORK`) and hook file naming (`pre_*`,
  `post_*`).
* Same template syntax and template variables (`yadm.*` / `YADM_*`) for the
  built-in engine as well as j2/esh/envtpl.
* Same stdout/stderr/exit codes for every command — including that
  `radm help` and other output still say "yadm". This is deliberate: yadm is
  the interface being implemented, and any script or tool that parses yadm's
  output must keep working unmodified when pointed at radm.

The one intentional difference is `radm version`: its first line reads
`radm version 3.5.0` where yadm prints `bash version ...`, so you can tell
which binary you're running. The `yadm version 3.5.0` line that follows is
preserved byte-for-byte for scripts that parse it.

This contract is enforced by a differential test suite (`task test:compat`)
that runs the same scenarios against both the original bash yadm script
(pinned at a specific commit, extracted straight from this repository's git
history) and radm, then diffs stdout, stderr, exit codes, and resulting
filesystem state. See [Development](#development) below.

## Install

Requires a Rust toolchain (edition 2021, Rust 1.74+) to build from source.

### From source with Task

```sh
task install
```

This runs `cargo install --path . --locked`, installing `radm` into your
Cargo bin directory (typically `~/.cargo/bin`).

### From source with Cargo directly

```sh
cargo install --path .
```

### Release tarball

Prebuilt macOS arm64 (Apple Silicon) binaries are published on the
[GitHub releases page][releases-link]. Download the tarball for your
version, verify the accompanying `.sha256` file, and place the `radm` binary
on your `PATH`.

## A very quick tour

    # Initialize a new repository
    radm init

    # Clone an existing repository
    radm clone <url>

    # Add files/changes
    radm add <important file>
    radm commit

    # Encrypt your ssh key
    echo '.ssh/id_rsa' > ~/.config/yadm/encrypt
    radm encrypt

    # Later, decrypt your ssh key
    radm decrypt

    # Create different files for Linux vs MacOS
    radm add path/file.cfg##os.Linux
    radm add path/file.cfg##os.Darwin

Since radm reads and writes the exact same `~/.config/yadm` and
`~/.local/share/yadm` paths as yadm, an existing yadm-managed dotfiles setup
works with radm immediately — no migration step required.

## Development

Everything goes through [Task][] (go-task). Run `task` with no arguments to
list all available tasks.

| Task            | Description                                                          |
|-----------------|-----------------------------------------------------------------------|
| `task build`    | Build the debug binary                                                |
| `task build:release` | Build the optimized release binary                               |
| `task fmt`      | Format the source tree                                                |
| `task fmt:check`| Verify formatting without modifying files                             |
| `task lint`     | Run clippy with warnings denied                                       |
| `task test`     | Run unit and integration tests                                        |
| `task test:compat` | Run the differential compatibility tests against the original bash yadm |
| `task ci`       | Run everything CI runs: fmt:check, lint, build, test, test:compat      |
| `task install`  | Install radm to the Cargo bin directory                               |
| `task release`  | Tag and publish a GitHub release with a macOS arm64 binary             |

### How the compatibility tests work

`task test:compat` extracts the original bash `yadm` script as it existed at
a pinned commit (`bbb58e6`, yadm 3.5.0) directly from this repository's git
history using `git cat-file`, and exposes it to the test suite via the
`RADM_COMPAT_YADM` environment variable. The integration tests in
`tests/compat_yadm.rs` then run identical scenarios — command sequences,
file writes, environment variables — against both that reference script and
the `radm` binary, normalize any path/version differences that are expected
to differ, and assert stdout, stderr, exit code, and resulting filesystem
state are identical. Without `RADM_COMPAT_YADM` set, these tests are skipped,
so a plain `cargo test` (`task test`) works everywhere without needing the
reference script.

## Platform support

Currently developed and tested on **macOS on Apple Silicon (M1 and later)**.
Other Unix-like platforms will likely work, since radm has zero runtime Rust
dependencies and shells out to `git`, `gpg`/`openssl`, and optional template
engines the same way yadm does — but they are untested. Contributions
verifying and fixing other platforms are welcome.

## License & attribution

radm is licensed under the [GPL-3.0-or-later][license-link], the same
license as yadm. It is a derivative work of [yadm][], copyright (C)
2015-2024 Tim Byrne, copyright (C) 2025 Erik Flodin.

* Upstream project: <https://github.com/yadm-dev/yadm>
* Documentation for the interface radm implements: <https://yadm.io/>

If you enjoy using radm, consider starring the yadm project too — this
project would not exist without it.

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
