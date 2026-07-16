//! Directory-local variables — the zmax port of the GNU Emacs `files.el`
//! `.dir-locals.el` machinery (`add-dir-local-variable`,
//! `delete-dir-local-variable`, `copy-file-locals-to-dir-locals` and their
//! inverses, plus `dir-locals-set-class-variables` /
//! `dir-locals-set-directory-class`).
//!
//! A `.dir-locals.el` is one elisp alist keyed by *mode*:
//!
//! ```text
//! ((nil . ((fill-column . 80)))
//!  (c-mode . ((indent-tabs-mode . t)
//!             (tab-width . 8))))
//! ```
//!
//! `nil` applies to every file in the tree; a mode symbol only to files in that
//! mode. Emacs merges the `.dir-locals.el` of every directory from the root down
//! to the file's own, the deepest winning, and merges *directory classes* —
//! variable sets registered under a name and bound to a directory at runtime —
//! into the same lookup.
//!
//! This module is the pure, tested core: parse, render, set, delete, merge. It
//! performs no I/O; the command layer reads and writes the files.

/// One `.dir-locals.el` entry: a mode (`nil` for "any mode", or a mode symbol /
/// subdirectory string) and the variables it sets, in file order.
pub type Entry = (String, Vec<(String, String)>);

/// Parse a `.dir-locals.el`. Tolerant by design — an unreadable or malformed file
/// yields the entries it could make sense of, never an error, because Emacs's own
/// reader ignores what it cannot use and a half-written file must not wedge the
/// editor.
///
/// Recognises the alist shape `((MODE . ((VAR . VAL) …)) …)`, with `.` optional
/// between a var and its value (Emacs accepts `(VAR . VAL)`; a bare `(VAR VAL)`
/// is read the same way here). Comments (`;` to end of line) are stripped.
pub fn parse(text: &str) -> Vec<Entry> {
    let toks = tokenize(text);
    let mut i = 0usize;
    let Some(top) = read_form(&toks, &mut i) else {
        return Vec::new();
    };
    let Form::List(entries) = top else {
        return Vec::new();
    };
    entries
        .into_iter()
        .filter_map(|entry| {
            let Form::List(parts) = entry else {
                return None;
            };
            let mut parts = parts.into_iter().filter(|f| !f.is_dot());
            let mode = match parts.next()? {
                Form::Atom(a) => a,
                Form::List(_) | Form::Dot => return None,
            };
            let mut vars = Vec::new();
            for rest in parts {
                if let Form::List(pairs) = rest {
                    for pair in pairs {
                        if let Form::List(kv) = pair {
                            let mut kv = kv.into_iter().filter(|f| !f.is_dot());
                            if let (Some(Form::Atom(k)), Some(Form::Atom(v))) =
                                (kv.next(), kv.next())
                            {
                                vars.push((k, v));
                            }
                        }
                    }
                }
            }
            Some((mode, vars))
        })
        .collect()
}

/// Render entries back to `.dir-locals.el` source, one variable per line — the
/// layout Emacs's own `add-dir-local-variable` produces, so the file stays
/// hand-editable.
pub fn render(entries: &[Entry]) -> String {
    if entries.is_empty() {
        return String::new();
    }
    let mut out = String::from("(");
    for (i, (mode, vars)) in entries.iter().enumerate() {
        if i > 0 {
            out.push_str("\n ");
        }
        out.push_str(&format!("({mode} . ("));
        for (j, (k, v)) in vars.iter().enumerate() {
            if j > 0 {
                out.push_str("\n         ");
            }
            out.push_str(&format!("({k} . {v})"));
        }
        out.push_str("))");
    }
    out.push_str(")\n");
    out
}

/// `add-dir-local-variable`: set `var` to `val` for `mode`, creating the mode's
/// entry if the file has none. Returns the new file text.
pub fn set_var(text: &str, mode: &str, var: &str, val: &str) -> String {
    let mut entries = parse(text);
    match entries.iter_mut().find(|(m, _)| m == mode) {
        Some((_, vars)) => match vars.iter_mut().find(|(k, _)| k == var) {
            Some(slot) => slot.1 = val.to_string(),
            None => vars.push((var.to_string(), val.to_string())),
        },
        None => entries.push((mode.to_string(), vec![(var.to_string(), val.to_string())])),
    }
    render(&entries)
}

/// `delete-dir-local-variable`: remove `var` from `mode`'s entry, dropping the
/// entry when it empties.
pub fn delete_var(text: &str, mode: &str, var: &str) -> String {
    let mut entries = parse(text);
    if let Some(e) = entries.iter_mut().find(|(m, _)| m == mode) {
        e.1.retain(|(k, _)| k != var);
    }
    entries.retain(|(_, vars)| !vars.is_empty());
    render(&entries)
}

/// The variables that apply to a file in `mode`: the `nil` (any-mode) entry first,
/// then the mode's own, later definitions of the same variable winning — Emacs's
/// `hack-dir-local-variables` precedence.
pub fn variables_for(entries: &[Entry], mode: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for (m, vars) in entries {
        if m != "nil" && m != mode {
            continue;
        }
        for (k, v) in vars {
            out.retain(|(existing, _)| existing != k);
            out.push((k.clone(), v.clone()));
        }
    }
    out
}

