pub mod animate;
pub mod asteroids;
pub mod battleship;
pub mod blackbox;
pub mod blackjack;
pub mod bomberman;
pub mod bookmark;
pub mod breakout;
pub mod bubbles;
pub mod bufmenu;
pub mod calc;
pub mod calendar;
pub mod centipede;
pub mod checkers;
pub mod chess;
pub mod comint;
mod completion;
pub mod connectfour;
pub mod context_menu;
pub mod dashboard;
pub mod decipher;
pub mod diffmode;
pub mod digdug;
pub mod dired;
pub mod dissociate;
pub mod doctor;
mod document;
pub mod donkeykong;
pub mod dunnet;
pub mod dunnet_data;
pub(crate) mod editor;
pub mod ex_input;
pub mod facemenu;
pub mod fifteen;
mod file_tree;
pub mod fivex5;
pub mod flappy;
pub mod frogger;
pub mod galaga;
pub mod gomoku;
pub mod hangman;
pub mod hanoi;
pub mod help;
pub mod hex;
pub mod icons;
mod ide;
mod info;
pub mod invaders;
pub mod keymap_editor;
pub mod klondike;
pub mod kmacro_menu;
pub mod landmark;
pub mod life;
pub mod lsp;
pub mod lunarlander;
pub mod magit;
pub mod mancala;
mod markdown;
pub mod mastermind;
pub mod menu;
pub mod merge;
pub mod minesweeper;
pub mod missilecommand;
pub mod mpuz;
pub mod nonogram;
pub mod occur;
pub mod org_agenda;
pub mod overlay;
pub mod package_menu;
pub mod pacman;
pub mod picker;
pub mod picture;
pub mod pong;
pub mod popup;
pub mod preferences;
pub mod proced;
pub mod project;
pub mod prompt;
pub mod query_replace;
pub mod rat;
pub mod recentf;
pub mod repl;
pub mod reversi;
pub mod rmail;
pub mod run;
pub mod run_config;
pub mod search;
mod select;
pub mod serial_plotter;
pub mod settings;
pub mod simon;
pub mod snake;
pub mod snippets;
pub mod sokoban;
pub mod solitaire;
mod spinner;
pub mod spook_data;
pub mod startify;
mod statusline;
pub mod subst_confirm;
pub mod sudoku;
pub mod switcher;
pub mod table;
pub mod terminal;
pub mod tetris;
mod text;
mod text_decorations;
pub mod theme_editor;
pub mod tictactoe;
pub mod tron;
pub mod twentyfortyeight;
pub mod undotree;
pub mod videopoker;
pub mod wordle;
pub mod xref;
pub mod yahtzee;
pub mod zone;

use crate::compositor::Compositor;
use crate::filter_picker_entry;
use crate::job::{self, Callback};
pub use completion::Completion;
pub use editor::EditorView;
pub use ex_input::ExInput;
pub use markdown::Markdown;
pub use menu::Menu;
pub use picker::{Column as PickerColumn, FileLocation, Picker};
pub use popup::Popup;
pub use prompt::{Prompt, PromptEvent};
pub use select::Select;
pub use spinner::{ProgressSpinners, Spinner};
pub use startify::Startify;
pub use subst_confirm::SubstituteConfirm;
pub use text::Text;
use zmax_stdx::rope;
use zmax_view::theme::Style;

use tui::text::{Span, Spans};
use zmax_view::Editor;

use std::path::Path;
use std::{error::Error, path::PathBuf};

struct Utf8PathBuf {
    path: String,
    is_dir: bool,
    is_symlink: bool,
}

impl AsRef<str> for Utf8PathBuf {
    fn as_ref(&self) -> &str {
        &self.path
    }
}

pub fn prompt(
    cx: &mut crate::commands::Context,
    prompt: std::borrow::Cow<'static, str>,
    history_register: Option<char>,
    completion_fn: impl FnMut(&Editor, &str) -> Vec<prompt::Completion> + 'static,
    callback_fn: impl FnMut(&mut crate::compositor::Context, &str, PromptEvent) + 'static,
) {
    let mut prompt = Prompt::new(prompt, history_register, completion_fn, callback_fn);
    // Calculate the initial completion
    prompt.recalculate_completion(cx.editor);
    cx.push_layer(Box::new(prompt));
}

pub fn prompt_with_input(
    cx: &mut crate::commands::Context,
    prompt: std::borrow::Cow<'static, str>,
    input: String,
    history_register: Option<char>,
    completion_fn: impl FnMut(&Editor, &str) -> Vec<prompt::Completion> + 'static,
    callback_fn: impl FnMut(&mut crate::compositor::Context, &str, PromptEvent) + 'static,
) {
    let prompt = Prompt::new(prompt, history_register, completion_fn, callback_fn)
        .with_line(input, cx.editor);
    cx.push_layer(Box::new(prompt));
}

