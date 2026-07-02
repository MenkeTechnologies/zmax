use std::collections::HashMap;
use std::fmt::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use zemacs_core::syntax::config::LanguageServerFeature;

use crate::{
    editor::GutterType,
    graphics::{Style, UnderlineStyle},
    Document, Editor, Theme, View,
};

fn count_digits(n: usize) -> usize {
    (usize::checked_ilog10(n).unwrap_or(0) + 1) as usize
}

// ── Git-blame annotate gutter (JetBrains "Annotate") ────────────────────────
//
// The gutter renders here, but the blame itself is computed by the term layer
// (which shells out to `git blame`); it can't be run from this crate. So the
// term layer pushes the formatted per-line strings into these statics and flips
// the enable flag, and the gutter just reads them back.

/// Column width of the blame annotate gutter.
pub const BLAME_GUTTER_WIDTH: usize = 18;

/// Whether the blame annotate gutter is shown (toggled from the term layer).
static BLAME_ENABLED: AtomicBool = AtomicBool::new(false);
/// path -> compact per-line annotate strings (index 0 = line 1).
static BLAME_ANNOTATE: Mutex<Option<HashMap<PathBuf, Vec<String>>>> = Mutex::new(None);

/// Whether the blame annotate gutter is enabled.
pub fn blame_gutter_enabled() -> bool {
    BLAME_ENABLED.load(Ordering::Relaxed)
}

/// Enable or disable the blame annotate gutter.
pub fn set_blame_gutter(on: bool) {
    BLAME_ENABLED.store(on, Ordering::Relaxed);
}

/// Store the term layer's computed annotate lines for `path`.
pub fn set_blame_annotate(path: PathBuf, lines: Vec<String>) {
    if let Ok(mut g) = BLAME_ANNOTATE.lock() {
        g.get_or_insert_with(HashMap::new).insert(path, lines);
    }
}

/// True when annotate lines are already cached for `path`.
pub fn has_blame_annotate(path: &Path) -> bool {
    BLAME_ANNOTATE
        .lock()
        .ok()
        .and_then(|g| g.as_ref().map(|m| m.contains_key(path)))
        .unwrap_or(false)
}

/// Drop the cached annotate lines for `path` (after it's edited/saved).
pub fn invalidate_blame_annotate(path: &Path) {
    if let Ok(mut g) = BLAME_ANNOTATE.lock() {
        if let Some(m) = g.as_mut() {
            m.remove(path);
        }
    }
}

fn blame_annotate_lines(path: &Path) -> Option<Vec<String>> {
    BLAME_ANNOTATE.lock().ok()?.as_ref()?.get(path).cloned()
}

/// Per-line git-blame annotate column: author + relative time, left-aligned and
/// padded to [`BLAME_GUTTER_WIDTH`]. Renders nothing (and takes no width) until
/// the term layer enables it and pushes data via [`set_blame_annotate`].
pub fn blame<'doc>(
    _editor: &'doc Editor,
    doc: &'doc Document,
    _view: &View,
    theme: &Theme,
    _is_focused: bool,
) -> GutterFn<'doc> {
    if !blame_gutter_enabled() {
        return Box::new(|_, _, _, _| None);
    }
    let style = theme
        .try_get("ui.gutter.blame")
        .unwrap_or_else(|| theme.get("comment"));
    let lines = doc.path().and_then(blame_annotate_lines);
    Box::new(
        move |line: usize, _selected: bool, first_visual_line: bool, out: &mut String| {
            let s = if first_visual_line {
                lines
                    .as_ref()
                    .and_then(|v| v.get(line))
                    .map(String::as_str)
                    .unwrap_or("")
            } else {
                ""
            };
            // Write exactly BLAME_GUTTER_WIDTH cells so the following gutters
            // stay aligned: at most WIDTH-1 content chars, then pad with spaces
            // (guaranteeing a trailing separator column).
            let mut n = 0;
            for c in s.chars() {
                if n + 1 >= BLAME_GUTTER_WIDTH {
                    break;
                }
                out.push(c);
                n += 1;
            }
            for _ in n..BLAME_GUTTER_WIDTH {
                out.push(' ');
            }
            Some(style)
        },
    )
}

pub type GutterFn<'doc> = Box<dyn FnMut(usize, bool, bool, &mut String) -> Option<Style> + 'doc>;
pub type Gutter =
    for<'doc> fn(&'doc Editor, &'doc Document, &View, &Theme, bool, usize) -> GutterFn<'doc>;

