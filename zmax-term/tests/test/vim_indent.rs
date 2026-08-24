use super::*;

use zmax_term::config::Config;

// Visual-mode `>` / `<`. Two things separate zmax from vim here, both aimed at
// the same workflow — shifting a block several levels:
//
//   * the highlighted area survives the shift, so `>>>>>` stacks (vim exits
//     Visual mode and needs `gv` between shifts, which is what the near
//     universal `vnoremap > >gv` works around);
//   * a count is levels, not lines (change.txt:511 `{Visual}[count]>`), which
//     zmax's operator path deliberately reads as lines for `3>>`.
fn vim() -> AppBuilder {
    preset_app("vim")
}

fn doc_text(app: &zmax_term::application::Application) -> String {
    zmax_view::doc!(app.editor).text().to_string()
}

#[tokio::test(flavor = "multi_thread")]
async fn visual_indent_repeats_without_reselecting() -> anyhow::Result<()> {
    let mut app = vim()
        .with_input_text(indoc! {"\
            #[o|]#ne
            two
            three"})
        .build()?;
    test_key_sequences(
        &mut app,
        vec![
            // Vj highlights the first two lines, then three shifts stack up.
            (
                Some("Vj>>>"),
                Some(&|app: &zmax_term::application::Application| {
                    assert_eq!(
                        doc_text(app),
                        // Scratch buffers indent with tabs, one per level.
                        "\t\t\tone\n\t\t\ttwo\nthree",
                        "three `>` presses shift the same two lines three levels"
                    );
                    assert_eq!(
                        app.editor.mode,
                        zmax_view::document::Mode::Select,
                        "the highlighted area is still highlighted after `>`"
                    );
                }),
            ),
            // ...and `<` walks the same block back down.
            (
                Some("<lt><lt>"),
                Some(&|app: &zmax_term::application::Application| {
                    assert_eq!(
                        doc_text(app),
                        "\tone\n\ttwo\nthree",
                        "two `<` presses undo two of the three levels"
                    );
                }),
            ),
        ],
        false,
    )
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn visual_indent_count_is_levels() -> anyhow::Result<()> {
    let mut app = vim()
        .with_input_text(indoc! {"\
            #[o|]#ne
            two"})
        .build()?;
    test_key_sequence(
        &mut app,
        // change.txt:511 — `{Visual}[count]>` shifts by [count] 'shiftwidth',
        // so `3>` is one keystroke for what `>>>` does above.
        Some("Vj3>"),
        Some(&|app| {
            assert_eq!(
                doc_text(app),
                "\t\t\tone\n\t\t\ttwo",
                "`3>` in Visual shifts three levels, not three lines by one"
            );
        }),
        false,
    )
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn normal_operator_count_still_means_lines() -> anyhow::Result<()> {
    let mut app = vim()
        .with_input_text(indoc! {"\
            #[o|]#ne
            two
            three"})
        .build()?;
    test_key_sequence(
        &mut app,
        // The operator form keeps vim's own reading: `2>>` is two LINES by one
        // level. Only the Visual form counts levels.
        Some("2>>"),
        Some(&|app| {
            assert_eq!(
                doc_text(app),
                "\tone\n\ttwo\nthree",
                "`2>>` shifts two lines one level"
            );
        }),
        false,
    )
    .await?;
    Ok(())
}
