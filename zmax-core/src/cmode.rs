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

/// The `hide-ifdef-env` a scan evaluates against, on top of the file's own
/// `#define`s: the symbols `hide-ifdef-define` has set (hideif.el:2638-2655 —
/// `(hif-set-var var (or val 1))`, so an omitted value is `1`) and the ones
/// `hide-ifdef-undef` has removed (hideif.el:2666-2683).
///
/// `hide-ifdef-undef` needs the `undefined` list because this port also consults
/// the file's own `#define`s, which hideif.el does not: without it `C-c @ u` on a
/// macro the file defines would be a no-op, and its docstring is "Undefine a VAR
/// so that #ifdef VAR would not be included".
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HideIfdefEnv {
    /// `(VAR, VALUE)` pairs. `hif-set-var` *prepends*, so a later `define` of the
    /// same name shadows the earlier one (hideif.el:500-502); this replaces in
    /// place, which is the same lookup result with no dead entries kept.
    defined: Vec<(String, String)>,
    /// Names `hide-ifdef-undef` removed, which shadow a file `#define`.
    undefined: Vec<String>,
}

impl HideIfdefEnv {
    /// `hide-ifdef-define VAR [VAL]`: define `name`, defaulting the value to `1`.
    pub fn define(&mut self, name: &str, value: Option<&str>) {
        let value = value.unwrap_or("1").to_string();
        self.undefined.retain(|u| u != name);
        match self.defined.iter_mut().find(|(n, _)| n == name) {
            Some(slot) => slot.1 = value,
            None => self.defined.push((name.to_string(), value)),
        }
    }

    /// `hide-ifdef-undef VAR`: drop `name` from the env and mask any `#define` of
    /// it in the file.
    pub fn undef(&mut self, name: &str) {
        self.defined.retain(|(n, _)| n != name);
        if !self.undefined.iter().any(|u| u == name) {
            self.undefined.push(name.to_string());
        }
    }

    /// Whether the env decides `name`'s definedness at all. `None` leaves the
    /// verdict to the file's own `#define`s, which is why an empty env keeps the
    /// pre-env behaviour exactly.
    pub fn defined(&self, name: &str) -> Option<bool> {
        if self.defined.iter().any(|(n, _)| n == name) {
            Some(true)
        } else if self.undefined.iter().any(|u| u == name) {
            Some(false)
        } else {
            None
        }
    }

