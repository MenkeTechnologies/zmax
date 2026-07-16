//! Tree-sitter language-injection detection: the effective language at a
//! position must follow embedded languages — `<style>` (CSS)/`<script>` (JS) in
//! HTML, and SQL strings in JS/TS/Python/Go. This is the "language injection"
//! behaviour that drives context-aware emmet and SQL completion the way
//! JetBrains does. Exercises [`Document::language_config_at`].

use super::*;

/// Byte offset of `needle` in the rope's text (UTF-8 byte index).
fn byte_of(text: &zmax_core::Rope, needle: &str) -> usize {
    text.to_string()
        .find(needle)
        .unwrap_or_else(|| panic!("`{needle}` not found in buffer"))
}

/// Open `contents` as a file with the given extension and return the language id
/// reported by `language_config_at` at each of `needles` (in order).
async fn langs_at(
    ext: &str,
    contents: &str,
    needles: &[&str],
) -> anyhow::Result<Vec<Option<String>>> {
    let file = tempfile::Builder::new()
        .suffix(&format!(".{ext}"))
        .tempfile()?;
    std::fs::write(file.path(), contents)?;
    let app = helpers::AppBuilder::new()
        .with_file(file.path(), None)
        .build()?;
    let loader = app.editor.syn_loader.load();
    let doc = app
        .editor
        .documents()
        .find(|d| {
            d.path()
                .is_some_and(|p| p.extension().is_some_and(|e| e == ext))
        })
        .expect("document open");
    let text = doc.text();
    Ok(needles
        .iter()
        .map(|n| {
            let b = byte_of(text, n);
            doc.language_config_at(&loader, b)
                .map(|c| c.language_id.clone())
        })
        .collect())
}

#[tokio::test(flavor = "multi_thread")]
async fn language_injection_sql_autodetect_multihost() -> anyhow::Result<()> {
    // Plain assignments (no method call / tag / hint) across hosts, incl.
    // multiline text blocks / raw strings, detected purely from content.
    let py = "q = \"\"\"\nSELECT id FROM t WHERE x = 1\n\"\"\"\nlabel = \"pick one from list\"\n";
    let go = "package p\nvar q = `INSERT INTO t (a) VALUES (1)`\nvar s = \"hello world\"\n";
    let java = "class C { String q = \"\"\"\nSELECT a, b FROM t\n\"\"\"; String s = \"hi\"; }\n";
    let cs = "class C { string q = \"\"\"\nUPDATE t SET a = 1 WHERE id = 2\n\"\"\"; }\n";
    let kt = "val q = \"\"\"\nDELETE FROM t WHERE id = 3\n\"\"\"\n";

    assert_eq!(
        langs_at("py", py, &["SELECT id"]).await?[0].as_deref(),
        Some("sql"),
        "py multiline"
    );
    assert_ne!(
        langs_at("py", py, &["pick one"]).await?[0].as_deref(),
        Some("sql"),
        "py english"
    );
    assert_eq!(
        langs_at("go", go, &["INSERT INTO"]).await?[0].as_deref(),
        Some("sql"),
        "go raw string"
    );
    assert_ne!(
        langs_at("go", go, &["hello world"]).await?[0].as_deref(),
        Some("sql"),
        "go plain"
    );
    assert_eq!(
        langs_at("java", java, &["SELECT a"]).await?[0].as_deref(),
        Some("sql"),
        "java text block"
    );
    assert_eq!(
        langs_at("cs", cs, &["UPDATE t"]).await?[0].as_deref(),
        Some("sql"),
        "c# raw string"
    );
    assert_eq!(
        langs_at("kt", kt, &["DELETE FROM"]).await?[0].as_deref(),
        Some("sql"),
        "kotlin multiline"
    );
    Ok(())
}

