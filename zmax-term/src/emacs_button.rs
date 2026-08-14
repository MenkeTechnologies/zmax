//! Emacs buttons (`button.el`) — the focusable regions `forward-button` /
//! `backward-button` step over, `push-button` activates and `display-local-help`
//! describes.
//!
//! Emacs marks a button with text properties, so a button is whatever the buffer's
//! producer decided to make one. zmax's documents carry no such properties, so a
//! button here is a region the editor can *act* on, recognised from the text:
//!
//! * a URL — activating it is `browse-url`;
//! * an existing file/directory path — activating it is `find-file`;
//! * a command name, but only in a buffer with no file behind it (zmax's `*Help*`
//!   / report scratch buffers). That mirrors emacs, where the symbol hyperlinks
//!   exist in help buffers and not in the C file you are editing.
//!
//! Offsets are character offsets into the text that was scanned, so the caller can
//! turn one straight into a `Selection::point`.

/// What activating a button does — button.el's `action` property.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ButtonKind {
    /// A URL; `browse-url` opens it.
    Url(String),
    /// A path that exists; `find-file` visits it.
    File(String),
    /// A command name; following the button runs it.
    Command(String),
}

/// One button: the region it covers plus what it does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Button {
    pub start: usize,
    pub end: usize,
    pub kind: ButtonKind,
}

impl Button {
    /// button.el's `help-echo`: the one-line description `display-local-help`
    /// shows in the echo area and `button-describe` reports as the action.
    pub fn help_echo(&self) -> String {
        match &self.kind {
            ButtonKind::Url(u) => format!("mouse-1, RET: browse-url — open {u}"),
            ButtonKind::File(p) => format!("mouse-1, RET: find-file — visit {p}"),
            ButtonKind::Command(c) => format!("mouse-1, RET: run the command `{c}`"),
        }
    }
}

/// Characters that end a URL when they trail it (a link at the end of a sentence).
const URL_TRAILERS: &[char] = &['.', ',', ';', ':', ')', ']', '}', '>', '"', '\'', '!', '?'];

/// The URL schemes zmax treats as links, as `browse-url` does.
const SCHEMES: &[&str] = &["http://", "https://", "ftp://", "file://", "mailto:"];

/// Every button in `text`, in buffer order. `is_command` decides whether a bare
/// word is a command name — the caller passes a closure that consults the command
/// table, and passes one that always answers `false` for a file-visiting buffer,
/// where emacs would have no symbol hyperlinks either.
pub fn buttons(text: &str, is_command: &dyn Fn(&str) -> bool) -> Vec<Button> {
    let mut out = Vec::new();
    let mut start = 0usize; // char offset of the token being accumulated
    let mut token = String::new();
    let mut idx = 0usize;
    let flush = |token: &mut String, start: usize, out: &mut Vec<Button>| {
        if !token.is_empty() {
            if let Some(b) = classify(token, start, is_command) {
                out.push(b);
            }
            token.clear();
        }
    };
    for ch in text.chars() {
        if ch.is_whitespace() {
            flush(&mut token, start, &mut out);
            start = idx + 1;
        } else {
            if token.is_empty() {
                start = idx;
            }
            token.push(ch);
        }
        idx += 1;
    }
    flush(&mut token, start, &mut out);
    out
}

/// Classify one whitespace-delimited token, trimming the punctuation that
/// commonly trails a link in prose.
fn classify(token: &str, start: usize, is_command: &dyn Fn(&str) -> bool) -> Option<Button> {
    let trimmed = token.trim_end_matches(URL_TRAILERS);
    if trimmed.is_empty() {
        return None;
    }
    let end = start + trimmed.chars().count();
    let lower = trimmed.to_ascii_lowercase();
    if SCHEMES.iter().any(|s| lower.starts_with(s)) {
        return Some(Button {
            start,
            end,
            kind: ButtonKind::Url(trimmed.to_string()),
        });
    }
    if let Some(path) = existing_path(trimmed) {
        return Some(Button {
            start,
            end,
            kind: ButtonKind::File(path),
        });
    }
    // A bare word only counts where the caller says command hyperlinks exist.
    let word = trimmed.trim_matches(|c: char| !c.is_alphanumeric() && c != '_' && c != '-');
    if !word.is_empty() && is_command(word) {
        let skipped = trimmed.find(word).unwrap_or(0);
        // `find` is a byte offset; the tokens that reach here are ASCII-ish, but
        // count characters so a multi-byte prefix cannot shift the region.
        let lead = trimmed[..skipped].chars().count();
        return Some(Button {
            start: start + lead,
            end: start + lead + word.chars().count(),
            kind: ButtonKind::Command(word.to_string()),
        });
    }
    None
}

