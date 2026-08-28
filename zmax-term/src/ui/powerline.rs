//! The vim-airline powerline status bar: `❮mode❯❮+❯❮⎇ branch❯❮path❯ … ❮ft❯❮enc❯❮pos❯`.
//!
//! One full-width row at the bottom of the frame, drawn in both the IDE workbench
//! (where [`crate::ui::ide::Ide`] feeds it the snapshot it already keeps for its
//! panels) and in the plain editor (where [`snapshot`] reads the focused view
//! directly). The renderer is shared so the bar looks identical in both.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tui::buffer::Buffer as Surface;
use zmax_view::{
    graphics::{Color, Modifier, Rect, Style},
    Editor,
};

/// Terminal display width of a string — the same model `Surface::set_stringn`
/// advances by, so segment layout and drawing stay in lockstep.
fn disp_width(s: &str) -> u16 {
    use zmax_core::unicode::width::UnicodeWidthStr;
    s.width() as u16
}

/// Everything the bar draws, snapshotted once per frame.
#[derive(Default, Clone)]
pub struct Status {
    /// 0 Normal, 1 Select/Visual, 2 Insert.
    pub mode: u8,
    pub modified: bool,
    pub branch: String,
    pub path: String,
    /// 1-based slot of the current file among the project's harpoon marks.
    pub harpoon_slot: Option<usize>,
    pub harpoon_total: usize,
    pub carets: usize,
    /// Total selected chars across all ranges.
    pub sel: usize,
    /// Lines touched by the non-empty ranges.
    pub sel_lines: usize,
    pub lang: String,
    pub encoding: String,
    /// Scroll position through the buffer, 0..=100.
    pub pct: u16,
    /// 1-based (line, column) of the primary cursor.
    pub lncol: (usize, usize),
}

/// Per-frame state that costs syscalls: the git branch (walks up to `.git/HEAD`)
/// and the harpoon marks (read from the mark store, each path stat'd). The IDE
/// refreshes both on its own cadence; outside it the bar is the only caller, so
/// they are cached here and refreshed when the directory changes or the entry
/// goes stale.
struct Cache {
    dir: Option<PathBuf>,
    branch: String,
    marks: Vec<PathBuf>,
    at: Option<Instant>,
}

static CACHE: Mutex<Cache> = Mutex::new(Cache {
    dir: None,
    branch: String::new(),
    marks: Vec::new(),
    at: None,
});

/// How long a cached branch/marks reading stays good for. Long enough that a
/// redraw storm doesn't hit the filesystem, short enough that a branch switch
/// or a new mark shows up without a keystroke.
const CACHE_TTL: Duration = Duration::from_millis(800);

/// Current git branch for `start`: walk up to a `.git`, read `HEAD`. Returns the short branch name
/// (or a 7-char hash for a detached HEAD). Cheap enough to call when the active directory changes.
pub fn git_branch(start: &Path) -> Option<String> {
    let mut cur = Some(start);
    while let Some(dir) = cur {
        let head = dir.join(".git").join("HEAD");
        if let Ok(content) = std::fs::read_to_string(&head) {
            let t = content.trim();
            return Some(match t.strip_prefix("ref: refs/heads/") {
                Some(branch) => branch.to_string(),
                None => t.chars().take(7).collect(),
            });
        }
        cur = dir.parent();
    }
    None
}

/// Cached `(branch, harpoon marks)` for `dir`, refreshed on a directory change
/// or once the previous reading goes stale.
fn branch_and_marks(dir: &Path) -> (String, Vec<PathBuf>) {
    let mut cache = match CACHE.lock() {
        Ok(cache) => cache,
        // A poisoned lock only means some other frame panicked mid-refresh; the
        // bar is decoration, so fall back to reading through.
        Err(poisoned) => poisoned.into_inner(),
    };
    let stale = cache
        .at
        .is_none_or(|at| at.elapsed() > CACHE_TTL || cache.dir.as_deref() != Some(dir));
    if stale {
        cache.branch = git_branch(dir).unwrap_or_default();
        cache.marks = crate::harpoon::list();
        cache.dir = Some(dir.to_path_buf());
        cache.at = Some(Instant::now());
    }
    (cache.branch.clone(), cache.marks.clone())
}

