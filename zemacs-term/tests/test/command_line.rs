use super::*;

use zemacs_core::diagnostic::Severity;

#[tokio::test(flavor = "multi_thread")]
async fn history_completion() -> anyhow::Result<()> {
    test_key_sequence(
        &mut AppBuilder::new().build()?,
        Some(":asdf<ret>:theme d<C-n><tab>"),
        Some(&|app| {
            assert!(!app.editor.is_err());
        }),
        false,
    )
    .await?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn prompt_reset_anchor() -> anyhow::Result<()> {
    test_key_sequence(
        &mut AppBuilder::new().build()?,
        Some(":string wider than the terminal window causing the anchor location to be non zero which would panic when the line is deleted<C-u>"),
        Some(&|app| {
            assert!(!app.editor.is_err());
        }),
        false,
    )
    .await?;

    Ok(())
}

async fn test_statusline(
    line: &str,
    expected_status: &str,
    expected_severity: Severity,
) -> anyhow::Result<()> {
    test_key_sequence(
        &mut AppBuilder::new().build()?,
        Some(&format!("{line}<ret>")),
        Some(&|app| {
            let (status, &severity) = app.editor.get_status().unwrap();
            assert_eq!(
                severity, expected_severity,
                "'{line}' printed {severity:?}: {status}"
            );
            assert_eq!(status.as_ref(), expected_status);
        }),
        false,
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn variable_expansion() -> anyhow::Result<()> {
    test_statusline(r#":echo %{cursor_line}"#, "1", Severity::Info).await?;
    // Double quotes can be used with expansions:
    test_statusline(
        r#":echo "line%{cursor_line}line""#,
        "line1line",
        Severity::Info,
    )
    .await?;
    // Within double quotes you can escape the percent token for an expansion by doubling it.
    test_statusline(
        r#":echo "%%{cursor_line}""#,
        "%{cursor_line}",
        Severity::Info,
    )
    .await?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn unicode_expansion() -> anyhow::Result<()> {
    test_statusline(r#":echo %u{20}"#, " ", Severity::Info).await?;
    test_statusline(r#":echo %u{0020}"#, " ", Severity::Info).await?;
    test_statusline(r#":echo %u{25CF}"#, "●", Severity::Info).await?;
    // Not a valid Unicode codepoint:
    test_statusline(
        r#":echo %u{deadbeef}"#,
        "'echo': could not interpret 'deadbeef' as a Unicode character code",
        Severity::Error,
    )
    .await?;

    Ok(())
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn shell_expansion() -> anyhow::Result<()> {
    test_statusline(
        r#":echo %sh{echo "hello world"}"#,
        "hello world",
        Severity::Info,
    )
    .await?;

    // Shell expansion is recursive.
    test_statusline(":echo %sh{echo '%{cursor_line}'}", "1", Severity::Info).await?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn register_expansion() -> anyhow::Result<()> {
    test_statusline(
        r#":set-register a hello world<ret>:echo %reg{a}"#,
        "hello world",
        Severity::Info,
    )
    .await?;
    test_statusline(r#":echo %reg{a}"#, "", Severity::Info).await?;
    test_statusline(
        r#":echo %reg{abc}"#,
        "'echo': Invalid register `abc`: should only be a single character",
        Severity::Error,
    )
    .await?;

    // Register expansion evaluation is *not* recursive.
    test_statusline(
        r#":set-register a b<ret>:set-register b hello<ret>:echo %reg{%reg{a}}"#,
        "'echo': Invalid register `%reg{a}`: should only be a single character",
        Severity::Error,
    )
    .await?;
    test_statusline(
        r#":set-register a hello<ret>:set-register b %%reg{a}<ret>:echo %reg{b}"#,
        "%reg{a}",
        Severity::Info,
    )
    .await?;

    // However, you can copy the contents of one register into another with this expansion if you
    // want to.
    test_statusline(
        r#":set-register a hello<ret>:set-register b %reg{a}<ret>:echo %reg{b}"#,
        "hello",
        Severity::Info,
    )
    .await?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn percent_escaping() -> anyhow::Result<()> {
    test_statusline(
        r#":sh echo hello 10%"#,
        "'run-shell-command': '%' was not properly escaped. Please use '%%'",
        Severity::Error,
    )
    .await?;
    Ok(())
}

// `:wincmd h` focuses across a split without panicking, and `:wincmd q` closes
// the current window — the split (2 views) drops back to a single view.
#[tokio::test(flavor = "multi_thread")]
async fn wincmd_split_focus_close() -> anyhow::Result<()> {
    test_key_sequence(
        &mut AppBuilder::new().build()?,
        Some(":vsplit<ret>:wincmd h<ret>:wincmd q<ret>"),
        Some(&|app| {
            assert!(!app.editor.is_err(), "{:?}", app.editor.get_status());
            assert_eq!(app.editor.tree.views().count(), 1);
        }),
        false,
    )
    .await?;
    Ok(())
}

// `:wincmd o` (only) closes every other window, leaving one.
#[tokio::test(flavor = "multi_thread")]
async fn wincmd_only_closes_others() -> anyhow::Result<()> {
    test_key_sequence(
        &mut AppBuilder::new().build()?,
        Some(":vsplit<ret>:vsplit<ret>:wincmd o<ret>"),
        Some(&|app| {
            assert!(!app.editor.is_err());
            assert_eq!(app.editor.tree.views().count(), 1);
        }),
        false,
    )
    .await?;
    Ok(())
}

// An unsupported `:wincmd` argument reports an error rather than silently no-op.
#[tokio::test(flavor = "multi_thread")]
async fn wincmd_unsupported_arg_errors() -> anyhow::Result<()> {
    test_key_sequence(
        &mut AppBuilder::new().build()?,
        Some(":wincmd =<ret>"),
        Some(&|app| {
            assert!(app.editor.is_err());
        }),
        false,
    )
    .await?;
    Ok(())
}

// `:windo {cmd}` runs an ex-command in each window of the split (both survive),
// then `:wincmd o` collapses back to one window so the app can exit on teardown.
#[tokio::test(flavor = "multi_thread")]
async fn windo_runs_in_each_window() -> anyhow::Result<()> {
    test_key_sequences(
        &mut AppBuilder::new().build()?,
        vec![
            (
                Some(":vsplit<ret>:windo echo %{cursor_line}<ret>"),
                Some(&|app| {
                    assert!(!app.editor.is_err(), "{:?}", app.editor.get_status());
                    assert_eq!(app.editor.tree.views().count(), 2);
                } as _),
            ),
            (Some(":wincmd o<ret>"), None),
        ],
        false,
    )
    .await?;
    Ok(())
}

// `:echon` is not a native zemacs command — it falls through to the embedded
// vimlrs interpreter (Vim's `:` prompt IS the Vimscript engine). Its captured
// echo output lands on the status line.
#[tokio::test(flavor = "multi_thread")]
async fn viml_passthrough_echon() -> anyhow::Result<()> {
    test_key_sequence(
        &mut AppBuilder::new().build()?,
        Some(r#":echon "zt42"<ret>"#),
        Some(&|app| {
            assert!(!app.editor.is_err(), "{:?}", app.editor.get_status());
            let (status, _) = app.editor.get_status().unwrap();
            assert_eq!(status.as_ref(), "zt42");
        }),
        false,
    )
    .await?;
    Ok(())
}

// `:source {file}` runs a real Vimscript file through vimlrs (script context).
#[tokio::test(flavor = "multi_thread")]
async fn source_vimscript_file() -> anyhow::Result<()> {
    use std::io::Write;
    let mut file = tempfile::NamedTempFile::new()?;
    writeln!(file, "let g:zt_sourced = 1")?;
    file.flush()?;
    let path = file.path().to_string_lossy().to_string();
    test_key_sequence(
        &mut AppBuilder::new().build()?,
        Some(&format!(":source {path}<ret>")),
        Some(&move |app| {
            assert!(!app.editor.is_err(), "{:?}", app.editor.get_status());
            let (status, _) = app.editor.get_status().unwrap();
            assert!(
                status.as_ref().starts_with("sourced"),
                "status: {}",
                status.as_ref()
            );
        }),
        false,
    )
    .await?;
    Ok(())
}

// `:make` runs the make program, capturing output into the quickfix list and
// setting a compilation status. `--version` keeps it deterministic and
// cwd-independent (no Makefile needed): make prints its version and exits 0.
#[tokio::test(flavor = "multi_thread")]
async fn make_runs_and_reports() -> anyhow::Result<()> {
    test_key_sequence(
        &mut AppBuilder::new().build()?,
        Some(":make --version<ret>"),
        Some(&|app| {
            assert!(!app.editor.is_err(), "{:?}", app.editor.get_status());
            let (status, _) = app.editor.get_status().unwrap();
            assert!(
                status.as_ref().starts_with("Compilation finished"),
                "status: {}",
                status.as_ref()
            );
        }),
        false,
    )
    .await?;
    Ok(())
}

// Vim tag stack over a real ctags `tags` file: `:tag` jumps to the first match
// and pushes the stack, `:tnext` cycles to the second match, `:pop` returns.
#[tokio::test(flavor = "multi_thread")]
async fn tag_stack_jump_next_pop() -> anyhow::Result<()> {
    use std::io::Write;
    let dir = tempfile::tempdir()?;
    let src = dir.path().join("unit.c");
    let mut f = std::fs::File::create(&src)?;
    // foo is defined on line 2 (idx 1) and line 4 (idx 3).
    writeln!(f, "int other() {{}}")?;
    writeln!(f, "int foo() {{ return 1; }}")?;
    writeln!(f, "void bar() {{}}")?;
    writeln!(f, "int foo() {{ return 2; }}")?;
    f.flush()?;
    let mut tags = std::fs::File::create(dir.path().join("tags"))?;
    write!(tags, "foo\tunit.c\t/^int foo() {{ return 1; }}$/\n")?;
    write!(tags, "foo\tunit.c\t/^int foo() {{ return 2; }}$/\n")?;
    tags.flush()?;

    fn cur_line(app: &zemacs_term::application::Application) -> usize {
        let view = app.editor.tree.get(app.editor.tree.focus);
        let doc = app.editor.document(view.doc).unwrap();
        let text = doc.text();
        text.char_to_line(doc.selection(view.id).primary().cursor(text.slice(..)))
    }

    let mut app = helpers::AppBuilder::new().with_file(&src, None).build()?;
    test_key_sequences(
        &mut app,
        vec![
            (
                Some(":tag foo<ret>"),
                Some(&|app| {
                    assert!(!app.editor.is_err(), "{:?}", app.editor.get_status());
                    assert_eq!(cur_line(app), 1, "first match on line idx 1");
                } as _),
            ),
            (
                Some(":tnext<ret>"),
                Some(&|app| {
                    assert!(!app.editor.is_err(), "{:?}", app.editor.get_status());
                    assert_eq!(cur_line(app), 3, "second match on line idx 3");
                } as _),
            ),
            (
                Some(":pop<ret>"),
                Some(&|app| {
                    assert!(!app.editor.is_err(), "{:?}", app.editor.get_status());
                    assert_eq!(cur_line(app), 0, "popped back to the start line");
                } as _),
            ),
        ],
        false,
    )
    .await?;
    Ok(())
}

fn tag_cur_line(app: &zemacs_term::application::Application) -> usize {
    let view = app.editor.tree.get(app.editor.tree.focus);
    let doc = app.editor.document(view.doc).unwrap();
    let text = doc.text();
    text.char_to_line(doc.selection(view.id).primary().cursor(text.slice(..)))
}

// `:tjump {name}` with a unique match jumps straight to it, no picker.
#[tokio::test(flavor = "multi_thread")]
async fn tag_tjump_unique_jumps() -> anyhow::Result<()> {
    use std::io::Write;
    let dir = tempfile::tempdir()?;
    let src = dir.path().join("u.c");
    let mut f = std::fs::File::create(&src)?;
    writeln!(f, "int a() {{}}")?;
    writeln!(f, "int uniq() {{ return 0; }}")?;
    f.flush()?;
    let mut tags = std::fs::File::create(dir.path().join("tags"))?;
    write!(tags, "uniq\tu.c\t/^int uniq() {{ return 0; }}$/\n")?;
    tags.flush()?;
    let mut app = helpers::AppBuilder::new().with_file(&src, None).build()?;
    test_key_sequence(
        &mut app,
        Some(":tjump uniq<ret>"),
        Some(&|app| {
            assert!(!app.editor.is_err(), "{:?}", app.editor.get_status());
            assert_eq!(tag_cur_line(app), 1);
        }),
        false,
    )
    .await?;
    Ok(())
}

// `:stag {name}` opens the definition in a new split (numeric tag address).
#[tokio::test(flavor = "multi_thread")]
async fn tag_stag_opens_split() -> anyhow::Result<()> {
    use std::io::Write;
    let dir = tempfile::tempdir()?;
    let src = dir.path().join("u.c");
    let mut f = std::fs::File::create(&src)?;
    writeln!(f, "int a() {{}}")?;
    writeln!(f, "int foo() {{}}")?;
    f.flush()?;
    let mut tags = std::fs::File::create(dir.path().join("tags"))?;
    write!(tags, "foo\tu.c\t2\n")?;
    tags.flush()?;
    let mut app = helpers::AppBuilder::new().with_file(&src, None).build()?;
    test_key_sequences(
        &mut app,
        vec![
            (
                Some(":stag foo<ret>"),
                Some(&|app| {
                    assert!(!app.editor.is_err(), "{:?}", app.editor.get_status());
                    assert_eq!(app.editor.tree.views().count(), 2, "split opened");
                    assert_eq!(tag_cur_line(app), 1, "cursor on the tag line");
                } as _),
            ),
            (Some(":wincmd o<ret>"), None),
        ],
        false,
    )
    .await?;
    Ok(())
}

// `:tselect {name}` opens the match picker without error when matches exist, and
// errors on an unknown name. (The picker's on-select jump reuses `jump_to_tag`,
// covered by the `:tag`/`:tjump` tests.)
#[tokio::test(flavor = "multi_thread")]
async fn tag_tselect_picker_and_errors() -> anyhow::Result<()> {
    use std::io::Write;
    let dir = tempfile::tempdir()?;
    let src = dir.path().join("u.c");
    let mut f = std::fs::File::create(&src)?;
    writeln!(f, "int foo() {{ return 1; }}")?;
    writeln!(f, "int foo() {{ return 2; }}")?;
    f.flush()?;
    let mut tags = std::fs::File::create(dir.path().join("tags"))?;
    write!(tags, "foo\tu.c\t1\n")?;
    write!(tags, "foo\tu.c\t2\n")?;
    tags.flush()?;
    let mut app = helpers::AppBuilder::new().with_file(&src, None).build()?;
    test_key_sequences(
        &mut app,
        vec![
            (
                Some(":tselect nope<ret>"),
                Some(&|app| assert!(app.editor.is_err(), "unknown tag should error") as _),
            ),
            (
                Some("<esc>:tselect foo<ret>"),
                Some(&|app| {
                    assert!(!app.editor.is_err(), "{:?}", app.editor.get_status());
                } as _),
            ),
        ],
        false,
    )
    .await?;
    Ok(())
}

// `:messages` shows the session message log; a status set via the vimlrs
// passthrough (`:echon`) is captured and appears in the popup.
#[tokio::test(flavor = "multi_thread")]
async fn messages_log_captures_status() -> anyhow::Result<()> {
    test_key_sequence(
        &mut AppBuilder::new().build()?,
        Some(r#":echon "msgtest99"<ret>:messages<ret>"#),
        Some(&|app| {
            assert!(!app.editor.is_err(), "{:?}", app.editor.get_status());
            let info = app.editor.autoinfo.as_ref().expect(":messages popup");
            assert!(
                info.text.contains("msgtest99"),
                "message log missing the status: {}",
                info.text
            );
        }),
        false,
    )
    .await?;
    Ok(())
}

// `:redir @a` … `:redir END` captures the intervening message output into
// register a, readable back via the `%reg{a}` expansion.
#[tokio::test(flavor = "multi_thread")]
async fn redir_captures_to_register() -> anyhow::Result<()> {
    test_key_sequence(
        &mut AppBuilder::new().build()?,
        Some(r#":redir @a<ret>:echon "captured7"<ret>:redir END<ret>:echo %reg{a}<ret>"#),
        Some(&|app| {
            let (status, _) = app.editor.get_status().unwrap();
            assert!(
                status.as_ref().contains("captured7"),
                "register a should hold the captured output: {}",
                status.as_ref()
            );
        }),
        false,
    )
    .await?;
    Ok(())
}

// `:redir > file` … `:redir END` writes the captured output to a file.
#[tokio::test(flavor = "multi_thread")]
async fn redir_captures_to_file() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let out = dir.path().join("cap.txt");
    // `<gt>` is the key-macro escape for `>` (a bare `>` confuses parse_macro).
    let seq = format!(
        r#":redir <gt>{}<ret>:echon "fileword"<ret>:redir END<ret>"#,
        out.display()
    );
    test_key_sequence(
        &mut AppBuilder::new().build()?,
        Some(&seq),
        Some(&|app| assert!(!app.editor.is_err(), "{:?}", app.editor.get_status()) as _),
        false,
    )
    .await?;
    let written = std::fs::read_to_string(&out)?;
    assert!(written.contains("fileword"), "file capture: {written:?}");
    Ok(())
}

// neovim `:Man` with no topic reports usage rather than spawning a bare `man`.
#[tokio::test(flavor = "multi_thread")]
async fn man_no_topic_errors() -> anyhow::Result<()> {
    test_key_sequence(
        &mut AppBuilder::new().build()?,
        Some(":Man<ret>"),
        Some(&|app| assert!(app.editor.is_err(), "bare :Man should report usage") as _),
        false,
    )
    .await?;
    Ok(())
}

// neovim `:Inspect` / `:InspectTree` are recognized command names (aliases of the
// tree-sitter inspection commands) — they dispatch without error on a real file,
// rather than falling through to the vimlrs "not a command" path.
#[tokio::test(flavor = "multi_thread")]
async fn neovim_inspect_aliases_recognized() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let src = dir.path().join("m.rs");
    std::fs::write(&src, "fn main() { let x = 1; }\n")?;
    let mut app = helpers::AppBuilder::new().with_file(&src, None).build()?;
    test_key_sequence(
        &mut app,
        Some(":InspectTree<ret>:Inspect<ret>"),
        Some(&|app| {
            assert!(
                !app.editor.is_err(),
                "neovim inspect aliases should be recognized: {:?}",
                app.editor.get_status()
            );
        }),
        false,
    )
    .await?;
    Ok(())
}

// `:undolist` lists the undo states in a popup after a couple of edits, marking
// the current one with `>`.
#[tokio::test(flavor = "multi_thread")]
async fn undolist_shows_states() -> anyhow::Result<()> {
    test_key_sequence(
        &mut AppBuilder::new().build()?,
        Some("iabc<esc>oxyz<esc>:undolist<ret>"),
        Some(&|app| {
            assert!(!app.editor.is_err(), "{:?}", app.editor.get_status());
            let info = app.editor.autoinfo.as_ref().expect(":undolist popup");
            assert!(
                info.text.contains('>'),
                "undo list should mark the current state: {}",
                info.text
            );
        }),
        false,
    )
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn version_command_shows_version_in_scratch() -> anyhow::Result<()> {
    // :version opens a scratch buffer whose first line is the build-time version
    // (never a hardcoded literal) plus the feature summary.
    test_key_sequence(
        &mut AppBuilder::new().build()?,
        Some(":version<ret>"),
        Some(&|app| {
            let text = zemacs_view::doc!(app.editor).text().to_string();
            assert!(
                text.contains(&format!("zemacs {}", env!("CARGO_PKG_VERSION"))),
                "scratch should contain the build-time zemacs version line, got: {text:?}"
            );
            assert!(
                text.contains("superset"),
                "version output should include the feature summary"
            );
        }),
        false,
    )
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn viml_statement_commands_eval_without_error() -> anyhow::Result<()> {
    // Each first-class VimL ex-command forwards to the vimlrs engine; a valid
    // statement must not leave the editor in an error state. Related commands are
    // chained in one app so variables exist for the later statements.
    let cases: &[&str] = &[
        ":eval 1 + 1<ret>",
        ":echomsg 'hi'<ret>",
        ":call type(0)<ret>",
        ":let g:e = 1<ret>:execute 'let g:e = 2'<ret>",
        ":const g:c = 3<ret>:unlet g:c<ret>",
    ];
    for keys in cases {
        test_key_sequence(
            &mut AppBuilder::new().build()?,
            Some(keys),
            Some(&|app| {
                let status = app.editor.get_status().map(|(s, _)| s.to_string());
                assert!(
                    !app.editor.is_err(),
                    "VimL ex-command {keys:?} errored: {status:?}"
                );
            }),
            false,
        )
        .await?;
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn snext_splits_window_and_edits_next_arg() -> anyhow::Result<()> {
    // :snext is `:split | :next` — with a two-file arg list, it splits the
    // window (1 -> 2 views) and edits the second file in the new split.
    let f1 = tempfile::NamedTempFile::new()?;
    let f2 = tempfile::NamedTempFile::new()?;
    let keys = format!(
        ":args {} {}<ret>:snext<ret>",
        f1.path().display(),
        f2.path().display()
    );
    // Step 1 splits and asserts 2 views; step 2 collapses back to one window so
    // the app can exit cleanly on teardown (leaving a split open deadlocks it).
    test_key_sequences(
        &mut AppBuilder::new().build()?,
        vec![
            (
                Some(keys.as_str()),
                Some(&|app| {
                    assert!(!app.editor.is_err(), "{:?}", app.editor.get_status());
                    assert_eq!(
                        app.editor.tree.views().count(),
                        2,
                        "snext should split the window"
                    );
                } as _),
            ),
            (Some(":wincmd o<ret>"), None),
        ],
        false,
    )
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn ptnext_shows_next_tag_in_preview_split() -> anyhow::Result<()> {
    // :ptnext shows the next tag match in a "preview" split. With two `foo`
    // matches, :tag selects the first; :ptnext opens the second in a split.
    use std::io::Write;
    let dir = tempfile::tempdir()?;
    let src = dir.path().join("u.c");
    let mut f = std::fs::File::create(&src)?;
    writeln!(f, "int foo() {{}}")?;
    writeln!(f, "int foo() {{}}")?;
    f.flush()?;
    let mut tags = std::fs::File::create(dir.path().join("tags"))?;
    write!(tags, "foo\tu.c\t1\n")?;
    write!(tags, "foo\tu.c\t2\n")?;
    tags.flush()?;
    let mut app = helpers::AppBuilder::new().with_file(&src, None).build()?;
    test_key_sequences(
        &mut app,
        vec![
            (
                Some(":tag foo<ret>:ptnext<ret>"),
                Some(&|app| {
                    assert!(!app.editor.is_err(), "{:?}", app.editor.get_status());
                    assert_eq!(
                        app.editor.tree.views().count(),
                        2,
                        "ptnext opens a preview split"
                    );
                } as _),
            ),
            (Some(":wincmd o<ret>"), None),
        ],
        false,
    )
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn pclose_closes_a_split() -> anyhow::Result<()> {
    test_key_sequence(
        &mut AppBuilder::new().build()?,
        Some(":vsplit<ret>:pclose<ret>"),
        Some(&|app| {
            assert!(!app.editor.is_err(), "{:?}", app.editor.get_status());
            assert_eq!(
                app.editor.tree.views().count(),
                1,
                "pclose closes the split"
            );
        }),
        false,
    )
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn startgreplace_enters_virtual_replace_mode() -> anyhow::Result<()> {
    test_key_sequence(
        &mut AppBuilder::new().build()?,
        Some(":startgreplace<ret>"),
        Some(&|app| {
            assert!(!app.editor.is_err());
            assert!(
                app.editor.overwrite,
                "virtual replace mode overtypes existing characters"
            );
        }),
        false,
    )
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn number_command_shows_numbered_lines_in_scratch() -> anyhow::Result<()> {
    // With the cursor on the first line, :number prints that line, numbered,
    // into a scratch buffer.
    test_key_sequence(
        &mut AppBuilder::new()
            .with_input_text("#[a|]#lpha\nbeta\n")
            .build()?,
        Some(":number<ret>"),
        Some(&|app| {
            assert!(!app.editor.is_err(), "{:?}", app.editor.get_status());
            let text = zemacs_view::doc!(app.editor).text().to_string();
            assert!(
                text.contains("1  alpha"),
                "scratch should contain the numbered first line, got: {text:?}"
            );
        }),
        false,
    )
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn set_number_toggles_line_numbers_gutter() -> anyhow::Result<()> {
    // :set nonumber hides the line-numbers gutter; :set number brings it back.
    // Unknown vim options (a real ~/.vimrc sets dozens) are accepted silently,
    // not errored — so sourcing never aborts.
    fn has_line_numbers(app: &zemacs_term::application::Application) -> bool {
        app.editor
            .config()
            .gutters
            .layout
            .iter()
            .any(|g| matches!(g, zemacs_view::editor::GutterType::LineNumbers))
    }
    test_key_sequences(
        &mut AppBuilder::new().build()?,
        vec![
            (
                Some(":set nonumber<ret>"),
                Some(&|app| {
                    assert!(!app.editor.is_err(), "{:?}", app.editor.get_status());
                    assert!(!has_line_numbers(app), "nonumber hides the gutter");
                } as _),
            ),
            (
                Some(":set backupdir=~/tmp<ret>:set number<ret>"),
                Some(&|app| {
                    assert!(!app.editor.is_err(), "unknown option must not error");
                    assert!(has_line_numbers(app), "number shows the gutter again");
                } as _),
            ),
        ],
        false,
    )
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn set_mouse_string_value_applies() -> anyhow::Result<()> {
    // vim `:set mouse=...` is a string option upstream but maps to a zemacs bool.
    // mouse defaults to true, so drive a real change: `:set mouse=` disables it
    // (empty value), then `:set mouse=a` re-enables — neither errors on parse.
    test_key_sequences(
        &mut AppBuilder::new().build()?,
        vec![
            (
                Some(":set mouse=<ret>"),
                Some(&|app| {
                    assert!(!app.editor.is_err(), "{:?}", app.editor.get_status());
                    assert!(!app.editor.config().mouse, "mouse= disables the mouse");
                } as _),
            ),
            (
                Some(":set mouse=a<ret>"),
                Some(&|app| {
                    assert!(!app.editor.is_err());
                    assert!(app.editor.config().mouse, "mouse=a enables the mouse");
                } as _),
            ),
        ],
        false,
    )
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn set_accepts_full_vimrc_option_surface() -> anyhow::Result<()> {
    // A real `:set all` / ~/.vimrc mixes options with editor equivalents and many
    // without; none may error, or sourcing the vimrc would abort.
    let opts = "autoindent expandtab ignorecase noswapfile nobackup smartcase \
                backspace=2 shiftwidth=4 tabstop=4 backupdir=~/tmp fileformat=unix \
                foldmethod=manual number relativenumber mouse=a signcolumn=no \
                laststatus=2 showtabline=2 updatetime=150 cpoptions=aAceFs_B \
                termguicolors linebreak scrolloff=0";
    let keys = format!(":set {opts}<ret>");
    test_key_sequence(
        &mut AppBuilder::new().build()?,
        Some(&keys),
        Some(&|app| {
            assert!(
                !app.editor.is_err(),
                "':set' over a full vimrc option surface errored: {:?}",
                app.editor.get_status()
            );
        }),
        false,
    )
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn set_signcolumn_no_hides_diagnostics_gutter() -> anyhow::Result<()> {
    test_key_sequence(
        &mut AppBuilder::new().build()?,
        Some(":set signcolumn=no<ret>"),
        Some(&|app| {
            assert!(!app.editor.is_err(), "{:?}", app.editor.get_status());
            let has_diag = app
                .editor
                .config()
                .gutters
                .layout
                .iter()
                .any(|g| matches!(g, zemacs_view::editor::GutterType::Diagnostics));
            assert!(!has_diag, "signcolumn=no hides the diagnostics gutter");
        }),
        false,
    )
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn set_maps_showtabline_fileformat_autoread() -> anyhow::Result<()> {
    // Options with real editor equivalents take effect, not just no-op.
    test_key_sequence(
        &mut AppBuilder::new().build()?,
        Some(":set showtabline=2 fileformat=unix autoread<ret>"),
        Some(&|app| {
            assert!(!app.editor.is_err(), "{:?}", app.editor.get_status());
            let c = app.editor.config();
            assert!(
                matches!(c.bufferline, zemacs_view::editor::BufferLine::Always),
                "showtabline=2 -> bufferline always"
            );
            assert!(
                matches!(
                    c.default_line_ending,
                    zemacs_view::editor::LineEndingConfig::LF
                ),
                "fileformat=unix -> line ending lf"
            );
            assert!(c.auto_reload, "autoread enables auto-reload");
        }),
        false,
    )
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn set_expandtab_shiftwidth_sets_document_indent() -> anyhow::Result<()> {
    // vim `:set expandtab shiftwidth=2` makes the current buffer indent with two
    // spaces; `:set noexpandtab` switches it back to tabs (buffer-local).
    use zemacs_core::indent::IndentStyle;
    test_key_sequences(
        &mut AppBuilder::new().build()?,
        vec![
            (
                Some(":set expandtab shiftwidth=2<ret>"),
                Some(&|app| {
                    assert!(!app.editor.is_err(), "{:?}", app.editor.get_status());
                    assert_eq!(
                        zemacs_view::doc!(app.editor).indent_style,
                        IndentStyle::Spaces(2)
                    );
                } as _),
            ),
            (
                Some(":set noexpandtab<ret>"),
                Some(&|app| {
                    assert_eq!(
                        zemacs_view::doc!(app.editor).indent_style,
                        IndentStyle::Tabs
                    );
                } as _),
            ),
        ],
        false,
    )
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn set_option_store_round_trips() -> anyhow::Result<()> {
    // Every option round-trips through the store, whether or not it has an editor
    // effect: set it, then `:set opt?` reports the value back (vim behavior for
    // inert options too).
    test_key_sequences(
        &mut AppBuilder::new().build()?,
        vec![
            (
                Some(":set backupdir=~/tmp<ret>:set backupdir?<ret>"),
                Some(&|app| {
                    assert!(!app.editor.is_err());
                    let (s, _) = app.editor.get_status().unwrap();
                    assert_eq!(s.as_ref(), "backupdir=~/tmp");
                } as _),
            ),
            (
                Some(":set nonumber<ret>:set number?<ret>"),
                Some(&|app| {
                    let (s, _) = app.editor.get_status().unwrap();
                    assert_eq!(s.as_ref(), "nonumber");
                } as _),
            ),
            (
                Some(":set shiftwidth=4<ret>:set shiftwidth?<ret>"),
                Some(&|app| {
                    let (s, _) = app.editor.get_status().unwrap();
                    assert_eq!(s.as_ref(), "shiftwidth=4");
                } as _),
            ),
        ],
        false,
    )
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn set_all_lists_the_full_option_surface() -> anyhow::Result<()> {
    test_key_sequence(
        &mut AppBuilder::new().build()?,
        Some(":set all<ret>"),
        Some(&|app| {
            assert!(!app.editor.is_err());
            let text = zemacs_view::doc!(app.editor).text().to_string();
            assert!(text.contains("autoindent"), "should list a boolean option");
            assert!(text.contains("backupdir="), "should list a valued option");
        }),
        false,
    )
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn set_tabstop_sets_document_tab_width() -> anyhow::Result<()> {
    // vim `:set tabstop=8` sets the current buffer's tab display width.
    test_key_sequence(
        &mut AppBuilder::new().build()?,
        Some(":set tabstop=8<ret>"),
        Some(&|app| {
            assert!(!app.editor.is_err(), "{:?}", app.editor.get_status());
            assert_eq!(zemacs_view::doc!(app.editor).tab_width(), 8);
        }),
        false,
    )
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn set_readonly_modifiable_toggles_document_flag() -> anyhow::Result<()> {
    // vim `:set readonly`/`modifiable`/`nomodifiable` toggle the buffer's
    // read-only flag (they are opposites).
    test_key_sequences(
        &mut AppBuilder::new().build()?,
        vec![
            (
                Some(":set readonly<ret>"),
                Some(&|app| {
                    assert!(!app.editor.is_err(), "{:?}", app.editor.get_status());
                    assert!(
                        zemacs_view::doc!(app.editor).readonly,
                        "readonly sets the flag"
                    );
                } as _),
            ),
            (
                Some(":set modifiable<ret>"),
                Some(&|app| {
                    assert!(
                        !zemacs_view::doc!(app.editor).readonly,
                        "modifiable clears it"
                    );
                } as _),
            ),
            (
                Some(":set nomodifiable<ret>"),
                Some(&|app| {
                    assert!(
                        zemacs_view::doc!(app.editor).readonly,
                        "nomodifiable sets it"
                    );
                } as _),
            ),
        ],
        false,
    )
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn set_colorcolumn_sets_rulers() -> anyhow::Result<()> {
    // vim `:set colorcolumn=80,100` renders vertical guides at columns 80 & 100.
    test_key_sequence(
        &mut AppBuilder::new().build()?,
        Some(":set colorcolumn=80,100<ret>"),
        Some(&|app| {
            assert!(!app.editor.is_err(), "{:?}", app.editor.get_status());
            assert_eq!(app.editor.config().rulers, vec![80u16, 100u16]);
        }),
        false,
    )
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn set_splitright_splitbelow_configure_split_placement() -> anyhow::Result<()> {
    // vim `splitright`/`splitbelow` (and the `no` forms) toggle the new split
    // placement config, which the window tree honors via split_with.
    test_key_sequences(
        &mut AppBuilder::new().build()?,
        vec![
            (
                Some(":set nosplitright<ret>"),
                Some(&|app| {
                    assert!(!app.editor.is_err(), "{:?}", app.editor.get_status());
                    assert!(!app.editor.config().split_right, "nosplitright clears it");
                } as _),
            ),
            (
                Some(":set splitright nosplitbelow<ret>"),
                Some(&|app| {
                    assert!(app.editor.config().split_right, "splitright sets it");
                    assert!(!app.editor.config().split_below, "nosplitbelow clears it");
                } as _),
            ),
        ],
        false,
    )
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn set_showbreak_sets_wrap_indicator() -> anyhow::Result<()> {
    // vim `:set showbreak=-->` sets the marker shown at the start of wrapped lines.
    test_key_sequence(
        &mut AppBuilder::new().build()?,
        Some(":set showbreak=+<ret>"),
        Some(&|app| {
            assert!(!app.editor.is_err(), "{:?}", app.editor.get_status());
            assert_eq!(
                app.editor.config().soft_wrap.wrap_indicator.as_deref(),
                Some("+")
            );
        }),
        false,
    )
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn set_listchars_sets_whitespace_characters() -> anyhow::Result<()> {
    // vim `:set listchars=tab:>-,space:.,eol:$` maps onto the whitespace render
    // characters (avoiding `-`/`>` which trip the harness key parser).
    test_key_sequence(
        &mut AppBuilder::new().build()?,
        Some(":set listchars=tab:x_,space:.,eol:$<ret>"),
        Some(&|app| {
            assert!(!app.editor.is_err(), "{:?}", app.editor.get_status());
            let ws = &app.editor.config().whitespace.characters;
            assert_eq!(ws.tab, 'x');
            assert_eq!(ws.tabpad, '_');
            assert_eq!(ws.space, '.');
            assert_eq!(ws.newline, '$');
        }),
        false,
    )
    .await?;
    Ok(())
}
