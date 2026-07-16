//! Filesystem watcher that live-refreshes the IDE file tree when files change
//! on disk outside the editor (create/delete/rename/modify), and the git gutters
//! when HEAD moves outside the editor.
//!
//! A dedicated OS thread owns the `notify` watcher and a receive loop. On a
//! relevant event it coalesces a short burst, then hops onto the main thread via
//! [`job::dispatch_blocking`] to rebuild the tree; the event loop renders right
//! after each dispatched callback, so the change shows up immediately.
//!
//! Two disjoint classes of event are handled, because a commit made in another
//! terminal writes *only* inside the git directory — the working tree is left
//! byte-for-byte identical, so no ordinary file event ever fires for it:
//!
//! * **worktree paths** — rebuild the file tree, auto-reload the buffers whose
//!   file changed.
//! * **git ref paths** ([`is_head_move`]) — HEAD moved, so every open buffer's
//!   diff base (HEAD's blob) is stale; re-fetch it via
//!   [`commands::refresh_all_diff_bases`](crate::commands::refresh_all_diff_bases).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use notify::{RecursiveMode, Watcher};

use crate::job;
use crate::ui::EditorView;

/// Ensures we only ever spawn a single watcher for the process.
static SPAWNED: AtomicBool = AtomicBool::new(false);

/// Directories whose churn should never trigger a tree refresh (build output,
/// VCS internals, dependency caches) — they're noisy and usually hidden anyway.
fn is_ignored(path: &Path) -> bool {
    path.components().any(|component| {
        matches!(
            component.as_os_str().to_str(),
            Some(
                ".git"
                    | "target"
                    | "node_modules"
                    | ".cache"
                    | "dist"
                    | "build"
                    | ".direnv"
                    | ".venv"
            )
        )
    })
}

/// Start watching `root` recursively. Idempotent: only the first call spawns a
/// watcher; later calls (e.g. reopening the IDE) are no-ops.
pub fn spawn(root: PathBuf) {
    if SPAWNED.swap(true, Ordering::SeqCst) {
        return;
    }

    // Startup must not block on registering the watches (a recursive add over a
    // large tree is not instant), so nothing here waits for readiness.
    let (ready, _) = mpsc::channel();
    std::thread::Builder::new()
        .name("file-tree-watcher".into())
        .spawn(move || run(root, ready))
        .ok();
}

/// The watcher loop, for a test to run on a thread of its choosing. Blocks
/// forever; `ready` receives `()` once the watches are established.
///
/// Waiting for that signal is not optional: registering the watches takes long
/// enough (hundreds of milliseconds) that a change made before they exist is
/// never reported at all — the OS only streams events from the moment the watch
/// is live. A test that sleeps a guessed interval instead silently tests nothing.
///
/// A test must run this on its own thread — it never returns, so a `spawn_blocking`
/// task would sit on a runtime thread and starve the scheduler driving the editor
/// — with the runtime context entered on that thread (`Handle::enter`). The latter
/// is a harness quirk: with the `integration` feature `job`'s queue is
/// `runtime_local!`, so a thread with no current runtime is handed a *separate*
/// instance and its callbacks never reach the editor. Production has one
/// process-wide queue, which is why [`spawn`]'s bare thread is right there.
#[doc(hidden)]
pub fn run_blocking(root: PathBuf, ready: mpsc::Sender<()>) {
    run(root, ready);
}

/// Watch the git directories of `root`'s repository so a commit made outside the
/// editor is seen even when it never touches a worktree path, and return them
/// for [`is_head_move`] to match against.
///
/// `refs/` is watched recursively (branch tips are nested: `refs/heads/foo/bar`);
/// the git directory itself only non-recursively, so the object churn of a
/// commit, fetch or gc under `.git/objects` never reaches us. Directories
/// already covered by the recursive watch on `root` are skipped, so the common
/// case (editor launched from the repo root) adds no second watch at all.
fn watch_git_dirs(watcher: &mut dyn Watcher, root: &Path) -> Vec<PathBuf> {
    let git_dirs = zmax_vcs::head_watch_dirs(root);
    for git_dir in &git_dirs {
        if git_dir.starts_with(root) {
            continue; // already inside the recursive root watch
        }
        for (dir, mode) in [
            (git_dir.clone(), RecursiveMode::NonRecursive),
            (git_dir.join("refs"), RecursiveMode::Recursive),
        ] {
            if let Err(err) = watcher.watch(&dir, mode) {
                log::warn!("could not watch {}: {err}", dir.display());
            }
        }
    }
    git_dirs
}

