//! Emacs `rebox2` (François Pinard's `rebox.el`, maintained by Le Wang), the
//! package the spacemacs `+tools/rebox` layer wraps: draw, redraw, reshape and
//! remove the comment boxes around a block of text.
//!
//! # Style numbers
//!
//! rebox2 names a style with a three-digit number. The hundreds digit is the
//! language, that is the comment delimiter — 100 none, 200 `/*`, 300 `//`,
//! 400 `#`, 500 `;`, 600 `%`. The tens digit is the quality: for languages 100
//! and 200 it selects simple / rounded / starred frames, for the others it is
//! the thickness in comment characters of the left and right sides (10, 20, 30
//! or 40 for 1 to 4). The units digit is the type: 1 openings and closings
//! only, 2 boxed on every side but the top, 3 boxed on all sides, 4 and 5 the
//! bolder variants of 2 and 3.
//!
//! A template numbered below 100 is generic: rebox2 registers it once per
//! language, substituting the language's comment character for `?`. This port
//! keeps the generic number as the style code and takes the comment token as a
//! parameter instead, because the caller (a `:` command) already knows the
//! buffer's comment token and rebox2's four-language table does not cover every
//! language zmax highlights. Two consequences:
//!
//! * `?` is replaced by the whole comment token, not by a single character, so
//!   a quality-1 style such as 72 draws `// ---------` for a `//` buffer where
//!   rebox2 would need style 82 to get the same frame from `/`.
//! * [`styles`] lists the generic codes (10..=86) once, not once per language.
//!   [`boxed`] still accepts rebox2's fully-qualified numbers 3xx..=6xx and
//!   resolves the comment character from the hundreds digit, and it accepts the
//!   language-specific 1xx (text) and 2xx (C) codes directly.
//!
//! Styles 71..=86 are the ones the spacemacs layer registers itself in
//! `rebox/init-rebox2`; rebox2's own `rebox-style-loop` default is
//! `'(21 25 27)`, and this port exposes the layer's three-style loop as
//! [`DEFAULT_STYLE_LOOP`]. Language 700 (`"`, for vimscript) is an addition of
//! this port; rebox2 has no such language.
//!
//! # Geometry
//!
//! Templates are decomposed exactly as `rebox-register-template` does, into
//! `nw`/`nn`/`ne` (top left, top ruler character, top right), `ww`/`ee` (the
//! sides) and `sw`/`ss`/`se` (bottom). A first or third template line shorter
//! than the box line is a "merged" opening or closing delimiter that stands on
//! its own line, as in style 221's `/*` and ` */`.
//!
//! One deviation: `rebox-build` right-strips a box line when the style has no
//! right border, which leaves the frame lines and the text lines at different
//! lengths. [`boxed`] pads every line of a non-merged box to the same width so
//! the frame lines up; `all_lines_of_every_full_frame_style_are_equal_length`
//! in the tests holds it to that.

/// A box style: rebox2's style number.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Style {
    /// The style number — two digits for a generic style, three for a
    /// language-specific one.
    pub code: u16,
}

impl Style {
    /// Wrap a style number.
    pub fn new(code: u16) -> Self {
        Style { code }
    }

    /// The language digit (the hundreds digit), or 0 for a generic style.
    pub fn language(self) -> u16 {
        self.code / 100
    }

    /// The quality digit (the tens digit).
    pub fn quality(self) -> u16 {
        (self.code / 10) % 10
    }

    /// The type digit (the units digit): 1 openings and closings, 2 boxed
    /// except on top, 3 boxed all round, 4 and 5 the bold variants.
    pub fn kind(self) -> u16 {
        self.code % 10
    }
}

/// The style loop the spacemacs layer's `SPC x b b` / `SPC x b B` cycle
/// through. rebox2's own `rebox-style-loop` default is `[21, 25, 27]`.
pub const DEFAULT_STYLE_LOOP: &[u16] = &[71, 72, 73];

/// `rebox-language-character-alist`, plus this port's language 7.
const LANGUAGE_CHARS: &[(u16, &str)] = &[(3, "/"), (4, "#"), (5, ";"), (6, "%"), (7, "\"")];

/// Comment tokens [`unbox`] tries when it has to guess the language.
const GUESS_TOKENS: &[&str] = &[";", "#", "//", "%", "\"", "/*", "/"];

