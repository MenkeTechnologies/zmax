//! File-local variables — the zemacs port of the GNU Emacs `files.el` commands
//! `add-file-local-variable`, `add-file-local-variable-prop-line`,
//! `delete-file-local-variable` and the `-prop-line` deleter.
//!
//! Two places hold file-local variables:
//!   * the **prop line** — the first line's `-*- key: val; key2: val2 -*-`
//!     (second line when the first is a `#!` shebang);
//!   * the **Local Variables block** near end of file:
//!     ```text
//!     Local Variables:
//!     key: val
//!     End:
//!     ```
//!     each line wrapped in the buffer's comment syntax.
//!
//! This module is the pure, tested core: it edits a file's text given the
//! comment `prefix`/`suffix` (the command layer supplies them from the buffer's
//! language). No I/O.

/// The line the prop line lives on: the first line, or the second when the first
/// is a `#!` shebang. Returns `(index, lines)`.
fn prop_line_index(lines: &[&str]) -> usize {
    if lines.first().is_some_and(|l| l.starts_with("#!")) {
        1
    } else {
        0
    }
}

/// The parsed content of a `-*- … -*-` prop line: `(prefix, pairs, suffix)`.
type PropLine = (String, Vec<(String, String)>, String);

/// Parse the `-*- … -*-` content of `line` into `(prefix, pairs, suffix)`, where
/// `prefix`/`suffix` are the text outside the markers and `pairs` are the
/// `key: value` entries. A bare `-*- modename -*-` becomes `[("mode", modename)]`.
fn parse_prop_line(line: &str) -> Option<PropLine> {
    let open = line.find("-*-")?;
    let after = open + 3;
    let close_rel = line[after..].find("-*-")?;
    let inner = line[after..after + close_rel].trim();
    let prefix = line[..open].to_string();
    let suffix = line[after + close_rel + 3..].to_string();
    let pairs = if inner.contains(':') {
        inner
            .split(';')
            .filter_map(|kv| {
                let (k, v) = kv.split_once(':')?;
                Some((k.trim().to_string(), v.trim().to_string()))
            })
            .collect()
    } else if inner.is_empty() {
        Vec::new()
    } else {
        // Bare mode form: `-*- lisp -*-`.
        vec![("mode".to_string(), inner.to_string())]
    };
    Some((prefix, pairs, suffix))
}

/// Render `pairs` back to a prop line body: `key: val; key2: val2`.
fn render_pairs(pairs: &[(String, String)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| format!("{k}: {v}"))
        .collect::<Vec<_>>()
        .join("; ")
}

/// Whether `content` ends with a trailing newline (preserved on edit).
fn split_lines(content: &str) -> (Vec<&str>, bool) {
    let trailing = content.ends_with('\n');
    let body = if trailing {
        &content[..content.len() - 1]
    } else {
        content
    };
    (body.split('\n').collect(), trailing)
}

fn join_lines(lines: &[String], trailing: bool) -> String {
    let mut out = lines.join("\n");
    if trailing {
        out.push('\n');
    }
    out
}

/// Emacs `add-file-local-variable-prop-line`: set `var` to `val` in the first
/// line's `-*- … -*-` prop line, creating the prop line (comment-wrapped with
/// `prefix`/`suffix`) if none exists.
pub fn set_prop_line(content: &str, var: &str, val: &str, prefix: &str, suffix: &str) -> String {
    let (lines, trailing) = split_lines(content);
    let idx = prop_line_index(&lines);
    let mut out: Vec<String> = lines.iter().map(|s| s.to_string()).collect();
    if let Some(line) = lines.get(idx) {
        if let Some((pre, mut pairs, suf)) = parse_prop_line(line) {
            match pairs.iter_mut().find(|(k, _)| k == var) {
                Some(pair) => pair.1 = val.to_string(),
                None => pairs.push((var.to_string(), val.to_string())),
            }
            out[idx] = format!("{pre}-*- {} -*-{suf}", render_pairs(&pairs));
            return join_lines(&out, trailing);
        }
    }
    // No prop line: insert one at the target position.
    let new = format!("{prefix}-*- {var}: {val} -*-{suffix}");
    out.insert(idx, new);
    join_lines(&out, trailing)
}

