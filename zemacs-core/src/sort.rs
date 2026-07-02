//! Pure-Rust sorting algorithms for the GNU Emacs `sort.el` "Sorting" family
//! that zemacs did not yet cover: `sort-numeric-fields`, `sort-columns` and
//! `sort-paragraphs`.
//!
//! The whole-line and single-field *lexicographic* sorts already live in
//! [`crate::region_ops::sort_lines`] (with reverse / case-fold / numeric /
//! unique flags) and [`crate::text_engine::sort_by_field`]; this module fills
//! the remaining, absent siblings — numeric-value field sort, column-range
//! sort, and paragraph sort — matching Emacs's documented semantics
//! (<https://www.gnu.org/software/emacs/manual/html_node/emacs/Sorting.html>).
//!
//! Like `region_ops` / `text_engine`, everything here is a plain function over
//! `&[String]` / `&str` with no editor types leaking in, so each is unit tested
//! in isolation. The command layer extracts the live selection's line (or
//! paragraph) span, calls one of these, and applies the result as a single
//! undoable transaction.

use std::cmp::Ordering;

// ---------------------------------------------------------------------------
// Field extraction
// ---------------------------------------------------------------------------

/// Return the `field`-th whitespace-separated field of `line`, faithful to
/// Emacs's 1-based field numbering: `field == 1` is the first field. A negative
/// `field` counts from the right (`-1` is the last field), matching Emacs's
/// `sort-fields` / `sort-numeric-fields` negative-argument behaviour. `field ==
/// 0` is treated as `1`. Returns `None` when the index is out of range.
pub fn nth_field(line: &str, field: i64) -> Option<&str> {
    let fields: Vec<&str> = line.split_whitespace().collect();
    if fields.is_empty() {
        return None;
    }
    let idx = if field < 0 {
        // -1 -> last, -2 -> second-to-last, ...
        let back = (-field) as usize;
        fields.len().checked_sub(back)?
    } else {
        (field.max(1) as usize) - 1
    };
    fields.get(idx).copied()
}

/// Parse `tok` as a number the way Emacs `sort-numeric-fields` does: a leading
/// `0x`/`0X` prefix selects hexadecimal, a leading `0` followed by an octal
/// digit selects octal, and everything else is base-10 (decimal, allowing a
/// sign and a fractional part). Returns `None` when `tok` is not a number.
pub fn parse_field_number(tok: &str) -> Option<f64> {
    let t = tok.trim();
    if t.is_empty() {
        return None;
    }
    let (sign, body) = match t.strip_prefix('-') {
        Some(rest) => (-1.0_f64, rest),
        None => (1.0_f64, t.strip_prefix('+').unwrap_or(t)),
    };
    if body.is_empty() {
        return None;
    }
    // Hexadecimal: 0x1f
    if let Some(hex) = body.strip_prefix("0x").or_else(|| body.strip_prefix("0X")) {
        return i64::from_str_radix(hex, 16).ok().map(|n| sign * n as f64);
    }
    // Octal: a leading 0 immediately followed by octal digits (0755), but not a
    // bare "0" (decimal 0) and not "08" (the 8 is not octal -> decimal).
    if body.len() > 1
        && body.starts_with('0')
        && body.bytes().all(|b| b.is_ascii_digit() && b < b'8')
    {
        return i64::from_str_radix(body, 8).ok().map(|n| sign * n as f64);
    }
    body.parse::<f64>().ok().map(|n| sign * n)
}

// ---------------------------------------------------------------------------
// sort-numeric-fields
// ---------------------------------------------------------------------------

/// Emacs `sort-numeric-fields`: stably sort `lines` by the *numeric value* of
/// their `field`-th whitespace-separated field (1-based; negative counts from
/// the right — see [`nth_field`]). Numbers are parsed per [`parse_field_number`]
/// (hex/octal/decimal). Lines whose field is missing or non-numeric sort first
/// (as the smallest possible value); ties are broken by the whole line to keep
/// a deterministic, stable order. `reverse` flips the final order.
pub fn sort_numeric_fields(lines: &[String], field: i64, reverse: bool) -> Vec<String> {
    let key = |l: &str| -> f64 {
        nth_field(l, field)
            .and_then(parse_field_number)
            .unwrap_or(f64::NEG_INFINITY)
    };
    let mut v = lines.to_vec();
    v.sort_by(|a, b| {
        key(a)
            .partial_cmp(&key(b))
            .unwrap_or(Ordering::Equal)
            .then_with(|| a.cmp(b))
    });
    if reverse {
        v.reverse();
    }
    v
}

