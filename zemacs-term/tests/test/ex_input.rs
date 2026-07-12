use super::*;

use zemacs_term::application::Application;

fn buffer_text(app: &Application) -> String {
    app.editor.documents().next().unwrap().text().to_string()
}

/// vim `:append` — the Ex line-input mode collects typed lines until a lone `.`
/// and inserts them after the current line as one edit. The command and the line
/// input are separate key batches so the event loop pushes the `ExInput` layer
/// (an async job) before the lines are typed — mirroring real interactive use,
/// where an event-loop tick separates the command from the next keystroke.
#[tokio::test(flavor = "multi_thread")]
async fn ex_append_inserts_typed_lines() -> anyhow::Result<()> {
    test_key_sequences(
        &mut AppBuilder::new().build()?,
        vec![
            (Some(":append<ret>"), None),
            (
                Some("foo<ret>bar<ret>.<ret>"),
                Some(&|app: &Application| {
                    assert!(
                        !app.editor.is_err(),
                        "append errored: {:?}",
                        app.editor.get_status()
                    );
                    let text = buffer_text(app);
                    assert!(
                        text.contains("foo\nbar"),
                        "appended lines not present in order, buffer: {text:?}"
                    );
                }),
            ),
        ],
        false,
    )
    .await?;
    Ok(())
}

/// vim `:change` — replace the current line with the typed lines.
#[tokio::test(flavor = "multi_thread")]
async fn ex_change_replaces_current_line() -> anyhow::Result<()> {
    test_key_sequences(
        &mut AppBuilder::new().build()?,
        vec![
            (Some("iHELLO<esc>:change<ret>"), None),
            (
                Some("NEWLINE<ret>.<ret>"),
                Some(&|app: &Application| {
                    let text = buffer_text(app);
                    assert!(text.contains("NEWLINE"), "change did not insert new text: {text:?}");
                    assert!(!text.contains("HELLO"), "change did not remove old line: {text:?}");
                }),
            ),
        ],
        false,
    )
    .await?;
    Ok(())
}

/// Esc aborts Ex line-input: nothing is inserted, and for `:change` the line is
/// left intact (the delete is part of the commit that never runs).
#[tokio::test(flavor = "multi_thread")]
async fn ex_change_esc_aborts_and_keeps_line() -> anyhow::Result<()> {
    test_key_sequences(
        &mut AppBuilder::new().build()?,
        vec![
            (Some("iKEEPME<esc>:change<ret>"), None),
            (
                Some("DISCARD<esc>"),
                Some(&|app: &Application| {
                    let text = buffer_text(app);
                    assert!(text.contains("KEEPME"), "aborted :change should keep the line: {text:?}");
                    assert!(!text.contains("DISCARD"), "aborted :change must not insert: {text:?}");
                }),
            ),
        ],
        false,
    )
    .await?;
    Ok(())
}
