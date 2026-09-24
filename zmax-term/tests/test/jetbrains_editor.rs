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
        "md_table_insert_row_below" => keymap!({ "Normal mode" "Z" => md_table_insert_row_below, }),
        "md_table_move_column_right" => keymap!({ "Normal mode" "Z" => md_table_move_column_right, }),
        "md_table_align_right" => keymap!({ "Normal mode" "Z" => md_table_align_right, }),
        "md_toggle_bold" => keymap!({ "Normal mode" "Z" => md_toggle_bold, }),
        "md_heading_up" => keymap!({ "Normal mode" "Z" => md_heading_up, }),
        "move_down_and_scroll" => keymap!({ "Normal mode" "Z" => move_down_and_scroll, }),
        "editor_escape" => keymap!({ "Normal mode" "Z" => editor_escape, }),
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

fn cursor_line(app: &Application) -> usize {
    let (view, doc) = zmax_view::current_ref!(app.editor);
    let text = doc.text().slice(..);
    text.char_to_line(doc.selection(view.id).primary().cursor(text))
}

/// Mnemonic bookmarks: toggling sets one, a second line takes the mnemonic
/// over, toggling on its own line removes it, and goto follows it.
#[tokio::test(flavor = "multi_thread")]
async fn mnemonic_bookmark_moves_and_toggles_off() -> anyhow::Result<()> {
    let file = tempfile::Builder::new().suffix(".txt").tempfile()?;
    std::fs::write(file.path(), "a\nb\nc\n")?;
    let mut config = Config::default();
    config.keys.insert(
        Mode::Normal,
        keymap!({ "Normal mode"
            "Z" => toggle_bookmark_7,
            "Q" => goto_bookmark_7,
            "M" => toggle_bookmark_with_mnemonic,
        }),
    );
    let mut app = AppBuilder::new()
        .with_config(config)
        .with_file(file.path(), None)
        .build()?;

    test_key_sequences(
        &mut app,
        vec![
            (Some("jjZggQ"), Some(&|app| assert_eq!(2, cursor_line(app)))),
            // The prompt form moves 7 to line 0.
            (Some("ggM7jjQ"), Some(&|app| assert_eq!(0, cursor_line(app)))),
            (
                Some("ZjQ"),
                Some(&|app| {
                    assert_eq!(1, cursor_line(app), "goto did not move");
                    let (status, _) = app.editor.get_status().expect("a status");
                    assert_eq!("No bookmark 7", status);
                }),
            ),
        ],
        false,
    )
    .await
}

/// A markdown table edit rewrites the table aligned and leaves the cursor in
/// the cell the edit is about.
#[tokio::test(flavor = "multi_thread")]
async fn md_table_row_below_is_aligned_and_takes_the_cursor() -> anyhow::Result<()> {
    let mut app = app_with("| a | bb |\n|---|---|\n| #[1|]# | 2 |\n", "md_table_insert_row_below")?;
    test_key_sequences(
        &mut app,
        vec![(
            Some("Z"),
            Some(&|app| {
                assert_eq!(
                    "| a   | bb  |\n| --- | --- |\n| 1   | 2   |\n|     |     |\n",
                    text(app)
                );
                assert_eq!(3, cursor_line(app));
            }),
        )],
        false,
    )
    .await
}

/// Moving a column carries every row's cell, the separator row included.
#[tokio::test(flavor = "multi_thread")]
async fn md_table_column_moves_right_with_its_cells() -> anyhow::Result<()> {
    let mut app = app_with("| #[a|]# | b |\n| --- | --: |\n| 1 | 2 |\n", "md_table_move_column_right")?;
    test_key_sequences(
        &mut app,
        vec![(
            Some("Z"),
            Some(&|app| assert_eq!("|   b | a   |\n| --: | --- |\n|   2 | 1   |\n", text(app))),
        )],
        false,
    )
    .await
}

/// Right alignment pads the column's cells on the left.
#[tokio::test(flavor = "multi_thread")]
async fn md_table_align_right_pads_on_the_left() -> anyhow::Result<()> {
    let mut app = app_with("| #[a|]# |\n| --- |\n| 1 |\n", "md_table_align_right")?;
    test_key_sequences(
        &mut app,
        vec![(Some("Z"), Some(&|app| assert_eq!("|   a |\n| --: |\n|   1 |\n", text(app))))],
        false,
    )
    .await
}

/// Bold wraps the word under a cursor.
#[tokio::test(flavor = "multi_thread")]
async fn md_bold_wraps_the_word_under_the_cursor() -> anyhow::Result<()> {
    let mut app = app_with("say h#[e|]#llo\n", "md_toggle_bold")?;
    test_key_sequences(
        &mut app,
        vec![(Some("Z"), Some(&|app| assert_eq!("say **hello**\n", text(app))))],
        false,
    )
    .await
}

/// Increase Header Level takes a `#` away, as the IDE's `level - 1` does.
#[tokio::test(flavor = "multi_thread")]
async fn md_heading_up_removes_a_hash() -> anyhow::Result<()> {
    let mut app = app_with("## T#[i|]#tle\n", "md_heading_up")?;
    test_key_sequences(
        &mut app,
        vec![(Some("Z"), Some(&|app| assert_eq!("# Title\n", text(app))))],
        false,
    )
    .await
}

