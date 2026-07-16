; Concerto Language - Indent Queries (zmax)
; ============================================
; zmax-specific indentation rules. For use in zmax-editor/zmax at
; runtime/queries/concerto/indents.scm
;
; zmax uses @indent and @outdent captures, same as tree-sitter convention.
; See: https://docs.zmax-editor.com/guides/indent.html

; Indent inside declaration bodies and decorator argument lists
[
  (class_body)
  (enum_body)
  (map_body)
  (decorator_arguments)
] @indent

; Outdent at closing braces and parentheses
[
  "}"
  ")"
] @outdent