/// Snapshot the focused view for the plain-editor bar. Returns `None` when there
/// is no current view to read (transiently during startup / session restore).
pub fn snapshot(editor: &Editor) -> Option<Status> {
    let view = editor.tree.try_get(editor.tree.focus)?;
    let doc = editor.document(view.doc)?;

    let text = doc.text().slice(..);
    let sel = doc.selection(view.id);
    let cursor = sel.primary().cursor(text);
    let line = text.char_to_line(cursor);
    let col = cursor - text.line_to_char(line);
    let total_lines = doc.text().len_lines();

    let dir = doc
        .path()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));
    let (branch, marks) = branch_and_marks(&dir);
    let harpoon_slot = doc.path().and_then(|p| {
        let cp = std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
        marks.iter().position(|m| *m == cp).map(|i| i + 1)
    });

    Some(Status {
        mode: match editor.mode() {
            zmax_view::document::Mode::Normal => 0,
            zmax_view::document::Mode::Select => 1,
            zmax_view::document::Mode::Insert => 2,
        },
        modified: doc.is_modified(),
        branch,
        path: doc.display_name().to_string(),
        harpoon_slot,
        harpoon_total: marks.len(),
        carets: sel.len(),
        sel: sel.ranges().iter().map(|r| r.len()).sum(),
        sel_lines: sel
            .ranges()
            .iter()
            .filter(|r| !r.is_empty())
            .map(|r| {
                let a = text.char_to_line(r.from());
                let b = text.char_to_line(r.to().saturating_sub(1).max(r.from()));
                b - a + 1
            })
            .sum(),
        lang: doc.language_name().unwrap_or("plain text").to_string(),
        encoding: doc.encoding().name().to_string(),
        pct: if total_lines <= 1 {
            0
        } else {
            ((line * 100) / (total_lines - 1)).min(100) as u16
        },
        lncol: (line + 1, col + 1),
    })
}

/// vim-airline powerline status bar: ❮mode❯❮paste❯❮⎇ branch❯❮path❯ … ❮ft❯❮enc❯❮pos❯.
/// The position pill, honoring Emacs's `line-number-mode` and
/// `column-number-mode` the same way the plain status line's `Position` element
/// does: both on gives `12:5`, `line-number-mode` alone `12`, `column-number-mode`
/// alone `:5`, and with both off the pill is dropped entirely. The scroll
/// percentage is a line-position construct, so it follows `line-number-mode`
/// alone — which is why the pill can still appear with only a percentage in it.
/// `ln_glyph` is the caller's line-number glyph. Pure — unit tested.
fn position_segment(
    st: &Status,
    ln_glyph: &str,
    line_number_mode: bool,
    column_number_mode: bool,
) -> Option<String> {
    let (ln, co) = st.lncol;
    let coords = match (line_number_mode, column_number_mode) {
        (true, true) => Some(format!("{ln}:{co}")),
        (true, false) => Some(ln.to_string()),
        (false, true) => Some(format!(":{co}")),
        (false, false) => None,
    };
    match (line_number_mode, coords) {
        (true, Some(coords)) => Some(format!(" {}%  {ln_glyph} {coords} ", st.pct)),
        (true, None) => Some(format!(" {}% ", st.pct)),
        (false, Some(coords)) => Some(format!(" {ln_glyph} {coords} ")),
        (false, None) => None,
    }
}

