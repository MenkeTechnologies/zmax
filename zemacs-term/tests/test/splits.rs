use super::*;

use zemacs_stdx::path;
use zemacs_term::application::Application;

#[tokio::test(flavor = "multi_thread")]
async fn test_split_write_quit_all() -> anyhow::Result<()> {
    let mut file1 = tempfile::NamedTempFile::new()?;
    let mut file2 = tempfile::NamedTempFile::new()?;
    let mut file3 = tempfile::NamedTempFile::new()?;

    let mut app = helpers::AppBuilder::new()
        .with_file(file1.path(), None)
        .build()?;

    test_key_sequences(
        &mut app,
        vec![
            (
                Some(&format!(
                    "ihello1<esc>:sp<ret>:o {}<ret>ihello2<esc>:sp<ret>:o {}<ret>ihello3<esc>",
                    file2.path().to_string_lossy(),
                    file3.path().to_string_lossy()
                )),
                Some(&|app| {
                    let docs: Vec<_> = app.editor.documents().collect();
                    assert_eq!(3, docs.len());

                    let doc1 = docs
                        .iter()
                        .find(|doc| doc.path().unwrap() == &path::normalize(file1.path()))
                        .unwrap();

                    assert_eq!("hello1", doc1.text().to_string());

                    let doc2 = docs
                        .iter()
                        .find(|doc| doc.path().unwrap() == &path::normalize(file2.path()))
                        .unwrap();

                    assert_eq!("hello2", doc2.text().to_string());

                    let doc3 = docs
                        .iter()
                        .find(|doc| doc.path().unwrap() == &path::normalize(file3.path()))
                        .unwrap();

                    assert_eq!("hello3", doc3.text().to_string());

                    helpers::assert_status_not_error(&app.editor);
                    assert_eq!(3, app.editor.tree.views().count());
                }),
            ),
            (
                Some(":wqa<ret>"),
                Some(&|app| {
                    helpers::assert_status_not_error(&app.editor);
                    assert_eq!(0, app.editor.tree.views().count());
                }),
            ),
        ],
        true,
    )
    .await?;

    helpers::assert_file_has_content(&mut file1, &LineFeedHandling::Native.apply("hello1"))?;
    helpers::assert_file_has_content(&mut file2, &LineFeedHandling::Native.apply("hello2"))?;
    helpers::assert_file_has_content(&mut file3, &LineFeedHandling::Native.apply("hello3"))?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_split_write_quit_same_file() -> anyhow::Result<()> {
    let mut file = tempfile::NamedTempFile::new()?;
    let mut app = helpers::AppBuilder::new()
        .with_file(file.path(), None)
        .build()?;

    test_key_sequences(
        &mut app,
        vec![
            (
                Some("O<esc>ihello<esc>:sp<ret>ogoodbye<esc>"),
                Some(&|app| {
                    assert_eq!(2, app.editor.tree.views().count());
                    helpers::assert_status_not_error(&app.editor);

                    let mut docs: Vec<_> = app.editor.documents().collect();
                    assert_eq!(1, docs.len());

                    let doc = docs.pop().unwrap();

                    assert_eq!(
                        LineFeedHandling::Native.apply("hello\ngoodbye"),
                        doc.text().to_string()
                    );

                    assert!(doc.is_modified());
                }),
            ),
            (
                Some(":wq<ret>"),
                Some(&|app| {
                    helpers::assert_status_not_error(&app.editor);
                    assert_eq!(1, app.editor.tree.views().count());

                    let mut docs: Vec<_> = app.editor.documents().collect();
                    assert_eq!(1, docs.len());

                    let doc = docs.pop().unwrap();

                    assert_eq!(
                        LineFeedHandling::Native.apply("hello\ngoodbye"),
                        doc.text().to_string()
                    );

                    assert!(!doc.is_modified());
                }),
            ),
        ],
        false,
    )
    .await?;

    helpers::assert_file_has_content(&mut file, &LineFeedHandling::Native.apply("hello\ngoodbye"))?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_changes_in_splits_apply_to_all_views() -> anyhow::Result<()> {
    // upstream regression test for split-view change propagation.
    // Transactions must be applied to any view that has the changed document open.
    // This sequence would panic since the jumplist entry would be modified in one
    // window but not the other. Attempting to update the changelist in the other
    // window would cause a panic since it would point outside of the document.

    // The key sequence here:
    // * <C-w>v       Create a vertical split of the current buffer.
    //                Both views look at the same doc.
    // * [<space>     Add a line ending to the beginning of the document.
    //                The cursor is now at line 2 in window 2.
    // * <C-s>        Save that selection to the jumplist in window 2.
    // * <C-w>w       Switch to window 1.
    // * kd           Delete line 1 in window 1.
    // * <C-w>q       Close window 1, focusing window 2.
    // * d            Delete line 1 in window 2.
    //
    // This panicked in the past because the jumplist entry on line 2 of window 2
    // was not updated and after the `kd` step, pointed outside of the document.
    test((
        "#[|]#",
        "<C-w>v[<space><C-s><C-w>wkd<C-w>qd",
        "#[|]#",
        LineFeedHandling::AsIs,
    ))
    .await?;

    // Transactions are applied to the views for windows lazily when they are focused.
    // This case panics if the transactions and inversions are not applied in the
    // correct order as we switch between windows.
    test((
        "#[|]#",
        "[<space>[<space>[<space><C-w>vuuu<C-w>wUUU<C-w>quuu",
        "#[|]#",
        LineFeedHandling::AsIs,
    ))
    .await?;

    // upstream regression test for undo history across splits.
    // This sequence undoes part of the history and then adds new changes, creating a
    // new branch in the history tree. `View::sync_changes` applies transactions down
    // and up to the lowest common ancestor in the path between old and new revision
    // numbers. If we apply these up/down transactions in the wrong order, this case
    // panics.
    // The key sequence:
    // * 3[<space>    Create three empty lines so we are at the end of the document.
    // * <C-w>v<C-s>  Create a split and save that point at the end of the document
    //                in the jumplist.
    // * <C-w>w       Switch back to the first window.
    // * uu           Undo twice (not three times which would bring us back to the
    //                root of the tree).
    // * 3[<space>    Create three empty lines. Now the end of the document is past
    //                where it was on step 1.
    // * <C-w>q       Close window 1, focusing window 2 and causing a sync. This step
    //                panics if we don't apply in the right order.
    // * %d           Clean up the buffer.
    test((
        "#[|]#",
        "3[<space><C-w>v<C-s><C-w>wuu3[<space><C-w>q%d",
        "#[|]#",
        LineFeedHandling::AsIs,
    ))
    .await?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_changes_in_splits_jumplist_sync() -> anyhow::Result<()> {
    // upstream regression test for split jumplist sync.
    // When jumping backwards (<C-o>) switches between two documents, we need to
    // ensure that the current view has been synced with all changes to the
    // document that occurred since the last time the view focused this document.
    // If the view isn't synced then this case panics since we try to form a
    // selection on "test" (which was deleted in the other view).
    test((
        "#[test|]#",
        "<C-w>sgf<C-w>wd<C-w>w<C-o><C-w>qd",
        "#[|]#",
        LineFeedHandling::AsIs,
    ))
    .await?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_reload_all_with_split_jumplist() -> anyhow::Result<()> {
    // upstream reproduction for reload-all with a split jumplist.
    //
    // The key sequence:
    // * <C-w>s   Horizontal split: two views on the same document.
    // * ]<space> Add an empty line below, growing the document.
    // * %        Select the whole document.
    // * 2G       Go to line 2. `goto_line` calls `push_jump`, recording a jump
    //            whose selection is valid at the *current* (grown) revision.
    // * ms/      Surround-add `/`, growing the document again.
    // * :rla     reload-all: re-reads the file from disk (shrinking the buffer
    //            back to its original contents) but only syncs the first view of
    //            each document, leaving the other split's `doc_revisions` stale.
    // * %J       Select-all and join, forcing a sync of the stale view.
    //
    // On the unfixed code the jumplist entry recorded by `2G` is left ahead of
    // the stale view's `doc_revisions`; once that view is synced, the entry is
    // mapped through a changeset whose pre-image predates it and
    // `ChangeSet::update_positions` panics.
    use std::io::Write;
    let mut file = tempfile::NamedTempFile::new()?;
    // `:reload-all` re-reads from disk, so the file must have on-disk contents
    // for the reload to shrink the (grown) buffer back down.
    file.write_all(b"line1\nline2\nline3\n")?;
    file.flush()?;

    let mut app = helpers::AppBuilder::new()
        .with_file(file.path(), None)
        .build()?;

    test_key_sequence(
        &mut app,
        // The trailing `<C-w>q` closes the split so a single window remains for
        // the harness's automatic `:q!` teardown. It also exercises the sync
        // that runs when a window is closed.
        Some("<C-w>s]<space>%2Gms/:rla<ret>%J<C-w>q"),
        Some(&|app| {
            helpers::assert_status_not_error(&app.editor);
        }),
        false,
    )
    .await?;

    Ok(())
}

/// vim `:sbfirst` / `:sblast` / `:sbnext` — split the window and land the new
/// split on the first / last / next buffer while the original window keeps its
/// buffer. Exercises the split-buffer navigation family end-to-end.
#[tokio::test(flavor = "multi_thread")]
async fn test_split_buffer_navigation() -> anyhow::Result<()> {
    let file1 = tempfile::NamedTempFile::new()?;
    let file2 = tempfile::NamedTempFile::new()?;
    let file3 = tempfile::NamedTempFile::new()?;

    // Start on file1, then open file2 and file3 into the SAME window (no split),
    // leaving three buffers in creation order [file1, file2, file3] and the
    // single window showing file3 (the last one edited).
    let mut app = helpers::AppBuilder::new()
        .with_file(file1.path(), None)
        .build()?;

    let p1 = path::normalize(file1.path());
    let p3 = path::normalize(file3.path());

    let doc_id = |app: &Application, want: &std::path::PathBuf| {
        app.editor
            .documents()
            .find(|d| d.path().is_some_and(|p| p == want))
            .map(|d| d.id())
            .expect("buffer for path should exist")
    };
    let focused = |app: &Application| app.editor.tree.get(app.editor.tree.focus).doc;

    test_key_sequences(
        &mut app,
        vec![
            (
                Some(&format!(
                    ":o {}<ret>:o {}<ret>",
                    file2.path().to_string_lossy(),
                    file3.path().to_string_lossy()
                )),
                Some(&|app| {
                    helpers::assert_status_not_error(&app.editor);
                    assert_eq!(3, app.editor.documents().count(), "three buffers");
                    assert_eq!(1, app.editor.tree.views().count(), "still one window");
                    assert_eq!(doc_id(app, &p3), focused(app), "window shows file3");
                }),
            ),
            (
                // Split + go to the first buffer: a second window opens on file1
                // while the original window still shows file3.
                Some(":sbfirst<ret>"),
                Some(&|app| {
                    helpers::assert_status_not_error(&app.editor);
                    assert_eq!(2, app.editor.tree.views().count(), "sbfirst opens a split");
                    assert_eq!(doc_id(app, &p1), focused(app), "new split shows file1");
                }),
            ),
            (
                // Split + go to the last buffer: file3 in the new (focused) split.
                Some(":sblast<ret>"),
                Some(&|app| {
                    helpers::assert_status_not_error(&app.editor);
                    assert_eq!(3, app.editor.tree.views().count(), "sblast opens a split");
                    assert_eq!(doc_id(app, &p3), focused(app), "new split shows file3");
                }),
            ),
            (
                // Split + next buffer wraps from the last (file3) back to file1,
                // matching :bnext wrap semantics.
                Some(":sbnext<ret>"),
                Some(&|app| {
                    helpers::assert_status_not_error(&app.editor);
                    assert_eq!(4, app.editor.tree.views().count(), "sbnext opens a split");
                    assert_eq!(doc_id(app, &p1), focused(app), "sbnext wraps to file1");
                }),
            ),
            (
                // Split + previous buffer from file1 wraps to file3.
                Some(":sbprevious<ret>"),
                Some(&|app| {
                    helpers::assert_status_not_error(&app.editor);
                    assert_eq!(
                        5,
                        app.editor.tree.views().count(),
                        "sbprevious opens a split"
                    );
                    assert_eq!(doc_id(app, &p3), focused(app), "sbprevious wraps to file3");
                }),
            ),
            // Close every window at once so the app exits cleanly (the harness's
            // single `:q!` teardown would only close one of the five splits).
            (Some(":qa!<ret>"), None),
        ],
        true,
    )
    .await?;

    Ok(())
}