/// Breakpoints on this buffer's file, as `(line, disabled)`.
fn breakpoints(app: &Application) -> Vec<(usize, bool)> {
    let doc = app.editor.documents().next().unwrap();
    let path = doc.path().unwrap();
    app.editor
        .breakpoints
        .get(path)
        .map(|bs| bs.iter().map(|b| (b.line, b.disabled)).collect())
        .unwrap_or_default()
}

/// Disabling keeps a breakpoint in place; removing every breakpoint in the
/// file and restoring brings back the last one removed, disabled as it was.
#[tokio::test(flavor = "multi_thread")]
async fn breakpoints_disable_remove_and_restore() -> anyhow::Result<()> {
    let file = tempfile::Builder::new().suffix(".txt").tempfile()?;
    std::fs::write(file.path(), "a\nb\nc\n")?;
    let mut config = Config::default();
    config.keys.insert(
        Mode::Normal,
        keymap!({ "Normal mode"
            "Z" => dap_toggle_breakpoint,
            "Q" => dap_toggle_breakpoint_enabled,
            "M" => dap_remove_breakpoints_in_file,
            "R" => dap_restore_breakpoint,
        }),
    );
    let mut app = AppBuilder::new()
        .with_config(config)
        .with_file(file.path(), None)
        .build()?;

    test_key_sequences(
        &mut app,
        vec![
            (Some("ZjZQ"), Some(&|app| assert_eq!(vec![(0, false), (1, true)], breakpoints(app)))),
            (Some("M"), Some(&|app| assert_eq!(Vec::<(usize, bool)>::new(), breakpoints(app)))),
            (Some("R"), Some(&|app| assert_eq!(vec![(1, true)], breakpoints(app)))),
        ],
        false,
    )
    .await
}

/// Next Line Bookmark in Editor walks this file's bookmarks and wraps.
#[tokio::test(flavor = "multi_thread")]
async fn bookmark_cycle_in_file_wraps() -> anyhow::Result<()> {
    let file = tempfile::Builder::new().suffix(".txt").tempfile()?;
    std::fs::write(file.path(), "a\nb\nc\nd\n")?;
    let mut config = Config::default();
    config.keys.insert(
        Mode::Normal,
        keymap!({ "Normal mode"
            "Z" => bookmark_toggle,
            "Q" => bookmark_next_in_file,
        }),
    );
    let mut app = AppBuilder::new()
        .with_config(config)
        .with_file(file.path(), None)
        .build()?;

    test_key_sequences(
        &mut app,
        vec![
            (Some("ZjjZkQ"), Some(&|app| assert_eq!(2, cursor_line(app)))),
            (Some("Q"), Some(&|app| assert_eq!(0, cursor_line(app)))),
        ],
        false,
    )
    .await
}

/// Maximize Editor in Split shows one window, and the second toggle brings
/// the others back.
#[tokio::test(flavor = "multi_thread")]
async fn maximize_split_and_restore() -> anyhow::Result<()> {
    let mut config = Config::default();
    config.keys.insert(
        Mode::Normal,
        keymap!({ "Normal mode"
            "Z" => toggle_maximize_split,
            "Q" => toggle_statusline,
        }),
    );
    let mut app = AppBuilder::new().with_config(config).build()?;
    let windows = |app: &Application| app.editor.tree.views().count();
    test_key_sequences(
        &mut app,
        vec![
            (Some(":vsplit<ret>:split<ret>"), Some(&|app| assert_eq!(3, windows(app)))),
            (Some("Z"), Some(&|app| assert_eq!(1, windows(app)))),
            (Some("Z"), Some(&|app| assert_eq!(3, windows(app)))),
            (Some("Q"), Some(&|app| assert!(!app.editor.config().render_statusline))),
        ],
        false,
    )
    .await
}

/// Move Down and Scroll moves the cursor and the top of the view by the same
/// line, so the cursor keeps its screen row.
#[tokio::test(flavor = "multi_thread")]
async fn move_down_and_scroll_keeps_the_screen_row() -> anyhow::Result<()> {
    let lines: String = (0..200).map(|i| format!("line {i}\n")).collect();
    let mut app = app_with(&format!("#[l|]#{}", &lines[1..]), "move_down_and_scroll")?;
    test_key_sequences(
        &mut app,
        vec![(
            Some("jjjjZ"),
            Some(&|app| {
                assert_eq!(5, cursor_line(app));
                let (view, doc) = zmax_view::current_ref!(app.editor);
                let top = doc.text().char_to_line(doc.view_offset(view.id).anchor);
                assert_eq!(1, top, "the view scrolled one line with the cursor");
            }),
        )],
        false,
    )
    .await
}

/// Escape leaves one cursor with no selection.
#[tokio::test(flavor = "multi_thread")]
async fn escape_keeps_one_collapsed_cursor() -> anyhow::Result<()> {
    let mut app = app_with("#[ab|]# #(cd|)#\n", "editor_escape")?;
    test_key_sequences(
        &mut app,
        vec![(
            Some("Z"),
            Some(&|app| {
                let (view, doc) = zmax_view::current_ref!(app.editor);
                let selection = doc.selection(view.id);
                assert_eq!(1, selection.len());
                assert!(selection.primary().len() <= 1, "collapsed");
            }),
        )],
        false,
    )
    .await
}
