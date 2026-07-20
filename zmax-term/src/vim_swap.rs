//! vim `swapfile`: a recovery swap file of unsaved changes. While a buffer is
//! edited (and `:set swapfile` is on) its contents are periodically written to a
//! `.<name>.swp` file; the swap is removed on a clean save. If a swap file
//! already exists when a file is opened, the user is warned (recovery awareness),
//! as vim's `E325`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use zmax_view::{Document, DocumentId};

/// Cached `swapfile` / `directory` config so the change hook (which only gets the
/// document) can act without the editor config.
static SWAPFILE_ON: AtomicBool = AtomicBool::new(false);
static SWAP_DIR: Mutex<String> = Mutex::new(String::new());

thread_local! {
    // Per-document change counter, so the full buffer isn't rewritten on every
    // keystroke — only every `updatecount` changes.
    static COUNTERS: RefCell<HashMap<DocumentId, usize>> = RefCell::new(HashMap::new());
    // Documents opened under vim `:noswapfile` — the modifier says "this command
    // doesn't touch the swap file", and a buffer that was opened that way must
    // not grow one behind the user's back either.
    static NO_SWAP: RefCell<std::collections::HashSet<DocumentId>> =
        RefCell::new(std::collections::HashSet::new());
}

/// vim `:noswapfile {cmd}` — the document `{cmd}` opened keeps no swap file, for
/// as long as it is open. Called by the command layer once the wrapped command
/// has run (it is the command that knows the modifier was there).
pub fn set_no_swap(doc: DocumentId) {
    NO_SWAP.with(|s| s.borrow_mut().insert(doc));
}

/// Whether `doc` was opened under `:noswapfile`.
fn no_swap(doc: DocumentId) -> bool {
    NO_SWAP.with(|s| s.borrow().contains(&doc))
}

/// vim `updatecount`'s own default: the swap file is rewritten after this many
/// changes when the option was never `:set`.
const UPDATECOUNT_DEFAULT: usize = 200;

/// vim `updatecount`: "After typing this many characters the swap file will be
/// written to disk. When zero, no swap file will be produced at all." `count` is
/// the document's running change count. Pure — unit tested.
fn swap_write_due(count: usize, updatecount: usize) -> bool {
    updatecount != 0 && count.is_multiple_of(updatecount)
}

/// The live `updatecount` (vim's default when it was never `:set`).
fn updatecount() -> usize {
    crate::commands::typed::vim_opt_num("updatecount")
        .or_else(|| crate::commands::typed::vim_opt_num("uc"))
        .unwrap_or(UPDATECOUNT_DEFAULT)
}

fn swap_dir() -> String {
    SWAP_DIR.lock().map(|d| d.clone()).unwrap_or_default()
}

/// Swap-file path for `file`: `<dir>/.<name>.swp`, or beside the file when no
/// swap directory is configured.
fn swap_path(file: &std::path::Path, dir: &str) -> Option<PathBuf> {
    let name = file.file_name()?.to_string_lossy();
    let swp = format!(".{name}.swp");
    if dir.is_empty() {
        Some(
            file.parent()
                .unwrap_or_else(|| std::path::Path::new("."))
                .join(swp),
        )
    } else {
        let dir = if let Some(rest) = dir.strip_prefix("~/") {
            std::env::var_os("HOME")
                .map(|h| PathBuf::from(h).join(rest))
                .unwrap_or_else(|| PathBuf::from(dir))
        } else {
            PathBuf::from(dir)
        };
        // Flatten the path into the swap dir name to avoid collisions.
        let flat = file.to_string_lossy().replace(['/', '\\'], "%");
        Some(dir.join(format!(".{flat}.swp")))
    }
}

/// Write the buffer to its swap file (best-effort).
fn write_swap(doc: &Document) {
    let dir = swap_dir();
    let Some(path) = doc.path().and_then(|p| swap_path(p, &dir)) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, doc.text().to_string());
}

/// The swap-file path for a document (vim `:swapname`), if it has a file name.
pub fn path_for(doc: &Document) -> Option<PathBuf> {
    swap_path(doc.path()?, &swap_dir())
}

/// vim `:preserve` — flush the buffer to its swap file now (rather than waiting
/// for the periodic change hook).
pub fn preserve(doc: &Document) {
    write_swap(doc);
}

