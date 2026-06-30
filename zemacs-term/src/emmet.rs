//! Emmet / zen-coding HTML abbreviation expansion.
//!
//! Turns abbreviations like `ul>li.item$*3` or `h4*100` into HTML, returned as
//! an LSP-snippet body (with `${1}`/`$0` tabstops) so the editor's existing
//! snippet machinery can insert it and tab through the empty content slots.
//!
//! Supported syntax:
//!   - operators: child `>`, sibling `+`, climb-up `^`, multiply `*N`, group `(...)`
//!   - element parts: `tag`, `#id`, `.class` (repeatable), `[attr=val ...]`, `{text}`
//!   - numbering: `$`, `$$$` (zero-padded), with `@N` start and `@-` reverse modifiers
//!   - implicit tags (`.foo` -> `div.foo`, `ul>.item` -> `li`, ...)
//!   - void elements (`img`, `br`, ...) and per-tag default attributes
//!   - snippet aliases (`!`, `html:5`, `link:css`, `input:email`, `a:link`, ...)
//!
//! This module is pure (no editor deps) so it can be unit-tested in isolation.

/// A node in the parsed abbreviation tree.
#[derive(Clone, Debug, Default)]
struct Node {
    tag: String,
    id: Option<String>,
    classes: Vec<String>,
    /// `(name, Some(value))` for `name="value"`, `(name, None)` for a bare attr.
    attrs: Vec<(String, Option<String>)>,
    text: Option<String>,
    children: Vec<Node>,
    /// Raw markup that bypasses normal rendering (used by snippet aliases like `!`).
    raw: Option<String>,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Languages for which Tab should attempt emmet expansion.
pub fn is_html_like(lang: Option<&str>) -> bool {
    matches!(
        lang.unwrap_or("").to_ascii_lowercase().as_str(),
        "html"
            | "htm"
            | "xhtml"
            | "xml"
            | "svg"
            | "vue"
            | "svelte"
            | "astro"
            | "php"
            | "jsx"
            | "tsx"
            | "javascriptreact"
            | "typescriptreact"
            | "handlebars"
            | "hbs"
            | "twig"
            | "erb"
            | "ejs"
            | "markdown"
            | "md"
            | "blade"
    )
}

/// Scan backwards from the end of `before` (the text on the line up to the
/// cursor) and return `(char_start_offset, abbreviation)` if one is present.
///
/// Spaces are allowed inside `[...]`/`{...}`; outside them a space (or any
/// non-abbreviation character) ends the abbreviation.
pub fn extract_abbreviation(before: &str) -> Option<(usize, String)> {
    let chars: Vec<char> = before.chars().collect();
    let mut depth: usize = 0;
    let mut i = chars.len();
    while i > 0 {
        let c = chars[i - 1];
        if c == ']' || c == '}' {
            depth += 1;
        } else if c == '[' || c == '{' {
            if depth > 0 {
                depth -= 1;
            } else {
                break; // unbalanced opener — not part of this abbreviation
            }
        } else if depth == 0 {
            if c.is_whitespace() || !is_abbr_char(c) {
                break;
            }
        }
        i -= 1;
    }
    let abbr: String = chars[i..].iter().collect();
    let trimmed = abbr.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some((i, abbr))
}

/// Expand `abbr` into an LSP-snippet body, indented with `indent_unit` per
/// nesting level and `base_indent` prepended to every line after the first.
///
/// Returns `None` when the abbreviation does not look like emmet (e.g. a bare
/// unknown word), so callers can fall back to a normal Tab.
pub fn expand(abbr: &str, indent_unit: &str, base_indent: &str) -> Option<String> {
    let abbr = abbr.trim();
    if abbr.is_empty() {
        return None;
    }
    if !looks_like_emmet(abbr) {
        return None;
    }
    // Whole-abbreviation snippet aliases (`!`, `link:css`, `input:email`, ...).
    if let Some(body) = alias_markup(abbr) {
        return Some(if base_indent.is_empty() {
            body
        } else {
            body.replace('\n', &format!("\n{base_indent}"))
        });
    }
    let nodes = Parser::new(abbr).parse()?;
    if nodes.is_empty() {
        return None;
    }

    // Count empty content slots so we can number tabstops and put `$0` last.
    let mut total_slots = 0usize;
    for n in &nodes {
        count_slots(n, "div", &mut total_slots);
    }
    let use_tabstops = total_slots > 0 && total_slots <= 40;

    let mut r = Renderer {
        indent_unit: indent_unit.to_string(),
        slot: 0,
        total_slots,
        use_tabstops,
    };
    let mut out = String::new();
    for (idx, n) in nodes.iter().enumerate() {
        if idx > 0 {
            out.push('\n');
        }
        r.render(n, "div", 0, &mut out);
    }

    if base_indent.is_empty() {
        Some(out)
    } else {
        Some(out.replace('\n', &format!("\n{base_indent}")))
    }
}

// ---------------------------------------------------------------------------
// Lexical helpers
// ---------------------------------------------------------------------------

fn is_abbr_char(c: char) -> bool {
    c.is_alphanumeric() || "+>^*().#$@!:_-".contains(c)
}

/// Heuristic: only expand bare words that are known tags/aliases; anything with
/// an emmet operator or modifier is always treated as an abbreviation.
fn looks_like_emmet(abbr: &str) -> bool {
    if abbr.chars().any(|c| ">+^*().#[]{}".contains(c)) {
        return true;
    }
    // Bare token: accept only recognised tags / aliases.
    let tag = abbr.split([':']).next().unwrap_or(abbr);
    is_known_tag(tag) || alias_markup(abbr).is_some() || abbr == "!"
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

struct Parser {
    chars: Vec<char>,
    pos: usize,
}

impl Parser {
    fn new(s: &str) -> Self {
        Parser {
            chars: s.chars().collect(),
            pos: 0,
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.chars.get(self.pos).copied();
        if c.is_some() {
            self.pos += 1;
        }
        c
    }

    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(c) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    /// Top-level: keep consuming sequences, tolerating stray climbs above root.
    fn parse(&mut self) -> Option<Vec<Node>> {
        let mut all = Vec::new();
        while self.peek().is_some() {
            let before = self.pos;
            let (nodes, _climb) = self.parse_sequence()?;
            all.extend(nodes);
            if self.pos == before {
                // no progress — bail to avoid an infinite loop on bad input
                return None;
            }
        }
        Some(all)
    }

    /// Parse a run of items joined by `+`/`>`/`^`. Returns the sibling list at
    /// this level plus the number of levels to climb (from a trailing `^`).
    fn parse_sequence(&mut self) -> Option<(Vec<Node>, usize)> {
        let mut result: Vec<Node> = Vec::new();
        loop {
            let mut items = self.parse_primary()?;
            if items.is_empty() {
                return None;
            }
            match self.peek() {
                Some('>') => {
                    self.bump();
                    let (children, climb) = self.parse_sequence()?;
                    attach_children(&mut items, &children);
                    result.extend(items);
                    if climb == 0 {
                        return Some((result, 0));
                    } else if climb == 1 {
                        // climb lands at this level — keep parsing siblings here
                        continue;
                    } else {
                        return Some((result, climb - 1));
                    }
                }
                Some('+') => {
                    self.bump();
                    result.extend(items);
                    continue;
                }
                Some('^') => {
                    let mut k = 0;
                    while self.eat('^') {
                        k += 1;
                    }
                    result.extend(items);
                    return Some((result, k));
                }
                _ => {
                    // end of input or `)`
                    result.extend(items);
                    return Some((result, 0));
                }
            }
        }
    }

    /// Parse a single element or a `( ... )` group, applying a trailing `*N`.
    /// Returns the (possibly repeated) list of nodes produced.
    fn parse_primary(&mut self) -> Option<Vec<Node>> {
        let group = if self.eat('(') {
            let (nodes, _climb) = self.parse_sequence()?;
            if !self.eat(')') {
                return None;
            }
            Some(nodes)
        } else {
            None
        };

        let base: Vec<Node> = match group {
            Some(nodes) => nodes,
            None => vec![self.parse_element()?],
        };

        // Optional multiplier.
        let count = if self.eat('*') {
            let mut digits = String::new();
            while let Some(c) = self.peek() {
                if c.is_ascii_digit() {
                    digits.push(c);
                    self.bump();
                } else {
                    break;
                }
            }
            digits.parse::<usize>().unwrap_or(1).max(1)
        } else {
            1
        };

        if count == 1 {
            return Some(base);
        }

        let mut out = Vec::with_capacity(base.len() * count);
        for i in 0..count {
            for n in &base {
                let mut copy = n.clone();
                number_node(&mut copy, i, count);
                out.push(copy);
            }
        }
        Some(out)
    }

    /// Parse `tag`, `#id`, `.class`, `[attrs]`, `{text}` in any order.
    fn parse_element(&mut self) -> Option<Node> {
        let mut node = Node::default();

        // Tag name (may be empty -> implicit, resolved at render time).
        let mut tag = String::new();
        while let Some(c) = self.peek() {
            if c.is_alphanumeric() || "$@:!-".contains(c) {
                tag.push(c);
                self.bump();
            } else {
                break;
            }
        }
        node.tag = tag;

        loop {
            match self.peek() {
                Some('#') => {
                    self.bump();
                    node.id = Some(self.read_name());
                }
                Some('.') => {
                    self.bump();
                    let cls = self.read_name();
                    if !cls.is_empty() {
                        node.classes.push(cls);
                    }
                }
                Some('[') => {
                    self.bump();
                    self.read_attrs(&mut node)?;
                }
                Some('{') => {
                    self.bump();
                    node.text = Some(self.read_braced()?);
                }
                _ => break,
            }
        }

        if node.tag.is_empty()
            && node.id.is_none()
            && node.classes.is_empty()
            && node.attrs.is_empty()
            && node.text.is_none()
        {
            return None;
        }
        Some(node)
    }

    /// Read an id/class token (letters, digits, `-`, `_`, `$`).
    fn read_name(&mut self) -> String {
        let mut s = String::new();
        while let Some(c) = self.peek() {
            if c.is_alphanumeric() || "-_$@".contains(c) {
                s.push(c);
                self.bump();
            } else {
                break;
            }
        }
        s
    }

    /// Read the body of `[ ... ]` into `node.attrs`.
    fn read_attrs(&mut self, node: &mut Node) -> Option<()> {
        loop {
            // skip separators
            while matches!(self.peek(), Some(c) if c.is_whitespace() || c == ',') {
                self.bump();
            }
            match self.peek() {
                Some(']') => {
                    self.bump();
                    return Some(());
                }
                None => return None,
                _ => {}
            }
            // attribute name
            let mut name = String::new();
            while let Some(c) = self.peek() {
                if c.is_whitespace() || c == '=' || c == ']' {
                    break;
                }
                name.push(c);
                self.bump();
            }
            if name.is_empty() {
                return None;
            }
            if self.eat('=') {
                let value = match self.peek() {
                    Some('"') => {
                        self.bump();
                        self.read_until('"')
                    }
                    Some('\'') => {
                        self.bump();
                        self.read_until('\'')
                    }
                    _ => {
                        let mut v = String::new();
                        while let Some(c) = self.peek() {
                            if c.is_whitespace() || c == ']' {
                                break;
                            }
                            v.push(c);
                            self.bump();
                        }
                        v
                    }
                };
                node.attrs.push((name, Some(value)));
            } else {
                node.attrs.push((name, None));
            }
        }
    }

    fn read_until(&mut self, end: char) -> String {
        let mut s = String::new();
        while let Some(c) = self.bump() {
            if c == end {
                break;
            }
            s.push(c);
        }
        s
    }

    /// Read the body of `{ ... }`, honouring balanced nested braces.
    fn read_braced(&mut self) -> Option<String> {
        let mut s = String::new();
        let mut depth = 1usize;
        while let Some(c) = self.bump() {
            match c {
                '{' => {
                    depth += 1;
                    s.push(c);
                }
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(s);
                    }
                    s.push(c);
                }
                _ => s.push(c),
            }
        }
        None
    }
}

/// Attach a deep clone of `children` to every node in `items`.
fn attach_children(items: &mut [Node], children: &[Node]) {
    for node in items.iter_mut() {
        for child in children {
            node.children.push(child.clone());
        }
    }
}

// ---------------------------------------------------------------------------
// Numbering ($)
// ---------------------------------------------------------------------------

/// Substitute `$` runs throughout a freshly-cloned node for multiply index `i`
/// (0-based) out of `total`. Fields already resolved by an inner multiply have
/// no `$` left, so they are naturally untouched.
fn number_node(node: &mut Node, i: usize, total: usize) {
    node.tag = apply_numbering(&node.tag, i, total);
    if let Some(id) = &node.id {
        node.id = Some(apply_numbering(id, i, total));
    }
    for c in &mut node.classes {
        *c = apply_numbering(c, i, total);
    }
    for (name, val) in &mut node.attrs {
        *name = apply_numbering(name, i, total);
        if let Some(v) = val {
            *v = apply_numbering(v, i, total);
        }
    }
    if let Some(t) = &node.text {
        node.text = Some(apply_numbering(t, i, total));
    }
    for child in &mut node.children {
        number_node(child, i, total);
    }
}

/// Replace each `$`-run in `s`. A run of k `$` becomes the number zero-padded to
/// width k. An optional `@` modifier follows: `@N` sets the start value, `@-`
/// reverses, `@-N` does both. `\$` is a literal dollar.
fn apply_numbering(s: &str, i: usize, total: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut p = 0;
    while p < chars.len() {
        let c = chars[p];
        if c == '\\' && p + 1 < chars.len() && chars[p + 1] == '$' {
            out.push('$');
            p += 2;
            continue;
        }
        if c == '$' {
            let mut width = 0;
            while p < chars.len() && chars[p] == '$' {
                width += 1;
                p += 1;
            }
            // optional @ modifier
            let mut start: i64 = 1;
            let mut reverse = false;
            if p < chars.len() && chars[p] == '@' {
                p += 1;
                if p < chars.len() && chars[p] == '-' {
                    reverse = true;
                    p += 1;
                }
                let mut num = String::new();
                while p < chars.len() && chars[p].is_ascii_digit() {
                    num.push(chars[p]);
                    p += 1;
                }
                if let Ok(n) = num.parse::<i64>() {
                    start = n;
                }
            }
            let value = if reverse {
                start + (total as i64 - 1 - i as i64)
            } else {
                start + i as i64
            };
            out.push_str(&format!("{:0>width$}", value, width = width));
            continue;
        }
        out.push(c);
        p += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// Renderer
// ---------------------------------------------------------------------------

struct Renderer {
    indent_unit: String,
    slot: usize,
    total_slots: usize,
    use_tabstops: bool,
}

impl Renderer {
    fn indent(&self, depth: usize) -> String {
        self.indent_unit.repeat(depth)
    }

    /// Emit the next tabstop placeholder for an empty content slot.
    fn next_tabstop(&mut self) -> String {
        self.slot += 1;
        if self.use_tabstops {
            if self.slot == self.total_slots {
                "$0".to_string()
            } else {
                format!("${{{}}}", self.slot)
            }
        } else if self.slot == 1 {
            // only the first empty slot gets the final cursor
            "$0".to_string()
        } else {
            String::new()
        }
    }

    fn render(&mut self, node: &Node, parent_tag: &str, depth: usize, out: &mut String) {
        let pad = self.indent(depth);
        out.push_str(&pad);

        if let Some(raw) = &node.raw {
            // Raw alias markup: re-indent its internal newlines.
            let replaced = raw.replace('\n', &format!("\n{pad}"));
            out.push_str(&replaced);
            return;
        }

        let tag = resolve_tag(&node.tag, parent_tag);
        let open = self.open_tag(node, &tag);

        if is_void(&tag) {
            out.push_str(&open);
            return;
        }

        out.push_str(&open);

        let has_element_children = !node.children.is_empty();
        if has_element_children {
            // text (if any) then children, each on their own line
            if let Some(t) = &node.text {
                out.push('\n');
                out.push_str(&self.indent(depth + 1));
                out.push_str(&snip_escape(t));
            }
            for child in &node.children {
                out.push('\n');
                self.render(child, &tag, depth + 1, out);
            }
            out.push('\n');
            out.push_str(&pad);
            out.push_str(&format!("</{tag}>"));
        } else {
            // inline: <tag>text-or-cursor</tag>
            match &node.text {
                Some(t) => out.push_str(&snip_escape(t)),
                None => {
                    let ts = self.next_tabstop();
                    out.push_str(&ts);
                }
            }
            out.push_str(&format!("</{tag}>"));
        }
    }

    fn open_tag(&self, node: &Node, tag: &str) -> String {
        let mut s = String::new();
        s.push('<');
        s.push_str(tag);

        // id
        if let Some(id) = &node.id {
            s.push_str(&format!(" id=\"{}\"", snip_escape(id)));
        }
        // classes
        if !node.classes.is_empty() {
            let cls = node
                .classes
                .iter()
                .map(|c| snip_escape(c))
                .collect::<Vec<_>>()
                .join(" ");
            s.push_str(&format!(" class=\"{cls}\""));
        }
        // explicit attributes
        for (name, val) in &node.attrs {
            match val {
                Some(v) => s.push_str(&format!(" {}=\"{}\"", name, snip_escape(v))),
                None => s.push_str(&format!(" {name}")),
            }
        }
        // implicit default attributes for known tags (only if not overridden)
        for (name, val) in default_attrs(tag) {
            let already = node.attrs.iter().any(|(n, _)| n == name)
                || (*name == "id" && node.id.is_some())
                || (*name == "class" && !node.classes.is_empty());
            if !already {
                s.push_str(&format!(" {name}=\"{}\"", snip_escape(val)));
            }
        }

        s.push('>');
        s
    }
}

/// Escape characters special to the LSP-snippet format in literal text.
fn snip_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' | '$' | '}' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}

fn count_slots(node: &Node, parent_tag: &str, total: &mut usize) {
    if node.raw.is_some() {
        return;
    }
    let tag = resolve_tag(&node.tag, parent_tag);
    if is_void(&tag) {
        return;
    }
    if node.children.is_empty() && node.text.is_none() {
        *total += 1;
    }
    for child in &node.children {
        count_slots(child, &tag, total);
    }
}

// ---------------------------------------------------------------------------
// Tag knowledge
// ---------------------------------------------------------------------------

/// Resolve an (possibly empty) tag name to a concrete one, applying aliases and
/// context-sensitive implicit tags.
fn resolve_tag(tag: &str, parent_tag: &str) -> String {
    if tag.is_empty() {
        return implicit_tag(parent_tag).to_string();
    }
    // `:`-aliases that map to a real tag are handled in default_attrs/alias path;
    // here we just strip a `:variant` to find the base element when it is a tag.
    if let Some(base) = tag.split(':').next() {
        if !base.is_empty() && base != tag && is_known_tag(base) {
            return base.to_string();
        }
    }
    tag.to_string()
}

fn implicit_tag(parent: &str) -> &'static str {
    match parent {
        "ul" | "ol" => "li",
        "table" | "tbody" | "thead" | "tfoot" => "tr",
        "tr" => "td",
        "select" | "optgroup" => "option",
        "audio" | "video" => "source",
        "map" => "area",
        "dl" => "dt",
        _ => "div",
    }
}

fn is_void(tag: &str) -> bool {
    matches!(
        tag,
        "area"
            | "base"
            | "br"
            | "col"
            | "embed"
            | "hr"
            | "img"
            | "input"
            | "link"
            | "meta"
            | "param"
            | "source"
            | "track"
            | "wbr"
    )
}

/// Per-tag default attributes that emmet fills in.
fn default_attrs(tag: &str) -> &'static [(&'static str, &'static str)] {
    match tag {
        "a" => &[("href", "")],
        "img" => &[("src", ""), ("alt", "")],
        "form" => &[("action", "")],
        "input" => &[("type", "text"), ("name", ""), ("value", "")],
        "script" => &[("src", "")],
        "link" => &[("rel", "stylesheet"), ("href", "")],
        "label" => &[("for", "")],
        "base" => &[("href", "")],
        "area" => &[("shape", ""), ("coords", ""), ("href", ""), ("alt", "")],
        "select" => &[("name", ""), ("id", "")],
        "option" => &[("value", "")],
        "textarea" => &[("name", ""), ("id", ""), ("cols", "30"), ("rows", "10")],
        "iframe" => &[("src", ""), ("frameborder", "0")],
        "embed" => &[("src", ""), ("type", "")],
        "object" => &[("data", ""), ("type", "")],
        "param" => &[("name", ""), ("value", "")],
        "meta" => &[("content", "")],
        "video" | "audio" => &[("src", "")],
        _ => &[],
    }
}

