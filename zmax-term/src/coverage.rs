//! Code coverage from LCOV tracefiles — the JetBrains coverage actions
//! (Import Coverage, Hide Coverage, Switch Coverage, Generate Coverage
//! Report) over the format `cargo llvm-cov`, `grcov`, `c8`, `jest`, `gcovr` and
//! `lcov` itself all write.
//!
//! An imported report is drawn as gutter signs: every instrumented line gets a
//! bar, `diff.plus`-coloured when it ran and `diff.minus` when it did not. The
//! signs live in their own sign group, keyed by file path, so they show in
//! files opened after the import too and hiding the report leaves `:sign`
//! placements alone.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{bail, Context as _};
use zmax_view::signs::{self, PlacedSign, SignDef, DEFAULT_PRIORITY};

const GROUP: &str = "coverage";
const HIT: &str = "coverage_hit";
const MISS: &str = "coverage_miss";

/// One imported tracefile: for each source file, its instrumented 1-based
/// lines and how often each ran.
#[derive(Debug, Default, PartialEq)]
pub struct Report {
    pub source: PathBuf,
    pub files: BTreeMap<PathBuf, BTreeMap<usize, u64>>,
}

impl Report {
    /// `(covered, instrumented)` lines over every file.
    pub fn totals(&self) -> (usize, usize) {
        self.files
            .values()
            .map(line_totals)
            .fold((0, 0), |(c, t), (fc, ft)| (c + fc, t + ft))
    }
}

/// `(covered, instrumented)` lines of one file.
pub fn line_totals(lines: &BTreeMap<usize, u64>) -> (usize, usize) {
    (
        lines.values().filter(|&&hits| hits > 0).count(),
        lines.len(),
    )
}

/// `covered` of `total` as a percentage; an empty report is fully covered.
pub fn percent(covered: usize, total: usize) -> f64 {
    if total == 0 {
        100.0
    } else {
        covered as f64 * 100.0 / total as f64
    }
}

/// Parse an LCOV tracefile. `SF:` paths are resolved against `base` (a
/// relative path is relative to where the tool ran, normally the project
/// root). `DA:<line>,<hits>` records for the same line — one per test in a
/// merged tracefile — add up. Every other record (functions, branches,
/// summaries) is skipped.
pub fn parse_lcov(text: &str, base: &Path) -> Report {
    let mut report = Report::default();
    let mut current: Option<PathBuf> = None;
    for line in text.lines().map(str::trim) {
        if let Some(path) = line.strip_prefix("SF:") {
            let path = zmax_stdx::path::canonicalize(base.join(path));
            report.files.entry(path.clone()).or_default();
            current = Some(path);
        } else if line == "end_of_record" {
            current = None;
        } else if let (Some(file), Some(data)) = (&current, line.strip_prefix("DA:")) {
            let mut fields = data.split(',');
            let line_no = fields.next().and_then(|n| n.trim().parse::<usize>().ok());
            let hits = fields.next().and_then(|n| n.trim().parse::<u64>().ok());
            if let (Some(line_no), Some(hits)) = (line_no, hits) {
                let lines = report.files.entry(file.clone()).or_default();
                *lines.entry(line_no).or_default() += hits;
            }
        }
    }
    report
}

/// Every imported report, oldest first, and the one on screen.
struct State {
    reports: Vec<Report>,
    shown: Option<usize>,
}

static STATE: Mutex<State> = Mutex::new(State {
    reports: Vec::new(),
    shown: None,
});

