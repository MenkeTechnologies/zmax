use arc_swap::{access::Map, ArcSwap};
use futures_util::Stream;
use serde_json::json;
use tui::backend::Backend;
use zmax_core::{diagnostic::Severity, pos_at_coords, syntax, Range, Selection};
use zmax_lsp::{
    lsp::{self, notification::Notification},
    util::lsp_range_to_range,
    LanguageServerId, LspProgressMap,
};
use zmax_stdx::path::get_relative_path;
use zmax_view::{
    align_view,
    document::{DocumentOpenError, DocumentSavedEventResult},
    editor::{ConfigEvent, EditorEvent},
    graphics::Rect,
    theme,
    tree::Layout,
    Align, Editor,
};

use crate::{
    args::Args,
    compositor::{Compositor, Event},
    config::Config,
    handlers,
    job::Jobs,
    keymap::Keymaps,
    ui,
};

use log::{debug, error, info, warn};
use std::{
    io::{stdin, IsTerminal},
    path::Path,
    sync::Arc,
};

#[cfg_attr(windows, allow(unused_imports))]
use anyhow::{Context, Error};

#[cfg(not(windows))]
use {signal_hook::consts::signal, signal_hook_tokio::Signals};
#[cfg(windows)]
type Signals = futures_util::stream::Empty<()>;

#[cfg(all(not(windows), not(feature = "integration")))]
use tui::backend::TerminaBackend;

#[cfg(all(windows, not(feature = "integration")))]
use tui::backend::CrosstermBackend;

#[cfg(feature = "integration")]
use tui::backend::TestBackend;

#[cfg(all(not(windows), not(feature = "integration")))]
type TerminalBackend = TerminaBackend;
#[cfg(all(windows, not(feature = "integration")))]
type TerminalBackend = CrosstermBackend<std::io::Stdout>;
#[cfg(feature = "integration")]
type TerminalBackend = TestBackend;

#[cfg(not(windows))]
type TerminalEvent = termina::Event;
#[cfg(windows)]
type TerminalEvent = crossterm::event::Event;

type Terminal = tui::terminal::Terminal<TerminalBackend>;

/// vim `shortmess` (`shm`): the flags controlling which messages are shown and
/// how short they are. Vim's default is `ltToOCF`, which is also what zmax's
/// messages have always looked like, so an unset option keeps them unchanged.
fn shortmess() -> String {
    crate::commands::typed::vim_opt_str("shortmess")
        .or_else(|| crate::commands::typed::vim_opt_str("shm"))
        .unwrap_or_else(|| "ltToOCF".to_string())
}