fn is_known_tag(tag: &str) -> bool {
    const TAGS: &[&str] = &[
        "a",
        "abbr",
        "address",
        "area",
        "article",
        "aside",
        "audio",
        "b",
        "base",
        "bdi",
        "bdo",
        "blockquote",
        "body",
        "br",
        "button",
        "canvas",
        "caption",
        "cite",
        "code",
        "col",
        "colgroup",
        "data",
        "datalist",
        "dd",
        "del",
        "details",
        "dfn",
        "dialog",
        "div",
        "dl",
        "dt",
        "em",
        "embed",
        "fieldset",
        "figcaption",
        "figure",
        "footer",
        "form",
        "h1",
        "h2",
        "h3",
        "h4",
        "h5",
        "h6",
        "head",
        "header",
        "hgroup",
        "hr",
        "html",
        "i",
        "iframe",
        "img",
        "input",
        "ins",
        "kbd",
        "label",
        "legend",
        "li",
        "link",
        "main",
        "map",
        "mark",
        "menu",
        "meta",
        "meter",
        "nav",
        "noscript",
        "object",
        "ol",
        "optgroup",
        "option",
        "output",
        "p",
        "param",
        "picture",
        "pre",
        "progress",
        "q",
        "rp",
        "rt",
        "ruby",
        "s",
        "samp",
        "script",
        "section",
        "select",
        "slot",
        "small",
        "source",
        "span",
        "strong",
        "style",
        "sub",
        "summary",
        "sup",
        "table",
        "tbody",
        "td",
        "template",
        "textarea",
        "tfoot",
        "th",
        "thead",
        "time",
        "title",
        "tr",
        "track",
        "u",
        "ul",
        "var",
        "video",
        "wbr",
    ];
    TAGS.contains(&tag)
}

