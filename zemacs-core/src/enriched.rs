//! `text/enriched` — the format converter behind Emacs `enriched-mode` and
//! `format-decode-buffer`.
//!
//! Emacs' `enriched.el` is a *format converter*: the file on disk carries the
//! face information as SGML-ish annotations, and `format-decode-buffer` turns
//! those annotations into real text properties on the buffer text (deleting the
//! markup), while saving runs the reverse conversion. Without a text-property
//! store there was nothing for the decoder to write to, which is why
//! `enriched-mode` was only a mode-name change in this port.
//!
//! The annotations handled here are the attribute set `facemenu` can produce, so
//! the pair [`decode`] / [`encode`] round-trips everything the face menu can put
//! on a region:
//!
//! ```text
//! Content-Type: text/enriched
//! Text-Width: 70
//!
//! plain <bold>bold</bold> and <x-color><param>red</param>red</x-color>
//! ```
//!
//! A literal `<` in the text is written `<<`, per RFC 1896.
//!
//! Not implemented: RFC 1896's soft-newline folding (a lone newline is a
//! soft break, `n` newlines mean `n-1` hard ones). That is the job of Emacs'
//! separate `use-hard-newlines`, which this port does not have, so the decoder
//! and encoder pass newlines through verbatim and a file round-trips byte for
//! byte apart from the annotations.

use crate::text_props::{Face, Rgb, Span, TextProps};

/// The header `enriched-mode` writes in front of an enriched file.
pub const HEADER: &str = "Content-Type: text/enriched\nText-Width: 70\n\n";

/// The `Content-Type` line that marks a buffer as enriched.
const CONTENT_TYPE: &str = "content-type: text/enriched";

/// True when `text` begins with an enriched-format header — what `enriched-mode`
/// checks before it decodes a freshly visited buffer.
pub fn has_header(text: &str) -> bool {
    header_len(text).is_some()
}

/// The char length of the leading RFC 1896 header (the `Content-Type:` line, any
/// further header lines, and the blank line that ends them), or `None` when the
/// text is not enriched.
fn header_len(text: &str) -> Option<usize> {
    let first = text.lines().next()?;
    if !first.trim().to_ascii_lowercase().starts_with(CONTENT_TYPE) {
        return None;
    }
    // The header runs to the first blank line; everything after it is the body.
    let mut chars = 0usize;
    let mut lines = text.split_inclusive('\n');
    for line in &mut lines {
        chars += line.chars().count();
        if line.trim().is_empty() {
            return Some(chars);
        }
    }
    // A header with no body at all.
    Some(chars)
}

/// A decoded enriched buffer: the plain text with all markup removed, and the
/// face runs that the markup described (char offsets into `text`).
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Decoded {
    /// The text with the header and every annotation stripped.
    pub text: String,
    /// The face runs the annotations described.
    pub props: TextProps,
}

/// Which face attribute an annotation carries.
fn attr_of(tag: &str) -> Option<fn(&mut Face)> {
    match tag {
        "bold" => Some(|f: &mut Face| f.bold = true),
        "italic" => Some(|f: &mut Face| f.italic = true),
        "underline" => Some(|f: &mut Face| f.underline = true),
        _ => None,
    }
}

/// Parse a color as enriched writes it: either a `#rrggbb` string or one of the
/// named colors in [`crate::facemenu::colors`].
fn parse_color(s: &str) -> Option<Rgb> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix('#') {
        // enriched files from Emacs may use 4-digit-per-channel X11 colors.
        let per = match hex.len() {
            6 => 2,
            12 => 4,
            _ => return None,
        };
        let chan = |i: usize| -> Option<u8> {
            let raw = &hex[i * per..i * per + per];
            let v = u32::from_str_radix(raw, 16).ok()?;
            Some((v >> (4 * (per - 2))) as u8)
        };
        return Some((chan(0)?, chan(1)?, chan(2)?));
    }
    crate::facemenu::find_color(s)
}

