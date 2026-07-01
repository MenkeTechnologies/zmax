//! `xxd`-style hex viewer & **editor** (slice 2: byte-faithful overwrite editing).
//!
//! A full-screen overlay [`Component`] that shows a file's **raw bytes** as a
//! classic hex dump: an offset gutter, 16 hex bytes per row grouped 8 + 8, and
//! an ASCII gutter. Opened with the `:hex` typable command.
//!
//! The view is backed by a plain [`Vec<u8>`] (read with [`std::fs::read`]) — not
//! the editor's text [`Rope`](zemacs_core::Rope) — so arbitrary, non-UTF-8 bytes
//! are shown and *written back* faithfully.
//!
//! ## Editing (slice 2)
//!
//! The editor opens in read-only **nav** mode (slice-1 keys). `i`/`R` enters
//! **EDIT** mode; `Esc` leaves it. `Tab` toggles the focused column between Hex
//! and Ascii in either mode.
//!
//! * EDIT + Hex: a hex digit `[0-9a-fA-F]` sets the **high** nibble of the byte
//!   under the cursor (recording a "pending high nibble"); the next digit sets
//!   the **low** nibble and advances to the next byte.
//! * EDIT + Ascii: a printable char (`0x20..=0x7e`) overwrites the byte and
//!   advances.
//!
//! Editing is **overwrite-only** — the file length never changes in this slice.
//! Any change marks the buffer dirty (`[+]` in the header).
//!
//! ## Saving
//!
//! `Ctrl-s` writes the current bytes to the file path via [`std::fs::write`]
//! (byte-faithful). Quitting (`q`/`Esc` in nav mode) with unsaved edits is
//! guarded: the first `q` warns, a second `q` discards and closes.
//!
//! Slice-1 nav keys still work in nav mode; in EDIT mode the arrow keys / Home /
//! End / PageUp / PageDown still navigate while printable keys edit.

use std::path::PathBuf;

use tui::buffer::Buffer as Surface;
use zemacs_view::graphics::Rect;
use zemacs_view::input::MouseEventKind;
use zemacs_view::keyboard::KeyModifiers;