pub fn regex_prompt(
    cx: &mut crate::commands::Context,
    prompt: std::borrow::Cow<'static, str>,
    history_register: Option<char>,
    completion_fn: impl FnMut(&Editor, &str) -> Vec<prompt::Completion> + 'static,
    fun: impl Fn(&mut crate::compositor::Context, rope::Regex, PromptEvent) + 'static,
) {
    raw_regex_prompt(
        cx,
        prompt,
        history_register,
        false,
        None,
        None,
        completion_fn,
        move |cx, regex, _, event| fun(cx, regex, event),
    );
}
#[allow(clippy::type_complexity)] // on_cycle incsearch callback box
#[allow(clippy::too_many_arguments)] // one prompt, one knob per search behaviour
pub fn raw_regex_prompt(
    cx: &mut crate::commands::Context,
    prompt: std::borrow::Cow<'static, str>,
    history_register: Option<char>,
    // When true (and in a vim preset), a trailing `/{offset}` is stripped from the
    // input before the pattern is compiled — used only by `/`-search.
    search_offsets: bool,
    // Emacs isearch: `Some(forward)` makes this prompt an incremental search — the
    // isearch keys (`C-s`, `C-w`, `M-r`, the `M-s` toggle map, …) become live, and
    // `forward` is the direction it was started in. Used only by `/`/`?`-search.
    isearch: Option<bool>,
    // vim incsearch `C-g`/`C-t` cycle (next/prev match while typing); search only.
    on_cycle: Option<Box<dyn FnMut(&mut crate::compositor::Context, &str, bool)>>,
    completion_fn: impl FnMut(&Editor, &str) -> Vec<prompt::Completion> + 'static,
    fun: impl Fn(&mut crate::compositor::Context, rope::Regex, &str, PromptEvent) + 'static,
) {
    let (view, doc) = current!(cx.editor);
    let doc_id = view.doc;
    let view_id = view.id;
    let snapshot = doc.selection(view.id).clone();
    let offset_snapshot = doc.view_offset(view.id);
    let config = cx.editor.config();

    // vim incsearch `C-g`/`C-t`: while typing a search, these advance/retreat the
    // preview to the next/prev match. When the user commits (`Validate`) after
    // cycling, we keep the live cycled selection instead of re-running the search
    // from the origin (which would skip past the cycled match). The flag is reset
    // whenever the pattern text changes (an `Update`).
    let cycled = std::rc::Rc::new(std::cell::RefCell::new(false));
    let cycled_cb = cycled.clone();

    let mut prompt = Prompt::new(
        prompt,
        history_register,
        completion_fn,
        move |cx: &mut crate::compositor::Context, input: &str, event: PromptEvent| {
            match event {
                PromptEvent::Abort => {
                    let doc = doc_mut!(cx.editor, &doc_id);
                    let view = view_mut!(cx.editor, view_id);
                    doc.set_selection(view.id, snapshot.clone());
                    doc.set_view_offset(view.id, offset_snapshot);
                }
                PromptEvent::Update | PromptEvent::Validate => {
                    // skip empty input
                    if input.is_empty() {
                        return;
                    }

                    // A pattern edit invalidates any prior C-g/C-t cycling.
                    if event == PromptEvent::Update {
                        *cycled_cb.borrow_mut() = false;
                    } else if *cycled_cb.borrow() {
                        // Validate after cycling: commit the live cycled selection
                        // as-is; re-searching would skip past it.
                        let doc = doc_mut!(cx.editor, &doc_id);
                        let view = view_mut!(cx.editor, view_id);
                        view.push_jump(doc, (doc_id, snapshot.clone()));
                        let (view, doc) = current!(cx.editor);
                        view.ensure_cursor_in_view(doc, config.scrolloff);
                        return;
                    }

                    // vim search offset (`/pat/e`, `/pat/+2`): the pattern is only
                    // the part before the first unescaped `/`. The full input is
                    // still handed to `fun`, which applies the offset after the match.
                    let pattern = if search_offsets && cx.editor.vim_semantics {
                        crate::commands::split_search_offset(input).0
                    } else {
                        input
                    };
                    if pattern.is_empty() {
                        return;
                    }

                    let case_insensitive = if config.search.smart_case {
                        !pattern.chars().any(char::is_uppercase)
                    } else {
                        false
                    };

                    let is_crlf = doc!(cx.editor).line_ending == zmax_core::LineEnding::Crlf;
                    // Translate vim magic-regex syntax to the engine's syntax in
                    // vim/spacemacs presets (smart-case above already read the raw
                    // input, so this only affects the compiled pattern).
                    let search_re =
                        crate::vim_regex::search_pattern(cx.editor.vim_semantics, pattern);
                    match rope::RegexBuilder::new()
                        .syntax(
                            rope::Config::new()
                                .case_insensitive(case_insensitive)
                                .multi_line(true)
                                .crlf(is_crlf),
                        )
                        .build(search_re.as_ref())
                    {
                        Ok(regex) => {
                            let doc = doc_mut!(cx.editor, &doc_id);
                            let view = view_mut!(cx.editor, view_id);

                            // revert state to what it was before the last update
                            doc.set_selection(view.id, snapshot.clone());

                            if event == PromptEvent::Validate {
                                // Equivalent to push_jump to store selection just before jump
                                view.push_jump(doc, (doc_id, snapshot.clone()));
                            }

                            fun(cx, regex, input, event);

                            let (view, doc) = current!(cx.editor);
                            view.ensure_cursor_in_view(doc, config.scrolloff);
                        }
                        Err(err) => {
                            let doc = doc_mut!(cx.editor, &doc_id);
                            let view = view_mut!(cx.editor, view_id);
                            doc.set_selection(view.id, snapshot.clone());
                            doc.set_view_offset(view.id, offset_snapshot);

                            if event == PromptEvent::Validate {
                                let callback = async move {
                                    let call: job::Callback = Callback::EditorCompositor(Box::new(
                                        move |_editor: &mut Editor, compositor: &mut Compositor| {
                                            let contents = Text::new(format!("{}", err));
                                            let size = compositor.size();
                                            let popup = Popup::new("invalid-regex", contents)
                                                .position(Some(zmax_core::Position::new(
                                                    size.height as usize - 2, // 2 = statusline + commandline
                                                    0,
                                                )))
                                                .anchored(true)
                                                .auto_close(true);
                                            compositor.replace_or_push("invalid-regex", popup);
                                        },
                                    ));
                                    Ok(call)
                                };

                                cx.jobs.callback(callback);
                            }
                        }
                    }
                }
            }
        },
    )
    .with_language("regex", std::sync::Arc::clone(&cx.editor.syn_loader));
    if let Some(mut cycle) = on_cycle {
        // Run the caller's cycle (advances the live selection to the next/prev
        // match), then mark that the commit should keep this cycled selection.
        let cycled_cy = cycled.clone();
        let wrapped = move |cx: &mut crate::compositor::Context, line: &str, forward: bool| {
            cycle(cx, line, forward);
            *cycled_cy.borrow_mut() = true;
        };
        prompt = prompt.with_incsearch_cycle(Box::new(wrapped));
    }
    if let Some(forward) = isearch {
        prompt = prompt.with_isearch(forward);
    }
    // Calculate initial completion
    prompt.recalculate_completion(cx.editor);
    // prompt
    cx.push_layer(Box::new(prompt));
}

