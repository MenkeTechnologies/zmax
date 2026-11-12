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
        "md_table_move_column_right" => {
            keymap!({ "Normal mode" "Z" => md_table_move_column_right, })
        }
        "md_table_align_right" => keymap!({ "Normal mode" "Z" => md_table_align_right, }),
        "md_toggle_bold" => keymap!({ "Normal mode" "Z" => md_toggle_bold, }),
        "md_heading_up" => keymap!({ "Normal mode" "Z" => md_heading_up, }),
        "move_down_and_scroll" => keymap!({ "Normal mode" "Z" => move_down_and_scroll, }),
        "editor_escape" => keymap!({ "Normal mode" "Z" => editor_escape, }),
        "surround_with_emmet" => keymap!({ "Normal mode" "Z" => surround_with_emmet, }),
        "md_link_to_reference" => keymap!({ "Normal mode" "Z" => md_link_to_reference, }),
        "next_occurrence" => keymap!({ "Normal mode" "Z" => next_occurrence, }),
        "fold_code_block" => keymap!({ "Normal mode" "Z" => fold_code_block, }),
        "select_line_at_caret" => keymap!({ "Normal mode" "Z" => select_line_at_caret, }),
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
            (
                Some("ggM7jjQ"),
                Some(&|app| assert_eq!(0, cursor_line(app))),
            ),
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
    let mut app = app_with(
        "| a | bb |\n|---|---|\n| #[1|]# | 2 |\n",
        "md_table_insert_row_below",
    )?;
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
    let mut app = app_with(
        "| #[a|]# | b |\n| --- | --: |\n| 1 | 2 |\n",
        "md_table_move_column_right",
    )?;
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
        vec![(
            Some("Z"),
            Some(&|app| assert_eq!("|   a |\n| --: |\n|   1 |\n", text(app))),
        )],
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
        vec![(
            Some("Z"),
            Some(&|app| assert_eq!("say **hello**\n", text(app))),
        )],
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
            (
                Some("ZjZQ"),
                Some(&|app| assert_eq!(vec![(0, false), (1, true)], breakpoints(app))),
            ),
            (
                Some("M"),
                Some(&|app| assert_eq!(Vec::<(usize, bool)>::new(), breakpoints(app))),
            ),
            (
                Some("R"),
                Some(&|app| assert_eq!(vec![(1, true)], breakpoints(app))),
            ),
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
            (
                Some(":vsplit<ret>:split<ret>"),
                Some(&|app| assert_eq!(3, windows(app))),
            ),
            (Some("Z"), Some(&|app| assert_eq!(1, windows(app)))),
            (Some("Z"), Some(&|app| assert_eq!(3, windows(app)))),
            (
                Some("Q"),
                Some(&|app| assert!(!app.editor.config().render_statusline)),
            ),
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

/// The shelf round trip in a throwaway repository: save the change to the
/// shelf, lose it from the tree, pop it back, and restore the popped entry.
#[tokio::test(flavor = "multi_thread")]
async fn shelf_save_pop_and_restore() -> anyhow::Result<()> {
    let repo = tempfile::tempdir()?;
    let git = |args: &[&str]| {
        let ok = std::process::Command::new("git")
            .arg("-C")
            .arg(repo.path())
            .args(args)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        assert!(ok, "git {args:?}");
    };
    git(&["init", "-q"]);
    git(&["config", "user.email", "user@example.com"]);
    git(&["config", "user.name", "user"]);
    let file = repo.path().join("f.txt");
    std::fs::write(&file, "one\n")?;
    git(&["add", "f.txt"]);
    git(&["commit", "-qm", "base"]);
    std::fs::write(&file, "two\n")?;

    let mut config = Config::default();
    config.keys.insert(
        Mode::Normal,
        keymap!({ "Normal mode"
            "Z" => shelf_save_keep,
            "Q" => shelf_pop,
            "M" => shelf_restore,
        }),
    );
    let mut app = AppBuilder::new()
        .with_config(config)
        .with_file(&file, None)
        .build()?;
    let shelf = repo.path().join(".git").join("zmax-shelf");
    let patches = move |dir: &std::path::Path| {
        std::fs::read_dir(dir)
            .map(|d| {
                d.flatten()
                    .filter(|e| e.path().extension().is_some_and(|x| x == "patch"))
                    .count()
            })
            .unwrap_or(0)
    };

    test_key_sequences(&mut app, vec![(Some("Z"), None)], false).await?;
    assert_eq!(1, patches(&shelf), "saved to the shelf");
    assert_eq!(
        "two\n",
        std::fs::read_to_string(&file)?,
        "the tree kept the change"
    );

    git(&["checkout", "--", "f.txt"]);
    test_key_sequences(&mut app, vec![(Some("Q<ret>"), None)], false).await?;
    assert_eq!(
        "two\n",
        std::fs::read_to_string(&file)?,
        "popped back into the tree"
    );
    assert_eq!(0, patches(&shelf), "and off the shelf");

    test_key_sequences(&mut app, vec![(Some("M<ret>"), None)], false).await?;
    assert_eq!(1, patches(&shelf), "restored from the recycled shelf");
    Ok(())
}

