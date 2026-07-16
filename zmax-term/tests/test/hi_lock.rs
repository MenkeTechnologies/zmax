use super::*;

use zmax_term::application::Application;

/// End-to-end Hi-Lock: drive `:highlight-regexp` through the real command
/// dispatch over a buffer that contains the pattern, so the event loop renders a
/// frame with an active pattern (exercising `EditorView::doc_hilock_highlights`
/// on non-empty matches — a render panic would fail here). Then `:unhighlight-
/// regexp` with no argument must clear every pattern. One test, sequential
/// steps, since the pattern store is process-global.
#[tokio::test(flavor = "multi_thread")]
async fn highlight_then_unhighlight_regexp() -> anyhow::Result<()> {
    zmax_term::hi_lock::clear();

    test_key_sequences(
        &mut AppBuilder::new().build()?,
        vec![
            (
                Some("ithe cat sat on the mat<esc>:highlight-regexp the<ret>"),
                Some(&|app: &Application| {
                    assert!(
                        !app.editor.is_err(),
                        "highlight-regexp errored: {:?}",
                        app.editor.get_status()
                    );
                    assert!(
                        zmax_term::hi_lock::sources().iter().any(|s| s == "the"),
                        "pattern not stored: {:?}",
                        zmax_term::hi_lock::sources()
                    );
                }),
            ),
            (
                Some(":unhighlight-regexp<ret>"),
                Some(&|app: &Application| {
                    assert!(!app.editor.is_err());
                    assert!(
                        zmax_term::hi_lock::is_empty(),
                        "patterns not cleared: {:?}",
                        zmax_term::hi_lock::sources()
                    );
                }),
            ),
        ],
        false,
    )
    .await?;

    zmax_term::hi_lock::clear();
    Ok(())
}
