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
// Dead-branch analysis (`hide-ifdef-mode`, `cpp-highlight-buffer`).
// ---------------------------------------------------------------------------

/// A branch of a preprocessor conditional: the lines its body spans, and whether
/// the preprocessor would compile it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Branch {
    /// First line of the *body* (the line after the directive).
    pub start: usize,
    /// One past the last line of the body (the line of the next directive).
    pub end: usize,
    /// The directive's condition, as written (`""` for `#else`).
    pub condition: String,
    /// `Some(false)` when the branch is provably not compiled, `Some(true)` when
    /// it provably is, `None` when the condition cannot be decided from the file
    /// alone.
    pub taken: Option<bool>,
}

/// Where each macro in `lines` is `#define`d. This is the only definition
/// environment this port has (Emacs takes one from `hide-ifdef-env`, populated by
/// `hide-ifdef-define`, which zmax does not have).
///
/// The *line* matters: a macro counts as defined only for the directives that
/// follow its `#define`, exactly as the preprocessor sees it. Ignoring the order
/// would break the include-guard idiom — `#ifndef FOO_H` immediately followed by
/// `#define FOO_H` would evaluate false and hide the whole header.
fn defined_macros(lines: &[&str]) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let rest = line.trim_start();
        let Some(rest) = rest.strip_prefix('#') else {
            continue;
        };
        let rest = rest.trim_start();
        let Some(rest) = rest.strip_prefix("define") else {
            continue;
        };
        let name: String = rest
            .trim_start()
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if !name.is_empty() {
            out.push((i, name));
        }
    }
    out
}

/// Whether `name` is `#define`d somewhere above line `before`.
fn is_defined(defines: &[(usize, String)], before: usize, name: &str) -> bool {
    defines
        .iter()
        .any(|(line, macro_name)| *line < before && macro_name == name)
}

/// The condition text of a conditional directive: everything after the keyword.
fn condition_of(line: &str) -> String {
    let rest = line.trim_start().trim_start_matches('#').trim_start();
    let keyword: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphabetic())
        .collect();
    rest[keyword.len()..].trim().to_string()
}

/// Evaluate a `#if` / `#ifdef` / `#elif` condition against the macros defined in
/// the file. Deliberately narrow: only the forms whose truth is *certain* from
/// the file alone are decided — a literal `0`/`1`, `defined(X)` / `!defined(X)`,
/// and the `#ifdef` / `#ifndef` keywords. Anything with arithmetic, comparisons
/// or an unknown macro's *value* is `None` ("cannot tell"), and a `None` branch is
/// never hidden.
///
/// GNU Emacs' `hide-ifdef-mode` instead evaluates against `hide-ifdef-env` and
/// hides every branch that is not true, so with the default (empty) env it hides
/// the body of every `#ifdef`. zmax has no `hide-ifdef-define` to populate such
/// an env, so hiding on "cannot tell" would blank out most of a real C file. The
/// file's own `#define`s are used instead, and undecidable branches stay visible.
fn eval_condition(
    keyword: &str,
    condition: &str,
    at: usize,
    defines: &[(usize, String)],
) -> Option<bool> {
    let cond = condition.trim();
    match keyword {
        "ifdef" | "elifdef" => return Some(is_defined(defines, at, cond)),
        "ifndef" | "elifndef" => return Some(!is_defined(defines, at, cond)),
        _ => {}
    }
    // `#if 0` / `#if 1` — the idiomatic "comment this out".
    if let Ok(n) = cond.parse::<i64>() {
        return Some(n != 0);
    }
    // `defined(X)` / `defined X`, optionally negated once.
    let (negated, rest) = match cond.strip_prefix('!') {
        Some(rest) => (true, rest.trim()),
        None => (false, cond),
    };
    let name = rest
        .strip_prefix("defined")
        .map(|r| r.trim())
        .map(|r| r.trim_start_matches('(').trim_end_matches(')').trim())?;
    if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return None;
    }
    let value = is_defined(defines, at, name);
    Some(value != negated)
}

