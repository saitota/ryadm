# Installation

These completions are for `radm`, the byte-compatible Rust rewrite of yadm.
They work exactly like yadm's completions, just against the `radm` command.

### Prerequisites

Bash and Zsh completion only works if Git completions are also enabled.

## Bash (manual installation)

Copy the completion script locally, and add this to you bashrc:

```bash
[ -f /path/to/radm/completion/bash/radm ] && source /path/to/radm/completion/bash/radm
```

## Zsh (manual installation)

Add the `completion/zsh` folder to `$fpath` in `.zshrc`:

```zsh
fpath=(/path/to/radm/completion/zsh $fpath)
autoload -U compinit
compinit
```

## Zsh (using [zplug](https://github.com/b4b4r07/zplug))

Load `_radm` as a plugin in your `.zshrc`:

```zsh
fpath=("$ZPLUG_HOME/bin" $fpath)
zplug "saitota/radm", use:"completion/zsh/_radm", as:command, defer:2
```

## Fish (manual installation)

Copy the completion script `radm.fish` to any folder within `$fish_complete_path`. For example, for local installation, you can copy it to `$HOME/.config/fish/completions/` and it will be loaded when `radm` is invoked.