/// Segments are coloured pills joined by powerline separators ( / ), mode colour by Normal/
/// Insert/Visual, just like the classic airline theme.
pub fn render(surface: &mut Surface, theme: &zmax_view::Theme, area: Rect, st: &Status) {
    const SEP_R: &str = "\u{e0b0}"; //  solid right separator
    const SEP_L: &str = "\u{e0b2}"; //  solid left separator
    const GIT: &str = "\u{e0a0}"; //  branch glyph
    const LN: &str = "\u{e0a1}"; //  line-number glyph

    // Colours come from the active theme's statusline scopes; the RGB values are only
    // fallbacks for themes that leave a given scope unstyled.
    let bgfg = |style: Style, fb_bg: Color, fb_fg: Color| {
        (style.bg.unwrap_or(fb_bg), style.fg.unwrap_or(fb_fg))
    };
    let (mode_txt, mode_scope, fb_mode) = match st.mode {
        2 => (
            "INSERT",
            "ui.statusline.insert",
            Color::Rgb(0x00, 0xb3, 0xd7),
        ),
        1 => (
            "VISUAL",
            "ui.statusline.select",
            Color::Rgb(0xff, 0x8c, 0x00),
        ),
        _ => (
            "NORMAL",
            "ui.statusline.normal",
            Color::Rgb(0x9e, 0xd0, 0x10),
        ),
    };
    let blackfg = Color::Rgb(0x10, 0x12, 0x16);
    let (mode_bg, mode_fg) = bgfg(theme.get(mode_scope), fb_mode, blackfg);
    let (gray, grayfg) = bgfg(
        theme.get("ui.statusline"),
        Color::Rgb(0x45, 0x45, 0x4d),
        Color::Rgb(0xd2, 0xd2, 0xd8),
    );
    let (dark, darkfg) = bgfg(
        theme.get("ui.statusline.inactive"),
        Color::Rgb(0x28, 0x28, 0x2f),
        Color::Rgb(0x9c, 0x9c, 0xa6),
    );
    let warn = theme
        .get("warning")
        .fg
        .unwrap_or(Color::Rgb(0x7a, 0xa8, 0x10));
    let fill = theme
        .get("ui.statusline")
        .bg
        .unwrap_or(Color::Rgb(0x1b, 0x1b, 0x20));
    let seg = |bg: Color, fg: Color| Style::default().bg(bg).fg(fg);

    surface.clear_with(area, seg(fill, darkfg));
    let bold = Modifier::BOLD;

    // ── left segments ──────────────────────────────────────────────
    let mut left: Vec<(String, Style)> = Vec::new();
    left.push((
        format!(" {mode_txt} "),
        seg(mode_bg, mode_fg).add_modifier(bold),
    ));
    if st.modified {
        // airline's secondary section (where PASTE/spell live) — here: modified flag
        left.push((" + ".to_string(), seg(warn, mode_fg).add_modifier(bold)));
    }
    if !st.branch.is_empty() {
        left.push((format!(" {GIT} {} ", st.branch), seg(gray, grayfg)));
    }
    if !st.path.is_empty() {
        left.push((format!(" {} ", st.path), seg(dark, darkfg)));
    }

    // ── right segments (display order left→right) ──────────────────
    let mut right: Vec<(String, Style)> = Vec::new();
    if st.harpoon_total > 0 {
        let label = match st.harpoon_slot {
            Some(n) => format!(" ⚓ {}/{} ", n, st.harpoon_total),
            None => format!(" ⚓ {} ", st.harpoon_total),
        };
        right.push((label, seg(gray, grayfg)));
    }
    // selection / multi-caret stats (only when meaningful)
    if st.carets > 1 {
        right.push((
            format!(" {} ⌶ ", st.carets),
            seg(warn, mode_fg).add_modifier(bold),
        ));
    } else if st.mode == 1 && st.sel > 0 {
        let lines = st.sel_lines.max(1);
        right.push((
            format!(" {}L {} sel ", lines, st.sel),
            seg(warn, mode_fg).add_modifier(bold),
        ));
    }
    if !st.lang.is_empty() {
        right.push((format!(" {} ", st.lang), seg(dark, darkfg)));
    }
    if !st.encoding.is_empty() {
        right.push((format!(" {} ", st.encoding), seg(gray, grayfg)));
    }
    if let Some(text) = position_segment(
        st,
        LN,
        crate::commands::line_number_mode_enabled(),
        crate::ui::statusline::column_number_mode_enabled(),
    ) {
        right.push((text, seg(mode_bg, mode_fg).add_modifier(bold)));
    }

    let right_edge = area.x + area.width;

    // draw left, separators point right () into the next segment's bg
    let mut x = area.x;
    for i in 0..left.len() {
        let (text, style) = &left[i];
        if x >= right_edge {
            break;
        }
        let avail = (right_edge - x) as usize;
        surface.set_stringn(x, area.y, text, avail, *style);
        x += disp_width(text).min(right_edge - x);
        if x >= right_edge {
            break;
        }
        let next_bg = left.get(i + 1).and_then(|(_, s)| s.bg).unwrap_or(fill);
        surface.set_stringn(
            x,
            area.y,
            SEP_R,
            1,
            Style::default().fg(style.bg.unwrap_or(fill)).bg(next_bg),
        );
        x += 1;
    }

    // draw right→left, separators point left () with the segment's bg as fg
    let mut rx = right_edge;
    for i in (0..right.len()).rev() {
        let (text, style) = &right[i];
        let w = disp_width(text);
        if rx <= x + w {
            break; // would collide with the left cluster
        }
        rx -= w;
        surface.set_stringn(rx, area.y, text, w as usize, *style);
        if rx <= x {
            break;
        }
        rx -= 1;
        let left_bg = if i == 0 {
            fill
        } else {
            right[i - 1].1.bg.unwrap_or(fill)
        };
        surface.set_stringn(
            rx,
            area.y,
            SEP_L,
            1,
            Style::default().fg(style.bg.unwrap_or(fill)).bg(left_bg),
        );
    }
}