impl GutterType {
    pub fn style<'doc>(
        self,
        editor: &'doc Editor,
        doc: &'doc Document,
        view: &View,
        theme: &Theme,
        is_focused: bool,
    ) -> GutterFn<'doc> {
        match self {
            GutterType::Diagnostics => {
                diagnostics_or_breakpoints(editor, doc, view, theme, is_focused)
            }
            GutterType::LineNumbers => line_numbers(editor, doc, view, theme, is_focused),
            GutterType::Spacer => padding(editor, doc, view, theme, is_focused),
            GutterType::Diff => diff(editor, doc, view, theme, is_focused),
            GutterType::CodeActionHint => code_action_hint(editor, doc, view, theme, is_focused),
            GutterType::Marks => marks(editor, doc, view, theme, is_focused),
            GutterType::Blame => blame(editor, doc, view, theme, is_focused),
        }
    }

    pub fn width(self, view: &View, doc: &Document) -> usize {
        match self {
            GutterType::Diagnostics => 1,
            GutterType::LineNumbers => line_numbers_width(view, doc),
            GutterType::Spacer => 1,
            GutterType::Diff => 1,
            GutterType::CodeActionHint => 1,
            GutterType::Marks => 1,
            // Zero width (and so invisible) until the term layer enables it.
            GutterType::Blame => {
                if blame_gutter_enabled() {
                    BLAME_GUTTER_WIDTH
                } else {
                    0
                }
            }
        }
    }
}

pub fn diagnostic<'doc>(
    _editor: &'doc Editor,
    doc: &'doc Document,
    _view: &View,
    theme: &Theme,
    _is_focused: bool,
) -> GutterFn<'doc> {
    let warning = theme.get("warning");
    let error = theme.get("error");
    let info = theme.get("info");
    let hint = theme.get("hint");
    let diagnostics = &doc.diagnostics;

    Box::new(
        move |line: usize, _selected: bool, first_visual_line: bool, out: &mut String| {
            if !first_visual_line {
                return None;
            }
            use zemacs_core::diagnostic::Severity;
            let first_diag_idx_maybe_on_line = diagnostics.partition_point(|d| d.line < line);
            let diagnostics_on_line = diagnostics[first_diag_idx_maybe_on_line..]
                .iter()
                .take_while(|d| {
                    d.line == line
                        && d.provider.language_server_id().is_none_or(|id| {
                            doc.language_servers_with_feature(LanguageServerFeature::Diagnostics)
                                .any(|ls| ls.id() == id)
                        })
                });
            diagnostics_on_line.max_by_key(|d| d.severity).map(|d| {
                write!(out, "●").ok();
                match d.severity {
                    Some(Severity::Error) => error,
                    Some(Severity::Warning) | None => warning,
                    Some(Severity::Info) => info,
                    Some(Severity::Hint) => hint,
                }
            })
        },
    )
}

/// Markology: render the full vim mark set in the gutter, one char per line.
///
/// Stored marks (`a`-`z`, `A`-`Z`, `^`, `<`, `>`, and the auto-tracked `.`/`[`/`]`) come from the
/// document; the cursor-relative marks (`(`/`)` sentence, `{`/`}` paragraph) and `'` (last jump)
/// are computed live here, matching vim's `:help marks`.
pub fn marks<'doc>(
    editor: &'doc Editor,
    doc: &'doc Document,
    view: &View,
    theme: &Theme,
    _is_focused: bool,
) -> GutterFn<'doc> {
    use std::collections::HashMap;
    use zemacs_core::textobject::{self, TextObject};

    let style = theme
        .try_get("ui.gutter.marks")
        .unwrap_or_else(|| theme.get("constant"));
    let text = doc.text();
    let slice = text.slice(..);
    let len = text.len_chars();
    let mut by_line: HashMap<usize, char> = HashMap::new();
    // Explicit marks set last keep priority (named marks beat computed sentence/paragraph).
    let mut put = |line: usize, ch: char| {
        by_line.insert(line, ch);
    };

    // Cursor-relative computed marks (lowest priority — inserted first).
    let range = doc.selection(view.id).primary();
    let sent = textobject::textobject_sentence(slice, range, TextObject::Around, 1);
    put(slice.char_to_line(sent.from().min(len)), '(');
    put(
        slice.char_to_line(sent.to().saturating_sub(1).min(len)),
        ')',
    );
    let para = textobject::textobject_paragraph(slice, range, TextObject::Around, 1);
    put(slice.char_to_line(para.from().min(len)), '{');
    put(
        slice.char_to_line(para.to().saturating_sub(1).min(len)),
        '}',
    );

    // `'` / `` ` `` — position before the most recent jump in this buffer.
    let doc_id = doc.id();
    if let Some((_, sel)) = view.jumps.iter().rfind(|(id, _)| *id == doc_id) {
        put(
            text.char_to_line(sel.primary().cursor(slice).min(len)),
            '\'',
        );
    }

    // Stored marks (named + ^ < > . [ ]) — highest priority.
    for (ch, pos) in doc.marks_iter() {
        put(text.char_to_line(pos.min(len)), ch);
    }

    // Global marks (`A`-`Z`) and numbered marks (`0`-`9`) live on the editor, not
    // the document, and store a path + line. Render the ones that point at this
    // file — vim shows uppercase/numbered marks in the gutter of whichever buffer
    // they currently target.
    if let Some(path) = doc.path() {
        let last_line = text.len_lines().saturating_sub(1);
        for (&ch, mark) in &editor.global_marks {
            if mark.path == *path {
                put(mark.line.min(last_line), ch);
            }
        }
    }

    Box::new(
        move |line: usize, _selected: bool, first_visual_line: bool, out: &mut String| {
            if !first_visual_line {
                return None;
            }
            by_line.get(&line).map(|&ch| {
                write!(out, "{ch}").ok();
                style
            })
        },
    )
}

