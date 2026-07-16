use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use zmax_term::application::Application;

use super::*;

fn git(args: &[&str], cwd: &Path) {
    let out = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .env("GIT_CONFIG_COUNT", "2")
        .env("GIT_CONFIG_KEY_0", "commit.gpgsign")
        .env("GIT_CONFIG_VALUE_0", "false")
        .env("GIT_CONFIG_KEY_1", "init.defaultBranch")
        .env("GIT_CONFIG_VALUE_1", "main")
        .output()
        .expect("run git");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// The gutter's current hunk count for `path`.
fn current_hunks(app: &Application, path: &Path) -> u32 {
    app.editor
        .document_by_path(path)
        .expect("document open")
        .diff_handle()
        .expect("file is tracked, so it has a diff base")
        .load()
        .len()
}

/// The differ re-diffs on a background task, so poll until the gutter settles on
/// `want` (or give up and return what it last had, letting the caller assert with
/// a useful message).
async fn hunks_settling_at(app: &Application, path: &Path, want: u32) -> u32 {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let hunks = current_hunks(app, path);
        if hunks == want || Instant::now() >= deadline {
            return hunks;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// A commit made *outside* the editor must clear the git gutter.
///
/// The gutter diffs the buffer against HEAD's blob. Once another terminal commits
/// the very bytes the buffer holds, the buffer *is* HEAD and the hunks must go —
/// but nothing in the working tree changed, so only a git-dir watch can notice.
/// This drives the editor half of that path (`refresh_all_diff_bases`, what the
/// watcher dispatches on a HEAD move); `file_watcher`'s own tests cover the half
/// that turns a `.git` write into that call.
///
/// Buffer text must survive untouched: the base is re-read, the document is not.
#[tokio::test(flavor = "multi_thread")]
async fn external_commit_clears_the_gutter() -> anyhow::Result<()> {
    let tmp = tempfile::tempdir()?;
    let root = tmp.path().canonicalize()?;
    let path = root.join("file.txt");

    git(&["init"], &root);
    std::fs::write(&path, "one\ntwo\n")?;
    git(&["add", "-A"], &root);
    git(&["commit", "-m", "first"], &root);

    // The buffer holds an edit that is saved to disk but not committed: one hunk.
    std::fs::write(&path, "one\nTWO\n")?;
    let mut app = AppBuilder::new().with_file(path.clone(), None).build()?;
    helpers::run_event_loop_until_idle(&mut app).await;
    assert_eq!(
        hunks_settling_at(&app, &path, 1).await,
        1,
        "an uncommitted change to a tracked file is one hunk"
    );

    // Another terminal commits it. The working tree does not change at all.
    git(&["commit", "-am", "external"], &root);
    zmax_term::commands::refresh_all_diff_bases(&mut app.editor);

    assert_eq!(
        hunks_settling_at(&app, &path, 0).await,
        0,
        "the committed buffer now matches HEAD — the gutter must be empty"
    );
    assert_eq!(
        app.editor
            .document_by_path(&path)
            .unwrap()
            .text()
            .to_string(),
        "one\nTWO\n",
        "refreshing the diff base must not touch the buffer text"
    );
    Ok(())
}

/// The whole chain, with nothing stubbed: a real watcher thread on a real repo,
/// a commit run as a real `git` subprocess, and the editor's own event loop
/// pumping the job the watcher dispatches. The gutter must clear on its own —
/// no keypress, no `:reload`, no in-editor git command.
///
/// This is the bug: before the watcher classified `.git` ref writes, every event
/// from an external commit was filtered out as VCS noise, no job was ever
/// dispatched, and the buffer kept diffing against the *old* HEAD forever.
#[tokio::test(flavor = "multi_thread")]
async fn external_commit_clears_the_gutter_through_the_watcher() -> anyhow::Result<()> {
    let tmp = tempfile::tempdir()?;
    let root = tmp.path().canonicalize()?;
    let path = root.join("file.txt");

    git(&["init"], &root);
    std::fs::write(&path, "one\ntwo\n")?;
    git(&["add", "-A"], &root);
    git(&["commit", "-m", "first"], &root);
    std::fs::write(&path, "one\nTWO\n")?;

    let mut app = AppBuilder::new().with_file(path.clone(), None).build()?;
    helpers::run_event_loop_until_idle(&mut app).await;
    assert_eq!(hunks_settling_at(&app, &path, 1).await, 1);

    // The watcher loop the editor really runs, on this repo — on its own OS
    // thread, exactly like `spawn` does, but with the test runtime's context
    // entered on it. Both halves of that matter:
    //
    // * Its own thread, not `spawn_blocking`: the loop never returns, and parking
    //   it on a runtime thread starves the scheduler that has to drive the editor.
    // * The runtime context, unlike production: under this harness the job queue
    //   is `runtime_local!`, so a thread with no current runtime is handed its own
    //   instance and its callbacks never reach this app. Production has a single
    //   process-wide queue, so the bare thread `spawn` uses is right there.
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let watch_root = root.clone();
    let handle = tokio::runtime::Handle::current();
    std::thread::spawn(move || {
        let _runtime = handle.enter();
        zmax_term::file_watcher::run_blocking(watch_root, ready_tx);
    });
    // The OS reports nothing that happened before the watches existed, so commit
    // too early and the event is never generated: wait for the signal, never a
    // guessed sleep.
    ready_rx
        .recv_timeout(Duration::from_secs(30))
        .expect("watcher established its watches");

    git(&["commit", "-am", "external"], &root);

    // Pump the editor loop the way the running editor does, until the watcher's
    // dispatched refresh lands and the differ has re-diffed against the new HEAD.
    let pumped = tokio::time::timeout(Duration::from_secs(60), async {
        loop {
            helpers::run_event_loop_until_idle(&mut app).await;
            if current_hunks(&app, &path) == 0 {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await;
    let hunks = current_hunks(&app, &path);
    assert!(
        pumped.is_ok() || hunks == 0,
        "timed out waiting for the watcher's refresh"
    );

    assert_eq!(
        hunks, 0,
        "a commit made outside the editor left the gutter showing stale hunks"
    );
    assert_eq!(
        app.editor
            .document_by_path(&path)
            .unwrap()
            .text()
            .to_string(),
        "one\nTWO\n",
        "the watcher must not have touched the buffer text"
    );
    Ok(())
}

/// Staging is not committing: `git add` moves the index, and the gutter diffs
/// against HEAD, so a staged-but-uncommitted change must still show its hunk.
/// This is why the watcher ignores `.git/index` — firing there would be harmless
/// but pointless, while treating the index as the base would silently erase the
/// hunks of everything staged.
#[tokio::test(flavor = "multi_thread")]
async fn staging_alone_keeps_the_gutter() -> anyhow::Result<()> {
    let tmp = tempfile::tempdir()?;
    let root = tmp.path().canonicalize()?;
    let path = root.join("file.txt");

    git(&["init"], &root);
    std::fs::write(&path, "one\ntwo\n")?;
    git(&["add", "-A"], &root);
    git(&["commit", "-m", "first"], &root);

    std::fs::write(&path, "one\nTWO\n")?;
    let mut app = AppBuilder::new().with_file(path.clone(), None).build()?;
    helpers::run_event_loop_until_idle(&mut app).await;
    assert_eq!(hunks_settling_at(&app, &path, 1).await, 1);

    git(&["add", "-A"], &root);
    zmax_term::commands::refresh_all_diff_bases(&mut app.editor);

    assert_eq!(
        hunks_settling_at(&app, &path, 1).await,
        1,
        "staged but uncommitted: HEAD is unmoved, so the hunk stays"
    );
    Ok(())
}
