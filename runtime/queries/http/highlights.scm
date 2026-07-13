; Request line
(method) @function.method
(request
  url: (_) @string.special.url)
(target_url) @string.special.url
(http_version) @constant

; Headers
(header
  name: (_) @constant)
(header
  ":" @punctuation.delimiter)
(header
  value: (_) @string)

; Variables: `@name = value`, referenced as `{{name}}`
(variable_declaration
  name: (identifier) @variable)
(variable_declaration
  "=" @operator)
(variable) @variable
[
  "{{"
  "}}"
] @punctuation.bracket

; Expected-response block (`HTTP/1.1 200 OK`)
(status_code) @constant.numeric.integer
(status_text) @string

; `< ./body.json` — body read from a file
(external_body
  path: (_) @string.special.path)

; Comment metadata: `# @name login`
(comment
  "@" @keyword
  name: (_) @keyword)
(comment
  "=" @operator)

[
  (comment)
  (request_separator)
] @comment
