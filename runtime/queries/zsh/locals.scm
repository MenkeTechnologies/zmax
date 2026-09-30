; Adapted from georgeharker/tree-sitter-zsh nvim-queries/zsh/locals.scm (MIT).

[
  (function_definition)
  (subshell)
] @local.scope

(variable_assignment
  name: (variable_name) @local.definition.variable)

(for_statement
  variable: (simple_variable_name) @local.definition.variable)

[
  (variable_name)
  (simple_variable_name)
] @local.reference
