use std::borrow::Cow;

use zmax_core::command_line::{ExpansionKind, Token, TokenKind, Tokenizer};

use anyhow::{anyhow, bail, ensure, Result};

use crate::Editor;

/// Variables that can be expanded in the command mode (`:`) via the expansion syntax.
///
/// For example `%{cursor_line}`.
//
// To add a new variable follow these steps:
//
// * Add the new enum member to `Variable` below.
// * Add an item to the `VARIANTS` constant - this enables completion.
// * Add a branch in `Variable::as_str`, converting the name from TitleCase to snake_case.
// * Add a branch in `Variable::from_name` with the reverse association.
// * Add a branch in the `expand_variable` function to read the value from the editor.
// * Add the new variable to the documentation in `book/src/command-line.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Variable {
    /// The one-indexed line number of the primary cursor in the currently focused document.
    CursorLine,
    /// The one-indexed column number of the primary cursor in the currently focused document.
    ///
    /// Note that this is the count of grapheme clusters from the start of the line (regardless of
    /// softwrap) - the same as the `position` element in the statusline.
    CursorColumn,
    /// The display name of the currently focused document.
    ///
    /// This corresponds to `crate::Document::display_name`.
    BufferName,
    /// The absolute path of the currently focused document. For scratch buffers this will default
    /// to the current working directory.
    FilePathAbsolute,
    /// A string containing the line-ending of the currently focused document.
    LineEnding,
    /// Curreng working directory
    CurrentWorkingDirectory,
    /// Nearest ancestor directory of the current working directory that contains `.git`, `.svn`, `jj` or `.zmax`
    WorkspaceDirectory,
    // The name of current buffers language as set in `languages.toml`
    Language,
    // Primary selection
    Selection,
    // The one-indexed line number of the start of the primary selection in the currently focused document.
    SelectionLineStart,
    // The one-indexed line number of the end of the primary selection in the currently focused document.
    SelectionLineEnd,
}

impl Variable {
    pub const VARIANTS: &'static [Self] = &[
        Self::CursorLine,
        Self::CursorColumn,
        Self::BufferName,
        Self::FilePathAbsolute,
        Self::LineEnding,
        Self::CurrentWorkingDirectory,
        Self::WorkspaceDirectory,
        Self::Language,
        Self::Selection,
        Self::SelectionLineStart,
        Self::SelectionLineEnd,
    ];

    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::CursorLine => "cursor_line",
            Self::CursorColumn => "cursor_column",
            Self::BufferName => "buffer_name",
            Self::FilePathAbsolute => "file_path_absolute",
            Self::LineEnding => "line_ending",
            Self::CurrentWorkingDirectory => "current_working_directory",
            Self::WorkspaceDirectory => "workspace_directory",
            Self::Language => "language",
            Self::Selection => "selection",
            Self::SelectionLineStart => "selection_line_start",
            Self::SelectionLineEnd => "selection_line_end",
        }
    }

    pub fn from_name(s: &str) -> Option<Self> {
        match s {
            "cursor_line" => Some(Self::CursorLine),
            "cursor_column" => Some(Self::CursorColumn),
            "buffer_name" => Some(Self::BufferName),
            "file_path_absolute" => Some(Self::FilePathAbsolute),
            "line_ending" => Some(Self::LineEnding),
            "workspace_directory" => Some(Self::WorkspaceDirectory),
            "current_working_directory" => Some(Self::CurrentWorkingDirectory),
            "language" => Some(Self::Language),
            "selection" => Some(Self::Selection),
            "selection_line_start" => Some(Self::SelectionLineStart),
            "selection_line_end" => Some(Self::SelectionLineEnd),
            _ => None,
        }
    }
}

/// Expands the given command line token.
///
/// Note that the lifetime of the expanded variable is only bound to the input token and not the
/// `Editor`. See `expand_variable` below for more discussion of lifetimes.
pub fn expand<'a>(editor: &Editor, token: Token<'a>) -> Result<Cow<'a, str>> {
    // Note: see the `TokenKind` documentation for more details on how each branch should expand.
    match token.kind {
        TokenKind::Unquoted | TokenKind::Quoted(_) => Ok(token.content),
        TokenKind::Expansion(ExpansionKind::Variable) => {
            let var = Variable::from_name(&token.content)
                .ok_or_else(|| anyhow!("unknown variable '{}'", token.content))?;

            expand_variable(editor, var)
        }
        TokenKind::Expansion(ExpansionKind::Unicode) => {
            if let Some(ch) = u32::from_str_radix(token.content.as_ref(), 16)
                .ok()
                .and_then(char::from_u32)
            {
                Ok(Cow::Owned(ch.to_string()))
            } else {
                Err(anyhow!(
                    "could not interpret '{}' as a Unicode character code",
                    token.content
                ))
            }
        }
        TokenKind::Expand => expand_inner(editor, token.content),
        TokenKind::Expansion(ExpansionKind::Shell) => expand_shell(editor, token.content),
        TokenKind::Expansion(ExpansionKind::Register) => expand_register(editor, token.content),
        // Note: see the docs for this variant.
        TokenKind::ExpansionKind => unreachable!(
            "expansion name tokens cannot be emitted when command line validation is enabled"
        ),
    }
}

