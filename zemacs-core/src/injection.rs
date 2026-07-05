//! Config-driven language-injection registry — a lightweight port of the idea
//! behind JetBrains' IntelliLang "Language Injections" settings.
//!
//! JetBrains does **not** sniff string content to guess a language; its engine is
//! a user-editable table of *injection places* (call-site / annotation / tag →
//! language) plus an optional `value-pattern` regex that filters an
//! already-matched place. We keep that model, and add one extra trigger the user
//! asked for — **content auto-detection** — as an explicitly-configurable rule
//! type (so it can be tuned or turned off) rather than something hardcoded across
//! grammar query files.
//!
//! Rules are expressed in TOML and merged from two scopes (global
//! `~/.zemacs/injections.toml`, then project `.zemacs/injections.toml`), on top
//! of a built-in default set. At language-load time [`generate`] expands the
//! rules that apply to a host grammar into tree-sitter injection-query text,
//! which is appended to that host's `injections.scm`. Reusing tree-sitter for the
//! actual matching means injected regions get highlighting for free and flow
//! through the existing `language_config_at` path.
//!
//! v1 implements the **content** trigger for the languages with known string
//! node shapes (js/ts, python, go, java, c#, kotlin). The `methods`/`arg` fields
//! are parsed so call-site rules can be authored, but query generation for them
//! is a follow-up (call-site rules currently live in the grammar `injections.scm`
//! files).

use serde::Deserialize;

/// A single injection rule (one `[[injection]]` table in `injections.toml`).
#[derive(Debug, Clone, Deserialize)]
pub struct InjectionRule {
    /// Guest language id to inject (e.g. `"sql"`).
    pub language: String,
    /// Host grammar ids this applies to. Empty = every host we have a string
    /// template for.
    #[serde(default)]
    pub hosts: Vec<String>,
    /// Content trigger: inject when a string literal's content matches this
    /// regex (tree-sitter `#match?` semantics — a search, so anchor with `^`).
    #[serde(default)]
    pub content: Option<String>,
    /// Call-site trigger: method names whose string argument is the guest
    /// language. (Parsed; query generation is a follow-up — see module docs.)
    #[serde(default)]
    pub methods: Vec<String>,
    /// Argument index holding the query string for `methods` rules.
    #[serde(default)]
    pub arg: usize,
    /// Allow turning a rule off without deleting it.
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

/// Deserialized `injections.toml` (`[[injection]]` array).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct InjectionConfig {
    #[serde(default, rename = "injection")]
    pub rules: Vec<InjectionRule>,
}

/// Conservative SQL statement classifier used by the default content rule.
/// Unambiguous DML/DDL matches outright; `SELECT` needs real structure (`*`, a
/// column list, or `FROM … + clause keyword`) so plain-English "select X from Y"
/// does not false-positive. Stored tree-sitter-escaped (doubled backslashes) so
/// it embeds verbatim into a generated `#match?` predicate string.
pub const SQL_CONTENT_REGEX: &str = "(?i)^\\s*(INSERT\\s+INTO\\s|UPDATE\\s+\\S+\\s+SET\\s|DELETE\\s+FROM\\s|CREATE\\s+(TABLE|INDEX|VIEW|OR\\s+REPLACE|MATERIALIZED)|ALTER\\s+TABLE\\s|DROP\\s+(TABLE|INDEX|VIEW)\\b|TRUNCATE\\s+TABLE\\s|WITH\\s+\\S+\\s+AS\\s*\\(|SELECT\\s+(\\*|DISTINCT\\s|[\\w.]+\\s*,|[\\s\\S]+?\\sFROM\\s+[\\s\\S]+?\\s(WHERE|JOIN|GROUP\\s+BY|ORDER\\s+BY|HAVING|LIMIT|UNION)\\b))";

/// The built-in rules shipped with the editor. Currently: SQL content
/// auto-detection across the hosts we have string templates for.
pub fn default_rules() -> Vec<InjectionRule> {
    vec![InjectionRule {
        language: "sql".to_string(),
        hosts: Vec::new(),
        content: Some(SQL_CONTENT_REGEX.to_string()),
        methods: Vec::new(),
        arg: 0,
        enabled: true,
    }]
}

