//! vim modeline parsing. A modeline is a `vim:`/`vi:`/`ex:` directive embedded
//! in the first or last few lines of a file that sets buffer-local options, e.g.
//! `// vim: set sw=4 ts=4 et:` or `# vim: sw=2 ts=2`. The command layer scans a
//! freshly opened buffer (`commands::apply_modeline`) and runs the extracted
//! options through the normal `:set` path.

/// Extract the option tokens from a single modeline candidate line. Returns an
/// empty vec when the line has no modeline. Handles both forms:
/// * `[text] {vim|vi|ex}: set {opts}:` — options run up to the closing colon.
/// * `[text] {vim|vi|ex}: {opts}`      — the rest of the line is options.
/// The marker must be at the start of the line or preceded by whitespace.
pub fn parse_modeline(line: &str) -> Vec<String> {
    // Accept `vim:`, `vi:`, `ex:` and version-tagged `vim>=800:` style markers.
    let bytes = line.as_bytes();
    let mut rest: Option<&str> = None;
    // Scan for one of the markers (longest first so `vim` wins over `vi`).
    for marker in ["vim", "vi", "ex"] {
        let mut search_from = 0;
        while let Some(off) = line[search_from..].find(marker) {
            let pos = search_from + off;
            let before_ok = pos == 0
                || bytes
                    .get(pos - 1)
                    .map(|b| b.is_ascii_whitespace())
                    .unwrap_or(true);
            // After the marker: optional version chars, then a `:` or `=`.
            let after = &line[pos + marker.len()..];
            let after_trim = after.trim_start_matches(|c: char| {
                c.is_ascii_digit() || matches!(c, '<' | '>' | '=' | '.')
            });
            if before_ok && (after_trim.starts_with(':') || after_trim.starts_with('=')) {
                rest = Some(&after_trim[1..]);
                break;
            }
            search_from = pos + marker.len();
        }
        if rest.is_some() {
            break;
        }
    }
    let Some(rest) = rest else {
        return Vec::new();
    };
    let rest = rest.trim_start();

    // `set`/`se` form: options are everything up to the next (unescaped) colon.
    let opts = if let Some(after) = rest
        .strip_prefix("set ")
        .or_else(|| rest.strip_prefix("se "))
    {
        match after.find(':') {
            Some(end) => &after[..end],
            None => after,
        }
    } else {
        // Bare form: the whole remainder (a trailing colon, if any, is dropped).
        rest.trim_end_matches(':')
    };

    opts.split_whitespace()
        .filter(|t| !t.is_empty())
        .map(|t| t.to_string())
        .collect()
}

/// Scan the first and last `count` lines of `lines` for the first modeline and
/// return its option tokens (empty if none). vim scans both ends of the file.
pub fn scan_modeline(lines: &[&str], count: usize) -> Vec<String> {
    let n = lines.len();
    let mut heads: Vec<usize> = (0..count.min(n)).collect();
    let tail_start = n.saturating_sub(count).max(heads.last().map_or(0, |&h| h + 1));
    heads.extend(tail_start..n);
    for i in heads {
        let opts = parse_modeline(lines[i]);
        if !opts.is_empty() {
            return opts;
        }
    }
    Vec::new()
}