/// `rebox-templates`: `(style, recognition weight, lines)`. Codes below 100 are
/// generic and carry `?` where the comment token goes. 71..=86 come from the
/// spacemacs layer's `rebox-register-template` calls.
const TEMPLATES: &[(u16, u16, &[&str])] = &[
    // Generic programming-language templates.
    (10, 114, &["?box123456"]),
    (11, 115, &["? box123456"]),
    (12, 215, &["? box123456 ?", "? --------- ?"]),
    (
        13,
        315,
        &["? --------- ?", "? box123456 ?", "? --------- ?"],
    ),
    (14, 415, &["? box123456 ?", "?????????????"]),
    (
        15,
        515,
        &["?????????????", "? box123456 ?", "?????????????"],
    ),
    (16, 126, &["?,----", "?| box123456", "?`----"]),
    (17, 226, &["?,----------", "?| box123456", "?`----------"]),
    (20, 124, &["??box123456"]),
    (21, 125, &["?? box123456"]),
    (22, 225, &["?? box123456 ??", "?? --------- ??"]),
    (
        23,
        325,
        &["?? --------- ??", "?? box123456 ??", "?? --------- ??"],
    ),
    (24, 425, &["?? box123456 ??", "???????????????"]),
    (
        25,
        525,
        &["???????????????", "?? box123456 ??", "???????????????"],
    ),
    (26, 126, &["??,----", "??| box123456", "??`----"]),
    (
        27,
        226,
        &["??,----------", "??| box123456", "??`----------"],
    ),
    (30, 134, &["???box123456"]),
    (31, 135, &["??? box123456"]),
    (32, 235, &["??? box123456 ???", "??? --------- ???"]),
    (
        33,
        335,
        &[
            "??? --------- ???",
            "??? box123456 ???",
            "??? --------- ???",
        ],
    ),
    (34, 435, &["??? box123456 ???", "?????????????????"]),
    (
        35,
        535,
        &[
            "?????????????????",
            "??? box123456 ???",
            "?????????????????",
        ],
    ),
    (40, 144, &["????box123456"]),
    (41, 145, &["???? box123456"]),
    (42, 245, &["???? box123456 ????", "???? --------- ????"]),
    (
        43,
        345,
        &[
            "???? --------- ????",
            "???? box123456 ????",
            "???? --------- ????",
        ],
    ),
    (44, 445, &["???? box123456 ????", "???????????????????"]),
    (
        45,
        545,
        &[
            "???????????????????",
            "???? box123456 ????",
            "???????????????????",
        ],
    ),
    (50, 154, &["?????box123456"]),
    (51, 155, &["????? box123456"]),
    (60, 164, &["??????box123456"]),
    (61, 165, &["?????? box123456"]),
    // Registered by the spacemacs +tools/rebox layer.
    (71, 176, &["?", "? box123456", "?"]),
    (72, 176, &["? ---------", "? box123456", "? ---------"]),
    (73, 376, &["? =========", "? box123456", "? ========="]),
    (74, 176, &["?-----------", "? box123456 ", "?-----------"]),
    (75, 276, &["?-----------+", "? box123456 ", "?-----------+"]),
    (76, 376, &["?===========", "? box123456 ", "?==========="]),
    (81, 176, &["??", "?? box123456", "??"]),
    (82, 286, &["?? ---------", "?? box123456", "?? ---------"]),
    (83, 486, &["?? =========", "?? box123456", "?? ========="]),
    (
        84,
        286,
        &["??-----------", "?? box123456 ", "??-----------"],
    ),
    (
        85,
        386,
        &["??-----------+", "?? box123456 ", "??-----------+"],
    ),
    (
        86,
        486,
        &["??===========", "?? box123456 ", "??==========="],
    ),
    // Text mode (language 100).
    (111, 113, &["box123456"]),
    (112, 213, &["| box123456 |", "+-----------+"]),
    (
        113,
        313,
        &["+-----------+", "| box123456 |", "+-----------+"],
    ),
    (114, 413, &["| box123456 |", "*===========*"]),
    (
        115,
        513,
        &["*===========*", "| box123456 |", "*===========*"],
    ),
    (116, 114, &["---------", "box123456", "---------"]),
    (121, 243, &["| box123456 |"]),
    (122, 223, &["| box123456 |", "`-----------'"]),
    (
        123,
        323,
        &[".-----------.", "| box123456 |", "`-----------'"],
    ),
    (124, 423, &["| box123456 |", "\\===========/"]),
    (
        125,
        523,
        &["/===========\\", "| box123456 |", "\\===========/"],
    ),
    (126, 126, &[",----", "| box123456", "`----"]),
    (127, 226, &[",----------", "| box123456", "`----------"]),
    (136, 126, &[",----", "| box123456", "`----"]),
    (137, 226, &[",----------", "| box123456", "`----------"]),
    (141, 143, &["| box123456 "]),
    (142, 243, &["* box123456 *", "*************"]),
    (
        143,
        343,
        &["*************", "* box123456 *", "*************"],
    ),
    (144, 443, &["X box123456 X", "XXXXXXXXXXXXX"]),
    (
        145,
        543,
        &["XXXXXXXXXXXXX", "X box123456 X", "XXXXXXXXXXXXX"],
    ),
    // C language (language 200).
    (211, 118, &["/* box123456 */"]),
    (212, 218, &["/* box123456 */", "/* --------- */"]),
    (
        213,
        318,
        &["/* --------- */", "/* box123456 */", "/* --------- */"],
    ),
    (214, 418, &["/* box123456 */", "/* ========= */"]),
    (
        215,
        518,
        &["/* ========= */", "/* box123456 */", "/* ========= */"],
    ),
    (221, 128, &["/*", "   box123456", " */"]),
    (
        222,
        228,
        &["/*          .", " | box123456 |", " `----------*/"],
    ),
    (
        223,
        328,
        &["/*----------.", "| box123456 |", "`----------*/"],
    ),
    (
        224,
        428,
        &["/*          \\", "| box123456 |", "\\==========*/"],
    ),
    (
        225,
        528,
        &["/*==========\\", "| box123456 |", "\\==========*/"],
    ),
    (231, 138, &["/*", " | box123456", " */"]),
    (
        232,
        238,
        &["/*             ", " | box123456 | ", " *-----------*/"],
    ),
    (
        233,
        338,
        &["/*-----------* ", " | box123456 | ", " *-----------*/"],
    ),
    (234, 438, &["/* box123456 */", "/*-----------*/"]),
    (
        235,
        538,
        &["/*-----------*/", "/* box123456 */", "/*-----------*/"],
    ),
    (241, 148, &["/*", " * box123456", " */"]),
    (
        242,
        248,
        &["/*           * ", " * box123456 * ", " *************/"],
    ),
    (
        243,
        348,
        &["/************* ", " * box123456 * ", " *************/"],
    ),
    (244, 448, &["/* box123456 */", "/*************/"]),
    (
        245,
        548,
        &["/*************/", "/* box123456 */", "/*************/"],
    ),
    (
        246,
        248,
        &["/************//**", " * box123456 ", " ****************/"],
    ),
];