// ---------------------------------------------------------------------------
// Snippet aliases (handled before normal parsing for the whole abbreviation)
// ---------------------------------------------------------------------------

/// Full-markup aliases that replace the entire abbreviation. Returned strings
/// are snippet bodies (may contain `$0`, must already be snippet-escaped).
fn alias_markup(abbr: &str) -> Option<String> {
    let body = match abbr {
        "!" | "html:5" | "!!!" => HTML5_DOC,
        "link:css" => r#"<link rel="stylesheet" href="${1:style.css}">$0"#,
        "link:favicon" => {
            r#"<link rel="shortcut icon" type="image/x-icon" href="${1:favicon.ico}">$0"#
        }
        "link:rss" => {
            r#"<link rel="alternate" type="application/rss+xml" title="RSS" href="${1:rss.xml}">$0"#
        }
        "script:src" => r#"<script src="${1}">$0</script>"#,
        "meta:utf" => r#"<meta charset="${1:UTF-8}">$0"#,
        "meta:vp" => {
            r#"<meta name="viewport" content="width=${1:device-width}, initial-scale=${2:1.0}">$0"#
        }
        "input:text" | "inp" => r#"<input type="text" name="${1}" id="${2}">$0"#,
        "input:email" => r#"<input type="email" name="${1}" id="${2}">$0"#,
        "input:password" => r#"<input type="password" name="${1}" id="${2}">$0"#,
        "input:checkbox" | "input:c" => r#"<input type="checkbox" name="${1}" id="${2}">$0"#,
        "input:radio" | "input:r" => r#"<input type="radio" name="${1}" id="${2}">$0"#,
        "input:hidden" | "input:h" => r#"<input type="hidden" name="${1}" value="${2}">$0"#,
        "input:submit" | "input:s" => r#"<input type="submit" value="${1}">$0"#,
        "input:button" | "input:b" => r#"<input type="button" value="${1}">$0"#,
        "input:file" | "input:f" => r#"<input type="file" name="${1}" id="${2}">$0"#,
        "a:link" => r#"<a href="http://${1}">$0</a>"#,
        "a:mail" => r#"<a href="mailto:${1}">$0</a>"#,
        "btn:s" => r#"<button type="submit">$0</button>"#,
        "btn:r" => r#"<button type="reset">$0</button>"#,
        _ => return None,
    };
    Some(body.to_string())
}

