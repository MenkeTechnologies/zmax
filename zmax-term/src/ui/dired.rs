//! Dired — a directory-editor mode, the zmax port of GNU Emacs Dired.
//!
//! A full-screen [`Component`] listing one directory. Each row is a file or
//! subdirectory with a left-hand mark column (`*` marked, `D` flagged for
//! deletion), a type/size column and the name. Marks are keyed by file **name**
//! so they survive re-sorting and refresh. All pure logic (sorting, human sizes,
//! name transforms, the mark glyph) lives in the filesystem-free, unit-tested
//! [`zmax_core::dired`]; this module does the directory I/O, rendering and key
//! handling.
//!
//! Keys follow the real `dired-mode-map` (checked against Emacs 30's
//! `C-h b` dump), including the `%` / `*` / `:` / `C-t` / `M-s` prefix chords,
//! which this component walks with a one-key [`Prefix`] state:
//!
//! > Motion / visiting
//!   n / p / C-n / C-p / SPC / arrows — next / previous line
//!   C-v / M-v (PageDown / PageUp) — scroll a page forward / back
//!   RET, f, e — visit the file (or enter the subdirectory in place)
//!   a — dired-find-alternate-file; o — other window; C-o — display in a split
//!   v — view read-only; ^ — up to the parent; < / > — prev / next dirline
//!   j — dired-goto-file (prompt); g — revert (re-read); l — dired-do-redisplay
//! > Marks and flags
//!   m / u / DEL / t / U — mark, unmark, unmark-backward, toggle, unmark-all
//!   d — flag for deletion; x — delete the flagged; D — delete the targets now
//!   ~ / # — flag backups / auto-saves; . — dired-clean-directory (excess backups)
//!   M-DEL — dired-unmark-all-files (type the mark char, RET = every mark)
//!   * * / * / / * @ — mark executables / directories / symlinks
//!   * % — mark by regexp; * c — change marks; * s — mark this subdir's files
//!   * N — count+size of the marked; * ! — unmark all; * ? — unmark one mark char
//!   * m / * u / * t / * DEL — mark / unmark / toggle / unmark-backward
//!   * C-n / * C-p (and M-} / M-{) — next / previous marked file
//! > Operating on the marked files (or the file at point)
//!   C copy · R rename · S symlink · Y relsymlink · H hardlink · M chmod
//!   O chown · G chgrp · T touch · Z compress · c compress-to (archive)
//!   ! shell command · X shell command · & async shell command · P print
//!   N man · L load (elisp) · B byte-compile · A grep for a regexp
//!   Q grep-and-replace · E open externally · I run info on the file
//!   W browse-url-of-dired-file · = diff vs another file · w copy names to clipboard
//! > `%` — whole-name regexp batch ops
//!   % u / % l — upcase / downcase the names on disk
//!   % R / % C / % H / % S / % Y — rename / copy / hardlink / symlink / relsymlink
//!                                 each target through a regexp -> replacement
//!   % m / % g — mark by name regexp / by CONTENTS regexp
//!   % d / % & — flag by name regexp / flag garbage (build + TeX droppings)
//! > `:` — epa (gpg): : e encrypt · : d decrypt · : s sign · : v verify
//! > `C-t` — image-dired: C-t d / C-t i display · C-t x external viewer
//!           C-t c comment the marked · C-t e edit comment;tags
//! > `M-s` — search: M-s f C-s / C-M-s isearch file NAMES (literal / regexp)
//!           M-s a C-s / C-M-s search the marked files' CONTENTS
//! > Subdirectories
//!   i insert · $ hide · M-$ hide all · M-G goto subdir
//!   C-M-n / C-M-p — next / previous subdir; C-M-u / C-M-d — tree up / down
//! > Listing
//!   + create directory · M-+ create empty file · @ compare with another directory
//!   s cycle the sort order · r reverse · z toggle dotfiles · ( hide details
//!   k / K — kill (hide) lines · _ / C-_ — dired-undo · q / Esc — quit
//! > Mouse
//!   mouse-2 (middle click) — dired-mouse-find-file-other-window, or, under
//!     dired-click-to-select-mode, dired-mark-for-click (toggle the file's mark)
//!   touchscreen-hold — dired-enable-click-to-select-mode (no terminal event
//!     source; the handler exists for parity but never fires)
//!
//! zmax-only aliases kept alongside the Emacs keys (they predate the real
//! chords and stay bound so nothing that used them breaks): J goto-file,
//! K kill-lines, V open-externally, `,` clean-directory, `/` flag-by-regexp,
//! F hardlink-by-regexp, y mark-containing, h isearch-regexp, b byte-compile,
//! M-d/M-x mark dirs/executables, M-l load, M-m man, M-r print, M-q grep-replace,
//! M-n/M-p/M-u/M-y subdir motion, M-e/M-k/M-z epa, M-i/M-o image-dired,
//! M-f find-dired (the find ARGS string), M-F find-name, M-g find-grep,
//! M-c locate, M-t open in a new tab, M-w wdired. The find/locate commands list
//! their hits AS the Dired listing, the way emacs's find-dired buffer does, so
//! the results can be marked, deleted, visited and shelled over.
//!
//! Deferred / absent (honest): `?` dired-summary and `h` describe-mode (no
//! in-overlay help page), and the image-dired tag database commands
//! (C-t f / C-t r / C-t t). `touchscreen-hold` is ported as a never-firing
//! handler (terminals deliver no touch events), matching Emacs on a text terminal.

// The module doc above is an ASCII key-binding table where a leading `>` is a
// literal Dired key, not a Markdown blockquote — so lazy-continuation doesn't
// apply.
#![allow(clippy::doc_lazy_continuation)]

use std::collections::HashSet;
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::io::FromRawFd;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use regex::{Regex, RegexBuilder};
use tui::buffer::Buffer as Surface;
use zmax_core::dired::{
    backups_to_clean, destination_path, dirs_differ, human_size, is_executable_mode,
    is_valid_filename, mark_char, marked_summary, next_dir_index, parse_octal_mode,
    regexp_replace_name, relative_path, sort_entries, transform_name, DiredEntry, NameTransform,
    SortKey,
};
use zmax_view::{editor::Action, graphics::Rect};

use crate::{
    alt,
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};
use zmax_view::input::{KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use zmax_view::keyboard::{KeyCode, KeyModifiers};

/// A pending in-mode minibuffer read: which fs/listing action to run when the
/// user hits Enter, carrying the target names captured when the prompt opened.
enum Pending {
    CreateDir,
    CreateFile,
    Copy(Vec<String>),
    Rename(Vec<String>),
    Symlink(Vec<String>),
    Hardlink(Vec<String>),
    Chmod(Vec<String>),
    MarkRegexp,
    FlagRegexp,
    GotoFile,
    /// `dired-change-marks`: read "OLD NEW" (two chars) and re-mark.
    ChangeMarks,
    /// `dired-mark-files-containing-regexp`: mark files whose *contents* match.
    MarkContaining,
    /// `dired-isearch-filenames` (literal) / `-regexp`: move point to the next
    /// entry whose name matches the typed pattern.
    IsearchFilenames {
        regexp: bool,
    },
    /// `dired-do-relsymlink`: make a *relative* symlink of the targets in `dest`.
    RelSymlink(Vec<String>),
    /// `dired-do-chgrp` / `-chown`: run the external tool on the targets.
    Chgrp(Vec<String>),
    Chown(Vec<String>),
    /// `dired-do-compress-to`: archive the targets into the named archive.
    CompressTo(Vec<String>),
    /// `dired-do-shell-command`: run a shell command with the targets appended.
    ShellCommand(Vec<String>),
    /// `dired-do-async-shell-command` (`&`): same command line, but detached —
    /// Dired does not wait for it and the listing is not blocked.
    AsyncShellCommand(Vec<String>),
    /// `dired-diff`: diff the file at point against the file named here.
    Diff(String),
    /// `dired-compare-directories`: mark entries that differ from this directory.
    CompareDir,
    /// `find-dired`'s FIRST prompt: the directory to run `find` in. Emacs's
    /// interactive spec reads it before the args (`(read-directory-name "Run find
    /// in directory: " …)`, find-dired.el:189), so the read is two-legged; the
    /// answer is carried into [`Pending::FindArgs`].
    FindDir,
    /// `dired-do-find-regexp` / `dired-do-isearch`: search the targets' contents
    /// and show the hits. `regexp` is false for the LITERAL read (`M-s a C-s`,
    /// `dired-do-isearch`), which passes `multi-isearch-files` a plain string.
    FindRegexp { targets: Vec<String>, regexp: bool },
    /// First leg of a `% R`/`% C`/`% H`/`% S`/`% Y` regexp file op: read the
    /// match regexp; the second leg reads the replacement.
    RegexpOpPattern(RegexpKind, Vec<String>),
    /// Second leg: apply `kind` using the stashed regexp text and this replacement.
    RegexpOpReplace(RegexpKind, Vec<String>, String),
    /// `dired-goto-subdir`: jump to the inserted subdir section named here.
    GotoSubdir,
    /// `find-dired`: the whole find ARGS string, shell-parsed and run as
    /// `find . \( ARGS \)`.
    FindArgs(std::path::PathBuf),
    /// `find-name-dired`: list files under the tree whose name matches this glob.
    FindName,
    /// `find-grep-dired`: list files under the tree whose contents match this regexp.
    FindGrep,
    /// `epa-dired-do-encrypt`: gpg-encrypt the targets to the recipient named here.
    EpaEncrypt(Vec<String>),
    /// `locate` with a filter: run `locate` for this pattern, filtered to the dir.
    Locate,
    /// First leg of `dired-do-find-regexp-and-replace`: read the search regexp;
    /// the second leg reads the replacement string.
    FindReplacePattern(Vec<String>),
    /// Second leg: replace `pattern` with this text in every target file.
    FindReplaceWith(Vec<String>, String),
    /// `image-dired-dired-comment-files` / `-thumbnail-set-image-description`:
    /// set the comment on the given images.
    ImageComment(Vec<String>),
    /// `image-dired-dired-edit-comment-and-tags`: set `comment;tags` on the file.
    ImageCommentTags(String),
}

/// A pending Emacs Dired prefix chord: the first key has been typed and the next
/// key selects the command inside that prefix map (`dired-mode-map` binds `%`,
/// `*`, `:`, `C-t` and `M-s` as prefixes, so a single-key match cannot express
/// them). `M-s` is two levels deep (`M-s f C-s`), hence the split states.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Prefix {
    /// `%` — the whole-name regexp batch map.
    Pct,
    /// `*` — the mark map.
    Star,
    /// `:` — the epa (gpg) map.
    Epa,
    /// `C-t` — the image-dired map.
    Image,
    /// `M-s` — the search map; the next key picks the sub-map.
    Search,
    /// `M-s f` — file-NAME search; `C-s` literal, `C-M-s` regexp.
    SearchName,
    /// `M-s a` — search the marked files' CONTENTS; `C-s` literal, `C-M-s` regexp.
    SearchContents,
    /// `M-DEL` / `* ?` (`dired-unmark-all-files`): the next key is the mark
    /// character to remove everywhere — `RET` means every mark.
    UnmarkChar,
}

/// A whole-name regexp batch operation (`% R`/`% C`/`% H`/`% S`/`% Y`): rename,
/// copy, hardlink, absolute-symlink or relative-symlink each matching target to
/// its transformed name.
#[derive(Clone, Copy)]
enum RegexpKind {
    Rename,
    Copy,
    Hardlink,
    Symlink,
    RelSymlink,
}

impl RegexpKind {
    /// Prompt shown when reading the match regexp (first leg).
    fn pattern_prompt(self) -> &'static str {
        match self {
            RegexpKind::Rename => "Rename from (regexp): ",
            RegexpKind::Copy => "Copy from (regexp): ",
            RegexpKind::Hardlink => "Hardlink from (regexp): ",
            RegexpKind::Symlink => "Symlink from (regexp): ",
            RegexpKind::RelSymlink => "RelSymlink from (regexp): ",
        }
    }
    /// Prompt shown when reading the replacement (second leg).
    fn replace_prompt(self) -> &'static str {
        match self {
            RegexpKind::Rename => "Rename to (replacement): ",
            RegexpKind::Copy => "Copy to (replacement): ",
            RegexpKind::Hardlink => "Hardlink to (replacement): ",
            RegexpKind::Symlink => "Symlink to (replacement): ",
            RegexpKind::RelSymlink => "RelSymlink to (replacement): ",
        }
    }
    fn past(self) -> &'static str {
        match self {
            RegexpKind::Rename => "renamed",
            RegexpKind::Copy => "copied",
            RegexpKind::Hardlink => "hardlinked",
            RegexpKind::Symlink => "symlinked",
            RegexpKind::RelSymlink => "relsymlinked",
        }
    }
}

/// The four two-path filesystem operations Dired offers on a set of targets.
#[derive(Clone, Copy)]
enum LinkKind {
    Copy,
    Rename,
    Symlink,
    Hardlink,
}

impl LinkKind {
    /// Present-tense verb for an error message.
    fn verb(self) -> &'static str {
        match self {
            LinkKind::Copy => "copy",
            LinkKind::Rename => "rename",
            LinkKind::Symlink => "symlink",
            LinkKind::Hardlink => "hardlink",
        }
    }
    /// Past-tense verb for the success status line.
    fn past(self) -> &'static str {
        match self {
            LinkKind::Copy => "copied",
            LinkKind::Rename => "renamed",
            LinkKind::Symlink => "symlinked",
            LinkKind::Hardlink => "hardlinked",
        }
    }
}

/// The in-mode minibuffer state (Emacs Dired reads copy/rename/regexp arguments
/// in the echo area). Kept inside the component so the action can mutate the
/// listing and refresh it in place — a pushed `Prompt` layer could not reach
/// back into this component's marks.
struct Input {
    /// Owned because Emacs builds some of these prompts from the targets:
    /// `dired-do-shell-command` reads with `"! on %s: "` filled in by
    /// `dired-mark-prompt` (dired-aux.el), so the file name is part of the text.
    prompt: String,
    buffer: String,
    action: Pending,
    /// Live isearch state, for the one read that is not modal
    /// (`dired-isearch-filenames`). `None` for every other prompt.
    isearch: Option<Isearch>,
}

/// A point inside the text isearch may match: `(row, byte offset into that row's
/// file name)`. `dired-isearch-filenames` narrows the search to the file-name
/// regions, so the names are the whole address space and no match can straddle
/// two rows. The tuple orders lexicographically, i.e. in buffer order, which is
/// what the wrap tests compare.
type IPos = (usize, usize);

/// One entry of Emacs's `isearch-cmds`: the state the search was left in by a
/// single isearch command. `DEL` (`isearch-delete-char`) pops back to the entry
/// before, which is why the pattern is part of the state and not just the read's
/// buffer — `DEL` after a `C-s` undoes the repeat and keeps the pattern.
#[derive(Clone)]
struct IsearchState {
    string: String,
    /// Point. A forward search leaves it at the end of the match and a backward
    /// search at its start; `other_end` is always the opposite end (Emacs
    /// `isearch-other-end`).
    point: IPos,
    other_end: IPos,
    success: bool,
    forward: bool,
    wrapped: bool,
}

/// Live `dired-isearch-filenames[-regexp]` state hung off the open read.
struct Isearch {
    regexp: bool,
    /// `isearch-opoint`: where point was when the search started. `C-g` returns
    /// here, and it decides "wrapped" versus "overwrapped".
    opoint: IPos,
    /// `isearch-barrier`: the point of the last repeat, bounding how far back
    /// typing may drag a backward search.
    barrier: IPos,
    /// `isearch-just-started`: true until the first search runs, so the very
    /// first repeat does not try to step off an empty match.
    just_started: bool,
    /// `isearch-cmds`, oldest first; never empty.
    cmds: Vec<IsearchState>,
}

/// Translate an Emacs regexp into the dialect the `regex` crate speaks. The two
/// differ by an inversion: Emacs spells grouping and alternation `\(`, `\)`,
/// `\|`, `\{`, `\}` and treats the bare characters as literals, while `.`, `*`,
/// `+`, `?`, `[`, `]`, `^`, `$` are special on both sides. Word boundaries
/// `\<`, `\>`, `\_<`, `\_>` become `\b`, and the buffer anchors `` \` `` and
/// `\'` become `\A` and `\z`. Inside `[...]` Emacs has no escape character at
/// all, so a backslash there is a literal set member. `None` for a trailing
/// backslash, which Emacs rejects as an incomplete regexp, and for the
/// constructs the `regex` crate has no way to express — backreferences
/// (`\1`..`\9`), character categories (`\cC`) and the syntax classes beyond
/// whitespace and word (see the `\s` arm). The caller turns that `None` into
/// Emacs's `invalid-regexp`; emitting an untranslatable pattern instead would
/// either fail to compile or, worse, compile to something that matches other
/// text.
fn emacs_regexp_to_rust(pat: &str) -> Option<String> {
    let mut out = String::with_capacity(pat.len() + 8);
    let mut chars = pat.chars().peekable();
    let mut in_class = false;
    while let Some(c) = chars.next() {
        if in_class {
            match c {
                ']' => {
                    in_class = false;
                    out.push(c);
                }
                '\\' => out.push_str("\\\\"),
                _ => out.push(c),
            }
            continue;
        }
        match c {
            '[' => {
                in_class = true;
                out.push(c);
                // A leading `^` negates, and a `]` right after either of them is
                // a literal member rather than the closing bracket.
                if chars.peek() == Some(&'^') {
                    out.extend(chars.next());
                }
                if chars.peek() == Some(&']') {
                    chars.next();
                    out.push_str("\\]");
                }
            }
            '(' | ')' | '|' | '{' | '}' => {
                out.push('\\');
                out.push(c);
            }
            '\\' => match chars.next() {
                Some(e @ ('(' | ')' | '|' | '{' | '}')) => out.push(e),
                Some('<' | '>') => out.push_str("\\b"),
                // `\_<` / `\_>` — symbol boundaries, the closest analogue is `\b`.
                Some('_') => {
                    chars.next();
                    out.push_str("\\b");
                }
                Some('`') => out.push_str("\\A"),
                Some('\'') => out.push_str("\\z"),
                // `\=` anchors at point; there is no point to anchor to here.
                Some('=') => {}
                // `\sC` / `\SC` — syntax classes, read off the buffer's syntax
                // table. Only the two with an exact `regex`-crate analogue can
                // be translated: whitespace (`\s-`, also spelled `\s `) and word
                // constituent (`\sw`). Every other class (`\s_` symbol, `\s.`
                // punctuation, `\s(`/`\s)` parens, `\s"` string quote, …) is a
                // per-mode table lookup with no counterpart, so refuse the whole
                // pattern rather than emit `\s` + a stray character, which
                // compiles to whitespace-then-that-character and silently
                // matches the wrong text.
                Some(k @ ('s' | 'S')) => {
                    let complement = k == 'S';
                    match chars.next()? {
                        '-' | ' ' => out.push_str(if complement { "\\S" } else { "\\s" }),
                        'w' => out.push_str(if complement { "\\W" } else { "\\w" }),
                        _ => return None,
                    }
                }
                // `\cC` / `\CC` — character categories (`\cg` greek, `\cl`
                // latin, …). Emacs's category table has no `regex` equivalent.
                Some('c' | 'C') => return None,
                // `\1`..`\9` — backreferences. The `regex` crate is a finite
                // automaton and cannot support them at all, so there is nothing
                // to translate them into.
                Some('1'..='9') => return None,
                Some(e) => {
                    out.push('\\');
                    out.push(e);
                }
                None => return None,
            },
            _ => out.push(c),
        }
    }
    Some(out)
}

/// The `grep` flags a contents search runs with (`dired-do-find-regexp` and
/// `dired-do-isearch` both grep the targets here). A LITERAL search asks grep
/// for `-F` instead of backslash-quoting the pattern: grep's default dialect is
/// POSIX BRE with the GNU extensions, where `\+`, `\?`, `\|` and `\{` are
/// OPERATORS, so quoting a literal `a+` would hand grep the regexp "one or more
/// `a`" — the very misreading the quoting was meant to prevent.
fn find_grep_flags(regexp: bool) -> &'static [&'static str] {
    if regexp {
        &["-rnH"]
    } else {
        &["-rnH", "-F"]
    }
}