/// The tree-sitter capture that binds a host string literal's *inner content*
/// (never the delimiters — capturing the quotes silently fails to register the
/// injection layer) to `@injection.content`. `None` for unknown hosts.
fn string_content_capture(host: &str) -> Option<&'static str> {
    Some(match host {
        "javascript" | "typescript" | "tsx" | "jsx" | "ecma" => {
            "[\n  (string (string_fragment) @injection.content)\n  (template_string (string_fragment) @injection.content)\n ]"
        }
        "python" => "(string (string_content) @injection.content)",
        "go" => {
            "[\n  (interpreted_string_literal (interpreted_string_literal_content) @injection.content)\n  (raw_string_literal (raw_string_literal_content) @injection.content)\n ]"
        }
        "java" => "(string_literal [(string_fragment) (multiline_string_fragment)] @injection.content)",
        "c-sharp" => {
            "[\n  (string_literal (string_literal_content) @injection.content)\n  (raw_string_literal (raw_string_content) @injection.content)\n ]"
        }
        "kotlin" => "(string_literal (string_content) @injection.content)",
        _ => return None,
    })
}

/// Generate tree-sitter injection-query text for `host` from `rules`. Returns an
/// empty string when nothing applies. Appended to the host's `injections.scm`.
pub fn generate(host: &str, rules: &[InjectionRule]) -> String {
    let mut out = String::new();
    for rule in rules {
        if !rule.enabled {
            continue;
        }
        if !rule.hosts.is_empty() && !rule.hosts.iter().any(|h| h == host) {
            continue;
        }
        if let Some(pattern) = &rule.content {
            if let Some(cap) = string_content_capture(host) {
                out.push_str(&format!(
                    "\n; [injection-engine] content rule -> {lang}\n({cap}\n (#match? @injection.content \"{pattern}\")\n (#set! injection.language \"{lang}\"))\n",
                    lang = rule.language,
                ));
            }
        }
        // `methods`/`arg` call-site generation: follow-up (see module docs).
    }
    out
}

/// Merge the built-in defaults with a user `injections.toml` (parsed from
/// `text`), letting the user file append rules. Malformed TOML is ignored (rules
/// must never break language loading).
pub fn merge_user_config(mut rules: Vec<InjectionRule>, text: &str) -> Vec<InjectionRule> {
    match toml::from_str::<InjectionConfig>(text) {
        Ok(cfg) => rules.extend(cfg.rules),
        Err(err) => log::warn!("ignoring malformed injections.toml: {err}"),
    }
    rules
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_content_rule_for_known_hosts() {
        let rules = default_rules();
        for host in ["python", "go", "java", "c-sharp", "kotlin", "typescript"] {
            let q = generate(host, &rules);
            assert!(q.contains("injection.content"), "no capture for {host}");
            assert!(q.contains("injection.language \"sql\""), "no sql set for {host}");
            assert!(q.contains("#match?"), "no content filter for {host}");
        }
        // Unknown host -> nothing.
        assert!(generate("cobol", &rules).is_empty());
    }

    #[test]
    fn host_scoping_and_enabled_flag() {
        let rules = vec![
            InjectionRule {
                language: "sql".into(),
                hosts: vec!["python".into()],
                content: Some("(?i)^SELECT".into()),
                methods: vec![],
                arg: 0,
                enabled: true,
            },
            InjectionRule {
                language: "sql".into(),
                hosts: vec![],
                content: Some("(?i)^INSERT".into()),
                methods: vec![],
                arg: 0,
                enabled: false,
            },
        ];
        assert!(generate("python", &rules).contains("^SELECT"));
        assert!(!generate("go", &rules).contains("^SELECT"), "host scoping ignored");
        assert!(!generate("python", &rules).contains("^INSERT"), "disabled rule emitted");
    }

    #[test]
    fn merge_user_config_appends() {
        let base = default_rules();
        let toml = r#"
[[injection]]
language = "graphql"
hosts = ["typescript"]
content = "(?i)^\\s*(query|mutation)\\s"
"#;
        let merged = merge_user_config(base, toml);
        let q = generate("typescript", &merged);
        assert!(q.contains("graphql"), "user rule not merged");
        assert!(q.contains("sql"), "default rule lost");
    }
}