/// vim `titlelen` (default 85): the percentage of the screen width the window
/// title may take. A longer title is cut (with an ellipsis so the truncation is
/// visible); 0 means "never set the title", which vim spells as `titlelen=0`, so
/// the title is emptied. Pure — unit tested.
fn truncate_title(title: &str, titlelen: Option<usize>, columns: usize) -> String {
    let Some(percent) = titlelen else {
        return title.to_string();
    };
    if percent == 0 {
        return String::new();
    }
    let max = columns * percent.min(100) / 100;
    if max == 0 || title.chars().count() <= max {
        return title.to_string();
    }
    let mut out: String = title.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// vim `icon` / `iconstring`: set the terminal's *icon name* with OSC 1. The
/// backend's `set_title` is OSC 2 (the window title); the icon name is the
/// separate label a terminal shows on its tab or in the taskbar, so it has its
/// own escape and cannot go through `set_title`. `\x1b\\` (ST) terminates the
/// string — accepted by every terminal that understands OSC, unlike BEL.
/// Control characters are dropped so a file name can never inject an escape.
fn set_icon_name(icon: &str) {
    use std::io::Write;
    let clean: String = icon.chars().filter(|c| !c.is_control()).collect();
    let mut out = std::io::stdout().lock();
    let _ = write!(out, "\x1b]1;{clean}\x1b\\");
    let _ = out.flush();
}

/// The message shown after a file is written, under vim `shortmess`:
///
/// * `W` — no message at all.
/// * `w` — "\[w\]" instead of "written" (`a` implies it).
/// * `l` — "12L 1.2KiB" instead of "12 lines, 1234 bytes" (`a` implies it; both
///   are in vim's default value, which is why this is the message zmax has
///   always printed).
///
/// Pure — unit tested.
fn write_message(
    name: &str,
    lines: usize,
    bytes: usize,
    human_size: &str,
    shortmess: &str,
) -> Option<String> {
    if shortmess.contains('W') {
        return None;
    }
    let all = shortmess.contains('a');
    let written = if all || shortmess.contains('w') {
        "[w]"
    } else {
        "written"
    };
    let size = if all || shortmess.contains('l') {
        format!("{lines}L {human_size}")
    } else {
        format!("{lines} lines, {bytes} bytes")
    };
    Some(format!("'{name}' {written}, {size}"))
}

// ── the zmax server (emacs `server-start`) ────────────────────────────────
//
// `M-x server-start` binds the Unix socket (it is the command that can report a
// bind failure to the user) and hands the listening socket here, because the
// event loop is what can `accept()` on it: the accept future joins the
// `tokio::select!` below, beside the terminal, the LSP and the job callbacks.
// A client that asked for a file is then parked on `Editor::server`, still
// holding its socket, until `server-edit` (`C-x #`) releases it.

/// A listening socket handed over by `M-x server-start`.
#[cfg(unix)]
static PENDING_LISTENER: std::sync::Mutex<Option<std::os::unix::net::UnixListener>> =
    std::sync::Mutex::new(None);
/// Set by `M-x server-start` when it stops a running server.
#[cfg(unix)]
static LISTENER_STOPPED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Hand a freshly-bound server socket to the event loop.
#[cfg(unix)]
pub fn set_pending_server_listener(listener: std::os::unix::net::UnixListener) {
    if let Ok(mut slot) = PENDING_LISTENER.lock() {
        *slot = Some(listener);
    }
}

/// Tell the event loop to drop the listening socket (the server was stopped).
#[cfg(unix)]
pub fn stop_server_listener() {
    LISTENER_STOPPED.store(true, std::sync::atomic::Ordering::Relaxed);
}

#[cfg(unix)]
type ServerListener = tokio::net::UnixListener;
#[cfg(not(unix))]
type ServerListener = ();
#[cfg(unix)]
type ServerConn = tokio::net::UnixStream;
#[cfg(not(unix))]
type ServerConn = std::convert::Infallible;

pub struct Application {
    compositor: Compositor,
    terminal: Terminal,
    pub editor: Editor,

    config: Arc<ArcSwap<Config>>,

    signals: Signals,
    jobs: Jobs,
    lsp_progress: LspProgressMap,

    theme_mode: Option<theme::Mode>,

    /// The `server-start` listening socket, once a server has been started.
    server_listener: Option<ServerListener>,
}

#[cfg(feature = "integration")]
fn setup_integration_logging() {
    let level = std::env::var("ZMAX_LOG_LEVEL")
        .map(|lvl| lvl.parse().unwrap())
        .unwrap_or(log::LevelFilter::Info);

    crate::logging::init_stdout(level);
}

impl Application {
    pub fn new(
        args: Args,
        config: Config,
        lang_loader: syntax::Loader,
        workspace_trust: zmax_loader::workspace_trust::WorkspaceTrust,
    ) -> Result<Self, Error> {
        #[cfg(feature = "integration")]
        setup_integration_logging();

        use zmax_view::editor::Action;

        let mut theme_parent_dirs = vec![zmax_loader::config_dir()];
        theme_parent_dirs.extend(zmax_loader::runtime_dirs().iter().cloned());
        let theme_loader = theme::Loader::new(&theme_parent_dirs);

        #[cfg(all(not(windows), not(feature = "integration")))]
        let backend = TerminaBackend::new((&config.editor).into())
            .context("failed to create terminal backend")?;
        #[cfg(all(windows, not(feature = "integration")))]
        let backend = CrosstermBackend::new(std::io::stdout(), (&config.editor).into());

        #[cfg(feature = "integration")]
        let backend = TestBackend::new(120, 150);

        let theme_mode = backend.get_theme_mode();
        let mut terminal = Terminal::new(backend)?;
        let area = terminal.size();
        let mut compositor = Compositor::new(area);
        let config = Arc::new(ArcSwap::from_pointee(config));
        let handlers = handlers::setup(config.clone());
        let mut editor = Editor::new(
            area,
            Arc::new(theme_loader),
            Arc::new(ArcSwap::from_pointee(lang_loader)),
            Arc::new(Map::new(Arc::clone(&config), |config: &Config| {
                &config.editor
            })),
            handlers,
            workspace_trust,
        );
        editor.vim_semantics = matches!(config.load().keymap.as_str(), "vim" | "spacemacs");
        Self::load_configured_theme(&mut editor, &config.load(), &mut terminal, theme_mode);

        // Restore vim global marks (`A`-`Z`) and numbered file marks (`0`-`9`)
        // from the previous session's `.zmaxinfo`, so `` `A ``/`` `3 `` jump
        // across restarts. Buffer-local `a`-`z` marks stay per-document.
        editor.global_marks = crate::zmaxinfo::load();
        // Seed the vim `` `" `` last-position restore across sessions: the numbered
        // file marks (`'0`-`'9`) record each recent file's last cursor line/col.
        for (mark, gm) in &editor.global_marks {
            if mark.is_ascii_digit() {
                editor
                    .last_positions
                    .insert(zmax_stdx::path::canonicalize(&gm.path), (gm.line, gm.col));
            }
        }

        let keys = Box::new(Map::new(Arc::clone(&config), |config: &Config| {
            &config.keys
        }));
        // Session persistence: restore drawer layout / reopen tabs from a previous session.
        let appdata = crate::appdata::load();

        // Restore the last session's theme (chosen via `:theme`, the picker, or
        // `:theme-toggle`, persisted to appdata on exit). This overrides the
        // config theme loaded above so a runtime theme change sticks across
        // restarts. Falls back to the already-loaded config theme on failure.
        // Skipped when `sync-zwire-theme` is on — zwire is authoritative there,
        // so a stale persisted theme must not clobber the zwire scheme.
        if let Some(name) = appdata
            .as_ref()
            .filter(|_| !config.load().editor.sync_zwire_theme)
            .and_then(|d| d.theme.as_deref())
        {
            let true_color = terminal.backend().supports_true_color()
                || config.load().editor.true_color
                || crate::true_color();
            match editor.theme_loader.load(name) {
                Ok(theme) if true_color || theme.is_16_color() => {
                    let _ = editor.set_theme(theme);
                }
                Ok(_) => {}
                Err(e) => log::warn!("failed to restore saved theme `{}` - {}", name, e),
            }
        }

        // Start the filesystem watcher at boot (not just when the IDE opens) so
        // open buffers auto-reload on external changes even in plain editing;
        // `spawn` is idempotent, so the IDE's own call is a later no-op. The tree
        // refresh inside the watcher no-ops until an IDE workbench exists.
        crate::file_watcher::spawn(
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
        );

        // Live-follow the zwire host colorscheme: a dedicated watcher on
        // `~/.zwire/global.toml` re-applies the theme on change, independent of
        // user input (the dispatched apply gates on `sync-zwire-theme`).
        crate::zwire::spawn_watcher();

        let mut editor_view = ui::EditorView::new(Keymaps::new(keys));
        // Make the previous session's IDE layout (drawer widths, folds, collapse /
        // hide state) available so that opening the workbench later — via `:ide`,
        // a toggle, or `--ide` — restores the user's arrangement instead of
        // starting from defaults.
        if let Some(data) = &appdata {
            editor_view.set_ide_layout(data.ide.clone());
        }
        // Reopen the IDE workbench when it was open last session (persisted
        // `ide.open` in appdata.toml) or when explicitly requested with `--ide`,
        // so `:ide` survives a restart — you get your workbench back, with the
        // stored layout applied above, exactly as you left it.
        // Opening a directory (`zmax .`) boots the IDE workbench with a scratch
        // buffer instead of a file picker — the file tree is the point.
        let dir_arg = args.files.first().is_some_and(|(p, _)| p.is_dir());
        let reopen_ide = appdata.as_ref().is_some_and(|d| d.ide.open);
        if args.ide || reopen_ide || dir_arg {
            editor_view.open_sidebar();
        }
        compositor.push(Box::new(editor_view));

        let jobs = Jobs::new();

        if args.load_tutor {
            let path = zmax_loader::runtime_file(Path::new("tutor"));
            editor.open(&path, Action::VerticalSplit)?;
            // Unset path to prevent accidentally saving to the original tutor file.
            doc_mut!(editor).set_path(None);
        } else if !args.files.is_empty() {
            let mut files_it = args.files.into_iter().peekable();

            // A leading directory (`zmax .`) just boots IDE mode (handled above
            // via `dir_arg`) with the default scratch buffer — consume the dir arg
            // and don't pop a file picker.
            let _ = files_it.next_if(|(p, _)| p.is_dir());

            // If there are any more files specified, open them
            if files_it.peek().is_some() {
                let mut nr_of_files = 0;
                for (file, pos) in files_it {
                    nr_of_files += 1;
                    if file.is_dir() {
                        return Err(anyhow::anyhow!(
                            "expected a path to file, but found a directory: {file:?}. (to open a directory pass it as first argument)"
                        ));
                    } else {
                        // If the user passes in either `--vsplit` or
                        // `--hsplit` as a command line argument, all the given
                        // files will be opened according to the selected
                        // option. If neither of those two arguments are passed
                        // in, just load the files normally.
                        let action = match args.split {
                            _ if nr_of_files == 1 => Action::VerticalSplit,
                            Some(Layout::Vertical) => Action::VerticalSplit,
                            Some(Layout::Horizontal) => Action::HorizontalSplit,
                            None => Action::Load,
                        };
                        let old_id = editor.document_id_by_path(&file);
                        let doc_id = match editor.open(&file, action) {
                            // Ignore irregular files during application init.
                            Err(DocumentOpenError::IrregularFile) => {
                                nr_of_files -= 1;
                                continue;
                            }
                            // A binary file opens in the hex editor instead of
                            // being rejected.
                            Err(DocumentOpenError::BinaryFile) => {
                                if let Ok(bytes) = std::fs::read(&file) {
                                    let name = file
                                        .file_name()
                                        .map(|n| n.to_string_lossy().into_owned())
                                        .unwrap_or_else(|| file.display().to_string());
                                    compositor.push(Box::new(ui::hex::HexView::new(
                                        name,
                                        Some(file.clone()),
                                        bytes,
                                    )));
                                }
                                nr_of_files -= 1;
                                continue;
                            }
                            Err(err) => return Err(anyhow::anyhow!(err)),
                            // We can't open more than 1 buffer for 1 file, in this case we already have opened this file previously
                            Ok(doc_id) if old_id == Some(doc_id) => {
                                nr_of_files -= 1;
                                doc_id
                            }
                            Ok(doc_id) => doc_id,
                        };
                        // with Action::Load all documents have the same view
                        // NOTE: this isn't necessarily true anymore. If
                        // `--vsplit` or `--hsplit` are used, the file which is
                        // opened last is focused on.
                        let view_id = editor.tree.focus;
                        let doc = doc_mut!(editor, &doc_id);
                        let selection = pos
                            .into_iter()
                            .map(|coords| {
                                Range::point(pos_at_coords(doc.text().slice(..), coords, true))
                            })
                            .collect();
                        doc.set_selection(view_id, selection);
                    }
                }

                // if all files were invalid, replace with empty buffer
                if nr_of_files == 0 {
                    editor.new_file(Action::VerticalSplit);
                } else {
                    editor.set_status(format!(
                        "Loaded {} file{}.",
                        nr_of_files,
                        if nr_of_files == 1 { "" } else { "s" } // avoid "Loaded 1 files." grammo
                    ));
                    // align the view to center after all files are loaded,
                    // does not affect views without pos since it is at the top
                    let (view, doc) = current!(editor);
                    align_view(doc, view, Align::Center);
                }
            } else {
                editor.new_file(Action::VerticalSplit);
            }
        } else if stdin().is_terminal() || cfg!(feature = "integration") {
            // No file arguments. What we open is governed by the `editor.startup`
            // option (default: the Startify start screen).
            //
            // Integration tests always get a clean, path-less scratch buffer with no
            // overlay: they launch with no files and assert on document text, path,
            // and buffer count. Honoring `startup` there would reopen the developer's
            // real `appdata.toml` session or recent files and break those assertions.
            #[cfg(feature = "integration")]
            {
                editor.new_file(Action::VerticalSplit);
            }
            #[cfg(not(feature = "integration"))]
            {
                use zmax_view::editor::StartupScreen;
                // Snapshot the config so we don't hold the ArcSwap guard across `editor` calls.
                let (startup, startup_file) = {
                    let cfg = config.load();
                    (cfg.editor.startup.clone(), cfg.editor.startup_file.clone())
                };

                let opened = match startup {
                    StartupScreen::Startify => false,
                    StartupScreen::Recent => {
                        // Open the most-recently-used file over a fresh scratch.
                        editor.new_file(Action::VerticalSplit);
                        crate::recent_files::load()
                            .first()
                            .map(|path| editor.open(path, Action::Replace).is_ok())
                            .unwrap_or(false)
                    }
                    StartupScreen::File => {
                        editor.new_file(Action::VerticalSplit);
                        let path = std::path::Path::new(&startup_file);
                        !startup_file.is_empty()
                            && path.is_file()
                            && editor.open(path, Action::Replace).is_ok()
                    }
                    // Restore manages its own buffers (background-loads each tab, then
                    // replaces into the focused view) so it doesn't leave a stray scratch.
                    StartupScreen::Session => restore_session(appdata.as_ref(), &mut editor),
                };

                // The tree must end up with a focused view or the first render panics.
                // When nothing was opened (Startify mode, or a fallback failed), ensure
                // a scratch buffer exists and render Startify over it.
                if !opened {
                    if editor.tree.try_get(editor.tree.focus).is_none() {
                        editor.new_file(Action::VerticalSplit);
                    }
                    compositor.push(Box::new(ui::Startify::new()));
                }
            }
        } else {
            editor
                .new_file_from_stdin(Action::VerticalSplit)
                .unwrap_or_else(|_| editor.new_file(Action::VerticalSplit));
        }

        #[cfg(windows)]
        let signals = futures_util::stream::empty();
        #[cfg(not(windows))]
        let signals = Signals::new([
            signal::SIGTSTP,
            signal::SIGCONT,
            signal::SIGUSR1,
            signal::SIGTERM,
            signal::SIGINT,
        ])
        .context("build signal handler")?;

        // Emacs is modeless — start in Insert mode so typing self-inserts
        // immediately (the modal keymaps stay in Normal, the existing default).
        editor.mode = crate::keymap::default_mode(&config.load().keymap);

        let app = Self {
            compositor,
            terminal,
            editor,
            config,
            signals,
            jobs,
            lsp_progress: LspProgressMap::new(),
            theme_mode,
            server_listener: None,
        };

        Ok(app)
    }

    async fn render(&mut self) {
        if self.compositor.full_redraw {
            self.terminal.clear().expect("Cannot clear the terminal");
            self.compositor.full_redraw = false;
        }

        let mut cx = crate::compositor::Context {
            editor: &mut self.editor,
            jobs: &mut self.jobs,
            scroll: None,
        };

        zmax_event::start_frame();
        cx.editor.needs_redraw = false;

        let area = self
            .terminal
            .autoresize()
            .expect("Unable to determine terminal size");

        // TODO: need to recalculate view tree if necessary

        let surface = self.terminal.current_buffer_mut();

        self.compositor.render(area, surface, &mut cx);

        // vim `title`: keep the terminal window title in sync with the current
        // file, only re-emitting the OSC when it actually changes.
        {
            let cfg = self.editor.config();
            if cfg.title {
                let (_view, doc) = current_ref!(self.editor);
                let path = doc.path();
                let name = path
                    .and_then(|p| p.file_name())
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "[scratch]".to_string());
                let full = path
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_else(|| name.clone());
                let title = if cfg.title_string.is_empty() {
                    format!("{name} - zmax")
                } else {
                    cfg.title_string.replace("%f", &full).replace("%t", &name)
                };
                // vim `titlelen`: the title may only use this percentage of the
                // screen width; a longer one is truncated (vim shortens the path,
                // which for zmax's file-name title is the same as cutting it).
                let title = truncate_title(
                    &title,
                    crate::commands::typed::vim_opt_num("titlelen"),
                    area.width as usize,
                );
                static LAST_TITLE: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());
                if let Ok(mut last) = LAST_TITLE.lock() {
                    if *last != title {
                        *last = title.clone();
                        let _ = self.terminal.backend_mut().set_title(&title);
                    }
                }
            }
            // vim `icon`: the terminal's *icon name* — the short label a terminal
            // puts on its tab / in the taskbar, which is a different string from
            // the window title and a different escape (OSC 1, not OSC 2).
            // `iconstring` names it; unset, vim uses the file name.
            if crate::commands::typed::vim_opt_bool("icon") {
                let (_view, doc) = current_ref!(self.editor);
                let path = doc.path();
                let name = path
                    .and_then(|p| p.file_name())
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "[scratch]".to_string());
                let full = path
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_else(|| name.clone());
                let icon = crate::commands::typed::vim_opt_str_alias("iconstring", "iconstr")
                    .map(|fmt| fmt.replace("%f", &full).replace("%t", &name))
                    .unwrap_or(name);
                static LAST_ICON: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());
                if let Ok(mut last) = LAST_ICON.lock() {
                    if *last != icon {
                        *last = icon.clone();
                        set_icon_name(&icon);
                    }
                }
            }
        }

        let (pos, kind) = self.compositor.cursor(area, &self.editor);
        // reset cursor cache
        self.editor.cursor_cache.reset();

        let pos = pos.map(|pos| (pos.col as u16, pos.row as u16));
        self.terminal.draw(pos, kind).unwrap();
    }

    pub async fn event_loop<S>(&mut self, input_stream: &mut S)
    where
        S: Stream<Item = std::io::Result<TerminalEvent>> + Unpin,
    {
        self.render().await;

        loop {
            if !self.event_loop_until_idle(input_stream).await {
                break;
            }
        }
    }

    pub async fn event_loop_until_idle<S>(&mut self, input_stream: &mut S) -> bool
    where
        S: Stream<Item = std::io::Result<TerminalEvent>> + Unpin,
    {
        loop {
            if self.editor.should_close() {
                // emacs `desktop-save-mode`: the desktop (every file-visiting
                // buffer and its point) is saved on the way out, unasked.
                if self.editor.desktop_save_mode {
                    let mut cx = crate::compositor::Context {
                        editor: &mut self.editor,
                        jobs: &mut self.jobs,
                        scroll: None,
                    };
                    crate::commands::typed::run_command_line(&mut cx, "desktop-save");
                }
                return false;
            }

            use futures_util::StreamExt;

            // Pick up a socket `M-x server-start` just bound (or drop the one it
            // just stopped) before the accept branch below is armed.
            self.sync_server_listener();

            // vim 'timeout'/'timeoutlen'/'ttimeout'/'ttimeoutlen': a half-typed
            // chord (`g`, `SPC w`, `C-x r`) gets a deadline instead of waiting for
            // its next key forever. `None` until one of the four options is `:set`.
            let pending_timeout = self
                .compositor
                .find::<ui::EditorView>()
                .and_then(|view| crate::commands::typed::pending_key_timeout(view.pending_keys()));

            tokio::select! {
                biased;

                Some(conn) = Self::accept_server(self.server_listener.as_ref()) => {
                    self.handle_server_connection(conn).await;
                    self.render().await;
                }
                Some(signal) = self.signals.next() => {
                    if !self.handle_signals(signal).await {
                        return false;
                    };
                }
                Some(event) = input_stream.next() => {
                    self.handle_terminal_events(event).await;
                    // A command may have queued an external-fzf request.
                    self.drain_fzf().await;
                    // ...or a full-screen tty command (image-dired image viewer).
                    self.drain_tty_command().await;
                }
                Some(callback) = self.jobs.callbacks.recv() => {
                    if let Some(job) = self.jobs.handle_callback(&mut self.editor, &mut self.compositor, Ok(Some(callback))) {
                        self.jobs.add(job);
                    }
                    self.render().await;
                }
                Some(msg) = self.jobs.status_messages.recv() => {
                    let severity = match msg.severity{
                        zmax_event::status::Severity::Hint => Severity::Hint,
                        zmax_event::status::Severity::Info => Severity::Info,
                        zmax_event::status::Severity::Warning => Severity::Warning,
                        zmax_event::status::Severity::Error => Severity::Error,
                    };
                    // TODO: show multiple status messages at once to avoid clobbering
                    self.editor.status_msg = Some((msg.message, severity));
                    zmax_event::request_redraw();
                }
                Some(callback) = self.jobs.wait_futures.next() => {
                    if let Some(job) = self.jobs.handle_callback(&mut self.editor, &mut self.compositor, callback) {
                        self.jobs.add(job);
                    }
                    self.render().await;
                }
                event = self.editor.wait_event() => {
                    let _idle_handled = self.handle_editor_event(event).await;

                    #[cfg(feature = "integration")]
                    {
                        // Don't report idle while a save is still in flight, or an assertion on the post-save document state (e.g. its path,
                        // set in `handle_document_write`) can run before the `DocumentSavedEvent` is processed. Slow file I/O on Windows
                        // (atomic_save's rename/fsync dance over the still-open temp file) makes this race observable.
                        // Errors produce an event too, so it cannot hang.
                        if _idle_handled && self.editor.write_count == 0 {
                            return true;
                        }
                    }
                }
                // vim 'timeout': the pending chord's deadline. Last and guarded, so
                // a real key always wins the race against the timer.
                () = tokio::time::sleep(pending_timeout.unwrap_or_default()), if pending_timeout.is_some() => {
                    if let Some(view) = self.compositor.find::<ui::EditorView>() {
                        view.cancel_pending_keys(&mut self.editor);
                    }
                    self.render().await;
                }
            }

            // for integration tests only, reset the idle timer after every
            // event to signal when test events are done processing
            #[cfg(feature = "integration")]
            {
                self.editor.reset_idle_timer();
            }
        }
    }

    /// Load embedded-scripting init files (currently `~/.zmax/init.el`) once
    /// the editor is constructed. Best-effort: errors surface on the status line.
    pub fn load_init_scripts(&mut self) {
        let mut cx = crate::compositor::Context {
            editor: &mut self.editor,
            jobs: &mut self.jobs,
            scroll: None,
        };
        // emacs `package-activate-all` (what `package-initialize` does at startup):
        // every installed package is put on the elisp load path and its autoload
        // file evaluated, BEFORE `init.el` runs — so an init file can configure a
        // package it has installed, exactly as in Emacs.
        let packages = crate::commands::package_activate_all_impl(&mut cx);
        if packages > 0 {
            log::info!("package-activate-all: activated {packages} package(s)");
        }
        crate::commands::scripting::load_init_scripts(&mut cx);
        self.load_exrc();
        self.load_plugins();
    }

    /// vim `loadplugins`: "when on the plugin scripts are loaded when starting
    /// up". Vim loads them *after* the init file, which is why `:set noloadplugins`
    /// in a vimrc suppresses them — so this runs last, after `load_init_scripts`
    /// and `load_exrc` have had their chance to turn the option off.
    ///
    /// "The plugin scripts" are `plugin/**.vim` on the 'runtimepath' plus every
    /// package under `pack/*/start/` on the 'packpath' — the same files `:runtime`
    /// and `:packloadall` source, through the same vimlrs interpreter.
    fn load_plugins(&mut self) {
        if !crate::commands::typed::vim_opt_bool("loadplugins") {
            log::info!("loadplugins is off — not sourcing plugin scripts");
            return;
        }
        let mut cx = crate::compositor::Context {
            editor: &mut self.editor,
            jobs: &mut self.jobs,
            scroll: None,
        };
        let n = crate::commands::typed::load_startup_plugins(&mut cx);
        if n > 0 {
            log::info!("loadplugins: sourced {n} plugin file(s)");
        }
    }

    /// vim `exrc`: "Enables project-local configuration. Nvim will execute any
    /// .nvim.lua, .nvimrc, or .exrc file found in the current directory and all
    /// parent directories (ordered upwards), if the files are in the trust list."
    ///
    /// zmax runs the vimscript ones (`.exrc`, `.nvimrc`, `.vimrc`) through the
    /// same interpreter `:source` uses, after the user's own init files — which is
    /// where `:set exrc` itself lives. The trust list is zmax's own workspace
    /// trust (the gate that already decides whether a project's `.zmax/config.toml`
    /// may be loaded), so an untrusted directory's rc file is never executed.
    fn load_exrc(&mut self) {
        if !crate::commands::typed::vim_opt_bool("exrc") {
            return;
        }
        let Ok(cwd) = std::env::current_dir() else {
            return;
        };
        if !self
            .editor
            .workspace_trust
            .query_for_file(
                &cwd,
                zmax_loader::workspace_trust::TrustQuery::LocalConfig,
            )
            .is_trusted()
        {
            log::info!("exrc: {} is not trusted, not sourcing", cwd.display());
            return;
        }
        // Parents first, so the closest directory's rc wins (vim "ordered upwards"
        // means the search walks up; the nearest file is applied last). The home
        // directory is skipped: its `.vimrc` is the user's *init* file, which the
        // init pass above already sourced — vim likewise never re-sources it here.
        let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
        let mut dirs: Vec<&std::path::Path> = cwd
            .ancestors()
            .filter(|dir| home.as_deref() != Some(*dir))
            .collect();
        dirs.reverse();
        let mut sourced = Vec::new();
        for dir in dirs {
            for name in [".exrc", ".nvimrc", ".vimrc"] {
                let path = dir.join(name);
                if !path.is_file() {
                    continue;
                }
                let mut cx = crate::compositor::Context {
                    editor: &mut self.editor,
                    jobs: &mut self.jobs,
                    scroll: None,
                };
                match crate::commands::scripting::source_viml_file(&mut cx, &path) {
                    Ok(()) => sourced.push(path),
                    Err(err) => self
                        .editor
                        .set_error(format!("exrc: {}: {err}", path.display())),
                }
            }
        }
        if !sourced.is_empty() {
            log::info!("exrc: sourced {sourced:?}");
        }
    }

    pub fn handle_config_events(&mut self, config_event: ConfigEvent) {
        let old_editor_config = self.editor.config();

        match config_event {
            ConfigEvent::Refresh => self.refresh_config(),

            // Since only the Application can make changes to Editor's config,
            // the Editor must send up a new copy of a modified config so that
            // the Application can apply it.
            ConfigEvent::Update(editor_config) => {
                let mut app_config = (*self.config.load().clone()).clone();
                app_config.editor = *editor_config;
                if let Err(err) = self.terminal.reconfigure((&app_config.editor).into()) {
                    self.editor.set_error(err.to_string());
                };
                self.config.store(Arc::new(app_config));
            }
            ConfigEvent::ThemeChanged => {
                // With `transparent-background`, don't push a theme bg to the
                // terminal via OSC so its own (possibly translucent) background
                // stays in effect.
                let bg = if self.config.load().editor.transparent_background {
                    None
                } else {
                    self.editor
                        .theme
                        .try_get_exact("ui.background")
                        .and_then(|style| style.bg)
                };
                let _ = self.terminal.backend_mut().set_background_color(bg);
                // Bidirectional sync: push a committed theme change back to the
                // zwire host so the browser/HUD follow. `last_theme.is_none()`
                // means this was a committed `set_theme` (a picker *preview*
                // leaves `last_theme` = Some), so scrolling the theme picker
                // doesn't flicker the whole desktop. `write_back` ignores
                // non-`zgui-*` themes and no-ops when `global.toml` already
                // matches, which also absorbs the echo from our own watcher.
                if self.config.load().editor.sync_zwire_theme && self.editor.last_theme.is_none() {
                    crate::zwire::write_back(self.editor.theme.name());
                }
                return;
            }
            ConfigEvent::ApplyUserMappings => {
                // Merge the runtime `:map` overlay onto the live keymap.
                let mut app_config = (*self.config.load().clone()).clone();
                crate::keymap::vim_map::apply_user_mappings(&mut app_config.keys);
                self.config.store(Arc::new(app_config));
                return;
            }
            ConfigEvent::SetKeymap(name) => {
                match crate::keymap::preset(&name) {
                    Some(mut keys) => {
                        // Swap the live keymap by storing a new app config; the
                        // editor view reads `config.keys` through this ArcSwap.
                        // Re-apply the runtime `:map` overlay on top of the fresh
                        // preset so live mappings survive the switch.
                        crate::keymap::vim_map::apply_user_mappings(&mut keys);
                        let mut app_config = (*self.config.load().clone()).clone();
                        app_config.keys = keys;
                        app_config.keymap = name.clone();
                        self.config.store(Arc::new(app_config));
                        // Match the preset's natural mode (emacs is modeless →
                        // Insert) so the switch is immediately usable.
                        self.editor.mode = crate::keymap::default_mode(&name);
                        // Follow the preset for vim-only semantics (dot-repeat,
                        // operator-count multiplication, magic-regex translation).
                        self.editor.vim_semantics = matches!(name.as_str(), "vim" | "spacemacs");
                        self.editor.set_status(format!("keymap: {name}"));
                    }
                    None => {
                        self.editor.set_error(format!(
                            "unknown keymap `{name}` (expected one of: {})",
                            crate::keymap::PRESETS.join(", ")
                        ));
                    }
                }
                return;
            }
        }

        // Update all the relevant members in the editor after updating
        // the configuration.
        self.editor.refresh_config(&old_editor_config);

        // reset view position in case softwrap was enabled/disabled
        let scrolloff = self.editor.config().scrolloff;
        for (view, _) in self.editor.tree.views() {
            let doc = doc_mut!(self.editor, &view.doc);
            view.ensure_cursor_in_view(doc, scrolloff);
        }
    }

    fn refresh_config(&mut self) {
        let mut refresh_config = || -> Result<(), Error> {
            let default_config = Config::load_default()
                .map_err(|err| anyhow::anyhow!("Failed to load config: {}", err))?;

            // Apply any change to editor.workspace_trust before reading local language config.
            self.editor
                .workspace_trust
                .set_config((&default_config.editor.workspace_trust).into());

            // Update the syntax language loader before setting the theme. Setting the theme will
            // call `Loader::set_scopes` which must be done before the documents are re-parsed for
            // the sake of locals highlighting.
            let lang_loader = zmax_core::config::user_lang_loader(&self.editor.workspace_trust)?;
            self.editor.syn_loader.store(Arc::new(lang_loader));
            Self::load_configured_theme(
                &mut self.editor,
                &default_config,
                &mut self.terminal,
                self.theme_mode,
            );

            // Re-parse any open documents with the new language config.
            let lang_loader = self.editor.syn_loader.load();
            for document in self.editor.documents.values_mut() {
                // Re-detect .editorconfig
                document.detect_editor_config();
                document.detect_language(&lang_loader);
                let diagnostics = Editor::doc_diagnostics(
                    &self.editor.language_servers,
                    &self.editor.diagnostics,
                    document,
                );
                document.replace_diagnostics(diagnostics, &[], None);
            }

            self.terminal.reconfigure((&default_config.editor).into())?;
            // Re-apply the runtime `:map` overlay: reloading from disk rebuilds
            // `config.keys` from the preset + config file and would drop any live
            // vimscript `:map`s otherwise.
            let mut default_config = default_config;
            crate::keymap::vim_map::apply_user_mappings(&mut default_config.keys);
            // Store new config
            self.config.store(Arc::new(default_config));
            Ok(())
        };

        match refresh_config() {
            Ok(_) => {
                self.editor.set_status("Config refreshed");
            }
            Err(err) => {
                self.editor.set_error(err.to_string());
            }
        }
    }

    /// Load the theme set in configuration
    fn load_configured_theme(
        editor: &mut Editor,
        config: &Config,
        terminal: &mut Terminal,
        mode: Option<theme::Mode>,
    ) {
        let true_color = terminal.backend().supports_true_color()
            || config.editor.true_color
            || crate::true_color();
        // Follow the zwire host's scheme when `sync-zwire-theme` is on: the
        // resolved `zgui-<scheme>` theme painted with zwire's live `[theme.palette]`
        // overrides the configured/persisted one. Uses the SAME resolver as the
        // live watcher so startup and live sync never diverge. Falls through to the
        // normal config theme if zwire names nothing usable or the theme can't be
        // loaded under the current color support.
        if config.editor.sync_zwire_theme {
            if let Some(theme) = crate::zwire::resolve_theme(editor, true_color) {
                let _ = editor.set_theme(theme);
                return;
            }
        }
        let theme = config
            .theme
            .as_ref()
            .and_then(|theme_config| {
                let theme = theme_config.choose(mode);
                editor
                    .theme_loader
                    .load(theme)
                    .map_err(|e| {
                        log::warn!("failed to load theme `{}` - {}", theme, e);
                        e
                    })
                    .ok()
                    .filter(|theme| {
                        let colors_ok = true_color || theme.is_16_color();
                        if !colors_ok {
                            log::warn!(
                                "loaded theme `{}` but cannot use it because true color \
                                support is not enabled",
                                theme.name()
                            );
                        }
                        colors_ok
                    })
            })
            .unwrap_or_else(|| {
                // Default colorscheme: zgui-cyberpunk (a true-color theme). Fall
                // back to the built-in default if the user's terminal lacks true
                // color or the theme can't be loaded.
                if true_color {
                    editor
                        .theme_loader
                        .load("zgui-cyberpunk")
                        .unwrap_or_else(|_| editor.theme_loader.default_theme(true_color))
                } else {
                    editor.theme_loader.default_theme(true_color)
                }
            });
        let _ = editor.set_theme(theme);
    }

    #[cfg(windows)]
    // no signal handling available on windows
    pub async fn handle_signals(&mut self, _signal: ()) -> bool {
        true
    }

    #[cfg(not(windows))]
    pub async fn handle_signals(&mut self, signal: i32) -> bool {
        match signal {
            signal::SIGTSTP => {
                self.restore_term().unwrap();

                // SAFETY:
                //
                // - zmax must have permissions to send signals to all processes in its signal
                //   group, either by already having the requisite permission, or by having the
                //   user's UID / EUID / SUID match that of the receiving process(es).
                let res = unsafe {
                    // A pid of 0 sends the signal to the entire process group, allowing the user to
                    // regain control of their terminal if the editor was spawned under another process
                    // (e.g. when running `git commit`).
                    //
                    // We have to send SIGSTOP (not SIGTSTP) to the entire process group, because,
                    // as mentioned above, the terminal will get stuck if `zmax` was spawned from
                    // an external process and that process waits for `zmax` to complete. This may
                    // be an issue with signal-hook-tokio, but the author of signal-hook believes it
                    // could be a tokio issue instead:
                    // https://github.com/vorner/signal-hook/issues/132
                    libc::kill(0, signal::SIGSTOP)
                };

                if res != 0 {
                    let err = std::io::Error::last_os_error();
                    eprintln!("{}", err);
                    let res = err.raw_os_error().unwrap_or(1);
                    std::process::exit(res);
                }
            }
            signal::SIGCONT => {
                // Copy/Paste from same issue from neovim:
                // https://github.com/neovim/neovim/issues/12322
                // https://github.com/neovim/neovim/pull/13084
                for retries in 1..=10 {
                    match self.terminal.claim() {
                        Ok(()) => break,
                        Err(err) if retries == 10 => panic!("Failed to claim terminal: {}", err),
                        Err(_) => continue,
                    }
                }

                // redraw the terminal
                let area = self.terminal.size();
                self.compositor.resize(area);
                self.terminal.clear().expect("couldn't clear terminal");

                self.render().await;
            }
            signal::SIGUSR1 => {
                self.refresh_config();
                self.render().await;
            }
            signal::SIGTERM | signal::SIGINT => {
                self.restore_term().unwrap();
                return false;
            }
            _ => unreachable!(),
        }

        true
    }

    pub async fn handle_idle_timeout(&mut self) {
        let mut cx = crate::compositor::Context {
            editor: &mut self.editor,
            jobs: &mut self.jobs,
            scroll: None,
        };
        let should_render = self.compositor.handle_event(&Event::IdleTimeout, &mut cx);
        if should_render || self.editor.needs_redraw {
            self.render().await;
        }
    }

    pub fn handle_document_write(&mut self, doc_save_event: DocumentSavedEventResult) {
        let doc_save_event = match doc_save_event {
            Ok(event) => event,
            Err(err) => {
                self.editor.set_error(err.to_string());
                return;
            }
        };

        // Local History: snapshot every save; invalidate the blame cache.
        crate::local_history::record(&doc_save_event.path, &doc_save_event.text);
        crate::blame::invalidate(&doc_save_event.path);

        let doc = match self.editor.document_mut(doc_save_event.doc_id) {
            None => {
                warn!(
                    "received document saved event for non-existent doc id: {}",
                    doc_save_event.doc_id
                );

                return;
            }
            Some(doc) => doc,
        };

        debug!(
            "document {:?} saved with revision {}",
            doc.path(),
            doc_save_event.revision
        );

        doc.set_last_saved_revision(doc_save_event.revision, doc_save_event.save_time);

        let lines = doc_save_event.text.len_lines();
        let size = doc_save_event.text.len_bytes();

        enum Size {
            Bytes(u16),
            HumanReadable(f32, &'static str),
        }

        impl std::fmt::Display for Size {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match self {
                    Self::Bytes(bytes) => write!(f, "{bytes}B"),
                    Self::HumanReadable(size, suffix) => write!(f, "{size:.1}{suffix}"),
                }
            }
        }

        let bytes = size;
        let size = if size < 1024 {
            Size::Bytes(size as u16)
        } else {
            const SUFFIX: [&str; 4] = ["B", "KiB", "MiB", "GiB"];
            let mut size = size as f32;
            let mut i = 0;
            while i < SUFFIX.len() - 1 && size >= 1024.0 {
                size /= 1024.0;
                i += 1;
            }
            Size::HumanReadable(size, SUFFIX[i])
        };

        self.editor
            .set_doc_path(doc_save_event.doc_id, &doc_save_event.path);
        // TODO: fix being overwritten by lsp
        // vim `shortmess`: `W` drops this message entirely, `w` shortens "written"
        // to "[w]", and `l` (in the default value) is what makes it "12L 1.2KiB"
        // rather than "12 lines, 1234 bytes".
        let name = get_relative_path(&doc_save_event.path)
            .to_string_lossy()
            .into_owned();
        if let Some(msg) = write_message(&name, lines, bytes, &size.to_string(), &shortmess()) {
            self.editor.set_status(msg);
        }
    }

    #[inline(always)]
    pub async fn handle_editor_event(&mut self, event: EditorEvent) -> bool {
        log::debug!("received editor event: {:?}", event);

        match event {
            EditorEvent::DocumentSaved(event) => {
                self.handle_document_write(event);
                self.render().await;
            }
            EditorEvent::ConfigEvent(event) => {
                self.handle_config_events(event);
                self.render().await;
            }
            EditorEvent::LanguageServerMessage((id, call)) => {
                self.handle_language_server_message(call, id).await;
                // limit render calls for fast language server messages
                zmax_event::request_redraw();
            }
            EditorEvent::DebuggerEvent((id, payload)) => {
                let needs_render = self.editor.handle_debugger_message(id, payload).await;
                if needs_render {
                    self.render().await;
                }
            }
            EditorEvent::Redraw => {
                self.render().await;
            }
            EditorEvent::IdleTimer => {
                self.editor.clear_idle_timer();
                self.handle_idle_timeout().await;

                #[cfg(feature = "integration")]
                {
                    return true;
                }
            }
        }

        false
    }

    pub async fn handle_terminal_events(&mut self, event: std::io::Result<TerminalEvent>) {
        #[cfg(not(windows))]
        use termina::escape::csi;

        let mut cx = crate::compositor::Context {
            editor: &mut self.editor,
            jobs: &mut self.jobs,
            scroll: None,
        };
        // Handle key events
        let should_redraw = match event.unwrap() {
            #[cfg(not(windows))]
            termina::Event::WindowResized(termina::WindowSize { rows, cols, .. }) => {
                self.terminal
                    .resize(Rect::new(0, 0, cols, rows))
                    .expect("Unable to resize terminal");

                let area = self.terminal.size();

                self.compositor.resize(area);

                self.compositor
                    .handle_event(&Event::Resize(cols, rows), &mut cx)
            }
            #[cfg(not(windows))]
            // Ignore keyboard release events.
            termina::Event::Key(termina::event::KeyEvent {
                kind: termina::event::KeyEventKind::Release,
                ..
            }) => false,
            #[cfg(not(windows))]
            termina::Event::Csi(csi::Csi::Mode(csi::Mode::ReportTheme(mode))) => {
                let config = self.config.load();
                let mode = mode.into();
                let mode_changed = self.theme_mode.as_ref().is_none_or(|m| m != &mode);
                if mode_changed && config.theme.as_ref().is_some_and(|t| t.is_adaptive()) {
                    self.theme_mode = Some(mode);
                    Self::load_configured_theme(
                        &mut self.editor,
                        &config,
                        &mut self.terminal,
                        self.theme_mode,
                    );
                    true
                } else {
                    false
                }
            }
            #[cfg(windows)]
            TerminalEvent::Resize(width, height) => {
                self.terminal
                    .resize(Rect::new(0, 0, width, height))
                    .expect("Unable to resize terminal");

                let area = self.terminal.size();

                self.compositor.resize(area);

                self.compositor
                    .handle_event(&Event::Resize(width, height), &mut cx)
            }
            #[cfg(windows)]
            // Ignore keyboard release events.
            crossterm::event::Event::Key(crossterm::event::KeyEvent {
                kind: crossterm::event::KeyEventKind::Release,
                ..
            }) => false,
            #[cfg(not(windows))]
            event if event.is_escape() => false,
            event => self.compositor.handle_event(&event.into(), &mut cx),
        };

        if should_redraw && !self.editor.should_close() {
            self.render().await;
        }
    }

    pub async fn handle_language_server_message(
        &mut self,
        call: zmax_lsp::Call,
        server_id: LanguageServerId,
    ) {
        use zmax_lsp::{Call, MethodCall, Notification};

        macro_rules! language_server {
            () => {
                match self.editor.language_server_by_id(server_id) {
                    Some(language_server) => language_server,
                    None => {
                        warn!("can't find language server with id `{}`", server_id);
                        return;
                    }
                }
            };
        }

        match call {
            Call::Notification(zmax_lsp::jsonrpc::Notification { method, params, .. }) => {
                let notification = match Notification::parse(&method, params) {
                    Ok(notification) => notification,
                    Err(zmax_lsp::Error::Unhandled) => {
                        info!("Ignoring Unhandled notification from Language Server");
                        return;
                    }
                    Err(err) => {
                        error!(
                            "Ignoring unknown notification from Language Server: {}",
                            err
                        );
                        return;
                    }
                };

                match notification {
                    Notification::Initialized => {
                        let language_server = language_server!();

                        // Trigger a workspace/didChangeConfiguration notification after initialization.
                        // This might not be required by the spec but Neovim does this as well, so it's
                        // probably a good idea for compatibility.
                        if let Some(config) = language_server.config() {
                            language_server.did_change_configuration(config.clone());
                        }

                        zmax_event::dispatch(zmax_view::events::LanguageServerInitialized {
                            editor: &mut self.editor,
                            server_id,
                        });
                    }
                    Notification::PublishDiagnostics(params) => {
                        let uri = match zmax_core::Uri::try_from(params.uri) {
                            Ok(uri) => uri,
                            Err(err) => {
                                log::error!("{err}");
                                return;
                            }
                        };
                        let language_server = language_server!();
                        if !language_server.is_initialized() {
                            log::error!(
                                "Discarding publishDiagnostic notification sent by an uninitialized server: {}",
                                language_server.name()
                            );
                            return;
                        }
                        let provider = zmax_core::diagnostic::DiagnosticProvider::Lsp {
                            server_id,
                            identifier: None,
                        };
                        self.editor.handle_lsp_diagnostics(
                            &provider,
                            uri,
                            params.version,
                            params.diagnostics,
                        );
                    }
                    Notification::ShowMessage(params) => {
                        self.handle_show_message(params.typ, params.message);
                    }
                    Notification::LogMessage(params) => {
                        log::info!("window/logMessage: {:?}", params);
                    }
                    Notification::ProgressMessage(params)
                        if !self
                            .compositor
                            .has_component(std::any::type_name::<ui::Prompt>()) =>
                    {
                        let editor_view = self
                            .compositor
                            .find::<ui::EditorView>()
                            .expect("expected at least one EditorView");
                        let lsp::ProgressParams {
                            token,
                            value: lsp::ProgressParamsValue::WorkDone(work),
                        } = params;
                        let (title, message, percentage) = match &work {
                            lsp::WorkDoneProgress::Begin(lsp::WorkDoneProgressBegin {
                                title,
                                message,
                                percentage,
                                ..
                            }) => (Some(title), message, percentage),
                            lsp::WorkDoneProgress::Report(lsp::WorkDoneProgressReport {
                                message,
                                percentage,
                                ..
                            }) => (None, message, percentage),
                            lsp::WorkDoneProgress::End(lsp::WorkDoneProgressEnd { message }) => {
                                if message.is_some() {
                                    (None, message, &None)
                                } else {
                                    self.lsp_progress.end_progress(server_id, &token);
                                    if !self.lsp_progress.is_progressing(server_id) {
                                        editor_view.spinners_mut().get_or_create(server_id).stop();
                                        self.editor.lsp_progress = None;
                                    }
                                    self.editor.clear_status();

                                    // we want to render to clear any leftover spinners or messages
                                    return;
                                }
                            }
                        };

                        if self.editor.config().lsp.display_progress_messages {
                            let title =
                                title.or_else(|| self.lsp_progress.title(server_id, &token));
                            if title.is_some() || percentage.is_some() || message.is_some() {
                                use std::fmt::Write as _;
                                let mut status = format!("{}: ", language_server!().name());
                                if let Some(percentage) = percentage {
                                    write!(status, "{percentage:>2}% ").unwrap();
                                }
                                if let Some(title) = title {
                                    status.push_str(title);
                                }
                                if title.is_some() && message.is_some() {
                                    status.push_str(" ⋅ ");
                                }
                                if let Some(message) = message {
                                    status.push_str(message);
                                }
                                self.editor.set_status(status);
                            }
                        }

                        // Mirror the structured progress onto the editor so UI
                        // surfaces (the IDE workbench gauge) can render it,
                        // independent of the display_progress_messages toggle.
                        let server_name = language_server!().name().to_string();
                        match work {
                            lsp::WorkDoneProgress::Begin(begin_status) => {
                                self.editor.lsp_progress = Some(zmax_view::editor::LspProgress {
                                    server: server_name,
                                    title: begin_status.title.clone(),
                                    message: begin_status.message.clone(),
                                    percentage: begin_status.percentage,
                                });
                                self.lsp_progress
                                    .begin(server_id, token.clone(), begin_status);
                            }
                            lsp::WorkDoneProgress::Report(report_status) => {
                                let title = self
                                    .lsp_progress
                                    .title(server_id, &token)
                                    .cloned()
                                    .unwrap_or_default();
                                self.editor.lsp_progress = Some(zmax_view::editor::LspProgress {
                                    server: server_name,
                                    title,
                                    message: report_status.message.clone(),
                                    percentage: report_status.percentage,
                                });
                                self.lsp_progress
                                    .update(server_id, token.clone(), report_status);
                            }
                            lsp::WorkDoneProgress::End(_) => {
                                self.lsp_progress.end_progress(server_id, &token);
                                if !self.lsp_progress.is_progressing(server_id) {
                                    editor_view.spinners_mut().get_or_create(server_id).stop();
                                    self.editor.lsp_progress = None;
                                };
                            }
                        }
                    }
                    Notification::ProgressMessage(_params) => {
                        // do nothing
                    }
                    Notification::Exit => {
                        self.editor.set_status("Language server exited");

                        // LSPs may produce diagnostics for files that haven't been opened in zmax,
                        // we need to clear those and remove the entries from the list if this leads to
                        // an empty diagnostic list for said files
                        for diags in self.editor.diagnostics.values_mut() {
                            diags.retain(|(_, provider)| {
                                provider.language_server_id() != Some(server_id)
                            });
                        }

                        self.editor.diagnostics.retain(|_, diags| !diags.is_empty());

                        // Clear any diagnostics for documents with this server open.
                        for doc in self.editor.documents_mut() {
                            doc.clear_diagnostics_for_language_server(server_id);
                        }

                        zmax_event::dispatch(zmax_view::events::LanguageServerExited {
                            editor: &mut self.editor,
                            server_id,
                        });

                        // Remove the language server from the registry.
                        self.editor.language_servers.remove_by_id(server_id);
                    }
                }
            }
            Call::MethodCall(zmax_lsp::jsonrpc::MethodCall {
                method, params, id, ..
            }) => {
                let reply = match MethodCall::parse(&method, params) {
                    Err(zmax_lsp::Error::Unhandled) => {
                        error!(
                            "Language Server: Method {} not found in request {}",
                            method, id
                        );
                        Err(zmax_lsp::jsonrpc::Error {
                            code: zmax_lsp::jsonrpc::ErrorCode::MethodNotFound,
                            message: format!("Method not found: {}", method),
                            data: None,
                        })
                    }
                    Err(err) => {
                        log::error!(
                            "Language Server: Received malformed method call {} in request {}: {}",
                            method,
                            id,
                            err
                        );
                        Err(zmax_lsp::jsonrpc::Error {
                            code: zmax_lsp::jsonrpc::ErrorCode::ParseError,
                            message: format!("Malformed method call: {}", method),
                            data: None,
                        })
                    }
                    Ok(MethodCall::WorkDoneProgressCreate(params)) => {
                        self.lsp_progress.create(server_id, params.token);

                        let editor_view = self
                            .compositor
                            .find::<ui::EditorView>()
                            .expect("expected at least one EditorView");
                        let spinner = editor_view.spinners_mut().get_or_create(server_id);
                        if spinner.is_stopped() {
                            spinner.start();
                        }

                        Ok(serde_json::Value::Null)
                    }
                    Ok(MethodCall::ApplyWorkspaceEdit(params)) => {
                        let language_server = language_server!();
                        if language_server.is_initialized() {
                            let offset_encoding = language_server.offset_encoding();
                            let res = self
                                .editor
                                .apply_workspace_edit(offset_encoding, &params.edit);

                            Ok(json!(lsp::ApplyWorkspaceEditResponse {
                                applied: res.is_ok(),
                                failure_reason: res.as_ref().err().map(|err| err.kind.to_string()),
                                failed_change: res
                                    .as_ref()
                                    .err()
                                    .map(|err| err.failed_change_idx as u32),
                            }))
                        } else {
                            Err(zmax_lsp::jsonrpc::Error {
                                code: zmax_lsp::jsonrpc::ErrorCode::InvalidRequest,
                                message: "Server must be initialized to request workspace edits"
                                    .to_string(),
                                data: None,
                            })
                        }
                    }
                    Ok(MethodCall::WorkspaceFolders) => {
                        Ok(json!(&*language_server!().workspace_folders().await))
                    }
                    Ok(MethodCall::WorkspaceConfiguration(params)) => {
                        let language_server = language_server!();
                        let result: Vec<_> = params
                            .items
                            .iter()
                            .map(|item| {
                                let mut config = language_server.config()?;
                                if let Some(section) = item.section.as_ref() {
                                    // for some reason some lsps send an empty string (observed in 'vscode-eslint-language-server')
                                    if !section.is_empty() {
                                        for part in section.split('.') {
                                            config = config.get(part)?;
                                        }
                                    }
                                }
                                Some(config)
                            })
                            .collect();
                        Ok(json!(result))
                    }
                    Ok(MethodCall::RegisterCapability(params)) => {
                        if let Some(client) = self.editor.language_servers.get_by_id(server_id) {
                            for reg in params.registrations {
                                match reg.method.as_str() {
                                    lsp::notification::DidChangeWatchedFiles::METHOD => {
                                        let Some(options) = reg.register_options else {
                                            continue;
                                        };
                                        let ops: lsp::DidChangeWatchedFilesRegistrationOptions =
                                            match serde_json::from_value(options) {
                                                Ok(ops) => ops,
                                                Err(err) => {
                                                    log::warn!(
                                                        "Failed to deserialize DidChangeWatchedFilesRegistrationOptions: {err}"
                                                    );
                                                    continue;
                                                }
                                            };
                                        self.editor.language_servers.file_event_handler.register(
                                            client.id(),
                                            Arc::downgrade(client),
                                            reg.id,
                                            ops,
                                        )
                                    }
                                    _ => {
                                        // Language Servers based on the `vscode-languageserver-node` library often send
                                        // client/registerCapability even though we do not enable dynamic registration
                                        // for most capabilities. We should send a MethodNotFound JSONRPC error in this
                                        // case but that rejects the registration promise in the server which causes an
                                        // exit. So we work around this by ignoring the request and sending back an OK
                                        // response.
                                        log::warn!(
                                            "Ignoring a client/registerCapability request because dynamic capability registration is not enabled. Please report this upstream to the language server"
                                        );
                                    }
                                }
                            }
                        }

                        Ok(serde_json::Value::Null)
                    }
                    Ok(MethodCall::UnregisterCapability(params)) => {
                        for unreg in params.unregisterations {
                            match unreg.method.as_str() {
                                lsp::notification::DidChangeWatchedFiles::METHOD => {
                                    self.editor
                                        .language_servers
                                        .file_event_handler
                                        .unregister(server_id, unreg.id);
                                }
                                _ => {
                                    log::warn!(
                                        "Received unregistration request for unsupported method: {}",
                                        unreg.method
                                    );
                                }
                            }
                        }
                        Ok(serde_json::Value::Null)
                    }
                    Ok(MethodCall::ShowDocument(params)) => {
                        let language_server = language_server!();
                        let offset_encoding = language_server.offset_encoding();

                        let result = self.handle_show_document(params, offset_encoding);
                        Ok(json!(result))
                    }
                    Ok(MethodCall::WorkspaceDiagnosticRefresh) => {
                        let language_server = language_server!().id();

                        let documents: Vec<_> = self
                            .editor
                            .documents
                            .values()
                            .filter(|x| x.supports_language_server(language_server))
                            .map(|x| x.id())
                            .collect();

                        for document in documents {
                            handlers::diagnostics::request_document_diagnostics(
                                &mut self.editor,
                                document,
                            );
                        }

                        Ok(serde_json::Value::Null)
                    }
                    Ok(MethodCall::ShowMessageRequest(params)) => {
                        if let Some(actions) = params.actions.filter(|a| !a.is_empty()) {
                            let id = id.clone();
                            let select = ui::Select::new(
                                params.message,
                                actions,
                                (),
                                move |editor, action, event| {
                                    let reply = match event {
                                        ui::PromptEvent::Update => return,
                                        ui::PromptEvent::Validate => Some(action.clone()),
                                        ui::PromptEvent::Abort => None,
                                    };
                                    if let Some(language_server) =
                                        editor.language_server_by_id(server_id)
                                    {
                                        if let Err(err) =
                                            language_server.reply(id.clone(), Ok(json!(reply)))
                                        {
                                            log::error!(
                                                "Failed to send reply to server '{}' request {id}: {err}",
                                                language_server.name()
                                            );
                                        }
                                    }
                                },
                            );
                            self.compositor
                                .replace_or_push("lsp-show-message-request", select);
                            // Avoid sending a reply. The `Select` callback above sends the reply.
                            return;
                        } else {
                            self.handle_show_message(params.typ, params.message);
                            Ok(serde_json::Value::Null)
                        }
                    }
                };

                let language_server = language_server!();
                if let Err(err) = language_server.reply(id.clone(), reply) {
                    log::error!(
                        "Failed to send reply to server '{}' request {id}: {err}",
                        language_server.name()
                    );
                }
            }
            Call::Invalid { id } => log::error!("LSP invalid method call id={:?}", id),
        }
    }

    fn handle_show_message(&mut self, message_type: lsp::MessageType, message: String) {
        if self.config.load().editor.lsp.display_messages {
            match message_type {
                lsp::MessageType::ERROR => self.editor.set_error(message),
                lsp::MessageType::WARNING => self.editor.set_warning(message),
                _ => self.editor.set_status(message),
            }
        }
    }

    fn handle_show_document(
        &mut self,
        params: lsp::ShowDocumentParams,
        offset_encoding: zmax_lsp::OffsetEncoding,
    ) -> lsp::ShowDocumentResult {
        if let lsp::ShowDocumentParams {
            external: Some(true),
            uri,
            ..
        } = params
        {
            self.jobs.callback(crate::open_external_url_callback(uri));
            return lsp::ShowDocumentResult { success: true };
        };

        let lsp::ShowDocumentParams {
            uri,
            selection,
            take_focus,
            ..
        } = params;

        let uri = match zmax_core::Uri::try_from(uri) {
            Ok(uri) => uri,
            Err(err) => {
                log::error!("{err}");
                return lsp::ShowDocumentResult { success: false };
            }
        };
        // If `Uri` gets another variant other than `Path` this may not be valid.
        let path = uri.as_path().expect("URIs are valid paths");

        // Determine the focus strategy based on the current UI state and LSP request:
        // 1. If there is no pop-up overlay, jump directly (Replace).
        // 2. If there is a pop-up overlay, unless the LSP forces take_focus, only load in the background (Load) to prevent interruption of input.
        // Note: We assume layer_count() == 1 means only the EditorView is present (no popups/overlays).
        let action = if self.compositor.layer_count() == 1 {
            zmax_view::editor::Action::Replace
        } else {
            match take_focus {
                Some(true) => zmax_view::editor::Action::Replace,
                _ => zmax_view::editor::Action::Load,
            }
        };

        let doc_id = match self.editor.open(path, action) {
            Ok(id) => id,
            Err(err) => {
                log::error!("failed to open path: {:?}: {:?}", uri, err);
                return lsp::ShowDocumentResult { success: false };
            }
        };

        let doc = doc_mut!(self.editor, &doc_id);
        if let Some(range) = selection {
            // TODO: convert inside server
            if let Some(new_range) = lsp_range_to_range(doc.text(), range, offset_encoding) {
                let view = view_mut!(self.editor);

                // we flip the range so that the cursor sits on the start of the symbol
                // (for example start of the function).
                doc.set_selection(view.id, Selection::single(new_range.head, new_range.anchor));
                if action.align_view(view, doc.id()) {
                    align_view(doc, view, Align::Center);
                }
            } else {
                log::warn!("lsp position out of bounds - {:?}", range);
            };
        };
        lsp::ShowDocumentResult { success: true }
    }

    fn restore_term(&mut self) -> std::io::Result<()> {
        use zmax_view::graphics::CursorKind;
        // vim `titleold`: the title to put back when zmax gives the terminal up
        // (vim uses it when the terminal cannot report its original title).
        if let Some(old) = crate::commands::typed::vim_opt_str("titleold") {
            let _ = self.terminal.backend_mut().set_title(&old);
        }
        self.terminal
            .backend_mut()
            .show_cursor(CursorKind::Block)
            .ok();
        self.terminal.restore()
    }

    /// Run a pending fzf.vim-style request: hand the terminal to the external
    /// `fzf`, then run the request's sink `:` command on the picked line. `fzf`
    /// draws on `/dev/tty`, so we leave the TUI (restore), let it take over, and
    /// re-enter (claim) afterwards; the selection comes back on stdout.
    /// Run a pending full-screen tty command (image-dired's terminal image
    /// viewer): leave the TUI so the child owns the real terminal, run it, then
    /// re-enter and force a full repaint — the same tty handoff `drain_fzf` uses.
    /// Adopt the socket `M-x server-start` bound (or drop a stopped one).
    #[cfg(unix)]
    fn sync_server_listener(&mut self) {
        use std::sync::atomic::Ordering;
        if LISTENER_STOPPED.swap(false, Ordering::Relaxed) {
            self.server_listener = None;
        }
        let pending = PENDING_LISTENER
            .lock()
            .ok()
            .and_then(|mut slot| slot.take());
        if let Some(listener) = pending {
            match tokio::net::UnixListener::from_std(listener) {
                Ok(listener) => self.server_listener = Some(listener),
                Err(err) => {
                    self.editor
                        .set_error(format!("server-start: cannot listen: {err}"));
                }
            }
        }
    }

    #[cfg(not(unix))]
    fn sync_server_listener(&mut self) {}

    /// The accept future of the server socket, or a future that never completes
    /// when no server is running — so the `select!` arm is simply never taken.
    #[cfg(unix)]
    async fn accept_server(listener: Option<&ServerListener>) -> Option<ServerConn> {
        let Some(listener) = listener else {
            return std::future::pending().await;
        };
        match listener.accept().await {
            Ok((stream, _addr)) => Some(stream),
            Err(err) => {
                log::error!("server: accept failed: {err}");
                // Park rather than spin: the arm is rebuilt on the next loop pass.
                std::future::pending().await
            }
        }
    }

    #[cfg(not(unix))]
    async fn accept_server(_listener: Option<&ServerListener>) -> Option<ServerConn> {
        std::future::pending().await
    }

    /// One line back to the client (`done`, `abort`, `ok <value>`, `error …`).
    #[cfg(unix)]
    async fn server_reply(stream: &mut tokio::io::BufReader<ServerConn>, msg: &str) {
        use tokio::io::AsyncWriteExt;
        let line = format!("{msg}\n");
        let _ = stream.get_mut().write_all(line.as_bytes()).await;
        let _ = stream.get_mut().flush().await;
    }

    /// Serve one client (`emacsclient`): read its request, open the files it
    /// asked for / evaluate what it asked for, and — unless it said `-nowait` —
    /// park it holding its socket until `server-edit` releases it.
    #[cfg(unix)]
    async fn handle_server_connection(&mut self, stream: ServerConn) {
        use tokio::io::{AsyncBufReadExt, BufReader};

        let mut reader = BufReader::new(stream);
        let mut request: Vec<String> = Vec::new();
        loop {
            let mut line = String::new();
            let read = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                reader.read_line(&mut line),
            )
            .await;
            match read {
                Ok(Ok(0)) => break,
                Ok(Ok(_)) => {
                    let line = line.trim_end_matches(['\r', '\n']).to_string();
                    if line.is_empty() {
                        break;
                    }
                    request.push(line);
                }
                Ok(Err(err)) => {
                    log::warn!("server: read failed: {err}");
                    return;
                }
                Err(_) => {
                    log::warn!("server: client sent no request in 5s");
                    return;
                }
            }
        }

        let mut auth: Option<&str> = None;
        let mut dir: Option<&str> = None;
        let mut files: Vec<&str> = Vec::new();
        let mut evals: Vec<&str> = Vec::new();
        let mut nowait = false;
        for line in &request {
            let (opt, arg) = match line.split_once(' ') {
                Some((opt, arg)) => (opt, arg.trim()),
                None => (line.as_str(), ""),
            };
            match opt {
                "-auth" => auth = Some(arg),
                "-dir" => dir = Some(arg),
                "-file" | "-position" => files.push(arg),
                "-eval" => evals.push(arg),
                "-nowait" => nowait = true,
                other => log::warn!("server: unknown request option {other}"),
            }
        }

        // `server-generate-key`: a keyed server serves nobody who cannot show it.
        let key = self.editor.server.as_ref().and_then(|s| s.auth_key.clone());
        if let Some(key) = key {
            if auth != Some(key.as_str()) {
                Self::server_reply(&mut reader, "error authentication failed").await;
                return;
            }
        }

        for expr in &evals {
            let mut cx = crate::compositor::Context {
                editor: &mut self.editor,
                jobs: &mut self.jobs,
                scroll: None,
            };
            let msg = match crate::commands::scripting::eval_elisp(&mut cx, expr) {
                Ok(value) => format!("ok {}", value.replace('\n', " ")),
                Err(err) => format!("error {}", err.replace('\n', " ")),
            };
            Self::server_reply(&mut reader, &msg).await;
        }

        let mut docs = Vec::new();
        for spec in &files {
            // `path`, `path:LINE` and `path:LINE:COL`, as emacsclient sends.
            let mut parts = spec.rsplitn(3, ':');
            let (path, line, col): (String, Option<usize>, Option<usize>) = {
                let a = parts.next().unwrap_or("");
                let b = parts.next();
                let c = parts.next();
                match (b.map(str::parse::<usize>), c) {
                    (Some(Ok(b)), Some(c)) if a.parse::<usize>().is_ok() => {
                        (c.to_string(), Some(b), a.parse().ok())
                    }
                    (Some(_), None) if a.parse::<usize>().is_ok() => {
                        (b.unwrap_or_default().to_string(), a.parse().ok(), None)
                    }
                    _ => (spec.to_string(), None, None),
                }
            };
            let path = Path::new(&path);
            let path: std::path::PathBuf = if path.is_absolute() {
                path.to_path_buf()
            } else {
                match dir {
                    Some(dir) => Path::new(dir).join(path),
                    None => path.to_path_buf(),
                }
            };
            match self
                .editor
                .open(&path, zmax_view::editor::Action::Replace)
            {
                Ok(id) => {
                    docs.push(id);
                    if let Some(line) = line {
                        let view_id = self.editor.tree.focus;
                        let doc = doc_mut!(self.editor, &id);
                        let text = doc.text();
                        let line = line
                            .saturating_sub(1)
                            .min(text.len_lines().saturating_sub(1));
                        let pos = text.line_to_char(line)
                            + col
                                .unwrap_or(1)
                                .saturating_sub(1)
                                .min(text.line(line).len_chars().saturating_sub(1));
                        doc.set_selection(view_id, Selection::point(pos.min(text.len_chars())));
                        self.editor.ensure_cursor_in_view(view_id);
                    }
                }
                Err(err) => {
                    Self::server_reply(&mut reader, &format!("error {}: {err}", path.display()))
                        .await;
                    return;
                }
            }
        }

        if docs.is_empty() || nowait {
            Self::server_reply(&mut reader, "done").await;
            return;
        }

        // Park the client, still connected, until `server-edit` says the buffers
        // are done with — that block is the point of the protocol.
        let std_stream = match reader.into_inner().into_std() {
            Ok(stream) => stream,
            Err(err) => {
                log::error!("server: cannot park client: {err}");
                return;
            }
        };
        if let Err(err) = std_stream.set_nonblocking(false) {
            log::error!("server: cannot park client: {err}");
            return;
        }
        if let Some(server) = self.editor.server.as_mut() {
            let id = server.next_client_id;
            server.next_client_id += 1;
            server.clients.push(zmax_view::editor::ServerClient {
                id,
                stream: std_stream,
                docs,
            });
            let waiting = server.clients.len();
            self.editor.set_status(format!(
                "client waiting ({waiting}) — C-x # (server-edit) when done"
            ));
        }
    }

    #[cfg(not(unix))]
    async fn handle_server_connection(&mut self, conn: ServerConn) {
        match conn {}
    }

    async fn drain_tty_command(&mut self) {
        let Some(argv) = self.editor.pending_tty_command.take() else {
            return;
        };
        let Some((prog, args)) = argv.split_first() else {
            return;
        };
        if self.restore_term().is_err() {
            return;
        }
        let status = std::process::Command::new(prog).args(args).status();
        if let Err(e) = status {
            self.editor.set_error(format!("tty command: {e}"));
        }
        for _ in 0..10 {
            if self.terminal.claim().is_ok() {
                break;
            }
        }
        let area = self.terminal.size();
        self.compositor.resize(area);
        let _ = self.terminal.clear();
        self.render().await;
    }

    async fn drain_fzf(&mut self) {
        let Some(req) = self.editor.pending_fzf.take() else {
            return;
        };
        let selection = self.run_fzf(&req);
        if let Some(sel) = selection {
            let sel = sel.trim();
            if !sel.is_empty() && !req.sink.is_empty() {
                let line = req.sink.replace("{}", sel);
                let mut cx = crate::compositor::Context {
                    editor: &mut self.editor,
                    scroll: None,
                    jobs: &mut self.jobs,
                };
                crate::commands::typed::run_command_line(&mut cx, &line);
            }
        }
        self.render().await;
    }

    /// Blocking TTY handoff to `fzf`. Returns the first selected line (or `None`
    /// on cancel / spawn failure). When `candidates` is empty, `fzf` is left to
    /// use its own `$FZF_DEFAULT_COMMAND` (file walk) via the inherited tty.
    fn run_fzf(&mut self, req: &zmax_view::editor::FzfRequest) -> Option<String> {
        use std::io::Write;
        use std::process::{Command, Stdio};

        // Configurable popup size/layout + preview pane (applied on top of the
        // user's own $FZF_DEFAULT_OPTS, which fzf reads itself).
        let (cfg_opts, preview_cmd, preview_win) = {
            let c = self.editor.config();
            (
                c.fzf.options.clone(),
                c.fzf.preview.clone(),
                c.fzf.preview_window.clone(),
            )
        };

        // Compose the child's FZF_DEFAULT_OPTS: the user's own, plus — for
        // file/preview commands (:Files/:Buffers) — their FZF_CTRL_T_OPTS, which
        // is where preview configs conventionally live (fzf reads FZF_CTRL_T_OPTS
        // only in its CTRL-T *widget*, not for a bare `fzf`; we replicate that so
        // :Files matches the shell's CTRL-T file finder). fzf parses the whole
        // string itself, so the complex quoted/multiline preview survives intact.
        let base_opts = std::env::var("FZF_DEFAULT_OPTS").unwrap_or_default();
        let mut fzf_opts = base_opts;
        if req.preview {
            if let Ok(ct) = std::env::var("FZF_CTRL_T_OPTS") {
                fzf_opts.push(' ');
                fzf_opts.push_str(&ct);
            }
            if !preview_cmd.is_empty() {
                fzf_opts.push_str(" --preview '");
                fzf_opts.push_str(&preview_cmd);
                fzf_opts.push('\'');
                if !preview_win.is_empty() {
                    fzf_opts.push_str(" --preview-window ");
                    fzf_opts.push_str(&preview_win);
                }
            }
        }
        for opt in &cfg_opts {
            fzf_opts.push(' ');
            fzf_opts.push_str(opt);
        }
        // Source command: an explicit per-request command (git ls-files, rg, …)
        // wins; else, when finding files with no candidates, the shell's CTRL-T
        // file command.
        let source_cmd = req.command.clone().filter(|s| !s.is_empty()).or_else(|| {
            if req.candidates.is_empty() {
                std::env::var("FZF_CTRL_T_COMMAND")
                    .ok()
                    .filter(|s| !s.is_empty())
            } else {
                None
            }
        });
        let stream_command = req.candidates.is_empty();

        if self.restore_term().is_err() {
            return None;
        }
        // No --prompt override: let the user's own prompt (e.g. ZPWR's
        // `<<)ZPWR(>>` from FZF_DEFAULT_OPTS/FZF_CTRL_T_OPTS) show through. Only
        // per-command flags (e.g. `+m`) are passed as args.
        let _ = &req.prompt;
        let args: Vec<String> = req.options.clone();

        let result = (|| -> std::io::Result<Option<String>> {
            let mut cmd = Command::new("fzf");
            cmd.args(&args)
                .env("FZF_DEFAULT_OPTS", &fzf_opts)
                .stdout(Stdio::piped());
            if let Some(c) = &source_cmd {
                cmd.env("FZF_DEFAULT_COMMAND", c);
            }
            if stream_command {
                cmd.stdin(Stdio::inherit()); // fzf runs FZF_DEFAULT_COMMAND
            } else {
                cmd.stdin(Stdio::piped());
            }
            let mut child = cmd.spawn()?;
            if !stream_command {
                if let Some(mut stdin) = child.stdin.take() {
                    // Write on a thread so a large list can't deadlock the pipe.
                    let data = req.candidates.join("\n");
                    std::thread::spawn(move || {
                        let _ = stdin.write_all(data.as_bytes());
                    });
                }
            }
            let out = child.wait_with_output()?;
            if !out.status.success() {
                return Ok(None); // Esc / Ctrl-C → no pick
            }
            Ok(String::from_utf8_lossy(&out.stdout)
                .lines()
                .next()
                .map(str::to_string))
        })()
        .ok()
        .flatten();

        // Re-enter the TUI regardless of how fzf exited, then force a FULL
        // repaint: fzf drew over our screen while we were out, so the diff
        // cache is stale — clear it (same dance as the SIGCONT/resume path).
        for _ in 0..10 {
            if self.terminal.claim().is_ok() {
                break;
            }
        }
        let area = self.terminal.size();
        self.compositor.resize(area);
        let _ = self.terminal.clear();
        result
    }

    #[cfg(all(not(feature = "integration"), not(windows)))]
    pub fn event_stream(&self) -> impl Stream<Item = std::io::Result<TerminalEvent>> + Unpin {
        use termina::{escape::csi, Terminal as _};
        let reader = self.terminal.backend().terminal().event_reader();
        termina::EventStream::new(reader, |event| {
            // Accept either non-escape sequences or theme mode updates.
            !event.is_escape()
                || matches!(
                    event,
                    termina::Event::Csi(csi::Csi::Mode(csi::Mode::ReportTheme(_)))
                )
        })
    }

    #[cfg(all(not(feature = "integration"), windows))]
    pub fn event_stream(&self) -> impl Stream<Item = std::io::Result<TerminalEvent>> + Unpin {
        crossterm::event::EventStream::new()
    }

    #[cfg(feature = "integration")]
    pub fn event_stream(&self) -> impl Stream<Item = std::io::Result<TerminalEvent>> + Unpin {
        use std::{
            pin::Pin,
            task::{Context, Poll},
        };

        /// A dummy stream that never polls as ready.
        pub struct DummyEventStream;

        impl Stream for DummyEventStream {
            type Item = std::io::Result<TerminalEvent>;

            fn poll_next(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
                Poll::Pending
            }
        }

        DummyEventStream
    }

    pub async fn run<S>(&mut self, input_stream: &mut S) -> Result<i32, Error>
    where
        S: Stream<Item = std::io::Result<TerminalEvent>> + Unpin,
    {
        self.terminal.claim()?;

        self.event_loop(input_stream).await;

        self.save_appdata();
        crate::zmaxinfo::save(&self.editor);

        let close_errs = self.close().await;

        self.restore_term()?;

        for err in close_errs {
            self.editor.exit_code = 1;
            eprintln!("Error: {}", err);
        }

        Ok(self.editor.exit_code)
    }

    /// Persist the session (theme, open tabs, focused file + cursor, drawer layout) to appdata.toml.
    fn save_appdata(&mut self) {
        let mut data = crate::appdata::AppData {
            theme: Some(self.editor.theme.name().to_string()),
            ..Default::default()
        };
        for doc in self.editor.documents() {
            if let Some(path) = doc.path() {
                data.open_files.push(path.to_string_lossy().into_owned());
            }
        }
        // On quit the last view may already be closed, leaving `tree.focus`
        // pointing at the root container — so resolve the focused view safely
        // instead of `current_ref!`, which would panic.
        if let Some((view_id, doc_id)) = self
            .editor
            .tree
            .try_get(self.editor.tree.focus)
            .map(|view| (view.id, view.doc))
        {
            if let Some(doc) = self.editor.documents.get(&doc_id) {
                if let Some(path) = doc.path() {
                    data.focused_file = Some(path.to_string_lossy().into_owned());
                }
                data.cursor = Some(
                    doc.selection(view_id)
                        .primary()
                        .cursor(doc.text().slice(..)),
                );
            }
        }
        if let Some(layout) = self
            .compositor
            .find::<crate::ui::EditorView>()
            .and_then(|ev| ev.ide_layout())
        {
            data.ide = layout;
        } else if let Some(prev) = crate::appdata::load() {
            // The IDE wasn't opened this session (e.g. launched without `--ide`),
            // so there's no live layout to snapshot. Carry forward the previously
            // saved widths/folds instead of clobbering them with zeroed defaults.
            data.ide = prev.ide;
        }
        // Breakpoints — persist the user-set fields per file.
        data.breakpoints = self
            .editor
            .breakpoints
            .iter()
            .filter(|(_, bps)| !bps.is_empty())
            .map(|(path, bps)| crate::appdata::FileBreakpoints {
                path: path.to_string_lossy().into_owned(),
                breakpoints: bps
                    .iter()
                    .map(|b| crate::appdata::BreakpointData {
                        line: b.line,
                        column: b.column,
                        condition: b.condition.clone(),
                        hit_condition: b.hit_condition.clone(),
                        log_message: b.log_message.clone(),
                    })
                    .collect(),
            })
            .collect();
        // Window split layout (arrangement + files + focus). Cursors for the
        // focused view are already captured in `data.cursor` above. Split
        // persistence (`shape_to_split`) is compiled out under `integration`
        // (the test harness has no session to restore), so gate the capture to
        // match — otherwise the reference fails to resolve in that build.
        #[cfg(not(feature = "integration"))]
        {
            let shape = self.editor.tree.shape();
            data.splits = Some(shape_to_split(&self.editor, &shape));
        }
        crate::appdata::save(&data);
    }

    pub async fn close(&mut self) -> Vec<anyhow::Error> {
        // [NOTE] we intentionally do not return early for errors because we
        //        want to try to run as much cleanup as we can, regardless of
        //        errors along the way
        let mut errs = Vec::new();

        if let Err(err) = self
            .jobs
            .finish(&mut self.editor, Some(&mut self.compositor))
            .await
        {
            log::error!("Error executing job: {}", err);
            errs.push(err);
        };

        if let Err(err) = self.editor.flush_writes().await {
            log::error!("Error writing: {}", err);
            errs.push(err);
        }

        self.editor.close_language_servers(None).await;

        errs
    }
}