/// Emacs `isearch-no-upper-case-p`: with `case-fold-search` t and
/// `search-upper-case` non-nil, an isearch folds case unless the pattern itself
/// carries an upper-case letter. In a regexp a letter preceded by an odd number
/// of backslashes is an operator, not a literal, so it does not count — but an
/// explicit `[:upper:]` / `[:lower:]` class does.
pub(super) fn isearch_no_upper_case(text: &str, regexp: bool) -> bool {
    if regexp && (text.contains("[:upper:]") || text.contains("[:lower:]")) {
        return false;
    }
    let mut quoted = false;
    for c in text.chars() {
        if regexp && c == '\\' {
            quoted = !quoted;
            continue;
        }
        if !quoted && c.is_uppercase() {
            return false;
        }
        quoted = false;
    }
    true
}

/// Set a file's access + modification times to now (POSIX `utimes(path, NULL)`),
/// the `touch` behind Emacs `dired-do-touch`. Uses the already-present `libc`.
fn set_mtime_now(path: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    match std::ffi::CString::new(path.as_os_str().as_bytes()) {
        Ok(c) => unsafe { libc::utimes(c.as_ptr(), std::ptr::null()) == 0 },
        Err(_) => false,
    }
}

/// Read `dir` into a fresh, unsorted `Vec<DiredEntry>` (skipping dotfiles unless
/// `show_hidden`). Shared by the in-place [`Dired::read_dir`] and by
/// `dired-compare-directories`, which needs to read a *second* directory without
/// disturbing the current listing. Unreadable entries are skipped.
fn read_entries(dir: &Path, show_hidden: bool) -> std::io::Result<Vec<DiredEntry>> {
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        if !show_hidden && name.starts_with('.') {
            continue;
        }
        let ft = entry.file_type().ok();
        let meta = entry.metadata().ok();
        let mtime = meta
            .as_ref()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        entries.push(DiredEntry {
            name,
            is_dir: ft.map(|f| f.is_dir()).unwrap_or(false),
            is_symlink: ft.map(|f| f.is_symlink()).unwrap_or(false),
            size: meta.map(|m| m.len()).unwrap_or(0),
            mtime,
        });
    }
    Ok(entries)
}

/// Build the listing rows for an explicit set of paths named relative to `dir` —
/// the `find-dired` / `locate-dired` case, where the rows come from a command's
/// output instead of from reading a directory. Paths that have gone away are
/// dropped, which is what re-running the command would do to them. The type is
/// taken WITHOUT following symlinks and the size/time WITH, exactly as
/// [`read_entries`] does for a directory listing.
fn entries_for_paths(dir: &Path, paths: &[String]) -> Vec<DiredEntry> {
    let mut entries = Vec::new();
    for name in paths {
        let path = dir.join(name);
        let Ok(ft) = std::fs::symlink_metadata(&path).map(|m| m.file_type()) else {
            continue;
        };
        let meta = std::fs::metadata(&path).ok();
        let mtime = meta
            .as_ref()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        entries.push(DiredEntry {
            name: name.clone(),
            is_dir: ft.is_dir(),
            is_symlink: ft.is_symlink(),
            size: meta.map(|m| m.len()).unwrap_or(0),
            mtime,
        });
    }
    entries
}

/// The rows a `find .` run's output names. find prints `./sub/name`, and the
/// rows are named relative to the buffer's directory — the way the
/// inserted-subdir rows are — so every `dir.join(name)` file op still resolves.
/// `.` itself, which `find .` always prints, is not a result; emacs drops it the
/// same way (`find-dired-filter` rewrites ` ./FILE` to ` FILE`).
fn find_paths(body: &str) -> Vec<String> {
    body.lines()
        .map(|l| l.trim().trim_start_matches("./"))
        .filter(|l| !l.is_empty() && *l != ".")
        .map(str::to_string)
        .collect()
}

/// POSIX single-quote a shell word so a file name with spaces/metacharacters is
/// passed verbatim to `dired-do-shell-command`.
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// What a child wrote on its merged output stream, plus how it ended.
struct MergedOutput {
    text: String,
    /// The exit code, or `None` when the child was killed by a signal.
    status: Option<i32>,
}

/// Run `cmd` with stdout and stderr sharing ONE pipe, the way mg's `d_exec`
/// hands the child its output (`dup2(fds[1], STDOUT_FILENO)` immediately
/// followed by `dup2(fds[1], STDERR_FILENO)`, dired.c:711-712), so the two
/// streams interleave in the captured text exactly as they would on a terminal.
/// `Command::output()` gives each stream a pipe of its own and cannot do this.
fn merged_output(mut cmd: std::process::Command) -> std::io::Result<MergedOutput> {
    let mut fds = [0 as libc::c_int; 2];
    // SAFETY: `pipe` writes two fresh descriptors into the array it is given.
    if unsafe { libc::pipe(fds.as_mut_ptr()) } == -1 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: `pipe` returned success, so both descriptors are open and owned
    // here; wrapping them in `File` transfers that ownership.
    let (mut read, write) = unsafe {
        (
            std::fs::File::from_raw_fd(fds[0]),
            std::fs::File::from_raw_fd(fds[1]),
        )
    };
    let write_err = write.try_clone()?;
    cmd.stdout(write).stderr(write_err);
    let mut child = cmd.spawn()?;
    // `Command` keeps its own copy of an explicitly configured descriptor, so the
    // write end stays open in this process until the builder goes away — and the
    // read below would never see EOF.
    drop(cmd);
    let mut buf = Vec::new();
    read.read_to_end(&mut buf)?;
    Ok(MergedOutput {
        text: String::from_utf8_lossy(&buf).into_owned(),
        status: child.wait()?.code(),
    })
}

/// Emacs `dired-mark-prompt` (dired.el:4167): how the targets are named in a
/// Dired prompt — the file itself when there is one, else `* [N files]`.
fn mark_prompt(names: &[String]) -> String {
    match names {
        [only] => only.clone(),
        _ => format!("* [{} files]", names.len()),
    }
}

/// Active `wdired` (writable Dired) edit session: the directory being edited and
/// the original top-level file names, in listing order. Set when Dired switches
/// to wdired (dumping the names into an editable buffer) and consumed by
/// `wdired-finish-edit`, which pairs edited lines with these originals to rename.
pub(crate) static WDIRED_SESSION: std::sync::Mutex<Option<(PathBuf, Vec<String>)>> =
    std::sync::Mutex::new(None);

/// Pair the wdired-edited names against the originals and return the `(old, new)`
/// renames to apply — only entries whose name changed. Errors if the edited line
/// count no longer matches (a line was added/removed), which would desynchronize
/// the pairing.
pub(crate) fn wdired_rename_plan(
    originals: &[String],
    edited: &[String],
) -> Result<Vec<(String, String)>, String> {
    if edited.len() != originals.len() {
        return Err(format!(
            "line count changed ({} vs {})",
            edited.len(),
            originals.len()
        ));
    }
    Ok(originals
        .iter()
        .zip(edited)
        .filter(|(o, n)| o != n)
        .map(|(o, n)| (o.clone(), n.clone()))
        .collect())
}

/// The image-dired comment/tag store file (emacs keeps an `image-dired` db);
/// one `absolute-path\tcomment\ttags` record per line.
fn image_db_path() -> PathBuf {
    zmax_loader::config_dir().join("image-dired-comments")
}

/// Set (or clear, when empty) the comment and/or tags for `abs_path` in the
/// image-dired db. `None` leaves that field unchanged.
fn set_image_meta(abs_path: &str, comment: Option<&str>, tags: Option<&str>) {
    let path = image_db_path();
    let mut rows: Vec<(String, String, String)> = std::fs::read_to_string(&path)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            let mut it = l.splitn(3, '\t');
            Some((
                it.next()?.to_string(),
                it.next().unwrap_or("").to_string(),
                it.next().unwrap_or("").to_string(),
            ))
        })
        .collect();
    match rows.iter_mut().find(|r| r.0 == abs_path) {
        Some(r) => {
            if let Some(c) = comment {
                r.1 = c.to_string();
            }
            if let Some(t) = tags {
                r.2 = t.to_string();
            }
        }
        None => rows.push((
            abs_path.to_string(),
            comment.unwrap_or("").to_string(),
            tags.unwrap_or("").to_string(),
        )),
    }
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let body: String = rows
        .iter()
        .map(|(p, c, t)| format!("{p}\t{c}\t{t}"))
        .collect::<Vec<_>>()
        .join("\n");
    let _ = std::fs::write(&path, body);
}

/// Remove the overstrike sequences (`char BACKSPACE char`) that `man` emits to
/// render bold/underline for a terminal pager, leaving plain text.
fn strip_man_overstrike(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c == '\u{8}' {
            out.pop();
        } else {
            out.push(c);
        }
    }
    out
}

/// The interactive Dired overlay for a single directory.
pub struct Dired {
    dir: PathBuf,
    entries: Vec<DiredEntry>,
    /// Marked / deletion-flagged entries, keyed by file name (survive re-sort).
    marked: HashSet<String>,
    flagged: HashSet<String>,
    selected: usize,
    scroll: usize,
    viewport: usize,
    sort: SortKey,
    reverse: bool,
    show_hidden: bool,
    /// When false (Emacs `dired-hide-details-mode`), rows show only the mark and
    /// file name, hiding the type/size columns.
    show_details: bool,
    error: Option<String>,
    /// A non-error echo-area message (Emacs `message`, mg `ewprintf`). The
    /// overlay covers the editor's status line, so `Editor::set_status` is not
    /// visible while Dired is up; anything the user must read without closing
    /// Dired goes here and is drawn on the same row as `error`.
    message: Option<String>,
    /// Active in-mode minibuffer read, if any (see [`Input`]).
    input: Option<Input>,
    /// The listing as it was before the last `K`/undoable listing edit, for
    /// `dired-undo` (which restores the previous set of visible rows).
    undo_snapshot: Option<Vec<DiredEntry>>,
    /// Set by actions that display their result in a buffer beneath the overlay
    /// (`dired-diff`, `dired-do-find-regexp`): the overlay pops so the result is
    /// visible.
    close_requested: bool,
    /// A `find-dired` / `find-name-dired` / `find-grep-dired` / `locate` result
    /// listing: the exact paths the buffer shows, named relative to `dir`. Emacs
    /// makes these by inserting the command's output into a Dired buffer, so the
    /// rows are ordinary Dired rows and every command (mark, delete, visit, `!`)
    /// works on them. `None` is the ordinary directory listing.
    found: Option<Vec<String>>,
    /// Inserted subdirectories (Emacs `i` / `dired-maybe-insert-subdir`), as paths
    /// relative to `dir`, in insertion order. Each expands into a contiguous run
    /// of entries whose `name` carries the `reldir/` prefix.
    subdirs: Vec<String>,
    /// Inserted subdirs currently collapsed (Emacs `$` / `dired-hide-subdir`).
    hidden_subdirs: HashSet<String>,
    /// A prefix chord in flight (`%`, `*`, `:`, `C-t`, `M-s`, `M-DEL`); the next
    /// key completes it. See [`Prefix`].
    prefix: Option<Prefix>,
    /// Emacs `search-ring` / `regexp-search-ring`, one slot each: the last
    /// pattern a filename isearch exited with. `C-s` on an empty pattern
    /// repeats it.
    search_ring: Option<String>,
    regexp_search_ring: Option<String>,
    /// Where point was left INSIDE the row when the last filename isearch exited
    /// with RET. Emacs's point is a buffer position, so the next `M-s f C-s`
    /// carries on from the middle of the name the previous search stopped in
    /// rather than from the start of the row; `selected` alone cannot hold that
    /// column. Cleared by anything that moves the row cursor or re-reads the
    /// listing, since those put point back at the file name (`dired-move-to-filename`).
    isearch_point: Option<IPos>,
    /// Emacs `dired-click-to-select-mode`: when on, `mouse-2` toggles the clicked
    /// file's mark (`dired-mark-for-click`) instead of visiting it in another
    /// window. Turned on by a touch-screen "hold" gesture (`dired-touchscreen-hold`
    /// -> `dired-enable-click-to-select-mode`); turning it off unmarks everything.
    click_to_select: bool,
    /// The rectangle the last [`Component::render`] drew into, so a mouse click
    /// can be mapped back to the row (and therefore the entry) under it.
    area: Rect,
}

impl Dired {
    /// Open Dired on `dir`, reading its contents. Errors if the directory can't
    /// be read.
    pub fn new(dir: PathBuf) -> std::io::Result<Self> {
        let dir = std::fs::canonicalize(&dir).unwrap_or(dir);
        // Marks set from outside a listing (Emacs `image-mode-mark-file`, which
        // marks the file in whatever Dired buffer shows its directory) show up
        // the moment that directory is listed.
        let marked = crate::emacs_image::external_marks(&dir);
        let mut d = Dired {
            dir,
            entries: Vec::new(),
            marked,
            flagged: HashSet::new(),
            selected: 0,
            scroll: 0,
            viewport: 1,
            sort: SortKey::Name,
            reverse: false,
            show_hidden: false,
            show_details: true,
            error: None,
            message: None,
            input: None,
            undo_snapshot: None,
            close_requested: false,
            found: None,
            subdirs: Vec::new(),
            hidden_subdirs: HashSet::new(),
            prefix: None,
            search_ring: None,
            regexp_search_ring: None,
            isearch_point: None,
            click_to_select: false,
            area: Rect::default(),
        };
        d.read_dir()?;
        Ok(d)
    }

    /// The inserted-subdir prefix of an entry name (`"a/b/file"` -> `Some("a/b")`),
    /// or `None` for a top-level entry.
    fn entry_subdir(name: &str) -> Option<&str> {
        name.rsplit_once('/').map(|(dir, _)| dir)
    }

    /// The subdir section the point is in (`None` = the top directory).
    fn current_subdir(&self) -> Option<String> {
        self.entries
            .get(self.selected)
            .and_then(|e| Self::entry_subdir(&e.name))
            .map(str::to_string)
    }

    /// Emacs `i` / `dired-maybe-insert-subdir`: when point is on a directory,
    /// insert its listing as a new subdir section (prefixed entries); otherwise
    /// fall back to filename isearch. Re-inserting an already-shown subdir just
    /// un-hides and jumps to it.
    fn insert_subdir(&mut self) {
        let (is_dir, reldir) = match self.entries.get(self.selected) {
            Some(e) => (e.is_dir, e.name.clone()),
            None => return,
        };
        if !is_dir {
            self.begin_input(
                "Filename I-search: ",
                Pending::IsearchFilenames { regexp: false },
            );
            return;
        }
        self.hidden_subdirs.remove(&reldir);
        if !self.subdirs.iter().any(|s| s == &reldir) {
            self.subdirs.push(reldir.clone());
        }
        let _ = self.read_dir();
        // Move point onto the newly inserted section's first entry, if any.
        let target = format!("{reldir}/");
        if let Some(i) = self
            .entries
            .iter()
            .position(|e| e.name.starts_with(&target))
        {
            self.selected = i;
        }
    }

    /// Emacs `$` / `dired-hide-subdir`: collapse (or re-expand) the subdir section
    /// at point. On a top-level entry this is a no-op.
    fn hide_subdir(&mut self) {
        if let Some(sd) = self.current_subdir() {
            if self.hidden_subdirs.contains(&sd) {
                self.hidden_subdirs.remove(&sd);
            } else {
                self.hidden_subdirs.insert(sd);
            }
            let _ = self.read_dir();
        }
    }

    /// Emacs `M-$` / `dired-hide-all`: collapse every inserted subdir (or, when
    /// all are already hidden, re-expand them all).
    fn hide_all_subdirs(&mut self) {
        if self.subdirs.iter().all(|s| self.hidden_subdirs.contains(s)) {
            self.hidden_subdirs.clear();
        } else {
            self.hidden_subdirs = self.subdirs.iter().cloned().collect();
        }
        let _ = self.read_dir();
    }

    /// Read `self.dir` into `self.entries` (respecting `show_hidden`) and sort.
    /// Marks/flags naming files no longer present are dropped.
    fn read_dir(&mut self) -> std::io::Result<()> {
        // A `find-dired` result buffer lists an explicit set of paths rather than
        // a directory, so re-reading it re-stats those paths (dropping whatever
        // has since gone) instead of scanning `dir`. Inserted subdirectories
        // belong to a directory listing and have no meaning here.
        let mut entries = match &self.found {
            Some(paths) => entries_for_paths(&self.dir, paths),
            None => read_entries(&self.dir, self.show_hidden)?,
        };
        sort_entries(&mut entries, self.sort, self.reverse);
        // Append each inserted subdirectory's listing as a contiguous section,
        // prefixing every name with `reldir/` so it stays unique and every file
        // op (which does `self.dir.join(name)`) still resolves correctly. Drop any
        // subdir whose directory has gone away.
        self.subdirs.retain(|reldir| self.dir.join(reldir).is_dir());
        for reldir in &self.subdirs {
            if self.hidden_subdirs.contains(reldir) {
                continue;
            }
            if let Ok(mut sub) = read_entries(&self.dir.join(reldir), self.show_hidden) {
                sort_entries(&mut sub, self.sort, self.reverse);
                for e in &mut sub {
                    e.name = format!("{reldir}/{}", e.name);
                }
                entries.extend(sub);
            }
        }
        let present: HashSet<&String> = entries.iter().map(|e| &e.name).collect();
        self.marked.retain(|n| present.contains(n));
        self.flagged.retain(|n| present.contains(n));
        self.entries = entries;
        if self.selected >= self.entries.len() {
            self.selected = self.entries.len().saturating_sub(1);
        }
        // The rows have been rebuilt, so a column an isearch left in one of them
        // no longer means anything.
        self.isearch_point = None;
        self.error = None;
        Ok(())
    }

    fn resort(&mut self) {
        if self.subdirs.is_empty() {
            sort_entries(&mut self.entries, self.sort, self.reverse);
        } else {
            // With inserted subdirs, sort each section independently and keep the
            // sections contiguous — rebuild the sectioned listing.
            let _ = self.read_dir();
        }
    }

    fn move_selection(&mut self, delta: isize) {
        if self.entries.is_empty() {
            return;
        }
        let max = self.entries.len() as isize - 1;
        self.selected = (self.selected as isize + delta).clamp(0, max) as usize;
        // Moving the cursor puts point back on the file name
        // (`dired-move-to-filename`), so the column an isearch left is gone.
        self.isearch_point = None;
    }

    /// mg `dired-scroll-down` / `dired-scroll-up` (`C-v` / `M-v`): mg's
    /// `d_forwpage` / `d_backpage` are `forwpage` / `backpage` followed by
    /// `d_warpdot`, which puts point back on the file-name field (dired.c:889-900).
    ///
    /// `forwpage` moves the window top by `w_ntrows - 2` — a page less mg's
    /// two-line overlap — and refuses with "End of buffer" when that would step
    /// past the last line, leaving the window where it was; `backpage` is the
    /// mirror image with "Beginning of buffer". Neither moves point while it is
    /// still on screen after the scroll; when the page left it behind they drag
    /// it to the nearest visible line — the new top going forward, the last
    /// visible row going back (basic.c `forwpage`/`backpage`).
    fn scroll_page(&mut self, forward: bool) {
        if self.entries.is_empty() {
            return;
        }
        let last = self.entries.len() - 1;
        // mg clamps the overlap away in a window too short to have one.
        let step = self.viewport.saturating_sub(2).max(1);
        if forward {
            if self.scroll + step > last {
                self.message = Some("End of buffer".to_string());
                return;
            }
            self.scroll += step;
        } else {
            if self.scroll == 0 {
                self.message = Some("Beginning of buffer".to_string());
                return;
            }
            self.scroll = self.scroll.saturating_sub(step);
        }
        let bottom = (self.scroll + self.viewport.saturating_sub(1)).min(last);
        self.selected = self.selected.clamp(self.scroll, bottom);
        // `d_warpdot` puts point back on the file name, so a column a filename
        // isearch left in the old row is gone (as after any other motion).
        self.isearch_point = None;
    }

    fn current_name(&self) -> Option<String> {
        self.entries.get(self.selected).map(|e| e.name.clone())
    }

    /// Emacs `image-mode-mark-file` / `-unmark-file`: (un)mark `name` from
    /// outside the listing, so a mark set while viewing the image shows here as
    /// soon as this listing is on screen again. A no-op for another directory,
    /// which is how Emacs's loop over the Dired buffers behaves.
    pub fn set_external_mark(&mut self, dir: &Path, name: &str, mark: bool) {
        if self.dir != dir {
            return;
        }
        if mark {
            self.marked.insert(name.to_string());
        } else {
            self.marked.remove(name);
        }
    }

