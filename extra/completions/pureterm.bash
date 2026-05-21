#!/usr/bin/env bash
# PureTerm bash completion

_pureterm() {
    local cur prev words cword
    _init_completion || return

    local commands="msg migrate help"
    local options="--print-events --ref-test --embed --config-file --socket -q -v --daemon --hold -e -T -t --title --class -o --option"

    if [[ $cword -eq 1 ]]; then
        if [[ $cur == -* ]]; then
            COMPREPLY=($(compgen -W "$options" -- "$cur"))
        else
            COMPREPLY=($(compgen -W "$commands" -- "$cur"))
        fi
        return
    fi

    case ${words[1]} in
        msg)
            local msg_options="--socket -s CreateWindow Config GetConfig"
            case $prev in
                --socket|-s)
                    _filedir
                    ;;
                CreateWindow)
                    COMPREPLY=($(compgen -W "$options" -- "$cur"))
                    ;;
                *)
                    COMPREPLY=($(compgen -W "$msg_options" -- "$cur"))
                    ;;
            esac
            ;;
        migrate)
            local migrate_options="--config-file -c --dry-run -d --skip-imports -i --skip-renames --silent -s"
            case $prev in
                --config-file|-c)
                    _filedir
                    ;;
                *)
                    COMPREPLY=($(compgen -W "$migrate_options" -- "$cur"))
                    ;;
            esac
            ;;
    esac
}

complete -F _pureterm pureterm