// ---------------------------------------------------------------------------
// sort-columns
// ---------------------------------------------------------------------------

/// The sort key for [`sort_columns`]: the character columns `[beg, end)` of
/// `line`. Columns are counted in `char`s (Emacs columns, tabs aside). A line
/// shorter than `beg` yields an empty key; `end` past the line end is clamped.
fn column_key(line: &str, beg: usize, end: usize) -> String {
    let (lo, hi) = (beg.min(end), beg.max(end));
    line.chars().skip(lo).take(hi - lo).collect()
}

/// Emacs `sort-columns`: stably sort `lines` alphabetically by the text in the
/// character-column range `[beg, end)`. Ties are broken by the whole line for a
/// deterministic order. `reverse` sorts into descending order (the command's
/// prefix argument). Unlike Emacs — which rejects tabs because a tab can straddle
/// the column boundary — this treats a tab as a single column character.
pub fn sort_columns(lines: &[String], beg: usize, end: usize, reverse: bool) -> Vec<String> {
    let mut v = lines.to_vec();
    v.sort_by(|a, b| column_key(a, beg, end).cmp(&column_key(b, beg, end)).then_with(|| a.cmp(b)));
    if reverse {
        v.reverse();
    }
    v
}

// ---------------------------------------------------------------------------
// sort-paragraphs
// ---------------------------------------------------------------------------

/// Split `text` into paragraphs the way Emacs's paragraph motion does for
/// `sort-paragraphs`: a paragraph is a maximal run of non-blank lines, and one
/// or more blank lines (lines that are empty or all whitespace) separate them.
/// Leading/trailing blank runs produce no paragraphs. Each returned paragraph
/// preserves its own internal line breaks but not the surrounding blanks.
pub fn split_paragraphs(text: &str) -> Vec<String> {
    let mut paras: Vec<String> = Vec::new();
    let mut cur: Vec<&str> = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            if !cur.is_empty() {
                paras.push(cur.join("\n"));
                cur.clear();
            }
        } else {
            cur.push(line);
        }
    }
    if !cur.is_empty() {
        paras.push(cur.join("\n"));
    }
    paras
}

