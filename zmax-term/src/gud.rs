//! GNU Emacs GUD/GDB session state that outlives a single command: the watch
//! expressions the speedbar shows, the `gud-tooltip-mode` flag, the
//! `next-error` follow/selection state, and the on-disk window configuration
//! `gdb-save-window-configuration` / `gdb-load-window-configuration` read and
//! write.
//!
//! The debugger itself is zmax's DAP client (`zmax-dap`); nothing here talks to
//! an adapter. This module owns the process-global bookkeeping and the pure
//! conversion between the live window tree and its serialized form, so both are
//! testable without an editor.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use zmax_view::editor::Action;
use zmax_view::tree::{Layout, TreeShape};
use zmax_view::{Editor, View};

// ── Watch expressions (the GDB speedbar's watch list) ───────────────────────
//
// Emacs's GDB speedbar holds one "variable object" per watched expression
// (`gdb-var-create` adds, `gdb-var-delete` removes). zmax keeps the expression
// strings and re-evaluates them through DAP `evaluate` whenever the watch
// buffer is drawn, which is the same observable behaviour without gdb's
// varobj bookkeeping.

/// The watched expressions, in the order they were added.
static WATCH: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Add `expr` to the watch list. Returns false when it is already watched
/// (Emacs also refuses to create a second varobj for the same expression).
pub fn watch_add(expr: &str) -> bool {
    let expr = expr.trim();
    if expr.is_empty() {
        return false;
    }
    let Ok(mut list) = WATCH.lock() else {
        return false;
    };
    if list.iter().any(|e| e == expr) {
        return false;
    }
    list.push(expr.to_string());
    true
}

/// `gdb-var-delete`: drop `expr` from the watch list. Returns false when it was
/// not being watched.
pub fn watch_remove(expr: &str) -> bool {
    let expr = expr.trim();
    let Ok(mut list) = WATCH.lock() else {
        return false;
    };
    let before = list.len();
    list.retain(|e| e != expr);
    list.len() != before
}

/// `gdb-var-delete` on the row at `index` of the watch buffer. Returns the
/// deleted expression.
pub fn watch_remove_at(index: usize) -> Option<String> {
    let mut list = WATCH.lock().ok()?;
    (index < list.len()).then(|| list.remove(index))
}

/// The watch list, newest last.
pub fn watch_list() -> Vec<String> {
    WATCH.lock().map(|l| l.clone()).unwrap_or_default()
}

// ── Disabled breakpoints (the Breakpoints buffer's SPC) ─────────────────────
//
// `Editor::breakpoints` is the set zmax pushes to the adapter, so a breakpoint
// cannot be "listed but inactive" there. gdb's `disable` keeps the breakpoint in
// `info breakpoints` while withdrawing it from the running program, which is
// what the Emacs Breakpoints buffer's SPC toggles — so a disabled breakpoint is
// parked here and re-armed on the next SPC.

/// Parked breakpoints: `(file, 0-based line, condition)`.
static DISABLED: Mutex<Vec<(PathBuf, usize, Option<String>)>> = Mutex::new(Vec::new());

/// Every parked breakpoint, for the Breakpoints buffer's listing.
pub fn disabled_breakpoints() -> Vec<(PathBuf, usize, Option<String>)> {
    DISABLED.lock().map(|d| d.clone()).unwrap_or_default()
}

/// Drop a parked breakpoint without re-arming it (`D` on a disabled row).
pub fn forget_disabled(path: &Path, line: usize) -> bool {
    let Ok(mut disabled) = DISABLED.lock() else {
        return false;
    };
    let before = disabled.len();
    disabled.retain(|(p, l, _)| !(p == path && *l == line));
    disabled.len() != before
}