/// Emacs `delete-file-local-variable-prop-line`: remove `var` from the prop
/// line; if that empties the prop line, remove the line entirely.
pub fn delete_prop_line(content: &str, var: &str) -> String {
    let (lines, trailing) = split_lines(content);
    let idx = prop_line_index(&lines);
    let mut out: Vec<String> = lines.iter().map(|s| s.to_string()).collect();
    if let Some(line) = lines.get(idx) {
        if let Some((pre, mut pairs, suf)) = parse_prop_line(line) {
            pairs.retain(|(k, _)| k != var);
            if pairs.is_empty() {
                out.remove(idx);
            } else {
                out[idx] = format!("{pre}-*- {} -*-{suf}", render_pairs(&pairs));
            }
        }
    }
    join_lines(&out, trailing)
}

/// Find the `Local Variables:` / `End:` block. Returns `(start, end, prefix,
/// suffix)` where `start`/`end` are the block-delimiter line indices.
fn find_local_block(lines: &[String]) -> Option<(usize, usize, String, String)> {
    let start = lines.iter().position(|l| l.contains("Local Variables:"))?;
    let head = &lines[start];
    let at = head.find("Local Variables:").unwrap();
    let prefix = head[..at].to_string();
    let suffix = head[at + "Local Variables:".len()..].to_string();
    let end = lines
        .iter()
        .enumerate()
        .skip(start + 1)
        .find(|(_, l)| l.contains("End:"))
        .map(|(i, _)| i)?;
    Some((start, end, prefix, suffix))
}

/// Parse the `key: value` of a Local-Variables body line given its `prefix`.
fn parse_local_var(line: &str, prefix: &str) -> Option<(String, String)> {
    let body = line.strip_prefix(prefix).unwrap_or(line);
    let (k, v) = body.split_once(':')?;
    Some((k.trim().to_string(), v.trim().trim_end().to_string()))
}

/// Emacs `add-file-local-variable`: set `var` to `val` in the Local Variables
/// block, creating the block at end of file (wrapped in `prefix`/`suffix`) if
/// none exists.
pub fn set_local_var(content: &str, var: &str, val: &str, prefix: &str, suffix: &str) -> String {
    let (lines_raw, trailing) = split_lines(content);
    let mut lines: Vec<String> = lines_raw.iter().map(|s| s.to_string()).collect();
    if let Some((start, end, pre, suf)) = find_local_block(&lines) {
        // Update an existing variable, or insert before End:.
        for i in (start + 1)..end {
            if let Some((k, _)) = parse_local_var(&lines[i], &pre) {
                if k == var {
                    lines[i] = format!("{pre}{var}: {val}{suf}");
                    return join_lines(&lines, trailing);
                }
            }
        }
        lines.insert(end, format!("{pre}{var}: {val}{suf}"));
        return join_lines(&lines, trailing);
    }
    // No block: append one.
    lines.push(format!("{prefix}Local Variables:{suffix}"));
    lines.push(format!("{prefix}{var}: {val}{suffix}"));
    lines.push(format!("{prefix}End:{suffix}"));
    join_lines(&lines, trailing)
}

/// Emacs `delete-file-local-variable`: remove `var` from the Local Variables
/// block; if that empties the block, remove the whole block.
pub fn delete_local_var(content: &str, var: &str) -> String {
    let (lines_raw, trailing) = split_lines(content);
    let mut lines: Vec<String> = lines_raw.iter().map(|s| s.to_string()).collect();
    let Some((start, end, pre, _)) = find_local_block(&lines) else {
        return content.to_string();
    };
    let mut removed = None;
    for (offset, line) in lines[(start + 1)..end].iter().enumerate() {
        if let Some((k, _)) = parse_local_var(line, &pre) {
            if k == var {
                removed = Some(start + 1 + offset);
                break;
            }
        }
    }
    if let Some(i) = removed {
        lines.remove(i);
        // If only the delimiters remain, drop the whole block.
        if end - 1 == start + 1 {
            // `end` shifted down by one after the removal.
            lines.remove(start); // Local Variables:
            lines.remove(start); // End:
        }
    }
    join_lines(&lines, trailing)
}