/// Restore a previous session into `editor`: background-load every persisted tab,
/// then replace the focused view with the last-focused file and restore its cursor.
/// Persisted paths that no longer point at a regular file are skipped.
///
/// Returns whether a focused view ended up present (i.e. the session was non-empty
/// and at least the focused file reopened). `false` means the caller should fall
/// back to a scratch buffer + start screen.
#[cfg(not(feature = "integration"))]
fn restore_session(appdata: Option<&crate::appdata::AppData>, editor: &mut Editor) -> bool {
    use zmax_view::editor::{Action, Breakpoint};

    let Some(data) = appdata else { return false };

    // Debugger breakpoints — restore the user-set fields (runtime id/verified are
    // re-established when a debug session attaches). Independent of open files.
    for fb in &data.breakpoints {
        let bps: Vec<Breakpoint> = fb
            .breakpoints
            .iter()
            .map(|b| Breakpoint {
                id: None,
                verified: false,
                message: None,
                line: b.line,
                column: b.column,
                condition: b.condition.clone(),
                hit_condition: b.hit_condition.clone(),
                log_message: b.log_message.clone(),
            })
            .collect();
        if !bps.is_empty() {
            editor
                .breakpoints
                .insert(std::path::PathBuf::from(&fb.path), bps);
        }
    }

    // Prefer the full split layout when a previous session saved one.
    if let Some(node) = &data.splits {
        if restore_splits(node, editor) {
            if let Some(pos) = data.cursor {
                let view_id = editor.tree.focus;
                if editor.tree.try_get(view_id).is_some() {
                    let doc_id = editor.tree.get(view_id).doc;
                    let doc = doc_mut!(editor, &doc_id);
                    let pos = pos.min(doc.text().len_chars());
                    doc.set_selection(view_id, Selection::point(pos));
                }
            }
            return editor.tree.try_get(editor.tree.focus).is_some();
        }
    }

    // Fallback: flat reopen (sessions saved before the split tree existed).
    if data.open_files.is_empty() {
        return false;
    }
    for file in &data.open_files {
        let path = std::path::Path::new(file);
        if path.is_file() {
            let _ = editor.open(path, Action::Load);
        }
    }
    if let Some(focused) = &data.focused_file {
        let focused_path = std::path::Path::new(focused);
        if focused_path.is_file() {
            if let Ok(doc_id) = editor.open(focused_path, Action::Replace) {
                let view_id = editor.tree.focus;
                if let Some(pos) = data.cursor {
                    let doc = doc_mut!(editor, &doc_id);
                    let pos = pos.min(doc.text().len_chars());
                    doc.set_selection(view_id, Selection::point(pos));
                }
            }
        }
    }
    // A focused view only exists if `focused_file` actually reopened; background
    // `Action::Load` calls create no view on their own.
    editor.tree.try_get(editor.tree.focus).is_some()
}