/// We want to exclude files that the editor can't handle yet
fn get_excluded_types() -> ignore::types::Types {
    use ignore::types::TypesBuilder;
    let mut type_builder = TypesBuilder::new();
    type_builder
        .add(
            "compressed",
            "*.{zip,gz,bz2,zst,lzo,sz,tgz,tbz2,lz,lz4,lzma,lzo,z,Z,xz,7z,rar,cab}",
        )
        .expect("Invalid type definition");
    type_builder.negate("all");
    type_builder
        .build()
        .expect("failed to build excluded_types")
}

#[derive(Debug)]
pub struct FilePickerData {
    root: PathBuf,
    directory_style: Style,
}
type FilePicker = Picker<PathBuf, FilePickerData>;

pub fn file_picker(editor: &Editor, root: PathBuf) -> FilePicker {
    use ignore::WalkBuilder;
    use std::time::Instant;

    let config = editor.config();
    let data = FilePickerData {
        root: root.clone(),
        directory_style: editor.theme.get("ui.text.directory"),
    };

    let now = Instant::now();

    let dedup_symlinks = config.file_picker.deduplicate_links;
    let absolute_root = root.canonicalize().unwrap_or_else(|_| root.clone());

    let mut walk_builder = WalkBuilder::new(&root);

    let mut files = walk_builder
        .hidden(config.file_picker.hidden)
        .parents(config.file_picker.parents)
        .ignore(config.file_picker.ignore)
        .follow_links(config.file_picker.follow_symlinks)
        .git_ignore(config.file_picker.git_ignore)
        .git_global(config.file_picker.git_global)
        .git_exclude(config.file_picker.git_exclude)
        .sort_by_file_name(|name1, name2| name1.cmp(name2))
        .max_depth(config.file_picker.max_depth)
        .filter_entry(move |entry| filter_picker_entry(entry, &absolute_root, dedup_symlinks))
        .add_custom_ignore_filename(zmax_loader::config_dir().join("ignore"))
        .add_custom_ignore_filename(".zmax/ignore")
        .types(get_excluded_types())
        .build()
        .filter_map(|entry| {
            let entry = entry.ok()?;
            if !entry.path().is_file() {
                return None;
            }
            Some(entry.into_path())
        });
    log::debug!("file_picker init {:?}", Instant::now().duration_since(now));

    let columns = [PickerColumn::new(
        "path",
        |item: &PathBuf, data: &FilePickerData| {
            let path = item.strip_prefix(&data.root).unwrap_or(item);
            let mut spans = Vec::with_capacity(3);
            if let Some(dirs) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
                spans.extend([
                    Span::styled(dirs.to_string_lossy(), data.directory_style),
                    Span::styled(std::path::MAIN_SEPARATOR_STR, data.directory_style),
                ]);
            }
            let filename = path
                .file_name()
                .expect("normalized paths can't end in `..`")
                .to_string_lossy();
            spans.push(Span::raw(filename));
            Spans::from(spans).into()
        },
    )];
    let picker = Picker::new(columns, 0, [], data, move |cx, path: &PathBuf, action| {
        if let Err(e) = cx.editor.open(path, action) {
            let err = if let Some(err) = e.source() {
                format!("{}", err)
            } else {
                format!("unable to open \"{}\"", path.display())
            };
            cx.editor.set_error(err);
        }
    })
    .with_preview(|_editor, path| Some((path.as_path().into(), None)));
    let injector = picker.injector();
    let timeout = std::time::Instant::now() + std::time::Duration::from_millis(30);

    let mut hit_timeout = false;
    for file in &mut files {
        if injector.push(file).is_err() {
            break;
        }
        if std::time::Instant::now() >= timeout {
            hit_timeout = true;
            break;
        }
    }
    if hit_timeout {
        std::thread::spawn(move || {
            for file in files {
                if injector.push(file).is_err() {
                    break;
                }
            }
        });
    }
    picker
}

