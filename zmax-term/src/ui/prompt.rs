use crate::compositor::{Component, Compositor, Context, Event, EventResult};
use crate::{alt, ctrl, key, shift, ui};
use arc_swap::ArcSwap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::{borrow::Cow, ops::RangeFrom};
use tui::buffer::Buffer as Surface;
use tui::text::Span;
use tui::widgets::{Block, Widget};
use zmax_core::syntax;
use zmax_view::document::Mode;
use zmax_view::input::{KeyEvent, MouseButton, MouseEventKind};
use zmax_view::keyboard::KeyCode;

use zmax_core::{
    chars::{literal_code_char, LiteralRadix},
    search::{self, IsearchFlags},
    selection::Selection,
    unicode::segmentation::{GraphemeCursor, UnicodeSegmentation},
    unicode::width::UnicodeWidthStr,
    Position,
};
use zmax_stdx::rope::{self, RopeSliceExt};
use zmax_view::{
    graphics::{CursorKind, Margin, Rect},
    info::Info,
    Editor,
};

type PromptCharHandler = Box<dyn Fn(&mut Prompt, char, &Context)>;

pub type Completion = (RangeFrom<usize>, Span<'static>);
type CompletionFn = Box<dyn FnMut(&Editor, &str) -> Vec<Completion>>;
type CallbackFn = Box<dyn FnMut(&mut Context, &str, PromptEvent)>;
pub type DocFn = Box<dyn Fn(&str) -> Option<Cow<str>>>;

pub struct Prompt {
    prompt: Cow<'static, str>,
    line: String,
    cursor: usize,
    // Fields used for Component callbacks and rendering:
    line_area: Rect,
    anchor: usize,
    truncate_start: bool,
    truncate_end: bool,
    // ---
    completion: Vec<Completion>,
    selection: Option<usize>,
    history_register: Option<char>,
    history_pos: Option<usize>,
    completion_fn: CompletionFn,
    callback_fn: CallbackFn,
    pub doc_fn: DocFn,
    next_char_handler: Option<PromptCharHandler>,
    language: Option<(&'static str, Arc<ArcSwap<syntax::Loader>>)>,
    /// Last text removed by a kill (C-w/C-k/C-u/M-d), for readline `C-y` yank.
    kill: String,
    /// vim incsearch: `C-g`/`C-t` cycle to the next/prev match while typing a
    /// search. `(cx, current_input, forward)`. `None` for non-search prompts.
    #[allow(clippy::type_complexity)]
    incsearch_cycle: Option<Box<dyn FnMut(&mut Context, &str, bool)>>,
    /// vim `c_<Insert>`: overstrike (replace) instead of insert. Toggled by
    /// `<Insert>`, and reset for every new prompt.
    overstrike: bool,
    /// vim `'digraph'`: a `<BS>` armed `{char1}<BS>{char2}` entry, and this is
    /// char1, waiting for char2. Held on the prompt rather than on the editor
    /// (where the Insert-mode half keeps it) because a prompt is modal — an
    /// abandoned command line must not leave a half-entered digraph armed for
    /// whatever the user types next.
    digraph_pending: Option<char>,
    /// vim `c_CTRL-\`: `CTRL-\` was typed and the next key decides what it means
    /// (`CTRL-N`/`CTRL-G` abandon the command line).
    pending_ctrl_backslash: bool,
    /// vim `c_CTRL-R`: `CTRL-R` was typed and the register to insert is still to
    /// come. The `CTRL-R`/`CTRL-O`/`CTRL-P` variants only reassert that the insert
    /// is literal — which it always is here — so they leave this pending.
    pending_register: bool,
    /// vim `c_CTRL-V`: `CTRL-V` was typed, so the next key goes in literally.
    pending_literal: bool,
    /// vim `c_CTRL-V {number}`: a character code is being typed after `CTRL-V`
    /// (`CTRL-V 065` → `A`), with the digits collected so far.
    literal_code: Option<(LiteralRadix, String)>,
    /// Emacs `read-passwd`: echo `*` instead of what is typed. Used by
    /// `comint-send-invisible`, which must not put a password on screen.
    masked: bool,
    /// vim `wildmode`: how many times completion has been asked for on the
    /// current line. vim's `wildmode` is a comma list of what each successive
    /// press does (`longest:full` = first press completes the common prefix, the
    /// next cycles), so the press count picks the action. Reset by every edit.
    wild_press: usize,
    /// Emacs incremental search: the toggles that decide what the typed string
    /// means — regexp or literal (`M-r`), word (`M-s w`), symbol (`M-s _`),
    /// character folding (`M-s '`), lax whitespace (`M-s SPC`), and whether a
    /// match hidden in a closed fold opens it (`M-s i`). `None` in every prompt
    /// that is not a search, where none of the isearch keys exist.
    isearch: Option<IsearchFlags>,
    /// The direction the incremental search was started in (`/` forward, `?`
    /// backward), so `C-s`/`C-r` repeat forward/backward whichever way it began.
    isearch_forward: bool,
    /// dte's `M-r` (`A-R` here — `A-r` is the regexp toggle): the search in
    /// flight has been turned round, so it runs against
    /// [`Prompt::isearch_forward`] rather than with it until the key is pressed
    /// again. The flip is applied through the incsearch cycle, which is the only
    /// thing that can move the search the other way from here.
    isearch_reversed: bool,
    /// Emacs `isearch-toggle-case-fold` (`M-c`, `M-s c`): forces case folding on
    /// or off for this search. `None` until the key is pressed, so an untouched
    /// search still uses the editor's smart-case setting.
    isearch_case: Option<bool>,
    /// Emacs isearch `M-s`: the prefix of the search-toggle map — the next key
    /// says which toggle (`M-s r`, `M-s c`, `M-s i`, `M-s o`, `M-s C-e`, …).
    pending_isearch_s: bool,
    /// Emacs's `isearch-success`: whether what is typed is currently found. A
    /// failing search's `C-g` only rubs out what made it fail, so it takes
    /// `C-g C-g` to leave one.
    isearch_success: bool,
    /// The last search string that *was* found — where a failing search's `C-g`
    /// rubs back to (Emacs pops isearch states until one succeeded).
    isearch_found: String,
    /// Emacs isearch `C-h` (the help key): the prefix of `isearch-help-map` —
    /// the key after it picks which help to show (`b`, `k`, `m`, `q`, `C-h`).
    pending_ctrl_h: bool,
    /// Emacs `isearch-describe-key` (`C-h k`): the key sequence to describe is
    /// still being read, with the prefix keys of it read so far. The next key is
    /// documentation to look up, not a command to run.
    describe_key: Option<String>,
    /// Whether the isearch help box is the one on screen, so the next key can
    /// take it down again — Emacs's `*Help*` window goes when the search moves on.
    isearch_help: bool,
    /// Emacs `isearch-edit-string` (`M-e`, `Mouse-1` on the search prompt): the
    /// search string is being *edited*, so the search does not run as it is
    /// typed. `RET` resumes the incremental search with what the editing made
    /// of it.
    isearch_edit: bool,
    /// Emacs's `minibuffer-depth`: how many prompts were live when this one
    /// opened, which `minibuffer-depth-indicate-mode` shows as a `[N]` prefix.
    depth: usize,
    /// Emacs minibuffer `C-x`: the prefix of `C-x UP` (complete from the history)
    /// and `C-x DOWN` (complete from the prompt's default).
    pending_ctrl_x: bool,
    /// `C-x 8` inside a search: emacs's insert-char prefix, which
    /// `isearch-mode-map` keeps live so `C-x 8 RET` adds a character by name and
    /// `C-x 8 e RET` adds an emoji by name. `Some(false)` is `C-x 8`,
    /// `Some(true)` is `C-x 8 e`.
    pending_ctrl_x_8: Option<bool>,
    /// vim `c_CTRL-\_e {expr}`: the command line set aside while the nested `=`
    /// prompt asks for the expression. `Some` for as long as what is typed is
    /// the expression rather than the command line — `Enter` evaluates it and
    /// the result becomes the command line, `Esc` puts the saved one back.
    cmdline_eval: Option<CmdlineEval>,
    /// Emacs `previous-matching-history-element` (`M-r`) /
    /// `next-matching-history-element` (`M-s`): the minibuffer set aside while the
    /// *recursive* minibuffer reads the regexp to search the history for. `Some`
    /// for as long as what is typed is that regexp rather than the answer —
    /// `Enter` runs the search, `Esc` puts the answer back untouched.
    history_search_read: Option<HistorySearchRead>,
    /// Emacs `isearch-yank-pop-only` (`M-y`): the byte length the last
    /// `isearch-yank-kill` / `isearch-yank-pop-only` appended to the search
    /// string. This is Emacs's `last-command` check — `M-y` only replaces a kill
    /// that is still sitting at the end of the line, and yanks afresh otherwise.
    isearch_yank_len: Option<usize>,
}

/// vim `c_CTRL-\_e`: the command line put aside by the nested `=` prompt, so it
/// can come back on `Esc` and be read by the expression through `getcmdline()`.
struct CmdlineEval {
    /// The command line as it stood when `CTRL-\ e` was typed.
    line: String,
    /// The cursor's byte index in that line (vim's `getcmdpos()` minus one).
    cursor: usize,
    /// The prompt string `=` replaced (`:`, `/`, …), restored with the line.
    prompt: Cow<'static, str>,
}

/// Emacs `previous-matching-history-element` / `next-matching-history-element`:
/// the minibuffer put aside while the recursive one reads the regexp. Emacs binds
/// `enable-recursive-minibuffers` to `t` for that read (simple.el), so whatever
/// the read does, the minibuffer underneath comes back.
struct HistorySearchRead {
    /// The answer being typed when `M-r` / `M-s` was pressed.
    line: String,
    /// Its cursor, so an abandoned search leaves the answer exactly as it was.
    cursor: usize,
    /// The prompt string the "… element matching regexp" one replaced.
    prompt: Cow<'static, str>,
    /// `M-r` walks to older entries, `M-s` to newer ones.
    backward: bool,
    /// Emacs's `(prefix-numeric-value current-prefix-arg)`: how many matching
    /// entries to walk. The key gives 1; the command passes its count.
    count: usize,
}

/// Emacs's `minibuffer-history-search-history`: the regexps `M-r` / `M-s` have
/// been given. Only its newest entry is ever read back — it is the default the
/// recursive read offers and the one empty input reuses — so that is all this
/// keeps. Process-global, as the Emacs variable is.
static HISTORY_SEARCH_REGEXP: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

/// The newest `minibuffer-history-search-history` entry, `""` when there is none.
fn last_history_search_regexp() -> String {
    HISTORY_SEARCH_REGEXP
        .lock()
        .map(|regexp| regexp.clone())
        .unwrap_or_default()
}

/// Add a regexp to `minibuffer-history-search-history`.
fn push_history_search_regexp(regexp: &str) {
    if let Ok(mut last) = HISTORY_SEARCH_REGEXP.lock() {
        last.clear();
        last.push_str(regexp);
    }
}

/// The prompt the regexp is read with, as `format-prompt` builds it in simple.el:
/// "Previous element matching regexp" going back, "Next …" going on, then
/// ` (default REGEXP)` when there is a last regexp — which is what empty input
/// reuses — and the `": "` every minibuffer prompt ends in.
fn history_search_prompt(backward: bool, default: &str) -> String {
    format!(
        "{} element matching regexp{}: ",
        if backward { "Previous" } else { "Next" },
        if default.is_empty() {
            String::new()
        } else {
            format!(" (default {default})")
        }
    )
}

/// Where point goes in the entry a matching-history-element search found: Emacs
/// matches `".*\(REGEXP\)"` going back, so the start of the **last** match on the
/// entry, and plain `REGEXP` going on, so the end of the **first** one
/// (simple.el). An entry that does not match at all leaves point at its end.
fn matching_history_point(regex: &regex::Regex, entry: &str, backward: bool) -> usize {
    if backward {
        regex
            .find_iter(entry)
            .last()
            .map_or(entry.len(), |m| m.start())
    } else {
        regex.find(entry).map_or(entry.len(), |m| m.end())
    }
}

/// The toggles an incremental search starts with. zmax's `/` is a regexp
/// search — Emacs's starts literal, and `M-r` toggles between the two either way
/// — and it leaves case and whitespace to the editor's own settings until a key
/// says otherwise.
///
/// `invisible` starts on, which is both Emacs's default (`search-invisible` is
/// `open`: a match in invisible text is found and what hides it opened) and vim's
/// (`'foldopen'` contains `search`). `M-s i` turns it off, and then a match a
/// closed fold hides is not found at all.
const ISEARCH_START: IsearchFlags = IsearchFlags {
    regexp: true,
    word: false,
    symbol: false,
    case_fold: true,
    lax_whitespace: false,
    char_fold: false,
    invisible: true,
};

/// The mode toggles of Emacs's isearch (`M-r` and the `M-s` map). Emacs makes the
/// pattern modes mutually exclusive (`isearch-define-mode-toggle`): turning one
/// on turns the others off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IsearchToggle {
    Regexp,
    Word,
    Symbol,
    CharFold,
    LaxWhitespace,
    Invisible,
}

/// The "selection the last yank left behind" that [`crate::emacs_kill`] gates its
/// ring cycling on. A search string is not a buffer selection, so the isearch
/// yanks hand it this sentinel instead: the prompt does Emacs's `last-command`
/// check itself (`isearch_yank_len`) and only needs the ring's *pointer*, which
/// Emacs shares between `yank-pop` and `isearch-yank-pop-only` too.
pub(crate) const ISEARCH_YANK_SEL: &[(usize, usize)] = &[(usize::MAX, usize::MAX)];

/// What an `isearch-yank-*` key grabs from the buffer at the end of the match.
#[derive(Debug, Clone, Copy)]
enum IsearchYank {
    Char,
    WordOrChar,
    Line,
}

// ── Emacs recursive minibuffers ─────────────────────────────────────────────
// Emacs lets a minibuffer be read while another one is already being read, and
// `minibuffer-depth-indicate-mode` puts the depth of that recursion in front of
// the prompt so it is clear which one is being answered. A zmax prompt is a
// compositor layer, so the depth is how many of them are live.

/// Emacs `minibuffer-depth-indicate-mode`: whether a recursive minibuffer says
/// how deep it is. Off, as the mode is in Emacs until it is turned on.
static DEPTH_INDICATE: AtomicBool = AtomicBool::new(false);

/// Emacs's `minibuffer-depth`: how many prompts are live. A prompt opened while
/// another is still on the compositor stack is the recursive one.
static PROMPT_DEPTH: AtomicUsize = AtomicUsize::new(0);

/// Emacs `minibuffer-depth-indicate-mode`: turn the `[N]` depth prefix on
/// recursive minibuffers on or off. Returns the new state.
pub fn minibuffer_depth_indicate_mode() -> bool {
    !DEPTH_INDICATE.fetch_xor(true, Ordering::Relaxed)
}

// ── Emacs minibuffer display modes ──────────────────────────────────────────
// Three global minor modes that change what a minibuffer *looks* like rather
// than what its keys do. All three are off until turned on, as they are in
// Emacs, and all three are process-global because the Emacs modes are.

/// Emacs `fido-mode`: the ido-flavoured `icomplete-mode` — `RET` takes the top
/// candidate rather than what is literally typed (`icomplete-fido-ret`).
static FIDO_MODE: AtomicBool = AtomicBool::new(false);

/// Emacs `minibuffer-electric-default-mode`: the prompt's `(default X)` segment
/// shows only while the input is still the empty one the prompt opened with.
static ELECTRIC_DEFAULT: AtomicBool = AtomicBool::new(false);

/// Emacs `file-name-shadow-mode`: the leading part of a typed file name that the
/// name's own later components make irrelevant is dimmed out.
static FILE_NAME_SHADOW: AtomicBool = AtomicBool::new(false);

/// Emacs `icomplete-mode`: the matching candidates are shown *on the prompt
/// line* as `{a | b | c}` while you type, instead of in a list above it.
static ICOMPLETE_MODE: AtomicBool = AtomicBool::new(false);

/// Emacs `icomplete-vertical-mode`: the same candidates, one per line.
static ICOMPLETE_VERTICAL: AtomicBool = AtomicBool::new(false);

/// Emacs `fido-mode`: toggle it. Returns the new state.
pub fn fido_mode() -> bool {
    !FIDO_MODE.fetch_xor(true, Ordering::Relaxed)
}

/// Emacs `icomplete-mode`: toggle it. Returns the new state.
pub fn icomplete_mode() -> bool {
    !ICOMPLETE_MODE.fetch_xor(true, Ordering::Relaxed)
}

/// Emacs `icomplete-vertical-mode`: toggle it. Returns the new state.
///
/// Turning it on turns `icomplete-mode` on when neither it nor `fido-mode` is
/// already on — vertical display is a property of icomplete, not a mode of its
/// own (icomplete.el:702, "If none of these modes are on, turn on
/// `icomplete-mode'").
pub fn icomplete_vertical_mode() -> bool {
    let on = !ICOMPLETE_VERTICAL.fetch_xor(true, Ordering::Relaxed);
    if on && !icomplete_enabled() {
        ICOMPLETE_MODE.store(true, Ordering::Relaxed);
    }
    on
}

/// Whether candidates display the icomplete way. `fido-mode` is icomplete with
/// ido flavouring, so it implies it.
fn icomplete_enabled() -> bool {
    ICOMPLETE_MODE.load(Ordering::Relaxed) || FIDO_MODE.load(Ordering::Relaxed)
}

/// Whether the icomplete display is the vertical one.
fn icomplete_vertical_enabled() -> bool {
    ICOMPLETE_VERTICAL.load(Ordering::Relaxed) && icomplete_enabled()
}

/// `icomplete-separator` (icomplete.el:61).
const ICOMPLETE_SEPARATOR: &str = " | ";

/// `icomplete-prospects-height` (icomplete.el:119): how many lines of the
/// minibuffer the prospects may fill.
const ICOMPLETE_PROSPECTS_HEIGHT: usize = 2;

/// The ellipsis icomplete uses when it truncates (icomplete.el:998).
const ICOMPLETE_ELLIPSIS: &str = "…";

/// Build icomplete's prospects string — the port of `icomplete-completions`
/// (icomplete.el:933).
///
/// `name` is what has been typed, `comps` the candidates matching it in display
/// order, `require_match` whether the prompt refuses a non-candidate (which is
/// the only thing that changes the brackets), and `width` the room available.
///
/// The shape, straight from the source:
///
/// - no candidates → `" [No matches]"`;
/// - one candidate, or the typed text already completes uniquely → `determ [Matched]`;
/// - otherwise → `determ{a | b | c…}`.
///
/// `determ` is the part of the unique completion that typing has not produced
/// yet, in brackets — `[foo]` after typing `f` when every candidate starts
/// `foo`. It is absent when there is nothing to add. Candidates then have that
/// common prefix stripped (`icomplete-hide-common-prefix`, icomplete.el:66), so
/// the list shows what distinguishes them rather than repeating the prefix.
pub(crate) fn icomplete_completions(
    name: &str,
    comps: &[String],
    require_match: bool,
    width: usize,
) -> String {
    let (open, close) = if require_match {
        ("(", ")")
    } else {
        ("[", "]")
    };
    if comps.is_empty() {
        return format!(" {open}No matches{close}");
    }

    // `most`: what completing the typed text yields — the candidates' common
    // prefix. `most_try == t` in the source, i.e. the typed text is already the
    // whole (unique) completion.
    let most = common_prefix(comps);
    let most_is_exact = comps.len() == 1 && comps[0] == name;

    // `compare`: how much of `name` and `most` agree. The source works in
    // 1-based `compare-strings` terms and immediately subtracts one; this is
    // that index directly.
    let agree = name
        .chars()
        .zip(most.chars())
        .take_while(|(a, b)| a == b)
        .count();
    let name_len = name.chars().count();
    let most_len = most.chars().count();

    let determ = if most_is_exact || name == most || agree == most_len {
        // Nothing to add to what was typed.
        String::new()
    } else {
        let tail: String = if agree == name_len {
            // The typical case: what was typed is a prefix of the completion.
            most.chars().skip(agree).collect()
        } else if agree < 2 + ICOMPLETE_ELLIPSIS.chars().count() {
            // Truncating would not gain two columns, so do not.
            most.clone()
        } else {
            format!(
                "{ICOMPLETE_ELLIPSIS}{}",
                most.chars().skip(agree).collect::<String>()
            )
        };
        format!("{open}{tail}{close}")
    };

    if most_is_exact || comps.len() == 1 {
        return format!("{determ} [Matched]");
    }

    // The typed text is a candidate but not a unique one: show an empty bracket
    // pair as the visual cue the source describes (icomplete.el:1047-1065),
    // since hiding the common prefix would otherwise leave it invisible.
    let determ = if determ.is_empty() && comps.iter().any(|c| c == name) {
        format!("{open}{close}")
    } else {
        determ
    };

    // Candidates are shown with their common prefix removed, but only when that
    // prefix is already on screen as part of `most`.
    let prefix_len = most_len.min(common_prefix(comps).chars().count());

    let width = width.max(1);
    let mut prospects_len = display_width(&determ).max(display_width(&format!("{open}{close}")))
        + display_width(ICOMPLETE_SEPARATOR)
        + 2
        + display_width(ICOMPLETE_ELLIPSIS)
        + display_width(name);
    let prospects_max = (ICOMPLETE_PROSPECTS_HEIGHT + prospects_len / width) * width;

    let mut prospects: Vec<String> = Vec::new();
    let mut limit = false;
    for comp in comps {
        let shown: String = comp.chars().skip(prefix_len).collect();
        prospects_len += display_width(&shown) + display_width(ICOMPLETE_SEPARATOR);
        if prospects_len < prospects_max {
            prospects.push(shown);
        } else {
            limit = true;
            break;
        }
    }

    let tail = if limit {
        format!("{ICOMPLETE_SEPARATOR}{ICOMPLETE_ELLIPSIS}")
    } else {
        String::new()
    };
    format!("{determ}{{{}{tail}}}", prospects.join(ICOMPLETE_SEPARATOR))
}

/// The longest common prefix of `strings` — Emacs's `try-completion ""`.
fn common_prefix(strings: &[String]) -> String {
    let mut iter = strings.iter();
    let Some(first) = iter.next() else {
        return String::new();
    };
    let mut len = first.chars().count();
    for s in iter {
        len = len.min(
            first
                .chars()
                .zip(s.chars())
                .take_while(|(a, b)| a == b)
                .count(),
        );
    }
    first.chars().take(len).collect()
}

/// Columns a string occupies on screen.
fn display_width(s: &str) -> usize {
    s.width()
}

/// Emacs `minibuffer-electric-default-mode`: toggle it. Returns the new state.
pub fn minibuffer_electric_default_mode() -> bool {
    !ELECTRIC_DEFAULT.fetch_xor(true, Ordering::Relaxed)
}

/// Emacs `file-name-shadow-mode`: toggle it. Returns the new state.
pub fn file_name_shadow_mode() -> bool {
    !FILE_NAME_SHADOW.fetch_xor(true, Ordering::Relaxed)
}

impl Drop for Prompt {
    /// The depth the mode shows is how many prompts are live, so a prompt
    /// leaving the compositor takes its level back off.
    fn drop(&mut self) {
        PROMPT_DEPTH.fetch_sub(1, Ordering::Relaxed);
    }
}

/// The keys of Emacs's `isearch-mode-map` that are live in this prompt, in the
/// spelling Emacs writes them in. This is the body of `isearch-describe-bindings`
/// (`C-h b`) and the table `isearch-describe-key` (`C-h k`) looks a key up in.
const ISEARCH_BINDINGS: &[(&str, &str)] = &[
    ("C-s", "isearch-repeat-forward"),
    ("C-r", "isearch-repeat-backward"),
    ("C-w", "isearch-yank-word-or-char"),
    ("C-y", "isearch-yank-kill"),
    ("M-y", "isearch-yank-pop-only"),
    ("C-q", "isearch-quote-char"),
    ("C-g", "isearch-abort"),
    ("C-h", "isearch-help-map"),
    ("C-M-y", "isearch-yank-char"),
    ("C-M-w", "isearch-yank-symbol-or-char"),
    ("C-M-d", "isearch-del-char"),
    ("C-M-z", "isearch-yank-until-char"),
    ("C-x \\", "isearch-transient-input-method"),
    ("C-x 8 RET", "isearch-char-by-name"),
    ("C-x 8 e RET", "isearch-emoji-by-name"),
    ("DEL", "isearch-delete-char"),
    ("RET", "isearch-exit"),
    ("M-c", "isearch-toggle-case-fold"),
    ("M-e", "isearch-edit-string"),
    ("M-r", "isearch-toggle-regexp"),
    ("M-TAB", "isearch-complete"),
    ("M-s r", "isearch-toggle-regexp"),
    ("M-s w", "isearch-toggle-word"),
    ("M-s _", "isearch-toggle-symbol"),
    ("M-s c", "isearch-toggle-case-fold"),
    ("M-s i", "isearch-toggle-invisible"),
    ("M-s '", "isearch-toggle-char-fold"),
    ("M-s SPC", "isearch-toggle-lax-whitespace"),
    ("M-s C-e", "isearch-yank-line"),
    ("M-s o", "isearch-occur"),
    ("M-s M-<", "isearch-beginning-of-buffer"),
    ("M-s M->", "isearch-end-of-buffer"),
    ("up", "isearch-ring-retreat"),
    ("down", "isearch-ring-advance"),
];

/// The options `isearch-help-for-help` (`C-h C-h`) offers, which are the rest of
/// `isearch-help-map`.
const ISEARCH_HELP_OPTIONS: &[(&str, &str)] = &[
    ("b", "Display all Isearch key bindings"),
    ("k", "Display full documentation of Isearch key sequence"),
    ("m", "Display documentation of Isearch mode"),
    ("q", "Exit the Help command"),
];

/// What `isearch-describe-mode` (`C-h m`) shows: the documentation of the mode
/// the search is in, as opposed to the key list `C-h b` prints.
const ISEARCH_MODE_DOC: &str = "\
Incremental search: the buffer moves to the first match as the
string is typed, so the search is over as soon as enough of it
has been typed to find what is wanted.

C-s and C-r go on to the next match forward and backward; with
nothing typed yet they bring back the string searched for last.
RET stops on the match the search is showing, and C-g goes back
to where the search started — on a failing search the first C-g
rubs out only the characters that were not found, so C-g C-g is
what leaves one.
";

/// The keys the `M-s` map is entered by — a key sequence `isearch-describe-key`
/// is asked about can carry on after one of these.
const ISEARCH_PREFIXES: &[&str] = &["M-s", "C-x", "C-x 8", "C-x 8 e"];

/// A key in the spelling Emacs writes it in (`C-s`, `M-s`, `DEL`, `SPC`, `RET`),
/// which is how [`ISEARCH_BINDINGS`] names them. zmax's own `Display` spells
/// Meta `A-` and the named keys in its config syntax, so the two differ.
fn emacs_key_name(event: KeyEvent) -> String {
    use zmax_view::keyboard::KeyModifiers;
    let mut name = String::new();
    if event.modifiers.contains(KeyModifiers::CONTROL) {
        name.push_str("C-");
    }
    if event.modifiers.contains(KeyModifiers::ALT) {
        name.push_str("M-");
    }
    match event.code {
        KeyCode::Backspace => name.push_str("DEL"),
        KeyCode::Enter => name.push_str("RET"),
        KeyCode::Tab => name.push_str("TAB"),
        KeyCode::Esc => name.push_str("ESC"),
        KeyCode::Char(' ') => name.push_str("SPC"),
        KeyCode::Char(c) => name.push(c),
        code => name.push_str(
            &KeyEvent {
                code,
                modifiers: KeyModifiers::NONE,
            }
            .to_string(),
        ),
    }
    name
}

/// Emacs's `substitute-in-file-name`: resolve `$VAR` / `${VAR}` (and `$$` for a
/// literal `$`), and let a second absolute name inside the string throw away
/// everything typed in front of it. That last part is what `file-name-shadow-mode`
/// is about, so it is the part that has to be right.
fn substitute_in_file_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut chars = name.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '$' if chars.peek() == Some(&'$') => {
                chars.next();
                out.push('$');
            }
            '$' => {
                let braced = chars.peek() == Some(&'{');
                if braced {
                    chars.next();
                }
                let mut var = String::new();
                while let Some(&c) = chars.peek() {
                    if braced {
                        chars.next();
                        if c == '}' {
                            break;
                        }
                        var.push(c);
                    } else if c.is_alphanumeric() || c == '_' {
                        chars.next();
                        var.push(c);
                    } else {
                        break;
                    }
                }
                // The value goes through the same rules: an absolute one landing
                // after a separator restarts the name, as it would if typed.
                for c in std::env::var(&var).unwrap_or_default().chars() {
                    push_name_char(&mut out, c);
                }
            }
            c => push_name_char(&mut out, c),
        }
    }
    out
}