    /// Names to act on: the marked set if non-empty, else the entry at point.
    fn targets(&self) -> Vec<String> {
        if !self.marked.is_empty() {
            self.entries
                .iter()
                .filter(|e| self.marked.contains(&e.name))
                .map(|e| e.name.clone())
                .collect()
        } else {
            self.current_name().into_iter().collect()
        }
    }

    fn toggle_all_marks(&mut self) {
        let mut next = HashSet::new();
        for e in &self.entries {
            if !self.marked.contains(&e.name) {
                next.insert(e.name.clone());
            }
        }
        self.marked = next;
    }

    /// Move point to the next (`dir = 1`) or previous (`dir = -1`) marked file,
    /// wrapping around — Emacs `dired-next-marked-file` / `dired-prev-marked-file`.
    fn next_marked(&mut self, dir: isize) {
        let n = self.entries.len();
        if n == 0 || self.marked.is_empty() {
            return;
        }
        for step in 1..=n as isize {
            let idx = (self.selected as isize + dir * step).rem_euclid(n as isize) as usize;
            if self.marked.contains(&self.entries[idx].name) {
                self.selected = idx;
                return;
            }
        }
    }

    /// Flag every entry whose name satisfies `pred` for deletion, returning the
    /// number newly flagged — the shared engine behind the Emacs `~`/`#`/`&`
    /// dired flag-by-pattern commands.
    fn flag_matching(&mut self, pred: impl Fn(&str) -> bool) -> usize {
        let mut n = 0;
        for e in &self.entries {
            if pred(&e.name) && self.flagged.insert(e.name.clone()) {
                n += 1;
            }
        }
        n
    }

    /// Delete a set of names from disk (files or directory trees). Returns the
    /// count deleted; records the first error.
    fn delete_names(&mut self, names: &[String]) -> usize {
        let mut n = 0;
        for name in names {
            let path = self.dir.join(name);
            let res = if path.is_dir() {
                std::fs::remove_dir_all(&path)
            } else {
                std::fs::remove_file(&path)
            };
            match res {
                Ok(()) => {
                    self.marked.remove(name);
                    self.flagged.remove(name);
                    n += 1;
                }
                Err(e) => {
                    self.error = Some(format!("delete {name}: {e}"));
                    break;
                }
            }
        }
        n
    }

    /// Mark every entry satisfying `pred`, returning the number newly marked —
    /// the engine behind `dired-mark-directories` / `dired-mark-symlinks`.
    fn mark_where(&mut self, pred: impl Fn(&DiredEntry) -> bool) -> usize {
        let mut n = 0;
        for e in &self.entries {
            if pred(e) && self.marked.insert(e.name.clone()) {
                n += 1;
            }
        }
        n
    }

    /// `dired-mark-executables`: mark every regular file with an execute bit set.
    fn mark_executables(&mut self) -> usize {
        let names: Vec<String> = self
            .entries
            .iter()
            .filter(|e| !e.is_dir)
            .map(|e| e.name.clone())
            .collect();
        let mut n = 0;
        for name in names {
            if let Ok(meta) = std::fs::metadata(self.dir.join(&name)) {
                if is_executable_mode(meta.permissions().mode()) && self.marked.insert(name) {
                    n += 1;
                }
            }
        }
        n
    }

    /// `dired-upcase` / `dired-downcase`: rename the target(s) on disk applying
    /// `t`, then re-read the directory. Returns the number renamed.
    fn rename_transform(&mut self, t: NameTransform) -> usize {
        let targets = self.targets();
        let mut n = 0;
        for name in &targets {
            let new = transform_name(name, t);
            if new == *name {
                continue;
            }
            let from = self.dir.join(name);
            let to = self.dir.join(&new);
            match std::fs::rename(&from, &to) {
                Ok(()) => {
                    self.marked.remove(name);
                    self.flagged.remove(name);
                    n += 1;
                }
                Err(e) => {
                    self.error = Some(format!("rename {name}: {e}"));
                    break;
                }
            }
        }
        let _ = self.read_dir();
        n
    }

    /// `dired-do-kill-lines`: drop the target entries from the *listing* only
    /// (they stay on disk). Returns how many rows were removed.
    fn kill_lines(&mut self) -> usize {
        let targets: HashSet<String> = self.targets().into_iter().collect();
        if targets.is_empty() {
            return 0;
        }
        self.undo_snapshot = Some(self.entries.clone());
        let before = self.entries.len();
        self.entries.retain(|e| !targets.contains(&e.name));
        for name in &targets {
            self.marked.remove(name);
            self.flagged.remove(name);
        }
        if self.selected >= self.entries.len() {
            self.selected = self.entries.len().saturating_sub(1);
        }
        before - self.entries.len()
    }

    /// `dired-undo`: restore the listing captured by the last undoable edit
    /// (currently `K` kill-lines / `dired-clean-directory` listing changes). Only
    /// the *visible rows* are restored — files are never touched on disk. Returns
    /// whether anything was undone.
    /// `dired-undo` (`_`): restore the listing snapshot taken before the last
    /// listing-changing command (`dired-do-kill-lines`, `dired-clean-directory`).
    /// `false` when there is nothing to undo. Public so the `dired-undo` command
    /// can drive the live Dired from the command palette, not just the key.
    pub fn undo(&mut self) -> bool {
        if let Some(snap) = self.undo_snapshot.take() {
            self.entries = snap;
            if self.selected >= self.entries.len() {
                self.selected = self.entries.len().saturating_sub(1);
            }
            true
        } else {
            false
        }
    }

    /// `dired-mark-subdir-files`: mark every regular file (not a subdirectory) in
    /// the current directory listing. Returns the number newly marked.
    fn mark_subdir_files(&mut self) -> usize {
        self.mark_where(|e| !e.is_dir)
    }

    /// `dired-change-marks`: replace every occurrence of mark char `old` with
    /// `new` across the listing (`*`/`D` are the two marks Dired shows). Returns
    /// how many entries were re-marked.
    fn change_marks(&mut self, old: char, new: char) -> usize {
        // Snapshot the names carrying `old`, then move them to `new`'s set.
        let names: Vec<String> = self
            .entries
            .iter()
            .map(|e| e.name.clone())
            .filter(|n| match old {
                '*' => self.marked.contains(n),
                'D' => self.flagged.contains(n),
                _ => false,
            })
            .collect();
        for n in &names {
            self.marked.remove(n);
            self.flagged.remove(n);
        }
        for n in &names {
            match new {
                '*' => {
                    self.marked.insert(n.clone());
                }
                'D' => {
                    self.flagged.insert(n.clone());
                }
                // A space (or anything else) clears the mark entirely.
                _ => {}
            }
        }
        names.len()
    }

    /// `dired-mark-files-containing-regexp`: mark every regular file whose textual
    /// contents match `re`. Binary/unreadable files are skipped. Returns the count.
    fn mark_containing(&mut self, re: &Regex) -> usize {
        let names: Vec<String> = self
            .entries
            .iter()
            .filter(|e| !e.is_dir)
            .map(|e| e.name.clone())
            .collect();
        let mut n = 0;
        for name in names {
            if let Ok(bytes) = std::fs::read(self.dir.join(&name)) {
                if let Ok(text) = String::from_utf8(bytes) {
                    if re.is_match(&text) && self.marked.insert(name) {
                        n += 1;
                    }
                }
            }
        }
        n
    }

    /// `dired-clean-directory`: flag excess numbered backups (`file.~N~`) for
    /// deletion, keeping the two newest versions of each base (Emacs
    /// `dired-kept-versions`). Snapshots for `dired-undo`. Returns count flagged.
    fn clean_directory(&mut self) -> usize {
        let names: Vec<String> = self.entries.iter().map(|e| e.name.clone()).collect();
        let excess = backups_to_clean(&names, 2);
        self.undo_snapshot = Some(self.entries.clone());
        let mut n = 0;
        for name in excess {
            if self.flagged.insert(name) {
                n += 1;
            }
        }
        n
    }

    /// `dired-do-relsymlink`: make a *relative* symlink of each target into the
    /// destination directory `dest`. Returns the count linked.
    fn relsymlink_targets(&mut self, targets: &[String], dest: &str, cx: &mut Context) {
        if dest.is_empty() || targets.is_empty() {
            return;
        }
        let dest_path = self.dir.join(dest);
        let dest_is_dir = dest_path.is_dir();
        let mut n = 0;
        for name in targets {
            let src = self.dir.join(name);
            let link = destination_path(&dest_path, dest_is_dir, name);
            let link_dir = link.parent().map(Path::to_path_buf).unwrap_or_default();
            let rel = relative_path(&link_dir, &src);
            match std::os::unix::fs::symlink(&rel, &link) {
                Ok(()) => n += 1,
                Err(e) => {
                    cx.editor.set_error(format!("relsymlink {name}: {e}"));
                    break;
                }
            }
        }
        let _ = self.read_dir();
        cx.editor
            .set_status(format!("dired: relsymlinked {n} file(s)"));
    }

    /// Run a whole-name regexp op (`% R`/`% C`/`% H`/`% S`/`% Y`): for each target
    /// matching `re`, compute its transformed name via [`regexp_replace_name`] and
    /// apply `kind` (rename/copy/hardlink/(rel)symlink) in the same directory.
    fn run_regexp_op(
        &mut self,
        kind: RegexpKind,
        targets: &[String],
        re: &Regex,
        repl: &str,
        cx: &mut Context,
    ) {
        let mut n = 0;
        for name in targets {
            let new = match regexp_replace_name(name, re, repl) {
                Some(new) if new != *name => new,
                _ => continue,
            };
            let src = self.dir.join(name);
            let dst = self.dir.join(&new);
            let res = match kind {
                RegexpKind::Rename => std::fs::rename(&src, &dst),
                RegexpKind::Copy => std::fs::copy(&src, &dst).map(|_| ()),
                RegexpKind::Hardlink => std::fs::hard_link(&src, &dst),
                RegexpKind::Symlink => std::os::unix::fs::symlink(&src, &dst),
                RegexpKind::RelSymlink => {
                    let rel = relative_path(&self.dir, &src);
                    std::os::unix::fs::symlink(&rel, &dst)
                }
            };
            match res {
                Ok(()) => n += 1,
                Err(e) => {
                    cx.editor.set_error(format!("{name}: {e}"));
                    break;
                }
            }
        }
        let _ = self.read_dir();
        cx.editor
            .set_status(format!("dired: {} {n} file(s)", kind.past()));
    }

    /// `dired-do-touch`: set the target(s)' mtime to now, then refresh.
    fn touch_targets(&mut self) -> usize {
        let targets = self.targets();
        let mut n = 0;
        for name in &targets {
            if set_mtime_now(&self.dir.join(name)) {
                n += 1;
            } else {
                self.error = Some(format!("touch {name}: failed"));
                break;
            }
        }
        let _ = self.read_dir();
        n
    }

    /// Build a callback that opens the file at point in another editor window
    /// via `action` (a split), popping this overlay. Directories are entered in
    /// place instead. `read_only` marks the opened document read-only.
    fn open_file(&mut self, action: Action, read_only: bool) -> Option<Callback> {
        if self.entries.get(self.selected)?.is_dir {
            self.visit();
            return None;
        }
        let name = self.current_name()?;
        let path = self.dir.join(&name);
        Some(Box::new(
            move |compositor: &mut Compositor, cx: &mut Context| {
                compositor.pop();
                match cx.editor.open(&path, action) {
                    Ok(id) => {
                        if read_only {
                            if let Some(doc) = cx.editor.document_mut(id) {
                                doc.readonly = true;
                            }
                            cx.editor.set_status(format!(
                                "dired: viewing {} (read-only)",
                                path.display()
                            ));
                        }
                    }
                    Err(err) => cx
                        .editor
                        .set_error(format!("failed to open {}: {err}", path.display())),
                }
            },
        ))
    }

    /// Open the in-mode minibuffer for `action`, showing `prompt`.
    fn begin_input(&mut self, prompt: impl Into<String>, action: Pending) {
        // `dired-isearch-filenames` is live rather than modal, so it carries the
        // whole isearch state machine (command stack, direction, wrap flags).
        //
        // A search starts AT POINT, not at the start of the row: emacs leaves
        // point inside the name the previous search stopped in, so `M-s f C-s`
        // twice in a row finds the second match in that name rather than the same
        // one again. `isearch_point` is that column, good only while the cursor
        // has not left the row it was recorded on.
        let start = match self.isearch_point.take() {
            Some(p) if p.0 == self.selected => p,
            _ => (self.selected, 0),
        };
        let isearch = match &action {
            Pending::IsearchFilenames { regexp } => Some(Isearch {
                regexp: *regexp,
                opoint: start,
                barrier: start,
                just_started: true,
                cmds: vec![IsearchState {
                    string: String::new(),
                    point: start,
                    other_end: start,
                    success: true,
                    forward: true,
                    wrapped: false,
                }],
            }),
            _ => None,
        };
        self.input = Some(Input {
            prompt: prompt.into(),
            buffer: String::new(),
            action,
            isearch,
        });
    }

    /// The end of the searchable text (Emacs `point-max` narrowed to the names).
    fn isearch_point_max(&self) -> IPos {
        match self.entries.last() {
            Some(e) => (self.entries.len() - 1, e.name.len()),
            None => (0, 0),
        }
    }

    /// One character forward from `p`, crossing into the next row at the end of
    /// a name. `None` at point-max.
    fn isearch_forward_char(&self, p: IPos) -> Option<IPos> {
        let name = &self.entries.get(p.0)?.name;
        match name.get(p.1..).and_then(|s| s.chars().next()) {
            Some(c) => Some((p.0, p.1 + c.len_utf8())),
            None if p.0 + 1 < self.entries.len() => Some((p.0 + 1, 0)),
            None => None,
        }
    }

    /// One character backward from `p`, crossing into the previous row at the
    /// start of a name. `None` at point-min.
    fn isearch_backward_char(&self, p: IPos) -> Option<IPos> {
        let name = &self.entries.get(p.0)?.name;
        match name.get(..p.1).and_then(|s| s.chars().next_back()) {
            Some(c) => Some((p.0, p.1 - c.len_utf8())),
            None if p.0 > 0 => Some((p.0 - 1, self.entries[p.0 - 1].name.len())),
            None => None,
        }
    }

    /// Compile the pattern into the matcher Emacs would use: a literal search
    /// quotes it, a regexp search is translated out of Emacs syntax, and both
    /// fold case unless the pattern itself carries an upper-case letter. `None`
    /// for a pattern that does not parse (yet) — Emacs's `invalid-regexp`.
    ///
    /// `pub(super)` because comint's input-history isearch
    /// (`comint-history-isearch-backward-regexp`) needs the very same matcher:
    /// both are plain isearches over text, so they must agree on the Emacs regexp
    /// dialect and on `isearch-no-upper-case-p` case folding.
    pub(super) fn isearch_matcher(text: &str, regexp: bool) -> Option<Regex> {
        let pat = if regexp {
            emacs_regexp_to_rust(text)?
        } else {
            regex::escape(text)
        };
        RegexBuilder::new(&pat)
            .case_insensitive(isearch_no_upper_case(text, regexp))
            .build()
            .ok()
    }

    /// `re-search-forward` / `re-search-backward` restricted to the file names:
    /// the match nearest `from` in the given direction. Returns `(point,
    /// other_end)` — forward leaves point at the match end, backward at its
    /// start. No wrapping; the repeat command wraps explicitly.
    fn isearch_find(&self, re: &Regex, from: IPos, forward: bool) -> Option<(IPos, IPos)> {
        let last = self.entries.len().checked_sub(1)?;
        if forward {
            (from.0..self.entries.len()).find_map(|row| {
                let name = &self.entries[row].name;
                let start = if row == from.0 {
                    from.1.min(name.len())
                } else {
                    0
                };
                re.find_at(name, start)
                    .map(|m| ((row, m.end()), (row, m.start())))
            })
        } else {
            (0..=from.0.min(last)).rev().find_map(|row| {
                let name = &self.entries[row].name;
                let lim = if row == from.0 {
                    from.1.min(name.len())
                } else {
                    name.len()
                };
                // Emacs takes the latest match that still ends at or before the
                // limit. Anchoring each candidate start keeps overlapping
                // matches visible, which a non-overlapping scan would skip.
                (0..=lim)
                    .rev()
                    .filter(|s| name.is_char_boundary(*s))
                    .find_map(|s| {
                        re.find_at(name, s)
                            .filter(|m| m.start() == s && m.end() <= lim)
                            .map(|m| ((row, m.start()), (row, m.end())))
                    })
            })
        }
    }

    /// Emacs `isearch-search`: search from `st.point` with the current pattern
    /// and record success, point and other-end. A pattern that does not parse
    /// leaves the state untouched, so point holds while a regexp is half typed.
    fn isearch_search(&mut self, regexp: bool, st: &mut IsearchState) {
        let Some(re) = Self::isearch_matcher(&st.string, regexp) else {
            return;
        };
        match self.isearch_find(&re, st.point, st.forward) {
            Some((point, other)) => {
                st.point = point;
                st.other_end = other;
                st.success = true;
            }
            None => {
                st.success = false;
                // A failed search leaves point where the command found it, not
                // where the search was started from: Emacs restores it from the
                // state on top of `isearch-cmds`, which this command has not
                // pushed onto yet.
                if let Some(prev) = self
                    .input
                    .as_ref()
                    .and_then(|inp| inp.isearch.as_ref())
                    .and_then(|is| is.cmds.last())
                {
                    st.point = prev.point;
                }
            }
        }
        if let Some(is) = self.input.as_mut().and_then(|inp| inp.isearch.as_mut()) {
            is.just_started = false;
        }
    }

    /// The live search's constants plus a copy of its current state, for a
    /// command to mutate and push back.
    fn isearch_snapshot(&self) -> Option<(bool, IPos, IPos, bool, IsearchState)> {
        let is = self.input.as_ref()?.isearch.as_ref()?;
        Some((
            is.regexp,
            is.opoint,
            is.barrier,
            is.just_started,
            is.cmds.last()?.clone(),
        ))
    }

    /// `isearch-push-state`: record the state this command ended in, then show
    /// it (the footer echoes the pattern, the selection follows point).
    fn isearch_push(&mut self, st: IsearchState) {
        if let Some(is) = self.input.as_mut().and_then(|inp| inp.isearch.as_mut()) {
            is.cmds.push(st);
        }
        self.isearch_sync();
    }

    /// Mirror the current isearch state into the visible read.
    fn isearch_sync(&mut self) {
        let Some((text, row)) = self
            .input
            .as_ref()
            .and_then(|inp| inp.isearch.as_ref())
            .and_then(|is| is.cmds.last())
            .map(|st| (st.string.clone(), st.point.0))
        else {
            return;
        };
        if let Some(inp) = self.input.as_mut() {
            inp.buffer = text;
        }
        self.selected = row.min(self.entries.len().saturating_sub(1));
    }

    /// `isearch-printing-char`: extend the pattern and re-search
    /// (`isearch-search-and-update`).
    fn isearch_type(&mut self, c: char) {
        let Some((regexp, opoint, barrier, _, mut st)) = self.isearch_snapshot() else {
            return;
        };
        st.string.push(c);
        // Adding characters to a failing literal search cannot make it match, so
        // Emacs does not even re-search; a regexp can still become valid.
        if st.success || regexp {
            // Re-search from the near end of the current match, never from the
            // search origin — a typed character must not drag point backwards.
            st.point = if st.forward {
                st.other_end
            } else {
                // Backwards, the match may grow by the new character, but never
                // past the origin or the last repeat.
                self.isearch_forward_char(st.other_end)
                    .unwrap_or(st.other_end)
                    .min(opoint)
                    .min(barrier)
            };
            self.isearch_search(regexp, &mut st);
        }
        self.isearch_push(st);
    }

    /// `isearch-delete-char` (`DEL`): pop the last isearch command, restoring
    /// the pattern and point it had before. After a `C-s` this undoes the
    /// repeat and leaves the pattern alone, exactly as Emacs does.
    fn isearch_delete_char(&mut self) {
        if let Some(is) = self.input.as_mut().and_then(|inp| inp.isearch.as_mut()) {
            if is.cmds.len() > 1 {
                is.cmds.pop();
            }
        }
        self.isearch_sync();
    }

