//! Diff-mode — the zmax port of GNU Emacs `diff-mode`, a self-contained
//! unified-diff viewer overlay.
//!
//! A modal full-screen [`Component`] that renders a parsed unified diff as a
//! scrollable, colour-coded listing: added lines green (`diff.plus`), removed red
//! (`diff.minus`, or `error` if that scope is absent), file banners highlighted
//! (`ui.text.focus`) and hunk headers accented (`function`). All parsing lives in
//! the filesystem-free, unit-tested [`zmax_core::diffmode`]; this module only
//! renders and handles keys.
//!
//! Keys (parsed into a `diffmode` keymap mode by `scripts/gen_port_report.py`, so
//! each maps to its Emacs `diff-mode` counterpart in the port tracker):
//!   j/k/n-arrows — line down/up (`C-n`/`C-p`, Down/Up)
//!   C-d/PgDn, C-u/PgUp — page down / up
//!   g/Home, G/End — top / bottom
//!   n / p — diff-hunk-next / diff-hunk-prev (jump to the next/prev `@@` header)
//!   } / M-n, { / M-p — diff-file-next / diff-file-prev (next/prev file banner)
//!   Enter / o — diff-goto-source: visit the current file's new path if on disk
//!   r — diff-refine-hunk (emacs binds it to `C-c C-b`, which is not available
//!       here because `C-c` already quits the overlay)
//!   q/Esc/C-c — quit
//!
//! Deferred to a later slice: diff-restrict-view (`|`, narrow to one file/hunk).

use std::collections::HashMap;
use std::ops::Range;
use std::path::PathBuf;

use imara_diff::{Algorithm, Diff, InternedInput};
use tui::buffer::Buffer as Surface;
use zmax_core::diffmode::{self, DiffLine, LineKind};
use zmax_view::{
    editor::Action,
    graphics::{Modifier, Rect},
};