type FileExplorer = Picker<(PathBuf, bool), (PathBuf, Style)>;

pub fn file_explorer(root: PathBuf, editor: &Editor) -> Result<FileExplorer, std::io::Error> {
    let directory_style = editor.theme.get("ui.text.directory");
    let directory_content = directory_content(&root, editor)?;

    let columns = [PickerColumn::new(
        "path",
        |(path, is_dir): &(PathBuf, bool), (root, directory_style): &(PathBuf, Style)| {
            let name = path.strip_prefix(root).unwrap_or(path).to_string_lossy();
            if *is_dir {
                Span::styled(format!("{}/", name), *directory_style).into()
            } else {
                name.into()
            }
        },
    )];
    let picker = Picker::new(
        columns,
        0,
        directory_content,
        (root, directory_style),
        move |cx, (path, is_dir): &(PathBuf, bool), action| {
            if *is_dir {
                let new_root = zmax_stdx::path::normalize(path);
                let callback = Box::pin(async move {
                    let call: Callback =
                        Callback::EditorCompositor(Box::new(move |editor, compositor| {
                            if let Ok(picker) = file_explorer(new_root, editor) {
                                compositor.push(Box::new(overlay::overlaid(picker)));
                            }
                        }));
                    Ok(call)
                });
                cx.jobs.callback(callback);
            } else if let Err(e) = cx.editor.open(path, action) {
                let err = if let Some(err) = e.source() {
                    format!("{}", err)
                } else {
                    format!("unable to open \"{}\"", path.display())
                };
                cx.editor.set_error(err);
            }
        },
    )
    .with_preview(|_editor, (path, _is_dir)| Some((path.as_path().into(), None)));

    Ok(picker)
}