use crate::{
    compositor::{Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

/// Number of bytes shown per row.
const BYTES_PER_ROW: usize = 16;

/// Which column edits / cursor highlighting are focused on.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Column {
    Hex,
    Ascii,
}

impl Column {
    /// The other column.
    fn toggled(self) -> Column {
        match self {
            Column::Hex => Column::Ascii,
            Column::Ascii => Column::Hex,
        }
    }
}

/// The full-screen hex viewer / editor overlay.
pub struct HexView {
    /// Display name of the file (shown in the header).
    file_name: String,
    /// Absolute path to write on save; `None` when opened from bytes only.
    path: Option<PathBuf>,
    /// The raw file bytes — the source of truth for everything rendered.
    bytes: Vec<u8>,
    /// Index of the byte under the cursor (`0` when the file is empty).
    cursor: usize,
    /// Index of the top visible row (each row is [`BYTES_PER_ROW`] bytes).
    scroll: usize,
    /// Number of body rows visible in the last render (for page scrolling and
    /// keeping the cursor on screen). Updated every frame.
    viewport: usize,
    /// `true` once `i`/`R` has entered EDIT mode; `false` in read-only nav.
    edit_mode: bool,
    /// The focused column (Hex or Ascii) for editing and cursor highlight.
    focus: Column,
    /// In Hex EDIT mode, the high nibble already typed for the cursor byte, if a
    /// second (low-nibble) digit is awaited.
    pending_high: Option<u8>,
    /// `true` once any byte has been overwritten since the last save.
    dirty: bool,
    /// `true` once a dirty-quit has been warned; a second `q` then discards.
    quit_armed: bool,
    /// A transient notice shown in the header (save confirmation, quit warning).
    /// Rendered *inside* the overlay because the editor statusline is hidden
    /// behind it. Cleared on the next keypress.
    message: Option<String>,
    /// The pane rect the viewer last rendered into. Mouse events outside it are
    /// passed through so the surrounding IDE chrome stays usable.
    last_area: Rect,
}

impl HexView {
    /// Construct a viewer over `bytes`, labelled `file_name`, optionally backed
    /// by `path` (so it can be saved). Pass `None` to open from bytes only.
    pub fn new(file_name: String, path: Option<PathBuf>, bytes: Vec<u8>) -> Self {
        HexView {
            file_name,
            path,
            bytes,
            cursor: 0,
            scroll: 0,
            viewport: 1,
            edit_mode: false,
            focus: Column::Hex,
            pending_high: None,
            dirty: false,
            quit_armed: false,
            message: None,
            last_area: Rect::default(),
        }
    }

    /// Total number of rows needed to show every byte (at least 1 so an empty
    /// file still draws a blank body).
    fn total_rows(&self) -> usize {
        self.bytes.len().div_ceil(BYTES_PER_ROW).max(1)
    }

    /// Largest valid top-row scroll offset.
    fn max_scroll(&self) -> usize {
        self.total_rows().saturating_sub(self.viewport)
    }

    /// Scroll the viewport by `delta` rows, clamped to `[0, max_scroll]`.
    fn scroll_by(&mut self, delta: isize) {
        let next = self.scroll as isize + delta;
        self.scroll = next.clamp(0, self.max_scroll() as isize) as usize;
    }

    /// Move the cursor to byte `idx` (clamped to a valid byte) and scroll so it
    /// stays visible. No-op on an empty file. Abandons any pending high nibble.
    fn move_to(&mut self, idx: isize) {
        self.pending_high = None;
        if self.bytes.is_empty() {
            return;
        }
        let max = self.bytes.len() as isize - 1;
        self.cursor = idx.clamp(0, max) as usize;
        self.ensure_cursor_visible();
    }

    /// Scroll so the cursor's row is within the viewport.
    fn ensure_cursor_visible(&mut self) {
        let row = self.cursor / BYTES_PER_ROW;
        if row < self.scroll {
            self.scroll = row;
        } else if row >= self.scroll + self.viewport {
            self.scroll = row + 1 - self.viewport;
        }
    }

    /// Apply a typed `ch` to the focused column, overwriting the cursor byte.
    /// In Hex focus only hex digits act (composing high then low nibble); in
    /// Ascii focus only printable bytes act. Marks dirty + advances on a change.
    fn type_char(&mut self, ch: char) {
        match self.focus {
            Column::Hex => {
                if let Some(digit) = hex_digit(ch) {
                    let (cursor, pending, changed) =
                        apply_hex_digit(&mut self.bytes, self.cursor, self.pending_high, digit);
                    self.cursor = cursor;
                    self.pending_high = pending;
                    if changed {
                        self.dirty = true;
                        self.ensure_cursor_visible();
                    }
                }
            }
            Column::Ascii => {
                let (cursor, changed) = apply_ascii_char(&mut self.bytes, self.cursor, ch);
                self.cursor = cursor;
                if changed {
                    self.dirty = true;
                    self.ensure_cursor_visible();
                }
            }
        }
    }

    /// Write the current bytes to `path` (byte-faithful). Reports both on the
    /// editor statusline *and* in the overlay's own header (the statusline is
    /// hidden behind this full-screen overlay, so the in-overlay notice is what
    /// the user actually sees).
    fn save(&mut self, cx: &mut Context) {
        let notice = match &self.path {
            Some(path) => match std::fs::write(path, &self.bytes) {
                Ok(()) => {
                    self.dirty = false;
                    format!("✔ wrote {} bytes to {}", self.bytes.len(), path.display())
                }
                Err(err) => format!("✘ save failed: {err}"),
            },
            None => "✘ can't save: opened from bytes, no file path".to_string(),
        };
        cx.editor.set_status(notice.clone());
        self.message = Some(notice);
    }
}

impl Component for HexView {
    fn handle_event(&mut self, event: &Event, cx: &mut Context) -> EventResult {
        let key = match event {
            Event::Key(key) => *key,
            Event::Mouse(ev) => {
                // Only act on mouse events inside our pane; let everything else
                // fall through so the surrounding IDE chrome (file tree, tabs,
                // toolbar, other splits) stays clickable/scrollable.
                let a = self.last_area;
                let inside = ev.column >= a.x
                    && ev.column < a.x + a.width
                    && ev.row >= a.y
                    && ev.row < a.y + a.height;
                if !inside {
                    return EventResult::Ignored(None);
                }
                match ev.kind {
                    MouseEventKind::ScrollDown => self.scroll_by(3),
                    MouseEventKind::ScrollUp => self.scroll_by(-3),
                    _ => {}
                }
                return EventResult::Consumed(None);
            }
            _ => return EventResult::Ignored(None),
        };

        // Quit-arming only survives consecutive `q` presses: clear it now, and
        // re-arm only in the quit branch below. The transient header notice is
        // likewise cleared on every key and re-set by save / the quit warning.
        let was_armed = self.quit_armed;
        self.quit_armed = false;
        self.message = None;

        let cursor = self.cursor as isize;
        let bpr = BYTES_PER_ROW as isize;
        // A screenful of bytes, used for page scrolling.
        let page = (self.viewport.max(1) * BYTES_PER_ROW) as isize;
        // Start of the cursor's current row, for `0`/`$`.
        let row_start = self.cursor - (self.cursor % BYTES_PER_ROW);

        // Keys handled identically in both modes: save and column toggle.
        match key {
            ctrl!('s') => {
                self.save(cx);
                return EventResult::Consumed(None);
            }
            key!(Tab) => {
                self.focus = self.focus.toggled();
                self.pending_high = None;
                return EventResult::Consumed(None);
            }
            _ => {}
        }

        if self.edit_mode {
            // EDIT mode: arrows still navigate; printable keys edit the focus.
            match key {
                key!(Esc) => {
                    self.edit_mode = false;
                    self.pending_high = None;
                }
                key!(Left) => self.move_to(cursor - 1),
                key!(Right) => self.move_to(cursor + 1),
                key!(Up) => self.move_to(cursor - bpr),
                key!(Down) => self.move_to(cursor + bpr),
                key!(Home) => self.move_to(row_start as isize),
                key!(End) => self.move_to((row_start + BYTES_PER_ROW - 1) as isize),
                key!(PageDown) | ctrl!('d') | ctrl!('f') => {
                    self.scroll_by(self.viewport.max(1) as isize);
                    self.move_to(cursor + page);
                }
                key!(PageUp) | ctrl!('u') | ctrl!('b') => {
                    self.scroll_by(-(self.viewport.max(1) as isize));
                    self.move_to(cursor - page);
                }
                _ => {
                    // A bare or shifted printable char edits the focused column.
                    if key.modifiers == KeyModifiers::NONE || key.modifiers == KeyModifiers::SHIFT {
                        if let Some(ch) = key.char() {
                            self.type_char(ch);
                        }
                    }
                }
            }
            return EventResult::Consumed(None);
        }

        // NAV mode (slice-1 keys, read-only).
        match key {
            key!('q') | key!(Esc) | ctrl!('c') => {
                if self.dirty && !was_armed {
                    self.quit_armed = true;
                    let warn = "unsaved edits — Ctrl-s to save, or press q again to discard";
                    cx.editor.set_status(warn);
                    self.message = Some(warn.to_string());
                    return EventResult::Consumed(None);
                }
                return EventResult::Consumed(Some(Box::new(
                    |compositor: &mut Compositor, _cx| {
                        compositor.pop();
                    },
                )));
            }
            key!('i') | key!('R') => self.edit_mode = true,
            key!('h') | key!(Left) => self.move_to(cursor - 1),
            key!('l') | key!(Right) => self.move_to(cursor + 1),
            key!('j') | key!(Down) => self.move_to(cursor + bpr),
            key!('k') | key!(Up) => self.move_to(cursor - bpr),
            key!('0') | key!(Home) => self.move_to(row_start as isize),
            key!('$') | key!(End) => self.move_to((row_start + BYTES_PER_ROW - 1) as isize),
            key!('g') => self.move_to(0),
            key!('G') => self.move_to(isize::MAX),
            key!(PageDown) | ctrl!('d') | ctrl!('f') => {
                self.scroll_by(self.viewport.max(1) as isize);
                self.move_to(cursor + page);
            }
            key!(PageUp) | ctrl!('u') | ctrl!('b') => {
                self.scroll_by(-(self.viewport.max(1) as isize));
                self.move_to(cursor - page);
            }
            _ => {}
        }
        // Stay modal: never leak keys to the editor behind us.
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        use crate::ui::rat::to_rat_style;
        use ratatui::text::{Line, Span};
        use ratatui::widgets::Paragraph;

        // Confine the viewer to the focused editor pane rather than the whole
        // terminal: in IDE mode this keeps the file tree, tabs, and bottom
        // drawer visible, and with splits it stays inside the current pane.
        // (EditorView underneath still paints the surrounding chrome.)
        let pane = ctx.editor.tree.get(ctx.editor.tree.focus).area;
        let area = area.intersection(pane);
        self.last_area = area;

        let theme = &ctx.editor.theme;
        let bg = theme.get("ui.background");
        let text_style = theme.get("ui.text");
        let linenr_style = theme.get("ui.linenr");
        let hex_style = theme.get("constant.numeric");
        let cursor_style = theme.get("ui.cursor");
        let title_style = theme.get("ui.text.focus");

        surface.clear_with(area, bg);
        if area.width < 8 || area.height < 3 {
            return;
        }

        // Mode / focus indicator for the header.
        let mode = match (self.edit_mode, self.focus) {
            (true, Column::Hex) => "-- EDIT (hex) --",
            (true, Column::Ascii) => "-- EDIT (ascii) --",
            (false, Column::Hex) => "NAV [hex]",
            (false, Column::Ascii) => "NAV [ascii]",
        };
        let dirty = if self.dirty { " [+]" } else { "" };

        // ── Header (two rows): title + byte count + mode, then a key hint ─────
        let header_h = 2u16;
        let header = format!(
            " {}{}  —  {} byte{}  ·  cursor 0x{:08x}  ·  {}{}",
            self.file_name,
            dirty,
            self.bytes.len(),
            if self.bytes.len() == 1 { "" } else { "s" },
            self.cursor,
            mode,
            if self.dirty { "  (modified)" } else { "" },
        );
        surface.set_stringn(
            area.x,
            area.y,
            &header,
            area.width as usize,
            title_style.add_modifier(zemacs_view::graphics::Modifier::BOLD),
        );
        // Row 2: a transient notice (save confirmation / quit warning) when one
        // is pending, otherwise the persistent key hint. The notice must live
        // here because the editor statusline is hidden behind this overlay.
        match &self.message {
            Some(msg) => {
                surface.set_stringn(
                    area.x,
                    area.y + 1,
                    &format!(" {msg}"),
                    area.width as usize,
                    title_style.add_modifier(zemacs_view::graphics::Modifier::BOLD),
                );
            }
            None => {
                let hint = if self.edit_mode {
                    " type to edit  ·  Tab hex/ascii  ·  arrows move  ·  ^s save  ·  Esc leave edit"
                } else {
                    " h/l/j/k move  ·  i edit  ·  Tab hex/ascii  ·  g/G start/end  ·  ^s save  ·  q quit"
                };
                surface.set_stringn(area.x, area.y + 1, hint, area.width as usize, linenr_style);
            }
        }

        let body_y = area.y + header_h;
        let body_h = area.height.saturating_sub(header_h);
        self.viewport = body_h as usize;
        if body_h == 0 {
            return;
        }

        // The "active" cursor highlight (the focused column) vs. the mirror
        // highlight on the unfocused column.
        let hex_cursor_style = if self.focus == Column::Hex {
            cursor_style
        } else {
            linenr_style
        };
        let ascii_cursor_style = if self.focus == Column::Ascii {
            cursor_style
        } else {
            linenr_style
        };

        // ── Body: one ratatui Line per visible row, with the cursor byte
        // highlighted in both the hex and ASCII columns ─────────────────────
        let mut lines: Vec<Line> = Vec::with_capacity(body_h as usize);
        for row in self.scroll..(self.scroll + body_h as usize) {
            let start = row * BYTES_PER_ROW;
            if start >= self.bytes.len() && !(self.bytes.is_empty() && row == 0) {
                lines.push(Line::default());
                continue;
            }
            let end = (start + BYTES_PER_ROW).min(self.bytes.len());
            let chunk = &self.bytes[start.min(self.bytes.len())..end];

            let mut spans: Vec<Span> = Vec::with_capacity(BYTES_PER_ROW * 2 + 4);
            // Offset gutter.
            spans.push(Span::styled(
                format!("{:08x}  ", start),
                to_rat_style(linenr_style),
            ));
            // Hex columns: 16 cells, grouped 8 + 8, cursor byte highlighted.
            for i in 0..BYTES_PER_ROW {
                if i == 8 {
                    spans.push(Span::styled(" ", to_rat_style(text_style)));
                }
                match chunk.get(i) {
                    Some(b) => {
                        let is_cursor = start + i == self.cursor;
                        let style = if is_cursor {
                            hex_cursor_style
                        } else {
                            hex_style
                        };
                        spans.push(Span::styled(format!("{:02x}", b), to_rat_style(style)));
                        spans.push(Span::styled(" ", to_rat_style(text_style)));
                    }
                    None => spans.push(Span::styled("   ", to_rat_style(text_style))),
                }
            }
            // ASCII gutter.
            spans.push(Span::styled("|", to_rat_style(linenr_style)));
            for i in 0..BYTES_PER_ROW {
                match chunk.get(i) {
                    Some(&b) => {
                        let ch = if (0x20..=0x7e).contains(&b) {
                            b as char
                        } else {
                            '.'
                        };
                        let is_cursor = start + i == self.cursor;
                        let style = if is_cursor {
                            ascii_cursor_style
                        } else {
                            text_style
                        };
                        spans.push(Span::styled(ch.to_string(), to_rat_style(style)));
                    }
                    None => spans.push(Span::styled(" ", to_rat_style(text_style))),
                }
            }
            spans.push(Span::styled("|", to_rat_style(linenr_style)));
            lines.push(Line::from(spans));
        }

        let body = Rect::new(area.x, body_y, area.width, body_h);
        crate::ui::rat::render(Paragraph::new(lines), body, surface);
    }

    fn id(&self) -> Option<&'static str> {
        Some("hex")
    }
}