fn run(root: PathBuf, ready: mpsc::Sender<()>) {
    let (tx, rx) = mpsc::channel();
    let mut watcher = match notify::recommended_watcher(move |res| {
        // Forward both events and errors; the loop decides what to do.
        let _ = tx.send(res);
    }) {
        Ok(watcher) => watcher,
        Err(err) => {
            log::warn!("file watcher unavailable: {err}");
            return;
        }
    };

    if let Err(err) = watcher.watch(&root, RecursiveMode::Recursive) {
        log::warn!("could not watch {}: {err}", root.display());
        return;
    }
    let git_dirs = watch_git_dirs(&mut watcher, &root);

    // Every watch is live: changes from here on are reported.
    let _ = ready.send(());

    // Keep `watcher` alive for the lifetime of this thread.
    loop {
        // Block until something happens.
        let first = match rx.recv() {
            Ok(event) => event,
            Err(_) => return, // sender dropped — watcher gone
        };

        let mut relevant = event_is_relevant(&first);
        let mut head_moved = event_moves_head(&first, &git_dirs);
        let mut changed: Vec<PathBuf> = changed_paths(&first);

        // Coalesce a burst (e.g. a `git checkout` touching many files) into one
        // refresh so we don't rebuild the tree dozens of times. This also lets
        // git finish its ref-lock dance (write `refs/heads/x.lock`, rename it
        // over `refs/heads/x`) before we read HEAD back.
        while let Ok(event) = rx.recv_timeout(Duration::from_millis(150)) {
            relevant |= event_is_relevant(&event);
            head_moved |= event_moves_head(&event, &git_dirs);
            changed.extend(changed_paths(&event));
        }

        if relevant || head_moved {
            changed.sort();
            changed.dedup();
            job::dispatch_blocking(move |editor, compositor| {
                // Auto-reload any open buffer whose file changed on disk
                // (vim `autoread`); `auto_reload_file` honors the setting,
                // skips the editor's own saves, and protects unsaved edits.
                for path in &changed {
                    editor.auto_reload_file(path);
                }
                // HEAD moved under us (a commit/checkout/reset/rebase in another
                // terminal): the diff base of every open buffer is now the old
                // commit's blob, so the gutters still show the pre-commit hunks.
                // Re-fetch the base only — never the buffer text, so this is safe
                // on buffers with unsaved edits.
                if head_moved {
                    crate::commands::refresh_all_diff_bases(editor);
                }
                if let Some(view) = compositor.find::<EditorView>() {
                    view.refresh_file_tree();
                }
            });
        }
    }
}

/// True if the event touches a git ref file whose change moves HEAD, making the
/// diff base of every open buffer stale. See [`is_head_move`].
fn event_moves_head(event: &notify::Result<notify::Event>, git_dirs: &[PathBuf]) -> bool {
    match event {
        Ok(event) => event.paths.iter().any(|path| is_head_move(path, git_dirs)),
        Err(_) => false,
    }
}