    /// `isearch-repeat-forward` / `isearch-repeat-backward` (`C-s` / `C-r`).
    fn isearch_repeat(&mut self, forward: bool) {
        let Some((regexp, _, _, just_started, mut st)) = self.isearch_snapshot() else {
            return;
        };
        if forward == st.forward {
            if st.string.is_empty() {
                // An empty pattern repeats the previous search string (Emacs
                // takes it off `search-ring` / `regexp-search-ring`).
                let ring = if regexp {
                    self.regexp_search_ring.clone()
                } else {
                    self.search_ring.clone()
                };
                match ring {
                    Some(prev) => st.string = prev,
                    // "No previous search string": nothing to repeat.
                    None => {
                        self.isearch_push(st);
                        return;
                    }
                }
            } else if !st.success {
                // `isearch-wrap-pause` is t, so the repeat that met the end of
                // the buffer only failed; this next one wraps around.
                st.wrapped = true;
                st.point = if st.forward {
                    (0, 0)
                } else {
                    self.isearch_point_max()
                };
            }
        } else {
            // Reversing direction is a command in itself: point does not move
            // (`isearch-repeat-on-direction-change` is nil), and the search
            // below re-finds the same match from the other side.
            st.forward = forward;
            st.success = true;
        }
        let barrier = st.point;
        if st.string.is_empty() {
            st.success = true;
        } else if st.success && st.point == st.other_end && !just_started {
            // Repeating on a match that was empty: step a character first so the
            // search cannot stand still, and fail if there is nowhere to step.
            let next = if st.forward {
                self.isearch_forward_char(st.point)
            } else {
                self.isearch_backward_char(st.point)
            };
            match next {
                Some(p) => {
                    st.point = p;
                    self.isearch_search(regexp, &mut st);
                }
                None => st.success = false,
            }
        } else {
            self.isearch_search(regexp, &mut st);
        }
        if let Some(is) = self.input.as_mut().and_then(|inp| inp.isearch.as_mut()) {
            is.barrier = barrier;
        }
        self.isearch_push(st);
    }

    /// `isearch-exit` (`RET`): the point the search ends at, which is where the
    /// NEXT isearch starts from. `None` for every other in-mode read — those
    /// leave no point behind.
    fn isearch_exit_point(isearch: Option<&Isearch>) -> Option<IPos> {
        isearch?.cmds.last().map(|st| st.point)
    }

    /// `isearch-abort` (`C-g`): a successful search returns point to where it
    /// started and quits; a failing one only rubs out the commands that failed
    /// and stays in the search. Returns true when the search is over.
    fn isearch_abort(&mut self) -> bool {
        let quit = self
            .input
            .as_ref()
            .and_then(|inp| inp.isearch.as_ref())
            .and_then(|is| is.cmds.last())
            .is_some_and(|st| st.success);
        if quit {
            if let Some(opoint) = self
                .input
                .as_ref()
                .and_then(|inp| inp.isearch.as_ref())
                .map(|is| is.opoint)
            {
                self.selected = opoint.0.min(self.entries.len().saturating_sub(1));
            }
            return true;
        }
        if let Some(is) = self.input.as_mut().and_then(|inp| inp.isearch.as_mut()) {
            while is.cmds.len() > 1 && !is.cmds[is.cmds.len() - 1].success {
                is.cmds.pop();
            }
        }
        self.isearch_sync();
        false
    }

    /// Emacs's echo-area prompt for the live search, including the `failing` /
    /// `wrapped` / `overwrapped` annotations and the direction.
    fn isearch_prompt(is: &Isearch) -> String {
        let Some(st) = is.cmds.last() else {
            return String::new();
        };
        let over = st.wrapped
            && if st.forward {
                st.point > is.opoint
            } else {
                st.point < is.opoint
            };
        let m = format!(
            "{}{}{}{}filename {}I-search{}: ",
            if st.success { "" } else { "failing " },
            if over { "over" } else { "" },
            if st.wrapped { "wrapped " } else { "" },
            // A failing search says so when the pattern's own case made it fail.
            if !st.success && !isearch_no_upper_case(&st.string, is.regexp) {
                "case-sensitive "
            } else {
                ""
            },
            if is.regexp { "regexp " } else { "" },
            if st.forward { "" } else { " backward" },
        );
        // Emacs upcases only the first character of the assembled prefix.
        let mut cs = m.chars();
        match cs.next() {
            Some(f) => f.to_uppercase().collect::<String>() + cs.as_str(),
            None => String::new(),
        }
    }

    /// Execute a completed minibuffer read (`text` is the typed line), refresh
    /// the listing if the filesystem changed, and report via `set_status`.
    fn run_pending(&mut self, action: Pending, text: &str, cx: &mut Context) {
        let text = text.trim();
        match action {
            Pending::CreateDir => {
                if text.is_empty() {
                    return;
                }
                let path = self.dir.join(text);
                match std::fs::create_dir_all(&path) {
                    Ok(()) => {
                        let _ = self.read_dir();
                        cx.editor
                            .set_status(format!("dired: created directory {text}"));
                    }
                    Err(e) => cx.editor.set_error(format!("mkdir {text}: {e}")),
                }
            }
            Pending::CreateFile => {
                if !is_valid_filename(text) {
                    cx.editor.set_error("dired: invalid file name");
                    return;
                }
                match std::fs::File::create(self.dir.join(text)) {
                    Ok(_) => {
                        let _ = self.read_dir();
                        cx.editor.set_status(format!("dired: created {text}"));
                    }
                    Err(e) => cx.editor.set_error(format!("create {text}: {e}")),
                }
            }
            Pending::Copy(targets) => {
                self.link_or_copy(&targets, text, LinkKind::Copy, cx);
            }
            Pending::Rename(targets) => {
                self.link_or_copy(&targets, text, LinkKind::Rename, cx);
            }
            Pending::Symlink(targets) => {
                self.link_or_copy(&targets, text, LinkKind::Symlink, cx);
            }
            Pending::Hardlink(targets) => {
                self.link_or_copy(&targets, text, LinkKind::Hardlink, cx);
            }
            Pending::Chmod(targets) => {
                let mode = match parse_octal_mode(text) {
                    Some(m) => m,
                    None => {
                        cx.editor.set_error("dired: invalid octal mode");
                        return;
                    }
                };
                let mut n = 0;
                for name in &targets {
                    let perm = std::fs::Permissions::from_mode(mode);
                    match std::fs::set_permissions(self.dir.join(name), perm) {
                        Ok(()) => n += 1,
                        Err(e) => {
                            cx.editor.set_error(format!("chmod {name}: {e}"));
                            break;
                        }
                    }
                }
                let _ = self.read_dir();
                cx.editor
                    .set_status(format!("dired: chmod {text} on {n} file(s)"));
            }
            Pending::MarkRegexp | Pending::FlagRegexp => {
                if text.is_empty() {
                    return;
                }
                let re = match Regex::new(text) {
                    Ok(re) => re,
                    Err(e) => {
                        cx.editor.set_error(format!("dired: bad regexp: {e}"));
                        return;
                    }
                };
                let flag = matches!(action, Pending::FlagRegexp);
                let names: Vec<String> = self
                    .entries
                    .iter()
                    .filter(|e| re.is_match(&e.name))
                    .map(|e| e.name.clone())
                    .collect();
                let mut n = 0;
                for name in names {
                    let inserted = if flag {
                        self.flagged.insert(name)
                    } else {
                        self.marked.insert(name)
                    };
                    if inserted {
                        n += 1;
                    }
                }
                let what = if flag { "flagged" } else { "marked" };
                cx.editor
                    .set_status(format!("dired: {what} {n} file(s) matching /{text}/"));
            }
            Pending::GotoFile => {
                if let Some(i) = self
                    .entries
                    .iter()
                    .position(|e| e.name == text)
                    .or_else(|| self.entries.iter().position(|e| e.name.starts_with(text)))
                {
                    self.selected = i;
                } else {
                    cx.editor.set_status(format!("dired: no file named {text}"));
                }
            }
            Pending::ChangeMarks => {
                // Read "OLD NEW" — the first non-space char is the old mark, the
                // second the new one (space clears). Emacs prompts for each char.
                let mut chars = text.chars().filter(|c| !c.is_whitespace());
                match (chars.next(), text.chars().last()) {
                    (Some(old), _) => {
                        // NEW is the last typed char; if only one char given, use space.
                        let new = if text.chars().filter(|c| !c.is_whitespace()).count() >= 2 {
                            text.chars().filter(|c| !c.is_whitespace()).nth(1).unwrap()
                        } else {
                            ' '
                        };
                        let n = self.change_marks(old, new);
                        cx.editor
                            .set_status(format!("dired: changed {n} mark(s) {old} -> {new}"));
                    }
                    _ => cx.editor.set_status("dired: change-marks needs OLD NEW"),
                }
            }
            Pending::MarkContaining => {
                if text.is_empty() {
                    return;
                }
                match Regex::new(text) {
                    Ok(re) => {
                        let n = self.mark_containing(&re);
                        cx.editor
                            .set_status(format!("dired: marked {n} file(s) containing /{text}/"));
                    }
                    Err(e) => cx.editor.set_error(format!("dired: bad regexp: {e}")),
                }
            }
            // RET ends an isearch where it stands (Emacs `isearch-exit`): point
            // already tracked the pattern as it was typed, so all that is left
            // is to push the pattern onto the ring `isearch-done` updates.
            Pending::IsearchFilenames { regexp } => {
                if !text.is_empty() {
                    if regexp {
                        self.regexp_search_ring = Some(text.to_string());
                    } else {
                        self.search_ring = Some(text.to_string());
                    }
                    cx.editor
                        .set_status(format!("dired: isearch filename {text}"));
                }
            }
            Pending::RelSymlink(targets) => {
                self.relsymlink_targets(&targets, text, cx);
            }
            Pending::Chgrp(targets) => {
                if self.run_external("chgrp", &[text], &targets, cx) {
                    cx.editor
                        .set_status(format!("dired: chgrp {text} on {} file(s)", targets.len()));
                }
            }
            Pending::Chown(targets) => {
                if self.run_external("chown", &[text], &targets, cx) {
                    cx.editor
                        .set_status(format!("dired: chown {text} on {} file(s)", targets.len()));
                }
            }
            Pending::CompressTo(targets) => {
                if text.is_empty() {
                    return;
                }
                // tar+gzip archive of the targets, named as typed.
                let mut args: Vec<&str> = vec!["-czf", text];
                args.extend(targets.iter().map(String::as_str));
                if self.run_external("tar", &args, &[], cx) {
                    cx.editor.set_status(format!(
                        "dired: archived {} file(s) to {text}",
                        targets.len()
                    ));
                }
            }
            Pending::ShellCommand(targets) => {
                if text.is_empty() {
                    return;
                }
                // `!`: run `command file1 file2 ...` in the directory, with the
                // single target on stdin, and show what it wrote.
                self.run_shell(text, &targets, cx);
            }
            Pending::AsyncShellCommand(targets) => {
                if text.is_empty() {
                    return;
                }
                // Emacs `&`: the same line, run in the background.
                self.run_shell_async(text, &targets, cx);
            }
            Pending::Diff(other) => {
                self.run_diff(&other, text, cx);
            }
            Pending::CompareDir => {
                if text.is_empty() {
                    return;
                }
                let other = if Path::new(text).is_absolute() {
                    PathBuf::from(text)
                } else {
                    self.dir.join(text)
                };
                match read_entries(&other, self.show_hidden) {
                    Ok(there) => {
                        let names = dirs_differ(&self.entries, &there);
                        let mut n = 0;
                        for name in names {
                            if self.marked.insert(name) {
                                n += 1;
                            }
                        }
                        cx.editor.set_status(format!(
                            "dired: marked {n} file(s) differing from {}",
                            other.display()
                        ));
                    }
                    Err(e) => cx
                        .editor
                        .set_error(format!("compare-directories {}: {e}", other.display())),
                }
            }
            Pending::FindRegexp { targets, regexp } => {
                if text.is_empty() {
                    return;
                }
                self.run_find_regexp(text, regexp, &targets, cx);
            }
            Pending::RegexpOpPattern(kind, targets) => {
                if text.is_empty() {
                    return;
                }
                match Regex::new(text) {
                    Ok(_) => self.begin_input(
                        kind.replace_prompt(),
                        Pending::RegexpOpReplace(kind, targets, text.to_string()),
                    ),
                    Err(e) => cx.editor.set_error(format!("dired: bad regexp: {e}")),
                }
            }
            Pending::RegexpOpReplace(kind, targets, pattern) => {
                // `pattern` was validated in the first leg.
                if let Ok(re) = Regex::new(&pattern) {
                    self.run_regexp_op(kind, &targets, &re, text, cx);
                }
            }
            Pending::GotoSubdir => {
                if !text.is_empty() {
                    self.goto_named_subdir(text, cx);
                }
            }
            // `find-dired` takes the ARGS verbatim — emacs reads them with
            // `read-string "Run find (with args): "` and pastes them into the
            // command, so anything find understands goes.
            Pending::FindDir => {
                // An empty answer is emacs's default — the directory Dired is on.
                let dir = if text.trim().is_empty() {
                    self.dir.clone()
                } else {
                    zmax_stdx::path::expand_tilde(std::path::Path::new(text.trim())).into_owned()
                };
                if !dir.is_dir() {
                    cx.editor
                        .set_error(format!("find-dired: {} is not a directory", dir.display()));
                    return;
                }
                self.begin_input("Run find (with args): ", Pending::FindArgs(dir));
            }
            Pending::FindArgs(dir) => self.run_find_args(&dir, text, cx),
            Pending::FindName => {
                if !text.is_empty() {
                    self.run_find(&["-name", text], "find-name", cx);
                }
            }
            Pending::FindGrep => {
                if !text.is_empty() {
                    self.run_find(
                        &["-type", "f", "-exec", "grep", "-lE", text, "{}", ";"],
                        "find-grep",
                        cx,
                    );
                }
            }
            Pending::EpaEncrypt(targets) => {
                if text.is_empty() {
                    return;
                }
                let mut n = 0;
                for name in &targets {
                    if self.run_external(
                        "gpg",
                        &["--yes", "-e", "-r", text],
                        std::slice::from_ref(name),
                        cx,
                    ) {
                        n += 1;
                    }
                }
                let _ = self.read_dir();
                cx.editor
                    .set_status(format!("dired: encrypted {n} file(s) to {text}"));
            }
            Pending::Locate => {
                if !text.is_empty() {
                    self.run_locate(text, cx);
                }
            }
            Pending::FindReplacePattern(targets) => {
                if text.is_empty() {
                    return;
                }
                match Regex::new(text) {
                    Ok(_) => self.begin_input(
                        "Replace with: ",
                        Pending::FindReplaceWith(targets, text.to_string()),
                    ),
                    Err(e) => cx.editor.set_error(format!("dired: bad regexp: {e}")),
                }
            }
            Pending::FindReplaceWith(targets, pattern) => {
                if let Ok(re) = Regex::new(&pattern) {
                    self.run_find_replace(&targets, &re, text, cx);
                }
            }
            Pending::ImageComment(targets) => {
                for name in &targets {
                    let abs = self.dir.join(name);
                    set_image_meta(&abs.to_string_lossy(), Some(text), None);
                }
                cx.editor
                    .set_status(format!("dired: set comment on {} image(s)", targets.len()));
            }
            Pending::ImageCommentTags(name) => {
                // Input is `comment;tags`.
                let (comment, tags) = text.split_once(';').unwrap_or((text, ""));
                let abs = self.dir.join(&name);
                set_image_meta(
                    &abs.to_string_lossy(),
                    Some(comment.trim()),
                    Some(tags.trim()),
                );
                cx.editor
                    .set_status(format!("dired: set comment/tags on {name}"));
            }
        }
    }

    /// Run `prog arg... name...` in the Dired directory, then refresh. Used by
    /// the external-tool file ops (`chgrp`/`chown`/`tar`). Reports success or the
    /// tool's stderr.
    fn run_external(
        &mut self,
        prog: &str,
        args: &[&str],
        names: &[String],
        cx: &mut Context,
    ) -> bool {
        let mut cmd = std::process::Command::new(prog);
        cmd.current_dir(&self.dir).args(args);
        for n in names {
            cmd.arg(n);
        }
        match cmd.output() {
            Ok(out) if out.status.success() => {
                let _ = self.read_dir();
                true
            }
            Ok(out) => {
                let err = String::from_utf8_lossy(&out.stderr);
                cx.editor
                    .set_error(format!("{prog}: {}", err.trim().replace('\n', "; ")));
                let _ = self.read_dir();
                false
            }
            Err(e) => {
                cx.editor.set_error(format!("{prog}: {e}"));
                false
            }
        }
    }

    /// `dired-do-shell-command` (`!`) / mg `dired-shell-command`.
    ///
    /// Emacs appends the shell-quoted target names to the command line
    /// (dired-aux.el `dired-shell-stuff-it`); mg instead pipes the file at point
    /// in on standard input (`d_exec(0, bp, fname, "sh", "-c", command, NULL)`,
    /// dired.c:630, whose child does `dup2(infd, STDIN_FILENO)`). Both are done
    /// here: the names are appended, and when there is exactly one target — mg's
    /// only case, since mg's Dired has no marks — that file is also the child's
    /// stdin, so a command written either way works.
    ///
    /// mg gives the child a single pipe for stdout *and* stderr (dired.c:711-712)
    /// and puts everything it reads into `*Shell Command Output*`; Emacs displays
    /// the same buffer, and when the command wrote nothing says so in the echo
    /// area instead (simple.el `shell-command`). Both are ported: output opens a
    /// buffer under the overlay, silence leaves the listing up with the message.
    fn run_shell(&mut self, command: &str, names: &[String], cx: &mut Context) {
        let quoted: String = names
            .iter()
            .map(|n| format!(" {}", shell_quote(n)))
            .collect();
        let full = format!("{command}{quoted}");
        let mut cmd = std::process::Command::new("/bin/sh");
        cmd.arg("-c").arg(&full).current_dir(&self.dir);
        // mg's `d_exec` opens the input file itself and reports the failure
        // rather than running the command on the wrong input.
        match names {
            [only] => match std::fs::File::open(self.dir.join(only)) {
                Ok(f) => {
                    cmd.stdin(f);
                }
                Err(e) => {
                    cx.editor
                        .set_error(format!("dired: {only}: can't open input file: {e}"));
                    return;
                }
            },
            // `d_exec` reads /dev/null when it has no input file.
            _ => {
                cmd.stdin(std::process::Stdio::null());
            }
        }
        let out = match merged_output(cmd) {
            Ok(out) => out,
            Err(e) => {
                cx.editor.set_error(format!("shell: {e}"));
                return;
            }
        };
        let _ = self.read_dir();
        if out.text.is_empty() {
            self.message = Some(match out.status {
                Some(0) => "(Shell command succeeded with no output)".to_string(),
                Some(code) => {
                    format!("(Shell command failed with code {code} and no output)")
                }
                None => "(Shell command failed with error)".to_string(),
            });
            return;
        }
        crate::commands::show_text_in_scratch(cx.editor, &out.text);
        self.close_requested = true;
    }

