use crate::{
    annotations::diagnostics::{DiagnosticFilter, InlineDiagnosticsConfig},
    clipboard::ClipboardProvider,
    document::{
        DocumentOpenError, DocumentSavedEventFuture, DocumentSavedEventResult, Mode, SavePoint,
    },
    events::{DocumentDidClose, DocumentDidOpen, DocumentFocusLost},
    graphics::{CursorKind, Rect},
    handlers::Handlers,
    info::Info,
    input::KeyEvent,
    register::Registers,
    theme::{self, Theme},
    tree::{self, Tree},
    Document, DocumentId, View, ViewId,
};
use zemacs_event::dispatch;
use zemacs_loader::workspace_trust::{ImplicitTrustLevel, TrustQuery, WorkspaceTrust};
use zemacs_vcs::DiffProviderRegistry;

use futures_util::stream::select_all::SelectAll;
use futures_util::StreamExt;
use tokio_stream::wrappers::UnboundedReceiverStream;
use zemacs_lsp::{Call, LanguageServerId};

use std::{
    borrow::Cow,
    cell::Cell,
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    fs,
    io::{self, stdin},
    num::{NonZeroU8, NonZeroUsize},
    path::{Path, PathBuf},
    pin::Pin,
    sync::Arc,
};

use tokio::{
    sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender},
    time::{sleep, Duration, Instant, Sleep},
};

use anyhow::{anyhow, bail, Error};

pub use zemacs_core::diagnostic::Severity;
use zemacs_core::{
    auto_pairs::AutoPairs,
    diagnostic::DiagnosticProvider,
    syntax::{
        self,
        config::{AutoPairConfig, IndentationHeuristic, LanguageServerFeature, SoftWrap},
    },
    Change, LineEnding, Position, Range, Selection, Uri, NATIVE_LINE_ENDING,
};
use zemacs_dap::{self as dap, registry::DebugAdapterId};
use zemacs_lsp::lsp;
use zemacs_stdx::path::canonicalize;

use serde::{ser::SerializeMap, Deserialize, Deserializer, Serialize, Serializer};

use arc_swap::{
    access::{DynAccess, DynGuard},
    ArcSwap,
};

pub const DIR_STACK_CAP: usize = 10;
pub const DEFAULT_AUTO_SAVE_DELAY: u64 = 3000;

fn deserialize_duration_millis<'de, D>(deserializer: D) -> Result<Duration, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let millis = u64::deserialize(deserializer)?;
    Ok(Duration::from_millis(millis))
}

fn serialize_duration_millis<S>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_u64(
        duration
            .as_millis()
            .try_into()
            .map_err(|_| serde::ser::Error::custom("duration value overflowed u64"))?,
    )
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", default, deny_unknown_fields)]
pub struct GutterConfig {
    /// Gutter Layout
    pub layout: Vec<GutterType>,
    /// Options specific to the "line-numbers" gutter
    pub line_numbers: GutterLineNumbersConfig,
}

impl Default for GutterConfig {
    fn default() -> Self {
        Self {
            layout: vec![
                GutterType::Blame,
                GutterType::Diagnostics,
                GutterType::Marks,
                GutterType::Signs,
                GutterType::Spacer,
                GutterType::LineNumbers,
                GutterType::Spacer,
                GutterType::Diff,
            ],
            line_numbers: GutterLineNumbersConfig::default(),
        }
    }
}

impl From<Vec<GutterType>> for GutterConfig {
    fn from(x: Vec<GutterType>) -> Self {
        GutterConfig {
            layout: x,
            ..Default::default()
        }
    }
}

fn deserialize_gutter_seq_or_struct<'de, D>(deserializer: D) -> Result<GutterConfig, D::Error>
where
    D: Deserializer<'de>,
{
    struct GutterVisitor;

    impl<'de> serde::de::Visitor<'de> for GutterVisitor {
        type Value = GutterConfig;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            write!(
                formatter,
                "an array of gutter names or a detailed gutter configuration"
            )
        }

        fn visit_seq<S>(self, mut seq: S) -> Result<Self::Value, S::Error>
        where
            S: serde::de::SeqAccess<'de>,
        {
            let mut gutters = Vec::new();
            while let Some(gutter) = seq.next_element::<String>()? {
                gutters.push(
                    gutter
                        .parse::<GutterType>()
                        .map_err(serde::de::Error::custom)?,
                )
            }

            Ok(gutters.into())
        }

        fn visit_map<M>(self, map: M) -> Result<Self::Value, M::Error>
        where
            M: serde::de::MapAccess<'de>,
        {
            let deserializer = serde::de::value::MapAccessDeserializer::new(map);
            Deserialize::deserialize(deserializer)
        }
    }

    deserializer.deserialize_any(GutterVisitor)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", default, deny_unknown_fields)]
pub struct GutterLineNumbersConfig {
    /// Minimum number of characters to use for line number gutter. Defaults to 3.
    pub min_width: usize,
}

impl Default for GutterLineNumbersConfig {
    fn default() -> Self {
        Self { min_width: 3 }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", default, deny_unknown_fields)]
pub struct FilePickerConfig {
    /// IgnoreOptions
    /// Enables ignoring hidden files.
    /// Whether to hide hidden files in file picker and global search results. Defaults to true.
    pub hidden: bool,
    /// Enables following symlinks.
    /// Whether to follow symbolic links in file picker and file or directory completions. Defaults to true.
    pub follow_symlinks: bool,
    /// Hides symlinks that point into the current directory. Defaults to true.
    pub deduplicate_links: bool,
    /// Enables reading ignore files from parent directories. Defaults to true.
    pub parents: bool,
    /// Enables reading `.ignore` files.
    /// Whether to hide files listed in .ignore in file picker and global search results. Defaults to true.
    pub ignore: bool,
    /// Enables reading `.gitignore` files.
    /// Whether to hide files listed in .gitignore in file picker and global search results. Defaults to true.
    pub git_ignore: bool,
    /// Enables reading global .gitignore, whose path is specified in git's config: `core.excludefile` option.
    /// Whether to hide files listed in global .gitignore in file picker and global search results. Defaults to true.
    pub git_global: bool,
    /// Enables reading `.git/info/exclude` files.
    /// Whether to hide files listed in .git/info/exclude in file picker and global search results. Defaults to true.
    pub git_exclude: bool,
    /// WalkBuilder options
    /// Maximum Depth to recurse directories in file picker and global search. Defaults to `None`.
    pub max_depth: Option<usize>,
}

impl Default for FilePickerConfig {
    fn default() -> Self {
        Self {
            // Show dotfiles in the picker by default (`hidden` here means
            // "skip hidden files", so false = include them). gitignore and the
            // .git/.hg/… VCS filter still apply, so no repo internals leak in.
            hidden: false,
            follow_symlinks: true,
            deduplicate_links: true,
            parents: true,
            ignore: true,
            git_ignore: true,
            git_global: true,
            git_exclude: true,
            max_depth: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", default, deny_unknown_fields)]
pub struct FileExplorerConfig {
    /// IgnoreOptions
    /// Enables ignoring hidden files.
    /// Whether to hide hidden files in file explorer and global search results. Defaults to false.
    pub hidden: bool,
    /// Enables following symlinks.
    /// Whether to follow symbolic links in file picker and file or directory completions. Defaults to false.
    pub follow_symlinks: bool,
    /// Enables reading ignore files from parent directories. Defaults to false.
    pub parents: bool,
    /// Enables reading `.ignore` files.
    /// Whether to hide files listed in .ignore in file picker and global search results. Defaults to false.
    pub ignore: bool,
    /// Enables reading `.gitignore` files.
    /// Whether to hide files listed in .gitignore in file picker and global search results. Defaults to false.
    pub git_ignore: bool,
    /// Enables reading global .gitignore, whose path is specified in git's config: `core.excludefile` option.
    /// Whether to hide files listed in global .gitignore in file picker and global search results. Defaults to false.
    pub git_global: bool,
    /// Enables reading `.git/info/exclude` files.
    /// Whether to hide files listed in .git/info/exclude in file picker and global search results. Defaults to false.
    pub git_exclude: bool,
    /// Whether to flatten single-child directories in file explorer. Defaults to true.
    pub flatten_dirs: bool,
}

impl Default for FileExplorerConfig {
    fn default() -> Self {
        Self {
            hidden: false,
            follow_symlinks: false,
            parents: false,
            ignore: false,
            git_ignore: false,
            git_global: false,
            git_exclude: false,
            flatten_dirs: true,
        }
    }
}

fn serialize_alphabet<S>(alphabet: &[char], serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let alphabet: String = alphabet.iter().collect();
    serializer.serialize_str(&alphabet)
}

fn deserialize_alphabet<'de, D>(deserializer: D) -> Result<Vec<char>, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::Error;

    let str = String::deserialize(deserializer)?;
    let chars: Vec<_> = str.chars().collect();
    let unique_chars: HashSet<_> = chars.iter().copied().collect();
    if unique_chars.len() != chars.len() {
        return Err(<D::Error as Error>::custom(
            "jump-label-alphabet must contain unique characters",
        ));
    }
    Ok(chars)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", default, deny_unknown_fields)]
pub struct Config {
    /// Padding to keep between the edge of the screen and the cursor when
    /// scrolling. Defaults to 0 (vim/spacemacs: `H`/`L` reach the literal top/
    /// bottom row). Set to e.g. 5 for Helix-style scroll margins.
    pub scrolloff: usize,
    /// Number of lines to scroll at once. Defaults to 3
    pub scroll_lines: isize,
    /// Mouse support. Defaults to true.
    pub mouse: bool,
    /// Which register to use for mouse yank.
    pub mouse_yank_register: char,
    /// Shell to use for shell commands. Defaults to ["cmd", "/C"] on Windows and ["sh", "-c"] otherwise.
    pub shell: Vec<String>,
    /// Line number mode.
    pub line_number: LineNumber,
    /// Highlight the lines cursors are currently on. Defaults to false.
    pub cursorline: bool,
    /// Highlight the columns cursors are currently on. Defaults to false.
    pub cursorcolumn: bool,
    /// Highlight every occurrence of the word under the primary cursor in the
    /// visible viewport (like vim-illuminate / JetBrains identifier-under-caret).
    /// Defaults to true.
    pub highlight_word_under_cursor: bool,
    /// Highlight all matches of the last search pattern (vim `hlsearch`).
    /// Defaults to `false`.
    pub search_highlight: bool,
    #[serde(deserialize_with = "deserialize_gutter_seq_or_struct")]
    pub gutters: GutterConfig,
    /// Middle click paste support. Defaults to true.
    pub middle_click_paste: bool,
    /// Automatic insertion of pairs to parentheses, brackets,
    /// etc. Optionally, this can be a list of 2-tuples to specify a
    /// global list of characters to pair. Defaults to true.
    pub auto_pairs: AutoPairConfig,
    /// Automatic auto-completion, automatically pop up without user trigger. Defaults to true.
    pub auto_completion: bool,
    /// Enable filepath completion.
    /// Show files and directories if an existing path at the cursor was recognized,
    /// either absolute or relative to the current opened document or current working directory (if the buffer is not yet saved).
    /// Defaults to true.
    pub path_completion: bool,
    /// Configures completion of words from open buffers.
    /// Defaults to enabled with a trigger length of 7.
    pub word_completion: WordCompletion,
    /// Automatic formatting on save. Defaults to true.
    pub auto_format: bool,
    /// Default register used for yank/paste. Defaults to '"'
    pub default_yank_register: char,
    /// Automatic save on focus lost and/or after delay.
    /// Time delay in milliseconds since last edit after which auto save timer triggers.
    /// Time delay defaults to false with 3000ms delay. Focus lost defaults to false.
    #[serde(deserialize_with = "deserialize_auto_save")]
    pub auto_save: AutoSave,
    /// Reload a buffer automatically when its file changes on disk outside the
    /// editor (vim `autoread`). Only reloads buffers with no unsaved edits; if
    /// both the buffer and the file changed, the buffer is kept and a warning is
    /// shown. Defaults to true.
    pub auto_reload: bool,
    /// Set a global text_width
    pub text_width: usize,
    /// Time in milliseconds since last keypress before idle timers trigger.
    /// Used for various UI timeouts. Defaults to 250ms.
    #[serde(
        serialize_with = "serialize_duration_millis",
        deserialize_with = "deserialize_duration_millis"
    )]
    pub idle_timeout: Duration,
    /// Time in milliseconds after typing a word character before auto completions
    /// are shown, set to 5 for instant. Defaults to 250ms.
    #[serde(
        serialize_with = "serialize_duration_millis",
        deserialize_with = "deserialize_duration_millis"
    )]
    pub completion_timeout: Duration,
    /// Whether to insert the completion suggestion on hover. Defaults to true.
    pub preview_completion_insert: bool,
    pub completion_trigger_len: u8,
    /// Whether to instruct the LSP to replace the entire word when applying a completion
    /// or to only insert new text
    pub completion_replace: bool,
    /// `true` if zemacs should automatically add a line comment token if you're currently in a comment
    /// and press `enter`.
    pub continue_comments: bool,
    /// Whether to display infoboxes. Defaults to true.
    pub auto_info: bool,
    /// Keymap prefix keys whose auto-info (which-key) popup is suppressed, matched against the
    /// first key of the pending sequence (e.g. "g", "y", "z", "space"). Silences individual
    /// prefix menus while leaving `auto-info` on for the rest. Defaults to ["g", "y", "z"];
    /// set to [] to show every popup, or add "space" to also silence the leader menu.
    /// Note: only consulted when `auto-info-leader-only` is false.
    pub auto_info_exclude: Vec<String>,
    /// When true, only the deliberate leader prefixes get a which-key popup — the
    /// `space` leader and the emacs/spacemacs `C-x` prefix; popups for every other
    /// prefix (c, d, g, z, >, ci, di, ca, da, C-w, ...) are suppressed because they
    /// are too distracting. Set to false to show which-key for all prefixes (subject
    /// to `auto-info-exclude`). Defaults to true.
    pub auto_info_leader_only: bool,
    /// zemacs *global* which-key: when true, every pending key sequence pops
    /// a which-key infobox. When false (the default), only the
    /// deliberate global prefixes — the `space` leader and the emacs/spacemacs
    /// `C-x`/`C-c`/`C-h` prefixes — get a popup; operator + text-object prefixes
    /// (`ci`/`ca`, `di`/`da`, `g`, `y`, `z`, `>`, `C-w`, …) stay quiet. Defaults
    /// to false.
    pub which_key_global: bool,
    /// Master switch for the which-key prefix popups (the `space`/`C-x`/`C-c`/`C-h`
    /// and per-prefix menus). When false, NO which-key popup is ever shown,
    /// regardless of `which_key_global` / `auto_info_leader_only` / `auto_info_exclude`
    /// — other autoinfo (mark/register/help prompts) still works. Defaults to true.
    pub which_key: bool,
    /// External-`fzf` integration (fzf.vim-style `:Files`/`:Colors`/… commands):
    /// popup size/layout options, and the preview-pane command.
    pub fzf: FzfConfig,
    /// When true, vim-sneak overrides `s`/`S` (jump to a two-character sequence). When false,
    /// `s`/`S` keep vim's substitute-char / substitute-line. Defaults to true.
    pub vim_sneak: bool,
    /// When true, zemacs sources the user's personal Vim configuration
    /// (`~/.vimrc`, `~/.vim/vimrc`, `~/.config/nvim/init.vim`) at startup,
    /// honouring its `:set`/`:map`/`:colorscheme`. **Defaults to false** — zemacs
    /// is not Vim and does not read your personal Vim config unless you opt in.
    /// (zemacs's own `init.vim` in the config dir is always sourced regardless.)
    pub source_vimrc: bool,
    /// When true, zemacs sources the user's personal Emacs configuration
    /// (`~/.emacs.d/init.el`, `~/.config/emacs/init.el`, `~/.emacs`) at startup,
    /// running its Emacs Lisp. **Defaults to false** — zemacs is not Emacs and
    /// does not run your personal init.el unless you opt in. (zemacs's own
    /// `init.el` in the config dir is always sourced regardless.)
    pub source_emacs_config: bool,
    /// Path to an arbitrary Emacs Lisp file to source at startup (`~` and env
    /// vars are expanded). **Defaults to none** (nothing sourced). When both this
    /// and `source-viml-file` are set, the Emacs Lisp file is sourced FIRST.
    pub source_elisp_file: Option<String>,
    /// Path to an arbitrary Vimscript file to source at startup (`~` and env vars
    /// are expanded). **Defaults to none** (nothing sourced). Sourced AFTER
    /// `source-elisp-file` when both are set.
    pub source_viml_file: Option<String>,
    pub file_picker: FilePickerConfig,
    pub file_explorer: FileExplorerConfig,
    /// Configuration of the statusline elements
    pub statusline: StatusLineConfig,
    /// Render the per-window status line. `false` hides it (vim `laststatus=0`).
    /// Defaults to `true`.
    pub render_statusline: bool,
    /// Shape for cursor in each mode
    pub cursor_shape: CursorShapeConfig,
    /// Set to `true` to override automatic detection of terminal truecolor support in the event of a false negative. Defaults to `false`.
    pub true_color: bool,
    /// Set to `true` to make the editor background transparent by not painting
    /// the `ui.background` fill, so the terminal's own background (e.g. a
    /// translucent/blurred window) shows through. Defaults to `false`.
    pub transparent_background: bool,
    /// Set to `true` to override automatic detection of terminal undercurl support in the event of a false negative. Defaults to `false`.
    pub undercurl: bool,
    /// Search configuration.
    #[serde(default)]
    pub search: SearchConfig,
    pub lsp: LspConfig,
    pub terminal: Option<TerminalConfig>,
    /// Column numbers at which to draw the rulers. Defaults to `[]`, meaning no rulers.
    pub rulers: Vec<u16>,
    #[serde(default)]
    pub whitespace: WhitespaceConfig,
    /// Persistently display open buffers along the top
    pub bufferline: BufferLine,
    /// Place a new vertical split to the right of the current window (vim
    /// `splitright`). Defaults to `true`.
    pub split_right: bool,
    /// Place a new horizontal split below the current window (vim `splitbelow`).
    /// Defaults to `true`.
    pub split_below: bool,
    /// Vertical indent width guides.
    pub indent_guides: IndentGuidesConfig,
    /// Whether to color modes with different colors. Defaults to `false`.
    pub color_modes: bool,
    pub soft_wrap: SoftWrap,
    /// Workspace specific lsp ceiling dirs
    pub workspace_lsp_roots: Vec<PathBuf>,
    /// Which line ending to choose for new documents. Defaults to `native`. i.e. `crlf` on Windows, otherwise `lf`.
    pub default_line_ending: LineEndingConfig,
    /// Whether to automatically insert a trailing line-ending on write if missing. Defaults to `true`.
    pub insert_final_newline: bool,
    /// Whether to use atomic operations to write documents to disk.
    /// This prevents data loss if the editor is interrupted while writing the file, but may
    /// confuse some file watching/hot reloading programs. Defaults to `true`.
    pub atomic_save: bool,
    /// vim `backup`: keep a persistent backup copy of a file's previous contents
    /// (named `<file><backup_ext>`) before overwriting it on write. Defaults to
    /// `false` (vim's default; `writebackup` makes a transient one).
    pub backup: bool,
    /// vim `backupext`: the suffix appended to the backup file name. Defaults to
    /// `~`.
    pub backup_ext: String,
    /// vim `backupdir`: comma-separated directories that host the backup copy.
    /// The first non-empty entry is used; empty keeps the backup beside the file.
    pub backup_dir: String,
    /// vim `backupskip`: comma-separated glob patterns (`*`/`?`); a file whose
    /// path matches any pattern is written without a backup (e.g. `/tmp/*`).
    pub backup_skip: String,
    /// vim `swapfile`: keep a recovery swap file of unsaved changes; warn when a
    /// swap file already exists on open. Defaults to false.
    pub swapfile: bool,
    /// vim `directory`: directory for swap files. Empty keeps them beside the
    /// edited file (as a dotfile).
    pub swap_directory: String,
    /// vim `undofile`: persist the undo history to disk on write and reload it on
    /// open, so undo survives closing the file. Defaults to false.
    pub undofile: bool,
    /// vim `undodir`: directory for `undofile` history files. Empty uses
    /// `~/.zemacs/undo`.
    pub undo_dir: String,
    /// vim `title`: set the terminal window title to the current file. Defaults
    /// to false.
    pub title: bool,
    /// vim `titlestring`: the title text; empty uses `<file> - zemacs`. `%f`/`%t`
    /// expand to the file path / name. Defaults to empty.
    pub title_string: String,
    /// vim `conceallevel`: how concealed syntax markers are rendered. 0 shows
    /// them normally; >= 1 hides them. Defaults to 0.
    pub conceallevel: usize,
    /// vim `concealcursor`: modes (`n`/`v`/`i`/`c`) in which the cursor's line
    /// stays concealed. Defaults to empty (reveal on the cursor line).
    pub concealcursor: String,
    /// Whether to automatically remove all trailing line-endings after the final one on write.
    /// Defaults to `false`.
    pub trim_final_newlines: bool,
    /// Whether to automatically remove all whitespace characters preceding line-endings on write.
    /// Defaults to `false`.
    pub trim_trailing_whitespace: bool,
    /// Enables smart tab
    pub smart_tab: Option<SmartTabConfig>,
    /// Draw border around popups.
    pub popup_border: PopupBorderConfig,
    /// Which indent heuristic to use when a new line is inserted
    #[serde(default)]
    pub indent_heuristic: IndentationHeuristic,
    /// labels characters used in jumpmode
    #[serde(
        serialize_with = "serialize_alphabet",
        deserialize_with = "deserialize_alphabet"
    )]
    pub jump_label_alphabet: Vec<char>,
    /// Display diagnostic below the line they occur.
    pub inline_diagnostics: InlineDiagnosticsConfig,
    pub end_of_line_diagnostics: DiagnosticFilter,
    // Set to override the default clipboard provider
    pub clipboard_provider: ClipboardProvider,
    /// Whether to read settings from [EditorConfig](https://editorconfig.org) files. Defaults to
    /// `true`.
    pub editor_config: bool,
    /// Whether to render rainbow colors for matching brackets. Defaults to `false`.
    pub rainbow_brackets: bool,
    /// Whether to enable Kitty Keyboard Protocol
    pub kitty_keyboard_protocol: KittyKeyboardProtocolConfig,
    pub buffer_picker: BufferPickerConfig,
    /// Workspace-trust configuration.
    pub workspace_trust: WorkspaceTrustConfig,
    /// What to open on a no-args launch: `startify` (default), `recent` (the
    /// most-recently-used file), `session` (restore the previous session's
    /// tabs), or `file` (open `startup-file`).
    pub startup: StartupScreen,
    /// File opened on launch when `startup = "file"`. Ignored otherwise.
    pub startup_file: String,
    /// Follow the zwire terminal host's active colorscheme. When true, zemacs
    /// reads `~/.zwire/global.toml` (`[theme] scheme` + `[theme.ui] light`) and
    /// applies the matching `zgui-<scheme>` / `zgui-<scheme>-light` theme at
    /// startup, on `:config-reload`, and live while idle — overriding the
    /// configured/persisted theme. Defaults to `false`.
    pub sync_zwire_theme: bool,
}