fn state() -> std::sync::MutexGuard<'static, State> {
    STATE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Read and parse `tracefile`, keep it (replacing an earlier import of the
/// same file) and show it. Returns its index.
pub fn import(tracefile: &Path, base: &Path) -> anyhow::Result<usize> {
    let text = std::fs::read_to_string(tracefile)
        .with_context(|| format!("reading {}", tracefile.display()))?;
    let mut report = parse_lcov(&text, base);
    if report.files.is_empty() {
        bail!("{} has no LCOV source records (SF:)", tracefile.display());
    }
    report.source = zmax_stdx::path::canonicalize(tracefile);
    let index = {
        let mut st = state();
        match st.reports.iter().position(|r| r.source == report.source) {
            Some(i) => {
                st.reports[i] = report;
                i
            }
            None => {
                st.reports.push(report);
                st.reports.len() - 1
            }
        }
    };
    show(index);
    Ok(index)
}

/// Draw report `index` in the gutter, in place of whatever report was shown.
pub fn show(index: usize) -> bool {
    let mut st = state();
    if index >= st.reports.len() {
        return false;
    }
    unplace(&st);
    signs::define(
        HIT,
        SignDef {
            text: "▌".into(),
            texthl: Some("diff.plus".into()),
        },
    );
    signs::define(
        MISS,
        SignDef {
            text: "▌".into(),
            texthl: Some("diff.minus".into()),
        },
    );
    for (path, lines) in &st.reports[index].files {
        for (&line, &hits) in lines {
            let sign = PlacedSign {
                id: line as i64,
                group: GROUP.into(),
                line: line.saturating_sub(1),
                name: if hits > 0 { HIT } else { MISS }.into(),
                // Below vim's default, so a user's own sign wins the line.
                priority: DEFAULT_PRIORITY - 1,
            };
            let _ = signs::place(path, sign);
        }
    }
    st.shown = Some(index);
    true
}

/// Take the shown report out of the gutter. False when none was shown.
pub fn hide() -> bool {
    let mut st = state();
    let was_shown = st.shown.is_some();
    unplace(&st);
    st.shown = None;
    was_shown
}

fn unplace(st: &State) {
    if let Some(report) = st.shown.and_then(|i| st.reports.get(i)) {
        for path in report.files.keys() {
            signs::unplace(path, None, Some(GROUP));
        }
    }
}

/// Every imported report: `(tracefile, covered, instrumented, shown)`.
pub fn reports() -> Vec<(PathBuf, usize, usize, bool)> {
    let st = state();
    st.reports
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let (covered, total) = r.totals();
            (r.source.clone(), covered, total, st.shown == Some(i))
        })
        .collect()
}

/// The shown report, or else the newest import, with its summary.
pub fn current_totals() -> Option<(usize, usize, usize)> {
    let st = state();
    let report = st
        .shown
        .or(st.reports.len().checked_sub(1))
        .and_then(|i| st.reports.get(i))?;
    let (covered, total) = report.totals();
    Some((covered, total, report.files.len()))
}

