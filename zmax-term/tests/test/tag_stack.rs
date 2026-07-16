use super::*;

use zmax_term::config::Config;

fn vim() -> AppBuilder {
    AppBuilder::new().with_config(Config {
        keys: zmax_term::keymap::vim::default(),
        ..Default::default()
    })
}

/// The buffer line the primary cursor is on.
fn cursor_line(app: &zmax_term::application::Application) -> usize {
    let (view, doc) = zmax_view::current_ref!(app.editor);
    let text = doc.text().slice(..);
    let cursor = doc.selection(view.id).primary().cursor(text);
    text.char_to_line(cursor)
}

/// The file name of the focused buffer.
fn cursor_file(app: &zmax_term::application::Application) -> String {
    let (_, doc) = zmax_view::current_ref!(app.editor);
    doc.path()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// A workspace holding a `target.rs` to jump into and a `tags` file naming a
/// numeric address inside it. Returns (dir, caller path).
fn tags_workspace() -> anyhow::Result<(tempfile::TempDir, std::path::PathBuf)> {
    let dir = tempfile::tempdir()?;
    let root = dir.path();

    std::fs::write(
        root.join("target.rs"),
        "// 1\n// 2\nfn target_fn() {}\n// 4\n",
    )?;
    std::fs::write(root.join("caller.rs"), "// a\n// b\n// c\ncall_site\n")?;
    // ctags format: name<TAB>file<TAB>address. A numeric address is line 3.
    std::fs::write(root.join("tags"), "target_fn\ttarget.rs\t3\n")?;

    let caller = root.join("caller.rs");
    Ok((dir, caller))
}

/// vim `CTRL-T` pops the tag stack that `:tag` pushed, landing back on the exact
/// line the jump started from.
///
/// This is the round trip that regressed if `CTRL-T` were left on the jumplist:
/// the jumplist and the tag stack are separate, and `:pop`/`CTRL-T` must walk the
/// tag stack.
#[tokio::test(flavor = "multi_thread")]
async fn ctrl_t_pops_the_tag_stack_that_tag_pushed() -> anyhow::Result<()> {
    let (dir, caller) = tags_workspace()?;
    let tags = dir.path().join("tags");

    let mut app = vim().with_file(caller, None).build()?;

    test_key_sequences(
        &mut app,
        vec![
            // Point `tags` at the fixture, then park the cursor on line 3 so the
            // "from" that CTRL-T must restore is unambiguous.
            (
                Some(&format!(":set tags={}<ret>jjj", tags.display())),
                Some(&|app| {
                    assert_eq!(cursor_line(app), 3, "parked on the caller's line 3");
                    assert_eq!(cursor_file(app), "caller.rs");
                }),
            ),
            // :tag jumps into target.rs at the tags file's numeric address (line 3
            // 1-based == line index 2) and records the caller position.
            (
                Some(":tag target_fn<ret>"),
                Some(&|app| {
                    assert_eq!(cursor_file(app), "target.rs", ":tag opened the target");
                    assert_eq!(cursor_line(app), 2, "numeric tag address is 1-based");
                }),
            ),
            // CTRL-T pops that frame: back to the caller, on the line we left.
            (
                Some("<C-t>"),
                Some(&|app| {
                    assert_eq!(cursor_file(app), "caller.rs", "CTRL-T returned to caller");
                    assert_eq!(cursor_line(app), 3, "CTRL-T restored the exact from-line");
                }),
            ),
            // ...and it consumed the frame rather than walking the jumplist: the
            // stack is empty again, so a second CTRL-T hits the bottom. Landing on
            // the right line above is not enough to prove the tag stack was used —
            // the jumplist would return to the same place — so this is the
            // assertion that actually tells the two apart.
            (
                Some("<C-t>"),
                Some(&|app| {
                    let (msg, _) = app
                        .editor
                        .get_status()
                        .expect("a second CTRL-T must report the empty stack");
                    assert_eq!(msg, "at bottom of tag stack");
                }),
            ),
        ],
        false,
    )
    .await?;

    Ok(())
}

/// vim reports E555 at the bottom of the tag stack. CTRL-T must surface that
/// rather than silently walking somewhere else — the behaviour that tells the tag
/// stack apart from the jumplist, which would just keep jumping back.
#[tokio::test(flavor = "multi_thread")]
async fn ctrl_t_on_an_empty_tag_stack_reports_the_bottom() -> anyhow::Result<()> {
    let (_dir, caller) = tags_workspace()?;
    let mut app = vim().with_file(caller, None).build()?;

    test_key_sequence(
        &mut app,
        Some("<C-t>"),
        Some(&|app| {
            let (msg, _) = app
                .editor
                .get_status()
                .expect("CTRL-T on an empty tag stack must report, not stay silent");
            assert_eq!(msg, "at bottom of tag stack");
        }),
        false,
    )
    .await?;

    Ok(())
}
