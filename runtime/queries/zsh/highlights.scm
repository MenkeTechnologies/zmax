; Adapted from georgeharker/tree-sitter-zsh nvim-queries/zsh/highlights.scm
; (MIT), with zmax's capture names and zsh's builtins.

[
  "("
  ")"
  "{"
  "}"
  "["
  "]"
  "[["
  "]]"
  "(("
  "))"
] @punctuation.bracket

[
  ";"
  ";;"
  (case_fallthrough)
  (case_test_next)
  "&"
] @punctuation.delimiter

[
  ">"
  ">>"
  "<"
  "<<"
  "&&"
  "|"
  "|&"
  "||"
  "="
  "+="
  "=~"
  "=="
  "!="
  "&>"
  "&>>"
  "<&"
  ">&"
  ">|"
  "<&-"
  ">&-"
  "<<-"
  "<<<"
  ".."
  "!"
] @operator

; Do *not* spell check strings since they typically have some sort of
; interpolation in them, or, are typically used for things like filenames, URLs,
; flags and file content.
[
  (string)
  (raw_string)
  (ansi_c_string)
  (heredoc_body)
] @string

[
  (heredoc_start)
  (heredoc_end)
] @label

(variable_assignment
  (word) @string)

; (command
;   argument: "variable_ref" @string) ; bare dollar
(concatenation
  (word) @string)

[
  "if"
  "then"
  "else"
  "elif"
  "fi"
  "case"
  "in"
  "esac"
] @keyword.control.conditional

[
  "for"
  "do"
  "done"
  "select"
  "until"
  "while"
] @keyword.control.repeat

[
  "declare"
  "typeset"
  "readonly"
  "local"
  "unset"
  "unsetenv"
] @keyword

"export" @keyword.control.import

"function" @keyword.function

(special_variable_name) @variable.builtin

; trap -l
((word) @constant.builtin
  (#any-of? @constant.builtin
    "SIGHUP" "SIGINT" "SIGQUIT" "SIGILL" "SIGTRAP" "SIGABRT" "SIGBUS" "SIGFPE" "SIGKILL" "SIGUSR1"
    "SIGSEGV" "SIGUSR2" "SIGPIPE" "SIGALRM" "SIGTERM" "SIGSTKFLT" "SIGCHLD" "SIGCONT" "SIGSTOP"
    "SIGTSTP" "SIGTTIN" "SIGTTOU" "SIGURG" "SIGXCPU" "SIGXFSZ" "SIGVTALRM" "SIGPROF" "SIGWINCH"
    "SIGIO" "SIGPWR" "SIGSYS" "SIGRTMIN" "SIGRTMIN+1" "SIGRTMIN+2" "SIGRTMIN+3" "SIGRTMIN+4"
    "SIGRTMIN+5" "SIGRTMIN+6" "SIGRTMIN+7" "SIGRTMIN+8" "SIGRTMIN+9" "SIGRTMIN+10" "SIGRTMIN+11"
    "SIGRTMIN+12" "SIGRTMIN+13" "SIGRTMIN+14" "SIGRTMIN+15" "SIGRTMAX-14" "SIGRTMAX-13"
    "SIGRTMAX-12" "SIGRTMAX-11" "SIGRTMAX-10" "SIGRTMAX-9" "SIGRTMAX-8" "SIGRTMAX-7" "SIGRTMAX-6"
    "SIGRTMAX-5" "SIGRTMAX-4" "SIGRTMAX-3" "SIGRTMAX-2" "SIGRTMAX-1" "SIGRTMAX"))