/// Every style this port knows, in rebox2's numeric order. Codes below 100 are
/// generic and need the buffer's comment token; 1xx and 2xx are
/// language-specific.
pub fn styles() -> Vec<u16> {
    let mut v: Vec<u16> = TEMPLATES.iter().map(|(code, _, _)| *code).collect();
    v.sort_unstable();
    v
}

/// `rebox-rstrip`.
fn rstrip(s: &str) -> &str {
    s.trim_end_matches([' ', '\t'])
}

fn lstrip(s: &str) -> &str {
    s.trim_start_matches([' ', '\t'])
}

fn pad_to(s: &mut String, n: usize) {
    let len = s.chars().count();
    if len < n {
        s.extend(std::iter::repeat_n(' ', n - len));
    }
}

/// A template decomposed the way `rebox-register-template` decomposes it.
#[derive(Clone, Debug)]
struct Parsed {
    code: u16,
    weight: u16,
    /// The first template line is a standalone opening delimiter.
    merge_nw: bool,
    /// The third template line is a standalone closing delimiter.
    merge_sw: bool,
    nw: Option<String>,
    nn: Option<char>,
    ne: Option<String>,
    ww: Option<String>,
    ee: Option<String>,
    sw: Option<String>,
    ss: Option<char>,
    se: Option<String>,
}

impl Parsed {
    fn has_top(&self) -> bool {
        self.merge_nw || self.nw.is_some() || self.nn.is_some() || self.ne.is_some()
    }

    fn has_bottom(&self) -> bool {
        self.merge_sw || self.sw.is_some() || self.ss.is_some() || self.se.is_some()
    }

    /// A style with no frame at all (rebox2's 111, "delete the box") is never a
    /// detection candidate.
    fn has_decoration(&self) -> bool {
        self.has_top() || self.has_bottom() || self.ww.is_some() || self.ee.is_some()
    }

    /// How far right the sides sit, given an inner text width.
    fn right_margin(&self, inner: usize) -> usize {
        self.ww.as_deref().map(str::len).unwrap_or(0) + inner
    }

    /// The width of the widest right-hand piece, so every line can be padded to
    /// the same length.
    fn right_border(&self) -> usize {
        [self.ne.as_deref(), self.ee.as_deref(), self.se.as_deref()]
            .iter()
            .filter_map(|o| o.map(str::len))
            .max()
            .unwrap_or(0)
    }
}