/// vim `:recover` — the contents of the document's swap file, if one exists.
pub fn recover_text(doc: &Document) -> Option<String> {
    std::fs::read_to_string(path_for(doc)?).ok()
}

/// Remove a document's swap file (on a clean save).
pub fn remove(doc: &Document) {
    let dir = swap_dir();
    if let Some(path) = doc.path().and_then(|p| swap_path(p, &dir)) {
        let _ = std::fs::remove_file(path);
    }
}

/// Whether a swap file already exists for the document (recovery detection).
pub fn swap_exists(doc: &Document) -> bool {
    let dir = swap_dir();
    doc.path()
        .and_then(|p| swap_path(p, &dir))
        .map(|s| s.is_file())
        .unwrap_or(false)
}

// emacs interlocking (filelock.c / userlock.el): while a buffer visiting a file
// is modified, emacs holds a lock file `.#<name>` beside it — a symlink whose
// target is `user@host.pid` — so a second editor that starts modifying the same
// file notices and calls `ask-user-about-lock`. The lock is dropped when the
// buffer stops being modified or is killed.

/// What to do about a file locked by someone else. The three alternatives
/// `ask-user-about-lock` documents: "return t (grab the lock on the file)",
/// "return nil (edit the file even though it is locked)", and "do (signal
/// 'file-locked …) to refrain from editing the file".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockDecision {
    /// `s` — steal the lock.
    Steal,
    /// `p` — proceed, editing at your own risk without taking the lock.
    Proceed,
    /// `q` — refrain from editing this file.
    Quit,
}

/// The owner recorded in a lock file: emacs writes `user@host.pid`, with a
/// `:boot_time` suffix when it knows the boot time. zmax parses that suffix but
/// does not write one, so a pid reused across a reboot reads as live.
#[derive(Debug, Clone, PartialEq, Eq)]
struct LockInfo {
    user: String,
    host: String,
    pid: Option<i32>,
}

thread_local! {
    // Documents whose lock file we hold, so the lock is only taken once (on the
    // first modification, as emacs does) and only released by its owner.
    static LOCKED: RefCell<std::collections::HashSet<DocumentId>> =
        RefCell::new(std::collections::HashSet::new());
    // The `ask-user-about-lock` prompt for the most recent conflict, for the
    // command layer to show. emacs asks it mid-edit; zmax can only report it.
    static LOCK_PROMPT: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// emacs `create-lockfiles`: "Non-nil means use lockfiles to avoid editing
/// collisions." Default `t`, so an unset option means locking is on.
fn create_lockfiles() -> bool {
    match crate::commands::typed::vim_opt_str("create-lockfiles") {
        Some(v) => matches!(v.as_str(), "on" | "1" | "true" | "yes"),
        None => true,
    }
}

/// The configured answer to `ask-user-about-lock` (`s`/`p`/`q`, or the long
/// names). emacs loops on the query until the user picks one; with no way to
/// read a character mid-edit, zmax reads the answer from this setting and
/// otherwise proceeds — `p`, the only answer that neither takes the other
/// editor's lock nor stops the edit already made.
fn lock_answer(setting: Option<&str>) -> LockDecision {
    match setting {
        Some("s") | Some("steal") => LockDecision::Steal,
        Some("q") | Some("quit") => LockDecision::Quit,
        _ => LockDecision::Proceed,
    }
}

/// The lock-file path for `file`: `.#<name>` in the same directory (emacs
/// filelock.c `make_lock_file_name`).
fn lock_path(file: &std::path::Path) -> Option<PathBuf> {
    let name = file.file_name()?.to_string_lossy();
    Some(
        file.parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join(format!(".#{name}")),
    )
}

/// The lock-file path for a document, if it has a file name.
pub fn lock_path_for(doc: &Document) -> Option<PathBuf> {
    lock_path(doc.path()?)
}

/// Parse a lock file's contents, `user@host.pid[:boot_time]`. The host may
/// itself contain dots, so the pid is split off the end. Pure — unit tested.
fn parse_lock_info(target: &str) -> Option<LockInfo> {
    let (user, rest) = target.rsplit_once('@')?;
    // The boot time emacs appends is only there to catch pid reuse across a
    // reboot; zmax does not record one, and ignores one it reads.
    let rest = rest.split(':').next().unwrap_or(rest);
    let (host, pid) = match rest.rsplit_once('.') {
        Some((host, pid)) => (host, pid.parse().ok()),
        None => (rest, None),
    };
    Some(LockInfo {
        user: user.to_string(),
        host: host.to_string(),
        pid,
    })
}

/// The contents zmax writes into a lock file. Pure — unit tested.
fn lock_target(info: &LockInfo) -> String {
    match info.pid {
        Some(pid) => format!("{}@{}.{pid}", info.user, info.host),
        None => format!("{}@{}", info.user, info.host),
    }
}

/// The `opponent` string `ask-user-about-lock` is given: emacs formats it as
/// `user@host (pid NNN)`, dropping the pid when the lock file had none. Pure —
/// unit tested.
fn opponent(info: &LockInfo) -> String {
    match info.pid {
        Some(pid) => format!("{}@{} (pid {pid})", info.user, info.host),
        None => format!("{}@{}", info.user, info.host),
    }
}

/// This process's lock identity.
fn self_info() -> LockInfo {
    let user = std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_else(|_| "unknown".to_string());
    LockInfo {
        user,
        host: host_name(),
        pid: Some(std::process::id() as i32),
    }
}

/// This machine's host name (the second half of a lock's `user@host`).
fn host_name() -> String {
    #[cfg(unix)]
    {
        let mut buf = [0i8; 256];
        // SAFETY: `gethostname` writes at most `len` bytes into `buf`, which is
        // owned by this frame; the result is only read up to the NUL below.
        let ok = unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) } == 0;
        if ok {
            let bytes: Vec<u8> = buf
                .iter()
                .take_while(|&&c| c != 0)
                .map(|&c| c as u8)
                .collect();
            if !bytes.is_empty() {
                return String::from_utf8_lossy(&bytes).into_owned();
            }
        }
    }
    std::env::var("HOSTNAME").unwrap_or_else(|_| "localhost".to_string())
}

