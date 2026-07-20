//! Comint — a line-oriented interactive-subprocess buffer, the zmax port of
//! GNU Emacs `comint-mode` / `M-x shell`.
//!
//! Unlike the PTY-backed [`super::terminal`] (a full vt100 screen emulator, the
//! `term`/`ansi-term` analogue), comint is the *dumb-terminal* REPL model: a
//! scrollback of output lines plus a single editable input line at the bottom.
//! You type a command, press Enter, it is written to the child's stdin and its
//! stdout/stderr stream back into the scrollback. This is the mode inferior-lisp,
//! the Python shell and `gud` derive from in Emacs.
//!
//! The child (`$SHELL` by default) is spawned with piped stdio; two reader
//! threads append its stdout/stderr lines to shared state and request a redraw.
//! The pure input-ring history (`comint-input-ring`) lives in the filesystem-free
//! [`zmax_core::comint`].
//!
//! Keys — the real `comint-mode-map` plus `shell-mode-map` (checked against
//! Emacs 30's `C-h b` dump):
//!   Enter — comint-send-input (run the input line)
//!   Up / C-p — comint-previous-input; Down / C-n — comint-next-input
//!   C-a / Home — comint-bol-or-process-mark; C-e / End — end of input
//!   C-k — kill to end of line; M-. — comint-insert-previous-argument (!$)
//!   Space — comint-magic-space (expand !! / !$ history, then space)
//!   C-d — comint-delchar-or-maybe-eof (delete char, or EOF on empty line)
//!   TAB — completion-at-point (complete the file name before point)
//!   M-? — comint-dynamic-list-filename-completions
//!   M-r — comint-history-isearch-backward-regexp (reads the pattern, then yanks
//!         the newest older input containing it onto the input line)
//!   C-M-l — comint-show-output
//!   C-c is a prefix (comint job control), then:
//!     C-c — comint-interrupt-subjob (SIGINT)   C-z — comint-stop-subjob (SIGTSTP)
//!     C-u — comint-kill-input                  C-\\ — comint-quit-subjob (SIGQUIT)
//!     C-n / C-p — comint-next/previous-prompt   C-r — comint-show-output
//!     C-e — comint-show-maximum-output          C-o — comint-delete-output
//!     C-a — comint-bol-or-process-mark          C-d — comint-send-eof
//!     C-b / C-f — shell-backward/forward-command (one `;`/`|`/`&` command)
//!     C-l — comint-dynamic-list-input-ring      C-s — comint-write-output (to a file)
//!     C-w — backward-kill-word                  C-x — comint-get-next-from-history
//!     `.` — comint-insert-previous-argument     RET — comint-copy-old-input
//!   PageUp / PageDown — scroll the scrollback
//!   F12 — detach the comint panel (the child is killed on drop)
//!
//! Directory tracking (`shell.el` / `dirtrack.el`) lives in [`DirTrack`]: the
//! buffer's idea of the subshell's working directory plus its `pushd` stack,
//! kept in step either by watching the input for `cd`/`pushd`/`popd`
//! (`shell-dirtrack-mode`, on by default), by matching the shell's prompt in the
//! output (`dirtrack-mode`), or on demand by asking the shell (`dirs`).

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tui::buffer::Buffer as Surface;
use zmax_core::comint::InputRing;
use zmax_view::graphics::{CursorKind, Rect};
use zmax_view::input::KeyCode;

