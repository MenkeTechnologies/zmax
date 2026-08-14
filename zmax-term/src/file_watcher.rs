//! Filesystem watcher that live-refreshes the IDE file tree when files change
//! on disk outside the editor (create/delete/rename/modify), and the git gutters
//! when HEAD moves outside the editor.
//!
//! A dedicated OS thread owns the `notify` watcher and a receive loop. On a
//! relevant event it coalesces a short burst, then hops onto the main thread via
//! [`job::dispatch_blocking`] to rebuild the tree; the event loop renders right
//! after each dispatched callback, so the change shows up immediately.
//!
//! The launch directory is watched at boot, but the editor also opens files from
//! unrelated directories (`zmax /other/repo/file.rs` run from `~`), whose worktree
//! *and* `.git` live nowhere under it. [`watch_workspaces`] adds those roots to the
//! live watcher after the fact — the event loop feeds it every open buffer's
//! workspace root each tick — so an external edit or commit to any open file is
//! seen no matter where it lives.
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
//!
//! The platform watcher is not the only source, because it can stop delivering
//! without saying so — see [`poll`], which checks the same two things directly
//! every [`POLL_INTERVAL`] and reports only what actually moved.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use notify::{RecursiveMode, Watcher};

use crate::job;
use crate::ui::EditorView;

/// Ensures we only ever spawn a single watcher for the process.
static SPAWNED: AtomicBool = AtomicBool::new(false);

/// Sender into the live watcher thread, set once [`spawn`] runs. [`watch_workspaces`]
/// uses it to add roots discovered after boot; `None` before the watcher exists.
static SENDER: OnceLock<mpsc::Sender<Msg>> = OnceLock::new();

/// Roots already handed to the watcher (launch dir + every added workspace). Used
/// to skip a root already covered by an existing recursive watch, so feeding the
/// same open buffers every event-loop tick is close to free.
static ROOTS: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// Every open buffer's file, refreshed by the event loop through
/// [`track_open_files`] and read by the watcher thread's poll tick. Shared
/// rather than sent down the channel because the poll wants the *current* set,
/// not a queue of every set it has ever been.
static OPEN_FILES: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// A message the watcher loop consumes: either a filesystem event forwarded from
/// `notify`, or a request to start watching another root (a buffer opened from a
/// directory the launch-dir watch does not cover). Both share one channel so the
/// loop can register the new watch on the thread that owns the `notify::Watcher`.
enum Msg {
    Event(notify::Result<notify::Event>),
    AddRoot(PathBuf),
}

/// Hand every workspace root to the watcher once, so external edits and commits
/// to buffers opened outside the launch directory are seen. Cheap to call on
/// every event-loop tick: one lock, and a root already handed over is skipped.
///
/// Deliberately deduplicated by *exact* root, not by containment: a workspace
/// whose worktree sits inside an already-watched tree can still keep its refs
/// somewhere else entirely — a submodule's git dir is `<super>/.git/modules/…`,
/// a linked worktree's refs live in its common dir, `--separate-git-dir` puts
/// them anywhere at all. [`register_root`] skips the redundant recursive watch
/// but still discovers those git dirs, which is what classifies a later ref
/// write as a HEAD move.
///
/// No-op until [`spawn`] has installed the watcher; the launch-dir watch covers
/// the common case until then, and the reconcile call fires again next tick.
pub fn watch_workspaces<'a>(roots: impl Iterator<Item = &'a Path>) {
    let Some(tx) = SENDER.get() else {
        return;
    };
    let mut known = ROOTS.lock().unwrap_or_else(|p| p.into_inner());
    for root in roots {
        if known.iter().any(|w| w == root) {
            continue; // already handed to the watcher
        }
        known.push(root.to_path_buf());
        let _ = tx.send(Msg::AddRoot(root.to_path_buf()));
    }
}