/// Push the breakpoints of `path` to the debug adapter, if a session is live.
/// Without one the editor's set is still the source of truth and is re-sent on
/// the next launch, so a missing adapter is not an error.
fn push_breakpoints(editor: &mut zmax_view::Editor, path: &Path) -> Result<(), String> {
    let mut list = editor.breakpoints.get(path).cloned().unwrap_or_default();
    let Some(debugger) = editor.debug_adapters.get_active_client_mut() else {
        return Ok(());
    };
    zmax_view::handlers::dap::breakpoints_changed(debugger, path.to_path_buf(), &mut list)
        .map_err(|e| e.to_string())?;
    // The adapter fills in ids/verified flags; keep them.
    editor.breakpoints.insert(path.to_path_buf(), list);
    Ok(())
}

/// `gdb-toggle-breakpoint` (`SPC`, disabling half): withdraw the breakpoint from
/// the adapter but keep it listed.
pub fn disable_breakpoint(editor: &mut zmax_view::Editor, path: &Path, line: usize) -> bool {
    let Some(list) = editor.breakpoints.get_mut(path) else {
        return false;
    };
    let Some(pos) = list.iter().position(|b| b.line == line) else {
        return false;
    };
    let bp = list.remove(pos);
    if let Ok(mut disabled) = DISABLED.lock() {
        disabled.push((path.to_path_buf(), line, bp.condition));
    }
    let _ = push_breakpoints(editor, path);
    true
}

/// `gdb-toggle-breakpoint` (`SPC`, enabling half): re-arm a parked breakpoint.
pub fn enable_breakpoint(editor: &mut zmax_view::Editor, path: &Path, line: usize) -> bool {
    let condition = {
        let Ok(mut disabled) = DISABLED.lock() else {
            return false;
        };
        let Some(pos) = disabled
            .iter()
            .position(|(p, l, _)| p == path && *l == line)
        else {
            return false;
        };
        disabled.remove(pos).2
    };
    let list = editor.breakpoints.entry(path.to_path_buf()).or_default();
    if !list.iter().any(|b| b.line == line) {
        list.push(zmax_view::editor::Breakpoint {
            line,
            condition,
            ..Default::default()
        });
    }
    let _ = push_breakpoints(editor, path);
    true
}

/// `gdb-delete-breakpoint` (`D`): remove an enabled breakpoint and re-send the
/// file's set.
pub fn delete_breakpoint(editor: &mut zmax_view::Editor, path: &Path, line: usize) -> bool {
    let Some(list) = editor.breakpoints.get_mut(path) else {
        return false;
    };
    let Some(pos) = list.iter().position(|b| b.line == line) else {
        return false;
    };
    list.remove(pos);
    let _ = push_breakpoints(editor, path);
    true
}

// ── Frame / value helpers shared by the GDB buffers ─────────────────────────

/// The DAP frame id the data buffers read from: the user's selected frame of the
/// current thread, or its innermost frame.
pub fn selected_frame_id(editor: &zmax_view::Editor) -> Option<usize> {
    let debugger = editor.debug_adapters.get_active_client()?;
    let thread_id = debugger.thread_id?;
    let frames = debugger.stack_frames.get(&thread_id)?;
    frames.get(debugger.active_frame.unwrap_or(0)).map(|f| f.id)
}

/// `gdb-edit-value`: assign `value` to the lvalue `expr`. Uses DAP
/// `setExpression` when the adapter supports it, else an assignment `evaluate`
/// (which gdb and lldb both honour). Returns the adapter's new value.
pub fn assign_value(
    editor: &zmax_view::Editor,
    expr: &str,
    value: &str,
    frame_id: Option<usize>,
) -> Result<String, String> {
    let debugger = editor
        .debug_adapters
        .get_active_client()
        .ok_or("no debug session")?;
    if debugger
        .capabilities()
        .supports_set_expression
        .unwrap_or(false)
    {
        zmax_lsp::block_on(debugger.set_expression(expr.to_string(), value.to_string(), frame_id))
            .map(|r| r.value)
            .map_err(|e| e.to_string())
    } else {
        zmax_lsp::block_on(debugger.eval(format!("{expr} = {value}"), frame_id))
            .map(|r| r.result)
            .map_err(|e| e.to_string())
    }
}