/// Decompose one template's lines, following `rebox-register-template`.
fn parse(code: u16, weight: u16, lines: &[String]) -> Result<Parsed, String> {
    let bad = || format!("erroneous template for style {code}");
    let first = lines.first().ok_or_else(bad)?;
    let (line1, line2, line3) = if first.contains("box123456") {
        (None, first.clone(), lines.get(1).cloned())
    } else {
        (
            Some(first.clone()),
            lines.get(1).cloned().ok_or_else(bad)?,
            lines.get(2).cloned(),
        )
    };
    let mb = line2.find("box123456").ok_or_else(bad)?;
    let me = mb + "box123456".len();

    let merge_nw = line1.as_ref().is_some_and(|l| l.len() < line2.len());
    let merge_sw = line3.as_ref().is_some_and(|l| l.len() < line2.len());

    let nw = match &line1 {
        None => None,
        Some(l) if merge_nw => Some(l.clone()),
        Some(_) if mb == 0 => None,
        Some(l) => Some(l[..mb.min(l.len())].to_string()),
    };
    let nn = match &line1 {
        Some(l) if !merge_nw && mb < l.len() => {
            let c = l.as_bytes()[mb] as char;
            (c != ' ').then_some(c)
        }
        _ => None,
    };
    let ne = match &line1 {
        Some(l) if !merge_nw && me < l.len() => Some(rstrip(&l[me..]).to_string()),
        _ => None,
    };

    let ww = (mb != 0).then(|| line2[..mb].to_string());
    let ee = if me < line2.len() {
        let s = rstrip(&line2[me..]);
        (!s.is_empty()).then(|| s.to_string())
    } else {
        None
    };

    let sw = match &line3 {
        None => None,
        Some(l) if merge_sw => Some(rstrip(l).to_string()),
        Some(_) if mb == 0 => None,
        Some(l) => Some(l[..mb.min(l.len())].to_string()),
    };
    let ss = match &line3 {
        Some(l) if !merge_sw && mb < l.len() => {
            let c = l.as_bytes()[mb] as char;
            (c != ' ').then_some(c)
        }
        _ => None,
    };
    let se = match &line3 {
        Some(l) if !merge_sw && me < l.len() => Some(rstrip(&l[me..]).to_string()),
        _ => None,
    };

    Ok(Parsed {
        code,
        weight,
        merge_nw,
        merge_sw,
        nw,
        nn,
        ne,
        ww,
        ee,
        sw,
        ss,
        se,
    })
}

fn template(code: u16) -> Option<(u16, u16, &'static [&'static str])> {
    TEMPLATES.iter().copied().find(|(c, _, _)| *c == code)
}

/// Instantiate `code` for `comment`. Generic codes take the comment token;
/// rebox2's fully-qualified 3xx..=7xx codes take the language's character from
/// the hundreds digit; 1xx and 2xx are used as written.
fn style(code: u16, comment: &str) -> Result<Parsed, String> {
    let unknown = || format!("unknown rebox style {code}");
    let st = Style::new(code);
    if code < 100 {
        if comment.is_empty() {
            return Err(format!("generic rebox style {code} needs a comment token"));
        }
        let (c, w, lines) = template(code).ok_or_else(unknown)?;
        let inst: Vec<String> = lines.iter().map(|l| l.replace('?', comment)).collect();
        parse(c, w, &inst)
    } else if code < 300 {
        let (c, w, lines) = template(code).ok_or_else(unknown)?;
        let inst: Vec<String> = lines.iter().map(|l| (*l).to_string()).collect();
        parse(c, w, &inst)
    } else {
        let ch = LANGUAGE_CHARS
            .iter()
            .find(|(lang, _)| *lang == st.language())
            .map(|(_, ch)| *ch)
            .ok_or_else(unknown)?;
        let generic = code % 100;
        let (_, w, lines) = template(generic)
            .filter(|(c, _, _)| *c < 100)
            .ok_or_else(unknown)?;
        let inst: Vec<String> = lines.iter().map(|l| l.replace('?', ch)).collect();
        parse(code, w, &inst)
    }
}

/// Draw `text` as style `code`. `comment` is the buffer's line-comment token
/// (`";"`, `"#"`, `"//"`, …) and is used only by the generic styles. `width` is
/// the box's inner width; a box is widened when a line of `text` does not fit.
/// A trailing newline on the input is kept on the output.
pub fn boxed(text: &str, code: u16, comment: &str, width: usize) -> Result<String, String> {
    let p = style(code, comment)?;

    let had_newline = text.ends_with('\n');
    let body = if had_newline {
        &text[..text.len() - 1]
    } else {
        text
    };
    let content: Vec<&str> = body.split('\n').collect();
    let inner = content
        .iter()
        .map(|l| l.chars().count())
        .max()
        .unwrap_or(0)
        .max(width);

    let ww = p.ww.clone().unwrap_or_default();
    let right = p.right_margin(inner);
    let rb = p.right_border();
    let total = right + rb;

    let mut out: Vec<String> = Vec::new();

    if p.merge_nw {
        out.push(p.nw.clone().unwrap_or_default());
    } else if p.has_top() {
        let nw = p.nw.clone().unwrap_or_default();
        let mut line = nw.clone();
        if p.nn.is_some() || p.ne.is_some() {
            let fill = right.saturating_sub(nw.len());
            line.extend(std::iter::repeat_n(p.nn.unwrap_or(' '), fill));
            line.push_str(p.ne.as_deref().unwrap_or(""));
            pad_to(&mut line, total);
        }
        out.push(line);
    }

    for l in &content {
        let mut line = ww.clone();
        line.push_str(l);
        pad_to(&mut line, right);
        line.push_str(p.ee.as_deref().unwrap_or(""));
        pad_to(&mut line, total);
        out.push(line);
    }

    if p.merge_sw {
        out.push(p.sw.clone().unwrap_or_default());
    } else if p.has_bottom() {
        let sw = p.sw.clone().unwrap_or_default();
        let mut line = sw.clone();
        if p.ss.is_some() || p.se.is_some() {
            let fill = right.saturating_sub(sw.len());
            line.extend(std::iter::repeat_n(p.ss.unwrap_or(' '), fill));
            line.push_str(p.se.as_deref().unwrap_or(""));
            pad_to(&mut line, total);
        }
        out.push(line);
    }

    let mut s = out.join("\n");
    if had_newline {
        s.push('\n');
    }
    Ok(s)
}

