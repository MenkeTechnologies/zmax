; Highlights for PlantUML.
; Written against cathaysia/tree-sitter-plantuml @ e8b14f8, which covers the
; @startuml sequence/class dialect plus the embedded @startjson / @startyaml /
; @startebnf / @startregex / @startgantt / @startmindmap / @startwbs / @startchen
; / @startditaa / @startchronology sub-diagrams.
;
; Ordering follows the helix convention: broad captures first, specific ones
; after — a later pattern wins over an earlier one on the same node.

; ------------------------------------------------------------ generic leaves
(name) @variable
(value) @string
(label) @label
(string) @string
(comment) @comment
(digit) @constant.numeric.integer
(boolean_literal) @constant.builtin.boolean
(color) @constant
(font_name) @string.special
(escape_char) @constant.character.escape

; ---------------------------------------------------------- diagram fences
; Every dialect is delimited by its own @start.../@end... pair.
[
  "@startuml"
  "@enduml"
  "@startjson"
  "@endjson"
  "@startyaml"
  "@endyaml"
  "@startebnf"
  "@endebnf"
  "@startregex"
  "@endregex"
  "@startgantt"
  "@endgantt"
  "@startmindmap"
  "@endmindmap"
  "@startwbs"
  "@endwbs"
  "@startchen"
  "@endchen"
  "@startditaa"
  "@endditaa"
  "@startchronology"
  "@endchronology"
] @keyword.directive

; `!pragma teoz true`
(pragma) @keyword.directive
[
  "!pragma"
  "!!"
] @punctuation.special

; -------------------------------------------------------------- declarations
; `participant Alice as A order 10`
(participant_name) @constructor
(attr_alias) @variable
(attr_order) @variable.other.member
(stereotypes) @attribute
(anchor) @label

(attribute
  (kind) @keyword)
(attribute
  (name) @variable)

; `skinparam responseMessageBelowArrow true`
(skinparam) @keyword
(skinparam_attr) @variable.other.member

; ------------------------------------------------------------ control blocks
[
  "alt"
  "else"
  "loop"
  "group"
  "end"
  "ref"
  "end ref"
] @keyword.control

[
  "activate"
  "deactivate"
  "autoactivate"
  "destroy"
  "create"
  "hide"
  "box"
  "end box"
  "endbox"
  "sequence"
  "teoz"
] @keyword

[
  "autonumber"
  "autonumber stop"
  "inc"
  "resume"
] @function.builtin

[
  "as"
  "of"
  "on"
  "off"
  "over"
  "across"
  "order"
  "return"
] @keyword.operator

[
  "left"
  "right"
  "top"
  "bottom"
  "footbox"
] @keyword.storage.modifier

[
  "true"
  "false"
] @constant.builtin.boolean

; ------------------------------------------------------------------- notes
[
  "note"
  "hnote"
  "rnote"
  "end note"
  "end hnote"
  "end rnote"
  "endnote"
  "endhnote"
  "endrnote"
] @keyword

(block_note) @comment
(block_hnote) @comment
(block_rnote) @comment
(line_note) @comment
(footnote) @comment

; ------------------------------------------------------------------ titles
[
  "title"
  "end title"
  "endtitle"
] @keyword

(title) @markup.heading.1
(delay_title) @markup.italic

; ------------------------------------------------------- arrows / separators
; The `->`, `-->`, `[o`, `[x` … arrow vocabulary that draws the messages.
(outgoing) @punctuation.special
(iconnector) @punctuation.special
(connector) @punctuation.special
(separator) @punctuation.special

(custom_label
  "[" @punctuation.bracket
  "]" @punctuation.bracket)

[
  "["
  "]"
  "{"
  "}"
] @punctuation.bracket

[
  "<"
  ">"
  "<<"
  ">>"
  "||"
  "|||"
  "-"
  "--"
  "++"
  "**"
  "=="
  "..."
  "/"
  "//"
  "\\"
  "o"
  "x"
] @operator

[
  ","
  ":"
  "@"
] @punctuation.delimiter

; --------------------------------------------- embedded sub-diagram payloads
; The body of an @startjson / @startyaml / … block is handed to that dialect's
; own grammar via injections.scm; keep the raw span visually distinct here.
[
  (json_data)
  (yaml_data)
  (ebnf_data)
  (regex_data)
  (gantt_data)
  (mindmap_data)
  (wbs_data)
  (chen_data)
  (ditaa_data)
  (chronology_data)
] @markup.raw.block
