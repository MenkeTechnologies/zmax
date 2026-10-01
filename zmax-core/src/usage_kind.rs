//! What a usage of a symbol is, read from its line — for the JetBrains Usages
//! view's "Group by Usage Type" and its filters (comments, imports, read and
//! write access, generated code, test scope).
//!
//! The IDE gets these from its own code model; a language server's references
//! carry none of it, so they are read from the text: the comment token before
//! the usage, the import keyword the line starts with, the assignment or
//! increment after it, the call parenthesis. Filesystem-free and unit tested.

use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum UsageKind {
    Import,
    Comment,
    Write,
    Call,
    Read,
}

impl UsageKind {
    /// The group name the IDE shows for it.
    pub fn label(self) -> &'static str {
        match self {
            UsageKind::Import => "Import",
            UsageKind::Comment => "Usage in comments",
            UsageKind::Write => "Write access",
            UsageKind::Call => "Method call",
            UsageKind::Read => "Read access",
        }
    }
}

/// Keywords that open an import line, across the languages zmax edits.
const IMPORT_KEYWORDS: &[&str] = &[
    "use ", "import ", "from ", "#include", "extern crate ", "using ", "require ", "require(",
    "@import ", "library(", "source ",
];

/// Assignment operators that make the usage before them a write.
const COMPOUND_ASSIGN: &[&str] = &[
    "+=", "-=", "*=", "/=", "%=", "|=", "&=", "^=", "<<=", ">>=", "||=", "&&=", "??=", ".=", "**=",
    "//=",
];

/// Classify the usage at char column `col`, `len` chars long, in `line`.
/// `comment_tokens` are the language's line-comment tokens.
pub fn classify(line: &str, col: usize, len: usize, comment_tokens: &[&str]) -> UsageKind {
    let chars: Vec<char> = line.chars().collect();
    let before: String = chars.iter().take(col).collect();
    let after: String = chars.iter().skip(col + len).collect();
    let trimmed = line.trim_start();

    if in_comment(&before, comment_tokens) || is_comment_line(trimmed) {
        return UsageKind::Comment;
    }
    if IMPORT_KEYWORDS.iter().any(|k| trimmed.starts_with(k)) {
        return UsageKind::Import;
    }
    let next = after.trim_start();
    let assigns = next.starts_with('=') && !next.starts_with("==") && !next.starts_with("=>");
    if assigns
        || COMPOUND_ASSIGN.iter().any(|op| next.starts_with(op))
        || next.starts_with("++")
        || next.starts_with("--")
        || before.trim_end().ends_with("++")
        || before.trim_end().ends_with("--")
    {
        return UsageKind::Write;
    }
    if next.starts_with('(') || next.starts_with("!(") || next.starts_with("::<") {
        return UsageKind::Call;
    }
    UsageKind::Read
}

/// Whether `before` — the line up to the usage — has opened a comment outside
/// any string.
fn in_comment(before: &str, comment_tokens: &[&str]) -> bool {
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for (i, c) in before.char_indices() {
        match quote {
            Some(q) => {
                if escaped {
                    escaped = false;
                } else if c == '\\' {
                    escaped = true;
                } else if c == q {
                    quote = None;
                }
            }
            None => {
                if matches!(c, '"' | '\'' | '`') {
                    quote = Some(c);
                } else if comment_tokens.iter().any(|t| !t.is_empty() && before[i..].starts_with(t))
                    || before[i..].starts_with("/*")
                {
                    return true;
                }
            }
        }
    }
    false
}

/// A line inside a block comment, by its usual continuation marks.
fn is_comment_line(trimmed: &str) -> bool {
    trimmed.starts_with("* ") || trimmed == "*" || trimmed.starts_with("*/")
}

/// Test code, by the conventions of path and file name: a `test`/`tests`/
/// `spec`/`__tests__` directory, or `_test.`, `.test.`, `.spec.`, `test_`.
pub fn is_test_path(path: &Path) -> bool {
    let in_test_dir = path.components().any(|c| {
        matches!(
            c.as_os_str().to_str(),
            Some("test" | "tests" | "spec" | "specs" | "__tests__" | "testing")
        )
    });
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    in_test_dir
        || name.contains("_test.")
        || name.contains(".test.")
        || name.contains(".spec.")
        || name.contains("_spec.")
        || name.starts_with("test_")
}