/// Expand a shell command.
pub fn expand_shell<'a>(editor: &Editor, content: Cow<'a, str>) -> Result<Cow<'a, str>> {
    use std::process::{Command, Stdio};

    // Recursively expand the expansion's content before executing the shell command.
    let content = expand_inner(editor, content)?;

    let config = editor.config();
    let shell = &config.shell;
    let mut process = Command::new(&shell[0]);
    process
        .args(&shell[1..])
        .arg(content.as_ref())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // TODO: there is no protection here against a shell command taking a long time.
    // Ideally you should be able to hit `<ret>` in command mode and then be able to
    // cancel the invocation (for example with `<C-c>`) if it takes longer than you'd
    // like.
    let output = match process.spawn() {
        Ok(process) => process.wait_with_output()?,
        Err(err) => {
            bail!("Failed to start shell: {err}");
        }
    };

    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();

    if !output.stderr.is_empty() {
        log::warn!(
            "Shell expansion command `{content}` failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    // Trim exactly one trailing line ending if it exists.
    if text.ends_with('\n') {
        text.pop();
        if text.ends_with('\r') {
            text.pop();
        }
    }

    Ok(Cow::Owned(text))
}

/// Expand a register's contents
pub fn expand_register<'a>(editor: &Editor, content: Cow<'a, str>) -> Result<Cow<'a, str>> {
    let mut chars = content.chars();
    let Some(r) = chars.next() else {
        bail!("Missing register name");
    };
    ensure!(
        chars.next().is_none(),
        "Invalid register `{content}`: should only be a single character"
    );
    match editor.registers.read(r, editor) {
        Some(values) => Ok(Cow::Owned(values.collect())),
        None => Ok(Cow::Borrowed("")),
    }
}

/// Expand a token's contents recursively.
fn expand_inner<'a>(editor: &Editor, content: Cow<'a, str>) -> Result<Cow<'a, str>> {
    let mut escaped = String::new();
    let mut start = 0;

    while let Some(offset) = content[start..].find('%') {
        let idx = start + offset;
        if content.as_bytes().get(idx + '%'.len_utf8()).copied() == Some(b'%') {
            // Treat two percents in a row as an escaped percent.
            escaped.push_str(&content[start..=idx]);
            // Skip over both percents.
            start = idx + ('%'.len_utf8() * 2);
        } else {
            // Otherwise interpret the percent as an expansion. Push up to (but not
            // including) the percent token.
            escaped.push_str(&content[start..idx]);
            // Then parse the expansion,
            let mut tokenizer = Tokenizer::new(&content[idx..], true);
            let token = tokenizer
                .parse_percent_token()
                .unwrap()
                .map_err(|err| anyhow!("{err}"))?;
            // expand it (this is the recursive part),
            let expanded = expand(editor, token)?;
            escaped.push_str(expanded.as_ref());
            // and move forward to the end of the expansion.
            start = idx + tokenizer.pos();
        }
    }

    if escaped.is_empty() {
        Ok(content)
    } else {
        escaped.push_str(&content[start..]);
        Ok(Cow::Owned(escaped))
    }
}