#[cfg(test)]
mod test {
    use super::*;

    /// The row's text, as drawn.
    fn draw(width: u16, st: &Status) -> String {
        let area = Rect::new(0, 0, width, 1);
        let mut surface = Surface::empty(area);
        render(&mut surface, &zmax_view::Theme::default(), area, st);
        (0..width)
            .filter_map(|x| surface.get(x, 0).map(|cell| cell.symbol.to_string()))
            .collect()
    }

    #[test]
    fn the_position_pill_follows_line_and_column_number_mode() {
        let mut st = status();
        st.lncol = (12, 5);
        st.pct = 40;
        // Emacs `mode-line-position`, all four cases. The percentage is a
        // line-position construct, so it goes with `line-number-mode`.
        assert_eq!(
            position_segment(&st, "L", true, true).as_deref(),
            Some(" 40%  L 12:5 ")
        );
        assert_eq!(
            position_segment(&st, "L", true, false).as_deref(),
            Some(" 40%  L 12 ")
        );
        assert_eq!(
            position_segment(&st, "L", false, true).as_deref(),
            Some(" L :5 ")
        );
        // Both off: the whole pill goes, as the plain status line drops its
        // `Position` element.
        assert_eq!(position_segment(&st, "L", false, false), None);
    }

    fn status() -> Status {
        Status {
            mode: 2,
            modified: true,
            branch: "main".into(),
            path: "src/main.rs".into(),
            lang: "rust".into(),
            encoding: "UTF-8".into(),
            pct: 42,
            lncol: (12, 5),
            carets: 1,
            ..Status::default()
        }
    }

