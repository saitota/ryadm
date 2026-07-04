# Installation

These completions are for `ryadm`, the byte-compatible Rust rewrite of yadm.
They work exactly like yadm's completions, just against the `ryadm` command.

### Prerequisites

Bash and Zsh completion only works if Git completions are also enabled.

## Bash (manual installation)

Copy the completion script locally, and add this to you bashrc:

```bash
[ -f /path/to/ryadm/completion/bash/ryadm ] && source /path/to/ryadm/completion/bash/ryadm
```

## Zsh (manual installation)

Add the `completion/zsh` folder to `$fpath` in `.zshrc`:

```zsh
fpath=(/path/to/ryadm/completion/zsh $fpath)
autoload -U compinit
compinit
```

## Zsh (using [zplug](https://github.com/b4b4r07/zplug))

Load `_ryadm` as a plugin in your `.zshrc`:

```zsh
fpath=("$ZPLUG_HOME/bin" $fpath)
zplug "saitota/ryadm", use:"completion/zsh/_ryadm", as:command, defer:2
```

## Fish (manual installation)

Copy the completion script `ryadm.fish` to any folder within `$fish_complete_path`. For example, for local installation, you can copy it to `$HOME/.config/fish/completions/` and it will be loaded when `ryadm` is invoked.
