//! The state behind a handful of Emacs commands that have no larger home of
//! their own: recursive editing levels, the keyboard-macro query, per-connection
//! local variables, the termscript, the grep command abbreviation, `command-query`
//! and the abbrev-suggestion log.
//!
//! Everything here is process-global data plus the pure functions that decide
//! what it means; the editor-facing commands live in `commands.rs` and reach this
//! module for the state they keep between invocations.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use zmax_view::input::KeyEvent;

// ── Recursive editing levels (Emacs `Recursive Edit`) ───────────────────────

/// Why a recursive editing level was entered. Exiting one with `C-M-c` resumes
/// whatever asked for it, so the level has to remember which that was.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RecursiveReason {
    /// `M-x recursive-edit`: the user asked for a level and nothing is waiting
    /// on it beyond the command that started it.
    Interactive,
    /// `C-r` at a `kbd-macro-query` prompt: leaving the level asks the query
    /// again, which is what the manual promises ("you are asked again how to
    /// continue with the keyboard macro").
    MacroQuery,
}

fn levels() -> &'static Mutex<Vec<RecursiveReason>> {
    static L: Mutex<Vec<RecursiveReason>> = Mutex::new(Vec::new());
    &L
}

/// Enter a recursive editing level. Levels nest, exactly as Emacs's do.
pub fn recursive_enter(reason: RecursiveReason) -> usize {
    let mut l = levels().lock().unwrap();
    l.push(reason);
    l.len()
}

/// How many recursive editing levels are in progress (0 = top level). The mode
/// line shows one pair of square brackets per level.
pub fn recursive_depth() -> usize {
    levels().lock().unwrap().len()
}

/// `exit-recursive-edit` (`C-M-c`): leave the *innermost* level and report why
/// it had been entered. `None` when there is no recursive edit to leave.
pub fn recursive_exit() -> Option<RecursiveReason> {
    levels().lock().unwrap().pop()
}

/// `M-x top-level`: abandon every level at once. Returns how many were left.
pub fn recursive_clear() -> usize {
    let mut l = levels().lock().unwrap();
    let n = l.len();
    l.clear();
    n
}

// ── `kbd-macro-query` (`C-x q`) ────────────────────────────────────────────

/// A keyboard macro stopped mid-run by `C-x q`, holding everything needed to
/// pick it up again: the keys left in the repetition that was interrupted, the
/// whole macro (for the repetitions after it) and how many of those are left.
#[derive(Clone, Debug)]
pub struct MacroSuspension {
    /// Keys still to run in the repetition `C-x q` interrupted.
    pub rest: Vec<KeyEvent>,
    /// The macro itself, for each remaining repetition.
    pub all: Vec<KeyEvent>,
    /// Repetitions still to run after the interrupted one.
    pub reps_left: usize,
    /// The register the macro is being replayed from.
    pub register: char,
}

static QUERY_RAISED: AtomicBool = AtomicBool::new(false);

fn suspension() -> &'static Mutex<Option<MacroSuspension>> {
    static S: Mutex<Option<MacroSuspension>> = Mutex::new(None);
    &S
}

/// `kbd-macro-query` ran: the replay loop must stop where it is and let the user
/// answer.
pub fn macro_query_raise() {
    QUERY_RAISED.store(true, Ordering::SeqCst);
}

/// Whether a query is waiting to be answered — read by the replay loop after
/// every key it feeds.
pub fn macro_query_raised() -> bool {
    QUERY_RAISED.load(Ordering::SeqCst)
}

/// Park the rest of the run until the query is answered.
pub fn macro_suspend(state: MacroSuspension) {
    *suspension().lock().unwrap() = Some(state);
}

/// Take the parked run back, clearing the query flag with it.
pub fn macro_resume_state() -> Option<MacroSuspension> {
    QUERY_RAISED.store(false, Ordering::SeqCst);
    suspension().lock().unwrap().take()
}

/// Whether a run is parked (so `C-M-c` out of a `C-r` level knows to ask again).
pub fn macro_suspended() -> bool {
    suspension().lock().unwrap().is_some()
}

/// Put a parked run back without consuming it — `C-r` enters a recursive edit
/// and the query is asked again when that level is left.
pub fn macro_repark(state: MacroSuspension) {
    *suspension().lock().unwrap() = Some(state);
}

// ── Connection-local variables (Emacs `Connection Variables`) ───────────────