/// True if `path` is a git file whose change means HEAD moved.
///
/// The gutter's diff base is HEAD's blob, so the files that matter are the ones
/// that decide *which commit* HEAD is:
///
/// * `HEAD` — checkout, detach, or a commit while detached.
/// * `refs/heads/<branch>` — a commit, reset or rebase moving the branch tip.
///   (`git` writes the tip as a loose ref even in an otherwise packed repo.)
/// * `packed-refs` — `git pack-refs`/`gc` rewriting those tips.
/// * `ORIG_HEAD` — written by reset/rebase/merge before they move HEAD.
///
/// Deliberately *not* matched: `index` (staging moves the index, not HEAD, and
/// the gutter diffs against HEAD, so staging must not perturb it) and
/// `refs/remotes/**` (a fetch moves remote tips without touching the base).
///
/// Git's ref update is a lock dance — write `refs/heads/x.lock`, rename it over
/// `refs/heads/x` — and both of those paths match, so the refresh fires whether
/// the platform reports the temporary path, the final one, or both.
fn is_head_move(path: &Path, git_dirs: &[PathBuf]) -> bool {
    let Some(rel) = strip_git_dir(path, git_dirs) else {
        return false;
    };
    if matches!(
        rel.file_name().and_then(|name| name.to_str()),
        Some("HEAD" | "ORIG_HEAD" | "packed-refs")
    ) {
        return true;
    }
    rel.starts_with("refs/heads")
}

/// The portion of `path` below the git directory it lives in, or `None` when it
/// is not inside one.
///
/// `git_dirs` are the directories discovered at watch time, which is the only
/// way to recognize a git dir that is not named `.git` (`--separate-git-dir`).
/// The literal-component fallback covers the ordinary layout, plus a linked
/// worktree's `<main>/.git/worktrees/<name>/HEAD`, whose tail still ends in the
/// file name keyed on above.
fn strip_git_dir<'a>(path: &'a Path, git_dirs: &[PathBuf]) -> Option<&'a Path> {
    if let Some(rel) = git_dirs.iter().find_map(|dir| path.strip_prefix(dir).ok()) {
        return Some(rel);
    }
    let mut components = path.components();
    components
        .find(|component| component.as_os_str() == ".git")
        .map(|_| components.as_path())
}