/// User-facing configuration for `[editor.workspace-trust]`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(default, rename_all = "kebab-case", deny_unknown_fields)]
pub struct WorkspaceTrustConfig {
    /// What to trust implicitly without an explicit grant. See [`ImplicitTrustLevelConfig`].
    pub level: ImplicitTrustLevelConfig,
    /// Whether opening a file in an untrusted workspace surfaces the trust modal. The statusline
    /// `[⚠]` indicator is always shown either way; disabling the prompt is for users who would
    /// rather act explicitly via `:workspace-trust` than be interrupted. Defaults to `true`.
    pub prompt: bool,
    /// Glob patterns whose matching workspaces are implicitly trusted.
    pub trusted: Vec<String>,
}

impl Default for WorkspaceTrustConfig {
    fn default() -> Self {
        Self {
            level: ImplicitTrustLevelConfig::default(),
            prompt: true,
            trusted: Vec::new(),
        }
    }
}

#[derive(Debug, Default, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize, Clone, Copy)]
#[serde(rename_all = "kebab-case")]
pub enum ImplicitTrustLevelConfig {
    /// Don't trust anything implicitly — prompt for every new workspace.
    None,
    /// Trust Zemacs-launched server processes (LSP and DAP) implicitly. Workspace-local config and
    /// git full-trust still require explicit `:workspace-trust`. This is the default — language
    /// servers are configured globally, so auto-starting them in fresh workspaces matches user
    /// expectations while the workspace-controlled `.zemacs/` config still requires opt-in.
    #[default]
    Servers,
    /// Trust everything implicitly. Explicit excludes still win.
    Insecure,
}

impl From<ImplicitTrustLevelConfig> for ImplicitTrustLevel {
    fn from(v: ImplicitTrustLevelConfig) -> Self {
        match v {
            ImplicitTrustLevelConfig::None => ImplicitTrustLevel::None,
            ImplicitTrustLevelConfig::Servers => ImplicitTrustLevel::Servers,
            ImplicitTrustLevelConfig::Insecure => ImplicitTrustLevel::Insecure,
        }
    }
}

impl From<&WorkspaceTrustConfig> for zemacs_loader::workspace_trust::Config {
    fn from(v: &WorkspaceTrustConfig) -> Self {
        Self {
            level: v.level.into(),
            prompt: v.prompt,
            trusted_globs: zemacs_loader::workspace_trust::build_trusted_globs(&v.trusted),
        }
    }
}

impl Config {
    pub fn code_action_hint(&self) -> bool {
        self.gutters.layout.contains(&GutterType::CodeActionHint)
            || self
                .statusline
                .left
                .contains(&StatusLineElement::CodeActionHint)
            || self
                .statusline
                .center
                .contains(&StatusLineElement::CodeActionHint)
            || self
                .statusline
                .right
                .contains(&StatusLineElement::CodeActionHint)
    }
}

#[derive(Debug, Default, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize, Clone, Copy)]
#[serde(rename_all = "kebab-case")]
pub struct BufferPickerConfig {
    pub start_position: PickerStartPosition,
}

#[derive(Debug, Default, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize, Clone, Copy)]
#[serde(rename_all = "kebab-case")]
pub enum PickerStartPosition {
    #[default]
    Current,
    Previous,
}

impl PickerStartPosition {
    #[must_use]
    pub fn is_previous(self) -> bool {
        matches!(self, Self::Previous)
    }

    #[must_use]
    pub fn is_current(self) -> bool {
        matches!(self, Self::Current)
    }
}

#[derive(Debug, Default, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize, Clone, Copy)]
#[serde(rename_all = "kebab-case")]
pub enum KittyKeyboardProtocolConfig {
    #[default]
    Auto,
    Disabled,
    Enabled,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, Eq, PartialOrd, Ord)]
#[serde(default, rename_all = "kebab-case", deny_unknown_fields)]
pub struct SmartTabConfig {
    pub enable: bool,
    pub supersede_menu: bool,
}