/// The identifier at `pos` in `doc_id`, for `gud-tooltip-mode` and `:gdb-watch`.
/// Uses the same word text object the editor's `w`/`b` motions do.
pub fn word_at(
    editor: &zmax_view::Editor,
    doc_id: zmax_view::DocumentId,
    pos: usize,
) -> Option<String> {
    let doc = editor.documents.get(&doc_id)?;
    let text = doc.text().slice(..);
    let range = zmax_core::Range::point(pos.min(text.len_chars()));
    let word = zmax_core::textobject::textobject_word(
        text,
        range,
        zmax_core::textobject::TextObject::Inside,
        1,
        false,
    );
    let s = text.slice(word.from()..word.to()).to_string();
    let s = s.trim().to_string();
    (!s.is_empty() && s.chars().any(|c| c.is_alphanumeric() || c == '_')).then_some(s)
}

/// Evaluate `expr` in the selected frame (DAP `evaluate`). Used by
/// `gud-tooltip-mode` and the watch buffer.
pub fn eval_expression(
    editor: &zmax_view::Editor,
    expr: &str,
    frame_id: Option<usize>,
) -> Result<String, String> {
    let debugger = editor
        .debug_adapters
        .get_active_client()
        .ok_or("no debug session")?;
    zmax_lsp::block_on(debugger.eval(expr.to_string(), frame_id))
        .map(|r| r.result)
        .map_err(|e| e.to_string())
}

// ── gud-tooltip-mode ────────────────────────────────────────────────────────

/// `gud-tooltip-mode`: when on, pointing at an identifier in a source buffer
/// during a stopped debug session shows its value.
static TOOLTIP_MODE: AtomicBool = AtomicBool::new(false);

/// Whether `gud-tooltip-mode` is on.
pub fn tooltip_mode() -> bool {
    TOOLTIP_MODE.load(Ordering::Relaxed)
}

/// Set `gud-tooltip-mode`, returning the new state.
pub fn set_tooltip_mode(on: bool) -> bool {
    TOOLTIP_MODE.store(on, Ordering::Relaxed);
    on
}

/// Toggle `gud-tooltip-mode`, returning the new state.
pub fn toggle_tooltip_mode() -> bool {
    let on = !tooltip_mode();
    set_tooltip_mode(on);
    on
}

// ── next-error follow / buffer selection ────────────────────────────────────

/// `next-error-follow-minor-mode`: when on, merely moving over an entry in an
/// error/match list visits it, instead of waiting for `RET`.
static NEXT_ERROR_FOLLOW: AtomicBool = AtomicBool::new(false);

/// Whether `next-error-follow-minor-mode` is on.
pub fn next_error_follow() -> bool {
    NEXT_ERROR_FOLLOW.load(Ordering::Relaxed)
}

/// Set `next-error-follow-minor-mode`, returning the new state.
pub fn set_next_error_follow(on: bool) -> bool {
    NEXT_ERROR_FOLLOW.store(on, Ordering::Relaxed);
    on
}

/// Toggle `next-error-follow-minor-mode`, returning the new state.
pub fn toggle_next_error_follow() -> bool {
    let on = !next_error_follow();
    set_next_error_follow(on);
    on
}

/// Which list `next-error` / `previous-error` / `first-error` walk. Emacs picks
/// a *buffer* (`next-error-select-buffer`); zmax's equivalent choice is which of
/// its error lists is current, since each lives in its own store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorSource {
    /// The `:compile` / `:make` compilation list (the default, as in Emacs).
    Compilation,
    /// The vim quickfix list (`:cnext`, filled by `:cgetexpr`/`:grep`).
    Quickfix,
    /// The focused window's location list (`:lnext`, filled by `:lmake`).
    LocationList,
    /// LSP diagnostics, walked in buffer order.
    Diagnostics,
}