/// Record the files currently open in buffers, so the watcher thread's poll tick
/// can notice a change the platform watcher never reported. Called from the
/// event loop next to [`watch_workspaces`]; writes only when the set actually
/// changed, so the common tick is one lock and a comparison.
pub fn track_open_files<'a>(paths: impl Iterator<Item = &'a Path>) {
    let current: Vec<PathBuf> = paths.map(Path::to_path_buf).collect();
    let mut open = OPEN_FILES.lock().unwrap_or_else(|p| p.into_inner());
    if *open != current {
        *open = current;
    }
}

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

    // The loop owns the `notify::Watcher`, so a root discovered later (a buffer
    // opened elsewhere) must be handed in over this channel for the loop to
    // register — the notify callback forwards events on the same channel.
    let (tx, rx) = mpsc::channel();
    let _ = SENDER.set(tx.clone());
    // Seed the shared root set so buffers under the launch dir are recognized as
    // already covered and never re-sent by `watch_workspaces`.
    ROOTS
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .push(root.clone());

    // Startup must not block on registering the watches (a recursive add over a
    // large tree is not instant), so nothing here waits for readiness.
    let (ready, _) = mpsc::channel();
    std::thread::Builder::new()
        .name("file-tree-watcher".into())
        .spawn(move || run(root, tx, rx, ready))
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
    let (tx, rx) = mpsc::channel();
    run(root, tx, rx, ready);
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

/// Start watching `root` recursively (plus its git ref dirs), tracking it in
/// `watched` and folding its git dirs into `git_dirs` for [`is_head_move`].
///
/// A root already covered by an existing recursive watch skips the redundant
/// `watch` call — but *not* the git-dir discovery, because containment of the
/// worktree says nothing about where that workspace keeps its refs (a
/// submodule's git dir is `<super>/.git/modules/<path>`, a linked worktree's
/// refs live in its common dir). Skipping discovery there left those ref writes
/// unwatched and unclassified, so a commit in a submodule never refreshed the
/// gutters of its open buffers.
fn register_root(
    watcher: &mut dyn Watcher,
    root: PathBuf,
    watched: &mut Vec<PathBuf>,
    git_dirs: &mut Vec<PathBuf>,
) {
    let covered = watched.iter().any(|w| root.starts_with(w));
    if !covered {
        if let Err(err) = watcher.watch(&root, RecursiveMode::Recursive) {
            log::warn!("could not watch {}: {err}", root.display());
            return;
        }
    }
    git_dirs.append(&mut watch_git_dirs(watcher, &root));
    git_dirs.sort();
    git_dirs.dedup();
    watched.push(root);
}

