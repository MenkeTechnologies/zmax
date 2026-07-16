//! Pure, editor-free algorithms backing the SGML/HTML editing substrate (the
//! zmax port of GNU Emacs `sgml-mode`/`html-mode`), plus the `nroff-mode`
//! text-line motions and the `htmlfontify-buffer` HTML exporter.
//!
//! The command layer in the term crate reads the buffer text, calls these, and
//! applies the result. Everything here is dependency-free and unit-tested.
//! Prior art: Emacs `sgml-close-tag`, `sgml-delete-tag`, `sgml-skip-tag-forward`,
//! `sgml-name-char`, `nroff-forward-text-line`, `htmlfontify-buffer`.

/// Whether a parsed tag opens an element, closes one, or is self-contained.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TagKind {
    /// A start tag such as `<p>` or `<a href="x">`.
    Open,
    /// An end tag such as `</p>`.
    Close,
    /// A self-closing/empty tag such as `<br/>` or `<img .../>`.
    SelfClose,
}

/// One markup tag located in the source text. Offsets are in `char`s (not
/// bytes) so callers can map straight onto a rope; `start..end` spans the whole
/// tag including the angle brackets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tag {
    /// Char offset of the opening `<`.
    pub start: usize,
    /// Char offset just past the closing `>`.
    pub end: usize,
    /// The element name (without brackets or slash), e.g. `div`.
    pub name: String,
    /// Whether this opens, closes, or is self-contained.
    pub kind: TagKind,
}

/// Parse every markup tag in `text`, in source order. Comments (`<!-- ... -->`),
/// declarations (`<!DOCTYPE ...>`) and processing instructions (`<? ... ?>`) are
/// recognised and skipped (not returned). A bare `<` that does not begin a tag
/// name (e.g. `a < b`) is treated as literal text.
pub fn parse_tags(text: &str) -> Vec<Tag> {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let mut out = Vec::new();
    let mut i = 0;
    while i < n {
        if chars[i] != '<' {
            i += 1;
            continue;
        }
        let start = i;
        // Comment: <!-- ... -->
        if i + 3 < n && chars[i + 1] == '!' && chars[i + 2] == '-' && chars[i + 3] == '-' {
            let mut j = i + 4;
            while j + 2 < n && !(chars[j] == '-' && chars[j + 1] == '-' && chars[j + 2] == '>') {
                j += 1;
            }
            i = if j + 2 < n { j + 3 } else { n };
            continue;
        }
        // Declaration (<!DOCTYPE ...>) or processing instruction (<? ... ?>).
        if i + 1 < n && (chars[i + 1] == '!' || chars[i + 1] == '?') {
            let mut j = i + 1;
            while j < n && chars[j] != '>' {
                j += 1;
            }
            i = if j < n { j + 1 } else { n };
            continue;
        }
        // Start or end tag.
        let mut j = i + 1;
        let is_close = j < n && chars[j] == '/';
        if is_close {
            j += 1;
        }
        let name_start = j;
        while j < n
            && (chars[j].is_alphanumeric() || chars[j] == '-' || chars[j] == '_' || chars[j] == ':')
        {
            j += 1;
        }
        if j == name_start {
            // '<' not followed by a name — literal text.
            i += 1;
            continue;
        }
        let name: String = chars[name_start..j].iter().collect();
        // Scan to the closing '>', skipping quoted attribute values.
        let mut k = j;
        while k < n && chars[k] != '>' {
            if chars[k] == '"' || chars[k] == '\'' {
                let q = chars[k];
                k += 1;
                while k < n && chars[k] != q {
                    k += 1;
                }
            }
            k += 1;
        }
        let self_close = k < n && k > name_start && chars[k - 1] == '/';
        let end = if k < n { k + 1 } else { n };
        let kind = if is_close {
            TagKind::Close
        } else if self_close {
            TagKind::SelfClose
        } else {
            TagKind::Open
        };
        out.push(Tag {
            start,
            end,
            name,
            kind,
        });
        i = end;
    }
    out
}