impl ErrorSource {
    /// The name `:next-error-select-buffer` accepts and reports.
    pub fn name(self) -> &'static str {
        match self {
            ErrorSource::Compilation => "compilation",
            ErrorSource::Quickfix => "quickfix",
            ErrorSource::LocationList => "loclist",
            ErrorSource::Diagnostics => "diagnostics",
        }
    }

    /// Parse a `:next-error-select-buffer` argument.
    pub fn parse(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "compilation" | "compile" | "*compilation*" => Some(ErrorSource::Compilation),
            "quickfix" | "qf" => Some(ErrorSource::Quickfix),
            "loclist" | "location" | "location-list" => Some(ErrorSource::LocationList),
            "diagnostics" | "diag" => Some(ErrorSource::Diagnostics),
            _ => None,
        }
    }

    /// Every name `:next-error-select-buffer` completes, for its completer and
    /// its error message.
    pub const NAMES: &'static [&'static str] =
        &["compilation", "quickfix", "loclist", "diagnostics"];
}

/// The selected error list. A plain global rather than an atomic enum: the UI is
/// single-threaded and this is read on every `next-error`.
static ERROR_SOURCE: Mutex<ErrorSource> = Mutex::new(ErrorSource::Compilation);

/// Which error list `next-error` currently walks.
pub fn error_source() -> ErrorSource {
    ERROR_SOURCE
        .lock()
        .map(|s| *s)
        .unwrap_or(ErrorSource::Compilation)
}

/// `next-error-select-buffer`: make `source` the list `next-error` walks.
pub fn set_error_source(source: ErrorSource) {
    if let Ok(mut s) = ERROR_SOURCE.lock() {
        *s = source;
    }
}

// ── Window configurations ───────────────────────────────────────────────────

/// A serialized window tree: the same shape as [`TreeShape`], with each leaf's
/// document replaced by its file path (a pathless scratch buffer serializes as
/// `null` and reloads as an empty buffer).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum WinConfig {
    /// One window showing `file`.
    Leaf {
        /// The window's file, or `None` for a scratch buffer.
        file: Option<PathBuf>,
        /// Whether this window had focus.
        focused: bool,
    },
    /// A split containing `children`, each with its share of the parent's size.
    Split {
        /// `"horizontal"` (children stacked) or `"vertical"` (side by side).
        layout: String,
        /// The children, in tree order, with their weights.
        children: Vec<WinChild>,
    },
}

/// One child of a [`WinConfig::Split`]: its size weight and its subtree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WinChild {
    /// The child's share of the split (zmax's `Node::weight`).
    pub weight: f32,
    /// The child subtree.
    pub node: WinConfig,
}

/// Spell a [`Layout`] the way [`WinConfig`] stores it.
fn layout_name(layout: Layout) -> &'static str {
    match layout {
        Layout::Horizontal => "horizontal",
        Layout::Vertical => "vertical",
    }
}

/// Read a [`Layout`] back. Anything unrecognised is a vertical (side-by-side)
/// split, which is what `:vsplit` produces and what zmax's root container is.
fn layout_of(name: &str) -> Layout {
    match name {
        "horizontal" => Layout::Horizontal,
        _ => Layout::Vertical,
    }
}

/// The default file `gdb-save-window-configuration` writes and
/// `gdb-load-window-configuration` reads when called with no argument
/// (Emacs: `gdb-default-window-configuration-file` under
/// `gdb-window-configuration-directory`).
pub fn default_config_file() -> PathBuf {
    zmax_loader::config_dir().join("gdb-window-configuration.json")
}

/// `gdb-save-window-configuration`: snapshot the live window tree.
pub fn capture(editor: &Editor) -> WinConfig {
    from_shape(editor, &editor.tree.shape())
}

