use super::*;

use zemacs_term::config::Config;

fn vim() -> AppBuilder {
    AppBuilder::new().with_config(Config {
        keys: zemacs_term::keymap::vim::default(),
        ..Default::default()
    })
}

/// The buffer line the primary cursor is on.
fn cursor_line(app: &zemacs_term::application::Application) -> usize {
    let (view, doc) = zemacs_view::current_ref!(app.editor);
    let text = doc.text().slice(..);
    let cursor = doc.selection(view.id).primary().cursor(text);
    text.char_to_line(cursor)
}

#[tokio::test(flavor = "multi_thread")]
async fn changelist_walks_older_and_newer() -> anyhow::Result<()> {
    // Edit lines 0, 2, 3 (each `x` deletes a char), then walk the changelist:
    // `g;` steps to older edits, `g,` back toward newer ones.
    let mut app = vim()
        .with_input_text("#[o|]#ne\ntwo\nthree\nfour")
        .build()?;
    test_key_sequences(
        &mut app,
        vec![
            (
                Some("xjjxjx"),
                Some(&|app| {
                    assert_eq!(cursor_line(app), 3, "last edit is on line 3");
                }),
            ),
            (
                Some("g;"),
                Some(&|app| {
                    assert_eq!(cursor_line(app), 3, "g; -> newest change (line 3)");
                }),
            ),
            (
                Some("g;"),
                Some(&|app| {
                    assert_eq!(cursor_line(app), 2, "g; -> line 2");
                }),
            ),
            (
                Some("g;"),
                Some(&|app| {
                    assert_eq!(cursor_line(app), 0, "g; -> line 0 (oldest)");
                }),
            ),
            (
                Some("g;"),
                Some(&|app| {
                    assert_eq!(cursor_line(app), 0, "g; at oldest stays put");
                }),
            ),
            (
                Some("g,"),
                Some(&|app| {
                    assert_eq!(cursor_line(app), 2, "g, -> newer (line 2)");
                }),
            ),
            (
                Some("g,"),
                Some(&|app| {
                    assert_eq!(cursor_line(app), 3, "g, -> newest (line 3)");
                }),
            ),
        ],
        false,
    )
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn changelist_count_steps_multiple() -> anyhow::Result<()> {
    // `2g;` steps back two entries at once.
    let mut app = vim()
        .with_input_text("#[o|]#ne\ntwo\nthree\nfour")
        .build()?;
    test_key_sequences(
        &mut app,
        vec![
            (Some("xjjxjx"), None),
            (
                Some("2g;"),
                Some(&|app| {
                    // from "after newest": 2g; -> second-oldest step -> line 2.
                    assert_eq!(cursor_line(app), 2, "2g; steps back two -> line 2");
                }),
            ),
        ],
        false,
    )
    .await?;
    Ok(())
}