fn directory_content(root: &Path, editor: &Editor) -> Result<Vec<(PathBuf, bool)>, std::io::Error> {
    use ignore::WalkBuilder;

    let config = editor.config();

    let mut walk_builder = WalkBuilder::new(root);

    let mut content: Vec<(PathBuf, bool)> = walk_builder
        .hidden(config.file_explorer.hidden)
        .parents(config.file_explorer.parents)
        .ignore(config.file_explorer.ignore)
        .follow_links(config.file_explorer.follow_symlinks)
        .git_ignore(config.file_explorer.git_ignore)
        .git_global(config.file_explorer.git_global)
        .git_exclude(config.file_explorer.git_exclude)
        .max_depth(Some(1))
        .add_custom_ignore_filename(zmax_loader::config_dir().join("ignore"))
        .add_custom_ignore_filename(".zmax/ignore")
        .types(get_excluded_types())
        .build()
        .filter_map(|entry| {
            entry
                .map(|entry| {
                    let path = entry.path();
                    let is_dir = path.is_dir();
                    let mut path = path.to_path_buf();
                    if is_dir && path != root && config.file_explorer.flatten_dirs {
                        while let Some(single_child_directory) = get_child_if_single_dir(&path) {
                            path = single_child_directory;
                        }
                    }
                    (path, is_dir)
                })
                .ok()
                .filter(|entry| entry.0 != root)
        })
        .collect();

    content.sort_by(|(path1, is_dir1), (path2, is_dir2)| (!is_dir1, path1).cmp(&(!is_dir2, path2)));

    if root.parent().is_some() {
        content.insert(0, (root.join(".."), true));
    }

    Ok(content)
}

fn get_child_if_single_dir(path: &Path) -> Option<PathBuf> {
    let mut entries = path.read_dir().ok()?;
    let entry = entries.next()?.ok()?;
    let entry_path = entry.path();
    if entries.next().is_none() && entry_path.is_dir() {
        Some(entry_path)
    } else {
        None
    }
}

/// vim `wildignore`: a comma-separated list of globs (`*.o,*.class,target/*`).
/// A file whose name — or whose path — matches any of them is never offered by
/// file-name completion. Pure — unit tested.
pub(crate) fn wildignored(wildignore: &str, path: &std::path::Path) -> bool {
    if wildignore.trim().is_empty() {
        return false;
    }
    let full = path.to_string_lossy();
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy())
        .unwrap_or_default();
    wildignore
        .split(',')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .any(|pat| {
            zmax_core::arglist::glob_match(pat, &name) || zmax_core::arglist::glob_match(pat, &full)
        })
}

pub mod completers {
    use super::Utf8PathBuf;
    use crate::ui::prompt::Completion;
    use once_cell::sync::Lazy;
    use std::borrow::Cow;
    use std::collections::BTreeSet;
    use tui::text::Span;
    use zmax_core::command_line::{self, Tokenizer};
    use zmax_core::fuzzy::fuzzy_match;
    use zmax_core::syntax::config::LanguageServerFeature;
    use zmax_view::document::SCRATCH_BUFFER_NAME;
    use zmax_view::theme;
    use zmax_view::{editor::Config, Editor};

    pub type Completer = fn(&Editor, &str) -> Vec<Completion>;

    /// Whether a candidate matches what was typed by vim's rules: a prefix match,
    /// case-folded when vim `wildignorecase` says so. Pure — unit tested.
    pub(crate) fn wild_prefix_match(candidate: &str, pattern: &str, ignorecase: bool) -> bool {
        if ignorecase {
            candidate
                .to_lowercase()
                .starts_with(&pattern.to_lowercase())
        } else {
            candidate.starts_with(pattern)
        }
    }

    /// Match command-line completion candidates against what was typed, honoring
    /// vim `wildignorecase` and `wildoptions`.
    ///
    /// zmax matches fuzzily, smart-case (an uppercase letter in the pattern
    /// makes it case-sensitive). vim matches by prefix, and only fuzzily when
    /// `wildoptions` contains `fuzzy` — so setting `wildoptions` at all switches
    /// to vim's prefix matching unless it asks for fuzzy, and `wildignorecase`
    /// folds case in either mode (`:e SRC/` finding `src/`).
    fn wild_match<T: AsRef<str>>(
        pattern: &str,
        items: impl IntoIterator<Item = T>,
        path: bool,
    ) -> Vec<(T, u16)> {
        let ignorecase = crate::commands::vim_opt_bool("wildignorecase");
        let fuzzy = match crate::commands::typed::vim_opt_str("wildoptions") {
            Some(opts) => opts.split(',').any(|o| o.trim() == "fuzzy"),
            None => true, // zmax's own default
        };
        if fuzzy {
            // The matcher is smart-case: an all-lowercase pattern already ignores
            // case, which is exactly what `wildignorecase` asks for.
            let pattern = if ignorecase {
                Cow::Owned(pattern.to_lowercase())
            } else {
                Cow::Borrowed(pattern)
            };
            return fuzzy_match(&pattern, items, path);
        }
        items
            .into_iter()
            .filter(|item| wild_prefix_match(item.as_ref(), pattern, ignorecase))
            .map(|item| (item, 0))
            .collect()
    }