    /// `hif-lookup`: the value `hide-ifdef-define` set, for a bare `#if VAR`.
    pub fn value(&self, name: &str) -> Option<&str> {
        self.defined
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    pub fn is_empty(&self) -> bool {
        self.defined.is_empty() && self.undefined.is_empty()
    }
}

/// Where each macro in `lines` is `#define`d. Emacs takes its whole definition
/// environment from `hide-ifdef-env` instead; this port consults the file's own
/// `#define`s as well, so that a scan with no env at all still decides the
/// common cases (see [`HideIfdefEnv`]).
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

/// Whether `name` is defined for a directive on line `before`: the `hide-ifdef-env`
/// wins (that is the environment the user typed, and `hide-ifdef-undef` must be
/// able to mask a file `#define`), otherwise the file's own `#define`s decide.
fn is_defined(defines: &[(usize, String)], env: &HideIfdefEnv, before: usize, name: &str) -> bool {
    if let Some(verdict) = env.defined(name) {
        return verdict;
    }
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
/// GNU Emacs' `hide-ifdef-mode` instead evaluates against `hide-ifdef-env` alone
/// and hides every branch that is not true, so with the default (empty) env it
/// hides the body of every `#ifdef`. Hiding on "cannot tell" would blank out most
/// of a real C file, so `env` is consulted *in addition to* the file's own
/// `#define`s and undecidable branches stay visible.
fn eval_condition(
    keyword: &str,
    condition: &str,
    at: usize,
    defines: &[(usize, String)],
    env: &HideIfdefEnv,
) -> Option<bool> {
    let cond = condition.trim();
    match keyword {
        "ifdef" | "elifdef" => return Some(is_defined(defines, env, at, cond)),
        "ifndef" | "elifndef" => return Some(!is_defined(defines, env, at, cond)),
        _ => {}
    }
    // `#if 0` / `#if 1` — the idiomatic "comment this out".
    if let Ok(n) = cond.parse::<i64>() {
        return Some(n != 0);
    }
    // A bare `#if VAR` whose value the env carries: `hif-lookup` substitutes it
    // and the expression is then the literal above (hideif.el:519-527).
    if let Some(n) = env.value(cond).and_then(|v| v.trim().parse::<i64>().ok()) {
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
    let value = is_defined(defines, env, at, name);
    Some(value != negated)
}

/// Split every preprocessor conditional in `lines` into its branches, deciding
/// which are compiled. Nested conditionals produce nested branches (each is
/// reported independently); a branch inside a dead branch is reported with its
/// own verdict, so callers that hide dead code hide the outer body anyway.
///
/// This is the engine behind `hide-ifdef-mode` (hide the dead ones) and
/// `cpp-highlight-buffer` (shade them). Evaluated against the file's own
/// `#define`s only; see [`conditional_branches_with_env`] for the
/// `hide-ifdef-define` environment.
pub fn conditional_branches(lines: &[&str]) -> Vec<Branch> {
    conditional_branches_with_env(lines, &HideIfdefEnv::default())
}

/// [`conditional_branches`] evaluated against `env` as well as the file's own
/// `#define`s — the `hide-ifdef-env` that `hide-ifdef-define` and
/// `hide-ifdef-undef` populate (hideif.el:272). An empty `env` decides nothing,
/// so it reproduces [`conditional_branches`] exactly.
pub fn conditional_branches_with_env(lines: &[&str], env: &HideIfdefEnv) -> Vec<Branch> {
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
                let taken = branch_verdict(
                    &open_kw,
                    &open_cond,
                    open_line,
                    earlier_taken,
                    &defines,
                    env,
                );
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
        let taken = branch_verdict(
            &open_kw,
            &open_cond,
            open_line,
            earlier_taken,
            &defines,
            env,
        );
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
    env: &HideIfdefEnv,
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
    eval_condition(keyword, condition, at, defines, env)
}

/// The line ranges `hide-ifdef-mode` hides and `cpp-highlight-buffer` shades:
/// the bodies of the branches the preprocessor provably skips.
pub fn dead_branches(lines: &[&str]) -> Vec<std::ops::Range<usize>> {
    dead_branches_with_env(lines, &HideIfdefEnv::default())
}

/// [`dead_branches`] resolved against a `hide-ifdef-env` as well: the symbols
/// `hide-ifdef-define` / `hide-ifdef-undef` set are what emacs's `hide-ifdefs`
/// evaluates against ("Assume that defined symbols have been added to
/// `hide-ifdef-env'", hideif.el:2685-2688). An empty `env` decides nothing, so
/// this reproduces [`dead_branches`] exactly.
pub fn dead_branches_with_env(lines: &[&str], env: &HideIfdefEnv) -> Vec<std::ops::Range<usize>> {
    conditional_branches_with_env(lines, env)
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
    /// `void f(int &x)` — a `&` in a top-level parameter list, so the argument is
    /// silently passed by reference and the callee can write through it (Emacs
    /// `cwarn-font-lock-reference-keywords`). C++ only.
    ReferenceParameter,
}

/// Which entry of `cwarn-configuration` (cwarn.el:117-119) a scan runs under.
/// The default configuration is `((c-mode (not reference)) (c++-mode t))`, so the
/// pass-by-reference check is the one feature that depends on the language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CWarnLang {
    /// `(c-mode (not reference))`: every check except [`CWarn::ReferenceParameter`].
    C,
    /// `(c++-mode t)`: every check.
    Cpp,
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

/// Scan `line` for the constructs `cwarn-mode` highlights within a single line:
/// an assignment inside a condition, and a semicolon straight after a condition
/// (which silently empties the body). `cwarn.el`'s third check, the
/// pass-by-reference `&`, needs the enclosing list and brace nesting and so lives
/// in `cwarn_reference_scan`, which walks the whole buffer.
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

/// Every construct `cwarn-mode` flags in `lines` under the C configuration
/// (`(c-mode (not reference))`) — i.e. every check but pass-by-reference. Use
/// [`cwarn_scan_lang`] to pick the language.
pub fn cwarn_scan(lines: &[&str]) -> Vec<CWarning> {
    cwarn_scan_lang(lines, CWarnLang::C)
}

/// Every construct `cwarn-mode` flags in `lines`, in buffer order, with the
/// feature set `cwarn-configuration` gives `lang`.
pub fn cwarn_scan_lang(lines: &[&str], lang: CWarnLang) -> Vec<CWarning> {
    let mut out: Vec<CWarning> = lines
        .iter()
        .enumerate()
        .flat_map(|(i, line)| cwarn_line(i, line))
        .collect();
    if lang == CWarnLang::Cpp {
        out.extend(cwarn_reference_scan(lines));
    }
    out.sort_by_key(|w| (w.line, w.range.start));
    out
}

/// The pass-by-reference `&`s of `cwarn-font-lock-match-reference` (cwarn.el:305):
///
/// ```text
/// (cwarn-font-lock-match
///  "[^&]\\(&\\)[^&=]"
///  (backward-up-list 1)
///  (and (eq (following-char) ?\()
///       (not (cwarn-inside-macro))
///       (c-at-toplevel-p)))
/// ```
///
/// Purely syntactic — nothing about the callee is consulted. The regexp picks a
/// lone `&` (a character on each side, neither of them `&`, and the one after not
/// `=`), then the innermost enclosing list must be a `(`, the point must not be
/// inside a macro, and `c-at-toplevel-p` must hold; together that is a `&` in the
/// parameter list of a declaration written where a function may be written.
///
/// `backward-up-list` and `c-at-toplevel-p` read the whole buffer, so this walks
/// `lines` carrying a delimiter stack, skipping comments and literals so a `(` or
/// `&` inside one neither nests nor fires. `c-at-toplevel-p` (cc-engine.el:12188)
/// is "outside any enclosing block … or directly inside a class, namespace or
/// other block that contains another declaration level"; a `{` is therefore
/// recorded as a declaration level when the statement that opened it named one of
/// [`DECL_BLOCK_KEYWORDS`], and top level means every open `{` is one of those.
fn cwarn_reference_scan(lines: &[&str]) -> Vec<CWarning> {
    let mut out = Vec::new();
    let mut delims: Vec<u8> = Vec::new(); // innermost-last stack of `(`, `[`, `{`
    let mut decl_level: Vec<bool> = Vec::new(); // one entry per open `{`
    let mut stmt = String::new(); // text since the last `;`, `{` or `}`
    let mut in_block_comment = false;
    for (i, line) in lines.iter().enumerate() {
        let in_macro = in_cpp_macro(lines, i);
        let b = line.as_bytes();
        let mut j = 0usize;
        while j < b.len() {
            let c = b[j];
            if in_block_comment {
                if c == b'*' && b.get(j + 1) == Some(&b'/') {
                    in_block_comment = false;
                    j += 1;
                }
                j += 1;
                continue;
            }
            match c {
                b'/' if b.get(j + 1) == Some(&b'/') => break,
                b'/' if b.get(j + 1) == Some(&b'*') => {
                    in_block_comment = true;
                    j += 2;
                    continue;
                }
                b'"' | b'\'' => {
                    j = skip_literal(b, j);
                    continue;
                }
                b'(' | b'[' => {
                    delims.push(c);
                    stmt.push(c as char);
                }
                b'{' => {
                    delims.push(c);
                    decl_level.push(DECL_BLOCK_KEYWORDS.iter().any(|k| contains_word(&stmt, k)));
                    stmt.clear();
                }
                b')' | b']' => {
                    delims.pop();
                }
                b'}' => {
                    delims.pop();
                    decl_level.pop();
                    stmt.clear();
                }
                b';' => stmt.clear(),
                b'&' => {
                    let prev = j.checked_sub(1).map(|p| b[p]);
                    let next = b.get(j + 1).copied();
                    let lone = prev.is_some_and(|p| p != b'&')
                        && next.is_some_and(|n| n != b'&' && n != b'=');
                    if lone
                        && delims.last() == Some(&b'(')
                        && !in_macro
                        && decl_level.iter().all(|top| *top)
                    {
                        out.push(CWarning {
                            line: i,
                            range: j..j + 1,
                            kind: CWarn::ReferenceParameter,
                        });
                    }
                    stmt.push('&');
                }
                // Non-ASCII bytes become a space so a UTF-8 identifier cannot
                // glue itself onto one of the keywords looked for above.
                _ => stmt.push(if c.is_ascii() { c as char } else { ' ' }),
            }
            j += 1;
        }
    }
    out
}

/// The block openers that keep `c-at-toplevel-p` true: a brace that introduces
/// another declaration level rather than a body of statements.
const DECL_BLOCK_KEYWORDS: [&str; 5] = ["class", "struct", "union", "namespace", "extern"];

/// Index just past the string or character literal opening at `at`, or the end of
/// the line when it is unterminated.
fn skip_literal(b: &[u8], at: usize) -> usize {
    let quote = b[at];
    let mut i = at + 1;
    while i < b.len() {
        match b[i] {
            b'\\' => i += 2,
            c if c == quote => return i + 1,
            _ => i += 1,
        }
    }
    b.len()
}

/// True when `hay` contains `word` delimited as a whole identifier.
fn contains_word(hay: &str, word: &str) -> bool {
    let hb = hay.as_bytes();
    let mut from = 0usize;
    while let Some(rel) = hay[from..].find(word) {
        let at = from + rel;
        from = at + word.len();
        let before = at == 0 || !is_word_byte(hb[at - 1]);
        let after = hb.get(at + word.len()).is_none_or(|b| !is_word_byte(*b));
        if before && after {
            return true;
        }
    }
    false
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

    /// hideif.el:2638-2655 `(defun hide-ifdef-define (var &optional val)` —
    /// "Define a VAR to VAL (default 1) in `hide-ifdef-env'. This allows #ifndef
    /// VAR to be hidden." — with `(hif-set-var var (or val 1))` as the body.
    #[test]
    fn hide_ifdef_define_decides_a_macro_the_file_never_defines() {
        let lines = ["#ifdef HAVE_X", "yes();", "#else", "no();", "#endif"];
        // Without an env the file's own defines rule: HAVE_X is nowhere, so the
        // `#ifdef` body is dead and the `#else` lives.
        assert_eq!(dead_branches(&lines), vec![1..2]);

        let mut env = HideIfdefEnv::default();
        env.define("HAVE_X", None);
        // With HAVE_X in `hide-ifdef-env` the verdict flips to the `#else`, and
        // `#ifndef HAVE_X` would now be hidden, which is what the docstring
        // promises.
        assert_eq!(dead_branches_with_env(&lines, &env), vec![3..4]);
    }

    /// hideif.el:2666-2683 `(defun hide-ifdef-undef ...)` — "Undefine a VAR so
    /// that #ifdef VAR would not be included." The file `#define`s HAVE_X, which
    /// hideif.el would not even look at, so the undef has to mask it or the
    /// command does nothing here.
    #[test]
    fn hide_ifdef_undef_masks_the_files_own_define() {
        let lines = [
            "#define HAVE_X",
            "#ifdef HAVE_X",
            "yes();",
            "#else",
            "no();",
            "#endif",
        ];
        assert_eq!(dead_branches(&lines), vec![4..5]);

        let mut env = HideIfdefEnv::default();
        env.undef("HAVE_X");
        assert_eq!(dead_branches_with_env(&lines, &env), vec![2..3]);
    }

    /// hideif.el:519-527 `hif-lookup` returns the value stored in
    /// `hide-ifdef-env`, which `hif-expand-token`/`hif-mathify` then evaluate —
    /// so a bare `#if VERSION` is decidable once VERSION has a value.
    #[test]
    fn a_defined_value_decides_a_bare_if() {
        let lines = ["#if VERSION", "x();", "#endif"];
        assert!(
            dead_branches(&lines).is_empty(),
            "with no env the value is unknown, so nothing may be hidden"
        );

        let mut env = HideIfdefEnv::default();
        env.define("VERSION", Some("0"));
        assert_eq!(dead_branches_with_env(&lines, &env), vec![1..2]);

        env.define("VERSION", Some("3"));
        assert!(dead_branches_with_env(&lines, &env).is_empty());
    }

    /// `hide-ifdef-env` is nil by default (hideif.el:272), and this port must
    /// keep deciding branches from the file when it is — otherwise every existing
    /// `dead_branches` caller changes behaviour.
    #[test]
    fn an_empty_env_reproduces_the_file_only_verdicts() {
        let lines = [
            "#define A",
            "#if defined(A)",
            "one();",
            "#endif",
            "#if !defined(A)",
            "two();",
            "#endif",
        ];
        let env = HideIfdefEnv::default();
        assert!(env.is_empty());
        assert_eq!(dead_branches_with_env(&lines, &env), dead_branches(&lines));
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

    /// `cwarn-font-lock-match-reference` (cwarn.el:301-311) is
    /// `(cwarn-font-lock-match "[^&]\\(&\\)[^&=]" (backward-up-list 1)
    /// (and (eq (following-char) ?\() (not (cwarn-inside-macro)) (c-at-toplevel-p)))`,
    /// and `cwarn-configuration` (cwarn.el:117-119) is
    /// `((c-mode (not reference)) (c++-mode t))` — so the check is C++-only.
    #[test]
    fn cwarn_flags_a_reference_parameter_in_cpp_only() {
        let lines = ["void f(int &x);"];
        let warns = cwarn_scan_lang(&lines, CWarnLang::Cpp);
        assert_eq!(warns.len(), 1, "{warns:?}");
        assert_eq!(warns[0].kind, CWarn::ReferenceParameter);
        // group 1 of the regexp is the `&` itself
        assert_eq!(warns[0].range, 11..12);
        assert_eq!(&lines[0][warns[0].range.clone()], "&");
        // `(c-mode (not reference))`: the same source is clean in C.
        assert!(cwarn_scan_lang(&lines, CWarnLang::C).is_empty());
        assert!(cwarn_scan(&lines).is_empty());
    }

    /// The regexp `"[^&]\\(&\\)[^&=]"` (cwarn.el:307) excludes `&&` and `&=`, and
    /// needs a character on each side of the `&`.
    #[test]
    fn cwarn_reference_ignores_and_and_compound_assignment() {
        for src in ["void f(int a && b);", "void f(int a &= b);"] {
            assert!(cwarn_scan_lang(&[src], CWarnLang::Cpp).is_empty(), "{src}");
        }
    }

    /// `(eq (following-char) ?\()` after `backward-up-list 1`: the innermost
    /// enclosing list must be a paren, and `(not (cwarn-inside-macro))`
    /// (cwarn.el:203) drops anything inside a `#define`.
    #[test]
    fn cwarn_reference_needs_a_paren_and_no_macro() {
        // innermost list is a brace, not a paren
        assert!(cwarn_scan_lang(&["int a[] = {1 & 2};"], CWarnLang::Cpp).is_empty());
        // inside a macro definition, including its backslash continuation
        let macro_lines = ["#define F(x) \\", "    g(x & 1)"];
        assert!(
            cwarn_scan_lang(&macro_lines, CWarnLang::Cpp).is_empty(),
            "{:?}",
            cwarn_scan_lang(&macro_lines, CWarnLang::Cpp)
        );
    }

    /// `c-at-toplevel-p` (cc-engine.el:12188) is "outside any enclosing block …
    /// or directly inside a class, namespace or other block that contains another
    /// declaration level", so a `&` in a function body is not a parameter.
    #[test]
    fn cwarn_reference_respects_toplevel() {
        // a bitwise `and` in a statement body: not at top level, not flagged
        let body = ["void f() {", "    g(a & b);", "}"];
        assert!(cwarn_scan_lang(&body, CWarnLang::Cpp).is_empty());
        // a member declaration is at top level: the class brace is a declaration level
        let member = ["class C {", "    void f(int &x);", "};"];
        let warns = cwarn_scan_lang(&member, CWarnLang::Cpp);
        assert_eq!(warns.len(), 1, "{warns:?}");
        assert_eq!(warns[0].line, 1);
        assert_eq!(warns[0].range, 15..16);
        // …but a body nested inside that class is not
        let inline = ["class C {", "    void f() { g(a & b); }", "};"];
        assert!(cwarn_scan_lang(&inline, CWarnLang::Cpp).is_empty());
    }

    #[test]
    fn cwarn_reference_skips_comments_and_literals() {
        // an unbalanced `(` in a comment or a literal must not open a list, and a
        // `&` inside one must not fire
        let lines = [
            "// void f(int &x);",
            "const char *s = \"f(int &x)\";",
            "/* void f(int &x); */",
            "void g(int &y);",
        ];
        let warns = cwarn_scan_lang(&lines, CWarnLang::Cpp);
        assert_eq!(warns.len(), 1, "{warns:?}");
        assert_eq!(warns[0].line, 3);
        assert_eq!(warns[0].range, 11..12);
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