/// Convert one live [`TreeShape`] node, resolving each document to its path.
fn from_shape(editor: &Editor, shape: &TreeShape) -> WinConfig {
    match shape {
        TreeShape::Leaf { doc, focused } => WinConfig::Leaf {
            file: editor
                .documents
                .get(doc)
                .and_then(|d| d.path().map(Path::to_path_buf)),
            focused: *focused,
        },
        TreeShape::Split { layout, children } => WinConfig::Split {
            layout: layout_name(*layout).to_string(),
            children: children
                .iter()
                .map(|(weight, child)| WinChild {
                    weight: *weight,
                    node: from_shape(editor, child),
                })
                .collect(),
        },
    }
}

/// The number of windows a configuration describes.
pub fn window_count(cfg: &WinConfig) -> usize {
    match cfg {
        WinConfig::Leaf { .. } => 1,
        WinConfig::Split { children, .. } => children.iter().map(|c| window_count(&c.node)).sum(),
    }
}

/// `gdb-load-window-configuration`: rebuild the window tree from `cfg`, opening
/// every file it names. Returns the number of windows restored.
///
/// Files that no longer exist are skipped; if that leaves nothing to show the
/// tree is left untouched and `Err` explains why.
pub fn restore(editor: &mut Editor, cfg: &WinConfig) -> Result<usize, String> {
    // Open every leaf's file first: `build_from_shape` needs a DocumentId per
    // leaf, and opening reuses an already-open document when there is one.
    let mut docs = Vec::new();
    collect_docs(editor, cfg, &mut docs);
    if docs.iter().all(Option::is_none) {
        return Err("no window in the saved configuration could be opened".to_string());
    }

    let mut next = docs.into_iter();
    let Some(shape) = to_shape(cfg, &mut next) else {
        return Err("saved configuration has no usable window".to_string());
    };

    let gutters = editor.config().gutters.clone();
    let mut make = |doc| View::new(doc, gutters.clone());
    let new_ids = editor.tree.build_from_shape(&shape, &mut make);
    for &view_id in &new_ids {
        let doc_id = editor.tree.get(view_id).doc;
        if let Some(doc) = editor.documents.get_mut(&doc_id) {
            doc.ensure_view_init(view_id);
        }
    }
    let focus = editor.tree.focus;
    editor.ensure_cursor_in_view(focus);
    Ok(new_ids.len())
}

/// Open each leaf's file (in tree order), recording the document it landed in.
/// A leaf whose file is missing or unreadable records `None`.
fn collect_docs(
    editor: &mut Editor,
    cfg: &WinConfig,
    out: &mut Vec<Option<zmax_view::DocumentId>>,
) {
    match cfg {
        WinConfig::Leaf { file, .. } => {
            let doc = match file {
                // `Action::Load` opens the document without touching the layout,
                // which this function must not disturb before build_from_shape.
                Some(path) => editor.open(path, Action::Load).ok(),
                None => Some(editor.new_file(Action::Load)),
            };
            out.push(doc);
        }
        WinConfig::Split { children, .. } => {
            for child in children {
                collect_docs(editor, &child.node, out);
            }
        }
    }
}

/// Rebuild a [`TreeShape`], consuming one entry of `docs` per leaf. Leaves whose
/// document failed to open are dropped, and a split left with no children drops
/// with them.
fn to_shape(
    cfg: &WinConfig,
    docs: &mut impl Iterator<Item = Option<zmax_view::DocumentId>>,
) -> Option<TreeShape> {
    match cfg {
        WinConfig::Leaf { focused, .. } => docs.next().flatten().map(|doc| TreeShape::Leaf {
            doc,
            focused: *focused,
        }),
        WinConfig::Split { layout, children } => {
            let kept: Vec<(f32, TreeShape)> = children
                .iter()
                .filter_map(|c| to_shape(&c.node, docs).map(|node| (c.weight, node)))
                .collect();
            match kept.len() {
                0 => None,
                // A split with one surviving child is that child.
                1 => Some(kept.into_iter().next().expect("checked len").1),
                _ => Some(TreeShape::Split {
                    layout: layout_of(layout),
                    children: kept,
                }),
            }
        }
    }
}