    /// `dired-do-async-shell-command` (`&`): the same command line as `!`, but
    /// spawned detached — Dired does not wait for it, so a long-running command
    /// leaves the listing responsive. The listing is re-read immediately (the
    /// command's own effects show up on the next `g`).
    fn run_shell_async(&mut self, command: &str, names: &[String], cx: &mut Context) {
        let quoted: String = names
            .iter()
            .map(|n| format!(" {}", shell_quote(n)))
            .collect();
        let full = format!("{command}{quoted}");
        match std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg(&full)
            .current_dir(&self.dir)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            Ok(child) => cx.editor.set_status(format!(
                "dired: `{command}` on {} file(s) running async (pid {})",
                names.len(),
                child.id()
            )),
            Err(e) => cx.editor.set_error(format!("shell: {e}")),
        }
    }

    /// `browse-url-of-dired-file` (`W`): hand the file at point to the system
    /// browser as a `file://` URL (Emacs calls `browse-url` on exactly that URL).
    /// `$BROWSER` wins when set, as it does for `browse-url-generic`.
    fn browse_url(&mut self, cx: &mut Context) {
        let Some(name) = self.current_name() else {
            return;
        };
        let url = format!("file://{}", self.dir.join(&name).display());
        let browser = std::env::var("BROWSER").ok();
        let program = match browser.as_deref() {
            Some(b) if !b.is_empty() => b,
            _ if cfg!(target_os = "macos") => "open",
            _ => "xdg-open",
        };
        match std::process::Command::new(program)
            .arg(&url)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            Ok(_) => cx.editor.set_status(format!("dired: browsing {url}")),
            Err(e) => cx.editor.set_error(format!("{program} {url}: {e}")),
        }
    }

    /// `dired-do-info` (`I`): run info on the file at point, the same way Emacs
    /// hands it to Info mode. The rendering is done by the system `info(1)` —
    /// the binary already behind `info_search` — so the file opens at its `Top`
    /// node and the usual gzipped `.info.gz` form decompresses transparently.
    /// The node text lands in a scratch buffer; node-to-node navigation
    /// (`n`/`p`/`u`/`l`) still needs an Info mode zmax does not have.
    fn dired_do_info(&mut self, cx: &mut Context) {
        let Some(name) = self.current_name() else {
            return;
        };
        let path = self.dir.join(&name);
        match std::process::Command::new("info")
            .arg("--file")
            .arg(&path)
            .arg("--output")
            .arg("-")
            .output()
        {
            Ok(out) if out.status.success() && !out.stdout.is_empty() => {
                let text = String::from_utf8_lossy(&out.stdout);
                crate::commands::show_text_in_scratch(cx.editor, &text);
                self.close_requested = true;
            }
            // `info` rendered nothing: the file has no `Top` node, which is what
            // Emacs reports as "No such node or anchor".
            Ok(_) => cx
                .editor
                .set_error(format!("info {name}: no such node or anchor: Top")),
            // No `info(1)` on this box — fall back to the file's own bytes with
            // the node separators (`\x1f`) turned into visible rules.
            Err(_) => match std::fs::read(&path) {
                Ok(bytes) => {
                    let text = String::from_utf8_lossy(&bytes);
                    let shown: String = text
                        .lines()
                        .map(|l| l.replace('\u{1f}', "────────").replace('\u{c}', ""))
                        .collect::<Vec<_>>()
                        .join("\n");
                    crate::commands::show_text_in_scratch(cx.editor, &shown);
                    self.close_requested = true;
                }
                Err(e) => cx.editor.set_error(format!("info {name}: {e}")),
            },
        }
    }

    /// `dired-unmark-all-files` (`M-DEL`, `* ?`): remove one mark character from
    /// every file — `*` drops the `*` marks, `D` drops the deletion flags, and
    /// `RET` (or `\n`) drops every mark. Returns how many files were unmarked.
    fn unmark_all_files(&mut self, mark: char) -> usize {
        match mark {
            '*' => {
                let n = self.marked.len();
                self.marked.clear();
                n
            }
            'D' => {
                let n = self.flagged.len();
                self.flagged.clear();
                n
            }
            '\n' => {
                let n = self.marked.len() + self.flagged.len();
                self.marked.clear();
                self.flagged.clear();
                n
            }
            // Dired only ever writes `*` and `D` in the mark column, so any other
            // character matches nothing.
            _ => 0,
        }
    }

    /// `dired-diff`: run `diff -u OTHER FILE` and show the result in a scratch
    /// buffer beneath the overlay (which is then closed to reveal it).
    fn run_diff(&mut self, file: &str, other: &str, cx: &mut Context) {
        if other.is_empty() {
            return;
        }
        let a = if Path::new(other).is_absolute() {
            PathBuf::from(other)
        } else {
            self.dir.join(other)
        };
        let b = self.dir.join(file);
        match std::process::Command::new("diff")
            .arg("-u")
            .arg(&a)
            .arg(&b)
            .output()
        {
            Ok(out) => {
                let body = String::from_utf8_lossy(&out.stdout);
                let content = if body.trim().is_empty() {
                    format!(
                        "diff -u {} {}\n(no differences)\n",
                        a.display(),
                        b.display()
                    )
                } else {
                    format!("diff -u {} {}\n{body}", a.display(), b.display())
                };
                crate::commands::show_text_in_scratch(cx.editor, &content);
                self.close_requested = true;
            }
            Err(e) => cx.editor.set_error(format!("diff: {e}")),
        }
    }

    /// Open the read behind `M-s a C-s` / `M-s a C-M-s`. The two differ only in
    /// how the answer is matched, and Emacs says so in the prompt: plain
    /// "Search marked files: " for the literal `dired-do-isearch`, the "(regexp)"
    /// suffix for `dired-do-isearch-regexp`.
    fn begin_contents_search(&mut self, regexp: bool) {
        let targets = self.targets();
        let prompt = if regexp {
            "Search marked files (regexp): "
        } else {
            "Search marked files: "
        };
        self.begin_input(prompt, Pending::FindRegexp { targets, regexp });
    }

    /// `dired-do-find-regexp` / `dired-do-isearch`: grep the targets for
    /// `pattern`, show the hits in a scratch buffer (overlay closes to reveal
    /// it). `regexp` false searches for the pattern LITERALLY.
    fn run_find_regexp(&mut self, pattern: &str, regexp: bool, names: &[String], cx: &mut Context) {
        let mut cmd = std::process::Command::new("grep");
        cmd.current_dir(&self.dir).args(find_grep_flags(regexp));
        cmd.args(["-e", pattern]);
        for n in names {
            cmd.arg(n);
        }
        match cmd.output() {
            Ok(out) => {
                let body = String::from_utf8_lossy(&out.stdout);
                // Emacs shows the literal search quoted and the regexp one in
                // slashes, the way `isearch` and `re-search-forward` echo them.
                let shown = if regexp {
                    format!("find-regexp /{pattern}/")
                } else {
                    format!("find-literal \"{pattern}\"")
                };
                let content = if body.trim().is_empty() {
                    format!("{shown}\n(no matches)\n")
                } else {
                    format!("{shown}\n{body}")
                };
                crate::commands::show_text_in_scratch(cx.editor, &content);
                self.close_requested = true;
            }
            Err(e) => cx.editor.set_error(format!("grep: {e}")),
        }
    }

    /// `dired-do-compress`: gzip each target, or gunzip it when already a `.gz`
    /// (Emacs toggles compression). Refreshes the listing afterwards.
    fn compress_targets(&mut self, cx: &mut Context) {
        let targets = self.targets();
        let mut n = 0;
        for name in &targets {
            let (prog, arg) = if name.ends_with(".gz") {
                ("gunzip", name.as_str())
            } else {
                ("gzip", name.as_str())
            };
            match std::process::Command::new(prog)
                .arg(arg)
                .current_dir(&self.dir)
                .status()
            {
                Ok(s) if s.success() => n += 1,
                Ok(_) => {}
                Err(e) => {
                    cx.editor.set_error(format!("{prog} {name}: {e}"));
                    break;
                }
            }
        }
        let _ = self.read_dir();
        cx.editor
            .set_status(format!("dired: (un)compressed {n} file(s)"));
    }

    /// `dired-do-open`: hand each target to the system opener (`open` on macOS,
    /// `xdg-open` elsewhere), detached, so the OS default app handles it.
    fn open_targets(&mut self, cx: &mut Context) {
        let opener = if cfg!(target_os = "macos") {
            "open"
        } else {
            "xdg-open"
        };
        let targets = self.targets();
        let mut n = 0;
        for name in &targets {
            match std::process::Command::new(opener)
                .arg(self.dir.join(name))
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
            {
                Ok(_) => n += 1,
                Err(e) => {
                    cx.editor.set_error(format!("{opener} {name}: {e}"));
                    break;
                }
            }
        }
        cx.editor
            .set_status(format!("dired: opened {n} file(s) externally"));
    }

    /// Emacs `dired-do-load` (`L`): evaluate each marked Emacs-Lisp file through
    /// zmax's embedded elisp (`elisprs`), loading its definitions. Stops at the
    /// first read/eval error.
    fn dired_do_load(&mut self, cx: &mut Context) {
        let targets = self.targets();
        let mut loaded = 0;
        for name in &targets {
            let path = self.dir.join(name);
            let src = match std::fs::read_to_string(&path) {
                Ok(s) => s,
                Err(e) => {
                    cx.editor.set_error(format!("{name}: {e}"));
                    break;
                }
            };
            match crate::commands::scripting::eval_elisp(cx, &src) {
                Ok(_) => loaded += 1,
                Err(e) => {
                    cx.editor.set_error(format!("load {name}: {e}"));
                    break;
                }
            }
        }
        cx.editor
            .set_status(format!("dired: loaded {loaded} elisp file(s)"));
    }

    /// Emacs `dired-do-byte-compile` (`B`): zmax's elisp (`elisprs`) is an
    /// interpreter with no `.elc` output, so "byte-compiling" here evaluates each
    /// marked file to *validate* it compiles/loads cleanly, reporting the first
    /// error. Tracked as a partial port (no bytecode file is produced).
    fn dired_byte_compile(&mut self, cx: &mut Context) {
        let targets = self.targets();
        let mut ok = 0;
        for name in &targets {
            let path = self.dir.join(name);
            let src = match std::fs::read_to_string(&path) {
                Ok(s) => s,
                Err(e) => {
                    cx.editor.set_error(format!("{name}: {e}"));
                    break;
                }
            };
            match crate::commands::scripting::eval_elisp(cx, &src) {
                Ok(_) => ok += 1,
                Err(e) => {
                    cx.editor.set_error(format!("compile {name}: {e}"));
                    break;
                }
            }
        }
        cx.editor.set_status(format!(
            "dired: checked {ok} elisp file(s) (interpreted, no .elc)"
        ));
    }

    /// Emacs `dired-next-subdir`/`dired-prev-subdir`: move point to the first
    /// entry of the next (`forward`) or previous inserted subdir section. The top
    /// directory counts as the first section.
    fn goto_subdir(&mut self, forward: bool) {
        if self.entries.is_empty() {
            return;
        }
        // Section start indices — where the `reldir/` prefix changes.
        let mut starts = Vec::new();
        let mut prev: Option<Option<String>> = None;
        for (i, e) in self.entries.iter().enumerate() {
            let sd = Self::entry_subdir(&e.name).map(str::to_string);
            if prev.as_ref() != Some(&sd) {
                starts.push(i);
                prev = Some(sd);
            }
        }
        let cur = starts
            .iter()
            .rposition(|&s| s <= self.selected)
            .unwrap_or(0);
        let target = if forward {
            match starts.get(cur + 1) {
                Some(&i) => i,
                None => return,
            }
        } else if cur > 0 {
            starts[cur - 1]
        } else {
            return;
        };
        self.selected = target;
    }

    /// Emacs `dired-tree-up`/`dired-tree-down`: with a flat section model, move to
    /// the top directory section (`up`) or into the first inserted subdir section
    /// (`down`).
    fn tree_move(&mut self, up: bool) {
        if up {
            self.selected = 0;
        } else if let Some(i) = self
            .entries
            .iter()
            .position(|e| Self::entry_subdir(&e.name).is_some())
        {
            self.selected = i;
        }
    }

    /// Emacs `dired-goto-subdir`: jump to the first entry of the inserted subdir
    /// section whose relative path is `reldir`.
    fn goto_named_subdir(&mut self, reldir: &str, cx: &mut Context) {
        let prefix = format!("{}/", reldir.trim_end_matches('/'));
        match self
            .entries
            .iter()
            .position(|e| e.name.starts_with(&prefix))
        {
            Some(i) => self.selected = i,
            None => cx
                .editor
                .set_error(format!("dired: subdir '{reldir}' not inserted")),
        }
    }

    /// Emacs `dired-do-man`: show the man page for the file at point in a scratch
    /// buffer (`man -l` renders a file as a man page; falls back to `man`).
    fn dired_do_man(&mut self, cx: &mut Context) {
        let Some(name) = self.current_name() else {
            return;
        };
        let out = std::process::Command::new("man")
            .arg("-l")
            .arg(&name)
            .current_dir(&self.dir)
            .output()
            .or_else(|_| {
                std::process::Command::new("man")
                    .arg(&name)
                    .current_dir(&self.dir)
                    .output()
            });
        match out {
            Ok(o) if o.status.success() && !o.stdout.is_empty() => {
                let text = String::from_utf8_lossy(&o.stdout);
                // Strip overstrike bolding (`x\bx`) that `man` emits for a pager.
                let clean: String = strip_man_overstrike(&text);
                crate::commands::show_text_in_scratch(cx.editor, &clean);
                self.close_requested = true;
            }
            Ok(o) => cx.editor.set_error(format!(
                "man {name}: {}",
                String::from_utf8_lossy(&o.stderr).trim()
            )),
            Err(e) => cx.editor.set_error(format!("man: {e}")),
        }
    }

    /// Emacs `dired-do-print`: send the marked files to the printer via `lpr`
    /// (falling back to `lp`).
    fn dired_do_print(&mut self, cx: &mut Context) {
        let targets = self.targets();
        if targets.is_empty() {
            return;
        }
        let ran = self.run_external("lpr", &[], &targets, cx)
            || self.run_external("lp", &[], &targets, cx);
        if ran {
            cx.editor
                .set_status(format!("dired: printed {} file(s)", targets.len()));
        }
    }

    /// Emacs `dired-other-tab`: open the file at point in a new tabpage.
    fn open_other_tab(&mut self) -> Option<Callback> {
        if self.entries.get(self.selected)?.is_dir {
            self.visit();
            return None;
        }
        let name = self.current_name()?;
        let path = self.dir.join(&name);
        Some(Box::new(
            move |compositor: &mut Compositor, cx: &mut Context| {
                compositor.pop();
                match cx.editor.open(&path, Action::Load) {
                    Ok(id) => cx.editor.new_tab_with_doc(id),
                    Err(err) => cx
                        .editor
                        .set_error(format!("failed to open {}: {err}", path.display())),
                }
            },
        ))
    }

    /// `find-dired` (and `find-name-dired` / `find-grep-dired`, which are the
    /// same command with the ARGS built for you): run `find . <args>` under the
    /// Dired directory and list what it prints AS THE DIRED LISTING.
    ///
    /// Emacs inserts the find output into a Dired buffer precisely so the result
    /// is operable — mark it, delete it, visit it, `!` a command over it — which
    /// a text dump of the paths is not.
    fn run_find(&mut self, args: &[&str], label: &str, cx: &mut Context) {
        let out = std::process::Command::new("find")
            .arg(".")
            .args(args)
            .current_dir(&self.dir)
            .output();
        self.list_find_output(out, label, cx);
    }

    /// `find-dired` proper: ARGS as the user typed them. Emacs builds
    /// `find . \( ARGS \) -ls` and hands the whole line to
    /// `start-file-process-shell-command` (find-dired.el:194), so ARGS is
    /// SHELL-parsed — quoting, globs and `-o` alternations mean what the shell
    /// and find say they mean, which is the whole point of the command. Empty
    /// ARGS lists the tree, as emacs's empty-string case does (no parens).
    /// zmax stats the hits itself, so it asks find for the plain path list
    /// rather than for `-ls` output it would have to parse back.
    fn run_find_args(&mut self, dir: &std::path::Path, args: &str, cx: &mut Context) {
        let args = args.trim();
        let command = if args.is_empty() {
            "find .".to_string()
        } else {
            format!("find . \\( {args} \\)")
        };
        let out = std::process::Command::new("sh")
            .arg("-c")
            .arg(&command)
            .current_dir(dir)
            .output();
        // Emacs's find-dired buffer is Dired ON THE DIRECTORY IT SEARCHED, so a
        // find run somewhere else moves the listing there before showing the hits.
        if dir != self.dir {
            self.dir = dir.to_path_buf();
        }
        self.list_find_output(out, "find", cx);
    }

    /// Turn the path list a find/grep run printed into the Dired listing.
    fn list_find_output(
        &mut self,
        out: std::io::Result<std::process::Output>,
        label: &str,
        cx: &mut Context,
    ) {
        match out {
            Ok(o) => {
                let paths = find_paths(&String::from_utf8_lossy(&o.stdout));
                if paths.is_empty() {
                    cx.editor
                        .set_status(format!("{label}: no matches under {}", self.dir.display()));
                    return;
                }
                let n = paths.len();
                self.enter_found(paths);
                cx.editor
                    .set_status(format!("{label}: {n} file(s) under {}", self.dir.display()));
            }
            Err(e) => cx.editor.set_error(format!("find: {e}")),
        }
    }

    /// Turn the buffer into a result listing over `paths` (named relative to
    /// `dir`) — what emacs gets by inserting a command's output into a Dired
    /// buffer. Marks and inserted subdirectories describe the directory listing,
    /// so they do not carry over.
    fn enter_found(&mut self, paths: Vec<String>) {
        self.found = Some(paths);
        self.marked.clear();
        self.flagged.clear();
        self.subdirs.clear();
        self.hidden_subdirs.clear();
        self.selected = 0;
        self.scroll = 0;
        self.undo_snapshot = None;
        if let Err(err) = self.read_dir() {
            self.error = Some(format!("{err}"));
        }
    }

    /// Emacs `epa-dired-do-decrypt`/`-sign`/`-verify`: run gpg on each target. gpg
    /// uses its agent for any private-key passphrase; `verify` needs none.
    fn epa_run(&mut self, gpg_args: &[&str], label: &str, cx: &mut Context) {
        let targets = self.targets();
        if targets.is_empty() {
            return;
        }
        let mut n = 0;
        for name in &targets {
            if self.run_external("gpg", gpg_args, std::slice::from_ref(name), cx) {
                n += 1;
            }
        }
        let _ = self.read_dir();
        cx.editor.set_status(format!("dired: {label} {n} file(s)"));
    }

    /// Emacs `locate` (with the current directory as an implicit filter): run
    /// `locate PATTERN`, keep the hits under this directory and list them as the
    /// Dired listing — `locate-mode` derives from Dired for the same reason
    /// find-dired does, so the hits are operable rows rather than text.
    fn run_locate(&mut self, pattern: &str, cx: &mut Context) {
        match std::process::Command::new("locate").arg(pattern).output() {
            Ok(o) => {
                let dir = self.dir.to_string_lossy().into_owned();
                let prefix = format!("{}/", dir.trim_end_matches('/'));
                let body = String::from_utf8_lossy(&o.stdout);
                // The rows are named relative to this directory, so a hit that is
                // merely *mentioned* by an unrelated path is not one of ours.
                let paths: Vec<String> = body
                    .lines()
                    .filter_map(|l| l.trim().strip_prefix(&prefix))
                    .filter(|l| !l.is_empty())
                    .map(str::to_string)
                    .collect();
                if paths.is_empty() {
                    cx.editor
                        .set_status(format!("locate {pattern}: no matches under {dir}"));
                    return;
                }
                let n = paths.len();
                self.enter_found(paths);
                cx.editor
                    .set_status(format!("locate {pattern}: {n} file(s) under {dir}"));
            }
            Err(e) => cx.editor.set_error(format!("locate: {e}")),
        }
    }

    /// Emacs `dired-do-find-regexp-and-replace`: replace every match of `re` with
    /// `replacement` in each target file (a non-interactive bulk replace). Reports
    /// how many files changed.
    fn run_find_replace(
        &mut self,
        targets: &[String],
        re: &Regex,
        replacement: &str,
        cx: &mut Context,
    ) {
        let mut changed = 0;
        for name in targets {
            let path = self.dir.join(name);
            let Ok(src) = std::fs::read_to_string(&path) else {
                continue;
            };
            let new = re.replace_all(&src, replacement);
            if new != src {
                if let Err(e) = std::fs::write(&path, new.as_ref()) {
                    cx.editor.set_error(format!("{name}: {e}"));
                    break;
                }
                changed += 1;
            }
        }
        let _ = self.read_dir();
        cx.editor
            .set_status(format!("dired: replaced in {changed} file(s)"));
    }

    /// Emacs `wdired-change-to-wdired-mode` (`C-x C-q` in Dired): dump the current
    /// top-level file names into an editable scratch buffer and record the session.
    /// The user edits the names, then runs `:wdired-finish-edit` to apply renames.
    /// (Inserted subdir sections are excluded — only the top directory is editable.)
    fn wdired_change(&mut self, cx: &mut Context) {
        let names: Vec<String> = self
            .entries
            .iter()
            .filter(|e| !e.name.contains('/'))
            .map(|e| e.name.clone())
            .collect();
        let listing = format!("{}\n", names.join("\n"));
        *WDIRED_SESSION.lock().unwrap() = Some((self.dir.clone(), names));
        crate::commands::show_text_in_scratch(cx.editor, &listing);
        self.close_requested = true;
        cx.editor
            .set_status("wdired: edit names, then run :wdired-finish-edit");
    }

    /// Emacs `image-dired-dired-display-external`: open the image at point in the
    /// OS's default external viewer.
    fn image_display_external(&mut self, cx: &mut Context) {
        let Some(name) = self.current_name() else {
            return;
        };
        let opener = if cfg!(target_os = "macos") {
            "open"
        } else {
            "xdg-open"
        };
        match std::process::Command::new(opener)
            .arg(self.dir.join(&name))
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            Ok(_) => cx
                .editor
                .set_status(format!("dired: displaying {name} externally")),
            Err(e) => cx.editor.set_error(format!("{opener}: {e}")),
        }
    }

    /// Emacs image-dired inline display (`display-image`/`display-this`/
    /// `display-thumbs`): show the marked images (or the one at point) in the
    /// terminal with the first available image viewer, handing the terminal over
    /// so the graphics render, then returning on Enter. Requires a terminal that
    /// displays images plus one of chafa/kitty-icat/imgcat/viu/timg/catimg.
    fn image_display_inline(&mut self, cx: &mut Context) {
        let paths: Vec<PathBuf> = self.targets().iter().map(|n| self.dir.join(n)).collect();
        if paths.is_empty() {
            return;
        }
        crate::commands::display_images_in_terminal(cx.editor, &paths, 0, false, false, 100);
        cx.editor
            .set_status(format!("dired: displaying {} image(s)", paths.len()));
    }

    /// Shared body for copy/rename/symlink/hardlink over a set of targets to a
    /// user-typed destination, refreshing and reporting the result.
    fn link_or_copy(&mut self, targets: &[String], dest: &str, kind: LinkKind, cx: &mut Context) {
        if dest.is_empty() || targets.is_empty() {
            return;
        }
        let dest_path = self.dir.join(dest);
        let dest_is_dir = dest_path.is_dir();
        let mut n = 0;
        for name in targets {
            let src = self.dir.join(name);
            let to = destination_path(&dest_path, dest_is_dir, name);
            let res = match kind {
                LinkKind::Copy => std::fs::copy(&src, &to).map(|_| ()),
                LinkKind::Rename => std::fs::rename(&src, &to),
                LinkKind::Symlink => std::os::unix::fs::symlink(&src, &to),
                LinkKind::Hardlink => std::fs::hard_link(&src, &to),
            };
            match res {
                Ok(()) => n += 1,
                Err(e) => {
                    cx.editor.set_error(format!("{} {name}: {e}", kind.verb()));
                    break;
                }
            }
        }
        let _ = self.read_dir();
        cx.editor
            .set_status(format!("dired: {} {n} file(s)", kind.past()));
    }

    /// Visit the entry at point: enter a subdirectory in place, or open a file
    /// (popping this overlay).
    fn visit(&mut self) -> Option<Callback> {
        let e = self.entries.get(self.selected)?;
        let path = self.dir.join(&e.name);
        if e.is_dir {
            self.dir = std::fs::canonicalize(&path).unwrap_or(path);
            self.selected = 0;
            self.scroll = 0;
            self.marked.clear();
            self.flagged.clear();
            // Entering a directory leaves any find/locate result listing behind:
            // its paths were relative to the directory we came from.
            self.found = None;
            if let Err(err) = self.read_dir() {
                self.error = Some(format!("{err}"));
            }
            None
        } else {
            Some(Box::new(
                move |compositor: &mut Compositor, cx: &mut Context| {
                    compositor.pop();
                    if let Err(err) = cx.editor.open(&path, Action::Replace) {
                        cx.editor
                            .set_error(format!("failed to open {}: {err}", path.display()));
                    }
                },
            ))
        }
    }

    /// Go up to the parent directory, selecting the directory we came from.
    fn up_dir(&mut self) {
        if let Some(parent) = self.dir.parent().map(|p| p.to_path_buf()) {
            let from = self
                .dir
                .file_name()
                .map(|n| n.to_string_lossy().into_owned());
            self.dir = parent;
            self.marked.clear();
            self.flagged.clear();
            self.selected = 0;
            self.scroll = 0;
            // As in `visit`: a result listing does not survive leaving its
            // directory, since its rows are named relative to it.
            self.found = None;
            if let Err(err) = self.read_dir() {
                self.error = Some(format!("{err}"));
            }
            if let Some(from) = from {
                if let Some(i) = self.entries.iter().position(|e| e.name == from) {
                    self.selected = i;
                }
            }
        }
    }
}