/// Merge the `.dir-locals.el` of a directory chain — outermost first, innermost
/// last — into one variable set for `mode`. The deeper the directory, the higher
/// the precedence, which is exactly Emacs's rule.
pub fn merge_chain(chain: &[Vec<Entry>], mode: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for entries in chain {
        for (k, v) in variables_for(entries, mode) {
            out.retain(|(existing, _)| *existing != k);
            out.push((k, v));
        }
    }
    out
}

// ── a very small s-expression reader ────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
enum Form {
    Atom(String),
    List(Vec<Form>),
    /// The `.` of a dotted pair, kept as a token so `(a . b)` and `(a b)` read
    /// alike after it is filtered out.
    Dot,
}

impl Form {
    fn is_dot(&self) -> bool {
        matches!(self, Form::Dot)
    }
}

/// Split `.dir-locals.el` source into parens, strings and bare atoms, dropping
/// `;` comments.
fn tokenize(text: &str) -> Vec<String> {
    let mut toks = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            ';' => {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            '(' | ')' => {
                toks.push(c.to_string());
                i += 1;
            }
            '"' => {
                let start = i;
                i += 1;
                while i < chars.len() && chars[i] != '"' {
                    i += if chars[i] == '\\' { 2 } else { 1 };
                }
                i = (i + 1).min(chars.len());
                toks.push(chars[start..i].iter().collect());
            }
            c if c.is_whitespace() => i += 1,
            _ => {
                let start = i;
                while i < chars.len()
                    && !chars[i].is_whitespace()
                    && chars[i] != '('
                    && chars[i] != ')'
                    && chars[i] != ';'
                {
                    i += 1;
                }
                toks.push(chars[start..i].iter().collect());
            }
        }
    }
    toks
}

/// Read one form from `toks` starting at `i`. `None` at end of input or on an
/// unbalanced `)`.
fn read_form(toks: &[String], i: &mut usize) -> Option<Form> {
    let tok = toks.get(*i)?;
    if tok == ")" {
        return None;
    }
    if tok == "(" {
        *i += 1;
        let mut items = Vec::new();
        while let Some(t) = toks.get(*i) {
            if t == ")" {
                *i += 1;
                return Some(Form::List(items));
            }
            items.push(read_form(toks, i)?);
        }
        return None; // unbalanced
    }
    *i += 1;
    Some(if tok == "." {
        Form::Dot
    } else {
        Form::Atom(tok.clone())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
;; my project
((nil . ((fill-column . 80)
         (indent-tabs-mode . nil)))
 (c-mode . ((tab-width . 8))))
";

    /// The alist reads back as mode → variables, comments and dots ignored.
    #[test]
    fn parses_the_emacs_alist() {
        let e = parse(SAMPLE);
        assert_eq!(e.len(), 2);
        assert_eq!(e[0].0, "nil");
        assert_eq!(
            e[0].1,
            vec![
                ("fill-column".to_string(), "80".to_string()),
                ("indent-tabs-mode".to_string(), "nil".to_string()),
            ]
        );
        assert_eq!(e[1].0, "c-mode");
        assert_eq!(e[1].1, vec![("tab-width".to_string(), "8".to_string())]);
    }

    /// Garbage in, empty out — never a panic, never an error.
    #[test]
    fn parse_tolerates_malformed_input() {
        assert!(parse("").is_empty());
        assert!(parse("((nil . ((a . 1))").is_empty(), "unbalanced");
        assert!(parse("not-a-list").is_empty());
    }

    /// A written file parses back to what was written (round trip), and setting
    /// an existing variable updates it in place rather than duplicating it.
    #[test]
    fn set_var_round_trips_and_updates_in_place() {
        let once = set_var(SAMPLE, "nil", "fill-column", "100");
        let e = parse(&once);
        assert_eq!(variables_for(&e, "nil").len(), 2, "no duplicate added");
        assert_eq!(
            variables_for(&e, "nil")
                .iter()
                .find(|(k, _)| k == "fill-column")
                .map(|(_, v)| v.as_str()),
            Some("100")
        );
        // A new mode gets its own entry.
        let added = set_var(&once, "rust-mode", "tab-width", "4");
        let e = parse(&added);
        assert_eq!(e.len(), 3);
        assert_eq!(
            variables_for(&e, "rust-mode"),
            vec![
                ("fill-column".to_string(), "100".to_string()),
                ("indent-tabs-mode".to_string(), "nil".to_string()),
                ("tab-width".to_string(), "4".to_string()),
            ],
            "the nil entry applies to every mode, the mode's own is layered on top"
        );
    }

    /// Deleting the last variable of a mode removes the mode entry with it.
    #[test]
    fn delete_var_prunes_empty_entries() {
        let out = delete_var(SAMPLE, "c-mode", "tab-width");
        let e = parse(&out);
        assert_eq!(e.len(), 1, "the emptied c-mode entry is gone");
        assert_eq!(e[0].0, "nil");
        // Deleting something absent is a no-op, not a corruption.
        assert_eq!(parse(&delete_var(&out, "nil", "nope")).len(), 1);
    }

    /// The deeper directory wins when the same variable is set twice.
    #[test]
    fn merge_chain_lets_the_deepest_directory_win() {
        let root = parse("((nil . ((fill-column . 80) (a . 1))))");
        let inner = parse("((nil . ((fill-column . 100))))");
        assert_eq!(
            merge_chain(&[root, inner], "nil"),
            vec![
                ("a".to_string(), "1".to_string()),
                ("fill-column".to_string(), "100".to_string()),
            ]
        );
    }
}
