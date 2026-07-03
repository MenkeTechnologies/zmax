//! Emacs page motion. Pages are delimited by the form-feed control character
//! `^L` (`\u{000C}`); the buffer boundaries are implicit page delimiters. This
//! backs `forward-page` (C-x ]), `backward-page` (C-x [) and `mark-page`
//! (C-x C-p). All functions operate on char indices into `text`.

/// The page-delimiter character (`^L`, form feed).
pub const PAGE_DELIMITER: char = '\u{000C}';

/// Char index just past the next page delimiter at or after `cursor`
/// (`forward-page`). Returns the buffer length when no further delimiter exists.
pub fn forward_page(text: &str, cursor: usize) -> usize {
    let chars: Vec<char> = text.chars().collect();
    let mut i = cursor;
    while i < chars.len() {
        if chars[i] == PAGE_DELIMITER {
            return i + 1;
        }
        i += 1;
    }
    chars.len()
}

/// Char index of the start of the page *before* `cursor` (`backward-page`): the
/// position just past the previous page delimiter. When `cursor` already sits at
/// a page start, the delimiter immediately behind it is skipped so point moves to
/// the previous page rather than staying put. Returns 0 when none precedes it.
pub fn backward_page(text: &str, cursor: usize) -> usize {
    let chars: Vec<char> = text.chars().collect();
    if cursor == 0 {
        return 0;
    }
    let mut i = cursor - 1;
    // Skip a delimiter immediately behind us — we are at a page start already.
    if chars.get(i) == Some(&PAGE_DELIMITER) {
        if i == 0 {
            return 0;
        }
        i -= 1;
    }
    loop {
        if chars[i] == PAGE_DELIMITER {
            return i + 1;
        }
        if i == 0 {
            return 0;
        }
        i -= 1;
    }
}

/// The `[start, end)` char range of the page containing `cursor` (`mark-page`):
/// from the start of the current page to the start of the next one (or the
/// buffer end). The trailing delimiter is included in the range, matching Emacs.
pub fn page_bounds(text: &str, cursor: usize) -> (usize, usize) {
    let chars: Vec<char> = text.chars().collect();
    // Start of the current page: just past the nearest preceding delimiter.
    let mut start = 0;
    if cursor > 0 {
        let mut i = cursor - 1;
        loop {
            if chars.get(i) == Some(&PAGE_DELIMITER) {
                start = i + 1;
                break;
            }
            if i == 0 {
                break;
            }
            i -= 1;
        }
    }
    (start, forward_page(text, cursor))
}

#[cfg(test)]
mod tests {
    use super::*;

    // "a\fb\fc": a=0 \f=1 b=2 \f=3 c=4 len=5
    const S: &str = "a\u{000C}b\u{000C}c";

    #[test]
    fn forward_page_lands_past_delimiter() {
        assert_eq!(forward_page(S, 0), 2); // start of "b"
        assert_eq!(forward_page(S, 2), 4); // start of "c"
        assert_eq!(forward_page(S, 4), 5); // no more delimiters -> EOB
    }

    #[test]
    fn backward_page_moves_to_previous_page_start() {
        assert_eq!(backward_page(S, 5), 4); // from EOB -> start of "c"
        assert_eq!(backward_page(S, 4), 2); // at "c" start -> skip, prev page "b"
        assert_eq!(backward_page(S, 2), 0); // at "b" start -> skip, first page
        assert_eq!(backward_page(S, 0), 0);
    }

    #[test]
    fn page_bounds_wraps_the_current_page() {
        assert_eq!(page_bounds(S, 4), (4, 5)); // "c"
        assert_eq!(page_bounds(S, 2), (2, 4)); // "b\f"
        assert_eq!(page_bounds(S, 0), (0, 2)); // "a\f"
    }

    #[test]
    fn no_delimiters_is_one_page() {
        let t = "hello world";
        assert_eq!(forward_page(t, 0), t.chars().count());
        assert_eq!(backward_page(t, 5), 0);
        assert_eq!(page_bounds(t, 5), (0, t.chars().count()));
    }
}