/// `:shelve` and `:unshelve` act on the whole repository even when the buffer
/// sits in a subdirectory. Run from there, `git checkout -- .` restored only
/// the subdirectory and `git apply` skipped the rest, both without an error.
#[tokio::test(flavor = "multi_thread")]
async fn shelve_from_a_subdirectory_covers_the_whole_tree() -> anyhow::Result<()> {
    let repo = tempfile::tempdir()?;
    let git = |args: &[&str]| {
        let ok = std::process::Command::new("git")
            .arg("-C")
            .arg(repo.path())
            .args(args)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        assert!(ok, "git {args:?}");
    };
    git(&["init", "-q"]);
    git(&["config", "user.email", "user@example.com"]);
    git(&["config", "user.name", "user"]);
    std::fs::create_dir(repo.path().join("sub"))?;
    let top = repo.path().join("top.txt");
    let nested = repo.path().join("sub").join("s.txt");
    std::fs::write(&top, "a\n")?;
    std::fs::write(&nested, "b\n")?;
    git(&["add", "."]);
    git(&["commit", "-qm", "base"]);
    std::fs::write(&top, "A\n")?;
    std::fs::write(&nested, "B\n")?;

    let mut app = AppBuilder::new().with_file(&nested, None).build()?;
    test_key_sequences(&mut app, vec![(Some(":shelve work<ret>"), None)], false).await?;
    assert_eq!(
        "a\n",
        std::fs::read_to_string(&top)?,
        "the top-level file was shelved too"
    );
    assert_eq!("b\n", std::fs::read_to_string(&nested)?);

    test_key_sequences(&mut app, vec![(Some(":unshelve work<ret>"), None)], false).await?;
    assert_eq!("A\n", std::fs::read_to_string(&top)?, "and came back");
    assert_eq!("B\n", std::fs::read_to_string(&nested)?);
    Ok(())
}