/// Whether the process holding a lock is still running. A lock left behind by a
/// dead process is stale, and emacs takes it over without asking.
fn pid_alive(pid: i32) -> bool {
    #[cfg(unix)]
    {
        // SAFETY: signal 0 performs the permission/existence check only and
        // sends nothing, so no process is affected by this call.
        if unsafe { libc::kill(pid, 0) } == 0 {
            return true;
        }
        // EPERM means the process exists but belongs to another user.
        std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        true
    }
}

/// Who holds `file`'s lock, in emacs `current_lock_owner` terms.
enum LockState {
    /// No lock file, or one left behind by a process that is gone.
    Free,
    /// We wrote it (emacs `I_OWN_IT`).
    Ours,
    /// Another editor holds it (emacs `ANOTHER`).
    Theirs(LockInfo),
}

/// Read a lock file. It is a symlink whose target is the owner; on filesystems
/// without symlinks emacs falls back to a regular file with the same contents.
fn read_lock(path: &std::path::Path) -> Option<LockInfo> {
    let target = std::fs::read_link(path)
        .map(|t| t.to_string_lossy().into_owned())
        .or_else(|_| std::fs::read_to_string(path))
        .ok()?;
    parse_lock_info(target.trim())
}

fn lock_state(file: &std::path::Path) -> LockState {
    let Some(path) = lock_path(file) else {
        return LockState::Free;
    };
    let Some(info) = read_lock(&path) else {
        return LockState::Free;
    };
    let me = self_info();
    if info == me {
        return LockState::Ours;
    }
    // Liveness is only knowable for locks taken on this machine; a lock from
    // another host always counts as held.
    match info.pid {
        Some(pid) if info.host == me.host && !pid_alive(pid) => LockState::Free,
        _ => LockState::Theirs(info),
    }
}

/// Write the lock file for `file`, replacing any stale or stolen one.
fn write_lock(file: &std::path::Path) -> bool {
    let Some(path) = lock_path(file) else {
        return false;
    };
    let target = lock_target(&self_info());
    let _ = std::fs::remove_file(&path);
    #[cfg(unix)]
    {
        if std::os::unix::fs::symlink(&target, &path).is_ok() {
            return true;
        }
    }
    std::fs::write(&path, target).is_ok()
}

/// emacs `file-locked-p`: the owner of `file`'s lock when another editor holds
/// it, else `None` (unlocked, stale, or locked by us).
pub fn locked_by(doc: &Document) -> Option<String> {
    match lock_state(doc.path()?) {
        LockState::Theirs(info) => Some(opponent(&info)),
        _ => None,
    }
}

