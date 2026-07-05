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
//! Two triggers are generated today, for the languages with known node shapes
//! (js/ts, python, go, java, c#, kotlin): **content** (a `#match?` regex on a
//! string's inner text) and **call-site** (`methods`/`arg` — inject the Nth
//! string argument of a call to one of these methods, IntelliLang's core
//! trigger). The built-in SQL content-sniff and the JDBC/JPA/ADO.NET/DB-API
//! query-method rules ship as defaults here rather than in the grammar
//! `injections.scm` files. `annotation`/`tag` triggers and `prefix`/`suffix`
//! wrapping are the next increment.

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
    /// Comment-hint trigger: inject `language` into a string immediately
    /// preceded by a `language=<lang>` / `@Language("<lang>")` comment (the
    /// JetBrains manual-injection idiom). Works for block-comment hosts.
    #[serde(default)]
    pub hint: bool,
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

/// A contiguous injected fragment viewed as a virtual document: its guest
/// language + text, plus the bidirectional mapping to host coordinates. This is
/// the single-range analogue of IntelliJ's `DocumentWindow` — the foundation for
/// running the guest language's tools (LSP, formatting) over the fragment and
/// translating positions/edits back to the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FragmentView {
    /// Guest language id (e.g. `"sql"`).
    pub language: String,
    /// Host char offset where the fragment begins.
    pub host_start: usize,
    /// The fragment's text (the injected content).
    pub text: String,
}

impl FragmentView {
    pub fn new(language: impl Into<String>, host_start: usize, text: impl Into<String>) -> Self {
        Self {
            language: language.into(),
            host_start,
            text: text.into(),
        }
    }

    /// Length of the fragment in chars.
    pub fn len(&self) -> usize {
        self.text.chars().count()
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Host char offset for a fragment char offset.
    pub fn to_host(&self, frag: usize) -> usize {
        self.host_start + frag
    }

    /// Fragment char offset for a host char offset, or `None` if the host offset
    /// is outside this fragment. The end boundary is inclusive so a cursor at the
    /// fragment's end still maps.
    pub fn from_host(&self, host: usize) -> Option<usize> {
        (self.host_start..=self.host_start + self.len())
            .contains(&host)
            .then(|| host - self.host_start)
    }

    /// The fragment's char range in the host document.
    pub fn host_range(&self) -> std::ops::Range<usize> {
        self.host_start..self.host_start + self.len()
    }
}

/// The language-injection engine: the active rule set plus the operations over
/// it. One is owned by the syntax [`Loader`](crate::syntax::Loader); it expands
/// the rules that apply to a host grammar into injection-query text at compile
/// time, and backs the `:injections` inspection commands.
#[derive(Debug, Clone)]
pub struct InjectionEngine {
    rules: Vec<InjectionRule>,
}

impl Default for InjectionEngine {
    fn default() -> Self {
        Self::with_rules(default_rules())
    }
}

impl InjectionEngine {
    /// Engine over an explicit rule set (built-ins live in [`default_rules`]).
    pub fn with_rules(rules: Vec<InjectionRule>) -> Self {
        Self { rules }
    }

    /// Built-in defaults, then the global `injections.toml` under `config_dir`,
    /// then the project `injections.toml` under `workspace/.zemacs` (later scopes
    /// append). Missing/malformed files are ignored so injection config can never
    /// break language loading.
    pub fn load(config_dir: &std::path::Path, workspace: &std::path::Path) -> Self {
        let mut rules = default_rules();
        let paths = [
            config_dir.join("injections.toml"),
            workspace.join(".zemacs").join("injections.toml"),
        ];
        for path in paths {
            if let Ok(text) = std::fs::read_to_string(&path) {
                rules = merge_user_config(rules, &text);
            }
        }
        Self { rules }
    }

    /// The active rules (built-in + user), for inspection commands.
    pub fn rules(&self) -> &[InjectionRule] {
        &self.rules
    }

    /// Injection-query text for `host` (see [`generate`]).
    pub fn generate(&self, host: &str) -> String {
        generate(host, &self.rules)
    }