pub fn diff<'doc>(
    _editor: &'doc Editor,
    doc: &'doc Document,
    _view: &View,
    theme: &Theme,
    _is_focused: bool,
) -> GutterFn<'doc> {
    let added = theme.get("diff.plus.gutter");
    let deleted = theme.get("diff.minus.gutter");
    let modified = theme.get("diff.delta.gutter");
    if let Some(diff_handle) = doc.diff_handle() {
        let hunks = diff_handle.load();
        let mut hunk_i = 0;
        let mut hunk = hunks.nth_hunk(hunk_i);
        Box::new(
            move |line: usize, _selected: bool, first_visual_line: bool, out: &mut String| {
                // truncating the line is fine here because we don't compute diffs
                // for files with more lines than i32::MAX anyways
                // we need to special case removals here
                // these technically do not have a range of lines to highlight (`hunk.after.start == hunk.after.end`).
                // However we still want to display these hunks correctly we must not yet skip to the next hunk here
                while hunk.after.end < line as u32
                    || !hunk.is_pure_removal() && line as u32 == hunk.after.end
                {
                    hunk_i += 1;
                    hunk = hunks.nth_hunk(hunk_i);
                }

                if hunk.after.start > line as u32 {
                    return None;
                }

                let (icon, style) = if hunk.is_pure_insertion() {
                    ("▍", added)
                } else if hunk.is_pure_removal() {
                    if !first_visual_line {
                        return None;
                    }
                    ("▔", deleted)
                } else {
                    ("▍", modified)
                };

                write!(out, "{}", icon).unwrap();
                Some(style)
            },
        )
    } else {
        Box::new(move |_, _, _, _| None)
    }
}

pub fn line_numbers<'doc>(
    editor: &'doc Editor,
    doc: &'doc Document,
    view: &View,
    theme: &Theme,
    is_focused: bool,
) -> GutterFn<'doc> {
    let text = doc.text().slice(..);
    let width = line_numbers_width(view, doc);

    let last_line_in_view = view.estimate_last_doc_line(doc);

    // Whether to draw the line number for the last line of the
    // document or not.  We only draw it if it's not an empty line.
    let draw_last = text.line_to_byte(last_line_in_view) < text.len_bytes();

    let linenr = theme.get("ui.linenr");
    let linenr_select = theme.get("ui.linenr.selected");

    let current_line = doc
        .text()
        .char_to_line(doc.selection(view.id).primary().cursor(text));

    let line_number = editor.config().line_number;
    let mode = editor.mode;

    Box::new(
        move |line: usize, selected: bool, first_visual_line: bool, out: &mut String| {
            if line == last_line_in_view && !draw_last {
                write!(out, "{:>1$}", '~', width).unwrap();
                Some(linenr)
            } else {
                use crate::{document::Mode, editor::LineNumber};

                let relative = line_number == LineNumber::Relative
                    && mode != Mode::Insert
                    && is_focused
                    && current_line != line;

                let display_num = if relative {
                    current_line.abs_diff(line)
                } else {
                    line + 1
                };

                let style = if selected && is_focused {
                    linenr_select
                } else {
                    linenr
                };

                if first_visual_line {
                    write!(out, "{:>1$}", display_num, width).unwrap();
                } else {
                    write!(out, "{:>1$}", " ", width).unwrap();
                }

                first_visual_line.then_some(style)
            }
        },
    )
}

