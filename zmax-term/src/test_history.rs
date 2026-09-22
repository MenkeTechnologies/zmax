//! A small record of the test runs that have finished, for JetBrains' "Recent
//! Tests": what was run, where, whether it passed, and which tests failed.
//!
//! The file is a tab-separated line per run under the cache directory, newest
//! last, capped at [`MAX_ENTRIES`]. A line that cannot be parsed is skipped
//! rather than discarding the file: a half-written line from a killed process
//! must not cost the user the rest of their history.

use std::path::{Path, PathBuf};

/// How many runs are kept. Old entries fall off the front.
pub const MAX_ENTRIES: usize = 50;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Unix seconds when the run finished.
    pub when: u64,
    pub ok: bool,
    pub cwd: PathBuf,
    pub cmd: String,
    /// The tests the run reported as failed, as `run::failed_tests` read them.
    pub failed: Vec<String>,
}

fn store_path() -> Option<PathBuf> {
    let dir = zmax_loader::cache_dir();
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join("test-history.tsv"))
}

/// A run is worth remembering when it named tests — either the command asks for
/// them or the output reported failures. A plain build is not a test run.
pub fn is_test_run(cmd: &str, failed: &[String]) -> bool {
    !failed.is_empty() || cmd.contains("test") || cmd.contains("spec")
}

pub fn parse(text: &str) -> Vec<Entry> {
    text.lines()
        .filter_map(|line| {
            let mut fields = line.split('\t');
            let when = fields.next()?.parse().ok()?;
            let ok = match fields.next()? {
                "ok" => true,
                "fail" => false,
                _ => return None,
            };
            let cwd = PathBuf::from(fields.next()?);
            let cmd = fields.next()?.to_string();
            let failed = fields
                .next()
                .filter(|s| !s.is_empty())
                .map(|s| s.split('\x1f').map(str::to_string).collect())
                .unwrap_or_default();
            Some(Entry {
                when,
                ok,
                cwd,
                cmd,
                failed,
            })
        })
        .collect()
}

pub fn render(entries: &[Entry]) -> String {
    let mut out = String::new();
    for e in entries {
        out.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\n",
            e.when,
            if e.ok { "ok" } else { "fail" },
            e.cwd.display(),
            // A tab or newline in the command would split the record; the
            // command is rewritten rather than the record escaped, because a
            // command holding a raw tab is not something to re-run verbatim.
            e.cmd.replace(['\t', '\n'], " "),
            e.failed.join("\x1f")
        ));
    }
    out
}

pub fn entries() -> Vec<Entry> {
    store_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|text| parse(&text))
        .unwrap_or_default()
}

/// Append one finished run, keeping the file at [`MAX_ENTRIES`].
pub fn record(cmd: &str, cwd: &Path, ok: bool, failed: Vec<String>) {
    if !is_test_run(cmd, &failed) {
        return;
    }
    let Some(path) = store_path() else {
        return;
    };
    let when = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    let mut all = entries();
    all.push(Entry {
        when,
        ok,
        cwd: cwd.to_path_buf(),
        cmd: cmd.to_string(),
        failed,
    });
    let start = all.len().saturating_sub(MAX_ENTRIES);
    let _ = std::fs::write(path, render(&all[start..]));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(cmd: &str, failed: &[&str]) -> Entry {
        Entry {
            when: 1_700_000_000,
            ok: failed.is_empty(),
            cwd: PathBuf::from("/tmp/project"),
            cmd: cmd.to_string(),
            failed: failed.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn a_run_round_trips_through_the_file_format() {
        let rows = vec![
            entry("cargo test", &[]),
            entry("pytest", &["tests/test_a.py::test_x", "tests/test_b.py::test_y"]),
        ];
        assert_eq!(parse(&render(&rows)), rows);
    }

    #[test]
    fn a_half_written_line_costs_only_itself() {
        let text = format!("{}17000\tnot-a-status\n", render(&[entry("cargo test", &[])]));
        assert_eq!(parse(&text), vec![entry("cargo test", &[])]);
    }

    #[test]
    fn only_runs_that_named_tests_are_remembered() {
        assert!(is_test_run("cargo test -p zmax-term", &[]));
        assert!(is_test_run("npm run spec", &[]));
        // A build that failed a test is still a test run.
        assert!(is_test_run("make", &["TestThing".to_string()]));
        assert!(!is_test_run("cargo build", &[]));
    }
}
