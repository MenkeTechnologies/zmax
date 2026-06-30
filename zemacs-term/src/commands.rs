pub(crate) mod dap;
pub(crate) mod lsp;
pub(crate) mod org;
/// Embedded scripting host. The real `scripting/` module (which pulls the
/// interpreter crates) is compiled only with the `scripting` feature; otherwise
/// a stub exposing the same entry points reports that scripting was not built in.
#[cfg(feature = "scripting")]
pub mod scripting;
#[cfg(not(feature = "scripting"))]
#[path = "commands/scripting_stub.rs"]
pub mod scripting;
pub(crate) mod syntax;
pub(crate) mod typed;

pub use dap::*;
use futures_util::FutureExt;
pub use lsp::*;
pub use syntax::*;
use tui::{
    text::{Span, Spans},
    widgets::Cell,
};
pub use typed::*;
use zemacs_event::status;
use zemacs_stdx::{
    path::{self, find_paths},
    rope::{self, RopeSliceExt},
};
use zemacs_vcs::{CommitInfo, FileChange, Hunk};

use zemacs_core::{
    char_idx_at_visual_offset,
    chars::char_is_word,
    command_line::{self, Args},
    comment,
    doc_formatter::TextFormat,
    encoding, find_workspace,
    graphemes::{self, next_grapheme_boundary},
    history::UndoKind,
    increment,
    indent::{self, IndentStyle},
    line_ending::{get_line_ending_of_str, line_end_char_index},
    match_brackets,
    movement::{self, move_vertically_visual, Direction},
    object, pos_at_coords,
    regex::{self, Regex},
    search::{self},
    selection, surround,
    syntax::config::{BlockCommentToken, LanguageServerFeature},
    text_annotations::{Overlay, TextAnnotations},
    textobject,
    unicode::width::UnicodeWidthChar,
    visual_offset_from_block, Deletion, LineEnding, Position, Range, Rope, RopeReader, RopeSlice,
    Selection, SmallVec, Syntax, Tendril, Transaction,
};
use zemacs_view::{
    document::{FormatterError, Mode, SCRATCH_BUFFER_NAME},
    editor::{Action, Motion},
    expansion,
    info::Info,
    input::KeyEvent,
    keyboard::KeyCode,
    theme::Style,
    tree,
    view::View,
    Document, DocumentId, Editor, ViewId,
};

use anyhow::{anyhow, bail, ensure, Context as _};
use arc_swap::access::DynAccess;
use insert::*;
use movement::Movement;

use crate::{
    compositor::{self, Component, Compositor},
    filter_picker_entry,
    job::Callback,
    ui::{self, overlay::overlaid, Picker, PickerColumn, Popup, Prompt, PromptEvent},
};

use crate::job::{self, Jobs};
use std::{
    char::{ToLowercase, ToUppercase},
    cmp::Ordering,
    collections::{HashMap, HashSet},
    error::Error,
    fmt,
    future::Future,
    io::Read,
    num::NonZeroUsize,
};

use std::{
    borrow::Cow,
    path::{Path, PathBuf},
};

use once_cell::sync::Lazy;
use serde::de::{self, Deserialize, Deserializer};
use zemacs_stdx::Url;

use grep_regex::RegexMatcherBuilder;
use grep_searcher::{sinks, BinaryDetection, SearcherBuilder};
use ignore::{DirEntry, WalkBuilder, WalkState};

pub type OnKeyCallback = Box<dyn FnOnce(&mut Context, KeyEvent)>;
#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum OnKeyCallbackKind {
    PseudoPending,
    Fallback,
}

pub struct Context<'a> {
    pub register: Option<char>,
    pub count: Option<NonZeroUsize>,
    pub editor: &'a mut Editor,

    pub callback: Vec<crate::compositor::Callback>,
    pub on_next_key_callback: Option<(OnKeyCallback, OnKeyCallbackKind)>,
    pub jobs: &'a mut Jobs,
}

impl Context<'_> {
    /// Push a new component onto the compositor.
    pub fn push_layer(&mut self, component: Box<dyn Component>) {
        self.callback
            .push(Box::new(|compositor: &mut Compositor, _| {
                compositor.push(component)
            }));
    }

    /// Call `replace_or_push` on the Compositor
    pub fn replace_or_push_layer<T: Component>(&mut self, id: &'static str, component: T) {
        self.callback
            .push(Box::new(move |compositor: &mut Compositor, _| {
                compositor.replace_or_push(id, component);
            }));
    }

    #[inline]
    pub fn on_next_key(
        &mut self,
        on_next_key_callback: impl FnOnce(&mut Context, KeyEvent) + 'static,
    ) {
        self.on_next_key_callback = Some((
            Box::new(on_next_key_callback),
            OnKeyCallbackKind::PseudoPending,
        ));
    }

    #[inline]
    pub fn on_next_key_fallback(
        &mut self,
        on_next_key_callback: impl FnOnce(&mut Context, KeyEvent) + 'static,
    ) {
        self.on_next_key_callback =
            Some((Box::new(on_next_key_callback), OnKeyCallbackKind::Fallback));
    }

    #[inline]
    pub fn callback<T, F>(
        &mut self,
        call: impl Future<Output = zemacs_lsp::Result<T>> + 'static + Send,
        callback: F,
    ) where
        T: Send + 'static,
        F: FnOnce(&mut Editor, &mut Compositor, T) + Send + 'static,
    {
        self.jobs.callback(make_job_callback(call, callback));
    }

    /// Returns 1 if no explicit count was provided
    #[inline]
    pub fn count(&self) -> usize {
        self.count.map_or(1, |v| v.get())
    }

    /// Waits on all pending jobs, and then tries to flush all pending write
    /// operations for all documents.
    pub fn block_try_flush_writes(&mut self) -> anyhow::Result<()> {
        compositor::Context {
            editor: self.editor,
            jobs: self.jobs,
            scroll: None,
        }
        .block_try_flush_writes()
    }
}

#[inline]
fn make_job_callback<T, F>(
    call: impl Future<Output = zemacs_lsp::Result<T>> + 'static + Send,
    callback: F,
) -> std::pin::Pin<Box<impl Future<Output = Result<Callback, anyhow::Error>>>>
where
    T: Send + 'static,
    F: FnOnce(&mut Editor, &mut Compositor, T) + Send + 'static,
{
    Box::pin(async move {
        let response = call.await?;
        let call: job::Callback = Callback::EditorCompositor(Box::new(
            move |editor: &mut Editor, compositor: &mut Compositor| {
                callback(editor, compositor, response)
            },
        ));
        Ok(call)
    })
}

use zemacs_view::{align_view, Align};

/// MappableCommands are commands that can be bound to keys, executable in
/// normal, insert or select mode.
///
/// There are three kinds:
///
/// * Static: commands usually bound to keys and used for editing, movement,
///   etc., for example `move_char_left`.
/// * Typable: commands executable from command mode, prefixed with a `:`,
///   for example `:write!`.
/// * Macro: a sequence of keys to execute, for example `@miw`.
#[derive(Clone)]
pub enum MappableCommand {
    Typable {
        name: String,
        args: String,
        doc: String,
    },
    Static {
        name: &'static str,
        fun: fn(cx: &mut Context),
        doc: &'static str,
    },
    Macro {
        name: String,
        keys: Vec<KeyEvent>,
    },
}

macro_rules! static_commands {
    ( $($name:ident, $doc:literal,)* ) => {
        $(
            #[allow(non_upper_case_globals)]
            pub const $name: Self = Self::Static {
                name: stringify!($name),
                fun: $name,
                doc: $doc
            };
        )*

        pub const STATIC_COMMAND_LIST: &'static [Self] = &[
            $( Self::$name, )*
        ];
    }
}

impl MappableCommand {
    pub fn execute(&self, cx: &mut Context) {
        match &self {
            Self::Typable { name, args, doc: _ } => {
                if let Some(command) = typed::TYPABLE_COMMAND_MAP.get(name.as_str()) {
                    let mut cx = compositor::Context {
                        editor: cx.editor,
                        jobs: cx.jobs,
                        scroll: None,
                    };
                    if let Err(e) =
                        typed::execute_command(&mut cx, command, args, PromptEvent::Validate)
                    {
                        cx.editor.set_error(format!("{}", e));
                    }
                } else {
                    cx.editor.set_error(format!("no such command: '{name}'"));
                }
            }
            Self::Static { fun, .. } => (fun)(cx),
            Self::Macro { keys, .. } => {
                // Protect against recursive macros.
                if cx.editor.macro_replaying.contains(&'@') {
                    cx.editor.set_error(
                        "Cannot execute macro because the [@] register is already playing a macro",
                    );
                    return;
                }
                cx.editor.macro_replaying.push('@');
                let keys = keys.clone();
                cx.callback.push(Box::new(move |compositor, cx| {
                    for key in keys.into_iter() {
                        compositor.handle_event(&compositor::Event::Key(key), cx);
                    }
                    cx.editor.macro_replaying.pop();
                }));
            }
        }
    }

    pub fn name(&self) -> &str {
        match &self {
            Self::Typable { name, .. } => name,
            Self::Static { name, .. } => name,
            Self::Macro { name, .. } => name,
        }
    }

    pub fn doc(&self) -> &str {
        match &self {
            Self::Typable { doc, .. } => doc,
            Self::Static { doc, .. } => doc,
            Self::Macro { name, .. } => name,
        }
    }

    #[rustfmt::skip]
    static_commands!(
        no_op, "Do nothing",
        move_char_left, "Move left",
        move_char_right, "Move right",
        move_line_up, "Move up",
        move_line_down, "Move down",
        drag_line_down, "Drag the current line down (SPC x . j)",
        drag_line_up, "Drag the current line up (SPC x . k)",
        toggle_test_file, "Toggle between implementation and test file (SPC p a)",
        fold_comments, "Fold multi-line comment blocks (SPC c h)",
        move_visual_line_up, "Move up",
        move_visual_line_down, "Move down",
        extend_char_left, "Extend left",
        extend_char_right, "Extend right",
        extend_line_up, "Extend up",
        extend_line_down, "Extend down",
        extend_visual_line_up, "Extend up",
        extend_visual_line_down, "Extend down",
        copy_selection_on_next_line, "Copy selection on next line",
        copy_selection_on_prev_line, "Copy selection on previous line",
        move_next_word_start, "Move to start of next word",
        move_prev_word_start, "Move to start of previous word",
        move_next_word_end, "Move to end of next word",
        move_prev_word_end, "Move to end of previous word",
        move_next_long_word_start, "Move to start of next long word",
        move_prev_long_word_start, "Move to start of previous long word",
        move_next_long_word_end, "Move to end of next long word",
        move_prev_long_word_end, "Move to end of previous long word",
        move_next_sub_word_start, "Move to start of next sub word",
        move_prev_sub_word_start, "Move to start of previous sub word",
        move_next_sub_word_end, "Move to end of next sub word",
        move_prev_sub_word_end, "Move to end of previous sub word",
        vim_move_next_word_start, "Move to start of next word (vim caret)",
        vim_move_prev_word_start, "Move to start of previous word (vim caret)",
        vim_move_next_word_end, "Move to end of next word (vim caret)",
        vim_move_prev_word_end, "Move to end of previous word (vim caret)",
        vim_move_next_long_word_start, "Move to start of next long word (vim caret)",
        vim_move_prev_long_word_start, "Move to start of previous long word (vim caret)",
        vim_move_next_long_word_end, "Move to end of next long word (vim caret)",
        vim_move_prev_long_word_end, "Move to end of previous long word (vim caret)",
        move_parent_node_end, "Move to end of the parent node",
        move_parent_node_start, "Move to beginning of the parent node",
        extend_next_word_start, "Extend to start of next word",
        extend_prev_word_start, "Extend to start of previous word",
        extend_next_word_end, "Extend to end of next word",
        extend_prev_word_end, "Extend to end of previous word",
        extend_next_long_word_start, "Extend to start of next long word",
        extend_prev_long_word_start, "Extend to start of previous long word",
        extend_next_long_word_end, "Extend to end of next long word",
        extend_prev_long_word_end, "Extend to end of prev long word",
        extend_next_sub_word_start, "Extend to start of next sub word",
        extend_prev_sub_word_start, "Extend to start of previous sub word",
        extend_next_sub_word_end, "Extend to end of next sub word",
        extend_prev_sub_word_end, "Extend to end of prev sub word",
        extend_parent_node_end, "Extend to end of the parent node",
        extend_parent_node_start, "Extend to beginning of the parent node",
        find_till_char, "Move till next occurrence of char",
        find_next_char, "Move to next occurrence of char",
        extend_till_char, "Extend till next occurrence of char",
        extend_next_char, "Extend to next occurrence of char",
        till_prev_char, "Move till previous occurrence of char",
        find_prev_char, "Move to previous occurrence of char",
        sneak_forward, "Sneak: jump forward to a two-character sequence",
        sneak_backward, "Sneak: jump backward to a two-character sequence",
        sneak_or_substitute_char, "Sneak forward, or substitute char when vim-sneak is off",
        sneak_or_substitute_line, "Sneak backward, or substitute line when vim-sneak is off",
        extend_till_prev_char, "Extend till previous occurrence of char",
        extend_prev_char, "Extend to previous occurrence of char",
        repeat_last_motion, "Repeat last motion",
        repeat_find_char_reverse, "Repeat last find in opposite direction (,)",
        replace, "Replace with new char",
        switch_case, "Switch (toggle) case",
        switch_to_uppercase, "Switch to uppercase",
        switch_to_lowercase, "Switch to lowercase",
        page_up, "Move page up",
        page_down, "Move page down",
        half_page_up, "Move half page up",
        half_page_down, "Move half page down",
        page_cursor_up, "Move page and cursor up",
        page_cursor_down, "Move page and cursor down",
        page_cursor_half_up, "Move page and cursor half up",
        page_cursor_half_down, "Move page and cursor half down",
        select_all, "Select whole document",
        select_regex, "Select all regex matches inside selections",
        select_all_instances, "Select all occurrences of the current selection in the buffer",
        split_selection, "Split selections on regex matches",
        split_selection_on_newline, "Split selection on newlines",
        merge_selections, "Merge selections",
        merge_consecutive_selections, "Merge consecutive selections",
        search, "Search for regex pattern",
        rsearch, "Reverse search for regex pattern",
        search_next, "Select next search match",
        search_prev, "Select previous search match",
        extend_search_next, "Add next search match to selection",
        extend_search_prev, "Add previous search match to selection",
        search_selection, "Use current selection as search pattern",
        search_selection_detect_word_boundaries, "Use current selection as the search pattern, automatically wrapping with `\\b` on word boundaries",
        make_search_word_bounded, "Modify current search to make it word bounded",
        global_search, "Global search in workspace folder",
        global_search_symbol, "Global search seeded with the symbol under the cursor",
        clear_search_highlight, "Clear persistent search highlight (SPC s c)",
        regex_convert_form, "Convert the selected regex between PCRE and Emacs forms (SPC x r c)",
        regex_emacs_to_rx_replace, "Convert the selected Emacs regex to rx form (SPC x r e x)",
        regex_emacs_to_rx_explain, "Explain the selected Emacs regex as rx (SPC x r e /)",
        regex_pcre_to_rx_replace, "Convert the selected PCRE regex to rx form (SPC x r x)",
        regex_pcre_to_rx_explain, "Explain the selected PCRE regex as rx (SPC x r /)",
        justify_left, "Left-justify (fill) the region (SPC x j l)",
        justify_right, "Right-justify the region (SPC x j r)",
        justify_center, "Center-justify the region (SPC x j c)",
        justify_full, "Full-justify the region (SPC x j f)",
        justify_none, "Remove justification / left-fill (SPC x j n)",
        count_words_region, "Count occurrences per word in the selection (SPC x w c)",
        goto_next_close_paren, "Go forward to next closing paren (SPC k j)",
        goto_prev_open_paren, "Go backward to previous opening paren (SPC k k)",
        ediff_windows, "Diff the two front windows side by side (SPC D w w)",
        ediff_buffer, "Diff the current buffer against a picked buffer (SPC D b b)",
        transpose_paragraph, "Swap the current paragraph with the previous one (SPC x t p)",
        transpose_sexp, "Swap the current s-expression with the previous one (SPC x t e)",
        transpose_sentence, "Swap the current sentence with the previous one (SPC x t s)",
        make_3_windows, "Lay out three vertical windows (SPC w 3)",
        make_4_windows, "Lay out a 2x2 window grid (SPC w 4)",
        narrow_to_function, "Narrow the buffer to the enclosing function (SPC n f)",
        align_at_equals, "Align region at = (SPC x a =)",
        align_at_comma, "Align region at , (SPC x a ,)",
        align_at_colon, "Align region at : (SPC x a :)",
        align_at_semicolon, "Align region at ; (SPC x a ;)",
        align_at_ampersand, "Align region at & (SPC x a &)",
        align_at_lparen, "Align region at ( (SPC x a ()",
        align_at_rparen, "Align region at ) (SPC x a ))",
        align_at_lbracket, "Align region at [ (SPC x a [)",
        align_at_rbracket, "Align region at ] (SPC x a ])",
        align_at_lbrace, "Align region at { (SPC x a {)",
        align_at_rbrace, "Align region at } (SPC x a })",
        align_at_dot, "Align region at . (SPC x a .)",
        align_at_arithmetic, "Align region at arithmetic operators (SPC x a m)",
        align_at_regex, "Align region at a user-specified regexp (SPC x a r)",
        align_left_at_char, "Left-align region at a typed delimiter (SPC x a l)",
        align_right_at_char, "Right-align region at a typed delimiter (SPC x a L)",
        buffer_to_window_1, "Move current buffer to window 1 (SPC b . 1)",
        buffer_to_window_2, "Move current buffer to window 2 (SPC b . 2)",
        buffer_to_window_3, "Move current buffer to window 3 (SPC b . 3)",
        buffer_to_window_4, "Move current buffer to window 4 (SPC b . 4)",
        buffer_to_window_5, "Move current buffer to window 5 (SPC b . 5)",
        buffer_to_window_6, "Move current buffer to window 6 (SPC b . 6)",
        buffer_to_window_7, "Move current buffer to window 7 (SPC b . 7)",
        buffer_to_window_8, "Move current buffer to window 8 (SPC b . 8)",
        buffer_to_window_9, "Move current buffer to window 9 (SPC b . 9)",
        goto_window_1, "Go to window 1 (SPC 1)",
        goto_window_2, "Go to window 2 (SPC 2)",
        goto_window_3, "Go to window 3 (SPC 3)",
        goto_window_4, "Go to window 4 (SPC 4)",
        goto_window_5, "Go to window 5 (SPC 5)",
        goto_window_6, "Go to window 6 (SPC 6)",
        goto_window_7, "Go to window 7 (SPC 7)",
        goto_window_8, "Go to window 8 (SPC 8)",
        goto_window_9, "Go to window 9 (SPC 9)",
        delete_window_and_buffer, "Close window and kill its buffer (SPC w . x)",
        eval_elisp_region, "Evaluate the selection as elisp (SPC m e r)",
        eval_elisp_buffer, "Evaluate the buffer as elisp (SPC m e b)",
        eval_elisp_line, "Evaluate the current line as elisp (SPC m e e)",
        eval_elisp_defun, "Evaluate the enclosing form as elisp (SPC m e f)",
        layout_create, "Create a new window-layout from the current windows (SPC l l)",
        layout_next, "Switch to the next layout (SPC l n)",
        layout_prev, "Switch to the previous layout (SPC l p)",
        layout_last, "Switch to the last-used layout (SPC l TAB)",
        layout_default, "Switch to the default (first) layout (SPC l h)",
        layout_delete, "Delete the current layout, keeping its buffers (SPC l d)",
        layout_save, "Save layouts to disk (SPC l s)",
        layout_load, "Load layouts from disk (SPC l L)",
        layout_goto_1, "Switch to layout 1 (SPC l 1)",
        layout_goto_2, "Switch to layout 2 (SPC l 2)",
        layout_goto_3, "Switch to layout 3 (SPC l 3)",
        layout_goto_4, "Switch to layout 4 (SPC l 4)",
        layout_goto_5, "Switch to layout 5 (SPC l 5)",
        layout_goto_6, "Switch to layout 6 (SPC l 6)",
        layout_goto_7, "Switch to layout 7 (SPC l 7)",
        layout_goto_8, "Switch to layout 8 (SPC l 8)",
        layout_goto_9, "Switch to layout 9 (SPC l 9)",
        toggle_modeline_position, "Toggle cursor position in the mode line (SPC t m p)",
        toggle_modeline_vcs, "Toggle version-control info in the mode line (SPC t m v)",
        toggle_centered_cursor, "Keep the cursor vertically centered (SPC t -)",
        toggle_fill_column, "Toggle a fill-column ruler (SPC t f)",
        toggle_long_line_marker, "Toggle an 80th-column ruler (SPC t 8)",
        toggle_soft_wrap, "Toggle soft-wrap of long lines (IntelliJ View > Soft-Wrap)",
        toggle_whitespace_render, "Toggle rendering of whitespace characters (IntelliJ View > Show Whitespaces)",
        ediff_file, "Diff a prompted file against the current buffer (SPC D f f)",
        ediff_3_files, "3-way diff of three prompted files, read-only (SPC D f 3)",
        ediff_3_buffers, "3-way diff of three open buffers, read-only (SPC D b 3)",
        kill_buffers_by_regex, "Kill all buffers whose name matches a regex (SPC b M)",
        narrow_to_page, "Narrow the buffer to the current page (SPC n p)",
        copy_file, "Copy the current file to a prompted destination (SPC f c)",
        find_file_replace_buffer, "Open a file and replace the current buffer with it (SPC f A)",
        open_file_literally, "Open a file with no syntax/language (fundamental mode, SPC f l)",
        locate_file, "Locate a file via system locate/mdfind and open it (SPC f L)",
        edit_project_config, "Edit the project-local .zemacs/config.toml (SPC p e)",
        man_page_search, "Search man pages via apropos and view the selected page (SPC h m)",
        info_search, "Search GNU info manuals (apropos) and view the selected node (SPC h i)",
        diagnostics_verify_setup, "Report the buffer's diagnostics/LSP setup (SPC e v)",
        describe_diagnostics_checker, "Describe the buffer's checkers/language servers (SPC e h)",
        describe_text_properties, "Describe the tree-sitter node stack at the cursor (SPC h d t)",
        copy_system_info, "Copy system info (version/OS/arch) to the clipboard (SPC h d s)",
        copy_last_keys, "Copy the most recently pressed keys to the clipboard (SPC h d l)",
        ace_window, "Jump to a window by its number, prompted (ace-window, SPC w . a)",
        browse_news, "Browse zemacs release notes / NEWS (SPC h n)",
        show_environment, "Show the editor's environment variables (SPC f e e)",
        reimport_shell_env, "Re-import the shell environment into the editor (SPC f e C-e)",
        goto_buffer_window, "Focus the window already showing a chosen buffer (SPC b w)",
        git_file_dispatch, "Magit-style file operations dispatch for the current file (SPC g f m)",
        describe_current_modes, "Describe the current editor/buffer modes (SPC h d m)",
        describe_language_package, "Describe the language-support config for the buffer (SPC h d p)",
        package_search, "Search configured language packages and describe one (SPC h p)",
        config_variable_search, "Search editor config variables, copy path on select (SPC h .)",
        clone_indirect_buffer, "Clone the current buffer into a shared-document split (SPC b N i)",
        clone_indirect_from_buffer, "Open an existing buffer in a shared-document split (SPC b N C-i)",
        open_junk_file, "Open a fresh timestamped junk file (SPC f J)",
        open_hex, "Open the current file in the hex editor (SPC f h, hexl)",
        open_file_external, "Open the current file with the OS default program (SPC f o)",
        git_init, "Initialize a new git repository (SPC g i)",
        view_file_at_rev, "View the current file at a branch/commit (SPC g f f)",
        extend_line, "Select current line, if already selected, extend to another line based on the anchor",
        extend_line_below, "Select current line, if already selected, extend to next line",
        extend_line_above, "Select current line, if already selected, extend to previous line",
        select_line_above, "Select current line, if already selected, extend or shrink line above based on the anchor",
        select_line_below, "Select current line, if already selected, extend or shrink line below based on the anchor",
        extend_to_line_bounds, "Extend selection to line bounds",
        shrink_to_line_bounds, "Shrink selection to line bounds",
        delete_selection, "Delete selection",
        delete_selection_noyank, "Delete selection without yanking",
        change_selection, "Change selection",
        change_selection_noyank, "Change selection without yanking",
        collapse_selection, "Collapse selection into single cursor",
        flip_selections, "Flip selection cursor and anchor",
        ensure_selections_forward, "Ensure all selections face forward",
        insert_mode, "Insert before selection",
        append_mode, "Append after selection",
        replace_mode, "Enter Replace mode (overtype)",
        command_mode, "Enter command mode",
        file_picker, "Open file picker",
        file_picker_in_current_buffer_directory, "Open file picker at current buffer's directory",
        file_picker_in_current_directory, "Open file picker at current working directory",
        file_explorer, "Open file explorer in workspace root",
        file_explorer_in_current_buffer_directory, "Open file explorer at current buffer's directory",
        file_explorer_in_current_directory, "Open file explorer at current working directory",
        code_action, "Perform code action",
        buffer_picker, "Open buffer picker",
        jumplist_picker, "Open jumplist picker",
        register_picker, "Browse registers and paste the chosen one",
        marks_picker, "Fuzzy-pick a vim mark and jump to it (:Marks)",
        buffer_line_picker, "Fuzzy-search lines in the current buffer (:BLines)",
        command_history_picker, "Fuzzy-pick and run a past command line (:History:)",
        search_history_picker, "Fuzzy-pick and re-run a past search (:History/)",
        unicode_picker, "Fuzzy-pick a character/digraph and insert it (helm-unicode)",
        git_file_log_picker, "Commit log for the current file (:BCommits)",
        git_repo_log_picker, "Commit log for the whole repo (:Commits)",
        theme_picker, "Open fuzzy theme picker with live preview",
        wrap_sexp, "Wrap the selection in parentheses",
        symbol_picker, "Open symbol picker",
        syntax_symbol_picker, "Open symbol picker from syntax information",
        lsp_or_syntax_symbol_picker, "Open symbol picker from LSP or syntax information",
        changed_file_picker, "Open changed file picker",
        frecent_file_picker, "Open recent files ranked by frecency (z algorithm)",
        reopen_last_closed, "Reopen the most recently closed file",
        harpoon_add, "Pin the current file to the harpoon list",
        harpoon_jump, "Jump to the harpoon mark in slot [count]",
        harpoon_1, "Jump to harpoon mark 1",
        harpoon_2, "Jump to harpoon mark 2",
        harpoon_3, "Jump to harpoon mark 3",
        harpoon_4, "Jump to harpoon mark 4",
        harpoon_next, "Open the next harpoon mark",
        harpoon_prev, "Open the previous harpoon mark",
        harpoon_menu, "Open the harpoon marks menu",
        harpoon_remove, "Unpin the current file from harpoon",
        select_references_to_symbol_under_cursor, "Select symbol references",
        workspace_symbol_picker, "Open workspace symbol picker",
        syntax_workspace_symbol_picker, "Open workspace symbol picker from syntax information",
        lsp_or_syntax_workspace_symbol_picker, "Open workspace symbol picker from LSP or syntax information",
        diagnostics_picker, "Open diagnostic picker",
        workspace_diagnostics_picker, "Open workspace diagnostic picker",
        last_picker, "Open last picker",
        insert_at_line_start, "Insert at start of line",
        insert_at_line_end, "Insert at end of line",
        open_below, "Open new line below selection",
        open_above, "Open new line above selection",
        normal_mode, "Enter normal mode",
        select_mode, "Enter selection extend mode",
        exit_select_mode, "Exit selection mode",
        goto_definition, "Goto definition",
        goto_declaration, "Goto declaration",
        add_newline_above, "Add newline above",
        add_newline_below, "Add newline below",
        goto_type_definition, "Goto type definition",
        goto_implementation, "Goto implementation",
        goto_file_start, "Goto line number `<n>` else file start",
        goto_file_end, "Goto file end",
        extend_to_file_start, "Extend to line number `<n>` else file start",
        extend_to_file_end, "Extend to file end",
        goto_file, "Goto files/URLs in selections",
        goto_file_hsplit, "Goto files in selections (hsplit)",
        goto_file_vsplit, "Goto files in selections (vsplit)",
        goto_reference, "Goto references",
        goto_window_top, "Goto window top",
        goto_window_center, "Goto window center",
        goto_window_bottom, "Goto window bottom",
        goto_last_accessed_file, "Goto last accessed file",
        goto_last_modified_file, "Goto last modified file",
        goto_last_modification, "Goto last modification",
        goto_line, "Goto line",
        goto_last_line, "Goto last line",
        extend_to_last_line, "Extend to last line",
        goto_first_diag, "Goto first diagnostic",
        copy_diagnostic, "Copy the diagnostic message(s) on the current line",
        goto_last_diag, "Goto last diagnostic",
        goto_next_diag, "Goto next diagnostic",
        goto_prev_diag, "Goto previous diagnostic",
        goto_next_change, "Goto next change",
        goto_prev_change, "Goto previous change",
        goto_next_conflict, "Goto next merge-conflict marker",
        goto_prev_conflict, "Goto previous merge-conflict marker",
        conflict_take_all_ours, "Resolve ALL conflicts: keep our side",
        conflict_take_all_theirs, "Resolve ALL conflicts: keep their side",
        git_diff, "Open side-by-side diff vs HEAD",
        resolve_conflicts, "Resolve merge conflicts (3-way)",
        git_status, "Magit status",
        org_cycle, "Org: toggle subtree fold",
        org_todo, "Org: cycle TODO keyword",
        org_priority, "Org: cycle priority cookie",
        org_promote, "Org: promote heading",
        org_demote, "Org: demote heading",
        org_next_heading, "Org: next heading",
        org_prev_heading, "Org: previous heading",
        org_fold_all, "Org: fold all headings",
        org_unfold_all, "Org: unfold all",
        org_agenda, "Org: open agenda",
        org_capture, "Org: capture note",
        goto_first_change, "Goto first change",
        goto_last_change, "Goto last change",
        goto_line_start, "Goto line start",
        goto_line_end, "Goto line end",
        goto_column, "Goto column",
        extend_to_column, "Extend to column",
        goto_next_buffer, "Goto next buffer",
        goto_previous_buffer, "Goto previous buffer",
        goto_line_end_newline, "Goto newline at line end",
        goto_first_nonwhitespace, "Goto first non-blank in line",
        trim_selections, "Trim whitespace from selections",
        extend_to_line_start, "Extend to line start",
        extend_to_first_nonwhitespace, "Extend to first non-blank in line",
        extend_to_line_end, "Extend to line end",
        extend_to_line_end_newline, "Extend to line end",
        signature_help, "Show signature help",
        smart_tab, "Insert tab if all cursors have all whitespace to their left; otherwise, run a separate command.",
        insert_tab, "Insert tab char",
        insert_newline, "Insert newline char",
        insert_char_interactive, "Insert an interactively-chosen char",
        append_char_interactive, "Append an interactively-chosen char",
        delete_char_backward, "Delete previous char",
        delete_char_forward, "Delete next char",
        delete_word_backward, "Delete previous word",
        delete_word_forward, "Delete next word",
        kill_to_line_start, "Delete till start of line",
        kill_to_line_end, "Delete till end of line",
        undo, "Undo change",
        redo, "Redo change",
        earlier, "Move backward in history",
        later, "Move forward in history",
        commit_undo_checkpoint, "Commit changes to new checkpoint",
        yank, "Yank selection",
        yank_to_clipboard, "Yank selections to clipboard",
        yank_to_primary_clipboard, "Yank selections to primary clipboard",
        yank_joined, "Join and yank selections",
        yank_joined_to_clipboard, "Join and yank selections to clipboard",
        yank_main_selection_to_clipboard, "Yank main selection to clipboard",
        yank_joined_to_primary_clipboard, "Join and yank selections to primary clipboard",
        yank_main_selection_to_primary_clipboard, "Yank main selection to primary clipboard",
        replace_with_yanked, "Replace with yanked text",
        replace_selections_with_clipboard, "Replace selections by clipboard content",
        replace_selections_with_primary_clipboard, "Replace selections by primary clipboard",
        paste_after, "Paste after selection",
        paste_before, "Paste before selection",
        yank_from_kill_ring, "Yank the latest kill-ring entry (emacs C-y)",
        yank_pop, "Replace the just-yanked text with the next kill-ring entry (emacs M-y)",
        set_mark_command, "Set mark and activate region, pushing to the mark ring (emacs C-SPC)",
        pop_to_mark, "Jump to the top of the mark ring, rotating it (emacs C-x C-SPC)",
        point_to_register, "Save point to a register (emacs C-x r SPC)",
        jump_to_register, "Jump to the position in a register (emacs C-x r j)",
        number_to_register, "Store the prefix count in a register (emacs C-x r n)",
        increment_register, "Add the prefix count to a number register (emacs C-x r +)",
        emacs_insert_register, "Insert a number register's value as text (emacs C-x r i)",
        kill_rectangle, "Kill (cut) the rectangle, saving it for yank (emacs C-x r k)",
        delete_rectangle, "Delete the rectangle without saving (emacs C-x r d)",
        clear_rectangle, "Blank the rectangle with spaces (emacs C-x r c)",
        copy_rectangle_as_kill, "Copy the rectangle without deleting (emacs C-x r M-w)",
        yank_rectangle, "Insert the saved rectangle at point (emacs C-x r y)",
        bookmark_set, "Set a named persistent bookmark at point (emacs C-x r m)",
        bookmark_jump, "Jump to a bookmark via a picker (emacs C-x r b / C-x r l)",
        define_abbrev, "Define a global abbrev: <name> <expansion> (emacs C-x a g)",
        expand_abbrev, "Expand the abbrev before point (emacs C-x ')",
        paste_clipboard_after, "Paste clipboard after selections",
        paste_clipboard_before, "Paste clipboard before selections",
        paste_primary_clipboard_after, "Paste primary clipboard after selections",
        paste_primary_clipboard_before, "Paste primary clipboard before selections",
        indent, "Indent selection",
        unindent, "Unindent selection",
        format_selections, "Format selection",
        join_selections, "Join lines inside selection",
        join_selections_space, "Join lines inside selection and select spaces",
        keep_selections, "Keep selections matching regex",
        remove_selections, "Remove selections matching regex",
        align_selections, "Align selections in column",
        keep_primary_selection, "Keep primary selection",
        remove_primary_selection, "Remove primary selection",
        completion, "Invoke completion popup",
        hover, "Show docs for item under cursor",
        toggle_comments, "Comment/uncomment selections",
        toggle_line_comments, "Line comment/uncomment selections",
        comment_to_line, "Comment/uncomment from the cursor line to a prompted line (SPC c t)",
        invert_comment_to_line, "Invert comments per line from the cursor to a prompted line (SPC c T)",
        toggle_block_comments, "Block comment/uncomment selections",
        rotate_selections_forward, "Rotate selections forward",
        rotate_selections_backward, "Rotate selections backward",
        rotate_selection_contents_forward, "Rotate selection contents forward",
        rotate_selection_contents_backward, "Rotate selections contents backward",
        reverse_selection_contents, "Reverse selections contents",
        expand_selection, "Expand selection to parent syntax node",
        shrink_selection, "Shrink selection to previously expanded syntax node",
        wildfire, "Wildfire: select/expand to the closest text object",
        wildfire_shrink, "Wildfire: shrink to the previously selected text object",
        select_next_sibling, "Select next sibling in the syntax tree",
        select_prev_sibling, "Select previous sibling the in syntax tree",
        select_all_siblings, "Select all siblings of the current node",
        select_all_children, "Select all children of the current node",
        jump_forward, "Jump forward on jumplist",
        jump_backward, "Jump backward on jumplist",
        save_selection, "Save current selection to jumplist",
        jump_view_right, "Jump to right split",
        jump_view_left, "Jump to left split",
        jump_view_up, "Jump to split above",
        jump_view_down, "Jump to split below",
        swap_view_right, "Swap with right split",
        swap_view_left, "Swap with left split",
        swap_view_up, "Swap with split above",
        swap_view_down, "Swap with split below",
        transpose_view, "Transpose splits",
        rotate_view, "Goto next window",
        rotate_view_reverse, "Goto previous window",
        hsplit, "Horizontal bottom split",
        hsplit_new, "Horizontal bottom split scratch buffer",
        vsplit, "Vertical right split",
        vsplit_new, "Vertical right split scratch buffer",
        wclose, "Close window",
        wonly, "Close windows except current",
        select_register, "Select register",
        insert_register, "Insert register",
        insert_last_inserted_text, "Insert the previously inserted text (vim i_CTRL-A)",
        insert_last_inserted_and_stop, "Insert previously inserted text and stop insert (vim i_CTRL-@)",
        copy_between_registers, "Copy between two registers",
        align_view_middle, "Align view middle",
        align_view_top, "Align view top",
        align_view_center, "Align view center",
        align_view_bottom, "Align view bottom",
        scroll_up, "Scroll view up",
        scroll_down, "Scroll view down",
        scroll_column_left, "Scroll view left one column (zh)",
        scroll_column_right, "Scroll view right one column (zl)",
        scroll_half_column_left, "Scroll view left half a screen (zH)",
        scroll_half_column_right, "Scroll view right half a screen (zL)",
        resize_view_wider, "Make current window wider (CTRL-W >)",
        resize_view_narrower, "Make current window narrower (CTRL-W <)",
        resize_view_taller, "Make current window taller (CTRL-W +)",
        resize_view_shorter, "Make current window shorter (CTRL-W -)",
        resize_view_equalize, "Make all windows equal size (CTRL-W =)",
        golden_ratio_resize, "Resize the focused window to the golden ratio (SPC t g)",
        rot13, "ROT13-encode the selection (g?)",
        url_encode, "Percent-encode (URL-encode) the selection",
        url_decode, "Percent-decode (URL-decode) the selection",
        parse_query_selection, "Expand a URL query string into decoded key=value lines",
        build_query_selection, "Build a URL query string from key=value lines",
        url_info_selection, "Break the selected URL into scheme/host/port/path/query lines",
        encode_base64, "Base64-encode the selection",
        decode_base64, "Base64-decode the selection",
        encode_base64url, "URL-safe base64-encode the selection (no padding)",
        decode_base64url, "URL-safe base64-decode the selection (JWT-friendly)",
        jwt_decode_selection, "Decode the selected JWT into pretty header + payload JSON",
        encode_html, "HTML-escape the selection (& < > \" ')",
        decode_html, "Decode HTML entities in the selection",
        html_to_text_selection, "Strip HTML tags and decode entities to plain text",
        title_case_selection, "Title-case the selection (capitalize each word)",
        sentence_case_selection, "Capitalize the first letter of each sentence in the selection",
        straighten_quotes_selection, "Convert smart quotes/dashes in the selection to plain ASCII",
        hex_to_rgb_selection, "Convert a #hex color in the selection to rgb(r, g, b)",
        rgb_to_hex_selection, "Convert an rgb(r, g, b) color in the selection to #hex",
        to_roman_selection, "Convert the selected integer to a Roman numeral",
        from_roman_selection, "Convert the selected Roman numeral to an integer",
        add_commas_selection, "Add thousands separators to numbers in the selection",
        strip_commas_selection, "Remove thousands separators from numbers in the selection",
        swap_quotes_selection, "Swap ' and \" quote characters in the selection",
        strip_quotes_selection, "Remove surrounding quotes from the selection",
        reverse_words_selection, "Reverse the word order within each selected line",
        unwrap_tag_selection, "Strip the outermost <tag>…</tag> wrapper from the selection",
        sort_paragraphs_selection, "Sort blank-line-separated paragraphs in the selection",
        lighten_selection, "Lighten the hex color in the selection by 10%",
        darken_selection, "Darken the hex color in the selection by 10%",
        contrast_text, "Recommend black/white text for the selected hex background color",
        toggle_value_selection, "Toggle the boolean/keyword in the selection (true<->false, …)",
        normalize_whitespace_selection, "Collapse internal whitespace runs in the selection",
        insert_toc, "Insert a markdown table of contents from the buffer's headings",
        slugify_selection, "Slugify the selection (lowercase, hyphen-separated)",
        humanize_selection, "Humanize a slug/identifier into a Title-Cased label",
        transpose_csv_selection, "Transpose the selected CSV/TSV table (rows <-> columns)",
        csv_to_json_selection, "Convert the selected CSV/TSV to a JSON array of objects",
        regex_escape_selection, "Escape regex metacharacters in the selection",
        blockquote_selection, "Prefix each selected line with \"> \" (markdown blockquote)",
        unblockquote_selection, "Strip a leading \"> \" from each selected line",
        bullet_list_selection, "Make a markdown bullet list from the selected lines",
        unbullet_selection, "Strip a leading bullet (- * +) from each selected line",
        strip_ansi_selection, "Strip ANSI/VT escape codes from the selection",
        html_escape_selection, "HTML-escape the selection (& < > \" ' to entities)",
        html_unescape_selection, "HTML-unescape entities in the selection back to characters",
        reverse_chars_selection, "Reverse the characters in the selection",
        json_escape_selection, "JSON-escape the selection (for a string literal)",
        to_json_string_selection, "Wrap the selection in quotes as a JSON string literal",
        json_unescape_selection, "JSON-unescape the selection",
        to_hex_selection, "Encode the selection as hex bytes",
        from_hex_selection, "Decode hex bytes in the selection back to text",
        format_table_selection, "Align the selected markdown table's columns",
        csv_to_table_selection, "Convert the selected CSV/TSV to a markdown table",
        table_to_csv_selection, "Convert the selected markdown table to CSV",
        json_pretty_selection, "Pretty-print the selected JSON (preserves key order)",
        json_minify_selection, "Minify the selected JSON",
        xml_pretty_selection, "Pretty-print the selected XML/HTML",
        insert_digraph, "Insert a digraph by two-character mnemonic (CTRL-K)",
        insert_uuid_v4, "Insert a random UUIDv4 (SPC i U 4)",
        insert_uuid_v1, "Insert a time-based UUIDv1 (SPC i U 1)",
        insert_lorem_sentence, "Insert a lorem-ipsum sentence (SPC i l s)",
        insert_lorem_paragraph, "Insert a lorem-ipsum paragraph (SPC i l p)",
        insert_lorem_list, "Insert a lorem-ipsum list (SPC i l l)",
        insert_password_simple, "Insert a simple alphanumeric password (SPC i p 1)",
        insert_password_strong, "Insert a stronger password with symbols (SPC i p 2)",
        insert_password_paranoid, "Insert a long password for paranoids (SPC i p 3)",
        insert_password_numerical, "Insert a numeric password (SPC i p n)",
        insert_password_phonetic, "Insert a phonetically easy password (SPC i p p)",
        symbol_upper_camel, "Change symbol style to UpperCamelCase (SPC x i C)",
        symbol_up_case, "Change symbol style to UP_CASE (SPC x i U)",
        symbol_under_score, "Change symbol style to under_score (SPC x i _)",
        randomize_lines_in_region, "Randomize lines in the selection (SPC x l r)",
        randomize_words_in_region, "Randomize words in the selection (SPC x w r)",
        copy_char_below, "Insert the character below the cursor (i_CTRL-E)",
        copy_char_above, "Insert the character above the cursor (i_CTRL-Y)",
        file_info, "Show file name and cursor position (CTRL-G)",
        document_stats, "Show document line/word/char counts (g CTRL-G)",
        git_blame_line, "Show git blame for the current line (g b)",
        git_branch_picker, "Pick a git branch and check it out",
        preferences, "Open the unified Preferences window",
        help, "Open the inline Help browser",
        dashboard, "Open the system-stats Dashboard (Preferences)",
        search_in_files, "Open the project-wide Find in Files panel",
        terminal, "Open an integrated terminal (PTY shell)",
        run_config_manager, "Manage run/debug configurations",
        run_active_config, "Run the active run configuration",
        clear_run_output, "Clear the Run tool window output",
        rerun_last_run, "Re-run the last command in the Run console",
        run_next_error, "Jump to the next file:line in the run output",
        run_prev_error, "Jump to the previous file:line in the run output",
        reveal_in_tree, "Reveal the current file in the project tree",
        toggle_auto_reveal, "Toggle always-select-opened-file (autoscroll from source)",
        focus_file_tree, "Focus the project file tree panel",
        focus_structure, "Focus the structure/symbol outline panel",
        hide_active_tool_window, "Return focus to the editor, hiding the active tool window (JetBrains Shift-Esc)",
        jump_to_last_tool_window, "Toggle focus between the editor and the last tool window (JetBrains F12)",
        focus_bookmarks, "Focus the Bookmarks tool window (pinned files; JetBrains Bookmarks)",
        focus_marks_panel, "Focus the Marks tool window",
        focus_registers_panel, "Focus the Registers tool window",
        focus_jumplist_panel, "Focus the Jumplist tool window",
        focus_recent_panel, "Focus the Recent Files tool window",
        focus_todo_panel, "Focus the TODO tool window",
        focus_problems, "Focus the problems/diagnostics panel",
        focus_run_console, "Focus the Run console (scroll output with j/k/PgUp/PgDn)",
        focus_git_panel, "Focus the Git changes panel (j/k select, Enter opens)",
        focus_ci_panel, "Focus the CI status panel (GitHub Actions runs; Enter opens in browser)",
        toggle_bottom_zoom, "Maximize / restore the bottom panel",
        toggle_drawer_mid, "Fold / unfold the middle column of the bottom drawer",
        toggle_ide, "Toggle the IDE workbench (Zen / focus mode)",
        settings_page, "Open the settings page (config.toml editor)",
        goto_next_spell_error, "Move to the next misspelled word (]s)",
        goto_prev_spell_error, "Move to the previous misspelled word ([s)",
        spell_add_good, "Mark word under cursor as correctly spelled (zg)",
        spell_add_bad, "Mark word under cursor as misspelled (zw)",
        spell_undo, "Undo a zg/zw for the word under cursor (zug)",
        spell_suggest, "Show spelling suggestions for the word under cursor (z=)",
        fold_create, "Create a fold over the selection (zf)",
        fold_toggle, "Toggle fold under cursor (za)",
        fold_open, "Open fold under cursor (zo)",
        fold_close, "Close fold under cursor (zc)",
        fold_open_all, "Open all folds (zR)",
        fold_close_all, "Close all folds (zM)",
        fold_delete, "Delete fold under cursor (zd)",
        fold_delete_all, "Delete all folds (zE)",
        narrow_to_region, "Narrow the buffer to the selected region (SPC n r)",
        widen, "Widen: remove narrowing and reveal the whole buffer (SPC n w)",
        narrow_to_function_indirect, "Narrow to the function in an indirect (split) view (SPC n F)",
        narrow_to_page_indirect, "Narrow to the page in an indirect (split) view (SPC n P)",
        kmacro_ring_next, "Cycle to the next macro in the ring (SPC K r n)",
        kmacro_ring_prev, "Cycle to the previous macro in the ring (SPC K r p)",
        kmacro_ring_delete, "Delete the head macro in the ring (SPC K r d)",
        kmacro_ring_swap, "Swap the first two macros in the ring (SPC K r s)",
        kmacro_ring_view, "View the head macro in the ring (SPC K r L)",
        kmacro_to_register, "Write the last macro to a register (SPC K e r)",
        kmacro_add_counter, "Add [count] to the keyboard-macro counter (SPC K c a)",
        kmacro_insert_counter, "Insert the macro counter value, then increment (SPC K c c)",
        toggle_readonly, "Toggle the buffer's read-only (writable) state (SPC b w)",
        paredit_slurp_forward, "Paredit: slurp the next s-expression forward (SPC k s)",
        paredit_barf_forward, "Paredit: barf the last s-expression forward (SPC k b)",
        paredit_slurp_backward, "Paredit: slurp the previous s-expression backward (SPC k S)",
        paredit_barf_backward, "Paredit: barf the first s-expression backward (SPC k B)",
        paredit_splice, "Paredit: splice/unwrap the enclosing s-expression (SPC k W)",
        paredit_raise, "Paredit: raise the current s-expression (SPC k r)",
        paredit_transpose, "Paredit: transpose the s-expressions around point (SPC k t)",
        paredit_split, "Paredit: split the enclosing list at point (SPC j s)",
        paredit_absorb, "Paredit: absorb the previous sexp into the current form (SPC k a)",
        paredit_convolute, "Paredit: convolute — swap enclosing/inner prefixes (SPC k c)",
        buffer_swap_window_1, "Swap current buffer with window 1 (SPC b . M-1)",
        buffer_swap_window_2, "Swap current buffer with window 2 (SPC b . M-2)",
        buffer_swap_window_3, "Swap current buffer with window 3 (SPC b . M-3)",
        buffer_swap_window_4, "Swap current buffer with window 4 (SPC b . M-4)",
        buffer_swap_window_5, "Swap current buffer with window 5 (SPC b . M-5)",
        buffer_swap_window_6, "Swap current buffer with window 6 (SPC b . M-6)",
        buffer_swap_window_7, "Swap current buffer with window 7 (SPC b . M-7)",
        buffer_swap_window_8, "Swap current buffer with window 8 (SPC b . M-8)",
        buffer_swap_window_9, "Swap current buffer with window 9 (SPC b . M-9)",
        paredit_splice_kill_forward, "Paredit: splice, killing forward (SPC k e)",
        paredit_splice_kill_backward, "Paredit: splice, killing backward (SPC k E)",
        paredit_insert_sexp_after, "Paredit: insert a new () sexp after the current one (SPC k ))",
        paredit_insert_sexp_before, "Paredit: insert a new () sexp before the current one (SPC k ()",
        fold_next, "Move to next fold (zj)",
        fold_prev, "Move to previous fold (zk)",
        goto_line_last_nonblank, "Goto last non-blank on line (g_)",
        goto_line_middle, "Goto middle of text line (gM)",
        goto_byte, "Goto byte {count} in buffer (go)",
        goto_prev_unmatched_paren, "Goto previous unmatched ( ([()",
        goto_prev_unmatched_brace, "Goto previous unmatched { ([{)",
        goto_next_unmatched_paren, "Goto next unmatched ) (])",
        goto_next_unmatched_brace, "Goto next unmatched } (]})",
        goto_prev_mark, "Goto previous lowercase mark ([`)",
        goto_next_mark, "Goto next lowercase mark (]`)",
        goto_prev_mark_line, "Goto previous lowercase mark, line start (['])",
        goto_next_mark_line, "Goto next lowercase mark, line start (]')",
        yank_file_path, "Yank current file path to clipboard",
        yank_file_name, "Yank current file name to clipboard",
        yank_file_path_with_line, "Yank current file path:line to clipboard",
        yank_file_path_with_line_col, "Yank current file path:line:col to clipboard",
        yank_file_dir, "Yank current file's directory to clipboard",
        copy_remote_url, "Copy web permalink (host/blob/<sha>/path#Ln) for current line",
        open_remote_url, "Open current line's web permalink in the browser",
        open_url_under_cursor, "Open the URL under the cursor in the browser",
        duplicate_selection_down, "Duplicate current line(s) downward",
        duplicate_selection_up, "Duplicate current line(s) upward",
        move_text_line_down, "Move current line(s) down past the next line",
        move_text_line_up, "Move current line(s) up past the previous line",
        count_selection, "Count chars/words/lines in selection",
        match_brackets, "Goto matching bracket",
        match_brackets_or_goto_percent, "Goto matching bracket, or {count} percent through the file",
        surround_add, "Surround add",
        surround_replace, "Surround replace",
        surround_delete, "Surround delete",
        select_textobject_around, "Select around object",
        select_textobject_inner, "Select inside object",
        change_textobject_inner, "Change inside object (ci)",
        change_textobject_around, "Change around object (ca)",
        delete_textobject_inner, "Delete inside object (di)",
        delete_textobject_around, "Delete around object (da)",
        yank_textobject_inner, "Yank inside object (yi)",
        yank_textobject_around, "Yank around object (ya)",
        delete_find_char_forward, "Delete to next char (df)",
        delete_till_char_forward, "Delete till next char (dt)",
        delete_find_char_backward, "Delete to prev char (dF)",
        delete_till_char_backward, "Delete till prev char (dT)",
        change_find_char_forward, "Change to next char (cf)",
        change_till_char_forward, "Change till next char (ct)",
        change_find_char_backward, "Change to prev char (cF)",
        change_till_char_backward, "Change till prev char (cT)",
        yank_find_char_forward, "Yank to next char (yf)",
        yank_till_char_forward, "Yank till next char (yt)",
        yank_find_char_backward, "Yank to prev char (yF)",
        yank_till_char_backward, "Yank till prev char (yT)",
        set_mark, "Set mark (m{a-z})",
        goto_mark, "Goto mark exact (`{a-z})",
        goto_mark_line, "Goto mark line ('{a-z})",
        repeat_substitute, "Repeat last :substitute (&)",
        repeat_substitute_global, "Repeat last :substitute on whole file (g&)",
        vim_record_macro, "Record macro into register (q{reg})",
        vim_replay_macro, "Replay macro from register (@{reg})",
        save_visual_selection, "Save the visual selection (for gv)",
        reselect_visual, "Reselect the last visual area (gv)",
        mark_insert_exit, "Record the insert-exit position (for gi)",
        insert_at_last_insert, "Insert at the last insert position (gi)",
        goto_next_function, "Goto next function",
        goto_prev_function, "Goto previous function",
        goto_next_class, "Goto next type definition",
        goto_prev_class, "Goto previous type definition",
        goto_next_parameter, "Goto next parameter",
        goto_prev_parameter, "Goto previous parameter",
        goto_next_comment, "Goto next comment",
        goto_prev_comment, "Goto previous comment",
        goto_next_test, "Goto next test",
        goto_prev_test, "Goto previous test",
        goto_next_xml_element, "Goto next (X)HTML element",
        goto_prev_xml_element, "Goto previous (X)HTML element",
        goto_next_entry, "Goto next pairing",
        goto_prev_entry, "Goto previous pairing",
        goto_next_paragraph, "Goto next paragraph",
        goto_prev_paragraph, "Goto previous paragraph",
        move_sentence_forward, "Move to next sentence",
        move_sentence_backward, "Move to previous sentence",
        dap_launch, "Launch debug target",
        dap_restart, "Restart debugging session",
        dap_toggle_breakpoint, "Toggle breakpoint",
        dap_continue, "Continue program execution",
        dap_run_to_cursor, "Run the debugger up to the cursor line (JetBrains Run To Cursor)",
        dap_pause, "Pause program execution",
        dap_step_in, "Step in",
        dap_step_out, "Step out",
        dap_next, "Step to next",
        dap_variables, "List variables",
        dap_terminate, "End debug session",
        dap_edit_condition, "Edit breakpoint condition on current line",
        dap_edit_log, "Edit breakpoint log message on current line",
        dap_switch_thread, "Switch current thread",
        dap_switch_stack_frame, "Switch stack frame",
        dap_enable_exceptions, "Enable exception breakpoints",
        dap_disable_exceptions, "Disable exception breakpoints",
        shell_pipe, "Pipe selections through shell command",
        shell_pipe_to, "Pipe selections into shell command ignoring output",
        shell_insert_output, "Insert shell command output before selections",
        shell_append_output, "Append shell command output after selections",
        shell_keep_pipe, "Filter selections with shell predicate",
        suspend, "Suspend and return to shell",
        rename_symbol, "Rename symbol",
        increment, "Increment item under cursor",
        decrement, "Decrement item under cursor",
        record_macro, "Record macro",
        replay_macro, "Replay macro",
        command_palette, "Open command palette",
        repl, "Open the embedded-language REPL (elisp/viml/stryke/awk/zsh)",
        goto_word, "Jump to a two-character label",
        extend_to_word, "Extend to a two-character label",
        goto_next_tabstop, "Goto next snippet placeholder",
        goto_prev_tabstop, "Goto next snippet placeholder",
        emmet_expand, "Expand emmet/zen HTML abbreviation (or Tab)",
        snippet_expand, "Expand the user snippet whose trigger precedes the cursor",
        rotate_selections_first, "Make the first selection your primary one",
        rotate_selections_last, "Make the last selection your primary one",
    );
}

impl fmt::Debug for MappableCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MappableCommand::Static { name, .. } => {
                f.debug_tuple("MappableCommand").field(name).finish()
            }
            MappableCommand::Typable { name, args, .. } => f
                .debug_tuple("MappableCommand")
                .field(name)
                .field(args)
                .finish(),
            MappableCommand::Macro { name, keys, .. } => f
                .debug_tuple("MappableCommand")
                .field(name)
                .field(keys)
                .finish(),
        }
    }
}

impl fmt::Display for MappableCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

impl std::str::FromStr for MappableCommand {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Some(suffix) = s.strip_prefix(':') {
            let (name, args, _) = command_line::split(suffix);
            ensure!(!name.is_empty(), "Expected typable command name");
            typed::TYPABLE_COMMAND_MAP
                .get(name)
                .map(|cmd| {
                    let doc = if args.is_empty() {
                        cmd.doc.to_string()
                    } else {
                        format!(":{} {:?}", cmd.name, args)
                    };
                    MappableCommand::Typable {
                        name: cmd.name.to_owned(),
                        doc,
                        args: args.to_string(),
                    }
                })
                .ok_or_else(|| anyhow!("No TypableCommand named '{}'", s))
        } else if let Some(suffix) = s.strip_prefix('@') {
            zemacs_view::input::parse_macro(suffix).map(|keys| Self::Macro {
                name: s.to_string(),
                keys,
            })
        } else {
            MappableCommand::STATIC_COMMAND_LIST
                .iter()
                .find(|cmd| cmd.name() == s)
                .cloned()
                .ok_or_else(|| anyhow!("No command named '{}'", s))
        }
    }
}

impl<'de> Deserialize<'de> for MappableCommand {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(de::Error::custom)
    }
}

impl PartialEq for MappableCommand {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (
                MappableCommand::Typable {
                    name: first_name,
                    args: first_args,
                    ..
                },
                MappableCommand::Typable {
                    name: second_name,
                    args: second_args,
                    ..
                },
            ) => first_name == second_name && first_args == second_args,
            (
                MappableCommand::Static {
                    name: first_name, ..
                },
                MappableCommand::Static {
                    name: second_name, ..
                },
            ) => first_name == second_name,
            _ => false,
        }
    }
}

fn no_op(_cx: &mut Context) {}

type MoveFn =
    fn(RopeSlice, Range, Direction, usize, Movement, &TextFormat, &mut TextAnnotations) -> Range;

fn move_impl(cx: &mut Context, move_fn: MoveFn, dir: Direction, behaviour: Movement) {
    let count = cx.count();
    let (view, doc) = current!(cx.editor);
    let text = doc.text().slice(..);
    let text_fmt = doc.text_format(view.inner_area(doc).width, None, Some(view.id));
    let mut annotations = view.text_annotations(doc, None);

    let selection = doc.selection(view.id).clone().transform(|range| {
        move_fn(
            text,
            range,
            dir,
            count,
            behaviour,
            &text_fmt,
            &mut annotations,
        )
    });
    drop(annotations);
    doc.set_selection(view.id, selection);
}

use zemacs_core::movement::{move_horizontally, move_vertically};

fn move_char_left(cx: &mut Context) {
    move_impl(cx, move_horizontally, Direction::Backward, Movement::Move)
}

fn move_char_right(cx: &mut Context) {
    move_impl(cx, move_horizontally, Direction::Forward, Movement::Move)
}

fn move_line_up(cx: &mut Context) {
    move_impl(cx, move_vertically, Direction::Backward, Movement::Move)
}

fn move_line_down(cx: &mut Context) {
    move_impl(cx, move_vertically, Direction::Forward, Movement::Move)
}

/// Reorder two adjacent lines: line A (with its trailing newline) and the next
/// line B (which may lack a trailing newline if it's the final line) → the
/// region text with B before A, preserving newline structure. Pure (tested).
fn reorder_two_lines(a: &str, b: &str, le: &str) -> String {
    if a.ends_with('\n') && !b.ends_with('\n') {
        format!("{}{}{}", b, le, a.trim_end_matches(['\n', '\r']))
    } else {
        format!("{}{}", b, a)
    }
}

/// Drag the current line down/up, moving the cursor with it (Spacemacs
/// drag-stuff). Minimal two-line transaction (no whole-buffer rewrite).
fn drag_line(cx: &mut Context, down: bool) {
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let n = text.len_lines();
    if n == 0 || text.len_chars() == 0 {
        return;
    }
    let ends_nl = text.char(text.len_chars() - 1) == '\n';
    let real_last = if ends_nl {
        n.saturating_sub(2)
    } else {
        n.saturating_sub(1)
    };
    let slice = text.slice(..);
    let cur = text
        .char_to_line(doc.selection(view.id).primary().cursor(slice))
        .min(real_last);
    let (a, b) = if down {
        if cur >= real_last {
            return;
        }
        (cur, cur + 1)
    } else {
        if cur == 0 {
            return;
        }
        (cur - 1, cur)
    };
    let a_start = text.line_to_char(a);
    let b_start = text.line_to_char(b);
    let b_end = text.line_to_char((b + 1).min(n));
    let la = text.slice(a_start..b_start).to_string();
    let lb = text.slice(b_start..b_end).to_string();
    let le = doc.line_ending.as_str();
    let swapped = reorder_two_lines(&la, &lb, le);
    let transaction =
        Transaction::change(text, std::iter::once((a_start, b_end, Some(swapped.into()))));
    doc.apply(&transaction, view.id);
    let (view, doc) = current!(cx.editor);
    let target = (if down { b } else { a }).min(doc.text().len_lines().saturating_sub(1));
    let pos = doc.text().line_to_char(target);
    doc.set_selection(view.id, Selection::point(pos));
}
fn drag_line_down(cx: &mut Context) { drag_line(cx, true) }
fn drag_line_up(cx: &mut Context) { drag_line(cx, false) }

fn move_visual_line_up(cx: &mut Context) {
    move_impl(
        cx,
        move_vertically_visual,
        Direction::Backward,
        Movement::Move,
    )
}

fn move_visual_line_down(cx: &mut Context) {
    move_impl(
        cx,
        move_vertically_visual,
        Direction::Forward,
        Movement::Move,
    )
}

fn extend_char_left(cx: &mut Context) {
    move_impl(cx, move_horizontally, Direction::Backward, Movement::Extend)
}

fn extend_char_right(cx: &mut Context) {
    move_impl(cx, move_horizontally, Direction::Forward, Movement::Extend)
}

fn extend_line_up(cx: &mut Context) {
    move_impl(cx, move_vertically, Direction::Backward, Movement::Extend)
}

fn extend_line_down(cx: &mut Context) {
    move_impl(cx, move_vertically, Direction::Forward, Movement::Extend)
}

fn extend_visual_line_up(cx: &mut Context) {
    move_impl(
        cx,
        move_vertically_visual,
        Direction::Backward,
        Movement::Extend,
    )
}

fn extend_visual_line_down(cx: &mut Context) {
    move_impl(
        cx,
        move_vertically_visual,
        Direction::Forward,
        Movement::Extend,
    )
}

fn goto_line_end_impl(view: &mut View, doc: &mut Document, movement: Movement) {
    let text = doc.text().slice(..);

    let selection = doc.selection(view.id).clone().transform(|range| {
        let line = range.cursor_line(text);
        let line_start = text.line_to_char(line);

        let pos = graphemes::prev_grapheme_boundary(text, line_end_char_index(&text, line))
            .max(line_start);

        range.put_cursor(text, pos, movement == Movement::Extend)
    });
    doc.set_selection(view.id, selection);
}

fn goto_line_end(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);
    goto_line_end_impl(
        view,
        doc,
        if cx.editor.mode == Mode::Select {
            Movement::Extend
        } else {
            Movement::Move
        },
    )
}

fn extend_to_line_end(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);
    goto_line_end_impl(view, doc, Movement::Extend)
}

fn goto_line_end_newline_impl(view: &mut View, doc: &mut Document, movement: Movement) {
    let text = doc.text().slice(..);

    let selection = doc.selection(view.id).clone().transform(|range| {
        let line = range.cursor_line(text);
        let pos = line_end_char_index(&text, line);

        range.put_cursor(text, pos, movement == Movement::Extend)
    });
    doc.set_selection(view.id, selection);
}

fn goto_line_end_newline(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);
    goto_line_end_newline_impl(
        view,
        doc,
        if cx.editor.mode == Mode::Select {
            Movement::Extend
        } else {
            Movement::Move
        },
    )
}

fn extend_to_line_end_newline(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);
    goto_line_end_newline_impl(view, doc, Movement::Extend)
}

fn goto_line_start_impl(view: &mut View, doc: &mut Document, movement: Movement) {
    let text = doc.text().slice(..);

    let selection = doc.selection(view.id).clone().transform(|range| {
        let line = range.cursor_line(text);

        // adjust to start of the line
        let pos = text.line_to_char(line);
        range.put_cursor(text, pos, movement == Movement::Extend)
    });
    doc.set_selection(view.id, selection);
}

fn goto_line_start(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);
    goto_line_start_impl(
        view,
        doc,
        if cx.editor.mode == Mode::Select {
            Movement::Extend
        } else {
            Movement::Move
        },
    )
}

fn goto_next_buffer(cx: &mut Context) {
    goto_buffer(cx.editor, Direction::Forward, cx.count());
}

fn goto_previous_buffer(cx: &mut Context) {
    goto_buffer(cx.editor, Direction::Backward, cx.count());
}

fn goto_buffer(editor: &mut Editor, direction: Direction, count: usize) {
    let current = view!(editor).doc;

    let id = match direction {
        Direction::Forward => {
            let iter = editor.documents.keys();
            // skip 'count' times past current buffer
            iter.cycle().skip_while(|id| *id != &current).nth(count)
        }
        Direction::Backward => {
            let iter = editor.documents.keys();
            // skip 'count' times past current buffer
            iter.rev()
                .cycle()
                .skip_while(|id| *id != &current)
                .nth(count)
        }
    }
    .unwrap();

    let id = *id;

    editor.switch(id, Action::Replace);
}

fn extend_to_line_start(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);
    goto_line_start_impl(view, doc, Movement::Extend)
}

fn kill_to_line_start(cx: &mut Context) {
    delete_by_selection_insert_mode(
        cx,
        move |text, range| {
            let line = range.cursor_line(text);
            let first_char = text.line_to_char(line);
            let anchor = range.cursor(text);
            let head = if anchor == first_char && line != 0 {
                // select until previous line
                line_end_char_index(&text, line - 1)
            } else if let Some(pos) = text.line(line).first_non_whitespace_char() {
                if first_char + pos < anchor {
                    // select until first non-blank in line if cursor is after it
                    first_char + pos
                } else {
                    // select until start of line
                    first_char
                }
            } else {
                // select until start of line
                first_char
            };
            (head, anchor)
        },
        Direction::Backward,
    );
}

fn kill_to_line_end(cx: &mut Context) {
    delete_by_selection_insert_mode(
        cx,
        |text, range| {
            let line = range.cursor_line(text);
            let line_end_pos = line_end_char_index(&text, line);
            let pos = range.cursor(text);

            // if the cursor is on the newline char delete that
            if pos == line_end_pos {
                (pos, text.line_to_char(line + 1))
            } else {
                (pos, line_end_pos)
            }
        },
        Direction::Forward,
    );
}

fn goto_first_nonwhitespace(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);

    goto_first_nonwhitespace_impl(
        view,
        doc,
        if cx.editor.mode == Mode::Select {
            Movement::Extend
        } else {
            Movement::Move
        },
    )
}

fn extend_to_first_nonwhitespace(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);
    goto_first_nonwhitespace_impl(view, doc, Movement::Extend)
}

fn goto_first_nonwhitespace_impl(view: &mut View, doc: &mut Document, movement: Movement) {
    let text = doc.text().slice(..);

    let selection = doc.selection(view.id).clone().transform(|range| {
        let line = range.cursor_line(text);

        if let Some(pos) = text.line(line).first_non_whitespace_char() {
            let pos = pos + text.line_to_char(line);
            range.put_cursor(text, pos, movement == Movement::Extend)
        } else {
            range
        }
    });
    doc.set_selection(view.id, selection);
}

// vim `g_`: to the last non-blank character of the line.
fn goto_line_last_nonblank(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);
    let text = doc.text().slice(..);
    let extend = cx.editor.mode == Mode::Select;
    let selection = doc.selection(view.id).clone().transform(|range| {
        let line = range.cursor_line(text);
        let start = text.line_to_char(line);
        let end = line_end_char_index(&text, line);
        // scan back from the line end to the last non-whitespace grapheme
        let mut pos = end;
        while pos > start {
            let prev = graphemes::prev_grapheme_boundary(text, pos);
            if text.slice(prev..pos).chars().any(|c| !c.is_whitespace()) {
                pos = prev;
                break;
            }
            pos = prev;
        }
        range.put_cursor(text, pos, extend)
    });
    doc.set_selection(view.id, selection);
}

// vim `gM`: to the character at the middle of the text line (by length).
fn goto_line_middle(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);
    let text = doc.text().slice(..);
    let extend = cx.editor.mode == Mode::Select;
    let selection = doc.selection(view.id).clone().transform(|range| {
        let line = range.cursor_line(text);
        let start = text.line_to_char(line);
        let end = line_end_char_index(&text, line);
        let pos = start + (end - start) / 2;
        range.put_cursor(text, pos, extend)
    });
    doc.set_selection(view.id, selection);
}

// vim `go`: to byte {count} in the buffer (1-based; default 1).
fn goto_byte(cx: &mut Context) {
    let byte = cx.count().saturating_sub(1);
    let (view, doc) = current!(cx.editor);
    let text = doc.text().slice(..);
    let byte = byte.min(text.len_bytes());
    let char_idx = text.byte_to_char(byte);
    let extend = cx.editor.mode == Mode::Select;
    let selection = doc
        .selection(view.id)
        .clone()
        .transform(|range| range.put_cursor(text, char_idx, extend));
    doc.set_selection(view.id, selection);
}

// vim `[(` `[{` `])` `]}`: jump to the {count}th previous/next *unmatched*
// bracket of the given pair, honoring nesting. Plaintext scan (no syntax needed).
fn find_unmatched_bracket(
    text: RopeSlice,
    cursor: usize,
    open: char,
    close: char,
    forward: bool,
    count: usize,
) -> Option<usize> {
    let mut depth = 0i32;
    let mut remaining = count.max(1);
    if forward {
        for i in (cursor + 1)..text.len_chars() {
            let ch = text.char(i);
            if ch == open {
                depth += 1;
            } else if ch == close {
                if depth == 0 {
                    remaining -= 1;
                    if remaining == 0 {
                        return Some(i);
                    }
                } else {
                    depth -= 1;
                }
            }
        }
    } else {
        for i in (0..cursor).rev() {
            let ch = text.char(i);
            if ch == close {
                depth += 1;
            } else if ch == open {
                if depth == 0 {
                    remaining -= 1;
                    if remaining == 0 {
                        return Some(i);
                    }
                } else {
                    depth -= 1;
                }
            }
        }
    }
    None
}

fn goto_unmatched_bracket(cx: &mut Context, open: char, close: char, forward: bool) {
    let count = cx.count();
    let (view, doc) = current!(cx.editor);
    let text = doc.text().slice(..);
    let extend = cx.editor.mode == Mode::Select;
    let selection = doc.selection(view.id).clone().transform(|range| {
        let cursor = range.cursor(text);
        match find_unmatched_bracket(text, cursor, open, close, forward, count) {
            Some(pos) => range.put_cursor(text, pos, extend),
            None => range,
        }
    });
    doc.set_selection(view.id, selection);
}

fn goto_prev_unmatched_paren(cx: &mut Context) {
    goto_unmatched_bracket(cx, '(', ')', false);
}
fn goto_prev_unmatched_brace(cx: &mut Context) {
    goto_unmatched_bracket(cx, '{', '}', false);
}
fn goto_next_unmatched_paren(cx: &mut Context) {
    goto_unmatched_bracket(cx, '(', ')', true);
}
fn goto_next_unmatched_brace(cx: &mut Context) {
    goto_unmatched_bracket(cx, '{', '}', true);
}

// vim `[`` ` `` / `]`` ` `` (and the `['` / `]'` line variants): jump to the
// previous / next lowercase mark relative to the cursor.
fn goto_adjacent_mark(cx: &mut Context, forward: bool, to_line_start: bool) {
    let (view, doc) = current!(cx.editor);
    let text = doc.text().slice(..);
    let cursor = doc.selection(view.id).primary().cursor(text);
    let marks = doc.lowercase_mark_positions();
    let target = if forward {
        marks.into_iter().filter(|&p| p > cursor).min()
    } else {
        marks.into_iter().filter(|&p| p < cursor).max()
    };
    let Some(mut pos) = target else {
        cx.editor.set_error("No mark in that direction");
        return;
    };
    if to_line_start {
        let line = text.char_to_line(pos);
        pos = text
            .line(line)
            .first_non_whitespace_char()
            .map(|p| p + text.line_to_char(line))
            .unwrap_or_else(|| text.line_to_char(line));
    }
    push_jump(view, doc);
    doc.set_selection(view.id, Selection::point(pos));
}

fn goto_prev_mark(cx: &mut Context) {
    goto_adjacent_mark(cx, false, false);
}
fn goto_next_mark(cx: &mut Context) {
    goto_adjacent_mark(cx, true, false);
}
fn goto_prev_mark_line(cx: &mut Context) {
    goto_adjacent_mark(cx, false, true);
}
fn goto_next_mark_line(cx: &mut Context) {
    goto_adjacent_mark(cx, true, true);
}

// spacemacs `SPC f y y` / `y n` / `y l`: copy the current file's path / name /
// path-with-line into the clipboard register.
#[derive(Clone, Copy)]
enum FilePathKind {
    Full,
    Name,
    WithLine,
    WithLineCol,
    Dir,
}

/// Pure path formatter for the yank-file-path commands (unit tested).
fn format_file_path(path: &std::path::Path, kind: FilePathKind, line: usize, col: usize) -> String {
    match kind {
        FilePathKind::Full => path.display().to_string(),
        FilePathKind::Name => path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        FilePathKind::WithLine => format!("{}:{}", path.display(), line),
        FilePathKind::WithLineCol => format!("{}:{}:{}", path.display(), line, col),
        FilePathKind::Dir => path
            .parent()
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
    }
}

fn yank_file_path_kind(cx: &mut Context, kind: FilePathKind) {
    let formatted = {
        let (view, doc) = current!(cx.editor);
        doc.path().map(|path| {
            let text = doc.text().slice(..);
            let cursor = doc.selection(view.id).primary().cursor(text);
            let line = text.char_to_line(cursor);
            let col = cursor - text.line_to_char(line) + 1;
            format_file_path(path, kind, line + 1, col)
        })
    };
    match formatted {
        Some(s) => {
            let _ = cx.editor.registers.write('+', vec![s.clone()]);
            cx.editor.set_status(format!("Yanked to clipboard: {s}"));
        }
        None => cx.editor.set_error("buffer has no file path"),
    }
}

fn yank_file_path(cx: &mut Context) {
    yank_file_path_kind(cx, FilePathKind::Full);
}
fn yank_file_name(cx: &mut Context) {
    yank_file_path_kind(cx, FilePathKind::Name);
}
fn yank_file_path_with_line(cx: &mut Context) {
    yank_file_path_kind(cx, FilePathKind::WithLine);
}
fn yank_file_path_with_line_col(cx: &mut Context) {
    yank_file_path_kind(cx, FilePathKind::WithLineCol);
}
fn yank_file_dir(cx: &mut Context) {
    yank_file_path_kind(cx, FilePathKind::Dir);
}

/// Convert a git remote URL (ssh, scp-like, git://, or `http[s]` form) to its base
/// web URL: `git@github.com:o/r.git` / `https://github.com/o/r.git` → `https://github.com/o/r`.
fn git_remote_to_web_base(remote: &str) -> Option<String> {
    let r = remote.trim();
    let r = r.strip_suffix(".git").unwrap_or(r);
    if let Some(rest) = r.strip_prefix("git@") {
        // scp-like: git@host:owner/repo
        if let Some((host, path)) = rest.split_once(':') {
            return Some(format!("https://{host}/{path}"));
        }
    }
    if let Some(rest) = r.strip_prefix("ssh://") {
        let rest = rest.strip_prefix("git@").unwrap_or(rest);
        // ssh://[git@]host[:port]/owner/repo
        if let Some((hostport, path)) = rest.split_once('/') {
            let host = hostport.split(':').next().unwrap_or(hostport);
            return Some(format!("https://{host}/{path}"));
        }
    }
    if r.starts_with("https://") || r.starts_with("http://") {
        return Some(r.to_string());
    }
    if let Some(rest) = r.strip_prefix("git://") {
        return Some(format!("https://{rest}"));
    }
    None
}

/// Build a web permalink to `rel_path` at `git_ref`, line `line`, from a base web
/// URL. GitHub/GitLab use `/blob/<ref>/<path>#L<n>`; Bitbucket uses
/// `/src/<ref>/<path>#lines-<n>`.
fn build_permalink(web_base: &str, git_ref: &str, rel_path: &str, line: usize) -> String {
    let rel = rel_path.trim_start_matches('/');
    if web_base.contains("bitbucket") {
        format!("{web_base}/src/{git_ref}/{rel}#lines-{line}")
    } else {
        format!("{web_base}/blob/{git_ref}/{rel}#L{line}")
    }
}

fn git_out(dir: &std::path::Path, args: &[&str]) -> Option<String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// Copy a web permalink (`host/blob/<sha>/<path>#L<line>`) for the current line to
/// the clipboard — GitLens "Copy permalink" analogue. Pins to the exact HEAD
/// commit so the link stays valid as the branch moves.
/// Compute the web permalink for the primary cursor's line, or an error message
/// suitable for `set_error`. Shared by [`copy_remote_url`] and [`open_remote_url`].
fn current_line_permalink(cx: &mut Context) -> Result<String, String> {
    let (path, line) = {
        let (view, doc) = current!(cx.editor);
        let path = doc
            .path()
            .map(|p| p.to_path_buf())
            .ok_or("buffer has no file path")?;
        let text = doc.text().slice(..);
        let cursor = doc.selection(view.id).primary().cursor(text);
        let line = text.char_to_line(cursor) + 1;
        (path, line)
    };
    let dir = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    let remote = git_out(dir, &["remote", "get-url", "origin"]).ok_or("no git remote 'origin'")?;
    let web_base = git_remote_to_web_base(&remote)
        .ok_or_else(|| format!("unsupported remote URL: {remote}"))?;
    let root = git_out(dir, &["rev-parse", "--show-toplevel"]).ok_or("not in a git repository")?;
    let git_ref = git_out(dir, &["rev-parse", "HEAD"]).unwrap_or_else(|| "HEAD".into());
    let rel = path
        .strip_prefix(root.as_str())
        .unwrap_or(&path)
        .to_string_lossy()
        .replace('\\', "/");
    Ok(build_permalink(&web_base, &git_ref, &rel, line))
}

fn copy_remote_url(cx: &mut Context) {
    match current_line_permalink(cx) {
        Ok(url) => {
            let _ = cx.editor.registers.write('+', vec![url.clone()]);
            cx.editor.set_status(format!("Yanked permalink: {url}"));
        }
        Err(e) => cx.editor.set_error(e),
    }
}

/// Open a URL in the OS default browser (detached; output suppressed).
fn open_in_browser(url: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let opener = "open";
    #[cfg(target_os = "windows")]
    let opener = "explorer";
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let opener = "xdg-open";
    std::process::Command::new(opener)
        .arg(url)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Open the current line's web permalink in the OS browser — "Open on GitHub".
fn open_remote_url(cx: &mut Context) {
    match current_line_permalink(cx) {
        Ok(url) => match open_in_browser(&url) {
            Ok(()) => cx.editor.set_status(format!("Opening {url}")),
            Err(e) => cx.editor.set_error(format!("failed to open browser: {e}")),
        },
        Err(e) => cx.editor.set_error(e),
    }
}

/// Find the URL (`http(s)://…` or `www.…`) at char column `col` in `line`, or
/// `None`. Trailing sentence punctuation is trimmed. Pure — unit tested.
fn url_at(line: &str, col: usize) -> Option<String> {
    let chars: Vec<char> = line.chars().collect();
    if chars.is_empty() {
        return None;
    }
    let is_url_char =
        |c: char| !c.is_whitespace() && !matches!(c, '<' | '>' | '"' | '\'' | '(' | ')' | '`');
    let mut i = col.min(chars.len() - 1);
    if !is_url_char(chars[i]) {
        if i > 0 && is_url_char(chars[i - 1]) {
            i -= 1;
        } else {
            return None;
        }
    }
    let mut start = i;
    while start > 0 && is_url_char(chars[start - 1]) {
        start -= 1;
    }
    let mut end = i;
    while end < chars.len() && is_url_char(chars[end]) {
        end += 1;
    }
    let token: String = chars[start..end].iter().collect();
    let token = token.trim_end_matches(['.', ',', ';', ':', '!', '?']);
    if token.starts_with("http://") || token.starts_with("https://") || token.starts_with("www.") {
        Some(token.to_string())
    } else {
        None
    }
}

/// `:open-url` — open the URL under the cursor in the OS browser.
fn open_url_under_cursor(cx: &mut Context) {
    let (line, col) = {
        let (view, doc) = current!(cx.editor);
        let text = doc.text().slice(..);
        let cursor = doc.selection(view.id).primary().cursor(text);
        let line_idx = text.char_to_line(cursor);
        let col = cursor - text.line_to_char(line_idx);
        let line: String = text
            .line(line_idx)
            .chars()
            .filter(|c| *c != '\n' && *c != '\r')
            .collect();
        (line, col)
    };
    match url_at(&line, col) {
        Some(u) => {
            let url = if u.starts_with("www.") {
                format!("https://{u}")
            } else {
                u
            };
            match open_in_browser(&url) {
                Ok(()) => cx.editor.set_status(format!("Opening {url}")),
                Err(e) => cx.editor.set_error(format!("failed to open browser: {e}")),
            }
        }
        None => cx.editor.set_error("no URL under cursor"),
    }
}

// spacemacs `SPC x c`: count characters / words / lines in the selection.
/// Pure counter over a string slice (unit tested).
fn count_region(s: &str) -> (usize, usize, usize) {
    let chars = s.chars().count();
    let words = s.split_whitespace().count();
    // lines spanned: number of newlines + 1 (a non-empty region covers >=1 line)
    let lines = if s.is_empty() {
        0
    } else {
        s.matches('\n').count() + 1
    };
    (chars, words, lines)
}

fn count_selection(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);
    let text = doc.text().slice(..);
    let s = doc.selection(view.id).primary().slice(text).to_string();
    let (chars, words, lines) = count_region(&s);
    cx.editor
        .set_status(format!("{lines} lines, {words} words, {chars} chars"));
}

fn trim_selections(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);
    let text = doc.text().slice(..);

    let ranges: SmallVec<[Range; 1]> = doc
        .selection(view.id)
        .iter()
        .filter_map(|range| {
            if range.is_empty() || range.slice(text).chars().all(|ch| ch.is_whitespace()) {
                return None;
            }
            let mut start = range.from();
            let mut end = range.to();
            start = movement::skip_while(text, start, |x| x.is_whitespace()).unwrap_or(start);
            end = movement::backwards_skip_while(text, end, |x| x.is_whitespace()).unwrap_or(end);
            Some(Range::new(start, end).with_direction(range.direction()))
        })
        .collect();

    if !ranges.is_empty() {
        let primary = doc.selection(view.id).primary();
        let idx = ranges
            .iter()
            .position(|range| range.overlaps(&primary))
            .unwrap_or(ranges.len() - 1);
        doc.set_selection(view.id, Selection::new(ranges, idx));
    } else {
        collapse_selection(cx);
        keep_primary_selection(cx);
    };
}

// align text in selection
#[allow(deprecated)]
fn align_selections(cx: &mut Context) {
    use zemacs_core::visual_coords_at_pos;

    let (view, doc) = current!(cx.editor);
    let text = doc.text().slice(..);
    let selection = doc.selection(view.id);

    let tab_width = doc.tab_width();

    let mut column_widths: Vec<usize> = Vec::new();
    let mut coordinates = Vec::with_capacity(selection.len());

    let mut previous_line = usize::MAX;
    let mut col_idx = 0;
    let mut running_offset = 0;

    for range in selection {
        let coords = visual_coords_at_pos(text, range.head, tab_width);
        let anchor_coords = visual_coords_at_pos(text, range.anchor, tab_width);

        if coords.row != anchor_coords.row {
            cx.editor
                .set_error("align cannot work with multi line selections");
            return;
        }
        if coords.row != previous_line {
            col_idx = 0;
            running_offset = 0;
            previous_line = coords.row;
        }

        let width = coords.col - running_offset;

        match column_widths.get_mut(col_idx) {
            Some(n) => *n = (*n).max(width),
            None => column_widths.push(width),
        }

        coordinates.push(coords);

        running_offset += width;
        col_idx += 1;
    }

    let column_positions: Vec<_> = column_widths
        .into_iter()
        .scan(0, |sum, n| {
            *sum += n;
            Some(*sum)
        })
        .collect();

    previous_line = usize::MAX;

    let changes = coordinates
        .into_iter()
        .zip(selection)
        .map(|(coords, range)| {
            if coords.row != previous_line {
                col_idx = 0;
                running_offset = 0;
                previous_line = coords.row;
            }
            let current_inserts = column_positions[col_idx] - coords.col - running_offset;
            let insert_pos = range.from();

            col_idx += 1;
            running_offset += current_inserts;

            (
                insert_pos,
                insert_pos,
                Some(" ".repeat(current_inserts).into()),
            )
        });

    let transaction = Transaction::change(doc.text(), changes);
    doc.apply(&transaction, view.id);
    exit_select_mode(cx);
}

fn goto_window(cx: &mut Context, align: Align) {
    let count = cx.count() - 1;
    let config = cx.editor.config();
    let (view, doc) = current!(cx.editor);
    let view_offset = doc.view_offset(view.id);

    let height = view.inner_height();

    // respect user given count if any
    // - 1 so we have at least one gap in the middle.
    // a height of 6 with padding of 3 on each side will keep shifting the view back and forth
    // as we type
    let scrolloff = config.scrolloff.min(height.saturating_sub(1) / 2);

    let last_visual_line = view.last_visual_line(doc);

    let visual_line = match align {
        Align::Top => view_offset.vertical_offset + scrolloff + count,
        Align::Center => view_offset.vertical_offset + (last_visual_line / 2),
        Align::Bottom => {
            view_offset.vertical_offset + last_visual_line.saturating_sub(scrolloff + count)
        }
    };
    let visual_line = visual_line
        .max(view_offset.vertical_offset + scrolloff)
        .min(view_offset.vertical_offset + last_visual_line.saturating_sub(scrolloff));

    let pos = view
        .pos_at_visual_coords(doc, visual_line as u16, 0, false)
        .expect("visual_line was constrained to the view area");

    let text = doc.text().slice(..);
    let selection = doc
        .selection(view.id)
        .clone()
        .transform(|range| range.put_cursor(text, pos, cx.editor.mode == Mode::Select));
    doc.set_selection(view.id, selection);
}

fn goto_window_top(cx: &mut Context) {
    goto_window(cx, Align::Top)
}

fn goto_window_center(cx: &mut Context) {
    goto_window(cx, Align::Center)
}

fn goto_window_bottom(cx: &mut Context) {
    goto_window(cx, Align::Bottom)
}

fn move_word_impl<F>(cx: &mut Context, move_fn: F)
where
    F: Fn(RopeSlice, Range, usize) -> Range,
{
    let count = cx.count();
    let (view, doc) = current!(cx.editor);
    let text = doc.text().slice(..);

    let selection = doc
        .selection(view.id)
        .clone()
        .transform(|range| move_fn(text, range, count));
    doc.set_selection(view.id, selection);
}

/// vim normal-mode word motion (`w`/`b`/`e`/`ge` and their long-word forms).
///
/// The underlying motions are Helix's selection-based ones: they return a range
/// whose head is one past the target on the side away from the cursor, and Helix
/// then renders the block cursor at `range.cursor()` — `head - 1` for a forward
/// range, `head` for a backward one. That is off-by-one from vim for word-START
/// motions (`w`/`W` must land *on* the next word's first char) and word-END
/// motions (`ge`/`gE` must land *on* the previous word's last char).
///
/// vim's caret is always the motion's target character: `head` for a START
/// target, the grapheme just before `head` for an END target — independent of
/// travel direction. Collapse to that point so vim motions land exactly like
/// vim. (`b`/`e` already happen to match; applying the rule uniformly leaves
/// them unchanged.)
fn move_word_vim_impl<F>(cx: &mut Context, move_fn: F, end_target: bool)
where
    F: Fn(RopeSlice, Range, usize) -> Range,
{
    let count = cx.count();
    let (view, doc) = current!(cx.editor);
    let text = doc.text().slice(..);

    let selection = doc.selection(view.id).clone().transform(|range| {
        let moved = move_fn(text, range, count);
        let caret = if end_target {
            graphemes::prev_grapheme_boundary(text, moved.head)
        } else {
            moved.head
        };
        Range::point(caret)
    });
    doc.set_selection(view.id, selection);
}

fn vim_move_next_word_start(cx: &mut Context) {
    move_word_vim_impl(cx, movement::move_next_word_start, false)
}

fn vim_move_prev_word_start(cx: &mut Context) {
    move_word_vim_impl(cx, movement::move_prev_word_start, false)
}

fn vim_move_next_word_end(cx: &mut Context) {
    move_word_vim_impl(cx, movement::move_next_word_end, true)
}

fn vim_move_prev_word_end(cx: &mut Context) {
    move_word_vim_impl(cx, movement::move_prev_word_end, true)
}

fn vim_move_next_long_word_start(cx: &mut Context) {
    move_word_vim_impl(cx, movement::move_next_long_word_start, false)
}

fn vim_move_prev_long_word_start(cx: &mut Context) {
    move_word_vim_impl(cx, movement::move_prev_long_word_start, false)
}

fn vim_move_next_long_word_end(cx: &mut Context) {
    move_word_vim_impl(cx, movement::move_next_long_word_end, true)
}

fn vim_move_prev_long_word_end(cx: &mut Context) {
    move_word_vim_impl(cx, movement::move_prev_long_word_end, true)
}

fn move_next_word_start(cx: &mut Context) {
    move_word_impl(cx, movement::move_next_word_start)
}

fn move_prev_word_start(cx: &mut Context) {
    move_word_impl(cx, movement::move_prev_word_start)
}

fn move_prev_word_end(cx: &mut Context) {
    move_word_impl(cx, movement::move_prev_word_end)
}

fn move_next_word_end(cx: &mut Context) {
    move_word_impl(cx, movement::move_next_word_end)
}

fn move_next_long_word_start(cx: &mut Context) {
    move_word_impl(cx, movement::move_next_long_word_start)
}

fn move_prev_long_word_start(cx: &mut Context) {
    move_word_impl(cx, movement::move_prev_long_word_start)
}

fn move_prev_long_word_end(cx: &mut Context) {
    move_word_impl(cx, movement::move_prev_long_word_end)
}

fn move_next_long_word_end(cx: &mut Context) {
    move_word_impl(cx, movement::move_next_long_word_end)
}

fn move_next_sub_word_start(cx: &mut Context) {
    move_word_impl(cx, movement::move_next_sub_word_start)
}

fn move_prev_sub_word_start(cx: &mut Context) {
    move_word_impl(cx, movement::move_prev_sub_word_start)
}

fn move_prev_sub_word_end(cx: &mut Context) {
    move_word_impl(cx, movement::move_prev_sub_word_end)
}

fn move_next_sub_word_end(cx: &mut Context) {
    move_word_impl(cx, movement::move_next_sub_word_end)
}

fn goto_para_impl<F>(cx: &mut Context, move_fn: F)
where
    F: Fn(RopeSlice, Range, usize, Movement) -> Range + 'static,
{
    let count = cx.count();
    let motion = move |editor: &mut Editor| {
        let (view, doc) = current!(editor);
        let text = doc.text().slice(..);
        let behavior = if editor.mode == Mode::Select {
            Movement::Extend
        } else {
            Movement::Move
        };

        let selection = doc
            .selection(view.id)
            .clone()
            .transform(|range| move_fn(text, range, count, behavior));
        doc.set_selection(view.id, selection);
    };
    cx.editor.apply_motion(motion)
}

fn goto_prev_paragraph(cx: &mut Context) {
    goto_para_impl(cx, movement::move_prev_paragraph)
}

fn goto_next_paragraph(cx: &mut Context) {
    goto_para_impl(cx, movement::move_next_paragraph)
}

fn move_sentence_forward(cx: &mut Context) {
    goto_para_impl(cx, movement::move_next_sentence)
}

fn move_sentence_backward(cx: &mut Context) {
    goto_para_impl(cx, movement::move_prev_sentence)
}

fn goto_file_start(cx: &mut Context) {
    goto_file_start_impl(cx, Movement::Move);
}

fn extend_to_file_start(cx: &mut Context) {
    goto_file_start_impl(cx, Movement::Extend);
}

fn goto_file_start_impl(cx: &mut Context, movement: Movement) {
    if cx.count.is_some() {
        goto_line_impl(cx, movement);
    } else {
        let (view, doc) = current!(cx.editor);
        let start = doc.view_point_min(view.id);
        let text = doc.text().slice(..);
        let selection = doc
            .selection(view.id)
            .clone()
            .transform(|range| range.put_cursor(text, start, movement == Movement::Extend));
        push_jump(view, doc);
        doc.set_selection(view.id, selection);
    }
}

fn goto_file_end(cx: &mut Context) {
    goto_file_end_impl(cx, Movement::Move);
}

fn extend_to_file_end(cx: &mut Context) {
    goto_file_end_impl(cx, Movement::Extend)
}

fn goto_file_end_impl(cx: &mut Context, movement: Movement) {
    let (view, doc) = current!(cx.editor);
    let text = doc.text().slice(..);
    // Confine to the narrowed region end when narrowed (Emacs point-max), per-view aware.
    let pos = doc.view_point_max(view.id);
    let selection = doc
        .selection(view.id)
        .clone()
        .transform(|range| range.put_cursor(text, pos, movement == Movement::Extend));
    push_jump(view, doc);
    doc.set_selection(view.id, selection);
}

fn goto_file(cx: &mut Context) {
    goto_file_impl(cx, Action::Replace);
}

fn goto_file_hsplit(cx: &mut Context) {
    goto_file_impl(cx, Action::HorizontalSplit);
}

fn goto_file_vsplit(cx: &mut Context) {
    goto_file_impl(cx, Action::VerticalSplit);
}

/// Returns true when a selection overlaps an LSP document link range.
fn selection_overlaps_document_link(
    selection: &Range,
    link: &zemacs_view::document::DocumentLink,
) -> bool {
    if selection.is_empty() {
        let pos = selection.from();
        link.start <= pos && pos < link.end
    } else {
        selection.from() < link.end && selection.to() > link.start
    }
}

/// Create a document link resolve request when the target isn't already present.
///
/// This only builds the LSP request. The request is awaited from a background
/// job so `goto_file_impl` does not block the UI thread while the language
/// server resolves the target.
fn resolve_document_link_request(
    editor: &Editor,
    link: &zemacs_view::document::DocumentLink,
) -> Option<impl Future<Output = zemacs_lsp::Result<zemacs_lsp::lsp::DocumentLink>> + Send + 'static>
{
    let language_server = editor.language_server_by_id(link.language_server_id)?;
    let supports_resolve = language_server
        .capabilities()
        .document_link_provider
        .as_ref()?
        .resolve_provider
        .unwrap_or(false);

    if !supports_resolve {
        return None;
    }

    language_server.resolve_document_link(link.link.clone())
}

/// Goto files/URLs in selection.
///
/// Prefers LSP document links when the cursor/selection overlaps a link range,
/// falling back to the built-in path/URL detection otherwise.
fn goto_file_impl(cx: &mut Context, action: Action) {
    let (view, doc) = current_ref!(cx.editor);
    let text = doc.text().clone();
    let selections = doc.selection(view.id).ranges().to_vec();
    let rel_path = doc
        .relative_path()
        .map(|path| path.parent().unwrap().to_path_buf())
        .unwrap_or_default();
    let text = text.slice(..);

    let mut lsp_targets = Vec::new();
    let mut lsp_targets_seen = HashSet::new();
    let mut unresolved_links = HashSet::new();
    let mut resolve_requests = Vec::new();
    let mut fallback_ranges = Vec::new();

    if doc.document_links.is_empty() {
        fallback_ranges.extend_from_slice(&selections);
    } else {
        for selection in &selections {
            let mut matched = false;
            for link in &doc.document_links {
                if !selection_overlaps_document_link(selection, link) {
                    continue;
                }
                matched = true;
                if let Some(target) = link.link.target.clone() {
                    if lsp_targets_seen.insert(target.clone()) {
                        lsp_targets.push(target);
                    }
                } else if unresolved_links.insert((link.start, link.end, link.language_server_id)) {
                    if let Some(request) = resolve_document_link_request(cx.editor, link) {
                        resolve_requests.push(request);
                    }
                }
            }
            if !matched {
                fallback_ranges.push(*selection);
            }
        }
    }

    for target in lsp_targets {
        open_url(cx, target, action);
    }

    if !resolve_requests.is_empty() {
        let rel_path = rel_path.clone();
        cx.jobs.callback(async move {
            let mut targets = Vec::new();
            let mut seen = HashSet::new();

            // Resolve links off the main thread, then hand the resulting URLs
            // back to the editor/compositor callback once all requests finish.
            for request in resolve_requests {
                match request.await {
                    Ok(link) => {
                        if let Some(target) = link.target {
                            if seen.insert(target.clone()) {
                                targets.push(target);
                            }
                        }
                    }
                    Err(err) => log::warn!("Failed to resolve document link: {err}"),
                }
            }

            Ok(Callback::EditorCompositor(Box::new(
                move |editor, compositor| {
                    for target in targets {
                        open_url_in_callback(editor, compositor, target, action, &rel_path);
                    }
                },
            )))
        });
    }

    if fallback_ranges.is_empty() {
        return;
    }

    let paths: Vec<_> = if fallback_ranges.len() == 1 && fallback_ranges[0].len() == 1 {
        let selection = fallback_ranges[0];
        // Cap the search at roughly 1k bytes around the cursor.
        let lookaround = 1000;
        let pos = text.char_to_byte(selection.cursor(text));
        let search_start = text
            .line_to_byte(text.byte_to_line(pos))
            .max(text.floor_char_boundary(pos.saturating_sub(lookaround)));
        let search_end = text
            .line_to_byte(text.byte_to_line(pos) + 1)
            .min(text.ceil_char_boundary(pos + lookaround));
        let search_range = text.byte_slice(search_start..search_end);
        // we also allow paths that are next to the cursor (can be ambiguous but
        // rarely so in practice) so that gf on quoted/braced path works (not sure about this
        // but apparently that is how gf has worked historically in zemacs)
        let path = find_paths(search_range, true)
            .take_while(|range| search_start + range.start <= pos + 1)
            .find(|range| pos <= search_start + range.end)
            .map(|range| Cow::from(search_range.byte_slice(range)));
        log::debug!("goto_file auto-detected path: {path:?}");
        let path = path.unwrap_or_else(|| selection.fragment(text));
        vec![path.into_owned()]
    } else {
        // Otherwise use each selection, trimmed.
        fallback_ranges
            .iter()
            .map(|range| range.fragment(text).trim().to_owned())
            .filter(|sel| !sel.is_empty())
            .collect()
    };

    for sel in paths {
        if let Ok(url) = Url::parse(&sel) {
            open_url(cx, url, action);
            continue;
        }

        let path = path::expand(&sel);
        let path = &rel_path.join(path);
        if path.is_dir() {
            let picker = ui::file_picker(cx.editor, path.into());
            cx.push_layer(Box::new(overlaid(picker)));
        } else if let Err(e) = cx.editor.open(path, action) {
            cx.editor.set_error(format!("Open file failed: {:?}", e));
        }
    }
}

/// Opens the given url. If the URL points to a valid textual file it is open in zemacs.
/// Otherwise, the file is open using external program.
fn open_url(cx: &mut Context, url: Url, action: Action) {
    let doc = doc!(cx.editor);
    let rel_path = doc
        .relative_path()
        .map(|path| path.parent().unwrap().to_path_buf())
        .unwrap_or_default();

    if should_open_url_externally(&url) {
        return cx.jobs.callback(crate::open_external_url_callback(url));
    }

    let path = &rel_path.join(url.path());
    if path.is_dir() {
        let picker = ui::file_picker(cx.editor, path.into());
        cx.push_layer(Box::new(overlaid(picker)));
    } else if let Err(e) = cx.editor.open(path, action) {
        cx.editor.set_error(format!("Open file failed: {:?}", e));
    }
}

/// Open a URL from an editor/compositor callback.
///
/// This mirrors `open_url` but does not require a full `Context`, which makes
/// it usable from async job completions such as deferred document link
/// resolves.
fn open_url_in_callback(
    editor: &mut Editor,
    compositor: &mut Compositor,
    url: Url,
    action: Action,
    rel_path: &Path,
) {
    if should_open_url_externally(&url) {
        tokio::spawn(async move {
            match crate::open_external_url_callback(url).await {
                Ok(callback) => job::dispatch_callback(callback).await,
                Err(err) => status::report(err).await,
            }
        });
        return;
    }

    let path = &rel_path.join(url.path());
    if path.is_dir() {
        let picker = ui::file_picker(editor, path.into());
        compositor.push(Box::new(overlaid(picker)));
    } else if let Err(e) = editor.open(path, action) {
        editor.set_error(format!("Open file failed: {:?}", e));
    }
}

/// Returns whether a URL should opened externally.
///
/// Non-`file` URLs always open externally. `file` URLs are opened externally
/// only when the target looks like a binary file (a non-textual file that can't
/// be viewed in zemacs).
fn should_open_url_externally(url: &Url) -> bool {
    if url.scheme() != "file" {
        return true;
    }

    let is_binary = std::fs::File::open(url.path()).and_then(|file| {
        // Read up to 1kb to detect the content type
        let mut read_buffer = Vec::new();
        let n = file.take(1024).read_to_end(&mut read_buffer)?;
        Ok(crate::is_binary(&read_buffer[..n]))
    });

    matches!(is_binary, Ok(true))
}

fn extend_word_impl<F>(cx: &mut Context, extend_fn: F)
where
    F: Fn(RopeSlice, Range, usize) -> Range,
{
    let count = cx.count();
    let (view, doc) = current!(cx.editor);
    let text = doc.text().slice(..);

    let selection = doc.selection(view.id).clone().transform(|range| {
        let word = extend_fn(text, range, count);
        let pos = word.cursor(text);
        range.put_cursor(text, pos, true)
    });
    doc.set_selection(view.id, selection);
}

fn extend_next_word_start(cx: &mut Context) {
    extend_word_impl(cx, movement::move_next_word_start)
}

fn extend_prev_word_start(cx: &mut Context) {
    extend_word_impl(cx, movement::move_prev_word_start)
}

fn extend_next_word_end(cx: &mut Context) {
    extend_word_impl(cx, movement::move_next_word_end)
}

fn extend_prev_word_end(cx: &mut Context) {
    extend_word_impl(cx, movement::move_prev_word_end)
}

fn extend_next_long_word_start(cx: &mut Context) {
    extend_word_impl(cx, movement::move_next_long_word_start)
}

fn extend_prev_long_word_start(cx: &mut Context) {
    extend_word_impl(cx, movement::move_prev_long_word_start)
}

fn extend_prev_long_word_end(cx: &mut Context) {
    extend_word_impl(cx, movement::move_prev_long_word_end)
}

fn extend_next_long_word_end(cx: &mut Context) {
    extend_word_impl(cx, movement::move_next_long_word_end)
}

fn extend_next_sub_word_start(cx: &mut Context) {
    extend_word_impl(cx, movement::move_next_sub_word_start)
}

fn extend_prev_sub_word_start(cx: &mut Context) {
    extend_word_impl(cx, movement::move_prev_sub_word_start)
}

fn extend_prev_sub_word_end(cx: &mut Context) {
    extend_word_impl(cx, movement::move_prev_sub_word_end)
}

fn extend_next_sub_word_end(cx: &mut Context) {
    extend_word_impl(cx, movement::move_next_sub_word_end)
}

/// Separate branch to find_char designed only for `<ret>` char.
//
// This is necessary because the one document can have different line endings inside. And we
// cannot predict what character to find when <ret> is pressed. On the current line it can be `lf`
// but on the next line it can be `crlf`. That's why [`find_char_impl`] cannot be applied here.
fn find_char_line_ending_motion(
    editor: &mut Editor,
    count: usize,
    direction: Direction,
    inclusive: bool,
    extend: bool,
) {
    let (view, doc) = current!(editor);
    let text = doc.text().slice(..);

    let selection = doc.selection(view.id).clone().transform(|range| {
        let cursor_anchor = range.cursor(text);
        let cursor_head = next_grapheme_boundary(text, cursor_anchor);
        let cursor_line = range.cursor_line(text);

        let pos = match direction {
            Direction::Forward => {
                let line_end = line_end_char_index(&text, cursor_line);
                let on_edge = if inclusive {
                    line_end == cursor_anchor
                } else {
                    line_end == cursor_head || line_end == cursor_anchor
                };
                let line = cursor_line + count - 1 + on_edge as usize;
                if line >= text.len_lines() - 1 {
                    return range;
                }
                line_end_char_index(&text, line) - !inclusive as usize
            }
            Direction::Backward => {
                if inclusive {
                    let line = cursor_line as isize - count as isize;
                    if line < 0 {
                        return range;
                    }
                    line_end_char_index(&text, line as usize)
                } else {
                    let on_edge = text.line_to_char(cursor_line) == cursor_anchor;
                    let line = cursor_line as isize - count as isize + 1 - on_edge as isize;
                    if line <= 0 {
                        return range;
                    }
                    text.line_to_char(line as usize)
                }
            }
        };

        if extend {
            range.put_cursor(text, pos, true)
        } else {
            Range::point(range.cursor(text)).put_cursor(text, pos, true)
        }
    });
    doc.set_selection(view.id, selection);
}

fn find_char(cx: &mut Context, direction: Direction, inclusive: bool, extend: bool) {
    find_char_then(cx, direction, inclusive, extend, None)
}

fn find_char_then(
    cx: &mut Context,
    direction: Direction,
    inclusive: bool,
    extend: bool,
    after: Option<fn(&mut Context)>,
) {
    // TODO: count is reset to 1 before next key so we move it into the closure here.
    // Would be nice to carry over.
    let count = cx.count();

    // need to wait for next key
    // TODO: should this be done by grapheme rather than char?  For example,
    // we can't properly handle the line-ending CRLF case here in terms of char.
    cx.on_next_key(move |cx, event| {
        let motion: Motion = if event.code == KeyCode::Enter {
            Box::new(move |editor: &mut Editor| {
                find_char_line_ending_motion(editor, count, direction, inclusive, extend);
            })
        } else if let Some(ch) = match event.code {
            KeyCode::Tab => Some('\t'),
            KeyCode::Char(ch) => Some(ch),
            _ => None,
        } {
            // remember this find so `,` can repeat it in the opposite direction
            cx.editor.last_find = Some((ch, inclusive, matches!(direction, Direction::Forward)));
            Box::new(move |editor: &mut Editor| {
                let (view, doc) = current!(editor);
                let text = doc.text().slice(..);

                let selection = doc.selection(view.id).clone().transform(|range| {
                    let cursor_anchor = range.cursor(text);
                    let cursor_head = next_grapheme_boundary(text, cursor_anchor);

                    // Exclusive search skips the next char after cursor to enable repeated application
                    let search_start_pos = match (inclusive, direction) {
                        (true, Direction::Forward) => cursor_head,
                        (true, Direction::Backward) => cursor_anchor,
                        (false, Direction::Forward) => cursor_head + 1,
                        (false, Direction::Backward) => cursor_anchor.saturating_sub(1),
                    };

                    search::find_nth_char(count, text, ch, search_start_pos, direction)
                        // Exclusive search should stop on previous character
                        .map(|pos| match (inclusive, direction) {
                            (true, Direction::Forward) => pos,
                            (true, Direction::Backward) => pos,
                            (false, Direction::Forward) => pos - 1,
                            (false, Direction::Backward) => pos + 1,
                        })
                        .map_or(range, |pos| {
                            if extend {
                                range.put_cursor(text, pos, true)
                            } else {
                                Range::point(range.cursor(text)).put_cursor(text, pos, true)
                            }
                        })
                });

                doc.set_selection(view.id, selection);
            })
        } else {
            return;
        };

        cx.editor.apply_motion(motion);
        // Apply a pending operator (vim `d`/`c`/`y` + `f`/`t`/`F`/`T`).
        if let Some(after) = after {
            after(cx);
        }
    })
}

// vim named marks: `m{a-z}` sets a mark at the cursor; `` `{a-z} `` jumps to the
// exact mark position; `'{a-z}` jumps to the mark's line (first non-blank).
fn set_mark(cx: &mut Context) {
    cx.on_next_key(move |cx, event| {
        cx.editor.autoinfo = None;
        if let Some(ch) = event.char() {
            let (view, doc) = current!(cx.editor);
            let pos = doc
                .selection(view.id)
                .primary()
                .cursor(doc.text().slice(..));
            doc.set_mark(ch, pos);
        }
    });
    cx.editor.autoinfo = Some(Info::new("Set mark", &[("a-z", "mark name")]));
}

fn goto_mark_impl(cx: &mut Context, to_line_start: bool) {
    cx.on_next_key(move |cx, event| {
        cx.editor.autoinfo = None;
        if let Some(ch) = event.char() {
            let (view, doc) = current!(cx.editor);
            // Most marks are looked up directly; vim's structural marks that aren't
            // stored (paragraph `{`/`}`, sentence `(`/`)` — approximated by paragraph)
            // are computed from the current cursor on the fly.
            let computed = doc.mark(ch).or_else(|| {
                let text = doc.text().slice(..);
                let range = doc.selection(view.id).primary();
                match ch {
                    '}' | ')' => Some(
                        zemacs_core::movement::move_next_paragraph(text, range, 1, Movement::Move)
                            .cursor(text),
                    ),
                    '{' | '(' => Some(
                        zemacs_core::movement::move_prev_paragraph(text, range, 1, Movement::Move)
                            .cursor(text),
                    ),
                    _ => None,
                }
            });
            let Some(mut pos) = computed else {
                cx.editor.set_error(format!("Mark '{ch}' not set"));
                return;
            };
            let text = doc.text().slice(..);
            if to_line_start {
                let line = text.char_to_line(pos);
                pos = text
                    .line(line)
                    .first_non_whitespace_char()
                    .map(|p| p + text.line_to_char(line))
                    .unwrap_or_else(|| text.line_to_char(line));
            }
            push_jump(view, doc);
            doc.set_selection(view.id, Selection::point(pos));
        }
    });
    cx.editor.autoinfo = Some(Info::new("Goto mark", &[("a-z", "mark name")]));
}

// vim `gi`: re-enter insert mode at the position where insert mode was last
// left (the `^` mark). The mark is edit-tracked like any other.
fn mark_insert_exit(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);
    let pos = doc
        .selection(view.id)
        .primary()
        .cursor(doc.text().slice(..));
    doc.set_mark('^', pos);
}

fn insert_at_last_insert(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);
    if let Some(pos) = doc.mark('^') {
        doc.set_selection(view.id, Selection::point(pos));
    }
    insert_mode(cx);
}

// vim `gv`: reselect the last visual (select-mode) area. The selection is saved
// when leaving select mode and restored (clamped to the current text) here.
fn save_visual_selection(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);
    let selection = doc.selection(view.id).clone();
    // record the `<` / `>` marks (start / end of the last visual area) so
    // `` `< `` / `` `> `` (and the `'<` / `'>` line variants) jump there.
    let prim = selection.primary();
    let from = prim.from();
    let to = prim.to().saturating_sub(1).max(from);
    doc.set_last_visual(selection);
    doc.set_mark('<', from);
    doc.set_mark('>', to);
}

fn reselect_visual(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);
    if let Some(selection) = doc.last_visual().cloned() {
        let selection = selection.ensure_invariants(doc.text().slice(..));
        doc.set_selection(view.id, selection);
    }
    cx.editor.mode = Mode::Select;
}

// vim macros: `q{reg}` starts recording into a register (and `q` again stops);
// `@{reg}` replays it. The register char is captured interactively so the vim
// `qa` / `@a` syntax works (zemacs natively selects the register with `"`).
fn vim_record_macro(cx: &mut Context) {
    if cx.editor.macro_recording.is_some() {
        record_macro(cx); // stop recording
        return;
    }
    cx.editor.autoinfo = Some(Info::new("Record macro", &[("a-z0-9\"", "register")]));
    cx.on_next_key(move |cx, event| {
        cx.editor.autoinfo = None;
        if let Some(ch) = event.char() {
            cx.register = Some(ch);
            record_macro(cx);
        }
    });
}

fn vim_replay_macro(cx: &mut Context) {
    cx.editor.autoinfo = Some(Info::new("Replay macro", &[("a-z0-9@\":", "register")]));
    cx.on_next_key(move |cx, event| {
        cx.editor.autoinfo = None;
        if let Some(ch) = event.char() {
            // vim `@:` re-runs the last command-line rather than replaying a register.
            if ch == ':' {
                typed::repeat_last_command_line(cx);
                return;
            }
            cx.register = Some(ch);
            replay_macro(cx);
        }
    });
}

/// vim `&` / `g&`: repeat the last `:substitute` on the current line / whole file.
fn repeat_substitute_impl(cx: &mut Context, whole_file: bool) {
    let Some((pattern, replacement, flags)) = cx.editor.last_substitute.clone() else {
        cx.editor.set_error("No previous substitute");
        return;
    };
    if let Err(err) = do_substitute(cx.editor, whole_file, &pattern, &replacement, &flags) {
        cx.editor.set_error(err.to_string());
    }
}

fn repeat_substitute(cx: &mut Context) {
    repeat_substitute_impl(cx, false);
}

fn repeat_substitute_global(cx: &mut Context) {
    repeat_substitute_impl(cx, true);
}

fn goto_mark(cx: &mut Context) {
    goto_mark_impl(cx, false);
}

fn goto_mark_line(cx: &mut Context) {
    goto_mark_impl(cx, true);
}

// vim operator + find-char: extend to the target char (inclusive `f`/`F`,
// exclusive `t`/`T`), then apply the operator. Makes `df,`, `dt)`, `ct"`, … work.
fn delete_find_char_forward(cx: &mut Context) {
    find_char_then(cx, Direction::Forward, true, true, Some(delete_selection));
}
fn delete_till_char_forward(cx: &mut Context) {
    find_char_then(cx, Direction::Forward, false, true, Some(delete_selection));
}
fn delete_find_char_backward(cx: &mut Context) {
    find_char_then(cx, Direction::Backward, true, true, Some(delete_selection));
}
fn delete_till_char_backward(cx: &mut Context) {
    find_char_then(cx, Direction::Backward, false, true, Some(delete_selection));
}
fn change_find_char_forward(cx: &mut Context) {
    find_char_then(cx, Direction::Forward, true, true, Some(change_selection));
}
fn change_till_char_forward(cx: &mut Context) {
    find_char_then(cx, Direction::Forward, false, true, Some(change_selection));
}
fn change_find_char_backward(cx: &mut Context) {
    find_char_then(cx, Direction::Backward, true, true, Some(change_selection));
}
fn change_till_char_backward(cx: &mut Context) {
    find_char_then(cx, Direction::Backward, false, true, Some(change_selection));
}
fn yank_find_char_forward(cx: &mut Context) {
    find_char_then(cx, Direction::Forward, true, true, Some(yank_textobject));
}
fn yank_till_char_forward(cx: &mut Context) {
    find_char_then(cx, Direction::Forward, false, true, Some(yank_textobject));
}
fn yank_find_char_backward(cx: &mut Context) {
    find_char_then(cx, Direction::Backward, true, true, Some(yank_textobject));
}
fn yank_till_char_backward(cx: &mut Context) {
    find_char_then(cx, Direction::Backward, false, true, Some(yank_textobject));
}

fn find_till_char(cx: &mut Context) {
    find_char(cx, Direction::Forward, false, false);
}

fn find_next_char(cx: &mut Context) {
    find_char(cx, Direction::Forward, true, false)
}

fn extend_till_char(cx: &mut Context) {
    find_char(cx, Direction::Forward, false, true)
}

fn extend_next_char(cx: &mut Context) {
    find_char(cx, Direction::Forward, true, true)
}

fn till_prev_char(cx: &mut Context) {
    find_char(cx, Direction::Backward, false, false)
}

fn find_prev_char(cx: &mut Context) {
    find_char(cx, Direction::Backward, true, false)
}

fn extend_till_prev_char(cx: &mut Context) {
    find_char(cx, Direction::Backward, false, true)
}

fn sneak_char(event: KeyEvent) -> Option<char> {
    match event.code {
        KeyCode::Char(ch) => Some(ch),
        KeyCode::Enter => Some('\n'),
        KeyCode::Tab => Some('\t'),
        _ => None,
    }
}

/// vim-sneak: read two characters, then jump the cursor to the next/previous
/// occurrence of that pair.
fn sneak(cx: &mut Context, direction: Direction) {
    cx.on_next_key(move |cx, ev1| {
        let Some(c1) = sneak_char(ev1) else { return };
        cx.on_next_key(move |cx, ev2| {
            let Some(c2) = sneak_char(ev2) else { return };
            sneak_jump(cx, c1, c2, direction);
        });
    });
}

fn sneak_jump(cx: &mut Context, c1: char, c2: char, direction: Direction) {
    let target = {
        let (view, doc) = current!(cx.editor);
        let text = doc.text().slice(..);
        let cursor = doc.selection(view.id).primary().cursor(text);
        match direction {
            Direction::Forward => {
                let start = (cursor + 1).min(text.len_chars());
                let mut prev: Option<char> = None;
                let mut found = None;
                for (pos, c) in (start..).zip(text.chars_at(start)) {
                    if prev == Some(c1) && c == c2 {
                        found = Some(pos - 1);
                        break;
                    }
                    prev = Some(c);
                }
                found
            }
            Direction::Backward => {
                let mut prev: Option<char> = None;
                let mut last = None;
                for (pos, c) in text.chars().enumerate() {
                    if pos > cursor {
                        break;
                    }
                    if prev == Some(c1) && c == c2 && pos >= 1 && (pos - 1) < cursor {
                        last = Some(pos - 1);
                    }
                    prev = Some(c);
                }
                last
            }
        }
    };

    match target {
        Some(idx) => {
            let scrolloff = cx.editor.config().scrolloff;
            let (view, doc) = current!(cx.editor);
            doc.set_selection(view.id, Selection::point(idx));
            view.ensure_cursor_in_view(doc, scrolloff);
        }
        None => cx.editor.set_status(format!("sneak: '{c1}{c2}' not found")),
    }
}

fn sneak_forward(cx: &mut Context) {
    sneak(cx, Direction::Forward)
}

fn sneak_backward(cx: &mut Context) {
    sneak(cx, Direction::Backward)
}

/// `s`: vim-sneak forward when `editor.vim-sneak` is on, else vim substitute-char.
fn sneak_or_substitute_char(cx: &mut Context) {
    if cx.editor.config().vim_sneak {
        sneak_forward(cx);
    } else {
        change_selection(cx);
    }
}

/// `S`: vim-sneak backward when `editor.vim-sneak` is on, else vim substitute-line.
fn sneak_or_substitute_line(cx: &mut Context) {
    if cx.editor.config().vim_sneak {
        sneak_backward(cx);
    } else {
        extend_to_line_bounds(cx);
        change_selection(cx);
    }
}

fn extend_prev_char(cx: &mut Context) {
    find_char(cx, Direction::Backward, true, true)
}

fn repeat_last_motion(cx: &mut Context) {
    cx.editor.repeat_last_motion(cx.count())
}

// Run a find-char selection with explicit params (no interactive key wait).
// Shared by `,` reverse find-repeat below.
fn apply_find_char(
    editor: &mut Editor,
    ch: char,
    inclusive: bool,
    direction: Direction,
    extend: bool,
    count: usize,
) {
    let (view, doc) = current!(editor);
    let text = doc.text().slice(..);
    let selection = doc.selection(view.id).clone().transform(|range| {
        let cursor_anchor = range.cursor(text);
        let cursor_head = next_grapheme_boundary(text, cursor_anchor);
        let search_start_pos = match (inclusive, direction) {
            (true, Direction::Forward) => cursor_head,
            (true, Direction::Backward) => cursor_anchor,
            (false, Direction::Forward) => cursor_head + 1,
            (false, Direction::Backward) => cursor_anchor.saturating_sub(1),
        };
        search::find_nth_char(count, text, ch, search_start_pos, direction)
            .map(|pos| match (inclusive, direction) {
                (true, _) => pos,
                (false, Direction::Forward) => pos - 1,
                (false, Direction::Backward) => pos + 1,
            })
            .map_or(range, |pos| {
                // `,` is a standalone motion: extend in visual, else a clean
                // 1-wide cursor at the target (don't drag a selection across).
                range.put_cursor(text, pos, extend)
            })
    });
    doc.set_selection(view.id, selection);
}

// vim `,`: repeat the last f/t/F/T find in the OPPOSITE direction.
fn repeat_find_char_reverse(cx: &mut Context) {
    let count = cx.count();
    let Some((ch, inclusive, forward)) = cx.editor.last_find else {
        return;
    };
    // reverse the original direction
    let direction = if forward {
        Direction::Backward
    } else {
        Direction::Forward
    };
    let extend = cx.editor.mode == Mode::Select;
    apply_find_char(cx.editor, ch, inclusive, direction, extend, count);
}

fn replace(cx: &mut Context) {
    let mut buf = [0u8; 4]; // To hold utf8 encoded char.

    // need to wait for next key
    cx.on_next_key(move |cx, event| {
        let (view, doc) = current!(cx.editor);
        let ch: Option<&str> = match event {
            KeyEvent {
                code: KeyCode::Char(ch),
                ..
            } => Some(ch.encode_utf8(&mut buf[..])),
            KeyEvent {
                code: KeyCode::Enter,
                ..
            } => Some(doc.line_ending.as_str()),
            KeyEvent {
                code: KeyCode::Tab, ..
            } => Some("\t"),
            _ => None,
        };

        let selection = doc.selection(view.id);

        if let Some(ch) = ch {
            let transaction = Transaction::change_by_selection(doc.text(), selection, |range| {
                if !range.is_empty() {
                    let text: Tendril = doc
                        .text()
                        .slice(range.from()..range.to())
                        .graphemes()
                        .map(|_g| ch)
                        .collect();
                    (range.from(), range.to(), Some(text))
                } else {
                    // No change.
                    (range.from(), range.to(), None)
                }
            });

            doc.apply(&transaction, view.id);
            exit_select_mode(cx);
        }
    })
}

fn switch_case_impl<F>(cx: &mut Context, change_fn: F)
where
    F: Fn(RopeSlice) -> Tendril,
{
    let (view, doc) = current!(cx.editor);
    let selection = doc.selection(view.id);
    let transaction = Transaction::change_by_selection(doc.text(), selection, |range| {
        let text: Tendril = change_fn(range.slice(doc.text().slice(..)));

        (range.from(), range.to(), Some(text))
    });

    doc.apply(&transaction, view.id);
    exit_select_mode(cx);
}

enum CaseSwitcher {
    Upper(ToUppercase),
    Lower(ToLowercase),
    Keep(Option<char>),
}

impl Iterator for CaseSwitcher {
    type Item = char;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            CaseSwitcher::Upper(upper) => upper.next(),
            CaseSwitcher::Lower(lower) => lower.next(),
            CaseSwitcher::Keep(ch) => ch.take(),
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        match self {
            CaseSwitcher::Upper(upper) => upper.size_hint(),
            CaseSwitcher::Lower(lower) => lower.size_hint(),
            CaseSwitcher::Keep(ch) => {
                let n = if ch.is_some() { 1 } else { 0 };
                (n, Some(n))
            }
        }
    }
}

impl ExactSizeIterator for CaseSwitcher {}

fn switch_case(cx: &mut Context) {
    switch_case_impl(cx, |string| {
        string
            .chars()
            .flat_map(|ch| {
                if ch.is_lowercase() {
                    CaseSwitcher::Upper(ch.to_uppercase())
                } else if ch.is_uppercase() {
                    CaseSwitcher::Lower(ch.to_lowercase())
                } else {
                    CaseSwitcher::Keep(Some(ch))
                }
            })
            .collect()
    });
}

fn switch_to_uppercase(cx: &mut Context) {
    switch_case_impl(cx, |string| {
        string.chunks().map(|chunk| chunk.to_uppercase()).collect()
    });
}

fn switch_to_lowercase(cx: &mut Context) {
    switch_case_impl(cx, |string| {
        string.chunks().map(|chunk| chunk.to_lowercase()).collect()
    });
}

// vim `g?` ROT13: rotate every ASCII letter 13 places, leave everything else
// untouched. The operator/line variants are wired in the keymap; this performs
// the transform over the current selection.
fn rot13(cx: &mut Context) {
    switch_case_impl(cx, |string| {
        string
            .chars()
            .map(|ch| match ch {
                'a'..='z' => (((ch as u8 - b'a' + 13) % 26) + b'a') as char,
                'A'..='Z' => (((ch as u8 - b'A' + 13) % 26) + b'A') as char,
                _ => ch,
            })
            .collect()
    });
}

/// Percent-encode per RFC 3986: keep the unreserved set (A-Z a-z 0-9 - _ . ~),
/// encode every other UTF-8 byte as `%XX` (uppercase hex).
fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Decode percent-encoded text: `%XX` → byte, invalid escapes left verbatim.
/// `+` is preserved literally (path-style, not form-style). Output is lossy UTF-8.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = (bytes[i + 1] as char).to_digit(16);
            let lo = (bytes[i + 2] as char).to_digit(16);
            if let (Some(h), Some(l)) = (hi, lo) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn url_encode(cx: &mut Context) {
    switch_case_impl(cx, |slice| {
        let s: String = slice.chunks().collect();
        percent_encode(&s).into()
    });
}

fn url_decode(cx: &mut Context) {
    switch_case_impl(cx, |slice| {
        let s: String = slice.chunks().collect();
        percent_decode(&s).into()
    });
}

/// Expand a URL query string into one decoded `key=value` per line. Handles a
/// leading `?`, `+` as space, and percent-decoding of keys and values. Pure —
/// unit tested.
fn parse_query_string(s: &str) -> String {
    let decode = |part: &str| percent_decode(&part.replace('+', " "));
    s.trim()
        .trim_start_matches('?')
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|pair| match pair.split_once('=') {
            Some((k, v)) => format!("{}={}", decode(k), decode(v)),
            None => decode(pair),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn parse_query_selection(cx: &mut Context) {
    switch_case_impl(cx, |slice| {
        let s: String = slice.chunks().collect();
        parse_query_string(&s).into()
    });
}

/// Build a URL query string from `key=value` lines, percent-encoding each key and
/// value and joining with `&`. The inverse of [`parse_query_string`]. Pure — unit tested.
fn build_query_string(s: &str) -> String {
    s.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|line| match line.split_once('=') {
            Some((k, v)) => format!("{}={}", percent_encode(k.trim()), percent_encode(v.trim())),
            None => percent_encode(line.trim()),
        })
        .collect::<Vec<_>>()
        .join("&")
}

fn build_query_selection(cx: &mut Context) {
    switch_case_impl(cx, |slice| {
        let s: String = slice.chunks().collect();
        build_query_string(&s).into()
    });
}

/// Break a URL into labeled `scheme`/`host`/`port`/`path`/`query` lines, with the
/// query expanded to decoded `key=value` pairs (via [`parse_query_string`]).
/// Pure — unit tested.
fn url_info(s: &str) -> String {
    let s = s.trim();
    let mut out = String::new();
    let rest = match s.find("://") {
        Some(i) => {
            out.push_str(&format!("scheme: {}\n", &s[..i]));
            &s[i + 3..]
        }
        None => s,
    };
    let (before_query, query) = match rest.split_once('?') {
        Some((b, q)) => (b, Some(q)),
        None => (rest, None),
    };
    let (authority, path) = match before_query.find('/') {
        Some(i) => (&before_query[..i], &before_query[i..]),
        None => (before_query, ""),
    };
    let authority = authority.rsplit('@').next().unwrap_or(authority); // drop userinfo
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) if !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()) => (h, Some(p)),
        _ => (authority, None),
    };
    if !host.is_empty() {
        out.push_str(&format!("host: {host}\n"));
    }
    if let Some(p) = port {
        out.push_str(&format!("port: {p}\n"));
    }
    if !path.is_empty() {
        out.push_str(&format!("path: {path}\n"));
    }
    if let Some(q) = query {
        out.push_str("query:\n");
        for line in parse_query_string(q).lines() {
            out.push_str(&format!("  {line}\n"));
        }
    }
    out.trim_end().to_string()
}

fn url_info_selection(cx: &mut Context) {
    switch_case_impl(cx, |slice| {
        let s: String = slice.chunks().collect();
        url_info(&s).into()
    });
}

const BASE64_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

const BASE64URL_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// The 6-bit value of a base64 character (standard `+/` *and* URL-safe `-_`), or
/// `None` for padding, whitespace, or any other byte (so decode skips them
/// leniently — and handles both standard and URL-safe input, e.g. JWT segments).
fn base64_val(c: u8) -> Option<u32> {
    match c {
        b'A'..=b'Z' => Some((c - b'A') as u32),
        b'a'..=b'z' => Some((c - b'a' + 26) as u32),
        b'0'..=b'9' => Some((c - b'0' + 52) as u32),
        b'+' | b'-' => Some(62),
        b'/' | b'_' => Some(63),
        _ => None,
    }
}

/// URL-safe base64-encode (`-_` alphabet, no `=` padding) — the encoding used by
/// JWTs and URLs.
fn base64url_encode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b1 = chunk.get(1).copied();
        let b2 = chunk.get(2).copied();
        let n =
            ((chunk[0] as u32) << 16) | ((b1.unwrap_or(0) as u32) << 8) | (b2.unwrap_or(0) as u32);
        out.push(BASE64URL_ALPHABET[((n >> 18) & 63) as usize] as char);
        out.push(BASE64URL_ALPHABET[((n >> 12) & 63) as usize] as char);
        if b1.is_some() {
            out.push(BASE64URL_ALPHABET[((n >> 6) & 63) as usize] as char);
        }
        if b2.is_some() {
            out.push(BASE64URL_ALPHABET[(n & 63) as usize] as char);
        }
    }
    out
}

/// Standard base64-encode a string's UTF-8 bytes (with `=` padding).
fn base64_encode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b1 = chunk.get(1).copied();
        let b2 = chunk.get(2).copied();
        let n =
            ((chunk[0] as u32) << 16) | ((b1.unwrap_or(0) as u32) << 8) | (b2.unwrap_or(0) as u32);
        out.push(BASE64_ALPHABET[((n >> 18) & 63) as usize] as char);
        out.push(BASE64_ALPHABET[((n >> 12) & 63) as usize] as char);
        out.push(if b1.is_some() {
            BASE64_ALPHABET[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if b2.is_some() {
            BASE64_ALPHABET[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

/// Base64-decode, leniently skipping padding/whitespace/invalid bytes. Output is
/// lossy UTF-8 (base64 may carry arbitrary bytes).
fn base64_decode(input: &str) -> String {
    let vals: Vec<u32> = input.bytes().filter_map(base64_val).collect();
    let mut out: Vec<u8> = Vec::with_capacity(vals.len() / 4 * 3);
    for chunk in vals.chunks(4) {
        let k = chunk.len();
        if k == 1 {
            break; // a lone 6-bit group carries no full byte
        }
        let mut n = 0u32;
        for i in 0..4 {
            n = (n << 6) | chunk.get(i).copied().unwrap_or(0);
        }
        out.push((n >> 16) as u8);
        if k >= 3 {
            out.push((n >> 8) as u8);
        }
        if k >= 4 {
            out.push(n as u8);
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn encode_base64(cx: &mut Context) {
    switch_case_impl(cx, |slice| {
        let s: String = slice.chunks().collect();
        base64_encode(&s).into()
    });
}

fn decode_base64(cx: &mut Context) {
    switch_case_impl(cx, |slice| {
        let s: String = slice.chunks().collect();
        base64_decode(&s).into()
    });
}

fn encode_base64url(cx: &mut Context) {
    switch_case_impl(cx, |slice| {
        let s: String = slice.chunks().collect();
        base64url_encode(&s).into()
    });
}

fn decode_base64url(cx: &mut Context) {
    // base64_decode already accepts both the standard and URL-safe alphabets.
    switch_case_impl(cx, |slice| {
        let s: String = slice.chunks().collect();
        base64_decode(&s).into()
    });
}

/// Decode a JWT (`header.payload.signature`) into its pretty-printed header and
/// payload JSON. Returns the input unchanged if it isn't JWT-shaped. Composes
/// base64url-decode (via [`base64_decode`]) with [`pretty_json`]. Pure — unit tested.
fn jwt_decode(token: &str) -> String {
    let parts: Vec<&str> = token.trim().split('.').collect();
    if parts.len() < 2 {
        return token.to_string();
    }
    format!(
        "// header\n{}\n\n// payload\n{}",
        pretty_json(&base64_decode(parts[0]), "  "),
        pretty_json(&base64_decode(parts[1]), "  ")
    )
}

fn jwt_decode_selection(cx: &mut Context) {
    switch_case_impl(cx, |slice| {
        let s: String = slice.chunks().collect();
        jwt_decode(&s).into()
    });
}

/// HTML-escape the five significant characters (`& < > " '`).
fn html_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// Decode a single HTML entity body (the text between `&` and `;`): named
/// entities plus decimal (`#39`) and hex (`#x27`) numeric references.
fn decode_html_entity(entity: &str) -> Option<char> {
    match entity {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        "nbsp" => Some('\u{a0}'),
        _ => {
            if let Some(hex) = entity
                .strip_prefix("#x")
                .or_else(|| entity.strip_prefix("#X"))
            {
                u32::from_str_radix(hex, 16).ok().and_then(char::from_u32)
            } else if let Some(dec) = entity.strip_prefix('#') {
                dec.parse::<u32>().ok().and_then(char::from_u32)
            } else {
                None
            }
        }
    }
}

/// Decode HTML entities, leaving unrecognized or malformed `&…;` runs verbatim.
fn html_decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after = &rest[amp..];
        if let Some(semi_rel) = after[1..].find(';') {
            let entity = &after[1..1 + semi_rel];
            let plausible = semi_rel <= 12
                && !entity.contains(|c: char| c.is_whitespace() || c == '&' || c == '<');
            if plausible {
                if let Some(c) = decode_html_entity(entity) {
                    out.push(c);
                    rest = &after[1 + semi_rel + 1..];
                    continue;
                }
            }
        }
        out.push('&');
        rest = &after[1..];
    }
    out.push_str(rest);
    out
}

fn encode_html(cx: &mut Context) {
    switch_case_impl(cx, |slice| {
        let s: String = slice.chunks().collect();
        html_encode(&s).into()
    });
}

fn decode_html(cx: &mut Context) {
    switch_case_impl(cx, |slice| {
        let s: String = slice.chunks().collect();
        html_decode(&s).into()
    });
}

/// Strip HTML tags (`<…>`) and decode entities to plain text. Pure — unit tested.
fn strip_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    html_decode(&out)
}

fn html_to_text_selection(cx: &mut Context) {
    switch_case_impl(cx, |slice| {
        let s: String = slice.chunks().collect();
        strip_html(&s).into()
    });
}

/// Convert text to a URL/file slug: lowercase ASCII alphanumerics kept, every
/// other run collapsed to a single `-`, with no leading/trailing hyphen.
fn slugify(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut pending_sep = false;
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            if pending_sep && !out.is_empty() {
                out.push('-');
            }
            out.push(c.to_ascii_lowercase());
            pending_sep = false;
        } else {
            pending_sep = true;
        }
    }
    out
}

/// Title-case text: the first letter of each word uppercased, the rest
/// lowercased. Apostrophes stay inside a word (so `don't` → `Don't`).
fn title_case(s: &str) -> String {
    let is_word = |c: char| c.is_alphanumeric() || c == '\'';
    let mut out = String::with_capacity(s.len());
    let mut at_word_start = true;
    for c in s.chars() {
        if is_word(c) {
            if at_word_start {
                out.extend(c.to_uppercase());
            } else {
                out.extend(c.to_lowercase());
            }
            at_word_start = false;
        } else {
            out.push(c);
            at_word_start = true;
        }
    }
    out
}

/// Parse a `#rgb` or `#rrggbb` hex color into `(r, g, b)` bytes. Pure.
fn parse_hex_rgb(s: &str) -> Option<(u8, u8, u8)> {
    let h = s.trim().trim_start_matches('#');
    match h.len() {
        3 => Some((
            u8::from_str_radix(&h[0..1].repeat(2), 16).ok()?,
            u8::from_str_radix(&h[1..2].repeat(2), 16).ok()?,
            u8::from_str_radix(&h[2..3].repeat(2), 16).ok()?,
        )),
        6 => Some((
            u8::from_str_radix(&h[0..2], 16).ok()?,
            u8::from_str_radix(&h[2..4], 16).ok()?,
            u8::from_str_radix(&h[4..6], 16).ok()?,
        )),
        _ => None,
    }
}

/// Convert a `#rgb` or `#rrggbb` hex color to `rgb(r, g, b)`. Returns `None` if it
/// isn't a valid hex color. Pure — unit tested.
fn hex_to_rgb(s: &str) -> Option<String> {
    let (r, g, b) = parse_hex_rgb(s)?;
    Some(format!("rgb({r}, {g}, {b})"))
}

/// Adjust a hex color toward white (`lighten`) or black by `pct` percent.
/// Returns `None` for non-hex input. Pure — unit tested.
fn adjust_lightness(hex: &str, pct: u32, lighten: bool) -> Option<String> {
    let (r, g, b) = parse_hex_rgb(hex)?;
    let f = pct as f64 / 100.0;
    let adj = |c: u8| -> u8 {
        let c = c as f64;
        let new = if lighten {
            c + (255.0 - c) * f
        } else {
            c * (1.0 - f)
        };
        new.round().clamp(0.0, 255.0) as u8
    };
    Some(format!("#{:02x}{:02x}{:02x}", adj(r), adj(g), adj(b)))
}

/// Convert `rgb(r, g, b)` (or `r, g, b` / `r g b`) to `#rrggbb`. Returns `None`
/// if it doesn't contain three 0–255 components. Pure — unit tested.
fn rgb_to_hex(s: &str) -> Option<String> {
    let nums: Vec<u8> = s
        .trim()
        .trim_start_matches("rgb")
        .split([',', ' ', '(', ')'])
        .filter_map(|t| {
            let t = t.trim();
            if t.is_empty() {
                None
            } else {
                t.parse::<u8>().ok()
            }
        })
        .collect();
    if nums.len() < 3 {
        return None;
    }
    Some(format!("#{:02x}{:02x}{:02x}", nums[0], nums[1], nums[2]))
}

fn hex_to_rgb_selection(cx: &mut Context) {
    switch_case_impl(cx, |slice| {
        let s: String = slice.chunks().collect();
        hex_to_rgb(&s).unwrap_or(s).into()
    });
}

fn rgb_to_hex_selection(cx: &mut Context) {
    switch_case_impl(cx, |slice| {
        let s: String = slice.chunks().collect();
        rgb_to_hex(&s).unwrap_or(s).into()
    });
}

/// Convert an integer (1–3999) to a Roman numeral, or `None` if out of range.
/// Pure — unit tested.
fn to_roman(mut n: u32) -> Option<String> {
    if n == 0 || n > 3999 {
        return None;
    }
    const TABLE: [(u32, &str); 13] = [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    let mut out = String::new();
    for (v, sym) in TABLE {
        while n >= v {
            out.push_str(sym);
            n -= v;
        }
    }
    Some(out)
}

/// Parse a Roman numeral into an integer, or `None` if malformed. Pure — unit tested.
fn from_roman(s: &str) -> Option<u32> {
    let val = |c: char| match c {
        'I' => 1,
        'V' => 5,
        'X' => 10,
        'L' => 50,
        'C' => 100,
        'D' => 500,
        'M' => 1000,
        _ => 0,
    };
    let digits: Vec<i64> = s.trim().to_uppercase().chars().map(val).collect();
    if digits.is_empty() || digits.contains(&0) {
        return None;
    }
    let mut total = 0i64;
    for i in 0..digits.len() {
        if i + 1 < digits.len() && digits[i] < digits[i + 1] {
            total -= digits[i];
        } else {
            total += digits[i];
        }
    }
    if (1..=3999).contains(&total) {
        Some(total as u32)
    } else {
        None
    }
}

fn to_roman_selection(cx: &mut Context) {
    switch_case_impl(cx, |slice| {
        let s: String = slice.chunks().collect();
        s.trim()
            .parse::<u32>()
            .ok()
            .and_then(to_roman)
            .unwrap_or_else(|| s.clone())
            .into()
    });
}

fn from_roman_selection(cx: &mut Context) {
    switch_case_impl(cx, |slice| {
        let s: String = slice.chunks().collect();
        from_roman(&s)
            .map(|n| n.to_string())
            .unwrap_or_else(|| s.clone())
            .into()
    });
}

/// Group a run of digits with commas every three from the right (`1234567` →
/// `1,234,567`).
fn group_thousands(digits: &str) -> String {
    let bytes = digits.as_bytes();
    let len = bytes.len();
    let mut out = String::with_capacity(len + len / 3);
    for (idx, &b) in bytes.iter().enumerate() {
        if idx > 0 && (len - idx).is_multiple_of(3) {
            out.push(',');
        }
        out.push(b as char);
    }
    out
}

/// Add thousands separators to every integer run in `s`. Pure — unit tested.
fn add_thousands(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_digit() {
            let start = i;
            while i < chars.len() && chars[i].is_ascii_digit() {
                i += 1;
            }
            let digits: String = chars[start..i].iter().collect();
            out.push_str(&group_thousands(&digits));
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

/// Remove commas that sit between two digits (thousands separators). Pure — unit tested.
fn strip_thousands(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    for (i, &c) in chars.iter().enumerate() {
        if c == ','
            && i > 0
            && i + 1 < chars.len()
            && chars[i - 1].is_ascii_digit()
            && chars[i + 1].is_ascii_digit()
        {
            continue;
        }
        out.push(c);
    }
    out
}

fn add_commas_selection(cx: &mut Context) {
    switch_case_impl(cx, |slice| {
        let s: String = slice.chunks().collect();
        add_thousands(&s).into()
    });
}

fn strip_commas_selection(cx: &mut Context) {
    switch_case_impl(cx, |slice| {
        let s: String = slice.chunks().collect();
        strip_thousands(&s).into()
    });
}

/// Swap single and double quote characters (`'` ↔ `"`) — switch a string's quote
/// style. Pure — unit tested.
fn swap_quotes(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '\'' => '"',
            '"' => '\'',
            c => c,
        })
        .collect()
}

fn swap_quotes_selection(cx: &mut Context) {
    switch_case_impl(cx, |slice| {
        let s: String = slice.chunks().collect();
        swap_quotes(&s).into()
    });
}

/// Remove a matching pair of surrounding quotes (`"`, `'`, or `` ` ``) from the
/// trimmed text, returning the inner content. Unchanged if not quoted. Pure —
/// unit tested.
fn strip_quotes(s: &str) -> String {
    let t = s.trim();
    let mut cs = t.chars();
    if let (Some(first), Some(last)) = (cs.next(), t.chars().last()) {
        if t.chars().count() >= 2 && first == last && matches!(first, '"' | '\'' | '`') {
            return t[first.len_utf8()..t.len() - last.len_utf8()].to_string();
        }
    }
    s.to_string()
}

fn strip_quotes_selection(cx: &mut Context) {
    switch_case_impl(cx, |slice| {
        let s: String = slice.chunks().collect();
        strip_quotes(&s).into()
    });
}

/// Reverse the word order within each line, preserving leading indentation and
/// collapsing internal whitespace to single spaces. Pure — unit tested.
fn reverse_words(block: &str) -> String {
    map_lines(block, |l| {
        let indent: String = l.chars().take_while(|c| c.is_whitespace()).collect();
        let words: Vec<&str> = l.split_whitespace().collect();
        format!(
            "{indent}{}",
            words.into_iter().rev().collect::<Vec<_>>().join(" ")
        )
    })
}

fn reverse_words_selection(cx: &mut Context) {
    switch_case_impl(cx, |slice| {
        let s: String = slice.chunks().collect();
        reverse_words(&s).into()
    });
}

/// Strip the outermost `<tag>…</tag>` wrapper, returning the inner content (the
/// inverse of wrapping). Returns the input unchanged if not tag-wrapped. Pure —
/// unit tested.
fn unwrap_tag(s: &str) -> String {
    let t = s.trim();
    if !t.starts_with('<') || !t.ends_with('>') {
        return s.to_string();
    }
    let Some(open_end) = t.find('>') else {
        return s.to_string();
    };
    let Some(close_start) = t.rfind("</") else {
        return s.to_string();
    };
    if close_start <= open_end {
        return s.to_string();
    }
    t[open_end + 1..close_start].to_string()
}

fn unwrap_tag_selection(cx: &mut Context) {
    switch_case_impl(cx, |slice| {
        let s: String = slice.chunks().collect();
        unwrap_tag(&s).into()
    });
}

/// Sort blank-line-separated paragraphs alphabetically, rejoining them with a
/// single blank line. Pure — unit tested.
fn sort_paragraphs(block: &str) -> String {
    let mut paras: Vec<String> = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    for line in block.lines() {
        if line.trim().is_empty() {
            if !current.is_empty() {
                paras.push(current.join("\n"));
                current.clear();
            }
        } else {
            current.push(line);
        }
    }
    if !current.is_empty() {
        paras.push(current.join("\n"));
    }
    paras.sort();
    paras.join("\n\n")
}

fn sort_paragraphs_selection(cx: &mut Context) {
    switch_case_impl(cx, |slice| {
        let s: String = slice.chunks().collect();
        sort_paragraphs(&s).into()
    });
}

fn lighten_selection(cx: &mut Context) {
    switch_case_impl(cx, |slice| {
        let s: String = slice.chunks().collect();
        adjust_lightness(s.trim(), 10, true).unwrap_or(s).into()
    });
}

fn darken_selection(cx: &mut Context) {
    switch_case_impl(cx, |slice| {
        let s: String = slice.chunks().collect();
        adjust_lightness(s.trim(), 10, false).unwrap_or(s).into()
    });
}

/// For a hex background color, report its perceived luminance (0–1) and whether
/// black or white text reads better on it. `None` for non-hex input. Pure — unit tested.
fn contrast_recommendation(hex: &str) -> Option<String> {
    let (r, g, b) = parse_hex_rgb(hex)?;
    let lum = (0.299 * r as f64 + 0.587 * g as f64 + 0.114 * b as f64) / 255.0;
    let text = if lum > 0.5 {
        "#000000 (black)"
    } else {
        "#ffffff (white)"
    };
    Some(format!("luminance {lum:.2} → use {text} text"))
}

fn contrast_text(cx: &mut Context) {
    let s: String = {
        let (view, doc) = current!(cx.editor);
        let text = doc.text().slice(..);
        let sel = doc.selection(view.id).primary();
        text.slice(sel.from()..sel.to()).chunks().collect()
    };
    match contrast_recommendation(s.trim()) {
        Some(msg) => cx.editor.set_status(msg),
        None => cx
            .editor
            .set_error(format!("not a hex color: {}", s.trim())),
    }
}

/// Recase `replacement` to match the casing pattern of `original`
/// (ALL CAPS / Capitalized / lowercase). Pure — unit tested.
fn match_case(original: &str, replacement: &str) -> String {
    let has_upper = original.chars().any(|c| c.is_uppercase());
    let has_lower = original.chars().any(|c| c.is_lowercase());
    if has_upper && !has_lower {
        replacement.to_uppercase()
    } else if original.chars().next().is_some_and(|c| c.is_uppercase()) {
        let mut cs = replacement.chars();
        match cs.next() {
            Some(f) => f.to_uppercase().chain(cs).collect(),
            None => String::new(),
        }
    } else {
        replacement.to_string()
    }
}

/// Toggle a boolean/opposite keyword (`true`↔`false`, `yes`↔`no`, `min`↔`max`, …),
/// preserving the original casing. Returns `None` if the word has no opposite.
/// Pure — unit tested.
fn toggle_word(word: &str) -> Option<String> {
    const PAIRS: [(&str, &str); 16] = [
        ("true", "false"),
        ("yes", "no"),
        ("on", "off"),
        ("enabled", "disabled"),
        ("enable", "disable"),
        ("left", "right"),
        ("up", "down"),
        ("min", "max"),
        ("show", "hide"),
        ("first", "last"),
        ("before", "after"),
        ("start", "end"),
        ("open", "close"),
        ("width", "height"),
        ("horizontal", "vertical"),
        ("public", "private"),
    ];
    let lower = word.to_lowercase();
    for (a, b) in PAIRS {
        if lower == a {
            return Some(match_case(word, b));
        }
        if lower == b {
            return Some(match_case(word, a));
        }
    }
    None
}

fn toggle_value_selection(cx: &mut Context) {
    switch_case_impl(cx, |slice| {
        let s: String = slice.chunks().collect();
        match toggle_word(s.trim()) {
            Some(t) => t.into(),
            None => s.into(),
        }
    });
}

/// Collapse runs of internal spaces/tabs in `s` to a single space and trim
/// trailing whitespace, while preserving each line's leading indentation and the
/// line structure. Pure — unit tested.
fn normalize_whitespace(s: &str) -> String {
    let had_trailing = s.ends_with('\n');
    let mut lines: Vec<&str> = s.split('\n').collect();
    if had_trailing {
        lines.pop();
    }
    let normalized: Vec<String> = lines
        .iter()
        .map(|line| {
            let indent: String = line
                .chars()
                .take_while(|c| *c == ' ' || *c == '\t')
                .collect();
            let body = line[indent.len()..]
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            format!("{indent}{body}")
        })
        .collect();
    let out = normalized.join("\n");
    if had_trailing {
        format!("{out}\n")
    } else {
        out
    }
}

fn normalize_whitespace_selection(cx: &mut Context) {
    switch_case_impl(cx, |slice| {
        let s: String = slice.chunks().collect();
        normalize_whitespace(&s).into()
    });
}

fn title_case_selection(cx: &mut Context) {
    switch_case_impl(cx, |slice| {
        let s: String = slice.chunks().collect();
        title_case(&s).into()
    });
}

/// Capitalize the first letter of each sentence (after `.`/`!`/`?` and at the
/// start), leaving the rest of the text untouched (non-destructive — preserves
/// acronyms etc.). Pure — unit tested.
fn sentence_case(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut capitalize_next = true;
    for c in s.chars() {
        if capitalize_next && c.is_alphabetic() {
            out.extend(c.to_uppercase());
            capitalize_next = false;
        } else {
            out.push(c);
            if matches!(c, '.' | '!' | '?') {
                capitalize_next = true;
            }
        }
    }
    out
}

fn sentence_case_selection(cx: &mut Context) {
    switch_case_impl(cx, |slice| {
        let s: String = slice.chunks().collect();
        sentence_case(&s).into()
    });
}

/// Convert smart/curly typography to plain ASCII: curly quotes → `'`/`"`, en/em
/// dashes → `-`, ellipsis → `...`, non-breaking space → space. Pure — unit tested.
fn straighten_quotes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}' => out.push('\''),
            '\u{201C}' | '\u{201D}' | '\u{201E}' | '\u{201F}' => out.push('"'),
            '\u{2013}' | '\u{2014}' => out.push('-'),
            '\u{2026}' => out.push_str("..."),
            '\u{00A0}' => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

fn straighten_quotes_selection(cx: &mut Context) {
    switch_case_impl(cx, |slice| {
        let s: String = slice.chunks().collect();
        straighten_quotes(&s).into()
    });
}

/// GitHub-style heading anchor: lowercase, alphanumerics kept, spaces and hyphens
/// become `-`, underscores kept, all other punctuation dropped (no run collapsing,
/// matching GitHub's slug algorithm). Pure — unit tested.
fn github_anchor(title: &str) -> String {
    let mut out = String::with_capacity(title.len());
    for c in title.chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
        } else if c == ' ' || c == '-' {
            out.push('-');
        } else if c == '_' {
            out.push('_');
        }
    }
    out
}

/// Build a markdown table of contents (nested `- [title](#anchor)` list) from the
/// ATX (`#`) headings in `text`, skipping fenced code blocks. Pure — unit tested.
fn markdown_toc(text: &str) -> String {
    let mut out = String::new();
    let mut in_fence = false;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        let hashes = trimmed.chars().take_while(|&c| c == '#').count();
        if (1..=6).contains(&hashes) && trimmed[hashes..].starts_with(' ') {
            let title = trimmed[hashes..].trim();
            if !title.is_empty() {
                let indent = "  ".repeat(hashes - 1);
                let anchor = github_anchor(title);
                out.push_str(&format!("{indent}- [{title}](#{anchor})\n"));
            }
        }
    }
    out
}

/// `:toc` — insert a markdown table of contents (from the buffer's headings) at
/// the cursor.
fn insert_toc(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);
    let text: String = doc.text().slice(..).chunks().collect();
    let toc = markdown_toc(&text);
    if toc.is_empty() {
        cx.editor.set_error("no markdown headings found");
        return;
    }
    let pos = doc
        .selection(view.id)
        .primary()
        .cursor(doc.text().slice(..));
    let transaction =
        Transaction::change(doc.text(), std::iter::once((pos, pos, Some(toc.into()))));
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
}

fn slugify_selection(cx: &mut Context) {
    switch_case_impl(cx, |slice| {
        let s: String = slice.chunks().collect();
        slugify(&s).into()
    });
}

/// Turn a slug/identifier (`foo-bar`, `my_file`) into a readable Title-Cased
/// label (`Foo Bar`, `My File`) — the inverse of slugify. Pure — unit tested.
fn humanize(s: &str) -> String {
    s.split(['-', '_', ' '])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut cs = w.chars();
            match cs.next() {
                Some(f) => f.to_uppercase().chain(cs).collect::<String>(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn humanize_selection(cx: &mut Context) {
    switch_case_impl(cx, |slice| {
        let s: String = slice.chunks().collect();
        humanize(&s).into()
    });
}

/// Transpose a delimited table (rows ↔ columns). The delimiter is a tab if any
/// tab is present, otherwise a comma. Ragged rows are padded with empties. Pure —
/// unit tested.
fn transpose_csv(s: &str) -> String {
    let delim = if s.contains('\t') { '\t' } else { ',' };
    let rows: Vec<Vec<&str>> = s
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.split(delim).collect())
        .collect();
    if rows.is_empty() {
        return s.to_string();
    }
    let ncol = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    let sep = delim.to_string();
    (0..ncol)
        .map(|c| {
            rows.iter()
                .map(|r| r.get(c).copied().unwrap_or(""))
                .collect::<Vec<_>>()
                .join(&sep)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn transpose_csv_selection(cx: &mut Context) {
    switch_case_impl(cx, |slice| {
        let s: String = slice.chunks().collect();
        transpose_csv(&s).into()
    });
}

/// Convert CSV/TSV (first row = headers) to a pretty JSON array of objects. All
/// values are strings (JSON-escaped). Pure — unit tested.
fn csv_to_json(s: &str) -> String {
    let delim = if s.contains('\t') { '\t' } else { ',' };
    let lines: Vec<&str> = s.lines().filter(|l| !l.trim().is_empty()).collect();
    if lines.len() < 2 {
        return s.to_string();
    }
    let headers: Vec<&str> = lines[0].split(delim).map(|h| h.trim()).collect();
    let objs: Vec<String> = lines[1..]
        .iter()
        .map(|line| {
            let fields: Vec<&str> = line.split(delim).collect();
            let pairs: Vec<String> = headers
                .iter()
                .enumerate()
                .map(|(i, h)| {
                    let v = fields.get(i).map(|f| f.trim()).unwrap_or("");
                    format!("\"{}\": \"{}\"", json_escape(h), json_escape(v))
                })
                .collect();
            format!("  {{{}}}", pairs.join(", "))
        })
        .collect();
    format!("[\n{}\n]", objs.join(",\n"))
}

fn csv_to_json_selection(cx: &mut Context) {
    switch_case_impl(cx, |slice| {
        let s: String = slice.chunks().collect();
        csv_to_json(&s).into()
    });
}

/// Backslash-escape regex metacharacters so the text can be searched literally.
/// Pure — unit tested.
fn regex_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(
            c,
            '.' | '^' | '$' | '*' | '+' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '|' | '\\'
        ) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn regex_escape_selection(cx: &mut Context) {
    switch_case_impl(cx, |slice| {
        let s: String = slice.chunks().collect();
        regex_escape(&s).into()
    });
}

/// Apply `f` to each line of `block`, preserving the trailing-newline shape.
fn map_lines(block: &str, f: impl Fn(&str) -> String) -> String {
    let had_trailing = block.ends_with('\n');
    let mut lines: Vec<&str> = block.split('\n').collect();
    if had_trailing {
        lines.pop();
    }
    let out = lines.iter().map(|l| f(l)).collect::<Vec<_>>().join("\n");
    if had_trailing {
        format!("{out}\n")
    } else {
        out
    }
}

/// Prefix each line with `"> "` (markdown blockquote). Pure — unit tested.
fn blockquote(block: &str) -> String {
    map_lines(block, |l| format!("> {l}"))
}

/// Strip a leading `"> "` (or `">"`) from each line. Pure — unit tested.
fn unblockquote(block: &str) -> String {
    map_lines(block, |l| {
        l.strip_prefix("> ")
            .or_else(|| l.strip_prefix('>'))
            .unwrap_or(l)
            .to_string()
    })
}

fn blockquote_selection(cx: &mut Context) {
    switch_case_impl(cx, |slice| {
        let s: String = slice.chunks().collect();
        blockquote(&s).into()
    });
}

fn unblockquote_selection(cx: &mut Context) {
    switch_case_impl(cx, |slice| {
        let s: String = slice.chunks().collect();
        unblockquote(&s).into()
    });
}

/// Prefix each non-empty line with `"- "` (markdown bullet list); blank lines are
/// left blank. Pure — unit tested.
fn bullet_list(block: &str) -> String {
    map_lines(block, |l| {
        if l.trim().is_empty() {
            l.to_string()
        } else {
            format!("- {l}")
        }
    })
}

/// Strip a leading `"- "`, `"* "`, or `"+ "` bullet from each line. Pure — unit tested.
fn unbullet(block: &str) -> String {
    map_lines(block, |l| {
        l.strip_prefix("- ")
            .or_else(|| l.strip_prefix("* "))
            .or_else(|| l.strip_prefix("+ "))
            .unwrap_or(l)
            .to_string()
    })
}

fn bullet_list_selection(cx: &mut Context) {
    switch_case_impl(cx, |slice| {
        let s: String = slice.chunks().collect();
        bullet_list(&s).into()
    });
}

fn unbullet_selection(cx: &mut Context) {
    switch_case_impl(cx, |slice| {
        let s: String = slice.chunks().collect();
        unbullet(&s).into()
    });
}

/// Remove ANSI/VT escape sequences (CSI `ESC[…`, OSC `ESC]…BEL/ST`, and lone
/// `ESC`) from `s`, leaving plain text. Multi-byte text is preserved.
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        match chars.peek() {
            Some('[') => {
                chars.next();
                // CSI: consume until a final byte in 0x40..=0x7e
                for nc in chars.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&nc) {
                        break;
                    }
                }
            }
            Some(']') => {
                chars.next();
                // OSC: consume until BEL (0x07) or ST (ESC \)
                while let Some(&nc) = chars.peek() {
                    if nc == '\u{07}' {
                        chars.next();
                        break;
                    }
                    if nc == '\u{1b}' {
                        chars.next();
                        if chars.peek() == Some(&'\\') {
                            chars.next();
                        }
                        break;
                    }
                    chars.next();
                }
            }
            _ => {} // lone ESC: drop it
        }
    }
    out
}

fn strip_ansi_selection(cx: &mut Context) {
    switch_case_impl(cx, |slice| {
        let s: String = slice.chunks().collect();
        strip_ansi(&s).into()
    });
}

/// Escape the five HTML-significant characters (`& < > " '`) into their entity
/// forms so text can be embedded safely in markup. Pure — unit tested.
fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// Decode HTML entities back to characters: the named five (`amp lt gt quot
/// apos`) plus decimal (`&#NN;`) and hex (`&#xHH;`) numeric references. Unknown
/// entities are left verbatim. Pure — unit tested.
fn html_unescape(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '&' {
            if let Some(semi) = chars[i + 1..].iter().position(|&c| c == ';') {
                let entity: String = chars[i + 1..i + 1 + semi].iter().collect();
                let replacement = match entity.as_str() {
                    "amp" => Some('&'),
                    "lt" => Some('<'),
                    "gt" => Some('>'),
                    "quot" => Some('"'),
                    "apos" | "#39" => Some('\''),
                    _ => {
                        if let Some(hex) = entity
                            .strip_prefix("#x")
                            .or_else(|| entity.strip_prefix("#X"))
                        {
                            u32::from_str_radix(hex, 16).ok().and_then(char::from_u32)
                        } else if let Some(dec) = entity.strip_prefix('#') {
                            dec.parse::<u32>().ok().and_then(char::from_u32)
                        } else {
                            None
                        }
                    }
                };
                if let Some(ch) = replacement {
                    out.push(ch);
                    i += semi + 2;
                    continue;
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn html_escape_selection(cx: &mut Context) {
    switch_case_impl(cx, |slice| {
        let s: String = slice.chunks().collect();
        html_escape(&s).into()
    });
}

fn html_unescape_selection(cx: &mut Context) {
    switch_case_impl(cx, |slice| {
        let s: String = slice.chunks().collect();
        html_unescape(&s).into()
    });
}

/// Reverse the characters of `s` (distinct from reversing line order). Operates on
/// Unicode scalar values, so precomposed accented characters are preserved. Pure —
/// unit tested.
fn reverse_chars(s: &str) -> String {
    s.chars().rev().collect()
}

fn reverse_chars_selection(cx: &mut Context) {
    switch_case_impl(cx, |slice| {
        let s: String = slice.chunks().collect();
        reverse_chars(&s).into()
    });
}

/// Escape text for use inside a JSON string literal (does not add the surrounding
/// quotes). Pure — unit tested.
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// Unescape JSON-string escape sequences (`\n \t \" \\ \uXXXX` …), leaving any
/// unrecognized escape verbatim. Pure — unit tested.
fn json_unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('"') => out.push('"'),
            Some('\\') => out.push('\\'),
            Some('/') => out.push('/'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some('b') => out.push('\u{08}'),
            Some('f') => out.push('\u{0c}'),
            Some('u') => {
                let hex: String = (0..4).filter_map(|_| chars.next()).collect();
                match u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                    Some(ch) => out.push(ch),
                    None => {
                        out.push_str("\\u");
                        out.push_str(&hex);
                    }
                }
            }
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

fn json_escape_selection(cx: &mut Context) {
    switch_case_impl(cx, |slice| {
        let s: String = slice.chunks().collect();
        json_escape(&s).into()
    });
}

/// Wrap text in double quotes and JSON-escape its contents, producing a complete
/// JSON string literal (newlines become `\n`). Pure — unit tested.
fn to_json_string(s: &str) -> String {
    format!("\"{}\"", json_escape(s))
}

fn to_json_string_selection(cx: &mut Context) {
    switch_case_impl(cx, |slice| {
        let s: String = slice.chunks().collect();
        to_json_string(&s).into()
    });
}

fn json_unescape_selection(cx: &mut Context) {
    switch_case_impl(cx, |slice| {
        let s: String = slice.chunks().collect();
        json_unescape(&s).into()
    });
}

/// Render text as space-separated lowercase hex of its UTF-8 bytes. Pure — unit tested.
fn to_hex(s: &str) -> String {
    s.bytes()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Decode hex back to text: every pair of hex digits becomes a byte, ignoring any
/// non-hex characters (spaces, newlines). Lossy UTF-8. Pure — unit tested.
fn from_hex(s: &str) -> String {
    let digits: Vec<u32> = s.chars().filter_map(|c| c.to_digit(16)).collect();
    let mut bytes = Vec::with_capacity(digits.len() / 2);
    for pair in digits.chunks(2) {
        if pair.len() == 2 {
            bytes.push((pair[0] * 16 + pair[1]) as u8);
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

fn to_hex_selection(cx: &mut Context) {
    switch_case_impl(cx, |slice| {
        let s: String = slice.chunks().collect();
        to_hex(&s).into()
    });
}

fn from_hex_selection(cx: &mut Context) {
    switch_case_impl(cx, |slice| {
        let s: String = slice.chunks().collect();
        from_hex(&s).into()
    });
}

/// Is `cells` a markdown table separator row (e.g. `---`, `:--`, `:-:`)?
fn is_table_separator(cells: &[String]) -> bool {
    !cells.is_empty()
        && cells.iter().all(|c| {
            let t = c.trim();
            !t.is_empty() && t.contains('-') && t.chars().all(|ch| ch == '-' || ch == ':')
        })
}

/// Re-align a markdown pipe table: pad every column to its widest cell, normalize
/// the `| a | b |` spacing, and rebuild the separator row to match (preserving
/// alignment colons). Left-aligns data cells. Pure — unit tested.
fn format_markdown_table(block: &str) -> String {
    let had_trailing = block.ends_with('\n');
    let mut lines: Vec<&str> = block.split('\n').collect();
    if had_trailing {
        lines.pop();
    }
    // Parse each row into trimmed cells, dropping the empty edges from outer pipes.
    let rows: Vec<Vec<String>> = lines
        .iter()
        .map(|line| {
            let t = line.trim();
            let t = t.strip_prefix('|').unwrap_or(t);
            let t = t.strip_suffix('|').unwrap_or(t);
            t.split('|').map(|c| c.trim().to_string()).collect()
        })
        .collect();
    let ncol = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    if ncol == 0 {
        return block.to_string();
    }
    // Column widths from non-separator rows (min 3 so `---` fits).
    let mut widths = vec![3usize; ncol];
    for row in &rows {
        if is_table_separator(row) {
            continue;
        }
        for (i, cell) in row.iter().enumerate() {
            widths[i] = widths[i].max(cell.chars().count());
        }
    }
    let mut out_lines = Vec::with_capacity(rows.len());
    for row in &rows {
        let sep = is_table_separator(row);
        let cells: Vec<String> = (0..ncol)
            .map(|i| {
                let cell = row.get(i).map(|s| s.as_str()).unwrap_or("");
                if sep {
                    let lc = cell.starts_with(':');
                    let rc = cell.ends_with(':') && cell.len() > 1;
                    let dashes = widths[i].saturating_sub(lc as usize + rc as usize).max(1);
                    let mut s = String::new();
                    if lc {
                        s.push(':');
                    }
                    s.push_str(&"-".repeat(dashes));
                    if rc {
                        s.push(':');
                    }
                    s
                } else {
                    format!("{:<width$}", cell, width = widths[i])
                }
            })
            .collect();
        out_lines.push(format!("| {} |", cells.join(" | ")));
    }
    let out = out_lines.join("\n");
    if had_trailing {
        format!("{out}\n")
    } else {
        out
    }
}

fn format_table_selection(cx: &mut Context) {
    switch_case_impl(cx, |slice| {
        let s: String = slice.chunks().collect();
        format_markdown_table(&s).into()
    });
}

/// Convert CSV/TSV text to an aligned markdown table (first row is the header).
/// The delimiter is a tab if any tab is present, otherwise a comma. Pure — unit tested.
fn csv_to_markdown_table(block: &str) -> String {
    let lines: Vec<&str> = block.lines().filter(|l| !l.trim().is_empty()).collect();
    if lines.is_empty() {
        return block.to_string();
    }
    let delim = if block.contains('\t') { '\t' } else { ',' };
    let mut rows: Vec<String> = Vec::with_capacity(lines.len() + 1);
    for (i, line) in lines.iter().enumerate() {
        let cells: Vec<&str> = line.split(delim).map(|c| c.trim()).collect();
        rows.push(format!("| {} |", cells.join(" | ")));
        if i == 0 {
            let sep = vec!["---"; cells.len()].join(" | ");
            rows.push(format!("| {sep} |"));
        }
    }
    let raw = rows.join("\n");
    // reuse the aligner; keep the input's trailing-newline shape
    let table = format_markdown_table(&raw);
    if block.ends_with('\n') {
        format!("{table}\n")
    } else {
        table
    }
}

fn csv_to_table_selection(cx: &mut Context) {
    switch_case_impl(cx, |slice| {
        let s: String = slice.chunks().collect();
        csv_to_markdown_table(&s).into()
    });
}

/// Quote a CSV field if it contains a comma, quote, or newline (RFC 4180).
fn csv_quote(cell: &str) -> String {
    if cell.contains([',', '"', '\n']) {
        format!("\"{}\"", cell.replace('"', "\"\""))
    } else {
        cell.to_string()
    }
}

/// Convert a markdown pipe table back to CSV, dropping the separator row. Pure —
/// unit tested.
fn markdown_table_to_csv(block: &str) -> String {
    let mut out_lines = Vec::new();
    for line in block.lines() {
        let t = line.trim();
        if t.is_empty() || !t.contains('|') {
            continue;
        }
        let t = t.strip_prefix('|').unwrap_or(t);
        let t = t.strip_suffix('|').unwrap_or(t);
        let cells: Vec<String> = t.split('|').map(|c| c.trim().to_string()).collect();
        if is_table_separator(&cells) {
            continue;
        }
        out_lines.push(
            cells
                .iter()
                .map(|c| csv_quote(c))
                .collect::<Vec<_>>()
                .join(","),
        );
    }
    let mut res = out_lines.join("\n");
    if block.ends_with('\n') && !res.is_empty() {
        res.push('\n');
    }
    res
}

fn table_to_csv_selection(cx: &mut Context) {
    switch_case_impl(cx, |slice| {
        let s: String = slice.chunks().collect();
        markdown_table_to_csv(&s).into()
    });
}

/// Pretty-print JSON by reformatting only the whitespace *outside* string
/// literals — so key order and all values are preserved exactly (no parse/
/// re-serialize round-trip). `indent` is one indentation unit. Pure — unit tested.
fn pretty_json(s: &str, indent: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len() + s.len() / 2);
    let mut depth = 0usize;
    let mut in_str = false;
    let mut esc = false;
    let newline = |out: &mut String, depth: usize| {
        out.push('\n');
        for _ in 0..depth {
            out.push_str(indent);
        }
    };
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if in_str {
            out.push(c);
            if esc {
                esc = false;
            } else if c == '\\' {
                esc = true;
            } else if c == '"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        match c {
            '"' => {
                in_str = true;
                out.push('"');
            }
            '{' | '[' => {
                let close = if c == '{' { '}' } else { ']' };
                let mut j = i + 1;
                while j < chars.len() && chars[j].is_whitespace() {
                    j += 1;
                }
                if j < chars.len() && chars[j] == close {
                    out.push(c);
                    out.push(close);
                    i = j + 1;
                    continue;
                }
                out.push(c);
                depth += 1;
                newline(&mut out, depth);
            }
            '}' | ']' => {
                depth = depth.saturating_sub(1);
                newline(&mut out, depth);
                out.push(c);
            }
            ',' => {
                out.push(',');
                newline(&mut out, depth);
            }
            ':' => out.push_str(": "),
            c if c.is_whitespace() => {}
            c => out.push(c),
        }
        i += 1;
    }
    out
}

/// Minify JSON by removing all whitespace outside string literals. Pure — unit tested.
fn minify_json(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_str = false;
    let mut esc = false;
    for c in s.chars() {
        if in_str {
            out.push(c);
            if esc {
                esc = false;
            } else if c == '\\' {
                esc = true;
            } else if c == '"' {
                in_str = false;
            }
        } else if c == '"' {
            in_str = true;
            out.push('"');
        } else if !c.is_whitespace() {
            out.push(c);
        }
    }
    out
}

fn json_pretty_selection(cx: &mut Context) {
    switch_case_impl(cx, |slice| {
        let s: String = slice.chunks().collect();
        pretty_json(&s, "  ").into()
    });
}

fn json_minify_selection(cx: &mut Context) {
    switch_case_impl(cx, |slice| {
        let s: String = slice.chunks().collect();
        minify_json(&s).into()
    });
}

/// Pretty-print XML/HTML: one tag per line, indented by nesting depth. Handles
/// closing/self-closing tags, comments, declarations, and `>` inside quoted
/// attributes; text runs are trimmed onto their own line. Pure — unit tested.
fn pretty_xml(s: &str, indent: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len() + s.len() / 2);
    let mut depth = 0usize;
    let mut i = 0;
    let emit_indent = |out: &mut String, depth: usize| {
        if !out.is_empty() {
            out.push('\n');
        }
        for _ in 0..depth {
            out.push_str(indent);
        }
    };
    while i < chars.len() {
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        if i >= chars.len() {
            break;
        }
        if chars[i] == '<' {
            // comment <!-- ... -->
            if chars[i..].starts_with(&['<', '!', '-', '-']) {
                let mut j = i + 4;
                while j + 2 < chars.len()
                    && !(chars[j] == '-' && chars[j + 1] == '-' && chars[j + 2] == '>')
                {
                    j += 1;
                }
                let end = (j + 3).min(chars.len());
                emit_indent(&mut out, depth);
                out.extend(&chars[i..end]);
                i = end;
                continue;
            }
            // regular tag: find '>' while respecting quotes
            let mut j = i + 1;
            let mut quote: Option<char> = None;
            while j < chars.len() {
                let c = chars[j];
                match quote {
                    Some(q) if c == q => quote = None,
                    Some(_) => {}
                    None if c == '"' || c == '\'' => quote = Some(c),
                    None if c == '>' => break,
                    None => {}
                }
                j += 1;
            }
            let end = (j + 1).min(chars.len());
            let tag: String = chars[i..end].iter().collect();
            let is_close = tag.starts_with("</");
            let is_self = tag.ends_with("/>");
            let is_decl = tag.starts_with("<?") || tag.starts_with("<!");
            if is_close {
                depth = depth.saturating_sub(1);
            }
            emit_indent(&mut out, depth);
            out.push_str(&tag);
            if !is_close && !is_self && !is_decl {
                depth += 1;
            }
            i = end;
        } else {
            let start = i;
            while i < chars.len() && chars[i] != '<' {
                i += 1;
            }
            let text: String = chars[start..i].iter().collect();
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                emit_indent(&mut out, depth);
                out.push_str(trimmed);
            }
        }
    }
    out
}

fn xml_pretty_selection(cx: &mut Context) {
    switch_case_impl(cx, |slice| {
        let s: String = slice.chunks().collect();
        pretty_xml(&s, "  ").into()
    });
}

pub fn scroll(cx: &mut Context, offset: usize, direction: Direction, sync_cursor: bool) {
    use Direction::*;
    let config = cx.editor.config();
    let (view, doc) = current!(cx.editor);
    let mut view_offset = doc.view_offset(view.id);

    let range = doc.selection(view.id).primary();
    let text = doc.text().slice(..);

    let cursor = range.cursor(text);
    let height = view.inner_height();

    let scrolloff = config.scrolloff.min(height.saturating_sub(1) / 2);
    let offset = match direction {
        Forward => offset as isize,
        Backward => -(offset as isize),
    };

    let doc_text = doc.text().slice(..);
    let viewport = view.inner_area(doc);
    let text_fmt = doc.text_format(viewport.width, None, Some(view.id));
    (view_offset.anchor, view_offset.vertical_offset) = char_idx_at_visual_offset(
        doc_text,
        view_offset.anchor,
        view_offset.vertical_offset as isize + offset,
        0,
        &text_fmt,
        // &annotations,
        &view.text_annotations(&*doc, None),
    );
    doc.set_view_offset(view.id, view_offset);

    let doc_text = doc.text().slice(..);
    let mut annotations = view.text_annotations(&*doc, None);

    if sync_cursor {
        let movement = match cx.editor.mode {
            Mode::Select => Movement::Extend,
            _ => Movement::Move,
        };
        // TODO: When inline diagnostics gets merged- 1. move_vertically_visual removes
        // line annotations/diagnostics so the cursor may jump further than the view.
        // 2. If the cursor lands on a complete line of virtual text, the cursor will
        // jump a different distance than the view.
        let selection = doc.selection(view.id).clone().transform(|range| {
            move_vertically_visual(
                doc_text,
                range,
                direction,
                offset.unsigned_abs(),
                movement,
                &text_fmt,
                &mut annotations,
            )
        });
        drop(annotations);
        doc.set_selection(view.id, selection);
        return;
    }

    let view_offset = doc.view_offset(view.id);

    let mut head;
    match direction {
        Forward => {
            let off;
            (head, off) = char_idx_at_visual_offset(
                doc_text,
                view_offset.anchor,
                (view_offset.vertical_offset + scrolloff) as isize,
                0,
                &text_fmt,
                &annotations,
            );
            head += (off != 0) as usize;
            if head <= cursor {
                return;
            }
        }
        Backward => {
            head = char_idx_at_visual_offset(
                doc_text,
                view_offset.anchor,
                (view_offset.vertical_offset + height - scrolloff - 1) as isize,
                0,
                &text_fmt,
                &annotations,
            )
            .0;
            if head >= cursor {
                return;
            }
        }
    }

    let anchor = if cx.editor.mode == Mode::Select {
        range.anchor
    } else {
        head
    };

    // replace primary selection with an empty selection at cursor pos
    let prim_sel = Range::new(anchor, head);
    let mut sel = doc.selection(view.id).clone();
    let idx = sel.primary_index();
    sel = sel.replace(idx, prim_sel);
    drop(annotations);
    doc.set_selection(view.id, sel);
}

fn page_up(cx: &mut Context) {
    let view = view!(cx.editor);
    let offset = view.inner_height();
    scroll(cx, offset, Direction::Backward, false);
}

fn page_down(cx: &mut Context) {
    let view = view!(cx.editor);
    let offset = view.inner_height();
    scroll(cx, offset, Direction::Forward, false);
}

fn half_page_up(cx: &mut Context) {
    let view = view!(cx.editor);
    let offset = view.inner_height() / 2;
    scroll(cx, offset, Direction::Backward, false);
}

fn half_page_down(cx: &mut Context) {
    let view = view!(cx.editor);
    let offset = view.inner_height() / 2;
    scroll(cx, offset, Direction::Forward, false);
}

fn page_cursor_up(cx: &mut Context) {
    let view = view!(cx.editor);
    let offset = view.inner_height();
    scroll(cx, offset, Direction::Backward, true);
}

fn page_cursor_down(cx: &mut Context) {
    let view = view!(cx.editor);
    let offset = view.inner_height();
    scroll(cx, offset, Direction::Forward, true);
}

fn page_cursor_half_up(cx: &mut Context) {
    let view = view!(cx.editor);
    let offset = view.inner_height() / 2;
    scroll(cx, offset, Direction::Backward, true);
}

fn page_cursor_half_down(cx: &mut Context) {
    let view = view!(cx.editor);
    let offset = view.inner_height() / 2;
    scroll(cx, offset, Direction::Forward, true);
}

#[allow(deprecated)]
// currently uses the deprecated `visual_coords_at_pos`/`pos_at_visual_coords` functions
// as this function ignores softwrapping (and virtual text) and instead only cares
// about "text visual position"
//
// TODO: implement a variant of that uses visual lines and respects virtual text
fn copy_selection_on_line(cx: &mut Context, direction: Direction) {
    use zemacs_core::{pos_at_visual_coords, visual_coords_at_pos};

    let count = cx.count();
    let (view, doc) = current!(cx.editor);
    let text = doc.text().slice(..);
    let selection = doc.selection(view.id);
    let mut ranges = SmallVec::with_capacity(selection.ranges().len() * (count + 1));
    ranges.extend_from_slice(selection.ranges());
    let mut primary_index = 0;
    for range in selection.iter() {
        let is_primary = *range == selection.primary();

        // The range is always head exclusive
        let (head, anchor) = if range.anchor < range.head {
            (range.head - 1, range.anchor)
        } else {
            (range.head, range.anchor.saturating_sub(1))
        };

        let tab_width = doc.tab_width();

        let head_pos = visual_coords_at_pos(text, head, tab_width);
        let anchor_pos = visual_coords_at_pos(text, anchor, tab_width);

        let height = std::cmp::max(head_pos.row, anchor_pos.row)
            - std::cmp::min(head_pos.row, anchor_pos.row)
            + 1;

        if is_primary {
            primary_index = ranges.len();
        }
        ranges.push(*range);

        let mut sels = 0;
        let mut i = 0;
        while sels < count {
            let offset = (i + 1) * height;

            let anchor_row = match direction {
                Direction::Forward => anchor_pos.row + offset,
                Direction::Backward => anchor_pos.row.saturating_sub(offset),
            };

            let head_row = match direction {
                Direction::Forward => head_pos.row + offset,
                Direction::Backward => head_pos.row.saturating_sub(offset),
            };

            if anchor_row >= text.len_lines() || head_row >= text.len_lines() {
                break;
            }

            let anchor =
                pos_at_visual_coords(text, Position::new(anchor_row, anchor_pos.col), tab_width);
            let head = pos_at_visual_coords(text, Position::new(head_row, head_pos.col), tab_width);

            // skip lines that are too short
            if visual_coords_at_pos(text, anchor, tab_width).col == anchor_pos.col
                && visual_coords_at_pos(text, head, tab_width).col == head_pos.col
            {
                if is_primary {
                    primary_index = ranges.len();
                }
                // This is Range::new(anchor, head), but it will place the cursor on the correct column
                ranges.push(Range::point(anchor).put_cursor(text, head, true));
                sels += 1;
            }

            if anchor_row == 0 && head_row == 0 {
                break;
            }

            i += 1;
        }
    }

    let selection = Selection::new(ranges, primary_index);
    doc.set_selection(view.id, selection);
}

fn copy_selection_on_prev_line(cx: &mut Context) {
    copy_selection_on_line(cx, Direction::Backward)
}

fn copy_selection_on_next_line(cx: &mut Context) {
    copy_selection_on_line(cx, Direction::Forward)
}

/// Given the full text of the line-block being duplicated, return the string to
/// insert so the copy lands on its own line(s). Ensures a separating newline when
/// the block (the file's last line) has no trailing one.
fn duplicate_block_insert(block: &str, downward: bool) -> String {
    if block.ends_with('\n') {
        block.to_string()
    } else if downward {
        format!("\n{block}")
    } else {
        format!("{block}\n")
    }
}

/// Duplicate the whole line(s) spanned by the primary selection above or below
/// it (VS Code's Shift-Alt-Up/Down). Honours the editor count for N copies.
fn duplicate_selection_impl(cx: &mut Context, downward: bool) {
    let count = cx.count();
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let slice = text.slice(..);
    let range = doc.selection(view.id).primary();
    let start_line = slice.char_to_line(range.from());
    let last_char = range.to().saturating_sub(1).max(range.from());
    let end_line = slice.char_to_line(last_char);
    let block_start = slice.line_to_char(start_line);
    let block_end = if end_line + 1 < slice.len_lines() {
        slice.line_to_char(end_line + 1)
    } else {
        slice.len_chars()
    };
    let block: String = slice.slice(block_start..block_end).to_string();
    let insert = duplicate_block_insert(&block, downward).repeat(count);
    let pos = if downward { block_end } else { block_start };
    let transaction = Transaction::change(text, std::iter::once((pos, pos, Some(insert.into()))));
    doc.apply(&transaction, view.id);
    exit_select_mode(cx);
}

fn duplicate_selection_down(cx: &mut Context) {
    duplicate_selection_impl(cx, true)
}

fn duplicate_selection_up(cx: &mut Context) {
    duplicate_selection_impl(cx, false)
}

/// Reorder a `block` (ends with a newline unless it is the file's last line) with
/// the `neighbor` line below it. Returns the new text for the spanned region.
/// Keeps exactly one EOL between lines and never introduces a trailing newline
/// the region didn't have.
fn swap_block_down(block: &str, neighbor: &str) -> String {
    if neighbor.ends_with('\n') {
        format!("{neighbor}{block}")
    } else {
        // neighbor was the final line (no EOL); after moving up it gains one and
        // the block becomes the new final line without a trailing newline.
        format!("{neighbor}\n{}", block.strip_suffix('\n').unwrap_or(block))
    }
}

/// Reorder a `block` with the `neighbor` line above it (mirror of [`swap_block_down`]).
fn swap_block_up(block: &str, neighbor: &str) -> String {
    if block.ends_with('\n') {
        format!("{block}{neighbor}")
    } else {
        // block was the final line (no EOL); after moving up it gains one and the
        // neighbor becomes the new final line without a trailing newline.
        format!(
            "{block}\n{}",
            neighbor.strip_suffix('\n').unwrap_or(neighbor)
        )
    }
}

/// Drag the whole line(s) spanned by the primary selection up or down past the
/// adjacent line (VS Code's Alt-Up/Down), preserving the cursor column so the
/// move can be repeated. Honours the editor count for repeated hops.
fn move_text_line_impl(cx: &mut Context, downward: bool) {
    let count = cx.count();
    for _ in 0..count {
        let (view, doc) = current!(cx.editor);
        let text = doc.text();
        let slice = text.slice(..);
        let len_lines = slice.len_lines();
        let sel = doc.selection(view.id).clone();
        let range = sel.primary();
        let start_line = slice.char_to_line(range.from());
        let last_char = range.to().saturating_sub(1).max(range.from());
        let end_line = slice.char_to_line(last_char);

        // Bail at the buffer edges (nothing to swap with).
        if downward && end_line + 1 >= len_lines {
            return;
        }
        if !downward && start_line == 0 {
            return;
        }

        let block_start = slice.line_to_char(start_line);
        let (region_start, region_end, new_text, new_start_line) = if downward {
            let neighbor_line = end_line + 1;
            let block_end = slice.line_to_char(neighbor_line);
            let neighbor_end = if neighbor_line + 1 < len_lines {
                slice.line_to_char(neighbor_line + 1)
            } else {
                slice.len_chars()
            };
            let block: String = slice.slice(block_start..block_end).to_string();
            let neighbor: String = slice.slice(block_end..neighbor_end).to_string();
            (
                block_start,
                neighbor_end,
                swap_block_down(&block, &neighbor),
                start_line + 1,
            )
        } else {
            let neighbor_line = start_line - 1;
            let region_start = slice.line_to_char(neighbor_line);
            let block_end = if end_line + 1 < len_lines {
                slice.line_to_char(end_line + 1)
            } else {
                slice.len_chars()
            };
            let neighbor: String = slice.slice(region_start..block_start).to_string();
            let block: String = slice.slice(block_start..block_end).to_string();
            (
                region_start,
                block_end,
                swap_block_up(&block, &neighbor),
                start_line - 1,
            )
        };

        let transaction = Transaction::change(
            text,
            std::iter::once((region_start, region_end, Some(new_text.into()))),
        );
        doc.apply(&transaction, view.id);

        // Shift the selection by the block's displacement so the cursor follows
        // the moved text (every char of the block shifts by the same delta).
        let new_slice = doc.text().slice(..);
        let delta = new_slice.line_to_char(new_start_line) as isize - block_start as isize;
        let shift = |p: usize| (p as isize + delta).max(0) as usize;
        let new_range = Range::new(shift(range.anchor), shift(range.head));
        doc.set_selection(view.id, Selection::single(new_range.anchor, new_range.head));
    }
    exit_select_mode(cx);
}

fn move_text_line_down(cx: &mut Context) {
    move_text_line_impl(cx, true)
}

fn move_text_line_up(cx: &mut Context) {
    move_text_line_impl(cx, false)
}

fn select_all(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);

    // Confine to the narrowed region when narrowed — per-view narrow wins over the doc-wide one.
    let start = doc.view_point_min(view.id);
    let end = doc.view_point_max(view.id);
    doc.set_selection(view.id, Selection::single(start, end))
}

/// Char-offset ranges of every non-overlapping occurrence of `needle` in
/// `haystack`. O(n) overall. Pure — unit tested.
fn find_all_ranges(haystack: &str, needle: &str) -> Vec<(usize, usize)> {
    if needle.is_empty() {
        return Vec::new();
    }
    let needle_chars = needle.chars().count();
    let mut ranges = Vec::new();
    let mut byte = 0;
    let mut char_off = 0;
    while let Some(rel) = haystack[byte..].find(needle) {
        let match_byte = byte + rel;
        char_off += haystack[byte..match_byte].chars().count();
        ranges.push((char_off, char_off + needle_chars));
        char_off += needle_chars;
        byte = match_byte + needle.len();
    }
    ranges
}

/// Select every occurrence of the primary selection's text across the whole
/// buffer (VS Code's "select all occurrences" / Ctrl-Shift-L).
fn select_all_instances(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);
    let slice = doc.text().slice(..);
    let primary = doc.selection(view.id).primary();
    let needle: String = slice.slice(primary.from()..primary.to()).chunks().collect();
    if needle.is_empty() {
        cx.editor.set_error("select text first");
        return;
    }
    let haystack: String = slice.chunks().collect();
    let ranges = find_all_ranges(&haystack, &needle);
    if ranges.is_empty() {
        return;
    }
    let selection = Selection::new(ranges.iter().map(|&(s, e)| Range::new(s, e)).collect(), 0);
    doc.set_selection(view.id, selection);
    cx.editor
        .set_status(format!("{} matches selected", ranges.len()));
}

fn select_regex(cx: &mut Context) {
    let reg = cx.register.unwrap_or('/');
    ui::regex_prompt(
        cx,
        "select:".into(),
        Some(reg),
        ui::completers::none,
        move |cx, regex, event| {
            let (view, doc) = current!(cx.editor);
            if !matches!(event, PromptEvent::Update | PromptEvent::Validate) {
                return;
            }
            let text = doc.text().slice(..);
            if let Some(selection) =
                selection::select_on_matches(text, doc.selection(view.id), &regex)
            {
                doc.set_selection(view.id, selection);
            } else if event == PromptEvent::Validate {
                cx.editor.set_error("nothing selected");
            }
        },
    );
}

fn split_selection(cx: &mut Context) {
    let reg = cx.register.unwrap_or('/');
    ui::regex_prompt(
        cx,
        "split:".into(),
        Some(reg),
        ui::completers::none,
        move |cx, regex, event| {
            let (view, doc) = current!(cx.editor);
            if !matches!(event, PromptEvent::Update | PromptEvent::Validate) {
                return;
            }
            let text = doc.text().slice(..);
            let selection = selection::split_on_matches(text, doc.selection(view.id), &regex);
            doc.set_selection(view.id, selection);
        },
    );
}

fn split_selection_on_newline(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);
    let text = doc.text().slice(..);
    let selection = selection::split_on_newline(text, doc.selection(view.id));
    doc.set_selection(view.id, selection);
}

fn merge_selections(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);
    let selection = doc.selection(view.id).clone().merge_ranges();
    doc.set_selection(view.id, selection);
}

fn merge_consecutive_selections(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);
    let selection = doc.selection(view.id).clone().merge_consecutive_ranges();
    doc.set_selection(view.id, selection);
}

#[allow(clippy::too_many_arguments)]
fn search_impl(
    editor: &mut Editor,
    regex: &rope::Regex,
    movement: Movement,
    direction: Direction,
    scrolloff: usize,
    wrap_around: bool,
    show_warnings: bool,
) {
    let (view, doc) = current!(editor);
    let text = doc.text().slice(..);
    let selection = doc.selection(view.id);

    // Get the right side of the primary block cursor for forward search, or the
    // grapheme before the start of the selection for reverse search.
    let start = match direction {
        Direction::Forward => text.char_to_byte(graphemes::ensure_grapheme_boundary_next(
            text,
            selection.primary().to(),
        )),
        Direction::Backward => text.char_to_byte(graphemes::ensure_grapheme_boundary_prev(
            text,
            selection.primary().from(),
        )),
    };

    // A regex::Match returns byte-positions in the str. In the case where we
    // do a reverse search and wraparound to the end, we don't need to search
    // the text before the current cursor position for matches, but by slicing
    // it out, we need to add it back to the position of the selection.
    let doc = doc!(editor).text().slice(..);

    // use find_at to find the next match after the cursor, loop around the end
    // Careful, `Regex` uses `bytes` as offsets, not character indices!
    let mut mat = match direction {
        Direction::Forward => regex.find(doc.regex_input_at_bytes(start..)),
        Direction::Backward => regex.find_iter(doc.regex_input_at_bytes(..start)).last(),
    };

    if mat.is_none() {
        if wrap_around {
            mat = match direction {
                Direction::Forward => regex.find(doc.regex_input()),
                Direction::Backward => regex.find_iter(doc.regex_input_at_bytes(start..)).last(),
            };
        }
        if show_warnings {
            if wrap_around && mat.is_some() {
                editor.set_status("Wrapped around document");
            } else {
                editor.set_error("No more matches");
            }
        }
    }

    let (view, doc) = current!(editor);
    let text = doc.text().slice(..);
    let selection = doc.selection(view.id);

    if let Some(mat) = mat {
        let start = text.byte_to_char(mat.start());
        let end = text.byte_to_char(mat.end());

        if end == 0 {
            // skip empty matches that don't make sense
            return;
        }

        // Determine range direction based on the primary range
        let primary = selection.primary();
        let range = Range::new(start, end).with_direction(primary.direction());

        let selection = match movement {
            Movement::Extend => selection.clone().push(range),
            Movement::Move => selection.clone().replace(selection.primary_index(), range),
        };

        doc.set_selection(view.id, selection);
        view.ensure_cursor_in_view_center(doc, scrolloff);
    };
}

fn search_completions(cx: &mut Context, reg: Option<char>) -> Vec<String> {
    let mut items = reg
        .and_then(|reg| cx.editor.registers.read(reg, cx.editor))
        .map_or(Vec::new(), |reg| reg.take(200).collect());
    items.sort_unstable();
    items.dedup();
    items.into_iter().map(|value| value.to_string()).collect()
}

fn search(cx: &mut Context) {
    searcher(cx, Direction::Forward)
}

fn rsearch(cx: &mut Context) {
    searcher(cx, Direction::Backward)
}

fn searcher(cx: &mut Context, direction: Direction) {
    let reg = cx.register.unwrap_or('/');
    let config = cx.editor.config();
    let scrolloff = config.scrolloff;
    let wrap_around = config.search.wrap_around;
    let movement = if cx.editor.mode() == Mode::Select {
        Movement::Extend
    } else {
        Movement::Move
    };

    // TODO: could probably share with select_on_matches?
    let completions = search_completions(cx, Some(reg));

    ui::regex_prompt(
        cx,
        "search:".into(),
        Some(reg),
        move |_editor: &Editor, input: &str| {
            completions
                .iter()
                .filter(|comp| comp.starts_with(input))
                .map(|comp| (0.., comp.clone().into()))
                .collect()
        },
        move |cx, regex, event| {
            if event == PromptEvent::Validate {
                cx.editor.registers.last_search_register = reg;
            } else if event != PromptEvent::Update {
                return;
            }
            search_impl(
                cx.editor,
                &regex,
                movement,
                direction,
                scrolloff,
                wrap_around,
                false,
            );
        },
    );
}

fn search_next_or_prev_impl(cx: &mut Context, movement: Movement, direction: Direction) {
    let count = cx.count();
    let register = cx
        .register
        .unwrap_or(cx.editor.registers.last_search_register);
    let config = cx.editor.config();
    let scrolloff = config.scrolloff;
    if let Some(query) = cx.editor.registers.first(register, cx.editor) {
        let search_config = &config.search;
        let case_insensitive = if search_config.smart_case {
            !query.chars().any(char::is_uppercase)
        } else {
            false
        };
        let wrap_around = search_config.wrap_around;
        let is_crlf = doc!(cx.editor).line_ending == LineEnding::Crlf;
        if let Ok(regex) = rope::RegexBuilder::new()
            .syntax(
                rope::Config::new()
                    .case_insensitive(case_insensitive)
                    .multi_line(true)
                    .crlf(is_crlf),
            )
            .build(&query)
        {
            for _ in 0..count {
                search_impl(
                    cx.editor,
                    &regex,
                    movement,
                    direction,
                    scrolloff,
                    wrap_around,
                    true,
                );
            }
        } else {
            let error = format!("Invalid regex: {}", query);
            cx.editor.set_error(error);
        }
    }
}

fn search_next(cx: &mut Context) {
    search_next_or_prev_impl(cx, Movement::Move, Direction::Forward);
}

fn search_prev(cx: &mut Context) {
    search_next_or_prev_impl(cx, Movement::Move, Direction::Backward);
}
fn extend_search_next(cx: &mut Context) {
    search_next_or_prev_impl(cx, Movement::Extend, Direction::Forward);
}

fn extend_search_prev(cx: &mut Context) {
    search_next_or_prev_impl(cx, Movement::Extend, Direction::Backward);
}

fn search_selection(cx: &mut Context) {
    search_selection_impl(cx, false)
}

fn search_selection_detect_word_boundaries(cx: &mut Context) {
    search_selection_impl(cx, true)
}

fn search_selection_impl(cx: &mut Context, detect_word_boundaries: bool) {
    fn is_at_word_start(text: RopeSlice, index: usize) -> bool {
        // This can happen when the cursor is at the last character in
        // the document +1 (ge + j), in this case text.char(index) will panic as
        // it will index out of bounds. See https://github.com/helix-editor/helix/issues/12609
        if index == text.len_chars() {
            return false;
        }
        let ch = text.char(index);
        if index == 0 {
            return char_is_word(ch);
        }
        let prev_ch = text.char(index - 1);

        !char_is_word(prev_ch) && char_is_word(ch)
    }

    fn is_at_word_end(text: RopeSlice, index: usize) -> bool {
        if index == 0 || index == text.len_chars() {
            return false;
        }
        let ch = text.char(index);
        let prev_ch = text.char(index - 1);

        char_is_word(prev_ch) && !char_is_word(ch)
    }

    let register = cx.register.unwrap_or('/');
    let (view, doc) = current!(cx.editor);
    let text = doc.text().slice(..);

    let regex = doc
        .selection(view.id)
        .iter()
        .map(|selection| {
            let add_boundary_prefix =
                detect_word_boundaries && is_at_word_start(text, selection.from());
            let add_boundary_suffix =
                detect_word_boundaries && is_at_word_end(text, selection.to());

            let prefix = if add_boundary_prefix { "\\b" } else { "" };
            let suffix = if add_boundary_suffix { "\\b" } else { "" };

            let word = regex::escape(&selection.fragment(text));
            format!("{}{}{}", prefix, word, suffix)
        })
        .collect::<HashSet<_>>() // Collect into hashset to deduplicate identical regexes
        .into_iter()
        .collect::<Vec<_>>()
        .join("|");

    let msg = format!("register '{}' set to '{}'", register, &regex);
    match cx.editor.registers.push(register, regex) {
        Ok(_) => {
            cx.editor.registers.last_search_register = register;
            cx.editor.set_status(msg)
        }
        Err(err) => cx.editor.set_error(err.to_string()),
    }
}

fn make_search_word_bounded(cx: &mut Context) {
    // Defaults to the active search register instead `/` to be more ergonomic assuming most people
    // would use this command following `search_selection`. This avoids selecting the register
    // twice.
    let register = cx
        .register
        .unwrap_or(cx.editor.registers.last_search_register);
    let regex = match cx.editor.registers.first(register, cx.editor) {
        Some(regex) => regex,
        None => return,
    };
    let start_anchored = regex.starts_with("\\b");
    let end_anchored = regex.ends_with("\\b");

    if start_anchored && end_anchored {
        return;
    }

    let mut new_regex = String::with_capacity(
        regex.len() + if start_anchored { 0 } else { 2 } + if end_anchored { 0 } else { 2 },
    );

    if !start_anchored {
        new_regex.push_str("\\b");
    }
    new_regex.push_str(&regex);
    if !end_anchored {
        new_regex.push_str("\\b");
    }

    let msg = format!("register '{}' set to '{}'", register, &new_regex);
    match cx.editor.registers.push(register, new_regex) {
        Ok(_) => {
            cx.editor.registers.last_search_register = register;
            cx.editor.set_status(msg)
        }
        Err(err) => cx.editor.set_error(err.to_string()),
    }
}

// ---------------------------------------------------------------------------
// Region alignment (Spacemacs `SPC x a *` / evil-lion). Align every line spanned
// by the selection so the first match of a delimiter shares a column.
// ---------------------------------------------------------------------------

/// Align `lines` so the first match of `pat` in each line starts at a shared
/// column. Lines with no match are left unchanged. `right` right-justifies the
/// text before the delimiter instead of left-justifying it. Pure (tested).
fn align_lines(lines: &[String], pat: &regex::Regex, right: bool) -> Vec<String> {
    let mut parts: Vec<Option<(String, String)>> = Vec::with_capacity(lines.len());
    let mut width = 0usize;
    for line in lines {
        if let Some(mat) = pat.find(line) {
            let before = &line[..mat.start()];
            let rest = &line[mat.start()..];
            let trimmed = before.trim_end();
            width = width.max(trimmed.chars().count());
            parts.push(Some((trimmed.to_string(), rest.to_string())));
        } else {
            parts.push(None);
        }
    }
    let target = width + 1; // one space before the aligned delimiter
    lines
        .iter()
        .zip(parts)
        .map(|(orig, p)| match p {
            Some((before, rest)) => {
                let len = before.chars().count();
                let pad = " ".repeat(target.saturating_sub(len));
                if right {
                    format!("{}{}{}", pad, before, rest)
                } else {
                    format!("{}{}{}", before, pad, rest)
                }
            }
            None => orig.clone(),
        })
        .collect()
}

/// Apply `align_lines` to the lines spanned by the primary selection.
fn align_region(editor: &mut Editor, pat: regex::Regex, right: bool) {
    let (view, doc) = current!(editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let start_line = text.char_to_line(sel.from());
    let end_char = sel.to().saturating_sub(1).max(sel.from());
    let end_line = text.char_to_line(end_char);
    let le = doc.line_ending.as_str();

    let mut lines = Vec::new();
    let mut last_has_nl = false;
    for l in start_line..=end_line {
        let mut s = text.line(l).to_string();
        last_has_nl = s.ends_with('\n');
        if last_has_nl {
            s.pop();
            if s.ends_with('\r') {
                s.pop();
            }
        }
        lines.push(s);
    }
    let aligned = align_lines(&lines, &pat, right);
    if aligned == lines {
        return;
    }
    let mut out = aligned.join(le);
    if last_has_nl {
        out.push_str(le);
    }
    let from = text.line_to_char(start_line);
    let to = if last_has_nl {
        text.line_to_char(end_line + 1)
    } else {
        text.len_chars()
    };
    let transaction =
        Transaction::change(doc.text(), std::iter::once((from, to, Some(out.into()))));
    doc.apply(&transaction, view.id);
}

// --- region justification (Spacemacs `SPC x j *`, set-justification + fill) ---
#[derive(Clone, Copy)]
enum Justify {
    Left,
    Right,
    Center,
    Full,
    None,
}

fn full_justify_line(words: &[&str], width: usize) -> String {
    if words.len() < 2 {
        return words.join(" ");
    }
    let text_len: usize = words.iter().map(|w| w.chars().count()).sum();
    let gaps = words.len() - 1;
    let spaces = width.saturating_sub(text_len).max(gaps);
    let base = spaces / gaps;
    let extra = spaces % gaps;
    let mut s = String::new();
    for (i, w) in words.iter().enumerate() {
        s.push_str(w);
        if i < gaps {
            let n = base + usize::from(i < extra);
            s.push_str(&" ".repeat(n.max(1)));
        }
    }
    s
}

/// Reflow `text` to fill `width` columns, aligning each line per `mode`. Pure.
fn fill_justify(text: &str, width: usize, mode: Justify) -> String {
    let width = width.max(1);
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.is_empty() {
        return String::new();
    }
    let mut lines: Vec<Vec<&str>> = Vec::new();
    let mut cur: Vec<&str> = Vec::new();
    let mut len = 0usize;
    for w in words {
        let add = if cur.is_empty() {
            w.chars().count()
        } else {
            w.chars().count() + 1
        };
        if len + add > width && !cur.is_empty() {
            lines.push(std::mem::take(&mut cur));
            len = 0;
        }
        len += if cur.is_empty() {
            w.chars().count()
        } else {
            w.chars().count() + 1
        };
        cur.push(w);
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    let n = lines.len();
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let joined = line.join(" ");
        let pad = width.saturating_sub(joined.chars().count());
        let s = match mode {
            Justify::Left | Justify::None => joined,
            Justify::Right => format!("{}{}", " ".repeat(pad), joined),
            Justify::Center => format!("{}{}", " ".repeat(pad / 2), joined),
            Justify::Full => {
                if i + 1 == n {
                    joined
                } else {
                    full_justify_line(line, width)
                }
            }
        };
        out.push(s);
    }
    out.join("\n")
}

fn justify_region(cx: &mut Context, mode: Justify) {
    let width = {
        let w = cx.editor.config().text_width;
        if w == 0 {
            80
        } else {
            w
        }
    };
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let (from, to) = (sel.from(), sel.to().max(sel.from() + 1).min(text.len_chars()));
    let start_line = text.char_to_line(from);
    let end_line = text.char_to_line(to.saturating_sub(1).max(from));
    let lstart = text.line_to_char(start_line);
    let lend = text.line_to_char((end_line + 1).min(text.len_lines()));
    let src = text.slice(lstart..lend).to_string();
    let trailing_nl = src.ends_with('\n');
    let le = doc.line_ending.as_str();
    let mut filled = fill_justify(&src, width, mode).replace('\n', le);
    if trailing_nl {
        filled.push_str(le);
    }
    if filled == src {
        return;
    }
    let transaction =
        Transaction::change(doc.text(), std::iter::once((lstart, lend, Some(filled.into()))));
    doc.apply(&transaction, view.id);
}

fn justify_left(cx: &mut Context) { justify_region(cx, Justify::Left) }
fn justify_right(cx: &mut Context) { justify_region(cx, Justify::Right) }
fn justify_center(cx: &mut Context) { justify_region(cx, Justify::Center) }
fn justify_full(cx: &mut Context) { justify_region(cx, Justify::Full) }
fn justify_none(cx: &mut Context) { justify_region(cx, Justify::None) }

/// Count occurrences per word in the selection (Spacemacs `SPC x w c`).
fn count_words_region(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);
    let text = doc.text().slice(..);
    let frag = doc.selection(view.id).primary().fragment(text);
    let mut counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for w in frag.split_whitespace() {
        *counts.entry(w).or_insert(0) += 1;
    }
    let total: usize = counts.values().sum();
    let uniq = counts.len();
    let top = counts
        .iter()
        .max_by_key(|(_, &c)| c)
        .map(|(w, c)| format!("; most: \"{w}\"×{c}"))
        .unwrap_or_default();
    cx.editor
        .set_status(format!("{total} words, {uniq} unique{top}"));
}

/// SPC k j: move to the next closing parenthesis/bracket/brace.
fn goto_next_close_paren(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);
    let text = doc.text().slice(..);
    let pos = doc.selection(view.id).primary().cursor(text);
    let mut i = pos + 1;
    while i < text.len_chars() {
        if matches!(text.char(i), ')' | ']' | '}') {
            doc.set_selection(view.id, Selection::point(i));
            return;
        }
        i += 1;
    }
}

/// SPC k k: move to the previous opening parenthesis/bracket/brace.
fn goto_prev_open_paren(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);
    let text = doc.text().slice(..);
    let pos = doc.selection(view.id).primary().cursor(text);
    let mut i = pos;
    while i > 0 {
        i -= 1;
        if matches!(text.char(i), '(' | '[' | '{') {
            doc.set_selection(view.id, Selection::point(i));
            return;
        }
    }
}

/// SPC w 3: lay out three vertical windows.
/// Swap the text in char range `prev` with `cur` (prev must precede cur), keeping
/// the separator between them. Returns the rebuilt `prev.0..cur.1` slice. Pure.
fn swap_ranges_text(s: &str, prev: (usize, usize), cur: (usize, usize)) -> String {
    let ch: Vec<char> = s.chars().collect();
    let pt: String = ch[prev.0..prev.1].iter().collect();
    let mid: String = ch[prev.1..cur.0].iter().collect();
    let ct: String = ch[cur.0..cur.1].iter().collect();
    format!("{ct}{mid}{pt}")
}

/// Find the (previous, current) paragraph char-ranges around `cursor` in `lines`
/// terms, expressed as char offsets into the whole text. Pure (tested).
fn paragraph_ranges(
    line_starts: &[usize],
    blanks: &[bool],
    cursor_line: usize,
) -> Option<((usize, usize), (usize, usize))> {
    let n = blanks.len();
    if n == 0 || cursor_line >= n || blanks[cursor_line] {
        return None;
    }
    let mut cs = cursor_line;
    while cs > 0 && !blanks[cs - 1] {
        cs -= 1;
    }
    let mut ce = cursor_line;
    while ce + 1 < n && !blanks[ce + 1] {
        ce += 1;
    }
    if cs == 0 {
        return None;
    }
    let mut pe = cs - 1;
    while pe > 0 && blanks[pe] {
        pe -= 1;
    }
    if blanks[pe] {
        return None;
    }
    let mut ps = pe;
    while ps > 0 && !blanks[ps - 1] {
        ps -= 1;
    }
    let line_char = |l: usize| line_starts.get(l).copied();
    let pr = (line_char(ps)?, line_char(pe + 1)?);
    let cr = (line_char(cs)?, line_char(ce + 1)?);
    Some((pr, cr))
}

/// Read the s-expression ending at exclusive index `end` (skipping trailing
/// whitespace): a balanced `()[]{}` group, or an atom. Returns its char range.
fn read_sexp_back(ch: &[char], end: usize) -> Option<(usize, usize)> {
    let mut i = end.min(ch.len());
    while i > 0 && ch[i - 1].is_whitespace() {
        i -= 1;
    }
    if i == 0 {
        return None;
    }
    let end = i;
    if matches!(ch[i - 1], ')' | ']' | '}') {
        let mut depth = 0i32;
        let mut j = i;
        while j > 0 {
            j -= 1;
            match ch[j] {
                ')' | ']' | '}' => depth += 1,
                '(' | '[' | '{' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some((j, end));
                    }
                }
                _ => {}
            }
        }
        None
    } else {
        let mut j = i;
        while j > 0
            && !ch[j - 1].is_whitespace()
            && !matches!(ch[j - 1], '(' | '[' | '{' | ')' | ']' | '}')
        {
            j -= 1;
        }
        Some((j, end))
    }
}

/// The exclusive end index of the s-expression *containing* `cursor`: the close
/// of the innermost enclosing bracket group, or the end of the token at point.
fn sexp_current_end(ch: &[char], cursor: usize) -> usize {
    let n = ch.len();
    let cur = cursor.min(n);
    let close_from = |open: usize| -> usize {
        let mut dd = 0i32;
        let mut k = open;
        while k < n {
            match ch[k] {
                '(' | '[' | '{' => dd += 1,
                ')' | ']' | '}' => {
                    dd -= 1;
                    if dd == 0 {
                        return k + 1;
                    }
                }
                _ => {}
            }
            k += 1;
        }
        n
    };
    // backward scan for an unmatched opening bracket (the enclosing group)
    let mut depth = 0i32;
    let mut j = cur;
    while j > 0 {
        j -= 1;
        match ch[j] {
            ')' | ']' | '}' => depth += 1,
            '(' | '[' | '{' => {
                if depth == 0 {
                    return close_from(j);
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    // top level: end of the token (group or atom) at/after the cursor
    let mut i = cur;
    while i < n && ch[i].is_whitespace() {
        i += 1;
    }
    if i < n && matches!(ch[i], '(' | '[' | '{') {
        return close_from(i);
    }
    while i < n && !ch[i].is_whitespace() && !matches!(ch[i], '(' | '[' | '{' | ')' | ']' | '}') {
        i += 1;
    }
    i
}

/// (previous, current) s-expression ranges around `cursor`. Pure (tested).
fn sexp_pair(ch: &[char], cursor: usize) -> Option<((usize, usize), (usize, usize))> {
    let cur = read_sexp_back(ch, sexp_current_end(ch, cursor))?;
    let prev = read_sexp_back(ch, cur.0)?;
    Some((prev, cur))
}

/// (previous, current) sentence ranges around `cursor`. A sentence ends at
/// `.`/`!`/`?`. Pure (tested).
fn sentence_pair(ch: &[char], cursor: usize) -> Option<((usize, usize), (usize, usize))> {
    let is_end = |i: usize| matches!(ch.get(i), Some('.') | Some('!') | Some('?'));
    let cursor = cursor.min(ch.len().saturating_sub(1));
    // current sentence end: next terminator at/after cursor (inclusive)
    let mut ce = cursor;
    while ce < ch.len() && !is_end(ce) {
        ce += 1;
    }
    let ce = (ce + 1).min(ch.len()); // include the terminator
                                     // current start: just after the previous terminator
    let mut cs = cursor;
    while cs > 0 && !is_end(cs - 1) {
        cs -= 1;
    }
    if cs == 0 {
        return None;
    }
    // previous sentence end is the terminator just before cs
    let pe = cs;
    let mut ps = pe.saturating_sub(1);
    while ps > 0 && !is_end(ps - 1) {
        ps -= 1;
    }
    // trim leading whitespace of each sentence so the swap reads cleanly
    let trim = |mut a: usize, b: usize| {
        while a < b && ch[a].is_whitespace() {
            a += 1;
        }
        (a, b)
    };
    Some((trim(ps, pe), trim(cs, ce)))
}

fn transpose_units(cx: &mut Context, finder: fn(&[char], usize) -> Option<((usize, usize), (usize, usize))>) {
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let ch: Vec<char> = text.chars().collect();
    let cursor = doc
        .selection(view.id)
        .primary()
        .head
        .min(ch.len().saturating_sub(1));
    let Some((pr, cr)) = finder(&ch, cursor) else {
        cx.editor.set_status("nothing to transpose");
        return;
    };
    let whole: String = ch.iter().collect();
    let swapped = swap_ranges_text(&whole, pr, cr);
    let transaction =
        Transaction::change(doc.text(), std::iter::once((pr.0, cr.1, Some(swapped.into()))));
    doc.apply(&transaction, view.id);
}

/// SPC x t e: swap the current s-expression with the previous one.
fn transpose_sexp(cx: &mut Context) {
    transpose_units(cx, sexp_pair)
}

/// SPC x t s: swap the current sentence with the previous one.
fn transpose_sentence(cx: &mut Context) {
    transpose_units(cx, sentence_pair)
}

/// SPC x t p: swap the current paragraph with the previous one.
fn transpose_paragraph(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let n = text.len_lines();
    let line_starts: Vec<usize> = (0..=n).map(|l| text.line_to_char(l.min(n))).collect();
    let blanks: Vec<bool> = (0..n).map(|l| text.line(l).to_string().trim().is_empty()).collect();
    let cur_line = text.char_to_line(
        doc.selection(view.id)
            .primary()
            .head
            .min(text.len_chars().saturating_sub(1)),
    );
    let Some((pr, cr)) = paragraph_ranges(&line_starts, &blanks, cur_line) else {
        cx.editor.set_status("no previous paragraph to transpose");
        return;
    };
    let whole = text.to_string();
    let swapped = swap_ranges_text(&whole, pr, cr);
    let transaction =
        Transaction::change(doc.text(), std::iter::once((pr.0, cr.1, Some(swapped.into()))));
    doc.apply(&transaction, view.id);
}

fn make_3_windows(cx: &mut Context) {
    wonly(cx);
    vsplit(cx);
    vsplit(cx);
}

/// SPC w 4: lay out a 2x2 window grid.
fn make_4_windows(cx: &mut Context) {
    wonly(cx);
    vsplit(cx);
    hsplit(cx);
    jump_view_left(cx);
    hsplit(cx);
}

/// SPC n f: narrow the buffer to the enclosing function (≈ paragraph), folding
/// everything outside it (reuses the narrowing fold machinery).
fn narrow_to_function(cx: &mut Context) {
    let (start, end, last) = {
        let (view, doc) = current!(cx.editor);
        let text = doc.text();
        let last = text.len_lines().saturating_sub(1);
        let cur = text
            .char_to_line(doc.selection(view.id).primary().head.min(text.len_chars()))
            .min(last);
        let is_blank = |l: usize| text.line(l).to_string().trim().is_empty();
        let mut start = cur;
        while start > 0 && !is_blank(start - 1) {
            start -= 1;
        }
        let mut end = cur;
        while end < last && !is_blank(end + 1) {
            end += 1;
        }
        (start, end, last)
    };
    let (view, doc) = current!(cx.editor);
    let lo = doc.text().line_to_char(start);
    let hi = doc.text().line_to_char((end + 1).min(doc.text().len_lines()));
    for (s, e) in narrow_outside_ranges(start, end, last) {
        doc.folds_mut().create(s, e);
    }
    doc.folds_mut().clamp(last);
    doc.narrow_to(lo, hi);
    fold_goto_line(view, doc, start);
    cx.editor.set_status(format!(
        "narrowed to function (lines {}-{})",
        start + 1,
        end + 1
    ));
}

/// Pick another buffer and diff it against the current one (Spacemacs `SPC D b b`).
fn ediff_buffer(cx: &mut Context) {
    let current = view!(cx.editor).doc;

    struct BufMeta {
        id: DocumentId,
        name: String,
    }
    let items: Vec<BufMeta> = cx
        .editor
        .documents()
        .filter(|d| d.id() != current)
        .map(|d| BufMeta {
            id: d.id(),
            name: d.display_name().into_owned(),
        })
        .collect();
    if items.is_empty() {
        cx.editor.set_status("no other buffer to diff against");
        return;
    }

    let columns = [PickerColumn::new("buffer", |m: &BufMeta, _: &()| m.name.clone().into())];
    let picker = Picker::new(columns, 0, items, (), move |cx, meta, _action| {
        let other = meta.id;
        let g = |cx: &compositor::Context, id: DocumentId| {
            cx.editor
                .documents()
                .find(|d| d.id() == id)
                .map(|d| (d.display_name().into_owned(), d.text().to_string()))
        };
        if let (Some((na, ta)), Some((nb, tb))) = (g(cx, current), g(cx, other)) {
            let view =
                crate::ui::merge::DiffView::new(format!("{na} ⇔ {nb}"), other, &ta, &tb).read_only();
            let call = crate::job::Callback::EditorCompositor(Box::new(
                move |_editor: &mut Editor, compositor: &mut crate::compositor::Compositor| {
                    compositor.push(Box::new(view));
                },
            ));
            cx.jobs.callback(async move { Ok(call) });
        }
    });
    cx.push_layer(Box::new(overlaid(picker)));
}

/// Compare the two front windows' buffers side by side (Spacemacs `SPC D w w/l`).
fn ediff_windows(cx: &mut Context) {
    let docs: Vec<DocumentId> = cx.editor.tree.traverse().map(|(_, v)| v.doc).collect();
    if docs.len() < 2 {
        cx.editor.set_status("ediff needs two windows");
        return;
    }
    let get = |cx: &Context, id: DocumentId| {
        cx.editor
            .documents()
            .find(|d| d.id() == id)
            .map(|d| (d.display_name().into_owned(), d.text().to_string()))
    };
    let (Some((na, ta)), Some((nb, tb))) = (get(cx, docs[0]), get(cx, docs[1])) else {
        return;
    };
    let view =
        crate::ui::merge::DiffView::new(format!("{na} ⇔ {nb}"), docs[1], &ta, &tb).read_only();
    cx.push_layer(Box::new(view));
}

fn align_region_lit(cx: &mut Context, delim: &str, right: bool) {
    match regex::Regex::new(&regex::escape(delim)) {
        Ok(re) => align_region(cx.editor, re, right),
        Err(_) => {}
    }
}

/// SPC x a r: align the region at a user-specified regexp.
fn align_at_regex(cx: &mut Context) {
    let prompt = crate::ui::prompt::Prompt::new(
        "align at regexp:".into(),
        None,
        ui::completers::none,
        move |cx: &mut crate::compositor::Context, input: &str, ev: PromptEvent| {
            if ev != PromptEvent::Validate || input.is_empty() {
                return;
            }
            match regex::Regex::new(input) {
                Ok(re) => align_region(cx.editor, re, false),
                Err(e) => cx.editor.set_error(format!("bad regex: {e}")),
            }
        },
    );
    cx.push_layer(Box::new(prompt));
}

fn align_at_equals(cx: &mut Context) { align_region_lit(cx, "=", false) }
fn align_at_comma(cx: &mut Context) { align_region_lit(cx, ",", false) }
fn align_at_colon(cx: &mut Context) { align_region_lit(cx, ":", false) }
fn align_at_semicolon(cx: &mut Context) { align_region_lit(cx, ";", false) }
fn align_at_ampersand(cx: &mut Context) { align_region_lit(cx, "&", false) }
fn align_at_lparen(cx: &mut Context) { align_region_lit(cx, "(", false) }
fn align_at_rparen(cx: &mut Context) { align_region_lit(cx, ")", false) }
fn align_at_lbracket(cx: &mut Context) { align_region_lit(cx, "[", false) }
fn align_at_rbracket(cx: &mut Context) { align_region_lit(cx, "]", false) }
fn align_at_lbrace(cx: &mut Context) { align_region_lit(cx, "{", false) }
fn align_at_rbrace(cx: &mut Context) { align_region_lit(cx, "}", false) }
fn align_at_dot(cx: &mut Context) { align_region_lit(cx, ".", false) }

/// Align at arithmetic operators `+ - * /` (Spacemacs `SPC x a m`).
fn align_at_arithmetic(cx: &mut Context) {
    if let Ok(re) = regex::Regex::new(r"[-+*/]") {
        align_region(cx.editor, re, false);
    }
}

/// Left-align at a delimiter typed next (evil-lion `gl`, Spacemacs `SPC x a l`).
fn align_left_at_char(cx: &mut Context) {
    cx.on_next_key(move |cx, event| {
        if let KeyEvent { code: KeyCode::Char(ch), .. } = event {
            align_region_lit(cx, &ch.to_string(), false);
        }
    });
}

/// Right-align at a delimiter typed next (evil-lion `gL`, Spacemacs `SPC x a L`).
fn align_right_at_char(cx: &mut Context) {
    cx.on_next_key(move |cx, event| {
        if let KeyEvent { code: KeyCode::Char(ch), .. } = event {
            align_region_lit(cx, &ch.to_string(), true);
        }
    });
}

// ---------------------------------------------------------------------------
// Window-by-number (Spacemacs `SPC 1`..`SPC 9` / window-numbering-mode). Windows
// are numbered 1..N in the tree's traversal order; jump straight to the Nth.
// ---------------------------------------------------------------------------

fn goto_window_n(cx: &mut Context, n: usize) {
    if n == 0 {
        return;
    }
    let ids: Vec<_> = cx.editor.tree.traverse().map(|(id, _)| id).collect();
    if let Some(&id) = ids.get(n - 1) {
        cx.editor.focus(id);
    }
}

/// SPC g f m : a magit-style "file dispatch" transient — press a key to run a git operation on the
/// current file: [s]tage, [u]nstage, [d]iff vs HEAD, [l]og, [b]lame, [g] status. Spacemacs
/// `magit-file-dispatch`.
fn git_file_dispatch(cx: &mut Context) {
    cx.editor.set_status(
        "git file: [s]tage [u]nstage [d]iff [l]og [b]lame [g]status  (any other key: cancel)",
    );
    cx.on_next_key(move |cx, event| match event.code {
        KeyCode::Char('d') => git_diff(cx),
        KeyCode::Char('l') => git_file_log_picker(cx),
        KeyCode::Char('b') => git_blame_line(cx),
        KeyCode::Char('g') => git_status(cx),
        KeyCode::Char('s') | KeyCode::Char('u') => {
            let stage = matches!(event.code, KeyCode::Char('s'));
            let args: &[&str] = if stage {
                &["add"]
            } else {
                &["reset", "-q", "HEAD"]
            };
            let verb = if stage { "staged" } else { "unstaged" };
            let mut bridge = crate::compositor::Context {
                editor: cx.editor,
                jobs: cx.jobs,
                scroll: None,
            };
            if let Err(e) = typed::git_on_current_file(&mut bridge, args, verb) {
                bridge.editor.set_error(e.to_string());
            }
        }
        _ => cx.editor.set_status("git file: cancelled"),
    });
}

/// SPC w . a : ace-window. Show a hint, then jump to the window whose 1-based number is pressed
/// (windows are numbered in traversal order, matching `SPC w 1..9`). Spacemacs `ace-window`.
fn ace_window(cx: &mut Context) {
    let n = cx.editor.tree.traverse().count();
    if n <= 1 {
        cx.editor.set_status("ace-window: only one window");
        return;
    }
    cx.editor.set_status(format!(
        "ace-window: press 1-{} to select a window",
        n.min(9)
    ));
    cx.on_next_key(move |cx, event| {
        if let KeyCode::Char(c) = event.code {
            if let Some(d) = c.to_digit(10) {
                if d >= 1 {
                    goto_window_n(cx, d as usize);
                    return;
                }
            }
        }
        cx.editor.set_status("ace-window: cancelled");
    });
}

/// Move the current buffer into window N and focus there (Spacemacs `SPC b . 1..9`).
fn buffer_to_window_n(cx: &mut Context, n: usize) {
    if n == 0 {
        return;
    }
    let doc_id = doc!(cx.editor).id();
    let ids: Vec<_> = cx.editor.tree.traverse().map(|(id, _)| id).collect();
    if let Some(&id) = ids.get(n - 1) {
        cx.editor.focus(id);
        cx.editor.switch(doc_id, Action::Replace);
    }
}
/// Swap the current buffer with the one in window N (Spacemacs `SPC b . M-1..9`).
fn buffer_swap_window_n(cx: &mut Context, n: usize) {
    if n == 0 {
        return;
    }
    let cur_view = cx.editor.tree.focus;
    let cur_doc = doc!(cx.editor).id();
    let views: Vec<ViewId> = cx.editor.tree.traverse().map(|(id, _)| id).collect();
    let Some(&target_view) = views.get(n - 1) else {
        return;
    };
    if target_view == cur_view {
        return;
    }
    let target_doc = cx.editor.tree.get(target_view).doc;
    cx.editor.focus(cur_view);
    cx.editor.switch(target_doc, Action::Replace);
    cx.editor.focus(target_view);
    cx.editor.switch(cur_doc, Action::Replace);
    cx.editor.focus(cur_view);
}
fn buffer_swap_window_1(cx: &mut Context) { buffer_swap_window_n(cx, 1) }
fn buffer_swap_window_2(cx: &mut Context) { buffer_swap_window_n(cx, 2) }
fn buffer_swap_window_3(cx: &mut Context) { buffer_swap_window_n(cx, 3) }
fn buffer_swap_window_4(cx: &mut Context) { buffer_swap_window_n(cx, 4) }
fn buffer_swap_window_5(cx: &mut Context) { buffer_swap_window_n(cx, 5) }
fn buffer_swap_window_6(cx: &mut Context) { buffer_swap_window_n(cx, 6) }
fn buffer_swap_window_7(cx: &mut Context) { buffer_swap_window_n(cx, 7) }
fn buffer_swap_window_8(cx: &mut Context) { buffer_swap_window_n(cx, 8) }
fn buffer_swap_window_9(cx: &mut Context) { buffer_swap_window_n(cx, 9) }

fn buffer_to_window_1(cx: &mut Context) { buffer_to_window_n(cx, 1) }
fn buffer_to_window_2(cx: &mut Context) { buffer_to_window_n(cx, 2) }
fn buffer_to_window_3(cx: &mut Context) { buffer_to_window_n(cx, 3) }
fn buffer_to_window_4(cx: &mut Context) { buffer_to_window_n(cx, 4) }
fn buffer_to_window_5(cx: &mut Context) { buffer_to_window_n(cx, 5) }
fn buffer_to_window_6(cx: &mut Context) { buffer_to_window_n(cx, 6) }
fn buffer_to_window_7(cx: &mut Context) { buffer_to_window_n(cx, 7) }
fn buffer_to_window_8(cx: &mut Context) { buffer_to_window_n(cx, 8) }
fn buffer_to_window_9(cx: &mut Context) { buffer_to_window_n(cx, 9) }

fn goto_window_1(cx: &mut Context) { goto_window_n(cx, 1) }
fn goto_window_2(cx: &mut Context) { goto_window_n(cx, 2) }
fn goto_window_3(cx: &mut Context) { goto_window_n(cx, 3) }
fn goto_window_4(cx: &mut Context) { goto_window_n(cx, 4) }
fn goto_window_5(cx: &mut Context) { goto_window_n(cx, 5) }
fn goto_window_6(cx: &mut Context) { goto_window_n(cx, 6) }
fn goto_window_7(cx: &mut Context) { goto_window_n(cx, 7) }
fn goto_window_8(cx: &mut Context) { goto_window_n(cx, 8) }
fn goto_window_9(cx: &mut Context) { goto_window_n(cx, 9) }

/// Close the current window and kill its buffer (Spacemacs `SPC w . x`).
fn delete_window_and_buffer(cx: &mut Context) {
    let view_id = view!(cx.editor).id;
    let doc_id = doc!(cx.editor).id();
    let _ = cx.editor.close_document(doc_id, false);
    if cx.editor.tree.views().count() > 1 {
        cx.editor.close(view_id);
    }
}

// ---------------------------------------------------------------------------
// Layouts / workspaces (Spacemacs `SPC l`). A layout is a named window
// configuration: the set of files open across the splits plus which one is
// focused. Switching saves the current layout's state and restores the target's
// (closes extra splits, reopens the saved files side-by-side, refocuses).
// ---------------------------------------------------------------------------

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct WorkLayout {
    name: String,
    files: Vec<std::path::PathBuf>,
    focused: usize,
}

struct LayoutStore {
    layouts: Vec<WorkLayout>,
    current: usize,
    last: usize,
}

static LAYOUTS: std::sync::Mutex<LayoutStore> = std::sync::Mutex::new(LayoutStore {
    layouts: Vec::new(),
    current: 0,
    last: 0,
});

fn layouts_file() -> std::path::PathBuf {
    zemacs_loader::config_dir().join("layouts.json")
}

/// Snapshot the current window configuration: file paths in tree order + the
/// index of the focused file (scratch buffers without a path are skipped).
fn layout_capture(cx: &mut Context) -> (Vec<std::path::PathBuf>, usize) {
    let focus = cx.editor.tree.focus;
    let pairs: Vec<(ViewId, DocumentId)> =
        cx.editor.tree.traverse().map(|(id, v)| (id, v.doc)).collect();
    let mut files = Vec::new();
    let mut focused = 0;
    for (id, doc_id) in pairs {
        let path = cx
            .editor
            .documents()
            .find(|d| d.id() == doc_id)
            .and_then(|d| d.path().map(|p| p.to_path_buf()));
        if let Some(p) = path {
            if id == focus {
                focused = files.len();
            }
            files.push(p);
        }
    }
    (files, focused)
}

/// Restore a window configuration: reduce to one window, reopen each file (first
/// replacing the current view, the rest as vertical splits), then refocus.
fn layout_restore(cx: &mut Context, files: &[std::path::PathBuf], focused: usize) {
    if files.is_empty() {
        return;
    }
    let others: Vec<ViewId> = cx
        .editor
        .tree
        .views()
        .filter(|(_, f)| !f)
        .map(|(v, _)| v.id)
        .collect();
    for id in others {
        cx.editor.close(id);
    }
    for (i, p) in files.iter().enumerate() {
        let action = if i == 0 {
            Action::Replace
        } else {
            Action::VerticalSplit
        };
        let _ = cx.editor.open(p, action);
    }
    let ids: Vec<_> = cx.editor.tree.traverse().map(|(id, _)| id).collect();
    if let Some(&id) = ids.get(focused) {
        cx.editor.focus(id);
    }
}

/// SPC l l: capture the current windows as a new named layout.
fn layout_create(cx: &mut Context) {
    let (files, focused) = layout_capture(cx);
    let name = {
        let mut s = LAYOUTS.lock().unwrap();
        let n = s.layouts.len() + 1;
        let name = format!("@{n}");
        s.layouts.push(WorkLayout {
            name: name.clone(),
            files,
            focused,
        });
        s.last = s.current;
        s.current = s.layouts.len() - 1;
        name
    };
    cx.editor.set_status(format!("created layout {name}"));
}

/// Save the current windows into the active layout, then restore layout `target`.
fn layout_switch(cx: &mut Context, target: usize) {
    let (files, focused) = layout_capture(cx);
    let restore = {
        let mut s = LAYOUTS.lock().unwrap();
        if s.layouts.is_empty() {
            None
        } else {
            let cur = s.current.min(s.layouts.len() - 1);
            s.layouts[cur].files = files;
            s.layouts[cur].focused = focused;
            s.last = cur;
            let t = target % s.layouts.len();
            s.current = t;
            Some(s.layouts[t].clone())
        }
    };
    match restore {
        Some(l) => {
            layout_restore(cx, &l.files, l.focused);
            cx.editor.set_status(format!("layout {}", l.name));
        }
        None => cx.editor.set_status("no layouts — SPC l l to create one"),
    }
}

fn layout_next(cx: &mut Context) {
    let t = {
        let s = LAYOUTS.lock().unwrap();
        if s.layouts.is_empty() {
            0
        } else {
            (s.current + 1) % s.layouts.len()
        }
    };
    layout_switch(cx, t);
}

fn layout_prev(cx: &mut Context) {
    let t = {
        let s = LAYOUTS.lock().unwrap();
        if s.layouts.is_empty() {
            0
        } else {
            (s.current + s.layouts.len() - 1) % s.layouts.len()
        }
    };
    layout_switch(cx, t);
}

fn layout_last(cx: &mut Context) {
    let t = LAYOUTS.lock().unwrap().last;
    layout_switch(cx, t);
}

fn layout_default(cx: &mut Context) {
    layout_switch(cx, 0);
}

fn layout_goto_n(cx: &mut Context, n: usize) {
    if n >= 1 {
        layout_switch(cx, n - 1);
    }
}
fn layout_goto_1(cx: &mut Context) { layout_goto_n(cx, 1) }
fn layout_goto_2(cx: &mut Context) { layout_goto_n(cx, 2) }
fn layout_goto_3(cx: &mut Context) { layout_goto_n(cx, 3) }
fn layout_goto_4(cx: &mut Context) { layout_goto_n(cx, 4) }
fn layout_goto_5(cx: &mut Context) { layout_goto_n(cx, 5) }
fn layout_goto_6(cx: &mut Context) { layout_goto_n(cx, 6) }
fn layout_goto_7(cx: &mut Context) { layout_goto_n(cx, 7) }
fn layout_goto_8(cx: &mut Context) { layout_goto_n(cx, 8) }
fn layout_goto_9(cx: &mut Context) { layout_goto_n(cx, 9) }

/// SPC l d / l x: delete the current layout (keeps the open buffers).
fn layout_delete(cx: &mut Context) {
    let msg = {
        let mut s = LAYOUTS.lock().unwrap();
        if s.layouts.is_empty() {
            "no layouts to delete".to_string()
        } else {
            let c = s.current.min(s.layouts.len() - 1);
            let name = s.layouts.remove(c).name;
            if !s.layouts.is_empty() && s.current >= s.layouts.len() {
                s.current = s.layouts.len() - 1;
            }
            format!("deleted layout {name}")
        }
    };
    cx.editor.set_status(msg);
}

/// SPC l s: persist all layouts to `<config>/layouts.json`.
fn layout_save(cx: &mut Context) {
    let json = {
        let s = LAYOUTS.lock().unwrap();
        serde_json::to_string_pretty(&s.layouts)
    };
    match json {
        Ok(j) => match std::fs::write(layouts_file(), j) {
            Ok(_) => cx.editor.set_status("saved layouts"),
            Err(e) => cx.editor.set_error(format!("save layouts: {e}")),
        },
        Err(e) => cx.editor.set_error(format!("serialize layouts: {e}")),
    }
}

/// SPC l L: load layouts from `<config>/layouts.json`.
fn layout_load(cx: &mut Context) {
    match std::fs::read_to_string(layouts_file()) {
        Ok(j) => match serde_json::from_str::<Vec<WorkLayout>>(&j) {
            Ok(v) => {
                let n = v.len();
                let mut s = LAYOUTS.lock().unwrap();
                s.layouts = v;
                s.current = 0;
                s.last = 0;
                drop(s);
                cx.editor.set_status(format!("loaded {n} layouts"));
            }
            Err(e) => cx.editor.set_error(format!("parse layouts: {e}")),
        },
        Err(e) => cx.editor.set_error(format!("no layouts file: {e}")),
    }
}

// ---------------------------------------------------------------------------
// Runtime config toggles (Spacemacs `SPC t *` minor-mode toggles). Each clones
// the live Config, mutates it, and pushes a ConfigEvent::Update so the change
// takes effect immediately — honest because every toggle has a visible effect.
// ---------------------------------------------------------------------------

fn edit_live_config(cx: &mut Context, f: impl FnOnce(&mut zemacs_view::editor::Config)) {
    let mut config = (*cx.editor.config()).clone();
    f(&mut config);
    let _ = cx
        .editor
        .config_events
        .0
        .send(zemacs_view::editor::ConfigEvent::Update(Box::new(config)));
}

fn toggle_statusline_element(cx: &mut Context, el: zemacs_view::editor::StatusLineElement) {
    let mut shown = false;
    edit_live_config(cx, |c| {
        let seg = &mut c.statusline.right;
        if let Some(p) = seg.iter().position(|e| *e == el) {
            seg.remove(p);
        } else {
            seg.push(el);
            shown = true;
        }
    });
    cx.editor.set_status(format!(
        "mode-line {:?}: {}",
        el,
        if shown { "shown" } else { "hidden" }
    ));
}

/// SPC t m p: toggle the cursor position in the mode line.
fn toggle_modeline_position(cx: &mut Context) {
    toggle_statusline_element(cx, zemacs_view::editor::StatusLineElement::Position)
}

/// SPC t m v: toggle the version-control info in the mode line.
fn toggle_modeline_vcs(cx: &mut Context) {
    toggle_statusline_element(cx, zemacs_view::editor::StatusLineElement::VersionControl)
}

/// SPC t - / t C--: keep the cursor vertically centered (large scrolloff).
fn toggle_centered_cursor(cx: &mut Context) {
    let mut on = false;
    edit_live_config(cx, |c| {
        if c.scrolloff >= 9999 {
            c.scrolloff = 5;
        } else {
            c.scrolloff = 9999;
            on = true;
        }
    });
    cx.editor
        .set_status(format!("centered-cursor: {}", if on { "on" } else { "off" }));
}

/// SPC t f: toggle a fill-column ruler at `text-width` (default 80).
fn toggle_fill_column(cx: &mut Context) {
    let mut on = false;
    edit_live_config(cx, |c| {
        if c.rulers.is_empty() {
            let w = if c.text_width == 0 { 80 } else { c.text_width as u16 };
            c.rulers = vec![w];
            on = true;
        } else {
            c.rulers.clear();
        }
    });
    cx.editor
        .set_status(format!("fill-column ruler: {}", if on { "on" } else { "off" }));
}

/// SPC t 8 / t C-8: toggle a ruler highlighting the 80th column.
fn toggle_long_line_marker(cx: &mut Context) {
    let mut on = false;
    edit_live_config(cx, |c| {
        if c.rulers.contains(&80) {
            c.rulers.retain(|&r| r != 80);
        } else {
            c.rulers.push(80);
            on = true;
        }
    });
    cx.editor
        .set_status(format!("long-line marker (col 80): {}", if on { "on" } else { "off" }));
}

/// Toggle soft-wrapping of long lines (IntelliJ "View > Active Editor > Soft-Wrap").
fn toggle_soft_wrap(cx: &mut Context) {
    let mut on = false;
    edit_live_config(cx, |c| {
        let enabled = c.soft_wrap.enable.unwrap_or(false);
        on = !enabled;
        c.soft_wrap.enable = Some(on);
    });
    cx.editor
        .set_status(format!("soft-wrap: {}", if on { "on" } else { "off" }));
}

/// Toggle rendering of whitespace characters (IntelliJ "View > Active Editor > Show Whitespaces").
fn toggle_whitespace_render(cx: &mut Context) {
    use zemacs_view::editor::{WhitespaceRender, WhitespaceRenderValue};
    let mut on = false;
    edit_live_config(cx, |c| {
        let showing = !matches!(c.whitespace.render.space(), WhitespaceRenderValue::None);
        on = !showing;
        c.whitespace.render = WhitespaceRender::Basic(if on {
            WhitespaceRenderValue::All
        } else {
            WhitespaceRenderValue::None
        });
    });
    cx.editor
        .set_status(format!("show whitespace: {}", if on { "on" } else { "off" }));
}

/// Prompt for a file and diff it against the current buffer (Spacemacs `SPC D f f`).
fn ediff_file(cx: &mut Context) {
    let (cur_name, cur_text, cur_id) = {
        let doc = doc!(cx.editor);
        (doc.display_name().into_owned(), doc.text().to_string(), doc.id())
    };
    let prompt = crate::ui::prompt::Prompt::new(
        "ediff with file:".into(),
        None,
        ui::completers::filename,
        move |cx: &mut crate::compositor::Context, input: &str, event: PromptEvent| {
            if event != PromptEvent::Validate || input.trim().is_empty() {
                return;
            }
            let path = std::path::PathBuf::from(input.trim());
            let other = match std::fs::read_to_string(&path) {
                Ok(s) => s,
                Err(e) => {
                    cx.editor.set_error(format!("read {}: {e}", path.display()));
                    return;
                }
            };
            let view = crate::ui::merge::DiffView::new(
                format!("{} ⇔ {}", path.display(), cur_name),
                cur_id,
                &other,
                &cur_text,
            )
            .read_only();
            let call = crate::job::Callback::EditorCompositor(Box::new(
                move |_editor: &mut Editor, compositor: &mut crate::compositor::Compositor| {
                    compositor.push(Box::new(view));
                },
            ));
            cx.jobs.callback(async move { Ok(call) });
        },
    );
    cx.push_layer(Box::new(prompt));
}

/// 3-way ediff: prompt for three space-separated file paths (A B C, where C is
/// the common ancestor) and show a read-only diff3 comparison (Spacemacs
/// `SPC D f 3`). Comparison-only — never writes back, so it's safe on arbitrary
/// external files.
/// 3-way ediff over three open buffers (names, space-separated; C = ancestor),
/// shown read-only (Spacemacs `SPC D b 3`).
fn ediff_3_buffers(cx: &mut Context) {
    let cur_id = doc!(cx.editor).id();
    let prompt = crate::ui::prompt::Prompt::new(
        "ediff 3 buffers (A B C):".into(),
        None,
        ui::completers::buffer,
        move |cx: &mut crate::compositor::Context, input: &str, ev: PromptEvent| {
            if ev != PromptEvent::Validate {
                return;
            }
            let names: Vec<&str> = input.split_whitespace().collect();
            if names.len() != 3 {
                cx.editor
                    .set_error("need exactly three space-separated buffer names");
                return;
            }
            let mut texts = Vec::with_capacity(3);
            for nm in &names {
                let found = cx
                    .editor
                    .documents()
                    .find(|d| d.display_name().as_ref() == *nm)
                    .map(|d| d.text().to_string());
                match found {
                    Some(t) => texts.push(t),
                    None => {
                        cx.editor.set_error(format!("no open buffer named {nm}"));
                        return;
                    }
                }
            }
            let segs = crate::ui::merge::diff3(&texts[2], &texts[0], &texts[1]);
            let view = crate::ui::merge::DiffView::from_conflicts(
                format!("ediff3: {}", names.join(" ")),
                cur_id,
                None,
                segs,
            )
            .read_only();
            let call = crate::job::Callback::EditorCompositor(Box::new(
                move |_e: &mut Editor, comp: &mut crate::compositor::Compositor| {
                    comp.push(Box::new(view));
                },
            ));
            cx.jobs.callback(async move { Ok(call) });
        },
    );
    cx.push_layer(Box::new(prompt));
}

fn ediff_3_files(cx: &mut Context) {
    let cur_id = doc!(cx.editor).id();
    let prompt = crate::ui::prompt::Prompt::new(
        "ediff 3 files (A B C):".into(),
        None,
        ui::completers::filename,
        move |cx: &mut crate::compositor::Context, input: &str, ev: PromptEvent| {
            if ev != PromptEvent::Validate {
                return;
            }
            let paths: Vec<&str> = input.split_whitespace().collect();
            if paths.len() != 3 {
                cx.editor
                    .set_error("need exactly three space-separated file paths");
                return;
            }
            let mut texts = Vec::with_capacity(3);
            for p in &paths {
                match std::fs::read_to_string(p) {
                    Ok(s) => texts.push(s),
                    Err(e) => {
                        cx.editor.set_error(format!("read {p}: {e}"));
                        return;
                    }
                }
            }
            // C (ancestor) is the diff3 base; A=ours, B=theirs.
            let segs = crate::ui::merge::diff3(&texts[2], &texts[0], &texts[1]);
            let view = crate::ui::merge::DiffView::from_conflicts(
                format!("ediff3: {} {} {}", paths[0], paths[1], paths[2]),
                cur_id,
                None,
                segs,
            )
            .read_only();
            let call = crate::job::Callback::EditorCompositor(Box::new(
                move |_e: &mut Editor, comp: &mut crate::compositor::Compositor| {
                    comp.push(Box::new(view));
                },
            ));
            cx.jobs.callback(async move { Ok(call) });
        },
    );
    cx.push_layer(Box::new(prompt));
}

/// Kill all buffers whose name matches a prompted regex (Spacemacs `SPC b M`).
fn kill_buffers_by_regex(cx: &mut Context) {
    let prompt = crate::ui::prompt::Prompt::new(
        "kill buffers matching:".into(),
        None,
        ui::completers::none,
        move |cx: &mut crate::compositor::Context, input: &str, event: PromptEvent| {
            if event != PromptEvent::Validate || input.trim().is_empty() {
                return;
            }
            let re = match regex::Regex::new(input.trim()) {
                Ok(r) => r,
                Err(e) => {
                    cx.editor.set_error(format!("bad regex: {e}"));
                    return;
                }
            };
            let current = view!(cx.editor).doc;
            let ids: Vec<DocumentId> = cx
                .editor
                .documents()
                .filter(|d| d.id() != current && re.is_match(&d.display_name()))
                .map(|d| d.id())
                .collect();
            let n = ids.len();
            for id in ids {
                let _ = cx.editor.close_document(id, false);
            }
            cx.editor.set_status(format!("killed {n} buffer(s)"));
        },
    );
    cx.push_layer(Box::new(prompt));
}

/// Narrow the buffer to the current page (between form-feed `^L` lines), folding
/// everything outside it (Spacemacs `SPC n p`).
fn narrow_to_page(cx: &mut Context) {
    let (start, end, last) = {
        let (view, doc) = current!(cx.editor);
        let text = doc.text();
        let last = text.len_lines().saturating_sub(1);
        let cur = text
            .char_to_line(doc.selection(view.id).primary().head.min(text.len_chars()))
            .min(last);
        let is_ff = |l: usize| text.line(l).to_string().contains('\u{0c}');
        let mut start = cur;
        while start > 0 && !is_ff(start - 1) {
            start -= 1;
        }
        let mut end = cur;
        while end < last && !is_ff(end + 1) {
            end += 1;
        }
        (start, end, last)
    };
    let (view, doc) = current!(cx.editor);
    let lo = doc.text().line_to_char(start);
    let hi = doc.text().line_to_char((end + 1).min(doc.text().len_lines()));
    for (s, e) in narrow_outside_ranges(start, end, last) {
        doc.folds_mut().create(s, e);
    }
    doc.folds_mut().clamp(last);
    doc.narrow_to(lo, hi);
    fold_goto_line(view, doc, start);
    cx.editor
        .set_status(format!("narrowed to page (lines {}-{})", start + 1, end + 1));
}

/// Copy the current file to a prompted destination (Spacemacs `SPC f c`).
fn copy_file(cx: &mut Context) {
    let Some(src) = doc!(cx.editor).path().map(|p| p.to_path_buf()) else {
        cx.editor.set_error("buffer has no file path");
        return;
    };
    let prompt = crate::ui::prompt::Prompt::new(
        "copy to:".into(),
        None,
        ui::completers::filename,
        move |cx: &mut crate::compositor::Context, input: &str, event: PromptEvent| {
            if event != PromptEvent::Validate || input.trim().is_empty() {
                return;
            }
            let dest = std::path::PathBuf::from(input.trim());
            match std::fs::copy(&src, &dest) {
                Ok(_) => cx.editor.set_status(format!("copied to {}", dest.display())),
                Err(e) => cx.editor.set_error(format!("copy failed: {e}")),
            }
        },
    );
    cx.push_layer(Box::new(prompt));
}

/// SPC f A : open a prompted file and replace the current buffer with it, closing the old buffer
/// (Spacemacs `spacemacs/find-file-and-replace-buffer`).
fn find_file_replace_buffer(cx: &mut Context) {
    let old_id = doc!(cx.editor).id();
    let prompt = crate::ui::prompt::Prompt::new(
        "find file (replace buffer):".into(),
        None,
        ui::completers::filename,
        move |cx: &mut crate::compositor::Context, input: &str, event: PromptEvent| {
            if event != PromptEvent::Validate || input.trim().is_empty() {
                return;
            }
            let path = std::path::PathBuf::from(input.trim());
            match cx.editor.open(&path, Action::Replace) {
                Ok(new_id) => {
                    // Only retire the previous buffer if we actually switched away from it
                    // and it has no unsaved changes (force=false leaves a dirty buffer alone).
                    if new_id != old_id {
                        let _ = cx.editor.close_document(old_id, false);
                    }
                }
                Err(e) => cx.editor.set_error(format!("open {}: {e}", path.display())),
            }
        },
    );
    cx.push_layer(Box::new(prompt));
}

/// Open a fresh timestamped junk file under `<config>/junk/` (Spacemacs `SPC f J`).
fn open_junk_file(cx: &mut Context) {
    let dir = zemacs_loader::config_dir().join("junk");
    let _ = std::fs::create_dir_all(&dir);
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let path = dir.join(format!("junk-{ts}.txt"));
    let _ = std::fs::write(&path, b"");
    match cx.editor.open(&path, Action::Replace) {
        Ok(_) => cx
            .editor
            .set_status(format!("junk file: {}", path.display())),
        Err(e) => cx.editor.set_error(format!("open junk file: {e}")),
    }
}

/// Open the current buffer's bytes in the hex editor (Spacemacs `SPC f h`, hexl).
fn open_hex(cx: &mut Context) {
    let (name, path, bytes) = {
        let doc = doc!(cx.editor);
        (
            doc.display_name().into_owned(),
            doc.path().map(|p| p.to_path_buf()),
            doc.text().to_string().into_bytes(),
        )
    };
    cx.push_layer(Box::new(crate::ui::hex::HexView::new(name, path, bytes)));
}

/// Open the current file with the OS default program (Spacemacs `SPC f o`).
fn open_file_external(cx: &mut Context) {
    let path = doc!(cx.editor).path().map(|p| p.to_path_buf());
    match path {
        Some(p) => match open_in_browser(&p.to_string_lossy()) {
            Ok(()) => cx
                .editor
                .set_status(format!("Opening {} externally", p.display())),
            Err(e) => cx.editor.set_error(e),
        },
        None => cx.editor.set_error("buffer has no file path"),
    }
}

/// SPC f l : open a prompted file with no language/syntax applied — the equivalent of Emacs'
/// `find-file-literally` / fundamental-mode (raw text, no highlighting or language servers).
fn open_file_literally(cx: &mut Context) {
    let prompt = crate::ui::prompt::Prompt::new(
        "find file literally:".into(),
        None,
        ui::completers::filename,
        move |cx: &mut crate::compositor::Context, input: &str, event: PromptEvent| {
            if event != PromptEvent::Validate || input.trim().is_empty() {
                return;
            }
            let path = std::path::PathBuf::from(input.trim());
            match cx.editor.open(&path, Action::Replace) {
                Ok(id) => {
                    let loader = cx.editor.syn_loader.load();
                    if let Some(doc) = cx.editor.document_mut(id) {
                        doc.set_language(None, &loader);
                    }
                    cx.editor
                        .set_status(format!("opened literally (no syntax): {}", path.display()));
                }
                Err(e) => cx.editor.set_error(format!("open {}: {e}", path.display())),
            }
        },
    );
    cx.push_layer(Box::new(prompt));
}

/// SPC f L : fuzzy "locate" picker. Runs the system `locate` against the typed query (falling
/// back to macOS Spotlight's `mdfind -name` when the locate database is unavailable) and opens
/// the chosen path — Spacemacs `helm-locate`.
fn locate_file(cx: &mut Context) {
    let columns = [PickerColumn::new("path", |item: &PathBuf, _: &()| {
        item.display().to_string().into()
    })];

    let get_files = |query: &str,
                     _editor: &mut Editor,
                     _config: std::sync::Arc<()>,
                     injector: &ui::picker::Injector<PathBuf, ()>| {
        let query = query.trim().to_owned();
        if query.is_empty() {
            return async { Ok(()) }.boxed();
        }
        let injector = injector.clone();
        async move {
            let run = |prog: &str, args: &[&str]| {
                std::process::Command::new(prog)
                    .args(args)
                    .output()
                    .ok()
                    .filter(|o| o.status.success())
                    .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            };
            let out = run("locate", &["-l", "200", &query])
                .filter(|s| !s.trim().is_empty())
                .or_else(|| run("mdfind", &["-name", &query]));
            if let Some(out) = out {
                for line in out.lines().take(500) {
                    let line = line.trim();
                    if line.is_empty() {
                        continue;
                    }
                    if injector.push(PathBuf::from(line)).is_err() {
                        break;
                    }
                }
            }
            Ok(())
        }
        .boxed()
    };

    let picker = Picker::new(columns, 0, [], (), |cx, item: &PathBuf, action| {
        if let Err(e) = cx.editor.open(item, action) {
            cx.editor.set_error(format!("open {}: {e}", item.display()));
        }
    })
    .with_preview(|_editor, item: &PathBuf| Some((item.as_path().into(), None)))
    .with_dynamic_query(get_files, Some(275));

    cx.push_layer(Box::new(overlaid(picker)));
}

/// SPC h m : search system man pages. A dynamic picker runs `apropos <query>` and, on selection,
/// renders the chosen page (`man <section> <name>`, de-formatted with `col -bx`) into a scratch
/// buffer — Spacemacs `helm-man-woman`.
fn man_page_search(cx: &mut Context) {
    #[derive(Debug)]
    struct ManPage {
        name: String,
        section: String,
        line: String,
    }

    // apropos lines look like: "ls (1) - list directory contents".
    fn parse(line: &str) -> Option<ManPage> {
        let open = line.find('(')?;
        let close = line[open..].find(')')? + open;
        let name = line[..open].trim().split_whitespace().next()?.to_string();
        let section = line[open + 1..close].to_string();
        if name.is_empty() {
            return None;
        }
        Some(ManPage {
            name,
            section,
            line: line.trim().to_string(),
        })
    }

    let columns = [PickerColumn::new("man page", |item: &ManPage, _: &()| {
        item.line.clone().into()
    })];

    let get_pages = |query: &str,
                     _editor: &mut Editor,
                     _config: std::sync::Arc<()>,
                     injector: &ui::picker::Injector<ManPage, ()>| {
        let query = query.trim().to_owned();
        if query.is_empty() {
            return async { Ok(()) }.boxed();
        }
        let injector = injector.clone();
        async move {
            let out = std::process::Command::new("apropos")
                .arg(&query)
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).into_owned());
            if let Some(out) = out {
                for line in out.lines().take(500) {
                    if let Some(page) = parse(line) {
                        if injector.push(page).is_err() {
                            break;
                        }
                    }
                }
            }
            Ok(())
        }
        .boxed()
    };

    let picker = Picker::new(columns, 0, [], (), |cx, item: &ManPage, _action| {
        // Section first disambiguates pages that exist in multiple sections.
        let cmd = format!("man {} {} | col -bx", item.section, item.name);
        let out = std::process::Command::new("sh")
            .arg("-c")
            .arg(&cmd)
            .env("MANWIDTH", "80")
            .output();
        match out {
            Ok(o) if o.status.success() && !o.stdout.is_empty() => {
                let content = String::from_utf8_lossy(&o.stdout).into_owned();
                show_text_in_scratch(cx.editor, &content);
                cx.editor
                    .set_status(format!("man {}({})", item.name, item.section));
            }
            _ => cx
                .editor
                .set_error(format!("no man page for {}", item.name)),
        }
    })
    .with_dynamic_query(get_pages, Some(275));

    cx.push_layer(Box::new(overlaid(picker)));
}

/// SPC h i : search GNU info manuals via `info --apropos`, seeded with the symbol under the
/// cursor, and render the chosen node into a scratch buffer. Spacemacs `helm-info-at-point`.
fn info_search(cx: &mut Context) {
    // Seed the picker query with the word/symbol under the cursor.
    let seed = {
        let (view, doc) = current!(cx.editor);
        let text = doc.text().slice(..);
        let range = doc.selection(view.id).primary();
        let word = if range.from() != range.to() {
            range
        } else {
            textobject::textobject_word(text, range, textobject::TextObject::Inside, 1, false)
        };
        let s = text.slice(word.from()..word.to()).to_string().trim().to_string();
        Some(s).filter(|s| !s.is_empty())
    };

    #[derive(Debug)]
    struct InfoEntry {
        node: String,
        line: String,
    }

    // apropos lines look like: "(bash)Bourne Shell Builtins" -- false
    fn parse(line: &str) -> Option<InfoEntry> {
        let line = line.trim();
        let rest = line.strip_prefix('"')?;
        let end = rest.find('"')?;
        let node = rest[..end].to_string();
        if !node.starts_with('(') {
            return None;
        }
        Some(InfoEntry {
            node,
            line: line.to_string(),
        })
    }

    let columns = [PickerColumn::new("info", |item: &InfoEntry, _: &()| {
        item.line.clone().into()
    })];

    let get_nodes = |query: &str,
                     _editor: &mut Editor,
                     _config: std::sync::Arc<()>,
                     injector: &ui::picker::Injector<InfoEntry, ()>| {
        let query = query.trim().to_owned();
        if query.is_empty() {
            return async { Ok(()) }.boxed();
        }
        let injector = injector.clone();
        async move {
            let out = std::process::Command::new("info")
                .arg(format!("--apropos={query}"))
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).into_owned());
            if let Some(out) = out {
                for line in out.lines().take(500) {
                    if let Some(entry) = parse(line) {
                        if injector.push(entry).is_err() {
                            break;
                        }
                    }
                }
            }
            Ok(())
        }
        .boxed()
    };

    let picker = Picker::new(columns, 0, [], (), |cx, item: &InfoEntry, _action| {
        let out = std::process::Command::new("info")
            .arg("-o")
            .arg("-")
            .arg(&item.node)
            .output();
        match out {
            Ok(o) if o.status.success() && !o.stdout.is_empty() => {
                let content = String::from_utf8_lossy(&o.stdout).into_owned();
                show_text_in_scratch(cx.editor, &content);
                cx.editor.set_status(format!("info {}", item.node));
            }
            _ => cx
                .editor
                .set_error(format!("could not open info node {}", item.node)),
        }
    })
    .with_dynamic_query(get_nodes, Some(275));
    let picker = match seed {
        Some(q) => picker.with_query(q, cx.editor),
        None => picker,
    };

    cx.push_layer(Box::new(overlaid(picker)));
}

/// SPC e v : report the diagnostics setup for the current buffer — its language, attached language
/// servers and their init state, which servers provide (push/pull) diagnostics, and the current
/// diagnostic count. The zemacs analogue of Spacemacs' `flycheck-verify-setup`.
fn diagnostics_verify_setup(cx: &mut Context) {
    let report = {
        let doc = doc!(cx.editor);
        let mut out = format!("Diagnostics setup — {}\n", doc.display_name());
        out.push_str(&format!(
            "language: {}\n\n",
            doc.language_name().unwrap_or("(none / fundamental)")
        ));

        let servers: Vec<&zemacs_lsp::Client> = doc.language_servers().collect();
        if servers.is_empty() {
            out.push_str("No language servers attached to this buffer.\n");
        } else {
            out.push_str("Language servers:\n");
            for ls in &servers {
                out.push_str(&format!(
                    "  - {} ({})\n",
                    ls.name(),
                    if ls.is_initialized() {
                        "initialized"
                    } else {
                        "starting"
                    }
                ));
            }
        }

        let names = |feat| {
            let v: Vec<&str> = doc
                .language_servers_with_feature(feat)
                .map(|c| c.name())
                .collect();
            if v.is_empty() {
                "(none)".to_string()
            } else {
                v.join(", ")
            }
        };
        out.push_str(&format!(
            "\nPush-diagnostics providers: {}\n",
            names(LanguageServerFeature::Diagnostics)
        ));
        out.push_str(&format!(
            "Pull-diagnostics providers: {}\n",
            names(LanguageServerFeature::PullDiagnostics)
        ));
        out.push_str(&format!(
            "Current diagnostics in buffer: {}\n",
            doc.diagnostics().len()
        ));
        out
    };
    show_text_in_scratch(cx.editor, &report);
    cx.editor.set_status("diagnostics setup");
}

/// SPC e h : describe the "checkers" (language servers) attached to the current buffer — each
/// server's name, init state, and which features it advertises. The zemacs analogue of Spacemacs'
/// `flycheck-describe-checker`.
fn describe_diagnostics_checker(cx: &mut Context) {
    if doc!(cx.editor).language_servers().next().is_none() {
        cx.editor
            .set_error("no checker (language server) attached to this buffer");
        return;
    }
    let report = {
        let doc = doc!(cx.editor);
        let mut out = format!(
            "Checkers for {} ({})\n\n",
            doc.display_name(),
            doc.language_name().unwrap_or("(none)")
        );
        let feats = [
            ("diagnostics (push)", LanguageServerFeature::Diagnostics),
            ("diagnostics (pull)", LanguageServerFeature::PullDiagnostics),
            ("hover", LanguageServerFeature::Hover),
            ("completion", LanguageServerFeature::Completion),
            ("format", LanguageServerFeature::Format),
            ("code-action", LanguageServerFeature::CodeAction),
            ("rename", LanguageServerFeature::RenameSymbol),
            ("goto-definition", LanguageServerFeature::GotoDefinition),
            ("references", LanguageServerFeature::GotoReference),
            ("document-symbols", LanguageServerFeature::DocumentSymbols),
            ("inlay-hints", LanguageServerFeature::InlayHints),
            ("signature-help", LanguageServerFeature::SignatureHelp),
        ];
        for ls in doc.language_servers() {
            out.push_str(&format!(
                "● {} — {}\n",
                ls.name(),
                if ls.is_initialized() {
                    "initialized"
                } else {
                    "starting"
                }
            ));
            let supported: Vec<&str> = feats
                .iter()
                .filter(|(_, f)| ls.supports_feature(*f))
                .map(|(label, _)| *label)
                .collect();
            out.push_str(&format!(
                "    features: {}\n\n",
                if supported.is_empty() {
                    "(none advertised)".to_string()
                } else {
                    supported.join(", ")
                }
            ));
        }
        out
    };
    show_text_in_scratch(cx.editor, &report);
    cx.editor.set_status("describe checker");
}

/// SPC h d m : describe the current "modes" — editor mode, the buffer's major mode (language),
/// encoding, line ending, indent width, and attached language servers — into a scratch buffer.
/// Spacemacs `describe-mode`.
fn describe_current_modes(cx: &mut Context) {
    let mode = match cx.editor.mode() {
        Mode::Normal => "Normal",
        Mode::Insert => "Insert",
        Mode::Select => "Select (visual)",
    };
    let report = {
        let doc = doc!(cx.editor);
        let servers: Vec<&str> = doc.language_servers().map(|c| c.name()).collect();
        let mut out = format!("Current modes — {}\n\n", doc.display_name());
        out.push_str(&format!("editor mode: {mode}\n"));
        out.push_str(&format!(
            "major mode (language): {}\n",
            doc.language_name().unwrap_or("fundamental (none)")
        ));
        out.push_str(&format!("encoding: {}\n", doc.encoding().name()));
        out.push_str(&format!("line ending: {:?}\n", doc.line_ending));
        out.push_str(&format!("indent width: {}\n", doc.indent_width()));
        out.push_str(&format!(
            "language servers: {}\n",
            if servers.is_empty() {
                "(none)".to_string()
            } else {
                servers.join(", ")
            }
        ));
        out
    };
    show_text_in_scratch(cx.editor, &report);
    cx.editor.set_status("describe current modes");
}

/// SPC h d p : describe the language-support "package" for the current buffer — its grammar, file
/// types, comment tokens, formatter/debugger config, and configured language servers. zemacs has no
/// package manager, so the language configuration is the closest analogue of Spacemacs'
/// `describe-package`.
/// Render a human-readable summary of a language-support configuration (the zemacs "package").
fn render_language_package(
    lc: &zemacs_core::syntax::config::LanguageConfiguration,
) -> String {
    let mut out = format!("Language package: {}\n\n", lc.language_id);
    out.push_str(&format!("scope: {}\n", lc.scope));
    out.push_str(&format!(
        "grammar: {}\n",
        lc.grammar.as_deref().unwrap_or(&lc.language_id)
    ));
    let fts: Vec<String> = lc.file_types.iter().map(|f| format!("{f:?}")).collect();
    out.push_str(&format!(
        "file types: {}\n",
        if fts.is_empty() {
            "(none)".to_string()
        } else {
            fts.join(", ")
        }
    ));
    if !lc.shebangs.is_empty() {
        out.push_str(&format!("shebangs: {}\n", lc.shebangs.join(", ")));
    }
    out.push_str(&format!(
        "line comment: {}\n",
        lc.comment_tokens
            .as_ref()
            .map(|t| t.join(" "))
            .unwrap_or_else(|| "(none)".to_string())
    ));
    out.push_str(&format!(
        "block comment: {}\n",
        if lc.block_comment_tokens.is_some() {
            "yes"
        } else {
            "no"
        }
    ));
    out.push_str(&format!("auto-format on save: {}\n", lc.auto_format));
    out.push_str(&format!(
        "external formatter: {}\n",
        if lc.formatter.is_some() { "yes" } else { "no" }
    ));
    out.push_str(&format!(
        "debugger: {}\n",
        if lc.debugger.is_some() {
            "configured"
        } else {
            "none"
        }
    ));
    if let Some(tw) = lc.text_width {
        out.push_str(&format!("text width: {tw}\n"));
    }
    let servers: Vec<&str> = lc.language_servers.iter().map(|s| s.name.as_str()).collect();
    out.push_str(&format!(
        "configured language servers: {}\n",
        if servers.is_empty() {
            "(none)".to_string()
        } else {
            servers.join(", ")
        }
    ));
    out
}

fn describe_language_package(cx: &mut Context) {
    let report = {
        let doc = doc!(cx.editor);
        match doc.language_config() {
            None => format!(
                "No language-support package for {} (fundamental mode).\n",
                doc.display_name()
            ),
            Some(lc) => render_language_package(lc),
        }
    };
    show_text_in_scratch(cx.editor, &report);
    cx.editor.set_status("describe language package");
}

/// SPC h . : "search dotfile variables" — a picker over every editor config variable (dotted path
/// + current value). Selecting one copies its path to the clipboard for pasting into config.toml.
/// zemacs' config is the analogue of the Spacemacs dotfile. Spacemacs `helm-spacemacs-help-dotspacemacs`.
fn config_variable_search(cx: &mut Context) {
    struct ConfigVar {
        path: String,
        value: String,
    }
    fn flatten(v: &toml::Value, prefix: &str, out: &mut Vec<ConfigVar>) {
        match v {
            toml::Value::Table(t) => {
                let mut keys: Vec<&String> = t.keys().collect();
                keys.sort();
                for k in keys {
                    let p = if prefix.is_empty() {
                        k.clone()
                    } else {
                        format!("{prefix}.{k}")
                    };
                    flatten(&t[k], &p, out);
                }
            }
            other => out.push(ConfigVar {
                path: prefix.to_string(),
                value: other.to_string(),
            }),
        }
    }
    let mut items: Vec<ConfigVar> = Vec::new();
    if let Ok(cfg) = toml::Value::try_from(&*cx.editor.config()) {
        flatten(&cfg, "editor", &mut items);
    }
    if items.is_empty() {
        cx.editor.set_error("no config variables found");
        return;
    }
    let columns = [
        PickerColumn::new("variable", |it: &ConfigVar, _: &()| it.path.clone().into()),
        PickerColumn::new("value", |it: &ConfigVar, _: &()| it.value.clone().into()),
    ];
    let picker = Picker::new(columns, 0, items, (), |cx, it: &ConfigVar, _action| {
        let _ = cx.editor.registers.write('+', vec![it.path.clone()]);
        cx.editor
            .set_status(format!("{} = {} (path copied)", it.path, it.value));
    });
    cx.push_layer(Box::new(overlaid(picker)));
}

/// SPC h p : "search packages" — a picker over every configured language (zemacs' analogue of a
/// package list); selecting one renders its package description into a scratch buffer. Spacemacs
/// `helm-spacemacs-help-packages`.
fn package_search(cx: &mut Context) {
    struct PackageItem {
        name: String,
        types: String,
        desc: String,
    }
    let items: Vec<PackageItem> = {
        let loader: &zemacs_core::syntax::Loader = &cx.editor.syn_loader.load();
        let mut v: Vec<PackageItem> = loader
            .language_configs()
            .map(|lc| {
                let types: Vec<String> =
                    lc.file_types.iter().map(|f| format!("{f:?}")).collect();
                PackageItem {
                    name: lc.language_id.clone(),
                    types: types.join(", "),
                    desc: render_language_package(lc),
                }
            })
            .collect();
        v.sort_by(|a, b| a.name.cmp(&b.name));
        v
    };
    if items.is_empty() {
        cx.editor.set_error("no language packages configured");
        return;
    }
    let columns = [
        PickerColumn::new("package (language)", |it: &PackageItem, _: &()| {
            it.name.clone().into()
        }),
        PickerColumn::new("file types", |it: &PackageItem, _: &()| {
            it.types.clone().into()
        }),
    ];
    let picker = Picker::new(columns, 0, items, (), |cx, it: &PackageItem, _action| {
        show_text_in_scratch(cx.editor, &it.desc);
        cx.editor.set_status(format!("package: {}", it.name));
    });
    cx.push_layer(Box::new(overlaid(picker)));
}

/// SPC f e e : show the editor's environment variables (inherited from the shell) in a scratch
/// buffer. A terminal editor has no `.spacemacs.env` file — env vars live in the process
/// environment — so this displays where they are actually set. Spacemacs `dotspacemacs/user-env`.
fn show_environment(cx: &mut Context) {
    let mut vars: Vec<(String, String)> = std::env::vars().collect();
    vars.sort();
    let mut out = String::from("Editor environment variables (inherited from the shell)\n\n");
    for (k, v) in vars {
        out.push_str(&format!("{k}={v}\n"));
    }
    show_text_in_scratch(cx.editor, &out);
    cx.editor.set_status("environment variables");
}

/// SPC f e C-e : re-import the shell environment into the editor's process environment by running
/// the login shell and capturing its `env` (like `exec-path-from-shell` / Spacemacs' env reinit).
/// Newly-spawned subprocesses (LSP servers, formatters, terminals) then see the refreshed env.
fn reimport_shell_env(cx: &mut Context) {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
    match std::process::Command::new(&shell).args(["-lc", "env"]).output() {
        Ok(o) if o.status.success() => {
            let text = String::from_utf8_lossy(&o.stdout);
            let mut n = 0;
            for line in text.lines() {
                if let Some((k, v)) = line.split_once('=') {
                    if !k.is_empty() && !k.contains(char::is_whitespace) {
                        std::env::set_var(k, v);
                        n += 1;
                    }
                }
            }
            cx.editor
                .set_status(format!("imported {n} environment variables from {shell}"));
        }
        Ok(o) => cx.editor.set_error(format!(
            "env import failed: {}",
            String::from_utf8_lossy(&o.stderr).trim()
        )),
        Err(e) => cx.editor.set_error(format!("could not run {shell}: {e}")),
    }
}

/// SPC h n : browse zemacs' release notes (the editor's NEWS), embedded at build time and shown
/// in a scratch buffer. Spacemacs `view-emacs-news`.
fn browse_news(cx: &mut Context) {
    const CHANGELOG: &str = include_str!("../../CHANGELOG.md");
    show_text_in_scratch(cx.editor, CHANGELOG);
    cx.editor.set_status("zemacs release notes (CHANGELOG)");
}

/// SPC h d l : copy the most recently pressed keys to the system clipboard (newest last), for
/// pasting into bug reports / chat. Spacemacs `spacemacs/describe-last-keys`.
fn copy_last_keys(cx: &mut Context) {
    // The final key in the ring is the one that triggered this command; drop it.
    let mut keys: Vec<KeyEvent> = cx.editor.last_keys.iter().copied().collect();
    keys.pop();
    if keys.is_empty() {
        cx.editor.set_status("no recent keys recorded yet");
        return;
    }
    let s = keys
        .into_iter()
        .map(|key| {
            let s = key.to_string();
            if s.chars().count() == 1 {
                s
            } else {
                format!("<{s}>")
            }
        })
        .collect::<String>();
    if let Err(e) = cx.editor.registers.write('+', vec![s.clone()]) {
        cx.editor.set_error(format!("clipboard write failed: {e}"));
        return;
    }
    cx.editor.set_status(format!("Copied last keys to clipboard: {s}"));
}

/// SPC h d s : copy system information (zemacs version, OS, arch, term) to the system clipboard,
/// for pasting into bug reports — Spacemacs `spacemacs/describe-system-info`.
fn copy_system_info(cx: &mut Context) {
    let info = format!(
        "zemacs {}\nOS: {} ({})\narch: {}\nterm: {}",
        zemacs_loader::VERSION_AND_GIT_HASH,
        std::env::consts::OS,
        std::env::consts::FAMILY,
        std::env::consts::ARCH,
        std::env::var("TERM").unwrap_or_else(|_| "(unknown)".to_string()),
    );
    if let Err(e) = cx.editor.registers.write('+', vec![info.clone()]) {
        cx.editor.set_error(format!("clipboard write failed: {e}"));
        return;
    }
    cx.editor.set_status(format!(
        "Copied system info to clipboard ({} bytes)",
        info.len()
    ));
}

/// SPC h d t : describe the "text properties" at the cursor. In a structural editor that means the
/// tree-sitter node stack at point (innermost → outermost) plus the character under the cursor —
/// the zemacs analogue of Emacs' `describe-text-properties`.
fn describe_text_properties(cx: &mut Context) {
    let report = {
        let (view, doc) = current!(cx.editor);
        let text = doc.text();
        let slice = text.slice(..);
        let cursor = doc.selection(view.id).primary().cursor(slice);
        let byte = text.char_to_byte(cursor) as u32;
        let line = text.char_to_line(cursor);
        let col = cursor - text.line_to_char(line);
        let mut out = format!(
            "Text properties at line {}, col {} (char {cursor}, byte {byte})\n",
            line + 1,
            col + 1
        );
        out.push_str(&format!(
            "char: {}\n\n",
            match slice.get_char(cursor) {
                Some(c) => format!("{c:?} (U+{:04X})", c as u32),
                None => "(end of buffer)".to_string(),
            }
        ));
        match doc.syntax() {
            None => out.push_str("No syntax tree (fundamental mode / unsupported language).\n"),
            Some(syntax) => match syntax.descendant_for_byte_range(byte, byte) {
                None => out.push_str("No syntax node at point.\n"),
                Some(node) => {
                    out.push_str("Syntax nodes (innermost \u{2192} outermost):\n");
                    let mut cur = Some(node);
                    let mut depth = 0usize;
                    while let Some(n) = cur {
                        let r = n.byte_range();
                        out.push_str(&format!(
                            "  {}{}{}  [{}..{}]\n",
                            "  ".repeat(depth),
                            n.kind(),
                            if n.is_named() { "" } else { " (anon)" },
                            r.start,
                            r.end
                        ));
                        cur = n.parent();
                        depth += 1;
                    }
                }
            },
        }
        out
    };
    show_text_in_scratch(cx.editor, &report);
    cx.editor.set_status("describe text properties");
}

/// SPC p e : open the project-local config (`<workspace>/.zemacs/config.toml`), creating it (and
/// its `.zemacs/` directory) if absent — zemacs' analogue of editing a project's dir-locals.
fn edit_project_config(cx: &mut Context) {
    let path = zemacs_loader::workspace_config_file();
    if !path.exists() {
        if let Some(parent) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                cx.editor
                    .set_error(format!("create {}: {e}", parent.display()));
                return;
            }
        }
        if let Err(e) = std::fs::write(&path, b"# zemacs project-local config\n") {
            cx.editor
                .set_error(format!("create {}: {e}", path.display()));
            return;
        }
    }
    if let Err(e) = cx.editor.open(&path, Action::Replace) {
        cx.editor
            .set_error(format!("open {}: {e}", path.display()));
    }
}

/// `git init` in the current working directory (Spacemacs `SPC g i`).
fn git_init(cx: &mut Context) {
    let dir = zemacs_stdx::env::current_working_dir();
    match std::process::Command::new("git")
        .arg("init")
        .current_dir(&dir)
        .output()
    {
        Ok(out) if out.status.success() => {
            cx.editor.set_status("Initialized empty git repository")
        }
        Ok(out) => cx
            .editor
            .set_error(String::from_utf8_lossy(&out.stderr).trim().to_string()),
        Err(e) => cx.editor.set_error(format!("git init failed: {e}")),
    }
}

/// Load `content` into a fresh scratch buffer in the current window (replacing the view's
/// document, which stays in the buffer list). Used to display read-only generated text.
fn show_text_in_scratch(editor: &mut Editor, content: &str) {
    editor.new_file(Action::Replace);
    let (view, doc) = current!(editor);
    doc.ensure_view_init(view.id);
    let transaction =
        Transaction::insert(doc.text(), doc.selection(view.id), content.into())
            .with_selection(Selection::point(0));
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
}

/// SPC g f f : prompt for a branch/commit and show the current file as it was at that revision
/// (`git show <rev>:<file>`) in a scratch buffer (Spacemacs `magit-find-file` / git-timemachine).
fn view_file_at_rev(cx: &mut Context) {
    let Some(path) = doc!(cx.editor).path().map(|p| p.to_path_buf()) else {
        cx.editor.set_error("buffer has no file path");
        return;
    };
    let prompt = crate::ui::prompt::Prompt::new(
        "view file at rev (branch/commit):".into(),
        None,
        ui::completers::none,
        move |cx: &mut crate::compositor::Context, input: &str, event: PromptEvent| {
            if event != PromptEvent::Validate || input.trim().is_empty() {
                return;
            }
            let rev = input.trim();
            let dir = path.parent().unwrap_or_else(|| std::path::Path::new("."));
            let fname = path.file_name().map(|f| f.to_string_lossy().into_owned());
            let Some(fname) = fname else {
                cx.editor.set_error("bad file name");
                return;
            };
            // `<rev>:./<name>` resolves relative to the prefix git is run in.
            let spec = format!("{rev}:./{fname}");
            let out = std::process::Command::new("git")
                .arg("-C")
                .arg(dir)
                .arg("show")
                .arg(&spec)
                .output();
            match out {
                Ok(o) if o.status.success() => {
                    let content = String::from_utf8_lossy(&o.stdout).into_owned();
                    show_text_in_scratch(cx.editor, &content);
                    cx.editor.set_status(format!("{spec}"));
                }
                Ok(o) => cx
                    .editor
                    .set_error(String::from_utf8_lossy(&o.stderr).trim().to_string()),
                Err(e) => cx.editor.set_error(format!("git show failed: {e}")),
            }
        },
    );
    cx.push_layer(Box::new(prompt));
}

// ---------------------------------------------------------------------------
// Elisp evaluation (Spacemacs `SPC m e *` in emacs-lisp-mode). zemacs embeds an
// elisp interpreter; evaluate the selection / line / defun / buffer against the
// live editor and echo the result. (No-ops politely off elisp buffers — the
// reader simply errors.)
// ---------------------------------------------------------------------------

fn run_elisp(cx: &mut Context, src: &str) {
    if src.trim().is_empty() {
        cx.editor.set_status("nothing to evaluate");
        return;
    }
    let result = {
        let mut ccx = crate::compositor::Context {
            editor: cx.editor,
            jobs: cx.jobs,
            scroll: None,
        };
        crate::commands::scripting::eval_elisp(&mut ccx, src)
    };
    match result {
        Ok(out) => cx.editor.set_status(format!("⇒ {out}")),
        Err(e) => cx.editor.set_error(format!("elisp: {e}")),
    }
}

/// SPC m e r: evaluate the current selection as elisp.
fn eval_elisp_region(cx: &mut Context) {
    let src = {
        let (view, doc) = current!(cx.editor);
        let text = doc.text().slice(..);
        doc.selection(view.id).primary().fragment(text).to_string()
    };
    run_elisp(cx, &src);
}

/// SPC m e b: evaluate the whole buffer as elisp.
fn eval_elisp_buffer(cx: &mut Context) {
    let src = doc!(cx.editor).text().to_string();
    run_elisp(cx, &src);
}

/// SPC m e e / e $ / e l: evaluate the current line (≈ last sexp) as elisp.
fn eval_elisp_line(cx: &mut Context) {
    let src = {
        let (view, doc) = current!(cx.editor);
        let text = doc.text();
        let head = doc
            .selection(view.id)
            .primary()
            .head
            .min(text.len_chars().saturating_sub(1));
        let line = text.char_to_line(head);
        text.line(line).to_string()
    };
    run_elisp(cx, &src);
}

/// SPC m e f / e c: evaluate the enclosing top-level form (≈ paragraph) as elisp.
fn eval_elisp_defun(cx: &mut Context) {
    let src = {
        let (view, doc) = current!(cx.editor);
        let text = doc.text();
        let last = text.len_lines().saturating_sub(1);
        let cur = text
            .char_to_line(doc.selection(view.id).primary().head.min(text.len_chars()))
            .min(last);
        let is_blank = |l: usize| text.line(l).to_string().trim().is_empty();
        let mut start = cur;
        while start > 0 && !is_blank(start - 1) {
            start -= 1;
        }
        let mut end = cur;
        while end < last && !is_blank(end + 1) {
            end += 1;
        }
        let from = text.line_to_char(start);
        let to = text.line_to_char((end + 1).min(text.len_lines()));
        text.slice(from..to).to_string()
    };
    run_elisp(cx, &src);
}

// ---------------------------------------------------------------------------
// Regex form conversion (Spacemacs `SPC x r`, pcre2el). The grouping `( )`,
// alternation `|`, and quantifier `{ }` metacharacters are *inverted* between
// PCRE (bare = special) and Emacs regex (backslash-escaped = special), so a
// single backslash-toggle on those five characters converts either direction.
// ---------------------------------------------------------------------------

/// Toggle backslash-escaping of `( ) | { }` — converts a regex between PCRE and
/// Emacs (elisp) forms for the grouping/alternation/quantifier subset. Pure.
fn swap_regex_grouping(s: &str) -> String {
    let ch: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < ch.len() {
        let c = ch[i];
        if c == '\\' && i + 1 < ch.len() {
            let n = ch[i + 1];
            if matches!(n, '(' | ')' | '|' | '{' | '}') {
                out.push(n); // escaped special -> bare
            } else {
                out.push('\\');
                out.push(n); // preserve other escapes (\d, \w, \b, \\, …)
            }
            i += 2;
            continue;
        }
        if matches!(c, '(' | ')' | '|' | '{' | '}') {
            out.push('\\'); // bare special -> escaped
        }
        out.push(c);
        i += 1;
    }
    out
}

// --- regex -> rx sexp form (Spacemacs `SPC x r x` / `x r /`, pcre2el) ---------
// A small recursive-descent parser for *Emacs* regex syntax that emits the
// Emacs `rx` s-expression form. Honest by construction: it returns Err on any
// construct it does not understand, so it never emits a wrong conversion.

#[derive(Debug, PartialEq)]
enum Rx {
    Lit(String),
    AnyChar,
    Bol,
    Eol,
    Class(bool, String),
    WordChar,
    NotWordChar,
    Space,
    Group(Box<Rx>),
    Seq(Vec<Rx>),
    Or(Vec<Rx>),
    Star(Box<Rx>),
    Plus(Box<Rx>),
    Opt(Box<Rx>),
    Repeat(usize, Option<usize>, Box<Rx>),
}

struct RxParser {
    ch: Vec<char>,
    i: usize,
}

impl RxParser {
    fn at(&self, s: &str) -> bool {
        self.ch[self.i..].iter().collect::<String>().starts_with(s)
    }
    fn parse_alt(&mut self) -> Result<Rx, String> {
        let mut alts = vec![self.parse_seq()?];
        while self.at("\\|") {
            self.i += 2;
            alts.push(self.parse_seq()?);
        }
        Ok(if alts.len() == 1 {
            alts.pop().unwrap()
        } else {
            Rx::Or(alts)
        })
    }
    fn parse_seq(&mut self) -> Result<Rx, String> {
        let mut items = Vec::new();
        while self.i < self.ch.len() && !self.at("\\|") && !self.at("\\)") {
            items.push(self.parse_quant()?);
        }
        // merge adjacent single-char literals into runs
        let mut merged: Vec<Rx> = Vec::new();
        for it in items {
            if let (Rx::Lit(a), Some(Rx::Lit(b))) = (&it, merged.last_mut()) {
                b.push_str(a);
            } else {
                merged.push(it);
            }
        }
        Ok(if merged.len() == 1 {
            merged.pop().unwrap()
        } else {
            Rx::Seq(merged)
        })
    }
    fn parse_quant(&mut self) -> Result<Rx, String> {
        let atom = self.parse_atom()?;
        if self.i < self.ch.len() {
            match self.ch[self.i] {
                '*' => {
                    self.i += 1;
                    return Ok(Rx::Star(Box::new(atom)));
                }
                '+' => {
                    self.i += 1;
                    return Ok(Rx::Plus(Box::new(atom)));
                }
                '?' => {
                    self.i += 1;
                    return Ok(Rx::Opt(Box::new(atom)));
                }
                _ => {}
            }
            if self.at("\\{") {
                self.i += 2;
                let mut num = String::new();
                while self.i < self.ch.len() && self.ch[self.i].is_ascii_digit() {
                    num.push(self.ch[self.i]);
                    self.i += 1;
                }
                let lo: usize = num.parse().map_err(|_| "bad {n}".to_string())?;
                let hi = if self.i < self.ch.len() && self.ch[self.i] == ',' {
                    self.i += 1;
                    let mut h = String::new();
                    while self.i < self.ch.len() && self.ch[self.i].is_ascii_digit() {
                        h.push(self.ch[self.i]);
                        self.i += 1;
                    }
                    if h.is_empty() {
                        None
                    } else {
                        Some(h.parse().map_err(|_| "bad {n,m}".to_string())?)
                    }
                } else {
                    Some(lo)
                };
                if !self.at("\\}") {
                    return Err("unterminated \\{".into());
                }
                self.i += 2;
                return Ok(Rx::Repeat(lo, hi, Box::new(atom)));
            }
        }
        Ok(atom)
    }
    fn parse_atom(&mut self) -> Result<Rx, String> {
        if self.at("\\(") {
            self.i += 2;
            if self.at("?:") {
                self.i += 2;
            }
            let inner = self.parse_alt()?;
            if !self.at("\\)") {
                return Err("unterminated \\(".into());
            }
            self.i += 2;
            return Ok(Rx::Group(Box::new(inner)));
        }
        let c = self.ch[self.i];
        match c {
            '.' => {
                self.i += 1;
                Ok(Rx::AnyChar)
            }
            '^' => {
                self.i += 1;
                Ok(Rx::Bol)
            }
            '$' => {
                self.i += 1;
                Ok(Rx::Eol)
            }
            '[' => {
                self.i += 1;
                let neg = self.i < self.ch.len() && self.ch[self.i] == '^';
                if neg {
                    self.i += 1;
                }
                let mut body = String::new();
                while self.i < self.ch.len() && self.ch[self.i] != ']' {
                    body.push(self.ch[self.i]);
                    self.i += 1;
                }
                if self.i >= self.ch.len() {
                    return Err("unterminated [".into());
                }
                self.i += 1;
                Ok(Rx::Class(neg, body))
            }
            '\\' => {
                if self.i + 1 >= self.ch.len() {
                    return Err("trailing backslash".into());
                }
                let n = self.ch[self.i + 1];
                self.i += 2;
                match n {
                    'w' => Ok(Rx::WordChar),
                    'W' => Ok(Rx::NotWordChar),
                    's' => Ok(Rx::Space),
                    '.' | '*' | '+' | '?' | '[' | ']' | '^' | '$' | '\\' | '(' | ')' | '|'
                    | '{' | '}' => Ok(Rx::Lit(n.to_string())),
                    other => Err(format!("unsupported escape \\{other}")),
                }
            }
            _ => {
                self.i += 1;
                Ok(Rx::Lit(c.to_string()))
            }
        }
    }
}

fn rx_to_string(rx: &Rx) -> String {
    match rx {
        Rx::Lit(s) => format!("{s:?}"),
        Rx::AnyChar => "nonl".into(),
        Rx::Bol => "bol".into(),
        Rx::Eol => "eol".into(),
        Rx::Class(false, b) => format!("(any {b:?})"),
        Rx::Class(true, b) => format!("(not (any {b:?}))"),
        Rx::WordChar => "wordchar".into(),
        Rx::NotWordChar => "(not wordchar)".into(),
        Rx::Space => "space".into(),
        Rx::Group(i) => format!("(group {})", rx_to_string(i)),
        Rx::Seq(items) => format!(
            "(seq {})",
            items.iter().map(rx_to_string).collect::<Vec<_>>().join(" ")
        ),
        Rx::Or(items) => format!(
            "(or {})",
            items.iter().map(rx_to_string).collect::<Vec<_>>().join(" ")
        ),
        Rx::Star(i) => format!("(zero-or-more {})", rx_to_string(i)),
        Rx::Plus(i) => format!("(one-or-more {})", rx_to_string(i)),
        Rx::Opt(i) => format!("(opt {})", rx_to_string(i)),
        Rx::Repeat(lo, Some(hi), i) if lo == hi => format!("(= {lo} {})", rx_to_string(i)),
        Rx::Repeat(lo, Some(hi), i) => format!("(** {lo} {hi} {})", rx_to_string(i)),
        Rx::Repeat(lo, None, i) => format!("(>= {lo} {})", rx_to_string(i)),
    }
}

/// Convert an Emacs-regex string to its `rx` s-expression form, or Err.
fn emacs_regex_to_rx(s: &str) -> Result<String, String> {
    if s.is_empty() {
        return Err("empty regex".into());
    }
    let mut p = RxParser {
        ch: s.chars().collect(),
        i: 0,
    };
    let rx = p.parse_alt()?;
    if p.i != p.ch.len() {
        return Err(format!("unexpected `{}`", p.ch[p.i]));
    }
    Ok(rx_to_string(&rx))
}

/// Shared: convert the selection to rx form. `pcre` first swaps grouping to the
/// Emacs convention. `replace` swaps the text in place; otherwise echoes it.
fn regex_to_rx(cx: &mut Context, pcre: bool, replace: bool) {
    let (view, doc) = current!(cx.editor);
    let text = doc.text().slice(..);
    let range = doc.selection(view.id).primary();
    if range.from() == range.to() {
        cx.editor.set_status("select a regex to convert to rx");
        return;
    }
    let src = range.fragment(text).to_string();
    let emacs = if pcre { swap_regex_grouping(&src) } else { src };
    match emacs_regex_to_rx(&emacs) {
        Ok(rx) => {
            if replace {
                let transaction = Transaction::change(
                    doc.text(),
                    std::iter::once((range.from(), range.to(), Some(rx.clone().into()))),
                );
                doc.apply(&transaction, view.id);
                cx.editor.set_status(format!("rx: {rx}"));
            } else {
                cx.editor.set_status(format!("rx: {rx}"));
            }
        }
        Err(e) => cx.editor.set_error(format!("can't convert to rx: {e}")),
    }
}

fn regex_emacs_to_rx_replace(cx: &mut Context) { regex_to_rx(cx, false, true) }
fn regex_emacs_to_rx_explain(cx: &mut Context) { regex_to_rx(cx, false, false) }
fn regex_pcre_to_rx_replace(cx: &mut Context) { regex_to_rx(cx, true, true) }
fn regex_pcre_to_rx_explain(cx: &mut Context) { regex_to_rx(cx, true, false) }

/// Convert the selected regex between PCRE and Emacs forms (Spacemacs
/// `SPC x r c` / `x r e p` / `x r p e`). Operates on the primary selection.
fn regex_convert_form(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);
    let text = doc.text().slice(..);
    let range = doc.selection(view.id).primary();
    if range.from() == range.to() {
        cx.editor.set_status("select a regex to convert (PCRE ⇔ Emacs)");
        return;
    }
    let converted = swap_regex_grouping(&range.fragment(text).to_string());
    let transaction = Transaction::change(
        doc.text(),
        std::iter::once((range.from(), range.to(), Some(converted.into()))),
    );
    doc.apply(&transaction, view.id);
    cx.editor.set_status("converted regex grouping (PCRE ⇔ Emacs)");
}

fn global_search(cx: &mut Context) {
    global_search_seeded(cx, None)
}

/// Clear persistent search state/highlight (Spacemacs `SPC s c`): drops the
/// active search register so nothing remains highlighted or navigable.
fn clear_search_highlight(cx: &mut Context) {
    let reg = cx.editor.registers.last_search_register;
    cx.editor.registers.remove(reg);
    cx.editor.clear_status();
}

/// The word/selection under the cursor, for "search with default input".
fn symbol_at_point(cx: &mut Context) -> Option<String> {
    let (view, doc) = current!(cx.editor);
    let text = doc.text().slice(..);
    let range = doc.selection(view.id).primary();
    // A non-empty selection wins; otherwise grab the word object at the cursor.
    let word = if range.from() != range.to() {
        range
    } else {
        textobject::textobject_word(text, range, textobject::TextObject::Inside, 1, false)
    };
    let s: String = text.slice(word.from()..word.to()).to_string();
    let s = s.trim();
    if s.is_empty() {
        None
    } else {
        Some(regex::escape(s))
    }
}

/// Like `global_search` but pre-seeds the query with the symbol under the
/// cursor (Spacemacs "search … with default input", the uppercase `SPC s`
/// variants and `SPC *`).
fn global_search_symbol(cx: &mut Context) {
    let seed = symbol_at_point(cx);
    global_search_seeded(cx, seed)
}

fn global_search_seeded(cx: &mut Context, seed: Option<String>) {
    #[derive(Debug)]
    struct FileResult<'a> {
        path: Cow<'a, Path>,
        /// 0 indexed line start
        line_start: usize,
        /// 0 indexed line end
        line_end: usize,
    }

    impl FileResult<'_> {
        fn new(path: &Path, line_start: usize, line_end: usize) -> Self {
            Self {
                path: zemacs_stdx::path::get_relative_path(path.to_path_buf()),
                line_start,
                line_end,
            }
        }
    }

    struct GlobalSearchConfig {
        smart_case: bool,
        file_picker_config: zemacs_view::editor::FilePickerConfig,
        style: PathStyleConfig,
    }

    let config = cx.editor.config();
    let config = GlobalSearchConfig {
        smart_case: config.search.smart_case,
        file_picker_config: config.file_picker.clone(),
        style: PathStyleConfig::new(&cx.editor.theme),
    };

    let columns = [
        PickerColumn::new("path", |item: &FileResult, config: &GlobalSearchConfig| {
            config
                .style
                .stylize(Some(&item.path), Some(item.line_start))
        }),
        PickerColumn::hidden("contents"),
    ];

    let get_files = |query: &str,
                     editor: &mut Editor,
                     config: std::sync::Arc<GlobalSearchConfig>,
                     injector: &ui::picker::Injector<_, _>| {
        if query.is_empty() {
            return async { Ok(()) }.boxed();
        }

        let search_root = zemacs_stdx::env::current_working_dir();
        if !search_root.exists() {
            return async { Err(anyhow::anyhow!("Current working directory does not exist")) }
                .boxed();
        }

        let documents: Vec<_> = editor
            .documents()
            .map(|doc| (doc.path().map(ToOwned::to_owned), doc.text().to_owned()))
            .collect();

        let matcher = match RegexMatcherBuilder::new()
            .case_smart(config.smart_case)
            .multi_line(true)
            .build(query)
        {
            Ok(matcher) => {
                // Clear any "Failed to compile regex" errors out of the statusline.
                editor.clear_status();
                matcher
            }
            Err(err) => {
                log::info!("Failed to compile search pattern in global search: {}", err);
                return async { Err(anyhow::anyhow!("Failed to compile regex")) }.boxed();
            }
        };

        let dedup_symlinks = config.file_picker_config.deduplicate_links;
        let absolute_root = search_root
            .canonicalize()
            .unwrap_or_else(|_| search_root.clone());

        let injector = injector.clone();
        async move {
            let searcher = SearcherBuilder::new()
                .binary_detection(BinaryDetection::quit(b'\x00'))
                .multi_line(true)
                .build();
            WalkBuilder::new(search_root)
                .hidden(config.file_picker_config.hidden)
                .parents(config.file_picker_config.parents)
                .ignore(config.file_picker_config.ignore)
                .follow_links(config.file_picker_config.follow_symlinks)
                .git_ignore(config.file_picker_config.git_ignore)
                .git_global(config.file_picker_config.git_global)
                .git_exclude(config.file_picker_config.git_exclude)
                .max_depth(config.file_picker_config.max_depth)
                .filter_entry(move |entry| {
                    filter_picker_entry(entry, &absolute_root, dedup_symlinks)
                })
                .add_custom_ignore_filename(zemacs_loader::config_dir().join("ignore"))
                .add_custom_ignore_filename(".zemacs/ignore")
                .build_parallel()
                .run(|| {
                    let mut searcher = searcher.clone();
                    let matcher = matcher.clone();
                    let injector = injector.clone();
                    let documents = &documents;
                    Box::new(move |entry: Result<DirEntry, ignore::Error>| -> WalkState {
                        let entry = match entry {
                            Ok(entry) => entry,
                            Err(_) => return WalkState::Continue,
                        };

                        if !entry.path().is_file() {
                            return WalkState::Continue;
                        }

                        let mut stop = false;
                        let sink = sinks::UTF8(|line_start, line_content| {
                            let line_start = line_start as usize - 1;
                            let line_end = line_start + line_content.lines().count() - 1;
                            stop = injector
                                .push(FileResult::new(entry.path(), line_start, line_end))
                                .is_err();

                            Ok(!stop)
                        });
                        let doc = documents.iter().find(|&(doc_path, _)| {
                            doc_path
                                .as_ref()
                                .is_some_and(|doc_path| doc_path == entry.path())
                        });

                        let result = if let Some((_, doc)) = doc {
                            // there is already a buffer for this file
                            // search the buffer instead of the file because it's faster
                            // and captures new edits without requiring a save
                            if searcher.multi_line_with_matcher(&matcher) {
                                // in this case a continuous buffer is required
                                // convert the rope to a string
                                let text = doc.to_string();
                                searcher.search_slice(&matcher, text.as_bytes(), sink)
                            } else {
                                searcher.search_reader(
                                    &matcher,
                                    RopeReader::new(doc.slice(..)),
                                    sink,
                                )
                            }
                        } else {
                            searcher.search_path(&matcher, entry.path(), sink)
                        };

                        if let Err(err) = result {
                            log::error!("Global search error: {}, {}", entry.path().display(), err);
                        }
                        if stop {
                            WalkState::Quit
                        } else {
                            WalkState::Continue
                        }
                    })
                });
            Ok(())
        }
        .boxed()
    };

    let reg = cx.register.unwrap_or('/');
    cx.editor.registers.last_search_register = reg;

    let picker = Picker::new(
        columns,
        1, // contents
        [],
        config,
        move |cx,
              FileResult {
                  path,
                  line_start,
                  line_end,
                  ..
              },
              action| {
            let doc = match cx.editor.open(path, action) {
                Ok(id) => doc_mut!(cx.editor, &id),
                Err(e) => {
                    cx.editor
                        .set_error(format!("Failed to open file '{}': {}", path.display(), e));
                    return;
                }
            };

            let line_start = *line_start;
            let line_end = *line_end;
            let view = view_mut!(cx.editor);
            let text = doc.text();
            if line_start >= text.len_lines() {
                cx.editor.set_error(
                    "The line you jumped to does not exist anymore because the file has changed.",
                );
                return;
            }
            let start = text.line_to_char(line_start);
            let end = text.line_to_char((line_end + 1).min(text.len_lines()));

            doc.set_selection(view.id, Selection::single(start, end));
            if action.align_view(view, doc.id()) {
                align_view(doc, view, Align::Center);
            }
        },
    )
    .with_preview(
        |_editor,
         FileResult {
             path,
             line_start,
             line_end,
             ..
         }| { Some((path.as_ref().into(), Some((*line_start, *line_end)))) },
    )
    .with_history_register(Some(reg));
    let picker = match seed {
        Some(q) if !q.is_empty() => picker.with_query(q, cx.editor),
        _ => picker,
    };
    let picker = picker.with_dynamic_query(get_files, Some(275));

    cx.push_layer(Box::new(overlaid(picker)));
}

enum Extend {
    Above,
    Below,
}

fn extend_line(cx: &mut Context) {
    let (view, doc) = current_ref!(cx.editor);
    let extend = match doc.selection(view.id).primary().direction() {
        Direction::Forward => Extend::Below,
        Direction::Backward => Extend::Above,
    };
    extend_line_impl(cx, extend);
}

fn extend_line_below(cx: &mut Context) {
    extend_line_impl(cx, Extend::Below);
}

fn extend_line_above(cx: &mut Context) {
    extend_line_impl(cx, Extend::Above);
}
fn extend_line_impl(cx: &mut Context, extend: Extend) {
    let count = cx.count();
    let (view, doc) = current!(cx.editor);

    let text = doc.text();
    let selection = doc.selection(view.id).clone().transform(|range| {
        let (start_line, end_line) = range.line_range(text.slice(..));

        let start = text.line_to_char(start_line);
        let end = text.line_to_char(
            (end_line + 1) // newline of end_line
                .min(text.len_lines()),
        );

        // extend to previous/next line if current line is selected
        let (anchor, head) = if range.from() == start && range.to() == end {
            match extend {
                Extend::Above => (end, text.line_to_char(start_line.saturating_sub(count))),
                Extend::Below => (
                    start,
                    text.line_to_char((end_line + count + 1).min(text.len_lines())),
                ),
            }
        } else {
            match extend {
                Extend::Above => (end, text.line_to_char(start_line.saturating_sub(count - 1))),
                Extend::Below => (
                    start,
                    text.line_to_char((end_line + count).min(text.len_lines())),
                ),
            }
        };

        Range::new(anchor, head)
    });

    doc.set_selection(view.id, selection);
}
fn select_line_below(cx: &mut Context) {
    select_line_impl(cx, Extend::Below);
}
fn select_line_above(cx: &mut Context) {
    select_line_impl(cx, Extend::Above);
}
fn select_line_impl(cx: &mut Context, extend: Extend) {
    let mut count = cx.count();
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let saturating_add = |a: usize, b: usize| (a + b).min(text.len_lines());
    let selection = doc.selection(view.id).clone().transform(|range| {
        let (start_line, end_line) = range.line_range(text.slice(..));
        let start = text.line_to_char(start_line);
        let end = text.line_to_char(saturating_add(end_line, 1));
        let direction = range.direction();

        // Extending to line bounds is counted as one step
        if range.from() != start || range.to() != end {
            count = count.saturating_sub(1)
        }
        let (anchor_line, head_line) = match (&extend, direction) {
            (Extend::Above, Direction::Forward) => (start_line, end_line.saturating_sub(count)),
            (Extend::Above, Direction::Backward) => (end_line, start_line.saturating_sub(count)),
            (Extend::Below, Direction::Forward) => (start_line, saturating_add(end_line, count)),
            (Extend::Below, Direction::Backward) => (end_line, saturating_add(start_line, count)),
        };
        let (anchor, head) = match anchor_line.cmp(&head_line) {
            Ordering::Less => (
                text.line_to_char(anchor_line),
                text.line_to_char(saturating_add(head_line, 1)),
            ),
            Ordering::Equal => match extend {
                Extend::Above => (
                    text.line_to_char(saturating_add(anchor_line, 1)),
                    text.line_to_char(head_line),
                ),
                Extend::Below => (
                    text.line_to_char(head_line),
                    text.line_to_char(saturating_add(anchor_line, 1)),
                ),
            },

            Ordering::Greater => (
                text.line_to_char(saturating_add(anchor_line, 1)),
                text.line_to_char(head_line),
            ),
        };
        Range::new(anchor, head)
    });

    doc.set_selection(view.id, selection);
}

fn extend_to_line_bounds(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);

    doc.set_selection(
        view.id,
        doc.selection(view.id).clone().transform(|range| {
            let text = doc.text();

            let (start_line, end_line) = range.line_range(text.slice(..));
            let start = text.line_to_char(start_line);
            let end = text.line_to_char((end_line + 1).min(text.len_lines()));

            Range::new(start, end).with_direction(range.direction())
        }),
    );
}

fn shrink_to_line_bounds(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);

    doc.set_selection(
        view.id,
        doc.selection(view.id).clone().transform(|range| {
            let text = doc.text();

            let (start_line, end_line) = range.line_range(text.slice(..));

            // Do nothing if the selection is within one line to prevent
            // conditional logic for the behavior of this command
            if start_line == end_line {
                return range;
            }

            let mut start = text.line_to_char(start_line);

            // line_to_char gives us the start position of the line, so
            // we need to get the start position of the next line. In
            // the editor, this will correspond to the cursor being on
            // the EOL whitespace character, which is what we want.
            let mut end = text.line_to_char((end_line + 1).min(text.len_lines()));

            if start != range.from() {
                start = text.line_to_char((start_line + 1).min(text.len_lines()));
            }

            if end != range.to() {
                end = text.line_to_char(end_line);
            }

            Range::new(start, end).with_direction(range.direction())
        }),
    );
}

enum Operation {
    Delete,
    Change,
}

fn selection_is_linewise(selection: &Selection, text: &Rope) -> bool {
    selection.ranges().iter().all(|range| {
        let text = text.slice(..);
        if range.slice(text).len_lines() < 2 {
            return false;
        }
        // If the start of the selection is at the start of a line and the end at the end of a line.
        let (start_line, end_line) = range.line_range(text);
        let start = text.line_to_char(start_line);
        let end = text.line_to_char((end_line + 1).min(text.len_lines()));
        start == range.from() && end == range.to()
    })
}

enum YankAction {
    Yank,
    NoYank,
}

fn delete_selection_impl(cx: &mut Context, op: Operation, yank: YankAction) {
    let (view, doc) = current!(cx.editor);

    let selection = doc.selection(view.id);
    let only_whole_lines = selection_is_linewise(selection, doc.text());

    if cx.register != Some('_') && matches!(yank, YankAction::Yank) {
        // yank the selection
        let text = doc.text().slice(..);
        let values: Vec<String> = selection.fragments(text).map(Cow::into_owned).collect();
        let reg_name = cx
            .register
            .unwrap_or_else(|| cx.editor.config.load().default_yank_register);
        crate::emacs_kill::record(values.join("\n"));
        if let Err(err) = cx.editor.registers.write(reg_name, values) {
            cx.editor.set_error(err.to_string());
            return;
        }
    }

    // delete the selection
    let transaction =
        Transaction::delete_by_selection(doc.text(), selection, |range| (range.from(), range.to()));
    doc.apply(&transaction, view.id);

    match op {
        Operation::Delete => {
            // exit select mode, if currently in select mode
            exit_select_mode(cx);
        }
        Operation::Change => {
            if only_whole_lines {
                open(cx, Open::Above, CommentContinuation::Disabled);
            } else {
                enter_insert_mode(cx);
            }
        }
    }
}

#[inline]
fn delete_by_selection_insert_mode(
    cx: &mut Context,
    mut f: impl FnMut(RopeSlice, &Range) -> Deletion,
    direction: Direction,
) {
    let (view, doc) = current!(cx.editor);
    let text = doc.text().slice(..);
    let mut selection = SmallVec::new();
    let mut insert_newline = false;
    let text_len = text.len_chars();
    let mut transaction =
        Transaction::delete_by_selection(doc.text(), doc.selection(view.id), |range| {
            let (start, end) = f(text, range);
            if direction == Direction::Forward {
                let mut range = *range;
                if range.head > range.anchor {
                    insert_newline |= end == text_len;
                    // move the cursor to the right so that the selection
                    // doesn't shrink when deleting forward (so the text appears to
                    // move to  left)
                    // += 1 is enough here as the range is normalized to grapheme boundaries
                    // later anyway
                    range.head += 1;
                }
                selection.push(range);
            }
            (start, end)
        });

    // in case we delete the last character and the cursor would be moved to the EOF char
    // insert a newline, just like when entering append mode
    if insert_newline {
        transaction = transaction.insert_at_eof(doc.line_ending.as_str().into());
    }

    if direction == Direction::Forward {
        doc.set_selection(
            view.id,
            Selection::new(selection, doc.selection(view.id).primary_index()),
        );
    }
    doc.apply(&transaction, view.id);
}

fn delete_selection(cx: &mut Context) {
    delete_selection_impl(cx, Operation::Delete, YankAction::Yank);
}

fn delete_selection_noyank(cx: &mut Context) {
    delete_selection_impl(cx, Operation::Delete, YankAction::NoYank);
}

fn change_selection(cx: &mut Context) {
    delete_selection_impl(cx, Operation::Change, YankAction::Yank);
}

fn change_selection_noyank(cx: &mut Context) {
    delete_selection_impl(cx, Operation::Change, YankAction::NoYank);
}

fn collapse_selection(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);
    let text = doc.text().slice(..);

    let selection = doc.selection(view.id).clone().transform(|range| {
        let pos = range.cursor(text);
        Range::new(pos, pos)
    });
    doc.set_selection(view.id, selection);
}

fn flip_selections(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);

    let selection = doc
        .selection(view.id)
        .clone()
        .transform(|range| range.flip());
    doc.set_selection(view.id, selection);
}

/// Emacs `set-mark-command` (C-SPC): push the current point onto the mark ring
/// and activate the region (enter Select mode), so later `pop-to-mark` can
/// return here.
fn set_mark_command(cx: &mut Context) {
    let pos = {
        let (view, doc) = current!(cx.editor);
        let text = doc.text().slice(..);
        doc.selection(view.id).primary().cursor(text)
    };
    crate::emacs_mark::push(pos);
    select_mode(cx);
}

/// Emacs `pop-to-mark`/`pop-global-mark` (C-x C-SPC): jump point to the top of
/// the mark ring, rotating it so repeated pops walk back through prior marks.
fn pop_to_mark(cx: &mut Context) {
    let pos = {
        let (view, doc) = current!(cx.editor);
        let text = doc.text().slice(..);
        doc.selection(view.id).primary().cursor(text)
    };
    match crate::emacs_mark::pop_to(pos) {
        Some(target) => {
            let (view, doc) = current!(cx.editor);
            push_jump(view, doc);
            let target = target.min(doc.text().len_chars());
            doc.set_selection(view.id, Selection::point(target));
        }
        None => cx.editor.set_error("Mark ring is empty"),
    }
}

/// Emacs `point-to-register` (C-x r SPC): save point in the register read next.
fn point_to_register(cx: &mut Context) {
    cx.on_next_key(move |cx, event| {
        cx.editor.autoinfo = None;
        if let Some(ch) = event.char() {
            let (view, doc) = current!(cx.editor);
            let pos = doc.selection(view.id).primary().cursor(doc.text().slice(..));
            crate::emacs_register::set_pos(ch, pos);
            cx.editor.set_status(format!("Point saved to register {ch}"));
        }
    });
    cx.editor.autoinfo = Some(Info::new("Point to register", &[("char", "register name")]));
}

/// Emacs `jump-to-register` (C-x r j): jump to the position in the register read next.
fn jump_to_register(cx: &mut Context) {
    cx.on_next_key(move |cx, event| {
        cx.editor.autoinfo = None;
        if let Some(ch) = event.char() {
            match crate::emacs_register::get_pos(ch) {
                Some(pos) => {
                    let (view, doc) = current!(cx.editor);
                    let pos = pos.min(doc.text().len_chars());
                    push_jump(view, doc);
                    doc.set_selection(view.id, Selection::point(pos));
                }
                None => cx
                    .editor
                    .set_error(format!("Register {ch} does not hold a position")),
            }
        }
    });
    cx.editor.autoinfo = Some(Info::new("Jump to register", &[("char", "register name")]));
}

/// Emacs `number-to-register` (C-x r n): store the prefix count in a register.
fn number_to_register(cx: &mut Context) {
    let n = cx.count() as i64;
    cx.on_next_key(move |cx, event| {
        cx.editor.autoinfo = None;
        if let Some(ch) = event.char() {
            crate::emacs_register::set_num(ch, n);
            cx.editor.set_status(format!("Stored {n} in register {ch}"));
        }
    });
    cx.editor.autoinfo = Some(Info::new("Number to register", &[("char", "register name")]));
}

/// Emacs `increment-register` (C-x r +): add the prefix count to a number register.
fn increment_register(cx: &mut Context) {
    let by = cx.count() as i64;
    cx.on_next_key(move |cx, event| {
        cx.editor.autoinfo = None;
        if let Some(ch) = event.char() {
            let next = crate::emacs_register::incr(ch, by);
            cx.editor.set_status(format!("Register {ch} = {next}"));
        }
    });
    cx.editor.autoinfo = Some(Info::new("Increment register", &[("char", "register name")]));
}

/// Emacs `insert-register` (C-x r i): insert a number register's value as text.
fn emacs_insert_register(cx: &mut Context) {
    cx.on_next_key(move |cx, event| {
        cx.editor.autoinfo = None;
        if let Some(ch) = event.char() {
            match crate::emacs_register::get_num(ch) {
                Some(n) => {
                    let mode = cx.editor.mode;
                    let (view, doc) = current!(cx.editor);
                    paste_impl(&[n.to_string()], doc, view, Paste::Before, 1, mode);
                }
                None => cx
                    .editor
                    .set_error(format!("Register {ch} does not hold a number")),
            }
        }
    });
    cx.editor.autoinfo = Some(Info::new("Insert register", &[("char", "register name")]));
}

#[derive(Clone, Copy)]
enum RectOp {
    Kill,
    Delete,
    Clear,
    CopyAsKill,
    Yank,
}

/// Split the document into lines without their trailing line ending.
fn doc_lines(text: &zemacs_core::Rope) -> Vec<String> {
    (0..text.len_lines())
        .map(|i| {
            let mut s: String = text.line(i).chars().collect();
            while s.ends_with('\n') || s.ends_with('\r') {
                s.pop();
            }
            s
        })
        .collect()
}

/// Shared driver for the emacs rectangle commands. Derives the rectangle from
/// the primary selection's two corners and rewrites the buffer in one undoable
/// step. Columns are character offsets within the line (LF assumed on rejoin).
fn rectangle_op(cx: &mut Context, op: RectOp) {
    let (view, doc) = current!(cx.editor);
    let text = doc.text().clone();
    let sel = doc.selection(view.id).primary();
    let (p0, p1) = (sel.from(), sel.to());
    let l0 = text.char_to_line(p0);
    let l1 = text.char_to_line(p1.max(p0).saturating_sub(1).max(p0));
    let col_of = |pos: usize| pos - text.line_to_char(text.char_to_line(pos));
    let (c0, c1) = (col_of(p0), col_of(p1));
    let lines = doc_lines(&text);
    let le = doc.line_ending.as_str();

    let new_lines = match op {
        RectOp::CopyAsKill => {
            crate::emacs_rect::save(crate::emacs_rect::extract(&lines, l0, l1, c0, c1));
            cx.editor.set_status("Rectangle copied");
            return;
        }
        RectOp::Kill => {
            crate::emacs_rect::save(crate::emacs_rect::extract(&lines, l0, l1, c0, c1));
            crate::emacs_rect::delete(&lines, l0, l1, c0, c1)
        }
        RectOp::Delete => crate::emacs_rect::delete(&lines, l0, l1, c0, c1),
        RectOp::Clear => crate::emacs_rect::clear(&lines, l0, l1, c0, c1),
        RectOp::Yank => {
            let rect = crate::emacs_rect::saved();
            if rect.is_empty() {
                cx.editor.set_error("No rectangle to yank");
                return;
            }
            crate::emacs_rect::yank(&lines, l0, c0, &rect)
        }
    };

    let new_text = new_lines.join(le);
    let old_len = text.len_chars();
    let transaction = Transaction::change(
        &text,
        std::iter::once((0, old_len, Some(Tendril::from(new_text.as_str())))),
    );
    let (view, doc) = current!(cx.editor);
    doc.apply(&transaction, view.id);
    let caret = p0.min(doc.text().len_chars());
    doc.set_selection(view.id, Selection::point(caret));
    doc.append_changes_to_history(view);
}

/// Emacs `kill-rectangle` (C-x r k): delete the rectangle, saving it for yank.
fn kill_rectangle(cx: &mut Context) {
    rectangle_op(cx, RectOp::Kill);
}
/// Emacs `delete-rectangle` (C-x r d): delete the rectangle without saving.
fn delete_rectangle(cx: &mut Context) {
    rectangle_op(cx, RectOp::Delete);
}
/// Emacs `clear-rectangle` (C-x r c): blank the rectangle with spaces.
fn clear_rectangle(cx: &mut Context) {
    rectangle_op(cx, RectOp::Clear);
}
/// Emacs `copy-rectangle-as-kill` (C-x r M-w): save the rectangle without deleting.
fn copy_rectangle_as_kill(cx: &mut Context) {
    rectangle_op(cx, RectOp::CopyAsKill);
}
/// Emacs `yank-rectangle` (C-x r y): insert the saved rectangle at point.
fn yank_rectangle(cx: &mut Context) {
    rectangle_op(cx, RectOp::Yank);
}

/// Emacs `bookmark-set` (C-x r m): prompt for a name and store the current
/// file + point as a persistent bookmark.
fn bookmark_set(cx: &mut Context) {
    let (file, pos) = {
        let (view, doc) = current!(cx.editor);
        let pos = doc.selection(view.id).primary().cursor(doc.text().slice(..));
        (doc.path().map(|p| p.to_path_buf()), pos)
    };
    let Some(file) = file else {
        cx.editor.set_error("Buffer has no file to bookmark");
        return;
    };
    ui::prompt(
        cx,
        "bookmark name: ".into(),
        None,
        |_, _| Vec::new(),
        move |cx, input, event| {
            if event != PromptEvent::Validate || input.is_empty() {
                return;
            }
            crate::emacs_bookmark::set(input, &file, pos);
            cx.editor.set_status(format!("Bookmark '{input}' set"));
        },
    );
}

/// Emacs `bookmark-jump`/`list-bookmarks` (C-x r b / C-x r l): pick a bookmark
/// and jump to its file and position.
fn bookmark_jump(cx: &mut Context) {
    let marks = crate::emacs_bookmark::list();
    if marks.is_empty() {
        cx.editor
            .set_status("No bookmarks yet — set one with C-x r m");
        return;
    }
    let columns = [
        PickerColumn::new("name", |item: &(String, PathBuf, usize), _: &()| {
            item.0.clone().into()
        }),
        PickerColumn::new("file", |item: &(String, PathBuf, usize), _: &()| {
            item.1.display().to_string().into()
        }),
    ];
    let picker = Picker::new(
        columns,
        0,
        marks,
        (),
        |cx, item: &(String, PathBuf, usize), action| {
            if let Err(e) = cx.editor.open(&item.1, action) {
                cx.editor
                    .set_error(format!("unable to open \"{}\": {e}", item.1.display()));
                return;
            }
            let (view, doc) = current!(cx.editor);
            let pos = item.2.min(doc.text().len_chars());
            doc.set_selection(view.id, Selection::point(pos));
        },
    );
    cx.push_layer(Box::new(overlaid(picker)));
}

/// Emacs `add-global-abbrev` (C-x a g): prompt `<abbrev> <expansion...>` and
/// define it in the persistent abbrev table.
fn define_abbrev(cx: &mut Context) {
    ui::prompt(
        cx,
        "define abbrev (name expansion): ".into(),
        None,
        |_, _| Vec::new(),
        move |cx, input, event| {
            if event != PromptEvent::Validate || input.trim().is_empty() {
                return;
            }
            match input.trim().split_once(char::is_whitespace) {
                Some((name, exp)) => {
                    crate::emacs_abbrev::define(name, exp.trim_start());
                    cx.editor.set_status(format!("Abbrev '{name}' defined"));
                }
                None => cx.editor.set_error("Usage: <abbrev> <expansion...>"),
            }
        },
    );
}

/// Emacs `expand-abbrev` (C-x '): expand the word before point if it is a
/// defined abbrev.
fn expand_abbrev(cx: &mut Context) {
    let (start, word) = {
        let (view, doc) = current!(cx.editor);
        let text = doc.text().slice(..);
        let cursor = doc.selection(view.id).primary().cursor(text);
        let mut start = cursor;
        while start > 0 {
            let ch = text.char(start - 1);
            if ch.is_alphanumeric() || ch == '_' {
                start -= 1;
            } else {
                break;
            }
        }
        let word: String = text.slice(start..cursor).chars().collect();
        (start, word)
    };
    if word.is_empty() {
        cx.editor.set_error("No word before cursor");
        return;
    }
    let Some(expansion) = crate::emacs_abbrev::get(&word) else {
        cx.editor.set_error(format!("'{word}' is not an abbrev"));
        return;
    };
    let (view, doc) = current!(cx.editor);
    let cursor = doc.selection(view.id).primary().cursor(doc.text().slice(..));
    let transaction = Transaction::change(
        doc.text(),
        std::iter::once((start, cursor, Some(Tendril::from(expansion.as_str())))),
    );
    doc.apply(&transaction, view.id);
    let new_pos = start + expansion.chars().count();
    doc.set_selection(view.id, Selection::point(new_pos));
    doc.append_changes_to_history(view);
}

fn ensure_selections_forward(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);

    let selection = doc
        .selection(view.id)
        .clone()
        .transform(|r| r.with_direction(Direction::Forward));

    doc.set_selection(view.id, selection);
}

/// Char offset where the current insert session began, and the text of the most
/// recently completed insert session — backing vim's `i_CTRL-A`/`i_CTRL-@`
/// (insert previously-inserted text). The text is captured on leaving insert as
/// the span from the insert anchor to the exit cursor (a close approximation of
/// vim's `.` register; intra-insert cursor jumps make it best-effort).
static INSERT_ANCHOR: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static LAST_INSERTED_TEXT: Lazy<std::sync::Mutex<String>> =
    Lazy::new(|| std::sync::Mutex::new(String::new()));

fn enter_insert_mode(cx: &mut Context) {
    cx.editor.mode = Mode::Insert;
    let (view, doc) = current!(cx.editor);
    let pos = doc.selection(view.id).primary().cursor(doc.text().slice(..));
    INSERT_ANCHOR.store(pos, std::sync::atomic::Ordering::Relaxed);
}

// inserts at the start of each selection
fn insert_mode(cx: &mut Context) {
    enter_insert_mode(cx);
    let (view, doc) = current!(cx.editor);

    log::trace!(
        "entering insert mode with sel: {:?}, text: {:?}",
        doc.selection(view.id),
        doc.text().to_string()
    );

    let selection = doc
        .selection(view.id)
        .clone()
        .transform(|range| Range::new(range.to(), range.from()));

    doc.set_selection(view.id, selection);
}

// vim Replace mode (`R`): enter insert mode in overtype mode, so typed
// characters replace existing ones until <esc>.
fn replace_mode(cx: &mut Context) {
    insert_mode(cx);
    cx.editor.overwrite = true;
}

// inserts at the end of each selection
fn append_mode(cx: &mut Context) {
    enter_insert_mode(cx);
    let (view, doc) = current!(cx.editor);
    doc.restore_cursor = true;
    let text = doc.text().slice(..);

    // Make sure there's room at the end of the document if the last
    // selection butts up against it.
    let end = text.len_chars();
    let last_range = doc
        .selection(view.id)
        .iter()
        .last()
        .expect("selection should always have at least one range");
    if !last_range.is_empty() && last_range.to() == end {
        let transaction = Transaction::change(
            doc.text(),
            [(end, end, Some(doc.line_ending.as_str().into()))].into_iter(),
        );
        doc.apply(&transaction, view.id);
    }

    let selection = doc.selection(view.id).clone().transform(|range| {
        Range::new(
            range.from(),
            graphemes::next_grapheme_boundary(doc.text().slice(..), range.to()),
        )
    });
    doc.set_selection(view.id, selection);
}

fn file_picker(cx: &mut Context) {
    let root = find_workspace().0;
    if !root.exists() {
        cx.editor.set_error("Workspace directory does not exist");
        return;
    }
    let picker = ui::file_picker(cx.editor, root);
    cx.push_layer(Box::new(overlaid(picker)));
}

fn file_picker_in_current_buffer_directory(cx: &mut Context) {
    let doc_dir = doc!(cx.editor)
        .path()
        .and_then(|path| path.parent().map(|path| path.to_path_buf()));

    let path = match doc_dir {
        Some(path) => path,
        None => {
            let cwd = zemacs_stdx::env::current_working_dir();
            if !cwd.exists() {
                cx.editor.set_error(
                    "Current buffer has no parent and current working directory does not exist",
                );
                return;
            }
            cx.editor.set_error(
                "Current buffer has no parent, opening file picker in current working directory",
            );
            cwd
        }
    };

    let picker = ui::file_picker(cx.editor, path);
    cx.push_layer(Box::new(overlaid(picker)));
}

fn file_picker_in_current_directory(cx: &mut Context) {
    let cwd = zemacs_stdx::env::current_working_dir();
    if !cwd.exists() {
        cx.editor
            .set_error("Current working directory does not exist");
        return;
    }
    let picker = ui::file_picker(cx.editor, cwd);
    cx.push_layer(Box::new(overlaid(picker)));
}

fn file_explorer(cx: &mut Context) {
    let root = find_workspace().0;
    if !root.exists() {
        cx.editor.set_error("Workspace directory does not exist");
        return;
    }

    if let Ok(picker) = ui::file_explorer(root, cx.editor) {
        cx.push_layer(Box::new(overlaid(picker)));
    }
}

fn file_explorer_in_current_buffer_directory(cx: &mut Context) {
    let doc_dir = doc!(cx.editor)
        .path()
        .and_then(|path| path.parent().map(|path| path.to_path_buf()));

    let path = match doc_dir {
        Some(path) => path,
        None => {
            let cwd = zemacs_stdx::env::current_working_dir();
            if !cwd.exists() {
                cx.editor.set_error(
                    "Current buffer has no parent and current working directory does not exist",
                );
                return;
            }
            cx.editor.set_error(
                "Current buffer has no parent, opening file explorer in current working directory",
            );
            cwd
        }
    };

    if let Ok(picker) = ui::file_explorer(path, cx.editor) {
        cx.push_layer(Box::new(overlaid(picker)));
    }
}

fn file_explorer_in_current_directory(cx: &mut Context) {
    let cwd = zemacs_stdx::env::current_working_dir();
    if !cwd.exists() {
        cx.editor
            .set_error("Current working directory does not exist");
        return;
    }

    if let Ok(picker) = ui::file_explorer(cwd, cx.editor) {
        cx.push_layer(Box::new(overlaid(picker)));
    }
}

struct PathStyleConfig {
    directory_style: Style,
    number_style: Style,
    colon_style: Style,
}

impl PathStyleConfig {
    fn new(theme: &zemacs_view::Theme) -> Self {
        Self {
            directory_style: theme.get("ui.text.directory"),
            number_style: theme.get("constant.numeric.integer"),
            colon_style: theme.get("punctuation"),
        }
    }

    fn stylize<'a>(&self, path: Option<&'a Path>, line: Option<usize>) -> Cell<'a> {
        let mut spans = Vec::new();
        if let Some(path) = path {
            let directories = path
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .map(|p| format!("{}{}", p.display(), std::path::MAIN_SEPARATOR))
                .unwrap_or_default();
            spans.push(Span::styled(directories, self.directory_style));
        }
        let filename = path.as_ref().map_or(SCRATCH_BUFFER_NAME.into(), |path| {
            path.file_name()
                .expect("all document names are normalized (can't end in `..`)")
                .to_string_lossy()
        });
        spans.push(Span::raw(filename));
        if let Some(line) = line {
            spans.extend([
                Span::styled(":", self.colon_style),
                Span::styled((line + 1).to_string(), self.number_style),
            ]);
        }

        Cell::from(Spans::from(spans))
    }
}

fn buffer_picker(cx: &mut Context) {
    buffer_picker_impl(cx, None, false);
}

/// SPC b N C-i : open an existing buffer in a split — a second view of that buffer's shared
/// Document, i.e. an indirect buffer of it (Spacemacs `make-indirect-buffer` from a buffer).
fn clone_indirect_from_buffer(cx: &mut Context) {
    buffer_picker_impl(cx, Some(Action::VerticalSplit), false);
}

/// SPC b w : go to the window already displaying the chosen buffer (focus it instead of reopening),
/// falling back to opening it in the current window. Spacemacs `spacemacs/goto-buffer-window`.
fn goto_buffer_window(cx: &mut Context) {
    buffer_picker_impl(cx, None, true);
}

/// Shared body for the buffer picker. When `force_action` is set, the chosen buffer always opens
/// with that action (e.g. always in a split). When `goto_window` is set, an existing window
/// already showing the buffer is focused instead of opening it again. Otherwise the picker's own
/// open action is used.
fn buffer_picker_impl(cx: &mut Context, force_action: Option<Action>, goto_window: bool) {
    let current = view!(cx.editor).doc;

    struct BufferMeta<'a> {
        id: DocumentId,
        path: Option<Cow<'a, Path>>,
        is_modified: bool,
        is_current: bool,
        focused_at: std::time::Instant,
    }

    let new_meta = |doc: &Document| BufferMeta {
        id: doc.id(),
        path: doc
            .path()
            .map(ToOwned::to_owned)
            .map(zemacs_stdx::path::get_relative_path),
        is_modified: doc.is_modified(),
        is_current: doc.id() == current,
        focused_at: doc.focused_at,
    };

    let mut items = cx
        .editor
        .documents
        .values()
        .map(new_meta)
        .collect::<Vec<BufferMeta>>();

    // mru
    items.sort_unstable_by_key(|item| std::cmp::Reverse(item.focused_at));

    let columns = [
        PickerColumn::new("id", |meta: &BufferMeta, _| meta.id.to_string().into()),
        PickerColumn::new("flags", |meta: &BufferMeta, _| {
            let mut flags = String::new();
            if meta.is_modified {
                flags.push('+');
            }
            if meta.is_current {
                flags.push('*');
            }
            flags.into()
        }),
        PickerColumn::new("path", |meta: &BufferMeta, config: &PathStyleConfig| {
            config.stylize(meta.path.as_deref(), None)
        }),
    ];

    let initial_cursor = if cx
        .editor
        .config()
        .buffer_picker
        .start_position
        .is_previous()
        && !items.is_empty()
    {
        1
    } else {
        0
    };

    let picker = Picker::new(
        columns,
        2,
        items,
        PathStyleConfig::new(&cx.editor.theme),
        move |cx, meta, action| {
            if goto_window {
                let existing = cx
                    .editor
                    .tree
                    .traverse()
                    .find(|(_, v)| v.doc == meta.id)
                    .map(|(id, _)| id);
                if let Some(view_id) = existing {
                    cx.editor.focus(view_id);
                    return;
                }
            }
            cx.editor.switch(meta.id, force_action.unwrap_or(action));
        },
    )
    .with_initial_cursor(initial_cursor)
    .with_preview(|editor, meta| {
        let doc = &editor.documents.get(&meta.id)?;
        let lines = doc.selections().values().next().map(|selection| {
            let cursor_line = selection.primary().cursor_line(doc.text().slice(..));
            (cursor_line, cursor_line)
        });
        Some((meta.id.into(), lines))
    });
    cx.push_layer(Box::new(overlaid(picker)));
}

fn jumplist_picker(cx: &mut Context) {
    struct JumpMeta<'a> {
        id: DocumentId,
        path: Option<Cow<'a, Path>>,
        selection: Selection,
        line_start: usize,
        text: String,
        is_current: bool,
    }

    for (view, _) in cx.editor.tree.views_mut() {
        for doc_id in view.jumps.iter().map(|e| e.0).collect::<Vec<_>>().iter() {
            let doc = doc_mut!(cx.editor, doc_id);
            view.sync_changes(doc);
        }
    }

    let new_meta = |view: &View, doc_id: DocumentId, selection: Selection| {
        let doc = doc!(cx.editor, &doc_id);
        let text = doc.text().slice(..);
        let contents = selection
            .fragments(text)
            .map(Cow::into_owned)
            .collect::<Vec<_>>()
            .join(" ");
        let line_start = selection.primary().cursor_line(text);

        JumpMeta {
            id: doc_id,
            path: doc
                .path()
                .map(ToOwned::to_owned)
                .map(zemacs_stdx::path::get_relative_path),
            selection,
            line_start,
            text: contents,
            is_current: view.doc == doc_id,
        }
    };

    let columns = [
        ui::PickerColumn::new("id", |item: &JumpMeta, _| item.id.to_string().into()),
        ui::PickerColumn::new("path", |item: &JumpMeta, config: &PathStyleConfig| {
            config.stylize(item.path.as_deref(), Some(item.line_start))
        }),
        ui::PickerColumn::new("flags", |item: &JumpMeta, _| {
            let mut flags = Vec::new();
            if item.is_current {
                flags.push("*");
            }

            if flags.is_empty() {
                "".into()
            } else {
                format!(" ({})", flags.join("")).into()
            }
        }),
        ui::PickerColumn::new("contents", |item: &JumpMeta, _| item.text.as_str().into()),
    ];

    let picker = Picker::new(
        columns,
        1, // path
        cx.editor.tree.views().flat_map(|(view, _)| {
            view.jumps
                .iter()
                .rev()
                .map(|(doc_id, selection)| new_meta(view, *doc_id, selection.clone()))
        }),
        PathStyleConfig::new(&cx.editor.theme),
        |cx, meta, action| {
            cx.editor.switch(meta.id, action);
            let config = cx.editor.config();
            let (view, doc) = (view_mut!(cx.editor), doc_mut!(cx.editor, &meta.id));
            doc.set_selection(view.id, meta.selection.clone());
            if action.align_view(view, doc.id()) {
                view.ensure_cursor_in_view_center(doc, config.scrolloff);
            }
        },
    )
    .with_preview(|editor, meta| {
        let doc = &editor.documents.get(&meta.id)?;
        let line = meta.selection.primary().cursor_line(doc.text().slice(..));
        Some((meta.id.into(), Some((line, line))))
    });
    cx.push_layer(Box::new(overlaid(picker)));
}

/// Pin the current file to the project's harpoon list (jump to it later with
/// `harpoon_jump`/`SPC H 1..9` or the menu). Idempotent.
fn harpoon_add(cx: &mut Context) {
    let Some(path) = doc!(cx.editor).path().map(|p| p.to_path_buf()) else {
        cx.editor.set_error("Cannot pin a scratch buffer");
        return;
    };
    let slot = crate::harpoon::add(&path);
    let name = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    cx.editor.set_status(format!("Harpoon {slot}: {name}"));
}

/// Open the harpoon mark at 1-based slot `n` (shared by the count + slot commands).
fn harpoon_open_slot(cx: &mut Context, n: usize) {
    match crate::harpoon::get(n) {
        Some(path) => {
            if let Err(e) = cx.editor.open(&path, Action::Replace) {
                cx.editor
                    .set_error(format!("unable to open \"{}\": {e}", path.display()));
            }
        }
        None => cx.editor.set_status(format!("No harpoon mark in slot {n}")),
    }
}

/// Jump to the harpoon mark at 1-based slot `cx.count` (default 1).
fn harpoon_jump(cx: &mut Context) {
    let n = cx.count.map(|c| c.get()).unwrap_or(1);
    harpoon_open_slot(cx, n);
}

fn harpoon_1(cx: &mut Context) {
    harpoon_open_slot(cx, 1);
}
fn harpoon_2(cx: &mut Context) {
    harpoon_open_slot(cx, 2);
}
fn harpoon_3(cx: &mut Context) {
    harpoon_open_slot(cx, 3);
}
fn harpoon_4(cx: &mut Context) {
    harpoon_open_slot(cx, 4);
}

/// Open the next / previous harpoon mark relative to the current file (wrapping).
/// If the current file isn't pinned, jumps to the first mark.
fn harpoon_cycle(cx: &mut Context, forward: bool) {
    let marks = crate::harpoon::list();
    if marks.is_empty() {
        cx.editor.set_status("No harpoon marks yet");
        return;
    }
    let cur = doc!(cx.editor).path().and_then(|p| {
        let cp = std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
        marks.iter().position(|m| *m == cp)
    });
    let len = marks.len();
    let idx = match cur {
        Some(i) if forward => (i + 1) % len,
        Some(i) => (i + len - 1) % len,
        None => 0,
    };
    let path = marks[idx].clone();
    if let Err(e) = cx.editor.open(&path, Action::Replace) {
        cx.editor
            .set_error(format!("unable to open \"{}\": {e}", path.display()));
    }
}

/// Open the next harpoon mark (wraps).
fn harpoon_next(cx: &mut Context) {
    harpoon_cycle(cx, true);
}

/// Open the previous harpoon mark (wraps).
fn harpoon_prev(cx: &mut Context) {
    harpoon_cycle(cx, false);
}

/// Fuzzy menu of the project's harpoon marks; Enter opens, and the order is the
/// pin order (slot 1 at the top).
fn harpoon_menu(cx: &mut Context) {
    let marks = crate::harpoon::list();
    if marks.is_empty() {
        cx.editor
            .set_status("No harpoon marks yet — pin one with harpoon_add");
        return;
    }
    let cwd = zemacs_stdx::env::current_working_dir();
    // pair each mark with its 1-based slot so the picker shows the `SPC H N` mapping
    let items: Vec<(usize, PathBuf)> = marks
        .into_iter()
        .enumerate()
        .map(|(i, p)| (i + 1, p))
        .collect();
    let columns = [
        PickerColumn::new("#", |item: &(usize, PathBuf), _: &PathBuf| {
            item.0.to_string().into()
        }),
        PickerColumn::new("pinned file", |item: &(usize, PathBuf), cwd: &PathBuf| {
            item.1
                .strip_prefix(cwd)
                .unwrap_or(&item.1)
                .display()
                .to_string()
                .into()
        }),
    ];
    let picker = Picker::new(
        columns,
        1,
        items,
        cwd,
        |cx, item: &(usize, PathBuf), action| {
            if let Err(e) = cx.editor.open(&item.1, action) {
                cx.editor
                    .set_error(format!("unable to open \"{}\": {e}", item.1.display()));
            }
        },
    )
    .with_preview(|_editor, item| Some((item.1.as_path().into(), None)));
    cx.push_layer(Box::new(overlaid(picker)));
}

/// Remove the current file from the project's harpoon list.
fn harpoon_remove(cx: &mut Context) {
    let Some(path) = doc!(cx.editor).path().map(|p| p.to_path_buf()) else {
        return;
    };
    crate::harpoon::remove(&path);
    cx.editor.set_status("Unpinned from harpoon");
}

/// Pick a local git branch and check it out (magit `b`), reloading open buffers.
pub(crate) fn git_branch_picker(cx: &mut Context) {
    let dir = std::env::current_dir().unwrap_or_default();
    let branches: Vec<String> = std::process::Command::new("git")
        .arg("-C")
        .arg(&dir)
        .args(["for-each-ref", "--format=%(refname:short)", "refs/heads/"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();
    if branches.is_empty() {
        cx.editor.set_status("No git branches");
        return;
    }
    let columns = [PickerColumn::new("branch", |b: &String, _: &()| {
        b.as_str().into()
    })];
    let picker = Picker::new(columns, 0, branches, (), |cx, branch: &String, _action| {
        let dir = std::env::current_dir().unwrap_or_default();
        match std::process::Command::new("git")
            .arg("-C")
            .arg(&dir)
            .args(["checkout", branch])
            .output()
        {
            Ok(o) if o.status.success() => {
                crate::commands::typed::reload_open_docs(cx);
                cx.editor.set_status(format!("Switched to branch {branch}"));
            }
            Ok(o) => cx.editor.set_error(
                String::from_utf8_lossy(&o.stderr)
                    .lines()
                    .next()
                    .unwrap_or("checkout failed")
                    .trim()
                    .to_owned(),
            ),
            Err(e) => cx.editor.set_error(format!("git: {e}")),
        }
    });
    cx.push_layer(Box::new(overlaid(picker)));
}

/// Reopen the most-recently-closed file (the IDE `Ctrl-Shift-T` gesture).
/// Repeated calls walk back through the session's close history.
fn reopen_last_closed(cx: &mut Context) {
    match crate::closed_files::pop() {
        Some(path) => {
            if let Err(e) = cx.editor.open(&path, Action::Replace) {
                cx.editor
                    .set_error(format!("unable to reopen \"{}\": {e}", path.display()));
            }
        }
        None => cx.editor.set_status("No recently closed file to reopen"),
    }
}

/// Fuzzy-pick a previously opened file, ranked by `z`-style frecency
/// (frequency × recency). Persisted across sessions in `<config>/recent_files`.
fn frecent_file_picker(cx: &mut Context) {
    let files = crate::recent_files::load_frecent();
    if files.is_empty() {
        cx.editor.set_status("No recent files yet");
        return;
    }
    let cwd = zemacs_stdx::env::current_working_dir();
    let columns = [PickerColumn::new("file", |p: &PathBuf, cwd: &PathBuf| {
        p.strip_prefix(cwd)
            .unwrap_or(p)
            .display()
            .to_string()
            .into()
    })];
    let picker = Picker::new(columns, 0, files, cwd, |cx, path: &PathBuf, action| {
        if let Err(e) = cx.editor.open(path, action) {
            cx.editor
                .set_error(format!("unable to open \"{}\": {e}", path.display()));
        }
    })
    .with_preview(|_editor, path| Some((path.as_path().into(), None)));
    cx.push_layer(Box::new(overlaid(picker)));
}

fn changed_file_picker(cx: &mut Context) {
    pub struct FileChangeData {
        cwd: PathBuf,
        style_untracked: Style,
        style_modified: Style,
        style_conflict: Style,
        style_deleted: Style,
        style_renamed: Style,
    }

    let cwd = zemacs_stdx::env::current_working_dir();
    if !cwd.exists() {
        cx.editor
            .set_error("Current working directory does not exist");
        return;
    }

    let added = cx.editor.theme.get("diff.plus");
    let modified = cx.editor.theme.get("diff.delta");
    let conflict = cx.editor.theme.get("diff.delta.conflict");
    let deleted = cx.editor.theme.get("diff.minus");
    let renamed = cx.editor.theme.get("diff.delta.moved");

    let columns = [
        PickerColumn::new("change", |change: &FileChange, data: &FileChangeData| {
            match change {
                FileChange::Untracked { .. } => Span::styled("+ untracked", data.style_untracked),
                FileChange::Modified { .. } => Span::styled("~ modified", data.style_modified),
                FileChange::Conflict { .. } => Span::styled("x conflict", data.style_conflict),
                FileChange::Deleted { .. } => Span::styled("- deleted", data.style_deleted),
                FileChange::Renamed { .. } => Span::styled("> renamed", data.style_renamed),
            }
            .into()
        }),
        PickerColumn::new("path", |change: &FileChange, data: &FileChangeData| {
            let display_path = |path: &PathBuf| {
                path.strip_prefix(&data.cwd)
                    .unwrap_or(path)
                    .display()
                    .to_string()
            };
            match change {
                FileChange::Untracked { path } => display_path(path),
                FileChange::Modified { path } => display_path(path),
                FileChange::Conflict { path } => display_path(path),
                FileChange::Deleted { path } => display_path(path),
                FileChange::Renamed { from_path, to_path } => {
                    format!("{} -> {}", display_path(from_path), display_path(to_path))
                }
            }
            .into()
        }),
    ];

    let picker = Picker::new(
        columns,
        1, // path
        [],
        FileChangeData {
            cwd: cwd.clone(),
            style_untracked: added,
            style_modified: modified,
            style_conflict: conflict,
            style_deleted: deleted,
            style_renamed: renamed,
        },
        |cx, meta: &FileChange, action| {
            let path_to_open = meta.path();
            if let Err(e) = cx.editor.open(path_to_open, action) {
                let err = if let Some(err) = e.source() {
                    format!("{}", err)
                } else {
                    format!("unable to open \"{}\"", path_to_open.display())
                };
                cx.editor.set_error(err);
            }
        },
    )
    .with_preview(|_editor, meta| Some((meta.path().into(), None)));
    let injector = picker.injector();

    let trust_full = cx
        .editor
        .workspace_trust
        .query(
            &zemacs_loader::find_workspace_in(&cwd).0,
            zemacs_loader::workspace_trust::TrustQuery::Git,
        )
        .is_trusted();
    cx.editor
        .diff_providers
        .clone()
        .for_each_changed_file(cwd, trust_full, move |change| match change {
            Ok(change) => injector.push(change).is_ok(),
            Err(err) => {
                status::report_blocking(err);
                true
            }
        });
    cx.push_layer(Box::new(overlaid(picker)));
}

pub fn command_palette(cx: &mut Context) {
    let register = cx.register;
    let count = cx.count;

    cx.callback.push(Box::new(
        move |compositor: &mut Compositor, cx: &mut compositor::Context| {
            let keymap = compositor.find::<ui::EditorView>().unwrap().keymaps.map()
                [&cx.editor.mode]
                .reverse_map();

            let commands = MappableCommand::STATIC_COMMAND_LIST.iter().cloned().chain(
                typed::TYPABLE_COMMAND_LIST
                    .iter()
                    .map(|cmd| MappableCommand::Typable {
                        name: cmd.name.to_owned(),
                        args: String::new(),
                        doc: cmd.doc.to_owned(),
                    }),
            );

            let columns = [
                ui::PickerColumn::new("name", |item, _| match item {
                    MappableCommand::Typable { name, .. } => format!(":{name}").into(),
                    MappableCommand::Static { name, .. } => (*name).into(),
                    MappableCommand::Macro { .. } => {
                        unreachable!("macros aren't included in the command palette")
                    }
                }),
                ui::PickerColumn::new(
                    "bindings",
                    |item: &MappableCommand, keymap: &crate::keymap::ReverseKeymap| {
                        keymap
                            .get(item.name())
                            .map(|bindings| {
                                bindings.iter().fold(String::new(), |mut acc, bind| {
                                    if !acc.is_empty() {
                                        acc.push(' ');
                                    }
                                    for key in bind {
                                        acc.push_str(&key.key_sequence_format());
                                    }
                                    acc
                                })
                            })
                            .unwrap_or_default()
                            .into()
                    },
                ),
                ui::PickerColumn::new("doc", |item: &MappableCommand, _| item.doc().into()),
            ];

            let picker = Picker::new(columns, 0, commands, keymap, move |cx, command, _action| {
                let mut ctx = Context {
                    register,
                    count,
                    editor: cx.editor,
                    callback: Vec::new(),
                    on_next_key_callback: None,
                    jobs: cx.jobs,
                };
                let focus = view!(ctx.editor).id;

                command.execute(&mut ctx);

                if ctx.editor.tree.contains(focus) {
                    let config = ctx.editor.config();
                    let mode = ctx.editor.mode();
                    let view = view_mut!(ctx.editor, focus);
                    let doc = doc_mut!(ctx.editor, &view.doc);

                    view.ensure_cursor_in_view(doc, config.scrolloff);

                    if mode != Mode::Insert {
                        doc.append_changes_to_history(view);
                    }
                }
            });
            compositor.push(Box::new(overlaid(picker)));
        },
    ));
}

fn last_picker(cx: &mut Context) {
    // TODO: last picker does not seem to work well with buffer_picker
    cx.callback.push(Box::new(|compositor, cx| {
        if let Some(picker) = compositor.last_picker.take() {
            compositor.push(picker);
        } else {
            cx.editor.set_error("no last picker")
        }
    }));
}

/// Fallback position to use for [`insert_with_indent`].
enum IndentFallbackPos {
    LineStart,
    LineEnd,
}

// `I` inserts at the first nonwhitespace character of each line with a selection.
// If the line is empty, automatically indent.
fn insert_at_line_start(cx: &mut Context) {
    insert_with_indent(cx, IndentFallbackPos::LineStart);
}

// `A` inserts at the end of each line with a selection.
// If the line is empty, automatically indent.
fn insert_at_line_end(cx: &mut Context) {
    insert_with_indent(cx, IndentFallbackPos::LineEnd);
}

// Enter insert mode and auto-indent the current line if it is empty.
// If the line is not empty, move the cursor to the specified fallback position.
fn insert_with_indent(cx: &mut Context, cursor_fallback: IndentFallbackPos) {
    let was_select_mode = cx.editor.mode == Mode::Select;
    enter_insert_mode(cx);

    let (view, doc) = current!(cx.editor);
    let loader = cx.editor.syn_loader.load();

    let text = doc.text().slice(..);
    let contents = doc.text();
    let selection = doc.selection(view.id);

    let syntax = doc.syntax();
    let tab_width = doc.tab_width();

    let mut ranges = SmallVec::with_capacity(selection.len());
    let mut offs = 0;

    let mut transaction = Transaction::change_by_selection(contents, selection, |range| {
        let cursor_line = range.cursor_line(text);
        let cursor_line_start = text.line_to_char(cursor_line);

        if line_end_char_index(&text, cursor_line) == cursor_line_start {
            // line is empty => auto indent
            let line_end_index = cursor_line_start;

            let indent = indent::indent_for_newline(
                &loader,
                syntax,
                &doc.config.load().indent_heuristic,
                &doc.indent_style,
                tab_width,
                text,
                cursor_line,
                line_end_index,
                cursor_line,
            );

            // calculate new selection ranges
            let pos = offs + cursor_line_start;
            let indent_width = indent.chars().count();
            ranges.push(Range::point(pos + indent_width));
            offs += indent_width;

            (line_end_index, line_end_index, Some(indent.into()))
        } else {
            // move cursor to the fallback position
            let pos = match cursor_fallback {
                IndentFallbackPos::LineStart => text
                    .line(cursor_line)
                    .first_non_whitespace_char()
                    .map(|ws_offset| ws_offset + cursor_line_start)
                    .unwrap_or(cursor_line_start),
                IndentFallbackPos::LineEnd => line_end_char_index(&text, cursor_line),
            };

            ranges.push(range.put_cursor(text, pos + offs, was_select_mode));

            (cursor_line_start, cursor_line_start, None)
        }
    });

    transaction = transaction.with_selection(Selection::new(ranges, selection.primary_index()));
    doc.apply(&transaction, view.id);
}

// Creates an LspCallback that waits for formatting changes to be computed. When they're done,
// it applies them, but only if the doc hasn't changed.
//
// TODO: provide some way to cancel this, probably as part of a more general job cancellation
// scheme
async fn make_format_callback(
    doc_id: DocumentId,
    doc_version: i32,
    view_id: ViewId,
    format: impl Future<Output = Result<Transaction, FormatterError>> + Send + 'static,
    write: Option<(Option<PathBuf>, bool)>,
) -> anyhow::Result<job::Callback> {
    let format = format.await;

    let call: job::Callback = Callback::Editor(Box::new(move |editor| {
        if !editor.documents.contains_key(&doc_id) || !editor.tree.contains(view_id) {
            return;
        }

        let scrolloff = editor.config().scrolloff;
        let doc = doc_mut!(editor, &doc_id);
        let view = view_mut!(editor, view_id);

        match format {
            Ok(format) => {
                if doc.version() == doc_version {
                    doc.apply(&format, view.id);
                    doc.append_changes_to_history(view);
                    doc.detect_indent_and_line_ending();
                    view.ensure_cursor_in_view(doc, scrolloff);
                } else {
                    log::info!("discarded formatting changes because the document changed");
                }
            }
            Err(err) => {
                if write.is_none() {
                    editor.set_error(err.to_string());
                    return;
                }
                log::info!("failed to format '{}': {err}", doc.display_name());
            }
        }

        if let Some((path, force)) = write {
            let id = doc.id();
            if let Err(err) = editor.save(id, path, force) {
                editor.set_error(format!("Error saving: {}", err));
            }
        }
    }));

    Ok(call)
}

#[derive(PartialEq, Eq)]
pub enum Open {
    Below,
    Above,
}

#[derive(PartialEq)]
pub enum CommentContinuation {
    Enabled,
    Disabled,
}

fn open(cx: &mut Context, open: Open, comment_continuation: CommentContinuation) {
    let count = cx.count();
    enter_insert_mode(cx);
    let config = cx.editor.config();
    let (view, doc) = current!(cx.editor);
    let loader = cx.editor.syn_loader.load();

    let text = doc.text().slice(..);
    let contents = doc.text();
    let selection = doc.selection(view.id);
    let mut offs = 0;

    let mut ranges = SmallVec::with_capacity(selection.len());

    let mut transaction = Transaction::change_by_selection(contents, selection, |range| {
        // the line number, where the cursor is currently
        let curr_line_num = text.char_to_line(match open {
            Open::Below => graphemes::prev_grapheme_boundary(text, range.to()),
            Open::Above => range.from(),
        });

        // the next line number, where the cursor will be, after finishing the transaction
        let next_new_line_num = match open {
            Open::Below => curr_line_num + 1,
            Open::Above => curr_line_num,
        };

        let above_next_new_line_num = next_new_line_num.saturating_sub(1);

        // Continue the comment leader using the comment tokens of the layer at the current line.
        let continue_comment_token =
            if comment_continuation == CommentContinuation::Enabled && config.continue_comments {
                text.line(curr_line_num)
                    .first_non_whitespace_char()
                    .map(|c| text.char_to_byte(text.line_to_char(curr_line_num) + c))
                    .and_then(|byte| doc.language_config_at(&loader, byte))
                    .and_then(|config| config.comment_tokens.as_ref())
                    .and_then(|tokens| comment::get_comment_token(text, tokens, curr_line_num))
            } else {
                None
            };

        // Index to insert newlines after, as well as the char width
        // to use to compensate for those inserted newlines.
        let (above_next_line_end_index, above_next_line_end_width) = if next_new_line_num == 0 {
            (0, 0)
        } else {
            (
                line_end_char_index(&text, above_next_new_line_num),
                doc.line_ending.len_chars(),
            )
        };

        let line = text.line(curr_line_num);
        let indent = match line.first_non_whitespace_char() {
            Some(pos) if continue_comment_token.is_some() => line.slice(..pos).to_string(),
            _ => indent::indent_for_newline(
                &loader,
                doc.syntax(),
                &config.indent_heuristic,
                &doc.indent_style,
                doc.tab_width(),
                text,
                above_next_new_line_num,
                above_next_line_end_index,
                curr_line_num,
            ),
        };

        let indent_len = indent.len();
        let mut text = String::with_capacity(1 + indent_len);

        if open == Open::Above && next_new_line_num == 0 {
            text.push_str(&indent);
            if let Some(token) = continue_comment_token {
                text.push_str(token);
                text.push(' ');
            }
            text.push_str(doc.line_ending.as_str());
        } else {
            text.push_str(doc.line_ending.as_str());
            text.push_str(&indent);

            if let Some(token) = continue_comment_token {
                text.push_str(token);
                text.push(' ');
            }
        }

        let text = text.repeat(count);

        // calculate new selection ranges
        let pos = offs + above_next_line_end_index + above_next_line_end_width;
        let comment_len = continue_comment_token
            .map(|token| token.len() + 1) // `+ 1` for the extra space added
            .unwrap_or_default();
        for i in 0..count {
            // pos                     -> beginning of reference line,
            // + (i * (line_ending_len + indent_len + comment_len)) -> beginning of i'th line from pos (possibly including comment token)
            // + indent_len + comment_len ->        -> indent for i'th line
            ranges.push(Range::point(
                pos + (i * (doc.line_ending.len_chars() + indent_len + comment_len))
                    + indent_len
                    + comment_len,
            ));
        }

        // update the offset for the next range
        offs += text.chars().count();

        (
            above_next_line_end_index,
            above_next_line_end_index,
            Some(text.into()),
        )
    });

    transaction = transaction.with_selection(Selection::new(ranges, selection.primary_index()));

    doc.apply(&transaction, view.id);
}

// o inserts a new line after each line with a selection
fn open_below(cx: &mut Context) {
    open(cx, Open::Below, CommentContinuation::Enabled)
}

// O inserts a new line before each line with a selection
fn open_above(cx: &mut Context) {
    open(cx, Open::Above, CommentContinuation::Enabled)
}

fn normal_mode(cx: &mut Context) {
    // Capture the text inserted this session (for i_CTRL-A) before leaving insert.
    if cx.editor.mode == Mode::Insert {
        let (view, doc) = current!(cx.editor);
        let text = doc.text().slice(..);
        let end = doc.selection(view.id).primary().cursor(text);
        let start = INSERT_ANCHOR
            .load(std::sync::atomic::Ordering::Relaxed)
            .min(text.len_chars());
        if end > start {
            let s: String = text.slice(start..end).chars().collect();
            *LAST_INSERTED_TEXT.lock().unwrap() = s;
        }
    }
    cx.editor.enter_normal_mode();
}

/// vim `i_CTRL-A`: insert the text of the most recently completed insert session.
fn insert_last_inserted_text(cx: &mut Context) {
    let text = LAST_INSERTED_TEXT.lock().unwrap().clone();
    if text.is_empty() {
        return;
    }
    let mode = cx.editor.mode;
    let (view, doc) = current!(cx.editor);
    paste_impl(&[text], doc, view, Paste::Cursor, 1, mode);
}

/// vim `i_CTRL-@`: insert the last-inserted text and then leave insert mode.
fn insert_last_inserted_and_stop(cx: &mut Context) {
    insert_last_inserted_text(cx);
    normal_mode(cx);
}

// Store a jump on the jumplist.
fn push_jump(view: &mut View, doc: &mut Document) {
    doc.append_changes_to_history(view);
    let jump = (doc.id(), doc.selection(view.id).clone());
    view.push_jump(doc, jump);
}

fn goto_line(cx: &mut Context) {
    goto_line_impl(cx, Movement::Move);
}

fn goto_line_impl(cx: &mut Context, movement: Movement) {
    if cx.count.is_some() {
        let (view, doc) = current!(cx.editor);
        push_jump(view, doc);

        goto_line_without_jumplist(cx.editor, cx.count, movement);
    }
}

fn goto_line_without_jumplist(
    editor: &mut Editor,
    count: Option<NonZeroUsize>,
    movement: Movement,
) {
    if let Some(count) = count {
        let (view, doc) = current!(editor);
        let text = doc.text().slice(..);
        let max_line = if text.line(text.len_lines() - 1).len_chars() == 0 {
            // If the last line is blank, don't jump to it.
            text.len_lines().saturating_sub(2)
        } else {
            text.len_lines() - 1
        };
        let line_idx = std::cmp::min(count.get() - 1, max_line);
        let pos = text.line_to_char(line_idx);
        let selection = doc
            .selection(view.id)
            .clone()
            .transform(|range| range.put_cursor(text, pos, movement == Movement::Extend));

        doc.set_selection(view.id, selection);
    }
}

fn goto_last_line(cx: &mut Context) {
    goto_last_line_impl(cx, Movement::Move)
}

fn extend_to_last_line(cx: &mut Context) {
    goto_last_line_impl(cx, Movement::Extend)
}

fn goto_last_line_impl(cx: &mut Context, movement: Movement) {
    let (view, doc) = current!(cx.editor);
    // When narrowed, `G` goes to the last line of the region (Emacs point-max line), per-view aware.
    let narrowed = doc.is_narrowed() || doc.view_narrow(view.id).is_some();
    let point_max = doc.view_point_max(view.id);
    let text = doc.text().slice(..);
    let last_line = if narrowed {
        text.char_to_line(point_max.min(text.len_chars()))
    } else {
        text.len_lines() - 1
    };
    let line_idx = if text.line(last_line).len_chars() == 0 && last_line > 0 {
        // If the last line is blank, don't jump to it.
        last_line.saturating_sub(1)
    } else {
        last_line
    };
    let pos = text.line_to_char(line_idx);
    let selection = doc
        .selection(view.id)
        .clone()
        .transform(|range| range.put_cursor(text, pos, movement == Movement::Extend));

    push_jump(view, doc);
    doc.set_selection(view.id, selection);
}

fn goto_column(cx: &mut Context) {
    goto_column_impl(cx, Movement::Move);
}

fn extend_to_column(cx: &mut Context) {
    goto_column_impl(cx, Movement::Extend);
}

fn goto_column_impl(cx: &mut Context, movement: Movement) {
    let count = cx.count();
    let (view, doc) = current!(cx.editor);
    let text = doc.text().slice(..);
    let selection = doc.selection(view.id).clone().transform(|range| {
        let line = range.cursor_line(text);
        let line_start = text.line_to_char(line);
        let line_end = line_end_char_index(&text, line);
        let pos = graphemes::nth_next_grapheme_boundary(text, line_start, count - 1).min(line_end);
        range.put_cursor(text, pos, movement == Movement::Extend)
    });
    push_jump(view, doc);
    doc.set_selection(view.id, selection);
}

fn goto_last_accessed_file(cx: &mut Context) {
    let view = view_mut!(cx.editor);
    if let Some(alt) = view.docs_access_history.pop() {
        cx.editor.switch(alt, Action::Replace);
    } else {
        cx.editor.set_error("no last accessed buffer")
    }
}

fn goto_last_modification(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);
    let pos = doc.history.get_mut().last_edit_pos();
    let text = doc.text().slice(..);
    if let Some(pos) = pos {
        let selection = doc
            .selection(view.id)
            .clone()
            .transform(|range| range.put_cursor(text, pos, cx.editor.mode == Mode::Select));
        push_jump(view, doc);
        doc.set_selection(view.id, selection);
    }
}

fn goto_last_modified_file(cx: &mut Context) {
    let view = view!(cx.editor);
    let alternate_file = view
        .last_modified_docs
        .into_iter()
        .flatten()
        .find(|&id| id != view.doc);
    if let Some(alt) = alternate_file {
        cx.editor.switch(alt, Action::Replace);
    } else {
        cx.editor.set_error("no last modified buffer")
    }
}

fn select_mode(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);
    let text = doc.text().slice(..);

    // Make sure end-of-document selections are also 1-width.
    // (With the exception of being in an empty document, of course.)
    let selection = doc.selection(view.id).clone().transform(|range| {
        if range.is_empty() && range.head == text.len_chars() {
            Range::new(
                graphemes::prev_grapheme_boundary(text, range.anchor),
                range.head,
            )
        } else {
            range
        }
    });
    doc.set_selection(view.id, selection);

    cx.editor.mode = Mode::Select;
}

fn exit_select_mode(cx: &mut Context) {
    if cx.editor.mode == Mode::Select {
        cx.editor.mode = Mode::Normal;
    }
}

/// Copy the diagnostic message(s) on the current line to the clipboard — handy
/// for pasting an error into a search or issue.
fn copy_diagnostic(cx: &mut Context) {
    let messages = {
        let (view, doc) = current_ref!(cx.editor);
        let text = doc.text().slice(..);
        let line = text.char_to_line(doc.selection(view.id).primary().cursor(text));
        doc.diagnostics()
            .iter()
            .filter(|d| text.char_to_line(d.range.start) == line)
            .map(|d| d.message.clone())
            .collect::<Vec<_>>()
    };
    if messages.is_empty() {
        cx.editor.set_status("No diagnostic on this line");
        return;
    }
    let joined = messages.join("\n");
    let _ = cx.editor.registers.write('+', vec![joined.clone()]);
    let first = joined.lines().next().unwrap_or_default();
    cx.editor.set_status(format!("Copied diagnostic: {first}"));
}

fn goto_first_diag(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);
    let selection = match doc.diagnostics().first() {
        Some(diag) => Selection::single(diag.range.start, diag.range.end),
        None => return,
    };
    push_jump(view, doc);
    doc.set_selection(view.id, selection);
    view.diagnostics_handler
        .immediately_show_diagnostic(doc, view.id);
}

fn goto_last_diag(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);
    let selection = match doc.diagnostics().last() {
        Some(diag) => Selection::single(diag.range.start, diag.range.end),
        None => return,
    };
    push_jump(view, doc);
    doc.set_selection(view.id, selection);
    view.diagnostics_handler
        .immediately_show_diagnostic(doc, view.id);
}

fn goto_next_diag(cx: &mut Context) {
    let motion = move |editor: &mut Editor| {
        let (view, doc) = current!(editor);

        let cursor_pos = doc
            .selection(view.id)
            .primary()
            .cursor(doc.text().slice(..));

        let diag = doc
            .diagnostics()
            .iter()
            .find(|diag| diag.range.start > cursor_pos);

        let selection = match diag {
            Some(diag) => Selection::single(diag.range.start, diag.range.end),
            None => return,
        };
        push_jump(view, doc);
        doc.set_selection(view.id, selection);
        view.diagnostics_handler
            .immediately_show_diagnostic(doc, view.id);
    };

    cx.editor.apply_motion(motion);
}

fn goto_prev_diag(cx: &mut Context) {
    let motion = move |editor: &mut Editor| {
        let (view, doc) = current!(editor);

        let cursor_pos = doc
            .selection(view.id)
            .primary()
            .cursor(doc.text().slice(..));

        let diag = doc
            .diagnostics()
            .iter()
            .rev()
            .find(|diag| diag.range.start < cursor_pos);

        let selection = match diag {
            // NOTE: the selection is reversed because we're jumping to the
            // previous diagnostic.
            Some(diag) => Selection::single(diag.range.end, diag.range.start),
            None => return,
        };
        push_jump(view, doc);
        doc.set_selection(view.id, selection);
        view.diagnostics_handler
            .immediately_show_diagnostic(doc, view.id);
    };
    cx.editor.apply_motion(motion)
}

fn goto_first_change(cx: &mut Context) {
    goto_first_change_impl(cx, false);
}

fn goto_last_change(cx: &mut Context) {
    goto_first_change_impl(cx, true);
}

fn goto_first_change_impl(cx: &mut Context, reverse: bool) {
    let editor = &mut cx.editor;
    let (view, doc) = current!(editor);
    if let Some(handle) = doc.diff_handle() {
        let hunk = {
            let diff = handle.load();
            let idx = if reverse {
                diff.len().saturating_sub(1)
            } else {
                0
            };
            diff.nth_hunk(idx)
        };
        if hunk != Hunk::NONE {
            let range = hunk_range(hunk, doc.text().slice(..));
            push_jump(view, doc);
            doc.set_selection(view.id, Selection::single(range.anchor, range.head));
        }
    }
}

/// True if a line begins with a git merge-conflict marker.
fn is_conflict_marker(line: RopeSlice) -> bool {
    let prefix: String = line.chars().take(7).collect();
    matches!(
        prefix.as_str(),
        "<<<<<<<" | "=======" | ">>>>>>>" | "|||||||"
    )
}

/// Pure core of `goto_conflict_impl`: starting from `cur_line` (exclusive),
/// return the index of the nearest line that begins with a git merge-conflict
/// marker, searching forward or backward. Unit-tested.
fn find_conflict_line(slice: RopeSlice, cur_line: usize, forward: bool) -> Option<usize> {
    let total = slice.len_lines();
    if forward {
        (cur_line + 1..total).find(|&l| is_conflict_marker(slice.line(l)))
    } else {
        (0..cur_line)
            .rev()
            .find(|&l| is_conflict_marker(slice.line(l)))
    }
}

fn goto_conflict_impl(cx: &mut Context, forward: bool) {
    let scrolloff = cx.editor.config().scrolloff;
    let target = {
        let (view, doc) = current_ref!(cx.editor);
        let slice = doc.text().slice(..);
        let cur_line = slice.char_to_line(doc.selection(view.id).primary().cursor(slice));
        find_conflict_line(slice, cur_line, forward).map(|l| slice.line_to_char(l))
    };
    match target {
        Some(pos) => {
            let (view, doc) = current!(cx.editor);
            push_jump(view, doc);
            doc.set_selection(view.id, Selection::point(pos));
            view.ensure_cursor_in_view(doc, scrolloff);
        }
        None => cx.editor.set_status("No merge-conflict markers"),
    }
}

fn line_starts_with(line: RopeSlice, marker: &str) -> bool {
    line.chars().take(marker.len()).collect::<String>() == marker
}

#[derive(Clone, Copy)]
enum ConflictSide {
    Ours,
    Theirs,
    #[allow(dead_code)] // keep-both resolution not yet wired to a command
    Both,
}

/// Resolve EVERY merge conflict in the buffer, keeping the given side(s).
fn resolve_all_conflicts(cx: &mut Context, side: ConflictSide) {
    let changes: Vec<(usize, usize, Option<Tendril>)> = {
        let (_, doc) = current_ref!(cx.editor);
        let slice = doc.text().slice(..);
        let total = slice.len_lines();
        let mut changes = Vec::new();
        let mut l = 0;
        while l < total {
            if !line_starts_with(slice.line(l), "<<<<<<<") {
                l += 1;
                continue;
            }
            let start = l;
            let (mut base, mut sep, mut end) = (None, None, None);
            let mut k = start + 1;
            while k < total {
                if base.is_none() && sep.is_none() && line_starts_with(slice.line(k), "|||||||") {
                    base = Some(k);
                } else if sep.is_none() && line_starts_with(slice.line(k), "=======") {
                    sep = Some(k);
                } else if line_starts_with(slice.line(k), ">>>>>>>") {
                    end = Some(k);
                    break;
                }
                k += 1;
            }
            if let (Some(sep), Some(end)) = (sep, end) {
                let ours_end = base.unwrap_or(sep);
                let ours: String = slice
                    .slice(slice.line_to_char(start + 1)..slice.line_to_char(ours_end))
                    .chars()
                    .collect();
                let theirs: String = slice
                    .slice(slice.line_to_char(sep + 1)..slice.line_to_char(end))
                    .chars()
                    .collect();
                let repl = match side {
                    ConflictSide::Ours => ours,
                    ConflictSide::Theirs => theirs,
                    ConflictSide::Both => format!("{ours}{theirs}"),
                };
                changes.push((
                    slice.line_to_char(start),
                    slice.line_to_char((end + 1).min(total)),
                    (!repl.is_empty()).then(|| repl.into()),
                ));
                l = end + 1;
            } else {
                l += 1;
            }
        }
        changes
    };
    if changes.is_empty() {
        cx.editor.set_status("No conflicts to resolve");
        return;
    }
    let n = changes.len();
    let (view, doc) = current!(cx.editor);
    let tx = Transaction::change(doc.text(), changes.into_iter());
    doc.apply(&tx, view.id);
    doc.append_changes_to_history(view);
    cx.editor.set_status(format!("Resolved {n} conflicts"));
}

fn conflict_take_all_ours(cx: &mut Context) {
    resolve_all_conflicts(cx, ConflictSide::Ours);
}
fn conflict_take_all_theirs(cx: &mut Context) {
    resolve_all_conflicts(cx, ConflictSide::Theirs);
}

/// vim/git `]x`: jump to the next merge-conflict marker.
fn goto_next_conflict(cx: &mut Context) {
    goto_conflict_impl(cx, true);
}

/// vim/git `[x`: jump to the previous merge-conflict marker.
fn goto_prev_conflict(cx: &mut Context) {
    goto_conflict_impl(cx, false);
}

fn goto_next_change(cx: &mut Context) {
    goto_next_change_impl(cx, Direction::Forward)
}

fn goto_prev_change(cx: &mut Context) {
    goto_next_change_impl(cx, Direction::Backward)
}

fn goto_next_change_impl(cx: &mut Context, direction: Direction) {
    let count = cx.count() as u32 - 1;
    let motion = move |editor: &mut Editor| {
        let (view, doc) = current!(editor);
        let doc_text = doc.text().slice(..);
        let diff_handle = if let Some(diff_handle) = doc.diff_handle() {
            diff_handle
        } else {
            editor.set_status("Diff is not available in current buffer");
            return;
        };

        let selection = doc.selection(view.id).clone().transform(|range| {
            let cursor_line = range.cursor_line(doc_text) as u32;

            let diff = diff_handle.load();
            let hunk_idx = match direction {
                Direction::Forward => diff
                    .next_hunk(cursor_line)
                    .map(|idx| (idx + count).min(diff.len() - 1)),
                Direction::Backward => diff
                    .prev_hunk(cursor_line)
                    .map(|idx| idx.saturating_sub(count)),
            };
            let Some(hunk_idx) = hunk_idx else {
                return range;
            };
            let hunk = diff.nth_hunk(hunk_idx);
            let new_range = hunk_range(hunk, doc_text);
            if editor.mode == Mode::Select {
                let head = if new_range.head < range.anchor {
                    new_range.anchor
                } else {
                    new_range.head
                };

                Range::new(range.anchor, head)
            } else {
                new_range.with_direction(direction)
            }
        });

        push_jump(view, doc);
        doc.set_selection(view.id, selection)
    };
    cx.editor.apply_motion(motion);
}

/// Returns the [Range] for a [Hunk] in the given text.
/// Additions and modifications cover the added and modified ranges.
/// Deletions are represented as the point at the start of the deletion hunk.
fn hunk_range(hunk: Hunk, text: RopeSlice) -> Range {
    let anchor = text.line_to_char(hunk.after.start as usize);
    let head = if hunk.after.is_empty() {
        anchor + 1
    } else {
        text.line_to_char(hunk.after.end as usize)
    };

    Range::new(anchor, head)
}

pub mod insert {
    use crate::{events::PostInsertChar, key};

    use super::*;
    pub type Hook = fn(&Rope, &Selection, char) -> Option<Transaction>;

    /// Exclude the cursor in range.
    fn exclude_cursor(text: RopeSlice, range: Range, cursor: Range) -> Range {
        if range.to() == cursor.to() && text.len_chars() != cursor.to() {
            Range::new(
                range.from(),
                graphemes::prev_grapheme_boundary(text, cursor.to()),
            )
        } else {
            range
        }
    }

    use zemacs_core::auto_pairs;
    use zemacs_view::editor::SmartTabConfig;

    pub fn insert_char(cx: &mut Context, c: char) {
        // vim Replace mode (`R`): overtype the character under the cursor rather
        // than inserting, unless the cursor sits at the end of the line (where
        // vim appends). Auto-pairs are bypassed while overtyping.
        if cx.editor.overwrite {
            return overtype_char(cx, c);
        }

        let (view, doc) = current_ref!(cx.editor);
        let text = doc.text();
        let selection = doc.selection(view.id);

        let loader: &zemacs_core::syntax::Loader = &cx.editor.syn_loader.load();
        let auto_pairs = doc.auto_pairs(cx.editor, loader, view);

        let insert_char = |range: Range, ch: char| {
            let cursor = range.cursor(text.slice(..));
            let t = Tendril::from_iter([ch]);
            ((cursor, cursor, Some(t)), None)
        };

        let transaction = Transaction::change_by_and_with_selection(text, selection, |range| {
            auto_pairs
                .as_ref()
                .and_then(|ap| {
                    auto_pairs::hook_insert(text, range, c, ap)
                        .map(|(change, range)| (change, Some(range)))
                        .or_else(|| Some(insert_char(*range, c)))
                })
                .unwrap_or_else(|| insert_char(*range, c))
        });

        let doc = doc_mut!(cx.editor, &doc.id());
        doc.apply(&transaction, view.id);

        zemacs_event::dispatch(PostInsertChar { c, cx });
    }

    /// Overtype the grapheme under each cursor with `c` (vim Replace mode). At a
    /// line end the character is appended instead, matching vim.
    pub fn overtype_char(cx: &mut Context, c: char) {
        let (view, doc) = current_ref!(cx.editor);
        let text = doc.text();
        let slice = text.slice(..);
        let selection = doc.selection(view.id);

        let transaction = Transaction::change_by_and_with_selection(text, selection, |range| {
            let cursor = range.cursor(slice);
            let line = slice.char_to_line(cursor);
            let line_end = line_end_char_index(&slice, line);
            let t = Tendril::from_iter([c]);
            // Replace the char under the cursor unless at end-of-line (append).
            let end = if cursor < line_end {
                graphemes::next_grapheme_boundary(slice, cursor)
            } else {
                cursor
            };
            // Advance the cursor past the overtyped character (vim moves right).
            ((cursor, end, Some(t)), Some(Range::point(cursor + 1)))
        });

        let doc = doc_mut!(cx.editor, &doc.id());
        doc.apply(&transaction, view.id);

        zemacs_event::dispatch(PostInsertChar { c, cx });
    }

    pub fn smart_tab(cx: &mut Context) {
        let (view, doc) = current_ref!(cx.editor);
        let view_id = view.id;

        if matches!(
            cx.editor.config().smart_tab,
            Some(SmartTabConfig { enable: true, .. })
        ) {
            let cursors_after_whitespace = doc.selection(view_id).ranges().iter().all(|range| {
                let cursor = range.cursor(doc.text().slice(..));
                let current_line_num = doc.text().char_to_line(cursor);
                let current_line_start = doc.text().line_to_char(current_line_num);
                let left = doc.text().slice(current_line_start..cursor);
                left.chars().all(|c| c.is_whitespace())
            });

            if !cursors_after_whitespace {
                if doc.active_snippet.is_some() {
                    goto_next_tabstop(cx);
                } else {
                    move_parent_node_end(cx);
                }
                return;
            }
        }

        insert_tab(cx);
    }

    pub fn insert_tab(cx: &mut Context) {
        insert_tab_impl(cx, 1)
    }

    fn insert_tab_impl(cx: &mut Context, count: usize) {
        let (view, doc) = current!(cx.editor);

        let transaction = Transaction::change(
            doc.text(),
            doc.selection(view.id).ranges().iter().map(|range| {
                let cursor = range.cursor(doc.text().slice(..));
                let indent = if let IndentStyle::Spaces(indent_width) = doc.indent_style {
                    let line = range.cursor_line(doc.text().slice(..));
                    let line_start = doc.text().line_to_char(line);
                    let offset = (cursor - line_start) % indent_width as usize;

                    Tendril::from(doc.indent_style.as_str().repeat(count)).split_off(offset)
                } else {
                    Tendril::from(doc.indent_style.as_str().repeat(count))
                };

                (cursor, cursor, Some(indent))
            }),
        );
        doc.apply(&transaction, view.id);
    }

    pub fn append_char_interactive(cx: &mut Context) {
        // Save the current mode, so we can restore it later.
        let mode = cx.editor.mode;
        append_mode(cx);
        insert_selection_interactive(cx, mode);
    }

    pub fn insert_char_interactive(cx: &mut Context) {
        let mode = cx.editor.mode;
        insert_mode(cx);
        insert_selection_interactive(cx, mode);
    }

    fn insert_selection_interactive(cx: &mut Context, old_mode: Mode) {
        let count = cx.count();

        // need to wait for next key
        cx.on_next_key(move |cx, event| {
            match event {
                KeyEvent {
                    code: KeyCode::Char(ch),
                    ..
                } => {
                    for _ in 0..count {
                        insert::insert_char(cx, ch)
                    }
                }
                key!(Enter) => {
                    if count != 1 {
                        cx.editor
                            .set_error("inserting multiple newlines not yet supported");
                        return;
                    }
                    insert_newline(cx)
                }
                key!(Tab) => insert_tab_impl(cx, count),
                _ => (),
            };
            // Restore the old mode.
            cx.editor.mode = old_mode;
        });
    }

    pub fn insert_newline(cx: &mut Context) {
        let config = cx.editor.config();
        let (view, doc) = current_ref!(cx.editor);
        let loader = cx.editor.syn_loader.load();
        let text = doc.text().slice(..);
        let line_ending = doc.line_ending.as_str();

        let contents = doc.text();
        let selection = doc.selection(view.id);
        let mut ranges = SmallVec::with_capacity(selection.len());

        // TODO: this is annoying, but we need to do it to properly calculate pos after edits
        let mut global_offs = 0;
        let mut new_text = String::new();

        let mut last_pos = 0;
        let mut transaction = Transaction::change_by_selection(contents, selection, |range| {
            // Tracks the number of trailing whitespace characters deleted by this selection.
            let mut chars_deleted = 0;
            let pos = range.cursor(text);

            let prev = if pos == 0 {
                ' '
            } else {
                contents.char(pos - 1)
            };
            let curr = contents.get_char(pos).unwrap_or(' ');

            let current_line = text.char_to_line(pos);
            let line_start = text.line_to_char(current_line);

            // Continue the comment leader using the comment tokens of the layer at the comment
            // leader (i.e. the first non-whitespace char on the line). Looking up at the cursor
            // would land inside an injected layer (e.g. `comment`, or markdown in a doc comment)
            // and miss the host language's tokens.
            let continue_comment_token = if config.continue_comments {
                text.line(current_line)
                    .first_non_whitespace_char()
                    .map(|c| text.char_to_byte(line_start + c))
                    .and_then(|byte| doc.language_config_at(&loader, byte))
                    .and_then(|config| config.comment_tokens.as_ref())
                    .and_then(|tokens| comment::get_comment_token(text, tokens, current_line))
            } else {
                None
            };

            let (from, to, local_offs) = if let Some(idx) =
                text.slice(line_start..pos).last_non_whitespace_char()
            {
                let first_trailing_whitespace_char = (line_start + idx + 1).clamp(last_pos, pos);
                last_pos = pos;
                let line = text.line(current_line);

                let indent = match line.first_non_whitespace_char() {
                    Some(pos) if continue_comment_token.is_some() => line.slice(..pos).to_string(),
                    _ => indent::indent_for_newline(
                        &loader,
                        doc.syntax(),
                        &config.indent_heuristic,
                        &doc.indent_style,
                        doc.tab_width(),
                        text,
                        current_line,
                        pos,
                        current_line,
                    ),
                };

                let loader: &zemacs_core::syntax::Loader = &cx.editor.syn_loader.load();
                // If we are between pairs (such as brackets), we want to
                // insert an additional line which is indented one level
                // more and place the cursor there
                let on_auto_pair = doc
                    .auto_pairs(cx.editor, loader, view)
                    .and_then(|pairs| pairs.get(prev))
                    .is_some_and(|pair| pair.open == prev && pair.close == curr);

                let local_offs = if let Some(token) = continue_comment_token {
                    new_text.reserve_exact(line_ending.len() + indent.len() + token.len() + 1);
                    new_text.push_str(line_ending);
                    new_text.push_str(&indent);
                    new_text.push_str(token);
                    new_text.push(' ');
                    new_text.chars().count()
                } else if on_auto_pair {
                    // line where the cursor will be
                    let inner_indent = indent.clone() + doc.indent_style.as_str();
                    new_text
                        .reserve_exact(line_ending.len() * 2 + indent.len() + inner_indent.len());
                    new_text.push_str(line_ending);
                    new_text.push_str(&inner_indent);

                    // line where the matching pair will be
                    let local_offs = new_text.chars().count();
                    new_text.push_str(line_ending);
                    new_text.push_str(&indent);

                    local_offs
                } else {
                    new_text.reserve_exact(line_ending.len() + indent.len());
                    new_text.push_str(line_ending);
                    new_text.push_str(&indent);

                    new_text.chars().count()
                };

                // Note that `first_trailing_whitespace_char` is at least `pos` so this unsigned
                // subtraction cannot underflow.
                chars_deleted = pos - first_trailing_whitespace_char;

                (
                    first_trailing_whitespace_char,
                    pos,
                    local_offs as isize - chars_deleted as isize,
                )
            } else {
                // If the current line is all whitespace, insert a line ending at the beginning of
                // the current line. This makes the current line empty and the new line contain the
                // indentation of the old line.
                new_text.push_str(line_ending);

                (line_start, line_start, new_text.chars().count() as isize)
            };

            let new_range = if range.cursor(text) > range.anchor {
                // when appending, extend the range by local_offs
                Range::new(
                    (range.anchor as isize + global_offs) as usize,
                    (range.head as isize + local_offs + global_offs) as usize,
                )
            } else {
                // when inserting, slide the range by local_offs
                Range::new(
                    (range.anchor as isize + local_offs + global_offs) as usize,
                    (range.head as isize + local_offs + global_offs) as usize,
                )
            };

            // TODO: range replace or extend
            // range.replace(|range| range.is_empty(), head); -> fn extend if cond true, new head pos
            // can be used with cx.mode to do replace or extend on most changes
            ranges.push(new_range);
            global_offs += new_text.chars().count() as isize - chars_deleted as isize;
            let tendril = Tendril::from(&new_text);
            new_text.clear();

            (from, to, Some(tendril))
        });

        transaction = transaction.with_selection(Selection::new(ranges, selection.primary_index()));

        let (view, doc) = current!(cx.editor);
        doc.apply(&transaction, view.id);
    }

    fn dedent(doc: &Document, range: &Range) -> Option<Deletion> {
        let text = doc.text().slice(..);
        let pos = range.cursor(text);
        let line_start_pos = text.line_to_char(range.cursor_line(text));

        // consider to delete by indent level if all characters before `pos` are indent units.
        let fragment = Cow::from(text.slice(line_start_pos..pos));

        if fragment.is_empty() || !fragment.chars().all(|ch| ch == ' ' || ch == '\t') {
            return None;
        }

        if text.get_char(pos.saturating_sub(1)) == Some('\t') {
            // fast path, delete one char
            return Some((graphemes::nth_prev_grapheme_boundary(text, pos, 1), pos));
        }

        let tab_width = doc.tab_width();
        let indent_width = doc.indent_width();

        let width: usize = fragment
            .chars()
            .map(|ch| {
                if ch == '\t' {
                    tab_width
                } else {
                    // it can be none if it still meet control characters other than '\t'
                    // here just set the width to 1 (or some value better?).
                    ch.width().unwrap_or(1)
                }
            })
            .sum();

        // round down to nearest unit
        let mut drop = width % indent_width;

        // if it's already at a unit, consume a whole unit
        if drop == 0 {
            drop = indent_width
        };

        let mut chars = fragment.chars().rev();
        let mut start = pos;

        for _ in 0..drop {
            // delete up to `drop` spaces
            match chars.next() {
                Some(' ') => start -= 1,
                _ => break,
            }
        }

        Some((start, pos)) // delete!
    }

    pub fn delete_char_backward(cx: &mut Context) {
        let count = cx.count();
        let (view, doc) = current_ref!(cx.editor);
        let text = doc.text().slice(..);

        let loader: &zemacs_core::syntax::Loader = &cx.editor.syn_loader.load();
        let auto_pairs = doc.auto_pairs(cx.editor, loader, view);

        let transaction = Transaction::delete_by_and_with_selection(
            doc.text(),
            doc.selection(view.id),
            |range| {
                let pos = range.cursor(text);

                log::debug!("cursor: {}, len: {}", pos, text.len_chars());

                if pos == 0 {
                    return ((pos, pos), None);
                }

                dedent(doc, range)
                    .map(|dedent| (dedent, None))
                    .or_else(|| {
                        // [TODO] should this be fixed to get the auto pairs for
                        // each selection after 46af40017c0704142516b5740cf1a000ba4fd7c1 ?
                        auto_pairs::hook_delete(doc.text(), range, auto_pairs?)
                            .map(|(delete, new_range)| (delete, Some(new_range)))
                    })
                    .unwrap_or_else(|| {
                        (
                            (graphemes::nth_prev_grapheme_boundary(text, pos, count), pos),
                            None,
                        )
                    })
            },
        );

        log::debug!("delete_char_backward transaction: {:?}", transaction);

        let doc = doc_mut!(cx.editor, &doc.id());
        doc.apply(&transaction, view.id);
    }

    pub fn delete_char_forward(cx: &mut Context) {
        let count = cx.count();
        delete_by_selection_insert_mode(
            cx,
            |text, range| {
                let pos = range.cursor(text);
                (pos, graphemes::nth_next_grapheme_boundary(text, pos, count))
            },
            Direction::Forward,
        )
    }

    pub fn delete_word_backward(cx: &mut Context) {
        let count = cx.count();
        delete_by_selection_insert_mode(
            cx,
            |text, range| {
                let anchor = movement::move_prev_word_start(text, *range, count).from();
                let next = Range::new(anchor, range.cursor(text));
                let range = exclude_cursor(text, next, *range);
                (range.from(), range.to())
            },
            Direction::Backward,
        );
    }

    pub fn delete_word_forward(cx: &mut Context) {
        let count = cx.count();
        delete_by_selection_insert_mode(
            cx,
            |text, range| {
                let head = movement::move_next_word_end(text, *range, count).to();
                (range.cursor(text), head)
            },
            Direction::Forward,
        );
    }
}

// Undo / Redo

fn undo(cx: &mut Context) {
    let count = cx.count();
    let (view, doc) = current!(cx.editor);
    for _ in 0..count {
        if !doc.undo(view) {
            cx.editor.set_status("Already at oldest change");
            break;
        }
    }
}

fn redo(cx: &mut Context) {
    let count = cx.count();
    let (view, doc) = current!(cx.editor);
    for _ in 0..count {
        if !doc.redo(view) {
            cx.editor.set_status("Already at newest change");
            break;
        }
    }
}

fn earlier(cx: &mut Context) {
    let count = cx.count();
    let (view, doc) = current!(cx.editor);
    for _ in 0..count {
        // rather than doing in batch we do this so get error halfway
        if !doc.earlier(view, UndoKind::Steps(1)) {
            cx.editor.set_status("Already at oldest change");
            break;
        }
    }
}

fn later(cx: &mut Context) {
    let count = cx.count();
    let (view, doc) = current!(cx.editor);
    for _ in 0..count {
        // rather than doing in batch we do this so get error halfway
        if !doc.later(view, UndoKind::Steps(1)) {
            cx.editor.set_status("Already at newest change");
            break;
        }
    }
}

fn commit_undo_checkpoint(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);
    doc.append_changes_to_history(view);
}

// Yank / Paste

fn yank(cx: &mut Context) {
    yank_impl(
        cx.editor,
        cx.register
            .unwrap_or(cx.editor.config().default_yank_register),
    );
    exit_select_mode(cx);
}

fn yank_to_clipboard(cx: &mut Context) {
    yank_impl(cx.editor, '+');
    exit_select_mode(cx);
}

fn yank_to_primary_clipboard(cx: &mut Context) {
    yank_impl(cx.editor, '*');
    exit_select_mode(cx);
}

fn yank_impl(editor: &mut Editor, register: char) {
    let (view, doc) = current!(editor);
    let (from, to, values) = {
        let text = doc.text().slice(..);
        let primary = doc.selection(view.id).primary();
        let values: Vec<String> = doc
            .selection(view.id)
            .fragments(text)
            .map(Cow::into_owned)
            .collect();
        (primary.from(), primary.to(), values)
    };
    let selections = values.len();
    // vim: yanking sets the `[`/`]` marks to the start/end of the yanked text.
    doc.set_mark('[', from);
    doc.set_mark(']', to.saturating_sub(1).max(from));

    crate::emacs_kill::record(values.join("\n"));
    match editor.registers.write(register, values) {
        Ok(_) => editor.set_status(format!(
            "yanked {selections} selection{} to register {register}",
            if selections == 1 { "" } else { "s" }
        )),
        Err(err) => editor.set_error(err.to_string()),
    }
}

fn yank_joined_impl(editor: &mut Editor, separator: &str, register: char) {
    let (view, doc) = current!(editor);
    let text = doc.text().slice(..);

    let selection = doc.selection(view.id);
    let selections = selection.len();
    let joined = selection
        .fragments(text)
        .fold(String::new(), |mut acc, fragment| {
            if !acc.is_empty() {
                acc.push_str(separator);
            }
            acc.push_str(&fragment);
            acc
        });

    match editor.registers.write(register, vec![joined]) {
        Ok(_) => editor.set_status(format!(
            "joined and yanked {selections} selection{} to register {register}",
            if selections == 1 { "" } else { "s" }
        )),
        Err(err) => editor.set_error(err.to_string()),
    }
}

fn yank_joined(cx: &mut Context) {
    let separator = doc!(cx.editor).line_ending.as_str();
    yank_joined_impl(
        cx.editor,
        separator,
        cx.register
            .unwrap_or(cx.editor.config().default_yank_register),
    );
    exit_select_mode(cx);
}

fn yank_joined_to_clipboard(cx: &mut Context) {
    let line_ending = doc!(cx.editor).line_ending;
    yank_joined_impl(cx.editor, line_ending.as_str(), '+');
    exit_select_mode(cx);
}

fn yank_joined_to_primary_clipboard(cx: &mut Context) {
    let line_ending = doc!(cx.editor).line_ending;
    yank_joined_impl(cx.editor, line_ending.as_str(), '*');
    exit_select_mode(cx);
}

pub(crate) fn yank_main_selection_to_register(editor: &mut Editor, register: char) {
    let (view, doc) = current!(editor);
    let text = doc.text().slice(..);

    let selection = doc.selection(view.id).primary().fragment(text).to_string();

    match editor.registers.write(register, vec![selection]) {
        Ok(_) => editor.set_status(format!("yanked primary selection to register {register}",)),
        Err(err) => editor.set_error(err.to_string()),
    }
}

fn yank_main_selection_to_clipboard(cx: &mut Context) {
    yank_main_selection_to_register(cx.editor, '+');
    exit_select_mode(cx);
}

fn yank_main_selection_to_primary_clipboard(cx: &mut Context) {
    yank_main_selection_to_register(cx.editor, '*');
    exit_select_mode(cx);
}

#[derive(Copy, Clone)]
pub(crate) enum Paste {
    Before,
    After,
    Cursor,
}

static LINE_ENDING_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(r"\r\n|\r|\n").unwrap());

fn paste_impl(
    values: &[String],
    doc: &mut Document,
    view: &mut View,
    action: Paste,
    count: usize,
    mode: Mode,
) {
    if values.is_empty() {
        return;
    }

    if mode == Mode::Insert {
        doc.append_changes_to_history(view);
    }

    // if any of values ends with a line ending, it's linewise paste
    let linewise = values
        .iter()
        .any(|value| get_line_ending_of_str(value).is_some());

    let map_value = |value| {
        let value = LINE_ENDING_REGEX.replace_all(value, doc.line_ending.as_str());
        let mut out = Tendril::from(value.as_ref());
        for _ in 1..count {
            out.push_str(&value);
        }
        out
    };

    let repeat = std::iter::repeat(
        // `values` is asserted to have at least one entry above.
        map_value(values.last().unwrap()),
    );

    let mut values = values.iter().map(|value| map_value(value)).chain(repeat);

    let text = doc.text();
    let selection = doc.selection(view.id);

    let mut offset = 0;
    let mut ranges = SmallVec::with_capacity(selection.len());

    let mut transaction = Transaction::change_by_selection(text, selection, |range| {
        let pos = match (action, linewise) {
            // paste linewise before
            (Paste::Before, true) => text.line_to_char(text.char_to_line(range.from())),
            // paste linewise after
            (Paste::After, true) => {
                let line = range.line_range(text.slice(..)).1;
                text.line_to_char((line + 1).min(text.len_lines()))
            }
            // paste insert
            (Paste::Before, false) => range.from(),
            // paste append
            (Paste::After, false) => range.to(),
            // paste at cursor
            (Paste::Cursor, _) => range.cursor(text.slice(..)),
        };

        let value = values.next();

        let value_len = value
            .as_ref()
            .map(|content| content.chars().count())
            .unwrap_or_default();
        let anchor = offset + pos;

        let new_range = Range::new(anchor, anchor + value_len).with_direction(range.direction());
        ranges.push(new_range);
        offset += value_len;

        (pos, pos, value)
    });

    if mode == Mode::Normal {
        transaction = transaction.with_selection(Selection::new(ranges, selection.primary_index()));
    }

    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
}

/// Emacs `yank` (C-y): insert the most-recent kill-ring entry at each cursor
/// and select it, so a following `yank-pop` can swap it for an older kill.
fn yank_from_kill_ring(cx: &mut Context) {
    let Some(text) = crate::emacs_kill::top() else {
        cx.editor.set_error("Kill ring is empty");
        return;
    };
    let count = cx.count();
    let mode = cx.editor.mode;
    let (view, doc) = current!(cx.editor);
    paste_impl(&[text], doc, view, Paste::Before, count, mode);
    let sel: Vec<(usize, usize)> = doc
        .selection(view.id)
        .iter()
        .map(|r| (r.anchor, r.head))
        .collect();
    crate::emacs_kill::begin_yank(sel);
}

/// Emacs `yank-pop` (M-y): replace the just-yanked text with the next-older
/// kill-ring entry, cycling. Only fires while the live selection still covers
/// the previous yank (our stand-in for emacs's last-command-was-yank check).
fn yank_pop(cx: &mut Context) {
    let cur: Vec<(usize, usize)> = {
        let (view, doc) = current!(cx.editor);
        doc.selection(view.id)
            .iter()
            .map(|r| (r.anchor, r.head))
            .collect()
    };
    let Some(entry) = crate::emacs_kill::next_entry(&cur) else {
        cx.editor
            .set_error("Previous command was not a yank — nothing to cycle");
        return;
    };
    let (view, doc) = current!(cx.editor);
    let value = Tendril::from(entry.as_str());
    let value_len = entry.chars().count();
    let selection = doc.selection(view.id).clone();
    let mut offset: isize = 0;
    let mut ranges = SmallVec::with_capacity(selection.len());
    let transaction =
        Transaction::change_by_selection(doc.text(), &selection, |range| {
            let (from, to) = (range.from(), range.to());
            let anchor = (from as isize + offset) as usize;
            ranges.push(Range::new(anchor, anchor + value_len).with_direction(range.direction()));
            offset += value_len as isize - (to as isize - from as isize);
            (from, to, Some(value.clone()))
        });
    doc.apply(&transaction, view.id);
    doc.set_selection(view.id, Selection::new(ranges, selection.primary_index()));
    doc.append_changes_to_history(view);
    let new_sel: Vec<(usize, usize)> = doc
        .selection(view.id)
        .iter()
        .map(|r| (r.anchor, r.head))
        .collect();
    crate::emacs_kill::set_yank_sel(new_sel);
}

pub(crate) fn paste_bracketed_value(cx: &mut Context, contents: String) {
    let count = cx.count();
    let paste = match cx.editor.mode {
        Mode::Insert | Mode::Select => Paste::Cursor,
        Mode::Normal => Paste::Before,
    };
    let (view, doc) = current!(cx.editor);
    paste_impl(&[contents], doc, view, paste, count, cx.editor.mode);
    exit_select_mode(cx);
}

fn paste_clipboard_after(cx: &mut Context) {
    paste(cx.editor, '+', Paste::After, cx.count());
    exit_select_mode(cx);
}

fn paste_clipboard_before(cx: &mut Context) {
    paste(cx.editor, '+', Paste::Before, cx.count());
    exit_select_mode(cx);
}

fn paste_primary_clipboard_after(cx: &mut Context) {
    paste(cx.editor, '*', Paste::After, cx.count());
    exit_select_mode(cx);
}

fn paste_primary_clipboard_before(cx: &mut Context) {
    paste(cx.editor, '*', Paste::Before, cx.count());
    exit_select_mode(cx);
}

fn replace_with_yanked(cx: &mut Context) {
    replace_selections_with_register(
        cx.editor,
        cx.register
            .unwrap_or(cx.editor.config().default_yank_register),
        cx.count(),
    );
    exit_select_mode(cx);
}

pub(crate) fn replace_selections_with_register(editor: &mut Editor, register: char, count: usize) {
    let Some(values) = editor
        .registers
        .read(register, editor)
        .filter(|values| values.len() > 0)
    else {
        return;
    };
    let scrolloff = editor.config().scrolloff;
    let (view, doc) = current_ref!(editor);

    let map_value = |value: &Cow<str>| {
        let value = LINE_ENDING_REGEX.replace_all(value, doc.line_ending.as_str());
        let mut out = Tendril::from(value.as_ref());
        for _ in 1..count {
            out.push_str(&value);
        }
        out
    };
    let mut values_rev = values.rev().peekable();
    // `values` is asserted to have at least one entry above.
    let last = values_rev.peek().unwrap();
    let repeat = std::iter::repeat(map_value(last));
    let mut values = values_rev
        .rev()
        .map(|value| map_value(&value))
        .chain(repeat);
    let selection = doc.selection(view.id);
    let transaction = Transaction::change_by_selection(doc.text(), selection, |range| {
        if !range.is_empty() {
            (range.from(), range.to(), Some(values.next().unwrap()))
        } else {
            (range.from(), range.to(), None)
        }
    });
    drop(values);

    let (view, doc) = current!(editor);
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    view.ensure_cursor_in_view(doc, scrolloff);
}

fn replace_selections_with_clipboard(cx: &mut Context) {
    replace_selections_with_register(cx.editor, '+', cx.count());
    exit_select_mode(cx);
}

fn replace_selections_with_primary_clipboard(cx: &mut Context) {
    replace_selections_with_register(cx.editor, '*', cx.count());
    exit_select_mode(cx);
}

pub(crate) fn paste(editor: &mut Editor, register: char, pos: Paste, count: usize) {
    let Some(values) = editor.registers.read(register, editor) else {
        return;
    };
    let values: Vec<_> = values.map(|value| value.to_string()).collect();

    let (view, doc) = current!(editor);
    paste_impl(&values, doc, view, pos, count, editor.mode);
}

fn paste_after(cx: &mut Context) {
    paste(
        cx.editor,
        cx.register
            .unwrap_or(cx.editor.config().default_yank_register),
        Paste::After,
        cx.count(),
    );
    exit_select_mode(cx);
}

/// Browse the written registers (named registers + yank ring) in a picker and
/// paste the chosen one after the cursor. Backs spacemacs `SPC r r/e/y`.
fn register_picker(cx: &mut Context) {
    struct RegMeta {
        name: char,
        preview: String,
    }

    let items: Vec<RegMeta> = cx
        .editor
        .registers
        .written()
        .into_iter()
        .filter_map(|name| {
            let preview = cx
                .editor
                .registers
                .read(name, cx.editor)?
                .map(|v| v.into_owned())
                .collect::<Vec<_>>()
                .join(" ⏎ ")
                .replace('\n', "⏎");
            (!preview.is_empty()).then_some(RegMeta { name, preview })
        })
        .collect();

    let columns = [
        ui::PickerColumn::new("reg", |m: &RegMeta, _: &()| m.name.to_string().into()),
        ui::PickerColumn::new("contents", |m: &RegMeta, _: &()| m.preview.as_str().into()),
    ];

    let picker = Picker::new(columns, 1, items, (), |cx, meta, _action| {
        paste(cx.editor, meta.name, Paste::After, 1);
    });
    cx.push_layer(Box::new(overlaid(picker)));
}

/// fzf.vim `:Marks` / spacemacs `SPC r m` (helm-mark-ring): fuzzy-pick a vim
/// named mark in the current buffer and jump to it. Marks are stored per-document
/// (`m{a-z}` sets them, plus the auto-marks `.`/`[`/`]`/`^`/`<`/`>`).
fn marks_picker(cx: &mut Context) {
    struct MarkMeta {
        mark: char,
        doc_id: DocumentId,
        pos: usize,
        line: usize,
        text: String,
    }

    let (_, doc) = current_ref!(cx.editor);
    let doc_id = doc.id();
    let text = doc.text().slice(..);
    let len = text.len_chars();

    let mut items: Vec<MarkMeta> = doc
        .marks_iter()
        .map(|(mark, pos)| {
            let pos = pos.min(len);
            let line = text.char_to_line(pos);
            let contents = text.line(line).to_string();
            MarkMeta {
                mark,
                doc_id,
                pos,
                line,
                text: contents.trim_end().to_string(),
            }
        })
        .collect();
    // letters first, then the structural/auto marks, for a stable, vim-like order
    items.sort_by_key(|m| (!m.mark.is_ascii_alphabetic(), m.mark));

    if items.is_empty() {
        cx.editor.set_status("No marks set");
        return;
    }

    let columns = [
        ui::PickerColumn::new("mark", |m: &MarkMeta, _: &()| m.mark.to_string().into()),
        ui::PickerColumn::new("line", |m: &MarkMeta, _: &()| {
            (m.line + 1).to_string().into()
        }),
        ui::PickerColumn::new("text", |m: &MarkMeta, _: &()| m.text.as_str().into()),
    ];

    let picker = Picker::new(columns, 2, items, (), |cx, meta, _action| {
        let (view, doc) = current!(cx.editor);
        push_jump(view, doc);
        let pos = meta.pos.min(doc.text().len_chars());
        doc.set_selection(view.id, Selection::point(pos));
    })
    .with_preview(|_editor, meta| Some((meta.doc_id.into(), Some((meta.line, meta.line)))));
    cx.push_layer(Box::new(overlaid(picker)));
}

/// fzf.vim `:BLines`: fuzzy-search every line of the current buffer and jump to
/// the chosen one. (Project-wide line search is `SPC s s` / `global_search`.)
fn buffer_line_picker(cx: &mut Context) {
    struct LineMeta {
        doc_id: DocumentId,
        line: usize,
        pos: usize,
        text: String,
    }

    let (_, doc) = current_ref!(cx.editor);
    let doc_id = doc.id();
    let text = doc.text().slice(..);

    let items: Vec<LineMeta> = text
        .lines()
        .enumerate()
        .map(|(line, slice)| LineMeta {
            doc_id,
            line,
            pos: text.line_to_char(line),
            text: slice.to_string().trim_end().to_string(),
        })
        .collect();

    let columns = [
        ui::PickerColumn::new("line", |m: &LineMeta, _: &()| {
            (m.line + 1).to_string().into()
        }),
        ui::PickerColumn::new("text", |m: &LineMeta, _: &()| m.text.as_str().into()),
    ];

    let picker = Picker::new(columns, 1, items, (), |cx, meta, _action| {
        let (view, doc) = current!(cx.editor);
        push_jump(view, doc);
        let pos = meta.pos.min(doc.text().len_chars());
        doc.set_selection(view.id, Selection::point(pos));
        let config = cx.editor.config();
        let (view, doc) = current!(cx.editor);
        view.ensure_cursor_in_view_center(doc, config.scrolloff);
    })
    .with_preview(|_editor, meta| Some((meta.doc_id.into(), Some((meta.line, meta.line)))));
    cx.push_layer(Box::new(overlaid(picker)));
}

/// fzf.vim `:History:` : fuzzy-pick a past command line (`:` register history)
/// and execute it.
fn command_history_picker(cx: &mut Context) {
    let mut items: Vec<String> = cx
        .editor
        .registers
        .read(':', cx.editor)
        .map(|values| values.map(|v| v.into_owned()).collect())
        .unwrap_or_default();
    items.reverse(); // most-recent first

    if items.is_empty() {
        cx.editor.set_status("No command-line history");
        return;
    }

    let columns = [ui::PickerColumn::new("command", |cmd: &String, _: &()| {
        cmd.as_str().into()
    })];

    let picker = Picker::new(columns, 0, items, (), |cx, cmd, _action| {
        crate::commands::typed::run_command_line(cx, cmd);
    });
    cx.push_layer(Box::new(overlaid(picker)));
}

/// fzf.vim `:History/` : fuzzy-pick a past search pattern (`/` register history)
/// and re-run the search.
fn search_history_picker(cx: &mut Context) {
    let mut items: Vec<String> = cx
        .editor
        .registers
        .read('/', cx.editor)
        .map(|values| values.map(|v| v.into_owned()).collect())
        .unwrap_or_default();
    items.reverse(); // most-recent first

    if items.is_empty() {
        cx.editor.set_status("No search history");
        return;
    }

    let columns = [ui::PickerColumn::new("pattern", |p: &String, _: &()| {
        p.as_str().into()
    })];

    let picker = Picker::new(columns, 0, items, (), |cx, query, _action| {
        let _ = cx.editor.registers.write('/', vec![query.clone()]);
        cx.editor.registers.last_search_register = '/';
        let config = cx.editor.config();
        let case_insensitive = config.search.smart_case && !query.chars().any(char::is_uppercase);
        let wrap_around = config.search.wrap_around;
        let scrolloff = config.scrolloff;
        let is_crlf = doc!(cx.editor).line_ending == LineEnding::Crlf;
        if let Ok(regex) = rope::RegexBuilder::new()
            .syntax(
                rope::Config::new()
                    .case_insensitive(case_insensitive)
                    .multi_line(true)
                    .crlf(is_crlf),
            )
            .build(query)
        {
            search_impl(
                cx.editor,
                &regex,
                Movement::Move,
                Direction::Forward,
                scrolloff,
                wrap_around,
                true,
            );
        } else {
            cx.editor.set_error(format!("Invalid regex: {query}"));
        }
    });
    cx.push_layer(Box::new(overlaid(picker)));
}

/// fzf.vim `:BCommits` (current file) / `:Commits` (whole repo), and spacemacs
/// `SPC g f l` (commits log for current file): fuzzy-pick a commit from history.
/// Enter copies the commit hash to the clipboard (there is no commit-diff viewer yet).
fn git_log_picker(cx: &mut Context, current_file_only: bool) {
    let Some(path) = doc!(cx.editor).path().map(|p| p.to_path_buf()) else {
        cx.editor.set_error("Current buffer has no path");
        return;
    };
    let Some(repo_dir) = path.parent().map(|p| p.to_path_buf()) else {
        cx.editor.set_error("File has no parent directory");
        return;
    };

    let trust_full = cx
        .editor
        .workspace_trust
        .query(
            &zemacs_loader::find_workspace_in(&repo_dir).0,
            zemacs_loader::workspace_trust::TrustQuery::Git,
        )
        .is_trusted();

    let file_arg = current_file_only.then(|| path.clone());

    let columns = [
        ui::PickerColumn::new("hash", |c: &CommitInfo, _: &()| c.id.as_str().into()),
        ui::PickerColumn::new("summary", |c: &CommitInfo, _: &()| {
            c.summary.as_str().into()
        }),
        ui::PickerColumn::new("author", |c: &CommitInfo, _: &()| c.author.as_str().into()),
    ];

    let picker = Picker::new(columns, 1, [], (), |cx, commit, _action| {
        let _ = cx.editor.registers.write('+', vec![commit.id.clone()]);
        cx.editor
            .set_status(format!("Copied commit {} to clipboard", commit.id));
    });
    let injector = picker.injector();

    // Walk history on a background thread, streaming commits into the picker.
    cx.editor.diff_providers.clone().for_each_commit(
        repo_dir,
        file_arg,
        trust_full,
        256,
        move |commit| match commit {
            Ok(commit) => injector.push(commit).is_ok(),
            Err(err) => {
                status::report_blocking(err);
                true
            }
        },
    );
    cx.push_layer(Box::new(overlaid(picker)));
}

/// fzf.vim `:BCommits` / spacemacs `SPC g f l`: commit log for the current file.
fn git_file_log_picker(cx: &mut Context) {
    git_log_picker(cx, true);
}

/// fzf.vim `:Commits`: commit log for the whole repository.
fn git_repo_log_picker(cx: &mut Context) {
    git_log_picker(cx, false);
}

/// spacemacs `SPC i u` (helm-unicode): fuzzy-pick a character from the digraph
/// table by its mnemonic / glyph and insert it at the cursor.
fn unicode_picker(cx: &mut Context) {
    struct CharMeta {
        ch: char,
        mnemonic: String,
    }

    let items: Vec<CharMeta> = DIGRAPHS
        .iter()
        .map(|&(a, b, ch)| CharMeta {
            ch,
            mnemonic: format!("{a}{b}"),
        })
        .collect();

    let columns = [
        ui::PickerColumn::new("char", |m: &CharMeta, _: &()| m.ch.to_string().into()),
        ui::PickerColumn::new("mnemonic", |m: &CharMeta, _: &()| {
            m.mnemonic.as_str().into()
        }),
        ui::PickerColumn::new("code", |m: &CharMeta, _: &()| {
            format!("U+{:04X}", m.ch as u32).into()
        }),
    ];

    let picker = Picker::new(columns, 1, items, (), |cx, meta, _action| {
        let mut ctx = Context {
            register: None,
            count: None,
            editor: cx.editor,
            callback: Vec::new(),
            on_next_key_callback: None,
            jobs: cx.jobs,
        };
        insert::insert_char(&mut ctx, meta.ch);
    });
    cx.push_layer(Box::new(overlaid(picker)));
}

/// Fuzzy theme picker with live preview, like vim/fzf.vim `:Colors`. Bound to `SPC T c`.
/// Moving the highlight previews the theme live; Enter commits, Esc reverts.
fn theme_picker(cx: &mut Context) {
    let current = cx.editor.theme.name().to_string();
    let themes = crate::commands::typed::all_theme_names();
    let initial = themes.iter().position(|n| n == &current).unwrap_or(0) as u32;

    let columns = [ui::PickerColumn::new("theme", |name: &String, _: &()| {
        name.as_str().into()
    })];

    let picker = Picker::new(
        columns,
        0,
        themes,
        (),
        |cx, name: &String, _action| match cx.editor.theme_loader.load(name) {
            Ok(theme) => {
                if let Err(err) = cx.editor.set_theme(theme) {
                    cx.editor
                        .set_error(format!("failed to set theme '{name}': {err}"));
                }
            }
            Err(err) => cx
                .editor
                .set_error(format!("failed to load theme '{name}': {err}")),
        },
    )
    .with_initial_cursor(initial)
    // Show the current buffer in the preview pane so you see real code re-themed
    // live as you move through the list (and the picker no longer hides everything).
    .with_preview(|editor, _name: &String| {
        let doc_id = editor.tree.try_get(editor.tree.focus)?.doc;
        Some((doc_id.into(), None))
    })
    .with_on_highlight(|cx, name: Option<&String>| {
        if let Some(name) = name {
            if let Ok(theme) = cx.editor.theme_loader.load(name) {
                let _ = cx.editor.set_theme_preview(theme);
            }
        }
    })
    .with_on_abort(|cx| {
        let _ = cx.editor.unset_theme_preview();
    });

    cx.push_layer(Box::new(overlaid(picker)));
}

fn paste_before(cx: &mut Context) {
    paste(
        cx.editor,
        cx.register
            .unwrap_or(cx.editor.config().default_yank_register),
        Paste::Before,
        cx.count(),
    );
    exit_select_mode(cx);
}

fn get_lines(doc: &Document, view_id: ViewId) -> Vec<usize> {
    let mut lines = Vec::new();

    // Get all line numbers
    for range in doc.selection(view_id) {
        let (start, end) = range.line_range(doc.text().slice(..));

        for line in start..=end {
            lines.push(line)
        }
    }
    lines.sort_unstable(); // sorting by usize so _unstable is preferred
    lines.dedup();
    lines
}

fn indent(cx: &mut Context) {
    let count = cx.count();
    let (view, doc) = current!(cx.editor);
    let lines = get_lines(doc, view.id);

    // Indent by one level
    let indent = Tendril::from(doc.indent_style.as_str().repeat(count));

    let transaction = Transaction::change(
        doc.text(),
        lines.into_iter().filter_map(|line| {
            let is_blank = doc.text().line(line).chunks().all(|s| s.trim().is_empty());
            if is_blank {
                return None;
            }
            let pos = doc.text().line_to_char(line);

            let indent = if let IndentStyle::Spaces(indent_width) = doc.indent_style {
                let line = doc.text().line(line);
                let offset = line.first_non_whitespace_char().unwrap_or(0) % indent_width as usize;
                indent.clone().split_off(offset)
            } else {
                indent.clone()
            };

            Some((pos, pos, Some(indent)))
        }),
    );
    doc.apply(&transaction, view.id);
    exit_select_mode(cx);
}

fn unindent(cx: &mut Context) {
    let count = cx.count();
    let (view, doc) = current!(cx.editor);
    let lines = get_lines(doc, view.id);
    let mut changes = Vec::with_capacity(lines.len());
    let tab_width = doc.tab_width();
    let indent_width = count * doc.indent_width();

    for line_idx in lines {
        let line = doc.text().line(line_idx);
        let mut width = 0;
        let mut pos = 0;

        for ch in line.chars() {
            match ch {
                ' ' => width += 1,
                '\t' => width = (width / tab_width + 1) * tab_width,
                _ => break,
            }

            pos += 1;

            if width >= indent_width {
                break;
            }
        }

        // now delete from start to first non-blank
        if pos > 0 {
            let start = doc.text().line_to_char(line_idx);
            changes.push((start, start + pos, None))
        }
    }

    let transaction = Transaction::change(doc.text(), changes.into_iter());

    doc.apply(&transaction, view.id);
    exit_select_mode(cx);
}

fn format_selections(cx: &mut Context) {
    use zemacs_lsp::{lsp, util::range_to_lsp_range};

    let (view, doc) = current!(cx.editor);
    let view_id = view.id;

    // via lsp if available
    // TODO: else via tree-sitter indentation calculations

    if doc.selection(view_id).len() != 1 {
        cx.editor
            .set_error("format_selections only supports a single selection for now");
        return;
    }

    // TODO extra LanguageServerFeature::FormatSelections?
    // maybe such that LanguageServerFeature::Format contains it as well
    let Some(language_server) = doc
        .language_servers_with_feature(LanguageServerFeature::Format)
        .find(|ls| {
            matches!(
                ls.capabilities().document_range_formatting_provider,
                Some(lsp::OneOf::Left(true) | lsp::OneOf::Right(_))
            )
        })
    else {
        cx.editor
            .set_error("No configured language server supports range formatting");
        return;
    };

    let offset_encoding = language_server.offset_encoding();
    let ranges: Vec<lsp::Range> = doc
        .selection(view_id)
        .iter()
        .map(|range| range_to_lsp_range(doc.text(), *range, offset_encoding))
        .collect();

    // TODO: handle fails
    // TODO: concurrent map over all ranges

    let range = ranges[0];

    let future = language_server
        .text_document_range_formatting(
            doc.identifier(),
            range,
            lsp::FormattingOptions {
                tab_size: doc.tab_width() as u32,
                insert_spaces: matches!(doc.indent_style, IndentStyle::Spaces(_)),
                ..Default::default()
            },
            None,
        )
        .unwrap();

    let text = doc.text().clone();
    let doc_id = doc.id();
    let doc_version = doc.version();

    tokio::spawn(async move {
        match future.await {
            Ok(Some(res)) => {
                let transaction =
                    zemacs_lsp::util::generate_transaction_from_edits(&text, res, offset_encoding);
                job::dispatch(move |editor, _compositor| {
                    let Some(doc) = editor.document_mut(doc_id) else {
                        return;
                    };
                    // Updating a desynced document causes problems with applying the transaction
                    if doc.version() != doc_version {
                        return;
                    }
                    doc.apply(&transaction, view_id);
                })
                .await
            }
            Err(err) => log::error!("format sections failed: {err}"),
            Ok(None) => (),
        }
    });
}

fn join_selections_impl(cx: &mut Context, select_space: bool) {
    use movement::skip_while;
    let loader = cx.editor.syn_loader.load();
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let slice = text.slice(..);

    let mut changes = Vec::new();

    for selection in doc.selection(view.id) {
        let (start, mut end) = selection.line_range(slice);
        if start == end {
            end = (end + 1).min(text.len_lines() - 1);
        }
        let lines = start..end;

        changes.reserve(lines.len());

        // Strip the comment leader of joined lines using the comment tokens of the layer at this selection.
        let byte = slice.char_to_byte(slice.line_to_char(start));
        let comment_tokens = doc
            .language_config_at(&loader, byte)
            .and_then(|config| config.comment_tokens.as_deref())
            .unwrap_or(&[]);
        // Sort by length to handle Rust's /// vs //
        let mut comment_tokens: Vec<&str> = comment_tokens.iter().map(|x| x.as_str()).collect();
        comment_tokens.sort_unstable_by_key(|x| std::cmp::Reverse(x.len()));

        let first_line_idx = slice.line_to_char(start);
        let first_line_idx = skip_while(slice, first_line_idx, |ch| matches!(ch, ' ' | '\t'))
            .unwrap_or(first_line_idx);
        let first_line = slice.slice(first_line_idx..);
        let mut current_comment_token = comment_tokens
            .iter()
            .find(|token| first_line.starts_with(token));

        for line in lines {
            let start = line_end_char_index(&slice, line);
            let mut end = text.line_to_char(line + 1);
            end = skip_while(slice, end, |ch| matches!(ch, ' ' | '\t')).unwrap_or(end);
            let slice_from_end = slice.slice(end..);
            if let Some(token) = comment_tokens
                .iter()
                .find(|token| slice_from_end.starts_with(token))
            {
                if Some(token) == current_comment_token {
                    end += token.chars().count();
                    end = skip_while(slice, end, |ch| matches!(ch, ' ' | '\t')).unwrap_or(end);
                } else {
                    // update current token, but don't delete this one.
                    current_comment_token = Some(token);
                }
            }

            let separator = if end == line_end_char_index(&slice, line + 1) {
                // the joining line contains only space-characters => don't include a whitespace when joining
                None
            } else {
                Some(Tendril::from(" "))
            };
            changes.push((start, end, separator));
        }
    }

    // nothing to do, bail out early to avoid crashes later
    if changes.is_empty() {
        return;
    }

    changes.sort_unstable_by_key(|(from, _to, _text)| *from);
    changes.dedup();

    // select inserted spaces
    let transaction = if select_space {
        let mut offset: usize = 0;
        let ranges: SmallVec<_> = changes
            .iter()
            .filter_map(|change| {
                if change.2.is_some() {
                    let range = Range::point(change.0 - offset);
                    offset += change.1 - change.0 - 1; // -1 adjusts for the replacement of the range by a space
                    Some(range)
                } else {
                    offset += change.1 - change.0;
                    None
                }
            })
            .collect();
        let t = Transaction::change(text, changes.into_iter());
        if ranges.is_empty() {
            t
        } else {
            let selection = Selection::new(ranges, 0);
            t.with_selection(selection)
        }
    } else {
        Transaction::change(text, changes.into_iter())
    };

    doc.apply(&transaction, view.id);
}

fn keep_or_remove_selections_impl(cx: &mut Context, remove: bool) {
    // keep or remove selections matching regex
    let reg = cx.register.unwrap_or('/');
    ui::regex_prompt(
        cx,
        if remove { "remove:" } else { "keep:" }.into(),
        Some(reg),
        ui::completers::none,
        move |cx, regex, event| {
            let (view, doc) = current!(cx.editor);
            if !matches!(event, PromptEvent::Update | PromptEvent::Validate) {
                return;
            }
            let text = doc.text().slice(..);

            if let Some(selection) =
                selection::keep_or_remove_matches(text, doc.selection(view.id), &regex, remove)
            {
                doc.set_selection(view.id, selection);
            } else if event == PromptEvent::Validate {
                cx.editor.set_error("no selections remaining");
            }
        },
    )
}

fn join_selections(cx: &mut Context) {
    join_selections_impl(cx, false)
}

fn join_selections_space(cx: &mut Context) {
    join_selections_impl(cx, true)
}

fn keep_selections(cx: &mut Context) {
    keep_or_remove_selections_impl(cx, false)
}

fn remove_selections(cx: &mut Context) {
    keep_or_remove_selections_impl(cx, true)
}

fn keep_primary_selection(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);
    // TODO: handle count

    let range = doc.selection(view.id).primary();
    doc.set_selection(view.id, Selection::single(range.anchor, range.head));
}

fn remove_primary_selection(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);
    // TODO: handle count

    let selection = doc.selection(view.id);
    if selection.len() == 1 {
        cx.editor.set_error("no selections remaining");
        return;
    }
    let index = selection.primary_index();
    let selection = selection.clone().remove(index);

    doc.set_selection(view.id, selection);
}

pub fn completion(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);
    let range = doc.selection(view.id).primary();
    let text = doc.text().slice(..);
    let cursor = range.cursor(text);

    cx.editor
        .handlers
        .trigger_completions(cursor, doc.id(), view.id);
}

// comments
type CommentTransactionFn = fn(
    line_token: Option<&str>,
    block_tokens: Option<&[BlockCommentToken]>,
    doc: &Rope,
    selection: &Selection,
) -> Transaction;

fn toggle_comments_impl(cx: &mut Context, comment_transaction: CommentTransactionFn) {
    apply_comment_transaction(cx.editor, comment_transaction);
    exit_select_mode(cx);
}

/// Resolve the comment tokens for the cursor's enclosing language layer, build the transaction
/// via `comment_transaction`, and apply it to the current selection. Operates on `&mut Editor`
/// alone so it can run from a prompt callback (where no full `Context` is available).
fn apply_comment_transaction(editor: &mut Editor, comment_transaction: CommentTransactionFn) {
    let loader: &zemacs_core::syntax::Loader = &editor.syn_loader.load();
    let (view, doc) = current!(editor);
    let cursor = doc
        .selection(view.id)
        .primary()
        .cursor(doc.text().slice(..));
    let byte_pos = doc.text().char_to_byte(cursor);
    // Resolve the comment tokens from the enclosing injection layer that owns the comment,
    // not the innermost layer at the cursor. Prefer the innermost layer that defines
    // *line* comment tokens, falling back to the innermost layer with block tokens.
    let mut line_layer = None;
    let mut block_layer = None;
    if let Some(syntax) = doc.syntax() {
        for layer in syntax.layers_for_byte_range(byte_pos as u32, byte_pos as u32) {
            let language = syntax.layer(layer).language;
            let config = loader.language(language).config();
            if config.comment_tokens.is_some() {
                line_layer = Some(language);
            }
            if config.block_comment_tokens.is_some() {
                block_layer = Some(language);
            }
        }
    }
    let lang_config = line_layer
        .or(block_layer)
        .map(|language| &**loader.language(language).config())
        .or_else(|| doc.language_config());

    // Pick the token the cursor's line is already commented with (longest match, so `///` wins over `//`).
    // If the line isn't commented yet, fall back to the primary token for adding a comment.
    let cursor_line = doc.text().char_to_line(cursor);
    let line_token: Option<&str> = lang_config
        .and_then(|lc| lc.comment_tokens.as_ref())
        .and_then(|tokens| {
            comment::get_comment_token(doc.text().slice(..), tokens, cursor_line)
                .or_else(|| tokens.first().map(|token| token.as_str()))
        });
    let block_tokens: Option<&[BlockCommentToken]> = lang_config
        .and_then(|lc| lc.block_comment_tokens.as_ref())
        .map(|tc| &tc[..]);

    let transaction =
        comment_transaction(line_token, block_tokens, doc.text(), doc.selection(view.id));

    doc.apply(&transaction, view.id);
}

/// commenting behavior:
/// 1. only line comment tokens -> line comment
/// 2. each line block commented -> uncomment all lines
/// 3. whole selection block commented -> uncomment selection
/// 4. all lines not commented and block tokens -> comment uncommented lines
/// 5. no comment tokens and not block commented -> line comment
fn toggle_comments(cx: &mut Context) {
    toggle_comments_impl(cx, |line_token, block_tokens, doc, selection| {
        let text = doc.slice(..);

        // only have line comment tokens
        if line_token.is_some() && block_tokens.is_none() {
            return comment::toggle_line_comments(doc, selection, line_token);
        }

        let split_lines = comment::split_lines_of_selection(text, selection);

        let default_block_tokens = &[BlockCommentToken::default()];
        let block_comment_tokens = block_tokens.unwrap_or(default_block_tokens);

        let (line_commented, line_comment_changes) =
            comment::find_block_comments(block_comment_tokens, text, &split_lines);

        // block commented by line would also be block commented so check this first
        if line_commented {
            return comment::create_block_comment_transaction(
                doc,
                &split_lines,
                line_commented,
                line_comment_changes,
            )
            .0;
        }

        let (block_commented, comment_changes) =
            comment::find_block_comments(block_comment_tokens, text, selection);

        // check if selection has block comments
        if block_commented {
            return comment::create_block_comment_transaction(
                doc,
                selection,
                block_commented,
                comment_changes,
            )
            .0;
        }

        // not commented and only have block comment tokens
        if line_token.is_none() && block_tokens.is_some() {
            return comment::create_block_comment_transaction(
                doc,
                &split_lines,
                line_commented,
                line_comment_changes,
            )
            .0;
        }

        // not block commented at all and don't have any tokens
        comment::toggle_line_comments(doc, selection, line_token)
    })
}

/// Comment the range of lines between the cursor's line and `target_line` (0-based, clamped to the
/// buffer). When `invert`, each line is toggled independently (Spacemacs
/// `evilnc-invert-comment-line-by-line`); otherwise the whole range shares one decision
/// (`evilnc-comment-or-uncomment-to-the-line`).
fn comment_line_range(editor: &mut Editor, target_line: usize, invert: bool) {
    {
        let (view, doc) = current!(editor);
        let text = doc.text();
        let slice = text.slice(..);
        let cursor = doc.selection(view.id).primary().cursor(slice);
        let cur_line = text.char_to_line(cursor);
        let last = text.len_lines().saturating_sub(1);
        let target = target_line.min(last);
        let (lo, hi) = if cur_line <= target {
            (cur_line, target)
        } else {
            (target, cur_line)
        };
        let start = text.line_to_char(lo);
        let end = (text.line_to_char(hi) + text.line(hi).len_chars()).min(text.len_chars());
        doc.set_selection(view.id, Selection::single(start, end));
    }
    if invert {
        apply_comment_transaction(editor, |line_token, _block_tokens, doc, selection| {
            comment::invert_line_comments(doc, selection, line_token)
        });
    } else {
        apply_comment_transaction(editor, |line_token, _block_tokens, doc, selection| {
            comment::toggle_line_comments(doc, selection, line_token)
        });
    }
}

/// Prompt for a 1-based target line (or take a count prefix) and comment the range from the
/// cursor's line to it; `invert` toggles each line independently.
fn comment_to_line_prompt(cx: &mut Context, invert: bool) {
    let cur_line = {
        let (view, doc) = current!(cx.editor);
        let slice = doc.text().slice(..);
        doc.text()
            .char_to_line(doc.selection(view.id).primary().cursor(slice))
            + 1
    };
    if let Some(count) = cx.count {
        comment_line_range(cx.editor, count.get().saturating_sub(1), invert);
        return;
    }
    let label = if invert {
        format!("Invert comment to line (cursor at {cur_line}): ")
    } else {
        format!("Comment to line (cursor at {cur_line}): ")
    };
    let prompt = crate::ui::prompt::Prompt::new(
        label.into(),
        None,
        ui::completers::none,
        move |cx: &mut crate::compositor::Context, input: &str, event: PromptEvent| {
            if event != PromptEvent::Validate {
                return;
            }
            match input.trim().parse::<usize>() {
                Ok(n) if n >= 1 => comment_line_range(cx.editor, n - 1, invert),
                _ => cx.editor.set_error("comment to line: enter a 1-based line number"),
            }
        },
    );
    cx.push_layer(Box::new(prompt));
}

/// SPC c t : line-comment the range from the cursor to a prompted line (uniform).
fn comment_to_line(cx: &mut Context) {
    comment_to_line_prompt(cx, false);
}

/// SPC c T : invert (per-line toggle) comments over the range from the cursor to a prompted line.
fn invert_comment_to_line(cx: &mut Context) {
    comment_to_line_prompt(cx, true);
}

fn toggle_line_comments(cx: &mut Context) {
    toggle_comments_impl(cx, |line_token, block_tokens, doc, selection| {
        if line_token.is_none() && block_tokens.is_some() {
            let default_block_tokens = &[BlockCommentToken::default()];
            let block_comment_tokens = block_tokens.unwrap_or(default_block_tokens);
            comment::toggle_block_comments(
                doc,
                &comment::split_lines_of_selection(doc.slice(..), selection),
                block_comment_tokens,
            )
        } else {
            comment::toggle_line_comments(doc, selection, line_token)
        }
    });
}

fn toggle_block_comments(cx: &mut Context) {
    toggle_comments_impl(cx, |line_token, block_tokens, doc, selection| {
        if line_token.is_some() && block_tokens.is_none() {
            comment::toggle_line_comments(doc, selection, line_token)
        } else {
            let default_block_tokens = &[BlockCommentToken::default()];
            let block_comment_tokens = block_tokens.unwrap_or(default_block_tokens);
            comment::toggle_block_comments(doc, selection, block_comment_tokens)
        }
    });
}

fn rotate_selections(cx: &mut Context, direction: Direction) {
    let count = cx.count();
    let (view, doc) = current!(cx.editor);
    let mut selection = doc.selection(view.id).clone();
    let index = selection.primary_index();
    let len = selection.len();
    selection.set_primary_index(match direction {
        Direction::Forward => (index + count) % len,
        Direction::Backward => (index + (len.saturating_sub(count) % len)) % len,
    });
    doc.set_selection(view.id, selection);
}
fn rotate_selections_forward(cx: &mut Context) {
    rotate_selections(cx, Direction::Forward)
}
fn rotate_selections_backward(cx: &mut Context) {
    rotate_selections(cx, Direction::Backward)
}

fn rotate_selections_first(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);
    let mut selection = doc.selection(view.id).clone();
    selection.set_primary_index(0);
    doc.set_selection(view.id, selection);
}

fn rotate_selections_last(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);
    let mut selection = doc.selection(view.id).clone();
    let len = selection.len();
    selection.set_primary_index(len - 1);
    doc.set_selection(view.id, selection);
}

#[derive(Debug)]
enum ReorderStrategy {
    RotateForward,
    RotateBackward,
    Reverse,
}

fn reorder_selection_contents(cx: &mut Context, strategy: ReorderStrategy) {
    let count = cx.count;
    let (view, doc) = current!(cx.editor);
    let text = doc.text().slice(..);

    let selection = doc.selection(view.id);

    let mut ranges: Vec<_> = selection
        .slices(text)
        .map(|fragment| fragment.chunks().collect())
        .collect();

    let rotate_by = count.map_or(1, |count| count.get().min(ranges.len()));

    let primary_index = match strategy {
        ReorderStrategy::RotateForward => {
            ranges.rotate_right(rotate_by);
            // Like `usize::wrapping_add`, but provide a custom range from `0` to `ranges.len()`
            (selection.primary_index() + ranges.len() + rotate_by) % ranges.len()
        }
        ReorderStrategy::RotateBackward => {
            ranges.rotate_left(rotate_by);
            // Like `usize::wrapping_sub`, but provide a custom range from `0` to `ranges.len()`
            (selection.primary_index() + ranges.len() - rotate_by) % ranges.len()
        }
        ReorderStrategy::Reverse => {
            if rotate_by.is_multiple_of(2) {
                // nothing changed, if we reverse something an even
                // amount of times, the output will be the same
                return;
            }
            ranges.reverse();
            // -1 to turn 1-based len into 0-based index
            (ranges.len() - 1) - selection.primary_index()
        }
    };

    let transaction = Transaction::change(
        doc.text(),
        selection
            .ranges()
            .iter()
            .zip(ranges)
            .map(|(range, fragment)| (range.from(), range.to(), Some(fragment))),
    );

    doc.set_selection(
        view.id,
        Selection::new(selection.ranges().into(), primary_index),
    );
    doc.apply(&transaction, view.id);
}

fn rotate_selection_contents_forward(cx: &mut Context) {
    reorder_selection_contents(cx, ReorderStrategy::RotateForward)
}
fn rotate_selection_contents_backward(cx: &mut Context) {
    reorder_selection_contents(cx, ReorderStrategy::RotateBackward)
}
fn reverse_selection_contents(cx: &mut Context) {
    reorder_selection_contents(cx, ReorderStrategy::Reverse)
}

// tree sitter node selection

fn expand_selection(cx: &mut Context) {
    let motion = |editor: &mut Editor| {
        let (view, doc) = current!(editor);

        if let Some(syntax) = doc.syntax() {
            let text = doc.text().slice(..);

            let current_selection = doc.selection(view.id);
            let selection = object::expand_selection(syntax, text, current_selection.clone());

            // check if selection is different from the last one
            if *current_selection != selection {
                // save current selection so it can be restored using shrink_selection
                view.object_selections.push(current_selection.clone());

                doc.set_selection(view.id, selection);
            }
        }
    };
    cx.editor.apply_motion(motion);
}

fn shrink_selection(cx: &mut Context) {
    let motion = |editor: &mut Editor| {
        let (view, doc) = current!(editor);
        let current_selection = doc.selection(view.id);
        // try to restore previous selection
        if let Some(prev_selection) = view.object_selections.pop() {
            if current_selection.contains(&prev_selection) {
                doc.set_selection(view.id, prev_selection);
                return;
            } else {
                // clear existing selection as they can't be shrunk to anyway
                view.object_selections.clear();
            }
        }
        // if not previous selection, shrink to first child
        if let Some(syntax) = doc.syntax() {
            let text = doc.text().slice(..);
            let selection = object::shrink_selection(syntax, text, current_selection.clone());
            doc.set_selection(view.id, selection);
        }
    };
    cx.editor.apply_motion(motion);
}

/// Returns true if `outer` fully contains `inner` and is strictly larger than
/// it (i.e. they are not equal). Direction is ignored; only the covered span
/// (`from()`..`to()`) matters.
fn wildfire_range_strictly_contains(outer: Range, inner: Range) -> bool {
    outer.from() <= inner.from()
        && outer.to() >= inner.to()
        && (outer.from() < inner.from() || outer.to() > inner.to())
}

/// Compute the wildfire target for a single range.
///
/// * With `count > 1` we jump directly to the Nth closest enclosing pair's
///   inside (like the count-aware text-object commands).
/// * With `count == 1` (a plain `<Enter>`) we must grow *strictly*: because
///   `find_nth_closest_pairs_pos` on a range that is already a pair's inside
///   can return that very same pair, we retry with an increasing `skip`
///   (Nth-closest) until the result strictly contains the current range. If no
///   larger enclosing object exists, the range is returned unchanged.
fn wildfire_grow_range(
    syntax: Option<&Syntax>,
    slice: RopeSlice,
    range: Range,
    count: usize,
) -> Range {
    use textobject::{textobject_pair_surround_closest, TextObject};

    if count > 1 {
        return textobject_pair_surround_closest(syntax, slice, range, TextObject::Inside, count);
    }

    // Grow strictly: try successively larger enclosing pairs.
    let mut last: Option<Range> = None;
    let mut skip = 1usize;
    // Bound the search so a pathological document can never spin forever.
    while skip <= 64 {
        let candidate =
            textobject_pair_surround_closest(syntax, slice, range, TextObject::Inside, skip);
        if wildfire_range_strictly_contains(candidate, range) {
            return candidate;
        }
        // `textobject_pair_surround_closest` returns the input range unchanged
        // when no pair is found, so once the candidate stops changing there is
        // no larger object to grow into.
        if last == Some(candidate) {
            break;
        }
        last = Some(candidate);
        skip += 1;
    }
    range
}

/// Wildfire: select / expand to the closest enclosing text object.
///
/// Each plain `<Enter>` grows the selection to the inside of the next-larger
/// enclosing pair (quote/paren/bracket/brace/tag). `N<Enter>` jumps straight to
/// the Nth closest. The previous selection is pushed onto the per-view
/// `object_selections` stack so `wildfire_shrink` (`<BS>`) can restore it.
fn wildfire(cx: &mut Context) {
    let count = cx.count();
    let motion = move |editor: &mut Editor| {
        let (view, doc) = current!(editor);
        let text = doc.text().slice(..);
        let current_selection = doc.selection(view.id).clone();
        let new_selection = current_selection
            .clone()
            .transform(|range| wildfire_grow_range(doc.syntax(), text, range, count));

        // Only record / apply when something actually changed, so that a
        // no-op `<Enter>` (no larger object) doesn't pollute the shrink stack.
        if new_selection != current_selection {
            view.object_selections.push(current_selection);
            doc.set_selection(view.id, new_selection);
        }
    };
    cx.editor.apply_motion(motion);
}

/// Wildfire: shrink to the previously selected (smaller) text object.
///
/// Pops the per-view `object_selections` stack (shared with `expand_selection`
/// / `shrink_selection`) and restores the previous selection. No-op when the
/// stack is empty.
fn wildfire_shrink(cx: &mut Context) {
    let motion = |editor: &mut Editor| {
        let (view, doc) = current!(editor);
        let current_selection = doc.selection(view.id);
        if let Some(prev_selection) = view.object_selections.pop() {
            if current_selection.contains(&prev_selection) {
                doc.set_selection(view.id, prev_selection);
            } else {
                // The stack no longer matches the current selection; drop it so
                // a later expand starts fresh.
                view.object_selections.clear();
            }
        }
    };
    cx.editor.apply_motion(motion);
}

fn select_sibling_impl<F>(cx: &mut Context, sibling_fn: F)
where
    F: Fn(&zemacs_core::Syntax, RopeSlice, Selection) -> Selection + 'static,
{
    let motion = move |editor: &mut Editor| {
        let (view, doc) = current!(editor);

        if let Some(syntax) = doc.syntax() {
            let text = doc.text().slice(..);
            let current_selection = doc.selection(view.id);
            let selection = sibling_fn(syntax, text, current_selection.clone());
            doc.set_selection(view.id, selection);
        }
    };
    cx.editor.apply_motion(motion);
}

fn select_next_sibling(cx: &mut Context) {
    select_sibling_impl(cx, object::select_next_sibling)
}

fn select_prev_sibling(cx: &mut Context) {
    select_sibling_impl(cx, object::select_prev_sibling)
}

/// Surround each selection with parentheses (paredit-style `wrap`). An empty
/// selection produces `()` with the cursor between the parens.
fn wrap_sexp(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let slice = text.slice(..);
    let selection = doc.selection(view.id);
    let transaction = Transaction::change_by_selection(text, selection, |range| {
        let from = range.from();
        let to = range.to();
        let wrapped = format!("({})", slice.slice(from..to));
        (from, to, Some(wrapped.into()))
    });
    doc.apply(&transaction, view.id);
}

fn move_node_bound_impl(cx: &mut Context, dir: Direction, movement: Movement) {
    let motion = move |editor: &mut Editor| {
        let (view, doc) = current!(editor);

        if let Some(syntax) = doc.syntax() {
            let text = doc.text().slice(..);
            let current_selection = doc.selection(view.id);

            let selection = movement::move_parent_node_end(
                syntax,
                text,
                current_selection.clone(),
                dir,
                movement,
            );

            doc.set_selection(view.id, selection);
        }
    };

    cx.editor.apply_motion(motion);
}

pub fn move_parent_node_end(cx: &mut Context) {
    move_node_bound_impl(cx, Direction::Forward, Movement::Move)
}

pub fn move_parent_node_start(cx: &mut Context) {
    move_node_bound_impl(cx, Direction::Backward, Movement::Move)
}

pub fn extend_parent_node_end(cx: &mut Context) {
    move_node_bound_impl(cx, Direction::Forward, Movement::Extend)
}

pub fn extend_parent_node_start(cx: &mut Context) {
    move_node_bound_impl(cx, Direction::Backward, Movement::Extend)
}

fn select_all_impl<F>(editor: &mut Editor, select_fn: F)
where
    F: Fn(&Syntax, RopeSlice, Selection) -> Selection,
{
    let (view, doc) = current!(editor);

    if let Some(syntax) = doc.syntax() {
        let text = doc.text().slice(..);
        let current_selection = doc.selection(view.id);
        let selection = select_fn(syntax, text, current_selection.clone());
        doc.set_selection(view.id, selection);
    }
}

fn select_all_siblings(cx: &mut Context) {
    let motion = |editor: &mut Editor| {
        select_all_impl(editor, object::select_all_siblings);
    };

    cx.editor.apply_motion(motion);
}

fn select_all_children(cx: &mut Context) {
    let motion = |editor: &mut Editor| {
        select_all_impl(editor, object::select_all_children);
    };

    cx.editor.apply_motion(motion);
}

// vim `%`: with no count, match the bracket under the cursor; with a count,
// `{count}%` jumps to {count} percent through the file, on the first non-blank.
fn match_brackets_or_goto_percent(cx: &mut Context) {
    let Some(count) = cx.count else {
        match_brackets(cx);
        return;
    };
    let is_select = cx.editor.mode == Mode::Select;
    {
        let (view, doc) = current!(cx.editor);
        push_jump(view, doc);
        let text = doc.text().slice(..);
        // Effective line count excludes a phantom trailing empty line so the
        // percentage matches vim's "number-of-lines".
        let lines = if text.line(text.len_lines() - 1).len_chars() == 0 {
            text.len_lines().saturating_sub(1)
        } else {
            text.len_lines()
        };
        // vim formula: ({count} * number-of-lines + 99) / 100
        let line = (count.get().saturating_mul(lines).saturating_add(99) / 100)
            .saturating_sub(1)
            .min(lines.saturating_sub(1));
        let pos = text.line_to_char(line);
        let selection = doc
            .selection(view.id)
            .clone()
            .transform(|range| range.put_cursor(text, pos, is_select));
        doc.set_selection(view.id, selection);
    }
    goto_first_nonwhitespace(cx);
}

fn match_brackets(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);
    let is_select = cx.editor.mode == Mode::Select;
    let text = doc.text();
    let text_slice = text.slice(..);

    let selection = doc.selection(view.id).clone().transform(|range| {
        let pos = range.cursor(text_slice);
        if let Some(matched_pos) = doc.syntax().map_or_else(
            || match_brackets::find_matching_bracket_plaintext(text.slice(..), pos),
            |syntax| match_brackets::find_matching_bracket_fuzzy(syntax, text.slice(..), pos),
        ) {
            range.put_cursor(text_slice, matched_pos, is_select)
        } else {
            range
        }
    });

    doc.set_selection(view.id, selection);
}

//

fn jump_forward(cx: &mut Context) {
    cx.editor.jump_forward(cx.editor.tree.focus, cx.count());
}

fn jump_backward(cx: &mut Context) {
    cx.editor.jump_backward(cx.editor.tree.focus, cx.count());
}

fn save_selection(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);
    push_jump(view, doc);
    cx.editor.set_status("Selection saved to jumplist");
}

fn rotate_view(cx: &mut Context) {
    cx.editor.focus_next()
}

fn rotate_view_reverse(cx: &mut Context) {
    cx.editor.focus_prev()
}

fn jump_view_right(cx: &mut Context) {
    cx.editor.focus_direction(tree::Direction::Right)
}

fn jump_view_left(cx: &mut Context) {
    cx.editor.focus_direction(tree::Direction::Left)
}

fn jump_view_up(cx: &mut Context) {
    cx.editor.focus_direction(tree::Direction::Up)
}

fn jump_view_down(cx: &mut Context) {
    cx.editor.focus_direction(tree::Direction::Down)
}

fn swap_view_right(cx: &mut Context) {
    cx.editor.swap_split_in_direction(tree::Direction::Right)
}

fn swap_view_left(cx: &mut Context) {
    cx.editor.swap_split_in_direction(tree::Direction::Left)
}

fn swap_view_up(cx: &mut Context) {
    cx.editor.swap_split_in_direction(tree::Direction::Up)
}

fn swap_view_down(cx: &mut Context) {
    cx.editor.swap_split_in_direction(tree::Direction::Down)
}

fn transpose_view(cx: &mut Context) {
    cx.editor.transpose_view()
}

/// Open a new split in the given direction specified by the action.
///
/// Maintain the current view (both the cursor's position and view in document).
fn split(editor: &mut Editor, action: Action) {
    let (view, doc) = current!(editor);
    let id = doc.id();
    let selection = doc.selection(view.id).clone();
    let offset = doc.view_offset(view.id);

    editor.switch(id, action);

    // match the selection in the previous view
    let (view, doc) = current!(editor);
    doc.set_selection(view.id, selection);
    // match the view scroll offset (switch doesn't handle this fully
    // since the selection is only matched after the split)
    doc.set_view_offset(view.id, offset);
}

fn hsplit(cx: &mut Context) {
    split(cx.editor, Action::HorizontalSplit);
}

fn hsplit_new(cx: &mut Context) {
    cx.editor.new_file(Action::HorizontalSplit);
}

fn vsplit(cx: &mut Context) {
    split(cx.editor, Action::VerticalSplit);
}

fn vsplit_new(cx: &mut Context) {
    cx.editor.new_file(Action::VerticalSplit);
}

/// SPC b N i : clone the current buffer into a new split. zemacs views share their underlying
/// Document — text and edits propagate between them — so a second view of the same buffer is
/// functionally an Emacs indirect buffer of the same base. Spacemacs `make-indirect-buffer`.
fn clone_indirect_buffer(cx: &mut Context) {
    split(cx.editor, Action::VerticalSplit);
    cx.editor
        .set_status("indirect clone: new view sharing this buffer");
}

fn wclose(cx: &mut Context) {
    if cx.editor.tree.views().count() == 1 {
        if let Err(err) = typed::buffers_remaining_impl(cx.editor) {
            cx.editor.set_error(err.to_string());
            return;
        }
    }
    let view_id = view!(cx.editor).id;
    // close current split
    cx.editor.close(view_id);
}

fn wonly(cx: &mut Context) {
    let views = cx
        .editor
        .tree
        .views()
        .map(|(v, focus)| (v.id, focus))
        .collect::<Vec<_>>();
    for (view_id, focus) in views {
        if !focus {
            cx.editor.close(view_id);
        }
    }
}

fn select_register(cx: &mut Context) {
    cx.editor.autoinfo = Some(Info::from_registers(
        "Select register",
        &cx.editor.registers,
    ));
    cx.on_next_key(move |cx, event| {
        cx.editor.autoinfo = None;
        if let Some(ch) = event.char() {
            cx.editor.selected_register = Some(ch);
        }
    })
}

fn insert_register(cx: &mut Context) {
    // TODO: count is reset to 1 before next key so we move it into the closure here.
    // Would be nice to carry over.
    let count = cx.count();
    cx.editor.autoinfo = Some(Info::from_registers(
        "Insert register",
        &cx.editor.registers,
    ));
    cx.on_next_key(move |cx, event| {
        cx.editor.autoinfo = None;
        if let Some(ch) = event.char() {
            cx.register = Some(ch);
            paste(
                cx.editor,
                cx.register
                    .unwrap_or(cx.editor.config().default_yank_register),
                Paste::Cursor,
                count,
            );
        }
    })
}

fn copy_between_registers(cx: &mut Context) {
    cx.editor.autoinfo = Some(Info::from_registers(
        "Copy from register",
        &cx.editor.registers,
    ));
    cx.on_next_key(move |cx, event| {
        cx.editor.autoinfo = None;

        let Some(source) = event.char() else {
            return;
        };

        let Some(values) = cx.editor.registers.read(source, cx.editor) else {
            cx.editor.set_error(format!("register {source} is empty"));
            return;
        };
        let values: Vec<_> = values.map(|value| value.to_string()).collect();

        cx.editor.autoinfo = Some(Info::from_registers(
            "Copy into register",
            &cx.editor.registers,
        ));
        cx.on_next_key(move |cx, event| {
            cx.editor.autoinfo = None;

            let Some(dest) = event.char() else {
                return;
            };

            let n_values = values.len();
            match cx.editor.registers.write(dest, values) {
                Ok(_) => cx.editor.set_status(format!(
                    "yanked {n_values} value{} from register {source} to {dest}",
                    if n_values == 1 { "" } else { "s" }
                )),
                Err(err) => cx.editor.set_error(err.to_string()),
            }
        });
    });
}

fn align_view_top(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);
    align_view(doc, view, Align::Top);
}

fn align_view_center(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);
    align_view(doc, view, Align::Center);
}

fn align_view_bottom(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);
    align_view(doc, view, Align::Bottom);
}

fn align_view_middle(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);
    let inner_width = view.inner_width(doc);
    let text_fmt = doc.text_format(inner_width, None, Some(view.id));
    // there is no horizontal position when softwrap is enabled
    if text_fmt.soft_wrap {
        return;
    }
    let doc_text = doc.text().slice(..);
    let pos = doc.selection(view.id).primary().cursor(doc_text);
    let pos = visual_offset_from_block(
        doc_text,
        doc.view_offset(view.id).anchor,
        pos,
        &text_fmt,
        &view.text_annotations(doc, None),
    )
    .0;

    let mut offset = doc.view_offset(view.id);
    offset.horizontal_offset = pos
        .col
        .saturating_sub((view.inner_area(doc).width as usize) / 2);
    doc.set_view_offset(view.id, offset);
}

// --- horizontal scroll (vim z h / z l / z H / z L) ---------------------------
fn scroll_column_impl(cx: &mut Context, cols: usize, right: bool) {
    let (view, doc) = current!(cx.editor);
    let mut offset = doc.view_offset(view.id);
    offset.horizontal_offset = if right {
        offset.horizontal_offset.saturating_add(cols)
    } else {
        offset.horizontal_offset.saturating_sub(cols)
    };
    doc.set_view_offset(view.id, offset);
}

fn scroll_column_right(cx: &mut Context) {
    let n = cx.count();
    scroll_column_impl(cx, n, true);
}

fn scroll_column_left(cx: &mut Context) {
    let n = cx.count();
    scroll_column_impl(cx, n, false);
}

fn scroll_half_column_right(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);
    let half = (view.inner_area(doc).width as usize / 2).max(1);
    scroll_column_impl(cx, half, true);
}

fn scroll_half_column_left(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);
    let half = (view.inner_area(doc).width as usize / 2).max(1);
    scroll_column_impl(cx, half, false);
}

// --- window width resize (vim CTRL-W < / CTRL-W > / CTRL-W |) -----------------
fn resize_view_wider(cx: &mut Context) {
    let delta = cx.count() as i16;
    let view = cx.editor.tree.focus;
    cx.editor.tree.resize_horizontal(view, delta);
}

fn resize_view_narrower(cx: &mut Context) {
    let delta = cx.count() as i16;
    let view = cx.editor.tree.focus;
    cx.editor.tree.resize_horizontal(view, -delta);
}

fn resize_view_taller(cx: &mut Context) {
    let delta = cx.count() as i16;
    let view = cx.editor.tree.focus;
    cx.editor.tree.resize_vertical(view, delta);
}

fn resize_view_shorter(cx: &mut Context) {
    let delta = cx.count() as i16;
    let view = cx.editor.tree.focus;
    cx.editor.tree.resize_vertical(view, -delta);
}

fn resize_view_equalize(cx: &mut Context) {
    cx.editor.tree.equalize();
}

/// Resize the focused window to the golden ratio (~62% of the frame), the
/// primary effect of Spacemacs golden-ratio mode (SPC t g / SPC w . g). Uses
/// only the public resize APIs — no core layout surgery.
fn golden_ratio_resize(cx: &mut Context) {
    const PHI: f32 = 0.618;
    cx.editor.tree.equalize();
    let total = cx.editor.tree.area();
    let focus = cx.editor.tree.focus;
    let cur = cx.editor.tree.get(focus).area;
    let dw = (total.width as f32 * PHI) as i16 - cur.width as i16;
    let dh = (total.height as f32 * PHI) as i16 - cur.height as i16;
    if dw > 0 {
        cx.editor.tree.resize_horizontal(focus, dw);
    }
    if dh > 0 {
        cx.editor.tree.resize_vertical(focus, dh);
    }
}

// --- digraphs (vim i_CTRL-K {char1}{char2}) ----------------------------------
// A practical subset of vim's RFC-1345 digraph table: accented Latin letters,
// common punctuation/symbols, currency, fractions, Greek, and arrows. Typed as
// two characters after CTRL-K.
#[rustfmt::skip]
const DIGRAPHS: &[(char, char, char)] = &[
    // grave / acute / circumflex / tilde / diaeresis / ring — lowercase
    ('a','!','à'),('a','\'','á'),('a','>','â'),('a','?','ã'),('a',':','ä'),('a','a','å'),('a','e','æ'),
    ('e','!','è'),('e','\'','é'),('e','>','ê'),('e',':','ë'),
    ('i','!','ì'),('i','\'','í'),('i','>','î'),('i',':','ï'),
    ('o','!','ò'),('o','\'','ó'),('o','>','ô'),('o','?','õ'),('o',':','ö'),('o','/','ø'),
    ('u','!','ù'),('u','\'','ú'),('u','>','û'),('u',':','ü'),
    ('n','?','ñ'),('c',',','ç'),('y','\'','ý'),('y',':','ÿ'),('s','s','ß'),
    // uppercase
    ('A','!','À'),('A','\'','Á'),('A','>','Â'),('A','?','Ã'),('A',':','Ä'),('A','A','Å'),('A','E','Æ'),
    ('E','!','È'),('E','\'','É'),('E','>','Ê'),('E',':','Ë'),
    ('I','!','Ì'),('I','\'','Í'),('I','>','Î'),('I',':','Ï'),
    ('O','!','Ò'),('O','\'','Ó'),('O','>','Ô'),('O','?','Õ'),('O',':','Ö'),('O','/','Ø'),
    ('U','!','Ù'),('U','\'','Ú'),('U','>','Û'),('U',':','Ü'),
    ('N','?','Ñ'),('C',',','Ç'),('Y','\'','Ý'),
    // punctuation / symbols
    ('C','o','©'),('R','g','®'),('T','M','™'),('D','G','°'),('+','-','±'),('M','y','µ'),
    ('S','E','§'),('P','I','¶'),('P','d','£'),('Y','e','¥'),('C','t','¢'),('E','u','€'),
    ('*','X','×'),('-',':','÷'),('1','4','¼'),('1','2','½'),('3','4','¾'),
    ('<','<','«'),('>','>','»'),('?','I','¿'),('!','I','¡'),('o','o','°'),
    ('\'','6','‘'),('\'','9','’'),('"','6','“'),('"','9','”'),('-','N','–'),('-','M','—'),
    ('R','T','√'),('0','0','∞'),('!','=','≠'),('=','<','≤'),('=','>','≥'),
    // arrows
    ('-','>','→'),('<','-','←'),('-','!','↑'),('-','v','↓'),('=','>','⇒'),
    // greek (lower)
    ('a','*','α'),('b','*','β'),('g','*','γ'),('d','*','δ'),('e','*','ε'),('z','*','ζ'),
    ('y','*','η'),('h','*','θ'),('i','*','ι'),('k','*','κ'),('l','*','λ'),('m','*','μ'),
    ('n','*','ν'),('c','*','ξ'),('p','*','π'),('r','*','ρ'),('s','*','σ'),('t','*','τ'),
    ('u','*','υ'),('f','*','φ'),('x','*','χ'),('q','*','ψ'),('w','*','ω'),
    // greek (upper)
    ('D','*','Δ'),('H','*','Θ'),('L','*','Λ'),('C','*','Ξ'),('P','*','Π'),('S','*','Σ'),
    ('F','*','Φ'),('Q','*','Ψ'),('W','*','Ω'),('G','*','Γ'),
];

fn digraph_lookup(a: char, b: char) -> Option<char> {
    DIGRAPHS
        .iter()
        .find(|(x, y, _)| (*x == a && *y == b) || (*x == b && *y == a))
        .map(|(_, _, c)| *c)
}

// vim i_CTRL-E / i_CTRL-Y: insert the character that is directly below / above
// the cursor (same column on the adjacent line).
fn copy_char_from_adjacent_line(cx: &mut Context, below: bool) {
    let ch = {
        let (view, doc) = current!(cx.editor);
        let text = doc.text();
        let pos = doc.selection(view.id).primary().cursor(text.slice(..));
        let line = text.char_to_line(pos);
        let col = pos - text.line_to_char(line);
        let target = if below {
            line + 1
        } else if line == 0 {
            return;
        } else {
            line - 1
        };
        if target >= text.len_lines() {
            return;
        }
        let tstart = text.line_to_char(target);
        let tline = text.line(target);
        let mut len = tline.len_chars();
        if len > 0 && tline.get_char(len - 1) == Some('\n') {
            len -= 1;
        }
        if col >= len {
            return;
        }
        text.char(tstart + col)
    };
    insert::insert_char(cx, ch);
}

// --- spell checking (vim z= / zg / zw / [s / ]s) -----------------------------
/// Alphabetic-run word ranges across the whole buffer.
fn spell_word_ranges(text: RopeSlice) -> Vec<(usize, usize)> {
    let len = text.len_chars();
    let mut ranges = Vec::new();
    let mut i = 0;
    while i < len {
        if text.char(i).is_alphabetic() {
            let start = i;
            while i < len && text.char(i).is_alphabetic() {
                i += 1;
            }
            ranges.push((start, i));
        } else {
            i += 1;
        }
    }
    ranges
}

/// The word range + text under the cursor (ranges are sorted by position).
fn spell_word_at(text: RopeSlice, pos: usize) -> Option<(usize, usize, String)> {
    for (s, e) in spell_word_ranges(text) {
        if pos >= s && pos < e {
            return Some((s, e, text.slice(s..e).chars().collect()));
        }
        if s > pos {
            break;
        }
    }
    None
}

fn goto_spell_error(cx: &mut Context, forward: bool) {
    let (view, doc) = current!(cx.editor);
    let text = doc.text().slice(..);
    let pos = doc.selection(view.id).primary().cursor(text);
    let ranges = spell_word_ranges(text);
    let misspelled: Vec<(usize, usize)> = ranges
        .into_iter()
        .filter(|&(s, e)| {
            crate::spell::is_misspelled(&text.slice(s..e).chars().collect::<String>())
        })
        .collect();
    if misspelled.is_empty() {
        cx.editor.set_status("No misspelled words");
        return;
    }
    let target = if forward {
        misspelled
            .iter()
            .find(|&&(s, _)| s > pos)
            .or_else(|| misspelled.first())
    } else {
        misspelled
            .iter()
            .rev()
            .find(|&&(s, _)| s < pos)
            .or_else(|| misspelled.last())
    };
    if let Some(&(s, _)) = target {
        push_jump(view, doc);
        doc.set_selection(view.id, Selection::point(s));
    }
}

fn goto_next_spell_error(cx: &mut Context) {
    goto_spell_error(cx, true);
}
fn goto_prev_spell_error(cx: &mut Context) {
    goto_spell_error(cx, false);
}

fn spell_word_under_cursor(cx: &mut Context) -> Option<(usize, usize, String)> {
    let (view, doc) = current!(cx.editor);
    let text = doc.text().slice(..);
    let pos = doc.selection(view.id).primary().cursor(text);
    spell_word_at(text, pos)
}

fn spell_add_good(cx: &mut Context) {
    if let Some((_, _, w)) = spell_word_under_cursor(cx) {
        crate::spell::add_good(&w);
        cx.editor
            .set_status(format!("Added '{w}' to spellfile (good)"));
    }
}
fn spell_add_bad(cx: &mut Context) {
    if let Some((_, _, w)) = spell_word_under_cursor(cx) {
        crate::spell::add_bad(&w);
        cx.editor.set_status(format!("Marked '{w}' as misspelled"));
    }
}
fn spell_undo(cx: &mut Context) {
    if let Some((_, _, w)) = spell_word_under_cursor(cx) {
        crate::spell::remove_user(&w);
        cx.editor
            .set_status(format!("Removed '{w}' from spellfile"));
    }
}

/// vim `z=`: show numbered suggestions for the word under the cursor; the next
/// digit key replaces the word with that suggestion.
fn spell_suggest(cx: &mut Context) {
    let Some((start, end, word)) = spell_word_under_cursor(cx) else {
        return;
    };
    let suggestions: Vec<String> = crate::spell::suggest(&word).into_iter().take(9).collect();
    if suggestions.is_empty() {
        cx.editor.set_status(format!("No suggestions for '{word}'"));
        return;
    }
    let rows: Vec<(String, String)> = suggestions
        .iter()
        .enumerate()
        .map(|(i, s)| (format!("{}", i + 1), s.clone()))
        .collect();
    cx.editor.autoinfo = Some(Info::new(format!("Change \"{word}\" to"), &rows));
    cx.on_next_key(move |cx, ev| {
        cx.editor.autoinfo = None;
        let Some(d) = ev.char().and_then(|c| c.to_digit(10)) else {
            return;
        };
        let idx = d as usize;
        if idx == 0 || idx > suggestions.len() {
            return;
        }
        let repl = suggestions[idx - 1].clone();
        let (view, doc) = current!(cx.editor);
        let tx = Transaction::change(
            doc.text(),
            [(start, end, Some(repl.as_str().into()))].into_iter(),
        );
        doc.apply(&tx, view.id);
    });
}

/// Open the unified Preferences window (tabs: Settings, Keymap, Color Scheme, Run Configs).
fn preferences(cx: &mut Context) {
    cx.push_layer(Box::new(crate::ui::preferences::PreferencesPanel::new(0)));
}

/// Open the configuration page on the Help tab (searchable: commands, keybindings, topics).
fn help(cx: &mut Context) {
    cx.push_layer(Box::new(crate::ui::preferences::PreferencesPanel::new(4)));
}

/// Open the embedded-language REPL panel (defaults to elisp; Tab switches).
fn repl(cx: &mut Context) {
    cx.push_layer(Box::new(crate::ui::repl::ReplPanel::new(
        crate::ui::repl::ReplLang::Elisp,
    )));
}

/// Open Preferences on the Run/Debug Configurations tab.
fn run_config_manager(cx: &mut Context) {
    cx.push_layer(Box::new(crate::ui::preferences::PreferencesPanel::new(3)));
}

/// Open Preferences on the Dashboard tab (live system/process stats).
fn dashboard(cx: &mut Context) {
    cx.push_layer(Box::new(crate::ui::preferences::PreferencesPanel::new(5)));
}

/// Open an integrated terminal running the user's `$SHELL` in a PTY.
///
/// Pushes the panel through the job queue (an `EditorCompositor` callback)
/// rather than `cx.push_layer`, because the command palette executes commands
/// with a throwaway context and drops `push_layer` callbacks — so a `push_layer`
/// here would do nothing from `SPC SPC terminal`. Jobs are carried through, so
/// this works both from a keybinding and from the palette.
fn terminal(cx: &mut Context) {
    let call: job::Callback = Callback::EditorCompositor(Box::new(|editor, compositor| {
        match crate::ui::terminal::TerminalPanel::new() {
            Ok(panel) => compositor.push(Box::new(panel)),
            Err(e) => editor.set_error(format!("terminal: {e}")),
        }
    }));
    cx.jobs.callback(async move { Ok(call) });
}

/// Open the project-wide Find in Files panel, seeded with the primary selection.
fn search_in_files(cx: &mut Context) {
    let seed = {
        let (view, doc) = current!(cx.editor);
        let sel = doc.selection(view.id).primary();
        if sel.from() != sel.to() {
            doc.text().slice(sel.from()..sel.to()).to_string()
        } else {
            String::new()
        }
    };
    cx.push_layer(Box::new(crate::ui::search::SearchPanel::new(&seed)));
}

/// Open Preferences on the Settings tab (config.toml editor).
fn settings_page(cx: &mut Context) {
    cx.push_layer(Box::new(crate::ui::preferences::PreferencesPanel::new(0)));
}

/// Toggle "always select opened file" — auto-reveal the current buffer in the
/// project tree whenever you switch buffers (JetBrains autoscroll-from-source).
fn toggle_auto_reveal(cx: &mut Context) {
    cx.callback.push(Box::new(|compositor, cx| {
        if let Some(view) = compositor.find::<crate::ui::EditorView>() {
            view.toggle_auto_reveal(cx);
        }
    }));
}

/// Reveal the current buffer's file in the project tree (JetBrains "Select
/// Opened File"): expands ancestors, focuses the tree, and selects the row.
fn reveal_in_tree(cx: &mut Context) {
    let Some(path) = doc!(cx.editor).path().map(|p| p.to_path_buf()) else {
        cx.editor.set_error("Current buffer has no file to reveal");
        return;
    };
    cx.callback.push(Box::new(move |compositor, _cx| {
        if let Some(view) = compositor.find::<crate::ui::EditorView>() {
            view.reveal_in_tree(&path);
        }
    }));
}

/// Toggle maximizing the bottom panel (read long logs/diffs/errors full-height).
fn toggle_bottom_zoom(cx: &mut Context) {
    cx.callback.push(Box::new(|compositor, cx| {
        if let Some(view) = compositor.find::<crate::ui::EditorView>() {
            view.toggle_bottom_zoom(cx);
        }
    }));
}

fn toggle_drawer_mid(cx: &mut Context) {
    cx.callback.push(Box::new(|compositor, cx| {
        if let Some(view) = compositor.find::<crate::ui::EditorView>() {
            view.toggle_drawer_mid(cx);
        }
    }));
}

/// Shared helper: focus a named IDE workbench panel via the editor view.
fn focus_ide_panel(cx: &mut Context, name: &'static str) {
    cx.callback.push(Box::new(move |compositor, _cx| {
        if let Some(view) = compositor.find::<crate::ui::EditorView>() {
            view.focus_ide_panel(name);
        }
    }));
}

/// Focus the project file tree panel (workbench).
fn focus_file_tree(cx: &mut Context) {
    focus_ide_panel(cx, "project");
}

/// Focus the structure / symbol outline panel (workbench).
fn focus_structure(cx: &mut Context) {
    focus_ide_panel(cx, "structure");
}

/// Focus the problems / diagnostics panel (workbench).
fn focus_problems(cx: &mut Context) {
    focus_ide_panel(cx, "problems");
}

/// Focus the Run console (bottom panel) — then j/k/PgUp/PgDn/g/G scroll output.
fn focus_run_console(cx: &mut Context) {
    focus_ide_panel(cx, "run");
}

/// Focus the Git changes panel — then j/k select a file and Enter opens it.
fn focus_git_panel(cx: &mut Context) {
    focus_ide_panel(cx, "git");
}

fn focus_ci_panel(cx: &mut Context) {
    focus_ide_panel(cx, "ci");
    // Explicit open always refetches (the panel's own auto-fetch only fires once).
    crate::ci::spawn_fetch(cx.jobs);
}

/// Focus the Bookmarks tool window (pinned files; JetBrains Bookmarks, Cmd 2).
fn focus_bookmarks(cx: &mut Context) {
    focus_ide_panel(cx, "bookmarks");
}
/// Focus the Marks tool window (vim a-z marks).
fn focus_marks_panel(cx: &mut Context) {
    focus_ide_panel(cx, "marks");
}
/// Focus the Registers tool window.
fn focus_registers_panel(cx: &mut Context) {
    focus_ide_panel(cx, "registers");
}
/// Focus the Jumplist tool window.
fn focus_jumplist_panel(cx: &mut Context) {
    focus_ide_panel(cx, "jumplist");
}
/// Focus the Recent Files tool window.
fn focus_recent_panel(cx: &mut Context) {
    focus_ide_panel(cx, "recent");
}
/// Focus the TODO/marker tool window.
fn focus_todo_panel(cx: &mut Context) {
    focus_ide_panel(cx, "todo");
}

/// Jump to the next `file:line` reference in the run output (vim `:cnext`).
fn run_next_error(cx: &mut Context) {
    cx.callback.push(Box::new(|compositor, cx| {
        if let Some(view) = compositor.find::<crate::ui::EditorView>() {
            view.goto_run_error(cx, true);
        }
    }));
}

/// Jump to the previous `file:line` reference in the run output (vim `:cprev`).
fn run_prev_error(cx: &mut Context) {
    cx.callback.push(Box::new(|compositor, cx| {
        if let Some(view) = compositor.find::<crate::ui::EditorView>() {
            view.goto_run_error(cx, false);
        }
    }));
}

/// Re-run the last command shown in the Run console (re-run tests after an edit).
fn rerun_last_run(cx: &mut Context) {
    cx.callback.push(Box::new(|compositor, cx| {
        if let Some(view) = compositor.find::<crate::ui::EditorView>() {
            view.rerun_last_run(cx);
        }
    }));
}

/// Clear the Run tool window's output (the process, if any, keeps streaming).
fn clear_run_output(cx: &mut Context) {
    cx.callback.push(Box::new(|compositor, cx| {
        if let Some(view) = compositor.find::<crate::ui::EditorView>() {
            view.clear_run_output(cx);
        }
    }));
}

/// Toggle the IDE workbench (Zen / focus mode): hide every panel for
/// distraction-free editing, then restore them on the next invocation.
fn toggle_ide(cx: &mut Context) {
    cx.callback.push(Box::new(|compositor, _cx| {
        if let Some(view) = compositor.find::<crate::ui::EditorView>() {
            view.toggle_ide();
        }
    }));
}

/// JetBrains "Hide Active Tool Window" (Shift-Esc): return focus to the editor.
fn hide_active_tool_window(cx: &mut Context) {
    cx.callback.push(Box::new(|compositor, _cx| {
        if let Some(view) = compositor.find::<crate::ui::EditorView>() {
            view.hide_active_tool_window();
        }
    }));
}

/// JetBrains "Jump to Last Tool Window" (F12): toggle focus between the editor
/// and the most-recently-focused workbench tool window.
fn jump_to_last_tool_window(cx: &mut Context) {
    cx.callback.push(Box::new(|compositor, _cx| {
        if let Some(view) = compositor.find::<crate::ui::EditorView>() {
            view.jump_to_last_tool_window();
        }
    }));
}

/// Open the side-by-side diff viewer of the buffer vs. its git HEAD version.
/// Static-command mirror of the `:diff` typable command (cf. `toggle_ide` / `:ide`).
fn git_diff(cx: &mut Context) {
    typed::open_diff(cx.editor, cx.jobs);
}

/// Open the 3-pane merge-conflict resolver over the buffer's git conflicts.
/// Static-command mirror of the `:merge` / `:resolve` typable command.
fn resolve_conflicts(cx: &mut Context) {
    typed::open_merge(cx.editor, cx.jobs);
}

/// Open the Magit-style git status porcelain for the focused buffer's repo.
/// Static-command mirror of the `:magit` / `:git` typable command.
fn git_status(cx: &mut Context) {
    typed::open_magit(cx.editor, cx.jobs);
}

/// Toggle a fold over the current org heading's subtree (`:org-cycle`).
fn org_cycle(cx: &mut Context) {
    typed::org_toggle_fold(cx.editor);
}

/// Cycle the current org heading's TODO keyword (`:org-todo`).
fn org_todo(cx: &mut Context) {
    typed::org_cycle_keyword(cx.editor);
}

/// Cycle the current org heading's priority cookie (`:org-priority`).
fn org_priority(cx: &mut Context) {
    typed::org_cycle_priority(cx.editor);
}

/// Promote the current org heading one level (`:org-promote`).
fn org_promote(cx: &mut Context) {
    typed::org_promote_heading(cx.editor);
}

/// Demote the current org heading one level (`:org-demote`).
fn org_demote(cx: &mut Context) {
    typed::org_demote_heading(cx.editor);
}

/// Move the cursor to the next org heading (`:org-next-heading`).
fn org_next_heading(cx: &mut Context) {
    typed::org_goto_next_heading(cx.editor);
}

/// Move the cursor to the previous org heading (`:org-prev-heading`).
fn org_prev_heading(cx: &mut Context) {
    typed::org_goto_prev_heading(cx.editor);
}

/// Fold every org heading subtree in the buffer (`:org-fold-all`).
fn org_fold_all(cx: &mut Context) {
    typed::org_fold_all_headings(cx.editor);
}

/// Open every fold in the buffer (`:org-unfold-all`).
fn org_unfold_all(cx: &mut Context) {
    typed::org_unfold_all_folds(cx.editor);
}

/// Open the org agenda overlay over the working tree (`:org-agenda`).
fn org_agenda(cx: &mut Context) {
    typed::open_org_agenda(cx.editor, cx.jobs);
}

/// Prompt for a note and capture it into the inbox org file (`:org-capture`).
fn org_capture(cx: &mut Context) {
    typed::open_org_capture(cx.editor, cx.jobs, None);
}

/// Run the active named run configuration (or auto-detect when none is set).
fn run_active_config(cx: &mut Context) {
    cx.callback.push(Box::new(|compositor, cx| {
        if let Some(view) = compositor.find::<crate::ui::EditorView>() {
            view.run_active(cx);
        }
    }));
}

/// Count whitespace-delimited words in a rope slice.
fn count_words(slice: RopeSlice) -> usize {
    let mut count = 0;
    let mut in_word = false;
    for ch in slice.chars() {
        if ch.is_whitespace() {
            in_word = false;
        } else if !in_word {
            in_word = true;
            count += 1;
        }
    }
    count
}

/// vim `g CTRL-G`: report document statistics — total lines/words/chars, plus
/// the selected lines/words/chars when a selection is active.
fn document_stats(cx: &mut Context) {
    let (view, doc) = current_ref!(cx.editor);
    let slice = doc.text().slice(..);
    let lines = slice.len_lines();
    let chars = slice.len_chars();
    let words = count_words(slice);

    let sel = doc.selection(view.id);
    let sel_chars: usize = sel.ranges().iter().map(|r| r.len()).sum();
    // In normal mode the cursor is a 1-wide range; only treat it as a selection
    // when it spans more than one char or there are multiple cursors.
    let has_selection = sel_chars > 1 || sel.ranges().len() > 1;
    let msg = if has_selection {
        let mut sel_words = 0;
        let mut sel_lines = 0;
        for r in sel.ranges() {
            if r.is_empty() {
                continue;
            }
            sel_words += count_words(slice.slice(r.from()..r.to()));
            let a = slice.char_to_line(r.from());
            let b = slice.char_to_line(r.to().saturating_sub(1).max(r.from()));
            sel_lines += b - a + 1;
        }
        format!(
            "Selected {sel_lines} of {lines} lines; {sel_words} of {words} words; {sel_chars} of {chars} chars"
        )
    } else {
        format!("{lines} lines; {words} words; {chars} chars")
    };
    cx.editor.set_status(msg);
}

/// GitLens-style blame: show who last changed the line under the cursor in the
/// status line (`git blame -L`), on demand so there's no per-move cost.
fn git_blame_line(cx: &mut Context) {
    let info = {
        let (view, doc) = current_ref!(cx.editor);
        doc.path().map(|p| {
            let text = doc.text();
            let cursor = doc.selection(view.id).primary().cursor(text.slice(..));
            (p.to_path_buf(), text.char_to_line(cursor) + 1)
        })
    };
    let Some((path, line)) = info else {
        cx.editor.set_error("No file to blame");
        return;
    };
    let dir = path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(&dir)
        .args(["blame", "-L", &format!("{line},{line}"), "--"])
        .arg(&path)
        .output();
    match out {
        Ok(o) if o.status.success() => {
            let text = String::from_utf8_lossy(&o.stdout);
            let first = text.lines().next().unwrap_or("").trim();
            if first.is_empty() {
                cx.editor.set_status("blame: no info");
            } else {
                cx.editor
                    .set_status(first.chars().take(180).collect::<String>());
            }
        }
        Ok(o) => {
            let err = String::from_utf8_lossy(&o.stderr);
            let first = err.lines().next().unwrap_or("blame failed").to_owned();
            cx.editor.set_error(format!("git blame: {first}"));
        }
        Err(e) => cx.editor.set_error(format!("git: {e}")),
    }
}

// vim CTRL-G / g CTRL-G: print the current file name and cursor position.
fn file_info(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let pos = doc.selection(view.id).primary().cursor(text.slice(..));
    let line = text.char_to_line(pos) + 1;
    let total = text.len_lines();
    let col = pos - text.line_to_char(line - 1) + 1;
    let name = doc
        .path()
        .and_then(|p| p.file_name())
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "[No Name]".to_string());
    let modified = if doc.is_modified() { " [Modified]" } else { "" };
    let pct = if total > 1 {
        (line - 1) * 100 / (total - 1)
    } else {
        0
    };
    cx.editor.set_status(format!(
        "\"{name}\"{modified} line {line} of {total} col {col} --{pct}%--"
    ));
}

fn copy_char_below(cx: &mut Context) {
    copy_char_from_adjacent_line(cx, true);
}

fn copy_char_above(cx: &mut Context) {
    copy_char_from_adjacent_line(cx, false);
}

fn insert_digraph(cx: &mut Context) {
    cx.editor.autoinfo.replace(Info::new(
        "Digraph",
        &[("{c1}{c2}", "two-character mnemonic")],
    ));
    cx.on_next_key(move |cx, ev1| {
        cx.editor.autoinfo = None;
        let Some(c1) = ev1.char() else { return };
        cx.on_next_key(move |cx, ev2| {
            let Some(c2) = ev2.char() else { return };
            match digraph_lookup(c1, c2) {
                Some(ch) => insert::insert_char(cx, ch),
                None => cx
                    .editor
                    .set_error(format!("E1393: digraph '{c1}{c2}' not found")),
            }
        });
    });
}

// --- insert generators (spacemacs SPC i) -------------------------------------
// Self-contained text generators wired under the `SPC i` leader: UUIDs
// (`SPC i U`), lorem-ipsum (`SPC i l`) and passwords (`SPC i p`). Each command
// builds a string from a pure helper (unit-tested below) and drops it at every
// cursor. Randomness comes from `fastrand`, which self-seeds from system entropy.

/// Insert `text` at every selection's cursor (collapsing nothing), then leave
/// select mode — the shared tail of every `SPC i` generator.
fn insert_generated(cx: &mut Context, text: &str) {
    let (view, doc) = current!(cx.editor);
    let sel = doc.selection(view.id);
    let t = Tendril::from(text);
    let transaction = Transaction::change_by_selection(doc.text(), sel, |range| {
        let pos = range.cursor(doc.text().slice(..));
        (pos, pos, Some(t.clone()))
    });
    doc.apply(&transaction, view.id);
    exit_select_mode(cx);
}

/// 16 random bytes formatted `8-4-4-4-12`, with the given version nibble and the
/// RFC 4122 variant bits already applied.
fn uuid_format(mut bytes: [u8; 16], version: u8) -> String {
    bytes[6] = (bytes[6] & 0x0f) | (version << 4);
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let h: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    )
}

fn uuid_v4_string() -> String {
    let mut bytes = [0u8; 16];
    bytes.iter_mut().for_each(|b| *b = fastrand::u8(..));
    uuid_format(bytes, 4)
}

/// Time-based UUID (version 1): a 60-bit timestamp of 100ns intervals since the
/// Gregorian epoch (1582-10-15), a random clock sequence, and a random node with
/// the multicast bit set (so it can't collide with a real MAC address).
fn uuid_v1_string() -> String {
    let nanos = time::OffsetDateTime::now_utc()
        .unix_timestamp_nanos()
        .max(0) as u128;
    // 100ns ticks since 1582-10-15, the Gregorian/UUID epoch.
    let ts = (nanos / 100) as u64 + 0x01B2_1DD2_1381_4000;
    let time_low = (ts & 0xffff_ffff) as u32;
    let time_mid = ((ts >> 32) & 0xffff) as u16;
    let time_hi = (((ts >> 48) & 0x0fff) as u16) | 0x1000; // version 1
    let clock_seq = (fastrand::u16(..) & 0x3fff) | 0x8000; // variant
    let mut node = [0u8; 6];
    node.iter_mut().for_each(|b| *b = fastrand::u8(..));
    node[0] |= 0x01; // multicast bit -> not a real MAC
    let node_hex: String = node.iter().map(|b| format!("{b:02x}")).collect();
    format!("{time_low:08x}-{time_mid:04x}-{time_hi:04x}-{clock_seq:04x}-{node_hex}")
}

fn insert_uuid_v4(cx: &mut Context) {
    let s = uuid_v4_string();
    insert_generated(cx, &s);
}

fn insert_uuid_v1(cx: &mut Context) {
    let s = uuid_v1_string();
    insert_generated(cx, &s);
}

/// Pool of classic lorem-ipsum words, lowercase and punctuation-free.
const LOREM_WORDS: &[&str] = &[
    "lorem",
    "ipsum",
    "dolor",
    "sit",
    "amet",
    "consectetur",
    "adipiscing",
    "elit",
    "sed",
    "do",
    "eiusmod",
    "tempor",
    "incididunt",
    "ut",
    "labore",
    "et",
    "dolore",
    "magna",
    "aliqua",
    "enim",
    "ad",
    "minim",
    "veniam",
    "quis",
    "nostrud",
    "exercitation",
    "ullamco",
    "laboris",
    "nisi",
    "aliquip",
    "ex",
    "ea",
    "commodo",
    "consequat",
    "duis",
    "aute",
    "irure",
    "in",
    "reprehenderit",
    "voluptate",
    "velit",
    "esse",
    "cillum",
    "fugiat",
    "nulla",
    "pariatur",
    "excepteur",
    "sint",
    "occaecat",
    "cupidatat",
    "non",
    "proident",
    "sunt",
    "culpa",
    "qui",
    "officia",
    "deserunt",
    "mollit",
    "anim",
    "id",
    "est",
    "laborum",
];

/// One sentence: `len` words, first capitalised, terminated by a period.
fn lorem_sentence(len: usize) -> String {
    let len = len.max(1);
    let mut out = String::new();
    for i in 0..len {
        if i > 0 {
            out.push(' ');
        }
        let w = LOREM_WORDS[fastrand::usize(..LOREM_WORDS.len())];
        if i == 0 {
            let mut c = w.chars();
            if let Some(f) = c.next() {
                out.extend(f.to_uppercase());
                out.push_str(c.as_str());
            }
        } else {
            out.push_str(w);
        }
    }
    out.push('.');
    out
}

/// A paragraph of `sentences` lorem sentences of varied length.
fn lorem_paragraph(sentences: usize) -> String {
    (0..sentences.max(1))
        .map(|_| lorem_sentence(6 + fastrand::usize(..8)))
        .collect::<Vec<_>>()
        .join(" ")
}

/// `items` dash-bulleted lines, one short lorem fragment each.
fn lorem_list(items: usize) -> String {
    (0..items.max(1))
        .map(|_| format!("- {}", lorem_sentence(3 + fastrand::usize(..4))))
        .collect::<Vec<_>>()
        .join("\n")
}

fn insert_lorem_sentence(cx: &mut Context) {
    let s = lorem_sentence(8 + fastrand::usize(..6));
    insert_generated(cx, &s);
}

fn insert_lorem_paragraph(cx: &mut Context) {
    let s = lorem_paragraph(4 + fastrand::usize(..3));
    insert_generated(cx, &s);
}

fn insert_lorem_list(cx: &mut Context) {
    let s = lorem_list(4 + fastrand::usize(..4));
    insert_generated(cx, &s);
}

const PW_ALNUM: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
const PW_SYMBOLS: &[u8] = b"!@#$%^&*()-_=+[]{};:,.<>?";

/// `len` characters drawn uniformly from `charset`.
fn password(len: usize, charset: &[u8]) -> String {
    (0..len)
        .map(|_| charset[fastrand::usize(..charset.len())] as char)
        .collect()
}

/// A pronounceable password: alternating consonants and vowels, `len` chars.
fn password_phonetic(len: usize) -> String {
    const CONS: &[u8] = b"bcdfghjklmnpqrstvwxz";
    const VOWELS: &[u8] = b"aeiouy";
    (0..len)
        .map(|i| {
            let set = if i % 2 == 0 { CONS } else { VOWELS };
            set[fastrand::usize(..set.len())] as char
        })
        .collect()
}

fn insert_password_simple(cx: &mut Context) {
    let s = password(12, PW_ALNUM);
    insert_generated(cx, &s);
}

fn insert_password_strong(cx: &mut Context) {
    let mut set = PW_ALNUM.to_vec();
    set.extend_from_slice(PW_SYMBOLS);
    let s = password(20, &set);
    insert_generated(cx, &s);
}

fn insert_password_paranoid(cx: &mut Context) {
    let mut set = PW_ALNUM.to_vec();
    set.extend_from_slice(PW_SYMBOLS);
    let s = password(32, &set);
    insert_generated(cx, &s);
}

fn insert_password_numerical(cx: &mut Context) {
    let s = password(8, b"0123456789");
    insert_generated(cx, &s);
}

fn insert_password_phonetic(cx: &mut Context) {
    let s = password_phonetic(14);
    insert_generated(cx, &s);
}

// --- symbol-case styles + region shuffles (spacemacs SPC x) ------------------
// Pure string transforms applied to each selection range, reusing the
// `switch_case_impl` plumbing. `SPC x i {C,U,_}` re-style an identifier;
// `SPC x l r` / `SPC x w r` shuffle the lines / words of the selection.

/// Split an identifier into its component words, recognising camelCase humps,
/// snake_case, kebab-case and whitespace as boundaries.
fn split_identifier_words(s: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut cur = String::new();
    let mut prev_lower_or_digit = false;
    for ch in s.chars() {
        if ch == '_' || ch == '-' || ch.is_whitespace() {
            if !cur.is_empty() {
                words.push(std::mem::take(&mut cur));
            }
            prev_lower_or_digit = false;
            continue;
        }
        if ch.is_uppercase() && prev_lower_or_digit && !cur.is_empty() {
            words.push(std::mem::take(&mut cur));
        }
        cur.push(ch);
        prev_lower_or_digit = ch.is_lowercase() || ch.is_numeric();
    }
    if !cur.is_empty() {
        words.push(cur);
    }
    words
}

fn capitalize(w: &str) -> String {
    let mut c = w.chars();
    match c.next() {
        Some(f) => {
            let mut out: String = f.to_uppercase().collect();
            out.push_str(&c.as_str().to_lowercase());
            out
        }
        None => String::new(),
    }
}

fn to_upper_camel(s: &str) -> String {
    split_identifier_words(s)
        .iter()
        .map(|w| capitalize(w))
        .collect()
}

fn to_up_case(s: &str) -> String {
    split_identifier_words(s)
        .iter()
        .map(|w| w.to_uppercase())
        .collect::<Vec<_>>()
        .join("_")
}

fn to_under_score(s: &str) -> String {
    split_identifier_words(s)
        .iter()
        .map(|w| w.to_lowercase())
        .collect::<Vec<_>>()
        .join("_")
}

fn change_symbol_case(cx: &mut Context, f: fn(&str) -> String) {
    switch_case_impl(cx, move |slice| {
        let s: Cow<str> = slice.into();
        Tendril::from(f(&s))
    });
}

fn symbol_upper_camel(cx: &mut Context) {
    change_symbol_case(cx, to_upper_camel);
}

fn symbol_up_case(cx: &mut Context) {
    change_symbol_case(cx, to_up_case);
}

fn symbol_under_score(cx: &mut Context) {
    change_symbol_case(cx, to_under_score);
}

/// In-place Fisher–Yates shuffle using `fastrand`.
fn shuffle_in_place<T>(v: &mut [T]) {
    for i in (1..v.len()).rev() {
        v.swap(i, fastrand::usize(..=i));
    }
}

/// Shuffle the lines of `s`; a single trailing newline is preserved.
fn randomize_lines(s: &str) -> String {
    let had_trailing = s.ends_with('\n');
    let mut lines: Vec<&str> = s.strip_suffix('\n').unwrap_or(s).split('\n').collect();
    shuffle_in_place(&mut lines);
    let mut out = lines.join("\n");
    if had_trailing {
        out.push('\n');
    }
    out
}

/// Shuffle the whitespace-separated words of `s`, joined by single spaces.
fn randomize_words(s: &str) -> String {
    let mut words: Vec<&str> = s.split_whitespace().collect();
    shuffle_in_place(&mut words);
    words.join(" ")
}

fn randomize_lines_in_region(cx: &mut Context) {
    switch_case_impl(cx, |slice| {
        let s: Cow<str> = slice.into();
        Tendril::from(randomize_lines(&s))
    });
}

fn randomize_words_in_region(cx: &mut Context) {
    switch_case_impl(cx, |slice| {
        let s: Cow<str> = slice.into();
        Tendril::from(randomize_words(&s))
    });
}

// --- code folding (vim z* family) --------------------------------------------
// Folds live on the document (`Document::folds`). A closed fold hides its inner
// lines from rendering and line-wise motion — the ranges flow into the
// `DocumentFormatter` via `Document::text_format`. See `zemacs_core::fold`.
// After a fold change we snap the cursor out of any freshly hidden region.

/// Document line the primary cursor sits on.
fn fold_cursor_line(view: &View, doc: &Document) -> usize {
    let text = doc.text().slice(..);
    let cursor = doc.selection(view.id).primary().cursor(text);
    text.char_to_line(cursor)
}

/// Move the primary cursor to the first char of `line` (clamped).
fn fold_goto_line(view: &View, doc: &mut Document, line: usize) {
    let last = doc.text().len_lines().saturating_sub(1);
    let line = line.min(last);
    let pos = doc.text().line_to_char(line);
    let sel = Selection::point(pos);
    doc.set_selection(view.id, sel);
}

/// If the cursor ended up on a hidden line, pull it to the fold's visible header.
fn fold_snap_cursor(view: &mut View, doc: &mut Document) {
    let line = fold_cursor_line(view, doc);
    let anchor = doc.folds().visible_anchor(line);
    if anchor != line {
        fold_goto_line(view, doc, anchor);
    }
}

fn fold_create(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);
    let text = doc.text().slice(..);
    let range = doc.selection(view.id).primary();
    let start = text.char_to_line(range.from());
    // last line touched by the selection (exclusive `to` maps back one char)
    let end = text.char_to_line(range.to().saturating_sub(1).max(range.from()));
    let last = doc.text().len_lines().saturating_sub(1);
    doc.folds_mut().create(start, end);
    doc.folds_mut().clamp(last);
    fold_goto_line(view, doc, start);
    cx.editor
        .set_status(format!("created fold {}-{}", start + 1, end + 1));
}

fn fold_toggle(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);
    let line = fold_cursor_line(view, doc);
    doc.folds_mut().toggle(line);
    fold_snap_cursor(view, doc);
}

fn fold_open(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);
    let line = fold_cursor_line(view, doc);
    doc.folds_mut().open(line);
}

fn line_has_marker(doc: &Document, line: usize, pat: &str) -> bool {
    doc.text()
        .line(line)
        .chars()
        .collect::<String>()
        .contains(pat)
}

/// vim marker folding: the `{{{`…`}}}` region enclosing `cursor_line`, if any.
/// Simple (non-nested) scan — the nearest `{{{` at/above and the next `}}}` below.
fn marker_fold_region(doc: &Document, cursor_line: usize) -> Option<(usize, usize)> {
    let total = doc.text().len_lines();
    if total == 0 {
        return None;
    }
    let from = cursor_line.min(total - 1);
    let mut start = None;
    for l in (0..=from).rev() {
        if line_has_marker(doc, l, "{{{") {
            start = Some(l);
            break;
        }
        // a `}}}` above the cursor with no `{{{` first means we're not inside a region
        if l != from && line_has_marker(doc, l, "}}}") {
            break;
        }
    }
    let start = start?;
    let mut end = None;
    for l in start..total {
        if line_has_marker(doc, l, "}}}") {
            end = Some(l);
            break;
        }
    }
    let end = end?;
    (end > start).then_some((start, end))
}

fn fold_close(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);
    let line = fold_cursor_line(view, doc);
    // vim marker folding: if the cursor sits in a `{{{ }}}` region, create the fold first.
    if let Some((start, end)) = marker_fold_region(doc, line) {
        let last = doc.text().len_lines().saturating_sub(1);
        doc.folds_mut().create(start, end);
        doc.folds_mut().clamp(last);
    }
    doc.folds_mut().close(line);
    fold_snap_cursor(view, doc);
}

fn fold_open_all(cx: &mut Context) {
    let (_view, doc) = current!(cx.editor);
    doc.folds_mut().open_all();
}

/// SPC n w : widen — remove any narrowing (document-wide and this view's) and reveal the whole
/// buffer (Emacs `widen`).
fn widen(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);
    let was_narrowed = doc.is_narrowed() || doc.view_narrow(view.id).is_some();
    let view_id = view.id;
    doc.widen();
    doc.clear_view_narrow(view_id);
    doc.folds_mut().open_all();
    if was_narrowed {
        cx.editor.set_status("widened");
    }
}

/// SPC n F : narrow to the enclosing function in an *indirect* view — clone the buffer into a
/// split and narrow only that new view, leaving the original full (Emacs indirect-buffer narrow).
fn narrow_to_function_indirect(cx: &mut Context) {
    let (lo, hi) = {
        let (view, doc) = current!(cx.editor);
        let text = doc.text();
        let last = text.len_lines().saturating_sub(1);
        let cur = text
            .char_to_line(doc.selection(view.id).primary().head.min(text.len_chars()))
            .min(last);
        let is_blank = |l: usize| text.line(l).to_string().trim().is_empty();
        let mut start = cur;
        while start > 0 && !is_blank(start - 1) {
            start -= 1;
        }
        let mut end = cur;
        while end < last && !is_blank(end + 1) {
            end += 1;
        }
        (
            text.line_to_char(start),
            text.line_to_char((end + 1).min(text.len_lines())),
        )
    };
    split(cx.editor, Action::VerticalSplit);
    let (view, doc) = current!(cx.editor);
    doc.set_view_narrow(view.id, lo, hi);
    doc.set_selection(view.id, Selection::point(lo));
    cx.editor
        .set_status("narrowed to function in an indirect view (SPC n w widens)");
}

/// SPC n P : narrow to the current page (form-feed delimited) in an *indirect* view.
fn narrow_to_page_indirect(cx: &mut Context) {
    let (lo, hi) = {
        let (view, doc) = current!(cx.editor);
        let text = doc.text();
        let last = text.len_lines().saturating_sub(1);
        let cur = text
            .char_to_line(doc.selection(view.id).primary().head.min(text.len_chars()))
            .min(last);
        let is_ff = |l: usize| text.line(l).to_string().contains('\u{0c}');
        let mut start = cur;
        while start > 0 && !is_ff(start - 1) {
            start -= 1;
        }
        let mut end = cur;
        while end < last && !is_ff(end + 1) {
            end += 1;
        }
        (
            text.line_to_char(start),
            text.line_to_char((end + 1).min(text.len_lines())),
        )
    };
    split(cx.editor, Action::VerticalSplit);
    let (view, doc) = current!(cx.editor);
    doc.set_view_narrow(view.id, lo, hi);
    doc.set_selection(view.id, Selection::point(lo));
    cx.editor
        .set_status("narrowed to page in an indirect view (SPC n w widens)");
}

fn fold_close_all(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);
    doc.folds_mut().close_all();
    fold_snap_cursor(view, doc);
}

fn fold_delete(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);
    let line = fold_cursor_line(view, doc);
    doc.folds_mut().delete(line);
}

fn fold_delete_all(cx: &mut Context) {
    let (_view, doc) = current!(cx.editor);
    doc.folds_mut().clear();
}

fn fold_next(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);
    let line = fold_cursor_line(view, doc);
    let next = doc.folds().next_fold_start(line);
    if let Some(line) = next {
        fold_goto_line(view, doc, line);
    }
}

fn fold_prev(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);
    let line = fold_cursor_line(view, doc);
    if let Some(prev) = doc.folds().prev_fold_end(line) {
        let target = doc.folds().visible_anchor(prev);
        fold_goto_line(view, doc, target);
    }
}

// --- narrowing (spacemacs SPC n) --------------------------------------------
// Approximate Emacs narrowing using the fold engine: fold every line OUTSIDE the
// region so only the region is visible. This is *visual* narrowing — editing is
// not actually restricted to the region — so these bindings are recorded as
// `partial` in the port mapping. `widen` (SPC n w) reuses `fold_open_all`.

/// Line ranges to fold so that only `[start, end]` stays visible in a buffer of
/// `last_line` (0-based, inclusive). Returns the before- and after-region spans
/// that are non-empty.
fn narrow_outside_ranges(start: usize, end: usize, last_line: usize) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    if start > 0 {
        out.push((0, start - 1));
    }
    if end < last_line {
        out.push((end + 1, last_line));
    }
    out
}

/// Fold everything outside the lines spanned by the primary selection.
fn narrow_to_region(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);
    let text = doc.text().slice(..);
    let range = doc.selection(view.id).primary();
    let start = text.char_to_line(range.from());
    let end = text.char_to_line(range.to().saturating_sub(1).max(range.from()));
    let last = doc.text().len_lines().saturating_sub(1);
    // Line-aligned char bounds of the region, computed before the fold mutations below
    // (which need `&mut doc` and would otherwise conflict with the `text` borrow).
    let lo = text.line_to_char(start);
    let hi = text.line_to_char((end + 1).min(text.len_lines()));
    for (s, e) in narrow_outside_ranges(start, end, last) {
        doc.folds_mut().create(s, e);
    }
    doc.folds_mut().clamp(last);
    // True narrowing: restrict the accessible buffer to the region, so goto-start/end,
    // last-line, and select-all confine to it — not just the visual fold.
    doc.narrow_to(lo, hi);
    fold_goto_line(view, doc, start);
    cx.editor
        .set_status(format!("narrowed to lines {}-{}", start + 1, end + 1));
}

// --- keyboard-macro counter (spacemacs SPC K c) ------------------------------
// A process-global integer counter, mirroring Emacs `kmacro-*-counter`. Recording
// a macro that runs `kmacro_insert_counter` and replaying it (@reg) inserts an
// incrementing number each time — which is exactly what the counter is for.

static KMACRO_COUNTER: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);

fn kmacro_counter_value() -> i64 {
    KMACRO_COUNTER.load(std::sync::atomic::Ordering::Relaxed)
}

/// Add `n` to the counter and return the new value.
fn kmacro_counter_add(n: i64) -> i64 {
    KMACRO_COUNTER.fetch_add(n, std::sync::atomic::Ordering::Relaxed) + n
}

#[cfg(test)]
fn kmacro_counter_reset() {
    KMACRO_COUNTER.store(0, std::sync::atomic::Ordering::Relaxed);
}

// --- keyboard-macro ring (spacemacs SPC K r / SPC K e) -----------------------
// A process-global recency list of recorded macro strings (most recent first),
// mirroring Emacs `kmacro-ring`. `record_macro` pushes each new recording here.
static MACRO_RING: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

fn macro_ring_push(s: String) {
    if s.is_empty() {
        return;
    }
    if let Ok(mut ring) = MACRO_RING.lock() {
        ring.retain(|m| m != &s);
        ring.insert(0, s);
        ring.truncate(30);
    }
}

fn macro_ring_preview(s: &str) -> String {
    if s.chars().count() > 40 {
        format!("{}…", s.chars().take(40).collect::<String>())
    } else {
        s.to_string()
    }
}

/// Copy the ring head into register `@` so `@@` / replay uses it.
fn kmacro_ring_sync(cx: &mut Context) {
    let head = MACRO_RING.lock().ok().and_then(|r| r.first().cloned());
    match head {
        Some(s) => {
            let _ = cx.editor.registers.write('@', vec![s.clone()]);
            cx.editor
                .set_status(format!("macro ring head → @ : {}", macro_ring_preview(&s)));
        }
        None => cx.editor.set_status("macro ring is empty"),
    }
}

/// SPC K r n: cycle to the next macro in the ring (head moves to the back).
fn kmacro_ring_next(cx: &mut Context) {
    if let Ok(mut r) = MACRO_RING.lock() {
        if r.len() > 1 {
            let f = r.remove(0);
            r.push(f);
        }
    }
    kmacro_ring_sync(cx);
}

/// SPC K r p / r N: cycle to the previous macro in the ring.
fn kmacro_ring_prev(cx: &mut Context) {
    if let Ok(mut r) = MACRO_RING.lock() {
        if r.len() > 1 {
            if let Some(b) = r.pop() {
                r.insert(0, b);
            }
        }
    }
    kmacro_ring_sync(cx);
}

/// SPC K r d: delete the head macro from the ring.
fn kmacro_ring_delete(cx: &mut Context) {
    let left = if let Ok(mut r) = MACRO_RING.lock() {
        if !r.is_empty() {
            r.remove(0);
        }
        r.len()
    } else {
        0
    };
    cx.editor
        .set_status(format!("deleted head macro ({left} in ring)"));
}

/// SPC K r s: swap the first two macros in the ring.
fn kmacro_ring_swap(cx: &mut Context) {
    if let Ok(mut r) = MACRO_RING.lock() {
        if r.len() >= 2 {
            r.swap(0, 1);
        }
    }
    kmacro_ring_sync(cx);
}

/// SPC K r L: show the head macro in the ring.
fn kmacro_ring_view(cx: &mut Context) {
    let head = MACRO_RING.lock().ok().and_then(|r| r.first().cloned());
    match head {
        Some(s) => cx.editor.set_status(format!("head macro: {s}")),
        None => cx.editor.set_status("macro ring is empty"),
    }
}

/// SPC K e r / e n: write the head macro to a register typed next.
fn kmacro_to_register(cx: &mut Context) {
    let head = MACRO_RING.lock().ok().and_then(|r| r.first().cloned());
    let Some(s) = head else {
        cx.editor.set_status("no macro recorded yet");
        return;
    };
    cx.editor.set_status("save macro to register: press a key");
    cx.on_next_key(move |cx, event| {
        if let KeyEvent {
            code: KeyCode::Char(ch),
            ..
        } = event
        {
            let _ = cx.editor.registers.write(ch, vec![s.clone()]);
            cx.editor
                .set_status(format!("macro → register [{ch}]"));
        }
    });
}

/// SPC K c a: add `count` (default 1) to the macro counter.
fn kmacro_add_counter(cx: &mut Context) {
    let v = kmacro_counter_add(cx.count() as i64);
    cx.editor.set_status(format!("macro counter = {v}"));
}

/// SPC K c c: insert the current counter value at the cursor, then increment it.
fn kmacro_insert_counter(cx: &mut Context) {
    let v = kmacro_counter_value();
    insert_generated(cx, &v.to_string());
    kmacro_counter_add(1);
}

// --- paredit-style structural editing (spacemacs SPC k) ----------------------
// Pure s-expression transforms over a char buffer + cursor, applied with a
// minimal prefix/suffix diff so undo/marks stay tight. An "s-expression" is a
// balanced ()/[]/{} group or an atom (a run of non-space, non-bracket chars).

fn pe_is_open(c: char) -> bool {
    matches!(c, '(' | '[' | '{')
}
fn pe_is_close(c: char) -> bool {
    matches!(c, ')' | ']' | '}')
}

/// Innermost bracket pair `(open_idx, close_idx)` enclosing `pos`.
fn pe_enclosing(ch: &[char], pos: usize) -> Option<(usize, usize)> {
    let mut stack: Vec<usize> = Vec::new();
    let mut best: Option<(usize, usize)> = None;
    for (i, &c) in ch.iter().enumerate() {
        if pe_is_open(c) {
            stack.push(i);
        } else if pe_is_close(c) {
            if let Some(o) = stack.pop() {
                if o <= pos && pos <= i && best.is_none_or(|(bo, _)| o > bo) {
                    best = Some((o, i));
                }
            }
        }
    }
    best
}

/// First s-expression at/after `i` (skipping leading whitespace): `(start, end_inclusive)`.
fn pe_sexp_forward(ch: &[char], i: usize) -> Option<(usize, usize)> {
    let mut j = i;
    while j < ch.len() && ch[j].is_whitespace() {
        j += 1;
    }
    if j >= ch.len() || pe_is_close(ch[j]) {
        return None;
    }
    if pe_is_open(ch[j]) {
        let mut depth = 0i32;
        for (k, &c) in ch.iter().enumerate().skip(j) {
            if pe_is_open(c) {
                depth += 1;
            } else if pe_is_close(c) {
                depth -= 1;
                if depth == 0 {
                    return Some((j, k));
                }
            }
        }
        None
    } else {
        let start = j;
        while j < ch.len() && !ch[j].is_whitespace() && !pe_is_open(ch[j]) && !pe_is_close(ch[j]) {
            j += 1;
        }
        Some((start, j - 1))
    }
}

/// Last s-expression ending before `i` (scanning back over whitespace): `(start, end_inclusive)`.
fn pe_sexp_backward(ch: &[char], i: usize) -> Option<(usize, usize)> {
    let mut k = i as isize - 1;
    while k >= 0 && ch[k as usize].is_whitespace() {
        k -= 1;
    }
    if k < 0 {
        return None;
    }
    let k = k as usize;
    if pe_is_open(ch[k]) {
        return None;
    }
    if pe_is_close(ch[k]) {
        let mut depth = 0i32;
        let mut m = k as isize;
        while m >= 0 {
            let c = ch[m as usize];
            if pe_is_close(c) {
                depth += 1;
            } else if pe_is_open(c) {
                depth -= 1;
                if depth == 0 {
                    return Some((m as usize, k));
                }
            }
            m -= 1;
        }
        None
    } else {
        let end = k;
        let mut m = k as isize;
        while m >= 0
            && !ch[m as usize].is_whitespace()
            && !pe_is_open(ch[m as usize])
            && !pe_is_close(ch[m as usize])
        {
            m -= 1;
        }
        Some(((m + 1) as usize, end))
    }
}

fn pe_vec(prefix: &[char], mid: impl IntoIterator<Item = char>, suffix: &[char]) -> Vec<char> {
    let mut v = Vec::with_capacity(prefix.len() + suffix.len() + 8);
    v.extend_from_slice(prefix);
    v.extend(mid);
    v.extend_from_slice(suffix);
    v
}

/// Slurp forward: move the closing bracket past the next outside s-expression.
/// `a (b) c` → `a (b c)`.
fn pe_slurp_forward(ch: &[char], pos: usize) -> Option<(Vec<char>, usize)> {
    let (_o, c) = pe_enclosing(ch, pos)?;
    let (_ns, ne) = pe_sexp_forward(ch, c + 1)?;
    let closer = ch[c];
    let mut out = pe_vec(&ch[..c], ch[c + 1..=ne].iter().copied(), &ch[ne + 1..]);
    out.insert(ne, closer); // ne shifted left by one after removing the closer at c
    Some((out, pos))
}

/// Barf forward: move the closing bracket in before the last inside s-expression.
/// `(a b c)` → `(a b) c`.
fn pe_barf_forward(ch: &[char], pos: usize) -> Option<(Vec<char>, usize)> {
    let (o, c) = pe_enclosing(ch, pos)?;
    let (ls, _le) = pe_sexp_backward(ch, c)?;
    if ls <= o + 1 {
        return None; // only one sexp inside; nothing to barf
    }
    // place the closer right after the previous content (skip the gap before `ls`)
    let mut ins = ls;
    while ins > o + 1 && ch[ins - 1].is_whitespace() {
        ins -= 1;
    }
    let closer = ch[c];
    let mut out = pe_vec(&ch[..ins], std::iter::once(closer), &ch[ins..c]);
    out.extend_from_slice(&ch[c + 1..]);
    Some((out, pos.min(ins)))
}

/// Slurp backward: move the opening bracket before the previous outside s-expression.
/// `a (b) c` → `(a b) c`.
fn pe_slurp_backward(ch: &[char], pos: usize) -> Option<(Vec<char>, usize)> {
    let (o, _c) = pe_enclosing(ch, pos)?;
    let (ps, _pe) = pe_sexp_backward(ch, o)?;
    let opener = ch[o];
    let mut out = pe_vec(&ch[..ps], std::iter::once(opener), &ch[ps..o]);
    out.extend_from_slice(&ch[o + 1..]);
    Some((out, pos + 1))
}

/// Barf backward: move the opening bracket in after the first inside s-expression.
/// `(a b c)` → `a (b c)`.
fn pe_barf_backward(ch: &[char], pos: usize) -> Option<(Vec<char>, usize)> {
    let (o, c) = pe_enclosing(ch, pos)?;
    let (_fs, fe) = pe_sexp_forward(ch, o + 1)?;
    if fe >= c {
        return None;
    }
    // place the opener right before the next content (skip the gap after `fe`)
    let mut ins = fe + 1;
    while ins < c && ch[ins].is_whitespace() {
        ins += 1;
    }
    let opener = ch[o];
    let mut out = pe_vec(&ch[..o], ch[o + 1..ins].iter().copied(), &[]);
    out.push(opener);
    out.extend_from_slice(&ch[ins..]);
    Some((out, pos))
}

/// Unwrap / splice: remove the enclosing brackets, keeping their contents.
/// `(a b)` → `a b`.
fn pe_splice(ch: &[char], pos: usize) -> Option<(Vec<char>, usize)> {
    let (o, c) = pe_enclosing(ch, pos)?;
    let out = pe_vec(&ch[..o], ch[o + 1..c].iter().copied(), &ch[c + 1..]);
    let cursor = if pos > o { pos - 1 } else { pos };
    Some((out, cursor))
}

/// Splice killing forward: splice the enclosing list, discarding everything from
/// point to its close. `(as (bs| cs) ds)` → `(as bs ds)`.
fn pe_splice_kill_forward(ch: &[char], pos: usize) -> Option<(Vec<char>, usize)> {
    let (o, c) = pe_enclosing(ch, pos)?;
    let out = pe_vec(&ch[..o], ch[o + 1..pos].iter().copied(), &ch[c + 1..]);
    Some((out, pos.saturating_sub(1)))
}

/// Splice killing backward: splice the enclosing list, discarding everything from
/// its open to point. `(as (bs |cs) ds)` → `(as cs ds)`.
fn pe_splice_kill_backward(ch: &[char], pos: usize) -> Option<(Vec<char>, usize)> {
    let (o, c) = pe_enclosing(ch, pos)?;
    let out = pe_vec(&ch[..o], ch[pos..c].iter().copied(), &ch[c + 1..]);
    Some((out, o))
}

/// Insert a new empty `()` sibling after the enclosing s-expression; cursor lands
/// between the new parens. `(a|)` → `(a) ()` with the cursor in the new pair.
fn pe_insert_sexp_after(ch: &[char], pos: usize) -> Option<(Vec<char>, usize)> {
    let (_o, c) = pe_enclosing(ch, pos)?;
    let mut out = ch[..=c].to_vec();
    out.extend([' ', '(', ')']);
    out.extend_from_slice(&ch[c + 1..]);
    Some((out, c + 3)) // between '(' (c+2) and ')' (c+3)
}

/// Insert a new empty `()` sibling before the enclosing s-expression; cursor lands
/// between the new parens. `(a|)` → `() (a)` with the cursor in the new pair.
fn pe_insert_sexp_before(ch: &[char], pos: usize) -> Option<(Vec<char>, usize)> {
    let (o, _c) = pe_enclosing(ch, pos)?;
    let mut out = ch[..o].to_vec();
    out.extend(['(', ')', ' ']);
    out.extend_from_slice(&ch[o..]);
    Some((out, o + 1)) // between '(' (o) and ')' (o+1)
}

/// Raise: replace the enclosing s-expression with the child at point.
/// `(a (b c) d)` with point in `(b c)` → `(b c)`.
fn pe_raise(ch: &[char], pos: usize) -> Option<(Vec<char>, usize)> {
    let (o, c) = pe_enclosing(ch, pos)?;
    let (cs, ce) = pe_sexp_forward(ch, pos)?;
    if cs < o || ce > c {
        return None;
    }
    let out = pe_vec(&ch[..o], ch[cs..=ce].iter().copied(), &ch[c + 1..]);
    Some((out, o))
}

/// paredit-convolute-sexp: swap the enclosing form's prefix with the inner
/// form's prefix-up-to-point. `(as (bs <pt> ..)) → (bs (as <pt> ..))`. Pure.
fn pe_convolute(ch: &[char], pos: usize) -> Option<(Vec<char>, usize)> {
    let (io, ic) = pe_enclosing(ch, pos)?; // inner form
    if io == 0 {
        return None;
    }
    let (oo, oc) = pe_enclosing(ch, io - 1)?; // form enclosing the inner one
    if oo >= io || oc <= ic || pos <= io || pos > ic {
        return None;
    }
    let outer_prefix: Vec<char> = ch[oo + 1..io].to_vec(); // `as `
    let inner_prefix: Vec<char> = ch[io + 1..pos].to_vec(); // `bs `
    let mut out = Vec::new();
    out.extend_from_slice(&ch[..=oo]); // up to & incl outer-open
    out.extend(inner_prefix.iter().copied()); // bs
    out.push(ch[io]); // inner-open
    out.extend(outer_prefix.iter().copied()); // as
    out.extend_from_slice(&ch[pos..]); // point .. inner-close, outer tail, outer-close
    Some((out, oo + 1))
}

/// Transpose: swap the s-expression before point with the one after it.
/// `(a b)` with point between → `(b a)`.
fn pe_transpose(ch: &[char], pos: usize) -> Option<(Vec<char>, usize)> {
    let (ps, pe_) = pe_sexp_backward(ch, pos)?;
    let (ns, ne) = pe_sexp_forward(ch, pos)?;
    if pe_ >= ns {
        return None;
    }
    let mut out = pe_vec(&ch[..ps], ch[ns..=ne].iter().copied(), &ch[pe_ + 1..ns]);
    out.extend_from_slice(&ch[ps..=pe_]);
    out.extend_from_slice(&ch[ne + 1..]);
    Some((out, ne + 1))
}

/// paredit-split-sexp: split the enclosing list at point into two siblings.
/// `(a b| c)` → `(a b) ( c)`.
fn pe_split(ch: &[char], pos: usize) -> Option<(Vec<char>, usize)> {
    let (o, c) = pe_enclosing(ch, pos)?;
    if pos <= o || pos > c {
        return None;
    }
    let open = ch[o];
    let close = ch[c];
    let mut out = ch[..pos].to_vec();
    out.push(close);
    out.push(' ');
    out.push(open);
    out.extend_from_slice(&ch[pos..]);
    Some((out, pos + 3))
}

/// paredit-absorb-sexp: pull the previous sibling sexp into the enclosing form,
/// after its first element. `(a (bs <pt> ..)) → ((bs a <pt> ..))`. Pure.
fn pe_absorb(ch: &[char], pos: usize) -> Option<(Vec<char>, usize)> {
    let (eo, ec) = pe_enclosing(ch, pos)?; // enclosing form E = (bs ..)
    let (ps, pe) = pe_sexp_backward(ch, eo)?; // previous sexp P before E
    let (_fs, fe) = pe_sexp_forward(ch, eo + 1)?; // first element inside E
    if pe >= eo || fe >= ec {
        return None;
    }
    let p_text: Vec<char> = ch[ps..=pe].to_vec();
    let mut out = Vec::new();
    out.extend_from_slice(&ch[..ps]); // up to (and dropping) P
    out.extend_from_slice(&ch[pe + 1..=fe]); // ws + E-open + first element
    out.push(' ');
    out.extend(p_text.iter().copied()); // P moved after the first element
    out.extend_from_slice(&ch[fe + 1..]); // remainder of E + tail
    Some((out, eo))
}

/// A pure paredit transform: given the buffer chars and cursor offset, returns
/// the new chars and cursor offset, or `None` if the edit doesn't apply.
type PareditFn = fn(&[char], usize) -> Option<(Vec<char>, usize)>;

/// Run a pure paredit transform on the primary cursor, applying a minimal diff.
fn apply_paredit(cx: &mut Context, f: PareditFn) {
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let slice = text.slice(..);
    let chars: Vec<char> = slice.chars().collect();
    let pos = doc.selection(view.id).primary().cursor(slice);
    let Some((newc, newcursor)) = f(&chars, pos) else {
        cx.editor.set_status("paredit: no target s-expression here");
        return;
    };
    // common prefix / suffix in char space → one minimal change
    let mut p = 0;
    while p < chars.len() && p < newc.len() && chars[p] == newc[p] {
        p += 1;
    }
    let mut q = 0;
    while q < chars.len() - p
        && q < newc.len() - p
        && chars[chars.len() - 1 - q] == newc[newc.len() - 1 - q]
    {
        q += 1;
    }
    let repl: String = newc[p..newc.len() - q].iter().collect();
    let transaction = Transaction::change(
        text,
        std::iter::once((p, chars.len() - q, Some(Tendril::from(repl)))),
    );
    doc.apply(&transaction, view.id);
    let (view, doc) = current!(cx.editor);
    let nc = newcursor.min(doc.text().len_chars());
    doc.set_selection(view.id, Selection::point(nc));
}

fn paredit_slurp_forward(cx: &mut Context) {
    apply_paredit(cx, pe_slurp_forward);
}
fn paredit_barf_forward(cx: &mut Context) {
    apply_paredit(cx, pe_barf_forward);
}
fn paredit_slurp_backward(cx: &mut Context) {
    apply_paredit(cx, pe_slurp_backward);
}
fn paredit_barf_backward(cx: &mut Context) {
    apply_paredit(cx, pe_barf_backward);
}
fn paredit_splice(cx: &mut Context) {
    apply_paredit(cx, pe_splice);
}
fn paredit_raise(cx: &mut Context) {
    apply_paredit(cx, pe_raise);
}
fn paredit_transpose(cx: &mut Context) {
    apply_paredit(cx, pe_transpose);
}
fn paredit_split(cx: &mut Context) {
    apply_paredit(cx, pe_split);
}
fn paredit_absorb(cx: &mut Context) {
    apply_paredit(cx, pe_absorb);
}
fn paredit_convolute(cx: &mut Context) {
    apply_paredit(cx, pe_convolute);
}
fn paredit_splice_kill_forward(cx: &mut Context) {
    apply_paredit(cx, pe_splice_kill_forward);
}
fn paredit_splice_kill_backward(cx: &mut Context) {
    apply_paredit(cx, pe_splice_kill_backward);
}
fn paredit_insert_sexp_after(cx: &mut Context) {
    apply_paredit(cx, pe_insert_sexp_after);
    enter_insert_mode(cx);
}
fn paredit_insert_sexp_before(cx: &mut Context) {
    apply_paredit(cx, pe_insert_sexp_before);
    enter_insert_mode(cx);
}

/// SPC b w: toggle the current buffer's read-only (writable) state.
fn toggle_readonly(cx: &mut Context) {
    let (_view, doc) = current!(cx.editor);
    doc.readonly = !doc.readonly;
    let state = if doc.readonly {
        "read-only"
    } else {
        "writable"
    };
    cx.editor.set_status(format!("buffer is now {state}"));
}

fn scroll_up(cx: &mut Context) {
    scroll(cx, cx.count(), Direction::Backward, false);
}

fn scroll_down(cx: &mut Context) {
    scroll(cx, cx.count(), Direction::Forward, false);
}

fn goto_ts_object_impl(cx: &mut Context, object: &'static str, direction: Direction) {
    let count = cx.count();
    let motion = move |editor: &mut Editor| {
        let (view, doc) = current!(editor);
        let loader = editor.syn_loader.load();
        if let Some(syntax) = doc.syntax() {
            let text = doc.text().slice(..);

            let selection = doc.selection(view.id).clone().transform(|range| {
                let new_range = movement::goto_treesitter_object(
                    text, range, object, direction, syntax, &loader, count,
                );

                if editor.mode == Mode::Select {
                    let head = if new_range.head < range.anchor {
                        new_range.anchor
                    } else {
                        new_range.head
                    };

                    Range::new(range.anchor, head)
                } else {
                    new_range.with_direction(direction)
                }
            });

            push_jump(view, doc);
            doc.set_selection(view.id, selection);
        } else {
            editor.set_status("Syntax-tree is not available in current buffer");
        }
    };
    cx.editor.apply_motion(motion);
}

fn goto_next_function(cx: &mut Context) {
    goto_ts_object_impl(cx, "function", Direction::Forward)
}

fn goto_prev_function(cx: &mut Context) {
    goto_ts_object_impl(cx, "function", Direction::Backward)
}

fn goto_next_class(cx: &mut Context) {
    goto_ts_object_impl(cx, "class", Direction::Forward)
}

fn goto_prev_class(cx: &mut Context) {
    goto_ts_object_impl(cx, "class", Direction::Backward)
}

fn goto_next_parameter(cx: &mut Context) {
    goto_ts_object_impl(cx, "parameter", Direction::Forward)
}

fn goto_prev_parameter(cx: &mut Context) {
    goto_ts_object_impl(cx, "parameter", Direction::Backward)
}

fn goto_next_comment(cx: &mut Context) {
    goto_ts_object_impl(cx, "comment", Direction::Forward)
}

fn goto_prev_comment(cx: &mut Context) {
    goto_ts_object_impl(cx, "comment", Direction::Backward)
}

fn goto_next_test(cx: &mut Context) {
    goto_ts_object_impl(cx, "test", Direction::Forward)
}

fn goto_prev_test(cx: &mut Context) {
    goto_ts_object_impl(cx, "test", Direction::Backward)
}

/// Runs of ≥2 consecutive `true` flags as inclusive (start, end) ranges. Pure.
fn comment_fold_runs(flags: &[bool]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let n = flags.len();
    let mut l = 0;
    while l < n {
        if flags[l] {
            let s = l;
            while l < n && flags[l] {
                l += 1;
            }
            if l - 1 > s {
                out.push((s, l - 1));
            }
        } else {
            l += 1;
        }
    }
    out
}

/// SPC c h: hide/show comments by folding multi-line comment blocks (lines whose
/// trimmed text starts with the language's line-comment token). Toggles: if any
/// comment fold is open, re-fold; the widen is on SPC n w / zR.
fn fold_comments(cx: &mut Context) {
    let loader = cx.editor.syn_loader.load();
    let (view, doc) = current!(cx.editor);
    let tokens: Vec<String> = doc
        .language_config_at(&loader, 0)
        .and_then(|c| c.comment_tokens.as_deref())
        .map(|t| t.iter().cloned().collect())
        .unwrap_or_default();
    if tokens.is_empty() {
        cx.editor.set_status("no comment syntax for this buffer");
        return;
    }
    let text = doc.text();
    let n = text.len_lines();
    let flags: Vec<bool> = (0..n)
        .map(|l| {
            let s = text.line(l).to_string();
            let t = s.trim_start();
            tokens.iter().any(|tok| t.starts_with(tok.as_str()))
        })
        .collect();
    let runs = comment_fold_runs(&flags);
    if runs.is_empty() {
        cx.editor.set_status("no multi-line comment blocks to fold");
        return;
    }
    let count = runs.len();
    for (s, e) in runs {
        doc.folds_mut().create(s, e);
    }
    doc.folds_mut().clamp(n.saturating_sub(1));
    let _ = view;
    cx.editor
        .set_status(format!("folded {count} comment block(s)"));
}

/// Candidate counterpart file names for impl<->test toggling. If `name` looks
/// like a test file, returns the implementation name; otherwise returns common
/// test-file names. Pure (tested).
fn test_counterpart_names(name: &str) -> Vec<String> {
    let (stem, dotext) = match name.rfind('.') {
        Some(i) => (&name[..i], &name[i..]),
        None => (name, ""),
    };
    let mut out = Vec::new();
    if let Some(s) = stem.strip_suffix("_test").or_else(|| stem.strip_suffix("_spec")) {
        out.push(format!("{s}{dotext}"));
    }
    if let Some(s) = stem.strip_prefix("test_").or_else(|| stem.strip_prefix("spec_")) {
        out.push(format!("{s}{dotext}"));
    }
    if let Some(s) = stem.strip_suffix(".test").or_else(|| stem.strip_suffix(".spec")) {
        out.push(format!("{s}{dotext}"));
    }
    if !out.is_empty() {
        return out;
    }
    out.push(format!("{stem}_test{dotext}"));
    out.push(format!("{stem}_spec{dotext}"));
    out.push(format!("test_{stem}{dotext}"));
    if !dotext.is_empty() {
        let ext = &dotext[1..];
        out.push(format!("{stem}.test.{ext}"));
        out.push(format!("{stem}.spec.{ext}"));
    }
    out
}

/// SPC p a: toggle between an implementation file and its test counterpart,
/// trying common naming conventions in the same directory and a `tests/` sibling.
fn toggle_test_file(cx: &mut Context) {
    let Some(path) = doc!(cx.editor).path().map(|p| p.to_path_buf()) else {
        cx.editor.set_error("buffer has no file path");
        return;
    };
    let Some(name) = path.file_name().and_then(|n| n.to_str()).map(String::from) else {
        return;
    };
    let dir = path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let cands = test_counterpart_names(&name);
    // search dirs: same dir, a `tests/` subdir, and a sibling `tests/` dir
    let mut dirs = vec![dir.clone(), dir.join("tests")];
    if let Some(parent) = dir.parent() {
        dirs.push(parent.join("tests"));
        dirs.push(parent.join("src"));
    }
    for d in &dirs {
        for cand in &cands {
            let p = d.join(cand);
            if p.exists() && p != path {
                match cx.editor.open(&p, Action::Replace) {
                    Ok(_) => cx.editor.set_status(format!("→ {}", p.display())),
                    Err(e) => cx.editor.set_error(format!("{e}")),
                }
                return;
            }
        }
    }
    cx.editor
        .set_status("no implementation/test counterpart found");
}

fn goto_next_xml_element(cx: &mut Context) {
    goto_ts_object_impl(cx, "xml-element", Direction::Forward)
}

fn goto_prev_xml_element(cx: &mut Context) {
    goto_ts_object_impl(cx, "xml-element", Direction::Backward)
}

fn goto_next_entry(cx: &mut Context) {
    goto_ts_object_impl(cx, "entry", Direction::Forward)
}

fn goto_prev_entry(cx: &mut Context) {
    goto_ts_object_impl(cx, "entry", Direction::Backward)
}

fn select_textobject_around(cx: &mut Context) {
    select_textobject(cx, textobject::TextObject::Around);
}

fn select_textobject_inner(cx: &mut Context) {
    select_textobject(cx, textobject::TextObject::Inside);
}

// vim operator + text object: select the inner/around object (capturing the
// object char interactively, like `mi`/`ma`) then apply the operator. These
// make `ciw`, `diw`, `yiw`, `ci(`, `da"`, `dip`, … work.
fn change_textobject_inner(cx: &mut Context) {
    select_textobject_then(cx, textobject::TextObject::Inside, Some(change_selection));
}
fn change_textobject_around(cx: &mut Context) {
    select_textobject_then(cx, textobject::TextObject::Around, Some(change_selection));
}
fn delete_textobject_inner(cx: &mut Context) {
    select_textobject_then(cx, textobject::TextObject::Inside, Some(delete_selection));
}
fn delete_textobject_around(cx: &mut Context) {
    select_textobject_then(cx, textobject::TextObject::Around, Some(delete_selection));
}
fn yank_textobject_inner(cx: &mut Context) {
    select_textobject_then(cx, textobject::TextObject::Inside, Some(yank_textobject));
}
fn yank_textobject_around(cx: &mut Context) {
    select_textobject_then(cx, textobject::TextObject::Around, Some(yank_textobject));
}

/// Yank the current selection then collapse it (vim `yi`/`ya` leave the cursor
/// at the object start rather than keeping it selected).
fn yank_textobject(cx: &mut Context) {
    yank(cx);
    collapse_selection(cx);
}

fn select_textobject(cx: &mut Context, objtype: textobject::TextObject) {
    select_textobject_then(cx, objtype, None);
}

fn select_textobject_then(
    cx: &mut Context,
    objtype: textobject::TextObject,
    after: Option<fn(&mut Context)>,
) {
    let count = cx.count();

    cx.on_next_key(move |cx, event| {
        cx.editor.autoinfo = None;
        if let Some(ch) = event.char() {
            let textobject = move |editor: &mut Editor| {
                let (view, doc) = current!(editor);
                let loader = editor.syn_loader.load();
                let text = doc.text().slice(..);

                let textobject_treesitter = |obj_name: &str, range: Range| -> Range {
                    let Some(syntax) = doc.syntax() else {
                        return range;
                    };
                    textobject::textobject_treesitter(
                        text, range, objtype, obj_name, syntax, &loader, count,
                    )
                };

                if ch == 'g' && doc.diff_handle().is_none() {
                    editor.set_status("Diff is not available in current buffer");
                    return;
                }

                let textobject_change = |range: Range| -> Range {
                    let diff_handle = doc.diff_handle().unwrap();
                    let diff = diff_handle.load();
                    let line = range.cursor_line(text);
                    let hunk_idx = if let Some(hunk_idx) = diff.hunk_at(line as u32, false) {
                        hunk_idx
                    } else {
                        return range;
                    };
                    let hunk = diff.nth_hunk(hunk_idx).after;

                    let start = text.line_to_char(hunk.start as usize);
                    let end = text.line_to_char(hunk.end as usize);
                    Range::new(start, end).with_direction(range.direction())
                };

                let selection = doc.selection(view.id).clone().transform(|range| {
                    match ch {
                        'w' => textobject::textobject_word(text, range, objtype, count, false),
                        'W' => textobject::textobject_word(text, range, objtype, count, true),
                        // vim block aliases: `ib`/`ab` == `i(`/`a(`, `iB`/`aB` == `i{`/`a{`.
                        'b' => textobject::textobject_pair_surround(
                            doc.syntax(),
                            text,
                            range,
                            objtype,
                            '(',
                            count,
                        ),
                        'B' => textobject::textobject_pair_surround(
                            doc.syntax(),
                            text,
                            range,
                            objtype,
                            '{',
                            count,
                        ),
                        // vim `it`/`at`: change/select inside/around the enclosing
                        // (X)HTML/XML tag. `C` keeps the type/class object.
                        't' => textobject_treesitter("xml-element", range),
                        'C' => textobject_treesitter("class", range),
                        'f' => textobject_treesitter("function", range),
                        'a' => textobject_treesitter("parameter", range),
                        'c' => textobject_treesitter("comment", range),
                        'T' => textobject_treesitter("test", range),
                        'e' => textobject_treesitter("entry", range),
                        'x' => textobject_treesitter("xml-element", range),
                        'p' => textobject::textobject_paragraph(text, range, objtype, count),
                        's' => textobject::textobject_sentence(text, range, objtype, count),
                        'm' => textobject::textobject_pair_surround_closest(
                            doc.syntax(),
                            text,
                            range,
                            objtype,
                            count,
                        ),
                        'g' => textobject_change(range),
                        // TODO: cancel new ranges if inconsistent surround matches across lines
                        ch if !ch.is_ascii_alphanumeric() => textobject::textobject_pair_surround(
                            doc.syntax(),
                            text,
                            range,
                            objtype,
                            ch,
                            count,
                        ),
                        _ => range,
                    }
                });
                doc.set_selection(view.id, selection);
            };
            cx.editor.apply_motion(textobject);
            // Apply the pending operator (vim `c`/`d`/`y` + text object).
            if let Some(after) = after {
                after(cx);
            }
        }
    });

    let title = match objtype {
        textobject::TextObject::Inside => "Match inside",
        textobject::TextObject::Around => "Match around",
        _ => return,
    };
    let help_text = [
        ("w", "Word"),
        ("W", "WORD"),
        ("b", "Block — parentheses (alias for ( )"),
        ("B", "Block — braces (alias for { )"),
        ("p", "Paragraph"),
        ("t", "Tag / (X)HTML element (tree-sitter)"),
        ("C", "Type/class definition (tree-sitter)"),
        ("f", "Function (tree-sitter)"),
        ("a", "Argument/parameter (tree-sitter)"),
        ("c", "Comment (tree-sitter)"),
        ("T", "Test (tree-sitter)"),
        ("e", "Data structure entry (tree-sitter)"),
        ("m", "Closest surrounding pair (tree-sitter)"),
        ("g", "Change"),
        ("x", "(X)HTML element (tree-sitter)"),
        (" ", "... or any character acting as a pair"),
    ];

    cx.editor.autoinfo = Some(Info::new(title, &help_text));
}

static SURROUND_HELP_TEXT: [(&str, &str); 6] = [
    ("m", "Nearest matching pair"),
    ("( or )", "Parentheses"),
    ("{ or }", "Curly braces"),
    ("< or >", "Angled brackets"),
    ("[ or ]", "Square brackets"),
    (" ", "... or any character"),
];

fn surround_add(cx: &mut Context) {
    cx.on_next_key(move |cx, event| {
        cx.editor.autoinfo = None;
        let (view, doc) = current!(cx.editor);
        // surround_len is the number of new characters being added.
        let (open, close, surround_len) = match event.char() {
            Some(ch) => {
                let (o, c) = match_brackets::get_pair(ch);
                let mut open = Tendril::new();
                open.push(o);
                let mut close = Tendril::new();
                close.push(c);
                (open, close, 2)
            }
            None if event.code == KeyCode::Enter => (
                doc.line_ending.as_str().into(),
                doc.line_ending.as_str().into(),
                2 * doc.line_ending.len_chars(),
            ),
            None => return,
        };

        let selection = doc.selection(view.id);
        let mut changes = Vec::with_capacity(selection.len() * 2);
        let mut ranges = SmallVec::with_capacity(selection.len());
        let mut offs = 0;

        for range in selection.iter() {
            changes.push((range.from(), range.from(), Some(open.clone())));
            changes.push((range.to(), range.to(), Some(close.clone())));

            ranges.push(
                Range::new(offs + range.from(), offs + range.to() + surround_len)
                    .with_direction(range.direction()),
            );

            offs += surround_len;
        }

        let transaction = Transaction::change(doc.text(), changes.into_iter())
            .with_selection(Selection::new(ranges, selection.primary_index()));
        doc.apply(&transaction, view.id);
        exit_select_mode(cx);
    });

    cx.editor.autoinfo = Some(Info::new(
        "Surround selections with",
        &SURROUND_HELP_TEXT[1..],
    ));
}

fn surround_replace(cx: &mut Context) {
    let count = cx.count();
    cx.on_next_key(move |cx, event| {
        cx.editor.autoinfo = None;
        let surround_ch = match event.char() {
            Some('m') => None, // m selects the closest surround pair
            Some(ch) => Some(ch),
            None => return,
        };
        let (view, doc) = current!(cx.editor);
        let text = doc.text().slice(..);
        let selection = doc.selection(view.id);

        let change_pos =
            match surround::get_surround_pos(doc.syntax(), text, selection, surround_ch, count) {
                Ok(c) => c,
                Err(err) => {
                    cx.editor.set_error(err.to_string());
                    return;
                }
            };

        let selection = selection.clone();
        let ranges: SmallVec<[Range; 1]> = change_pos.iter().map(|&p| Range::point(p)).collect();
        doc.set_selection(
            view.id,
            Selection::new(ranges, selection.primary_index() * 2),
        );

        cx.on_next_key(move |cx, event| {
            cx.editor.autoinfo = None;
            let (view, doc) = current!(cx.editor);
            let to = match event.char() {
                Some(to) => to,
                None => return doc.set_selection(view.id, selection),
            };
            let (open, close) = match_brackets::get_pair(to);

            // the changeset has to be sorted to allow nested surrounds
            let mut sorted_pos: Vec<(usize, char)> = Vec::new();
            for p in change_pos.chunks(2) {
                sorted_pos.push((p[0], open));
                sorted_pos.push((p[1], close));
            }
            sorted_pos.sort_unstable();

            let transaction = Transaction::change(
                doc.text(),
                sorted_pos.iter().map(|&pos| {
                    let mut t = Tendril::new();
                    t.push(pos.1);
                    (pos.0, pos.0 + 1, Some(t))
                }),
            );
            doc.set_selection(view.id, selection);
            doc.apply(&transaction, view.id);
            exit_select_mode(cx);
        });

        cx.editor.autoinfo = Some(Info::new(
            "Replace with a pair of",
            &SURROUND_HELP_TEXT[1..],
        ));
    });

    cx.editor.autoinfo = Some(Info::new(
        "Replace surrounding pair of",
        &SURROUND_HELP_TEXT,
    ));
}

fn surround_delete(cx: &mut Context) {
    let count = cx.count();
    cx.on_next_key(move |cx, event| {
        cx.editor.autoinfo = None;
        let surround_ch = match event.char() {
            Some('m') => None, // m selects the closest surround pair
            Some(ch) => Some(ch),
            None => return,
        };
        let (view, doc) = current!(cx.editor);
        let text = doc.text().slice(..);
        let selection = doc.selection(view.id);

        let mut change_pos =
            match surround::get_surround_pos(doc.syntax(), text, selection, surround_ch, count) {
                Ok(c) => c,
                Err(err) => {
                    cx.editor.set_error(err.to_string());
                    return;
                }
            };
        change_pos.sort_unstable(); // the changeset has to be sorted to allow nested surrounds
        let transaction =
            Transaction::change(doc.text(), change_pos.into_iter().map(|p| (p, p + 1, None)));
        doc.apply(&transaction, view.id);
        exit_select_mode(cx);
    });

    cx.editor.autoinfo = Some(Info::new("Delete surrounding pair of", &SURROUND_HELP_TEXT));
}

#[derive(Eq, PartialEq)]
enum ShellBehavior {
    Replace,
    Ignore,
    Insert,
    Append,
}

fn shell_pipe(cx: &mut Context) {
    shell_prompt_for_behavior(cx, "pipe:".into(), ShellBehavior::Replace);
}

fn shell_pipe_to(cx: &mut Context) {
    shell_prompt_for_behavior(cx, "pipe-to:".into(), ShellBehavior::Ignore);
}

fn shell_insert_output(cx: &mut Context) {
    shell_prompt_for_behavior(cx, "insert-output:".into(), ShellBehavior::Insert);
}

fn shell_append_output(cx: &mut Context) {
    shell_prompt_for_behavior(cx, "append-output:".into(), ShellBehavior::Append);
}

fn shell_keep_pipe(cx: &mut Context) {
    shell_prompt(cx, "keep-pipe:".into(), |cx, args| {
        let shell = &cx.editor.config().shell;
        let (view, doc) = current!(cx.editor);
        let selection = doc.selection(view.id);

        let mut ranges = SmallVec::with_capacity(selection.len());
        let old_index = selection.primary_index();
        let mut index: Option<usize> = None;
        let text = doc.text().slice(..);

        for (i, range) in selection.ranges().iter().enumerate() {
            let fragment = range.slice(text);
            if let Err(err) = shell_impl(shell, args.join(" ").as_str(), Some(fragment.into())) {
                log::debug!("Shell command failed: {}", err);
            } else {
                ranges.push(*range);
                if i >= old_index && index.is_none() {
                    index = Some(ranges.len() - 1);
                }
            }
        }

        if ranges.is_empty() {
            cx.editor.set_error("No selections remaining");
            return;
        }

        let index = index.unwrap_or_else(|| ranges.len() - 1);
        doc.set_selection(view.id, Selection::new(ranges, index));
    });
}

fn shell_impl(shell: &[String], cmd: &str, input: Option<Rope>) -> anyhow::Result<Tendril> {
    tokio::task::block_in_place(|| zemacs_lsp::block_on(shell_impl_async(shell, cmd, input)))
}

async fn shell_impl_async(
    shell: &[String],
    cmd: &str,
    input: Option<Rope>,
) -> anyhow::Result<Tendril> {
    use std::process::Stdio;
    use tokio::process::Command;
    ensure!(!shell.is_empty(), "No shell set");

    let mut process = Command::new(&shell[0]);
    process
        .args(&shell[1..])
        .arg(cmd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    if input.is_some() || cfg!(windows) {
        process.stdin(Stdio::piped());
    } else {
        process.stdin(Stdio::null());
    }

    let mut process = match process.spawn() {
        Ok(process) => process,
        Err(e) => {
            log::error!("Failed to start shell: {}", e);
            return Err(e.into());
        }
    };
    let output = if let Some(mut stdin) = process.stdin.take() {
        let input_task = tokio::spawn(async move {
            if let Some(input) = input {
                zemacs_view::document::to_writer(&mut stdin, (encoding::UTF_8, false), &input)
                    .await?;
            }
            anyhow::Ok(())
        });
        let (output, _) = tokio::join! {
            process.wait_with_output(),
            input_task,
        };
        output?
    } else {
        // Process has no stdin, so we just take the output
        process.wait_with_output().await?
    };

    let output = if !output.status.success() {
        if output.stderr.is_empty() {
            match output.status.code() {
                Some(exit_code) => bail!("Shell command failed: status {}", exit_code),
                None => bail!("Shell command failed"),
            }
        }
        String::from_utf8_lossy(&output.stderr)
        // Prioritize `stderr` output over `stdout`
    } else if !output.stderr.is_empty() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        log::debug!("Command printed to stderr: {stderr}");
        stderr
    } else {
        String::from_utf8_lossy(&output.stdout)
    };

    Ok(Tendril::from(output))
}

fn shell(cx: &mut compositor::Context, cmd: &str, behavior: &ShellBehavior) {
    let pipe = match behavior {
        ShellBehavior::Replace | ShellBehavior::Ignore => true,
        ShellBehavior::Insert | ShellBehavior::Append => false,
    };

    let config = cx.editor.config();
    let shell = &config.shell;
    let (view, doc) = current!(cx.editor);
    let selection = doc.selection(view.id);

    let mut changes = Vec::with_capacity(selection.len());
    let mut ranges = SmallVec::with_capacity(selection.len());
    let text = doc.text().slice(..);

    let mut shell_output: Option<Tendril> = None;
    let mut offset = 0isize;
    for range in selection.ranges() {
        let output = if let Some(output) = shell_output.as_ref() {
            output.clone()
        } else {
            let input = range.slice(text);
            match shell_impl(shell, cmd, pipe.then(|| input.into())) {
                Ok(mut output) => {
                    if !input.ends_with("\n") && output.ends_with('\n') {
                        output.pop();
                        if output.ends_with('\r') {
                            output.pop();
                        }
                    }

                    if !pipe {
                        shell_output = Some(output.clone());
                    }
                    output
                }
                Err(err) => {
                    cx.editor.set_error(err.to_string());
                    return;
                }
            }
        };

        let output_len = output.chars().count();

        let (from, to, deleted_len) = match behavior {
            ShellBehavior::Replace => (range.from(), range.to(), range.len()),
            ShellBehavior::Insert => (range.from(), range.from(), 0),
            ShellBehavior::Append => (range.to(), range.to(), 0),
            _ => (range.from(), range.from(), 0),
        };

        // These `usize`s cannot underflow because selection ranges cannot overlap.
        let anchor = to
            .checked_add_signed(offset)
            .expect("Selection ranges cannot overlap")
            .checked_sub(deleted_len)
            .expect("Selection ranges cannot overlap");
        let new_range = Range::new(anchor, anchor + output_len).with_direction(range.direction());
        ranges.push(new_range);
        offset = offset
            .checked_add_unsigned(output_len)
            .expect("Selection ranges cannot overlap")
            .checked_sub_unsigned(deleted_len)
            .expect("Selection ranges cannot overlap");

        changes.push((from, to, Some(output)));
    }

    if behavior != &ShellBehavior::Ignore {
        let transaction = Transaction::change(doc.text(), changes.into_iter())
            .with_selection(Selection::new(ranges, selection.primary_index()));
        doc.apply(&transaction, view.id);
        doc.append_changes_to_history(view);
    }

    // after replace cursor may be out of bounds, do this to
    // make sure cursor is in view and update scroll as well
    view.ensure_cursor_in_view(doc, config.scrolloff);
}

fn shell_prompt<F>(cx: &mut Context, prompt: Cow<'static, str>, mut callback_fn: F)
where
    F: FnMut(&mut compositor::Context, Args) + 'static,
{
    ui::prompt(
        cx,
        prompt,
        Some('|'),
        |editor, input| complete_command_args(editor, SHELL_SIGNATURE, &SHELL_COMPLETER, input, 0),
        move |cx, input, event| {
            if event != PromptEvent::Validate || input.is_empty() {
                return;
            }
            match Args::parse(input, SHELL_SIGNATURE, true, |token| {
                expansion::expand(cx.editor, token).map_err(|err| err.into())
            }) {
                Ok(args) => callback_fn(cx, args),
                Err(err) => cx.editor.set_error(err.to_string()),
            }
        },
    );
}

fn shell_prompt_for_behavior(cx: &mut Context, prompt: Cow<'static, str>, behavior: ShellBehavior) {
    shell_prompt(cx, prompt, move |cx, args| {
        shell(cx, args.join(" ").as_str(), &behavior)
    })
}

fn suspend(_cx: &mut Context) {
    #[cfg(not(windows))]
    {
        // SAFETY: These are calls to standard POSIX functions.
        // Unsafe is necessary since we are calling outside of Rust.
        let is_session_leader = unsafe { libc::getpid() == libc::getsid(0) };

        // If zemacs is the session leader, there is nothing to suspend to, so skip
        if is_session_leader {
            return;
        }
        _cx.block_try_flush_writes().ok();
        signal_hook::low_level::raise(signal_hook::consts::signal::SIGTSTP).unwrap();
    }
}

fn add_newline_above(cx: &mut Context) {
    add_newline_impl(cx, Open::Above);
}

fn add_newline_below(cx: &mut Context) {
    add_newline_impl(cx, Open::Below)
}

fn add_newline_impl(cx: &mut Context, open: Open) {
    let count = cx.count();
    let (view, doc) = current!(cx.editor);
    let selection = doc.selection(view.id);
    let text = doc.text();
    let slice = text.slice(..);

    let changes = selection.into_iter().map(|range| {
        let (start, end) = range.line_range(slice);
        let line = match open {
            Open::Above => start,
            Open::Below => end + 1,
        };
        let pos = text.line_to_char(line);
        (
            pos,
            pos,
            Some(doc.line_ending.as_str().repeat(count).into()),
        )
    });

    let transaction = Transaction::change(text, changes);
    doc.apply(&transaction, view.id);
}

enum IncrementDirection {
    Increase,
    Decrease,
}

/// Increment objects within selections by count.
fn increment(cx: &mut Context) {
    increment_impl(cx, IncrementDirection::Increase);
}

/// Decrement objects within selections by count.
fn decrement(cx: &mut Context) {
    increment_impl(cx, IncrementDirection::Decrease);
}

/// Increment objects within selections by `amount`.
/// A negative `amount` will decrement objects within selections.
fn increment_impl(cx: &mut Context, increment_direction: IncrementDirection) {
    let sign = match increment_direction {
        IncrementDirection::Increase => 1,
        IncrementDirection::Decrease => -1,
    };
    let mut amount = sign * cx.count() as i64;
    // If the register is `#` then increase or decrease the `amount` by 1 per element
    let increase_by = if cx.register == Some('#') { sign } else { 0 };

    let (view, doc) = current!(cx.editor);
    let selection = doc.selection(view.id);
    let text = doc.text().slice(..);

    let mut new_selection_ranges = SmallVec::new();
    let mut cumulative_length_diff: i128 = 0;
    let mut changes = vec![];

    for range in selection {
        let selected_text: Cow<str> = range.fragment(text);
        let new_from = ((range.from() as i128) + cumulative_length_diff) as usize;
        let incremented = [increment::integer, increment::date_time]
            .iter()
            .find_map(|incrementor| incrementor(selected_text.as_ref(), amount));

        amount += increase_by;

        match incremented {
            None => {
                let new_range = Range::new(
                    new_from,
                    (range.to() as i128 + cumulative_length_diff) as usize,
                );
                new_selection_ranges.push(new_range);
            }
            Some(new_text) => {
                let new_range = Range::new(new_from, new_from + new_text.len());
                cumulative_length_diff += new_text.len() as i128 - selected_text.len() as i128;
                new_selection_ranges.push(new_range);
                changes.push((range.from(), range.to(), Some(new_text.into())));
            }
        }
    }

    if !changes.is_empty() {
        let new_selection = Selection::new(new_selection_ranges, selection.primary_index());
        let transaction = Transaction::change(doc.text(), changes.into_iter());
        let transaction = transaction.with_selection(new_selection);
        doc.apply(&transaction, view.id);
        exit_select_mode(cx);
    }
}

/// Insert-mode Tab handler with emmet/zen-coding expansion.
///
/// Precedence:
///   1. If a snippet is active, advance to its next tabstop (this is how Tab
///      walks through the placeholders emmet itself emits).
///   2. In an HTML-like document, try to expand the abbreviation before the
///      cursor (e.g. `ul>li*3`, `h4*100`).
///   3. Otherwise fall back to a normal Tab.
fn emmet_expand(cx: &mut Context) {
    {
        let (_, doc) = current_ref!(cx.editor);
        if doc.active_snippet.is_some() {
            goto_next_tabstop(cx);
            return;
        }
    }
    // User-defined snippets take priority over emmet's abbreviation guessing:
    // they are explicit, scoped triggers, whereas emmet only fires in HTML/CSS.
    if try_user_snippet_expand(cx) {
        return;
    }
    if try_emmet_expand(cx) {
        return;
    }
    // No abbreviation to expand — behave like a normal smart Tab.
    smart_tab(cx);
}

/// Expand the user-defined snippet whose trigger is the word before the cursor,
/// or advance to the next tabstop if a snippet is already active. Bound for
/// users who want snippet expansion on a dedicated key rather than via Tab.
fn snippet_expand(cx: &mut Context) {
    {
        let (_, doc) = current_ref!(cx.editor);
        if doc.active_snippet.is_some() {
            goto_next_tabstop(cx);
            return;
        }
    }
    if !try_user_snippet_expand(cx) {
        cx.editor.set_status("no snippet trigger before the cursor");
    }
}

/// Attempt to expand a user-defined snippet at the primary cursor: the trailing
/// run of word characters before the cursor is matched against the snippet
/// store (scoped to the current language). Returns `true` if a snippet matched
/// and was expanded (activating its tabstops), `false` otherwise.
fn try_user_snippet_expand(cx: &mut Context) -> bool {
    let (view, doc) = current!(cx.editor);
    let view_id = view.id;

    let (from, cursor, body, selection) = {
        let text = doc.text();
        let slice = text.slice(..);
        let selection = doc.selection(view_id);
        // Single cursor only.
        if selection.ranges().len() != 1 {
            return false;
        }
        let cursor = selection.primary().cursor(slice);
        let line = text.char_to_line(cursor);
        let line_start = text.line_to_char(line);
        // The trigger is the trailing run of word chars immediately before the
        // cursor on the current line.
        let before: Vec<char> = slice.slice(line_start..cursor).chars().collect();
        let trigger_len = before
            .iter()
            .rev()
            .take_while(|c| zemacs_core::chars::char_is_word(**c))
            .count();
        if trigger_len == 0 {
            return false;
        }
        let trigger: String = before[before.len() - trigger_len..].iter().collect();
        let Some(body) = crate::snippet_store::lookup_trigger(doc.language_name(), &trigger) else {
            return false;
        };
        (cursor - trigger_len, cursor, body, selection.clone())
    };

    let Ok(snippet) = zemacs_core::snippets::Snippet::parse(&body) else {
        return false;
    };
    let edit_offset = Some((from as i128 - cursor as i128, 0i128));
    let (transaction, rendered) = zemacs_lsp::util::generate_transaction_from_snippet(
        doc.text(),
        &selection,
        edit_offset,
        false,
        snippet,
        &mut doc.snippet_ctx(),
    );
    doc.apply(&transaction, view_id);
    doc.append_changes_to_history(view);
    doc.active_snippet = zemacs_core::snippets::ActiveSnippet::new(rendered);
    true
}

/// Attempt emmet expansion at the primary cursor. Returns `true` if an
/// abbreviation was expanded (and the document mutated), `false` otherwise.
fn try_emmet_expand(cx: &mut Context) -> bool {
    let (view, doc) = current!(cx.editor);
    let view_id = view.id;

    let lang = doc.language_id();
    let is_css = crate::emmet::is_css_like(lang);
    let is_html = crate::emmet::is_html_like(lang);
    if !is_css && !is_html {
        return false;
    }

    // Gather everything we need from immutable borrows first.
    let (from, cursor, snippet_str, selection) = {
        let text = doc.text();
        let slice = text.slice(..);
        let selection = doc.selection(view_id);
        // Single cursor only for now.
        if selection.ranges().len() != 1 {
            return false;
        }
        let cursor = selection.primary().cursor(slice);
        let line = text.char_to_line(cursor);
        let line_start = text.line_to_char(line);
        let before: String = slice.slice(line_start..cursor).chars().collect();

        let (start_in_line, snippet_str) = if is_css {
            let Some((start, abbr)) = crate::emmet::extract_css_abbreviation(&before) else {
                return false;
            };
            let Some(snippet_str) = crate::emmet::expand_css(&abbr) else {
                return false;
            };
            (start, snippet_str)
        } else {
            let Some((start, abbr)) = crate::emmet::extract_abbreviation(&before) else {
                return false;
            };
            let base_indent: String = before
                .chars()
                .take_while(|c| *c == ' ' || *c == '\t')
                .collect();
            let indent_unit = doc.indent_style.as_str();
            let Some(snippet_str) = crate::emmet::expand(&abbr, indent_unit, &base_indent) else {
                return false;
            };
            (start, snippet_str)
        };
        (
            line_start + start_in_line,
            cursor,
            snippet_str,
            selection.clone(),
        )
    };

    let Ok(snippet) = zemacs_core::snippets::Snippet::parse(&snippet_str) else {
        return false;
    };
    let edit_offset = Some((from as i128 - cursor as i128, 0i128));
    let (transaction, rendered) = zemacs_lsp::util::generate_transaction_from_snippet(
        doc.text(),
        &selection,
        edit_offset,
        false,
        snippet,
        &mut doc.snippet_ctx(),
    );
    doc.apply(&transaction, view_id);
    doc.append_changes_to_history(view);
    doc.active_snippet = zemacs_core::snippets::ActiveSnippet::new(rendered);
    true
}

fn goto_next_tabstop(cx: &mut Context) {
    goto_next_tabstop_impl(cx, Direction::Forward)
}

fn goto_prev_tabstop(cx: &mut Context) {
    goto_next_tabstop_impl(cx, Direction::Backward)
}

fn goto_next_tabstop_impl(cx: &mut Context, direction: Direction) {
    let (view, doc) = current!(cx.editor);
    let view_id = view.id;
    let Some(mut snippet) = doc.active_snippet.take() else {
        cx.editor.set_error("no snippet is currently active");
        return;
    };
    let tabstop = match direction {
        Direction::Forward => Some(snippet.next_tabstop(doc.selection(view_id))),
        Direction::Backward => snippet
            .prev_tabstop(doc.selection(view_id))
            .map(|selection| (selection, false)),
    };
    let Some((selection, last_tabstop)) = tabstop else {
        return;
    };
    doc.set_selection(view_id, selection);
    if !last_tabstop {
        doc.active_snippet = Some(snippet)
    }
    if cx.editor.mode() == Mode::Insert {
        cx.on_next_key_fallback(|cx, key| {
            if let Some(c) = key.char() {
                let (view, doc) = current!(cx.editor);
                if let Some(snippet) = &doc.active_snippet {
                    doc.apply(&snippet.delete_placeholder(doc.text()), view.id);
                }
                insert_char(cx, c);
            }
        })
    }
}

fn record_macro(cx: &mut Context) {
    if let Some((reg, mut keys)) = cx.editor.macro_recording.take() {
        // Remove the keypress which ends the recording
        keys.pop();
        let s = keys
            .into_iter()
            .map(|key| {
                let s = key.to_string();
                if s.chars().count() == 1 {
                    s
                } else {
                    format!("<{}>", s)
                }
            })
            .collect::<String>();
        macro_ring_push(s.clone());
        match cx.editor.registers.write(reg, vec![s]) {
            Ok(_) => cx
                .editor
                .set_status(format!("Recorded to register [{}]", reg)),
            Err(err) => cx.editor.set_error(err.to_string()),
        }
    } else {
        let reg = cx.register.take().unwrap_or('@');
        cx.editor.macro_recording = Some((reg, Vec::new()));
        cx.editor
            .set_status(format!("Recording to register [{}]", reg));
    }
}

fn replay_macro(cx: &mut Context) {
    let reg = cx.register.unwrap_or('@');

    if cx.editor.macro_replaying.contains(&reg) {
        cx.editor.set_error(format!(
            "Cannot replay from register [{}] because already replaying from same register",
            reg
        ));
        return;
    }

    let keys: Vec<KeyEvent> = if let Some(keys) = cx
        .editor
        .registers
        .read(reg, cx.editor)
        .filter(|values| values.len() == 1)
        .map(|mut values| values.next().unwrap())
    {
        match zemacs_view::input::parse_macro(&keys) {
            Ok(keys) => keys,
            Err(err) => {
                cx.editor.set_error(format!("Invalid macro: {}", err));
                return;
            }
        }
    } else {
        cx.editor.set_error(format!("Register [{}] empty", reg));
        return;
    };

    // Once the macro has been fully validated, it's marked as being under replay
    // to ensure we don't fall into infinite recursion.
    cx.editor.macro_replaying.push(reg);

    let count = cx.count();
    cx.callback.push(Box::new(move |compositor, cx| {
        for _ in 0..count {
            for &key in keys.iter() {
                compositor.handle_event(&compositor::Event::Key(key), cx);
            }
        }
        // The macro under replay is cleared at the end of the callback, not in the
        // macro replay context, or it will not correctly protect the user from
        // replaying recursively.
        cx.editor.macro_replaying.pop();
    }));
}

fn goto_word(cx: &mut Context) {
    jump_to_word(cx, Movement::Move)
}

fn extend_to_word(cx: &mut Context) {
    jump_to_word(cx, Movement::Extend)
}

fn jump_to_label(cx: &mut Context, labels: Vec<Range>, behaviour: Movement) {
    let doc = doc!(cx.editor);
    let alphabet = &cx.editor.config().jump_label_alphabet;
    if labels.is_empty() {
        return;
    }
    let alphabet_char = |i| {
        let mut res = Tendril::new();
        res.push(alphabet[i]);
        res
    };

    // Add label for each jump candidate to the View as virtual text.
    let text = doc.text().slice(..);
    let mut overlays: Vec<_> = labels
        .iter()
        .enumerate()
        .flat_map(|(i, range)| {
            [
                Overlay::new(range.from(), alphabet_char(i / alphabet.len())),
                Overlay::new(
                    graphemes::next_grapheme_boundary(text, range.from()),
                    alphabet_char(i % alphabet.len()),
                ),
            ]
        })
        .collect();
    overlays.sort_unstable_by_key(|overlay| overlay.char_idx);
    let (view, doc) = current!(cx.editor);
    doc.set_jump_labels(view.id, overlays);

    // Accept two characters matching a visible label. Jump to the candidate
    // for that label if it exists.
    let primary_selection = doc.selection(view.id).primary();
    let view_id = view.id;
    let doc = doc.id();
    cx.on_next_key(move |cx, event| {
        let alphabet = &cx.editor.config().jump_label_alphabet;
        let Some(i) = event
            .char()
            .filter(|_| event.modifiers.is_empty())
            .and_then(|ch| alphabet.iter().position(|&it| it == ch))
        else {
            doc_mut!(cx.editor, &doc).remove_jump_labels(view_id);
            return;
        };
        let outer = i * alphabet.len();
        // Bail if the given character cannot be a jump label.
        if outer > labels.len() {
            doc_mut!(cx.editor, &doc).remove_jump_labels(view_id);
            return;
        }
        cx.on_next_key(move |cx, event| {
            doc_mut!(cx.editor, &doc).remove_jump_labels(view_id);
            let alphabet = &cx.editor.config().jump_label_alphabet;
            let Some(inner) = event
                .char()
                .filter(|_| event.modifiers.is_empty())
                .and_then(|ch| alphabet.iter().position(|&it| it == ch))
            else {
                return;
            };
            if let Some(mut range) = labels.get(outer + inner).copied() {
                range = if behaviour == Movement::Extend {
                    let anchor = if range.anchor < range.head {
                        let from = primary_selection.from();
                        if range.anchor < from {
                            range.anchor
                        } else {
                            from
                        }
                    } else {
                        let to = primary_selection.to();
                        if range.anchor > to {
                            range.anchor
                        } else {
                            to
                        }
                    };
                    Range::new(anchor, range.head)
                } else {
                    range.with_direction(Direction::Forward)
                };
                let doc = doc_mut!(cx.editor, &doc);
                let view = view_mut!(cx.editor, view_id);
                push_jump(view, doc);
                doc.set_selection(view_id, range.into());
            }
        });
    });
}

fn jump_to_word(cx: &mut Context, behaviour: Movement) {
    // Calculate the jump candidates: ranges for any visible words with two or
    // more characters.
    let alphabet = &cx.editor.config().jump_label_alphabet;
    if alphabet.is_empty() {
        return;
    }

    let jump_label_limit = alphabet.len() * alphabet.len();
    let mut words = Vec::with_capacity(jump_label_limit);
    let (view, doc) = current_ref!(cx.editor);
    let text = doc.text().slice(..);

    // This is not necessarily exact if there is virtual text like soft wrap.
    // It's ok though because the extra jump labels will not be rendered.
    let start = text.line_to_char(text.char_to_line(doc.view_offset(view.id).anchor));
    let end = text.line_to_char(view.estimate_last_doc_line(doc) + 1);

    let primary_selection = doc.selection(view.id).primary();
    let cursor = primary_selection.cursor(text);
    let mut cursor_fwd = Range::point(cursor);
    let mut cursor_rev = Range::point(cursor);
    if text.get_char(cursor).is_some_and(|c| !c.is_whitespace()) {
        let cursor_word_end = movement::move_next_word_end(text, cursor_fwd, 1);
        //  single grapheme words need a special case
        if cursor_word_end.anchor == cursor {
            cursor_fwd = cursor_word_end;
        }
        let cursor_word_start = movement::move_prev_word_start(text, cursor_rev, 1);
        if cursor_word_start.anchor == next_grapheme_boundary(text, cursor) {
            cursor_rev = cursor_word_start;
        }
    }
    'outer: loop {
        let mut changed = false;
        while cursor_fwd.head < end {
            cursor_fwd = movement::move_next_word_end(text, cursor_fwd, 1);
            // The cursor is on a word that is atleast two graphemes long and
            // madeup of word characters. The latter condition is needed because
            // move_next_word_end simply treats a sequence of characters from
            // the same char class as a word so `=<` would also count as a word.
            let add_label = text
                .slice(..cursor_fwd.head)
                .graphemes_rev()
                .take(2)
                .take_while(|g| g.chars().all(char_is_word))
                .count()
                == 2;
            if !add_label {
                continue;
            }
            changed = true;
            // skip any leading whitespace
            cursor_fwd.anchor += text
                .chars_at(cursor_fwd.anchor)
                .take_while(|&c| !char_is_word(c))
                .count();
            words.push(cursor_fwd);
            if words.len() == jump_label_limit {
                break 'outer;
            }
            break;
        }
        while cursor_rev.head > start {
            cursor_rev = movement::move_prev_word_start(text, cursor_rev, 1);
            // The cursor is on a word that is atleast two graphemes long and
            // madeup of word characters. The latter condition is needed because
            // move_prev_word_start simply treats a sequence of characters from
            // the same char class as a word so `=<` would also count as a word.
            let add_label = text
                .slice(cursor_rev.head..)
                .graphemes()
                .take(2)
                .take_while(|g| g.chars().all(char_is_word))
                .count()
                == 2;
            if !add_label {
                continue;
            }
            changed = true;
            cursor_rev.anchor -= text
                .chars_at(cursor_rev.anchor)
                .reversed()
                .take_while(|&c| !char_is_word(c))
                .count();
            words.push(cursor_rev);
            if words.len() == jump_label_limit {
                break 'outer;
            }
            break;
        }
        if !changed {
            break;
        }
    }
    jump_to_label(cx, words, behaviour)
}

fn lsp_or_syntax_symbol_picker(cx: &mut Context) {
    let doc = doc!(cx.editor);

    if doc
        .language_servers_with_feature(LanguageServerFeature::DocumentSymbols)
        .next()
        .is_some()
    {
        lsp::symbol_picker(cx);
    } else if doc.syntax().is_some() {
        syntax_symbol_picker(cx);
    } else {
        cx.editor
            .set_error("No language server supporting document symbols or syntax info available");
    }
}

fn lsp_or_syntax_workspace_symbol_picker(cx: &mut Context) {
    let doc = doc!(cx.editor);

    if doc
        .language_servers_with_feature(LanguageServerFeature::WorkspaceSymbols)
        .next()
        .is_some()
    {
        lsp::workspace_symbol_picker(cx);
    } else {
        syntax_workspace_symbol_picker(cx);
    }
}

#[cfg(test)]
mod insert_generator_tests {
    use super::*;

    fn lines(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn comment_fold_runs_groups_consecutive() {
        assert_eq!(comment_fold_runs(&[true, true, false, true]), vec![(0, 1)]);
        assert_eq!(comment_fold_runs(&[false, true, true, true]), vec![(1, 3)]);
        assert!(comment_fold_runs(&[true, false, true]).is_empty()); // singles don't fold
    }

    #[test]
    fn test_counterpart_names_both_directions() {
        // impl -> test candidates
        let c = test_counterpart_names("foo.rs");
        assert!(c.contains(&"foo_test.rs".to_string()));
        assert!(c.contains(&"test_foo.rs".to_string()));
        // test -> impl
        assert_eq!(test_counterpart_names("foo_test.rs"), vec!["foo.rs".to_string()]);
        assert_eq!(test_counterpart_names("test_foo.py"), vec!["foo.py".to_string()]);
        assert_eq!(test_counterpart_names("foo.test.js"), vec!["foo.js".to_string()]);
    }

    #[test]
    fn reorder_two_lines_handles_newlines() {
        // middle lines: both have trailing newlines
        assert_eq!(reorder_two_lines("foo\n", "bar\n", "\n"), "bar\nfoo\n");
        // B is the final line (no trailing newline): B gains one, A loses it
        assert_eq!(reorder_two_lines("foo\n", "bar", "\n"), "bar\nfoo");
    }

    #[test]
    fn paredit_convolute_swaps_prefixes() {
        let ch: Vec<char> = "(as (bs c))".chars().collect();
        let (out, _) = pe_convolute(&ch, 8).expect("convolute"); // point at `c`
        assert_eq!(out.iter().collect::<String>(), "(bs (as c))");
    }

    #[test]
    fn paredit_absorb_pulls_previous_into_form() {
        let ch: Vec<char> = "(a (bs c))".chars().collect();
        let (out, _) = pe_absorb(&ch, 4).expect("absorb"); // point inside (bs c)
        // `a` is pulled inside after `bs`; outer now wraps just the inner form
        assert_eq!(out.iter().collect::<String>(), "( (bs a c))");
    }

    #[test]
    fn paredit_split_makes_two_lists() {
        let ch: Vec<char> = "(a b c)".chars().collect();
        let (out, cur) = pe_split(&ch, 4).expect("split");
        assert_eq!(out.iter().collect::<String>(), "(a b) ( c)");
        assert_eq!(cur, 7);
    }

    #[test]
    fn emacs_regex_to_rx_basic() {
        assert_eq!(emacs_regex_to_rx("abc").unwrap(), "\"abc\"");
        assert_eq!(emacs_regex_to_rx("a*").unwrap(), "(zero-or-more \"a\")");
        assert_eq!(emacs_regex_to_rx("a+").unwrap(), "(one-or-more \"a\")");
        assert_eq!(
            emacs_regex_to_rx("\\(foo\\|bar\\)").unwrap(),
            "(group (or \"foo\" \"bar\"))"
        );
        assert_eq!(emacs_regex_to_rx("[a-z]").unwrap(), "(any \"a-z\")");
        assert_eq!(emacs_regex_to_rx("a\\{2,3\\}").unwrap(), "(** 2 3 \"a\")");
        // honest: unsupported escape errors rather than emitting garbage
        assert!(emacs_regex_to_rx("\\q").is_err());
    }

    #[test]
    fn regex_grouping_swaps_pcre_and_emacs() {
        // PCRE -> Emacs: bare grouping/alt becomes escaped
        assert_eq!(swap_regex_grouping("(foo|bar)+"), "\\(foo\\|bar\\)+");
        // round-trips back to PCRE
        assert_eq!(swap_regex_grouping("\\(foo\\|bar\\)+"), "(foo|bar)+");
        // other escapes (\d, \b) are preserved untouched
        assert_eq!(swap_regex_grouping("\\d+\\bx"), "\\d+\\bx");
        // quantifier braces toggle too
        assert_eq!(swap_regex_grouping("a{2,3}"), "a\\{2,3\\}");
    }

    #[test]
    fn transpose_sexp_swaps_balanced_groups() {
        let s = "(a) (b)";
        let ch: Vec<char> = s.chars().collect();
        // cursor inside the second group
        let (pr, cr) = sexp_pair(&ch, 5).expect("sexp pair");
        let out = swap_ranges_text(s, pr, cr);
        assert_eq!(out, "(b) (a)");
    }

    #[test]
    fn transpose_sentence_swaps_with_previous() {
        let s = "One. Two.";
        let ch: Vec<char> = s.chars().collect();
        let (pr, cr) = sentence_pair(&ch, 6).expect("sentence pair");
        let out = swap_ranges_text(s, pr, cr);
        assert!(out.starts_with("Two."), "got {out:?}");
    }

    #[test]
    fn transpose_paragraph_swaps_with_previous() {
        // three lines, blank, three lines — cursor in the second paragraph
        let s = "alpha\n\nbeta\n";
        let line_starts: Vec<usize> = {
            let mut v = vec![0];
            let mut acc = 0;
            for line in s.split_inclusive('\n') {
                acc += line.chars().count();
                v.push(acc);
            }
            v
        };
        let blanks = vec![false, true, false];
        let (pr, cr) = paragraph_ranges(&line_starts, &blanks, 2).expect("ranges");
        let out = swap_ranges_text(s, pr, cr);
        assert!(out.starts_with("beta"), "current paragraph moved up: {out:?}");
        assert!(out.trim_end().ends_with("alpha"), "previous moved down: {out:?}");
    }

    #[test]
    fn fill_justify_wraps_and_aligns() {
        let t = "the quick brown fox jumps over the lazy dog";
        let left = fill_justify(t, 20, Justify::Left);
        assert!(left.lines().all(|l| l.chars().count() <= 20), "left: {left:?}");
        let right = fill_justify(t, 20, Justify::Right);
        // every non-final wrapped line is padded to the width on the right
        let rlines: Vec<&str> = right.lines().collect();
        assert!(rlines.len() >= 2);
        assert_eq!(rlines[0].chars().count(), 20, "right pads to width: {right:?}");
        // full justification pads interior lines to exactly the width
        let full = fill_justify(t, 20, Justify::Full);
        let flines: Vec<&str> = full.lines().collect();
        assert_eq!(flines[0].chars().count(), 20, "full fills width: {full:?}");
    }

    #[test]
    fn align_at_equals_pads_to_shared_column() {
        let re = regex::Regex::new("=").unwrap();
        let out = align_lines(&lines(&["a = 1", "bb = 2", "ccc = 3"]), &re, false);
        // delimiter column is the same on every line
        let cols: Vec<usize> = out.iter().map(|l| l.find('=').unwrap()).collect();
        assert!(cols.windows(2).all(|w| w[0] == w[1]), "aligned: {out:?}");
        // content preserved
        assert!(out[0].starts_with("a"));
        assert!(out[2].starts_with("ccc"));
    }

    #[test]
    fn align_skips_lines_without_delimiter() {
        let re = regex::Regex::new(":").unwrap();
        let out = align_lines(&lines(&["x: 1", "no delim here", "yy: 2"]), &re, false);
        assert_eq!(out[1], "no delim here");
        assert_eq!(out[0].find(':'), out[2].find(':'));
    }

    #[test]
    fn align_right_justifies_before_text() {
        let re = regex::Regex::new("=").unwrap();
        let out = align_lines(&lines(&["a = 1", "bbb = 2"]), &re, true);
        // delimiters still aligned; shorter "before" is right-padded with leading spaces
        assert_eq!(out[0].find('='), out[1].find('='));
        assert!(out[0].starts_with(' '));
    }

    fn is_hex(s: &str) -> bool {
        s.chars().all(|c| c.is_ascii_hexdigit())
    }

    fn assert_uuid_shape(u: &str, version: char) {
        let parts: Vec<&str> = u.split('-').collect();
        assert_eq!(parts.len(), 5, "uuid has 5 groups: {u}");
        let lens = [8, 4, 4, 4, 12];
        for (p, l) in parts.iter().zip(lens) {
            assert_eq!(p.len(), l, "group length in {u}");
            assert!(is_hex(p), "group is hex in {u}");
        }
        // version nibble is the first char of group 3; variant is 8/9/a/b.
        assert_eq!(parts[2].chars().next().unwrap(), version, "version of {u}");
        assert!(
            matches!(parts[3].chars().next().unwrap(), '8' | '9' | 'a' | 'b'),
            "rfc4122 variant of {u}"
        );
    }

    #[test]
    fn uuid_v4_is_well_formed_and_random() {
        let a = uuid_v4_string();
        let b = uuid_v4_string();
        assert_uuid_shape(&a, '4');
        assert_uuid_shape(&b, '4');
        assert_ne!(a, b, "two v4 uuids should differ");
    }

    #[test]
    fn uuid_v1_is_well_formed() {
        assert_uuid_shape(&uuid_v1_string(), '1');
    }

    #[test]
    fn lorem_has_expected_structure() {
        let s = lorem_sentence(5);
        assert!(s.ends_with('.'));
        assert_eq!(s.trim_end_matches('.').split(' ').count(), 5);
        assert!(s.chars().next().unwrap().is_uppercase());

        let list = lorem_list(3);
        assert_eq!(list.lines().count(), 3);
        assert!(list.lines().all(|l| l.starts_with("- ")));
    }

    #[test]
    fn symbol_case_styles() {
        for input in [
            "myVariableName",
            "my_variable_name",
            "my-variable-name",
            "MyVariableName",
        ] {
            assert_eq!(
                to_upper_camel(input),
                "MyVariableName",
                "camel from {input}"
            );
            assert_eq!(to_up_case(input), "MY_VARIABLE_NAME", "upcase from {input}");
            assert_eq!(
                to_under_score(input),
                "my_variable_name",
                "snake from {input}"
            );
        }
        assert_eq!(to_upper_camel("http2Server"), "Http2Server");
    }

    #[test]
    fn region_shuffles_are_permutations() {
        let shuffled = randomize_lines("a\nb\nc\nd\ne\n");
        let mut got: Vec<&str> = shuffled.lines().collect();
        got.sort_unstable();
        assert_eq!(got, vec!["a", "b", "c", "d", "e"]);
        assert!(shuffled.ends_with('\n'));

        let words = randomize_words("one two three four");
        let mut got: Vec<&str> = words.split(' ').collect();
        got.sort_unstable();
        assert_eq!(got, vec!["four", "one", "three", "two"]);
    }

    fn paredit_run(f: super::PareditFn, s: &str, cursor_marker: char) -> Option<String> {
        // `cursor_marker` in `s` marks the cursor position, then is removed.
        let pos = s.find(cursor_marker).expect("marker present");
        let clean: String = s.chars().filter(|&c| c != cursor_marker).collect();
        // char index of the marker (markers are single-byte ASCII here)
        let pos = clean[..pos].chars().count();
        let ch: Vec<char> = clean.chars().collect();
        f(&ch, pos).map(|(v, _)| v.into_iter().collect())
    }

    #[test]
    fn paredit_slurp_and_barf() {
        // slurp forward: cursor inside (b), pull c in
        assert_eq!(
            paredit_run(pe_slurp_forward, "(a (b|) c)", '|').as_deref(),
            Some("(a (b c))")
        );
        // barf forward: push c out
        assert_eq!(
            paredit_run(pe_barf_forward, "(a (b| c))", '|').as_deref(),
            Some("(a (b) c)")
        );
        // slurp backward: pull a in
        assert_eq!(
            paredit_run(pe_slurp_backward, "(a (b|) c)", '|').as_deref(),
            Some("((a b) c)")
        );
        // barf backward: push b out
        assert_eq!(
            paredit_run(pe_barf_backward, "((a| b) c)", '|').as_deref(),
            Some("(a (b) c)")
        );
    }

    #[test]
    fn paredit_splice_raise_transpose() {
        assert_eq!(
            paredit_run(pe_splice, "(a (b| c) d)", '|').as_deref(),
            Some("(a b c d)")
        );
        // raise replaces the enclosing list with the s-expression at point
        assert_eq!(
            paredit_run(pe_raise, "(a (b |c) d)", '|').as_deref(),
            Some("(a c d)")
        );
        // transpose the atom before point with the one after
        assert_eq!(
            paredit_run(pe_transpose, "(a |b)", '|').as_deref(),
            Some("(b a)")
        );
    }

    #[test]
    fn paredit_splice_killing() {
        assert_eq!(
            paredit_run(pe_splice_kill_forward, "(as (bs| cs) ds)", '|').as_deref(),
            Some("(as bs ds)")
        );
        assert_eq!(
            paredit_run(pe_splice_kill_backward, "(as (bs |cs) ds)", '|').as_deref(),
            Some("(as cs ds)")
        );
    }

    #[test]
    fn paredit_insert_sibling_sexp() {
        assert_eq!(
            paredit_run(pe_insert_sexp_after, "(a|)", '|').as_deref(),
            Some("(a) ()")
        );
        assert_eq!(
            paredit_run(pe_insert_sexp_before, "(a|)", '|').as_deref(),
            Some("() (a)")
        );
    }

    #[test]
    fn paredit_no_target_returns_none() {
        // no enclosing pair → None (command would just report a status)
        assert!(pe_slurp_forward(&"a b c".chars().collect::<Vec<_>>(), 2).is_none());
    }

    #[test]
    fn kmacro_counter_add_and_reset() {
        kmacro_counter_reset();
        assert_eq!(kmacro_counter_value(), 0);
        assert_eq!(kmacro_counter_add(1), 1);
        assert_eq!(kmacro_counter_add(5), 6);
        assert_eq!(kmacro_counter_value(), 6);
        kmacro_counter_reset();
        assert_eq!(kmacro_counter_value(), 0);
    }

    #[test]
    fn narrow_ranges_fold_outside_region() {
        // region in the middle: fold before and after
        assert_eq!(narrow_outside_ranges(3, 7, 10), vec![(0, 2), (8, 10)]);
        // region at the top: only fold after
        assert_eq!(narrow_outside_ranges(0, 4, 10), vec![(5, 10)]);
        // region at the bottom: only fold before
        assert_eq!(narrow_outside_ranges(6, 10, 10), vec![(0, 5)]);
        // whole buffer selected: nothing to fold
        assert_eq!(
            narrow_outside_ranges(0, 10, 10),
            Vec::<(usize, usize)>::new()
        );
    }

    #[test]
    fn passwords_have_right_length_and_charset() {
        assert_eq!(password(12, PW_ALNUM).len(), 12);
        assert!(password(12, PW_ALNUM)
            .chars()
            .all(|c| c.is_ascii_alphanumeric()));
        assert!(password(8, b"0123456789")
            .chars()
            .all(|c| c.is_ascii_digit()));
        let ph = password_phonetic(14);
        assert_eq!(ph.len(), 14);
        assert!(ph.chars().all(|c| c.is_ascii_lowercase()));
    }
}

#[cfg(test)]
mod path_yank_tests {
    use super::{format_file_path, FilePathKind};
    use std::path::Path;

    #[test]
    fn file_path_formats() {
        let p = Path::new("/home/u/proj/src/main.rs");
        assert_eq!(
            format_file_path(p, FilePathKind::Full, 7, 3),
            "/home/u/proj/src/main.rs"
        );
        assert_eq!(format_file_path(p, FilePathKind::Name, 7, 3), "main.rs");
        assert_eq!(
            format_file_path(p, FilePathKind::WithLine, 7, 3),
            "/home/u/proj/src/main.rs:7"
        );
        assert_eq!(
            format_file_path(p, FilePathKind::WithLineCol, 7, 3),
            "/home/u/proj/src/main.rs:7:3"
        );
        assert_eq!(
            format_file_path(p, FilePathKind::Dir, 7, 3),
            "/home/u/proj/src"
        );
    }

    #[test]
    fn conflict_marker_navigation() {
        use super::{find_conflict_line, is_conflict_marker, Rope};

        let text = "one\n\
                    <<<<<<< HEAD\n\
                    ours\n\
                    =======\n\
                    theirs\n\
                    >>>>>>> branch\n\
                    middle\n\
                    <<<<<<< HEAD\n\
                    a\n\
                    =======\n\
                    b\n\
                    >>>>>>> other\n\
                    tail\n";
        let rope = Rope::from_str(text);
        let slice = rope.slice(..);

        // Marker detection: only the conflict lines match.
        assert!(is_conflict_marker(slice.line(1)));
        assert!(is_conflict_marker(slice.line(3)));
        assert!(is_conflict_marker(slice.line(5)));
        assert!(!is_conflict_marker(slice.line(0)));
        assert!(!is_conflict_marker(slice.line(2)));

        // Forward from the top lands on the first `<<<<<<<` (line 1), then the
        // next marker (the `=======` on line 3), and so on.
        assert_eq!(find_conflict_line(slice, 0, true), Some(1));
        assert_eq!(find_conflict_line(slice, 1, true), Some(3));
        // Forward from past the first hunk reaches the second hunk's start.
        assert_eq!(find_conflict_line(slice, 6, true), Some(7));

        // Backward from the bottom lands on the last marker, then the previous.
        assert_eq!(find_conflict_line(slice, 12, false), Some(11));
        assert_eq!(find_conflict_line(slice, 7, false), Some(5));

        // No markers past the end / before the start.
        assert_eq!(find_conflict_line(slice, 11, true), None);
        assert_eq!(find_conflict_line(slice, 1, false), None);
    }

    #[test]
    fn count_region_counts() {
        use super::count_region;
        assert_eq!(count_region(""), (0, 0, 0));
        assert_eq!(count_region("hello world"), (11, 2, 1));
        assert_eq!(count_region("a b\nc d\n"), (8, 4, 3));
    }

    #[test]
    fn url_under_cursor() {
        use super::url_at;
        let line = "see https://example.com/path for more.";
        // cursor anywhere inside the URL finds it
        assert_eq!(url_at(line, 4).as_deref(), Some("https://example.com/path"));
        assert_eq!(
            url_at(line, 20).as_deref(),
            Some("https://example.com/path")
        );
        // cursor in plain text → None
        assert_eq!(url_at(line, 0), None);
        // trailing punctuation trimmed
        assert_eq!(url_at("(http://x.io).", 5).as_deref(), Some("http://x.io"));
        // www. without scheme is recognized
        assert_eq!(
            url_at("go www.foo.com now", 6).as_deref(),
            Some("www.foo.com")
        );
    }

    #[test]
    fn remote_url_normalizes() {
        use super::git_remote_to_web_base as b;
        assert_eq!(
            b("git@github.com:owner/repo.git").as_deref(),
            Some("https://github.com/owner/repo")
        );
        assert_eq!(
            b("git@github.com:owner/repo").as_deref(),
            Some("https://github.com/owner/repo")
        );
        assert_eq!(
            b("https://github.com/owner/repo.git").as_deref(),
            Some("https://github.com/owner/repo")
        );
        assert_eq!(
            b("https://gitlab.com/g/sub/repo.git").as_deref(),
            Some("https://gitlab.com/g/sub/repo")
        );
        assert_eq!(
            b("ssh://git@github.com/owner/repo.git").as_deref(),
            Some("https://github.com/owner/repo")
        );
        assert_eq!(
            b("ssh://git@ssh.github.com:443/owner/repo.git").as_deref(),
            Some("https://ssh.github.com/owner/repo")
        );
        assert_eq!(
            b("git://github.com/owner/repo.git").as_deref(),
            Some("https://github.com/owner/repo")
        );
        assert_eq!(b("not a url"), None);
    }

    #[test]
    fn strip_ansi_codes() {
        use super::strip_ansi;
        // SGR color codes removed, text kept
        assert_eq!(strip_ansi("\u{1b}[31mhello\u{1b}[0m"), "hello");
        assert_eq!(strip_ansi("a\u{1b}[1;32mb\u{1b}[mc"), "abc");
        // OSC sequence (window title) removed
        assert_eq!(strip_ansi("\u{1b}]0;title\u{07}x"), "x");
        // multi-byte text preserved
        assert_eq!(strip_ansi("café\u{1b}[0m"), "café");
        // plain text untouched
        assert_eq!(strip_ansi("plain text"), "plain text");
    }

    #[test]
    fn html_escape_roundtrip() {
        use super::{html_escape, html_unescape};
        let raw = r#"a < b && c > "d" 'e'"#;
        let escaped = r#"a &lt; b &amp;&amp; c &gt; &quot;d&quot; &#39;e&#39;"#;
        assert_eq!(html_escape(raw), escaped);
        // escape then unescape is the identity
        assert_eq!(html_unescape(&html_escape(raw)), raw);
        // named, decimal, and hex numeric references all decode
        assert_eq!(html_unescape("&lt;&apos;&#64;&#x41;&gt;"), "<'@A>");
        // unknown entity left verbatim
        assert_eq!(html_unescape("a &bogus; b"), "a &bogus; b");
    }

    #[test]
    fn reverse_chars_reverses() {
        use super::reverse_chars;
        assert_eq!(reverse_chars("abc"), "cba");
        assert_eq!(reverse_chars("a b"), "b a");
        // precomposed accented character preserved (not byte-reversed into mojibake)
        assert_eq!(reverse_chars("héllo"), "olléh");
        // double reverse is the identity
        assert_eq!(reverse_chars(&reverse_chars("hello world")), "hello world");
    }

    #[test]
    fn xml_pretty_prints() {
        use super::pretty_xml;
        assert_eq!(
            pretty_xml("<a><b>x</b></a>", "  "),
            "<a>\n  <b>\n    x\n  </b>\n</a>"
        );
        // self-closing tags don't increase depth
        assert_eq!(
            pretty_xml("<r><br/><br/></r>", "  "),
            "<r>\n  <br/>\n  <br/>\n</r>"
        );
        // a `>` inside a quoted attribute does not end the tag early
        assert_eq!(
            pretty_xml(r#"<a x="1>2"><c/></a>"#, "  "),
            "<a x=\"1>2\">\n  <c/>\n</a>"
        );
        // comments and declarations are passed through, not nested
        assert_eq!(pretty_xml("<!-- hi --><x/>", "  "), "<!-- hi -->\n<x/>");
    }

    #[test]
    fn json_pretty_and_minify() {
        use super::{minify_json, pretty_json};
        let compact = r#"{"a":1,"b":[2,3],"c":{}}"#;
        let pretty = "{\n  \"a\": 1,\n  \"b\": [\n    2,\n    3\n  ],\n  \"c\": {}\n}";
        assert_eq!(pretty_json(compact, "  "), pretty);
        // minify is the inverse (for already-compact-friendly input)
        assert_eq!(minify_json(pretty), compact);
        // commas/colons/braces inside string literals are NOT treated as structure
        let s = r#"{"k":"a, b: {c}"}"#;
        assert_eq!(minify_json(&pretty_json(s, "  ")), s);
        // key order is preserved (z before a)
        assert!(
            pretty_json(r#"{"z":1,"a":2}"#, "  ").find("\"z\"").unwrap()
                < pretty_json(r#"{"z":1,"a":2}"#, "  ").find("\"a\"").unwrap()
        );
    }

    #[test]
    fn table_becomes_csv() {
        use super::markdown_table_to_csv;
        assert_eq!(
            markdown_table_to_csv("| name | age |\n| --- | --- |\n| Alice | 30 |\n"),
            "name,age\nAlice,30\n"
        );
        // a cell containing a comma gets quoted (RFC 4180)
        assert_eq!(
            markdown_table_to_csv("| a | b |\n|---|---|\n| x, y | z |\n"),
            "a,b\n\"x, y\",z\n"
        );
        // round-trips with csv_to_markdown_table for a simple table
        let csv = "h1,h2\nv1,v2\n";
        assert_eq!(
            markdown_table_to_csv(&super::csv_to_markdown_table(csv)),
            csv
        );
    }

    #[test]
    fn csv_becomes_table() {
        use super::csv_to_markdown_table;
        assert_eq!(
            csv_to_markdown_table("name,age\nAlice,30\nBob,5\n"),
            "| name  | age |\n| ----- | --- |\n| Alice | 30  |\n| Bob   | 5   |\n"
        );
        // tab-separated input is auto-detected
        assert_eq!(
            csv_to_markdown_table("a\tbb\n1\t2"),
            "| a   | bb  |\n| --- | --- |\n| 1   | 2   |"
        );
    }

    #[test]
    fn markdown_table_aligns() {
        use super::format_markdown_table;
        // columns pad to their widest cell (min 3 so the separator stays `---`)
        let input = "| a | bb |\n|-|-|\n| 1 | 2 |\n";
        let expected = "| a   | bb  |\n| --- | --- |\n| 1   | 2   |\n";
        assert_eq!(format_markdown_table(input), expected);
        // alignment colons in the separator are preserved
        let input2 = "| x | y |\n|:-|-:|\n| 100 | z |\n";
        let expected2 = "| x   | y   |\n| :-- | --: |\n| 100 | z   |\n";
        assert_eq!(format_markdown_table(input2), expected2);
        // ragged rows are padded to the column count
        let input3 = "| a | b | c |\n| 1 | 2 |\n";
        let expected3 = "| a   | b   | c   |\n| 1   | 2   |     |\n";
        assert_eq!(format_markdown_table(input3), expected3);
    }

    #[test]
    fn hex_encode_decode() {
        use super::{from_hex, to_hex};
        assert_eq!(to_hex("AB"), "41 42");
        assert_eq!(to_hex("é"), "c3 a9"); // UTF-8 bytes
        assert_eq!(from_hex("41 42"), "AB");
        assert_eq!(from_hex("4142"), "AB"); // spaces optional
        assert_eq!(from_hex("c3a9"), "é");
        // non-hex chars (and an odd trailing nibble) are ignored
        assert_eq!(from_hex("41 42 zz"), "AB");
        // round-trip
        let s = "Hex! 世界";
        assert_eq!(from_hex(&to_hex(s)), s);
    }

    #[test]
    fn to_json_string_wraps() {
        use super::{json_unescape, to_json_string};
        assert_eq!(to_json_string("hi"), "\"hi\"");
        // quotes and newlines escaped inside the wrapping quotes
        assert_eq!(to_json_string("a\"b\nc"), "\"a\\\"b\\nc\"");
        // stripping the wrapping quotes + unescaping round-trips to the original
        let s = "multi\nline \"quoted\"";
        let wrapped = to_json_string(s);
        assert_eq!(json_unescape(&wrapped[1..wrapped.len() - 1]), s);
    }

    #[test]
    fn json_escape_unescape() {
        use super::{json_escape, json_unescape};
        assert_eq!(
            json_escape("he said \"hi\"\n\ttab"),
            "he said \\\"hi\\\"\\n\\ttab"
        );
        assert_eq!(json_escape("back\\slash"), "back\\\\slash");
        // control char → \u escape
        assert_eq!(json_escape("\u{01}"), "\\u0001");
        // unescape inverts escape
        assert_eq!(json_unescape("a\\nb\\t\\\"c\\\""), "a\nb\t\"c\"");
        assert_eq!(json_unescape("\\u0041\\u00e9"), "Aé");
        // round-trip including a quote, backslash, and newline
        let s = "path \"C:\\\\x\"\nline2";
        assert_eq!(json_unescape(&json_escape(s)), s);
    }

    #[test]
    fn markdown_toc_builds() {
        use super::{github_anchor, markdown_toc};
        // GitHub anchor rules
        assert_eq!(github_anchor("Hello, World!"), "hello-world");
        assert_eq!(github_anchor("Foo & Bar"), "foo--bar"); // & dropped, both spaces → -
        assert_eq!(github_anchor("snake_case Title"), "snake_case-title");
        // nested TOC from headings, skipping a fenced code block
        let md = "# Title\n\n## Section One\n```\n## not a heading\n```\n### Deep\n#nope\n";
        assert_eq!(
            markdown_toc(md),
            "- [Title](#title)\n  - [Section One](#section-one)\n    - [Deep](#deep)\n"
        );
        // no headings → empty
        assert_eq!(markdown_toc("just text\nmore text\n"), "");
    }

    #[test]
    fn normalize_whitespace_collapses() {
        use super::normalize_whitespace;
        // internal runs collapse to one space; trailing trimmed
        assert_eq!(normalize_whitespace("a   b\t c  \n"), "a b c\n");
        // leading indentation preserved
        assert_eq!(normalize_whitespace("    a    b\n"), "    a b\n");
        // multiple lines, no trailing newline
        assert_eq!(normalize_whitespace("x  y\n  z   w"), "x y\n  z w");
        // already-clean text unchanged
        assert_eq!(normalize_whitespace("clean text\n"), "clean text\n");
    }

    #[test]
    fn toggle_word_pairs() {
        use super::{match_case, toggle_word};
        // case preservation
        assert_eq!(match_case("TRUE", "false"), "FALSE");
        assert_eq!(match_case("True", "false"), "False");
        assert_eq!(match_case("true", "false"), "false");
        // both directions
        assert_eq!(toggle_word("true").as_deref(), Some("false"));
        assert_eq!(toggle_word("FALSE").as_deref(), Some("TRUE"));
        assert_eq!(toggle_word("Yes").as_deref(), Some("No"));
        assert_eq!(toggle_word("min").as_deref(), Some("max"));
        assert_eq!(toggle_word("disabled").as_deref(), Some("enabled"));
        // no opposite
        assert_eq!(toggle_word("banana"), None);
    }

    #[test]
    fn contrast_recommends_text() {
        use super::contrast_recommendation;
        // light background → black text
        assert!(contrast_recommendation("#ffffff")
            .unwrap()
            .contains("#000000"));
        // dark background → white text
        assert!(contrast_recommendation("#000000")
            .unwrap()
            .contains("#ffffff"));
        // a light yellow → black text
        assert!(contrast_recommendation("#ffff00")
            .unwrap()
            .contains("#000000"));
        assert_eq!(contrast_recommendation("notacolor"), None);
    }

    #[test]
    fn lighten_darken_colors() {
        use super::adjust_lightness;
        // mid-gray ±50%
        assert_eq!(
            adjust_lightness("#808080", 50, true).as_deref(),
            Some("#c0c0c0")
        );
        assert_eq!(
            adjust_lightness("#808080", 50, false).as_deref(),
            Some("#404040")
        );
        // black lightened 50% → mid-gray; white darkened 50% → mid-gray
        assert_eq!(
            adjust_lightness("#000000", 50, true).as_deref(),
            Some("#808080")
        );
        assert_eq!(
            adjust_lightness("#ffffff", 50, false).as_deref(),
            Some("#808080")
        );
        // short form accepted; non-hex → None
        assert_eq!(
            adjust_lightness("#fff", 0, true).as_deref(),
            Some("#ffffff")
        );
        assert_eq!(adjust_lightness("nope", 10, true), None);
    }

    #[test]
    fn sort_paragraphs_orders() {
        use super::sort_paragraphs;
        assert_eq!(sort_paragraphs("b\nb2\n\na\na2\n"), "a\na2\n\nb\nb2");
        // multiple blank lines between paragraphs are normalized to one
        assert_eq!(sort_paragraphs("z\n\n\n\na\n"), "a\n\nz");
        // single paragraph unchanged
        assert_eq!(sort_paragraphs("only\nlines"), "only\nlines");
    }

    #[test]
    fn unwrap_tag_strips() {
        use super::unwrap_tag;
        assert_eq!(unwrap_tag("<b>text</b>"), "text");
        assert_eq!(unwrap_tag("<div>a & b</div>"), "a & b");
        // not tag-wrapped → unchanged
        assert_eq!(unwrap_tag("plain"), "plain");
        // empty element
        assert_eq!(unwrap_tag("<br></br>"), "");
    }

    #[test]
    fn reverse_words_per_line() {
        use super::reverse_words;
        assert_eq!(reverse_words("the quick brown\n"), "brown quick the\n");
        // leading indentation preserved
        assert_eq!(reverse_words("  a b c"), "  c b a");
        // multiple lines
        assert_eq!(
            reverse_words("one two\nthree four\n"),
            "two one\nfour three\n"
        );
    }

    #[test]
    fn strip_quotes_unwraps() {
        use super::strip_quotes;
        assert_eq!(strip_quotes("\"hello\""), "hello");
        assert_eq!(strip_quotes("'x'"), "x");
        assert_eq!(strip_quotes("`code`"), "code");
        // not quoted, or mismatched → unchanged
        assert_eq!(strip_quotes("plain"), "plain");
        assert_eq!(strip_quotes("\"mismatch'"), "\"mismatch'");
        // a lone quote is not a pair
        assert_eq!(strip_quotes("\""), "\"");
    }

    #[test]
    fn swap_quotes_toggles() {
        use super::swap_quotes;
        assert_eq!(swap_quotes("'a' \"b\""), "\"a\" 'b'");
        assert_eq!(swap_quotes("print('hi')"), "print(\"hi\")");
        assert_eq!(swap_quotes("no quotes"), "no quotes");
    }

    #[test]
    fn thousands_separators() {
        use super::{add_thousands, strip_thousands};
        assert_eq!(add_thousands("1234567"), "1,234,567");
        assert_eq!(add_thousands("price: 1000000 yen"), "price: 1,000,000 yen");
        assert_eq!(add_thousands("12"), "12");
        assert_eq!(strip_thousands("1,234,567"), "1234567");
        // a comma not between digits is preserved
        assert_eq!(strip_thousands("a, b"), "a, b");
        // round-trip
        assert_eq!(strip_thousands(&add_thousands("9876543")), "9876543");
    }

    #[test]
    fn roman_numerals() {
        use super::{from_roman, to_roman};
        assert_eq!(to_roman(4).as_deref(), Some("IV"));
        assert_eq!(to_roman(2024).as_deref(), Some("MMXXIV"));
        assert_eq!(to_roman(0), None);
        assert_eq!(to_roman(4000), None);
        assert_eq!(from_roman("IV"), Some(4));
        assert_eq!(from_roman("mmxxiv"), Some(2024)); // case-insensitive
        assert_eq!(from_roman("foo"), None);
        // round-trip for a range of values
        for n in [1u32, 9, 49, 444, 1994, 3888] {
            assert_eq!(from_roman(&to_roman(n).unwrap()), Some(n));
        }
    }

    #[test]
    fn hex_rgb_conversions() {
        use super::{hex_to_rgb, rgb_to_hex};
        assert_eq!(hex_to_rgb("#ff8800").as_deref(), Some("rgb(255, 136, 0)"));
        // short #rgb form expands each nibble
        assert_eq!(hex_to_rgb("#f80").as_deref(), Some("rgb(255, 136, 0)"));
        assert_eq!(hex_to_rgb("not-a-color"), None);
        assert_eq!(rgb_to_hex("rgb(255, 136, 0)").as_deref(), Some("#ff8800"));
        // bare comma/space forms also parse
        assert_eq!(rgb_to_hex("255, 136, 0").as_deref(), Some("#ff8800"));
        assert_eq!(rgb_to_hex("12 34").as_deref(), None); // too few components
                                                          // round-trip
        assert_eq!(
            rgb_to_hex(&hex_to_rgb("#1e90ff").unwrap()).as_deref(),
            Some("#1e90ff")
        );
    }

    #[test]
    fn straighten_quotes_to_ascii() {
        use super::straighten_quotes;
        // curly double/single quotes → straight
        assert_eq!(
            straighten_quotes("\u{201C}hi\u{201D} it\u{2019}s"),
            "\"hi\" it's"
        );
        // em dash → -, ellipsis → ...
        assert_eq!(straighten_quotes("a\u{2014}b\u{2026}"), "a-b...");
        // non-breaking space → space; plain text untouched
        assert_eq!(straighten_quotes("x\u{00A0}y"), "x y");
        assert_eq!(straighten_quotes("plain 'text'"), "plain 'text'");
    }

    #[test]
    fn sentence_case_capitalizes() {
        use super::sentence_case;
        assert_eq!(
            sentence_case("hello world. how are you? fine."),
            "Hello world. How are you? Fine."
        );
        // acronyms / mid-sentence casing left untouched (non-destructive)
        assert_eq!(
            sentence_case("use the API now. it works."),
            "Use the API now. It works."
        );
        // already-capitalized stays
        assert_eq!(sentence_case("Hi."), "Hi.");
    }

    #[test]
    fn title_case_words() {
        use super::title_case;
        assert_eq!(title_case("hello world"), "Hello World");
        assert_eq!(title_case("the QUICK brown FOX"), "The Quick Brown Fox");
        // apostrophes stay within the word
        assert_eq!(title_case("don't stop"), "Don't Stop");
        // hyphens/underscores are word boundaries; punctuation preserved
        assert_eq!(title_case("foo-bar_baz"), "Foo-Bar_Baz");
        assert_eq!(title_case("a.b c"), "A.B C");
    }

    #[test]
    fn bullet_list_roundtrip() {
        use super::{bullet_list, unbullet};
        assert_eq!(bullet_list("a\nb\n"), "- a\n- b\n");
        // blank lines stay blank
        assert_eq!(bullet_list("x\n\ny\n"), "- x\n\n- y\n");
        // unbullet tolerates *, +, - bullets and leaves plain lines
        assert_eq!(unbullet("- a\n* b\n+ c\nplain"), "a\nb\nc\nplain");
        // round-trip
        assert_eq!(unbullet(&bullet_list("one\ntwo")), "one\ntwo");
    }

    #[test]
    fn blockquote_roundtrip() {
        use super::{blockquote, unblockquote};
        assert_eq!(blockquote("a\nb\n"), "> a\n> b\n");
        assert_eq!(unblockquote("> a\n> b\n"), "a\nb\n");
        // unblockquote tolerates a bare '>' and leaves unquoted lines alone
        assert_eq!(unblockquote(">x\nplain"), "x\nplain");
        // round-trip
        assert_eq!(unblockquote(&blockquote("hello\nworld")), "hello\nworld");
    }

    #[test]
    fn regex_escapes_metachars() {
        use super::regex_escape;
        assert_eq!(regex_escape("a.b(c)"), "a\\.b\\(c\\)");
        assert_eq!(regex_escape("1+1=2?"), "1\\+1=2\\?");
        assert_eq!(regex_escape("a\\b"), "a\\\\b"); // backslash itself escaped
        assert_eq!(regex_escape("plain text"), "plain text");
    }

    #[test]
    fn csv_to_json_objects() {
        use super::csv_to_json;
        assert_eq!(
            csv_to_json("name,age\nAlice,30\nBob,5"),
            "[\n  {\"name\": \"Alice\", \"age\": \"30\"},\n  {\"name\": \"Bob\", \"age\": \"5\"}\n]"
        );
        // fewer than 2 lines → unchanged
        assert_eq!(csv_to_json("just headers"), "just headers");
    }

    #[test]
    fn transpose_csv_grid() {
        use super::transpose_csv;
        assert_eq!(transpose_csv("a,b,c\n1,2,3"), "a,1\nb,2\nc,3");
        // tab-separated auto-detected (transposed rows joined by newline)
        assert_eq!(transpose_csv("x\ty\n1\t2"), "x\t1\ny\t2");
        // ragged rows padded with empties
        assert_eq!(transpose_csv("a,b\n1"), "a,1\nb,");
    }

    #[test]
    fn humanize_slugs() {
        use super::humanize;
        assert_eq!(humanize("foo-bar-baz"), "Foo Bar Baz");
        assert_eq!(humanize("my_file_name"), "My File Name");
        assert_eq!(humanize("hello"), "Hello");
        // mixed separators and extra dashes collapse
        assert_eq!(humanize("a--b_c"), "A B C");
    }

    #[test]
    fn slugify_text() {
        use super::slugify;
        assert_eq!(slugify("Hello, World!"), "hello-world");
        assert_eq!(slugify("  Foo   Bar  "), "foo-bar");
        assert_eq!(slugify("a_b.c"), "a-b-c");
        assert_eq!(slugify("Already-Slug"), "already-slug");
        assert_eq!(slugify("Section 1.2: Intro"), "section-1-2-intro");
        assert_eq!(slugify("!!!"), "");
    }

    #[test]
    fn find_all_ranges_works() {
        use super::find_all_ranges;
        assert_eq!(find_all_ranges("foo bar foo", "foo"), vec![(0, 3), (8, 11)]);
        // non-overlapping matches
        assert_eq!(find_all_ranges("aaaa", "aa"), vec![(0, 2), (2, 4)]);
        // multi-byte needle: ranges are in chars, not bytes
        assert_eq!(
            find_all_ranges("héllo héllo", "héllo"),
            vec![(0, 5), (6, 11)]
        );
        // no match / empty needle
        assert!(find_all_ranges("x", "y").is_empty());
        assert!(find_all_ranges("x", "").is_empty());
    }

    #[test]
    fn html_to_text_strips() {
        use super::strip_html;
        assert_eq!(
            strip_html("<b>Hello</b> &amp; <i>world</i>"),
            "Hello & world"
        );
        // attributes and self-closing tags removed
        assert_eq!(strip_html(r#"<a href="x">link</a><br/>end"#), "linkend");
        // entities decoded; plain text untouched
        assert_eq!(strip_html("5 &lt; 10"), "5 < 10");
        assert_eq!(strip_html("no tags here"), "no tags here");
    }

    #[test]
    fn html_encode_decode() {
        use super::{html_decode, html_encode};
        assert_eq!(html_encode("a<b>&\"'"), "a&lt;b&gt;&amp;&quot;&#39;");
        // decode inverts encode (the round-trip)
        assert_eq!(html_decode("a&lt;b&gt;&amp;&quot;&#39;"), "a<b>&\"'");
        // named, decimal, and hex numeric entities
        assert_eq!(html_decode("&amp; &#65; &#x42;"), "& A B");
        assert_eq!(html_decode("&apos;&nbsp;"), "'\u{a0}");
        // unknown or malformed entities are left verbatim
        assert_eq!(
            html_decode("100% &unknownentity; x"),
            "100% &unknownentity; x"
        );
        assert_eq!(html_decode("a & b"), "a & b");
        // ampersand-encoding must come first so it doesn't double-escape
        assert_eq!(
            html_decode(&html_encode("<a href=\"?x=1&y=2\">")),
            "<a href=\"?x=1&y=2\">"
        );
    }

    #[test]
    fn jwt_decodes() {
        use super::{base64url_encode, jwt_decode};
        let token = format!(
            "{}.{}.signature",
            base64url_encode(r#"{"alg":"HS256"}"#),
            base64url_encode(r#"{"sub":"42"}"#)
        );
        let decoded = jwt_decode(&token);
        assert!(decoded.starts_with("// header"));
        assert!(decoded.contains("\"alg\": \"HS256\"")); // pretty-printed
        assert!(decoded.contains("// payload"));
        assert!(decoded.contains("\"sub\": \"42\""));
        // not JWT-shaped → unchanged
        assert_eq!(jwt_decode("notajwt"), "notajwt");
    }

    #[test]
    fn base64url_roundtrip() {
        use super::{base64_decode, base64url_encode};
        // url-safe alphabet uses '-' (62) where standard uses '+'; no padding
        assert_eq!(base64url_encode(">>>"), "Pj4-");
        assert_eq!(base64url_encode("foob"), "Zm9vYg"); // no '=' padding
                                                        // decode accepts both alphabets (so it handles JWT/URL base64)
        assert_eq!(base64_decode("Pj4-"), ">>>");
        assert_eq!(base64_decode("Zm9vYg"), "foob");
        // round-trip including bytes that map to 62/63
        let s = "??>~ data";
        assert_eq!(base64_decode(&base64url_encode(s)), s);
    }

    #[test]
    fn base64_encode_decode() {
        use super::{base64_decode, base64_encode};
        // RFC 4648 test vectors
        assert_eq!(base64_encode(""), "");
        assert_eq!(base64_encode("f"), "Zg==");
        assert_eq!(base64_encode("fo"), "Zm8=");
        assert_eq!(base64_encode("foo"), "Zm9v");
        assert_eq!(base64_encode("foob"), "Zm9vYg==");
        assert_eq!(base64_encode("fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode("foobar"), "Zm9vYmFy");
        // decode inverts encode
        assert_eq!(base64_decode("Zg=="), "f");
        assert_eq!(base64_decode("Zm9vYmFy"), "foobar");
        // lenient: whitespace and stray padding are ignored
        assert_eq!(base64_decode("Zm9v\nYmFy"), "foobar");
        // round-trip including multi-byte UTF-8
        let s = "Hello, 世界! 🌍";
        assert_eq!(base64_decode(&base64_encode(s)), s);
    }

    #[test]
    fn url_info_breakdown() {
        use super::url_info;
        assert_eq!(
            url_info("https://api.example.com:8080/v1/users?name=John%20Doe&limit=10"),
            "scheme: https\nhost: api.example.com\nport: 8080\npath: /v1/users\nquery:\n  name=John Doe\n  limit=10"
        );
        // minimal URL: just host
        assert_eq!(
            url_info("http://example.com"),
            "scheme: http\nhost: example.com"
        );
        // no scheme, with path
        assert_eq!(
            url_info("example.com/path"),
            "host: example.com\npath: /path"
        );
    }

    #[test]
    fn query_string_builds() {
        use super::{build_query_string, parse_query_string};
        assert_eq!(
            build_query_string("name=John Doe\nage=30"),
            "name=John%20Doe&age=30"
        );
        // keys/values are trimmed before encoding; blank lines skipped
        assert_eq!(build_query_string(" a = 1 \n\n b = 2 "), "a=1&b=2");
        // round-trips with parse_query_string
        let q = "x=hello%20world&y=a%2Fb";
        assert_eq!(build_query_string(&parse_query_string(q)), q);
    }

    #[test]
    fn query_string_parses() {
        use super::parse_query_string;
        assert_eq!(
            parse_query_string("name=John%20Doe&age=30"),
            "name=John Doe\nage=30"
        );
        // leading '?' stripped, '+' decoded as space
        assert_eq!(parse_query_string("?q=a+b&x=1"), "q=a b\nx=1");
        // a key with no '=' is kept (decoded)
        assert_eq!(parse_query_string("flag&y=2"), "flag\ny=2");
        // empty pairs are skipped
        assert_eq!(parse_query_string("a=1&&b=2"), "a=1\nb=2");
    }

    #[test]
    fn percent_encode_decode_roundtrip() {
        use super::{percent_decode, percent_encode};
        // unreserved chars pass through; space/slash/colon get encoded
        assert_eq!(percent_encode("a b/c:d"), "a%20b%2Fc%3Ad");
        assert_eq!(percent_encode("safe-_.~"), "safe-_.~");
        // multi-byte UTF-8 encodes each byte
        assert_eq!(percent_encode("é"), "%C3%A9");
        // decode inverts encode
        assert_eq!(percent_decode("a%20b%2Fc%3Ad"), "a b/c:d");
        assert_eq!(percent_decode("%C3%A9"), "é");
        // invalid/truncated escapes are left verbatim
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz"), "%zz");
        // round-trip on a mixed string
        let s = "Hello, World! 路漫漫";
        assert_eq!(percent_decode(&percent_encode(s)), s);
    }

    #[test]
    fn duplicate_block_newline() {
        use super::duplicate_block_insert as d;
        // normal block (ends with newline): inserted verbatim, both directions
        assert_eq!(d("foo\n", true), "foo\n");
        assert_eq!(d("foo\n", false), "foo\n");
        assert_eq!(d("a\nb\n", true), "a\nb\n");
        // last line without trailing newline: a separator newline is synthesized
        assert_eq!(d("foo", true), "\nfoo");
        assert_eq!(d("foo", false), "foo\n");
    }

    #[test]
    fn move_line_reorder() {
        use super::{swap_block_down, swap_block_up};
        // normal interior swap: both lines keep their trailing newline
        assert_eq!(swap_block_down("B\n", "N\n"), "N\nB\n");
        assert_eq!(swap_block_up("B\n", "N\n"), "B\nN\n");
        // multi-line block moving down past a single neighbor
        assert_eq!(swap_block_down("B1\nB2\n", "N\n"), "N\nB1\nB2\n");
        // neighbor is the final line without an EOL: block must end EOL-less
        assert_eq!(swap_block_down("B\n", "N"), "N\nB");
        // block is the final line without an EOL, moving up: neighbor ends EOL-less
        assert_eq!(swap_block_up("B", "N\n"), "B\nN");
        // multi-line block moving up past a single neighbor
        assert_eq!(swap_block_up("B1\nB2\n", "N\n"), "B1\nB2\nN\n");
    }

    #[test]
    fn permalink_formats() {
        use super::build_permalink as p;
        assert_eq!(
            p("https://github.com/o/r", "abc123", "src/main.rs", 42),
            "https://github.com/o/r/blob/abc123/src/main.rs#L42"
        );
        // leading slash on the path is trimmed
        assert_eq!(
            p("https://gitlab.com/o/r", "deadbeef", "/lib/x.rs", 7),
            "https://gitlab.com/o/r/blob/deadbeef/lib/x.rs#L7"
        );
        // bitbucket uses a different anchor scheme
        assert_eq!(
            p("https://bitbucket.org/o/r", "feed", "a.py", 3),
            "https://bitbucket.org/o/r/src/feed/a.py#lines-3"
        );
    }
}

#[cfg(test)]
mod wildfire_tests {
    use super::{wildfire_grow_range, wildfire_range_strictly_contains};
    use zemacs_core::{Range, Rope};

    #[test]
    fn strictly_contains() {
        assert!(wildfire_range_strictly_contains(
            Range::new(0, 10),
            Range::new(2, 5)
        ));
        // Equal ranges are not strictly larger.
        assert!(!wildfire_range_strictly_contains(
            Range::new(2, 5),
            Range::new(2, 5)
        ));
        // Same end, larger start span -> contains.
        assert!(wildfire_range_strictly_contains(
            Range::new(1, 5),
            Range::new(2, 5)
        ));
        // Partial overlap is not containment.
        assert!(!wildfire_range_strictly_contains(
            Range::new(0, 4),
            Range::new(2, 5)
        ));
    }

    #[test]
    fn grow_strictly_through_nested_pairs() {
        // Indices: ( a   [ b ]   c )
        //          0 1 2 3 4 5 6 7 8
        let doc = Rope::from("(a [b] c)");
        let slice = doc.slice(..);

        // Start with the cursor on `b` (a point selection).
        let start = Range::point(4);

        // First plain <Enter>: grow to the inside of `[]` -> just "b".
        let g1 = wildfire_grow_range(None, slice, start, 1);
        assert_eq!(slice.slice(g1.from()..g1.to()), "b");
        assert!(wildfire_range_strictly_contains(g1, start));

        // Second plain <Enter>: from inside `[]` grow strictly to inside `()`.
        let g2 = wildfire_grow_range(None, slice, g1, 1);
        assert_eq!(slice.slice(g2.from()..g2.to()), "a [b] c");
        assert!(wildfire_range_strictly_contains(g2, g1));

        // Third plain <Enter>: no larger enclosing pair -> unchanged.
        let g3 = wildfire_grow_range(None, slice, g2, 1);
        assert_eq!(g3, g2);
    }

    #[test]
    fn count_jumps_to_nth_closest() {
        let doc = Rope::from("(a [b] c)");
        let slice = doc.slice(..);
        let start = Range::point(4);

        // N=2 jumps straight to the inside of the 2nd-closest pair `()`.
        let g = wildfire_grow_range(None, slice, start, 2);
        assert_eq!(slice.slice(g.from()..g.to()), "a [b] c");
    }
}
