use super::*;

use zmax_term::config::Config;

fn vim() -> AppBuilder {
    AppBuilder::new().with_config(Config {
        keys: zmax_term::keymap::vim::default(),
        ..Default::default()
    })
}

fn cursor_line(app: &zmax_term::application::Application) -> usize {
    let (view, doc) = zmax_view::current_ref!(app.editor);
    let text = doc.text().slice(..);
    let cursor = doc.selection(view.id).primary().cursor(text);
    text.char_to_line(cursor)
}

fn buffer(app: &zmax_term::application::Application) -> String {
    let (_, doc) = zmax_view::current_ref!(app.editor);
    doc.text().to_string()
}

#[tokio::test(flavor = "multi_thread")]
async fn gq_reflows_to_textwidth() -> anyhow::Result<()> {
    // `gqq` reflows the current line to 'text-width' (vim gq), hard-wrapping words.
    let mut app = vim()
        .with_input_text("#[|aa bb cc dd ee ff gg]#\n")
        .build()?;
    test_key_sequences(
        &mut app,
        vec![
            (Some(":set textwidth=15<ret>"), None),
            (
                Some("gqq"),
                Some(&|app| {
                    assert!(!app.editor.is_err(), "{:?}", app.editor.get_status());
                    let text = buffer(app);
                    // words are preserved and in order
                    assert_eq!(
                        text.split_whitespace().collect::<Vec<_>>(),
                        vec!["aa", "bb", "cc", "dd", "ee", "ff", "gg"]
                    );
                    // the line was actually wrapped, and every line fits text-width
                    assert!(text.lines().count() >= 2, "should wrap: {text:?}");
                    for line in text.lines() {
                        assert!(line.chars().count() <= 15, "line too long: {line:?}");
                    }
                }),
            ),
        ],
        false,
    )
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn gw_reflows_and_keeps_cursor_at_start() -> anyhow::Result<()> {
    // `gww` reflows like gq but restores the cursor to the start (line 0), whereas
    // gq leaves it at the end.
    let mut app = vim()
        .with_input_text("#[|aa bb cc dd ee ff gg]#\n")
        .build()?;
    test_key_sequences(
        &mut app,
        vec![
            (Some(":set textwidth=15<ret>"), None),
            (
                Some("gww"),
                Some(&|app| {
                    assert!(!app.editor.is_err(), "{:?}", app.editor.get_status());
                    assert!(buffer(app).lines().count() >= 2, "gw should reflow too");
                    assert_eq!(cursor_line(app), 0, "gw restores the cursor to the start");
                }),
            ),
        ],
        false,
    )
    .await?;
    Ok(())
}
