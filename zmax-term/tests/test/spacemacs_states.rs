use super::*;

/// Spacemacs `SPC z x` is a *transient state*: the first `+` scales and the
/// state stays up, so bare `+`/`k` keep scaling until `q` (or any other key)
/// leaves it. Without the sticky node each key would have been a one-shot and
/// the second `+` would have hit Normal mode's `increment` instead.
#[tokio::test(flavor = "multi_thread")]
async fn font_scaling_transient_state_keeps_scaling() -> anyhow::Result<()> {
    let mut app = preset_app("spacemacs")
        .with_input_text("#[a|]#bc\n")
        .build()?;
    test_key_sequences(
        &mut app,
        vec![
            (
                // Enter with `+`, then stay: `+` again, then the `k` alias.
                Some("<space>zx++k"),
                Some(&|app: &zmax_term::application::Application| {
                    assert_eq!(app.editor.text_scale, 3, "three steps up in the state");
                }),
            ),
            (
                Some("0"),
                Some(&|app: &zmax_term::application::Application| {
                    assert_eq!(app.editor.text_scale, 0, "0 resets without leaving");
                }),
            ),
            (
                // `q` leaves the state; the buffer is untouched throughout.
                Some("q"),
                Some(&|app: &zmax_term::application::Application| {
                    let (_v, doc) = zmax_view::current_ref!(app.editor);
                    assert_eq!(doc.text().to_string(), "abc\n", "scaling never edits");
                }),
            ),
        ],
        false,
    )
    .await?;
    Ok(())
}

/// The same shape for `SPC z f` (`zoom-frm-in`/`out`/`unzoom`), which keeps its
/// own counter so `SPC z f 0` reports and resets the steps that family took.
#[tokio::test(flavor = "multi_thread")]
async fn frame_scaling_transient_state_keeps_zooming() -> anyhow::Result<()> {
    let mut app = preset_app("spacemacs")
        .with_input_text("#[a|]#bc\n")
        .build()?;
    test_key_sequences(
        &mut app,
        vec![
            (
                Some("<space>zf--j"),
                Some(&|app: &zmax_term::application::Application| {
                    assert_eq!(app.editor.frame_scale, -3, "three steps out in the state");
                }),
            ),
            (
                Some("0"),
                Some(&|app: &zmax_term::application::Application| {
                    assert_eq!(app.editor.frame_scale, 0, "0 resets without leaving");
                }),
            ),
        ],
        false,
    )
    .await?;
    Ok(())
}