/// Capture the live window split tree as a persistable [`crate::appdata::SplitNode`]
/// (paths + horizontal/vertical arrangement + weights).
#[cfg(not(feature = "integration"))]
fn shape_to_split(
    editor: &Editor,
    shape: &zmax_view::tree::TreeShape,
) -> crate::appdata::SplitNode {
    use zmax_view::tree::{Layout, TreeShape};
    match shape {
        TreeShape::Leaf { doc, focused } => {
            let path = editor
                .documents
                .get(doc)
                .and_then(|d| d.path())
                .map(|p| p.to_string_lossy().into_owned());
            crate::appdata::SplitNode {
                kind: "leaf".into(),
                weight: 1.0,
                path,
                focused: *focused,
                cursor: None,
                children: Vec::new(),
            }
        }
        TreeShape::Split { layout, children } => crate::appdata::SplitNode {
            kind: if matches!(layout, Layout::Horizontal) {
                "h"
            } else {
                "v"
            }
            .into(),
            weight: 1.0,
            path: None,
            focused: false,
            cursor: None,
            children: children
                .iter()
                .map(|(w, c)| {
                    let mut n = shape_to_split(editor, c);
                    n.weight = *w;
                    n
                })
                .collect(),
        },
    }
}

/// Reopen every file referenced by `node` and rebuild the split tree from it.
/// Returns false (caller falls back to flat restore) if nothing usable remains.
#[cfg(not(feature = "integration"))]
fn restore_splits(node: &crate::appdata::SplitNode, editor: &mut Editor) -> bool {
    use zmax_view::editor::Action;

    let mut paths = Vec::new();
    collect_leaf_paths(node, &mut paths);
    if paths.is_empty() {
        return false;
    }
    let mut map: std::collections::HashMap<String, zmax_view::DocumentId> = Default::default();
    for p in &paths {
        if map.contains_key(p) {
            continue;
        }
        let path = std::path::Path::new(p);
        if path.is_file() {
            if let Ok(doc_id) = editor.open(path, Action::Load) {
                map.insert(p.clone(), doc_id);
            }
        }
    }
    let Some(shape) = node_to_shape(node, &map) else {
        return false;
    };
    let gutters = editor.config().gutters.clone();
    let mut make = |doc| zmax_view::view::View::new(doc, gutters.clone());
    editor.tree.build_from_shape(&shape, &mut make);
    true
}