#[test]
fn fragment_writeback_escaping() {
    use zmax_term::commands::escape_fragment;
    // double-quoted host: escape backslash, quote, newline
    assert_eq!(
        escape_fragment("a\"b\\c\nd", Some("javascript"), "= \""),
        "a\\\"b\\\\c\\nd"
    );
    // single-quoted host
    assert_eq!(escape_fragment("it's", Some("python"), "('"), "it\\'s");
    // JS/TS template literal: escape backtick and ${
    assert_eq!(
        escape_fragment("a`b${c}", Some("typescript"), "= `"),
        "a\\`b\\${c}"
    );
    // triple-quoted (Java text block / Python) — no escaping
    assert_eq!(
        escape_fragment("a\"b\nc", Some("java"), "\"\"\""),
        "a\"b\nc"
    );
    // Go raw string (backtick) — no escaping
    assert_eq!(escape_fragment("a\\b", Some("go"), "(`"), "a\\b");
}

#[tokio::test(flavor = "multi_thread")]
async fn language_injection_comment_hint() -> anyhow::Result<()> {
    // A `/* language=… */` hint forces a language the content sniffers would NOT
    // detect (plain text), proving hint detection is independent of content.
    let ts = "\
const a = /* language=json */ \"plain text not json\";
const b = /* language=graphql */ \"whatever text\";
const c = \"ordinary string\";
";
    let got = langs_at(
        "ts",
        ts,
        &["plain text", "whatever text", "ordinary string"],
    )
    .await?;
    assert_eq!(got[0].as_deref(), Some("json"), "language=json hint");
    assert_eq!(got[1].as_deref(), Some("graphql"), "language=graphql hint");
    assert_ne!(got[2].as_deref(), Some("json"), "no hint → not forced");

    // Python: the hint is on the line BEFORE the assignment (no inline block comment).
    let py = "# language=sql\nq = \"whatever text goes here\"\nplain = \"nope\"\n";
    let pygot = langs_at("py", py, &["whatever text", "nope"]).await?;
    assert_eq!(
        pygot[0].as_deref(),
        Some("sql"),
        "python prev-line # language=sql"
    );
    assert_ne!(pygot[1].as_deref(), Some("sql"), "python unhinted");
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn injected_fragment_extraction() -> anyhow::Result<()> {
    // The core of "Edit Fragment": at a byte inside an injected SQL string,
    // `injected_fragment_at` returns the guest language + the char range that
    // spans exactly the injected content.
    let ts = "const q = sql`SELECT frag_a, frag_b FROM fragtbl`;\n";
    let file = tempfile::Builder::new().suffix(".ts").tempfile()?;
    std::fs::write(file.path(), ts)?;
    let app = helpers::AppBuilder::new()
        .with_file(file.path(), None)
        .build()?;
    let loader = app.editor.syn_loader.load();
    let doc = app
        .editor
        .documents()
        .find(|d| {
            d.path()
                .is_some_and(|p| p.extension().is_some_and(|e| e == "ts"))
        })
        .unwrap();
    let text = doc.text();
    let inside = text.to_string().find("frag_a").unwrap();
    let byte = text.char_to_byte(text.byte_to_char(inside));

    let frag = zmax_term::commands::injected_fragment_at(doc, &loader, byte).expect("fragment");
    assert_eq!(frag.language, "sql");
    assert_eq!(
        frag.text, "SELECT frag_a, frag_b FROM fragtbl",
        "fragment span"
    );
    // host<->fragment offset mapping round-trips at the cursor
    let host_cursor = text.byte_to_char(byte);
    let in_frag = frag.from_host(host_cursor).expect("cursor inside fragment");
    assert_eq!(frag.to_host(in_frag), host_cursor, "offset round-trip");

    // A byte in the host (the `const q` declaration) is NOT an injection.
    let host_byte = text.char_to_byte(text.byte_to_char(text.to_string().find("const").unwrap()));
    assert!(
        zmax_term::commands::injected_fragment_at(doc, &loader, host_byte).is_none(),
        "host code must not be reported as a fragment"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn language_injection_content_sniffers() -> anyhow::Result<()> {
    // The multi-language content sniffers (JSON / HTML / GraphQL / SQL) with a
    // battery of tricky negatives (prose, JS-objects, placeholders, `<3`).
    let ts = "\
const j = '{\"aaa\": 1, \"bbb\": [2, 3]}';
const h = `<section id=\"x\">hi</section>`;
const g = \"query GetUser { gqlfield { id } }\";
const s = \"SELECT sniff_col FROM t WHERE id = 1\";
const css = `.btncss { color: cssred; padding: 4px; }`;
const arr = \"[1, 2, 3]\";
const n1 = \"{ not really json }\";
const n2 = \"<3 love this\";
const n3 = \"<placeholder text here\";
const n4 = \"query the results carefully { maybe }\";
const n5 = \"please pick a plan from the list\";
const n6 = \"just some label\";
";
    let needles = [
        "aaa",
        "section id",
        "gqlfield",
        "SELECT sniff_col",
        "cssred",
        "[1, 2, 3]",
        "not really json",
        "<3 love",
        "placeholder text",
        "query the results",
        "please pick",
        "just some label",
    ];
    let got = langs_at("ts", ts, &needles).await?;
    // positives
    assert_eq!(got[0].as_deref(), Some("json"), "json object");
    assert_eq!(got[1].as_deref(), Some("html"), "html element");
    assert_eq!(got[2].as_deref(), Some("graphql"), "graphql query");
    assert_eq!(got[3].as_deref(), Some("sql"), "sql");
    assert_eq!(got[4].as_deref(), Some("css"), "css block");
    assert_eq!(got[5].as_deref(), Some("json"), "json array");
    // negatives — none of these may be a sniffed guest language
    for (i, needle) in needles.iter().enumerate().skip(6) {
        let g = got[i].as_deref();
        assert!(
            !matches!(
                g,
                Some("json") | Some("html") | Some("graphql") | Some("sql") | Some("css")
            ),
            "false positive: {needle:?} sniffed as {g:?}"
        );
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn content_sniffers_across_hosts() -> anyhow::Result<()> {
    // The JSON/HTML/GraphQL sniffers are host-agnostic (all string hosts), not
    // just JS. Verify in Python, Go, and Java — with a negative each.
    // (ext, source, [(needle, expected)])
    let py = "\
cfg = '{\"host\": \"x\", \"port\": 8080}'
tpl = '''<section id=\"a\"><b>hi</b></section>'''
gql = '''query Q { pyField { id } }'''
plain = 'just a python label'
";
    let got = langs_at(
        "py",
        py,
        &["host", "section id", "pyField", "just a python"],
    )
    .await?;
    assert_eq!(got[0].as_deref(), Some("json"), "py json");
    assert_eq!(got[1].as_deref(), Some("html"), "py html");
    assert_eq!(got[2].as_deref(), Some("graphql"), "py graphql");
    assert!(
        !matches!(
            got[3].as_deref(),
            Some("json") | Some("html") | Some("graphql")
        ),
        "py plain string false positive"
    );

    let go = "package p\nvar j = `{\"a\": 1}`\nvar h = `<div><p>x</p></div>`\nvar s = \"plain go text\"\n";
    let got = langs_at("go", go, &["\"a\": 1", "<div>", "plain go text"]).await?;
    assert_eq!(got[0].as_deref(), Some("json"), "go json raw string");
    assert_eq!(got[1].as_deref(), Some("html"), "go html raw string");
    assert_ne!(got[2].as_deref(), Some("json"), "go plain");

    // Java text block. JSON would work too but embedded `"` split the fragment
    // into pieces; HTML (quote-free here) stays a single multiline fragment.
    let java =
        "class C { String h = \"\"\"\n<div><p>x</p></div>\n\"\"\"; String s = \"plain\"; }\n";
    let got = langs_at("java", java, &["<div>", "plain"]).await?;
    assert_eq!(got[0].as_deref(), Some("html"), "java html text block");
    assert_ne!(got[1].as_deref(), Some("html"), "java plain");
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn comment_hint_block_comment_hosts() -> anyhow::Result<()> {
    // `/* language=X */` immediately before a string forces X. Verified in Java
    // and C#, where the block comment anchors as a sibling of the string literal.
    // (Go attaches an inline comment to the statement rather than the expression,
    // so its `.`-anchored hint does not fire — content/call-site rules cover Go.)
    let java = "class C { String q = /* language=graphql */ \"whatever\"; }\n";
    assert_eq!(
        langs_at("java", java, &["whatever"]).await?[0].as_deref(),
        Some("graphql"),
        "java /* language=graphql */"
    );
    let cs = "class C { void M() { var q = /* language=sql */ \"plain text here\"; } }\n";
    assert_eq!(
        langs_at("cs", cs, &["plain text here"]).await?[0].as_deref(),
        Some("sql"),
        "c# /* language=sql */"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn language_injection_sql_autodetect_js() -> anyhow::Result<()> {
    // No tag, no hint, no query method — detected purely from content.
    let js = "\
const a = \"SELECT id, name FROM users WHERE id = 1\";
const b = \"INSERT INTO t (x) VALUES (1)\";
const c = \"UPDATE users SET name = 'x' WHERE id = 2\";
const d = \"select an option from the settings menu\";
const e = \"Please choose a plan\";
const f = \"SELECT * FROM logs\";
";
    let got = langs_at(
        "js",
        js,
        &[
            "SELECT id",
            "INSERT INTO",
            "UPDATE users",
            "select an option",
            "Please choose",
            "SELECT * FROM",
        ],
    )
    .await?;
    assert_eq!(got[0].as_deref(), Some("sql"), "SELECT … FROM … WHERE");
    assert_eq!(got[1].as_deref(), Some("sql"), "INSERT INTO");
    assert_eq!(got[2].as_deref(), Some("sql"), "UPDATE … SET");
    assert_ne!(
        got[3].as_deref(),
        Some("sql"),
        "English 'select … from …' must NOT match"
    );
    assert_ne!(
        got[4].as_deref(),
        Some("sql"),
        "plain English must NOT match"
    );
    assert_eq!(got[5].as_deref(), Some("sql"), "SELECT *");
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn language_injection_java_language_annotation() -> anyhow::Result<()> {
    let java = "\
class Repo {
  @Language(\"SQL\") String q = \"SELECT id FROM users\";
  String plain = \"not a query\";
  void m() {
    @Language(\"SQL\") String local = \"DELETE FROM t\";
  }
}
";
    let got = langs_at("java", java, &["SELECT id", "not a query", "DELETE FROM"]).await?;
    assert_eq!(
        got[0].as_deref(),
        Some("sql"),
        "@Language(\"SQL\") on field"
    );
    assert_ne!(got[1].as_deref(), Some("sql"), "unannotated field");
    assert_eq!(
        got[2].as_deref(),
        Some("sql"),
        "@Language(\"SQL\") on local var"
    );
    Ok(())
}

/// Open a real fixture file from tests/fixtures/injection and report the
/// injected language at each needle.
async fn fixture_langs(name: &str, needles: &[&str]) -> anyhow::Result<Vec<Option<String>>> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/injection")
        .join(name);
    let app = helpers::AppBuilder::new().with_file(&path, None).build()?;
    let loader = app.editor.syn_loader.load();
    let doc = app
        .editor
        .documents()
        .find(|d| d.path().is_some_and(|p| p.file_name() == path.file_name()))
        .unwrap_or_else(|| panic!("fixture {name} not opened"));
    let text = doc.text();
    Ok(needles
        .iter()
        .map(|n| {
            let b = byte_of(text, n);
            doc.language_config_at(&loader, b)
                .map(|c| c.language_id.clone())
        })
        .collect())
}

/// Assert every (needle -> expected injected language) across the polyglot
/// fixture corpus. `None` expected means "must not be SQL" (negative case).
#[tokio::test(flavor = "multi_thread")]
async fn language_injection_polyglot_corpus() -> anyhow::Result<()> {
    // (fixture, [(needle, Some(expected) | None=must-not-be-sql)])
    let cases: &[(&str, &[(&str, Option<&str>)])] = &[
        (
            "polyglot.html",
            &[
                ("margin: 0", Some("css")),
                ("env", Some("json")),
                ("hello world", Some("javascript")),
                ("display: flex", Some("css")),
                ("handleClick", Some("javascript")),
            ],
        ),
        (
            "app.ts",
            &[
                ("SELECT tag_col", Some("sql")),
                ("UPDATE hint_tbl", Some("sql")),
                ("DELETE FROM method_tbl", Some("sql")),
                ("SELECT auto_a", Some("sql")),
                ("styledblue", Some("css")),
                ("graphField", Some("graphql")),
                ("please select a plan", None),
            ],
        ),
        (
            "dao.py",
            &[
                ("SELECT exec_col", Some("sql")),
                ("UPDATE text_tbl", Some("sql")),
                ("SELECT auto_py_a", Some("sql")),
                ("select an item from the dropdown", None),
            ],
        ),
        (
            "store.go",
            &[
                ("SELECT go_col", Some("sql")),
                ("DELETE FROM go_ctx", Some("sql")),
                ("INSERT INTO go_auto", Some("sql")),
                ("row %d of goplain", None),
            ],
        ),
        (
            "UserRepo.java",
            &[
                ("SELECT ann_col", Some("sql")),
                ("SELECT jm_col", Some("sql")),
                ("SELECT jb_a", Some("sql")),
                ("choose an option", None),
            ],
        ),
        (
            "Repo.cs",
            &[
                ("SELECT cs_col", Some("sql")),
                ("INSERT INTO cs_auto", Some("sql")),
                ("select a report", None),
            ],
        ),
        (
            "Dao.kt",
            &[
                ("SELECT kt_col", Some("sql")),
                ("UPDATE kt_auto", Some("sql")),
                ("pick a theme", None),
            ],
        ),
        (
            "notes.md",
            &[
                ("md_col", Some("sql")),
                ("md_func", Some("python")),
                ("md_rust", Some("rust")),
            ],
        ),
        (
            "Component.vue",
            &[
                ("vue label", Some("typescript")),
                ("vuegreen", Some("scss")),
            ],
        ),
    ];

    let mut failures = Vec::new();
    for (fixture, checks) in cases {
        let needles: Vec<&str> = checks.iter().map(|(n, _)| *n).collect();
        let got = fixture_langs(fixture, &needles).await?;
        for ((needle, expected), actual) in checks.iter().zip(got) {
            let ok = match expected {
                Some(lang) => actual.as_deref() == Some(*lang),
                None => actual.as_deref() != Some("sql"),
            };
            if !ok {
                failures.push(format!(
                    "{fixture} @ {:?}: expected {:?}, got {:?}",
                    needle, expected, actual
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "injection corpus mismatches:\n{}",
        failures.join("\n")
    );
    Ok(())
}

#[test]
fn embedded_interpreter_language_servers() {
    // `.stk` -> stryke --lsp, `.sh`/`.zsh` -> zshrs --lsp (the stryke/zshrs
    // binaries are their own LSP servers over stdio).
    let loader = helpers::test_syntax_loader(None);

    // Server definitions.
    let servers = loader.language_server_configs();
    let stryke = servers.get("stryke-lsp").expect("stryke-lsp server def");
    assert_eq!(stryke.command, "stryke");
    assert_eq!(stryke.args, vec!["--lsp".to_string()]);
    let zshrs = servers.get("zshrs-lsp").expect("zshrs-lsp server def");
    assert_eq!(zshrs.command, "zshrs");
    assert_eq!(zshrs.args, vec!["--lsp".to_string()]);

    // `.stk` is the stryke language, served by stryke-lsp.
    let stk = loader
        .language_for_filename(std::path::Path::new("script.stk"))
        .expect(".stk unmapped");
    let stk_cfg = loader.language(stk).config();
    assert_eq!(stk_cfg.language_id, "stryke");
    assert!(
        stk_cfg
            .language_servers
            .iter()
            .any(|s| s.name == "stryke-lsp"),
        "stryke not served by stryke-lsp"
    );

    // `.sh` and `.zsh` (the bash language) are served by zshrs-lsp.
    for ext in ["x.sh", "x.zsh"] {
        let lang = loader
            .language_for_filename(std::path::Path::new(ext))
            .unwrap_or_else(|| panic!("{ext} unmapped"));
        let cfg = loader.language(lang).config();
        assert!(
            cfg.language_servers.iter().any(|s| s.name == "zshrs-lsp"),
            "{ext} not served by zshrs-lsp"
        );
    }
}

/// Count `ERROR` nodes in a document's tree-sitter parse (0 = clean parse).
fn tree_error_count(doc: &zmax_view::Document) -> usize {
    let syntax = doc.syntax().expect("no syntax tree");
    let mut stack = vec![syntax.tree().root_node()];
    let mut errors = 0;
    while let Some(n) = stack.pop() {
        if n.kind() == "ERROR" {
            errors += 1;
        }
        for i in 0..n.child_count() {
            if let Some(c) = n.child(i) {
                stack.push(c);
            }
        }
    }
    errors
}

async fn open_stk(src: &str) -> anyhow::Result<helpers::AppBuilder> {
    let file = tempfile::Builder::new().suffix(".stk").tempfile()?;
    std::fs::write(file.path(), src)?;
    Ok(helpers::AppBuilder::new().with_file(file.path(), None))
}

#[tokio::test(flavor = "multi_thread")]
async fn stryke_parses_common_constructs_without_errors() -> anyhow::Result<()> {
    let src = "\
my $x = 1;
my $y = \"hello\";
my $z = 'raw';
my $n = 0x1F;
sub double { return $x * 2; }
$obj->call(1, 2);
$x |> double;
$x ~> pmap;
print \"v: \", $x, $y;
# a comment
";
    let app = open_stk(src).await?.build()?;
    let doc = app.editor.documents().next().unwrap();
    assert_eq!(doc.language_name(), Some("stryke"));
    assert_eq!(
        tree_error_count(doc),
        0,
        "stryke program must parse with no ERROR nodes"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn stryke_all_pipe_operators_parse_clean() -> anyhow::Result<()> {
    // Every stryke pipe/thread operator (from strykelang token.rs) must tokenize
    // and parse as an operator — no ERROR fallback.
    for op in [
        "|>", "~>", "~>>", "~s>", "~s>>", "~p>", "~p>>", "~d>", "~d>>", "->>", "~|>", "||>",
        "|then|",
    ] {
        let src = format!("$a {op} $b;\n");
        let app = open_stk(&src).await?.build()?;
        let doc = app.editor.documents().next().unwrap();
        assert_eq!(
            tree_error_count(doc),
            0,
            "operator {op:?} did not parse cleanly"
        );
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn stryke_grammar_highlights() -> anyhow::Result<()> {
    // `.stk` has its own tree-sitter grammar (incl. stryke pipe ops |> ~>).
    // A present syntax tree proves the grammar dylib loaded AND highlights.scm
    // compiled against it.
    let stk = "my $x = [1, 2, 3];\n$x |> pmap { $_ * 2 } ~> psort;\nsub greet { print \"hi\", $x; }\n# comment\n";
    let file = tempfile::Builder::new().suffix(".stk").tempfile()?;
    std::fs::write(file.path(), stk)?;
    let app = helpers::AppBuilder::new()
        .with_file(file.path(), None)
        .build()?;
    let doc = app
        .editor
        .documents()
        .find(|d| {
            d.path()
                .is_some_and(|p| p.extension().is_some_and(|e| e == "stk"))
        })
        .expect("stk doc");
    assert_eq!(doc.language_name(), Some("stryke"));
    assert!(
        doc.syntax().is_some(),
        "stryke grammar must load and parse (also validates highlights.scm)"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn language_injection_follows_style_and_script() -> anyhow::Result<()> {
    let html = "\
<!DOCTYPE html>
<html>
<head>
<style>
body { margin: 0; }
</style>
<script>
let x = 1;
</script>
</head>
<body><p>hi</p></body>
</html>
";
    let got = langs_at("html", html, &["margin: 0", "let x = 1", "<p>hi"]).await?;
    assert_eq!(got[0].as_deref(), Some("css"));
    assert_eq!(got[1].as_deref(), Some("javascript"));
    assert_eq!(got[2].as_deref(), Some("html"));
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn language_injection_html_inline_attributes() -> anyhow::Result<()> {
    let html = "<div style=\"color: red\" onclick=\"doThing()\">hi</div>\n";
    let got = langs_at("html", html, &["color: red", "doThing()", "hi</div"]).await?;
    assert_eq!(
        got[0].as_deref(),
        Some("css"),
        "inline style= attribute -> css"
    );
    assert_eq!(
        got[1].as_deref(),
        Some("javascript"),
        "on*= handler -> javascript"
    );
    assert_eq!(got[2].as_deref(), Some("html"), "element text stays html");
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn language_injection_html_script_type() -> anyhow::Result<()> {
    let html = "\
<script type=\"application/json\">{\"aaa\": 1}</script>
<script type=\"text/typescript\">let yyy: number = 2;</script>
<script>let zzz = 3;</script>
";
    let got = langs_at("html", html, &["aaa", "yyy", "zzz"]).await?;
    assert_eq!(
        got[0].as_deref(),
        Some("json"),
        "type=application/json -> json"
    );
    assert_eq!(
        got[1].as_deref(),
        Some("typescript"),
        "type=text/typescript -> typescript"
    );
    assert_eq!(
        got[2].as_deref(),
        Some("javascript"),
        "plain <script> -> javascript"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn language_injection_sql_in_javascript() -> anyhow::Result<()> {
    let js = "\
const a = sql`SELECT 1 FROM t`;
const b = /* sql */ \"UPDATE t SET x=1\";
db.query(\"DELETE FROM t WHERE id=1\");
const plain = \"just a string\";
";
    let got = langs_at(
        "js",
        js,
        &["SELECT 1", "UPDATE t", "DELETE FROM", "just a string"],
    )
    .await?;
    assert_eq!(got[0].as_deref(), Some("sql"), "sql`` tagged template");
    assert_eq!(got[1].as_deref(), Some("sql"), "/* sql */ comment hint");
    assert_eq!(got[2].as_deref(), Some("sql"), "db.query() method");
    assert_ne!(
        got[3].as_deref(),
        Some("sql"),
        "plain string must not be SQL"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn language_injection_sql_in_python() -> anyhow::Result<()> {
    let py = "\
cursor.execute(\"SELECT id FROM users\")
q = text(\"UPDATE t SET x=1\")
label = \"not a query here\"
";
    let got = langs_at("py", py, &["SELECT id", "UPDATE t", "not a query"]).await?;
    assert_eq!(got[0].as_deref(), Some("sql"), "cursor.execute()");
    assert_eq!(got[1].as_deref(), Some("sql"), "text()");
    assert_ne!(got[2].as_deref(), Some("sql"), "plain string");
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn language_injection_sql_in_java() -> anyhow::Result<()> {
    let java = "\
class Repo {
  void m(java.sql.Statement s) throws Exception {
    var rs = s.executeQuery(\"SELECT id FROM users\");
    String plain = \"not a query\";
  }
}
";
    let got = langs_at("java", java, &["SELECT id", "not a query"]).await?;
    assert_eq!(got[0].as_deref(), Some("sql"), "s.executeQuery()");
    assert_ne!(got[1].as_deref(), Some("sql"), "plain string");
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn language_injection_sql_in_csharp() -> anyhow::Result<()> {
    let cs = "\
class Repo {
  void M(System.Data.IDbConnection conn) {
    var r = conn.ExecuteReader(\"SELECT id FROM users\");
    var plain = conn.Foo(\"not a query\");
  }
}
";
    let got = langs_at("cs", cs, &["SELECT id", "not a query"]).await?;
    assert_eq!(got[0].as_deref(), Some("sql"), "conn.ExecuteReader()");
    assert_ne!(got[1].as_deref(), Some("sql"), "non-query method");
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn language_injection_sql_in_kotlin() -> anyhow::Result<()> {
    let kt = "\
fun m(db: android.database.sqlite.SQLiteDatabase) {
    val c = db.rawQuery(\"SELECT id FROM users\")
    val plain = db.foo(\"not a query\")
}
";
    let got = langs_at("kt", kt, &["SELECT id", "not a query"]).await?;
    assert_eq!(got[0].as_deref(), Some("sql"), "db.rawQuery()");
    assert_ne!(got[1].as_deref(), Some("sql"), "non-query method");
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn language_injection_sql_in_go() -> anyhow::Result<()> {
    let go = "\
package main

//go:generate echo hello
func run() {
\trows, _ := db.Query(`SELECT id FROM users`)
\t_, _ = conn.ExecContext(ctx, \"DELETE FROM t\")
\t_ = rows
}
";
    let got = langs_at("go", go, &["echo hello", "SELECT id", "DELETE FROM"]).await?;
    // //go:generate injects bash — proves Go injections load at all.
    assert_eq!(
        got[0].as_deref(),
        Some("bash"),
        "go injections load (go:generate -> bash)"
    );
    assert_eq!(got[1].as_deref(), Some("sql"), "db.Query(`...`)");
    assert_eq!(got[2].as_deref(), Some("sql"), "conn.ExecContext(ctx, ...)");
    Ok(())
}