impl Dired {
    /// Complete a prefix chord: `self.prefix` was armed by `%`/`*`/`:`/`C-t`/`M-s`
    /// (or `M-DEL`) and `key` is the next key. Unbound keys in a prefix map are
    /// dropped, exactly as Emacs drops an undefined chord.
    fn handle_prefix(&mut self, prefix: Prefix, key: KeyEvent, cx: &mut Context) {
        // `C-M-s` (isearch-regexp) has no key macro — it is CONTROL|ALT.
        let ctrl_alt = |k: KeyEvent, ch: char| {
            k.code == KeyCode::Char(ch) && k.modifiers == KeyModifiers::CONTROL | KeyModifiers::ALT
        };
        match prefix {
            // ---- `%` — whole-name regexp batch operations ----
            Prefix::Pct => match key {
                // % & — dired-flag-garbage-files
                key!('&') => {
                    let n = self.flag_matching(zmax_core::dired::is_garbage_file);
                    cx.editor
                        .set_status(format!("dired: flagged {n} garbage file(s)"));
                }
                // % d — dired-flag-files-regexp
                key!('d') => self.begin_input("Flag for deletion (regexp): ", Pending::FlagRegexp),
                // % m — dired-mark-files-regexp
                key!('m') => self.begin_input("Mark (regexp): ", Pending::MarkRegexp),
                // % g — dired-mark-files-containing-regexp (searches CONTENTS)
                key!('g') => {
                    self.begin_input("Mark files containing (regexp): ", Pending::MarkContaining)
                }
                // % u / % l — dired-upcase / dired-downcase (renames on disk)
                key!('u') => {
                    let n = self.rename_transform(NameTransform::Upcase);
                    cx.editor.set_status(format!("dired: upcased {n} name(s)"));
                }
                key!('l') => {
                    let n = self.rename_transform(NameTransform::Downcase);
                    cx.editor
                        .set_status(format!("dired: downcased {n} name(s)"));
                }
                // % R / % r / % C / % H / % S / % Y — regexp -> replacement over
                // the whole name of each target.
                key!('R') | key!('r') => self.begin_regexp_op(RegexpKind::Rename),
                key!('C') => self.begin_regexp_op(RegexpKind::Copy),
                key!('H') => self.begin_regexp_op(RegexpKind::Hardlink),
                key!('S') => self.begin_regexp_op(RegexpKind::Symlink),
                key!('Y') => self.begin_regexp_op(RegexpKind::RelSymlink),
                _ => {}
            },
            // ---- `*` — the mark map ----
            Prefix::Star => match key {
                // * * / * / / * @ — mark executables / directories / symlinks
                key!('*') => {
                    let n = self.mark_executables();
                    cx.editor
                        .set_status(format!("dired: marked {n} executable(s)"));
                }
                key!('/') => {
                    let n = self.mark_where(|e| e.is_dir);
                    cx.editor
                        .set_status(format!("dired: marked {n} directory(ies)"));
                }
                key!('@') => {
                    let n = self.mark_where(|e| e.is_symlink);
                    cx.editor
                        .set_status(format!("dired: marked {n} symlink(s)"));
                }
                // * % — dired-mark-files-regexp
                key!('%') => self.begin_input("Mark (regexp): ", Pending::MarkRegexp),
                // * c — dired-change-marks
                key!('c') => self.begin_input("Change marks (OLD NEW): ", Pending::ChangeMarks),
                // * s — dired-mark-subdir-files
                key!('s') => {
                    let n = self.mark_subdir_files();
                    cx.editor
                        .set_status(format!("dired: marked {n} file(s) in subdir"));
                }
                // * N — dired-number-of-marked-files
                key!('N') => {
                    let (count, bytes) = marked_summary(&self.entries, |n| self.marked.contains(n));
                    cx.editor.set_status(format!(
                        "dired: {count} marked file(s), {} total",
                        human_size(bytes)
                    ));
                }
                // * ! — dired-unmark-all-marks (every mark, no prompt)
                key!('!') => {
                    let n = self.unmark_all_files('\n');
                    cx.editor.set_status(format!("dired: unmarked {n} file(s)"));
                }
                // * ? — dired-unmark-all-files (prompts for the mark character)
                key!('?') => {
                    self.prefix = Some(Prefix::UnmarkChar);
                    cx.editor.set_status("Remove marks (RET means all): ");
                }
                // * m / * u / * DEL / * t — mark / unmark / unmark-backward / toggle
                key!('m') => {
                    if let Some(n) = self.current_name() {
                        self.marked.insert(n);
                        self.move_selection(1);
                    }
                }
                key!('u') => {
                    if let Some(n) = self.current_name() {
                        self.marked.remove(&n);
                        self.flagged.remove(&n);
                        self.move_selection(1);
                    }
                }
                key!(Backspace) => {
                    self.move_selection(-1);
                    if let Some(n) = self.current_name() {
                        self.marked.remove(&n);
                        self.flagged.remove(&n);
                    }
                }
                key!('t') => self.toggle_all_marks(),
                // * C-n / * C-p — dired-next/prev-marked-file
                ctrl!('n') => self.next_marked(1),
                ctrl!('p') => self.next_marked(-1),
                _ => {}
            },
            // ---- `:` — epa (gpg) ----
            Prefix::Epa => match key {
                key!('e') => {
                    let t = self.targets();
                    self.begin_input("Encrypt to recipient: ", Pending::EpaEncrypt(t));
                }
                key!('d') => self.epa_run(&["--yes", "-d"], "decrypted", cx),
                key!('s') => self.epa_run(&["--yes", "--detach-sign"], "signed", cx),
                key!('v') => self.epa_run(&["--verify"], "verified", cx),
                _ => {}
            },
            // ---- `C-t` — image-dired ----
            Prefix::Image => match key {
                // C-t d / C-t i / C-t . — show the image(s) in the editor
                key!('d') | key!('i') | key!('.') => self.image_display_inline(cx),
                // C-t x — hand it to the OS image viewer
                key!('x') => self.image_display_external(cx),
                // C-t c — comment the marked images
                key!('c') => {
                    let t = self.targets();
                    self.begin_input("Image comment: ", Pending::ImageComment(t));
                }
                // C-t e — edit the comment and tags of the image at point
                key!('e') => {
                    if let Some(n) = self.current_name() {
                        self.begin_input("Comment;tags: ", Pending::ImageCommentTags(n));
                    }
                }
                _ => {}
            },
            // ---- `M-s` — the search map ----
            Prefix::Search => match key {
                key!('f') => {
                    self.prefix = Some(Prefix::SearchName);
                    cx.editor.set_status("M-s f- (C-s: names, C-M-s: regexp)");
                }
                key!('a') => {
                    self.prefix = Some(Prefix::SearchContents);
                    cx.editor
                        .set_status("M-s a- (C-s: contents, C-M-s: regexp)");
                }
                _ => {}
            },
            // M-s f C-s / C-M-s — dired-isearch-filenames[-regexp]
            Prefix::SearchName => {
                if key == ctrl!('s') {
                    self.begin_input(
                        "Filename I-search: ",
                        Pending::IsearchFilenames { regexp: false },
                    );
                } else if ctrl_alt(key, 's') {
                    self.begin_input(
                        "Filename regexp I-search: ",
                        Pending::IsearchFilenames { regexp: true },
                    );
                }
            }
            // M-s a C-s / C-M-s — dired-do-isearch[-regexp]: search the marked
            // files' contents. `dired-do-isearch` (C-s) hands
            // `multi-isearch-files` a LITERAL string; only
            // `dired-do-isearch-regexp` (C-M-s) goes through
            // `multi-isearch-files-regexp` (dired-aux.el). Sharing one regexp
            // read made a `.` or `*` typed at the C-s prompt match something
            // other than itself.
            Prefix::SearchContents => {
                if key == ctrl!('s') {
                    self.begin_contents_search(false);
                } else if ctrl_alt(key, 's') {
                    self.begin_contents_search(true);
                }
            }
            // M-DEL / `* ?` — the mark character to remove everywhere.
            Prefix::UnmarkChar => {
                let mark = match key {
                    key!(Enter) => '\n',
                    KeyEvent {
                        code: KeyCode::Char(c),
                        ..
                    } => c,
                    _ => return,
                };
                let n = self.unmark_all_files(mark);
                cx.editor.set_status(format!("dired: unmarked {n} file(s)"));
            }
        }
    }

    /// Arm the first leg of a `%`-prefixed regexp batch op (read the regexp; the
    /// second leg reads the replacement).
    fn begin_regexp_op(&mut self, kind: RegexpKind) {
        let t = self.targets();
        self.begin_input(kind.pattern_prompt(), Pending::RegexpOpPattern(kind, t));
    }

    /// Re-read the directory, reporting an unreadable directory in the overlay.
    fn revert(&mut self) {
        if let Err(err) = self.read_dir() {
            self.error = Some(format!("{err}"));
        }
    }

    /// Whether `dired-click-to-select-mode` is on, so the command that toggles
    /// it can report the state it left behind.
    pub fn click_to_select(&self) -> bool {
        self.click_to_select
    }

    /// Emacs `dired-click-to-select-mode`: toggle the minor mode that repurposes
    /// `mouse-2` from "visit in another window" to "toggle this file's mark"
    /// (`dired-mark-for-click`). Turning it *off* runs `dired-unmark-all-marks`, as
    /// Emacs does in the mode body. `pub` so a palette command can drive the live
    /// Dired, mirroring [`Dired::undo`] / `dired-undo`.
    pub fn toggle_click_to_select(&mut self) {
        self.click_to_select = !self.click_to_select;
        if !self.click_to_select {
            // Mode disabled -> `dired-unmark-all-marks` (both `*` and `D`).
            self.marked.clear();
            self.flagged.clear();
        }
    }

    /// Emacs `dired-enable-click-to-select-mode`: force click-to-select on and mark
    /// the file at point (Emacs marks the file under the touch event; with no event
    /// source the row at point is the equivalent). `pub`, same rationale as
    /// [`Dired::toggle_click_to_select`].
    pub fn enable_click_to_select(&mut self) {
        if let Some(n) = self.current_name() {
            self.marked.insert(n);
        }
        self.click_to_select = true;
    }

    /// Emacs `<touchscreen-hold>` in `dired-mode-map`, bound to
    /// `dired-enable-click-to-select-mode`: a long-press over a file name turns on
    /// click-to-select. A terminal delivers no touch-screen events, so this handler
    /// exists for parity but is never reached from real input; it does exactly what
    /// the Emacs binding does — enable the mode and mark the file at point.
    pub fn touchscreen_hold(&mut self) {
        self.enable_click_to_select();
    }

    /// Emacs `dired-mark-for-click` (`mouse-2` while `dired-click-to-select-mode` is
    /// on): toggle the mark on the entry at `index` — mark an unmarked file, unmark
    /// an already-marked one.
    fn mark_for_click(&mut self, index: usize) {
        if let Some(name) = self.entries.get(index).map(|e| e.name.clone()) {
            if !self.marked.remove(&name) {
                self.marked.insert(name);
            }
        }
    }

    /// `dired-mouse-find-file-other-window` driven from the command palette
    /// rather than from a click: Emacs's command moves point to the click first,
    /// so with no event the row already at point is the target. `pub`, same
    /// rationale as [`Dired::undo`]. Returns the callback that opens the file, or
    /// `None` when the entry at point is a directory (which is entered in place).
    pub fn find_file_other_window(&mut self) -> Option<Callback> {
        self.open_file(Action::VerticalSplit, false)
    }

    /// Emacs `dired-mouse-find-file-other-window` (`mouse-2`): move point to the
    /// clicked row — Emacs does the same with `goto-char (posn-point ...)`, so a
    /// click anywhere on the line counts, not just on the name — then visit it in
    /// another window. A directory is entered in place (this overlay *is* the
    /// dired buffer, as with `dired-goto-subdir`); a file opens in a split. That
    /// is exactly the `o` key's action, which is the point of the binding.
    ///
    /// While `dired-click-to-select-mode` is on, `mouse-2` is remapped to
    /// `dired-mark-for-click` instead, toggling the clicked file's mark.
    fn mouse_find_file_other_window(&mut self, row: u16) -> EventResult {
        // Rows start below the title and the error/blank line (see `render`).
        let body_y = self.area.y + 2;
        if row < body_y {
            return EventResult::Ignored(None);
        }
        let index = self.scroll + (row - body_y) as usize;
        if index >= self.entries.len() {
            return EventResult::Ignored(None);
        }
        self.selected = index;
        if self.click_to_select {
            // `dired-mark-for-click`: toggle the mark, never visit.
            self.mark_for_click(index);
            return EventResult::Consumed(None);
        }
        EventResult::Consumed(self.open_file(Action::VerticalSplit, false))
    }

    /// Dired's mouse map. Emacs binds one event here — `mouse-2` to
    /// `dired-mouse-find-file-other-window` — so every other mouse event is left
    /// alone for the layers underneath.
    fn handle_mouse_event(&mut self, ev: &MouseEvent) -> EventResult {
        // An open minibuffer read owns the overlay until it is answered.
        if self.input.is_some() {
            return EventResult::Ignored(None);
        }
        let inside = ev.column >= self.area.left()
            && ev.column < self.area.right()
            && ev.row >= self.area.top()
            && ev.row < self.area.bottom();
        if !inside {
            return EventResult::Ignored(None);
        }
        match ev.kind {
            MouseEventKind::Down(MouseButton::Middle) => self.mouse_find_file_other_window(ev.row),
            _ => EventResult::Ignored(None),
        }
    }
}