use crate::{
    alt,
    compositor::{Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

/// Cap on retained scrollback lines (matches `super::run::MAX_LINES` intent).
const MAX_LINES: usize = 5000;

/// Output scrollback shared between the reader threads and the render loop.
/// `is_prompt[i]` marks line `i` as an echoed prompt/input line (written by
/// [`Comint::submit`]) rather than subprocess output — the marker Emacs keeps via
/// the `field`/`comint-prompt` text properties, used by prompt/output navigation.
#[derive(Default)]
struct Scrollback {
    lines: Vec<String>,
    is_prompt: Vec<bool>,
}

impl Scrollback {
    fn push(&mut self, line: String) {
        self.push_kind(line, false);
    }

    fn push_kind(&mut self, line: String, is_prompt: bool) {
        self.lines.push(line);
        self.is_prompt.push(is_prompt);
        if self.lines.len() > MAX_LINES {
            let drop = self.lines.len() - MAX_LINES;
            self.lines.drain(0..drop);
            self.is_prompt.drain(0..drop);
        }
    }
}

/// `shell-mode`'s directory tracking: where the subshell is (Emacs's buffer
/// `default-directory`), its `pushd` stack (`shell-dirstack`, newest first and
/// *not* holding the current directory) and the toggles that steer both
/// trackers.
///
/// Shared with the reader threads because the two tracking methods read
/// opposite ends of the process: `shell-dirtrack-mode` watches the INPUT for
/// `cd`/`pushd`/`popd`, while `dirtrack-mode` is a pre-output filter that reads
/// the directory out of the shell's PROMPT.
struct DirTrack {
    /// `default-directory` — where file names in this buffer resolve from.
    cwd: PathBuf,
    /// `shell-dirstack`, newest entry first.
    stack: Vec<PathBuf>,
    /// `shell-last-dir` — the directory `cd -` returns to.
    last_dir: PathBuf,
    /// `shell-dirtrack-mode`: parse each submitted line for `cd`/`pushd`/`popd`.
    /// On by default, as `shell-mode` enables it.
    input_track: bool,
    /// `dirtrack-mode`: take the directory from the prompt in the output.
    dirtrack: bool,
    /// `dirtrack-list` — the prompt regexp and the group holding the directory.
    /// The default is Emacs's own (an example prompt, meant to be customised).
    dirtrack_re: Option<regex::Regex>,
    dirtrack_group: usize,
    /// Armed by `dirs`: the next non-empty output line is the shell's reply to
    /// the dirstack query and rebuilds `cwd`/`stack` instead of being scrollback.
    resync: bool,
    /// `shell-dirtrack-verbose` — echo the stack after every change.
    verbose: bool,
    /// `shell-pushd-tohome`: bare `pushd` behaves as `pushd ~` (tcsh's option).
    pushd_tohome: bool,
    /// `shell-pushd-dextract`: `pushd +n` pops the n-th entry to the stack top
    /// rather than rotating the stack around it (tcsh's option).
    pushd_dextract: bool,
    /// `shell-pushd-dunique`: `pushd` only adds directories not already stacked.
    pushd_dunique: bool,
}

/// Emacs's `dirtrack-list` default: the prompt regexp and its directory group.
const DIRTRACK_LIST: (&str, usize) = ("^emacs ([a-zA-Z]:.*)>", 1);

/// `shell-dirstack-query` — what `dirs` asks the shell.
const DIRSTACK_QUERY: &str = "dirs";

impl DirTrack {
    fn new() -> Self {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
        Self {
            last_dir: cwd.clone(),
            cwd,
            stack: Vec::new(),
            input_track: true,
            dirtrack: false,
            dirtrack_re: regex::Regex::new(DIRTRACK_LIST.0).ok(),
            dirtrack_group: DIRTRACK_LIST.1,
            resync: false,
            verbose: true,
            pushd_tohome: false,
            pushd_dextract: false,
            pushd_dunique: false,
        }
    }

    /// `shell-cd` — move to `dir`. Emacs's `cd` *signals* on a directory that is
    /// not there, and the tracker runs inside `ignore-errors`, so a failure here
    /// abandons the rest of the input line; that is what the `bool` reports.
    fn cd(&mut self, dir: PathBuf) -> bool {
        if !dir.is_dir() {
            return false;
        }
        self.cwd = dir;
        true
    }

    /// `shell-process-cd` — `cd [dir]`; no argument goes home, `-` goes back to
    /// `shell-last-dir`, which is set even when the `cd` itself fails.
    fn process_cd(&mut self, arg: &str) -> bool {
        let new_dir = match arg {
            "" => expand_dir(&self.cwd, "~"),
            "-" => self.last_dir.clone(),
            dir => expand_dir(&self.cwd, dir),
        };
        self.last_dir = self.cwd.clone();
        self.cd(new_dir)
    }

    /// `shell-process-pushd` — `pushd [+n | dir]`.
    fn process_pushd(&mut self, arg: &str) -> bool {
        if arg.is_empty() {
            // No arg: swap the current directory with the top of the stack —
            // unless `shell-pushd-tohome`, which makes it `pushd ~` instead.
            if self.pushd_tohome {
                return self.process_pushd("~");
            }
            let Some(top) = self.stack.first().cloned() else {
                return false; // "Directory stack empty."
            };
            let old = self.cwd.clone();
            if !self.cd(top) {
                return false;
            }
            self.stack[0] = old;
            return true;
        }
        if let Some(num) = extract_num(arg) {
            // `pushd +n`. `extract_num` only matches `+1`, `+2`, … so Emacs's
            // `(= num 0)` "Couldn't cd" arm is unreachable here.
            if num > self.stack.len() {
                return false; // "Directory stack not that deep."
            }
            if self.pushd_dextract {
                // Extract the n-th entry to the top: pop it out of the stack,
                // push the directory we are leaving in its place, then go there.
                let dir = self.stack[num - 1].clone();
                self.process_popd(arg);
                let cwd = self.cwd.clone();
                self.process_pushd(&cwd.to_string_lossy());
                return self.cd(dir);
            }
            // Otherwise rotate the whole stack (current directory included) so
            // that the n-th entry becomes the current one.
            let mut ds = vec![self.cwd.clone()];
            ds.extend(self.stack.iter().cloned());
            let new_ds: Vec<PathBuf> = ds[num..].iter().chain(ds[..num].iter()).cloned().collect();
            if !self.cd(new_ds[0].clone()) {
                return false;
            }
            self.stack = new_ds[1..].to_vec();
            return true;
        }
        // `pushd <dir>`: go there and stack the directory we came from —
        // `shell-pushd-dunique` suppressing the push when it is stacked already.
        let old = self.cwd.clone();
        if !self.cd(expand_dir(&self.cwd, arg)) {
            return false;
        }
        if !self.pushd_dunique || !self.stack.contains(&old) {
            self.stack.insert(0, old);
        }
        true
    }

    /// `shell-process-popd` — `popd [+n]`; bare `popd` goes to the top entry and
    /// drops it, `popd +n` only drops the n-th without moving.
    fn process_popd(&mut self, arg: &str) -> bool {
        let num = extract_num(arg).unwrap_or(0);
        if num == 0 {
            let Some(top) = self.stack.first().cloned() else {
                return false; // "Couldn't popd"
            };
            if !self.cd(top) {
                return false;
            }
            self.stack.remove(0);
            true
        } else if num <= self.stack.len() {
            self.stack.remove(num - 1);
            true
        } else {
            false
        }
    }

    /// `shell-directory-tracker` — the input side of the tracking. Every
    /// `;`/`&`/`|`-separated command on the submitted line is inspected, and a
    /// leading `cd`/`pushd`/`popd` is replayed against our own stack. Emacs
    /// wraps the whole walk in `ignore-errors`, so the first command that will
    /// not work ends the walk.
    fn directory_tracker(&mut self, input: &str) {
        if !self.input_track {
            return;
        }
        for command in input.split(['\n', ';', '&', '|']) {
            let mut words = command.split_whitespace();
            let Some(head) = words.next() else {
                continue;
            };
            let arg = words
                .next()
                .map(|a| substitute_in_file_name(&unquote_argument(a)))
                .unwrap_or_default();
            let ok = match head {
                "popd" => self.process_popd(&arg),
                "pushd" => self.process_pushd(&arg),
                "cd" => self.process_cd(&arg),
                _ => continue,
            };
            if !ok {
                return;
            }
        }
    }

    /// `dirtrack` — the output side: the pre-output filter `dirtrack-mode`
    /// installs. When `line` matches `dirtrack-list`, the captured group is the
    /// directory the shell claims to be in, and we `cd` there if it differs.
    fn dirtrack_filter(&mut self, line: &str) {
        let Some(prompt_path) = self
            .dirtrack_re
            .as_ref()
            .and_then(|re| re.captures(line))
            .and_then(|c| c.get(self.dirtrack_group))
            .map(|m| m.as_str().to_string())
        else {
            return;
        };
        if prompt_path.is_empty() {
            return;
        }
        if Path::new(&prompt_path).is_absolute() {
            let target = normalize(Path::new(&prompt_path));
            // Emacs will not chase a prompt into a directory it cannot reach
            // (rlogin buffers show the remote side's path).
            if target != self.cwd && target.is_dir() {
                self.process_cd(&prompt_path);
            }
        } else if let Some(full) = dirtrack_relative(&prompt_path, &self.cwd.to_string_lossy()) {
            // A relative prompt: see whether it names a place up or down from
            // where we already are.
            self.process_cd(&full);
        }
    }

    /// `shell-resync-dirs`, second half — rebuild `cwd`/`stack` from the shell's
    /// own reply to the dirstack query.
    fn apply_dirs_reply(&mut self, line: &str) {
        let ds = parse_dirs_reply(line);
        let Some((first, rest)) = ds.split_first() else {
            return;
        };
        if !self.cd(first.clone()) {
            return;
        }
        self.stack = rest.to_vec();
        self.last_dir = self
            .stack
            .first()
            .cloned()
            .unwrap_or_else(|| self.cwd.clone());
    }

    /// `shell-dirstack-message` — the current directory followed by the stack,
    /// with `$HOME` shortened to `~`. `None` unless `shell-dirtrack-verbose`.
    fn dirstack_message(&self) -> Option<String> {
        if !self.verbose {
            return None;
        }
        let mut out = abbreviate_home(&self.cwd);
        for dir in &self.stack {
            out.push(' ');
            out.push_str(&abbreviate_home(dir));
        }
        Some(out)
    }
}

/// `shell-extract-num` — the `n` of a `+n` (n > 0) stack argument.
fn extract_num(arg: &str) -> Option<usize> {
    let digits = arg.strip_prefix('+')?;
    if digits.starts_with('0') || digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// `shell-unquote-argument` — drop the shell quoting from one argument.
fn unquote_argument(arg: &str) -> String {
    let mut out = String::new();
    let mut inside: Option<char> = None;
    let mut chars = arg.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' if inside.is_none() => out.extend(chars.next()),
            '\'' | '"' | '`' if inside == Some(c) => inside = None,
            '\'' | '"' | '`' if inside.is_none() => inside = Some(c),
            _ => out.push(c),
        }
    }
    out
}

/// `comint-substitute-in-file-name` — expand `$VAR` / `${VAR}` in a file name
/// (the `~` half is left to [`expand_dir`], which needs the cwd anyway).
fn substitute_in_file_name(arg: &str) -> String {
    let mut out = String::new();
    let mut rest = arg;
    while let Some(at) = rest.find('$') {
        out.push_str(&rest[..at]);
        let tail = &rest[at + 1..];
        let (name, after) = match tail.strip_prefix('{') {
            Some(braced) => match braced.find('}') {
                Some(end) => (&braced[..end], &braced[end + 1..]),
                None => (braced, ""),
            },
            None => {
                let end = tail
                    .find(|c: char| !c.is_alphanumeric() && c != '_')
                    .unwrap_or(tail.len());
                (&tail[..end], &tail[end..])
            }
        };
        if name.is_empty() {
            out.push('$');
        } else {
            out.push_str(&std::env::var(name).unwrap_or_default());
        }
        rest = after;
    }
    out.push_str(rest);
    out
}

/// Emacs's `expand-file-name` for the tracker: `~` is the user's home, a
/// relative name hangs off `cwd`, and `.`/`..` are resolved lexically.
fn expand_dir(cwd: &Path, arg: &str) -> PathBuf {
    let expanded = zmax_stdx::path::expand_tilde(Path::new(arg)).into_owned();
    let joined = if expanded.is_absolute() {
        expanded
    } else {
        cwd.join(expanded)
    };
    normalize(&joined)
}

/// Where the directory part of a file-name fragment points: `~` is the shell's
/// home and a relative name (the empty one included) hangs off the buffer's
/// tracked working directory, so completion follows the subshell's `cd`.
fn resolve_dir(cwd: &Path, dir: &str) -> PathBuf {
    if dir.is_empty() {
        return cwd.to_path_buf();
    }
    expand_dir(cwd, dir)
}

/// Resolve `.` and `..` without touching the filesystem (`expand-file-name` is
/// lexical too — it does not follow symlinks).
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// The `~/…` form of `dir`, used by the dirstack message.
fn abbreviate_home(dir: &Path) -> String {
    match std::env::var_os("HOME")
        .map(PathBuf::from)
        .and_then(|home| dir.strip_prefix(home).ok().map(Path::to_path_buf))
    {
        Some(rest) if rest.as_os_str().is_empty() => "~".to_string(),
        Some(rest) => format!("~/{}", rest.display()),
        None => dir.display().to_string(),
    }
}

/// The relative-prompt arm of `dirtrack`: Emacs matches `prompt_path` against
/// the current directory with a back-reference, i.e. it looks for the longest
/// leading run of `prompt_path` that already appears as a component of `cwd`,
/// and rebuilds the full path from everything before it. Returns the directory
/// to `cd` to, if the prompt can be placed at all.
fn dirtrack_relative(prompt_path: &str, cwd: &str) -> Option<String> {
    // The back-reference's greedy group tries the whole prompt path first, then
    // each shorter `/`-delimited prefix of it.
    let mut heads = vec![prompt_path];
    heads.extend(
        prompt_path
            .match_indices('/')
            .map(|(i, _)| &prompt_path[..i])
            .rev(),
    );
    for head in heads.into_iter().filter(|h| !h.is_empty()) {
        // The prefix group is greedy as well, so the LAST placement wins; it
        // ends in `/`, and what follows the head must be a component boundary.
        for (start, _) in cwd.rmatch_indices(head) {
            if start == 0 || !cwd[..start].ends_with('/') {
                continue;
            }
            let after = &cwd[start + head.len()..];
            if after.is_empty() || after.starts_with('/') {
                return Some(format!("{}{prompt_path}", &cwd[..start]));
            }
        }
    }
    None
}

/// Parse the shell's reply to `dirs` into a directory list, current directory
/// first. Directory names may contain spaces and `dirs` does not quote them, so
/// Emacs works from the END of the line, gluing tokens back together until they
/// name a directory that exists.
fn parse_dirs_reply(line: &str) -> Vec<PathBuf> {
    // Split into alternating runs of whitespace and non-whitespace.
    let mut tokens: Vec<String> = Vec::new();
    for c in line.trim_end_matches(' ').chars() {
        match tokens.last_mut() {
            Some(last) if last.ends_with(char::is_whitespace) == c.is_whitespace() => last.push(c),
            _ => tokens.push(c.to_string()),
        }
    }
    let mut dirs: Vec<PathBuf> = Vec::new();
    while !tokens.is_empty() {
        let mut glued = String::new();
        let mut found = false;
        while let Some(token) = tokens.pop() {
            glued = format!("{token}{glued}");
            let path = zmax_stdx::path::expand_tilde(Path::new(&glued)).into_owned();
            if path.is_dir() {
                dirs.insert(0, path);
                // Swallow the separator before this entry.
                if tokens.last().is_some_and(|t| t.trim().is_empty()) {
                    tokens.pop();
                }
                glued = String::new();
                found = true;
                break;
            }
        }
        if !found {
            break;
        }
    }
    dirs
}

/// The interactive comint overlay hosting one subprocess.
pub struct Comint {
    program: String,
    scrollback: Arc<Mutex<Scrollback>>,
    /// Child handle, kept so the process is killed when the panel is dropped.
    child: Child,
    /// Writer to the child's stdin; `None` after `comint-send-eof`.
    stdin: Option<ChildStdin>,
    ring: InputRing,
    /// The current, unsubmitted input line and caret column (in chars).
    input: String,
    caret: usize,
    /// Input stashed when history navigation began, restored on `next` past the
    /// newest entry (Emacs `comint-stored-incomplete-input`).
    stash: Option<String>,
    /// Lines scrolled up from the bottom (0 = following the tail).
    scroll: usize,
    /// A scrollback line index that the next render should bring to the top of
    /// the body (resolved there, where the body height is known). Set by
    /// `comint-show-output` / `comint-next-prompt` / `comint-previous-prompt`.
    pending_top: Option<usize>,
    /// Set by a reader thread at EOF (the child exited).
    dead: Arc<AtomicBool>,
    /// Screen cursor, updated each render for `Component::cursor`.
    cursor: Option<zmax_core::Position>,
    /// `true` after a bare `C-c`, awaiting the second key of a `C-c <key>` comint
    /// prefix command (interrupt/stop/kill-input/prompt-nav/…).
    pending_ctrl_c: bool,
    /// An in-mode minibuffer read at the foot of the panel: which command is
    /// waiting for the line, and the line typed so far. Emacs reads these in the
    /// echo area (`C-c C-s` a file name, `M-r` a history pattern).
    reading: Option<(Reading, String)>,
    /// Where the subshell is and how we keep up with it — see [`DirTrack`].
    /// Shared with the reader threads, which run the `dirtrack-mode` output
    /// filter and consume the reply to the `dirs` query.
    dirstack: Arc<Mutex<DirTrack>>,
}

/// What an in-mode minibuffer read (see [`Comint::reading`]) will do with the
/// line when Enter is pressed.
#[derive(Clone, Copy)]
enum Reading {
    /// `C-c C-s` (`comint-write-output`): the file to write the last output to.
    WriteOutput,
    /// `M-r` (`comint-history-isearch-backward-regexp`): the pattern to search
    /// the input ring backward for.
    HistorySearch,
}

impl Comint {
    /// Open a comint on `$SHELL` (Emacs `M-x shell`).
    pub fn shell() -> std::io::Result<Self> {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
        Self::with_program(&shell, &[] as &[&str])
    }

    /// Open a comint running `program` with `args` (Emacs `comint-run`).
    pub fn with_program(
        program: &str,
        args: &[impl AsRef<std::ffi::OsStr>],
    ) -> std::io::Result<Self> {
        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let scrollback = Arc::new(Mutex::new(Scrollback::default()));
        let dead = Arc::new(AtomicBool::new(false));
        let dirstack = Arc::new(Mutex::new(DirTrack::new()));
        let stdin = child.stdin.take();

        // One reader thread each for stdout and stderr; both append lines to the
        // shared scrollback and request a redraw as output arrives.
        for pipe in [
            child.stdout.take().map(PipeSource::Out),
            child.stderr.take().map(PipeSource::Err),
        ]
        .into_iter()
        .flatten()
        {
            let scrollback = scrollback.clone();
            let dead = dead.clone();
            let dirstack = dirstack.clone();
            std::thread::spawn(move || {
                let is_out = matches!(pipe, PipeSource::Out(_));
                let mut reader: BufReader<Box<dyn std::io::Read + Send>> = match pipe {
                    PipeSource::Out(o) => BufReader::new(Box::new(o)),
                    PipeSource::Err(e) => BufReader::new(Box::new(e)),
                };
                // Read the pipe as BYTES and decode them with the coding system
                // emacs `set-buffer-process-coding-system` names (UTF-8 by
                // default). Reading straight into a `String` would instead make a
                // single byte that is not valid UTF-8 an `Err` — and the `Err`
                // arm here ends the thread, so one stray byte used to silently
                // kill every further line of output from the process.
                let mut buf: Vec<u8> = Vec::new();
                loop {
                    buf.clear();
                    match reader.read_until(b'\n', &mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {
                            let (decode, _) = zmax_core::coding::process_coding();
                            let line = zmax_core::coding::decode_with(decode, &buf);
                            let line = line.trim_end_matches(['\n', '\r']).to_string();
                            // `dirtrack-mode` is a comint PRE-output filter and
                            // the `dirs` reply is read straight off the process,
                            // so both run here, before the line becomes
                            // scrollback (the reply itself never does).
                            let mut swallowed = false;
                            let mut message = None;
                            if let Ok(mut dt) = dirstack.lock() {
                                if dt.resync {
                                    if !line.trim().is_empty() {
                                        dt.resync = false;
                                        dt.apply_dirs_reply(&line);
                                        message = dt.dirstack_message();
                                        swallowed = true;
                                    }
                                } else if dt.dirtrack {
                                    let before = dt.cwd.clone();
                                    dt.dirtrack_filter(&line);
                                    if dt.cwd != before {
                                        message = dt.dirstack_message();
                                    }
                                }
                            }
                            if let Ok(mut sb) = scrollback.lock() {
                                if !swallowed {
                                    sb.push(line);
                                }
                                if let Some(msg) = message {
                                    sb.push(msg);
                                }
                            }
                            zmax_event::request_redraw();
                        }
                    }
                }
                // Only stdout EOF marks the process as dead; stderr may close
                // first without the child having exited.
                if is_out {
                    dead.store(true, Ordering::Relaxed);
                    zmax_event::request_redraw();
                }
            });
        }

        Ok(Self {
            program: program.to_string(),
            scrollback,
            child,
            stdin,
            ring: InputRing::default(),
            input: String::new(),
            caret: 0,
            stash: None,
            scroll: 0,
            pending_top: None,
            dead,
            cursor: None,
            pending_ctrl_c: false,
            reading: None,
            dirstack,
        })
    }

    /// Echo `line` into the scrollback with the prompt, record it in the input
    /// ring and write it (plus a newline) to the child's stdin.
    fn submit(&mut self, line: &str) {
        if let Ok(mut sb) = self.scrollback.lock() {
            sb.push_kind(format!("{} {line}", self.prompt().trim_end()), true);
        }
        self.ring.add(line);
        if let Some(stdin) = self.stdin.as_mut() {
            // emacs `set-buffer-process-coding-system`: the ENCODE half — what the
            // text typed at the prompt is turned into bytes with on its way to the
            // process. Unset, this is UTF-8, exactly as before.
            let (_, encode) = zmax_core::coding::process_coding();
            let _ = stdin.write_all(&zmax_core::coding::encode_with(encode, line));
            let _ = stdin.write_all(b"\n");
            let _ = stdin.flush();
        }
        // `shell-directory-tracker` is a `comint-input-filter-function`: every
        // submitted line is inspected for `cd`/`pushd`/`popd`.
        let message = match self.dirstack.lock() {
            Ok(mut dt) => {
                let before = (dt.cwd.clone(), dt.stack.clone());
                dt.directory_tracker(line);
                ((dt.cwd.clone(), dt.stack.clone()) != before)
                    .then(|| dt.dirstack_message())
                    .flatten()
            }
            Err(_) => None,
        };
        if let (Some(msg), Ok(mut sb)) = (message, self.scrollback.lock()) {
            sb.push(msg);
        }
        self.scroll = 0;
    }

    /// Submit the current input line to the subprocess — `comint-send-input`.
    fn send_input(&mut self) {
        let line = std::mem::take(&mut self.input);
        self.caret = 0;
        self.stash = None;
        self.submit(&line);
    }

    /// Send a caller-supplied command line to the subprocess (used by `gud-*`
    /// to drive an inferior debugger running in this comint).
    pub fn send_command(&mut self, cmd: &str) {
        self.submit(cmd);
    }

    /// The input prompt shown at the bottom.
    fn prompt(&self) -> String {
        if self.dead.load(Ordering::Relaxed) {
            "[process exited] ".to_string()
        } else {
            "> ".to_string()
        }
    }

    /// Replace the input line with a history entry (or the stashed incomplete
    /// input) during `comint-previous-input` / `comint-next-input`.
    fn set_input(&mut self, text: &str) {
        self.input = text.to_string();
        self.caret = self.input.chars().count();
    }

    fn history_previous(&mut self) {
        if !self.ring.navigating() {
            self.stash = Some(self.input.clone());
        }
        if let Some(prev) = self.ring.previous().map(str::to_string) {
            self.set_input(&prev);
        }
    }

    fn history_next(&mut self) {
        match self.ring.next_input().map(str::to_string) {
            Some(next) => self.set_input(&next),
            None => {
                // Stepped past the newest entry: restore the stashed input.
                let stash = self.stash.take().unwrap_or_default();
                self.set_input(&stash);
            }
        }
    }

    /// Deliver Unix signal `sig` to the child process. Because the comint child
    /// is spawned with piped stdio (no controlling PTY / distinct process group),
    /// this signals the direct child by PID — faithful for a single inferior
    /// (shell/gdb) but not a whole job-control pipeline. Returns whether the
    /// signal was dispatched (`false` when the child has already exited).
    #[cfg(unix)]
    fn signal(&self, sig: i32) -> bool {
        if self.dead.load(Ordering::Relaxed) {
            return false;
        }
        // SAFETY: `kill(2)` with a valid pid and signal number has no memory
        // effects; a stale pid merely yields ESRCH, which we ignore.
        unsafe { libc::kill(self.child.id() as libc::pid_t, sig) == 0 }
    }
    #[cfg(not(unix))]
    fn signal(&self, _sig: i32) -> bool {
        false
    }

    /// `comint-interrupt-subjob` (C-c C-c) — send SIGINT to the child.
    pub fn interrupt_subjob(&self) -> bool {
        #[cfg(unix)]
        {
            self.signal(libc::SIGINT)
        }
        #[cfg(not(unix))]
        {
            false
        }
    }

    /// `comint-stop-subjob` (C-c C-z) — suspend the child with SIGTSTP.
    pub fn stop_subjob(&self) -> bool {
        #[cfg(unix)]
        {
            self.signal(libc::SIGTSTP)
        }
        #[cfg(not(unix))]
        {
            false
        }
    }

    /// `comint-continue-subjob` — resume a stopped child with SIGCONT.
    pub fn continue_subjob(&self) -> bool {
        #[cfg(unix)]
        {
            self.signal(libc::SIGCONT)
        }
        #[cfg(not(unix))]
        {
            false
        }
    }

    /// `comint-quit-subjob` — send SIGQUIT (core-dumping quit) to the child.
    pub fn quit_subjob(&self) -> bool {
        #[cfg(unix)]
        {
            self.signal(libc::SIGQUIT)
        }
        #[cfg(not(unix))]
        {
            false
        }
    }

    /// `comint-kill-subjob` — send SIGKILL to the child.
    pub fn kill_subjob(&self) -> bool {
        #[cfg(unix)]
        {
            self.signal(libc::SIGKILL)
        }
        #[cfg(not(unix))]
        {
            false
        }
    }

    /// `comint-kill-input` (C-c C-u) — discard the pending (unsubmitted) input.
    pub fn kill_input(&mut self) {
        self.input.clear();
        self.caret = 0;
        self.stash = None;
        self.ring.reset();
    }

    /// `comint-bol-or-process-mark` (C-a) — go to the process mark, or, when
    /// already there, to the true beginning of line.
    pub fn bol_or_process_mark(&mut self) {
        self.caret = zmax_core::comint::bol_or_process_mark_target(self.caret, 0);
    }

    /// `comint-delchar-or-maybe-eof` (C-d) — delete the char after point, or send
    /// EOF (close stdin) when the input line is empty. Returns whether EOF was
    /// sent.
    pub fn delchar_or_maybe_eof(&mut self) -> bool {
        if self.input.is_empty() {
            self.send_eof();
            return true;
        }
        if self.caret < self.input.chars().count() {
            let start = char_index_to_byte(&self.input, self.caret);
            let end = char_index_to_byte(&self.input, self.caret + 1);
            self.input.replace_range(start..end, "");
        }
        false
    }

    /// `comint-insert-previous-argument` (M-.) — insert the last argument of the
    /// previous command (`!$`) at point.
    pub fn insert_previous_argument(&mut self) {
        if let Some(arg) = self
            .ring
            .newest()
            .and_then(zmax_core::comint::last_argument)
            .map(str::to_string)
        {
            self.insert_str(&arg);
        }
    }

    /// `comint-magic-space` — expand any history designators (`!!`, `!$`, …) in
    /// the input line, then insert a space at point.
    pub fn magic_space(&mut self) {
        if let Some(expanded) = zmax_core::comint::expand_history(&self.input, &self.ring) {
            self.input = expanded;
            self.caret = self.input.chars().count();
        }
        self.insert_char(' ');
    }

    /// `comint-get-next-from-history` — replace the input with the next entry
    /// forward from the current history position (like `M-n` after a history
    /// yank).
    pub fn get_next_from_history(&mut self) {
        self.history_next();
    }

    /// `comint-copy-old-input` (C-c RET) — copy the most recent submitted input
    /// onto the current input line (zmax has no separate point in the
    /// scrollback, so this yanks the last command rather than the one under a
    /// buffer cursor).
    pub fn copy_old_input(&mut self) -> bool {
        if let Some(old) = self.ring.newest().map(str::to_string) {
            self.set_input(&old);
            self.ring.reset();
            true
        } else {
            false
        }
    }

    /// `comint-show-maximum-output` (C-c C-e) — scroll so the newest output is at
    /// the bottom of the window.
    pub fn show_maximum_output(&mut self) {
        self.scroll = 0;
        self.pending_top = None;
    }

    /// `comint-show-output` (C-c C-r) — put the beginning of the last command's
    /// output at the top of the window.
    pub fn show_output(&mut self) -> bool {
        let anchor = self
            .scrollback
            .lock()
            .ok()
            .and_then(|sb| zmax_core::comint::last_prompt_line(&sb.is_prompt));
        match anchor {
            Some(idx) => {
                self.pending_top = Some(idx);
                true
            }
            None => false,
        }
    }

    /// Shared prompt navigation for `comint-next-prompt` / `comint-previous-prompt`.
    fn goto_prompt(&mut self, forward: bool) -> bool {
        let (prompts, total) = match self.scrollback.lock() {
            Ok(sb) => (
                sb.is_prompt
                    .iter()
                    .enumerate()
                    .filter_map(|(i, &p)| p.then_some(i))
                    .collect::<Vec<_>>(),
                sb.lines.len(),
            ),
            Err(_) => return false,
        };
        if prompts.is_empty() {
            return false;
        }
        // The line currently anchored to the top of the body (approx.).
        let current_top = self
            .pending_top
            .unwrap_or_else(|| total.saturating_sub(self.scroll + 1));
        let target = if forward {
            prompts.iter().copied().find(|&p| p > current_top)
        } else {
            prompts.iter().copied().rev().find(|&p| p < current_top)
        };
        match target {
            Some(idx) => {
                self.pending_top = Some(idx);
                true
            }
            None => false,
        }
    }

    /// `comint-next-prompt` (C-c C-n) — move to the next prompt below.
    pub fn next_prompt(&mut self) -> bool {
        self.goto_prompt(true)
    }

    /// `comint-previous-prompt` (C-c C-p) — move to the previous prompt above.
    pub fn previous_prompt(&mut self) -> bool {
        self.goto_prompt(false)
    }

    /// `comint-delete-output` (C-c C-o) — delete the output produced by the last
    /// command (the lines after the most recent prompt). Returns how many lines
    /// were removed.
    pub fn delete_output(&mut self) -> usize {
        let Ok(mut sb) = self.scrollback.lock() else {
            return 0;
        };
        if let Some((start, end)) = zmax_core::comint::last_output_range(&sb.is_prompt) {
            sb.lines.drain(start..end);
            sb.is_prompt.drain(start..end);
            end - start
        } else {
            0
        }
    }

    /// `comint-write-output` — write the last command's output to `path`. Returns
    /// the number of lines written.
    pub fn write_output(&self, path: &std::path::Path) -> std::io::Result<usize> {
        let lines = {
            let sb = self
                .scrollback
                .lock()
                .map_err(|_| std::io::Error::other("scrollback poisoned"))?;
            match zmax_core::comint::last_output_range(&sb.is_prompt) {
                Some((start, end)) => sb.lines[start..end].to_vec(),
                None => Vec::new(),
            }
        };
        let mut body = lines.join("\n");
        if !body.is_empty() {
            body.push('\n');
        }
        std::fs::write(path, body)?;
        Ok(lines.len())
    }

    /// `comint-truncate-buffer` — trim the scrollback to at most `max` lines,
    /// keeping the newest (the tail nearest the prompt). Returns lines removed.
    pub fn truncate_buffer(&mut self, max: usize) -> usize {
        let Ok(mut sb) = self.scrollback.lock() else {
            return 0;
        };
        if sb.lines.len() > max {
            let drop = sb.lines.len() - max;
            sb.lines.drain(0..drop);
            sb.is_prompt.drain(0..drop);
            drop
        } else {
            0
        }
    }

    /// `comint-strip-ctrl-m` — strip carriage returns (`^M`) from every scrollback
    /// line. Returns the number of lines that changed.
    pub fn strip_ctrl_m(&mut self) -> usize {
        let Ok(mut sb) = self.scrollback.lock() else {
            return 0;
        };
        let mut changed = 0;
        for line in sb.lines.iter_mut() {
            if line.contains('\r') {
                *line = zmax_core::comint::strip_ctrl_m(line);
                changed += 1;
            }
        }
        changed
    }

    /// `comint-dynamic-list-input-ring` — echo the input history into the
    /// scrollback (Emacs pops a `*Input History*` help buffer; zmax lists it
    /// inline). Returns the number of entries listed.
    pub fn list_input_ring(&mut self) -> usize {
        let items: Vec<String> = self.ring.items().to_vec();
        if let Ok(mut sb) = self.scrollback.lock() {
            sb.push("=== Input history (newest first) ===".to_string());
            for (i, it) in items.iter().enumerate() {
                sb.push(format!("{:4}  {it}", i + 1));
            }
        }
        self.scroll = 0;
        items.len()
    }

    /// `comint-dynamic-list-filename-completions` — list, in the buffer, the file
    /// names that complete the file name before point. The fragment at the caret
    /// is split into a directory and a prefix
    /// (`zmax_core::comint::split_filename_fragment`), the directory is read,
    /// and every entry with that prefix is listed (directories with a trailing
    /// `/`). Returns how many completions were listed.
    pub fn list_filename_completions(&mut self) -> usize {
        let frag = zmax_core::comint::filename_fragment(&self.input, self.caret);
        let (dir, prefix) = zmax_core::comint::split_filename_fragment(&frag);
        let dir_path = resolve_dir(&self.directory(), &dir);
        let mut names: Vec<String> = match std::fs::read_dir(&dir_path) {
            Ok(rd) => rd
                .filter_map(|e| e.ok())
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().into_owned();
                    if !name.starts_with(&prefix) {
                        return None;
                    }
                    let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
                    Some(if is_dir { format!("{name}/") } else { name })
                })
                .collect(),
            Err(err) => {
                if let Ok(mut sb) = self.scrollback.lock() {
                    sb.push(format!("{}: {err}", dir_path.display()));
                }
                self.scroll = 0;
                return 0;
            }
        };
        names.sort();
        if let Ok(mut sb) = self.scrollback.lock() {
            if names.is_empty() {
                sb.push(format!("No completions of `{frag}`"));
            } else {
                sb.push(format!("=== Completions of `{frag}` ==="));
                for name in &names {
                    sb.push(format!("  {dir}{name}"));
                }
            }
        }
        self.scroll = 0;
        names.len()
    }

    /// `comint-send-invisible` — send `secret` to the subprocess *without* it
    /// appearing anywhere: it is not echoed into the scrollback and not recorded
    /// in the input ring, which is what makes it usable for a password prompt.
    /// `false` when stdin is already closed (`comint-send-eof`).
    pub fn send_invisible(&mut self, secret: &str) -> bool {
        let Some(stdin) = self.stdin.as_mut() else {
            return false;
        };
        let (_, encode) = zmax_core::coding::process_coding();
        let _ = stdin.write_all(&zmax_core::coding::encode_with(encode, secret));
        let _ = stdin.write_all(b"\n");
        let _ = stdin.flush();
        self.scroll = 0;
        true
    }

    /// `comint-history-isearch-backward-regexp` (degraded) — search the input
    /// ring backward for `needle` (substring) and yank the first older match onto
    /// the input line. Returns the matched entry.
    pub fn history_search_backward(&mut self, needle: &str) -> Option<String> {
        if !self.ring.navigating() {
            self.stash = Some(self.input.clone());
        }
        let found = self.ring.previous_matching(needle).map(str::to_string);
        if let Some(m) = &found {
            self.set_input(m);
        }
        found
    }

    /// `shell-forward-command` (M-f) — move point forward over the next shell
    /// command on the input line (`;`/`|`/`&`-separated).
    pub fn forward_command(&mut self) {
        self.caret = zmax_core::comint::forward_command(&self.input, self.caret);
    }

    /// `shell-backward-command` (M-b) — move point backward to the start of the
    /// shell command on the input line.
    pub fn backward_command(&mut self) {
        self.caret = zmax_core::comint::backward_command(&self.input, self.caret);
    }

    /// The directory this buffer's file names resolve from — Emacs's
    /// `default-directory`, kept up to date by the directory trackers.
    pub fn directory(&self) -> PathBuf {
        match self.dirstack.lock() {
            Ok(dt) => dt.cwd.clone(),
            Err(_) => PathBuf::from("."),
        }
    }

    /// The `pushd` stack (`shell-dirstack`), newest first, without the current
    /// directory.
    pub fn directory_stack(&self) -> Vec<PathBuf> {
        match self.dirstack.lock() {
            Ok(dt) => dt.stack.clone(),
            Err(_) => Vec::new(),
        }
    }

    /// `shell-dirtrack-mode` — watch the submitted input for `cd`/`pushd`/`popd`
    /// (on by default, as it is in `shell-mode`). Returns the new state.
    pub fn shell_dirtrack_mode(&mut self, on: Option<bool>) -> bool {
        match self.dirstack.lock() {
            Ok(mut dt) => {
                dt.input_track = on.unwrap_or(!dt.input_track);
                dt.input_track
            }
            Err(_) => false,
        }
    }

    /// `dirtrack-mode` — the alternative tracker: instead of parsing the input,
    /// read the directory out of the shell's PROMPT, matching each output line
    /// against `dirtrack-list`. Requires a prompt that carries the working
    /// directory and a `dirtrack-list` that matches it (see
    /// [`Self::set_dirtrack_list`]). Returns the new state.
    pub fn dirtrack_mode(&mut self, on: Option<bool>) -> bool {
        match self.dirstack.lock() {
            Ok(mut dt) => {
                dt.dirtrack = on.unwrap_or(!dt.dirtrack);
                dt.dirtrack
            }
            Err(_) => false,
        }
    }

    /// `dirtrack-list` — the prompt regexp `dirtrack-mode` matches and the group
    /// number holding the directory. Errs on a regexp that will not compile.
    pub fn set_dirtrack_list(&mut self, regexp: &str, group: usize) -> Result<(), String> {
        let re = regex::Regex::new(regexp).map_err(|e| format!("bad regexp: {e}"))?;
        if let Ok(mut dt) = self.dirstack.lock() {
            dt.dirtrack_re = Some(re);
            dt.dirtrack_group = group;
        }
        Ok(())
    }

    /// `shell-pushd-tohome` — bare `pushd` behaves as `pushd ~` (tcsh's option)
    /// rather than swapping with the top of the stack.
    pub fn set_pushd_tohome(&mut self, on: bool) {
        if let Ok(mut dt) = self.dirstack.lock() {
            dt.pushd_tohome = on;
        }
    }

    /// `shell-pushd-dextract` — `pushd +n` pops the n-th entry to the top of the
    /// stack (tcsh's option) instead of rotating the stack around it.
    pub fn set_pushd_dextract(&mut self, on: bool) {
        if let Ok(mut dt) = self.dirstack.lock() {
            dt.pushd_dextract = on;
        }
    }

    /// `shell-pushd-dunique` — `pushd` only stacks directories that are not on
    /// the stack already (tcsh's option).
    pub fn set_pushd_dunique(&mut self, on: bool) {
        if let Ok(mut dt) = self.dirstack.lock() {
            dt.pushd_dunique = on;
        }
    }

    /// `shell-dirtrack-verbose` — echo the directory stack after every change.
    pub fn set_dirtrack_verbose(&mut self, on: bool) {
        if let Ok(mut dt) = self.dirstack.lock() {
            dt.verbose = on;
        }
    }

    /// `dirs` (`shell-resync-dirs`) — resynchronise our idea of the directory
    /// stack by ASKING the shell: the query goes to the subprocess and the reply
    /// is consumed by the reader thread (it never reaches the scrollback), which
    /// rebuilds the stack from it. Returns whether the query could be sent.
    ///
    /// Emacs reads the reply synchronously and keeps its last line; over pipes,
    /// with no prompt to delimit the output, we take the first non-empty line
    /// the shell answers with.
    pub fn dirs(&mut self) -> bool {
        let Some(stdin) = self.stdin.as_mut() else {
            return false;
        };
        if let Ok(mut dt) = self.dirstack.lock() {
            dt.resync = true;
        }
        let _ = stdin.write_all(DIRSTACK_QUERY.as_bytes());
        let _ = stdin.write_all(b"\n");
        let _ = stdin.flush();
        true
    }

    /// The file names in `dir` that start with `prefix` (directories get a
    /// trailing `/`), sorted — the candidate set behind both
    /// `comint-dynamic-list-filename-completions` and TAB completion. `cwd` is
    /// the tracked working directory a relative `dir` resolves against.
    fn filename_candidates(cwd: &Path, dir: &str, prefix: &str) -> Vec<String> {
        let dir_path = resolve_dir(cwd, dir);
        let mut names: Vec<String> = match std::fs::read_dir(&dir_path) {
            Ok(rd) => rd
                .filter_map(|e| e.ok())
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().into_owned();
                    if !name.starts_with(prefix) {
                        return None;
                    }
                    let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
                    Some(if is_dir { format!("{name}/") } else { name })
                })
                .collect(),
            Err(_) => Vec::new(),
        };
        names.sort();
        names
    }

    /// The longest prefix every candidate shares — what TAB inserts when the
    /// completion is not unique (Emacs's `completion-at-point` "partial
    /// completion" step).
    fn common_prefix(names: &[String]) -> String {
        let Some(first) = names.first() else {
            return String::new();
        };
        let mut end = first.chars().count();
        for other in &names[1..] {
            let shared = first
                .chars()
                .zip(other.chars())
                .take_while(|(a, b)| a == b)
                .count();
            end = end.min(shared);
        }
        first.chars().take(end).collect()
    }

    /// `completion-at-point` (TAB in `shell-mode`): complete what is before point.
    /// The word that *names the command* completes against `PATH`
    /// (`shell-dynamic-complete-command`), any other word against the file system
    /// — the split Emacs's `shell-dynamic-complete-functions` makes. A unique
    /// candidate is inserted whole; several share their longest common prefix, and
    /// TAB on an already-complete prefix lists them (Emacs's second-TAB
    /// behaviour). Returns how many candidates matched.
    pub fn complete_at_point(&mut self) -> usize {
        if zmax_core::comint::is_command_position(&self.input, self.caret) {
            return self.complete_command_at_point();
        }
        let frag = zmax_core::comint::filename_fragment(&self.input, self.caret);
        let (dir, prefix) = zmax_core::comint::split_filename_fragment(&frag);
        let names = Self::filename_candidates(&self.directory(), &dir, &prefix);
        match names.len() {
            0 => {}
            1 => self.insert_str(&names[0][prefix.len()..]),
            _ => {
                let common = Self::common_prefix(&names);
                if common.len() > prefix.len() {
                    self.insert_str(&common[prefix.len()..]);
                } else {
                    // No progress to be made — show what the choices are.
                    self.list_filename_completions();
                }
            }
        }
        names.len()
    }

    /// `shell-dynamic-complete-command`: complete the word before point against the
    /// executables on `PATH`. Unique candidate → inserted; several → their common
    /// prefix, and when that adds nothing, the list is printed into the
    /// scrollback. Returns how many candidates matched.
    pub fn complete_command_at_point(&mut self) -> usize {
        let prefix = zmax_core::comint::filename_fragment(&self.input, self.caret);
        let names: Vec<String> = super::completers::programs_in_path()
            .iter()
            .filter(|n| n.starts_with(&prefix))
            .cloned()
            .collect();
        match names.len() {
            0 => {}
            1 => self.insert_str(&names[0][prefix.len()..]),
            _ => {
                let common = Self::common_prefix(&names);
                if common.len() > prefix.len() {
                    self.insert_str(&common[prefix.len()..]);
                } else if let Ok(mut sb) = self.scrollback.lock() {
                    sb.push(format!("=== Completions of `{prefix}` ==="));
                    for name in &names {
                        sb.push(format!("  {name}"));
                    }
                    self.scroll = 0;
                }
            }
        }
        names.len()
    }

    /// `backward-kill-word` (`C-c C-w`): delete from point back over the previous
    /// whitespace-delimited word of the input line.
    pub fn backward_kill_word(&mut self) {
        let chars: Vec<char> = self.input.chars().collect();
        let mut i = self.caret.min(chars.len());
        while i > 0 && chars[i - 1].is_whitespace() {
            i -= 1;
        }
        while i > 0 && !chars[i - 1].is_whitespace() {
            i -= 1;
        }
        let start = char_index_to_byte(&self.input, i);
        let end = char_index_to_byte(&self.input, self.caret.min(chars.len()));
        self.input.replace_range(start..end, "");
        self.caret = i;
        self.ring.reset();
    }

    /// Run the line typed into the in-mode minibuffer (see [`Reading`]).
    fn finish_reading(&mut self, what: Reading, text: &str) {
        match what {
            Reading::WriteOutput => {
                let path = std::path::PathBuf::from(zmax_stdx::path::expand_tilde(
                    std::path::Path::new(text.trim()),
                ));
                let msg = match self.write_output(&path) {
                    Ok(n) => format!("Wrote {n} line(s) to {}", path.display()),
                    Err(e) => format!("{}: {e}", path.display()),
                };
                if let Ok(mut sb) = self.scrollback.lock() {
                    sb.push(msg);
                }
                self.scroll = 0;
            }
            Reading::HistorySearch => {
                if self.history_search_backward(text.trim()).is_none() {
                    if let Ok(mut sb) = self.scrollback.lock() {
                        sb.push(format!("No earlier input matching `{}`", text.trim()));
                    }
                    self.scroll = 0;
                }
            }
        }
    }

    /// Insert an owned string at point (used by history/argument insertion).
    fn insert_str(&mut self, s: &str) {
        for c in s.chars() {
            self.insert_char(c);
        }
    }

    /// Close the child's stdin — `comint-send-eof`.
    fn send_eof(&mut self) {
        self.stdin = None;
    }

    fn insert_char(&mut self, c: char) {
        let byte = char_index_to_byte(&self.input, self.caret);
        self.input.insert(byte, c);
        self.caret += 1;
        self.ring.reset();
    }

    fn backspace(&mut self) {
        if self.caret == 0 {
            return;
        }
        let start = char_index_to_byte(&self.input, self.caret - 1);
        let end = char_index_to_byte(&self.input, self.caret);
        self.input.replace_range(start..end, "");
        self.caret -= 1;
        self.ring.reset();
    }

    fn close() -> EventResult {
        EventResult::Consumed(Some(Box::new(|c: &mut Compositor, _| {
            c.pop();
        })))
    }
}