    #[test]
    fn segments_are_joined_by_powerline_separators() {
        let row = draw(120, &status());
        // Left cluster: mode, modified flag, branch, path — in that order, each
        // followed by the right-pointing separator that carries its colour into
        // the next pill.
        let left = row.find("INSERT").unwrap();
        let modified = row.find(" + ").unwrap();
        let branch = row.find("\u{e0a0} main").unwrap();
        let path = row.find("src/main.rs").unwrap();
        assert!(left < modified && modified < branch && branch < path);
        assert!(
            row.contains("\u{e0b0}"),
            "no right separator drawn: {row:?}"
        );
        // Right cluster, drawn right-to-left, is joined by left-pointing separators.
        let lang = row.find(" rust ").unwrap();
        let enc = row.find("UTF-8").unwrap();
        let pos = row.find("\u{e0a1} 12:5").unwrap();
        assert!(path < lang && lang < enc && enc < pos);
        assert!(row.contains("42%"), "no scroll percentage: {row:?}");
        assert!(row.contains("\u{e0b2}"), "no left separator drawn: {row:?}");
    }

    #[test]
    fn right_cluster_stops_before_it_collides_with_the_left() {
        // Wide enough for the left cluster plus the position pill, not for the
        // encoding behind it: the segments that would overlap the left cluster are
        // dropped, not wrapped, and nothing is drawn past the row.
        let row = draw(55, &status());
        assert_eq!(row.chars().count(), 55);
        assert!(row.contains("\u{e0a1} 12:5"), "position dropped: {row:?}");
        assert!(!row.contains("UTF-8"), "encoding should not fit: {row:?}");
        // Narrower still: the whole right cluster goes, the left cluster stays.
        let row = draw(40, &status());
        assert_eq!(row.chars().count(), 40);
        assert!(row.contains("src/main.rs"), "path dropped: {row:?}");
        assert!(!row.contains("12:5"), "position should not fit: {row:?}");
    }

    #[test]
    fn mode_label_tracks_the_editor_mode() {
        for (mode, label) in [(0u8, "NORMAL"), (1, "VISUAL"), (2, "INSERT")] {
            let row = draw(120, &Status { mode, ..status() });
            assert!(row.contains(label), "mode {mode} drew {row:?}");
        }
    }

    #[test]
    fn selection_stats_only_show_where_they_mean_something() {
        // Multiple carets win over the selection readout.
        let row = draw(
            120,
            &Status {
                carets: 3,
                mode: 1,
                sel: 9,
                sel_lines: 2,
                ..status()
            },
        );
        assert!(row.contains("3 \u{2336}"), "caret count missing: {row:?}");
        assert!(!row.contains("sel"), "selection shown too: {row:?}");
        // A single caret in select mode shows the selected lines/chars instead.
        let row = draw(
            120,
            &Status {
                carets: 1,
                mode: 1,
                sel: 9,
                sel_lines: 2,
                ..status()
            },
        );
        assert!(row.contains("2L 9 sel"), "selection missing: {row:?}");
        // In normal mode neither is drawn.
        let row = draw(
            120,
            &Status {
                carets: 1,
                mode: 0,
                sel: 9,
                sel_lines: 2,
                ..status()
            },
        );
        assert!(!row.contains("sel"), "selection shown in normal: {row:?}");
    }

    #[test]
    fn harpoon_pill_shows_the_slot_only_when_the_file_is_marked() {
        let marked = draw(
            120,
            &Status {
                harpoon_slot: Some(2),
                harpoon_total: 4,
                ..status()
            },
        );
        // The anchor is double-width, so the cell after it is the surface's filler;
        // anchor the assertions on the text that follows the glyph.
        let tail = |row: &str| row[row.find('\u{2693}').expect("no anchor pill")..].to_string();
        assert!(tail(&marked).starts_with("\u{2693}  2/4 "), "{marked:?}");
        let unmarked = draw(
            120,
            &Status {
                harpoon_slot: None,
                harpoon_total: 4,
                ..status()
            },
        );
        assert!(tail(&unmarked).starts_with("\u{2693}  4 "), "{unmarked:?}");
        // No marks in the project: no pill at all.
        let none = draw(120, &status());
        assert!(!none.contains('\u{2693}'), "{none:?}");
    }
}