/// Whether `line` is the top or bottom frame of a style. `rebox-regexp-quote`
/// left-strips the corner it quotes (the `^[ \t]*` in front of the pattern has
/// already eaten the indentation) and right-strips the closing corner, so the
/// template pieces are stripped the same way here.
fn frame_matches(
    line: &str,
    merged: bool,
    w: Option<&str>,
    ruler: Option<char>,
    e: Option<&str>,
) -> bool {
    if merged {
        return rstrip(lstrip(line)) == rstrip(lstrip(w.unwrap_or("")));
    }
    // The corner keeps its own trailing space (`:rstrip nil`), so the line is
    // only right-stripped after the corner has been matched off the front.
    let t = lstrip(line);
    let w = lstrip(w.unwrap_or(""));
    let e = rstrip(e.unwrap_or(""));
    let Some(rest) = t.strip_prefix(w) else {
        return false;
    };
    let rest = rstrip(rest);
    if !rest.ends_with(e) || rest.len() < e.len() {
        return false;
    }
    let mid = &rest[..rest.len() - e.len()];
    match ruler {
        Some(c) => rstrip(mid).chars().all(|ch| ch == c),
        None => mid.chars().all(|ch| ch == ' ' || ch == '\t'),
    }
}

/// Whether `line` is a text line of a style. rebox2 drops the text-line pattern
/// altogether when the sides carry nothing but whitespace (style 221's `ww` is
/// three spaces), and accepts a line whose trailing whitespace was trimmed
/// away, hence the `rstrip(ww)` alternative.
fn middle_matches(line: &str, ww: Option<&str>, ee: Option<&str>) -> bool {
    let sides = format!("{}{}", ww.unwrap_or(""), ee.unwrap_or(""));
    if rstrip(&sides).is_empty() {
        return true;
    }
    let t = rstrip(lstrip(line));
    let w = ww.map(lstrip);
    if let Some(w) = w {
        let ok = t.starts_with(w) || (ee.is_none() && t == rstrip(w));
        if !ok {
            return false;
        }
    }
    if let Some(e) = ee {
        let e = rstrip(e);
        if !t.ends_with(e) || t.len() < w.map(str::len).unwrap_or(0) + e.len() {
            return false;
        }
    }
    true
}

fn style_matches(p: &Parsed, lines: &[&str]) -> bool {
    let top = p.has_top();
    let bottom = p.has_bottom();
    if lines.len() < 1 + top as usize + bottom as usize {
        return false;
    }
    let start = top as usize;
    let end = lines.len() - bottom as usize;
    if end <= start {
        return false;
    }
    if top && !frame_matches(lines[0], p.merge_nw, p.nw.as_deref(), p.nn, p.ne.as_deref()) {
        return false;
    }
    if bottom
        && !frame_matches(
            lines[end],
            p.merge_sw,
            p.sw.as_deref(),
            p.ss,
            p.se.as_deref(),
        )
    {
        return false;
    }
    lines[start..end]
        .iter()
        .all(|l| middle_matches(l, p.ww.as_deref(), p.ee.as_deref()))
}

/// The heaviest style matching `text` for `comment`, mirroring
/// `rebox-guess-style`; ties go to the lower style number so the answer is
/// deterministic.
fn best_match(text: &str, comment: &str) -> Option<Parsed> {
    let body = text.strip_suffix('\n').unwrap_or(text);
    let lines: Vec<&str> = body.split('\n').collect();
    let mut best: Option<Parsed> = None;
    for (code, _, _) in TEMPLATES {
        if *code < 100 && comment.is_empty() {
            continue;
        }
        let Ok(p) = style(*code, comment) else {
            continue;
        };
        if !p.has_decoration() || !style_matches(&p, &lines) {
            continue;
        }
        let better = match &best {
            None => true,
            Some(b) => p.weight > b.weight,
        };
        if better {
            best = Some(p);
        }
    }
    best
}

/// Detect which style `text` currently is, if it is a box at all. `comment` is
/// the buffer's line-comment token; the language-specific 1xx and 2xx styles
/// are always considered as well.
pub fn detect_style(text: &str, comment: &str) -> Option<u16> {
    best_match(text, comment).map(|p| p.code)
}

