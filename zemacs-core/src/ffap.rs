//! `ffap` — the pure part of Emacs's "find file at point" (`ffap.el`): scanning
//! text for the things that look like file names.
//!
//! Emacs decides with `ffap-next-regexp` what is worth trying, then keeps only
//! the guesses that name a file that exists (`ffap-file-exists-string`). This
//! module is the first half — no file system, so it is unit-tested here; the
//! commands in `zemacs-term` do the resolving and the existence check.

/// One file-name guess in a buffer: the char range it occupies and its text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRef {
    /// Char index of the first character of the guess.
    pub start: usize,
    /// Char index one past its last character.
    pub end: usize,
    /// The guess itself, stripped of surrounding punctuation and of a trailing
    /// `:LINE` / `:LINE:COL` suffix (kept in [`FileRef::line`]).
    pub path: String,
    /// The line number that followed the name (`src/main.rs:42`), if any.
    pub line: Option<usize>,
}

/// Characters a file name may contain, once quotes and brackets are peeled off.
fn is_path_char(c: char) -> bool {
    c.is_alphanumeric() || "/._-~+$@%#:\\".contains(c)
}

/// Characters that surround a file name in prose or code and are not part of it.
fn is_wrapper(c: char) -> bool {
    "\"'`(){}[]<>,;!?".contains(c)
}

/// Every file-name guess in `text`, in order.
///
/// A guess is a run of [path characters](is_path_char) that looks like a path
/// rather than a word: it contains a `/`, or starts with `~`, or has a file
/// extension (`foo.rs`). Trailing sentence punctuation (`.`, `,`, `:`) is peeled
/// off, and a trailing `:LINE` (as in a compiler error) is split out of the name.
/// Bare words, plain numbers and lone dots are not guesses.
pub fn file_refs(text: &str) -> Vec<FileRef> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if !is_path_char(chars[i]) || is_wrapper(chars[i]) {
            i += 1;
            continue;
        }
        let start = i;
        while i < chars.len() && is_path_char(chars[i]) && !is_wrapper(chars[i]) {
            i += 1;
        }
        let mut end = i;
        // Peel trailing punctuation that ends a sentence rather than a name.
        while end > start && matches!(chars[end - 1], '.' | ',' | ':' | ';' | '-') {
            end -= 1;
        }
        if end == start {
            continue;
        }
        let token: String = chars[start..end].iter().collect();
        let (path, line) = split_line_suffix(&token);
        if !looks_like_path(&path) {
            continue;
        }
        let end = start + path.chars().count() + line.map_or(0, |n| 1 + n.to_string().len());
        out.push(FileRef {
            start,
            end,
            path,
            line,
        });
    }
    out
}

/// Split a `path:LINE` or `path:LINE:COL` guess into the path and the line.
fn split_line_suffix(token: &str) -> (String, Option<usize>) {
    let mut parts = token.split(':');
    let head = parts.next().unwrap_or("").to_string();
    // A Windows drive letter (`C:\src`) is not a line number.
    if head.len() == 1 && head.chars().all(|c| c.is_ascii_alphabetic()) {
        return (token.to_string(), None);
    }
    match parts.next().map(str::parse::<usize>) {
        Some(Ok(n)) => (head, Some(n)),
        _ => (token.to_string(), None),
    }
}

/// Whether a token is worth trying as a file name at all: it has a directory
/// separator, starts at the home directory, or carries an extension.
fn looks_like_path(token: &str) -> bool {
    if token.is_empty() || token.chars().all(|c| c == '.' || c == '/') {
        return false;
    }
    if token.contains('/') || token.starts_with('~') {
        return true;
    }
    match token.rsplit_once('.') {
        Some((stem, ext)) => {
            !stem.is_empty()
                && !ext.is_empty()
                && ext.chars().all(|c| c.is_alphanumeric())
                && !ext.chars().all(|c| c.is_ascii_digit())
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(text: &str) -> Vec<String> {
        file_refs(text).into_iter().map(|r| r.path).collect()
    }

    #[test]
    fn finds_paths_and_ignores_words() {
        assert_eq!(
            paths("see src/main.rs for the rest"),
            vec!["src/main.rs".to_string()],
            "a word with a slash is a path; plain words are not"
        );
        assert_eq!(paths("just some prose here"), Vec::<String>::new());
        assert_eq!(paths("open ~/.config/zemacs"), vec!["~/.config/zemacs"]);
        assert_eq!(paths("Cargo.toml is the manifest"), vec!["Cargo.toml"]);
    }

    #[test]
    fn strips_wrappers_and_trailing_punctuation() {
        assert_eq!(paths("(see ./notes.md)."), vec!["./notes.md"]);
        assert_eq!(paths("\"src/lib.rs\","), vec!["src/lib.rs"]);
        assert_eq!(paths("edit README.md."), vec!["README.md"]);
    }

    #[test]
    fn splits_a_trailing_line_number() {
        let refs = file_refs("src/main.rs:42: error here");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].path, "src/main.rs");
        assert_eq!(refs[0].line, Some(42));
        // The range covers the name and the line suffix.
        assert_eq!(refs[0].start, 0);
        assert_eq!(refs[0].end, "src/main.rs:42".chars().count());
    }

    #[test]
    fn ranges_point_at_the_guess() {
        let text = "look at src/a.rs now";
        let refs = file_refs(text);
        assert_eq!(refs.len(), 1);
        let got: String = text
            .chars()
            .skip(refs[0].start)
            .take(refs[0].end - refs[0].start)
            .collect();
        assert_eq!(got, "src/a.rs");
    }

    #[test]
    fn a_bare_number_or_version_is_not_a_path() {
        assert_eq!(paths("version 1.2.3 released"), Vec::<String>::new());
        assert_eq!(paths("... and so on"), Vec::<String>::new());
    }
}