/// The prompt text emacs would show, with userlock.el's truncation: the file
/// name is elided to its last 22 characters and the opponent to its first 13
/// plus the `(pid N)` tail. Pure — unit tested.
fn lock_prompt(file: &str, opponent: &str) -> String {
    let short_file = if file.chars().count() > 22 {
        let n = file.chars().count() - 22;
        format!("...{}", file.chars().skip(n).collect::<String>())
    } else {
        file.to_string()
    };
    let short_opponent = if opponent.chars().count() > 25 {
        let head: String = opponent.chars().take(13).collect();
        let pid = opponent
            .find(" (pid ")
            .map(|i| opponent[i..].to_string())
            .unwrap_or_default();
        format!("{head}...{pid}")
    } else {
        opponent.to_string()
    };
    format!("{short_file} locked by {short_opponent}: (s, q, p, ?)? ")
}

/// emacs `ask-user-about-lock` — what to do when the user modifies a file that
/// another editor has locked. emacs reads one of `s`/`q`/`p` from the user;
/// zmax cannot read a character from inside the change hook, so the answer
/// comes from the `ask-user-about-lock` setting, which is the extension point
/// emacs documents ("You can redefine this function to choose among those three
/// alternatives in any way you like"). The prompt emacs would have shown is
/// recorded for the command layer to report.
pub fn ask_user_about_lock(file: &std::path::Path, opponent: &str) -> LockDecision {
    let prompt = lock_prompt(&file.to_string_lossy(), opponent);
    LOCK_PROMPT.with(|p| *p.borrow_mut() = Some(prompt));
    lock_answer(
        crate::commands::typed::vim_opt_str("ask-user-about-lock")
            .as_deref()
            .map(str::trim),
    )
}

/// Take the most recent lock-conflict prompt, if one has not been reported yet.
pub fn take_lock_prompt() -> Option<String> {
    LOCK_PROMPT.with(|p| p.borrow_mut().take())
}

/// emacs `lock-file`: lock the document's file before its first modification,
/// asking about a lock another editor holds. Returns the decision that was
/// taken, or `None` when nothing had to be decided.
fn lock_file(doc: &Document) -> Option<LockDecision> {
    let path = doc.path()?.to_path_buf();
    if LOCKED.with(|l| l.borrow().contains(&doc.id())) {
        return None;
    }
    match lock_state(&path) {
        LockState::Ours => {
            LOCKED.with(|l| l.borrow_mut().insert(doc.id()));
            None
        }
        LockState::Free => {
            if write_lock(&path) {
                LOCKED.with(|l| l.borrow_mut().insert(doc.id()));
            }
            None
        }
        LockState::Theirs(info) => {
            let decision = ask_user_about_lock(&path, &opponent(&info));
            if decision == LockDecision::Steal && write_lock(&path) {
                LOCKED.with(|l| l.borrow_mut().insert(doc.id()));
            }
            Some(decision)
        }
    }
}

