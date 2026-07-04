//! Emacs `what-cursor-position` (`C-x =`): report the character after point, its
//! decimal/octal/hex code, point's char position within the buffer, the buffer
//! size, the position as a percentage, and the column.
//!
//! The message strings mirror GNU Emacs 30.2 `what-cursor-position` (lisp/
//! simple.el) for the un-narrowed buffer. The narrowed-region `<beg-end>` form
//! and the file-coding-system annex are not reproduced: zemacs has no per-char
//! coding-system substrate and these commands report against the whole buffer,
//! matching the sibling `what-line` / `what-page` commands. All functions are
//! pure and unit tested against real Emacs output.

/// Emacs `single-key-description` for a single character, restricted to the cases
/// `what-cursor-position` uses: the named control keys, the `C-x` form for the
/// remaining control characters, `SPC`/`DEL`, and the literal glyph otherwise.
/// Verified against GNU Emacs 30.2 `single-key-description`.
pub fn key_description(ch: char) -> String {
    match ch {
        '\t' => "TAB".to_string(),
        '\r' => "RET".to_string(),
        '\u{1b}' => "ESC".to_string(),
        ' ' => "SPC".to_string(),
        '\u{7f}' => "DEL".to_string(),
        c if (c as u32) < 0x20 => {
            let code = c as u8;
            // 1..=26 render as the lowercase letter (C-a..C-z); the rest use the
            // code + 0x40 glyph (0 -> @, 28 -> \, 29 -> ], 30 -> ^, 31 -> _).
            let glyph = if (1..=26).contains(&code) {
                (b'a' + code - 1) as char
            } else {
                (code + 0x40) as char
            };
            format!("C-{glyph}")
        }
        c => c.to_string(),
    }
}

/// Build the `what-cursor-position` echo-area message.
///
/// * `char_at_point` — the character after point, or `None` when point is at the
///   end of the buffer.
/// * `point` — 1-based character position of point (Emacs `point`).
/// * `total` — total character count of the buffer (Emacs `buffer-size`).
/// * `column` — 0-based column of point (Emacs `current-column`).
pub fn what_cursor_position(
    char_at_point: Option<char>,
    point: usize,
    total: usize,
    column: usize,
) -> String {
    match char_at_point {
        // End of buffer: no character, no percentage.
        None => format!("point={point} of {total} (EOB) column={column}"),
        Some(ch) => {
            let code = ch as u32;
            let percent =
                ((100.0 * (point.saturating_sub(1)) as f64) / (total.max(1) as f64)).round() as i64;
            format!(
                "Char: {desc} ({code}, #o{code:o}, #x{code:x}) point={point} of {total} ({percent}%) column={column}",
                desc = key_description(ch),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Pinned against GNU Emacs 30.2 `(single-key-description c)`.
    #[test]
    fn key_description_matches_emacs() {
        assert_eq!(key_description('\u{0}'), "C-@");
        assert_eq!(key_description('\u{1}'), "C-a");
        assert_eq!(key_description('\t'), "TAB");
        assert_eq!(key_description('\n'), "C-j");
        assert_eq!(key_description('\r'), "RET");
        assert_eq!(key_description('\u{1b}'), "ESC");
        assert_eq!(key_description(' '), "SPC");
        assert_eq!(key_description('A'), "A");
        assert_eq!(key_description('a'), "a");
        assert_eq!(key_description('~'), "~");
        assert_eq!(key_description('\u{7f}'), "DEL");
        assert_eq!(key_description('\u{1c}'), "C-\\");
        assert_eq!(key_description('λ'), "λ");
    }

    // Pinned against GNU Emacs 30.2 `what-cursor-position` on "hello world":
    //   point 3 -> "Char: l (108, #o154, #x6c) point=3 of 11 (18%) column=2"
    //   end    -> "point=12 of 11 (EOB) column=11"
    #[test]
    fn what_cursor_position_matches_emacs() {
        assert_eq!(
            what_cursor_position(Some('l'), 3, 11, 2),
            "Char: l (108, #o154, #x6c) point=3 of 11 (18%) column=2"
        );
        assert_eq!(
            what_cursor_position(None, 12, 11, 11),
            "point=12 of 11 (EOB) column=11"
        );
    }

    #[test]
    fn what_cursor_position_first_char_is_zero_percent() {
        // point=1 -> (point-1)=0 -> 0%.
        assert_eq!(
            what_cursor_position(Some('h'), 1, 11, 0),
            "Char: h (104, #o150, #x68) point=1 of 11 (0%) column=0"
        );
    }

    #[test]
    fn what_cursor_position_control_char_uses_key_description() {
        // Pinned against GNU Emacs 30.2 on "abcd\tXYZ", point on the TAB (col 4):
        //   "Char: TAB (9, #o11, #x9) point=5 of 8 (50%) column=4"
        assert_eq!(
            what_cursor_position(Some('\t'), 5, 8, 4),
            "Char: TAB (9, #o11, #x9) point=5 of 8 (50%) column=4"
        );
    }

    #[test]
    fn what_cursor_position_empty_buffer_no_divide_by_zero() {
        // Empty buffer: total 0, point 1 at EOB.
        assert_eq!(
            what_cursor_position(None, 1, 0, 0),
            "point=1 of 0 (EOB) column=0"
        );
    }
}
