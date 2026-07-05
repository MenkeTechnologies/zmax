([(line_comment) (block_comment)] @injection.content
 (#set! injection.language "comment"))

; JDBC/JPA/Spring query-method call sites now come from the config-driven
; injection engine (see docs/LANGUAGE_INJECTION.md). The `@Language` annotation
; hint below stays here — it's structural, not a call-site rule.

; Explicit `@Language("SQL")` annotation on a field/local (JetBrains idiom):
; inject SQL into the annotated declaration's string initializer.
((field_declaration
  (modifiers
    (annotation
      name: (identifier) @_ann
      arguments: (annotation_argument_list (string_literal (string_fragment) @_lang))))
  declarator: (variable_declarator
    value: (string_literal (string_fragment) @injection.content)))
 (#eq? @_ann "Language")
 (#match? @_lang "(?i)^sql$")
 (#set! injection.language "sql"))

((local_variable_declaration
  (modifiers
    (annotation
      name: (identifier) @_ann
      arguments: (annotation_argument_list (string_literal (string_fragment) @_lang))))
  declarator: (variable_declarator
    value: (string_literal (string_fragment) @injection.content)))
 (#eq? @_ann "Language")
 (#match? @_lang "(?i)^sql$")
 (#set! injection.language "sql"))
