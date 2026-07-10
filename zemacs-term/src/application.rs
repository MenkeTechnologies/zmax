use arc_swap::{access::Map, ArcSwap};
use futures_util::Stream;
use serde_json::json;
use tui::backend::Backend;
use zemacs_core::{diagnostic::Severity, pos_at_coords, syntax, Range, Selection};
use zemacs_lsp::{
    lsp::{self, notification::Notification},
    util::lsp_range_to_range,
    LanguageServerId, LspProgressMap,
};
use zemacs_stdx::path::get_relative_path;
use zemacs_view::{
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

pub struct Application {
    compositor: Compositor,
    terminal: Terminal,
    pub editor: Editor,

    config: Arc<ArcSwap<Config>>,

    signals: Signals,
    jobs: Jobs,
    lsp_progress: LspProgressMap,

    theme_mode: Option<theme::Mode>,
}

#[cfg(feature = "integration")]
fn setup_integration_logging() {
    let level = std::env::var("ZEMACS_LOG_LEVEL")
        .map(|lvl| lvl.parse().unwrap())
        .unwrap_or(log::LevelFilter::Info);

    crate::logging::init_stdout(level);
}

impl Application {
    pub fn new(
        args: Args,
        config: Config,
        lang_loader: syntax::Loader,
        workspace_trust: zemacs_loader::workspace_trust::WorkspaceTrust,
    ) -> Result<Self, Error> {
        #[cfg(feature = "integration")]
        setup_integration_logging();

        use zemacs_view::editor::Action;

        let mut theme_parent_dirs = vec![zemacs_loader::config_dir()];
        theme_parent_dirs.extend(zemacs_loader::runtime_dirs().iter().cloned());
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
        Self::load_configured_theme(&mut editor, &config.load(), &mut terminal, theme_mode);

        // Restore vim global marks (`A`-`Z`) and numbered file marks (`0`-`9`)
        // from the previous session's `.zemacsinfo`, so `` `A ``/`` `3 `` jump
        // across restarts. Buffer-local `a`-`z` marks stay per-document.
        editor.global_marks = crate::zemacsinfo::load();

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
        // Opening a directory (`zemacs .`) boots the IDE workbench with a scratch
        // buffer instead of a file picker — the file tree is the point.
        let dir_arg = args.files.first().is_some_and(|(p, _)| p.is_dir());
        let reopen_ide = appdata.as_ref().is_some_and(|d| d.ide.open);
        if args.ide || reopen_ide || dir_arg {
            editor_view.open_sidebar();
        }
        compositor.push(Box::new(editor_view));

        let jobs = Jobs::new();

        if args.load_tutor {
            let path = zemacs_loader::runtime_file(Path::new("tutor"));
            editor.open(&path, Action::VerticalSplit)?;
            // Unset path to prevent accidentally saving to the original tutor file.
            doc_mut!(editor).set_path(None);
        } else if !args.files.is_empty() {
            let mut files_it = args.files.into_iter().peekable();

            // A leading directory (`zemacs .`) just boots IDE mode (handled above
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
                use zemacs_view::editor::StartupScreen;
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

        zemacs_event::start_frame();
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
                    format!("{name} - zemacs")
                } else {
                    cfg.title_string.replace("%f", &full).replace("%t", &name)
                };
                static LAST_TITLE: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());
                if let Ok(mut last) = LAST_TITLE.lock() {
                    if *last != title {
                        *last = title.clone();
                        let _ = self.terminal.backend_mut().set_title(&title);
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
                return false;
            }

            use futures_util::StreamExt;

            tokio::select! {
                biased;

                Some(signal) = self.signals.next() => {
                    if !self.handle_signals(signal).await {
                        return false;
                    };
                }
                Some(event) = input_stream.next() => {
                    self.handle_terminal_events(event).await;
                    // A command may have queued an external-fzf request.
                    self.drain_fzf().await;
                }
                Some(callback) = self.jobs.callbacks.recv() => {
                    if let Some(job) = self.jobs.handle_callback(&mut self.editor, &mut self.compositor, Ok(Some(callback))) {
                        self.jobs.add(job);
                    }
                    self.render().await;
                }
                Some(msg) = self.jobs.status_messages.recv() => {
                    let severity = match msg.severity{
                        zemacs_event::status::Severity::Hint => Severity::Hint,
                        zemacs_event::status::Severity::Info => Severity::Info,
                        zemacs_event::status::Severity::Warning => Severity::Warning,
                        zemacs_event::status::Severity::Error => Severity::Error,
                    };
                    // TODO: show multiple status messages at once to avoid clobbering
                    self.editor.status_msg = Some((msg.message, severity));
                    zemacs_event::request_redraw();
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
            }

            // for integration tests only, reset the idle timer after every
            // event to signal when test events are done processing
            #[cfg(feature = "integration")]
            {
                self.editor.reset_idle_timer();
            }
        }
    }

    /// Load embedded-scripting init files (currently `~/.zemacs/init.el`) once
    /// the editor is constructed. Best-effort: errors surface on the status line.
    pub fn load_init_scripts(&mut self) {
        let mut cx = crate::compositor::Context {
            editor: &mut self.editor,
            jobs: &mut self.jobs,
            scroll: None,
        };
        crate::commands::scripting::load_init_scripts(&mut cx);
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
                let _ = self.terminal.backend_mut().set_background_color(
                    self.editor
                        .theme
                        .try_get_exact("ui.background")
                        .and_then(|style| style.bg),
                );
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
            let lang_loader = zemacs_core::config::user_lang_loader(&self.editor.workspace_trust)?;
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
        // resolved `zgui-<scheme>` theme overrides the configured/persisted one.
        // Falls through to the normal config theme if zwire names nothing usable
        // or the theme can't be loaded under the current color support.
        if config.editor.sync_zwire_theme {
            if let Some(theme) = crate::zwire::theme_name().and_then(|name| {
                editor
                    .theme_loader
                    .load(&name)
                    .map_err(|e| log::warn!("failed to load zwire theme `{}` - {}", name, e))
                    .ok()
                    .filter(|theme| true_color || theme.is_16_color())
            }) {
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
                // - zemacs must have permissions to send signals to all processes in its signal
                //   group, either by already having the requisite permission, or by having the
                //   user's UID / EUID / SUID match that of the receiving process(es).
                let res = unsafe {
                    // A pid of 0 sends the signal to the entire process group, allowing the user to
                    // regain control of their terminal if the editor was spawned under another process
                    // (e.g. when running `git commit`).
                    //
                    // We have to send SIGSTOP (not SIGTSTP) to the entire process group, because,
                    // as mentioned above, the terminal will get stuck if `zemacs` was spawned from
                    // an external process and that process waits for `zemacs` to complete. This may
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
        self.sync_zwire_theme();
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

    /// Live-follow the zwire host scheme while idle: if `sync-zwire-theme` is on
    /// and `~/.zwire/global.toml` now names a different theme than the one in
    /// use, apply it. Stateless (compares against the active theme name), so a
    /// scheme change in zwire is picked up within one idle tick; a no-change
    /// tick only re-reads a ~100-byte file and does nothing.
    fn sync_zwire_theme(&mut self) {
        if !self.config.load().editor.sync_zwire_theme {
            return;
        }
        let Some(name) = crate::zwire::theme_name() else {
            return;
        };
        if self.editor.theme.name() == name {
            return;
        }
        let true_color = self.terminal.backend().supports_true_color()
            || self.config.load().editor.true_color
            || crate::true_color();
        match self.editor.theme_loader.load(&name) {
            Ok(theme) if true_color || theme.is_16_color() => {
                let _ = self.editor.set_theme(theme);
            }
            Ok(_) => {}
            Err(e) => log::warn!("failed to load zwire theme `{}` - {}", name, e),
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
        self.editor.set_status(format!(
            "'{}' written, {lines}L {size}",
            get_relative_path(&doc_save_event.path).to_string_lossy(),
        ));
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
                zemacs_event::request_redraw();
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
        call: zemacs_lsp::Call,
        server_id: LanguageServerId,
    ) {
        use zemacs_lsp::{Call, MethodCall, Notification};

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
            Call::Notification(zemacs_lsp::jsonrpc::Notification { method, params, .. }) => {
                let notification = match Notification::parse(&method, params) {
                    Ok(notification) => notification,
                    Err(zemacs_lsp::Error::Unhandled) => {
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

                        zemacs_event::dispatch(zemacs_view::events::LanguageServerInitialized {
                            editor: &mut self.editor,
                            server_id,
                        });
                    }
                    Notification::PublishDiagnostics(params) => {
                        let uri = match zemacs_core::Uri::try_from(params.uri) {
                            Ok(uri) => uri,
                            Err(err) => {
                                log::error!("{err}");
                                return;
                            }
                        };
                        let language_server = language_server!();
                        if !language_server.is_initialized() {
                            log::error!("Discarding publishDiagnostic notification sent by an uninitialized server: {}", language_server.name());
                            return;
                        }
                        let provider = zemacs_core::diagnostic::DiagnosticProvider::Lsp {
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
                                self.editor.lsp_progress = Some(zemacs_view::editor::LspProgress {
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
                                self.editor.lsp_progress = Some(zemacs_view::editor::LspProgress {
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

                        // LSPs may produce diagnostics for files that haven't been opened in zemacs,
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

                        zemacs_event::dispatch(zemacs_view::events::LanguageServerExited {
                            editor: &mut self.editor,
                            server_id,
                        });

                        // Remove the language server from the registry.
                        self.editor.language_servers.remove_by_id(server_id);
                    }
                }
            }
            Call::MethodCall(zemacs_lsp::jsonrpc::MethodCall {
                method, params, id, ..
            }) => {
                let reply = match MethodCall::parse(&method, params) {
                    Err(zemacs_lsp::Error::Unhandled) => {
                        error!(
                            "Language Server: Method {} not found in request {}",
                            method, id
                        );
                        Err(zemacs_lsp::jsonrpc::Error {
                            code: zemacs_lsp::jsonrpc::ErrorCode::MethodNotFound,
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
                        Err(zemacs_lsp::jsonrpc::Error {
                            code: zemacs_lsp::jsonrpc::ErrorCode::ParseError,
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
                            Err(zemacs_lsp::jsonrpc::Error {
                                code: zemacs_lsp::jsonrpc::ErrorCode::InvalidRequest,
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
                                                    log::warn!("Failed to deserialize DidChangeWatchedFilesRegistrationOptions: {err}");
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
                                        log::warn!("Ignoring a client/registerCapability request because dynamic capability registration is not enabled. Please report this upstream to the language server");
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
                                    log::warn!("Received unregistration request for unsupported method: {}", unreg.method);
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
        offset_encoding: zemacs_lsp::OffsetEncoding,
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

        let uri = match zemacs_core::Uri::try_from(uri) {
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
            zemacs_view::editor::Action::Replace
        } else {
            match take_focus {
                Some(true) => zemacs_view::editor::Action::Replace,
                _ => zemacs_view::editor::Action::Load,
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
        use zemacs_view::graphics::CursorKind;
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
    fn run_fzf(&mut self, req: &zemacs_view::editor::FzfRequest) -> Option<String> {
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
        crate::zemacsinfo::save(&self.editor);

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
    use zemacs_view::editor::{Action, Breakpoint};

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
    shape: &zemacs_view::tree::TreeShape,
) -> crate::appdata::SplitNode {
    use zemacs_view::tree::{Layout, TreeShape};
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
    use zemacs_view::editor::Action;

    let mut paths = Vec::new();
    collect_leaf_paths(node, &mut paths);
    if paths.is_empty() {
        return false;
    }
    let mut map: std::collections::HashMap<String, zemacs_view::DocumentId> = Default::default();
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
    let mut make = |doc| zemacs_view::view::View::new(doc, gutters.clone());
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

/// Convert a persisted [`crate::appdata::SplitNode`] into a live [`zemacs_view::tree::TreeShape`],
/// dropping leaves whose file failed to open and collapsing now-empty splits.
#[cfg(not(feature = "integration"))]
fn node_to_shape(
    node: &crate::appdata::SplitNode,
    map: &std::collections::HashMap<String, zemacs_view::DocumentId>,
) -> Option<zemacs_view::tree::TreeShape> {
    use zemacs_view::tree::{Layout, TreeShape};
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
