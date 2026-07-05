| Name | Description | Default keybinds |
| --- | --- | --- |
| `no_op` | Do nothing |  |
| `move_char_left` | Move left | normal: `` h ``, `` <left> ``, insert: `` <C-b> ``, `` <left> `` |
| `move_char_right` | Move right | normal: `` l ``, `` <right> ``, insert: `` <C-f> ``, `` <right> `` |
| `move_line_up` | Move up | normal: `` gk ``, `` g<up> `` |
| `move_line_down` | Move down | normal: `` gj ``, `` g<down> `` |
| `shift_line_up` | Move the line/selection up (JetBrains Move Statement Up) | normal: `` <A-up> `` |
| `shift_line_down` | Move the line/selection down (JetBrains Move Statement Down) | normal: `` <A-down> `` |
| `drag_line_down` | Drag the current line down (SPC x . j) | normal: `` <C-x><C-t> ``, `` <space>x.J ``, `` <space>x.j ``, `` <space>x.<down> ``, select: `` <C-x><C-t> ``, `` <space>x.J ``, `` <space>x.j ``, `` <space>x.<down> ``, insert: `` <C-x><C-t> `` |
| `drag_line_up` | Drag the current line up (SPC x . k) | normal: `` <space>x.K ``, `` <space>x.k ``, `` <space>x.<up> ``, select: `` <space>x.K ``, `` <space>x.k ``, `` <space>x.<up> `` |
| `toggle_test_file` | Toggle between implementation and test file (SPC p a) | normal: `` <space>pa ``, select: `` <space>pa `` |
| `fold_comments` | Fold multi-line comment blocks (SPC c h) | normal: `` <space>ch ``, select: `` <space>ch `` |
| `move_visual_line_up` | Move up | normal: `` k ``, `` <up> ``, `` <C-p> ``, insert: `` <up> ``, `` <C-g>k ``, `` <C-g><up> ``, `` <C-g><C-k> `` |
| `move_visual_line_down` | Move down | normal: `` j ``, `` <C-j> ``, `` <C-n> ``, `` <down> ``, insert: `` <C-g>j ``, `` <down> ``, `` <C-g><C-j> ``, `` <C-g><down> `` |
| `extend_char_left` | Extend left |  |
| `extend_char_right` | Extend right |  |
| `extend_line_up` | Extend up |  |
| `extend_line_down` | Extend down |  |
| `extend_visual_line_up` | Extend up |  |
| `extend_visual_line_down` | Extend down |  |
| `copy_selection_on_next_line` | Copy selection on next line |  |
| `copy_selection_on_prev_line` | Copy selection on previous line |  |
| `column_selection` | Turn the selection into a rectangular column block (IntelliJ column selection) |  |
| `visual_block_mode` | Enter/leave vim visual-block selection (CTRL-V) | normal: `` <C-v> ``, `` g<C-h> ``, `` <C-x><space> ``, select: `` <C-v> ``, `` <C-x><space> ``, insert: `` <C-x><space> `` |
| `block_reproject` | Rebuild the visual-block rectangle from its anchor (internal motion helper) |  |
| `block_dollar` | Visual-block: extend each row to its own line end (CTRL-V $) | select: `` $ ``, `` <end> `` |
| `block_swap_corners` | Visual-block: move cursor to the opposite corner (o) | select: `` o `` |
| `block_swap_columns` | Visual-block: move cursor to the other column edge (O) | select: `` O `` |
| `move_next_word_start` | Move to start of next word | normal: `` <A-f> ``, `` <C-right> ``, `` <S-right> ``, insert: `` <A-f> ``, `` <C-right> ``, `` <S-right> `` |
| `move_prev_word_start` | Move to start of previous word | normal: `` <A-b> ``, `` <C-left> ``, `` <S-left> ``, insert: `` <A-b> ``, `` <C-left> ``, `` <S-left> `` |
| `move_next_word_end` | Move to end of next word |  |
| `move_prev_word_end` | Move to end of previous word |  |
| `move_next_long_word_start` | Move to start of next long word |  |
| `move_prev_long_word_start` | Move to start of previous long word |  |
| `move_next_long_word_end` | Move to end of next long word |  |
| `move_prev_long_word_end` | Move to end of previous long word |  |
| `move_next_sub_word_start` | Move to start of next sub word |  |
| `move_prev_sub_word_start` | Move to start of previous sub word |  |
| `move_next_sub_word_end` | Move to end of next sub word |  |
| `move_prev_sub_word_end` | Move to end of previous sub word |  |
| `vim_move_next_word_start` | Move to start of next word (vim caret) |  |
| `vim_move_prev_word_start` | Move to start of previous word (vim caret) |  |
| `vim_move_next_word_end` | Move to end of next word (vim caret) |  |
| `vim_move_prev_word_end` | Move to end of previous word (vim caret) | normal: `` ge `` |
| `vim_move_next_long_word_start` | Move to start of next long word (vim caret) | normal: `` W `` |
| `vim_move_prev_long_word_start` | Move to start of previous long word (vim caret) | normal: `` B `` |
| `vim_move_next_long_word_end` | Move to end of next long word (vim caret) | normal: `` E `` |
| `vim_move_prev_long_word_end` | Move to end of previous long word (vim caret) | normal: `` gE `` |
| `move_parent_node_end` | Move to end of the parent node | normal: `` <space>k$ ``, select: `` <space>k$ `` |
| `move_parent_node_start` | Move to beginning of the parent node | normal: `` <space>k0 ``, select: `` <space>k0 `` |
| `extend_next_word_start` | Extend to start of next word |  |
| `extend_prev_word_start` | Extend to start of previous word |  |
| `extend_next_word_end` | Extend to end of next word |  |
| `extend_prev_word_end` | Extend to end of previous word |  |
| `extend_next_long_word_start` | Extend to start of next long word |  |
| `extend_prev_long_word_start` | Extend to start of previous long word |  |
| `extend_next_long_word_end` | Extend to end of next long word |  |
| `extend_prev_long_word_end` | Extend to end of prev long word |  |
| `extend_next_sub_word_start` | Extend to start of next sub word |  |
| `extend_prev_sub_word_start` | Extend to start of previous sub word |  |
| `extend_next_sub_word_end` | Extend to end of next sub word |  |
| `extend_prev_sub_word_end` | Extend to end of prev sub word |  |
| `extend_parent_node_end` | Extend to end of the parent node |  |
| `extend_parent_node_start` | Extend to beginning of the parent node |  |
| `find_till_char` | Move till next occurrence of char |  |
| `find_next_char` | Move to next occurrence of char |  |
| `extend_till_char` | Extend till next occurrence of char |  |
| `extend_next_char` | Extend to next occurrence of char |  |
| `till_prev_char` | Move till previous occurrence of char |  |
| `find_prev_char` | Move to previous occurrence of char |  |
| `sneak_forward` | Sneak: jump forward to a two-character sequence |  |
| `sneak_backward` | Sneak: jump backward to a two-character sequence |  |
| `sneak_or_substitute_char` | Sneak forward, or substitute char when vim-sneak is off | normal: `` s `` |
| `sneak_or_substitute_line` | Sneak backward, or substitute line when vim-sneak is off | normal: `` S `` |
| `extend_till_prev_char` | Extend till previous occurrence of char |  |
| `extend_prev_char` | Extend to previous occurrence of char |  |
| `repeat_last_motion` | Repeat last motion | normal: `` <C-x>z ``, select: `` <C-x>z ``, insert: `` <C-x>z `` |
| `repeat_find_char` | Repeat last find in same direction (;) | normal: `` ; ``, select: `` ; `` |
| `repeat_find_char_reverse` | Repeat last find in opposite direction (,) | normal: `` , ``, select: `` , `` |
| `replace` | Replace with new char | normal: `` r ``, select: `` r `` |
| `switch_case` | Switch (toggle) case | select: `` ~ `` |
| `switch_case_forward` | Toggle case and advance cursor (vim ~) | normal: `` ~ `` |
| `switch_to_uppercase` | Switch to uppercase | normal: `` <space>xU ``, `` <C-x><C-u> ``, select: `` <space>xU ``, `` <C-x><C-u> ``, insert: `` <C-x><C-u> `` |
| `switch_to_lowercase` | Switch to lowercase | normal: `` <space>xu ``, `` <C-x><C-l> ``, select: `` <space>xu ``, `` <C-x><C-l> ``, insert: `` <C-x><C-l> `` |
| `upcase_word` | Upper-case the word after point (emacs upcase-word, M-u) |  |
| `downcase_word` | Lower-case the word after point (emacs downcase-word, M-l) |  |
| `capitalize_word` | Capitalize the word after point (emacs capitalize-word, M-c) |  |
| `capitalize_region` | Title-case every word in the region (emacs capitalize-region) |  |
| `upcase_initials_region` | Upper-case the first letter of each word in the region (emacs upcase-initials-region) |  |
| `page_up` | Move page up | normal: `` z^ ``, `` <A-v> ``, `` <C-b> ``, `` <C-x>[ ``, `` <S-up> ``, `` <pageup> ``, `` <S-minus> ``, select: `` <C-x>[ ``, insert: `` <A-v> ``, `` <C-x>[ ``, `` <S-up> ``, `` <pageup> `` |
| `page_down` | Move page down | normal: `` z+ ``, `` <C-f> ``, `` <S-+> ``, `` <C-x>] ``, `` <S-ret> ``, `` <S-down> ``, `` <pagedown> ``, select: `` <C-x>] ``, insert: `` <C-x>] ``, `` <S-down> ``, `` <pagedown> `` |
| `half_page_up` | Move half page up |  |
| `half_page_down` | Move half page down |  |
| `page_cursor_up` | Move page and cursor up |  |
| `page_cursor_down` | Move page and cursor down |  |
| `page_cursor_half_up` | Move page and cursor half up | normal: `` <C-u> `` |
| `page_cursor_half_down` | Move page and cursor half down | normal: `` <C-d> `` |
| `select_all` | Select whole document | normal: `` <C-x>h ``, `` <C-x><C-p> ``, select: `` <C-x>h ``, `` <C-x><C-p> ``, insert: `` <C-x>h ``, `` <C-x><C-p> `` |
| `select_regex` | Select all regex matches inside selections | normal: `` <C-x>wh ``, `` <C-x>wi ``, `` <C-x>wl ``, `` <C-x>wp ``, select: `` <C-x>wh ``, `` <C-x>wi ``, `` <C-x>wl ``, `` <C-x>wp ``, insert: `` <C-x>wh ``, `` <C-x>wi ``, `` <C-x>wl ``, `` <C-x>wp `` |
| `select_all_instances` | Select all occurrences of the current selection in the buffer |  |
| `split_selection` | Split selections on regex matches |  |
| `split_selection_on_newline` | Split selection on newlines |  |
| `merge_selections` | Merge selections |  |
| `merge_consecutive_selections` | Merge consecutive selections |  |
| `search` | Search for regex pattern | normal: `` / ``, `` <C-s> ``, select: `` / `` |
| `rsearch` | Reverse search for regex pattern | normal: `` ? ``, select: `` ? `` |
| `search_next` | Select next search match | normal: `` n ``, `` gn ``, `` <space>sH ``, select: `` <space>sH `` |
| `search_prev` | Select previous search match | normal: `` N ``, `` gN `` |
| `extend_search_next` | Add next search match to selection | select: `` n `` |
| `extend_search_prev` | Add previous search match to selection | select: `` N `` |
| `add_selection_to_next_match` | Add the next occurrence of the selection as a new cursor |  |
| `select_all_occurrences` | Select every occurrence of the selection as a cursor (JetBrains Select All Occurrences) | normal: `` <space>xo ``, select: `` <space>xo `` |
| `search_selection` | Use current selection as search pattern |  |
| `search_selection_detect_word_boundaries` | Use current selection as the search pattern, automatically wrapping with `\b` on word boundaries |  |
| `make_search_word_bounded` | Modify current search to make it word bounded |  |
| `global_search` | Global search in workspace folder | normal: `` <space>/ ``, `` <space>po ``, `` <space>ps ``, `` <space>sP ``, `` <space>sb ``, `` <space>sd ``, `` <space>sf ``, `` <space>sp ``, `` <space>ss ``, `` <space>saa ``, `` <space>sab ``, `` <space>sad ``, `` <space>saf ``, `` <space>sap ``, `` <space>sgb ``, `` <space>sgd ``, `` <space>sgf ``, `` <space>sgg ``, `` <space>sgp ``, `` <space>skb ``, `` <space>skd ``, `` <space>skf ``, `` <space>skp ``, `` <space>srb ``, `` <space>srd ``, `` <space>srf ``, `` <space>srp ``, `` <space>srr ``, select: `` <space>/ ``, `` <space>po ``, `` <space>ps ``, `` <space>sP ``, `` <space>sb ``, `` <space>sd ``, `` <space>sf ``, `` <space>sp ``, `` <space>ss ``, `` <space>saa ``, `` <space>sab ``, `` <space>sad ``, `` <space>saf ``, `` <space>sap ``, `` <space>sgb ``, `` <space>sgd ``, `` <space>sgf ``, `` <space>sgg ``, `` <space>sgp ``, `` <space>skb ``, `` <space>skd ``, `` <space>skf ``, `` <space>skp ``, `` <space>srb ``, `` <space>srd ``, `` <space>srf ``, `` <space>srp ``, `` <space>srr `` |
| `global_search_symbol` | Global search seeded with the symbol under the cursor | normal: `` <space>* ``, `` <space>sB ``, `` <space>sD ``, `` <space>sF ``, `` <space>saA ``, `` <space>saB ``, `` <space>saD ``, `` <space>saF ``, `` <space>saP ``, `` <space>sgB ``, `` <space>sgF ``, `` <space>sgG ``, `` <space>skB ``, `` <space>skD ``, `` <space>skF ``, `` <space>skP ``, `` <space>srB ``, `` <space>srD ``, `` <space>srF ``, `` <space>srP ``, `` <space>srR ``, select: `` <space>* ``, `` <space>sB ``, `` <space>sD ``, `` <space>sF ``, `` <space>saA ``, `` <space>saB ``, `` <space>saD ``, `` <space>saF ``, `` <space>saP ``, `` <space>sgB ``, `` <space>sgF ``, `` <space>sgG ``, `` <space>skB ``, `` <space>skD ``, `` <space>skF ``, `` <space>skP ``, `` <space>srB ``, `` <space>srD ``, `` <space>srF ``, `` <space>srP ``, `` <space>srR `` |
| `clear_search_highlight` | Clear persistent search highlight (SPC s c) | normal: `` <C-x>wr ``, `` <space>sc ``, select: `` <C-x>wr ``, `` <space>sc ``, insert: `` <C-x>wr `` |
| `regex_convert_form` | Convert the selected regex between PCRE and Emacs forms (SPC x r c) | normal: `` <space>xrc ``, `` <space>xrep ``, `` <space>xrpe ``, select: `` <space>xrc ``, `` <space>xrep ``, `` <space>xrpe `` |
| `regex_emacs_to_rx_replace` | Convert the selected Emacs regex to rx form (SPC x r e x) | normal: `` <space>xret ``, `` <space>xrex ``, select: `` <space>xret ``, `` <space>xrex `` |
| `regex_emacs_to_rx_explain` | Explain the selected Emacs regex as rx (SPC x r e /) | normal: `` <space>xre/ ``, select: `` <space>xre/ `` |
| `regex_pcre_to_rx_replace` | Convert the selected PCRE regex to rx form (SPC x r x) | normal: `` <space>xrt ``, `` <space>xrx ``, `` <space>xrpx ``, select: `` <space>xrt ``, `` <space>xrx ``, `` <space>xrpx `` |
| `regex_pcre_to_rx_explain` | Explain the selected PCRE regex as rx (SPC x r /) | normal: `` <space>xr/ ``, `` <space>xrp/ ``, select: `` <space>xr/ ``, `` <space>xrp/ `` |
| `justify_left` | Left-justify (fill) the region (SPC x j l) | normal: `` <space>xjl ``, select: `` <space>xjl `` |
| `justify_right` | Right-justify the region (SPC x j r) | normal: `` <space>xjr ``, select: `` <space>xjr `` |
| `justify_center` | Center-justify the region (SPC x j c) | normal: `` <space>xjc ``, select: `` <space>xjc `` |
| `justify_full` | Full-justify the region (SPC x j f) | normal: `` <space>xjf ``, select: `` <space>xjf `` |
| `justify_none` | Remove justification / left-fill (SPC x j n) | normal: `` <space>xjn ``, select: `` <space>xjn `` |
| `count_words_region` | Count occurrences per word in the selection (SPC x w c) | normal: `` <space>xwc ``, select: `` <space>xwc `` |
| `goto_next_close_paren` | Go forward to next closing paren (SPC k j) | normal: `` <space>kj ``, select: `` <space>kj `` |
| `goto_prev_open_paren` | Go backward to previous opening paren (SPC k k) | normal: `` <space>kk ``, select: `` <space>kk `` |
| `ediff_windows` | Diff the two front windows side by side (SPC D w w) | normal: `` <space>Dwl ``, `` <space>Dww ``, select: `` <space>Dwl ``, `` <space>Dww `` |
| `ediff_buffer` | Diff the current buffer against a picked buffer (SPC D b b) | normal: `` <space>Dbb ``, select: `` <space>Dbb `` |
| `compare_with_clipboard` | Diff the current buffer against the clipboard (JetBrains Compare with Clipboard) | normal: `` <space>Dc ``, select: `` <space>Dc `` |
| `transpose_paragraph` | Swap the current paragraph with the previous one (SPC x t p) | normal: `` <space>xtp ``, select: `` <space>xtp `` |
| `transpose_line` | Swap the current line with the previous one (emacs transpose-lines, C-x C-t) |  |
| `move_element_right` | Swap the syntax node under the cursor with its next sibling (JetBrains Move Element Right) | normal: `` <space>x<gt> ``, select: `` <space>x<gt> `` |
| `move_element_left` | Swap the syntax node under the cursor with its previous sibling (JetBrains Move Element Left) | normal: `` <space>x<lt> ``, select: `` <space>x<lt> `` |
| `convert_indent_to_spaces` | Convert leading indentation to spaces (JetBrains Convert Indents to Spaces) |  |
| `convert_indent_to_tabs` | Convert leading indentation to tabs (JetBrains Convert Indents to Tabs) |  |
| `transpose_sexp` | Swap the current s-expression with the previous one (SPC x t e) | normal: `` <space>xte ``, select: `` <space>xte `` |
| `transpose_sentence` | Swap the current sentence with the previous one (SPC x t s) | normal: `` <space>xts ``, select: `` <space>xts `` |
| `make_3_windows` | Lay out three vertical windows (SPC w 3) | normal: `` <C-w>3 ``, `` <space>w3 ``, `` <space>u<space>w3 ``, select: `` <space>w3 ``, `` <space>u<space>w3 `` |
| `make_4_windows` | Lay out a 2x2 window grid (SPC w 4) | normal: `` <C-w>4 ``, `` <space>w4 ``, `` <space>u<space>w4 ``, select: `` <space>w4 ``, `` <space>u<space>w4 `` |
| `narrow_to_function` | Narrow the buffer to the enclosing function (SPC n f) | normal: `` <C-x>nd ``, `` <space>nf ``, select: `` <C-x>nd ``, `` <space>nf ``, insert: `` <C-x>nd `` |
| `align_at_equals` | Align region at = (SPC x a =) | normal: `` <space>xa= ``, select: `` <space>xa= `` |
| `align_at_comma` | Align region at , (SPC x a ,) | normal: `` <space>xa, ``, select: `` <space>xa, `` |
| `align_at_colon` | Align region at : (SPC x a :) | normal: `` <space>xa: ``, select: `` <space>xa: `` |
| `align_at_semicolon` | Align region at ; (SPC x a ;) | normal: `` <space>xa; ``, select: `` <space>xa; `` |
| `align_at_ampersand` | Align region at & (SPC x a &) | normal: `` <space>xa& ``, select: `` <space>xa& `` |
| `align_at_lparen` | Align region at ( (SPC x a () | normal: `` <space>xa( ``, select: `` <space>xa( `` |
| `align_at_rparen` | Align region at ) (SPC x a )) | normal: `` <space>xa) ``, select: `` <space>xa) `` |
| `align_at_lbracket` | Align region at [ (SPC x a [) | normal: `` <space>xa[ ``, select: `` <space>xa[ `` |
| `align_at_rbracket` | Align region at ] (SPC x a ]) | normal: `` <space>xa] ``, select: `` <space>xa] `` |
| `align_at_lbrace` | Align region at { (SPC x a {) | normal: `` <space>xa{ ``, select: `` <space>xa{ `` |
| `align_at_rbrace` | Align region at } (SPC x a }) | normal: `` <space>xa} ``, select: `` <space>xa} `` |
| `align_at_dot` | Align region at . (SPC x a .) | normal: `` <space>xa. ``, select: `` <space>xa. `` |
| `align_at_arithmetic` | Align region at arithmetic operators (SPC x a m) | normal: `` <space>xam ``, select: `` <space>xam `` |
| `align_at_regex` | Align region at a user-specified regexp (SPC x a r) | normal: `` <space>xar ``, select: `` <space>xar `` |
| `align_current` | Auto-align the region into columns, per blank-line section (emacs align-current) |  |
| `align_entire` | Auto-align the whole region into columns as one section (emacs align-entire) |  |
| `align_left_at_char` | Left-align region at a typed delimiter (SPC x a l) | normal: `` <space>xal ``, select: `` <space>xal `` |
| `align_right_at_char` | Right-align region at a typed delimiter (SPC x a L) | normal: `` <space>xaL ``, select: `` <space>xaL `` |
| `buffer_to_window_1` | Move current buffer to window 1 (SPC b . 1) | normal: `` <space>b.1 ``, select: `` <space>b.1 `` |
| `buffer_to_window_2` | Move current buffer to window 2 (SPC b . 2) | normal: `` <space>b.2 ``, select: `` <space>b.2 `` |
| `buffer_to_window_3` | Move current buffer to window 3 (SPC b . 3) | normal: `` <space>b.3 ``, select: `` <space>b.3 `` |
| `buffer_to_window_4` | Move current buffer to window 4 (SPC b . 4) | normal: `` <space>b.4 ``, select: `` <space>b.4 `` |
| `buffer_to_window_5` | Move current buffer to window 5 (SPC b . 5) | normal: `` <space>b.5 ``, select: `` <space>b.5 `` |
| `buffer_to_window_6` | Move current buffer to window 6 (SPC b . 6) | normal: `` <space>b.6 ``, select: `` <space>b.6 `` |
| `buffer_to_window_7` | Move current buffer to window 7 (SPC b . 7) | normal: `` <space>b.7 ``, select: `` <space>b.7 `` |
| `buffer_to_window_8` | Move current buffer to window 8 (SPC b . 8) | normal: `` <space>b.8 ``, select: `` <space>b.8 `` |
| `buffer_to_window_9` | Move current buffer to window 9 (SPC b . 9) | normal: `` <space>b.9 ``, select: `` <space>b.9 `` |
| `goto_window_1` | Go to window 1 (SPC 1) | normal: `` <C-w>.1 ``, `` <space>1 ``, `` <space>w.1 ``, `` <space>b.<C-1> ``, select: `` <space>1 ``, `` <space>w.1 ``, `` <space>b.<C-1> `` |
| `goto_window_2` | Go to window 2 (SPC 2) | normal: `` <C-w>.2 ``, `` <space>2 ``, `` <space>w.2 ``, `` <space>b.<C-2> ``, select: `` <space>2 ``, `` <space>w.2 ``, `` <space>b.<C-2> `` |
| `goto_window_3` | Go to window 3 (SPC 3) | normal: `` <C-w>.3 ``, `` <space>3 ``, `` <space>w.3 ``, `` <space>b.<C-3> ``, select: `` <space>3 ``, `` <space>w.3 ``, `` <space>b.<C-3> `` |
| `goto_window_4` | Go to window 4 (SPC 4) | normal: `` <C-w>.4 ``, `` <space>4 ``, `` <space>w.4 ``, `` <space>b.<C-4> ``, select: `` <space>4 ``, `` <space>w.4 ``, `` <space>b.<C-4> `` |
| `goto_window_5` | Go to window 5 (SPC 5) | normal: `` <C-w>.5 ``, `` <space>5 ``, `` <space>w.5 ``, `` <space>b.<C-5> ``, select: `` <space>5 ``, `` <space>w.5 ``, `` <space>b.<C-5> `` |
| `goto_window_6` | Go to window 6 (SPC 6) | normal: `` <C-w>.6 ``, `` <space>6 ``, `` <space>w.6 ``, `` <space>b.<C-6> ``, select: `` <space>6 ``, `` <space>w.6 ``, `` <space>b.<C-6> `` |
| `goto_window_7` | Go to window 7 (SPC 7) | normal: `` <C-w>.7 ``, `` <space>7 ``, `` <space>w.7 ``, `` <space>b.<C-7> ``, select: `` <space>7 ``, `` <space>w.7 ``, `` <space>b.<C-7> `` |
| `goto_window_8` | Go to window 8 (SPC 8) | normal: `` <C-w>.8 ``, `` <space>8 ``, `` <space>w.8 ``, `` <space>b.<C-8> ``, select: `` <space>8 ``, `` <space>w.8 ``, `` <space>b.<C-8> `` |
| `goto_window_9` | Go to window 9 (SPC 9) | normal: `` <C-w>.9 ``, `` <space>9 ``, `` <space>w.9 ``, `` <space>b.<C-9> ``, select: `` <space>9 ``, `` <space>w.9 ``, `` <space>b.<C-9> `` |
| `delete_window_and_buffer` | Close window and kill its buffer (SPC w . x) | normal: `` <C-w>.x ``, `` <space>w.x ``, `` <space>u<space>bD ``, `` <space>u<space>wD ``, select: `` <space>w.x ``, `` <space>u<space>bD ``, `` <space>u<space>wD `` |
| `eval_elisp_region` | Evaluate the selection as elisp (SPC m e r) | normal: `` <space>mer ``, select: `` <space>mer `` |
| `eval_elisp_buffer` | Evaluate the buffer as elisp (SPC m e b) | normal: `` <space>meb ``, select: `` <space>meb `` |
| `eval_elisp_line` | Evaluate the current line as elisp (SPC m e e) | normal: `` <C-x><C-e> ``, `` <space>me$ ``, `` <space>mee ``, `` <space>mel ``, select: `` <C-x><C-e> ``, `` <space>me$ ``, `` <space>mee ``, `` <space>mel ``, insert: `` <C-x><C-e> `` |
| `eval_elisp_defun` | Evaluate the enclosing form as elisp (SPC m e f) | normal: `` <space>mec ``, `` <space>mef ``, select: `` <space>mec ``, `` <space>mef `` |
| `layout_create` | Create a new window-layout from the current windows (SPC l l) | normal: `` <C-x>rf ``, `` <C-x>rw ``, `` <space>Fn ``, `` <space>ll ``, `` <space>lww ``, select: `` <C-x>rf ``, `` <C-x>rw ``, `` <space>Fn ``, `` <space>ll ``, `` <space>lww ``, insert: `` <C-x>rf ``, `` <C-x>rw `` |
| `layout_next` | Switch to the next layout (SPC l n) | normal: `` <space>ln ``, `` <space>lwl ``, `` <space>lwn ``, `` <space>l<C-l> ``, select: `` <space>ln ``, `` <space>lwl ``, `` <space>lwn ``, `` <space>l<C-l> `` |
| `layout_prev` | Switch to the previous layout (SPC l p) | normal: `` <space>lN ``, `` <space>lp ``, `` <space>lwN ``, `` <space>lwh ``, `` <space>lwp ``, `` <space>l<C-h> ``, select: `` <space>lN ``, `` <space>lp ``, `` <space>lwN ``, `` <space>lwh ``, `` <space>lwp ``, `` <space>l<C-h> `` |
| `layout_last` | Switch to the last-used layout (SPC l TAB) | normal: `` <space>l<tab> ``, `` <space>lw<tab> ``, select: `` <space>l<tab> ``, `` <space>lw<tab> `` |
| `layout_default` | Switch to the default (first) layout (SPC l h) | normal: `` <space>lh ``, select: `` <space>lh `` |
| `layout_delete` | Delete the current layout, keeping its buffers (SPC l d) | normal: `` <space>lD ``, `` <space>lX ``, `` <space>ld ``, `` <space>lx ``, `` <space>lwd ``, select: `` <space>lD ``, `` <space>lX ``, `` <space>ld ``, `` <space>lx ``, `` <space>lwd `` |
| `layout_save` | Save layouts to disk (SPC l s) | normal: `` <space>lS ``, select: `` <space>lS `` |
| `layout_rename` | Rename the current layout (SPC l R) | normal: `` <space>lR ``, `` <space>lwR ``, select: `` <space>lR ``, `` <space>lwR `` |
| `layout_load` | Load layouts from disk (SPC l L) | normal: `` <space>lL ``, `` <space>lo ``, select: `` <space>lL ``, `` <space>lo `` |
| `layout_goto_1` | Switch to layout 1 (SPC l 1) | normal: `` <space>l1 ``, `` <space>lw1 ``, `` <space>l<C-1> ``, `` <space>lw<C-1> ``, select: `` <space>l1 ``, `` <space>lw1 ``, `` <space>l<C-1> ``, `` <space>lw<C-1> `` |
| `layout_goto_2` | Switch to layout 2 (SPC l 2) | normal: `` <space>l2 ``, `` <space>lw2 ``, `` <space>l<C-2> ``, `` <space>lw<C-2> ``, select: `` <space>l2 ``, `` <space>lw2 ``, `` <space>l<C-2> ``, `` <space>lw<C-2> `` |
| `layout_goto_3` | Switch to layout 3 (SPC l 3) | normal: `` <space>l3 ``, `` <space>lw3 ``, `` <space>l<C-3> ``, `` <space>lw<C-3> ``, select: `` <space>l3 ``, `` <space>lw3 ``, `` <space>l<C-3> ``, `` <space>lw<C-3> `` |
| `layout_goto_4` | Switch to layout 4 (SPC l 4) | normal: `` <space>l4 ``, `` <space>lw4 ``, `` <space>l<C-4> ``, `` <space>lw<C-4> ``, select: `` <space>l4 ``, `` <space>lw4 ``, `` <space>l<C-4> ``, `` <space>lw<C-4> `` |
| `layout_goto_5` | Switch to layout 5 (SPC l 5) | normal: `` <space>l5 ``, `` <space>lw5 ``, `` <space>l<C-5> ``, `` <space>lw<C-5> ``, select: `` <space>l5 ``, `` <space>lw5 ``, `` <space>l<C-5> ``, `` <space>lw<C-5> `` |
| `layout_goto_6` | Switch to layout 6 (SPC l 6) | normal: `` <space>l6 ``, `` <space>lw6 ``, `` <space>l<C-6> ``, `` <space>lw<C-6> ``, select: `` <space>l6 ``, `` <space>lw6 ``, `` <space>l<C-6> ``, `` <space>lw<C-6> `` |
| `layout_goto_7` | Switch to layout 7 (SPC l 7) | normal: `` <space>l7 ``, `` <space>lw7 ``, `` <space>l<C-7> ``, `` <space>lw<C-7> ``, select: `` <space>l7 ``, `` <space>lw7 ``, `` <space>l<C-7> ``, `` <space>lw<C-7> `` |
| `layout_goto_8` | Switch to layout 8 (SPC l 8) | normal: `` <space>l8 ``, `` <space>lw8 ``, `` <space>l<C-8> ``, `` <space>lw<C-8> ``, select: `` <space>l8 ``, `` <space>lw8 ``, `` <space>l<C-8> ``, `` <space>lw<C-8> `` |
| `layout_goto_9` | Switch to layout 9 (SPC l 9) | normal: `` <space>l9 ``, `` <space>lw9 ``, `` <space>l<C-9> ``, `` <space>lw<C-9> ``, select: `` <space>l9 ``, `` <space>lw9 ``, `` <space>l<C-9> ``, `` <space>lw<C-9> `` |
| `toggle_modeline_position` | Toggle cursor position in the mode line (SPC t m p) | normal: `` <space>tmp ``, select: `` <space>tmp `` |
| `toggle_modeline_vcs` | Toggle version-control info in the mode line (SPC t m v) | normal: `` <space>tmv ``, select: `` <space>tmv `` |
| `toggle_centered_cursor` | Keep the cursor vertically centered (SPC t -) | normal: `` <space>t<minus> ``, select: `` <space>t<minus> `` |
| `toggle_hl_line` | Highlight the current line (emacs hl-line-mode / global-hl-line-mode) |  |
| `toggle_electric_pair` | Auto-insert matching close delimiters (emacs electric-pair-mode) |  |
| `toggle_auto_revert` | Reload buffers when their file changes on disk (emacs auto-revert-mode) |  |
| `set_fill_prefix` | Set the fill-prefix from line start to point (emacs set-fill-prefix) |  |
| `set_goal_column` | Make vertical motion stick to the current column (emacs set-goal-column) |  |
| `toggle_fill_column` | Toggle a fill-column ruler (SPC t f) | normal: `` <C-x>f ``, `` <space>tf ``, select: `` <C-x>f ``, `` <space>tf ``, insert: `` <C-x>f `` |
| `toggle_long_line_marker` | Toggle an 80th-column ruler (SPC t 8) | normal: `` <space>t8 ``, `` <space>t<C-8> ``, select: `` <space>t8 ``, `` <space>t<C-8> `` |
| `toggle_soft_wrap` | Toggle soft-wrap of long lines (IntelliJ View > Soft-Wrap) | normal: `` <C-x>xt ``, select: `` <C-x>xt ``, insert: `` <C-x>xt `` |
| `toggle_whitespace_render` | Toggle rendering of whitespace characters (IntelliJ View > Show Whitespaces) |  |
| `toggle_line_numbers` | Toggle the line-numbers gutter (IntelliJ View > Show Line Numbers) |  |
| `toggle_indent_guides` | Toggle indentation guides (IntelliJ View > Show Indent Guides) |  |
| `toggle_inlay_hints` | Toggle display of LSP inlay hints (IntelliJ View > Inlay Hints) |  |
| `toggle_auto_highlight` | Toggle automatic symbol-under-cursor highlight (SPC t h a) | normal: `` <C-x>w. ``, `` <C-x>wb ``, `` <space>tha ``, select: `` <C-x>w. ``, `` <C-x>wb ``, `` <space>tha ``, insert: `` <C-x>w. ``, `` <C-x>wb `` |
| `toggle_syntax_highlighting` | Toggle syntax highlighting for the current buffer (SPC t h s) | normal: `` <space>ths ``, select: `` <space>ths `` |
| `toggle_diagnostics` | Toggle diagnostics display / flycheck (SPC t s) | normal: `` <space>ts ``, select: `` <space>ts `` |
| `ediff_file` | Diff a prompted file against the current buffer (SPC D f f) | normal: `` <space>Dff ``, select: `` <space>Dff `` |
| `ediff_3_files` | 3-way diff of three prompted files, read-only (SPC D f 3) | normal: `` <space>Df3 ``, select: `` <space>Df3 `` |
| `ediff_regions` | Ediff two regions linewise: mark A, then diff B (SPC D r l) | normal: `` <space>Drl ``, select: `` <space>Drl `` |
| `ediff_merge_file` | Merge a picked file into the current buffer (editable, SPC D m f f) | normal: `` <space>Dmff ``, select: `` <space>Dmff `` |
| `ediff_3_buffers` | 3-way diff of three open buffers, read-only (SPC D b 3) | normal: `` <space>Db3 ``, select: `` <space>Db3 `` |
| `kill_buffers_by_regex` | Kill all buffers whose name matches a regex (SPC b M) | normal: `` <space>b<C-D> ``, select: `` <space>b<C-D> `` |
| `narrow_to_page` | Narrow the buffer to the current page (SPC n p) | normal: `` <C-x>np ``, `` <space>np ``, select: `` <C-x>np ``, `` <space>np ``, insert: `` <C-x>np `` |
| `copy_file` | Copy the current file to a prompted destination (SPC f c) | normal: `` <space>fc ``, select: `` <space>fc `` |
| `find_file_replace_buffer` | Open a file and replace the current buffer with it (SPC f A) | normal: `` <space>fA ``, select: `` <space>fA `` |
| `open_file_literally` | Open a file with no syntax/language (fundamental mode, SPC f l) | normal: `` <space>fl ``, select: `` <space>fl `` |
| `locate_file` | Locate a file via system locate/mdfind and open it (SPC f L) | normal: `` <space>fL ``, select: `` <space>fL `` |
| `edit_project_config` | Edit the project-local .zemacs/config.toml (SPC p e) | normal: `` <space>pe ``, select: `` <space>pe `` |
| `man_page_search` | Search man pages via apropos and view the selected page (SPC h m) | normal: `` <C-h>S ``, `` <space>hm ``, select: `` <C-h>S ``, `` <space>hm ``, insert: `` <C-h>S `` |
| `info_search` | Search GNU info manuals (apropos) and view the selected node (SPC h i) | normal: `` <C-h>i ``, `` <C-h>4i ``, `` <space>hi ``, select: `` <C-h>i ``, `` <C-h>4i ``, `` <space>hi ``, insert: `` <C-h>i ``, `` <C-h>4i `` |
| `diagnostics_verify_setup` | Report the buffer's diagnostics/LSP setup (SPC e v) | normal: `` <space>ev ``, select: `` <space>ev `` |
| `clear_diagnostics` | Clear all diagnostics for the current buffer (SPC e c) | normal: `` <space>ec ``, select: `` <space>ec `` |
| `ai_chat` | Ask the AI provider about the selection/buffer (SPC a i) | normal: `` <space>ai ``, select: `` <space>ai `` |
| `ai_chat_panel` | Open the streaming AI chat drawer (SPC a p) | normal: `` <space>ap ``, select: `` <space>ap `` |
| `ai_model_picker` | Pick the AI model at runtime (SPC a m) |  |
| `toggle_ai_privacy` | Toggle AI privacy mode (SPC a P) | normal: `` <space>aP ``, select: `` <space>aP `` |
| `ai_apply_block` | Apply the last AI code block to the selection (SPC a y) | normal: `` <space>ay ``, select: `` <space>ay `` |
| `ai_add_file_context` | Add a file as @context for the next AI chat (SPC a @) | normal: `` <space>a@ ``, select: `` <space>a@ `` |
| `ai_codebase_context` | Add codebase-search results as @context (SPC a b) | normal: `` <space>ab ``, select: `` <space>ab `` |
| `ai_symbol_context` | Add the symbol-under-cursor's definitions as @context (SPC a s) | normal: `` <space>as ``, select: `` <space>as `` |
| `ai_terminal_command` | Generate a shell command from natural language (SPC a k) | normal: `` <space>ak ``, select: `` <space>ak `` |
| `ai_inline_edit` | AI inline edit/generate on the selection (SPC a e) | normal: `` <space>ae ``, select: `` <space>ae `` |
| `ai_inline_edit_preview` | AI inline edit with a diff preview (SPC a E) | normal: `` <space>aE ``, select: `` <space>aE `` |
| `ai_accept_edit` | Accept the pending AI inline-edit preview (SPC a .) | normal: `` <space>a. ``, select: `` <space>a. `` |
| `ai_explain` | Explain the selected code with AI (SPC a x) | normal: `` <space>ax ``, select: `` <space>ax `` |
| `ai_generate_tests` | Generate tests for the selection with AI (SPC a u) | normal: `` <space>au ``, select: `` <space>au `` |
| `ai_commit_message` | Generate a git commit message with AI (SPC a c) |  |
| `ai_agent` | Run the autonomous AI agent on a task (SPC a a) | normal: `` <space>aa ``, select: `` <space>aa `` |
| `ai_agent_review` | Toggle agent review (dry-run) mode — propose changes without applying (SPC a R) | normal: `` <space>aR ``, select: `` <space>aR `` |
| `ai_complete` | AI code completion at the cursor (SPC a TAB) | normal: `` <space>a<tab> ``, select: `` <space>a<tab> `` |
| `ai_docs_context` | @docs keyword-search over the docs directory for AI context (SPC a D) | normal: `` <space>aD ``, select: `` <space>aD `` |
| `ai_web_context` | @web live web-search results as AI context (SPC a w) | normal: `` <space>aw ``, select: `` <space>aw `` |
| `toggle_ai_autocomplete` | Toggle real-time AI ghost-text autocomplete (SPC a g) | normal: `` <space>ag ``, select: `` <space>ag `` |
| `ghost_text_accept` | Accept the AI ghost-text suggestion, else Tab (insert mode) | insert: `` <tab> `` |
| `ghost_text_accept_word` | Accept the next word of the AI ghost-text suggestion (partial accept) | insert: `` <A-right> `` |
| `ai_revert_agent` | Revert the workspace to the last agent checkpoint (SPC a z) | normal: `` <space>az ``, select: `` <space>az `` |
| `ai_fix` | AI-fix the diagnostic(s) on the current line (SPC a F) | normal: `` <space>aF ``, select: `` <space>aF `` |
| `describe_diagnostics_checker` | Describe the buffer's checkers/language servers (SPC e h) | normal: `` <space>eh ``, select: `` <space>eh `` |
| `describe_text_properties` | Describe the tree-sitter node stack at the cursor (SPC h d t) | normal: `` <space>hdt ``, select: `` <space>hdt `` |
| `copy_system_info` | Copy system info (version/OS/arch) to the clipboard (SPC h d s) | normal: `` <space>hds ``, select: `` <space>hds `` |
| `copy_last_keys` | Copy the most recently pressed keys to the clipboard (SPC h d l) | normal: `` <space>hdl ``, select: `` <space>hdl `` |
| `ace_window` | Jump to a window by its number, prompted (ace-window, SPC w . a) | normal: `` <C-w>.a ``, `` <space>w.a ``, select: `` <space>w.a `` |
| `browse_news` | Browse zemacs release notes / NEWS (SPC h n) | normal: `` <C-h>n ``, `` <space>hn ``, `` <C-h><C-n> ``, select: `` <C-h>n ``, `` <space>hn ``, `` <C-h><C-n> ``, insert: `` <C-h>n ``, `` <C-h><C-n> `` |
| `browse_faq` | Browse the zemacs FAQ (SPC h f) | normal: `` <space>hf ``, `` <C-h><C-p> ``, select: `` <space>hf ``, `` <C-h><C-p> ``, insert: `` <C-h><C-p> `` |
| `layer_search` | Search zemacs capability areas / layers (SPC h l) | normal: `` <space>hl ``, select: `` <space>hl `` |
| `show_environment` | Show the editor's environment variables (SPC f e e) | normal: `` <space>fee ``, select: `` <space>fee `` |
| `reimport_shell_env` | Re-import the shell environment into the editor (SPC f e C-e) | normal: `` <space>feE ``, `` <space>fe<C-e> ``, select: `` <space>feE ``, `` <space>fe<C-e> `` |
| `goto_buffer_window` | Focus the window already showing a chosen buffer (SPC b w) | normal: `` <space>bW ``, select: `` <space>bW `` |
| `git_file_dispatch` | Magit-style file operations dispatch for the current file (SPC g f m) | normal: `` <C-x>vG ``, `` <C-x>vi ``, `` <C-x>vu ``, `` <space>gfm ``, select: `` <C-x>vG ``, `` <C-x>vi ``, `` <C-x>vu ``, `` <space>gfm ``, insert: `` <C-x>vG ``, `` <C-x>vi ``, `` <C-x>vu `` |
| `describe_current_modes` | Describe the current editor/buffer modes (SPC h d m) | normal: `` <C-h>m ``, `` <space>hdm ``, select: `` <C-h>m ``, `` <space>hdm ``, insert: `` <C-h>m `` |
| `describe_command` | Describe a command — its doc and key bindings (C-h f) | normal: `` <C-h>a ``, `` <C-h>d ``, `` <C-h>f ``, `` <C-h>o ``, `` <C-h>x ``, select: `` <C-h>a ``, `` <C-h>d ``, `` <C-h>f ``, `` <C-h>o ``, `` <C-h>x ``, insert: `` <C-h>a ``, `` <C-h>d ``, `` <C-h>f ``, `` <C-h>o ``, `` <C-h>x `` |
| `where_is` | Show the keys a command is bound to (C-h w) | normal: `` <C-h>w ``, select: `` <C-h>w ``, insert: `` <C-h>w `` |
| `describe_key` | Describe a key — pick a binding, show its command and doc (C-h k) | normal: `` <C-h>c ``, `` <C-h>k ``, select: `` <C-h>c ``, `` <C-h>k ``, insert: `` <C-h>c ``, `` <C-h>k `` |
| `describe_bindings` | List every key binding of the current mode (C-h b) | normal: `` <C-h>b ``, select: `` <C-h>b ``, insert: `` <C-h>b `` |
| `describe_coding_system` | Describe the buffer's coding system / encoding (C-h C) | normal: `` <C-h>C ``, select: `` <C-h>C ``, insert: `` <C-h>C `` |
| `describe_language_environment` | Describe the language environment / locale (C-h L) | normal: `` <C-h>L ``, select: `` <C-h>L ``, insert: `` <C-h>L `` |
| `describe_syntax` | Describe the buffer's syntax / tree-sitter status (C-h s) | normal: `` <C-h>s ``, select: `` <C-h>s ``, insert: `` <C-h>s `` |
| `view_lossage` | Show the recently pressed keys (C-h l) | normal: `` <C-h>l ``, select: `` <C-h>l ``, insert: `` <C-h>l `` |
| `describe_char` | Describe the character after point — code, Unicode block, category (emacs describe-char, C-u C-x =) |  |
| `view_hello_file` | Show a multi-script greeting sample (emacs view-hello-file, C-h h) | normal: `` <C-h>h ``, select: `` <C-h>h ``, insert: `` <C-h>h `` |
| `view_echo_area_messages` | Show the last echo-area message (emacs view-echo-area-messages, C-h e) | normal: `` <C-h>e ``, select: `` <C-h>e ``, insert: `` <C-h>e `` |
| `describe_copying` | Show zemacs's copying license, the GPL (emacs describe-copying, C-h C-c) | normal: `` <C-h><C-c> ``, select: `` <C-h><C-c> ``, insert: `` <C-h><C-c> `` |
| `describe_distribution` | How to get zemacs / GNU software (emacs describe-distribution, C-h C-d) | normal: `` <C-h><C-d> ``, select: `` <C-h><C-d> ``, insert: `` <C-h><C-d> `` |
| `describe_gnu_project` | Open the GNU project page (emacs describe-gnu-project, C-h g) | normal: `` <C-h>g ``, select: `` <C-h>g ``, insert: `` <C-h>g `` |
| `describe_no_warranty` | Show the GPL no-warranty sections (emacs describe-no-warranty, C-h C-w) | normal: `` <C-h><C-w> ``, select: `` <C-h><C-w> ``, insert: `` <C-h><C-w> `` |
| `view_emacs_faq` | Open the GNU Emacs FAQ (emacs view-emacs-FAQ, C-h C-f) | normal: `` <C-h><C-f> ``, select: `` <C-h><C-f> ``, insert: `` <C-h><C-f> `` |
| `view_emacs_todo` | Open the Emacs TODO list (emacs view-emacs-todo) |  |
| `view_emacs_problems` | Open the Emacs known-problems file (emacs view-emacs-problems) |  |
| `view_emacs_debugging` | Open the Emacs debugging manual (emacs view-emacs-debugging) |  |
| `view_order_manuals` | Open where to get the GNU manuals (emacs view-order-manuals) |  |
| `view_external_packages` | Open GNU ELPA / external packages (emacs view-external-packages) |  |
| `describe_keymap` | List every binding of the current mode's keymap (emacs describe-keymap) |  |
| `describe_prefix_bindings` | List the sub-bindings of a prefix (emacs describe-prefix-bindings) |  |
| `describe_categories` | List the character categories zemacs recognises (emacs describe-categories) |  |
| `list_character_sets` | List the Unicode blocks zemacs knows (emacs list-character-sets) |  |
| `list_charset_chars` | List the printable characters of each Unicode block (emacs list-charset-chars) |  |
| `list_coding_systems` | List the coding systems / encodings zemacs supports (emacs list-coding-systems) |  |
| `describe_language_package` | Describe the language-support config for the buffer (SPC h d p) | normal: `` <space>hdp ``, select: `` <space>hdp `` |
| `package_search` | Search configured language packages and describe one (SPC h p) | normal: `` <C-h>P ``, `` <C-h>p ``, `` <space>hp ``, `` <C-h><C-e> ``, select: `` <C-h>P ``, `` <C-h>p ``, `` <space>hp ``, `` <C-h><C-e> ``, insert: `` <C-h>P ``, `` <C-h>p ``, `` <C-h><C-e> `` |
| `config_variable_search` | Search editor config variables, copy path on select (SPC h .) | normal: `` <C-h>v ``, `` <space>h. ``, select: `` <C-h>v ``, `` <space>h. ``, insert: `` <C-h>v `` |
| `clone_indirect_buffer` | Clone the current buffer into a shared-document split (SPC b N i) | normal: `` <space>bNI ``, `` <space>bNi ``, select: `` <space>bNI ``, `` <space>bNi `` |
| `clone_indirect_from_buffer` | Open an existing buffer in a shared-document split (SPC b N C-i) | normal: `` <space>bN<C-i> ``, select: `` <space>bN<C-i> `` |
| `open_junk_file` | Open a fresh timestamped junk file (SPC f J) | normal: `` <space>fJ ``, select: `` <space>fJ `` |
| `open_hex` | Open the current file in the hex editor (SPC f h, hexl) | normal: `` <space>fh ``, select: `` <space>fh `` |
| `open_file_external` | Open the current file with the OS default program (SPC f o) | normal: `` <space>fo ``, select: `` <space>fo `` |
| `git_init` | Initialize a new git repository (SPC g i) | normal: `` <space>gi ``, select: `` <space>gi `` |
| `view_file_at_rev` | View the current file at a branch/commit (SPC g f f) | normal: `` <C-x>v~ ``, `` <space>gff ``, select: `` <C-x>v~ ``, `` <space>gff ``, insert: `` <C-x>v~ `` |
| `extend_line` | Select current line, if already selected, extend to another line based on the anchor | normal: `` <space>kV ``, select: `` <space>kV `` |
| `extend_line_below` | Select current line, if already selected, extend to next line |  |
| `extend_line_above` | Select current line, if already selected, extend to previous line |  |
| `select_line_above` | Select current line, if already selected, extend or shrink line above based on the anchor |  |
| `select_line_below` | Select current line, if already selected, extend or shrink line below based on the anchor |  |
| `extend_to_line_bounds` | Extend selection to line bounds | select: `` V `` |
| `shrink_to_line_bounds` | Shrink selection to line bounds |  |
| `delete_selection` | Delete selection |  |
| `delete_selection_noyank` | Delete selection without yanking |  |
| `change_selection` | Change selection | select: `` c ``, `` s `` |
| `change_selection_noyank` | Change selection without yanking |  |
| `collapse_selection` | Collapse selection into single cursor |  |
| `flip_selections` | Flip selection cursor and anchor | normal: `` <C-x><C-x> ``, select: `` <C-x><C-x> ``, insert: `` <C-x><C-x> `` |
| `ensure_selections_forward` | Ensure all selections face forward |  |
| `insert_mode` | Insert before selection | normal: `` i ``, `` <ins> ``, `` <space>ki ``, select: `` <space>ki `` |
| `append_mode` | Append after selection | normal: `` a ``, select: `` A `` |
| `replace_mode` | Enter Replace mode (overtype) | normal: `` R ``, `` gR ``, insert: `` <ins> `` |
| `command_mode` | Enter command mode | normal: `` : ``, `` gQ ``, `` <space>: ``, `` <space>k: ``, select: `` : ``, `` <space>: ``, `` <space>k: `` |
| `file_picker` | Open file picker | normal: `` <space>ff ``, `` <space>pf ``, `` <space>ph ``, `` <space>pp ``, `` <C-x><C-f> ``, `` <C-x><C-r> ``, `` <C-x><C-v> ``, select: `` <space>ff ``, `` <space>pf ``, `` <space>ph ``, `` <space>pp ``, `` <C-x><C-f> ``, `` <C-x><C-r> ``, `` <C-x><C-v> ``, insert: `` <C-x><C-f> ``, `` <C-x><C-r> ``, `` <C-x><C-v> `` |
| `file_picker_in_current_buffer_directory` | Open file picker at current buffer's directory | normal: `` <C-x><C-j> ``, select: `` <C-x><C-j> ``, insert: `` <C-x><C-j> `` |
| `file_picker_in_current_directory` | Open file picker at current working directory | normal: `` <C-x>d ``, `` <C-x><C-d> ``, select: `` <C-x>d ``, `` <C-x><C-d> ``, insert: `` <C-x>d ``, `` <C-x><C-d> `` |
| `file_explorer` | Open file explorer in workspace root | normal: `` <space>ad ``, `` <space>af ``, `` <space>ft ``, `` <space>pd ``, `` <space>pt ``, `` <space>atrd ``, `` <space>atrr ``, select: `` <space>ad ``, `` <space>af ``, `` <space>ft ``, `` <space>pd ``, `` <space>pt ``, `` <space>atrd ``, `` <space>atrr `` |
| `file_explorer_in_current_buffer_directory` | Open file explorer at current buffer's directory | normal: `` <space>fd ``, `` <space>fj ``, `` <space>jD ``, `` <space>jd ``, select: `` <space>fd ``, `` <space>fj ``, `` <space>jD ``, `` <space>jd `` |
| `file_explorer_in_current_directory` | Open file explorer at current working directory |  |
| `buffer_menu` | Open the Buffer Menu (emacs buffer-menu / C-x C-b) | normal: `` <space>bM ``, select: `` <space>bM `` |
| `list_buffers` | List open buffers in the Buffer Menu (emacs list-buffers) |  |
| `calendar` | Open the Calendar month grid (emacs calendar) |  |
| `diary` | Show today's diary entries (emacs diary) |  |
| `diary_view_entries` | Show diary entries for the current date (emacs diary-view-entries) |  |
| `diary_show_all_entries` | Open the diary file (emacs diary-show-all-entries) |  |
| `diary_insert_entry` | Add a diary entry for today (emacs diary-insert-entry) |  |
| `diary_insert_weekly_entry` | Add a weekly diary entry for today (emacs diary-insert-weekly-entry) |  |
| `diary_mark_entries` | Mark calendar dates that have diary entries (emacs diary-mark-entries) |  |
| `diary_list_entries` | List diary entries for the current date (emacs diary-list-entries) |  |
| `diary_fancy_display` | Show the day's diary entries in fancy format (emacs diary-fancy-display) |  |
| `diary_print_entries` | Print the day's diary entries (emacs diary-print-entries) |  |
| `diary_day_of_year` | Report today's day-of-year and days remaining (emacs diary-day-of-year) |  |
| `diary_hebrew_date` | Today's Hebrew calendar date (emacs diary-hebrew-date) |  |
| `diary_islamic_date` | Today's Islamic calendar date (emacs diary-islamic-date) |  |
| `diary_french_date` | Today's French Revolutionary date (emacs diary-french-date) |  |
| `diary_bahai_date` | Today's Baha'i calendar date (emacs diary-bahai-date) |  |
| `diary_coptic_date` | Today's Coptic calendar date (emacs diary-coptic-date) |  |
| `diary_ethiopic_date` | Today's Ethiopic calendar date (emacs diary-ethiopic-date) |  |
| `diary_astro_day_number` | Today's astronomical (Julian) day number (emacs diary-astro-day-number) |  |
| `diary_hebrew_omer` | Report today's Omer count, if any (emacs diary-hebrew-omer) |  |
| `diary_hebrew_rosh_hodesh` | Report if today is Rosh Hodesh (emacs diary-hebrew-rosh-hodesh) |  |
| `diary_hebrew_birthday` | Today's Hebrew date for a birthday entry (emacs diary-hebrew-birthday) |  |
| `diary_insert_monthly_entry` | Add a monthly diary entry for today (emacs diary-insert-monthly-entry) |  |
| `diary_insert_yearly_entry` | Add a yearly diary entry for today (emacs diary-insert-yearly-entry) |  |
| `diary_insert_anniversary_entry` | Add a diary-anniversary entry for today (emacs diary-insert-anniversary-entry) |  |
| `diary_insert_block_entry` | Add a diary-block entry for today (emacs diary-insert-block-entry) |  |
| `diary_insert_cyclic_entry` | Add a diary-cyclic entry for today (emacs diary-insert-cyclic-entry) |  |
| `diary_hebrew_insert_entry` | Add a Hebrew-date diary entry for today (emacs diary-hebrew-insert-entry) |  |
| `diary_hebrew_insert_monthly_entry` | Add a monthly Hebrew diary entry (emacs diary-hebrew-insert-monthly-entry) |  |
| `diary_hebrew_insert_yearly_entry` | Add a yearly Hebrew diary entry (emacs diary-hebrew-insert-yearly-entry) |  |
| `diary_hebrew_insert_anniversary_entry` | Add a Hebrew anniversary diary entry (emacs diary-hebrew-insert-anniversary-entry) |  |
| `diary_islamic_insert_entry` | Add an Islamic-date diary entry for today (emacs diary-islamic-insert-entry) |  |
| `diary_islamic_insert_monthly_entry` | Add a monthly Islamic diary entry (emacs diary-islamic-insert-monthly-entry) |  |
| `diary_islamic_insert_yearly_entry` | Add a yearly Islamic diary entry (emacs diary-islamic-insert-yearly-entry) |  |
| `diary_islamic_insert_anniversary_entry` | Add an Islamic anniversary diary entry (emacs diary-islamic-insert-anniversary-entry) |  |
| `diary_bahai_insert_entry` | Add a Baha'i-date diary entry for today (emacs diary-bahai-insert-entry) |  |
| `diary_bahai_insert_monthly_entry` | Add a monthly Baha'i diary entry (emacs diary-bahai-insert-monthly-entry) |  |
| `diary_bahai_insert_yearly_entry` | Add a yearly Baha'i diary entry (emacs diary-bahai-insert-yearly-entry) |  |
| `diary_bahai_insert_anniversary_entry` | Add a Baha'i anniversary diary entry (emacs diary-bahai-insert-anniversary-entry) |  |
| `appt_add` | Add an appointment reminder (emacs appt-add) |  |
| `appt_delete` | Delete appointment reminders (emacs appt-delete) |  |
| `appt_activate` | Toggle appointment checking (emacs appt-activate) |  |
| `calendar_print_other_dates` | Report today's date in all other calendars (emacs calendar-print-other-dates) |  |
| `calendar_julian_print_date` | Today's Julian (Roman) calendar date (emacs calendar-julian-print-date) |  |
| `calendar_iso_print_date` | Today's ISO 8601 week date (emacs calendar-iso-print-date) |  |
| `calendar_hebrew_print_date` | Today's Hebrew calendar date (emacs calendar-hebrew-print-date) |  |
| `calendar_islamic_print_date` | Today's Islamic calendar date (emacs calendar-islamic-print-date) |  |
| `calendar_persian_print_date` | Today's Persian calendar date (emacs calendar-persian-print-date) |  |
| `calendar_coptic_print_date` | Today's Coptic calendar date (emacs calendar-coptic-print-date) |  |
| `calendar_ethiopic_print_date` | Today's Ethiopic calendar date (emacs calendar-ethiopic-print-date) |  |
| `calendar_french_print_date` | Today's French Revolutionary date (emacs calendar-french-print-date) |  |
| `calendar_bahai_print_date` | Today's Baha'i date, approx (emacs calendar-bahai-print-date) |  |
| `calendar_astro_print_day_number` | Astronomical (Julian) day number (emacs calendar-astro-print-day-number) |  |
| `calendar_mayan_print_date` | Today's Mayan date (emacs calendar-mayan-print-date) |  |
| `calendar_day_of_year` | Day-of-year of today (emacs calendar-day-of-year) |  |
| `calendar_goto_day_of_year` | Echo the date for a day-of-year (emacs calendar-goto-day-of-year) |  |
| `calendar_count_days_region` | Count days between two dates (emacs calendar-count-days-region) |  |
| `calendar_list_holidays` | List this year's holidays (emacs calendar-list-holidays) |  |
| `holidays` | List this year's holidays (emacs holidays) |  |
| `holiday_list` | List this year's holidays (emacs holiday-list) |  |
| `list_holidays` | List this year's holidays (emacs list-holidays) |  |
| `calendar_lunar_phases` | This month's moon phases, approx (emacs calendar-lunar-phases) |  |
| `calendar_sunrise_sunset` | Sunrise/sunset today, approx (emacs calendar-sunrise-sunset) |  |
| `calendar_other_month` | Open the Calendar at another month (emacs calendar-other-month) |  |
| `calendar_set_date_style` | Cycle american/european/iso date style (emacs calendar-set-date-style) |  |
| `calendar_hebrew_goto_date` | Echo Gregorian for a Hebrew date (emacs calendar-hebrew-goto-date) |  |
| `calendar_islamic_goto_date` | Echo Gregorian for an Islamic date (emacs calendar-islamic-goto-date) |  |
| `calendar_julian_goto_date` | Echo Gregorian for a Julian date (emacs calendar-julian-goto-date) |  |
| `calendar_iso_goto_week` | Echo Gregorian for an ISO week date (emacs calendar-iso-goto-week) |  |
| `calendar_persian_goto_date` | Echo Gregorian for a Persian date (emacs calendar-persian-goto-date) |  |
| `calendar_coptic_goto_date` | Echo Gregorian for a Coptic date (emacs calendar-coptic-goto-date) |  |
| `calendar_ethiopic_goto_date` | Echo Gregorian for an Ethiopic date (emacs calendar-ethiopic-goto-date) |  |
| `calendar_french_goto_date` | Echo Gregorian for a French Revolutionary date (emacs calendar-french-goto-date) |  |
| `calendar_bahai_goto_date` | Echo Gregorian for a Baha'i date (emacs calendar-bahai-goto-date) |  |
| `calendar_astro_goto_day_number` | Echo Gregorian for an astro day number (emacs calendar-astro-goto-day-number) |  |
| `calendar_mayan_goto_long_count` | Echo Gregorian for a Mayan long count (emacs calendar-mayan-goto-long-count) |  |
| `calc_dispatch` | Open the RPN Calc stack calculator (emacs calc / C-x *) | normal: `` <space>ac ``, select: `` <space>ac `` |
| `occur` | List lines matching a regexp in an *Occur* overlay (emacs occur / M-s o) |  |
| `isearch_forward_word` | Incremental whole-word search forward (emacs isearch-forward-word) |  |
| `isearch_forward_symbol` | Incremental whole-symbol search forward (emacs isearch-forward-symbol) |  |
| `isearch_forward_thing_at_point` | Search for the symbol/word at point (emacs isearch-forward-thing-at-point) |  |
| `isearch_forward_symbol_at_point` | Search for the symbol at point (emacs isearch-forward-symbol-at-point) |  |
| `isearch_toggle_regexp` | Toggle regexp matching for the current search (emacs isearch-toggle-regexp) |  |
| `isearch_toggle_word` | Toggle whole-word matching for the current search (emacs isearch-toggle-word) |  |
| `isearch_toggle_symbol` | Toggle whole-symbol matching for the current search (emacs isearch-toggle-symbol) |  |
| `isearch_toggle_case_fold` | Toggle case-folding for the current search (emacs isearch-toggle-case-fold) |  |
| `isearch_toggle_lax_whitespace` | Toggle lax-whitespace matching for the current search (emacs isearch-toggle-lax-whitespace) |  |
| `isearch_toggle_char_fold` | Toggle character folding (inert: no fold table) (emacs isearch-toggle-char-fold) |  |
| `isearch_toggle_invisible` | Toggle invisible-text matching (inert) (emacs isearch-toggle-invisible) |  |
| `isearch_toggle_input_method` | Input-method toggle (unsupported in zemacs) (emacs isearch-toggle-input-method) |  |
| `isearch_yank_char` | Extend the search with the next buffer char (emacs isearch-yank-char) |  |
| `isearch_yank_word_or_char` | Extend the search with the next word or char (emacs isearch-yank-word-or-char) |  |
| `isearch_yank_symbol_or_char` | Extend the search with the next symbol or char (emacs isearch-yank-symbol-or-char) |  |
| `isearch_yank_word` | Extend the search with the next word (emacs isearch-yank-word) |  |
| `isearch_yank_symbol` | Extend the search with the next symbol (emacs isearch-yank-symbol) |  |
| `isearch_yank_line` | Extend the search to end of line (emacs isearch-yank-line) |  |
| `isearch_yank_until_char` | Extend the search up to a given char (emacs isearch-yank-until-char) |  |
| `isearch_yank_kill` | Extend the search with the kill-ring top (emacs isearch-yank-kill) |  |
| `isearch_yank_pop` | Extend the search with a kill-ring entry (emacs isearch-yank-pop) |  |
| `isearch_yank_x_selection` | Extend the search with the clipboard selection (emacs isearch-yank-x-selection) |  |
| `isearch_del_char` | Shorten the search string by one char (emacs isearch-del-char) |  |
| `isearch_delete_char` | Shorten the search string by one char (emacs isearch-delete-char) |  |
| `isearch_edit_string` | Edit the search string in a prompt (emacs isearch-edit-string) |  |
| `isearch_ring_advance` | Cycle to an older search-ring entry (emacs isearch-ring-advance) |  |
| `isearch_ring_retreat` | Cycle to a newer search-ring entry (emacs isearch-ring-retreat) |  |
| `isearch_exit` | End the current incremental search (emacs isearch-exit) |  |
| `isearch_abort` | Abort the search, return to origin (emacs isearch-abort) |  |
| `isearch_cancel` | Cancel the search, return to origin (emacs isearch-cancel) |  |
| `isearch_quote_char` | Add a literal char to the search string (emacs isearch-quote-char) |  |
| `isearch_complete` | Complete the search string from history (emacs isearch-complete) |  |
| `isearch_char_by_name` | Add a char by digraph mnemonic to the search (emacs isearch-char-by-name) |  |
| `isearch_emoji_by_name` | Add a char by digraph mnemonic to the search (emacs isearch-emoji-by-name) |  |
| `isearch_occur` | Run occur with the current search pattern (emacs isearch-occur) |  |
| `isearch_query_replace` | Query-replace the current search pattern (emacs isearch-query-replace) |  |
| `isearch_query_replace_regexp` | Query-replace the current search regexp (emacs isearch-query-replace-regexp) |  |
| `isearch_highlight_regexp` | Highlight matches of the current search (emacs isearch-highlight-regexp) |  |
| `isearch_highlight_lines_matching_regexp` | List/highlight lines matching the search (emacs isearch-highlight-lines-matching-regexp) |  |
| `rmail` | Open the Rmail mail reader on ~/RMAIL (emacs rmail) | normal: `` <space>ar ``, select: `` <space>ar `` |
| `dired` | Open the Dired directory editor (emacs C-x d) |  |
| `dired_jump` | Open Dired on the current buffer's directory (emacs C-x C-j) |  |
| `dired_other_window` | Open Dired (overlay; emacs dired-other-window C-x 4 d) |  |
| `dired_jump_other_window` | Open Dired on the buffer's dir (overlay; emacs dired-jump-other-window) |  |
| `dired_at_point` | Open Dired on the file name at point (emacs dired-at-point) |  |
| `tex_insert_braces` | TeX: insert a {} brace pair (emacs tex-insert-braces) |  |
| `tex_insert_quote` | TeX: insert `` or '' smart quotes (emacs tex-insert-quote) |  |
| `tex_terminate_paragraph` | TeX: end the paragraph (emacs tex-terminate-paragraph) |  |
| `latex_insert_block` | LaTeX: insert a \begin{}..\end{} block (emacs latex-insert-block) |  |
| `latex_close_block` | LaTeX: close the innermost open environment (emacs latex-close-block) |  |
| `tex_validate` | TeX: check {}/$/begin-end balance (emacs tex-validate-region) |  |
| `tex_mode` | TeX: enter TeX editing mode (emacs tex-mode) |  |
| `latex_mode` | LaTeX: enter LaTeX editing mode (emacs latex-mode) |  |
| `latex_electric_env_pair_mode` | LaTeX: toggle electric \begin/\end pairing (emacs latex-electric-env-pair-mode) |  |
| `tex_file` | TeX: run LaTeX on the current file (emacs tex-file) |  |
| `tex_buffer` | TeX: compile the current buffer (emacs tex-buffer) |  |
| `tex_region` | TeX: compile the current file (emacs tex-region) |  |
| `tex_compile` | TeX: run LaTeX on the current file (emacs tex-compile) |  |
| `tex_bibtex_file` | TeX: run BibTeX on the current file (emacs tex-bibtex-file) |  |
| `tex_view` | TeX: open the compiled PDF (emacs tex-view) |  |
| `tex_print` | TeX: print the compiled PDF via lpr (emacs tex-print) |  |
| `tex_kill_job` | TeX: kill the running TeX job (emacs tex-kill-job) |  |
| `tex_recenter_output_buffer` | TeX: recenter the TeX output (emacs tex-recenter-output-buffer) |  |
| `sgml_tag` | SGML: wrap region/point in a <tag>..</tag> (emacs sgml-tag) |  |
| `sgml_close_tag` | SGML: close the innermost open element (emacs sgml-close-tag) |  |
| `sgml_delete_tag` | SGML: delete the enclosing tag pair, keeping content (emacs sgml-delete-tag) |  |
| `sgml_skip_tag_forward` | SGML: move past a balanced tag group (emacs sgml-skip-tag-forward) |  |
| `sgml_skip_tag_backward` | SGML: move back over a balanced tag group (emacs sgml-skip-tag-backward) |  |
| `sgml_name_char` | SGML: insert a &entity; for a character (emacs sgml-name-char) |  |
| `sgml_tag_help` | SGML: describe an HTML element (emacs sgml-tag-help) |  |
| `sgml_attributes` | SGML: insert attributes at point (emacs sgml-attributes) |  |
| `sgml_tags_invisible` | SGML: toggle tag invisibility flag (emacs sgml-tags-invisible) |  |
| `sgml_name_8bit_mode` | SGML: toggle 8-bit entity name display (emacs sgml-name-8bit-mode) |  |
| `sgml_validate` | SGML: validate the file with onsgmls/nsgmls (emacs sgml-validate) |  |
| `sgml_mode` | SGML: enter SGML editing mode (emacs sgml-mode) |  |
| `html_mode` | HTML: enter HTML editing mode (emacs html-mode) |  |
| `htmlfontify_buffer` | HTML: export the buffer as highlighted HTML (emacs htmlfontify-buffer) |  |
| `nroff_forward_text_line` | nroff: forward one text line, skip requests (emacs nroff-forward-text-line) |  |
| `nroff_backward_text_line` | nroff: backward one text line, skip requests (emacs nroff-backward-text-line) |  |
| `nroff_count_text_lines` | nroff: count text lines in region (emacs nroff-count-text-lines) |  |
| `nroff_mode` | nroff: enter nroff editing mode (emacs nroff-mode) |  |
| `nroff_electric_mode` | nroff: toggle electric request closing (emacs nroff-electric-mode) |  |
| `code_action` | Perform code action | normal: `` <space>la ``, select: `` <space>la `` |
| `extract_refactor` | Extract refactoring (method/variable/constant) via LSP (IntelliJ Extract) |  |
| `extract_function` | Extract Method/Function via LSP (IntelliJ Extract Method) |  |
| `extract_variable` | Introduce Variable via LSP (IntelliJ Introduce Variable) |  |
| `extract_constant` | Extract Constant via LSP (IntelliJ Extract Constant) |  |
| `extract_field` | Introduce Field via LSP (IntelliJ Introduce Field) |  |
| `extract_parameter` | Introduce Parameter via LSP (IntelliJ Introduce Parameter) |  |
| `inline_refactor` | Inline refactoring (variable/method) via LSP (IntelliJ Inline) |  |
| `rewrite_refactor` | Rewrite refactoring (change signature etc.) via LSP |  |
| `refactor_this` | Show all applicable refactorings (IntelliJ Refactor This) |  |
| `organize_imports` | Organize/optimize imports via LSP source action (IntelliJ Ctrl-Alt-O) | normal: `` <space>lO ``, select: `` <space>lO `` |
| `implement_methods` | Implement missing interface/trait members via LSP (IntelliJ Ctrl-I) | normal: `` <space>li ``, select: `` <space>li `` |
| `override_methods` | Override inherited members via LSP (IntelliJ Ctrl-O) | normal: `` <space>lv ``, select: `` <space>lv `` |
| `generate_code` | Generate code (getters/constructors/impls) via LSP (SPC l g) | normal: `` <space>lg ``, select: `` <space>lg `` |
| `change_signature` | Change signature refactor via LSP |  |
| `pull_members_up` | Pull members up refactor via LSP (IntelliJ) |  |
| `push_members_down` | Push members down refactor via LSP (IntelliJ) |  |
| `buffer_picker` | Open buffer picker | normal: `` <C-x>b ``, `` <C-x>4b ``, `` <C-x>6b ``, `` <C-x>wd ``, `` <C-x>xi ``, `` <space>bb ``, `` <space>lb ``, `` <space>lt ``, `` <space>pb ``, `` <C-x><C-b> ``, `` <space>b.b ``, select: `` <C-x>b ``, `` <C-x>4b ``, `` <C-x>6b ``, `` <C-x>wd ``, `` <C-x>xi ``, `` <space>bb ``, `` <space>lb ``, `` <space>lt ``, `` <space>pb ``, `` <C-x><C-b> ``, `` <space>b.b ``, insert: `` <C-x>b ``, `` <C-x>4b ``, `` <C-x>6b ``, `` <C-x>wd ``, `` <C-x>xi ``, `` <C-x><C-b> `` |
| `jumplist_picker` | Open jumplist picker | normal: `` <space>jj ``, select: `` <space>jj `` |
| `register_picker` | Browse registers and paste the chosen one | normal: `` <space>re ``, `` <space>rr ``, `` <space>ry ``, select: `` <space>re ``, `` <space>rr ``, `` <space>ry `` |
| `marks_picker` | Fuzzy-pick a vim mark and jump to it (:Marks) | normal: `` <space>fb ``, `` <space>rm ``, select: `` <space>fb ``, `` <space>rm `` |
| `buffer_line_picker` | Fuzzy-search lines in the current buffer (:BLines) | normal: `` <space>sL ``, select: `` <space>sL `` |
| `command_history_picker` | Fuzzy-pick and run a past command line (:History:) | normal: `` <space>r: ``, `` <C-x><esc><esc> ``, select: `` <space>r: ``, `` <C-x><esc><esc> ``, insert: `` <C-x><esc><esc> `` |
| `search_history_picker` | Fuzzy-pick and re-run a past search (:History/) | normal: `` <space>r/ ``, select: `` <space>r/ `` |
| `unicode_picker` | Fuzzy-pick a character/digraph and insert it (helm-unicode) | normal: `` <C-x>8 ``, `` <space>iu ``, select: `` <C-x>8 ``, `` <space>iu ``, insert: `` <C-x>8 `` |
| `git_file_log_picker` | Commit log for the current file (:BCommits) | normal: `` <C-x>va ``, `` <C-x>vh ``, `` <C-x>vl ``, `` <space>gt ``, `` <space>gfl ``, select: `` <C-x>va ``, `` <C-x>vh ``, `` <C-x>vl ``, `` <space>gt ``, `` <space>gfl ``, insert: `` <C-x>va ``, `` <C-x>vh ``, `` <C-x>vl `` |
| `git_repo_log_picker` | Commit log for the whole repo (:Commits) | normal: `` <C-x>vI ``, `` <C-x>vL ``, `` <C-x>vO ``, `` <C-x>vbl ``, `` <space>gL ``, select: `` <C-x>vI ``, `` <C-x>vL ``, `` <C-x>vO ``, `` <C-x>vbl ``, `` <space>gL ``, insert: `` <C-x>vI ``, `` <C-x>vL ``, `` <C-x>vO ``, `` <C-x>vbl `` |
| `theme_picker` | Open fuzzy theme picker with live preview | normal: `` <space>Tc ``, select: `` <space>Tc `` |
| `wrap_sexp` | Wrap the selection in parentheses | normal: `` <space>kw ``, select: `` <space>kw `` |
| `symbol_picker` | Open symbol picker | normal: `` <C-c>,l ``, `` <space>ji ``, `` <space>pg ``, `` <space>sj ``, select: `` <C-c>,l ``, `` <space>ji ``, `` <space>pg ``, `` <space>sj ``, insert: `` <C-c>,l `` |
| `syntax_symbol_picker` | Open symbol picker from syntax information |  |
| `lsp_or_syntax_symbol_picker` | Open symbol picker from LSP or syntax information |  |
| `changed_file_picker` | Open changed file picker | normal: `` <space>bm ``, select: `` <space>bm `` |
| `frecent_file_picker` | Open recent files ranked by frecency (z algorithm) | normal: `` <space>fr ``, select: `` <space>fr `` |
| `reopen_last_closed` | Reopen the most recently closed file | normal: `` <space>bu ``, `` <space>fu ``, select: `` <space>bu ``, `` <space>fu `` |
| `harpoon_add` | Pin the current file to the harpoon list | normal: `` <space>Ha ``, select: `` <space>Ha `` |
| `harpoon_jump` | Jump to the harpoon mark in slot [count] | normal: `` <space>Hj ``, select: `` <space>Hj `` |
| `harpoon_1` | Jump to harpoon mark 1 | normal: `` <space>H1 ``, select: `` <space>H1 `` |
| `harpoon_2` | Jump to harpoon mark 2 | normal: `` <space>H2 ``, select: `` <space>H2 `` |
| `harpoon_3` | Jump to harpoon mark 3 | normal: `` <space>H3 ``, select: `` <space>H3 `` |
| `harpoon_4` | Jump to harpoon mark 4 | normal: `` <space>H4 ``, select: `` <space>H4 `` |
| `harpoon_next` | Open the next harpoon mark | normal: `` <space>Hn ``, select: `` <space>Hn `` |
| `harpoon_prev` | Open the previous harpoon mark | normal: `` <space>Hp ``, select: `` <space>Hp `` |
| `bookmark_toggle` | Toggle a line bookmark (JetBrains F11) | normal: `` <space>rt ``, select: `` <space>rt `` |
| `bookmark_next` | Jump to the next line bookmark (JetBrains) | normal: `` <space>rn ``, select: `` <space>rn `` |
| `bookmark_prev` | Jump to the previous line bookmark (JetBrains) | normal: `` <space>rN ``, select: `` <space>rN `` |
| `harpoon_menu` | Open the harpoon marks menu | normal: `` <space>Hh ``, `` <space>Hl ``, select: `` <space>Hh ``, `` <space>Hl `` |
| `harpoon_remove` | Unpin the current file from harpoon | normal: `` <space>Hd ``, select: `` <space>Hd `` |
| `select_references_to_symbol_under_cursor` | Select symbol references | normal: `` <space>se ``, `` <space>sh ``, select: `` <space>se ``, `` <space>sh `` |
| `workspace_symbol_picker` | Open workspace symbol picker | normal: `` <C-c>,J ``, `` <space>jI ``, `` <space>sS ``, select: `` <C-c>,J ``, `` <space>jI ``, `` <space>sS ``, insert: `` <C-c>,J `` |
| `syntax_workspace_symbol_picker` | Open workspace symbol picker from syntax information |  |
| `lsp_or_syntax_workspace_symbol_picker` | Open workspace symbol picker from LSP or syntax information |  |
| `diagnostics_picker` | Open diagnostic picker | normal: `` <space>el ``, select: `` <space>el `` |
| `workspace_diagnostics_picker` | Open workspace diagnostic picker | normal: `` <space>eL ``, select: `` <space>eL `` |
| `last_picker` | Open last picker | normal: `` <space>' ``, `` <space>rl ``, `` <space>rs ``, `` <space>sl ``, select: `` <space>' ``, `` <space>rl ``, `` <space>rs ``, `` <space>sl `` |
| `insert_at_line_start` | Insert at start of line | normal: `` I ``, `` gI `` |
| `insert_at_line_end` | Insert at end of line | normal: `` A `` |
| `open_below` | Open new line below selection | normal: `` o `` |
| `open_above` | Open new line above selection | normal: `` O `` |
| `complete_current_statement` | Complete the current statement (close brackets, add terminator, open next line) (JetBrains) | normal: `` <C-c>; ``, select: `` <C-c>; ``, insert: `` <C-c>; `` |
| `postfix_expand` | Postfix completion: expand `expr.kw` (if/for/while/match/let/return/not/…) (JetBrains) | normal: `` <C-c>. ``, select: `` <C-c>. ``, insert: `` <C-c>. `` |
| `normal_mode` | Enter normal mode | normal: `` <C-\><C-g> ``, `` <C-\><C-n> ``, `` <space>b.q ``, select: `` <space>b.q `` |
| `select_mode` | Enter selection extend mode | normal: `` v ``, `` gh ``, `` <C-space> ``, `` <space>kv ``, `` <space>k<C-v> ``, select: `` <space>kv ``, `` <space>k<C-v> `` |
| `exit_select_mode` | Exit selection mode |  |
| `goto_definition` | Goto definition | normal: `` g] ``, `` gd ``, `` <C-]> ``, `` <C-w>] ``, `` ]<C-d> ``, `` g<C-]> ``, `` <C-c>,j ``, `` <C-w>g] ``, `` <C-x>4. ``, `` <space>gd ``, `` <space>jf ``, `` <space>jv ``, `` <space>w] ``, `` <C-w><C-]> ``, `` <space>mgg ``, `` <space>wg] ``, `` <C-w>g<C-]> ``, `` <space>w<C-]> ``, `` <space>wg<C-]> ``, select: `` <C-]> ``, `` <C-c>,j ``, `` <C-x>4. ``, `` <space>gd ``, `` <space>jf ``, `` <space>jv ``, `` <space>w] ``, `` <space>mgg ``, `` <space>wg] ``, `` <space>w<C-]> ``, `` <space>wg<C-]> ``, insert: `` <C-c>,j ``, `` <C-x>4. `` |
| `peek_definition` | Peek the definition in a popup without navigating (JetBrains Quick Definition) | normal: `` <space>lq ``, select: `` <space>lq `` |
| `goto_declaration` | Goto declaration | normal: `` gD ``, `` <C-w>i ``, `` [<C-d> ``, `` <space>gD ``, `` <space>wi ``, `` <C-w><C-i> ``, `` <space>w<C-i> ``, select: `` <space>gD ``, `` <space>wi ``, `` <space>w<C-i> `` |
| `add_newline_above` | Add newline above |  |
| `add_newline_below` | Add newline below |  |
| `goto_type_definition` | Goto type definition | normal: `` gy ``, `` <space>gy ``, select: `` <space>gy `` |
| `goto_implementation` | Goto implementation |  |
| `goto_file_start` | Goto line number `<n>` else file start | normal: `` gg ``, `` <A-lt> ``, `` <C-home> ``, insert: `` <A-lt> ``, `` <C-home> `` |
| `goto_file_end` | Goto file end | insert: `` <A-gt> ``, `` <C-end> `` |
| `extend_to_file_start` | Extend to line number `<n>` else file start |  |
| `extend_to_file_end` | Extend to file end |  |
| `goto_file` | Goto files/URLs in selections | normal: `` [f ``, `` ]f ``, `` gf ``, `` gx ``, `` <C-w>gF ``, `` <C-w>gf ``, `` <C-x>4f ``, `` <space>fF ``, `` <space>jU ``, `` <space>ju ``, `` <space>wgF ``, `` <space>wgf ``, select: `` <C-x>4f ``, `` <space>fF ``, `` <space>jU ``, `` <space>ju ``, `` <space>wgF ``, `` <space>wgf ``, insert: `` <C-x>4f `` |
| `goto_file_hsplit` | Goto files in selections (hsplit) | normal: `` <C-w>F ``, `` <space>wF ``, `` <C-w><C-f> ``, `` <space>w<C-f> ``, select: `` <space>wF ``, `` <space>w<C-f> `` |
| `goto_file_vsplit` | Goto files in selections (vsplit) |  |
| `goto_reference` | Goto references | normal: `` gr `` |
| `call_hierarchy_incoming_calls` | Call hierarchy: who calls the symbol (JetBrains Ctrl-Alt-H) | normal: `` <space>gh ``, select: `` <space>gh `` |
| `call_hierarchy_outgoing_calls` | Call hierarchy: what the symbol calls | normal: `` <space>gH ``, select: `` <space>gH `` |
| `type_hierarchy_supertypes` | Type hierarchy: supertypes of the symbol (JetBrains Ctrl-H) | normal: `` <space>gT ``, select: `` <space>gT `` |
| `type_hierarchy_subtypes` | Type hierarchy: subtypes of the symbol |  |
| `goto_window_top` | Goto window top | normal: `` H `` |
| `what_line` | Report the line number of point (emacs what-line) |  |
| `what_page` | Report the page number and line within the page (emacs what-page) |  |
| `count_lines_page` | Report lines on the current page, before + after point (emacs count-lines-page, C-x l) |  |
| `what_cursor_position` | Report the character at point, its code, position, percentage and column (emacs what-cursor-position, C-x =) | normal: `` <C-x>= ``, select: `` <C-x>= ``, insert: `` <C-x>= `` |
| `move_to_window_line_top_bottom` | Move point to window centre/top/bottom, cycling (emacs move-to-window-line-top-bottom, M-r) |  |
| `goto_window_center` | Goto window center | normal: `` M `` |
| `goto_window_bottom` | Goto window bottom | normal: `` L `` |
| `goto_last_accessed_file` | Goto last accessed file | normal: `` <C-^> ``, `` <C-w>^ ``, `` g<tab> ``, `` <C-tab> ``, `` <space>w^ ``, `` <C-w><C-^> ``, `` <C-w>g<tab> ``, `` <space><tab> ``, `` <space>w<C-^> ``, `` <space>wg<tab> ``, select: `` <space>w^ ``, `` <space><tab> ``, `` <space>w<C-^> ``, `` <space>wg<tab> `` |
| `goto_last_modified_file` | Goto last modified file |  |
| `goto_last_modification` | Goto last modification | normal: `` g, ``, `` g. ``, `` g; `` |
| `goto_line` | Goto line |  |
| `goto_last_line` | Goto last line | normal: `` G ``, `` <A-gt> ``, `` <C-end> `` |
| `extend_to_last_line` | Extend to last line |  |
| `goto_first_diag` | Goto first diagnostic | normal: `` <space>ef ``, select: `` <space>ef `` |
| `copy_diagnostic` | Copy the diagnostic message(s) on the current line |  |
| `goto_last_diag` | Goto last diagnostic | normal: `` <space>e. ``, select: `` <space>e. `` |
| `goto_next_diag` | Goto next diagnostic | normal: `` ]d ``, `` <space>en ``, select: `` <space>en `` |
| `goto_prev_diag` | Goto previous diagnostic | normal: `` [d ``, `` <space>ep ``, select: `` <space>ep `` |
| `goto_next_change` | Goto next change | normal: `` ]g `` |
| `goto_prev_change` | Goto previous change | normal: `` [g `` |
| `goto_next_conflict` | Goto next merge-conflict marker | normal: `` ]n `` |
| `goto_prev_conflict` | Goto previous merge-conflict marker | normal: `` [n `` |
| `conflict_take_all_ours` | Resolve ALL conflicts: keep our side | normal: `` <space>gcO ``, select: `` <space>gcO `` |
| `conflict_take_all_theirs` | Resolve ALL conflicts: keep their side | normal: `` <space>gcT ``, select: `` <space>gcT `` |
| `git_diff` | Open side-by-side diff vs HEAD | normal: `` <C-x>v= ``, `` <C-x>vD ``, `` <space>g= ``, `` <space>Dfv ``, `` <space>gfd ``, select: `` <C-x>v= ``, `` <C-x>vD ``, `` <space>g= ``, `` <space>Dfv ``, `` <space>gfd ``, insert: `` <C-x>v= ``, `` <C-x>vD `` |
| `resolve_conflicts` | Resolve merge conflicts (3-way) | normal: `` <space>gm ``, `` <space>gcr ``, select: `` <space>gm ``, `` <space>gcr `` |
| `git_status` | Magit status | normal: `` <C-x>v! ``, `` <C-x>vd ``, `` <C-x>vv ``, `` <space>gs ``, `` <space>pv ``, select: `` <C-x>v! ``, `` <C-x>vd ``, `` <C-x>vv ``, `` <space>gs ``, `` <space>pv ``, insert: `` <C-x>v! ``, `` <C-x>vd ``, `` <C-x>vv `` |
| `git_push` | Push the current branch to its remote (SPC g P) | normal: `` <C-x>vP ``, `` <space>gP ``, select: `` <C-x>vP ``, `` <space>gP ``, insert: `` <C-x>vP `` |
| `git_pull` | Fast-forward pull from upstream (SPC g u) | normal: `` <C-x>v+ ``, `` <space>gu ``, select: `` <C-x>v+ ``, `` <space>gu ``, insert: `` <C-x>v+ `` |
| `git_fetch` | Fetch all remotes (SPC g F) | normal: `` <space>gF ``, select: `` <space>gF `` |
| `vc_print_log` | VC log for the current file (emacs vc-print-log) |  |
| `vc_print_root_log` | VC log for the whole repository (emacs vc-print-root-log) |  |
| `vc_print_branch_log` | VC log for a named branch (emacs vc-print-branch-log) |  |
| `vc_root_diff` | Diff the whole working tree vs HEAD (emacs vc-root-diff) |  |
| `vc_region_history` | History of the selected line range (emacs vc-region-history) |  |
| `vc_log_incoming` | Commits incoming from upstream (emacs vc-log-incoming) |  |
| `vc_log_outgoing` | Commits outgoing to upstream (emacs vc-log-outgoing) |  |
| `vc_log_search` | Search the commit log by pattern (emacs vc-log-search) |  |
| `vc_create_branch` | Create and switch to a new branch (emacs vc-create-branch) |  |
| `vc_switch_branch` | Switch to an existing branch (emacs vc-switch-branch) |  |
| `vc_create_tag` | Create a git tag at HEAD (emacs vc-create-tag) |  |
| `vc_retrieve_tag` | Check out a tag or branch (emacs vc-retrieve-tag) |  |
| `vc_rename_file` | Rename the current file under VC (emacs vc-rename-file) |  |
| `vc_delete_file` | Delete the current file under VC (emacs vc-delete-file) |  |
| `vc_ignore` | Add the current file to .gitignore (emacs vc-ignore) |  |
| `vc_revert` | Revert the current file to HEAD (emacs vc-revert) |  |
| `vc_refresh_state` | Recompute the buffer's VC state (emacs vc-refresh-state) |  |
| `vc_state_refresh` | Recompute the buffer's VC state (emacs vc-state-refresh) |  |
| `vc_pull` | Pull from upstream (emacs vc-pull) |  |
| `vc_push` | Push to upstream (emacs vc-push) |  |
| `vc_next_action` | Do the next logical VC step: stage + commit (emacs vc-next-action) |  |
| `vc_dir` | Open the VC directory / Magit status (emacs vc-dir) |  |
| `project_vc_dir` | Open the project's VC directory / Magit status (emacs project-vc-dir) |  |
| `project_switch_project` | Switch to another project (emacs project-switch-project) |  |
| `project_search` | Grep-search the project (emacs project-search) |  |
| `project_query_replace_regexp` | Project-wide regex replace (emacs project-query-replace-regexp) |  |
| `project_list_buffers` | List open buffers (emacs project-list-buffers) |  |
| `project_shell_command` | Run a shell command in the project root (emacs project-shell-command) |  |
| `project_async_shell_command` | Run an async shell command in the project (emacs project-async-shell-command) |  |
| `project_eshell` | Open a shell buffer for the project (emacs project-eshell) |  |
| `xref_find_definitions_other_window` | Goto definition in another window (emacs xref-find-definitions-other-window) |  |
| `xref_query_replace_in_results` | Query-replace across xref/project results (emacs xref-query-replace-in-results) |  |
| `xref_find_references_and_replace` | Find references and replace them (emacs xref-find-references-and-replace) |  |
| `cut_to_clipboard` | Cut the selection to the system clipboard |  |
| `org_cycle` | Org: toggle subtree fold | normal: `` <space>mz ``, `` <space>m<tab> ``, select: `` <space>mz ``, `` <space>m<tab> `` |
| `org_todo` | Org: cycle TODO keyword |  |
| `org_priority` | Org: cycle priority cookie | normal: `` <space>mp ``, select: `` <space>mp `` |
| `org_promote` | Org: promote heading | normal: `` <space>mH ``, select: `` <space>mH `` |
| `org_demote` | Org: demote heading | normal: `` <space>ml ``, select: `` <space>ml `` |
| `org_next_heading` | Org: next heading | normal: `` <space>mj ``, select: `` <space>mj `` |
| `org_prev_heading` | Org: previous heading |  |
| `org_fold_all` | Org: fold all headings | normal: `` <space>ma ``, select: `` <space>ma `` |
| `org_unfold_all` | Org: unfold all | normal: `` <space>mA ``, select: `` <space>mA `` |
| `org_agenda` | Org: open agenda | normal: `` <space>aoa ``, select: `` <space>aoa `` |
| `org_capture` | Org: capture note | normal: `` <space>oc ``, `` <space>aoc ``, select: `` <space>oc ``, `` <space>aoc `` |
| `goto_first_change` | Goto first change |  |
| `goto_last_change` | Goto last change | normal: `` <space>jc ``, select: `` <space>jc `` |
| `goto_line_start` | Goto line start | normal: `` 0 ``, `` g0 ``, `` <home> ``, `` g<home> ``, `` <space>j0 ``, select: `` <space>j0 ``, insert: `` <home> `` |
| `goto_line_end` | Goto line end | normal: `` $ ``, `` g$ ``, `` gl ``, `` <end> ``, `` g<end> ``, `` <space>j$ ``, select: `` <space>j$ `` |
| `goto_visual_line_start` | Goto visual line start (soft-wrap aware) |  |
| `goto_visual_line_end` | Goto visual line end (soft-wrap aware) |  |
| `extend_to_visual_line_start` | Extend to visual line start |  |
| `extend_to_visual_line_end` | Extend to visual line end |  |
| `goto_column` | Goto column | normal: `` \| `` |
| `extend_to_column` | Extend to column |  |
| `goto_next_buffer` | Goto next buffer | normal: `` ]b ``, `` <space>bn ``, `` <space>b.n ``, `` <C-x><right> ``, select: `` <space>bn ``, `` <space>b.n ``, `` <C-x><right> ``, insert: `` <C-x><right> `` |
| `goto_previous_buffer` | Goto previous buffer | normal: `` [b ``, `` <space>bp ``, `` <space>b.N ``, `` <space>b.p ``, `` <C-x><left> ``, select: `` <space>bp ``, `` <space>b.N ``, `` <space>b.p ``, `` <C-x><left> ``, insert: `` <C-x><left> `` |
| `goto_line_end_newline` | Goto newline at line end | insert: `` <end> `` |
| `goto_first_nonwhitespace` | Goto first non-blank in line | normal: `` ^ ``, `` _ ``, `` g^ ``, `` <A-m> `` |
| `trim_selections` | Trim whitespace from selections |  |
| `extend_to_line_start` | Extend to line start |  |
| `extend_to_first_nonwhitespace` | Extend to first non-blank in line |  |
| `extend_to_line_end` | Extend to line end |  |
| `extend_to_line_end_newline` | Extend to line end |  |
| `signature_help` | Show signature help | normal: `` <space>ls ``, select: `` <space>ls `` |
| `smart_tab` | Insert tab if all cursors have all whitespace to their left; otherwise, run a separate command. |  |
| `insert_tab` | Insert tab char |  |
| `insert_newline` | Insert newline char | normal: `` <C-x>6<ret> ``, select: `` <C-x>6<ret> ``, insert: `` <C-j> ``, `` <ret> ``, `` <C-x>6<ret> `` |
| `default_indent_new_line` | Break line at point and continue the comment, indenting under it (emacs default-indent-new-line, M-j) | insert: `` <A-j> `` |
| `insert_char_interactive` | Insert an interactively-chosen char | insert: `` <C-q> ``, `` <C-v> `` |
| `append_char_interactive` | Append an interactively-chosen char |  |
| `delete_char_backward` | Delete previous char | insert: `` <backspace> `` |
| `delete_char_forward` | Delete next char | insert: `` <del> `` |
| `delete_chars_forward_vim` | Delete char(s) under cursor, line-bounded (vim x) | normal: `` x ``, `` <del> `` |
| `delete_chars_backward_vim` | Delete char(s) before cursor, no line join (vim X) | normal: `` X `` |
| `delete_word_backward` | Delete previous word | normal: `` <C-x><backspace> ``, select: `` <C-x><backspace> ``, insert: `` <C-w> ``, `` <A-backspace> ``, `` <C-x><backspace> `` |
| `delete_word_forward` | Delete next word | normal: `` <A-d> ``, insert: `` <A-d> `` |
| `kill_to_line_start` | Delete till start of line | insert: `` <C-u> `` |
| `kill_to_line_end` | Delete till end of line |  |
| `undo` | Undo change | normal: `` U ``, `` u ``, `` <C-/> ``, `` <C-_> ``, `` <C-x>u ``, `` <space>ku ``, select: `` <C-x>u ``, `` <space>ku ``, insert: `` <C-/> ``, `` <C-_> ``, `` <C-x>u `` |
| `redo` | Redo change | normal: `` <C-r> ``, `` <space>k<C-r> ``, select: `` <space>k<C-r> `` |
| `earlier` | Move backward in history | normal: `` g<minus> `` |
| `later` | Move forward in history | normal: `` g+ `` |
| `undo_tree` | Browse the branching undo history (vim undotree) | normal: `` <space>aU ``, select: `` <space>aU `` |
| `edit_injected_fragment` | Edit the injected-language fragment at point in its own buffer |  |
| `apply_injected_fragment` | Write the fragment buffer back into its host string |  |
| `commit_undo_checkpoint` | Commit changes to new checkpoint | insert: `` <C-g>U ``, `` <C-g>u `` |
| `yank` | Yank selection | normal: `` <A-w> `` |
| `yank_to_clipboard` | Yank selections to clipboard |  |
| `yank_to_primary_clipboard` | Yank selections to primary clipboard |  |
| `yank_joined` | Join and yank selections |  |
| `yank_joined_to_clipboard` | Join and yank selections to clipboard |  |
| `yank_main_selection_to_clipboard` | Yank main selection to clipboard |  |
| `yank_joined_to_primary_clipboard` | Join and yank selections to primary clipboard |  |
| `yank_main_selection_to_primary_clipboard` | Yank main selection to primary clipboard |  |
| `replace_with_yanked` | Replace with yanked text | select: `` P ``, `` p `` |
| `replace_selections_with_clipboard` | Replace selections by clipboard content |  |
| `replace_selections_with_primary_clipboard` | Replace selections by primary clipboard |  |
| `paste_after` | Paste after selection | normal: `` p ``, `` ]p ``, `` gp ``, `` zp ``, `` <space>kp ``, select: `` <space>kp `` |
| `paste_before` | Paste before selection | normal: `` P ``, `` [P ``, `` [p ``, `` ]P ``, `` gP ``, `` zP ``, `` <space>kP ``, select: `` <space>kP `` |
| `yank_from_kill_ring` | Yank the latest kill-ring entry (emacs C-y) |  |
| `yank_pop` | Replace the just-yanked text with the next kill-ring entry (emacs M-y) |  |
| `set_mark_command` | Set mark and activate region, pushing to the mark ring (emacs C-SPC) |  |
| `pop_to_mark` | Jump to the top of the mark ring, rotating it (emacs C-x C-SPC) | normal: `` <C-x><C-space> ``, select: `` <C-x><C-space> ``, insert: `` <C-x><C-space> `` |
| `point_to_register` | Save point to a register (emacs C-x r SPC) | normal: `` <C-x>rs ``, `` <C-x>r<space> ``, select: `` <C-x>rs ``, `` <C-x>r<space> ``, insert: `` <C-x>rs ``, `` <C-x>r<space> `` |
| `jump_to_register` | Jump to the position in a register (emacs C-x r j) | normal: `` <C-x>rj ``, select: `` <C-x>rj ``, insert: `` <C-x>rj `` |
| `number_to_register` | Store the prefix count in a register (emacs C-x r n) | normal: `` <C-x>rn ``, select: `` <C-x>rn ``, insert: `` <C-x>rn `` |
| `increment_register` | Add the prefix count to a number register (emacs C-x r +) | normal: `` <C-x>r+ ``, select: `` <C-x>r+ ``, insert: `` <C-x>r+ `` |
| `emacs_insert_register` | Insert a number or rectangle register's value at point (emacs C-x r i) | normal: `` <C-x>ri ``, select: `` <C-x>ri ``, insert: `` <C-x>ri `` |
| `copy_rectangle_to_register` | Copy the selected rectangle into a register (emacs C-x r r) |  |
| `kill_rectangle` | Kill (cut) the rectangle, saving it for yank (emacs C-x r k) | normal: `` <C-x>rk ``, select: `` <C-x>rk ``, insert: `` <C-x>rk `` |
| `delete_rectangle` | Delete the rectangle without saving (emacs C-x r d) | normal: `` <C-x>rd ``, select: `` <C-x>rd ``, insert: `` <C-x>rd `` |
| `clear_rectangle` | Blank the rectangle with spaces (emacs C-x r c) | normal: `` <C-x>rc ``, `` <C-x>ro ``, `` <C-x>rt ``, select: `` <C-x>rc ``, `` <C-x>ro ``, `` <C-x>rt ``, insert: `` <C-x>rc ``, `` <C-x>ro ``, `` <C-x>rt `` |
| `copy_rectangle_as_kill` | Copy the rectangle without deleting (emacs C-x r M-w) | normal: `` <C-x>rr ``, `` <C-x>r<A-w> ``, select: `` <C-x>rr ``, `` <C-x>r<A-w> ``, insert: `` <C-x>rr ``, `` <C-x>r<A-w> `` |
| `yank_rectangle` | Insert the saved rectangle at point (emacs C-x r y) | normal: `` <C-x>ry ``, select: `` <C-x>ry ``, insert: `` <C-x>ry `` |
| `open_rectangle` | Insert blank space to shift the rectangle right (emacs C-x r o) |  |
| `delete_whitespace_rectangle` | Delete whitespace after the rectangle's left column on each line (emacs delete-whitespace-rectangle) |  |
| `bookmark_set` | Set a named persistent bookmark at point (emacs C-x r m) | normal: `` <C-x>rm ``, select: `` <C-x>rm ``, insert: `` <C-x>rm `` |
| `bookmark_set_no_overwrite` | Set a bookmark, refusing to overwrite an existing name (emacs C-x r M) | normal: `` <C-x>rM ``, select: `` <C-x>rM ``, insert: `` <C-x>rM `` |
| `bookmark_jump` | Jump to a bookmark via a picker (emacs C-x r b) | normal: `` <C-x>rb ``, `` <space>rj ``, select: `` <C-x>rb ``, `` <space>rj ``, insert: `` <C-x>rb `` |
| `list_bookmarks` | List bookmarks in a picker; select to jump (emacs C-x r l / list-bookmarks) | normal: `` <C-x>rl ``, select: `` <C-x>rl ``, insert: `` <C-x>rl `` |
| `bookmark_insert_location` | Insert a bookmark's file path at point (emacs bookmark-insert-location, C-x r I) |  |
| `bookmark_insert` | Insert the contents of a bookmark's file at point (emacs bookmark-insert) |  |
| `bookmark_delete` | Delete a bookmark via a picker (emacs bookmark-delete) |  |
| `bookmark_rename` | Rename a bookmark via a picker (emacs bookmark-rename) |  |
| `define_abbrev` | Define a global abbrev: <name> <expansion> (emacs C-x a g) | normal: `` <C-x>ag ``, `` <C-x>al ``, `` <C-x>ail ``, select: `` <C-x>ag ``, `` <C-x>al ``, `` <C-x>ail ``, insert: `` <C-x>ag ``, `` <C-x>al ``, `` <C-x>ail `` |
| `inverse_add_global_abbrev` | Define the word before point as an abbrev, prompting for its expansion (emacs inverse-add-global-abbrev, C-x a i g) | normal: `` <C-x>aig ``, select: `` <C-x>aig ``, insert: `` <C-x>aig `` |
| `expand_abbrev` | Expand the abbrev before point (emacs C-x ') | normal: `` <C-x>' ``, select: `` <C-x>' ``, insert: `` <C-x>' `` |
| `abbrev_prefix_mark` | Mark point as an abbrev prefix boundary; insert a hyphen the next expand-abbrev removes (emacs abbrev-prefix-mark, M-') | normal: `` <space>xem ``, select: `` <space>xem `` |
| `unexpand_abbrev` | Undo the last abbrev expansion, restoring the original abbrev text (emacs unexpand-abbrev) | normal: `` <space>xeu ``, select: `` <space>xeu `` |
| `insert_abbrevs` | Insert a description of every defined abbrev at point (emacs insert-abbrevs) |  |
| `define_abbrevs` | Define abbrevs from the buffer text after point (emacs define-abbrevs) |  |
| `paste_clipboard_after` | Paste clipboard after selections |  |
| `paste_clipboard_before` | Paste clipboard before selections |  |
| `paste_primary_clipboard_after` | Paste primary clipboard after selections |  |
| `paste_primary_clipboard_before` | Paste primary clipboard before selections |  |
| `indent` | Indent selection | normal: `` == ``, `` <C-x><tab> ``, `` <space>xac ``, `` <space>x<tab> ``, select: `` <C-x><tab> ``, `` <space>xac ``, `` <space>x<tab> ``, insert: `` <C-t> ``, `` <C-x><tab> `` |
| `unindent` | Unindent selection | insert: `` <C-d> `` |
| `format_selections` | Format selection | normal: `` <A-q> ``, `` <space>j+ ``, `` <space>j= ``, `` <space>lf ``, select: `` <space>j+ ``, `` <space>j= ``, `` <space>lf `` |
| `join_selections` | Join lines inside selection | normal: `` <A-^> ``, `` <space>kJ ``, select: `` <space>kJ `` |
| `join_selections_space` | Join lines inside selection and select spaces |  |
| `join_lines_vim` | Join line(s) with a space, cursor at join (vim J) | normal: `` J `` |
| `join_lines_vim_nospace` | Join line(s) without a space (vim gJ) |  |
| `keep_selections` | Keep selections matching regex |  |
| `remove_selections` | Remove selections matching regex |  |
| `align_selections` | Align selections in column | normal: `` <space>xaa ``, select: `` <space>xaa `` |
| `keep_primary_selection` | Keep primary selection |  |
| `remove_primary_selection` | Remove primary selection |  |
| `completion` | Invoke completion popup | normal: `` <C-c>,<space> ``, select: `` <C-c>,<space> ``, insert: `` <A-/> ``, `` <C-n> ``, `` <C-p> ``, `` <C-c>,<space> `` |
| `hover` | Show docs for item under cursor | normal: `` K ``, `` <C-h>. ``, `` <C-w>} ``, `` <C-w>g} ``, `` <space>lk ``, `` <space>w} ``, `` <space>hda ``, `` <space>hdf ``, `` <space>hdv ``, `` <space>mhh ``, `` <space>wg} ``, select: `` K ``, `` <C-h>. ``, `` <space>lk ``, `` <space>w} ``, `` <space>hda ``, `` <space>hdf ``, `` <space>hdv ``, `` <space>mhh ``, `` <space>wg} ``, insert: `` <C-h>. `` |
| `toggle_comments` | Comment/uncomment selections | normal: `` <A-;> ``, `` <space>; ``, `` <space>cP ``, `` <space>cc ``, `` <space>cp ``, `` <C-x><C-;> ``, select: `` <space>; ``, `` <space>cP ``, `` <space>cc ``, `` <space>cp ``, `` <C-x><C-;> ``, insert: `` <C-x><C-;> `` |
| `toggle_line_comments` | Line comment/uncomment selections | normal: `` <space>cL ``, `` <space>cl ``, select: `` <space>cL ``, `` <space>cl `` |
| `comment_to_line` | Comment/uncomment from the cursor line to a prompted line (SPC c t) | normal: `` <space>ct ``, select: `` <space>ct `` |
| `invert_comment_to_line` | Invert comments per line from the cursor to a prompted line (SPC c T) | normal: `` <space>cT ``, select: `` <space>cT `` |
| `toggle_block_comments` | Block comment/uncomment selections | normal: `` <space>cb ``, select: `` <space>cb `` |
| `comment_kill` | Kill the comment on the current line to the kill ring (emacs comment-kill; count kills that many lines) | normal: `` <space>cx ``, select: `` <space>cx `` |
| `rotate_selections_forward` | Rotate selections forward |  |
| `rotate_selections_backward` | Rotate selections backward |  |
| `rotate_selection_contents_forward` | Rotate selection contents forward |  |
| `rotate_selection_contents_backward` | Rotate selections contents backward |  |
| `reverse_selection_contents` | Reverse selections contents |  |
| `expand_selection` | Expand selection to parent syntax node | normal: `` <space>v ``, `` <space>kU ``, select: `` <space>v ``, `` <space>kU `` |
| `shrink_selection` | Shrink selection to previously expanded syntax node |  |
| `wildfire` | Wildfire: select/expand to the closest text object | normal: `` <ret> `` |
| `wildfire_shrink` | Wildfire: shrink to the previously selected text object | normal: `` <backspace> `` |
| `select_next_sibling` | Select next sibling in the syntax tree | normal: `` <space>kL ``, `` <space>kl ``, select: `` <space>kL ``, `` <space>kl `` |
| `select_prev_sibling` | Select previous sibling the in syntax tree | normal: `` <space>kH ``, `` <space>kh ``, select: `` <space>kH ``, `` <space>kh `` |
| `select_all_siblings` | Select all siblings of the current node |  |
| `select_all_children` | Select all children of the current node |  |
| `jump_forward` | Jump forward on jumplist | normal: `` <C-i> ``, `` <tab> `` |
| `jump_backward` | Jump backward on jumplist | normal: `` <C-o> ``, `` <C-t> ``, `` <space>jb ``, `` <space>s` ``, select: `` <space>jb ``, `` <space>s` `` |
| `save_selection` | Save current selection to jumplist |  |
| `jump_view_right` | Jump to right split | normal: `` <C-w>l ``, `` <C-w>.l ``, `` <space>wl ``, `` <C-w><C-l> ``, `` <space>w.l ``, `` <C-w><right> ``, `` <space>w<C-l> ``, `` <space>w<right> ``, select: `` <space>wl ``, `` <space>w.l ``, `` <space>w<C-l> ``, `` <space>w<right> `` |
| `jump_view_left` | Jump to left split | normal: `` <C-w>h ``, `` <C-w>.h ``, `` <space>wh ``, `` <C-w><C-h> ``, `` <space>w.h ``, `` <C-w><left> ``, `` <space>w<C-h> ``, `` <space>w<left> ``, select: `` <space>wh ``, `` <space>w.h ``, `` <space>w<C-h> ``, `` <space>w<left> `` |
| `jump_view_up` | Jump to split above | normal: `` <C-w>k ``, `` <C-w>.k ``, `` <C-w><up> ``, `` <space>wk ``, `` <C-w><C-k> ``, `` <C-w><C-t> ``, `` <space>w.k ``, `` <space>w<up> ``, `` <space>w<C-k> ``, `` <space>w<C-t> ``, select: `` <space>wk ``, `` <space>w.k ``, `` <space>w<up> ``, `` <space>w<C-k> ``, `` <space>w<C-t> `` |
| `jump_view_down` | Jump to split below | normal: `` <C-w>b ``, `` <C-w>j ``, `` <C-w>.j ``, `` <space>wb ``, `` <space>wj ``, `` <C-w><C-b> ``, `` <C-w><C-j> ``, `` <space>w.j ``, `` <C-w><down> ``, `` <space>w<C-b> ``, `` <space>w<C-j> ``, `` <space>w<down> ``, select: `` <space>wb ``, `` <space>wj ``, `` <space>w.j ``, `` <space>w<C-b> ``, `` <space>w<C-j> ``, `` <space>w<down> `` |
| `swap_view_right` | Swap with right split | normal: `` <C-w>L ``, `` <C-w>.L ``, `` <space>wL ``, `` <space>w.L ``, select: `` <space>wL ``, `` <space>w.L `` |
| `swap_view_left` | Swap with left split | normal: `` <C-w>H ``, `` <C-w>.H ``, `` <space>wH ``, `` <space>w.H ``, select: `` <space>wH ``, `` <space>w.H `` |
| `swap_view_up` | Swap with split above | normal: `` <C-w>K ``, `` <C-w>.K ``, `` <space>wK ``, `` <space>w.K ``, select: `` <space>wK ``, `` <space>w.K `` |
| `swap_view_down` | Swap with split below | normal: `` <C-w>J ``, `` <C-w>.J ``, `` <space>wJ ``, `` <space>w.J ``, select: `` <space>wJ ``, `` <space>w.J `` |
| `transpose_view` | Transpose splits | normal: `` <C-w>M ``, `` <C-w>x ``, `` <space>wM ``, `` <space>wx ``, `` <C-w><C-x> ``, `` <space>w<C-x> ``, select: `` <space>wM ``, `` <space>wx ``, `` <space>w<C-x> `` |
| `quickfix_next` | Quickfix: jump to next entry (:cnext) |  |
| `quickfix_prev` | Quickfix: jump to previous entry (:cprev) |  |
| `quickfix_first` | Quickfix: jump to first entry (:cfirst) |  |
| `quickfix_last` | Quickfix: jump to last entry (:clast) |  |
| `quickfix_open` | Quickfix: open the quickfix list window (:copen) |  |
| `loclist_next` | Location list: jump to next entry (:lnext) |  |
| `loclist_prev` | Location list: jump to previous entry (:lprev) |  |
| `loclist_first` | Location list: jump to first entry (:lfirst) |  |
| `loclist_last` | Location list: jump to last entry (:llast) |  |
| `loclist_open` | Location list: open the location list window (:lopen) |  |
| `goto_next_tabpage` | Go to the next tabpage (gt / :tabnext) | normal: `` gt ``, `` <C-w>gt ``, `` <space>wgt ``, select: `` <space>wgt `` |
| `goto_previous_tabpage` | Go to the previous tabpage (gT / :tabprevious) | normal: `` gT ``, `` <C-w>gT ``, `` <space>wgT ``, select: `` <space>wgT `` |
| `new_tab` | Open a new tabpage (:tabnew) | normal: `` <C-x>t ``, select: `` <C-x>t ``, insert: `` <C-x>t `` |
| `close_tab` | Close the current tabpage (:tabclose) |  |
| `tab_only` | Close all other tabpages (:tabonly) |  |
| `goto_first_tabpage` | Go to the first tabpage (:tabfirst) |  |
| `goto_last_tabpage` | Go to the last tabpage (:tablast) |  |
| `tab_select` | Go to the [count]-th tab (emacs tab-select) |  |
| `tab_recent` | Switch to the most recently visited tab (emacs tab-recent) |  |
| `forward_list` | Move forward over a balanced () group (emacs forward-list, C-M-n) |  |
| `backward_list` | Move backward over a balanced () group (emacs backward-list, C-M-p) |  |
| `down_list` | Descend into the next list (emacs down-list, C-M-d) |  |
| `up_list` | Move forward out of the enclosing list (emacs up-list) |  |
| `backward_up_list` | Move backward out of the enclosing list (emacs backward-up-list, C-M-u) |  |
| `kill_sexp` | Kill the s-expression after point (emacs kill-sexp, C-M-k) |  |
| `mark_sexp` | Set the region over the following s-expression (emacs mark-sexp, C-M-SPC) |  |
| `forward_sexp` | Move forward over the next s-expression (emacs forward-sexp, C-M-f) |  |
| `backward_sexp` | Move backward over the previous s-expression (emacs backward-sexp, C-M-b) |  |
| `prog_indent_sexp` | Re-indent the s-expression after point, or the enclosing defun with a prefix (emacs prog-indent-sexp, C-M-q; here = s) | normal: `` =s `` |
| `copy_region_as_kill` | Copy the region to the kill ring without deleting (emacs copy-region-as-kill, M-w) |  |
| `mark_word` | Set the region over the next word (emacs mark-word, M-@) |  |
| `mark_paragraph` | Select the paragraph around point (emacs mark-paragraph, M-h) |  |
| `mark_defun` | Select the function/defun around point (emacs mark-defun, C-M-h) |  |
| `kill_sentence` | Kill from point to end of sentence (emacs kill-sentence, M-k) |  |
| `backward_kill_sentence` | Kill from start of sentence to point (emacs backward-kill-sentence, C-x DEL) |  |
| `append_next_kill` | Make the next kill append to the last kill-ring entry (emacs append-next-kill, C-M-w) | normal: `` <space>ra ``, select: `` <space>ra `` |
| `forward_page` | Move to the next form-feed page (emacs forward-page, C-x ]) |  |
| `backward_page` | Move to the previous form-feed page (emacs backward-page, C-x [) |  |
| `mark_page` | Select the current form-feed page (emacs mark-page, C-x C-p) |  |
| `move_to_opposite_group` | Move the current editor to the opposite split group (JetBrains) |  |
| `rotate_view` | Goto next window | normal: `` <C-w>p ``, `` <C-w>r ``, `` <C-w>w ``, `` <C-x>o ``, `` <C-w>.o ``, `` <C-w>.r ``, `` <C-w>.w ``, `` <space>wp ``, `` <space>wr ``, `` <space>ww ``, `` <C-w><C-p> ``, `` <C-w><C-r> ``, `` <C-w><C-w> ``, `` <C-w><tab> ``, `` <space>b.o ``, `` <space>w.o ``, `` <space>w.r ``, `` <space>w.w ``, `` <space>w<C-p> ``, `` <space>w<C-r> ``, `` <space>w<C-w> ``, `` <space>w<tab> ``, select: `` <C-x>o ``, `` <space>wp ``, `` <space>wr ``, `` <space>ww ``, `` <space>b.o ``, `` <space>w.o ``, `` <space>w.r ``, `` <space>w.w ``, `` <space>w<C-p> ``, `` <space>w<C-r> ``, `` <space>w<C-w> ``, `` <space>w<tab> ``, insert: `` <C-x>o `` |
| `rotate_view_reverse` | Goto previous window | normal: `` <C-w>R ``, `` <C-w>W ``, `` <C-w>.R ``, `` <space>wR ``, `` <space>wW ``, `` <space>w.R ``, select: `` <space>wR ``, `` <space>wW ``, `` <space>w.R `` |
| `scroll_other_window` | Scroll the other window forward (emacs scroll-other-window, C-M-v) |  |
| `scroll_other_window_down` | Scroll the other window backward (emacs scroll-other-window-down, C-M-S-v) |  |
| `recenter_other_window` | Recenter point in the other window (emacs recenter-other-window, C-M-S-l) | normal: `` <C-w>z ``, `` <space>wz ``, select: `` <space>wz `` |
| `hsplit` | Horizontal bottom split | normal: `` <C-w>S ``, `` <C-w>s ``, `` <C-x>2 ``, `` <C-w>.S ``, `` <C-w>.s ``, `` <space>wS ``, `` <space>ws ``, `` <C-w><C-s> ``, `` <space>w.S ``, `` <space>w.s ``, `` <C-w>.<minus> ``, `` <space>w<C-s> ``, `` <space>w.<minus> ``, select: `` <C-x>2 ``, `` <space>wS ``, `` <space>ws ``, `` <space>w.S ``, `` <space>w.s ``, `` <space>w<C-s> ``, `` <space>w.<minus> ``, insert: `` <C-x>2 `` |
| `hsplit_new` | Horizontal bottom split scratch buffer | normal: `` <C-w>n ``, `` <space>wn ``, `` <C-w><C-n> ``, `` <space>bNj ``, `` <space>bNk ``, `` <space>w<C-n> ``, select: `` <space>wn ``, `` <space>bNj ``, `` <space>bNk ``, `` <space>w<C-n> `` |
| `vsplit` | Vertical right split | normal: `` <C-w>/ ``, `` <C-w>2 ``, `` <C-w>V ``, `` <C-w>v ``, `` <C-x>3 ``, `` <C-x>5 ``, `` <C-w>./ ``, `` <C-w>.V ``, `` <C-w>.v ``, `` <C-x>62 ``, `` <C-x>6s ``, `` <space>w/ ``, `` <space>w2 ``, `` <space>wV ``, `` <space>wv ``, `` <C-w><C-v> ``, `` <space>w./ ``, `` <space>w.V ``, `` <space>w.v ``, `` <space>w<C-v> ``, `` <space>u<space>w2 ``, select: `` <C-x>3 ``, `` <C-x>5 ``, `` <C-x>62 ``, `` <C-x>6s ``, `` <space>w/ ``, `` <space>w2 ``, `` <space>wV ``, `` <space>wv ``, `` <space>w./ ``, `` <space>w.V ``, `` <space>w.v ``, `` <space>w<C-v> ``, `` <space>u<space>w2 ``, insert: `` <C-x>3 ``, `` <C-x>5 ``, `` <C-x>62 ``, `` <C-x>6s `` |
| `vsplit_new` | Vertical right split scratch buffer | normal: `` <space>bNh ``, `` <space>bNl ``, select: `` <space>bNh ``, `` <space>bNl `` |
| `wclose` | Close window | normal: `` <C-w>D ``, `` <C-w>c ``, `` <C-w>d ``, `` <C-w>q ``, `` <C-x>0 ``, `` <C-w>.d ``, `` <C-x>40 ``, `` <C-x>6d ``, `` <space>cd ``, `` <space>wD ``, `` <space>wc ``, `` <space>wd ``, `` <space>wq ``, `` <C-w><C-d> ``, `` <C-w><C-q> ``, `` <space>w.d ``, `` <space>w<C-d> ``, `` <space>w<C-q> ``, `` <space>u<space>wd ``, select: `` <C-x>0 ``, `` <C-x>40 ``, `` <C-x>6d ``, `` <space>cd ``, `` <space>wD ``, `` <space>wc ``, `` <space>wd ``, `` <space>wq ``, `` <space>w.d ``, `` <space>w<C-d> ``, `` <space>w<C-q> ``, `` <space>u<space>wd ``, insert: `` <C-x>0 ``, `` <C-x>40 ``, `` <C-x>6d `` |
| `wonly` | Close windows except current | normal: `` <C-w>1 ``, `` <C-w>_ ``, `` <C-w>m ``, `` <C-w>o ``, `` <C-x>1 ``, `` <C-w>.D ``, `` <C-w>._ ``, `` <C-w>.m ``, `` <C-w>\| ``, `` <C-x>61 ``, `` <C-w>.\| ``, `` <space>w1 ``, `` <space>w_ ``, `` <space>wm ``, `` <space>wo ``, `` <C-w><C-o> ``, `` <space>w.D ``, `` <space>w._ ``, `` <space>w.m ``, `` <space>w\| ``, `` <space>w.\| ``, `` <space>w<C-o> ``, `` <space>u<space>w1 ``, select: `` <C-x>1 ``, `` <C-x>61 ``, `` <space>w1 ``, `` <space>w_ ``, `` <space>wm ``, `` <space>wo ``, `` <space>w.D ``, `` <space>w._ ``, `` <space>w.m ``, `` <space>w\| ``, `` <space>w.\| ``, `` <space>w<C-o> ``, `` <space>u<space>w1 ``, insert: `` <C-x>1 ``, `` <C-x>61 `` |
| `select_register` | Select register | normal: `` " `` |
| `insert_register` | Insert register | insert: `` <C-r> `` |
| `view_register` | Show a register's contents (emacs view-register, C-x r v) |  |
| `not_modified` | Mark the buffer unmodified without saving (emacs not-modified, M-~) |  |
| `insert_char_by_code` | Insert a character by Unicode code point (emacs insert-char, C-x 8 RET) |  |
| `backward_delete_char_untabify` | Delete backward, expanding a tab into spaces first (emacs backward-delete-char-untabify) |  |
| `insert_last_inserted_text` | Insert the previously inserted text (vim i_CTRL-A) | insert: `` <C-a> `` |
| `insert_last_inserted_and_stop` | Insert previously inserted text and stop insert (vim i_CTRL-@) | insert: `` <C-@> `` |
| `copy_between_registers` | Copy between two registers |  |
| `copy_to_register` | Copy the region into a register (emacs copy-to-register, C-x r s) |  |
| `append_to_register` | Append the region to a register (emacs append-to-register, C-x r a) |  |
| `prepend_to_register` | Prepend the region to a register (emacs prepend-to-register, C-x r p) |  |
| `align_view_middle` | Align view middle |  |
| `align_view_top` | Align view top | normal: `` zt `` |
| `align_view_center` | Align view center | normal: `` zz ``, `` <C-l> ``, `` <C-w>.z ``, `` <space>b.z ``, `` <space>w.z ``, select: `` <space>b.z ``, `` <space>w.z `` |
| `align_view_bottom` | Align view bottom | normal: `` zb `` |
| `scroll_up` | Scroll view up | normal: `` <C-y> `` |
| `scroll_down` | Scroll view down | normal: `` <C-e> `` |
| `scroll_column_left` | Scroll view left one column (zh) | normal: `` zh ``, `` z<left> `` |
| `scroll_column_right` | Scroll view right one column (zl) | normal: `` zl ``, `` z<right> `` |
| `scroll_half_column_left` | Scroll view left half a screen (zH) | normal: `` zH ``, `` ze ``, `` <C-x><gt> ``, select: `` <C-x><gt> ``, insert: `` <C-x><gt> `` |
| `scroll_half_column_right` | Scroll view right half a screen (zL) | normal: `` zL ``, `` zs ``, `` <C-x><lt> ``, select: `` <C-x><lt> ``, insert: `` <C-x><lt> `` |
| `resize_view_wider` | Make current window wider (CTRL-W >) | normal: `` <C-x>} ``, `` <C-w>.] ``, `` <C-w><gt> ``, `` <space>w.] ``, `` <space>w<gt> ``, select: `` <C-x>} ``, `` <space>w.] ``, `` <space>w<gt> ``, insert: `` <C-x>} `` |
| `resize_view_narrower` | Make current window narrower (CTRL-W <) | normal: `` <C-w>[ ``, `` <C-x>{ ``, `` <C-w>.[ ``, `` <C-w><lt> ``, `` <space>w[ ``, `` <space>w.[ ``, `` <space>w<lt> ``, select: `` <C-x>{ ``, `` <space>w[ ``, `` <space>w.[ ``, `` <space>w<lt> ``, insert: `` <C-x>{ `` |
| `resize_view_taller` | Make current window taller (CTRL-W +) | normal: `` <C-w>+ ``, `` <C-x>^ ``, `` <C-w>.} ``, `` <space>w+ ``, `` <space>w.} ``, select: `` <C-x>^ ``, `` <space>w+ ``, `` <space>w.} ``, insert: `` <C-x>^ `` |
| `resize_view_shorter` | Make current window shorter (CTRL-W -) | normal: `` <C-w>{ ``, `` <C-w>.{ ``, `` <space>w{ ``, `` <space>w.{ ``, `` <C-w><minus> ``, `` <C-x><minus> ``, `` <space>w<minus> ``, select: `` <space>w{ ``, `` <space>w.{ ``, `` <C-x><minus> ``, `` <space>w<minus> ``, insert: `` <C-x><minus> `` |
| `resize_view_equalize` | Make all windows equal size (CTRL-W =) | normal: `` <C-w>= ``, `` <C-x>+ ``, `` <space>w= ``, select: `` <C-x>+ ``, `` <space>w= ``, insert: `` <C-x>+ `` |
| `golden_ratio_resize` | Resize the focused window to the golden ratio (SPC t g) | normal: `` <C-w>.g ``, `` <space>tg ``, `` <space>w.g ``, select: `` <space>tg ``, `` <space>w.g `` |
| `rot13` | ROT13-encode the selection (g?) |  |
| `check_parens` | Move to the first unbalanced bracket, or report all balanced (check-parens) |  |
| `url_encode` | Percent-encode (URL-encode) the selection |  |
| `url_decode` | Percent-decode (URL-decode) the selection |  |
| `parse_query_selection` | Expand a URL query string into decoded key=value lines |  |
| `build_query_selection` | Build a URL query string from key=value lines |  |
| `url_info_selection` | Break the selected URL into scheme/host/port/path/query lines |  |
| `encode_base64` | Base64-encode the selection |  |
| `decode_base64` | Base64-decode the selection |  |
| `encode_base64url` | URL-safe base64-encode the selection (no padding) |  |
| `decode_base64url` | URL-safe base64-decode the selection (JWT-friendly) |  |
| `jwt_decode_selection` | Decode the selected JWT into pretty header + payload JSON |  |
| `encode_html` | HTML-escape the selection (& < > " ') |  |
| `decode_html` | Decode HTML entities in the selection |  |
| `html_to_text_selection` | Strip HTML tags and decode entities to plain text |  |
| `title_case_selection` | Title-case the selection (capitalize each word) |  |
| `sentence_case_selection` | Capitalize the first letter of each sentence in the selection |  |
| `straighten_quotes_selection` | Convert smart quotes/dashes in the selection to plain ASCII |  |
| `hex_to_rgb_selection` | Convert a #hex color in the selection to rgb(r, g, b) |  |
| `rgb_to_hex_selection` | Convert an rgb(r, g, b) color in the selection to #hex |  |
| `to_roman_selection` | Convert the selected integer to a Roman numeral |  |
| `from_roman_selection` | Convert the selected Roman numeral to an integer |  |
| `add_commas_selection` | Add thousands separators to numbers in the selection |  |
| `strip_commas_selection` | Remove thousands separators from numbers in the selection |  |
| `swap_quotes_selection` | Swap ' and " quote characters in the selection |  |
| `strip_quotes_selection` | Remove surrounding quotes from the selection |  |
| `reverse_words_selection` | Reverse the word order within each selected line |  |
| `unwrap_tag_selection` | Strip the outermost <tag>…</tag> wrapper from the selection |  |
| `sort_paragraphs_selection` | Sort blank-line-separated paragraphs in the selection |  |
| `lighten_selection` | Lighten the hex color in the selection by 10% |  |
| `darken_selection` | Darken the hex color in the selection by 10% |  |
| `contrast_text` | Recommend black/white text for the selected hex background color |  |
| `toggle_value_selection` | Toggle the boolean/keyword in the selection (true<->false, …) |  |
| `normalize_whitespace_selection` | Collapse internal whitespace runs in the selection |  |
| `insert_toc` | Insert a markdown table of contents from the buffer's headings |  |
| `slugify_selection` | Slugify the selection (lowercase, hyphen-separated) |  |
| `humanize_selection` | Humanize a slug/identifier into a Title-Cased label |  |
| `transpose_csv_selection` | Transpose the selected CSV/TSV table (rows <-> columns) |  |
| `csv_to_json_selection` | Convert the selected CSV/TSV to a JSON array of objects |  |
| `regex_escape_selection` | Escape regex metacharacters in the selection |  |
| `blockquote_selection` | Prefix each selected line with "> " (markdown blockquote) |  |
| `unblockquote_selection` | Strip a leading "> " from each selected line |  |
| `bullet_list_selection` | Make a markdown bullet list from the selected lines |  |
| `unbullet_selection` | Strip a leading bullet (- * +) from each selected line |  |
| `strip_ansi_selection` | Strip ANSI/VT escape codes from the selection |  |
| `html_escape_selection` | HTML-escape the selection (& < > " ' to entities) |  |
| `html_unescape_selection` | HTML-unescape entities in the selection back to characters |  |
| `reverse_chars_selection` | Reverse the characters in the selection |  |
| `json_escape_selection` | JSON-escape the selection (for a string literal) |  |
| `to_json_string_selection` | Wrap the selection in quotes as a JSON string literal |  |
| `json_unescape_selection` | JSON-unescape the selection |  |
| `to_hex_selection` | Encode the selection as hex bytes |  |
| `from_hex_selection` | Decode hex bytes in the selection back to text |  |
| `format_table_selection` | Align the selected markdown table's columns |  |
| `csv_to_table_selection` | Convert the selected CSV/TSV to a markdown table |  |
| `table_to_csv_selection` | Convert the selected markdown table to CSV |  |
| `json_pretty_selection` | Pretty-print the selected JSON (preserves key order) |  |
| `json_minify_selection` | Minify the selected JSON |  |
| `xml_pretty_selection` | Pretty-print the selected XML/HTML |  |
| `insert_digraph` | Insert a digraph by two-character mnemonic (CTRL-K) | insert: `` <C-k> `` |
| `insert_uuid_v4` | Insert a random UUIDv4 (SPC i U 4) | normal: `` <space>iU4 ``, `` <space>iUU ``, select: `` <space>iU4 ``, `` <space>iUU `` |
| `insert_uuid_v1` | Insert a time-based UUIDv1 (SPC i U 1) | normal: `` <space>iU1 ``, select: `` <space>iU1 `` |
| `insert_lorem_sentence` | Insert a lorem-ipsum sentence (SPC i l s) | normal: `` <space>ils ``, select: `` <space>ils `` |
| `insert_lorem_paragraph` | Insert a lorem-ipsum paragraph (SPC i l p) | normal: `` <space>ilp ``, select: `` <space>ilp `` |
| `insert_lorem_list` | Insert a lorem-ipsum list (SPC i l l) | normal: `` <space>ill ``, select: `` <space>ill `` |
| `insert_password_simple` | Insert a simple alphanumeric password (SPC i p 1) | normal: `` <space>ip1 ``, select: `` <space>ip1 `` |
| `insert_password_strong` | Insert a stronger password with symbols (SPC i p 2) | normal: `` <space>ip2 ``, select: `` <space>ip2 `` |
| `insert_password_paranoid` | Insert a long password for paranoids (SPC i p 3) | normal: `` <space>ip3 ``, select: `` <space>ip3 `` |
| `insert_password_numerical` | Insert a numeric password (SPC i p n) | normal: `` <space>ipn ``, select: `` <space>ipn `` |
| `insert_password_phonetic` | Insert a phonetically easy password (SPC i p p) | normal: `` <space>ipp ``, select: `` <space>ipp `` |
| `symbol_upper_camel` | Change symbol style to UpperCamelCase (SPC x i C) | normal: `` <space>xiC ``, select: `` <space>xiC `` |
| `symbol_up_case` | Change symbol style to UP_CASE (SPC x i U) | normal: `` <space>xiU ``, select: `` <space>xiU `` |
| `symbol_under_score` | Change symbol style to under_score (SPC x i _) | normal: `` <space>xi_ ``, select: `` <space>xi_ `` |
| `symbol_lower_camel` | Change symbol style to camelCase (vim-abolish crc) |  |
| `symbol_kebab` | Change symbol style to kebab-case (vim-abolish cr-) |  |
| `symbol_dot` | Change symbol style to dot.case (vim-abolish cr.) | normal: `` <space>xi. ``, select: `` <space>xi. `` |
| `randomize_lines_in_region` | Randomize lines in the selection (SPC x l r) | normal: `` <space>xlr ``, select: `` <space>xlr `` |
| `randomize_words_in_region` | Randomize words in the selection (SPC x w r) | normal: `` <space>xwr ``, select: `` <space>xwr `` |
| `copy_char_below` | Insert the character below the cursor (i_CTRL-E) | insert: `` <C-e> `` |
| `copy_char_above` | Insert the character above the cursor (i_CTRL-Y) | insert: `` <C-y> `` |
| `file_info` | Show file name and cursor position (CTRL-G) | normal: `` <C-g> `` |
| `document_stats` | Show document line/word/char counts (g CTRL-G) | normal: `` <C-x>l ``, `` g<C-g> ``, select: `` <C-x>l ``, `` g<C-g> ``, insert: `` <C-x>l `` |
| `git_blame_line` | Show git blame for the current line (g b) | normal: `` <C-x>vg ``, `` <space>gM ``, `` <space>gb ``, select: `` <C-x>vg ``, `` <space>gM ``, `` <space>gb ``, insert: `` <C-x>vg `` |
| `toggle_inline_blame` | Toggle GitLens-style inline blame on the current line | normal: `` <space>gI ``, select: `` <space>gI `` |
| `toggle_blame_annotate` | Toggle the git-blame annotate gutter column (SPC g B) | normal: `` <space>gB ``, select: `` <space>gB `` |
| `git_branch_picker` | Pick a git branch and check it out | normal: `` <C-x>vr ``, `` <C-x>vs ``, `` <C-x>vbc ``, `` <C-x>vbs ``, select: `` <C-x>vr ``, `` <C-x>vs ``, `` <C-x>vbc ``, `` <C-x>vbs ``, insert: `` <C-x>vr ``, `` <C-x>vs ``, `` <C-x>vbc ``, `` <C-x>vbs `` |
| `preferences` | Open the unified Preferences window | normal: `` <space>, ``, select: `` <space>, `` |
| `set_selective_display` | Hide lines indented past the prefix-arg column; no arg turns it off (emacs set-selective-display, C-x $) |  |
| `global_whitespace_toggle_options` | Toggle rendering of whitespace characters (emacs global-whitespace-toggle-options) |  |
| `global_tab_line_mode` | Toggle the buffer tab line (emacs global-tab-line-mode) |  |
| `global_visual_wrap_prefix_mode` | Toggle soft-wrap with indentation carry-over (emacs global-visual-wrap-prefix-mode) |  |
| `help` | Open the inline Help browser | normal: `` <C-h>? ``, `` <C-h>F ``, `` <C-h>I ``, `` <C-h>K ``, `` <C-h>r ``, `` <C-h>t ``, `` <C-h>4s ``, `` <space>bH ``, `` <space>h? ``, `` <space>hc ``, `` <space>hh ``, `` <space>hk ``, `` <space>hr ``, `` <C-h><C-h> ``, `` <C-h><C-m> ``, `` <C-h><C-o> ``, `` <C-h><C-q> ``, `` <C-h><C-t> ``, `` <space>hdb ``, `` <space>hdk ``, `` <space>hdx ``, `` <space>h<space> ``, select: `` <C-h>? ``, `` <C-h>F ``, `` <C-h>I ``, `` <C-h>K ``, `` <C-h>r ``, `` <C-h>t ``, `` <C-h>4s ``, `` <space>bH ``, `` <space>h? ``, `` <space>hc ``, `` <space>hh ``, `` <space>hk ``, `` <space>hr ``, `` <C-h><C-h> ``, `` <C-h><C-m> ``, `` <C-h><C-o> ``, `` <C-h><C-q> ``, `` <C-h><C-t> ``, `` <space>hdb ``, `` <space>hdk ``, `` <space>hdx ``, `` <space>h<space> ``, insert: `` <C-h>? ``, `` <C-h>F ``, `` <C-h>I ``, `` <C-h>K ``, `` <C-h>r ``, `` <C-h>t ``, `` <C-h>4s ``, `` <C-h><C-h> ``, `` <C-h><C-m> ``, `` <C-h><C-o> ``, `` <C-h><C-q> ``, `` <C-h><C-t> `` |
| `dashboard` | Open the system-stats Dashboard (Preferences) | normal: `` <space>bh ``, select: `` <space>bh `` |
| `search_in_files` | Open the project-wide Find in Files panel |  |
| `terminal` | Open an integrated terminal (PTY shell) | normal: `` <space>p' ``, select: `` <space>p' `` |
| `comint_shell` | Open a comint line-oriented shell buffer (emacs M-x shell) |  |
| `gud_gdb` | Run gdb in a comint buffer (emacs gud-gdb) |  |
| `gud_up` | Select the stack frame one level up (emacs gud-up) |  |
| `gud_down` | Select the stack frame one level down (emacs gud-down) |  |
| `gud_stepi` | Step one machine instruction (emacs gud-stepi) |  |
| `gud_tbreak` | Set a temporary breakpoint at the current line (emacs gud-tbreak) |  |
| `gud_print` | Print the expression at point in the debugger (emacs gud-print) |  |
| `gud_watch` | Watch the expression at point in the debugger (emacs gud-watch) |  |
| `gud_jump` | Set the debugger execution point to the current line (emacs gud-jump) |  |
| `gud_refresh` | Redisplay the debugger buffer (emacs gud-refresh) |  |
| `comint_kill_input` | Discard the pending comint input line (emacs comint-kill-input) |  |
| `comint_bol_or_process_mark` | Move to the process mark or beginning of line (emacs comint-bol-or-process-mark) |  |
| `comint_delchar_or_maybe_eof` | Delete char or send EOF on empty input (emacs comint-delchar-or-maybe-eof) |  |
| `comint_magic_space` | Expand history designators then insert a space (emacs comint-magic-space) |  |
| `comint_insert_previous_argument` | Insert the last argument of the previous command (emacs comint-insert-previous-argument) |  |
| `comint_get_next_from_history` | Yank the next history entry onto the input (emacs comint-get-next-from-history) |  |
| `comint_next_prompt` | Move to the next command prompt (emacs comint-next-prompt) |  |
| `comint_previous_prompt` | Move to the previous command prompt (emacs comint-previous-prompt) |  |
| `comint_show_output` | Scroll the last command's output to the top (emacs comint-show-output) |  |
| `comint_show_maximum_output` | Scroll the newest output to the bottom (emacs comint-show-maximum-output) |  |
| `comint_delete_output` | Delete the last command's output (emacs comint-delete-output) |  |
| `comint_write_output` | Write the last command's output to a file (emacs comint-write-output) |  |
| `comint_copy_old_input` | Copy the most recent input onto the input line (emacs comint-copy-old-input) |  |
| `comint_truncate_buffer` | Trim the comint scrollback to the maximum size (emacs comint-truncate-buffer) |  |
| `comint_strip_ctrl_m` | Strip carriage returns from the scrollback (emacs comint-strip-ctrl-m) |  |
| `comint_interrupt_subjob` | Send SIGINT to the comint child (emacs comint-interrupt-subjob) |  |
| `comint_stop_subjob` | Suspend the comint child with SIGTSTP (emacs comint-stop-subjob) |  |
| `comint_continue_subjob` | Resume the comint child with SIGCONT (emacs comint-continue-subjob) |  |
| `comint_quit_subjob` | Send SIGQUIT to the comint child (emacs comint-quit-subjob) |  |
| `comint_kill_subjob` | Send SIGKILL to the comint child (emacs comint-kill-subjob) |  |
| `comint_dynamic_list_input_ring` | List the comint input history (emacs comint-dynamic-list-input-ring) |  |
| `comint_history_isearch_backward_regexp` | Search the comint input history backward (emacs comint-history-isearch-backward-regexp) |  |
| `comint_run` | Run a program in a new comint buffer (emacs comint-run) |  |
| `shell_forward_command` | Move forward over a shell command on the input line (emacs shell-forward-command) |  |
| `shell_backward_command` | Move backward over a shell command on the input line (emacs shell-backward-command) |  |
| `run_config_manager` | Manage run/debug configurations | normal: `` <space>Rc ``, `` <space>Re ``, `` <space>cm ``, `` <space>pi ``, select: `` <space>Rc ``, `` <space>Re ``, `` <space>cm ``, `` <space>pi `` |
| `run_active_config` | Run the active run configuration | normal: `` <F5> ``, `` <space>Rr ``, `` <space>cC ``, `` <space>pc ``, `` <space>pu ``, `` <C-c><C-c> ``, select: `` <space>Rr ``, `` <space>cC ``, `` <space>pc ``, `` <space>pu ``, `` <C-c><C-c> ``, insert: `` <C-c><C-c> `` |
| `clear_run_output` | Clear the Run tool window output | normal: `` <space>Rl ``, `` <space>Rx ``, `` <space>ck ``, select: `` <space>Rl ``, `` <space>Rx ``, `` <space>ck `` |
| `rerun_last_run` | Re-run the last command in the Run console | normal: `` <space>RR ``, `` <space>cr ``, `` <C-c><C-r> ``, select: `` <space>RR ``, `` <space>cr ``, `` <C-c><C-r> ``, insert: `` <C-c><C-r> `` |
| `run_next_error` | Jump to the next file:line in the run output | normal: `` <C-x>` ``, `` <space>Rn ``, select: `` <C-x>` ``, `` <space>Rn ``, insert: `` <C-x>` `` |
| `run_prev_error` | Jump to the previous file:line in the run output | normal: `` <space>Rp ``, select: `` <space>Rp `` |
| `reveal_in_tree` | Reveal the current file in the project tree |  |
| `toggle_auto_reveal` | Toggle always-select-opened-file (autoscroll from source) | normal: `` <space>pV ``, select: `` <space>pV `` |
| `focus_file_tree` | Focus the project file tree panel | normal: `` <space>Wp ``, `` <space>Wt ``, select: `` <space>Wp ``, `` <space>Wt `` |
| `focus_structure` | Focus the structure/symbol outline panel | normal: `` <space>Wo ``, `` <space>Ws ``, select: `` <space>Wo ``, `` <space>Ws `` |
| `hide_active_tool_window` | Return focus to the editor, hiding the active tool window (JetBrains Shift-Esc) | normal: `` <space>Wq ``, select: `` <space>Wq `` |
| `jump_to_last_tool_window` | Toggle focus between the editor and the last tool window (JetBrains F12) | normal: `` <space>W<tab> ``, select: `` <space>W<tab> `` |
| `focus_bookmarks` | Focus the Bookmarks tool window (pinned files; JetBrains Bookmarks) | normal: `` <space>Wb ``, select: `` <space>Wb `` |
| `focus_marks_panel` | Focus the Marks tool window | normal: `` <space>Wk ``, select: `` <space>Wk `` |
| `focus_registers_panel` | Focus the Registers tool window | normal: `` <space>WR ``, select: `` <space>WR `` |
| `focus_jumplist_panel` | Focus the Jumplist tool window | normal: `` <space>Wj ``, select: `` <space>Wj `` |
| `focus_recent_panel` | Focus the Recent Files tool window | normal: `` <space>Wu ``, select: `` <space>Wu `` |
| `focus_todo_panel` | Focus the TODO tool window | normal: `` <space>Wd ``, select: `` <space>Wd `` |
| `focus_problems` | Focus the problems/diagnostics panel | normal: `` <space>We ``, select: `` <space>We `` |
| `focus_run_console` | Focus the Run console (scroll output with j/k/PgUp/PgDn) | normal: `` <space>Wr ``, select: `` <space>Wr `` |
| `focus_git_panel` | Focus the Git changes panel (j/k select, Enter opens) | normal: `` <space>Wg ``, `` <space>gG ``, select: `` <space>Wg ``, `` <space>gG `` |
| `focus_ci_panel` | Focus the CI status panel (GitHub Actions runs; Enter opens in browser) | normal: `` <space>Wc ``, select: `` <space>Wc `` |
| `toggle_bottom_zoom` | Maximize / restore the bottom panel | normal: `` <space>Wm ``, select: `` <space>Wm `` |
| `toggle_drawer_mid` | Fold / unfold the middle column of the bottom drawer | normal: `` <space>Wf ``, select: `` <space>Wf `` |
| `toggle_ide` | Toggle the IDE workbench (Zen / focus mode) | normal: `` <space>z ``, `` <space>Wz ``, select: `` <space>z ``, `` <space>Wz `` |
| `settings_page` | Open the settings page (config.toml editor) | normal: `` <space>S ``, select: `` <space>S `` |
| `goto_next_spell_error` | Move to the next misspelled word (]s) | normal: `` ]s `` |
| `goto_prev_spell_error` | Move to the previous misspelled word ([s) | normal: `` [s `` |
| `spell_add_good` | Mark word under cursor as correctly spelled (zg) | normal: `` zG ``, `` zg `` |
| `spell_add_bad` | Mark word under cursor as misspelled (zw) | normal: `` zW ``, `` zw `` |
| `spell_undo` | Undo a zg/zw for the word under cursor (zug) | normal: `` zuG ``, `` zuW ``, `` zug ``, `` zuw `` |
| `spell_suggest` | Show spelling suggestions for the word under cursor (z=) | normal: `` z= `` |
| `ispell_word` | Spell-check the word at point with aspell/hunspell (emacs ispell-word, M-$) |  |
| `flyspell_auto_correct_word` | Correct the word at point with the top suggestion (emacs flyspell-auto-correct-word) |  |
| `view_file` | Open a file read-only for viewing (emacs view-file, C-x C-r) |  |
| `view_buffer` | Make the current buffer read-only for viewing (emacs view-buffer) |  |
| `ispell_region` | Spell-check the selection with an external speller (emacs ispell-region) |  |
| `ispell_buffer` | Spell-check the whole buffer with an external speller (emacs ispell-buffer) |  |
| `ispell` | Spell-check the region or buffer with an external speller (emacs ispell) |  |
| `ispell_change_dictionary` | Set the ispell dictionary/language (emacs ispell-change-dictionary) |  |
| `ispell_kill_ispell` | Stop the ispell process (emacs ispell-kill-ispell) |  |
| `outline_next_visible_heading` | Move to the next outline heading (emacs outline-next-visible-heading) |  |
| `outline_previous_visible_heading` | Move to the previous outline heading (emacs outline-previous-visible-heading) |  |
| `outline_up_heading` | Move to the parent outline heading (emacs outline-up-heading) |  |
| `outline_forward_same_level` | Move to the next same-level heading (emacs outline-forward-same-level) |  |
| `outline_backward_same_level` | Move to the previous same-level heading (emacs outline-backward-same-level) |  |
| `outline_hide_subtree` | Fold the subtree of the heading at point (emacs outline-hide-subtree) |  |
| `outline_show_subtree` | Reveal the subtree of the heading at point (emacs outline-show-subtree) |  |
| `outline_hide_entry` | Fold this heading's body (emacs outline-hide-entry) |  |
| `outline_show_entry` | Reveal this heading's body (emacs outline-show-entry) |  |
| `outline_hide_body` | Fold all bodies, showing only headings (emacs outline-hide-body) |  |
| `outline_show_all` | Reveal all outline body text (emacs outline-show-all) |  |
| `outline_hide_sublevels` | Show only the top N levels of headings (emacs outline-hide-sublevels) |  |
| `outline_hide_leaves` | Fold bodies in the current subtree, keeping subheadings (emacs outline-hide-leaves) |  |
| `outline_show_children` | Reveal the immediate subheadings of the heading at point (emacs outline-show-children) |  |
| `outline_show_branches` | Reveal every subheading in the subtree at point (emacs outline-show-branches) |  |
| `outline_cycle` | Cycle the heading at point folded -> children -> subtree (emacs outline-cycle) |  |
| `outline_cycle_buffer` | Cycle the whole buffer show-all -> overview -> contents (emacs outline-cycle-buffer) |  |
| `fold_create` | Create a fold over the selection (zf) |  |
| `fold_toggle` | Toggle fold under cursor (za) | normal: `` zA ``, `` za ``, `` zi ``, `` <C-c>@<C-c> ``, select: `` <C-c>@<C-c> ``, insert: `` <C-c>@<C-c> `` |
| `fold_open` | Open fold under cursor (zo) | normal: `` zO ``, `` zo ``, `` zv ``, `` zx ``, `` <C-c><C-z> ``, `` <C-c>@<C-r> ``, `` <C-c>@<C-s> ``, select: `` <C-c><C-z> ``, `` <C-c>@<C-r> ``, `` <C-c>@<C-s> ``, insert: `` <C-c><C-z> ``, `` <C-c>@<C-r> ``, `` <C-c>@<C-s> `` |
| `fold_close` | Close fold under cursor (zc) | normal: `` zC ``, `` zc ``, `` <C-c><C-x> ``, `` <C-c>@<C-h> ``, select: `` <C-c><C-x> ``, `` <C-c>@<C-h> ``, insert: `` <C-c><C-x> ``, `` <C-c>@<C-h> `` |
| `fold_open_recursive` | Open fold under cursor and all nested folds (IntelliJ Expand Recursively) |  |
| `fold_close_recursive` | Close fold under cursor and all nested folds (IntelliJ Collapse Recursively) | normal: `` <C-c>@<C-l> ``, select: `` <C-c>@<C-l> ``, insert: `` <C-c>@<C-l> `` |
| `fold_open_all` | Open all folds (zR) | normal: `` zR ``, `` zX ``, `` zn ``, `` zr `` |
| `fold_close_all` | Close all folds (zM) | normal: `` zM ``, `` zN ``, `` zm ``, `` <C-x>$ ``, select: `` <C-x>$ ``, insert: `` <C-x>$ `` |
| `fold_delete` | Delete fold under cursor (zd) | normal: `` zD ``, `` zd `` |
| `fold_delete_all` | Delete all folds (zE) | normal: `` zE `` |
| `narrow_to_region` | Narrow the buffer to the selected region (SPC n r) | normal: `` <C-x>nn ``, `` <space>nr ``, select: `` <C-x>nn ``, `` <space>nr ``, insert: `` <C-x>nn `` |
| `widen` | Widen: remove narrowing and reveal the whole buffer (SPC n w) | normal: `` <C-x>nw ``, `` <space>nw ``, select: `` <C-x>nw ``, `` <space>nw ``, insert: `` <C-x>nw `` |
| `narrow_to_function_indirect` | Narrow to the function in an indirect (split) view (SPC n F) | normal: `` <space>nF ``, select: `` <space>nF `` |
| `narrow_region_indirect` | Narrow to the selected region in an indirect (split) view (SPC n R) | normal: `` <space>nR ``, select: `` <space>nR `` |
| `layout_add_buffers` | Add another layout's buffers into the current windows (SPC l A) | normal: `` <space>lA ``, select: `` <space>lA `` |
| `winner_undo` | Undo the last window-layout change (winner-undo, SPC w u) | normal: `` <C-w>u ``, `` <C-w>.u ``, `` <space>wu ``, `` <space>w.u ``, select: `` <space>wu ``, `` <space>w.u `` |
| `winner_redo` | Redo a window-layout change (winner-redo, SPC w . U) | normal: `` <C-w>.U ``, `` <space>w.U ``, select: `` <space>w.U `` |
| `copy_version` | Display and copy the zemacs version to the clipboard (SPC f e v) | normal: `` <space>fev ``, select: `` <space>fev `` |
| `narrow_to_page_indirect` | Narrow to the page in an indirect (split) view (SPC n P) | normal: `` <space>nP ``, select: `` <space>nP `` |
| `kmacro_ring_next` | Cycle to the next macro in the ring (SPC K r n) | normal: `` <space>Krn ``, `` <C-x><C-k><C-k> ``, `` <C-x><C-k><C-n> ``, select: `` <space>Krn ``, `` <C-x><C-k><C-k> ``, `` <C-x><C-k><C-n> ``, insert: `` <C-x><C-k><C-k> ``, `` <C-x><C-k><C-n> `` |
| `kmacro_ring_prev` | Cycle to the previous macro in the ring (SPC K r p) | normal: `` <space>KrN ``, `` <space>Krp ``, `` <C-x><C-k><C-p> ``, select: `` <space>KrN ``, `` <space>Krp ``, `` <C-x><C-k><C-p> ``, insert: `` <C-x><C-k><C-p> `` |
| `kmacro_ring_delete` | Delete the head macro in the ring (SPC K r d) | normal: `` <space>Krd ``, `` <C-x><C-k>d ``, select: `` <space>Krd ``, `` <C-x><C-k>d ``, insert: `` <C-x><C-k>d `` |
| `kmacro_ring_swap` | Swap the first two macros in the ring (SPC K r s) | normal: `` <space>Krs ``, select: `` <space>Krs `` |
| `kmacro_ring_view` | View the head macro in the ring (SPC K r L) | normal: `` <space>KrL ``, `` <C-x><C-k>e ``, `` <C-x><C-k>l ``, `` <C-x><C-k><C-e> ``, `` <C-x><C-k><ret> ``, `` <C-x><C-k><space> ``, select: `` <space>KrL ``, `` <C-x><C-k>e ``, `` <C-x><C-k>l ``, `` <C-x><C-k><C-e> ``, `` <C-x><C-k><ret> ``, `` <C-x><C-k><space> ``, insert: `` <C-x><C-k>e ``, `` <C-x><C-k>l ``, `` <C-x><C-k><C-e> ``, `` <C-x><C-k><ret> ``, `` <C-x><C-k><space> `` |
| `kmacro_to_register` | Write the last macro to a register (SPC K e r) | normal: `` <space>Ken ``, `` <space>Ker ``, `` <C-x><C-k>b ``, `` <C-x><C-k>n ``, `` <C-x><C-k>x ``, select: `` <space>Ken ``, `` <space>Ker ``, `` <C-x><C-k>b ``, `` <C-x><C-k>n ``, `` <C-x><C-k>x ``, insert: `` <C-x><C-k>b ``, `` <C-x><C-k>n ``, `` <C-x><C-k>x `` |
| `kmacro_add_counter` | Add [count] to the keyboard-macro counter (SPC K c a) | normal: `` <space>Kca ``, `` <C-x><C-k><C-a> ``, `` <C-x><C-k><C-c> ``, `` <C-x><C-k><C-f> ``, select: `` <space>Kca ``, `` <C-x><C-k><C-a> ``, `` <C-x><C-k><C-c> ``, `` <C-x><C-k><C-f> ``, insert: `` <C-x><C-k><C-a> ``, `` <C-x><C-k><C-c> ``, `` <C-x><C-k><C-f> `` |
| `kmacro_insert_counter` | Insert the macro counter value, then increment (SPC K c c) | normal: `` <space>Kcc ``, `` <C-x><C-k><C-i> ``, select: `` <space>Kcc ``, `` <C-x><C-k><C-i> ``, insert: `` <C-x><C-k><C-i> `` |
| `kmacro_set_counter` | Set the keyboard-macro counter to [count] (emacs kmacro-set-counter) |  |
| `kmacro_set_format` | Set the macro-counter insert format, e.g. %03d (emacs kmacro-set-format) |  |
| `kmacro_name_last_macro` | Name the last kbd macro and register it as an invokable command (emacs kmacro-name-last-macro) |  |
| `insert_kbd_macro` | Insert a textual definition of the last kbd macro (emacs insert-kbd-macro) |  |
| `apply_macro_to_region_lines` | Run the last kbd macro at the start of each line in the region (emacs apply-macro-to-region-lines) |  |
| `kmacro_end_or_call_macro` | End recording, or call the last kbd macro (emacs kmacro-end-or-call-macro, F4) | normal: `` <F4> `` |
| `kmacro_end_or_call_macro_repeat` | Repeat-variant of end-or-call macro (emacs kmacro-end-or-call-macro-repeat) |  |
| `kmacro_edit_macro` | Edit the last kbd macro's keys as text (emacs edit-kbd-macro / kmacro-edit-macro) |  |
| `kmacro_step_edit_macro` | Edit the whole last kbd macro (emacs kmacro-step-edit-macro; no per-key stepping) |  |
| `kmacro_edit_lossage` | Edit the recently pressed keys as a macro (emacs kmacro-edit-lossage) |  |
| `kmacro_bind_to_key` | Report the config binding for the last kbd macro (emacs kmacro-bind-to-key) |  |
| `kmacro_redisplay` | Refresh display during macro execution (emacs kmacro-redisplay) |  |
| `toggle_readonly` | Toggle the buffer's read-only (writable) state (SPC b w) | normal: `` <space>bw ``, `` <C-x><C-q> ``, select: `` <space>bw ``, `` <C-x><C-q> ``, insert: `` <C-x><C-q> `` |
| `toggle_window_dedication` | Toggle window dedication (spacemacs SPC w t) | normal: `` <C-w>t ``, `` <space>wt ``, select: `` <space>wt `` |
| `toggle_subword` | Toggle sub-word w/b/e motions (spacemacs SPC t c) | normal: `` <space>tc ``, select: `` <space>tc `` |
| `toggle_superword` | Toggle super-word w/b/e motions: symbols join words (emacs superword-mode, SPC t C) | normal: `` <space>tC ``, select: `` <space>tC `` |
| `toggle_auto_fill` | Toggle auto-fill: wrap at text-width while typing (spacemacs SPC t F) | normal: `` <space>tF ``, select: `` <space>tF `` |
| `toggle_follow_mode` | Toggle follow mode: windows on the same doc scroll together (spacemacs SPC w f) | normal: `` <C-w>f ``, `` <space>wf ``, select: `` <space>wf `` |
| `subword_w` | Next word start, sub-word aware (w) | normal: `` w `` |
| `subword_b` | Previous word start, sub-word aware (b) | normal: `` b `` |
| `subword_e` | Next word end, sub-word aware (e) | normal: `` e `` |
| `subword_extend_w` | Extend to next word start, sub-word aware |  |
| `subword_extend_b` | Extend to previous word start, sub-word aware |  |
| `subword_extend_e` | Extend to next word end, sub-word aware |  |
| `paredit_slurp_forward` | Paredit: slurp the next s-expression forward (SPC k s) | normal: `` <space>ks ``, `` <space>k`s ``, select: `` <space>ks ``, `` <space>k`s `` |
| `paredit_barf_forward` | Paredit: barf the last s-expression forward (SPC k b) | normal: `` <space>kb ``, select: `` <space>kb `` |
| `paredit_slurp_backward` | Paredit: slurp the previous s-expression backward (SPC k S) | normal: `` <space>kS ``, select: `` <space>kS `` |
| `paredit_barf_backward` | Paredit: barf the first s-expression backward (SPC k B) | normal: `` <space>kB ``, select: `` <space>kB `` |
| `paredit_splice` | Paredit: splice/unwrap the enclosing s-expression (SPC k W) | normal: `` <space>kW ``, select: `` <space>kW `` |
| `paredit_raise` | Paredit: raise the current s-expression (SPC k r) | normal: `` <space>kr ``, select: `` <space>kr `` |
| `paredit_transpose` | Paredit: transpose the s-expressions around point (SPC k t) | normal: `` <space>kt ``, `` <space>k`p ``, `` <space>k`t ``, select: `` <space>kt ``, `` <space>k`p ``, `` <space>k`t `` |
| `paredit_split` | Paredit: split the enclosing list at point (SPC j s) | normal: `` <space>jS ``, `` <space>js ``, select: `` <space>jS ``, `` <space>js `` |
| `paredit_absorb` | Paredit: absorb the previous sexp into the current form (SPC k a) | normal: `` <space>ka ``, select: `` <space>ka `` |
| `paredit_convolute` | Paredit: convolute — swap enclosing/inner prefixes (SPC k c) | normal: `` <space>kc ``, select: `` <space>kc `` |
| `buffer_swap_window_1` | Swap current buffer with window 1 (SPC b . M-1) | normal: `` <space>b.<A-1> ``, select: `` <space>b.<A-1> `` |
| `buffer_swap_window_2` | Swap current buffer with window 2 (SPC b . M-2) | normal: `` <space>b.<A-2> ``, select: `` <space>b.<A-2> `` |
| `buffer_swap_window_3` | Swap current buffer with window 3 (SPC b . M-3) | normal: `` <space>b.<A-3> ``, select: `` <space>b.<A-3> `` |
| `buffer_swap_window_4` | Swap current buffer with window 4 (SPC b . M-4) | normal: `` <space>b.<A-4> ``, select: `` <space>b.<A-4> `` |
| `buffer_swap_window_5` | Swap current buffer with window 5 (SPC b . M-5) | normal: `` <space>b.<A-5> ``, select: `` <space>b.<A-5> `` |
| `buffer_swap_window_6` | Swap current buffer with window 6 (SPC b . M-6) | normal: `` <space>b.<A-6> ``, select: `` <space>b.<A-6> `` |
| `buffer_swap_window_7` | Swap current buffer with window 7 (SPC b . M-7) | normal: `` <space>b.<A-7> ``, select: `` <space>b.<A-7> `` |
| `buffer_swap_window_8` | Swap current buffer with window 8 (SPC b . M-8) | normal: `` <space>b.<A-8> ``, select: `` <space>b.<A-8> `` |
| `buffer_swap_window_9` | Swap current buffer with window 9 (SPC b . M-9) | normal: `` <space>b.<A-9> ``, select: `` <space>b.<A-9> `` |
| `paredit_splice_kill_forward` | Paredit: splice, killing forward (SPC k e) | normal: `` <space>ke ``, select: `` <space>ke `` |
| `paredit_splice_kill_backward` | Paredit: splice, killing backward (SPC k E) | normal: `` <space>kE ``, select: `` <space>kE `` |
| `paredit_insert_sexp_after` | Paredit: insert a new () sexp after the current one (SPC k )) | normal: `` <space>k) ``, select: `` <space>k) `` |
| `paredit_insert_sexp_before` | Paredit: insert a new () sexp before the current one (SPC k () | normal: `` <space>k( ``, select: `` <space>k( `` |
| `fold_next` | Move to next fold (zj) | normal: `` ]z ``, `` zj `` |
| `fold_prev` | Move to previous fold (zk) | normal: `` [z ``, `` zk `` |
| `goto_line_last_nonblank` | Goto last non-blank on line (g_) | normal: `` g_ `` |
| `goto_line_middle` | Goto middle of text line (gM) | normal: `` gM ``, `` gm `` |
| `goto_byte` | Goto byte {count} in buffer (go) | normal: `` go `` |
| `goto_prev_unmatched_paren` | Goto previous unmatched ( ([() | normal: `` [( ``, `` <space>j( ``, select: `` <space>j( `` |
| `goto_prev_unmatched_brace` | Goto previous unmatched { ([{) | normal: `` [{ `` |
| `goto_next_unmatched_paren` | Goto next unmatched ) (]) | normal: `` ]) `` |
| `goto_next_unmatched_brace` | Goto next unmatched } (]}) | normal: `` ]} `` |
| `goto_prev_mark` | Goto previous lowercase mark ([`) | normal: `` [` `` |
| `goto_next_mark` | Goto next lowercase mark (]`) | normal: `` ]` `` |
| `goto_prev_mark_line` | Goto previous lowercase mark, line start ([']) | normal: `` [' `` |
| `goto_next_mark_line` | Goto next lowercase mark, line start (]') | normal: `` ]' `` |
| `yank_file_path` | Yank current file path to clipboard | normal: `` <space>fyC ``, `` <space>fyY ``, `` <space>fyy ``, select: `` <space>fyC ``, `` <space>fyY ``, `` <space>fyy `` |
| `yank_file_name` | Yank current file name to clipboard | normal: `` <space>fyN ``, `` <space>fyn ``, select: `` <space>fyN ``, `` <space>fyn `` |
| `yank_file_path_with_line` | Yank current file path:line to clipboard | normal: `` <space>fyL ``, `` <space>fyl ``, select: `` <space>fyL ``, `` <space>fyl `` |
| `yank_file_path_with_line_col` | Yank current file path:line:col to clipboard | normal: `` <space>fyc ``, select: `` <space>fyc `` |
| `yank_file_dir` | Yank current file's directory to clipboard | normal: `` <space>fyD ``, `` <space>fyd ``, select: `` <space>fyD ``, `` <space>fyd `` |
| `copy_remote_url` | Copy web permalink (host/blob/<sha>/path#Ln) for current line | normal: `` <space>glC ``, `` <space>glL ``, `` <space>glP ``, select: `` <space>glC ``, `` <space>glL ``, `` <space>glP `` |
| `open_remote_url` | Open current line's web permalink in the browser | normal: `` <space>glc ``, `` <space>gll ``, `` <space>glp ``, select: `` <space>glc ``, `` <space>gll ``, `` <space>glp `` |
| `open_url_under_cursor` | Open the URL under the cursor in the browser |  |
| `goto_address_mode` | Report that URLs are always openable at point (emacs goto-address-mode) |  |
| `duplicate_selection_down` | Duplicate current line(s) downward |  |
| `duplicate_selection_up` | Duplicate current line(s) upward |  |
| `move_text_line_down` | Move current line(s) down past the next line |  |
| `move_text_line_up` | Move current line(s) up past the previous line |  |
| `count_selection` | Count chars/words/lines in selection | normal: `` <space>xc ``, select: `` <space>xc `` |
| `match_brackets` | Goto matching bracket | normal: `` <space>k% ``, select: `` <space>k% `` |
| `match_brackets_or_goto_percent` | Goto matching bracket, or {count} percent through the file | normal: `` % ``, select: `` % `` |
| `surround_add` | Surround add |  |
| `surround_replace` | Surround replace |  |
| `surround_delete` | Surround delete |  |
| `select_textobject_around` | Select around object | select: `` a `` |
| `select_textobject_inner` | Select inside object | select: `` i `` |
| `change_textobject_inner` | Change inside object (ci) | normal: `` ci `` |
| `change_textobject_around` | Change around object (ca) | normal: `` ca `` |
| `delete_textobject_inner` | Delete inside object (di) | normal: `` di `` |
| `delete_textobject_around` | Delete around object (da) | normal: `` da `` |
| `yank_textobject_inner` | Yank inside object (yi) | normal: `` yi `` |
| `yank_textobject_around` | Yank around object (ya) | normal: `` ya `` |
| `delete_find_char_forward` | Delete to next char (df) | normal: `` df `` |
| `delete_till_char_forward` | Delete till next char (dt) | normal: `` dt `` |
| `zap_to_char` | Kill through the next char, inclusive (emacs zap-to-char, M-z) |  |
| `zap_up_to_char` | Kill up to the next char, exclusive (emacs zap-up-to-char) |  |
| `five_by_five` | Play 5x5, the light-flipping puzzle (emacs 5x5) |  |
| `solitaire` | Play English peg solitaire (emacs solitaire) |  |
| `hanoi` | Watch the Towers of Hanoi solution (emacs hanoi) |  |
| `life` | Run Conway's Game of Life (emacs life) |  |
| `doctor` | Talk to the ELIZA psychotherapist (emacs doctor) |  |
| `gomoku` | Play five-in-a-row against the computer (emacs gomoku) |  |
| `butterfly` | Flip the desired bit with a butterfly (emacs butterfly, xkcd 378) |  |
| `mpuz` | Play the multiplication puzzle (emacs mpuz) |  |
| `bubbles` | Play the bubbles same-game (emacs bubbles) |  |
| `blackbox` | Play blackbox, the ray-tracing puzzle (emacs blackbox) |  |
| `snake` | Play snake (emacs snake) |  |
| `tetris` | Play tetris (emacs tetris) |  |
| `pong` | Play pong against the computer (emacs pong) |  |
| `space_invaders` | Play Space Invaders |  |
| `breakout` | Play Breakout, the brick-breaker |  |
| `asteroids` | Play Asteroids |  |
| `frogger` | Play Frogger, cross the traffic |  |
| `twenty_forty_eight` | Play 2048, the sliding-tile puzzle |  |
| `minesweeper` | Play Minesweeper |  |
| `tic_tac_toe` | Play Tic-Tac-Toe against the computer |  |
| `connect_four` | Play Connect Four against the computer |  |
| `reversi` | Play Reversi / Othello against the computer |  |
| `sokoban` | Play Sokoban, push the boxes onto the goals |  |
| `sudoku` | Play Sudoku |  |
| `fifteen` | Play the 15-puzzle sliding tiles |  |
| `hangman` | Play Hangman, guess the word |  |
| `wordle` | Play Wordle, guess the five-letter word |  |
| `mastermind` | Play Mastermind, crack the code |  |
| `pacman` | Play Pac-Man |  |
| `landmark` | Play landmark, find the hidden tree (emacs landmark) |  |
| `centipede` | Play Centipede |  |
| `missile_command` | Play Missile Command, defend the cities |  |
| `tron` | Play Tron light-cycles against the computer |  |
| `flappy` | Play Flappy, flap through the pipes |  |
| `checkers` | Play Checkers / draughts against the computer |  |
| `battleship` | Play Battleship against the computer |  |
| `blackjack` | Play Blackjack against the dealer |  |
| `yahtzee` | Play Yahtzee |  |
| `simon` | Play Simon, the memory game |  |
| `galaga` | Play Galaga |  |
| `dig_dug` | Play Dig Dug |  |
| `donkey_kong` | Play Donkey Kong |  |
| `bomberman` | Play Bomberman |  |
| `lunar_lander` | Play Lunar Lander |  |
| `chess` | Play chess against the computer |  |
| `mancala` | Play Mancala / Kalah against the computer |  |
| `video_poker` | Play Jacks-or-Better video poker |  |
| `klondike` | Play Klondike solitaire |  |
| `nonogram` | Play nonogram / picross |  |
| `xref_find_references` | Find references to a symbol across the workspace (emacs xref-find-references) |  |
| `project_find_file` | Find a file under the project root (emacs project-find-file) |  |
| `diffmode` | Open the unified-diff viewer (emacs diff-mode) |  |
| `diff_hunk_kill` | Kill the diff hunk at point (emacs diff-hunk-kill) |  |
| `diff_file_kill` | Kill the whole file section at point (emacs diff-file-kill) |  |
| `diff_split_hunk` | Split the diff hunk at point into two (emacs diff-split-hunk) |  |
| `diff_reverse_direction` | Reverse the direction of the diff (emacs diff-reverse-direction) |  |
| `diff_context_to_unified` | Convert a context diff to a unified diff (emacs diff-context->unified) |  |
| `diff_unified_to_context` | Convert a unified diff to a context diff (emacs diff-unified->context) |  |
| `diff_delete_trailing_whitespace` | Strip trailing whitespace from added lines (emacs diff-delete-trailing-whitespace) |  |
| `diff_restrict_view` | Narrow the diff buffer to the hunk at point (emacs diff-restrict-view) |  |
| `diff_apply_hunk` | Apply the diff hunk at point to its target file (emacs diff-apply-hunk) |  |
| `diff_apply_buffer` | Apply every hunk in the diff buffer to its target files (emacs diff-apply-buffer) |  |
| `picture` | Draw ASCII pictures on a canvas (emacs picture-mode) |  |
| `picture_mode` | Toggle picture-mode overwrite drawing on the buffer (emacs picture-mode) |  |
| `edit_picture` | Enter picture-mode on the current buffer (emacs edit-picture) |  |
| `picture_movement_right` | Picture-mode: draw toward the right (emacs picture-movement-right) |  |
| `picture_movement_left` | Picture-mode: draw toward the left (emacs picture-movement-left) |  |
| `picture_movement_up` | Picture-mode: draw upward (emacs picture-movement-up) |  |
| `picture_movement_down` | Picture-mode: draw downward (emacs picture-movement-down) |  |
| `picture_movement_nw` | Picture-mode: draw up-and-left (emacs picture-movement-nw) |  |
| `picture_movement_ne` | Picture-mode: draw up-and-right (emacs picture-movement-ne) |  |
| `picture_movement_sw` | Picture-mode: draw down-and-left (emacs picture-movement-sw) |  |
| `picture_movement_se` | Picture-mode: draw down-and-right (emacs picture-movement-se) |  |
| `picture_motion` | Picture-mode: move point in the drawing direction (emacs picture-motion) |  |
| `picture_motion_reverse` | Picture-mode: move point opposite the drawing direction (emacs picture-motion-reverse) |  |
| `picture_set_tab_stops` | Picture-mode: set tab stops from this line (emacs picture-set-tab-stops) |  |
| `picture_tab` | Picture-mode: move to the next tab stop (emacs picture-tab) |  |
| `picture_tab_search` | Picture-mode: move under next word-start above (emacs picture-tab-search) |  |
| `picture_open_line` | Picture-mode: split the line at point (emacs picture-open-line) |  |
| `picture_clear_line` | Picture-mode: clear to end of line (emacs picture-clear-line) |  |
| `picture_clear_column` | Picture-mode: blank columns after point (emacs picture-clear-column) |  |
| `picture_backward_clear_column` | Picture-mode: blank columns before point (emacs picture-backward-clear-column) |  |
| `picture_clear_rectangle_to_register` | Picture-mode: clear rectangle into a register (emacs picture-clear-rectangle-to-register) |  |
| `picture_yank_rectangle` | Picture-mode: overlay the killed rectangle (emacs picture-yank-rectangle) |  |
| `picture_yank_rectangle_from_register` | Picture-mode: overlay a register's rectangle (emacs picture-yank-rectangle-from-register) |  |
| `twocol_two_columns` | Two-column: create a side-by-side partner buffer (emacs 2C-two-columns) |  |
| `twocol_associate_buffer` | Two-column: associate the other window's buffer (emacs 2C-associate-buffer) |  |
| `twocol_split` | Two-column: split the buffer at point into two columns (emacs 2C-split) |  |
| `twocol_merge` | Two-column: merge the partner column back in (emacs 2C-merge) |  |
| `twocol_dissociate` | Two-column: break the association (emacs 2C-dissociate) |  |
| `twocol_newline` | Two-column: newline in both columns (emacs 2C-newline) |  |
| `table` | Edit a text table (emacs table.el) |  |
| `table_recognize` | Recognize the ASCII table at point and report its dimensions (emacs table-recognize) |  |
| `table_recognize_region` | Recognize the table in the selection and report its dimensions (emacs table-recognize-region) |  |
| `table_recognize_table` | Recognize the whole table at point (emacs table-recognize-table) |  |
| `table_recognize_cell` | Report the table cell at point (emacs table-recognize-cell) |  |
| `table_unrecognize` | Deactivate table recognition at point (emacs table-unrecognize) |  |
| `table_unrecognize_region` | Deactivate table recognition in the selection (emacs table-unrecognize-region) |  |
| `table_unrecognize_table` | Deactivate recognition of the table at point (emacs table-unrecognize-table) |  |
| `table_unrecognize_cell` | Deactivate recognition of the cell at point (emacs table-unrecognize-cell) |  |
| `table_query_dimension` | Report the size of the table cell and table at point (emacs table-query-dimension) |  |
| `table_justify` | Cycle the justification of the current table column (emacs table-justify) |  |
| `table_widen_cell` | Widen the current table column by one column (emacs table-widen-cell) |  |
| `table_narrow_cell` | Narrow the current table column by one column (emacs table-narrow-cell) |  |
| `table_heighten_cell` | Heighten the current table row by one line (emacs table-heighten-cell) |  |
| `table_shorten_cell` | Shorten the current table row by one line (emacs table-shorten-cell) |  |
| `table_span_cell` | Merge the current table cell with the one to its right (emacs table-span-cell) |  |
| `table_split_cell` | Split the current table cell vertically at its middle (emacs table-split-cell) |  |
| `table_split_cell_horizontally` | Split the current table cell into two columns (emacs table-split-cell-horizontally) |  |
| `table_split_cell_vertically` | Split the current table cell into two rows (emacs table-split-cell-vertically) |  |
| `table_insert_sequence` | Fill table cells from point with an incrementing sequence (emacs table-insert-sequence) |  |
| `table_generate_source` | Emit HTML source for the table at point (emacs table-generate-source) |  |
| `table_capture` | Capture the selected plain text into a table (emacs table-capture) |  |
| `table_release` | Release the table at point back to plain text (emacs table-release) |  |
| `table_fixed_width_mode` | Toggle table fixed-width mode (emacs table-fixed-width-mode) |  |
| `fortran_next_statement` | Move to the next fixed-form Fortran statement (emacs fortran-next-statement) |  |
| `fortran_previous_statement` | Move to the previous fixed-form Fortran statement (emacs fortran-previous-statement) |  |
| `fortran_beginning_of_block` | Move to the opening of the Fortran block at point (emacs fortran-beginning-of-block) |  |
| `fortran_end_of_block` | Move to the END of the Fortran block at point (emacs fortran-end-of-block) |  |
| `f90_next_statement` | Move to the next free-form F90 statement (emacs f90-next-statement) |  |
| `f90_previous_statement` | Move to the previous free-form F90 statement (emacs f90-previous-statement) |  |
| `f90_next_block` | Move to the next F90 block opening (emacs f90-next-block) |  |
| `f90_previous_block` | Move to the previous F90 block opening (emacs f90-previous-block) |  |
| `f90_beginning_of_block` | Move to the opening of the F90 block at point (emacs f90-beginning-of-block) |  |
| `f90_end_of_block` | Move to the end of the F90 block at point (emacs f90-end-of-block) |  |
| `fortran_split_line` | Break the line at point onto a Fortran continuation line (emacs fortran-split-line) |  |
| `fortran_join_line` | Join the current line with the following Fortran continuation line (emacs fortran-join-line) |  |
| `fortran_comment_region` | Comment (or uncomment) the selected lines as Fortran comments (emacs fortran-comment-region) |  |
| `fortran_indent_subprogram` | Re-indent the buffer by fixed-form Fortran block nesting (emacs fortran-indent-subprogram) |  |
| `fortran_strip_sequence_nos` | Delete sequence numbers in columns 73+ on every line (emacs fortran-strip-sequence-nos) |  |
| `fortran_column_ruler` | Show the fixed-form Fortran column ruler in the echo area (emacs fortran-column-ruler) |  |
| `fortran_window_create` | Report the fixed-form column-72 boundary (emacs fortran-window-create) |  |
| `fortran_window_create_momentarily` | Report the fixed-form column-72 boundary momentarily (emacs fortran-window-create-momentarily) |  |
| `fortran_mode` | Enter fixed-form Fortran mode (emacs fortran-mode) |  |
| `f90_mode` | Enter free-form Fortran/F90 mode (emacs f90-mode) |  |
| `facemenu` | Browse faces and colors (emacs list-faces-display / facemenu) |  |
| `bookmark_bmenu_list` | List bookmarks in an overlay (emacs bookmark-bmenu-list) |  |
| `proced` | Open the process viewer/manager (emacs proced) |  |
| `zone` | Run the zone screen-saver (emacs zone) |  |
| `decipher` | Solve a cryptogram (emacs decipher) |  |
| `dunnet` | Play the dunnet text adventure (emacs dunnet) |  |
| `animate_birthday_present` | Animate a birthday-present message (emacs animate-birthday-present) |  |
| `dissociated_press` | Scramble the buffer with the travesty generator (emacs dissociated-press) |  |
| `spook` | Insert random NSA-bait phrases (emacs spook) |  |
| `studlify_region` | StudlyCaps the selected region (emacs studlify-region) |  |
| `studlify_buffer` | StudlyCaps the whole buffer (emacs studlify-buffer) |  |
| `studlify_word` | StudlyCaps the word after point (emacs studlify-word) |  |
| `indent_relative` | Indent to under the next indent point in the previous line (emacs indent-relative) |  |
| `indent_code_rigidly` | Shift region lines by [count] columns, skipping lines that start in a string (emacs indent-code-rigidly, = r) | normal: `` =r `` |
| `c_hungry_delete_forward` | Delete all whitespace after point, else one char (emacs c-hungry-delete-forward, SPC x d f) | normal: `` <space>xdf ``, select: `` <space>xdf `` |
| `c_hungry_delete_backwards` | Delete all whitespace before point, else one char (emacs c-hungry-delete-backwards, SPC x d b) | normal: `` <space>xdb ``, select: `` <space>xdb `` |
| `c_beginning_of_defun` | Move to the start of the function at point (emacs c-beginning-of-defun) |  |
| `c_end_of_defun` | Move to the end of the function at point (emacs c-end-of-defun) |  |
| `c_mark_function` | Select the whole function around point (emacs c-mark-function) |  |
| `c_beginning_of_statement` | Move to the start of the C statement at point (emacs c-beginning-of-statement) |  |
| `c_end_of_statement` | Move to the end of the C statement at point (emacs c-end-of-statement) |  |
| `c_forward_conditional` | Move forward across a preprocessor conditional (emacs c-forward-conditional) |  |
| `c_backward_conditional` | Move backward across a preprocessor conditional (emacs c-backward-conditional) |  |
| `c_up_conditional` | Move up out of the containing preprocessor conditional (emacs c-up-conditional) |  |
| `c_indent_line_or_region` | Re-indent the current line or the selected lines (emacs c-indent-line-or-region) |  |
| `c_indent_defun` | Re-indent the whole function at point (emacs c-indent-defun) |  |
| `c_ts_mode_indent_defun` | Re-indent the whole function at point via tree-sitter (emacs c-ts-mode-indent-defun) |  |
| `c_indent_exp` | Re-indent the balanced expression after point (emacs c-indent-exp) |  |
| `c_fill_paragraph` | Fill the C comment block around point (emacs c-fill-paragraph, M-q) |  |
| `c_backslash_region` | Align trailing backslash continuations in the region (emacs c-backslash-region) |  |
| `c_context_line_break` | Break the line, continuing a comment or macro (emacs c-context-line-break) |  |
| `c_toggle_auto_newline` | Toggle auto-newline insertion in C mode (emacs c-toggle-auto-newline) |  |
| `c_toggle_hungry_state` | Toggle hungry-delete state in C mode (emacs c-toggle-hungry-state) |  |
| `c_toggle_electric_state` | Toggle electric behavior in C mode (emacs c-toggle-electric-state) |  |
| `c_show_syntactic_information` | Report the tree-sitter node kind at point (emacs c-show-syntactic-information) |  |
| `c_macro_expand` | Expand C preprocessor macros in the region via cpp (emacs c-macro-expand) |  |
| `c_set_style` | Report the C indentation style (emacs c-set-style) |  |
| `ps_print_buffer` | Print the buffer as PostScript via lpr (emacs ps-print-buffer) |  |
| `ps_print_region` | Print the region as PostScript via lpr (emacs ps-print-region) |  |
| `ps_print_buffer_with_faces` | Print the buffer as PostScript (plain, no faces) (emacs ps-print-buffer-with-faces) |  |
| `ps_print_region_with_faces` | Print the region as PostScript (plain, no faces) (emacs ps-print-region-with-faces) |  |
| `ps_spool_buffer` | Spool the buffer as PostScript for later printing (emacs ps-spool-buffer) |  |
| `ps_spool_region` | Spool the region as PostScript for later printing (emacs ps-spool-region) |  |
| `ps_spool_buffer_with_faces` | Spool the buffer as PostScript (plain, no faces) (emacs ps-spool-buffer-with-faces) |  |
| `ps_spool_region_with_faces` | Spool the region as PostScript (plain, no faces) (emacs ps-spool-region-with-faces) |  |
| `ps_despool` | Print the accumulated PostScript spool via lpr (emacs ps-despool) |  |
| `delete_find_char_backward` | Delete to prev char (dF) | normal: `` dF `` |
| `delete_till_char_backward` | Delete till prev char (dT) | normal: `` dT `` |
| `change_find_char_forward` | Change to next char (cf) | normal: `` cf `` |
| `change_till_char_forward` | Change till next char (ct) | normal: `` ct `` |
| `change_find_char_backward` | Change to prev char (cF) | normal: `` cF `` |
| `change_till_char_backward` | Change till prev char (cT) | normal: `` cT `` |
| `yank_find_char_forward` | Yank to next char (yf) | normal: `` yf `` |
| `yank_till_char_forward` | Yank till next char (yt) | normal: `` yt `` |
| `yank_find_char_backward` | Yank to prev char (yF) | normal: `` yF `` |
| `yank_till_char_backward` | Yank till prev char (yT) | normal: `` yT `` |
| `set_mark` | Set mark (m{a-z} buffer, m{A-Z} global) | normal: `` m `` |
| `goto_mark` | Goto mark exact (`{a-z/A-Z/0-9}, `` for last jump) | normal: `` ` ``, `` g` `` |
| `goto_mark_line` | Goto mark line ('{a-z/A-Z/0-9}, '' for last jump) | normal: `` ' ``, `` g' `` |
| `repeat_substitute` | Repeat last :substitute (&) | normal: `` & `` |
| `repeat_substitute_global` | Repeat last :substitute on whole file (g&) | normal: `` g& `` |
| `vim_record_macro` | Record macro into register (q{reg}) | normal: `` q `` |
| `vim_replay_macro` | Replay macro from register (@{reg}) | normal: `` @ `` |
| `save_visual_selection` | Save the visual selection (for gv) |  |
| `reselect_visual` | Reselect the last visual area (gv) | normal: `` gv ``, select: `` gv `` |
| `mark_insert_exit` | Record the insert-exit position (for gi) |  |
| `insert_at_last_insert` | Insert at the last insert position (gi) | normal: `` gi `` |
| `goto_next_function` | Goto next function | normal: `` ][ ``, `` ]m `` |
| `goto_prev_function` | Goto previous function | normal: `` [] ``, `` [m `` |
| `goto_next_class` | Goto next type definition |  |
| `goto_prev_class` | Goto previous type definition |  |
| `goto_next_parameter` | Goto next parameter |  |
| `goto_prev_parameter` | Goto previous parameter |  |
| `goto_next_comment` | Goto next comment | normal: `` ]* ``, `` ]/ `` |
| `goto_prev_comment` | Goto previous comment | normal: `` [* ``, `` [/ `` |
| `goto_next_test` | Goto next test |  |
| `goto_prev_test` | Goto previous test |  |
| `goto_next_xml_element` | Goto next (X)HTML element |  |
| `goto_prev_xml_element` | Goto previous (X)HTML element |  |
| `goto_next_entry` | Goto next pairing |  |
| `goto_prev_entry` | Goto previous pairing |  |
| `goto_next_paragraph` | Goto next paragraph | normal: `` } ``, `` ]] ``, select: `` } `` |
| `goto_prev_paragraph` | Goto previous paragraph | normal: `` { ``, `` [[ ``, select: `` { `` |
| `move_sentence_forward` | Move to next sentence | normal: `` ) ``, select: `` ) `` |
| `move_sentence_backward` | Move to previous sentence | normal: `` ( ``, select: `` ( `` |
| `dap_launch` | Launch debug target | normal: `` <S-F5> ``, `` <space>dd ``, `` <C-c><C-d> ``, select: `` <space>dd ``, `` <C-c><C-d> ``, insert: `` <C-c><C-d> `` |
| `dap_restart` | Restart debugging session | normal: `` <space>dr ``, select: `` <space>dr `` |
| `dap_toggle_breakpoint` | Toggle breakpoint | normal: `` <F9> ``, `` <space>db ``, select: `` <space>db `` |
| `dap_continue` | Continue program execution | normal: `` <space>dc ``, select: `` <space>dc `` |
| `dap_run_to_cursor` | Run the debugger up to the cursor line (JetBrains Run To Cursor) | normal: `` <space>dC ``, select: `` <space>dC `` |
| `dap_pause` | Pause program execution | normal: `` <space>dp ``, select: `` <space>dp `` |
| `dap_step_in` | Step in | normal: `` <F11> ``, `` <space>di ``, select: `` <space>di `` |
| `dap_step_out` | Step out | normal: `` <S-F11> ``, `` <space>do ``, select: `` <space>do `` |
| `dap_next` | Step to next | normal: `` <F10> ``, `` <space>dn ``, select: `` <space>dn `` |
| `dap_variables` | List variables | normal: `` <space>dv ``, select: `` <space>dv `` |
| `dap_terminate` | End debug session | normal: `` <space>dq ``, select: `` <space>dq `` |
| `dap_edit_condition` | Edit breakpoint condition on current line |  |
| `dap_breakpoints_picker` | View all breakpoints in a picker (JetBrains View Breakpoints) | normal: `` <space>dB ``, select: `` <space>dB `` |
| `dap_edit_log` | Edit breakpoint log message on current line |  |
| `dap_switch_thread` | Switch current thread | normal: `` <space>dgt ``, select: `` <space>dgt `` |
| `dap_switch_stack_frame` | Switch stack frame | normal: `` <space>dgf ``, select: `` <space>dgf `` |
| `dap_enable_exceptions` | Enable exception breakpoints |  |
| `dap_disable_exceptions` | Disable exception breakpoints |  |
| `gdb_display_locals_buffer` | Show local variables of the current frame (emacs gdb-display-locals-buffer) | normal: `` <space>dgl ``, select: `` <space>dgl `` |
| `gdb_display_registers_buffer` | Show CPU registers of the current frame (emacs gdb-display-registers-buffer) | normal: `` <space>dgr ``, select: `` <space>dgr `` |
| `gdb_display_stack_for_thread` | Show the call stack of the current thread (emacs gdb-display-stack-for-thread) | normal: `` <space>dgs ``, select: `` <space>dgs `` |
| `gdb_display_locals_for_thread` | Show locals of the current thread's innermost frame (emacs gdb-display-locals-for-thread) |  |
| `gdb_display_registers_for_thread` | Show registers of the current thread's innermost frame (emacs gdb-display-registers-for-thread) |  |
| `gdb_display_disassembly_buffer` | Disassemble around the current frame PC (emacs gdb-display-disassembly-buffer) | normal: `` <space>dgd ``, select: `` <space>dgd `` |
| `gdb_display_disassembly_for_thread` | Disassemble around the current thread PC (emacs gdb-display-disassembly-for-thread) |  |
| `gdb_display_io_buffer` | Show the inferior IO / Run console (emacs gdb-display-io-buffer) | normal: `` <space>dgi ``, select: `` <space>dgi `` |
| `gdb_display_memory_buffer` | Read and hexdump target memory (emacs gdb-display-memory-buffer) | normal: `` <space>dgm ``, select: `` <space>dgm `` |
| `gdb_delete_breakpoint` | Delete the breakpoint on the current line (emacs gdb-delete-breakpoint) | normal: `` <space>dgk ``, select: `` <space>dgk `` |
| `gdb_edit_value` | Set a variable/expression value in the debugger (emacs gdb-edit-value) | normal: `` <space>dge ``, select: `` <space>dge `` |
| `gdb_many_windows` | Open the multi-pane debugger layout (emacs gdb-many-windows) | normal: `` <space>dgw ``, select: `` <space>dgw `` |
| `gdb_restore_windows` | Restore the debugger window layout (emacs gdb-restore-windows) |  |
| `gud_gdb_complete_command` | Complete the gdb command at point (emacs gud-gdb-complete-command) |  |
| `shell_pipe` | Pipe selections through shell command |  |
| `shell_pipe_to` | Pipe selections into shell command ignoring output |  |
| `shell_insert_output` | Insert shell command output before selections |  |
| `shell_append_output` | Append shell command output after selections |  |
| `shell_keep_pipe` | Filter selections with shell predicate |  |
| `suspend` | Suspend and return to shell | normal: `` <C-z> ``, `` <C-x><C-z> ``, select: `` <C-x><C-z> ``, insert: `` <C-x><C-z> `` |
| `rename_symbol` | Rename symbol | normal: `` <space>lr ``, select: `` <space>lr `` |
| `increment` | Increment item under cursor | normal: `` <C-a> ``, `` g<C-a> ``, `` <space>n+ ``, `` <space>n= ``, select: `` <C-a> ``, `` g<C-a> ``, `` <space>n+ ``, `` <space>n= `` |
| `decrement` | Decrement item under cursor | normal: `` g<C-x> ``, `` <space>n_ ``, `` <space>n<minus> ``, select: `` g<C-x> ``, `` <space>n_ ``, `` <space>n<minus> `` |
| `record_macro` | Record macro |  |
| `replay_macro` | Replay macro | normal: `` Q `` |
| `command_palette` | Open command palette | normal: `` <F1> ``, `` <A-x> ``, `` <C-x># ``, `` <C-x>) ``, `` <C-x>* ``, `` <C-x>. ``, `` <C-x>; ``, `` <C-x>e ``, `` <C-x>i ``, `` <C-x>m ``, `` <C-x>q ``, `` <C-x>rN ``, `` <C-x>xg ``, `` <C-x>xr ``, `` <C-x>xu ``, `` <space>? ``, `` <C-x><C-+> ``, `` <C-x><C-0> ``, `` <C-x><C-=> ``, `` <C-x><C-n> ``, `` <C-x><C-o> ``, `` <C-x><ret> ``, `` <C-x><C-k>r ``, `` <space><space> ``, `` <C-x><C-a><C-b> ``, select: `` <C-x># ``, `` <C-x>) ``, `` <C-x>* ``, `` <C-x>. ``, `` <C-x>; ``, `` <C-x>e ``, `` <C-x>i ``, `` <C-x>m ``, `` <C-x>q ``, `` <C-x>rN ``, `` <C-x>xg ``, `` <C-x>xr ``, `` <C-x>xu ``, `` <space>? ``, `` <C-x><C-+> ``, `` <C-x><C-0> ``, `` <C-x><C-=> ``, `` <C-x><C-n> ``, `` <C-x><C-o> ``, `` <C-x><ret> ``, `` <C-x><C-k>r ``, `` <space><space> ``, `` <C-x><C-a><C-b> ``, insert: `` <C-x># ``, `` <C-x>) ``, `` <C-x>* ``, `` <C-x>. ``, `` <C-x>; ``, `` <C-x>e ``, `` <C-x>i ``, `` <C-x>m ``, `` <C-x>q ``, `` <C-x>rN ``, `` <C-x>xg ``, `` <C-x>xr ``, `` <C-x>xu ``, `` <C-x><C-+> ``, `` <C-x><C-0> ``, `` <C-x><C-=> ``, `` <C-x><C-n> ``, `` <C-x><C-o> ``, `` <C-x><ret> ``, `` <C-x><C-k>r ``, `` <C-x><C-a><C-b> `` |
| `search_everywhere` | Search Everywhere: choose Files/Symbols/Text/Actions/Buffers (JetBrains) | normal: `` <space>sE ``, select: `` <space>sE `` |
| `recent_files_switcher` | Recent Files switcher: tool windows + recent files (SPC b r) | normal: `` <space>br ``, select: `` <space>br `` |
| `repl` | Open the embedded-language REPL (elisp/viml/stryke/awk/zsh) |  |
| `goto_word` | Jump to a two-character label | normal: `` <space>jl ``, `` <space>jw ``, select: `` <space>jl ``, `` <space>jw `` |
| `extend_to_word` | Extend to a two-character label |  |
| `goto_char` | Label every visible occurrence of a char and jump (vim-easymotion s) | normal: `` <space>ja ``, `` <space>je ``, select: `` <space>ja ``, `` <space>je `` |
| `extend_to_char` | Label every visible occurrence of a char and extend to it |  |
| `find_char_forward_label` | easymotion f: label forward occurrences of a char and jump | normal: `` f `` |
| `find_char_backward_label` | easymotion F: label backward occurrences of a char and jump | normal: `` F `` |
| `till_char_forward_label` | easymotion t: label forward, jump till before a char | normal: `` t `` |
| `till_char_backward_label` | easymotion T: label backward, jump till after a char | normal: `` T `` |
| `goto_next_tabstop` | Goto next snippet placeholder |  |
| `goto_prev_tabstop` | Goto next snippet placeholder |  |
| `emmet_expand` | Expand emmet/zen HTML abbreviation (or Tab) |  |
| `snippet_expand` | Expand the user snippet whose trigger precedes the cursor |  |
| `rotate_selections_first` | Make the first selection your primary one |  |
| `rotate_selections_last` | Make the last selection your primary one |  |