/// Every file-local variable the file declares, prop line first then the Local
/// Variables block — what `copy-file-locals-to-dir-locals` copies out. The `mode`
/// pseudo-variable of a bare `-*- lisp -*-` prop line is included, as Emacs's
/// `hack-local-variables` sees it.
pub fn local_vars(content: &str) -> Vec<(String, String)> {
    let (lines, _) = split_lines(content);
    let mut out: Vec<(String, String)> = Vec::new();
    let idx = prop_line_index(&lines);
    if let Some((_, pairs, _)) = lines.get(idx).and_then(|l| parse_prop_line(l)) {
        out.extend(pairs);
    }
    let owned: Vec<String> = lines.iter().map(|s| s.to_string()).collect();
    if let Some((start, end, pre, _)) = find_local_block(&owned) {
        for line in &owned[(start + 1)..end] {
            if let Some(kv) = parse_local_var(line, &pre) {
                out.retain(|(k, _)| *k != kv.0);
                out.push(kv);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prop_line_create_and_update() {
        // Create on a file with no prop line.
        let c = set_prop_line("fn main() {}\n", "mode", "rust", "// ", "");
        assert_eq!(c, "// -*- mode: rust -*-\nfn main() {}\n");
        // Update an existing key; add a second.
        let c2 = set_prop_line(&c, "mode", "rustic", "// ", "");
        assert!(c2.starts_with("// -*- mode: rustic -*-\n"));
        let c3 = set_prop_line(&c2, "fill-column", "80", "// ", "");
        assert_eq!(
            c3.lines().next().unwrap(),
            "// -*- mode: rustic; fill-column: 80 -*-"
        );
    }

    #[test]
    fn prop_line_after_shebang() {
        let c = set_prop_line("#!/bin/sh\necho hi\n", "mode", "sh", "# ", "");
        assert_eq!(c, "#!/bin/sh\n# -*- mode: sh -*-\necho hi\n");
    }

    #[test]
    fn prop_line_bare_mode_form() {
        // A bare `-*- lisp -*-` parses as mode: lisp, then adds a key.
        let c = set_prop_line(
            ";; -*- lisp -*-\n(foo)\n",
            "lexical-binding",
            "t",
            ";; ",
            "",
        );
        assert_eq!(
            c.lines().next().unwrap(),
            ";; -*- mode: lisp; lexical-binding: t -*-"
        );
    }

    #[test]
    fn prop_line_delete() {
        let start = "// -*- mode: rust; fill-column: 80 -*-\ncode\n";
        let c = delete_prop_line(start, "fill-column");
        assert_eq!(c, "// -*- mode: rust -*-\ncode\n");
        // Deleting the last key removes the prop line.
        let c2 = delete_prop_line(&c, "mode");
        assert_eq!(c2, "code\n");
    }

    #[test]
    fn local_block_create_update_delete() {
        let base = "line1\nline2\n";
        let c = set_local_var(base, "mode", "text", "", "");
        assert_eq!(c, "line1\nline2\nLocal Variables:\nmode: text\nEnd:\n");
        // Add a second variable before End:.
        let c2 = set_local_var(&c, "fill-column", "72", "", "");
        assert_eq!(
            c2,
            "line1\nline2\nLocal Variables:\nmode: text\nfill-column: 72\nEnd:\n"
        );
        // Update existing.
        let c3 = set_local_var(&c2, "mode", "org", "", "");
        assert!(c3.contains("mode: org\n"));
        // Delete one variable.
        let c4 = delete_local_var(&c3, "fill-column");
        assert!(!c4.contains("fill-column"));
        assert!(c4.contains("Local Variables:\nmode: org\nEnd:\n"));
        // Deleting the last variable removes the block entirely.
        let c5 = delete_local_var(&c4, "mode");
        assert_eq!(c5, "line1\nline2\n");
    }

    #[test]
    fn local_block_with_comment_prefix() {
        // A C-style comment prefix/suffix wraps each block line.
        let c = set_local_var("code\n", "mode", "c", "/* ", " */");
        assert_eq!(
            c,
            "code\n/* Local Variables: */\n/* mode: c */\n/* End: */\n"
        );
        // Updating parses the prefixed line correctly.
        let c2 = set_local_var(&c, "mode", "c++", "/* ", " */");
        assert!(c2.contains("/* mode: c++ */"));
    }

    /// `copy-file-locals-to-dir-locals` needs to *read* what a file declares:
    /// both the prop line and the Local Variables block, block winning.
    #[test]
    fn local_vars_reads_prop_line_and_block() {
        let src = "\
#!/bin/sh
# -*- mode: sh; fill-column: 70 -*-
echo hi
# Local Variables:
# fill-column: 100
# indent-tabs-mode: nil
# End:
";
        assert_eq!(
            local_vars(src),
            vec![
                ("mode".to_string(), "sh".to_string()),
                ("fill-column".to_string(), "100".to_string()),
                ("indent-tabs-mode".to_string(), "nil".to_string()),
            ],
            "the block's fill-column overrides the prop line's"
        );
        assert!(local_vars("plain file\n").is_empty());
    }
}
