//! proced — the pure, I/O-free substrate for the zemacs port of GNU Emacs
//! `proced` (a process viewer/manager in the spirit of `top`).
//!
//! This module owns everything that can be unit-tested without touching the
//! process table: parsing the output of
//! `ps -axo pid,ppid,user,pcpu,pmem,comm` into [`Proc`] rows, sorting a slice of
//! them by a [`Sort`] key, and filtering by a case-insensitive needle. The
//! interactive overlay (running `ps`/`kill`, rendering, key handling) lives in
//! `zemacs-term/src/ui/proced.rs` and calls straight into here.

use std::cmp::Ordering;

/// A single process, one row of the `ps` table.
#[derive(Debug, Clone, PartialEq)]
pub struct Proc {
    pub pid: u32,
    pub ppid: u32,
    pub user: String,
    pub cpu: f32,
    pub mem: f32,
    pub comm: String,
}

/// The column a process list is ordered by. `Cpu`/`Mem` sort descending (the
/// busiest first, like `top`); `Pid` ascending; `User`/`Comm` lexically.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sort {
    Pid,
    Cpu,
    Mem,
    User,
    Comm,
}

impl Sort {
    /// A short human label for the header line.
    pub fn label(self) -> &'static str {
        match self {
            Sort::Pid => "pid",
            Sort::Cpu => "%cpu",
            Sort::Mem => "%mem",
            Sort::User => "user",
            Sort::Comm => "comm",
        }
    }

    /// The next key when cycling with `s` (pid → cpu → mem → user → comm → pid).
    pub fn next(self) -> Sort {
        match self {
            Sort::Pid => Sort::Cpu,
            Sort::Cpu => Sort::Mem,
            Sort::Mem => Sort::User,
            Sort::User => Sort::Comm,
            Sort::Comm => Sort::Pid,
        }
    }
}

/// Parse a single data row into a [`Proc`], or `None` if it is blank or
/// malformed. The first five whitespace-separated fields are pid, ppid, user,
/// %cpu and %mem; everything after the fifth field (which may itself contain
/// spaces, e.g. `Google Chrome Helper`) is the command.
fn parse_line(line: &str) -> Option<Proc> {
    let mut rest = line.trim();
    if rest.is_empty() {
        return None;
    }
    // Peel off exactly five leading fields, keeping the remainder intact so a
    // multi-word command survives.
    let mut fields: [&str; 5] = [""; 5];
    for slot in fields.iter_mut() {
        rest = rest.trim_start();
        let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        if end == 0 {
            return None; // ran out of fields before comm
        }
        *slot = &rest[..end];
        rest = &rest[end..];
    }
    let comm = rest.trim();
    if comm.is_empty() {
        return None;
    }
    Some(Proc {
        pid: fields[0].parse().ok()?,
        ppid: fields[1].parse().ok()?,
        user: fields[2].to_string(),
        cpu: fields[3].parse().ok()?,
        mem: fields[4].parse().ok()?,
        comm: comm.to_string(),
    })
}

/// Parse the whole output of `ps -axo pid,ppid,user,pcpu,pmem,comm`. The first
/// line (the `ps` header) is skipped; blank and malformed rows are dropped.
pub fn parse_ps(output: &str) -> Vec<Proc> {
    output.lines().skip(1).filter_map(parse_line).collect()
}

/// Sort `procs` in place by `by` (see [`Sort`] for direction per key).
pub fn sort_procs(procs: &mut [Proc], by: Sort) {
    match by {
        Sort::Pid => procs.sort_by(|a, b| a.pid.cmp(&b.pid)),
        Sort::Cpu => procs.sort_by(|a, b| b.cpu.partial_cmp(&a.cpu).unwrap_or(Ordering::Equal)),
        Sort::Mem => procs.sort_by(|a, b| b.mem.partial_cmp(&a.mem).unwrap_or(Ordering::Equal)),
        Sort::User => procs.sort_by(|a, b| a.user.cmp(&b.user)),
        Sort::Comm => procs.sort_by(|a, b| a.comm.cmp(&b.comm)),
    }
}

/// Return the processes whose user or command contains `needle`
/// (case-insensitive). An empty needle matches everything.
pub fn filter<'a>(procs: &'a [Proc], needle: &str) -> Vec<&'a Proc> {
    let needle = needle.to_lowercase();
    if needle.is_empty() {
        return procs.iter().collect();
    }
    procs
        .iter()
        .filter(|p| {
            p.user.to_lowercase().contains(&needle) || p.comm.to_lowercase().contains(&needle)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
  PID  PPID USER      %CPU %MEM COMM
    1     0 root       0.0  0.1 /sbin/launchd
  501     1 alice      2.5  1.2 Google Chrome Helper
  842   501 alice     10.0  3.4 rustc
";

    #[test]
    fn parses_rows_including_a_command_with_spaces() {
        let procs = parse_ps(SAMPLE);
        assert_eq!(procs.len(), 3);
        assert_eq!(procs[0].pid, 1);
        assert_eq!(procs[0].ppid, 0);
        assert_eq!(procs[0].user, "root");
        assert!((procs[0].cpu - 0.0).abs() < f32::EPSILON);
        assert!((procs[0].mem - 0.1).abs() < 1e-6);
        assert_eq!(procs[0].comm, "/sbin/launchd");
        // Multi-word command keeps its spaces.
        assert_eq!(procs[1].comm, "Google Chrome Helper");
        assert_eq!(procs[1].pid, 501);
    }

    #[test]
    fn header_line_is_skipped() {
        let procs = parse_ps(SAMPLE);
        assert!(procs.iter().all(|p| p.user != "USER"));
        assert!(procs.iter().all(|p| p.comm != "COMM"));
    }

    #[test]
    fn sort_by_cpu_is_descending() {
        let mut procs = parse_ps(SAMPLE);
        sort_procs(&mut procs, Sort::Cpu);
        assert_eq!(procs[0].comm, "rustc"); // 10.0 %cpu leads
        assert!(procs[0].cpu >= procs[1].cpu && procs[1].cpu >= procs[2].cpu);
    }

    #[test]
    fn sort_by_pid_is_ascending() {
        let mut procs = parse_ps(SAMPLE);
        sort_procs(&mut procs, Sort::Pid);
        assert_eq!(procs[0].pid, 1);
        assert!(procs[0].pid <= procs[1].pid && procs[1].pid <= procs[2].pid);
    }

    #[test]
    fn sort_by_user_is_lexical() {
        let mut procs = parse_ps(SAMPLE);
        sort_procs(&mut procs, Sort::User);
        assert_eq!(procs[0].user, "alice");
        assert_eq!(procs[2].user, "root");
    }

    #[test]
    fn filter_matches_command_substring_case_insensitively() {
        let procs = parse_ps(SAMPLE);
        let hits = filter(&procs, "chrome");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].pid, 501);
    }

    #[test]
    fn filter_matches_user_and_empty_needle_matches_all() {
        let procs = parse_ps(SAMPLE);
        assert_eq!(filter(&procs, "alice").len(), 2);
        assert_eq!(filter(&procs, "").len(), procs.len());
    }

    #[test]
    fn parse_tolerates_blank_and_malformed_rows() {
        let text = "HEADER LINE HERE\n\n   \nnot a valid row\n123 45 bob 1.0 2.0 bash\n";
        let procs = parse_ps(text);
        assert_eq!(procs.len(), 1);
        assert_eq!(procs[0].pid, 123);
        assert_eq!(procs[0].comm, "bash");
    }
}
