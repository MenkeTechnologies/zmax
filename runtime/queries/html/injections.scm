((comment) @injection.content
 (#set! injection.language "comment"))

; <script type="application/json" | "application/ld+json" | "importmap"> -> json.
; Specific type rules come before the default so they win.
((script_element
  (start_tag
    (attribute
      (attribute_name) @_attr
      (quoted_attribute_value (attribute_value) @_type)))
  (raw_text) @injection.content)
 (#eq? @_attr "type")
 (#any-of? @_type "application/json" "application/ld+json" "importmap")
 (#set! injection.language "json"))

; <script type="text/typescript" | "application/typescript"> -> typescript.
((script_element
  (start_tag
    (attribute
      (attribute_name) @_attr
      (quoted_attribute_value (attribute_value) @_type)))
  (raw_text) @injection.content)
 (#eq? @_attr "type")
 (#any-of? @_type "text/typescript" "application/typescript")
 (#set! injection.language "typescript"))

; Any other <script> (no type, module, text/javascript, …) -> javascript.
((script_element
  (start_tag) @_start
  (raw_text) @injection.content)
 (#not-match? @_start "type=.(application/(ld.)?json|importmap|text/typescript|application/typescript)")
 (#set! injection.language "javascript"))

((style_element
  (raw_text) @injection.content)
 (#set! injection.language "css"))

; Inline CSS in a `style="…"` attribute. Capture the inner attribute_value
; (not the quotes) — injecting the delimiters fails to register the layer.
((attribute
  (attribute_name) @_attr
  (quoted_attribute_value (attribute_value) @injection.content))
 (#eq? @_attr "style")
 (#set! injection.language "css"))

; Inline JavaScript in event-handler attributes: onclick="…", onload="…", etc.
((attribute
  (attribute_name) @_attr
  (quoted_attribute_value (attribute_value) @injection.content))
 (#match? @_attr "^on")
 (#set! injection.language "javascript"))