/// Write `cfg` to `path` as JSON, creating the parent directory.
pub fn write_config(path: &Path, cfg: &WinConfig) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let json = serde_json::to_string_pretty(cfg).map_err(|e| e.to_string())?;
    std::fs::write(path, json).map_err(|e| format!("{}: {e}", path.display()))
}

/// Read a configuration written by [`write_config`].
pub fn read_config(path: &Path) -> Result<WinConfig, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaf(name: &str, focused: bool) -> WinConfig {
        WinConfig::Leaf {
            file: Some(PathBuf::from(name)),
            focused,
        }
    }

    #[test]
    fn config_round_trips_through_json() {
        let cfg = WinConfig::Split {
            layout: "vertical".to_string(),
            children: vec![
                WinChild {
                    weight: 1.0,
                    node: leaf("src/main.rs", false),
                },
                WinChild {
                    weight: 2.0,
                    node: WinConfig::Split {
                        layout: "horizontal".to_string(),
                        children: vec![
                            WinChild {
                                weight: 1.0,
                                node: leaf("src/lib.rs", true),
                            },
                            WinChild {
                                weight: 1.0,
                                node: WinConfig::Leaf {
                                    file: None,
                                    focused: false,
                                },
                            },
                        ],
                    },
                },
            ],
        };
        let json = serde_json::to_string(&cfg).unwrap();
        assert_eq!(serde_json::from_str::<WinConfig>(&json).unwrap(), cfg);
        assert_eq!(window_count(&cfg), 3);
    }

    #[test]
    fn missing_files_collapse_the_split_they_emptied() {
        let cfg = WinConfig::Split {
            layout: "vertical".to_string(),
            children: vec![
                WinChild {
                    weight: 1.0,
                    node: leaf("gone.rs", false),
                },
                WinChild {
                    weight: 1.0,
                    node: leaf("kept.rs", true),
                },
            ],
        };
        // First leaf failed to open, second succeeded: the split collapses to
        // the surviving leaf rather than leaving a one-child container.
        let doc = zmax_view::DocumentId::default();
        let mut docs = vec![None, Some(doc)].into_iter();
        let shape = to_shape(&cfg, &mut docs).unwrap();
        assert!(matches!(shape, TreeShape::Leaf { focused: true, .. }));

        // Every leaf failing leaves nothing to build.
        let mut docs = vec![None, None].into_iter();
        assert!(to_shape(&cfg, &mut docs).is_none());
    }

    #[test]
    fn layout_names_round_trip() {
        assert_eq!(layout_name(Layout::Horizontal), "horizontal");
        assert_eq!(layout_name(Layout::Vertical), "vertical");
        assert_eq!(layout_of("horizontal"), Layout::Horizontal);
        assert_eq!(layout_of("vertical"), Layout::Vertical);
        // Unknown spellings fall back to the root container's layout.
        assert_eq!(layout_of("tabbed"), Layout::Vertical);
    }

    #[test]
    fn watch_list_rejects_duplicates_and_deletes_by_row() {
        // The store is process-global; start from a known state.
        WATCH.lock().unwrap().clear();
        assert!(watch_add("argc"));
        assert!(watch_add("argv[0]"));
        assert!(!watch_add("argc"), "duplicate must be refused");
        assert!(!watch_add("   "), "blank expression must be refused");
        assert_eq!(watch_list(), vec!["argc", "argv[0]"]);
        assert_eq!(watch_remove_at(0).as_deref(), Some("argc"));
        assert!(watch_remove_at(5).is_none());
        assert!(watch_remove("argv[0]"));
        assert!(!watch_remove("argv[0]"));
        assert!(watch_list().is_empty());
    }

    #[test]
    fn error_source_names_parse_back() {
        for name in ErrorSource::NAMES {
            let parsed = ErrorSource::parse(name).expect("documented name parses");
            assert_eq!(parsed.name(), *name);
        }
        assert_eq!(
            ErrorSource::parse("*compilation*"),
            Some(ErrorSource::Compilation)
        );
        assert!(ErrorSource::parse("nonsense").is_none());
    }
}