/// Scan a freshly opened document for a modeline and apply the buffer-local
/// options it sets (indentation, filetype, readonly) directly to the document.
/// vim modelines overwhelmingly set these. Honors `:set nomodeline` (skip) and
/// `modelines` (lines scanned at each end, default 5).
pub fn apply_modeline(editor: &mut zemacs_view::Editor, doc_id: zemacs_view::DocumentId) {
    if crate::commands::vim_opt_str("modeline").as_deref() == Some("off") {
        return;
    }
    let count = crate::commands::vim_opt_str("modelines")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(5)
        .max(1);

    let tokens = {
        let Some(doc) = editor.document(doc_id) else {
            return;
        };
        let text = doc.text();
        let lines: Vec<String> = (0..text.len_lines()).map(|i| text.line(i).to_string()).collect();
        let refs: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
        scan_modeline(&refs, count)
    };
    if tokens.is_empty() {
        return;
    }

    let mut indent_expand: Option<bool> = None;
    let mut indent_width: Option<u8> = None;
    let mut tab_width: Option<u8> = None;
    let mut readonly: Option<bool> = None;
    let mut filetype: Option<String> = None;
    for tok in &tokens {
        let (name, val) = match tok.split_once('=') {
            Some((n, v)) => (n, Some(v)),
            None => (tok.as_str(), None),
        };
        match name {
            "expandtab" | "et" if val.is_none() => indent_expand = Some(true),
            "noexpandtab" | "noet" => indent_expand = Some(false),
            "shiftwidth" | "sw" | "softtabstop" | "sts" => {
                if let Some(n) = val.and_then(|v| v.parse::<u8>().ok()) {
                    if n > 0 {
                        indent_width = Some(n);
                    }
                }
            }
            "tabstop" | "ts" => {
                if let Some(n) = val.and_then(|v| v.parse::<u8>().ok()) {
                    if n > 0 {
                        tab_width = Some(n);
                    }
                }
            }
            "readonly" | "ro" if val.is_none() => readonly = Some(true),
            "noreadonly" | "noro" => readonly = Some(false),
            "filetype" | "ft" | "syntax" | "syn" => {
                if let Some(v) = val.filter(|v| !v.is_empty()) {
                    filetype = Some(v.to_string());
                }
            }
            _ => {}
        }
    }

    if let Some(lang) = filetype {
        let loader = editor.syn_loader.load();
        if let Some(doc) = editor.document_mut(doc_id) {
            let _ = doc.set_language_by_language_id(&lang, &loader);
        }
    }
    if indent_expand.is_some()
        || indent_width.is_some()
        || tab_width.is_some()
        || readonly.is_some()
    {
        use zemacs_core::indent::{IndentStyle, MAX_INDENT};
        if let Some(doc) = editor.document_mut(doc_id) {
            if let Some(tw) = tab_width {
                doc.set_tab_width(tw);
            }
            if let Some(ro) = readonly {
                doc.readonly = ro;
            }
            if indent_expand.is_some() || indent_width.is_some() {
                let cur = match doc.indent_style {
                    IndentStyle::Spaces(n) => n,
                    IndentStyle::Tabs => doc.tab_width() as u8,
                };
                let width = indent_width.unwrap_or(cur).clamp(1, MAX_INDENT);
                doc.indent_style = match indent_expand {
                    Some(true) => IndentStyle::Spaces(width),
                    Some(false) => IndentStyle::Tabs,
                    None => match doc.indent_style {
                        IndentStyle::Spaces(_) => IndentStyle::Spaces(width),
                        IndentStyle::Tabs => IndentStyle::Tabs,
                    },
                };
            }
        }
    }
}

/// Register the modeline scan to run whenever a document is opened.
pub fn register_hooks() {
    use zemacs_event::register_hook;
    use zemacs_view::events::DocumentDidOpen;
    register_hook!(move |event: &mut DocumentDidOpen<'_>| {
        apply_modeline(event.editor, event.doc);
        Ok(())
    });
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn set_form() {
        assert_eq!(
            parse_modeline("// vim: set sw=4 ts=4 et:"),
            vec!["sw=4", "ts=4", "et"]
        );
        assert_eq!(
            parse_modeline("/* vim: set tw=80: */"),
            vec!["tw=80"]
        );
    }

    #[test]
    fn bare_form() {
        assert_eq!(parse_modeline("# vim: sw=2 ts=2"), vec!["sw=2", "ts=2"]);
        assert_eq!(parse_modeline("vi: noet"), vec!["noet"]);
    }

    #[test]
    fn versioned_and_none() {
        assert_eq!(parse_modeline("// vim>=800: sw=4"), vec!["sw=4"]);
        assert_eq!(parse_modeline("just a normal line"), Vec::<String>::new());
        // `vim` not at a word boundary marker must not trip it.
        assert_eq!(parse_modeline("using vims here"), Vec::<String>::new());
    }

    #[test]
    fn scans_head_and_tail() {
        let lines = vec!["first", "code", "code", "code", "# vim: sw=3"];
        assert_eq!(scan_modeline(&lines, 2), vec!["sw=3"]);
        let head = vec!["// vim: et", "code", "code"];
        assert_eq!(scan_modeline(&head, 2), vec!["et"]);
    }
}