((word) @constant.builtin.boolean
  (#any-of? @constant.builtin.boolean "true" "false"))

(comment) @comment

(test_operator) @operator

(command_substitution
  "$(" @punctuation.special
  ")" @punctuation.special)

(process_substitution
  [
    "<("
    ">("
  ] @punctuation.special
  ")" @punctuation.special)

(arithmetic_expansion
  [
    "$(("
    "(("
  ] @punctuation.special
  "))" @punctuation.special)

(arithmetic_expansion
  "," @punctuation.delimiter)

(ternary_expression
  [
    "?"
    ":"
  ] @operator)

(binary_expression
  operator: _ @operator)

(unary_expression
  operator: _ @operator)

(postfix_expression
  operator: _ @operator)

(function_definition
  name: (word) @function)

(command_name
  (word) @function)

(command_name
  (word) @function.builtin
  (#any-of? @function.builtin
    "-" "." ":" "[" "alias" "autoload" "bg" "bindkey" "break" "builtin" "bye" "cd" "chdir"
    "command" "comparguments" "compcall" "compctl" "compdescribe" "compfiles" "compgroups"
    "compquote" "compset" "comptags" "comptry" "compvalues" "continue" "declare" "dirs" "disable"
    "disown" "echo" "echotc" "echoti" "emulate" "enable" "eval" "exec" "exit" "export" "false" "fc"
    "fg" "float" "functions" "getln" "getopts" "hash" "history" "integer" "jobs" "kill" "let"
    "limit" "local" "logout" "noglob" "popd" "print" "printf" "private" "pushd" "pushln" "pwd" "r"
    "read" "readonly" "rehash" "return" "sched" "set" "setopt" "shift" "source" "suspend" "test"
    "times" "trap" "true" "ttyctl" "type" "typeset" "ulimit" "umask" "unalias" "unfunction"
    "unhash" "unlimit" "unset" "unsetopt" "vared" "wait" "whence" "where" "which" "zcompile"
    "zformat" "zle" "zmodload" "zparseopts" "zregexparse" "zstyle" "zcurses" "zftp" "zprof" "zpty"
    "zselect" "zsocket" "zstat" "zsystem" "ztcp" "strftime" "sysopen" "sysread" "sysseek"
    "syswrite" "syserror" "zgetattr" "zsetattr" "zdelattr" "zlistattr"))

(command
  argument: [
    (word) @variable.parameter
    (concatenation
      (word) @variable.parameter)
  ])

(declaration_command
  (word) @variable.parameter)

(unset_command
  (word) @variable.parameter)

(number) @constant.numeric

((word) @constant.numeric
  (#match? @constant.numeric "^[0-9]+$"))

(file_redirect
  (word) @string.special.path)

(herestring_redirect
  (word) @string)

(file_descriptor) @operator

(variable_ref
  "$" @punctuation.special)

(expansion
  "${" @punctuation.special
  "}" @punctuation.special)

"``" @punctuation.special

(array_star) @variable.builtin

(array_at) @variable.builtin

(expansion_flags) @attribute

(expansion_style) @attribute

(expansion_pattern
  pattern: "#" @attribute)

(expansion_modifier) @attribute

(simple_variable_name) @variable

(glob_pattern) @string.regexp

(variable_name) @variable

((variable_name) @constant
  (#match? @constant "^[A-Z][A-Z_0-9]*$"))

((variable_name) @variable.builtin
  (#any-of? @variable.builtin
    ; POSIX shell variables
    "CDPATH" "HOME" "IFS" "MAIL" "MAILPATH" "OPTARG" "OPTIND" "PATH" "PS1" "PS2"
    ; https://zsh.sourceforge.io/Doc/Release/Parameters.html#Parameters-Set-By-The-Shell
    "_" "ARGC" "CPUTYPE" "DIRSTACK" "EGID" "EPOCHREALTIME" "EPOCHSECONDS" "ERRNO" "EUID"
    "FUNCFILETRACE" "FUNCNEST" "FUNCSOURCETRACE" "FUNCSTACK" "GID" "HISTCMD" "HOST" "LINENO"
    "LOGNAME" "MACHTYPE" "OLDPWD" "OSTYPE" "PIPESTATUS" "PPID" "PWD" "RANDOM" "SECONDS" "SHLVL"
    "TRY_BLOCK_ERROR" "TRY_BLOCK_INTERRUPT" "TTY" "TTYIDLE" "UID" "USERNAME" "VENDOR" "ZSH_ARGZERO"
    "ZSH_EVAL_CONTEXT" "ZSH_EXECUTION_STRING" "ZSH_NAME" "ZSH_PATCHLEVEL" "ZSH_SCRIPT"
    "ZSH_SUBSHELL" "ZSH_VERSION"
    ; ZLE parameters (set by shell) - https://zsh.sourceforge.io/Doc/Release/Zsh-Line-Editor.html
    "BUFFER" "BUFFERLINES" "CONTEXT" "CURSOR" "CUTBUFFER" "HISTNO" "ISEARCHMATCH_ACTIVE"
    "ISEARCHMATCH_END" "ISEARCHMATCH_START" "KEYMAP" "KEYS" "KEYS_QUEUED_COUNT" "LASTABORTEDSEARCH"
    "LASTSEARCH" "LASTWIDGET" "LBUFFER" "MARK" "MATCH" "MBEGIN" "MEND" "NUMERIC" "PENDING"
    "POSTDISPLAY" "PREBUFFER" "PREDISPLAY" "PREFIX" "QIPREFIX" "QISUFFIX" "RBUFFER" "REGION_ACTIVE"
    "REPLY" "SAVECURSOR" "SUFFIX" "UNDO_CHANGE_NO" "UNDO_LIMIT_NO" "WIDGET" "WIDGETFUNC"
    "WIDGETSTYLE" "YANK_ACTIVE" "YANK_END" "YANK_START" "WORDS_STYLE" "ZLE_RECURSIVE" "ZLE_STATE"
    ; Completion parameters - https://zsh.sourceforge.io/Doc/Release/Completion-Widgets.html
    "CURRENT" "IPREFIX" "ISUFFIX" "compstate"
    ; https://zsh.sourceforge.io/Doc/Release/Parameters.html#Parameters-Used-By-The-Shell
    "ARGV0" "BAUD" "COLUMNS" "CORRECT_IGNORE" "CORRECT_IGNORE_FILE" "DIRSTACKSIZE" "EDITOR" "ENV"
    "FCEDIT" "FIGNORE" "FPATH" "HISTCHARS" "HISTFILE" "HISTSIZE" "KEYBOARD_HACK" "KEYTIMEOUT" "LANG"
    "LC_ALL" "LC_COLLATE" "LC_CTYPE" "LC_MESSAGES" "LC_NUMERIC" "LC_TIME" "LINES" "LISTMAX"
    "LOGCHECK" "MAILCHECK" "MANPATH" "MODULE_PATH" "NULLCMD" "POSTEDIT" "PROMPT" "PROMPT2" "PROMPT3"
    "PROMPT4" "PROMPTCHARS" "PS3" "PS4" "PSVAR" "READNULLCMD" "RPROMPT" "RPROMPT2" "RPS1" "RPS2"
    "SAVEHIST" "SHELL" "SPROMPT" "STTY" "TERM" "TIMEFMT" "TMOUT" "TMPPREFIX" "VISUAL" "WATCH"
    "WATCHFMT" "WORDCHARS" "ZBEEP" "ZDOTDIR" "ZLE_LINE_ABORTED" "ZLE_REMOVE_SUFFIX_CHARS"
    "ZLE_RPROMPT_INDENT" "ZLE_SPACE_SUFFIX_CHARS"
    ; Array/lowercase tied parameters - https://zsh.sourceforge.io/Doc/Release/Variables-Index.html
    "argv" "cdpath" "dirstack" "fignore" "fpath" "funcfiletrace" "funcsourcetrace" "funcstack"
    "functrace" "histchars" "killring" "mailpath" "manpath" "match" "mbegin" "mend" "module_path"
    "options" "path" "pipestatus" "prompt" "psvar" "reply" "signals" "status" "watch" "words"
    "zsh_eval_context"))

((simple_variable_name) @variable.builtin
  (#any-of? @variable.builtin
    ; POSIX shell variables
    "CDPATH" "HOME" "IFS" "MAIL" "MAILPATH" "OPTARG" "OPTIND" "PATH" "PS1" "PS2"
    ; https://zsh.sourceforge.io/Doc/Release/Parameters.html#Parameters-Set-By-The-Shell
    "_" "ARGC" "CPUTYPE" "DIRSTACK" "EGID" "EPOCHREALTIME" "EPOCHSECONDS" "ERRNO" "EUID"
    "FUNCFILETRACE" "FUNCNEST" "FUNCSOURCETRACE" "FUNCSTACK" "GID" "HISTCMD" "HOST" "LINENO"
    "LOGNAME" "MACHTYPE" "OLDPWD" "OSTYPE" "PIPESTATUS" "PPID" "PWD" "RANDOM" "SECONDS" "SHLVL"
    "TRY_BLOCK_ERROR" "TRY_BLOCK_INTERRUPT" "TTY" "TTYIDLE" "UID" "USERNAME" "VENDOR" "ZSH_ARGZERO"
    "ZSH_EVAL_CONTEXT" "ZSH_EXECUTION_STRING" "ZSH_NAME" "ZSH_PATCHLEVEL" "ZSH_SCRIPT"
    "ZSH_SUBSHELL" "ZSH_VERSION"
    ; ZLE parameters (set by shell) - https://zsh.sourceforge.io/Doc/Release/Zsh-Line-Editor.html
    "BUFFER" "BUFFERLINES" "CONTEXT" "CURSOR" "CUTBUFFER" "HISTNO" "ISEARCHMATCH_ACTIVE"
    "ISEARCHMATCH_END" "ISEARCHMATCH_START" "KEYMAP" "KEYS" "KEYS_QUEUED_COUNT" "LASTABORTEDSEARCH"
    "LASTSEARCH" "LASTWIDGET" "LBUFFER" "MARK" "MATCH" "MBEGIN" "MEND" "NUMERIC" "PENDING"
    "POSTDISPLAY" "PREBUFFER" "PREDISPLAY" "PREFIX" "QIPREFIX" "QISUFFIX" "RBUFFER" "REGION_ACTIVE"
    "REPLY" "SAVECURSOR" "SUFFIX" "UNDO_CHANGE_NO" "UNDO_LIMIT_NO" "WIDGET" "WIDGETFUNC"
    "WIDGETSTYLE" "YANK_ACTIVE" "YANK_END" "YANK_START" "WORDS_STYLE" "ZLE_RECURSIVE" "ZLE_STATE"
    ; Completion parameters - https://zsh.sourceforge.io/Doc/Release/Completion-Widgets.html
    "CURRENT" "IPREFIX" "ISUFFIX" "compstate"
    ; https://zsh.sourceforge.io/Doc/Release/Parameters.html#Parameters-Used-By-The-Shell
    "ARGV0" "BAUD" "COLUMNS" "CORRECT_IGNORE" "CORRECT_IGNORE_FILE" "DIRSTACKSIZE" "EDITOR" "ENV"
    "FCEDIT" "FIGNORE" "FPATH" "HISTCHARS" "HISTFILE" "HISTSIZE" "KEYBOARD_HACK" "KEYTIMEOUT" "LANG"
    "LC_ALL" "LC_COLLATE" "LC_CTYPE" "LC_MESSAGES" "LC_NUMERIC" "LC_TIME" "LINES" "LISTMAX"
    "LOGCHECK" "MAILCHECK" "MANPATH" "MODULE_PATH" "NULLCMD" "POSTEDIT" "PROMPT" "PROMPT2" "PROMPT3"
    "PROMPT4" "PROMPTCHARS" "PS3" "PS4" "PSVAR" "READNULLCMD" "RPROMPT" "RPROMPT2" "RPS1" "RPS2"
    "SAVEHIST" "SHELL" "SPROMPT" "STTY" "TERM" "TIMEFMT" "TMOUT" "TMPPREFIX" "VISUAL" "WATCH"
    "WATCHFMT" "WORDCHARS" "ZBEEP" "ZDOTDIR" "ZLE_LINE_ABORTED" "ZLE_REMOVE_SUFFIX_CHARS"
    "ZLE_RPROMPT_INDENT" "ZLE_SPACE_SUFFIX_CHARS"
    ; Array/lowercase tied parameters - https://zsh.sourceforge.io/Doc/Release/Variables-Index.html
    "argv" "cdpath" "dirstack" "fignore" "fpath" "funcfiletrace" "funcsourcetrace" "funcstack"
    "functrace" "histchars" "killring" "mailpath" "manpath" "match" "mbegin" "mend" "module_path"
    "options" "path" "pipestatus" "prompt" "psvar" "reply" "signals" "status" "watch" "words"
    "zsh_eval_context"))

((command
  name: (command_name
    (word) @_printf)
  .
  argument: (word) @_v
  .
  argument: (word) @variable)
  (#eq? @_printf "printf")
  (#eq? @_v "-v")
  (#match? @variable "^[a-zA-Z_][a-zA-Z0-9_]*$"))

(case_item
  value: (word) @variable.parameter)

[
  (regex)
  (extglob_pattern)
] @string.regexp

((program
  .
  (comment) @keyword.directive)
  (#match? @keyword.directive "^#![ \t]*/"))

; zsh-only syntax the upstream queries leave plain.
[
  "repeat"
  "always"
  "coproc"
] @keyword.control

[
  "float"
  "integer"
] @keyword

[
  (zsh_glob_qualifier)
  (zsh_glob_modifier)
  (zsh_extended_glob_flags)
  (zsh_array_subscript_flags)
] @attribute