impl Default for SmartTabConfig {
    fn default() -> Self {
        SmartTabConfig {
            enable: true,
            supersede_menu: false,
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case", deny_unknown_fields)]
pub struct TerminalConfig {
    pub command: String,
    #[serde(default)]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
}

#[cfg(windows)]
pub fn get_terminal_provider() -> Option<TerminalConfig> {
    use zemacs_stdx::env::binary_exists;

    if binary_exists("wt") {
        return Some(TerminalConfig {
            command: "wt".to_string(),
            args: vec![
                "new-tab".to_string(),
                "--title".to_string(),
                "DEBUG".to_string(),
                "cmd".to_string(),
                "/C".to_string(),
            ],
        });
    }

    Some(TerminalConfig {
        command: "conhost".to_string(),
        args: vec!["cmd".to_string(), "/C".to_string()],
    })
}

#[cfg(not(any(windows, target_arch = "wasm32")))]
pub fn get_terminal_provider() -> Option<TerminalConfig> {
    use zemacs_stdx::env::{binary_exists, env_var_is_set};

    if env_var_is_set("TMUX") && binary_exists("tmux") {
        return Some(TerminalConfig {
            command: "tmux".to_string(),
            args: vec!["split-window".to_string()],
        });
    }

    if env_var_is_set("WEZTERM_UNIX_SOCKET") && binary_exists("wezterm") {
        return Some(TerminalConfig {
            command: "wezterm".to_string(),
            args: vec!["cli".to_string(), "split-pane".to_string()],
        });
    }

    None
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case", deny_unknown_fields)]
pub struct LspConfig {
    /// Enables LSP
    pub enable: bool,
    /// Display LSP messagess from $/progress below statusline
    pub display_progress_messages: bool,
    /// Display LSP messages from window/showMessage below statusline
    pub display_messages: bool,
    /// Enable automatic pop up of signature help (parameter hints)
    pub auto_signature_help: bool,
    /// Display docs under signature help popup
    pub display_signature_help_docs: bool,
    /// Display inlay hints
    pub display_inlay_hints: bool,
    /// Automatically highlight symbol references at the cursor.
    pub auto_document_highlight: bool,
    /// Maximum displayed length of inlay hints (excluding the added trailing `…`).
    /// If it's `None`, there's no limit
    pub inlay_hints_length_limit: Option<NonZeroU8>,
    /// Display document color swatches
    pub display_color_swatches: bool,
    /// Whether to enable snippet support
    pub snippets: bool,
    /// Whether to include declaration in the goto reference query
    pub goto_reference_include_declaration: bool,
}

impl Default for LspConfig {
    fn default() -> Self {
        Self {
            enable: true,
            display_progress_messages: false,
            display_messages: true,
            auto_signature_help: true,
            display_signature_help_docs: true,
            display_inlay_hints: false,
            auto_document_highlight: false,
            inlay_hints_length_limit: None,
            snippets: true,
            goto_reference_include_declaration: true,
            display_color_swatches: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", default, deny_unknown_fields)]
pub struct SearchConfig {
    /// Smart case: Case insensitive searching unless pattern contains upper case characters. Defaults to true.
    pub smart_case: bool,
    /// Whether the search should wrap after depleting the matches. Default to true.
    pub wrap_around: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", default, deny_unknown_fields)]
pub struct StatusLineConfig {
    pub left: Vec<StatusLineElement>,
    pub center: Vec<StatusLineElement>,
    pub right: Vec<StatusLineElement>,
    pub separator: String,
    pub mode: ModeConfig,
    pub diagnostics: Vec<Severity>,
    pub workspace_diagnostics: Vec<Severity>,
}

impl Default for StatusLineConfig {
    fn default() -> Self {
        use StatusLineElement as E;

        Self {
            left: vec![
                E::Mode,
                E::Spinner,
                E::FileName,
                E::ReadOnlyIndicator,
                E::FileModificationIndicator,
            ],
            center: vec![],
            right: vec![
                E::Diagnostics,
                E::CiStatus,
                // airline-style warnings (only show when there's something to warn about)
                E::TrailingWhitespace,
                E::MixedIndent,
                E::Selections,
                E::Register,
                E::FileType,
                E::FileEncoding,
                E::FileFormatIcon,
                E::Position,
                E::PositionPercentage,
            ],
            separator: String::from("│"),
            mode: ModeConfig::default(),
            diagnostics: vec![Severity::Warning, Severity::Error],
            workspace_diagnostics: vec![Severity::Warning, Severity::Error],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", default, deny_unknown_fields)]
pub struct ModeConfig {
    pub normal: String,
    pub insert: String,
    pub select: String,
}

impl Default for ModeConfig {
    fn default() -> Self {
        Self {
            normal: String::from("NOR"),
            insert: String::from("INS"),
            select: String::from("SEL"),
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StatusLineElement {
    /// The editor mode (Normal, Insert, Visual/Selection)
    Mode,

    /// The LSP activity spinner
    Spinner,

    /// The file basename (the leaf of the open file's path)
    FileBaseName,

    /// The relative file path
    FileName,

    /// The file absolute path
    FileAbsolutePath,

    // The file modification indicator
    FileModificationIndicator,

    /// An indicator that shows `"[readonly]"` when a file cannot be written
    ReadOnlyIndicator,

    /// The file encoding
    FileEncoding,

    /// The file line endings (CRLF or LF)
    FileLineEnding,

    /// The file indentation style
    FileIndentStyle,

    /// The file type (language ID or "text")
    FileType,

    /// A summary of the number of errors and warnings
    Diagnostics,

    /// Latest CI run status badge (GitHub Actions), config name `ci-status`
    CiStatus,

    /// A summary of the number of errors and warnings on file and workspace
    WorkspaceDiagnostics,

    /// The number of selections (cursors)
    Selections,

    /// The number of characters currently in primary selection
    PrimarySelectionLength,

    /// The cursor position
    Position,

    /// The separator string
    Separator,

    /// The cursor position as a percent of the total file
    PositionPercentage,

    /// The total line numbers of the current file
    TotalLineNumbers,

    /// A single space
    Spacer,

    /// Current version control information
    VersionControl,

    /// Indicator for selected register
    Register,

    /// The base of current working directory
    CurrentWorkingDirectory,

    /// Indicator for when code actions are available
    CodeActionHint,

    /// vim-airline style warning showing the count of lines with trailing
    /// whitespace, e.g. `≥123 trailing`. Hidden when there is none.
    TrailingWhitespace,

    /// vim-airline style warning shown when a file mixes tabs and spaces for
    /// indentation, e.g. `mixed-indent[12]`. Hidden when indentation is clean.
    MixedIndent,

    /// The file line ending shown with a nerd-font OS icon (LF/CRLF/CR).
    FileFormatIcon,
}

// Cursor shape is read and used on every rendered frame and so needs
// to be fast. Therefore we avoid a hashmap and use an enum indexed array.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CursorShapeConfig([CursorKind; 3]);

impl CursorShapeConfig {
    pub fn from_mode(&self, mode: Mode) -> CursorKind {
        self.get(mode as usize).copied().unwrap_or_default()
    }
}

impl<'de> Deserialize<'de> for CursorShapeConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let m = HashMap::<Mode, CursorKind>::deserialize(deserializer)?;
        let into_cursor = |mode: Mode| m.get(&mode).copied().unwrap_or_default();
        Ok(CursorShapeConfig([
            into_cursor(Mode::Normal),
            into_cursor(Mode::Select),
            into_cursor(Mode::Insert),
        ]))
    }
}

impl Serialize for CursorShapeConfig {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut map = serializer.serialize_map(Some(self.len()))?;
        let modes = [Mode::Normal, Mode::Select, Mode::Insert];
        for mode in modes {
            map.serialize_entry(&mode, &self.from_mode(mode))?;
        }
        map.end()
    }
}

impl std::ops::Deref for CursorShapeConfig {
    type Target = [CursorKind; 3];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Default for CursorShapeConfig {
    fn default() -> Self {
        Self([CursorKind::Block; 3])
    }
}

/// What to open when zemacs is launched with no file arguments.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StartupScreen {
    /// Scratch buffer with the vim-startify start screen (default).
    #[default]
    Startify,
    /// Open the single most-recently-used file. Falls back to Startify if the
    /// MRU list is empty.
    Recent,
    /// Restore the previous session: reopen the tabs/cursor saved on last exit.
    /// Falls back to Startify when there is no saved session.
    Session,
    /// Open the specific file named by `startup-file`. Falls back to Startify if
    /// that path is unset or no longer a file.
    File,
}

/// bufferline render modes
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BufferLine {
    /// Don't render bufferline
    #[default]
    Never,
    /// Always render
    Always,
    /// Only if multiple buffers are open
    Multiple,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LineNumber {
    /// Show absolute line number
    Absolute,

    /// If focused and in normal/select mode, show relative line number to the primary cursor.
    /// If unfocused or in insert mode, show absolute line number.
    Relative,
}

impl std::str::FromStr for LineNumber {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "absolute" | "abs" => Ok(Self::Absolute),
            "relative" | "rel" => Ok(Self::Relative),
            _ => anyhow::bail!("Line number can only be `absolute` or `relative`."),
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GutterType {
    /// Show diagnostics and other features like breakpoints
    Diagnostics,
    /// Show line numbers
    LineNumbers,
    /// Show one blank space
    Spacer,
    /// Highlight local changes
    Diff,
    /// Indicator for when code actions are available
    CodeActionHint,
    /// Show vim marks (markology) in the gutter
    Marks,
    /// Git-blame annotate column (JetBrains "Annotate"); zero-width until enabled
    Blame,
    /// Vim signs (`:sign place`); zero-width until a sign is placed in the file
    Signs,
}

impl std::str::FromStr for GutterType {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "diagnostics" => Ok(Self::Diagnostics),
            "spacer" => Ok(Self::Spacer),
            "line-numbers" => Ok(Self::LineNumbers),
            "diff" => Ok(Self::Diff),
            "code-action-hint" => Ok(Self::CodeActionHint),
            "marks" => Ok(Self::Marks),
            "blame" => Ok(Self::Blame),
            "signs" => Ok(Self::Signs),
            _ => anyhow::bail!(
                "Gutter type can only be `diagnostics`, `spacer`, `line-numbers` or `diff`."
            ),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WhitespaceConfig {
    pub render: WhitespaceRender,
    pub characters: WhitespaceCharacters,
}

impl Default for WhitespaceConfig {
    fn default() -> Self {
        Self {
            render: WhitespaceRender::Basic(WhitespaceRenderValue::None),
            characters: WhitespaceCharacters::default(),
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged, rename_all = "kebab-case")]
pub enum WhitespaceRender {
    Basic(WhitespaceRenderValue),
    Specific {
        default: Option<WhitespaceRenderValue>,
        space: Option<WhitespaceRenderValue>,
        nbsp: Option<WhitespaceRenderValue>,
        nnbsp: Option<WhitespaceRenderValue>,
        tab: Option<WhitespaceRenderValue>,
        newline: Option<WhitespaceRenderValue>,
    },
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WhitespaceRenderValue {
    None,
    // TODO
    // Selection,
    All,
}

impl WhitespaceRender {
    pub fn space(&self) -> WhitespaceRenderValue {
        match *self {
            Self::Basic(val) => val,
            Self::Specific { default, space, .. } => {
                space.or(default).unwrap_or(WhitespaceRenderValue::None)
            }
        }
    }
    pub fn nbsp(&self) -> WhitespaceRenderValue {
        match *self {
            Self::Basic(val) => val,
            Self::Specific { default, nbsp, .. } => {
                nbsp.or(default).unwrap_or(WhitespaceRenderValue::None)
            }
        }
    }
    pub fn nnbsp(&self) -> WhitespaceRenderValue {
        match *self {
            Self::Basic(val) => val,
            Self::Specific { default, nnbsp, .. } => {
                nnbsp.or(default).unwrap_or(WhitespaceRenderValue::None)
            }
        }
    }
    pub fn tab(&self) -> WhitespaceRenderValue {
        match *self {
            Self::Basic(val) => val,
            Self::Specific { default, tab, .. } => {
                tab.or(default).unwrap_or(WhitespaceRenderValue::None)
            }
        }
    }
    pub fn newline(&self) -> WhitespaceRenderValue {
        match *self {
            Self::Basic(val) => val,
            Self::Specific {
                default, newline, ..
            } => newline.or(default).unwrap_or(WhitespaceRenderValue::None),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct AutoSave {
    /// Auto save immediately on every modification (JetBrains-style), with no
    /// delay. Defaults to enabled. Undo history is not fragmented by these saves.
    #[serde(default = "default_true")]
    pub on_change: bool,
    /// Auto save after a delay in milliseconds. Defaults to disabled.
    #[serde(default)]
    pub after_delay: AutoSaveAfterDelay,
    /// Auto save on focus lost. Defaults to enabled.
    #[serde(default = "default_true")]
    pub focus_lost: bool,
}

impl Default for AutoSave {
    fn default() -> Self {
        Self {
            on_change: true,
            after_delay: AutoSaveAfterDelay::default(),
            focus_lost: true,
        }
    }
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AutoSaveAfterDelay {
    #[serde(default)]
    /// Enable auto save after delay. Defaults to false.
    pub enable: bool,
    #[serde(default = "default_auto_save_delay")]
    /// Time delay in milliseconds. Defaults to [DEFAULT_AUTO_SAVE_DELAY].
    pub timeout: u64,
}

impl Default for AutoSaveAfterDelay {
    fn default() -> Self {
        Self {
            enable: false,
            timeout: DEFAULT_AUTO_SAVE_DELAY,
        }
    }
}

fn default_auto_save_delay() -> u64 {
    DEFAULT_AUTO_SAVE_DELAY
}

fn deserialize_auto_save<'de, D>(deserializer: D) -> Result<AutoSave, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize, Serialize)]
    #[serde(untagged, deny_unknown_fields, rename_all = "kebab-case")]
    enum AutoSaveToml {
        EnableFocusLost(bool),
        AutoSave(AutoSave),
    }

    match AutoSaveToml::deserialize(deserializer)? {
        // `auto-save = true` enables both immediate on-change saving and
        // save-on-focus-lost; `auto-save = false` disables everything.
        AutoSaveToml::EnableFocusLost(enable) => Ok(AutoSave {
            on_change: enable,
            focus_lost: enable,
            after_delay: AutoSaveAfterDelay::default(),
        }),
        AutoSaveToml::AutoSave(auto_save) => Ok(auto_save),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WhitespaceCharacters {
    pub space: char,
    pub nbsp: char,
    pub nnbsp: char,
    pub tab: char,
    pub tabpad: char,
    pub newline: char,
}

impl Default for WhitespaceCharacters {
    fn default() -> Self {
        Self {
            space: '·',   // U+00B7
            nbsp: '⍽',    // U+237D
            nnbsp: '␣',   // U+2423
            tab: '→',     // U+2192
            newline: '⏎', // U+23CE
            tabpad: ' ',
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct IndentGuidesConfig {
    pub render: bool,
    pub character: char,
    pub skip_levels: u8,
}

impl Default for IndentGuidesConfig {
    fn default() -> Self {
        Self {
            skip_levels: 0,
            render: false,
            character: '│',
        }
    }
}

/// Line ending configuration.
#[derive(Default, Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LineEndingConfig {
    /// The platform's native line ending.
    ///
    /// `crlf` on Windows, otherwise `lf`.
    #[default]
    Native,
    /// Line feed.
    LF,
    /// Carriage return followed by line feed.
    Crlf,
    /// Form feed.
    #[cfg(feature = "unicode-lines")]
    FF,
    /// Carriage return.
    #[cfg(feature = "unicode-lines")]
    CR,
    /// Next line.
    #[cfg(feature = "unicode-lines")]
    Nel,
}

impl From<LineEndingConfig> for LineEnding {
    fn from(line_ending: LineEndingConfig) -> Self {
        match line_ending {
            LineEndingConfig::Native => NATIVE_LINE_ENDING,
            LineEndingConfig::LF => LineEnding::LF,
            LineEndingConfig::Crlf => LineEnding::Crlf,
            #[cfg(feature = "unicode-lines")]
            LineEndingConfig::FF => LineEnding::FF,
            #[cfg(feature = "unicode-lines")]
            LineEndingConfig::CR => LineEnding::CR,
            #[cfg(feature = "unicode-lines")]
            LineEndingConfig::Nel => LineEnding::Nel,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PopupBorderConfig {
    None,
    All,
    Popup,
    Menu,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case", deny_unknown_fields)]
pub struct WordCompletion {
    pub enable: bool,
    pub trigger_length: NonZeroU8,
}

impl Default for WordCompletion {
    fn default() -> Self {
        Self {
            enable: true,
            trigger_length: NonZeroU8::new(7).unwrap(),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            scrolloff: 0,
            scroll_lines: 3,
            mouse: true,
            mouse_yank_register: '*',
            shell: if cfg!(windows) {
                vec!["cmd".to_owned(), "/C".to_owned()]
            } else {
                vec!["sh".to_owned(), "-c".to_owned()]
            },
            line_number: LineNumber::Absolute,
            cursorline: false,
            cursorcolumn: false,
            highlight_word_under_cursor: true,
            search_highlight: false,
            gutters: GutterConfig::default(),
            middle_click_paste: true,
            auto_pairs: AutoPairConfig::default(),
            auto_completion: true,
            path_completion: true,
            word_completion: WordCompletion::default(),
            auto_format: true,
            default_yank_register: '"',
            auto_save: AutoSave::default(),
            auto_reload: true,
            idle_timeout: Duration::from_millis(250),
            completion_timeout: Duration::from_millis(250),
            preview_completion_insert: true,
            completion_trigger_len: 2,
            auto_info: true,
            auto_info_exclude: vec!["g".into(), "y".into(), "z".into(), "d".into()],
            auto_info_leader_only: true,
            which_key_global: false,
            which_key: true,
            fzf: FzfConfig::default(),
            vim_sneak: true,
            source_vimrc: false,
            source_emacs_config: false,
            source_elisp_file: None,
            source_viml_file: None,
            file_picker: FilePickerConfig::default(),
            file_explorer: FileExplorerConfig::default(),
            statusline: StatusLineConfig::default(),
            render_statusline: true,
            cursor_shape: CursorShapeConfig::default(),
            true_color: false,
            transparent_background: false,
            undercurl: false,
            search: SearchConfig::default(),
            lsp: LspConfig::default(),
            terminal: get_terminal_provider(),
            rulers: Vec::new(),
            whitespace: WhitespaceConfig::default(),
            bufferline: BufferLine::default(),
            split_right: true,
            split_below: true,
            indent_guides: IndentGuidesConfig::default(),
            color_modes: false,
            soft_wrap: SoftWrap {
                enable: Some(false),
                ..SoftWrap::default()
            },
            text_width: 80,
            completion_replace: false,
            continue_comments: true,
            workspace_lsp_roots: Vec::new(),
            default_line_ending: LineEndingConfig::default(),
            insert_final_newline: true,
            atomic_save: true,
            backup: false,
            backup_ext: "~".to_string(),
            backup_dir: String::new(),
            backup_skip: String::new(),
            swapfile: false,
            swap_directory: String::new(),
            undofile: false,
            undo_dir: String::new(),
            title: false,
            title_string: String::new(),
            conceallevel: 0,
            concealcursor: String::new(),
            trim_final_newlines: false,
            trim_trailing_whitespace: false,
            smart_tab: Some(SmartTabConfig::default()),
            popup_border: PopupBorderConfig::None,
            indent_heuristic: IndentationHeuristic::default(),
            jump_label_alphabet: ('a'..='z').collect(),
            inline_diagnostics: InlineDiagnosticsConfig::default(),
            end_of_line_diagnostics: DiagnosticFilter::Enable(Severity::Hint),
            clipboard_provider: ClipboardProvider::default(),
            editor_config: true,
            rainbow_brackets: false,
            kitty_keyboard_protocol: Default::default(),
            buffer_picker: BufferPickerConfig::default(),
            workspace_trust: WorkspaceTrustConfig::default(),
            startup: StartupScreen::default(),
            startup_file: String::new(),
            sync_zwire_theme: false,
        }
    }
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            wrap_around: true,
            smart_case: true,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Breakpoint {
    pub id: Option<usize>,
    pub verified: bool,
    pub message: Option<String>,

    pub line: usize,
    pub column: Option<usize>,
    pub condition: Option<String>,
    pub hit_condition: Option<String>,
    pub log_message: Option<String>,
}

use futures_util::stream::{Flatten, Once};

type Diagnostics = BTreeMap<Uri, Vec<(lsp::Diagnostic, DiagnosticProvider)>>;

/// A single entry in a vim-style quickfix or location list: a jumpable
/// `{path, line, col}` plus a preview/message. Lines and columns are
/// 0-indexed (matching the editor's internal convention; the `:cgetexpr`
/// parser converts from vim's 1-based output).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QfEntry {
    pub path: PathBuf,
    pub line: usize,
    pub col: usize,
    pub text: String,
}

/// A saved vim tabpage: a serialized window layout plus each window's saved
/// selection (in left-to-right leaf order). Tabs share buffers — leaves
/// reference `DocumentId`s in the shared `documents` map — and only one tab's
/// window tree is live at a time (the others are parked here and rebuilt on
/// switch). See `Editor::switch_tab`/`new_tab`.
#[derive(Debug, Clone)]
pub struct TabPage {
    pub shape: crate::tree::TreeShape,
    pub selections: Vec<Selection>,
    /// User-assigned tab name (emacs `tab-rename` / `tab-switch`); `None` shows a
    /// default numbered label.
    pub name: Option<String>,
}

/// The document of a parked tab's focused window (or its first window).
fn tab_focused_doc(shape: &crate::tree::TreeShape) -> DocumentId {
    use crate::tree::TreeShape;
    match shape {
        TreeShape::Leaf { doc, .. } => *doc,
        TreeShape::Split { children, .. } => {
            // Prefer the subtree containing the focused leaf; else the first.
            for (_, child) in children {
                if shape_has_focus(child) {
                    return tab_focused_doc(child);
                }
            }
            children
                .first()
                .map(|(_, c)| tab_focused_doc(c))
                .unwrap_or_default()
        }
    }
}

fn shape_has_focus(shape: &crate::tree::TreeShape) -> bool {
    use crate::tree::TreeShape;
    match shape {
        TreeShape::Leaf { focused, .. } => *focused,
        TreeShape::Split { children, .. } => children.iter().any(|(_, c)| shape_has_focus(c)),
    }
}

/// A snapshot of the latest in-flight LSP `$/progress` work, mirrored onto the
/// [`Editor`] so UI surfaces can render a determinate gauge when a percentage is
/// reported (e.g. rust-analyzer indexing) or a spinner-style label otherwise.
#[derive(Debug, Clone, Default)]
pub struct LspProgress {
    /// The language server's name (e.g. `rust-analyzer`).
    pub server: String,
    /// The work title (e.g. `Indexing`, `Building`).
    pub title: String,
    /// The latest detail message, if any.
    pub message: Option<String>,
    /// Reported completion in `0..=100`, if the server provides one.
    pub percentage: Option<u32>,
}

/// vim visual-block (`CTRL-V`) state. Block mode reuses `Mode::Select`; this
/// flag (when `Some`) marks that the current Select is a rectangular block.
/// `anchor` is the fixed corner in visual (row, col); the active corner is
/// always the primary cursor, so motions extend the block by moving the cursor
/// and re-projecting the rectangle (see `block_reproject`). Cleared on return to
/// Normal, mirroring `overwrite`.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct BlockSelect {
    /// The fixed corner, in visual (row, col) coordinates.
    pub anchor: (usize, usize),
    /// vim `CTRL-V $`: extend each row to its own line end (ragged right).
    pub to_eol: bool,
}

/// A request to run the external `fzf` binary (fzf.vim-style commands). The
/// terminal layer hands the TTY to `fzf` with `candidates` piped to stdin and
/// `options` as extra CLI flags, then runs `sink` — a zemacs `:` command line
/// with each `{}` replaced by the picked line — on the selection.
#[derive(Debug, Clone)]
pub struct FzfRequest {
    pub candidates: Vec<String>,
    pub prompt: String,
    pub sink: String,
    pub options: Vec<String>,
    /// Show a preview pane (the config's `fzf.preview` command over the picked
    /// line). Set for file-listing commands (:Files/:Buffers).
    pub preview: bool,
    /// Optional shell command whose output fzf streams as its source (fzf.vim's
    /// `source` — e.g. `git ls-files`, `rg …`). When set, `candidates` is ignored
    /// and fzf runs this via the shell (set as the child's FZF_DEFAULT_COMMAND).
    pub command: Option<String>,
}

/// Configuration for the external-`fzf` integration (fzf.vim-style commands).
/// `fzf` also honors the user's `$FZF_DEFAULT_OPTS`; these apply on top.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", default, deny_unknown_fields)]
pub struct FzfConfig {
    /// Extra `fzf` CLI flags added to every fzf.vim-style command — popup size
    /// (`--height=95%`), layout, border, etc.
    pub options: Vec<String>,
    /// Preview command for file-listing commands; `{}` is the picked file.
    /// Empty disables the preview pane.
    pub preview: String,
    /// `fzf` `--preview-window` spec (position/size of the preview pane).
    pub preview_window: String,
}

impl Default for FzfConfig {
    fn default() -> Self {
        // Empty by default so zemacs adds NOTHING that would override the user's
        // own `$FZF_DEFAULT_OPTS` (preview, layout, colors, size — fzf reads it,
        // and command-line args we'd add here would clobber it). Set these to opt
        // in to zemacs-provided defaults when you have no env config.
        Self {
            options: Vec::new(),
            preview: String::new(),
            preview_window: "right:55%".into(),
        }
    }
}

/// A vim global mark (`A`-`Z`) or numbered file mark (`0`-`9`): a file path plus
/// a `(line, col)` position (both 0-based). Unlike the buffer-local `a`-`z` marks
/// (stored per-`Document`), these live on the `Editor` so they survive buffer
/// close and cross-file jumps, and round-trip through `.zemacsinfo`. Position is
/// stored as line/col (not a char offset) so it stays meaningful after the file
/// changes between sessions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalMark {
    pub path: PathBuf,
    pub line: usize,
    pub col: usize,
}

pub struct Editor {
    /// Current editing mode.
    pub mode: Mode,
    /// True when the active keymap preset carries the vim base (`vim`/`spacemacs`).
    /// Command and search paths read this to gate vim-only semantics (e.g. vim
    /// magic-regex translation) so `helix`/`emacs` presets keep native behavior.
    /// Set from `config.keymap` at startup and on every `:keymap` switch; defaults
    /// to true because `spacemacs` (a vim-base preset) is the default.
    pub vim_semantics: bool,
    /// Direction of the last search: `true` after a forward `/` (or `*`), `false`
    /// after a backward `?` (or `#`). vim's `n` repeats in this direction and `N`
    /// reverses it, so after `?pat` an `n` moves backward. Defaults to forward.
    pub last_search_forward: bool,
    /// vim `` `" ``: the last cursor `(line, col)` (0-based) per file path, recorded
    /// when a buffer is closed so reopening the file restores where you were. Stored
    /// as line/col (not a char offset) so it survives external edits and can be
    /// seeded across sessions from `.zemacsinfo`.
    pub last_positions: std::collections::HashMap<PathBuf, (usize, usize)>,
    /// vim Replace mode (`R`): typed characters overtype existing ones instead
    /// of being inserted. Only meaningful while `mode == Insert`; cleared on
    /// return to Normal.
    pub overwrite: bool,
    /// emacs `abbrev-mode`: when on, typing a non-word character in Insert mode
    /// first expands the abbrev before point (emacs's `self-insert-command` runs
    /// `expand-abbrev` for non-word-constituent input). Off by default.
    pub abbrev_mode: bool,
    /// vim `digraph`: a pending digraph entry armed by `<BS>` in Insert mode.
    /// Holds the character before the cursor at the time `<BS>` was pressed; the
    /// next inserted character combines with it (`{char1}<BS>{char2}`). Cleared on
    /// the combining insert and on return to Normal mode.
    pub digraph_pending: Option<char>,
    /// vim insert-mode `CTRL-O` one-shot: when armed, the editor is temporarily
    /// in Normal mode for exactly one command, after which the dispatch loop
    /// returns to Insert. Set by `insert_command_normal`, consumed in
    /// `EditorView::handle_keymap_event` once the following command completes.
    pub insert_oneshot: bool,
    /// Spacemacs transient state: set by `exit_transient_state` (the `q` key of
    /// a transient state) to ask the key dispatcher to drop the latched sticky
    /// keymap node, the same way `Esc` does. Consumed in
    /// `EditorView::handle_keymap_event`.
    pub exit_transient: bool,
    /// Emacs `text-scale-adjust` (spacemacs `SPC z x`): the current text-scale
    /// step, 0 = the terminal's own font size. Each step emits xterm `OSC 50`
    /// (`#+1` / `#-1` / `#0`), which selects the next/previous font slot on
    /// terminals that implement it and is ignored elsewhere; the count is kept
    /// here so the state survives a reset and can be reported.
    pub text_scale: i32,
    /// Spacemacs frame zoom (`SPC z f`, emacs `zoom-frm-in/out`): the frame
    /// scale step. In a terminal the frame font *is* the text font, so this
    /// drives the same `OSC 50` mechanism as [`text_scale`](Self::text_scale)
    /// but is tracked separately (a GUI host scales the whole frame).
    pub frame_scale: i32,
    /// vim `showmatch`: the position of the bracket that matches the closing
    /// bracket just typed, plus the instant it was armed. The focused view
    /// highlights it (`ui.cursor.match`) until `matchtime` tenths of a second
    /// have passed — vim jumps the cursor there, which a non-blocking editor
    /// cannot do without eating the next keystroke.
    pub show_match: Option<(DocumentId, usize, Instant)>,
    /// vim `virtualedit`: the comma list as set (`onemore`, `all`, `block`,
    /// `insert`, or empty). `onemore`/`all` let the cursor rest one past the last
    /// character of a line, so leaving Insert mode no longer pulls it back.
    /// Set by `:set virtualedit=…`; the option store lives in zemacs-term, so the
    /// value is mirrored here for the consumers in this crate.
    pub virtualedit: String,
    /// vim `equalalways`: re-balance every window whenever one is split.
    pub equalalways: bool,
    /// vim `tabpagemax`: refuse to open more than this many tab pages (0 = no
    /// limit, which is what zemacs did).
    pub tabpagemax: usize,
    /// vim visual-block selection state, when the current Select is a block.
    pub block: Option<BlockSelect>,
    /// vim visual-line state (`V`): the fixed anchor line of a linewise visual
    /// selection. While set, every Select-mode motion re-derives the region so
    /// BOTH the anchor line and the cursor's line stay whole-line selected (vim
    /// linewise visual), instead of a one-shot charwise extend. Mutually
    /// exclusive with [`block`](Self::block); cleared on return to Normal.
    pub visual_line: Option<usize>,
    /// Spacemacs subword-mode (`SPC t c`): when set, the `w`/`b`/`e` word
    /// motions (and the operators built on them) move by sub-word, splitting
    /// CamelCase / snake_case identifiers. A persistent toggle.
    pub subword: bool,
    /// Emacs superword-mode (`SPC t C`): when set, the `w`/`b`/`e` word motions
    /// treat symbol-syntax characters (`_`, `-`, etc.) as part of a word, so a
    /// whole `snake_case_symbol` moves as one word (the inverse of subword-mode).
    /// Mutually exclusive with [`subword`](Self::subword). A persistent toggle.
    pub superword: bool,
    /// Spacemacs auto-fill-mode (`SPC t F`): when set, typing past `text_width`
    /// breaks the line at the last whitespace (Emacs auto-fill). A persistent
    /// toggle; only applies with a single cursor.
    pub auto_fill: bool,
    /// Emacs picture-mode / `edit-picture`: when set, self-inserting a character
    /// overwrites the cell under point and then advances point one step in
    /// [`picture_dir`](Self::picture_dir) (quarter-plane overwrite editing),
    /// padding lines/columns with spaces past their ends. A persistent toggle.
    pub picture_mode: bool,
    /// The picture-mode drawing direction — which way point advances after a
    /// character is drawn (`picture-movement-*`). Only meaningful while
    /// [`picture_mode`](Self::picture_mode) is set.
    pub picture_dir: zemacs_core::picture::Dir,
    /// Picture-mode tab stops (columns), set by `picture-set-tab-stops` and used
    /// by `picture-tab`.
    pub picture_tab_stops: Vec<usize>,
    /// Spacemacs follow-mode (`SPC w f`): when set, windows showing the same
    /// document scroll as one continuous view — sibling windows are re-anchored
    /// to pick up where the focused window ends. A persistent toggle.
    pub follow: bool,
    /// Emacs `fill-prefix` (`C-x .` = `set-fill-prefix`): a string automatically
    /// inserted at the start of each new line produced by auto-fill (and other
    /// fill commands). `None` means no prefix. Set from the text between the
    /// line start and point by `set-fill-prefix`.
    pub fill_prefix: Option<String>,
    /// Emacs `goal-column` (`C-x C-n` = `set-goal-column`): a sticky column that
    /// vertical line motion (`next-line`/`previous-line`) tries to land on,
    /// overriding the "remembered" column. `None` means normal behavior.
    pub goal_column: Option<usize>,
    pub tree: Tree,
    pub next_document_id: DocumentId,
    pub documents: BTreeMap<DocumentId, Document>,

    // We Flatten<> to resolve the inner DocumentSavedEventFuture. For that we need a stream of streams, hence the Once<>.
    // https://stackoverflow.com/a/66875668
    pub saves: HashMap<DocumentId, UnboundedSender<Once<DocumentSavedEventFuture>>>,
    pub save_queue: SelectAll<Flatten<UnboundedReceiverStream<Once<DocumentSavedEventFuture>>>>,
    pub write_count: usize,

    pub count: Option<std::num::NonZeroUsize>,
    pub selected_register: Option<char>,
    /// The last `:substitute` (pattern, replacement, flags), for vim `&` repeat.
    pub last_substitute: Option<(String, String, String)>,
    pub registers: Registers,
    pub macro_recording: Option<(char, Vec<KeyEvent>)>,
    pub macro_replaying: Vec<char>,
    /// Register of the most recently replayed macro, for vim `@@` repeat.
    pub last_macro_register: Option<char>,
    /// Bounded ring of the most recently pressed keys (for "copy last keys"). Newest at the back.
    pub last_keys: std::collections::VecDeque<KeyEvent>,
    pub language_servers: zemacs_lsp::Registry,
    pub diagnostics: Diagnostics,
    pub diff_providers: DiffProviderRegistry,

    pub debug_adapters: dap::registry::Registry,
    pub breakpoints: HashMap<PathBuf, Vec<Breakpoint>>,

    /// vim global marks (`A`-`Z`) and numbered file marks (`0`-`9`), keyed by
    /// mark char. Unlike buffer-local `a`-`z` marks these persist across buffer
    /// close, jump across files, and round-trip through `.zemacsinfo`. Numbered
    /// marks are populated on startup from the recent-files history and are not
    /// user-settable (vim ignores `m0`-`m9`).
    pub global_marks: HashMap<char, GlobalMark>,

    /// Text inserted during the most recently completed insert session. Backs
    /// the vim `.` register (and `i_CTRL-A` / `i_CTRL-R .`). Updated on leaving
    /// insert mode.
    pub last_inserted_text: String,

    /// The global vim quickfix list and the index of the current entry. Filled
    /// by `:cgetexpr`/`:cbuffer`/`:Diagnostics`/`:make`, navigated with
    /// `:cnext`/`:cprev`/`:cc`, displayed by `:copen`.
    pub quickfix: Vec<QfEntry>,
    pub quickfix_idx: Option<usize>,
    /// Past quickfix lists for `:colder`/`:cnewer`/`:chistory` (vim keeps up to
    /// 10). `quickfix_stack_pos` indexes the currently-active list within it.
    pub quickfix_stack: Vec<Vec<QfEntry>>,
    pub quickfix_stack_pos: usize,

    /// Parked vim tabpages. The entry at `current_tab` is a stale placeholder
    /// (the live layout is in `tree`); it is refreshed from `tree` whenever the
    /// active tab changes. Empty until the first `:tabnew`.
    pub tabs: Vec<TabPage>,
    pub current_tab: usize,
    /// Recently closed tabs, most-recent last — reopened by emacs `tab-undo`.
    pub closed_tabs: Vec<TabPage>,
    /// emacs `tab-bar-history` back/forward stacks of visited tab indices, and
    /// whether history recording is on (`tab-bar-history-mode`).
    pub tab_back: Vec<usize>,
    pub tab_forward: Vec<usize>,
    pub tab_history_mode: bool,

    pub syn_loader: Arc<ArcSwap<syntax::Loader>>,
    pub theme_loader: Arc<theme::Loader>,
    /// last_theme is used for theme previews. We store the current theme here,
    /// and if previewing is cancelled, we can return to it.
    pub last_theme: Option<Theme>,
    /// The currently applied editor theme. While previewing a theme, the previewed theme
    /// is set here.
    pub theme: Theme,

    /// The primary Selection prior to starting a goto_line_number preview. This is
    /// restored when the preview is aborted, or added to the jumplist when it is
    /// confirmed.
    pub last_selection: Option<Selection>,

    pub status_msg: Option<(Cow<'static, str>, Severity)>,
    /// Log of every status/error/warning shown, newest last — backs `:messages`
    /// (vim) / the emacs `*Messages*` buffer. Capped to the most recent entries.
    pub messages: Vec<(Cow<'static, str>, Severity)>,
    pub autoinfo: Option<Info>,
    /// A pending external-`fzf` request (fzf.vim `:Files`/`:Colors`/`:Maps`/…).
    /// A command fills this; the terminal layer (which owns the TTY) drains it,
    /// hands the terminal to `fzf` with `candidates` on stdin, then runs `sink`
    /// (a zemacs `:` command line with `{}` replaced by the picked line).
    pub pending_fzf: Option<FzfRequest>,
    /// A full-screen external command (argv) to run with the tty handed over —
    /// used by image-dired to show images via a terminal image viewer. Like
    /// `pending_fzf`, the app leaves the TUI, runs it, and re-enters afterwards.
    pub pending_tty_command: Option<Vec<String>>,

    /// Latest in-flight LSP `$/progress` work (indexing, building, etc.), mirrored
    /// here by the event loop so UI surfaces (e.g. the IDE workbench gauge) can
    /// render it. `None` when no server is currently progressing.
    pub lsp_progress: Option<LspProgress>,

    /// Variables of the active debug stack frame as `(name, value)`, fetched when
    /// the debugger stops so the IDE Debug tool window can render them without an
    /// async round-trip. Cleared when the debug session ends.
    pub dap_variables: Vec<(String, String)>,

    pub config: Arc<dyn DynAccess<Config>>,
    pub auto_pairs: Option<AutoPairs>,

    pub idle_timer: Pin<Box<Sleep>>,
    redraw_timer: Pin<Box<Sleep>>,
    last_motion: Option<Motion>,
    /// Last `f`/`t`/`F`/`T` find: `(char, inclusive, forward)`. Lets `,` repeat
    /// it in the opposite direction (vim reverse find-repeat).
    pub last_find: Option<(char, bool, bool)>,
    pub last_completion: Option<CompleteAction>,
    pub last_cwd: Option<PathBuf>,
    pub dir_stack: VecDeque<PathBuf>,

    pub exit_code: i32,

    pub config_events: (UnboundedSender<ConfigEvent>, UnboundedReceiver<ConfigEvent>),
    pub needs_redraw: bool,
    /// Cached position of the cursor calculated during rendering.
    /// The content of `cursor_cache` is returned by `Editor::cursor` if
    /// set to `Some(_)`. The value will be cleared after it's used.
    /// If `cursor_cache` is `None` then the `Editor::cursor` function will
    /// calculate the cursor position.
    ///
    /// `Some(None)` represents a cursor position outside of the visible area.
    /// This will just cause `Editor::cursor` to return `None`.
    ///
    /// This cache is only a performance optimization to
    /// avoid calculating the cursor position multiple
    /// times during rendering and should not be set by other functions.
    pub handlers: Handlers,

    pub mouse_down_range: Option<Range>,
    pub cursor_cache: CursorCache,
    pub workspace_trust: WorkspaceTrust,
}

pub type Motion = Box<dyn Fn(&mut Editor)>;

#[derive(Debug)]
pub enum EditorEvent {
    DocumentSaved(DocumentSavedEventResult),
    ConfigEvent(ConfigEvent),
    LanguageServerMessage((LanguageServerId, Call)),
    DebuggerEvent((DebugAdapterId, dap::Payload)),
    IdleTimer,
    Redraw,
}

#[derive(Debug, Clone)]
pub enum ConfigEvent {
    Refresh,
    Update(Box<Config>),
    ThemeChanged,
    /// Switch the active keymap preset at runtime (e.g. from `:keymap emacs`).
    /// Carries the preset name; the terminal layer rebuilds the keymap and sets
    /// the appropriate mode (it owns the keymap preset registry).
    SetKeymap(String),
    /// Re-merge the runtime `:map` overlay (from vimscript `:map`/init.vim/
    /// plugins) onto the live `config.keys`. The terminal layer owns the overlay
    /// registry (`keymap::vim_map`) and the keymap, so it applies it.
    ApplyUserMappings,
}

enum ThemeAction {
    Set,
    Preview,
}

#[derive(Debug, Clone)]
pub enum CompleteAction {
    Triggered,
    /// A savepoint of the currently selected completion. The savepoint
    /// MUST be restored before sending any event to the LSP
    Selected {
        savepoint: Arc<SavePoint>,
    },
    Applied {
        trigger_offset: usize,
        changes: Vec<Change>,
        placeholder: bool,
    },
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Action {
    Load,
    Replace,
    HorizontalSplit,
    VerticalSplit,
}

/// Window dedication (Spacemacs `SPC w t`): a dedicated window keeps its buffer.
/// Replacing it with a *different* document is redirected to a split so the
/// dedicated buffer stays put. Other actions (and re-opening the same buffer)
/// pass through unchanged. Pure decision helper for [`Editor::switch`].
pub(crate) fn dedication_redirect(
    action: Action,
    view_dedicated: bool,
    same_document: bool,
) -> Action {
    if action == Action::Replace && view_dedicated && !same_document {
        Action::HorizontalSplit
    } else {
        action
    }
}

/// Follow-mode layout. The vertically-ordered windows showing one document act
/// as a single tall window: window `i` is anchored at `group_top + sum(heights[..i])`
/// so they tile one continuous view. `group_top` is the group's current scroll
/// (the top window's line); it is nudged just enough to keep the focused window's
/// `point_line` visible within that window's slice. Returns each window's new top
/// line, clamped to `[0, last_line]`.
pub(crate) fn follow_anchor_lines(
    heights: &[usize],
    focus_idx: usize,
    point_line: usize,
    group_top: usize,
    last_line: usize,
) -> Vec<usize> {
    let n = heights.len();
    if n == 0 {
        return Vec::new();
    }
    let fi = focus_idx.min(n - 1);
    let cum_before: usize = heights[..fi].iter().sum();
    let fwin_h = heights[fi].max(1);

    // Adjust the group scroll so point stays inside the focused window's slice.
    let fwin_top = group_top + cum_before;
    let mut gt = group_top;
    if point_line < fwin_top {
        gt = point_line.saturating_sub(cum_before);
    } else if point_line >= fwin_top + fwin_h {
        gt = (point_line + 1).saturating_sub(cum_before + fwin_h);
    }

    let mut out = Vec::with_capacity(n);
    let mut acc = gt;
    for &h in heights {
        out.push(acc.min(last_line));
        acc = acc.saturating_add(h);
    }
    out
}

impl Action {
    /// Whether to align the view to the cursor after executing this action
    pub fn align_view(&self, view: &View, new_doc: DocumentId) -> bool {
        !matches!((self, view.doc == new_doc), (Action::Load, false))
    }
}

/// Error thrown on failed document closed
pub enum CloseError {
    /// Document doesn't exist
    DoesNotExist,
    /// Buffer is modified
    BufferModified(String),
    /// Document failed to save
    SaveError(anyhow::Error),
}

impl Editor {
    pub fn new(
        mut area: Rect,
        theme_loader: Arc<theme::Loader>,
        syn_loader: Arc<ArcSwap<syntax::Loader>>,
        config: Arc<dyn DynAccess<Config>>,
        handlers: Handlers,
        workspace_trust: WorkspaceTrust,
    ) -> Self {
        let language_servers = zemacs_lsp::Registry::new(syn_loader.clone());
        let conf = config.load();
        let auto_pairs = (&conf.auto_pairs).into();

        // HAXX: offset the render area height by 1 to account for prompt/commandline
        area.height -= 1;

        Self {
            mode: Mode::Normal,
            vim_semantics: true,
            last_search_forward: true,
            last_positions: std::collections::HashMap::new(),
            overwrite: false,
            abbrev_mode: false,
            digraph_pending: None,
            insert_oneshot: false,
            show_match: None,
            virtualedit: String::new(),
            equalalways: false,
            tabpagemax: 0,
            exit_transient: false,
            text_scale: 0,
            frame_scale: 0,
            block: None,
            visual_line: None,
            subword: false,
            superword: false,
            auto_fill: false,
            picture_mode: false,
            picture_dir: zemacs_core::picture::Dir::E,
            picture_tab_stops: Vec::new(),
            follow: false,
            fill_prefix: None,
            goal_column: None,
            tree: Tree::new(area),
            next_document_id: DocumentId::default(),
            documents: BTreeMap::new(),
            saves: HashMap::new(),
            save_queue: SelectAll::new(),
            write_count: 0,
            count: None,
            selected_register: None,
            last_substitute: None,
            macro_recording: None,
            macro_replaying: Vec::new(),
            last_macro_register: None,
            last_keys: std::collections::VecDeque::new(),
            theme: theme_loader.default(),
            language_servers,
            diagnostics: Diagnostics::new(),
            diff_providers: DiffProviderRegistry::default(),
            debug_adapters: dap::registry::Registry::new(),
            global_marks: HashMap::new(),
            last_inserted_text: String::new(),
            breakpoints: HashMap::new(),
            quickfix: Vec::new(),
            quickfix_idx: None,
            quickfix_stack: Vec::new(),
            quickfix_stack_pos: 0,
            tabs: Vec::new(),
            current_tab: 0,
            closed_tabs: Vec::new(),
            tab_back: Vec::new(),
            tab_forward: Vec::new(),
            tab_history_mode: true,
            syn_loader,
            theme_loader,
            last_theme: None,
            last_selection: None,
            registers: Registers::new(Box::new(arc_swap::access::Map::new(
                Arc::clone(&config),
                |config: &Config| &config.clipboard_provider,
            ))),
            status_msg: None,
            messages: Vec::new(),
            autoinfo: None,
            pending_fzf: None,
            pending_tty_command: None,
            lsp_progress: None,
            dap_variables: Vec::new(),
            idle_timer: Box::pin(sleep(conf.idle_timeout)),
            redraw_timer: Box::pin(sleep(Duration::MAX)),
            last_motion: None,
            last_find: None,
            last_completion: None,
            last_cwd: None,
            config,
            auto_pairs,
            exit_code: 0,
            config_events: unbounded_channel(),
            needs_redraw: false,
            handlers,
            mouse_down_range: None,
            cursor_cache: CursorCache::default(),
            dir_stack: VecDeque::with_capacity(DIR_STACK_CAP),
            workspace_trust,
        }
    }

    pub fn popup_border(&self) -> bool {
        self.config().popup_border == PopupBorderConfig::All
            || self.config().popup_border == PopupBorderConfig::Popup
    }

    pub fn menu_border(&self) -> bool {
        self.config().popup_border == PopupBorderConfig::All
            || self.config().popup_border == PopupBorderConfig::Menu
    }

    pub fn apply_motion<F: Fn(&mut Self) + 'static>(&mut self, motion: F) {
        motion(self);
        self.last_motion = Some(Box::new(motion));
    }

    pub fn repeat_last_motion(&mut self, count: usize) {
        if let Some(motion) = self.last_motion.take() {
            for _ in 0..count {
                motion(self);
            }
            self.last_motion = Some(motion);
        }
    }
    /// Current editing mode for the [`Editor`].
    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn config(&self) -> DynGuard<Config> {
        self.config.load()
    }

    /// Call if the config has changed to let the editor update all
    /// relevant members.
    pub fn refresh_config(&mut self, old_config: &Config) {
        let config = self.config();
        self.auto_pairs = (&config.auto_pairs).into();
        self.reset_idle_timer();
        self._refresh();
        zemacs_event::dispatch(crate::events::ConfigDidChange {
            editor: self,
            old: old_config,
            new: &config,
        })
    }

    pub fn clear_idle_timer(&mut self) {
        // equivalent to internal Instant::far_future() (30 years)
        self.idle_timer
            .as_mut()
            .reset(Instant::now() + Duration::from_secs(86400 * 365 * 30));
    }

    pub fn reset_idle_timer(&mut self) {
        let config = self.config();
        self.idle_timer
            .as_mut()
            .reset(Instant::now() + config.idle_timeout);
    }

    pub fn clear_status(&mut self) {
        self.status_msg = None;
    }

    #[inline]
    /// Append a message to the `:messages` log, capping it to the most recent
    /// entries so a long session never grows the ring without bound.
    fn log_message(&mut self, msg: Cow<'static, str>, severity: Severity) {
        const MAX_MESSAGES: usize = 1000;
        self.messages.push((msg, severity));
        if self.messages.len() > MAX_MESSAGES {
            self.messages.drain(..self.messages.len() - MAX_MESSAGES);
        }
    }

    pub fn set_status<T: Into<Cow<'static, str>>>(&mut self, status: T) {
        let status = status.into();
        log::debug!("editor status: {}", status);
        self.log_message(status.clone(), Severity::Info);
        self.status_msg = Some((status, Severity::Info));
    }

    #[inline]
    pub fn set_error<T: Into<Cow<'static, str>>>(&mut self, error: T) {
        let error = error.into();
        log::debug!("editor error: {}", error);
        self.log_message(error.clone(), Severity::Error);
        self.status_msg = Some((error, Severity::Error));
    }

    #[inline]
    pub fn set_warning<T: Into<Cow<'static, str>>>(&mut self, warning: T) {
        let warning = warning.into();
        log::warn!("editor warning: {}", warning);
        self.log_message(warning.clone(), Severity::Warning);
        self.status_msg = Some((warning, Severity::Warning));
    }

    #[inline]
    pub fn get_status(&self) -> Option<(&Cow<'static, str>, &Severity)> {
        self.status_msg.as_ref().map(|(status, sev)| (status, sev))
    }

    /// Returns true if the current status is an error
    #[inline]
    pub fn is_err(&self) -> bool {
        self.status_msg
            .as_ref()
            .map(|(_, sev)| *sev == Severity::Error)
            .unwrap_or(false)
    }

    pub fn unset_theme_preview(&mut self) -> anyhow::Result<()> {
        if let Some(last_theme) = self.last_theme.take() {
            self.set_theme(last_theme)?;
        }
        // None likely occurs when the user types ":theme" and then exits before previewing
        Ok(())
    }

    pub fn set_theme_preview(&mut self, theme: Theme) -> anyhow::Result<()> {
        self.set_theme_impl(theme, ThemeAction::Preview)
    }

    pub fn set_theme(&mut self, theme: Theme) -> anyhow::Result<()> {
        self.set_theme_impl(theme, ThemeAction::Set)
    }

    fn set_theme_impl(&mut self, theme: Theme, preview: ThemeAction) -> anyhow::Result<()> {
        // `ui.selection` is the only scope required to be able to render a theme.
        if theme.find_highlight_exact("ui.selection").is_none() {
            bail!("Invalid theme: `ui.selection` required");
        }

        let scopes = theme.scopes();
        (*self.syn_loader).load().set_scopes(scopes.to_vec());

        match preview {
            ThemeAction::Preview => {
                let last_theme = std::mem::replace(&mut self.theme, theme);
                // only insert on first preview: this will be the last theme the user has saved
                self.last_theme.get_or_insert(last_theme);
            }
            ThemeAction::Set => {
                self.last_theme = None;
                self.theme = theme;
            }
        }

        self._refresh();
        self.config_events.0.send(ConfigEvent::ThemeChanged)?;

        Ok(())
    }

    #[inline]
    pub fn language_server_by_id(
        &self,
        language_server_id: LanguageServerId,
    ) -> Option<&zemacs_lsp::Client> {
        self.language_servers
            .get_by_id(language_server_id)
            .map(|client| &**client)
    }

    /// Refreshes the language server for a given document
    pub fn refresh_language_servers(&mut self, doc_id: DocumentId) {
        self.launch_language_servers(doc_id)
    }

    /// moves/renames a path, invoking any event handlers (currently only lsp)
    /// and calling `set_doc_path` if the file is open in the editor
    pub fn move_path(&mut self, old_path: &Path, new_path: &Path) -> io::Result<()> {
        let new_path = canonicalize(new_path);
        // sanity check
        if old_path == new_path {
            return Ok(());
        }
        let is_dir = old_path.is_dir();
        let language_servers: Vec<_> = self
            .language_servers
            .iter_clients()
            .filter(|client| client.is_initialized())
            .cloned()
            .collect();
        for language_server in language_servers {
            let Some(request) = language_server.will_rename(old_path, &new_path, is_dir) else {
                continue;
            };
            let edit = match zemacs_lsp::block_on(request) {
                Ok(edit) => edit.unwrap_or_default(),
                Err(err) => {
                    log::error!("invalid willRename response: {err:?}");
                    continue;
                }
            };
            if let Err(err) = self.apply_workspace_edit(language_server.offset_encoding(), &edit) {
                log::error!("failed to apply workspace edit: {err:?}")
            }
        }

        if old_path.exists() {
            fs::rename(old_path, &new_path)?;
        }

        if let Some(doc) = self.document_by_path(old_path) {
            self.set_doc_path(doc.id(), &new_path);
        }
        let is_dir = new_path.is_dir();
        for ls in self.language_servers.iter_clients() {
            // A new language server might have been started in `set_doc_path` and won't
            // be initialized yet. Skip the `did_rename` notification for this server.
            if !ls.is_initialized() {
                continue;
            }
            ls.did_rename(old_path, &new_path, is_dir);
        }
        self.language_servers
            .file_event_handler
            .file_changed(old_path.to_owned());
        self.language_servers
            .file_event_handler
            .file_changed(new_path);
        Ok(())
    }

    pub fn create_path(&mut self, path: &Path, is_dir: bool) -> io::Result<()> {
        let path = canonicalize(path);
        let language_servers: Vec<_> = self
            .language_servers
            .iter_clients()
            .filter(|client| client.is_initialized())
            .cloned()
            .collect();
        for language_server in language_servers {
            let Some(request) = language_server.will_create(&path, is_dir) else {
                continue;
            };
            let edit = match zemacs_lsp::block_on(request) {
                Ok(edit) => edit.unwrap_or_default(),
                Err(err) => {
                    log::error!("invalid willCreate response: {err:?}");
                    continue;
                }
            };
            if let Err(err) = self.apply_workspace_edit(language_server.offset_encoding(), &edit) {
                log::error!("failed to apply workspace edit: {err:?}")
            }
        }

        if let Some(dir) = path.parent() {
            if !dir.is_dir() {
                fs::create_dir_all(dir)?;
            }
        }
        if is_dir {
            fs::create_dir(&path)?;
        } else {
            fs::write(&path, [])?;
        }

        for ls in self.language_servers.iter_clients() {
            if !ls.is_initialized() {
                continue;
            }
            ls.did_create(&path, is_dir);
        }
        self.language_servers.file_event_handler.file_changed(path);
        Ok(())
    }

    pub fn delete_path(&mut self, path: &Path, recursive: bool) -> io::Result<()> {
        let path = canonicalize(path);
        let is_dir = path.is_dir();
        let language_servers: Vec<_> = self
            .language_servers
            .iter_clients()
            .filter(|client| client.is_initialized())
            .cloned()
            .collect();
        for language_server in language_servers {
            let Some(request) = language_server.will_delete(&path, is_dir) else {
                continue;
            };
            let edit = match zemacs_lsp::block_on(request) {
                Ok(edit) => edit.unwrap_or_default(),
                Err(err) => {
                    log::error!("invalid willDelete response: {err:?}");
                    continue;
                }
            };
            if let Err(err) = self.apply_workspace_edit(language_server.offset_encoding(), &edit) {
                log::error!("failed to apply workspace edit: {err:?}")
            }
        }

        if is_dir {
            if recursive {
                fs::remove_dir_all(&path)?;
            } else {
                fs::remove_dir(&path)?;
            }
        } else {
            fs::remove_file(&path)?;
        }

        for ls in self.language_servers.iter_clients() {
            if !ls.is_initialized() {
                continue;
            }
            ls.did_delete(&path, is_dir);
        }
        self.language_servers.file_event_handler.file_changed(path);
        Ok(())
    }

    pub fn set_doc_path(&mut self, doc_id: DocumentId, path: &Path) {
        let doc = doc_mut!(self, &doc_id);
        let old_path = doc.path();

        if let Some(old_path) = old_path {
            // sanity check, should not occur but some callers (like an LSP) may
            // create bogus calls
            if old_path == path {
                return;
            }
            // if we are open in LSPs send did_close notification
            for language_server in doc.language_servers() {
                language_server.text_document_did_close(doc.identifier());
            }
        }
        // we need to clear the list of language servers here so that
        // refresh_doc_language/refresh_language_servers doesn't resend
        // text_document_did_close. Since we called `text_document_did_close`
        // we have fully unregistered this document from its LS
        doc.language_servers.clear();
        doc.set_path(Some(path));
        doc.detect_editor_config();
        self.refresh_doc_language(doc_id)
    }

    pub fn refresh_doc_language(&mut self, doc_id: DocumentId) {
        let loader = self.syn_loader.load();
        let doc = doc_mut!(self, &doc_id);
        doc.detect_language(&loader);
        doc.detect_editor_config();
        doc.detect_indent_and_line_ending();
        self.refresh_language_servers(doc_id);
        let doc = doc_mut!(self, &doc_id);
        let diagnostics = Editor::doc_diagnostics(&self.language_servers, &self.diagnostics, doc);
        doc.replace_diagnostics(diagnostics, &[], None);
        doc.reset_all_inlay_hints();
    }

    /// Launch a language server for a given document
    pub fn launch_language_servers(&mut self, doc_id: DocumentId) {
        if !self.config().lsp.enable {
            return;
        }
        // if doc doesn't have a URL it's a scratch buffer, ignore it
        let Some(doc) = self.documents.get_mut(&doc_id) else {
            return;
        };
        let Some(doc_url) = doc.url() else {
            return;
        };
        let (lang, path) = (doc.language.clone(), doc.path());
        let config = doc.config.load();
        let root_dirs = &config.workspace_lsp_roots;

        let workspace = doc.workspace_root();
        let trust = self.workspace_trust.query(workspace, TrustQuery::Lsp);
        if !trust.is_trusted() {
            return;
        }

        // store only successfully started language servers
        let language_servers = lang.as_ref().map_or_else(HashMap::default, |language| {
            self.language_servers
                .get(language, path, root_dirs, config.lsp.snippets)
                .filter_map(|(lang, client)| match client {
                    Ok(client) => Some((lang, client)),
                    Err(err) => {
                        if let zemacs_lsp::Error::ExecutableNotFound(err) = err {
                            // Silence by default since some language servers might just not be installed
                            log::debug!(
                                "Language server not found for `{}` {} {}", language.scope, lang, err,
                            );
                        } else {
                            log::error!(
                                "Failed to initialize the language servers for `{}` - `{}` {{ {} }}",
                                language.scope,
                                lang,
                                err
                            );
                        }
                        None
                    }
                })
                .collect::<HashMap<_, _>>()
        });

        if language_servers.is_empty() && doc.language_servers.is_empty() {
            return;
        }

        let language_id = doc.language_id().map(ToOwned::to_owned).unwrap_or_default();

        // only spawn new language servers if the servers aren't the same
        let doc_language_servers_not_in_registry =
            doc.language_servers.iter().filter(|(name, doc_ls)| {
                language_servers
                    .get(*name)
                    .is_none_or(|ls| ls.id() != doc_ls.id())
            });

        for (_, language_server) in doc_language_servers_not_in_registry {
            language_server.text_document_did_close(doc.identifier());
        }

        let language_servers_not_in_doc = language_servers.iter().filter(|(name, ls)| {
            doc.language_servers
                .get(*name)
                .is_none_or(|doc_ls| ls.id() != doc_ls.id())
        });

        for (_, language_server) in language_servers_not_in_doc {
            // TODO: this now races with on_init code if the init happens too quickly
            language_server.text_document_did_open(
                doc_url.clone(),
                doc.version(),
                doc.text(),
                language_id.clone(),
            );
        }

        doc.language_servers = language_servers;
    }

    fn _refresh(&mut self) {
        let config = self.config();

        // Reset the inlay hints annotations *before* updating the views, that way we ensure they
        // will disappear during the `.sync_change(doc)` call below.
        //
        // We can't simply check this config when rendering because inlay hints are only parts of
        // the possible annotations, and others could still be active, so we need to selectively
        // drop the inlay hints.
        if !config.lsp.display_inlay_hints {
            for doc in self.documents_mut() {
                doc.reset_all_inlay_hints();
            }
        }

        for (view, _) in self.tree.views_mut() {
            let doc = doc_mut!(self, &view.doc);
            view.sync_changes(doc);
            view.gutters = config.gutters.clone();
            view.ensure_cursor_in_view(doc, config.scrolloff)
        }
    }

    fn replace_document_in_view(&mut self, current_view: ViewId, doc_id: DocumentId) {
        let scrolloff = self.config().scrolloff;
        let view = self.tree.get_mut(current_view);

        view.doc = doc_id;
        let doc = doc_mut!(self, &doc_id);

        doc.ensure_view_init(view.id);
        view.sync_changes(doc);
        doc.mark_as_focused();

        view.ensure_cursor_in_view(doc, scrolloff)
    }

    pub fn switch(&mut self, id: DocumentId, action: Action) {
        use crate::tree::Layout;

        if !self.documents.contains_key(&id) {
            log::error!("cannot switch to document that does not exist (anymore)");
            return;
        }

        if !matches!(action, Action::Load) {
            self.enter_normal_mode();
        }

        // vim `"` mark: remember the cursor position when leaving the current buffer.
        // Guarded: on the very first open there is no current view yet.
        if self.tree.try_get(self.tree.focus).is_some() {
            let leave = {
                let (view, doc) = current_ref!(self);
                (
                    doc.id(),
                    doc.selection(view.id)
                        .primary()
                        .cursor(doc.text().slice(..)),
                )
            };
            if leave.0 != id {
                if let Some(doc) = self.documents.get_mut(&leave.0) {
                    doc.set_mark('"', leave.1);
                }
            }
        }

        // Window dedication (Spacemacs `SPC w t`): a dedicated window keeps its
        // buffer; replacing it with a different document is redirected to a split.
        let action = match self.tree.try_get(self.tree.focus) {
            Some(view) => dedication_redirect(action, view.dedicated, view.doc == id),
            None => action,
        };

        let focust_lost = match action {
            Action::Replace => {
                let (view, doc) = current_ref!(self);
                // If the current view is an empty scratch buffer and is not displayed in any other views, delete it.
                // Boolean value is determined before the call to `view_mut` because the operation requires a borrow
                // of `self.tree`, which is mutably borrowed when `view_mut` is called.
                let remove_empty_scratch = !doc.is_modified()
                    // If the buffer has no path and is not modified, it is an empty scratch buffer.
                    && doc.path().is_none()
                    // If the buffer we are changing to is not this buffer
                    && id != doc.id
                    // Ensure the buffer is not displayed in any other splits.
                    && !self
                        .tree
                        .traverse()
                        .any(|(_, v)| v.doc == doc.id && v.id != view.id);

                let (view, doc) = current!(self);
                let view_id = view.id;

                // Append any outstanding changes to history in the old document.
                doc.append_changes_to_history(view);

                if remove_empty_scratch {
                    // Copy `doc.id` into a variable before calling `self.documents.remove`, which requires a mutable
                    // borrow, invalidating direct access to `doc.id`.
                    let id = doc.id;
                    self.documents.remove(&id);

                    // Remove the scratch buffer from any jumplists
                    for (view, _) in self.tree.views_mut() {
                        view.remove_document(&id);
                    }
                } else {
                    let jump = (view.doc, doc.selection(view.id).clone());
                    view.push_jump(doc, jump);
                    // Set last accessed doc if it is a different document
                    if doc.id != id {
                        view.add_to_history(view.doc);
                        // Set last modified doc if modified and last modified doc is different
                        if std::mem::take(&mut doc.modified_since_accessed)
                            && view.last_modified_docs[0] != Some(view.doc)
                        {
                            view.last_modified_docs = [Some(view.doc), view.last_modified_docs[0]];
                        }
                    }
                }

                self.replace_document_in_view(view_id, id);

                dispatch(DocumentFocusLost {
                    editor: self,
                    doc: id,
                });
                return;
            }
            Action::Load => {
                // Load into the current view — but on the very first open (e.g. restoring
                // a session into a fresh editor) there is no view yet. Create one instead
                // of dereferencing a nonexistent focus (which panicked in `tree.get`).
                let view_id = match self.tree.try_get(self.tree.focus) {
                    Some(view) => view.id,
                    None => {
                        let view = View::new(id, self.config().gutters.clone());
                        self.tree.split(view, Layout::Vertical)
                    }
                };
                let doc = doc_mut!(self, &id);
                doc.ensure_view_init(view_id);
                doc.mark_as_focused();
                return;
            }
            Action::HorizontalSplit | Action::VerticalSplit => {
                let focus_lost = self.tree.try_get(self.tree.focus).map(|view| view.doc);
                // copy the current view, unless there is no view yet
                let view = self
                    .tree
                    .try_get(self.tree.focus)
                    .filter(|v| id == v.doc) // Different Document
                    .cloned()
                    .unwrap_or_else(|| View::new(id, self.config().gutters.clone()));
                let (layout, before) = match action {
                    // vim `nosplitbelow` puts the new horizontal split above.
                    Action::HorizontalSplit => (Layout::Horizontal, !self.config().split_below),
                    // vim `nosplitright` puts the new vertical split to the left.
                    Action::VerticalSplit => (Layout::Vertical, !self.config().split_right),
                    _ => unreachable!(),
                };
                let view_id = self.tree.split_with(view, layout, before);
                // vim `equalalways`: every split re-balances all windows, undoing
                // any manual resize. Off by default, so a resized layout survives.
                if self.equalalways {
                    self.tree.equalize();
                }
                // initialize selection for view
                let doc = doc_mut!(self, &id);
                doc.ensure_view_init(view_id);
                doc.mark_as_focused();
                focus_lost
            }
        };

        self._refresh();
        if let Some(focus_lost) = focust_lost {
            dispatch(DocumentFocusLost {
                editor: self,
                doc: focus_lost,
            });
        }
    }

    /// Generate an id for a new document and register it.
    fn new_document(&mut self, mut doc: Document) -> DocumentId {
        let id = self.next_document_id;
        // Safety: adding 1 from 1 is fine, practically impossible to reach usize max
        self.next_document_id =
            DocumentId(unsafe { NonZeroUsize::new_unchecked(self.next_document_id.0.get() + 1) });
        doc.id = id;
        self.documents.insert(id, doc);

        let (save_sender, save_receiver) = tokio::sync::mpsc::unbounded_channel();
        self.saves.insert(id, save_sender);

        let stream = UnboundedReceiverStream::new(save_receiver).flatten();
        self.save_queue.push(stream);

        id
    }

    fn new_file_from_document(&mut self, action: Action, doc: Document) -> DocumentId {
        let id = self.new_document(doc);
        self.switch(id, action);
        id
    }

    pub fn new_file(&mut self, action: Action) -> DocumentId {
        self.new_file_from_document(
            action,
            Document::default(self.config.clone(), self.syn_loader.clone()),
        )
    }

    pub fn new_file_from_stdin(&mut self, action: Action) -> Result<DocumentId, Error> {
        let (stdin, encoding, has_bom) = crate::document::read_to_string(&mut stdin(), None)?;
        let doc = Document::from(
            zemacs_core::Rope::default(),
            Some((encoding, has_bom)),
            self.config.clone(),
            self.syn_loader.clone(),
        );
        let doc_id = self.new_file_from_document(action, doc);
        let doc = doc_mut!(self, &doc_id);
        let view = view_mut!(self);
        doc.ensure_view_init(view.id);
        let transaction =
            zemacs_core::Transaction::insert(doc.text(), doc.selection(view.id), stdin.into())
                .with_selection(Selection::point(0));
        doc.apply(&transaction, view.id);
        doc.append_changes_to_history(view);
        Ok(doc_id)
    }

    pub fn document_id_by_path(&self, path: &Path) -> Option<DocumentId> {
        self.document_by_path(path).map(|doc| doc.id)
    }

    /// Reload `path`'s buffer from disk after the file changed outside the editor
    /// (vim `autoread`), driven by the filesystem watcher. No-ops unless the
    /// `auto-reload` setting is on and the file genuinely changed on disk (not
    /// the editor's own save). A buffer with unsaved edits is never clobbered:
    /// it's kept and a warning is shown so the user can `:reload` to discard.
    /// Returns whether a reload happened.
    pub fn auto_reload_file(&mut self, path: &Path) -> bool {
        if !self.config().auto_reload {
            return false;
        }
        let Some(doc_id) = self.document_id_by_path(path) else {
            return false;
        };

        // Snapshot what we need, then drop the borrow before mutating `self`.
        let (changed_on_disk, modified, name, view_ids) = {
            let Some(doc) = self.documents.get(&doc_id) else {
                return false;
            };
            (
                doc.is_changed_on_disk(),
                doc.is_modified(),
                doc.display_name().into_owned(),
                doc.selections().keys().copied().collect::<Vec<_>>(),
            )
        };

        // Only react to real external edits — ignore our own (auto)saves.
        if !changed_on_disk {
            return false;
        }
        if modified {
            self.set_error(format!(
                "{name} changed on disk but the buffer has unsaved edits — kept your version (:reload to discard)"
            ));
            return false;
        }
        let Some(&first_view) = view_ids.first() else {
            return false;
        };

        let trust_full = self
            .workspace_trust
            .query(self.documents[&doc_id].workspace_root(), TrustQuery::Git)
            .is_trusted();

        {
            let doc = self.documents.get_mut(&doc_id).unwrap();
            let view = self.tree.get_mut(first_view);
            view.sync_changes(doc);
            if let Err(err) = doc.reload(view, &self.diff_providers, trust_full) {
                self.set_error(format!("auto-reload failed: {err}"));
                return false;
            }
        }

        // Keep any other views onto this document in sync with the reloaded text
        // so their jumplist selections don't reference the pre-reload buffer.
        for &view_id in &view_ids[1..] {
            let doc = self.documents.get_mut(&doc_id).unwrap();
            let view = self.tree.get_mut(view_id);
            view.sync_changes(doc);
        }

        if let Some(path) = self.documents[&doc_id].path().map(ToOwned::to_owned) {
            self.language_servers.file_event_handler.file_changed(path);
        }
        true
    }

    // ??? possible use for integration tests
    pub fn open(&mut self, path: &Path, action: Action) -> Result<DocumentId, DocumentOpenError> {
        let path = zemacs_stdx::path::canonicalize(path);
        let id = self.document_id_by_path(&path);

        let id = if let Some(id) = id {
            id
        } else {
            let mut doc = Document::open(
                &path,
                None,
                true,
                self.config.clone(),
                self.syn_loader.clone(),
            )?;

            let diagnostics =
                Editor::doc_diagnostics(&self.language_servers, &self.diagnostics, &doc);
            doc.replace_diagnostics(diagnostics, &[], None);

            let trust_full = self
                .workspace_trust
                .query(doc.workspace_root(), TrustQuery::Git)
                .is_trusted();
            if let Some(diff_base) = self.diff_providers.get_diff_base(&path, trust_full) {
                doc.set_diff_base(diff_base);
            }
            doc.set_version_control_head(
                self.diff_providers.get_current_head_name(&path, trust_full),
            );

            // vim `` `" ``: restore the cursor to where it was when this file was
            // last closed (consumed once when the doc is first shown in a view).
            // Stored as line/col; resolve against this buffer's text now.
            if let Some(&(line, col)) = self.last_positions.get(&path) {
                let text = doc.text().slice(..);
                let line = line.min(text.len_lines().saturating_sub(1));
                doc.restore_position = Some(zemacs_core::pos_at_coords(
                    text,
                    zemacs_core::Position::new(line, col),
                    true,
                ));
            }

            let id = self.new_document(doc);
            self.launch_language_servers(id);

            zemacs_event::dispatch(DocumentDidOpen {
                editor: self,
                doc: id,
            });

            id
        };

        self.switch(id, action);

        Ok(id)
    }

    pub fn close(&mut self, id: ViewId) {
        // Remove selections for the closed view on all documents.
        for doc in self.documents_mut() {
            doc.remove_view(id);
        }
        self.tree.remove(id);
        self._refresh();
    }

    pub fn close_document(&mut self, doc_id: DocumentId, force: bool) -> Result<(), CloseError> {
        let doc = match self.documents.get(&doc_id) {
            Some(doc) => doc,
            None => return Err(CloseError::DoesNotExist),
        };
        if !force && doc.is_modified() {
            return Err(CloseError::BufferModified(doc.display_name().into_owned()));
        }

        // vim `` `" ``: remember the cursor position for this file so a later reopen
        // restores it. Read the document's own stored selection (its view may no
        // longer be focused on it — e.g. after switching away then closing it).
        let recorded = self.documents.get(&doc_id).and_then(|doc| {
            let path = doc.path()?;
            let sel = doc.selections().values().next()?;
            let text = doc.text().slice(..);
            let coords = zemacs_core::coords_at_pos(text, sel.primary().cursor(text));
            Some((
                zemacs_stdx::path::canonicalize(path),
                (coords.row, coords.col),
            ))
        });
        if let Some((path, coords)) = recorded {
            self.last_positions.insert(path, coords);
        }

        // This will also disallow any follow-up writes
        self.saves.remove(&doc_id);

        enum Action {
            Close(ViewId),
            ReplaceDoc(ViewId, DocumentId),
        }

        let actions: Vec<Action> = self
            .tree
            .views_mut()
            .filter_map(|(view, _focus)| {
                view.remove_document(&doc_id);

                if view.doc == doc_id {
                    // something was previously open in the view, switch to previous doc
                    if let Some(prev_doc) = view.docs_access_history.pop() {
                        Some(Action::ReplaceDoc(view.id, prev_doc))
                    } else {
                        // only the document that is being closed was in the view, close it
                        Some(Action::Close(view.id))
                    }
                } else {
                    None
                }
            })
            .collect();

        for action in actions {
            match action {
                Action::Close(view_id) => {
                    self.close(view_id);
                }
                Action::ReplaceDoc(view_id, doc_id) => {
                    self.replace_document_in_view(view_id, doc_id);
                }
            }
        }

        let doc = self.documents.remove(&doc_id).unwrap();

        // If the document we removed was visible in all views, we will have no more views. We don't
        // want to close the editor just for a simple buffer close, so we need to create a new view
        // containing either an existing document, or a brand new document.
        if self.tree.views().next().is_none() {
            let doc_id = self
                .documents
                .iter()
                .map(|(&doc_id, _)| doc_id)
                .next()
                .unwrap_or_else(|| {
                    self.new_document(Document::default(
                        self.config.clone(),
                        self.syn_loader.clone(),
                    ))
                });
            let view = View::new(doc_id, self.config().gutters.clone());
            let view_id = self.tree.insert(view);
            let doc = doc_mut!(self, &doc_id);
            doc.ensure_view_init(view_id);
            doc.mark_as_focused();
        }

        self._refresh();

        zemacs_event::dispatch(DocumentDidClose { editor: self, doc });

        Ok(())
    }

    pub fn save<P: Into<PathBuf>>(
        &mut self,
        doc_id: DocumentId,
        path: Option<P>,
        force: bool,
    ) -> anyhow::Result<()> {
        // convert a channel of futures to pipe into main queue one by one
        // via stream.then() ? then push into main future

        let path = path.map(|path| path.into());
        let doc = doc_mut!(self, &doc_id);
        let doc_save_future = doc.save(path, force)?;

        // When a file is written to, notify the file event handler.
        // Note: This can be removed once proper file watching is implemented.
        let handler = self.language_servers.file_event_handler.clone();
        let future = async move {
            let res = doc_save_future.await;
            if let Ok(event) = &res {
                handler.file_changed(event.path.clone());
            }
            res
        };

        use futures_util::stream;

        self.saves
            .get(&doc_id)
            .ok_or_else(|| anyhow::format_err!("saves are closed for this document!"))?
            .send(stream::once(Box::pin(future)))
            .map_err(|err| anyhow!("failed to send save event: {}", err))?;

        self.write_count += 1;

        Ok(())
    }

    pub fn resize(&mut self, area: Rect) {
        if self.tree.resize(area) {
            self._refresh();
        };
    }

    /// Follow-mode (`SPC w f`): re-anchor sibling windows showing the focused
    /// document so the group scrolls as one continuous view. No-op unless
    /// `follow` is set or there are <2 windows on the doc.
    pub fn sync_follow_windows(&mut self) {
        if !self.follow {
            return;
        }
        let focus = self.tree.focus;
        let doc_id = match self.tree.try_get(focus) {
            Some(v) => v.doc,
            None => return,
        };
        let mut group: Vec<(u16, ViewId, usize)> = self
            .tree
            .views()
            .filter(|(v, _)| v.doc == doc_id)
            .map(|(v, _)| (v.area.y, v.id, v.inner_height()))
            .collect();
        if group.len() < 2 {
            return;
        }
        group.sort_by_key(|&(y, _, _)| y);
        let focus_idx = group
            .iter()
            .position(|&(_, id, _)| id == focus)
            .unwrap_or(0);
        let heights: Vec<usize> = group.iter().map(|&(_, _, h)| h).collect();

        // Compute each window's new anchor (char pos) under an immutable borrow,
        // then apply under a mutable borrow.
        let offsets: Vec<(ViewId, usize)> = {
            let doc = match self.documents.get(&doc_id) {
                Some(d) => d,
                None => return,
            };
            let text = doc.text().slice(..);
            let last_line = text.len_lines().saturating_sub(1);
            // Group scroll = the top window's current anchor line.
            let group_top =
                text.char_to_line(doc.view_offset(group[0].1).anchor.min(text.len_chars()));
            // Focused window's point line (keeps it visible within its slice).
            let point = doc.selection(focus).primary().cursor(text);
            let point_line = text.char_to_line(point);
            let lines = follow_anchor_lines(&heights, focus_idx, point_line, group_top, last_line);
            group
                .iter()
                .zip(lines.iter())
                .map(|(&(_, vid, _), &line)| (vid, text.line_to_char(line.min(last_line))))
                .collect()
        };
        let doc = self.documents.get_mut(&doc_id).unwrap();
        for (vid, anchor) in offsets {
            let mut off = doc.view_offset(vid);
            off.anchor = anchor;
            doc.set_view_offset(vid, off);
        }
    }

    pub fn focus(&mut self, view_id: ViewId) {
        if self.tree.focus == view_id {
            return;
        }

        // Reset mode to normal and ensure any pending changes are committed in the old document.
        self.enter_normal_mode();
        let (view, doc) = current!(self);
        doc.append_changes_to_history(view);
        self.ensure_cursor_in_view(view_id);
        // Update jumplist selections with new document changes.
        for (view, _focused) in self.tree.views_mut() {
            let doc = doc_mut!(self, &view.doc);
            view.sync_changes(doc);
        }

        let prev_id = std::mem::replace(&mut self.tree.focus, view_id);
        doc_mut!(self).mark_as_focused();

        let focus_lost = self.tree.get(prev_id).doc;
        dispatch(DocumentFocusLost {
            editor: self,
            doc: focus_lost,
        });
    }

    pub fn focus_next(&mut self) {
        self.focus(self.tree.next());
    }

    pub fn focus_prev(&mut self) {
        self.focus(self.tree.prev());
    }

    pub fn focus_direction(&mut self, direction: tree::Direction) {
        let current_view = self.tree.focus;
        if let Some(id) = self.tree.find_split_in_direction(current_view, direction) {
            self.focus(id)
        }
    }

    pub fn swap_split_in_direction(&mut self, direction: tree::Direction) {
        self.tree.swap_split_in_direction(direction);
    }

    pub fn transpose_view(&mut self) {
        self.tree.transpose();
    }

    pub fn should_close(&self) -> bool {
        self.tree.is_empty()
    }

    // --- Tabpages (vim `gt`/`gT`/`:tabnew`/`:tabnext`/…) -------------------
    //
    // Only one tab's window tree is live (in `self.tree`) at a time; the rest
    // are parked as `TabPage` snapshots. Switching snapshots the live tree,
    // drops its per-window document state, then rebuilds the target tab with
    // fresh (collision-free) ViewIds and restores its selections. Because all
    // tabs draw from the one shared `documents` map, buffers are shared.

    /// How many tabpages exist (always at least 1).
    pub fn tab_count(&self) -> usize {
        self.tabs.len().max(1)
    }

    /// The 0-based index of the active tab.
    pub fn current_tab(&self) -> usize {
        self.current_tab
    }

    /// Flash the active tab position in the statusline (tabs have no dedicated
    /// bar yet, so this is the user's feedback that a switch happened).
    fn report_tab(&mut self) {
        let (i, n) = (self.current_tab + 1, self.tab_count());
        if n > 1 {
            self.set_status(format!("tab {i}/{n}"));
        }
    }

    /// Snapshot the live window tree (layout + each window's selection).
    fn snapshot_current_tab(&self) -> TabPage {
        let shape = self.tree.shape();
        let selections = self
            .tree
            .leaf_ids()
            .into_iter()
            .map(|vid| {
                let doc_id = self.tree.get(vid).doc;
                self.documents
                    .get(&doc_id)
                    .map(|d| d.selection(vid).clone())
                    .unwrap_or_else(|| Selection::point(0))
            })
            .collect();
        // Carry the current tab's user-assigned name across the snapshot.
        let name = self.tabs.get(self.current_tab).and_then(|t| t.name.clone());
        TabPage {
            shape,
            selections,
            name,
        }
    }

    /// Forget every per-window document entry for the views currently in the
    /// live tree (called before rebuilding so stale low-index ViewIds can't
    /// alias the freshly-minted ones).
    fn drop_live_view_state(&mut self) {
        for vid in self.tree.leaf_ids() {
            for doc in self.documents.values_mut() {
                doc.remove_view(vid);
            }
        }
    }

    /// Rebuild the live tree from a parked tab and restore its selections.
    fn restore_tab(&mut self, tab: &TabPage) {
        let gutters = self.config().gutters.clone();
        let mut make = |doc| View::new(doc, gutters.clone());
        let new_ids = self.tree.build_from_shape(&tab.shape, &mut make);
        for (vid, sel) in new_ids.iter().zip(tab.selections.iter()) {
            let doc_id = self.tree.get(*vid).doc;
            if let Some(doc) = self.documents.get_mut(&doc_id) {
                doc.ensure_view_init(*vid);
                // Clamp the saved selection to the (possibly changed) buffer.
                let sel = sel.clone().ensure_invariants(doc.text().slice(..));
                doc.set_selection(*vid, sel);
            }
        }
        let focus = self.tree.focus;
        self.ensure_cursor_in_view(focus);
    }

    /// Make `self.tabs` have one slot per tab, seeding slot 0 from the live
    /// tree on first use.
    fn ensure_tabs_initialized(&mut self) {
        if self.tabs.is_empty() {
            self.tabs.push(self.snapshot_current_tab());
            self.current_tab = 0;
        }
    }

    /// Switch to tab `to` (0-based, clamped). No-op if it's already active.
    /// Records the departed tab in the back-history (emacs `tab-bar-history`).
    pub fn switch_tab(&mut self, to: usize) {
        self.switch_tab_inner(to, true);
    }

    /// Shared tab switch. `record` pushes the departed tab onto the history
    /// back-stack (and clears the forward-stack); the history navigation methods
    /// pass `false` so replaying history does not itself rewrite the history.
    fn switch_tab_inner(&mut self, to: usize, record: bool) {
        self.ensure_tabs_initialized();
        let to = to.min(self.tabs.len() - 1);
        if to == self.current_tab {
            return;
        }
        if record && self.tab_history_mode {
            self.tab_back.push(self.current_tab);
            self.tab_forward.clear();
        }
        self.tabs[self.current_tab] = self.snapshot_current_tab();
        self.drop_live_view_state();
        let target = self.tabs[to].clone();
        self.restore_tab(&target);
        self.current_tab = to;
        self.report_tab();
    }

    /// emacs `tab-bar-history-back`: return to the previously visited tab.
    /// Returns whether a move happened.
    pub fn tab_history_back(&mut self) -> bool {
        self.ensure_tabs_initialized();
        while let Some(to) = self.tab_back.pop() {
            let to = to.min(self.tabs.len() - 1);
            if to != self.current_tab {
                self.tab_forward.push(self.current_tab);
                self.switch_tab_inner(to, false);
                return true;
            }
        }
        false
    }

    /// emacs `tab-bar-history-forward`: re-visit a tab left via history-back.
    pub fn tab_history_forward(&mut self) -> bool {
        self.ensure_tabs_initialized();
        while let Some(to) = self.tab_forward.pop() {
            let to = to.min(self.tabs.len() - 1);
            if to != self.current_tab {
                self.tab_back.push(self.current_tab);
                self.switch_tab_inner(to, false);
                return true;
            }
        }
        false
    }

    /// emacs `tab-rename`: set (or clear, with `None`) the current tab's name.
    pub fn rename_current_tab(&mut self, name: Option<String>) {
        self.ensure_tabs_initialized();
        self.tabs[self.current_tab].name = name;
    }

    /// The current tab's user-assigned name, if any.
    pub fn current_tab_name(&self) -> Option<&str> {
        self.tabs
            .get(self.current_tab)
            .and_then(|t| t.name.as_deref())
    }

    /// `(index, name)` for every tab — backs `tab-switch`'s name lookup.
    pub fn tab_names(&self) -> Vec<(usize, Option<String>)> {
        self.tabs
            .iter()
            .enumerate()
            .map(|(i, t)| (i, t.name.clone()))
            .collect()
    }

    /// emacs `tab-undo`: reopen the most recently closed tab (inserted after the
    /// current one and focused). Returns whether a tab was reopened.
    pub fn reopen_closed_tab(&mut self) -> bool {
        self.ensure_tabs_initialized();
        let Some(tab) = self.closed_tabs.pop() else {
            return false;
        };
        self.tabs[self.current_tab] = self.snapshot_current_tab();
        self.drop_live_view_state();
        let idx = self.current_tab + 1;
        self.tabs.insert(idx, tab);
        let target = self.tabs[idx].clone();
        self.restore_tab(&target);
        self.current_tab = idx;
        self.report_tab();
        true
    }

    /// `gt` / `:tabnext`: go to the next tab (wraps).
    pub fn goto_next_tabpage(&mut self) {
        self.ensure_tabs_initialized();
        let next = (self.current_tab + 1) % self.tabs.len();
        self.switch_tab(next);
    }

    /// `gT` / `:tabprevious`: go to the previous tab (wraps).
    pub fn goto_previous_tabpage(&mut self) {
        self.ensure_tabs_initialized();
        let prev = (self.current_tab + self.tabs.len() - 1) % self.tabs.len();
        self.switch_tab(prev);
    }

    /// True when vim `tabpagemax` would be exceeded by opening another tab.
    fn tabpagemax_reached(&mut self) -> bool {
        self.ensure_tabs_initialized();
        if self.tabpagemax > 0 && self.tabs.len() >= self.tabpagemax {
            let max = self.tabpagemax;
            self.set_error(format!("tabpagemax ({max}) reached"));
            return true;
        }
        false
    }

    /// `:tabnew`: open a new tab after the current one with a single window on
    /// a fresh scratch buffer, and focus it.
    pub fn new_tab(&mut self) {
        if self.tabpagemax_reached() {
            return;
        }
        self.ensure_tabs_initialized();
        self.tabs[self.current_tab] = self.snapshot_current_tab();
        self.drop_live_view_state();
        let doc_id = self.new_document(Document::default(
            self.config.clone(),
            self.syn_loader.clone(),
        ));
        let new = TabPage {
            shape: crate::tree::TreeShape::Leaf {
                doc: doc_id,
                focused: true,
            },
            selections: vec![Selection::point(0)],
            name: None,
        };
        let idx = self.current_tab + 1;
        self.tabs.insert(idx, new);
        let target = self.tabs[idx].clone();
        self.restore_tab(&target);
        self.current_tab = idx;
        self.report_tab();
    }

    /// `:tabnew {path}` / `:tabedit`: open a tab whose single window shows the
    /// given (already-opened) document.
    pub fn new_tab_with_doc(&mut self, doc_id: DocumentId) {
        if self.tabpagemax_reached() {
            return;
        }
        self.ensure_tabs_initialized();
        self.tabs[self.current_tab] = self.snapshot_current_tab();
        self.drop_live_view_state();
        let new = TabPage {
            shape: crate::tree::TreeShape::Leaf {
                doc: doc_id,
                focused: true,
            },
            selections: vec![Selection::point(0)],
            name: None,
        };
        let idx = self.current_tab + 1;
        self.tabs.insert(idx, new);
        let target = self.tabs[idx].clone();
        self.restore_tab(&target);
        self.current_tab = idx;
    }

    /// `:tabclose`: close the current tab (refuses to close the last one).
    pub fn close_tab(&mut self) {
        self.ensure_tabs_initialized();
        if self.tabs.len() <= 1 {
            self.set_error("cannot close last tab");
            return;
        }
        // Capture the live layout before removing so `tab-undo` can restore it.
        self.tabs[self.current_tab] = self.snapshot_current_tab();
        self.drop_live_view_state();
        let closed = self.tabs.remove(self.current_tab);
        self.closed_tabs.push(closed);
        const MAX_CLOSED_TABS: usize = 32;
        if self.closed_tabs.len() > MAX_CLOSED_TABS {
            let excess = self.closed_tabs.len() - MAX_CLOSED_TABS;
            self.closed_tabs.drain(..excess);
        }
        let to = self.current_tab.min(self.tabs.len() - 1);
        let target = self.tabs[to].clone();
        self.restore_tab(&target);
        self.current_tab = to;
        self.report_tab();
    }

    /// `:tabonly`: close every tab except the current one.
    pub fn tab_only(&mut self) {
        self.ensure_tabs_initialized();
        if self.tabs.len() <= 1 {
            return;
        }
        let current = self.snapshot_current_tab();
        self.tabs = vec![current];
        self.current_tab = 0;
        self.set_status("only tab");
    }

    /// `:tabmove [N]`: move the current tab to position `to` (0-based, clamped).
    /// With no argument vim moves the tab to the end.
    pub fn move_current_tab(&mut self, to: usize) {
        self.ensure_tabs_initialized();
        self.tabs[self.current_tab] = self.snapshot_current_tab();
        let from = self.current_tab;
        let to = to.min(self.tabs.len() - 1);
        if to == from {
            return;
        }
        let tab = self.tabs.remove(from);
        self.tabs.insert(to, tab);
        self.current_tab = to;
        self.report_tab();
    }

    /// The focused document of each tab, in order (for `:tabs`). The active tab
    /// reads the live tree; parked tabs read their snapshot.
    pub fn tab_focused_docs(&self) -> Vec<DocumentId> {
        self.tabs
            .iter()
            .enumerate()
            .map(|(i, tab)| {
                if i == self.current_tab {
                    self.tree.get(self.tree.focus).doc
                } else {
                    tab_focused_doc(&tab.shape)
                }
            })
            .collect()
    }

    pub fn ensure_cursor_in_view(&mut self, id: ViewId) {
        let config = self.config();
        let view = self.tree.get(id);
        let doc = doc_mut!(self, &view.doc);
        view.ensure_cursor_in_view(doc, config.scrolloff)
    }

    #[inline]
    pub fn document(&self, id: DocumentId) -> Option<&Document> {
        self.documents.get(&id)
    }

    #[inline]
    pub fn document_mut(&mut self, id: DocumentId) -> Option<&mut Document> {
        self.documents.get_mut(&id)
    }

    #[inline]
    pub fn documents(&self) -> impl Iterator<Item = &Document> {
        self.documents.values()
    }

    #[inline]
    pub fn documents_mut(&mut self) -> impl Iterator<Item = &mut Document> {
        self.documents.values_mut()
    }

    pub fn document_by_path<P: AsRef<Path>>(&self, path: P) -> Option<&Document> {
        self.documents()
            .find(|doc| doc.path().is_some_and(|p| p == path.as_ref()))
    }

    pub fn document_by_path_mut<P: AsRef<Path>>(&mut self, path: P) -> Option<&mut Document> {
        self.documents_mut()
            .find(|doc| doc.path().is_some_and(|p| p == path.as_ref()))
    }

    /// Returns all supported diagnostics for the document
    pub fn doc_diagnostics<'a>(
        language_servers: &'a zemacs_lsp::Registry,
        diagnostics: &'a Diagnostics,
        document: &Document,
    ) -> impl Iterator<Item = zemacs_core::Diagnostic> + 'a {
        Editor::doc_diagnostics_with_filter(language_servers, diagnostics, document, |_, _| true)
    }

    /// Returns all supported diagnostics for the document
    /// filtered by `filter` which is invocated with the raw `lsp::Diagnostic` and the language server id it came from
    pub fn doc_diagnostics_with_filter<'a>(
        language_servers: &'a zemacs_lsp::Registry,
        diagnostics: &'a Diagnostics,
        document: &Document,
        filter: impl Fn(&lsp::Diagnostic, &DiagnosticProvider) -> bool + 'a,
    ) -> impl Iterator<Item = zemacs_core::Diagnostic> + 'a {
        let text = document.text().clone();
        let language_config = document.language.clone();
        document
            .uri()
            .and_then(|uri| diagnostics.get(&uri))
            .map(|diags| {
                diags.iter().filter_map(move |(diagnostic, provider)| {
                    let server_id = provider.language_server_id()?;
                    let ls = language_servers.get_by_id(server_id)?;
                    language_config
                        .as_ref()
                        .and_then(|c| {
                            c.language_servers.iter().find(|features| {
                                features.name == ls.name()
                                    && features.has_feature(LanguageServerFeature::Diagnostics)
                            })
                        })
                        .and_then(|_| {
                            if filter(diagnostic, provider) {
                                Document::lsp_diagnostic_to_diagnostic(
                                    &text,
                                    language_config.as_deref(),
                                    diagnostic,
                                    provider.clone(),
                                    ls.offset_encoding(),
                                )
                            } else {
                                None
                            }
                        })
                })
            })
            .into_iter()
            .flatten()
    }

    /// Gets the primary cursor position in screen coordinates,
    /// or `None` if the primary cursor is not visible on screen.
    pub fn cursor(&self) -> (Option<Position>, CursorKind) {
        let config = self.config();
        let (view, doc) = current_ref!(self);
        if let Some(mut pos) = self.cursor_cache.get(view, doc) {
            let inner = view.inner_area(doc);
            pos.col += inner.x as usize;
            pos.row += inner.y as usize;
            let cursorkind = config.cursor_shape.from_mode(self.mode);
            (Some(pos), cursorkind)
        } else {
            (None, CursorKind::default())
        }
    }

    /// Closes language servers with timeout. The default timeout is 10000 ms, use
    /// `timeout` parameter to override this.
    pub async fn close_language_servers(&self, timeout: Option<u64>) {
        // Remove all language servers from the file event handler.
        // Note: this is non-blocking.
        for client in self.language_servers.iter_clients() {
            self.language_servers
                .file_event_handler
                .remove_client(client.id());
        }

        // Enqueue shutdown+exit for every server (non-blocking fire-and-forget).
        for client in self.language_servers.iter_clients() {
            client.force_shutdown();
        }

        // Wait until shutdown+exit have actually been written to each server's stdin
        // before the runtime (and the pipes) are torn down, so well-behaved servers
        // can act on `exit` before kill_on_drop reaps them. This waits only on our
        // own outbound write -- not on any server response -- so a slow server (e.g.
        // gopls flushing logs) doesn't delay it. Capped so a wedged write can't hang
        // the quit.
        let cap = Duration::from_millis(timeout.unwrap_or(1000));
        let _ = tokio::time::timeout(cap, async {
            for client in self.language_servers.iter_clients() {
                client.wait_shutdown_flushed().await;
            }
        })
        .await;
    }

    pub async fn wait_event(&mut self) -> EditorEvent {
        // the loop only runs once or twice and would be better implemented with a recursion + const generic
        // however due to limitations with async functions that can not be implemented right now
        loop {
            tokio::select! {
                biased;

                Some(event) = self.save_queue.next() => {
                    self.write_count -= 1;
                    return EditorEvent::DocumentSaved(event)
                }
                Some(config_event) = self.config_events.1.recv() => {
                    return EditorEvent::ConfigEvent(config_event)
                }
                Some(message) = self.language_servers.incoming.next() => {
                    return EditorEvent::LanguageServerMessage(message)
                }
                Some(event) = self.debug_adapters.incoming.next() => {
                    return EditorEvent::DebuggerEvent(event)
                }

                _ = zemacs_event::redraw_requested() => {
                    if  !self.needs_redraw{
                        self.needs_redraw = true;
                        let timeout = Instant::now() + Duration::from_millis(33);
                        if timeout < self.idle_timer.deadline() && timeout < self.redraw_timer.deadline(){
                            self.redraw_timer.as_mut().reset(timeout)
                        }
                    }
                }

                _ = &mut self.redraw_timer  => {
                    self.redraw_timer.as_mut().reset(Instant::now() + Duration::from_secs(86400 * 365 * 30));
                    return EditorEvent::Redraw
                }
                _ = &mut self.idle_timer  => {
                    return EditorEvent::IdleTimer
                }
            }
        }
    }

    pub async fn flush_writes(&mut self) -> anyhow::Result<()> {
        while self.write_count > 0 {
            if let Some(save_event) = self.save_queue.next().await {
                self.write_count -= 1;

                let save_event = match save_event {
                    Ok(event) => event,
                    Err(err) => {
                        self.set_error(err.to_string());
                        bail!(err);
                    }
                };

                let doc = doc_mut!(self, &save_event.doc_id);
                doc.set_last_saved_revision(save_event.revision, save_event.save_time);
            }
        }

        Ok(())
    }

    /// Switches the editor into normal mode.
    pub fn enter_normal_mode(&mut self) {
        use zemacs_core::graphemes;

        // Replace mode is an insert-mode sub-state; always clear it on the way out.
        self.overwrite = false;
        // Visual-block and visual-line are Select sub-states; leaving to Normal
        // always ends them.
        self.block = None;
        self.visual_line = None;

        if self.mode == Mode::Normal {
            return;
        }

        self.mode = Mode::Normal;
        // vim `virtualedit=onemore`/`all`: the cursor may rest one past the last
        // character of a line, so leaving Insert must not pull it back.
        let onemore = self
            .virtualedit
            .split(',')
            .any(|v| matches!(v.trim(), "onemore" | "all"));
        let (view, doc) = current!(self);

        try_restore_indent(doc, view);

        if doc.restore_cursor && onemore {
            doc.restore_cursor = false;
        }
        // if leaving append mode, move cursor back by 1
        if doc.restore_cursor {
            let text = doc.text().slice(..);
            let selection = doc.selection(view.id).clone().transform(|range| {
                let mut head = range.to();
                if range.head > range.anchor {
                    head = graphemes::prev_grapheme_boundary(text, head);
                }

                Range::new(range.from(), head)
            });

            doc.set_selection(view.id, selection);
            doc.restore_cursor = false;
        }
    }

    pub fn current_stack_frame(&self) -> Option<&dap::StackFrame> {
        self.debug_adapters.current_stack_frame()
    }

    /// Returns the id of a view that this doc contains a selection for,
    /// making sure it is synced with the current changes
    /// if possible or there are no selections returns current_view
    /// otherwise uses an arbitrary view
    pub fn get_synced_view_id(&mut self, id: DocumentId) -> ViewId {
        let current_view = view_mut!(self);
        let doc = self.documents.get_mut(&id).unwrap();
        if doc.selections().contains_key(&current_view.id) {
            // only need to sync current view if this is not the current doc
            if current_view.doc != id {
                current_view.sync_changes(doc);
            }
            current_view.id
        } else if let Some(view_id) = doc.selections().keys().next() {
            let view_id = *view_id;
            let view = self.tree.get_mut(view_id);
            view.sync_changes(doc);
            view_id
        } else {
            doc.ensure_view_init(current_view.id);
            current_view.id
        }
    }

    pub fn set_cwd(&mut self, path: &Path) -> std::io::Result<()> {
        self.last_cwd = zemacs_stdx::env::set_current_working_dir(path)?;
        self.clear_doc_relative_paths();
        Ok(())
    }

    pub fn get_last_cwd(&mut self) -> Option<&Path> {
        self.last_cwd.as_deref()
    }

    pub fn jump_forward(&mut self, view_id: ViewId, count: usize) {
        if let Some((doc_id, selection)) = view_mut!(self, view_id).jumps.forward(count).cloned() {
            self.jump_to(view_id, doc_id, selection);
        }
    }

    pub fn jump_backward(&mut self, view_id: ViewId, count: usize) {
        let view = view_mut!(self, view_id);
        let doc = doc_mut!(self, &view.doc);
        // `backward` may push the current selection (valid at the document's
        // current revision) onto the jumplist. Sync first so the view's
        // `doc_revisions` matches, otherwise that entry would be left ahead of
        // it and a later sync would map it out of bounds.
        view.sync_changes(doc);
        if let Some((doc_id, selection)) = view.jumps.backward(view_id, doc, count).cloned() {
            self.jump_to(view_id, doc_id, selection);
        }
    }

    fn jump_to(&mut self, view_id: ViewId, dest_doc_id: DocumentId, mut selection: Selection) {
        let view = view_mut!(self, view_id);
        let old_doc_id = view.doc;
        if old_doc_id != dest_doc_id {
            let new_doc = doc_mut!(self, &dest_doc_id);
            if let Some(transaction) = view.changes_to_sync(new_doc) {
                let text = new_doc.text().slice(..);
                selection = selection.map(transaction.changes()).ensure_invariants(text);
            }
            self.replace_document_in_view(view_id, dest_doc_id);
            dispatch(DocumentFocusLost {
                editor: self,
                doc: old_doc_id,
            });
        }
        let (view, doc) = current!(self);
        doc.set_selection(view_id, selection);
        // vim jumplist navigation (Ctrl-O/Ctrl-I) scrolls minimally, not centered.
        view.ensure_cursor_in_view(doc, self.config.load().scrolloff);
    }
}

fn try_restore_indent(doc: &mut Document, view: &mut View) {
    use zemacs_core::{
        chars::char_is_whitespace,
        line_ending::{line_end_char_index, str_is_line_ending},
        unicode::segmentation::UnicodeSegmentation,
        Operation, Transaction,
    };

    fn inserted_a_new_blank_line(changes: &[Operation], pos: usize, line_end_pos: usize) -> bool {
        if let [Operation::Retain(move_pos), Operation::Insert(ref inserted_str), Operation::Retain(_)] =
            changes
        {
            let mut graphemes = inserted_str.graphemes(true);
            move_pos + inserted_str.len() == pos
                && graphemes.next().is_some_and(str_is_line_ending)
                && graphemes.all(|g| g.chars().all(char_is_whitespace))
                && pos == line_end_pos // ensure no characters exists after current position
        } else {
            false
        }
    }

    let doc_changes = doc.changes().changes();
    let text = doc.text().slice(..);
    let range = doc.selection(view.id).primary();
    let pos = range.cursor(text);
    let line_end_pos = line_end_char_index(&text, range.cursor_line(text));

    if inserted_a_new_blank_line(doc_changes, pos, line_end_pos) {
        // Removes tailing whitespaces for the primary selection only, preserving existing behavior
        let line_start_pos = text.line_to_char(range.cursor_line(text));
        let transaction =
            Transaction::change(doc.text(), [(line_start_pos, pos, None)].into_iter());
        doc.apply(&transaction, view.id);
    }
}

#[derive(Default)]
pub struct CursorCache(Cell<Option<Option<Position>>>);

impl CursorCache {
    pub fn get(&self, view: &View, doc: &Document) -> Option<Position> {
        if let Some(pos) = self.0.get() {
            return pos;
        }

        let text = doc.text().slice(..);
        let cursor = doc.selection(view.id).primary().cursor(text);
        let res = view.screen_coords_at_pos(doc, text, cursor);
        self.set(res);
        res
    }

    pub fn set(&self, cursor_pos: Option<Position>) {
        self.0.set(Some(cursor_pos))
    }

    pub fn reset(&self) {
        self.0.set(None)
    }
}

#[cfg(test)]
mod dedication_tests {
    use super::{dedication_redirect, follow_anchor_lines, Action};

    #[test]
    fn follow_anchor_chain() {
        // Windows tile one continuous view from the group top.
        assert_eq!(
            follow_anchor_lines(&[10, 10, 10], 0, 0, 0, 100),
            vec![0, 10, 20]
        );
        // Point past the focused (bottom) window's slice scrolls the group down
        // so point stays visible: focused window [16,26) contains line 25.
        assert_eq!(follow_anchor_lines(&[10, 10], 1, 25, 0, 100), vec![6, 16]);
        // Point above the focused window's slice scrolls the group up.
        assert_eq!(follow_anchor_lines(&[10, 10], 1, 12, 20, 100), vec![2, 12]);
        // Window tops are clamped to last_line.
        assert_eq!(follow_anchor_lines(&[10, 10], 0, 0, 0, 5), vec![0, 5]);
    }

    #[test]
    fn dedicated_window_redirects_replace_to_split() {
        // Replacing a dedicated window with a *different* doc splits instead.
        assert_eq!(
            dedication_redirect(Action::Replace, true, false),
            Action::HorizontalSplit
        );
        // Re-opening the *same* doc in a dedicated window does not split.
        assert_eq!(
            dedication_redirect(Action::Replace, true, true),
            Action::Replace
        );
        // A non-dedicated window replaces as usual.
        assert_eq!(
            dedication_redirect(Action::Replace, false, false),
            Action::Replace
        );
        // Non-Replace actions are never redirected, even when dedicated.
        assert_eq!(dedication_redirect(Action::Load, true, false), Action::Load);
        assert_eq!(
            dedication_redirect(Action::VerticalSplit, true, false),
            Action::VerticalSplit
        );
    }
}
