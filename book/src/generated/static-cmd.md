| Name | Description | Keybinds |
| --- | --- | --- |
| `no_op` | Do nothing |  |
| `move_char_left` | Move left | **spacemacs** — normal: `` h ``, insert: `` <C-b> ``, `` <left> ``<br>**vim** — normal: `` h ``, `` <C-h> ``, insert: `` <left> ``<br>**helix** — normal: `` h ``, `` <left> ``, insert: `` <left> ``<br>**emacs, cua** — normal: `` <C-b> ``, `` <left> ``, insert: `` <C-b> ``, `` <left> `` |
| `move_char_right` | Move right | **spacemacs** — normal: `` l ``, insert: `` <C-f> ``, `` <right> ``<br>**vim** — normal: `` l ``, `` <space> ``, insert: `` <right> ``<br>**helix** — normal: `` l ``, `` <right> ``, insert: `` <right> ``<br>**emacs, cua** — normal: `` <C-f> ``, `` <right> ``, insert: `` <C-f> ``, `` <right> `` |
| `move_line_up` | Move up | **spacemacs, vim** — normal: `` k ``, `` <C-p> ``, insert: `` <up> ``<br>**helix** — normal: `` gk `` |
| `move_line_down` | Move down | **spacemacs, vim** — normal: `` j ``, `` <C-j> ``, `` <C-n> ``, insert: `` <down> ``<br>**helix** — normal: `` gj `` |
| `shift_line_up` | Move the line/selection up (JetBrains Move Statement Up) | **spacemacs, vim** — normal: `` <A-up> `` |
| `shift_line_down` | Move the line/selection down (JetBrains Move Statement Down) | **spacemacs, vim** — normal: `` <A-down> `` |
| `drag_line_down` | Drag the current line down (SPC x . j) | **spacemacs** — normal: `` <space>x.J ``, `` <space>x.j ``, `` <space>x.<down> ``, select: `` <space>x.J ``, `` <space>x.j ``, `` <space>x.<down> `` |
| `drag_line_up` | Drag the current line up (SPC x . k) | **spacemacs** — normal: `` <space>x.K ``, `` <space>x.k ``, `` <space>x.<up> ``, select: `` <space>x.K ``, `` <space>x.k ``, `` <space>x.<up> `` |
| `toggle_test_file` | Toggle between implementation and test file (SPC p a) | **spacemacs** — normal: `` <space>pa ``, select: `` <space>pa `` |
| `fold_comments` | Fold multi-line comment blocks (SPC c h) | **spacemacs** — normal: `` <space>ch ``, select: `` <space>ch `` |
| `move_visual_line_up` | Move up | **spacemacs, vim** — normal: `` gk ``, `` g<up> ``, insert: `` <C-g>k ``, `` <C-g><up> ``, `` <C-g><C-k> ``<br>**helix** — normal: `` k ``, `` <up> ``, insert: `` <up> ``<br>**emacs, cua** — normal: `` <up> ``, `` <C-p> ``, insert: `` <up> ``, `` <C-p> `` |
| `move_visual_line_down` | Move down | **spacemacs, vim** — normal: `` gj ``, `` g<down> ``, insert: `` <C-g>j ``, `` <C-g><C-j> ``, `` <C-g><down> ``<br>**helix** — normal: `` j ``, `` <down> ``, insert: `` <down> ``<br>**emacs, cua** — normal: `` <C-n> ``, `` <down> ``, insert: `` <C-n> ``, `` <down> `` |
| `extend_char_left` | Extend left | **helix** — select: `` h ``, `` <left> ``<br>**emacs** — select: `` <C-b> ``, `` <left> ``<br>**cua** — select: `` <C-b> ``, `` <left> ``, `` <S-left> `` |
| `extend_char_right` | Extend right | **vim** — select: `` <space> ``<br>**helix** — select: `` l ``, `` <right> ``<br>**emacs** — select: `` <C-f> ``, `` <right> ``<br>**cua** — select: `` <C-f> ``, `` <right> ``, `` <S-right> `` |
| `extend_line_up` | Extend up | **helix** — select: `` gk `` |
| `extend_line_down` | Extend down | **helix** — select: `` gj `` |
| `extend_visual_line_up` | Extend up | **helix** — select: `` k ``, `` <up> ``<br>**emacs** — select: `` <up> ``, `` <C-p> ``<br>**cua** — select: `` <up> ``, `` <C-p> ``, `` <S-up> `` |
| `extend_visual_line_down` | Extend down | **helix** — select: `` j ``, `` <down> ``<br>**emacs** — select: `` <C-n> ``, `` <down> ``<br>**cua** — select: `` <C-n> ``, `` <down> ``, `` <S-down> `` |
| `copy_selection_on_next_line` | Copy selection on next line | **helix** — normal: `` C ``, select: `` C `` |
| `copy_selection_on_prev_line` | Copy selection on previous line | **helix** — normal: `` <A-C> ``, select: `` <A-C> `` |
| `column_selection` | Turn the selection into a rectangular column block (IntelliJ column selection) |  |
| `visual_block_mode` | Enter/leave vim visual-block selection (CTRL-V) | **spacemacs, vim** — normal: `` <C-v> ``, `` g<C-h> ``, select: `` <C-v> `` |
| `block_reproject` | Rebuild the visual-block rectangle from its anchor (internal motion helper) |  |
| `visual_line_mode` | Enter/leave vim visual-line selection (V) | **spacemacs, vim** — normal: `` V ``, select: `` V `` |
| `line_reproject` | Rebuild the visual-line whole-line span from its anchor (internal motion helper) |  |
| `block_dollar` | Visual-block: extend each row to its own line end (CTRL-V $) | **spacemacs, vim** — select: `` $ `` |
| `block_swap_corners` | Visual-block: move cursor to the opposite corner (o) | **spacemacs, vim** — select: `` o `` |
| `block_swap_columns` | Visual-block: move cursor to the other column edge (O) | **spacemacs, vim** — select: `` O `` |
| `block_insert` | Visual-block: insert at the left column on every row (I) | **spacemacs, vim** — select: `` I `` |
| `block_append` | Visual-block: append at the right column, padding short rows (A) | **spacemacs, vim** — select: `` A `` |
| `move_next_word_start` | Move to start of next word | **spacemacs** — normal: `` <A-f> ``, `` <A-right> ``, insert: `` <A-f> ``<br>**helix** — normal: `` w `` |
| `move_prev_word_start` | Move to start of previous word | **spacemacs** — normal: `` <A-b> ``, `` <A-left> ``, `` <C-left> ``, insert: `` <A-b> ``, `` <C-left> ``, `` <S-left> ``<br>**vim** — normal: `` <C-left> ``, insert: `` <C-left> ``, `` <S-left> ``<br>**helix** — normal: `` b ``<br>**emacs, cua** — normal: `` <A-b> ``, insert: `` <A-b> `` |
| `move_next_word_end` | Move to end of next word | **helix** — normal: `` e ``<br>**emacs, cua** — normal: `` <A-f> ``, insert: `` <A-f> `` |
| `move_prev_word_end` | Move to end of previous word |  |
| `move_next_long_word_start` | Move to start of next long word | **helix** — normal: `` W `` |
| `move_prev_long_word_start` | Move to start of previous long word | **helix** — normal: `` B `` |
| `move_next_long_word_end` | Move to end of next long word | **helix** — normal: `` E `` |
| `move_prev_long_word_end` | Move to end of previous long word |  |
| `move_next_sub_word_start` | Move to start of next sub word |  |
| `move_prev_sub_word_start` | Move to start of previous sub word |  |
| `move_next_sub_word_end` | Move to end of next sub word |  |
| `move_prev_sub_word_end` | Move to end of previous sub word |  |
| `vim_move_next_word_start` | Move to start of next word (vim caret) |  |
| `vim_move_prev_word_start` | Move to start of previous word (vim caret) |  |
| `vim_move_next_word_end` | Move to end of next word (vim caret) |  |
| `vim_move_prev_word_end` | Move to end of previous word (vim caret) | **spacemacs, vim** — normal: `` ge `` |
| `vim_move_next_long_word_start` | Move to start of next long word (vim caret) | **spacemacs, vim** — normal: `` W `` |
| `vim_move_prev_long_word_start` | Move to start of previous long word (vim caret) | **spacemacs, vim** — normal: `` B `` |
| `vim_move_next_long_word_end` | Move to end of next long word (vim caret) | **spacemacs, vim** — normal: `` E `` |
| `vim_move_prev_long_word_end` | Move to end of previous long word (vim caret) | **spacemacs, vim** — normal: `` gE `` |
| `move_parent_node_end` | Move to end of the parent node | **spacemacs** — normal: `` <space>k$ ``, select: `` <space>k$ ``<br>**helix** — normal: `` <A-e> `` |
| `move_parent_node_start` | Move to beginning of the parent node | **spacemacs** — normal: `` <space>k0 ``, select: `` <space>k0 ``<br>**helix** — normal: `` <A-b> `` |
| `extend_next_word_start` | Extend to start of next word | **helix** — select: `` w `` |
| `extend_prev_word_start` | Extend to start of previous word | **helix** — select: `` b ``<br>**emacs, cua** — select: `` <A-b> `` |
| `extend_next_word_end` | Extend to end of next word | **helix** — select: `` e ``<br>**emacs, cua** — select: `` <A-f> `` |
| `extend_prev_word_end` | Extend to end of previous word |  |
| `extend_next_long_word_start` | Extend to start of next long word | **helix** — select: `` W `` |
| `extend_prev_long_word_start` | Extend to start of previous long word | **helix** — select: `` B `` |
| `extend_next_long_word_end` | Extend to end of next long word | **helix** — select: `` E `` |
| `extend_prev_long_word_end` | Extend to end of prev long word |  |
| `extend_next_sub_word_start` | Extend to start of next sub word |  |
| `extend_prev_sub_word_start` | Extend to start of previous sub word |  |
| `extend_next_sub_word_end` | Extend to end of next sub word |  |
| `extend_prev_sub_word_end` | Extend to end of prev sub word |  |
| `extend_parent_node_end` | Extend to end of the parent node | **helix** — select: `` <A-e> `` |
| `extend_parent_node_start` | Extend to beginning of the parent node | **helix** — select: `` <A-b> `` |
| `find_till_char` | Move till next occurrence of char | **helix** — normal: `` t `` |
| `find_next_char` | Move to next occurrence of char | **helix** — normal: `` f `` |
| `extend_till_char` | Extend till next occurrence of char | **helix** — select: `` t `` |
| `extend_next_char` | Extend to next occurrence of char | **helix** — select: `` f `` |
| `till_prev_char` | Move till previous occurrence of char | **helix** — normal: `` T `` |
| `find_prev_char` | Move to previous occurrence of char | **helix** — normal: `` F `` |
| `vim_find_next_char` | Move to next occurrence of char on this line (vim f) | **spacemacs, vim** — normal: `` f `` |
| `vim_find_prev_char` | Move to previous occurrence of char on this line (vim F) | **spacemacs, vim** — normal: `` F `` |
| `vim_find_till_char` | Move till next occurrence of char on this line (vim t) | **spacemacs, vim** — normal: `` t `` |
| `vim_till_prev_char` | Move till previous occurrence of char on this line (vim T) | **spacemacs, vim** — normal: `` T `` |
| `sneak_forward` | Sneak: jump forward to a two-character sequence |  |
| `sneak_backward` | Sneak: jump backward to a two-character sequence |  |
| `sneak_or_substitute_char` | Sneak forward, or substitute char when vim-sneak is off | **spacemacs, vim** — normal: `` s `` |
| `sneak_or_substitute_line` | Sneak backward, or substitute line when vim-sneak is off | **spacemacs, vim** — normal: `` S `` |
| `vim_change_line` | Change count lines, keeping the first line's indent (vim cc) | **spacemacs, vim** — normal: `` cc `` |
| `extend_till_prev_char` | Extend till previous occurrence of char | **helix** — select: `` T `` |
| `extend_prev_char` | Extend to previous occurrence of char | **helix** — select: `` F `` |
| `repeat_last_motion` | Repeat last motion | **spacemacs** — normal: `` <C-x>z ``, select: `` <C-x>z ``, insert: `` <C-x>z ``<br>**helix** — normal: `` <A-.> ``, select: `` <A-.> `` |
| `repeat_find_char` | Repeat last find in same direction (;) | **spacemacs, vim** — normal: `` ; ``, select: `` ; `` |
| `repeat_find_char_reverse` | Repeat last find in opposite direction (,) | **spacemacs, vim** — normal: `` , ``, select: `` , `` |
| `replace` | Replace with new char | **spacemacs, vim** — select: `` r ``<br>**helix** — normal: `` r ``, select: `` r `` |
| `switch_case` | Switch (toggle) case | **spacemacs, vim** — select: `` ~ ``<br>**helix** — normal: `` ~ ``, select: `` ~ `` |
| `switch_case_forward` | Toggle case and advance cursor (vim ~) | **spacemacs, vim** — normal: `` ~ `` |
| `switch_to_uppercase` | Switch to uppercase | **spacemacs** — normal: `` <space>xU ``, `` <C-x><C-u> ``, select: `` <space>xU ``, `` <C-x><C-u> ``, insert: `` <C-x><C-u> ``<br>**helix** — normal: `` <A-`> ``, select: `` <A-`> ``<br>**emacs** — insert: `` <C-x><C-u> ``<br>**cua** — select: `` <C-X><C-u> ``, insert: `` <C-x><C-u> `` |
| `switch_to_lowercase` | Switch to lowercase | **spacemacs** — normal: `` <space>xu ``, `` <C-x><C-l> ``, select: `` <space>xu ``, `` <C-x><C-l> ``, insert: `` <C-x><C-l> ``<br>**helix** — normal: `` ` ``, select: `` ` ``<br>**emacs** — insert: `` <C-x><C-l> ``<br>**cua** — select: `` <C-X><C-l> ``, insert: `` <C-x><C-l> `` |
| `upcase_word` | Upper-case the word after point (emacs upcase-word, M-u) | **spacemacs** — normal: `` <A-u> ``<br>**emacs, cua** — normal: `` <A-u> ``, insert: `` <A-u> `` |
| `downcase_word` | Lower-case the word after point (emacs downcase-word, M-l) | **spacemacs** — normal: `` <A-l> ``<br>**emacs, cua** — normal: `` <A-l> ``, insert: `` <A-l> `` |
| `capitalize_word` | Capitalize the word after point (emacs capitalize-word, M-c) | **spacemacs** — normal: `` <A-c> ``<br>**emacs, cua** — normal: `` <A-c> ``, insert: `` <A-c> `` |
| `upcase_prev_word` | Upper-case the previous word (emacs M-- M-u) | **spacemacs, vim** — normal: `` <A-minus>u ``, `` <A-minus><A-u> `` |
| `downcase_prev_word` | Lower-case the previous word (emacs M-- M-l) | **spacemacs, vim** — normal: `` <A-minus>l ``, `` <A-minus><A-l> `` |
| `capitalize_prev_word` | Capitalize the previous word (emacs M-- M-c) | **spacemacs, vim** — normal: `` <A-minus>c ``, `` <A-minus><A-c> `` |
| `capitalize_region` | Title-case every word in the region (emacs capitalize-region) |  |
| `upcase_initials_region` | Upper-case the first letter of each word in the region (emacs upcase-initials-region) |  |
| `page_up` | Move page up | **spacemacs** — normal: `` <A-v> ``, `` <C-b> ``, `` <S-minus> ``, insert: `` <A-v> ``, `` <S-up> ``, `` <pageup> ``<br>**vim** — normal: `` <C-b> ``, `` <S-minus> ``, insert: `` <S-up> ``, `` <pageup> ``<br>**helix** — normal: `` <C-b> ``, `` Z<C-b> ``, `` z<C-b> ``, `` <pageup> ``, `` Z<pageup> ``, `` z<pageup> ``, select: `` <C-b> ``, `` Z<C-b> ``, `` z<C-b> ``, `` <pageup> ``, `` Z<pageup> ``, `` z<pageup> ``, insert: `` <pageup> ``<br>**emacs, cua** — normal: `` <A-v> ``, `` <pageup> ``, insert: `` <A-v> ``, `` <pageup> `` |
| `page_down` | Move page down | **spacemacs, vim** — normal: `` <C-f> ``, `` <S-+> ``, `` <S-ret> ``, insert: `` <S-down> ``, `` <pagedown> ``<br>**helix** — normal: `` <C-f> ``, `` Z<C-f> ``, `` z<C-f> ``, `` <pagedown> ``, `` Z<pagedown> ``, `` z<pagedown> ``, select: `` <C-f> ``, `` Z<C-f> ``, `` z<C-f> ``, `` <pagedown> ``, `` Z<pagedown> ``, `` z<pagedown> ``, insert: `` <pagedown> ``<br>**emacs** — normal: `` <C-v> ``, `` <pagedown> ``, insert: `` <C-v> ``, `` <pagedown> ``<br>**cua** — normal: `` <pagedown> ``, insert: `` <pagedown> `` |
| `half_page_up` | Move half page up |  |
| `half_page_down` | Move half page down |  |
| `page_cursor_up` | Move page and cursor up |  |
| `page_cursor_down` | Move page and cursor down |  |
| `page_cursor_half_up` | Move page and cursor half up | **spacemacs, vim** — normal: `` <C-u> ``<br>**helix** — normal: `` <C-u> ``, `` Z<C-u> ``, `` z<C-u> ``, `` Z<backspace> ``, `` z<backspace> ``, select: `` <C-u> ``, `` Z<C-u> ``, `` z<C-u> ``, `` Z<backspace> ``, `` z<backspace> `` |
| `page_cursor_half_down` | Move page and cursor half down | **spacemacs, vim** — normal: `` <C-d> ``<br>**helix** — normal: `` <C-d> ``, `` Z<C-d> ``, `` z<C-d> ``, `` Z<space> ``, `` z<space> ``, select: `` <C-d> ``, `` Z<C-d> ``, `` z<C-d> ``, `` Z<space> ``, `` z<space> `` |
| `extend_page_up` | Extend the selection a page up (vim <S-PageUp>) | **cua** — select: `` <S-pageup> `` |
| `extend_page_down` | Extend the selection a page down (vim <S-PageDown>) | **cua** — select: `` <S-pagedown> `` |
| `select_all` | Select whole document | **spacemacs** — normal: `` <C-x>h ``, select: `` <C-x>h ``, insert: `` <C-x>h ``<br>**helix** — normal: `` % ``, select: `` % ``<br>**emacs** — normal: `` <C-x>h ``, insert: `` <C-x>h ``<br>**cua** — normal: `` <C-x>h ``, select: `` <C-X>h ``, insert: `` <C-x>h `` |
| `select_regex` | Select all regex matches inside selections | **helix** — normal: `` s ``, select: `` s `` |
| `select_all_instances` | Select all occurrences of the current selection in the buffer |  |
| `split_selection` | Split selections on regex matches | **helix** — normal: `` S ``, select: `` S `` |
| `split_selection_on_newline` | Split selection on newlines | **helix** — normal: `` <A-s> ``, select: `` <A-s> `` |
| `merge_selections` | Merge selections | **helix** — normal: `` <A-minus> ``, select: `` <A-minus> `` |
| `merge_consecutive_selections` | Merge consecutive selections | **helix** — normal: `` <A-_> ``, select: `` <A-_> `` |
| `search` | Search for regex pattern | **spacemacs** — normal: `` / ``, `` <C-s> ``, `` <A-C-s> ``, select: `` / ``<br>**vim** — normal: `` / ``, select: `` / ``<br>**helix** — normal: `` / ``, `` Z/ ``, `` z/ ``, select: `` / ``, `` Z/ ``, `` z/ ``<br>**emacs, cua** — normal: `` <C-s> ``, `` <A-C-s> ``, insert: `` <C-s> ``, `` <A-C-s> `` |
| `rsearch` | Reverse search for regex pattern | **spacemacs** — normal: `` ? ``, `` <A-C-r> ``, select: `` ? ``<br>**vim** — normal: `` ? ``, select: `` ? ``<br>**helix** — normal: `` ? ``, `` Z? ``, `` z? ``, select: `` ? ``, `` Z? ``, `` z? ``<br>**emacs, cua** — normal: `` <C-r> ``, `` <A-C-r> ``, insert: `` <C-r> ``, `` <A-C-r> `` |
| `delete_to_search_forward` | Delete up to the next match (vim d/pat) | **spacemacs, vim** — normal: `` d/ `` |
| `delete_to_search_backward` | Delete back to the previous match (vim d?pat) | **spacemacs, vim** — normal: `` d? `` |
| `change_to_search_forward` | Change up to the next match (vim c/pat) | **spacemacs, vim** — normal: `` c/ `` |
| `change_to_search_backward` | Change back to the previous match (vim c?pat) | **spacemacs, vim** — normal: `` c? `` |
| `yank_to_search_forward` | Yank up to the next match (vim y/pat) | **spacemacs, vim** — normal: `` y/ `` |
| `yank_to_search_backward` | Yank back to the previous match (vim y?pat) | **spacemacs, vim** — normal: `` y? `` |
| `search_next` | Select next search match | **spacemacs** — normal: `` <space>sH ``, select: `` <space>sH ``<br>**helix** — normal: `` n ``, `` Zn ``, `` zn ``, select: `` Zn ``, `` zn `` |
| `search_prev` | Select previous search match | **helix** — normal: `` N ``, `` ZN ``, `` zN ``, select: `` ZN ``, `` zN `` |
| `extend_search_next` | Add next search match to selection | **helix** — select: `` n `` |
| `extend_search_prev` | Add previous search match to selection | **helix** — select: `` N `` |
| `select_gn_match` | vim gn: select the search match at/after the cursor (for cgn/dgn) |  |
| `select_gn_match_prev` | vim gN: select the search match at/before the cursor |  |
| `search_next_vim` | vim n: repeat last search in its direction | **spacemacs, vim** — normal: `` n `` |
| `search_prev_vim` | vim N: repeat last search in the opposite direction | **spacemacs, vim** — normal: `` N `` |
| `extend_search_next_vim` | vim n (visual): extend to the repeated match | **spacemacs, vim** — select: `` n `` |
| `extend_search_prev_vim` | vim N (visual): extend to the reverse match | **spacemacs, vim** — select: `` N `` |
| `add_selection_to_next_match` | Add the next occurrence of the selection as a new cursor |  |
| `select_all_occurrences` | Select every occurrence of the selection as a cursor (JetBrains Select All Occurrences) | **spacemacs** — normal: `` <space>xo ``, select: `` <space>xo `` |
| `search_selection` | Use current selection as search pattern | **helix** — normal: `` <A-*> ``, select: `` <A-*> `` |
| `search_selection_detect_word_boundaries` | Use current selection as the search pattern, automatically wrapping with `\b` on word boundaries | **helix** — normal: `` * ``, select: `` * `` |
| `make_search_word_bounded` | Modify current search to make it word bounded |  |
| `global_search` | Global search in workspace folder | **spacemacs** — normal: `` <space>/ ``, `` <space>po ``, `` <space>ps ``, `` <space>sP ``, `` <space>sb ``, `` <space>sd ``, `` <space>sf ``, `` <space>sp ``, `` <space>ss ``, `` <space>saa ``, `` <space>sab ``, `` <space>sad ``, `` <space>saf ``, `` <space>sap ``, `` <space>sgb ``, `` <space>sgd ``, `` <space>sgf ``, `` <space>sgg ``, `` <space>sgp ``, `` <space>skb ``, `` <space>skd ``, `` <space>skf ``, `` <space>skp ``, `` <space>srb ``, `` <space>srd ``, `` <space>srf ``, `` <space>srp ``, `` <space>srr ``, select: `` <space>/ ``, `` <space>po ``, `` <space>ps ``, `` <space>sP ``, `` <space>sb ``, `` <space>sd ``, `` <space>sf ``, `` <space>sp ``, `` <space>ss ``, `` <space>saa ``, `` <space>sab ``, `` <space>sad ``, `` <space>saf ``, `` <space>sap ``, `` <space>sgb ``, `` <space>sgd ``, `` <space>sgf ``, `` <space>sgg ``, `` <space>sgp ``, `` <space>skb ``, `` <space>skd ``, `` <space>skf ``, `` <space>skp ``, `` <space>srb ``, `` <space>srd ``, `` <space>srf ``, `` <space>srp ``, `` <space>srr ``<br>**helix** — normal: `` <space>/ ``, select: `` <space>/ `` |
| `dumb_jump_go` | Jump to the definition of the symbol at point by grepping for it (dumb-jump-go) |  |
| `global_search_symbol` | Global search seeded with the symbol under the cursor | **spacemacs** — normal: `` <space>* ``, `` <space>sB ``, `` <space>sD ``, `` <space>sF ``, `` <space>saA ``, `` <space>saB ``, `` <space>saD ``, `` <space>saF ``, `` <space>saP ``, `` <space>sgB ``, `` <space>sgF ``, `` <space>sgG ``, `` <space>skB ``, `` <space>skD ``, `` <space>skF ``, `` <space>skP ``, `` <space>srB ``, `` <space>srD ``, `` <space>srF ``, `` <space>srP ``, `` <space>srR ``, select: `` <space>* ``, `` <space>sB ``, `` <space>sD ``, `` <space>sF ``, `` <space>saA ``, `` <space>saB ``, `` <space>saD ``, `` <space>saF ``, `` <space>saP ``, `` <space>sgB ``, `` <space>sgF ``, `` <space>sgG ``, `` <space>skB ``, `` <space>skD ``, `` <space>skF ``, `` <space>skP ``, `` <space>srB ``, `` <space>srD ``, `` <space>srF ``, `` <space>srP ``, `` <space>srR `` |
| `clear_search_highlight` | Clear persistent search highlight (SPC s c) | **spacemacs** — normal: `` <space>sc ``, select: `` <space>sc `` |
| `regex_convert_form` | Convert the selected regex between PCRE and Emacs forms (SPC x r c) | **spacemacs** — normal: `` <space>xrc ``, `` <space>xrep ``, `` <space>xrpe ``, select: `` <space>xrc ``, `` <space>xrep ``, `` <space>xrpe `` |
| `regex_emacs_to_rx_replace` | Convert the selected Emacs regex to rx form (SPC x r e x) | **spacemacs** — normal: `` <space>xret ``, `` <space>xrex ``, select: `` <space>xret ``, `` <space>xrex `` |
| `regex_emacs_to_rx_explain` | Explain the selected Emacs regex as rx (SPC x r e /) | **spacemacs** — normal: `` <space>xre/ ``, select: `` <space>xre/ `` |
| `regex_pcre_to_rx_replace` | Convert the selected PCRE regex to rx form (SPC x r x) | **spacemacs** — normal: `` <space>xrt ``, `` <space>xrx ``, `` <space>xrpx ``, select: `` <space>xrt ``, `` <space>xrx ``, `` <space>xrpx `` |
| `regex_pcre_to_rx_explain` | Explain the selected PCRE regex as rx (SPC x r /) | **spacemacs** — normal: `` <space>xr/ ``, `` <space>xrp/ ``, select: `` <space>xr/ ``, `` <space>xrp/ `` |
| `justify_left` | Left-justify (fill) the region (SPC x j l) | **spacemacs** — normal: `` <space>xjl ``, select: `` <space>xjl `` |
| `justify_right` | Right-justify the region (SPC x j r) | **spacemacs** — normal: `` <space>xjr ``, select: `` <space>xjr `` |
| `justify_center` | Center-justify the region (SPC x j c) | **spacemacs** — normal: `` <space>xjc ``, select: `` <space>xjc `` |
| `justify_full` | Full-justify the region (SPC x j f) | **spacemacs** — normal: `` <space>xjf ``, select: `` <space>xjf `` |
| `justify_none` | Remove justification / left-fill (SPC x j n) | **spacemacs** — normal: `` <space>xjn ``, select: `` <space>xjn `` |
| `count_words_region` | Count occurrences per word in the selection (SPC x w c) | **spacemacs** — normal: `` <space>xwc ``, select: `` <space>xwc `` |
| `goto_next_close_paren` | Go forward to next closing paren (SPC k j) | **spacemacs** — normal: `` <space>kj ``, select: `` <space>kj `` |
| `goto_prev_open_paren` | Go backward to previous opening paren (SPC k k) | **spacemacs** — normal: `` <space>kk ``, select: `` <space>kk `` |
| `ediff_windows` | Diff the two front windows side by side (SPC D w w) | **spacemacs** — normal: `` <space>Dwl ``, `` <space>Dww ``, select: `` <space>Dwl ``, `` <space>Dww `` |
| `ediff_buffer` | Diff the current buffer against a picked buffer (SPC D b b) | **spacemacs** — normal: `` <space>Dbb ``, select: `` <space>Dbb `` |
| `ediff_dotfile_and_template` | Diff the config file against the shipped template (SPC f e D) |  |
| `compare_with_clipboard` | Diff the current buffer against the clipboard (JetBrains Compare with Clipboard) | **spacemacs** — normal: `` <space>Dc ``, select: `` <space>Dc `` |
| `transpose_paragraph` | Swap the current paragraph with the previous one (SPC x t p) | **spacemacs** — normal: `` <space>xtp ``, select: `` <space>xtp `` |
| `transpose_line` | Swap the current line with the previous one (emacs transpose-lines, C-x C-t) | **spacemacs** — normal: `` <C-x><C-t> ``, select: `` <C-x><C-t> ``, insert: `` <C-x><C-t> ``<br>**emacs** — insert: `` <C-x><C-t> ``<br>**cua** — select: `` <C-X><C-t> ``, insert: `` <C-x><C-t> `` |
| `move_element_right` | Swap the syntax node under the cursor with its next sibling (JetBrains Move Element Right) | **spacemacs** — normal: `` <space>x<gt> ``, select: `` <space>x<gt> `` |
| `move_element_left` | Swap the syntax node under the cursor with its previous sibling (JetBrains Move Element Left) | **spacemacs** — normal: `` <space>x<lt> ``, select: `` <space>x<lt> `` |
| `convert_indent_to_spaces` | Convert leading indentation to spaces (JetBrains Convert Indents to Spaces) |  |
| `convert_indent_to_tabs` | Convert leading indentation to tabs (JetBrains Convert Indents to Tabs) |  |
| `transpose_sexp` | Swap the current s-expression with the previous one (SPC x t e) | **spacemacs** — normal: `` <A-C-t> ``, `` <space>xte ``, select: `` <space>xte `` |
| `transpose_sentence` | Swap the current sentence with the previous one (SPC x t s) | **spacemacs** — normal: `` <space>xts ``, select: `` <space>xts `` |
| `make_3_windows` | Lay out three vertical windows (SPC w 3) | **spacemacs** — normal: `` <C-w>3 ``, `` <space>w3 ``, `` <space>u<space>w3 ``, select: `` <space>w3 ``, `` <space>u<space>w3 ``<br>**vim** — normal: `` <C-w>3 `` |
| `make_4_windows` | Lay out a 2x2 window grid (SPC w 4) | **spacemacs** — normal: `` <C-w>4 ``, `` <space>w4 ``, `` <space>u<space>w4 ``, select: `` <space>w4 ``, `` <space>u<space>w4 ``<br>**vim** — normal: `` <C-w>4 `` |
| `narrow_to_function` | Narrow the buffer to the enclosing function (SPC n f) | **spacemacs** — normal: `` <C-x>nd ``, `` <space>nf ``, select: `` <C-x>nd ``, `` <space>nf ``, insert: `` <C-x>nd `` |
| `align_at_equals` | Align region at = (SPC x a =) | **spacemacs** — normal: `` <space>xa= ``, select: `` <space>xa= `` |
| `align_at_comma` | Align region at , (SPC x a ,) | **spacemacs** — normal: `` <space>xa, ``, select: `` <space>xa, `` |
| `align_at_colon` | Align region at : (SPC x a :) | **spacemacs** — normal: `` <space>xa: ``, select: `` <space>xa: `` |
| `align_at_semicolon` | Align region at ; (SPC x a ;) | **spacemacs** — normal: `` <space>xa; ``, select: `` <space>xa; `` |
| `align_at_ampersand` | Align region at & (SPC x a &) | **spacemacs** — normal: `` <space>xa& ``, select: `` <space>xa& `` |
| `align_at_lparen` | Align region at ( (SPC x a () | **spacemacs** — normal: `` <space>xa( ``, select: `` <space>xa( `` |
| `align_at_rparen` | Align region at ) (SPC x a )) | **spacemacs** — normal: `` <space>xa) ``, select: `` <space>xa) `` |
| `align_at_lbracket` | Align region at [ (SPC x a [) | **spacemacs** — normal: `` <space>xa[ ``, select: `` <space>xa[ `` |
| `align_at_rbracket` | Align region at ] (SPC x a ]) | **spacemacs** — normal: `` <space>xa] ``, select: `` <space>xa] `` |
| `align_at_lbrace` | Align region at { (SPC x a {) | **spacemacs** — normal: `` <space>xa{ ``, select: `` <space>xa{ `` |
| `align_at_rbrace` | Align region at } (SPC x a }) | **spacemacs** — normal: `` <space>xa} ``, select: `` <space>xa} `` |
| `align_at_dot` | Align region at . (SPC x a .) | **spacemacs** — normal: `` <space>xa. ``, select: `` <space>xa. `` |
| `align_at_arithmetic` | Align region at arithmetic operators (SPC x a m) | **spacemacs** — normal: `` <space>xam ``, select: `` <space>xam `` |
| `align_at_regex` | Align region at a user-specified regexp (SPC x a r) | **spacemacs** — normal: `` <space>xar ``, select: `` <space>xar `` |
| `align_current` | Auto-align the region into columns, per blank-line section (emacs align-current) |  |
| `align_entire` | Auto-align the whole region into columns as one section (emacs align-entire) |  |
| `align_at_bar` | Align every | into columns (SPC x a |) | **spacemacs** — normal: `` <space>xa\| ``, select: `` <space>xa\| `` |
| `align_left_at_char` | Left-align region at a typed delimiter (SPC x a l) | **spacemacs** — normal: `` <space>xal ``, select: `` <space>xal `` |
| `align_right_at_char` | Right-align region at a typed delimiter (SPC x a L) | **spacemacs** — normal: `` <space>xaL ``, select: `` <space>xaL `` |
| `buffer_to_window_1` | Move current buffer to window 1 (SPC b . 1) | **spacemacs** — normal: `` <space>b.1 ``, select: `` <space>b.1 `` |
| `buffer_to_window_2` | Move current buffer to window 2 (SPC b . 2) | **spacemacs** — normal: `` <space>b.2 ``, select: `` <space>b.2 `` |
| `buffer_to_window_3` | Move current buffer to window 3 (SPC b . 3) | **spacemacs** — normal: `` <space>b.3 ``, select: `` <space>b.3 `` |
| `buffer_to_window_4` | Move current buffer to window 4 (SPC b . 4) | **spacemacs** — normal: `` <space>b.4 ``, select: `` <space>b.4 `` |
| `buffer_to_window_5` | Move current buffer to window 5 (SPC b . 5) | **spacemacs** — normal: `` <space>b.5 ``, select: `` <space>b.5 `` |
| `buffer_to_window_6` | Move current buffer to window 6 (SPC b . 6) | **spacemacs** — normal: `` <space>b.6 ``, select: `` <space>b.6 `` |
| `buffer_to_window_7` | Move current buffer to window 7 (SPC b . 7) | **spacemacs** — normal: `` <space>b.7 ``, select: `` <space>b.7 `` |
| `buffer_to_window_8` | Move current buffer to window 8 (SPC b . 8) | **spacemacs** — normal: `` <space>b.8 ``, select: `` <space>b.8 `` |
| `buffer_to_window_9` | Move current buffer to window 9 (SPC b . 9) | **spacemacs** — normal: `` <space>b.9 ``, select: `` <space>b.9 `` |
| `goto_window_1` | Go to window 1 (SPC 1) | **spacemacs** — normal: `` <C-w>.1 ``, `` <C-w>[1 ``, `` <C-w>{1 ``, `` <space>1 ``, `` <space>w.1 ``, `` <space>w[1 ``, `` <space>w{1 ``, `` <space>b.<C-1> ``, select: `` <space>1 ``, `` <space>w.1 ``, `` <space>w[1 ``, `` <space>w{1 ``, `` <space>b.<C-1> ``<br>**vim** — normal: `` <C-w>.1 ``, `` <C-w>[1 ``, `` <C-w>{1 `` |
| `goto_window_2` | Go to window 2 (SPC 2) | **spacemacs** — normal: `` <C-w>.2 ``, `` <C-w>[2 ``, `` <C-w>{2 ``, `` <space>2 ``, `` <space>w.2 ``, `` <space>w[2 ``, `` <space>w{2 ``, `` <space>b.<C-2> ``, select: `` <space>2 ``, `` <space>w.2 ``, `` <space>w[2 ``, `` <space>w{2 ``, `` <space>b.<C-2> ``<br>**vim** — normal: `` <C-w>.2 ``, `` <C-w>[2 ``, `` <C-w>{2 `` |
| `goto_window_3` | Go to window 3 (SPC 3) | **spacemacs** — normal: `` <C-w>.3 ``, `` <C-w>[3 ``, `` <C-w>{3 ``, `` <space>3 ``, `` <space>w.3 ``, `` <space>w[3 ``, `` <space>w{3 ``, `` <space>b.<C-3> ``, select: `` <space>3 ``, `` <space>w.3 ``, `` <space>w[3 ``, `` <space>w{3 ``, `` <space>b.<C-3> ``<br>**vim** — normal: `` <C-w>.3 ``, `` <C-w>[3 ``, `` <C-w>{3 `` |
| `goto_window_4` | Go to window 4 (SPC 4) | **spacemacs** — normal: `` <C-w>.4 ``, `` <C-w>[4 ``, `` <C-w>{4 ``, `` <space>4 ``, `` <space>w.4 ``, `` <space>w[4 ``, `` <space>w{4 ``, `` <space>b.<C-4> ``, select: `` <space>4 ``, `` <space>w.4 ``, `` <space>w[4 ``, `` <space>w{4 ``, `` <space>b.<C-4> ``<br>**vim** — normal: `` <C-w>.4 ``, `` <C-w>[4 ``, `` <C-w>{4 `` |
| `goto_window_5` | Go to window 5 (SPC 5) | **spacemacs** — normal: `` <C-w>.5 ``, `` <C-w>[5 ``, `` <C-w>{5 ``, `` <space>5 ``, `` <space>w.5 ``, `` <space>w[5 ``, `` <space>w{5 ``, `` <space>b.<C-5> ``, select: `` <space>5 ``, `` <space>w.5 ``, `` <space>w[5 ``, `` <space>w{5 ``, `` <space>b.<C-5> ``<br>**vim** — normal: `` <C-w>.5 ``, `` <C-w>[5 ``, `` <C-w>{5 `` |
| `goto_window_6` | Go to window 6 (SPC 6) | **spacemacs** — normal: `` <C-w>.6 ``, `` <C-w>[6 ``, `` <C-w>{6 ``, `` <space>6 ``, `` <space>w.6 ``, `` <space>w[6 ``, `` <space>w{6 ``, `` <space>b.<C-6> ``, select: `` <space>6 ``, `` <space>w.6 ``, `` <space>w[6 ``, `` <space>w{6 ``, `` <space>b.<C-6> ``<br>**vim** — normal: `` <C-w>.6 ``, `` <C-w>[6 ``, `` <C-w>{6 `` |
| `goto_window_7` | Go to window 7 (SPC 7) | **spacemacs** — normal: `` <C-w>.7 ``, `` <C-w>[7 ``, `` <C-w>{7 ``, `` <space>7 ``, `` <space>w.7 ``, `` <space>w[7 ``, `` <space>w{7 ``, `` <space>b.<C-7> ``, select: `` <space>7 ``, `` <space>w.7 ``, `` <space>w[7 ``, `` <space>w{7 ``, `` <space>b.<C-7> ``<br>**vim** — normal: `` <C-w>.7 ``, `` <C-w>[7 ``, `` <C-w>{7 `` |
| `goto_window_8` | Go to window 8 (SPC 8) | **spacemacs** — normal: `` <C-w>.8 ``, `` <C-w>[8 ``, `` <C-w>{8 ``, `` <space>8 ``, `` <space>w.8 ``, `` <space>w[8 ``, `` <space>w{8 ``, `` <space>b.<C-8> ``, select: `` <space>8 ``, `` <space>w.8 ``, `` <space>w[8 ``, `` <space>w{8 ``, `` <space>b.<C-8> ``<br>**vim** — normal: `` <C-w>.8 ``, `` <C-w>[8 ``, `` <C-w>{8 `` |
| `goto_window_9` | Go to window 9 (SPC 9) | **spacemacs** — normal: `` <C-w>.9 ``, `` <C-w>[9 ``, `` <C-w>{9 ``, `` <space>9 ``, `` <space>w.9 ``, `` <space>w[9 ``, `` <space>w{9 ``, `` <space>b.<C-9> ``, select: `` <space>9 ``, `` <space>w.9 ``, `` <space>w[9 ``, `` <space>w{9 ``, `` <space>b.<C-9> ``<br>**vim** — normal: `` <C-w>.9 ``, `` <C-w>[9 ``, `` <C-w>{9 `` |
| `delete_window_and_buffer` | Close window and kill its buffer (SPC w . x) | **spacemacs** — normal: `` <C-w>.x ``, `` <C-w>[x ``, `` <C-w>{x ``, `` <C-x>40 ``, `` <space>w.x ``, `` <space>w[x ``, `` <space>w{x ``, `` <space>u<space>bD ``, `` <space>u<space>wD ``, select: `` <C-x>40 ``, `` <space>w.x ``, `` <space>w[x ``, `` <space>w{x ``, `` <space>u<space>bD ``, `` <space>u<space>wD ``, insert: `` <C-x>40 ``<br>**vim** — normal: `` <C-w>.x ``, `` <C-w>[x ``, `` <C-w>{x ``<br>**emacs** — insert: `` <C-x>40 ``<br>**cua** — select: `` <C-X>40 ``, insert: `` <C-x>40 `` |
| `eval_elisp_region` | Evaluate the selection as elisp (SPC m e r) | **spacemacs** — normal: `` <space>mer ``, select: `` <space>mer `` |
| `eval_elisp_buffer` | Evaluate the buffer as elisp (SPC m e b) | **spacemacs** — normal: `` <space>meb ``, select: `` <space>meb `` |
| `eval_elisp_line` | Evaluate the current line as elisp (SPC m e e) | **spacemacs** — normal: `` <space>me$ ``, `` <space>mee ``, `` <space>mel ``, select: `` <space>me$ ``, `` <space>mee ``, `` <space>mel `` |
| `eval_elisp_defun` | Evaluate the enclosing form as elisp (SPC m e f) | **spacemacs** — normal: `` <space>mec ``, `` <space>mef ``, select: `` <space>mec ``, `` <space>mef `` |
| `eval_print_last_sexp` | Evaluate the sexp before point and insert its value (emacs eval-print-last-sexp) |  |
| `eval_last_sexp` | Evaluate the sexp before point and echo its value (emacs eval-last-sexp, C-x C-e) | **spacemacs** — normal: `` <C-x><C-e> ``, select: `` <C-x><C-e> ``, insert: `` <C-x><C-e> `` |
| `compare_windows` | Compare this window with the next, moving both points to the first difference (emacs compare-windows) |  |
| `layout_create` | Create a new window-layout from the current windows (SPC l l) | **spacemacs** — normal: `` <space>ll ``, `` <space>lww ``, select: `` <space>ll ``, `` <space>lww `` |
| `layout_next` | Switch to the next layout (SPC l n) | **spacemacs** — normal: `` <space>ln ``, `` <space>lwl ``, `` <space>lwn ``, `` <space>l<C-l> ``, select: `` <space>ln ``, `` <space>lwl ``, `` <space>lwn ``, `` <space>l<C-l> `` |
| `layout_prev` | Switch to the previous layout (SPC l p) | **spacemacs** — normal: `` <space>lN ``, `` <space>lp ``, `` <space>lwN ``, `` <space>lwh ``, `` <space>lwp ``, `` <space>l<C-h> ``, select: `` <space>lN ``, `` <space>lp ``, `` <space>lwN ``, `` <space>lwh ``, `` <space>lwp ``, `` <space>l<C-h> `` |
| `layout_last` | Switch to the last-used layout (SPC l TAB) | **spacemacs** — normal: `` <space>l<tab> ``, `` <space>lw<tab> ``, select: `` <space>l<tab> ``, `` <space>lw<tab> `` |
| `layout_default` | Switch to the default (first) layout (SPC l h) | **spacemacs** — normal: `` <space>lh ``, select: `` <space>lh `` |
| `layout_delete` | Delete the current layout, keeping its buffers (SPC l d) | **spacemacs** — normal: `` <space>lD ``, `` <space>lX ``, `` <space>ld ``, `` <space>lx ``, `` <space>lwd ``, select: `` <space>lD ``, `` <space>lX ``, `` <space>ld ``, `` <space>lx ``, `` <space>lwd `` |
| `layout_save` | Save layouts to disk (SPC l s) | **spacemacs** — normal: `` <space>lS ``, select: `` <space>lS `` |
| `layout_rename` | Rename the current layout (SPC l R) | **spacemacs** — normal: `` <space>lR ``, `` <space>lwR ``, select: `` <space>lR ``, `` <space>lwR `` |
| `layout_load` | Load layouts from disk (SPC l L) | **spacemacs** — normal: `` <space>lL ``, `` <space>lo ``, select: `` <space>lL ``, `` <space>lo `` |
| `layout_goto_1` | Switch to layout 1 (SPC l 1) | **spacemacs** — normal: `` <space>l1 ``, `` <space>lw1 ``, `` <space>l<C-1> ``, `` <space>lw<C-1> ``, select: `` <space>l1 ``, `` <space>lw1 ``, `` <space>l<C-1> ``, `` <space>lw<C-1> `` |
| `layout_goto_2` | Switch to layout 2 (SPC l 2) | **spacemacs** — normal: `` <space>l2 ``, `` <space>lw2 ``, `` <space>l<C-2> ``, `` <space>lw<C-2> ``, select: `` <space>l2 ``, `` <space>lw2 ``, `` <space>l<C-2> ``, `` <space>lw<C-2> `` |
| `layout_goto_3` | Switch to layout 3 (SPC l 3) | **spacemacs** — normal: `` <space>l3 ``, `` <space>lw3 ``, `` <space>l<C-3> ``, `` <space>lw<C-3> ``, select: `` <space>l3 ``, `` <space>lw3 ``, `` <space>l<C-3> ``, `` <space>lw<C-3> `` |
| `layout_goto_4` | Switch to layout 4 (SPC l 4) | **spacemacs** — normal: `` <space>l4 ``, `` <space>lw4 ``, `` <space>l<C-4> ``, `` <space>lw<C-4> ``, select: `` <space>l4 ``, `` <space>lw4 ``, `` <space>l<C-4> ``, `` <space>lw<C-4> `` |
| `layout_goto_5` | Switch to layout 5 (SPC l 5) | **spacemacs** — normal: `` <space>l5 ``, `` <space>lw5 ``, `` <space>l<C-5> ``, `` <space>lw<C-5> ``, select: `` <space>l5 ``, `` <space>lw5 ``, `` <space>l<C-5> ``, `` <space>lw<C-5> `` |
| `layout_goto_6` | Switch to layout 6 (SPC l 6) | **spacemacs** — normal: `` <space>l6 ``, `` <space>lw6 ``, `` <space>l<C-6> ``, `` <space>lw<C-6> ``, select: `` <space>l6 ``, `` <space>lw6 ``, `` <space>l<C-6> ``, `` <space>lw<C-6> `` |
| `layout_goto_7` | Switch to layout 7 (SPC l 7) | **spacemacs** — normal: `` <space>l7 ``, `` <space>lw7 ``, `` <space>l<C-7> ``, `` <space>lw<C-7> ``, select: `` <space>l7 ``, `` <space>lw7 ``, `` <space>l<C-7> ``, `` <space>lw<C-7> `` |
| `layout_goto_8` | Switch to layout 8 (SPC l 8) | **spacemacs** — normal: `` <space>l8 ``, `` <space>lw8 ``, `` <space>l<C-8> ``, `` <space>lw<C-8> ``, select: `` <space>l8 ``, `` <space>lw8 ``, `` <space>l<C-8> ``, `` <space>lw<C-8> `` |
| `layout_goto_9` | Switch to layout 9 (SPC l 9) | **spacemacs** — normal: `` <space>l9 ``, `` <space>lw9 ``, `` <space>l<C-9> ``, `` <space>lw<C-9> ``, select: `` <space>l9 ``, `` <space>lw9 ``, `` <space>l<C-9> ``, `` <space>lw<C-9> `` |
| `toggle_modeline_position` | Toggle cursor position in the mode line (SPC t m p) | **spacemacs** — normal: `` <space>tmp ``, select: `` <space>tmp `` |
| `toggle_modeline_vcs` | Toggle version-control info in the mode line (SPC t m v) | **spacemacs** — normal: `` <space>tmv ``, select: `` <space>tmv `` |
| `toggle_modeline_major_mode` | Toggle the major mode (file type) in the mode line (SPC t m M) |  |
| `toggle_system_monitor` | Toggle the system monitor in the minibuffer (symon-mode, SPC t m s) |  |
| `toggle_centered_cursor` | Keep the cursor vertically centered (SPC t -) | **spacemacs** — normal: `` <space>wcC ``, `` <space>wcc ``, `` <space>t<minus> ``, select: `` <space>wcC ``, `` <space>wcc ``, `` <space>t<minus> `` |
| `toggle_hl_line` | Highlight the current line (emacs hl-line-mode / global-hl-line-mode) |  |
| `toggle_electric_pair` | Auto-insert matching close delimiters (emacs electric-pair-mode) |  |
| `toggle_auto_revert` | Reload buffers when their file changes on disk (emacs auto-revert-mode) |  |
| `auto_revert_tail_mode` | Follow the tail of the file as it grows, like tail -f (emacs auto-revert-tail-mode) |  |
| `set_fill_prefix` | Set the fill-prefix from line start to point (emacs set-fill-prefix) | **spacemacs** — normal: `` <C-x>. ``, select: `` <C-x>. ``, insert: `` <C-x>. `` |
| `set_goal_column` | Make vertical motion stick to the current column (emacs set-goal-column) | **spacemacs** — normal: `` <C-x><C-n> ``, select: `` <C-x><C-n> ``, insert: `` <C-x><C-n> `` |
| `toggle_fill_column` | Toggle a fill-column ruler (SPC t f) | **spacemacs** — normal: `` <space>tf ``, select: `` <space>tf `` |
| `toggle_long_line_marker` | Toggle an 80th-column ruler (SPC t 8) | **spacemacs** — normal: `` <space>t8 ``, `` <space>t<C-8> ``, select: `` <space>t8 ``, `` <space>t<C-8> `` |
| `toggle_soft_wrap` | Toggle soft-wrap of long lines (IntelliJ View > Soft-Wrap) | **spacemacs** — normal: `` <C-x>xt ``, select: `` <C-x>xt ``, insert: `` <C-x>xt `` |
| `toggle_whitespace_render` | Toggle rendering of whitespace characters (IntelliJ View > Show Whitespaces) |  |
| `toggle_line_numbers` | Toggle the line-numbers gutter (IntelliJ View > Show Line Numbers) |  |
| `toggle_indent_guides` | Toggle indentation guides (IntelliJ View > Show Indent Guides) |  |
| `toggle_inlay_hints` | Toggle display of LSP inlay hints (IntelliJ View > Inlay Hints) |  |
| `toggle_auto_highlight` | Toggle automatic symbol-under-cursor highlight (SPC t h a) | **spacemacs** — normal: `` <space>tha ``, select: `` <space>tha `` |
| `toggle_syntax_highlighting` | Toggle syntax highlighting for the current buffer (SPC t h s) | **spacemacs** — normal: `` <space>ths ``, select: `` <space>ths `` |
| `toggle_diagnostics` | Toggle diagnostics display / flycheck (SPC t s) | **spacemacs** — normal: `` <space>ts ``, select: `` <space>ts `` |
| `ediff_file` | Diff a prompted file against the current buffer (SPC D f f) | **spacemacs** — normal: `` <space>Dff ``, select: `` <space>Dff `` |
| `ediff_documentation` | Show the Ediff manual and the ediff sessions this build ships (ediff-documentation, SPC D h) |  |
| `ediff_3_files` | 3-way diff of three prompted files, read-only (SPC D f 3) | **spacemacs** — normal: `` <space>Df3 ``, select: `` <space>Df3 `` |
| `ediff_directories` | Compare two directories: list same-name files that differ or are unique, open ediff on one (emacs ediff-directories, SPC D d d) | **spacemacs** — normal: `` <space>Ddd ``, select: `` <space>Ddd `` |
| `ediff_directories3` | Compare three directories: list same-name files present in all three that differ, open a 3-way ediff (emacs ediff-directories3, SPC D d 3) | **spacemacs** — normal: `` <space>Dd3 ``, select: `` <space>Dd3 `` |
| `ediff_merge_directories_with_ancestor` | Merge the same-name files of two directories against a third holding their ancestors (emacs ediff-merge-directories-with-ancestor, SPC D m d 3) |  |
| `ediff_directory_revisions` | Compare a directory's files against their revisions (emacs ediff-directory-revisions, SPC D d r) |  |
| `ediff_merge_revisions_with_ancestor` | Merge two revisions of this file against their common ancestor (emacs ediff-merge-revisions-with-ancestor, SPC D m r 3) |  |
| `ediff_merge_revisions` | Merge two revisions of this file, no ancestor (emacs ediff-merge-revisions, SPC D m r r) |  |
| `ediff_regions` | Ediff two regions linewise: mark A, then diff B (SPC D r l) | **spacemacs** — normal: `` <space>Drl ``, select: `` <space>Drl `` |
| `ediff_regions_wordwise` | Ediff two regions wordwise: mark A, then diff B by word (SPC D r w) | **spacemacs** — normal: `` <space>Drw ``, select: `` <space>Drw `` |
| `ediff_merge_file` | Merge a picked file into the current buffer (editable, SPC D m f f) | **spacemacs** — normal: `` <space>Dmff ``, select: `` <space>Dmff `` |
| `ediff_3_buffers` | 3-way diff of three open buffers, read-only (SPC D b 3) | **spacemacs** — normal: `` <space>Db3 ``, select: `` <space>Db3 `` |
| `kill_buffers_by_regex` | Kill all buffers whose name matches a regex (SPC b M) | **spacemacs** — normal: `` <space>b<C-D> ``, select: `` <space>b<C-D> `` |
| `narrow_to_page` | Narrow the buffer to the current page (SPC n p) | **spacemacs** — normal: `` <C-x>np ``, `` <space>np ``, select: `` <C-x>np ``, `` <space>np ``, insert: `` <C-x>np `` |
| `copy_file` | Copy the current file to a prompted destination (SPC f c) | **spacemacs** — normal: `` <space>fc ``, select: `` <space>fc `` |
| `find_file_replace_buffer` | Open a file and replace the current buffer with it (SPC f A) | **spacemacs** — normal: `` <space>fA ``, `` <C-x><C-v> ``, select: `` <space>fA ``, `` <C-x><C-v> ``, insert: `` <C-x><C-v> ``<br>**emacs** — insert: `` <C-x><C-v> ``<br>**cua** — select: `` <C-X><C-v> ``, insert: `` <C-x><C-v> `` |
| `open_file_literally` | Open a file with no syntax/language (fundamental mode, SPC f l) | **spacemacs** — normal: `` <space>fl ``, select: `` <space>fl `` |
| `locate_file` | Locate a file via system locate/mdfind and open it (SPC f L) | **spacemacs** — normal: `` <space>fL ``, select: `` <space>fL `` |
| `edit_project_config` | Edit the project-local .zmax/config.toml (SPC p e) | **spacemacs** — normal: `` <space>pe ``, select: `` <space>pe `` |
| `man_page_search` | Search man pages via apropos and view the selected page (SPC h m) | **spacemacs** — normal: `` <C-h>S ``, `` <space>hm ``, select: `` <C-h>S ``, `` <space>hm ``, insert: `` <C-h>S `` |
| `info_search` | Search GNU info manuals (apropos) and view the selected node (SPC h i) | **spacemacs** — normal: `` <C-h>i ``, `` <space>hi ``, select: `` <C-h>i ``, `` <space>hi ``, insert: `` <C-h>i `` |
| `info_goto_emacs_key_command_node` | Go to the Emacs manual node for the command a key runs (emacs Info-goto-emacs-key-command-node, C-h K) |  |
| `info_lookup_file` | Look the file name at point up in the Info file index (emacs info-lookup-file) |  |
| `dash_at_point` | Look the symbol at point up in the Dash/Zeal offline docsets (spacemacs dash layer, SPC d d) |  |
| `dash_at_point_with_docset` | Look the symbol at point up in a chosen Dash/Zeal docset (spacemacs dash layer, SPC d D) |  |
| `speed_reading` | Speed-read the buffer one word at a time, RSVP style (spacemacs speed-reading layer, spray-mode) |  |
| `diagnostics_verify_setup` | Report the buffer's diagnostics/LSP setup (SPC e v) | **spacemacs** — normal: `` <space>ev ``, select: `` <space>ev `` |
| `clear_diagnostics` | Clear all diagnostics for the current buffer (SPC e c) | **spacemacs** — normal: `` <space>ec ``, select: `` <space>ec `` |
| `ai_chat` | Ask the AI provider about the selection/buffer (SPC a i) | **spacemacs** — normal: `` <space>ai ``, select: `` <space>ai `` |
| `ai_chat_panel` | Open the streaming AI chat drawer (SPC a p) | **spacemacs** — normal: `` <space>ap ``, select: `` <space>ap `` |
| `ai_model_picker` | Pick the AI model at runtime (SPC a m) |  |
| `toggle_ai_privacy` | Toggle AI privacy mode (SPC a P) | **spacemacs** — normal: `` <space>aP ``, select: `` <space>aP `` |
| `ai_apply_block` | Apply the last AI code block to the selection (SPC a y) | **spacemacs** — normal: `` <space>ay ``, select: `` <space>ay `` |
| `ai_add_file_context` | Add a file as @context for the next AI chat (SPC a @) | **spacemacs** — normal: `` <space>a@ ``, select: `` <space>a@ `` |
| `ai_codebase_context` | Add codebase-search results as @context (SPC a b) | **spacemacs** — normal: `` <space>ab ``, select: `` <space>ab `` |
| `ai_symbol_context` | Add the symbol-under-cursor's definitions as @context (SPC a s) | **spacemacs** — normal: `` <space>as ``, select: `` <space>as `` |
| `ai_terminal_command` | Generate a shell command from natural language (SPC a k) | **spacemacs** — normal: `` <space>ak ``, select: `` <space>ak `` |
| `ai_inline_edit` | AI inline edit/generate on the selection (SPC a e) | **spacemacs** — normal: `` <space>ae ``, select: `` <space>ae `` |
| `ai_inline_edit_preview` | AI inline edit with a diff preview (SPC a E) | **spacemacs** — normal: `` <space>aE ``, select: `` <space>aE `` |
| `ai_accept_edit` | Accept the pending AI inline-edit preview (SPC a .) | **spacemacs** — normal: `` <space>a. ``, select: `` <space>a. `` |
| `ai_explain` | Explain the selected code with AI (SPC a x) | **spacemacs** — normal: `` <space>ax ``, select: `` <space>ax `` |
| `ai_generate_tests` | Generate tests for the selection with AI (SPC a u) | **spacemacs** — normal: `` <space>au ``, select: `` <space>au `` |
| `ai_commit_message` | Generate a git commit message with AI (SPC a c) |  |
| `ai_agent` | Run the autonomous AI agent on a task (SPC a a) | **spacemacs** — normal: `` <space>aa ``, select: `` <space>aa `` |
| `ai_agent_review` | Toggle agent review (dry-run) mode — propose changes without applying (SPC a R) | **spacemacs** — normal: `` <space>aR ``, select: `` <space>aR `` |
| `ai_complete` | AI code completion at the cursor (SPC a TAB) | **spacemacs** — normal: `` <space>a<tab> ``, select: `` <space>a<tab> `` |
| `ai_docs_context` | @docs keyword-search over the docs directory for AI context (SPC a D) | **spacemacs** — normal: `` <space>aD ``, select: `` <space>aD `` |
| `ai_web_context` | @web live web-search results as AI context (SPC a w) | **spacemacs** — normal: `` <space>aw ``, select: `` <space>aw `` |
| `toggle_ai_autocomplete` | Toggle real-time AI ghost-text autocomplete (SPC a g) | **spacemacs** — normal: `` <space>ag ``, select: `` <space>ag `` |
| `ghost_text_accept` | Accept the AI ghost-text suggestion, else Tab (insert mode) | **spacemacs, vim** — insert: `` <tab> `` |
| `ghost_text_accept_word` | Accept the next word of the AI ghost-text suggestion (partial accept) | **spacemacs, vim** — insert: `` <A-right> `` |
| `ai_revert_agent` | Revert the workspace to the last agent checkpoint (SPC a z) | **spacemacs** — normal: `` <space>az ``, select: `` <space>az `` |
| `ai_fix` | AI-fix the diagnostic(s) on the current line (SPC a F) | **spacemacs** — normal: `` <space>aF ``, select: `` <space>aF `` |
| `describe_diagnostics_checker` | Describe the buffer's checkers/language servers (SPC e h) | **spacemacs** — normal: `` <space>eh ``, select: `` <space>eh `` |
| `describe_text_properties` | Describe the tree-sitter node stack at the cursor (SPC h d t) | **spacemacs** — normal: `` <space>hdt ``, select: `` <space>hdt `` |
| `copy_system_info` | Copy system info (version/OS/arch) to the clipboard (SPC h d s) | **spacemacs** — normal: `` <space>hds ``, select: `` <space>hds `` |
| `copy_last_keys` | Copy the most recently pressed keys to the clipboard (SPC h d l) | **spacemacs** — normal: `` <space>hdl ``, select: `` <space>hdl `` |
| `ace_window` | Jump to a window by its number, prompted (ace-window, SPC w . a) | **spacemacs** — normal: `` <C-w>.a ``, `` <C-w>[a ``, `` <C-w>{a ``, `` <space>w.a ``, `` <space>w[a ``, `` <space>w{a ``, select: `` <space>w.a ``, `` <space>w[a ``, `` <space>w{a ``<br>**vim** — normal: `` <C-w>.a ``, `` <C-w>[a ``, `` <C-w>{a `` |
| `browse_news` | Browse zmax release notes / NEWS (SPC h n) | **spacemacs** — normal: `` <C-h>n ``, `` <space>hn ``, `` <C-h><C-n> ``, select: `` <C-h>n ``, `` <space>hn ``, `` <C-h><C-n> ``, insert: `` <C-h>n ``, `` <C-h><C-n> `` |
| `browse_faq` | Browse the zmax FAQ (SPC h f) | **spacemacs** — normal: `` <space>hf ``, select: `` <space>hf `` |
| `layer_search` | Search zmax capability areas / layers (SPC h l) | **spacemacs** — normal: `` <space>hl ``, select: `` <space>hl `` |
| `show_environment` | Show the editor's environment variables (SPC f e e) | **spacemacs** — normal: `` <space>fee ``, select: `` <space>fee `` |
| `reimport_shell_env` | Re-import the shell environment into the editor (SPC f e C-e) | **spacemacs** — normal: `` <space>feE ``, `` <space>fe<C-e> ``, select: `` <space>feE ``, `` <space>fe<C-e> `` |
| `image_converter_add_handler` | Register an external converter for an image suffix (emacs image-converter-add-handler) |  |
| `image_converter_list_handlers` | List the registered image converters (emacs image-converter) |  |
| `conda_env_list` | List the available conda environments (Spacemacs conda-env-list) |  |
| `conda_env_activate` | Activate a conda environment for spawned processes (Spacemacs conda-env-activate) |  |
| `conda_env_deactivate` | Deactivate the active conda environment (Spacemacs conda-env-deactivate) |  |
| `quickurl` | Insert the URL of the quickurl entry named by the word at point (emacs quickurl) |  |
| `quickurl_ask` | Prompt for a quickurl entry and insert its URL (emacs quickurl-ask) |  |
| `quickurl_add_url` | Add a named URL to the quickurl list (emacs quickurl-add-url) |  |
| `quickurl_list` | List the stored quickurl entries (emacs quickurl-list) |  |
| `quickurl_browse_url` | Browse the quickurl entry named by the word at point (emacs quickurl-browse-url) |  |
| `quickurl_browse_url_ask` | Prompt for a quickurl entry and browse it (emacs quickurl-browse-url-ask) |  |
| `rebox_dwim` | Box the selected lines / rebuild the comment box at point (rebox2 rebox-dwim) |  |
| `rebox_cycle` | Cycle the comment box at point to the next rebox style (rebox2 rebox-cycle) |  |
| `goto_buffer_window` | Focus the window already showing a chosen buffer (SPC b w) | **spacemacs** — normal: `` <space>bW ``, select: `` <space>bW `` |
| `git_file_dispatch` | Magit-style file operations dispatch for the current file (SPC g f m) | **spacemacs** — normal: `` <space>gfm ``, select: `` <space>gfm `` |
| `describe_current_modes` | Describe the current editor/buffer modes (SPC h d m) | **spacemacs** — normal: `` <C-h>m ``, `` <space>hdm ``, select: `` <C-h>m ``, `` <space>hdm ``, insert: `` <C-h>m `` |
| `describe_command` | Describe a command — its doc and key bindings (C-h f) | **spacemacs** — normal: `` <C-h>x ``, select: `` <C-h>x ``, insert: `` <C-h>x `` |
| `where_is` | Show the keys a command is bound to (C-h w) | **spacemacs** — normal: `` <C-h>w ``, select: `` <C-h>w ``, insert: `` <C-h>w `` |
| `describe_key` | Describe a key — pick a binding, show its command and doc (C-h k) | **spacemacs** — normal: `` <C-h>k ``, select: `` <C-h>k ``, insert: `` <C-h>k `` |
| `describe_bindings` | List every key binding of the current mode (C-h b) | **spacemacs** — normal: `` <C-h>b ``, `` <space>tkt ``, select: `` <C-h>b ``, `` <space>tkt ``, insert: `` <C-h>b `` |
| `describe_coding_system` | Describe the buffer's coding system / encoding (C-h C) | **spacemacs** — normal: `` <C-h>C ``, select: `` <C-h>C ``, insert: `` <C-h>C `` |
| `describe_language_environment` | Describe the language environment / locale (C-h L) | **spacemacs** — normal: `` <C-h>L ``, select: `` <C-h>L ``, insert: `` <C-h>L `` |
| `describe_syntax` | Describe the buffer's syntax / tree-sitter status (C-h s) | **spacemacs** — normal: `` <C-h>s ``, select: `` <C-h>s ``, insert: `` <C-h>s `` |
| `view_lossage` | Show the recently pressed keys (C-h l) | **spacemacs** — normal: `` <C-h>l ``, select: `` <C-h>l ``, insert: `` <C-h>l `` |
| `describe_char` | Describe the character after point — code, Unicode block, category (emacs describe-char, C-u C-x =) |  |
| `emoji_describe` | Say what the emoji after point is called (emacs emoji-describe) |  |
| `emoji_list` | Pick an emoji by name and insert it (emacs emoji-list) |  |
| `view_hello_file` | Show a multi-script greeting sample (emacs view-hello-file, C-h h) | **spacemacs** — normal: `` <C-h>h ``, select: `` <C-h>h ``, insert: `` <C-h>h `` |
| `view_echo_area_messages` | Show the last echo-area message (emacs view-echo-area-messages, C-h e) | **spacemacs** — normal: `` g<lt> ``, `` <C-h>e ``, select: `` <C-h>e ``, insert: `` <C-h>e ``<br>**vim** — normal: `` g<lt> `` |
| `describe_copying` | Show zmax's copying license, the GPL (emacs describe-copying, C-h C-c) | **spacemacs** — normal: `` <C-h><C-c> ``, select: `` <C-h><C-c> ``, insert: `` <C-h><C-c> `` |
| `describe_distribution` | How to get zmax / GNU software (emacs describe-distribution, C-h C-d) | **spacemacs** — normal: `` <C-h><C-d> ``, `` <C-h><C-o> ``, select: `` <C-h><C-d> ``, `` <C-h><C-o> ``, insert: `` <C-h><C-d> ``, `` <C-h><C-o> `` |
| `describe_gnu_project` | Open the GNU project page (emacs describe-gnu-project, C-h g) | **spacemacs** — normal: `` <C-h>g ``, select: `` <C-h>g ``, insert: `` <C-h>g `` |
| `describe_no_warranty` | Show the GPL no-warranty sections (emacs describe-no-warranty, C-h C-w) | **spacemacs** — normal: `` <C-h><C-w> ``, select: `` <C-h><C-w> ``, insert: `` <C-h><C-w> `` |
| `view_emacs_faq` | Open the GNU Emacs FAQ (emacs view-emacs-FAQ, C-h C-f) | **spacemacs** — normal: `` <C-h><C-f> ``, select: `` <C-h><C-f> ``, insert: `` <C-h><C-f> `` |
| `view_emacs_todo` | Open the Emacs TODO list (emacs view-emacs-todo) | **spacemacs** — normal: `` <C-h><C-t> ``, select: `` <C-h><C-t> ``, insert: `` <C-h><C-t> `` |
| `view_emacs_problems` | Open the Emacs known-problems file (emacs view-emacs-problems) | **spacemacs** — normal: `` <C-h><C-p> ``, select: `` <C-h><C-p> ``, insert: `` <C-h><C-p> `` |
| `view_emacs_debugging` | Open the Emacs debugging manual (emacs view-emacs-debugging) |  |
| `view_order_manuals` | Open where to get the GNU manuals (emacs view-order-manuals) | **spacemacs** — normal: `` <C-h><C-m> ``, select: `` <C-h><C-m> ``, insert: `` <C-h><C-m> `` |
| `view_external_packages` | Open GNU ELPA / external packages (emacs view-external-packages) | **spacemacs** — normal: `` <C-h><C-e> ``, select: `` <C-h><C-e> ``, insert: `` <C-h><C-e> `` |
| `describe_keymap` | List every binding of the current mode's keymap (emacs describe-keymap) | **spacemacs** — normal: `` <space>hdK ``, `` <space>tkM ``, `` <space>tkm ``, select: `` <space>hdK ``, `` <space>tkM ``, `` <space>tkm `` |
| `describe_prefix_bindings` | List the sub-bindings of a prefix (emacs describe-prefix-bindings) |  |
| `describe_categories` | List the character categories zmax recognises (emacs describe-categories) |  |
| `list_character_sets` | List the Unicode blocks zmax knows (emacs list-character-sets) |  |
| `list_charset_chars` | List the printable characters of each Unicode block (emacs list-charset-chars) |  |
| `list_coding_systems` | List the coding systems / encodings zmax supports (emacs list-coding-systems) |  |
| `describe_language_package` | Describe the language-support config for the buffer (SPC h d p) | **spacemacs** — normal: `` <space>hdp ``, select: `` <space>hdp `` |
| `package_search` | Search configured language packages and describe one (SPC h p) | **spacemacs** — normal: `` <C-h>P ``, `` <C-h>p ``, `` <space>hp ``, `` <space>hdP ``, select: `` <C-h>P ``, `` <C-h>p ``, `` <space>hp ``, `` <space>hdP ``, insert: `` <C-h>P ``, `` <C-h>p `` |
| `config_variable_search` | Search editor config variables, copy path on select (SPC h .) | **spacemacs** — normal: `` <space>h. ``, select: `` <space>h. `` |
| `apropos_local_value` | Search buffer-local variables by value (emacs apropos-local-value, SPC h v) | **spacemacs** — normal: `` <space>hv ``, select: `` <space>hv `` |
| `clone_indirect_buffer` | Clone the current buffer into a shared-document split (SPC b N i) | **spacemacs** — normal: `` <C-x>4c ``, `` <space>bNI ``, `` <space>bNi ``, select: `` <C-x>4c ``, `` <space>bNI ``, `` <space>bNi ``, insert: `` <C-x>4c `` |
| `clone_indirect_from_buffer` | Open an existing buffer in a shared-document split (SPC b N C-i) | **spacemacs** — normal: `` <space>bN<C-i> ``, select: `` <space>bN<C-i> `` |
| `open_junk_file` | Open a fresh timestamped junk file (SPC f J) | **spacemacs** — normal: `` <space>fJ ``, select: `` <space>fJ `` |
| `open_hex` | Open the current file in the hex editor (SPC f h, hexl) | **spacemacs** — normal: `` <space>fh ``, select: `` <space>fh `` |
| `open_file_external` | Open the current file with the OS default program (SPC f o) | **spacemacs** — normal: `` <space>fo ``, select: `` <space>fo `` |
| `git_init` | Initialize a new git repository (SPC g i) | **spacemacs** — normal: `` <space>gi ``, select: `` <space>gi `` |
| `view_file_at_rev` | View the current file at a branch/commit (SPC g f f) | **spacemacs** — normal: `` <C-x>v~ ``, `` <space>gff ``, select: `` <C-x>v~ ``, `` <space>gff ``, insert: `` <C-x>v~ `` |
| `extend_line` | Select current line, if already selected, extend to another line based on the anchor | **spacemacs** — normal: `` <space>kV ``, select: `` <space>kV `` |
| `extend_line_below` | Select current line, if already selected, extend to next line | **helix** — normal: `` x ``, select: `` x `` |
| `extend_line_above` | Select current line, if already selected, extend to previous line |  |
| `select_line_above` | Select current line, if already selected, extend or shrink line above based on the anchor |  |
| `select_line_below` | Select current line, if already selected, extend or shrink line below based on the anchor |  |
| `extend_to_line_bounds` | Extend selection to line bounds | **helix** — normal: `` X ``, select: `` X `` |
| `extend_chars_right_vim` | Extend count graphemes right, line-bounded (dl/cl/yl) |  |
| `extend_chars_left_vim` | Extend count graphemes left, line-bounded (dh/ch/yh) |  |
| `extend_line_below_linewise` | Extend whole lines down for a linewise operator (dj/cj/yj) |  |
| `extend_line_above_linewise` | Extend whole lines up for a linewise operator (dk/ck/yk) |  |
| `extend_next_paragraph` | Extend to next paragraph for an operator (d}/c}/y}) |  |
| `extend_prev_paragraph` | Extend to previous paragraph for an operator (d{/c{/y{) |  |
| `select_paragraph_forward_vim` | vim }: paragraph operator target with linewise promotion |  |
| `select_paragraph_backward_vim` | vim {: paragraph operator target with linewise promotion |  |
| `select_paragraph_forward_vim_linewise` | vim V}: force-linewise paragraph operator target |  |
| `select_paragraph_backward_vim_linewise` | vim V{: force-linewise paragraph operator target |  |
| `shrink_to_line_bounds` | Shrink selection to line bounds | **helix** — normal: `` <A-x> ``, select: `` <A-x> `` |
| `delete_selection` | Delete selection | **helix** — normal: `` d ``, select: `` d `` |
| `delete_selection_linewise` | Delete selection (vim linewise, EOF-aware) |  |
| `delete_selection_noyank` | Delete selection without yanking | **helix** — normal: `` <A-d> ``, select: `` <A-d> `` |
| `change_selection` | Change selection | **helix** — normal: `` c ``, select: `` c `` |
| `change_selection_noyank` | Change selection without yanking | **helix** — normal: `` <A-c> ``, select: `` <A-c> `` |
| `collapse_selection` | Collapse selection into single cursor | **helix** — normal: `` ; ``, select: `` ; ``<br>**emacs, cua** — normal: `` <C-g> ``, insert: `` <C-g> `` |
| `flip_selections` | Flip selection cursor and anchor | **spacemacs** — normal: `` <C-x><C-x> ``, select: `` <C-x><C-x> ``, insert: `` <C-x><C-x> ``<br>**helix** — normal: `` <A-;> ``, select: `` <A-;> ``<br>**emacs** — normal: `` <C-x><C-x> ``, insert: `` <C-x><C-x> ``<br>**cua** — normal: `` <C-x><C-x> ``, select: `` <C-X><C-x> ``, insert: `` <C-x><C-x> `` |
| `ensure_selections_forward` | Ensure all selections face forward | **helix** — normal: `` <A-:> ``, select: `` <A-:> `` |
| `insert_mode` | Insert before selection | **spacemacs** — normal: `` i ``, `` <ins> ``, `` <space>ki ``, select: `` <space>ki ``<br>**vim** — normal: `` i ``, `` <ins> ``<br>**helix** — normal: `` i ``, select: `` i ``<br>**emacs, cua** — normal: `` a ``, `` i `` |
| `append_mode` | Append after selection | **spacemacs, vim** — normal: `` a ``<br>**helix** — normal: `` a ``, select: `` a `` |
| `replace_mode` | Enter Replace mode (overtype) | **spacemacs, vim** — normal: `` R `` |
| `virtual_replace_mode` | Enter Virtual Replace mode, overtyping in screen space (vim gR) | **spacemacs, vim** — normal: `` gR `` |
| `command_mode` | Enter command mode | **spacemacs** — normal: `` <space>: ``, `` <space>k: ``, select: `` : ``, `` <space>: ``, `` <space>k: ``<br>**vim** — select: `` : ``<br>**helix** — normal: `` : ``, select: `` : `` |
| `ex_mode` | Enter Ex mode: type : commands one after another until :visual (vim gQ) | **spacemacs, vim** — normal: `` gQ `` |
| `file_picker` | Open file picker | **spacemacs** — normal: `` <space>ff ``, `` <space>pf ``, `` <space>ph ``, `` <space>pp ``, `` <C-x><C-f> ``, `` <C-x><C-r> ``, select: `` <space>ff ``, `` <space>pf ``, `` <space>ph ``, `` <space>pp ``, `` <C-x><C-f> ``, `` <C-x><C-r> ``, insert: `` <C-x><C-f> ``, `` <C-x><C-r> ``<br>**helix** — normal: `` <space>f ``, select: `` <space>f ``<br>**emacs** — normal: `` <C-x><C-f> ``, insert: `` <C-x><C-f> ``<br>**cua** — normal: `` <C-x><C-f> ``, select: `` <C-X><C-f> ``, insert: `` <C-x><C-f> `` |
| `bury_buffer` | Stop showing the current buffer without killing it (emacs bury-buffer) | **spacemacs** — normal: `` <space>b.<C-d> ``, select: `` <space>b.<C-d> `` |
| `toggle_column_indexing` | Toggle 0/1-based column indexing in the statusline (Spacemacs SPC t z) | **spacemacs** — normal: `` <space>tz ``, select: `` <space>tz `` |
| `next_file_in_dir` | Open the next file in the current file's directory (Spacemacs ] f) |  |
| `prev_file_in_dir` | Open the previous file in the current file's directory (Spacemacs [ f) |  |
| `file_picker_in_current_buffer_directory` | Open file picker at current buffer's directory |  |
| `file_picker_in_current_directory` | Open file picker at current working directory | **spacemacs** — normal: `` <C-x><C-d> ``, select: `` <C-x><C-d> ``, insert: `` <C-x><C-d> ``<br>**helix** — normal: `` <space>F ``, select: `` <space>F `` |
| `file_explorer` | Open file explorer in workspace root | **spacemacs** — normal: `` <space>ad ``, `` <space>af ``, `` <space>ft ``, `` <space>pd ``, `` <space>pt ``, `` <space>atrd ``, `` <space>atrr ``, select: `` <space>ad ``, `` <space>af ``, `` <space>ft ``, `` <space>pd ``, `` <space>pt ``, `` <space>atrd ``, `` <space>atrr ``<br>**helix** — normal: `` <space>e ``, select: `` <space>e `` |
| `file_explorer_in_current_buffer_directory` | Open file explorer at current buffer's directory | **spacemacs** — normal: `` <space>fd ``, `` <space>fj ``, `` <space>jD ``, `` <space>jd ``, select: `` <space>fd ``, `` <space>fj ``, `` <space>jD ``, `` <space>jd ``<br>**helix** — normal: `` <space>. ``, select: `` <space>. `` |
| `file_explorer_in_current_directory` | Open file explorer at current working directory |  |
| `buffer_menu` | Open the Buffer Menu (emacs buffer-menu / C-x C-b) | **spacemacs** — normal: `` <space>bM ``, select: `` <space>bM `` |
| `list_buffers` | List open buffers in the Buffer Menu (emacs list-buffers) | **spacemacs** — normal: `` <C-x><C-b> ``, select: `` <C-x><C-b> ``, insert: `` <C-x><C-b> ``<br>**emacs** — normal: `` <C-x><C-b> ``, insert: `` <C-x><C-b> ``<br>**cua** — normal: `` <C-x><C-b> ``, select: `` <C-X><C-b> ``, insert: `` <C-x><C-b> `` |
| `calendar` | Open the Calendar month grid (emacs calendar) |  |
| `diary` | Show today's diary entries (emacs diary) |  |
| `diary_view_entries` | Show diary entries for the current date (emacs diary-view-entries) |  |
| `diary_show_all_entries` | Open the diary file (emacs diary-show-all-entries) |  |
| `diary_insert_entry` | Add a diary entry for today (emacs diary-insert-entry) |  |
| `diary_insert_weekly_entry` | Add a weekly diary entry for today (emacs diary-insert-weekly-entry) |  |
| `diary_mark_entries` | Mark calendar dates that have diary entries (emacs diary-mark-entries) |  |
| `diary_list_entries` | List diary entries for the current date (emacs diary-list-entries) |  |
| `diary_fancy_display` | Show the day's diary entries in fancy format (emacs diary-fancy-display) |  |
| `diary_simple_display` | Show the day's diary entries in simple format (emacs diary-simple-display) |  |
| `diary_sort_entries` | Sort the day's diary entries by time (emacs diary-sort-entries) |  |
| `diary_include_other_diary_files` | Include entries from #include'd diary files (emacs diary-include-other-diary-files) |  |
| `diary_mark_included_diary_files` | Mark dates from #include'd diary files (emacs diary-mark-included-diary-files) |  |
| `icalendar_export_file` | Export the diary file to ~/diary.ics (emacs icalendar-export-file) |  |
| `icalendar_export_region` | Export the selected diary region to ~/diary.ics (emacs icalendar-export-region) |  |
| `icalendar_import_file` | Import ~/diary.ics into the diary file (emacs icalendar-import-file) |  |
| `icalendar_import_buffer` | Import the current buffer's iCalendar into the diary (emacs icalendar-import-buffer) |  |
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
| `diary_hebrew_birthday` | Report any Hebrew birthday falling today (emacs diary-hebrew-birthday) |  |
| `diary_hebrew_yahrzeit` | Report any yahrzeit falling today (emacs diary-hebrew-yahrzeit) |  |
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
| `diary_chinese_date` | Today's Chinese calendar date (emacs diary-chinese-date) |  |
| `diary_chinese_insert_entry` | Add a Chinese-date diary entry for today (emacs diary-chinese-insert-entry) |  |
| `diary_chinese_insert_monthly_entry` | Add a monthly Chinese diary entry (emacs diary-chinese-insert-monthly-entry) |  |
| `diary_chinese_insert_yearly_entry` | Add a yearly Chinese diary entry (emacs diary-chinese-insert-yearly-entry) |  |
| `diary_chinese_insert_anniversary_entry` | Add a Chinese anniversary diary entry (emacs diary-chinese-insert-anniversary-entry) |  |
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
| `calendar_chinese_print_date` | Today's Chinese calendar date (emacs calendar-chinese-print-date) |  |
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
| `calendar_chinese_goto_date` | Echo Gregorian for a Chinese date (emacs calendar-chinese-goto-date) |  |
| `calendar_astro_goto_day_number` | Echo Gregorian for an astro day number (emacs calendar-astro-goto-day-number) |  |
| `calendar_mayan_goto_long_count` | Echo Gregorian for a Mayan long count (emacs calendar-mayan-goto-long-count) |  |
| `calc_dispatch` | Open the RPN Calc stack calculator (emacs calc / C-x *) | **spacemacs** — normal: `` <space>ac ``, select: `` <space>ac `` |
| `occur` | List lines matching a regexp in an *Occur* overlay (emacs occur / M-s o) | **spacemacs** — normal: `` <A-s>o `` |
| `isearch_forward_word` | Incremental whole-word search forward (emacs isearch-forward-word) | **spacemacs** — normal: `` <A-s>w ``, `` <A-s><A-w> `` |
| `isearch_forward_symbol` | Incremental whole-symbol search forward (emacs isearch-forward-symbol) | **spacemacs** — normal: `` <A-s>_ `` |
| `isearch_forward_thing_at_point` | Search for the symbol/word at point (emacs isearch-forward-thing-at-point) |  |
| `isearch_forward_symbol_at_point` | Search for the symbol at point (emacs isearch-forward-symbol-at-point) | **spacemacs** — normal: `` <A-s>. ``, `` <A-s><A-.> `` |
| `isearch_toggle_regexp` | Toggle regexp matching for the current search (emacs isearch-toggle-regexp) |  |
| `isearch_toggle_word` | Toggle whole-word matching for the current search (emacs isearch-toggle-word) |  |
| `isearch_toggle_symbol` | Toggle whole-symbol matching for the current search (emacs isearch-toggle-symbol) |  |
| `isearch_toggle_case_fold` | Toggle case-folding for the current search (emacs isearch-toggle-case-fold) |  |
| `isearch_toggle_lax_whitespace` | Toggle lax-whitespace matching for the current search (emacs isearch-toggle-lax-whitespace) |  |
| `isearch_toggle_char_fold` | Toggle character folding — `resume` matches `résumé` (emacs isearch-toggle-char-fold) |  |
| `isearch_toggle_invisible` | Toggle matching inside folded (invisible) text (emacs isearch-toggle-invisible) |  |
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
| `query_replace` | Replace matches one by one, asking (emacs query-replace M-%) | **emacs, cua** — insert: `` <A-%> `` |
| `query_replace_regexp` | Replace regexp matches one by one, asking (emacs query-replace-regexp C-M-%) | **emacs, cua** — insert: `` <A-C-%> `` |
| `tags_query_replace` | Query-replace a regexp across every file in the tags table (emacs tags-query-replace) |  |
| `isearch_query_replace` | Query-replace the current search pattern (emacs isearch-query-replace) |  |
| `isearch_query_replace_regexp` | Query-replace the current search regexp (emacs isearch-query-replace-regexp) |  |
| `isearch_highlight_regexp` | Highlight matches of the current search (emacs isearch-highlight-regexp) |  |
| `isearch_highlight_lines_matching_regexp` | List/highlight lines matching the search (emacs isearch-highlight-lines-matching-regexp) |  |
| `rmail` | Open the Rmail mail reader on ~/RMAIL (emacs rmail) | **spacemacs** — normal: `` <space>ar ``, select: `` <space>ar `` |
| `rmail_continue` | Rmail: return to the message being composed (emacs rmail-continue) |  |
| `rmail_resend` | Rmail: resend (bounce) the current message (emacs rmail-resend) |  |
| `rmail_retry_failure` | Rmail: re-compose the message a bounce returned (emacs rmail-retry-failure) |  |
| `rmail_mime` | Rmail: toggle the decoded MIME display (emacs rmail-mime) |  |
| `rmail_mime_next_item` | Rmail: move to the next MIME entity (emacs rmail-mime-next-item) |  |
| `rmail_mime_previous_item` | Rmail: move to the previous MIME entity (emacs rmail-mime-previous-item) |  |
| `rmail_mime_toggle_hidden` | Rmail: show/collapse the MIME entity at point (emacs rmail-mime-toggle-hidden) |  |
| `rmail_epa_decrypt` | Rmail: decrypt the message's OpenPGP armor with gpg (emacs rmail-epa-decrypt) |  |
| `rmail_redecode_body` | Rmail: re-decode the message body with another coding system (emacs rmail-redecode-body) |  |
| `undigestify_rmail_message` | Rmail: split the current digest into its messages (emacs undigestify-rmail-message) |  |
| `unforward_rmail_message` | Rmail: extract the message a forward carries (emacs unforward-rmail-message) |  |
| `dired` | Open the Dired directory editor (emacs C-x d) | **spacemacs** — normal: `` <C-x>d ``, `` <space>pD ``, select: `` <C-x>d ``, `` <space>pD ``, insert: `` <C-x>d ``<br>**emacs** — insert: `` <C-x>d ``<br>**cua** — select: `` <C-X>d ``, insert: `` <C-x>d `` |
| `dired_jump` | Open Dired on the current buffer's directory (emacs C-x C-j) | **spacemacs** — normal: `` <C-x><C-j> ``, select: `` <C-x><C-j> ``, insert: `` <C-x><C-j> ``<br>**emacs** — insert: `` <C-x><C-j> ``<br>**cua** — select: `` <C-X><C-j> ``, insert: `` <C-x><C-j> `` |
| `dired_other_window` | Open Dired (overlay; emacs dired-other-window C-x 4 d) | **spacemacs** — normal: `` <C-x>4d ``, select: `` <C-x>4d ``, insert: `` <C-x>4d `` |
| `dired_jump_other_window` | Open Dired on the buffer's dir (overlay; emacs dired-jump-other-window) | **spacemacs** — normal: `` <C-x>4<C-j> ``, select: `` <C-x>4<C-j> ``, insert: `` <C-x>4<C-j> `` |
| `dired_at_point` | Open Dired on the file name at point (emacs dired-at-point) |  |
| `tex_insert_braces` | TeX: insert a {} brace pair (emacs tex-insert-braces) |  |
| `tex_insert_quote` | TeX: insert \`\` or '' smart quotes (emacs tex-insert-quote) |  |
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
| `tex_recenter_output_buffer` | TeX: recenter the TeX output (emacs tex-recenter-output-buffer) |  |
| `tex_kill_job` | TeX: kill the running TeX job (emacs tex-kill-job) |  |
| `sgml_tag` | SGML: wrap region/point in a <tag>..</tag> (emacs sgml-tag) |  |
| `sgml_close_tag` | SGML: close the innermost open element (emacs sgml-close-tag) |  |
| `sgml_delete_tag` | SGML: delete the enclosing tag pair, keeping content (emacs sgml-delete-tag) |  |
| `sgml_skip_tag_forward` | SGML: move past a balanced tag group (emacs sgml-skip-tag-forward) |  |
| `sgml_skip_tag_backward` | SGML: move back over a balanced tag group (emacs sgml-skip-tag-backward) |  |
| `sgml_name_char` | SGML: insert a &entity; for a character (emacs sgml-name-char) |  |
| `sgml_tag_help` | SGML: describe an HTML element (emacs sgml-tag-help) |  |
| `sgml_attributes` | SGML: insert attributes at point (emacs sgml-attributes) |  |
| `sgml_name_8bit_mode` | SGML: toggle 8-bit entity name display (emacs sgml-name-8bit-mode) |  |
| `sgml_validate` | SGML: validate the file with onsgmls/nsgmls (emacs sgml-validate) |  |
| `sgml_mode` | SGML: enter SGML editing mode (emacs sgml-mode) |  |
| `html_mode` | HTML: enter HTML editing mode (emacs html-mode) |  |
| `htmlfontify_buffer` | HTML: export the buffer as highlighted HTML (emacs htmlfontify-buffer) |  |
| `nroff_forward_text_line` | nroff: forward one text line, skip requests (emacs nroff-forward-text-line) |  |
| `nroff_backward_text_line` | nroff: backward one text line, skip requests (emacs nroff-backward-text-line) |  |
| `nroff_count_text_lines` | nroff: count text lines in region (emacs nroff-count-text-lines) |  |
| `code_action` | Perform code action | **spacemacs** — normal: `` <space>la ``, select: `` <space>la ``<br>**helix** — normal: `` <space>a ``, select: `` <space>a `` |
| `extract_refactor` | Extract refactoring (method/variable/constant) via LSP (IntelliJ Extract) |  |
| `extract_function` | Extract Method/Function via LSP (IntelliJ Extract Method) |  |
| `extract_variable` | Introduce Variable via LSP (IntelliJ Introduce Variable) |  |
| `extract_constant` | Extract Constant via LSP (IntelliJ Extract Constant) |  |
| `extract_field` | Introduce Field via LSP (IntelliJ Introduce Field) |  |
| `extract_parameter` | Introduce Parameter via LSP (IntelliJ Introduce Parameter) |  |
| `inline_refactor` | Inline refactoring (variable/method) via LSP (IntelliJ Inline) |  |
| `rewrite_refactor` | Rewrite refactoring (change signature etc.) via LSP |  |
| `refactor_this` | Show all applicable refactorings (IntelliJ Refactor This) |  |
| `organize_imports` | Organize/optimize imports via LSP source action (IntelliJ Ctrl-Alt-O) | **spacemacs** — normal: `` <space>lO ``, select: `` <space>lO `` |
| `implement_methods` | Implement missing interface/trait members via LSP (IntelliJ Ctrl-I) | **spacemacs** — normal: `` <space>li ``, select: `` <space>li `` |
| `override_methods` | Override inherited members via LSP (IntelliJ Ctrl-O) | **spacemacs** — normal: `` <space>lv ``, select: `` <space>lv `` |
| `generate_code` | Generate code (getters/constructors/impls) via LSP (SPC l g) | **spacemacs** — normal: `` <space>lg ``, select: `` <space>lg `` |
| `change_signature` | Change signature refactor via LSP |  |
| `pull_members_up` | Pull members up refactor via LSP (IntelliJ) |  |
| `push_members_down` | Push members down refactor via LSP (IntelliJ) |  |
| `safe_delete` | Safe Delete: delete the symbol under the cursor only if unused, else show its usages (JetBrains Safe Delete) |  |
| `buffer_picker` | Open buffer picker | **spacemacs** — normal: `` <C-x>b ``, `` <space>bb ``, `` <space>lb ``, `` <space>lt ``, `` <space>pb ``, `` <space>b.b ``, select: `` <C-x>b ``, `` <space>bb ``, `` <space>lb ``, `` <space>lt ``, `` <space>pb ``, `` <space>b.b ``, insert: `` <C-x>b ``<br>**helix** — normal: `` <space>b ``, select: `` <space>b ``<br>**emacs** — normal: `` <C-x>b ``, insert: `` <C-x>b ``<br>**cua** — normal: `` <C-x>b ``, select: `` <C-X>b ``, insert: `` <C-x>b `` |
| `jumplist_picker` | Open jumplist picker | **spacemacs** — normal: `` <space>jj ``, select: `` <space>jj ``<br>**helix** — normal: `` <space>j ``, select: `` <space>j `` |
| `register_picker` | Browse registers and paste the chosen one | **spacemacs** — normal: `` <space>re ``, `` <space>rr ``, `` <space>ry ``, select: `` <space>re ``, `` <space>rr ``, `` <space>ry `` |
| `marks_picker` | Fuzzy-pick a vim mark and jump to it (:Marks) | **spacemacs** — normal: `` <space>fb ``, `` <space>rm ``, select: `` <space>fb ``, `` <space>rm `` |
| `buffer_line_picker` | Fuzzy-search lines in the current buffer (:BLines) | **spacemacs** — normal: `` <space>sL ``, select: `` <space>sL `` |
| `command_history_picker` | Fuzzy-pick and run a past command line (:History:) | **spacemacs** — normal: `` <space>r: ``, `` <C-x><esc><esc> ``, select: `` <space>r: ``, `` <C-x><esc><esc> ``, insert: `` <C-x><esc><esc> `` |
| `cmdline_window` | Edit the command-line history in a buffer; <CR> runs the line (vim q:) |  |
| `cmdline_window_execute` | Run the command-line under the cursor and close the window (vim q: <CR>) |  |
| `search_history_picker` | Fuzzy-pick and re-run a past search (:History/) | **spacemacs** — normal: `` <space>r/ ``, select: `` <space>r/ `` |
| `unicode_picker` | Fuzzy-pick a character/digraph and insert it (helm-unicode) | **spacemacs** — normal: `` <C-x>8e ``, `` <space>iu ``, `` <C-x>8<ret> ``, select: `` <C-x>8e ``, `` <space>iu ``, `` <C-x>8<ret> ``, insert: `` <C-x>8e ``, `` <C-x>8<ret> `` |
| `git_file_log_picker` | Commit log for the current file (:BCommits) | **spacemacs** — normal: `` <space>gt ``, `` <space>gfl ``, select: `` <space>gt ``, `` <space>gfl `` |
| `git_repo_log_picker` | Commit log for the whole repo (:Commits) | **spacemacs** — normal: `` <space>gL ``, select: `` <space>gL `` |
| `theme_picker` | Open fuzzy theme picker with live preview | **spacemacs** — normal: `` <space>Tc ``, select: `` <space>Tc `` |
| `wrap_sexp` | Wrap the selection in parentheses | **spacemacs** — normal: `` <space>kw ``, select: `` <space>kw `` |
| `symbol_picker` | Open symbol picker | **spacemacs** — normal: `` gO ``, `` <space>ji ``, `` <space>pg ``, `` <space>sj ``, select: `` <space>ji ``, `` <space>pg ``, `` <space>sj ``<br>**vim** — normal: `` gO `` |
| `syntax_symbol_picker` | Open symbol picker from syntax information |  |
| `lsp_or_syntax_symbol_picker` | Open symbol picker from LSP or syntax information | **helix** — normal: `` <space>s ``, select: `` <space>s `` |
| `changed_file_picker` | Open changed file picker | **spacemacs** — normal: `` <space>bm ``, select: `` <space>bm ``<br>**helix** — normal: `` <space>g ``, select: `` <space>g `` |
| `frecent_file_picker` | Open recent files ranked by frecency (z algorithm) | **spacemacs** — normal: `` <space>fr ``, select: `` <space>fr `` |
| `reopen_last_closed` | Reopen the most recently closed file | **spacemacs** — normal: `` <space>bu ``, `` <space>fu ``, select: `` <space>bu ``, `` <space>fu `` |
| `harpoon_add` | Pin the current file to the harpoon list | **spacemacs** — normal: `` <space>Ha ``, select: `` <space>Ha `` |
| `harpoon_jump` | Jump to the harpoon mark in slot [count] | **spacemacs** — normal: `` <space>Hj ``, select: `` <space>Hj `` |
| `harpoon_1` | Jump to harpoon mark 1 | **spacemacs** — normal: `` <space>H1 ``, select: `` <space>H1 `` |
| `harpoon_2` | Jump to harpoon mark 2 | **spacemacs** — normal: `` <space>H2 ``, select: `` <space>H2 `` |
| `harpoon_3` | Jump to harpoon mark 3 | **spacemacs** — normal: `` <space>H3 ``, select: `` <space>H3 `` |
| `harpoon_4` | Jump to harpoon mark 4 | **spacemacs** — normal: `` <space>H4 ``, select: `` <space>H4 `` |
| `harpoon_next` | Open the next harpoon mark | **spacemacs** — normal: `` <space>Hn ``, select: `` <space>Hn `` |
| `harpoon_prev` | Open the previous harpoon mark | **spacemacs** — normal: `` <space>Hp ``, select: `` <space>Hp `` |
| `bookmark_toggle` | Toggle a line bookmark (JetBrains F11) | **spacemacs** — normal: `` <space>rt ``, select: `` <space>rt `` |
| `bookmark_next` | Jump to the next line bookmark (JetBrains) | **spacemacs** — normal: `` <space>rn ``, select: `` <space>rn `` |
| `bookmark_prev` | Jump to the previous line bookmark (JetBrains) | **spacemacs** — normal: `` <space>rN ``, select: `` <space>rN `` |
| `harpoon_menu` | Open the harpoon marks menu | **spacemacs** — normal: `` <space>Hh ``, `` <space>Hl ``, select: `` <space>Hh ``, `` <space>Hl `` |
| `harpoon_remove` | Unpin the current file from harpoon | **spacemacs** — normal: `` <space>Hd ``, select: `` <space>Hd `` |
| `select_references_to_symbol_under_cursor` | Select symbol references | **spacemacs** — normal: `` <space>se ``, `` <space>sh ``, select: `` <space>se ``, `` <space>sh ``<br>**helix** — normal: `` <space>h ``, select: `` <space>h `` |
| `workspace_symbol_picker` | Open workspace symbol picker | **spacemacs** — normal: `` <A-C-.> ``, `` <C-c>,J ``, `` <space>jI ``, `` <space>sS ``, select: `` <C-c>,J ``, `` <space>jI ``, `` <space>sS ``, insert: `` <C-c>,J `` |
| `syntax_workspace_symbol_picker` | Open workspace symbol picker from syntax information |  |
| `lsp_or_syntax_workspace_symbol_picker` | Open workspace symbol picker from LSP or syntax information | **helix** — normal: `` <space>S ``, select: `` <space>S `` |
| `diagnostics_picker` | Open diagnostic picker | **spacemacs** — normal: `` <space>el ``, `` <space>enl ``, `` <space>epl ``, select: `` <space>el ``, `` <space>enl ``, `` <space>epl ``<br>**helix** — normal: `` <space>d ``, select: `` <space>d `` |
| `workspace_diagnostics_picker` | Open workspace diagnostic picker | **spacemacs** — normal: `` <space>eL ``, select: `` <space>eL ``<br>**helix** — normal: `` <space>D ``, select: `` <space>D `` |
| `last_picker` | Open last picker | **spacemacs** — normal: `` <space>' ``, `` <space>rl ``, `` <space>rs ``, `` <space>sl ``, select: `` <space>' ``, `` <space>rl ``, `` <space>rs ``, `` <space>sl ``<br>**helix** — normal: `` <space>' ``, select: `` <space>' `` |
| `insert_at_line_start` | Insert at start of line | **spacemacs, vim** — normal: `` I ``, `` gI ``<br>**helix** — normal: `` I ``, select: `` I `` |
| `insert_at_line_end` | Insert at end of line | **spacemacs, vim** — normal: `` A ``<br>**helix** — normal: `` A ``, select: `` A `` |
| `open_below` | Open new line below selection | **spacemacs, vim** — normal: `` o ``<br>**helix** — normal: `` o ``, select: `` o `` |
| `open_above` | Open new line above selection | **spacemacs, vim** — normal: `` O ``<br>**helix** — normal: `` O ``, select: `` O `` |
| `complete_current_statement` | Complete the current statement (close brackets, add terminator, open next line) (JetBrains) | **spacemacs** — normal: `` <C-c>; ``, select: `` <C-c>; ``, insert: `` <C-c>; `` |
| `postfix_expand` | Postfix completion: expand `expr.kw` (if/for/while/match/let/return/not/…) (JetBrains) | **spacemacs** — normal: `` <C-c>. ``, select: `` <C-c>. ``, insert: `` <C-c>. `` |
| `normal_mode` | Enter normal mode | **spacemacs** — normal: `` <C-\><C-g> ``, `` <C-\><C-n> ``<br>**vim** — normal: `` <C-c> ``, `` <C-\><C-g> ``, `` <C-\><C-n> ``<br>**helix** — normal: `` <esc> ``, select: `` v ``, insert: `` <esc> `` |
| `select_mode` | Enter selection extend mode | **spacemacs** — normal: `` v ``, `` gh ``, `` <C-space> ``, `` <space>kv ``, `` <space>k<C-v> ``, select: `` <space>kv ``, `` <space>k<C-v> ``<br>**vim** — normal: `` v ``, `` gh ``<br>**helix** — normal: `` v ``<br>**emacs, cua** — normal: `` <C-space> `` |
| `exit_select_mode` | Exit selection mode | **helix** — select: `` <esc> `` |
| `goto_definition` | Goto definition | **spacemacs** — normal: `` g] ``, `` gd ``, `` <A-.> ``, `` <C-w>] ``, `` g<C-]> ``, `` <C-c>,j ``, `` <C-w>g] ``, `` <C-x>5. ``, `` <space>gd ``, `` <space>jf ``, `` <space>jv ``, `` <space>w] ``, `` <C-w><C-]> ``, `` <space>mgg ``, `` <space>wg] ``, `` <C-w>g<C-]> ``, `` <space>w<C-]> ``, `` <space>wg<C-]> ``, select: `` <C-]> ``, `` <C-c>,j ``, `` <C-x>5. ``, `` <space>gd ``, `` <space>jf ``, `` <space>jv ``, `` <space>w] ``, `` <space>mgg ``, `` <space>wg] ``, `` <space>w<C-]> ``, `` <space>wg<C-]> ``, insert: `` <C-c>,j ``, `` <C-x>5. ``<br>**vim** — normal: `` g] ``, `` gd ``, `` <C-w>] ``, `` g<C-]> ``, `` <C-w>g] ``, `` <C-w><C-]> ``, `` <C-w>g<C-]> ``, select: `` <C-]> ``<br>**helix** — normal: `` gd ``, select: `` gd ``<br>**emacs, cua** — normal: `` <A-.> ``, insert: `` <A-.> `` |
| `tag_jump` | Jump to the definition of the symbol under the cursor, recording where from on the tag stack (vim CTRL-]) | **spacemacs, vim** — normal: `` <C-]> `` |
| `preview_tag` | Show the tag under the cursor in the preview window (vim CTRL-W }) | **spacemacs** — normal: `` <C-w>} ``, `` <space>w} ``, select: `` <space>w} ``<br>**vim** — normal: `` <C-w>} `` |
| `preview_tjump` | Show the tag under the cursor in the preview window, listing ambiguous matches (vim CTRL-W g }) | **spacemacs** — normal: `` <C-w>g} ``, `` <space>wg} ``, select: `` <space>wg} ``<br>**vim** — normal: `` <C-w>g} `` |
| `tag_pop` | Jump back to the position the last tag jump started from (vim CTRL-T, :pop) | **spacemacs, vim** — normal: `` <C-t> `` |
| `peek_definition` | Peek the definition in a popup without navigating (JetBrains Quick Definition) | **spacemacs** — normal: `` <space>lq ``, select: `` <space>lq `` |
| `goto_declaration` | Goto declaration | **spacemacs** — normal: `` gD ``, `` <C-w>i ``, `` <space>gD ``, `` <space>wi ``, `` <C-w><C-i> ``, `` <space>w<C-i> ``, select: `` <space>gD ``, `` <space>wi ``, `` <space>w<C-i> ``<br>**vim** — normal: `` gD ``, `` <C-w>i ``, `` <C-w><C-i> ``<br>**helix** — normal: `` gD ``, select: `` gD `` |
| `add_newline_above` | Add newline above | **helix** — normal: `` [<space> ``, select: `` [<space> `` |
| `add_newline_below` | Add newline below | **helix** — normal: `` ]<space> ``, select: `` ]<space> `` |
| `goto_type_definition` | Goto type definition | **spacemacs** — normal: `` gy ``, `` <space>gy ``, select: `` <space>gy ``<br>**vim** — normal: `` gy ``<br>**helix** — normal: `` gy ``, select: `` gy `` |
| `goto_implementation` | Goto implementation | **helix** — normal: `` gi ``, select: `` gi `` |
| `goto_file_start` | Goto line number `<n>` else file start | **spacemacs** — normal: `` gg ``, `` <A-lt> ``, `` <C-home> ``, insert: `` <A-lt> ``, `` <C-home> ``<br>**vim** — normal: `` gg ``, `` <C-home> ``, insert: `` <C-home> ``<br>**helix** — normal: `` gg ``<br>**emacs, cua** — normal: `` <A-lt> ``, select: `` <A-lt> ``, insert: `` <A-lt> `` |
| `goto_file_end` | Goto file end | **spacemacs** — insert: `` <A-gt> ``, `` <C-end> ``<br>**vim** — insert: `` <C-end> `` |
| `extend_to_file_start` | Extend to line number `<n>` else file start | **helix** — select: `` gg `` |
| `extend_to_file_end` | Extend to file end |  |
| `goto_file` | Goto files/URLs in selections | **spacemacs** — normal: `` [f ``, `` ]f ``, `` gf ``, `` gx ``, `` <C-w>gF ``, `` <C-w>gf ``, `` <space>fF ``, `` <space>jU ``, `` <space>ju ``, `` <space>pF ``, `` <space>wgF ``, `` <space>wgf ``, select: `` <space>fF ``, `` <space>jU ``, `` <space>ju ``, `` <space>pF ``, `` <space>wgF ``, `` <space>wgf ``<br>**vim** — normal: `` [f ``, `` ]f ``, `` gf ``, `` gx ``, `` <C-w>gF ``, `` <C-w>gf ``<br>**helix** — normal: `` gf ``, select: `` gf `` |
| `goto_file_hsplit` | Goto files in selections (hsplit) | **spacemacs** — normal: `` <C-w>F ``, `` <space>wF ``, `` <C-w><C-f> ``, `` <space>w<C-f> ``, select: `` <space>wF ``, `` <space>w<C-f> ``<br>**vim** — normal: `` <C-w>F ``, `` <C-w><C-f> ``<br>**helix** — normal: `` <C-w>f ``, `` <space>wf ``, select: `` <C-w>f ``, `` <space>wf `` |
| `goto_file_vsplit` | Goto files in selections (vsplit) | **helix** — normal: `` <C-w>F ``, `` <space>wF ``, select: `` <C-w>F ``, `` <space>wF `` |
| `goto_file_readonly` | Visit the file at point read-only (emacs ffap-read-only, C-x C-r) |  |
| `goto_file_new_tab` | Visit the file at point in a new tab (emacs ffap-other-tab, C-x t C-f) |  |
| `goto_file_other_frame` | Visit the file at point in a new frame (emacs ffap-other-frame, C-x 5 f) |  |
| `goto_reference` | Goto references | **spacemacs, vim** — normal: `` gr ``<br>**helix** — normal: `` gr ``, select: `` gr ``<br>**emacs, cua** — normal: `` <A-?> ``, insert: `` <A-?> `` |
| `call_hierarchy_incoming_calls` | Call hierarchy: who calls the symbol (JetBrains Ctrl-Alt-H) | **spacemacs** — normal: `` <space>gh ``, select: `` <space>gh `` |
| `call_hierarchy_outgoing_calls` | Call hierarchy: what the symbol calls | **spacemacs** — normal: `` <space>gH ``, select: `` <space>gH `` |
| `type_hierarchy_supertypes` | Type hierarchy: supertypes of the symbol (JetBrains Ctrl-H) | **spacemacs** — normal: `` <space>gT ``, select: `` <space>gT `` |
| `type_hierarchy_subtypes` | Type hierarchy: subtypes of the symbol |  |
| `goto_window_top` | Goto window top | **spacemacs, vim** — normal: `` H ``<br>**helix** — normal: `` gt ``, select: `` gt `` |
| `what_line` | Report the line number of point (emacs what-line) |  |
| `what_page` | Report the page number and line within the page (emacs what-page) |  |
| `count_lines_page` | Report lines on the current page, before + after point (emacs count-lines-page, C-x l) | **spacemacs** — normal: `` <C-x>l ``, select: `` <C-x>l ``, insert: `` <C-x>l `` |
| `what_cursor_position` | Report the character at point, its code, position, percentage and column (emacs what-cursor-position, C-x =) | **spacemacs** — normal: `` <C-x>= ``, select: `` <C-x>= ``, insert: `` <C-x>= `` |
| `move_to_window_line_top_bottom` | Move point to window centre/top/bottom, cycling (emacs move-to-window-line-top-bottom, M-r) | **spacemacs** — normal: `` <A-r> ``<br>**emacs, cua** — normal: `` <A-r> ``, insert: `` <A-r> `` |
| `goto_window_center` | Goto window center | **spacemacs, vim** — normal: `` M ``<br>**helix** — normal: `` gc ``, select: `` gc `` |
| `goto_window_bottom` | Goto window bottom | **spacemacs, vim** — normal: `` L ``<br>**helix** — normal: `` gb ``, select: `` gb `` |
| `goto_last_accessed_file` | Goto last accessed file | **spacemacs** — normal: `` <C-^> ``, `` <C-w>^ ``, `` g<tab> ``, `` <C-tab> ``, `` <space>w^ ``, `` <C-w><C-^> ``, `` <C-w>g<tab> ``, `` <space><tab> ``, `` <space>w<C-^> ``, `` <space>wg<tab> ``, select: `` <space>w^ ``, `` <space><tab> ``, `` <space>w<C-^> ``, `` <space>wg<tab> ``<br>**vim** — normal: `` <C-^> ``, `` <C-w>^ ``, `` g<tab> ``, `` <C-tab> ``, `` <C-w><C-^> ``, `` <C-w>g<tab> ``<br>**helix** — normal: `` ga ``, select: `` ga `` |
| `goto_last_modified_file` | Goto last modified file | **helix** — normal: `` gm ``, select: `` gm `` |
| `goto_last_modification` | Goto last modification | **spacemacs, vim** — normal: `` g. ``<br>**helix** — normal: `` g. ``, select: `` g. `` |
| `goto_older_change` | vim g;: jump to an older change-list position | **spacemacs, vim** — normal: `` g; `` |
| `goto_newer_change` | vim g,: jump to a newer change-list position | **spacemacs, vim** — normal: `` g, `` |
| `goto_line` | Goto line | **spacemacs** — normal: `` <A-g>g ``, `` <A-g><A-g> ``<br>**helix** — normal: `` G ``, select: `` G ``<br>**emacs, cua** — normal: `` <A-g>g ``, `` <A-g><A-g> ``, insert: `` <A-g>g ``, `` <A-g><A-g> `` |
| `goto_last_line` | Goto last line | **spacemacs** — normal: `` G ``, `` <A-gt> ``, `` <C-end> ``<br>**vim** — normal: `` G ``, `` <C-end> ``<br>**helix** — normal: `` ge ``<br>**emacs, cua** — normal: `` <A-gt> ``, select: `` <A-gt> ``, insert: `` <A-gt> `` |
| `extend_to_last_line` | Extend to last line | **helix** — select: `` ge `` |
| `goto_first_diag` | Goto first diagnostic | **spacemacs** — normal: `` <space>ef ``, `` <space>enf ``, `` <space>epf ``, select: `` <space>ef ``, `` <space>enf ``, `` <space>epf ``<br>**helix** — normal: `` [D ``, select: `` [D `` |
| `copy_diagnostic` | Copy the diagnostic message(s) on the current line |  |
| `goto_last_diag` | Goto last diagnostic | **spacemacs** — normal: `` <space>e. ``, select: `` <space>e. ``<br>**helix** — normal: `` ]D ``, select: `` ]D `` |
| `goto_next_diag` | Goto next diagnostic | **spacemacs** — normal: `` ]d ``, `` <space>enj ``, `` <space>enn ``, `` <space>epj ``, `` <space>epn ``, select: `` <space>enj ``, `` <space>enn ``, `` <space>epj ``, `` <space>epn ``<br>**vim** — normal: `` ]d ``<br>**helix** — normal: `` ]d ``, select: `` ]d ``<br>**emacs, cua** — normal: `` <A-g>n ``, insert: `` <A-g>n ``, `` <A-g><A-n> `` |
| `goto_prev_diag` | Goto previous diagnostic | **spacemacs** — normal: `` [d ``, `` <space>enk ``, `` <space>enp ``, `` <space>epk ``, `` <space>epp ``, select: `` <space>enk ``, `` <space>enp ``, `` <space>epk ``, `` <space>epp ``<br>**vim** — normal: `` [d ``<br>**helix** — normal: `` [d ``, select: `` [d ``<br>**emacs, cua** — normal: `` <A-g>p ``, insert: `` <A-g>p ``, `` <A-g><A-p> `` |
| `goto_next_change` | Goto next change | **spacemacs, vim** — normal: `` ]g ``<br>**helix** — normal: `` ]g ``, select: `` ]g `` |
| `goto_prev_change` | Goto previous change | **spacemacs, vim** — normal: `` [g ``<br>**helix** — normal: `` [g ``, select: `` [g `` |
| `goto_next_conflict` | Goto next merge-conflict marker | **spacemacs, vim** — normal: `` ]n `` |
| `goto_prev_conflict` | Goto previous merge-conflict marker | **spacemacs, vim** — normal: `` [n `` |
| `conflict_take_all_ours` | Resolve ALL conflicts: keep our side | **spacemacs** — normal: `` <space>gcO ``, select: `` <space>gcO `` |
| `conflict_take_all_theirs` | Resolve ALL conflicts: keep their side | **spacemacs** — normal: `` <space>gcT ``, select: `` <space>gcT `` |
| `git_diff` | Open side-by-side diff vs HEAD | **spacemacs** — normal: `` <C-x>v= ``, `` <space>g= ``, `` <space>Dfv ``, `` <space>gfd ``, select: `` <C-x>v= ``, `` <space>g= ``, `` <space>Dfv ``, `` <space>gfd ``, insert: `` <C-x>v= `` |
| `resolve_conflicts` | Resolve merge conflicts (3-way) | **spacemacs** — normal: `` <space>gm ``, `` <space>gcr ``, select: `` <space>gm ``, `` <space>gcr `` |
| `git_status` | Magit status | **spacemacs** — normal: `` <C-x>v! ``, `` <C-x>vd ``, `` <space>gs ``, `` <space>pv ``, select: `` <C-x>v! ``, `` <C-x>vd ``, `` <space>gs ``, `` <space>pv ``, insert: `` <C-x>v! ``, `` <C-x>vd `` |
| `git_push` | Push the current branch to its remote (SPC g P) | **spacemacs** — normal: `` <C-x>vP ``, `` <space>gP ``, select: `` <C-x>vP ``, `` <space>gP ``, insert: `` <C-x>vP `` |
| `git_pull` | Fast-forward pull from upstream (SPC g u) | **spacemacs** — normal: `` <C-x>v+ ``, `` <space>gu ``, select: `` <C-x>v+ ``, `` <space>gu ``, insert: `` <C-x>v+ `` |
| `git_fetch` | Fetch all remotes (SPC g F) | **spacemacs** — normal: `` <space>gF ``, select: `` <space>gF `` |
| `git_acp` | Stage all, commit, and push in one shot (C-x v c) | **spacemacs** — normal: `` <C-x>vc ``, select: `` <C-x>vc ``, insert: `` <C-x>vc `` |
| `vc_print_log` | VC log for the current file (emacs vc-print-log) | **spacemacs** — normal: `` <C-x>vl ``, select: `` <C-x>vl ``, insert: `` <C-x>vl `` |
| `vc_print_root_log` | VC log for the whole repository (emacs vc-print-root-log) | **spacemacs** — normal: `` <C-x>vL ``, select: `` <C-x>vL ``, insert: `` <C-x>vL `` |
| `vc_print_branch_log` | VC log for a named branch (emacs vc-print-branch-log) | **spacemacs** — normal: `` <C-x>vbl ``, select: `` <C-x>vbl ``, insert: `` <C-x>vbl `` |
| `vc_root_diff` | Diff the whole working tree vs HEAD (emacs vc-root-diff) | **spacemacs** — normal: `` <C-x>vD ``, select: `` <C-x>vD ``, insert: `` <C-x>vD `` |
| `vc_region_history` | History of the selected line range (emacs vc-region-history) | **spacemacs** — normal: `` <C-x>vh ``, select: `` <C-x>vh ``, insert: `` <C-x>vh `` |
| `vc_log_incoming` | Commits incoming from upstream (emacs vc-log-incoming) | **spacemacs** — normal: `` <C-x>vI ``, select: `` <C-x>vI ``, insert: `` <C-x>vI `` |
| `vc_log_outgoing` | Commits outgoing to upstream (emacs vc-log-outgoing) | **spacemacs** — normal: `` <C-x>vO ``, select: `` <C-x>vO ``, insert: `` <C-x>vO `` |
| `vc_log_search` | Search the commit log by pattern (emacs vc-log-search) |  |
| `vc_create_branch` | Create and switch to a new branch (emacs vc-create-branch) | **spacemacs** — normal: `` <C-x>vbc ``, select: `` <C-x>vbc ``, insert: `` <C-x>vbc `` |
| `vc_switch_branch` | Switch to an existing branch (emacs vc-switch-branch) | **spacemacs** — normal: `` <C-x>vbs ``, select: `` <C-x>vbs ``, insert: `` <C-x>vbs `` |
| `vc_create_tag` | Create a git tag at HEAD (emacs vc-create-tag) | **spacemacs** — normal: `` <C-x>vs ``, select: `` <C-x>vs ``, insert: `` <C-x>vs `` |
| `vc_retrieve_tag` | Check out a tag or branch (emacs vc-retrieve-tag) | **spacemacs** — normal: `` <C-x>vr ``, select: `` <C-x>vr ``, insert: `` <C-x>vr `` |
| `vc_rename_file` | Rename the current file under VC (emacs vc-rename-file) |  |
| `vc_delete_file` | Delete the current file under VC (emacs vc-delete-file) |  |
| `vc_ignore` | Add the current file to .gitignore (emacs vc-ignore) | **spacemacs** — normal: `` <C-x>vG ``, select: `` <C-x>vG ``, insert: `` <C-x>vG `` |
| `vc_revert` | Revert the current file to HEAD (emacs vc-revert) | **spacemacs** — normal: `` <C-x>vu ``, select: `` <C-x>vu ``, insert: `` <C-x>vu `` |
| `vc_refresh_state` | Recompute the buffer's VC state (emacs vc-refresh-state) |  |
| `vc_state_refresh` | Recompute the buffer's VC state (emacs vc-state-refresh) |  |
| `vc_pull` | Pull from upstream (emacs vc-pull) |  |
| `vc_push` | Push to upstream (emacs vc-push) |  |
| `vc_next_action` | Do the next logical VC step: stage + commit (emacs vc-next-action) | **spacemacs** — normal: `` <C-x>vv ``, select: `` <C-x>vv ``, insert: `` <C-x>vv `` |
| `vc_dir` | Open the VC directory / Magit status (emacs vc-dir) |  |
| `project_vc_dir` | Open the project's VC directory / Magit status (emacs project-vc-dir) |  |
| `project_any_command` | Pick a command and run it with the working directory set to the project root (emacs project-any-command) |  |
| `imenu_add_menubar_index` | Add an Index menu of this buffer's definitions (emacs imenu-add-menubar-index) |  |
| `menu_bar_open` | Open the menu bar (emacs menu-bar-open, F10) | **emacs, cua** — normal: `` <F10> ``, insert: `` <F10> `` |
| `tmm_menubar` | Pick a menu-bar command from a text list (emacs tmm-menubar) | **emacs, cua** — normal: `` <A-`> ``, insert: `` <A-`> `` |
| `vc_dir_mark` | Mark the file at point in the VC directory (emacs vc-dir-mark) |  |
| `vc_dir_mark_all_files` | Mark every file in the VC directory (emacs vc-dir-mark-all-files) |  |
| `vc_dir_mark_by_regexp` | Mark VC-directory files matching a regexp (emacs vc-dir-mark-by-regexp) |  |
| `vc_dir_mark_registered_files` | Mark every registered file in the VC directory (emacs vc-dir-mark-registered-files) |  |
| `log_edit_done` | Finish the commit message and commit (emacs log-edit-done) |  |
| `log_edit_show_diff` | Show the diff of what is being committed (emacs log-edit-show-diff) |  |
| `log_edit_show_files` | List the files being committed (emacs log-edit-show-files) |  |
| `log_edit_insert_changelog` | Seed the commit message from the ChangeLog (emacs log-edit-insert-changelog) |  |
| `log_edit_generate_changelog_from_diff` | Write the commit message from the diff (emacs log-edit-generate-changelog-from-diff) |  |
| `project_switch_project` | Switch to another project (emacs project-switch-project) |  |
| `project_forget_project` | Remove a project from the known-project list (emacs project-forget-project) |  |
| `project_search` | Grep-search the project (emacs project-search) |  |
| `project_query_replace_regexp` | Project-wide regex replace (emacs project-query-replace-regexp) |  |
| `project_list_buffers` | List open buffers (emacs project-list-buffers) |  |
| `project_shell_command` | Run a shell command in the project root (emacs project-shell-command) | **spacemacs** — normal: `` <space>p! ``, select: `` <space>p! `` |
| `project_async_shell_command` | Run an async shell command in the project (emacs project-async-shell-command) | **spacemacs** — normal: `` <space>p& ``, select: `` <space>p& `` |
| `project_eshell` | Open a shell buffer for the project (emacs project-eshell) |  |
| `xref_find_definitions_other_window` | Goto definition in another window (emacs xref-find-definitions-other-window) | **spacemacs** — normal: `` <C-w>d ``, `` <C-x>4. ``, `` <space>mgG ``, select: `` <C-x>4. ``, `` <space>mgG ``, insert: `` <C-x>4. ``<br>**vim** — normal: `` <C-w>d ``<br>**emacs** — insert: `` <C-x>4. ``<br>**cua** — select: `` <C-X>4. ``, insert: `` <C-x>4. `` |
| `info_search_other_window` | Open the Info directory node in another window (emacs info-other-window) | **spacemacs** — normal: `` <C-h>4i ``, select: `` <C-h>4i ``, insert: `` <C-h>4i `` |
| `xref_query_replace_in_results` | Query-replace across xref/project results (emacs xref-query-replace-in-results) |  |
| `xref_find_references_and_replace` | Find references and replace them (emacs xref-find-references-and-replace) |  |
| `cut_to_clipboard` | Cut the selection to the system clipboard |  |
| `org_cycle` | Org: toggle subtree fold | **spacemacs** — normal: `` <space>mz ``, `` <space>m<tab> ``, select: `` <space>mz ``, `` <space>m<tab> `` |
| `org_todo` | Org: cycle TODO keyword |  |
| `org_priority` | Org: cycle priority cookie | **spacemacs** — normal: `` <space>mp ``, select: `` <space>mp `` |
| `org_promote` | Org: promote heading | **spacemacs** — normal: `` <space>mH ``, select: `` <space>mH `` |
| `org_demote` | Org: demote heading | **spacemacs** — normal: `` <space>ml ``, select: `` <space>ml `` |
| `org_next_heading` | Org: next heading | **spacemacs** — normal: `` <space>mj ``, select: `` <space>mj `` |
| `org_prev_heading` | Org: previous heading |  |
| `org_fold_all` | Org: fold all headings | **spacemacs** — normal: `` <space>ma ``, select: `` <space>ma `` |
| `outline_minor_mode` | Fold the buffer by outline headings; run again to unfold (emacs outline-minor-mode) |  |
| `org_unfold_all` | Org: unfold all | **spacemacs** — normal: `` <space>mA ``, select: `` <space>mA `` |
| `org_agenda` | Org: open agenda | **spacemacs** — normal: `` <space>aoa ``, select: `` <space>aoa `` |
| `org_capture` | Org: capture note | **spacemacs** — normal: `` <space>oc ``, `` <space>aoc ``, select: `` <space>oc ``, `` <space>aoc `` |
| `goto_first_change` | Goto first change | **helix** — normal: `` [G ``, select: `` [G `` |
| `goto_last_change` | Goto last change | **spacemacs** — normal: `` <space>jc ``, select: `` <space>jc ``<br>**helix** — normal: `` ]G ``, select: `` ]G `` |
| `goto_line_start` | Goto line start | **spacemacs** — normal: `` 0 ``, `` g<home> ``, `` <space>j0 ``, select: `` <space>j0 ``, insert: `` <home> ``<br>**vim** — normal: `` 0 ``, `` g<home> ``, insert: `` <home> ``<br>**helix** — normal: `` gh ``, `` <home> ``, select: `` gh ``, insert: `` <home> ``<br>**emacs, cua** — normal: `` <C-a> ``, `` <home> ``, select: `` <C-a> ``, insert: `` <C-a> ``, `` <home> `` |
| `goto_line_end` | Goto line end | **spacemacs** — normal: `` $ ``, `` gl ``, `` g<end> ``, `` <space>j$ ``, select: `` <space>j$ ``<br>**vim** — normal: `` $ ``, `` gl ``, `` g<end> ``<br>**helix** — normal: `` gl ``, `` <end> ``, select: `` gl ``<br>**emacs, cua** — normal: `` <C-e> ``, `` <end> ``, select: `` <C-e> ``, insert: `` <C-e> ``, `` <end> `` |
| `goto_visual_line_start` | Goto visual line start (soft-wrap aware) | **spacemacs, vim** — normal: `` g0 `` |
| `goto_visual_first_nonwhitespace` | Goto first non-blank of the screen line (vim g^) | **spacemacs, vim** — normal: `` g^ `` |
| `goto_visual_line_end` | Goto visual line end (soft-wrap aware) | **spacemacs, vim** — normal: `` g$ `` |
| `extend_to_visual_line_start` | Extend to visual line start |  |
| `extend_to_visual_line_end` | Extend to visual line end |  |
| `goto_column` | Goto column | **spacemacs** — normal: `` \| ``, `` <A-g><tab> ``<br>**vim** — normal: `` \| ``<br>**helix** — normal: `` g\| `` |
| `extend_to_column` | Extend to column | **helix** — select: `` g\| `` |
| `goto_next_buffer` | Goto next buffer | **spacemacs** — normal: `` ]b ``, `` <space>bn ``, `` <space>b.n ``, `` <C-x><right> ``, select: `` <space>bn ``, `` <space>b.n ``, `` <C-x><right> ``, insert: `` <C-x><right> ``<br>**vim** — normal: `` ]b ``<br>**helix** — normal: `` gn ``, select: `` gn ``<br>**emacs** — normal: `` <C-x><right> ``, insert: `` <C-x><right> ``<br>**cua** — normal: `` <C-x><right> ``, select: `` <C-X><right> ``, insert: `` <C-x><right> `` |
| `goto_previous_buffer` | Goto previous buffer | **spacemacs** — normal: `` [b ``, `` <space>bp ``, `` <space>b.N ``, `` <space>b.p ``, `` <C-x><left> ``, select: `` <space>bp ``, `` <space>b.N ``, `` <space>b.p ``, `` <C-x><left> ``, insert: `` <C-x><left> ``<br>**vim** — normal: `` [b ``<br>**helix** — normal: `` gp ``, select: `` gp ``<br>**emacs** — normal: `` <C-x><left> ``, insert: `` <C-x><left> ``<br>**cua** — normal: `` <C-x><left> ``, select: `` <C-X><left> ``, insert: `` <C-x><left> `` |
| `goto_line_end_newline` | Goto newline at line end | **spacemacs, vim, helix** — insert: `` <end> `` |
| `goto_first_nonwhitespace` | Goto first non-blank in line | **spacemacs** — normal: `` ^ ``, `` <A-m> ``<br>**vim** — normal: `` ^ ``<br>**helix** — normal: `` gs ``, select: `` gs ``<br>**emacs, cua** — normal: `` <A-m> ``, insert: `` <A-m> `` |
| `trim_selections` | Trim whitespace from selections | **helix** — normal: `` _ ``, select: `` _ `` |
| `extend_to_line_start` | Extend to line start | **helix** — select: `` <home> ``<br>**cua** — select: `` <S-home> `` |
| `extend_to_first_nonwhitespace` | Extend to first non-blank in line | **emacs, cua** — select: `` <A-m> `` |
| `extend_to_line_end` | Extend to line end | **helix** — select: `` <end> ``<br>**cua** — select: `` <S-end> `` |
| `extend_to_line_end_newline` | Extend to line end |  |
| `signature_help` | Show signature help | **spacemacs** — normal: `` <space>ls ``, select: `` <space>ls `` |
| `smart_tab` | Insert tab if all cursors have all whitespace to their left; otherwise, run a separate command. |  |
| `insert_tab` | Insert tab char | **helix** — insert: `` <S-tab> ``<br>**emacs, cua** — insert: `` <A-i> `` |
| `insert_newline` | Insert newline char | **spacemacs, vim, helix, emacs, cua** — insert: `` <C-j> ``, `` <ret> `` |
| `default_indent_new_line` | Break line at point and continue the comment, indenting under it (emacs default-indent-new-line, M-j) | **spacemacs** — normal: `` <A-C-j> ``, insert: `` <A-j> ``<br>**vim** — insert: `` <A-j> `` |
| `insert_char_interactive` | Insert an interactively-chosen char | **spacemacs, vim** — insert: `` <C-Q> ``, `` <C-V> ``, `` <C-q> ``, `` <C-v> `` |
| `append_char_interactive` | Append an interactively-chosen char |  |
| `delete_char_backward` | Delete previous char | **spacemacs** — insert: `` <backspace> ``<br>**vim, emacs, cua** — insert: `` <C-h> ``, `` <backspace> ``<br>**helix** — insert: `` <C-h> ``, `` <backspace> ``, `` <S-backspace> `` |
| `delete_char_forward` | Delete next char | **spacemacs, vim** — insert: `` <del> ``<br>**helix** — insert: `` <C-d> ``, `` <del> ``<br>**emacs, cua** — normal: `` <C-d> ``, insert: `` <C-d> ``, `` <del> `` |
| `delete_chars_forward_vim` | Delete char(s) under cursor, line-bounded (vim x) | **spacemacs, vim** — normal: `` x ``, `` <del> `` |
| `delete_chars_backward_vim` | Delete char(s) before cursor, no line join (vim X) | **spacemacs, vim** — normal: `` X `` |
| `replace_chars_vim` | Replace char(s) under cursor, line-bounded (vim r) | **spacemacs, vim** — normal: `` r `` |
| `delete_word_backward` | Delete previous word | **spacemacs, vim, helix** — insert: `` <C-w> ``, `` <A-backspace> ``<br>**emacs, cua** — insert: `` <C-w> ``, `` <A-backspace> ``, `` <A-C-backspace> `` |
| `delete_word_forward` | Delete next word | **spacemacs** — normal: `` <A-d> ``, insert: `` <A-d> ``<br>**vim, emacs, cua** — insert: `` <A-d> ``<br>**helix** — insert: `` <A-d> ``, `` <A-del> `` |
| `insert_kill_entered_vim` | Delete the text entered this insert session (vim i_CTRL-U) | **spacemacs, vim** — insert: `` <C-u> `` |
| `kill_to_line_start` | Delete till start of line | **helix** — insert: `` <C-u> `` |
| `kill_to_line_end` | Delete till end of line | **helix** — insert: `` <C-k> ``<br>**emacs, cua** — normal: `` <C-k> ``, insert: `` <C-k> `` |
| `undo` | Undo change | **spacemacs** — normal: `` u ``, `` <C-/> ``, `` <C-_> ``, `` <C-x>u ``, `` <space>ku ``, select: `` <C-x>u ``, `` <space>ku ``, insert: `` <C-/> ``, `` <C-_> ``, `` <C-x>u ``<br>**vim** — normal: `` u ``<br>**helix** — normal: `` u ``, select: `` u ``<br>**emacs** — normal: `` <C-/> ``, `` <C-_> ``, `` <C-x>u ``, insert: `` <C-/> ``, `` <C-_> ``, `` <C-x>u ``<br>**cua** — normal: `` <C-/> ``, `` <C-_> ``, `` <C-z> ``, `` <C-x>u ``, select: `` <C-X>u ``, insert: `` <C-/> ``, `` <C-_> ``, `` <C-z> ``, `` <C-x>u `` |
| `undo_line` | Undo all latest changes on one line (vim U) | **spacemacs, vim** — normal: `` U `` |
| `redo` | Redo change | **spacemacs** — normal: `` <C-r> ``, `` <space>k<C-r> ``, select: `` <space>k<C-r> ``<br>**vim** — normal: `` <C-r> ``<br>**helix** — normal: `` U ``, select: `` U `` |
| `earlier` | Move backward in history | **spacemacs, vim** — normal: `` g<minus> ``<br>**helix** — normal: `` <A-u> ``, select: `` <A-u> `` |
| `later` | Move forward in history | **spacemacs, vim** — normal: `` g+ ``<br>**helix** — normal: `` <A-U> ``, select: `` <A-U> `` |
| `undo_tree` | Browse the branching undo history (vim undotree) | **spacemacs** — normal: `` <space>aU ``, select: `` <space>aU `` |
| `edit_injected_fragment` | Edit the injected-language fragment at point in its own buffer |  |
| `apply_injected_fragment` | Write the fragment buffer back into its host string |  |
| `commit_undo_checkpoint` | Commit changes to new checkpoint | **spacemacs, vim** — insert: `` <C-g>U ``, `` <C-g>u ``<br>**helix** — insert: `` <C-s> `` |
| `yank` | Yank selection | **spacemacs** — normal: `` <A-w> ``<br>**helix** — normal: `` y ``, select: `` y `` |
| `yank_to_clipboard` | Yank selections to clipboard | **helix** — normal: `` <space>y ``, select: `` <space>y `` |
| `yank_to_primary_clipboard` | Yank selections to primary clipboard |  |
| `yank_joined` | Join and yank selections |  |
| `yank_joined_to_clipboard` | Join and yank selections to clipboard |  |
| `yank_main_selection_to_clipboard` | Yank main selection to clipboard | **helix** — normal: `` <space>Y ``, select: `` <space>Y `` |
| `yank_joined_to_primary_clipboard` | Join and yank selections to primary clipboard |  |
| `yank_main_selection_to_primary_clipboard` | Yank main selection to primary clipboard |  |
| `replace_with_yanked` | Replace with yanked text | **spacemacs, vim** — select: `` P ``, `` p ``<br>**helix** — normal: `` R ``, select: `` R `` |
| `replace_selections_with_clipboard` | Replace selections by clipboard content | **helix** — normal: `` <space>R ``, select: `` <space>R `` |
| `replace_selections_with_primary_clipboard` | Replace selections by primary clipboard |  |
| `paste_after` | Paste after selection | **spacemacs** — normal: `` p ``, `` <space>kp ``, select: `` <space>kp ``<br>**vim** — normal: `` p ``<br>**helix** — normal: `` p ``, select: `` p `` |
| `paste_after_cursor_after` | Paste after selection, cursor after the pasted text (vim gp) | **spacemacs, vim** — normal: `` gp `` |
| `select_pasted_text` | Reselect the text the last put inserted (spacemacs g p) |  |
| `paste_before_cursor_after` | Paste before selection, cursor after the pasted text (vim gP) | **spacemacs, vim** — normal: `` gP `` |
| `paste_before` | Paste before selection | **spacemacs** — normal: `` P ``, `` [P ``, `` ]P ``, `` <space>kP ``, select: `` <space>kP ``<br>**vim** — normal: `` P ``, `` [P ``, `` ]P ``<br>**helix** — normal: `` P ``, select: `` P `` |
| `yank_from_kill_ring` | Yank the latest kill-ring entry (emacs C-y) | **emacs** — normal: `` <C-y> ``, insert: `` <C-y> ``<br>**cua** — normal: `` <C-v> ``, `` <C-y> ``, insert: `` <C-v> ``, `` <C-y> `` |
| `yank_pop` | Replace the just-yanked text with the next kill-ring entry (emacs M-y) | **spacemacs** — normal: `` <A-y> ``<br>**emacs, cua** — normal: `` <A-y> ``, insert: `` <A-y> `` |
| `set_mark_command` | Set mark and activate region, pushing to the mark ring (emacs C-SPC) | **spacemacs** — normal: `` <C-@> ``<br>**emacs, cua** — insert: `` <C-space> `` |
| `pop_to_mark` | Jump to the top of the mark ring, rotating it (emacs C-x C-SPC) | **spacemacs** — normal: `` <C-x><C-space> ``, select: `` <C-x><C-space> ``, insert: `` <C-x><C-space> ``<br>**emacs** — insert: `` <C-x><C-space> ``<br>**cua** — select: `` <C-X><C-space> ``, insert: `` <C-x><C-space> `` |
| `universal_argument` | Begin a prefix argument for the next command; repeat to multiply it by 4 (emacs C-u) | **emacs, cua** — normal: `` <C-u> ``, insert: `` <C-u> `` |
| `digit_argument` | Read a numeric prefix argument for the next command (emacs M-1 … M-9) |  |
| `negative_argument` | Make the prefix argument negative (emacs M--) |  |
| `list_packages` | Open the package menu: browse, install and delete elisp packages (emacs M-x list-packages) |  |
| `package_refresh_contents` | Re-read the package archives' listings (emacs package-refresh-contents) |  |
| `package_install` | Install a package, with its dependencies, from an archive (emacs package-install) |  |
| `package_install_file` | Install a package from a .el or .tar file on disk (emacs package-install-file) |  |
| `package_delete` | Delete an installed package (emacs package-delete) |  |
| `package_upgrade` | Install the newer version of a package (emacs package-upgrade) |  |
| `package_upgrade_all` | Upgrade every package that has a newer version (emacs package-upgrade-all) |  |
| `package_activate_all` | Load every installed package now (emacs package-activate-all) |  |
| `package_quickstart_refresh` | Write package-quickstart.el, preloading every package's autoloads (emacs package-quickstart-refresh) |  |
| `package_recompile` | Re-evaluate a package's elisp, reporting any file that fails (emacs package-recompile) |  |
| `package_recompile_all` | Re-evaluate every installed package's elisp (emacs package-recompile-all) |  |
| `describe_package` | Show a package's version, home page, keywords and dependencies (emacs C-h P) |  |
| `finder_by_keyword` | List the packages that carry a keyword (emacs C-h p / finder-by-keyword) |  |
| `package_browse_url` | Open the home page of the package under point (emacs package-browse-url) |  |
| `package_menu_describe_package` | Describe the package under point in the package menu (emacs RET) |  |
| `package_menu_mark_install` | Mark the package under point for installation (emacs package menu i) |  |
| `package_menu_mark_delete` | Mark the package under point for deletion (emacs package menu d) |  |
| `package_menu_mark_unmark` | Remove the mark from the package under point (emacs package menu u) |  |
| `package_menu_mark_upgrades` | Mark every package that has a newer version (emacs package menu U) |  |
| `package_menu_mark_obsolete_for_deletion` | Mark superseded installed versions for deletion (emacs package menu ~) |  |
| `package_menu_execute` | Carry out the marked installations and deletions (emacs package menu x) |  |
| `package_menu_quick_help` | Show the package menu's key bindings (emacs package menu ?) |  |
| `package_menu_hide_package` | Hide the package under point from the listing (emacs package menu H) |  |
| `package_menu_toggle_hiding` | Show or hide the packages hidden with H (emacs package menu () |  |
| `package_menu_filter_by_name` | Show only packages whose name matches (emacs package menu / n) |  |
| `package_menu_filter_by_description` | Show only packages whose summary matches (emacs package menu / d) |  |
| `package_menu_filter_by_name_or_description` | Show only packages whose name or summary matches (emacs package menu / N) |  |
| `package_menu_filter_by_keyword` | Show only packages carrying a keyword (emacs package menu / k) |  |
| `package_menu_filter_by_status` | Show only packages with a status: installed, available, obsolete, external (emacs package menu / s) |  |
| `package_menu_filter_by_archive` | Show only packages from one archive (emacs package menu / a) |  |
| `package_menu_filter_by_version` | Show only packages of a version (emacs package menu / v) |  |
| `package_menu_filter_marked` | Show only the marked packages (emacs package menu / m) |  |
| `package_menu_filter_upgradable` | Show only packages that can be upgraded (emacs package menu / u) |  |
| `package_menu_filter_clear` | Clear every package-menu filter (emacs package menu / /) |  |
| `package_vc_install` | Install a package straight from its source repository (emacs package-vc-install) |  |
| `package_vc_checkout` | Clone a package's source without installing it (emacs package-vc-checkout) |  |
| `package_vc_install_from_checkout` | Make an existing checkout an installed package (emacs package-vc-install-from-checkout) |  |
| `package_vc_rebuild` | Rebuild a package installed from source (emacs package-vc-rebuild) |  |
| `package_vc_prepare_patch` | Turn your commits on a package into a patch series (emacs package-vc-prepare-patch) |  |
| `package_report_bug` | Start a bug report against a package, with its version filled in (emacs package-report-bug) |  |
| `point_to_register` | Save point to a register (emacs C-x r SPC) | **spacemacs** — normal: `` <C-x>r<space> ``, select: `` <C-x>r<space> ``, insert: `` <C-x>r<space> ``<br>**emacs** — insert: `` <C-x>r<space> ``<br>**cua** — select: `` <C-X>r<space> ``, insert: `` <C-x>r<space> `` |
| `jump_to_register` | Jump to the position in a register (emacs C-x r j) | **spacemacs** — normal: `` <C-x>rj ``, select: `` <C-x>rj ``, insert: `` <C-x>rj ``<br>**emacs** — insert: `` <C-x>rj ``<br>**cua** — select: `` <C-X>rj ``, insert: `` <C-x>rj `` |
| `number_to_register` | Store the prefix count in a register (emacs C-x r n) | **spacemacs** — normal: `` <C-x>rn ``, select: `` <C-x>rn ``, insert: `` <C-x>rn ``<br>**emacs** — insert: `` <C-x>rn ``<br>**cua** — select: `` <C-X>rn ``, insert: `` <C-x>rn `` |
| `increment_register` | Add the prefix count to a number register (emacs C-x r +) | **spacemacs** — normal: `` <C-x>r+ ``, select: `` <C-x>r+ ``, insert: `` <C-x>r+ ``<br>**emacs** — insert: `` <C-x>r+ ``<br>**cua** — select: `` <C-X>r+ ``, insert: `` <C-x>r+ `` |
| `emacs_insert_register` | Insert a number or rectangle register's value at point (emacs C-x r i) | **spacemacs** — normal: `` <C-x>ri ``, select: `` <C-x>ri ``, insert: `` <C-x>ri ``<br>**emacs** — insert: `` <C-x>ri ``<br>**cua** — select: `` <C-X>ri ``, insert: `` <C-x>ri `` |
| `copy_rectangle_to_register` | Copy the selected rectangle into a register (emacs C-x r r) | **spacemacs** — normal: `` <C-x>rr ``, select: `` <C-x>rr ``, insert: `` <C-x>rr `` |
| `kill_rectangle` | Kill (cut) the rectangle, saving it for yank (emacs C-x r k) | **spacemacs** — normal: `` <C-x>rk ``, select: `` <C-x>rk ``, insert: `` <C-x>rk ``<br>**emacs** — insert: `` <C-x>rk ``<br>**cua** — select: `` <C-X>rk ``, insert: `` <C-x>rk `` |
| `delete_rectangle` | Delete the rectangle without saving (emacs C-x r d) | **spacemacs** — normal: `` <C-x>rd ``, select: `` <C-x>rd ``, insert: `` <C-x>rd ``<br>**emacs** — insert: `` <C-x>rd ``<br>**cua** — select: `` <C-X>rd ``, insert: `` <C-x>rd `` |
| `clear_rectangle` | Blank the rectangle with spaces (emacs C-x r c) | **spacemacs** — normal: `` <C-x>rc ``, select: `` <C-x>rc ``, insert: `` <C-x>rc ``<br>**emacs** — insert: `` <C-x>rc ``<br>**cua** — select: `` <C-X>rc ``, insert: `` <C-x>rc `` |
| `copy_rectangle_as_kill` | Copy the rectangle without deleting (emacs C-x r M-w) | **spacemacs** — normal: `` <C-x>r<A-w> ``, select: `` <C-x>r<A-w> ``, insert: `` <C-x>r<A-w> ``<br>**emacs** — insert: `` <C-x>r<A-w> ``<br>**cua** — select: `` <C-X>r<A-w> ``, insert: `` <C-x>r<A-w> `` |
| `yank_rectangle` | Insert the saved rectangle at point (emacs C-x r y) | **spacemacs** — normal: `` <C-x>ry ``, select: `` <C-x>ry ``, insert: `` <C-x>ry ``<br>**emacs** — insert: `` <C-x>ry ``<br>**cua** — select: `` <C-X>ry ``, insert: `` <C-x>ry `` |
| `open_rectangle` | Insert blank space to shift the rectangle right (emacs C-x r o) | **spacemacs** — normal: `` <C-x>ro ``, select: `` <C-x>ro ``, insert: `` <C-x>ro `` |
| `delete_whitespace_rectangle` | Delete whitespace after the rectangle's left column on each line (emacs delete-whitespace-rectangle) |  |
| `bookmark_set` | Set a named persistent bookmark at point (emacs C-x r m) | **spacemacs** — normal: `` <C-x>rm ``, select: `` <C-x>rm ``, insert: `` <C-x>rm ``<br>**emacs** — insert: `` <C-x>rm ``<br>**cua** — select: `` <C-X>rm ``, insert: `` <C-x>rm `` |
| `bookmark_set_no_overwrite` | Set a bookmark, refusing to overwrite an existing name (emacs C-x r M) | **spacemacs** — normal: `` <C-x>rM ``, select: `` <C-x>rM ``, insert: `` <C-x>rM `` |
| `bookmark_jump` | Jump to a bookmark via a picker (emacs C-x r b) | **spacemacs** — normal: `` <C-x>rb ``, `` <space>rj ``, select: `` <C-x>rb ``, `` <space>rj ``, insert: `` <C-x>rb ``<br>**emacs** — insert: `` <C-x>rb ``, `` <C-x>rl ``<br>**cua** — select: `` <C-X>rb ``, `` <C-X>rl ``, insert: `` <C-x>rb ``, `` <C-x>rl `` |
| `list_bookmarks` | List bookmarks in a picker; select to jump (emacs C-x r l / list-bookmarks) | **spacemacs** — normal: `` <C-x>rl ``, select: `` <C-x>rl ``, insert: `` <C-x>rl `` |
| `bookmark_insert_location` | Insert a bookmark's file path at point (emacs bookmark-insert-location, C-x r I) |  |
| `bookmark_insert` | Insert the contents of a bookmark's file at point (emacs bookmark-insert) |  |
| `bookmark_delete` | Delete a bookmark via a picker (emacs bookmark-delete) |  |
| `bookmark_rename` | Rename a bookmark via a picker (emacs bookmark-rename) |  |
| `define_abbrev` | Define a global abbrev: <name> <expansion> (emacs C-x a g) | **spacemacs** — normal: `` <C-x>ag ``, select: `` <C-x>ag ``, insert: `` <C-x>ag ``<br>**emacs** — insert: `` <C-x>ag ``<br>**cua** — select: `` <C-X>ag ``, insert: `` <C-x>ag `` |
| `add_mode_abbrev` | Define the region or word before point as a mode-local abbrev, prompting for its name (emacs add-mode-abbrev, C-x a l) | **spacemacs** — normal: `` <C-x>al ``, select: `` <C-x>al ``, insert: `` <C-x>al ``<br>**emacs** — insert: `` <C-x>al ``<br>**cua** — select: `` <C-X>al ``, insert: `` <C-x>al `` |
| `inverse_add_global_abbrev` | Define the word before point as an abbrev, prompting for its expansion (emacs inverse-add-global-abbrev, C-x a i g) | **spacemacs** — normal: `` <C-x>aig ``, select: `` <C-x>aig ``, insert: `` <C-x>aig ``<br>**emacs** — insert: `` <C-x>aig ``<br>**cua** — select: `` <C-X>aig ``, insert: `` <C-x>aig `` |
| `inverse_add_mode_abbrev` | Define the word before point as a mode-local abbrev, prompting for its expansion (emacs inverse-add-mode-abbrev, C-x a i l) | **spacemacs** — normal: `` <C-x>ail ``, select: `` <C-x>ail ``, insert: `` <C-x>ail ``<br>**emacs** — insert: `` <C-x>ail ``<br>**cua** — select: `` <C-X>ail ``, insert: `` <C-x>ail `` |
| `toggle_abbrev_mode` | Toggle abbrev-mode: auto-expand abbrevs when typing a word separator (emacs abbrev-mode) |  |
| `wdired_finish_edit` | Apply the file renames edited in a wdired buffer (emacs wdired-finish-edit) |  |
| `timeclock_in` | Clock in to a project, prompting for its name (emacs timeclock-in) |  |
| `timeclock_out` | Clock out (emacs timeclock-out) |  |
| `timeclock_change` | Clock out of the current project and into another (emacs timeclock-change) |  |
| `timeclock_workday_remaining` | Report the time left in the workday (emacs timeclock-workday-remaining) |  |
| `timeclock_when_to_leave` | Report how long until the workday is complete (emacs timeclock-when-to-leave) |  |
| `timeclock_reread_log` | Reload the timelog and report the clock state (emacs timeclock-reread-log) |  |
| `timeclock_mode_line_display` | Show today's worked/remaining time and clock state (emacs timeclock-mode-line-display) |  |
| `expand_abbrev` | Expand the abbrev before point (emacs C-x ') | **spacemacs** — normal: `` <A-'> ``, `` <C-x>' ``, select: `` <C-x>' ``, insert: `` <C-]> ``, `` <C-x>' ``<br>**vim** — insert: `` <C-]> ``<br>**emacs** — insert: `` <C-x>' ``<br>**cua** — select: `` <C-X>' ``, insert: `` <C-x>' `` |
| `abbrev_prefix_mark` | Mark point as an abbrev prefix boundary; insert a hyphen the next expand-abbrev removes (emacs abbrev-prefix-mark, M-') | **spacemacs** — normal: `` <space>xem ``, select: `` <space>xem `` |
| `unexpand_abbrev` | Undo the last abbrev expansion, restoring the original abbrev text (emacs unexpand-abbrev) | **spacemacs** — normal: `` <space>xeu ``, select: `` <space>xeu `` |
| `insert_abbrevs` | Insert a description of every defined abbrev at point (emacs insert-abbrevs) |  |
| `define_abbrevs` | Define abbrevs from the buffer text after point (emacs define-abbrevs) |  |
| `paste_clipboard_after` | Paste clipboard after selections | **helix** — normal: `` <space>p ``, select: `` <space>p `` |
| `paste_clipboard_before` | Paste clipboard before selections | **helix** — normal: `` <space>P ``, select: `` <space>P `` |
| `paste_primary_clipboard_after` | Paste primary clipboard after selections |  |
| `paste_primary_clipboard_before` | Paste primary clipboard before selections |  |
| `indent` | Indent selection | **spacemacs** — normal: `` == ``, `` <A-C-\> ``, `` <space>xac ``, `` <space>x<tab> ``, select: `` <space>xac ``, `` <space>x<tab> ``, insert: `` <C-t> ``<br>**vim** — normal: `` == ``, insert: `` <C-t> ``<br>**helix** — normal: `` <gt> ``, select: `` <gt> ``<br>**emacs, cua** — insert: `` <A-C-\> `` |
| `unindent` | Unindent selection | **helix** — normal: `` <lt> ``, select: `` <lt> `` |
| `format_selections` | Format selection | **spacemacs** — normal: `` <space>j+ ``, `` <space>j= ``, `` <space>lf ``, select: `` <space>j+ ``, `` <space>j= ``, `` <space>lf ``<br>**helix** — normal: `` = ``, select: `` = `` |
| `reflow_selections` | vim gq: reflow selection to text-width | **spacemacs** — normal: `` <A-q> ``<br>**emacs, cua** — normal: `` <A-q> ``, insert: `` <A-q> `` |
| `reflow_selections_keep_cursor` | vim gw: reflow to text-width, keep cursor |  |
| `reflow_mark_cursor` | Record the cursor for the vim gw that follows |  |
| `join_selections` | Join lines inside selection | **spacemacs** — normal: `` <A-^> ``, `` <space>kJ ``, select: `` <space>kJ ``<br>**helix** — normal: `` J ``, select: `` J ``<br>**emacs, cua** — insert: `` <A-^> `` |
| `join_selections_space` | Join lines inside selection and select spaces | **helix** — normal: `` <A-J> ``, select: `` <A-J> `` |
| `join_lines_vim` | Join line(s) with a space, cursor at join (vim J) | **spacemacs, vim** — normal: `` J `` |
| `join_lines_vim_nospace` | Join line(s) without a space (vim gJ) |  |
| `keep_selections` | Keep selections matching regex | **helix** — normal: `` K ``, select: `` K `` |
| `remove_selections` | Remove selections matching regex | **helix** — normal: `` <A-K> ``, select: `` <A-K> `` |
| `align_selections` | Align selections in column | **spacemacs** — normal: `` <space>xaa ``, select: `` <space>xaa ``<br>**helix** — normal: `` & ``, select: `` & `` |
| `keep_primary_selection` | Keep primary selection | **helix** — normal: `` , ``, select: `` , `` |
| `remove_primary_selection` | Remove primary selection | **helix** — normal: `` <A-,> ``, select: `` <A-,> `` |
| `completion` | Invoke completion popup | **spacemacs** — normal: `` <A-C-i> ``, `` <A-tab> ``, `` <C-c>,l ``, `` <C-c>,<space> ``, select: `` <C-c>,l ``, `` <C-c>,<space> ``, insert: `` <A-/> ``, `` <C-n> ``, `` <C-p> ``, `` <C-c>,l ``, `` <C-c>,<space> ``<br>**vim** — insert: `` <C-n> ``, `` <C-p> ``, `` <C-x><C-i> ``, `` <C-x><C-n> ``, `` <C-x><C-p> ``<br>**helix** — insert: `` <C-x> `` |
| `hover` | Show docs for item under cursor | **spacemacs** — normal: `` <C-h>. ``, `` <space>lk ``, `` <space>hda ``, `` <space>hdf ``, `` <space>hdv ``, `` <space>mhh ``, select: `` K ``, `` <C-h>. ``, `` <space>lk ``, `` <space>hda ``, `` <space>hdf ``, `` <space>hdv ``, `` <space>mhh ``, insert: `` <C-h>. ``<br>**vim** — select: `` K ``<br>**helix** — normal: `` <space>k ``, select: `` <space>k `` |
| `keyword_lookup` | vim K: run keywordprg on the word under cursor, else LSP hover | **spacemacs, vim** — normal: `` K `` |
| `goto_first_nonwhitespace_down` | vim _: first non-blank, count-1 lines down | **spacemacs, vim** — normal: `` _ `` |
| `toggle_replace_mode` | vim <Insert>: toggle insert/overtype | **spacemacs, vim** — insert: `` <ins> `` |
| `insert_unindent` | vim i_CTRL-D: unindent, or 0/^ CTRL-D delete all indent | **spacemacs, vim** — insert: `` <C-d> `` |
| `toggle_comments` | Comment/uncomment selections | **spacemacs** — normal: `` <A-;> ``, `` <space>; ``, `` <space>cP ``, `` <space>cc ``, `` <space>cp ``, `` <C-x><C-;> ``, select: `` <space>; ``, `` <space>cP ``, `` <space>cc ``, `` <space>cp ``, `` <C-x><C-;> ``, insert: `` <C-x><C-;> ``<br>**helix** — normal: `` <C-c> ``, `` <space>c ``, select: `` <C-c> ``, `` <space>c ``<br>**emacs** — normal: `` <A-;> ``, insert: `` <A-;> ``, `` <C-x><C-;> ``<br>**cua** — normal: `` <A-;> ``, select: `` <C-X><C-;> ``, insert: `` <A-;> ``, `` <C-x><C-;> `` |
| `toggle_line_comments` | Line comment/uncomment selections | **spacemacs** — normal: `` gcc ``, `` <space>cL ``, `` <space>cl ``, select: `` <space>cL ``, `` <space>cl ``<br>**vim** — normal: `` gcc ``<br>**helix** — normal: `` <space><A-c> ``, select: `` <space><A-c> `` |
| `comment_to_line` | Comment/uncomment from the cursor line to a prompted line (SPC c t) | **spacemacs** — normal: `` <space>ct ``, select: `` <space>ct `` |
| `invert_comment_to_line` | Invert comments per line from the cursor to a prompted line (SPC c T) | **spacemacs** — normal: `` <space>cT ``, select: `` <space>cT `` |
| `toggle_block_comments` | Block comment/uncomment selections | **spacemacs** — normal: `` <space>cb ``, select: `` <space>cb ``<br>**helix** — normal: `` <space>C ``, select: `` <space>C `` |
| `comment_kill` | Kill the comment on the current line to the kill ring (emacs comment-kill; count kills that many lines) | **spacemacs** — normal: `` <space>cx ``, select: `` <space>cx `` |
| `rotate_selections_forward` | Rotate selections forward | **helix** — normal: `` ) ``, select: `` ) `` |
| `rotate_selections_backward` | Rotate selections backward | **helix** — normal: `` ( ``, select: `` ( `` |
| `rotate_selection_contents_forward` | Rotate selection contents forward | **helix** — normal: `` <A-)> ``, select: `` <A-)> `` |
| `rotate_selection_contents_backward` | Rotate selections contents backward | **helix** — normal: `` <A-(> ``, select: `` <A-(> `` |
| `reverse_selection_contents` | Reverse selections contents |  |
| `expand_selection` | Expand selection to parent syntax node | **spacemacs** — normal: `` <space>v ``, `` <space>kU ``, select: `` <space>v ``, `` <space>kU ``<br>**helix** — normal: `` <A-o> ``, `` <A-up> ``, select: `` <A-o> ``, `` <A-up> `` |
| `shrink_selection` | Shrink selection to previously expanded syntax node | **helix** — normal: `` <A-i> ``, `` <A-down> ``, select: `` <A-i> ``, `` <A-down> `` |
| `wildfire` | Wildfire: select/expand to the closest text object | **spacemacs, vim** — normal: `` <ret> `` |
| `wildfire_shrink` | Wildfire: shrink to the previously selected text object | **spacemacs, vim** — normal: `` <backspace> `` |
| `select_next_sibling` | Select next sibling in the syntax tree | **spacemacs** — normal: `` <space>kL ``, `` <space>kl ``, select: `` <space>kL ``, `` <space>kl ``<br>**helix** — normal: `` <A-n> ``, `` <A-right> ``, select: `` <A-n> ``, `` <A-right> `` |
| `select_prev_sibling` | Select previous sibling the in syntax tree | **spacemacs** — normal: `` <space>kH ``, `` <space>kh ``, select: `` <space>kH ``, `` <space>kh ``<br>**helix** — normal: `` <A-p> ``, `` <A-left> ``, select: `` <A-p> ``, `` <A-left> `` |
| `select_all_siblings` | Select all siblings of the current node | **helix** — normal: `` <A-a> ``, select: `` <A-a> `` |
| `select_all_children` | Select all children of the current node | **helix** — normal: `` <A-I> ``, `` <S-A-down> ``, select: `` <A-I> ``, `` <S-A-down> `` |
| `jump_forward` | Jump forward on jumplist | **spacemacs, vim** — normal: `` <C-i> ``, `` <tab> ``<br>**helix** — normal: `` <C-i> ``, `` <tab> ``, select: `` <C-i> ``, `` <tab> ``<br>**emacs, cua** — insert: `` <A-C-,> `` |
| `jump_backward` | Jump backward on jumplist | **spacemacs** — normal: `` <C-o> ``, `` <space>jb ``, `` <space>s` ``, select: `` <space>jb ``, `` <space>s` ``<br>**vim** — normal: `` <C-o> ``<br>**helix** — normal: `` <C-o> ``, select: `` <C-o> ``<br>**emacs, cua** — normal: `` <A-,> ``, insert: `` <A-,> `` |
| `save_selection` | Save current selection to jumplist | **helix** — normal: `` <C-s> ``, select: `` <C-s> `` |
| `jump_view_right` | Jump to right split | **spacemacs** — normal: `` <C-w>l ``, `` <C-w>.l ``, `` <C-w>[l ``, `` <C-w>{l ``, `` <space>wl ``, `` <C-w><C-l> ``, `` <space>w.l ``, `` <space>w[l ``, `` <space>w{l ``, `` <C-w><right> ``, `` <space>w<C-l> ``, `` <space>w<right> ``, select: `` <space>wl ``, `` <space>w.l ``, `` <space>w[l ``, `` <space>w{l ``, `` <space>w<C-l> ``, `` <space>w<right> ``<br>**vim** — normal: `` <C-w>l ``, `` <C-w>.l ``, `` <C-w>[l ``, `` <C-w>{l ``, `` <C-w><C-l> ``, `` <C-w><right> ``<br>**helix** — normal: `` <C-w>l ``, `` <space>wl ``, `` <C-w><C-l> ``, `` <C-w><right> ``, `` <space>w<C-l> ``, `` <space>w<right> ``, select: `` <C-w>l ``, `` <space>wl ``, `` <C-w><C-l> ``, `` <C-w><right> ``, `` <space>w<C-l> ``, `` <space>w<right> `` |
| `jump_view_left` | Jump to left split | **spacemacs** — normal: `` <C-w>h ``, `` <C-w>.h ``, `` <C-w>[h ``, `` <C-w>{h ``, `` <space>wh ``, `` <C-w><C-h> ``, `` <space>w.h ``, `` <space>w[h ``, `` <space>w{h ``, `` <C-w><left> ``, `` <space>w<C-h> ``, `` <space>w<left> ``, select: `` <space>wh ``, `` <space>w.h ``, `` <space>w[h ``, `` <space>w{h ``, `` <space>w<C-h> ``, `` <space>w<left> ``<br>**vim** — normal: `` <C-w>h ``, `` <C-w>.h ``, `` <C-w>[h ``, `` <C-w>{h ``, `` <C-w><C-h> ``, `` <C-w><left> ``<br>**helix** — normal: `` <C-w>h ``, `` <space>wh ``, `` <C-w><C-h> ``, `` <C-w><left> ``, `` <space>w<C-h> ``, `` <space>w<left> ``, select: `` <C-w>h ``, `` <space>wh ``, `` <C-w><C-h> ``, `` <C-w><left> ``, `` <space>w<C-h> ``, `` <space>w<left> `` |
| `jump_view_up` | Jump to split above | **spacemacs** — normal: `` <C-w>k ``, `` <C-w>.k ``, `` <C-w>[k ``, `` <C-w>{k ``, `` <C-w><up> ``, `` <space>wk ``, `` <C-w><C-k> ``, `` <C-w><C-t> ``, `` <space>w.k ``, `` <space>w[k ``, `` <space>w{k ``, `` <space>w<up> ``, `` <space>w<C-k> ``, `` <space>w<C-t> ``, select: `` <space>wk ``, `` <space>w.k ``, `` <space>w[k ``, `` <space>w{k ``, `` <space>w<up> ``, `` <space>w<C-k> ``, `` <space>w<C-t> ``<br>**vim** — normal: `` <C-w>k ``, `` <C-w>.k ``, `` <C-w>[k ``, `` <C-w>{k ``, `` <C-w><up> ``, `` <C-w><C-k> ``, `` <C-w><C-t> ``<br>**helix** — normal: `` <C-w>k ``, `` <C-w><up> ``, `` <space>wk ``, `` <C-w><C-k> ``, `` <space>w<up> ``, `` <space>w<C-k> ``, select: `` <C-w>k ``, `` <C-w><up> ``, `` <space>wk ``, `` <C-w><C-k> ``, `` <space>w<up> ``, `` <space>w<C-k> `` |
| `jump_view_down` | Jump to split below | **spacemacs** — normal: `` <C-w>b ``, `` <C-w>j ``, `` <C-w>.j ``, `` <C-w>[j ``, `` <C-w>{j ``, `` <space>wb ``, `` <space>wj ``, `` <C-w><C-b> ``, `` <C-w><C-j> ``, `` <space>w.j ``, `` <space>w[j ``, `` <space>w{j ``, `` <C-w><down> ``, `` <space>w<C-b> ``, `` <space>w<C-j> ``, `` <space>w<down> ``, select: `` <space>wb ``, `` <space>wj ``, `` <space>w.j ``, `` <space>w[j ``, `` <space>w{j ``, `` <space>w<C-b> ``, `` <space>w<C-j> ``, `` <space>w<down> ``<br>**vim** — normal: `` <C-w>b ``, `` <C-w>j ``, `` <C-w>.j ``, `` <C-w>[j ``, `` <C-w>{j ``, `` <C-w><C-b> ``, `` <C-w><C-j> ``, `` <C-w><down> ``<br>**helix** — normal: `` <C-w>j ``, `` <space>wj ``, `` <C-w><C-j> ``, `` <C-w><down> ``, `` <space>w<C-j> ``, `` <space>w<down> ``, select: `` <C-w>j ``, `` <space>wj ``, `` <C-w><C-j> ``, `` <C-w><down> ``, `` <space>w<C-j> ``, `` <space>w<down> `` |
| `swap_view_right` | Swap with right split | **spacemacs** — normal: `` <C-w>L ``, `` <C-w>.L ``, `` <C-w>[L ``, `` <C-w>{L ``, `` <space>wL ``, `` <space>w.L ``, `` <space>w[L ``, `` <space>w{L ``, select: `` <space>wL ``, `` <space>w.L ``, `` <space>w[L ``, `` <space>w{L ``<br>**vim** — normal: `` <C-w>L ``, `` <C-w>.L ``, `` <C-w>[L ``, `` <C-w>{L ``<br>**helix** — normal: `` <C-w>L ``, `` <space>wL ``, select: `` <C-w>L ``, `` <space>wL `` |
| `swap_view_left` | Swap with left split | **spacemacs** — normal: `` <C-w>H ``, `` <C-w>.H ``, `` <C-w>[H ``, `` <C-w>{H ``, `` <space>wH ``, `` <space>w.H ``, `` <space>w[H ``, `` <space>w{H ``, select: `` <space>wH ``, `` <space>w.H ``, `` <space>w[H ``, `` <space>w{H ``<br>**vim** — normal: `` <C-w>H ``, `` <C-w>.H ``, `` <C-w>[H ``, `` <C-w>{H ``<br>**helix** — normal: `` <C-w>H ``, `` <space>wH ``, select: `` <C-w>H ``, `` <space>wH `` |
| `swap_view_up` | Swap with split above | **spacemacs** — normal: `` <C-w>K ``, `` <C-w>.K ``, `` <C-w>[K ``, `` <C-w>{K ``, `` <space>wK ``, `` <space>w.K ``, `` <space>w[K ``, `` <space>w{K ``, select: `` <space>wK ``, `` <space>w.K ``, `` <space>w[K ``, `` <space>w{K ``<br>**vim** — normal: `` <C-w>K ``, `` <C-w>.K ``, `` <C-w>[K ``, `` <C-w>{K ``<br>**helix** — normal: `` <C-w>K ``, `` <space>wK ``, select: `` <C-w>K ``, `` <space>wK `` |
| `swap_view_down` | Swap with split below | **spacemacs** — normal: `` <C-w>J ``, `` <C-w>.J ``, `` <C-w>[J ``, `` <C-w>{J ``, `` <space>wJ ``, `` <space>w.J ``, `` <space>w[J ``, `` <space>w{J ``, select: `` <space>wJ ``, `` <space>w.J ``, `` <space>w[J ``, `` <space>w{J ``<br>**vim** — normal: `` <C-w>J ``, `` <C-w>.J ``, `` <C-w>[J ``, `` <C-w>{J ``<br>**helix** — normal: `` <C-w>J ``, `` <space>wJ ``, select: `` <C-w>J ``, `` <space>wJ `` |
| `tmux_navigate_left` | Move left, on out into tmux at the edge (spacemacs tmux layer) |  |
| `tmux_navigate_down` | Move down, on out into tmux at the edge (spacemacs tmux layer) |  |
| `tmux_navigate_up` | Move up, on out into tmux at the edge (spacemacs tmux layer) |  |
| `tmux_navigate_right` | Move right, on out into tmux at the edge (spacemacs tmux layer) |  |
| `windmove_delete_left` | Delete the window to the left (emacs windmove-delete-left) |  |
| `windmove_delete_right` | Delete the window to the right (emacs windmove-delete-right) |  |
| `windmove_delete_up` | Delete the window above (emacs windmove-delete-up) |  |
| `windmove_delete_down` | Delete the window below (emacs windmove-delete-down) |  |
| `windmove_default_keybindings` | Bind S-<arrow> to move between windows (emacs windmove-default-keybindings) |  |
| `windmove_swap_states_default_keybindings` | Bind C-S-<arrow> to swap windows (emacs windmove-swap-states-default-keybindings) |  |
| `windmove_delete_default_keybindings` | Bind C-x S-<arrow> to delete a neighbouring window (emacs windmove-delete-default-keybindings) |  |
| `windmove_display_left` | Display the next buffer in the window to the left (emacs windmove-display-left) |  |
| `windmove_display_right` | Display the next buffer in the window to the right (emacs windmove-display-right) |  |
| `windmove_display_up` | Display the next buffer in the window above (emacs windmove-display-up) |  |
| `windmove_display_down` | Display the next buffer in the window below (emacs windmove-display-down) |  |
| `windmove_display_same_window` | Display the next buffer in this window (emacs windmove-display-same-window) |  |
| `windmove_display_new_frame` | Display the next buffer in a new frame (emacs windmove-display-new-frame) |  |
| `windmove_display_new_tab` | Display the next buffer in a new tab (emacs windmove-display-new-tab) |  |
| `windmove_display_default_keybindings` | Bind M-S-<arrow> to display the next buffer in that direction (emacs windmove-display-default-keybindings) |  |
| `transpose_view` | Transpose splits | **spacemacs** — normal: `` <C-w>M ``, `` <C-w>x ``, `` <space>wM ``, `` <space>wx ``, `` <C-w><C-x> ``, `` <space>w<C-x> ``, select: `` <space>wM ``, `` <space>wx ``, `` <space>w<C-x> ``<br>**vim** — normal: `` <C-w>M ``, `` <C-w>x ``, `` <C-w><C-x> ``<br>**helix** — normal: `` <C-w>t ``, `` <space>wt ``, `` <C-w><C-t> ``, `` <space>w<C-t> ``, select: `` <C-w>t ``, `` <space>wt ``, `` <C-w><C-t> ``, `` <space>w<C-t> `` |
| `quickfix_next` | Quickfix: jump to next entry (:cnext) | **spacemacs, vim** — normal: `` ]q `` |
| `quickfix_prev` | Quickfix: jump to previous entry (:cprev) | **spacemacs, vim** — normal: `` [q `` |
| `quickfix_first` | Quickfix: jump to first entry (:cfirst) |  |
| `quickfix_last` | Quickfix: jump to last entry (:clast) |  |
| `quickfix_open` | Quickfix: open the quickfix list window (:copen) |  |
| `loclist_next` | Location list: jump to next entry (:lnext) | **spacemacs, vim** — normal: `` ]l `` |
| `loclist_prev` | Location list: jump to previous entry (:lprev) | **spacemacs, vim** — normal: `` [l `` |
| `loclist_first` | Location list: jump to first entry (:lfirst) |  |
| `loclist_last` | Location list: jump to last entry (:llast) |  |
| `loclist_open` | Location list: open the location list window (:lopen) |  |
| `goto_next_tabpage` | Go to the next tabpage (gt / :tabnext) | **spacemacs** — normal: `` gt ``, `` <C-w>gt ``, `` <C-x>to ``, `` <space>wgt ``, select: `` <C-x>to ``, `` <space>wgt ``, insert: `` <C-x>to ``<br>**vim** — normal: `` gt ``, `` <C-w>gt `` |
| `goto_previous_tabpage` | Go to the previous tabpage (gT / :tabprevious) | **spacemacs** — normal: `` gT ``, `` <C-w>gT ``, `` <S-C-tab> ``, `` <space>wgT ``, select: `` <space>wgT ``<br>**vim** — normal: `` gT ``, `` <C-w>gT `` |
| `new_tab` | Open a new tabpage (:tabnew) | **spacemacs** — normal: `` <C-x>t2 ``, select: `` <C-x>t2 ``, insert: `` <C-x>t2 `` |
| `close_tab` | Close the current tabpage (:tabclose) | **spacemacs** — normal: `` <C-x>t0 ``, select: `` <C-x>t0 ``, insert: `` <C-x>t0 `` |
| `tab_only` | Close all other tabpages (:tabonly) | **spacemacs** — normal: `` <C-x>t1 ``, select: `` <C-x>t1 ``, insert: `` <C-x>t1 `` |
| `window_to_new_tab` | Move the current window to a new tabpage (vim CTRL-W T) | **spacemacs, vim** — normal: `` <C-w>T `` |
| `goto_first_tabpage` | Go to the first tabpage (:tabfirst) |  |
| `goto_last_tabpage` | Go to the last tabpage (:tablast) |  |
| `tab_select` | Go to the [count]-th tab (emacs tab-select) |  |
| `tab_recent` | Switch to the most recently visited tab (emacs tab-recent) |  |
| `tab_bar_mode` | Toggle the tab bar (emacs tab-bar-mode) |  |
| `tab_rename` | Name the current tab (emacs tab-rename) | **spacemacs** — normal: `` <C-x>tr ``, select: `` <C-x>tr ``, insert: `` <C-x>tr `` |
| `tab_switch` | Switch to a tab by name or number (emacs tab-switch) | **spacemacs** — normal: `` <C-x>t<ret> ``, select: `` <C-x>t<ret> ``, insert: `` <C-x>t<ret> `` |
| `tab_undo` | Reopen the most recently closed tab (emacs tab-undo) |  |
| `tab_bar_history_mode` | Toggle tab-visit history recording (emacs tab-bar-history-mode) |  |
| `tab_bar_history_back` | Return to the previously visited tab (emacs tab-bar-history-back) |  |
| `tab_bar_history_forward` | Re-visit a tab left via history-back (emacs tab-bar-history-forward) |  |
| `forward_list` | Move forward over a balanced () group (emacs forward-list, C-M-n) | **spacemacs** — normal: `` <A-C-n> `` |
| `backward_list` | Move backward over a balanced () group (emacs backward-list, C-M-p) | **spacemacs** — normal: `` <A-C-p> `` |
| `down_list` | Descend into the next list (emacs down-list, C-M-d) | **spacemacs** — normal: `` <A-C-d> `` |
| `up_list` | Move forward out of the enclosing list (emacs up-list) |  |
| `backward_up_list` | Move backward out of the enclosing list (emacs backward-up-list, C-M-u) | **spacemacs** — normal: `` <A-C-u> `` |
| `kill_sexp` | Kill the s-expression after point (emacs kill-sexp, C-M-k) | **spacemacs** — normal: `` <A-C-k> `` |
| `mark_sexp` | Set the region over the following s-expression (emacs mark-sexp, C-M-SPC) | **spacemacs** — normal: `` <A-C-@> ``, `` <A-C-space> `` |
| `forward_sexp` | Move forward over the next s-expression (emacs forward-sexp, C-M-f) | **spacemacs** — normal: `` <A-C-f> `` |
| `backward_sexp` | Move backward over the previous s-expression (emacs backward-sexp, C-M-b) | **spacemacs** — normal: `` <A-C-b> `` |
| `prog_indent_sexp` | Re-indent the s-expression after point, or the enclosing defun with a prefix (emacs prog-indent-sexp, C-M-q; here = s) | **spacemacs** — normal: `` =s ``, `` <A-C-q> ``<br>**vim** — normal: `` =s `` |
| `copy_region_as_kill` | Copy the region to the kill ring without deleting (emacs copy-region-as-kill, M-w) |  |
| `copy_as_format` | Copy the region or line to the kill ring, wrapped for the default format (spacemacs copy-as-format) |  |
| `copy_as_format_asciidoc` | Copy the region or line as an AsciiDoc source block (copy-as-format-asciidoc) |  |
| `copy_as_format_bitbucket` | Copy the region or line as Bitbucket markup (copy-as-format-bitbucket) |  |
| `copy_as_format_disqus` | Copy the region or line as Disqus markup (copy-as-format-disqus) |  |
| `copy_as_format_github` | Copy the region or line as a GitHub fenced block (copy-as-format-github) |  |
| `copy_as_format_gitlab` | Copy the region or line as GitLab markup (copy-as-format-gitlab) |  |
| `copy_as_format_hipchat` | Copy the region or line as a HipChat /code message (copy-as-format-hipchat) |  |
| `copy_as_format_html` | Copy the region or line as HTML pre/code (copy-as-format-html) |  |
| `copy_as_format_jira` | Copy the region or line as a Jira {code} block (copy-as-format-jira) |  |
| `copy_as_format_markdown` | Copy the region or line as indented Markdown code (copy-as-format-markdown) |  |
| `copy_as_format_mediawiki` | Copy the region or line as MediaWiki syntaxhighlight (copy-as-format-mediawiki) |  |
| `copy_as_format_org_mode` | Copy the region or line as an Org src block (copy-as-format-org-mode) |  |
| `copy_as_format_pod` | Copy the region or line as POD (copy-as-format-pod) |  |
| `copy_as_format_rst` | Copy the region or line as a reStructuredText code directive (copy-as-format-rst) |  |
| `copy_as_format_telegram` | Copy the region or line as Telegram markup (copy-as-format-telegram) |  |
| `copy_as_format_slack` | Copy the region or line as Slack markup (copy-as-format-slack) |  |
| `copy_as_format_whatsapp` | Copy the region or line as WhatsApp markup (copy-as-format-whatsapp) |  |
| `mark_word` | Set the region over the next word (emacs mark-word, M-@) | **spacemacs** — normal: `` <A-@> `` |
| `mark_paragraph` | Select the paragraph around point (emacs mark-paragraph, M-h) | **spacemacs** — normal: `` <A-h> ``<br>**emacs, cua** — normal: `` <A-h> ``, insert: `` <A-h> `` |
| `mark_defun` | Select the function/defun around point (emacs mark-defun, C-M-h) | **spacemacs** — normal: `` <A-C-h> `` |
| `kill_sentence` | Kill from point to end of sentence (emacs kill-sentence, M-k) | **spacemacs** — normal: `` <A-k> `` |
| `backward_kill_sentence` | Kill from start of sentence to point (emacs backward-kill-sentence, C-x DEL) | **spacemacs** — normal: `` <C-x><backspace> ``, select: `` <C-x><backspace> ``, insert: `` <C-x><backspace> `` |
| `append_next_kill` | Make the next kill append to the last kill-ring entry (emacs append-next-kill, C-M-w) | **spacemacs** — normal: `` <A-C-w> ``, `` <space>ra ``, select: `` <space>ra `` |
| `forward_page` | Move to the next form-feed page (emacs forward-page, C-x ]) | **spacemacs** — normal: `` <C-x>] ``, select: `` <C-x>] ``, insert: `` <C-x>] `` |
| `backward_page` | Move to the previous form-feed page (emacs backward-page, C-x [) | **spacemacs** — normal: `` <C-x>[ ``, select: `` <C-x>[ ``, insert: `` <C-x>[ `` |
| `mark_page` | Select the current form-feed page (emacs mark-page, C-x C-p) | **spacemacs** — normal: `` <C-x><C-p> ``, select: `` <C-x><C-p> ``, insert: `` <C-x><C-p> `` |
| `move_to_opposite_group` | Move the current editor to the opposite split group (JetBrains) |  |
| `rotate_view` | Goto next window | **spacemacs** — normal: `` ]w ``, `` <C-w>p ``, `` <C-w>r ``, `` <C-w>w ``, `` <C-x>o ``, `` <C-w>.o ``, `` <C-w>.r ``, `` <C-w>.w ``, `` <C-w>[o ``, `` <C-w>[r ``, `` <C-w>[w ``, `` <C-w>{o ``, `` <C-w>{r ``, `` <C-w>{w ``, `` <space>wp ``, `` <space>wr ``, `` <space>ww ``, `` <C-w><C-p> ``, `` <C-w><C-r> ``, `` <C-w><C-w> ``, `` <C-w><tab> ``, `` <space>b.o ``, `` <space>w.o ``, `` <space>w.r ``, `` <space>w.w ``, `` <space>w[o ``, `` <space>w[r ``, `` <space>w[w ``, `` <space>w{o ``, `` <space>w{r ``, `` <space>w{w ``, `` <space>w<C-p> ``, `` <space>w<C-r> ``, `` <space>w<C-w> ``, `` <space>w<tab> ``, select: `` <C-x>o ``, `` <space>wp ``, `` <space>wr ``, `` <space>ww ``, `` <space>b.o ``, `` <space>w.o ``, `` <space>w.r ``, `` <space>w.w ``, `` <space>w[o ``, `` <space>w[r ``, `` <space>w[w ``, `` <space>w{o ``, `` <space>w{r ``, `` <space>w{w ``, `` <space>w<C-p> ``, `` <space>w<C-r> ``, `` <space>w<C-w> ``, `` <space>w<tab> ``, insert: `` <C-x>o ``<br>**vim** — normal: `` ]w ``, `` <C-w>p ``, `` <C-w>r ``, `` <C-w>w ``, `` <C-w>.o ``, `` <C-w>.r ``, `` <C-w>.w ``, `` <C-w>[o ``, `` <C-w>[r ``, `` <C-w>[w ``, `` <C-w>{o ``, `` <C-w>{r ``, `` <C-w>{w ``, `` <C-w><C-p> ``, `` <C-w><C-r> ``, `` <C-w><C-w> ``, `` <C-w><tab> ``<br>**helix** — normal: `` <C-w>w ``, `` <space>ww ``, `` <C-w><C-w> ``, `` <space>w<C-w> ``, select: `` <C-w>w ``, `` <space>ww ``, `` <C-w><C-w> ``, `` <space>w<C-w> ``<br>**emacs** — normal: `` <C-x>o ``, insert: `` <C-x>o ``<br>**cua** — normal: `` <C-x>o ``, select: `` <C-X>o ``, insert: `` <C-x>o `` |
| `goto_preview_window` | Goto the preview window (vim CTRL-W P) | **spacemacs** — normal: `` <C-w>P ``, `` <space>wP ``, select: `` <space>wP ``<br>**vim** — normal: `` <C-w>P `` |
| `rotate_view_reverse` | Goto previous window | **spacemacs** — normal: `` [t ``, `` [w ``, `` <C-w>R ``, `` <C-w>W ``, `` <C-w>.R ``, `` <C-w>[R ``, `` <C-w>{R ``, `` <space>wR ``, `` <space>wW ``, `` <space>w.R ``, `` <space>w[R ``, `` <space>w{R ``, select: `` <space>wR ``, `` <space>wW ``, `` <space>w.R ``, `` <space>w[R ``, `` <space>w{R ``<br>**vim** — normal: `` [t ``, `` [w ``, `` <C-w>R ``, `` <C-w>W ``, `` <C-w>.R ``, `` <C-w>[R ``, `` <C-w>{R `` |
| `scroll_other_window` | Scroll the other window forward (emacs scroll-other-window, C-M-v) | **spacemacs** — normal: `` <A-C-v> `` |
| `scroll_other_window_down` | Scroll the other window backward (emacs scroll-other-window-down, C-M-S-v) | **spacemacs** — normal: `` <A-C-V> `` |
| `recenter_other_window` | Recenter point in the other window (emacs recenter-other-window, C-M-S-l) | **spacemacs** — normal: `` <C-w>z ``, `` <A-C-L> ``, `` <space>wz ``, `` <C-w><C-z> ``, `` <space>w<C-z> ``, select: `` <space>wz ``, `` <space>w<C-z> ``<br>**vim** — normal: `` <C-w>z ``, `` <C-w><C-z> `` |
| `hsplit` | Horizontal bottom split | **spacemacs** — normal: `` <C-w>S ``, `` <C-w>s ``, `` <C-x>2 ``, `` <C-w>.S ``, `` <C-w>.s ``, `` <C-w>[S ``, `` <C-w>[s ``, `` <C-w>{S ``, `` <C-w>{s ``, `` <space>wS ``, `` <space>ws ``, `` <C-w><C-s> ``, `` <space>w.S ``, `` <space>w.s ``, `` <space>w[S ``, `` <space>w[s ``, `` <space>w{S ``, `` <space>w{s ``, `` <C-w>.<minus> ``, `` <C-w>[<minus> ``, `` <C-w>{<minus> ``, `` <space>w<C-s> ``, `` <space>w.<minus> ``, `` <space>w[<minus> ``, `` <space>w{<minus> ``, select: `` <C-x>2 ``, `` <space>wS ``, `` <space>ws ``, `` <space>w.S ``, `` <space>w.s ``, `` <space>w[S ``, `` <space>w[s ``, `` <space>w{S ``, `` <space>w{s ``, `` <space>w<C-s> ``, `` <space>w.<minus> ``, `` <space>w[<minus> ``, `` <space>w{<minus> ``, insert: `` <C-x>2 ``<br>**vim** — normal: `` <C-w>S ``, `` <C-w>s ``, `` <C-w>.S ``, `` <C-w>.s ``, `` <C-w>[S ``, `` <C-w>[s ``, `` <C-w>{S ``, `` <C-w>{s ``, `` <C-w><C-s> ``, `` <C-w>.<minus> ``, `` <C-w>[<minus> ``, `` <C-w>{<minus> ``<br>**helix** — normal: `` <C-w>s ``, `` <space>ws ``, `` <C-w><C-s> ``, `` <space>w<C-s> ``, select: `` <C-w>s ``, `` <space>ws ``, `` <C-w><C-s> ``, `` <space>w<C-s> ``<br>**emacs** — normal: `` <C-x>2 ``, insert: `` <C-x>2 ``<br>**cua** — normal: `` <C-x>2 ``, select: `` <C-X>2 ``, insert: `` <C-x>2 `` |
| `hsplit_new` | Horizontal bottom split scratch buffer | **spacemacs** — normal: `` <C-w>n ``, `` <space>wn ``, `` <C-w><C-n> ``, `` <space>bNj ``, `` <space>bNk ``, `` <space>w<C-n> ``, select: `` <space>wn ``, `` <space>bNj ``, `` <space>bNk ``, `` <space>w<C-n> ``<br>**vim** — normal: `` <C-w>n ``, `` <C-w><C-n> ``<br>**helix** — normal: `` <C-w>ns ``, `` <space>wns ``, `` <C-w>n<C-s> ``, `` <space>wn<C-s> ``, select: `` <C-w>ns ``, `` <space>wns ``, `` <C-w>n<C-s> ``, `` <space>wn<C-s> `` |
| `vsplit` | Vertical right split | **spacemacs** — normal: `` <C-w>/ ``, `` <C-w>2 ``, `` <C-w>V ``, `` <C-w>v ``, `` <C-x>3 ``, `` <C-w>./ ``, `` <C-w>.V ``, `` <C-w>.v ``, `` <C-w>[/ ``, `` <C-w>[V ``, `` <C-w>[v ``, `` <C-w>{/ ``, `` <C-w>{V ``, `` <C-w>{v ``, `` <space>w/ ``, `` <space>w2 ``, `` <space>wV ``, `` <space>wv ``, `` <C-w><C-v> ``, `` <space>w./ ``, `` <space>w.V ``, `` <space>w.v ``, `` <space>w[/ ``, `` <space>w[V ``, `` <space>w[v ``, `` <space>w{/ ``, `` <space>w{V ``, `` <space>w{v ``, `` <space>w<C-v> ``, `` <space>u<space>w2 ``, select: `` <C-x>3 ``, `` <space>w/ ``, `` <space>w2 ``, `` <space>wV ``, `` <space>wv ``, `` <space>w./ ``, `` <space>w.V ``, `` <space>w.v ``, `` <space>w[/ ``, `` <space>w[V ``, `` <space>w[v ``, `` <space>w{/ ``, `` <space>w{V ``, `` <space>w{v ``, `` <space>w<C-v> ``, `` <space>u<space>w2 ``, insert: `` <C-x>3 ``<br>**vim** — normal: `` <C-w>/ ``, `` <C-w>2 ``, `` <C-w>V ``, `` <C-w>v ``, `` <C-w>./ ``, `` <C-w>.V ``, `` <C-w>.v ``, `` <C-w>[/ ``, `` <C-w>[V ``, `` <C-w>[v ``, `` <C-w>{/ ``, `` <C-w>{V ``, `` <C-w>{v ``, `` <C-w><C-v> ``<br>**helix** — normal: `` <C-w>v ``, `` <space>wv ``, `` <C-w><C-v> ``, `` <space>w<C-v> ``, select: `` <C-w>v ``, `` <space>wv ``, `` <C-w><C-v> ``, `` <space>w<C-v> ``<br>**emacs** — normal: `` <C-x>3 ``, insert: `` <C-x>3 ``<br>**cua** — normal: `` <C-x>3 ``, select: `` <C-X>3 ``, insert: `` <C-x>3 `` |
| `vsplit_new` | Vertical right split scratch buffer | **spacemacs** — normal: `` <space>bNh ``, `` <space>bNl ``, select: `` <space>bNh ``, `` <space>bNl ``<br>**helix** — normal: `` <C-w>nv ``, `` <space>wnv ``, `` <C-w>n<C-v> ``, `` <space>wn<C-v> ``, select: `` <C-w>nv ``, `` <space>wnv ``, `` <C-w>n<C-v> ``, `` <space>wn<C-v> `` |
| `wclose` | Close window | **spacemacs** — normal: `` <C-w>D ``, `` <C-w>c ``, `` <C-w>q ``, `` <C-x>0 ``, `` <C-w>.d ``, `` <C-w>[d ``, `` <C-w>{d ``, `` <space>cd ``, `` <space>wD ``, `` <space>wd ``, `` <space>wq ``, `` <C-w><C-d> ``, `` <C-w><C-q> ``, `` <space>w.d ``, `` <space>w[d ``, `` <space>w{d ``, `` <space>w<C-d> ``, `` <space>w<C-q> ``, `` <space>u<space>wd ``, select: `` <C-x>0 ``, `` <space>cd ``, `` <space>wD ``, `` <space>wd ``, `` <space>wq ``, `` <space>w.d ``, `` <space>w[d ``, `` <space>w{d ``, `` <space>w<C-d> ``, `` <space>w<C-q> ``, `` <space>u<space>wd ``, insert: `` <C-x>0 ``<br>**vim** — normal: `` <C-w>D ``, `` <C-w>c ``, `` <C-w>q ``, `` <C-w>.d ``, `` <C-w>[d ``, `` <C-w>{d ``, `` <C-w><C-d> ``, `` <C-w><C-q> ``<br>**helix** — normal: `` <C-w>q ``, `` <space>wq ``, `` <C-w><C-q> ``, `` <space>w<C-q> ``, select: `` <C-w>q ``, `` <space>wq ``, `` <C-w><C-q> ``, `` <space>w<C-q> ``<br>**emacs** — normal: `` <C-x>0 ``, insert: `` <C-x>0 ``<br>**cua** — normal: `` <C-x>0 ``, select: `` <C-X>0 ``, insert: `` <C-x>0 `` |
| `wonly` | Close windows except current | **spacemacs** — normal: `` <C-w>1 ``, `` <C-w>_ ``, `` <C-w>m ``, `` <C-w>o ``, `` <C-x>1 ``, `` <C-w>.D ``, `` <C-w>._ ``, `` <C-w>.m ``, `` <C-w>[D ``, `` <C-w>[_ ``, `` <C-w>[m ``, `` <C-w>\| ``, `` <C-w>{D ``, `` <C-w>{_ ``, `` <C-w>{m ``, `` <C-w>.\| ``, `` <C-w>[\| ``, `` <C-w>{\| ``, `` <space>w1 ``, `` <space>w_ ``, `` <space>wm ``, `` <space>wo ``, `` <C-w><C-_> ``, `` <C-w><C-o> ``, `` <space>w.D ``, `` <space>w._ ``, `` <space>w.m ``, `` <space>w[D ``, `` <space>w[_ ``, `` <space>w[m ``, `` <space>w\| ``, `` <space>w{D ``, `` <space>w{_ ``, `` <space>w{m ``, `` <space>w.\| ``, `` <space>w[\| ``, `` <space>w{\| ``, `` <space>w<C-o> ``, `` <space>u<space>w1 ``, select: `` <C-x>1 ``, `` <space>w1 ``, `` <space>w_ ``, `` <space>wm ``, `` <space>wo ``, `` <space>w.D ``, `` <space>w._ ``, `` <space>w.m ``, `` <space>w[D ``, `` <space>w[_ ``, `` <space>w[m ``, `` <space>w\| ``, `` <space>w{D ``, `` <space>w{_ ``, `` <space>w{m ``, `` <space>w.\| ``, `` <space>w[\| ``, `` <space>w{\| ``, `` <space>w<C-o> ``, `` <space>u<space>w1 ``, insert: `` <C-x>1 ``<br>**vim** — normal: `` <C-w>1 ``, `` <C-w>_ ``, `` <C-w>m ``, `` <C-w>o ``, `` <C-w>.D ``, `` <C-w>._ ``, `` <C-w>.m ``, `` <C-w>[D ``, `` <C-w>[_ ``, `` <C-w>[m ``, `` <C-w>\| ``, `` <C-w>{D ``, `` <C-w>{_ ``, `` <C-w>{m ``, `` <C-w>.\| ``, `` <C-w>[\| ``, `` <C-w>{\| ``, `` <C-w><C-_> ``, `` <C-w><C-o> ``<br>**helix** — normal: `` <C-w>o ``, `` <space>wo ``, `` <C-w><C-o> ``, `` <space>w<C-o> ``, select: `` <C-w>o ``, `` <space>wo ``, `` <C-w><C-o> ``, `` <space>w<C-o> ``<br>**emacs** — normal: `` <C-x>1 ``, insert: `` <C-x>1 ``<br>**cua** — normal: `` <C-x>1 ``, select: `` <C-X>1 ``, insert: `` <C-x>1 `` |
| `select_register` | Select register | **spacemacs, vim, helix** — normal: `` " ``, select: `` " `` |
| `insert_register` | Insert register | **spacemacs, vim, helix** — insert: `` <C-r> `` |
| `view_register` | Show a register's contents (emacs view-register, C-x r v) |  |
| `not_modified` | Mark the buffer unmodified without saving (emacs not-modified, M-~) | **spacemacs** — normal: `` <A-~> `` |
| `insert_char_by_code` | Insert a character by Unicode code point (emacs insert-char, C-x 8 RET) |  |
| `backward_delete_char_untabify` | Delete backward, expanding a tab into spaces first (emacs backward-delete-char-untabify) |  |
| `insert_last_inserted_text` | Insert the previously inserted text (vim i_CTRL-A) | **spacemacs, vim** — insert: `` <C-a> `` |
| `insert_command_normal` | Run one Normal-mode command, then return to Insert (vim i_CTRL-O) | **spacemacs, vim** — insert: `` <C-o> `` |
| `insert_last_inserted_and_stop` | Insert previously inserted text and stop insert (vim i_CTRL-@) | **spacemacs, vim** — insert: `` <C-@> `` |
| `copy_between_registers` | Copy between two registers |  |
| `copy_to_register` | Copy the region into a register (emacs copy-to-register, C-x r s) | **spacemacs** — normal: `` <C-x>rs ``, select: `` <C-x>rs ``, insert: `` <C-x>rs `` |
| `append_to_register` | Append the region to a register (emacs append-to-register, C-x r a) |  |
| `prepend_to_register` | Prepend the region to a register (emacs prepend-to-register, C-x r p) |  |
| `align_view_middle` | Align view middle | **helix** — normal: `` Zm ``, `` zm ``, select: `` Zm ``, `` zm `` |
| `align_view_top` | Align view top | **spacemacs, vim** — normal: `` zt ``, select: `` zt ``<br>**helix** — normal: `` Zt ``, `` zt ``, select: `` Zt ``, `` zt `` |
| `align_view_center` | Align view center | **spacemacs** — normal: `` zz ``, `` <C-l> ``, `` <C-w>.z ``, `` <C-w>[z ``, `` <C-w>{z ``, `` <space>b.z ``, `` <space>w.z ``, `` <space>w[z ``, `` <space>wc. ``, `` <space>w{z ``, select: `` zz ``, `` <space>b.z ``, `` <space>w.z ``, `` <space>w[z ``, `` <space>wc. ``, `` <space>w{z ``<br>**vim** — normal: `` zz ``, `` <C-w>.z ``, `` <C-w>[z ``, `` <C-w>{z ``, select: `` zz ``<br>**helix** — normal: `` Zc ``, `` Zz ``, `` zc ``, `` zz ``, select: `` Zc ``, `` Zz ``, `` zc ``, `` zz ``<br>**emacs, cua** — insert: `` <C-l> `` |
| `align_view_bottom` | Align view bottom | **spacemacs, vim** — normal: `` zb ``, select: `` zb ``<br>**helix** — normal: `` Zb ``, `` zb ``, select: `` Zb ``, `` zb `` |
| `scroll_up` | Scroll view up | **spacemacs** — normal: `` <C-y> ``<br>**vim** — normal: `` <C-y> ``, insert: `` <C-x><C-y> ``<br>**helix** — normal: `` Zk ``, `` zk ``, `` Z<up> ``, `` z<up> ``, select: `` Zk ``, `` zk ``, `` Z<up> ``, `` z<up> `` |
| `scroll_down` | Scroll view down | **spacemacs** — normal: `` <C-e> ``<br>**vim** — normal: `` <C-e> ``, insert: `` <C-x><C-e> ``<br>**helix** — normal: `` Zj ``, `` zj ``, `` Z<down> ``, `` z<down> ``, select: `` Zj ``, `` zj ``, `` Z<down> ``, `` z<down> `` |
| `scroll_column_left` | Scroll view left one column (zh) | **spacemacs, vim** — normal: `` zh ``, `` z<left> ``, select: `` zh ``, `` z<left> `` |
| `scroll_column_right` | Scroll view right one column (zl) | **spacemacs, vim** — normal: `` zl ``, `` z<right> ``, select: `` zl ``, `` z<right> `` |
| `scroll_half_column_left` | Scroll view left half a screen (zH) | **spacemacs** — normal: `` zH ``, `` <C-x><gt> ``, select: `` zH ``, `` <C-x><gt> ``, insert: `` <C-x><gt> ``<br>**vim** — normal: `` zH ``, select: `` zH `` |
| `scroll_half_column_right` | Scroll view right half a screen (zL) | **spacemacs** — normal: `` zL ``, `` <C-x><lt> ``, select: `` zL ``, `` <C-x><lt> ``, insert: `` <C-x><lt> ``<br>**vim** — normal: `` zL ``, select: `` zL `` |
| `scroll_cursor_to_left_edge` | Scroll so the cursor sits at the left edge (zs) | **spacemacs, vim** — normal: `` zs ``, select: `` zs `` |
| `scroll_cursor_to_right_edge` | Scroll so the cursor sits at the right edge (ze) | **spacemacs, vim** — normal: `` ze ``, select: `` ze `` |
| `resize_view_wider` | Make current window wider (CTRL-W >) | **spacemacs** — normal: `` <C-x>} ``, `` <C-w>.] ``, `` <C-w>[] ``, `` <C-w>{] ``, `` <C-w><gt> ``, `` <C-w>.<gt> ``, `` <C-w>[<gt> ``, `` <C-w>{<gt> ``, `` <space>w.] ``, `` <space>w[] ``, `` <space>w{] ``, `` <space>w<gt> ``, `` <space>w.<gt> ``, `` <space>w[<gt> ``, `` <space>w{<gt> ``, select: `` <C-x>} ``, `` <space>w.] ``, `` <space>w[] ``, `` <space>w{] ``, `` <space>w<gt> ``, `` <space>w.<gt> ``, `` <space>w[<gt> ``, `` <space>w{<gt> ``, insert: `` <C-x>} ``<br>**vim** — normal: `` <C-w>.] ``, `` <C-w>[] ``, `` <C-w>{] ``, `` <C-w><gt> ``, `` <C-w>.<gt> ``, `` <C-w>[<gt> ``, `` <C-w>{<gt> ``<br>**emacs** — normal: `` <C-x>} ``, insert: `` <C-x>} ``<br>**cua** — normal: `` <C-x>} ``, select: `` <C-X>} ``, insert: `` <C-x>} `` |
| `resize_view_narrower` | Make current window narrower (CTRL-W <) | **spacemacs** — normal: `` <C-x>{ ``, `` <C-w>.[ ``, `` <C-w>[[ ``, `` <C-w>{[ ``, `` <C-w><lt> ``, `` <C-w>.<lt> ``, `` <C-w>[<lt> ``, `` <C-w>{<lt> ``, `` <space>w.[ ``, `` <space>w[[ ``, `` <space>w{[ ``, `` <space>w<lt> ``, `` <space>w.<lt> ``, `` <space>w[<lt> ``, `` <space>w{<lt> ``, select: `` <C-x>{ ``, `` <space>w.[ ``, `` <space>w[[ ``, `` <space>w{[ ``, `` <space>w<lt> ``, `` <space>w.<lt> ``, `` <space>w[<lt> ``, `` <space>w{<lt> ``, insert: `` <C-x>{ ``<br>**vim** — normal: `` <C-w>.[ ``, `` <C-w>[[ ``, `` <C-w>{[ ``, `` <C-w><lt> ``, `` <C-w>.<lt> ``, `` <C-w>[<lt> ``, `` <C-w>{<lt> ``<br>**emacs** — normal: `` <C-x>{ ``, insert: `` <C-x>{ ``<br>**cua** — normal: `` <C-x>{ ``, select: `` <C-X>{ ``, insert: `` <C-x>{ `` |
| `resize_view_taller` | Make current window taller (CTRL-W +) | **spacemacs** — normal: `` <C-w>+ ``, `` <C-x>^ ``, `` <C-w>.} ``, `` <C-w>[} ``, `` <C-w>{} ``, `` <space>w+ ``, `` <space>w.} ``, `` <space>w[} ``, `` <space>w{} ``, select: `` <C-x>^ ``, `` <space>w+ ``, `` <space>w.} ``, `` <space>w[} ``, `` <space>w{} ``, insert: `` <C-x>^ ``<br>**vim** — normal: `` <C-w>+ ``, `` <C-w>.} ``, `` <C-w>[} ``, `` <C-w>{} ``<br>**emacs** — normal: `` <C-x>^ ``, insert: `` <C-x>^ ``<br>**cua** — normal: `` <C-x>^ ``, select: `` <C-X>^ ``, insert: `` <C-x>^ `` |
| `resize_view_shorter` | Make current window shorter (CTRL-W -) | **spacemacs** — normal: `` <C-w>.{ ``, `` <C-w>[{ ``, `` <C-w>{{ ``, `` <space>w.{ ``, `` <space>w[{ ``, `` <space>w{{ ``, `` <C-w><minus> ``, `` <space>w<minus> ``, select: `` <space>w.{ ``, `` <space>w[{ ``, `` <space>w{{ ``, `` <space>w<minus> ``<br>**vim** — normal: `` <C-w>.{ ``, `` <C-w>[{ ``, `` <C-w>{{ ``, `` <C-w><minus> `` |
| `resize_view_equalize` | Make all windows equal size (CTRL-W =) | **spacemacs** — normal: `` <C-w>= ``, `` <C-x>+ ``, `` <C-w>.= ``, `` <C-w>[= ``, `` <C-w>{= ``, `` <space>w= ``, `` <space>w.= ``, `` <space>w[= ``, `` <space>w{= ``, select: `` <C-x>+ ``, `` <space>w= ``, `` <space>w.= ``, `` <space>w[= ``, `` <space>w{= ``, insert: `` <C-x>+ ``<br>**vim** — normal: `` <C-w>= ``, `` <C-w>.= ``, `` <C-w>[= ``, `` <C-w>{= ``<br>**emacs** — normal: `` <C-x>+ ``, insert: `` <C-x>+ ``<br>**cua** — normal: `` <C-x>+ ``, select: `` <C-X>+ ``, insert: `` <C-x>+ `` |
| `golden_ratio_resize` | Resize the focused window to the golden ratio (SPC t g) | **spacemacs** — normal: `` <C-w>.g ``, `` <C-w>[g ``, `` <C-w>{g ``, `` <space>tg ``, `` <space>w.g ``, `` <space>w[g ``, `` <space>w{g ``, select: `` <space>tg ``, `` <space>w.g ``, `` <space>w[g ``, `` <space>w{g ``<br>**vim** — normal: `` <C-w>.g ``, `` <C-w>[g ``, `` <C-w>{g `` |
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
| `insert_digraph` | Insert a digraph by two-character mnemonic (CTRL-K) | **spacemacs, vim** — insert: `` <C-k> `` |
| `insert_uuid_v4` | Insert a random UUIDv4 (SPC i U 4) | **spacemacs** — normal: `` <space>iU4 ``, `` <space>iUU ``, select: `` <space>iU4 ``, `` <space>iUU `` |
| `insert_uuid_v1` | Insert a time-based UUIDv1 (SPC i U 1) | **spacemacs** — normal: `` <space>iU1 ``, select: `` <space>iU1 `` |
| `insert_lorem_sentence` | Insert a lorem-ipsum sentence (SPC i l s) | **spacemacs** — normal: `` <space>ils ``, select: `` <space>ils `` |
| `insert_lorem_paragraph` | Insert a lorem-ipsum paragraph (SPC i l p) | **spacemacs** — normal: `` <space>ilp ``, select: `` <space>ilp `` |
| `insert_lorem_list` | Insert a lorem-ipsum list (SPC i l l) | **spacemacs** — normal: `` <space>ill ``, select: `` <space>ill `` |
| `insert_password_simple` | Insert a simple alphanumeric password (SPC i p 1) | **spacemacs** — normal: `` <space>ip1 ``, select: `` <space>ip1 `` |
| `insert_password_strong` | Insert a stronger password with symbols (SPC i p 2) | **spacemacs** — normal: `` <space>ip2 ``, select: `` <space>ip2 `` |
| `insert_password_paranoid` | Insert a long password for paranoids (SPC i p 3) | **spacemacs** — normal: `` <space>ip3 ``, select: `` <space>ip3 `` |
| `insert_password_numerical` | Insert a numeric password (SPC i p n) | **spacemacs** — normal: `` <space>ipn ``, select: `` <space>ipn `` |
| `insert_password_phonetic` | Insert a phonetically easy password (SPC i p p) | **spacemacs** — normal: `` <space>ipp ``, select: `` <space>ipp `` |
| `symbol_upper_camel` | Change symbol style to UpperCamelCase (SPC x i C) | **spacemacs** — normal: `` <space>xiC ``, select: `` <space>xiC `` |
| `symbol_up_case` | Change symbol style to UP_CASE (SPC x i U) | **spacemacs** — normal: `` <space>xiU ``, select: `` <space>xiU `` |
| `symbol_under_score` | Change symbol style to under_score (SPC x i _) | **spacemacs** — normal: `` <space>xi_ ``, select: `` <space>xi_ `` |
| `symbol_lower_camel` | Change symbol style to camelCase (vim-abolish crc) |  |
| `symbol_kebab` | Change symbol style to kebab-case (vim-abolish cr-) |  |
| `symbol_dot` | Change symbol style to dot.case (vim-abolish cr.) | **spacemacs** — normal: `` <space>xi. ``, select: `` <space>xi. `` |
| `randomize_lines_in_region` | Randomize lines in the selection (SPC x l r) | **spacemacs** — normal: `` <space>xlr ``, select: `` <space>xlr `` |
| `randomize_words_in_region` | Randomize words in the selection (SPC x w r) | **spacemacs** — normal: `` <space>xwr ``, select: `` <space>xwr `` |
| `copy_char_below` | Insert the character below the cursor (i_CTRL-E) | **spacemacs, vim** — insert: `` <C-e> `` |
| `copy_char_above` | Insert the character above the cursor (i_CTRL-Y) | **spacemacs, vim** — insert: `` <C-y> `` |
| `toggle_revins` | Toggle 'revins', inserting text right-to-left (vim i_CTRL-_) | **vim** — insert: `` <C-_> `` |
| `page_up_key` | Page up, ending Select mode with 'keymodel' stopsel (vim <PageUp>) | **spacemacs, vim** — normal: `` <pageup> `` |
| `page_down_key` | Page down, ending Select mode with 'keymodel' stopsel (vim <PageDown>) | **spacemacs, vim** — normal: `` <pagedown> `` |
| `home_key` | Go to line start, ending Select mode with 'keymodel' stopsel (vim <Home>) | **spacemacs, vim** — normal: `` <home> `` |
| `end_key` | Go to line end, ending Select mode with 'keymodel' stopsel (vim <End>) | **spacemacs, vim** — normal: `` <end> `` |
| `shift_home_key` | Select to line start with 'keymodel' startsel, else go there (vim <S-Home>) | **spacemacs, vim** — normal: `` <S-home> `` |
| `shift_end_key` | Select to line end with 'keymodel' startsel, else go there (vim <S-End>) | **spacemacs, vim** — normal: `` <S-end> `` |
| `left_key` | Move left, ending Select mode with 'keymodel' stopsel (vim <Left>) | **spacemacs, vim** — normal: `` <left> `` |
| `right_key` | Move right, ending Select mode with 'keymodel' stopsel (vim <Right>) | **spacemacs, vim** — normal: `` <right> `` |
| `up_key` | Move up, ending Select mode with 'keymodel' stopsel (vim <Up>) | **spacemacs, vim** — normal: `` <up> `` |
| `down_key` | Move down, ending Select mode with 'keymodel' stopsel (vim <Down>) | **spacemacs, vim** — normal: `` <down> `` |
| `shift_right_key` | Select a char right with 'keymodel' startsel, else move a word right (vim <S-Right>) | **spacemacs, vim** — normal: `` <S-right> `` |
| `shift_left_key` | Select a char left with 'keymodel' startsel, else move a word left (vim <S-Left>) | **spacemacs, vim** — normal: `` <S-left> `` |
| `shift_down_key` | Select a line down with 'keymodel' startsel, else page down (vim <S-Down>) | **spacemacs, vim** — normal: `` <S-down> `` |
| `shift_up_key` | Select a line up with 'keymodel' startsel, else page up (vim <S-Up>) | **spacemacs, vim** — normal: `` <S-up> `` |
| `shift_page_up_key` | Extend a page up with 'keymodel' startsel, else page up (vim <S-PageUp>) | **spacemacs, vim** — normal: `` <S-pageup> `` |
| `shift_page_down_key` | Extend a page down with 'keymodel' startsel, else page down (vim <S-PageDown>) | **spacemacs, vim** — normal: `` <S-pagedown> `` |
| `select_left_key` | Select-mode <Left>: stop Select with 'keymodel' stopsel, else extend left |  |
| `select_right_key` | Select-mode <Right>: stop Select with 'keymodel' stopsel, else extend right |  |
| `select_up_key` | Select-mode <Up>: stop Select with 'keymodel' stopsel, else extend up |  |
| `select_down_key` | Select-mode <Down>: stop Select with 'keymodel' stopsel, else extend down |  |
| `select_home_key` | Select-mode <Home>: stop Select with 'keymodel' stopsel, else extend to line start |  |
| `select_end_key` | Select-mode <End>: stop Select with 'keymodel' stopsel, else extend to line end | **spacemacs, vim** — select: `` <end> `` |
| `select_page_up_key` | Select-mode <PageUp>: stop Select with 'keymodel' stopsel, else extend a page up |  |
| `select_page_down_key` | Select-mode <PageDown>: stop Select with 'keymodel' stopsel, else extend a page down |  |
| `file_info` | Show file name and cursor position (CTRL-G) | **spacemacs, vim** — normal: `` <C-g> `` |
| `document_stats` | Show document line/word/char counts (g CTRL-G) | **spacemacs, vim** — normal: `` g<C-g> ``, select: `` g<C-g> `` |
| `git_blame_line` | Show git blame for the current line (g b) | **spacemacs** — normal: `` <C-x>vg ``, `` <space>gM ``, `` <space>gb ``, select: `` <C-x>vg ``, `` <space>gM ``, `` <space>gb ``, insert: `` <C-x>vg `` |
| `toggle_inline_blame` | Toggle GitLens-style inline blame on the current line | **spacemacs** — normal: `` <space>gI ``, select: `` <space>gI `` |
| `toggle_blame_annotate` | Toggle the git-blame annotate gutter column (SPC g B) | **spacemacs** — normal: `` <space>gB ``, select: `` <space>gB `` |
| `git_branch_picker` | Pick a git branch and check it out |  |
| `preferences` | Open the unified Preferences window | **spacemacs** — normal: `` <space>, ``, select: `` <space>, `` |
| `set_selective_display` | Hide lines indented past the prefix-arg column; no arg turns it off (emacs set-selective-display, C-x $) | **spacemacs** — normal: `` <C-x>$ ``, select: `` <C-x>$ ``, insert: `` <C-x>$ `` |
| `global_whitespace_toggle_options` | Toggle rendering of whitespace characters (emacs global-whitespace-toggle-options) |  |
| `global_tab_line_mode` | Toggle the buffer tab line (emacs global-tab-line-mode) |  |
| `global_visual_wrap_prefix_mode` | Toggle soft-wrap with indentation carry-over (emacs global-visual-wrap-prefix-mode) |  |
| `help` | Open the inline Help browser | **spacemacs** — normal: `` <F1> ``, `` <C-h>? ``, `` <C-h>F ``, `` <C-h>I ``, `` <C-h>K ``, `` <C-h>r ``, `` <C-h>4s ``, `` <space>bH ``, `` <space>h? ``, `` <space>hc ``, `` <space>hh ``, `` <space>hk ``, `` <space>hr ``, `` <C-h><C-h> ``, `` <space>hdb ``, `` <space>hdk ``, `` <space>hdx ``, `` <space>h<space> ``, select: `` <C-h>? ``, `` <C-h>F ``, `` <C-h>I ``, `` <C-h>K ``, `` <C-h>r ``, `` <C-h>4s ``, `` <space>bH ``, `` <space>h? ``, `` <space>hc ``, `` <space>hh ``, `` <space>hk ``, `` <space>hr ``, `` <C-h><C-h> ``, `` <space>hdb ``, `` <space>hdk ``, `` <space>hdx ``, `` <space>h<space> ``, insert: `` <C-h>? ``, `` <C-h>F ``, `` <C-h>I ``, `` <C-h>K ``, `` <C-h>r ``, `` <C-h>4s ``, `` <C-h><C-h> ``<br>**vim** — normal: `` <F1> `` |
| `dashboard` | Open the system-stats Dashboard (Preferences) | **spacemacs** — normal: `` <space>bh ``, select: `` <space>bh `` |
| `search_in_files` | Open the project-wide Find in Files panel |  |
| `terminal` | Open an integrated terminal (PTY shell) | **spacemacs** — normal: `` <space>p' ``, select: `` <space>p' `` |
| `comint_shell` | Open a comint line-oriented shell buffer (emacs M-x shell) |  |
| `gud_gdb` | Run gdb in a comint buffer (emacs gud-gdb) |  |
| `gud_up` | Select the stack frame one level up (emacs gud-up) | **spacemacs** — normal: `` <C-c><lt> ``, `` <C-x><C-a><lt> ``, select: `` <C-c><lt> ``, `` <C-x><C-a><lt> ``, insert: `` <C-c><lt> ``, `` <C-x><C-a><lt> `` |
| `gud_down` | Select the stack frame one level down (emacs gud-down) | **spacemacs** — normal: `` <C-c><gt> ``, `` <C-x><C-a><gt> ``, select: `` <C-c><gt> ``, `` <C-x><C-a><gt> ``, insert: `` <C-c><gt> ``, `` <C-x><C-a><gt> `` |
| `gud_stepi` | Step one machine instruction (emacs gud-stepi) | **spacemacs** — normal: `` <C-c><C-i> ``, `` <C-x><C-a><C-i> ``, select: `` <C-c><C-i> ``, `` <C-x><C-a><C-i> ``, insert: `` <C-c><C-i> ``, `` <C-x><C-a><C-i> `` |
| `gud_tbreak` | Set a temporary breakpoint at the current line (emacs gud-tbreak) | **spacemacs** — normal: `` <C-x><C-a><C-t> ``, select: `` <C-x><C-a><C-t> ``, insert: `` <C-x><C-a><C-t> `` |
| `gud_print` | Print the expression at point in the debugger (emacs gud-print) | **spacemacs** — normal: `` <C-c><C-p> ``, `` <C-x><C-a><C-p> ``, select: `` <C-c><C-p> ``, `` <C-x><C-a><C-p> ``, insert: `` <C-c><C-p> ``, `` <C-x><C-a><C-p> `` |
| `gud_watch` | Watch the expression at point in the debugger (emacs gud-watch) | **spacemacs** — normal: `` <C-x><C-a><C-w> ``, select: `` <C-x><C-a><C-w> ``, insert: `` <C-x><C-a><C-w> `` |
| `gud_jump` | Set the debugger execution point to the current line (emacs gud-jump) | **spacemacs** — normal: `` <C-x><C-a><C-j> ``, select: `` <C-x><C-a><C-j> ``, insert: `` <C-x><C-a><C-j> `` |
| `gud_refresh` | Redisplay the debugger buffer (emacs gud-refresh) | **spacemacs** — normal: `` <C-c><C-l> ``, `` <C-x><C-a><C-l> ``, select: `` <C-c><C-l> ``, `` <C-x><C-a><C-l> ``, insert: `` <C-c><C-l> ``, `` <C-x><C-a><C-l> `` |
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
| `comint_dynamic_list_filename` | List the file names completing the one before point (emacs comint-dynamic-list-filename-completions) |  |
| `comint_send_invisible` | Send a non-echoed line (a password) to the process (emacs comint-send-invisible) |  |
| `comint_history_isearch_backward_regexp` | Search the comint input history backward (emacs comint-history-isearch-backward-regexp) |  |
| `comint_run` | Run a program in a new comint buffer (emacs comint-run) |  |
| `shell_forward_command` | Move forward over a shell command on the input line (emacs shell-forward-command) |  |
| `shell_backward_command` | Move backward over a shell command on the input line (emacs shell-backward-command) |  |
| `run_config_manager` | Manage run/debug configurations | **spacemacs** — normal: `` <space>Rc ``, `` <space>Re ``, `` <space>cm ``, `` <space>pi ``, select: `` <space>Rc ``, `` <space>Re ``, `` <space>cm ``, `` <space>pi `` |
| `run_active_config` | Run the active run configuration | **spacemacs** — normal: `` <F5> ``, `` <space>Rr ``, `` <space>cC ``, `` <space>pc ``, `` <space>pu ``, `` <C-c><C-c> ``, select: `` <space>Rr ``, `` <space>cC ``, `` <space>pc ``, `` <space>pu ``, `` <C-c><C-c> ``, insert: `` <C-c><C-c> ``<br>**vim** — normal: `` <F5> `` |
| `clear_run_output` | Clear the Run tool window output | **spacemacs** — normal: `` <space>Rl ``, `` <space>Rx ``, `` <space>ck ``, select: `` <space>Rl ``, `` <space>Rx ``, `` <space>ck `` |
| `rerun_last_run` | Re-run the last command in the Run console | **spacemacs** — normal: `` <space>RR ``, `` <space>cr ``, `` <C-c><C-r> ``, select: `` <space>RR ``, `` <space>cr ``, `` <C-c><C-r> ``, insert: `` <C-c><C-r> `` |
| `run_next_error` | Jump to the next file:line in the run output | **spacemacs** — normal: `` <A-g>n ``, `` <C-x>` ``, `` <space>Rn ``, `` <A-g><A-n> ``, select: `` <C-x>` ``, `` <space>Rn ``, insert: `` <C-x>` `` |
| `run_prev_error` | Jump to the previous file:line in the run output | **spacemacs** — normal: `` <A-g>p ``, `` <space>Rp ``, `` <A-g><A-p> ``, select: `` <space>Rp `` |
| `reveal_in_tree` | Reveal the current file in the project tree |  |
| `toggle_auto_reveal` | Toggle always-select-opened-file (autoscroll from source) | **spacemacs** — normal: `` <space>pV ``, select: `` <space>pV `` |
| `focus_file_tree` | Focus the project file tree panel | **spacemacs** — normal: `` <space>Wp ``, `` <space>Wt ``, select: `` <space>Wp ``, `` <space>Wt `` |
| `focus_structure` | Focus the structure/symbol outline panel | **spacemacs** — normal: `` <space>Wo ``, `` <space>Ws ``, select: `` <space>Wo ``, `` <space>Ws `` |
| `hide_active_tool_window` | Return focus to the editor, hiding the active tool window (JetBrains Shift-Esc) | **spacemacs** — normal: `` <space>Wq ``, select: `` <space>Wq `` |
| `jump_to_last_tool_window` | Toggle focus between the editor and the last tool window (JetBrains F12) | **spacemacs** — normal: `` <space>W<tab> ``, select: `` <space>W<tab> `` |
| `focus_bookmarks` | Focus the Bookmarks tool window (pinned files; JetBrains Bookmarks) | **spacemacs** — normal: `` <space>Wb ``, select: `` <space>Wb `` |
| `focus_marks_panel` | Focus the Marks tool window | **spacemacs** — normal: `` <space>Wk ``, select: `` <space>Wk `` |
| `focus_registers_panel` | Focus the Registers tool window | **spacemacs** — normal: `` <space>WR ``, select: `` <space>WR `` |
| `focus_jumplist_panel` | Focus the Jumplist tool window | **spacemacs** — normal: `` <space>Wj ``, select: `` <space>Wj `` |
| `focus_recent_panel` | Focus the Recent Files tool window | **spacemacs** — normal: `` <space>Wu ``, select: `` <space>Wu `` |
| `focus_todo_panel` | Focus the TODO tool window | **spacemacs** — normal: `` <space>Wd ``, select: `` <space>Wd `` |
| `focus_problems` | Focus the problems/diagnostics panel | **spacemacs** — normal: `` <space>We ``, select: `` <space>We `` |
| `focus_run_console` | Focus the Run console (scroll output with j/k/PgUp/PgDn) | **spacemacs** — normal: `` <space>Wr ``, select: `` <space>Wr `` |
| `focus_git_panel` | Focus the Git changes panel (j/k select, Enter opens) | **spacemacs** — normal: `` <space>Wg ``, `` <space>gG ``, select: `` <space>Wg ``, `` <space>gG `` |
| `focus_ci_panel` | Focus the CI status panel (GitHub Actions runs; Enter opens in browser) | **spacemacs** — normal: `` <space>Wc ``, select: `` <space>Wc `` |
| `toggle_bottom_zoom` | Maximize / restore the bottom panel | **spacemacs** — normal: `` <space>Wm ``, select: `` <space>Wm `` |
| `toggle_drawer_mid` | Fold / unfold the middle column of the bottom drawer | **spacemacs** — normal: `` <space>Wf ``, select: `` <space>Wf `` |
| `toggle_ide` | Toggle the IDE workbench (Zen / focus mode) | **spacemacs** — normal: `` <space>Wz ``, `` <space>zz ``, select: `` <space>Wz ``, `` <space>zz `` |
| `settings_page` | Open the settings page (config.toml editor) | **spacemacs** — normal: `` <space>S ``, select: `` <space>S `` |
| `goto_next_spell_error` | Move to the next misspelled word (]s) | **spacemacs, vim** — normal: `` ]s `` |
| `goto_prev_spell_error` | Move to the previous misspelled word ([s) | **spacemacs, vim** — normal: `` [s `` |
| `spell_add_good` | Mark word under cursor as correctly spelled (zg) | **spacemacs, vim** — normal: `` zg `` |
| `spell_add_bad` | Mark word under cursor as misspelled (zw) | **spacemacs, vim** — normal: `` zw `` |
| `spell_undo` | Undo a zg/zw for the word under cursor (zug) | **spacemacs, vim** — normal: `` zug ``, `` zuw `` |
| `spell_add_good_internal` | Mark word under cursor good for this session only (zG) | **spacemacs, vim** — normal: `` zG `` |
| `spell_add_bad_internal` | Mark word under cursor misspelled for this session only (zW) | **spacemacs, vim** — normal: `` zW `` |
| `spell_undo_internal` | Undo a zG/zW for the word under cursor (zuG) | **spacemacs, vim** — normal: `` zuG ``, `` zuW `` |
| `spell_suggest` | Show spelling suggestions for the word under cursor (z=) | **spacemacs, vim** — normal: `` z= `` |
| `ispell_word` | Spell-check the word at point with aspell/hunspell (emacs ispell-word, M-$) | **spacemacs** — normal: `` <A-$> `` |
| `flyspell_auto_correct_word` | Correct the word at point with the top suggestion (emacs flyspell-auto-correct-word) |  |
| `view_file` | Open a file read-only for viewing (emacs view-file, C-x C-r) |  |
| `view_buffer` | Make the current buffer read-only for viewing (emacs view-buffer) |  |
| `view_buffer_other_window` | Show the current buffer read-only in a new split (emacs view-buffer-other-window) |  |
| `ispell_region` | Spell-check the selection with an external speller (emacs ispell-region) |  |
| `ispell_buffer` | Spell-check the whole buffer with an external speller (emacs ispell-buffer) |  |
| `ispell_continue` | Continue the halted spell-check from the word at point (emacs ispell-continue) |  |
| `ispell_message` | Spell-check a mail message body, skipping headers/citations/signature (emacs ispell-message) |  |
| `ispell` | Spell-check the region or buffer with an external speller (emacs ispell) |  |
| `ispell_change_dictionary` | Set the ispell dictionary/language (emacs ispell-change-dictionary) |  |
| `flyspell_buffer` | Check the whole buffer with the wordlist speller and move to the first misspelling (emacs flyspell-buffer) |  |
| `flyspell_region` | Check the selection with the wordlist speller (emacs flyspell-region) |  |
| `flyspell_word` | Check the word at point with the wordlist speller (emacs flyspell-word) |  |
| `flyspell_check_previous_highlighted_word` | Move to the previous misspelled word before point (emacs flyspell-check-previous-highlighted-word) |  |
| `flyspell_goto_next_error` | Move to the next misspelled word (emacs flyspell-goto-next-error) |  |
| `flyspell_mode` | Toggle on-the-fly spell checking (emacs flyspell-mode) | **spacemacs** — normal: `` <space>tS ``, select: `` <space>tS `` |
| `flyspell_prog_mode` | Toggle on-the-fly spell checking of comments and strings (emacs flyspell-prog-mode) |  |
| `outline_next_visible_heading` | Move to the next outline heading (emacs outline-next-visible-heading) | **spacemacs** — normal: `` <C-c>@<C-n> ``, select: `` <C-c>@<C-n> ``, insert: `` <C-c>@<C-n> `` |
| `outline_previous_visible_heading` | Move to the previous outline heading (emacs outline-previous-visible-heading) | **spacemacs** — normal: `` <C-c>@<C-p> ``, select: `` <C-c>@<C-p> ``, insert: `` <C-c>@<C-p> `` |
| `outline_up_heading` | Move to the parent outline heading (emacs outline-up-heading) | **spacemacs** — normal: `` <C-c>@<C-u> ``, select: `` <C-c>@<C-u> ``, insert: `` <C-c>@<C-u> `` |
| `outline_forward_same_level` | Move to the next same-level heading (emacs outline-forward-same-level) | **spacemacs** — normal: `` <C-c>@<C-f> ``, select: `` <C-c>@<C-f> ``, insert: `` <C-c>@<C-f> `` |
| `outline_backward_same_level` | Move to the previous same-level heading (emacs outline-backward-same-level) | **spacemacs** — normal: `` <C-c>@<C-b> ``, select: `` <C-c>@<C-b> ``, insert: `` <C-c>@<C-b> `` |
| `outline_hide_subtree` | Fold the subtree of the heading at point (emacs outline-hide-subtree) | **spacemacs** — normal: `` <C-c>@<C-d> ``, select: `` <C-c>@<C-d> ``, insert: `` <C-c>@<C-d> `` |
| `outline_show_subtree` | Reveal the subtree of the heading at point (emacs outline-show-subtree) |  |
| `outline_hide_entry` | Fold this heading's body (emacs outline-hide-entry) |  |
| `outline_show_entry` | Reveal this heading's body (emacs outline-show-entry) | **spacemacs** — normal: `` <C-c>@<C-e> ``, select: `` <C-c>@<C-e> ``, insert: `` <C-c>@<C-e> `` |
| `outline_hide_body` | Fold all bodies, showing only headings (emacs outline-hide-body) | **spacemacs** — normal: `` <C-c>@<C-t> ``, select: `` <C-c>@<C-t> ``, insert: `` <C-c>@<C-t> `` |
| `outline_show_all` | Reveal all outline body text (emacs outline-show-all) | **spacemacs** — normal: `` <C-c>@<C-a> ``, select: `` <C-c>@<C-a> ``, insert: `` <C-c>@<C-a> `` |
| `outline_hide_sublevels` | Show only the top N levels of headings (emacs outline-hide-sublevels) | **spacemacs** — normal: `` <C-c>@<C-q> ``, select: `` <C-c>@<C-q> ``, insert: `` <C-c>@<C-q> `` |
| `outline_hide_leaves` | Fold bodies in the current subtree, keeping subheadings (emacs outline-hide-leaves) |  |
| `outline_show_children` | Reveal the immediate subheadings of the heading at point (emacs outline-show-children) | **spacemacs** — normal: `` <C-c>@<C-i> ``, select: `` <C-c>@<C-i> ``, insert: `` <C-c>@<C-i> `` |
| `outline_show_branches` | Reveal every subheading in the subtree at point (emacs outline-show-branches) | **spacemacs** — normal: `` <C-c>@<C-k> ``, select: `` <C-c>@<C-k> ``, insert: `` <C-c>@<C-k> `` |
| `outline_cycle` | Cycle the heading at point folded -> children -> subtree (emacs outline-cycle) |  |
| `outline_cycle_buffer` | Cycle the whole buffer show-all -> overview -> contents (emacs outline-cycle-buffer) |  |
| `fold_create` | Create a fold over the selection (zf) |  |
| `fold_toggle` | Toggle fold under cursor (za) | **spacemacs** — normal: `` zA ``, `` za ``, `` zi ``, `` <C-c>@<C-c> ``, select: `` <C-c>@<C-c> ``, insert: `` <C-c>@<C-c> ``<br>**vim** — normal: `` zA ``, `` za ``, `` zi `` |
| `fold_open` | Open fold under cursor (zo) | **spacemacs** — normal: `` zo ``, `` zv ``, `` zx ``, `` <C-c>@<C-r> ``, `` <C-c>@<C-s> ``, select: `` <C-c>@<C-r> ``, `` <C-c>@<C-s> ``, insert: `` <C-c>@<C-r> ``, `` <C-c>@<C-s> ``<br>**vim** — normal: `` zo ``, `` zv ``, `` zx `` |
| `fold_close` | Close fold under cursor (zc) | **spacemacs** — normal: `` zC ``, `` zc ``, `` <C-c>@<C-h> ``, select: `` <C-c>@<C-h> ``, insert: `` <C-c>@<C-h> ``<br>**vim** — normal: `` zC ``, `` zc `` |
| `fold_open_recursive` | Open fold under cursor and all nested folds (IntelliJ Expand Recursively) | **spacemacs, vim** — normal: `` zO `` |
| `fold_close_recursive` | Close fold under cursor and all nested folds (IntelliJ Collapse Recursively) |  |
| `fold_open_all` | Open all folds (zR) | **spacemacs** — normal: `` zR ``, `` zn ``, `` <C-c>@<A-C-s> ``, select: `` <C-c>@<A-C-s> ``, insert: `` <C-c>@<A-C-s> ``<br>**vim** — normal: `` zR ``, `` zn `` |
| `fold_close_all` | Close all folds (zM) | **spacemacs** — normal: `` zM ``, `` zN ``, `` zX ``, `` <C-c>@<A-C-h> ``, select: `` <C-c>@<A-C-h> ``, insert: `` <C-c>@<A-C-h> ``<br>**vim** — normal: `` zM ``, `` zN ``, `` zX `` |
| `fold_more` | Fold more: close one more level of nested folds (zm) | **spacemacs, vim** — normal: `` zm `` |
| `fold_less` | Fold less: open one more level of nested folds (zr) | **spacemacs, vim** — normal: `` zr `` |
| `fold_delete` | Delete fold under cursor (zd) | **spacemacs, vim** — normal: `` zD ``, `` zd `` |
| `fold_delete_all` | Delete all folds (zE) | **spacemacs, vim** — normal: `` zE `` |
| `narrow_to_region` | Narrow the buffer to the selected region (SPC n r) | **spacemacs** — normal: `` <C-x>nn ``, `` <space>nr ``, select: `` <C-x>nn ``, `` <space>nr ``, insert: `` <C-x>nn `` |
| `widen` | Widen: remove narrowing and reveal the whole buffer (SPC n w) | **spacemacs** — normal: `` <C-x>nw ``, `` <space>nw ``, select: `` <C-x>nw ``, `` <space>nw ``, insert: `` <C-x>nw `` |
| `narrow_to_function_indirect` | Narrow to the function in an indirect (split) view (SPC n F) | **spacemacs** — normal: `` <space>nF ``, select: `` <space>nF `` |
| `narrow_region_indirect` | Narrow to the selected region in an indirect (split) view (SPC n R) | **spacemacs** — normal: `` <space>nR ``, select: `` <space>nR `` |
| `layout_add_buffers` | Add another layout's buffers into the current windows (SPC l A) | **spacemacs** — normal: `` <space>lA ``, select: `` <space>lA `` |
| `winner_undo` | Undo the last window-layout change (winner-undo, SPC w u) | **spacemacs** — normal: `` <C-w>u ``, `` <C-w>.u ``, `` <C-w>[u ``, `` <C-w>{u ``, `` <space>wu ``, `` <space>w.u ``, `` <space>w[u ``, `` <space>w{u ``, select: `` <space>wu ``, `` <space>w.u ``, `` <space>w[u ``, `` <space>w{u ``<br>**vim** — normal: `` <C-w>u ``, `` <C-w>.u ``, `` <C-w>[u ``, `` <C-w>{u `` |
| `winner_redo` | Redo a window-layout change (winner-redo, SPC w . U) | **spacemacs** — normal: `` <C-w>.U ``, `` <C-w>[U ``, `` <C-w>{U ``, `` <space>w.U ``, `` <space>w[U ``, `` <space>w{U ``, select: `` <space>w.U ``, `` <space>w[U ``, `` <space>w{U ``<br>**vim** — normal: `` <C-w>.U ``, `` <C-w>[U ``, `` <C-w>{U `` |
| `exit_transient_state` | Leave the current transient state (q) | **spacemacs** — normal: `` <C-w>.q ``, `` <C-w>[q ``, `` <C-w>{q ``, `` <space>b.q ``, `` <space>enq ``, `` <space>epq ``, `` <space>lwq ``, `` <space>n+q ``, `` <space>n=q ``, `` <space>n_q ``, `` <space>w.q ``, `` <space>w[q ``, `` <space>w{q ``, `` <space>x.q ``, `` <space>zfq ``, `` <space>zxq ``, `` <space>l<ret> ``, `` <space>n<minus>q ``, select: `` <space>b.q ``, `` <space>enq ``, `` <space>epq ``, `` <space>lwq ``, `` <space>n+q ``, `` <space>n=q ``, `` <space>n_q ``, `` <space>w.q ``, `` <space>w[q ``, `` <space>w{q ``, `` <space>x.q ``, `` <space>zfq ``, `` <space>zxq ``, `` <space>l<ret> ``, `` <space>n<minus>q ``<br>**vim** — normal: `` <C-w>.q ``, `` <C-w>[q ``, `` <C-w>{q `` |
| `text_scale_increase` | Increase the text scale one step (SPC z x +) | **spacemacs** — normal: `` <C-x><C-+> ``, `` <C-x><C-=> ``, `` <space>zx+ ``, `` <space>zx= ``, `` <space>zxk ``, `` <C-x><A-C-+> ``, `` <C-x><A-C-=> ``, select: `` <C-x><C-+> ``, `` <C-x><C-=> ``, `` <space>zx+ ``, `` <space>zx= ``, `` <space>zxk ``, `` <C-x><A-C-+> ``, `` <C-x><A-C-=> ``, insert: `` <C-x><C-+> ``, `` <C-x><C-=> ``, `` <C-x><A-C-+> ``, `` <C-x><A-C-=> `` |
| `text_scale_decrease` | Decrease the text scale one step (SPC z x -) | **spacemacs** — normal: `` <space>zx_ ``, `` <space>zxj ``, `` <C-x><C-minus> ``, `` <C-x><A-C-minus> ``, `` <space>zx<minus> ``, select: `` <space>zx_ ``, `` <space>zxj ``, `` <C-x><C-minus> ``, `` <C-x><A-C-minus> ``, `` <space>zx<minus> ``, insert: `` <C-x><C-minus> ``, `` <C-x><A-C-minus> `` |
| `text_scale_reset` | Reset the text scale to the default size (SPC z x 0) | **spacemacs** — normal: `` <C-x><C-0> ``, `` <space>zx0 ``, `` <C-x><A-C-0> ``, select: `` <C-x><C-0> ``, `` <space>zx0 ``, `` <C-x><A-C-0> ``, insert: `` <C-x><C-0> ``, `` <C-x><A-C-0> `` |
| `frame_zoom_in` | Zoom the frame in one step (SPC z f +) | **spacemacs** — normal: `` <space>zf+ ``, `` <space>zf= ``, `` <space>zfk ``, select: `` <space>zf+ ``, `` <space>zf= ``, `` <space>zfk `` |
| `frame_zoom_out` | Zoom the frame out one step (SPC z f -) | **spacemacs** — normal: `` <space>zf_ ``, `` <space>zfj ``, `` <space>zf<minus> ``, select: `` <space>zf_ ``, `` <space>zfj ``, `` <space>zf<minus> `` |
| `frame_zoom_reset` | Reset the frame zoom to the default size (SPC z f 0) | **spacemacs** — normal: `` <space>zf0 ``, select: `` <space>zf0 `` |
| `copy_version` | Display and copy the zmax version to the clipboard (SPC f e v) | **spacemacs** — normal: `` <space>fev ``, select: `` <space>fev `` |
| `narrow_to_page_indirect` | Narrow to the page in an indirect (split) view (SPC n P) | **spacemacs** — normal: `` <space>nP ``, select: `` <space>nP `` |
| `kmacro_ring_next` | Cycle to the next macro in the ring (SPC K r n) | **spacemacs** — normal: `` <space>Krn ``, `` <C-x><C-k><C-n> ``, select: `` <space>Krn ``, `` <C-x><C-k><C-n> ``, insert: `` <C-x><C-k><C-n> `` |
| `kmacro_ring_prev` | Cycle to the previous macro in the ring (SPC K r p) | **spacemacs** — normal: `` <space>KrN ``, `` <space>Krp ``, `` <C-x><C-k><C-p> ``, select: `` <space>KrN ``, `` <space>Krp ``, `` <C-x><C-k><C-p> ``, insert: `` <C-x><C-k><C-p> `` |
| `kmacro_ring_delete` | Delete the head macro in the ring (SPC K r d) | **spacemacs** — normal: `` <space>Krd ``, `` <C-x><C-k>d ``, select: `` <space>Krd ``, `` <C-x><C-k>d ``, insert: `` <C-x><C-k>d `` |
| `kmacro_ring_swap` | Swap the first two macros in the ring (SPC K r s) | **spacemacs** — normal: `` <space>Krs ``, `` <C-x><C-k><C-t> ``, select: `` <space>Krs ``, `` <C-x><C-k><C-t> ``, insert: `` <C-x><C-k><C-t> `` |
| `kmacro_ring_view` | View the head macro in the ring (SPC K r L) | **spacemacs** — normal: `` <space>Kv ``, `` <space>KrL ``, select: `` <space>Kv ``, `` <space>KrL `` |
| `kmacro_call_ring_2nd` | Run the second macro in the ring without rotating it (emacs kmacro-call-ring-2nd, SPC K r l) |  |
| `kmacro_to_register` | Write the last macro to a register (SPC K e r) | **spacemacs** — normal: `` <space>Ken ``, `` <space>Ker ``, `` <C-x><C-k>x ``, select: `` <space>Ken ``, `` <space>Ker ``, `` <C-x><C-k>x ``, insert: `` <C-x><C-k>x `` |
| `kmacro_add_counter` | Add [count] to the keyboard-macro counter (SPC K c a) | **spacemacs** — normal: `` <space>Kca ``, `` <C-x><C-k><C-a> ``, select: `` <space>Kca ``, `` <C-x><C-k><C-a> ``, insert: `` <C-x><C-k><C-a> `` |
| `kmacro_insert_counter` | Insert the macro counter value, then increment (SPC K c c) | **spacemacs** — normal: `` <space>Kcc ``, `` <C-x><C-k><C-i> ``, select: `` <space>Kcc ``, `` <C-x><C-k><C-i> ``, insert: `` <C-x><C-k><C-i> `` |
| `kmacro_set_counter` | Set the keyboard-macro counter to [count] (emacs kmacro-set-counter) | **spacemacs** — normal: `` <space>KcC ``, `` <C-x><C-k><C-c> ``, select: `` <space>KcC ``, `` <C-x><C-k><C-c> ``, insert: `` <C-x><C-k><C-c> `` |
| `kmacro_set_format` | Set the macro-counter insert format, e.g. %03d (emacs kmacro-set-format) | **spacemacs** — normal: `` <space>Kcf ``, `` <C-x><C-k><C-f> ``, select: `` <space>Kcf ``, `` <C-x><C-k><C-f> ``, insert: `` <C-x><C-k><C-f> `` |
| `kmacro_name_last_macro` | Name the last kbd macro and register it as an invokable command (emacs kmacro-name-last-macro) | **spacemacs** — normal: `` <C-x><C-k>n ``, select: `` <C-x><C-k>n ``, insert: `` <C-x><C-k>n `` |
| `insert_kbd_macro` | Insert a textual definition of the last kbd macro (emacs insert-kbd-macro) |  |
| `apply_macro_to_region_lines` | Run the last kbd macro at the start of each line in the region (emacs apply-macro-to-region-lines) | **spacemacs** — normal: `` <C-x><C-k>r ``, select: `` <C-x><C-k>r ``, insert: `` <C-x><C-k>r `` |
| `kmacro_end_or_call_macro` | End recording, or call the last kbd macro (emacs kmacro-end-or-call-macro, F4) | **spacemacs** — normal: `` <F4> ``, `` <C-x>e ``, `` <space>KK ``, select: `` <C-x>e ``, `` <space>KK ``, insert: `` <C-x>e ``<br>**vim** — normal: `` <F4> `` |
| `kmacro_end_or_call_macro_repeat` | Repeat-variant of end-or-call macro (emacs kmacro-end-or-call-macro-repeat) | **spacemacs** — normal: `` <C-x><C-k><C-k> ``, select: `` <C-x><C-k><C-k> ``, insert: `` <C-x><C-k><C-k> `` |
| `kmacro_edit_macro` | Edit the last kbd macro's keys as text (emacs edit-kbd-macro / kmacro-edit-macro) | **spacemacs** — normal: `` <space>Kee ``, `` <C-x><C-k>e ``, `` <C-x><C-k><C-e> ``, `` <C-x><C-k><ret> ``, select: `` <space>Kee ``, `` <C-x><C-k>e ``, `` <C-x><C-k><C-e> ``, `` <C-x><C-k><ret> ``, insert: `` <C-x><C-k>e ``, `` <C-x><C-k><C-e> ``, `` <C-x><C-k><ret> `` |
| `edmacro_insert_key` | Read one literal key and insert its written name at point (emacs edmacro-insert-key) |  |
| `edmacro_set_macro_to_region_lines` | Set the kbd macro to the lines the region covers (emacs edmacro-set-macro-to-region-lines) |  |
| `kmacro_step_edit_macro` | Step through the last kbd macro key by key, editing as you go (emacs kmacro-step-edit-macro) | **spacemacs** — normal: `` <space>Kes ``, `` <C-x><C-k><space> ``, select: `` <space>Kes ``, `` <C-x><C-k><space> ``, insert: `` <C-x><C-k><space> `` |
| `kmacro_edit_lossage` | Edit the recently pressed keys as a macro (emacs kmacro-edit-lossage) | **spacemacs** — normal: `` <space>Kel ``, `` <C-x><C-k>l ``, select: `` <space>Kel ``, `` <C-x><C-k>l ``, insert: `` <C-x><C-k>l `` |
| `kmacro_bind_to_key` | Report the config binding for the last kbd macro (emacs kmacro-bind-to-key) | **spacemacs** — normal: `` <space>Keb ``, `` <C-x><C-k>b ``, select: `` <space>Keb ``, `` <C-x><C-k>b ``, insert: `` <C-x><C-k>b `` |
| `kmacro_redisplay` | Force a redisplay while a kbd macro runs (emacs kmacro-redisplay, C-x C-k d) |  |
| `toggle_readonly` | Toggle the buffer's read-only (writable) state (SPC b w) | **spacemacs** — normal: `` <space>bw ``, `` <C-x><C-q> ``, select: `` <space>bw ``, `` <C-x><C-q> ``, insert: `` <C-x><C-q> `` |
| `toggle_window_dedication` | Toggle window dedication (spacemacs SPC w t) | **spacemacs** — normal: `` <C-w>t ``, `` <C-x>wd ``, `` <space>wt ``, select: `` <C-x>wd ``, `` <space>wt ``, insert: `` <C-x>wd ``<br>**vim** — normal: `` <C-w>t `` |
| `toggle_subword` | Toggle sub-word w/b/e motions (spacemacs SPC t c) | **spacemacs** — normal: `` <space>tc ``, select: `` <space>tc `` |
| `toggle_superword` | Toggle super-word w/b/e motions: symbols join words (emacs superword-mode, SPC t C) | **spacemacs** — normal: `` <space>tC ``, select: `` <space>tC `` |
| `toggle_auto_fill` | Toggle auto-fill: wrap at text-width while typing (spacemacs SPC t F) | **spacemacs** — normal: `` <space>tF ``, select: `` <space>tF `` |
| `toggle_follow_mode` | Toggle follow mode: windows on the same doc scroll together (spacemacs SPC w f) | **spacemacs** — normal: `` <C-w>f ``, `` <space>wf ``, select: `` <space>wf ``<br>**vim** — normal: `` <C-w>f `` |
| `subword_w` | Next word start, sub-word aware (w) | **spacemacs, vim** — normal: `` w ``, `` <C-right> ``, insert: `` <C-right> ``, `` <S-right> `` |
| `subword_b` | Previous word start, sub-word aware (b) | **spacemacs, vim** — normal: `` b `` |
| `subword_e` | Next word end, sub-word aware (e) | **spacemacs, vim** — normal: `` e `` |
| `subword_extend_w` | Extend to next word start, sub-word aware |  |
| `subword_extend_b` | Extend to previous word start, sub-word aware |  |
| `subword_extend_e` | Extend to next word end, sub-word aware |  |
| `subword_extend_ge` | Extend to previous word end, sub-word aware (ge) |  |
| `paredit_slurp_forward` | Paredit: slurp the next s-expression forward (SPC k s) | **spacemacs** — normal: `` <space>ks ``, `` <space>k`s ``, select: `` <space>ks ``, `` <space>k`s `` |
| `paredit_barf_forward` | Paredit: barf the last s-expression forward (SPC k b) | **spacemacs** — normal: `` <space>kb ``, select: `` <space>kb `` |
| `paredit_slurp_backward` | Paredit: slurp the previous s-expression backward (SPC k S) | **spacemacs** — normal: `` <space>kS ``, select: `` <space>kS `` |
| `paredit_barf_backward` | Paredit: barf the first s-expression backward (SPC k B) | **spacemacs** — normal: `` <space>kB ``, select: `` <space>kB `` |
| `paredit_splice` | Paredit: splice/unwrap the enclosing s-expression (SPC k W) | **spacemacs** — normal: `` <space>kW ``, select: `` <space>kW `` |
| `paredit_raise` | Paredit: raise the current s-expression (SPC k r) | **spacemacs** — normal: `` <space>kr ``, select: `` <space>kr `` |
| `paredit_transpose` | Paredit: transpose the s-expressions around point (SPC k t) | **spacemacs** — normal: `` <space>kt ``, `` <space>k`p ``, `` <space>k`t ``, select: `` <space>kt ``, `` <space>k`p ``, `` <space>k`t `` |
| `paredit_split` | Paredit: split the enclosing list at point (SPC j s) | **spacemacs** — normal: `` <space>jS ``, `` <space>js ``, select: `` <space>jS ``, `` <space>js `` |
| `paredit_absorb` | Paredit: absorb the previous sexp into the current form (SPC k a) | **spacemacs** — normal: `` <space>ka ``, select: `` <space>ka `` |
| `paredit_convolute` | Paredit: convolute — swap enclosing/inner prefixes (SPC k c) | **spacemacs** — normal: `` <space>kc ``, select: `` <space>kc `` |
| `buffer_swap_window_1` | Swap current buffer with window 1 (SPC b . M-1) | **spacemacs** — normal: `` <space>b.<A-1> ``, select: `` <space>b.<A-1> `` |
| `buffer_swap_window_2` | Swap current buffer with window 2 (SPC b . M-2) | **spacemacs** — normal: `` <space>b.<A-2> ``, select: `` <space>b.<A-2> `` |
| `buffer_swap_window_3` | Swap current buffer with window 3 (SPC b . M-3) | **spacemacs** — normal: `` <space>b.<A-3> ``, select: `` <space>b.<A-3> `` |
| `buffer_swap_window_4` | Swap current buffer with window 4 (SPC b . M-4) | **spacemacs** — normal: `` <space>b.<A-4> ``, select: `` <space>b.<A-4> `` |
| `buffer_swap_window_5` | Swap current buffer with window 5 (SPC b . M-5) | **spacemacs** — normal: `` <space>b.<A-5> ``, select: `` <space>b.<A-5> `` |
| `buffer_swap_window_6` | Swap current buffer with window 6 (SPC b . M-6) | **spacemacs** — normal: `` <space>b.<A-6> ``, select: `` <space>b.<A-6> `` |
| `buffer_swap_window_7` | Swap current buffer with window 7 (SPC b . M-7) | **spacemacs** — normal: `` <space>b.<A-7> ``, select: `` <space>b.<A-7> `` |
| `buffer_swap_window_8` | Swap current buffer with window 8 (SPC b . M-8) | **spacemacs** — normal: `` <space>b.<A-8> ``, select: `` <space>b.<A-8> `` |
| `buffer_swap_window_9` | Swap current buffer with window 9 (SPC b . M-9) | **spacemacs** — normal: `` <space>b.<A-9> ``, select: `` <space>b.<A-9> `` |
| `paredit_splice_kill_forward` | Paredit: splice, killing forward (SPC k e) | **spacemacs** — normal: `` <space>ke ``, select: `` <space>ke `` |
| `paredit_splice_kill_backward` | Paredit: splice, killing backward (SPC k E) | **spacemacs** — normal: `` <space>kE ``, select: `` <space>kE `` |
| `paredit_insert_sexp_after` | Paredit: insert a new () sexp after the current one (SPC k )) | **spacemacs** — normal: `` <space>k) ``, select: `` <space>k) `` |
| `paredit_insert_sexp_before` | Paredit: insert a new () sexp before the current one (SPC k () | **spacemacs** — normal: `` <space>k( ``, select: `` <space>k( `` |
| `fold_next` | Move to next fold (zj) | **spacemacs, vim** — normal: `` ]z ``, `` zj `` |
| `fold_prev` | Move to previous fold (zk) | **spacemacs, vim** — normal: `` [z ``, `` zk `` |
| `goto_line_last_nonblank` | Goto last non-blank on line (g_) | **spacemacs, vim** — normal: `` g_ `` |
| `goto_line_middle` | Goto middle of text line (gM) | **spacemacs, vim** — normal: `` gM ``, `` gm `` |
| `goto_byte` | Goto byte {count} in buffer (go) | **spacemacs, vim** — normal: `` go `` |
| `goto_prev_unmatched_paren` | Goto previous unmatched ( ([() | **spacemacs** — normal: `` [( ``, `` <space>j( ``, select: `` <space>j( ``<br>**vim** — normal: `` [( `` |
| `goto_prev_unmatched_brace` | Goto previous unmatched { ([{) | **spacemacs, vim** — normal: `` [{ `` |
| `goto_next_unmatched_paren` | Goto next unmatched ) (]) | **spacemacs, vim** — normal: `` ]) `` |
| `goto_next_unmatched_brace` | Goto next unmatched } (]}) | **spacemacs, vim** — normal: `` ]} `` |
| `goto_prev_preproc` | Goto previous unmatched #if/#else ([#) | **spacemacs, vim** — normal: `` [# `` |
| `goto_next_preproc` | Goto next unmatched #endif/#else (]#) | **spacemacs, vim** — normal: `` ]# `` |
| `vim_sleep` | Sleep for {count} seconds (vim gs) | **spacemacs, vim** — normal: `` gs `` |
| `goto_prev_mark` | Goto previous lowercase mark ([\`) | **spacemacs, vim** — normal: `` [` `` |
| `goto_next_mark` | Goto next lowercase mark (]\`) | **spacemacs, vim** — normal: `` ]` `` |
| `goto_prev_mark_line` | Goto previous lowercase mark, line start ([']) | **spacemacs, vim** — normal: `` [' `` |
| `goto_next_mark_line` | Goto next lowercase mark, line start (]') | **spacemacs, vim** — normal: `` ]' `` |
| `yank_file_path` | Yank current file path to clipboard | **spacemacs** — normal: `` <space>fyC ``, `` <space>fyY ``, `` <space>fyy ``, select: `` <space>fyC ``, `` <space>fyY ``, `` <space>fyy `` |
| `yank_file_name` | Yank current file name to clipboard | **spacemacs** — normal: `` <space>fyN ``, `` <space>fyn ``, select: `` <space>fyN ``, `` <space>fyn `` |
| `yank_file_path_with_line` | Yank current file path:line to clipboard | **spacemacs** — normal: `` <space>fyL ``, `` <space>fyl ``, select: `` <space>fyL ``, `` <space>fyl `` |
| `yank_file_path_with_line_col` | Yank current file path:line:col to clipboard | **spacemacs** — normal: `` <space>fyc ``, select: `` <space>fyc `` |
| `yank_file_dir` | Yank current file's directory to clipboard | **spacemacs** — normal: `` <space>fyD ``, `` <space>fyd ``, select: `` <space>fyD ``, `` <space>fyd `` |
| `copy_remote_url` | Copy web permalink (host/blob/<sha>/path#Ln) for current line | **spacemacs** — normal: `` <space>glC ``, `` <space>glL ``, `` <space>glP ``, select: `` <space>glC ``, `` <space>glL ``, `` <space>glP `` |
| `open_remote_url` | Open current line's web permalink in the browser | **spacemacs** — normal: `` <space>glc ``, `` <space>gll ``, `` <space>glp ``, select: `` <space>glc ``, `` <space>gll ``, `` <space>glp `` |
| `open_url_under_cursor` | Open the URL under the cursor in the browser | **spacemacs** — normal: `` <C-c><ret> ``, select: `` <C-c><ret> ``, insert: `` <C-c><ret> `` |
| `open_all_buffer_links` | Open every URL in the current buffer in the browser (link-hint-open-all-links, SPC x A) | **spacemacs** — normal: `` <space>xA ``, select: `` <space>xA `` |
| `copy_all_buffer_links` | Copy every URL in the current buffer to the clipboard (link-hint-copy-all-links, SPC x Y) | **spacemacs** — normal: `` <space>xY ``, select: `` <space>xY `` |
| `link_hint_open_link` | Pick a URL from the current buffer and open it in the browser (link-hint-open-link, SPC x O) | **spacemacs** — normal: `` <space>xO ``, select: `` <space>xO `` |
| `helm_google_suggest` | Google suggestions, opening the results in the browser (helm-google-suggest, SPC s w g) |  |
| `link_hint_copy_link` | Pick a URL from the current buffer and copy it to the clipboard (link-hint-copy-link, SPC x y) | **spacemacs** — normal: `` <space>xy ``, select: `` <space>xy `` |
| `profiler_start` | Start (and reset) the command profiler (SPC h P s) | **spacemacs** — normal: `` <space>hPs ``, select: `` <space>hPs `` |
| `profiler_stop` | Stop the command profiler, keeping its samples (SPC h P k) | **spacemacs** — normal: `` <space>hPk ``, select: `` <space>hPk `` |
| `profiler_report` | Show the command profiler report, slowest first (SPC h P r) | **spacemacs** — normal: `` <space>hPr ``, select: `` <space>hPr `` |
| `profiler_write_report` | Write the command profiler report to a prompted file (SPC h P w) | **spacemacs** — normal: `` <space>hPw ``, select: `` <space>hPw `` |
| `regexp_generate_strings` | Generate every string matched by a finite regexp (SPC x r ') | **spacemacs** — normal: `` <space>xr' ``, `` <space>xrp' ``, select: `` <space>xr' ``, `` <space>xrp' `` |
| `regexp_generate_strings_emacs` | Generate every string matched by a finite Emacs regexp (SPC x r e ') | **spacemacs** — normal: `` <space>xre' ``, select: `` <space>xre' `` |
| `toggle_fringe` | Hide or show the whole fringe (gutter column strip) (fringe-mode, SPC T f) | **spacemacs** — normal: `` <space>Tf ``, select: `` <space>Tf `` |
| `restart_editor` | Close every view and relaunch zmax with the same arguments (restart-emacs, SPC q r) | **spacemacs** — normal: `` <space>qr ``, select: `` <space>qr `` |
| `duplicate_selection_down` | Duplicate current line(s) downward |  |
| `duplicate_selection_up` | Duplicate current line(s) upward |  |
| `move_text_line_down` | Move current line(s) down past the next line |  |
| `move_text_line_up` | Move current line(s) up past the previous line |  |
| `count_selection` | Count chars/words/lines in selection | **spacemacs** — normal: `` <A-=> ``, `` <space>xc ``, select: `` <space>xc `` |
| `match_brackets` | Goto matching bracket | **spacemacs** — normal: `` <space>k% ``, select: `` <space>k% ``<br>**helix** — normal: `` mm ``, select: `` mm `` |
| `match_brackets_extend` | Extend to matching bracket, inclusive (vim d%/c%/y%) |  |
| `match_brackets_or_goto_percent` | Goto matching bracket, or {count} percent through the file | **spacemacs, vim** — normal: `` % ``, select: `` % `` |
| `surround_add` | Surround add | **helix** — normal: `` ms ``, select: `` ms `` |
| `surround_replace` | Surround replace | **helix** — normal: `` mr ``, select: `` mr `` |
| `surround_delete` | Surround delete | **helix** — normal: `` md ``, select: `` md `` |
| `select_textobject_around` | Select around object | **spacemacs, vim** — select: `` a ``<br>**helix** — normal: `` ma ``, select: `` ma `` |
| `select_textobject_inner` | Select inside object | **spacemacs, vim** — select: `` i ``<br>**helix** — normal: `` mi ``, select: `` mi `` |
| `change_textobject_inner` | Change inside object (ci) | **spacemacs, vim** — normal: `` ci `` |
| `change_textobject_around` | Change around object (ca) | **spacemacs, vim** — normal: `` ca `` |
| `delete_textobject_inner` | Delete inside object (di) | **spacemacs, vim** — normal: `` di `` |
| `delete_textobject_around` | Delete around object (da) | **spacemacs, vim** — normal: `` da `` |
| `yank_textobject` | Yank the selection and put the cursor at its start (vim visual y) |  |
| `yank_textobject_inner` | Yank inside object (yi) | **spacemacs, vim** — normal: `` yi `` |
| `yank_textobject_around` | Yank around object (ya) | **spacemacs, vim** — normal: `` ya `` |
| `extend_backward_exclusive_vim` | Make a backward selection exclusive of the cursor (vim backward motions) |  |
| `extend_forward_exclusive_vim` | Make a forward selection exclusive of the cursor (vim n, /pat under an operator) |  |
| `delete_to_mark` | Delete to a mark (vim d\`) | **spacemacs, vim** — normal: `` d` `` |
| `delete_to_mark_line` | Delete whole lines to a mark (vim d') | **spacemacs, vim** — normal: `` d' `` |
| `change_to_mark` | Change to a mark (vim c\`) | **spacemacs, vim** — normal: `` c` `` |
| `change_to_mark_line` | Change whole lines to a mark (vim c') | **spacemacs, vim** — normal: `` c' `` |
| `yank_to_mark` | Yank to a mark (vim y\`) | **spacemacs, vim** — normal: `` y` `` |
| `yank_to_mark_line` | Yank whole lines to a mark (vim y') | **spacemacs, vim** — normal: `` y' `` |
| `reflow_textobject_inner` | Reflow inside object (vim gqi) | **spacemacs, vim** — normal: `` gqi `` |
| `reflow_textobject_around` | Reflow around object (vim gqa) | **spacemacs, vim** — normal: `` gqa `` |
| `reflow_keep_textobject_inner` | Reflow inside object, keep cursor (vim gwi) | **spacemacs, vim** — normal: `` gwi `` |
| `reflow_keep_textobject_around` | Reflow around object, keep cursor (vim gwa) | **spacemacs, vim** — normal: `` gwa `` |
| `indent_textobject_inner` | Indent inside object (vim >i) | **spacemacs, vim** — normal: `` <gt>i `` |
| `indent_textobject_around` | Indent around object (vim >a) | **spacemacs, vim** — normal: `` <gt>a `` |
| `unindent_textobject_inner` | Unindent inside object (vim <i) | **spacemacs, vim** — normal: `` <lt>i `` |
| `unindent_textobject_around` | Unindent around object (vim <a) | **spacemacs, vim** — normal: `` <lt>a `` |
| `uppercase_textobject_inner` | Uppercase inside object (vim gUi) | **spacemacs, vim** — normal: `` gUi `` |
| `uppercase_textobject_around` | Uppercase around object (vim gUa) | **spacemacs, vim** — normal: `` gUa `` |
| `lowercase_textobject_inner` | Lowercase inside object (vim gui) | **spacemacs, vim** — normal: `` gui `` |
| `lowercase_textobject_around` | Lowercase around object (vim gua) | **spacemacs, vim** — normal: `` gua `` |
| `togglecase_textobject_inner` | Toggle case inside object (vim g~i) | **spacemacs, vim** — normal: `` g~i `` |
| `togglecase_textobject_around` | Toggle case around object (vim g~a) | **spacemacs, vim** — normal: `` g~a `` |
| `delete_find_char_forward` | Delete to next char (df) | **spacemacs, vim** — normal: `` df `` |
| `delete_till_char_forward` | Delete till next char (dt) | **spacemacs, vim** — normal: `` dt `` |
| `zap_to_char` | Kill through the next char, inclusive (emacs zap-to-char, M-z) | **spacemacs** — normal: `` <A-z> ``<br>**emacs, cua** — normal: `` <A-z> ``, insert: `` <A-z> `` |
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
| `xref_find_references` | Find references to a symbol across the workspace (emacs xref-find-references) | **spacemacs** — normal: `` <A-?> ``, `` <space>pE ``, select: `` <space>pE `` |
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
| `picture_open_line` | Picture-mode: split the line at point (emacs picture-open-line) | **emacs, cua** — insert: `` <C-o> `` |
| `picture_clear_line` | Picture-mode: clear to end of line (emacs picture-clear-line) |  |
| `picture_clear_column` | Picture-mode: blank columns after point (emacs picture-clear-column) |  |
| `picture_backward_clear_column` | Picture-mode: blank columns before point (emacs picture-backward-clear-column) |  |
| `picture_clear_rectangle_to_register` | Picture-mode: clear rectangle into a register (emacs picture-clear-rectangle-to-register) |  |
| `picture_yank_rectangle` | Picture-mode: overlay the killed rectangle (emacs picture-yank-rectangle) |  |
| `picture_yank_rectangle_from_register` | Picture-mode: overlay a register's rectangle (emacs picture-yank-rectangle-from-register) |  |
| `twocol_two_columns` | Two-column: create a side-by-side partner buffer (emacs 2C-two-columns) | **spacemacs** — normal: `` <F2>2 ``, `` <C-x>62 ``, select: `` <C-x>62 ``, insert: `` <C-x>62 `` |
| `twocol_associate_buffer` | Two-column: associate the other window's buffer (emacs 2C-associate-buffer) | **spacemacs** — normal: `` <F2>b ``, `` <C-x>6b ``, select: `` <C-x>6b ``, insert: `` <C-x>6b `` |
| `twocol_split` | Two-column: split the buffer at point into two columns (emacs 2C-split) | **spacemacs** — normal: `` <F2>s ``, `` <C-x>6s ``, select: `` <C-x>6s ``, insert: `` <C-x>6s `` |
| `twocol_merge` | Two-column: merge the partner column back in (emacs 2C-merge) | **spacemacs** — normal: `` <F2>1 ``, `` <C-x>61 ``, select: `` <C-x>61 ``, insert: `` <C-x>61 `` |
| `twocol_dissociate` | Two-column: break the association (emacs 2C-dissociate) | **spacemacs** — normal: `` <F2>d ``, `` <C-x>6d ``, select: `` <C-x>6d ``, insert: `` <C-x>6d `` |
| `twocol_newline` | Two-column: newline in both columns (emacs 2C-newline) | **spacemacs** — normal: `` <F2><ret> ``, `` <C-x>6<ret> ``, select: `` <C-x>6<ret> ``, insert: `` <C-x>6<ret> `` |
| `table` | Edit a text table (emacs table.el) |  |
| `table_recognize` | Recognize the ASCII table at point and report its dimensions (emacs table-recognize) |  |
| `table_recognize_region` | Recognize the table in the selection and report its dimensions (emacs table-recognize-region) |  |
| `table_recognize_table` | Recognize the whole table at point (emacs table-recognize-table) |  |
| `table_recognize_cell` | Report the table cell at point (emacs table-recognize-cell) |  |
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
| `table_fixed_width_mode` | Toggle table fixed-width mode: cells wrap instead of widening (emacs table-fixed-width-mode) |  |
| `table_unrecognize` | Deactivate every table in the buffer (emacs table-unrecognize) |  |
| `table_unrecognize_table` | Deactivate the table at point (emacs table-unrecognize-table) |  |
| `table_unrecognize_region` | Deactivate every table in the region (emacs table-unrecognize-region) |  |
| `table_unrecognize_cell` | Deactivate the table cell at point (emacs table-unrecognize-cell) |  |
| `word_search_forward` | Search forward for words, ignoring punctuation (emacs word-search-forward) |  |
| `word_search_backward` | Search backward for words, ignoring punctuation (emacs word-search-backward) |  |
| `fill_region_as_paragraph` | Fill the region as a single paragraph (emacs fill-region-as-paragraph) |  |
| `indent_rigidly` | Shift the region by N columns (emacs indent-rigidly) | **spacemacs** — normal: `` <C-x><tab> ``, select: `` <C-x><tab> ``, insert: `` <C-x><tab> `` |
| `quoted_insert` | Insert the next key literally, control chars included (emacs quoted-insert) | **emacs, cua** — insert: `` <C-q> `` |
| `foldout_zoom_subtree` | Narrow to the outline subtree at point (emacs foldout-zoom-subtree) | **spacemacs** — normal: `` <C-c><C-z> ``, select: `` <C-c><C-z> ``, insert: `` <C-c><C-z> `` |
| `foldout_exit_fold` | Leave the zoomed subtree and widen (emacs foldout-exit-fold) | **spacemacs** — normal: `` <C-c><C-x> ``, select: `` <C-c><C-x> ``, insert: `` <C-c><C-x> `` |
| `ff_find_related_file` | Visit the related header/source file (emacs ff-find-related-file) |  |
| `revert_buffer_quick` | Revert the buffer from its file without confirmation (emacs revert-buffer-quick) | **spacemacs** — normal: `` <C-x>xg ``, select: `` <C-x>xg ``, insert: `` <C-x>xg `` |
| `delete_file` | Delete a file from disk (emacs delete-file) |  |
| `quit_window` | Quit this window, burying the buffer (emacs quit-window) |  |
| `switch_to_buffer_other_window` | Show a buffer in another window (emacs switch-to-buffer-other-window) | **spacemacs** — normal: `` <C-x>4b ``, select: `` <C-x>4b ``, insert: `` <C-x>4b ``<br>**emacs** — insert: `` <C-x>4b ``<br>**cua** — select: `` <C-X>4b ``, insert: `` <C-x>4b `` |
| `find_file_other_window` | Open a file in another window (emacs find-file-other-window) | **spacemacs** — normal: `` <C-x>4f ``, `` <C-x>4<C-f> ``, select: `` <C-x>4f ``, `` <C-x>4<C-f> ``, insert: `` <C-x>4f ``, `` <C-x>4<C-f> ``<br>**emacs** — insert: `` <C-x>4f ``, `` <C-x>4<C-f> ``<br>**cua** — select: `` <C-X>4f ``, `` <C-X>4<C-f> ``, insert: `` <C-x>4f ``, `` <C-x>4<C-f> `` |
| `shrink_window_if_larger_than_buffer` | Shrink the window to fit its buffer (emacs shrink-window-if-larger-than-buffer) | **spacemacs** — normal: `` <C-x><minus> ``, select: `` <C-x><minus> ``, insert: `` <C-x><minus> `` |
| `edit_abbrevs` | Show every abbrev definition for editing (emacs edit-abbrevs) |  |
| `quietly_read_abbrev_file` | Read abbrev definitions from a file, silently (emacs quietly-read-abbrev-file) |  |
| `whitespace_toggle_options` | Toggle one whitespace visualization by key (emacs whitespace-toggle-options) |  |
| `xref_find_apropos` | List project identifiers matching a pattern (emacs xref-find-apropos) |  |
| `keyboard_escape_quit` | Drop extra cursors and deactivate the region (emacs keyboard-escape-quit) | **emacs, cua** — insert: `` <C-]> ``, `` <esc><esc><esc> `` |
| `kill_ring_deindent_mode` | Toggle deindenting text saved to the kill ring (emacs kill-ring-deindent-mode) |  |
| `kill_some_buffers` | Offer to kill each buffer in turn (emacs kill-some-buffers) |  |
| `async_shell_command` | Run a shell command without blocking, output to a buffer (emacs async-shell-command) | **emacs, cua** — insert: `` <A-&> `` |
| `xref_quit_and_pop_marker_stack` | Close the xref results and jump back (emacs xref-quit-and-pop-marker-stack) |  |
| `display_fill_column_indicator_mode` | Toggle a rule at the fill column (emacs display-fill-column-indicator-mode) |  |
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
| `fortran_mode` | Enter fixed-form Fortran mode (emacs fortran-mode) |  |
| `f90_mode` | Enter free-form Fortran/F90 mode (emacs f90-mode) |  |
| `facemenu` | Browse faces and colors (emacs list-faces-display / facemenu) |  |
| `facemenu_set_face` | Put a named face on the region (emacs facemenu-set-face) |  |
| `facemenu_set_foreground` | Put a foreground color on the region (emacs facemenu-set-foreground) |  |
| `facemenu_set_background` | Put a background color on the region (emacs facemenu-set-background) |  |
| `facemenu_set_bold` | Make the region bold (emacs facemenu-set-bold) |  |
| `facemenu_set_italic` | Make the region italic (emacs facemenu-set-italic) |  |
| `facemenu_set_bold_italic` | Make the region bold italic (emacs facemenu-set-bold-italic) |  |
| `facemenu_set_underline` | Underline the region (emacs facemenu-set-underline) |  |
| `facemenu_set_default` | Put the default face on the region (emacs facemenu-set-default) |  |
| `facemenu_remove_face_props` | Remove the face property from the region (emacs facemenu-remove-face-props) |  |
| `facemenu_remove_all` | Remove every text property from the region (emacs facemenu-remove-all) |  |
| `format_decode_buffer` | Decode this buffer's format annotations into text properties (emacs format-decode-buffer) |  |
| `hide_ifdef_mode` | Hide the code the C preprocessor skips (emacs hide-ifdef-mode) |  |
| `cpp_highlight_buffer` | Shade the preprocessor branches that are compiled out (emacs cpp-highlight-buffer) |  |
| `cwarn_mode` | Highlight suspicious C constructs in this buffer (emacs cwarn-mode) |  |
| `global_cwarn_mode` | Highlight suspicious C constructs everywhere (emacs global-cwarn-mode) |  |
| `sgml_tags_invisible` | Hide the markup tags, show only the text (emacs sgml-tags-invisible) |  |
| `goto_address_mode` | Highlight the URLs and e-mails in this buffer (emacs goto-address-mode) |  |
| `transient_mark_mode` | Toggle highlighting of the region (emacs transient-mark-mode) |  |
| `prettify_symbols_mode` | Draw -> as an arrow, lambda as a lambda (emacs prettify-symbols-mode) |  |
| `glyphless_display_mode` | Reveal control and zero-width characters (emacs glyphless-display-mode) |  |
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
| `indent_code_rigidly` | Shift region lines by [count] columns, skipping lines that start in a string (emacs indent-code-rigidly, = r) | **spacemacs, vim** — normal: `` =r `` |
| `indent_sexp_rigidly` | Reindent this line and shift the grouping starting on it by the same amount (emacs C-u TAB) |  |
| `c_hungry_delete_forward` | Delete all whitespace after point, else one char (emacs c-hungry-delete-forward, SPC x d f) | **spacemacs** — normal: `` <space>xdf ``, select: `` <space>xdf `` |
| `c_hungry_delete_backwards` | Delete all whitespace before point, else one char (emacs c-hungry-delete-backwards, SPC x d b) | **spacemacs** — normal: `` <space>xdb ``, select: `` <space>xdb `` |
| `c_beginning_of_defun` | Move to the start of the function at point (emacs c-beginning-of-defun) | **spacemacs** — normal: `` <A-C-a> `` |
| `c_end_of_defun` | Move to the end of the function at point (emacs c-end-of-defun) | **spacemacs** — normal: `` <A-C-e> `` |
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
| `comment_set_column` | Set the comment column from point, aligning this line's comment with a prefix (emacs comment-set-column, C-x ;) |  |
| `fortran_fill_paragraph` | Fill the Fortran comment block or statement at point (emacs fortran-fill-paragraph, M-q) |  |
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
| `delete_find_char_backward` | Delete to prev char (dF) | **spacemacs, vim** — normal: `` dF `` |
| `delete_till_char_backward` | Delete till prev char (dT) | **spacemacs, vim** — normal: `` dT `` |
| `change_find_char_forward` | Change to next char (cf) | **spacemacs, vim** — normal: `` cf `` |
| `change_till_char_forward` | Change till next char (ct) | **spacemacs, vim** — normal: `` ct `` |
| `change_find_char_backward` | Change to prev char (cF) | **spacemacs, vim** — normal: `` cF `` |
| `change_till_char_backward` | Change till prev char (cT) | **spacemacs, vim** — normal: `` cT `` |
| `yank_find_char_forward` | Yank to next char (yf) | **spacemacs, vim** — normal: `` yf `` |
| `yank_till_char_forward` | Yank till next char (yt) | **spacemacs, vim** — normal: `` yt `` |
| `yank_find_char_backward` | Yank to prev char (yF) | **spacemacs, vim** — normal: `` yF `` |
| `yank_till_char_backward` | Yank till prev char (yT) | **spacemacs, vim** — normal: `` yT `` |
| `set_mark` | Set mark (m{a-z} buffer, m{A-Z} global) | **spacemacs, vim** — normal: `` m `` |
| `goto_mark` | Goto mark exact (\`{a-z/A-Z/0-9}, \`\` for last jump) | **spacemacs, vim** — normal: `` ` `` |
| `goto_mark_line` | Goto mark line ('{a-z/A-Z/0-9}, '' for last jump) | **spacemacs, vim** — normal: `` ' `` |
| `goto_mark_nojump` | Goto mark exact without changing jumplist (g\`) | **spacemacs, vim** — normal: `` g` `` |
| `goto_mark_line_nojump` | Goto mark line without changing jumplist (g') | **spacemacs, vim** — normal: `` g' `` |
| `repeat_substitute` | Repeat last :substitute (&) | **spacemacs, vim** — normal: `` & `` |
| `repeat_substitute_global` | Repeat last :substitute on whole file (g&) | **spacemacs, vim** — normal: `` g& `` |
| `vim_record_macro` | Record macro into register (q{reg}) | **spacemacs, vim** — normal: `` q `` |
| `vim_replay_macro` | Replay macro from register (@{reg}) | **spacemacs, vim** — normal: `` @ `` |
| `save_visual_selection` | Save the visual selection (for gv) |  |
| `reselect_visual` | Reselect the last visual area (gv) | **spacemacs, vim** — normal: `` gv ``, select: `` gv `` |
| `mark_insert_exit` | Record the insert-exit position (for gi) |  |
| `insert_at_last_insert` | Insert at the last insert position (gi) | **spacemacs, vim** — normal: `` gi `` |
| `goto_next_function` | Goto next function | **spacemacs, vim** — normal: `` ][ ``, `` ]m ``<br>**helix** — normal: `` ]f ``, select: `` ]f ``<br>**emacs, cua** — normal: `` <A-C-e> ``, insert: `` <A-C-e> `` |
| `goto_prev_function` | Goto previous function | **spacemacs, vim** — normal: `` [] ``, `` [m ``<br>**helix** — normal: `` [f ``, select: `` [f ``<br>**emacs, cua** — normal: `` <A-C-a> ``, insert: `` <A-C-a> `` |
| `goto_next_class` | Goto next type definition | **helix** — normal: `` ]t ``, select: `` ]t `` |
| `goto_prev_class` | Goto previous type definition | **helix** — normal: `` [t ``, select: `` [t `` |
| `goto_next_parameter` | Goto next parameter | **helix** — normal: `` ]a ``, select: `` ]a `` |
| `goto_prev_parameter` | Goto previous parameter | **helix** — normal: `` [a ``, select: `` [a `` |
| `goto_next_comment` | Goto next comment | **spacemacs, vim** — normal: `` ]* ``, `` ]/ ``<br>**helix** — normal: `` ]c ``, select: `` ]c `` |
| `goto_prev_comment` | Goto previous comment | **spacemacs, vim** — normal: `` [* ``, `` [/ ``<br>**helix** — normal: `` [c ``, select: `` [c `` |
| `goto_next_test` | Goto next test | **helix** — normal: `` ]T ``, select: `` ]T `` |
| `goto_prev_test` | Goto previous test | **helix** — normal: `` [T ``, select: `` [T `` |
| `goto_next_xml_element` | Goto next (X)HTML element | **helix** — normal: `` ]x ``, select: `` ]x `` |
| `goto_prev_xml_element` | Goto previous (X)HTML element | **helix** — normal: `` [x ``, select: `` [x `` |
| `goto_next_entry` | Goto next pairing | **helix** — normal: `` ]e ``, select: `` ]e `` |
| `goto_prev_entry` | Goto previous pairing | **helix** — normal: `` [e ``, select: `` [e `` |
| `goto_next_paragraph` | Goto next paragraph | **spacemacs** — normal: `` } ``, `` <A-}> ``, select: `` } ``<br>**vim** — normal: `` } ``, select: `` } ``<br>**helix** — normal: `` ]p ``, select: `` ]p ``<br>**emacs, cua** — normal: `` <A-}> ``, insert: `` <A-}> `` |
| `goto_prev_paragraph` | Goto previous paragraph | **spacemacs** — normal: `` { ``, `` <A-{> ``, select: `` { ``<br>**vim** — normal: `` { ``, select: `` { ``<br>**helix** — normal: `` [p ``, select: `` [p ``<br>**emacs, cua** — normal: `` <A-{> ``, insert: `` <A-{> `` |
| `goto_next_section` | Goto next section (vim ]]) | **spacemacs, vim** — normal: `` ]] `` |
| `goto_prev_section` | Goto previous section (vim [[) | **spacemacs, vim** — normal: `` [[ `` |
| `move_sentence_forward` | Move to next sentence | **spacemacs** — normal: `` ) ``, `` <A-e> ``, select: `` ) ``<br>**vim** — normal: `` ) ``, select: `` ) `` |
| `move_sentence_backward` | Move to previous sentence | **spacemacs** — normal: `` ( ``, `` <A-a> ``, select: `` ( ``<br>**vim** — normal: `` ( ``, select: `` ( `` |
| `dap_launch` | Launch debug target | **spacemacs** — normal: `` <S-F5> ``, `` <space>dd ``, `` <C-c><C-d> ``, select: `` <space>dd ``, `` <C-c><C-d> ``, insert: `` <C-c><C-d> ``<br>**vim** — normal: `` <S-F5> ``<br>**helix** — normal: `` <space>Gl ``, select: `` <space>Gl `` |
| `dap_restart` | Restart debugging session | **spacemacs** — normal: `` <space>dr ``, select: `` <space>dr ``<br>**helix** — normal: `` <space>Gr ``, select: `` <space>Gr `` |
| `dap_toggle_breakpoint` | Toggle breakpoint | **spacemacs** — normal: `` <F9> ``, `` <space>db ``, `` <C-x><C-a><C-b> ``, select: `` <space>db ``, `` <C-x><C-a><C-b> ``, insert: `` <C-x><C-a><C-b> ``<br>**vim** — normal: `` <F9> ``<br>**helix** — normal: `` <space>Gb ``, select: `` <space>Gb `` |
| `dap_remove_breakpoint` | Remove breakpoint on current line (Emacs gud-remove) | **spacemacs** — normal: `` <C-x><C-a><C-d> ``, select: `` <C-x><C-a><C-d> ``, insert: `` <C-x><C-a><C-d> `` |
| `dap_continue` | Continue program execution | **spacemacs** — normal: `` <space>dc ``, `` <C-x><C-a><C-r> ``, select: `` <space>dc ``, `` <C-x><C-a><C-r> ``, insert: `` <C-x><C-a><C-r> ``<br>**helix** — normal: `` <space>Gc ``, select: `` <space>Gc `` |
| `dap_run_to_cursor` | Run the debugger up to the cursor line (JetBrains Run To Cursor) | **spacemacs** — normal: `` <space>dC ``, `` <C-c><C-u> ``, `` <C-x><C-a><C-u> ``, select: `` <space>dC ``, `` <C-c><C-u> ``, `` <C-x><C-a><C-u> ``, insert: `` <C-c><C-u> ``, `` <C-x><C-a><C-u> `` |
| `dap_pause` | Pause program execution | **spacemacs** — normal: `` <space>dp ``, select: `` <space>dp ``<br>**helix** — normal: `` <space>Gh ``, select: `` <space>Gh `` |
| `dap_step_in` | Step in | **spacemacs** — normal: `` <F11> ``, `` <space>di ``, `` <C-c><C-s> ``, `` <C-x><C-a><C-s> ``, select: `` <space>di ``, `` <C-c><C-s> ``, `` <C-x><C-a><C-s> ``, insert: `` <C-c><C-s> ``, `` <C-x><C-a><C-s> ``<br>**vim** — normal: `` <F11> ``<br>**helix** — normal: `` <space>Gi ``, select: `` <space>Gi `` |
| `dap_step_out` | Step out | **spacemacs** — normal: `` <S-F11> ``, `` <space>do ``, `` <C-c><C-f> ``, `` <C-x><C-a><C-f> ``, select: `` <space>do ``, `` <C-c><C-f> ``, `` <C-x><C-a><C-f> ``, insert: `` <C-c><C-f> ``, `` <C-x><C-a><C-f> ``<br>**vim** — normal: `` <S-F11> ``<br>**helix** — normal: `` <space>Go ``, select: `` <space>Go `` |
| `dap_next` | Step to next | **spacemacs** — normal: `` <F10> ``, `` <space>dn ``, `` <C-c><C-n> ``, `` <C-x><C-a><C-n> ``, select: `` <space>dn ``, `` <C-c><C-n> ``, `` <C-x><C-a><C-n> ``, insert: `` <C-c><C-n> ``, `` <C-x><C-a><C-n> ``<br>**vim** — normal: `` <F10> ``<br>**helix** — normal: `` <space>Gn ``, select: `` <space>Gn `` |
| `dap_variables` | List variables | **spacemacs** — normal: `` <space>dv ``, select: `` <space>dv ``<br>**helix** — normal: `` <space>Gv ``, select: `` <space>Gv `` |
| `dap_terminate` | End debug session | **spacemacs** — normal: `` <space>dq ``, select: `` <space>dq ``<br>**helix** — normal: `` <space>Gt ``, select: `` <space>Gt `` |
| `dap_edit_condition` | Edit breakpoint condition on current line | **helix** — normal: `` <space>G<C-c> ``, select: `` <space>G<C-c> `` |
| `dap_breakpoints_picker` | View all breakpoints in a picker (JetBrains View Breakpoints) | **spacemacs** — normal: `` <space>dB ``, select: `` <space>dB `` |
| `dap_edit_log` | Edit breakpoint log message on current line | **helix** — normal: `` <space>G<C-l> ``, select: `` <space>G<C-l> `` |
| `dap_switch_thread` | Switch current thread | **spacemacs** — normal: `` <space>dgt ``, select: `` <space>dgt ``<br>**helix** — normal: `` <space>Gst ``, select: `` <space>Gst `` |
| `dap_switch_stack_frame` | Switch stack frame | **spacemacs** — normal: `` <space>dgf ``, select: `` <space>dgf ``<br>**helix** — normal: `` <space>Gsf ``, select: `` <space>Gsf `` |
| `dap_enable_exceptions` | Enable exception breakpoints | **helix** — normal: `` <space>Ge ``, select: `` <space>Ge `` |
| `dap_disable_exceptions` | Disable exception breakpoints | **helix** — normal: `` <space>GE ``, select: `` <space>GE `` |
| `gdb_display_locals_buffer` | Show local variables of the current frame (emacs gdb-display-locals-buffer) | **spacemacs** — normal: `` <space>dgl ``, select: `` <space>dgl `` |
| `gdb_display_registers_buffer` | Show CPU registers of the current frame (emacs gdb-display-registers-buffer) | **spacemacs** — normal: `` <space>dgr ``, select: `` <space>dgr `` |
| `gdb_display_stack_for_thread` | Show the call stack of the current thread (emacs gdb-display-stack-for-thread) | **spacemacs** — normal: `` <space>dgs ``, select: `` <space>dgs `` |
| `gdb_display_locals_for_thread` | Show locals of the current thread's innermost frame (emacs gdb-display-locals-for-thread) |  |
| `gdb_display_registers_for_thread` | Show registers of the current thread's innermost frame (emacs gdb-display-registers-for-thread) |  |
| `gdb_display_disassembly_buffer` | Disassemble around the current frame PC (emacs gdb-display-disassembly-buffer) | **spacemacs** — normal: `` <space>dgd ``, select: `` <space>dgd `` |
| `gdb_display_disassembly_for_thread` | Disassemble around the current thread PC (emacs gdb-display-disassembly-for-thread) |  |
| `gdb_display_io_buffer` | Show the inferior IO / Run console (emacs gdb-display-io-buffer) | **spacemacs** — normal: `` <space>dgi ``, select: `` <space>dgi `` |
| `gdb_display_memory_buffer` | Read and hexdump target memory (emacs gdb-display-memory-buffer) | **spacemacs** — normal: `` <space>dgm ``, select: `` <space>dgm `` |
| `gdb_delete_breakpoint` | Delete the breakpoint on the current line (emacs gdb-delete-breakpoint) | **spacemacs** — normal: `` <space>dgk ``, select: `` <space>dgk `` |
| `gdb_edit_value` | Set a variable/expression value in the debugger (emacs gdb-edit-value) | **spacemacs** — normal: `` <space>dge ``, select: `` <space>dge `` |
| `gdb_many_windows` | Open the multi-pane debugger layout (emacs gdb-many-windows) | **spacemacs** — normal: `` <space>dgw ``, select: `` <space>dgw `` |
| `gdb_restore_windows` | Restore the debugger window layout (emacs gdb-restore-windows) |  |
| `gud_gdb_complete_command` | Complete the gdb command at point (emacs gud-gdb-complete-command) |  |
| `pandoc_menu` | Convert the buffer through pandoc, picking the output format (SPC P /) | **spacemacs** — normal: `` <space>P/ ``, select: `` <space>P/ `` |
| `shell_pipe` | Pipe selections through shell command | **spacemacs** — normal: `` <A-\|> ``<br>**helix** — normal: `` \| ``, select: `` \| `` |
| `shell_pipe_to` | Pipe selections into shell command ignoring output | **helix** — normal: `` <A-\|> ``, select: `` <A-\|> `` |
| `shell_insert_output` | Insert shell command output before selections | **spacemacs** — normal: `` <A-!> ``<br>**helix** — normal: `` ! ``, select: `` ! `` |
| `shell_append_output` | Append shell command output after selections | **helix** — normal: `` <A-!> ``, select: `` <A-!> `` |
| `shell_keep_pipe` | Filter selections with shell predicate | **helix** — normal: `` $ ``, select: `` $ `` |
| `suspend` | Suspend and return to shell | **spacemacs** — normal: `` <C-z> ``, `` <C-x><C-z> ``, select: `` <C-x><C-z> ``, insert: `` <C-x><C-z> ``<br>**vim** — normal: `` <C-z> ``<br>**helix** — normal: `` <C-z> ``, select: `` <C-z> `` |
| `rename_symbol` | Rename symbol | **spacemacs** — normal: `` <space>lr ``, select: `` <space>lr ``<br>**helix** — normal: `` <space>r ``, select: `` <space>r `` |
| `increment` | Increment item under cursor | **spacemacs** — normal: `` <C-a> ``, `` g<C-a> ``, `` <space>n++ ``, `` <space>n+= ``, `` <space>n=+ ``, `` <space>n== ``, `` <space>n_+ ``, `` <space>n_= ``, `` <space>n<minus>+ ``, `` <space>n<minus>= ``, select: `` <C-a> ``, `` <space>n++ ``, `` <space>n+= ``, `` <space>n=+ ``, `` <space>n== ``, `` <space>n_+ ``, `` <space>n_= ``, `` <space>n<minus>+ ``, `` <space>n<minus>= ``<br>**vim** — normal: `` <C-a> ``, `` g<C-a> ``, select: `` <C-a> ``<br>**helix** — normal: `` <C-a> ``, select: `` <C-a> `` |
| `decrement` | Decrement item under cursor | **spacemacs** — normal: `` g<C-x> ``, `` <space>n+_ ``, `` <space>n=_ ``, `` <space>n__ ``, `` <space>n+<minus> ``, `` <space>n<minus>_ ``, `` <space>n=<minus> ``, `` <space>n_<minus> ``, `` <space>n<minus><minus> ``, select: `` <space>n+_ ``, `` <space>n=_ ``, `` <space>n__ ``, `` <space>n+<minus> ``, `` <space>n<minus>_ ``, `` <space>n=<minus> ``, `` <space>n_<minus> ``, `` <space>n<minus><minus> ``<br>**vim** — normal: `` <C-x> ``, `` g<C-x> ``, select: `` <C-x> ``<br>**helix** — normal: `` <C-x> ``, select: `` <C-x> `` |
| `increment_sequential` | Increment each line in the selection by a growing amount (vim g CTRL-A) | **spacemacs, vim** — select: `` g<C-a> `` |
| `decrement_sequential` | Decrement each line in the selection by a growing amount (vim g CTRL-X) | **spacemacs, vim** — select: `` g<C-x> `` |
| `record_macro` | Record macro | **spacemacs** — normal: `` <C-x>( ``, select: `` <C-x>( ``, insert: `` <C-x>( ``<br>**helix** — normal: `` Q ``, select: `` Q `` |
| `replay_macro` | Replay macro | **spacemacs, vim** — normal: `` Q ``<br>**helix** — normal: `` q ``, select: `` q `` |
| `command_palette` | Open command palette | **spacemacs** — normal: `` <A-X> ``, `` <A-x> ``, `` <C-x>* ``, `` <C-x>; ``, `` <C-x>q ``, `` <space>? ``, `` <space><space> ``, select: `` <C-x>* ``, `` <C-x>; ``, `` <C-x>q ``, `` <space>? ``, `` <space><space> ``, insert: `` <C-x>* ``, `` <C-x>; ``, `` <C-x>q ``<br>**helix** — normal: `` <space>? ``, select: `` <space>? ``<br>**emacs, cua** — normal: `` <A-x> ``, insert: `` <A-X> ``, `` <A-x> `` |
| `search_everywhere` | Search Everywhere: choose Files/Symbols/Text/Actions/Buffers (JetBrains) | **spacemacs** — normal: `` <space>sE ``, select: `` <space>sE `` |
| `recent_files_switcher` | Recent Files switcher: tool windows + recent files (SPC b r) | **spacemacs** — normal: `` <space>br ``, select: `` <space>br `` |
| `recentf_mode` | Toggle recording of opened files in the recent-files list (emacs recentf-mode) |  |
| `recentf_save_list` | Write the recent-files list to its store file (emacs recentf-save-list) |  |
| `recentf_edit_list` | Edit the recent-files list: mark entries and delete them (emacs recentf-edit-list) |  |
| `repl` | Open the embedded-language REPL (elisp/viml/stryke/awk/zsh) |  |
| `goto_word` | Jump to a two-character label | **spacemacs** — normal: `` <space>jl ``, `` <space>jw ``, select: `` <space>jl ``, `` <space>jw ``<br>**helix** — normal: `` gw `` |
| `extend_to_word` | Extend to a two-character label | **helix** — select: `` gw `` |
| `goto_char` | Label every visible occurrence of a char and jump (vim-easymotion s) | **spacemacs** — normal: `` <A-g>c ``, `` <space>ja ``, `` <space>je ``, select: `` <space>ja ``, `` <space>je `` |
| `extend_to_char` | Label every visible occurrence of a char and extend to it |  |
| `find_char_forward_label` | easymotion f: label forward occurrences of a char and jump |  |
| `find_char_backward_label` | easymotion F: label backward occurrences of a char and jump |  |
| `till_char_forward_label` | easymotion t: label forward, jump till before a char |  |
| `till_char_backward_label` | easymotion T: label backward, jump till after a char |  |
| `goto_next_tabstop` | Goto next snippet placeholder |  |
| `goto_prev_tabstop` | Goto next snippet placeholder |  |
| `emmet_expand` | Expand emmet/zen HTML abbreviation (or Tab) | **helix, emacs, cua** — insert: `` <tab> `` |
| `snippet_expand` | Expand the user snippet whose trigger precedes the cursor |  |
| `rotate_selections_first` | Make the first selection your primary one |  |
| `rotate_selections_last` | Make the last selection your primary one |  |
| `show_keyword_line_from_start` | Show the first line containing the keyword ([i) | **spacemacs, vim** — normal: `` [i `` |
| `show_keyword_line_from_cursor` | Show the next line containing the keyword (]i) | **spacemacs, vim** — normal: `` ]i `` |
| `list_keyword_lines_from_start` | List every line containing the keyword ([I) | **spacemacs, vim** — normal: `` [I `` |
| `list_keyword_lines_from_cursor` | List the lines below containing the keyword (]I) | **spacemacs, vim** — normal: `` ]I `` |
| `list_defines_from_start` | List every #define of the keyword ([D) | **spacemacs, vim** — normal: `` [D `` |
| `list_defines_from_cursor` | List the #defines of the keyword below the cursor (]D) | **spacemacs, vim** — normal: `` ]D `` |
| `goto_keyword_line_from_start` | Jump to the first line containing the keyword ([CTRL-I) | **spacemacs, vim** — normal: `` [<C-i> `` |
| `goto_keyword_line_from_cursor` | Jump to the next line containing the keyword (]CTRL-I) | **spacemacs, vim** — normal: `` ]<C-i> `` |
| `goto_define_from_start` | Jump to the first #define of the keyword ([CTRL-D) | **spacemacs, vim** — normal: `` [<C-d> `` |
| `goto_define_from_cursor` | Jump to the next #define of the keyword (]CTRL-D) | **spacemacs, vim** — normal: `` ]<C-d> `` |
| `scroll_line_below_window` | Put the line below the window at the top of it (z+) |  |
| `scroll_line_above_window` | Put the line above the window at the bottom of it (z^) |  |
| `yank_no_trailing_whitespace` | Yank without the trailing whitespace of each line (zy) | **spacemacs, vim** — normal: `` zy `` |
| `paste_after_no_trailing_whitespace` | Paste after without trailing whitespace (zp) | **spacemacs, vim** — normal: `` zp `` |
| `paste_before_no_trailing_whitespace` | Paste before without trailing whitespace (zP) | **spacemacs, vim** — normal: `` zP `` |
| `command_mode_count` | Open the Ex line, with the count's line range ({count}:) | **spacemacs, vim** — normal: `` : `` |
| `filter_equalprg` | Filter the selection through 'equalprg', else reindent (v_=) |  |
| `complete_line` | Complete a whole line (i_CTRL-X CTRL-L) | **vim** — insert: `` <C-x><C-l> `` |
| `complete_filename` | Complete a file name (i_CTRL-X CTRL-F) | **vim** — insert: `` <C-x><C-f> `` |
| `complete_dictionary` | Complete from 'dictionary' (i_CTRL-X CTRL-K) | **vim** — insert: `` <C-x><C-k> `` |
| `complete_tag` | Complete a tag from the tags file (i_CTRL-X CTRL-]) | **vim** — insert: `` <C-x><C-]> `` |
| `complete_cmdline` | Complete like on the : command line (i_CTRL-X CTRL-V) | **vim** — insert: `` <C-x><C-v> `` |
| `complete_thesaurus` | Complete from 'thesaurus' (i_CTRL-X CTRL-T) | **vim** — insert: `` <C-x><C-t> `` |
| `complete_register_word` | Complete a word from the registers (i_CTRL-X CTRL-R) | **vim** — insert: `` <C-x><C-r> `` |
| `complete_define` | Complete a defined identifier (i_CTRL-X CTRL-D) | **vim** — insert: `` <C-x><C-d> `` |
| `complete_user_func` | Complete with 'completefunc' (i_CTRL-X CTRL-U) | **vim** — insert: `` <C-x><C-u> `` |
| `complete_omni_func` | Complete with 'omnifunc' (i_CTRL-X CTRL-O) | **vim** — insert: `` <C-x><C-o> `` |
| `operator_func` | Call 'operatorfunc' on the selection (g@) | **spacemacs, vim** — normal: `` g@ `` |
| `toggle_lang_keymap` | Toggle the :lmap language keymap (i_CTRL-^, 'iminsert') | **spacemacs, vim** — insert: `` <C-^> `` |
| `insert_spell_suggest` | Spelling suggestions for the word being typed (i_CTRL-X s) | **vim** — insert: `` <C-x>s ``, `` <C-x><C-s> `` |
| `kmacro_menu` | List the keyboard-macro ring — mark, delete, copy, edit (emacs kmacro-menu) |  |
| `kmacro_menu_mark` | Mark the macro at point in the kmacro list (emacs kmacro-menu-mark) |  |
| `kmacro_menu_unmark` | Unmark the macro at point in the kmacro list (emacs kmacro-menu-unmark) |  |
| `kmacro_menu_unmark_backward` | Move back a line and unmark (emacs kmacro-menu-unmark-backward) |  |
| `kmacro_menu_unmark_all` | Remove every mark in the kmacro list (emacs kmacro-menu-unmark-all) |  |
| `kmacro_menu_flag_for_deletion` | Flag the macro at point for deletion (emacs kmacro-menu-flag-for-deletion) |  |
| `kmacro_menu_do_flagged_delete` | Delete the flagged macros (emacs kmacro-menu-do-flagged-delete) |  |
| `kmacro_menu_do_delete` | Delete the marked macros (emacs kmacro-menu-do-delete) |  |
| `kmacro_menu_do_copy` | Duplicate the marked macros in the ring (emacs kmacro-menu-do-copy) |  |
| `kmacro_menu_transpose` | Transpose the macro at point with the previous one (emacs kmacro-menu-transpose) |  |
| `kmacro_menu_edit_column` | Edit the column at point in the kmacro list (emacs kmacro-menu-edit-column) |  |
| `kmacro_menu_edit_keys` | Edit the keys of the macro at point (emacs kmacro-menu-edit-keys) |  |
| `kmacro_menu_edit_counter` | Edit the counter of the macro at point (emacs kmacro-menu-edit-counter) |  |
| `kmacro_menu_edit_format` | Edit the counter format of the macro at point (emacs kmacro-menu-edit-format) |  |
| `kmacro_menu_edit_position` | Move the macro at point to another ring position (emacs kmacro-menu-edit-position) |  |
| `kmacro_end_macro` | End the keyboard macro being defined (emacs kmacro-end-macro, C-x )) | **spacemacs** — normal: `` <C-x>) ``, select: `` <C-x>) ``, insert: `` <C-x>) `` |
| `kmacro_start_macro_or_insert_counter` | Start a kbd macro, or insert the counter while defining one (emacs kmacro-start-macro-or-insert-counter, F3) | **spacemacs** — normal: `` <F3> ``, `` <space>Kk ``, select: `` <space>Kk `` |
| `keymap_global_set` | Bind a key sequence to a command, live (emacs keymap-global-set) |  |
| `keymap_global_unset` | Remove a key sequence's global binding (emacs keymap-global-unset) |  |
| `keymap_set` | Bind a key in one keymap — normal/select/insert (emacs keymap-set) |  |
| `keymap_unset` | Remove a key's binding from one keymap (emacs keymap-unset) |  |
| `keymap_substitute` | Rebind every key that runs OLD so it runs NEW (emacs keymap-substitute) |  |
| `describe_function` | Describe a command — its doc and key bindings (emacs describe-function, C-h f) | **spacemacs** — normal: `` <C-h>f ``, select: `` <C-h>f ``, insert: `` <C-h>f `` |
| `describe_key_briefly` | Echo, in one line, the command a key runs (emacs describe-key-briefly, C-h c) | **spacemacs** — normal: `` <C-h>c ``, select: `` <C-h>c ``, insert: `` <C-h>c `` |
| `describe_variable` | Describe an editor/vim variable — value and default (emacs describe-variable, C-h v) | **spacemacs** — normal: `` <C-h>v ``, select: `` <C-h>v ``, insert: `` <C-h>v `` |
| `describe_symbol` | Describe a name, command or variable (emacs describe-symbol, C-h o) | **spacemacs** — normal: `` <C-h>o ``, select: `` <C-h>o ``, insert: `` <C-h>o `` |
| `describe_repeat_maps` | List the sticky (transient-state) keymaps — zmax's repeat maps (emacs describe-repeat-maps) |  |
| `describe_character_set` | Describe a character set / Unicode block (emacs describe-character-set) |  |
| `help_quick` | Show the quick-help sheet with the live keys (emacs help-quick, C-h C-q) |  |
| `help_quick_toggle` | Show or close the quick-help sheet (emacs help-quick-toggle) | **spacemacs** — normal: `` <C-h><C-q> ``, select: `` <C-h><C-q> ``, insert: `` <C-h><C-q> `` |
| `help_go_back` | Go back to the previously visited Help entry (emacs help-go-back) | **spacemacs, vim** — normal: `` gb `` |
| `help_go_forward` | Go forward again in the Help history (emacs help-go-forward) |  |
| `help_goto_next_page` | Scroll the Help text down one screenful (emacs help-goto-next-page) |  |
| `help_goto_previous_page` | Scroll the Help text up one screenful (emacs help-goto-previous-page) |  |
| `calendar_mark_today` | Mark today's date in the calendar (emacs calendar-mark-today) |  |
| `calendar_unmark` | Remove the marks from the calendar (emacs calendar-unmark) |  |
| `calendar_star_date` | Replace the date under the cursor with asterisks (emacs calendar-star-date) |  |
| `calendar_redraw` | Redraw the calendar, re-reading the diary (emacs calendar-redraw) |  |
| `calendar_scroll_left` | Show the next month in the calendar (emacs calendar-scroll-left) |  |
| `calendar_scroll_right` | Show the previous month in the calendar (emacs calendar-scroll-right) |  |
| `diary_hebrew_list_entries` | List today's Hebrew-dated diary entries (emacs diary-hebrew-list-entries) |  |
| `diary_hebrew_mark_entries` | Mark the calendar days with Hebrew-dated diary entries (emacs diary-hebrew-mark-entries) |  |
| `diary_islamic_list_entries` | List today's Islamic-dated diary entries (emacs diary-islamic-list-entries) |  |
| `diary_islamic_mark_entries` | Mark the calendar days with Islamic-dated diary entries (emacs diary-islamic-mark-entries) |  |
| `diary_bahai_list_entries` | List today's Baha'i-dated diary entries (emacs diary-bahai-list-entries) |  |
| `diary_bahai_mark_entries` | Mark the calendar days with Baha'i-dated diary entries (emacs diary-bahai-mark-entries) |  |
| `diary_chinese_list_entries` | List today's Chinese-dated diary entries (emacs diary-chinese-list-entries) |  |
| `diary_chinese_mark_entries` | Mark the calendar days with Chinese-dated diary entries (emacs diary-chinese-mark-entries) |  |
| `diary_lunar_phases` | Report today's lunar phase, if any (emacs diary-lunar-phases) |  |
| `diary_sunrise_sunset` | Report today's sunrise and sunset (emacs diary-sunrise-sunset) |  |
| `diary_hebrew_sabbath_candles` | Report Friday's candle-lighting time (emacs diary-hebrew-sabbath-candles) |  |
| `dired_undo` | Undo the last change to the Dired listing (emacs dired-undo) |  |
| `minibuffer_complete_word` | Complete the minibuffer input one word further (emacs minibuffer-complete-word) |  |
| `minibuffer_complete_and_exit` | Complete uniquely and accept the minibuffer (emacs minibuffer-complete-and-exit) |  |
| `minibuffer_choose_completion` | Accept the minibuffer with the selected completion (emacs minibuffer-choose-completion) |  |
| `minibuffer_complete_history` | Complete the minibuffer input against its history (emacs minibuffer-complete-history) |  |
| `dabbrev_expand` | Expand the word before point from the buffer's other words (emacs dabbrev-expand, JetBrains Cyclic Expand Word) | **emacs, cua** — normal: `` <A-/> ``, insert: `` <A-/> `` |
| `dabbrev_completion` | List the buffer words that could expand the word before point (emacs dabbrev-completion) | **spacemacs** — normal: `` <A-C-/> `` |
| `copy_reference` | Copy a project-relative file:line reference to the clipboard (JetBrains Copy Reference) |  |
| `error_description` | Show the full text of the diagnostic under the cursor (JetBrains Error Description) |  |
| `find_sibling_file` | Open a sibling of this file: header/source, test/impl, style/template (emacs find-sibling-file, JetBrains Related File) |  |
| `navigation_bar` | Jump to an enclosing function/class from the breadcrumb trail (JetBrains Navigation Bar) |  |
| `select_in` | Select this file in another view: project tree, dired, terminal (JetBrains Select In) |  |
| `unwrap_remove` | Unwrap the block around the cursor, keeping its body (JetBrains Unwrap/Remove) |  |
| `move_file_refactor` | Move the current file to another directory (JetBrains Move, F6) |  |
| `run_anything` | Run any shell command in the Run tool window (JetBrains Run Anything) |  |
| `show_debug_window` | Open the Debug tool window (JetBrains Cmd-5) |  |
| `auto_indent_lines` | Reindent the selected lines (JetBrains Auto-Indent Lines) |  |
| `find_file_at_point` | Open the file or URL under the cursor (emacs find-file-at-point / ffap) |  |
| `find_file_other_tab` | Open a file in a new tab (emacs find-file-other-tab) | **spacemacs** — normal: `` <C-x>tf ``, select: `` <C-x>tf ``, insert: `` <C-x>tf `` |
| `find_grep` | Grep the project for a regex and list the hits (emacs find-grep) |  |
| `list_keyboard_macros` | List the keyboard-macro ring (emacs list-keyboard-macros) |  |
| `vc_register` | Put the current file under version control: git add (emacs vc-register) | **spacemacs** — normal: `` <C-x>vi ``, select: `` <C-x>vi ``, insert: `` <C-x>vi `` |
| `vc_annotate` | Annotate each line of the file with its last commit: git blame (emacs vc-annotate) |  |
| `vc_prepare_patch` | Prepare a patch of the last commit for mailing (emacs vc-prepare-patch) |  |
| `vc_insert_headers` | Insert a version header comment at point (emacs vc-insert-headers) |  |
| `vc_update_change_log` | Insert ChangeLog entries from the recent commits (emacs vc-update-change-log) | **spacemacs** — normal: `` <C-x>va ``, select: `` <C-x>va ``, insert: `` <C-x>va `` |
| `vc_ediff` | Diff the current file against a revision (emacs vc-ediff) |  |
| `set_buffer_file_coding_system` | Set the encoding this buffer is saved in (emacs set-buffer-file-coding-system) | **spacemacs** — normal: `` <C-x><ret>f ``, select: `` <C-x><ret>f ``, insert: `` <C-x><ret>f `` |
| `set_terminal_coding_system` | Set the coding system the hosted terminal's output is decoded with (emacs C-x RET t) | **spacemacs** — normal: `` <C-x><ret>t ``, select: `` <C-x><ret>t ``, insert: `` <C-x><ret>t `` |
| `set_keyboard_coding_system` | Set the coding system keys are encoded into for the hosted terminal (emacs C-x RET k) | **spacemacs** — normal: `` <C-x><ret>k ``, select: `` <C-x><ret>k ``, insert: `` <C-x><ret>k `` |
| `set_selection_coding_system` | Set the coding system used for the system clipboard (emacs C-x RET x) | **spacemacs** — normal: `` <C-x><ret>x ``, select: `` <C-x><ret>x ``, insert: `` <C-x><ret>x `` |
| `set_next_selection_coding_system` | Set the coding system for the next clipboard transfer only (emacs C-x RET X) | **spacemacs** — normal: `` <C-x><ret>X ``, select: `` <C-x><ret>X ``, insert: `` <C-x><ret>X `` |
| `set_buffer_process_coding_system` | Set the coding systems a subprocess's pipes are decoded/encoded with (emacs C-x RET p) | **spacemacs** — normal: `` <C-x><ret>p ``, select: `` <C-x><ret>p ``, insert: `` <C-x><ret>p `` |
| `set_file_name_coding_system` | Set the coding system file names are encoded into (emacs C-x RET F) | **spacemacs** — normal: `` <C-x><ret>F ``, select: `` <C-x><ret>F ``, insert: `` <C-x><ret>F `` |
| `prefer_coding_system` | Make a coding system the default for files that are opened (emacs C-x RET c / prefer-coding-system) |  |
| `universal_coding_system_argument` | Run the next file read or write with a coding system you name (emacs C-x RET c) | **spacemacs** — normal: `` <C-x><ret>c ``, select: `` <C-x><ret>c ``, insert: `` <C-x><ret>c `` |
| `recode_file_name` | Rename a file whose name was decoded with the wrong coding system (emacs recode-file-name) |  |
| `set_language_environment` | Choose a language environment, setting its default coding systems (emacs set-language-environment) |  |
| `set_locale_environment` | Take the default coding systems from the locale ($LC_ALL/$LC_CTYPE/$LANG) (emacs set-locale-environment) |  |
| `set_input_method` | Choose the input method this buffer composes characters with (emacs set-input-method, C-x RET C-\) |  |
| `activate_transient_input_method` | Turn an input method on for one character only (emacs activate-transient-input-method, C-x \) |  |
| `custom_prompt_customize_unsaved_options` | Ask whether to examine options customized this session but not saved (emacs custom-prompt-customize-unsaved-options) |  |
| `describe_input_method` | Show an input method's documentation and key table (emacs describe-input-method, C-h C-\) |  |
| `global_display_line_numbers_mode` | Toggle line numbers in every window (emacs global-display-line-numbers-mode) |  |
| `global_font_lock_mode` | Toggle syntax highlighting in every window (emacs global-font-lock-mode) |  |
| `global_visual_line_mode` | Toggle soft line wrapping in every window (emacs global-visual-line-mode) |  |
| `global_display_fill_column_indicator_mode` | Toggle the fill-column rule in every window (emacs global-display-fill-column-indicator-mode) |  |
| `global_text_scale_adjust` | Adjust the terminal font size for every window (emacs global-text-scale-adjust) |  |
| `diff_buffers` | Diff two open buffers (emacs diff-buffers) |  |
| `diff_backup` | Diff this file against its backup file (emacs diff-backup) | **spacemacs** — normal: `` <space>DB ``, select: `` <space>DB `` |
| `c_guess` | Guess this buffer's indent style and report it (emacs c-guess) |  |
| `c_guess_install` | Guess this buffer's indent style and apply it (emacs c-guess-install) |  |
| `plain_tex_mode` | Edit this buffer as plain TeX (emacs plain-tex-mode) |  |
| `slitex_mode` | Edit this buffer as SliTeX (emacs slitex-mode) |  |
| `doctex_mode` | Edit this buffer as DocTeX (emacs doctex-mode) |  |
| `bibtex_mode` | Edit this buffer as a BibTeX database (emacs bibtex-mode) |  |
| `nxml_mode` | Edit this buffer as XML (emacs nxml-mode) |  |
| `org_mode` | Edit this buffer as Org (emacs org-mode) |  |
| `outline_mode` | Outline major mode: C-c C-n/C-p headings, C-c C-d/C-s fold (emacs outline-mode) |  |
| `text_mode` | Text major mode (emacs text-mode) |  |
| `enriched_mode` | Enriched major mode: margins and justification (emacs enriched-mode) |  |
| `nroff_mode` | Nroff major mode: M-n/M-p text lines (emacs nroff-mode) |  |
| `nroff_electric_mode` | Toggle RET closing the nroff request the line opens (emacs nroff-electric-mode) |  |
| `change_log_mode` | ChangeLog major mode: entries for files and the functions in them (emacs change-log-mode) |  |
| `view_mode` | View major mode: SPC/DEL page the buffer; run again to leave (emacs view-mode) |  |
| `view_exit` | Leave view-mode, staying at the current position (emacs View-exit) |  |
| `view_quit` | Leave view-mode (emacs View-quit) |  |
| `fundamental_mode` | Leave the major mode; back to the base keymap (emacs fundamental-mode) |  |
| `scheme_mode` | Edit this buffer as Scheme (emacs scheme-mode) |  |
| `emacs_lisp_mode` | Edit this buffer as Emacs Lisp (emacs emacs-lisp-mode) |  |
| `lisp_interaction_mode` | Open a scratch buffer for evaluating elisp forms (emacs lisp-interaction-mode) |  |
| `iso_tex2iso` | Convert TeX escape sequences in the region to accented characters (emacs iso-tex2iso) |  |
| `iso_iso2tex` | Convert accented characters in the region to TeX escapes (emacs iso-iso2tex) |  |
| `iso_gtex2iso` | Convert German-TeX shorthands in the region to accented characters (emacs iso-gtex2iso) |  |
| `iso_iso2gtex` | Convert accented characters in the region to German-TeX shorthands (emacs iso-iso2gtex) |  |
| `time_stamp` | Update the Time-stamp template near the top of the buffer (emacs time-stamp) |  |
| `visit_tags_table` | Read an etags TAGS file and use it for tag lookups (emacs visit-tags-table) |  |
| `etags_regen_mode` | Generate the project's tags table automatically instead of running etags by hand (emacs etags-regen-mode) |  |
| `xref_etags_mode` | Resolve xref definitions through the tags table instead of the language server (emacs xref-etags-mode) |  |
| `list_tags` | List the tags one file of the tags table defines (emacs list-tags) |  |
| `find_tag_other_window` | Open a tag's definition in a split (emacs find-tag-other-window) |  |
| `tags_search` | Search every file in the tags table for a regexp (emacs tags-search) |  |
| `tags_next_file` | Visit the next file in the tags table (emacs tags-next-file) |  |
| `fileloop_continue` | Jump to the next match of the running multi-file search (emacs fileloop-continue) |  |
| `multi_isearch_buffers` | Search every open buffer for a string (emacs multi-isearch-buffers) |  |
| `multi_isearch_buffers_regexp` | Search every open buffer for a regexp (emacs multi-isearch-buffers-regexp) |  |
| `multi_isearch_files` | Search a list of files for a string (emacs multi-isearch-files) |  |
| `multi_isearch_files_regexp` | Search a list of files for a regexp (emacs multi-isearch-files-regexp) |  |
| `ispell_comments_and_strings` | Spell-check every comment and string in the buffer (emacs ispell-comments-and-strings) |  |
| `ispell_comment_or_string_at_point` | Spell-check the comment or string at point (emacs ispell-comment-or-string-at-point) |  |
| `ispell_hunspell_add_multi_dic` | Spell-check against several hunspell dictionaries at once (emacs ispell-hunspell-add-multi-dic) |  |
| `disable_command` | Put a command on the disabled list so it refuses to run (emacs disable-command) |  |
| `enable_command` | Take a command off the disabled list (emacs enable-command) |  |
| `emerge_buffers` | Merge two buffers into a merge buffer (emacs emerge-buffers) | **spacemacs** — normal: `` <space>Dmbb ``, select: `` <space>Dmbb `` |
| `emerge_buffers_with_ancestor` | Three-way merge two buffers against their ancestor (emacs emerge-buffers-with-ancestor) | **spacemacs** — normal: `` <space>Dmb3 ``, select: `` <space>Dmb3 `` |
| `emerge_files_with_ancestor` | Three-way merge two files against their ancestor (emacs emerge-files-with-ancestor) | **spacemacs** — normal: `` <space>Dmf3 ``, select: `` <space>Dmf3 `` |
| `text_scale_set` | Set the terminal font size to an absolute level (emacs text-scale-set) |  |
| `text_scale_adjust` | Keep adjusting the font size with +/-/0 (emacs text-scale-adjust) |  |
| `text_scale_mode` | Turn text scaling off and back on (emacs text-scale-mode) |  |
| `sunrise_sunset` | Report today's sunrise and sunset (emacs sunrise-sunset) |  |
| `lunar_phases` | Report this month's moon phases (emacs lunar-phases) |  |
| `add_change_log_entry_other_window` | Start a ChangeLog entry for this file and function in a split (emacs add-change-log-entry-other-window) | **spacemacs** — normal: `` <C-x>4a ``, select: `` <C-x>4a ``, insert: `` <C-x>4a `` |
| `diff_add_change_log_entries_other_window` | Start ChangeLog entries for every file in this patch (emacs diff-add-change-log-entries-other-window) |  |
| `change_log_goto_source` | Open the file and function the ChangeLog entry at point names (emacs change-log-goto-source) |  |
| `change_log_merge` | Merge another ChangeLog into this one, newest entries first (emacs change-log-merge) |  |
| `move_file_to_trash` | Move this file to the system trash and close the buffer (emacs move-file-to-trash) |  |
| `revert_buffer_with_fine_grain` | Re-read the file as an edit, keeping point and undo (emacs revert-buffer-with-fine-grain) |  |
| `revert_buffer_with_coding_system` | Re-read the file with a coding system you name (emacs revert-buffer-with-coding-system) | **spacemacs** — normal: `` <C-x><ret>r ``, select: `` <C-x><ret>r ``, insert: `` <C-x><ret>r `` |
| `clean_buffer_list` | Kill the buffers left untouched for three days (emacs clean-buffer-list) |  |
| `switch_to_buffer_other_tab` | Show a buffer in a new tab (emacs switch-to-buffer-other-tab) | **spacemacs** — normal: `` <C-x>tb ``, select: `` <C-x>tb ``, insert: `` <C-x>tb `` |
| `dired_other_tab` | Open Dired in a new tab (emacs dired-other-tab) | **spacemacs** — normal: `` <C-x>td ``, select: `` <C-x>td ``, insert: `` <C-x>td `` |
| `rot13_other_window` | Show this buffer ROT13'd in a split (emacs rot13-other-window) |  |
| `outline_hide_other` | Hide everything but the current entry, its parents and the top-level headings (emacs outline-hide-other) | **spacemacs** — normal: `` <C-c>@<C-o> ``, select: `` <C-c>@<C-o> ``, insert: `` <C-c>@<C-o> `` |
| `reposition_window` | Scroll so the whole function at point is on screen (emacs reposition-window) | **emacs, cua** — insert: `` <A-C-l> `` |
| `load` | Evaluate an elisp file in the embedded interpreter (emacs load) |  |
| `load_library` | Load an elisp library by name from the load path (emacs load-library) |  |
| `complete_keyword` | Complete the keyword before the cursor from the 'complete' sources (vim i_CTRL-N) |  |
| `show_paren_mode` | Toggle highlighting of the paren matching the one at point (emacs show-paren-mode) |  |
| `show_paren_local_mode` | Toggle paren-match highlighting in this buffer only (emacs show-paren-local-mode) |  |
| `hi_lock_mode` | Toggle display of the interactive highlight-regexp patterns (emacs hi-lock-mode) |  |
| `delete_selection_mode` | Toggle: typing with an active region replaces it (emacs delete-selection-mode) |  |
| `electric_indent_mode` | Toggle re-indenting the line a newline opens (emacs electric-indent-mode) |  |
| `electric_quote_mode` | Toggle self-inserting curved quotes (emacs electric-quote-mode) |  |
| `electric_layout_mode` | Toggle opening a new line after ';' '{' '}' (emacs electric-layout-mode) |  |
| `aggressive_indent_mode` | Toggle re-indenting the enclosing form after every edit (aggressive-indent-mode, SPC t I) |  |
| `which_function_mode` | Toggle showing the enclosing function on the status line (emacs which-function-mode) |  |
| `compilation_next_file` | Jump to the first error of the next file in the run output (emacs compilation-next-file) |  |
| `compilation_previous_file` | Jump to the first error of the previous file in the run output (emacs compilation-previous-file) |  |
| `compile_goto_error` | Visit the error the run output is parked on (emacs compile-goto-error) |  |
| `kill_compilation` | Kill the running compilation (emacs kill-compilation) |  |
| `rgrep` | Recursive grep under a directory, filtered by a file glob (emacs rgrep) |  |
| `lgrep` | Grep the files of one directory, not its subdirectories (emacs lgrep) |  |
| `grep_find` | Recursive grep from the project root (emacs grep-find) |  |
| `hs_hide_level` | Fold every block at a nesting depth you name (emacs hs-hide-level) | **spacemacs** — normal: `` <C-c>@<C-l> ``, select: `` <C-c>@<C-l> ``, insert: `` <C-c>@<C-l> `` |
| `hs_show_region` | Unfold every fold overlapping the selection (emacs hs-show-region) |  |
| `string_rectangle` | Replace each line's rectangle segment with a string you type (emacs string-rectangle) |  |
| `rectangle_mark_mode` | Toggle rectangular region selection (emacs rectangle-mark-mode) | **spacemacs** — normal: `` <C-x><space> ``, select: `` <C-x><space> ``, insert: `` <C-x><space> ``<br>**emacs** — insert: `` <C-x><space> ``<br>**cua** — normal: `` <C-ret> ``, select: `` <C-ret> ``, `` <C-X><space> ``, insert: `` <C-ret> ``, `` <C-x><space> `` |
| `rectangle_exchange_point_and_mark` | Move point to the opposite corner of the rectangle (emacs rectangle-exchange-point-and-mark) |  |
| `customize_group` | Edit the settings of one configuration group (emacs customize-group) |  |
| `customize_apropos` | Search the settings whose name or value matches a pattern (emacs customize-apropos) |  |
| `customize_changed` | List the settings changed from their built-in defaults (emacs customize-changed) |  |
| `customize_saved` | List the settings saved in the config file (emacs customize-saved) |  |
| `customize_unsaved` | List the settings set for this session but not saved (emacs customize-unsaved) |  |
| `customize_dirlocals` | Edit this project's directory-local settings (emacs customize-dirlocals) |  |
| `org_metaleft` | Org: promote the heading at point (emacs org-metaleft) |  |
| `org_metaright` | Org: demote the heading at point (emacs org-metaright) |  |
| `org_metaup` | Org: move the subtree at point above its previous sibling (emacs org-metaup) |  |
| `org_metadown` | Org: move the subtree at point below its next sibling (emacs org-metadown) |  |
| `org_shifttab` | Org: cycle the whole buffer show-all -> overview -> contents (emacs org-shifttab) |  |
| `c_ts_mode_set_style` | Report the C indentation style (emacs c-ts-mode-set-style) |  |
| `diff_ignore_whitespace_hunk` | Re-diff the hunk at point ignoring whitespace changes (emacs diff-ignore-whitespace-hunk) |  |
| `diff_refresh_hunk` | Re-diff the hunk at point (emacs diff-refresh-hunk) |  |
| `diff_ediff_patch` | Apply this patch and review it side by side (emacs diff-ediff-patch) | **spacemacs** — normal: `` <space>Dfp ``, select: `` <space>Dfp `` |
| `ediff_patch_buffer` | Patch a buffer with a patch buffer/file and review it (emacs ediff-patch-buffer, SPC D b p) |  |
| `diff_add_change_log_entry_other_window` | Start a ChangeLog entry for the hunk at point (emacs add-change-log-entry-other-window, in Diff mode) |  |
| `next_completion` | Move to the next completion candidate (emacs next-completion) |  |
| `previous_completion` | Move to the previous completion candidate (emacs previous-completion, M-<up>) |  |
| `term_line_mode` | Terminal: edit input locally, Enter sends the line (emacs term-line-mode) | **spacemacs** — normal: `` <C-c><C-j> ``, select: `` <C-c><C-j> ``, insert: `` <C-c><C-j> `` |
| `comint_completion_at_point` | Shell buffer: complete the command or file name before point (emacs completion-at-point in Shell mode) |  |
| `window_configuration_to_register` | Save the window configuration in a register (emacs window-configuration-to-register) | **spacemacs** — normal: `` <C-x>rw ``, select: `` <C-x>rw ``, insert: `` <C-x>rw `` |
| `do_auto_save` | Write every modified buffer to its auto-save file now (emacs do-auto-save) |  |
| `ffap_next` | Visit the next file named in this buffer (emacs ffap-next) |  |
| `lossage_size` | Set how many recent keystrokes view-lossage remembers (emacs lossage-size) |  |
| `tabulated_list_sort` | Sort the tabulated list by the column at point (emacs tabulated-list-sort) |  |
| `emerge_auto_advance` | Merge: choosing a side moves to the next difference (emacs emerge-auto-advance) |  |
| `emerge_skip_prefers` | Merge: step over the differences already decided (emacs emerge-skip-prefers) |  |
| `tabulated_list_narrow_current_column` | Narrow the tabulated-list column at point (emacs tabulated-list-narrow-current-column) |  |
| `tabulated_list_widen_current_column` | Widen the tabulated-list column at point (emacs tabulated-list-widen-current-column) |  |
| `ffap_menu` | Pick from the files named in this buffer and visit one (emacs ffap-menu) |  |
| `auto_save_mode` | Toggle auto-saving of the buffer (emacs auto-save-mode) |  |
| `recover_session` | Recover a file from the auto-save data of an interrupted session (emacs recover-session) |  |
| `frameset_to_register` | Save the frame's window configuration in a register (emacs frameset-to-register) | **spacemacs** — normal: `` <C-x>rf ``, select: `` <C-x>rf ``, insert: `` <C-x>rf `` |
| `shell_dynamic_complete_command` | Shell buffer: complete the command name before point from PATH (emacs shell-dynamic-complete-command) |  |
| `term_char_mode` | Terminal: send every key straight to the process (emacs term-char-mode) | **spacemacs** — normal: `` <C-c><C-k> ``, select: `` <C-c><C-k> ``, insert: `` <C-c><C-k> `` |
| `term_pager_toggle` | Terminal: stop output after each screenful (emacs term-pager-toggle) | **spacemacs** — normal: `` <C-c><C-q> ``, select: `` <C-c><C-q> ``, insert: `` <C-c><C-q> `` |
| `switch_to_completions` | Move into the list of completions (emacs switch-to-completions) |  |
| `previous_matching_history_element` | Recall the newest older history entry matching the regexp on the line (emacs previous-matching-history-element) |  |
| `next_matching_history_element` | Recall the oldest newer history entry matching the regexp on the line (emacs next-matching-history-element) |  |
| `line_number_mode` | Toggle the cursor position display in the mode line (emacs line-number-mode) |  |
| `column_number_mode` | Toggle the column number display in the mode line (emacs column-number-mode) |  |
| `size_indication_mode` | Toggle the buffer size display in the mode line (emacs size-indication-mode) |  |
| `display_time` | Toggle the clock in the mode line (emacs display-time) | **spacemacs** — normal: `` <space>tmt ``, select: `` <space>tmt `` |
| `display_battery_mode` | Toggle the battery charge in the mode line (emacs display-battery-mode) | **spacemacs** — normal: `` <space>tmb ``, select: `` <space>tmb `` |
| `make_local_variable` | Give a variable a buffer-local binding in this document (emacs make-local-variable) |  |
| `make_variable_buffer_local` | Make a variable become buffer-local whenever it is set (emacs make-variable-buffer-local) |  |
| `kill_local_variable` | Drop this document's local binding of a variable (emacs kill-local-variable) |  |
| `setq_default` | Set a variable's global value, seen by every document without a local binding (emacs setq-default) |  |
| `default_value` | Show a variable's global value, ignoring this document's local binding (emacs default-value) |  |
| `add_hook` | Add a function to a hook variable (emacs add-hook) |  |
| `remove_hook` | Take a function off a hook variable (emacs remove-hook) |  |
| `org_schedule` | Prompt for a date and add/update SCHEDULED: on the heading at point (emacs org-schedule) |  |
| `org_deadline` | Prompt for a date and add/update DEADLINE: on the heading at point (emacs org-deadline) |  |
| `org_clock_toggle` | Start or stop the org task clock on the heading at point (emacs org-clock-in/org-clock-out) |  |
| `outline_hide_by_heading_regexp` | Prompt for a regexp and fold the subtree of every heading matching it (emacs outline-hide-by-heading-regexp) |  |
| `outline_show_by_heading_regexp` | Prompt for a regexp and reveal the subtree of every heading matching it (emacs outline-show-by-heading-regexp) |  |
| `dbx` | Run the dbx debugger in a comint buffer (emacs M-x dbx) |  |
| `xdb` | Run the xdb debugger in a comint buffer (emacs M-x xdb) |  |
| `sdb` | Run the sdb debugger in a comint buffer (emacs M-x sdb) |  |
| `lldb` | Run the LLDB debugger in a comint buffer (emacs M-x lldb) |  |
| `jdb` | Run the Java debugger in a comint buffer (emacs M-x jdb) |  |
| `pdb` | Run the Python debugger on this file in a comint buffer (emacs M-x pdb) |  |
| `perldb` | Run the Perl debugger on this file in a comint buffer (emacs M-x perldb) |  |
| `guiler` | Run the Guile debugger in a comint buffer (emacs M-x guiler) |  |
| `run_lisp` | Run an inferior Common Lisp in a comint buffer (emacs run-lisp) |  |
| `run_scheme` | Run an inferior Scheme in a comint buffer (emacs run-scheme) |  |
| `lisp_eval_defun` | Send the top-level form at point to the inferior Lisp (emacs lisp-eval-defun) |  |
| `rebuild_mail_abbrevs` | Re-read the mail aliases in ~/.mailrc (emacs rebuild-mail-abbrevs) |  |
| `merge_mail_abbrevs` | Merge the mail aliases of another file into the table (emacs merge-mail-abbrevs) |  |
| `define_mail_abbrev` | Define a mail alias and save it to ~/.mailrc (emacs define-mail-abbrev) |  |
| `mail_abbrev_insert_alias` | Pick a mail alias and insert its addresses at point (emacs mail-abbrev-insert-alias) |  |
| `mail_abbrev_complete_alias` | Expand the mail alias before point into its addresses (emacs mail-abbrev-complete-alias) |  |
| `message_tab` | In a draft's headers complete the mail alias before point, else indent (emacs message-tab) |  |
| `message_goto_fcc` | Go to the draft's Fcc: header, creating it if absent (emacs message-goto-fcc) |  |
| `goto_reply_to` | Go to the draft's Reply-To: header, creating it if absent (emacs goto-reply-to) |  |
| `goto_followup_to` | Go to the draft's Followup-To: header, creating it if absent (emacs goto-followup-to) |  |
| `message_yank_original` | Insert the message being replied to, cited (emacs message-yank-original) |  |
| `message_yank_prefix` | Set the prefix message-yank-original puts on cited lines (emacs message-yank-prefix) |  |
| `mail_fill_yanked_message` | Refill the cited paragraphs of the yanked message (emacs mail-fill-yanked-message) |  |
| `add_dir_local_variable` | Set a variable for this whole tree in .dir-locals.el (emacs add-dir-local-variable) | **spacemacs** — normal: `` <space>fvd ``, select: `` <space>fvd `` |
| `delete_dir_local_variable` | Remove a variable from the tree's .dir-locals.el (emacs delete-dir-local-variable) |  |
| `copy_file_locals_to_dir_locals` | Copy this file's file-local variables into .dir-locals.el (emacs copy-file-locals-to-dir-locals) |  |
| `copy_dir_locals_to_file_locals` | Write the tree's directory-local variables into this file's Local Variables block (emacs copy-dir-locals-to-file-locals) |  |
| `copy_dir_locals_to_file_locals_prop_line` | Write the tree's directory-local variables into this file's -*- prop line (emacs copy-dir-locals-to-file-locals-prop-line) |  |
| `dir_locals_set_class_variables` | Register a named set of directory-local variables (emacs dir-locals-set-class-variables) |  |
| `dir_locals_set_directory_class` | Apply a registered directory-local class to a directory (emacs dir-locals-set-directory-class) |  |
| `top_level` | Close every open overlay and go back to the editor (emacs top-level) |  |
| `report_emacs_bug` | Compose a bug report with the version and system information (emacs report-emacs-bug) |  |
| `align_highlight_rule` | Highlight what an alignment rule matches (emacs align-highlight-rule) |  |
| `align_unhighlight_rule` | Remove the alignment rule highlighting (emacs align-unhighlight-rule) |  |
| `font_lock_add_keywords` | Highlight an extra regexp on top of the syntax highlighting (emacs font-lock-add-keywords) |  |
| `font_lock_remove_keywords` | Stop highlighting an extra regexp (emacs font-lock-remove-keywords) |  |
| `edit_tab_stops` | Edit the list of tab-stop columns (emacs edit-tab-stops) |  |
| `fortran_window_create` | Mark column 72, the fixed-form Fortran line limit (emacs fortran-window-create) |  |
| `fortran_window_create_momentarily` | Mark column 72 until the next key (emacs fortran-window-create-momentarily) |  |
| `recode_region` | Re-decode the region with the coding system it was really in (emacs recode-region) |  |
| `make_frame_command` | Create a new frame showing this buffer (emacs make-frame-command) | **spacemacs** — normal: `` <C-x>52 ``, `` <space>Fn ``, select: `` <C-x>52 ``, `` <space>Fn ``, insert: `` <C-x>52 `` |
| `clone_frame` | Create a new frame with a copy of this frame's layout (emacs clone-frame) | **spacemacs** — normal: `` <C-x>5c ``, select: `` <C-x>5c ``, insert: `` <C-x>5c `` |
| `delete_frame` | Delete the displayed frame (emacs delete-frame) | **spacemacs** — normal: `` <C-x>50 ``, select: `` <C-x>50 ``, insert: `` <C-x>50 `` |
| `delete_other_frames` | Delete every frame but this one (emacs delete-other-frames) | **spacemacs** — normal: `` <C-x>51 ``, `` <space>FD ``, select: `` <C-x>51 ``, `` <space>FD ``, insert: `` <C-x>51 `` |
| `other_frame` | Display the next frame (emacs other-frame) | **spacemacs** — normal: `` ]t ``, `` <C-x>5o ``, select: `` <C-x>5o ``, insert: `` <C-x>5o ``<br>**vim** — normal: `` ]t `` |
| `undelete_frame` | Bring back the most recently deleted frame (emacs undelete-frame) | **spacemacs** — normal: `` <C-x>5u ``, select: `` <C-x>5u ``, insert: `` <C-x>5u `` |
| `undelete_frame_mode` | Record deleted frames so undelete-frame can bring them back (emacs undelete-frame-mode) |  |
| `select_frame_by_name` | Pick a frame by name and display it (emacs select-frame-by-name) |  |
| `set_frame_name` | Rename the displayed frame (emacs set-frame-name) |  |
| `find_file_other_frame` | Open a file in a new frame (emacs find-file-other-frame) | **spacemacs** — normal: `` <C-x>5f ``, select: `` <C-x>5f ``, insert: `` <C-x>5f `` |
| `find_file_read_only_other_frame` | Open a file read-only in a new frame (emacs find-file-read-only-other-frame) | **spacemacs** — normal: `` <C-x>5r ``, select: `` <C-x>5r ``, insert: `` <C-x>5r `` |
| `switch_to_buffer_other_frame` | Display a buffer in a new frame (emacs switch-to-buffer-other-frame) | **spacemacs** — normal: `` <C-x>5b ``, select: `` <C-x>5b ``, insert: `` <C-x>5b `` |
| `display_buffer_other_frame` | Put a buffer in another frame without selecting it (emacs display-buffer-other-frame, SPC F B) |  |
| `dired_other_frame` | Open Dired in a new frame (emacs dired-other-frame) | **spacemacs** — normal: `` <C-x>5d ``, `` <space>FO ``, select: `` <C-x>5d ``, `` <space>FO ``, insert: `` <C-x>5d `` |
| `other_frame_prefix` | Display the next command's buffer in a new frame (emacs other-frame-prefix) | **spacemacs** — normal: `` <C-x>55 ``, select: `` <C-x>55 ``, insert: `` <C-x>55 `` |
| `other_window_prefix` | Display the next command's buffer in another window (emacs other-window-prefix) | **spacemacs** — normal: `` <C-x>44 ``, select: `` <C-x>44 ``, insert: `` <C-x>44 `` |
| `other_tab_prefix` | Display the next command's buffer in a new tab (emacs other-tab-prefix) | **spacemacs** — normal: `` <C-x>tt ``, select: `` <C-x>tt ``, insert: `` <C-x>tt `` |
| `display_buffer` | Show a buffer in another window without selecting it (emacs display-buffer) | **spacemacs** — normal: `` <C-x>4<C-o> ``, select: `` <C-x>4<C-o> ``, insert: `` <C-x>4<C-o> `` |
| `server_start` | Start the zmax server so clients can hand it files (emacs server-start) |  |
| `server_edit` | Finish with this buffer and let the waiting client return (emacs server-edit) | **spacemacs** — normal: `` <C-x># ``, select: `` <C-x># ``, insert: `` <C-x># `` |
| `server_edit_abort` | Abandon this buffer's edit and tell the waiting client (emacs server-edit-abort) |  |
| `server_generate_key` | Mint the shared key clients must authenticate with (emacs server-generate-key) |  |
| `server_eval_at` | Evaluate an expression in another zmax server (emacs server-eval-at) |  |
| `mouse_set_point` | Move point to the last click (emacs mouse-set-point) |  |
| `mouse_goto_tag` | Go to the definition of the symbol at the click (vim <C-LeftMouse>) |  |
| `mouse_pop_tag` | Pop the tag/jump stack (vim <C-RightMouse>, CTRL-T) |  |
| `mouse_search_word_forward` | Search forward for the word at the click (vim <S-LeftMouse>, *) |  |
| `mouse_search_word_backward` | Search backward for the word at the click (vim <S-RightMouse>, #) |  |
| `paste_after_reindent` | Paste after, indent shifted to the current line (vim ]p) | **spacemacs, vim** — normal: `` ]p `` |
| `paste_before_reindent` | Paste before, indent shifted to the current line (vim [p) | **spacemacs, vim** — normal: `` [p `` |
| `mouse_paste_before` | Paste before the click, adjusting indent (vim [<MiddleMouse>, [p) |  |
| `mouse_paste_after` | Paste after the click, adjusting indent (vim ]<MiddleMouse>, ]p) |  |
| `mouse_scroll_left` | Move the window mousescroll=hor:N columns left (vim <ScrollWheelLeft>) |  |
| `mouse_scroll_right` | Move the window mousescroll=hor:N columns right (vim <ScrollWheelRight>) |  |
| `mouse_scroll_page_left` | Move the window one page left (vim <S-ScrollWheelLeft>) |  |
| `mouse_scroll_page_right` | Move the window one page right (vim <S-ScrollWheelRight>) |  |
| `mouse_scroll_page_up` | Move the window one page up (vim <S-ScrollWheelUp>) |  |
| `mouse_scroll_page_down` | Move the window one page down (vim <S-ScrollWheelDown>) |  |
| `mouse_set_region` | Set the region from the drag's start to the click (emacs mouse-set-region) |  |
| `mouse_save_then_kill` | Extend the region to the click and copy it; again to kill it (emacs mouse-save-then-kill) |  |
| `mouse_yank_at_click` | Move point to the click and yank there (emacs mouse-yank-at-click) |  |
| `mouse_yank_primary` | Yank the primary selection at point (emacs mouse-yank-primary) |  |
| `mouse_wheel_mode` | Turn wheel scrolling on or off (emacs mouse-wheel-mode) |  |
| `mouse_buffer_menu` | The buffer menu, from the mouse (emacs mouse-buffer-menu) |  |
| `browse_url_at_mouse` | Open the URL under the mouse in a browser (emacs browse-url-at-mouse) |  |
| `mouse_start_secondary` | Anchor the secondary selection at the click (emacs mouse-start-secondary) |  |
| `mouse_set_secondary` | Set the secondary selection from the anchor to the click (emacs mouse-set-secondary) |  |
| `mouse_yank_secondary` | Insert the secondary selection at the click (emacs mouse-yank-secondary) |  |
| `mouse_secondary_save_then_kill` | Copy the secondary selection; again to kill it (emacs mouse-secondary-save-then-kill) |  |
| `overwrite_mode` | Typing replaces the character under point (emacs overwrite-mode) |  |
| `binary_overwrite_mode` | Overwrite mode that replaces newlines too (emacs binary-overwrite-mode) |  |
| `compose_mail_other_frame` | Open a mail draft in a new frame (emacs compose-mail-other-frame) | **spacemacs** — normal: `` <C-x>5m ``, select: `` <C-x>5m ``, insert: `` <C-x>5m `` |
| `context_menu_mode` | Turn the right-button popup menu on or off (emacs context-menu-mode) |  |
| `reveal_mode` | Reveal hidden text while point is inside it (emacs reveal-mode) |  |
| `desktop_save_mode` | Save the desktop on exit (emacs desktop-save-mode) |  |
| `visual_wrap_prefix_mode` | Indent soft-wrapped continuation lines under their line (emacs visual-wrap-prefix-mode) |  |
| `open_dribble_file` | Record every key into a file (emacs open-dribble-file) |  |
| `winner_mode` | Record window-layout changes so winner-undo can take them back (emacs winner-mode) |  |