impl Drop for Comint {
    /// A `std::process::Child` is *not* reaped on drop, so kill and wait for the
    /// subprocess explicitly when the panel closes to avoid orphaning the shell.
    fn drop(&mut self) {
        // Dropping stdin sends EOF first, letting well-behaved shells exit.
        self.stdin = None;
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A taken child pipe tagged with its source stream.
enum PipeSource {
    Out(std::process::ChildStdout),
    Err(std::process::ChildStderr),
}

/// Byte offset of the `n`-th char in `s` (or `s.len()` when `n` is past the end).
fn char_index_to_byte(s: &str, n: usize) -> usize {
    s.char_indices().nth(n).map_or(s.len(), |(b, _)| b)
}

impl Component for Comint {
    fn handle_event(&mut self, event: &Event, _cx: &mut Context) -> EventResult {
        let key = match event {
            Event::Key(key) => *key,
            _ => return EventResult::Ignored(None),
        };
        // F12 detaches the panel (KeyCode::F(12) has no `key!` form).
        if key.code == KeyCode::F(12) {
            return Comint::close();
        }
        // An in-mode minibuffer read (`C-c C-s`, `M-r`) owns every key.
        if let Some((what, mut buf)) = self.reading.take() {
            match key {
                key!(Esc) | ctrl!('g') => {}
                key!(Enter) => self.finish_reading(what, &buf),
                key!(Backspace) => {
                    buf.pop();
                    self.reading = Some((what, buf));
                }
                _ => {
                    if let KeyCode::Char(c) = key.code {
                        use zmax_view::keyboard::KeyModifiers;
                        if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT {
                            buf.push(c);
                        }
                    }
                    self.reading = Some((what, buf));
                }
            }
            return EventResult::Consumed(None);
        }

        // Second key of a `C-c <key>` comint job-control prefix.
        if self.pending_ctrl_c {
            self.pending_ctrl_c = false;
            match key {
                ctrl!('c') => {
                    self.interrupt_subjob();
                }
                ctrl!('u') => self.kill_input(),
                ctrl!('z') => {
                    self.stop_subjob();
                }
                ctrl!('n') => {
                    self.next_prompt();
                }
                ctrl!('p') => {
                    self.previous_prompt();
                }
                ctrl!('r') => {
                    self.show_output();
                }
                ctrl!('e') => self.show_maximum_output(),
                ctrl!('o') => {
                    self.delete_output();
                }
                key!(Enter) => {
                    self.copy_old_input();
                }
                // C-c C-a — comint-bol-or-process-mark.
                ctrl!('a') => self.bol_or_process_mark(),
                // C-c C-b / C-c C-f — shell-backward/forward-command: move over one
                // `;`/`|`/`&`-separated command on the input line.
                ctrl!('b') => self.backward_command(),
                ctrl!('f') => self.forward_command(),
                // C-c C-d — comint-send-eof (close the child's stdin).
                ctrl!('d') => self.send_eof(),
                // C-c C-l — comint-dynamic-list-input-ring.
                ctrl!('l') => {
                    self.list_input_ring();
                }
                // C-c C-s — comint-write-output: dump the last command's output.
                ctrl!('s') => self.reading = Some((Reading::WriteOutput, String::new())),
                // C-c C-w — backward-kill-word on the input line.
                ctrl!('w') => self.backward_kill_word(),
                // C-c C-x — comint-get-next-from-history.
                ctrl!('x') => self.get_next_from_history(),
                // C-c C-\ — comint-quit-subjob (SIGQUIT).
                ctrl!('\\') => {
                    self.quit_subjob();
                }
                // C-c . — comint-insert-previous-argument (`!$`).
                key!('.') => self.insert_previous_argument(),
                _ => {}
            }
            return EventResult::Consumed(None);
        }
        match key {
            key!(Enter) => self.send_input(),
            // Emacs binds history to M-p / M-n (comint-previous/next-input); Up/Down
            // do the same at the prompt via comint-{up,down}-or-history.
            key!(Up) | alt!('p') => self.history_previous(),
            key!(Down) | alt!('n') => self.history_next(),
            ctrl!('p') => self.history_previous(),
            ctrl!('n') => self.history_next(),
            key!(Home) | ctrl!('a') => self.bol_or_process_mark(),
            key!(End) | ctrl!('e') => self.caret = self.input.chars().count(),
            key!(Left) | ctrl!('b') => self.caret = self.caret.saturating_sub(1),
            key!(Right) | ctrl!('f') => {
                self.caret = (self.caret + 1).min(self.input.chars().count());
            }
            key!(Backspace) => self.backspace(),
            // C-k — kill from point to end of the input line (Emacs `kill-line`).
            ctrl!('k') => {
                let start = char_index_to_byte(&self.input, self.caret);
                self.input.truncate(start);
                self.ring.reset();
            }
            // M-. — comint-insert-previous-argument (!$).
            alt!('.') => self.insert_previous_argument(),
            // TAB — completion-at-point: complete the file name before point.
            key!(Tab) => {
                self.complete_at_point();
            }
            // M-? — comint-dynamic-list-filename-completions.
            alt!('?') => {
                self.list_filename_completions();
            }
            // M-r — comint-history-isearch-backward-regexp: read a pattern, then
            // yank the newest older input containing it onto the input line.
            alt!('r') => self.reading = Some((Reading::HistorySearch, String::new())),
            // C-c — enter the comint job-control prefix.
            ctrl!('c') => self.pending_ctrl_c = true,
            ctrl!('d') => {
                self.delchar_or_maybe_eof();
            }
            key!(PageUp) => self.scroll = self.scroll.saturating_add(5),
            key!(PageDown) => self.scroll = self.scroll.saturating_sub(5),
            // C-M-l — comint-show-output (the other Emacs binding of `C-c C-r`;
            // CONTROL|ALT is not expressible with the ctrl!/alt! macros).
            other
                if other.code == KeyCode::Char('l')
                    && other.modifiers
                        == zmax_view::keyboard::KeyModifiers::CONTROL
                            | zmax_view::keyboard::KeyModifiers::ALT =>
            {
                self.show_output();
            }
            _ => {
                // Plain printable character (no control/alt) -> text input.
                if let KeyCode::Char(c) = key.code {
                    use zmax_view::keyboard::KeyModifiers;
                    if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT {
                        self.insert_char(c);
                    }
                }
            }
        }
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        let theme = &ctx.editor.theme;
        let mut bg = theme.get("ui.background");
        // `transparent-background`: drop the panel fill so the terminal shows
        // through, matching the editor surface and the rest of the IDE.
        if ctx.editor.config().transparent_background {
            bg.bg = None;
        }
        let text_style = theme.get("ui.text");
        let header_style = theme.get("ui.text.focus");
        let prompt_style = theme.get("ui.text.directory");

        surface.clear_with(area, bg);
        if area.width < 4 || area.height < 3 {
            return;
        }

        let title = format!(" Comint: {} ", self.program);
        surface.set_stringn(area.x, area.y, &title, area.width as usize, header_style);

        // Body spans from the row below the title to the row above the input.
        let body_top = area.y + 1;
        let input_y = area.y + area.height - 1;
        let body_rows = input_y.saturating_sub(body_top) as usize;

        let lines = self
            .scrollback
            .lock()
            .map(|sb| sb.lines.clone())
            .unwrap_or_default();
        // Tail-follow, offset upward by `scroll`.
        let total = lines.len();
        // Resolve a pending "put this line at the top" request now that the body
        // height is known (comint-show-output / next-prompt / previous-prompt).
        if let Some(top) = self.pending_top.take() {
            let end = (top + body_rows).min(total);
            self.scroll = total.saturating_sub(end);
        }
        let max_scroll = total.saturating_sub(body_rows);
        let scroll = self.scroll.min(max_scroll);
        let end = total.saturating_sub(scroll);
        let start = end.saturating_sub(body_rows);
        for (i, line) in lines[start..end].iter().enumerate() {
            surface.set_stringn(
                area.x,
                body_top + i as u16,
                line,
                area.width as usize,
                text_style,
            );
        }

        // An in-mode minibuffer read (C-c C-s, M-r) takes over the input line.
        if let Some((what, buf)) = &self.reading {
            let label = match what {
                Reading::WriteOutput => "Write output to file: ",
                Reading::HistorySearch => "History search backward: ",
            };
            let line = format!("{label}{buf}");
            let cursor_x = area.x + line.chars().count() as u16;
            surface.set_stringn(area.x, input_y, &line, area.width as usize, header_style);
            self.cursor = Some(zmax_core::Position::new(
                input_y as usize,
                cursor_x as usize,
            ));
            return;
        }

        // Input line: prompt + current input.
        let prompt = self.prompt();
        surface.set_stringn(area.x, input_y, &prompt, area.width as usize, prompt_style);
        let px = area.x + prompt.chars().count() as u16;
        surface.set_stringn(
            px,
            input_y,
            &self.input,
            (area.width as usize).saturating_sub(prompt.chars().count()),
            text_style,
        );
        self.cursor = Some(zmax_core::Position::new(
            input_y as usize,
            (px as usize) + self.caret,
        ));
    }

    fn cursor(
        &self,
        _area: Rect,
        _editor: &zmax_view::editor::Editor,
    ) -> (Option<zmax_core::Position>, CursorKind) {
        (self.cursor, CursorKind::Block)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A comint on `cat` — a real child with piped stdio, so the input-line
    /// commands run against the same state the shell would. Killed on drop.
    fn comint() -> Comint {
        Comint::with_program("cat", &[] as &[&str]).expect("spawn cat")
    }

    /// TAB (`completion-at-point`) completes a unique file name whole, and stops
    /// at the longest common prefix when several match — it must not pick one.
    #[test]
    fn tab_completes_the_filename_before_point() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().display().to_string();
        std::fs::write(tmp.path().join("alpha.txt"), b"x").unwrap();
        std::fs::write(tmp.path().join("alpine.txt"), b"x").unwrap();
        std::fs::write(tmp.path().join("zulu.txt"), b"x").unwrap();

        // Unique: `z` completes to the whole name.
        let mut c = comint();
        c.set_input(&format!("cat {dir}/z"));
        c.caret = c.input.chars().count();
        assert_eq!(c.complete_at_point(), 1);
        assert!(c.input.ends_with("/zulu.txt"), "{}", c.input);

        // Ambiguous: `al` matches alpha/alpine, so only the shared `alp` is added.
        let mut c = comint();
        c.set_input(&format!("cat {dir}/al"));
        c.caret = c.input.chars().count();
        assert_eq!(c.complete_at_point(), 2);
        assert!(c.input.ends_with("/alp"), "{}", c.input);
        assert_eq!(c.caret, c.input.chars().count(), "point follows the insert");
    }

    /// The common-prefix step of TAB: the longest prefix EVERY candidate shares.
    #[test]
    fn common_prefix_is_shared_by_every_candidate() {
        let names = |v: &[&str]| -> Vec<String> { v.iter().map(|s| s.to_string()).collect() };
        assert_eq!(Comint::common_prefix(&names(&["alpha", "alpine"])), "alp");
        assert_eq!(Comint::common_prefix(&names(&["alpha"])), "alpha");
        // One outlier collapses it to nothing — TAB then lists instead of inserting.
        assert_eq!(
            Comint::common_prefix(&names(&["alpha", "alpine", "zulu"])),
            ""
        );
        assert_eq!(Comint::common_prefix(&[]), "");
    }

    /// `C-c C-w` (backward-kill-word) deletes back over one word, skipping any
    /// whitespace it starts in, and leaves point where the word began.
    #[test]
    fn backward_kill_word_deletes_one_word_back() {
        let mut c = comint();
        c.set_input("grep -n needle file.txt");
        c.caret = c.input.chars().count();

        c.backward_kill_word();
        assert_eq!(c.input, "grep -n needle ");
        assert_eq!(c.caret, c.input.chars().count());

        // Starting on the trailing blank, it skips it and eats `needle`.
        c.backward_kill_word();
        assert_eq!(c.input, "grep -n ");

        // Point is honoured: killing from the middle only removes what is behind it.
        c.set_input("alpha beta");
        c.caret = 7; // just after `b`
        c.backward_kill_word();
        assert_eq!(c.input, "alpha eta");
        assert_eq!(c.caret, 6);
    }

    /// A tracker sitting in `root` with `root/a`, `root/b` and `root/c` around
    /// it — the fixture every directory-stack test works from.
    fn tracker(root: &std::path::Path) -> (DirTrack, Vec<PathBuf>) {
        let dirs: Vec<PathBuf> = ["a", "b", "c"]
            .iter()
            .map(|n| {
                let p = root.join(n);
                std::fs::create_dir(&p).unwrap();
                p
            })
            .collect();
        let mut dt = DirTrack::new();
        dt.verbose = false;
        dt.cwd = root.to_path_buf();
        (dt, dirs)
    }

    /// `shell-directory-tracker`: `cd`/`pushd`/`popd` typed at the prompt move
    /// the buffer's directory and stack, including several commands on one line.
    #[test]
    fn input_tracker_follows_cd_pushd_and_popd() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut dt, dirs) = tracker(tmp.path());
        let root = tmp.path().to_path_buf();

        dt.directory_tracker(&format!(
            "pushd {} ; pushd {}",
            dirs[0].display(),
            dirs[1].display()
        ));
        assert_eq!(dt.cwd, dirs[1]);
        assert_eq!(dt.stack, vec![dirs[0].clone(), root.clone()]);

        // `popd` goes to the top entry and drops it.
        dt.directory_tracker("popd");
        assert_eq!(dt.cwd, dirs[0]);
        assert_eq!(dt.stack, vec![root.clone()]);

        // `cd -` returns to where the last `cd` left from; the stack is untouched.
        dt.directory_tracker(&format!("cd {}", dirs[2].display()));
        dt.directory_tracker("cd -");
        assert_eq!(dt.cwd, dirs[0]);
        assert_eq!(dt.stack, vec![root]);
    }

