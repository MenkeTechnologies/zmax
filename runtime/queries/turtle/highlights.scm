(string) @string

(lang_tag) @type

[
  "_:"
  "<"
  ">"
  (namespace)
] @namespace

[
  (iri_reference)
  (prefixed_name)
] @variable

(blank_node_label) @variable

"a" @variable.builtin

(integer) @constant.numeric.integer

[
  (decimal)
  (double)
] @constant.numeric.float

(boolean_literal) @constant.builtin.boolean

[
  "BASE"
  "PREFIX"
  "@prefix"
  "@base"
] @keyword

[
  "."
  ","
  ";"
] @punctuation.delimiter

[
  "("
  ")"
  "["
  "]"
  (anon)
] @punctuation.bracket

(comment) @comment

(echar) @string.escape

(rdf_literal
  "^^" @type
  datatype: (_
    [
      "<"
      ">"
      (namespace)
    ] @type) @type)