/// Split every preprocessor conditional in `lines` into its branches, deciding
/// which are compiled. Nested conditionals produce nested branches (each is
/// reported independently); a branch inside a dead branch is reported with its
/// own verdict, so callers that hide dead code hide the outer body anyway.
///
/// This is the engine behind `hide-ifdef-mode` (hide the dead ones) and
/// `cpp-highlight-buffer` (shade them).
pub fn conditional_branches(lines: &[&str]) -> Vec<Branch> {
    let defines = defined_macros(lines);
    let mut out = Vec::new();
    // The open conditionals: (line of the directive that opened the branch,
    // keyword, condition, whether an earlier branch of this group was taken).
    let mut stack: Vec<(usize, String, String, bool)> = Vec::new();

    for (i, line) in lines.iter().enumerate() {
        let directive = classify_directive(line);
        if directive == Directive::None {
            continue;
        }
        let rest = line.trim_start().trim_start_matches('#').trim_start();
        let keyword: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphabetic())
            .collect();
        let condition = condition_of(line);

        // Close the branch this directive ends, if any.
        if matches!(directive, Directive::Else | Directive::Endif) {
            if let Some((open_line, open_kw, open_cond, earlier_taken)) = stack.pop() {
                let taken =
                    branch_verdict(&open_kw, &open_cond, open_line, earlier_taken, &defines);
                out.push(Branch {
                    start: open_line + 1,
                    end: i,
                    condition: open_cond,
                    taken,
                });
                if directive == Directive::Else {
                    // `#else` / `#elif` opens the next branch of the same group.
                    // It can only be taken when no earlier branch was.
                    let any_taken = earlier_taken || taken == Some(true);
                    stack.push((i, keyword.clone(), condition.clone(), any_taken));
                    continue;
                }
            } else if directive == Directive::Else {
                // An `#else` with no opener: nothing to close, open a branch anyway.
                stack.push((i, keyword.clone(), condition.clone(), false));
            }
            continue;
        }
        stack.push((i, keyword, condition, false));
    }
    // Unterminated conditionals run to the end of the file.
    while let Some((open_line, open_kw, open_cond, earlier_taken)) = stack.pop() {
        let taken = branch_verdict(&open_kw, &open_cond, open_line, earlier_taken, &defines);
        out.push(Branch {
            start: open_line + 1,
            end: lines.len(),
            condition: open_cond,
            taken,
        });
    }
    out.sort_by_key(|b| (b.start, b.end));
    out
}

/// Whether one branch of a conditional group is compiled: `#else` is taken iff no
/// earlier branch was, and any branch after a taken one is dead.
fn branch_verdict(
    keyword: &str,
    condition: &str,
    at: usize,
    earlier_taken: bool,
    defines: &[(usize, String)],
) -> Option<bool> {
    if earlier_taken {
        // A preceding branch of the group already ran: this one cannot.
        return Some(false);
    }
    if keyword == "else" {
        // `earlier_taken` only records a *provably* taken branch, so a false value
        // may mean "no earlier branch ran" or "we could not tell". Undecidable, and
        // an undecidable branch is never hidden.
        return None;
    }
    eval_condition(keyword, condition, at, defines)
}

/// The line ranges `hide-ifdef-mode` hides and `cpp-highlight-buffer` shades:
/// the bodies of the branches the preprocessor provably skips.
pub fn dead_branches(lines: &[&str]) -> Vec<std::ops::Range<usize>> {
    conditional_branches(lines)
        .into_iter()
        .filter(|b| b.taken == Some(false) && b.start < b.end)
        .map(|b| b.start..b.end)
        .collect()
}

// ---------------------------------------------------------------------------
// `cwarn-mode`: suspicious C constructs.
// ---------------------------------------------------------------------------

/// One construct `cwarn-mode` flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CWarn {
    /// `if (a = b)` — an assignment where a comparison was almost certainly meant
    /// (Emacs `cwarn-font-lock-assignment-keywords`).
    AssignmentInCondition,
    /// `if (x);` — a semicolon that makes the body empty (Emacs
    /// `cwarn-font-lock-semicolon-keywords`).
    EmptyBodySemicolon,
}

/// A flagged construct: the line, the byte range within it, and what is wrong.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CWarning {
    /// Line index.
    pub line: usize,
    /// Byte range within the line.
    pub range: std::ops::Range<usize>,
    /// Which check fired.
    pub kind: CWarn,
}

/// The keywords whose parenthesised condition `cwarn-mode` inspects.
const CWARN_KEYWORDS: [&str; 3] = ["if", "while", "for"];

