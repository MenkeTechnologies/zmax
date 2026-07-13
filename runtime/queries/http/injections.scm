; The request body is a foreign document — parse it with its own grammar so a
; JSON payload gets real JSON highlighting instead of one flat string.
((json_body) @injection.content
 (#set! injection.language "json"))

((xml_body) @injection.content
 (#set! injection.language "xml"))

((graphql_data) @injection.content
 (#set! injection.language "graphql"))

((comment) @injection.content
 (#set! injection.language "comment"))
