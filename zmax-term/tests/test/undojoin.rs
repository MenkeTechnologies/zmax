use super::*;

use zmax_term::application::Application;

/// End-to-end vim `:undojoin`: two edits separated by an undo boundary (`<esc>`
/// then `o`) that would normally be two undo blocks are joined by `:undojoin`, so
/// a single `u` reverts BOTH. Without the join, one `u` would revert only the
/// second edit — asserting that neither edit survives the single undo pins the
/// join behavior.
#[tokio::test(flavor = "multi_thread")]
async fn undojoin_merges_next_change_into_previous_block() -> anyhow::Result<()> {
    test_key_sequences(
        &mut AppBuilder::new().build()?,
        vec![
            (
                // First edit (its own block after <esc>), then arm the join and
                // make a second edit on a new line.
                Some("ifoo<esc>:undojoin<ret>obar<esc>"),
                Some(&|app: &Application| {
                    assert!(
                        !app.editor.is_err(),
                        "setup errored: {:?}",
                        app.editor.get_status()
                    );
                    let text = app.editor.documents().next().unwrap().text().to_string();
                    assert!(
                        text.contains("foo") && text.contains("bar"),
                        "both edits present: {text:?}"
                    );
                }),
            ),
            (
                // A single undo must revert BOTH joined edits.
                Some("u"),
                Some(&|app: &Application| {
                    let text = app.editor.documents().next().unwrap().text().to_string();
                    assert!(
                        !text.contains("foo") && !text.contains("bar"),
                        "one undo should revert both joined edits, got: {text:?}"
                    );
                }),
            ),
        ],
        false,
    )
    .await?;
    Ok(())
}
