//! Editor actions ported from the JetBrains action registry. Each command is
//! bound to a plain key here, since the harness drives keys rather than names.

use super::*;

use zmax_core::hashmap;
use zmax_term::{application::Application, keymap};
use zmax_view::document::Mode;

/// An app over `text` with `command` bound to `Z` in normal mode.
fn app_with(text: &str, command: &str) -> anyhow::Result<Application> {
    let mut config = Config::default();
    let keys = match command {
        "split_line" => keymap!({ "Normal mode" "Z" => split_line, }),
        "cut_to_line_end" => keymap!({ "Normal mode" "Z" => cut_to_line_end, }),
        "cut_to_line_start" => keymap!({ "Normal mode" "Z" => cut_to_line_start, }),
        "extend_to_block_start" => keymap!({ "Normal mode" "Z" => extend_to_block_start, }),
        "extend_to_block_end" => keymap!({ "Normal mode" "Z" => extend_to_block_end, }),
        "extend_to_window_bottom" => keymap!({ "Normal mode" "Z" => extend_to_window_bottom, }),
        other => panic!("no binding for {other}"),
    };
    config.keys.insert(Mode::Normal, keys);
    AppBuilder::new()
        .with_config(config)
        .with_input_text(text)
        .build()
}

fn text(app: &Application) -> String {
    app.editor.documents().next().unwrap().text().to_string()
}

/// `(anchor, head)` of the primary selection.
fn primary(app: &Application) -> (usize, usize) {
    let (view, doc) = zmax_view::current_ref!(app.editor);
    let range = doc.selection(view.id).primary();
    (range.anchor, range.head)
}

/// Split Line breaks at the cursor and leaves the cursor before the break, so
/// the text after it moves down and the cursor does not.
#[tokio::test(flavor = "multi_thread")]
async fn split_line_keeps_the_cursor_before_the_break() -> anyhow::Result<()> {
    let mut app = app_with("ab#[c|]#d\n", "split_line")?;
    test_key_sequences(
        &mut app,
        vec![(
            Some("Z"),
            Some(&|app| {
                assert_eq!("ab\ncd\n", text(app));
                // Normal mode widens the cursor over the break it sits on.
                assert_eq!((2, 3), primary(app));
            }),
        )],
        false,
    )
    .await
}

/// Cut up to Line End takes the rest of the line; on the line break it takes
/// the break, which joins the next line on.
#[tokio::test(flavor = "multi_thread")]
async fn cut_to_line_end_takes_the_rest_then_the_break() -> anyhow::Result<()> {
    let mut app = app_with("ab#[c|]#d\nef\n", "cut_to_line_end")?;
    test_key_sequences(
        &mut app,
        vec![
            (Some("Z"), Some(&|app| assert_eq!("ab\nef\n", text(app)))),
            (Some("Z"), Some(&|app| assert_eq!("abef\n", text(app)))),
        ],
        false,
    )
    .await
}

/// Cut Line Backward takes the line up to the cursor and nothing after it.
#[tokio::test(flavor = "multi_thread")]
async fn cut_to_line_start_takes_the_text_before_the_cursor() -> anyhow::Result<()> {
    let mut app = app_with("xy\nab#[c|]#d\n", "cut_to_line_start")?;
    test_key_sequences(
        &mut app,
        vec![(Some("Z"), Some(&|app| assert_eq!("xy\ncd\n", text(app))))],
        false,
    )
    .await
}

/// The block-edge motions extend from where the cursor was in normal mode too,
/// where `[{` / `]}` would only move.
#[tokio::test(flavor = "multi_thread")]
async fn block_edge_selection_extends_in_normal_mode() -> anyhow::Result<()> {
    let mut app = app_with("f { a#[b|]#c }\n", "extend_to_block_end")?;
    test_key_sequences(
        &mut app,
        vec![(Some("Z"), Some(&|app| assert_eq!((5, 9), primary(app))))],
        false,
    )
    .await?;

    let mut app = app_with("f { a#[b|]#c }\n", "extend_to_block_start")?;
    test_key_sequences(
        &mut app,
        vec![(
            Some("Z"),
            Some(&|app| {
                let (anchor, head) = primary(app);
                assert_eq!(2, head.min(anchor));
                assert!(head.max(anchor) >= 5, "selection kept its start at b");
            }),
        )],
        false,
    )
    .await
}

/// Page Bottom with Selection extends to the last visible line instead of
/// moving there.
#[tokio::test(flavor = "multi_thread")]
async fn window_bottom_selection_extends_in_normal_mode() -> anyhow::Result<()> {
    let mut app = app_with("#[a|]#\nb\nc\n", "extend_to_window_bottom")?;
    test_key_sequences(
        &mut app,
        vec![(
            Some("Z"),
            Some(&|app| {
                let (anchor, head) = primary(app);
                assert_eq!(0, anchor);
                assert!(head >= 4, "head reached the last line: {head}");
            }),
        )],
        false,
    )
    .await
}
