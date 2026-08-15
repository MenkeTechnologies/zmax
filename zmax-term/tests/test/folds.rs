use super::*;

/// A buffer whose fold `foldmethod=marker` can find. Markers keep these tests
/// independent of tree-sitter and of the indent settings, and keep the
/// (process-global) 'foldmethod' from creating folds in any other test's buffer
/// while it is set.
fn marker_file() -> anyhow::Result<tempfile::NamedTempFile> {
    temp_file_with_contents("head\nopen {{{\nbody\nclose }}}\ntail\n")
}

/// The fold column of a buffer is drawn from that buffer's own fold set, so a
/// buffer with no folds renders it blank. `:set foldmethod=` only rebuilt the
/// *current* document's folds, and zmax opens the files named on the command
/// line before it runs the init scripts — so a 'foldmethod' set in a vimrc left
/// every buffer but the current one with an empty fold column until a `z`
/// command in it happened to compute the folds.
#[tokio::test(flavor = "multi_thread")]
async fn foldmethod_populates_every_open_buffer() -> anyhow::Result<()> {
    let first = marker_file()?;
    let second = marker_file()?;

    let mut app = AppBuilder::new()
        .with_file(first.path(), None)
        .with_file(second.path(), None)
        .build()?;

    test_key_sequences(
        &mut app,
        vec![
            (
                Some(":set foldmethod=marker<ret>"),
                Some(&|app: &zmax_term::application::Application| {
                    let counts: Vec<usize> =
                        app.editor.documents().map(|d| d.folds().len()).collect();
                    assert_eq!(counts.len(), 2, "both files open, got {counts:?}");
                    assert!(
                        counts.iter().all(|n| *n == 1),
                        "every open buffer has its marker fold, got {counts:?}"
                    );
                }),
            ),
            // Leave the process-global option as the rest of the suite expects.
            (Some(":set foldmethod=manual<ret>"), None),
        ],
        false,
    )
    .await?;
    Ok(())
}

/// A buffer opened *after* 'foldmethod' is set must come up with its folds
/// already built — vim evaluates 'foldmethod' when the buffer is loaded, not on
/// the first fold command in it.
#[tokio::test(flavor = "multi_thread")]
async fn buffer_opened_later_gets_its_folds() -> anyhow::Result<()> {
    let first = marker_file()?;
    let second = marker_file()?;
    let open_second = format!(":open {}<ret>", second.path().to_string_lossy());

    let mut app = AppBuilder::new().with_file(first.path(), None).build()?;

    test_key_sequences(
        &mut app,
        vec![
            (Some(":set foldmethod=marker<ret>"), None),
            (
                Some(&open_second),
                Some(&|app: &zmax_term::application::Application| {
                    assert_eq!(
                        app.editor.documents().count(),
                        2,
                        "the second file should be open"
                    );
                    let (_view, doc) = zmax_view::current_ref!(app.editor);
                    assert_eq!(
                        doc.folds().len(),
                        1,
                        "a buffer opened under foldmethod=marker already has its fold"
                    );
                }),
            ),
            (Some(":set foldmethod=manual<ret>"), None),
        ],
        false,
    )
    .await?;
    Ok(())
}
