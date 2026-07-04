//! Pure helpers backing the Emacs `vc` (version-control) command ports.
//!
//! zemacs is git-backed, so these mirror the git plumbing the `vc-*` commands
//! shell out to. Kept filesystem-free and unit-tested; the term layer runs the
//! actual `git` subprocess.

/// `git log` rev-range for `vc-log-outgoing`: commits reachable from `HEAD`
/// that are **not** yet on the upstream tracking branch — i.e. what a `push`
/// would send. Equivalent to Emacs computing the outgoing changesets.
pub fn outgoing_rev_range() -> &'static str {
    "@{u}.."
}

/// `git log` rev-range for `vc-log-incoming`: commits on the upstream tracking
/// branch that are **not** yet reachable from `HEAD` — i.e. what a `pull` would
/// bring in.
pub fn incoming_rev_range() -> &'static str {
    "..@{u}"
}

/// `git log -L <start>,<end>:<file>` line-range spec for `vc-region-history`.
/// `start`/`end` are 1-based inclusive line numbers.
pub fn region_log_spec(start: usize, end: usize, file: &str) -> String {
    let (lo, hi) = if start <= end {
        (start, end)
    } else {
        (end, start)
    };
    format!("-L{lo},{hi}:{file}")
}

/// Append `entry` to the contents of a `.gitignore` file for `vc-ignore`.
///
/// Returns the new file contents to write, or `None` when `entry` is empty or
/// already present as an exact (trimmed) line — so the caller can skip the
/// write. Guarantees the appended entry sits on its own line with a trailing
/// newline, without disturbing existing content.
pub fn gitignore_append(existing: &str, entry: &str) -> Option<String> {
    let entry = entry.trim();
    if entry.is_empty() {
        return None;
    }
    if existing.lines().any(|line| line.trim() == entry) {
        return None;
    }
    let mut out = String::from(existing);
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(entry);
    out.push('\n');
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rev_ranges_match_git_semantics() {
        // Outgoing = HEAD ahead of upstream; incoming = upstream ahead of HEAD.
        assert_eq!(outgoing_rev_range(), "@{u}..");
        assert_eq!(incoming_rev_range(), "..@{u}");
    }

    #[test]
    fn region_spec_is_git_log_L_form() {
        assert_eq!(
            region_log_spec(10, 20, "src/main.rs"),
            "-L10,20:src/main.rs"
        );
        // Order-insensitive: a backwards selection is normalized.
        assert_eq!(region_log_spec(20, 10, "a.txt"), "-L10,20:a.txt");
        assert_eq!(region_log_spec(5, 5, "a.txt"), "-L5,5:a.txt");
    }

    #[test]
    fn gitignore_append_to_empty() {
        assert_eq!(
            gitignore_append("", "target/"),
            Some("target/\n".to_string())
        );
    }

    #[test]
    fn gitignore_append_adds_missing_trailing_newline() {
        assert_eq!(
            gitignore_append("*.log", "target/"),
            Some("*.log\ntarget/\n".to_string())
        );
    }

    #[test]
    fn gitignore_append_preserves_existing_content() {
        assert_eq!(
            gitignore_append("*.log\nbuild/\n", "target/"),
            Some("*.log\nbuild/\ntarget/\n".to_string())
        );
    }

    #[test]
    fn gitignore_append_skips_duplicate() {
        assert_eq!(gitignore_append("*.log\ntarget/\n", "target/"), None);
        // Whitespace-insensitive duplicate detection.
        assert_eq!(gitignore_append("  target/  \n", "target/"), None);
    }

    #[test]
    fn gitignore_append_rejects_empty_entry() {
        assert_eq!(gitignore_append("*.log\n", "   "), None);
    }
}