    /// `shell-pushd-dextract`: `pushd +n` pops the n-th entry to the top instead
    /// of rotating the stack around it.
    #[test]
    fn pushd_dextract_pops_the_nth_entry_to_the_top() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_path_buf();

        // Default (nil): the stack rotates, so what was below +2 follows it up.
        let (mut dt, dirs) = tracker(&root);
        dt.stack = dirs.clone();
        assert!(dt.process_pushd("+2"));
        assert_eq!(dt.cwd, dirs[1]);
        assert_eq!(
            dt.stack,
            vec![dirs[2].clone(), root.clone(), dirs[0].clone()]
        );

        // dextract: only the n-th entry moves; the rest keep their order below
        // the directory we left.
        let tmp2 = tempfile::tempdir().unwrap();
        let root2 = tmp2.path().to_path_buf();
        let (mut dt, dirs) = tracker(&root2);
        dt.pushd_dextract = true;
        dt.stack = dirs.clone();
        assert!(dt.process_pushd("+2"));
        assert_eq!(dt.cwd, dirs[1]);
        assert_eq!(dt.stack, vec![root2, dirs[0].clone(), dirs[2].clone()]);
    }

    /// `shell-pushd-dunique`: a directory already on the stack is not stacked
    /// again.
    #[test]
    fn pushd_dunique_keeps_the_stack_unique() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_path_buf();

        // Walk root -> a -> root, which leaves `root` on the stack, and then
        // leave `root` a second time.
        let walk = |dt: &mut DirTrack, dirs: &[PathBuf]| {
            assert!(dt.process_pushd(&dirs[0].to_string_lossy()));
            assert!(dt.process_pushd(&root.to_string_lossy()));
            assert!(dt.process_pushd(&dirs[1].to_string_lossy()));
        };

        // Default (nil): `root` is stacked twice over.
        let (mut dt, dirs) = tracker(&root);
        walk(&mut dt, &dirs);
        assert_eq!(dt.cwd, dirs[1]);
        assert_eq!(dt.stack, vec![root.clone(), dirs[0].clone(), root.clone()]);

        // dunique: the second departure from `root` does not re-stack it.
        let tmp2 = tempfile::tempdir().unwrap();
        let (mut dt, dirs) = tracker(tmp2.path());
        dt.cwd = root.clone();
        dt.pushd_dunique = true;
        walk(&mut dt, &dirs);
        assert_eq!(dt.cwd, dirs[1]);
        assert_eq!(dt.stack, vec![dirs[0].clone(), root]);
    }

    /// `shell-pushd-tohome`: bare `pushd` goes home rather than swapping with
    /// the top of the stack (and, with an empty stack, does anything at all).
    #[test]
    fn pushd_tohome_sends_bare_pushd_home() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut dt, dirs) = tracker(tmp.path());
        let root = tmp.path().to_path_buf();

        // Default (nil): bare `pushd` swaps the current directory with the top.
        dt.stack = vec![dirs[0].clone()];
        assert!(dt.process_pushd(""));
        assert_eq!(dt.cwd, dirs[0]);
        assert_eq!(dt.stack, vec![root.clone()]);

        // tohome: it becomes `pushd ~`, stacking where we were.
        let home = zmax_stdx::path::expand_tilde(Path::new("~")).into_owned();
        let (mut dt, _) = tracker(&tmp.path().join("a"));
        dt.pushd_tohome = true;
        assert!(dt.process_pushd(""));
        assert_eq!(dt.cwd, home);
        assert_eq!(dt.stack, vec![tmp.path().join("a")]);
    }

    /// The `dirs` reply is unquoted, so directory names containing spaces are
    /// only recoverable by gluing tokens back together from the end.
    #[test]
    fn dirs_reply_glues_names_that_contain_spaces() {
        let tmp = tempfile::tempdir().unwrap();
        let plain = tmp.path().join("plain");
        let spaced = tmp.path().join("two words");
        std::fs::create_dir(&plain).unwrap();
        std::fs::create_dir(&spaced).unwrap();

        let reply = format!("{} {} ", plain.display(), spaced.display());
        assert_eq!(
            parse_dirs_reply(&reply),
            vec![plain.clone(), spaced.clone()]
        );

        // And it lands as current directory + stack.
        let mut dt = DirTrack::new();
        dt.verbose = false;
        dt.apply_dirs_reply(&reply);
        assert_eq!(dt.cwd, plain);
        assert_eq!(dt.stack, vec![spaced]);
    }

    /// `dirtrack-mode` takes the directory from the prompt in the OUTPUT, and
    /// leaves the directory alone when the line does not match `dirtrack-list`.
    #[test]
    fn dirtrack_filter_reads_the_directory_from_the_prompt() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut dt, dirs) = tracker(tmp.path());
        dt.dirtrack = true;
        dt.dirtrack_re = Some(regex::Regex::new(r"^\[(.*)\]\$ ").unwrap());
        dt.dirtrack_group = 1;

        dt.dirtrack_filter(&format!("[{}]$ ", dirs[1].display()));
        assert_eq!(dt.cwd, dirs[1]);

        // Ordinary output is not a prompt: nothing moves.
        dt.dirtrack_filter("total 0");
        assert_eq!(dt.cwd, dirs[1]);

        // A prompt naming a directory that is not there is not followed.
        dt.dirtrack_filter(&format!("[{}]$ ", tmp.path().join("gone").display()));
        assert_eq!(dt.cwd, dirs[1]);
    }

    /// `shell-extract-num` accepts only `+n` for n > 0 — a bare directory name
    /// must not be read as a stack index.
    #[test]
    fn extract_num_matches_only_positive_stack_indices() {
        assert_eq!(extract_num("+1"), Some(1));
        assert_eq!(extract_num("+12"), Some(12));
        assert_eq!(extract_num("+0"), None);
        assert_eq!(extract_num("+"), None);
        assert_eq!(extract_num("1"), None);
        assert_eq!(extract_num("+1a"), None);
        assert_eq!(extract_num("~/+1"), None);
    }
}