/// The token as a path that exists on disk, with a leading `~` expanded — what
/// `find-file` would visit. `None` when it is not a path to anything.
fn existing_path(token: &str) -> Option<String> {
    if !token.contains('/') && !token.contains('.') {
        return None;
    }
    let expanded = match token.strip_prefix("~/") {
        Some(rest) => std::env::var("HOME").ok().map(|h| format!("{h}/{rest}")),
        None => None,
    };
    let candidate = expanded.unwrap_or_else(|| token.to_string());
    std::path::Path::new(&candidate)
        .exists()
        .then_some(candidate)
}

/// The button covering `pos`, if any — button.el's `button-at`.
pub fn button_at(buttons: &[Button], pos: usize) -> Option<&Button> {
    buttons.iter().find(|b| pos >= b.start && pos < b.end)
}

/// `forward-button` (`TAB` in Help mode): the start of the `n`th next button from
/// `pos`, or the `n`th previous when `n` is negative. With `wrap`, moving past
/// either end continues from the other, which is what the interactive call does.
/// `None` when there is no further button — the caller reports emacs' error.
pub fn forward(buttons: &[Button], pos: usize, n: isize, wrap: bool) -> Option<usize> {
    if buttons.is_empty() {
        return None;
    }
    if n == 0 {
        // "If N is 0, move to the start of any button at point."
        return button_at(buttons, pos).map(|b| b.start);
    }
    let mut cur = pos;
    let step = if n > 0 { 1isize } else { -1 };
    for _ in 0..n.abs() {
        let next = if step > 0 {
            buttons.iter().find(|b| b.start > cur).map(|b| b.start)
        } else {
            buttons
                .iter()
                .rev()
                .find(|b| b.start < cur)
                .map(|b| b.start)
        };
        cur = match next {
            Some(p) => p,
            None if wrap => {
                if step > 0 {
                    buttons[0].start
                } else {
                    buttons[buttons.len() - 1].start
                }
            }
            None => return None,
        };
    }
    Some(cur)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_commands(_: &str) -> bool {
        false
    }

    #[test]
    fn urls_and_paths_are_buttons_and_trailing_punctuation_is_not() {
        let text = "see https://example.com/a, and zz-no-such.file here";
        let bs = buttons(text, &no_commands);
        assert_eq!(
            bs.iter().map(|b| b.kind.clone()).collect::<Vec<_>>(),
            vec![ButtonKind::Url("https://example.com/a".into())],
            "the comma is trimmed off the link; a path that does not exist is no button"
        );
        let b = &bs[0];
        assert_eq!(&text[b.start..b.end], "https://example.com/a");
        assert!(b.help_echo().contains("browse-url"));
    }

    #[test]
    fn an_existing_file_is_a_file_button() {
        let file = concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml");
        let text = format!("edit {file} now");
        let bs = buttons(&text, &no_commands);
        assert_eq!(bs.len(), 1);
        assert_eq!(bs[0].kind, ButtonKind::File(file.to_string()));
    }

    #[test]
    fn command_words_are_buttons_only_where_the_caller_allows_them() {
        let is_cmd = |w: &str| w == "goto_line";
        let text = "run goto_line to jump";
        assert!(buttons(text, &no_commands).is_empty());
        let bs = buttons(text, &is_cmd);
        assert_eq!(bs.len(), 1);
        assert_eq!(bs[0].kind, ButtonKind::Command("goto_line".into()));
        assert_eq!(&text[bs[0].start..bs[0].end], "goto_line");
    }

    #[test]
    fn forward_and_backward_wrap_like_button_buffer_map() {
        let text = "https://a.example https://b.example https://c.example";
        let bs = buttons(text, &no_commands);
        assert_eq!(bs.len(), 3);
        let first = bs[0].start;
        let last = bs[2].start;

        assert_eq!(forward(&bs, first, 1, true), Some(bs[1].start));
        assert_eq!(forward(&bs, first, 2, true), Some(last));
        // Past the last button, a wrapping call continues from the first.
        assert_eq!(forward(&bs, last, 1, true), Some(first));
        assert_eq!(
            forward(&bs, last, 1, false),
            None,
            "no wrap: no next button"
        );
        assert_eq!(forward(&bs, first, -1, true), Some(last));
        assert_eq!(forward(&bs, first, -1, false), None);
        // n = 0 moves to the start of the button at point.
        assert_eq!(forward(&bs, first + 3, 0, true), Some(first));
        assert_eq!(forward(&bs, first + 3, 0, false), Some(first));
    }

    #[test]
    fn button_at_finds_the_region_under_point() {
        let text = "x https://example.com y";
        let bs = buttons(text, &no_commands);
        assert!(button_at(&bs, 0).is_none());
        assert_eq!(button_at(&bs, 5).map(|b| b.start), Some(2));
        assert!(button_at(&bs, 22).is_none());
    }
}