/// Committing the default changelist takes the changes no list claims and
/// leaves another list's file alone.
#[tokio::test(flavor = "multi_thread")]
async fn default_changelist_commits_the_unclaimed_changes() -> anyhow::Result<()> {
    let repo = tempfile::tempdir()?;
    let git = |args: &[&str]| -> String {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(repo.path())
            .args(args)
            .output()
            .expect("git runs");
        assert!(out.status.success(), "git {args:?}");
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    git(&["init", "-q"]);
    git(&["config", "user.email", "user@example.com"]);
    git(&["config", "user.name", "user"]);
    let (a, b) = (repo.path().join("a.txt"), repo.path().join("b.txt"));
    std::fs::write(&a, "a\n")?;
    std::fs::write(&b, "b\n")?;
    git(&["add", "."]);
    git(&["commit", "-qm", "base"]);
    std::fs::write(&a, "A\n")?;
    std::fs::write(&b, "B\n")?;

    let mut app = AppBuilder::new().with_file(&a, None).build()?;
    test_key_sequences(
        &mut app,
        vec![(
            Some(":changelist-default main<ret>:changelist-new work<ret>:changelist work<ret>:changelist-commit main fix b<ret>"),
            None,
        )],
        false,
    )
    .await?;
    assert_eq!(
        "b.txt\n",
        git(&["show", "--name-only", "--format=", "HEAD"])
    );
    assert_eq!(
        " M a.txt\n",
        git(&["status", "--porcelain"]),
        "work's file stayed uncommitted"
    );
    Ok(())
}

/// Watches are kept whether or not a debugger runs: add one, duplicate it,
/// and clearing reports both.
#[tokio::test(flavor = "multi_thread")]
async fn watches_add_duplicate_and_clear() -> anyhow::Result<()> {
    let mut config = Config::default();
    config.keys.insert(
        Mode::Normal,
        keymap!({ "Normal mode"
            "Z" => dap_add_watch,
            "Q" => dap_copy_watch,
            "M" => dap_remove_all_watches,
        }),
    );
    let mut app = AppBuilder::new().with_config(config).build()?;
    test_key_sequences(
        &mut app,
        vec![(
            Some("Zcount + 1<ret>Q<ret>M"),
            Some(&|app| {
                let (status, _) = app.editor.get_status().expect("a status");
                assert_eq!("removed 2 watch(es)", status);
            }),
        )],
        false,
    )
    .await
}

/// Clear Undo History leaves the text and takes away what undo would undo.
#[tokio::test(flavor = "multi_thread")]
async fn clear_undo_history_keeps_the_text() -> anyhow::Result<()> {
    let mut config = Config::default();
    config.keys.insert(
        Mode::Normal,
        keymap!({ "Normal mode" "Z" => clear_undo_history, }),
    );
    let mut app = AppBuilder::new().with_config(config).build()?;
    test_key_sequences(
        &mut app,
        vec![(
            Some("ihello<esc>Zu"),
            Some(&|app| assert_eq!("hello\n", text(app))),
        )],
        false,
    )
    .await
}

/// Open in Opposite Group shows the buffer in the neighbouring split too.
#[tokio::test(flavor = "multi_thread")]
async fn open_in_opposite_group_keeps_the_original() -> anyhow::Result<()> {
    let mut config = Config::default();
    config.keys.insert(
        Mode::Normal,
        keymap!({ "Normal mode" "Z" => open_in_opposite_group, }),
    );
    let mut app = AppBuilder::new().with_config(config).build()?;
    test_key_sequences(
        &mut app,
        vec![(
            Some(":vnew<ret><C-w>hZ"),
            Some(&|app| {
                let docs: Vec<_> = app.editor.tree.views().map(|(v, _)| v.doc).collect();
                assert_eq!(2, docs.len());
                assert_eq!(docs[0], docs[1], "both splits show the same buffer");
            }),
        )],
        false,
    )
    .await
}

/// HTML File writes a page beside the current file and opens it.
#[tokio::test(flavor = "multi_thread")]
async fn new_html_file_beside_the_buffer() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let file = dir.path().join("a.txt");
    std::fs::write(&file, "a\n")?;
    let mut config = Config::default();
    config.keys.insert(
        Mode::Normal,
        keymap!({ "Normal mode" "Z" => new_html_file, }),
    );
    let mut app = AppBuilder::new()
        .with_config(config)
        .with_file(&file, None)
        .build()?;
    test_key_sequences(&mut app, vec![(Some("Zpage<ret>"), None)], false).await?;
    let page = std::fs::read_to_string(dir.path().join("page.html"))?;
    assert!(
        page.starts_with("<!DOCTYPE html>") && page.contains("<title>page</title>"),
        "{page}"
    );
    Ok(())
}

/// With Scroll to Results During Typing off, typing a pattern leaves the
/// cursor where it was until Enter.
#[tokio::test(flavor = "multi_thread")]
async fn search_preview_off_waits_for_enter() -> anyhow::Result<()> {
    let mut config = Config::default();
    config.keys.insert(
        Mode::Normal,
        keymap!({ "Normal mode" "Z" => toggle_search_preview, }),
    );
    let mut app = AppBuilder::new()
        .with_config(config)
        .with_input_text("#[a|]#bc\nxyz\n")
        .build()?;
    test_key_sequences(
        &mut app,
        vec![
            (
                Some("Z/xy"),
                Some(&|app| assert_eq!(0, cursor_line(app), "no move while typing")),
            ),
            (
                Some("<ret>"),
                Some(&|app| assert_eq!(1, cursor_line(app), "moved on Enter")),
            ),
            (Some("Z"), None),
        ],
        false,
    )
    .await
}

/// Surround with Emmet puts the selection in the expansion's first slot.
#[tokio::test(flavor = "multi_thread")]
async fn surround_with_emmet_wraps_the_selection() -> anyhow::Result<()> {
    let mut app = app_with("#[hello|]#\n", "surround_with_emmet")?;
    test_key_sequences(
        &mut app,
        vec![(
            Some("Zdiv.box<ret>"),
            Some(&|app| assert_eq!("<div class=\"box\">hello</div>\n", text(app))),
        )],
        false,
    )
    .await
}