const HTML5_DOC: &str = r#"<!DOCTYPE html>
<html lang="${1:en}">
<head>
	<meta charset="UTF-8">
	<meta name="viewport" content="width=device-width, initial-scale=1.0">
	<title>${2:Document}</title>
</head>
<body>
	$0
</body>
</html>"#;

// ===========================================================================
// CSS emmet (fuzzy property abbreviations + value/unit inference)
// ===========================================================================

/// Languages for which Tab should attempt CSS emmet expansion.
pub fn is_css_like(lang: Option<&str>) -> bool {
    matches!(
        lang.unwrap_or("").to_ascii_lowercase().as_str(),
        "css" | "scss" | "sass" | "less" | "stylus" | "styl" | "postcss" | "sss"
    )
}

/// Scan back over the characters that can make up a CSS abbreviation
/// (`m10`, `p10-20`, `c#fff`, `pos:a`, `fz1.5e`, `m0!`).
pub fn extract_css_abbreviation(before: &str) -> Option<(usize, String)> {
    let chars: Vec<char> = before.chars().collect();
    let mut i = chars.len();
    while i > 0 {
        let c = chars[i - 1];
        if c.is_alphanumeric() || "#.-:%!".contains(c) {
            i -= 1;
        } else {
            break;
        }
    }
    let abbr: String = chars[i..].iter().collect();
    if abbr.trim().is_empty() {
        return None;
    }
    Some((i, abbr))
}

