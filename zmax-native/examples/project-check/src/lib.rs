//! Example plugin: can this project actually be built here?
//!
//! Answers the question you ask after cloning something: is the manifest here,
//! and is the tool that reads it on PATH? Combines the filesystem predicates
//! with [`Host::executable`] and [`Host::exepath`].
//!
//! The two are deliberately separate calls. `executable` answers whether a
//! command resolves; `exepath` says WHERE. Reporting the path matters when the
//! wrong one is first on PATH — a project failing against a system toolchain
//! while a newer one sits further down is invisible if you only ask "is it
//! installed".
//!
//! ```text
//! :plugin load .../libzmax_native_project_check.dylib
//! :project   # → "rust: Cargo.toml ✓, cargo ✓ /usr/bin/cargo — ready"
//! ```

use std::os::raw::c_int;

use zmax_native::{declare_plugin, Args, Host};

/// The ecosystems recognised, as (name, manifest, tool).
///
/// Ordered so the more specific manifest wins where a directory holds several:
/// a Rust project with a `package.json` for its docs site is a Rust project.
const ECOSYSTEMS: [(&str, &str, &str); 6] = [
    ("rust", "Cargo.toml", "cargo"),
    ("go", "go.mod", "go"),
    ("node", "package.json", "node"),
    ("python", "pyproject.toml", "python3"),
    ("ruby", "Gemfile", "ruby"),
    ("make", "Makefile", "make"),
];

/// What was found for one ecosystem.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Finding {
    name: &'static str,
    manifest: &'static str,
    tool: &'static str,
    tool_path: Option<String>,
}

/// The verdict for a finding: a manifest with no tool is the actionable case,
/// and is worth naming rather than folding into a generic failure.
fn verdict(finding: &Finding) -> String {
    match &finding.tool_path {
        Some(path) => format!(
            "{}: {} ✓, {} ✓ {path} — ready",
            finding.name, finding.manifest, finding.tool
        ),
        None => format!(
            "{}: {} ✓, but {} is not on PATH — install it",
            finding.name, finding.manifest, finding.tool
        ),
    }
}

/// The whole report. Several ecosystems in one directory is normal, so all of
/// them are reported rather than only the first.
fn report(findings: &[Finding], cwd: &str) -> String {
    if findings.is_empty() {
        return format!("no recognised project manifest in {cwd}");
    }
    findings
        .iter()
        .map(verdict)
        .collect::<Vec<_>>()
        .join("  ·  ")
}

/// `:project` — check each recognised manifest and its tool.
fn project(host: &Host, _args: &Args) -> c_int {
    let Some(cwd) = host.cwd() else {
        host.error("project: no working directory");
        return 1;
    };

    let findings: Vec<Finding> = ECOSYSTEMS
        .iter()
        .filter(|(_name, manifest, _tool)| {
            // Join through the host so the path separator is the editor's, not
            // this plugin's guess at one.
            let path = format!("{}/{}", cwd.trim_end_matches('/'), manifest);
            host.file_readable(&path)
        })
        .map(|(name, manifest, tool)| Finding {
            name,
            manifest,
            tool,
            // `executable` gates the lookup; `exepath` says which one wins.
            tool_path: host.executable(tool).then(|| host.exepath(tool)).flatten(),
        })
        .collect();

    host.message(&report(&findings, &cwd));
    0
}

declare_plugin! {
    name: "project-check",
    version: "0.1.0",
    commands: { "project" => project },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finding(
        name: &'static str,
        manifest: &'static str,
        tool: &'static str,
        path: Option<&str>,
    ) -> Finding {
        Finding {
            name,
            manifest,
            tool,
            tool_path: path.map(str::to_string),
        }
    }

    /// The useful case: the project is here but its toolchain is not. That is
    /// actionable, so it says what to do rather than just failing.
    #[test]
    fn a_manifest_without_its_tool_is_actionable() {
        let missing = verdict(&finding("rust", "Cargo.toml", "cargo", None));
        assert!(missing.contains("not on PATH"));
        assert!(missing.contains("install it"));
        assert!(
            missing.contains("Cargo.toml ✓"),
            "the manifest was still found"
        );
    }

    /// When the tool resolves, WHERE it resolved is reported — the wrong one
    /// first on PATH is invisible otherwise.
    #[test]
    fn a_resolved_tool_reports_its_path() {
        let ready = verdict(&finding("go", "go.mod", "go", Some("/usr/local/bin/go")));
        assert!(ready.contains("/usr/local/bin/go"), "which one won");
        assert!(ready.contains("ready"));
    }

    /// A directory can belong to several ecosystems at once, and all are
    /// reported rather than only the first match.
    #[test]
    fn several_ecosystems_are_all_reported() {
        let findings = [
            finding("rust", "Cargo.toml", "cargo", Some("/usr/bin/cargo")),
            finding("node", "package.json", "node", None),
        ];
        let line = report(&findings, "/tmp/proj");
        assert!(line.contains("rust:"));
        assert!(line.contains("node:"));
    }

    /// A directory with nothing recognisable says so, and says where it looked.
    #[test]
    fn an_unrecognised_directory_names_itself() {
        let line = report(&[], "/tmp/empty");
        assert!(line.contains("no recognised project manifest"));
        assert!(line.contains("/tmp/empty"), "says where it looked");
    }

    /// The more specific manifests are checked first, so a Rust project that
    /// also ships a package.json is reported as Rust first.
    #[test]
    fn manifests_are_ordered_most_specific_first() {
        let rust = ECOSYSTEMS.iter().position(|(n, ..)| *n == "rust").unwrap();
        let node = ECOSYSTEMS.iter().position(|(n, ..)| *n == "node").unwrap();
        let make = ECOSYSTEMS.iter().position(|(n, ..)| *n == "make").unwrap();
        assert!(rust < node, "a Cargo project is Rust, not node");
        assert!(node < make, "a Makefile is the weakest signal");
    }
}