fn run(root: PathBuf, tx: mpsc::Sender<Msg>, rx: mpsc::Receiver<Msg>, ready: mpsc::Sender<()>) {
    let mut watcher = match notify::recommended_watcher(move |res| {
        // Forward both events and errors; the loop decides what to do.
        let _ = tx.send(Msg::Event(res));
    }) {
        Ok(watcher) => watcher,
        Err(err) => {
            log::warn!("file watcher unavailable: {err}");
            return;
        }
    };

    // Roots under a live recursive watch, and the git dirs found under them.
    // `register_root` grows both as `watch_workspaces` hands in new workspaces.
    let mut watched: Vec<PathBuf> = Vec::new();
    let mut git_dirs: Vec<PathBuf> = Vec::new();
    register_root(&mut watcher, root, &mut watched, &mut git_dirs);

    // Every watch is live: changes from here on are reported.
    let _ = ready.send(());

    // Snapshot of what the poll tick last saw on disk, so it can report only
    // genuine changes. Empty until the first tick fills it in.
    let mut seen = Snapshot::default();

    // Keep `watcher` alive for the lifetime of this thread.
    loop {
        let mut relevant = false;
        let mut head_moved = false;
        let mut changed: Vec<PathBuf> = Vec::new();

        // Wait for an event, but never longer than a poll interval: the platform
        // watcher is not trustworthy enough to be the only source. See [`poll`].
        match rx.recv_timeout(POLL_INTERVAL) {
            Ok(first) => {
                apply(
                    first,
                    &mut watcher,
                    &mut watched,
                    &mut git_dirs,
                    &mut relevant,
                    &mut head_moved,
                    &mut changed,
                );

                // Coalesce a burst (e.g. a `git checkout` touching many files)
                // into one refresh so we don't rebuild the tree dozens of times.
                // This also lets git finish its ref-lock dance (write
                // `refs/heads/x.lock`, rename it over `refs/heads/x`) before we
                // read HEAD back.
                let started = Instant::now();
                while let Some(window) = burst_window(started, Instant::now()) {
                    let Ok(msg) = rx.recv_timeout(window) else {
                        break;
                    };
                    apply(
                        msg,
                        &mut watcher,
                        &mut watched,
                        &mut git_dirs,
                        &mut relevant,
                        &mut head_moved,
                        &mut changed,
                    );
                }
                // The burst may have moved HEAD or touched a buffer's file; fold
                // that into the snapshot so the next tick does not report it a
                // second time.
                poll(
                    &mut seen,
                    &git_dirs,
                    &mut false,
                    &mut false,
                    &mut Vec::new(),
                );
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                poll(
                    &mut seen,
                    &git_dirs,
                    &mut relevant,
                    &mut head_moved,
                    &mut changed,
                );
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => return, // watcher gone
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

/// How often the watcher thread checks the filesystem itself, independently of
/// anything the platform reports. See [`poll`].
const POLL_INTERVAL: Duration = Duration::from_secs(2);

/// What the last poll tick saw on disk: each open buffer's file modification
/// time, and each git directory's HEAD (the `HEAD` file's contents plus the
/// modification time of whatever it resolves to). A change in either is what
/// makes a tick report something.
#[derive(Default)]
struct Snapshot {
    files: Vec<(PathBuf, std::time::SystemTime)>,
    heads: Vec<(PathBuf, HeadState)>,
}

/// A git directory's HEAD as a poll tick can cheaply observe it: the contents of
/// `HEAD` (which branch, or which commit when detached) and the modification
/// time of the branch tip it names. Comparing both catches a checkout (contents
/// change) and a commit on the same branch (tip changes).
#[derive(PartialEq, Eq)]
struct HeadState {
    head: String,
    tip: Option<std::time::SystemTime>,
}

impl Snapshot {
    /// Replace `key`'s entry in `entries`, returning whether it differed from
    /// what was there. A key seen for the first time is recorded and reported as
    /// unchanged — the first tick establishes the baseline, it does not refresh
    /// the world.
    fn update<T: PartialEq>(entries: &mut Vec<(PathBuf, T)>, key: &Path, value: T) -> bool {
        match entries.iter_mut().find(|(path, _)| path == key) {
            Some((_, current)) => {
                let changed = *current != value;
                *current = value;
                changed
            }
            None => {
                entries.push((key.to_path_buf(), value));
                false
            }
        }
    }
}

/// Read a git directory's HEAD state, or `None` when it cannot be read at all.
fn head_state(git_dir: &Path) -> Option<HeadState> {
    let head = std::fs::read_to_string(git_dir.join("HEAD")).ok()?;
    // `ref: refs/heads/<branch>` — a detached HEAD holds the commit id instead,
    // and that id changing is itself the whole signal, so there is no tip.
    let tip = head
        .strip_prefix("ref:")
        .map(str::trim)
        .map(|reference| git_dir.join(reference))
        .and_then(|tip| std::fs::metadata(tip).ok())
        .and_then(|meta| meta.modified().ok());
    Some(HeadState {
        head: head.trim().to_owned(),
        tip,
    })
}

/// Check the filesystem directly: every open buffer's file, and every known git
/// directory's HEAD. Sets the same flags an event would.
///
/// This exists because the platform watcher cannot be trusted to be the only
/// source. On macOS an `fseventsd` that wedges — which a full disk is enough to
/// cause — stops delivering to every client on the volume while still accepting
/// their streams, and a stream does not survive the daemon being restarted
/// either. There is nothing to detect and no error to report: the editor simply
/// stops seeing external commits and external edits, indistinguishably from a
/// filesystem where nothing is happening. A tick costs one `stat` per open
/// buffer plus one small read per git directory, and reports nothing when
/// nothing moved, so the cost of not needing it is negligible.
fn poll(
    seen: &mut Snapshot,
    git_dirs: &[PathBuf],
    relevant: &mut bool,
    head_moved: &mut bool,
    changed: &mut Vec<PathBuf>,
) {
    let open = OPEN_FILES.lock().unwrap_or_else(|p| p.into_inner());
    for path in open.iter() {
        let Ok(mtime) = std::fs::metadata(path).and_then(|meta| meta.modified()) else {
            continue; // deleted or unreadable: the buffer keeps what it has
        };
        if Snapshot::update(&mut seen.files, path, mtime) {
            *relevant = true;
            changed.push(path.clone());
        }
    }
    // Forget files no longer open, so a session that visits thousands of buffers
    // over days does not carry an entry for every one of them.
    seen.files.retain(|(path, _)| open.contains(path));
    drop(open);

    for git_dir in git_dirs {
        let Some(state) = head_state(git_dir) else {
            continue;
        };
        if Snapshot::update(&mut seen.heads, git_dir, state) {
            *head_moved = true;
        }
    }
}

/// Quiet period that ends a burst: no further event for this long flushes the
/// pending refresh.
const BURST_QUIET: Duration = Duration::from_millis(150);

/// Hard ceiling on a burst, however busy the tree stays. Without it a burst is
/// extended by *every* arriving event, and a tree that never falls quiet for
/// 150ms — a `cargo build` writing `target/`, a `node_modules` install, several
/// builds at once — postpones the refresh for as long as the churn lasts. The
/// events driving it are ones we then discard as ignored, so the editor would
/// sit on stale gutters and unreloaded buffers precisely while the machine is
/// busiest.
const BURST_CAP: Duration = Duration::from_millis(500);

/// How long the burst that began at `started` may keep waiting for more events,
/// or `None` once it has run long enough and must be flushed now.
fn burst_window(started: Instant, now: Instant) -> Option<Duration> {
    let elapsed = now.saturating_duration_since(started);
    let left = BURST_CAP.checked_sub(elapsed)?;
    if left.is_zero() {
        return None;
    }
    Some(BURST_QUIET.min(left))
}

/// Fold one [`Msg`] into the pending-refresh state. A filesystem event updates
/// the relevance/head-move flags and the changed-path list; an `AddRoot` request
/// registers a new watch on the thread that owns the `notify::Watcher` and never
/// itself triggers a refresh (starting to watch is not a change).
#[allow(clippy::too_many_arguments)]
fn apply(
    msg: Msg,
    watcher: &mut dyn Watcher,
    watched: &mut Vec<PathBuf>,
    git_dirs: &mut Vec<PathBuf>,
    relevant: &mut bool,
    head_moved: &mut bool,
    changed: &mut Vec<PathBuf>,
) {
    match msg {
        Msg::Event(event) => {
            *relevant |= event_is_relevant(&event);
            *head_moved |= event_moves_head(&event, git_dirs);
            changed.extend(changed_paths(&event));
        }
        Msg::AddRoot(root) => register_root(watcher, root, watched, git_dirs),
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
    holds_branch_tip(rel)
}

/// True when `rel` — a path already known to live inside a git directory —
/// holds a branch tip, i.e. contains the consecutive components `refs/heads`.
///
/// Anchoring at the start (`rel.starts_with("refs/heads")`) is not enough: a
/// submodule's git dir is `<super>/.git/modules/<path>`, so when the
/// superproject's `.git` is the git dir we recognized, a commit inside the
/// submodule arrives as `modules/<path>/refs/heads/<branch>` and was classified
/// as irrelevant — the exact case where every open buffer belongs to a submodule
/// of the repo the editor was launched from.
///
/// `refs/remotes/**` still does not match, so a fetch remains a non-event.
fn holds_branch_tip(rel: &Path) -> bool {
    let components: Vec<_> = rel.components().map(|c| c.as_os_str()).collect();
    components
        .windows(2)
        .any(|pair| pair[0] == "refs" && pair[1] == "heads")
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
    // Longest match wins (shortest remainder): with both a superproject's
    // `.git` and a submodule's `.git/modules/<path>` known, the submodule's is
    // the one that makes `refs/heads/<branch>` the remainder.
    if let Some(rel) = git_dirs
        .iter()
        .filter_map(|dir| path.strip_prefix(dir).ok())
        .min_by_key(|rel| rel.components().count())
    {
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
    use super::{
        apply, burst_window, changed_paths, event_moves_head, is_head_move, poll, Msg, Snapshot,
        OPEN_FILES,
    };
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::mpsc;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use notify::{RecursiveMode, Watcher};

    /// A `Watcher` that only records the `(path, mode)` pairs it is asked to watch,
    /// so a test can assert which roots `register_root`/`apply` register without a
    /// real OS backend.
    struct RecordingWatcher {
        watched: Arc<Mutex<Vec<(PathBuf, RecursiveMode)>>>,
    }

    impl Watcher for RecordingWatcher {
        fn new<F: notify::EventHandler>(_: F, _: notify::Config) -> notify::Result<Self> {
            unreachable!("the test constructs RecordingWatcher directly")
        }
        fn watch(&mut self, path: &Path, mode: RecursiveMode) -> notify::Result<()> {
            self.watched
                .lock()
                .unwrap()
                .push((path.to_path_buf(), mode));
            Ok(())
        }
        fn unwatch(&mut self, _: &Path) -> notify::Result<()> {
            Ok(())
        }
        fn kind() -> notify::WatcherKind {
            notify::WatcherKind::NullWatcher
        }
    }

    /// The bug: a file opened from a directory the launch-dir watch does not cover
    /// stayed unwatched, so external edits and commits to it were never seen. An
    /// `AddRoot` must register a fresh recursive watch on that workspace — once —
    /// and a second file already inside a watched root must add no new watch.
    /// Registering a watch is not itself a change, so no refresh must be flagged.
    #[test]
    fn add_root_watches_a_new_workspace_once_and_flags_no_refresh() {
        let recorded = Arc::new(Mutex::new(Vec::new()));
        let mut watcher = RecordingWatcher {
            watched: recorded.clone(),
        };
        let mut watched = Vec::new();
        let mut git_dirs = Vec::new();
        let (mut relevant, mut head_moved, mut changed) = (false, false, Vec::new());

        let launch = PathBuf::from("/launch");
        let other = PathBuf::from("/elsewhere/repo");
        for root in [
            launch.clone(),
            other.clone(),
            other.join("src/deep"), // already covered by `other`
            launch.join("sub"),     // already covered by `launch`
        ] {
            apply(
                Msg::AddRoot(root),
                &mut watcher,
                &mut watched,
                &mut git_dirs,
                &mut relevant,
                &mut head_moved,
                &mut changed,
            );
        }

        let roots: Vec<PathBuf> = recorded
            .lock()
            .unwrap()
            .iter()
            .map(|(path, _)| path.clone())
            .collect();
        assert_eq!(
            roots,
            vec![launch, other],
            "each distinct workspace watched once; paths under one already watched are skipped"
        );
        assert!(
            !relevant && !head_moved && changed.is_empty(),
            "starting to watch a root is not a filesystem change and must not trigger a refresh"
        );
    }

    /// A burst must be flushed even when the tree never falls quiet. The old
    /// loop waited 150ms *per event*, so an unbroken stream — `cargo build`
    /// writing `target/`, which is this editor's own normal working condition —
    /// extended the wait indefinitely and the refresh never ran. The window must
    /// shrink toward the cap and then close.
    #[test]
    fn a_burst_is_capped_however_busy_the_tree_stays() {
        let started = Instant::now();

        assert_eq!(
            burst_window(started, started),
            Some(super::BURST_QUIET),
            "a fresh burst waits the full quiet period"
        );

        let nearly_done = started + super::BURST_CAP - Duration::from_millis(20);
        assert_eq!(
            burst_window(started, nearly_done),
            Some(Duration::from_millis(20)),
            "close to the cap, the wait is trimmed to what is left of it"
        );

        for elapsed in [super::BURST_CAP, super::BURST_CAP + Duration::from_secs(9)] {
            assert_eq!(
                burst_window(started, started + elapsed),
                None,
                "past the cap the burst must flush instead of waiting for quiet"
            );
        }
    }

    /// The poll tick is the editor's guarantee that it sees external changes
    /// even when the platform watcher reports nothing at all — a wedged
    /// `fseventsd`, a daemon restart that invalidates live streams, a path the
    /// backend does not cover. The first tick only establishes a baseline; a
    /// later write must be reported exactly once.
    #[test]
    fn a_poll_tick_reports_external_writes_without_any_platform_event() {
        let dir = tempfile::tempdir().expect("temp dir");
        let file = dir.path().join("buffer.txt");
        std::fs::write(&file, "one\n").expect("write");
        *OPEN_FILES.lock().unwrap() = vec![file.clone()];

        let mut seen = Snapshot::default();
        let poll_once = |seen: &mut Snapshot| {
            let (mut relevant, mut head_moved, mut changed) = (false, false, Vec::new());
            poll(seen, &[], &mut relevant, &mut head_moved, &mut changed);
            (relevant, changed)
        };

        let (relevant, changed) = poll_once(&mut seen);
        assert!(
            !relevant && changed.is_empty(),
            "the first tick records what is on disk; it must not claim a change"
        );

        // A different process writes the file. No event is delivered to anyone.
        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(&file, "one\ntwo\n").expect("write");

        let (relevant, changed) = poll_once(&mut seen);
        assert!(
            relevant,
            "the write must be noticed without a platform event"
        );
        assert_eq!(changed, vec![file.clone()]);

        let (relevant, changed) = poll_once(&mut seen);
        assert!(
            !relevant && changed.is_empty(),
            "the same write must not be reported again on the next tick"
        );

        OPEN_FILES.lock().unwrap().clear();
    }

    /// The other half: a commit made while the platform watcher is silent must
    /// still refresh the gutters, which means noticing HEAD by reading it.
    #[test]
    fn a_poll_tick_notices_a_commit_the_platform_never_reported() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let root = tmp.path().canonicalize().expect("canonicalize");
        git(&["init"], &root);
        git(&["config", "user.email", "test@example.com"], &root);
        git(&["config", "user.name", "test"], &root);
        git(&["config", "commit.gpgsign", "false"], &root);
        std::fs::write(root.join("file.txt"), "one\n").expect("write");
        git(&["add", "-A"], &root);
        git(&["commit", "-m", "first"], &root);

        let git_dirs = [root.join(".git")];
        let mut seen = Snapshot::default();
        let poll_once = |seen: &mut Snapshot| {
            let (mut relevant, mut head_moved, mut changed) = (false, false, Vec::new());
            poll(
                seen,
                &git_dirs,
                &mut relevant,
                &mut head_moved,
                &mut changed,
            );
            head_moved
        };

        assert!(!poll_once(&mut seen), "the first tick is the baseline");

        std::thread::sleep(Duration::from_millis(20));
        git(&["commit", "--allow-empty", "-m", "external"], &root);

        assert!(
            poll_once(&mut seen),
            "a commit moved the branch tip — the gutters' diff base is now stale"
        );
        assert!(
            !poll_once(&mut seen),
            "HEAD has not moved again; a settled repo must stay quiet"
        );
    }

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
            "/repo/target/debug/zmax",   // ignored: target/
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

    /// The submodule bug: every MenkeTech repo is a shell of submodules, so the
    /// buffers open in the editor usually belong to one while the git dir the
    /// watcher discovered is the *superproject's* `.git`. A commit inside the
    /// submodule then writes `<super>/.git/modules/<path>/refs/heads/<branch>`,
    /// which the old start-anchored match classified as irrelevant — the gutters
    /// kept showing pre-commit hunks until the buffer was reopened.
    #[test]
    fn head_moves_in_a_submodule_under_the_superproject_git_dir() {
        let super_git = [PathBuf::from("/meta/.git")];
        for path in [
            "/meta/.git/modules/zmax/refs/heads/main", // commit inside the submodule
            "/meta/.git/modules/zmax/refs/heads/main.lock",
            "/meta/.git/modules/zmax/HEAD", // checkout inside the submodule
            "/meta/.git/modules/zmax/ORIG_HEAD", // rebase / reset inside it
        ] {
            assert!(
                is_head_move(Path::new(path), &super_git),
                "{path} moves a submodule's HEAD and must refresh the gutters"
            );
        }

        for path in [
            "/meta/.git/modules/zmax/index",                    // staging only
            "/meta/.git/modules/zmax/refs/remotes/origin/main", // fetch only
            "/meta/.git/modules/zmax/objects/ab/cdef",          // object churn
        ] {
            assert!(
                !is_head_move(Path::new(path), &super_git),
                "{path} leaves HEAD where it is and must not refresh"
            );
        }
    }

    /// With both the superproject's git dir and the submodule's own known, the
    /// longest match must win — stripping the shorter one first would leave
    /// `modules/zmax/...` and lose the `HEAD`/`ORIG_HEAD` file-name match.
    #[test]
    fn the_longest_git_dir_match_wins() {
        let git_dirs = [
            PathBuf::from("/meta/.git"),
            PathBuf::from("/meta/.git/modules/zmax"),
        ];
        assert!(is_head_move(
            Path::new("/meta/.git/modules/zmax/HEAD"),
            &git_dirs
        ));
        assert!(is_head_move(
            Path::new("/meta/.git/modules/zmax/refs/heads/main"),
            &git_dirs
        ));
        assert!(!is_head_move(
            Path::new("/meta/.git/modules/zmax/index"),
            &git_dirs
        ));
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