#[cfg(not(feature = "integration"))]
fn collect_leaf_paths(node: &crate::appdata::SplitNode, out: &mut Vec<String>) {
    if node.kind == "leaf" {
        if let Some(p) = &node.path {
            out.push(p.clone());
        }
    } else {
        for c in &node.children {
            collect_leaf_paths(c, out);
        }
    }
}

/// Convert a persisted [`crate::appdata::SplitNode`] into a live [`zmax_view::tree::TreeShape`],
/// dropping leaves whose file failed to open and collapsing now-empty splits.
#[cfg(not(feature = "integration"))]
fn node_to_shape(
    node: &crate::appdata::SplitNode,
    map: &std::collections::HashMap<String, zmax_view::DocumentId>,
) -> Option<zmax_view::tree::TreeShape> {
    use zmax_view::tree::{Layout, TreeShape};
    if node.kind == "leaf" {
        let path = node.path.as_ref()?;
        let doc = *map.get(path)?;
        Some(TreeShape::Leaf {
            doc,
            focused: node.focused,
        })
    } else {
        let layout = if node.kind == "h" {
            Layout::Horizontal
        } else {
            Layout::Vertical
        };
        let children: Vec<(f32, TreeShape)> = node
            .children
            .iter()
            .filter_map(|c| node_to_shape(c, map).map(|s| (c.weight, s)))
            .collect();
        match children.len() {
            0 => None,
            1 => Some(children.into_iter().next().unwrap().1),
            _ => Some(TreeShape::Split { layout, children }),
        }
    }
}