/// Emacs `format-decode-buffer` for `text/enriched`: strip the header and the
/// annotations, and turn them into face runs over the remaining text.
///
/// Unknown annotations are dropped along with their markup (RFC 1896 requires a
/// reader to ignore what it does not understand) but their *content* is kept.
pub fn decode(src: &str) -> Decoded {
    let body = match header_len(src) {
        Some(n) => src.chars().skip(n).collect::<String>(),
        None => src.to_string(),
    };

    let mut text = String::with_capacity(body.len());
    let mut props = TextProps::new();
    // The stack of annotations currently open, each with the char offset it
    // opened at and the pending `<param>` value.
    let mut open: Vec<(String, usize, Option<String>)> = Vec::new();
    // Set while the parser is inside `<param>…</param>`, capturing its text.
    let mut param: Option<String> = None;
    let mut out_len = 0usize;

    let mut chars = body.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '<' {
            match &mut param {
                Some(buf) => buf.push(c),
                None => {
                    text.push(c);
                    out_len += 1;
                }
            }
            continue;
        }
        // `<<` is a literal `<`.
        if chars.peek() == Some(&'<') {
            chars.next();
            match &mut param {
                Some(buf) => buf.push('<'),
                None => {
                    text.push('<');
                    out_len += 1;
                }
            }
            continue;
        }
        let mut tag = String::new();
        for c in chars.by_ref() {
            if c == '>' {
                break;
            }
            tag.push(c);
        }
        let tag = tag.trim().to_ascii_lowercase();
        if let Some(name) = tag.strip_prefix('/') {
            let name = name.trim();
            if name == "param" {
                // Attach the captured parameter to the innermost open annotation.
                let value = param.take().unwrap_or_default();
                if let Some(last) = open.last_mut() {
                    last.2 = Some(value);
                }
                continue;
            }
            // Close the innermost matching annotation and emit its face run.
            if let Some(idx) = open.iter().rposition(|(t, _, _)| t == name) {
                let (tag, start, value) = open.remove(idx);
                if let Some(face) = face_for(&tag, value.as_deref()) {
                    props.add_face(start..out_len, &face);
                }
            }
            continue;
        }
        if tag == "param" {
            param = Some(String::new());
            continue;
        }
        open.push((tag, out_len, None));
    }
    // Unclosed annotations run to the end of the buffer, as RFC 1896 permits.
    while let Some((tag, start, value)) = open.pop() {
        if let Some(face) = face_for(&tag, value.as_deref()) {
            props.add_face(start..out_len, &face);
        }
    }

    Decoded { text, props }
}

/// The face an annotation describes, or `None` for annotations this port does
/// not model (justification, margins, `<fixed>`, …).
fn face_for(tag: &str, param: Option<&str>) -> Option<Face> {
    if let Some(set) = attr_of(tag) {
        let mut face = Face::default();
        set(&mut face);
        return Some(face);
    }
    match tag {
        "x-color" => parse_color(param?).map(Face::foreground),
        "x-bg-color" => parse_color(param?).map(Face::background),
        _ => None,
    }
}

/// Escape a run of plain text for an enriched file: `<` doubles.
fn escape(s: &str) -> String {
    s.replace('<', "<<")
}

/// The annotations a face needs, outermost first. A named face is decomposed
/// into the attributes `text/enriched` can actually carry — a face that names
/// nothing but a theme scope (`font-lock-keyword-face`) has no representation in
/// the format and encodes as plain text, exactly as Emacs drops the attributes it
/// cannot express.
fn annotations(face: &Face) -> Vec<(String, Option<String>)> {
    let mut out = Vec::new();
    let named = face.name.as_deref().unwrap_or("");
    let bold = face.bold || matches!(named, "bold" | "bold-italic");
    let italic = face.italic || matches!(named, "italic" | "bold-italic");
    let underline = face.underline || named == "underline";
    if bold {
        out.push(("bold".to_string(), None));
    }
    if italic {
        out.push(("italic".to_string(), None));
    }
    if underline {
        out.push(("underline".to_string(), None));
    }
    if let Some(rgb) = face.fg {
        out.push(("x-color".to_string(), Some(crate::facemenu::hex(rgb))));
    }
    if let Some(rgb) = face.bg {
        out.push(("x-bg-color".to_string(), Some(crate::facemenu::hex(rgb))));
    }
    out
}