/// Value of a hex-digit char `[0-9a-fA-F]` (`0..=15`), or `None`.
fn hex_digit(c: char) -> Option<u8> {
    c.to_digit(16).map(|d| d as u8)
}

/// Replace the **high** nibble of `b` with the low 4 bits of `digit`, keeping
/// the low nibble.
pub fn set_high_nibble(b: u8, digit: u8) -> u8 {
    (b & 0x0f) | ((digit & 0x0f) << 4)
}

/// Replace the **low** nibble of `b` with the low 4 bits of `digit`, keeping the
/// high nibble.
pub fn set_low_nibble(b: u8, digit: u8) -> u8 {
    (b & 0xf0) | (digit & 0x0f)
}

/// Apply one hex-digit edit (overwrite only). Returns
/// `(new_cursor, new_pending_high, changed)`.
///
/// * With no pending high nibble, sets the **high** nibble of the byte under
///   `cursor` and records `digit` as the pending high nibble (cursor unchanged).
/// * With a pending high nibble, sets the **low** nibble and advances the cursor
///   (clamped to the last byte).
///
/// Editing past the end of `bytes` (including an empty buffer) is a no-op.
fn apply_hex_digit(
    bytes: &mut [u8],
    cursor: usize,
    pending: Option<u8>,
    digit: u8,
) -> (usize, Option<u8>, bool) {
    if cursor >= bytes.len() {
        return (cursor, None, false);
    }
    match pending {
        None => {
            bytes[cursor] = set_high_nibble(bytes[cursor], digit);
            (cursor, Some(digit), true)
        }
        Some(_) => {
            bytes[cursor] = set_low_nibble(bytes[cursor], digit);
            let next = (cursor + 1).min(bytes.len() - 1);
            (next, None, true)
        }
    }
}

