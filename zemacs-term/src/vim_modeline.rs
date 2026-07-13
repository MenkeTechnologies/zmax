//! vim modeline parsing. A modeline is a `vim:`/`vi:`/`ex:` directive embedded
//! in the first or last few lines of a file that sets buffer-local options, e.g.
//! `// vim: set sw=4 ts=4 et:` or `# vim: sw=2 ts=2`. The command layer scans a
//! freshly opened buffer (`commands::apply_modeline`) and runs the extracted
//! options through the normal `:set` path.

/// Extract the option tokens from a single modeline candidate line. Returns an
/// empty vec when the line has no modeline. Handles both forms:
/// * `[text] {vim|vi|ex}: set {opts}:` — options run up to the closing colon.
/// * `[text] {vim|vi|ex}: {opts}`      — the rest of the line is options.
///
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

    // `set`/`se` form: options run up to the next unescaped colon. Bare form: the
    // whole remainder (a trailing colon, if any, is dropped).
    match rest
        .strip_prefix("set ")
        .or_else(|| rest.strip_prefix("se "))
    {
        Some(after) => split_modeline_opts(after, true),
        None => split_modeline_opts(rest.trim_end_matches(':'), false),
    }
}

/// Split a modeline's option text into tokens. As in vim, a backslash escapes
/// the character after it, so a value may contain the colon that would otherwise
/// end the `set` form (`fde=MyFold(v\:lnum)`) or a space that would otherwise end
/// the token (`stl=%f\ %m`); the backslash itself is removed. With `stop_at_colon`
/// (the `set` form) the first unescaped colon ends the options.
fn split_modeline_opts(text: &str, stop_at_colon: bool) -> Vec<String> {
    let mut out = Vec::new();
    let mut token = String::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if let Some(escaped) = chars.next() {
                    token.push(escaped);
                }
            }
            ':' if stop_at_colon => break,
            c if c.is_whitespace() => {
                if !token.is_empty() {
                    out.push(std::mem::take(&mut token));
                }
            }
            c => token.push(c),
        }
    }
    if !token.is_empty() {
        out.push(token);
    }
    out
}

/// Scan the first and last `count` lines of `lines` for the first modeline and
/// return its option tokens (empty if none). vim scans both ends of the file.
pub fn scan_modeline(lines: &[&str], count: usize) -> Vec<String> {
    let n = lines.len();
    let mut heads: Vec<usize> = (0..count.min(n)).collect();
    let tail_start = n
        .saturating_sub(count)
        .max(heads.last().map_or(0, |&h| h + 1));
    heads.extend(tail_start..n);
    for i in heads {
        let opts = parse_modeline(lines[i]);
        if !opts.is_empty() {
            return opts;
        }
    }
    Vec::new()
}

/// The options whose value zemacs *evaluates* rather than stores: a Vimscript
/// expression or a `%{…}` format. A modeline is untrusted text — a file you just
/// opened — so vim refuses to let one set these unless `:set modelineexpr` is on,
/// and errors with E992 otherwise. Listed under every spelling `:set` accepts,
/// since a modeline uses the same tokens.
const MODELINE_EXPR_OPTIONS: &[&str] = &[
    "balloonexpr",
    "bexpr",
    "ccv",
    "charconvert",
    "dex",
    "diffexpr",
    "fde",
    "fdt",
    "foldexpr",
    "foldtext",
    "fex",
    "formatexpr",
    "iconstring",
    "inde",
    "indentexpr",
    "includeexpr",
    "inex",
    "patchexpr",
    "pex",
    "printexpr",
    "rulerformat",
    "ruf",
    "statusline",
    "stl",
    "tabline",
    "tal",
    "titlestring",
];

/// Whether `name` names an option a modeline may only set with `modelineexpr`.
pub fn is_modeline_expr_option(name: &str) -> bool {
    MODELINE_EXPR_OPTIONS.contains(&name)
}

/// Scan a freshly opened document for a modeline and apply the buffer-local
/// options it sets (indentation, filetype, readonly) directly to the document.
/// vim modelines overwhelmingly set these. Honors `:set nomodeline` (skip) and
/// `modelines` (lines scanned at each end, default 5).
///
/// Expression-valued options (`foldexpr`, `statusline`, …) are the exception:
/// a modeline may set them only with `:set modelineexpr` — the file would
/// otherwise choose what code the editor runs. Without it they are rejected with
/// vim's E992, exactly as vim does.
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
        let lines: Vec<String> = (0..text.len_lines())
            .map(|i| text.line(i).to_string())
            .collect();
        let refs: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
        scan_modeline(&refs, count)
    };
    if tokens.is_empty() {
        return;
    }

    // vim `modelineexpr` — off by default, so a modeline that sets an
    // expression option is rejected (E992) rather than obeyed.
    let modelineexpr = matches!(
        crate::commands::vim_opt_str("modelineexpr")
            .or_else(|| crate::commands::vim_opt_str("mle"))
            .as_deref(),
        Some("on" | "1" | "true" | "yes")
    );
    let mut rejected: Vec<&str> = Vec::new();

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
        // An expression option: allowed only with `modelineexpr`, and then set
        // exactly as `:set` would (the store is what the consumers read).
        if is_modeline_expr_option(name) {
            if modelineexpr {
                crate::commands::vim_opt_store(name, val.unwrap_or("").to_string());
            } else {
                rejected.push(name);
            }
            continue;
        }
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

    if !rejected.is_empty() {
        editor.set_error(format!(
            "E992: Not allowed in a modeline when 'modelineexpr' is off: {}",
            rejected.join(" ")
        ));
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
        assert_eq!(parse_modeline("/* vim: set tw=80: */"), vec!["tw=80"]);
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

    /// vim `modelineexpr`: the options a modeline may not set without it are the
    /// evaluated ones — an expression or a `%{…}` format — under every spelling.
    /// Plain buffer options are never gated.
    #[test]
    fn modeline_expr_options_are_the_evaluated_ones() {
        for expr in ["foldexpr", "fde", "statusline", "stl", "indentexpr", "inde"] {
            assert!(is_modeline_expr_option(expr), "{expr} must be gated");
        }
        for plain in ["sw", "shiftwidth", "et", "ts", "filetype", "foldmethod"] {
            assert!(!is_modeline_expr_option(plain), "{plain} must not be gated");
        }
    }

    /// The gate operates on the tokens a real modeline yields: a `vim: set` line
    /// carrying both a plain and an expression option splits into both. The
    /// expression's own colon is backslash-escaped, as vim requires, and the
    /// backslash is removed — an unescaped colon would end the `set` form.
    #[test]
    fn modeline_can_carry_an_expr_option() {
        let toks = parse_modeline(r"// vim: set sw=4 fde=MyFold(v\:lnum):");
        assert_eq!(toks, vec!["sw=4", "fde=MyFold(v:lnum)"]);
        let gated: Vec<&String> = toks
            .iter()
            .filter(|t| is_modeline_expr_option(t.split('=').next().unwrap_or("")))
            .collect();
        assert_eq!(gated, vec!["fde=MyFold(v:lnum)"]);
        // An escaped space keeps a `%`-format value in one token.
        assert_eq!(
            parse_modeline(r"// vim: set stl=%f\ %m:"),
            vec!["stl=%f %m"]
        );
    }

    #[test]
    fn scans_head_and_tail() {
        let lines = vec!["first", "code", "code", "code", "# vim: sw=3"];
        assert_eq!(scan_modeline(&lines, 2), vec!["sw=3"]);
        let head = vec!["// vim: et", "code", "code"];
        assert_eq!(scan_modeline(&head, 2), vec!["et"]);
    }
}