/// Emacs `sort-paragraphs`: sort the paragraphs of `text` alphabetically,
/// stably, ties broken by the paragraph text itself. Paragraphs are delimited
/// by blank lines (see [`split_paragraphs`]); the result rejoins them with a
/// single blank line between each. `reverse` sorts into descending order.
pub fn sort_paragraphs(text: &str, reverse: bool) -> String {
    let mut paras = split_paragraphs(text);
    paras.sort();
    if reverse {
        paras.reverse();
    }
    let joined = paras.join("\n\n");
    // Preserve a trailing newline if the input had one, so applying the result
    // over a region doesn't swallow the buffer's final line break.
    if text.ends_with('\n') && !joined.is_empty() {
        format!("{joined}\n")
    } else {
        joined
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn nth_field_indexing() {
        assert_eq!(nth_field("a b c", 1), Some("a"));
        assert_eq!(nth_field("a b c", 2), Some("b"));
        assert_eq!(nth_field("a b c", 3), Some("c"));
        // 0 is treated as 1.
        assert_eq!(nth_field("a b c", 0), Some("a"));
        // Out of range.
        assert_eq!(nth_field("a b c", 4), None);
        // Negative counts from the right.
        assert_eq!(nth_field("a b c", -1), Some("c"));
        assert_eq!(nth_field("a b c", -3), Some("a"));
        assert_eq!(nth_field("a b c", -4), None);
        // Leading / interior whitespace is collapsed.
        assert_eq!(nth_field("   x\ty  z", 2), Some("y"));
        assert_eq!(nth_field("   ", 1), None);
    }

    #[test]
    fn parse_field_number_bases() {
        assert_eq!(parse_field_number("42"), Some(42.0));
        assert_eq!(parse_field_number("-7"), Some(-7.0));
        assert_eq!(parse_field_number("+3"), Some(3.0));
        assert_eq!(parse_field_number("3.5"), Some(3.5));
        // Hex.
        assert_eq!(parse_field_number("0x1f"), Some(31.0));
        assert_eq!(parse_field_number("0XFF"), Some(255.0));
        assert_eq!(parse_field_number("-0x10"), Some(-16.0));
        // Octal: leading 0 + octal digits.
        assert_eq!(parse_field_number("0755"), Some(493.0));
        // Bare 0 is decimal, not octal.
        assert_eq!(parse_field_number("0"), Some(0.0));
        // 08 is not octal (8 is not an octal digit) -> decimal.
        assert_eq!(parse_field_number("08"), Some(8.0));
        // Non-numbers.
        assert_eq!(parse_field_number("abc"), None);
        assert_eq!(parse_field_number(""), None);
        assert_eq!(parse_field_number("  "), None);
    }

    #[test]
    fn numeric_field_sort_beats_lexical() {
        // Lexically "10" < "9"; numerically 9 < 10. Sorting by field 2.
        let input = v(&["a 10", "b 9", "c 100", "d 2"]);
        assert_eq!(
            sort_numeric_fields(&input, 2, false),
            v(&["d 2", "b 9", "a 10", "c 100"]),
        );
    }

    #[test]
    fn numeric_field_sort_stable_on_ties() {
        // Equal keys keep whole-line order deterministic (broken by line cmp).
        let input = v(&["x 5", "y 5", "a 5"]);
        assert_eq!(
            sort_numeric_fields(&input, 2, false),
            v(&["a 5", "x 5", "y 5"]),
        );
    }

    #[test]
    fn numeric_field_missing_sorts_first() {
        let input = v(&["a 3", "nofield", "b 1"]);
        assert_eq!(
            sort_numeric_fields(&input, 2, false),
            v(&["nofield", "b 1", "a 3"]),
        );
    }

    #[test]
    fn numeric_field_reverse() {
        let input = v(&["a 1", "b 2", "c 3"]);
        assert_eq!(
            sort_numeric_fields(&input, 2, true),
            v(&["c 3", "b 2", "a 1"]),
        );
    }

    #[test]
    fn numeric_field_negative_index() {
        // Sort by the last field's number.
        let input = v(&["row 10", "row 2", "row 30"]);
        assert_eq!(
            sort_numeric_fields(&input, -1, false),
            v(&["row 2", "row 10", "row 30"]),
        );
    }

    #[test]
    fn column_key_slices_chars() {
        assert_eq!(column_key("abcdef", 1, 4), "bcd");
        // Short line -> clamped / empty.
        assert_eq!(column_key("ab", 3, 6), "");
        assert_eq!(column_key("abcdef", 4, 100), "ef");
        // beg > end is normalised.
        assert_eq!(column_key("abcdef", 4, 1), "bcd");
        // Multibyte columns counted as chars, not bytes.
        assert_eq!(column_key("héllo", 1, 3), "él");
    }

    #[test]
    fn columns_sort_by_range() {
        // Sort by columns [3,8): the field after the leading number.
        let input = v(&["01 zeta", "02 alpha", "03 mu"]);
        assert_eq!(
            sort_columns(&input, 3, 8, false),
            v(&["02 alpha", "03 mu", "01 zeta"]),
        );
    }

    #[test]
    fn columns_reverse() {
        let input = v(&["a", "b", "c"]);
        assert_eq!(sort_columns(&input, 0, 1, true), v(&["c", "b", "a"]));
    }

    #[test]
    fn split_paragraphs_on_blank_lines() {
        let text = "one\ntwo\n\nthree\n\n\nfour\nfive\n";
        assert_eq!(
            split_paragraphs(text),
            v(&["one\ntwo", "three", "four\nfive"]),
        );
        // Leading / trailing blanks are ignored.
        assert_eq!(split_paragraphs("\n\nsolo\n\n"), v(&["solo"]));
        // Whitespace-only lines count as blank separators.
        assert_eq!(split_paragraphs("a\n   \nb"), v(&["a", "b"]));
        assert!(split_paragraphs("   \n\n").is_empty());
    }

    #[test]
    fn paragraphs_sort_alphabetically() {
        let text = "zebra\ntail\n\nalpha\nbody\n\nmiddle";
        assert_eq!(
            sort_paragraphs(text, false),
            "alpha\nbody\n\nmiddle\n\nzebra\ntail",
        );
    }

    #[test]
    fn paragraphs_sort_reverse_and_trailing_newline() {
        let text = "a\n\nb\n\nc\n";
        assert_eq!(sort_paragraphs(text, true), "c\n\nb\n\na\n");
        // No trailing newline in -> none out.
        assert_eq!(sort_paragraphs("a\n\nb", false), "a\n\nb");
    }

    #[test]
    fn paragraphs_stable_on_equal() {
        let text = "same\n\nsame\n\nother";
        assert_eq!(sort_paragraphs(text, false), "other\n\nsame\n\nsame");
    }
}