use crate::{
    alt,
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

/// The interactive diff-mode overlay.
pub struct DiffMode {
    /// The flattened, renderable diff lines.
    flat: Vec<DiffLine>,
    /// Per-line kinds, kept alongside `flat` for the pure navigation helpers.
    kinds: Vec<LineKind>,
    /// Flat indices of each file's `FileHeader` line (aligned with `new_paths`).
    file_starts: Vec<usize>,
    /// Each file's new path, for diff-goto-source.
    new_paths: Vec<String>,
    file_count: usize,
    added: usize,
    removed: usize,
    /// Current line (point).
    cursor: usize,
    scroll: usize,
    viewport: usize,
    status: Option<String>,
    /// diff-refine-hunk output: flat line index → that line's text split into
    /// `(text, emphasised)` runs, where the emphasised runs are the characters
    /// the fine-grained refinement flagged as changed (emacs's
    /// `diff-refine-removed` / `diff-refine-added` overlays). Lines absent from
    /// the map are drawn whole, in their line style.
    refine: HashMap<usize, Vec<(String, bool)>>,
}

impl DiffMode {
    /// Build the overlay from raw unified-diff text.
    pub fn new(diff_text: String) -> Self {
        let diff = diffmode::parse(&diff_text);
        let flat = diffmode::flatten(&diff);
        let kinds: Vec<LineKind> = flat.iter().map(|l| l.kind).collect();
        let file_starts: Vec<usize> = flat
            .iter()
            .enumerate()
            .filter(|(_, l)| l.kind == LineKind::FileHeader)
            .map(|(i, _)| i)
            .collect();
        let new_paths: Vec<String> = diff.files.iter().map(|f| f.new_path.clone()).collect();
        let (added, removed) = diffmode::stats(&diff);
        DiffMode {
            flat,
            kinds,
            file_starts,
            new_paths,
            file_count: diff.files.len(),
            added,
            removed,
            cursor: 0,
            scroll: 0,
            viewport: 1,
            status: None,
            refine: HashMap::new(),
        }
    }

    fn max_line(&self) -> usize {
        self.flat.len().saturating_sub(1)
    }

    fn move_cursor(&mut self, delta: isize) {
        if self.flat.is_empty() {
            return;
        }
        let max = self.max_line() as isize;
        self.cursor = (self.cursor as isize + delta).clamp(0, max) as usize;
    }

    /// The ordinal of the file the cursor is currently inside.
    fn current_file(&self) -> Option<usize> {
        if self.file_starts.is_empty() {
            return None;
        }
        let mut ord = 0;
        for (k, &start) in self.file_starts.iter().enumerate() {
            if start <= self.cursor {
                ord = k;
            } else {
                break;
            }
        }
        Some(ord)
    }

    fn next_file(&mut self) {
        if let Some(&s) = self.file_starts.iter().find(|&&s| s > self.cursor) {
            self.cursor = s;
        }
    }

    fn prev_file(&mut self) {
        if let Some(&s) = self.file_starts.iter().rev().find(|&&s| s < self.cursor) {
            self.cursor = s;
        }
    }

    /// diff-goto-source: return a callback opening the current file's new path,
    /// or set a status message if there is nothing to visit.
    fn goto_source(&mut self) -> Option<Callback> {
        let path = self
            .current_file()
            .and_then(|ord| self.new_paths.get(ord))
            .cloned();
        let path = match path {
            Some(p) if !p.is_empty() && p != "/dev/null" => p,
            _ => {
                self.status = Some("diff: no source file at point".to_string());
                return None;
            }
        };
        let pb = PathBuf::from(&path);
        if !pb.exists() {
            self.status = Some(format!("diff: file not on disk: {path}"));
            return None;
        }
        Some(Box::new(
            move |compositor: &mut Compositor, cx: &mut Context| {
                compositor.pop();
                if let Err(err) = cx.editor.open(&pb, Action::Replace) {
                    cx.editor
                        .set_error(format!("failed to open {}: {err}", pb.display()));
                }
            },
        ))
    }

    /// The `[start, end)` flat range of the hunk the cursor sits in: from the
    /// `@@` header at or above point up to (but not including) the next header
    /// or file banner. `None` when point is not inside a hunk, which is emacs's
    /// `diff--some-hunks-p` guard failing.
    fn hunk_bounds(&self) -> Option<(usize, usize)> {
        if self.flat.is_empty() {
            return None;
        }
        let beg = self.kinds[..=self.cursor.min(self.max_line())]
            .iter()
            .rposition(|k| *k == LineKind::HunkHeader)?;
        let end = self.kinds[beg + 1..]
            .iter()
            .position(|k| {
                matches!(
                    k,
                    LineKind::HunkHeader | LineKind::FileHeader | LineKind::Header
                )
            })
            .map_or(self.flat.len(), |off| beg + 1 + off);
        Some((beg, end))
    }

    /// Collect a refinement region: the bodies of `range`'s lines (the leading
    /// `-`/`+` glyph dropped, which is what emacs's `diff-refine-preproc` achieves
    /// by rewriting `+` to `-`), newline-terminated and concatenated. Returns the
    /// region's chars plus, per line, its flat index and slice of the region.
    fn refine_region(&self, range: Range<usize>) -> (Vec<char>, Vec<(usize, Range<usize>)>) {
        let mut chars: Vec<char> = Vec::new();
        let mut spans: Vec<(usize, Range<usize>)> = Vec::new();
        for idx in range {
            let start = chars.len();
            chars.extend(self.flat[idx].text.chars().skip(1));
            spans.push((idx, start..chars.len()));
            chars.push('\n');
        }
        (chars, spans)
    }

    /// Turn per-character change flags back into this line's render runs, keeping
    /// the leading glyph unemphasised.
    fn store_refinement(&mut self, spans: &[(usize, Range<usize>)], changed: &[bool]) {
        for (idx, span) in spans {
            let mut chars = self.flat[*idx].text.chars();
            let mut runs: Vec<(String, bool)> = Vec::new();
            if let Some(glyph) = chars.next() {
                runs.push((glyph.to_string(), false));
            }
            for (c, emph) in chars.zip(changed[span.clone()].iter().copied()) {
                match runs.last_mut() {
                    Some((prev, prev_emph)) if *prev_emph == emph => prev.push(c),
                    _ => runs.push((c.to_string(), emph)),
                }
            }
            self.refine.insert(*idx, runs);
        }
    }

    /// diff-refine-hunk: highlight the changes of the hunk at point at a finer
    /// granularity. Ports `diff--refine-hunk`'s unified-diff arm: every maximal
    /// run of `-` lines that is immediately followed by a run of `+` lines is
    /// refined against it as a whole region. A `-` run with no `+` after it (a
    /// pure deletion) and a bare `+` run (a pure insertion) get nothing, since
    /// `diff-refine-nonmodified` defaults to nil.
    fn refine_hunk(&mut self) {
        let Some((beg, end)) = self.hunk_bounds() else {
            self.status = Some("diff: no hunk at point".to_string());
            return;
        };
        // Emacs removes the hunk's existing `fine` overlays before re-refining.
        for i in beg..end {
            self.refine.remove(&i);
        }
        let mut i = beg;
        while i < end {
            if self.kinds[i] != LineKind::Removed {
                i += 1;
                continue;
            }
            let del = i;
            while i < end && self.kinds[i] == LineKind::Removed {
                i += 1;
            }
            let del_end = i;
            while i < end && self.kinds[i] == LineKind::Added {
                i += 1;
            }
            if i == del_end {
                continue;
            }
            let (old, old_spans) = self.refine_region(del..del_end);
            let (new, new_spans) = self.refine_region(del_end..i);
            let (old_changed, new_changed) = refine_regions(&old, &new);
            self.store_refinement(&old_spans, &old_changed);
            self.store_refinement(&new_spans, &new_changed);
        }
    }
}

/// Chop a refinement region into the "atomic elements" emacs compares, matching
/// `smerge--refine-forward`'s regexp
/// `[[:upper:]]?[[:lower:]]+\|[[:upper:]]+\|[[:digit:]]+\|.\|\n`: a camel-cased
/// word, an all-caps run, a digit run, or any single character (newlines and
/// white space included — `smerge-refine-ignore-whitespace` only takes effect
/// when the weight hack is off, and it is on by default). Returns char ranges,
/// which concatenated cover the region exactly.
fn refine_tokens(chars: &[char]) -> Vec<Range<usize>> {
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let start = i;
        let lowers = |from: usize| {
            let mut j = from;
            while j < chars.len() && chars[j].is_lowercase() {
                j += 1;
            }
            j
        };
        if chars[i].is_uppercase() && chars.get(i + 1).is_some_and(|c| c.is_lowercase()) {
            i = lowers(i + 1);
        } else if chars[i].is_lowercase() {
            i = lowers(i);
        } else if chars[i].is_uppercase() {
            while i < chars.len() && chars[i].is_uppercase() {
                i += 1;
            }
        } else if chars[i].is_ascii_digit() {
            while i < chars.len() && chars[i].is_ascii_digit() {
                i += 1;
            }
        } else {
            i += 1;
        }
        tokens.push(start..i);
    }
    tokens
}