/// Convert to Reference swaps the inline link for a reference and defines it
/// at the end of the document.
#[tokio::test(flavor = "multi_thread")]
async fn md_inline_link_becomes_a_reference() -> anyhow::Result<()> {
    let mut app = app_with("see [#[D|]#ocs](https://x.io)\n", "md_link_to_reference")?;
    test_key_sequences(
        &mut app,
        vec![(
            Some("Z"),
            Some(&|app| assert_eq!("see [Docs][docs]\n\n[docs]: https://x.io\n", text(app))),
        )],
        false,
    )
    .await
}

/// A command run from the command palette gets to open its prompt. The
/// palette used to drop the layers a command queued, so the prompt never
/// appeared and the answer was typed into the buffer as keys.
#[tokio::test(flavor = "multi_thread")]
async fn palette_commands_open_their_prompts() -> anyhow::Result<()> {
    let mut config = Config::default();
    config.keys.insert(
        Mode::Normal,
        keymap!({ "Normal mode" "Z" => command_palette, }),
    );
    let mut app = AppBuilder::new().with_config(config).build()?;
    test_key_sequences(
        &mut app,
        vec![(
            Some("Zmd_insert_image<ret>a.png<ret>"),
            Some(&|app| assert_eq!("![a](a.png)\n", text(app))),
        )],
        false,
    )
    .await
}

/// With no Find in Path or Find Usages results listed, Next Occurrence steps
/// through the matches of the editor search.
#[tokio::test(flavor = "multi_thread")]
async fn next_occurrence_without_results_is_the_next_match() -> anyhow::Result<()> {
    let mut app = app_with("#[a|]# x y x\n", "next_occurrence")?;
    test_key_sequences(
        &mut app,
        vec![
            (
                Some("/x<ret>"),
                Some(&|app| assert_eq!((2, 3), primary(app))),
            ),
            (Some("Z"), Some(&|app| assert_eq!((6, 7), primary(app)))),
        ],
        false,
    )
    .await
}

/// Fold Code Block makes the fold when the buffer has none, over the block the
/// caret is in, and closes it: the body lines are hidden, the header shown.
#[tokio::test(flavor = "multi_thread")]
async fn fold_code_block_folds_the_block_around_the_caret() -> anyhow::Result<()> {
    let file = tempfile::Builder::new().suffix(".rs").tempfile()?;
    std::fs::write(
        file.path(),
        "fn a() {\n    let x = 1;\n    let y = 2;\n}\nfn b() {}\n",
    )?;
    let mut config = Config::default();
    config.keys.insert(
        Mode::Normal,
        keymap!({ "Normal mode" "Z" => fold_code_block, }),
    );
    let mut app = AppBuilder::new()
        .with_config(config)
        .with_file(file.path(), None)
        .build()?;
    test_key_sequences(
        &mut app,
        vec![(
            Some("jjZ"),
            Some(&|app| {
                let folds = zmax_view::doc!(app.editor).folds();
                assert_eq!(1, folds.len());
                assert!(!folds.is_line_hidden(0), "the header stays");
                assert!(folds.is_line_hidden(2), "the body is folded");
                assert!(!folds.is_line_hidden(4), "the next item is not");
            }),
        )],
        false,
    )
    .await
}

/// Bookmark lists: a line goes into a second list, keeps its description
/// there, and Move Up reorders it within its lists.
#[tokio::test(flavor = "multi_thread")]
async fn bookmark_lists_descriptions_and_order() -> anyhow::Result<()> {
    let file = tempfile::Builder::new().suffix(".txt").tempfile()?;
    std::fs::write(file.path(), "a\nb\nc\n")?;
    let path = file.path().to_path_buf();
    let mut config = Config::default();
    config.keys.insert(
        Mode::Normal,
        keymap!({ "Normal mode"
            "Z" => bookmark_toggle,
            "W" => bookmark_list_create,
            "E" => bookmark_add_to_list,
            "R" => bookmark_edit_description,
            "U" => bookmark_move_up,
        }),
    );
    let mut app = AppBuilder::new()
        .with_config(config)
        .with_file(file.path(), None)
        .build()?;
    let status = |app: &Application| {
        app.editor
            .get_status()
            .map(|(s, _)| s.to_string())
            .unwrap_or_default()
    };

    test_key_sequences(
        &mut app,
        vec![
            (Some("ZWreview<ret>Ereview<ret>Rfirst line<ret>"), None),
            (
                Some("jjZU"),
                Some(&|app| assert!(status(app).starts_with("Bookmark moved"), "{}", status(app))),
            ),
        ],
        false,
    )
    .await?;
    let lists = zmax_term::line_bookmarks::lists_of(&path, 0);
    assert!(lists.contains(&"review".to_owned()), "{lists:?}");
    let mark = zmax_term::line_bookmarks::get(&path, 0).expect("line 0 is bookmarked");
    assert_eq!(Some("first line".to_owned()), mark.description);
    Ok(())
}