    /// A one-line human description of each rule, for `:injections`.
    pub fn describe(&self) -> Vec<String> {
        self.rules
            .iter()
            .map(|r| {
                let trigger = if r.content.is_some() {
                    "content".to_string()
                } else if !r.methods.is_empty() {
                    format!("methods[{}] arg {}", r.methods.join(","), r.arg)
                } else {
                    "—".to_string()
                };
                let hosts = if r.hosts.is_empty() {
                    "*".to_string()
                } else {
                    r.hosts.join(",")
                };
                let off = if r.enabled { "" } else { " (disabled)" };
                format!("-> {:8} [{hosts}] {trigger}{off}", r.language)
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Content classifiers.
//
// These are the "sniffers": conservative regexes that recognise a language from
// a string's inner content, used as the built-in content rules. They are stored
// as normal regexes (single backslashes) — [`generate`] doubles backslashes when
// embedding them into a tree-sitter `#match?` predicate. Each is tuned against a
// positive + negative battery (see the integration tests) to keep false
// positives on prose/JS-object/placeholder strings low. All are user-tunable and
// can be disabled per rule in `injections.toml`.

/// SQL: unambiguous DML/DDL matches outright; `SELECT` needs real structure
/// (`*`, a column list, or `FROM … + clause keyword`) so plain-English
/// "select X from Y" does not false-positive.
pub const SQL_CONTENT_REGEX: &str = r"(?i)^\s*(INSERT\s+INTO\s|UPDATE\s+\S+\s+SET\s|DELETE\s+FROM\s|CREATE\s+(TABLE|INDEX|VIEW|OR\s+REPLACE|MATERIALIZED)|ALTER\s+TABLE\s|DROP\s+(TABLE|INDEX|VIEW)\b|TRUNCATE\s+TABLE\s|WITH\s+\S+\s+AS\s*\(|SELECT\s+(\*|DISTINCT\s|[\w.]+\s*,|[\s\S]+?\sFROM\s+[\s\S]+?\s(WHERE|JOIN|GROUP\s+BY|ORDER\s+BY|HAVING|LIMIT|UNION)\b))";

/// JSON: an object opening with a quoted key (`{"k":`) or an array opening with
/// a JSON value. Requires the quote/value so a bare `{ … }` JS-object or
/// `{placeholder}` string is not caught.
pub const JSON_CONTENT_REGEX: &str = r#"^\s*(\{\s*"[^"]*"\s*:|\[\s*[\{"\-0-9])"#;

/// HTML/XML: a DOCTYPE, XML/PI declaration, comment, or a real element (a tag
/// with a matching close tag). Requiring `</tag` avoids catching `<3` or a lone
/// `<placeholder>` string.
pub const HTML_CONTENT_REGEX: &str =
    r"^\s*(<!DOCTYPE|<\?xml|<!--|<[a-zA-Z][\w-]*[\s\S]*</[a-zA-Z])";

/// CSS: a selector block containing at least one `prop: value` declaration.
/// Requiring the `{ … : … }` shape keeps prose and single words out; a bare
/// `color: red` fragment is intentionally *not* matched (too ambiguous).
pub const CSS_CONTENT_REGEX: &str =
    r#"(?s)^\s*[.#*:\[]?[\w\-][\w\s.#>+~:,\[\]='"()\-]*\{\s*[a-zA-Z\-]+\s*:\s*[^;{}]+"#;

/// GraphQL: an operation with a proper header + selection brace
/// (`query [Name] [(args)] {`), a `fragment X on Y`, or an SDL definition
/// (`type X {`). Requiring at most one identifier before `{` keeps prose like
/// "query the results { … }" out (two words before the brace won't match).
pub const GRAPHQL_CONTENT_REGEX: &str = r"(?i)^\s*((query|mutation|subscription)\s*(\w+\s*)?(\([\s\S]*?\)\s*)?\{|fragment\s+\w+\s+on\s+\w|(type|input|interface|enum|union|schema)\s+\w+\s*\{)";

/// The built-in rules shipped with the editor: SQL content auto-detection plus
/// the SQL call-site rules (JDBC/JPA/ADO.NET/Dapper/DB-API/… query methods).
pub fn default_rules() -> Vec<InjectionRule> {
    let sql = |hosts: &[&str], methods: &[&str], arg: usize| InjectionRule {
        language: "sql".to_string(),
        hosts: hosts.iter().map(|s| s.to_string()).collect(),
        content: None,
        methods: methods.iter().map(|s| s.to_string()).collect(),
        arg,
        hint: false,
        enabled: true,
    };
    let content = |language: &str, regex: &str| InjectionRule {
        language: language.to_string(),
        hosts: Vec::new(),
        content: Some(regex.to_string()),
        methods: Vec::new(),
        arg: 0,
        hint: false,
        enabled: true,
    };
    // Comment-hint rule: `/* language=<lang> */ "…"` forces <lang>.
    let hint = |language: &str| InjectionRule {
        language: language.to_string(),
        hosts: Vec::new(),
        content: None,
        methods: Vec::new(),
        arg: 0,
        hint: true,
        enabled: true,
    };
    let ecma = &["javascript", "typescript", "tsx", "jsx"][..];
    vec![
        // Content auto-detection (sniffers) across every host with a string
        // template. Conservative classifiers; tunable/disable-able via TOML.
        content("sql", SQL_CONTENT_REGEX),
        content("json", JSON_CONTENT_REGEX),
        content("html", HTML_CONTENT_REGEX),
        content("graphql", GRAPHQL_CONTENT_REGEX),
        content("css", CSS_CONTENT_REGEX),
        // Manual comment-hint injection: `/* language=<lang> */ "…"`.
        hint("sql"),
        hint("json"),
        hint("html"),
        hint("xml"),
        hint("css"),
        hint("graphql"),
        hint("javascript"),
        hint("typescript"),
        hint("python"),
        hint("bash"),
        hint("regex"),
        hint("yaml"),
        hint("toml"),
        hint("rust"),
        hint("go"),
        // Call-site rules (the string argument of a query method is SQL).
        sql(ecma, &["query", "execute", "prepare", "raw"], 0),
        sql(
            &["python"],
            &["execute", "executemany", "executescript", "execute_batch"],
            0,
        ),
        sql(
            &["go"],
            &[
                "Query",
                "QueryRow",
                "Exec",
                "Prepare",
                "NamedExec",
                "NamedQuery",
                "MustExec",
            ],
            0,
        ),
        sql(
            &["go"],
            &[
                "QueryContext",
                "QueryRowContext",
                "ExecContext",
                "PrepareContext",
            ],
            1,
        ),
        sql(
            &["java"],
            &[
                "executeQuery",
                "executeUpdate",
                "execute",
                "prepareStatement",
                "prepareCall",
                "createQuery",
                "createNativeQuery",
                "query",
                "queryForObject",
                "queryForList",
                "queryForMap",
                "update",
                "batchUpdate",
            ],
            0,
        ),
        sql(
            &["c-sharp"],
            &[
                "ExecuteReader",
                "ExecuteNonQuery",
                "ExecuteScalar",
                "ExecuteReaderAsync",
                "ExecuteNonQueryAsync",
                "ExecuteScalarAsync",
                "FromSqlRaw",
                "FromSqlInterpolated",
                "ExecuteSqlRaw",
                "Query",
                "QueryAsync",
                "QueryFirst",
                "QueryFirstOrDefault",
                "QuerySingle",
                "Execute",
                "ExecuteAsync",
            ],
            0,
        ),
        sql(
            &["kotlin"],
            &[
                "rawQuery",
                "execSQL",
                "query",
                "prepareStatement",
                "createStatement",
                "createQuery",
                "createNativeQuery",
            ],
            0,
        ),
    ]
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

/// The inner-content capture for a *single* string node of `host` (no
/// alternation wrapper), for use as the argument in a call-site rule. `None` for
/// unknown hosts. Returns a template that already binds `@injection.content`.
fn arg_string_capture(host: &str) -> Option<&'static str> {
    Some(match host {
        "javascript" | "typescript" | "tsx" | "jsx" | "ecma" => {
            "[(string (string_fragment) @injection.content) (template_string (string_fragment) @injection.content)]"
        }
        "python" => "(string (string_content) @injection.content)",
        "go" => {
            "[(raw_string_literal (raw_string_literal_content) @injection.content) (interpreted_string_literal (interpreted_string_literal_content) @injection.content)]"
        }
        "java" => "(string_literal [(string_fragment) (multiline_string_fragment)] @injection.content)",
        "c-sharp" => {
            "(argument [(string_literal (string_literal_content) @injection.content) (raw_string_literal (raw_string_content) @injection.content)])"
        }
        "kotlin" => "(value_argument (string_literal (string_content) @injection.content))",
        _ => return None,
    })
}

/// The comment node pattern for `host` (some grammars split line/block).
fn comment_node(host: &str) -> &'static str {
    match host {
        "java" => "[(line_comment) (block_comment)]",
        "kotlin" => "[(line_comment) (multiline_comment)]",
        _ => "(comment)",
    }
}

/// Generate a comment-hint rule for `host`: inject `lang` into a string that is
/// immediately preceded by a `language=<lang>` / `@Language("<lang>")` comment
/// (mirrors the built-in `/* GraphQL */` idiom, generalised). `None` for hosts
/// without a string template.
fn hint_query(host: &str, lang: &str) -> Option<String> {
    let cap = string_content_capture(host)?;
    let comment = comment_node(host);
    // Match `language=lang`, `language: lang`, `@Language("lang"`, `#lang`.
    let raw = format!(
        r#"(?i)(language\s*[=:]\s*|@language\s*\(\s*["']?|^#\s*){}\b"#,
        lang
    );
    let esc = raw.replace('\\', "\\\\").replace('"', "\\\"");

    // Python has no inline block comment, so a `# language=…` hint sits on the
    // line *before* the assignment rather than immediately before the string.
    let pattern = if host == "python" {
        format!("((({comment}) @_hint) . (expression_statement (assignment right: {cap}))")
    } else {
        format!("((({comment}) @_hint) . {cap}")
    };
    Some(format!(
        "\n; [injection-engine] comment-hint rule -> {lang}\n{pattern}\n (#match? @_hint \"{esc}\")\n (#set! injection.language \"{lang}\"))\n"
    ))
}

/// Generate a call-site (method-call) injection rule for `host`: inject `lang`
/// into the `arg`-th string argument of a call whose method name is one of
/// `methods`. Encodes each grammar's call/selector/string shape. `None` when the
/// host isn't supported.
fn method_query(host: &str, methods: &[String], arg: usize, lang: &str) -> Option<String> {
    if methods.is_empty() {
        return None;
    }
    let cap = arg_string_capture(host)?;
    // Anchor the argument position: `.` = first named child, `(_) .` = second, …
    let anchor = format!("{}.", "(_) ".repeat(arg));
    let anyof = methods
        .iter()
        .map(|m| format!("\"{m}\""))
        .collect::<Vec<_>>()
        .join(" ");

    let body = match host {
        "javascript" | "typescript" | "tsx" | "jsx" | "ecma" => format!(
            "((call_expression\n  function: (member_expression property: (property_identifier) @_m)\n  arguments: (arguments {anchor} {cap}))\n (#any-of? @_m {anyof})\n (#set! injection.language \"{lang}\"))\n"
        ),
        "python" => format!(
            "((call\n  function: (attribute attribute: (identifier) @_m)\n  arguments: (argument_list {anchor} {cap}))\n (#any-of? @_m {anyof})\n (#set! injection.language \"{lang}\"))\n"
        ),
        "java" => format!(
            "((method_invocation\n  name: (identifier) @_m\n  arguments: (argument_list {anchor} {cap}))\n (#any-of? @_m {anyof})\n (#set! injection.language \"{lang}\"))\n"
        ),
        "c-sharp" => format!(
            "((invocation_expression\n  (member_access_expression name: (identifier) @_m)\n  (argument_list {anchor} {cap}))\n (#any-of? @_m {anyof})\n (#set! injection.language \"{lang}\"))\n"
        ),
        "kotlin" => format!(
            "((call_expression\n  (navigation_expression (navigation_suffix (simple_identifier) @_m))\n  (call_suffix (value_arguments {anchor} {cap})))\n (#any-of? @_m {anyof})\n (#set! injection.language \"{lang}\"))\n"
        ),
        "go" => {
            // Go's method call is matched positionally on the full selector text
            // (the field-labelled form is fragile in this grammar).
            let alt = methods.join("|");
            let re = format!("\\.({alt})$").replace('\\', "\\\\");
            format!(
                "((call_expression\n  (selector_expression) @_fn\n  (argument_list {anchor} {cap}))\n (#match? @_fn \"{re}\")\n (#set! injection.language \"{lang}\"))\n"
            )
        }
        _ => return None,
    };
    Some(format!(
        "\n; [injection-engine] call-site rule -> {lang}\n{body}"
    ))
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
                // The rule holds a normal regex (single backslashes). A
                // tree-sitter query string unescapes `\\` -> `\`, so double the
                // backslashes (and escape quotes) when embedding into `#match?`.
                let escaped = pattern.replace('\\', "\\\\").replace('"', "\\\"");
                out.push_str(&format!(
                    "\n; [injection-engine] content rule -> {lang}\n({cap}\n (#match? @injection.content \"{escaped}\")\n (#set! injection.language \"{lang}\"))\n",
                    lang = rule.language,
                ));
            }
        }
        if let Some(q) = method_query(host, &rule.methods, rule.arg, &rule.language) {
            out.push_str(&q);
        }
        if rule.hint {
            if let Some(q) = hint_query(host, &rule.language) {
                out.push_str(&q);
            }
        }
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
            assert!(
                q.contains("injection.language \"sql\""),
                "no sql set for {host}"
            );
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
                hint: false,
                enabled: true,
            },
            InjectionRule {
                language: "sql".into(),
                hosts: vec![],
                content: Some("(?i)^INSERT".into()),
                methods: vec![],
                arg: 0,
                hint: false,
                enabled: false,
            },
        ];
        assert!(generate("python", &rules).contains("^SELECT"));
        assert!(
            !generate("go", &rules).contains("^SELECT"),
            "host scoping ignored"
        );
        assert!(
            !generate("python", &rules).contains("^INSERT"),
            "disabled rule emitted"
        );
    }

    #[test]
    fn generates_call_site_rules() {
        let rules = default_rules();
        // Go uses positional selector-text #match; others use #any-of? on the name.
        let go = generate("go", &rules);
        assert!(go.contains("call-site rule"), "no go call-site rule");
        assert!(go.contains("QueryContext"), "missing go context method");
        assert!(
            go.contains("(_) ."),
            "arg=1 anchor missing for go context methods"
        );
        let java = generate("java", &rules);
        assert!(
            java.contains("method_invocation"),
            "java call-site shape wrong"
        );
        assert!(
            java.contains("createNativeQuery"),
            "java method set incomplete"
        );
    }

    #[test]
    fn user_method_rule_generates() {
        let toml = r#"
[[injection]]
language = "sql"
hosts = ["python"]
methods = ["myCustomQuery"]
arg = 0
"#;
        let merged = merge_user_config(Vec::new(), toml);
        let q = generate("python", &merged);
        assert!(
            q.contains("myCustomQuery"),
            "user call-site method not generated"
        );
        assert!(
            q.contains("(attribute attribute: (identifier) @_m)"),
            "python call shape wrong"
        );
    }

    #[test]
    fn generates_hint_rules() {
        let rules = default_rules();
        // ecma: adjacent `/* language=sql */ "…"` comment-hint form.
        let ecma = generate("typescript", &rules);
        assert!(ecma.contains("comment-hint rule"), "no ecma hint rule");
        assert!(ecma.contains("(comment)"), "ecma hint missing comment node");
        assert!(ecma.contains("@_hint"), "ecma hint missing capture");
        assert!(
            ecma.contains("#match? @_hint"),
            "hint missing content match"
        );
        assert!(ecma.contains("language"), "hint regex missing 'language'");
        // python: the hint sits on the line BEFORE the assignment.
        let py = generate("python", &rules);
        assert!(
            py.contains("expression_statement (assignment right:"),
            "python hint must use prev-line assignment form"
        );
        // java: split comment node.
        let java = generate("java", &rules);
        assert!(
            java.contains("[(line_comment) (block_comment)]"),
            "java comment node"
        );
    }

    #[test]
    fn fragment_view_edges() {
        // Empty fragment.
        let e = FragmentView::new("json", 10, "");
        assert!(e.is_empty());
        assert_eq!(e.len(), 0);
        assert_eq!(e.host_range(), 10..10);
        assert_eq!(e.from_host(10), Some(0)); // start==end, still maps
        assert_eq!(e.from_host(9), None);
        assert_eq!(e.from_host(11), None);
        // Multi-byte chars are counted as chars, not bytes.
        let u = FragmentView::new("sql", 3, "héllo"); // 5 chars
        assert_eq!(u.len(), 5);
        assert_eq!(u.host_range(), 3..8);
        assert_eq!(u.to_host(5), 8);
        assert_eq!(u.from_host(8), Some(5));
    }

    #[test]
    fn fragment_view_offset_mapping() {
        // Host: `q = "SELECT 1";` — fragment "SELECT 1" starts at host char 5.
        let f = FragmentView::new("sql", 5, "SELECT 1");
        assert_eq!(f.len(), 8);
        assert_eq!(f.host_range(), 5..13);
        // fragment -> host
        assert_eq!(f.to_host(0), 5);
        assert_eq!(f.to_host(8), 13);
        // host -> fragment, round-trips
        assert_eq!(f.from_host(5), Some(0));
        assert_eq!(f.from_host(9), Some(4));
        assert_eq!(f.from_host(13), Some(8)); // end boundary inclusive
                                              // outside the fragment
        assert_eq!(f.from_host(4), None);
        assert_eq!(f.from_host(14), None);
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