    pub fn none(_editor: &Editor, _input: &str) -> Vec<Completion> {
        Vec::new()
    }

    pub fn buffer(editor: &Editor, input: &str) -> Vec<Completion> {
        let names = editor.documents.values().map(|doc| {
            doc.relative_path()
                .map(|p| p.display().to_string().into())
                .unwrap_or_else(|| Cow::from(SCRATCH_BUFFER_NAME))
        });

        fuzzy_match(input, names, true)
            .into_iter()
            .map(|(name, _)| ((0..), name.into()))
            .collect()
    }

    pub fn theme(_editor: &Editor, input: &str) -> Vec<Completion> {
        let mut names = theme::Loader::read_names(&zmax_loader::config_dir().join("themes"));
        for rt_dir in zmax_loader::runtime_dirs() {
            names.extend(theme::Loader::read_names(&rt_dir.join("themes")));
        }
        names.push("default".into());
        names.push("base16_default".into());
        names.sort();
        names.dedup();

        fuzzy_match(input, names, false)
            .into_iter()
            .map(|(name, _)| ((0..), name.into()))
            .collect()
    }

    /// Recursive function to get all keys from this value and add them to vec
    fn get_keys(value: &serde_json::Value, vec: &mut Vec<String>, scope: Option<&str>) {
        if let Some(map) = value.as_object() {
            for (key, value) in map.iter() {
                let key = match scope {
                    Some(scope) => format!("{}.{}", scope, key),
                    None => key.clone(),
                };
                get_keys(value, vec, Some(&key));
                if !value.is_object() {
                    vec.push(key);
                }
            }
        }
    }

    /// Completes names of language servers which are running for the current document.
    pub fn active_language_servers(editor: &Editor, input: &str) -> Vec<Completion> {
        let language_servers = doc!(editor).language_servers().map(|ls| ls.name());

        fuzzy_match(input, language_servers, false)
            .into_iter()
            .map(|(name, _)| ((0..), Span::raw(name.to_string())))
            .collect()
    }

    /// Completes names of language servers which are configured for the language of the current
    /// document.
    pub fn configured_language_servers(editor: &Editor, input: &str) -> Vec<Completion> {
        let language_servers = doc!(editor)
            .language_config()
            .into_iter()
            .flat_map(|config| &config.language_servers)
            .map(|ls| ls.name.as_str());

        fuzzy_match(input, language_servers, false)
            .into_iter()
            .map(|(name, _)| ((0..), Span::raw(name.to_string())))
            .collect()
    }

    pub fn setting(_editor: &Editor, input: &str) -> Vec<Completion> {
        static KEYS: Lazy<Vec<String>> = Lazy::new(|| {
            let mut keys = Vec::new();
            let json = serde_json::json!(Config::default());
            get_keys(&json, &mut keys, None);
            // Vim option names (and `no…` for booleans) so `:set number<tab>`,
            // `:set nonu<tab>`, `:set expandtab<tab>`, … complete.
            for (name, is_bool, _) in crate::commands::vim_options_data::VIM_OPTION_TABLE {
                keys.push((*name).to_string());
                if *is_bool {
                    keys.push(format!("no{name}"));
                }
            }
            keys.sort();
            keys.dedup();
            keys
        });

        fuzzy_match(input, &*KEYS, false)
            .into_iter()
            .map(|(name, _)| ((0..), Span::raw(name)))
            .collect()
    }

    pub fn filename(editor: &Editor, input: &str) -> Vec<Completion> {
        filename_with_git_ignore(editor, input, true)
    }

    pub fn filename_with_git_ignore(
        editor: &Editor,
        input: &str,
        git_ignore: bool,
    ) -> Vec<Completion> {
        filename_impl(editor, input, git_ignore, |entry| {
            if entry.path().is_dir() {
                FileMatch::AcceptIncomplete
            } else {
                FileMatch::Accept
            }
        })
    }

    pub fn language(editor: &Editor, input: &str) -> Vec<Completion> {
        let text: String = "text".into();

        let loader = editor.syn_loader.load();
        let language_ids = loader
            .language_configs()
            .map(|config| &config.language_id)
            .chain(std::iter::once(&text));

        fuzzy_match(input, language_ids, false)
            .into_iter()
            .map(|(name, _)| ((0..), name.to_owned().into()))
            .collect()
    }

    pub fn lsp_workspace_command(editor: &Editor, input: &str) -> Vec<Completion> {
        let commands = doc!(editor)
            .language_servers_with_feature(LanguageServerFeature::WorkspaceCommand)
            .flat_map(|ls| {
                ls.capabilities()
                    .execute_command_provider
                    .iter()
                    .flat_map(|options| options.commands.iter())
            });

        fuzzy_match(input, commands, false)
            .into_iter()
            .map(|(name, _)| ((0..), name.to_owned().into()))
            .collect()
    }

