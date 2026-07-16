use crate::compositor::{Component, Compositor, Context, Event, EventResult};
use crate::{alt, ctrl, key, shift, ui};
use arc_swap::ArcSwap;
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
    unicode::segmentation::{GraphemeCursor, UnicodeSegmentation},
    unicode::width::UnicodeWidthStr,
    Position,
};
use zmax_view::{
    graphics::{CursorKind, Margin, Rect},
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
    /// Emacs `isearch-toggle-case-fold` (`M-c`, `M-s c`): forces case folding on
    /// or off for this search. `None` until the key is pressed, so an untouched
    /// search still uses the editor's smart-case setting.
    isearch_case: Option<bool>,
    /// Emacs isearch `M-s`: the prefix of the search-toggle map — the next key
    /// says which toggle (`M-s r`, `M-s c`, `M-s i`, `M-s o`, `M-s C-e`, …).
    pending_isearch_s: bool,
    /// Emacs minibuffer `C-x`: the prefix of `C-x UP` (complete from the history)
    /// and `C-x DOWN` (complete from the prompt's default).
    pending_ctrl_x: bool,
    /// Emacs `previous-matching-history-element` / `next-matching-history-element`:
    /// the regexp being searched for and the history entry it last put on the
    /// line. While the line still holds that entry the same regexp keeps
    /// searching (so the commands repeat); once the line is something else, that
    /// something else is taken as a new regexp.
    history_search: Option<(String, String)>,
}

