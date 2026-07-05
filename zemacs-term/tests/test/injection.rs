//! Tree-sitter language-injection detection: the effective language at a
//! position must follow embedded languages — `<style>` (CSS)/`<script>` (JS) in
//! HTML, and SQL strings in JS/TS/Python/Go. This is the "language injection"
//! behaviour that drives context-aware emmet and SQL completion the way
//! JetBrains does. Exercises [`Document::language_config_at`].

use super::*;

/// Byte offset of `needle` in the rope's text (UTF-8 byte index).
fn byte_of(text: &zemacs_core::Rope, needle: &str) -> usize {
    text.to_string()
        .find(needle)
        .unwrap_or_else(|| panic!("`{needle}` not found in buffer"))
}

/// Open `contents` as a file with the given extension and return the language id
/// reported by `language_config_at` at each of `needles` (in order).
async fn langs_at(ext: &str, contents: &str, needles: &[&str]) -> anyhow::Result<Vec<Option<String>>> {
    let file = tempfile::Builder::new().suffix(&format!(".{ext}")).tempfile()?;
    std::fs::write(file.path(), contents)?;
    let app = helpers::AppBuilder::new().with_file(file.path(), None).build()?;
    let loader = app.editor.syn_loader.load();
    let doc = app
        .editor
        .documents()
        .find(|d| d.path().is_some_and(|p| p.extension().is_some_and(|e| e == ext)))
        .expect("document open");
    let text = doc.text();
    Ok(needles
        .iter()
        .map(|n| {
            let b = byte_of(text, n);
            doc.language_config_at(&loader, b).map(|c| c.language_id.clone())
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

    assert_eq!(langs_at("py", py, &["SELECT id"]).await?[0].as_deref(), Some("sql"), "py multiline");
    assert_ne!(langs_at("py", py, &["pick one"]).await?[0].as_deref(), Some("sql"), "py english");
    assert_eq!(langs_at("go", go, &["INSERT INTO"]).await?[0].as_deref(), Some("sql"), "go raw string");
    assert_ne!(langs_at("go", go, &["hello world"]).await?[0].as_deref(), Some("sql"), "go plain");
    assert_eq!(langs_at("java", java, &["SELECT a"]).await?[0].as_deref(), Some("sql"), "java text block");
    assert_eq!(langs_at("cs", cs, &["UPDATE t"]).await?[0].as_deref(), Some("sql"), "c# raw string");
    assert_eq!(langs_at("kt", kt, &["DELETE FROM"]).await?[0].as_deref(), Some("sql"), "kotlin multiline");
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
        &["SELECT id", "INSERT INTO", "UPDATE users", "select an option", "Please choose", "SELECT * FROM"],
    )
    .await?;
    assert_eq!(got[0].as_deref(), Some("sql"), "SELECT … FROM … WHERE");
    assert_eq!(got[1].as_deref(), Some("sql"), "INSERT INTO");
    assert_eq!(got[2].as_deref(), Some("sql"), "UPDATE … SET");
    assert_ne!(got[3].as_deref(), Some("sql"), "English 'select … from …' must NOT match");
    assert_ne!(got[4].as_deref(), Some("sql"), "plain English must NOT match");
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
    assert_eq!(got[0].as_deref(), Some("sql"), "@Language(\"SQL\") on field");
    assert_ne!(got[1].as_deref(), Some("sql"), "unannotated field");
    assert_eq!(got[2].as_deref(), Some("sql"), "@Language(\"SQL\") on local var");
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
            doc.language_config_at(&loader, b).map(|c| c.language_id.clone())
        })
        .collect())
}

/// Assert every (needle -> expected injected language) across the polyglot
/// fixture corpus. `None` expected means "must not be SQL" (negative case).
#[tokio::test(flavor = "multi_thread")]
async fn language_injection_polyglot_corpus() -> anyhow::Result<()> {
    // (fixture, [(needle, Some(expected) | None=must-not-be-sql)])
    let cases: &[(&str, &[(&str, Option<&str>)])] = &[
        ("polyglot.html", &[
            ("margin: 0", Some("css")),
            ("env", Some("json")),
            ("hello world", Some("javascript")),
            ("display: flex", Some("css")),
            ("handleClick", Some("javascript")),
        ]),
        ("app.ts", &[
            ("SELECT tag_col", Some("sql")),
            ("UPDATE hint_tbl", Some("sql")),
            ("DELETE FROM method_tbl", Some("sql")),
            ("SELECT auto_a", Some("sql")),
            ("styledblue", Some("css")),
            ("graphField", Some("graphql")),
            ("please select a plan", None),
        ]),
        ("dao.py", &[
            ("SELECT exec_col", Some("sql")),
            ("UPDATE text_tbl", Some("sql")),
            ("SELECT auto_py_a", Some("sql")),
            ("select an item from the dropdown", None),
        ]),
        ("store.go", &[
            ("SELECT go_col", Some("sql")),
            ("DELETE FROM go_ctx", Some("sql")),
            ("INSERT INTO go_auto", Some("sql")),
            ("row %d of goplain", None),
        ]),
        ("UserRepo.java", &[
            ("SELECT ann_col", Some("sql")),
            ("SELECT jm_col", Some("sql")),
            ("SELECT jb_a", Some("sql")),
            ("choose an option", None),
        ]),
        ("Repo.cs", &[
            ("SELECT cs_col", Some("sql")),
            ("INSERT INTO cs_auto", Some("sql")),
            ("select a report", None),
        ]),
        ("Dao.kt", &[
            ("SELECT kt_col", Some("sql")),
            ("UPDATE kt_auto", Some("sql")),
            ("pick a theme", None),
        ]),
        ("notes.md", &[
            ("md_col", Some("sql")),
            ("md_func", Some("python")),
            ("md_rust", Some("rust")),
        ]),
        ("Component.vue", &[
            ("vue label", Some("typescript")),
            ("vuegreen", Some("scss")),
        ]),
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
    assert!(failures.is_empty(), "injection corpus mismatches:\n{}", failures.join("\n"));
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
    assert_eq!(got[0].as_deref(), Some("css"), "inline style= attribute -> css");
    assert_eq!(got[1].as_deref(), Some("javascript"), "on*= handler -> javascript");
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
    assert_eq!(got[0].as_deref(), Some("json"), "type=application/json -> json");
    assert_eq!(got[1].as_deref(), Some("typescript"), "type=text/typescript -> typescript");
    assert_eq!(got[2].as_deref(), Some("javascript"), "plain <script> -> javascript");
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
    let got = langs_at("js", js, &["SELECT 1", "UPDATE t", "DELETE FROM", "just a string"]).await?;
    assert_eq!(got[0].as_deref(), Some("sql"), "sql`` tagged template");
    assert_eq!(got[1].as_deref(), Some("sql"), "/* sql */ comment hint");
    assert_eq!(got[2].as_deref(), Some("sql"), "db.query() method");
    assert_ne!(got[3].as_deref(), Some("sql"), "plain string must not be SQL");
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
    assert_eq!(got[0].as_deref(), Some("bash"), "go injections load (go:generate -> bash)");
    assert_eq!(got[1].as_deref(), Some("sql"), "db.Query(`...`)");
    assert_eq!(got[2].as_deref(), Some("sql"), "conn.ExecContext(ctx, ...)");
    Ok(())
}
