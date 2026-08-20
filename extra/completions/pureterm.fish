# PureTerm fish 补全

complete -c pureterm -n '__fish_use_subcommand' -l print-events -d 'Print all events to STDOUT'
complete -c pureterm -n '__fish_use_subcommand' -l ref-test -d 'Generates ref test'
complete -c pureterm -n '__fish_use_subcommand' -l embed -d 'X11 window ID to embed PureTerm within'
complete -c pureterm -n '__fish_use_subcommand' -l config-file -d 'Specify alternative configuration file' -r
complete -c pureterm -n '__fish_use_subcommand' -l socket -d 'Path for IPC socket creation' -r
complete -c pureterm -n '__fish_use_subcommand' -s q -d 'Reduces the level of verbosity'
complete -c pureterm -n '__fish_use_subcommand' -s v -d 'Increases the level of verbosity'
complete -c pureterm -n '__fish_use_subcommand' -l daemon -d 'Do not spawn an initial window'
complete -c pureterm -n '__fish_use_subcommand' -l hold -d 'Remain open after child process exit'
complete -c pureterm -n '__fish_use_subcommand' -l title -s T -d 'Defines the window title' -r
complete -c pureterm -n '__fish_use_subcommand' -l class -d 'Defines window class/app_id on X11/Wayland' -r
complete -c pureterm -n '__fish_use_subcommand' -l option -s o -d 'Override configuration file options' -r
complete -c pureterm -n '__fish_use_subcommand' -s e -d 'Command and args to execute' -r

complete -c pureterm -n '__fish_use_subcommand' -a 'msg' -d 'Send a message to the PureTerm socket'
complete -c pureterm -n '__fish_use_subcommand' -a 'migrate' -d 'Migrate the configuration file'
complete -c pureterm -n '__fish_use_subcommand' -a 'help' -d 'Print help'

complete -c pureterm -n '__fish_seen_subcommand_from msg' -l socket -s s -d 'IPC socket connection path override' -r
complete -c pureterm -n '__fish_seen_subcommand_from msg' -a 'CreateWindow' -d 'Create a new window in the same PureTerm process'
complete -c pureterm -n '__fish_seen_subcommand_from msg' -a 'Config' -d 'Update the PureTerm configuration'
complete -c pureterm -n '__fish_seen_subcommand_from msg' -a 'GetConfig' -d 'Read runtime PureTerm configuration'

complete -c pureterm -n '__fish_seen_subcommand_from migrate' -l config-file -s c -d 'Path to the configuration file' -r
complete -c pureterm -n '__fish_seen_subcommand_from migrate' -l dry-run -s d -d 'Only output TOML config to STDOUT'
complete -c pureterm -n '__fish_seen_subcommand_from migrate' -l skip-imports -s i -d 'Do not recurse over imports'
complete -c pureterm -n '__fish_seen_subcommand_from migrate' -l skip-renames -d 'Do not move renamed fields'
complete -c pureterm -n '__fish_seen_subcommand_from migrate' -l silent -s s -d 'Do not output to STDOUT'