/// One character of a file name being resolved: a `/` straight after another `/`
/// starts the name over at root, and a `~` at the start of a component starts it
/// over at a home directory — everything before either is ignored.
fn push_name_char(out: &mut String, c: char) {
    match c {
        '/' if out.ends_with('/') => {
            out.clear();
            out.push('/');
        }
        '~' if out.is_empty() || out.ends_with('/') => {
            out.clear();
            out.push('~');
        }
        c => out.push(c),
    }
}

/// An info box of running text — Emacs's `*Help*` window — rather than the
/// key/description grid [`Info::new`] builds out of a binding list.
fn help_text(title: &'static str, body: &str) -> Info {
    Info {
        title: Cow::Borrowed(title),
        width: body.lines().map(|line| line.width()).max().unwrap_or(0) as u16,
        height: body.lines().count() as u16,
        text: body.to_string(),
        scroll: 0,
    }
}

/// Emacs `isearch-transient-input-method` (`C-x \`): what the input method makes
/// of one character. zmax's input method is the vim Lang-Arg (`:lmap`) table,
/// whose 'imsearch' switch is turned on for the single lookup and put straight
/// back — a transient method applies whether or not the method is on, and must
/// leave the search's own setting alone.
fn transient_lang_map(c: char) -> String {
    use crate::commands::typed::{lang_map_lookup, toggle_lang_arg};
    // The toggle reports the state it left behind, which is how the state it
    // started in is read without a getter for it.
    let was_off = toggle_lang_arg(false);
    if !was_off {
        toggle_lang_arg(false);
    }
    let text = lang_map_lookup(c, false);
    if was_off {
        toggle_lang_arg(false);
    }
    text.unwrap_or_else(|| c.to_string())
}

/// What one press of the completion key does, per vim `wildmode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WildAction {
    /// Insert the longest prefix every candidate shares, select none.
    Longest,
    /// Select (and insert) candidates one after another.
    Full,
    /// Only show the candidate list.
    ListOnly,
}

/// vim `wildmode`: what the `press`-th completion key does. The option is a comma
/// list — one entry per press, the last entry repeating — and each entry is a
/// colon list of `longest` / `list` / `full`. The default (`full`) selects
/// candidates in turn, which is what zmax's `<Tab>` has always done. Pure —
/// unit tested.
fn wildmode_action(value: &str, press: usize) -> WildAction {
    let items: Vec<&str> = value.split(',').collect();
    let item = items[press.min(items.len() - 1)];
    let flags: Vec<&str> = item.split(':').map(str::trim).collect();
    // `longest:full` completes the common prefix and *then* offers the menu, so
    // `longest` decides what the press does when both are named.
    if flags.contains(&"longest") {
        WildAction::Longest
    } else if flags.contains(&"full") {
        WildAction::Full
    } else if flags.contains(&"list") {
        WildAction::ListOnly
    } else {
        // An empty entry: complete the first match (vim `wildmode=`).
        WildAction::Full
    }
}

/// The action the next completion key press performs.
fn wild_action(press: usize) -> WildAction {
    match crate::commands::typed::vim_opt_str("wildmode") {
        Some(value) => wildmode_action(&value, press),
        None => WildAction::Full,
    }
}

/// vim `wildcharm`: the key that triggers command-line completion from inside a
/// mapping (`:set wildcharm=<C-z>`). vim stores it as a character code, and also
/// accepts the `<C-z>` and `^I` spellings. Pure — unit tested.
fn parse_wildcharm(value: &str) -> Option<KeyEvent> {
    use zmax_view::keyboard::KeyModifiers;
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let ctrl_key = |c: char| KeyEvent {
        code: KeyCode::Char(c),
        modifiers: KeyModifiers::CONTROL,
    };
    // A raw character code (`:set wildcharm=9` is <Tab>).
    if let Ok(code) = value.parse::<u32>() {
        let c = char::from_u32(code)?;
        return match c as u32 {
            9 => Some(key!(Tab)),
            // Control codes: 1 = CTRL-A … 26 = CTRL-Z.
            n if n < 27 => Some(ctrl_key(char::from_u32('a' as u32 + n - 1)?)),
            _ => Some(KeyEvent {
                code: KeyCode::Char(c),
                modifiers: KeyModifiers::NONE,
            }),
        };
    }
    // The `^I` spelling: a caret and the control letter.
    if let Some(letter) = value.strip_prefix('^') {
        let letter = letter.chars().next()?.to_ascii_lowercase();
        return match letter {
            'i' => Some(key!(Tab)),
            c if c.is_ascii_lowercase() => Some(ctrl_key(c)),
            _ => None,
        };
    }
    // The `<C-z>` / `<Tab>` spellings, which are zmax's own key syntax once the
    // angle brackets come off. Key *names* are lowercase there (`tab`), while
    // modifiers are uppercase (`C-`), so a name that fails is retried folded.
    let key = value.trim_start_matches('<').trim_end_matches('>');
    key.parse().ok().or_else(|| key.to_lowercase().parse().ok())
}

/// The key vim `wildcharm` is currently set to, if any.
fn wildcharm() -> Option<KeyEvent> {
    parse_wildcharm(&crate::commands::typed::vim_opt_str("wildcharm")?)
}