/// Non-ignored paths touched by an event, used to drive buffer auto-reload.
/// `Editor::auto_reload_file` filters these down to open buffers whose file
/// genuinely changed on disk, so collecting every touched path here is fine.
fn changed_paths(event: &notify::Result<notify::Event>) -> Vec<PathBuf> {
    match event {
        Ok(event) => event
            .paths
            .iter()
            .filter(|path| !is_ignored(path))
            .cloned()
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// True if an event touches a path we actually display (outside ignored dirs).
fn event_is_relevant(event: &notify::Result<notify::Event>) -> bool {
    match event {
        Ok(event) => event.paths.is_empty() || event.paths.iter().any(|path| !is_ignored(path)),
        // On error, be conservative and refresh.
        Err(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::{changed_paths, event_moves_head, is_head_move};
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    use notify::{RecursiveMode, Watcher};

    fn modify_event(paths: &[&str]) -> notify::Result<notify::Event> {
        let mut event =
            notify::Event::new(notify::EventKind::Modify(notify::event::ModifyKind::Any));
        for p in paths {
            event = event.add_path(PathBuf::from(p));
        }
        Ok(event)
    }

    #[test]
    fn changed_paths_keeps_real_files_and_drops_ignored() {
        let got = changed_paths(&modify_event(&[
            "/repo/src/main.rs",
            "/repo/target/debug/zmax", // ignored: target/
            "/repo/.git/index",          // ignored: .git/
            "/repo/node_modules/x/y.js", // ignored: node_modules/
            "/repo/docs/readme.md",
        ]));
        assert_eq!(
            got,
            vec![
                PathBuf::from("/repo/src/main.rs"),
                PathBuf::from("/repo/docs/readme.md"),
            ]
        );
    }

    #[test]
    fn changed_paths_on_error_is_empty() {
        let err: notify::Result<notify::Event> = Err(notify::Error::generic("watch error"));
        assert!(changed_paths(&err).is_empty());
    }

    /// The gutter's diff base is HEAD's blob: exactly the writes that move HEAD
    /// must trigger a refresh, and the far noisier writes that do not (objects,
    /// index, remote tips, worktree files) must not — a commit writes hundreds
    /// of the former and a fetch thousands.
    #[test]
    fn head_moves_are_ref_writes_only() {
        let head_move = [
            "/repo/.git/refs/heads/main",        // commit / reset moves the tip
            "/repo/.git/refs/heads/main.lock",   // ...seen mid-lock-dance
            "/repo/.git/refs/heads/feat/nested", // hierarchical branch name
            "/repo/.git/HEAD",                   // checkout / detach
            "/repo/.git/ORIG_HEAD",              // rebase / merge / reset
            "/repo/.git/packed-refs",            // gc / pack-refs
            "/repo/.git/worktrees/wt/HEAD",      // commit in a linked worktree
        ];
        for path in head_move {
            assert!(is_head_move(Path::new(path), &[]), "{path} should refresh");
        }

        let no_head_move = [
            "/repo/.git/index",                    // staging: the base is HEAD, not the index
            "/repo/.git/refs/remotes/origin/main", // fetch: remote tips are not the base
            "/repo/.git/objects/ab/cdef",          // object churn
            "/repo/.git/COMMIT_EDITMSG",
            "/repo/src/main.rs",     // ordinary worktree file
            "/repo/HEAD",            // a worktree file that merely shares the name
            "/repo/refs/heads/main", // ...likewise
        ];
        for path in no_head_move {
            assert!(
                !is_head_move(Path::new(path), &[]),
                "{path} should not refresh"
            );
        }
    }

    /// A git dir that is not named `.git` (`--separate-git-dir`, submodules) is
    /// only recognizable through the dirs discovered at watch time.
    #[test]
    fn head_moves_in_a_git_dir_not_named_dot_git() {
        let git_dirs = [PathBuf::from("/store/gitdirs/repo")];
        assert!(is_head_move(
            Path::new("/store/gitdirs/repo/refs/heads/main"),
            &git_dirs
        ));
        assert!(is_head_move(
            Path::new("/store/gitdirs/repo/HEAD"),
            &git_dirs
        ));
        assert!(!is_head_move(
            Path::new("/store/gitdirs/repo/index"),
            &git_dirs
        ));
    }

    fn git(args: &[&str], cwd: &Path) {
        let out = Command::new("git")
            .args(args)
            .current_dir(cwd)
            .env("GIT_CONFIG_COUNT", "1")
            .env("GIT_CONFIG_KEY_0", "init.defaultBranch")
            .env("GIT_CONFIG_VALUE_0", "main")
            .output()
            .expect("run git");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// The load-bearing assumption of the whole fix: a commit made *outside* the
    /// editor writes only inside `.git`, and the platform watcher does deliver
    /// those writes to us. If this regresses (an OS backend stops reporting ref
    /// writes, or `.git` gets filtered before classification again), the gutters
    /// silently keep showing pre-commit hunks — the bug this test pins.
    #[test]
    fn a_commit_outside_the_editor_reaches_the_watcher_as_a_head_move() {
        let tmp = tempfile::tempdir().expect("temp dir");
        // macOS reports events under /private/var, not the /var symlink.
        let root = tmp.path().canonicalize().expect("canonicalize");

        git(&["init"], &root);
        git(&["config", "user.email", "test@example.com"], &root);
        git(&["config", "user.name", "test"], &root);
        git(&["config", "commit.gpgsign", "false"], &root);
        std::fs::write(root.join("file.txt"), "one\n").expect("write");
        git(&["add", "-A"], &root);
        git(&["commit", "-m", "first"], &root);

        let (tx, rx) = mpsc::channel();
        let mut watcher = notify::recommended_watcher(move |res| {
            let _ = tx.send(res);
        })
        .expect("watcher");
        watcher
            .watch(&root, RecursiveMode::Recursive)
            .expect("watch root");

        // The buffer's file is left untouched: the second commit only rewrites
        // refs/heads/main and HEAD's log, which is precisely why nothing but a
        // `.git` watch can notice it.
        git(&["commit", "--allow-empty", "-m", "external"], &root);

        let deadline = Instant::now() + Duration::from_secs(20);
        let mut saw_head_move = false;
        while Instant::now() < deadline && !saw_head_move {
            match rx.recv_timeout(Duration::from_millis(500)) {
                Ok(event) => saw_head_move = event_moves_head(&event, &[]),
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        assert!(
            saw_head_move,
            "no HEAD-move event for an external commit — git gutters would stay stale"
        );
    }
}