/// Emacs `format-encode-region` for `text/enriched`: render `text` and its face
/// runs back to an enriched file, header included. The inverse of [`decode`].
pub fn encode(text: &str, props: &TextProps) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len() + HEADER.len());
    out.push_str(HEADER);

    let mut runs: Vec<&Span> = props
        .spans()
        .iter()
        .filter(|s| !s.props.face.is_default())
        .collect();
    runs.sort_by_key(|s| s.start);

    let mut pos = 0usize;
    for run in runs {
        let start = run.start.min(chars.len());
        let end = run.end.min(chars.len());
        if start >= end {
            continue;
        }
        if pos < start {
            out.push_str(&escape(&chars[pos..start].iter().collect::<String>()));
        }
        let anns = annotations(&run.props.face);
        for (tag, param) in &anns {
            out.push('<');
            out.push_str(tag);
            out.push('>');
            if let Some(value) = param {
                out.push_str("<param>");
                out.push_str(&escape(value));
                out.push_str("</param>");
            }
        }
        out.push_str(&escape(&chars[start..end].iter().collect::<String>()));
        for (tag, _) in anns.iter().rev() {
            out.push_str("</");
            out.push_str(tag);
            out.push('>');
        }
        pos = end;
    }
    if pos < chars.len() {
        out.push_str(&escape(&chars[pos..].iter().collect::<String>()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_is_detected_and_stripped() {
        let src = format!("{HEADER}hello");
        assert!(has_header(&src));
        assert!(!has_header("hello"));
        assert_eq!(decode(&src).text, "hello");
    }

    #[test]
    fn bold_annotation_becomes_a_face_run() {
        let d = decode("a <bold>bee</bold> c");
        assert_eq!(d.text, "a bee c");
        let spans = d.props.spans();
        assert_eq!(spans.len(), 1);
        assert_eq!((spans[0].start, spans[0].end), (2, 5));
        assert_eq!(spans[0].props.face, Face::bold());
    }

    #[test]
    fn nested_annotations_merge_into_one_face() {
        let d = decode("<bold><italic>x</italic></bold>");
        assert_eq!(d.text, "x");
        assert_eq!(d.props.props_at(0).unwrap().face, Face::bold_italic());
    }

    #[test]
    fn double_left_angle_decodes_to_a_literal_one() {
        let d = decode("a <<b <bold>c</bold>");
        assert_eq!(d.text, "a <b c");
        assert_eq!(d.props.props_at(5).unwrap().face, Face::bold());
    }

    #[test]
    fn color_param_is_parsed_by_name_and_by_hex() {
        let d = decode("<x-color><param>red</param>r</x-color>");
        assert_eq!(d.props.props_at(0).unwrap().face.fg, Some((255, 0, 0)));
        let d = decode("<x-bg-color><param>#00ff00</param>g</x-bg-color>");
        assert_eq!(d.props.props_at(0).unwrap().face.bg, Some((0, 255, 0)));
    }

    #[test]
    fn twelve_digit_x11_color_is_truncated_to_eight_bits() {
        let d = decode("<x-color><param>#ffff00000000</param>r</x-color>");
        assert_eq!(d.props.props_at(0).unwrap().face.fg, Some((255, 0, 0)));
    }

    #[test]
    fn unknown_annotations_are_dropped_but_their_text_is_kept() {
        let d = decode("<flushright>hi</flushright>");
        assert_eq!(d.text, "hi");
        assert!(d.props.is_empty());
    }

    #[test]
    fn unclosed_annotation_runs_to_the_end() {
        let d = decode("a <bold>bcd");
        assert_eq!(d.text, "a bcd");
        assert_eq!(d.props.props_at(4).unwrap().face, Face::bold());
    }

    #[test]
    fn encode_then_decode_round_trips_faces_and_text() {
        let text = "plain bold red < end";
        let mut props = TextProps::new();
        props.add_face(6..10, &Face::bold());
        props.add_face(11..14, &Face::foreground((255, 0, 0)));
        let file = encode(text, &props);
        assert!(file.starts_with(HEADER));
        let back = decode(&file);
        assert_eq!(back.text, text);
        assert_eq!(back.props, props);
    }

    #[test]
    fn encode_escapes_a_literal_left_angle_in_the_text() {
        let file = encode("a < b", &TextProps::new());
        assert!(file.ends_with("a << b"), "{file}");
        assert_eq!(decode(&file).text, "a < b");
    }

    #[test]
    fn named_faces_decompose_to_the_attributes_the_format_carries() {
        let mut props = TextProps::new();
        props.add_face(0..2, &Face::named("bold-italic"));
        let file = encode("hi", &props);
        assert!(file.contains("<bold><italic>hi</italic></bold>"), "{file}");
    }

    #[test]
    fn a_face_with_only_a_theme_scope_name_encodes_as_plain_text() {
        let mut props = TextProps::new();
        props.add_face(0..2, &Face::named("font-lock-keyword-face"));
        let file = encode("hi", &props);
        assert!(file.ends_with("hi"), "{file}");
    }

    #[test]
    fn invisible_runs_are_not_written_to_the_file() {
        let mut props = TextProps::new();
        props.set_invisible(0..2, true);
        let file = encode("hi", &props);
        assert!(file.ends_with("hi"), "{file}");
    }
}
