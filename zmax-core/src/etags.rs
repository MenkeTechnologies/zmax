//! Tags tables — the zmax port of the GNU Emacs `etags.el` reader.
//!
//! An `etags`-format TAGS file is a sequence of per-source-file sections. Each
//! section starts with a form feed and a header line, then one line per tag:
//!
//! ```text
//! \x0c
//! src/main.c,257
//! int main(\x7f12,143
//! static void helper(\x7fhelper\x0119,201
//! ```
//!
//! The header is `FILE,SIZE`. A tag line is `PATTERN \x7f [NAME \x01] LINE,BYTE`:
//! the explicit `NAME\x01` part is optional, and when it is absent the tag name
//! is the last identifier in the pattern (which is how `etags` writes C).
//!
//! This module parses that format and answers the questions the tags commands
//! ask — which files does the table cover, which tags does a file define, where
//! is a named tag. No I/O: the command layer reads the TAGS file and the source
//! files.

/// One tag: a name, the source line it was found on, and the text `etags`
/// captured as the pattern for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tag {
    /// The identifier the tag is looked up by.
    pub name: String,
    /// The source text `etags` recorded, used to confirm the line has not moved.
    pub pattern: String,
    /// 1-based line number in the source file.
    pub line: usize,
    /// Byte offset of the tag within the source file.
    pub byte_offset: usize,
}

/// One source file's section of a tags table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TagsFile {
    /// The path as written in the TAGS file (relative to the TAGS file's dir).
    pub path: String,
    /// The tags this file defines, in the order `etags` emitted them.
    pub tags: Vec<Tag>,
}

/// Parse an etags-format TAGS file.
pub fn parse(src: &str) -> Vec<TagsFile> {
    let mut out: Vec<TagsFile> = Vec::new();
    // Sections are separated by the form-feed character.
    for section in src.split('\x0c') {
        let mut lines = section.lines().skip_while(|l| l.is_empty());
        let Some(header) = lines.next() else { continue };
        // `FILE,SIZE` — the size is advisory and ignored.
        let Some((path, _size)) = header.rsplit_once(',') else {
            continue;
        };
        if path.is_empty() {
            continue;
        }
        let mut tags = Vec::new();
        for line in lines {
            if let Some(tag) = parse_tag_line(line) {
                tags.push(tag);
            }
        }
        out.push(TagsFile {
            path: path.to_string(),
            tags,
        });
    }
    out
}

/// One `PATTERN \x7f [NAME \x01] LINE,BYTE` line, or `None` if it is not one.
fn parse_tag_line(line: &str) -> Option<Tag> {
    let (pattern, rest) = line.split_once('\x7f')?;
    // The explicit-name form puts `NAME\x01` between the DEL and the position.
    let (explicit, pos) = match rest.split_once('\x01') {
        Some((name, pos)) => (Some(name), pos),
        None => (None, rest),
    };
    let (line_no, byte) = pos.split_once(',')?;
    let name = match explicit {
        Some(n) => n.to_string(),
        None => implicit_name(pattern)?,
    };
    Some(Tag {
        name,
        pattern: pattern.to_string(),
        line: line_no.trim().parse().ok()?,
        byte_offset: byte.trim().parse().unwrap_or(0),
    })
}

/// The tag name `etags` leaves implicit: the last identifier in the pattern.
/// `etags` records `int main(` for `int main(void)`, and the reader recovers
/// `main` by taking the trailing identifier before the punctuation.
fn implicit_name(pattern: &str) -> Option<String> {
    let is_ident = |c: char| c.is_alphanumeric() || c == '_' || c == '$';
    // Walk the identifier runs from the right. A run of digits is a literal
    // (`= 0`), never a tag name, so keep going left past it.
    let mut rest = pattern;
    loop {
        rest = rest.trim_end_matches(|c: char| !is_ident(c));
        if rest.is_empty() {
            return None;
        }
        let start = rest
            .char_indices()
            .rev()
            .take_while(|(_, c)| is_ident(*c))
            .last()
            .map(|(i, _)| i)
            .unwrap_or(0);
        let name = &rest[start..];
        if !name.chars().all(|c| c.is_ascii_digit()) {
            return Some(name.to_string());
        }
        rest = &rest[..start];
    }
}

