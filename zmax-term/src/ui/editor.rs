use crate::{
    alt,
    commands::{self, OnKeyCallback, OnKeyCallbackKind},
    compositor::{Component, Context, Event, EventResult},
    events::{OnModeSwitch, PostCommand},
    handlers::completion::CompletionItem,
    key,
    keymap::{KeymapResult, Keymaps},
    ui::{
        document::{render_document, LinePos, TextRenderer},
        statusline,
        text_decorations::{self, Decoration, DecorationManager, InlineDiagnostics},
        Completion, ProgressSpinners,
    },
};

use std::{mem::take, num::NonZeroUsize, ops, path::PathBuf, rc::Rc};
use zmax_core::{
    diagnostic::NumberOrString,
    graphemes::{next_grapheme_boundary, prev_grapheme_boundary},
    movement::Direction,
    syntax::{self, OverlayHighlights},
    text_annotations::TextAnnotations,
    unicode::width::UnicodeWidthStr,
    visual_offset_from_block, Change, Position, Range, Selection,
};
use zmax_view::{
    annotations::diagnostics::DiagnosticFilter,
    document::{Mode, SCRATCH_BUFFER_NAME},
    editor::{CompleteAction, CursorShapeConfig, PrefixArg},
    graphics::{Color, CursorKind, Modifier, Rect, Style},
    input::{KeyEvent, MouseButton, MouseEvent, MouseEventKind},
    keyboard::{KeyCode, KeyModifiers},
    Document, Editor, Theme, View,
};

use tui::{buffer::Buffer as Surface, text::Span};

/// Bufferline tab hit regions: `(x_start, x_end, close_x, doc)` per tab.
type BufferlineTabs = Vec<(u16, u16, u16, zmax_view::DocumentId)>;

/// Sticky-scroll cache: `(doc, doc len, scopes)` where each scope is
/// `(start_line, end_line, header_text)`.
type StickyCache =
    std::cell::RefCell<Option<(zmax_view::DocumentId, usize, Vec<(usize, usize, String)>)>>;

// ── vim render-loop options ─────────────────────────────────────────────────
//
// `:set` options whose consumer is the render loop. Each is read from the option
// store at the point it changes what is drawn, so setting it takes effect on the
// next frame and leaving it unset keeps zmax's own behaviour.

/// vim `cmdheight`: rows at the bottom of the screen reserved for the command
/// line and its messages (default 1). `cmdheight=0` reserves none — the editor
/// gets the whole screen and a message, when there is one, draws over its last
/// row (as nvim's `cmdheight=0` does).
fn cmdheight() -> u16 {
    crate::commands::typed::vim_opt_num("cmdheight")
        .unwrap_or(1)
        .min(16) as u16
}

/// One entry of vim `fillchars` (`vert:│,eob:~,fold:·`): the character named for
/// `item`, or `None` when the option doesn't name it. Pure — unit tested.
fn parse_fillchar(value: &str, item: &str) -> Option<char> {
    value.split(',').find_map(|pair| {
        let (name, ch) = pair.trim().split_once(':')?;
        (name.trim() == item).then(|| ch.chars().next()).flatten()
    })
}

/// The `fillchars` character for `item` as currently `:set`.
fn fillchar(item: &str) -> Option<char> {
    parse_fillchar(&crate::commands::typed::vim_opt_str("fillchars")?, item)
}

/// vim `showcmdloc`: which row the pending command is drawn on — `last` (the
/// command line, the default), `statusline` (the focused window's status line) or
/// `tabline` (the top bar). Pure — unit tested.
fn showcmd_row(loc: &str, cmdline: u16, statusline: u16, tabline: u16) -> u16 {
    match loc {
        "statusline" => statusline,
        "tabline" => tabline,
        _ => cmdline,
    }
}

/// The document facts a vim bar format (`winbar`, `tabline`) can name.
struct BarContext {
    path: String,
    name: String,
    modified: bool,
    readonly: bool,
    filetype: String,
    line: usize,
    lines: usize,
    col: usize,
}