/// A criteria that identifies a connection: any of the parts may be `None`,
/// which matches everything (Emacs's `nil` criteria matches every remote
/// directory).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ConnectionCriteria {
    /// `:protocol` — the access method (`ssh`, `sudo`, …).
    pub protocol: Option<String>,
    /// `:user` — the remote user name.
    pub user: Option<String>,
    /// `:machine` — a regexp matched against the host name.
    pub machine: Option<String>,
}

impl ConnectionCriteria {
    /// Parse `:machine host :protocol ssh` into a criteria. Unknown keywords are
    /// ignored, so a criteria that names nothing matches every connection.
    pub fn parse(spec: &str) -> Self {
        let mut c = ConnectionCriteria::default();
        let mut words = spec.split_whitespace();
        while let Some(key) = words.next() {
            let Some(value) = words.next() else { break };
            match key.trim_start_matches(':') {
                "protocol" => c.protocol = Some(value.to_string()),
                "user" => c.user = Some(value.to_string()),
                "machine" | "host" => c.machine = Some(value.to_string()),
                _ => {}
            }
        }
        c
    }

    /// Whether this criteria selects `conn`. Each part that is set must be equal
    /// to the connection's (the machine part is a substring match, standing in
    /// for Emacs's host regexp); parts that are unset match anything.
    pub fn matches(&self, conn: &Connection) -> bool {
        let eq = |want: &Option<String>, have: &str| match want {
            Some(w) => w == have,
            None => true,
        };
        eq(&self.protocol, &conn.protocol)
            && eq(&self.user, &conn.user)
            && match &self.machine {
                Some(m) => conn.machine.contains(m.as_str()),
                None => true,
            }
    }
}

/// The remote connection a buffer's directory belongs to, in the pieces a
/// criteria discriminates on.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Connection {
    pub protocol: String,
    pub user: String,
    pub machine: String,
}

impl Connection {
    /// Read a connection out of a Tramp-style file name — `/ssh:user@host:/path`
    /// (the user is optional: `/ssh:host:/path`). `None` for a local path, which
    /// is what makes connection-local variables apply to remote buffers only.
    pub fn from_path(path: &str) -> Option<Self> {
        let rest = path.strip_prefix('/')?;
        let (protocol, rest) = rest.split_once(':')?;
        if protocol.is_empty() || protocol.contains('/') {
            return None;
        }
        let (hostpart, _) = rest.split_once(':')?;
        let (user, machine) = match hostpart.split_once('@') {
            Some((u, h)) => (u.to_string(), h.to_string()),
            None => (String::new(), hostpart.to_string()),
        };
        if machine.is_empty() {
            return None;
        }
        Some(Connection {
            protocol: protocol.to_string(),
            user,
            machine,
        })
    }
}

type Profiles = HashMap<String, Vec<(String, String)>>;

fn profiles() -> &'static Mutex<Profiles> {
    static P: std::sync::OnceLock<Mutex<Profiles>> = std::sync::OnceLock::new();
    P.get_or_init(|| Mutex::new(Profiles::new()))
}

#[allow(clippy::type_complexity)]
fn applied() -> &'static Mutex<Vec<(ConnectionCriteria, Vec<String>)>> {
    static A: Mutex<Vec<(ConnectionCriteria, Vec<String>)>> = Mutex::new(Vec::new());
    &A
}

/// `connection-local-set-profile-variables`: declare a profile as a group of
/// variable/value pairs. Redeclaring a profile replaces it, as in Emacs.
pub fn set_profile_variables(profile: &str, vars: Vec<(String, String)>) {
    profiles().lock().unwrap().insert(profile.to_string(), vars);
}

/// The variables of one profile, or `None` when no such profile is declared.
pub fn profile_variables(profile: &str) -> Option<Vec<(String, String)>> {
    profiles().lock().unwrap().get(profile).cloned()
}

/// Every declared profile name, sorted.
pub fn profile_names() -> Vec<String> {
    let mut names: Vec<String> = profiles().lock().unwrap().keys().cloned().collect();
    names.sort();
    names
}

/// `connection-local-set-profiles`: activate `names` for every connection the
/// criteria matches. A criteria may be given more than once; the activations
/// accumulate in declaration order.
pub fn set_profiles(criteria: ConnectionCriteria, names: Vec<String>) {
    applied().lock().unwrap().push((criteria, names));
}