/// Every `(file, tag)` whose tag name is exactly `name` — what `find-tag` needs.
pub fn find<'a>(table: &'a [TagsFile], name: &str) -> Vec<(&'a TagsFile, &'a Tag)> {
    table
        .iter()
        .flat_map(|f| f.tags.iter().map(move |t| (f, t)))
        .filter(|(_, t)| t.name == name)
        .collect()
}

/// Tag names starting with `prefix`, sorted and deduplicated — the completion
/// list Emacs offers at the `find-tag` prompt.
pub fn complete(table: &[TagsFile], prefix: &str) -> Vec<String> {
    let mut names: Vec<String> = table
        .iter()
        .flat_map(|f| f.tags.iter())
        .filter(|t| t.name.starts_with(prefix))
        .map(|t| t.name.clone())
        .collect();
    names.sort();
    names.dedup();
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\x0c\nsrc/main.c,142\nint main(\x7f12,143\nstatic void helper(\x7fhelper\x0119,201\n\x0c\nsrc/util.h,64\n#define MAX_LEN\x7fMAX_LEN\x014,30\n";

    /// Both tag-line forms must parse: the implicit one (name recovered from the
    /// pattern) and the explicit one (`NAME\x01`). Getting the implicit form
    /// wrong is how a tags table silently loses half its C functions.
    #[test]
    fn parses_both_the_implicit_and_explicit_tag_forms() {
        let table = parse(SAMPLE);
        assert_eq!(table.len(), 2, "two file sections");
        assert_eq!(table[0].path, "src/main.c");
        assert_eq!(table[0].tags.len(), 2);

        let main = &table[0].tags[0];
        assert_eq!(main.name, "main", "recovered from the pattern `int main(`");
        assert_eq!(main.pattern, "int main(");
        assert_eq!(main.line, 12);
        assert_eq!(main.byte_offset, 143);

        let helper = &table[0].tags[1];
        assert_eq!(helper.name, "helper", "taken from the explicit NAME field");
        assert_eq!(helper.line, 19);

        assert_eq!(table[1].path, "src/util.h");
        assert_eq!(table[1].tags[0].name, "MAX_LEN");
    }

    /// The implicit name is the trailing identifier, so declarators with
    /// pointers, templates or namespaces still name the right symbol.
    #[test]
    fn implicit_name_takes_the_last_identifier_in_the_pattern() {
        assert_eq!(implicit_name("int main(").as_deref(), Some("main"));
        assert_eq!(implicit_name("char *strdup(").as_deref(), Some("strdup"));
        assert_eq!(implicit_name("void Foo::bar(").as_deref(), Some("bar"));
        // A trailing numeric literal is skipped: `0` is not a tag name.
        assert_eq!(
            implicit_name("static int count_ = 0").as_deref(),
            Some("count_")
        );
        assert_eq!(implicit_name("#define MAX 128").as_deref(), Some("MAX"));
        assert_eq!(implicit_name("  ").as_deref(), None);
    }

    /// Lookup and completion are what the commands actually call.
    #[test]
    fn lookup_finds_the_defining_file_and_completion_is_sorted() {
        let table = parse(SAMPLE);
        let hits = find(&table, "helper");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].0.path, "src/main.c");
        assert_eq!(hits[0].1.line, 19);
        assert!(find(&table, "nope").is_empty());

        assert_eq!(complete(&table, "m"), vec!["main"]);
        assert_eq!(complete(&table, ""), vec!["MAX_LEN", "helper", "main"]);
    }

    /// Junk between sections must not derail the parse — a TAGS file with a
    /// trailing newline or an unparseable line still yields its good tags.
    #[test]
    fn tolerates_malformed_lines() {
        let src = "\x0c\nsrc/a.c,10\ngarbage with no del\nint f(\x7f3,4\n";
        let table = parse(src);
        assert_eq!(table.len(), 1);
        assert_eq!(table[0].tags.len(), 1, "the garbage line is skipped");
        assert_eq!(table[0].tags[0].name, "f");
        assert!(parse("").is_empty());
    }
}