/// Expand a CSS abbreviation into a single-line snippet declaration, e.g.
/// `m10` -> `margin: 10px;$0`, `m` -> `margin: $0;`, `df` -> `display: flex;$0`.
pub fn expand_css(abbr: &str) -> Option<String> {
    let abbr = abbr.trim();
    if abbr.is_empty() {
        return None;
    }
    let important = abbr.ends_with('!');
    let core = abbr.trim_end_matches('!');
    let (prop, value) = css_declaration(core)?;
    let imp = if important { " !important" } else { "" };
    Some(match value {
        Some(v) if !v.is_empty() => format!("{prop}: {v}{imp};$0"),
        _ => format!("{prop}: $0{imp};"),
    })
}

/// Resolve a CSS abbreviation to `(property, value)`. `value == None` means an
/// empty value slot (the caller drops a tabstop there).
fn css_declaration(core: &str) -> Option<(String, Option<String>)> {
    // 1. Whole-abbreviation keyword snippets (`df`, `pos:a`, `d:f`, `fw:b`, ...).
    if let Some((prop, val)) = css_keyword(core) {
        return Some((prop.to_string(), Some(val.to_string())));
    }

    // 2. Split leading property letters from the value remainder.
    let split = core
        .char_indices()
        .find(|(_, c)| !c.is_ascii_alphabetic())
        .map(|(i, _)| i)
        .unwrap_or(core.len());
    let (prop_abbr, rest) = core.split_at(split);
    if prop_abbr.is_empty() {
        return None;
    }
    let property = css_property(prop_abbr)?;

    if rest.is_empty() {
        return Some((property.to_string(), None));
    }
    if let Some(kw) = rest.strip_prefix(':') {
        // Explicit keyword value not covered by a combo snippet — pass through.
        if kw.is_empty() {
            return Some((property.to_string(), None));
        }
        return Some((property.to_string(), Some(kw.to_string())));
    }
    Some((property.to_string(), Some(format_css_value(property, rest))))
}

