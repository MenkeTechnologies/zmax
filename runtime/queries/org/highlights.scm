(headline (stars) @markup.heading.marker (#eq? @markup.heading.marker "*")) @markup.heading.1
(headline (stars) @markup.heading.marker (#eq? @markup.heading.marker "**")) @markup.heading.2
(headline (stars) @markup.heading.marker (#eq? @markup.heading.marker "***")) @markup.heading.3
(headline (stars) @markup.heading.marker (#eq? @markup.heading.marker "****")) @markup.heading.4
(headline (stars) @markup.heading.marker (#eq? @markup.heading.marker "*****")) @markup.heading.5
(headline (stars) @markup.heading.marker (#eq? @markup.heading.marker "******")) @markup.heading.6

(block) @markup.raw.block
(list) @markup.list.unnumbered
(directive) @markup.label
(property_drawer) @markup.label


((expr) @markup.bold
 (#match? @markup.bold "\\*.*\\*"))

((expr) @markup.italic
 (#match? @markup.italic "/.*/"))
((expr) @markup.raw.inline
 (#match? @markup.raw.inline "~.*~"))

((expr) @markup.quote
 (#match? @markup.quote "=.*="))

; Comments (lines beginning with `#`).
(comment) @comment

; Tags trailing a headline (`:work:urgent:`).
(tag) @label

; List checkboxes (`[ ]`, `[X]`, `[-]`).
(checkbox) @markup.list.checked

; --- TODO/DONE keywords and priority cookies -----------------------------------
; The milisims org grammar has no dedicated TODO-keyword or priority nodes:
; a headline's text is an `item` made of `expr` tokens. We match those by text.

; Leading TODO-state keyword of a headline (the first `expr` of its `item`).
((item . (expr) @keyword)
 (#any-of? @keyword "TODO" "DONE" "NEXT" "WAITING" "HOLD" "CANCELLED" "CANCELED"))

; Priority cookies like `[#A]`, `[#B]`, `[#1]` inside a headline.
((item (expr) @constant)
 (#match? @constant "^\\[#.\\]$"))

; --- Timestamps & planning -----------------------------------------------------
; `<...>` / `[...]` timestamps and the date/time inside them are real nodes.
(timestamp) @string.special
(timestamp (date) @constant.numeric)
(timestamp (time) @constant.numeric)

; `SCHEDULED:` / `DEADLINE:` / `CLOSED:` planning keyword before a timestamp.
(entry_name) @keyword