/// vim `wildoptions=pum`: show the completion candidates as a vertical popup menu
/// (one candidate per row) rather than zmax's multi-column list.
fn wildoptions_pum() -> bool {
    crate::commands::typed::vim_opt_str("wildoptions")
        .is_some_and(|opts| opts.split(',').any(|o| o.trim() == "pum"))
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PromptEvent {
    /// The prompt input has been updated.
    Update,
    /// Validate and finalize the change.
    Validate,
    /// Abort the change, reverting to the initial state.
    Abort,
}

pub enum CompletionDirection {
    Forward,
    Backward,
}

#[derive(Debug, Clone, Copy)]
pub enum Movement {
    BackwardChar(usize),
    BackwardWord(usize),
    ForwardChar(usize),
    ForwardWord(usize),
    StartOfLine,
    EndOfLine,
    None,
}

/// vim 'cedit': whether `event` is the key that opens the command-line window
/// from the command line. The option names the key (`CTRL-F` by default, empty
/// to turn it off) and `typed::cedit_key` parses it into `(needs_ctrl, char)`;
/// this is the other half — the comparison against what was actually typed.
fn cedit_pressed(event: KeyEvent) -> bool {
    let Some((needs_ctrl, key)) = crate::commands::typed::cedit_key() else {
        return false; // an empty 'cedit' is how vim turns the key off
    };
    let Some(c) = event.char() else {
        return false;
    };
    let ctrl = event
        .modifiers
        .contains(zmax_view::keyboard::KeyModifiers::CONTROL);
    ctrl == needs_ctrl && c.to_ascii_lowercase() == key
}

/// Run a static command from inside the prompt — the shape the `C-x 8` legs
/// need, since the commands they reach (`isearch-char-by-name`,
/// `isearch-emoji-by-name`) live in the command table and open prompts of their
/// own on top of this one.
fn run_isearch_command(_compositor: &mut Compositor, cx: &mut Context, name: &str) {
    let mut ccx = crate::commands::Context {
        register: None,
        count: None,
        editor: cx.editor,
        callback: Vec::new(),
        on_next_key_callback: None,
        jobs: cx.jobs,
    };
    match name.parse::<crate::commands::MappableCommand>() {
        Ok(command) => command.execute(&mut ccx),
        Err(err) => return cx.editor.set_error(err.to_string()),
    }
    for callback in std::mem::take(&mut ccx.callback) {
        callback(_compositor, cx);
    }
}

/// vim 'cedit': open the command-line window `name` (`q:`, `q/` or `q?`) and put
/// `line` — the command line as it was typed — on its last line, which is where
/// vim leaves it: "the command line is used to fill the last line of the window"
/// (cmdline.txt), with the cursor after it.
fn open_cmdline_window(compositor: &mut Compositor, cx: &mut Context, name: &str, line: &str) {
    let mut ccx = crate::commands::Context {
        register: None,
        count: None,
        editor: cx.editor,
        callback: Vec::new(),
        on_next_key_callback: None,
        jobs: cx.jobs,
    };
    match name.parse::<crate::commands::MappableCommand>() {
        Ok(command) => command.execute(&mut ccx),
        Err(err) => return cx.editor.set_error(err.to_string()),
    }
    for callback in std::mem::take(&mut ccx.callback) {
        callback(compositor, cx);
    }
    if line.is_empty() {
        return;
    }
    let (view, doc) = current!(cx.editor);
    let pos = doc
        .selection(view.id)
        .primary()
        .cursor(doc.text().slice(..));
    let transaction =
        zmax_core::Transaction::insert(doc.text(), doc.selection(view.id), line.into());
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    doc.set_selection(view.id, Selection::point(pos + line.chars().count()));
}

/// kakoune's `Quoting::Kakoune` quoter (`quote()`, string_utils.hh:74-77): the
/// value in single quotes with every single quote in it doubled, which is how
/// kakoune's command language spells a string that is one argument whatever it
/// contains. What the prompt's `<c-r>` puts on the line when the register key
/// carries Control.
fn kak_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn is_word_sep(c: char) -> bool {
    c == std::path::MAIN_SEPARATOR || c.is_whitespace()
}

/// `n` pulled back to a character boundary of `s`, and no further than its end.
/// A command line an expression replaced can be shorter than the cursor was.
fn clamp_to_boundary(s: &str, n: usize) -> usize {
    let mut n = n.min(s.len());
    while n > 0 && !s.is_char_boundary(n) {
        n -= 1;
    }
    n
}

impl Prompt {
    pub fn new(
        prompt: Cow<'static, str>,
        history_register: Option<char>,
        completion_fn: impl FnMut(&Editor, &str) -> Vec<Completion> + 'static,
        callback_fn: impl FnMut(&mut Context, &str, PromptEvent) + 'static,
    ) -> Self {
        Self {
            prompt,
            line: String::new(),
            cursor: 0,
            line_area: Rect::default(),
            anchor: 0,
            truncate_start: false,
            truncate_end: false,
            completion: Vec::new(),
            selection: None,
            history_register,
            history_pos: None,
            completion_fn: Box::new(completion_fn),
            callback_fn: Box::new(callback_fn),
            doc_fn: Box::new(|_| None),
            next_char_handler: None,
            language: None,
            kill: String::new(),
            incsearch_cycle: None,
            overstrike: false,
            digraph_pending: None,
            pending_ctrl_backslash: false,
            pending_register: false,
            pending_literal: false,
            literal_code: None,
            masked: false,
            wild_press: 0,
            isearch: None,
            isearch_forward: true,
            isearch_reversed: false,
            isearch_case: None,
            pending_isearch_s: false,
            isearch_success: true,
            isearch_found: String::new(),
            pending_ctrl_h: false,
            describe_key: None,
            isearch_help: false,
            isearch_edit: false,
            depth: PROMPT_DEPTH.fetch_add(1, Ordering::Relaxed) + 1,
            pending_ctrl_x: false,
            pending_ctrl_x_8: None,
            cmdline_eval: None,
            history_search_read: None,
            isearch_yank_len: None,
        }
    }

    /// Make this prompt an Emacs incremental search: the isearch keys (`C-s`,
    /// `C-w`, `C-y`, `M-r`, the `M-s` toggle map, …) come alive on top of the
    /// command-line editing keys. `forward` is the direction the search was
    /// started in, so `C-s`/`C-r` can repeat either way regardless of it.
    pub fn with_isearch(mut self, forward: bool) -> Self {
        self.isearch = Some(ISEARCH_START);
        self.isearch_forward = forward;
        self
    }

    /// Echo `*` instead of what is typed (Emacs `read-passwd`) — for
    /// `comint-send-invisible`, where the secret must not reach the screen.
    pub fn masked(mut self) -> Self {
        self.masked = true;
        self
    }

    /// Set the vim incsearch `C-g`/`C-t` cycle handler (next/prev match while typing).
    pub fn with_incsearch_cycle(
        mut self,
        f: impl FnMut(&mut Context, &str, bool) + 'static,
    ) -> Self {
        self.incsearch_cycle = Some(Box::new(f));
        self
    }

    /// Gets the byte index in the input representing the current cursor location.
    #[inline]
    pub(crate) fn position(&self) -> usize {
        self.cursor
    }

    pub fn with_line(mut self, line: String, editor: &Editor) -> Self {
        self.set_line(line, editor);
        self
    }

    pub fn set_line(&mut self, line: String, editor: &Editor) {
        let cursor = line.len();
        self.line = line;
        self.cursor = cursor;
        self.recalculate_completion(editor);
    }

    pub fn with_language(
        mut self,
        language: &'static str,
        loader: Arc<ArcSwap<syntax::Loader>>,
    ) -> Self {
        self.language = Some((language, loader));
        self
    }

    pub fn line(&self) -> &String {
        &self.line
    }

    /// Emacs `file-cache-minibuffer-complete` (`C-TAB`): complete the file name
    /// on this line from the file-name cache, and — once the name is unique —
    /// cycle through the directories it was cached in each time the command
    /// repeats. Returns the message Emacs would have shown in the minibuffer, or
    /// the `Err` half for `file-cache-no-match-message`.
    pub fn file_cache_complete(&mut self, editor: &Editor) -> Result<String, String> {
        use crate::file_cache::Completion;
        if crate::file_cache::len() == 0 {
            return Err("The file cache is empty (M-x file-cache-add-directory)".into());
        }
        match crate::file_cache::minibuffer_complete(&self.line) {
            Completion::Expanded { path, directory } => {
                self.set_line(path.clone(), editor);
                Ok(match directory {
                    Some((n, total)) => format!("{path} [{n} of {total}]"),
                    None => path,
                })
            }
            Completion::Ambiguous { prefix, matches } => {
                // Emacs extends the name as far as the candidates agree and says
                // the completion is not unique; the alternatives go in the
                // completion menu the prompt already draws.
                let dir = match self.line.rfind('/') {
                    Some(i) => self.line[..=i].to_string(),
                    None => String::new(),
                };
                self.set_line(format!("{dir}{prefix}"), editor);
                self.completion = matches
                    .iter()
                    .map(|name| (0.., format!("{dir}{name}").into()))
                    .collect();
                Ok(format!(
                    "[Complete, but not unique: {} matches]",
                    matches.len()
                ))
            }
            Completion::NoMatch => Err("[No match]".into()),
        }
    }

    pub fn with_history_register(&mut self, history_register: Option<char>) -> &mut Self {
        self.history_register = history_register;
        self
    }

    pub(crate) fn history_register(&self) -> Option<char> {
        self.history_register
    }

    pub(crate) fn first_history_completion<'a>(
        &'a self,
        editor: &'a Editor,
    ) -> Option<Cow<'a, str>> {
        self.history_register
            .and_then(|reg| editor.registers.first(reg, editor))
    }

    /// vim `wildmode`: one press of the completion key (`<Tab>`, or the
    /// `wildcharm` key). The first press does what the option's first entry says,
    /// the next what its second says, and so on — so `wildmode=longest:full`
    /// completes the shared prefix, then starts cycling. Any edit to the line
    /// resets the count (`recalculate_completion`).
    fn wild_complete(&mut self, editor: &Editor, direction: CompletionDirection) {
        let action = wild_action(self.wild_press);
        self.wild_press += 1;
        match action {
            WildAction::Longest => {
                self.complete_longest_common(editor);
            }
            WildAction::ListOnly => {
                // The candidates are already on screen; select none of them.
                self.exit_selection();
            }
            WildAction::Full => {
                self.change_completion_selection(direction);
                // If the single candidate is a directory, list what is inside it.
                if self.completion.len() == 1 && self.line.ends_with(std::path::MAIN_SEPARATOR) {
                    let press = self.wild_press;
                    self.recalculate_completion(editor);
                    self.wild_press = press;
                }
            }
        }
    }

    pub fn recalculate_completion(&mut self, editor: &Editor) {
        self.exit_selection();
        // Editing the line starts vim `wildmode` over from its first entry.
        self.wild_press = 0;
        // In the `=` prompt of `c_CTRL-\_e` the line is an expression, not the
        // command line, so the command line's candidates do not apply to it.
        self.completion = if self.in_nested_read() {
            Vec::new()
        } else {
            (self.completion_fn)(editor, &self.line)
        };
    }

    /// Whether what is being typed belongs to a *nested* read rather than to this
    /// prompt's own answer: vim's `=` expression line (`c_CTRL-\_e`) or the Emacs
    /// recursive minibuffer that `M-r` / `M-s` read a history regexp in. Neither is
    /// the prompt's value, so neither takes its completions or feeds its callback.
    fn in_nested_read(&self) -> bool {
        self.cmdline_eval.is_some() || self.history_search_read.is_some()
    }

    /// vim `c_CTRL-D`: the candidates for "the pattern in front of the cursor"
    /// (cmdline.txt), which is the line up to the cursor and not the whole of
    /// it — `:setx` with the cursor on the `x` lists the `set*` names, where the
    /// whole line matches nothing. Nothing is selected and the line is left
    /// alone, which is `wildmode`'s `list` action.
    fn list_completion_before_cursor(&mut self, editor: &Editor) {
        self.exit_selection();
        self.wild_press = 0;
        self.completion = if self.in_nested_read() {
            Vec::new()
        } else {
            (self.completion_fn)(editor, &self.line[..self.cursor])
        };
    }

    /// Compute the cursor position after applying movement
    /// Taken from: <https://github.com/wez/wezterm/blob/e0b62d07ca9bf8ce69a61e30a3c20e7abc48ce7e/termwiz/src/lineedit/mod.rs#L516-L611>
    fn eval_movement(&self, movement: Movement) -> usize {
        match movement {
            Movement::BackwardChar(rep) => {
                let mut position = self.cursor;
                for _ in 0..rep {
                    let mut cursor = GraphemeCursor::new(position, self.line.len(), false);
                    if let Ok(Some(pos)) = cursor.prev_boundary(&self.line, 0) {
                        position = pos;
                    } else {
                        break;
                    }
                }
                position
            }
            Movement::BackwardWord(rep) => {
                let char_indices: Vec<(usize, char)> = self.line.char_indices().collect();
                if char_indices.is_empty() {
                    return self.cursor;
                }
                let mut char_position = char_indices
                    .iter()
                    .position(|(idx, _)| *idx == self.cursor)
                    .unwrap_or(char_indices.len() - 1);

                for _ in 0..rep {
                    if char_position == 0 {
                        break;
                    }

                    let mut found = None;
                    for prev in (0..char_position - 1).rev() {
                        if is_word_sep(char_indices[prev].1) {
                            found = Some(prev + 1);
                            break;
                        }
                    }

                    char_position = found.unwrap_or(0);
                }
                char_indices[char_position].0
            }
            Movement::ForwardWord(rep) => {
                let char_indices: Vec<(usize, char)> = self.line.char_indices().collect();
                if char_indices.is_empty() {
                    return self.cursor;
                }
                let mut char_position = char_indices
                    .iter()
                    .position(|(idx, _)| *idx == self.cursor)
                    .unwrap_or(char_indices.len());

                for _ in 0..rep {
                    // Skip any non-whitespace characters
                    while char_position < char_indices.len()
                        && !is_word_sep(char_indices[char_position].1)
                    {
                        char_position += 1;
                    }

                    // Skip any whitespace characters
                    while char_position < char_indices.len()
                        && is_word_sep(char_indices[char_position].1)
                    {
                        char_position += 1;
                    }

                    // We are now on the start of the next word
                }
                char_indices
                    .get(char_position)
                    .map(|(i, _)| *i)
                    .unwrap_or_else(|| self.line.len())
            }
            Movement::ForwardChar(rep) => {
                let mut position = self.cursor;
                for _ in 0..rep {
                    let mut cursor = GraphemeCursor::new(position, self.line.len(), false);
                    if let Ok(Some(pos)) = cursor.next_boundary(&self.line, 0) {
                        position = pos;
                    } else {
                        break;
                    }
                }
                position
            }
            Movement::StartOfLine => 0,
            Movement::EndOfLine => self.line.len(),
            Movement::None => self.cursor,
        }
    }

    /// vim `'digraph'`, the `<BS>` half: remember the character before the cursor
    /// as char1 of a `{char1}<BS>{char2}` digraph and swallow the `<BS>`.
    /// `true` when that happened, so the caller must not also delete.
    ///
    /// A `<BS>` that arrives with a digraph already armed cancels it and deletes
    /// normally — `take()` clears the arming and the `is_none()` fails — which is
    /// how `:h digraphs-use` says to recover from an unwanted one ("you will have
    /// to type `<BS>` e again").
    fn arm_digraph(&mut self) -> bool {
        if self.digraph_pending.take().is_some()
            || !crate::commands::typed::vim_opt_bool("digraph")
            || self.cursor == 0
        {
            return false;
        }
        match self.line[..self.cursor].chars().next_back() {
            Some(char1) => {
                self.digraph_pending = Some(char1);
                true
            }
            None => false,
        }
    }

    /// vim `'digraph'`, the char2 half: the character `c` combines into, when a
    /// `<BS>` armed char1 and the pair is a digraph. The arming is consumed
    /// either way — a pair that is not a digraph just inserts `c` after char1,
    /// which is what keeps a stray `<BS>` from swallowing the next keystroke.
    fn take_digraph(&mut self, c: char) -> Option<char> {
        let char1 = self.digraph_pending.take()?;
        crate::commands::digraph_lookup(char1, c)
    }

    pub fn insert_char(&mut self, c: char, cx: &Context) {
        self.insert_typed_char(c, cx, true)
    }

    /// The body of [`Prompt::insert_char`]. `abbrev` is false for a character
    /// `CTRL-V` made literal: vim's way of *avoiding* an abbreviation is to type
    /// CTRL-V before the character that would trigger it (map.txt), so a literal
    /// character must not expand one.
    fn insert_typed_char(&mut self, c: char, cx: &Context, abbrev: bool) {
        if let Some(handler) = &self.next_char_handler.take() {
            self.pending_register = false;
            handler(self, c, cx);

            self.next_char_handler = None;
            return;
        }

        // vim `:cmap` — a Command-line-mode mapping replaces the typed character
        // with its rhs. vim `:lmap` + 'imsearch' — a language keymap translates it.
        let mapped = crate::commands::typed::cmdline_map_lookup(&c.to_string())
            .or_else(|| crate::commands::typed::lang_map_lookup(c, false));
        if let Some(rhs) = mapped {
            for ch in rhs.chars() {
                self.line.insert(self.cursor, ch);
                self.cursor += ch.len_utf8();
            }
            self.recalculate_completion(cx.editor);
            return;
        }

        // vim abbreviations: "An abbreviation is only recognized when you type a
        // non-keyword character … The non-keyword character which ends the
        // abbreviation is inserted after the expanded abbreviation" (map.txt), so
        // the expansion happens first and `c` then goes in behind it. A `:cmap` /
        // `:lmap` rhs is already-mapped text rather than a typed character, which
        // is why the mapping arm above returns before reaching this.
        if abbrev && !zmax_core::abbrev::is_keyword_char(c) {
            self.expand_cmdline_abbrev(cx.editor);
        }

        // vim `c_<Insert>`: in overstrike mode a typed character replaces the one
        // under the cursor instead of pushing it right (except at end of line).
        if self.overstrike {
            let mut cursor = GraphemeCursor::new(self.cursor, self.line.len(), false);
            if let Ok(Some(end)) = cursor.next_boundary(&self.line, 0) {
                self.line.replace_range(self.cursor..end, "");
            }
        }
        self.line.insert(self.cursor, c);
        // vim `revins` (reverse insert), armed here by `c_CTRL-_`: leave the cursor
        // before the character just typed, so the next one is inserted ahead of it
        // and typing appears reversed (`abc` -> `cba`). Same rule the buffer's
        // insert follows.
        if !crate::commands::typed::vim_opt_bool("revins") {
            let mut cursor = GraphemeCursor::new(self.cursor, self.line.len(), false);
            if let Ok(Some(pos)) = cursor.next_boundary(&self.line, 0) {
                self.cursor = pos;
            }
        }
        self.recalculate_completion(cx.editor);
    }

    /// The command-line window vim's 'cedit' opens from *this* prompt: `q:` over
    /// the Ex history for the `:` line, `q/` / `q?` over the search history for a
    /// search (whichever way it was started), and `None` for a prompt vim has no
    /// window for — an `input()`-style read or the nested `=` expression line.
    fn cmdline_window_command(&self) -> Option<&'static str> {
        match self.cmdline_type() {
            ':' => Some("cmdline_window"),
            '/' | '?' if self.isearch_forward => Some("search_cmdline_window"),
            '/' | '?' => Some("rsearch_cmdline_window"),
            _ => None,
        }
    }

    /// vim `:cabbrev` / `:cnoreabbrev`: expand the Command-line-mode abbreviation
    /// in front of the cursor. Returns whether the line changed.
    ///
    /// Called from three places, which are vim's three triggers: a typed
    /// non-keyword character ([`Prompt::insert_typed_char`]), the `<CR>` that ends
    /// the command ([`Prompt::submit`]) and `c_CTRL-]`, which expands without
    /// inserting anything extra (map.txt).
    ///
    /// Which abbreviations may fire, and what has to stand in front of the match
    /// for one to, is `zmax_core::abbrev`'s job — this only decides *where*
    /// abbreviations are live: vim's own command lines (`:`, `/`, `?`, `=`), not
    /// an Emacs-style read (`@`), whose answer is a value rather than a command.
    /// "Abbreviations are disabled if the 'paste' option is on" (map.txt).
    fn expand_cmdline_abbrev(&mut self, editor: &Editor) -> bool {
        if self.cmdline_type() == '@' || crate::commands::typed::vim_opt_bool("paste") {
            return false;
        }
        let before = self.line[..self.cursor].to_string();
        let Some((lhs, rhs)) = crate::commands::typed::cmdline_abbrev_expand(&before) else {
            return false;
        };
        self.replace_before_cursor(lhs.len(), &rhs);
        self.recalculate_completion(editor);
        true
    }

    /// The text kakoune's prompt `<c-r>` inserts for register `name`: "insert the
    /// content of the register given by next key, if next key has the Alt
    /// modifier, it will insert all values in the register joined with spaces,
    /// else it will insert the main one. if it has the Control modifier, it will
    /// quote the inserted value(s)" (keys.asciidoc).
    ///
    /// A faithful read of input_handler.cc:762-773: the two modifiers are read off
    /// the register key, `joined` picks every value over the main one, and the
    /// quoter is applied to *each* value before they are joined — so a joined
    /// insert of a quoted register is a list of quoted arguments, not one quoted
    /// string.
    fn register_text(editor: &Editor, name: char, joined: bool, quoted: bool) -> String {
        let quote = |value: Cow<str>| {
            if quoted {
                kak_quote(&value)
            } else {
                value.into_owned()
            }
        };
        if joined {
            editor
                .registers
                .read(name, editor)
                .map(|values| values.map(quote).collect::<Vec<String>>().join(" "))
                .unwrap_or_default()
        } else {
            editor
                .registers
                .first(name, editor)
                .map(quote)
                .unwrap_or_default()
        }
    }

    /// Swap the `len` bytes in front of the cursor for `text`, leaving the cursor
    /// after what went in. What follows the cursor is untouched, so this works
    /// mid-line and not only at the end of it.
    fn replace_before_cursor(&mut self, len: usize, text: &str) {
        let start = self.cursor - len;
        self.line.replace_range(start..self.cursor, text);
        self.cursor = start + text.len();
    }

    /// vim `c_CTRL-L`: complete the command line by the longest prefix every
    /// candidate shares, and stop there — no candidate is selected, so typing goes
    /// on from the part that is certain. Returns whether the line grew.
    fn complete_longest_common(&mut self, editor: &Editor) -> bool {
        let Some((range, _)) = self.completion.first() else {
            return false;
        };
        let range = range.clone();
        let candidates = self
            .completion
            .iter()
            .map(|(_, item)| item.content.as_ref());
        let common = zmax_core::command_line::longest_common_prefix(candidates);
        if common.is_empty() || self.line[range.clone()] == common {
            return false;
        }
        self.line.replace_range(range, &common);
        self.move_end();
        // Recompute against the grown line, but keep the (now longer) candidate
        // list visible rather than selecting one of them.
        self.completion = (self.completion_fn)(editor, &self.line);
        self.exit_selection();
        true
    }

    /// vim `c_CTRL-A`: "All names that match the pattern in front of the cursor
    /// are inserted" (cmdline.txt) — every candidate at once, space separated, in
    /// place of the pattern, with no candidate selected afterwards. Returns
    /// whether anything was inserted.
    ///
    /// Like `c_CTRL-D`, the pattern is the line *up to the cursor* and not the
    /// whole line, so the candidates come from that prefix; what follows the
    /// cursor is left alone, which is why this replaces an explicit
    /// `start..cursor` range rather than the candidate's open-ended one.
    fn complete_insert_all_matches(&mut self, editor: &Editor) -> bool {
        // A nested read is not the command line, so it has no names to insert.
        if self.in_nested_read() {
            return false;
        }
        let candidates = (self.completion_fn)(editor, &self.line[..self.cursor]);
        let Some((range, _)) = candidates.first() else {
            return false;
        };
        let start = range.start;
        let joined = candidates
            .iter()
            .map(|(_, item)| item.content.as_ref())
            .collect::<Vec<&str>>()
            .join(" ");
        if self.line[start..self.cursor] == joined {
            return false;
        }
        self.line.replace_range(start..self.cursor, &joined);
        self.cursor = start + joined.len();
        // The line changed, so the candidate list is stale: recompute it against
        // what is now there (which normally leaves it empty — the inserted list is
        // not itself a pattern) and select nothing, as vim does after `CTRL-A`.
        self.recalculate_completion(editor);
        true
    }

    pub fn insert_str(&mut self, s: &str, editor: &Editor) {
        self.line.insert_str(self.cursor, s);
        self.cursor += s.len();
        self.recalculate_completion(editor);
    }

    pub fn move_cursor(&mut self, movement: Movement) {
        let pos = self.eval_movement(movement);
        self.cursor = pos
    }

    pub fn move_start(&mut self) {
        self.cursor = 0;
    }

    pub fn move_end(&mut self) {
        self.cursor = self.line.len();
    }

    /// vim `c_<LeftMouse>`: put the command-line cursor at the click. The inverse
    /// of [`Prompt::cursor`]'s index→column mapping — walk graphemes from the
    /// render anchor until their accumulated width reaches the clicked column, so
    /// wide characters and a horizontally scrolled line both land right.
    pub fn move_to_column(&mut self, column: u16) {
        let target = column.saturating_sub(self.line_area.x) as usize;
        let mut width = 0;
        let mut idx = self.anchor;
        for grapheme in self.line[self.anchor..].graphemes(true) {
            if width >= target {
                break;
            }
            width += grapheme.width();
            idx += grapheme.len();
        }
        self.cursor = idx;
    }

    pub fn delete_char_backwards(&mut self, editor: &Editor) {
        let pos = self.eval_movement(Movement::BackwardChar(1));
        self.line.replace_range(pos..self.cursor, "");
        self.cursor = pos;

        self.recalculate_completion(editor);
    }

    pub fn delete_char_forwards(&mut self, editor: &Editor) {
        let pos = self.eval_movement(Movement::ForwardChar(1));
        self.line.replace_range(self.cursor..pos, "");

        self.recalculate_completion(editor);
    }

    pub fn delete_word_backwards(&mut self, editor: &Editor) {
        let pos = self.eval_movement(Movement::BackwardWord(1));
        self.kill = self.line[pos..self.cursor].to_string();
        self.line.replace_range(pos..self.cursor, "");
        self.cursor = pos;

        self.recalculate_completion(editor);
    }

    pub fn delete_word_forwards(&mut self, editor: &Editor) {
        let pos = self.eval_movement(Movement::ForwardWord(1));
        self.kill = self.line[self.cursor..pos].to_string();
        self.line.replace_range(self.cursor..pos, "");

        self.recalculate_completion(editor);
    }

    pub fn kill_to_start_of_line(&mut self, editor: &Editor) {
        let pos = self.eval_movement(Movement::StartOfLine);
        self.kill = self.line[pos..self.cursor].to_string();
        self.line.replace_range(pos..self.cursor, "");
        self.cursor = pos;

        self.recalculate_completion(editor);
    }

    pub fn kill_to_end_of_line(&mut self, editor: &Editor) {
        let pos = self.eval_movement(Movement::EndOfLine);
        self.kill = self.line[self.cursor..pos].to_string();
        self.line.replace_range(self.cursor..pos, "");

        self.recalculate_completion(editor);
    }

    /// readline `C-y`: re-insert the most recently killed text at the cursor.
    pub fn yank(&mut self, editor: &Editor) {
        if self.kill.is_empty() {
            return;
        }
        let text = self.kill.clone();
        self.line.insert_str(self.cursor, &text);
        self.cursor += text.len();
        self.recalculate_completion(editor);
    }

    pub fn clear(&mut self, editor: &Editor) {
        self.line.clear();
        self.cursor = 0;
        self.recalculate_completion(editor);
    }

    pub fn change_history(
        &mut self,
        cx: &mut Context,
        register: char,
        direction: CompletionDirection,
    ) {
        (self.callback_fn)(cx, &self.line, PromptEvent::Abort);
        let mut values = match cx.editor.registers.read(register, cx.editor) {
            Some(values) if values.len() > 0 => values.rev(),
            _ => return,
        };

        let end = values.len().saturating_sub(1);

        let index = match direction {
            CompletionDirection::Forward => self.history_pos.map_or(0, |i| i + 1),
            CompletionDirection::Backward => self
                .history_pos
                .unwrap_or_else(|| values.len())
                .saturating_sub(1),
        }
        .min(end);

        self.line = values.nth(index).unwrap().to_string();
        // Appease the borrow checker.
        drop(values);

        self.history_pos = Some(index);

        self.move_end();
        self.fire_update(cx);
        self.recalculate_completion(cx.editor);
    }

    pub fn change_completion_selection(&mut self, direction: CompletionDirection) {
        if self.completion.is_empty() {
            return;
        }

        let index = match direction {
            CompletionDirection::Forward => self.selection.map_or(0, |i| i + 1),
            CompletionDirection::Backward => {
                self.selection.unwrap_or(0) + self.completion.len() - 1
            }
        } % self.completion.len();

        self.selection = Some(index);

        let (range, item) = &self.completion[index];

        self.line.replace_range(range.clone(), &item.content);

        self.move_end();
    }

    pub fn exit_selection(&mut self) {
        self.selection = None;
    }

    // ── Emacs minibuffer completion commands ────────────────────────────────
    // These are the pieces `minibuffer-complete-word` / `-complete-and-exit` /
    // `-choose-completion` / `-complete-history` need; the commands themselves
    // live in `commands.rs` and reach the live prompt through the compositor.

    /// How many completion candidates the prompt is currently offering.
    pub fn completion_count(&self) -> usize {
        self.completion.len()
    }

    /// The index of the selected candidate, if one is selected.
    pub fn selected_completion(&self) -> Option<usize> {
        self.selection
    }

    /// Whether `line` is already exactly one of the candidates.
    pub fn line_is_candidate(&self) -> bool {
        self.completion
            .iter()
            .any(|(range, item)| self.line[range.clone()] == *item.content)
    }

    /// Splice candidate `index` into the line (as selecting it does), leaving no
    /// selection behind — the completion is now just text the user typed.
    pub fn apply_completion(&mut self, index: usize) -> bool {
        let Some((range, item)) = self.completion.get(index) else {
            return false;
        };
        let (range, content) = (range.clone(), item.content.to_string());
        self.line.replace_range(range, &content);
        self.move_end();
        self.selection = None;
        true
    }

    /// Emacs `minibuffer-complete-word` (`SPC` in a completing read): complete
    /// the input only as far as the next word boundary of the common completion,
    /// instead of all the way. Returns whether the line grew.
    pub fn complete_word(&mut self, editor: &Editor) -> bool {
        let Some((range, _)) = self.completion.first() else {
            return false;
        };
        let range = range.clone();
        let candidates = self
            .completion
            .iter()
            .map(|(_, item)| item.content.as_ref());
        let common = zmax_core::command_line::longest_common_prefix(candidates);
        let current = &self.line[range.clone()];
        if common.is_empty() || common.len() <= current.len() {
            return false;
        }
        // Stop at the first word separator strictly after what is already typed —
        // Emacs's "one word at a time" completion.
        let grown = &common[current.len()..];
        let stop = grown
            .char_indices()
            .find(|(i, c)| is_word_sep(*c) && *i > 0)
            .map(|(i, c)| current.len() + i + c.len_utf8())
            .unwrap_or(common.len());
        let partial = common[..stop].to_string();
        if partial == current {
            return false;
        }
        self.line.replace_range(range, &partial);
        self.move_end();
        self.completion = (self.completion_fn)(editor, &self.line);
        self.exit_selection();
        true
    }

    /// Emacs `minibuffer-complete-history`: complete the input against the
    /// prompt's history instead of its completion table — the candidate list
    /// becomes the history entries containing what is typed. Returns how many.
    pub fn complete_from_history(&mut self, editor: &Editor) -> usize {
        let Some(register) = self.history_register else {
            return 0;
        };
        let needle = self.line.clone();
        let entries: Vec<String> = match editor.registers.read(register, editor) {
            Some(values) => values
                .map(|v| v.to_string())
                .filter(|v| v.contains(&needle))
                .collect(),
            None => Vec::new(),
        };
        self.completion = entries
            .into_iter()
            .map(|e| ((0..), Span::raw(e)))
            .collect::<Vec<_>>();
        self.exit_selection();
        self.completion.len()
    }

    /// Move the completion selection without splicing anything the caller did not
    /// ask for: select candidate `index` (clamped) and put it on the line, as
    /// moving point in Emacs's `*Completions*` buffer does. `false` when there is
    /// nothing to select.
    pub fn select_completion(&mut self, index: usize) -> bool {
        if self.completion.is_empty() {
            return false;
        }
        let index = index.min(self.completion.len() - 1);
        let (range, item) = &self.completion[index];
        let (range, content) = (range.clone(), item.content.to_string());
        self.line.replace_range(range, &content);
        self.selection = Some(index);
        self.move_end();
        true
    }
    /// Accept the line — store it in the history register and fire the
    /// `Validate` callback. This is exactly what `Enter` does; `false` means the
    /// prompt must stay open (a directory completion was selected, and the
    /// candidate list was refreshed for the next component instead).
    pub fn submit(&mut self, cx: &mut Context) -> bool {
        // vim: the non-keyword character that expands an abbreviation "can also be
        // … the <CR> that ends a command" (map.txt) — so `:W<CR>` runs what
        // `:cabbrev W write` stands for, which is the whole point of a
        // command-line abbreviation.
        self.expand_cmdline_abbrev(cx.editor);
        if self.selection.is_some() && self.line.ends_with(std::path::MAIN_SEPARATOR) {
            self.recalculate_completion(cx.editor);
            return false;
        }
        let last_item = self
            .first_history_completion(cx.editor)
            .map(|entry| entry.to_string())
            .unwrap_or_default();
        // An empty line runs the most recent history entry, as Enter does. What is
        // stored and run is `pattern()`: for a search whose isearch toggles have
        // been used, the pattern is what the toggles made of the line, so a later
        // repeat of the search from the history repeats the same search.
        let input = if self.line.is_empty() {
            last_item
        } else {
            let pattern = self.pattern();
            if last_item != pattern {
                if let Some(register) = self.history_register {
                    if let Err(err) = cx.editor.registers.push(register, pattern.clone()) {
                        cx.editor.set_error(err.to_string());
                    }
                }
            }
            pattern
        };
        let folds = self.isearch_fold_snapshot(cx.editor);
        (self.callback_fn)(cx, &input, PromptEvent::Validate);
        // `isearch-invisible` nil: the committed match has to be a visible one
        // too. `Validate` searches afresh from where the prompt opened, so it can
        // land back on a match the incremental search had already stepped over.
        self.isearch_skip_invisible(cx, folds);
        true
    }

    /// Emacs `icomplete-fido-ret` (`RET` under `fido-mode`): `RET` runs the top
    /// candidate rather than what is literally typed. Selecting the head is
    /// `icomplete-force-complete-and-exit`'s "use the first of the matches if
    /// there are any displayed, and the default otherwise" — with none displayed
    /// [`Prompt::submit`] already runs the default. A candidate that is a
    /// directory is stepped into instead of run (`icomplete-force-complete`),
    /// which `submit` does for a selected candidate ending in a separator.
    fn fido_ret(&mut self, cx: &mut Context) -> bool {
        if self.selection.is_none() && !self.completion.is_empty() {
            self.change_completion_selection(CompletionDirection::Forward);
        }
        self.submit(cx)
    }

    /// vim `c_CTRL-\_e {expr}`: open the nested `=` prompt. The command line
    /// being typed is set aside — it is not the expression, it is what the
    /// expression gets to read (`getcmdline()`) and what it replaces.
    fn begin_cmdline_eval(&mut self, editor: &Editor) {
        self.cmdline_eval = Some(CmdlineEval {
            line: std::mem::take(&mut self.line),
            cursor: std::mem::replace(&mut self.cursor, 0),
            prompt: std::mem::replace(&mut self.prompt, Cow::Borrowed("=")),
        });
        self.recalculate_completion(editor);
    }

    /// Whether this prompt is a *completing read* — Emacs's
    /// `minibuffer-local-completion-map`, where `SPC` completes a word and `?`
    /// lists the candidates instead of both being characters of the answer.
    ///
    /// vim's command line (`:`), its searches (`/`, `?`) and the expression line
    /// (`=`) are not: an argument typed there may contain a space or a question
    /// mark. Every other prompt reads one value with completion, which is what
    /// `getcmdtype()` calls `@` — vim's `input()` line.
    fn is_completing_read(&self) -> bool {
        self.isearch.is_none() && self.cmdline_eval.is_none() && self.cmdline_type() == '@'
    }

    /// vim's `getcmdtype()` character for this prompt: the command line's own
    /// first character when it is one vim names (`:` ex, `/` and `?` search, `=`
    /// expression, `-` `:insert`), else the history it shares with one of those
    /// lines — zmax words the search prompt "search:", but it is vim's `/`. Any
    /// other prompt is `@`, vim's type for an `input()` line.
    fn cmdline_type(&self) -> char {
        const VIM_TYPES: [char; 5] = [':', '/', '?', '=', '-'];
        let first = self.prompt.chars().next();
        first
            .filter(|c| VIM_TYPES.contains(c))
            .or_else(|| self.history_register.filter(|c| VIM_TYPES.contains(c)))
            .unwrap_or('@')
    }

    /// vim `c_CTRL-\_e {expr}`: `<Enter>` finishes the expression. It is
    /// evaluated with the set-aside command line published to vimlrs first, so
    /// the documented `getcmdline() .. " Some()"` idiom reads the text
    /// `CTRL-\ e` interrupted, and its result becomes the whole command line.
    /// An expression that errors leaves the command line as it was.
    fn finish_cmdline_eval(&mut self, cx: &mut Context) {
        let Some(saved) = self.cmdline_eval.take() else {
            return;
        };
        let expr = std::mem::take(&mut self.line);
        self.prompt = saved.prompt;
        // The expression runs against the interrupted command line, not against
        // itself: `getcmdline()`, `getcmdpos()` and `getcmdtype()` all answer
        // for the line `CTRL-\ e` set aside.
        let cmdtype = self.cmdline_type();
        crate::commands::typed::cmdline_publish_state(&saved.line, saved.cursor, cmdtype);
        let evaluated = crate::commands::typed::cmdline_eval_expr(cx, &expr);
        // `setcmdpos()` is how the expression says where the cursor goes; read it
        // back before the command line stops being published.
        let repositioned =
            crate::commands::typed::cmdline_published_cursor().filter(|pos| *pos != saved.cursor);
        crate::commands::typed::cmdline_clear_state();
        match evaluated {
            Ok(result) => {
                // c: "The cursor position is unchanged, except when the cursor
                // was at the end of the line, then it stays at the end"
                // (cmdline.txt) — and `setcmdpos()` overrides both.
                let cursor = match repositioned {
                    Some(pos) => pos,
                    None if saved.cursor == saved.line.len() => result.len(),
                    None => saved.cursor,
                };
                self.cursor = clamp_to_boundary(&result, cursor);
                self.line = result;
                self.recalculate_completion(cx.editor);
            }
            Err(e) => {
                self.line = saved.line;
                self.cursor = saved.cursor;
                self.recalculate_completion(cx.editor);
                cx.editor.set_error(format!("CTRL-\\ e: {e}"));
            }
        }
    }

    /// vim `c_CTRL-\_e {expr}`: abandon the expression — the command line comes
    /// back exactly as it was, cursor included.
    fn cancel_cmdline_eval(&mut self, editor: &Editor) {
        let Some(saved) = self.cmdline_eval.take() else {
            return;
        };
        self.prompt = saved.prompt;
        self.line = saved.line;
        self.cursor = saved.cursor;
        self.recalculate_completion(editor);
    }

    /// The character a key stands for when `CTRL-V` made it literal: a control
    /// chord is the control character itself (`CTRL-V CTRL-R` puts `0x12` on the
    /// line, as in vim), anything else is its plain character.
    fn literal_char(event: KeyEvent) -> Option<char> {
        let c = event.char()?;
        if event
            .modifiers
            .contains(zmax_view::keyboard::KeyModifiers::CONTROL)
            && c.is_ascii_alphabetic()
        {
            return Some(char::from(c.to_ascii_uppercase() as u8 - 0x40));
        }
        Some(c)
    }

    /// vim `c_CTRL-V`: consume a key that `CTRL-V` made literal, or a digit of the
    /// character code it opened. Returns whether the key belonged to one of those.
    fn handle_literal(&mut self, event: KeyEvent, cx: &Context) -> bool {
        // A code in progress: collect digits until the form is full, and let any
        // other key end it — the character is inserted, then that key normally.
        if let Some((radix, mut digits)) = self.literal_code.take() {
            match event.char() {
                Some(c) if radix.is_digit(c) => {
                    digits.push(c);
                    if digits.len() >= radix.max_digits() {
                        self.insert_literal_code(radix, &digits, cx);
                    } else {
                        self.literal_code = Some((radix, digits));
                    }
                }
                terminator => {
                    self.insert_literal_code(radix, &digits, cx);
                    if let Some(c) = terminator {
                        self.insert_char(c, cx);
                    }
                }
            }
            return true;
        }

        if !self.pending_literal {
            return false;
        }
        self.pending_literal = false;
        match event.char() {
            // A digit opens a decimal code, `o`/`x`/`u`/`U`/`b` the other forms.
            Some(c) if c.is_ascii_digit() => {
                self.literal_code = Some((LiteralRadix::Decimal, c.to_string()));
            }
            Some(c) => match LiteralRadix::from_introducer(c) {
                Some(radix) => self.literal_code = Some((radix, String::new())),
                None => {
                    if let Some(c) = Self::literal_char(event) {
                        self.insert_typed_char(c, cx, false);
                    }
                }
            },
            // A key with no character of its own (an arrow, a function key) has
            // nothing literal to insert.
            None => {}
        }
        true
    }

    /// Insert the character the digits of a `CTRL-V` code name.
    fn insert_literal_code(&mut self, radix: LiteralRadix, digits: &str, cx: &Context) {
        if let Some(c) = literal_code_char(radix, digits) {
            self.insert_typed_char(c, cx, false);
        }
    }

    // ── Emacs incremental search (isearch) ──────────────────────────────────
    // Emacs's isearch keys live *inside* the search: they edit the string being
    // typed and re-run the search from it. In zmax that string is the search
    // prompt's line, so the keys live here. The pattern the search actually runs
    // is `pattern()` — what the isearch toggles make of the typed line, built by
    // the same `zmax_core::search::IsearchFlags` the `isearch-*` commands use.

    /// The pattern the callback must search for. For every prompt that is not an
    /// incremental search this is just the line; inside one it is what the isearch
    /// toggles (`M-r`, `M-c`, `M-s SPC`, …) make of it. With the toggles untouched
    /// the two are the same string, so an ordinary `/` search is unchanged.
    fn pattern(&self) -> String {
        let Some(flags) = self.isearch else {
            return self.line.clone();
        };
        let pattern = flags.build_regex(&self.line);
        match self.isearch_case {
            // `M-c` overrides the smart-case default the search prompt computes:
            // an inline flag in the pattern beats the compiler's setting.
            Some(fold) if !pattern.is_empty() => {
                let flag = if fold { "(?i)" } else { "(?-i)" };
                format!("{flag}{pattern}")
            }
            _ => pattern,
        }
    }

    /// Re-run the search / re-notify the caller for what is now typed.
    fn fire_update(&mut self, cx: &mut Context) {
        // What is typed into the `=` prompt of `c_CTRL-\_e` is an expression:
        // the command line's own callback (an incremental search, say) must not
        // see it — it only ever sees the result the expression produces.
        // `isearch-edit-string` is the same story: while the search string is
        // being edited the search does not run, so the buffer stays where the
        // last search left it until `RET` resumes. So is the regexp typed into the
        // recursive minibuffer `M-r` / `M-s` opens.
        if self.in_nested_read() || self.isearch_edit {
            return;
        }
        // `isearch-invisible` nil: what the folds looked like before the search
        // moved, which is what decides whether the match it finds is one Emacs
        // would have found at all.
        let folds = self.isearch_fold_snapshot(cx.editor);
        let pattern = self.pattern();
        (self.callback_fn)(cx, &pattern, PromptEvent::Update);
        // The update searched the way the prompt was opened; with the direction
        // toggle on (dte `M-r`), turn it back round before anything reads where
        // the search landed.
        self.isearch_step_reversed(cx);
        // Step off a match a closed fold hid before anything else reads where the
        // search landed.
        self.isearch_skip_invisible(cx, folds);
        if self.isearch.is_some() {
            self.isearch_note_result(cx.editor, &pattern);
        }
        self.isearch_reveal(cx);
    }

    /// The cursor of the match the search is sitting on when a closed fold hides
    /// it, `None` when it is visible. zmax's invisible text is folded text, so
    /// this is Emacs's `isearch-range-invisible` test.
    fn hidden_match_cursor(editor: &Editor) -> Option<usize> {
        let (view, doc) = current_ref!(editor);
        let text = doc.text();
        let cursor = doc.selection(view.id).primary().cursor(text.slice(..));
        doc.folds()
            .is_line_hidden(text.char_to_line(cursor))
            .then_some(cursor)
    }

    /// The fold state to judge the next match against, taken *before* the search
    /// runs. `None` when there is nothing to judge: not a search, the `M-s i`
    /// toggle is on (the match is revealed instead), or the prompt has no
    /// next-match cycle to step with — it is not one of the incremental searches
    /// (`/`, `?`).
    ///
    /// The snapshot is needed because the search opens what it lands in on its own
    /// (vim `'foldopen'` contains `search`), so by the time the match can be looked
    /// at, the fold that hid it is already open.
    fn isearch_fold_snapshot(&self, editor: &Editor) -> Option<zmax_core::fold::Folds> {
        match self.isearch {
            None
            | Some(IsearchFlags {
                invisible: true, ..
            }) => return None,
            Some(_) => {}
        }
        self.incsearch_cycle.as_ref()?;
        let (_view, doc) = current_ref!(editor);
        Some(doc.folds().clone())
    }

    /// Put the fold state back as `snapshot` had it, undoing what the search's own
    /// `'foldopen'` reveal did.
    fn restore_folds(editor: &mut Editor, snapshot: &zmax_core::fold::Folds) {
        let (_view, doc) = current!(editor);
        if doc.folds() != snapshot {
            *doc.folds_mut() = snapshot.clone();
        }
    }

    /// Emacs `isearch-invisible` nil (`M-s i`, off): a match hidden in invisible
    /// text "is not found at all" — `isearch-range-invisible` rejects it and the
    /// search goes on to the next one (isearch.el). With the toggle on (Emacs's
    /// default `search-invisible` = `open`) the match is kept and what hides it
    /// opened instead, which is [`Prompt::isearch_reveal`].
    ///
    /// So each candidate is judged against the fold state as it was *before* the
    /// search moved, and that state is put back first — a match Emacs would not
    /// have found must not be left revealed by the search's own `'foldopen'`. The
    /// step is the incremental search's own next-match cycle, so skipping obeys
    /// `search.wrap-around` exactly as `C-s` does and goes on in the direction the
    /// search is running. It stops at the first visible match; when every match has
    /// been stepped over — the cycle comes back to one already rejected, or stops
    /// moving with wrapping off — the search has failed, so the buffer goes back to
    /// where it was, as a failing Emacs search leaves point where the last
    /// successful one put it.
    fn isearch_skip_invisible(
        &mut self,
        cx: &mut Context,
        snapshot: Option<zmax_core::fold::Folds>,
    ) {
        let Some(snapshot) = snapshot else {
            return;
        };
        let line = self.line.clone();
        let mut rejected = std::collections::HashSet::new();
        let mut failed = false;
        loop {
            Self::restore_folds(cx.editor, &snapshot);
            let Some(cursor) = Self::hidden_match_cursor(cx.editor) else {
                break;
            };
            if !rejected.insert(cursor) {
                failed = true;
                break;
            }
            let Some(cycle) = &mut self.incsearch_cycle else {
                break;
            };
            cycle(cx, &line, true);
        }
        if failed {
            Self::restore_folds(cx.editor, &snapshot);
            (self.callback_fn)(cx, &line, PromptEvent::Abort);
        }
    }

    /// Emacs `isearch-invisible` (`M-s i`, on): the match the search just landed
    /// on must be visible, so the closed folds hiding it are opened — zmax's
    /// invisible text is a closed fold. With the toggle off nothing is opened.
    fn isearch_reveal(&mut self, cx: &mut Context) {
        if !self.isearch.is_some_and(|flags| flags.invisible) {
            return;
        }
        let scrolloff = cx.editor.config().scrolloff;
        let (view, doc) = current!(cx.editor);
        let line = {
            let text = doc.text();
            let cursor = doc.selection(view.id).primary().cursor(text.slice(..));
            text.char_to_line(cursor)
        };
        // Nested folds: keep opening the innermost one until the line shows.
        while doc.folds().is_line_hidden(line) && doc.folds_mut().open(line) {}
        view.ensure_cursor_in_view(doc, scrolloff);
    }

    /// Emacs `isearch-repeat-forward` (`C-s`) / `isearch-repeat-backward` (`C-r`):
    /// go to the next match in that direction. With nothing typed yet the previous
    /// search string comes back instead, as it does in Emacs.
    fn isearch_repeat(&mut self, cx: &mut Context, forward: bool) {
        if self.line.is_empty() {
            let previous = self
                .first_history_completion(cx.editor)
                .map(|entry| entry.to_string());
            if let Some(previous) = previous {
                self.set_line(previous, cx.editor);
                self.fire_update(cx);
            }
            return;
        }
        // The cycle searches on from the current match, and its flag means "the
        // way the search was started" — so an absolute forward/backward repeat is
        // that flag compared against the starting direction.
        let pattern = self.pattern();
        let with_start = forward == self.isearch_forward;
        if let Some(cycle) = &mut self.incsearch_cycle {
            cycle(cx, &pattern, with_start);
        }
    }

    /// dte `M-r` — "Reverse search direction" (dte.md, search mode): turn the
    /// search in flight round, so it runs the other way from here on.
    ///
    /// Bound to `A-R` rather than dte's `A-r`, which is already Emacs's
    /// `isearch-toggle-regexp` on this prompt (and is cited as that by the port
    /// mapping) — the shifted sibling is the free key next to it.
    ///
    /// The direction the search prompt's own callback runs in was fixed when the
    /// prompt was opened, so what actually moves the search the other way is the
    /// incsearch cycle: stepping it against the starting direction lands on the
    /// match on the other side, and marks the prompt "cycled", which is what makes
    /// `<Enter>` commit the match it is showing instead of re-searching forward
    /// from where the prompt opened (ui/mod.rs `raw_regex_prompt`). The flag keeps
    /// that up for what is typed after, one step per update.
    fn isearch_reverse_direction(&mut self, cx: &mut Context) {
        if self.incsearch_cycle.is_none() {
            return;
        }
        self.isearch_reversed = !self.isearch_reversed;
        cx.editor
            .set_status(if self.isearch_reversed == self.isearch_forward {
                "search direction: backward"
            } else {
                "search direction: forward"
            });
        self.isearch_step_reversed(cx);
    }

    /// Move the search to the match on the other side of the current one, when
    /// the direction toggle above is on. A no-op otherwise, and with nothing
    /// typed — there is no match to step from yet.
    fn isearch_step_reversed(&mut self, cx: &mut Context) {
        if !self.isearch_reversed || self.line.is_empty() {
            return;
        }
        let pattern = self.pattern();
        if let Some(cycle) = &mut self.incsearch_cycle {
            // `false` is "against the direction the search was started in", which
            // is exactly what the toggle asks for.
            cycle(cx, &pattern, false);
        }
    }

    /// Emacs `isearch-beginning-of-buffer` (`M-s M-<`) / `isearch-end-of-buffer`
    /// (`M-s M->`): restart the search from the far end of the buffer rather than
    /// carrying on from the current match, so it finds the first (or last) match
    /// in the whole buffer. Moving point is what makes the repeat start there —
    /// the cycle always searches on from wherever the cursor is.
    fn isearch_from_edge(&mut self, cx: &mut Context, first: bool) {
        if self.line.is_empty() {
            return;
        }
        {
            let (view, doc) = current!(cx.editor);
            let pos = if first { 0 } else { doc.text().len_chars() };
            doc.set_selection(view.id, Selection::point(pos));
        }
        self.isearch_repeat(cx, first);
    }

    /// The text an `isearch-yank-*` key takes: Emacs grabs it from the end of the
    /// current match, so what is yanked is the buffer text the match is about to
    /// grow over.
    fn isearch_grab(&self, editor: &Editor, kind: IsearchYank) -> String {
        let (view, doc) = current_ref!(editor);
        let text = doc.text().slice(..);
        let pos = doc.selection(view.id).primary().to();
        match kind {
            IsearchYank::Char => search::grab_char(text, pos).unwrap_or_default(),
            IsearchYank::WordOrChar => search::grab_word_or_char(text, pos),
            IsearchYank::Line => search::grab_line(text, pos),
        }
    }

    /// Quote text that is going into the search string: a regexp search must take
    /// yanked (or `C-q`-quoted) text literally, so its characters cannot act as
    /// operators. A literal search is quoted by `IsearchFlags::build_regex` itself.
    fn isearch_quote(&self, text: &str) -> String {
        match self.isearch {
            Some(flags) if flags.regexp => regex::escape(text),
            _ => text.to_string(),
        }
    }

    /// Add text to the end of the search string and search again — the match grows
    /// by what was added, which is what every `isearch-yank-*` key does. Returns
    /// how many bytes landed on the line, which is what `M-y` has to take back off
    /// again when it swaps one kill for an older one.
    fn isearch_add(&mut self, cx: &mut Context, text: &str) -> usize {
        // Anything but a kill yank ends the `M-y` cycle, as it ends Emacs's
        // `last-command` check; the two yank keys re-arm it after this returns.
        self.isearch_yank_len = None;
        if text.is_empty() {
            return 0;
        }
        let quoted = self.isearch_quote(text);
        self.move_end();
        self.insert_str(&quoted, cx.editor);
        self.fire_update(cx);
        quoted.len()
    }

    /// Emacs `isearch-yank-kill` (`C-y`): grow the search string by the most
    /// recent kill, and start the kill ring cycling that `M-y` carries on.
    fn isearch_yank_kill(&mut self, cx: &mut Context) {
        let Some(kill) = crate::emacs_kill::top() else {
            cx.editor.set_error("Kill ring is empty");
            return;
        };
        let len = self.isearch_add(cx, &kill);
        crate::emacs_kill::begin_yank(ISEARCH_YANK_SEL.to_vec());
        self.isearch_yank_len = Some(len);
    }

    /// Emacs `isearch-yank-pop-only` (`M-y`, the key isearch-mode-map binds):
    /// replace the kill `C-y` just appended with the next-older one. Called
    /// anywhere else than straight after a kill yank it only pops the last kill —
    /// that is what the `-only` in the name means, as against `isearch-yank-pop`,
    /// which opens a read over the whole ring instead
    /// (`commands::isearch_yank_from_kill_ring`, reachable by name).
    fn isearch_yank_pop_only(&mut self, cx: &mut Context) {
        let previous = self.isearch_yank_len.filter(|len| {
            *len <= self.line.len() && self.line.is_char_boundary(self.line.len() - len)
        });
        let Some(len) = previous else {
            self.isearch_yank_kill(cx);
            return;
        };
        let Some(older) = crate::emacs_kill::next_entry(ISEARCH_YANK_SEL) else {
            // One entry, or the ring moved on: nothing older to swap in.
            return;
        };
        // Take the previous kill back off the end before the older one goes on,
        // so the search grows by the replacement and not by both.
        self.line.truncate(self.line.len() - len);
        self.cursor = self.line.len();
        let len = self.isearch_add(cx, &older);
        crate::emacs_kill::set_yank_sel(ISEARCH_YANK_SEL.to_vec());
        self.isearch_yank_len = Some(len);
    }

    /// `C-w`, `C-M-y`, `M-s C-e`: yank buffer text at the match into the search.
    fn isearch_yank(&mut self, cx: &mut Context, kind: IsearchYank) {
        let text = self.isearch_grab(cx.editor, kind);
        self.isearch_add(cx, &text);
    }

    /// Emacs `isearch-toggle-regexp` (`M-r`, `M-s r`) and the rest of the `M-s`
    /// mode toggles: flip one, report it, and re-run the search under it. The
    /// pattern modes are mutually exclusive, as they are in Emacs.
    fn isearch_toggle(&mut self, cx: &mut Context, toggle: IsearchToggle) {
        let Some(flags) = self.isearch.as_mut() else {
            return;
        };
        let (name, on) = match toggle {
            IsearchToggle::Regexp => {
                flags.regexp = !flags.regexp;
                if flags.regexp {
                    flags.word = false;
                    flags.symbol = false;
                    flags.char_fold = false;
                }
                ("Regexp", flags.regexp)
            }
            IsearchToggle::Word => {
                flags.word = !flags.word;
                if flags.word {
                    flags.regexp = false;
                    flags.symbol = false;
                    flags.char_fold = false;
                }
                ("Word", flags.word)
            }
            IsearchToggle::Symbol => {
                flags.symbol = !flags.symbol;
                if flags.symbol {
                    flags.regexp = false;
                    flags.word = false;
                    flags.char_fold = false;
                }
                ("Symbol", flags.symbol)
            }
            IsearchToggle::CharFold => {
                flags.char_fold = !flags.char_fold;
                if flags.char_fold {
                    // Char folding expands each character into its equivalence
                    // class, which only a literal search is quoted into.
                    flags.regexp = false;
                    flags.word = false;
                    flags.symbol = false;
                }
                ("Char-fold", flags.char_fold)
            }
            IsearchToggle::LaxWhitespace => {
                flags.lax_whitespace = !flags.lax_whitespace;
                ("Lax-whitespace", flags.lax_whitespace)
            }
            IsearchToggle::Invisible => {
                flags.invisible = !flags.invisible;
                ("Invisible-match", flags.invisible)
            }
        };
        cx.editor.set_status(format!(
            "{name} I-search: {}",
            if on { "on" } else { "off" }
        ));
        self.fire_update(cx);
    }

    /// Emacs `isearch-toggle-case-fold` (`M-c`, `M-s c`): flip whether the search
    /// ignores case. The first press flips the state the search is running with —
    /// zmax's smart case, which folds until an upper-case letter is typed — and
    /// from then on the choice is explicit.
    fn isearch_toggle_case(&mut self, cx: &mut Context) {
        if self.isearch.is_none() {
            return;
        }
        let folding = self.isearch_case.unwrap_or_else(|| {
            cx.editor.config().search.smart_case && !self.line.chars().any(char::is_uppercase)
        });
        self.isearch_case = Some(!folding);
        cx.editor.set_status(format!(
            "Case-fold I-search: {}",
            if folding { "off" } else { "on" }
        ));
        self.fire_update(cx);
    }

    /// Emacs `isearch-complete` (`M-TAB`): complete the search string from the
    /// search ring. A single candidate is taken; several are offered.
    fn isearch_complete(&mut self, cx: &mut Context) {
        match self.complete_from_history(cx.editor) {
            0 => cx.editor.set_error("No search string completes that"),
            1 => {
                self.apply_completion(0);
                self.fire_update(cx);
            }
            // More than one: the candidates are on screen to pick from.
            _ => {}
        }
    }

    /// Emacs's `isearch-success` and `isearch-error`: whether the search string
    /// as it now stands is found, and — when it is — the string it was found
    /// with, which is what `isearch-abort` rubs back to.
    fn isearch_note_result(&mut self, editor: &Editor, pattern: &str) {
        let found = pattern.is_empty() || self.isearch_found_in(editor, pattern);
        self.isearch_success = found;
        if found {
            self.isearch_found = self.line.clone();
        }
    }

    /// Whether the pattern matches anywhere in the buffer. A pattern that does
    /// not compile is a failing search too — Emacs's `isearch-error`, which the
    /// half-typed `[` of a regexp search puts it in.
    fn isearch_found_in(&self, editor: &Editor, pattern: &str) -> bool {
        let case_insensitive =
            editor.config().search.smart_case && !self.line.chars().any(char::is_uppercase);
        let Ok(regex) = rope::RegexBuilder::new()
            .syntax(
                rope::Config::new()
                    .case_insensitive(case_insensitive)
                    .multi_line(true),
            )
            .build(pattern)
        else {
            return false;
        };
        let (_view, doc) = current_ref!(editor);
        let text = doc.text().slice(..);
        // `isearch-invisible` nil (`M-s i`, off): a match a closed fold hides is
        // not found at all, so a search with only those left to offer is a failing
        // one — which is what `C-g` rubs back out of.
        if matches!(self.isearch, Some(flags) if !flags.invisible) {
            return regex.find_iter(text.regex_input()).any(|mat| {
                !doc.folds()
                    .is_line_hidden(text.char_to_line(text.byte_to_char(mat.start())))
            });
        }
        regex.is_match(text.regex_input())
    }

    /// Emacs `isearch-abort` (`C-g`): a search that has found what was asked for
    /// goes back to where it started and quits; a failing one only rubs out the
    /// characters that made it fail and stays in the search. So the search that
    /// cannot find `FOOT` takes `C-g C-g` to leave: the first one puts `FOO`
    /// back, the second quits. Returns true when the search is over.
    fn isearch_abort(&mut self, cx: &mut Context) -> bool {
        if self.isearch_success {
            (self.callback_fn)(cx, &self.line, PromptEvent::Abort);
            return true;
        }
        let found = self.isearch_found.clone();
        self.set_line(found, cx.editor);
        self.fire_update(cx);
        false
    }

    /// Emacs `isearch-edit-string` (`M-e`, and `Mouse-1` on the minibuffer while
    /// a search is running): edit the search string instead of searching with
    /// every keystroke. The buffer stays on the match it is on until `RET`
    /// resumes the incremental search with the edited string.
    fn isearch_edit_string(&mut self, cx: &mut Context) {
        if self.isearch.is_none() || self.isearch_edit {
            return;
        }
        self.isearch_edit = true;
        cx.editor
            .set_status("Edit search string: RET to resume searching");
    }

    /// Emacs `isearch-help-for-help` (`C-h C-h`, `C-h ?`, `C-h f1`): the help
    /// options `isearch-help-map` offers.
    fn isearch_help_for_help(&mut self, cx: &mut Context) {
        self.show_isearch_help(cx, Info::new("Isearch help", ISEARCH_HELP_OPTIONS));
    }

    /// Emacs `isearch-describe-bindings` (`C-h b`): every key the search binds.
    fn isearch_describe_bindings(&mut self, cx: &mut Context) {
        self.show_isearch_help(cx, Info::new("Isearch mode bindings", ISEARCH_BINDINGS));
    }

    /// Emacs `isearch-describe-mode` (`C-h m`): what the mode the search is in
    /// does, rather than the keys it binds.
    fn isearch_describe_mode(&mut self, cx: &mut Context) {
        self.show_isearch_help(cx, help_text("Isearch mode", ISEARCH_MODE_DOC));
    }

    /// Emacs `isearch-describe-key` (`C-h k`): the command a key of the search
    /// runs. `M-s` and `C-x` are prefixes, so a sequence that starts with one of
    /// them goes on being read.
    fn isearch_describe_key(&mut self, cx: &mut Context, event: KeyEvent) {
        let prefix = self.describe_key.take().unwrap_or_default();
        let keys = format!("{prefix}{}", emacs_key_name(event));
        if ISEARCH_PREFIXES.contains(&keys.as_str()) {
            self.describe_key = Some(format!("{keys} "));
            return;
        }
        let info = match ISEARCH_BINDINGS.iter().find(|(key, _)| *key == keys) {
            Some((key, command)) => Info::new("Isearch key", &[(*key, *command)]),
            None => Info::new("Isearch key", &[(keys.as_str(), "is undefined")]),
        };
        self.show_isearch_help(cx, info);
    }

    /// Put a help box up over the buffer — Emacs's `*Help*` window, which the
    /// next key of the search takes down again.
    fn show_isearch_help(&mut self, cx: &mut Context, info: Info) {
        cx.editor.autoinfo = Some(info);
        self.isearch_help = true;
    }

    /// Emacs `minibuffer-depth-indicate-mode`: the `[N]` a recursive minibuffer
    /// is prefixed with, empty for the outermost one and while the mode is off.
    ///
    /// The regexp `M-r` / `M-s` read is read in a recursive minibuffer too — it is
    /// one prompt component here rather than two, but `minibuffer-depth` counts it,
    /// so the read is one deeper than the prompt it interrupted.
    fn depth_prefix(&self) -> String {
        let depth = self.depth + usize::from(self.history_search_read.is_some());
        if depth > 1 && DEPTH_INDICATE.load(Ordering::Relaxed) {
            format!("[{}]", depth)
        } else {
            String::new()
        }
    }

    /// Emacs `minibuffer-electric-default-mode`: the `(default X)` a prompt with a
    /// default names in its prompt string. The mode makes that segment *electric* —
    /// it is there while the input is still the empty one the minibuffer opened
    /// with, and gone the moment anything is typed, because typing is what makes
    /// the default no longer what `RET` would run. Empty when the mode is off or
    /// the prompt has no default. (Emacs's `minibuffer-eldef-shorten-default` picks
    /// the `[X]` spelling instead; it defaults off, so this is the long form.)
    fn default_segment(&self, editor: &Editor) -> String {
        if !ELECTRIC_DEFAULT.load(Ordering::Relaxed) || !self.line.is_empty() {
            return String::new();
        }
        match self.first_history_completion(editor) {
            Some(default) if !default.is_empty() => format!(" (default {})", default),
            _ => String::new(),
        }
    }

    /// Emacs `rfn-eshadow-update-overlay` (`file-name-shadow-mode`): how many
    /// leading bytes of the typed file name are *shadowed* — the part that
    /// resolving the name throws away, which is what typing a second absolute
    /// name over the first leaves behind. Emacs's rule is the longest prefix
    /// whose removal leaves `substitute-in-file-name` returning the same name;
    /// Emacs binary-searches for it, a command line is short enough to scan.
    ///
    /// zmax has no file-name *category* on a prompt the way Emacs's minibuffer
    /// does, so the shadow is keyed on the input itself looking like a path.
    fn shadow_end(&self) -> usize {
        if !FILE_NAME_SHADOW.load(Ordering::Relaxed) || !self.line.starts_with(['/', '~', '.', '$'])
        {
            return 0;
        }
        let goal = substitute_in_file_name(&self.line);
        self.line
            .char_indices()
            .filter(|&(i, _)| substitute_in_file_name(&self.line[i..]) == goal)
            .map(|(i, _)| i)
            .next_back()
            .unwrap_or(0)
    }

    /// Emacs `previous-matching-history-element` (`M-r`) / `next-matching-history-element`
    /// (`M-s`): open the recursive minibuffer that reads the regexp to search the
    /// history for. The answer being typed is *not* that regexp, so it is set aside
    /// and comes back whatever the search does.
    ///
    /// The prompt is the one `format-prompt` builds in simple.el — "Previous
    /// element matching regexp" / "Next element matching regexp" — with the newest
    /// `minibuffer-history-search-history` entry shown as ` (default REGEXP)`,
    /// because that is what empty input reuses.
    pub(crate) fn begin_history_search(&mut self, editor: &Editor, backward: bool, count: usize) {
        // A prompt that keeps no history has nothing to search, and the read is not
        // recursive into itself: `M-r` while it is open would lose the answer the
        // first one set aside.
        if self.history_register.is_none() || self.history_search_read.is_some() {
            return;
        }
        let prompt = history_search_prompt(backward, &last_history_search_regexp());
        self.history_search_read = Some(HistorySearchRead {
            line: std::mem::take(&mut self.line),
            cursor: std::mem::replace(&mut self.cursor, 0),
            prompt: std::mem::replace(&mut self.prompt, Cow::Owned(prompt)),
            backward,
            count: count.max(1),
        });
        self.recalculate_completion(editor);
    }

    /// `Enter` in that recursive minibuffer: the regexp is read, the minibuffer
    /// underneath comes back, and the search runs on it. "Use the last regexp
    /// specified, by default, if input is empty" (simple.el) — and empty input with
    /// no last regexp is the `user-error` "No history search regexp".
    fn finish_history_search(&mut self, cx: &mut Context) {
        let Some(saved) = self.history_search_read.take() else {
            return;
        };
        let typed = std::mem::take(&mut self.line);
        self.prompt = saved.prompt;
        self.line = saved.line;
        self.cursor = saved.cursor;
        self.recalculate_completion(cx.editor);
        let pattern = if typed.is_empty() {
            last_history_search_regexp()
        } else {
            typed
        };
        if pattern.is_empty() {
            cx.editor.set_error("No history search regexp");
            return;
        }
        push_history_search_regexp(&pattern);
        // Emacs's N argument: walk N matching entries, stopping at the first
        // step that finds none (which reports "No earlier matching history item").
        for _ in 0..saved.count {
            let before = self.history_pos;
            self.apply_matching_history(cx, &pattern, saved.backward);
            if self.history_pos == before {
                break;
            }
        }
    }

    /// `Esc` / `C-g` in that recursive minibuffer (`abort-recursive-edit`): the
    /// regexp is dropped and the answer underneath comes back exactly as it was,
    /// cursor included. No search runs.
    fn cancel_history_search(&mut self, editor: &Editor) {
        let Some(saved) = self.history_search_read.take() else {
            return;
        };
        self.prompt = saved.prompt;
        self.line = saved.line;
        self.cursor = saved.cursor;
        self.recalculate_completion(editor);
    }

    /// The history walk `previous-matching-history-element` does (simple.el): step
    /// one entry at a time — older when `backward`, newer when not — until one
    /// matches `pattern`. Returns its index in oldest-first order, the entry, and
    /// where point goes in it; `Ok(None)` is "no (further) matching entry".
    ///
    /// Point lands *in* the match: Emacs matches `".*\(REGEXP\)"` going back, so
    /// the start of the **last** match on the entry, and plain `REGEXP` going on,
    /// so the end of the **first** one.
    fn find_matching_history(
        &self,
        editor: &Editor,
        pattern: &str,
        backward: bool,
    ) -> Result<Option<(usize, String, usize)>, String> {
        let Some(register) = self.history_register else {
            return Ok(None);
        };
        // "history elements are matched case-insensitively if `case-fold-search'
        // is non-nil, but an uppercase letter in REGEXP makes the search
        // case-sensitive" (simple.el) — which is zmax's `search.smart-case`.
        let case_insensitive =
            editor.config().search.smart_case && !pattern.chars().any(char::is_uppercase);
        let regex = regex::RegexBuilder::new(pattern)
            .case_insensitive(case_insensitive)
            .build()
            .map_err(|_| format!("Invalid regexp: {pattern}"))?;
        // `change_history` counts the history from its oldest entry, and the ring
        // reads most-recent first, so it is reversed to share that index.
        let entries: Vec<String> = match editor.registers.read(register, editor) {
            Some(values) => values.map(|value| value.to_string()).rev().collect(),
            None => return Ok(None),
        };
        let start = self.history_pos.unwrap_or(entries.len());
        let found = if backward {
            (0..start.min(entries.len()))
                .rev()
                .find(|&i| regex.is_match(&entries[i]))
        } else {
            (start + 1..entries.len()).find(|&i| regex.is_match(&entries[i]))
        };
        Ok(found.map(|index| {
            let entry = entries[index].clone();
            let offset = matching_history_point(&regex, &entry, backward);
            (index, entry, offset)
        }))
    }

    /// Put the entry the walk found on the line — Emacs's
    /// `delete-minibuffer-contents` + `insert` + `goto-char` — and report the
    /// `user-error` it signals when there is none to put there.
    fn apply_matching_history(&mut self, cx: &mut Context, pattern: &str, backward: bool) {
        match self.find_matching_history(cx.editor, pattern, backward) {
            Err(e) => cx.editor.set_error(e),
            Ok(None) => cx.editor.set_error(if backward {
                "No earlier matching history item"
            } else {
                "No later matching history item"
            }),
            Ok(Some((index, entry, offset))) => {
                (self.callback_fn)(cx, &self.line, PromptEvent::Abort);
                self.line = entry;
                self.history_pos = Some(index);
                self.cursor = clamp_to_boundary(&self.line, offset);
                self.fire_update(cx);
                self.recalculate_completion(cx.editor);
            }
        }
    }

    /// Emacs `minibuffer-complete-defaults` (`C-x DOWN`): offer the prompt's
    /// default — the value it runs when the line is empty — as the completion.
    pub fn complete_from_default(&mut self, editor: &Editor) -> bool {
        let Some(default) = self
            .first_history_completion(editor)
            .map(|entry| entry.to_string())
        else {
            return false;
        };
        self.completion = vec![((0..), Span::raw(default))];
        self.exit_selection();
        true
    }
}

