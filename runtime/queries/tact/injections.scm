; See: https://docs.zemacs-editor.com/guides/injection.html

((comment) @injection.content
 (#set! injection.language "comment")
 (#match? @injection.content "^//"))