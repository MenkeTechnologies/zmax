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