/// The shown report — or else the newest import — as one HTML page: a table
/// of files with their line coverage, then each file's source with the lines
/// that ran on green and the ones that did not on red. Source files that can
/// no longer be read are listed without a body.
pub fn html() -> Option<String> {
    use zmax_core::sgml::htmlfontify::escape;

    let st = state();
    let report = st
        .shown
        .or(st.reports.len().checked_sub(1))
        .and_then(|i| st.reports.get(i))?;
    let root = zmax_loader::find_workspace().0;
    let name = |p: &Path| p.strip_prefix(&root).unwrap_or(p).display().to_string();
    let row = |label: &str, (covered, total): (usize, usize)| {
        format!(
            "<tr><td>{}</td><td>{:.1}%</td><td>{covered} / {total}</td></tr>\n",
            escape(label),
            percent(covered, total)
        )
    };

    let mut out = format!(
        "<!DOCTYPE html>\n<html>\n<head><meta charset=\"utf-8\"><title>Coverage: {src}</title>\n\
         <style>body{{font-family:sans-serif}} table{{border-collapse:collapse}} \
         td,th{{padding:2px 10px;text-align:left}} pre{{margin:0}} \
         .hit{{background:#dfd}} .miss{{background:#fdd}}</style></head>\n<body>\n\
         <h1>Coverage: {src}</h1>\n<table>\n<tr><th>File</th><th>Lines</th><th>Covered</th></tr>\n",
        src = escape(&name(&report.source)),
    );
    out.push_str(&row("all files", report.totals()));
    for (path, lines) in &report.files {
        out.push_str(&row(&name(path), line_totals(lines)));
    }
    out.push_str("</table>\n");
    for (path, lines) in &report.files {
        out.push_str(&format!("<h2>{}</h2>\n", escape(&name(path))));
        let Ok(text) = std::fs::read_to_string(path) else {
            out.push_str("<p>(source not readable)</p>\n");
            continue;
        };
        for (i, line) in text.lines().enumerate() {
            let class = match lines.get(&(i + 1)) {
                Some(0) => " class=\"miss\"",
                Some(_) => " class=\"hit\"",
                None => "",
            };
            out.push_str(&format!(
                "<pre{class}>{:>5}  {}</pre>\n",
                i + 1,
                escape(line)
            ));
        }
    }
    out.push_str("</body>\n</html>\n");
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LCOV: &str = "TN:\nSF:src/a.rs\nFN:1,main\nDA:1,3\nDA:2,0\nDA:4,1\nLF:3\nLH:2\nend_of_record\n\
                        TN:\nSF:src/a.rs\nDA:2,5\nend_of_record\nSF:/abs/b.rs\nDA:7,0\nend_of_record\n";

    /// Relative `SF:` paths resolve against the base, and a line measured by
    /// two records counts the hits of both.
    #[test]
    fn records_for_one_file_add_up() {
        let report = parse_lcov(LCOV, Path::new("/proj"));
        let a = &report.files[Path::new("/proj/src/a.rs")];
        assert_eq!(
            a.iter().map(|(l, h)| (*l, *h)).collect::<Vec<_>>(),
            [(1, 3), (2, 5), (4, 1)]
        );
        assert_eq!(line_totals(&report.files[Path::new("/abs/b.rs")]), (0, 1));
        assert_eq!(report.totals(), (3, 4));
        assert_eq!(percent(3, 4), 75.0);
    }

    /// Showing a report puts a hit or miss bar on each instrumented line;
    /// switching to another moves the bars, and hiding takes them away while
    /// leaving a `:sign` of the user's own. The HTML report is of the report
    /// on screen.
    #[test]
    fn showing_switching_and_hiding_move_the_gutter_bars() {
        let dir = tempfile::tempdir().unwrap();
        let src = zmax_stdx::path::canonicalize(dir.path().join("lib.rs"));
        let first = dir.path().join("first.info");
        let second = dir.path().join("second.info");
        std::fs::write(&src, "a\nb<\nc\n").unwrap();
        std::fs::write(&first, "SF:lib.rs\nDA:1,2\nDA:3,0\nend_of_record\n").unwrap();
        std::fs::write(&second, "SF:lib.rs\nDA:5,1\nend_of_record\n").unwrap();
        signs::define(
            "mine",
            SignDef {
                text: "M".into(),
                texthl: None,
            },
        );
        signs::place(
            &src,
            PlacedSign {
                id: 1,
                group: String::new(),
                line: 8,
                name: "mine".into(),
                priority: 10,
            },
        )
        .unwrap();
        let bars = || {
            let mut v = signs::line_signs(&src)
                .into_iter()
                .map(|(line, _, hl)| (line, hl.unwrap_or_default()))
                .collect::<Vec<_>>();
            v.sort();
            v
        };

        let a = import(&first, dir.path()).unwrap();
        assert_eq!(
            bars(),
            [
                (0, "diff.plus".into()),
                (2, "diff.minus".into()),
                (8, String::new())
            ]
        );
        let b = import(&second, dir.path()).unwrap();
        assert_eq!(bars(), [(4, "diff.plus".into()), (8, String::new())]);
        assert!(show(a));
        assert_eq!(
            bars(),
            [
                (0, "diff.plus".into()),
                (2, "diff.minus".into()),
                (8, String::new())
            ]
        );
        assert_ne!(a, b);

        let page = html().unwrap();
        assert!(page.contains("<td>50.0%</td>"), "{page}");
        assert!(page.contains("<pre class=\"hit\">    1  a</pre>"), "{page}");
        assert!(page.contains("<pre>    2  b&lt;</pre>"), "{page}");
        assert!(
            page.contains("<pre class=\"miss\">    3  c</pre>"),
            "{page}"
        );

        assert!(hide());
        assert_eq!(bars(), [(8, String::new())]);
        assert!(!hide(), "nothing left to hide");
    }
}