    pub fn directory(editor: &Editor, input: &str) -> Vec<Completion> {
        directory_with_git_ignore(editor, input, true)
    }

    pub fn directory_with_git_ignore(
        editor: &Editor,
        input: &str,
        git_ignore: bool,
    ) -> Vec<Completion> {
        filename_impl(editor, input, git_ignore, |entry| {
            if entry.path().is_dir() {
                FileMatch::Accept
            } else {
                FileMatch::Reject
            }
        })
    }

    #[derive(Copy, Clone, PartialEq, Eq)]
    enum FileMatch {
        /// Entry should be ignored
        Reject,
        /// Entry is usable but can't be the end (for instance if the entry is a directory and we
        /// try to match a file)
        AcceptIncomplete,
        /// Entry is usable and can be the end of the match
        Accept,
    }

    // TODO: we could return an iter/lazy thing so it can fetch as many as it needs.
    fn filename_impl<F>(
        editor: &Editor,
        input: &str,
        git_ignore: bool,
        filter_fn: F,
    ) -> Vec<Completion>
    where
        F: Fn(&ignore::DirEntry) -> FileMatch,
    {
        // Rust's filename handling is really annoying.

        use ignore::WalkBuilder;
        use std::path::Path;

        let is_tilde = input == "~";
        let path = zmax_stdx::path::expand_tilde(Path::new(input));

        let (dir, file_name) = if input.ends_with(std::path::MAIN_SEPARATOR) {
            (path, None)
        } else {
            let is_period = (input.ends_with((format!("{}.", std::path::MAIN_SEPARATOR)).as_str())
                && input.len() > 2)
                || input == ".";
            let file_name = if is_period {
                Some(String::from("."))
            } else {
                path.file_name()
                    .and_then(|file| file.to_str().map(|path| path.to_owned()))
            };

            let path = if is_period {
                path
            } else {
                match path.parent() {
                    Some(path) if !path.as_os_str().is_empty() => Cow::Borrowed(path),
                    // Path::new("h")'s parent is Some("")...
                    _ => Cow::Owned(zmax_stdx::env::current_working_dir()),
                }
            };

            (path, file_name)
        };

        let end = input.len()..;

        // vim `wildignore`: globs whose matches never show up in file completion.
        let wildignore = crate::commands::vim_opt_str("wildignore").unwrap_or_default();

        let files = WalkBuilder::new(&dir)
            .hidden(false)
            .follow_links(false) // We're scanning over depth 1
            .git_ignore(git_ignore)
            .max_depth(Some(1))
            .build()
            .filter_map(|file| {
                file.ok().and_then(|entry| {
                    let fmatch = filter_fn(&entry);

                    if fmatch == FileMatch::Reject {
                        return None;
                    }

                    if super::wildignored(&wildignore, entry.path()) {
                        return None;
                    }

                    let path = entry.path();
                    let is_dir = path.is_dir();
                    let file_type = entry.file_type();
                    let is_symlink = file_type.is_some_and(|ft| ft.is_symlink());
                    let mut path = if is_tilde {
                        // if it's a single tilde an absolute path is displayed so that when `TAB` is pressed on
                        // one of the directories the tilde will be replaced with a valid path not with a relative
                        // home directory name.
                        // ~ -> <TAB> -> /home/user
                        // ~/ -> <TAB> -> ~/first_entry
                        path.to_path_buf()
                    } else {
                        path.strip_prefix(&dir).unwrap_or(path).to_path_buf()
                    };

                    if fmatch == FileMatch::AcceptIncomplete {
                        path.push("");
                    }

                    let path = path.into_os_string().into_string().ok()?;
                    Some(Utf8PathBuf {
                        path,
                        is_dir,
                        is_symlink,
                    })
                })
            }) // TODO: unwrap or skip
            .filter(|path| !path.path.is_empty());

        let directory_color = editor.theme.get("ui.text.directory");
        let symlink_color = editor.theme.get("ui.text.symlink");

        let style_from_file = |file: Utf8PathBuf| {
            if file.is_symlink {
                Span::styled(file.path, symlink_color)
            } else if file.is_dir {
                Span::styled(file.path, directory_color)
            } else {
                Span::raw(file.path)
            }
        };

        // if empty, return a list of dirs and files in current dir
        if let Some(file_name) = file_name {
            let range = (input.len().saturating_sub(file_name.len()))..;
            // vim `wildignorecase` / `wildoptions`: how a typed name matches a file.
            let mut matches = wild_match(&file_name, files, true);
            // vim 'suffixes': when several names match, the ones ending in one of
            // them go last — they are the build products you almost never mean
            // (`main.o` after `main.c`). Stable, so the wildcard's own order
            // survives inside each rank.
            let suffixes = crate::commands::typed::suffixes_spec();
            matches.sort_by_key(|(name, _)| {
                crate::commands::typed::suffix_ranked_last(&suffixes, name.as_ref())
            });
            matches
                .into_iter()
                .map(|(name, _)| (range.clone(), style_from_file(name)))
                .collect()

            // TODO: complete to longest common match
        } else {
            let mut files: Vec<_> = files
                .map(|file| (end.clone(), style_from_file(file)))
                .collect();
            files.sort_unstable_by(|(_, path1), (_, path2)| path1.content.cmp(&path2.content));
            files
        }
    }