// Note: the lifetime of the expanded variable (the `Cow`) must not be tied to the lifetime of
// the borrow of `Editor`. That would prevent commands from mutating the `Editor` until the
// command consumed or cloned all arguments - this is poor ergonomics. A sensible thing for this
// function to return then, instead, would normally be a `String`. We can return some statically
// known strings like the scratch buffer name or line ending strings though, so this function
// returns a `Cow<'static, str>` instead.
fn expand_variable(editor: &Editor, variable: Variable) -> Result<Cow<'static, str>> {
    let (view, doc) = current_ref!(editor);
    let text = doc.text().slice(..);

    match variable {
        Variable::CursorLine => {
            let cursor_line = doc.selection(view.id).primary().cursor_line(text);
            Ok(Cow::Owned((cursor_line + 1).to_string()))
        }
        Variable::CursorColumn => {
            let cursor = doc.selection(view.id).primary().cursor(text);
            let position = zmax_core::coords_at_pos(text, cursor);
            Ok(Cow::Owned((position.col + 1).to_string()))
        }
        Variable::BufferName => {
            // Note: usually we would use `Document::display_name` but we can statically borrow
            // the scratch buffer name by partially reimplementing `display_name`.
            if let Some(path) = doc.relative_path() {
                Ok(Cow::Owned(path.to_string_lossy().into_owned()))
            } else {
                Ok(Cow::Borrowed(crate::document::SCRATCH_BUFFER_NAME))
            }
        }
        Variable::FilePathAbsolute => {
            let path = match doc.path() {
                Some(path) => path.to_owned(),
                None => zmax_stdx::env::current_working_dir(),
            }
            .to_string_lossy()
            .to_string();
            Ok(Cow::Owned(path))
        }
        Variable::LineEnding => Ok(Cow::Borrowed(doc.line_ending.as_str())),
        Variable::CurrentWorkingDirectory => Ok(std::borrow::Cow::Owned(
            zmax_stdx::env::current_working_dir()
                .to_string_lossy()
                .to_string(),
        )),
        Variable::WorkspaceDirectory => Ok(std::borrow::Cow::Owned(
            zmax_loader::find_workspace()
                .0
                .to_string_lossy()
                .to_string(),
        )),
        Variable::Language => Ok(match doc.language_name() {
            Some(lang) => Cow::Owned(lang.to_owned()),
            None => Cow::Borrowed("text"),
        }),
        Variable::Selection => Ok(Cow::Owned(
            doc.selection(view.id).primary().fragment(text).to_string(),
        )),
        Variable::SelectionLineStart => {
            let start_line = doc.selection(view.id).primary().line_range(text).0;
            Ok(Cow::Owned((start_line + 1).to_string()))
        }
        Variable::SelectionLineEnd => {
            let end_line = doc.selection(view.id).primary().line_range(text).1;
            Ok(Cow::Owned((end_line + 1).to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The variable table in the book, as `name` -> description. Adding a
    /// variable means touching five places in this file plus that table (see the
    /// checklist at the top), and nothing but this test notices when one is
    /// missed.
    fn documented_variables() -> Vec<String> {
        include_str!("../../book/src/command-line.md")
            .lines()
            // Rows of the variable table: `| `name` | description |`.
            .filter_map(|line| line.strip_prefix("| `")?.split_once('`'))
            .map(|(name, _)| name.to_string())
            // The table of expansion kinds uses the same row shape; variables are
            // plain snake_case names, so anything else is a different table.
            .filter(|name| {
                !name.is_empty() && name.chars().all(|c| c.is_ascii_lowercase() || c == '_')
            })
            .collect()
    }

    /// `as_str` and `from_name` are hand-written inverses of each other. A name
    /// that does not survive the round trip is a variable the user can see in
    /// completion but cannot actually expand.
    #[test]
    fn every_variant_round_trips_through_its_name() {
        for variable in Variable::VARIANTS {
            let name = variable.as_str();
            assert_eq!(
                Variable::from_name(name),
                Some(*variable),
                "{name} does not come back"
            );
        }

        assert_eq!(Variable::from_name("not_a_variable"), None);
        assert_eq!(Variable::from_name(""), None);
        // Names are matched exactly, not case-insensitively or by prefix.
        assert_eq!(Variable::from_name("Cursor_Line"), None);
        assert_eq!(Variable::from_name("cursor_lin"), None);
    }

    /// Two variants sharing a name would make one of them unreachable through
    /// `from_name`, and a duplicate in `VARIANTS` would show twice in completion.
    #[test]
    fn variant_names_are_unique() {
        let mut names: Vec<_> = Variable::VARIANTS.iter().map(|v| v.as_str()).collect();
        let count = names.len();
        names.sort_unstable();
        names.dedup();

        assert_eq!(names.len(), count, "duplicate name in VARIANTS");
    }

    /// The book documents exactly the variables that exist. A variable missing
    /// from the table is undiscoverable; one listed but not implemented expands
    /// to an error when a reader copies it.
    #[test]
    fn the_book_documents_exactly_the_supported_variables() {
        let documented = documented_variables();
        assert!(
            documented.len() >= Variable::VARIANTS.len(),
            "the variable table did not parse: found {documented:?}"
        );

        for variable in Variable::VARIANTS {
            let name = variable.as_str();
            assert!(
                documented.iter().any(|doc| doc == name),
                "{name} is not in book/src/command-line.md"
            );
        }

        for name in &documented {
            // Only rows naming a real variable belong in that table.
            assert!(
                Variable::from_name(name).is_some(),
                "the book documents `{name}`, which no longer exists"
            );
        }
    }
}
