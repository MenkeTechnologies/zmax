use super::*;

use zmax_core::hashmap;
use zmax_term::keymap;

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

/// JetBrains Expand to Level N acts on the fold around the cursor only: it
/// opens that fold, closes the ones nested deeper than the count, and leaves
/// the other top-level fold as it was.
#[tokio::test(flavor = "multi_thread")]
async fn fold_block_to_level_leaves_other_blocks_alone() -> anyhow::Result<()> {
    let file = temp_file_with_contents("a {{{\nb {{{\nc\nd }}}\ne }}}\nf {{{\ng\nh }}}\n")?;
    let mut config = Config::default();
    config.keys.insert(
        zmax_view::document::Mode::Normal,
        keymap!({ "Normal mode" "Z" => fold_block_to_level, }),
    );
    let mut app = AppBuilder::new()
        .with_config(config)
        .with_file(file.path(), None)
        .build()?;

    test_key_sequences(
        &mut app,
        vec![
            (
                Some(":set foldmethod=marker<ret>zRggZ"),
                Some(&|app: &zmax_term::application::Application| {
                    let closed: Vec<(usize, usize)> = zmax_view::doc!(app.editor)
                        .folds()
                        .iter()
                        .filter(|f| f.closed)
                        .map(|f| (f.start, f.end))
                        .collect();
                    assert_eq!(closed, vec![(1, 3)]);
                }),
            ),
            (Some(":set foldmethod=manual<ret>"), None),
        ],
        false,
    )
    .await
}
