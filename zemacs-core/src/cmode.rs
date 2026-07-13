//! Pure line/brace logic for GNU Emacs `c-mode` / `cc-mode` (`cc-cmds.el`).
//!
//! Everything here is pure: it operates on borrowed `&str` lines or a `&str`
//! buffer plus a char index and returns line indices, char indices or new
//! `String`s, so the preprocessor-conditional matching, comment fill,
//! backslash alignment and statement motion can be unit tested without an
//! editor. The behaviour mirrors the documented algorithms of GNU Emacs 30.x
//! `cc-cmds.el`; where a construct's detection is deliberately restricted to
//! the common forms (e.g. statement motion does not parse string/comment
//! contents) that is called out on the relevant function.

// ---------------------------------------------------------------------------
// Preprocessor conditional motion (#if / #ifdef / #ifndef / #elif / #else /
// #endif). Faithful port of the depth-counting core of `c-scan-conditionals`.
// ---------------------------------------------------------------------------

/// A C preprocessor conditional directive on a source line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Directive {
    /// An opening directive: `#if`, `#ifdef`, `#ifndef`.
    If,
    /// A continuation directive: `#elif`, `#elifdef`, `#elifndef`, `#else`.
    Else,
    /// A closing directive: `#endif`.
    Endif,
    /// Any line that is not a preprocessor conditional directive.
    None,
}

/// Classify `line` as a preprocessor conditional directive. Recognises an
/// optional leading run of whitespace, then `#`, then optional whitespace, then
/// the directive keyword (matching Emacs `c-cpp-conditional-key`).
pub fn classify_directive(line: &str) -> Directive {
    let rest = line.trim_start();
    let Some(rest) = rest.strip_prefix('#') else {
        return Directive::None;
    };
    let word: String = rest
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_alphabetic())
        .collect();
    match word.as_str() {
        "if" | "ifdef" | "ifndef" => Directive::If,
        "elif" | "elifdef" | "elifndef" | "else" => Directive::Else,
        "endif" => Directive::Endif,
        _ => Directive::None,
    }
}