/// Format a (possibly multi-valued) numeric/colour CSS value.
fn format_css_value(property: &str, rest: &str) -> String {
    if let Some(hex) = rest.strip_prefix('#') {
        return expand_color(hex);
    }
    // Split into values on `-`, honouring leading `-` as a negative sign.
    let parts: Vec<&str> = rest.split('-').collect();
    let mut values: Vec<String> = Vec::new();
    let mut k = 0;
    while k < parts.len() {
        if parts[k].is_empty() {
            // a `-` separator that signals the following token is negative
            k += 1;
            if k < parts.len() {
                values.push(format_number(property, &format!("-{}", parts[k])));
            }
        } else {
            values.push(format_number(property, parts[k]));
        }
        k += 1;
    }
    values.join(" ")
}

/// Format a single numeric token with unit inference.
fn format_number(property: &str, tok: &str) -> String {
    // keyword-ish value (e.g. `a` -> auto)
    let first_significant = tok.trim_start_matches('-');
    if !first_significant
        .chars()
        .next()
        .map(|c| c.is_ascii_digit() || c == '.')
        .unwrap_or(false)
    {
        return match tok {
            "a" => "auto".to_string(),
            other => other.to_string(),
        };
    }

    // separate the numeric prefix from a trailing unit
    let neg = tok.starts_with('-');
    let body = tok.trim_start_matches('-');
    let split = body
        .char_indices()
        .find(|(_, c)| !(c.is_ascii_digit() || *c == '.'))
        .map(|(i, _)| i)
        .unwrap_or(body.len());
    let (num_raw, unit_raw) = body.split_at(split);

    let mut num = num_raw.to_string();
    if num.starts_with('.') {
        num.insert(0, '0');
    }
    if num.is_empty() {
        return tok.to_string();
    }
    let mut value = if neg { format!("-{num}") } else { num.clone() };

    // bare zero never gets a unit
    let is_zero = num.chars().all(|c| c == '0' || c == '.');
    if is_zero && unit_raw.is_empty() {
        return "0".to_string();
    }

    if is_unitless_property(property) {
        return value;
    }

    let unit = if unit_raw.is_empty() {
        if num.contains('.') {
            "em".to_string()
        } else {
            "px".to_string()
        }
    } else {
        unit_alias(unit_raw)
    };
    value.push_str(&unit);
    value
}

fn unit_alias(u: &str) -> String {
    match u {
        "p" => "%".to_string(),
        "e" => "em".to_string(),
        "r" => "rem".to_string(),
        "x" => "ex".to_string(),
        other => other.to_string(),
    }
}

/// Expand an emmet colour shorthand: `#1`->#111111, `#e0`->#e0e0e0, `#abc`->#abc.
fn expand_color(hex: &str) -> String {
    let h = hex.trim();
    let expanded = match h.len() {
        0 => "#000".to_string(),
        1 => format!("#{}", h.repeat(6)),
        2 => format!("#{}", h.repeat(3)),
        _ => format!("#{h}"),
    };
    expanded
}

fn is_unitless_property(p: &str) -> bool {
    matches!(
        p,
        "font-weight"
            | "line-height"
            | "opacity"
            | "z-index"
            | "zoom"
            | "flex"
            | "flex-grow"
            | "flex-shrink"
            | "order"
            | "orphans"
            | "widows"
            | "column-count"
    )
}