impl ui::menu::Item for lsp::MessageActionItem {
    type Data = ();
    fn format(&self, _data: &Self::Data) -> tui::widgets::Row<'_> {
        self.title.as_str().into()
    }
}

#[cfg(test)]
mod vim_option_tests {
    use super::{truncate_title, write_message};

    /// vim `shortmess`: `W` drops the write message, `w` shortens "written" to
    /// "[w]", `l` picks the short size form (both are in vim's default `ltToOCF`,
    /// which is the message zmax has always printed), and `a` implies them all.
    #[test]
    fn shortmess_shapes_the_write_message() {
        let msg = |shm: &str| write_message("src/main.rs", 12, 1234, "1.2KiB", shm);

        // vim's default value — unchanged from what zmax always showed.
        assert_eq!(
            msg("ltToOCF").as_deref(),
            Some("'src/main.rs' written, 12L 1.2KiB")
        );

        // `:set shortmess=` — the long, unabbreviated form.
        assert_eq!(
            msg("").as_deref(),
            Some("'src/main.rs' written, 12 lines, 1234 bytes")
        );

        // `w` abbreviates "written".
        assert_eq!(msg("lw").as_deref(), Some("'src/main.rs' [w], 12L 1.2KiB"));

        // `a` = all of the abbreviations.
        assert_eq!(msg("a").as_deref(), Some("'src/main.rs' [w], 12L 1.2KiB"));

        // `W` = no message at all.
        assert_eq!(msg("ltToOCFW"), None);
        assert_eq!(msg("W"), None);
    }

    /// vim `titlelen`: the title may use at most this percentage of the screen
    /// width; `titlelen=0` means no title at all.
    #[test]
    fn titlelen_caps_the_window_title() {
        let title = "a-very-long-file-name.rs - zmax";
        assert_eq!(title.chars().count(), 33);

        // Unset: zmax's title, untouched.
        assert_eq!(truncate_title(title, None, 40), title);

        // 85% (vim's default) of 80 columns = 68 => it fits.
        assert_eq!(truncate_title(title, Some(85), 80), title);

        // 50% of 40 columns = 20 => cut to 20 cells, ellipsis included.
        let cut = truncate_title(title, Some(50), 40);
        assert_eq!(cut.chars().count(), 20);
        assert!(cut.ends_with('…'), "{cut}");
        assert!(title.starts_with(&cut[..cut.len() - '…'.len_utf8()]));

        // `titlelen=0` clears the title.
        assert_eq!(truncate_title(title, Some(0), 80), "");
    }
}
