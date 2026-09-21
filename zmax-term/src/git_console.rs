//! The VCS console: a record of the git commands the editor has run.
//!
//! JetBrains keeps a "Console" tab beside the Version Control tool window
//! holding every git invocation it made, with its exit status. It is what you
//! read when the IDE did something to the repository and you want to know
//! exactly what — the same question a terminal editor has to answer, since
//! `:magit`, the hunk commands and the log view all shell out to git.
//!
//! The recorder is a bounded ring: git runs often and a session is long, so
//! only the most recent [`CAPACITY`] invocations are kept. Recording happens in
//! the three helpers every git call goes through, so a new command is in the
//! console without its author doing anything.

use std::sync::Mutex;

/// How many invocations to keep. A few hundred covers a working session; the
/// oldest fall off the front.
const CAPACITY: usize = 500;

/// One recorded git invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitRun {
    /// The argv after `git`, joined for display (`diff HEAD`).
    pub args: String,
    /// The directory it ran in.
    pub dir: String,
    /// Whether git exited zero.
    pub ok: bool,
}

static RUNS: Mutex<Vec<GitRun>> = Mutex::new(Vec::new());

/// Record one invocation. Never panics and never blocks the caller for long:
/// a poisoned lock means the console loses an entry, which must not take the
/// git command down with it.
pub fn record(dir: &std::path::Path, args: &[&str], ok: bool) {
    let Ok(mut runs) = RUNS.lock() else {
        return;
    };
    if runs.len() >= CAPACITY {
        runs.remove(0);
    }
    runs.push(GitRun {
        args: args.join(" "),
        dir: dir.display().to_string(),
        ok,
    });
}

/// Everything recorded so far, oldest first.
pub fn entries() -> Vec<GitRun> {
    RUNS.lock().map(|runs| runs.clone()).unwrap_or_default()
}

/// Drop the record (JetBrains' console has a clear button).
pub fn clear() -> usize {
    match RUNS.lock() {
        Ok(mut runs) => {
            let n = runs.len();
            runs.clear();
            n
        }
        Err(_) => 0,
    }
}

/// The console as text: one line per invocation, failures marked. Pure — unit
/// tested.
pub fn render(runs: &[GitRun]) -> String {
    if runs.is_empty() {
        return "no git commands run yet\n".to_string();
    }
    let mut out = String::new();
    for run in runs {
        // The marker goes first so failures line up down the left edge.
        out.push_str(if run.ok { "  " } else { "! " });
        out.push_str("git ");
        out.push_str(&run.args);
        out.push_str("    [");
        out.push_str(&run.dir);
        out.push_str("]\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(args: &str, ok: bool) -> GitRun {
        GitRun {
            args: args.to_string(),
            dir: "/repo".to_string(),
            ok,
        }
    }

    #[test]
    fn render_marks_failures_and_keeps_order() {
        let text = render(&[run("status --short", true), run("push origin", false)]);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[0], "  git status --short    [/repo]");
        assert_eq!(lines[1], "! git push origin    [/repo]");
    }

    #[test]
    fn render_says_so_when_nothing_ran() {
        assert_eq!(render(&[]), "no git commands run yet\n");
    }

    #[test]
    fn the_ring_keeps_the_most_recent() {
        clear();
        for i in 0..(CAPACITY + 10) {
            record(std::path::Path::new("/repo"), &["log", &i.to_string()], true);
        }
        let got = entries();
        assert_eq!(got.len(), CAPACITY, "the ring is bounded");
        assert_eq!(
            got.last().unwrap().args,
            format!("log {}", CAPACITY + 9),
            "and holds the newest"
        );
        clear();
    }
}