/// The blank diff window: two empty sides, each diffed against the other as
/// it is typed into.
#[tokio::test(flavor = "multi_thread")]
async fn blank_diff_sides_diff_against_each_other() -> anyhow::Result<()> {
    let mut config = Config::default();
    config.keys.insert(
        Mode::Normal,
        keymap!({ "Normal mode" "Z" => blank_diff_window, }),
    );
    let mut app = AppBuilder::new().with_config(config).build()?;
    test_key_sequences(
        &mut app,
        vec![(
            Some("Zia<ret>c<esc><C-w>hia<ret>b<esc>"),
            Some(&|app| {
                let bases: Vec<String> = app
                    .editor
                    .documents()
                    .filter_map(|d| {
                        d.diff_handle()
                            .map(|h| (d.text().to_string(), h.load().diff_base().to_string()))
                    })
                    .map(|(text, base)| format!("{text}|{base}"))
                    .collect();
                assert!(bases.contains(&"a\nc\n|a\nb\n".to_string()), "{bases:?}");
                assert!(bases.contains(&"a\nb\n|a\nc\n".to_string()), "{bases:?}");
            }),
        )],
        false,
    )
    .await
}

/// Go to Tab N counts buffers in buffer-line order; a count past the last
/// buffer leaves the current one shown.
#[tokio::test(flavor = "multi_thread")]
async fn goto_buffer_tab_picks_the_nth_buffer() -> anyhow::Result<()> {
    let files = [
        temp_file_with_contents("one\n")?,
        temp_file_with_contents("two\n")?,
        temp_file_with_contents("three\n")?,
    ];
    let mut config = Config::default();
    config.keys.insert(
        Mode::Normal,
        keymap!({ "Normal mode" "Z" => goto_buffer_tab, }),
    );
    let mut builder = AppBuilder::new().with_config(config);
    for f in &files {
        builder = builder.with_file(f.path(), None);
    }
    let mut app = builder.build()?;
    let shown = |app: &Application| zmax_view::doc!(app.editor).text().to_string();
    test_key_sequences(
        &mut app,
        vec![
            (Some("2Z"), Some(&|app| assert_eq!(shown(app), "two\n"))),
            (Some("Z"), Some(&|app| assert_eq!(shown(app), "one\n"))),
            (Some("3Z9Z"), Some(&|app| assert_eq!(shown(app), "three\n"))),
        ],
        false,
    )
    .await
}

/// Toggle Popup Hints for Current File switches the buffer it ran in; the
/// other buffer keeps its popups.
#[tokio::test(flavor = "multi_thread")]
async fn popup_hints_toggle_stays_with_its_file() -> anyhow::Result<()> {
    let files = [
        temp_file_with_contents("one\n")?,
        temp_file_with_contents("two\n")?,
    ];
    let mut config = Config::default();
    config.keys.insert(
        Mode::Normal,
        keymap!({ "Normal mode"
            "Z" => toggle_popup_hints,
            "X" => goto_next_buffer,
        }),
    );
    let mut app = AppBuilder::new()
        .with_config(config)
        .with_file(files[0].path(), None)
        .with_file(files[1].path(), None)
        .build()?;
    let off = |app: &Application| {
        let doc = zmax_view::doc!(app.editor);
        (doc.text().to_string(), doc.popup_hints_off)
    };
    test_key_sequences(
        &mut app,
        vec![
            (
                Some("Z"),
                Some(&|app| assert_eq!(off(app), ("one\n".into(), true))),
            ),
            (
                Some("X"),
                Some(&|app| assert_eq!(off(app), ("two\n".into(), false))),
            ),
            (
                Some("X"),
                Some(&|app| assert_eq!(off(app), ("one\n".into(), true))),
            ),
            (
                Some("Z"),
                Some(&|app| assert_eq!(off(app), ("one\n".into(), false))),
            ),
        ],
        false,
    )
    .await
}