const BASE_WIDTH: u16 = 30;

impl Prompt {
    pub fn render_prompt(&mut self, area: Rect, surface: &mut Surface, cx: &mut Context) {
        let theme = &cx.editor.theme;
        let prompt_color = theme.get("ui.text");
        let completion_color = theme.get("ui.menu");
        let selected_color = theme.get("ui.menu.selected");
        let suggestion_color = theme.get("ui.text.inactive");
        let background = theme.get("ui.background");
        // completion

        let max_len = self
            .completion
            .iter()
            .map(|(_, completion)| completion.content.len() as u16)
            .max()
            .unwrap_or(BASE_WIDTH)
            .max(BASE_WIDTH);

        // vim `wildoptions=pum`: one candidate per row (a popup menu) instead of
        // zmax's multi-column list. `icomplete-vertical-mode` lays the
        // candidates out the same way, one per line.
        let cols = if wildoptions_pum() || icomplete_vertical_enabled() {
            1
        } else {
            std::cmp::max(1, area.width / max_len)
        };
        let col_width = (area.width.saturating_sub(cols)) / cols;

        let height = (self.completion.len() as u16)
            .div_ceil(cols)
            .min(10) // at most 10 rows (or less)
            .min(area.height.saturating_sub(1));

        let completion_area = Rect::new(
            area.x,
            (area.height - height).saturating_sub(1),
            area.width,
            height,
        );

        // Under `icomplete-mode` the candidates live on the prompt line itself
        // (drawn further down), so the list above it is not also drawn — that
        // inline display *is* what the mode is. Vertical icomplete keeps the
        // list, since one-candidate-per-line is exactly what it asks for.
        let icomplete_inline = icomplete_enabled() && !icomplete_vertical_enabled();

        if completion_area.height > 0 && !self.completion.is_empty() && !icomplete_inline {
            let area = completion_area;
            let background = theme.get("ui.menu");

            let items = height as usize * cols as usize;

            let offset = self
                .selection
                .map(|selection| selection / items * items)
                .unwrap_or_default();

            surface.clear_with(area, background);

            let mut row = 0;
            let mut col = 0;

            for (i, (_range, completion)) in
                self.completion.iter().enumerate().skip(offset).take(items)
            {
                let is_selected = Some(i) == self.selection;

                let completion_item_style = if is_selected {
                    selected_color
                } else {
                    completion_color.patch(completion.style)
                };

                surface.set_stringn(
                    area.x + col * (1 + col_width),
                    area.y + row,
                    &completion.content,
                    col_width.saturating_sub(1) as usize,
                    completion_item_style,
                );

                row += 1;
                if row > area.height - 1 {
                    row = 0;
                    col += 1;
                }
            }
        }

        if let Some(doc) = (self.doc_fn)(&self.line) {
            let mut text = ui::Text::new(doc.to_string());

            let max_width = BASE_WIDTH * 3;
            let horizontal_padding = 2; // border + margin
            let vertical_padding = 1; // border only
            let text_width = max_width - horizontal_padding * 2;

            let viewport = area;

            let (_width, height) = ui::text::required_size(&text.contents, text_width);

            let area = viewport.intersection(Rect::new(
                completion_area.x,
                completion_area
                    .y
                    .saturating_sub(height + vertical_padding * 2),
                max_width,
                height + vertical_padding * 2,
            ));

            let background = theme.get("ui.help");
            surface.clear_with(area, background);

            let block = Block::bordered()
                // .title(self.title.as_str())
                .border_style(background);

            let inner = block.inner(area).inner(Margin::horizontal(1));

            block.render(area, surface);
            text.render(inner, surface, cx);
        }

        let line = area.height - 1;
        surface.clear_with(area.clip_top(line), background);
        // render buffer text, behind the `[N]` of a recursive minibuffer when
        // `minibuffer-depth-indicate-mode` is on
        let depth = self.depth_prefix();
        surface.set_string(area.x, area.y + line, &depth, prompt_color);
        surface.set_string(
            area.x + depth.len() as u16,
            area.y + line,
            &self.prompt,
            prompt_color,
        );
        // `minibuffer-electric-default-mode`: the `(default X)` segment, which is
        // on screen only while nothing has been typed over the default.
        let default = self.default_segment(cx.editor);
        surface.set_string(
            area.x + (depth.len() + self.prompt.len()) as u16,
            area.y + line,
            &default,
            suggestion_color,
        );

        self.line_area = area
            .clip_left((depth.len() + self.prompt.len() + default.len()) as u16)
            .clip_top(line)
            .clip_right(2);

        if self.line.is_empty() {
            self.anchor = 0;
            // Show the most recently entered value as a suggestion — unless the
            // electric `(default X)` segment above is already naming it.
            if let Some(suggestion) = self
                .first_history_completion(cx.editor)
                .filter(|_| default.is_empty())
            {
                surface.set_string(
                    self.line_area.x,
                    self.line_area.y,
                    &suggestion,
                    suggestion_color,
                );
            }
        } else if self.masked {
            // A password: show only its length, never its characters.
            self.anchor = 0;
            self.truncate_start = false;
            self.truncate_end = false;
            let stars = "*".repeat(self.line.chars().count());
            surface.set_string(self.line_area.x, self.line_area.y, &stars, prompt_color);
        } else if let Some((language, loader)) = self.language.as_ref() {
            let mut text: ui::text::Text = crate::ui::markdown::highlighted_code_block(
                &self.line,
                language,
                Some(&cx.editor.theme),
                &loader.load(),
                None,
            )
            .into();
            text.render(self.line_area, surface, cx);
        } else {
            let line_width = self.line_area.width as usize;

            if self.line.width() < line_width {
                self.anchor = 0;
            } else if self.cursor <= self.anchor {
                // Ensure the grapheme under the cursor is in view.
                self.anchor = self.line[..self.cursor]
                    .grapheme_indices(true)
                    .next_back()
                    .map(|(i, _)| i)
                    .unwrap_or_default();
            } else if self.line[self.anchor..self.cursor].width() > line_width {
                // Set the anchor to the last grapheme cluster before the width is exceeded.
                let mut width = 0;
                self.anchor = self.line[..self.cursor]
                    .grapheme_indices(true)
                    .rev()
                    .find_map(|(idx, g)| {
                        width += g.width();
                        if width > line_width {
                            Some(idx + g.len())
                        } else {
                            None
                        }
                    })
                    .unwrap();
            }

            self.truncate_start = self.anchor > 0;
            self.truncate_end = self.line[self.anchor..].width() > line_width;

            // if we keep inserting characters just before the end elipsis, we move the anchor
            // so that those new characters are displayed
            if self.truncate_end && self.line[self.anchor..self.cursor].width() >= line_width {
                // Move the anchor forward by one non-zero-width grapheme.
                self.anchor += self.line[self.anchor..]
                    .grapheme_indices(true)
                    .find_map(|(idx, g)| {
                        if g.width() > 0 {
                            Some(idx + g.len())
                        } else {
                            None
                        }
                    })
                    .unwrap();
            }

            // `file-name-shadow-mode`: the leading part of the name that resolving
            // it throws away is dimmed rather than shown as live input.
            let shadow_end = self.shadow_end();
            let anchor = self.anchor;
            surface.set_string_anchored(
                self.line_area.x,
                self.line_area.y,
                self.truncate_start,
                self.truncate_end,
                &self.line.as_str()[self.anchor..],
                line_width,
                |offset| {
                    if anchor + offset < shadow_end {
                        suggestion_color
                    } else {
                        prompt_color
                    }
                },
            );
        }

        // `icomplete-mode`: the prospects sit after the input on the prompt
        // line, which is where the minibuffer shows them in Emacs.
        if icomplete_inline {
            let typed = self.line.width() as u16;
            let x = self.line_area.x.saturating_add(typed);
            let room = self.line_area.right().saturating_sub(x) as usize;
            if room > 0 {
                let candidates: Vec<String> = self
                    .completion
                    .iter()
                    .map(|(_, span)| span.content.to_string())
                    .collect();
                // zmax prompts do not carry Emacs's `require-match` flag, which
                // is the only thing that changes the brackets, so they are
                // always the permissive `[…]` pair.
                let prospects = icomplete_completions(&self.line, &candidates, false, room);
                surface.set_string_truncated(
                    x,
                    self.line_area.y,
                    &prospects,
                    room,
                    |_| completion_color,
                    true,
                    false,
                );
            }
        }
    }
}