/// emacs `unlock-file`: drop the lock we hold on the document's file. Locks
/// belonging to another editor are left alone.
pub fn unlock_file(doc: &Document) {
    if !LOCKED.with(|l| l.borrow_mut().remove(&doc.id())) {
        return;
    }
    if let Some(path) = doc.path().and_then(lock_path) {
        // Only our own lock is removed — the other editor may have stolen it
        // in the meantime, and stealing it back on the way out would be wrong.
        if matches!(read_lock(&path), Some(info) if info == self_info()) {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Refresh swap files on edits, prime the cached config, and warn on recovery.
pub fn register_hooks() {
    use zmax_event::register_hook;
    use zmax_view::events::{
        ConfigDidChange, DocumentDidChange, DocumentDidClose, DocumentDidOpen,
    };

    register_hook!(move |event: &mut ConfigDidChange<'_>| {
        SWAPFILE_ON.store(event.new.swapfile, Ordering::Relaxed);
        if let Ok(mut d) = SWAP_DIR.lock() {
            *d = event.new.swap_directory.clone();
        }
        Ok(())
    });

    register_hook!(move |event: &mut DocumentDidChange<'_>| {
        // vim `updatecount`: how many changes go by between swap-file writes, and
        // `updatecount=0` means no swap file is produced at all.
        let updatecount = updatecount();
        if SWAPFILE_ON.load(Ordering::Relaxed) && updatecount != 0 && !no_swap(event.doc.id()) {
            let id = event.doc.id();
            let count = COUNTERS.with(|c| {
                let mut c = c.borrow_mut();
                let n = c.entry(id).or_insert(0);
                *n += 1;
                *n
            });
            if swap_write_due(count, updatecount) {
                write_swap(event.doc);
            }
        }
        // emacs interlocking: the lock is taken before the buffer's first
        // modification and dropped again as soon as it is unmodified.
        if create_lockfiles() {
            if event.doc.is_modified() {
                if lock_file(event.doc) == Some(LockDecision::Quit) {
                    // "don't modify this file" — emacs signals `file-locked`,
                    // aborting the edit. The change here has already landed, so
                    // the nearest equivalent is to stop the buffer from taking
                    // (or writing) any more.
                    event.doc.readonly = true;
                }
            } else {
                unlock_file(event.doc);
            }
        }
        Ok(())
    });

    // emacs drops the lock when the buffer is killed.
    register_hook!(move |event: &mut DocumentDidClose<'_>| {
        unlock_file(&event.doc);
        Ok(())
    });

    // emacs asks about a lock at the first modification; zmax cannot query the
    // user from inside the change hook, so a lock another editor already holds
    // is reported when the file is visited instead.
    register_hook!(move |event: &mut DocumentDidOpen<'_>| {
        if create_lockfiles() {
            if let Some(owner) = event.editor.document(event.doc).and_then(locked_by) {
                event.editor.set_error(format!("locked by {owner}"));
            }
        }
        Ok(())
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// vim `updatecount`: the swap file is rewritten every N changes, and
    /// `updatecount=0` turns swap-file writing off entirely.
    #[test]
    fn updatecount_controls_the_swap_write_cadence() {
        // The default: every 200th change.
        assert!(!swap_write_due(1, UPDATECOUNT_DEFAULT));
        assert!(!swap_write_due(199, UPDATECOUNT_DEFAULT));
        assert!(swap_write_due(200, UPDATECOUNT_DEFAULT));
        assert!(swap_write_due(400, UPDATECOUNT_DEFAULT));

        // `:set updatecount=10` writes ten times as often.
        assert!(swap_write_due(10, 10));
        assert!(swap_write_due(20, 10));
        assert!(!swap_write_due(11, 10));

        // `:set updatecount=0` never writes.
        for n in [0, 1, 200, 1000] {
            assert!(!swap_write_due(n, 0), "updatecount=0 must never write");
        }
    }

    /// vim `:noswapfile {cmd}`: the buffer that command opened keeps no swap file
    /// — including from the periodic writer, which is the only thing that would
    /// have created one after the command itself was over.
    #[test]
    fn noswapfile_buffers_never_get_a_swap_file() {
        let doc = DocumentId::default();
        assert!(!no_swap(doc), "an ordinary buffer does get a swap file");
        set_no_swap(doc);
        assert!(
            no_swap(doc),
            "`:noswapfile edit x` must keep the periodic writer off that buffer"
        );
        NO_SWAP.with(|s| s.borrow_mut().clear());
    }

    /// emacs writes `user@host.pid` into the lock file, optionally with a
    /// `:boot_time` suffix, and a host name may itself contain dots.
    #[test]
    fn lock_files_round_trip_their_owner() {
        let info = parse_lock_info("jane@build.example.com.4711").unwrap();
        assert_eq!(info.user, "jane");
        assert_eq!(info.host, "build.example.com");
        assert_eq!(info.pid, Some(4711));
        assert_eq!(lock_target(&info), "jane@build.example.com.4711");
        assert_eq!(opponent(&info), "jane@build.example.com (pid 4711)");

        // The boot time emacs appends is parsed off and ignored.
        assert_eq!(parse_lock_info("jane@host.7:1700000000").unwrap(), {
            LockInfo {
                user: "jane".into(),
                host: "host".into(),
                pid: Some(7),
            }
        });

        // A lock with no pid at all: emacs drops the `(pid N)` from the prompt.
        let no_pid = parse_lock_info("jane@host").unwrap();
        assert_eq!(no_pid.pid, None);
        assert_eq!(opponent(&no_pid), "jane@host");

        assert!(parse_lock_info("nonsense").is_none());
    }

    /// userlock.el elides the file name to its last 22 characters and the
    /// opponent to its first 13 plus the `(pid N)` tail.
    #[test]
    fn the_lock_prompt_truncates_like_userlock_el() {
        assert_eq!(
            lock_prompt("short.txt", "jane@host (pid 12)"),
            "short.txt locked by jane@host (pid 12): (s, q, p, ?)? "
        );

        let long = "/home/jane/src/project/deeply/nested/file.txt";
        let prompt = lock_prompt(long, "jane@host (pid 12)");
        assert!(
            prompt.starts_with("...deeply/nested/file.txt locked"),
            "a long file name keeps its last 22 characters: {prompt}"
        );

        let prompt = lock_prompt("f.txt", "verylonguser@build.example.com (pid 4711)");
        assert_eq!(
            prompt,
            "f.txt locked by verylonguser@... (pid 4711): (s, q, p, ?)? "
        );
    }

    /// The three answers `ask-user-about-lock` documents, and the `p` fallback
    /// for the answers emacs would have re-asked for.
    #[test]
    fn the_lock_answer_maps_to_the_three_alternatives() {
        assert_eq!(lock_answer(Some("s")), LockDecision::Steal);
        assert_eq!(lock_answer(Some("steal")), LockDecision::Steal);
        assert_eq!(lock_answer(Some("q")), LockDecision::Quit);
        assert_eq!(lock_answer(Some("quit")), LockDecision::Quit);
        assert_eq!(lock_answer(Some("p")), LockDecision::Proceed);
        assert_eq!(lock_answer(None), LockDecision::Proceed);
        assert_eq!(lock_answer(Some("x")), LockDecision::Proceed);
    }

    /// The lock file lives beside the file it locks, named `.#<name>`.
    #[test]
    fn the_lock_file_sits_beside_its_file() {
        assert_eq!(
            lock_path(std::path::Path::new("/tmp/notes.txt")).unwrap(),
            PathBuf::from("/tmp/.#notes.txt")
        );
        assert_eq!(
            lock_path(std::path::Path::new("notes.txt")).unwrap(),
            PathBuf::from(".#notes.txt")
        );
    }

    /// emacs `create-lockfiles` defaults to `t`; only turning it off stops the
    /// lock from being taken.
    #[test]
    fn create_lockfiles_defaults_on() {
        assert!(create_lockfiles(), "an unset `create-lockfiles` means on");
        crate::commands::typed::vim_opt_store("create-lockfiles", "off".to_string());
        assert!(!create_lockfiles());
        crate::commands::typed::vim_opt_store("create-lockfiles", String::new());
        assert!(create_lockfiles());
    }

    /// A live lock is another editor's; one from a process that is gone is
    /// stale, and emacs takes it over without asking.
    #[test]
    fn a_dead_owner_leaves_the_file_free() {
        let dir = std::env::temp_dir().join(format!("zmax-lock-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("notes.txt");
        std::fs::write(&file, "x").unwrap();
        let lock = lock_path(&file).unwrap();

        assert!(matches!(lock_state(&file), LockState::Free), "no lock yet");

        // Our own lock is not a conflict.
        assert!(write_lock(&file));
        assert!(matches!(lock_state(&file), LockState::Ours));

        // A lock from a pid that cannot be running on this host is stale.
        let host = host_name();
        std::fs::remove_file(&lock).unwrap();
        std::os::unix::fs::symlink(format!("jane@{host}.2147483645"), &lock).unwrap();
        assert!(
            matches!(lock_state(&file), LockState::Free),
            "a lock left by a dead process is stale"
        );

        // A lock from another host is always held — liveness is unknowable.
        std::fs::remove_file(&lock).unwrap();
        std::os::unix::fs::symlink("jane@elsewhere.2147483645", &lock).unwrap();
        assert!(matches!(lock_state(&file), LockState::Theirs(_)));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// `:set updatecount=N` is what the change hook reads; unset keeps vim's 200.
    #[test]
    fn updatecount_reads_the_option_store() {
        assert_eq!(updatecount(), UPDATECOUNT_DEFAULT);
        crate::commands::typed::vim_opt_store("updatecount", "50".to_string());
        assert_eq!(updatecount(), 50);
        crate::commands::typed::vim_opt_store("updatecount", "0".to_string());
        assert_eq!(updatecount(), 0);
        crate::commands::typed::vim_opt_store("updatecount", String::new());
        assert_eq!(updatecount(), UPDATECOUNT_DEFAULT);
    }
}
