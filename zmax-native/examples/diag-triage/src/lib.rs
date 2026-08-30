//! Example plugin: triage the language server's diagnostics.
//!
//! [`Host::diagnostics`] returns each diagnostic as one value — where, what,
//! and how bad — rather than as three parallel lists that could fall out of
//! step.
//!
//! Severity arrives as a string: `error`, `warning`, `info` or `hint`. It is
//! deliberately not an integer, so a plugin cannot silently mis-order them the
//! way LSP's numbering does (there, 1 is the MOST severe, which reverses the
//! usual sort and catches people out). Ordering here is explicit.
//!
//! These are NOT the quickfix list — see the `three-lists` example. `:cnext`
//! does not walk them.
//!
//! ```text
//! :plugin load .../libzmax_native_diag_triage.dylib
//! :diags   # → "9: 2 errors, 5 warnings, 2 hints · first error line 42: cannot find value"
//! ```

use std::os::raw::c_int;

use zmax_native::{declare_plugin, Args, DiagnosticInfo, Host};

/// Severity order, worst first. Explicit because the string carries no ordering
/// of its own, and LSP's integers run the other way.
const SEVERITY_ORDER: [&str; 4] = ["error", "warning", "info", "hint"];

/// How bad a severity is, lower being worse. An unrecognised severity sorts
/// last rather than being dropped — a new severity should still be counted.
fn rank(severity: &str) -> usize {
    SEVERITY_ORDER
        .iter()
        .position(|known| *known == severity)
        .unwrap_or(SEVERITY_ORDER.len())
}

/// Plural label for a count, since "1 errors" reads badly on a status line.
fn plural(count: usize, severity: &str) -> String {
    if count == 1 {
        format!("1 {severity}")
    } else {
        format!("{count} {severity}s")
    }
}

/// Counts per severity, worst first, omitting the ones with none.
fn tally(diagnostics: &[DiagnosticInfo]) -> Vec<(String, usize)> {
    SEVERITY_ORDER
        .iter()
        .filter_map(|severity| {
            let count = diagnostics
                .iter()
                .filter(|d| d.severity == *severity)
                .count();
            (count > 0).then(|| (plural(count, severity), count))
        })
        .collect()
}

/// The worst diagnostic, and the earliest one among equals.
///
/// Sorting by severity alone would pick an arbitrary error; tie-breaking on
/// position makes the answer stable and puts the user at the first thing to fix.
fn worst(diagnostics: &[DiagnosticInfo]) -> Option<&DiagnosticInfo> {
    diagnostics
        .iter()
        .min_by_key(|d| (rank(&d.severity), d.span.line, d.span.anchor))
}

/// The summary line.
fn summary(diagnostics: &[DiagnosticInfo]) -> String {
    if diagnostics.is_empty() {
        return "no diagnostics".to_string();
    }
    let counts = tally(diagnostics)
        .into_iter()
        .map(|(label, _count)| label)
        .collect::<Vec<_>>()
        .join(", ");

    match worst(diagnostics) {
        Some(d) => format!(
            "{}: {counts} · first {} line {}: {}",
            diagnostics.len(),
            d.severity,
            d.span.line + 1,
            d.message,
        ),
        None => format!("{}: {counts}", diagnostics.len()),
    }
}

/// `:diags` — triage the current buffer's diagnostics.
fn diags(host: &Host, _args: &Args) -> c_int {
    host.message(&summary(&host.diagnostics()));
    0
}

declare_plugin! {
    name: "diag-triage",
    version: "0.1.0",
    commands: { "diags" => diags },
}

#[cfg(test)]
mod tests {
    use super::*;
    use zmax_native::Span;

    fn diag(severity: &str, line: usize, anchor: usize, message: &str) -> DiagnosticInfo {
        DiagnosticInfo {
            span: Span {
                anchor,
                head: anchor,
                line,
                valid: 1,
            },
            message: message.to_string(),
            severity: severity.to_string(),
        }
    }

    /// Errors outrank warnings outrank hints. The ordering is explicit here
    /// because the severity string carries none, and LSP's integers run the
    /// opposite way.
    #[test]
    fn severity_order_is_explicit_and_worst_first() {
        assert!(rank("error") < rank("warning"));
        assert!(rank("warning") < rank("info"));
        assert!(rank("info") < rank("hint"));
    }

    /// An unknown severity is ranked last rather than dropped — a new one
    /// should still be counted, not silently disappear.
    #[test]
    fn an_unknown_severity_sorts_last_but_survives() {
        assert!(rank("catastrophe") > rank("hint"));
        let diagnostics = [diag("catastrophe", 0, 0, "?"), diag("error", 5, 0, "real")];
        assert_eq!(worst(&diagnostics).unwrap().message, "real");
    }

    /// The worst diagnostic wins on severity first, then on position — so the
    /// answer is stable and points at the first thing to fix.
    #[test]
    fn the_worst_is_the_earliest_of_the_most_severe() {
        let diagnostics = [
            diag("warning", 1, 10, "early but mild"),
            diag("error", 40, 5, "later error"),
            diag("error", 20, 5, "earlier error"),
        ];
        assert_eq!(worst(&diagnostics).unwrap().message, "earlier error");
    }

    /// Counts come back worst-first and omit severities with none.
    #[test]
    fn the_tally_is_worst_first_and_omits_empties() {
        let diagnostics = [
            diag("hint", 1, 0, "h"),
            diag("error", 2, 0, "e"),
            diag("error", 3, 0, "e2"),
        ];
        let labels: Vec<String> = tally(&diagnostics).into_iter().map(|(l, _)| l).collect();
        assert_eq!(labels, vec!["2 errors".to_string(), "1 hint".to_string()]);
    }

    /// Singular and plural both read correctly — "1 errors" on a status line
    /// is the kind of thing that makes a tool feel unfinished.
    #[test]
    fn one_is_singular() {
        assert_eq!(plural(1, "error"), "1 error");
        assert_eq!(plural(2, "error"), "2 errors");
    }

    /// The line is reported 1-based, and a clean buffer says so.
    #[test]
    fn the_report_is_one_based_and_handles_empty() {
        let line = summary(&[diag("error", 41, 0, "cannot find value")]);
        assert!(line.contains("line 42"));
        assert!(line.contains("cannot find value"));
        assert_eq!(summary(&[]), "no diagnostics");
    }
}
