; Highlights for Fountain, the plain-text screenplay markup.
; Written against ArmingLou/fountain-tree-sitter @ ffffb22.
;
; Ordering follows the helix convention: broad captures first, specific ones
; after — a later pattern wins over an earlier one on the same node.

; -------------------------------------------------------------- prose bodies
(text) @none
(description) @none

; ----------------------------------------------------------------- structure
; `### Act One` — section headings nest three deep.
(section_heading
  (section_start) @markup.heading.marker
  (description) @markup.heading.1)

; `INT. WAREHOUSE - NIGHT #1#`
(scene_heading) @markup.heading.2
(scene_start) @markup.heading.marker
(scene_location) @markup.heading.2
(scene_time) @keyword
(scene_number) @constant.numeric.integer

; `= A synopsis of the scene.`
(synopsis) @comment
(synopsis_start) @punctuation.special

; `===` forced page break.
(page_break) @punctuation.special
(page_break_marker) @punctuation.special

; ------------------------------------------------------------------ dialogue
; The speaking character's name line (`@McCLANE` or bare `MCCLANE (V.O.)`).
(character) @function
(forced_character_start) @punctuation.special

; `(beat)` — an actor's direction.
(parenthetical_line) @comment
(paren_text) @comment

; The spoken lines themselves.
(dialogue_text) @string
(dialogue_line_start) @punctuation.special

; ------------------------------------------------------------------- actions
(action) @none
(forced_action_start) @punctuation.special
(uppercase_text) @constant

; `CUT TO:` / `> FADE OUT.`
(transition) @keyword.control.return
(forced_transition_start) @punctuation.special

; `> Centered text <`
(centered_text) @markup.heading.3
[
  (centered_start)
  (centered_end)
] @punctuation.special

; `~ Sung lyrics`
(lyric) @string.special
(lyric_start) @punctuation.special

; ------------------------------------------------------------------- inlines
(bold) @markup.bold
(italic) @markup.italic
(bold_italic) @markup.bold
(underline) @markup.italic

(escaped_char) @constant.character.escape
(literal_char) @constant.character.escape

; --------------------------------------------------------------- title page
; `Title: Big Fish` — key/value metadata at the top of the script. The grammar
; splits these two cases: `title_key_with_space` carries its value inline as a
; `description`, while a bare `title_key` takes only indented continuation lines.
(title_page_field
  (title_key_with_space) @variable.other.member
  (description) @string)
(title_page_field
  (title_key) @variable.other.member)
(title_continuation) @string

; ---------------------------------------------------------- notes / boneyard
; `[[ a note ]]` and `/* commented-out pages */` are both non-printing.
(note) @comment
(note_start) @comment
(note_content_nested) @comment
(inline_note) @comment

(boneyard) @comment
(boneyard_start) @comment
(boneyard_content_nested) @comment
(inline_boneyard) @comment