/// Generated code by its location: build output directories, or a generated
/// name.
pub fn is_generated_path(path: &Path) -> bool {
    let in_output = path.components().any(|c| {
        matches!(
            c.as_os_str().to_str(),
            Some("target" | "build" | "dist" | "out" | "gen" | "generated" | "node_modules")
        )
    });
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    in_output || name.contains(".generated.") || name.contains("_generated.") || name.ends_with(".pb.go")
}

/// The marker generators leave in a file's first lines.
pub fn has_generated_marker(head: &str) -> bool {
    head.lines()
        .take(5)
        .any(|l| l.contains("@generated") || l.contains("DO NOT EDIT") || l.contains("Code generated"))
}

/// Files that make a directory a module root: a package of its build tool.
const MODULE_FILES: &[&str] = &[
    "Cargo.toml", "package.json", "go.mod", "pom.xml", "build.gradle", "build.gradle.kts",
    "pyproject.toml", "setup.py", "Gemfile", "mix.exs", "composer.json", "CMakeLists.txt",
];

/// The nearest directory above `path` holding a module file — the closest a
/// directory tree comes to the IDE's module. `exists` is the filesystem test.
pub fn module_root(path: &Path, exists: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    path.ancestors()
        .skip(1)
        .find(|dir| MODULE_FILES.iter().any(|f| exists(&dir.join(f))))
        .map(Path::to_path_buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kind(line: &str, word: &str) -> UsageKind {
        let col = line[..line.find(word).unwrap()].chars().count();
        classify(line, col, word.chars().count(), &["//", "#"])
    }

    #[test]
    fn kinds_of_usage() {
        assert_eq!(UsageKind::Write, kind("    count = 1;", "count"));
        assert_eq!(UsageKind::Write, kind("count += step", "count"));
        assert_eq!(UsageKind::Write, kind("count++;", "count"));
        assert_eq!(UsageKind::Write, kind("++count;", "count"));
        assert_eq!(UsageKind::Read, kind("if count == 1 {", "count"));
        assert_eq!(UsageKind::Read, kind("x => count", "count"));
        assert_eq!(UsageKind::Call, kind("let n = parse(s);", "parse"));
        assert_eq!(UsageKind::Call, kind("println!(\"x\")", "println"));
        assert_eq!(UsageKind::Import, kind("use crate::parse;", "parse"));
        assert_eq!(UsageKind::Import, kind("from x import parse", "parse"));
        assert_eq!(UsageKind::Comment, kind("x(); // calls parse", "parse"));
        assert_eq!(UsageKind::Comment, kind(" * see parse", "parse"));
        assert_eq!(UsageKind::Call, kind("f(\"//\", parse(x))", "parse"), "a token in a string is not a comment");
    }

    #[test]
    fn scopes_and_generated_code() {
        assert!(is_test_path(Path::new("src/tests/a.rs")));
        assert!(is_test_path(Path::new("web/app.spec.ts")));
        assert!(is_test_path(Path::new("pkg/x_test.go")));
        assert!(!is_test_path(Path::new("src/contest.rs")));
        assert!(is_generated_path(Path::new("target/debug/build/x.rs")));
        assert!(is_generated_path(Path::new("api/v1.pb.go")));
        assert!(!is_generated_path(Path::new("src/builder.rs")));
        assert!(has_generated_marker("// Code generated by protoc. DO NOT EDIT.\npackage x"));
    }

    #[test]
    fn module_is_the_nearest_package_root() {
        let roots = [Path::new("/w/Cargo.toml"), Path::new("/w/crates/a/Cargo.toml")];
        let exists = |p: &Path| roots.contains(&p);
        assert_eq!(Some(PathBuf::from("/w/crates/a")), module_root(Path::new("/w/crates/a/src/x.rs"), exists));
        assert_eq!(Some(PathBuf::from("/w")), module_root(Path::new("/w/src/y.rs"), exists));
        assert_eq!(None, module_root(Path::new("/elsewhere/z.rs"), exists));
    }
}