/// Remove the frame a `Parsed` style would have drawn.
fn strip_with(p: &Parsed, lines: &[&str]) -> String {
    let start = p.has_top() as usize;
    let end = lines.len() - p.has_bottom() as usize;
    lines[start..end]
        .iter()
        .map(|line| {
            let mut t = lstrip(line);
            if let Some(w) = &p.ww {
                if let Some(rest) = t.strip_prefix(w.as_str()) {
                    t = rest;
                } else if t == rstrip(w) {
                    t = "";
                }
            }
            let mut t = rstrip(t);
            if let Some(e) = &p.ee {
                if let Some(rest) = t.strip_suffix(e.as_str()) {
                    t = rstrip(rest);
                }
            }
            t.to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Characters this port treats as frame drawing when it has to un-box text
/// whose style it could not identify.
fn is_frame_char(c: char) -> bool {
    matches!(
        c,
        '-' | '=' | '*' | '+' | '|' | '~' | '.' | ',' | '\'' | '`' | '_' | '/' | '\\' | 'X'
    )
}

/// Un-box text whose style was not recognised: drop the comment token from each
/// line and drop lines that are nothing but frame characters. A heuristic —
/// [`unbox`] uses it only when no registered style matches.
fn unbox_generic(text: &str) -> String {
    let body = text.strip_suffix('\n').unwrap_or(text);
    body.split('\n')
        .filter_map(|line| {
            let t = lstrip(line);
            let t = t
                .strip_prefix("/*")
                .or_else(|| t.strip_prefix("*/"))
                .unwrap_or(t);
            let t = t.trim_start_matches([';', '#', '%', '/', '"']);
            let t = rstrip(t);
            let stripped = t.trim_start_matches(' ');
            if !stripped.is_empty() && stripped.chars().all(is_frame_char) {
                return None;
            }
            Some(stripped.to_string())
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Strip any box decoration from `text`, returning the comment content.
/// rebox2 always un-boxes before it re-boxes. The style is guessed across every
/// comment token this port knows; text that matches no style is passed through
/// [`unbox_generic`].
pub fn unbox(text: &str) -> String {
    let body = text.strip_suffix('\n').unwrap_or(text);
    let lines: Vec<&str> = body.split('\n').collect();
    let mut best: Option<Parsed> = None;
    for token in GUESS_TOKENS {
        if let Some(p) = best_match(body, token) {
            if best.as_ref().is_none_or(|b| p.weight > b.weight) {
                best = Some(p);
            }
        }
    }
    match best {
        Some(p) => strip_with(&p, &lines),
        None => unbox_generic(body),
    }
}

/// The style after `current` in `loop_styles`, wrapping at the end
/// (`SPC x b b`). A `current` that is not in the loop starts it from the
/// beginning; an empty loop leaves `current` alone.
pub fn next_style(current: u16, loop_styles: &[u16]) -> u16 {
    if loop_styles.is_empty() {
        return current;
    }
    match loop_styles.iter().position(|s| *s == current) {
        Some(i) => loop_styles[(i + 1) % loop_styles.len()],
        None => loop_styles[0],
    }
}

/// The style before `current` in `loop_styles`, wrapping at the start
/// (`SPC x b B`).
pub fn prev_style(current: u16, loop_styles: &[u16]) -> u16 {
    if loop_styles.is_empty() {
        return current;
    }
    match loop_styles.iter().position(|s| *s == current) {
        Some(i) => loop_styles[(i + loop_styles.len() - 1) % loop_styles.len()],
        None => loop_styles[loop_styles.len() - 1],
    }
}

fn leading_spaces(line: &str) -> usize {
    line.chars().take_while(|c| *c == ' ' || *c == '\t').count()
}

/// The left margin shared by every non-blank line.
fn margin_of(lines: &[&str]) -> usize {
    lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| leading_spaces(l))
        .min()
        .unwrap_or(0)
}

/// Shift a box `n` columns right, or `-n` columns left (`SPC x b >` /
/// `SPC x b <`). A left shift stops at the leftmost line so the box keeps its
/// shape and its own width.
pub fn shift(text: &str, n: i32) -> String {
    if n == 0 {
        return text.to_string();
    }
    let had_newline = text.ends_with('\n');
    let body = text.strip_suffix('\n').unwrap_or(text);
    let lines: Vec<&str> = body.split('\n').collect();
    let out: Vec<String> = if n > 0 {
        let pad = " ".repeat(n as usize);
        lines
            .iter()
            .map(|l| {
                if l.is_empty() {
                    String::new()
                } else {
                    format!("{pad}{l}")
                }
            })
            .collect()
    } else {
        let k = margin_of(&lines).min(n.unsigned_abs() as usize);
        lines
            .iter()
            .map(|l| l.chars().skip(k.min(leading_spaces(l))).collect())
            .collect()
    };
    let mut s = out.join("\n");
    if had_newline {
        s.push('\n');
    }
    s
}

/// Centre a box within `width` columns (`SPC x b c`). Every line moves by the
/// same amount, so the box's own width is unchanged.
pub fn center(text: &str, width: usize) -> String {
    let body = text.strip_suffix('\n').unwrap_or(text);
    let lines: Vec<&str> = body.split('\n').collect();
    let margin = margin_of(&lines);
    let box_width = lines
        .iter()
        .map(|l| l.chars().count())
        .max()
        .unwrap_or(0)
        .saturating_sub(margin);
    let target = width.saturating_sub(box_width) / 2;
    shift(text, target as i32 - margin as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The width of a block ignoring its shared left margin.
    fn block_width(text: &str) -> usize {
        let lines: Vec<&str> = text.split('\n').collect();
        lines
            .iter()
            .map(|l| l.chars().count())
            .max()
            .unwrap_or(0)
            .saturating_sub(margin_of(&lines))
    }

    #[test]
    fn styles_are_numeric_and_cover_the_loop_and_the_language_tables() {
        let s = styles();
        let mut sorted = s.clone();
        sorted.sort_unstable();
        assert_eq!(s, sorted);
        for code in DEFAULT_STYLE_LOOP {
            assert!(
                s.contains(code),
                "style loop entry {code} is not registered"
            );
        }
        // The generic table, the text-mode table and the C table.
        assert!(s.contains(&21) && s.contains(&86));
        assert!(s.contains(&111) && s.contains(&145));
        assert!(s.contains(&211) && s.contains(&246));
        assert_eq!(s.len(), TEMPLATES.len());
    }

    #[test]
    fn semicolon_boxes_at_the_layer_default_styles() {
        assert_eq!(boxed("hello", 71, ";", 9).unwrap(), ";\n; hello    \n;");
        assert_eq!(
            boxed("hello", 72, ";", 9).unwrap(),
            "; ---------\n; hello    \n; ---------"
        );
        assert_eq!(
            boxed("hello", 73, ";", 9).unwrap(),
            "; =========\n; hello    \n; ========="
        );
    }

    #[test]
    fn slash_slash_boxes_at_the_layer_default_styles() {
        // `?` takes the whole comment token, so quality-1 styles work for C++.
        assert_eq!(
            boxed("hello", 72, "//", 9).unwrap(),
            "// ---------\n// hello    \n// ---------"
        );
        assert_eq!(boxed("hello", 71, "//", 9).unwrap(), "//\n// hello    \n//");
        // rebox2's own numbering still resolves: 372 is language 3 (`/`).
        assert_eq!(
            boxed("hello", 372, "", 9).unwrap(),
            "/ ---------\n/ hello    \n/ ---------"
        );
    }

    #[test]
    fn language_specific_styles_ignore_the_comment_token() {
        assert_eq!(
            boxed("hi", 113, "", 4).unwrap(),
            "+------+\n| hi   |\n+------+"
        );
        assert_eq!(
            boxed("hi", 213, "", 4).unwrap(),
            "/* ---- */\n/* hi   */\n/* ---- */"
        );
    }

    #[test]
    fn all_lines_of_every_full_frame_style_are_equal_length() {
        for code in styles() {
            let p = style(code, ";").unwrap();
            if p.merge_nw || p.merge_sw {
                // Opening/closing delimiters stand alone by construction.
                continue;
            }
            let text = boxed("alpha\nbeta gamma", code, ";", 12).unwrap();
            let lens: Vec<usize> = text.split('\n').map(|l| l.chars().count()).collect();
            assert!(
                lens.windows(2).all(|w| w[0] == w[1]),
                "style {code} produced ragged lines {lens:?}:\n{text}"
            );
        }
    }

    #[test]
    fn a_multi_line_paragraph_stays_inside_the_frame() {
        let text = boxed("one\ntwo two two\nthree", 73, ";", 4).unwrap();
        let lines: Vec<&str> = text.split('\n').collect();
        assert_eq!(lines.len(), 5);
        // The box widened to the longest line, and every content line is
        // padded to the same width as the rulers.
        let w = lines[0].chars().count();
        assert_eq!(w, 2 + "two two two".len());
        for l in &lines {
            assert_eq!(l.chars().count(), w, "{l:?}");
        }
        for l in &lines[1..4] {
            assert!(l.starts_with("; "), "{l:?}");
        }
    }

    #[test]
    fn box_unbox_box_round_trips() {
        for code in [71u16, 72, 73, 74, 75, 76, 81, 82, 83, 84, 85, 86] {
            for comment in [";", "#", "//"] {
                let first = boxed("alpha\nbeta gamma", code, comment, 14).unwrap();
                let content = unbox(&first);
                let second = boxed(&content, code, comment, 14).unwrap();
                assert_eq!(
                    first, second,
                    "style {code} with {comment:?} did not round trip"
                );
            }
        }
        assert_eq!(
            unbox(&boxed("alpha\nbeta", 213, "", 10).unwrap()),
            "alpha\nbeta"
        );
        assert_eq!(
            unbox(&boxed("alpha\nbeta", 123, "", 10).unwrap()),
            "alpha\nbeta"
        );
    }

    #[test]
    fn detect_style_recognises_the_layer_styles() {
        for code in [71u16, 72, 73, 74, 75, 76, 81, 82, 83, 84, 85, 86] {
            let text = boxed("alpha\nbeta", code, ";", 12).unwrap();
            assert_eq!(detect_style(&text, ";"), Some(code), "{code}:\n{text}");
        }
        for code in [113u16, 123, 213, 235] {
            let text = boxed("alpha\nbeta", code, "", 12).unwrap();
            assert_eq!(detect_style(&text, ""), Some(code), "{code}:\n{text}");
        }
    }

    #[test]
    fn detect_style_is_stable_for_every_registered_style() {
        // Detection does not always return the number the box was drawn with.
        // rebox2 keeps the heaviest matching style, and some frames are a
        // subset of a heavier one: a style-16 box (`;,----`) also matches
        // style 17, whose ruler regexp is `-+`, and 17 is heavier (226 vs
        // 126). 126 and 136 are outright identical templates. What must hold
        // is that detection succeeds for every decorated style and is a fixed
        // point — re-boxing at the detected style detects that style again.
        for code in styles() {
            let text = boxed("alpha\nbeta", code, ";", 12).unwrap();
            let Some(found) = detect_style(&text, ";") else {
                let p = style(code, ";").unwrap();
                assert!(
                    !p.has_decoration(),
                    "style {code} was not detected:\n{text}"
                );
                continue;
            };
            let again = boxed("alpha\nbeta", found, ";", 12).unwrap();
            assert_eq!(
                detect_style(&again, ";"),
                Some(found),
                "style {code} detected as {found}, which is not a fixed point:\n{again}"
            );
        }
    }

    #[test]
    fn detect_style_rejects_plain_text() {
        assert_eq!(detect_style("just some prose\nover two lines", ";"), None);
    }

    #[test]
    fn unbox_leaves_plain_text_alone() {
        assert_eq!(
            unbox("hello world\nsecond line"),
            "hello world\nsecond line"
        );
    }

    #[test]
    fn next_and_prev_style_wrap_around_the_loop() {
        let l = DEFAULT_STYLE_LOOP;
        assert_eq!(next_style(71, l), 72);
        assert_eq!(next_style(72, l), 73);
        assert_eq!(next_style(73, l), 71);
        assert_eq!(prev_style(73, l), 72);
        assert_eq!(prev_style(72, l), 71);
        assert_eq!(prev_style(71, l), 73);
        // A style outside the loop enters it at either end.
        assert_eq!(next_style(999, l), 71);
        assert_eq!(prev_style(999, l), 73);
        assert_eq!(next_style(21, &[]), 21);
        assert_eq!(prev_style(21, &[]), 21);
    }

    #[test]
    fn shift_moves_the_box_without_resizing_it() {
        let text = boxed("alpha\nbeta", 73, ";", 10).unwrap();
        let w = block_width(&text);
        let right = shift(&text, 4);
        assert_eq!(block_width(&right), w);
        assert!(right.split('\n').all(|l| l.starts_with("    ;")));
        // Shifting back recovers the original.
        assert_eq!(shift(&right, -4), text);
        // A left shift never eats past the leftmost line.
        assert_eq!(shift(&text, -10), text);
        assert_eq!(block_width(&shift(&right, -2)), w);
    }

    #[test]
    fn center_moves_the_box_without_resizing_it() {
        let text = boxed("alpha\nbeta", 73, ";", 10).unwrap();
        let w = block_width(&text);
        let centered = center(&text, 40);
        assert_eq!(block_width(&centered), w);
        assert_eq!(
            margin_of(&centered.split('\n').collect::<Vec<_>>()),
            (40 - w) / 2
        );
        // Centering an already-shifted box replaces the old margin.
        assert_eq!(center(&shift(&text, 7), 40), centered);
        // A box wider than the target is flush left.
        assert_eq!(
            margin_of(&center(&text, 2).split('\n').collect::<Vec<_>>()),
            0
        );
    }

    #[test]
    fn unknown_styles_and_missing_comment_tokens_are_errors() {
        assert!(boxed("x", 999, ";", 10).is_err());
        assert!(boxed("x", 78, ";", 10).is_err());
        assert!(boxed("x", 72, "", 10).is_err());
        // Language 1 has no generic instantiation in rebox2.
        assert!(boxed("x", 172, "", 10).is_err());
    }

    #[test]
    fn trailing_newlines_survive_boxing_and_shifting() {
        let text = boxed("alpha\n", 72, ";", 8).unwrap();
        assert!(text.ends_with('\n'));
        assert_eq!(text.split('\n').count(), 4);
        assert!(shift(&text, 2).ends_with('\n'));
    }
}
