; Adapted from georgeharker/tree-sitter-zsh nvim-queries/zsh/injections.scm
; (MIT). The printf and readline injections need `#offset!`, which zmax's
; queries do not use; `eval` / `trap` strings are shell, as in the bash queries.

((comment) @injection.content
  (#set! injection.language "comment"))

((regex) @injection.content
  (#not-match? @injection.content "\\$\\{.*\\}")
  (#set! injection.language "regex"))

(heredoc_redirect
  (heredoc_body) @injection.content
  (heredoc_end) @injection.language)

(command
  name: (command_name (word) @_command)
  argument: (raw_string) @injection.content
  (#match? @_command "^[gnm]?awk$")
  (#set! injection.language "awk"))

(command
  name: (command_name (word) @_command (#any-of? @_command "jq" "jaq"))
  argument: [
    (raw_string) @injection.content
    (string (string_content) @injection.content)
  ]
  (#set! injection.language "jq"))

(command
  name: (command_name (word) @_command (#any-of? @_command "eval" "trap"))
  .
  argument: [
    (raw_string) @injection.content
    (string (string_content) @injection.content)
  ]
  (#set! injection.language "zsh"))
