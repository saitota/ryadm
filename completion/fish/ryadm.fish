#!/usr/bin/fish

function __fish_ryadm_universial_optspecs
    string join \n 'a-yadm-dir=' 'b-yadm-repo=' 'c-yadm-config=' \
                    'd-yadm-encrypt=' 'e-yadm-archive=' 'f-yadm-bootstrap='
end

function __fish_ryadm_needs_command
    # Figure out if the current invocation already has a command.
    set -l cmd (commandline -opc)
    set -e cmd[1]
    argparse -s (__fish_ryadm_universial_optspecs) -- $cmd 2>/dev/null
    or return 0
    if set -q argv[1]
        echo $argv[1]
        return 1
    end
    return 0
end

function __fish_ryadm_using_command
    set -l cmd (__fish_ryadm_needs_command)
    test -z "$cmd"
    and return 1
    contains -- $cmd $argv
    and return 0
end

# ryadm's specific autocomplete
complete -x -c ryadm -n '__fish_ryadm_needs_command'       -a 'clone'      -d 'Clone an existing repository'
complete -F -c ryadm -n '__fish_ryadm_using_command clone' -s w            -d 'work-tree to use (default: $HOME)'
complete -f -c ryadm -n '__fish_ryadm_using_command clone' -s b            -d 'branch to clone'
complete -x -c ryadm -n '__fish_ryadm_using_command clone' -s f            -d 'force to overwrite'
complete -x -c ryadm -n '__fish_ryadm_using_command clone' -l bootstrap    -d 'force bootstrap to run'
complete -x -c ryadm -n '__fish_ryadm_using_command clone' -l no-bootstrap -d 'prevent bootstrap from beingrun'

complete -x -c ryadm -n '__fish_ryadm_needs_command' -a 'alt'       -d 'Create links for alternates'
complete -x -c ryadm -n '__fish_ryadm_needs_command' -a 'bootstrap' -d 'Execute $HOME/.config/yadm/bootstrap'
complete -x -c ryadm -n '__fish_ryadm_needs_command' -a 'perms'     -d 'Fix perms for private files'
complete -x -c ryadm -n '__fish_ryadm_needs_command' -a 'enter'     -d 'Run sub-shell with GIT variables set'
complete    -c ryadm -n '__fish_ryadm_needs_command' -a 'git-crypt' -d 'Run git-crypt commands for the yadm repo'
complete -x -c ryadm -n '__fish_ryadm_needs_command' -a 'help'      -d 'Print a summary of yadm commands'
complete -x -c ryadm -n '__fish_ryadm_needs_command' -a 'upgrade'   -d 'Upgrade to version 2 of yadm directory structure'
complete -x -c ryadm -n '__fish_ryadm_needs_command' -a 'version'   -d 'Print the version of yadm'

complete -x -c ryadm -n '__fish_ryadm_needs_command' -a 'init' -d 'Initialize an empty repository'
complete -x -c ryadm -n '__fish_ryadm_using_command init' -s f -d 'force to overwrite'
complete -F -c ryadm -n '__fish_ryadm_using_command init' -s w -d 'set work-tree (default: $HOME)'

complete -x -c ryadm -n '__fish_ryadm_needs_command' -a 'list' -d 'List tracked files at current directory'
complete -x -c ryadm -n '__fish_ryadm_using_command list' -s a -d 'list all managed files instead'

complete -x -c ryadm -n '__fish_ryadm_needs_command' -a 'encrypt' -d 'Encrypt files'
complete -x -c ryadm -n '__fish_ryadm_needs_command' -a 'decrypt' -d 'Decrypt files'
complete -x -c ryadm -n '__fish_ryadm_using_command decrypt' -s l -d 'list the files stored without extracting'

complete -x -c ryadm -n '__fish_ryadm_needs_command' -a 'introspect' -d 'Report internal yadm data'
complete -x -c ryadm -n '__fish_ryadm_using_command introspect' -a (printf -- '%s\n' 'commands configs repo switches') -d 'category'

complete -x -c ryadm -n '__fish_ryadm_needs_command' -a 'gitconfig' -d 'Pass options to the git config command'
complete -x -c ryadm -n '__fish_ryadm_needs_command' -a 'config'    -d 'Configure a setting'
for name in (ryadm introspect configs)
    complete -x -c ryadm -n '__fish_ryadm_using_command config' -a $name -d 'yadm config'
end

# yadm universial options
complete --force-files -c ryadm -s Y -l yadm-dir       -d 'Override location of yadm directory'
complete --force-files -c ryadm      -l yadm-repo      -d 'Override location of yadm repository'
complete --force-files -c ryadm      -l yadm-config    -d 'Override location of yadm configuration file'
complete --force-files -c ryadm      -l yadm-encrypt   -d 'Override location of yadm encryption configuration'
complete --force-files -c ryadm      -l yadm-archive   -d 'Override location of yadm encrypted files archive'
complete --force-files -c ryadm      -l yadm-bootstrap -d 'Override location of yadm bootstrap program'

# wraps git's autocomplete
set -l GIT_DIR (ryadm introspect repo)
# setup the correct git-dir by appending it to git's argunment
complete -c ryadm -w "git --git-dir=$GIT_DIR"