/// Scan `line` for the constructs `cwarn-mode` highlights.
///
/// Ports the two checks of GNU Emacs' `cwarn.el` that are language-neutral: an
/// assignment inside a condition, and a semicolon straight after a condition
/// (which silently empties the body). `cwarn.el`'s third check — a `&` in a C++
/// function call, warning that the argument is passed by reference — is not
/// ported: it needs the callee's declaration, which a line scan does not have.
pub fn cwarn_line(line: usize, src: &str) -> Vec<CWarning> {
    let mut out = Vec::new();
    let bytes = src.as_bytes();
    for keyword in CWARN_KEYWORDS {
        let mut from = 0usize;
        while let Some(rel) = src[from..].find(keyword) {
            let at = from + rel;
            from = at + keyword.len();
            // A keyword, not part of an identifier.
            let before_ok = at == 0 || !is_word_byte(bytes[at - 1]);
            if !before_ok {
                continue;
            }
            let after = &src[at + keyword.len()..];
            let paren_off = after.len() - after.trim_start().len();
            if !after[paren_off..].starts_with('(') {
                continue;
            }
            let open = at + keyword.len() + paren_off;
            let Some(close) = matching_paren(src, open) else {
                continue;
            };
            let condition = &src[open + 1..close];
            if keyword != "for" {
                if let Some(rel) = lone_assignment(condition) {
                    out.push(CWarning {
                        line,
                        range: open + 1 + rel..open + 2 + rel,
                        kind: CWarn::AssignmentInCondition,
                    });
                }
            }
            // `if (…) ;` — a semicolon is the whole body.
            let tail = &src[close + 1..];
            let semi_off = tail.len() - tail.trim_start().len();
            if tail[semi_off..].starts_with(';') {
                let semi = close + 1 + semi_off;
                out.push(CWarning {
                    line,
                    range: semi..semi + 1,
                    kind: CWarn::EmptyBodySemicolon,
                });
            }
        }
    }
    out.sort_by_key(|w| w.range.start);
    out
}

/// Every construct `cwarn-mode` flags in `lines`.
pub fn cwarn_scan(lines: &[&str]) -> Vec<CWarning> {
    lines
        .iter()
        .enumerate()
        .flat_map(|(i, line)| cwarn_line(i, line))
        .collect()
}

fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// The index of the `)` matching the `(` at `open`, or `None` when unbalanced.
fn matching_paren(src: &str, open: usize) -> Option<usize> {
    let mut depth = 0i32;
    for (i, c) in src.char_indices().skip_while(|(i, _)| *i < open) {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// The offset of a bare `=` in `condition` — an assignment, not `==`, `!=`, `<=`,
/// `>=`, `+=`, `-=`, `*=`, `/=`, `%=`, `&=`, `|=`, `^=` or a `<<=`/`>>=` tail.
/// A compound assignment in a condition is just as suspicious, but `cwarn.el`
/// only flags plain `=`, so this does too.
fn lone_assignment(condition: &str) -> Option<usize> {
    let b = condition.as_bytes();
    for (i, c) in b.iter().enumerate() {
        if *c != b'=' {
            continue;
        }
        if b.get(i + 1) == Some(&b'=') {
            continue;
        }
        let prev = if i == 0 { None } else { Some(b[i - 1]) };
        if matches!(
            prev,
            Some(b'=' | b'!' | b'<' | b'>' | b'+' | b'-' | b'*' | b'/' | b'%' | b'&' | b'|' | b'^')
        ) {
            continue;
        }
        return Some(i);
    }
    None
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

    #[test]
    fn if_zero_body_is_dead_and_if_one_body_is_not() {
        let lines = ["#if 0", "dead();", "#endif", "#if 1", "live();", "#endif"];
        assert_eq!(dead_branches(&lines), vec![1..2]);
    }

    #[test]
    fn the_else_of_a_taken_branch_is_dead() {
        let lines = ["#if 1", "live();", "#else", "dead();", "#endif"];
        assert_eq!(dead_branches(&lines), vec![3..4]);
    }

    #[test]
    fn ifdef_resolves_against_the_files_own_defines() {
        let lines = [
            "#define HAVE_X",
            "#ifdef HAVE_X",
            "yes();",
            "#else",
            "no();",
            "#endif",
            "#ifdef HAVE_Y",
            "maybe();",
            "#endif",
        ];
        // HAVE_X is defined -> the else is dead. HAVE_Y is not defined anywhere ->
        // `#ifdef HAVE_Y` is provably false, so its body is dead too.
        assert_eq!(dead_branches(&lines), vec![4..5, 7..8]);
    }

    #[test]
    fn ifndef_include_guard_body_is_live() {
        let lines = ["#ifndef FOO_H", "#define FOO_H", "body();", "#endif"];
        assert!(dead_branches(&lines).is_empty());
    }

    #[test]
    fn an_undecidable_condition_is_never_hidden() {
        let lines = ["#if VERSION > 3", "x();", "#else", "y();", "#endif"];
        assert!(
            dead_branches(&lines).is_empty(),
            "hiding code on a condition we cannot evaluate would blank out the file"
        );
    }

    #[test]
    fn defined_forms_are_evaluated() {
        let lines = [
            "#define A",
            "#if defined(A)",
            "one();",
            "#endif",
            "#if !defined(A)",
            "two();",
            "#endif",
        ];
        assert_eq!(dead_branches(&lines), vec![5..6]);
    }

    #[test]
    fn nested_dead_branches_are_reported_independently() {
        let lines = ["#if 0", "  #if 1", "  a();", "  #endif", "#endif", "b();"];
        let dead = dead_branches(&lines);
        assert!(dead.contains(&(1..4)), "{dead:?}");
    }

    #[test]
    fn branch_conditions_are_captured_for_display() {
        let lines = ["#if defined(A)", "x();", "#endif"];
        let branches = conditional_branches(&lines);
        assert_eq!(branches.len(), 1);
        assert_eq!(branches[0].condition, "defined(A)");
    }

    #[test]
    fn cwarn_flags_an_assignment_in_an_if_condition() {
        let warns = cwarn_line(0, "  if (a = b) {");
        assert_eq!(warns.len(), 1);
        assert_eq!(warns[0].kind, CWarn::AssignmentInCondition);
        assert_eq!(&"  if (a = b) {"[warns[0].range.clone()], "=");
    }

    #[test]
    fn cwarn_does_not_flag_comparisons_or_compound_assignment() {
        assert!(cwarn_line(0, "if (a == b) {").is_empty());
        assert!(cwarn_line(0, "if (a != b) {").is_empty());
        assert!(cwarn_line(0, "if (a <= b) {").is_empty());
        assert!(cwarn_line(0, "while (a >= b) {").is_empty());
    }

    #[test]
    fn cwarn_flags_an_empty_body_semicolon() {
        let warns = cwarn_line(3, "if (x);");
        assert_eq!(warns.len(), 1);
        assert_eq!(warns[0].kind, CWarn::EmptyBodySemicolon);
        assert_eq!(warns[0].line, 3);
        assert_eq!(&"if (x);"[warns[0].range.clone()], ";");
    }

    #[test]
    fn cwarn_ignores_a_for_loops_own_assignments() {
        // `for (i = 0; …)` is the idiom, not a mistake — cwarn only inspects
        // `if` and `while` conditions for assignment.
        let warns = cwarn_line(0, "for (i = 0; i < n; i++) {");
        assert!(
            warns.iter().all(|w| w.kind != CWarn::AssignmentInCondition),
            "{warns:?}"
        );
    }

    #[test]
    fn cwarn_flags_an_empty_for_body() {
        let warns = cwarn_line(0, "for (i = 0; i < n; i++);");
        assert_eq!(warns.len(), 1);
        assert_eq!(warns[0].kind, CWarn::EmptyBodySemicolon);
    }

    #[test]
    fn cwarn_does_not_fire_on_identifiers_ending_in_a_keyword() {
        assert!(cwarn_line(0, "notif (a = b);").is_empty());
        assert!(cwarn_line(0, "verify(a == b);").is_empty());
    }

    #[test]
    fn cwarn_scan_reports_every_line() {
        let lines = ["if (a = 1) {", "}", "while (b = 2);"];
        let warns = cwarn_scan(&lines);
        assert_eq!(warns.len(), 3, "{warns:?}");
        assert_eq!(warns[0].line, 0);
        assert!(warns.iter().filter(|w| w.line == 2).count() == 2);
    }

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
