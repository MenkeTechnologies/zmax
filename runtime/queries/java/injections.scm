([(line_comment) (block_comment)] @injection.content
 (#set! injection.language "comment"))

; ---------------------------------------------------------------------------
; SQL language injection: query strings passed to JDBC / JPA / Spring methods.
; Capture the inner string_fragment (not the delimiters).
((method_invocation
  name: (identifier) @_method
  arguments: (argument_list
    . (string_literal (string_fragment) @injection.content)))
 (#any-of? @_method "executeQuery" "executeUpdate" "execute" "prepareStatement"
                    "prepareCall" "createQuery" "createNativeQuery" "query"
                    "queryForObject" "queryForList" "queryForMap" "update"
                    "batchUpdate")
 (#set! injection.language "sql"))

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