/// Expand a vim statusline-format string (`winbar` / `tabline`) into its left and
/// right halves (`%=` separates them).
///
/// Supported items: `%f`/`%F` (path), `%t` (file name), `%m`/`%M` (modified),
/// `%r` (read-only), `%y`/`%Y` (filetype), `%l`/`%L` (line / line count), `%c`
/// (column), `%=` (split) and `%%` (a literal `%`). vim's `%{expr}` calls out to
/// vimscript, which does not run in the render loop, so those are dropped rather
/// than faked. Pure — unit tested.
fn vim_bar_expand(fmt: &str, cx: &BarContext) -> (String, String) {
    let mut left = String::new();
    let mut right = String::new();
    let mut split = false;
    let mut chars = fmt.chars().peekable();
    while let Some(c) = chars.next() {
        let out = if split { &mut right } else { &mut left };
        if c != '%' {
            out.push(c);
            continue;
        }
        // Skip a width/alignment prefix (`%-0.10f`), which zmax doesn't pad by.
        while chars
            .peek()
            .is_some_and(|c| c.is_ascii_digit() || matches!(c, '-' | '.'))
        {
            chars.next();
        }
        match chars.next() {
            Some('f') | Some('F') => out.push_str(&cx.path),
            Some('t') => out.push_str(&cx.name),
            Some('m') => out.push_str(if cx.modified { "[+]" } else { "" }),
            Some('M') => out.push_str(if cx.modified { "+" } else { "" }),
            Some('r') => out.push_str(if cx.readonly { "[RO]" } else { "" }),
            Some('y') => {
                if !cx.filetype.is_empty() {
                    out.push_str(&format!("[{}]", cx.filetype));
                }
            }
            Some('Y') => out.push_str(&cx.filetype),
            Some('l') => out.push_str(&cx.line.to_string()),
            Some('L') => out.push_str(&cx.lines.to_string()),
            Some('c') => out.push_str(&cx.col.to_string()),
            Some('=') => split = true,
            Some('%') => out.push('%'),
            // `%{expr}` / `%#Highlight#`: consume the item, render nothing.
            Some('{') => {
                for c in chars.by_ref() {
                    if c == '}' {
                        break;
                    }
                }
            }
            Some('#') => {
                for c in chars.by_ref() {
                    if c == '#' {
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    (left, right)
}

/// vim `foldtext`: the summary a closed fold's line shows in place of its
/// hidden body.
///
/// A literal value is used as-is. An empty value or a function call
/// (`foldtext()`, `MyFoldText()`) names a vimscript function the render loop
/// cannot call, so the fold falls back to the text vim's own default
/// `foldtext()` produces: `+-`, one dash per fold nesting `level`, the `lines`
/// count right-justified to width 3 (vim's `%3ld`), then the fold's first line
/// already cleaned of comment leaders and `{{{`/`}}}` markers (see
/// [`clean_fold_line`]). Pure — unit tested.
fn fold_text(value: &str, lines: usize, level: usize, cleaned_first_line: &str) -> String {
    let value = value.trim();
    if !value.is_empty() && !value.ends_with(')') {
        return value.to_string();
    }
    let dashes = "-".repeat(level.max(1));
    format!("+-{dashes}{lines:>3} lines: {cleaned_first_line}")
}

/// Reduce a fold's first line to what vim's `foldtext()` displays: drop leading
/// whitespace, comment leaders, and `{{{`/`}}}` fold markers (with any trailing
/// level digits), in any order, then trim. So `#{{{  MARK:Header` shows as
/// `MARK:Header`. Pure — unit tested.
fn clean_fold_line(first_line: &str, comment_tokens: &[String]) -> String {
    let mut s = first_line.trim();
    loop {
        let start = s;
        s = s.trim_start();
        for marker in ["{{{", "}}}"] {
            if let Some(rest) = s.strip_prefix(marker) {
                s = rest.trim_start_matches(|c: char| c.is_ascii_digit());
            }
        }
        for tok in comment_tokens {
            if let Some(rest) = s.strip_prefix(tok.as_str()) {
                s = rest;
            }
        }
        if s == start {
            break;
        }
    }
    s.trim().to_string()
}

/// Split a status message over the rows vim `cmdheight` gave the command line:
/// at most `rows` chunks of at most `width` columns. With the default
/// `cmdheight=1` this is the one (possibly cut off) line zmax always drew.
/// Pure — unit tested.
fn wrap_message(msg: &str, width: usize, rows: usize) -> Vec<String> {
    if width == 0 || rows == 0 {
        return Vec::new();
    }
    let mut out: Vec<String> = Vec::new();
    let mut line = String::new();
    let mut col = 0;
    for g in msg.chars() {
        if col == width {
            out.push(std::mem::take(&mut line));
            col = 0;
            if out.len() == rows {
                return out;
            }
        }
        line.push(g);
        col += 1;
    }
    if !line.is_empty() {
        out.push(line);
    }
    out
}

/// vim `redrawtime`: whether a viewport highlight pass has run past the budget
/// the option gives it (in milliseconds) and must stop. No budget — the option
/// unset — never stops. Pure — unit tested.
fn over_redrawtime(elapsed_ms: u128, budget_ms: Option<usize>) -> bool {
    budget_ms.is_some_and(|budget| elapsed_ms > budget as u128)
}

/// vim `spelloptions=camel`: split a word at its internal capitals, so each part
/// of `fooBarBaz` is spell-checked on its own. Returns `(offset, part)` pairs
/// relative to the word's start. Pure — unit tested.
fn camel_parts(word: &[char]) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut start = 0;
    for i in 1..word.len() {
        // A capital that follows a lowercase letter opens a new part.
        if word[i].is_uppercase() && word[i - 1].is_lowercase() {
            out.push((start, word[start..i].iter().collect()));
            start = i;
        }
    }
    out.push((start, word[start..].iter().collect()));
    out
}

/// vim `spellcapcheck`: the characters that end a sentence, taken from the
/// option's leading character class (the default is `[.?!]\_[\])'" \t]\+`). An
/// empty option turns the capitalization check off. Pure — unit tested.
fn spellcap_end_chars(value: &str) -> Vec<char> {
    let value = value.trim();
    if value.is_empty() {
        return Vec::new();
    }
    match (value.find('['), value.find(']')) {
        (Some(open), Some(close)) if close > open + 1 => value[open + 1..close].chars().collect(),
        // Not a character class (a plain pattern): take the sentence-ending
        // punctuation it names, and nothing else (a `\.` is a full stop, not a
        // backslash).
        _ => value
            .chars()
            .filter(|c| matches!(c, '.' | '?' | '!' | ';' | ':'))
            .collect(),
    }
}

pub struct EditorView {
    pub keymaps: Keymaps,
    on_next_key: Option<(OnKeyCallback, OnKeyCallbackKind)>,
    pseudo_pending: Vec<KeyEvent>,
    /// Ring of the most recently pressed keys, for `view-lossage` (C-h l).
    pub recent_keys: std::collections::VecDeque<KeyEvent>,
    /// `gud-tooltip-mode`: the identifier the pointer last showed a value for,
    /// so a pointer that stays on the same word does not re-query the adapter.
    gud_tooltip_word: Option<String>,
    pub(crate) last_insert: (commands::MappableCommand, Vec<InsertEvent>),
    pub(crate) completion: Option<Completion>,
    spinners: ProgressSpinners,
    /// Tracks if the terminal window is focused by reaction to terminal focus events
    terminal_focused: bool,
    /// vim dot-repeat (`.`): the key sequence of the last buffer-changing command
    /// in normal/select mode, including any insert session that followed it.
    last_change: Vec<KeyEvent>,
    /// The count that prefixed `last_change` (vim reuses it when `.` is pressed
    /// without an explicit count: `2x` then `.` deletes two, not one). `1` when
    /// the change had no count.
    last_change_count: usize,
    /// Count captured at the start of the in-progress change, promoted to
    /// `last_change_count` alongside `change_buf`.
    change_count: usize,
    /// Keys accumulated for the in-progress command; promoted to `last_change`
    /// once the command modifies the buffer (or after the insert session it began).
    change_buf: Vec<KeyEvent>,
    /// True while recording an insert session that began as a change, so the typed
    /// keys join the change recording.
    recording_insert_change: bool,
    /// Guard set while replaying a change for `.`, so the replay isn't re-recorded.
    replaying: bool,
    /// vim operator count: the count typed *before* an operator (`2` in `2d3w`),
    /// snapshotted when the operator enters pending state so the count after the
    /// operator (`3`) starts fresh. The effective count is the product
    /// (`2 * 3 = 6`), matching vim. `None` outside an operator-pending sequence.
    operator_count: Option<NonZeroUsize>,
    /// IDE workbench (file tree + structure + problems + error stripe). None until opened.
    ide: Option<Ide>,
    /// Persisted IDE layout (widths, folds, collapse/hide state) from the last
    /// session, applied whenever the workbench is (re)created so `:ide` and friends
    /// restore the user's arrangement instead of starting from defaults.
    ide_layout: crate::appdata::IdeLayout,
    /// Name of the most-recently-focused workbench tool window, for JetBrains
    /// "Jump to Last Tool Window" (toggle focus between editor and this panel).
    last_ide_panel: String,
    /// Tab strip hit regions `(x_start, x_end, doc)` and its row, for click-to-switch.
    bufferline_tabs: BufferlineTabs,
    /// `(x_start, x_end)` of the trailing `+` new-buffer button.
    bufferline_new: (u16, u16),
    bufferline_y: u16,
    /// Active split-divider drag: `(view, vertical_divider, grab_offset)`.
    /// `vertical_divider` is true for a left/right border (resize width) and false
    /// for a top/bottom border (resize height). `grab_offset` is the signed
    /// distance between where the mouse first grabbed and the divider's actual
    /// edge, so the divider tracks the cursor *absolutely* (no incremental drift)
    /// while preserving where on the divider the user grabbed.
    resize_drag: Option<(zmax_view::ViewId, bool, i16)>,
    /// Sticky-scroll cache: `(doc, doc len, scopes)` where each scope is
    /// `(start_line, end_line, header_text)`. Recomputed only when the focused
    /// document's length changes, so scrolling stays cheap.
    sticky_cache: StickyCache,
    /// The window whose mode line (status line) the middle/right button was last
    /// pressed on. emacs's `mouse-delete-window` / `mouse-delete-other-windows`
    /// act only when the press and the release are on the same window's mode
    /// line, so the press has to be remembered until the click completes.
    mode_line_press: Option<zmax_view::ViewId>,
    /// spacemacs `nav-flash`: the line the cursor landed on after a navigation
    /// command, flashed for `nav-flash-delay` (0.5s) then expired.
    /// `(document, line, armed)`.
    nav_flash: Option<(zmax_view::DocumentId, usize, std::time::Instant)>,
    /// `nav-flash--last-point`: the window/buffer/point of the previous flash.
    /// A trigger that leaves the cursor exactly where it already was does not
    /// flash again (`nav-flash/blink-cursor-maybe`).
    nav_flash_last: Option<(zmax_view::ViewId, zmax_view::DocumentId, usize)>,
    /// emacs `menu-bar-mode`: the menu-bar row's hit regions, `(x_start, x_end,
    /// menu index)`, and the row they were drawn on. Filled every frame the row
    /// is drawn so a click can find which title it landed on.
    menu_bar_hits: Vec<(u16, u16, usize)>,
    menu_bar_y: u16,
    /// emacs `tool-bar-mode`: the tool-bar row's hit regions, `(x_start, x_end,
    /// button index)`, and its row.
    tool_bar_hits: Vec<(u16, u16, usize)>,
    tool_bar_y: u16,
    /// emacs `modifier-bar-mode`: the modifier-bar row's hit regions and its row.
    modifier_bar_hits: Vec<(u16, u16, usize)>,
    modifier_bar_y: u16,
    /// The window whose vertical scroll bar `mouse-1` is currently dragging.
    /// Emacs scrolls the window continuously while the button is held.
    scroll_bar_drag: Option<zmax_view::ViewId>,
    /// vim 'mousetime': where and when the last left button press landed, so the
    /// next one can be recognized as the second/third/fourth click of a multi
    /// click (`<2-LeftMouse>`, `<3-LeftMouse>`, `<4-LeftMouse>`).
    /// `(column, row, when)`.
    last_click: Option<(u16, u16, std::time::Instant)>,
    /// How many clicks in a row have landed on `last_click`'s cell: 1 for a plain
    /// click, up to 4. vim never counts past 4 — the fifth click starts over
    /// (`orig_num_clicks != 4` in its increment condition, src/mouse.c).
    click_count: u8,
}

use super::ide::{Ide, IdeAction};

#[derive(Debug, Clone)]
#[allow(dead_code)] // payload consumed via pattern matches; fields read situationally
pub enum InsertEvent {
    Key(KeyEvent),
    CompletionApply {
        trigger_offset: usize,
        changes: Vec<Change>,
    },
    TriggerCompletion,
    RequestCompletion,
}

/// vim operator × motion count product: `2d3w` → `2 * 3 = 6`. An absent side
/// counts as 1; both absent yields `None` (no count at all, so commands fall back
/// to their own default of 1). Caps at the same ceiling the count parser uses.
fn combine_counts(op: Option<NonZeroUsize>, motion: Option<NonZeroUsize>) -> Option<NonZeroUsize> {
    match (op, motion) {
        (None, None) => None,
        _ => {
            let product = op
                .map_or(1, NonZeroUsize::get)
                .saturating_mul(motion.map_or(1, NonZeroUsize::get))
                .min(100_000_000);
            NonZeroUsize::new(product)
        }
    }
}

/// spacemacs `nav-flash`: `nav-flash-delay`, how long the line containing the
/// point stays highlighted after a navigation command.
const NAV_FLASH_DELAY: std::time::Duration = std::time::Duration::from_millis(500);

/// The commands the `nav-flash` layer advises with `nav-flash/blink-cursor-maybe`
/// (`layers/+misc/nav-flash/packages.el`), mapped onto their zmax equivalents:
/// `scroll-up-command`/`scroll-down-command`, `recenter-top-bottom`,
/// `other-window`, `winum-select-window-by-number`, `pop-tag-mark` (and the rest
/// of `better-jumper-post-jump-hook`), `spacemacs/alternate-buffer`,
/// `evil-window-top`/`-middle`/`-bottom` and `what-cursor-position`.
const NAV_FLASH_COMMANDS: &[&str] = &[
    "page_up",
    "page_down",
    "align_view_top",
    "align_view_center",
    "align_view_middle",
    "align_view_bottom",
    "rotate_view",
    "rotate_view_reverse",
    "goto_window_1",
    "goto_window_2",
    "goto_window_3",
    "goto_window_4",
    "goto_window_5",
    "goto_window_6",
    "goto_window_7",
    "goto_window_8",
    "goto_window_9",
    "jump_backward",
    "jump_forward",
    "goto_last_accessed_file",
    "goto_last_modified_file",
    "goto_window_top",
    "goto_window_center",
    "goto_window_bottom",
    "what_cursor_position",
];

impl EditorView {
    pub fn new(keymaps: Keymaps) -> Self {
        Self {
            keymaps,
            on_next_key: None,
            pseudo_pending: Vec::new(),
            recent_keys: std::collections::VecDeque::new(),
            gud_tooltip_word: None,
            last_insert: (commands::MappableCommand::normal_mode, Vec::new()),
            completion: None,
            spinners: ProgressSpinners::default(),
            terminal_focused: true,
            last_change: Vec::new(),
            last_change_count: 1,
            change_count: 1,
            change_buf: Vec::new(),
            recording_insert_change: false,
            replaying: false,
            operator_count: None,
            ide: None,
            ide_layout: crate::appdata::IdeLayout::default(),
            last_ide_panel: String::from("project"),
            bufferline_tabs: Vec::new(),
            bufferline_new: (0, 0),
            bufferline_y: 0,
            resize_drag: None,
            sticky_cache: std::cell::RefCell::new(None),
            mode_line_press: None,
            nav_flash: None,
            nav_flash_last: None,
            menu_bar_hits: Vec::new(),
            menu_bar_y: 0,
            tool_bar_hits: Vec::new(),
            tool_bar_y: 0,
            modifier_bar_hits: Vec::new(),
            modifier_bar_y: 0,
            scroll_bar_drag: None,
            last_click: None,
            click_count: 0,
        }
    }

    /// Refresh the IDE file tree from disk (invoked by the filesystem watcher).
    pub fn refresh_file_tree(&mut self) {
        if let Some(ide) = &mut self.ide {
            ide.refresh_tree();
        }
    }

    /// Get the IDE workbench, creating it if absent. On first creation the
    /// persisted layout (widths, folds, collapse/hide state) is applied, so every
    /// entry point (`:ide`, toggle, reveal, panel focus, …) restores the user's
    /// last arrangement instead of starting from defaults.
    fn ide_or_create(&mut self) -> &mut Ide {
        if self.ide.is_none() {
            let mut ide = Ide::new();
            ide.apply_layout(&self.ide_layout);
            self.ide = Some(ide);
        }
        self.ide.as_mut().unwrap()
    }

    /// Store the IDE layout persisted from the previous session so it's applied
    /// the next time the workbench is opened.
    pub fn set_ide_layout(&mut self, layout: crate::appdata::IdeLayout) {
        self.ide_layout = layout;
    }

    /// Boot the IDE workbench, editor focused (the `zmax --ide` entry point).
    pub fn open_sidebar(&mut self) {
        self.ide_or_create().focus_editor();
    }

    /// Reveal a file path in the project tree (creates the workbench if needed).
    pub fn reveal_in_tree(&mut self, path: &std::path::Path) {
        self.ide_or_create().reveal(path);
    }

    /// Focus a workbench panel by name (creates the workbench if needed).
    pub fn focus_ide_panel(&mut self, name: &str) {
        self.last_ide_panel = name.to_string();
        self.ide_or_create().focus_panel(name);
    }

    /// JetBrains "Hide Active Tool Window" (Shift-Esc): return focus to the
    /// editor, defocusing whatever tool window was active.
    pub fn hide_active_tool_window(&mut self) {
        if let Some(ide) = self.ide.as_mut() {
            ide.focus_editor();
        }
    }

    /// JetBrains "Jump to Last Tool Window" (F12): toggle focus between the
    /// editor and the most-recently-focused tool window.
    pub fn jump_to_last_tool_window(&mut self) {
        let last = self.last_ide_panel.clone();
        match self.ide.as_mut() {
            // A tool window currently has focus -> go back to the editor.
            Some(ide) if ide.visible() => ide.focus_editor(),
            // Editor has focus -> jump to the last-used tool window.
            Some(ide) => ide.focus_panel(&last),
            None => self.ide_or_create().focus_panel(&last),
        }
    }

    /// Toggle "always select opened file" (auto-reveal the current buffer in tree).
    pub fn toggle_auto_reveal(&mut self, cx: &mut crate::compositor::Context) {
        let on = self.ide_or_create().toggle_auto_reveal();
        cx.editor.set_status(if on {
            "Always select opened file: on"
        } else {
            "Always select opened file: off"
        });
    }

    /// Jump to the next / previous `file:line` in the run output (error nav).
    pub fn goto_run_error(&mut self, cx: &mut crate::compositor::Context, forward: bool) {
        let action = match self.ide.as_mut() {
            Some(ide) => ide.goto_run_error(forward),
            None => super::ide::IdeAction::None,
        };
        match action {
            super::ide::IdeAction::None => {
                cx.editor
                    .set_status("No file:line references in run output");
            }
            other => {
                let _ = self.apply_ide_action(other, cx);
            }
        }
    }

    /// Emacs `compilation-next-file` / `compilation-previous-file`: jump to the
    /// first error of the next / previous file named in the run output.
    pub fn goto_run_error_file(&mut self, cx: &mut crate::compositor::Context, forward: bool) {
        let action = match self.ide.as_mut() {
            Some(ide) => ide.goto_run_error_file(forward),
            None => super::ide::IdeAction::None,
        };
        match action {
            super::ide::IdeAction::None => {
                cx.editor
                    .set_status("No other file with errors in the run output");
            }
            other => {
                let _ = self.apply_ide_action(other, cx);
            }
        }
    }

    /// Emacs `compile-goto-error`: visit the error the run output is parked on.
    pub fn goto_current_run_error(&mut self, cx: &mut crate::compositor::Context) {
        let action = match self.ide.as_mut() {
            Some(ide) => ide.goto_current_run_error(),
            None => super::ide::IdeAction::None,
        };
        match action {
            super::ide::IdeAction::None => {
                cx.editor
                    .set_status("No file:line references in run output");
            }
            other => {
                let _ = self.apply_ide_action(other, cx);
            }
        }
    }

    /// Emacs `kill-compilation`: SIGTERM the running compile / run process.
    /// Reports whether there was one to kill.
    pub fn kill_active_run(&mut self, cx: &mut crate::compositor::Context) {
        let running = self
            .ide
            .as_ref()
            .and_then(|ide| ide.run_running())
            .unwrap_or(false);
        if !running {
            cx.editor.set_status("kill-compilation: nothing is running");
            return;
        }
        self.stop_active_run();
        cx.editor.set_status("kill-compilation: sent SIGTERM");
    }

    /// Toggle maximizing the bottom panel (read long logs/diffs full-height).
    pub fn toggle_bottom_zoom(&mut self, cx: &mut crate::compositor::Context) {
        let on = self.ide_or_create().toggle_bottom_zoom();
        cx.editor.set_status(if on {
            "Bottom panel maximized (toggle to restore)"
        } else {
            "Bottom panel restored"
        });
    }

    pub fn toggle_drawer_mid(&mut self, cx: &mut crate::compositor::Context) {
        let folded = self.ide_or_create().toggle_mid_fold();
        cx.editor.set_status(if folded {
            "Middle drawer column folded"
        } else {
            "Middle drawer column shown"
        });
    }

    /// Re-run the last command (status hint when there's nothing to re-run).
    pub fn rerun_last_run(&mut self, cx: &mut crate::compositor::Context) {
        let ok = self.ide.as_mut().is_some_and(Ide::rerun_last);
        if !ok {
            cx.editor.set_status("No previous run to re-run");
        }
    }

    /// Stop the active run (Run-console context menu / toolbar Stop).
    pub fn stop_active_run(&mut self) {
        if let Some(ide) = self.ide.as_mut() {
            ide.stop_run();
        }
    }

    /// Toggle a workbench panel's fold state (context-menu "Fold").
    pub fn ide_toggle_fold(&mut self, which: &str) {
        if let Some(ide) = self.ide.as_mut() {
            ide.toggle_fold_panel(which);
        }
    }

    /// Clear the Run console output (no-op with a status hint when nothing ran).
    pub fn clear_run_output(&mut self, cx: &mut crate::compositor::Context) {
        let cleared = self.ide.as_mut().is_some_and(Ide::clear_run);
        if !cleared {
            cx.editor.set_status("No run output to clear");
        }
    }

    /// Toggle the IDE workbench on/off (Zen / focus mode). Creates the workbench
    /// on first use; thereafter flips its visibility, reclaiming the full screen
    /// for distraction-free editing and restoring the panels on the next toggle.
    pub fn toggle_ide(&mut self) {
        match &mut self.ide {
            Some(ide) => ide.toggle_visible(),
            None => {
                self.ide_or_create().focus_editor();
            }
        }
    }

    /// JetBrains "Stretch to …": resize the workbench drawers from the
    /// keyboard. False when there is no visible workbench to resize.
    pub fn stretch_ide(&mut self, dir: crate::ui::StretchDir) -> bool {
        match &mut self.ide {
            Some(ide) => ide.stretch(dir, 4),
            None => false,
        }
    }

    /// Attach a running command to the IDE Run tool window (opens + focuses it).
    pub fn set_run(&mut self, run: crate::ui::run::Run) {
        self.ide_or_create().set_run(run);
    }

    /// Snapshot the IDE workbench layout for persistence (None if never opened).
    pub fn ide_layout(&self) -> Option<crate::appdata::IdeLayout> {
        self.ide.as_ref().map(Ide::layout)
    }

    /// Render the workbench (if any) into its regions; return the editor's remaining area.
    fn render_sidebar(
        &mut self,
        area: Rect,
        surface: &mut Surface,
        cx: &mut crate::compositor::Context,
    ) -> Rect {
        match self.ide.as_mut() {
            Some(ide) => ide.render(area, surface, cx),
            None => area,
        }
    }

    /// Apply a workbench action: open a file, jump to a symbol/diagnostic, or run/debug.
    /// Returns a compositor callback when the action needs to push UI (e.g. the debug picker).
    /// The file-tree right-click "Run" action: materialize a JetBrains-style run
    /// configuration for `path` (auto-detected command + dir), make it the active
    /// config, then run it in the Run tool window.
    pub fn run_path(&mut self, editor: &mut Editor, path: &std::path::Path) {
        let (cmd, cwd) = crate::ui::run::smart_command(Some(path));
        let root = zmax_loader::find_workspace().0;
        let dir = cwd
            .strip_prefix(&root)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| cwd.to_string_lossy().into_owned());
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Run".to_string());
        let cfg = crate::run_config::upsert_active(name, cmd, dir);
        let shell = editor.config().shell.clone();
        let run = crate::ui::run::spawn(
            cfg.command.clone(),
            shell,
            crate::run_config::resolve_dir(&cfg.dir),
        );
        self.ide_or_create().set_run(run);
        editor.set_status(format!("run config '{}' created — running", cfg.name));
    }

    /// Run the active named configuration (or auto-detect a command when none is set).
    /// Shared by the Run toolbar button, the run keybinding, and the config manager.
    pub fn run_active(&mut self, context: &mut crate::compositor::Context) {
        match crate::run_config::active() {
            Some(c) if !c.command.trim().is_empty() => {
                let env_prefix: String = c
                    .env
                    .lines()
                    .map(str::trim)
                    .filter(|l| !l.is_empty() && l.contains('='))
                    .map(|l| format!("{l} "))
                    .collect();
                let cmd = format!("{env_prefix}{}", c.command);
                let cwd = crate::run_config::resolve_dir(&c.dir);
                self.start_run(context, cmd, cwd);
            }
            // No active config: JetBrains auto-creates one when you run a file, so
            // materialize + activate a config for the current file, then run it.
            _ => {
                let path = doc!(context.editor).path().map(|p| p.to_path_buf());
                match path {
                    Some(p) => self.run_path(context.editor, &p),
                    None => {
                        let (cmd, cwd) = crate::ui::run::smart_command(None);
                        self.start_run(context, cmd, cwd);
                    }
                }
            }
        }
    }

    /// Spawn `cmd` in `cwd` and show it in the Run tool window. Shared by the Run
    /// toolbar button, the active run-configuration, and the run-config manager.
    pub fn start_run(
        &mut self,
        context: &mut crate::compositor::Context,
        cmd: String,
        cwd: std::path::PathBuf,
    ) {
        self.start_run_with_editor(context.editor, cmd, cwd);
    }

    /// [`Self::start_run`] for callers that only hold an [`Editor`] — the job
    /// callbacks, which run with `(&mut Editor, &mut Compositor)` and so cannot
    /// build a `compositor::Context`.
    pub fn start_run_with_editor(&mut self, editor: &Editor, cmd: String, cwd: std::path::PathBuf) {
        let shell = editor.config().shell.clone();
        let run = crate::ui::run::spawn(cmd, shell, cwd);
        self.ide_or_create().set_run(run);
    }

    fn apply_ide_action(
        &mut self,
        action: IdeAction,
        context: &mut crate::compositor::Context,
    ) -> Option<crate::compositor::Callback> {
        match action {
            IdeAction::None => None,
            IdeAction::OpenFile(path) => {
                // A binary file (e.g. .zwc) opens in the hex editor instead of
                // being silently rejected — same routing as :open / the pickers.
                if let Err(zmax_view::DocumentOpenError::BinaryFile) = context
                    .editor
                    .open(&path, zmax_view::editor::Action::Replace)
                {
                    // push_hex_view replaces any existing hex overlay.
                    crate::commands::typed::push_hex_view(context, path);
                    None
                } else {
                    // Opened as text — dismiss any hex overlay left over from a
                    // previously-opened binary file so the new buffer is visible.
                    Some(Box::new(|compositor, _cx| {
                        compositor.remove("hex");
                    }))
                }
            }
            IdeAction::OpenUrl(url) => {
                let _ = open::that(&url);
                context.editor.set_status(format!("opened {url}"));
                None
            }
            IdeAction::OpenFileAt { path, line } => {
                let opened = context
                    .editor
                    .open(&path, zmax_view::editor::Action::Replace)
                    .is_ok();
                if opened {
                    let scrolloff = context.editor.config().scrolloff;
                    let (view, doc) = current!(context.editor);
                    let text = doc.text();
                    let li = line
                        .saturating_sub(1)
                        .min(text.len_lines().saturating_sub(1));
                    let pos = text.line_to_char(li);
                    doc.set_selection(view.id, Selection::point(pos));
                    view.ensure_cursor_in_view(doc, scrolloff);
                }
                None
            }
            IdeAction::Goto { from, to } => {
                let scrolloff = context.editor.config().scrolloff;
                let (view, doc) = current!(context.editor);
                doc.set_selection(view.id, super::ide::goto_selection(from, to));
                view.ensure_cursor_in_view(doc, scrolloff);
                None
            }
            IdeAction::PasteRegister(ch) => {
                // Read the register's real contents (not the truncated tab preview).
                let text: String = context
                    .editor
                    .registers
                    .read(ch, context.editor)
                    .map(|vals| vals.map(|v| v.into_owned()).collect::<Vec<_>>().join("\n"))
                    .unwrap_or_default();
                if !text.is_empty() {
                    let (view, doc) = current!(context.editor);
                    let sel = doc.selection(view.id).clone();
                    let tx = zmax_core::Transaction::insert(doc.text(), &sel, text.into());
                    doc.apply(&tx, view.id);
                }
                None
            }
            IdeAction::RunStart => {
                self.run_active(context);
                None
            }
            IdeAction::GitDiff(path) => {
                let cwd = path
                    .parent()
                    .map(|p| p.to_path_buf())
                    .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
                let cmd = format!("git diff HEAD -- '{}'", path.display());
                self.start_run(context, cmd, cwd);
                None
            }
            IdeAction::CopyText(text) => {
                let n = text.lines().count();
                let _ = context.editor.registers.write('+', vec![text]);
                context
                    .editor
                    .set_status(format!("Copied {n} lines to clipboard"));
                None
            }
            IdeAction::GitPush => {
                let cwd = std::env::current_dir().unwrap_or_default();
                self.start_run(context, "git push".to_string(), cwd);
                None
            }
            IdeAction::GitPull => {
                let cwd = std::env::current_dir().unwrap_or_default();
                self.start_run(context, "git pull --ff-only".to_string(), cwd);
                None
            }
            IdeAction::GitFetch => {
                let cwd = std::env::current_dir().unwrap_or_default();
                self.start_run(context, "git fetch --all --prune".to_string(), cwd);
                None
            }
            IdeAction::GitStash => {
                crate::commands::typed::git_stash_action(context, false);
                None
            }
            IdeAction::GitStashPop => {
                crate::commands::typed::git_stash_action(context, true);
                None
            }
            IdeAction::GitBranchPicker => {
                // List branches into the status line, then prompt for the target —
                // a Prompt pushed from here works reliably (unlike a Picker, whose
                // matcher isn't spawned via this path). `:git-branch-picker` gives
                // the fuzzy picker from the command palette.
                let dir = std::env::current_dir().unwrap_or_default();
                let branches = std::process::Command::new("git")
                    .arg("-C")
                    .arg(&dir)
                    .args(["for-each-ref", "--format=%(refname:short)", "refs/heads/"])
                    .output()
                    .ok()
                    .filter(|o| o.status.success())
                    .map(|o| {
                        String::from_utf8_lossy(&o.stdout)
                            .split_whitespace()
                            .collect::<Vec<_>>()
                            .join(" ")
                    })
                    .unwrap_or_default();
                context.editor.set_status(format!("branches: {branches}"));
                Some(Box::new(|compositor, _cx| {
                    let prompt = crate::ui::Prompt::new(
                        "checkout branch: ".into(),
                        None,
                        |_e, _i| Vec::new(),
                        move |cx, input: &str, event| {
                            if event != crate::ui::PromptEvent::Validate {
                                return;
                            }
                            let branch = input.trim();
                            if branch.is_empty() {
                                return;
                            }
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
                        },
                    );
                    compositor.push(Box::new(prompt));
                }))
            }
            IdeAction::GitLog => {
                let cwd = std::env::current_dir().unwrap_or_default();
                self.start_run(
                    context,
                    "git log --oneline --graph --decorate --all -30".into(),
                    cwd,
                );
                None
            }
            IdeAction::GitBlame(path) => {
                // Enable the annotate gutter for this file rather than dumping
                // `git blame` into the Run console.
                if !crate::blame::annotate_enabled() {
                    crate::blame::toggle_annotate();
                }
                crate::blame::ensure_annotate(&path);
                context.editor.set_status("blame annotate: on");
                None
            }
            IdeAction::ResolveConflict(path) => {
                // Open the conflicted file, then run the same `:merge` flow the
                // `merge`/`resolve` typable command uses, dropping into the 3-pane
                // ours/result/theirs resolver on the just-opened buffer.
                if context
                    .editor
                    .open(&path, zmax_view::editor::Action::Replace)
                    .is_ok()
                {
                    if let Some(cmd) = crate::commands::typed::TYPABLE_COMMAND_MAP.get("merge") {
                        let _ = (cmd.fun)(
                            context,
                            zmax_core::command_line::Args::default(),
                            crate::ui::PromptEvent::Validate,
                        );
                    }
                }
                None
            }
            IdeAction::RunConfigManager => Some(Box::new(|compositor, _cx| {
                compositor.push(Box::new(crate::ui::preferences::PreferencesPanel::new(3)));
            })),
            IdeAction::GitCommit => Some(Box::new(|compositor, _cx| {
                let prompt = crate::ui::Prompt::new(
                    "commit message: ".into(),
                    None,
                    |_editor, _input| Vec::new(),
                    move |cx, input: &str, event| {
                        if event != crate::ui::PromptEvent::Validate {
                            return;
                        }
                        let msg = input.trim();
                        if msg.is_empty() {
                            cx.editor.set_error("Aborted: empty commit message");
                            return;
                        }
                        let dir = std::env::current_dir().unwrap_or_default();
                        match std::process::Command::new("git")
                            .arg("-C")
                            .arg(&dir)
                            .args(["commit", "-m", msg])
                            .output()
                        {
                            Ok(o) if o.status.success() => {
                                // HEAD moved: refresh open buffers' gutter hunks
                                // (base-only — working tree bytes are unchanged).
                                crate::commands::refresh_all_diff_bases(cx.editor);
                                let out = String::from_utf8_lossy(&o.stdout);
                                let first = out.lines().next().unwrap_or("committed").to_owned();
                                cx.editor.set_status(format!("git: {first}"));
                            }
                            Ok(o) => {
                                let err = String::from_utf8_lossy(&o.stderr);
                                let first = err
                                    .lines()
                                    .chain(String::from_utf8_lossy(&o.stdout).lines())
                                    .find(|l| !l.trim().is_empty())
                                    .unwrap_or("commit failed")
                                    .to_owned();
                                cx.editor.set_error(format!("git commit: {first}"));
                            }
                            Err(e) => cx.editor.set_error(format!("git: {e}")),
                        }
                    },
                );
                compositor.push(Box::new(prompt));
            })),
            IdeAction::OpenPrefs(tab) => Some(Box::new(move |compositor, _cx| {
                compositor.push(Box::new(crate::ui::preferences::PreferencesPanel::new(tab)));
            })),
            IdeAction::Debug => {
                // Launch a DAP session (shows the debug-template picker).
                let mut cx = commands::Context {
                    editor: context.editor,
                    count: None,
                    register: None,
                    callback: Vec::new(),
                    on_next_key_callback: None,
                    jobs: context.jobs,
                };
                crate::commands::dap::dap_launch(&mut cx);
                let callbacks = cx.callback;
                if callbacks.is_empty() {
                    None
                } else {
                    Some(Box::new(
                        move |compositor: &mut crate::compositor::Compositor,
                              cx: &mut crate::compositor::Context| {
                            for cb in callbacks {
                                cb(compositor, cx);
                            }
                        },
                    ))
                }
            }
            IdeAction::ShowContextMenu {
                path,
                is_dir,
                row,
                col,
            } => Some(Box::new(
                move |compositor: &mut crate::compositor::Compositor,
                      _cx: &mut crate::compositor::Context| {
                    compositor.push(Box::new(super::ide::file_context_menu(
                        path, is_dir, row, col,
                    )));
                },
            )),
            IdeAction::ShowMenu(menu) => Some(Box::new(
                move |compositor: &mut crate::compositor::Compositor,
                      _cx: &mut crate::compositor::Context| {
                    compositor.push(Box::new(menu));
                },
            )),
        }
    }

    pub fn spinners_mut(&mut self) -> &mut ProgressSpinners {
        &mut self.spinners
    }

    pub fn render_view(
        &self,
        editor: &Editor,
        doc: &Document,
        view: &View,
        viewport: Rect,
        surface: &mut Surface,
        is_focused: bool,
    ) {
        let inner = view.inner_area(doc);
        let area = view.area;
        let theme = &editor.theme;
        let config = editor.config();
        let loader = editor.syn_loader.load();

        let view_offset = doc.view_offset(view.id);

        let text_annotations = view.text_annotations(doc, Some(theme));
        let mut decorations = DecorationManager::default();

        // vim `cursorlineopt`: `number` highlights only the line's number (the
        // line-number gutter does that half), so the full-line highlight is off.
        let cursorline_opt = crate::commands::typed::vim_opt_str("cursorlineopt");
        let highlight_line = cursorline_opt
            .as_deref()
            .is_none_or(crate::commands::typed::cursorline_opt_line);
        if is_focused && config.cursorline && highlight_line {
            decorations.add_decoration(Self::cursorline(doc, view, theme));
        }

        if is_focused && config.cursorcolumn {
            Self::highlight_cursorcolumn(doc, view, surface, theme, inner, &text_annotations);
        }

        // Set DAP highlights, if needed.
        if let Some(frame) = editor.current_stack_frame() {
            let dap_line = frame.line.saturating_sub(1);
            let style = theme.get("ui.highlight.frameline");
            let line_decoration = move |renderer: &mut TextRenderer, pos: LinePos| {
                if pos.doc_line != dap_line {
                    return;
                }
                renderer.set_style(Rect::new(inner.x, pos.visual_line, inner.width, 1), style);
            };

            decorations.add_decoration(line_decoration);
        }

        let syntax_highlighter =
            Self::doc_syntax_highlighter(doc, view_offset.anchor, inner.height, &loader);
        let mut overlays = Vec::new();

        overlays.push(Self::overlay_syntax_highlights(
            doc,
            view_offset.anchor,
            inner.height,
            &text_annotations,
        ));

        if doc
            .language_config()
            .and_then(|config| config.rainbow_brackets)
            .unwrap_or(config.rainbow_brackets)
        {
            if let Some(overlay) =
                Self::doc_rainbow_highlights(doc, view_offset.anchor, inner.height, theme, &loader)
            {
                overlays.push(overlay);
            }
        }

        if let Some(overlay) = Self::doc_document_link_highlights(doc, theme) {
            overlays.push(overlay);
        }

        Self::doc_diagnostics_highlights_into(doc, theme, &mut overlays);

        // Emacs face text properties: facemenu / enriched-mode faces stored on
        // the buffer's characters. Under the search / selection overlays, above
        // syntax — Emacs' `face` property overrides font-lock the same way.
        if let Some(overlay) = Self::doc_text_prop_highlights(doc, view, theme) {
            overlays.push(overlay);
        }

        // Emacs cwarn-mode: `if (a = b)` / `if (x);` in C.
        if let Some(overlay) = Self::doc_cwarn_highlights(doc, view, theme) {
            overlays.push(overlay);
        }

        // Emacs goto-address-mode: buttonize URLs and e-mail addresses.
        if let Some(overlay) = Self::doc_goto_address_highlights(doc, view, theme) {
            overlays.push(overlay);
        }

        // Emacs `bug-reference-mode`: `Bug#1234` and friends as tracker links.
        if let Some(overlay) = Self::doc_bug_reference_highlights(doc, view, theme) {
            overlays.push(overlay);
        }

        // Emacs `highlight-changes-mode`: the parts of the buffer changed since
        // the mode was turned on (or since the last save).
        if let Some(overlay) = Self::doc_highlight_changes(doc, view, theme) {
            overlays.push(overlay);
        }

        // Emacs Hi-Lock: persistent user regexp highlights (all windows).
        overlays.extend(Self::doc_hilock_highlights(doc, view, theme));

        // spacemacs colors layer: colour literals painted with their own colour,
        // then identifiers coloured from their own text.
        if let Some(overlay) = Self::doc_color_literal_highlights(doc, view) {
            overlays.push(overlay);
        }
        if let Some(overlay) = Self::doc_identifier_color_highlights(doc, view) {
            overlays.push(overlay);
        }

        // spacemacs nav-flash layer: the just-jumped-to line, briefly.
        if let Some(overlay) = Self::doc_nav_flash_highlight(doc, view, theme) {
            overlays.push(overlay);
        }

        // smeargle: lines tinted by the age of the commit that last touched them.
        overlays.extend(Self::doc_smeargle_highlights(doc, view, theme));

        // vim `hlsearch`: highlight all matches of the last search pattern.
        if let Some(overlay) = Self::doc_search_highlights(editor, doc, view, theme) {
            overlays.push(overlay);
        }

        // vim `spell`: underline misspelled words in the viewport.
        if let Some(overlay) = Self::doc_spell_highlights(doc, view, theme) {
            overlays.push(overlay);
        }

        if is_focused {
            if config.lsp.auto_document_highlight {
                if let Some(overlay) = Self::doc_document_highlights(doc, view, theme) {
                    overlays.push(overlay);
                }
            }
            if config.highlight_word_under_cursor {
                if let Some(overlay) = Self::doc_word_occurrence_highlights(doc, view, theme) {
                    overlays.push(overlay);
                }
            }
            if let Some(tabstops) = Self::tabstop_highlights(doc, theme) {
                overlays.push(tabstops);
            }
            overlays.push(Self::doc_selection_highlights(
                editor.mode(),
                doc,
                view,
                theme,
                &config.cursor_shape,
                self.terminal_focused,
            ));
            if let Some(overlay) = Self::highlight_focused_view_elements(view, doc, theme) {
                overlays.push(overlay);
            }
            if let Some(overlay) = Self::showmatch_highlight(editor, doc, theme) {
                overlays.push(overlay);
            }
            if let Some(overlay) = self.nav_flash_highlight(doc, theme) {
                overlays.push(overlay);
            }
        }

        let gutter_overflow = view.gutter_offset(doc) == 0;
        if !gutter_overflow {
            Self::render_gutter(
                editor,
                doc,
                view,
                view.area,
                theme,
                is_focused & self.terminal_focused,
                &mut decorations,
            );
        }

        Self::render_rulers(editor, doc, view, inner, surface, theme);

        let primary_cursor = doc
            .selection(view.id)
            .primary()
            .cursor(doc.text().slice(..));
        if is_focused {
            decorations.add_decoration(text_decorations::Cursor {
                cache: &editor.cursor_cache,
                primary_cursor,
            });
        }
        let width = view.inner_width(doc);
        let config = doc.config.load();
        let enable_cursor_line = view
            .diagnostics_handler
            .show_cursorline_diagnostics(doc, view.id);
        let inline_diagnostic_config = config.inline_diagnostics.prepare(width, enable_cursor_line);
        decorations.add_decoration(InlineDiagnostics::new(
            doc,
            theme,
            primary_cursor,
            inline_diagnostic_config,
            config.end_of_line_diagnostics,
        ));
        render_document(
            surface,
            inner,
            doc,
            view_offset,
            &text_annotations,
            syntax_highlighter,
            overlays,
            theme,
            decorations,
            Some(view.id),
        );

        // Sticky scroll: pin enclosing scope headers at the top of the viewport.
        if is_focused {
            self.render_sticky_context(doc, inner, view_offset.anchor, surface, theme, &loader);
        }

        // vim `fillchars` `eob:` — the character marking the rows below the last
        // line of the buffer (vim's `~` lines). zmax leaves them blank, which is
        // `fillchars=eob:\ `, so this only draws once the option asks for it.
        // spacemacs `+vim/vim-empty-lines` turns the same markers on without a
        // `:set`, so it is consulted when `fillchars` names no `eob:` item.
        if let Some(eob) = fillchar("eob").or_else(crate::sm_misc::empty_lines_char) {
            Self::render_eob(doc, view, inner, surface, theme, eob);
        }

        // vim `foldtext`: a closed fold shows this summary instead of its first
        // line. vim renders it by default; the option only overrides the format,
        // so an unset option falls through to the built-in `+-- N lines:` look.
        let foldtext = crate::commands::typed::vim_opt_str("foldtext");
        Self::render_foldtext(doc, view, inner, surface, theme, foldtext.as_deref());

        // vim `winbar`: a bar on the window's top row (the row `View::inner_area`
        // already took out of the text area).
        if let Some(fmt) = crate::commands::typed::vim_opt_str("winbar") {
            let bar = view.winbar_area();
            if bar.height > 0 {
                let style = theme.get(if is_focused {
                    "ui.statusline"
                } else {
                    "ui.statusline.inactive"
                });
                Self::render_vim_bar(&fmt, doc, view, bar, surface, style);
            }
        }

        // emacs `window-tool-bar-mode`: the window's own tool bar, on the row
        // under the winbar that `View::inner_area` already reserved.
        let wtb = view.window_tool_bar_area();
        if wtb.height > 0 {
            Self::render_button_row(
                wtb,
                surface,
                theme.get("ui.menu"),
                crate::emacs_frame::WINDOW_TOOL_BAR_BUTTONS
                    .iter()
                    .map(|(label, _)| (*label, false)),
                theme.get("ui.menu.selected"),
            );
        }

        // emacs `scroll-bar-mode` / `horizontal-scroll-bar-mode`: the reserved
        // strips, drawn from the same view offset the text was rendered at.
        Self::render_scroll_bars(doc, view, surface, theme);

        // if we're not at the edge of the screen, draw a right border. emacs
        // `window-divider-mode` decides whether windows are separated at all;
        // with it off the two panes touch, as they do in emacs without dividers.
        if viewport.right() != view.area.right() && crate::emacs_frame::window_divider() {
            let x = area.right();
            let border_style = theme.get("ui.window");
            // vim `fillchars` `vert:` — the character the vertical split is drawn
            // with (`:set fillchars=vert:┃`).
            let symbol = fillchar("vert")
                .map(String::from)
                .unwrap_or_else(|| tui::symbols::line::VERTICAL.to_string());
            for y in area.top()..area.bottom() {
                surface[(x, y)].set_symbol(&symbol).set_style(border_style);
            }
        }

        if config.inline_diagnostics.disabled()
            && config.end_of_line_diagnostics == DiagnosticFilter::Disable
        {
            Self::render_diagnostics(doc, view, inner, surface, theme);
        }

        // vim `laststatus=0`: skip the per-window status line entirely. The
        // frame-wide powerline bar does the same for the row it replaces — with
        // it on the window has no status row to draw into (`inner_area` already
        // handed the row to the text).
        if config.render_statusline && zmax_view::view::window_status_line_rows() > 0 {
            let statusline_area = view
                .area
                .clip_top(view.area.height.saturating_sub(1))
                .clip_bottom(1); // -1 from bottom to remove commandline

            let mut context =
                statusline::RenderContext::new(editor, doc, view, is_focused, &self.spinners);

            statusline::render(&mut context, statusline_area, surface);
        }
    }

    /// vim `fillchars=eob:~`: fill every row below the last line of the buffer
    /// with that character (vim's `~` lines). Rows below the document's last
    /// visual line, left column only, exactly as vim draws them.
    fn render_eob(
        doc: &Document,
        view: &View,
        inner: Rect,
        surface: &mut Surface,
        theme: &Theme,
        eob: char,
    ) {
        let text = doc.text().slice(..);
        let Some(last) = view.screen_coords_at_pos(doc, text, text.len_chars()) else {
            return; // the end of the buffer is not on screen: nothing to fill
        };
        let style = theme
            .try_get("ui.virtual.whitespace")
            .unwrap_or_else(|| theme.get("ui.linenr"));
        let eob = eob.to_string();
        for row in (last.row + 1)..inner.height as usize {
            surface.set_string(inner.x, inner.y + row as u16, &eob, style);
        }
    }

    /// vim `foldtext`: draw the fold's text over the first line of every *closed*
    /// fold that is on screen, padded out with the `fillchars` `fold:` character
    /// (vim pads the fold line the same way).
    fn render_foldtext(
        doc: &Document,
        view: &View,
        inner: Rect,
        surface: &mut Surface,
        theme: &Theme,
        value: Option<&str>,
    ) {
        let folds = doc.folds();
        let closed: Vec<_> = folds.iter().filter(|f| f.closed).copied().collect();
        if closed.is_empty() {
            return;
        }
        // An unset `foldtext` uses vim's built-in default format.
        let value = value.unwrap_or("");
        let text = doc.text().slice(..);
        let style = theme
            .try_get("ui.virtual.jump-label")
            .unwrap_or_else(|| theme.get("ui.linenr"));
        // vim's default `fillchars` fold char is `-`; the dashes trailing the
        // summary come from it.
        let pad = fillchar("fold").unwrap_or('-');
        let comment_tokens = doc
            .language_config()
            .and_then(|c| c.comment_tokens.as_deref())
            .unwrap_or(&[]);
        for fold in closed {
            if fold.start >= text.len_lines() {
                continue;
            }
            // A nested fold whose header is itself hidden by an outer closed fold
            // must not draw — only the outer summary line shows.
            if folds.is_line_hidden(fold.start) {
                continue;
            }
            let start = text.line_to_char(fold.start);
            let Some(pos) = view.screen_coords_at_pos(doc, text, start) else {
                continue;
            };
            if pos.row >= inner.height as usize {
                continue;
            }
            // vim's fold level (`v:foldlevel`): how many folds enclose the header.
            let level = folds
                .iter()
                .filter(|f| f.start <= fold.start && fold.start <= f.end)
                .count();
            let first_line: String = text.line(fold.start).chars().collect();
            let cleaned = clean_fold_line(&first_line, comment_tokens);
            let mut line = fold_text(value, fold.len(), level, &cleaned);
            let width = inner.width as usize;
            for _ in line.chars().count()..width {
                line.push(pad);
            }
            surface.set_stringn(inner.x, inner.y + pos.row as u16, &line, width, style);
        }
    }

    /// Render a vim bar format (`winbar` / `tabline`) into one row: the part
    /// before `%=` is left-aligned, the part after it right-aligned.
    fn render_vim_bar(
        fmt: &str,
        doc: &Document,
        view: &View,
        area: Rect,
        surface: &mut Surface,
        style: Style,
    ) {
        let text = doc.text().slice(..);
        let cursor = doc.selection(view.id).primary().cursor(text);
        let line = text.char_to_line(cursor);
        let bar_cx = BarContext {
            path: doc
                .path()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|| SCRATCH_BUFFER_NAME.to_string()),
            name: doc.display_name().into_owned(),
            modified: doc.is_modified(),
            readonly: doc.readonly,
            filetype: doc.language_name().map(str::to_string).unwrap_or_default(),
            line: line + 1,
            lines: text.len_lines(),
            col: cursor - text.line_to_char(line) + 1,
        };
        let (left, right) = vim_bar_expand(fmt, &bar_cx);
        surface.clear_with(area, style);
        surface.set_stringn(area.x, area.y, &left, area.width as usize, style);
        let right_width = right.width() as u16;
        if right_width > 0 && right_width < area.width {
            surface.set_stringn(
                area.right() - right_width,
                area.y,
                &right,
                right_width as usize,
                style,
            );
        }
    }

    /// Sticky scroll: overlay the enclosing scope header lines (function/class
    /// signatures whose opening scrolled above the viewport) at the top of the
    /// text area. The outline is cached per document length so scrolling is cheap.
    fn render_sticky_context(
        &self,
        doc: &Document,
        inner: Rect,
        anchor: usize,
        surface: &mut Surface,
        theme: &Theme,
        loader: &syntax::Loader,
    ) {
        if inner.height < 6 || inner.width < 8 {
            return;
        }
        let text = doc.text();
        let key = (doc.id(), text.len_chars());
        let mut cache = self.sticky_cache.borrow_mut();
        if cache.as_ref().map(|c| (c.0, c.1)) != Some(key) {
            let items = crate::commands::syntax::document_outline(doc, loader);
            let mut scopes: Vec<(usize, usize, String)> = items
                .iter()
                .filter_map(|it| {
                    let s = it.start.min(text.len_chars());
                    let e = it.end.min(text.len_chars());
                    let sl = text.char_to_line(s);
                    let el = text.char_to_line(e);
                    // only multi-line scopes are worth pinning
                    (el > sl).then(|| {
                        let line: String =
                            text.line(sl).chars().filter(|c| !c.is_control()).collect();
                        (sl, el, line)
                    })
                })
                .collect();
            scopes.sort_by_key(|s| s.0);
            *cache = Some((key.0, key.1, scopes));
        }
        let scopes = &cache.as_ref().unwrap().2;

        let top = text.char_to_line(anchor.min(text.len_chars()));
        // Enclosing scopes that opened above the viewport, outermost first.
        let mut ctx: Vec<&(usize, usize, String)> = scopes
            .iter()
            .filter(|(sl, el, _)| *sl < top && *el >= top)
            .collect();
        ctx.sort_by_key(|(sl, _, _)| *sl);
        if ctx.is_empty() {
            return;
        }
        // Keep at most a third of the viewport, innermost-closest to the content.
        let max = ((inner.height as usize) / 3).clamp(1, 5);
        if ctx.len() > max {
            ctx = ctx.split_off(ctx.len() - max);
        }

        let hdr = theme.get("ui.statusline");
        let marker = theme.get("comment");
        for (i, (_, _, line)) in ctx.iter().enumerate() {
            let y = inner.y + i as u16;
            let w = inner.width.saturating_sub(1) as usize;
            surface.set_style(Rect::new(inner.x, y, inner.width, 1), hdr);
            surface.set_stringn(inner.x, y, line, w, hdr);
            surface.set_stringn(inner.x + inner.width - 1, y, "▏", 1, marker);
        }
    }

    pub fn render_rulers(
        editor: &Editor,
        doc: &Document,
        view: &View,
        viewport: Rect,
        surface: &mut Surface,
        theme: &Theme,
    ) {
        let editor_rulers = &editor.config().rulers;
        let ruler_theme = theme
            .try_get("ui.virtual.ruler")
            .unwrap_or_else(|| Style::default().bg(Color::Red));

        let rulers = doc
            .language_config()
            .and_then(|config| config.rulers.as_ref())
            .unwrap_or(editor_rulers);

        let view_offset = doc.view_offset(view.id);

        rulers
            .iter()
            // View might be horizontally scrolled, convert from absolute distance
            // from the 1st column to relative distance from left of viewport
            .filter_map(|ruler| ruler.checked_sub(1 + view_offset.horizontal_offset as u16))
            .filter(|ruler| ruler < &viewport.width)
            .map(|ruler| viewport.clip_left(ruler).with_width(1))
            .for_each(|area| surface.set_style(area, ruler_theme))
    }

    fn viewport_byte_range(
        text: zmax_core::RopeSlice,
        row: usize,
        height: u16,
    ) -> std::ops::Range<usize> {
        // Calculate viewport byte ranges:
        // Saturating subs to make it inclusive zero indexing.
        let last_line = text.len_lines().saturating_sub(1);
        let last_visible_line = (row + height as usize).saturating_sub(1).min(last_line);
        let start = text.line_to_byte(row.min(last_line));
        let end = text.line_to_byte(last_visible_line + 1);

        start..end
    }

    /// Get the syntax highlighter for a document in a view represented by the first line
    /// and column (`offset`) and the last line. This is done instead of using a view
    /// directly to enable rendering syntax highlighted docs anywhere (eg. picker preview)
    pub fn doc_syntax_highlighter<'editor>(
        doc: &'editor Document,
        anchor: usize,
        height: u16,
        loader: &'editor syntax::Loader,
    ) -> Option<syntax::Highlighter<'editor>> {
        let syntax = doc.syntax()?;
        let text = doc.text().slice(..);
        let row = text.char_to_line(anchor.min(text.len_chars()));
        let range = Self::viewport_byte_range(text, row, height);

        // vim `synmaxcol`: give up on syntax highlighting when a line in view is
        // longer than this, which is what makes a minified file scroll at all.
        // vim decides per line; the highlighter is built per viewport, so the
        // whole viewport goes unhighlighted when it holds such a line.
        if let Some(max_col) = crate::commands::vim_opt_num("synmaxcol").filter(|n| *n > 0) {
            let last_line = text.len_lines().saturating_sub(1);
            let last_visible = (row + height as usize).saturating_sub(1).min(last_line);
            if (row..=last_visible).any(|line| text.line(line).len_chars() > max_col) {
                return None;
            }
        }

        let range = range.start as u32..range.end as u32;
        let highlighter = syntax.highlighter(text, loader, range);
        Some(highlighter)
    }

    pub fn overlay_syntax_highlights(
        doc: &Document,
        anchor: usize,
        height: u16,
        text_annotations: &TextAnnotations,
    ) -> OverlayHighlights {
        let text = doc.text().slice(..);
        let row = text.char_to_line(anchor.min(text.len_chars()));

        let mut range = Self::viewport_byte_range(text, row, height);
        range = text.byte_to_char(range.start)..text.byte_to_char(range.end);

        text_annotations.collect_overlay_highlights(range)
    }

    pub fn doc_rainbow_highlights(
        doc: &Document,
        anchor: usize,
        height: u16,
        theme: &Theme,
        loader: &syntax::Loader,
    ) -> Option<OverlayHighlights> {
        let syntax = doc.syntax()?;
        let text = doc.text().slice(..);
        let row = text.char_to_line(anchor.min(text.len_chars()));
        let visible_range = Self::viewport_byte_range(text, row, height);
        let start = syntax::child_for_byte_range(
            &syntax.tree().root_node(),
            visible_range.start as u32..visible_range.end as u32,
        )
        .map_or(visible_range.start as u32, |node| node.start_byte());
        let range = start..visible_range.end as u32;

        Some(syntax.rainbow_highlights(text, theme.rainbow_length(), loader, range))
    }

    /// Get highlight spans for document diagnostics
    pub fn doc_diagnostics_highlights_into(
        doc: &Document,
        theme: &Theme,
        overlay_highlights: &mut Vec<OverlayHighlights>,
    ) {
        // Skip redundant work if no diagnostics.
        if doc.diagnostics().is_empty() {
            return;
        }

        use zmax_core::diagnostic::{DiagnosticTag, Range, Severity};
        let get_scope_of = |scope| {
            theme
                .find_highlight_exact(scope)
                // get one of the themes below as fallback values
                .or_else(|| theme.find_highlight_exact("diagnostic"))
                .or_else(|| theme.find_highlight_exact("ui.cursor"))
                .or_else(|| theme.find_highlight_exact("ui.selection"))
                .expect(
                    "at least one of the following scopes must be defined in the theme: `diagnostic`, `ui.cursor`, or `ui.selection`",
                )
        };

        // Diagnostic tags
        let unnecessary = theme.find_highlight_exact("diagnostic.unnecessary");
        let deprecated = theme.find_highlight_exact("diagnostic.deprecated");

        let mut default_vec = Vec::new();
        let mut info_vec = Vec::new();
        let mut hint_vec = Vec::new();
        let mut warning_vec = Vec::new();
        let mut error_vec = Vec::new();
        let mut unnecessary_vec = Vec::new();
        let mut deprecated_vec = Vec::new();

        let push_diagnostic = |vec: &mut Vec<ops::Range<usize>>, range: Range| {
            // If any diagnostic overlaps ranges with the prior diagnostic,
            // merge the two together. Otherwise push a new span.
            match vec.last_mut() {
                Some(existing_range) if range.start <= existing_range.end => {
                    // This branch merges overlapping diagnostics, assuming that the current
                    // diagnostic starts on range.start or later. If this assertion fails,
                    // we will discard some part of `diagnostic`. This implies that
                    // `doc.diagnostics()` is not sorted by `diagnostic.range`.
                    debug_assert!(existing_range.start <= range.start);
                    existing_range.end = range.end.max(existing_range.end)
                }
                _ => vec.push(range.start..range.end),
            }
        };

        for diagnostic in doc.diagnostics() {
            // Separate diagnostics into different Vecs by severity.
            let vec = match diagnostic.severity {
                Some(Severity::Info) => &mut info_vec,
                Some(Severity::Hint) => &mut hint_vec,
                Some(Severity::Warning) => &mut warning_vec,
                Some(Severity::Error) => &mut error_vec,
                _ => &mut default_vec,
            };

            // If the diagnostic has tags and a non-warning/error severity, skip rendering
            // the diagnostic as info/hint/default and only render it as unnecessary/deprecated
            // instead. For warning/error diagnostics, render both the severity highlight and
            // the tag highlight.
            if diagnostic.tags.is_empty()
                || matches!(
                    diagnostic.severity,
                    Some(Severity::Warning | Severity::Error)
                )
            {
                push_diagnostic(vec, diagnostic.range);
            }

            for tag in &diagnostic.tags {
                match tag {
                    DiagnosticTag::Unnecessary => {
                        if unnecessary.is_some() {
                            push_diagnostic(&mut unnecessary_vec, diagnostic.range)
                        }
                    }
                    DiagnosticTag::Deprecated => {
                        if deprecated.is_some() {
                            push_diagnostic(&mut deprecated_vec, diagnostic.range)
                        }
                    }
                }
            }
        }

        overlay_highlights.push(OverlayHighlights::Homogeneous {
            highlight: get_scope_of("diagnostic"),
            ranges: default_vec,
        });
        if let Some(highlight) = unnecessary {
            overlay_highlights.push(OverlayHighlights::Homogeneous {
                highlight,
                ranges: unnecessary_vec,
            });
        }
        if let Some(highlight) = deprecated {
            overlay_highlights.push(OverlayHighlights::Homogeneous {
                highlight,
                ranges: deprecated_vec,
            });
        }
        overlay_highlights.extend([
            OverlayHighlights::Homogeneous {
                highlight: get_scope_of("diagnostic.info"),
                ranges: info_vec,
            },
            OverlayHighlights::Homogeneous {
                highlight: get_scope_of("diagnostic.hint"),
                ranges: hint_vec,
            },
            OverlayHighlights::Homogeneous {
                highlight: get_scope_of("diagnostic.warning"),
                ranges: warning_vec,
            },
            OverlayHighlights::Homogeneous {
                highlight: get_scope_of("diagnostic.error"),
                ranges: error_vec,
            },
        ]);
    }

    pub fn doc_document_highlights(
        doc: &Document,
        view: &View,
        theme: &Theme,
    ) -> Option<OverlayHighlights> {
        let ranges = doc.document_highlights(view.id)?;
        if ranges.is_empty() {
            return None;
        }

        let highlight = theme
            .find_highlight_exact("ui.highlight")
            .or_else(|| theme.find_highlight_exact("ui.selection"))
            .or_else(|| theme.find_highlight_exact("ui.cursor"))?;

        Some(OverlayHighlights::Homogeneous {
            highlight,
            ranges: ranges.to_vec(),
        })
    }

    /// Highlight every whole-word occurrence of the word under the primary
    /// cursor within the visible viewport (vim-illuminate / JetBrains
    /// identifier-under-caret behaviour). Bounded to the visible line range so
    /// the per-frame scan stays cheap regardless of file size.
    /// Emacs Hi-Lock overlays: one homogeneous overlay per active pattern (each
    /// coloured by its index), scanning only the visible line range. Empty when
    /// no `highlight-regexp` pattern is active.
    pub fn doc_hilock_highlights(
        doc: &Document,
        view: &View,
        theme: &Theme,
    ) -> Vec<OverlayHighlights> {
        // Emacs `hi-lock-mode` gates the display (the patterns stay registered).
        if !crate::commands::hi_lock_enabled() || crate::hi_lock::is_empty() {
            return Vec::new();
        }
        let text = doc.text().slice(..);
        if text.len_chars() == 0 {
            return Vec::new();
        }
        let view_offset = doc.view_offset(view.id);
        let height = view.inner_area(doc).height as usize;
        let first_line = text.char_to_line(view_offset.anchor);
        let last_line = (first_line + height + 1).min(text.len_lines());
        let scan_start = text.line_to_char(first_line);
        let scan_end = text.line_to_char(last_line);
        let haystack: String = text.slice(scan_start..scan_end).chars().collect();

        let matches =
            crate::hi_lock::with_patterns(|pats| crate::hi_lock::viewport_matches(&haystack, pats));
        if matches.is_empty() {
            return Vec::new();
        }

        // A small palette cycled per pattern index; all fall back to the
        // guaranteed match highlight so patterns are always visible.
        const SCOPES: [&str; 5] = [
            "ui.highlight",
            "diagnostic.warning",
            "diagnostic.info",
            "diagnostic.error",
            "diagnostic.hint",
        ];
        let fallback = theme.find_highlight_exact("ui.cursor.match");

        let mut by_pat: std::collections::BTreeMap<usize, Vec<ops::Range<usize>>> =
            std::collections::BTreeMap::new();
        for (cs, ce, idx) in matches {
            by_pat
                .entry(idx)
                .or_default()
                .push((scan_start + cs)..(scan_start + ce));
        }

        let mut out = Vec::new();
        for (idx, mut ranges) in by_pat {
            ranges.sort_by_key(|r| r.start);
            let highlight = theme
                .find_highlight_exact(SCOPES[idx % SCOPES.len()])
                .or(fallback);
            if let Some(highlight) = highlight {
                out.push(OverlayHighlights::Homogeneous { highlight, ranges });
            }
        }
        out
    }

    /// smeargle overlays: one homogeneous overlay per age band, each covering the
    /// whole of every visible line whose last commit falls in that band. Empty
    /// unless `smeargle` / `smeargle-commits` (`SPC g H t` / `SPC g H h`) is on
    /// and the buffer is a file in a git repo. Only the visible line range is
    /// turned into ranges, so the per-frame cost does not grow with the file.
    pub fn doc_smeargle_highlights(
        doc: &Document,
        view: &View,
        theme: &Theme,
    ) -> Vec<OverlayHighlights> {
        use crate::spacemacs_keys::{line_bands, smeargle_mode, SMEARGLE_SCOPES};

        let Some(mode) = smeargle_mode() else {
            return Vec::new();
        };
        let Some(path) = doc.path() else {
            return Vec::new();
        };
        let bands = line_bands(path, mode);
        if bands.is_empty() {
            return Vec::new();
        }

        let text = doc.text().slice(..);
        let view_offset = doc.view_offset(view.id);
        let height = view.inner_area(doc).height as usize;
        let first_line = text.char_to_line(view_offset.anchor);
        let last_line = (first_line + height + 1).min(text.len_lines());

        let mut by_band: Vec<Vec<ops::Range<usize>>> = vec![Vec::new(); SMEARGLE_SCOPES.len()];
        for line in first_line..last_line {
            let Some(Some(band)) = bands.get(line).copied() else {
                continue;
            };
            let start = text.line_to_char(line);
            let end = text.line_to_char((line + 1).min(text.len_lines()));
            if end > start {
                by_band[band].push(start..end);
            }
        }

        // Not every theme defines every scope in the palette; a band whose scope
        // this theme is missing falls back to one every theme has, so the
        // highlighting never has invisible holes in it.
        let fallback = theme
            .find_highlight_exact("ui.selection")
            .or_else(|| theme.find_highlight_exact("ui.highlight"));
        by_band
            .into_iter()
            .enumerate()
            .filter(|(_, ranges)| !ranges.is_empty())
            .filter_map(|(band, ranges)| {
                let highlight = theme
                    .find_highlight_exact(SMEARGLE_SCOPES[band])
                    .or(fallback)?;
                Some(OverlayHighlights::Homogeneous { highlight, ranges })
            })
            .collect()
    }

    /// The `Style` an Emacs face text property renders as, or `None` when the
    /// face carries nothing this theme can paint.
    ///
    /// A named face (`facemenu-set-face`) resolves through
    /// [`zmax_core::facemenu::theme_scope`] against the *live* theme, so
    /// `font-lock-string-face` is whatever the current theme paints strings with;
    /// the attribute toggles and the two colours are layered on top of it, which
    /// is Emacs' attribute-merge order.
    fn text_prop_style(face: &zmax_core::text_props::Face, theme: &Theme) -> Option<Style> {
        use zmax_view::graphics::UnderlineStyle;

        let mut style = Style::default();
        let mut painted = false;
        if let Some(scope) = face
            .name
            .as_deref()
            .and_then(zmax_core::facemenu::theme_scope)
        {
            if let Some(base) = theme.try_get(scope) {
                style = style.patch(base);
                painted = true;
            }
        }
        if face.bold {
            style = style.add_modifier(Modifier::BOLD);
            painted = true;
        }
        if face.italic {
            style = style.add_modifier(Modifier::ITALIC);
            painted = true;
        }
        if face.underline {
            style = style.underline_style(UnderlineStyle::Line);
            painted = true;
        }
        if let Some((r, g, b)) = face.fg {
            style = style.fg(Color::Rgb(r, g, b));
            painted = true;
        }
        if let Some((r, g, b)) = face.bg {
            style = style.bg(Color::Rgb(r, g, b));
            painted = true;
        }
        painted.then_some(style)
    }

    /// Emacs face text properties: the persistent per-region faces that
    /// `facemenu-set-*`, `enriched-mode` / `format-decode-buffer` and
    /// `cpp-highlight-buffer` put on the buffer's characters.
    ///
    /// Unlike every other overlay here these are *stored on the document*, not
    /// recomputed from a scan, so this only clips the runs to the viewport and
    /// interns each run's `Style` into a `Highlight`
    /// ([`Theme::face_highlight`]) — the face attributes a run can carry are
    /// arbitrary and name no theme scope.
    /// Emacs `reveal-mode`: while point is inside a run of text hidden with the
    /// `invisible` text property, that run is made visible; when point leaves it,
    /// it is hidden again. Only one run is held open at a time, exactly as emacs's
    /// `reveal-mode` tracks the overlays it has opened.
    fn apply_reveal_mode(editor: &mut Editor) {
        if !editor.reveal_mode {
            return;
        }
        let (view_id, doc_id) = {
            let (view, doc) = current_ref!(editor);
            (view.id, doc.id())
        };
        let cursor = {
            let doc = doc!(editor);
            doc.selection(view_id)
                .primary()
                .cursor(doc.text().slice(..))
        };

        // Close a run point has left.
        if let Some((open_doc, range)) = editor.revealed.clone() {
            let still_inside = open_doc == doc_id && range.contains(&cursor);
            if !still_inside {
                if let Some(doc) = editor.documents.get_mut(&open_doc) {
                    doc.update_text_props(|props| props.set_invisible(range, true));
                }
                editor.revealed = None;
            } else {
                return;
            }
        }

        // Open the run point is inside, if it is hidden.
        let hidden = {
            let doc = doc!(editor);
            doc.text_props()
                .spans_in(cursor..cursor.saturating_add(1))
                .find(|span| span.props.invisible && span.start <= cursor && cursor < span.end)
                .map(|span| span.start..span.end)
        };
        if let Some(range) = hidden {
            let doc = doc_mut!(editor, &doc_id);
            doc.update_text_props(|props| props.set_invisible(range.clone(), false));
            editor.revealed = Some((doc_id, range));
        }
    }

    pub fn doc_text_prop_highlights(
        doc: &Document,
        view: &View,
        theme: &Theme,
    ) -> Option<OverlayHighlights> {
        let props = doc.text_props();
        if props.is_empty() {
            return None;
        }
        let text = doc.text().slice(..);
        let view_offset = doc.view_offset(view.id);
        let height = view.inner_area(doc).height as usize;
        let first_line = text.char_to_line(view_offset.anchor.min(text.len_chars()));
        let last_line = (first_line + height + 1).min(text.len_lines());
        let start = text.line_to_char(first_line);
        let end = text.line_to_char(last_line);

        let highlights: Vec<_> = props
            .spans_in(start..end)
            .filter_map(|span| {
                let style = Self::text_prop_style(&span.props.face, theme)?;
                let highlight = Theme::face_highlight(style)?;
                Some((highlight, span.start..span.end))
            })
            .collect();
        (!highlights.is_empty()).then_some(OverlayHighlights::Heterogenous { highlights })
    }

    /// Emacs `cwarn-mode` / `global-cwarn-mode`: flag the C constructs that are
    /// legal but almost always a mistake — `if (a = b)` and `if (x);`.
    ///
    /// Scans only the visible line range, so it stays cheap on a large
    /// translation unit and re-runs as you type (the warnings are not stored on
    /// the buffer; they are a property of the current text).
    pub fn doc_cwarn_highlights(
        doc: &Document,
        view: &View,
        theme: &Theme,
    ) -> Option<OverlayHighlights> {
        if !crate::commands::cwarn_enabled(doc.id()) {
            return None;
        }
        let text = doc.text().slice(..);
        let view_offset = doc.view_offset(view.id);
        let height = view.inner_area(doc).height as usize;
        let first_line = text.char_to_line(view_offset.anchor.min(text.len_chars()));
        let last_line = (first_line + height + 1).min(text.len_lines());

        let highlight = theme
            .find_highlight_exact("diagnostic.warning")
            .or_else(|| theme.find_highlight_exact("warning"))?;

        let mut ranges = Vec::new();
        for line in first_line..last_line {
            let src = text.line(line).to_string();
            let line_start = text.line_to_char(line);
            for warning in zmax_core::cmode::cwarn_line(line, src.trim_end_matches('\n')) {
                // `cwarn_line` reports byte offsets within the line; the renderer
                // needs char offsets into the document.
                let start = src[..warning.range.start].chars().count();
                let end = src[..warning.range.end].chars().count();
                ranges.push(line_start + start..line_start + end);
            }
        }
        ranges.sort_by_key(|r| r.start);
        (!ranges.is_empty()).then_some(OverlayHighlights::Homogeneous { highlight, ranges })
    }

    /// The visible line range of `view`, as `(first_line, last_line)` — the
    /// bound every viewport scanner in this file shares.
    fn visible_lines(doc: &Document, view: &View) -> (usize, usize) {
        let text = doc.text().slice(..);
        let view_offset = doc.view_offset(view.id);
        let height = view.inner_area(doc).height as usize;
        let first = text.char_to_line(view_offset.anchor.min(text.len_chars()));
        (first, (first + height + 1).min(text.len_lines()))
    }

    /// spacemacs `+themes/colors` — `rainbow-mode`: paint every colour literal in
    /// the viewport with the colour it names, background plus a contrasting
    /// foreground so the literal is still readable. The colours are arbitrary
    /// RGB, so each one is interned as a face highlight rather than looked up in
    /// the theme.
    pub fn doc_color_literal_highlights(doc: &Document, view: &View) -> Option<OverlayHighlights> {
        if !crate::rainbow::rainbow_enabled(doc.id()) {
            return None;
        }
        let text = doc.text().slice(..);
        let (first_line, last_line) = Self::visible_lines(doc, view);
        let scan_start = text.line_to_char(first_line);
        let haystack: String = text
            .slice(scan_start..text.line_to_char(last_line))
            .chars()
            .collect();

        let mut highlights = Vec::new();
        for lit in crate::rainbow::color_literals(&haystack) {
            let fg = crate::rainbow::contrast_fg(lit.rgb);
            let style = Style::new()
                .bg(Color::Rgb(lit.rgb.0, lit.rgb.1, lit.rgb.2))
                .fg(Color::Rgb(fg.0, fg.1, fg.2));
            if let Some(highlight) = Theme::face_highlight(style) {
                highlights.push((highlight, (scan_start + lit.start)..(scan_start + lit.end)));
            }
        }
        (!highlights.is_empty()).then_some(OverlayHighlights::Heterogenous { highlights })
    }

    /// spacemacs `+themes/colors` — `rainbow-identifiers-mode` /
    /// `color-identifiers-mode`: give each identifier in the viewport a colour
    /// derived from its own text. The `Variables` mode keeps only the words the
    /// grammar parses as an identifier/variable node, which is the distinction
    /// between the two emacs modes.
    pub fn doc_identifier_color_highlights(
        doc: &Document,
        view: &View,
    ) -> Option<OverlayHighlights> {
        let mode = crate::rainbow::ident_mode(doc.id())?;
        let text = doc.text().slice(..);
        let (first_line, last_line) = Self::visible_lines(doc, view);
        let scan_start = text.line_to_char(first_line);
        let haystack: String = text
            .slice(scan_start..text.line_to_char(last_line))
            .chars()
            .collect();

        let mut highlights = Vec::new();
        for (start, end, name) in crate::rainbow::identifiers(&haystack) {
            let from = scan_start + start;
            if mode == crate::rainbow::IdentMode::Variables && !Self::is_variable_node(doc, from) {
                continue;
            }
            let (r, g, b) = crate::rainbow::identifier_color(&name);
            if let Some(highlight) = Theme::face_highlight(Style::new().fg(Color::Rgb(r, g, b))) {
                highlights.push((highlight, from..(scan_start + end)));
            }
        }
        (!highlights.is_empty()).then_some(OverlayHighlights::Heterogenous { highlights })
    }

    /// Whether the syntax tree calls the node at char index `pos` an identifier
    /// or a variable — `color-identifiers-mode`'s "only the variables" filter,
    /// answered from the grammar instead of from emacs faces.
    fn is_variable_node(doc: &Document, pos: usize) -> bool {
        let Some(syntax) = doc.syntax() else {
            // With no grammar loaded there is nothing to filter on; colouring
            // every word would be the *other* mode, so colour nothing.
            return false;
        };
        let text = doc.text().slice(..);
        let byte = text.char_to_byte(pos) as u32;
        let node = syntax
            .tree()
            .root_node()
            .descendant_for_byte_range(byte, byte + 1);
        node.is_some_and(|n| {
            let kind = n.kind();
            kind.contains("identifier") || kind.contains("variable") || kind == "name"
        })
    }

    /// spacemacs `+misc/nav-flash`: highlight the line the cursor just jumped to
    /// for a moment. Arming happens here too — the render path is the one place
    /// that sees every cursor movement regardless of which command caused it.
    pub fn doc_nav_flash_highlight(
        doc: &Document,
        view: &View,
        theme: &Theme,
    ) -> Option<OverlayHighlights> {
        if !crate::sm_misc::nav_flash_enabled() {
            return None;
        }
        let text = doc.text().slice(..);
        let cursor = doc.selection(view.id).primary().cursor(text);
        let line = text.char_to_line(cursor);
        crate::sm_misc::note_cursor(view.id, doc.id(), line);

        let flashing = crate::sm_misc::flashing_line(doc.id())?;
        if flashing >= text.len_lines() {
            return None;
        }
        let highlight = theme
            .find_highlight_exact("ui.cursorline.primary")
            .or_else(|| theme.find_highlight_exact("ui.highlight"))?;
        let start = text.line_to_char(flashing);
        let end = text.line_to_char((flashing + 1).min(text.len_lines()));
        Some(OverlayHighlights::Homogeneous {
            highlight,
            ranges: std::iter::once(start..end).collect(),
        })
    }

    /// Emacs `goto-address-mode`: buttonize the URLs and e-mail addresses in the
    /// visible lines, so an address in a comment reads as the link it is.
    pub fn doc_goto_address_highlights(
        doc: &Document,
        view: &View,
        theme: &Theme,
    ) -> Option<OverlayHighlights> {
        if !crate::commands::goto_address_enabled(doc.id()) {
            return None;
        }
        let highlight = theme
            .find_highlight_exact("markup.link.url")
            .or_else(|| theme.find_highlight_exact("markup.link"))
            .or_else(|| theme.find_highlight_exact("ui.highlight"))?;

        let text = doc.text().slice(..);
        let view_offset = doc.view_offset(view.id);
        let height = view.inner_area(doc).height as usize;
        let first_line = text.char_to_line(view_offset.anchor.min(text.len_chars()));
        let last_line = (first_line + height + 1).min(text.len_lines());

        let mut ranges = Vec::new();
        for line in first_line..last_line {
            let src = text.line(line).to_string();
            let line_start = text.line_to_char(line);
            for address in zmax_core::goto_address::addresses(&src) {
                // `addresses` reports byte offsets; the renderer needs chars.
                let start = src[..address.range.start].chars().count();
                let end = src[..address.range.end].chars().count();
                ranges.push(line_start + start..line_start + end);
            }
        }
        ranges.sort_by_key(|r| r.start);
        (!ranges.is_empty()).then_some(OverlayHighlights::Homogeneous { highlight, ranges })
    }

    /// Emacs `bug-reference-mode`: buttonize the bug references in the visible
    /// lines, so `Bug#1234` in a comment reads as the tracker link it is.
    ///
    /// `bug-reference-prog-mode` keeps only the references inside a comment or a
    /// string, through the same `comment_string_spans_in` the `flyspell-prog-mode`
    /// path uses — the restriction is the one difference between the two modes.
    pub fn doc_bug_reference_highlights(
        doc: &Document,
        view: &View,
        theme: &Theme,
    ) -> Option<OverlayHighlights> {
        let prog = crate::commands::bug_reference_enabled(doc.id())?;
        let highlight = theme
            .find_highlight_exact("markup.link.url")
            .or_else(|| theme.find_highlight_exact("markup.link"))
            .or_else(|| theme.find_highlight_exact("ui.highlight"))?;

        let text = doc.text().slice(..);
        let view_offset = doc.view_offset(view.id);
        let height = view.inner_area(doc).height as usize;
        let first_line = text.char_to_line(view_offset.anchor.min(text.len_chars()));
        let last_line = (first_line + height + 1).min(text.len_lines());

        let mut ranges = Vec::new();
        for line in first_line..last_line {
            let src = text.line(line).to_string();
            let line_start = text.line_to_char(line);
            for reference in zmax_core::bug_reference::references(&src) {
                // `references` reports byte offsets; the renderer needs chars.
                let start = src[..reference.range.start].chars().count();
                let end = src[..reference.range.end].chars().count();
                ranges.push(line_start + start..line_start + end);
            }
        }
        ranges.sort_by_key(|r| r.start);

        if prog {
            let scan_start = text.line_to_char(first_line);
            let scan_end = text.line_to_char(last_line);
            let prose = crate::commands::comment_string_spans_in(doc, scan_start, scan_end);
            ranges.retain(|r| {
                prose
                    .iter()
                    .any(|&(from, to)| r.start >= from && r.end <= to)
            });
        }

        (!ranges.is_empty()).then_some(OverlayHighlights::Homogeneous { highlight, ranges })
    }

    /// Emacs `highlight-changes-mode`: paint the regions that differ from the
    /// text the mode was armed over, so recent edits stand out. Only the visible
    /// lines are painted; the diff itself is over the whole buffer, which is
    /// what makes a change scrolled into view show up.
    pub fn doc_highlight_changes(
        doc: &Document,
        view: &View,
        theme: &Theme,
    ) -> Option<OverlayHighlights> {
        let text = doc.text();
        let mut ranges = crate::highlight_changes::ranges_for(doc.id(), text)?;
        if ranges.is_empty() {
            return None;
        }
        // Emacs' `highlight-changes` face is a background tint; `ui.highlight`
        // is zmax's equivalent and is what the other "this region matters"
        // overlays use.
        let highlight = theme
            .find_highlight_exact("diff.delta")
            .or_else(|| theme.find_highlight_exact("ui.highlight"))?;

        let slice = text.slice(..);
        let view_offset = doc.view_offset(view.id);
        let height = view.inner_area(doc).height as usize;
        let first_line = slice.char_to_line(view_offset.anchor.min(slice.len_chars()));
        let last_line = (first_line + height + 1).min(slice.len_lines());
        let from = slice.line_to_char(first_line);
        let to = slice.line_to_char(last_line);
        ranges.retain(|r| r.start < to && from < r.end);
        (!ranges.is_empty()).then_some(OverlayHighlights::Homogeneous { highlight, ranges })
    }

    /// vim `spell`: underline misspelled words in the visible viewport when
    /// `:set spell` is active. Uses the existing spell engine (`crate::spell`);
    /// scans only the visible line range so it stays cheap on large files.
    ///
    /// vim `spelloptions=camel` splits `fooBar` into `foo` and `Bar` before the
    /// check (so identifiers stop being flagged wholesale), and vim
    /// `spellcapcheck` additionally flags a word that starts a sentence without a
    /// capital.
    pub fn doc_spell_highlights(
        doc: &Document,
        view: &View,
        theme: &Theme,
    ) -> Option<OverlayHighlights> {
        // Emacs `flyspell-mode` / `flyspell-prog-mode` share this renderer with
        // vim `:set spell`: either arms the underline. `flyspell-prog-mode`
        // additionally restricts it to the buffer's comments and strings.
        let fly = crate::spell::flyspell(doc.id());
        if !crate::commands::vim_opt_bool("spell") && fly == crate::spell::Flyspell::Off {
            return None;
        }
        let camel = crate::commands::typed::vim_opt_str("spelloptions")
            .is_some_and(|opts| opts.split(',').any(|o| o.trim() == "camel"));
        let capcheck = crate::commands::typed::vim_opt_str("spellcapcheck")
            .map(|v| spellcap_end_chars(&v))
            .unwrap_or_default();
        let text = doc.text().slice(..);
        if text.len_chars() == 0 {
            return None;
        }
        let view_offset = doc.view_offset(view.id);
        let height = view.inner_area(doc).height as usize;
        let first_line = text.char_to_line(view_offset.anchor);
        let last_line = (first_line + height + 1).min(text.len_lines());
        let scan_start = text.line_to_char(first_line);
        let scan_end = text.line_to_char(last_line);
        let haystack: Vec<char> = text.slice(scan_start..scan_end).chars().collect();

        // Tokenize into words (alphabetic runs, apostrophes allowed inside) and
        // flag the misspelled ones.
        let mut ranges: Vec<ops::Range<usize>> = Vec::new();
        let mut i = 0;
        // vim `spellcapcheck`: set once a sentence-ending character is seen, so
        // the next word must start with a capital.
        let mut want_capital = !capcheck.is_empty();
        while i < haystack.len() {
            if haystack[i].is_alphabetic() {
                let start = i;
                while i < haystack.len() && (haystack[i].is_alphabetic() || haystack[i] == '\'') {
                    i += 1;
                }
                // Trim trailing apostrophes so `word'` checks `word`.
                let mut end = i;
                while end > start && haystack[end - 1] == '\'' {
                    end -= 1;
                }
                // vim `spellcapcheck`: a sentence must not open with a lowercase
                // word (vim's `SpellCap`).
                if want_capital && haystack[start].is_lowercase() {
                    ranges.push((scan_start + start)..(scan_start + end));
                }
                want_capital = false;

                // vim `spelloptions=camel`: check each camel-case part on its own.
                let parts = if camel {
                    camel_parts(&haystack[start..end])
                } else {
                    vec![(0, haystack[start..end].iter().collect::<String>())]
                };
                for (offset, word) in parts {
                    if word.chars().count() >= 2 && crate::spell::is_misspelled(&word) {
                        let from = start + offset;
                        ranges
                            .push((scan_start + from)..(scan_start + from + word.chars().count()));
                    }
                }
            } else {
                if capcheck.contains(&haystack[i]) {
                    want_capital = true;
                }
                i += 1;
            }
        }
        ranges.sort_by_key(|r| r.start);
        ranges.dedup();

        // `flyspell-prog-mode`: keep only the words inside a comment or a string
        // literal, so identifiers and keywords are never flagged.
        if fly == crate::spell::Flyspell::Prog {
            let prose = crate::commands::comment_string_spans_in(doc, scan_start, scan_end);
            ranges.retain(|r| {
                prose
                    .iter()
                    .any(|&(from, to)| r.start >= from && r.end <= to)
            });
        }

        if ranges.is_empty() {
            return None;
        }
        let highlight = theme
            .find_highlight_exact("diagnostic.spell")
            .or_else(|| theme.find_highlight_exact("diagnostic.error"))
            .or_else(|| theme.find_highlight_exact("diagnostic"))?;
        Some(OverlayHighlights::Homogeneous { highlight, ranges })
    }

    /// vim `hlsearch`: highlight every match of the last search pattern (register
    /// `/`) in the visible viewport. Off unless `editor.search_highlight` is set.
    pub fn doc_search_highlights(
        editor: &Editor,
        doc: &Document,
        view: &View,
        theme: &Theme,
    ) -> Option<OverlayHighlights> {
        use zmax_stdx::rope::{Config, RegexBuilder, RopeSliceExt};
        if !editor.config().search_highlight {
            return None;
        }
        let pattern = editor.registers.first('/', editor)?;
        if pattern.is_empty() {
            return None;
        }
        let text = doc.text().slice(..);
        if text.len_chars() == 0 {
            return None;
        }
        let case_insensitive = if editor.config().search.smart_case {
            !pattern.chars().any(char::is_uppercase)
        } else {
            false
        };
        let is_crlf = doc.line_ending == zmax_core::LineEnding::Crlf;
        let regex = RegexBuilder::new()
            .syntax(
                Config::new()
                    .case_insensitive(case_insensitive)
                    .multi_line(true)
                    .crlf(is_crlf),
            )
            .build(&pattern)
            .ok()?;

        let view_offset = doc.view_offset(view.id);
        let height = view.inner_area(doc).height as usize;
        let first_line = text.char_to_line(view_offset.anchor.min(text.len_chars()));
        let last_line = (first_line + height + 1).min(text.len_lines());
        let start_byte = text.char_to_byte(text.line_to_char(first_line));
        let end_byte = text.char_to_byte(text.line_to_char(last_line));

        // vim `redrawtime`: the time this highlight pass may take before it gives
        // up (vim's guard against a pattern that is too slow to redraw with).
        // Unset — the default — means no budget, as before.
        let budget = crate::commands::typed::vim_opt_num("redrawtime");
        let started = std::time::Instant::now();

        let mut ranges = Vec::new();
        for (i, m) in regex
            .find_iter(text.regex_input_at_bytes(start_byte..end_byte))
            .enumerate()
        {
            // Checking the clock every match would cost more than the search.
            if i % 256 == 0 && over_redrawtime(started.elapsed().as_millis(), budget) {
                break;
            }
            if m.start() == m.end() {
                continue; // skip zero-width matches
            }
            ranges.push(text.byte_to_char(m.start())..text.byte_to_char(m.end()));
            if ranges.len() >= 10_000 {
                break; // viewport safety cap
            }
        }
        if ranges.is_empty() {
            return None;
        }
        let highlight = theme
            .find_highlight_exact("ui.cursor.match")
            .or_else(|| theme.find_highlight_exact("ui.highlight"))?;
        Some(OverlayHighlights::Homogeneous { highlight, ranges })
    }

    pub fn doc_word_occurrence_highlights(
        doc: &Document,
        view: &View,
        theme: &Theme,
    ) -> Option<OverlayHighlights> {
        use zmax_core::chars::char_is_word;

        let text = doc.text().slice(..);
        let len = text.len_chars();
        if len == 0 {
            return None;
        }

        // Expand around the primary cursor to the word boundaries.
        let pos = doc.selection(view.id).primary().cursor(text);
        if pos >= len || !char_is_word(text.char(pos)) {
            return None;
        }
        let mut start = pos;
        while start > 0 && char_is_word(text.char(start - 1)) {
            start -= 1;
        }
        let mut end = pos;
        while end < len && char_is_word(text.char(end)) {
            end += 1;
        }
        let word: String = text.slice(start..end).chars().collect();

        let highlight = theme
            .find_highlight_exact("ui.highlight.word")
            .or_else(|| theme.find_highlight_exact("ui.highlight"))
            .or_else(|| theme.find_highlight_exact("ui.cursor.match"))?;

        // Restrict the scan to the visible line range.
        let view_offset = doc.view_offset(view.id);
        let height = view.inner_area(doc).height as usize;
        let first_line = text.char_to_line(view_offset.anchor);
        let last_line = (first_line + height + 1).min(text.len_lines());
        let scan_start = text.line_to_char(first_line);
        let scan_end = text.line_to_char(last_line);
        let haystack: String = text.slice(scan_start..scan_end).chars().collect();

        let mut ranges: Vec<ops::Range<usize>> = Vec::new();
        for (byte_idx, _) in haystack.match_indices(&word) {
            // Whole-word check: neighbours must not be word characters.
            let before_ok = haystack[..byte_idx]
                .chars()
                .next_back()
                .is_none_or(|c| !char_is_word(c));
            let after_ok = haystack[byte_idx + word.len()..]
                .chars()
                .next()
                .is_none_or(|c| !char_is_word(c));
            if !before_ok || !after_ok {
                continue;
            }
            let match_start = scan_start + haystack[..byte_idx].chars().count();
            let match_end = match_start + word.chars().count();
            ranges.push(match_start..match_end);
        }

        if ranges.is_empty() {
            return None;
        }

        Some(OverlayHighlights::Homogeneous { highlight, ranges })
    }

    pub fn doc_document_link_highlights(
        doc: &Document,
        theme: &Theme,
    ) -> Option<OverlayHighlights> {
        let highlight = theme
            .find_highlight_exact("markup.link.url")
            .or_else(|| theme.find_highlight_exact("markup.link"))?;

        if doc.document_links.is_empty() {
            return None;
        }

        let mut ranges: Vec<ops::Range<usize>> = Vec::new();
        for link in &doc.document_links {
            if link.start >= link.end {
                continue;
            }

            match ranges.last_mut() {
                Some(existing_range) if link.start <= existing_range.end => {
                    existing_range.end = existing_range.end.max(link.end);
                }
                _ => ranges.push(link.start..link.end),
            }
        }

        if ranges.is_empty() {
            return None;
        }

        Some(OverlayHighlights::Homogeneous { highlight, ranges })
    }

    /// Get highlight spans for selections in a document view.
    pub fn doc_selection_highlights(
        mode: Mode,
        doc: &Document,
        view: &View,
        theme: &Theme,
        cursor_shape_config: &CursorShapeConfig,
        is_terminal_focused: bool,
    ) -> OverlayHighlights {
        let text = doc.text().slice(..);
        let selection = doc.selection(view.id);
        let primary_idx = selection.primary_index();

        let cursorkind = cursor_shape_config.from_mode(mode);
        let cursor_is_block = cursorkind == CursorKind::Block;

        let selection_scope = theme
            .find_highlight_exact("ui.selection")
            .expect("could not find `ui.selection` scope in the theme!");
        let primary_selection_scope = theme
            .find_highlight_exact("ui.selection.primary")
            .unwrap_or(selection_scope);

        let base_cursor_scope = theme
            .find_highlight_exact("ui.cursor")
            .unwrap_or(selection_scope);
        let base_primary_cursor_scope = theme
            .find_highlight("ui.cursor.primary")
            .unwrap_or(base_cursor_scope);

        let cursor_scope = match mode {
            Mode::Insert => theme.find_highlight_exact("ui.cursor.insert"),
            Mode::Select => theme.find_highlight_exact("ui.cursor.select"),
            Mode::Normal => theme.find_highlight_exact("ui.cursor.normal"),
        }
        .unwrap_or(base_cursor_scope);

        let primary_cursor_scope = match mode {
            Mode::Insert => theme.find_highlight_exact("ui.cursor.primary.insert"),
            Mode::Select => theme.find_highlight_exact("ui.cursor.primary.select"),
            Mode::Normal => theme.find_highlight_exact("ui.cursor.primary.normal"),
        }
        .unwrap_or(base_primary_cursor_scope);

        // Emacs `transient-mark-mode`: with it off, the region is still there and
        // every region command still acts on it — it is simply not shaded. The
        // cursor is drawn either way.
        let shade_region = crate::commands::transient_mark_enabled();

        let mut spans = Vec::new();
        for (i, range) in selection.iter().enumerate() {
            let selection_is_primary = i == primary_idx;
            let (cursor_scope, selection_scope) = if selection_is_primary {
                (primary_cursor_scope, primary_selection_scope)
            } else {
                (cursor_scope, selection_scope)
            };

            // Special-case: cursor at end of the rope.
            if range.head == range.anchor && range.head == text.len_chars() {
                if !selection_is_primary || (cursor_is_block && is_terminal_focused) {
                    // Bar and underline cursors are drawn by the terminal
                    // BUG: If the editor area loses focus while having a bar or
                    // underline cursor (eg. when a regex prompt has focus) then
                    // the primary cursor will be invisible. This doesn't happen
                    // with block cursors since we manually draw *all* cursors.
                    spans.push((cursor_scope, range.head..range.head + 1));
                }
                continue;
            }

            let range = range.min_width_1(text);
            if range.head > range.anchor {
                // Standard case.
                let cursor_start = prev_grapheme_boundary(text, range.head);
                // non block cursors look like they exclude the cursor
                let selection_end =
                    if selection_is_primary && !cursor_is_block && mode != Mode::Insert {
                        range.head
                    } else {
                        cursor_start
                    };
                if shade_region {
                    spans.push((selection_scope, range.anchor..selection_end));
                }
                // add block cursors
                // skip primary cursor if terminal is unfocused - terminal cursor is used in that case
                if !selection_is_primary || (cursor_is_block && is_terminal_focused) {
                    spans.push((cursor_scope, cursor_start..range.head));
                }
            } else {
                // Reverse case.
                let cursor_end = next_grapheme_boundary(text, range.head);
                // add block cursors
                // skip primary cursor if terminal is unfocused - terminal cursor is used in that case
                if !selection_is_primary || (cursor_is_block && is_terminal_focused) {
                    spans.push((cursor_scope, range.head..cursor_end));
                }
                // non block cursors look like they exclude the cursor
                let selection_start = if selection_is_primary
                    && !cursor_is_block
                    && !(mode == Mode::Insert && cursor_end == range.anchor)
                {
                    range.head
                } else {
                    cursor_end
                };
                if shade_region {
                    spans.push((selection_scope, selection_start..range.anchor));
                }
            }
        }

        OverlayHighlights::Heterogenous { highlights: spans }
    }

    /// Render brace match, etc (meant for the focused view only)
    pub fn highlight_focused_view_elements(
        view: &View,
        doc: &Document,
        theme: &Theme,
    ) -> Option<OverlayHighlights> {
        // Emacs `show-paren-mode` / `show-paren-local-mode` gate this overlay.
        if !crate::commands::show_paren_enabled(doc.id()) {
            return None;
        }
        // Highlight matching braces
        let syntax = doc.syntax()?;
        let highlight = theme.find_highlight_exact("ui.cursor.match")?;
        let text = doc.text().slice(..);
        let pos = doc.selection(view.id).primary().cursor(text);
        let pos = zmax_core::match_brackets::find_matching_bracket(syntax, text, pos)?;
        Some(OverlayHighlights::single(highlight, pos..pos + 1))
    }

    /// vim `showmatch`: the bracket matching the closing one just typed stays
    /// highlighted for `matchtime` tenths of a second (default 5 = half a
    /// second), then the flash expires on its own.
    fn showmatch_highlight(
        editor: &Editor,
        doc: &Document,
        theme: &Theme,
    ) -> Option<OverlayHighlights> {
        let (doc_id, pos, armed) = editor.show_match?;
        if doc_id != doc.id() {
            return None;
        }
        let tenths = crate::commands::vim_opt_num("matchtime").unwrap_or(5) as u64;
        if armed.elapsed() >= std::time::Duration::from_millis(tenths * 100) {
            return None;
        }
        let highlight = theme.find_highlight_exact("ui.cursor.match")?;
        Some(OverlayHighlights::single(highlight, pos..pos + 1))
    }

    /// spacemacs `nav-flash`: after a navigation command the line the cursor
    /// landed on is highlighted (`nav-flash-face`, which inherits `highlight`)
    /// for `nav-flash-delay`, then the flash expires on its own.
    fn nav_flash_highlight(&self, doc: &Document, theme: &Theme) -> Option<OverlayHighlights> {
        let (doc_id, line, armed) = self.nav_flash?;
        if doc_id != doc.id() || armed.elapsed() >= NAV_FLASH_DELAY {
            return None;
        }
        let text = doc.text();
        if line >= text.len_lines() {
            return None;
        }
        let highlight = theme
            .find_highlight_exact("ui.cursorline.primary")
            .or_else(|| theme.find_highlight_exact("ui.selection"))?;
        // `nav-flash-show` covers the line plus one character past its end, so
        // the newline is part of the flash (the face is `:extend t`).
        let start = text.line_to_char(line);
        let end = text.line_to_char((line + 1).min(text.len_lines()));
        Some(OverlayHighlights::single(highlight, start..end))
    }

    pub fn tabstop_highlights(doc: &Document, theme: &Theme) -> Option<OverlayHighlights> {
        let snippet = doc.active_snippet.as_ref()?;
        let highlight = theme.find_highlight_exact("tabstop")?;
        let mut ranges = Vec::new();
        for tabstop in snippet.tabstops() {
            ranges.extend(tabstop.ranges.iter().map(|range| range.start..range.end));
        }
        Some(OverlayHighlights::Homogeneous { highlight, ranges })
    }

    /// Render bufferline at the top. Returns `(tabs, new_button)` where each tab is
    /// `(x_start, x_end, close_x, doc)` (`close_x` = the `×` column) and `new_button`
    /// is the `(x_start, x_end)` of the trailing `+` new-buffer button.
    /// The first visible line and the visible line count of a window, which is
    /// what both scroll bars measure the buffer against.
    fn scroll_bar_extent(doc: &Document, view: &View) -> (usize, usize, usize) {
        let text = doc.text().slice(..);
        let anchor = doc.view_offset(view.id).anchor;
        let top = text.char_to_line(anchor.min(text.len_chars()));
        (top, text.len_lines(), view.inner_height())
    }

    /// Draw the window's scroll bars into the strips `View::inner_area` reserved.
    /// The vertical bar's thumb covers the fraction of the buffer the window is
    /// showing, at the same fraction down the bar (emacs's own proportions); the
    /// horizontal bar does the same for the longest visible line against the
    /// window's horizontal offset.
    fn render_scroll_bars(doc: &Document, view: &View, surface: &mut Surface, theme: &Theme) {
        let track = theme.get("ui.menu");
        let thumb = theme.get("ui.menu.selected");

        let bar = view.scroll_bar_area(doc);
        if bar.width > 0 && bar.height > 0 {
            let (top, total, visible) = Self::scroll_bar_extent(doc, view);
            let (start, end) =
                crate::emacs_frame::thumb_range(bar.height as usize, total, top, visible);
            for (i, y) in (bar.y..bar.bottom()).enumerate() {
                let inside = i >= start && i < end;
                surface[(bar.x, y)]
                    .set_symbol(if inside { "█" } else { "│" })
                    .set_style(if inside { thumb } else { track });
            }
        }

        let hbar = view.horizontal_scroll_bar_area();
        if hbar.height > 0 && hbar.width > 0 {
            // The horizontal extent emacs measures is the widest line the window
            // could show; the visible span is the window's own width.
            let offset = doc.view_offset(view.id).horizontal_offset;
            let visible = view.inner_width(doc) as usize;
            let total = (offset + visible).max(visible);
            let (start, end) =
                crate::emacs_frame::thumb_range(hbar.width as usize, total, offset, visible);
            for (i, x) in (hbar.x..hbar.right()).enumerate() {
                let inside = i >= start && i < end;
                surface[(x, hbar.y)]
                    .set_symbol(if inside { "█" } else { "─" })
                    .set_style(if inside { thumb } else { track });
            }
        }
    }

    /// Which `[Label]` of a button row the column `x` is over. Mirrors the widths
    /// `render_button_row` lays out, so the hit regions never have to be stored.
    fn button_hit(area: Rect, labels: impl Iterator<Item = &'static str>, x: u16) -> Option<usize> {
        let mut left = area.x;
        for (i, label) in labels.enumerate() {
            let width = label.chars().count() as u16 + 2;
            if left + width > area.right() {
                return None;
            }
            if x >= left && x < left + width {
                return Some(i);
            }
            left += width;
        }
        None
    }

    /// Draw a row of `[Label]` cells across `viewport` and return each one's
    /// `(x_start, x_end, index)` hit region. This is how the emacs menu bar, tool
    /// bar, modifier bar and window tool bar all render — a terminal has no icons,
    /// so each button is its label between brackets, and a click is resolved by
    /// looking the column up in the returned regions.
    fn render_button_row<'a>(
        viewport: Rect,
        surface: &mut Surface,
        base: Style,
        labels: impl Iterator<Item = (&'a str, bool)>,
        active: Style,
    ) -> Vec<(u16, u16, usize)> {
        surface.clear_with(viewport, base);
        let mut hits = Vec::new();
        let mut x = viewport.x;
        for (i, (label, lit)) in labels.enumerate() {
            let cell = format!(" {label} ");
            let width = cell.chars().count() as u16;
            if x + width > viewport.right() {
                break;
            }
            let style = if lit { active } else { base };
            surface.set_stringn(x, viewport.y, &cell, width as usize, style);
            hits.push((x, x + width, i));
            x += width;
        }
        hits
    }

    /// Draw the frame-wide bars — emacs's menu bar, tool bar and modifier bar —
    /// into the rows `render` reserved for them, and remember each button's hit
    /// region so a click can be routed. `area` is exactly `frame_bar_rows()` tall,
    /// so a mode that is off contributes no row and nothing is drawn for it.
    fn render_frame_bars(&mut self, area: Rect, surface: &mut Surface, cx: &mut Context) {
        self.menu_bar_hits.clear();
        self.tool_bar_hits.clear();
        self.modifier_bar_hits.clear();
        if area.height == 0 {
            return;
        }
        let theme = &cx.editor.theme;
        let base = theme.get("ui.menu");
        let active = theme.get("ui.menu.selected");
        let mut y = area.y;

        if crate::emacs_frame::menu_bar() {
            let row = Rect {
                y,
                height: 1,
                ..area
            };
            let titles = crate::commands::menu_bar_titles();
            self.menu_bar_hits = Self::render_button_row(
                row,
                surface,
                base,
                titles.iter().map(|t| (*t, false)),
                active,
            );
            self.menu_bar_y = y;
            y += 1;
        }

        if crate::emacs_frame::tool_bar() {
            let row = Rect {
                y,
                height: 1,
                ..area
            };
            self.tool_bar_hits = Self::render_button_row(
                row,
                surface,
                base,
                crate::emacs_frame::TOOL_BAR_BUTTONS
                    .iter()
                    .map(|(label, _)| (*label, false)),
                active,
            );
            self.tool_bar_y = y;
            y += 1;
        }

        if crate::emacs_frame::modifier_bar() {
            let row = Rect {
                y,
                height: 1,
                ..area
            };
            // A latched modifier is drawn lit, which is how emacs shows that the
            // next key will carry it.
            let sticky = crate::emacs_frame::sticky_modifiers();
            self.modifier_bar_hits = Self::render_button_row(
                row,
                surface,
                base,
                crate::emacs_frame::MODIFIER_BAR_BUTTONS
                    .iter()
                    .map(|(label, m)| (*label, sticky.contains(*m))),
                active,
            );
            self.modifier_bar_y = y;
        }
    }

    /// Columns a tabline string occupies once drawn.
    fn str_width(s: &str) -> u16 {
        s.width() as u16
    }

    /// Index of the first buffer the tabline shows, so the current one is always
    /// on screen: the row scrolls by whole pills, keeping as many buffers to the
    /// left of the current one as still fit. `widths` are the drawn pill widths,
    /// `budget` the columns the pills may use.
    fn bufferline_scroll(widths: &[u16], current: Option<usize>, budget: u16) -> usize {
        let Some(current) = current else { return 0 };
        // Walk back from the current buffer while its predecessors fit, then show
        // from there — the same window airline slides along its tabline.
        let mut first = current;
        let mut used = widths.get(current).copied().unwrap_or(0);
        while first > 0 {
            let next = used + widths[first - 1];
            if next > budget {
                break;
            }
            used = next;
            first -= 1;
        }
        first
    }

    /// vim-airline's tabline: the open buffers as powerline pills across the top
    /// row, the current one in the accent colour, `…` where the row runs out of
    /// buffers to show, and airline's right-hand `buffers` label.
    ///
    /// The pills carry the same click targets the plain bufferline had — the tab
    /// body switches, the `×` closes, the trailing `+` opens a scratch buffer —
    /// so the hit boxes this returns keep their meaning: `(start, end, close_x,
    /// doc)` with the close zone at `close_x..end`.
    pub fn render_bufferline(
        editor: &Editor,
        viewport: Rect,
        surface: &mut Surface,
    ) -> (BufferlineTabs, (u16, u16)) {
        const SEP_R: &str = "\u{e0b0}"; //  pill → pill, points right
        const SEP_R_THIN: &str = "\u{e0b1}"; //  same-coloured neighbours
        const SEP_L: &str = "\u{e0b2}"; //  fill → label, points left
        const LABEL: &str = " buffers ";

        let scratch = PathBuf::from(SCRATCH_BUFFER_NAME); // default filename to use for scratch buffer
        let fill_style = editor
            .theme
            .try_get("ui.bufferline.background")
            .unwrap_or_else(|| editor.theme.get("ui.statusline"));
        surface.clear_with(viewport, fill_style);

        // A pill's separator is drawn in the pill's own background, so a theme
        // that gives the bufferline a foreground only would draw every arrow
        // invisibly. Borrow the matching status line background in that case.
        let with_bg = |style: Style, fallback: &str| match style.bg {
            Some(_) => style,
            None => match editor.theme.get(fallback).bg {
                Some(bg) => style.bg(bg),
                None => style,
            },
        };
        let active_style = with_bg(
            editor
                .theme
                .try_get("ui.bufferline.active")
                .unwrap_or_else(|| editor.theme.get("ui.statusline.active")),
            "ui.statusline.active",
        );
        let inactive_style = with_bg(
            editor
                .theme
                .try_get("ui.bufferline")
                .unwrap_or_else(|| editor.theme.get("ui.statusline.inactive")),
            "ui.statusline.inactive",
        );
        // airline paints its tabline label in the mode colour; fall back to the
        // active pill's colours for themes that don't style the mode.
        let label_style = editor
            .theme
            .try_get("ui.statusline.normal")
            .unwrap_or(active_style);
        let fill = fill_style.bg;
        // airline's two separators: the solid arrow where the colour changes, and
        // the thin one between neighbours that share a background — a solid arrow
        // there would be drawn in its own background colour and vanish.
        let sep = |from: Style, to_bg: Option<Color>| {
            let from_bg = from.bg.or(fill);
            let to_bg = to_bg.or(fill);
            if from_bg == to_bg {
                let style = Style {
                    fg: from.fg.or(fill),
                    bg: from_bg,
                    ..Default::default()
                };
                (SEP_R_THIN, style)
            } else {
                let style = Style {
                    fg: from_bg,
                    bg: to_bg,
                    ..Default::default()
                };
                (SEP_R, style)
            }
        };

        let current_doc = view!(editor).doc;
        // Buffer-line order, not DocumentId order: `>b`/`<b` and the sort
        // commands rearrange `Editor::buffer_order`, and the bar is what they
        // rearrange.
        let entries: Vec<(zmax_view::DocumentId, String)> = editor
            .ordered_documents()
            .map(|doc| {
                let fname = doc
                    .path()
                    .unwrap_or(&scratch)
                    .file_name()
                    .unwrap_or_default()
                    .to_str()
                    .unwrap_or_default();
                (
                    doc.id(),
                    format!(
                        " {} {}{} ",
                        super::icons::file_icon(fname),
                        fname,
                        if doc.is_modified() { "[+]" } else { "" }
                    ),
                )
            })
            .collect();
        let current = entries.iter().position(|(id, _)| *id == current_doc);

        // The label sits at the right edge and the pills get what is left of the
        // row. Everything below measures against this budget, so a long list
        // scrolls instead of running under the label.
        let right_edge = viewport.right();
        let label_width = Self::str_width(LABEL) + 1; // + its  separator
        let pills_end = right_edge.saturating_sub(label_width).max(viewport.x);

        // Pill widths: the label, the `×` close cell, and the  that follows.
        let widths: Vec<u16> = entries
            .iter()
            .map(|(_, text)| Self::str_width(text) + Self::str_width("× ") + 1)
            .collect();
        let first = Self::bufferline_scroll(&widths, current, pills_end.saturating_sub(viewport.x));

        let mut tabs = Vec::new();
        let mut x = viewport.x;
        // `‹` where buffers are scrolled off the left, so the row never lies
        // about being the whole list.
        if first > 0 && x < pills_end {
            x = surface
                .set_stringn(x, viewport.y, "‹", (pills_end - x) as usize, inactive_style)
                .0;
        }
        let mut overflow = false;
        for (i, (doc_id, text)) in entries.iter().enumerate().skip(first) {
            if x + widths[i] > pills_end {
                overflow = true;
                break;
            }
            let style = if *doc_id == current_doc {
                active_style
            } else {
                inactive_style
            };
            let start = x;
            let close_x = surface
                .set_stringn(x, viewport.y, text, (pills_end - x) as usize, style)
                .0;
            x = surface
                .set_stringn(
                    close_x,
                    viewport.y,
                    "× ",
                    (pills_end - close_x) as usize,
                    style,
                )
                .0;
            tabs.push((start, x, close_x, *doc_id));
            // The separator takes the next pill's background so the two pills
            // meet in one solid arrow, exactly as the powerline status bar joins
            // its segments.
            // At the end of the row (or where the next pill no longer fits) it
            // fades into the bar's own fill instead.
            let next_fits = widths.get(i + 1).is_some_and(|w| x + 1 + w <= pills_end);
            let next_bg = match entries.get(i + 1) {
                Some((id, _)) if next_fits => {
                    if *id == current_doc {
                        active_style.bg
                    } else {
                        inactive_style.bg
                    }
                }
                _ => fill,
            };
            let (glyph, sep_style) = sep(style, next_bg);
            x = surface
                .set_stringn(
                    x,
                    viewport.y,
                    glyph,
                    (pills_end.saturating_sub(x)) as usize,
                    sep_style,
                )
                .0;
        }
        if overflow && x < pills_end {
            x = surface
                .set_stringn(x, viewport.y, "…", (pills_end - x) as usize, inactive_style)
                .0;
        }

        // Trailing "+" new-buffer button, only when the row still has room for it.
        let new_btn = if x + Self::str_width(" + ") <= pills_end {
            let start = x;
            x = surface
                .set_stringn(
                    x,
                    viewport.y,
                    " + ",
                    (pills_end - x) as usize,
                    inactive_style,
                )
                .0;
            (start, x)
        } else {
            (0, 0)
        };

        // airline's right-hand label, drawn last so it always owns its columns.
        // Its separator points the other way: it comes out of the bar's fill and
        // into the label, so it wears the label's background as its foreground.
        let label_x = right_edge.saturating_sub(label_width);
        if label_x >= viewport.x {
            let label_sep = Style {
                fg: label_style.bg.or(fill),
                bg: fill,
                ..Default::default()
            };
            let after = surface
                .set_stringn(
                    label_x,
                    viewport.y,
                    SEP_L,
                    (right_edge - label_x) as usize,
                    label_sep,
                )
                .0;
            surface.set_stringn(
                after,
                viewport.y,
                LABEL,
                (right_edge - after) as usize,
                label_style,
            );
        }

        (tabs, new_btn)
    }

    pub fn render_gutter<'d>(
        editor: &'d Editor,
        doc: &'d Document,
        view: &View,
        viewport: Rect,
        theme: &Theme,
        is_focused: bool,
        decoration_manager: &mut DecorationManager<'d>,
    ) {
        let text = doc.text().slice(..);
        let cursors: Rc<[_]> = doc
            .selection(view.id)
            .iter()
            .map(|range| range.cursor_line(text))
            .collect();

        let mut offset = 0;

        let mut gutter_style = theme.get("ui.gutter");
        let mut gutter_selected_style = theme.get("ui.gutter.selected");
        let mut gutter_style_virtual = theme.get("ui.gutter.virtual");
        let mut gutter_selected_style_virtual = theme.get("ui.gutter.selected.virtual");
        // `transparent-background`: drop the gutter fills too so the sign column
        // follows the editor's transparent background instead of the theme bg.
        if editor.config().transparent_background {
            gutter_style.bg = None;
            gutter_selected_style.bg = None;
            gutter_style_virtual.bg = None;
            gutter_selected_style_virtual.bg = None;
        }

        for gutter_type in view.gutters() {
            let mut gutter = gutter_type.style(editor, doc, view, theme, is_focused);
            let width = gutter_type.width(view, doc);
            // avoid lots of small allocations by reusing a text buffer for each line
            let mut text = String::with_capacity(width);
            let cursors = cursors.clone();
            let gutter_decoration = move |renderer: &mut TextRenderer, pos: LinePos| {
                // TODO handle softwrap in gutters
                let selected = cursors.contains(&pos.doc_line);
                let x = viewport.x + offset;
                let y = pos.visual_line;

                let gutter_style = match (selected, pos.first_visual_line) {
                    (false, true) => gutter_style,
                    (true, true) => gutter_selected_style,
                    (false, false) => gutter_style_virtual,
                    (true, false) => gutter_selected_style_virtual,
                };

                if let Some(style) =
                    gutter(pos.doc_line, selected, pos.first_visual_line, &mut text)
                {
                    renderer.set_stringn(x, y, &text, width, gutter_style.patch(style));
                } else {
                    renderer.set_style(
                        Rect {
                            x,
                            y,
                            width: width as u16,
                            height: 1,
                        },
                        gutter_style,
                    );
                }
                text.clear();
            };
            decoration_manager.add_decoration(gutter_decoration);

            offset += width as u16;
        }
    }

    pub fn render_diagnostics(
        doc: &Document,
        view: &View,
        viewport: Rect,
        surface: &mut Surface,
        theme: &Theme,
    ) {
        use tui::{
            layout::Alignment,
            text::Text,
            widgets::{Paragraph, Widget, Wrap},
        };
        use zmax_core::diagnostic::Severity;

        let cursor = doc
            .selection(view.id)
            .primary()
            .cursor(doc.text().slice(..));

        let diagnostics = doc.diagnostics().iter().filter(|diagnostic| {
            diagnostic.range.start <= cursor && diagnostic.range.end >= cursor
        });

        let warning = theme.get("warning");
        let error = theme.get("error");
        let info = theme.get("info");
        let hint = theme.get("hint");

        let mut lines = Vec::new();
        let background_style = theme.get("ui.background");
        for diagnostic in diagnostics {
            let style = Style::reset()
                .patch(background_style)
                .patch(match diagnostic.severity {
                    Some(Severity::Error) => error,
                    Some(Severity::Warning) | None => warning,
                    Some(Severity::Info) => info,
                    Some(Severity::Hint) => hint,
                });
            let text = Text::styled(&diagnostic.message, style);
            lines.extend(text.lines);
            let code = diagnostic.code.as_ref().map(|x| match x {
                NumberOrString::Number(n) => format!("({n})"),
                NumberOrString::String(s) => format!("({s})"),
            });
            if let Some(code) = code {
                let span = Span::styled(code, style);
                lines.push(span.into());
            }
        }

        let text = Text::from(lines);
        let paragraph = Paragraph::new(&text)
            .alignment(Alignment::Right)
            .wrap(Wrap { trim: true });
        let width = 100.min(viewport.width);
        let height = 15.min(viewport.height);
        paragraph.render(
            Rect::new(viewport.right() - width, viewport.y + 1, width, height),
            surface,
        );
    }

    /// Apply the highlighting on the lines where a cursor is active
    pub fn cursorline(doc: &Document, view: &View, theme: &Theme) -> impl Decoration {
        let text = doc.text().slice(..);
        // TODO only highlight the visual line that contains the cursor instead of the full visual line
        let primary_line = doc.selection(view.id).primary().cursor_line(text);

        // The secondary_lines do contain the primary_line, it doesn't matter
        // as the else-if clause in the loop later won't test for the
        // secondary_lines if primary_line == line.
        // It's used inside a loop so the collect isn't needless:
        // https://github.com/rust-lang/rust-clippy/issues/6164
        #[allow(clippy::needless_collect)]
        let secondary_lines: Vec<_> = doc
            .selection(view.id)
            .iter()
            .map(|range| range.cursor_line(text))
            .collect();

        let primary_style = theme.get("ui.cursorline.primary");
        let secondary_style = theme.get("ui.cursorline.secondary");
        let viewport = view.area;

        move |renderer: &mut TextRenderer, pos: LinePos| {
            let area = Rect::new(viewport.x, pos.visual_line, viewport.width, 1);
            if primary_line == pos.doc_line {
                renderer.set_style(area, primary_style);
            } else if secondary_lines.binary_search(&pos.doc_line).is_ok() {
                renderer.set_style(area, secondary_style);
            }
        }
    }

    /// Apply the highlighting on the columns where a cursor is active
    pub fn highlight_cursorcolumn(
        doc: &Document,
        view: &View,
        surface: &mut Surface,
        theme: &Theme,
        viewport: Rect,
        text_annotations: &TextAnnotations,
    ) {
        let text = doc.text().slice(..);

        // Manual fallback behaviour:
        // ui.cursorcolumn.{p/s} -> ui.cursorcolumn -> ui.cursorline.{p/s}
        let primary_style = theme
            .try_get_exact("ui.cursorcolumn.primary")
            .or_else(|| theme.try_get_exact("ui.cursorcolumn"))
            .unwrap_or_else(|| theme.get("ui.cursorline.primary"));
        let secondary_style = theme
            .try_get_exact("ui.cursorcolumn.secondary")
            .or_else(|| theme.try_get_exact("ui.cursorcolumn"))
            .unwrap_or_else(|| theme.get("ui.cursorline.secondary"));

        let inner_area = view.inner_area(doc);

        let selection = doc.selection(view.id);
        let view_offset = doc.view_offset(view.id);
        let primary = selection.primary();
        let text_format = doc.text_format(viewport.width, None, Some(view.id));
        for range in selection.iter() {
            let is_primary = primary == *range;
            let cursor = range.cursor(text);

            let Position { col, .. } =
                visual_offset_from_block(text, cursor, cursor, &text_format, text_annotations).0;

            // if the cursor is horizontally in the view
            if col >= view_offset.horizontal_offset
                && inner_area.width > (col - view_offset.horizontal_offset) as u16
            {
                let area = Rect::new(
                    inner_area.x + (col - view_offset.horizontal_offset) as u16,
                    view.area.y,
                    1,
                    view.area.height,
                );
                if is_primary {
                    surface.set_style(area, primary_style)
                } else {
                    surface.set_style(area, secondary_style)
                }
            }
        }
    }

    /// vim 'timeout': the keys of the chord currently waiting for its next key
    /// (empty when nothing is pending). Read by the event loop, which arms the
    /// pending-chord timer with `typed::pending_key_timeout`.
    pub fn pending_keys(&self) -> &[KeyEvent] {
        self.keymaps.pending()
    }

    /// vim 'timeout'/'timeoutlen': the pending chord ran out of time. Drop it —
    /// the same cancellation `<Esc>` performs — and take the which-key popup down.
    pub fn cancel_pending_keys(&mut self, editor: &mut Editor) {
        if self.keymaps.pending().is_empty() {
            return;
        }
        self.keymaps.get(editor.mode(), key!(Esc));
        editor.autoinfo = None;
    }

    /// Handle events by looking them up in `self.keymaps`. Returns None
    /// if event was handled (a command was executed or a subkeymap was
    /// activated). Only KeymapResult::{NotFound, Cancelled} is returned
    /// otherwise.
    fn handle_keymap_event(
        &mut self,
        mode: Mode,
        cxt: &mut commands::Context,
        event: KeyEvent,
    ) -> Option<KeymapResult> {
        // vim 'langmap': the character keys that make up *commands* are translated
        // before the keymap is consulted, so a Greek/Cyrillic/Dvorak layout drives
        // every binding — built-in or user. Insert-mode text is left alone (that is
        // what `:lmap` / 'iminsert' are for).
        //
        // vim 'langremap' (`lrm`, default off) gates that: "When off, setting
        // 'langmap' does not apply to characters resulting from a mapping"
        // (options.txt). vim's gate is `(p_lrm || (!p_lrm && KeyTyped))`
        // (src/macros.h, `LANGMAP_ADJUST`), i.e. a key only reaches 'langmap' if
        // the user typed it or the option is on. zmax's "was typed" is "no macro
        // is replaying": a `:map` rhs key sequence (`MappableCommand::Macro`) and
        // a `@q` register both feed their keys back through this same handler with
        // `Editor::macro_replaying` marked for the duration, so that flag is what
        // separates a typed key from a mapping-produced one. (vim additionally
        // never langmaps *stuffed* keys — register execution — even under
        // 'langremap'; both kinds of replay look identical at this boundary, so
        // `:set langremap` translates both here.)
        let key_typed = cxt.editor.macro_replaying.is_empty();
        let event = if key_typed || crate::commands::typed::vim_opt_bool("langremap") {
            crate::commands::typed::langmap_translate(event, mode)
        } else {
            event
        };
        // Emacs `normal-erase-is-backspace-mode`: with the mode off, <backspace>
        // and <delete> trade places before anything looks them up.
        let event = crate::emacs_modes::erase_translate(event);
        let mut last_mode = mode;
        // While a which-key popup is up, PgDn/PgUp scroll/page it (large prefix
        // maps overflow) instead of being treated as bindings. Preserves the
        // pending key sequence — we don't consult the keymap or regenerate the
        // popup, just nudge its scroll offset (the renderer clamps it).
        if let Some(info) = cxt.editor.autoinfo.as_mut() {
            const PAGE: u16 = 8;
            match event.code {
                zmax_view::input::KeyCode::PageDown => {
                    info.scroll = info.scroll.saturating_add(PAGE);
                    return None;
                }
                zmax_view::input::KeyCode::PageUp => {
                    info.scroll = info.scroll.saturating_sub(PAGE);
                    return None;
                }
                _ => {}
            }
        }
        // Record the key for `view-lossage` (C-h l), keeping a bounded ring. How
        // many keys are kept is Emacs's `lossage-size`.
        self.recent_keys.push_back(event);
        while self.recent_keys.len() > crate::commands::lossage_limit() {
            self.recent_keys.pop_front();
        }
        self.pseudo_pending.extend(self.keymaps.pending());
        // The focused document's Emacs *major mode* (see keymap::major_mode):
        // its explicit one (`M-x outline-mode` / `view-mode` / `text-mode` …,
        // via `Document::set_major_mode`) if it has one, else its language
        // (`M-x org-mode` / `latex-mode` / `sgml-mode` … set that). Its overlay
        // shadows the base keymap on the chords Emacs gives that mode (`C-c C-t`
        // = org-todo in Org, sgml-tag in HTML, outline-hide-body in Outline, …).
        let language = doc!(cxt.editor).major_mode();

        // Emacs `repeat-mode`: a repetition is live, so this key may be the
        // single-key shortcut that runs the multi-key chord again. Replaying the
        // chord's prefix into the keymap ahead of the key makes the repetition
        // take exactly the path typing the whole chord would, so the count, the
        // major-mode overlay and the which-key popup all behave identically.
        if self.keymaps.pending().is_empty() {
            if let Some(prefix) = crate::emacs_repeat::armed() {
                if crate::emacs_repeat::is_exit_key(event) {
                    // `repeat-exit-key`: ends the repetition and is not executed.
                    crate::emacs_repeat::disarm();
                    cxt.editor.autoinfo = None;
                    cxt.editor.clear_status();
                    return None;
                }
                let mut chord = prefix.clone();
                chord.push(event);
                if self.keymaps.resolves_to_command(mode, &chord) {
                    for key in prefix {
                        self.keymaps.get_with_language(mode, key, language);
                    }
                } else {
                    // "Typing any key other than those defined to repeat the
                    // previous command exits the transient repeating mode, and
                    // then the key you typed is executed normally."
                    crate::emacs_repeat::disarm();
                }
            }
        }

        // The chord's prefix, for `repeat-mode` to arm on a match below.
        let chord_prefix: Vec<KeyEvent> = self.keymaps.pending().to_vec();
        let key_result = self.keymaps.get_with_language(mode, event, language);
        cxt.editor.autoinfo = if cxt.editor.config().which_key {
            self.keymaps.sticky().map(|node| node.infobox())
        } else {
            None
        };
        // A pinned keymap listing (`SPC t k m`/`t`/`M`) shows whenever nothing
        // else has claimed the popup, so it survives keystrokes instead of
        // living for one pending chord.
        if cxt.editor.autoinfo.is_none() {
            cxt.editor.autoinfo = cxt.editor.persistent_autoinfo.clone();
        }

        // vim i_CTRL-O one-shot: if the flag was already armed when this event
        // began, the command about to run is the "one command" that should be
        // followed by a return to Insert. Reading it before the executing closure
        // borrows `cxt` excludes the arming CTRL-O press itself (which arms the
        // flag mid-execution).
        let oneshot_armed = cxt.editor.insert_oneshot;

        let mut execute_command = |command: &commands::MappableCommand| {
            // Emacs `repeat` (`C-x z`) re-runs the last command, so every
            // dispatched command is recorded — except the repeat itself, which
            // would otherwise repeat repeating.
            commands::record_last_command(command);
            command.execute(cxt);
            // command-log-mode: the live log wants the keys *and* the command,
            // which only this dispatch knows.
            if commands::command_log_enabled() {
                let keys: String = chord_prefix
                    .iter()
                    .chain(std::iter::once(&event))
                    .map(|k| k.key_sequence_format())
                    .collect::<Vec<_>>()
                    .join(" ");
                commands::command_log_append(cxt.editor, &keys, command.name());
            }
            zmax_event::dispatch(PostCommand { command, cx: cxt });

            // spacemacs `nav-flash`: the layer advises a fixed set of navigation
            // commands with `nav-flash/blink-cursor-maybe`, so the line the
            // cursor landed on flashes once the command has run.
            if NAV_FLASH_COMMANDS.contains(&command.name()) {
                let (view, doc) = current_ref!(cxt.editor);
                let line = doc.text().char_to_line(
                    doc.selection(view.id)
                        .primary()
                        .cursor(doc.text().slice(..)),
                );
                let point = (view.id, doc.id(), line);
                // `nav-flash--last-point`: a trigger that did not actually move
                // the cursor (same window, buffer and point) blinks nothing.
                if self.nav_flash_last != Some(point) {
                    self.nav_flash_last = Some(point);
                    self.nav_flash = Some((doc.id(), line, std::time::Instant::now()));
                    // Repaint once the flash is over — nothing else would
                    // repaint an idle editor (as `showmatch` does for its own).
                    tokio::spawn(async move {
                        tokio::time::sleep(NAV_FLASH_DELAY).await;
                        zmax_event::request_redraw();
                    });
                }
            }
            // Follow-mode (SPC w f): keep sibling windows scrolled in lockstep.
            cxt.editor.sync_follow_windows();

            let current_mode = cxt.editor.mode();
            if current_mode != last_mode {
                zmax_event::dispatch(OnModeSwitch {
                    old_mode: last_mode,
                    new_mode: current_mode,
                    cx: cxt,
                });

                // HAXX: if we just entered insert mode from normal, clear key buf
                // and record the command that got us into this mode.
                if current_mode == Mode::Insert {
                    // how we entered insert mode is important, and we should track that so
                    // we can repeat the side effect.
                    self.last_insert.0 = command.clone();
                    self.last_insert.1.clear();
                }
            }

            last_mode = current_mode;
        };

        match &key_result {
            KeymapResult::Matched(command) => {
                execute_command(command);
            }
            KeymapResult::Pending(node) => {
                // Decide whether to show the which-key popup, matched on the first
                // key of the pending sequence (e.g. "g"/"y"/"z"/">"/"space").
                let config = cxt.editor.config();
                // Global = the new `which-key-global` flag, or the legacy
                // `auto-info-leader-only = false` (kept working for old configs).
                let global = config.which_key_global || !config.auto_info_leader_only;
                let show = if !config.which_key {
                    // Master which-key off switch — never show a prefix popup.
                    false
                } else if global {
                    // Global which-key: every pending prefix pops up.
                    true
                } else {
                    // Default: only the deliberate global prefixes get a popup —
                    // the `space` leader and the emacs/spacemacs `C-x`/`C-c`/`C-h`
                    // prefixes. Operator + text-object prefixes (c, d, g, z, >,
                    // ci/ca, di/da, C-w, ...) stay quiet. (In the pure `vim` preset
                    // none of these is a prefix, so vim shows no which-key at all.)
                    matches!(
                        self.keymaps
                            .pending()
                            .first()
                            .map(KeyEvent::to_string)
                            .as_deref(),
                        // `F1` is grafted onto the same node as `C-h` (the emacs
                        // preset's help map), so a prefix opened with it must
                        // raise the which-key popup too.
                        Some("space" | "C-x" | "C-c" | "C-h" | "F1")
                    )
                };
                cxt.editor.autoinfo = show.then(|| node.infobox());
            }
            KeymapResult::MatchedSequence(commands) => {
                for command in commands {
                    execute_command(command);
                }
            }
            KeymapResult::NotFound | KeymapResult::Cancelled(_) => {
                if cxt.editor.autoinfo.is_none() {
                    cxt.editor.autoinfo = cxt.editor.persistent_autoinfo.clone();
                }
                return Some(key_result);
            }
        }

        // Emacs `repeat-mode`: the command came out of a chord of two or more
        // keys, so its last key alone repeats it until some other key is typed.
        // The transient map is the chord's own prefix node, which is why `C-x u`
        // then `u u u` keeps undoing and `C-x {` then `} ^ v` keeps resizing.
        if crate::emacs_repeat::enabled()
            && matches!(
                key_result,
                KeymapResult::Matched(_) | KeymapResult::MatchedSequence(_)
            )
        {
            if chord_prefix.is_empty() {
                crate::emacs_repeat::disarm();
            } else {
                crate::emacs_repeat::arm(&chord_prefix);
                let keys = self.keymaps.repeat_keys(mode, &chord_prefix);
                if !keys.is_empty() {
                    cxt.editor.set_status(crate::emacs_repeat::hint(&keys));
                }
            }
        }

        // `q` inside a spacemacs transient state ran `exit_transient_state`,
        // which can only raise a flag — the latched sticky node lives here.
        if cxt.editor.exit_transient {
            cxt.editor.exit_transient = false;
            self.keymaps.sticky = None;
            cxt.editor.autoinfo = None;
        }

        // Complete the vim i_CTRL-O one-shot: a command that was matched (not a
        // still-pending prefix) has now run in the temporary Normal mode, so
        // return to Insert. Only fires when the flag was armed before this event,
        // so the CTRL-O press itself and any following multi-key prefix are
        // skipped. If the one command itself changed the mode (e.g. `A`/`i`),
        // honor that instead of forcing Insert.
        if oneshot_armed
            && cxt.editor.insert_oneshot
            && matches!(
                key_result,
                KeymapResult::Matched(_) | KeymapResult::MatchedSequence(_)
            )
        {
            cxt.editor.insert_oneshot = false;
            if cxt.editor.mode() == Mode::Normal {
                cxt.editor.mode = Mode::Insert;
            }
        }
        None
    }

    fn insert_mode(&mut self, cx: &mut commands::Context, event: KeyEvent) {
        // emacs `quail-translation-keymap`: while an input method is in the
        // middle of a key sequence, that keymap *overrides* the ordinary one
        // (quail.el binds it in `overriding-terminal-local-map`), so `C-f`,
        // `C-b`, `C-n`, `C-p`, `TAB`, `C-SPC` and `DEL` belong to the method and
        // choose among its alternatives instead of moving point or completing.
        if commands::quail_translation_key(cx, event) {
            return;
        }
        // emacs prefix argument. The emacs keymap preset binds most of its
        // commands in Insert mode (that is where you type), so the argument has to
        // reach commands from here as well as from `command_mode`; a positive one
        // stands in as the count exactly as it does there.
        let prefix_arg = cx.editor.prefix_arg;
        if let Some(arg) = prefix_arg {
            if let Some(count) = usize::try_from(arg.value())
                .ok()
                .and_then(NonZeroUsize::new)
            {
                cx.count = Some(count);
            }
        }
        // `C-u 8 *` self-inserts eight asterisks (the manual's "Arguments" node).
        // A negative argument inserts nothing, as in emacs.
        let repeat = prefix_arg.map_or(1, |arg| arg.value().clamp(0, 100_000) as usize);

        if let Some(keyresult) = self.handle_keymap_event(Mode::Insert, cx, event) {
            match keyresult {
                KeymapResult::NotFound => {
                    if !self.on_next_key(OnKeyCallbackKind::Fallback, cx, event) {
                        if let Some(ch) = event.char() {
                            for _ in 0..repeat {
                                commands::insert::insert_char(cx, ch)
                            }
                        }
                    }
                }
                KeymapResult::Cancelled(pending) => {
                    for ev in pending {
                        // A modifier chord (e.g. `C-x`, `A-x`) is NOT self-inserting
                        // text — its `char()` is the base letter, so inserting it
                        // would turn a cancelled `C-x` prefix into a literal `x`.
                        // Only plain / shifted keys self-insert; chords are re-run
                        // as commands (and a still-pending prefix simply drops).
                        let is_chord = ev.modifiers.intersects(
                            KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER,
                        );
                        match ev.char() {
                            Some(ch) if !is_chord => commands::insert::insert_char(cx, ch),
                            _ => {
                                let language = doc!(cx.editor).major_mode();
                                if let KeymapResult::Matched(command) =
                                    self.keymaps.get_with_language(Mode::Insert, ev, language)
                                {
                                    command.execute(cx);
                                }
                            }
                        }
                    }
                }
                _ => unreachable!(),
            }
        }

        // The command has run: the prefix argument is spent, unless the command
        // was itself a prefix command (`universal-argument`) handing a new one on.
        if self.keymaps.pending().is_empty() {
            cx.editor.prefix_arg = cx.editor.pending_prefix_arg.take();
        }
    }

    /// The emacs **prefix argument** reader (the manual's "Arguments" node).
    ///
    /// `universal-argument` (`C-u`) arms an argument; while one is armed this
    /// consumes the keys that *build* it rather than letting them run commands —
    /// digits extend it (`C-u 3 0` = 30) and `-` negates it (`C-u - 5` = -5).
    /// `M-1`…`M-9`, `M-0` (`digit-argument`) and `M--` (`negative-argument`) start
    /// one from nothing, so they are consumed here too, but only when the active
    /// keymap does not bind the chord to something else — the same courtesy the
    /// vim count parser extends.
    ///
    /// Returns whether the key was a prefix-argument key. Nothing else in the
    /// editor sees it if so; `Editor::prefix_arg` is what the next command reads.
    fn handle_prefix_key(
        &mut self,
        cxt: &mut commands::Context,
        mode: Mode,
        key: KeyEvent,
    ) -> bool {
        // Never steal a key that a half-typed keymap sequence (`C-x …`) or an
        // on_next_key command (`r <c>`, `f <c>`) is already waiting for.
        if !self.keymaps.pending().is_empty() || self.on_next_key.is_some() {
            return false;
        }
        let current = cxt.editor.prefix_arg;
        let unbound = |view: &Self| !view.keymaps.contains_key(mode, key);
        let next = match (key, current) {
            // Extending an argument that is already being read.
            (key!(c @ '0'..='9'), Some(arg)) => arg.push_digit(c.to_digit(10).unwrap_or(0)),
            (key!('-'), Some(arg)) => arg.negate(),
            // `M-0`..`M-9` = digit-argument: starts (or extends) a numeric argument.
            (alt!(c @ '0'..='9'), _) if unbound(self) => current
                .unwrap_or(PrefixArg::Numeric(0))
                .push_digit(c.to_digit(10).unwrap_or(0)),
            // `M--` = negative-argument.
            (alt!('-'), _) if unbound(self) => {
                current.map_or(PrefixArg::Negative, PrefixArg::negate)
            }
            _ => return false,
        };
        cxt.editor.prefix_arg = Some(next);
        // Emacs echoes the argument as it is typed, so you can see what the next
        // command will run with.
        let echo = match next {
            PrefixArg::Universal(_) => "C-u-".to_string(),
            PrefixArg::Negative => "C-u - -".to_string(),
            PrefixArg::Numeric(v) => format!("C-u {v}-"),
        };
        cxt.editor.set_status(echo);
        true
    }

    /// Whether `key` would be consumed as a count prefix in `mode` (mirrors the
    /// count arms of `command_mode`). Such keys are excluded from the recorded
    /// change so `.` repeats the command, not its count.
    fn is_count_key(&self, mode: Mode, count: Option<NonZeroUsize>, key: KeyEvent) -> bool {
        match (key, count) {
            (key!('0'..='9'), Some(_)) => true,
            (key!('1'..='9'), None) => !self.keymaps.contains_key(mode, key),
            _ => false,
        }
    }

    /// Replay the last recorded change `count` times for vim dot-repeat (`.`).
    /// Routes each recorded key by the current mode, so an insert session inside
    /// the change (e.g. `cwfoo<Esc>`) replays correctly.
    fn replay_last_change(&mut self, cx: &mut commands::Context, count: usize) {
        if self.last_change.is_empty() {
            return;
        }
        let keys = self.last_change.clone();
        // vim `.` acts at a single cursor. zmax is helix-derived, so a counted
        // command like `2o` leaves one selection *per count* (two cursors), and
        // replaying the change then applies it at every cursor — `2oab<Esc>.`
        // opened a line at each of the two cursors on every iteration, six lines
        // instead of vim's four. Reduce to the primary selection first so the
        // replay runs once, at the primary cursor, exactly as vim does.
        {
            let (view, doc) = current!(cx.editor);
            let range = doc.selection(view.id).primary();
            doc.set_selection(view.id, Selection::single(range.anchor, range.head));
        }
        // vim `{count}i`/`{count}a`/`{count}A`/`{count}I` lay the typed text
        // `count` times inside a SINGLE insert session (via `insert_count`) and
        // press Esc once, so `3iab<Esc>` yields "ababab". Replaying the recorded
        // `i…<Esc>` keys `count` times instead runs `count` separate insert
        // sessions, and the cursor-left that each <Esc> applies scrambles the
        // text — `3iab<Esc>.` produced "ababaaaabbbb" instead of vim's
        // "ababaababab". For a change opening with an inline insert-entry key,
        // re-inject the count so the entry command captures it as `insert_count`,
        // and replay the keys exactly once.
        //
        // `o`/`O` are deliberately excluded: their open commands consume the
        // count to open that many lines, so the outer loop already lays one line
        // per iteration correctly — re-injecting would double-count (open N lines
        // AND lay the text N times), giving `2oab<Esc>.` six lines instead of
        // four.
        // `x`/`X` join them for a different reason: they apply the count
        // themselves *and clamp it to the line*. Replaying `x` `count` times drops
        // that clamp — `3x` on a two-character line correctly stops at the line
        // end, but three separate `x`es empty the line and then eat the newline,
        // so `3xj0.` merged two lines where vim leaves both.
        let entry = keys.first().and_then(|k| {
            (k.modifiers == KeyModifiers::NONE)
                .then(|| k.char())
                .flatten()
        });
        let consumes_count_itself = match entry {
            // The count belongs to the insert session.
            Some('i' | 'a' | 'A' | 'I') => true,
            // `x`/`X` on their own; as an operator prefix (`x` is not) this would
            // not hold, so require the change to be exactly that one key.
            Some('x' | 'X') => keys.len() == 1,
            _ => false,
        };
        let outer = if consumes_count_itself {
            cx.editor.count = NonZeroUsize::new(count);
            1
        } else {
            count
        };
        self.replaying = true;
        for _ in 0..outer {
            for &key in &keys {
                // Mirror `handle_event`'s dispatch order so on_next_key commands
                // replay faithfully: text objects (`ci"`, `da(`), finds
                // (`cf x`, `ct x`) and replace (`r x`) register a callback that
                // claims the *next* key. `command_mode` alone never consults
                // that callback, so a naive replay would run the recorded `"`
                // through the keymap (as a register prefix) instead of
                // completing the text object — the change would silently no-op.
                if !self.on_next_key(OnKeyCallbackKind::PseudoPending, cx, key) {
                    match cx.editor.mode() {
                        Mode::Insert => {
                            self.insert_mode(cx, key);
                            self.last_insert.1.push(InsertEvent::Key(key));
                        }
                        m => self.command_mode(m, cx, key),
                    }
                }
                // Carry any callback a command just registered to the next key,
                // exactly as `handle_event` does after each dispatch.
                self.on_next_key = cx.on_next_key_callback.take();
            }
        }
        self.replaying = false;
    }

    fn command_mode(&mut self, mode: Mode, cxt: &mut commands::Context, event: KeyEvent) {
        match (event, cxt.editor.count) {
            // If the count is already started and the input is a number, always continue the count.
            (key!(i @ '0'..='9'), Some(count)) => {
                let i = i.to_digit(10).unwrap() as usize;
                let count = count.get() * 10 + i;
                if count > 100_000_000 {
                    return;
                }
                cxt.editor.count = NonZeroUsize::new(count);
            }
            // A non-zero digit will start the count if that number isn't used by a keymap.
            (key!(i @ '1'..='9'), None) if !self.keymaps.contains_key(mode, event) => {
                let i = i.to_digit(10).unwrap() as usize;
                cxt.editor.count = NonZeroUsize::new(i);
            }
            // vim dot-repeat: replay the keys of the last buffer-changing command.
            // Unlike the old insert-only repeat, this replays the whole change
            // (operator + motion/text-object + any insert session), so `dd`, `x`,
            // `dw`, `p`, `cwfoo<Esc>`, `ci"bar<Esc>`, `di(`, `r x`, … all repeat.
            (key!('.'), _) if cxt.editor.vim_semantics && self.keymaps.pending().is_empty() => {
                // vim: `[count].` replaces the change's count; a bare `.` reuses
                // the original one (`2x` then `.` deletes two, not one).
                let count = cxt
                    .editor
                    .count
                    .map_or(self.last_change_count, NonZeroUsize::get);
                cxt.editor.count = None;
                self.replay_last_change(cxt, count);
            }
            _ => {
                // set the count — in vim mode fold in any operator count captured
                // when the operator entered its pending state, so `2d3w` deletes
                // `2 * 3 = 6` words (vim multiplies the operator and motion counts)
                // instead of concatenating the digits into `23`.
                cxt.count = if cxt.editor.vim_semantics {
                    combine_counts(self.operator_count, cxt.editor.count)
                } else {
                    cxt.editor.count
                };
                // TODO: edge case: 0j -> reset to 1
                // if this fails, count was Some(0)
                // debug_assert!(cxt.count != 0);

                // emacs prefix argument: a positive one stands in as the count —
                // which is what makes `C-u 5 C-f` move five characters and `C-u
                // C-k` kill four lines, with no per-command work. Commands that
                // care about the *shape* of the argument (a bare `C-u` before
                // `C-SPC` pops the mark) call `Context::prefix_arg()`, which reads
                // it straight off the editor; it is cleared only after they run.
                if let Some(arg) = cxt.editor.prefix_arg {
                    if let Some(count) = usize::try_from(arg.value())
                        .ok()
                        .and_then(NonZeroUsize::new)
                    {
                        cxt.count = Some(count);
                    }
                }

                // set the register
                cxt.register = cxt.editor.selected_register.take();

                let res = self.handle_keymap_event(mode, cxt, event);
                if matches!(&res, Some(KeymapResult::NotFound)) {
                    self.on_next_key(OnKeyCallbackKind::Fallback, cxt, event);
                }
                if self.keymaps.pending().is_empty() {
                    cxt.editor.count = None;
                    self.operator_count = None;
                    // The command has run, so the prefix argument is spent — unless
                    // the command *was* `universal-argument`/`digit-argument`, which
                    // hand the argument they just built to the next command through
                    // `pending_prefix_arg`.
                    cxt.editor.prefix_arg = cxt.editor.pending_prefix_arg.take();
                } else {
                    cxt.editor.selected_register = cxt.register.take();
                    // vim mode: this key started (or extended) an operator/prefix
                    // pending sequence. Snapshot the count typed before it as the
                    // operator count and clear the live count so digits typed after
                    // the operator form a fresh motion count; the two multiply when
                    // the motion finally executes.
                    if cxt.editor.vim_semantics
                        && self.operator_count.is_none()
                        && cxt.editor.count.is_some()
                    {
                        self.operator_count = cxt.editor.count.take();
                    }
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn set_completion(
        &mut self,
        editor: &mut Editor,
        items: Vec<CompletionItem>,
        trigger_offset: usize,
        size: Rect,
    ) -> Option<Rect> {
        let mut completion = Completion::new(editor, items, trigger_offset);

        if completion.is_empty() {
            // skip if we got no completion results
            return None;
        }

        let area = completion.area(size, editor);
        editor.last_completion = Some(CompleteAction::Triggered);
        self.last_insert.1.push(InsertEvent::TriggerCompletion);

        // TODO : propagate required size on resize to completion too
        self.completion = Some(completion);
        Some(area)
    }

    pub fn clear_completion(&mut self, editor: &mut Editor) -> Option<OnKeyCallback> {
        self.completion = None;
        let mut on_next_key: Option<OnKeyCallback> = None;
        editor.handlers.completions.request_controller.restart();
        editor.handlers.completions.active_completions.clear();
        if let Some(last_completion) = editor.last_completion.take() {
            match last_completion {
                CompleteAction::Triggered => (),
                CompleteAction::Applied {
                    trigger_offset,
                    changes,
                    placeholder,
                } => {
                    self.last_insert.1.push(InsertEvent::CompletionApply {
                        trigger_offset,
                        changes,
                    });
                    on_next_key = placeholder.then_some(Box::new(|cx, key| {
                        if let Some(c) = key.char() {
                            let (view, doc) = current!(cx.editor);
                            if let Some(snippet) = &doc.active_snippet {
                                doc.apply(&snippet.delete_placeholder(doc.text()), view.id);
                            }
                            commands::insert::insert_char(cx, c);
                        }
                    }))
                }
                CompleteAction::Selected { savepoint } => {
                    let (view, doc) = current!(editor);
                    doc.restore(view, &savepoint, false);
                }
            }
        }
        on_next_key
    }

    pub fn handle_idle_timeout(&mut self, cx: &mut commands::Context) -> EventResult {
        commands::compute_inlay_hints_for_all_views(cx.editor, cx.jobs);

        // GitLens-style inline blame: show the current line's author/date/summary
        // as an idle status hint (cached per file).
        if crate::blame::enabled() {
            let info = {
                let (view, doc) = zmax_view::current_ref!(cx.editor);
                doc.path().map(|p| {
                    let text = doc.text();
                    let cursor = doc.selection(view.id).primary().cursor(text.slice(..));
                    (p.to_path_buf(), text.char_to_line(cursor) + 1)
                })
            };
            if let Some((path, line)) = info {
                if let Some(b) = crate::blame::line_blame(&path, line) {
                    cx.editor.set_status(format!("  {b}"));
                }
            }
        }

        // Blame annotate gutter: the gutter renderer lives in zmax_view and
        // can't shell out to git, so compute+push the focused file's blame here
        // the first time it's shown (cheap no-op once cached).
        if crate::blame::annotate_enabled() {
            let path = {
                let (_, doc) = zmax_view::current_ref!(cx.editor);
                doc.path().map(|p| p.to_path_buf())
            };
            if let Some(path) = path {
                crate::blame::ensure_annotate(&path);
            }
        }

        EventResult::Ignored(None)
    }
}

/// Whether the focused doc's workspace is in restricted mode and running `trust` would
/// change something visible at the workspace level.
/// Run a normal-mode command from a context-menu callback, then dispatch any
/// compositor callbacks it queued (LSP pickers, rename prompt, code-action menu…).
fn run_editor_command(
    compositor: &mut crate::compositor::Compositor,
    cx: &mut crate::compositor::Context,
    cmd: impl FnOnce(&mut commands::Context),
) {
    let cbs = {
        let mut c = commands::Context {
            editor: cx.editor,
            count: None,
            register: None,
            callback: Vec::new(),
            on_next_key_callback: None,
            jobs: cx.jobs,
        };
        cmd(&mut c);
        std::mem::take(&mut c.callback)
    };
    for cb in cbs {
        cb(compositor, cx);
    }
}

/// Spawn a detached command (Open In Finder/Terminal/GitHub, gist).
fn ctx_spawn(program: &str, args: &[&str]) {
    let _ = std::process::Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

/// The context menu's Copy: yank the selection with `cmd`, or the WHOLE BUFFER
/// when nothing is selected, without moving the caret.
///
/// What counts as "nothing selected": a resting caret in this editor is a
/// ONE-CHARACTER range, not an empty one, so `Range::is_empty` would call every
/// caret a selection and the whole-buffer branch would never run. A real
/// selection is Select mode, more than one range, or a primary range spanning
/// more than one character.
fn ctx_copy(c: &mut crate::commands::Context, cmd: &crate::commands::MappableCommand) {
    let (has_selection, saved, view_id) = {
        let (view, doc) = zmax_view::current_ref!(c.editor);
        let sel = doc.selection(view.id);
        let has = c.editor.mode() == Mode::Select || sel.len() > 1 || sel.primary().len() > 1;
        (has, sel.clone(), view.id)
    };
    if has_selection {
        cmd.execute(c);
    } else {
        // Nothing selected: copy the whole buffer, then put the caret back.
        // Copying must not move the user's cursor.
        crate::commands::MappableCommand::select_all.execute(c);
        cmd.execute(c);
        doc_mut!(c.editor).set_selection(view_id, saved);
    }
}

/// The JetBrains in-editor context menu (right-click on editor text). Actions map
/// to real zmax commands; the Run/Debug + Open In/Git/Gist groups appear only
/// for a file backed by a path.
pub(crate) fn editor_menu_entries(
    path: Option<std::path::PathBuf>,
) -> Vec<crate::ui::context_menu::Entry> {
    use crate::commands::MappableCommand as MC;
    use crate::ui::context_menu::Entry;

    let mut e = vec![
        Entry::item_key("Show Context Actions", "⌥↵", |co, cx| {
            run_editor_command(co, cx, |c| {
                MC::code_action.execute(c);
            })
        }),
        Entry::sep(),
        // Copy/Paste come in pairs: the plain entries use the editor's own
        // register (vim `y`/`p`), the "System Clipboard" ones the `+` register
        // (`<space>y`/`<space>p`). Both destinations are reachable by mouse.
        Entry::item_key("Copy", "y", |co, cx| {
            run_editor_command(co, cx, |c| ctx_copy(c, &MC::yank))
        }),
        Entry::item_key("Copy to System Clipboard", "␣y", |co, cx| {
            run_editor_command(co, cx, |c| ctx_copy(c, &MC::yank_to_clipboard))
        }),
        Entry::item_key("Paste", "p", |co, cx| {
            run_editor_command(co, cx, |c| {
                MC::paste_after.execute(c);
            })
        }),
        Entry::item_key("Paste from System Clipboard", "␣p", |co, cx| {
            run_editor_command(co, cx, |c| {
                MC::paste_clipboard_after.execute(c);
            })
        }),
        Entry::item("Copy Reference", |_co, cx| {
            let (view, doc) = zmax_view::current_ref!(cx.editor);
            let text = doc.text();
            let pos = doc.selection(view.id).primary().cursor(text.slice(..));
            let line = text.char_to_line(pos) + 1;
            let name = doc
                .path()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|| "[scratch]".into());
            let r = format!("{name}:{line}");
            let _ = cx.editor.registers.push('"', r.clone());
            cx.editor.set_status(format!("yanked {r}"));
        }),
        Entry::sep(),
        Entry::item_key("Find Usages", "⌥F7", |co, cx| {
            run_editor_command(co, cx, |c| {
                MC::goto_reference.execute(c);
            })
        }),
        Entry::sub(
            "Go To",
            vec![
                Entry::item("Declaration", |co, cx| {
                    run_editor_command(co, cx, |c| {
                        MC::goto_declaration.execute(c);
                    })
                }),
                Entry::item("Definition", |co, cx| {
                    run_editor_command(co, cx, |c| {
                        MC::goto_definition.execute(c);
                    })
                }),
                Entry::item("Type Definition", |co, cx| {
                    run_editor_command(co, cx, |c| {
                        MC::goto_type_definition.execute(c);
                    })
                }),
                Entry::item("Implementation", |co, cx| {
                    run_editor_command(co, cx, |c| {
                        MC::goto_implementation.execute(c);
                    })
                }),
            ],
        ),
        Entry::sep(),
        Entry::sub(
            "Folding",
            vec![
                Entry::item("Fold", |co, cx| {
                    run_editor_command(co, cx, |c| {
                        MC::fold_create.execute(c);
                    })
                }),
                Entry::item("Toggle Fold", |co, cx| {
                    run_editor_command(co, cx, |c| {
                        MC::fold_toggle.execute(c);
                    })
                }),
                Entry::item("Fold All", |co, cx| {
                    run_editor_command(co, cx, |c| {
                        MC::fold_close_all.execute(c);
                    })
                }),
                Entry::item("Unfold All", |co, cx| {
                    run_editor_command(co, cx, |c| {
                        MC::fold_open_all.execute(c);
                    })
                }),
            ],
        ),
        Entry::sep(),
        Entry::item_key("Rename…", "⇧F6", |co, cx| {
            run_editor_command(co, cx, |c| {
                MC::rename_symbol.execute(c);
            })
        }),
        Entry::sub(
            "Refactor",
            vec![
                Entry::item("Rename…", |co, cx| {
                    run_editor_command(co, cx, |c| {
                        MC::rename_symbol.execute(c);
                    })
                }),
                Entry::item("Reformat Code", |co, cx| {
                    run_editor_command(co, cx, |c| {
                        MC::format_selections.execute(c);
                    })
                }),
            ],
        ),
        Entry::item_key("Generate…", "SPC F s", |_co, cx| {
            crate::commands::typed::run_command_line(cx, "Snippets");
        }),
    ];

    // tmux paste buffers: a third store next to the register and the system
    // clipboard, offered only inside a tmux session (`prefix ]` pastes them in
    // any pane). Copying here does not touch the system clipboard.
    if zmax_view::clipboard::tmux_available() {
        let tmux = vec![
            Entry::item("Copy to tmux Buffer", |co, cx| {
                run_editor_command(co, cx, |c| ctx_copy(c, &MC::yank_to_tmux_buffer))
            }),
            Entry::item("Paste Newest tmux Buffer", |co, cx| {
                run_editor_command(co, cx, |c| {
                    MC::paste_tmux_buffer_after.execute(c);
                })
            }),
            Entry::item("Paste tmux Buffer…", |co, cx| {
                run_editor_command(co, cx, |c| {
                    MC::tmux_buffer_picker.execute(c);
                })
            }),
        ];
        e.push(Entry::sep());
        e.push(Entry::sub("tmux Buffers", tmux));
    }

    if let Some(path) = path {
        let dir = path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| std::path::PathBuf::from("."));

        // Run / Debug — standalone file.
        e.push(Entry::sep());
        {
            let p = path.clone();
            e.push(Entry::item("Run", move |co, cx| {
                if let Some(v) = co.find::<EditorView>() {
                    v.run_path(cx.editor, &p);
                }
            }));
        }
        e.push(Entry::item("Debug", |co, cx| {
            run_editor_command(co, cx, |c| {
                crate::commands::dap::dap_launch(c);
            })
        }));

        // Open In ›
        e.push(Entry::sep());
        {
            let (pf, dt, pg) = (path.clone(), dir.clone(), path.clone());
            // `gh browse` wants a repo-relative path; an absolute path yields a
            // malformed `…/tree/<branch>//Users/…` URL. Strip the workspace root.
            let root = zmax_loader::find_workspace().0;
            let rel = pg
                .strip_prefix(&root)
                .map(|p| p.to_string_lossy().into_owned())
                .ok();
            e.push(Entry::sub(
                "Open In",
                vec![
                    Entry::item("Finder", move |_co, _cx| {
                        ctx_spawn("open", &["-R", pf.to_str().unwrap_or("")]);
                    }),
                    Entry::item("Terminal", move |_co, _cx| {
                        ctx_spawn("open", &["-a", "Terminal", dt.to_str().unwrap_or("")]);
                    }),
                    Entry::item("GitHub", move |_co, cx| match &rel {
                        Some(r) => ctx_spawn("gh", &["browse", "--", r]),
                        None => cx.editor.set_error("not in a repo"),
                    }),
                ],
            ));
        }

        // Git › + Local History (git log -p)
        e.push(Entry::sep());
        fn mkgit(
            label: &'static str,
            tmpl: &'static str,
            p: std::path::PathBuf,
            d: std::path::PathBuf,
        ) -> crate::ui::context_menu::Entry {
            crate::ui::context_menu::Entry::item(label, move |co, cx| {
                if let Some(v) = co.find::<EditorView>() {
                    let quoted = p.to_string_lossy().replace('\'', "'\\''");
                    v.start_run(cx, tmpl.replace("{}", &quoted), d.clone());
                }
            })
        }
        e.push(Entry::sub(
            "Git",
            vec![
                // Blame toggles the annotate gutter (JetBrains "Annotate"), not a
                // Run-console dump.
                {
                    let p = path.clone();
                    Entry::item("Blame", move |_co, cx| {
                        let on = crate::blame::toggle_annotate();
                        if on {
                            crate::blame::ensure_annotate(&p);
                        }
                        cx.editor.set_status(if on {
                            "blame annotate: on"
                        } else {
                            "blame annotate: off"
                        });
                    })
                },
                mkgit(
                    "Diff",
                    "git --no-pager diff '{}'",
                    path.clone(),
                    dir.clone(),
                ),
                mkgit(
                    "Log",
                    "git --no-pager log --oneline -- '{}'",
                    path.clone(),
                    dir.clone(),
                ),
            ],
        ));
        e.push(mkgit(
            "Local History",
            "git --no-pager log -p -- '{}'",
            path.clone(),
            dir.clone(),
        ));

        // Create Gist…
        e.push(Entry::sep());
        {
            let p = path.clone();
            e.push(Entry::item("Create Gist…", move |_co, _cx| {
                ctx_spawn("gh", &["gist", "create", "--web", p.to_str().unwrap_or("")]);
            }));
        }
    }

    e
}

/// emacs ffap: the file/URL guessed from the text around `pos` (`ffap-guesser`).
/// The scan mirrors `goto_file`'s own detection — a lookaround clipped to the
/// clicked line — so a `Some` here means `goto_file` will act on the same token,
/// and a `None` means it would open garbage and must not be run at all.
pub(crate) fn ffap_guess_at(doc: &Document, pos: usize) -> Option<String> {
    let text = doc.text().slice(..);
    let byte = text.char_to_byte(pos);
    let line = text.byte_to_line(byte);
    let start = text.line_to_byte(line);
    let end = text.line_to_byte(line + 1);
    let slice = text.byte_slice(start..end);
    zmax_stdx::path::find_paths(slice, true)
        .find(|range| start + range.start <= byte + 1 && byte <= start + range.end)
        .map(|range| slice.byte_slice(range).to_string())
}

/// emacs `ffap-menu-rescan`: every file/URL mentioned in the buffer, as
/// `(text, char position)`. Duplicates collapse to their last occurrence and the
/// result is ordered by buffer position, which is exactly the alist ffap builds
/// (string sort → dedupe → sort by position).
fn ffap_menu_candidates(doc: &Document) -> Vec<(String, usize)> {
    let text = doc.text().slice(..);
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for range in zmax_stdx::path::find_paths(text, true) {
        let found = text.byte_slice(range.clone()).to_string();
        seen.insert(found, text.byte_to_char(range.start));
    }
    let mut candidates: Vec<(String, usize)> = seen.into_iter().collect();
    candidates.sort_by_key(|&(_, pos)| pos);
    candidates
}

/// The menu rows for `ffap-menu`. Choosing one is `ffap-menu-cont`: set the mark,
/// jump to that occurrence, then `find-file-at-point` on it.
pub(crate) fn ffap_menu_entries(doc: &Document) -> Vec<crate::ui::context_menu::Entry> {
    use crate::commands::MappableCommand as MC;
    use crate::ui::context_menu::Entry;

    ffap_menu_candidates(doc)
        .into_iter()
        // The panel is drawn at its full height, so a buffer that mentions
        // thousands of paths would cover the screen; ffap itself lists them all.
        .take(FFAP_MENU_MAX)
        .map(|(label, pos)| {
            Entry::item(label, move |co, cx| {
                run_editor_command(co, cx, move |c| {
                    MC::save_selection.execute(c);
                    let (view, doc) = zmax_view::current!(c.editor);
                    let pos = pos.min(doc.text().len_chars());
                    doc.set_selection(view.id, Selection::point(pos));
                    MC::goto_file.execute(c);
                });
            })
        })
        .collect()
}

/// Rows drawn by one `ffap-menu` popup at most.
const FFAP_MENU_MAX: usize = 100;

/// vim `mousescroll=ver:N,hor:M`: the number of lines one mouse-wheel notch
/// scrolls (`ver`). `None` when the spec names no vertical amount, so the caller
/// keeps zmax's `scroll-lines`. Pure — unit tested.
fn mousescroll_lines(spec: &str) -> Option<usize> {
    spec.split(',').find_map(|part| {
        let (axis, n) = part.trim().split_once(':')?;
        (axis.trim() == "ver").then(|| n.trim().parse().ok())?
    })
}

/// vim `mousemodel`: whether the right mouse button extends the selection
/// (`extend`) rather than opening a popup menu (`popup`/`popup_setpos`). Pure —
/// unit tested.
fn mousemodel_extends(spec: &str) -> bool {
    spec.trim() == "extend"
}

fn workspace_trust_indicator_visible(editor: &Editor) -> bool {
    if editor.workspace_trust.implicit_level()
        == zmax_loader::workspace_trust::ImplicitTrustLevel::Insecure
    {
        return false;
    }
    let (_, doc) = zmax_view::current_ref!(editor);
    editor
        .workspace_trust
        .workspace_restricted(doc.workspace_root())
}

impl EditorView {
    /// must be called whenever the editor processed input that
    /// is not a `KeyEvent`. In these cases any pending keys/on next
    /// key callbacks must be canceled.
    fn handle_non_key_input(&mut self, cxt: &mut commands::Context) {
        cxt.editor.status_msg = None;
        cxt.editor.reset_idle_timer();
        // HACKS: create a fake key event that will never trigger any actual map
        // and therefore simply acts as "dismiss"
        let null_key_event = KeyEvent {
            code: KeyCode::Null,
            modifiers: KeyModifiers::empty(),
        };
        // dismiss any pending keys
        if let Some((on_next_key, _)) = self.on_next_key.take() {
            on_next_key(cxt, null_key_event);
        }
        self.handle_keymap_event(cxt.editor.mode, cxt, null_key_event);
        self.pseudo_pending.clear();
    }

    /// Emacs `gud-tooltip-mode`: while a debug session is stopped, pointing at an
    /// identifier evaluates it (DAP `evaluate`) and shows `name = value`.
    ///
    /// The value goes to the echo area — Emacs itself falls back to the echo area
    /// when frame tooltips are unavailable (`tooltip-mode` off), which is always
    /// the case in a terminal. Re-queried only when the word under the pointer
    /// changes, so a moving pointer does not flood the adapter.
    fn gud_tooltip(&mut self, cxt: &mut commands::Context, row: u16, column: u16) {
        if !crate::gud::tooltip_mode() {
            self.gud_tooltip_word = None;
            return;
        }
        let Some(frame_id) = crate::gud::selected_frame_id(cxt.editor) else {
            return;
        };
        let hit = cxt.editor.tree.views().find_map(|(view, _)| {
            view.pos_at_screen_coords(&cxt.editor.documents[&view.doc], row, column, true)
                .map(|pos| (view.doc, pos))
        });
        let Some((doc_id, pos)) = hit else {
            self.gud_tooltip_word = None;
            return;
        };
        let word = crate::gud::word_at(cxt.editor, doc_id, pos);
        if word == self.gud_tooltip_word {
            return;
        }
        self.gud_tooltip_word = word.clone();
        let Some(word) = word else { return };
        match crate::gud::eval_expression(cxt.editor, &word, Some(frame_id)) {
            Ok(value) => cxt.editor.set_status(format!("{word} = {value}")),
            // An identifier that is not in scope is the common case while the
            // pointer sweeps over code; say so quietly rather than as an error.
            Err(_) => cxt.editor.set_status(format!("{word}: not in scope")),
        }
    }

    /// Emacs `dictionary-tooltip-mode`: the word under the mouse pointer is
    /// looked up on the DICT server and its definition shown in a tooltip — a
    /// popup here, which is what a terminal frame has instead of a GUI tooltip.
    ///
    /// The lookup is a network round trip, so it runs on a blocking task and the
    /// popup is pushed from the job callback. Sweeping across the same word does
    /// not re-query.
    fn dictionary_tooltip(&mut self, cxt: &mut commands::Context, row: u16, column: u16) {
        if !crate::dictionary::tooltip_mode() {
            crate::dictionary::forget_hover();
            return;
        }
        let hit = cxt.editor.tree.views().find_map(|(view, _)| {
            view.pos_at_screen_coords(&cxt.editor.documents[&view.doc], row, column, true)
                .map(|pos| (view.doc, pos))
        });
        let Some((doc_id, pos)) = hit else {
            crate::dictionary::forget_hover();
            return;
        };
        let Some(doc) = cxt.editor.documents.get(&doc_id) else {
            return;
        };
        let text = doc.text().slice(..);
        let line_idx = text.char_to_line(pos.min(text.len_chars()));
        let col = pos - text.line_to_char(line_idx);
        let line: String = text
            .line(line_idx)
            .chars()
            .filter(|c| *c != '\n' && *c != '\r')
            .collect();
        let Some(word) = crate::dictionary::word_at(&line, col) else {
            crate::dictionary::forget_hover();
            return;
        };
        if !crate::dictionary::note_hover(&word) {
            return;
        }
        let loader = cxt.editor.syn_loader.clone();
        let query = word.clone();
        cxt.jobs.callback(async move {
            let defs = tokio::task::spawn_blocking(move || crate::dictionary::define(&query))
                .await
                .map_err(|e| anyhow::anyhow!("dictionary: {e}"))?;
            Ok(crate::job::Callback::EditorCompositor(Box::new(
                move |editor: &mut Editor, compositor: &mut crate::compositor::Compositor| {
                    match defs {
                        Ok(defs) if defs.is_empty() => {
                            editor.set_status(format!("dictionary: no match for {word}"))
                        }
                        Ok(defs) => {
                            let body = defs
                                .iter()
                                .map(|d| format!("**{}** — {}\n\n{}", d.word, d.database, d.text))
                                .collect::<Vec<_>>()
                                .join("\n\n---\n\n");
                            let contents = crate::ui::Markdown::new(body, loader);
                            let popup = crate::ui::Popup::new("dictionary-tooltip", contents)
                                .auto_close(true);
                            compositor.replace_or_push("dictionary-tooltip", popup);
                        }
                        Err(e) => editor.set_status(format!("dictionary: {e}")),
                    }
                },
            )))
        });
    }

    /// Route a `mouse-1` press that landed on one of the frame's bars. Returns
    /// `None` when the press was somewhere else, so the caller falls through to
    /// the ordinary text handling.
    fn handle_frame_bar_click(
        &mut self,
        cxt: &mut commands::Context,
        row: u16,
        column: u16,
    ) -> Option<EventResult> {
        // The menu bar: clicking a title drops that menu down under it.
        if crate::emacs_frame::menu_bar() && row == self.menu_bar_y {
            if let Some((x, _, index)) = self
                .menu_bar_hits
                .iter()
                .copied()
                .find(|(start, end, _)| column >= *start && column < *end)
            {
                crate::commands::open_menu_bar_menu(cxt, index, x, row + 1);
            }
            return Some(EventResult::Consumed(None));
        }

        // The tool bar: a button runs the command it names.
        if crate::emacs_frame::tool_bar() && row == self.tool_bar_y {
            if let Some((_, _, index)) = self
                .tool_bar_hits
                .iter()
                .copied()
                .find(|(start, end, _)| column >= *start && column < *end)
            {
                if let Some((_, spec)) = crate::emacs_frame::TOOL_BAR_BUTTONS.get(index) {
                    Self::run_bar_command(cxt, spec);
                }
            }
            return Some(EventResult::Consumed(None));
        }

        // The modifier bar: a button latches its modifier onto the next key.
        if crate::emacs_frame::modifier_bar() && row == self.modifier_bar_y {
            if let Some((_, _, index)) = self
                .modifier_bar_hits
                .iter()
                .copied()
                .find(|(start, end, _)| column >= *start && column < *end)
            {
                if let Some((label, m)) = crate::emacs_frame::MODIFIER_BAR_BUTTONS.get(index) {
                    crate::emacs_frame::toggle_sticky_modifier(*m);
                    let held = crate::emacs_frame::sticky_modifiers().contains(*m);
                    cxt.editor.set_status(if held {
                        format!("{label} applies to the next key")
                    } else {
                        format!("{label} released")
                    });
                }
            }
            return Some(EventResult::Consumed(None));
        }

        // A window's own tool bar, on its top row.
        let hit = cxt.editor.tree.views().find_map(|(view, _)| {
            let bar = view.window_tool_bar_area();
            (bar.height > 0 && row == bar.y && column >= bar.x && column < bar.right()).then(|| {
                (
                    view.id,
                    Self::button_hit(
                        bar,
                        crate::emacs_frame::WINDOW_TOOL_BAR_BUTTONS
                            .iter()
                            .map(|(label, _)| *label),
                        column,
                    ),
                )
            })
        });
        if let Some((view_id, index)) = hit {
            cxt.editor.focus(view_id);
            if let Some((_, spec)) =
                index.and_then(|i| crate::emacs_frame::WINDOW_TOOL_BAR_BUTTONS.get(i))
            {
                Self::run_bar_command(cxt, spec);
            }
            return Some(EventResult::Consumed(None));
        }

        None
    }

    /// Run the command a bar button names, whether it is a static command or a
    /// typable one (`:write`).
    fn run_bar_command(cxt: &mut commands::Context, spec: &str) {
        match spec.parse::<commands::MappableCommand>() {
            Ok(cmd) => cmd.execute(cxt),
            Err(err) => cxt.editor.set_error(err.to_string()),
        }
    }

    /// The window whose vertical scroll bar covers `(row, column)`, and how far
    /// down the bar the click landed (0.0 at the top, 1.0 at the bottom).
    fn scroll_bar_at(editor: &Editor, row: u16, column: u16) -> Option<(zmax_view::ViewId, f64)> {
        editor.tree.views().find_map(|(view, _)| {
            let bar = view.scroll_bar_area(&editor.documents[&view.doc]);
            (bar.width > 0
                && bar.height > 0
                && column == bar.x
                && row >= bar.y
                && row < bar.bottom())
            .then(|| (view.id, (row - bar.y) as f64 / bar.height.max(1) as f64))
        })
    }

    /// vim 'mousetime' (`mouset`, default 500): "Defines the maximum time in msec
    /// between two mouse clicks for the second click to be recognized as a multi
    /// click" (options.txt). Records this left-button press and returns the click
    /// count it makes. vim's own condition (src/mouse.c, `orig_num_clicks`) is:
    /// same button, less than 'mousetime' since the previous press, the previous
    /// count not already 4, and the same screen row *and* column — anything else
    /// restarts at 1, and the fifth click in a row is a plain click again.
    fn count_click(&mut self, column: u16, row: u16) -> u8 {
        let now = std::time::Instant::now();
        let mousetime = std::time::Duration::from_millis(
            crate::commands::typed::vim_opt_num("mousetime")
                .or_else(|| crate::commands::typed::vim_opt_num("mouset"))
                .unwrap_or(500) as u64,
        );
        self.click_count = match self.last_click {
            Some((last_column, last_row, when))
                if last_column == column
                    && last_row == row
                    && self.click_count != 4
                    && now.duration_since(when) < mousetime =>
            {
                self.click_count + 1
            }
            _ => 1,
        };
        self.last_click = Some((column, row, now));
        self.click_count
    }

    fn handle_mouse_event(
        &mut self,
        event: &MouseEvent,
        cxt: &mut commands::Context,
    ) -> EventResult {
        if event.kind != MouseEventKind::Moved {
            self.handle_non_key_input(cxt)
        }

        let config = cxt.editor.config();
        let MouseEvent {
            kind,
            row,
            column,
            modifiers,
            ..
        } = *event;

        let pos_and_view = |editor: &Editor, row, column, ignore_virtual_text| {
            editor.tree.views().find_map(|(view, _focus)| {
                view.pos_at_screen_coords(
                    &editor.documents[&view.doc],
                    row,
                    column,
                    ignore_virtual_text,
                )
                .map(|pos| (pos, view.id))
            })
        };

        // emacs's `mode-line` mouse area: the window's status line, i.e. the last
        // row of its area (`inner_area` clips exactly that row off the text).
        let mode_line_view = |editor: &Editor, row: u16, column: u16| {
            editor.tree.views().find_map(|(view, _focus)| {
                let a = view.area;
                (a.height > 0
                    && row == a.bottom().saturating_sub(1)
                    && column >= a.left()
                    && column < a.right())
                .then_some(view.id)
            })
        };

        let gutter_coords_and_view = |editor: &Editor, row, column| {
            editor.tree.views().find_map(|(view, _focus)| {
                view.gutter_coords_at_screen_coords(row, column)
                    .map(|coords| (coords, view.id))
            })
        };

        match kind {
            MouseEventKind::Down(MouseButton::Left) => {
                // vim 'mousetime': how many clicks in a row this press makes. Every
                // press of the button counts, wherever it landed — a press
                // somewhere else is what ends a multi click.
                let clicks = self.count_click(column, row);
                // emacs's frame bars own their rows: a click on the menu bar drops
                // that menu down, a click on the tool bar or modifier bar presses
                // that button.
                if let Some(result) = self.handle_frame_bar_click(cxt, row, column) {
                    return result;
                }

                // emacs `mouse-1` on a scroll bar drags the window through the
                // buffer for as long as the button is held — run `scroll-bar-drag`,
                // so the command really is the scroll bar's handler.
                if let Some((view_id, y_frac)) = Self::scroll_bar_at(cxt.editor, row, column) {
                    cxt.editor.focus(view_id);
                    self.scroll_bar_drag = Some(view_id);
                    crate::emacs_frame::set_scroll_bar_click(y_frac);
                    commands::MappableCommand::scroll_bar_drag.execute(cxt);
                    return EventResult::Consumed(None);
                }

                let editor = &mut cxt.editor;

                // A press on a split divider (the border between panes — vertical
                // between side-by-side panes, horizontal between stacked panes)
                // starts a drag-to-resize instead of moving the cursor.
                if let Some((view_id, vertical)) = editor.tree.split_divider_at(column, row) {
                    // Record where on the divider we grabbed relative to its
                    // actual edge, so dragging tracks the cursor without jumping
                    // when the grab point isn't exactly on the edge cell.
                    let area = editor.tree.try_get(view_id).map(|v| v.area);
                    let offset = area
                        .map(|a| {
                            if vertical {
                                column as i16 - a.right() as i16
                            } else {
                                row as i16 - a.bottom() as i16
                            }
                        })
                        .unwrap_or(0);
                    // emacs `[mode-line mouse-1]` / `[vertical-line mouse-1]` is
                    // `mouse-select-window`, and `[mode-line down-mouse-1]` is
                    // `mouse-drag-mode-line` — the divider a window owns is its
                    // mode line, so the press both selects that window and starts
                    // the drag that moves the boundary.
                    editor.last_mouse_view = Some(view_id);
                    commands::MappableCommand::mouse_select_window.execute(cxt);
                    self.resize_drag = Some((view_id, vertical, offset));
                    return EventResult::Consumed(None);
                }

                // The bottom window's mode line is no divider — nothing can be
                // dragged there — but `mouse-1` on it still selects the window.
                if let Some(view_id) = mode_line_view(cxt.editor, row, column) {
                    cxt.editor.last_mouse_view = Some(view_id);
                    commands::MappableCommand::mouse_select_window.execute(cxt);
                    return EventResult::Consumed(None);
                }
                let editor = &mut cxt.editor;

                if let Some((pos, view_id)) = pos_and_view(editor, row, column, true) {
                    editor.focus(view_id);

                    let prev_view_id = view!(editor).id;
                    let doc = doc_mut!(editor, &view!(editor, view_id).doc);
                    // Emacs's mouse commands take the click as their argument
                    // (`(interactive "e")`); this is that argument.
                    let doc_id = doc.id();

                    // vim `<2-LeftMouse>`/`<3-LeftMouse>`/`<4-LeftMouse>`
                    // (term.txt *double-click*): "For selecting text, extra clicks
                    // extend the selection: double = word or % match, triple =
                    // line, quadruple = rectangular block". Only the unmodified
                    // button multi-clicks — the Ctrl/Shift/Alt clicks below keep
                    // their own single-click meanings — and the click still places
                    // the cursor first, exactly as a single click does.
                    if modifiers.is_empty() && clicks > 1 {
                        let selected = {
                            let text = doc.text().slice(..);
                            if clicks == 2 {
                                // "A double click on a character that has a match
                                // selects until that match (like using v%)."
                                // Anything else selects the word under the click,
                                // whose bounds are the same word categories `viw`
                                // uses. (vim additionally turns the selection
                                // linewise when the match is an #if/#else/#endif
                                // block; zmax's `%` has no preprocessor matching to
                                // build that on.)
                                let matched = doc.syntax().map_or_else(
                                    || {
                                        zmax_core::match_brackets::find_matching_bracket_plaintext(
                                            text, pos,
                                        )
                                    },
                                    |syntax| {
                                        zmax_core::match_brackets::find_matching_bracket_fuzzy(
                                            syntax, text, pos,
                                        )
                                    },
                                );
                                match matched {
                                    Some(to) => Range::point(pos).put_cursor(text, to, true),
                                    None => zmax_core::textobject::textobject_word(
                                        text,
                                        Range::point(pos),
                                        zmax_core::textobject::TextObject::Inside,
                                        1,
                                        false,
                                    ),
                                }
                            } else {
                                // The triple/quadruple click only anchors the
                                // cursor — the whole-line span and the rectangle
                                // are derived by the Visual sub-mode entered below.
                                Range::point(pos)
                            }
                        };
                        doc.set_selection(
                            view_id,
                            Selection::single(selected.anchor, selected.head),
                        );
                        // Visual-line and visual-block are mutually exclusive and
                        // both commands below are toggles, so whatever the previous
                        // click left has to go first — otherwise a triple click
                        // after a triple click would *leave* Visual instead of
                        // re-anchoring it on the new line.
                        cxt.editor.visual_line = None;
                        cxt.editor.block = None;
                        cxt.editor.last_mouse_pos = Some((doc_id, pos));
                        match clicks {
                            3 => commands::MappableCommand::visual_line_mode.execute(cxt),
                            4 => commands::MappableCommand::visual_block_mode.execute(cxt),
                            _ => commands::MappableCommand::select_mode.execute(cxt),
                        }
                        if view_id != prev_view_id {
                            self.clear_completion(cxt.editor);
                        }
                        cxt.editor.ensure_cursor_in_view(view_id);
                        return EventResult::Consumed(None);
                    }

                    if modifiers == KeyModifiers::CONTROL {
                        editor.last_mouse_pos = Some((doc_id, pos));
                        if editor.vim_semantics {
                            // vim `<C-LeftMouse>` / `g<LeftMouse>`: go to the tag
                            // (definition) of the symbol at the click.
                            commands::MappableCommand::mouse_goto_tag.execute(cxt);
                        } else {
                            // emacs `C-down-mouse-1` is `mouse-buffer-menu`: the
                            // menu of buffers to switch to.
                            commands::MappableCommand::mouse_buffer_menu.execute(cxt);
                        }
                        return EventResult::Consumed(None);
                    } else if modifiers == KeyModifiers::ALT && !editor.vim_semantics {
                        // emacs `M-mouse-1` is `mouse-start-secondary`: the press
                        // anchors the *secondary* selection, which the drag below
                        // then sets. (Under the vim presets Alt-click keeps adding
                        // a cursor, which is the branch after this one.)
                        editor.last_mouse_pos = Some((doc_id, pos));
                        commands::MappableCommand::mouse_start_secondary.execute(cxt);
                        return EventResult::Consumed(None);
                    } else if modifiers == KeyModifiers::SHIFT && editor.mode == Mode::Normal {
                        // vim `<S-LeftMouse>`: `*` at the click position. Normal mode
                        // only — vim lists this as a normal-mode binding, and in
                        // Select mode a click still extends the selection below.
                        editor.last_mouse_pos = Some((doc_id, pos));
                        commands::MappableCommand::mouse_search_word_forward.execute(cxt);
                        return EventResult::Consumed(None);
                    } else if modifiers == KeyModifiers::ALT {
                        let selection = doc.selection(view_id).clone();
                        doc.set_selection(view_id, selection.push(Range::point(pos)));
                        editor.last_mouse_pos = Some((doc_id, pos));
                    } else if editor.mode == Mode::Select {
                        // Discards non-primary selections for consistent UX with normal mode
                        let primary = doc.selection(view_id).primary().put_cursor(
                            doc.text().slice(..),
                            pos,
                            true,
                        );
                        editor.mouse_down_range = Some(primary);
                        doc.set_selection(view_id, Selection::single(primary.anchor, primary.head));
                        editor.last_mouse_pos = Some((doc_id, pos));
                    } else {
                        // mouse-1 on text *is* emacs `mouse-set-point` — run the
                        // command, so the command really is the mouse's handler.
                        editor.last_mouse_pos = Some((doc_id, pos));
                        commands::mouse_set_point(cxt);
                    }

                    if view_id != prev_view_id {
                        self.clear_completion(cxt.editor);
                    }

                    cxt.editor.ensure_cursor_in_view(view_id);

                    return EventResult::Consumed(None);
                }

                if let Some((coords, view_id)) = gutter_coords_and_view(editor, row, column) {
                    editor.focus(view_id);

                    let (view, doc) = current!(cxt.editor);

                    let Some(path) = doc.path().map(ToOwned::to_owned) else {
                        return EventResult::Ignored(None);
                    };

                    if let Some(char_idx) =
                        view.pos_at_visual_coords(doc, coords.row as u16, coords.col as u16, true)
                    {
                        let line = doc.text().char_to_line(char_idx);
                        commands::dap_toggle_breakpoint_impl(cxt, path, line);
                        return EventResult::Consumed(None);
                    }
                }

                // Fall back to focusing whichever pane the click landed in, even
                // if it wasn't on text or the gutter (e.g. the blank area below a
                // short buffer, or another split). A click should always move
                // focus to the pane it hit so the next keystrokes go there.
                let clicked_view = cxt.editor.tree.views().find_map(|(view, _)| {
                    let a = view.area;
                    (column >= a.x
                        && column < a.x.saturating_add(a.width)
                        && row >= a.y
                        && row < a.y.saturating_add(a.height))
                    .then_some(view.id)
                });
                if let Some(view_id) = clicked_view {
                    cxt.editor.focus(view_id);
                    return EventResult::Consumed(None);
                }

                EventResult::Ignored(None)
            }

            MouseEventKind::Drag(MouseButton::Left) => {
                // A scroll-bar drag keeps scrolling the window it started on, even
                // once the pointer leaves the bar — emacs tracks the button, not
                // the position.
                if let Some(view_id) = self.scroll_bar_drag {
                    let bar = cxt
                        .editor
                        .tree
                        .try_get(view_id)
                        .map(|v| v.scroll_bar_area(&cxt.editor.documents[&v.doc]));
                    if let Some(bar) = bar.filter(|b| b.height > 0) {
                        let row_in_bar =
                            (row.saturating_sub(bar.y)).min(bar.height.saturating_sub(1));
                        crate::emacs_frame::set_scroll_bar_click(
                            row_in_bar as f64 / bar.height.max(1) as f64,
                        );
                        cxt.editor.focus(view_id);
                        commands::MappableCommand::scroll_bar_drag.execute(cxt);
                    }
                    return EventResult::Consumed(None);
                }

                // If a divider drag is in progress, move the divider to follow the
                // cursor *absolutely*: the target edge is the current mouse
                // position minus the grab offset, and we resize by the difference
                // from the divider's current edge. Computing from absolute
                // positions (rather than per-event deltas) means the divider never
                // drifts away from the cursor when a step is clamped at the minimum
                // pane size. The resize fns pin siblings and recalculate internally.
                if let Some((view_id, vertical, offset)) = self.resize_drag {
                    if let Some(area) = cxt.editor.tree.try_get(view_id).map(|v| v.area) {
                        if vertical {
                            let target = column as i16 - offset;
                            let delta = target - area.right() as i16;
                            if delta != 0 {
                                cxt.editor.tree.resize_horizontal(view_id, delta);
                            }
                        } else {
                            let target = row as i16 - offset;
                            let delta = target - area.bottom() as i16;
                            if delta != 0 {
                                cxt.editor.tree.resize_vertical(view_id, delta);
                            }
                        }
                    }
                    return EventResult::Consumed(None);
                }

                let (view, doc) = current!(cxt.editor);

                let pos = match view.pos_at_screen_coords(doc, row, column, true) {
                    Some(pos) => pos,
                    None => return EventResult::Ignored(None),
                };
                let (view_id, doc_id) = (view.id, doc.id());
                cxt.editor.last_mouse_pos = Some((doc_id, pos));

                // emacs `M-drag-mouse-1` is `mouse-set-secondary`: the drag sets
                // the *secondary* selection, from the anchor `M-mouse-1` dropped
                // to where the pointer is now, and leaves point and the region
                // where they were.
                if modifiers == KeyModifiers::ALT && !cxt.editor.vim_semantics {
                    commands::MappableCommand::mouse_set_secondary.execute(cxt);
                    return EventResult::Consumed(None);
                }

                // Dragging mouse-1 *is* emacs `mouse-set-region`: the region runs
                // from where the drag started to where the pointer is now. vim
                // `mouse=a` semantics ride along — a non-empty drag puts the editor
                // in Select mode so operators act on it, and collapsing the drag
                // back to a caret leaves Select again.
                commands::mouse_set_region(cxt);
                let empty = {
                    let (view, doc) = current_ref!(cxt.editor);
                    let primary = doc.selection(view.id).primary();
                    primary.anchor == primary.head
                };
                if empty && cxt.editor.mode == Mode::Select {
                    cxt.editor.mode = Mode::Normal;
                }
                cxt.editor.ensure_cursor_in_view(view_id);
                EventResult::Consumed(None)
            }

            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                // emacs `mouse-wheel-mode`: with the mode off, the wheel does not
                // scroll (the event is simply not ours).
                if !cxt.editor.mouse_wheel_mode {
                    return EventResult::Ignored(None);
                }
                let current_view = cxt.editor.tree.focus;

                let direction = match event.kind {
                    MouseEventKind::ScrollUp => Direction::Backward,
                    MouseEventKind::ScrollDown => Direction::Forward,
                    _ => unreachable!(),
                };
                // The wheel event is the argument emacs hands
                // `mouse-wheel-text-scale`; record its direction so the command is
                // also invokable on its own.
                crate::emacs_frame::set_last_wheel_up(direction == Direction::Backward);

                match pos_and_view(cxt.editor, row, column, false) {
                    Some((_, view_id)) => cxt.editor.tree.focus = view_id,
                    None => return EventResult::Ignored(None),
                }

                // emacs `mouse-wheel-scroll-amount` maps the control modifier to
                // `text-scale` and control+meta to `global-text-scale`, so with
                // either held the wheel resizes the font instead of scrolling.
                // Wheel *up* is emacs's `mouse-wheel-down-event`, which is the
                // "larger" direction for both — run the commands, so the commands
                // really are the wheel's handlers.
                if modifiers == (KeyModifiers::CONTROL | KeyModifiers::ALT) {
                    // `mouse-wheel-global-text-scale`. zmax's
                    // `global-text-scale-adjust` reads its increment as a following
                    // +/-/0 key; the wheel supplies that key itself, which is emacs
                    // calling the command with an explicit increment.
                    commands::MappableCommand::global_text_scale_adjust.execute(cxt);
                    if let Some((on_next_key, _)) = cxt.on_next_key_callback.take() {
                        let increment = KeyEvent {
                            code: KeyCode::Char(match direction {
                                Direction::Backward => '+',
                                Direction::Forward => '-',
                            }),
                            modifiers: KeyModifiers::empty(),
                        };
                        on_next_key(cxt, increment);
                    }
                } else if modifiers == KeyModifiers::CONTROL {
                    // `C-wheel-up` / `C-wheel-down` are bound to
                    // `mouse-wheel-text-scale`; run that command, so the command
                    // really is the wheel's handler.
                    commands::MappableCommand::mouse_wheel_text_scale.execute(cxt);
                }
                // vim `<S-ScrollWheelDown>` / `<S-ScrollWheelUp>`: shift makes the
                // wheel move the window a whole page — run the command, so the
                // command really is the mouse's handler. Otherwise
                // `mousescroll=ver:N` decides how many lines one notch scrolls.
                else if modifiers == KeyModifiers::SHIFT {
                    match direction {
                        Direction::Backward => {
                            commands::MappableCommand::mouse_scroll_page_up.execute(cxt)
                        }
                        Direction::Forward => {
                            commands::MappableCommand::mouse_scroll_page_down.execute(cxt)
                        }
                    }
                } else {
                    let offset = crate::commands::vim_opt_str("mousescroll")
                        .and_then(|spec| mousescroll_lines(&spec))
                        .unwrap_or_else(|| config.scroll_lines.unsigned_abs());
                    commands::scroll(cxt, offset, direction, false);
                }

                cxt.editor.tree.focus = current_view;
                cxt.editor.ensure_cursor_in_view(current_view);

                EventResult::Consumed(None)
            }

            // vim `<ScrollWheelLeft>` / `<ScrollWheelRight>`: move the window
            // `mousescroll=hor:N` columns (6 by default); `<S-ScrollWheelLeft>` /
            // `<S-ScrollWheelRight>` move it a whole page. This shifts the viewport
            // only — the cursor does not move, so `ensure_cursor_in_view` would
            // undo it and is deliberately not called.
            MouseEventKind::ScrollLeft | MouseEventKind::ScrollRight => {
                if !cxt.editor.mouse_wheel_mode {
                    return EventResult::Ignored(None);
                }
                let current_view = cxt.editor.tree.focus;
                match pos_and_view(cxt.editor, row, column, false) {
                    Some((_, view_id)) => cxt.editor.tree.focus = view_id,
                    None => return EventResult::Ignored(None),
                }

                let left = event.kind == MouseEventKind::ScrollLeft;
                let page = modifiers == KeyModifiers::SHIFT;
                match (left, page) {
                    (true, false) => commands::MappableCommand::mouse_scroll_left.execute(cxt),
                    (false, false) => commands::MappableCommand::mouse_scroll_right.execute(cxt),
                    (true, true) => commands::MappableCommand::mouse_scroll_page_left.execute(cxt),
                    (false, true) => {
                        commands::MappableCommand::mouse_scroll_page_right.execute(cxt)
                    }
                }

                cxt.editor.tree.focus = current_view;
                EventResult::Consumed(None)
            }

            MouseEventKind::Up(MouseButton::Left) => {
                // End an in-progress scroll-bar or pane-divider drag.
                if self.scroll_bar_drag.take().is_some() {
                    return EventResult::Consumed(None);
                }
                if self.resize_drag.take().is_some() {
                    return EventResult::Consumed(None);
                }

                if !config.middle_click_paste {
                    return EventResult::Ignored(None);
                }

                let (view, doc) = current!(cxt.editor);

                let should_yank = match cxt.editor.mouse_down_range.take() {
                    Some(down_range) => doc.selection(view.id).primary() != down_range,
                    None => {
                        // This should not happen under normal cases. We fall back to the original
                        // behavior of yanking on non-single-char selections.
                        doc.selection(view.id)
                            .primary()
                            .slice(doc.text().slice(..))
                            .len_chars()
                            > 1
                    }
                };

                if should_yank {
                    commands::yank_main_selection_to_register(
                        cxt.editor,
                        config.mouse_yank_register,
                    );
                    EventResult::Consumed(None)
                } else {
                    EventResult::Ignored(None)
                }
            }

            // emacs mouse-2 on the mode line is `mouse-delete-other-windows`, and
            // mouse-3 is `mouse-delete-window`; both are click events, so the
            // press only records where the click began.
            MouseEventKind::Down(MouseButton::Middle) => {
                // emacs `C-mouse-2` on a scroll bar is `mouse-split-window-vertically`:
                // the window splits at the line the click named.
                if modifiers == KeyModifiers::CONTROL {
                    if let Some((view_id, frac)) = Self::scroll_bar_at(cxt.editor, row, column) {
                        cxt.editor.focus(view_id);
                        crate::emacs_frame::set_scroll_bar_click(frac);
                        commands::MappableCommand::mouse_split_window_vertically.execute(cxt);
                        return EventResult::Consumed(None);
                    }
                }
                self.mode_line_press = mode_line_view(cxt.editor, row, column);
                if self.mode_line_press.is_some() {
                    return EventResult::Consumed(None);
                }
                // emacs binds `C-down-mouse-2` to `facemenu-menu` (facemenu.el):
                // the Text Properties menu — the faces and colors of the text.
                // Only on the non-vim presets, where the middle button is not
                // vim's paste.
                if modifiers == KeyModifiers::CONTROL && !cxt.editor.vim_semantics {
                    if let Some((_, view_id)) = pos_and_view(cxt.editor, row, column, true) {
                        cxt.editor.focus(view_id);
                    }
                    commands::MappableCommand::facemenu.execute(cxt);
                    return EventResult::Consumed(None);
                }
                EventResult::Ignored(None)
            }

            MouseEventKind::Down(MouseButton::Right) => {
                // emacs `C-mouse-3`: with the menu bar turned off, the right button
                // plus Control pops the menu bar's own tree up at the pointer, so
                // the menus stay reachable without a row for them.
                if modifiers == KeyModifiers::CONTROL && !crate::emacs_frame::menu_bar() {
                    commands::MappableCommand::menu_bar_open.execute(cxt);
                    return EventResult::Consumed(None);
                }
                if let Some(view_id) = mode_line_view(cxt.editor, row, column) {
                    self.mode_line_press = Some(view_id);
                    return EventResult::Consumed(None);
                }
                self.mode_line_press = None;

                // Right-click on editor text → JetBrains-style context menu. Only
                // for actual text positions: gutter right-clicks map to no text
                // position and fall through to the DAP breakpoint handler (on Up).
                // (gutter_coords_at_screen_coords returns Some for the whole view,
                // so it can't distinguish text from gutter — use pos_and_view.)
                let Some((click_pos, click_view)) = pos_and_view(cxt.editor, row, column, true)
                else {
                    return EventResult::Ignored(None);
                };
                let click_doc = cxt.editor.tree.get(click_view).doc;
                cxt.editor.last_mouse_pos = Some((click_doc, click_pos));
                // emacs `ffap-bindings` binds `C-S-mouse-3` to `ffap-menu`: a menu
                // of every file/URL mentioned in the buffer, ordered by position.
                // Only on the non-vim presets — under `vim`/`spacemacs` the right
                // button keeps its `mousemodel`/`#` meaning.
                if modifiers == KeyModifiers::CONTROL | KeyModifiers::SHIFT
                    && !cxt.editor.vim_semantics
                {
                    cxt.editor.focus(click_view);
                    cxt.editor.last_mouse_screen = Some((row, column));
                    commands::MappableCommand::ffap_menu.execute(cxt);
                    return EventResult::Consumed(None);
                }
                if modifiers == KeyModifiers::SHIFT && !cxt.editor.vim_semantics {
                    // emacs `ffap-bindings` binds `S-mouse-3` to `ffap-at-mouse`:
                    // point moves to the click and the file/URL guessed from the
                    // text there is fetched. With nothing to guess, ffap says so
                    // and does not open anything.
                    cxt.editor.focus(click_view);
                    commands::MappableCommand::ffap_at_mouse.execute(cxt);
                    return EventResult::Consumed(None);
                }
                // emacs `M-mouse-3` is `mouse-secondary-save-then-kill`, which the
                // release runs — the press must not pop a menu up in front of it.
                if modifiers == KeyModifiers::ALT && !cxt.editor.vim_semantics {
                    return EventResult::Consumed(None);
                }
                if modifiers == KeyModifiers::CONTROL {
                    if cxt.editor.vim_semantics {
                        // vim `<C-RightMouse>` / `g<RightMouse>`: same as CTRL-T —
                        // pop the tag/jump stack back to where the last jump began.
                        cxt.editor.focus(click_view);
                        commands::MappableCommand::mouse_pop_tag.execute(cxt);
                        return EventResult::Consumed(None);
                    }
                    // emacs `C-mouse-3`: with the menu bar disabled this pops up the
                    // menu-bar menus themselves. zmax has no menu bar to disable, so
                    // it always pops the same menu the right button otherwise would.
                    cxt.editor.focus(click_view);
                    let path = doc!(cxt.editor).path().map(|p| p.to_path_buf());
                    let cb: crate::compositor::Callback =
                        Box::new(move |compositor: &mut crate::compositor::Compositor, _cx| {
                            use crate::ui::context_menu::ContextMenu;
                            let entries = editor_menu_entries(path.clone());
                            compositor.push(Box::new(ContextMenu::new(row, column, entries)));
                        });
                    return EventResult::Consumed(Some(cb));
                }
                if modifiers == KeyModifiers::SHIFT && cxt.editor.mode == Mode::Normal {
                    // vim `<S-RightMouse>`: `#` at the click position. Normal mode
                    // only, matching `<S-LeftMouse>`; elsewhere the right button
                    // keeps its `mousemodel` meaning below.
                    cxt.editor.focus(click_view);
                    commands::MappableCommand::mouse_search_word_backward.execute(cxt);
                    return EventResult::Consumed(None);
                }
                // vim `mousemodel=extend`: the right button extends the selection to
                // the click instead of popping up a menu (`popup`/`popup_setpos`).
                // That is emacs's mouse-3 exactly — `mouse-save-then-kill`: extend
                // the region to the click and save it; press again in the same place
                // and it is killed.
                if mousemodel_extends(
                    crate::commands::vim_opt_str("mousemodel")
                        .as_deref()
                        .unwrap_or("popup_setpos"),
                ) {
                    cxt.editor.focus(click_view);
                    commands::mouse_save_then_kill(cxt);
                    return EventResult::Consumed(None);
                }
                // emacs `context-menu-mode`: with the mode off there is no popup.
                if !cxt.editor.context_menu_mode {
                    return EventResult::Ignored(None);
                }
                // `down-mouse-3` is what `context-menu-mode` binds, and it binds it
                // to the same `context-menu-open` that `S-<f10>` runs — so the
                // press dispatches through that command, at the click.
                cxt.editor.focus(click_view);
                cxt.editor.last_mouse_screen = Some((row, column));
                commands::MappableCommand::context_menu_open.execute(cxt);
                EventResult::Consumed(None)
            }

            MouseEventKind::Up(MouseButton::Right) => {
                // emacs `mouse-delete-window`: delete the window whose mode line
                // was clicked, doing nothing when it is the only one
                // (`one-window-p`) or when the button was released somewhere
                // other than the mode line it was pressed on.
                let pressed = self.mode_line_press.take();
                if let Some(view_id) = mode_line_view(cxt.editor, row, column) {
                    if pressed == Some(view_id) {
                        if cxt.editor.tree.views().count() > 1 {
                            cxt.editor.focus(view_id);
                            commands::MappableCommand::wclose.execute(cxt);
                        }
                        return EventResult::Consumed(None);
                    }
                }

                // emacs `M-mouse-3` is `mouse-secondary-save-then-kill`: the
                // secondary selection is copied, and killed by a second press.
                if modifiers == KeyModifiers::ALT && !cxt.editor.vim_semantics {
                    if let Some((_, view_id)) = pos_and_view(cxt.editor, row, column, true) {
                        cxt.editor.focus(view_id);
                        commands::MappableCommand::mouse_secondary_save_then_kill.execute(cxt);
                        return EventResult::Consumed(None);
                    }
                }

                if let Some((pos, view_id)) = gutter_coords_and_view(cxt.editor, row, column) {
                    cxt.editor.focus(view_id);

                    if let Some((pos, _)) = pos_and_view(cxt.editor, row, column, true) {
                        doc_mut!(cxt.editor).set_selection(view_id, Selection::point(pos));
                    } else {
                        let (view, doc) = current!(cxt.editor);

                        if let Some(pos) = view.pos_at_visual_coords(doc, pos.row as u16, 0, true) {
                            doc.set_selection(view_id, Selection::point(pos));
                            match modifiers {
                                KeyModifiers::ALT => {
                                    commands::MappableCommand::dap_edit_log.execute(cxt)
                                }
                                _ => commands::MappableCommand::dap_edit_condition.execute(cxt),
                            };
                        }
                    }

                    cxt.editor.ensure_cursor_in_view(view_id);
                    return EventResult::Consumed(None);
                }
                EventResult::Ignored(None)
            }

            MouseEventKind::Up(MouseButton::Middle) => {
                // emacs `mouse-delete-other-windows`: the window whose mode line
                // was clicked becomes the only one. Not a paste, so it runs
                // ahead of the `middle_click_paste` gate below.
                let pressed = self.mode_line_press.take();
                if let Some(view_id) = mode_line_view(cxt.editor, row, column) {
                    if pressed == Some(view_id) {
                        cxt.editor.last_mouse_view = Some(view_id);
                        // emacs `C-mouse-2` on the mode line is
                        // `mouse-split-window-horizontally`: the clicked window
                        // becomes two *side-by-side* windows with the boundary
                        // running through the click, where plain `mouse-2` makes
                        // it the only one.
                        if modifiers == KeyModifiers::CONTROL {
                            cxt.editor.last_mouse_screen = Some((row, column));
                            commands::MappableCommand::mouse_split_window_horizontally.execute(cxt);
                        } else {
                            cxt.editor.focus(view_id);
                            commands::MappableCommand::wonly.execute(cxt);
                        }
                        return EventResult::Consumed(None);
                    }
                }

                // The `C-down-mouse-2` press already opened the Text Properties
                // menu; the release must not paste on top of it.
                if modifiers == KeyModifiers::CONTROL && !cxt.editor.vim_semantics {
                    return EventResult::Consumed(None);
                }

                // emacs `S-mouse-2` under hs-minor-mode is `hs-toggle-hiding`: the
                // block the click landed in folds, or unfolds when it was folded.
                // Not a paste, so it runs ahead of the `middle_click_paste` gate.
                if modifiers == KeyModifiers::SHIFT {
                    if let Some((pos, view_id)) = pos_and_view(cxt.editor, row, column, true) {
                        let doc_id = cxt.editor.tree.get(view_id).doc;
                        cxt.editor.focus(view_id);
                        cxt.editor.last_mouse_pos = Some((doc_id, pos));
                        commands::MappableCommand::hs_toggle_hiding.execute(cxt);
                        return EventResult::Consumed(None);
                    }
                }

                // emacs `M-mouse-2` is `mouse-yank-secondary`: the secondary
                // selection is inserted at the click, and stays where it is.
                if modifiers == KeyModifiers::ALT && !cxt.editor.vim_semantics {
                    if let Some((pos, view_id)) = pos_and_view(cxt.editor, row, column, true) {
                        let doc_id = cxt.editor.tree.get(view_id).doc;
                        cxt.editor.focus(view_id);
                        cxt.editor.last_mouse_pos = Some((doc_id, pos));
                    }
                    commands::MappableCommand::mouse_yank_secondary.execute(cxt);
                    return EventResult::Consumed(None);
                }

                let editor = &mut cxt.editor;
                if !config.middle_click_paste {
                    return EventResult::Ignored(None);
                }

                if modifiers == KeyModifiers::ALT {
                    commands::replace_selections_with_register(
                        cxt.editor,
                        config.mouse_yank_register,
                        cxt.count(),
                    );

                    return EventResult::Consumed(None);
                }

                if let Some((pos, view_id)) = pos_and_view(editor, row, column, true) {
                    let doc_id = view!(editor, view_id).doc;
                    editor.last_mouse_pos = Some((doc_id, pos));
                    cxt.editor.focus(view_id);

                    // vim `[<MiddleMouse>` / `]<MiddleMouse>`: a pending `[` or `]`
                    // prefix turns the middle click into `[p` / `]p` at the click.
                    // The mouse event completes the chord, so clear the prefix
                    // ourselves — it never reaches `Keymaps::get`.
                    let pending = self.keymaps.pending();
                    let prefix =
                        (pending.len() == 1)
                            .then(|| pending[0])
                            .and_then(|k| match k.code {
                                KeyCode::Char('[') => Some(false),
                                KeyCode::Char(']') => Some(true),
                                _ => None,
                            });
                    if let Some(after) = prefix {
                        self.keymaps.clear_pending();
                        if after {
                            commands::MappableCommand::mouse_paste_after.execute(cxt);
                        } else {
                            commands::MappableCommand::mouse_paste_before.execute(cxt);
                        }
                        return EventResult::Consumed(None);
                    }

                    // mouse-2 on text is emacs `mouse-yank-at-click`: point moves
                    // to the click and the kill ring's top is inserted there.
                    commands::mouse_yank_at_click(cxt);
                    return EventResult::Consumed(None);
                }

                EventResult::Ignored(None)
            }

            // vim `mousefocus`: the window under the mouse pointer takes focus as
            // the pointer moves over it, without a click.
            MouseEventKind::Moved => {
                // `gud-tooltip-mode`: pointing at an identifier during a stopped
                // debug session shows its value.
                self.gud_tooltip(cxt, row, column);
                // `dictionary-tooltip-mode`: pointing at a word shows its
                // dictionary definition.
                self.dictionary_tooltip(cxt, row, column);
                if !crate::commands::vim_opt_bool("mousefocus") {
                    return EventResult::Ignored(None);
                }
                match pos_and_view(cxt.editor, row, column, true) {
                    Some((_, view_id)) if view_id != cxt.editor.tree.focus => {
                        cxt.editor.focus(view_id);
                        EventResult::Consumed(None)
                    }
                    _ => EventResult::Ignored(None),
                }
            }

            _ => EventResult::Ignored(None),
        }
    }
    /// Arm an on-next-key handler from *outside* a command turn — a compositor
    /// callback that replays keys must install its follow-up question after the
    /// replay, or the handler would swallow the replayed keys itself. Used by
    /// `kmacro-step-edit-macro`, which executes each accepted key as it is
    /// accepted and then asks about the next one.
    pub fn arm_on_next_key(&mut self, callback: Option<(OnKeyCallback, OnKeyCallbackKind)>) {
        self.on_next_key = callback;
    }

    fn on_next_key(
        &mut self,
        kind: OnKeyCallbackKind,
        ctx: &mut commands::Context,
        event: KeyEvent,
    ) -> bool {
        if let Some((on_next_key, kind_)) = self.on_next_key.take() {
            if kind == kind_ {
                on_next_key(ctx, event);
                true
            } else {
                self.on_next_key = Some((on_next_key, kind_));
                false
            }
        } else {
            false
        }
    }
}

impl Component for EditorView {
    fn handle_event(
        &mut self,
        event: &Event,
        context: &mut crate::compositor::Context,
    ) -> EventResult {
        // IDE workbench: F2 toggles; focused panels capture keys; clicks in a panel route here.
        if let Event::Key(key) = event {
            if key.code == KeyCode::F(2) && key.modifiers.is_empty() {
                self.ide_or_create().toggle();
                return EventResult::Consumed(None);
            }
            // Run the current file (F5) / Debug (F6), regardless of panel focus.
            if key.code == KeyCode::F(5) && key.modifiers.is_empty() {
                let cb = self.apply_ide_action(IdeAction::RunStart, context);
                return EventResult::Consumed(cb);
            }
            if key.code == KeyCode::F(6) && key.modifiers.is_empty() {
                let cb = self.apply_ide_action(IdeAction::Debug, context);
                return EventResult::Consumed(cb);
            }
            if self.ide.as_ref().is_some_and(Ide::capturing) {
                let action = self.ide.as_mut().unwrap().handle_key(*key);
                let cb = self.apply_ide_action(action, context);
                return EventResult::Consumed(cb);
            }
        }
        if let Event::Mouse(me) = event {
            // Scroll the wheel over the bufferline to cycle through buffers.
            if me.row == self.bufferline_y
                && matches!(
                    me.kind,
                    MouseEventKind::ScrollDown | MouseEventKind::ScrollUp
                )
            {
                let docs: Vec<zmax_view::DocumentId> =
                    context.editor.documents().map(|d| d.id()).collect();
                if docs.len() > 1 {
                    let cur = view!(context.editor).doc;
                    if let Some(i) = docs.iter().position(|d| *d == cur) {
                        let next = if matches!(me.kind, MouseEventKind::ScrollDown) {
                            (i + 1) % docs.len()
                        } else {
                            (i + docs.len() - 1) % docs.len()
                        };
                        context
                            .editor
                            .switch(docs[next], zmax_view::editor::Action::Replace);
                    }
                }
                return EventResult::Consumed(None);
            }
            // Right-click a bufferline tab → context menu (close / split / reveal).
            if me.row == self.bufferline_y
                && matches!(me.kind, MouseEventKind::Down(MouseButton::Right))
            {
                if let Some(&(_, _, _, doc_id)) = self
                    .bufferline_tabs
                    .iter()
                    .find(|(a, b, _, _)| me.column >= *a && me.column < *b)
                {
                    use crate::ui::context_menu::{ContextMenu, Entry};
                    use zmax_view::editor::Action;
                    let path = context
                        .editor
                        .document(doc_id)
                        .and_then(|d| d.path().map(|p| p.to_path_buf()));
                    let all: Vec<zmax_view::DocumentId> =
                        context.editor.documents().map(|d| d.id()).collect();
                    let (col, row) = (me.column, me.row);
                    let cb: crate::compositor::Callback = Box::new(move |compositor, _cx| {
                        let mut e = Vec::new();
                        e.push(Entry::item_key("Close", "SPC b d", move |_c, cx| {
                            if cx.editor.close_document(doc_id, false).is_err() {
                                cx.editor
                                    .set_error("unsaved changes (use :bc!)".to_string());
                            }
                        }));
                        let others: Vec<_> = all.iter().copied().filter(|d| *d != doc_id).collect();
                        e.push(Entry::item("Close Others", move |_c, cx| {
                            for d in &others {
                                let _ = cx.editor.close_document(*d, false);
                            }
                        }));
                        let every = all.clone();
                        e.push(Entry::item("Close All", move |_c, cx| {
                            for d in &every {
                                let _ = cx.editor.close_document(*d, false);
                            }
                        }));
                        if let Some(path) = path.clone() {
                            e.push(Entry::sep());
                            let p = path.clone();
                            e.push(Entry::item_key("Split Right", "⇧↵", move |_c, cx| {
                                let _ = cx.editor.open(&p, Action::VerticalSplit);
                            }));
                            let p = path.clone();
                            e.push(Entry::item("Reveal in Tree", move |compositor, _cx| {
                                if let Some(view) = compositor.find::<EditorView>() {
                                    view.reveal_in_tree(&p);
                                }
                            }));
                            let p = path.clone();
                            e.push(Entry::item("Copy Path", move |_c, cx| {
                                let s = p.to_string_lossy().to_string();
                                let _ = cx.editor.registers.push('"', s.clone());
                                cx.editor.set_status(format!("yanked {s}"));
                            }));
                        }
                        compositor.push(Box::new(ContextMenu::new(row, col, e)));
                    });
                    return EventResult::Consumed(Some(cb));
                }
            }
            // Left-click a bufferline tab switches to it; middle-click closes it
            // (the modern-IDE convention). The bufferline is its own row, so this
            // doesn't clash with middle-click paste in the editor body.
            if me.row == self.bufferline_y
                && matches!(
                    me.kind,
                    MouseEventKind::Down(MouseButton::Left)
                        | MouseEventKind::Down(MouseButton::Middle)
                )
            {
                if let Some(&(_, end, close_x, doc_id)) = self
                    .bufferline_tabs
                    .iter()
                    .find(|(a, b, _, _)| me.column >= *a && me.column < *b)
                {
                    // Close on the × button (left-click) or anywhere with middle-click.
                    let on_close = me.column >= close_x && me.column < end;
                    if matches!(me.kind, MouseEventKind::Down(MouseButton::Middle)) || on_close {
                        if context.editor.close_document(doc_id, false).is_err() {
                            context.editor.set_error(
                                "Buffer has unsaved changes (use :bc! to force-close)".to_string(),
                            );
                        }
                    } else {
                        context
                            .editor
                            .switch(doc_id, zmax_view::editor::Action::Replace);
                    }
                    return EventResult::Consumed(None);
                }
                // Left-click the trailing "+" opens a new scratch buffer.
                if matches!(me.kind, MouseEventKind::Down(MouseButton::Left))
                    && me.column >= self.bufferline_new.0
                    && me.column < self.bufferline_new.1
                {
                    context.editor.new_file(zmax_view::editor::Action::Replace);
                    return EventResult::Consumed(None);
                }
            }
            if self
                .ide
                .as_ref()
                .is_some_and(|ide| ide.visible() && ide.hit(me.column, me.row))
            {
                let text = doc!(context.editor).text().clone();
                let action = self.ide.as_mut().unwrap().handle_mouse(me, |line| {
                    text.line_to_char(line.min(text.len_lines().saturating_sub(1)))
                });
                let cb = self.apply_ide_action(action, context);
                return EventResult::Consumed(cb);
            }
            // A left-click in the editor body (outside every IDE panel) hands
            // keyboard focus back to the editor, so keys stop routing to a panel
            // (e.g. after opening a file from the tree, which keeps tree focus).
            if matches!(me.kind, MouseEventKind::Down(MouseButton::Left)) {
                if let Some(ide) = self.ide.as_mut() {
                    if ide.visible() && ide.capturing() {
                        ide.focus_editor();
                    }
                }
            }
        }

        let mut cx = commands::Context {
            editor: context.editor,
            count: None,
            register: None,
            callback: Vec::new(),
            on_next_key_callback: None,
            jobs: context.jobs,
        };

        match event {
            Event::Paste(contents) => {
                self.handle_non_key_input(&mut cx);
                cx.count = cx.editor.count;
                commands::paste_bracketed_value(&mut cx, contents.clone());
                cx.editor.count = None;

                let config = cx.editor.config();
                let mode = cx.editor.mode();
                let (view, doc) = current!(cx.editor);
                view.ensure_cursor_in_view(doc, config.scrolloff);

                // Store a history state if not in insert mode. Otherwise wait till we exit insert
                // to include any edits to the paste in the history state.
                if mode != Mode::Insert {
                    doc.append_changes_to_history(view);
                }

                EventResult::Consumed(None)
            }
            Event::Resize(_width, _height) => {
                // Ignore this event, we handle resizing just before rendering to screen.
                // Handling it here but not re-rendering will cause flashing
                EventResult::Consumed(None)
            }
            Event::Key(mut key) => {
                cx.editor.reset_idle_timer();
                // emacs `modifier-bar-mode`: a modifier latched by clicking the
                // modifier bar is applied to exactly this key, then released.
                let latched = crate::emacs_frame::take_sticky_modifiers();
                if !latched.is_empty() {
                    key.modifiers |= latched;
                }
                canonicalize_key(&mut key);

                // emacs `open-dribble-file`: every key the editor reads goes to
                // the dribble file while one is open.
                commands::dribble_key(&key);

                // spacemacs `+fun/selectric`: the typewriter click, one sound per
                // key the editor reads (a no-op while the mode is off).
                crate::sm_misc::selectric_key(&key);

                // clear status
                cx.editor.status_msg = None;

                let mode = cx.editor.mode();

                // emacs prefix argument (`C-u 3 0`, `M-5`, `M--`): the keys that
                // build the argument are not commands and never reach the keymap.
                // The argument they build is read by the command that follows.
                if self.handle_prefix_key(&mut cx, mode, key) {
                    return EventResult::Consumed(None);
                }

                // Document version before dispatch, so the on_next_key branch
                // below can tell whether the consumed key made a change.
                let dot_pre_version = cx
                    .editor
                    .tree
                    .try_get(cx.editor.tree.focus)
                    .map(|_| doc!(cx.editor).version());
                if self.on_next_key(OnKeyCallbackKind::PseudoPending, &mut cx, key) {
                    // A pending on_next_key callback consumed this key: the object
                    // char of `ci"`/`da(`, the target of `cf<c>`/`ct<c>`, the
                    // replacement of `r<c>`, and so on. These are dispatched here
                    // rather than through the keymap path below, so the dot-repeat
                    // recorder never sees them. Without mirroring the recording
                    // here, text-object (and other on_next_key) operators never
                    // populate `last_change`, so `.` silently does nothing — the
                    // reported `ci".` bug. The keymap prefix (`c`,`i`) is already
                    // in `change_buf`; append the argument key and then, exactly
                    // as the keymap arm does, either arm the insert-session
                    // recorder (`ci"`, `cf x`) or finalize a completed normal-mode
                    // change (`di"`, `da(`, `r x`).
                    if !self.replaying && key != key!('.') {
                        if !self.recording_insert_change {
                            self.change_buf.push(key);
                        }
                        if cx.editor.mode() == Mode::Insert {
                            self.recording_insert_change = true;
                        } else if let Some(pre) = dot_pre_version {
                            let post = cx
                                .editor
                                .tree
                                .try_get(cx.editor.tree.focus)
                                .map(|_| doc!(cx.editor).version());
                            if post.is_some_and(|post| post != pre) {
                                self.last_change = self.change_buf.clone();
                                self.last_change_count = self.change_count;
                            }
                        }
                    }
                } else {
                    match mode {
                        Mode::Insert => {
                            // let completion swallow the event if necessary
                            let mut consumed = false;
                            if let Some(completion) = &mut self.completion {
                                let res = {
                                    // use a fake context here
                                    let mut cx = Context {
                                        editor: cx.editor,
                                        jobs: cx.jobs,
                                        scroll: None,
                                    };

                                    if let EventResult::Consumed(callback) =
                                        completion.handle_event(event, &mut cx)
                                    {
                                        consumed = true;
                                        Some(callback)
                                    } else if let EventResult::Consumed(callback) =
                                        completion.handle_event(&Event::Key(key!(Enter)), &mut cx)
                                    {
                                        Some(callback)
                                    } else {
                                        None
                                    }
                                };

                                if let Some(callback) = res {
                                    if callback.is_some() {
                                        // assume close_fn
                                        if let Some(cb) = self.clear_completion(cx.editor) {
                                            if consumed {
                                                cx.on_next_key_callback =
                                                    Some((cb, OnKeyCallbackKind::Fallback))
                                            } else {
                                                self.on_next_key =
                                                    Some((cb, OnKeyCallbackKind::Fallback));
                                            }
                                        }
                                    }
                                }
                            }

                            // if completion didn't take the event, we pass it onto commands
                            if !consumed {
                                self.insert_mode(&mut cx, key);

                                // record last_insert key
                                self.last_insert.1.push(InsertEvent::Key(key));

                                // vim dot-repeat: keep the insert session as part of
                                // the change being recorded, and finalize once we
                                // leave insert mode (e.g. <Esc>).
                                if self.recording_insert_change && !self.replaying {
                                    self.change_buf.push(key);
                                    if cx.editor.mode() != Mode::Insert {
                                        self.last_change = take(&mut self.change_buf);
                                        self.last_change_count = self.change_count;
                                        self.recording_insert_change = false;
                                    }
                                }
                            }
                        }
                        mode => {
                            // vim dot-repeat: record the keys that make up a change.
                            if !self.replaying && key != key!('.') {
                                let at_boundary = self.keymaps.pending().is_empty()
                                    && !self.recording_insert_change;
                                if at_boundary {
                                    self.change_buf.clear();
                                    // Capture the count now, at the first key of the
                                    // change, before the command consumes and clears
                                    // it — vim reuses it for a later count-less `.`.
                                    self.change_count =
                                        cx.editor.count.map_or(1, NonZeroUsize::get);
                                }
                                if !self.is_count_key(mode, cx.editor.count, key) {
                                    self.change_buf.push(key);
                                }
                            }
                            // `None` when there is no current view (e.g. the editor
                            // started with only a picker open). Resolved fallibly so a
                            // command that closes the last view below can't panic here.
                            let pre_version = cx
                                .editor
                                .tree
                                .try_get(cx.editor.tree.focus)
                                .map(|_| doc!(cx.editor).version());

                            self.command_mode(mode, &mut cx, key);

                            if !self.replaying && key != key!('.') {
                                if cx.editor.mode() == Mode::Insert {
                                    // entered insert (i/a/o/cw/...) — keep recording
                                    // through the insert session.
                                    self.recording_insert_change = true;
                                } else if let Some(pre) = pre_version {
                                    // The command may have closed the view (`:q`, ZZ, a
                                    // misparsed terminal sequence, …); only inspect the
                                    // post-state document when one still exists.
                                    let post_version = cx
                                        .editor
                                        .tree
                                        .try_get(cx.editor.tree.focus)
                                        .map(|_| doc!(cx.editor).version());
                                    if post_version.is_some_and(|post| post != pre) {
                                        // a normal/select-mode change (dd, x, p, J, >>, ...)
                                        self.last_change = self.change_buf.clone();
                                        self.last_change_count = self.change_count;
                                    }
                                }
                            }
                        }
                    }
                }

                self.on_next_key = cx.on_next_key_callback.take();
                match self.on_next_key {
                    Some((_, OnKeyCallbackKind::PseudoPending)) => self.pseudo_pending.push(key),
                    _ => self.pseudo_pending.clear(),
                }

                // appease borrowck
                let callbacks = take(&mut cx.callback);

                // if the command consumed the last view, skip the render.
                // on the next loop cycle the Application will then terminate.
                if cx.editor.should_close() {
                    return EventResult::Ignored(None);
                }

                let config = cx.editor.config();
                let mode = cx.editor.mode();
                let (view, doc) = current!(cx.editor);

                view.ensure_cursor_in_view(doc, config.scrolloff);

                // Store a history state if not in insert mode. This also takes care of
                // committing changes when leaving insert mode.
                if mode != Mode::Insert {
                    doc.append_changes_to_history(view);
                }
                let callback = if callbacks.is_empty() {
                    None
                } else {
                    let callback: crate::compositor::Callback = Box::new(move |compositor, cx| {
                        for callback in callbacks {
                            callback(compositor, cx)
                        }
                    });
                    Some(callback)
                };

                EventResult::Consumed(callback)
            }

            Event::Mouse(event) => {
                let result = self.handle_mouse_event(event, &mut cx);
                // A command dispatched from the mouse can queue compositor
                // callbacks (a popup menu, a picker). The key path drains
                // `Context::callback` above; the mouse path returns its result
                // straight out, so it drains here — without this the queued layer
                // is dropped and the menu never appears.
                let queued = take(&mut cx.callback);
                if queued.is_empty() {
                    return result;
                }
                let (consumed, existing) = match result {
                    EventResult::Consumed(cb) => (true, cb),
                    EventResult::Ignored(cb) => (false, cb),
                };
                let chained: crate::compositor::Callback =
                    Box::new(move |compositor, cx: &mut Context| {
                        if let Some(cb) = existing {
                            cb(compositor, cx);
                        }
                        for callback in queued {
                            callback(compositor, cx);
                        }
                    });
                if consumed {
                    EventResult::Consumed(Some(chained))
                } else {
                    EventResult::Ignored(Some(chained))
                }
            }
            Event::IdleTimeout => self.handle_idle_timeout(&mut cx),
            Event::FocusGained => {
                self.terminal_focused = true;
                EventResult::Consumed(None)
            }
            Event::FocusLost => {
                if context.editor.config().auto_save.focus_lost {
                    let options = commands::WriteAllOptions {
                        force: false,
                        write_scratch: false,
                        auto_format: false,
                        code_actions: false,
                    };
                    if let Err(e) = commands::typed::write_all_impl(context, options) {
                        context.editor.set_error(format!("{}", e));
                    }
                }
                self.terminal_focused = false;
                EventResult::Consumed(None)
            }
        }
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, cx: &mut Context) {
        // Emacs `appt-check`: with appointment checking on, a reminder that has
        // come due is delivered to the echo area. The redraw is the tick (the
        // poller `appt-activate` starts asks for one every 30 seconds), so this is
        // where an appointment actually reaches the user.
        if let Some(msg) = crate::commands::appt_due_message() {
            cx.editor.set_status(msg);
        }
        // emacs `reveal-mode`: hidden text opens up while point is inside it and
        // closes again when point leaves. Redisplay is where "where is point now"
        // is answered, which is why emacs runs it from `post-command-hook` and we
        // run it here.
        Self::apply_reveal_mode(cx.editor);
        // IDE file-tree sidebar reserves a left strip; the editor uses what remains.
        let area = self.render_sidebar(area, surface, cx);
        let config = cx.editor.config();
        // clear with background color; when `transparent-background` is set, drop
        // the fill's bg so cells keep `Color::Reset` and the terminal background
        // shows through.
        let mut bg_style = cx.editor.theme.get("ui.background");
        if config.transparent_background {
            bg_style.bg = None;
        }
        surface.set_style(area, bg_style);

        // check if bufferline should be rendered
        use zmax_view::editor::BufferLine;
        let use_bufferline = match cx.editor.frame_tab_bar() {
            // emacs `tab-bar-lines`: this frame has its own answer
            // (`toggle-frame-tab-bar`), which wins over the global mode. The IDE
            // workbench still forces the row it reserves for its own tabs.
            Some(on) => on || self.ide.as_ref().is_some_and(Ide::visible),
            None => match config.bufferline {
                BufferLine::Always => true,
                BufferLine::Multiple if cx.editor.documents.len() > 1 => true,
                // Always show the top tab bar while the IDE workbench is open.
                _ => self.ide.as_ref().is_some_and(Ide::visible),
            },
        };

        // The IDE workbench reserves a dedicated row for the open-file tabs above
        // its button toolbar; outside IDE mode the bufferline sits at the top of
        // the editor area (and must be clipped out of it).
        let ide_visible = self.ide.as_ref().is_some_and(Ide::visible);
        let ide_bufrow = self
            .ide
            .as_ref()
            .map(Ide::bufferline_rect)
            .filter(|r| r.height > 0);
        let draw_bufferline = use_bufferline && (!ide_visible || ide_bufrow.is_some());

        // vim `cmdheight`: rows the command line keeps at the bottom (1 by
        // default). The bufferline takes one more off the top when it lives inside
        // `area`.
        let cmdheight = cmdheight();
        let mut editor_area = area.clip_bottom(cmdheight);
        // emacs frame furniture above the windows: the menu bar, then the tool
        // bar, then the modifier bar — emacs's own top-to-bottom order — each one
        // row, taken off the top before the tab bar.
        let frame_bars = area.with_height(crate::emacs_frame::frame_bar_rows());
        editor_area = editor_area.clip_top(frame_bars.height);
        if draw_bufferline && ide_bufrow.is_none() {
            editor_area = editor_area.clip_top(1);
        }

        // The vim-airline powerline bar spans the whole frame above the command
        // line. Inside the workbench the IDE owns that row (it carves it before
        // laying out its panels, in `Ide::render`); outside it, carve it here so
        // the bar is there in both modes. Skipped on a frame too short to give a
        // row up, matching the IDE's own height guard.
        // `[editor.statusline] powerline = false` turns the bar off and hands the
        // status row back to each window (the classic per-window status line).
        let powerline_row = if config.statusline.powerline && !ide_visible && editor_area.height > 2
        {
            let row = editor_area.clip_top(editor_area.height.saturating_sub(1));
            editor_area = editor_area.clip_bottom(1);
            Some(row)
        } else {
            None
        };

        // The bar carries what the per-window status line carried, so the windows
        // above it hand their status row back to the text instead of drawing the
        // same thing twice. Set before the resize below: window geometry
        // (`View::inner_area`/`inner_height`) reads it.
        let powerline_drawn = if ide_visible {
            self.ide.as_ref().is_some_and(Ide::statusbar_drawn)
        } else {
            powerline_row.is_some()
        };
        zmax_view::view::set_window_status_line(!powerline_drawn);

        // if the terminal size suddenly changed, we need to trigger a resize
        cx.editor.resize(editor_area);

        self.render_frame_bars(frame_bars, surface, cx);

        if draw_bufferline {
            let bar = ide_bufrow.unwrap_or_else(|| area.clip_top(frame_bars.height).with_height(1));
            // vim `tabline`: a format string replaces the tab bar's contents.
            match crate::commands::typed::vim_opt_str("tabline") {
                Some(fmt) => {
                    let style = cx.editor.theme.get("ui.statusline");
                    let (view, doc) = current_ref!(cx.editor);
                    Self::render_vim_bar(&fmt, doc, view, bar, surface, style);
                    self.bufferline_tabs.clear();
                    self.bufferline_new = (0, 0);
                }
                None => {
                    let (tabs, new_btn) = Self::render_bufferline(cx.editor, bar, surface);
                    self.bufferline_tabs = tabs;
                    self.bufferline_new = new_btn;
                }
            }
            self.bufferline_y = bar.y;
        } else {
            self.bufferline_tabs.clear();
            self.bufferline_new = (0, 0);
        }

        // vim `concealcursor`: the cursor line's concealed text stays concealed
        // only in the modes the option lists (`nvic`); in any other mode it is
        // revealed. The decision needs the editor's mode, which the view layer
        // (where the conceal overlays are applied) cannot see, so it is resolved
        // here, once per frame.
        let conceal_modes =
            crate::commands::typed::vim_opt_str("concealcursor").unwrap_or_default();
        let mode_letter = match cx.editor.mode() {
            Mode::Insert => 'i',
            Mode::Select => 'v',
            Mode::Normal => 'n',
        };
        zmax_view::view::set_conceal_reveal_cursor_line(!conceal_modes.contains(mode_letter));

        for (view, is_focused) in cx.editor.tree.views() {
            let doc = cx.editor.document(view.doc).unwrap();
            self.render_view(cx.editor, doc, view, area, surface, is_focused);
        }

        // The powerline status bar, drawn after the views so the row it owns is
        // never painted over by a window that grew into it mid-frame.
        if let Some(row) = powerline_row {
            if let Some(status) = crate::ui::powerline::snapshot(cx.editor) {
                crate::ui::powerline::render(surface, &cx.editor.theme, row, &status);
            }
        }

        // Overlay the IDE LSP/build progress card on top of the document.
        if let Some(ide) = self.ide.as_ref() {
            ide.render_progress_overlay(area, surface, &cx.editor.theme);
        }

        if config.auto_info {
            if let Some(mut info) = cx.editor.autoinfo.take() {
                info.render(area, surface, cx);
                cx.editor.autoinfo = Some(info)
            }
        }

        let key_width = 15u16; // for showing pending keys
        let mut status_msg_width = 0;

        // The command line: the last `cmdheight` rows of the screen (`cmdheight=0`
        // reserves none, so a message draws over the editor's last row).
        let cmd_top = area
            .y
            .saturating_add(area.height.saturating_sub(cmdheight.max(1)));
        let cmd_bottom = area.y + area.height.saturating_sub(1);

        // render status msg
        if let Some((status_msg, severity)) = &cx.editor.status_msg {
            status_msg_width = status_msg.width();
            use zmax_view::editor::Severity;
            let style = if *severity == Severity::Error {
                cx.editor.theme.get("error")
            } else {
                cx.editor.theme.get("ui.text")
            };

            // vim `cmdheight`: a message longer than the screen is wrapped over the
            // rows the command line was given rather than being cut off at one.
            let width = area.width as usize;
            let rows = cmdheight.max(1) as usize;
            for (row, chunk) in wrap_message(status_msg, width, rows).iter().enumerate() {
                surface.set_string(area.x, cmd_top + row as u16, chunk, style);
            }
        }

        // vim `showcmd`: the partial command (count + pending keys) shown at the
        // bottom right. `:set noshowcmd` hides it.
        if crate::commands::vim_opt_bool("showcmd")
            && area.width.saturating_sub(status_msg_width as u16) > key_width
        {
            let mut disp = String::new();
            if let Some(count) = cx.editor.count {
                disp.push_str(&count.to_string())
            }
            for key in self.keymaps.pending() {
                disp.push_str(&key.key_sequence_format());
            }
            for key in &self.pseudo_pending {
                disp.push_str(&key.key_sequence_format());
            }
            let style = cx.editor.theme.get("ui.text");
            let macro_width = if cx.editor.macro_recording.is_some() {
                3
            } else {
                0
            };
            let restricted = workspace_trust_indicator_visible(cx.editor);
            let trust_width = if restricted { 3 } else { 0 };
            // vim `showcmdloc`: `last` (the command line), `statusline` (the
            // focused window's status row) or `tabline` (the top bar).
            let loc = crate::commands::typed::vim_opt_str("showcmdloc")
                .unwrap_or_else(|| "last".to_string());
            let focused = cx.editor.tree.get(cx.editor.tree.focus).area;
            let row = showcmd_row(
                &loc,
                cmd_bottom,
                focused.y + focused.height.saturating_sub(1),
                area.y,
            );
            surface.set_string(
                area.x
                    + area
                        .width
                        .saturating_sub(key_width + macro_width + trust_width),
                row,
                disp.get(disp.len().saturating_sub(key_width as usize)..)
                    .unwrap_or(&disp),
                style,
            );
            if restricted {
                let style = style
                    .fg(zmax_view::graphics::Color::Yellow)
                    .add_modifier(Modifier::BOLD);
                surface.set_string(
                    area.x
                        .saturating_add(area.width.saturating_sub(3 + macro_width)),
                    cmd_bottom,
                    "[⚠]",
                    style,
                );
            }
            if let Some((reg, _)) = cx.editor.macro_recording {
                let disp = format!("[{}]", reg);
                let style = style
                    .fg(zmax_view::graphics::Color::Yellow)
                    .add_modifier(Modifier::BOLD);
                surface.set_string(
                    area.x + area.width.saturating_sub(3),
                    cmd_bottom,
                    &disp,
                    style,
                );
            }
        }

        if let Some(completion) = self.completion.as_mut() {
            completion.render(area, surface, cx);
        }
    }

    fn cursor(&self, _area: Rect, editor: &Editor) -> (Option<Position>, CursorKind) {
        if self.ide.as_ref().is_some_and(Ide::capturing) {
            return (None, CursorKind::Hidden);
        }
        match editor.cursor() {
            // all block cursors are drawn manually
            (pos, CursorKind::Block) => {
                if self.terminal_focused {
                    (pos, CursorKind::Hidden)
                } else {
                    // use terminal cursor when terminal loses focus
                    (pos, CursorKind::Underline)
                }
            }
            cursor => cursor,
        }
    }
}

fn canonicalize_key(key: &mut KeyEvent) {
    if let KeyEvent {
        code: KeyCode::Char(_),
        modifiers: _,
    } = key
    {
        key.modifiers.remove(KeyModifiers::SHIFT)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fillchars_names_one_character_each() {
        let value = "vert:┃,eob:~,fold:-";
        assert_eq!(parse_fillchar(value, "vert"), Some('┃'));
        assert_eq!(parse_fillchar(value, "eob"), Some('~'));
        assert_eq!(parse_fillchar(value, "fold"), Some('-'));
        // A position the option doesn't name keeps zmax's own character.
        assert_eq!(parse_fillchar(value, "stl"), None);
        // A prefix of a name must not match it (`eo` is not `eob`).
        assert_eq!(parse_fillchar(value, "eo"), None);
        assert_eq!(parse_fillchar("", "vert"), None);
    }

    /// The tabline can only draw the buffers that fit, so the window it shows has
    /// to contain the current one — otherwise opening the tenth buffer leaves the
    /// bar pointing at the first nine and no way to see where you are.
    #[test]
    fn bufferline_scrolls_to_keep_the_current_buffer_visible() {
        let widths = [10u16; 8];
        // Everything fits: no scroll, whatever is current.
        assert_eq!(EditorView::bufferline_scroll(&widths, Some(7), 80), 0);
        // Room for three pills: the current one is the last shown, with as many
        // of its predecessors as still fit.
        assert_eq!(EditorView::bufferline_scroll(&widths, Some(7), 30), 5);
        assert_eq!(EditorView::bufferline_scroll(&widths, Some(2), 30), 0);
        // A budget too small even for one pill still starts at the current one,
        // so the row degrades to "nothing fits" rather than showing the wrong end.
        assert_eq!(EditorView::bufferline_scroll(&widths, Some(4), 5), 4);
        // No current buffer (no open document) — start at the beginning.
        assert_eq!(EditorView::bufferline_scroll(&widths, None, 30), 0);
        // Uneven pills: a long name pushes fewer neighbours into view.
        let mixed = [8u16, 40, 8, 8];
        assert_eq!(EditorView::bufferline_scroll(&mixed, Some(3), 30), 2);
    }

    #[test]
    fn showcmdloc_picks_the_row() {
        // cmdline row 40, statusline row 30, tabline row 0.
        assert_eq!(showcmd_row("last", 40, 30, 0), 40);
        assert_eq!(showcmd_row("statusline", 40, 30, 0), 30);
        assert_eq!(showcmd_row("tabline", 40, 30, 0), 0);
        // vim's default, and anything unknown, is the command line.
        assert_eq!(showcmd_row("", 40, 30, 0), 40);
    }

    fn bar_cx() -> BarContext {
        BarContext {
            path: "/src/main.rs".into(),
            name: "main.rs".into(),
            modified: true,
            readonly: false,
            filetype: "rust".into(),
            line: 12,
            lines: 400,
            col: 7,
        }
    }

    #[test]
    fn vim_bar_expands_the_items_it_supports() {
        let cx = bar_cx();
        let (left, right) = vim_bar_expand("%f%m%=%l/%L", &cx);
        assert_eq!(left, "/src/main.rs[+]");
        assert_eq!(right, "12/400");
        // %t is the file name, %y the bracketed filetype, %c the column.
        let (left, right) = vim_bar_expand("%t %y%=col %c", &cx);
        assert_eq!(left, "main.rs [rust]");
        assert_eq!(right, "col 7");
        // %% is a literal percent; a width prefix is accepted and not padded by.
        assert_eq!(vim_bar_expand("%%%-10t", &cx).0, "%main.rs");
        // A vimscript expression cannot be evaluated here, so it renders nothing
        // rather than showing its source.
        assert_eq!(vim_bar_expand("a%{strftime('%c')}b", &cx).0, "ab");
    }

    #[test]
    fn vim_bar_reports_readonly_and_clean_buffers() {
        let mut cx = bar_cx();
        cx.modified = false;
        cx.readonly = true;
        assert_eq!(vim_bar_expand("%m%r", &cx).0, "[RO]");
    }

    #[test]
    fn foldtext_is_literal_or_vims_default() {
        // A literal value is what the fold line shows.
        assert_eq!(
            fold_text("-- folded --", 9, 1, "fn main() {"),
            "-- folded --"
        );
        // A function call has no evaluator here, so the fold gets vim's default
        // `foldtext()`: `+-`, `level` dashes, the count padded to width 3, then
        // the already-cleaned first line.
        assert_eq!(
            fold_text("foldtext()", 12, 1, "fn main() {"),
            "+-- 12 lines: fn main() {"
        );
        // Count < 100 is right-justified to 3 columns (vim `%3ld`).
        assert_eq!(fold_text("", 3, 1, "struct S;"), "+--  3 lines: struct S;");
        // A nested (level 2) fold gets an extra dash.
        assert_eq!(
            fold_text("", 8, 2, "MARK:Header"),
            "+---  8 lines: MARK:Header"
        );
    }

    #[test]
    fn clean_fold_line_strips_comments_and_markers() {
        let hash = vec!["#".to_string()];
        // `#{{{` comment + fold marker + padding collapses to the label.
        assert_eq!(
            clean_fold_line("#{{{                MARK:Header", &hash),
            "MARK:Header"
        );
        // Marker before the comment leader, with a level digit, also strips.
        assert_eq!(clean_fold_line("{{{1 # section", &hash), "section");
        // A plain line is only trimmed.
        assert_eq!(clean_fold_line("  fn main() {", &[]), "fn main() {");
    }

    #[test]
    fn camel_splits_a_word_at_its_capitals() {
        let word: Vec<char> = "fooBarBaz".chars().collect();
        assert_eq!(
            camel_parts(&word),
            vec![
                (0, "foo".to_string()),
                (3, "Bar".to_string()),
                (6, "Baz".to_string())
            ]
        );
        // An all-lowercase word is one part, and the offsets are word-relative.
        let word: Vec<char> = "plain".chars().collect();
        assert_eq!(camel_parts(&word), vec![(0, "plain".to_string())]);
        // A run of capitals does not split (`HTTPServer` -> `HTTPServer`).
        let word: Vec<char> = "HTTPServer".chars().collect();
        assert_eq!(camel_parts(&word), vec![(0, "HTTPServer".to_string())]);
    }

    #[test]
    fn spellcapcheck_reads_its_sentence_end_characters() {
        // vim's default pattern.
        assert_eq!(
            spellcap_end_chars(r#"[.?!]\_[\])'" \t]\+"#),
            vec!['.', '?', '!']
        );
        // An empty option turns the check off.
        assert!(spellcap_end_chars("").is_empty());
        // A pattern that is not a character class: its punctuation ends sentences.
        assert_eq!(spellcap_end_chars(r"\.\s"), vec!['.']);
    }

    #[test]
    fn cmdheight_wraps_a_long_message_over_its_rows() {
        // One row (the default): the message is cut off exactly as before.
        assert_eq!(wrap_message("abcdefgh", 4, 1), vec!["abcd"]);
        // Three rows: it wraps instead of being lost.
        assert_eq!(wrap_message("abcdefgh", 4, 3), vec!["abcd", "efgh"]);
        // A message longer than the whole command area still stops at its rows.
        assert_eq!(wrap_message("abcdefghij", 4, 2), vec!["abcd", "efgh"]);
        // A message that fits stays on one row.
        assert_eq!(wrap_message("ab", 4, 2), vec!["ab"]);
        assert!(wrap_message("ab", 0, 2).is_empty());
    }

    #[test]
    fn redrawtime_only_stops_a_pass_that_has_a_budget() {
        // Unset: no budget, so a pass never gives up.
        assert!(!over_redrawtime(10_000, None));
        // Set: over budget stops, under budget does not.
        assert!(over_redrawtime(2_001, Some(2_000)));
        assert!(!over_redrawtime(2_000, Some(2_000)));
    }
}