/// Show Hidden Tabs offers only the buffers the bufferline could not draw:
/// with two buffers there are none, so no picker opens and Enter does not
/// switch; with thirty the current buffer and its neighbours on the bar are
/// left out.
#[tokio::test(flavor = "multi_thread")]
async fn show_hidden_tabs_lists_the_buffers_off_the_bufferline() -> anyhow::Result<()> {
    let files = (0..30)
        .map(|i| temp_file_with_contents(format!("{i}\n")))
        .collect::<anyhow::Result<Vec<_>>>()?;
    let app_over = |count: usize| {
        let mut config = Config::default();
        config.keys.insert(
            Mode::Normal,
            keymap!({ "Normal mode" "Z" => show_hidden_tabs, }),
        );
        let mut builder = AppBuilder::new().with_config(config);
        for f in &files[..count] {
            builder = builder.with_file(f.path(), None);
        }
        builder.build()
    };
    let shown = |app: &Application| zmax_view::doc!(app.editor).text().to_string();

    let mut app = app_over(2)?;
    test_key_sequences(
        &mut app,
        vec![(
            Some("<esc>Z<ret>"),
            Some(&|app| assert_eq!(shown(app), "0\n")),
        )],
        false,
    )
    .await?;

    let mut app = app_over(30)?;
    test_key_sequences(
        &mut app,
        vec![(
            Some("<esc>Z<ret>"),
            Some(&|app| {
                let n: usize = shown(app).trim().parse().unwrap();
                assert!(n >= 3, "buffer {n} was on the bufferline");
            }),
        )],
        false,
    )
    .await
}

/// Select Single Line at Caret keeps only the line the cursor is on, even
/// when the selection spanned more.
#[tokio::test(flavor = "multi_thread")]
async fn select_line_at_caret_drops_the_other_lines() -> anyhow::Result<()> {
    let mut app = app_with("a#[b\nc|]#d\ne\n", "select_line_at_caret")?;
    test_key_sequences(
        &mut app,
        vec![(
            Some("Z"),
            Some(&|app| {
                assert_eq!(primary(app), (3, 6));
                assert_eq!(text(app), "ab\ncd\ne\n");
            }),
        )],
        false,
    )
    .await
}

/// Scroll Page Right / Left move the view by its whole width, and left stops
/// at the first column.
#[tokio::test(flavor = "multi_thread")]
async fn scroll_page_columns_move_a_full_width() -> anyhow::Result<()> {
    let mut config = Config::default();
    config.keys.insert(
        Mode::Normal,
        keymap!({ "Normal mode"
            "Z" => scroll_page_column_right,
            "Y" => scroll_page_column_left,
        }),
    );
    let mut app = AppBuilder::new()
        .with_config(config)
        .with_input_text(&format!("#[x|]#{}\n", "y".repeat(500)))
        .build()?;
    let state = |app: &Application| {
        let (view, doc) = zmax_view::current_ref!(app.editor);
        (
            doc.view_offset(view.id).horizontal_offset,
            view.inner_area(doc).width as usize,
        )
    };
    test_key_sequences(
        &mut app,
        vec![
            (
                Some("ZZ"),
                Some(&|app| {
                    let (offset, width) = state(app);
                    assert_eq!(offset, 2 * width);
                }),
            ),
            (Some("YYY"), Some(&|app| assert_eq!(state(app).0, 0))),
        ],
        false,
    )
    .await
}

