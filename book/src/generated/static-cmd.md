| Name | Description | Default keybinds |
| --- | --- | --- |
| `no_op` | Do nothing |  |
| `move_char_left` | Move left | normal: `` h ``, `` <C-h> ``, `` <left> ``, insert: `` <C-b> ``, `` <left> `` |
| `move_char_right` | Move right | normal: `` l ``, `` <right> ``, insert: `` <C-f> ``, `` <right> `` |
| `move_line_up` | Move up | normal: `` gk ``, `` g<up> `` |
| `move_line_down` | Move down | normal: `` gj ``, `` g<down> `` |
| `move_visual_line_up` | Move up | normal: `` k ``, `` <up> ``, `` <C-p> ``, insert: `` <up> ``, `` <C-g>k ``, `` <C-g><up> ``, `` <C-g><C-k> `` |
| `move_visual_line_down` | Move down | normal: `` j ``, `` <C-j> ``, `` <C-n> ``, `` <down> ``, insert: `` <C-g>j ``, `` <down> ``, `` <C-g><C-j> ``, `` <C-g><down> `` |
| `extend_char_left` | Extend left | select: `` h ``, `` <left> `` |
| `extend_char_right` | Extend right | select: `` l ``, `` <right> `` |
| `extend_line_up` | Extend up |  |
| `extend_line_down` | Extend down |  |
| `extend_visual_line_up` | Extend up | select: `` k ``, `` <up> `` |
| `extend_visual_line_down` | Extend down | select: `` j ``, `` <down> `` |
| `copy_selection_on_next_line` | Copy selection on next line | select: `` <C-v> `` |
| `copy_selection_on_prev_line` | Copy selection on previous line |  |
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
| `vim_move_next_word_start` | Move to start of next word (vim caret) | normal: `` w `` |
| `vim_move_prev_word_start` | Move to start of previous word (vim caret) | normal: `` b `` |
| `vim_move_next_word_end` | Move to end of next word (vim caret) | normal: `` e `` |
| `vim_move_prev_word_end` | Move to end of previous word (vim caret) | normal: `` ge `` |
| `vim_move_next_long_word_start` | Move to start of next long word (vim caret) | normal: `` W `` |
| `vim_move_prev_long_word_start` | Move to start of previous long word (vim caret) | normal: `` B `` |
| `vim_move_next_long_word_end` | Move to end of next long word (vim caret) | normal: `` E `` |
| `vim_move_prev_long_word_end` | Move to end of previous long word (vim caret) | normal: `` gE `` |
| `move_parent_node_end` | Move to end of the parent node | normal: `` <space>k$ ``, select: `` <space>k$ `` |
| `move_parent_node_start` | Move to beginning of the parent node | normal: `` <space>k0 ``, select: `` <space>k0 `` |
| `extend_next_word_start` | Extend to start of next word | select: `` w `` |
| `extend_prev_word_start` | Extend to start of previous word | select: `` b `` |
| `extend_next_word_end` | Extend to end of next word | select: `` e `` |
| `extend_prev_word_end` | Extend to end of previous word |  |
| `extend_next_long_word_start` | Extend to start of next long word | select: `` W `` |
| `extend_prev_long_word_start` | Extend to start of previous long word | select: `` B `` |
| `extend_next_long_word_end` | Extend to end of next long word | select: `` E `` |
| `extend_prev_long_word_end` | Extend to end of prev long word |  |
| `extend_next_sub_word_start` | Extend to start of next sub word |  |
| `extend_prev_sub_word_start` | Extend to start of previous sub word |  |
| `extend_next_sub_word_end` | Extend to end of next sub word |  |
| `extend_prev_sub_word_end` | Extend to end of prev sub word |  |
| `extend_parent_node_end` | Extend to end of the parent node |  |
| `extend_parent_node_start` | Extend to beginning of the parent node |  |
| `find_till_char` | Move till next occurrence of char | normal: `` t `` |
| `find_next_char` | Move to next occurrence of char | normal: `` f `` |
| `extend_till_char` | Extend till next occurrence of char | select: `` t `` |
| `extend_next_char` | Extend to next occurrence of char | select: `` f `` |
| `till_prev_char` | Move till previous occurrence of char | normal: `` T `` |
| `find_prev_char` | Move to previous occurrence of char | normal: `` F `` |
| `sneak_forward` | Sneak: jump forward to a two-character sequence |  |
| `sneak_backward` | Sneak: jump backward to a two-character sequence |  |
| `sneak_or_substitute_char` | Sneak forward, or substitute char when vim-sneak is off | normal: `` s `` |
| `sneak_or_substitute_line` | Sneak backward, or substitute line when vim-sneak is off | normal: `` S `` |
| `extend_till_prev_char` | Extend till previous occurrence of char | select: `` T `` |
| `extend_prev_char` | Extend to previous occurrence of char | select: `` F `` |
| `repeat_last_motion` | Repeat last motion | normal: `` ; ``, select: `` ; `` |
| `repeat_find_char_reverse` | Repeat last find in opposite direction (,) | normal: `` , ``, select: `` , `` |
| `replace` | Replace with new char | normal: `` r ``, select: `` r `` |
| `switch_case` | Switch (toggle) case | normal: `` ~ ``, select: `` ~ `` |
| `switch_to_uppercase` | Switch to uppercase | normal: `` <space>xU ``, select: `` <space>xU `` |
| `switch_to_lowercase` | Switch to lowercase | normal: `` <space>xu ``, select: `` <space>xu `` |
| `page_up` | Move page up | normal: `` z^ ``, `` <A-v> ``, `` <C-b> ``, `` <S-up> ``, `` <pageup> ``, `` <S-minus> ``, insert: `` <A-v> ``, `` <S-up> ``, `` <pageup> `` |
| `page_down` | Move page down | normal: `` z+ ``, `` <C-f> ``, `` <S-+> ``, `` <S-ret> ``, `` <S-down> ``, `` <pagedown> ``, insert: `` <S-down> ``, `` <pagedown> `` |
| `half_page_up` | Move half page up |  |
| `half_page_down` | Move half page down |  |
| `page_cursor_up` | Move page and cursor up |  |
| `page_cursor_down` | Move page and cursor down |  |
| `page_cursor_half_up` | Move page and cursor half up | normal: `` <C-u> `` |
| `page_cursor_half_down` | Move page and cursor half down | normal: `` <C-d> `` |
| `select_all` | Select whole document |  |
| `select_regex` | Select all regex matches inside selections |  |
| `select_all_instances` | Select all occurrences of the current selection in the buffer |  |
| `split_selection` | Split selections on regex matches |  |
| `split_selection_on_newline` | Split selection on newlines |  |
| `merge_selections` | Merge selections |  |
| `merge_consecutive_selections` | Merge consecutive selections |  |
| `search` | Search for regex pattern | normal: `` / ``, `` <C-s> `` |
| `rsearch` | Reverse search for regex pattern | normal: `` ? `` |
| `search_next` | Select next search match | normal: `` n ``, `` gn ``, `` <space>sH ``, select: `` <space>sH `` |
| `search_prev` | Select previous search match | normal: `` N ``, `` gN `` |
| `extend_search_next` | Add next search match to selection |  |
| `extend_search_prev` | Add previous search match to selection |  |
| `search_selection` | Use current selection as search pattern |  |
| `search_selection_detect_word_boundaries` | Use current selection as the search pattern, automatically wrapping with `\b` on word boundaries |  |
| `make_search_word_bounded` | Modify current search to make it word bounded |  |
| `global_search` | Global search in workspace folder | normal: `` <space>/ ``, `` <space>po ``, `` <space>ps ``, `` <space>sB ``, `` <space>sD ``, `` <space>sF ``, `` <space>sP ``, `` <space>sb ``, `` <space>sd ``, `` <space>sf ``, `` <space>sp ``, `` <space>ss ``, `` <space>saA ``, `` <space>saB ``, `` <space>saD ``, `` <space>saF ``, `` <space>saP ``, `` <space>saa ``, `` <space>sab ``, `` <space>sad ``, `` <space>saf ``, `` <space>sap ``, `` <space>sgB ``, `` <space>sgF ``, `` <space>sgG ``, `` <space>sgb ``, `` <space>sgd ``, `` <space>sgf ``, `` <space>sgg ``, `` <space>sgp ``, `` <space>skB ``, `` <space>skD ``, `` <space>skF ``, `` <space>skP ``, `` <space>skb ``, `` <space>skd ``, `` <space>skf ``, `` <space>skp ``, `` <space>srB ``, `` <space>srD ``, `` <space>srF ``, `` <space>srP ``, `` <space>srR ``, `` <space>srb ``, `` <space>srd ``, `` <space>srf ``, `` <space>srp ``, `` <space>srr ``, select: `` <space>/ ``, `` <space>po ``, `` <space>ps ``, `` <space>sB ``, `` <space>sD ``, `` <space>sF ``, `` <space>sP ``, `` <space>sb ``, `` <space>sd ``, `` <space>sf ``, `` <space>sp ``, `` <space>ss ``, `` <space>saA ``, `` <space>saB ``, `` <space>saD ``, `` <space>saF ``, `` <space>saP ``, `` <space>saa ``, `` <space>sab ``, `` <space>sad ``, `` <space>saf ``, `` <space>sap ``, `` <space>sgB ``, `` <space>sgF ``, `` <space>sgG ``, `` <space>sgb ``, `` <space>sgd ``, `` <space>sgf ``, `` <space>sgg ``, `` <space>sgp ``, `` <space>skB ``, `` <space>skD ``, `` <space>skF ``, `` <space>skP ``, `` <space>skb ``, `` <space>skd ``, `` <space>skf ``, `` <space>skp ``, `` <space>srB ``, `` <space>srD ``, `` <space>srF ``, `` <space>srP ``, `` <space>srR ``, `` <space>srb ``, `` <space>srd ``, `` <space>srf ``, `` <space>srp ``, `` <space>srr `` |
| `extend_line` | Select current line, if already selected, extend to another line based on the anchor | normal: `` <space>kV ``, select: `` <space>kV `` |
| `extend_line_below` | Select current line, if already selected, extend to next line |  |
| `extend_line_above` | Select current line, if already selected, extend to previous line |  |
| `select_line_above` | Select current line, if already selected, extend or shrink line above based on the anchor |  |
| `select_line_below` | Select current line, if already selected, extend or shrink line below based on the anchor |  |
| `extend_to_line_bounds` | Extend selection to line bounds | select: `` V `` |
| `shrink_to_line_bounds` | Shrink selection to line bounds |  |
| `delete_selection` | Delete selection | normal: `` x ``, `` <del> `` |
| `delete_selection_noyank` | Delete selection without yanking |  |
| `change_selection` | Change selection | select: `` c ``, `` s `` |
| `change_selection_noyank` | Change selection without yanking |  |
| `collapse_selection` | Collapse selection into single cursor | normal: `` <esc> `` |
| `flip_selections` | Flip selection cursor and anchor | select: `` O ``, `` o `` |
| `ensure_selections_forward` | Ensure all selections face forward |  |
| `insert_mode` | Insert before selection | normal: `` i ``, `` <ins> ``, `` <space>ki ``, select: `` <space>ki `` |
| `append_mode` | Append after selection | normal: `` a ``, select: `` A `` |
| `replace_mode` | Enter Replace mode (overtype) | normal: `` R ``, `` gR ``, insert: `` <ins> `` |
| `command_mode` | Enter command mode | normal: `` : ``, `` gQ ``, `` <space>: ``, `` <space>k: ``, select: `` : ``, `` <space>: ``, `` <space>k: `` |
| `file_picker` | Open file picker | normal: `` <space>fA ``, `` <space>fL ``, `` <space>ff ``, `` <space>fl ``, `` <space>pf ``, `` <space>ph ``, `` <space>pp ``, select: `` <space>fA ``, `` <space>fL ``, `` <space>ff ``, `` <space>fl ``, `` <space>pf ``, `` <space>ph ``, `` <space>pp `` |
| `file_picker_in_current_buffer_directory` | Open file picker at current buffer's directory |  |
| `file_picker_in_current_directory` | Open file picker at current working directory |  |
| `file_explorer` | Open file explorer in workspace root | normal: `` <space>ad ``, `` <space>af ``, `` <space>ft ``, `` <space>pd ``, `` <space>pt ``, `` <space>atrd ``, `` <space>atrr ``, select: `` <space>ad ``, `` <space>af ``, `` <space>ft ``, `` <space>pd ``, `` <space>pt ``, `` <space>atrd ``, `` <space>atrr `` |
| `file_explorer_in_current_buffer_directory` | Open file explorer at current buffer's directory | normal: `` <space>fd ``, `` <space>fj ``, `` <space>jD ``, `` <space>jd ``, select: `` <space>fd ``, `` <space>fj ``, `` <space>jD ``, `` <space>jd `` |
| `file_explorer_in_current_directory` | Open file explorer at current working directory |  |
| `code_action` | Perform code action | normal: `` <space>la ``, select: `` <space>la `` |
| `buffer_picker` | Open buffer picker | normal: `` <space>bW ``, `` <space>bb ``, `` <space>pb ``, `` <space>b.b ``, select: `` <space>bW ``, `` <space>bb ``, `` <space>pb ``, `` <space>b.b `` |
| `jumplist_picker` | Open jumplist picker | normal: `` <space>jj ``, select: `` <space>jj `` |
| `register_picker` | Browse registers and paste the chosen one | normal: `` <space>re ``, `` <space>rr ``, `` <space>ry ``, select: `` <space>re ``, `` <space>rr ``, `` <space>ry `` |
| `marks_picker` | Fuzzy-pick a vim mark and jump to it (:Marks) | normal: `` <space>fb ``, `` <space>rm ``, select: `` <space>fb ``, `` <space>rm `` |
| `buffer_line_picker` | Fuzzy-search lines in the current buffer (:BLines) | normal: `` <space>sL ``, select: `` <space>sL `` |
| `command_history_picker` | Fuzzy-pick and run a past command line (:History:) | normal: `` <space>r: ``, select: `` <space>r: `` |
| `search_history_picker` | Fuzzy-pick and re-run a past search (:History/) | normal: `` <space>r/ ``, select: `` <space>r/ `` |
| `unicode_picker` | Fuzzy-pick a character/digraph and insert it (helm-unicode) | normal: `` <space>iu ``, select: `` <space>iu `` |
| `git_file_log_picker` | Commit log for the current file (:BCommits) | normal: `` <space>gt ``, `` <space>gfl ``, select: `` <space>gt ``, `` <space>gfl `` |
| `git_repo_log_picker` | Commit log for the whole repo (:Commits) | normal: `` <space>gL ``, select: `` <space>gL `` |
| `theme_picker` | Open fuzzy theme picker with live preview | normal: `` <space>Tc ``, select: `` <space>Tc `` |
| `wrap_sexp` | Wrap the selection in parentheses | normal: `` <space>kw ``, select: `` <space>kw `` |
| `symbol_picker` | Open symbol picker | normal: `` <space>ji ``, `` <space>pg ``, `` <space>sj ``, select: `` <space>ji ``, `` <space>pg ``, `` <space>sj `` |
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
| `harpoon_menu` | Open the harpoon marks menu | normal: `` <space>Hh ``, `` <space>Hl ``, select: `` <space>Hh ``, `` <space>Hl `` |
| `harpoon_remove` | Unpin the current file from harpoon | normal: `` <space>Hd ``, select: `` <space>Hd `` |
| `select_references_to_symbol_under_cursor` | Select symbol references | normal: `` <space>se ``, `` <space>sh ``, select: `` <space>se ``, `` <space>sh `` |
| `workspace_symbol_picker` | Open workspace symbol picker | normal: `` <space>jI ``, `` <space>sS ``, select: `` <space>jI ``, `` <space>sS `` |
| `syntax_workspace_symbol_picker` | Open workspace symbol picker from syntax information |  |
| `lsp_or_syntax_workspace_symbol_picker` | Open workspace symbol picker from LSP or syntax information |  |
| `diagnostics_picker` | Open diagnostic picker | normal: `` <space>el ``, select: `` <space>el `` |
| `workspace_diagnostics_picker` | Open workspace diagnostic picker | normal: `` <space>eL ``, select: `` <space>eL `` |
| `last_picker` | Open last picker | normal: `` <space>' ``, `` <space>rl ``, `` <space>sl ``, select: `` <space>' ``, `` <space>rl ``, `` <space>sl `` |
| `insert_at_line_start` | Insert at start of line | normal: `` I ``, `` gI `` |
| `insert_at_line_end` | Insert at end of line | normal: `` A `` |
| `open_below` | Open new line below selection | normal: `` o `` |
| `open_above` | Open new line above selection | normal: `` O `` |
| `normal_mode` | Enter normal mode | normal: `` <C-c> ``, `` <C-\><C-g> ``, `` <C-\><C-n> `` |
| `select_mode` | Enter selection extend mode | normal: `` v ``, `` gh ``, `` <C-v> ``, `` g<C-h> ``, `` <C-space> ``, `` <space>kv ``, `` <space>k<C-v> ``, select: `` <space>kv ``, `` <space>k<C-v> `` |
| `exit_select_mode` | Exit selection mode |  |
| `goto_definition` | Goto definition | normal: `` g] ``, `` gd ``, `` <C-]> ``, `` <C-w>] ``, `` ]<C-d> ``, `` g<C-]> ``, `` <C-w>g] ``, `` <space>gd ``, `` <space>jf ``, `` <space>jv ``, `` <space>w] ``, `` <C-w><C-]> ``, `` <space>mgg ``, `` <space>wg] ``, `` <C-w>g<C-]> ``, `` <space>w<C-]> ``, `` <space>wg<C-]> ``, select: `` <C-]> ``, `` <space>gd ``, `` <space>jf ``, `` <space>jv ``, `` <space>w] ``, `` <space>mgg ``, `` <space>wg] ``, `` <space>w<C-]> ``, `` <space>wg<C-]> `` |
| `goto_declaration` | Goto declaration | normal: `` gD ``, `` <C-w>i ``, `` [<C-d> ``, `` <space>gD ``, `` <space>wi ``, `` <C-w><C-i> ``, `` <space>w<C-i> ``, select: `` <space>gD ``, `` <space>wi ``, `` <space>w<C-i> `` |
| `add_newline_above` | Add newline above |  |
| `add_newline_below` | Add newline below |  |
| `goto_type_definition` | Goto type definition | normal: `` gy ``, `` <space>gy ``, select: `` <space>gy `` |
| `goto_implementation` | Goto implementation | normal: `` <space>gi ``, select: `` <space>gi `` |
| `goto_file_start` | Goto line number `<n>` else file start | normal: `` gg ``, `` <A-lt> ``, `` <C-home> ``, insert: `` <A-lt> ``, `` <C-home> `` |
| `goto_file_end` | Goto file end | insert: `` <A-gt> ``, `` <C-end> `` |
| `extend_to_file_start` | Extend to line number `<n>` else file start | select: `` gg `` |
| `extend_to_file_end` | Extend to file end |  |
| `goto_file` | Goto files/URLs in selections | normal: `` [f ``, `` ]f ``, `` gf ``, `` gx ``, `` <C-w>gF ``, `` <C-w>gf ``, `` <space>fF ``, `` <space>fo ``, `` <space>jU ``, `` <space>ju ``, `` <space>xo ``, `` <space>wgF ``, `` <space>wgf ``, select: `` <space>fF ``, `` <space>fo ``, `` <space>jU ``, `` <space>ju ``, `` <space>xo ``, `` <space>wgF ``, `` <space>wgf `` |
| `goto_file_hsplit` | Goto files in selections (hsplit) | normal: `` <C-w>F ``, `` <C-w>f ``, `` <space>wF ``, `` <space>wf ``, `` <C-w><C-f> ``, `` <space>w<C-f> ``, select: `` <space>wF ``, `` <space>wf ``, `` <space>w<C-f> `` |
| `goto_file_vsplit` | Goto files in selections (vsplit) |  |
| `goto_reference` | Goto references | normal: `` gr `` |
| `goto_window_top` | Goto window top | normal: `` H `` |
| `goto_window_center` | Goto window center | normal: `` M `` |
| `goto_window_bottom` | Goto window bottom | normal: `` L `` |
| `goto_last_accessed_file` | Goto last accessed file | normal: `` <C-^> ``, `` <C-w>^ ``, `` g<tab> ``, `` <C-tab> ``, `` <space>w^ ``, `` <C-w><C-^> ``, `` <C-w>g<tab> ``, `` <space><tab> ``, `` <space>w<C-^> ``, `` <space>wg<tab> ``, select: `` <space>w^ ``, `` <space><tab> ``, `` <space>w<C-^> ``, `` <space>wg<tab> `` |
| `goto_last_modified_file` | Goto last modified file | normal: `` <space>pr ``, select: `` <space>pr `` |
| `goto_last_modification` | Goto last modification | normal: `` g, ``, `` g. ``, `` g; `` |
| `goto_line` | Goto line |  |
| `goto_last_line` | Goto last line | normal: `` G ``, `` <A-gt> ``, `` <C-end> `` |
| `extend_to_last_line` | Extend to last line | select: `` G ``, `` ge `` |
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
| `git_diff` | Open side-by-side diff vs HEAD | normal: `` <space>g= ``, select: `` <space>g= `` |
| `resolve_conflicts` | Resolve merge conflicts (3-way) | normal: `` <space>gm ``, `` <space>gcr ``, select: `` <space>gm ``, `` <space>gcr `` |
| `git_status` | Magit status | normal: `` <space>gs ``, select: `` <space>gs `` |
| `org_cycle` | Org: toggle subtree fold | normal: `` <space>mz ``, `` <space>m<tab> ``, select: `` <space>mz ``, `` <space>m<tab> `` |
| `org_todo` | Org: cycle TODO keyword | normal: `` <space>mt ``, select: `` <space>mt `` |
| `org_priority` | Org: cycle priority cookie | normal: `` <space>mp ``, select: `` <space>mp `` |
| `org_promote` | Org: promote heading | normal: `` <space>mH ``, select: `` <space>mH `` |
| `org_demote` | Org: demote heading | normal: `` <space>ml ``, select: `` <space>ml `` |
| `org_next_heading` | Org: next heading | normal: `` <space>mj ``, select: `` <space>mj `` |
| `org_prev_heading` | Org: previous heading | normal: `` <space>mk ``, select: `` <space>mk `` |
| `org_fold_all` | Org: fold all headings | normal: `` <space>ma ``, select: `` <space>ma `` |
| `org_unfold_all` | Org: unfold all | normal: `` <space>mA ``, select: `` <space>mA `` |
| `org_agenda` | Org: open agenda | normal: `` <space>aoa ``, select: `` <space>aoa `` |
| `org_capture` | Org: capture note | normal: `` <space>aoc ``, select: `` <space>aoc `` |
| `goto_first_change` | Goto first change |  |
| `goto_last_change` | Goto last change | normal: `` <space>jc ``, select: `` <space>jc `` |
| `goto_line_start` | Goto line start | normal: `` 0 ``, `` g0 ``, `` <home> ``, `` g<home> ``, `` <space>j0 ``, select: `` <space>j0 ``, insert: `` <home> `` |
| `goto_line_end` | Goto line end | normal: `` $ ``, `` g$ ``, `` gl ``, `` <end> ``, `` g<end> ``, `` <space>j$ ``, select: `` <space>j$ `` |
| `goto_column` | Goto column | normal: `` \| `` |
| `extend_to_column` | Extend to column |  |
| `goto_next_buffer` | Goto next buffer | normal: `` ]b ``, `` gt ``, `` <C-w>gt ``, `` <space>bn ``, `` <space>b.n ``, `` <space>wgt ``, select: `` <space>bn ``, `` <space>b.n ``, `` <space>wgt `` |
| `goto_previous_buffer` | Goto previous buffer | normal: `` [b ``, `` gT ``, `` <C-w>gT ``, `` <space>bp ``, `` <space>b.N ``, `` <space>b.p ``, `` <space>wgT ``, select: `` <space>bp ``, `` <space>b.N ``, `` <space>b.p ``, `` <space>wgT `` |
| `goto_line_end_newline` | Goto newline at line end | insert: `` <end> `` |
| `goto_first_nonwhitespace` | Goto first non-blank in line | normal: `` ^ ``, `` _ ``, `` g^ ``, `` <A-m> `` |
| `trim_selections` | Trim whitespace from selections |  |
| `extend_to_line_start` | Extend to line start | select: `` 0 ``, `` <home> `` |
| `extend_to_first_nonwhitespace` | Extend to first non-blank in line | select: `` ^ ``, `` gh `` |
| `extend_to_line_end` | Extend to line end | select: `` $ ``, `` g$ ``, `` gl ``, `` <end> `` |
| `extend_to_line_end_newline` | Extend to line end |  |
| `signature_help` | Show signature help | normal: `` <space>ls ``, select: `` <space>ls `` |
| `smart_tab` | Insert tab if all cursors have all whitespace to their left; otherwise, run a separate command. |  |
| `insert_tab` | Insert tab char |  |
| `insert_newline` | Insert newline char | insert: `` <C-j> ``, `` <ret> `` |
| `insert_char_interactive` | Insert an interactively-chosen char | insert: `` <C-q> ``, `` <C-v> `` |
| `append_char_interactive` | Append an interactively-chosen char |  |
| `delete_char_backward` | Delete previous char | normal: `` X ``, insert: `` <C-h> ``, `` <backspace> `` |
| `delete_char_forward` | Delete next char | insert: `` <del> `` |
| `delete_word_backward` | Delete previous word | insert: `` <C-w> ``, `` <A-backspace> `` |
| `delete_word_forward` | Delete next word | normal: `` <A-d> ``, insert: `` <A-d> `` |
| `kill_to_line_start` | Delete till start of line | insert: `` <C-u> `` |
| `kill_to_line_end` | Delete till end of line |  |
| `undo` | Undo change | normal: `` U ``, `` u ``, `` <C-/> ``, `` <C-_> ``, `` <space>ku ``, select: `` <space>ku ``, insert: `` <C-/> ``, `` <C-_> `` |
| `redo` | Redo change | normal: `` <C-r> ``, `` <space>k<C-r> ``, select: `` <space>k<C-r> `` |
| `earlier` | Move backward in history | normal: `` g<minus> `` |
| `later` | Move forward in history | normal: `` g+ `` |
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
| `paste_clipboard_after` | Paste clipboard after selections |  |
| `paste_clipboard_before` | Paste clipboard before selections |  |
| `paste_primary_clipboard_after` | Paste primary clipboard after selections |  |
| `paste_primary_clipboard_before` | Paste primary clipboard before selections |  |
| `indent` | Indent selection | normal: `` == ``, `` <gt><gt> ``, `` <space>x<tab> ``, select: `` <space>x<tab> ``, insert: `` <C-t> `` |
| `unindent` | Unindent selection | normal: `` <lt><lt> ``, insert: `` <C-d> `` |
| `format_selections` | Format selection | normal: `` <A-q> ``, `` <space>j+ ``, `` <space>j= ``, `` <space>lf ``, `` <space>xjc ``, `` <space>xjf ``, `` <space>xjl ``, `` <space>xjn ``, `` <space>xjr ``, select: `` <space>j+ ``, `` <space>j= ``, `` <space>lf ``, `` <space>xjc ``, `` <space>xjf ``, `` <space>xjl ``, `` <space>xjn ``, `` <space>xjr `` |
| `join_selections` | Join lines inside selection | normal: `` J ``, `` <A-^> ``, `` <space>kJ ``, select: `` <space>kJ `` |
| `join_selections_space` | Join lines inside selection and select spaces |  |
| `keep_selections` | Keep selections matching regex |  |
| `remove_selections` | Remove selections matching regex |  |
| `align_selections` | Align selections in column | normal: `` <space>xa& ``, `` <space>xa( ``, `` <space>xa) ``, `` <space>xa, ``, `` <space>xa. ``, `` <space>xa: ``, `` <space>xa; ``, `` <space>xa= ``, `` <space>xaL ``, `` <space>xa[ ``, `` <space>xa] ``, `` <space>xaa ``, `` <space>xac ``, `` <space>xal ``, `` <space>xam ``, `` <space>xar ``, `` <space>xa{ ``, `` <space>xa} ``, select: `` <space>xa& ``, `` <space>xa( ``, `` <space>xa) ``, `` <space>xa, ``, `` <space>xa. ``, `` <space>xa: ``, `` <space>xa; ``, `` <space>xa= ``, `` <space>xaL ``, `` <space>xa[ ``, `` <space>xa] ``, `` <space>xaa ``, `` <space>xac ``, `` <space>xal ``, `` <space>xam ``, `` <space>xar ``, `` <space>xa{ ``, `` <space>xa} `` |
| `keep_primary_selection` | Keep primary selection |  |
| `remove_primary_selection` | Remove primary selection |  |
| `completion` | Invoke completion popup | insert: `` <A-/> ``, `` <C-n> ``, `` <C-p> ``, `` <C-x>s ``, `` <C-x><C-]> ``, `` <C-x><C-d> ``, `` <C-x><C-f> ``, `` <C-x><C-i> ``, `` <C-x><C-k> ``, `` <C-x><C-l> ``, `` <C-x><C-n> ``, `` <C-x><C-o> ``, `` <C-x><C-p> ``, `` <C-x><C-r> ``, `` <C-x><C-s> ``, `` <C-x><C-t> ``, `` <C-x><C-u> ``, `` <C-x><C-v> `` |
| `hover` | Show docs for item under cursor | normal: `` K ``, `` <C-w>} ``, `` <C-w>g} ``, `` <space>lk ``, `` <space>w} ``, `` <space>hda ``, `` <space>mhh ``, `` <space>wg} ``, select: `` K ``, `` <space>lk ``, `` <space>w} ``, `` <space>hda ``, `` <space>mhh ``, `` <space>wg} `` |
| `toggle_comments` | Comment/uncomment selections | normal: `` <A-;> ``, `` <space>; ``, `` <space>cP ``, `` <space>cT ``, `` <space>cc ``, `` <space>ch ``, `` <space>cp ``, `` <space>ct ``, select: `` <space>; ``, `` <space>cP ``, `` <space>cT ``, `` <space>cc ``, `` <space>ch ``, `` <space>cp ``, `` <space>ct `` |
| `toggle_line_comments` | Line comment/uncomment selections | normal: `` <space>cL ``, `` <space>cl ``, select: `` <space>cL ``, `` <space>cl `` |
| `toggle_block_comments` | Block comment/uncomment selections | normal: `` <space>cb ``, select: `` <space>cb `` |
| `rotate_selections_forward` | Rotate selections forward |  |
| `rotate_selections_backward` | Rotate selections backward |  |
| `rotate_selection_contents_forward` | Rotate selection contents forward |  |
| `rotate_selection_contents_backward` | Rotate selections contents backward |  |
| `reverse_selection_contents` | Reverse selections contents |  |
| `expand_selection` | Expand selection to parent syntax node | normal: `` <space>v ``, `` <space>kU ``, `` <space>kk ``, select: `` <space>v ``, `` <space>kU ``, `` <space>kk `` |
| `shrink_selection` | Shrink selection to previously expanded syntax node | normal: `` <space>kj ``, select: `` <space>kj `` |
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
| `jump_view_up` | Jump to split above | normal: `` <C-w>k ``, `` <C-w>t ``, `` <C-w>.k ``, `` <C-w><up> ``, `` <space>wk ``, `` <space>wt ``, `` <C-w><C-k> ``, `` <C-w><C-t> ``, `` <space>w.k ``, `` <space>w<up> ``, `` <space>w<C-k> ``, `` <space>w<C-t> ``, select: `` <space>wk ``, `` <space>wt ``, `` <space>w.k ``, `` <space>w<up> ``, `` <space>w<C-k> ``, `` <space>w<C-t> `` |
| `jump_view_down` | Jump to split below | normal: `` <C-w>b ``, `` <C-w>j ``, `` <C-w>.j ``, `` <space>wb ``, `` <space>wj ``, `` <C-w><C-b> ``, `` <C-w><C-j> ``, `` <space>w.j ``, `` <C-w><down> ``, `` <space>w<C-b> ``, `` <space>w<C-j> ``, `` <space>w<down> ``, select: `` <space>wb ``, `` <space>wj ``, `` <space>w.j ``, `` <space>w<C-b> ``, `` <space>w<C-j> ``, `` <space>w<down> `` |
| `swap_view_right` | Swap with right split | normal: `` <C-w>L ``, `` <C-w>.L ``, `` <space>wL ``, `` <space>w.L ``, select: `` <space>wL ``, `` <space>w.L `` |
| `swap_view_left` | Swap with left split | normal: `` <C-w>H ``, `` <C-w>.H ``, `` <space>wH ``, `` <space>w.H ``, select: `` <space>wH ``, `` <space>w.H `` |
| `swap_view_up` | Swap with split above | normal: `` <C-w>K ``, `` <C-w>.K ``, `` <space>wK ``, `` <space>w.K ``, select: `` <space>wK ``, `` <space>w.K `` |
| `swap_view_down` | Swap with split below | normal: `` <C-w>J ``, `` <C-w>.J ``, `` <space>wJ ``, `` <space>w.J ``, select: `` <space>wJ ``, `` <space>w.J `` |
| `transpose_view` | Transpose splits | normal: `` <C-w>M ``, `` <C-w>x ``, `` <space>wM ``, `` <space>wx ``, `` <C-w><C-x> ``, `` <space>w<C-x> ``, select: `` <space>wM ``, `` <space>wx ``, `` <space>w<C-x> `` |
| `rotate_view` | Goto next window | normal: `` <C-w>p ``, `` <C-w>r ``, `` <C-w>w ``, `` <C-w>.o ``, `` <C-w>.r ``, `` <C-w>.w ``, `` <space>wp ``, `` <space>wr ``, `` <space>ww ``, `` <C-w><C-p> ``, `` <C-w><C-r> ``, `` <C-w><C-w> ``, `` <C-w><tab> ``, `` <space>b.o ``, `` <space>w.o ``, `` <space>w.r ``, `` <space>w.w ``, `` <space>w<C-p> ``, `` <space>w<C-r> ``, `` <space>w<C-w> ``, `` <space>w<tab> ``, select: `` <space>wp ``, `` <space>wr ``, `` <space>ww ``, `` <space>b.o ``, `` <space>w.o ``, `` <space>w.r ``, `` <space>w.w ``, `` <space>w<C-p> ``, `` <space>w<C-r> ``, `` <space>w<C-w> ``, `` <space>w<tab> `` |
| `rotate_view_reverse` | Goto previous window | normal: `` <C-w>R ``, `` <C-w>W ``, `` <C-w>.R ``, `` <space>wR ``, `` <space>wW ``, `` <space>w.R ``, select: `` <space>wR ``, `` <space>wW ``, `` <space>w.R `` |
| `hsplit` | Horizontal bottom split | normal: `` <C-w>S ``, `` <C-w>s ``, `` <C-w>.S ``, `` <C-w>.s ``, `` <space>wS ``, `` <space>ws ``, `` <C-w><C-s> ``, `` <space>w.S ``, `` <space>w.s ``, `` <C-w>.<minus> ``, `` <space>w<C-s> ``, `` <space>w.<minus> ``, select: `` <space>wS ``, `` <space>ws ``, `` <space>w.S ``, `` <space>w.s ``, `` <space>w<C-s> ``, `` <space>w.<minus> `` |
| `hsplit_new` | Horizontal bottom split scratch buffer | normal: `` <C-w>n ``, `` <space>Fn ``, `` <space>wn ``, `` <C-w><C-n> ``, `` <space>bNh ``, `` <space>bNj ``, `` <space>bNk ``, `` <space>bNl ``, `` <space>w<C-n> ``, select: `` <space>Fn ``, `` <space>wn ``, `` <space>bNh ``, `` <space>bNj ``, `` <space>bNk ``, `` <space>bNl ``, `` <space>w<C-n> `` |
| `vsplit` | Vertical right split | normal: `` <C-w>/ ``, `` <C-w>2 ``, `` <C-w>3 ``, `` <C-w>4 ``, `` <C-w>V ``, `` <C-w>v ``, `` <C-w>./ ``, `` <C-w>.V ``, `` <C-w>.v ``, `` <space>w/ ``, `` <space>w2 ``, `` <space>w3 ``, `` <space>w4 ``, `` <space>wV ``, `` <space>wv ``, `` <C-w><C-v> ``, `` <space>w./ ``, `` <space>w.V ``, `` <space>w.v ``, `` <space>w<C-v> ``, `` <space>u<space>w2 ``, `` <space>u<space>w3 ``, `` <space>u<space>w4 ``, select: `` <space>w/ ``, `` <space>w2 ``, `` <space>w3 ``, `` <space>w4 ``, `` <space>wV ``, `` <space>wv ``, `` <space>w./ ``, `` <space>w.V ``, `` <space>w.v ``, `` <space>w<C-v> ``, `` <space>u<space>w2 ``, `` <space>u<space>w3 ``, `` <space>u<space>w4 `` |
| `vsplit_new` | Vertical right split scratch buffer |  |
| `wclose` | Close window | normal: `` <C-w>D ``, `` <C-w>c ``, `` <C-w>d ``, `` <C-w>q ``, `` <C-w>.d ``, `` <space>cd ``, `` <space>wD ``, `` <space>wc ``, `` <space>wd ``, `` <space>wq ``, `` <C-w><C-d> ``, `` <C-w><C-q> ``, `` <space>w.d ``, `` <space>w<C-d> ``, `` <space>w<C-q> ``, `` <space>u<space>wd ``, select: `` <space>cd ``, `` <space>wD ``, `` <space>wc ``, `` <space>wd ``, `` <space>wq ``, `` <space>w.d ``, `` <space>w<C-d> ``, `` <space>w<C-q> ``, `` <space>u<space>wd `` |
| `wonly` | Close windows except current | normal: `` <C-w>1 ``, `` <C-w>_ ``, `` <C-w>m ``, `` <C-w>o ``, `` <C-w>.D ``, `` <C-w>._ ``, `` <C-w>\| ``, `` <C-w>.\| ``, `` <space>w1 ``, `` <space>w_ ``, `` <space>wm ``, `` <space>wo ``, `` <C-w><C-o> ``, `` <space>w.D ``, `` <space>w._ ``, `` <space>w\| ``, `` <space>w.\| ``, `` <space>w<C-o> ``, `` <space>u<space>w1 ``, select: `` <space>w1 ``, `` <space>w_ ``, `` <space>wm ``, `` <space>wo ``, `` <space>w.D ``, `` <space>w._ ``, `` <space>w\| ``, `` <space>w.\| ``, `` <space>w<C-o> ``, `` <space>u<space>w1 `` |
| `select_register` | Select register | normal: `` " `` |
| `insert_register` | Insert register | insert: `` <C-r> `` |
| `copy_between_registers` | Copy between two registers |  |
| `align_view_middle` | Align view middle |  |
| `align_view_top` | Align view top | normal: `` zt `` |
| `align_view_center` | Align view center | normal: `` zz ``, `` <C-l> ``, `` <C-w>.z ``, `` <space>b.z ``, `` <space>w.z ``, select: `` <space>b.z ``, `` <space>w.z `` |
| `align_view_bottom` | Align view bottom | normal: `` zb `` |
| `scroll_up` | Scroll view up | normal: `` <C-y> ``, insert: `` <C-x><C-y> `` |
| `scroll_down` | Scroll view down | normal: `` <C-e> ``, insert: `` <C-x><C-e> `` |
| `scroll_column_left` | Scroll view left one column (zh) | normal: `` zh ``, `` z<left> `` |
| `scroll_column_right` | Scroll view right one column (zl) | normal: `` zl ``, `` z<right> `` |
| `scroll_half_column_left` | Scroll view left half a screen (zH) | normal: `` zH ``, `` ze `` |
| `scroll_half_column_right` | Scroll view right half a screen (zL) | normal: `` zL ``, `` zs `` |
| `resize_view_wider` | Make current window wider (CTRL-W >) | normal: `` <C-w>.] ``, `` <C-w><gt> ``, `` <space>w.] ``, `` <space>w<gt> ``, select: `` <space>w.] ``, `` <space>w<gt> `` |
| `resize_view_narrower` | Make current window narrower (CTRL-W <) | normal: `` <C-w>[ ``, `` <C-w>.[ ``, `` <C-w><lt> ``, `` <space>w[ ``, `` <space>w.[ ``, `` <space>w<lt> ``, select: `` <space>w[ ``, `` <space>w.[ ``, `` <space>w<lt> `` |
| `resize_view_taller` | Make current window taller (CTRL-W +) | normal: `` <C-w>+ ``, `` <C-w>.} ``, `` <space>w+ ``, `` <space>w.} ``, select: `` <space>w+ ``, `` <space>w.} `` |
| `resize_view_shorter` | Make current window shorter (CTRL-W -) | normal: `` <C-w>{ ``, `` <C-w>.{ ``, `` <space>w{ ``, `` <space>w.{ ``, `` <C-w><minus> ``, `` <space>w<minus> ``, select: `` <space>w{ ``, `` <space>w.{ ``, `` <space>w<minus> `` |
| `resize_view_equalize` | Make all windows equal size (CTRL-W =) | normal: `` <C-w>= ``, `` <space>w= ``, select: `` <space>w= `` |
| `rot13` | ROT13-encode the selection (g?) |  |
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
| `randomize_lines_in_region` | Randomize lines in the selection (SPC x l r) | normal: `` <space>xlr ``, select: `` <space>xlr `` |
| `randomize_words_in_region` | Randomize words in the selection (SPC x w r) | normal: `` <space>xwr ``, select: `` <space>xwr `` |
| `copy_char_below` | Insert the character below the cursor (i_CTRL-E) | insert: `` <C-e> `` |
| `copy_char_above` | Insert the character above the cursor (i_CTRL-Y) | insert: `` <C-y> `` |
| `file_info` | Show file name and cursor position (CTRL-G) | normal: `` <C-g> `` |
| `document_stats` | Show document line/word/char counts (g CTRL-G) | normal: `` g<C-g> ``, select: `` g<C-g> `` |
| `git_blame_line` | Show git blame for the current line (g b) | normal: `` <space>gM ``, `` <space>gb ``, select: `` <space>gM ``, `` <space>gb `` |
| `git_branch_picker` | Pick a git branch and check it out |  |
| `preferences` | Open the unified Preferences window | normal: `` <space>, ``, select: `` <space>, `` |
| `help` | Open the inline Help browser | normal: `` <space>bH ``, `` <space>h? ``, `` <space>hc ``, `` <space>hh ``, `` <space>hk ``, select: `` <space>bH ``, `` <space>h? ``, `` <space>hc ``, `` <space>hh ``, `` <space>hk `` |
| `dashboard` | Open the system-stats Dashboard (Preferences) | normal: `` <space>bh ``, select: `` <space>bh `` |
| `search_in_files` | Open the project-wide Find in Files panel |  |
| `terminal` | Open an integrated terminal (PTY shell) | normal: `` <space>p' ``, select: `` <space>p' `` |
| `run_config_manager` | Manage run/debug configurations | normal: `` <space>Rc ``, `` <space>Re ``, `` <space>cm ``, `` <space>pi ``, select: `` <space>Rc ``, `` <space>Re ``, `` <space>cm ``, `` <space>pi `` |
| `run_active_config` | Run the active run configuration | normal: `` <F5> ``, `` <space>Rr ``, `` <space>cC ``, `` <space>pc ``, `` <space>pu ``, select: `` <space>Rr ``, `` <space>cC ``, `` <space>pc ``, `` <space>pu `` |
| `clear_run_output` | Clear the Run tool window output | normal: `` <space>Rl ``, `` <space>Rx ``, select: `` <space>Rl ``, `` <space>Rx `` |
| `rerun_last_run` | Re-run the last command in the Run console | normal: `` <space>RR ``, `` <space>cr ``, select: `` <space>RR ``, `` <space>cr `` |
| `run_next_error` | Jump to the next file:line in the run output | normal: `` <space>Rn ``, select: `` <space>Rn `` |
| `run_prev_error` | Jump to the previous file:line in the run output | normal: `` <space>Rp ``, select: `` <space>Rp `` |
| `reveal_in_tree` | Reveal the current file in the project tree | normal: `` <space>pv ``, select: `` <space>pv `` |
| `toggle_auto_reveal` | Toggle always-select-opened-file (autoscroll from source) | normal: `` <space>pV ``, select: `` <space>pV `` |
| `focus_file_tree` | Focus the project file tree panel | normal: `` <space>Wp ``, `` <space>Wt ``, select: `` <space>Wp ``, `` <space>Wt `` |
| `focus_structure` | Focus the structure/symbol outline panel | normal: `` <space>Wo ``, `` <space>Ws ``, select: `` <space>Wo ``, `` <space>Ws `` |
| `focus_problems` | Focus the problems/diagnostics panel | normal: `` <space>We ``, select: `` <space>We `` |
| `focus_run_console` | Focus the Run console (scroll output with j/k/PgUp/PgDn) | normal: `` <space>Wr ``, select: `` <space>Wr `` |
| `focus_git_panel` | Focus the Git changes panel (j/k select, Enter opens) | normal: `` <space>Wg ``, `` <space>gG ``, select: `` <space>Wg ``, `` <space>gG `` |
| `focus_ci_panel` | Focus the CI status panel (GitHub Actions runs; Enter opens in browser) | normal: `` <space>Wc ``, select: `` <space>Wc `` |
| `toggle_bottom_zoom` | Maximize / restore the bottom panel | normal: `` <space>Wm ``, select: `` <space>Wm `` |
| `toggle_drawer_mid` | Fold / unfold the middle column of the bottom drawer | normal: `` <space>Wf ``, select: `` <space>Wf `` |
| `toggle_ide` | Toggle the IDE workbench (Zen / focus mode) | normal: `` <space>z ``, select: `` <space>z `` |
| `settings_page` | Open the settings page (config.toml editor) | normal: `` <space>S ``, select: `` <space>S `` |
| `goto_next_spell_error` | Move to the next misspelled word (]s) | normal: `` ]s `` |
| `goto_prev_spell_error` | Move to the previous misspelled word ([s) | normal: `` [s `` |
| `spell_add_good` | Mark word under cursor as correctly spelled (zg) | normal: `` zG ``, `` zg `` |
| `spell_add_bad` | Mark word under cursor as misspelled (zw) | normal: `` zW ``, `` zw `` |
| `spell_undo` | Undo a zg/zw for the word under cursor (zug) | normal: `` zuG ``, `` zuW ``, `` zug ``, `` zuw `` |
| `spell_suggest` | Show spelling suggestions for the word under cursor (z=) | normal: `` z= `` |
| `fold_create` | Create a fold over the selection (zf) |  |
| `fold_toggle` | Toggle fold under cursor (za) | normal: `` zA ``, `` za ``, `` zi `` |
| `fold_open` | Open fold under cursor (zo) | normal: `` zO ``, `` zo ``, `` zv ``, `` zx `` |
| `fold_close` | Close fold under cursor (zc) | normal: `` zC ``, `` zc `` |
| `fold_open_all` | Open all folds (zR) | normal: `` zR ``, `` zX ``, `` zn ``, `` zr ``, `` <space>nw ``, select: `` <space>nw `` |
| `fold_close_all` | Close all folds (zM) | normal: `` zM ``, `` zN ``, `` zm `` |
| `fold_delete` | Delete fold under cursor (zd) | normal: `` zD ``, `` zd `` |
| `fold_delete_all` | Delete all folds (zE) | normal: `` zE `` |
| `narrow_to_region` | Narrow the view to the selected region (SPC n r) | normal: `` <space>nr ``, select: `` <space>nr `` |
| `kmacro_add_counter` | Add [count] to the keyboard-macro counter (SPC K c a) | normal: `` <space>Kca ``, select: `` <space>Kca `` |
| `kmacro_insert_counter` | Insert the macro counter value, then increment (SPC K c c) | normal: `` <space>Kcc ``, select: `` <space>Kcc `` |
| `toggle_readonly` | Toggle the buffer's read-only (writable) state (SPC b w) | normal: `` <space>bw ``, select: `` <space>bw `` |
| `paredit_slurp_forward` | Paredit: slurp the next s-expression forward (SPC k s) | normal: `` <space>ks ``, `` <space>k`s ``, select: `` <space>ks ``, `` <space>k`s `` |
| `paredit_barf_forward` | Paredit: barf the last s-expression forward (SPC k b) | normal: `` <space>kb ``, select: `` <space>kb `` |
| `paredit_slurp_backward` | Paredit: slurp the previous s-expression backward (SPC k S) | normal: `` <space>kS ``, select: `` <space>kS `` |
| `paredit_barf_backward` | Paredit: barf the first s-expression backward (SPC k B) | normal: `` <space>kB ``, select: `` <space>kB `` |
| `paredit_splice` | Paredit: splice/unwrap the enclosing s-expression (SPC k W) | normal: `` <space>kW ``, select: `` <space>kW `` |
| `paredit_raise` | Paredit: raise the current s-expression (SPC k r) | normal: `` <space>kr ``, select: `` <space>kr `` |
| `paredit_transpose` | Paredit: transpose the s-expressions around point (SPC k t) | normal: `` <space>kt ``, `` <space>k`p ``, `` <space>k`t ``, select: `` <space>kt ``, `` <space>k`p ``, `` <space>k`t `` |
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
| `duplicate_selection_down` | Duplicate current line(s) downward |  |
| `duplicate_selection_up` | Duplicate current line(s) upward |  |
| `move_text_line_down` | Move current line(s) down past the next line |  |
| `move_text_line_up` | Move current line(s) up past the previous line |  |
| `count_selection` | Count chars/words/lines in selection | normal: `` <space>xc ``, `` <space>xwc ``, select: `` <space>xc ``, `` <space>xwc `` |
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
| `set_mark` | Set mark (m{a-z}) | normal: `` m `` |
| `goto_mark` | Goto mark exact (`{a-z}) | normal: `` ` ``, `` g` `` |
| `goto_mark_line` | Goto mark line ('{a-z}) | normal: `` ' ``, `` g' `` |
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
| `goto_next_test` | Goto next test | normal: `` <space>pa ``, select: `` <space>pa `` |
| `goto_prev_test` | Goto previous test |  |
| `goto_next_xml_element` | Goto next (X)HTML element |  |
| `goto_prev_xml_element` | Goto previous (X)HTML element |  |
| `goto_next_entry` | Goto next pairing |  |
| `goto_prev_entry` | Goto previous pairing |  |
| `goto_next_paragraph` | Goto next paragraph | normal: `` } ``, `` ]] ``, select: `` } `` |
| `goto_prev_paragraph` | Goto previous paragraph | normal: `` { ``, `` [[ ``, select: `` { `` |
| `move_sentence_forward` | Move to next sentence | normal: `` ) ``, select: `` ) `` |
| `move_sentence_backward` | Move to previous sentence | normal: `` ( ``, select: `` ( `` |
| `dap_launch` | Launch debug target | normal: `` <S-F5> ``, `` <space>dd ``, select: `` <space>dd `` |
| `dap_restart` | Restart debugging session | normal: `` <space>dr ``, select: `` <space>dr `` |
| `dap_toggle_breakpoint` | Toggle breakpoint | normal: `` <F9> ``, `` <space>db ``, select: `` <space>db `` |
| `dap_continue` | Continue program execution | normal: `` <space>dc ``, select: `` <space>dc `` |
| `dap_pause` | Pause program execution | normal: `` <space>dp ``, select: `` <space>dp `` |
| `dap_step_in` | Step in | normal: `` <F11> ``, `` <space>di ``, select: `` <space>di `` |
| `dap_step_out` | Step out | normal: `` <S-F11> ``, `` <space>do ``, select: `` <space>do `` |
| `dap_next` | Step to next | normal: `` <F10> ``, `` <space>dn ``, select: `` <space>dn `` |
| `dap_variables` | List variables | normal: `` <space>dv ``, select: `` <space>dv `` |
| `dap_terminate` | End debug session | normal: `` <space>dq ``, select: `` <space>dq `` |
| `dap_edit_condition` | Edit breakpoint condition on current line |  |
| `dap_edit_log` | Edit breakpoint log message on current line |  |
| `dap_switch_thread` | Switch current thread |  |
| `dap_switch_stack_frame` | Switch stack frame |  |
| `dap_enable_exceptions` | Enable exception breakpoints |  |
| `dap_disable_exceptions` | Disable exception breakpoints |  |
| `shell_pipe` | Pipe selections through shell command |  |
| `shell_pipe_to` | Pipe selections into shell command ignoring output |  |
| `shell_insert_output` | Insert shell command output before selections |  |
| `shell_append_output` | Append shell command output after selections |  |
| `shell_keep_pipe` | Filter selections with shell predicate |  |
| `suspend` | Suspend and return to shell | normal: `` <C-z> `` |
| `rename_symbol` | Rename symbol | normal: `` <space>lr ``, select: `` <space>lr `` |
| `increment` | Increment item under cursor | normal: `` <C-a> ``, `` <space>n+ ``, `` <space>n= ``, select: `` <C-a> ``, `` g<C-a> ``, `` <space>n+ ``, `` <space>n= `` |
| `decrement` | Decrement item under cursor | normal: `` <C-x> ``, `` <space>n_ ``, `` <space>n<minus> ``, select: `` <C-x> ``, `` g<C-x> ``, `` <space>n_ ``, `` <space>n<minus> `` |
| `record_macro` | Record macro |  |
| `replay_macro` | Replay macro | normal: `` Q `` |
| `command_palette` | Open command palette | normal: `` <F1> ``, `` <A-x> ``, `` <space>? ``, `` <space>eh ``, `` <space>ev ``, `` <space>h. ``, `` <space>hf ``, `` <space>hi ``, `` <space>hl ``, `` <space>hm ``, `` <space>hn ``, `` <space>hp ``, `` <space>hr ``, `` <space>hdb ``, `` <space>hdf ``, `` <space>hdk ``, `` <space>hdl ``, `` <space>hdm ``, `` <space>hdp ``, `` <space>hds ``, `` <space>hdt ``, `` <space>hdv ``, `` <space>hdx ``, `` <space><space> ``, `` <space>h<space> ``, select: `` <space>? ``, `` <space>eh ``, `` <space>ev ``, `` <space>h. ``, `` <space>hf ``, `` <space>hi ``, `` <space>hl ``, `` <space>hm ``, `` <space>hn ``, `` <space>hp ``, `` <space>hr ``, `` <space>hdb ``, `` <space>hdf ``, `` <space>hdk ``, `` <space>hdl ``, `` <space>hdm ``, `` <space>hdp ``, `` <space>hds ``, `` <space>hdt ``, `` <space>hdv ``, `` <space>hdx ``, `` <space><space> ``, `` <space>h<space> `` |
| `repl` | Open the embedded-language REPL (elisp/viml/stryke/awk/zsh) | normal: `` <space>ar ``, select: `` <space>ar `` |
| `goto_word` | Jump to a two-character label | normal: `` <space>jl ``, `` <space>jw ``, select: `` <space>jl ``, `` <space>jw `` |
| `extend_to_word` | Extend to a two-character label |  |
| `goto_next_tabstop` | Goto next snippet placeholder |  |
| `goto_prev_tabstop` | Goto next snippet placeholder |  |
| `emmet_expand` | Expand emmet/zen HTML abbreviation (or Tab) | insert: `` <tab> `` |
| `snippet_expand` | Expand the user snippet whose trigger precedes the cursor |  |
| `rotate_selections_first` | Make the first selection your primary one |  |
| `rotate_selections_last` | Make the last selection your primary one |  |