/// Apply one ASCII-char edit (overwrite only). Returns `(new_cursor, changed)`.
///
/// Only printable bytes (`0x20..=0x7e`) act; the byte under `cursor` is
/// overwritten and the cursor advances (clamped to the last byte). Editing past
/// the end of `bytes`, or a non-printable char, is a no-op.
fn apply_ascii_char(bytes: &mut [u8], cursor: usize, ch: char) -> (usize, bool) {
    if cursor >= bytes.len() || !(0x20u32..=0x7e).contains(&(ch as u32)) {
        return (cursor, false);
    }
    bytes[cursor] = ch as u8;
    let next = (cursor + 1).min(bytes.len() - 1);
    (next, true)
}

/// Format one hex-dump row. Pure (no editor state) so the layout is unit-tested.
///
/// `offset` is the byte offset of the first byte in `chunk`; `chunk` holds up to
/// [`BYTES_PER_ROW`] bytes (a short final row is allowed). Returns:
///
/// * the **left** column — the `{offset:08x}` gutter, two spaces, then 16 hex
///   cells grouped 8 + 8 (`"7f 45 …  00 01 …"`); a short row pads its missing
///   cells with spaces so the column width (and therefore the ASCII gutter that
///   follows it) stays aligned with full rows.
/// * the **ASCII** column — exactly 16 characters: each printable byte
///   (`0x20..=0x7e`) as itself, any other byte as `.`, and missing trailing
///   bytes as spaces.
///
/// The caller wraps the ASCII column in `|…|` and applies cursor highlighting;
/// this helper only produces the text so the formatting can be tested directly.
pub fn hex_row(offset: usize, chunk: &[u8]) -> (String, String) {
    let mut hex = format!("{:08x}  ", offset);
    let mut ascii = String::with_capacity(BYTES_PER_ROW);
    for i in 0..BYTES_PER_ROW {
        if i == 8 {
            hex.push(' ');
        }
        match chunk.get(i) {
            Some(&b) => {
                hex.push_str(&format!("{:02x} ", b));
                ascii.push(if (0x20..=0x7e).contains(&b) {
                    b as char
                } else {
                    '.'
                });
            }
            None => {
                hex.push_str("   ");
                ascii.push(' ');
            }
        }
    }
    (hex, ascii)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_row() {
        let chunk = b"ABCDEFGHIJKLMNOP"; // 16 printable bytes
        let (hex, ascii) = hex_row(0, chunk);
        assert_eq!(
            hex,
            "00000000  41 42 43 44 45 46 47 48  49 4a 4b 4c 4d 4e 4f 50 "
        );
        assert_eq!(ascii, "ABCDEFGHIJKLMNOP");
        assert_eq!(ascii.len(), BYTES_PER_ROW);
    }

    #[test]
    fn short_final_row_is_padded() {
        let chunk = [0xAA, 0xBB, 0xCC];
        let (hex, ascii) = hex_row(0x10, &chunk);
        // Offset gutter + two spaces + the present bytes.
        assert!(hex.starts_with("00000010  aa bb cc "));
        // The hex column has a fixed width so the ASCII gutter stays aligned:
        // 8 (offset) + 2 (gap) + 16*3 (cells) + 1 (group gap) = 59.
        assert_eq!(hex.len(), 59);
        // ASCII is 16 wide: 3 dots (non-printable) then padding spaces.
        assert_eq!(ascii, format!("{:<16}", "..."));
        assert_eq!(ascii.len(), BYTES_PER_ROW);
    }

    #[test]
    fn non_printable_bytes_become_dot() {
        let chunk = [0x00, 0x41, 0x7f, 0x80, 0x1b];
        let (_hex, ascii) = hex_row(0, &chunk);
        // 0x41 is 'A'; 0x00/0x7f/0x80/0x1b are all non-printable; rest padded.
        assert_eq!(ascii, format!("{:<16}", ".A..."));
    }

    #[test]
    fn offset_is_zero_padded_hex() {
        let (hex, _ascii) = hex_row(0x1234abcd, &[]);
        assert!(hex.starts_with("1234abcd  "));
        // An empty chunk still produces a full-width, all-padded hex column.
        assert_eq!(hex.len(), 59);
    }

    // ── slice 2: byte-edit logic ─────────────────────────────────────────────

    #[test]
    fn hex_digit_parses_all_cases() {
        assert_eq!(hex_digit('0'), Some(0));
        assert_eq!(hex_digit('9'), Some(9));
        assert_eq!(hex_digit('a'), Some(10));
        assert_eq!(hex_digit('F'), Some(15));
        assert_eq!(hex_digit('g'), None);
        assert_eq!(hex_digit(' '), None);
    }

    #[test]
    fn nibbles_set_independently() {
        assert_eq!(set_high_nibble(0x00, 0x3), 0x30);
        assert_eq!(set_low_nibble(0x30, 0xc), 0x3c);
        // Setting the high nibble keeps the existing low nibble, and vice-versa.
        assert_eq!(set_high_nibble(0xab, 0x1), 0x1b);
        assert_eq!(set_low_nibble(0xab, 0x9), 0xa9);
        // Only the low 4 bits of the digit are used.
        assert_eq!(set_high_nibble(0x00, 0xff), 0xf0);
        assert_eq!(set_low_nibble(0x00, 0xff), 0x0f);
    }

    #[test]
    fn high_then_low_nibble_compose_to_byte() {
        let mut bytes = vec![0x00];
        // Type '3': sets high nibble, records pending, cursor stays.
        let (cursor, pending, changed) = apply_hex_digit(&mut bytes, 0, None, 0x3);
        assert_eq!(bytes, vec![0x30]);
        assert_eq!(cursor, 0);
        assert_eq!(pending, Some(0x3));
        assert!(changed);
        // Type 'c': sets low nibble, clears pending, advances (clamped).
        let (cursor, pending, changed) = apply_hex_digit(&mut bytes, cursor, pending, 0xc);
        assert_eq!(bytes, vec![0x3c]);
        assert_eq!(cursor, 0); // only one byte → clamps in place
        assert_eq!(pending, None);
        assert!(changed);
    }

    #[test]
    fn hex_low_nibble_advances_cursor() {
        let mut bytes = vec![0x00, 0x00];
        let (cursor, pending, _) = apply_hex_digit(&mut bytes, 0, None, 0xa);
        let (cursor, pending, changed) = apply_hex_digit(&mut bytes, cursor, pending, 0xb);
        assert_eq!(bytes, vec![0xab, 0x00]);
        assert_eq!(cursor, 1); // advanced to the next byte
        assert_eq!(pending, None);
        assert!(changed);
    }

    #[test]
    fn hex_edit_past_end_is_noop() {
        // Empty buffer: nothing to edit.
        let mut empty: Vec<u8> = vec![];
        let (cursor, pending, changed) = apply_hex_digit(&mut empty, 0, None, 0xf);
        assert!(empty.is_empty());
        assert_eq!(cursor, 0);
        assert_eq!(pending, None);
        assert!(!changed);

        // Cursor at/after end of a non-empty buffer.
        let mut bytes = vec![0x11];
        let (_c, _p, changed) = apply_hex_digit(&mut bytes, 1, None, 0xf);
        assert_eq!(bytes, vec![0x11]);
        assert!(!changed);
    }

    #[test]
    fn ascii_overwrite_sets_byte_and_advances() {
        let mut bytes = vec![0x00, 0x00];
        let (cursor, changed) = apply_ascii_char(&mut bytes, 0, 'A');
        assert_eq!(bytes, vec![0x41, 0x00]);
        assert_eq!(cursor, 1);
        assert!(changed);
    }

    #[test]
    fn ascii_non_printable_and_past_end_are_noop() {
        let mut bytes = vec![0x41];
        // Non-printable char.
        let (cursor, changed) = apply_ascii_char(&mut bytes, 0, '\n');
        assert_eq!(bytes, vec![0x41]);
        assert_eq!(cursor, 0);
        assert!(!changed);
        // Past the end.
        let (cursor, changed) = apply_ascii_char(&mut bytes, 5, 'Z');
        assert_eq!(bytes, vec![0x41]);
        assert_eq!(cursor, 5);
        assert!(!changed);
    }

    #[test]
    fn typing_marks_buffer_dirty() {
        let mut view = HexView::new("t".into(), None, vec![0x00, 0x00]);
        assert!(!view.dirty);
        view.edit_mode = true;
        view.focus = Column::Hex;
        view.type_char('4');
        view.type_char('1'); // composes 0x41 = 'A'
        assert!(view.dirty);
        assert_eq!(view.bytes, vec![0x41, 0x00]);
        assert_eq!(view.cursor, 1);

        // Ascii focus overwrite also marks dirty / advances.
        let mut view = HexView::new("t".into(), None, vec![0x00, 0x00]);
        view.edit_mode = true;
        view.focus = Column::Ascii;
        view.type_char('Z');
        assert!(view.dirty);
        assert_eq!(view.bytes, vec![0x5a, 0x00]);
        assert_eq!(view.cursor, 1);
    }
}