/// Refine two regions against each other, returning one change flag per input
/// character on each side (`diff-refine-removed` on the left,
/// `diff-refine-added` on the right).
///
/// `smerge-refine-weight-hack` is on by default, so emacs hands diff one copy of
/// each token per character the token holds — long symbols then cost
/// proportionally more to add or remove. Repeating the element in the diff input
/// reproduces that weighting exactly.
fn refine_regions(old: &[char], new: &[char]) -> (Vec<bool>, Vec<bool>) {
    let weigh = |chars: &[char], tokens: &[Range<usize>]| -> (Vec<String>, Vec<usize>) {
        let mut elements = Vec::new();
        let mut owner = Vec::new();
        for (t, range) in tokens.iter().enumerate() {
            let text: String = chars[range.clone()].iter().collect();
            for _ in 0..range.len() {
                elements.push(text.clone());
                owner.push(t);
            }
        }
        (elements, owner)
    };
    let old_tokens = refine_tokens(old);
    let new_tokens = refine_tokens(new);
    let (old_elements, old_owner) = weigh(old, &old_tokens);
    let (new_elements, new_owner) = weigh(new, &new_tokens);

    let mut input: InternedInput<String> = InternedInput::default();
    input.update_before(old_elements.iter().cloned());
    input.update_after(new_elements.iter().cloned());
    let diff = Diff::compute(Algorithm::Myers, &input);

    // A token is changed when any of its weighted copies falls in a diff hunk;
    // every character of a changed token is highlighted.
    let mut old_changed = vec![false; old.len()];
    let mut new_changed = vec![false; new.len()];
    let mark =
        |elements: Range<usize>, owner: &[usize], tokens: &[Range<usize>], out: &mut Vec<bool>| {
            for &t in &owner[elements] {
                out[tokens[t].clone()].fill(true);
            }
        };
    for hunk in diff.hunks() {
        mark(
            hunk.before.start as usize..hunk.before.end as usize,
            &old_owner,
            &old_tokens,
            &mut old_changed,
        );
        mark(
            hunk.after.start as usize..hunk.after.end as usize,
            &new_owner,
            &new_tokens,
            &mut new_changed,
        );
    }
    (old_changed, new_changed)
}