impl Component for Prompt {
    fn handle_event(&mut self, event: &Event, cx: &mut Context) -> EventResult {
        let event = match event {
            Event::Paste(data) => {
                self.insert_str(data, cx.editor);
                self.recalculate_completion(cx.editor);
                return EventResult::Consumed(None);
            }
            Event::Key(event) => *event,
            Event::Resize(..) => return EventResult::Consumed(None),
            // Prompt is a modal and should consume mouse events so clicks don't fall
            // through to the editor underneath. vim `c_<LeftMouse>` is the one that
            // means something here: a click on the command line moves its cursor.
            Event::Mouse(event) => {
                if event.kind == MouseEventKind::Down(MouseButton::Left)
                    && event.row == self.line_area.y
                    && (self.line_area.x..self.line_area.right()).contains(&event.column)
                {
                    self.move_to_column(event.column);
                    // Emacs: `down-mouse-1` in the minibuffer while a search is
                    // running is `isearch-edit-string` — the click is aimed at
                    // the search string, so it is edited rather than searched
                    // with until `RET` resumes the search.
                    self.isearch_edit_string(cx);
                }
                // Emacs `isearch-yank-x-selection`: `mouse-2` in the echo area
                // while a search is running appends the selection to the search
                // string, the same text `C-y` would take from the clipboard.
                if event.kind == MouseEventKind::Down(MouseButton::Middle)
                    && event.row == self.line_area.y
                    && self.isearch.is_some()
                {
                    let selection = cx
                        .editor
                        .registers
                        .read('+', cx.editor)
                        .and_then(|mut it| it.next())
                        .map(|v| v.to_string())
                        .unwrap_or_default();
                    if selection.is_empty() {
                        cx.editor.set_error("Clipboard is empty");
                    } else {
                        self.isearch_add(cx, &selection);
                    }
                }
                return EventResult::Consumed(None);
            }
            _ => return EventResult::Ignored(None),
        };

        let close_fn = EventResult::Consumed(Some(Box::new(|compositor: &mut Compositor, _| {
            // remove the layer
            compositor.pop();
        })));

        // vim `wildcharm`: the key that starts command-line completion from inside
        // a mapping. It does exactly what `<Tab>` does, so it is folded into it.
        let event = match wildcharm() {
            Some(key) if key == event => key!(Tab),
            _ => event,
        };

        // vim `c_CTRL-V`/`c_CTRL-Q`: the key after it is data, not a command — so
        // it is taken before any binding below can claim it.
        if self.handle_literal(event, cx) {
            self.fire_update(cx);
            return EventResult::Consumed(None);
        }
        // Emacs's `*Help*` window: the isearch help box stays up until the search
        // moves on, which the next key is.
        if std::mem::take(&mut self.isearch_help) {
            cx.editor.autoinfo = None;
        }
        // Emacs `isearch-describe-key` (`C-h k`): the key after it is one to look
        // up, not one to run — so it is taken before any binding can claim it.
        if self.describe_key.is_some() {
            self.isearch_describe_key(cx, event);
            return EventResult::Consumed(None);
        }
        // `CTRL-\` only means something together with the key that follows it.
        let ctrl_backslash = std::mem::take(&mut self.pending_ctrl_backslash);
        // Emacs isearch `M-s` and minibuffer `C-x`: both are prefixes — the key
        // that follows says what they do.
        let isearch_s = std::mem::take(&mut self.pending_isearch_s);
        let ctrl_x = std::mem::take(&mut self.pending_ctrl_x);
        let ctrl_x_8 = std::mem::take(&mut self.pending_ctrl_x_8);
        // Emacs isearch `C-h`: the help key is a prefix inside a search — the key
        // after it picks which help `isearch-help-map` shows.
        let ctrl_h = std::mem::take(&mut self.pending_ctrl_h);

        // Inside an incremental search the Emacs isearch keys are live. The ones
        // that need a control chord (`C-s`, `C-w`, `C-y`, `C-q`, `C-g`) are the
        // vim command-line keys too, so in the vim presets — where the search
        // prompt *is* vim's — those keep their vim meaning and only the Meta keys
        // (which vim's command line does not use) are Emacs's.
        let isearch = self.isearch.is_some();
        let isearch_ctl = isearch && !cx.editor.vim_semantics;
        // vim incsearch `C-g`/`C-t`: the vim presets' next/prev-match cycle.
        let vim_cycle = self.incsearch_cycle.is_some() && cx.editor.vim_semantics;
        // The chords Emacs's isearch spells with both Control and Meta (`C-M-y`
        // yank char, `C-M-w` yank symbol-or-char, `C-M-d` del char, `C-M-z` yank
        // until char). The key macros carry one modifier each, so they are matched
        // by hand.
        let ctrl_meta = {
            use zmax_view::keyboard::KeyModifiers;
            event.modifiers == KeyModifiers::CONTROL | KeyModifiers::ALT
        };

        // vim 'cedit' (`CTRL-F` by default): open the command-line window on what
        // has been typed so far — the history in a real buffer, where the line can
        // be edited with ordinary commands before `<CR>` runs it (cmdline.txt).
        // Only in the vim presets, where the key is vim's: in the Emacs ones
        // `C-f` is `forward-char`, the arm further down. Not while a register name
        // or a literal key is awaited, where the next key is data, and not in a
        // nested read, whose line is an expression rather than a command.
        if cx.editor.vim_semantics
            && !self.pending_register
            && self.next_char_handler.is_none()
            && !self.in_nested_read()
            && cedit_pressed(event)
        {
            if let Some(command) = self.cmdline_window_command() {
                let line = std::mem::take(&mut self.line);
                (self.callback_fn)(cx, &line, PromptEvent::Abort);
                return EventResult::Consumed(Some(Box::new(
                    move |compositor: &mut Compositor, cx: &mut Context| {
                        compositor.pop();
                        open_cmdline_window(compositor, cx, command, &line);
                    },
                )));
            }
        }

        match event {
            // vim `c_CTRL-\_e {expr}`: in the nested `=` prompt these abandon the
            // expression, not the command line — that comes back untouched.
            ctrl!('c') | key!(Esc) if self.cmdline_eval.is_some() => {
                self.cancel_cmdline_eval(cx.editor);
            }
            // Emacs `abort-recursive-edit`: in the recursive minibuffer `M-r` /
            // `M-s` opened these drop the regexp, not the answer underneath.
            ctrl!('c') | ctrl!('g') | key!(Esc) if self.history_search_read.is_some() => {
                self.cancel_history_search(cx.editor);
            }
            ctrl!('c') | key!(Esc) => {
                (self.callback_fn)(cx, &self.line, PromptEvent::Abort);
                return close_fn;
            }
            // vim `c_CTRL-\_CTRL-N` / `c_CTRL-\_CTRL-G`: abandon the command line
            // and go back to Normal mode (from wherever the prompt was opened).
            ctrl!('n') | ctrl!('g') if ctrl_backslash => {
                (self.callback_fn)(cx, &self.line, PromptEvent::Abort);
                cx.editor.mode = Mode::Normal;
                return close_fn;
            }
            // vim `c_CTRL-\_e {expr}`: the command line is replaced by the result
            // of evaluating an expression, which is asked for in a nested `=`
            // prompt. The line typed so far is set aside until `<Enter>` finishes
            // the expression, so the expression can read it (`getcmdline()`).
            key!('e') if ctrl_backslash => self.begin_cmdline_eval(cx.editor),
            ctrl!('\\') => self.pending_ctrl_backslash = true,
            // vim `c_CTRL-R_CTRL-R` / `_CTRL-O` / `_CTRL-P {regname}`: insert the
            // register literally / without indent changes. The insert below is
            // already literal, so these just wait for the register name.
            ctrl!('r') | ctrl!('o') | ctrl!('p') if self.pending_register => {}
            // kakoune `<c-r>` with a modifier on the register key: Alt inserts
            // every value in the register joined with spaces, Control quotes what
            // goes in (keys.asciidoc; input_handler.cc:762-773). It sits here, in
            // front of every other Alt/Control chord, because while the register
            // name is awaited the next key *is* the register name — but behind
            // vim's `c_CTRL-R CTRL-R` / `_CTRL-O` / `_CTRL-P` above, which are the
            // same keystroke and keep their vim meaning.
            KeyEvent {
                code: KeyCode::Char(c),
                modifiers,
            } if self.pending_register
                && modifiers.intersects(
                    zmax_view::keyboard::KeyModifiers::ALT
                        | zmax_view::keyboard::KeyModifiers::CONTROL,
                ) =>
            {
                use zmax_view::keyboard::KeyModifiers;
                self.pending_register = false;
                self.next_char_handler = None;
                let text = Self::register_text(
                    cx.editor,
                    c,
                    modifiers.contains(KeyModifiers::ALT),
                    modifiers.contains(KeyModifiers::CONTROL),
                );
                self.insert_str(&text, cx.editor);
                self.fire_update(cx);
                return EventResult::Consumed(None);
            }

            // ── Emacs isearch: the `M-s` toggle map ─────────────────────────
            // Emacs `isearch-toggle-regexp` (`M-s r`), `-word` (`M-s w`),
            // `-symbol` (`M-s _`), `-case-fold` (`M-s c`), `-invisible` (`M-s i`),
            // `isearch-yank-line` (`M-s C-e`) and `isearch-occur` (`M-s o`).
            key!('r') if isearch_s => self.isearch_toggle(cx, IsearchToggle::Regexp),
            key!('w') if isearch_s => self.isearch_toggle(cx, IsearchToggle::Word),
            key!('_') if isearch_s => self.isearch_toggle(cx, IsearchToggle::Symbol),
            key!('c') if isearch_s => self.isearch_toggle_case(cx),
            key!('i') if isearch_s => self.isearch_toggle(cx, IsearchToggle::Invisible),
            ctrl!('e') if isearch_s => self.isearch_yank(cx, IsearchYank::Line),
            // `isearch-beginning-of-buffer` / `isearch-end-of-buffer`: jump the
            // search to the first / last match in the whole buffer.
            alt!('<') if isearch_s => self.isearch_from_edge(cx, true),
            alt!('>') if isearch_s => self.isearch_from_edge(cx, false),
            key!('o') if isearch_s => {
                // Emacs `isearch-occur`: end the search and list every line the
                // search string matches, in an occur buffer.
                let pattern = self.pattern();
                self.submit(cx);
                let (doc_id, view_id) = {
                    let (view, doc) = current!(cx.editor);
                    (doc.id(), view.id)
                };
                crate::commands::occur_run(cx.editor, cx.jobs, doc_id, view_id, &pattern);
                return close_fn;
            }
            // `M-s '` (char folding) and `M-s SPC` (lax whitespace): a quote and a
            // space have no key-macro spelling, so they are matched by hand.
            KeyEvent {
                code: KeyCode::Char('\''),
                ..
            } if isearch_s => self.isearch_toggle(cx, IsearchToggle::CharFold),
            KeyEvent {
                code: KeyCode::Char(' '),
                ..
            } if isearch_s => self.isearch_toggle(cx, IsearchToggle::LaxWhitespace),

            // ── Emacs minibuffer: the `C-x` completion prefix ───────────────
            // `minibuffer-complete-history`: complete what is typed against the
            // prompt's history rather than its completion table.
            key!(Up) if ctrl_x => {
                if self.complete_from_history(cx.editor) == 0 {
                    cx.editor.set_error("No matching history element");
                }
            }
            // `minibuffer-complete-defaults`: complete against the prompt's
            // default — the value it runs when the line is left empty.
            key!(Down) if ctrl_x => {
                if !self.complete_from_default(cx.editor) {
                    cx.editor.set_error("No default to complete from");
                }
            }
            // ── Emacs minibuffer: the completion keys of a completing read ───
            // `minibuffer-completion-help` (`?`): show the list of completions.
            // Only in a completing read — on the `:` line and in a search, `?` is
            // a character of the argument being typed.
            key!('?') if self.is_completing_read() => {
                self.list_completion_before_cursor(cx.editor);
                if self.completion.is_empty() {
                    cx.editor.set_status("No completions");
                }
            }
            // `minibuffer-next-completion` (`M-DOWN`) / `minibuffer-previous-completion`
            // (`M-UP`): walk the candidate list. `minibuffer-completion-auto-choose`
            // defaults to t, so moving also puts the candidate on the line — which
            // is what selecting one here does.
            alt!(Down) => {
                if self.completion.is_empty() {
                    cx.editor.set_status("No completions");
                } else {
                    self.change_completion_selection(CompletionDirection::Forward);
                }
            }
            alt!(Up) => {
                if self.completion.is_empty() {
                    cx.editor.set_status("No completions");
                } else {
                    self.change_completion_selection(CompletionDirection::Backward);
                }
            }
            // `minibuffer-choose-completion` (`M-RET`): take the candidate the list
            // is on (the first when none is selected) and accept the minibuffer
            // with it.
            alt!(Enter) => {
                let index = self.selection.unwrap_or(0);
                if !self.apply_completion(index) {
                    cx.editor.set_status("No completions to choose from");
                } else if self.submit(cx) {
                    return close_fn;
                }
            }
            // `minibuffer-complete-word` (`SPC`): complete only as far as the next
            // word boundary. Emacs does not bind this where the argument may
            // contain spaces (file names, the command line); here the same rule is
            // "a completing read, and only when there is something to complete" —
            // a space that completes nothing is just a space.
            KeyEvent {
                code: KeyCode::Char(' '),
                modifiers,
            } if modifiers.is_empty() && self.is_completing_read() => {
                if self.complete_word(cx.editor) {
                    self.fire_update(cx);
                } else {
                    self.insert_char(' ', cx);
                    self.fire_update(cx);
                }
            }
            // `isearch-toggle-specified-input-method` (`C-^` in a search): turn on
            // a *named* input method for the search string. Outside a search the
            // key keeps its vim meaning (`c_CTRL-^`, further down).
            ctrl!('^') if isearch => {
                return EventResult::Consumed(Some(Box::new(|compositor, _cx| {
                    compositor.push(Box::new(crate::commands::specified_input_method_prompt()));
                })));
            }
            // `isearch-transient-input-method` (`C-x \`): the next character typed
            // goes into the search string through the language input method, and
            // the method turns itself off again after that one character.
            KeyEvent {
                code: KeyCode::Char('\\'),
                ..
            } if ctrl_x && isearch => {
                self.next_char_handler = Some(Box::new(|prompt, c, cx| {
                    let text = transient_lang_map(c);
                    let quoted = prompt.isearch_quote(&text);
                    prompt.insert_str(&quoted, cx.editor);
                }));
            }
            // ── emacs isearch: `C-x 8`, the insert-char prefix ───────────────
            // isearch-mode-map keeps `C-x 8` live inside a search, so a character
            // can be added to the search string by NAME: `C-x 8 RET` reads a
            // Unicode name (isearch-char-by-name) and `C-x 8 e RET` an emoji name
            // (isearch-emoji-by-name). zmax's prompt uses `C-x` as the minibuffer
            // completion prefix, so the `8` leg is opened only while a search is
            // running and everything else under `C-x` is untouched.
            KeyEvent {
                code: KeyCode::Char('8'),
                ..
            } if ctrl_x && isearch => {
                self.pending_ctrl_x_8 = Some(false);
            }
            KeyEvent {
                code: KeyCode::Char('e'),
                ..
            } if ctrl_x_8 == Some(false) => {
                self.pending_ctrl_x_8 = Some(true);
            }
            key!(Enter) if ctrl_x_8 == Some(false) => {
                return EventResult::Consumed(Some(Box::new(|compositor, cx| {
                    run_isearch_command(compositor, cx, "isearch_char_by_name");
                })));
            }
            key!(Enter) if ctrl_x_8 == Some(true) => {
                return EventResult::Consumed(Some(Box::new(|compositor, cx| {
                    run_isearch_command(compositor, cx, "isearch_emoji_by_name");
                })));
            }
            ctrl!('x') => {
                self.pending_ctrl_x = true;
            }

            // ── Emacs isearch: `isearch-help-map` (the help key) ─────────────
            // `isearch-help-for-help` (`C-h C-h`, `C-h ?`, `C-h f1`), then the
            // options it lists: `isearch-describe-bindings` (`b`),
            // `isearch-describe-key` (`k`), `isearch-describe-mode` (`m`) and
            // `help-quit` (`q`), which only takes the help box down again.
            ctrl!('h')
            | KeyEvent {
                code: KeyCode::F(1),
                ..
            }
            | KeyEvent {
                code: KeyCode::Char('?'),
                ..
            } if ctrl_h => self.isearch_help_for_help(cx),
            key!('b') if ctrl_h => self.isearch_describe_bindings(cx),
            key!('k') if ctrl_h => self.describe_key = Some(String::new()),
            key!('m') if ctrl_h => self.isearch_describe_mode(cx),
            key!('q') if ctrl_h => {}
            // The help key itself: a prefix while a search is running, where
            // Emacs binds `DEL` (and not it) to `isearch-delete-char`.
            ctrl!('h')
            | KeyEvent {
                code: KeyCode::F(1),
                ..
            } if isearch_ctl => self.pending_ctrl_h = true,

            // ── Emacs isearch: the keys inside an incremental search ─────────
            // `isearch-repeat-forward` / `-backward`: on to the next match — or,
            // with nothing typed yet, back to the previous search string.
            ctrl!('s') if isearch_ctl => self.isearch_repeat(cx, true),
            ctrl!('r') if isearch_ctl => self.isearch_repeat(cx, false),
            // `isearch-yank-word-or-char`: grow the search by the buffer text the
            // match is sitting in front of.
            ctrl!('w') if isearch_ctl => self.isearch_yank(cx, IsearchYank::WordOrChar),
            // `isearch-yank-kill`: grow the search by the most recent kill.
            ctrl!('y') if isearch_ctl => self.isearch_yank_kill(cx),
            // `isearch-yank-pop-only`: swap the kill `C-y` just appended for the
            // next-older one; anywhere else it only pops the last kill.
            alt!('y') if isearch => self.isearch_yank_pop_only(cx),
            // `isearch-quote-char`: the next character goes into the search string
            // as itself, quoted so a regexp search cannot read it as an operator.
            ctrl!('q') if isearch_ctl => {
                self.next_char_handler = Some(Box::new(|prompt, c, cx| {
                    let quoted = prompt.isearch_quote(&c.to_string());
                    prompt.insert_str(&quoted, cx.editor);
                }));
            }
            // `isearch-abort`: a search that has found what was asked for goes
            // back to where it started and quits; a failing one only rubs out
            // the characters that made it fail, so `C-g C-g` is what leaves one.
            ctrl!('g') if isearch_ctl => {
                if self.isearch_abort(cx) {
                    return close_fn;
                }
            }
            // `isearch-edit-string`: edit the search string rather than search
            // with every keystroke; `RET` resumes the search with the result.
            alt!('e') if isearch => self.isearch_edit_string(cx),
            // `isearch-yank-char` (`C-M-y`), `isearch-yank-symbol-or-char`
            // (`C-M-w`), `isearch-del-char` (`C-M-d`) and `isearch-yank-until-char`
            // (`C-M-z`), which reads the character to yank up to.
            KeyEvent {
                code: KeyCode::Char(c),
                ..
            } if isearch && ctrl_meta => match c {
                'y' => self.isearch_yank(cx, IsearchYank::Char),
                'w' => self.isearch_yank(cx, IsearchYank::WordOrChar),
                'd' => {
                    self.move_end();
                    self.delete_char_backwards(cx.editor);
                    self.fire_update(cx);
                }
                'z' => {
                    self.next_char_handler = Some(Box::new(|prompt, c, cx| {
                        let text = {
                            let (view, doc) = current_ref!(cx.editor);
                            let text = doc.text().slice(..);
                            let pos = doc.selection(view.id).primary().to();
                            search::grab_until_char(text, pos, c)
                        };
                        let quoted = prompt.isearch_quote(&text);
                        prompt.move_end();
                        prompt.insert_str(&quoted, cx.editor);
                    }));
                }
                _ => {}
            },
            // `isearch-toggle-case-fold` (`M-c`) and `isearch-toggle-regexp`
            // (`M-r`) — outside a search `M-r` is the minibuffer's
            // `previous-matching-history-element`.
            alt!('c') if isearch => self.isearch_toggle_case(cx),
            alt!('r') => {
                if isearch {
                    self.isearch_toggle(cx, IsearchToggle::Regexp);
                } else {
                    self.begin_history_search(cx.editor, true, 1);
                }
            }
            // dte `M-r` — "Reverse search direction": the search in flight turns
            // round. `A-r` above is the regexp toggle in both the vim and the
            // Emacs presets, so the direction toggle takes the shifted key.
            alt!('R') => self.isearch_reverse_direction(cx),
            // `isearch-complete`: complete the search string from the search ring.
            alt!(Tab) if isearch => self.isearch_complete(cx),
            // Emacs isearch `M-s`: the prefix of the toggle map above. Outside a
            // search it is the minibuffer's `next-matching-history-element`.
            alt!('s') => {
                self.pending_isearch_s = true;
                if !isearch {
                    self.pending_isearch_s = false;
                    self.begin_history_search(cx.editor, false, 1);
                }
            }

            // vim `c_CTRL-V` (and `c_CTRL-Q`): take the next key literally. `C-q`
            // only in the vim presets — in the Emacs ones it must not shadow the
            // completion-selection exit further down.
            ctrl!('v') => self.pending_literal = true,
            ctrl!('q') if cx.editor.vim_semantics => self.pending_literal = true,
            // vim `c_CTRL-]`: expand the `:cabbrev` in front of the cursor —
            // "used to expand an abbreviation without inserting any extra
            // characters" (map.txt), unlike the typed non-keyword character and
            // the `<CR>` that also trigger one.
            ctrl!(']') => {
                self.expand_cmdline_abbrev(cx.editor);
            }
            // vim `c_CTRL-^`: turn the `:lmap` language keymap off/on ('imsearch').
            ctrl!('^') => {
                let on = crate::commands::typed::toggle_lang_arg(false);
                cx.editor.set_status(if on {
                    "lmap on (imsearch=1)"
                } else {
                    "lmap off (imsearch=0)"
                });
            }
            // vim `c_<Insert>`: toggle overstrike.
            key!(Insert) => self.overstrike = !self.overstrike,
            // vim `c_CTRL-_`: toggle 'revins' for the command line, but only when
            // 'allowrevins' lets the key do it — that gate is the whole of what
            // 'allowrevins' is, and it covers Insert and Command-line mode alike.
            ctrl!('_') => crate::commands::typed::toggle_revins_cmdline(cx.editor),
            alt!('b') | ctrl!(Left) | shift!(Left) => self.move_cursor(Movement::BackwardWord(1)),
            alt!('f') | ctrl!(Right) | shift!(Right) => self.move_cursor(Movement::ForwardWord(1)),
            // vim `c_CTRL-B`: to the beginning of the command line — the same as
            // `<Home>`. In the Emacs presets `C-b` is `backward-char` instead.
            ctrl!('b') if cx.editor.vim_semantics => self.move_start(),
            ctrl!('b') | key!(Left) => self.move_cursor(Movement::BackwardChar(1)),
            ctrl!('f') | key!(Right) => self.move_cursor(Movement::ForwardChar(1)),
            ctrl!('e') | key!(End) => self.move_end(),
            // vim `c_CTRL-A`: insert every name matching the pattern in front of
            // the cursor. Only in the vim presets — in the Emacs ones `C-a` is
            // `beginning-of-line`, the arm below.
            ctrl!('a') if cx.editor.vim_semantics => {
                self.complete_insert_all_matches(cx.editor);
                self.fire_update(cx);
            }
            ctrl!('a') | key!(Home) => self.move_start(),
            // vim incsearch: C-g next match, C-t previous match (search prompts only;
            // in the Emacs presets `C-g` is `isearch-abort`, handled above).
            ctrl!('g') if vim_cycle => {
                let line = self.line.clone();
                if let Some(f) = &mut self.incsearch_cycle {
                    f(cx, &line, true);
                }
            }
            ctrl!('t') if vim_cycle => {
                let line = self.line.clone();
                if let Some(f) = &mut self.incsearch_cycle {
                    f(cx, &line, false);
                }
            }
            ctrl!('w') | alt!(Backspace) | ctrl!(Backspace) => {
                self.delete_word_backwards(cx.editor);
                self.fire_update(cx);
            }
            alt!('d') | alt!(Delete) | ctrl!(Delete) => {
                self.delete_word_forwards(cx.editor);
                self.fire_update(cx);
            }
            ctrl!('k') => {
                self.kill_to_end_of_line(cx.editor);
                self.fire_update(cx);
            }
            ctrl!('u') => {
                self.kill_to_start_of_line(cx.editor);
                self.fire_update(cx);
            }
            ctrl!('y') => {
                self.yank(cx.editor);
                self.fire_update(cx);
            }
            // Emacs `isearch-delete-char` (`DEL`): drop the last character of the
            // search string, which puts the search back where it was before it.
            ctrl!('h') | key!(Backspace) | shift!(Backspace) => {
                // vim `'digraph'`: `{char1}<BS>{char2}` enters a digraph on the
                // command line too — digraph.txt:93-95 has <Esc> ending "Insert
                // mode or Command-line mode" out of digraph entry, so both modes
                // have it. A `<BS>` with nothing armed remembers the character
                // before the cursor and does *not* delete; a second `<BS>`
                // cancels that and deletes normally, as in Insert mode.
                if self.arm_digraph() {
                    return EventResult::Consumed(None);
                }
                self.delete_char_backwards(cx.editor);
                self.fire_update(cx);
            }
            // vim `c_CTRL-D`: list the names matching the pattern in front of the
            // cursor and leave the line alone. In the Emacs presets `C-d` is
            // `delete-char`, so only the vim ones list.
            ctrl!('d') if cx.editor.vim_semantics => {
                self.list_completion_before_cursor(cx.editor);
            }
            ctrl!('d') | key!(Delete) => {
                self.delete_char_forwards(cx.editor);
                self.fire_update(cx);
            }
            ctrl!('s') => {
                let (view, doc) = current!(cx.editor);
                let text = doc.text().slice(..);

                use zmax_core::textobject;
                let range = textobject::textobject_word(
                    text,
                    doc.selection(view.id).primary(),
                    textobject::TextObject::Inside,
                    1,
                    false,
                );
                let line = text.slice(range.from()..range.to()).to_string();
                if !line.is_empty() {
                    self.insert_str(line.as_str(), cx.editor);
                    self.fire_update(cx);
                }
            }
            // vim `c_CTRL-\_e {expr}`: `<Enter>` finishes the expression rather
            // than the command line — the prompt stays open on the result.
            key!(Enter) | ctrl!('j') if self.cmdline_eval.is_some() => {
                self.finish_cmdline_eval(cx);
            }
            // Emacs `previous-matching-history-element` / `next-matching-history-element`:
            // `<Enter>` in the recursive minibuffer ends *that* read — the regexp is
            // what it returns — and the history search runs with it. The prompt
            // underneath stays open on whatever the search found.
            key!(Enter) | ctrl!('j') if self.history_search_read.is_some() => {
                self.finish_history_search(cx);
            }
            // Emacs `isearch-edit-string`: `RET` resumes the incremental search
            // with the edited string instead of ending the search — it takes a
            // second one to stop on the match that finds.
            key!(Enter) | ctrl!('j') if self.isearch_edit => {
                self.isearch_edit = false;
                self.fire_update(cx);
            }
            // Emacs `icomplete-fido-ret`: under `fido-mode` `RET` takes the top
            // candidate rather than the literal input.
            key!(Enter) | ctrl!('j') if FIDO_MODE.load(Ordering::Relaxed) => {
                if self.fido_ret(cx) {
                    return close_fn;
                }
            }
            // Emacs `isearch-exit` (`RET`) / `minibuffer-complete-and-exit`: take
            // what is typed — the search stops on the match it is showing.
            key!(Enter) | ctrl!('j') => {
                if self.submit(cx) {
                    return close_fn;
                }
            }
            // The recursive minibuffer `M-r` / `M-s` opens reads its regexp with a
            // history of its own (`minibuffer-history-search-history`), not the
            // answer's — and only that list's newest entry is kept, so there is
            // nothing to walk. Walking the answer's history here would replace the
            // answer the read set aside, which is what these keys must not do.
            alt!('p')
            | ctrl!('p')
            | key!(Up)
            | shift!(Up)
            | key!(PageUp)
            | alt!('n')
            | ctrl!('n')
            | key!(Down)
            | shift!(Down)
            | key!(PageDown)
                if self.history_search_read.is_some() => {}
            // Emacs `previous-history-element` (`M-p`, `UP`), which in a search is
            // `isearch-ring-retreat`: back to the search string used before this one.
            alt!('p') | ctrl!('p') | key!(Up) | shift!(Up) | key!(PageUp) => {
                if let Some(register) = self.history_register {
                    self.change_history(cx, register, CompletionDirection::Backward);
                }
            }
            // Emacs `next-history-element` (`M-n`, `DOWN`) / `isearch-ring-advance`.
            alt!('n') | ctrl!('n') | key!(Down) | shift!(Down) | key!(PageDown) => {
                if let Some(register) = self.history_register {
                    self.change_history(cx, register, CompletionDirection::Forward);
                }
            }
            key!(Tab) => {
                self.wild_complete(cx.editor, CompletionDirection::Forward);
                self.fire_update(cx)
            }
            shift!(Tab) => {
                self.wild_complete(cx.editor, CompletionDirection::Backward);
                self.fire_update(cx)
            }
            // Emacs `file-cache-minibuffer-complete` (`C-TAB`): complete the file
            // name from the file-name cache; repeating it cycles through the
            // directories that name was cached in.
            ctrl!(Tab) => {
                match self.file_cache_complete(cx.editor) {
                    Ok(msg) => cx.editor.set_status(msg),
                    Err(msg) => cx.editor.set_error(msg),
                }
                self.fire_update(cx)
            }
            ctrl!('l') => {
                // c_CTRL-L: complete the pattern in front of the cursor by the
                // longest prefix all the matches share — unlike <Tab>, it picks
                // none of them, so what it adds is always what you would type.
                self.complete_longest_common(cx.editor);
                self.fire_update(cx)
            }
            ctrl!('q') => self.exit_selection(),
            ctrl!('r') => {
                self.pending_register = true;
                self.completion = cx
                    .editor
                    .registers
                    .iter_preview()
                    .map(|(ch, preview)| (0.., format!("{} {}", ch, preview).into()))
                    .collect();
                self.next_char_handler = Some(Box::new(|prompt, c, context| {
                    prompt.insert_str(
                        &context
                            .editor
                            .registers
                            .first(c, context.editor)
                            .unwrap_or_default(),
                        context.editor,
                    );
                }));
                self.fire_update(cx);
                return EventResult::Consumed(None);
            }
            // any char event that's not mapped to any other combo
            KeyEvent {
                code: KeyCode::Char(c),
                modifiers: _,
            } => {
                // vim `'digraph'`: char2 of an armed `{char1}<BS>{char2}`
                // combines with char1, replacing it. A pair that is not a
                // digraph is not an error — char1 stays and `c` is inserted
                // after it, which is what makes a stray `<BS>` harmless.
                if let Some(dg) = self.take_digraph(c) {
                    self.delete_char_backwards(cx.editor);
                    self.insert_char(dg, cx);
                    self.fire_update(cx);
                    return EventResult::Consumed(None);
                }
                self.insert_char(c, cx);
                self.fire_update(cx);
            }
            _ => (),
        };

        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, cx: &mut Context) {
        self.render_prompt(area, surface, cx)
    }