/// The variable/value pairs in force for `conn`, in the order the profiles were
/// activated (a later profile overrides an earlier one for the same variable).
pub fn variables_for(conn: &Connection) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let table = profiles().lock().unwrap();
    for (criteria, names) in applied().lock().unwrap().iter() {
        if !criteria.matches(conn) {
            continue;
        }
        for name in names {
            let Some(vars) = table.get(name) else { continue };
            for (var, value) in vars {
                match out.iter_mut().find(|(v, _)| v == var) {
                    Some(slot) => slot.1 = value.clone(),
                    None => out.push((var.clone(), value.clone())),
                }
            }
        }
    }
    out
}

/// The value one connection-local variable takes for `conn`, if any.
pub fn variable_for(conn: &Connection, name: &str) -> Option<String> {
    variables_for(conn)
        .into_iter()
        .find(|(v, _)| v == name)
        .map(|(_, value)| value)
}

// ── `open-termscript` ──────────────────────────────────────────────────────

fn termscript() -> &'static Mutex<Option<PathBuf>> {
    static T: Mutex<Option<PathBuf>> = Mutex::new(None);
    &T
}

/// `open-termscript FILE`: from now on every screen the editor paints is
/// appended to `FILE`. `None` closes the script again.
pub fn set_termscript(path: Option<PathBuf>) {
    *termscript().lock().unwrap() = path;
}

/// The file the termscript is being written to, if one is open.
pub fn termscript_path() -> Option<PathBuf> {
    termscript().lock().unwrap().clone()
}

/// Append one painted screen to the open termscript. Silently does nothing when
/// no script is open — this runs on every redraw.
pub fn termscript_write(screen: &str) {
    let Some(path) = termscript_path() else { return };
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().append(true).create(true).open(path) {
        let _ = f.write_all(screen.as_bytes());
    }
}

// ── `grep-find-toggle-abbreviation` ────────────────────────────────────────

/// `grep-find-abbreviate`, default on: the long "skip these directories" part of
/// a constructed grep command is shown as an ellipsis.
static GREP_ABBREVIATE: AtomicBool = AtomicBool::new(true);

/// Flip the abbreviation and report the new state.
pub fn grep_toggle_abbreviation() -> bool {
    !GREP_ABBREVIATE.fetch_xor(true, Ordering::Relaxed)
}

/// Whether constructed grep commands are shown abbreviated.
pub fn grep_abbreviated() -> bool {
    GREP_ABBREVIATE.load(Ordering::Relaxed)
}

fn last_grep() -> &'static Mutex<Option<(String, String)>> {
    static G: Mutex<Option<(String, String)>> = Mutex::new(None);
    &G
}

/// Remember the last constructed grep command as `(head, ignored)` — the part
/// that is always shown and the ignore list the abbreviation conceals.
pub fn set_last_grep(head: String, ignored: String) {
    *last_grep().lock().unwrap() = Some((head, ignored));
}

/// The last grep command, rendered for display under the current abbreviation
/// setting: the ignore list is replaced by `…` while it is on.
pub fn last_grep_display() -> Option<String> {
    let g = last_grep().lock().unwrap();
    let (head, ignored) = g.as_ref()?;
    Some(if ignored.is_empty() {
        head.clone()
    } else if grep_abbreviated() {
        format!("{head} …")
    } else {
        format!("{head} {ignored}")
    })
}

// ── `command-query` (Emacs `Disabling`) ────────────────────────────────────

type Queries = HashMap<String, (String, bool)>;

fn queries() -> &'static Mutex<Queries> {
    static Q: std::sync::OnceLock<Mutex<Queries>> = std::sync::OnceLock::new();
    Q.get_or_init(|| Mutex::new(Queries::new()))
}

/// `command-query COMMAND PROMPT &optional YES-NO`: ask before running COMMAND.
/// An empty prompt takes the query off the command again.
pub fn set_command_query(command: &str, prompt: &str, yes_no: bool) {
    let mut q = queries().lock().unwrap();
    if prompt.is_empty() {
        q.remove(command);
    } else {
        q.insert(command.to_string(), (prompt.to_string(), yes_no));
    }
}

/// The prompt registered for `command`, and whether it wants `yes`/`no` rather
/// than `y`/`n`.
pub fn command_query(command: &str) -> Option<(String, bool)> {
    queries().lock().unwrap().get(command).cloned()
}

