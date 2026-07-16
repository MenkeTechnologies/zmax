//! Time stamps — the zmax port of GNU Emacs `time-stamp.el`.
//!
//! A file carries a *template* near its top, e.g.
//!
//! ```text
//! // Time-stamp: <2026-01-01 09:00:00>
//! ```
//!
//! and `M-x time-stamp` rewrites the text between the delimiters with the
//! current time, leaving everything else alone. Emacs only looks at the first
//! `time-stamp-line-limit` lines (8 by default) so a stamp mentioned in the body
//! of the file is not clobbered.
//!
//! This module is the pure part: find the template, rewrite it. The command
//! layer supplies the clock.

/// Emacs `time-stamp-line-limit`: only the first this-many lines are scanned.
pub const LINE_LIMIT: usize = 8;

/// The delimiter pairs Emacs accepts after `Time-stamp:` — `<…>` and `"…"`.
const DELIMS: &[(char, char)] = &[('<', '>'), ('"', '"')];

/// Where a template sits in the text: the char range of the *content* between
/// the delimiters, and the old content.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Template {
    /// Char offset of the first character after the opening delimiter.
    pub start: usize,
    /// Char offset of the closing delimiter.
    pub end: usize,
    /// The text currently between the delimiters.
    pub old: String,
}

/// Locate the first `Time-stamp: <…>` / `Time-stamp: "…"` template within the
/// first `line_limit` lines. Returns `None` when the file has no template —
/// Emacs leaves such a buffer untouched rather than inventing a stamp.
pub fn find(text: &str, line_limit: usize) -> Option<Template> {
    // Scan line by line, tracking the char offset of each line start.
    let mut line_start = 0usize;
    for line in text.split('\n').take(line_limit) {
        if let Some(t) = find_in_line(line, line_start) {
            return Some(t);
        }
        line_start += line.chars().count() + 1; // +1 for the '\n'
    }
    None
}

/// The template on a single line, with char offsets biased by `line_start`.
fn find_in_line(line: &str, line_start: usize) -> Option<Template> {
    let chars: Vec<char> = line.chars().collect();
    let key: Vec<char> = "Time-stamp:".chars().collect();
    let key_at =
        (0..chars.len().saturating_sub(key.len() - 1)).find(|&i| chars[i..].starts_with(&key))?;
    let mut i = key_at + key.len();
    // Emacs allows `\` before the delimiter (for stamps inside string literals).
    while i < chars.len() && (chars[i].is_whitespace() || chars[i] == '\\') {
        i += 1;
    }
    let open = *chars.get(i)?;
    let close = DELIMS.iter().find(|(o, _)| *o == open).map(|(_, c)| *c)?;
    let content_start = i + 1;
    let close_at = (content_start..chars.len()).find(|&j| chars[j] == close)?;
    Some(Template {
        start: line_start + content_start,
        end: line_start + close_at,
        old: chars[content_start..close_at].iter().collect(),
    })
}

/// Emacs `time-stamp-format`'s default, `"%Y-%m-%d %H:%M:%S"`, rendered from an
/// already-decomposed UTC time. Kept here so the format lives next to the
/// template that consumes it.
pub fn format_stamp(year: i32, month: u32, day: u32, hour: u32, min: u32, sec: u32) -> String {
    format!("{year:04}-{month:02}-{day:02} {hour:02}:{min:02}:{sec:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The stamp must be found wherever the comment syntax puts it, and only the
    /// text *between* the delimiters is reported — replacing a wider range would
    /// eat the delimiters and break the template for the next run.
    #[test]
    fn finds_the_template_content_not_the_delimiters() {
        let text = "// header\n// Time-stamp: <2020-01-01 00:00:00>\ncode\n";
        let t = find(text, LINE_LIMIT).expect("template on line 2");
        assert_eq!(t.old, "2020-01-01 00:00:00");
        assert_eq!(&text[t.start..t.end], "2020-01-01 00:00:00");
        assert_eq!(text.chars().nth(t.start - 1), Some('<'));
        assert_eq!(text.chars().nth(t.end), Some('>'));

        // The quote form, and an empty template (the shape you write by hand).
        let t = find("# Time-stamp: \"\"\n", LINE_LIMIT).expect("quote form");
        assert_eq!(t.old, "");
        assert_eq!(t.start, t.end);

        // A backslash-escaped delimiter, as in a C string literal.
        let t = find("char *v = \"Time-stamp: \\<x\\>\";\n", LINE_LIMIT).expect("escaped");
        assert_eq!(t.old, "x\\");
    }

    /// Only the first `line_limit` lines are scanned: a "Time-stamp:" written in
    /// the prose of a file must not be rewritten.
    #[test]
    fn ignores_templates_past_the_line_limit_and_files_without_one() {
        let mut text = String::new();
        for _ in 0..LINE_LIMIT {
            text.push_str("filler\n");
        }
        text.push_str("Time-stamp: <old>\n");
        assert_eq!(find(&text, LINE_LIMIT), None);
        // Raising the limit finds it again — the limit is the only thing hiding it.
        assert!(find(&text, LINE_LIMIT + 2).is_some());

        assert_eq!(find("no stamp here\n", LINE_LIMIT), None);
        // `Time-stamp:` with no delimiter after it is not a template.
        assert_eq!(find("Time-stamp: 2020\n", LINE_LIMIT), None);
    }

    /// Rewriting keeps every other character of the line, including the comment
    /// prefix and anything trailing the stamp.
    #[test]
    fn replacing_the_content_leaves_the_rest_of_the_line_intact() {
        let text = ";; Time-stamp: <2020-01-01 00:00:00> (auto)\n";
        let t = find(text, LINE_LIMIT).unwrap();
        let stamp = format_stamp(2026, 7, 12, 9, 5, 3);
        let mut out: String = text.chars().take(t.start).collect();
        out.push_str(&stamp);
        out.extend(text.chars().skip(t.end));
        assert_eq!(out, ";; Time-stamp: <2026-07-12 09:05:03> (auto)\n");
    }
}