    fn cursor(&self, area: Rect, editor: &Editor) -> (Option<Position>, CursorKind) {
        let area = area
            .clip_left(
                (self.depth_prefix().len() + self.prompt.len() + self.default_segment(editor).len())
                    as u16,
            )
            .clip_right(if self.prompt.is_empty() { 2 } else { 0 });

        let mut col = area.left() as usize + self.line[self.anchor..self.cursor].width();

        // ensure the cursor does not go beyond elipses
        if self.truncate_end
            && self.line[self.anchor..self.cursor].width() >= self.line_area.width as usize
        {
            col -= 1;
        }

        if self.truncate_start && self.cursor == self.anchor {
            col += self.line[self.cursor..]
                .graphemes(true)
                .next()
                .map_or(0, |g| g.width());
        }

        let line = area.height as usize - 1;

        (
            Some(Position::new(area.y as usize + line, col)),
            editor.config().cursor_shape.from_mode(Mode::Insert),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prompt_at(line: &str, area_x: u16, anchor: usize) -> Prompt {
        let mut p = Prompt::new(Cow::from(":"), None, |_, _| Vec::new(), |_, _, _| {});
        p.line = line.to_string();
        p.line_area = Rect::new(area_x, 0, 40, 1);
        p.anchor = anchor;
        p
    }

    /// vim `'digraph'` on the command line (`:h digraphs-use`, and digraph.txt:94
    /// which has <Esc> leaving digraph entry in "Insert mode or Command-line
    /// mode"). The prompt had no digraph entry at all: `<BS>` always deleted, so
    /// `a<BS>:` left `:` where vim gives `ä`.
    #[test]
    fn backspace_arms_a_digraph_only_when_the_option_is_on() {
        crate::commands::typed::vim_opt_store("digraph", "off".to_string());
        let mut p = prompt_at("a", 0, 0);
        p.cursor = 1;
        assert!(
            !p.arm_digraph(),
            "with 'nodigraph' <BS> is an ordinary delete"
        );
        assert_eq!(p.digraph_pending, None);

        crate::commands::typed::vim_opt_store("digraph", "on".to_string());
        let mut p = prompt_at("a", 0, 0);
        p.cursor = 1;
        assert!(p.arm_digraph(), "<BS> is swallowed and char1 remembered");
        assert_eq!(p.digraph_pending, Some('a'));

        // "To correct this, you will have to type <BS> e again": a second <BS>
        // cancels the arming and deletes.
        assert!(!p.arm_digraph());
        assert_eq!(p.digraph_pending, None);

        // Nothing before the cursor is nothing to combine with.
        let mut p = prompt_at("", 0, 0);
        assert!(!p.arm_digraph());
    }

    /// char2 combines, and a pair that is not a digraph leaves char1 alone
    /// rather than eating the keystroke.
    #[test]
    fn the_second_char_combines_or_is_inserted_plainly() {
        crate::commands::typed::vim_opt_store("digraph", "on".to_string());
        let mut p = prompt_at("a", 0, 0);
        p.cursor = 1;
        assert!(p.arm_digraph());
        assert_eq!(p.take_digraph(':'), Some('ä'));
        assert_eq!(p.digraph_pending, None, "the arming is spent");

        // `a` and `q` are not a digraph: no replacement, and the arming is still
        // consumed so the next key is not swallowed too.
        let mut p = prompt_at("a", 0, 0);
        p.cursor = 1;
        assert!(p.arm_digraph());
        assert_eq!(p.take_digraph('q'), None);
        assert_eq!(p.digraph_pending, None);

        // Nothing armed: an ordinary character is an ordinary character.
        assert_eq!(p.take_digraph(':'), None);
    }

    /// Emacs `previous-matching-history-element` puts point *in* the match, and
    /// which end of it depends on the direction: going back it matches
    /// `".*\(REGEXP\)"`, so the **last** match's start, and going on plain
    /// `REGEXP`, so the **first** match's end (simple.el). Landing at the end of
    /// the recalled entry instead — which is what history recall does — would lose
    /// that entirely.
    #[test]
    fn matching_history_point_follows_the_search_direction() {
        let re = regex::Regex::new("ab").unwrap();
        assert_eq!(matching_history_point(&re, "xabyab", true), 4);
        assert_eq!(matching_history_point(&re, "xabyab", false), 3);
        // An entry with no match at all (nothing the walk would return) leaves
        // point at the end rather than panicking on the missing match.
        assert_eq!(matching_history_point(&re, "zzz", true), 3);
    }

    /// The regexp `M-r` / `M-s` search the history with is read in a recursive
    /// minibuffer whose prompt is `format-prompt`'s (simple.el) — and the last
    /// regexp searched for is offered there as the default, because empty input
    /// reuses it.
    #[test]
    fn history_search_prompt_offers_the_last_regexp() {
        assert_eq!(
            history_search_prompt(true, ""),
            "Previous element matching regexp: "
        );
        assert_eq!(
            history_search_prompt(false, "^:w"),
            "Next element matching regexp (default ^:w): "
        );
    }

    /// vim `c_<LeftMouse>`: the click column maps back to a byte index. This is the
    /// inverse of `Prompt::cursor`, so the two must agree — a click on the column
    /// `cursor()` would render at has to return that same index.
    #[test]
    fn move_to_column_is_the_inverse_of_cursor_rendering() {
        // Prompt renders at x=1 (after the ":"), so column 1 is the line's start.
        let mut p = prompt_at("write foo", 1, 0);
        p.move_to_column(1);
        assert_eq!(p.cursor, 0);
        // Column 6 lands just before the space.
        p.move_to_column(6);
        assert_eq!(p.cursor, 5);
        // A click past the end clamps to the end rather than panicking.
        p.move_to_column(99);
        assert_eq!(p.cursor, "write foo".len());
        // A click left of the line area saturates to the start.
        p.move_to_column(0);
        assert_eq!(p.cursor, 0);
    }

    /// Wide graphemes advance two columns but one index each, so column
    /// arithmetic that assumed 1 cell per char would land mid-character and
    /// panic on the next byte-index slice of `line`.
    #[test]
    fn move_to_column_counts_display_width_not_chars() {
        let mut p = prompt_at("日本語", 0, 0);
        p.move_to_column(0);
        assert_eq!(p.cursor, 0);
        // Each CJK grapheme is 2 columns wide and 3 bytes long.
        p.move_to_column(2);
        assert_eq!(p.cursor, 3);
        p.move_to_column(4);
        assert_eq!(p.cursor, 6);
        assert!(p.line.is_char_boundary(p.cursor));
    }

    /// A horizontally scrolled line renders from `anchor`, so column→index must
    /// start counting there and not from byte 0.
    #[test]
    fn move_to_column_starts_from_the_render_anchor() {
        let mut p = prompt_at("abcdefghij", 0, 4);
        p.move_to_column(0);
        assert_eq!(p.cursor, 4); // the first *rendered* char is 'e'
        p.move_to_column(2);
        assert_eq!(p.cursor, 6);
    }

    #[test]
    fn wildmode_gives_each_press_its_action() {
        // vim's default: every press selects the next candidate.
        assert_eq!(wildmode_action("full", 0), WildAction::Full);
        assert_eq!(wildmode_action("full", 3), WildAction::Full);
        // `longest:full` — the first press completes the shared prefix, the next
        // (and every one after it: the last entry repeats) cycles.
        assert_eq!(wildmode_action("longest:full,full", 0), WildAction::Longest);
        assert_eq!(wildmode_action("longest:full,full", 1), WildAction::Full);
        assert_eq!(wildmode_action("longest:full,full", 9), WildAction::Full);
        // `list:longest` lists and completes the shared prefix, never selecting.
        assert_eq!(wildmode_action("list:longest", 0), WildAction::Longest);
        // `list` alone only shows the candidates.
        assert_eq!(wildmode_action("list", 0), WildAction::ListOnly);
        // An empty value completes the first match.
        assert_eq!(wildmode_action("", 0), WildAction::Full);
    }

    /// A prompt with no editor behind it — enough to exercise what the isearch
    /// toggles make of the typed line.
    fn test_prompt(isearch: bool) -> Prompt {
        let prompt = Prompt::new(
            "search:".into(),
            None,
            |_editor: &Editor, _input: &str| Vec::new(),
            |_cx: &mut Context, _input: &str, _event: PromptEvent| {},
        );
        if isearch {
            prompt.with_isearch(true)
        } else {
            prompt
        }
    }

    #[test]
    fn isearch_toggles_build_the_pattern_the_search_runs() {
        let mut prompt = test_prompt(true);
        prompt.line = "a.b".to_string();
        // zmax's `/` is a regexp search, so an untouched incremental search runs
        // exactly what was typed — the toggles below are the only thing that can
        // change that.
        assert_eq!(prompt.pattern(), "a.b");
        // `M-r` / `M-s r` (isearch-toggle-regexp): now a literal search, so the `.`
        // is quoted and matches a dot rather than any character.
        prompt.isearch.as_mut().unwrap().regexp = false;
        assert_eq!(prompt.pattern(), "a\\.b");
        // `M-c` / `M-s c` (isearch-toggle-case-fold): the search's case is no longer
        // the editor's smart-case guess but what the key says.
        prompt.isearch_case = Some(true);
        assert_eq!(prompt.pattern(), "(?i)a\\.b");
        prompt.isearch_case = Some(false);
        assert_eq!(prompt.pattern(), "(?-i)a\\.b");
        // Every other prompt (`:`, pickers, the other regex prompts) has no isearch
        // and is handed the line untouched.
        let mut plain = test_prompt(false);
        plain.line = "a.b".to_string();
        assert_eq!(plain.pattern(), "a.b");
    }

    #[test]
    fn isearch_yanks_are_quoted_into_a_regexp_search() {
        // `C-w`, `C-y`, `C-q`, `M-s C-e`: what they put into the search string is
        // text, not syntax — a regexp search must not read `a.b` as "a, anything, b".
        let mut prompt = test_prompt(true);
        assert_eq!(prompt.isearch_quote("a.b"), "a\\.b");
        // With regexp off the search string is quoted when the pattern is built
        // (`IsearchFlags::build_regex`), so quoting here too would double it.
        prompt.isearch.as_mut().unwrap().regexp = false;
        assert_eq!(prompt.isearch_quote("a.b"), "a.b");
        prompt.line = prompt.isearch_quote("a.b");
        assert_eq!(prompt.pattern(), "a\\.b");
    }

    /// Emacs `file-name-shadow-mode`: typing a second absolute name over the
    /// first makes everything in front of it irrelevant, and that is exactly the
    /// part the mode dims. The boundary comes out of `substitute-in-file-name`,
    /// so the two have to agree on what a name resolves to.
    #[test]
    fn file_name_shadow_marks_the_part_resolving_throws_away() {
        // `//` restarts the name at root; `/~` restarts it at a home directory.
        assert_eq!(substitute_in_file_name("/foo//bar"), "/bar");
        assert_eq!(substitute_in_file_name("/foo/~/bar"), "~/bar");
        // A `~` mid-component and a `$$` are literal text, not a restart.
        assert_eq!(substitute_in_file_name("/a~b"), "/a~b");
        assert_eq!(substitute_in_file_name("/a$$b"), "/a$b");
        // An unset variable expands to nothing, as it does in Emacs.
        assert_eq!(substitute_in_file_name("/a/${ZMAX_NOT_A_VAR}b"), "/a/b");

        let mut p = prompt_at("/foo//bar", 0, 0);
        // Off (as the mode is until it is turned on): nothing is dimmed.
        assert_eq!(p.shadow_end(), 0);
        assert!(file_name_shadow_mode());
        // The shadow covers "/foo/" — the largest prefix whose removal leaves
        // the name resolving to the same thing.
        assert_eq!(p.shadow_end(), 5);
        assert_eq!(&p.line[..p.shadow_end()], "/foo/");
        // Nothing to throw away: a plain name is all live input.
        p.line = "/foo/bar".to_string();
        assert_eq!(p.shadow_end(), 0);
        // A line that is not a path at all is left alone — zmax has no file-name
        // category on a prompt, so the shadow is keyed on the input.
        p.line = "s/a//b".to_string();
        assert_eq!(p.shadow_end(), 0);
        assert!(!file_name_shadow_mode());
    }

    /// An abbreviation is expanded in place: what follows the cursor stays put and
    /// the cursor ends up after the expansion, not at the end of the line. Byte
    /// arithmetic, so a multi-byte line has to survive it.
    #[test]
    fn abbrev_expansion_replaces_only_what_is_in_front_of_the_cursor() {
        // `:W arg` with the cursor after `W`: `W` becomes `write`, ` arg` stays.
        let mut p = prompt_at("W arg", 1, 0);
        p.cursor = 1;
        p.replace_before_cursor(1, "write");
        assert_eq!(p.line, "write arg");
        assert_eq!(p.cursor, 5);
        // The expansion may be shorter than the abbreviation, and either side may
        // be multi-byte — the cursor must still land on a character boundary.
        let mut p = prompt_at("日本語 x", 1, 0);
        p.cursor = "日本語".len();
        p.replace_before_cursor("日本語".len(), "ß");
        assert_eq!(p.line, "ß x");
        assert_eq!(p.cursor, "ß".len());
        assert!(p.line.is_char_boundary(p.cursor));
    }

    /// kakoune's `<c-r>` with Control on the register key quotes what it inserts
    /// the kakoune way (string_utils.hh:74-77): single quotes, with every single
    /// quote in the value doubled so the result is one argument.
    #[test]
    fn kakoune_quoting_doubles_the_quote() {
        assert_eq!(kak_quote("foo"), "'foo'");
        assert_eq!(kak_quote("it's"), "'it''s'");
        assert_eq!(kak_quote(""), "''");
        // A value that is already quoted is quoted again rather than left alone —
        // it is text, not syntax.
        assert_eq!(kak_quote("'a b'"), "'''a b'''");
    }

    /// vim 'cedit' names the key that opens the command-line window; its default
    /// is `CTRL-F`, so that is the chord the prompt has to recognise when nothing
    /// set the option.
    #[test]
    fn cedit_is_ctrl_f_until_the_option_says_otherwise() {
        assert!(cedit_pressed(ctrl!('f')));
        // The same letter without the modifier is a character of the command line.
        assert!(!cedit_pressed(key!('f')));
        assert!(!cedit_pressed(ctrl!('g')));
        // A key with no character of its own is never the cedit key.
        assert!(!cedit_pressed(key!(Enter)));
    }

    /// Which command-line window 'cedit' opens depends on which command line it
    /// was pressed on: the Ex history for `:`, the search history for a search —
    /// backwards when the search was started backwards — and none at all for a
    /// prompt that is not one of vim's command lines.
    #[test]
    fn cedit_opens_the_window_for_this_command_line() {
        assert_eq!(
            prompt_at("write", 1, 0).cmdline_window_command(),
            Some("cmdline_window")
        );
        let search = |forward| {
            Prompt::new(
                "search:".into(),
                Some('/'),
                |_: &Editor, _: &str| Vec::new(),
                |_: &mut Context, _: &str, _: PromptEvent| {},
            )
            .with_isearch(forward)
        };
        assert_eq!(
            search(true).cmdline_window_command(),
            Some("search_cmdline_window")
        );
        assert_eq!(
            search(false).cmdline_window_command(),
            Some("rsearch_cmdline_window")
        );
        // An `input()`-style read (`getcmdtype()` == `@`) has no history window.
        assert_eq!(test_prompt(false).cmdline_window_command(), None);
    }

    #[test]
    fn wildcharm_accepts_vims_spellings() {
        use zmax_view::keyboard::KeyModifiers;
        let ctrl_z = KeyEvent {
            code: KeyCode::Char('z'),
            modifiers: KeyModifiers::CONTROL,
        };
        // `<C-z>` (what a vimrc writes), `^Z` (what `:set wildcharm?` shows) and
        // 26 (what vim stores) are the same key.
        assert_eq!(parse_wildcharm("<C-z>"), Some(ctrl_z));
        assert_eq!(parse_wildcharm("^Z"), Some(ctrl_z));
        assert_eq!(parse_wildcharm("26"), Some(ctrl_z));
        // Tab, in all three spellings.
        assert_eq!(parse_wildcharm("<Tab>"), Some(key!(Tab)));
        assert_eq!(parse_wildcharm("^I"), Some(key!(Tab)));
        assert_eq!(parse_wildcharm("9"), Some(key!(Tab)));
        // Unset: no key completes from a mapping.
        assert_eq!(parse_wildcharm(""), None);
    }
}

#[cfg(test)]
mod icomplete_tests {
    use super::*;

    fn comps(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    /// No candidates is its own message, in the permissive brackets when the
    /// prompt takes any input and the parenthesised pair when it requires a
    /// match (icomplete.el:969-974).
    #[test]
    fn no_matches_reports_itself() {
        assert_eq!(icomplete_completions("zz", &[], false, 80), " [No matches]");
        assert_eq!(icomplete_completions("zz", &[], true, 80), " (No matches)");
    }

    /// A single candidate is "matched", with the part typing has not produced
    /// yet shown in brackets first.
    #[test]
    fn a_unique_candidate_is_matched() {
        assert_eq!(
            icomplete_completions("wri", &comps(&["write"]), false, 80),
            "[te] [Matched]"
        );
        // Typed in full: nothing left to add, so no bracket segment.
        assert_eq!(
            icomplete_completions("write", &comps(&["write"]), false, 80),
            " [Matched]"
        );
    }

    /// Several candidates are listed in braces, separated by `icomplete-separator`.
    #[test]
    fn several_candidates_are_listed_in_braces() {
        let out = icomplete_completions("q", &comps(&["quit", "quit!", "quiet"]), false, 80);
        assert!(out.ends_with('}'), "{out:?}");
        assert!(out.contains(" | "), "{out:?}");
    }

    /// `icomplete-hide-common-prefix` (icomplete.el:66): what every candidate
    /// shares is shown once, in the bracket segment, and stripped from each
    /// listed candidate — the list shows what distinguishes them.
    #[test]
    fn the_common_prefix_is_shown_once_not_repeated() {
        let out = icomplete_completions("wr", &comps(&["write", "write!", "write-all"]), false, 80);
        assert!(out.starts_with("[ite]"), "{out:?}");
        assert!(out.contains('{'), "{out:?}");
        // The shared "write" is not repeated inside the braces.
        let listed = &out[out.find('{').unwrap()..];
        assert!(!listed.contains("write"), "{listed:?}");
    }

    /// Typed text that is itself a candidate but not a unique one gets the
    /// empty bracket pair as its visual cue (icomplete.el:1047-1065) — without
    /// it, hiding the common prefix would leave the exact match invisible.
    #[test]
    fn an_exact_but_ambiguous_match_gets_a_cue() {
        let out = icomplete_completions("write", &comps(&["write", "write!"]), false, 80);
        assert!(out.starts_with("[]"), "{out:?}");
    }

    /// The prospects are cut to the room available, and the cut is marked with
    /// the separator and an ellipsis rather than silently truncated.
    #[test]
    fn a_narrow_minibuffer_truncates_with_an_ellipsis() {
        let many: Vec<String> = (0..60).map(|i| format!("candidate{i:02}")).collect();
        let out = icomplete_completions("c", &many, false, 40);
        assert!(out.ends_with("…}"), "{out:?}");
        // Wide enough, everything fits and there is no ellipsis marker.
        let out_wide = icomplete_completions("c", &comps(&["ca", "cb"]), false, 200);
        assert!(!out_wide.contains('…'), "{out_wide:?}");
    }

    /// The longest common prefix helper is Emacs's `try-completion ""`.
    #[test]
    fn common_prefix_is_the_longest_shared_head() {
        assert_eq!(common_prefix(&comps(&["write", "write!"])), "write");
        assert_eq!(common_prefix(&comps(&["abc", "abd"])), "ab");
        assert_eq!(common_prefix(&comps(&["x", "y"])), "");
        assert_eq!(common_prefix(&[]), "");
    }
}
