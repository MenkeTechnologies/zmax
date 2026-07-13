; Identifiers
(identifier) @variable

[
  "process"
  "effect"
] @variable.builtin

(parameters
  (identifier)) @variable.parameter

(access
  definition: (identifier) @variable.other.member)

(global_metadata
  key: (identifier) @variable.other.member)

(function_metadata
  function_name: (identifier) @variable.other.member)

; Literals
(_
  filename: (string)) @string.special.path

(documentation) @string.documentation

[
  (string)
  (fstring)
] @string

(int) @constant.numeric.integer

(real) @constant.numeric.float

; Types
(_
  type: [
    (int_type)
    (float_type)
    (any_type)
  ]) @type.builtin

[
  (single_precision)
  (double_precision)
  (quad_precision)
  (fixed_point_precision)
] @attribute

; Functions
(function_definition
  name: (identifier) @function)

(function_names) @function

(function_call
  (identifier) @function)

(function_call
  (access
    definition: (identifier) @function))

[
  "exp"
  "log"
  "log10"
  "sqrt"
  "abs"
  "floor"
  "ceil"
  "rint"
  "round"
  "acos"
  "asin"
  "atan"
  "cos"
  "sin"
  "tan"
  "atan2"
  "int"
  "float"
  "pow"
  "min"
  "max"
  "fmod"
  "remainder"
  "prefix"
  "attach"
  "enable"
  "control"
  "rdtable"
  "rwtable"
  "select2"
  "select3"
  "lowest"
  "highest"
  "assertbounds"
  (par)
  (seq)
  (sum)
  (prod)
  (component)
  (library)
  (vslider_type)
  (hslider_type)
  (nentry_type)
  (vbargraph_type)
  (hbargraph_type)
  (vgroup_type)
  (hgroup_type)
  (tgroup_type)
  "button"
  "checkbox"
  "soundfile"
  "inputs"
  "outputs"
  "route"
] @function.builtin

; xor is a @keyword.operator
[
  (add)
  (sub)
  (mult)
  (div)
  (mod)
  (pow)
  (or)
  (and)
  (lshift)
  (rshift)
  (lt)
  (le)
  (gt)
  (ge)
  (eq)
  (neq)
  (delay)
  (one_sample_delay)
  "="
  "=>"
  "->"
] @operator

(recursive
  "~" @operator)

(sequential
  ":" @operator)

(split
  "<:" @operator)

(merge
  ":>" @operator)

(parallel
  "," @operator)

; Keywords
[
  (par)
  (seq)
  (sum)
  (prod)
] @keyword.control.repeat

(file_import
  "import" @keyword.control.import)

[
  (wire)
  (cut)
  (mem)
  "declare"
  "with"
  "environment"
  "case"
  "ffunction"
  "fconstant"
  "fvariable"
] @keyword

(xor) @keyword.operator

; Punctuation
[
  ","
  ";"
  "."
] @punctuation.delimiter

[
  "("
  ")"
  "["
  "]"
  "{"
  "}"
] @punctuation.bracket

; Comments
(comment) @comment

; Tags
[
  "<mdoc>"
  "</mdoc>"
  "<metadata>"
  "</metadata>"
  "<equation>"
  "</equation>"
  "<diagram>"
  "</diagram>"
  "<listing"
  "<notice"
  "/>"
] @tag
