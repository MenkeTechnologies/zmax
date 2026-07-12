use super::*;

use zemacs_term::application::Application;

/// emacs `tab-undo` — closing a tab pushes it onto the closed-tab stack, and
/// `tab-undo` reopens it, restoring the tab count.
#[tokio::test(flavor = "multi_thread")]
async fn tab_undo_reopens_closed_tab() -> anyhow::Result<()> {
    test_key_sequences(
        &mut AppBuilder::new().build()?,
        vec![(
            Some(":tabnew<ret>:tabclose<ret>:tab-undo<ret>"),
            Some(&|app: &Application| {
                assert!(!app.editor.is_err(), "errored: {:?}", app.editor.get_status());
                assert_eq!(
                    app.editor.tab_count(),
                    2,
                    "tab-undo should restore the closed tab"
                );
            }),
        )],
        false,
    )
    .await?;
    Ok(())
}

/// emacs `tab-rename` + `tab-switch` — a named tab can be switched to by name.
#[tokio::test(flavor = "multi_thread")]
async fn tab_rename_then_switch_by_name() -> anyhow::Result<()> {
    test_key_sequences(
        &mut AppBuilder::new().build()?,
        vec![(
            Some(":tab-rename alpha<ret>:tabnew<ret>:tab-switch alpha<ret>"),
            Some(&|app: &Application| {
                assert_eq!(app.editor.current_tab(), 0, "should switch to the 'alpha' tab");
                assert_eq!(app.editor.current_tab_name(), Some("alpha"));
            }),
        )],
        false,
    )
    .await?;
    Ok(())
}

/// emacs `tab-bar-history-back`/`-forward` — after a `tab-switch`, history-back
/// returns to the tab we left, and history-forward re-visits.
#[tokio::test(flavor = "multi_thread")]
async fn tab_bar_history_back_and_forward() -> anyhow::Result<()> {
    test_key_sequences(
        &mut AppBuilder::new().build()?,
        vec![
            (
                // 3 tabs (current = 2), switch to tab 1 (index 0) recording history,
                // then step back to the tab we left (index 2).
                Some(":tabnew<ret>:tabnew<ret>:tab-switch 1<ret>:tab-bar-history-back<ret>"),
                Some(&|app: &Application| {
                    assert_eq!(
                        app.editor.current_tab(),
                        2,
                        "history-back returns to the tab left by tab-switch"
                    );
                }),
            ),
            (
                Some(":tab-bar-history-forward<ret>"),
                Some(&|app: &Application| {
                    assert_eq!(
                        app.editor.current_tab(),
                        0,
                        "history-forward re-visits the tab-switch target"
                    );
                }),
            ),
        ],
        false,
    )
    .await?;
    Ok(())
}