/// Emacs `c-forward-conditional` (`C-c C-n`): move forward across the following
/// preprocessor conditional. Scanning from `cur` (inclusive) with a depth that
/// starts at 0, each `#if` opens a level and each `#endif` closes one; the
/// target is the `#endif` that closes either a block opened after `cur` (depth
/// returns to 0) or the block already enclosing `cur` (depth goes to -1).
/// Returns the line index of that `#endif`, or `None` when there is none.
pub fn forward_conditional(lines: &[&str], cur: usize) -> Option<usize> {
    let mut depth: i32 = 0;
    for (i, line) in lines.iter().enumerate().skip(cur) {
        match classify_directive(line) {
            Directive::If => depth += 1,
            Directive::Endif => {
                depth -= 1;
                if depth <= 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Emacs `c-backward-conditional` (`C-c C-p`): move backward across the
/// preceding preprocessor conditional. The mirror of [`forward_conditional`]:
/// scanning up from `cur` (inclusive) each `#endif` opens a level and each
/// `#if` closes one; the target is the `#if` that closes a block ending before
/// `cur` (depth returns to 0) or the block enclosing `cur` (depth goes to -1).
pub fn backward_conditional(lines: &[&str], cur: usize) -> Option<usize> {
    let mut depth: i32 = 0;
    for i in (0..=cur.min(lines.len().saturating_sub(1))).rev() {
        match classify_directive(lines[i]) {
            Directive::Endif => depth += 1,
            Directive::If => {
                depth -= 1;
                if depth <= 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Emacs `c-up-conditional` (`C-c C-u`): move to the start of the conditional
/// that *contains* point, going up `count` levels of nesting. Unlike
/// [`backward_conditional`], balanced sibling blocks are skipped: the target is
/// only reached when the depth drops below the starting level (an enclosing,
/// still-open `#if`). Returns the enclosing `#if` line, or `None` at top level.
pub fn up_conditional(lines: &[&str], cur: usize, count: usize) -> Option<usize> {
    let mut from = cur;
    let mut result = None;
    for _ in 0..count.max(1) {
        let mut depth: i32 = 0;
        let mut found = None;
        for i in (0..=from.min(lines.len().saturating_sub(1))).rev() {
            match classify_directive(lines[i]) {
                Directive::Endif => depth += 1,
                Directive::If => {
                    depth -= 1;
                    if depth < 0 {
                        found = Some(i);
                        break;
                    }
                }
                _ => {}
            }
        }
        let j = found?;
        result = Some(j);
        if j == 0 {
            break;
        }
        from = j - 1;
    }
    result
}

// ---------------------------------------------------------------------------
// C macro (#define ... \) context.
// ---------------------------------------------------------------------------

/// True when line `cur` lies inside a multi-line preprocessor macro body: some
/// preceding, unbroken chain of backslash-continued lines starts with a `#`
/// directive. Used by `c-context-line-break` to decide whether a fresh line
/// needs a trailing `\` continuation.
pub fn in_cpp_macro(lines: &[&str], cur: usize) -> bool {
    if cur >= lines.len() {
        return false;
    }
    // Walk back over the continuation chain feeding into `cur`.
    let mut i = cur;
    while i > 0 && lines[i - 1].trim_end().ends_with('\\') {
        i -= 1;
    }
    // The chain is a macro when its first line is a preprocessor directive and
    // `cur` is not past the end of the chain (the chain reaches `cur`).
    (lines[i].trim_start().starts_with('#') && i < cur) || lines[cur].trim_start().starts_with('#')
}

/// The comment-continuation prefix for a fresh line opened inside the comment on
/// `line`, or `None` when `line` is not a continuable comment. A `//` line
/// yields `<indent>// `; a block-comment body line beginning with `*` yields
/// `<indent>* ` (aligning the `*` under the opener). Mirrors the prefix
/// `c-context-line-break` reinserts.
pub fn comment_continuation_prefix(line: &str) -> Option<String> {
    let indent = leading_ws(line);
    let body = &line[indent.len()..];
    if body.starts_with("//") {
        return Some(format!("{indent}// "));
    }
    if body.starts_with('*') || body.starts_with("/*") {
        // Align the continuation `*` one column in from the opener when the
        // line began with `/*`, else keep the existing `*` column.
        let star_indent = if body.starts_with("/*") {
            format!("{indent} ")
        } else {
            indent.to_string()
        };
        return Some(format!("{star_indent}* "));
    }
    None
}

// ---------------------------------------------------------------------------
// Comment fill — c-fill-paragraph over a `//` or ` * ` comment block.
// ---------------------------------------------------------------------------

/// The leading whitespace (spaces/tabs) of `line`.
fn leading_ws(line: &str) -> &str {
    let end = line
        .char_indices()
        .find(|(_, c)| *c != ' ' && *c != '\t')
        .map(|(i, _)| i)
        .unwrap_or(line.len());
    &line[..end]
}

/// Strip the comment markers from one body line of a `//` or ` * ` comment,
/// returning the bare text. Handles `//`, a leading `*`, and the `/*` / `*/`
/// delimiters so opener/closer lines contribute their words too.
fn strip_comment_markers(line: &str) -> &str {
    let t = line.trim_start();
    let t = t.strip_prefix("//").unwrap_or(t);
    let t = t.strip_prefix("/*").unwrap_or(t);
    let t = t.strip_suffix("*/").unwrap_or(t);
    let t = t.trim();
    // A pure ` * ` continuation marker.
    t.strip_prefix('*').unwrap_or(t).trim()
}

/// Emacs `c-fill-paragraph` for a run of comment lines: collect the words from
/// every line (stripping `//`, `*`, `/*`, `*/` markers) and greedily rewrap
/// them to `fill_column`, reusing the block's comment prefix. The prefix is
/// `<indent>// ` when the first line is a line comment, else `<indent>* `
/// (star-continuation block body). The `/*`/`*/` delimiters themselves are not
/// re-synthesised — the caller fills the interior lines.
pub fn fill_c_comment(lines: &[&str], fill_column: usize) -> Vec<String> {
    let first = match lines.iter().find(|l| !l.trim().is_empty()) {
        Some(l) => *l,
        None => return lines.iter().map(|l| l.to_string()).collect(),
    };
    let indent = leading_ws(first);
    let is_line_comment = first.trim_start().starts_with("//");
    let prefix = if is_line_comment {
        format!("{indent}// ")
    } else {
        format!("{indent}* ")
    };
    let mut words: Vec<String> = Vec::new();
    for line in lines {
        for w in strip_comment_markers(line).split_whitespace() {
            words.push(w.to_string());
        }
    }
    if words.is_empty() {
        return lines.iter().map(|l| l.to_string()).collect();
    }
    let filled = crate::text_engine::fill_paragraph(&words.join(" "), fill_column, &prefix);
    filled.lines().map(str::to_string).collect()
}

// ---------------------------------------------------------------------------
// Backslash region — c-backslash-region: align trailing `\` continuations.
// ---------------------------------------------------------------------------

/// The content of `line` with any trailing backslash and the whitespace before
/// it removed (the "code" part), plus its display width in characters.
fn line_body(line: &str) -> &str {
    let t = line.trim_end();
    t.strip_suffix('\\').unwrap_or(t).trim_end()
}

/// The alignment column Emacs `c-backslash-region` chooses with no prefix
/// argument: one column past the longest body among `lines` (excluding the
/// final line, which never gets a backslash), but never less than
/// `c-backslash-column` (48) and never more than `c-backslash-max-column` (72).
pub fn backslash_column(lines: &[&str]) -> usize {
    const BACKSLASH_COLUMN: usize = 48;
    const BACKSLASH_MAX_COLUMN: usize = 72;
    let n = lines.len();
    let longest = lines
        .iter()
        .take(n.saturating_sub(1))
        .map(|l| line_body(l).chars().count())
        .max()
        .unwrap_or(0);
    (longest + 1).clamp(BACKSLASH_COLUMN, BACKSLASH_MAX_COLUMN)
}

/// Emacs `c-backslash-region`: append a trailing `\` to every line except the
/// last, aligned at `column` (padding with spaces). When a line's body already
/// reaches or passes `column`, the `\` is placed a single space after it. The
/// final line has any trailing backslash removed. Blank lines are aligned too,
/// matching cc-mode.
pub fn align_backslashes(lines: &[&str], column: usize) -> Vec<String> {
    let n = lines.len();
    lines
        .iter()
        .enumerate()
        .map(|(i, line)| {
            let body = line_body(line);
            if i + 1 == n {
                // Last line: drop any continuation backslash.
                return body.to_string();
            }
            let width = body.chars().count();
            let pad = column.saturating_sub(width).max(1);
            format!("{body}{}\\", " ".repeat(pad))
        })
        .collect()
}

/// Emacs `c-backslash-region` with a prefix argument: remove every trailing
/// backslash (and the whitespace that padded it) in the region.
pub fn remove_backslashes(lines: &[&str]) -> Vec<String> {
    lines.iter().map(|l| line_body(l).to_string()).collect()
}

// ---------------------------------------------------------------------------
// Statement motion — c-beginning-of-statement / c-end-of-statement.
// ---------------------------------------------------------------------------

/// A character that terminates a C statement for the purposes of the simple
/// statement motions.
fn is_stmt_delimiter(c: char) -> bool {
    matches!(c, ';' | '{' | '}')
}

/// Emacs `c-beginning-of-statement` (simplified): return the char index of the
/// first non-whitespace character of the statement containing `pos`. Scans back
/// over whitespace, then to just after the previous `;`, `{` or `}`, then
/// forward over whitespace to the statement's first token. Does not parse
/// string or comment contents (a `;` inside a literal is treated as a
/// delimiter), which matches the common case.
pub fn beginning_of_statement(s: &str, pos: usize) -> usize {
    let chars: Vec<char> = s.chars().collect();
    let mut i = pos.min(chars.len());
    // Step back off whitespace and off a delimiter we are sitting on/just after.
    while i > 0 && chars[i - 1].is_whitespace() {
        i -= 1;
    }
    if i > 0 && is_stmt_delimiter(chars[i - 1]) {
        i -= 1;
    }
    // Scan back to the previous delimiter.
    while i > 0 && !is_stmt_delimiter(chars[i - 1]) {
        i -= 1;
    }
    // Skip forward over whitespace to the first token.
    while i < chars.len() && chars[i].is_whitespace() {
        i += 1;
    }
    i
}

/// Emacs `c-end-of-statement` (simplified): return the char index just past the
/// next `;`, `{` or `}` at or after `pos`, i.e. the end of the current
/// statement. Shares the string/comment limitation of
/// [`beginning_of_statement`].
pub fn end_of_statement(s: &str, pos: usize) -> usize {
    let chars: Vec<char> = s.chars().collect();
    let mut i = pos.min(chars.len());
    // If we are already on a delimiter, move past it.
    while i < chars.len() && !is_stmt_delimiter(chars[i]) {
        i += 1;
    }
    if i < chars.len() {
        i += 1; // consume the delimiter
    }
    i
}

// ---------------------------------------------------------------------------
// `ff-find-related-file` (find-file.el): the header <-> source counterpart.
// ---------------------------------------------------------------------------

/// The extensions `ff-find-related-file` looks for, keyed by the extension of
/// the file at hand — emacs's `cc-other-file-alist` for C/C++/ObjC.
const OTHER_FILE_EXTS: &[(&str, &[&str])] = &[
    ("c", &["h"]),
    ("m", &["h"]),
    ("cc", &["hh", "h", "hpp"]),
    ("cpp", &["hpp", "hh", "h", "hxx"]),
    ("cxx", &["hxx", "hpp", "hh", "h"]),
    ("c++", &["h++", "hpp", "hh", "h"]),
    ("h", &["c", "cc", "cpp", "cxx", "c++", "m"]),
    ("hh", &["cc", "cpp", "cxx", "c++"]),
    ("hpp", &["cpp", "cc", "cxx", "c++"]),
    ("hxx", &["cxx", "cpp", "cc"]),
    ("h++", &["c++", "cpp", "cc"]),
];

/// Emacs `ff-find-related-file`: the candidate names of the file related to
/// `file_name` — the header for a source file, the source for a header — in the
/// order emacs's `cc-other-file-alist` tries them. The stem is kept and only the
/// extension varies; an unknown extension has no counterpart.
pub fn related_file_names(file_name: &str) -> Vec<String> {
    let (stem, ext) = match file_name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s, e.to_ascii_lowercase()),
        _ => return Vec::new(),
    };
    OTHER_FILE_EXTS
        .iter()
        .find(|(from, _)| *from == ext)
        .map(|(_, to)| to.iter().map(|e| format!("{stem}.{e}")).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A source file's counterpart is its header (and vice versa), preserving the
    /// stem; an unrelated extension has no counterpart.
    #[test]
    fn related_file_names_pairs_source_and_header() {
        assert_eq!(related_file_names("src/foo.c"), vec!["src/foo.h"]);
        assert_eq!(
            related_file_names("foo.h"),
            vec!["foo.c", "foo.cc", "foo.cpp", "foo.cxx", "foo.c++", "foo.m"]
        );
        assert_eq!(
            related_file_names("a/b/Widget.cpp"),
            vec![
                "a/b/Widget.hpp",
                "a/b/Widget.hh",
                "a/b/Widget.h",
                "a/b/Widget.hxx"
            ]
        );
        assert!(related_file_names("main.rs").is_empty());
        assert!(related_file_names("Makefile").is_empty());
    }

    fn lines(s: &str) -> Vec<&str> {
        s.lines().collect()
    }

    #[test]
    fn classify_directive_recognises_forms() {
        assert_eq!(classify_directive("#if X"), Directive::If);
        assert_eq!(classify_directive("  #  ifdef X"), Directive::If);
        assert_eq!(classify_directive("#ifndef X"), Directive::If);
        assert_eq!(classify_directive("#elif Y"), Directive::Else);
        assert_eq!(classify_directive("#else"), Directive::Else);
        assert_eq!(classify_directive("#endif"), Directive::Endif);
        assert_eq!(classify_directive("int x;"), Directive::None);
        assert_eq!(classify_directive("#define X 1"), Directive::None);
    }

    #[test]
    fn forward_conditional_over_a_block() {
        let src = "#if A\nx\n#endif\ny";
        let l = lines(src);
        // Cursor on the #if (line 0) jumps to its matching #endif (line 2).
        assert_eq!(forward_conditional(&l, 0), Some(2));
        // Cursor inside the block (line 1) also exits at the enclosing #endif.
        assert_eq!(forward_conditional(&l, 1), Some(2));
        // Below the block there is nothing to move over.
        assert_eq!(forward_conditional(&l, 3), None);
    }

    #[test]
    fn forward_conditional_nested() {
        let src = "#if A\n#if B\ny\n#endif\n#endif";
        let l = lines(src);
        // From the inner #if, stop at the inner #endif.
        assert_eq!(forward_conditional(&l, 1), Some(3));
        // From the outer #if, stop at the outer #endif.
        assert_eq!(forward_conditional(&l, 0), Some(4));
    }

    #[test]
    fn backward_conditional_over_a_block() {
        let src = "#if A\nx\n#endif\ny";
        let l = lines(src);
        // Cursor on the #endif (line 2) jumps back to the #if.
        assert_eq!(backward_conditional(&l, 2), Some(0));
        // Cursor inside the block jumps back to the enclosing #if.
        assert_eq!(backward_conditional(&l, 1), Some(0));
    }

    #[test]
    fn up_conditional_skips_siblings() {
        // Inner balanced block, then a plain line, inside the outer block.
        let src = "#if OUTER\n#if INNER\n#endif\nx\n#endif";
        let l = lines(src);
        // From the plain line, up-conditional goes to the OUTER #if, not INNER.
        assert_eq!(up_conditional(&l, 3, 1), Some(0));
        // backward-conditional instead lands on the (balanced) INNER #if.
        assert_eq!(backward_conditional(&l, 3), Some(1));
    }

    #[test]
    fn up_conditional_multiple_levels() {
        let src = "#if A\n#if B\n#if C\nx\n#endif\n#endif\n#endif";
        let l = lines(src);
        assert_eq!(up_conditional(&l, 3, 1), Some(2)); // enclosing C
        assert_eq!(up_conditional(&l, 3, 2), Some(1)); // up to B
        assert_eq!(up_conditional(&l, 3, 3), Some(0)); // up to A
    }

    #[test]
    fn in_cpp_macro_detects_continuation() {
        let src = "#define FOO \\\n    bar \\\n    baz\nint x;";
        let l = lines(src);
        assert!(in_cpp_macro(&l, 0)); // the #define line itself
        assert!(in_cpp_macro(&l, 1)); // a continued body line
        assert!(in_cpp_macro(&l, 2)); // last continued line
        assert!(!in_cpp_macro(&l, 3)); // plain code after the macro
    }

    #[test]
    fn comment_continuation_prefix_forms() {
        assert_eq!(
            comment_continuation_prefix("    // hi"),
            Some("    // ".to_string())
        );
        assert_eq!(
            comment_continuation_prefix("   * body"),
            Some("   * ".to_string())
        );
        assert_eq!(
            comment_continuation_prefix("  /* open"),
            Some("   * ".to_string())
        );
        assert_eq!(comment_continuation_prefix("int x;"), None);
    }

    #[test]
    fn fill_c_comment_line_comment() {
        let src = ["// the quick brown", "// fox jumps"];
        let out = fill_c_comment(&src, 12);
        assert_eq!(out, vec!["// the quick", "// brown fox", "// jumps"]);
    }

    #[test]
    fn fill_c_comment_star_block() {
        let src = [" * alpha beta", " * gamma"];
        let out = fill_c_comment(&src, 10);
        assert_eq!(out, vec![" * alpha", " * beta", " * gamma"]);
    }

    #[test]
    fn backslash_column_default_is_forty_eight() {
        // Short lines round up to the c-backslash-column minimum of 48.
        let src = ["a", "bb", "ccc"];
        assert_eq!(backslash_column(&src), 48);
    }

    #[test]
    fn align_backslashes_pads_to_column() {
        let src = ["#define M(x) \\", "  do_it(x)"];
        let out = align_backslashes(&src, 20);
        assert_eq!(out[0], "#define M(x)        \\");
        assert_eq!(out[0].chars().count(), 21); // 20 cols + the backslash
        assert_eq!(out[1], "  do_it(x)"); // last line: no backslash
    }

    #[test]
    fn align_backslashes_long_line_gets_single_space() {
        let src = ["this_is_a_very_long_line_of_code()", "end"];
        let out = align_backslashes(&src, 10);
        assert_eq!(out[0], "this_is_a_very_long_line_of_code() \\");
    }

    #[test]
    fn remove_backslashes_strips_continuations() {
        let src = ["a   \\", "b\t\\", "c"];
        assert_eq!(remove_backslashes(&src), vec!["a", "b", "c"]);
    }

    #[test]
    fn beginning_of_statement_finds_token_start() {
        // "a = 1; b = 2;" — from inside the second statement, go to `b`.
        let s = "a = 1; b = 2;";
        let pos = s.find("2").unwrap();
        assert_eq!(beginning_of_statement(s, pos), s.find('b').unwrap());
    }

    #[test]
    fn beginning_of_statement_after_brace() {
        let s = "void f() { do_x(); }";
        let pos = s.find("do_x").unwrap() + 1;
        assert_eq!(beginning_of_statement(s, pos), s.find("do_x").unwrap());
    }

    #[test]
    fn end_of_statement_stops_after_semicolon() {
        let s = "a = 1; b = 2;";
        let pos = s.find('a').unwrap();
        assert_eq!(end_of_statement(s, pos), s.find(';').unwrap() + 1);
    }

    #[test]
    fn end_of_statement_stops_after_brace() {
        let s = "if (c) { x; }";
        let pos = 0;
        assert_eq!(end_of_statement(s, pos), s.find('{').unwrap() + 1);
    }
}