impl Component for Dired {
    fn handle_event(&mut self, event: &Event, cx: &mut Context) -> EventResult {
        let mut key = match event {
            Event::Key(key) => *key,
            Event::Mouse(ev) => return self.handle_mouse_event(ev),
            _ => return EventResult::Ignored(None),
        };

        // Terminals report an uppercase character with SHIFT set, but the
        // uppercase bindings below (I, S, C, R, H, T, E, W, ...) are written as
        // plain `key!('I')` — i.e. no modifiers. The character alone already
        // carries the shift, so drop the flag exactly like the editor's own
        // `canonicalize_key` does before dispatching a keymap.
        if matches!(key.code, KeyCode::Char(_)) {
            key.modifiers.remove(KeyModifiers::SHIFT);
        }

        // In-mode minibuffer: route keys to line editing while a prompt is open.
        if self.input.is_some() {
            // `dired-isearch-filenames` (`M-s f C-s` / `C-M-s`) is a genuine
            // isearch: point tracks the pattern as it is typed, `C-s`/`C-r`
            // repeat and reverse, and `DEL` walks the command stack back. Every
            // other read stays modal and acts only on RET.
            let isearch = self.input.as_ref().is_some_and(|inp| inp.isearch.is_some());
            match key {
                key!(Esc) | ctrl!('c') | ctrl!('g') => {
                    // `isearch-abort` keeps a failing search alive, rubbing out
                    // the commands that failed; only a successful one quits.
                    if isearch && !self.isearch_abort() {
                        return EventResult::Consumed(None);
                    }
                    self.input = None;
                }
                key!(Enter) | ctrl!('j') => {
                    if let Some(inp) = self.input.take() {
                        self.isearch_point = Self::isearch_exit_point(inp.isearch.as_ref());
                        self.run_pending(inp.action, &inp.buffer, cx);
                    }
                    if self.close_requested {
                        return EventResult::Consumed(Some(Box::new(
                            |compositor: &mut Compositor, _cx| {
                                compositor.pop();
                            },
                        )));
                    }
                }
                key!(Backspace) | ctrl!('h') => {
                    if isearch {
                        self.isearch_delete_char();
                    } else if let Some(inp) = self.input.as_mut() {
                        inp.buffer.pop();
                    }
                }
                ctrl!('u') => {
                    if isearch {
                        // Rub the whole search out, back to the opening state.
                        while self
                            .input
                            .as_ref()
                            .and_then(|inp| inp.isearch.as_ref())
                            .is_some_and(|is| is.cmds.len() > 1)
                        {
                            self.isearch_delete_char();
                        }
                    } else if let Some(inp) = self.input.as_mut() {
                        inp.buffer.clear();
                    }
                }
                // Repeat keys, live only while a filename isearch is running.
                ctrl!('s') if isearch => self.isearch_repeat(true),
                ctrl!('r') if isearch => self.isearch_repeat(false),
                KeyEvent {
                    code: KeyCode::Char(c),
                    modifiers,
                } if modifiers == KeyModifiers::NONE || modifiers == KeyModifiers::SHIFT => {
                    if isearch {
                        self.isearch_type(c);
                    } else if let Some(inp) = self.input.as_mut() {
                        inp.buffer.push(c);
                    }
                }
                _ => {}
            }
            return EventResult::Consumed(None);
        }

        // Emacs clears the echo area when the next command runs, so a message
        // lives exactly until the following key.
        self.message = None;

        // A prefix chord (`%`, `*`, `:`, `C-t`, `M-s`, `M-DEL`) owns the next key.
        if let Some(prefix) = self.prefix.take() {
            self.handle_prefix(prefix, key, cx);
            return EventResult::Consumed(None);
        }

        let close: Callback = Box::new(|compositor: &mut Compositor, _cx| {
            compositor.pop();
        });
        match key {
            key!('q') | key!(Esc) | ctrl!('c') => return EventResult::Consumed(Some(close)),
            // ---- prefix chords (see `handle_prefix`) ----
            key!('%') => self.prefix = Some(Prefix::Pct),
            key!('*') => self.prefix = Some(Prefix::Star),
            key!(':') => self.prefix = Some(Prefix::Epa),
            ctrl!('t') => self.prefix = Some(Prefix::Image),
            alt!('s') => self.prefix = Some(Prefix::Search),
            // M-DEL — dired-unmark-all-files: read the mark char to remove.
            alt!(Backspace) => {
                self.prefix = Some(Prefix::UnmarkChar);
                cx.editor.set_status("Remove marks (RET means all): ");
            }
            // ---- motion ----
            key!('n') | key!(Down) | ctrl!('n') | key!(' ') => self.move_selection(1),
            key!('p') | key!(Up) | ctrl!('p') => self.move_selection(-1),
            // C-v / M-v — mg dired-scroll-down / dired-scroll-up, which are also
            // the Emacs `scroll-up-command` / `scroll-down-command` Dired inherits
            // from the global map. PageDown/PageUp are the usual aliases.
            ctrl!('v') | key!(PageDown) => self.scroll_page(true),
            alt!('v') | key!(PageUp) => self.scroll_page(false),
            // Home / End jump the cursor, so like every other motion they drop
            // the column a filename isearch left behind.
            key!(Home) => {
                self.selected = 0;
                self.isearch_point = None;
            }
            key!(End) => {
                self.selected = self.entries.len().saturating_sub(1);
                self.isearch_point = None;
            }
            // g — revert-buffer (re-read the directory); l — dired-do-redisplay
            // (re-stat the marked files, which for this listing is the same read).
            key!('g') | key!('l') => self.revert(),
            alt!('}') => self.next_marked(1),
            alt!('{') => self.next_marked(-1),
            key!('(') => self.show_details = !self.show_details,
            // RET / f / e — dired-find-file; `a` — dired-find-alternate-file (which
            // in Emacs kills the Dired buffer; this overlay pops either way).
            key!(Enter) | key!('f') | key!('e') | key!('a') => {
                if let Some(cb) = self.visit() {
                    return EventResult::Consumed(Some(cb));
                }
            }
            key!('^') | key!('-') => self.up_dir(),
            key!('m') => {
                if let Some(n) = self.current_name() {
                    self.marked.insert(n);
                    self.move_selection(1);
                }
            }
            key!('u') => {
                if let Some(n) = self.current_name() {
                    self.marked.remove(&n);
                    self.flagged.remove(&n);
                    self.move_selection(1);
                }
            }
            key!(Backspace) => {
                self.move_selection(-1);
                if let Some(n) = self.current_name() {
                    self.marked.remove(&n);
                    self.flagged.remove(&n);
                }
            }
            // dired-unmark-all-marks (`U`, and `* !`): drop every mark, no prompt.
            key!('U') => {
                self.marked.clear();
                self.flagged.clear();
            }
            key!('t') => self.toggle_all_marks(),
            key!('d') => {
                if let Some(n) = self.current_name() {
                    self.flagged.insert(n);
                    self.move_selection(1);
                }
            }
            key!('~') => {
                let n = self.flag_matching(zmax_core::dired::is_backup_file);
                cx.editor
                    .set_status(format!("dired: flagged {n} backup file(s)"));
            }
            key!('#') => {
                let n = self.flag_matching(zmax_core::dired::is_auto_save_file);
                cx.editor
                    .set_status(format!("dired: flagged {n} auto-save file(s)"));
            }
            // & — dired-do-async-shell-command (flagging garbage is `% &`), read
            // with `"& on %s: "` (dired-aux.el:820).
            key!('&') => {
                let t = self.targets();
                let prompt = format!("& on {}: ", mark_prompt(&t));
                self.begin_input(prompt, Pending::AsyncShellCommand(t));
            }
            key!('x') => {
                let names: Vec<String> = self
                    .entries
                    .iter()
                    .filter(|e| self.flagged.contains(&e.name))
                    .map(|e| e.name.clone())
                    .collect();
                if names.is_empty() {
                    cx.editor.set_status("dired: no files flagged for deletion");
                } else {
                    let n = self.delete_names(&names);
                    let _ = self.read_dir();
                    cx.editor.set_status(format!("dired: deleted {n} file(s)"));
                }
            }
            key!('D') => {
                let names = self.targets();
                if !names.is_empty() {
                    let n = self.delete_names(&names);
                    let _ = self.read_dir();
                    cx.editor.set_status(format!("dired: deleted {n} file(s)"));
                }
            }
            key!('w') => {
                let names = self.targets();
                if !names.is_empty() {
                    let joined = names.join(" ");
                    let _ = cx.editor.registers.write('+', vec![joined.clone()]);
                    cx.editor.set_status(format!("dired: copied {joined}"));
                }
            }
            key!('s') => {
                self.sort = self.sort.next();
                self.resort();
                cx.editor
                    .set_status(format!("dired: sorted by {}", self.sort.label()));
            }
            key!('r') => {
                self.reverse = !self.reverse;
                self.resort();
            }
            // . — dired-clean-directory (flag excess numbered backups). `,` keeps
            // the old zmax binding; dotfile visibility moved to `z`.
            key!('.') | key!(',') => {
                let n = self.clean_directory();
                cx.editor
                    .set_status(format!("dired: flagged {n} excess backup(s)"));
            }
            key!('z') => {
                self.show_hidden = !self.show_hidden;
                self.revert();
            }
            // ---- zmax mark aliases (the Emacs keys are `* /` and `* *`) ----
            alt!('d') => {
                let n = self.mark_where(|e| e.is_dir);
                cx.editor
                    .set_status(format!("dired: marked {n} directory(ies)"));
            }
            alt!('x') => {
                let n = self.mark_executables();
                cx.editor
                    .set_status(format!("dired: marked {n} executable(s)"));
            }
            // A — dired-do-find-regexp: grep the targets.
            key!('A') => {
                let targets = self.targets();
                self.begin_input(
                    "Find regexp: ",
                    Pending::FindRegexp {
                        targets,
                        regexp: true,
                    },
                );
            }
            // Z — dired-do-compress (gzip / gunzip each target).
            key!('Z') => self.compress_targets(cx),
            // k / K — dired-do-kill-lines (hide the marked rows).
            key!('k') | key!('K') => {
                let n = self.kill_lines();
                cx.editor.set_status(format!("dired: killed {n} line(s)"));
            }
            key!('>') => {
                if let Some(i) = next_dir_index(&self.entries, self.selected, true) {
                    self.selected = i;
                }
            }
            key!('<') => {
                if let Some(i) = next_dir_index(&self.entries, self.selected, false) {
                    self.selected = i;
                }
            }
            key!('T') => {
                let n = self.touch_targets();
                cx.editor.set_status(format!("dired: touched {n} file(s)"));
            }
            key!('v') => {
                if let Some(cb) = self.open_file(Action::Replace, true) {
                    return EventResult::Consumed(Some(cb));
                }
            }
            key!('o') => {
                if let Some(cb) = self.open_file(Action::VerticalSplit, false) {
                    return EventResult::Consumed(Some(cb));
                }
            }
            ctrl!('o') => {
                if let Some(cb) = self.open_file(Action::HorizontalSplit, false) {
                    return EventResult::Consumed(Some(cb));
                }
            }
            // ---- ported minibuffer-prompt commands ----
            key!('+') => self.begin_input("Create directory: ", Pending::CreateDir),
            // dired-create-empty-file has no key in Emacs (M-x only); M-+ pairs it
            // with `+` (create directory).
            alt!('+') => self.begin_input("Create empty file: ", Pending::CreateFile),
            // E — dired-do-open: hand the targets to the OS opener (`V` alias).
            key!('E') => self.open_targets(cx),
            // I — dired-do-info: run info on the file at point.
            key!('I') => self.dired_do_info(cx),
            // W — browse-url-of-dired-file.
            key!('W') => self.browse_url(cx),
            key!('C') => {
                let t = self.targets();
                self.begin_input("Copy to: ", Pending::Copy(t));
            }
            // R — dired-do-rename.
            key!('R') => {
                let t = self.targets();
                self.begin_input("Rename to: ", Pending::Rename(t));
            }
            key!('S') => {
                let t = self.targets();
                self.begin_input("Symlink to: ", Pending::Symlink(t));
            }
            key!('H') => {
                let t = self.targets();
                self.begin_input("Hardlink to: ", Pending::Hardlink(t));
            }
            key!('M') => {
                let t = self.targets();
                self.begin_input("Chmod (octal): ", Pending::Chmod(t));
            }
            // `/` (zmax alias of `% d`) — flag by regexp.
            key!('/') => self.begin_input("Flag for deletion (regexp): ", Pending::FlagRegexp),
            // j — dired-goto-file (`J` alias).
            key!('j') | key!('J') => self.begin_input("Goto file: ", Pending::GotoFile),
            // c — dired-do-compress-to (tar+gzip the targets into an archive).
            key!('c') => {
                let t = self.targets();
                self.begin_input("Compress to (archive): ", Pending::CompressTo(t));
            }
            // `y` (zmax alias of `% g`) — mark by CONTENTS regexp.
            key!('y') => {
                self.begin_input("Mark files containing (regexp): ", Pending::MarkContaining)
            }
            // dired-undo: Emacs binds it to C-_ (and C-/ and C-x u); `_` is kept
            // as the single-key alias this component has always had.
            key!('_') | ctrl!('_') => {
                if self.undo() {
                    cx.editor.set_status("dired: undo");
                } else {
                    cx.editor.set_status("dired: nothing to undo");
                }
            }
            // `F` (zmax alias of `% H`) — hardlink each name through a regexp.
            key!('F') => self.begin_regexp_op(RegexpKind::Hardlink),
            key!('Y') => {
                let t = self.targets();
                self.begin_input("RelSymlink to (dir): ", Pending::RelSymlink(t));
            }
            // ---- ported: comparison ----
            key!('=') => {
                if let Some(name) = self.current_name() {
                    self.begin_input("Diff against: ", Pending::Diff(name));
                }
            }
            key!('@') => self.begin_input("Compare with directory: ", Pending::CompareDir),
            // ---- ported: external-tool file ops ----
            key!('O') => {
                let t = self.targets();
                self.begin_input("Chown to: ", Pending::Chown(t));
            }
            // G — dired-do-chgrp.
            key!('G') => {
                let t = self.targets();
                self.begin_input("Chgrp to: ", Pending::Chgrp(t));
            }
            // ! and X — dired-do-shell-command (Emacs binds both to it), read with
            // `"! on %s: "` (dired-aux.el:888); mg's `!` prompts the same way
            // (`eread("! on %s: ", ...)`, dired.c:626).
            key!('!') | key!('X') => {
                let t = self.targets();
                let prompt = format!("! on {}: ", mark_prompt(&t));
                self.begin_input(prompt, Pending::ShellCommand(t));
            }
            // Q — dired-do-find-regexp-and-replace.
            key!('Q') => {
                let t = self.targets();
                self.begin_input("Find regexp: ", Pending::FindReplacePattern(t));
            }
            key!('V') => self.open_targets(cx),
            // ---- ported: subdirectory insertion / hiding / motion ----
            // `i` on a directory inserts its listing as a subdir section; on a file
            // it falls back to filename isearch (Emacs binds isearch to `M-s f`).
            key!('i') => self.insert_subdir(),
            key!('$') => self.hide_subdir(),
            alt!('$') => self.hide_all_subdirs(),
            // Subdir motion. Emacs uses C-M-n / C-M-p / C-M-u / C-M-d (matched
            // below, since CONTROL|ALT has no key macro); M-n/M-p/M-u/M-y are the
            // zmax aliases.
            alt!('n') => self.goto_subdir(true),
            alt!('p') => self.goto_subdir(false),
            alt!('u') => self.tree_move(true),
            alt!('y') => self.tree_move(false),
            // dired-goto-subdir: Emacs binds it to M-G; `M-j` stays as the alias.
            alt!('j') | alt!('G') => self.begin_input("Goto subdir: ", Pending::GotoSubdir),
            // ---- elisp file operations (embedded elisprs) ----
            // L — dired-do-load (M-l alias); B — dired-do-byte-compile (b alias).
            key!('L') | alt!('l') => self.dired_do_load(cx),
            key!('B') | key!('b') => self.dired_byte_compile(cx),
            // ---- man / print / open-in-tab / find ----
            // N — dired-do-man (M-m alias); P — dired-do-print (M-r alias).
            key!('N') | alt!('m') => self.dired_do_man(cx),
            key!('P') | alt!('r') => self.dired_do_print(cx),
            alt!('t') => {
                if let Some(cb) = self.open_other_tab() {
                    return EventResult::Consumed(Some(cb));
                }
            }
            // M-f — find-dired: the whole find ARGS string, emacs's prompt
            // verbatim. M-F keeps find-name-dired, which is the same command with
            // the ARGS built from a glob (`-name PATTERN`), on its own chord.
            // Emacs reads the directory first, then the args, and defaults the
            // directory to the one Dired is on (find-dired.el:189-190).
            alt!('f') => self.begin_input("Run find in directory: ", Pending::FindDir), // find-dired
            alt!('F') => self.begin_input("Find name (glob): ", Pending::FindName), // find-name-dired
            alt!('g') => self.begin_input("Find grep (regexp): ", Pending::FindGrep), // find-grep-dired
            // ---- ported: epa (gpg) file operations ----
            alt!('e') => {
                let t = self.targets();
                self.begin_input("Encrypt to recipient: ", Pending::EpaEncrypt(t));
                // epa-dired-do-encrypt
            }
            alt!('k') => self.epa_run(&["--yes", "-d"], "decrypted", cx), // epa-dired-do-decrypt
            alt!('z') => self.epa_run(&["--yes", "--detach-sign"], "signed", cx), // epa-dired-do-sign
            // epa-dired-do-verify has no `M-v` alias: `M-v` is scroll-down in both
            // Emacs and mg, and the whole epa map is on the `:` prefix (`: v`).
            // ---- ported: find-and-replace / locate / image external ----
            alt!('q') => {
                let t = self.targets();
                self.begin_input("Find regexp: ", Pending::FindReplacePattern(t));
                // dired-do-find-regexp-and-replace
            }
            alt!('c') => self.begin_input("Locate: ", Pending::Locate), // locate-with-filter
            alt!('o') => self.image_display_external(cx), // image-dired-dired-display-external
            alt!('i') => self.image_display_inline(cx),   // image-dired display-image/this/thumbs
            alt!('w') => self.wdired_change(cx),          // wdired-change-to-wdired-mode
            // ---- ported: image-dired comment/tag metadata ----
            alt!('a') => {
                let t = self.targets();
                self.begin_input("Image comment: ", Pending::ImageComment(t)); // image-dired-dired-comment-files
            }
            alt!('b') => {
                if let Some(n) = self.current_name() {
                    self.begin_input("Comment;tags: ", Pending::ImageCommentTags(n));
                    // image-dired-dired-edit-comment-and-tags
                }
            }
            alt!('h') => {
                if let Some(n) = self.current_name() {
                    self.begin_input("Image description: ", Pending::ImageComment(vec![n]));
                    // image-dired-thumbnail-set-image-description
                }
            }
            key!('h') => self.begin_input(
                "Filename regexp I-search: ",
                Pending::IsearchFilenames { regexp: true },
            ),
            // C-M-n / C-M-p / C-M-u / C-M-d — the real Emacs subdirectory-motion
            // keys. CONTROL|ALT is not expressible with the ctrl!/alt! macros, so
            // the modifier pair is matched directly.
            KeyEvent {
                code: KeyCode::Char(c),
                modifiers,
            } if modifiers == KeyModifiers::CONTROL | KeyModifiers::ALT => match c {
                'n' => self.goto_subdir(true),  // dired-next-subdir
                'p' => self.goto_subdir(false), // dired-prev-subdir
                'u' => self.tree_move(true),    // dired-tree-up
                'd' => self.tree_move(false),   // dired-tree-down
                _ => {}
            },
            _ => {}
        }
        // A command that replaced the window's buffer (info, wdired, ...) asked
        // for the overlay to go away; without this only the prompt/RET branch
        // above popped it, so the new buffer stayed hidden behind Dired.
        if self.close_requested {
            return EventResult::Consumed(Some(Box::new(|compositor: &mut Compositor, _cx| {
                compositor.pop();
            })));
        }
        // Stay modal: never leak keys to the editor behind us.
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        // Remembered so `mouse-2` can map a click row back to an entry.
        self.area = area;
        let theme = &ctx.editor.theme;
        let mut bg = theme.get("ui.background");
        // `transparent-background`: drop the panel fill so the terminal shows
        // through, matching the editor surface and the rest of the IDE.
        if ctx.editor.config().transparent_background {
            bg.bg = None;
        }
        let header_style = theme.get("ui.text.focus");
        let dir_style = theme.get("ui.text.directory");
        let text_style = theme.get("ui.text");
        let info_style = theme.get("ui.linenr");
        let sel_style = theme.get("ui.selection");
        let mark_style = theme.get("diff.plus");
        let flag_style = theme.get("error");

        surface.clear_with(area, bg);
        if area.width < 8 || area.height < 3 {
            return;
        }

        let title = format!(" Dired: {}", self.dir.display());
        surface.set_stringn(area.x, area.y, &title, area.width as usize, header_style);
        let hint = "Enter open  m mark  d flag  x del  s sort  q quit";
        if title.len() + hint.len() + 3 < area.width as usize {
            surface.set_stringn(
                area.x + area.width - hint.len() as u16 - 1,
                area.y,
                hint,
                hint.len(),
                info_style,
            );
        }

        let body_y = area.y + 2;
        let body_h = area.height.saturating_sub(2);
        self.viewport = body_h as usize;

        // The echo row: an error wins over a plain message, as it does in the
        // Emacs echo area.
        if let Some(err) = &self.error {
            surface.set_stringn(area.x, area.y + 1, err, area.width as usize, flag_style);
        } else if let Some(msg) = &self.message {
            surface.set_stringn(area.x, area.y + 1, msg, area.width as usize, info_style);
        }

        if self.entries.is_empty() {
            surface.set_stringn(area.x, body_y, "(empty)", area.width as usize, info_style);
            return;
        }

        // Keep the selection in view.
        if self.selected < self.scroll {
            self.scroll = self.selected;
        } else if self.selected >= self.scroll + self.viewport {
            self.scroll = self.selected + 1 - self.viewport;
        }

        for (offset, e) in self
            .entries
            .iter()
            .enumerate()
            .skip(self.scroll)
            .take(body_h as usize)
        {
            let y = body_y + (offset - self.scroll) as u16;
            let m = mark_char(
                self.marked.contains(&e.name),
                self.flagged.contains(&e.name),
            );
            let kind = if e.is_symlink {
                "l"
            } else if e.is_dir {
                "d"
            } else {
                "-"
            };
            let size = if e.is_dir {
                String::new()
            } else {
                human_size(e.size)
            };
            let name = if e.is_dir {
                format!("{}/", e.name)
            } else {
                e.name.clone()
            };
            let line = if self.show_details {
                format!("{} {} {:>7}  {}", m, kind, size, name)
            } else {
                // dired-hide-details-mode: mark + name only.
                format!("{} {}", m, name)
            };
            let base = if offset == self.selected {
                sel_style
            } else if e.is_dir {
                dir_style
            } else {
                text_style
            };
            surface.set_stringn(area.x, y, &line, area.width as usize, base);
            // Accent the mark column.
            if m != ' ' {
                let ms = if m == 'D' { flag_style } else { mark_style };
                surface.set_stringn(area.x, y, &m.to_string(), 1, ms);
            }
        }

        // Footer: the active minibuffer read, else the listing counts.
        let footer = if let Some(inp) = &self.input {
            // A live isearch labels itself the way Emacs does, so the failing /
            // wrapped state is visible in the prompt.
            match &inp.isearch {
                Some(is) => format!("{}{}", Self::isearch_prompt(is), inp.buffer),
                None => format!("{}{}", inp.prompt, inp.buffer),
            }
        } else {
            format!(
                "{} items  {} marked  {} flagged  sort:{}{}",
                self.entries.len(),
                self.marked.len(),
                self.flagged.len(),
                self.sort.label(),
                if self.reverse { " (rev)" } else { "" }
            )
        };
        if body_h > 0 {
            let style = if self.input.is_some() {
                header_style
            } else {
                info_style
            };
            surface.set_stringn(
                area.x,
                area.y + area.height - 1,
                &footer,
                area.width as usize,
                style,
            );
        }
    }
}

