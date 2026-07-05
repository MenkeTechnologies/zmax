; strykelang highlights.

; Definition / call names (before the generic identifier rule below).
(subroutine_definition name: (identifier) @function)
(function_call name: (identifier) @function)
(method_call method: (identifier) @function.method)
(package_statement name: (identifier) @namespace)

; Built-in list operators (print, push, map, pmap, pfor, …).
(builtin) @function.builtin

; Keywords.
[
  "sub" "package" "use" "no" "require"
  "my" "our" "local"
  "if" "elsif" "else" "unless"
  "while" "until" "for" "foreach"
  "return" "last" "next" "redo"
  "not" "and" "or" "xor" "eq" "ne" "cmp" "lt" "gt" "le" "ge"
  "defined" "ref"
] @keyword

; Operators — including stryke's pipe / threading operators.
[
  "=" "+=" "-=" "*=" "/=" ".=" "//=" "||=" "&&=" "%=" "**=" "x="
  "==" "!=" "<=>" "<" ">" "<=" ">=" "=~" "!~"
  "+" "-" "*" "/" "%" "x" "**" ".." "..."
  "!" "\\" "||" "&&" "//"
  "->" "?" ":"
  "|>" "~>" "~>>" "~s>" "~s>>" "~p>" "~p>>" "~d>" "~d>>"
  "->>" "~|>" "||>" "|then|"
] @operator

; Variables (by leading sigil the theme can still color them one class).
(variable) @variable

; Literals.
(number) @constant.numeric
(string) @string
(interpolated_string) @string
(command_string) @string.special
(qw_list) @string
(regex) @string.regexp
(substitution) @string.regexp
(comment) @comment

; Bareword identifiers not otherwise captured.
(identifier) @variable
