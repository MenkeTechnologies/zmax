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
        .env("GIT_CONFIG_COUNT", "3")
        .env("GIT_CONFIG_KEY_0", "commit.gpgsign")
        .env("GIT_CONFIG_VALUE_0", "false")
        .env("GIT_CONFIG_KEY_1", "init.defaultBranch")
        .env("GIT_CONFIG_VALUE_1", "main")
        // `submodule add` from a local path is refused by default since the
        // CVE-2022-39253 fix; the submodule test below needs it.
        .env("GIT_CONFIG_KEY_2", "protocol.file.allow")
        .env("GIT_CONFIG_VALUE_2", "always")
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

/// The same chain, in the layout that actually broke: the editor is launched at
/// a **superproject** root, the buffer belongs to one of its **submodules**, and
/// the commit is made inside that submodule.
///
/// A submodule keeps no `.git` directory of its own — its refs live in
/// `<super>/.git/modules/<path>/refs/heads/<branch>`. The watcher discovers the
/// superproject's `.git` (that is the repo of the directory it was pointed at),
/// so the ref write arrives as `modules/<path>/refs/heads/<branch>` relative to
/// it. Matching branch tips only at the *start* of that relative path classified
/// the write as irrelevant, no refresh was dispatched, and the gutter kept
/// diffing against the submodule's pre-commit HEAD indefinitely.
#[tokio::test(flavor = "multi_thread")]
async fn external_commit_in_a_submodule_clears_the_gutter() -> anyhow::Result<()> {
    let tmp = tempfile::tempdir()?;
    let tmp = tmp.path().canonicalize()?;

    // The repository the submodule is cloned from.
    let origin = tmp.join("sub_origin");
    std::fs::create_dir(&origin)?;
    git(&["init"], &origin);
    std::fs::write(origin.join("file.txt"), "one\ntwo\n")?;
    git(&["add", "-A"], &origin);
    git(&["commit", "-m", "first"], &origin);

    // The superproject, with that repository as a submodule at `sub/`.
    let root = tmp.join("super");
    std::fs::create_dir(&root)?;
    git(&["init"], &root);
    std::fs::write(root.join("top.txt"), "top\n")?;
    git(&["add", "-A"], &root);
    git(&["commit", "-m", "super first"], &root);
    git(&["submodule", "add", "../sub_origin", "sub"], &root);
    git(&["commit", "-m", "add submodule"], &root);

    // The submodule's git dir is not `sub/.git` — that is a file pointing here.
    let sub = root.join("sub");
    assert!(
        root.join(".git/modules/sub").is_dir(),
        "the submodule's refs must live under the superproject's .git for this test to mean anything"
    );

    // An uncommitted change inside the submodule: one hunk.
    let path = sub.join("file.txt");
    std::fs::write(&path, "one\nTWO\n")?;

    // Auto-reload off, so this test can only pass through the path it is about:
    // a classified HEAD move re-fetching the diff base. Left on, a buffer reload
    // would refresh the base as a side effect and the test would pass even with
    // the submodule's ref writes misclassified — which is exactly what it did
    // before this line existed.
    let mut config = Config::default();
    config.editor.auto_reload = false;
    let mut app = AppBuilder::new()
        .with_config(config)
        .with_file(path.clone(), None)
        .build()?;
    helpers::run_event_loop_until_idle(&mut app).await;
    assert_eq!(hunks_settling_at(&app, &path, 1).await, 1);

    // The watcher is pointed at the *superproject* — the launch directory of an
    // editor opened at the top of a repo-of-submodules.
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let watch_root = root.clone();
    let handle = tokio::runtime::Handle::current();
    std::thread::spawn(move || {
        let _runtime = handle.enter();
        zmax_term::file_watcher::run_blocking(watch_root, ready_tx);
    });
    ready_rx
        .recv_timeout(Duration::from_secs(30))
        .expect("watcher established its watches");

    // Another terminal commits inside the submodule. Nothing in either working
    // tree changes; the only write is to `<super>/.git/modules/sub/`.
    git(&["commit", "-am", "external"], &sub);

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
        "a commit inside a submodule left the gutter showing stale hunks"
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
