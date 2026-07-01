; Concerto Language - Indent Queries (zemacs)
; ============================================
; zemacs-specific indentation rules. For use in zemacs-editor/zemacs at
; runtime/queries/concerto/indents.scm
;
; zemacs uses @indent and @outdent captures, same as tree-sitter convention.
; See: https://docs.zemacs-editor.com/guides/indent.html

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