/// The toggles an incremental search starts with. zmax's `/` is a regexp
/// search — Emacs's starts literal, and `M-r` toggles between the two either way
/// — and it leaves case and whitespace to the editor's own settings until a key
/// says otherwise.
const ISEARCH_START: IsearchFlags = IsearchFlags {
    regexp: true,
    word: false,
    symbol: false,
    case_fold: true,
    lax_whitespace: false,
    char_fold: false,
    invisible: false,
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

/// What an `isearch-yank-*` key grabs from the buffer at the end of the match.
#[derive(Debug, Clone, Copy)]
enum IsearchYank {
    Char,
    WordOrChar,
    Line,
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

fn is_word_sep(c: char) -> bool {
    c == std::path::MAIN_SEPARATOR || c.is_whitespace()
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
            pending_ctrl_backslash: false,
            pending_register: false,
            pending_literal: false,
            literal_code: None,
            masked: false,
            wild_press: 0,
            isearch: None,
            isearch_forward: true,
            isearch_case: None,
            pending_isearch_s: false,
            pending_ctrl_x: false,
            history_search: None,
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
        self.completion = (self.completion_fn)(editor, &self.line);
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

    pub fn insert_char(&mut self, c: char, cx: &Context) {
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

        // vim `c_<Insert>`: in overstrike mode a typed character replaces the one
        // under the cursor instead of pushing it right (except at end of line).
        if self.overstrike {
            let mut cursor = GraphemeCursor::new(self.cursor, self.line.len(), false);
            if let Ok(Some(end)) = cursor.next_boundary(&self.line, 0) {
                self.line.replace_range(self.cursor..end, "");
            }
        }
        self.line.insert(self.cursor, c);
        let mut cursor = GraphemeCursor::new(self.cursor, self.line.len(), false);
        if let Ok(Some(pos)) = cursor.next_boundary(&self.line, 0) {
            self.cursor = pos;
        }
        self.recalculate_completion(cx.editor);
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

    /// Emacs `previous-matching-history-element` (`M-r`) and
    /// `next-matching-history-element` (`M-s`): put on the line the next history
    /// entry — older when `backward`, newer when not — that matches a regexp.
    ///
    /// Emacs reads the regexp in a recursive minibuffer; here the line itself is
    /// the regexp (as in `comint-history-isearch-backward-regexp`), and it keeps
    /// being the regexp for as long as the command is repeated, so `M-r M-r`
    /// walks back through the matches. `Err` is a bad regexp; `Ok(false)` means
    /// no (further) history entry matches.
    pub fn matching_history_element(
        &mut self,
        editor: &Editor,
        backward: bool,
    ) -> Result<bool, String> {
        let Some(register) = self.history_register else {
            return Err("this prompt keeps no history".to_string());
        };
        let pattern = match &self.history_search {
            Some((pat, applied)) if *applied == self.line => pat.clone(),
            _ => self.line.clone(),
        };
        if pattern.is_empty() {
            return Err("no regexp — type one first".to_string());
        }
        let re = regex::Regex::new(&pattern).map_err(|e| format!("bad regexp: {e}"))?;
        // Oldest first, the order `change_history` indexes history in.
        let entries: Vec<String> = match editor.registers.read(register, editor) {
            Some(values) => values.map(|v| v.to_string()).rev().collect(),
            None => Vec::new(),
        };
        if entries.is_empty() {
            return Ok(false);
        }
        // Searching starts from where history navigation currently sits: past the
        // newest entry when nothing has been recalled yet.
        let from = self.history_pos.unwrap_or(entries.len());
        let found = if backward {
            (0..from.min(entries.len()))
                .rev()
                .find(|&i| re.is_match(&entries[i]))
        } else {
            (from + 1..entries.len()).find(|&i| re.is_match(&entries[i]))
        };
        let Some(index) = found else {
            return Ok(false);
        };
        self.line = entries[index].clone();
        self.history_pos = Some(index);
        self.history_search = Some((pattern, self.line.clone()));
        self.move_end();
        self.exit_selection();
        Ok(true)
    }

    /// Accept the line — store it in the history register and fire the
    /// `Validate` callback. This is exactly what `Enter` does; `false` means the
    /// prompt must stay open (a directory completion was selected, and the
    /// candidate list was refreshed for the next component instead).
    pub fn submit(&mut self, cx: &mut Context) -> bool {
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
        (self.callback_fn)(cx, &input, PromptEvent::Validate);
        true
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
                        self.insert_char(c, cx);
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
            self.insert_char(c, cx);
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
        let pattern = self.pattern();
        (self.callback_fn)(cx, &pattern, PromptEvent::Update);
        self.isearch_reveal(cx);
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
    /// by what was added, which is what every `isearch-yank-*` key does.
    fn isearch_add(&mut self, cx: &mut Context, text: &str) {
        if text.is_empty() {
            return;
        }
        let quoted = self.isearch_quote(text);
        self.move_end();
        self.insert_str(&quoted, cx.editor);
        self.fire_update(cx);
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

    /// Emacs `previous-matching-history-element` (`M-r`) / `next-matching-history-element`
    /// (`M-s`): step back / on to a history entry matching what is typed, read as a
    /// regexp. Emacs reads that regexp in a recursive minibuffer; here the line
    /// already holds it, as it does in a shell's history search.
    fn matching_history(&mut self, cx: &mut Context, older: bool) {
        let Some(register) = self.history_register else {
            return;
        };
        let regex = match regex::Regex::new(&self.line) {
            Ok(regex) => regex,
            Err(_) => {
                cx.editor
                    .set_error(format!("Invalid regexp: {}", self.line));
                return;
            }
        };
        // `change_history` counts the history from its oldest entry, and the ring
        // reads most-recent first, so it is reversed to share that index.
        let mut entries: Vec<String> = match cx.editor.registers.read(register, cx.editor) {
            Some(values) => values.map(|value| value.to_string()).collect(),
            None => return,
        };
        entries.reverse();
        let start = self.history_pos.unwrap_or(entries.len());
        let found = if older {
            (0..start.min(entries.len()))
                .rev()
                .find(|&i| regex.is_match(&entries[i]))
        } else {
            (start + 1..entries.len()).find(|&i| regex.is_match(&entries[i]))
        };
        let Some(index) = found else {
            cx.editor.set_error("No matching history element");
            return;
        };
        (self.callback_fn)(cx, &self.line, PromptEvent::Abort);
        self.line = entries[index].clone();
        self.history_pos = Some(index);
        self.move_end();
        self.fire_update(cx);
        self.recalculate_completion(cx.editor);
    }

    /// Emacs `minibuffer-complete-defaults` (`C-x DOWN`): offer the prompt's
    /// default — the value it runs when the line is empty — as the completion.
    fn complete_from_default(&mut self, editor: &Editor) -> bool {
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
        // zmax's multi-column list.
        let cols = if wildoptions_pum() {
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

        if completion_area.height > 0 && !self.completion.is_empty() {
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
        // render buffer text
        surface.set_string(area.x, area.y + line, &self.prompt, prompt_color);

        self.line_area = area
            .clip_left(self.prompt.len() as u16)
            .clip_top(line)
            .clip_right(2);

        if self.line.is_empty() {
            self.anchor = 0;
            // Show the most recently entered value as a suggestion.
            if let Some(suggestion) = self.first_history_completion(cx.editor) {
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

            surface.set_string_anchored(
                self.line_area.x,
                self.line_area.y,
                self.truncate_start,
                self.truncate_end,
                &self.line.as_str()[self.anchor..],
                line_width,
                |_| prompt_color,
            );
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
        // `CTRL-\` only means something together with the key that follows it.
        let ctrl_backslash = std::mem::take(&mut self.pending_ctrl_backslash);
        // Emacs isearch `M-s` and minibuffer `C-x`: both are prefixes — the key
        // that follows says what they do.
        let isearch_s = std::mem::take(&mut self.pending_isearch_s);
        let ctrl_x = std::mem::take(&mut self.pending_ctrl_x);

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

        match event {
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
            // of evaluating an expression. vim opens a nested `=` prompt for the
            // expression; zmax evaluates the line already typed — which is the
            // text you would have to retype into that prompt anyway.
            key!('e') if ctrl_backslash => {
                let expr = self.line.clone();
                match crate::commands::typed::cmdline_eval_expr(cx, &expr) {
                    Ok(result) => self.set_line(result, cx.editor),
                    Err(e) => cx.editor.set_error(format!("CTRL-\\ e: {e}")),
                }
            }
            ctrl!('\\') => self.pending_ctrl_backslash = true,
            // vim `c_CTRL-R_CTRL-R` / `_CTRL-O` / `_CTRL-P {regname}`: insert the
            // register literally / without indent changes. The insert below is
            // already literal, so these just wait for the register name.
            ctrl!('r') | ctrl!('o') | ctrl!('p') if self.pending_register => {}

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
            ctrl!('x') => {
                self.pending_ctrl_x = true;
            }

            // ── Emacs isearch: the keys inside an incremental search ─────────
            // `isearch-repeat-forward` / `-backward`: on to the next match — or,
            // with nothing typed yet, back to the previous search string.
            ctrl!('s') if isearch_ctl => self.isearch_repeat(cx, true),
            ctrl!('r') if isearch_ctl => self.isearch_repeat(cx, false),
            // `isearch-yank-word-or-char`: grow the search by the buffer text the
            // match is sitting in front of.
            ctrl!('w') if isearch_ctl => self.isearch_yank(cx, IsearchYank::WordOrChar),
            // `isearch-yank-kill`: grow the search by the most recent kill.
            ctrl!('y') if isearch_ctl => match crate::emacs_kill::top() {
                Some(kill) => self.isearch_add(cx, &kill),
                None => cx.editor.set_error("Kill ring is empty"),
            },
            // `isearch-quote-char`: the next character goes into the search string
            // as itself, quoted so a regexp search cannot read it as an operator.
            ctrl!('q') if isearch_ctl => {
                self.next_char_handler = Some(Box::new(|prompt, c, cx| {
                    let quoted = prompt.isearch_quote(&c.to_string());
                    prompt.insert_str(&quoted, cx.editor);
                }));
            }
            // `isearch-abort`: leave the search and go back to where it started.
            ctrl!('g') if isearch_ctl => {
                (self.callback_fn)(cx, &self.line, PromptEvent::Abort);
                return close_fn;
            }
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
                    self.matching_history(cx, true);
                }
            }
            // `isearch-complete`: complete the search string from the search ring.
            alt!(Tab) if isearch => self.isearch_complete(cx),
            // Emacs isearch `M-s`: the prefix of the toggle map above. Outside a
            // search it is the minibuffer's `next-matching-history-element`.
            alt!('s') => {
                self.pending_isearch_s = true;
                if !isearch {
                    self.pending_isearch_s = false;
                    self.matching_history(cx, false);
                }
            }

            // vim `c_CTRL-V` (and `c_CTRL-Q`): take the next key literally. `C-q`
            // only in the vim presets — in the Emacs ones it must not shadow the
            // completion-selection exit further down.
            ctrl!('v') => self.pending_literal = true,
            ctrl!('q') if cx.editor.vim_semantics => self.pending_literal = true,
            // vim `c_CTRL-]`: expand the `:cabbrev` in front of the cursor.
            ctrl!(']') => {
                let before = self.line[..self.cursor].to_string();
                if let Some((lhs, rhs)) = crate::commands::typed::cmdline_abbrev_expand(&before) {
                    let start = self.cursor - lhs.len();
                    let mut line = self.line.clone();
                    line.replace_range(start..self.cursor, &rhs);
                    self.set_line(line, cx.editor);
                }
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
            alt!('b') | ctrl!(Left) | shift!(Left) => self.move_cursor(Movement::BackwardWord(1)),
            alt!('f') | ctrl!(Right) | shift!(Right) => self.move_cursor(Movement::ForwardWord(1)),
            ctrl!('b') | key!(Left) => self.move_cursor(Movement::BackwardChar(1)),
            ctrl!('f') | key!(Right) => self.move_cursor(Movement::ForwardChar(1)),
            ctrl!('e') | key!(End) => self.move_end(),
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
                self.delete_char_backwards(cx.editor);
                self.fire_update(cx);
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
            // Emacs `isearch-exit` (`RET`) / `minibuffer-complete-and-exit`: take
            // what is typed — the search stops on the match it is showing.
            key!(Enter) | ctrl!('j') => {
                if self.submit(cx) {
                    return close_fn;
                }
            }
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
            .clip_left(self.prompt.len() as u16)
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

    /// vim `c_<LeftMouse>`: the click column maps back to a byte index. This is the
    /// inverse of `Prompt::cursor`, so the two must agree — a click on the column
    /// `cursor()` would render at has to return that same index.
    #[test]
    fn move_to_column_is_the_inverse_of_cursor_rendering() {
        // Prompt renders at x=1 (after the ":"), so column 1 is the line's start.
        let mut p = prompt_at("write foo", 1, 0);
        p.move_to_column(1);
        assert_eq!(p.cursor, 0);
        p.move_to_column(6);
        assert_eq!(p.cursor, 5); // just before the space
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