/// Export to HTML writes the buffer as a page whose highlighted stretches
/// carry the theme's colours; with a selection it writes the selection only.
#[tokio::test(flavor = "multi_thread")]
async fn export_html_colours_the_buffer_or_the_selection() -> anyhow::Result<()> {
    let file = tempfile::Builder::new().suffix(".rs").tempfile()?;
    std::fs::write(file.path(), "fn main() { let s = \"<b>\"; }\n")?;
    let dir = tempfile::tempdir()?;
    let (whole, part) = (dir.path().join("whole.html"), dir.path().join("part.html"));
    // Bound to keys rather than typed at the prompt: typing a temp path
    // re-lists the temp directory for completion on every character.
    let mut config = Config::default();
    config.keys = toml::from_str(&format!(
        "[normal]\nZ = ':export-html {}'\nX = ':export-html {}'\n",
        whole.display(),
        part.display()
    ))?;
    let mut app = AppBuilder::new()
        .with_config(config)
        .with_file(file.path(), None)
        .build()?;

    test_key_sequence(&mut app, Some("Z"), None, false).await?;
    let html = std::fs::read_to_string(&whole)?;
    assert!(html.contains("<span style=\"color:#"), "{html}");
    assert!(html.contains("&quot;&lt;b&gt;&quot;"), "{html}");
    let plain: String = {
        let pre = &html[html.find("<pre>").unwrap() + 5..html.find("</pre>").unwrap()];
        let mut out = String::new();
        let mut in_tag = false;
        for c in pre.chars() {
            match c {
                '<' => in_tag = true,
                '>' if in_tag => in_tag = false,
                c if !in_tag => out.push(c),
                _ => {}
            }
        }
        out
    };
    assert_eq!(plain, "fn main() { let s = &quot;&lt;b&gt;&quot;; }\n");

    {
        let (view, doc) = zmax_view::current!(app.editor);
        doc.set_selection(view.id, zmax_core::Selection::single(0, 7));
    }
    test_key_sequence(&mut app, Some("X"), None, false).await?;
    let html = std::fs::read_to_string(&part)?;
    assert!(html.contains("main"), "{html}");
    assert!(
        !html.contains("let"),
        "only the selection is exported: {html}"
    );
    Ok(())
}

/// A code lens drawn at a line's end stays there while text is typed before
/// it, and the edit marks the buffer's lenses for the next fetch.
#[tokio::test(flavor = "multi_thread")]
async fn code_lens_annotations_follow_edits() -> anyhow::Result<()> {
    let mut app = AppBuilder::new().with_input_text("#[a|]#b\ncd\n").build()?;
    {
        let doc = zmax_view::doc_mut!(app.editor);
        doc.code_lens_outdated = false;
        doc.code_lens_annotations = vec![zmax_core::text_annotations::InlineAnnotation::new(
            2, "  Run",
        )];
    }
    test_key_sequence(
        &mut app,
        Some("iXY<esc>"),
        Some(&|app| {
            let doc = zmax_view::doc!(app.editor);
            assert_eq!(doc.text().to_string(), "XYab\ncd\n");
            assert_eq!(
                doc.code_lens_annotations[0].char_idx, 4,
                "still the end of line 1"
            );
            assert!(doc.code_lens_outdated);
        }),
        false,
    )
    .await
}

/// Renaming a project node moves the file and the buffer showing it; renaming
/// a directory carries the buffers of the files inside along; a name that is
/// taken, or that is a path, is refused and nothing moves.
#[tokio::test(flavor = "multi_thread")]
async fn renaming_a_tree_node_takes_its_open_buffers_along() -> anyhow::Result<()> {
    use zmax_term::commands::rename_path;
    let dir = tempfile::tempdir()?;
    let root = dir.path().canonicalize()?;
    std::fs::create_dir(root.join("d"))?;
    std::fs::write(root.join("a.txt"), "a\n")?;
    std::fs::write(root.join("taken.txt"), "t\n")?;
    std::fs::write(root.join("d").join("x.txt"), "x\n")?;
    let mut app = AppBuilder::new()
        .with_file(root.join("a.txt"), None)
        .with_file(root.join("d").join("x.txt"), None)
        .build()?;
    let open_paths = |app: &Application| {
        let mut paths: Vec<_> = app
            .editor
            .documents()
            .filter_map(|doc| doc.path().map(std::path::Path::to_path_buf))
            .collect();
        paths.sort();
        paths
    };

    let new = rename_path(&mut app.editor, &root.join("a.txt"), "b.txt").unwrap();
    assert_eq!(new, root.join("b.txt"));
    assert!(!root.join("a.txt").exists() && root.join("b.txt").exists());

    let new = rename_path(&mut app.editor, &root.join("d"), "e").unwrap();
    assert_eq!(new, root.join("e"));
    assert_eq!(
        open_paths(&app),
        [root.join("b.txt"), root.join("e").join("x.txt")]
    );

    let taken = rename_path(&mut app.editor, &root.join("b.txt"), "taken.txt");
    assert!(taken.unwrap_err().contains("already exists"));
    assert!(rename_path(&mut app.editor, &root.join("b.txt"), "e/b.txt").is_err());
    assert_eq!(std::fs::read_to_string(root.join("b.txt"))?, "a\n");
    assert_eq!(std::fs::read_to_string(root.join("taken.txt"))?, "t\n");
    Ok(())
}
