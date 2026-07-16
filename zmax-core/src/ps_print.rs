//! Pure PostScript generation backing the zmax port of GNU Emacs `ps-print`
//! (`ps-print-buffer`, `ps-spool-buffer`, `ps-print-region`, …).
//!
//! This module is filesystem- and printer-free: it turns buffer text into a
//! self-contained PostScript document (monospaced Courier, one header line per
//! page carrying the title and `page N/M`, hard page breaks every
//! `lines_per_page` lines). The term layer either spools the string into a
//! buffer (`ps-spool-*`) or pipes it to `lpr` (`ps-print-*` / `ps-despool`).
//!
//! Faces/colours are NOT reproduced — the `*-with-faces` commands share this
//! plain builder and are marked partial, matching the honest coverage bar (real
//! ps-print colourises via font-lock, which needs a face-extraction pass zmax
//! does not expose here). The output is valid DSC-conforming PostScript level 2.

/// Layout options for [`to_postscript`]. Defaults mirror ps-print's US-letter,
/// 10pt Courier, 66-line page.
#[derive(Clone, Debug)]
pub struct PsOptions {
    /// Point size of the Courier body font.
    pub font_size: f64,
    /// Body lines per printed page (before a hard page break).
    pub lines_per_page: usize,
    /// Title shown in the per-page header (usually the buffer name).
    pub title: String,
}

impl Default for PsOptions {
    fn default() -> Self {
        PsOptions {
            font_size: 10.0,
            lines_per_page: 66,
            title: String::new(),
        }
    }
}

/// Escape a run of text for a PostScript literal string: `(`, `)` and `\` are
/// backslash-escaped, and non-printable/8-bit bytes become `\ooo` octal escapes
/// so the `(...)` string stays well-formed and ASCII-safe.
pub fn escape_ps(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '(' => out.push_str("\\("),
            ')' => out.push_str("\\)"),
            c if (' '..='~').contains(&c) => out.push(c),
            c => {
                // Octal-escape each UTF-8 byte (PostScript strings are byte-wise).
                let mut buf = [0u8; 4];
                for b in c.encode_utf8(&mut buf).bytes() {
                    out.push_str(&format!("\\{b:03o}"));
                }
            }
        }
    }
    out
}

/// Split `text` into printed pages of at most `lines_per_page` lines each. A
/// trailing newline does not create an extra empty page; an empty buffer yields
/// a single (blank) page so there is always something to print.
fn paginate(text: &str, lines_per_page: usize) -> Vec<Vec<&str>> {
    let lpp = lines_per_page.max(1);
    let lines: Vec<&str> = if text.is_empty() {
        vec![""]
    } else {
        // `lines()` already drops a single trailing newline.
        text.lines().collect()
    };
    let lines = if lines.is_empty() { vec![""] } else { lines };
    lines.chunks(lpp).map(|c| c.to_vec()).collect()
}

/// Build a complete PostScript document rendering `text` per `opts`. The result
/// is deterministic (no timestamps) so it is unit-testable.
pub fn to_postscript(text: &str, opts: &PsOptions) -> String {
    let pages = paginate(text, opts.lines_per_page);
    let total = pages.len();
    let size = opts.font_size;
    let leading = size * 1.2; // baseline-to-baseline
    let left = 72.0; // 1" left margin
    let top = 720.0; // start body just under a 1" top margin (11" = 792pt)
    let title = escape_ps(&opts.title);

    let mut out = String::new();
    out.push_str("%!PS-Adobe-3.0\n");
    out.push_str("%%Creator: zmax ps-print\n");
    out.push_str(&format!("%%Pages: {total}\n"));
    out.push_str("%%DocumentData: Clean7Bit\n");
    out.push_str("%%EndComments\n");

    for (i, lines) in pages.iter().enumerate() {
        let page = i + 1;
        out.push_str(&format!("%%Page: {page} {page}\n"));
        // Header line: "title    page N/M", then the body.
        out.push_str(&format!("/Courier findfont {size} scalefont setfont\n"));
        out.push_str(&format!("{left} 756 moveto ({title}) show\n"));
        out.push_str(&format!(
            "{} 756 moveto (page {page}/{total}) show\n",
            left + 396.0
        ));
        let mut y = top;
        for line in lines {
            out.push_str(&format!(
                "{left} {y:.1} moveto ({}) show\n",
                escape_ps(line)
            ));
            y -= leading;
        }
        out.push_str("showpage\n");
    }
    out.push_str("%%EOF\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_ps_special_chars() {
        assert_eq!(escape_ps("a(b)c\\d"), "a\\(b\\)c\\\\d");
        assert_eq!(escape_ps("plain text"), "plain text");
        // Non-ASCII becomes octal byte escapes (é = U+00E9 = C3 A9).
        assert_eq!(escape_ps("é"), "\\303\\251");
        // Tab (control char) is octal-escaped, not passed through.
        assert_eq!(escape_ps("\t"), "\\011");
    }

    #[test]
    fn paginate_respects_page_size() {
        let text = "l1\nl2\nl3\nl4\nl5";
        let pages = paginate(text, 2);
        assert_eq!(pages.len(), 3);
        assert_eq!(pages[0], vec!["l1", "l2"]);
        assert_eq!(pages[2], vec!["l5"]);
    }

    #[test]
    fn trailing_newline_no_extra_page() {
        assert_eq!(paginate("a\nb\n", 10).len(), 1);
        assert_eq!(paginate("a\nb\n", 10)[0], vec!["a", "b"]);
    }

    #[test]
    fn empty_buffer_is_one_blank_page() {
        let ps = to_postscript("", &PsOptions::default());
        assert!(ps.starts_with("%!PS-Adobe-3.0\n"));
        assert!(ps.contains("%%Pages: 1\n"));
        assert_eq!(ps.matches("showpage").count(), 1);
        assert!(ps.trim_end().ends_with("%%EOF"));
    }

    #[test]
    fn document_structure_and_page_count() {
        let opts = PsOptions {
            font_size: 10.0,
            lines_per_page: 2,
            title: "buf(1)".into(),
        };
        // 5 lines at 2 per page -> 3 pages.
        let ps = to_postscript("one\ntwo\nthree\nfour\nfive", &opts);
        assert!(ps.contains("%%Pages: 3\n"));
        // one %%Page: per page, one showpage per page
        assert_eq!(ps.matches("%%Page:").count(), 3);
        assert_eq!(ps.matches("showpage").count(), 3);
        // title is escaped in the header
        assert!(ps.contains("(buf\\(1\\)) show"));
        // body lines are shown
        assert!(ps.contains("(one) show"));
        assert!(ps.contains("(five) show"));
        assert!(ps.contains("page 3/3"));
    }
}