/// Property abbreviation -> property name (value-taking).
fn css_property(a: &str) -> Option<&'static str> {
    Some(match a {
        "p" => "padding",
        "pt" => "padding-top",
        "pr" => "padding-right",
        "pb" => "padding-bottom",
        "pl" => "padding-left",
        "m" => "margin",
        "mt" => "margin-top",
        "mr" => "margin-right",
        "mb" => "margin-bottom",
        "ml" => "margin-left",
        "w" => "width",
        "h" => "height",
        "maw" => "max-width",
        "mah" => "max-height",
        "miw" => "min-width",
        "mih" => "min-height",
        "o" => "outline",
        "bd" => "border",
        "bdt" => "border-top",
        "bdr" => "border-right",
        "bdb" => "border-bottom",
        "bdl" => "border-left",
        "bdw" => "border-width",
        "bdc" => "border-color",
        "bds" => "border-style",
        "bdrs" => "border-radius",
        "bg" => "background",
        "bgc" => "background-color",
        "bgi" => "background-image",
        "bgp" => "background-position",
        "bgr" => "background-repeat",
        "bgsz" => "background-size",
        "c" => "color",
        "op" | "opac" => "opacity",
        "fz" => "font-size",
        "fw" => "font-weight",
        "ff" => "font-family",
        "fs" => "font-style",
        "lh" => "line-height",
        "ls" => "letter-spacing",
        "ws" => "word-spacing",
        "ta" => "text-align",
        "td" => "text-decoration",
        "tt" => "text-transform",
        "ti" => "text-indent",
        "tsh" => "text-shadow",
        "d" => "display",
        "v" => "visibility",
        "ov" => "overflow",
        "ovx" => "overflow-x",
        "ovy" => "overflow-y",
        "pos" => "position",
        "t" => "top",
        "r" => "right",
        "b" => "bottom",
        "l" => "left",
        "z" => "z-index",
        "fl" => "float",
        "cl" => "clear",
        "cur" => "cursor",
        "bxsh" => "box-shadow",
        "bxz" => "box-sizing",
        "fx" => "flex",
        "fxg" => "flex-grow",
        "fxsh" => "flex-shrink",
        "fxb" => "flex-basis",
        "fxd" => "flex-direction",
        "fxw" => "flex-wrap",
        "jc" => "justify-content",
        "ai" => "align-items",
        "as" => "align-self",
        "ac" => "align-content",
        "ord" => "order",
        "gtc" => "grid-template-columns",
        "gtr" => "grid-template-rows",
        "gg" => "grid-gap",
        "gap" => "gap",
        "trf" => "transform",
        "trs" => "transition",
        "anim" => "animation",
        "cnt" => "content",
        "whs" => "white-space",
        "va" => "vertical-align",
        "zm" => "zoom",
        _ => return None,
    })
}