/// Every queried command, as `(command, prompt)` sorted by command name.
pub fn command_queries() -> Vec<(String, String)> {
    let q = queries().lock().unwrap();
    let mut out: Vec<(String, String)> = q
        .iter()
        .map(|(name, (prompt, _))| (name.clone(), prompt.clone()))
        .collect();
    out.sort();
    out
}

/// The command whose query has just been answered `yes`, so the re-run of it
/// does not ask again.
fn answered() -> &'static Mutex<Option<String>> {
    static A: Mutex<Option<String>> = Mutex::new(None);
    &A
}

/// Let `command` through the query check exactly once.
pub fn allow_once(command: &str) {
    *answered().lock().unwrap() = Some(command.to_string());
}

/// Whether `command` was let through; consumes the permission.
pub fn take_allowance(command: &str) -> bool {
    let mut a = answered().lock().unwrap();
    if a.as_deref() == Some(command) {
        *a = None;
        true
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recursive_levels_nest_and_unwind_innermost_first() {
        recursive_clear();
        assert_eq!(recursive_depth(), 0);
        recursive_enter(RecursiveReason::Interactive);
        recursive_enter(RecursiveReason::MacroQuery);
        assert_eq!(recursive_depth(), 2);
        assert_eq!(recursive_exit(), Some(RecursiveReason::MacroQuery));
        assert_eq!(recursive_exit(), Some(RecursiveReason::Interactive));
        assert_eq!(recursive_exit(), None);
        assert_eq!(recursive_depth(), 0);
    }

    #[test]
    fn connection_reads_a_tramp_file_name() {
        assert_eq!(
            Connection::from_path("/ssh:jane@build.example:/etc/hosts"),
            Some(Connection {
                protocol: "ssh".into(),
                user: "jane".into(),
                machine: "build.example".into(),
            })
        );
        assert_eq!(
            Connection::from_path("/sudo:root:/etc/hosts").map(|c| c.machine),
            Some("root".to_string())
        );
        // A local path is not a connection.
        assert_eq!(Connection::from_path("/etc/hosts"), None);
        assert_eq!(Connection::from_path("relative/path"), None);
    }

    #[test]
    fn criteria_matches_by_part_and_nil_matches_everything() {
        let conn = Connection {
            protocol: "ssh".into(),
            user: "jane".into(),
            machine: "remotemachine".into(),
        };
        assert!(ConnectionCriteria::parse("").matches(&conn));
        assert!(ConnectionCriteria::parse(":machine remotemachine").matches(&conn));
        assert!(ConnectionCriteria::parse(":protocol ssh :user jane").matches(&conn));
        assert!(!ConnectionCriteria::parse(":user root").matches(&conn));
    }

    #[test]
    fn profiles_apply_in_activation_order() {
        // The manual's own example: two profiles on one criteria.
        set_profile_variables(
            "remote-terminfo",
            vec![("system-uses-terminfo".into(), "t".into())],
        );
        set_profile_variables("remote-ksh", vec![("shell-file-name".into(), "/bin/ksh".into())]);
        set_profile_variables(
            "remote-bash",
            vec![("shell-file-name".into(), "/bin/bash".into())],
        );
        set_profiles(
            ConnectionCriteria::parse(":machine remotemachine"),
            vec!["remote-terminfo".into(), "remote-ksh".into()],
        );
        let conn = Connection {
            protocol: "ssh".into(),
            user: String::new(),
            machine: "remotemachine".into(),
        };
        assert_eq!(
            variable_for(&conn, "shell-file-name"),
            Some("/bin/ksh".to_string())
        );
        assert_eq!(
            variable_for(&conn, "system-uses-terminfo"),
            Some("t".to_string())
        );
        // A connection the criteria does not name gets nothing.
        let other = Connection {
            protocol: "ssh".into(),
            user: String::new(),
            machine: "elsewhere".into(),
        };
        assert_eq!(variable_for(&other, "shell-file-name"), None);
    }

    #[test]
    fn command_query_registers_and_clears() {
        set_command_query("end_of_buffer", "Really go to the end?", false);
        assert_eq!(
            command_query("end_of_buffer"),
            Some(("Really go to the end?".to_string(), false))
        );
        assert!(!take_allowance("end_of_buffer"));
        allow_once("end_of_buffer");
        assert!(take_allowance("end_of_buffer"));
        assert!(!take_allowance("end_of_buffer"));
        set_command_query("end_of_buffer", "", false);
        assert_eq!(command_query("end_of_buffer"), None);
    }
}