#[cfg(test)]
mod subdir_tests {
    use super::*;

    /// Emacs `i` / `$` / `M-$`: inserting a subdirectory expands its listing as a
    /// prefixed section, `$` collapses it, and `M-$` toggles all sections. Drives
    /// the component methods directly (no Editor needed — read_dir is pure I/O).
    #[test]
    fn insert_hide_and_hide_all_subdirs() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::write(root.join("top.txt"), b"x").unwrap();
        std::fs::create_dir(root.join("sub")).unwrap();
        std::fs::write(root.join("sub").join("inner.txt"), b"y").unwrap();

        let mut d = Dired::new(root.to_path_buf()).unwrap();
        assert!(!d.entries.iter().any(|e| e.name == "sub/inner.txt"));

        // Insert the "sub" directory's listing.
        d.selected = d.entries.iter().position(|e| e.name == "sub").unwrap();
        d.insert_subdir();
        assert!(
            d.entries.iter().any(|e| e.name == "sub/inner.txt"),
            "insert_subdir should add the prefixed subdir entry"
        );
        // A file op path resolves correctly through the prefix.
        assert!(d.dir.join("sub/inner.txt").is_file());

        // Hide the section from within it.
        d.selected = d
            .entries
            .iter()
            .position(|e| e.name == "sub/inner.txt")
            .unwrap();
        d.hide_subdir();
        assert!(
            !d.entries.iter().any(|e| e.name == "sub/inner.txt"),
            "hide_subdir should collapse the section"
        );

        // M-$ re-expands all (they were all hidden).
        d.hide_all_subdirs();
        assert!(
            d.entries.iter().any(|e| e.name == "sub/inner.txt"),
            "hide_all_subdirs should re-expand every section"
        );
        // ...and again collapses all.
        d.hide_all_subdirs();
        assert!(!d.entries.iter().any(|e| e.name == "sub/inner.txt"));
    }

    /// Emacs `dired-next-subdir`/`dired-prev-subdir`: motion jumps between the
    /// top section and each inserted subdir section's first entry.
    #[test]
    fn subdir_motion_between_sections() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir(root.join("aaa")).unwrap();
        std::fs::write(root.join("aaa").join("f1"), b"1").unwrap();
        std::fs::create_dir(root.join("bbb")).unwrap();
        std::fs::write(root.join("bbb").join("f2"), b"2").unwrap();

        let mut d = Dired::new(root.to_path_buf()).unwrap();
        // Insert both subdirs so there are three sections: top, aaa/, bbb/.
        d.selected = d.entries.iter().position(|e| e.name == "aaa").unwrap();
        d.insert_subdir();
        d.selected = d.entries.iter().position(|e| e.name == "bbb").unwrap();
        d.insert_subdir();

        // From the top section, next-subdir lands on aaa/'s first entry.
        d.selected = 0;
        d.goto_subdir(true);
        assert_eq!(
            Dired::entry_subdir(&d.entries[d.selected].name),
            Some("aaa")
        );
        // Next again -> bbb/ section.
        d.goto_subdir(true);
        assert_eq!(
            Dired::entry_subdir(&d.entries[d.selected].name),
            Some("bbb")
        );
        // Prev -> back to aaa/.
        d.goto_subdir(false);
        assert_eq!(
            Dired::entry_subdir(&d.entries[d.selected].name),
            Some("aaa")
        );
        // Prev -> top section (no subdir prefix).
        d.goto_subdir(false);
        assert_eq!(Dired::entry_subdir(&d.entries[d.selected].name), None);
    }

    /// `dired-unmark-all-files` (`M-DEL`, `* ?`): the typed mark character selects
    /// which marks go — `*` the marks, `D` the deletion flags, RET everything —
    /// unlike `U` / `* !`, which always drop both.
    #[test]
    fn unmark_all_files_removes_only_the_named_mark() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::write(root.join("a.txt"), b"a").unwrap();
        std::fs::write(root.join("b.txt"), b"b").unwrap();
        let mut d = Dired::new(root.to_path_buf()).unwrap();
        d.marked.insert("a.txt".into());
        d.flagged.insert("b.txt".into());

        // `*` removes the marks and leaves the deletion flags alone.
        assert_eq!(d.unmark_all_files('*'), 1);
        assert!(d.marked.is_empty());
        assert_eq!(d.flagged.len(), 1);

        // An unused mark character matches nothing.
        d.marked.insert("a.txt".into());
        assert_eq!(d.unmark_all_files('x'), 0);
        assert_eq!(d.marked.len(), 1);

        // `D` removes the deletion flags only.
        assert_eq!(d.unmark_all_files('D'), 1);
        assert!(d.flagged.is_empty());
        assert_eq!(d.marked.len(), 1);

        // RET (`\n`) removes every mark.
        d.flagged.insert("b.txt".into());
        assert_eq!(d.unmark_all_files('\n'), 2);
        assert!(d.marked.is_empty() && d.flagged.is_empty());
    }

    /// The `%` / `*` / `:` prefix chords are real two-key sequences: the first key
    /// arms the prefix (and does nothing else), the second runs the command. Here
    /// `* /` marks the directories — proving `*` alone did not mark anything.
    #[test]
    fn star_slash_marks_directories_via_the_prefix() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::write(root.join("f.txt"), b"f").unwrap();
        std::fs::create_dir(root.join("d1")).unwrap();
        std::fs::create_dir(root.join("d2")).unwrap();
        let mut d = Dired::new(root.to_path_buf()).unwrap();

        // Arming `*` marks nothing by itself.
        d.prefix = Some(Prefix::Star);
        assert!(d.marked.is_empty());

        // The follow-up `/` marks exactly the directories.
        let n = d.mark_where(|e| e.is_dir);
        assert_eq!(n, 2);
        assert!(d.marked.contains("d1") && d.marked.contains("d2"));
        assert!(!d.marked.contains("f.txt"));
    }

    /// wdired: only changed lines become renames, and a changed line count aborts.
    #[test]
    fn wdired_rename_plan_pairs_changed_names() {
        let orig = vec![
            "a.txt".to_string(),
            "b.txt".to_string(),
            "c.txt".to_string(),
        ];
        // Edit the first and last names.
        let edited = vec![
            "a1.txt".to_string(),
            "b.txt".to_string(),
            "c9.txt".to_string(),
        ];
        let plan = wdired_rename_plan(&orig, &edited).unwrap();
        assert_eq!(
            plan,
            vec![
                ("a.txt".to_string(), "a1.txt".to_string()),
                ("c.txt".to_string(), "c9.txt".to_string()),
            ]
        );
        // No changes -> empty plan.
        assert!(wdired_rename_plan(&orig, &orig).unwrap().is_empty());
        // A removed/added line desynchronizes the pairing -> error.
        assert!(wdired_rename_plan(&orig, &edited[..2]).is_err());
    }
}

#[cfg(test)]
mod isearch_tests {
    use super::*;

    /// `M-s f C-s` starts AT POINT. Emacs leaves point inside the name the
    /// previous search stopped in, so searching the same pattern again finds the
    /// NEXT occurrence in that name instead of the same one; moving the row
    /// cursor puts point back on the file name and the next search starts there.
    #[test]
    fn filename_isearch_resumes_from_where_the_last_one_stopped() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::write(root.join("banana.txt"), b"x").unwrap();

        let mut d = Dired::new(root.to_path_buf()).unwrap();
        d.selected = d
            .entries
            .iter()
            .position(|e| e.name == "banana.txt")
            .unwrap();
        let row = d.selected;

        // First search: `an` matches at 1..3 of "banana.txt".
        d.begin_input(
            "Filename I-search: ",
            Pending::IsearchFilenames { regexp: false },
        );
        d.isearch_type('a');
        d.isearch_type('n');
        let st = d.input.as_ref().unwrap().isearch.as_ref().unwrap();
        assert_eq!(
            st.cmds.last().unwrap().point,
            (row, 3),
            "point at match end"
        );

        // RET (isearch-exit) records that point...
        let inp = d.input.take().unwrap();
        d.isearch_point = Dired::isearch_exit_point(inp.isearch.as_ref());
        assert_eq!(d.isearch_point, Some((row, 3)));

        // ...and the next search picks up there, finding the SECOND `an` (3..5).
        d.begin_input(
            "Filename I-search: ",
            Pending::IsearchFilenames { regexp: false },
        );
        d.isearch_type('a');
        d.isearch_type('n');
        let st = d.input.as_ref().unwrap().isearch.as_ref().unwrap();
        assert_eq!(st.cmds.last().unwrap().point, (row, 5));

        // Moving the cursor drops the column, so a search starts at the name again.
        let inp = d.input.take().unwrap();
        d.isearch_point = Dired::isearch_exit_point(inp.isearch.as_ref());
        d.move_selection(0);
        assert_eq!(d.isearch_point, None);
        d.begin_input(
            "Filename I-search: ",
            Pending::IsearchFilenames { regexp: false },
        );
        d.isearch_type('a');
        d.isearch_type('n');
        let st = d.input.as_ref().unwrap().isearch.as_ref().unwrap();
        assert_eq!(
            st.cmds.last().unwrap().point,
            (row, 3),
            "back to the first match"
        );
    }

    /// The Emacs→`regex` translation may only ever hand the caller a pattern the
    /// `regex` crate can COMPILE, or `None`. The syntax-class and backreference
    /// escapes used to fall through as backslash-plus-character, which either
    /// failed to compile (`\1`) or compiled to something else entirely (`\sw`
    /// became whitespace followed by a literal `w`) — and `isearch_search` turns
    /// both into a search that silently holds point.
    #[test]
    fn every_translated_pattern_compiles_and_the_rest_are_refused() {
        for pat in [
            r"\(foo\|bar\)+",
            r"\<word\>",
            r"\_<sym\_>",
            r"\`anchored\'",
            r"a\{2,3\}",
            r"[]\.]",
            r"[^]a]",
            r"\s-+\sw",
            r"\S-\Sw",
            r"point\=here",
        ] {
            let out = emacs_regexp_to_rust(pat).unwrap_or_else(|| panic!("{pat} was refused"));
            regex::Regex::new(&out)
                .unwrap_or_else(|e| panic!("{pat} -> {out} does not compile: {e}"));
        }

        // The two syntax classes with an exact analogue translate to it, rather
        // than to `\s` plus a stray class character.
        assert_eq!(emacs_regexp_to_rust(r"\s-").as_deref(), Some(r"\s"));
        assert_eq!(emacs_regexp_to_rust(r"\sw").as_deref(), Some(r"\w"));
        assert_eq!(emacs_regexp_to_rust(r"\S-").as_deref(), Some(r"\S"));
        assert_eq!(emacs_regexp_to_rust(r"\Sw").as_deref(), Some(r"\W"));

        // Everything the `regex` crate cannot express is refused outright — a
        // backreference above all, which a finite automaton cannot support.
        for pat in [
            r"\(a\)\1",
            r"\s_",
            r"\s(",
            r"\.\s.",
            r"\cg",
            r"\Cg",
            r"\s",
            r"foo\",
        ] {
            assert_eq!(emacs_regexp_to_rust(pat), None, "{pat} must be refused");
        }

        // And the refusal is what the caller sees: no matcher, so the search
        // reports a bad pattern instead of matching the wrong text.
        assert!(Dired::isearch_matcher(r"\(a\)\1", true).is_none());
        assert!(Dired::isearch_matcher(r"\(a\)\1", false).is_some(), "literal");
    }

    /// `M-s a C-s` is `dired-do-isearch`, a LITERAL search over the marked files;
    /// only `M-s a C-M-s` (`dired-do-isearch-regexp`) reads a regexp. Both keys
    /// used to arm the same regexp read under the same "(regexp)" prompt, so a
    /// `.` or `*` typed at the literal prompt matched something other than itself.
    #[test]
    fn the_contents_isearch_is_literal_and_only_c_m_s_is_a_regexp() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("a.txt"), b"x").unwrap();
        let mut d = Dired::new(tmp.path().to_path_buf()).unwrap();

        d.begin_contents_search(false);
        let inp = d.input.take().unwrap();
        assert_eq!(inp.prompt, "Search marked files: ");
        assert!(matches!(inp.action, Pending::FindRegexp { regexp: false, .. }));

        d.begin_contents_search(true);
        let inp = d.input.take().unwrap();
        assert_eq!(inp.prompt, "Search marked files (regexp): ");
        assert!(matches!(inp.action, Pending::FindRegexp { regexp: true, .. }));

        // The literal read reaches grep as FIXED STRINGS. Backslash-quoting the
        // pattern would not do: grep's default BRE reads `\+`/`\?`/`\|` as
        // operators, so the quoting would introduce the metacharacter it removed.
        assert!(find_grep_flags(false).contains(&"-F"));
        assert!(!find_grep_flags(true).contains(&"-F"));
    }
}

#[cfg(test)]
mod find_dired_tests {
    use super::*;

    /// `find-dired` produces a DIRED listing over the hits, not a text dump: the
    /// rows are real entries named relative to the buffer's directory, so every
    /// file op (`dir.join(name)`) resolves, and re-reading drops what has gone.
    #[test]
    fn find_results_become_an_operable_listing() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir(root.join("sub")).unwrap();
        std::fs::write(root.join("sub").join("a.txt"), b"aa").unwrap();
        std::fs::write(root.join("b.txt"), b"b").unwrap();

        // What `find . -name '*.txt'` prints, with the `./` prefix and the `.`
        // row find always emits.
        let paths = find_paths(".\n./sub/a.txt\n./b.txt\n");
        assert_eq!(paths, vec!["sub/a.txt".to_string(), "b.txt".to_string()]);

        let mut d = Dired::new(root.to_path_buf()).unwrap();
        d.enter_found(paths);
        let names: Vec<&str> = d.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["b.txt", "sub/a.txt"], "sorted, prefixed rows");
        // The row names still address the files.
        assert!(d.dir.join(&d.entries[1].name).is_file());
        assert_eq!(d.entries[0].size, 1);
        assert_eq!(d.entries[1].size, 2);

        // Re-reading a result listing re-stats those paths instead of scanning
        // the directory: the deleted hit goes, and `sub` (never a hit) stays out.
        std::fs::remove_file(root.join("sub").join("a.txt")).unwrap();
        d.read_dir().unwrap();
        let names: Vec<&str> = d.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["b.txt"]);
    }
}

#[cfg(test)]
mod paging_tests {
    use super::*;

    /// 20 named rows, and a window `viewport` rows tall, so the page step is a
    /// known `viewport - 2`.
    fn listing(root: &Path, viewport: usize) -> Dired {
        for i in 0..20 {
            std::fs::write(root.join(format!("f{i:02}")), b"x").unwrap();
        }
        let mut d = Dired::new(root.to_path_buf()).unwrap();
        d.viewport = viewport;
        assert_eq!(d.entries.len(), 20);
        d
    }

    /// mg `dired-scroll-down` (`C-v`): the window top advances by `w_ntrows - 2`
    /// (basic.c `forwpage`), and point, left behind by that jump, is dragged to
    /// the new top line.
    #[test]
    fn page_forward_advances_the_window_by_a_page_less_the_overlap() {
        let tmp = tempfile::tempdir().unwrap();
        let mut d = listing(tmp.path(), 10);

        d.scroll_page(true);
        assert_eq!(d.scroll, 8, "page step is viewport - 2");
        assert_eq!(d.selected, 8, "point followed to the new top line");

        d.scroll_page(true);
        assert_eq!((d.scroll, d.selected), (16, 16));
    }

    /// `forwpage` refuses to step the top past the last line: it prints
    /// "End of buffer" and leaves the window exactly where it was.
    #[test]
    fn page_forward_stops_at_the_end_of_the_buffer() {
        let tmp = tempfile::tempdir().unwrap();
        let mut d = listing(tmp.path(), 10);
        d.scroll = 16;
        d.selected = 16;

        d.scroll_page(true);
        assert_eq!((d.scroll, d.selected), (16, 16), "window did not move");
        assert_eq!(d.message.as_deref(), Some("End of buffer"));
    }

    /// Both page commands leave point alone while the scroll kept it on screen —
    /// mg only moves the dot when the new window no longer contains it.
    #[test]
    fn paging_leaves_a_still_visible_point_alone() {
        let tmp = tempfile::tempdir().unwrap();
        let mut d = listing(tmp.path(), 10);
        d.selected = 9;

        d.scroll_page(true);
        assert_eq!(d.scroll, 8);
        assert_eq!(d.selected, 9, "row 9 is still inside rows 8..=17");
    }

    /// mg `dired-scroll-up` (`M-v`): `backpage` moves the top back a page and
    /// pulls point to the LAST visible row when the scroll left it below.
    #[test]
    fn page_back_pulls_point_to_the_last_visible_row() {
        let tmp = tempfile::tempdir().unwrap();
        let mut d = listing(tmp.path(), 10);
        d.scroll = 16;
        d.selected = 19;

        d.scroll_page(false);
        assert_eq!(d.scroll, 8);
        assert_eq!(d.selected, 17, "bottom of rows 8..=17");
    }

    /// `backpage` at the top of the listing reports "Beginning of buffer" rather
    /// than scrolling into nothing.
    #[test]
    fn page_back_stops_at_the_beginning_of_the_buffer() {
        let tmp = tempfile::tempdir().unwrap();
        let mut d = listing(tmp.path(), 10);
        d.selected = 3;

        d.scroll_page(false);
        assert_eq!((d.scroll, d.selected), (0, 3));
        assert_eq!(d.message.as_deref(), Some("Beginning of buffer"));
    }

    /// A window too short for mg's two-line overlap still pages, one line at a
    /// time (`n <= 0` -> `n = 1` in `forwpage`).
    #[test]
    fn a_window_with_no_room_for_the_overlap_pages_one_line() {
        let tmp = tempfile::tempdir().unwrap();
        let mut d = listing(tmp.path(), 2);

        d.scroll_page(true);
        assert_eq!(d.scroll, 1);
    }
}

#[cfg(test)]
mod shell_command_tests {
    use super::*;

    /// mg's `d_exec` hands the child one pipe for both streams
    /// (dired.c:711-712), so `*Shell Command Output*` holds stderr as well as
    /// stdout — the point of `!` on a command that fails.
    #[test]
    fn stdout_and_stderr_are_captured_through_one_pipe() {
        let mut cmd = std::process::Command::new("/bin/sh");
        cmd.arg("-c").arg("echo out; echo err 1>&2; exit 3");
        let out = merged_output(cmd).unwrap();
        assert!(out.text.contains("out"), "stdout captured: {:?}", out.text);
        assert!(out.text.contains("err"), "stderr captured: {:?}", out.text);
        assert_eq!(out.status, Some(3));
    }

    /// mg pipes the file at point in on stdin (`dup2(infd, STDIN_FILENO)`), so a
    /// command that reads standard input sees the file's bytes.
    #[test]
    fn the_target_file_is_the_childs_standard_input() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("a.txt");
        std::fs::write(&path, b"one\ntwo\n").unwrap();

        let mut cmd = std::process::Command::new("/bin/sh");
        cmd.arg("-c")
            .arg("cat")
            .stdin(std::fs::File::open(&path).unwrap());
        assert_eq!(merged_output(cmd).unwrap().text, "one\ntwo\n");
    }

    /// Emacs `dired-mark-prompt` (dired.el:4167): the file's own name when one
    /// file is targeted, `* [N files]` when several are.
    #[test]
    fn mark_prompt_names_the_targets_the_way_emacs_does() {
        assert_eq!(mark_prompt(&["foo.txt".to_string()]), "foo.txt");
        assert_eq!(
            mark_prompt(&["a".to_string(), "b".to_string(), "c".to_string()]),
            "* [3 files]"
        );
    }
}
