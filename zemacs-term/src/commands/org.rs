//! Org-mode support — slice 1: pure outline + TODO helpers.
//!
//! This module holds the engine-agnostic, unit-tested logic for org-style
//! outlines: detecting headings, finding a heading's subtree, cycling its TODO
//! keyword, and promoting/demoting its level. The typable commands that drive
//! the editor (`:org-cycle`, `:org-todo`, `:org-promote`, …) live in
//! `commands/typed.rs` and call into these helpers; folding reuses the
//! document's existing `zemacs_core::fold::Folds` model exactly like the vim
//! `z*` fold commands.
//!
//! Deferred to later slices: agenda, babel, export, priorities, keymap binds,
//! `.org` file-type detection and syntax highlighting.

/// Org heading depth: the number of leading `*` characters immediately followed
/// by a space (`* ` → 1, `** ` → 2, …). A run of stars not followed by a space
/// (`*notspace`, bare `***`) is not a heading and returns `None`. Headings must
/// start in column 0 — leading whitespace disqualifies the line.
pub fn heading_level(line: &str) -> Option<usize> {
    let stars = line.chars().take_while(|&c| c == '*').count();
    if stars == 0 {
        return None;
    }
    // The char right after the stars must be a space.
    if line[stars..].starts_with(' ') {
        Some(stars)
    } else {
        None
    }
}

/// Last line (0-based, inclusive) of the subtree rooted at `heading_line`: scan
/// forward to the line before the next heading whose level is `<=` this
/// heading's level, or the end of the buffer. If `heading_line` is not a
/// heading (or out of range), returns `heading_line` unchanged.
pub fn subtree_end(lines: &[&str], heading_line: usize) -> usize {
    let Some(&line) = lines.get(heading_line) else {
        return heading_line;
    };
    let Some(level) = heading_level(line) else {
        return heading_line;
    };
    for (i, l) in lines.iter().enumerate().skip(heading_line + 1) {
        if let Some(lvl) = heading_level(l) {
            if lvl <= level {
                return i - 1;
            }
        }
    }
    lines.len().saturating_sub(1)
}

/// Cycle the TODO keyword of a heading line: none → `TODO` → `DONE` → none.
/// The keyword sits right after the stars (`** foo` → `** TODO foo` →
/// `** DONE foo` → `** foo`). The stars and the remaining heading text are
/// preserved. Non-heading lines are returned unchanged.
pub fn cycle_todo(line: &str) -> String {
    let Some(level) = heading_level(line) else {
        return line.to_string();
    };
    let stars = &line[..level]; // the run of `*`
    let rest = &line[level + 1..]; // text after the single space following the stars

    let new_rest = if let Some(body) = strip_keyword(rest, "TODO") {
        // TODO → DONE, keeping the body.
        if body.is_empty() {
            "DONE".to_string()
        } else {
            format!("DONE {body}")
        }
    } else if let Some(body) = strip_keyword(rest, "DONE") {
        // DONE → none.
        body.to_string()
    } else {
        // none → TODO.
        if rest.is_empty() {
            "TODO".to_string()
        } else {
            format!("TODO {rest}")
        }
    };

    format!("{stars} {new_rest}")
}

/// If `rest` begins with `kw` as a whole word (followed by a space or end of
/// string), return the remaining body after it; otherwise `None`.
fn strip_keyword<'a>(rest: &'a str, kw: &str) -> Option<&'a str> {
    if rest == kw {
        Some("")
    } else {
        rest.strip_prefix(kw)
            .and_then(|tail| tail.strip_prefix(' '))
    }
}

/// Promote a heading one level (remove one leading `*`), clamped so a level-1
/// heading stays level 1. The space after the stars is preserved. Non-heading
/// lines are returned unchanged.
pub fn promote(line: &str) -> String {
    match heading_level(line) {
        Some(level) if level > 1 => line[1..].to_string(),
        _ => line.to_string(),
    }
}

/// Demote a heading one level (add one leading `*`). The space after the stars
/// is preserved. Non-heading lines are returned unchanged.
pub fn demote(line: &str) -> String {
    if heading_level(line).is_some() {
        format!("*{line}")
    } else {
        line.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heading_level_detects_stars_then_space() {
        assert_eq!(heading_level("not a heading"), None);
        assert_eq!(heading_level("* one"), Some(1));
        assert_eq!(heading_level("*** three"), Some(3));
        assert_eq!(heading_level("* "), Some(1)); // empty heading still a heading
        assert_eq!(heading_level("*notspace"), None); // stars not followed by space
        assert_eq!(heading_level("***"), None); // bare stars, no space
        assert_eq!(heading_level(" * indented"), None); // must start in column 0
        assert_eq!(heading_level(""), None);
    }

    #[test]
    fn subtree_end_stops_at_same_or_higher_level() {
        let lines = [
            "* a",      // 0
            "body",     // 1
            "** b",     // 2
            "body",     // 3
            "*** c",    // 4
            "body",     // 5
            "** d",     // 6
            "* e",      // 7
            "body",     // 8
        ];
        // subtree of "* a" runs until just before the next level-1 heading "* e".
        assert_eq!(subtree_end(&lines, 0), 6);
        // subtree of "** b" stops at the next same-level heading "** d".
        assert_eq!(subtree_end(&lines, 2), 5);
        // deepest heading takes its body up to the next shallower heading.
        assert_eq!(subtree_end(&lines, 4), 5);
        // last heading runs to end of buffer.
        assert_eq!(subtree_end(&lines, 7), 8);
        // non-heading line is its own end.
        assert_eq!(subtree_end(&lines, 1), 1);
    }

    #[test]
    fn cycle_todo_full_cycle_preserves_stars_and_text() {
        let none = "** foo bar";
        let todo = cycle_todo(none);
        assert_eq!(todo, "** TODO foo bar");
        let done = cycle_todo(&todo);
        assert_eq!(done, "** DONE foo bar");
        let back = cycle_todo(&done);
        assert_eq!(back, "** foo bar");
        // single-star heading cycles too.
        assert_eq!(cycle_todo("* x"), "* TODO x");
        // non-heading untouched.
        assert_eq!(cycle_todo("plain text"), "plain text");
    }

    #[test]
    fn promote_demote_clamp_levels() {
        assert_eq!(demote("* a"), "** a");
        assert_eq!(demote("** a"), "*** a");
        assert_eq!(promote("** a"), "* a");
        assert_eq!(promote("*** a"), "** a");
        // level-1 heading cannot be promoted further.
        assert_eq!(promote("* a"), "* a");
        // non-heading lines untouched by either.
        assert_eq!(promote("plain"), "plain");
        assert_eq!(demote("plain"), "plain");
    }
}