/// Emacs `sgml-tag`: build `<TAG attrs>content</TAG>` around `content`. When
/// `attrs` is `None` or empty, the start tag is just `<TAG>`. This is the pure
/// builder; the command layer supplies `content` from the region (or `""`).
pub fn wrap_tag(tag: &str, attrs: Option<&str>, content: &str) -> String {
    let open = match attrs {
        Some(a) if !a.trim().is_empty() => format!("<{tag} {}>", a.trim()),
        _ => format!("<{tag}>"),
    };
    format!("{open}{content}</{tag}>")
}

/// Emacs `sgml-close-tag`: return the name of the innermost element that is
/// still open at the end of `text` (a start tag with no matching end tag), so
/// the caller can insert `</NAME>`. Uses a stack; self-closing tags and
/// comments/declarations are ignored. Returns `None` when everything is closed.
pub fn unclosed_tag(text: &str) -> Option<String> {
    let mut stack: Vec<String> = Vec::new();
    for t in parse_tags(text) {
        match t.kind {
            TagKind::Open => stack.push(t.name),
            TagKind::Close => {
                if let Some(pos) = stack.iter().rposition(|e| *e == t.name) {
                    stack.truncate(pos);
                } else {
                    stack.pop();
                }
            }
            TagKind::SelfClose => {}
        }
    }
    stack.pop()
}

/// Emacs `sgml-skip-tag-forward` (`C-c C-f`): from `point`, move forward over
/// one balanced tag group and return the char offset just past its end tag.
/// `point` must be at or before a start tag. Nesting is balanced generically
/// (any start `+1`, any end `-1`); a self-closing tag returns its own end.
/// Returns `None` if there is no start tag at/after point or it is unbalanced.
pub fn skip_tag_forward(text: &str, point: usize) -> Option<usize> {
    let tags = parse_tags(text);
    let idx = tags.iter().position(|t| t.end > point)?;
    match tags[idx].kind {
        TagKind::SelfClose => Some(tags[idx].end),
        TagKind::Close => None,
        TagKind::Open => {
            let mut depth = 0i32;
            for t in &tags[idx..] {
                match t.kind {
                    TagKind::Open => depth += 1,
                    TagKind::Close => depth -= 1,
                    TagKind::SelfClose => {}
                }
                if depth == 0 {
                    return Some(t.end);
                }
            }
            None
        }
    }
}

/// Emacs `sgml-skip-tag-backward` (`C-c C-b`): from `point`, move backward over
/// one balanced tag group and return the char offset of its start tag's opening
/// `<`. `point` must be at or after an end tag. Returns `None` if there is no
/// end tag at/before point or it is unbalanced.
pub fn skip_tag_backward(text: &str, point: usize) -> Option<usize> {
    let tags = parse_tags(text);
    let idx = tags.iter().rposition(|t| t.end <= point)?;
    match tags[idx].kind {
        TagKind::SelfClose => Some(tags[idx].start),
        TagKind::Open => None,
        TagKind::Close => {
            let mut depth = 0i32;
            for t in tags[..=idx].iter().rev() {
                match t.kind {
                    TagKind::Close => depth += 1,
                    TagKind::Open => depth -= 1,
                    TagKind::SelfClose => {}
                }
                if depth == 0 {
                    return Some(t.start);
                }
            }
            None
        }
    }
}

/// Emacs `sgml-delete-tag` (`C-c DEL`): delete the tag on or after `point` and
/// its matching partner, keeping the enclosed content. Returns the resulting
/// text. A start tag deletes itself and its matching end tag; an end tag deletes
/// itself and its matching start tag; a self-closing tag deletes only itself.
/// Returns `None` when there is no tag at/after point or the pair is unbalanced.
pub fn delete_tag(text: &str, point: usize) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    let tags = parse_tags(text);
    let idx = tags.iter().position(|t| t.end > point)?;
    let target = &tags[idx];
    // Collect the char ranges to remove (one or two tag spans).
    let mut cuts: Vec<(usize, usize)> = Vec::new();
    match target.kind {
        TagKind::SelfClose => cuts.push((target.start, target.end)),
        TagKind::Open => {
            let mut depth = 0i32;
            let mut close_end = None;
            for t in &tags[idx..] {
                match t.kind {
                    TagKind::Open => depth += 1,
                    TagKind::Close => depth -= 1,
                    TagKind::SelfClose => {}
                }
                if depth == 0 {
                    close_end = Some(t.clone());
                    break;
                }
            }
            let close = close_end?;
            cuts.push((target.start, target.end));
            cuts.push((close.start, close.end));
        }
        TagKind::Close => {
            let mut depth = 0i32;
            let mut open = None;
            for t in tags[..=idx].iter().rev() {
                match t.kind {
                    TagKind::Close => depth += 1,
                    TagKind::Open => depth -= 1,
                    TagKind::SelfClose => {}
                }
                if depth == 0 {
                    open = Some(t.clone());
                    break;
                }
            }
            let open = open?;
            cuts.push((open.start, open.end));
            cuts.push((target.start, target.end));
        }
    }
    cuts.sort_unstable();
    let mut result = String::new();
    let mut i = 0;
    for (s, e) in cuts {
        result.extend(chars[i..s].iter());
        i = e;
    }
    result.extend(chars[i..].iter());
    Some(result)
}