    pub fn register(editor: &Editor, input: &str) -> Vec<Completion> {
        let iter = editor
            .registers
            .iter_preview()
            // Exclude special registers that shouldn't be written to
            .filter(|(ch, _)| !matches!(ch, '%' | '#' | '.'))
            .map(|(ch, _)| ch.to_string());

        fuzzy_match(input, iter, false)
            .into_iter()
            .map(|(name, _)| ((0..), name.into()))
            .collect()
    }

    /// Every executable name on `PATH`, read once. Shared by the `:` command-name
    /// completer and comint's `shell-dynamic-complete-command`.
    pub fn programs_in_path() -> &'static BTreeSet<String> {
        static PROGRAMS_IN_PATH: Lazy<BTreeSet<String>> = Lazy::new(|| {
            // Go through the entire PATH and read all files into a set.
            let Some(path) = std::env::var_os("PATH") else {
                return Default::default();
            };

            std::env::split_paths(&path)
                .filter_map(|path| std::fs::read_dir(path).ok())
                .flatten()
                .filter_map(|res| {
                    let entry = res.ok()?;
                    let file_type = entry.file_type().ok()?;
                    if file_type.is_file() || file_type.is_symlink() {
                        entry.file_name().into_string().ok()
                    } else {
                        None
                    }
                })
                .collect()
        });
        &PROGRAMS_IN_PATH
    }

    pub fn program(_editor: &Editor, input: &str) -> Vec<Completion> {
        // vim `wildignorecase` / `wildoptions` apply to command-name completion
        // just as they do to file names.
        wild_match(input, programs_in_path().iter(), false)
            .into_iter()
            .map(|(name, _)| ((0..), name.clone().into()))
            .collect()
    }

    /// This expects input to be a raw string of arguments, because this is what Signature's raw_after does.
    pub fn repeating_filenames(editor: &Editor, input: &str) -> Vec<Completion> {
        let token = match Tokenizer::new(input, false).last() {
            Some(token) => token.unwrap(),
            None => return filename(editor, input),
        };

        let offset = token.content_start;

        let mut completions = filename(editor, &input[offset..]);
        for completion in completions.iter_mut() {
            completion.0.start += offset;
        }
        completions
    }

    pub fn shell(editor: &Editor, input: &str) -> Vec<Completion> {
        let (command, args, complete_command) = command_line::split(input);

        if complete_command {
            return program(editor, command);
        }

        let mut completions = repeating_filenames(editor, args);
        for completion in completions.iter_mut() {
            // + 1 for separator between `command` and `args`
            completion.0.start += command.len() + 1;
        }

        completions
    }
}

#[cfg(test)]
mod tests {
    use std::fs::{create_dir, File};

    use super::*;

    #[test]
    fn wildignorecase_folds_case_only_when_asked() {
        // vim matches the typed prefix case-sensitively -- `SRC` is not `src`.
        assert!(completers::wild_prefix_match("src", "sr", false));
        assert!(!completers::wild_prefix_match("src", "SR", false));
        // `wildignorecase` folds case, so it is.
        assert!(completers::wild_prefix_match("src", "SR", true));
        assert!(completers::wild_prefix_match("SRC", "sr", true));
        // A candidate that does not start with what was typed never matches.
        assert!(!completers::wild_prefix_match("src", "rc", true));
    }

    #[test]
    fn test_get_child_if_single_dir() {
        let root = tempfile::tempdir().unwrap();

        assert_eq!(get_child_if_single_dir(root.path()), None);

        let dir = root.path().join("dir1");
        create_dir(&dir).unwrap();

        assert_eq!(get_child_if_single_dir(root.path()), Some(dir));

        let file = root.path().join("file");
        File::create(file).unwrap();

        assert_eq!(get_child_if_single_dir(root.path()), None);
    }
}