impl Component for DiffMode {
    fn handle_event(&mut self, event: &Event, _cx: &mut Context) -> EventResult {
        let key = match event {
            Event::Key(key) => *key,
            _ => return EventResult::Ignored(None),
        };
        let close: Callback = Box::new(|compositor: &mut Compositor, _cx| {
            compositor.pop();
        });
        let page = self.viewport.max(1) as isize;
        self.status = None;
        match key {
            key!('q') | key!(Esc) | ctrl!('c') => return EventResult::Consumed(Some(close)),
            key!('j') | key!(Down) | ctrl!('n') => self.move_cursor(1),
            key!('k') | key!(Up) | ctrl!('p') => self.move_cursor(-1),
            ctrl!('d') | key!(PageDown) => self.move_cursor(page),
            ctrl!('u') | key!(PageUp) => self.move_cursor(-page),
            key!('g') | key!(Home) => self.cursor = 0,
            key!('G') | key!(End) => self.cursor = self.max_line(),
            key!('n') => {
                if let Some(i) = diffmode::next_hunk_line(&self.kinds, self.cursor) {
                    self.cursor = i;
                }
            }
            key!('p') => {
                if let Some(i) = diffmode::prev_hunk_line(&self.kinds, self.cursor) {
                    self.cursor = i;
                }
            }
            key!('r') => self.refine_hunk(),
            key!('}') | alt!('n') => self.next_file(),
            key!('{') | alt!('p') => self.prev_file(),
            key!(Enter) | key!('o') => {
                if let Some(cb) = self.goto_source() {
                    return EventResult::Consumed(Some(cb));
                }
            }
            _ => {}
        }
        // Stay modal: never leak keys to the editor behind us.
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        let theme = &ctx.editor.theme;
        let mut bg = theme.get("ui.background");
        // `transparent-background`: drop the panel fill so the terminal shows
        // through, matching the editor surface and the rest of the IDE.
        if ctx.editor.config().transparent_background {
            bg.bg = None;
        }
        let header_style = theme.get("ui.text.focus");
        let text_style = theme.get("ui.text");
        let info_style = theme.get("ui.linenr");
        let sel_style = theme.get("ui.selection");
        let plus_style = theme.get("diff.plus");
        let minus_style = theme
            .try_get("diff.minus")
            .unwrap_or_else(|| theme.get("error"));
        let hunk_style = theme.get("function");

        surface.clear_with(area, bg);
        if area.width < 8 || area.height < 3 {
            return;
        }
        let width = area.width as usize;

        // Header: "Diff  N files  +A −R".
        let title = format!(
            "Diff  {} file{}  +{} −{}",
            self.file_count,
            if self.file_count == 1 { "" } else { "s" },
            self.added,
            self.removed
        );
        surface.set_stringn(area.x, area.y, &title, width, header_style);

        let body_y = area.y + 2;
        let body_h = area.height.saturating_sub(3);
        self.viewport = body_h.max(1) as usize;

        if let Some(msg) = &self.status {
            surface.set_stringn(area.x, area.y + 1, msg, width, minus_style);
        }

        if self.flat.is_empty() {
            surface.set_stringn(area.x, body_y, "(empty diff)", width, info_style);
            return;
        }

        // Keep the cursor in view.
        if self.cursor < self.scroll {
            self.scroll = self.cursor;
        } else if self.cursor >= self.scroll + self.viewport {
            self.scroll = self.cursor + 1 - self.viewport;
        }

        for (offset, line) in self
            .flat
            .iter()
            .enumerate()
            .skip(self.scroll)
            .take(body_h as usize)
        {
            let y = body_y + (offset - self.scroll) as u16;
            let style = if offset == self.cursor {
                sel_style
            } else {
                match line.kind {
                    LineKind::Added => plus_style,
                    LineKind::Removed => minus_style,
                    LineKind::FileHeader => header_style,
                    LineKind::HunkHeader => hunk_style,
                    LineKind::Header => info_style,
                    LineKind::Context => text_style,
                }
            };
            match self.refine.get(&offset) {
                // A refined line is drawn run by run so the fine-grained changes
                // stand out against the line's own added/removed colour.
                Some(runs) => {
                    let emph = style
                        .add_modifier(Modifier::REVERSED)
                        .add_modifier(Modifier::BOLD);
                    let mut x = area.x;
                    for (text, emphasised) in runs {
                        let left = width.saturating_sub((x - area.x) as usize);
                        if left == 0 {
                            break;
                        }
                        x = surface
                            .set_stringn(x, y, text, left, if *emphasised { emph } else { style })
                            .0;
                    }
                }
                None => {
                    surface.set_stringn(area.x, y, &line.text, width, style);
                }
            }
        }

        // Footer: keys.
        let footer = "j/k line  C-d/C-u page  n/p hunk  {/} file  r refine  Enter open  q quit";
        surface.set_stringn(area.x, area.y + area.height - 1, footer, width, info_style);
    }
}