// --------------------------------------------------------------------------
// Character entities (sgml-name-char / SGML entity encode+decode).
// --------------------------------------------------------------------------

/// The named-entity table used by `sgml-name-char`: `(char, entity-name)`
/// pairs. Mirrors the common subset of Emacs `sgml-char-names` / HTML named
/// character references.
const ENTITIES: &[(char, &str)] = &[
    ('"', "quot"),
    ('&', "amp"),
    ('\'', "apos"),
    ('<', "lt"),
    ('>', "gt"),
    ('\u{00A0}', "nbsp"),
    ('\u{00A1}', "iexcl"),
    ('\u{00A2}', "cent"),
    ('\u{00A3}', "pound"),
    ('\u{00A4}', "curren"),
    ('\u{00A5}', "yen"),
    ('\u{00A7}', "sect"),
    ('\u{00A9}', "copy"),
    ('\u{00AB}', "laquo"),
    ('\u{00AE}', "reg"),
    ('\u{00B0}', "deg"),
    ('\u{00B1}', "plusmn"),
    ('\u{00B5}', "micro"),
    ('\u{00B6}', "para"),
    ('\u{00BB}', "raquo"),
    ('\u{00BC}', "frac14"),
    ('\u{00BD}', "frac12"),
    ('\u{00BE}', "frac34"),
    ('\u{00BF}', "iquest"),
    ('\u{00C9}', "Eacute"),
    ('\u{00D7}', "times"),
    ('\u{00DF}', "szlig"),
    ('\u{00E0}', "agrave"),
    ('\u{00E9}', "eacute"),
    ('\u{00F1}', "ntilde"),
    ('\u{00F7}', "divide"),
    ('\u{2013}', "ndash"),
    ('\u{2014}', "mdash"),
    ('\u{2018}', "lsquo"),
    ('\u{2019}', "rsquo"),
    ('\u{201C}', "ldquo"),
    ('\u{201D}', "rdquo"),
    ('\u{2020}', "dagger"),
    ('\u{2022}', "bull"),
    ('\u{2026}', "hellip"),
    ('\u{20AC}', "euro"),
    ('\u{2122}', "trade"),
];

/// Emacs `sgml-name-char`: the entity *name* for `c`, e.g. `'&'` -> `Some("amp")`.
/// Returns `None` when there is no named entity for the character.
pub fn char_entity(c: char) -> Option<&'static str> {
    ENTITIES.iter().find(|(ch, _)| *ch == c).map(|(_, n)| *n)
}

/// Emacs `sgml-name-char`: the full entity reference to insert for `c`, e.g.
/// `'&'` -> `&amp;`. Characters with no named entity fall back to a numeric
/// reference (`&#NNN;`).
pub fn entity_ref(c: char) -> String {
    match char_entity(c) {
        Some(name) => format!("&{name};"),
        None => format!("&#{};", c as u32),
    }
}

/// Decode a single entity reference *name* (the text between `&` and `;`) back
/// to its character. Handles named entities plus numeric `#NNN` (decimal) and
/// `#xHH` (hex) forms. Returns `None` for an unknown name.
pub fn entity_to_char(name: &str) -> Option<char> {
    if let Some(rest) = name.strip_prefix('#') {
        let code = if let Some(hex) = rest.strip_prefix(['x', 'X']) {
            u32::from_str_radix(hex, 16).ok()?
        } else {
            rest.parse::<u32>().ok()?
        };
        return char::from_u32(code);
    }
    ENTITIES.iter().find(|(_, n)| *n == name).map(|(ch, _)| *ch)
}