/// Whole-abbreviation keyword snippets -> `(property, value)`.
fn css_keyword(a: &str) -> Option<(&'static str, &'static str)> {
    Some(match a {
        // display
        "d:n" | "dn" => ("display", "none"),
        "d:b" | "db" => ("display", "block"),
        "d:i" | "di" => ("display", "inline"),
        "d:ib" | "dib" => ("display", "inline-block"),
        "d:f" | "df" => ("display", "flex"),
        "d:if" | "dif" => ("display", "inline-flex"),
        "d:g" | "dg" => ("display", "grid"),
        "d:ig" | "dig" => ("display", "inline-grid"),
        "d:t" => ("display", "table"),
        // position
        "pos:a" => ("position", "absolute"),
        "pos:r" => ("position", "relative"),
        "pos:f" => ("position", "fixed"),
        "pos:s" => ("position", "static"),
        "pos:st" => ("position", "sticky"),
        // float / clear
        "fl:l" => ("float", "left"),
        "fl:r" => ("float", "right"),
        "fl:n" => ("float", "none"),
        "cl:l" => ("clear", "left"),
        "cl:r" => ("clear", "right"),
        "cl:b" => ("clear", "both"),
        "cl:n" => ("clear", "none"),
        // text-align
        "ta:l" => ("text-align", "left"),
        "ta:c" => ("text-align", "center"),
        "ta:r" => ("text-align", "right"),
        "ta:j" => ("text-align", "justify"),
        // font
        "fw:b" => ("font-weight", "bold"),
        "fw:n" => ("font-weight", "normal"),
        "fs:i" => ("font-style", "italic"),
        "fs:n" => ("font-style", "normal"),
        // text-decoration / transform
        "td:n" => ("text-decoration", "none"),
        "td:u" => ("text-decoration", "underline"),
        "td:l" => ("text-decoration", "line-through"),
        "tt:u" => ("text-transform", "uppercase"),
        "tt:l" => ("text-transform", "lowercase"),
        "tt:c" => ("text-transform", "capitalize"),
        "tt:n" => ("text-transform", "none"),
        // overflow
        "ov:h" => ("overflow", "hidden"),
        "ov:s" => ("overflow", "scroll"),
        "ov:a" => ("overflow", "auto"),
        "ov:v" => ("overflow", "visible"),
        // visibility
        "v:h" => ("visibility", "hidden"),
        "v:v" => ("visibility", "visible"),
        // cursor
        "cur:p" => ("cursor", "pointer"),
        "cur:d" => ("cursor", "default"),
        // box-sizing
        "bxz:bb" => ("box-sizing", "border-box"),
        "bxz:cb" => ("box-sizing", "content-box"),
        // white-space
        "whs:n" => ("white-space", "nowrap"),
        "whs:p" => ("white-space", "pre"),
        // border shorthand
        "bd:n" | "bd0" => ("border", "none"),
        // flex-direction
        "fxd:c" => ("flex-direction", "column"),
        "fxd:r" => ("flex-direction", "row"),
        // justify-content
        "jc:c" => ("justify-content", "center"),
        "jc:sb" => ("justify-content", "space-between"),
        "jc:sa" => ("justify-content", "space-around"),
        "jc:fs" => ("justify-content", "flex-start"),
        "jc:fe" => ("justify-content", "flex-end"),
        // align-items
        "ai:c" => ("align-items", "center"),
        "ai:fs" => ("align-items", "flex-start"),
        "ai:fe" => ("align-items", "flex-end"),
        "ai:s" => ("align-items", "stretch"),
        "ai:b" => ("align-items", "baseline"),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ex(s: &str) -> String {
        expand(s, "\t", "").unwrap()
    }

    fn css(s: &str) -> String {
        expand_css(s).unwrap()
    }

    #[test]
    fn simple() {
        assert_eq!(ex("div"), "<div>$0</div>");
    }

    #[test]
    fn class_and_id() {
        assert_eq!(ex("p.foo#bar"), "<p id=\"bar\" class=\"foo\">$0</p>");
    }

    #[test]
    fn implicit_div() {
        assert_eq!(ex(".box"), "<div class=\"box\">$0</div>");
    }

    #[test]
    fn child() {
        assert_eq!(ex("ul>li"), "<ul>\n\t<li>$0</li>\n</ul>");
    }

    #[test]
    fn implicit_li() {
        assert_eq!(ex("ul>.item"), "<ul>\n\t<li class=\"item\">$0</li>\n</ul>");
    }

    #[test]
    fn sibling() {
        assert_eq!(ex("div+span"), "<div>${1}</div>\n<span>$0</span>");
    }

    #[test]
    fn multiply_count() {
        let out = ex("li*3");
        assert_eq!(out, "<li>${1}</li>\n<li>${2}</li>\n<li>$0</li>");
    }

    #[test]
    fn numbering() {
        let out = ex("li.item$*3");
        assert!(out.contains("class=\"item1\""));
        assert!(out.contains("class=\"item2\""));
        assert!(out.contains("class=\"item3\""));
    }

    #[test]
    fn numbering_padded_and_reverse() {
        let out = ex("li.i$$@-*3");
        // reverse, width 2: 03, 02, 01
        assert!(out.contains("i03"));
        assert!(out.contains("i01"));
    }

    #[test]
    fn climb_up() {
        // a>b^c : c is sibling of a
        let out = ex("div>p^span");
        assert_eq!(out, "<div>\n\t<p>${1}</p>\n</div>\n<span>$0</span>");
    }

    #[test]
    fn group() {
        let out = ex("(div>p)*2");
        assert_eq!(
            out,
            "<div>\n\t<p>${1}</p>\n</div>\n<div>\n\t<p>$0</p>\n</div>"
        );
    }

    #[test]
    fn text_and_attrs() {
        assert_eq!(ex("a[href=#]{click}"), "<a href=\"#\">click</a>");
    }

    #[test]
    fn void_img() {
        assert_eq!(ex("img"), "<img src=\"\" alt=\"\">");
    }

    #[test]
    fn large_multiply_has_no_excess_tabstops() {
        let out = ex("h4*100");
        assert_eq!(out.matches("<h4>").count(), 100);
        assert_eq!(out.matches("</h4>").count(), 100);
        // beyond the 40-slot cap only one $0 cursor is emitted
        assert_eq!(out.matches("$0").count(), 1);
        assert!(!out.contains("${1}"));
    }

    #[test]
    fn doc_alias_via_expand_path() {
        // `!` is handled by alias_markup, not expand(); ensure it is recognised.
        assert!(alias_markup("!").is_some());
    }

    #[test]
    fn extract_basic() {
        let (start, abbr) = extract_abbreviation("  ul>li*3").unwrap();
        assert_eq!(start, 2);
        assert_eq!(abbr, "ul>li*3");
    }

    #[test]
    fn extract_with_text_spaces() {
        let (_s, abbr) = extract_abbreviation("p{hello world}").unwrap();
        assert_eq!(abbr, "p{hello world}");
    }

    // ---- CSS ----

    #[test]
    fn css_margin_px() {
        assert_eq!(css("m10"), "margin: 10px;$0");
    }

    #[test]
    fn css_empty_value_cursor() {
        assert_eq!(css("m"), "margin: $0;");
    }

    #[test]
    fn css_multi_value() {
        assert_eq!(css("p10-20"), "padding: 10px 20px;$0");
    }

    #[test]
    fn css_negative() {
        assert_eq!(css("m-10"), "margin: -10px;$0");
        assert_eq!(css("m10--20"), "margin: 10px -20px;$0");
    }

    #[test]
    fn css_unit_alias_percent_em() {
        assert_eq!(css("w50p"), "width: 50%;$0");
        assert_eq!(css("fz1.5e"), "font-size: 1.5em;$0");
    }

    #[test]
    fn css_zero_no_unit() {
        assert_eq!(css("m0"), "margin: 0;$0");
    }

    #[test]
    fn css_float_default_em() {
        assert_eq!(css("p1.5"), "padding: 1.5em;$0");
    }

    #[test]
    fn css_unitless() {
        assert_eq!(css("fw400"), "font-weight: 400;$0");
        assert_eq!(css("lh1.5"), "line-height: 1.5;$0");
        assert_eq!(css("z10"), "z-index: 10;$0");
        assert_eq!(css("op.5"), "opacity: 0.5;$0");
    }

    #[test]
    fn css_color() {
        assert_eq!(css("c#fff"), "color: #fff;$0");
        assert_eq!(css("c#f"), "color: #ffffff;$0");
        assert_eq!(css("bgc#e0"), "background-color: #e0e0e0;$0");
    }

    #[test]
    fn css_keyword_snippets() {
        assert_eq!(css("df"), "display: flex;$0");
        assert_eq!(css("pos:a"), "position: absolute;$0");
        assert_eq!(css("d:n"), "display: none;$0");
        assert_eq!(css("ta:c"), "text-align: center;$0");
        assert_eq!(css("fw:b"), "font-weight: bold;$0");
    }

    #[test]
    fn css_important() {
        assert_eq!(css("m10!"), "margin: 10px !important;$0");
        assert_eq!(css("df!"), "display: flex !important;$0");
    }

    #[test]
    fn css_unknown_is_none() {
        assert!(expand_css("zzqq123").is_none());
    }

    #[test]
    fn css_extract() {
        let (start, abbr) = extract_css_abbreviation("\tc#fff").unwrap();
        assert_eq!(start, 1);
        assert_eq!(abbr, "c#fff");
    }
}
