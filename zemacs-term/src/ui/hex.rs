//! Read-only `xxd`-style hex viewer (slice 1 of an editable hex editor).
//!
//! A full-screen overlay [`Component`] that shows a file's **raw bytes** as a
//! classic hex dump: an offset gutter, 16 hex bytes per row grouped 8 + 8, and
//! an ASCII gutter. Opened with the `:hex` typable command.
//!
//! The view is backed by a plain [`Vec<u8>`] (read with [`std::fs::read`]) — not
//! the editor's text [`Rope`](zemacs_core::Rope) — so arbitrary, non-UTF-8 bytes
//! are shown faithfully. This is the foundation for an editable hex editor
//! (slice 2), so the byte model, scrolling and the byte cursor are kept clean
//! and the row formatting lives in a pure, unit-tested helper ([`hex_row`]).
//!
//! Read-only: you can move a byte cursor and scroll, but not edit yet.
//!
//! Keys: `h`/`l`/arrows move the cursor ±1 byte, `j`/`k`/Down/Up move ±16 bytes
//! (one row), `0`/Home and `$`/End jump to the start / end of the row, `g`/`G`
//! to the start / end of the file, PageUp/PageDown (`ctrl-u`/`ctrl-d`) scroll a
//! screenful, `q`/`Esc`/`ctrl-c` close. The mouse wheel scrolls.

use tui::buffer::Buffer as Surface;
use zemacs_view::graphics::Rect;
use zemacs_view::input::MouseEventKind;

use crate::{
    compositor::{Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

/// Number of bytes shown per row.
const BYTES_PER_ROW: usize = 16;

/// The full-screen read-only hex viewer overlay.
pub struct HexView {
    /// Display name of the file (shown in the header).
    file_name: String,
    /// The raw file bytes — the source of truth for everything rendered.
    bytes: Vec<u8>,
    /// Index of the byte under the cursor (`0` when the file is empty).
    cursor: usize,
    /// Index of the top visible row (each row is [`BYTES_PER_ROW`] bytes).
    scroll: usize,
    /// Number of body rows visible in the last render (for page scrolling and
    /// keeping the cursor on screen). Updated every frame.
    viewport: usize,
}

impl HexView {
    /// Construct a viewer over `bytes`, labelled `file_name` in the header.
    pub fn new(file_name: String, bytes: Vec<u8>) -> Self {
        HexView {
            file_name,
            bytes,
            cursor: 0,
            scroll: 0,
            viewport: 1,
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
    /// stays visible. No-op on an empty file.
    fn move_to(&mut self, idx: isize) {
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
}

impl Component for HexView {
    fn handle_event(&mut self, event: &Event, _cx: &mut Context) -> EventResult {
        let key = match event {
            Event::Key(key) => *key,
            Event::Mouse(ev) => {
                match ev.kind {
                    MouseEventKind::ScrollDown => self.scroll_by(3),
                    MouseEventKind::ScrollUp => self.scroll_by(-3),
                    _ => {}
                }
                return EventResult::Consumed(None);
            }
            _ => return EventResult::Ignored(None),
        };

        let cursor = self.cursor as isize;
        let bpr = BYTES_PER_ROW as isize;
        // A screenful of bytes, used for page scrolling.
        let page = (self.viewport.max(1) * BYTES_PER_ROW) as isize;
        // Start of the cursor's current row, for `0`/`$`.
        let row_start = self.cursor - (self.cursor % BYTES_PER_ROW);

        match key {
            key!('q') | key!(Esc) | ctrl!('c') => {
                return EventResult::Consumed(Some(Box::new(
                    |compositor: &mut Compositor, _cx| {
                        compositor.pop();
                    },
                )));
            }
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

        // ── Header (two rows): title + byte count, then a key hint ───────────
        let header_h = 2u16;
        let header = format!(
            " {}  —  {} byte{}  ·  cursor 0x{:08x}",
            self.file_name,
            self.bytes.len(),
            if self.bytes.len() == 1 { "" } else { "s" },
            self.cursor,
        );
        surface.set_stringn(
            area.x,
            area.y,
            &header,
            area.width as usize,
            title_style.add_modifier(zemacs_view::graphics::Modifier::BOLD),
        );
        let hint = " h/l byte  j/k row  0/$ line  g/G file  ^u/^d page  q quit";
        surface.set_stringn(area.x, area.y + 1, hint, area.width as usize, linenr_style);

        let body_y = area.y + header_h;
        let body_h = area.height.saturating_sub(header_h);
        self.viewport = body_h as usize;
        if body_h == 0 {
            return;
        }

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
                        let style = if is_cursor { cursor_style } else { hex_style };
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
                        let style = if is_cursor { cursor_style } else { text_style };
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
}