// --------------------------------------------------------------------------
// sgml-tag-help: a static description table for common HTML elements.
// --------------------------------------------------------------------------

/// Emacs `sgml-tag-help`: a one-line description of a common HTML element, or
/// `None` if the tag is unknown. A curated static table (not the full DTD).
pub fn tag_help(tag: &str) -> Option<&'static str> {
    let t = tag.trim().trim_start_matches('/').to_ascii_lowercase();
    let desc = match t.as_str() {
        "a" => "Anchor / hyperlink",
        "abbr" => "Abbreviation",
        "b" => "Bold (stylistically offset) text",
        "blockquote" => "Block quotation",
        "body" => "Document body",
        "br" => "Line break",
        "button" => "Clickable button",
        "code" => "Inline code fragment",
        "div" => "Generic block container",
        "em" => "Emphasised text",
        "form" => "Interactive form",
        "h1" => "Top-level section heading",
        "h2" => "Second-level heading",
        "h3" => "Third-level heading",
        "head" => "Document metadata container",
        "hr" => "Thematic break (horizontal rule)",
        "html" => "Root of an HTML document",
        "i" => "Alternate-voice / italic text",
        "img" => "Embedded image",
        "input" => "Form input control",
        "li" => "List item",
        "link" => "External resource link",
        "meta" => "Document-level metadata",
        "ol" => "Ordered list",
        "p" => "Paragraph",
        "pre" => "Preformatted text",
        "script" => "Embedded or referenced script",
        "span" => "Generic inline container",
        "strong" => "Strong importance",
        "style" => "Embedded style information",
        "table" => "Tabular data",
        "td" => "Table data cell",
        "th" => "Table header cell",
        "title" => "Document title",
        "tr" => "Table row",
        "ul" => "Unordered list",
        _ => return None,
    };
    Some(desc)
}

// --------------------------------------------------------------------------
// nroff-mode: text-line classification and motion.
// --------------------------------------------------------------------------

/// The `nroff-mode` text-line helpers (Emacs `nroff-forward-text-line`,
/// `nroff-backward-text-line`, `nroff-count-text-lines`). A *request* line is
/// one starting with `.` or `'` (a troff/nroff control line); every other line
/// is a *text* line.
pub mod nroff {
    /// Emacs `nroff-text-line-regexp`: a request/control line starts (at column
    /// 0) with `.` or `'`. Everything else is a text line.
    pub fn is_request_line(line: &str) -> bool {
        matches!(line.as_bytes().first(), Some(b'.') | Some(b'\''))
    }

    /// A text line is any line that is not a request line.
    pub fn is_text_line(line: &str) -> bool {
        !is_request_line(line)
    }

    /// Emacs `nroff-count-text-lines`: count the text lines (non-request lines)
    /// among `lines`. Callers pass the lines of the region.
    pub fn count_text_lines(lines: &[&str]) -> usize {
        lines.iter().filter(|l| is_text_line(l)).count()
    }

    /// Emacs `nroff-forward-text-line` / `nroff-backward-text-line`: from the
    /// line at index `line`, move `cnt` text lines (negative moves backward),
    /// skipping request lines, and return the destination line index. Matches
    /// the Emacs algorithm: for each step, advance one line, then skip any
    /// run of request lines; clamp at the buffer ends.
    pub fn forward_text_line(lines: &[&str], line: usize, cnt: isize) -> usize {
        let n = lines.len();
        if n == 0 {
            return 0;
        }
        let mut idx = line.min(n);
        let mut remaining = cnt;
        while remaining > 0 && idx < n {
            idx += 1;
            while idx < n && lines[idx].starts_with(['.', '\'']) {
                idx += 1;
            }
            remaining -= 1;
        }
        while remaining < 0 && idx > 0 {
            idx -= 1;
            while idx > 0 && lines[idx].starts_with(['.', '\'']) {
                idx -= 1;
            }
            remaining += 1;
        }
        idx
    }
}

// --------------------------------------------------------------------------
// htmlfontify-buffer: export (text, style-spans) as syntax-highlighted HTML.
// --------------------------------------------------------------------------

