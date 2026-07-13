use crate::{
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
use zemacs_core::{
    diagnostic::NumberOrString,
    graphemes::{next_grapheme_boundary, prev_grapheme_boundary},
    movement::Direction,
    syntax::{self, OverlayHighlights},
    text_annotations::TextAnnotations,
    unicode::width::UnicodeWidthStr,
    visual_offset_from_block, Change, Position, Range, Selection,
};
use zemacs_view::{
    annotations::diagnostics::DiagnosticFilter,
    document::{Mode, SCRATCH_BUFFER_NAME},
    editor::{CompleteAction, CursorShapeConfig},
    graphics::{Color, CursorKind, Modifier, Rect, Style},
    input::{KeyEvent, MouseButton, MouseEvent, MouseEventKind},
    keyboard::{KeyCode, KeyModifiers},
    Document, Editor, Theme, View,
};

use tui::{buffer::Buffer as Surface, text::Span};

/// Bufferline tab hit regions: `(x_start, x_end, close_x, doc)` per tab.
type BufferlineTabs = Vec<(u16, u16, u16, zemacs_view::DocumentId)>;

/// Sticky-scroll cache: `(doc, doc len, scopes)` where each scope is
/// `(start_line, end_line, header_text)`.
type StickyCache =
    std::cell::RefCell<Option<(zemacs_view::DocumentId, usize, Vec<(usize, usize, String)>)>>;

pub struct EditorView {
    pub keymaps: Keymaps,
    on_next_key: Option<(OnKeyCallback, OnKeyCallbackKind)>,
    pseudo_pending: Vec<KeyEvent>,
    /// Ring of the most recently pressed keys, for `view-lossage` (C-h l).
    pub recent_keys: std::collections::VecDeque<KeyEvent>,
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
    resize_drag: Option<(zemacs_view::ViewId, bool, i16)>,
    /// Sticky-scroll cache: `(doc, doc len, scopes)` where each scope is
    /// `(start_line, end_line, header_text)`. Recomputed only when the focused
    /// document's length changes, so scrolling stays cheap.
    sticky_cache: StickyCache,
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

impl EditorView {
    pub fn new(keymaps: Keymaps) -> Self {
        Self {
            keymaps,
            on_next_key: None,
            pseudo_pending: Vec::new(),
            recent_keys: std::collections::VecDeque::new(),
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

    /// Boot the IDE workbench, editor focused (the `zemacs --ide` entry point).
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
        let root = zemacs_loader::find_workspace().0;
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
        let shell = context.editor.config().shell.clone();
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
                if let Err(zemacs_view::DocumentOpenError::BinaryFile) = context
                    .editor
                    .open(&path, zemacs_view::editor::Action::Replace)
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
                    .open(&path, zemacs_view::editor::Action::Replace)
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
                    let tx = zemacs_core::Transaction::insert(doc.text(), &sel, text.into());
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
                    .open(&path, zemacs_view::editor::Action::Replace)
                    .is_ok()
                {
                    if let Some(cmd) = crate::commands::typed::TYPABLE_COMMAND_MAP.get("merge") {
                        let _ = (cmd.fun)(
                            context,
                            zemacs_core::command_line::Args::default(),
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

        if is_focused && config.cursorline {
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

        // Emacs Hi-Lock: persistent user regexp highlights (all windows).
        overlays.extend(Self::doc_hilock_highlights(doc, view, theme));

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

        // if we're not at the edge of the screen, draw a right border
        if viewport.right() != view.area.right() {
            let x = area.right();
            let border_style = theme.get("ui.window");
            for y in area.top()..area.bottom() {
                surface[(x, y)]
                    .set_symbol(tui::symbols::line::VERTICAL)
                    //.set_symbol(" ")
                    .set_style(border_style);
            }
        }

        if config.inline_diagnostics.disabled()
            && config.end_of_line_diagnostics == DiagnosticFilter::Disable
        {
            Self::render_diagnostics(doc, view, inner, surface, theme);
        }

        // vim `laststatus=0`: skip the per-window status line entirely.
        if config.render_statusline {
            let statusline_area = view
                .area
                .clip_top(view.area.height.saturating_sub(1))
                .clip_bottom(1); // -1 from bottom to remove commandline

            let mut context =
                statusline::RenderContext::new(editor, doc, view, is_focused, &self.spinners);

            statusline::render(&mut context, statusline_area, surface);
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
        text: zemacs_core::RopeSlice,
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

        use zemacs_core::diagnostic::{DiagnosticTag, Range, Severity};
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
        if crate::hi_lock::is_empty() {
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

    /// vim `spell`: underline misspelled words in the visible viewport when
    /// `:set spell` is active. Uses the existing spell engine (`crate::spell`);
    /// scans only the visible line range so it stays cheap on large files.
    pub fn doc_spell_highlights(
        doc: &Document,
        view: &View,
        theme: &Theme,
    ) -> Option<OverlayHighlights> {
        if !crate::commands::vim_opt_bool("spell") {
            return None;
        }
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
                let word: String = haystack[start..end].iter().collect();
                if word.chars().count() >= 2 && crate::spell::is_misspelled(&word) {
                    ranges.push((scan_start + start)..(scan_start + end));
                }
            } else {
                i += 1;
            }
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
        use zemacs_stdx::rope::{Config, RegexBuilder, RopeSliceExt};
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
        let is_crlf = doc.line_ending == zemacs_core::LineEnding::Crlf;
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

        let mut ranges = Vec::new();
        for m in regex.find_iter(text.regex_input_at_bytes(start_byte..end_byte)) {
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
        use zemacs_core::chars::char_is_word;

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
                spans.push((selection_scope, range.anchor..selection_end));
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
                spans.push((selection_scope, selection_start..range.anchor));
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
        // Highlight matching braces
        let syntax = doc.syntax()?;
        let highlight = theme.find_highlight_exact("ui.cursor.match")?;
        let text = doc.text().slice(..);
        let pos = doc.selection(view.id).primary().cursor(text);
        let pos = zemacs_core::match_brackets::find_matching_bracket(syntax, text, pos)?;
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
    pub fn render_bufferline(
        editor: &Editor,
        viewport: Rect,
        surface: &mut Surface,
    ) -> (BufferlineTabs, (u16, u16)) {
        let scratch = PathBuf::from(SCRATCH_BUFFER_NAME); // default filename to use for scratch buffer
        surface.clear_with(
            viewport,
            editor
                .theme
                .try_get("ui.bufferline.background")
                .unwrap_or_else(|| editor.theme.get("ui.statusline")),
        );

        let bufferline_active = editor
            .theme
            .try_get("ui.bufferline.active")
            .unwrap_or_else(|| editor.theme.get("ui.statusline.active"));

        let bufferline_inactive = editor
            .theme
            .try_get("ui.bufferline")
            .unwrap_or_else(|| editor.theme.get("ui.statusline.inactive"));

        let mut x = viewport.x;
        let mut tabs = Vec::new();
        let current_doc = view!(editor).doc;

        for doc in editor.documents() {
            let fname = doc
                .path()
                .unwrap_or(&scratch)
                .file_name()
                .unwrap_or_default()
                .to_str()
                .unwrap_or_default();

            let style = if current_doc == doc.id() {
                bufferline_active
            } else {
                bufferline_inactive
            };

            let glyph = super::icons::file_icon(fname);
            let text = format!(
                " {} {}{} ",
                glyph,
                fname,
                if doc.is_modified() { "[+]" } else { "" }
            );
            let used_width = viewport.x.saturating_sub(x);
            let rem_width = surface.area.width.saturating_sub(used_width);

            let start = x;
            // tab label
            let after = surface
                .set_stringn(x, viewport.y, &text, rem_width as usize, style)
                .0;
            // clickable close button
            let close_x = after;
            let rem2 = (surface.area.right()).saturating_sub(close_x) as usize;
            x = surface
                .set_stringn(close_x, viewport.y, "× ", rem2, style)
                .0;
            tabs.push((start, x, close_x, doc.id()));

            if x >= surface.area.right() {
                break;
            }
        }
        // trailing "+" new-buffer button
        let new_start = x;
        let new_style = editor
            .theme
            .try_get("ui.bufferline")
            .unwrap_or_else(|| editor.theme.get("ui.statusline.inactive"));
        let rem = surface.area.right().saturating_sub(x) as usize;
        x = surface.set_stringn(x, viewport.y, " + ", rem, new_style).0;
        (tabs, (new_start, x))
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

        let gutter_style = theme.get("ui.gutter");
        let gutter_selected_style = theme.get("ui.gutter.selected");
        let gutter_style_virtual = theme.get("ui.gutter.virtual");
        let gutter_selected_style_virtual = theme.get("ui.gutter.selected.virtual");

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
        use zemacs_core::diagnostic::Severity;

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
        let mut last_mode = mode;
        // While a which-key popup is up, PgDn/PgUp scroll/page it (large prefix
        // maps overflow) instead of being treated as bindings. Preserves the
        // pending key sequence — we don't consult the keymap or regenerate the
        // popup, just nudge its scroll offset (the renderer clamps it).
        if let Some(info) = cxt.editor.autoinfo.as_mut() {
            const PAGE: u16 = 8;
            match event.code {
                zemacs_view::input::KeyCode::PageDown => {
                    info.scroll = info.scroll.saturating_add(PAGE);
                    return None;
                }
                zemacs_view::input::KeyCode::PageUp => {
                    info.scroll = info.scroll.saturating_sub(PAGE);
                    return None;
                }
                _ => {}
            }
        }
        // Record the key for `view-lossage` (C-h l), keeping a bounded ring.
        self.recent_keys.push_back(event);
        if self.recent_keys.len() > 100 {
            self.recent_keys.pop_front();
        }
        self.pseudo_pending.extend(self.keymaps.pending());
        let key_result = self.keymaps.get(mode, event);
        cxt.editor.autoinfo = if cxt.editor.config().which_key {
            self.keymaps.sticky().map(|node| node.infobox())
        } else {
            None
        };

        // vim i_CTRL-O one-shot: if the flag was already armed when this event
        // began, the command about to run is the "one command" that should be
        // followed by a return to Insert. Reading it before the executing closure
        // borrows `cxt` excludes the arming CTRL-O press itself (which arms the
        // flag mid-execution).
        let oneshot_armed = cxt.editor.insert_oneshot;

        let mut execute_command = |command: &commands::MappableCommand| {
            command.execute(cxt);
            zemacs_event::dispatch(PostCommand { command, cx: cxt });
            // Follow-mode (SPC w f): keep sibling windows scrolled in lockstep.
            cxt.editor.sync_follow_windows();

            let current_mode = cxt.editor.mode();
            if current_mode != last_mode {
                zemacs_event::dispatch(OnModeSwitch {
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
                        Some("space" | "C-x" | "C-c" | "C-h")
                    )
                };
                cxt.editor.autoinfo = show.then(|| node.infobox());
            }
            KeymapResult::MatchedSequence(commands) => {
                for command in commands {
                    execute_command(command);
                }
            }
            KeymapResult::NotFound | KeymapResult::Cancelled(_) => return Some(key_result),
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
        if let Some(keyresult) = self.handle_keymap_event(Mode::Insert, cx, event) {
            match keyresult {
                KeymapResult::NotFound => {
                    if !self.on_next_key(OnKeyCallbackKind::Fallback, cx, event) {
                        if let Some(ch) = event.char() {
                            commands::insert::insert_char(cx, ch)
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
                                if let KeymapResult::Matched(command) =
                                    self.keymaps.get(Mode::Insert, ev)
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
        self.replaying = true;
        for _ in 0..count {
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

                // set the register
                cxt.register = cxt.editor.selected_register.take();

                let res = self.handle_keymap_event(mode, cxt, event);
                if matches!(&res, Some(KeymapResult::NotFound)) {
                    self.on_next_key(OnKeyCallbackKind::Fallback, cxt, event);
                }
                if self.keymaps.pending().is_empty() {
                    cxt.editor.count = None;
                    self.operator_count = None;
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
                let (view, doc) = zemacs_view::current_ref!(cx.editor);
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

        // Blame annotate gutter: the gutter renderer lives in zemacs_view and
        // can't shell out to git, so compute+push the focused file's blame here
        // the first time it's shown (cheap no-op once cached).
        if crate::blame::annotate_enabled() {
            let path = {
                let (_, doc) = zemacs_view::current_ref!(cx.editor);
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

/// The JetBrains in-editor context menu (right-click on editor text). Actions map
/// to real zemacs commands; the Run/Debug + Open In/Git/Gist groups appear only
/// for a file backed by a path.
fn editor_menu_entries(path: Option<std::path::PathBuf>) -> Vec<crate::ui::context_menu::Entry> {
    use crate::commands::MappableCommand as MC;
    use crate::ui::context_menu::Entry;

    let mut e = vec![
        Entry::item_key("Show Context Actions", "⌥↵", |co, cx| {
            run_editor_command(co, cx, |c| {
                MC::code_action.execute(c);
            })
        }),
        Entry::sep(),
        Entry::item_key("Paste", "p", |co, cx| {
            run_editor_command(co, cx, |c| {
                MC::paste_clipboard_after.execute(c);
            })
        }),
        Entry::item("Copy Reference", |_co, cx| {
            let (view, doc) = zemacs_view::current_ref!(cx.editor);
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
            let root = zemacs_loader::find_workspace().0;
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

fn workspace_trust_indicator_visible(editor: &Editor) -> bool {
    if editor.workspace_trust.implicit_level()
        == zemacs_loader::workspace_trust::ImplicitTrustLevel::Insecure
    {
        return false;
    }
    let (_, doc) = zemacs_view::current_ref!(editor);
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

        let gutter_coords_and_view = |editor: &Editor, row, column| {
            editor.tree.views().find_map(|(view, _focus)| {
                view.gutter_coords_at_screen_coords(row, column)
                    .map(|coords| (coords, view.id))
            })
        };

        match kind {
            MouseEventKind::Down(MouseButton::Left) => {
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
                    self.resize_drag = Some((view_id, vertical, offset));
                    return EventResult::Consumed(None);
                }

                if let Some((pos, view_id)) = pos_and_view(editor, row, column, true) {
                    editor.focus(view_id);

                    let prev_view_id = view!(editor).id;
                    let doc = doc_mut!(editor, &view!(editor, view_id).doc);

                    if modifiers == KeyModifiers::ALT {
                        let selection = doc.selection(view_id).clone();
                        doc.set_selection(view_id, selection.push(Range::point(pos)));
                    } else if editor.mode == Mode::Select {
                        // Discards non-primary selections for consistent UX with normal mode
                        let primary = doc.selection(view_id).primary().put_cursor(
                            doc.text().slice(..),
                            pos,
                            true,
                        );
                        editor.mouse_down_range = Some(primary);
                        doc.set_selection(view_id, Selection::single(primary.anchor, primary.head));
                    } else {
                        doc.set_selection(view_id, Selection::point(pos));
                    }

                    if view_id != prev_view_id {
                        self.clear_completion(editor);
                    }

                    editor.ensure_cursor_in_view(view_id);

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

                let mut selection = doc.selection(view.id).clone();
                let primary = selection.primary_mut();
                *primary = primary.put_cursor(doc.text().slice(..), pos, true);
                let non_empty = primary.anchor != primary.head;
                doc.set_selection(view.id, selection);
                let view_id = view.id;
                // vim `mouse=a`: dragging a selection enters Visual (Select) mode so
                // operators — U, u, gu/gU, d, y, >, … — act on the dragged range;
                // collapsing back to a caret returns to Normal.
                if non_empty && cxt.editor.mode != Mode::Select {
                    cxt.editor.mode = Mode::Select;
                } else if !non_empty && cxt.editor.mode == Mode::Select {
                    cxt.editor.mode = Mode::Normal;
                }
                cxt.editor.ensure_cursor_in_view(view_id);
                EventResult::Consumed(None)
            }

            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                let current_view = cxt.editor.tree.focus;

                let direction = match event.kind {
                    MouseEventKind::ScrollUp => Direction::Backward,
                    MouseEventKind::ScrollDown => Direction::Forward,
                    _ => unreachable!(),
                };

                match pos_and_view(cxt.editor, row, column, false) {
                    Some((_, view_id)) => cxt.editor.tree.focus = view_id,
                    None => return EventResult::Ignored(None),
                }

                let offset = config.scroll_lines.unsigned_abs();
                commands::scroll(cxt, offset, direction, false);

                cxt.editor.tree.focus = current_view;
                cxt.editor.ensure_cursor_in_view(current_view);

                EventResult::Consumed(None)
            }

            MouseEventKind::Up(MouseButton::Left) => {
                // End an in-progress pane-divider drag.
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

            MouseEventKind::Down(MouseButton::Right) => {
                // Right-click on editor text → JetBrains-style context menu. Only
                // for actual text positions: gutter right-clicks map to no text
                // position and fall through to the DAP breakpoint handler (on Up).
                // (gutter_coords_at_screen_coords returns Some for the whole view,
                // so it can't distinguish text from gutter — use pos_and_view.)
                if pos_and_view(cxt.editor, row, column, true).is_none() {
                    return EventResult::Ignored(None);
                }
                let path = doc!(cxt.editor).path().map(|p| p.to_path_buf());
                let cb: crate::compositor::Callback =
                    Box::new(move |compositor: &mut crate::compositor::Compositor, _cx| {
                        use crate::ui::context_menu::{ContextMenu, Entry};
                        let mut entries = editor_menu_entries(path.clone());
                        // Reveal in Tree at the end when the buffer has a path.
                        if let Some(path) = path.clone() {
                            entries.push(Entry::sep());
                            entries.push(Entry::item("Reveal in Tree", move |compositor, _cx| {
                                if let Some(view) = compositor.find::<EditorView>() {
                                    view.reveal_in_tree(&path);
                                }
                            }));
                        }
                        compositor.push(Box::new(ContextMenu::new(row, column, entries)));
                    });
                EventResult::Consumed(Some(cb))
            }

            MouseEventKind::Up(MouseButton::Right) => {
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
                    let doc = doc_mut!(editor, &view!(editor, view_id).doc);
                    doc.set_selection(view_id, Selection::point(pos));
                    cxt.editor.focus(view_id);

                    commands::paste(
                        cxt.editor,
                        config.mouse_yank_register,
                        commands::Paste::Before,
                        cxt.count(),
                    );

                    return EventResult::Consumed(None);
                }

                EventResult::Ignored(None)
            }

            _ => EventResult::Ignored(None),
        }
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
                let docs: Vec<zemacs_view::DocumentId> =
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
                            .switch(docs[next], zemacs_view::editor::Action::Replace);
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
                    use zemacs_view::editor::Action;
                    let path = context
                        .editor
                        .document(doc_id)
                        .and_then(|d| d.path().map(|p| p.to_path_buf()));
                    let all: Vec<zemacs_view::DocumentId> =
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
                            .switch(doc_id, zemacs_view::editor::Action::Replace);
                    }
                    return EventResult::Consumed(None);
                }
                // Left-click the trailing "+" opens a new scratch buffer.
                if matches!(me.kind, MouseEventKind::Down(MouseButton::Left))
                    && me.column >= self.bufferline_new.0
                    && me.column < self.bufferline_new.1
                {
                    context
                        .editor
                        .new_file(zemacs_view::editor::Action::Replace);
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
                canonicalize_key(&mut key);

                // clear status
                cx.editor.status_msg = None;

                let mode = cx.editor.mode();

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

            Event::Mouse(event) => self.handle_mouse_event(event, &mut cx),
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
        use zemacs_view::editor::BufferLine;
        let use_bufferline = match config.bufferline {
            BufferLine::Always => true,
            BufferLine::Multiple if cx.editor.documents.len() > 1 => true,
            // Always show the top tab bar while the IDE workbench is open.
            _ => self.ide.as_ref().is_some_and(Ide::visible),
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

        // -1 for commandline; -1 for the bufferline only when it lives inside `area`
        let mut editor_area = area.clip_bottom(1);
        if draw_bufferline && ide_bufrow.is_none() {
            editor_area = editor_area.clip_top(1);
        }

        // if the terminal size suddenly changed, we need to trigger a resize
        cx.editor.resize(editor_area);

        if draw_bufferline {
            let bar = ide_bufrow.unwrap_or_else(|| area.with_height(1));
            let (tabs, new_btn) = Self::render_bufferline(cx.editor, bar, surface);
            self.bufferline_tabs = tabs;
            self.bufferline_new = new_btn;
            self.bufferline_y = bar.y;
        } else {
            self.bufferline_tabs.clear();
            self.bufferline_new = (0, 0);
        }

        for (view, is_focused) in cx.editor.tree.views() {
            let doc = cx.editor.document(view.doc).unwrap();
            self.render_view(cx.editor, doc, view, area, surface, is_focused);
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

        // render status msg
        if let Some((status_msg, severity)) = &cx.editor.status_msg {
            status_msg_width = status_msg.width();
            use zemacs_view::editor::Severity;
            let style = if *severity == Severity::Error {
                cx.editor.theme.get("error")
            } else {
                cx.editor.theme.get("ui.text")
            };

            surface.set_string(
                area.x,
                area.y + area.height.saturating_sub(1),
                status_msg,
                style,
            );
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
            surface.set_string(
                area.x
                    + area
                        .width
                        .saturating_sub(key_width + macro_width + trust_width),
                area.y + area.height.saturating_sub(1),
                disp.get(disp.len().saturating_sub(key_width as usize)..)
                    .unwrap_or(&disp),
                style,
            );
            if restricted {
                let style = style
                    .fg(zemacs_view::graphics::Color::Yellow)
                    .add_modifier(Modifier::BOLD);
                surface.set_string(
                    area.x
                        .saturating_add(area.width.saturating_sub(3 + macro_width)),
                    area.y + area.height.saturating_sub(1),
                    "[⚠]",
                    style,
                );
            }
            if let Some((reg, _)) = cx.editor.macro_recording {
                let disp = format!("[{}]", reg);
                let style = style
                    .fg(zemacs_view::graphics::Color::Yellow)
                    .add_modifier(Modifier::BOLD);
                surface.set_string(
                    area.x + area.width.saturating_sub(3),
                    area.y + area.height.saturating_sub(1),
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