/// The width of a "line-numbers" gutter
///
/// The width of the gutter depends on the number of lines in the document,
/// whether there is content on the last line (the `~` line), and the
/// `editor.gutters.line-numbers.min-width` settings.
fn line_numbers_width(view: &View, doc: &Document) -> usize {
    let text = doc.text();
    let last_line = text.len_lines().saturating_sub(1);
    let draw_last = text.line_to_byte(last_line) < text.len_bytes();
    let last_drawn = if draw_last { last_line + 1 } else { last_line };
    let digits = count_digits(last_drawn);
    let n_min = view.gutters.line_numbers.min_width;
    digits.max(n_min)
}

pub fn padding<'doc>(
    _editor: &'doc Editor,
    _doc: &'doc Document,
    _view: &View,
    _theme: &Theme,
    _is_focused: bool,
) -> GutterFn<'doc> {
    Box::new(|_line: usize, _selected: bool, _first_visual_line: bool, _out: &mut String| None)
}

pub fn breakpoints<'doc>(
    editor: &'doc Editor,
    doc: &'doc Document,
    _view: &View,
    theme: &Theme,
    _is_focused: bool,
) -> GutterFn<'doc> {
    let error = theme.get("error");
    let info = theme.get("info");
    let breakpoint_style = theme.get("ui.debug.breakpoint");

    let breakpoints = doc.path().and_then(|path| editor.breakpoints.get(path));

    let breakpoints = match breakpoints {
        Some(breakpoints) => breakpoints,
        None => return Box::new(move |_, _, _, _| None),
    };

    Box::new(
        move |line: usize, _selected: bool, first_visual_line: bool, out: &mut String| {
            if !first_visual_line {
                return None;
            }
            let breakpoint = breakpoints
                .iter()
                .find(|breakpoint| breakpoint.line == line)?;

            let style = if breakpoint.condition.is_some() && breakpoint.log_message.is_some() {
                error.underline_style(UnderlineStyle::Line)
            } else if breakpoint.condition.is_some() {
                error
            } else if breakpoint.log_message.is_some() {
                info
            } else {
                breakpoint_style
            };

            let sym = if breakpoint.verified { "●" } else { "◯" };
            write!(out, "{}", sym).unwrap();
            Some(style)
        },
    )
}

fn execution_pause_indicator<'doc>(
    editor: &'doc Editor,
    doc: &'doc Document,
    theme: &Theme,
    is_focused: bool,
) -> GutterFn<'doc> {
    let style = theme.get("ui.debug.active");
    let current_stack_frame = editor.current_stack_frame();
    let frame_line = current_stack_frame.map(|frame| frame.line.saturating_sub(1));
    let frame_source_path = current_stack_frame.map(|frame| {
        frame
            .source
            .as_ref()
            .and_then(|source| source.path.as_deref())
    });

    let should_display_for_current_doc =
        doc.path().is_some() && frame_source_path.unwrap_or(None) == doc.path();

    Box::new(
        move |line: usize, _selected: bool, first_visual_line: bool, out: &mut String| {
            if !first_visual_line
                || !is_focused
                || line != frame_line?
                || !should_display_for_current_doc
            {
                return None;
            }

            let sym = "▶";
            write!(out, "{}", sym).unwrap();
            Some(style)
        },
    )
}

pub fn diagnostics_or_breakpoints<'doc>(
    editor: &'doc Editor,
    doc: &'doc Document,
    view: &View,
    theme: &Theme,
    is_focused: bool,
) -> GutterFn<'doc> {
    let mut diagnostics = diagnostic(editor, doc, view, theme, is_focused);
    let mut breakpoints = breakpoints(editor, doc, view, theme, is_focused);
    let mut execution_pause_indicator = execution_pause_indicator(editor, doc, theme, is_focused);

    Box::new(move |line, selected, first_visual_line: bool, out| {
        execution_pause_indicator(line, selected, first_visual_line, out)
            .or_else(|| breakpoints(line, selected, first_visual_line, out))
            .or_else(|| diagnostics(line, selected, first_visual_line, out))
    })
}

