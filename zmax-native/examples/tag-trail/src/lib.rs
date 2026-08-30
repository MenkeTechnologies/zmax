//! Example plugin: where will `CTRL-T` take me back to?
//!
//! [`Host::tag_stack`] is vim `gettagstack()` — the locations tag jumps started
//! FROM, oldest first. `CTRL-T` unwinds it from the end, so the LAST frame is
//! the next return.
//!
//! It is a different history from the jump list (see `nav-history`): the jump
//! list records every jump, while this records only `CTRL-]` and `:tag`. A
//! plugin offering "go back" has to pick which one it means, and they are
//! frequently not the same place.
//!
//! Note `:set notagstack` makes tag jumps record nothing, so an empty stack
//! after jumping is a configuration, not a bug.
//!
//! ```text
//! :plugin load .../libzmax_native_tag_trail.dylib
//! :tags   # → "3 deep · CTRL-T → src/parser.rs · from: main.rs, lib.rs, parser.rs"
//! ```

use std::os::raw::c_int;

use zmax_native::{declare_plugin, Args, Host, TagFrame};

/// The basename of a path, for a report that fits on a status line.
///
/// Done here rather than through `fname_modify(":t")` because that is a host
/// call per frame; the trail is display-only and a split is enough.
fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Where `CTRL-T` goes next: the LAST frame, since the stack unwinds from the
/// end.
fn next_return(frames: &[TagFrame]) -> Option<&TagFrame> {
    frames.last()
}

/// The trail, oldest first — the order the frames were pushed.
fn trail(frames: &[TagFrame]) -> String {
    frames
        .iter()
        .map(|frame| basename(&frame.path))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The summary line.
fn summary(frames: &[TagFrame]) -> String {
    let Some(next) = next_return(frames) else {
        // Distinguishable from a bug only by knowing about 'tagstack', so the
        // hint is worth carrying.
        return "tag stack empty — no CTRL-T target (is 'tagstack' set?)".to_string();
    };
    format!(
        "{} deep · CTRL-T → {} · from: {}",
        frames.len(),
        basename(&next.path),
        trail(frames),
    )
}

/// `:tags` — show the tag stack and where `CTRL-T` would return.
fn tags(host: &Host, _args: &Args) -> c_int {
    host.message(&summary(&host.tag_stack()));
    0
}

declare_plugin! {
    name: "tag-trail",
    version: "0.1.0",
    commands: { "tags" => tags },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(path: &str, pos: usize) -> TagFrame {
        TagFrame {
            path: path.to_string(),
            pos,
        }
    }

    /// The stack unwinds from the END, so `CTRL-T` returns to the newest
    /// frame — taking the first would send the user to the oldest jump.
    #[test]
    fn ctrl_t_returns_to_the_newest_frame() {
        let frames = [
            frame("/p/src/main.rs", 10),
            frame("/p/src/lib.rs", 20),
            frame("/p/src/parser.rs", 30),
        ];
        assert_eq!(next_return(&frames).unwrap().path, "/p/src/parser.rs");
        assert!(summary(&frames).contains("CTRL-T → parser.rs"));
    }

    /// The trail reads oldest first, which is push order and the opposite end
    /// from where CTRL-T starts.
    #[test]
    fn the_trail_reads_oldest_first() {
        let frames = [frame("/p/a.rs", 1), frame("/p/b.rs", 2)];
        assert_eq!(trail(&frames), "a.rs, b.rs");
        assert!(
            summary(&frames).contains("CTRL-T → b.rs"),
            "but returns to the last"
        );
    }

    /// An empty stack carries the 'tagstack' hint, because a user who has
    /// jumped and sees nothing here has a setting, not a fault.
    #[test]
    fn an_empty_stack_hints_at_the_option() {
        let out = summary(&[]);
        assert!(out.contains("empty"));
        assert!(out.contains("tagstack"), "names the likely cause");
    }

    /// Basenames survive paths with no separator and paths ending in one.
    #[test]
    fn basename_handles_awkward_paths() {
        assert_eq!(basename("/a/b/c.rs"), "c.rs");
        assert_eq!(basename("bare.rs"), "bare.rs");
        assert_eq!(basename("/trailing/"), "", "nothing after the last slash");
    }
}