/// The `htmlfontify-buffer` HTML exporter: turn buffer text plus a list of
/// styled character ranges into a self-contained `<pre>` block.
pub mod htmlfontify {
    /// HTML-escape `c` into `out` (`&`, `<`, `>`, `"` become entity refs).
    fn push_escaped(out: &mut String, c: char) {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }

    /// HTML-escape a whole string (`&`, `<`, `>`, `"`).
    pub fn escape(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        for c in s.chars() {
            push_escaped(&mut out, c);
        }
        out
    }

    /// Emacs `htmlfontify-buffer`: wrap `text` in a `<pre>` element, emitting a
    /// `<span style="...">` for each styled range in `spans`. Each span is
    /// `(start_char, end_char, css_style)`; spans must be non-overlapping and
    /// sorted by `start`. Text outside any span is emitted plain (escaped).
    pub fn fontify(text: &str, spans: &[(usize, usize, &str)]) -> String {
        let chars: Vec<char> = text.chars().collect();
        let n = chars.len();
        let mut out = String::from("<pre>");
        let mut i = 0;
        let mut si = 0;
        while i < n {
            if si < spans.len() && i == spans[si].0 {
                let (s, e, style) = spans[si];
                let e = e.min(n);
                out.push_str("<span style=\"");
                out.push_str(style);
                out.push_str("\">");
                for &c in &chars[s..e] {
                    push_escaped(&mut out, c);
                }
                out.push_str("</span>");
                i = e;
                si += 1;
            } else {
                push_escaped(&mut out, chars[i]);
                i += 1;
            }
        }
        out.push_str("</pre>");
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_tags() {
        let tags = parse_tags("<p>hi</p>");
        assert_eq!(tags.len(), 2);
        assert_eq!(tags[0].kind, TagKind::Open);
        assert_eq!(tags[0].name, "p");
        assert_eq!((tags[0].start, tags[0].end), (0, 3));
        assert_eq!(tags[1].kind, TagKind::Close);
        assert_eq!((tags[1].start, tags[1].end), (5, 9));
    }

    #[test]
    fn parses_self_close_and_attrs() {
        let tags = parse_tags(r#"<img src="a>b.png"/><br/>"#);
        assert_eq!(tags.len(), 2);
        assert_eq!(tags[0].kind, TagKind::SelfClose);
        assert_eq!(tags[0].name, "img");
        // The '>' inside the quoted attribute must not end the tag early.
        assert_eq!(tags[0].end, 20);
        assert_eq!(tags[1].kind, TagKind::SelfClose);
    }

    #[test]
    fn skips_comments_and_decls() {
        let tags = parse_tags("<!DOCTYPE html><!-- <p> --><b>x</b>");
        assert_eq!(tags.len(), 2);
        assert_eq!(tags[0].name, "b");
        assert_eq!(tags[0].kind, TagKind::Open);
        assert_eq!(tags[1].kind, TagKind::Close);
    }

    #[test]
    fn bare_lt_is_literal() {
        assert!(parse_tags("a < b and c > d").is_empty());
    }

    #[test]
    fn wrap_tag_builds() {
        assert_eq!(wrap_tag("b", None, "x"), "<b>x</b>");
        assert_eq!(wrap_tag("b", Some(""), "x"), "<b>x</b>");
        assert_eq!(
            wrap_tag("a", Some("href=\"u\""), "link"),
            "<a href=\"u\">link</a>"
        );
    }

    #[test]
    fn unclosed_tag_innermost() {
        assert_eq!(unclosed_tag("<div><p>"), Some("p".to_string()));
        assert_eq!(unclosed_tag("<div><p></p>"), Some("div".to_string()));
        assert_eq!(unclosed_tag("<div></div>"), None);
        // A self-closing tag never needs a close.
        assert_eq!(unclosed_tag("<div><br/>"), Some("div".to_string()));
        assert_eq!(unclosed_tag("plain text"), None);
    }

    #[test]
    fn skip_forward_over_pair() {
        // <p>hi</p>  -> past the </p> at offset 9
        assert_eq!(skip_tag_forward("<p>hi</p>", 0), Some(9));
        // nested
        let s = "<div><p>x</p></div>";
        assert_eq!(skip_tag_forward(s, 0), Some(s.chars().count()));
        // starting inside at the <p>
        assert_eq!(skip_tag_forward(s, 5), Some(13));
        // self-closing
        assert_eq!(skip_tag_forward("<br/>", 0), Some(5));
        // at a close tag -> None
        assert_eq!(skip_tag_forward("</p>", 0), None);
    }

    #[test]
    fn skip_backward_over_pair() {
        let s = "<p>hi</p>";
        assert_eq!(skip_tag_backward(s, 9), Some(0));
        let s2 = "<div><p>x</p></div>";
        assert_eq!(skip_tag_backward(s2, s2.chars().count()), Some(0));
        // to the end of </p> (offset 13) -> back to <p> at 5
        assert_eq!(skip_tag_backward(s2, 13), Some(5));
        assert_eq!(skip_tag_backward("<br/>", 5), Some(0));
    }

    #[test]
    fn delete_tag_keeps_content() {
        // point on the start tag
        assert_eq!(delete_tag("<b>hi</b>", 0).as_deref(), Some("hi"));
        // point on the end tag
        assert_eq!(delete_tag("<b>hi</b>", 6).as_deref(), Some("hi"));
        // nested: delete outer, keep inner markup
        assert_eq!(
            delete_tag("<div><p>x</p></div>", 0).as_deref(),
            Some("<p>x</p>")
        );
        // self-closing removes only itself
        assert_eq!(delete_tag("a<br/>b", 1).as_deref(), Some("ab"));
    }

    #[test]
    fn entities_encode_decode() {
        assert_eq!(char_entity('&'), Some("amp"));
        assert_eq!(char_entity('<'), Some("lt"));
        assert_eq!(char_entity('\u{00A9}'), Some("copy"));
        assert_eq!(char_entity('z'), None);
        assert_eq!(entity_ref('>'), "&gt;");
        assert_eq!(entity_ref('\u{20AC}'), "&euro;");
        // no named entity -> numeric
        assert_eq!(entity_ref('\u{2603}'), "&#9731;");
        // decode round-trips
        assert_eq!(entity_to_char("amp"), Some('&'));
        assert_eq!(entity_to_char("euro"), Some('\u{20AC}'));
        assert_eq!(entity_to_char("#169"), Some('\u{00A9}'));
        assert_eq!(entity_to_char("#xA9"), Some('\u{00A9}'));
        assert_eq!(entity_to_char("nope"), None);
    }

    #[test]
    fn tag_help_lookups() {
        assert_eq!(tag_help("a"), Some("Anchor / hyperlink"));
        assert_eq!(tag_help("P"), Some("Paragraph"));
        assert_eq!(tag_help("/div"), Some("Generic block container"));
        assert_eq!(tag_help("bogus"), None);
    }

    #[test]
    fn nroff_classification() {
        assert!(nroff::is_request_line(".SH NAME"));
        assert!(nroff::is_request_line("'br"));
        assert!(!nroff::is_request_line("plain text"));
        assert!(nroff::is_text_line("plain text"));
        let lines = vec![".TH", "hello", ".PP", "world", "again"];
        assert_eq!(nroff::count_text_lines(&lines), 3);
    }

    #[test]
    fn nroff_motion() {
        // lines: 0:.TH 1:hello 2:.PP 3:world 4:again
        let lines = vec![".TH", "hello", ".PP", "world", "again"];
        // from line 1 (hello) forward one text line -> skip .PP -> land on 3 (world)
        assert_eq!(nroff::forward_text_line(&lines, 1, 1), 3);
        // forward two -> world then again
        assert_eq!(nroff::forward_text_line(&lines, 1, 2), 4);
        // backward one from 3 (world): step to 2 (.PP), skip up to 1 (hello)
        assert_eq!(nroff::forward_text_line(&lines, 3, -1), 1);
        // clamp at end
        assert_eq!(nroff::forward_text_line(&lines, 4, 3), 5);
    }

    #[test]
    fn htmlfontify_escapes_and_spans() {
        assert_eq!(htmlfontify::escape("a<b>&c"), "a&lt;b&gt;&amp;c");
        // no spans -> plain escaped <pre>
        assert_eq!(htmlfontify::fontify("a<b", &[]), "<pre>a&lt;b</pre>");
        // one styled span over chars 0..3
        let html = htmlfontify::fontify("fn x", &[(0, 2, "color:#00f")]);
        assert_eq!(html, "<pre><span style=\"color:#00f\">fn</span> x</pre>");
    }
}