pub fn code_action_hint<'doc>(
    _editor: &'doc Editor,
    doc: &'doc Document,
    view: &View,
    theme: &Theme,
    is_focused: bool,
) -> GutterFn<'doc> {
    let style = theme.get("ui.text");
    let text = doc.text().slice(..);
    let show_hint = doc.code_action_hints(view.id);
    let current_line = doc
        .text()
        .char_to_line(doc.selection(view.id).primary().cursor(text));

    Box::new(
        move |line: usize, _selected: bool, first_visual_line: bool, out: &mut String| {
            (is_focused && show_hint && current_line == line && first_visual_line).then(|| {
                write!(out, "⋮").unwrap();
                style
            })
        },
    )
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::document::Document;
    use crate::editor::{Config, GutterConfig, GutterLineNumbersConfig};
    use crate::graphics::Rect;
    use crate::DocumentId;
    use arc_swap::ArcSwap;
    use zemacs_core::{syntax, Rope};

    #[test]
    fn test_default_gutter_widths() {
        let mut view = View::new(DocumentId::default(), GutterConfig::default());
        view.area = Rect::new(40, 40, 40, 40);

        let rope = Rope::from_str("abc\n\tdef");
        let doc = Document::from(
            rope,
            None,
            Arc::new(ArcSwap::new(Arc::new(Config::default()))),
            Arc::new(ArcSwap::from_pointee(syntax::Loader::default())),
        );

        // default layout: blame, diagnostics, marks, spacer, line-numbers, spacer, diff
        assert_eq!(view.gutters.layout.len(), 7);
        assert_eq!(view.gutters.layout[0].width(&view, &doc), 0); // blame (disabled → 0 width)
        assert_eq!(view.gutters.layout[1].width(&view, &doc), 1); // diagnostics
        assert_eq!(view.gutters.layout[2].width(&view, &doc), 1); // marks
        assert_eq!(view.gutters.layout[3].width(&view, &doc), 1); // spacer
        assert_eq!(view.gutters.layout[4].width(&view, &doc), 3); // line numbers
        assert_eq!(view.gutters.layout[5].width(&view, &doc), 1); // spacer
        assert_eq!(view.gutters.layout[6].width(&view, &doc), 1); // diff
    }

    #[test]
    fn test_configured_gutter_widths() {
        let gutters = GutterConfig {
            layout: vec![GutterType::Diagnostics],
            ..Default::default()
        };

        let mut view = View::new(DocumentId::default(), gutters);
        view.area = Rect::new(40, 40, 40, 40);

        let rope = Rope::from_str("abc\n\tdef");
        let doc = Document::from(
            rope,
            None,
            Arc::new(ArcSwap::new(Arc::new(Config::default()))),
            Arc::new(ArcSwap::from_pointee(syntax::Loader::default())),
        );

        assert_eq!(view.gutters.layout.len(), 1);
        assert_eq!(view.gutters.layout[0].width(&view, &doc), 1);

        let gutters = GutterConfig {
            layout: vec![GutterType::Diagnostics, GutterType::LineNumbers],
            line_numbers: GutterLineNumbersConfig { min_width: 10 },
        };

        let mut view = View::new(DocumentId::default(), gutters);
        view.area = Rect::new(40, 40, 40, 40);

        let rope = Rope::from_str("abc\n\tdef");
        let doc = Document::from(
            rope,
            None,
            Arc::new(ArcSwap::new(Arc::new(Config::default()))),
            Arc::new(ArcSwap::from_pointee(syntax::Loader::default())),
        );

        assert_eq!(view.gutters.layout.len(), 2);
        assert_eq!(view.gutters.layout[0].width(&view, &doc), 1);
        assert_eq!(view.gutters.layout[1].width(&view, &doc), 10);
    }

    #[test]
    fn test_line_numbers_gutter_width_resizes() {
        let gutters = GutterConfig {
            layout: vec![GutterType::Diagnostics, GutterType::LineNumbers],
            line_numbers: GutterLineNumbersConfig { min_width: 1 },
        };

        let mut view = View::new(DocumentId::default(), gutters);
        view.area = Rect::new(40, 40, 40, 40);

        let rope = Rope::from_str("a\nb");
        let doc_short = Document::from(
            rope,
            None,
            Arc::new(ArcSwap::new(Arc::new(Config::default()))),
            Arc::new(ArcSwap::from_pointee(syntax::Loader::default())),
        );

        let rope = Rope::from_str("a\nb\nc\nd\ne\nf\ng\nh\ni\nj\nk\nl\nm\nn\no\np");
        let doc_long = Document::from(
            rope,
            None,
            Arc::new(ArcSwap::new(Arc::new(Config::default()))),
            Arc::new(ArcSwap::from_pointee(syntax::Loader::default())),
        );

        assert_eq!(view.gutters.layout.len(), 2);
        assert_eq!(view.gutters.layout[1].width(&view, &doc_short), 1);
        assert_eq!(view.gutters.layout[1].width(&view, &doc_long), 2);
    }
}
