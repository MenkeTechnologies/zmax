use std::fmt::Write;
use std::io::BufReader;
use std::ops::{self, Deref};

use crate::job::Job;

use super::*;

use zemacs_core::command_line::{Args, Flag, Signature, Token, TokenKind};
use zemacs_core::fuzzy::fuzzy_match;
use zemacs_core::indent::MAX_INDENT;
use zemacs_core::line_ending;
use zemacs_stdx::path::home_dir;
use zemacs_view::document::{read_to_string, DEFAULT_LANGUAGE_NAME};
use zemacs_view::editor::{CloseError, ConfigEvent};
use zemacs_view::expansion;
use serde_json::Value;
use ui::completers::{self, Completer};

#[derive(Clone)]
pub struct TypableCommand {
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub doc: &'static str,
    // params, flags, helper, completer
    pub fun: fn(&mut compositor::Context, Args, PromptEvent) -> anyhow::Result<()>,
    /// What completion methods, if any, does this command have?
    pub completer: CommandCompleter,
    pub signature: Signature,
}

#[derive(Clone)]
pub struct CommandCompleter {
    // Arguments with specific completion methods based on their position.
    positional_args: &'static [Completer],

    // All remaining arguments will use this completion method, if set.
    var_args: Completer,
}

impl CommandCompleter {
    const fn none() -> Self {
        Self {
            positional_args: &[],
            var_args: completers::none,
        }
    }

    const fn positional(completers: &'static [Completer]) -> Self {
        Self {
            positional_args: completers,
            var_args: completers::none,
        }
    }

    const fn all(completer: Completer) -> Self {
        Self {
            positional_args: &[],
            var_args: completer,
        }
    }

    fn for_argument_number(&self, n: usize) -> &Completer {
        match self.positional_args.get(n) {
            Some(completer) => completer,
            _ => &self.var_args,
        }
    }
}

fn exit(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    if doc!(cx.editor).is_modified() {
        write_impl(
            cx,
            args.first(),
            WriteOptions {
                force: false,
                auto_format: !args.has_flag(WRITE_NO_FORMAT_FLAG.name),
                code_actions: !args.has_flag(WRITE_NO_CODE_ACTIONS_FLAG.name),
            },
        )?;
    }
    cx.block_try_flush_writes()?;
    quit(cx, Args::default(), event)
}

fn force_exit(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    if doc!(cx.editor).is_modified() {
        write_impl(
            cx,
            args.first(),
            WriteOptions {
                force: true,
                auto_format: !args.has_flag(WRITE_NO_FORMAT_FLAG.name),
                code_actions: !args.has_flag(WRITE_NO_CODE_ACTIONS_FLAG.name),
            },
        )?;
    }
    cx.block_try_flush_writes()?;
    quit(cx, Args::default(), event)
}

fn quit(cx: &mut compositor::Context, _args: Args, event: PromptEvent) -> anyhow::Result<()> {
    log::debug!("quitting...");

    if event != PromptEvent::Validate {
        return Ok(());
    }

    // last view and we have unsaved changes
    if cx.editor.tree.views().count() == 1 {
        buffers_remaining_impl(cx.editor)?
    }

    cx.block_try_flush_writes()?;
    cx.editor.close(view!(cx.editor).id);

    Ok(())
}

fn force_quit(cx: &mut compositor::Context, _args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    cx.block_try_flush_writes()?;
    cx.editor.close(view!(cx.editor).id);

    Ok(())
}

fn open(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    open_impl(cx, args, Action::Replace)
}

fn open_impl(cx: &mut compositor::Context, args: Args, action: Action) -> anyhow::Result<()> {
    for arg in args {
        let (path, pos) = crate::args::parse_file(&arg);
        let path = zemacs_stdx::path::expand_tilde(path);
        // If the path is a directory, open a file picker on that directory and update the status
        // message
        if let Ok(true) = std::fs::canonicalize(&path).map(|p| p.is_dir()) {
            let callback = async move {
                let call: job::Callback = job::Callback::EditorCompositor(Box::new(
                    move |editor: &mut Editor, compositor: &mut Compositor| {
                        let picker =
                            ui::file_picker(editor, path.into_owned()).with_default_action(action);
                        compositor.push(Box::new(overlaid(picker)));
                    },
                ));
                Ok(call)
            };
            cx.jobs.callback(callback);
        } else {
            // Otherwise, just open the file
            let _ = cx.editor.open(&path, action)?;
            let (view, doc) = current!(cx.editor);
            let pos = Selection::point(pos_at_coords(doc.text().slice(..), pos, true));
            doc.set_selection(view.id, pos);
            // does not affect opening a buffer without pos
            align_view(doc, view, Align::Center);
        }
    }
    Ok(())
}

fn buffer_close_by_ids_impl(
    cx: &mut compositor::Context,
    doc_ids: &[DocumentId],
    force: bool,
) -> anyhow::Result<()> {
    cx.block_try_flush_writes()?;

    let (modified_ids, modified_names): (Vec<_>, Vec<_>) = doc_ids
        .iter()
        .filter_map(|&doc_id| {
            if let Err(CloseError::BufferModified(name)) = cx.editor.close_document(doc_id, force) {
                Some((doc_id, name))
            } else {
                None
            }
        })
        .unzip();

    if let Some(first) = modified_ids.first() {
        let current = doc!(cx.editor);
        // If the current document is unmodified, and there are modified
        // documents, switch focus to the first modified doc.
        if !modified_ids.contains(&current.id()) {
            cx.editor.switch(*first, Action::Replace);
        }
        bail!(
            "{} unsaved buffer{} remaining: {:?}",
            modified_names.len(),
            if modified_names.len() == 1 { "" } else { "s" },
            modified_names,
        );
    }

    Ok(())
}

fn buffer_gather_paths_impl(editor: &mut Editor, args: Args) -> Vec<DocumentId> {
    // No arguments implies current document
    if args.is_empty() {
        let doc_id = view!(editor).doc;
        return vec![doc_id];
    }

    let mut nonexistent_buffers = vec![];
    let mut document_ids = vec![];
    for arg in args {
        let doc_id = editor.documents().find_map(|doc| {
            let arg_path = Some(Path::new(arg.as_ref()));
            if doc.path() == arg_path || doc.relative_path() == arg_path {
                Some(doc.id())
            } else {
                None
            }
        });

        match doc_id {
            Some(doc_id) => document_ids.push(doc_id),
            None => nonexistent_buffers.push(format!("'{}'", arg)),
        }
    }

    if !nonexistent_buffers.is_empty() {
        editor.set_error(format!(
            "cannot close non-existent buffers: {}",
            nonexistent_buffers.join(", ")
        ));
    }

    document_ids
}

fn buffer_close(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    let document_ids = buffer_gather_paths_impl(cx.editor, args);
    buffer_close_by_ids_impl(cx, &document_ids, false)
}

fn force_buffer_close(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    let document_ids = buffer_gather_paths_impl(cx.editor, args);
    buffer_close_by_ids_impl(cx, &document_ids, true)
}

fn buffer_gather_others_impl(editor: &mut Editor, skip_visible: bool) -> Vec<DocumentId> {
    if skip_visible {
        let visible_document_ids = editor
            .tree
            .views()
            .map(|view| &view.0.doc)
            .collect::<HashSet<_>>();
        editor
            .documents()
            .map(|doc| doc.id())
            .filter(|doc_id| !visible_document_ids.contains(doc_id))
            .collect()
    } else {
        let current_document = &doc!(editor).id();
        editor
            .documents()
            .map(|doc| doc.id())
            .filter(|doc_id| doc_id != current_document)
            .collect()
    }
}

fn buffer_close_others(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    let document_ids = buffer_gather_others_impl(cx.editor, args.has_flag("skip-visible"));
    buffer_close_by_ids_impl(cx, &document_ids, false)
}

fn force_buffer_close_others(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    let document_ids = buffer_gather_others_impl(cx.editor, args.has_flag("skip-visible"));
    buffer_close_by_ids_impl(cx, &document_ids, true)
}

fn buffer_gather_all_impl(editor: &mut Editor) -> Vec<DocumentId> {
    editor.documents().map(|doc| doc.id()).collect()
}

fn buffer_close_all(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    let document_ids = buffer_gather_all_impl(cx.editor);
    buffer_close_by_ids_impl(cx, &document_ids, false)
}

fn force_buffer_close_all(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    let document_ids = buffer_gather_all_impl(cx.editor);
    buffer_close_by_ids_impl(cx, &document_ids, true)
}

fn buffer_next(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    goto_buffer(cx.editor, Direction::Forward, 1);
    Ok(())
}

fn buffer_previous(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    goto_buffer(cx.editor, Direction::Backward, 1);
    Ok(())
}

fn write_impl(
    cx: &mut compositor::Context,
    path: Option<&str>,
    options: WriteOptions,
) -> anyhow::Result<()> {
    let config = cx.editor.config();
    let (view, doc) = current!(cx.editor);
    let doc_id = doc.id();
    let view_id = view.id;

    if doc.trim_trailing_whitespace() {
        trim_trailing_whitespace(doc, view_id);
    }
    if config.trim_final_newlines {
        trim_final_newlines(doc, view_id);
    }
    if doc.insert_final_newline() {
        insert_final_newline(doc, view_id);
    }

    // Save an undo checkpoint for any outstanding changes.
    doc.append_changes_to_history(view);

    let auto_format = config.auto_format && options.auto_format;
    let force = options.force;
    let path: Option<PathBuf> = path.map(Into::into);

    // Does the document configure any code actions to run on save?
    let run_code_actions = options.code_actions
        && doc!(cx.editor, &doc_id)
            .language_config()
            .and_then(|c| c.code_actions_on_save.as_deref())
            .is_some_and(|kinds| !kinds.is_empty());

    // The tail of the on-save chain: re-build the auto-format job against the
    // latest document (so it formats after any code-action edits), or save
    // directly when there's no formatter. Deferred via `Followup`, and always
    // saves, so code-actions-on-save works even with auto-format off. Only
    // built when there is pre-save work. A plain `:w` saves synchronously below.
    let tail = (auto_format || run_code_actions).then(|| {
        let path = path.clone();
        let callback = Callback::Followup(Box::new(move |editor| {
            let doc = doc!(editor, &doc_id);
            let fmt_job = auto_format
                .then(|| doc.auto_format(editor))
                .flatten()
                .map(|fmt| {
                    let call = make_format_callback(
                        doc_id,
                        doc.version(),
                        view_id,
                        fmt,
                        Some((path.clone(), force)),
                    );
                    Job::with_callback(call).wait_before_exiting()
                });
            if fmt_job.is_none() {
                if let Err(err) = editor.save(doc_id, path, force) {
                    editor.set_error(format!("Error saving: {}", err));
                }
            }
            fmt_job
        }));
        Job::with_callback(async { Ok(callback) }).wait_before_exiting()
    });

    let job = if run_code_actions {
        code_actions_on_save(cx, doc_id, tail)
    } else {
        tail
    };

    if let Some(job) = job {
        cx.jobs.add(job);
    } else {
        cx.editor.save(doc_id, path, force)?;
    }

    Ok(())
}

/// Trim all whitespace preceding line-endings in a document.
fn trim_trailing_whitespace(doc: &mut Document, view_id: ViewId) {
    let text = doc.text();
    let mut pos = 0;
    let transaction = Transaction::delete(
        text,
        text.lines().filter_map(|line| {
            let line_end_len_chars = line_ending::get_line_ending(&line)
                .map(|le| le.len_chars())
                .unwrap_or_default();
            // Char after the last non-whitespace character or the beginning of the line if the
            // line is all whitespace:
            let first_trailing_whitespace =
                pos + line.last_non_whitespace_char().map_or(0, |idx| idx + 1);
            pos += line.len_chars();
            // Char before the line ending character(s), or the final char in the text if there
            // is no line-ending on this line:
            let line_end = pos - line_end_len_chars;
            if first_trailing_whitespace != line_end {
                Some((first_trailing_whitespace, line_end))
            } else {
                None
            }
        }),
    );
    doc.apply(&transaction, view_id);
}

/// Trim any extra line-endings after the final line-ending.
fn trim_final_newlines(doc: &mut Document, view_id: ViewId) {
    let rope = doc.text();
    let mut text = rope.slice(..);
    let mut total_char_len = 0;
    let mut final_char_len = 0;
    while let Some(line_ending) = line_ending::get_line_ending(&text) {
        total_char_len += line_ending.len_chars();
        final_char_len = line_ending.len_chars();
        text = text.slice(..text.len_chars() - line_ending.len_chars());
    }
    let chars_to_delete = total_char_len - final_char_len;
    if chars_to_delete != 0 {
        let transaction = Transaction::delete(
            rope,
            [(rope.len_chars() - chars_to_delete, rope.len_chars())].into_iter(),
        );
        doc.apply(&transaction, view_id);
    }
}

/// Ensure that the document is terminated with a line ending.
fn insert_final_newline(doc: &mut Document, view_id: ViewId) {
    let text = doc.text();
    if text.len_chars() > 0 && line_ending::get_line_ending(&text.slice(..)).is_none() {
        let eof = Selection::point(text.len_chars());
        let insert = Transaction::insert(text, &eof, doc.line_ending.as_str().into());
        doc.apply(&insert, view_id);
    }
}

#[derive(Debug, Clone, Copy)]
pub struct WriteOptions {
    pub force: bool,
    pub auto_format: bool,
    pub code_actions: bool,
}

fn write(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    write_impl(
        cx,
        args.first(),
        WriteOptions {
            force: false,
            auto_format: !args.has_flag(WRITE_NO_FORMAT_FLAG.name),
            code_actions: !args.has_flag(WRITE_NO_CODE_ACTIONS_FLAG.name),
        },
    )
}

fn force_write(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    write_impl(
        cx,
        args.first(),
        WriteOptions {
            force: true,
            auto_format: !args.has_flag(WRITE_NO_FORMAT_FLAG.name),
            code_actions: !args.has_flag(WRITE_NO_CODE_ACTIONS_FLAG.name),
        },
    )
}

fn write_buffer_close(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    write_impl(
        cx,
        args.first(),
        WriteOptions {
            force: false,
            auto_format: !args.has_flag(WRITE_NO_FORMAT_FLAG.name),
            code_actions: !args.has_flag(WRITE_NO_CODE_ACTIONS_FLAG.name),
        },
    )?;

    let document_ids = buffer_gather_paths_impl(cx.editor, args);
    buffer_close_by_ids_impl(cx, &document_ids, false)
}

fn force_write_buffer_close(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    write_impl(
        cx,
        args.first(),
        WriteOptions {
            force: true,
            auto_format: !args.has_flag(WRITE_NO_FORMAT_FLAG.name),
            code_actions: !args.has_flag(WRITE_NO_CODE_ACTIONS_FLAG.name),
        },
    )?;

    let document_ids = buffer_gather_paths_impl(cx.editor, args);
    buffer_close_by_ids_impl(cx, &document_ids, false)
}

fn new_file(cx: &mut compositor::Context, _args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    cx.editor.new_file(Action::Replace);

    Ok(())
}

fn format(cx: &mut compositor::Context, _args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    let (view, doc) = current_ref!(cx.editor);
    let format = doc.format(cx.editor).context(
        "A formatter isn't available, and no language server provides formatting capabilities",
    )?;
    let callback = make_format_callback(doc.id(), doc.version(), view.id, format, None);
    cx.jobs.callback(callback);

    Ok(())
}

fn set_indent_style(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    use IndentStyle::*;

    // If no argument, report current indent style.
    if args.is_empty() {
        let style = doc!(cx.editor).indent_style;
        cx.editor.set_status(match style {
            Tabs => "tabs".to_owned(),
            Spaces(1) => "1 space".to_owned(),
            Spaces(n) => format!("{} spaces", n),
        });
        return Ok(());
    }

    // Attempt to parse argument as an indent style.
    let style = match args.first() {
        Some(arg) if "tabs".starts_with(&arg.to_lowercase()) => Some(Tabs),
        Some("0") => Some(Tabs),
        Some(arg) => arg
            .parse::<u8>()
            .ok()
            .filter(|n| (1..=MAX_INDENT).contains(n))
            .map(Spaces),
        _ => None,
    };

    let style = style.context("invalid indent style")?;
    let doc = doc_mut!(cx.editor);
    doc.indent_style = style;

    Ok(())
}

/// Sets or reports the current document's line ending setting.
fn set_line_ending(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    use LineEnding::*;

    // If no argument, report current line ending setting.
    if args.is_empty() {
        let line_ending = doc!(cx.editor).line_ending;
        cx.editor.set_status(match line_ending {
            Crlf => "crlf",
            LF => "line feed",
            #[cfg(feature = "unicode-lines")]
            FF => "form feed",
            #[cfg(feature = "unicode-lines")]
            CR => "carriage return",
            #[cfg(feature = "unicode-lines")]
            Nel => "next line",

            // These should never be a document's default line ending.
            #[cfg(feature = "unicode-lines")]
            VT | LS | PS => "error",
        });

        return Ok(());
    }

    let arg = args
        .first()
        .context("argument missing")?
        .to_ascii_lowercase();

    // Attempt to parse argument as a line ending.
    let line_ending = match arg {
        arg if arg.starts_with("crlf") => Crlf,
        arg if arg.starts_with("lf") => LF,
        #[cfg(feature = "unicode-lines")]
        arg if arg.starts_with("cr") => CR,
        #[cfg(feature = "unicode-lines")]
        arg if arg.starts_with("ff") => FF,
        #[cfg(feature = "unicode-lines")]
        arg if arg.starts_with("nel") => Nel,
        _ => bail!("invalid line ending"),
    };
    let (view, doc) = current!(cx.editor);
    doc.line_ending = line_ending;

    let mut pos = 0;
    let transaction = Transaction::change(
        doc.text(),
        doc.text().lines().filter_map(|line| {
            pos += line.len_chars();
            match zemacs_core::line_ending::get_line_ending(&line) {
                Some(ending) if ending != line_ending => {
                    let start = pos - ending.len_chars();
                    let end = pos;
                    Some((start, end, Some(line_ending.as_str().into())))
                }
                _ => None,
            }
        }),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);

    Ok(())
}
fn earlier(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    let uk = args.join(" ").parse::<UndoKind>().map_err(|s| anyhow!(s))?;

    let (view, doc) = current!(cx.editor);
    let success = doc.earlier(view, uk);
    if !success {
        cx.editor.set_status("Already at oldest change");
    }

    Ok(())
}

fn later(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    let uk = args.join(" ").parse::<UndoKind>().map_err(|s| anyhow!(s))?;
    let (view, doc) = current!(cx.editor);
    let success = doc.later(view, uk);
    if !success {
        cx.editor.set_status("Already at newest change");
    }

    Ok(())
}

fn write_quit(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    write_impl(
        cx,
        args.first(),
        WriteOptions {
            force: false,
            auto_format: !args.has_flag(WRITE_NO_FORMAT_FLAG.name),
            code_actions: !args.has_flag(WRITE_NO_CODE_ACTIONS_FLAG.name),
        },
    )?;
    cx.block_try_flush_writes()?;
    quit(cx, Args::default(), event)
}

fn force_write_quit(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    write_impl(
        cx,
        args.first(),
        WriteOptions {
            force: true,
            auto_format: !args.has_flag(WRITE_NO_FORMAT_FLAG.name),
            code_actions: !args.has_flag(WRITE_NO_CODE_ACTIONS_FLAG.name),
        },
    )?;
    cx.block_try_flush_writes()?;
    force_quit(cx, Args::default(), event)
}

/// Results in an error if there are modified buffers remaining and sets editor
/// error, otherwise returns `Ok(())`. If the current document is unmodified,
/// and there are modified documents, switches focus to one of them.
pub(super) fn buffers_remaining_impl(editor: &mut Editor) -> anyhow::Result<()> {
    let modified_ids: Vec<_> = editor
        .documents()
        .filter(|doc| doc.is_modified())
        .map(|doc| doc.id())
        .collect();

    if let Some(first) = modified_ids.first() {
        let current = doc!(editor);
        // If the current document is unmodified, and there are modified
        // documents, switch focus to the first modified doc.
        if !modified_ids.contains(&current.id()) {
            editor.switch(*first, Action::Replace);
        }

        let modified_names: Vec<_> = modified_ids
            .iter()
            .map(|doc_id| doc!(editor, doc_id).display_name())
            .collect();

        bail!(
            "{} unsaved buffer{} remaining: {:?}",
            modified_names.len(),
            if modified_names.len() == 1 { "" } else { "s" },
            modified_names,
        );
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
pub struct WriteAllOptions {
    pub force: bool,
    pub write_scratch: bool,
    pub auto_format: bool,
    pub code_actions: bool,
}

pub fn write_all_impl(
    cx: &mut compositor::Context,
    options: WriteAllOptions,
) -> anyhow::Result<()> {
    let mut errors: Vec<&'static str> = Vec::new();
    let config = cx.editor.config();
    let saves: Vec<_> = cx
        .editor
        .documents
        .keys()
        .cloned()
        .collect::<Vec<_>>()
        .into_iter()
        .filter_map(|id| {
            let doc = doc!(cx.editor, &id);
            if !doc.is_modified() {
                return None;
            }
            if doc.path().is_none() {
                if options.write_scratch {
                    errors.push("cannot write a buffer without a filename");
                }
                return None;
            }

            // Look for a view to apply the formatting change to.
            let target_view = cx.editor.get_synced_view_id(doc.id());
            Some((id, target_view))
        })
        .collect();

    for (doc_id, target_view) in saves {
        let doc = doc_mut!(cx.editor, &doc_id);
        let view = view_mut!(cx.editor, target_view);

        if doc.trim_trailing_whitespace() {
            trim_trailing_whitespace(doc, target_view);
        }
        if config.trim_final_newlines {
            trim_final_newlines(doc, target_view);
        }
        if doc.insert_final_newline() {
            insert_final_newline(doc, target_view);
        }

        // Save an undo checkpoint for any outstanding changes.
        doc.append_changes_to_history(view);

        let auto_format = config.auto_format && options.auto_format;
        let force = options.force;

        let run_code_actions = options.code_actions
            && doc!(cx.editor, &doc_id)
                .language_config()
                .and_then(|c| c.code_actions_on_save.as_deref())
                .is_some_and(|kinds| !kinds.is_empty());

        // See `write_impl`: deferred format-or-save tail that always saves, only
        // built when there is pre-save work; otherwise a synchronous save below.
        let tail = (auto_format || run_code_actions).then(|| {
            let callback: job::Callback = Callback::Followup(Box::new(move |editor| {
                let doc = doc!(editor, &doc_id);
                let fmt_job = auto_format
                    .then(|| doc.auto_format(editor))
                    .flatten()
                    .map(|fmt| {
                        let call = make_format_callback(
                            doc_id,
                            doc.version(),
                            target_view,
                            fmt,
                            Some((None, force)),
                        );
                        Job::with_callback(call).wait_before_exiting()
                    });
                if fmt_job.is_none() {
                    if let Err(err) = editor.save::<PathBuf>(doc_id, None, force) {
                        editor.set_error(format!("Error saving: {}", err));
                    }
                }
                fmt_job
            }));
            Job::with_callback(async { Ok(callback) }).wait_before_exiting()
        });

        let job = if run_code_actions {
            code_actions_on_save(cx, doc_id, tail)
        } else {
            tail
        };

        if let Some(job) = job {
            cx.jobs.add(job);
        } else {
            cx.editor.save::<PathBuf>(doc_id, None, force)?;
        }
    }

    if !errors.is_empty() && !options.force {
        bail!("{:?}", errors);
    }

    Ok(())
}

fn write_all(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    write_all_impl(
        cx,
        WriteAllOptions {
            force: false,
            write_scratch: true,
            auto_format: !args.has_flag(WRITE_NO_FORMAT_FLAG.name),
            code_actions: !args.has_flag(WRITE_NO_CODE_ACTIONS_FLAG.name),
        },
    )
}

fn force_write_all(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    write_all_impl(
        cx,
        WriteAllOptions {
            force: true,
            write_scratch: true,
            auto_format: !args.has_flag(WRITE_NO_FORMAT_FLAG.name),
            code_actions: !args.has_flag(WRITE_NO_CODE_ACTIONS_FLAG.name),
        },
    )
}

fn write_all_quit(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    write_all_impl(
        cx,
        WriteAllOptions {
            force: false,
            write_scratch: true,
            auto_format: !args.has_flag(WRITE_NO_FORMAT_FLAG.name),
            code_actions: !args.has_flag(WRITE_NO_CODE_ACTIONS_FLAG.name),
        },
    )?;
    quit_all_impl(cx, false)
}

fn force_write_all_quit(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let _ = write_all_impl(
        cx,
        WriteAllOptions {
            force: true,
            write_scratch: true,
            auto_format: !args.has_flag(WRITE_NO_FORMAT_FLAG.name),
            code_actions: !args.has_flag(WRITE_NO_CODE_ACTIONS_FLAG.name),
        },
    );
    quit_all_impl(cx, true)
}

fn quit_all_impl(cx: &mut compositor::Context, force: bool) -> anyhow::Result<()> {
    cx.block_try_flush_writes()?;
    if !force {
        buffers_remaining_impl(cx.editor)?;
    }

    // close all views
    let views: Vec<_> = cx.editor.tree.views().map(|(view, _)| view.id).collect();
    for view_id in views {
        cx.editor.close(view_id);
    }

    Ok(())
}

fn quit_all(cx: &mut compositor::Context, _args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    quit_all_impl(cx, false)
}

fn force_quit_all(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    quit_all_impl(cx, true)
}

fn cquit(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    let exit_code = args
        .first()
        .and_then(|code| code.parse::<i32>().ok())
        .unwrap_or(1);

    cx.editor.exit_code = exit_code;
    quit_all_impl(cx, false)
}

fn force_cquit(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    let exit_code = args
        .first()
        .and_then(|code| code.parse::<i32>().ok())
        .unwrap_or(1);
    cx.editor.exit_code = exit_code;

    quit_all_impl(cx, true)
}

fn theme(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    let true_color = cx.editor.config.load().true_color || crate::true_color();
    match event {
        PromptEvent::Abort => {
            cx.editor.unset_theme_preview()?;
        }
        PromptEvent::Update => {
            if args.is_empty() {
                // Ensures that a preview theme gets cleaned up if the user backspaces until the prompt is empty.
                cx.editor.unset_theme_preview()?;
            } else if let Some(theme_name) = args.first() {
                if let Ok(theme) = cx.editor.theme_loader.load(theme_name) {
                    if !(true_color || theme.is_16_color()) {
                        bail!("Unsupported theme: theme requires true color support");
                    }
                    cx.editor.set_theme_preview(theme)?;
                };
            };
        }
        PromptEvent::Validate => {
            if let Some(theme_name) = args.first() {
                let theme = cx
                    .editor
                    .theme_loader
                    .load(theme_name)
                    .map_err(|err| anyhow::anyhow!("Could not load theme: {}", err))?;
                if !(true_color || theme.is_16_color()) {
                    bail!("Unsupported theme: theme requires true color support");
                }
                cx.editor.set_theme(theme)?;
            } else {
                let name = cx.editor.theme.name().to_string();

                cx.editor.set_status(name);
            }
        }
    };

    Ok(())
}

/// All available theme names (config dir + runtime dirs + the two built-ins), sorted/deduped.
pub(crate) fn all_theme_names() -> Vec<String> {
    let mut names =
        zemacs_view::theme::Loader::read_names(&zemacs_loader::config_dir().join("themes"));
    for rt_dir in zemacs_loader::runtime_dirs() {
        names.extend(zemacs_view::theme::Loader::read_names(&rt_dir.join("themes")));
    }
    names.push("default".to_string());
    names.push("base16_default".to_string());
    names.sort();
    names.dedup();
    names
}

fn cycle_theme(cx: &mut compositor::Context, delta: isize) -> anyhow::Result<()> {
    let names = all_theme_names();
    if names.is_empty() {
        return Ok(());
    }
    let current = cx.editor.theme.name();
    let idx = names.iter().position(|n| n == current).unwrap_or(0) as isize;
    let next = (idx + delta).rem_euclid(names.len() as isize) as usize;
    let name = names[next].clone();
    let theme = cx
        .editor
        .theme_loader
        .load(&name)
        .map_err(|err| anyhow::anyhow!("Could not load theme '{name}': {err}"))?;
    cx.editor.set_theme(theme)?;
    cx.editor.set_status(format!("theme: {name}"));
    Ok(())
}

/// Reset (undo) the git hunk under the primary cursor, restoring it from the diff base (HEAD/index).
/// This is gitsigns/`vim-gitgutter`'s `reset_hunk` and JetBrains' gutter "rollback".
fn hunk_reset(cx: &mut compositor::Context, _args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    // Compute the replacement under an immutable borrow, then re-borrow mutably to apply it.
    let change: Option<(usize, usize, String)> = {
        let (view, doc) = current_ref!(cx.editor);
        match doc.diff_handle() {
            None => None,
            Some(handle) => {
                let diff = handle.load();
                let text = doc.text();
                let cursor = doc.selection(view.id).primary().cursor(text.slice(..));
                let line = text.char_to_line(cursor) as u32;
                diff.hunk_at(line, true).map(|idx| {
                    let hunk = diff.nth_hunk(idx);
                    let base = diff.diff_base();
                    let bstart = base.line_to_char(hunk.before.start as usize);
                    let bend = base.line_to_char(hunk.before.end as usize);
                    let replacement: String = base.slice(bstart..bend).chars().collect();
                    let dstart = text.line_to_char(hunk.after.start as usize);
                    let dend = text.line_to_char(hunk.after.end as usize);
                    (dstart, dend, replacement)
                })
            }
        }
    };
    let Some((start, end, replacement)) = change else {
        cx.editor.set_status("no git hunk at cursor");
        return Ok(());
    };
    let (view, doc) = current!(cx.editor);
    let transaction = Transaction::change(
        doc.text(),
        std::iter::once((start, end, (!replacement.is_empty()).then(|| replacement.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    cx.editor.set_status("hunk reset");
    Ok(())
}

/// Move the primary cursor to the next/previous git hunk (gitsigns `]c`/`[c`, vim-gitgutter hunks).
fn hunk_goto(cx: &mut compositor::Context, forward: bool) -> anyhow::Result<()> {
    let target: Option<usize> = {
        let (view, doc) = current_ref!(cx.editor);
        match doc.diff_handle() {
            None => None,
            Some(handle) => {
                let diff = handle.load();
                let text = doc.text();
                let cursor = doc.selection(view.id).primary().cursor(text.slice(..));
                let line = text.char_to_line(cursor) as u32;
                let idx = if forward {
                    diff.next_hunk(line)
                } else {
                    diff.prev_hunk(line)
                };
                idx.map(|i| text.line_to_char(diff.nth_hunk(i).after.start as usize))
            }
        }
    };
    let Some(pos) = target else {
        cx.editor.set_status("no more hunks");
        return Ok(());
    };
    let scrolloff = cx.editor.config().scrolloff;
    let (view, doc) = current!(cx.editor);
    doc.set_selection(view.id, zemacs_core::Selection::point(pos));
    view.ensure_cursor_in_view(doc, scrolloff);
    Ok(())
}

fn hunk_next(cx: &mut compositor::Context, _args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    hunk_goto(cx, true)
}

fn hunk_prev(cx: &mut compositor::Context, _args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    hunk_goto(cx, false)
}

/// Locate the git merge-conflict block containing (or nearest above) `cursor`.
/// Returns `(block_start_char, block_end_char, ours_text, theirs_text)`. Handles both the 2-way
/// (`<<<<<<< ======= >>>>>>>`) and diff3 (`<<<<<<< ||||||| ======= >>>>>>>`) marker styles.
fn conflict_block(
    text: &zemacs_core::Rope,
    cursor: usize,
) -> Option<(usize, usize, String, String)> {
    let cursor_line = text.char_to_line(cursor);
    let total = text.len_lines();
    let line_str = |l: usize| text.line(l).chars().collect::<String>();

    let mut start = None;
    for l in (0..=cursor_line).rev() {
        let s = line_str(l);
        if s.starts_with("<<<<<<<") {
            start = Some(l);
            break;
        }
        if s.starts_with(">>>>>>>") && l != cursor_line {
            return None; // cursor sits past the end of the previous block
        }
    }
    let start = start?;
    let (mut base_sep, mut sep, mut end) = (None, None, None);
    for l in (start + 1)..total {
        let s = line_str(l);
        if s.starts_with("|||||||") && base_sep.is_none() {
            base_sep = Some(l);
        } else if s.starts_with("=======") && sep.is_none() {
            sep = Some(l);
        } else if s.starts_with(">>>>>>>") {
            end = Some(l);
            break;
        }
    }
    let (sep, end) = (sep?, end?);
    let ours_end = base_sep.unwrap_or(sep);
    let ours: String = ((start + 1)..ours_end).map(line_str).collect();
    let theirs: String = ((sep + 1)..end).map(line_str).collect();
    Some((text.line_to_char(start), text.line_to_char(end + 1), ours, theirs))
}

/// Resolve the merge conflict under the cursor by keeping `which` ∈ {ours, theirs, both}.
fn conflict_resolve(cx: &mut compositor::Context, which: &str) -> anyhow::Result<()> {
    let change: Option<(usize, usize, String)> = {
        let (view, doc) = current_ref!(cx.editor);
        let text = doc.text();
        let cursor = doc.selection(view.id).primary().cursor(text.slice(..));
        conflict_block(text, cursor).map(|(s, e, ours, theirs)| {
            let kept = match which {
                "theirs" => theirs,
                "both" => format!("{ours}{theirs}"),
                _ => ours,
            };
            (s, e, kept)
        })
    };
    let Some((start, end, kept)) = change else {
        cx.editor.set_status("no merge conflict at cursor");
        return Ok(());
    };
    let (view, doc) = current!(cx.editor);
    let transaction = Transaction::change(
        doc.text(),
        std::iter::once((start, end, (!kept.is_empty()).then(|| kept.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    cx.editor.set_status(format!("conflict resolved: kept {which}"));
    Ok(())
}

fn conflict_ours(cx: &mut compositor::Context, _a: Args, e: PromptEvent) -> anyhow::Result<()> {
    if e == PromptEvent::Validate {
        conflict_resolve(cx, "ours")?;
    }
    Ok(())
}
fn conflict_theirs(cx: &mut compositor::Context, _a: Args, e: PromptEvent) -> anyhow::Result<()> {
    if e == PromptEvent::Validate {
        conflict_resolve(cx, "theirs")?;
    }
    Ok(())
}
fn conflict_both(cx: &mut compositor::Context, _a: Args, e: PromptEvent) -> anyhow::Result<()> {
    if e == PromptEvent::Validate {
        conflict_resolve(cx, "both")?;
    }
    Ok(())
}

/// Jump to the next/previous `<<<<<<<` conflict marker.
fn conflict_goto(cx: &mut compositor::Context, forward: bool) -> anyhow::Result<()> {
    let target: Option<usize> = {
        let (view, doc) = current_ref!(cx.editor);
        let text = doc.text();
        let cursor = doc.selection(view.id).primary().cursor(text.slice(..));
        let cur = text.char_to_line(cursor);
        let total = text.len_lines();
        let starts = |l: usize| text.line(l).chars().take(7).collect::<String>() == "<<<<<<<";
        if forward {
            ((cur + 1)..total).find(|&l| starts(l))
        } else {
            (0..cur).rev().find(|&l| starts(l))
        }
        .map(|l| text.line_to_char(l))
    };
    let Some(pos) = target else {
        cx.editor.set_status("no more conflicts");
        return Ok(());
    };
    let scrolloff = cx.editor.config().scrolloff;
    let (view, doc) = current!(cx.editor);
    doc.set_selection(view.id, zemacs_core::Selection::point(pos));
    view.ensure_cursor_in_view(doc, scrolloff);
    Ok(())
}
fn conflict_next(cx: &mut compositor::Context, _a: Args, e: PromptEvent) -> anyhow::Result<()> {
    if e == PromptEvent::Validate {
        conflict_goto(cx, true)?;
    }
    Ok(())
}
fn conflict_prev(cx: &mut compositor::Context, _a: Args, e: PromptEvent) -> anyhow::Result<()> {
    if e == PromptEvent::Validate {
        conflict_goto(cx, false)?;
    }
    Ok(())
}

/// Toggle the editor between a dark and a light theme. The current mode is detected from the active
/// theme's background luminance, so it works regardless of which theme is set. Optional args override
/// the pair: `:theme-toggle [darkTheme] [lightTheme]`.
fn theme_toggle(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let is_light = match cx.editor.theme.get("ui.background").bg {
        Some(zemacs_view::graphics::Color::Rgb(r, g, b)) => {
            0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32 > 128.0
        }
        _ => false,
    };
    let cur = cx.editor.theme.name().to_string();
    // Prefer flipping a paired `zgui-<id>` <-> `zgui-<id>-light` theme; otherwise use the args
    // (defaults: zgui-cyberpunk <-> catppuccin_latte).
    let paired = if let Some(base) = cur.strip_suffix("-light") {
        Some(base.to_string())
    } else if cur.starts_with("zgui-") {
        Some(format!("{cur}-light"))
    } else {
        None
    };
    let dark = args.first().map(|s| s.to_string()).unwrap_or_else(|| "zgui-cyberpunk".to_string());
    let light = args.get(1).map(|s| s.to_string()).unwrap_or_else(|| "catppuccin_latte".to_string());
    let target = match (paired, is_light) {
        (Some(p), _) => p,
        (None, true) => dark,
        (None, false) => light,
    };
    let target = &target;
    let true_color = cx.editor.config.load().true_color || crate::true_color();
    let theme = cx
        .editor
        .theme_loader
        .load(target)
        .map_err(|err| anyhow::anyhow!("Could not load theme '{target}': {err}"))?;
    if !(true_color || theme.is_16_color()) {
        bail!("Unsupported theme: theme requires true color support");
    }
    cx.editor.set_theme(theme)?;
    cx.editor.set_status(format!("theme: {target}"));
    Ok(())
}

fn theme_next(cx: &mut compositor::Context, _args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    cycle_theme(cx, 1)
}

fn theme_prev(cx: &mut compositor::Context, _args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    cycle_theme(cx, -1)
}

fn run_command(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let path = doc!(cx.editor).path().map(|p| p.to_path_buf());
    let (default_cmd, cwd) = crate::ui::run::smart_command(path.as_deref());
    let cmd = if args.is_empty() {
        default_cmd
    } else {
        args.join(" ")
    };
    let shell = cx.editor.config().shell.clone();
    let run = crate::ui::run::spawn(cmd, shell, cwd);
    let call: job::Callback = job::Callback::EditorCompositor(Box::new(
        move |_editor: &mut Editor, compositor: &mut Compositor| {
            if let Some(view) = compositor.find::<crate::ui::EditorView>() {
                view.set_run(run);
            }
        },
    ));
    cx.jobs.callback(async move { Ok(call) });
    Ok(())
}

fn registers(cx: &mut compositor::Context, _args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    // vim `:registers`: show the contents of every register (named, numbered, and specials).
    let info = zemacs_view::info::Info::from_registers("Registers", &cx.editor.registers);
    cx.editor.autoinfo = Some(info);
    Ok(())
}

fn yank_main_selection_to_clipboard(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    yank_main_selection_to_register(cx.editor, '+');
    Ok(())
}

fn yank_joined(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    let doc = doc!(cx.editor);
    let default_sep = Cow::Borrowed(doc.line_ending.as_str());
    let separator = args.first().unwrap_or(&default_sep);
    let register = cx
        .editor
        .selected_register
        .unwrap_or(cx.editor.config().default_yank_register);
    yank_joined_impl(cx.editor, separator, register);
    Ok(())
}

fn yank_joined_to_clipboard(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    let doc = doc!(cx.editor);
    let default_sep = Cow::Borrowed(doc.line_ending.as_str());
    let separator = args.first().unwrap_or(&default_sep);
    yank_joined_impl(cx.editor, separator, '+');
    Ok(())
}

fn yank_main_selection_to_primary_clipboard(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    yank_main_selection_to_register(cx.editor, '*');
    Ok(())
}

fn yank_joined_to_primary_clipboard(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    let doc = doc!(cx.editor);
    let default_sep = Cow::Borrowed(doc.line_ending.as_str());
    let separator = args.first().unwrap_or(&default_sep);
    yank_joined_impl(cx.editor, separator, '*');
    Ok(())
}

fn paste_clipboard_after(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    paste(cx.editor, '+', Paste::After, 1);
    Ok(())
}

fn paste_clipboard_before(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    paste(cx.editor, '+', Paste::Before, 1);
    Ok(())
}

fn paste_primary_clipboard_after(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    paste(cx.editor, '*', Paste::After, 1);
    Ok(())
}

fn paste_primary_clipboard_before(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    paste(cx.editor, '*', Paste::Before, 1);
    Ok(())
}

fn replace_selections_with_clipboard(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    replace_selections_with_register(cx.editor, '+', 1);
    Ok(())
}

fn replace_selections_with_primary_clipboard(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    replace_selections_with_register(cx.editor, '*', 1);
    Ok(())
}

fn show_clipboard_provider(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    cx.editor
        .set_status(cx.editor.registers.clipboard_provider_name());
    Ok(())
}

/// Helper function to parse the first argument as a directory
#[inline]
fn parse_first_arg_as_dir(args: &Args, last_cwd: Option<PathBuf>) -> anyhow::Result<PathBuf> {
    match args.first().map(AsRef::as_ref) {
        Some("-") => last_cwd.ok_or_else(|| anyhow!("No previous working directory")),
        Some(path) => Ok(zemacs_stdx::path::expand_tilde(Path::new(path)).into_owned()),
        None => Ok(home_dir()?),
    }
}

/// Helper function to apply a directory change for an already-parsed Path ref
#[inline]
fn apply_directory_change(cx: &mut compositor::Context, dir: &Path) -> anyhow::Result<()> {
    cx.editor.set_cwd(dir).map_err(|err| {
        anyhow!(
            "Could not change working directory to '{}': {err}",
            dir.display()
        )
    })?;

    cx.editor.set_status(format!(
        "Current working directory is now {}",
        zemacs_stdx::env::current_working_dir().display()
    ));

    Ok(())
}

fn change_current_directory(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    let dir = parse_first_arg_as_dir(&args, cx.editor.get_last_cwd().map(|p| p.to_path_buf()))?;

    apply_directory_change(cx, &dir)
}

fn show_directory_stack(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    let serialized_stack = cx
        .editor
        .dir_stack
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(" ");

    if !serialized_stack.is_empty() {
        cx.editor.set_status(serialized_stack);
    } else {
        cx.editor.set_error("Stack is empty");
    }

    Ok(())
}

fn push_directory(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    // avoid an unbounded directory stack and reallocs for perf
    if cx.editor.dir_stack.len() == cx.editor.dir_stack.capacity() {
        cx.editor.dir_stack.pop_back();
    }

    cx.editor
        .dir_stack
        .push_front(zemacs_stdx::env::current_working_dir());

    change_current_directory(cx, args, event)
}

fn pop_directory(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    if let Some(dir) = cx.editor.dir_stack.pop_front() {
        apply_directory_change(cx, &dir)?;
    } else {
        cx.editor.set_error("Stack is empty");
    }

    Ok(())
}

fn show_current_directory(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    let cwd = zemacs_stdx::env::current_working_dir();
    let message = format!("Current working directory is {}", cwd.display());

    if cwd.exists() {
        cx.editor.set_status(message);
    } else {
        cx.editor.set_error(format!("{} (deleted)", message));
    }
    Ok(())
}

/// Sets the [`Document`]'s encoding..
fn set_encoding(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    let doc = doc_mut!(cx.editor);
    if let Some(label) = args.first() {
        doc.set_encoding(label)
    } else {
        let encoding = doc.encoding().name().to_owned();
        cx.editor.set_status(encoding);
        Ok(())
    }
}

/// Shows info about the character under the primary cursor.
fn get_character_info(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    let (view, doc) = current_ref!(cx.editor);
    let text = doc.text().slice(..);

    let grapheme_start = doc.selection(view.id).primary().cursor(text);
    let grapheme_end = graphemes::next_grapheme_boundary(text, grapheme_start);

    if grapheme_start == grapheme_end {
        return Ok(());
    }

    let grapheme = text.slice(grapheme_start..grapheme_end).to_string();
    let encoding = doc.encoding();

    let printable = grapheme.chars().fold(String::new(), |mut s, c| {
        match c {
            '\0' => s.push_str("\\0"),
            '\t' => s.push_str("\\t"),
            '\n' => s.push_str("\\n"),
            '\r' => s.push_str("\\r"),
            _ => s.push(c),
        }

        s
    });

    // Convert to Unicode codepoints if in UTF-8
    let unicode = if encoding == encoding::UTF_8 {
        let mut unicode = " (".to_owned();

        for (i, char) in grapheme.chars().enumerate() {
            if i != 0 {
                unicode.push(' ');
            }

            unicode.push_str("U+");

            let codepoint: u32 = if char.is_ascii() {
                char.into()
            } else {
                // Not ascii means it will be multi-byte, so strip out the extra
                // bits that encode the length & mark continuation bytes

                let s = String::from(char);
                let bytes = s.as_bytes();

                // First byte starts with 2-4 ones then a zero, so strip those off
                let first = bytes[0];
                let codepoint = first & (0xFF >> (first.leading_ones() + 1));
                let mut codepoint = u32::from(codepoint);

                // Following bytes start with 10
                for byte in bytes.iter().skip(1) {
                    codepoint <<= 6;
                    codepoint += u32::from(*byte) & 0x3F;
                }

                codepoint
            };

            write!(unicode, "{codepoint:0>4x}").unwrap();
        }

        unicode.push(')');
        unicode
    } else {
        String::new()
    };

    // Give the decimal value for ascii characters
    let dec = if encoding.is_ascii_compatible() && grapheme.len() == 1 {
        format!(" Dec {}", grapheme.as_bytes()[0])
    } else {
        String::new()
    };

    let hex = {
        let mut encoder = encoding.new_encoder();
        let max_encoded_len = encoder
            .max_buffer_length_from_utf8_without_replacement(grapheme.len())
            .unwrap();
        let mut bytes = Vec::with_capacity(max_encoded_len);
        let mut current_byte = 0;
        let mut hex = String::new();

        for (i, char) in grapheme.chars().enumerate() {
            if i != 0 {
                hex.push_str(" +");
            }

            let (result, _input_bytes_read) = encoder.encode_from_utf8_to_vec_without_replacement(
                &char.to_string(),
                &mut bytes,
                true,
            );

            if let encoding::EncoderResult::Unmappable(char) = result {
                bail!("{char:?} cannot be mapped to {}", encoding.name());
            }

            for byte in &bytes[current_byte..] {
                write!(hex, " {byte:0>2x}").unwrap();
            }

            current_byte = bytes.len();
        }

        hex
    };

    cx.editor
        .set_status(format!("\"{printable}\"{unicode}{dec} Hex{hex}"));

    Ok(())
}

/// Reload the [`Document`] from its source file.
fn reload(cx: &mut compositor::Context, _args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    let scrolloff = cx.editor.config().scrolloff;
    let trust_full = doc_trust_full(cx.editor);
    let (view, doc) = current!(cx.editor);
    doc.reload(view, &cx.editor.diff_providers, trust_full)
        .map(|_| {
            view.ensure_cursor_in_view(doc, scrolloff);
        })?;
    if let Some(path) = doc.path().map(ToOwned::to_owned) {
        cx.editor
            .language_servers
            .file_event_handler
            .file_changed(path);
    }
    Ok(())
}

fn reload_all(cx: &mut compositor::Context, _args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    let scrolloff = cx.editor.config().scrolloff;
    let view_id = view!(cx.editor).id;

    let docs_view_ids: Vec<(DocumentId, Vec<ViewId>)> = cx
        .editor
        .documents_mut()
        .map(|doc| {
            let mut view_ids: Vec<_> = doc.selections().keys().cloned().collect();

            if view_ids.is_empty() {
                doc.ensure_view_init(view_id);
                view_ids.push(view_id);
            };

            (doc.id(), view_ids)
        })
        .collect();

    for (doc_id, view_ids) in docs_view_ids {
        let doc = doc_mut!(cx.editor, &doc_id);

        // Every doc is guaranteed to have at least 1 view at this point.
        let view = view_mut!(cx.editor, view_ids[0]);

        // Ensure that the view is synced with the document's history.
        view.sync_changes(doc);

        // Per-document trust: each doc's workspace may differ.
        let trust_full = cx
            .editor
            .workspace_trust
            .query(
                doc.workspace_root(),
                zemacs_loader::workspace_trust::TrustQuery::Git,
            )
            .is_trusted();
        if let Err(error) = doc.reload(view, &cx.editor.diff_providers, trust_full) {
            cx.editor.set_error(format!("{}", error));
            continue;
        }

        if let Some(path) = doc.path().map(ToOwned::to_owned) {
            cx.editor
                .language_servers
                .file_event_handler
                .file_changed(path);
        }

        for view_id in view_ids {
            let view = view_mut!(cx.editor, view_id);
            if view.doc.eq(&doc_id) {
                // Reloading commits the diff against disk through the first view
                // only (above). Any other view onto this document is left
                // pointing at the pre-reload revision, so sync it now; otherwise
                // its jumplist entries keep referencing the old (e.g. larger)
                // text and a later commit panics when mapping them through a
                // changeset whose pre-image no longer contains them.
                view.sync_changes(doc);
                view.ensure_cursor_in_view(doc, scrolloff);
            }
        }
    }

    Ok(())
}

/// Update the [`Document`] if it has been modified.
fn update(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    let (_view, doc) = current!(cx.editor);
    if doc.is_modified() {
        write_impl(
            cx,
            None,
            WriteOptions {
                force: false,
                auto_format: !args.has_flag(WRITE_NO_FORMAT_FLAG.name),
                code_actions: !args.has_flag(WRITE_NO_CODE_ACTIONS_FLAG.name),
            },
        )
    } else {
        Ok(())
    }
}

fn lsp_workspace_command(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    let doc = doc!(cx.editor);
    let ls_id_commands = doc
        .language_servers_with_feature(LanguageServerFeature::WorkspaceCommand)
        .flat_map(|ls| {
            ls.capabilities()
                .execute_command_provider
                .iter()
                .flat_map(|options| options.commands.iter())
                .map(|command| (ls.id(), command))
        });

    if args.is_empty() {
        let commands = ls_id_commands
            .map(|(ls_id, command)| {
                (
                    ls_id,
                    zemacs_lsp::lsp::Command {
                        title: command.clone(),
                        command: command.clone(),
                        arguments: None,
                    },
                )
            })
            .collect::<Vec<_>>();
        let callback = async move {
            let call: job::Callback = Callback::EditorCompositor(Box::new(
                move |_editor: &mut Editor, compositor: &mut Compositor| {
                    let columns = [ui::PickerColumn::new(
                        "title",
                        |(_ls_id, command): &(_, zemacs_lsp::lsp::Command), _| {
                            command.title.as_str().into()
                        },
                    )];
                    let picker = ui::Picker::new(
                        columns,
                        0,
                        commands,
                        (),
                        move |cx, (ls_id, command), _action| {
                            cx.editor.execute_lsp_command(command.clone(), *ls_id);
                        },
                    );
                    compositor.push(Box::new(overlaid(picker)))
                },
            ));
            Ok(call)
        };
        cx.jobs.callback(callback);
    } else {
        let command = args[0].to_string();
        let matches: Vec<_> = ls_id_commands
            .filter(|(_ls_id, c)| *c == &command)
            .collect();

        match matches.as_slice() {
            [(ls_id, _command)] => {
                let arguments = args
                    .get(1)
                    .map(|rest| {
                        serde_json::Deserializer::from_str(rest)
                            .into_iter()
                            .collect::<Result<Vec<Value>, _>>()
                            .map_err(|err| anyhow!("failed to parse arguments: {err}"))
                    })
                    .transpose()?
                    .filter(|args| !args.is_empty());

                cx.editor.execute_lsp_command(
                    zemacs_lsp::lsp::Command {
                        title: command.clone(),
                        arguments,
                        command,
                    },
                    *ls_id,
                );
            }
            [] => {
                cx.editor.set_status(format!(
                    "`{command}` is not supported for any language server"
                ));
            }
            _ => {
                cx.editor.set_status(format!(
                    "`{command}` supported by multiple language servers"
                ));
            }
        }
    }
    Ok(())
}

fn lsp_restart(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    let editor_config = cx.editor.config.load();
    let doc = doc!(cx.editor);
    let config = doc
        .language_config()
        .context("LSP not defined for the current document")?;

    let language_servers: Vec<_> = config
        .language_servers
        .iter()
        .map(|ls| ls.name.as_str())
        .collect();
    let language_servers = if args.is_empty() {
        language_servers
    } else {
        let (valid, invalid): (Vec<_>, Vec<_>) = args
            .iter()
            .map(|arg| arg.as_ref())
            .partition(|name| language_servers.contains(name));
        if !invalid.is_empty() {
            let s = if invalid.len() == 1 { "" } else { "s" };
            bail!("Unknown language server{s}: {}", invalid.join(", "));
        }
        valid
    };

    let mut errors = Vec::new();
    for server in language_servers.iter() {
        match cx
            .editor
            .language_servers
            .restart_server(
                server,
                config,
                doc.path(),
                &editor_config.workspace_lsp_roots,
                editor_config.lsp.snippets,
            )
            .transpose()
        {
            // Ignore the executable-not-found error unless the server was explicitly requested
            // in the arguments.
            Err(zemacs_lsp::Error::ExecutableNotFound(_))
                if !args.iter().any(|arg| arg == server) => {}
            Err(err) => errors.push(err.to_string()),
            _ => (),
        }
    }

    // This collect is needed because refresh_language_server would need to re-borrow editor.
    let document_ids_to_refresh: Vec<DocumentId> = cx
        .editor
        .documents()
        .filter_map(|doc| match doc.language_config() {
            Some(config)
                if config.language_servers.iter().any(|ls| {
                    language_servers
                        .iter()
                        .any(|restarted_ls| restarted_ls == &ls.name)
                }) =>
            {
                Some(doc.id())
            }
            _ => None,
        })
        .collect();

    for document_id in document_ids_to_refresh {
        cx.editor.refresh_language_servers(document_id);
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "Error restarting language servers: {}",
            errors.join(", ")
        ))
    }
}

fn lsp_stop(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let doc = doc!(cx.editor);

    let language_servers: Vec<_> = doc
        .language_servers()
        .map(|ls| ls.name().to_string())
        .collect();
    let language_servers = if args.is_empty() {
        language_servers
    } else {
        let (valid, invalid): (Vec<_>, Vec<_>) = args
            .iter()
            .map(|arg| arg.to_string())
            .partition(|name| language_servers.contains(name));
        if !invalid.is_empty() {
            let s = if invalid.len() == 1 { "" } else { "s" };
            bail!("Unknown language server{s}: {}", invalid.join(", "));
        }
        valid
    };

    for ls_name in &language_servers {
        cx.editor.language_servers.stop(ls_name);

        for doc in cx.editor.documents_mut() {
            if let Some(client) = doc.remove_language_server_by_name(ls_name) {
                doc.clear_diagnostics_for_language_server(client.id());
                doc.reset_all_inlay_hints();
                doc.inlay_hints_oudated = true;
            }
        }
    }

    Ok(())
}

fn tree_sitter_scopes(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    let (view, doc) = current!(cx.editor);
    let text = doc.text().slice(..);

    let pos = doc.selection(view.id).primary().cursor(text);
    let scopes = indent::get_scopes(doc.syntax(), text, pos);

    let contents = format!("```json\n{:?}\n````", scopes);

    let callback = async move {
        let call: job::Callback = Callback::EditorCompositor(Box::new(
            move |editor: &mut Editor, compositor: &mut Compositor| {
                let contents = ui::Markdown::new(contents, editor.syn_loader.clone());
                let popup = Popup::new("hover", contents).auto_close(true);
                compositor.replace_or_push("hover", popup);
            },
        ));
        Ok(call)
    };

    cx.jobs.callback(callback);

    Ok(())
}

fn tree_sitter_highlight_name(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    let (view, doc) = current_ref!(cx.editor);
    let Some(syntax) = doc.syntax() else {
        return Ok(());
    };
    let text = doc.text().slice(..);
    let cursor = doc.selection(view.id).primary().cursor(text);
    let byte = text.char_to_byte(cursor) as u32;
    // Query the same range as the one used in syntax highlighting.
    let range = {
        // Calculate viewport byte ranges:
        let row = text.char_to_line(doc.view_offset(view.id).anchor.min(text.len_chars()));
        // Saturating subs to make it inclusive zero indexing.
        let last_line = text.len_lines().saturating_sub(1);
        let height = view.inner_area(doc).height;
        let last_visible_line = (row + height as usize).saturating_sub(1).min(last_line);
        let start = text.line_to_byte(row.min(last_line)) as u32;
        let end = text.line_to_byte(last_visible_line + 1) as u32;

        start..end
    };

    let loader = cx.editor.syn_loader.load();
    let mut highlighter = syntax.highlighter(text, &loader, range);
    let mut highlights = Vec::new();

    while highlighter.next_event_offset() <= byte {
        let (event, new_highlights) = highlighter.advance();
        if event == zemacs_core::syntax::HighlightEvent::Refresh {
            highlights.clear();
        }
        highlights.extend(new_highlights);
    }

    let content = highlights
        .into_iter()
        .fold(String::new(), |mut acc, highlight| {
            if !acc.is_empty() {
                acc.push_str(", ");
            }
            acc.push_str(cx.editor.theme.scope(highlight));
            acc
        });

    let callback = async move {
        let call: job::Callback = Callback::EditorCompositor(Box::new(
            move |editor: &mut Editor, compositor: &mut Compositor| {
                let content = ui::Markdown::new(content, editor.syn_loader.clone());
                let popup = Popup::new("hover", content).auto_close(true);
                compositor.replace_or_push("hover", popup);
            },
        ));
        Ok(call)
    };

    cx.jobs.callback(callback);

    Ok(())
}

fn tree_sitter_layers(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    let (view, doc) = current_ref!(cx.editor);
    let Some(syntax) = doc.syntax() else {
        bail!("Syntax information is not available");
    };

    let loader: &zemacs_core::syntax::Loader = &cx.editor.syn_loader.load();
    let text = doc.text().slice(..);
    let cursor = doc.selection(view.id).primary().cursor(text);
    let byte = text.char_to_byte(cursor) as u32;
    let languages =
        syntax
            .layers_for_byte_range(byte, byte)
            .fold(String::new(), |mut acc, layer| {
                if !acc.is_empty() {
                    acc.push_str(", ");
                }
                acc.push_str(
                    &loader
                        .language(syntax.layer(layer).language)
                        .config()
                        .language_id,
                );
                acc
            });

    let callback = async move {
        let call: job::Callback = Callback::EditorCompositor(Box::new(
            move |editor: &mut Editor, compositor: &mut Compositor| {
                let content = ui::Markdown::new(languages, editor.syn_loader.clone());
                let popup = Popup::new("hover", content).auto_close(true);
                compositor.replace_or_push("hover", popup);
            },
        ));
        Ok(call)
    };

    cx.jobs.callback(callback);

    Ok(())
}

fn vsplit(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    if args.is_empty() {
        split(cx.editor, Action::VerticalSplit);
    } else {
        open_impl(cx, args, Action::VerticalSplit)?;
    }

    Ok(())
}

fn hsplit(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    if args.is_empty() {
        split(cx.editor, Action::HorizontalSplit);
    } else {
        open_impl(cx, args, Action::HorizontalSplit)?;
    }

    Ok(())
}

fn vsplit_new(cx: &mut compositor::Context, _args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    cx.editor.new_file(Action::VerticalSplit);

    Ok(())
}

fn hsplit_new(cx: &mut compositor::Context, _args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    cx.editor.new_file(Action::HorizontalSplit);

    Ok(())
}

fn debug_eval(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    if let Some(debugger) = cx.editor.debug_adapters.get_active_client() {
        let (frame, thread_id) = match (debugger.active_frame, debugger.thread_id) {
            (Some(frame), Some(thread_id)) => (frame, thread_id),
            _ => {
                bail!("Cannot find current stack frame to access variables")
            }
        };

        // TODO: support no frame_id

        let frame_id = debugger.stack_frames[&thread_id][frame].id;
        let response = zemacs_lsp::block_on(debugger.eval(args.join(" "), Some(frame_id)))?;
        cx.editor.set_status(response.result);
    }
    Ok(())
}

fn debug_start(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    let mut args: Vec<_> = args.into_iter().collect();
    let name = match args.len() {
        0 => None,
        _ => Some(args.remove(0)),
    };
    dap_start_impl(cx, name.as_deref(), None, Some(args))
}

fn debug_remote(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    let mut args: Vec<_> = args.into_iter().collect();
    let address = match args.len() {
        0 => None,
        _ => Some(args.remove(0).parse()?),
    };
    let name = match args.len() {
        0 => None,
        _ => Some(args.remove(0)),
    };
    dap_start_impl(cx, name.as_deref(), address, Some(args))
}

fn tutor(cx: &mut compositor::Context, _args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    let path = zemacs_loader::runtime_file(Path::new("tutor"));
    cx.editor.open(&path, Action::Replace)?;
    // Unset path to prevent accidentally saving to the original tutor file.
    doc_mut!(cx.editor).set_path(None);
    Ok(())
}

fn abort_goto_line_number_preview(cx: &mut compositor::Context) {
    if let Some(last_selection) = cx.editor.last_selection.take() {
        let scrolloff = cx.editor.config().scrolloff;

        let (view, doc) = current!(cx.editor);
        doc.set_selection(view.id, last_selection);
        view.ensure_cursor_in_view(doc, scrolloff);
    }
}

fn update_goto_line_number_preview(cx: &mut compositor::Context, args: Args) -> anyhow::Result<()> {
    cx.editor.last_selection.get_or_insert_with(|| {
        let (view, doc) = current!(cx.editor);
        doc.selection(view.id).clone()
    });

    let scrolloff = cx.editor.config().scrolloff;
    let line = args[0].parse::<usize>()?;
    goto_line_without_jumplist(
        cx.editor,
        NonZeroUsize::new(line),
        if cx.editor.mode == Mode::Select {
            Movement::Extend
        } else {
            Movement::Move
        },
    );

    let (view, doc) = current!(cx.editor);
    view.ensure_cursor_in_view(doc, scrolloff);

    Ok(())
}

pub(super) fn goto_line_number(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    match event {
        PromptEvent::Abort => abort_goto_line_number_preview(cx),
        PromptEvent::Validate => {
            // If we are invoked directly via a keybinding, Validate is
            // sent without any prior Update events. Ensure the cursor
            // is moved to the appropriate location.
            update_goto_line_number_preview(cx, args)?;

            let last_selection = cx
                .editor
                .last_selection
                .take()
                .expect("update_goto_line_number_preview should always set last_selection");

            let (view, doc) = current!(cx.editor);
            view.push_jump(doc, (doc.id(), last_selection));
        }

        // When a user hits backspace and there are no numbers left,
        // we can bring them back to their original selection. If they
        // begin typing numbers again, we'll start a new preview session.
        PromptEvent::Update if args.is_empty() => abort_goto_line_number_preview(cx),
        PromptEvent::Update => update_goto_line_number_preview(cx, args)?,
    }

    Ok(())
}

// Fetch the current value of a config option and output as status.
fn get_option(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    let key = &args[0].to_lowercase();
    let key_error = || anyhow::anyhow!("Unknown key `{}`", key);

    let config = serde_json::json!(cx.editor.config().deref());
    let pointer = format!("/{}", key.replace('.', "/"));
    let value = config.pointer(&pointer).ok_or_else(key_error)?;

    cx.editor.set_status(value.to_string());
    Ok(())
}

/// Change config at runtime. Access nested values by dot syntax, for
/// example to disable smart case search, use `:set search.smart-case false`.
/// How a vim option maps onto a zemacs config value.
#[derive(Clone, Copy)]
enum VimOptKind {
    Bool,
    /// String enum: the value used when the option is set / unset.
    Enum(&'static str, Option<&'static str>),
    Num,
}

/// vim option (and abbreviations) -> (zemacs config key, kind).
#[rustfmt::skip]
const VIM_OPTIONS: &[(&[&str], &str, VimOptKind)] = &[
    (&["number", "nu"],            "line-number",        VimOptKind::Enum("absolute", None)),
    (&["relativenumber", "rnu"],   "line-number",        VimOptKind::Enum("relative", Some("absolute"))),
    (&["wrap"],                    "soft-wrap.enable",   VimOptKind::Bool),
    (&["linebreak", "lbr"],        "soft-wrap.enable",   VimOptKind::Bool),
    (&["ignorecase", "ic"],        "search.smart-case",  VimOptKind::Bool),
    (&["smartcase", "scs"],        "search.smart-case",  VimOptKind::Bool),
    (&["wrapscan", "ws"],          "search.wrap-around", VimOptKind::Bool),
    (&["cursorline", "cul"],       "cursorline",         VimOptKind::Bool),
    (&["cursorcolumn", "cuc"],     "cursorcolumn",       VimOptKind::Bool),
    (&["scrolloff", "so"],         "scrolloff",          VimOptKind::Num),
    (&["textwidth", "tw"],         "text-width",         VimOptKind::Num),
    (&["termguicolors", "tgc"],    "true-color",         VimOptKind::Bool),
    (&["mouse"],                   "mouse",              VimOptKind::Bool),
    (&["list"],                    "whitespace.render",  VimOptKind::Enum("all", Some("none"))),
];

fn lookup_vim_option(name: &str) -> Option<(&'static str, VimOptKind)> {
    VIM_OPTIONS
        .iter()
        .find(|(names, _, _)| names.contains(&name))
        .map(|(_, key, kind)| (*key, *kind))
}

/// Parse a vim `:set` token. Returns (negated, toggle, name, value).
/// Forms: `opt`, `noopt`, `opt!`, `invopt`, `opt=val`, `opt:val`.
fn parse_set_token(tok: &str) -> (bool, bool, &str, Option<&str>) {
    let mut t = tok;
    let mut toggle = false;
    if let Some(rest) = t.strip_suffix('!') {
        t = rest;
        toggle = true;
    }
    if let Some(rest) = t.strip_prefix("inv") {
        // Only treat `inv` as a prefix when it leaves a known option name.
        if lookup_vim_option(rest).is_some() {
            return (false, true, rest, None);
        }
    }
    if let Some((name, val)) = t.split_once(['=', ':']) {
        return (false, toggle, name, Some(val));
    }
    if let Some(rest) = t.strip_prefix("no") {
        if lookup_vim_option(rest).is_some() {
            return (true, toggle, rest, None);
        }
    }
    (false, toggle, t, None)
}

/// Translate a parsed vim option into (zemacs key, JSON value). `current_bool`
/// resolves the current value for toggles. Returns None for unknown options.
fn translate_vim_option(
    tok: &str,
    current_bool: impl FnOnce(&str) -> bool,
) -> Option<anyhow::Result<(String, Value)>> {
    let (neg, toggle, name, value) = parse_set_token(tok);
    let (hkey, kind) = lookup_vim_option(name)?;
    let result = (|| -> anyhow::Result<(String, Value)> {
        let json = match kind {
            VimOptKind::Bool => {
                let v = if toggle {
                    !current_bool(hkey)
                } else if let Some(val) = value {
                    val.parse().map_err(|_| anyhow!("expected bool for {name}"))?
                } else {
                    !neg
                };
                Value::Bool(v)
            }
            VimOptKind::Enum(on, off) => {
                let s = if neg {
                    off.ok_or_else(|| anyhow!("'{name}' cannot be turned off"))?
                } else {
                    value.unwrap_or(on)
                };
                Value::String(s.to_string())
            }
            VimOptKind::Num => {
                let val = value.ok_or_else(|| anyhow!("'{name}' needs a value (e.g. {name}=4)"))?;
                let n: i64 = val.parse().map_err(|_| anyhow!("expected number for {name}"))?;
                Value::Number(n.into())
            }
        };
        Ok((hkey.to_string(), json))
    })();
    Some(result)
}

fn apply_config_value(
    cx: &mut compositor::Context,
    zemacs_key: &str,
    new_value: Value,
) -> anyhow::Result<()> {
    let mut config = serde_json::json!(&cx.editor.config().deref());
    let pointer = format!("/{}", zemacs_key.replace('.', "/"));
    let slot = config
        .pointer_mut(&pointer)
        .ok_or_else(|| anyhow!("Unknown key `{zemacs_key}`"))?;
    *slot = new_value;
    let config = serde_json::from_value(config).map_err(|e| anyhow!("{e}"))?;
    cx.editor.config_events.0.send(ConfigEvent::Update(config))?;
    Ok(())
}

/// `:set` with vim-compatible syntax (`:set nu`, `:set nowrap`, `:set tw=80`,
/// `:set cursorline`), falling back to native `:set key value`.
fn vim_set(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let tokens: Vec<String> = (0..args.len()).map(|i| args[i].to_string()).collect();

    // Native two-token form `:set <zemacs-key> <value>` when the key resolves.
    if tokens.len() == 2 {
        let cfg = serde_json::json!(&cx.editor.config().deref());
        let pointer = format!("/{}", tokens[0].replace('.', "/"));
        if let Some(slot) = cfg.pointer(&pointer) {
            let new_value = if slot.is_string() {
                Value::String(tokens[1].clone())
            } else {
                tokens[1]
                    .parse()
                    .map_err(|_| anyhow!("Could not parse `{}`", tokens[1]))?
            };
            return apply_config_value(cx, &tokens[0], new_value);
        }
    }

    for tok in &tokens {
        let current_bool = |key: &str| -> bool {
            let cfg = serde_json::json!(&cx.editor.config().deref());
            cfg.pointer(&format!("/{}", key.replace('.', "/")))
                .and_then(Value::as_bool)
                .unwrap_or(false)
        };
        match translate_vim_option(tok, current_bool) {
            Some(Ok((hkey, val))) => apply_config_value(cx, &hkey, val)?,
            Some(Err(e)) => return Err(e),
            None => return Err(anyhow!("Unknown option `{tok}`")),
        }
    }
    Ok(())
}

fn set_option(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    let (key, arg) = (&args[0].to_lowercase(), args[1].trim());

    let key_error = || anyhow::anyhow!("Unknown key `{}`", key);
    let field_error = |_| anyhow::anyhow!("Could not parse field `{}`", arg);

    let mut config = serde_json::json!(&cx.editor.config().deref());
    let pointer = format!("/{}", key.replace('.', "/"));
    let value = config.pointer_mut(&pointer).ok_or_else(key_error)?;

    *value = if value.is_string() {
        // JSON strings require quotes, so we can't .parse() directly
        Value::String(arg.to_string())
    } else {
        arg.parse().map_err(field_error)?
    };
    let config = serde_json::from_value(config).map_err(field_error)?;

    cx.editor
        .config_events
        .0
        .send(ConfigEvent::Update(config))?;
    Ok(())
}

/// Toggle boolean config option at runtime. Access nested values by dot
/// syntax, for example to toggle smart case search, use `:toggle search.smart-
/// case`.
fn toggle_option(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    let key = &args[0].to_lowercase();

    let key_error = || anyhow::anyhow!("Unknown key `{}`", key);

    let mut config = serde_json::json!(&cx.editor.config().deref());
    let pointer = format!("/{}", key.replace('.', "/"));
    let value = config.pointer_mut(&pointer).ok_or_else(key_error)?;

    *value = match value {
        Value::Bool(ref value) => {
            ensure!(
                args.len() == 1,
                "Bad arguments. For boolean configurations use: `:toggle {key}`"
            );
            Value::Bool(!value)
        }
        Value::String(ref value) => {
            ensure!(
                args.len() == 2,
                "Bad arguments. For string configurations use: `:toggle {key} val1 val2 ...`",
            );
            // For string values, parse the input according to normal command line rules.
            let values: Vec<_> = command_line::Tokenizer::new(&args[1], true)
                .map(|res| res.map(|token| token.content))
                .collect::<Result<_, _>>()
                .map_err(|err| anyhow!("failed to parse values: {err}"))?;

            Value::String(
                values
                    .iter()
                    .skip_while(|e| *e != value)
                    .nth(1)
                    .map(AsRef::as_ref)
                    .unwrap_or_else(|| &values[0])
                    .to_string(),
            )
        }
        Value::Null => bail!("Configuration {key} cannot be toggled"),
        Value::Number(_) | Value::Array(_) | Value::Object(_) => {
            ensure!(
                args.len() == 2,
                "Bad arguments. For {kind} configurations use: `:toggle {key} val1 val2 ...`",
                kind = match value {
                    Value::Number(_) => "number",
                    Value::Array(_) => "array",
                    Value::Object(_) => "object",
                    _ => unreachable!(),
                }
            );
            // For numbers, arrays and objects, parse each argument with
            // `serde_json::StreamDeserializer`.
            let values: Vec<Value> = serde_json::Deserializer::from_str(&args[1])
                .into_iter()
                .collect::<Result<_, _>>()
                .map_err(|err| anyhow!("failed to parse value: {err}"))?;

            if let Some(wrongly_typed_value) = values
                .iter()
                .find(|v| std::mem::discriminant(*v) != std::mem::discriminant(&*value))
            {
                bail!("value '{wrongly_typed_value}' has a different type than '{value}'");
            }

            values
                .iter()
                .skip_while(|e| *e != value)
                .nth(1)
                .unwrap_or(&values[0])
                .clone()
        }
    };

    let status = format!("'{key}' is now set to {value}");
    let config = serde_json::from_value(config)
        .map_err(|err| anyhow::anyhow!("Failed to parse config: {err}"))?;

    cx.editor
        .config_events
        .0
        .send(ConfigEvent::Update(config))?;
    cx.editor.set_status(status);
    Ok(())
}

/// Change the language of the current buffer at runtime.
fn language(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    if args.is_empty() {
        let doc = doc!(cx.editor);
        let language = &doc.language_name().unwrap_or(DEFAULT_LANGUAGE_NAME);
        cx.editor.set_status(language.to_string());
        return Ok(());
    }

    let doc = doc_mut!(cx.editor);

    let loader = cx.editor.syn_loader.load();
    if &args[0] == DEFAULT_LANGUAGE_NAME {
        doc.set_language(None, &loader)
    } else {
        doc.set_language_by_language_id(&args[0], &loader)?;
    }
    doc.detect_indent_and_line_ending();

    let id = doc.id();
    cx.editor.refresh_language_servers(id);
    let doc = doc_mut!(cx.editor);
    let diagnostics =
        Editor::doc_diagnostics(&cx.editor.language_servers, &cx.editor.diagnostics, doc);
    doc.replace_diagnostics(diagnostics, &[], None);
    Ok(())
}

/// Swap consecutive line `upper` with the line below it, keeping the primary
/// cursor on the line it started on. `cursor_on_upper` selects which of the two
/// swapped lines the cursor follows (true = the originally-upper line).
fn swap_lines(cx: &mut compositor::Context, upper: usize, cursor_on_upper: bool) {
    let (view, doc) = current!(cx.editor);
    let line_ending = doc.line_ending.as_str();
    let slice = doc.text().slice(..);
    let total_lines = slice.len_lines();

    if upper + 1 >= total_lines {
        return;
    }

    let start = slice.line_to_char(upper);
    let mid = slice.line_to_char(upper + 1);
    let end = slice.line_to_char((upper + 2).min(total_lines));

    let first: Tendril = slice.slice(start..mid).chunks().collect();
    let second: Tendril = slice.slice(mid..end).chunks().collect();
    if second.is_empty() {
        // The line below is the phantom trailing empty line — nothing to swap.
        return;
    }

    // Build "second then first". If `second` is the last line (no trailing
    // newline) move `first`'s ending in front of it so line structure is kept.
    let swapped: Tendril = if second.ends_with('\n') {
        format!("{second}{first}").into()
    } else {
        let body = first
            .strip_suffix('\n')
            .map(|s| s.strip_suffix('\r').unwrap_or(s))
            .unwrap_or(&first);
        format!("{second}{line_ending}{body}").into()
    };

    // Preserve the cursor's column on whichever line it followed.
    let cursor = doc.selection(view.id).primary().cursor(slice);
    let cursor_line = if cursor_on_upper { upper } else { upper + 1 };
    let col = cursor.saturating_sub(slice.line_to_char(cursor_line));

    let transaction = Transaction::change(doc.text(), std::iter::once((start, end, Some(swapped))));
    doc.apply(&transaction, view.id);

    // After the swap the followed line sits at the opposite index.
    let new_slice = doc.text().slice(..);
    let new_line = if cursor_on_upper { upper + 1 } else { upper };
    let line_start = new_slice.line_to_char(new_line);
    let line_end = line_ending::line_end_char_index(&new_slice, new_line);
    let new_cursor = (line_start + col).min(line_end);
    doc.set_selection(view.id, Selection::point(new_cursor));
    doc.append_changes_to_history(view);
}

fn move_line_down(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let slice = doc.text().slice(..);
    let line = slice.char_to_line(doc.selection(view.id).primary().cursor(slice));
    // Swap this line with the one below; cursor follows the originally-upper line.
    swap_lines(cx, line, true);
    Ok(())
}

fn move_line_up(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let slice = doc.text().slice(..);
    let line = slice.char_to_line(doc.selection(view.id).primary().cursor(slice));
    if line == 0 {
        return Ok(());
    }
    // Swap the line above with this line; cursor follows the originally-lower line.
    swap_lines(cx, line - 1, false);
    Ok(())
}

fn transpose_chars(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    let (view, doc) = current!(cx.editor);
    let slice = doc.text().slice(..);
    let len = slice.len_chars();
    let cursor = doc.selection(view.id).primary().cursor(slice);

    // Choose the adjacent pair (i, i+1) to swap. At end of buffer, transpose the
    // final two characters (emacs C-t behaviour).
    let i = if cursor == 0 {
        return Ok(());
    } else if cursor >= len {
        if len < 2 {
            return Ok(());
        }
        len - 2
    } else {
        cursor - 1
    };

    let a = slice.char(i);
    let b = slice.char(i + 1);
    // Don't transpose across line boundaries.
    if a == '\n' || b == '\n' {
        return Ok(());
    }

    let swapped: Tendril = format!("{b}{a}").into();
    let transaction = Transaction::change(doc.text(), std::iter::once((i, i + 2, Some(swapped))));
    doc.apply(&transaction, view.id);
    // Emacs moves point forward past the transposed pair.
    let new_cursor = (i + 2).min(doc.text().len_chars());
    doc.set_selection(view.id, Selection::point(new_cursor));
    doc.append_changes_to_history(view);
    Ok(())
}

/// Split a symbol into lowercase words on `_`, `-`, and camelCase boundaries.
fn split_symbol_words(s: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut cur = String::new();
    let mut prev: Option<char> = None;
    for c in s.chars() {
        if c == '_' || c == '-' {
            if !cur.is_empty() {
                words.push(std::mem::take(&mut cur));
            }
        } else if c.is_uppercase() && prev.is_some_and(|p| p.is_lowercase() || p.is_numeric()) {
            if !cur.is_empty() {
                words.push(std::mem::take(&mut cur));
            }
            cur.push(c);
        } else {
            cur.push(c);
        }
        prev = Some(c);
    }
    if !cur.is_empty() {
        words.push(cur);
    }
    words.iter().map(|w| w.to_lowercase()).collect()
}

fn capitalize(w: &str) -> String {
    let mut chars = w.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// Rejoin the words of `sym` in the given case `style`. Returns None for an
/// unknown style.
fn symbol_to_case(sym: &str, style: &str) -> Option<String> {
    let words = split_symbol_words(sym);
    Some(match style {
        "snake" => words.join("_"),
        "kebab" => words.join("-"),
        "camel" => words
            .iter()
            .enumerate()
            .map(|(i, w)| if i == 0 { w.clone() } else { capitalize(w) })
            .collect(),
        "pascal" => words.iter().map(|w| capitalize(w)).collect(),
        _ => return None,
    })
}

/// Detect the case style of a symbol, for cycling.
fn detect_case(sym: &str) -> &'static str {
    if sym.contains('_') {
        "snake"
    } else if sym.contains('-') {
        "kebab"
    } else if sym.chars().next().is_some_and(char::is_uppercase) {
        "pascal"
    } else if sym.chars().any(char::is_uppercase) {
        "camel"
    } else {
        "lower"
    }
}

/// Apply `to_style` (a closure of the detected style) to the symbol under the
/// cursor.
fn transform_symbol_under_cursor(
    cx: &mut compositor::Context,
    to_style: impl FnOnce(&str) -> anyhow::Result<String>,
) -> anyhow::Result<()> {
    let (view, doc) = current!(cx.editor);
    let slice = doc.text().slice(..);
    let len = slice.len_chars();
    let cursor = doc.selection(view.id).primary().cursor(slice).min(len);

    let is_sym = |c: char| c.is_alphanumeric() || c == '_' || c == '-';
    let mut start = cursor;
    while start > 0 && is_sym(slice.char(start - 1)) {
        start -= 1;
    }
    let mut end = cursor;
    while end < len && is_sym(slice.char(end)) {
        end += 1;
    }
    if start == end {
        return Ok(()); // no symbol under cursor
    }

    let sym: String = slice.slice(start..end).chunks().collect();
    let result = to_style(&sym)?;

    let transaction = Transaction::change(
        doc.text(),
        std::iter::once((start, end, Some(Tendril::from(result.as_str())))),
    );
    doc.apply(&transaction, view.id);
    doc.set_selection(view.id, Selection::point(start.min(doc.text().len_chars())));
    doc.append_changes_to_history(view);
    Ok(())
}

fn change_case(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let style = args[0].to_lowercase();
    transform_symbol_under_cursor(cx, |sym| {
        symbol_to_case(sym, &style)
            .ok_or_else(|| anyhow!("Unknown case style `{style}` (use camel|snake|kebab|pascal)"))
    })
}

fn cycle_case(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    transform_symbol_under_cursor(cx, |sym| {
        // snake -> camel -> kebab -> pascal -> snake (lower joins the cycle at camel)
        let next = match detect_case(sym) {
            "snake" | "lower" => "camel",
            "camel" => "kebab",
            "kebab" => "pascal",
            _ => "snake",
        };
        Ok(symbol_to_case(sym, next).expect("known style"))
    })
}

/// Translate a vim `:s` replacement string to `regex` crate replacement syntax.
/// `\1`..`\9` and `\0`/`&` become `${1}`..`${0}`; `\n`/`\t` become real
/// newline/tab; `\&`/`\\` are literal; a literal `$` is escaped to `$$`.
fn vim_replacement_to_regex(rep: &str) -> String {
    let mut out = String::with_capacity(rep.len());
    let mut chars = rep.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '&' => out.push_str("${0}"),
            '$' => out.push_str("$$"),
            '\\' => match chars.next() {
                Some(d) if d.is_ascii_digit() => {
                    out.push_str("${");
                    out.push(d);
                    out.push('}');
                }
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('&') => out.push('&'),
                Some('\\') => out.push('\\'),
                Some(other) => out.push(other),
                None => out.push('\\'),
            },
            other => out.push(other),
        }
    }
    out
}

/// Parse a vim-style substitute command line: `[%]s<delim>pat<delim>rep<delim>flags`.
/// Returns (whole_file, pattern, replacement, flags) or None if `input` is not a
/// substitute.
fn parse_vim_substitute(input: &str) -> Option<(bool, String, String, String)> {
    let s = input.trim_start();
    let (whole, s) = match s.strip_prefix('%') {
        Some(rest) => (true, rest.trim_start()),
        None => (false, s),
    };
    // Command name: any prefix of "substitute" of length >= 1, immediately
    // followed by a non-alphanumeric, non-space delimiter.
    let name_len = (1..="substitute".len())
        .rev()
        .find(|&n| s.starts_with(&"substitute"[..n]))?;
    let after = &s[name_len..];
    let delim = after.chars().next()?;
    if delim.is_alphanumeric() || delim.is_whitespace() {
        return None;
    }
    let body = &after[delim.len_utf8()..];
    let mut parts = body.splitn(3, delim);
    let pattern = parts.next()?.to_string();
    if pattern.is_empty() {
        return None;
    }
    let replacement = parts.next().unwrap_or("").to_string();
    let flags = parts.next().unwrap_or("").to_string();
    Some((whole, pattern, replacement, flags))
}

/// Run a substitute over the target lines. `whole_file` selects the whole
/// buffer; otherwise the lines spanned by the primary selection are used.
pub(crate) fn do_substitute(
    editor: &mut zemacs_view::Editor,
    whole_file: bool,
    pattern: &str,
    replacement: &str,
    flags: &str,
) -> anyhow::Result<()> {
    let global = flags.contains('g');
    let case_insensitive = flags.contains('i');

    let re = regex::RegexBuilder::new(pattern)
        .case_insensitive(case_insensitive)
        .build()
        .map_err(|e| anyhow!("invalid pattern: {e}"))?;
    let rep = vim_replacement_to_regex(replacement);

    // Remember for vim `&` (repeat last substitute).
    editor.last_substitute = Some((
        pattern.to_string(),
        replacement.to_string(),
        flags.to_string(),
    ));

    let (view, doc) = current!(editor);
    let slice = doc.text().slice(..);
    let total = slice.len_lines();

    let (first_line, last_line) = if whole_file {
        (0, total.saturating_sub(1))
    } else {
        let sel = doc.selection(view.id).primary();
        (
            slice.char_to_line(sel.from()),
            slice.char_to_line(sel.to().min(slice.len_chars().saturating_sub(1).max(0))),
        )
    };

    let lines = (first_line..=last_line).filter(|&l| l < total);
    let changes = substitute_changes(&slice, lines, &re, &rep, global);

    if changes.is_empty() {
        return Ok(());
    }

    let transaction = Transaction::change(doc.text(), changes.into_iter());
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Build the per-line replacement changes for a substitute over `lines`.
fn substitute_changes(
    slice: &zemacs_core::ropey::RopeSlice,
    lines: impl Iterator<Item = usize>,
    re: &regex::Regex,
    rep: &str,
    global: bool,
) -> Vec<(usize, usize, Option<Tendril>)> {
    let mut changes = Vec::new();
    for line in lines {
        let lstart = slice.line_to_char(line);
        let lend = line_ending::line_end_char_index(slice, line);
        if lstart > lend {
            continue;
        }
        let text: std::borrow::Cow<str> = slice.slice(lstart..lend).into();
        let new = if global {
            re.replace_all(&text, rep)
        } else {
            re.replacen(&text, 1, rep)
        };
        if new != text {
            changes.push((lstart, lend, Some(Tendril::from(new.as_ref()))));
        }
    }
    changes
}

/// Match the longest prefix (>= 1 char) of `name` that `s` starts with, where
/// the following character is a delimiter (`!` or non-alphanumeric). Returns the
/// remainder after the matched name. Prevents matching e.g. `goto` for `global`.
fn match_command_prefix<'a>(s: &'a str, name: &str) -> Option<&'a str> {
    (1..=name.len()).rev().find_map(|n| {
        if s.starts_with(&name[..n]) {
            let next = s[n..].chars().next();
            match next {
                Some('!') => Some(&s[n..]),
                Some(c) if !c.is_alphanumeric() && !c.is_whitespace() => Some(&s[n..]),
                _ => None,
            }
        } else {
            None
        }
    })
}

/// Parse a vim global command line: `g/pat/cmd`, `g!/pat/cmd`, `v/pat/cmd`.
/// Returns (invert, pattern, command) or None.
fn parse_vim_global(input: &str) -> Option<(bool, String, String)> {
    let s = input.trim_start();
    let (mut invert, rest) = if let Some(r) = match_command_prefix(s, "vglobal") {
        (true, r)
    } else if let Some(r) = match_command_prefix(s, "global") {
        (false, r)
    } else {
        return None;
    };
    let rest = if let Some(r) = rest.strip_prefix('!') {
        invert = !invert;
        r
    } else {
        rest
    };
    let delim = rest.chars().next()?;
    if delim.is_alphanumeric() || delim.is_whitespace() {
        return None;
    }
    let body = &rest[delim.len_utf8()..];
    let mut parts = body.splitn(2, delim);
    let pattern = parts.next()?.to_string();
    if pattern.is_empty() {
        return None;
    }
    let command = parts.next().unwrap_or("").trim().to_string();
    Some((invert, pattern, command))
}

/// `:g/pat/{d,s/.../.../ }` and the `:v` inverse: on lines (not) matching
/// `pattern`, run the action `command` — delete (`d`) or substitute (`s`).
fn do_global(
    cx: &mut compositor::Context,
    invert: bool,
    pattern: &str,
    command: &str,
) -> anyhow::Result<()> {
    let re = regex::Regex::new(pattern).map_err(|e| anyhow!("invalid pattern: {e}"))?;

    let (view, doc) = current!(cx.editor);
    let slice = doc.text().slice(..);
    let total = slice.len_lines();
    let len = slice.len_chars();

    // The (non-)matching line numbers, skipping the phantom trailing line.
    let mut targets = Vec::new();
    for line in 0..total {
        let lstart = slice.line_to_char(line);
        let next = if line + 1 < total {
            slice.line_to_char(line + 1)
        } else {
            len
        };
        if lstart == next {
            continue;
        }
        let lend = line_ending::line_end_char_index(&slice, line);
        let text: std::borrow::Cow<str> = slice.slice(lstart..lend).into();
        if re.is_match(&text) != invert {
            targets.push(line);
        }
    }
    if targets.is_empty() {
        return Ok(());
    }

    let changes = if matches!(command, "d" | "delete") {
        targets
            .iter()
            .map(|&line| {
                let lstart = slice.line_to_char(line);
                let next = if line + 1 < total {
                    slice.line_to_char(line + 1)
                } else {
                    len
                };
                (lstart, next, None)
            })
            .collect::<Vec<_>>()
    } else if let Some(rest) = match_command_prefix(command, "substitute") {
        let delim = rest
            .chars()
            .next()
            .filter(|c| !c.is_alphanumeric() && !c.is_whitespace())
            .ok_or_else(|| anyhow!("global: bad substitute command"))?;
        let body = &rest[delim.len_utf8()..];
        let mut parts = body.splitn(3, delim);
        let pat2 = parts.next().unwrap_or("");
        let rep2 = vim_replacement_to_regex(parts.next().unwrap_or(""));
        let flags2 = parts.next().unwrap_or("");
        if pat2.is_empty() {
            bail!("global: empty substitute pattern");
        }
        let re2 = regex::RegexBuilder::new(pat2)
            .case_insensitive(flags2.contains('i'))
            .build()
            .map_err(|e| anyhow!("invalid pattern: {e}"))?;
        substitute_changes(
            &slice,
            targets.iter().copied(),
            &re2,
            &rep2,
            flags2.contains('g'),
        )
    } else {
        bail!("global: only 'd' (delete) and 's/.../.../' are supported");
    };

    if changes.is_empty() {
        return Ok(());
    }

    let transaction = Transaction::change(doc.text(), changes.into_iter());
    doc.apply(&transaction, view.id);
    doc.set_selection(view.id, Selection::point(0.min(doc.text().len_chars())));
    doc.append_changes_to_history(view);
    Ok(())
}

fn global_command(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
    invert: bool,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let raw = args[0].trim();
    let delim = raw
        .chars()
        .next()
        .filter(|c| !c.is_alphanumeric() && !c.is_whitespace())
        .ok_or_else(|| anyhow!("usage: :global/pattern/command"))?;
    let body = &raw[delim.len_utf8()..];
    let mut parts = body.splitn(2, delim);
    let pattern = parts.next().unwrap_or("");
    let command = parts.next().unwrap_or("").trim();
    if pattern.is_empty() {
        bail!("usage: :global/pattern/command");
    }
    do_global(cx, invert, pattern, command)
}

fn global(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    global_command(cx, args, event, false)
}

fn vglobal(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    global_command(cx, args, event, true)
}

/// Resolve a vim line address to a 1-based line number (0 = before the first
/// line). Supports `N`, `.`, `$`, `.+N`, `.-N`, `$-N`, `+N`, `-N`.
fn parse_line_address(addr: &str, current: usize, last: usize) -> anyhow::Result<usize> {
    let a = addr.trim();
    let rel = |base: usize, s: &str| -> anyhow::Result<usize> {
        if s.is_empty() {
            return Ok(base);
        }
        let n: isize = s.parse().map_err(|_| anyhow!("invalid address: {addr}"))?;
        Ok((base as isize + n).max(0) as usize)
    };
    if a == "." {
        Ok(current)
    } else if a == "$" {
        Ok(last)
    } else if let Some(r) = a.strip_prefix('.') {
        rel(current, r)
    } else if let Some(r) = a.strip_prefix('$') {
        rel(last, r)
    } else if a.starts_with(['+', '-']) {
        rel(current, a)
    } else {
        a.parse::<usize>()
            .map_err(|_| anyhow!("invalid address: {addr}"))
    }
}

/// Parse a vim move/copy command with no space: `m5`, `t.`, `co$`, `copy0`.
/// Returns (is_copy, address).
fn parse_vim_lineop(input: &str) -> Option<(bool, String)> {
    let s = input.trim();
    let is_addr = |c: char| c.is_ascii_digit() || matches!(c, '.' | '$' | '+' | '-');
    if let Some(rest) = s.strip_prefix('m') {
        if rest.chars().next().is_some_and(is_addr) {
            return Some((false, rest.to_string()));
        }
    }
    for pfx in ["copy", "co", "t"] {
        if let Some(rest) = s.strip_prefix(pfx) {
            if rest.chars().next().is_some_and(is_addr) {
                return Some((true, rest.to_string()));
            }
        }
    }
    None
}

/// `:m{addr}` (move) / `:t{addr}` (copy): relocate or duplicate the current
/// line to after line `addr`.
fn do_move_copy(cx: &mut compositor::Context, is_copy: bool, addr: &str) -> anyhow::Result<()> {
    let (view, doc) = current!(cx.editor);
    let line_ending = doc.line_ending.as_str();
    let slice = doc.text().slice(..);
    let len = slice.len_chars();
    let total_lines = slice.len_lines();
    // Number of real lines (exclude the phantom trailing empty line).
    let last = if len > 0 && slice.char(len - 1) == '\n' {
        total_lines - 1
    } else {
        total_lines
    };

    let cur0 = slice.char_to_line(doc.selection(view.id).primary().cursor(slice));
    let target1 = parse_line_address(addr, cur0 + 1, last)?.min(last);

    let src_start = slice.line_to_char(cur0);
    let src_end = if cur0 + 1 < total_lines {
        slice.line_to_char(cur0 + 1)
    } else {
        len
    };
    let src_text: Tendril = slice.slice(src_start..src_end).chunks().collect();
    let src_norm: Tendril = if src_text.ends_with('\n') {
        src_text
    } else {
        format!("{src_text}{line_ending}").into()
    };

    // Insertion point = start of the line after the 1-based target line.
    let insert_at = slice.line_to_char(target1.min(total_lines.saturating_sub(0)).min(last));

    if !is_copy && insert_at >= src_start && insert_at <= src_end {
        return Ok(()); // moving a line onto itself
    }

    let mut changes: Vec<(usize, usize, Option<Tendril>)> = Vec::new();
    if !is_copy {
        changes.push((src_start, src_end, None));
    }
    changes.push((insert_at, insert_at, Some(src_norm)));
    changes.sort_by_key(|c| c.0);

    let transaction = Transaction::change(doc.text(), changes.into_iter());
    doc.apply(&transaction, view.id);

    // Put the cursor on the relocated/copied line.
    let new_slice = doc.text().slice(..);
    let landed = if !is_copy && insert_at > src_end {
        insert_at - (src_end - src_start)
    } else {
        insert_at
    };
    let line = new_slice.char_to_line(landed.min(new_slice.len_chars()));
    doc.set_selection(view.id, Selection::point(new_slice.line_to_char(line)));
    doc.append_changes_to_history(view);
    Ok(())
}

fn line_op_command(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
    is_copy: bool,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    do_move_copy(cx, is_copy, args[0].trim())
}

fn move_lines(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    line_op_command(cx, args, event, false)
}

fn copy_lines(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    line_op_command(cx, args, event, true)
}

/// The line range spanned by the primary selection.
fn primary_line_range(doc: &zemacs_view::Document, view: zemacs_view::ViewId) -> (usize, usize) {
    let slice = doc.text().slice(..);
    let sel = doc.selection(view).primary();
    let first = slice.char_to_line(sel.from());
    let last = slice.char_to_line(sel.to().saturating_sub(1).max(sel.from()));
    (first, last)
}

fn do_indent(cx: &mut compositor::Context, dedent: bool) -> anyhow::Result<()> {
    let (view, doc) = current!(cx.editor);
    let unit = doc.indent_style.as_str().to_string();
    let indent_width = doc.indent_style.indent_width(doc.tab_width());
    let tab_width = doc.tab_width();
    let slice = doc.text().slice(..);
    let (first, last) = primary_line_range(doc, view.id);

    let mut changes = Vec::new();
    for line in first..=last {
        let lstart = slice.line_to_char(line);
        let lend = line_ending::line_end_char_index(&slice, line);
        if dedent {
            // Remove up to one indent level worth of leading whitespace.
            let mut width = 0;
            let mut removed = 0;
            for ch in slice.slice(lstart..lend).chars() {
                if width >= indent_width {
                    break;
                }
                match ch {
                    ' ' => width += 1,
                    '\t' => width += tab_width,
                    _ => break,
                }
                removed += 1;
            }
            if removed > 0 {
                changes.push((lstart, lstart + removed, None));
            }
        } else if lend > lstart {
            // Indent non-empty lines only (vim `>` skips blank lines).
            changes.push((lstart, lstart, Some(Tendril::from(unit.as_str()))));
        }
    }
    if changes.is_empty() {
        return Ok(());
    }
    let transaction = Transaction::change(doc.text(), changes.into_iter());
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

fn indent_cmd(cx: &mut compositor::Context, _args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    do_indent(cx, false)
}
fn dedent_cmd(cx: &mut compositor::Context, _args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    do_indent(cx, true)
}

fn yank_lines(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
    delete: bool,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (start, end, text) = {
        let (view, doc) = current!(cx.editor);
        let slice = doc.text().slice(..);
        let total = slice.len_lines();
        let (first, last) = primary_line_range(doc, view.id);
        let start = slice.line_to_char(first);
        let end = if last + 1 < total {
            slice.line_to_char(last + 1)
        } else {
            slice.len_chars()
        };
        let text: String = slice.slice(start..end).chunks().collect();
        (start, end, text)
    };

    cx.editor.registers.write('"', vec![text])?;

    if delete {
        let (view, doc) = current!(cx.editor);
        let transaction = Transaction::change(doc.text(), std::iter::once((start, end, None)));
        doc.apply(&transaction, view.id);
        doc.append_changes_to_history(view);
    }
    Ok(())
}

fn put_lines_impl(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
    above: bool,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let reg = args
        .first()
        .and_then(|a| a.trim().chars().next())
        .unwrap_or('"');

    let content: String = {
        match cx.editor.registers.read(reg, cx.editor) {
            Some(values) => values.collect::<Vec<_>>().join(""),
            None => return Ok(()),
        }
    };
    if content.is_empty() {
        return Ok(());
    }

    let (view, doc) = current!(cx.editor);
    let line_ending = doc.line_ending.as_str();
    let slice = doc.text().slice(..);
    let len = slice.len_chars();
    let total = slice.len_lines();
    let cur = slice.char_to_line(doc.selection(view.id).primary().cursor(slice));
    let insert_at = if above {
        slice.line_to_char(cur)
    } else if cur + 1 < total {
        slice.line_to_char(cur + 1)
    } else {
        len
    };

    // Ensure the put text forms whole line(s) below the current line.
    let mut text = content;
    if !text.ends_with('\n') {
        text.push_str(line_ending);
    }
    if insert_at == len && len > 0 && slice.char(len - 1) != '\n' {
        text.insert_str(0, line_ending);
    }

    let transaction =
        Transaction::change(doc.text(), std::iter::once((insert_at, insert_at, Some(Tendril::from(text.as_str())))));
    doc.apply(&transaction, view.id);
    // Place the cursor at the start of the first put line.
    let new_slice = doc.text().slice(..);
    let line = new_slice.char_to_line(insert_at.min(new_slice.len_chars()));
    doc.set_selection(view.id, Selection::point(new_slice.line_to_char(line)));
    doc.append_changes_to_history(view);
    Ok(())
}

fn put_lines(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    put_lines_impl(cx, args, event, false)
}
fn put_lines_above(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    put_lines_impl(cx, args, event, true)
}

fn join_lines(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
    with_space: bool,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let slice = doc.text().slice(..);
    let len = slice.len_chars();
    let total = slice.len_lines();
    let real_last = if len > 0 && slice.char(len - 1) == '\n' {
        total.saturating_sub(2)
    } else {
        total.saturating_sub(1)
    };

    let (first, last) = primary_line_range(doc, view.id);
    // A single-line selection joins the current line with the next.
    let last = if first == last {
        (last + 1).min(real_last)
    } else {
        last
    };
    if last <= first {
        return Ok(());
    }

    let mut changes = Vec::new();
    let mut join_pos = slice.line_to_char(first);
    for line in first..last {
        let lend = line_ending::line_end_char_index(&slice, line);
        let next_start = slice.line_to_char(line + 1);
        let next_end = line_ending::line_end_char_index(&slice, line + 1);
        let mut content = next_start;
        while content < next_end && matches!(slice.char(content), ' ' | '\t') {
            content += 1;
        }
        let sep: Tendril = if with_space && lend > slice.line_to_char(line) && content < next_end {
            " ".into()
        } else {
            "".into()
        };
        join_pos = lend;
        changes.push((lend, content, Some(sep)));
    }

    if changes.is_empty() {
        return Ok(());
    }
    let transaction = Transaction::change(doc.text(), changes.into_iter());
    doc.apply(&transaction, view.id);
    let new_len = doc.text().len_chars();
    doc.set_selection(view.id, Selection::point(join_pos.min(new_len)));
    doc.append_changes_to_history(view);
    Ok(())
}

#[derive(Clone, Copy)]
enum AlignMode {
    Left,
    Right,
    Center,
}

fn align_lines(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
    mode: AlignMode,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let arg: Option<usize> = args.first().and_then(|a| a.trim().parse().ok());
    let width = arg.unwrap_or(80);

    let (view, doc) = current!(cx.editor);
    let slice = doc.text().slice(..);
    let (first, last) = primary_line_range(doc, view.id);

    let mut changes = Vec::new();
    for line in first..=last {
        let lstart = slice.line_to_char(line);
        let lend = line_ending::line_end_char_index(&slice, line);
        let mut fnw = lstart;
        while fnw < lend && matches!(slice.char(fnw), ' ' | '\t') {
            fnw += 1;
        }
        if fnw == lend {
            continue; // blank line
        }
        let mut lnw = lend;
        while lnw > fnw && matches!(slice.char(lnw - 1), ' ' | '\t') {
            lnw -= 1;
        }
        let content_len = lnw - fnw;
        let pad = match mode {
            AlignMode::Left => arg.unwrap_or(0),
            AlignMode::Right => width.saturating_sub(content_len),
            AlignMode::Center => width.saturating_sub(content_len) / 2,
        };
        // Replace leading whitespace with `pad` spaces; strip trailing whitespace.
        changes.push((lstart, fnw, Some(Tendril::from(" ".repeat(pad).as_str()))));
        if lnw < lend {
            changes.push((lnw, lend, None));
        }
    }
    if changes.is_empty() {
        return Ok(());
    }
    changes.sort_by_key(|c| c.0);
    let transaction = Transaction::change(doc.text(), changes.into_iter());
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

fn left_cmd(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    align_lines(cx, args, event, AlignMode::Left)
}
fn right_cmd(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    align_lines(cx, args, event, AlignMode::Right)
}
fn center_cmd(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    align_lines(cx, args, event, AlignMode::Center)
}

fn undo_cmd(cx: &mut compositor::Context, _args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    doc.undo(view);
    Ok(())
}

fn redo_cmd(cx: &mut compositor::Context, _args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    doc.redo(view);
    Ok(())
}

fn retab(cx: &mut compositor::Context, _args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let tab_width = doc.tab_width();
    let spaces = Tendril::from(" ".repeat(tab_width).as_str());
    let slice = doc.text().slice(..);

    let mut changes = Vec::new();
    for (i, ch) in slice.chars().enumerate() {
        if ch == '\t' {
            changes.push((i, i + 1, Some(spaces.clone())));
        }
    }
    if changes.is_empty() {
        return Ok(());
    }
    let transaction = Transaction::change(doc.text(), changes.into_iter());
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

fn join_lines_cmd(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    join_lines(cx, args, event, true)
}
fn join_lines_nospace_cmd(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    join_lines(cx, args, event, false)
}

fn yank_lines_cmd(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    yank_lines(cx, args, event, false)
}
fn delete_lines_cmd(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    yank_lines(cx, args, event, true)
}

fn substitute(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    // Raw `/pat/rep/flags` argument (space form: `:s /p/r/g`). The no-space vim
    // form `:s/p/r/g` is handled earlier in execute_command_line.
    let raw = args[0].trim();
    let delim = raw
        .chars()
        .next()
        .filter(|c| !c.is_alphanumeric() && !c.is_whitespace())
        .ok_or_else(|| anyhow!("usage: :s/pattern/replacement/[flags]"))?;
    let body = &raw[delim.len_utf8()..];
    let mut parts = body.splitn(3, delim);
    let pattern = parts.next().unwrap_or("");
    let replacement = parts.next().unwrap_or("");
    let flags = parts.next().unwrap_or("");
    if pattern.is_empty() {
        bail!("usage: :s/pattern/replacement/[flags]");
    }
    do_substitute(cx.editor, false, pattern, replacement, flags)
}

fn split_line(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    let (view, doc) = current!(cx.editor);
    let line_ending = Tendril::from(doc.line_ending.as_str());
    let slice = doc.text().slice(..);
    let cursor = doc.selection(view.id).primary().cursor(slice);

    // Insert a line ending at the cursor, pushing the rest of the line down,
    // but keep the cursor where it was (emacs `split-line`).
    let transaction =
        Transaction::change(doc.text(), std::iter::once((cursor, cursor, Some(line_ending))));
    doc.apply(&transaction, view.id);
    doc.set_selection(view.id, Selection::point(cursor));
    doc.append_changes_to_history(view);
    Ok(())
}

fn just_one_space(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    let (view, doc) = current!(cx.editor);
    let slice = doc.text().slice(..);
    let len = slice.len_chars();
    let cursor = doc.selection(view.id).primary().cursor(slice).min(len);

    // Expand over the run of spaces/tabs surrounding the cursor (not newlines).
    let mut start = cursor;
    while start > 0 && matches!(slice.char(start - 1), ' ' | '\t') {
        start -= 1;
    }
    let mut end = cursor;
    while end < len && matches!(slice.char(end), ' ' | '\t') {
        end += 1;
    }

    // Replace the run with exactly one space (emacs `just-one-space`). When
    // there is no surrounding whitespace, insert a single space at the cursor.
    let transaction = Transaction::change(
        doc.text(),
        std::iter::once((start, end, Some(Tendril::from(" ")))),
    );
    doc.apply(&transaction, view.id);
    doc.set_selection(view.id, Selection::point((start + 1).min(doc.text().len_chars())));
    doc.append_changes_to_history(view);
    Ok(())
}

fn delete_blank_lines(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    let (view, doc) = current!(cx.editor);
    let slice = doc.text().slice(..);
    let total = slice.len_lines();
    let len = slice.len_chars();

    let is_blank = |line: usize| {
        let start = slice.line_to_char(line);
        let end = line_ending::line_end_char_index(&slice, line);
        slice.slice(start..end).chars().all(|c| c == ' ' || c == '\t')
    };

    // Collapse each run of consecutive blank lines down to a single blank line.
    let mut changes = Vec::new();
    let mut line = 0;
    while line < total {
        if is_blank(line) {
            let mut run_end = line;
            while run_end < total && is_blank(run_end) {
                run_end += 1;
            }
            if run_end - line > 1 {
                let del_start = slice.line_to_char(line + 1);
                let del_end = if run_end < total {
                    slice.line_to_char(run_end)
                } else {
                    len
                };
                if del_start < del_end {
                    changes.push((del_start, del_end, None));
                }
            }
            line = run_end;
        } else {
            line += 1;
        }
    }

    if changes.is_empty() {
        return Ok(());
    }

    let transaction = Transaction::change(doc.text(), changes.into_iter());
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

fn uniquify_lines(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    let (view, doc) = current!(cx.editor);
    let slice = doc.text().slice(..);
    let total = slice.len_lines();
    let len = slice.len_chars();

    let mut seen = std::collections::HashSet::new();
    let mut changes = Vec::new();
    for line in 0..total {
        let start = slice.line_to_char(line);
        let end = if line + 1 < total {
            slice.line_to_char(line + 1)
        } else {
            len
        };
        if start == end {
            continue; // phantom trailing empty line
        }
        let content_end = line_ending::line_end_char_index(&slice, line);
        let key: String = slice.slice(start..content_end).chunks().collect();
        if !seen.insert(key) {
            // Duplicate of an earlier line — delete it entirely.
            changes.push((start, end, None));
        }
    }

    if changes.is_empty() {
        return Ok(());
    }

    let transaction = Transaction::change(doc.text(), changes.into_iter());
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

fn transpose_words(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    use zemacs_core::chars::char_is_word;

    if event != PromptEvent::Validate {
        return Ok(());
    }

    let (view, doc) = current!(cx.editor);
    let slice = doc.text().slice(..);
    let len = slice.len_chars();
    let cursor = doc.selection(view.id).primary().cursor(slice).min(len);

    // End of the word at/before the cursor.
    let mut w1_end = cursor;
    while w1_end < len && char_is_word(slice.char(w1_end)) {
        w1_end += 1;
    }
    if w1_end == cursor {
        // Cursor was not inside a word; back up to the previous word's end.
        while w1_end > 0 && !char_is_word(slice.char(w1_end - 1)) {
            w1_end -= 1;
        }
    }
    let mut w1_start = w1_end;
    while w1_start > 0 && char_is_word(slice.char(w1_start - 1)) {
        w1_start -= 1;
    }
    if w1_start == w1_end {
        return Ok(()); // no word before
    }

    // The next word after the first.
    let mut w2_start = w1_end;
    while w2_start < len && !char_is_word(slice.char(w2_start)) {
        w2_start += 1;
    }
    if w2_start >= len {
        return Ok(()); // no word after
    }
    let mut w2_end = w2_start;
    while w2_end < len && char_is_word(slice.char(w2_end)) {
        w2_end += 1;
    }

    let word1: Tendril = slice.slice(w1_start..w1_end).chunks().collect();
    let sep: Tendril = slice.slice(w1_end..w2_start).chunks().collect();
    let word2: Tendril = slice.slice(w2_start..w2_end).chunks().collect();
    let swapped: Tendril = format!("{word2}{sep}{word1}").into();

    let transaction =
        Transaction::change(doc.text(), std::iter::once((w1_start, w2_end, Some(swapped))));
    doc.apply(&transaction, view.id);
    // Point lands after the transposed pair (emacs behaviour). Region length is
    // unchanged, so w2_end is still the end of the swapped span.
    doc.set_selection(view.id, Selection::point(w2_end.min(doc.text().len_chars())));
    doc.append_changes_to_history(view);
    Ok(())
}

fn duplicate_line(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    let (view, doc) = current!(cx.editor);
    let line_ending = doc.line_ending.as_str();
    let slice = doc.text().slice(..);

    let cursor = doc.selection(view.id).primary().cursor(slice);
    let line = slice.char_to_line(cursor);
    let from = slice.line_to_char(line);
    let to = if line + 1 < slice.len_lines() {
        slice.line_to_char(line + 1)
    } else {
        slice.len_chars()
    };

    let line_text: Tendril = slice.slice(from..to).chunks().collect();
    // Insert a copy as a whole new line below. If the current line has no
    // trailing newline (last line), prepend one so the copy lands on its own line.
    let dup: Tendril = if line_text.ends_with('\n') {
        line_text
    } else {
        format!("{line_ending}{line_text}").into()
    };

    let transaction = Transaction::change(doc.text(), std::iter::once((to, to, Some(dup))));
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

fn delete_trailing_whitespace(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    let (view, doc) = current!(cx.editor);
    let slice = doc.text().slice(..);

    // For each line, delete the run of spaces/tabs before the line ending.
    let mut changes = Vec::new();
    for line in 0..slice.len_lines() {
        let start = slice.line_to_char(line);
        let end = line_ending::line_end_char_index(&slice, line);
        let mut ws = end;
        while ws > start && matches!(slice.char(ws - 1), ' ' | '\t') {
            ws -= 1;
        }
        if ws < end {
            changes.push((ws, end, None));
        }
    }

    if changes.is_empty() {
        return Ok(());
    }

    let transaction = Transaction::change(doc.text(), changes.into_iter());
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

fn sort(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    let scrolloff = cx.editor.config().scrolloff;
    let (view, doc) = current!(cx.editor);
    let text = doc.text().slice(..);

    let selection = doc.selection(view.id);

    if selection.len() == 1 {
        bail!("Sorting requires multiple selections. Hint: split selection first");
    }

    let mut fragments: Vec<_> = selection
        .slices(text)
        .map(|fragment| fragment.chunks().collect())
        .collect();

    fragments.sort_by(
        match (args.has_flag("insensitive"), args.has_flag("reverse")) {
            (true, true) => |a: &Tendril, b: &Tendril| b.to_lowercase().cmp(&a.to_lowercase()),
            (true, false) => |a: &Tendril, b: &Tendril| a.to_lowercase().cmp(&b.to_lowercase()),
            (false, true) => |a: &Tendril, b: &Tendril| b.cmp(a),
            (false, false) => |a: &Tendril, b: &Tendril| a.cmp(b),
        },
    );

    let transaction = Transaction::change(
        doc.text(),
        selection
            .into_iter()
            .zip(fragments)
            .map(|(s, fragment)| (s.from(), s.to(), Some(fragment))),
    );

    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    view.ensure_cursor_in_view(doc, scrolloff);

    Ok(())
}

fn reflow(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    let scrolloff = cx.editor.config().scrolloff;
    let (view, doc) = current!(cx.editor);

    // Find the text_width by checking the following sources in order:
    //   - The passed argument in `args`
    //   - The configured text-width for this language in languages.toml
    //   - The configured text-width in the config.toml
    let text_width: usize = args
        .first()
        .map(|num| num.parse::<usize>())
        .transpose()?
        .unwrap_or_else(|| doc.text_width());

    let rope = doc.text();

    let selection = doc.selection(view.id);
    let transaction = Transaction::change_by_selection(rope, selection, |range| {
        let fragment = range.fragment(rope.slice(..));
        let reflowed_text = zemacs_core::wrap::reflow_hard_wrap(&fragment, text_width);

        (range.from(), range.to(), Some(reflowed_text))
    });

    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    view.ensure_cursor_in_view(doc, scrolloff);

    Ok(())
}

fn tree_sitter_subtree(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    let (view, doc) = current!(cx.editor);

    if let Some(syntax) = doc.syntax() {
        let primary_selection = doc.selection(view.id).primary();
        let text = doc.text();
        let from = text.char_to_byte(primary_selection.from()) as u32;
        let to = text.char_to_byte(primary_selection.to()) as u32;
        if let Some(selected_node) = syntax.descendant_for_byte_range(from, to) {
            let mut contents = String::from("```tsq\n");
            zemacs_core::syntax::pretty_print_tree(&mut contents, selected_node)?;
            contents.push_str("\n```");

            let callback = async move {
                let call: job::Callback = Callback::EditorCompositor(Box::new(
                    move |editor: &mut Editor, compositor: &mut Compositor| {
                        let contents = ui::Markdown::new(contents, editor.syn_loader.clone());
                        let popup = Popup::new("hover", contents).auto_close(true);
                        compositor.replace_or_push("hover", popup);
                    },
                ));
                Ok(call)
            };

            cx.jobs.callback(callback);
        }
    }

    Ok(())
}

fn open_config(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    cx.editor
        .open(&zemacs_loader::config_file(), Action::Replace)?;
    Ok(())
}

fn open_workspace_config(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    cx.editor
        .open(&zemacs_loader::workspace_config_file(), Action::Replace)?;
    Ok(())
}

fn open_log(cx: &mut compositor::Context, _args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    cx.editor.open(&zemacs_loader::log_file(), Action::Replace)?;
    Ok(())
}

fn refresh_config(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    cx.editor.config_events.0.send(ConfigEvent::Refresh)?;
    Ok(())
}

fn append_output(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    shell(cx, &args.join(" "), &ShellBehavior::Append);
    Ok(())
}

fn insert_output(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    shell(cx, &args.join(" "), &ShellBehavior::Insert);
    Ok(())
}

fn pipe_to(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    pipe_impl(cx, args, event, &ShellBehavior::Ignore)
}

fn pipe(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    pipe_impl(cx, args, event, &ShellBehavior::Replace)
}

fn pipe_impl(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
    behavior: &ShellBehavior,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    shell(cx, &args.join(" "), behavior);
    Ok(())
}

fn run_shell_command(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    let shell = cx.editor.config().shell.clone();
    let args = args.join(" ");

    let callback = async move {
        let output = shell_impl_async(&shell, &args, None).await?;
        let call: job::Callback = Callback::EditorCompositor(Box::new(
            move |editor: &mut Editor, compositor: &mut Compositor| {
                if !output.trim().is_empty() {
                    let contents = ui::Markdown::new(
                        format!("```sh\n{}\n```", output.trim_end()),
                        editor.syn_loader.clone(),
                    );
                    let popup = Popup::new("shell", contents).position(Some(
                        zemacs_core::Position::new(editor.cursor().0.unwrap_or_default().row, 2),
                    ));
                    compositor.replace_or_push("shell", popup);
                }
                editor.set_status("Command run");
            },
        ));
        Ok(call)
    };
    cx.jobs.callback(callback);

    Ok(())
}

fn reset_diff_change(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    let editor = &mut cx.editor;
    let scrolloff = editor.config().scrolloff;

    let (view, doc) = current!(editor);
    let Some(handle) = doc.diff_handle() else {
        bail!("Diff is not available in the current buffer")
    };

    let diff = handle.load();
    let doc_text = doc.text().slice(..);
    let diff_base = diff.diff_base();
    let mut changes = 0;

    let transaction = Transaction::change(
        doc.text(),
        diff.hunks_intersecting_line_ranges(doc.selection(view.id).line_ranges(doc_text))
            .map(|hunk| {
                changes += 1;
                let start = diff_base.line_to_char(hunk.before.start as usize);
                let end = diff_base.line_to_char(hunk.before.end as usize);
                let text: Tendril = diff_base.slice(start..end).chunks().collect();
                (
                    doc_text.line_to_char(hunk.after.start as usize),
                    doc_text.line_to_char(hunk.after.end as usize),
                    (!text.is_empty()).then_some(text),
                )
            }),
    );
    if changes == 0 {
        bail!("There are no changes under any selection");
    }

    drop(diff); // make borrow check happy
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    view.ensure_cursor_in_view(doc, scrolloff);
    cx.editor.set_status(format!(
        "Reset {changes} change{}",
        if changes == 1 { "" } else { "s" }
    ));
    Ok(())
}

fn clear_register(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    if args.is_empty() {
        cx.editor.registers.clear();
        cx.editor.set_status("All registers cleared");
        return Ok(());
    }

    ensure!(
        args[0].chars().count() == 1,
        format!("Invalid register {}", &args[0])
    );
    let register = args[0].chars().next().unwrap_or_default();
    if cx.editor.registers.remove(register) {
        cx.editor
            .set_status(format!("Register {} cleared", register));
    } else {
        cx.editor
            .set_error(format!("Register {} not found", register));
    }
    Ok(())
}

fn set_register(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    ensure!(
        args[0].chars().count() == 1,
        format!("Invalid register {}", &args[0])
    );

    let register = args[0].chars().next().unwrap_or_default();
    cx.editor.registers.write(register, vec![args[1].into()])
}

fn redraw(cx: &mut compositor::Context, _args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    let callback = Box::pin(async move {
        let call: job::Callback =
            job::Callback::EditorCompositor(Box::new(|_editor, compositor| {
                compositor.need_full_redraw();
            }));

        Ok(call)
    });

    cx.jobs.callback(callback);

    Ok(())
}

#[derive(Debug, Clone, Copy)]
pub struct MoveBufferOptions {
    pub force: bool,
}

fn move_buffer(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    let new_path: PathBuf = args.first().unwrap().into();
    move_buffer_impl(cx, new_path, MoveBufferOptions { force: false })
}

fn force_move_buffer(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    let new_path: PathBuf = args.first().unwrap().into();
    move_buffer_impl(cx, new_path, MoveBufferOptions { force: true })
}

fn move_buffer_impl(
    cx: &mut compositor::Context,
    new_path: PathBuf,
    options: MoveBufferOptions,
) -> anyhow::Result<()> {
    let doc = doc!(cx.editor);
    let old_path = doc
        .path()
        .map(ToOwned::to_owned)
        .context("Scratch buffer cannot be moved. Use :write instead")?;

    // if new_path is a directory, append the original file name
    // to move the file into that directory.
    let new_path = old_path
        .file_name()
        .filter(|_| new_path.is_dir())
        .map(|old_file_name| new_path.join(old_file_name))
        .unwrap_or(new_path);

    if old_path.exists() {
        if let Some(parent) = new_path.parent() {
            if !parent.exists() {
                if options.force {
                    std::fs::DirBuilder::new().recursive(true).create(parent)?;
                } else {
                    bail!(
                        "can't move file, parent directory does not exist (use :mv! to create it)"
                    )
                }
            }
        }
    }

    if let Err(err) = cx.editor.move_path(&old_path, new_path.as_ref()) {
        bail!("Could not move file: {err}");
    }
    Ok(())
}

fn yank_diagnostic(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    let reg = match args.first() {
        Some(s) => {
            ensure!(s.chars().count() == 1, format!("Invalid register {s}"));
            s.chars().next().unwrap()
        }
        None => '+',
    };

    let (view, doc) = current_ref!(cx.editor);
    let primary = doc.selection(view.id).primary();

    // Look only for diagnostics that intersect with the primary selection
    let diag: Vec<_> = doc
        .diagnostics()
        .iter()
        .filter(|d| primary.overlaps(&zemacs_core::Range::new(d.range.start, d.range.end)))
        .map(|d| d.message.clone())
        .collect();
    let n = diag.len();
    if n == 0 {
        bail!("No diagnostics under primary selection");
    }

    cx.editor.registers.write(reg, diag)?;
    cx.editor.set_status(format!(
        "Yanked {n} diagnostic{} to register {reg}",
        if n == 1 { "" } else { "s" }
    ));
    Ok(())
}

fn read(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    let scrolloff = cx.editor.config().scrolloff;
    let (view, doc) = current!(cx.editor);

    let filename = args.first().unwrap();
    let path = zemacs_stdx::path::expand_tilde(PathBuf::from(filename.to_string()));

    ensure!(
        path.exists() && path.is_file(),
        "path is not a file: {:?}",
        path
    );

    let file = std::fs::File::open(path).map_err(|err| anyhow!("error opening file: {}", err))?;
    let mut reader = BufReader::new(file);
    let (contents, _, _) = read_to_string(&mut reader, Some(doc.encoding()))
        .map_err(|err| anyhow!("error reading file: {}", err))?;
    let contents = Tendril::from(contents);
    let selection = doc.selection(view.id);
    let transaction = Transaction::insert(doc.text(), selection, contents);
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    view.ensure_cursor_in_view(doc, scrolloff);

    Ok(())
}

fn echo(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    let output = args.into_iter().fold(String::new(), |mut acc, arg| {
        if !acc.is_empty() {
            acc.push(' ');
        }
        acc.push_str(&arg);
        acc
    });
    cx.editor.set_status(output);

    Ok(())
}

fn noop(_cx: &mut compositor::Context, _args: Args, _event: PromptEvent) -> anyhow::Result<()> {
    Ok(())
}

/// This command accepts a single boolean --skip-visible flag and no positionals.
const BUFFER_CLOSE_OTHERS_SIGNATURE: Signature = Signature {
    positionals: (0, Some(0)),
    flags: &[Flag {
        name: "skip-visible",
        alias: Some('s'),
        doc: "don't close buffers that are visible",
        ..Flag::DEFAULT
    }],
    ..Signature::DEFAULT
};

// TODO: SHELL_SIGNATURE should specify var args for arguments, so that just completers::filename can be used,
// but Signature does not yet allow for var args.

/// This command handles all of its input as-is with no quoting or flags.
pub const SHELL_SIGNATURE: Signature = Signature {
    positionals: (1, Some(2)),
    raw_after: Some(1),
    ..Signature::DEFAULT
};

pub const SHELL_COMPLETER: CommandCompleter = CommandCompleter::positional(&[
    // Command name
    completers::program,
    // Shell argument(s)
    completers::repeating_filenames,
]);

const WRITE_NO_FORMAT_FLAG: Flag = Flag {
    name: "no-format",
    doc: "skip auto-formatting",
    ..Flag::DEFAULT
};

const WRITE_NO_CODE_ACTIONS_FLAG: Flag = Flag {
    name: "no-code-actions",
    doc: "skip code actions on save",
    ..Flag::DEFAULT
};

pub const TYPABLE_COMMAND_LIST: &[TypableCommand] = &[
    TypableCommand {
        name: "exit",
        aliases: &["x", "xit"],
        doc: "Write changes to disk if the buffer is modified and then quit. Accepts an optional path (:exit some/path.txt).",
        fun: exit,
        completer: CommandCompleter::positional(&[completers::filename]),
        signature: Signature {
            positionals: (0, Some(1)),
            flags: &[WRITE_NO_FORMAT_FLAG, WRITE_NO_CODE_ACTIONS_FLAG],
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "exit!",
        aliases: &["x!", "xit!"],
        doc: "Force write changes to disk, creating necessary subdirectories, if the buffer is modified and then quit. Accepts an optional path (:exit! some/path.txt).",
        fun: force_exit,
        completer: CommandCompleter::positional(&[completers::filename]),
        signature: Signature {
            positionals: (0, Some(1)),
            flags: &[WRITE_NO_FORMAT_FLAG, WRITE_NO_CODE_ACTIONS_FLAG],
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "quit",
        aliases: &["q"],
        doc: "Close the current view.",
        fun: quit,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "quit!",
        aliases: &["q!"],
        doc: "Force close the current view, ignoring unsaved changes.",
        fun: force_quit,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "open",
        aliases: &["o", "edit", "e"],
        doc: "Open a file from disk into the current view.",
        fun: open,
        completer: CommandCompleter::all(completers::filename),
        signature: Signature {
            positionals: (1, None),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "buffer-close",
        aliases: &["bc", "bclose"],
        doc: "Close the current buffer.",
        fun: buffer_close,
        completer: CommandCompleter::all(completers::buffer),
        signature: Signature {
            positionals: (0, None),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "buffer-close!",
        aliases: &["bc!", "bclose!"],
        doc: "Close the current buffer forcefully, ignoring unsaved changes.",
        fun: force_buffer_close,
        completer: CommandCompleter::all(completers::buffer),
        signature: Signature {
            positionals: (0, None),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "buffer-close-others",
        aliases: &["bco", "bcloseother"],
        doc: "Close all buffers but the currently focused one.",
        fun: buffer_close_others,
        completer: CommandCompleter::none(),
        signature: BUFFER_CLOSE_OTHERS_SIGNATURE,
    },
    TypableCommand {
        name: "buffer-close-others!",
        aliases: &["bco!", "bcloseother!"],
        doc: "Force close all buffers but the currently focused one.",
        fun: force_buffer_close_others,
        completer: CommandCompleter::none(),
        signature: BUFFER_CLOSE_OTHERS_SIGNATURE,
    },
    TypableCommand {
        name: "buffer-close-all",
        aliases: &["bca", "bcloseall"],
        doc: "Close all buffers without quitting.",
        fun: buffer_close_all,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "buffer-close-all!",
        aliases: &["bca!", "bcloseall!"],
        doc: "Force close all buffers ignoring unsaved changes without quitting.",
        fun: force_buffer_close_all,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "buffer-next",
        aliases: &["bn", "bnext"],
        doc: "Goto next buffer.",
        fun: buffer_next,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "buffer-previous",
        aliases: &["bp", "bprev"],
        doc: "Goto previous buffer.",
        fun: buffer_previous,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "write",
        aliases: &["w"],
        doc: "Write changes to disk. Accepts an optional path (:write some/path.txt)",
        fun: write,
        completer: CommandCompleter::positional(&[completers::filename]),
        signature: Signature {
            positionals: (0, Some(1)),
            flags: &[WRITE_NO_FORMAT_FLAG, WRITE_NO_CODE_ACTIONS_FLAG],
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "write!",
        aliases: &["w!"],
        doc: "Force write changes to disk creating necessary subdirectories. Accepts an optional path (:write! some/path.txt)",
        fun: force_write,
        completer: CommandCompleter::positional(&[completers::filename]),
        signature: Signature {
            positionals: (0, Some(1)),
            flags: &[WRITE_NO_FORMAT_FLAG,WRITE_NO_CODE_ACTIONS_FLAG],
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "write-buffer-close",
        aliases: &["wbc"],
        doc: "Write changes to disk and closes the buffer. Accepts an optional path (:write-buffer-close some/path.txt)",
        fun: write_buffer_close,
        completer: CommandCompleter::positional(&[completers::filename]),
        signature: Signature {
            positionals: (0, Some(1)),
            flags: &[WRITE_NO_FORMAT_FLAG,WRITE_NO_CODE_ACTIONS_FLAG],
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "write-buffer-close!",
        aliases: &["wbc!"],
        doc: "Force write changes to disk creating necessary subdirectories and closes the buffer. Accepts an optional path (:write-buffer-close! some/path.txt)",
        fun: force_write_buffer_close,
        completer: CommandCompleter::positional(&[completers::filename]),
        signature: Signature {
            positionals: (0, Some(1)),
            flags: &[WRITE_NO_FORMAT_FLAG,WRITE_NO_CODE_ACTIONS_FLAG],
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "new",
        aliases: &["n"],
        doc: "Create a new scratch buffer.",
        fun: new_file,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "format",
        aliases: &["fmt"],
        doc: "Format the file using an external formatter or language server.",
        fun: format,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "indent-style",
        aliases: &[],
        doc: "Set the indentation style for editing. ('t' for tabs or 1-16 for number of spaces.)",
        fun: set_indent_style,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(1)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "line-ending",
        aliases: &[],
        #[cfg(not(feature = "unicode-lines"))]
        doc: "Set the document's default line ending. Options: crlf, lf.",
        #[cfg(feature = "unicode-lines")]
        doc: "Set the document's default line ending. Options: crlf, lf, cr, ff, nel.",
        fun: set_line_ending,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(1)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "earlier",
        aliases: &["ear"],
        doc: "Jump back to an earlier point in edit history. Accepts a number of steps or a time span.",
        fun: earlier,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(1)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "later",
        aliases: &["lat"],
        doc: "Jump to a later point in edit history. Accepts a number of steps or a time span.",
        fun: later,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(1)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "write-quit",
        aliases: &["wq"],
        doc: "Write changes to disk and close the current view. Accepts an optional path (:wq some/path.txt)",
        fun: write_quit,
        completer: CommandCompleter::positional(&[completers::filename]),
        signature: Signature {
            positionals: (0, Some(1)),
            flags: &[WRITE_NO_FORMAT_FLAG, WRITE_NO_CODE_ACTIONS_FLAG],
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "write-quit!",
        aliases: &["wq!"],
        doc: "Write changes to disk and close the current view forcefully. Accepts an optional path (:wq! some/path.txt)",
        fun: force_write_quit,
        completer: CommandCompleter::positional(&[completers::filename]),
        signature: Signature {
            positionals: (0, Some(1)),
            flags: &[WRITE_NO_FORMAT_FLAG, WRITE_NO_CODE_ACTIONS_FLAG],
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "write-all",
        aliases: &["wa"],
        doc: "Write changes from all buffers to disk.",
        fun: write_all,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            flags: &[WRITE_NO_FORMAT_FLAG, WRITE_NO_CODE_ACTIONS_FLAG],
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "write-all!",
        aliases: &["wa!"],
        doc: "Forcefully write changes from all buffers to disk creating necessary subdirectories.",
        fun: force_write_all,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            flags: &[WRITE_NO_FORMAT_FLAG, WRITE_NO_CODE_ACTIONS_FLAG],
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "write-quit-all",
        aliases: &["wqa", "xa"],
        doc: "Write changes from all buffers to disk and close all views.",
        fun: write_all_quit,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            flags: &[WRITE_NO_FORMAT_FLAG, WRITE_NO_CODE_ACTIONS_FLAG],
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "write-quit-all!",
        aliases: &["wqa!", "xa!"],
        doc: "Forcefully write changes from all buffers to disk, creating necessary subdirectories, and close all views (ignoring unsaved changes).",
        fun: force_write_all_quit,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            flags: &[WRITE_NO_FORMAT_FLAG, WRITE_NO_CODE_ACTIONS_FLAG],
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "quit-all",
        aliases: &["qa"],
        doc: "Close all views.",
        fun: quit_all,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "quit-all!",
        aliases: &["qa!"],
        doc: "Force close all views ignoring unsaved changes.",
        fun: force_quit_all,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "cquit",
        aliases: &["cq"],
        doc: "Quit with exit code (default 1). Accepts an optional integer exit code (:cq 2).",
        fun: cquit,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(1)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "cquit!",
        aliases: &["cq!"],
        doc: "Force quit with exit code (default 1) ignoring unsaved changes. Accepts an optional integer exit code (:cq! 2).",
        fun: force_cquit,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(1)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "theme",
        aliases: &[],
        doc: "Change the editor theme (show current theme if no name specified).",
        fun: theme,
        completer: CommandCompleter::positional(&[completers::theme]),
        signature: Signature {
            positionals: (0, Some(1)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "hunk-reset",
        aliases: &["reset-hunk", "hunk-undo"],
        doc: "Undo the git hunk under the cursor, restoring it from HEAD (gitsigns reset_hunk).",
        fun: hunk_reset,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "hunk-next",
        aliases: &["next-hunk"],
        doc: "Move the cursor to the next git hunk.",
        fun: hunk_next,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "hunk-prev",
        aliases: &["prev-hunk"],
        doc: "Move the cursor to the previous git hunk.",
        fun: hunk_prev,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "conflict-ours",
        aliases: &["diffget-ours", "conflict-keep-ours"],
        doc: "Resolve the merge conflict at the cursor by keeping OUR side (HEAD).",
        fun: conflict_ours,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "conflict-theirs",
        aliases: &["diffget-theirs", "conflict-keep-theirs"],
        doc: "Resolve the merge conflict at the cursor by keeping THEIR side (incoming).",
        fun: conflict_theirs,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "conflict-both",
        aliases: &["conflict-keep-both"],
        doc: "Resolve the merge conflict at the cursor by keeping BOTH sides.",
        fun: conflict_both,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "conflict-next",
        aliases: &[],
        doc: "Jump to the next merge-conflict marker.",
        fun: conflict_next,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "conflict-prev",
        aliases: &[],
        doc: "Jump to the previous merge-conflict marker.",
        fun: conflict_prev,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "theme-toggle",
        aliases: &["toggle-theme", "light-dark"],
        doc: "Toggle between a dark and light theme (`:theme-toggle [dark] [light]`).",
        fun: theme_toggle,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(2)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "theme-next",
        aliases: &[],
        doc: "Switch to the next theme (alphabetical).",
        fun: theme_next,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "theme-prev",
        aliases: &[],
        doc: "Switch to the previous theme (alphabetical).",
        fun: theme_prev,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "run",
        aliases: &["r!"],
        doc: "Run a command in the IDE Run tool window (defaults to `cargo run`).",
        fun: run_command,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, None),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "registers",
        aliases: &["reg", "display"],
        doc: "Show the contents of all registers.",
        fun: registers,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "yank-join",
        aliases: &[],
        doc: "Yank joined selections. A separator can be provided as first argument. Default value is newline.",
        fun: yank_joined,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(1)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "clipboard-yank",
        aliases: &[],
        doc: "Yank main selection into system clipboard.",
        fun: yank_main_selection_to_clipboard,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "clipboard-yank-join",
        aliases: &[],
        doc: "Yank joined selections into system clipboard. A separator can be provided as first argument. Default value is newline.", // FIXME: current UI can't display long doc.
        fun: yank_joined_to_clipboard,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(1)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "primary-clipboard-yank",
        aliases: &[],
        doc: "Yank main selection into system primary clipboard.",
        fun: yank_main_selection_to_primary_clipboard,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "primary-clipboard-yank-join",
        aliases: &[],
        doc: "Yank joined selections into system primary clipboard. A separator can be provided as first argument. Default value is newline.", // FIXME: current UI can't display long doc.
        fun: yank_joined_to_primary_clipboard,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(1)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "clipboard-paste-after",
        aliases: &[],
        doc: "Paste system clipboard after selections.",
        fun: paste_clipboard_after,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "clipboard-paste-before",
        aliases: &[],
        doc: "Paste system clipboard before selections.",
        fun: paste_clipboard_before,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "clipboard-paste-replace",
        aliases: &[],
        doc: "Replace selections with content of system clipboard.",
        fun: replace_selections_with_clipboard,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "primary-clipboard-paste-after",
        aliases: &[],
        doc: "Paste primary clipboard after selections.",
        fun: paste_primary_clipboard_after,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "primary-clipboard-paste-before",
        aliases: &[],
        doc: "Paste primary clipboard before selections.",
        fun: paste_primary_clipboard_before,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "primary-clipboard-paste-replace",
        aliases: &[],
        doc: "Replace selections with content of system primary clipboard.",
        fun: replace_selections_with_primary_clipboard,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "show-clipboard-provider",
        aliases: &[],
        doc: "Show clipboard provider name in status bar.",
        fun: show_clipboard_provider,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "change-current-directory",
        aliases: &["cd"],
        doc: "Change the current working directory.",
        fun: change_current_directory,
        completer: CommandCompleter::positional(&[completers::directory]),
        signature: Signature {
            positionals: (1, Some(1)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "show-directory-stack",
        aliases: &[],
        doc: "Show the directory stack as a <space> delimited string.",
        fun: show_directory_stack,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "push-directory",
        aliases: &["pushd"],
        doc: "Save and then change the current directory.",
        fun: push_directory,
        completer: CommandCompleter::positional(&[completers::directory]),
        signature: Signature {
            positionals: (1, Some(1)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "pop-directory",
        aliases: &["popd"],
        doc: "Remove the top entry from the directory stack, and cd to the new top directory..",
        fun: pop_directory,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "show-directory",
        aliases: &["pwd"],
        doc: "Show the current working directory.",
        fun: show_current_directory,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "encoding",
        aliases: &[],
        doc: "Set encoding. Based on `https://encoding.spec.whatwg.org`.",
        fun: set_encoding,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(1)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "character-info",
        aliases: &["char"],
        doc: "Get info about the character under the primary cursor.",
        fun: get_character_info,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "reload",
        aliases: &["rl"],
        doc: "Discard changes and reload from the source file.",
        fun: reload,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "reload-all",
        aliases: &["rla"],
        doc: "Discard changes and reload all documents from the source files.",
        fun: reload_all,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "update",
        aliases: &["u"],
        doc: "Write changes only if the file has been modified.",
        fun: update,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            flags: &[WRITE_NO_FORMAT_FLAG],
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "lsp-workspace-command",
        aliases: &[],
        doc: "Open workspace command picker",
        fun: lsp_workspace_command,
        completer: CommandCompleter::positional(&[completers::lsp_workspace_command]),
        signature: Signature {
            positionals: (0, None),
            raw_after: Some(1),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "lsp-restart",
        aliases: &[],
        doc: "Restarts the given language servers, or all language servers that are used by the current file if no arguments are supplied",
        fun: lsp_restart,
        completer: CommandCompleter::all(completers::configured_language_servers),
        signature: Signature {
            positionals: (0, None),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "set",
        aliases: &["se"],
        doc: "Set options with vim syntax (:set nu, :set nowrap, :set tw=80, :set cursorline) or native :set key value.",
        fun: vim_set,
        completer: CommandCompleter::positional(&[completers::setting]),
        signature: Signature {
            positionals: (1, None),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "lsp-stop",
        aliases: &[],
        doc: "Stops the given language servers, or all language servers that are used by the current file if no arguments are supplied",
        fun: lsp_stop,
        completer: CommandCompleter::all(completers::active_language_servers),
        signature: Signature {
            positionals: (0, None),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "tree-sitter-scopes",
        aliases: &[],
        doc: "Display tree sitter scopes, primarily for theming and development.",
        fun: tree_sitter_scopes,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "tree-sitter-highlight-name",
        aliases: &[],
        doc: "Display name of tree-sitter highlight scope under the cursor.",
        fun: tree_sitter_highlight_name,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "tree-sitter-layers",
        aliases: &[],
        doc: "Display language names of tree-sitter injection layers under the cursor.",
        fun: tree_sitter_layers,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "debug-start",
        aliases: &["dbg"],
        doc: "Start a debug session from a given template with given parameters.",
        fun: debug_start,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, None),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "debug-remote",
        aliases: &["dbg-tcp"],
        doc: "Connect to a debug adapter by TCP address and start a debugging session from a given template with given parameters.",
        fun: debug_remote,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, None),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "debug-eval",
        aliases: &[],
        doc: "Evaluate expression in current debug context.",
        fun: debug_eval,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (1, Some(1)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "vsplit",
        aliases: &["vs"],
        doc: "Open the file in a vertical split.",
        fun: vsplit,
        completer: CommandCompleter::all(completers::filename),
        signature: Signature {
            positionals: (0, None),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "vsplit-new",
        aliases: &["vnew"],
        doc: "Open a scratch buffer in a vertical split.",
        fun: vsplit_new,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "hsplit",
        aliases: &["hs", "sp"],
        doc: "Open the file in a horizontal split.",
        fun: hsplit,
        completer: CommandCompleter::all(completers::filename),
        signature: Signature {
            positionals: (0, None),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "hsplit-new",
        aliases: &["hnew"],
        doc: "Open a scratch buffer in a horizontal split.",
        fun: hsplit_new,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "tutor",
        aliases: &[],
        doc: "Open the tutorial.",
        fun: tutor,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "goto",
        aliases: &["g"],
        doc: "Goto line number.",
        fun: goto_line_number,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (1, Some(1)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "set-language",
        aliases: &["lang"],
        doc: "Set the language of current buffer (show current language if no value specified).",
        fun: language,
        completer: CommandCompleter::positional(&[completers::language]),
        signature: Signature {
            positionals: (0, Some(1)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "set-option",
        aliases: &[],
        doc: "Set a config option at runtime.\nFor example to disable smart case search, use `:set-option search.smart-case false`.",
        fun: set_option,
        // TODO: Add support for completion of the options value(s), when appropriate.
        completer: CommandCompleter::positional(&[completers::setting]),
        signature: Signature {
            positionals: (2, Some(2)),
            raw_after: Some(1),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "toggle-option",
        aliases: &["toggle"],
        doc: "Toggle a config option at runtime.\nFor example to toggle smart case search, use `:toggle search.smart-case`.",
        fun: toggle_option,
        completer: CommandCompleter::positional(&[completers::setting]),
        signature: Signature {
            positionals: (1, None),
            raw_after: Some(1),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "get-option",
        aliases: &["get"],
        doc: "Get the current value of a config option.",
        fun: get_option,
        completer: CommandCompleter::positional(&[completers::setting]),
        signature: Signature {
            positionals: (1, Some(1)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "move-line-down",
        aliases: &[],
        doc: "Move the current line down by one (drag down).",
        fun: move_line_down,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "move-line-up",
        aliases: &[],
        doc: "Move the current line up by one (drag up).",
        fun: move_line_up,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "cycle-case",
        aliases: &[],
        doc: "Cycle the case style of the symbol under the cursor.",
        fun: cycle_case,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "change-case",
        aliases: &[],
        doc: "Change the symbol under the cursor to camel|snake|kebab|pascal case.",
        fun: change_case,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (1, Some(1)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "left",
        aliases: &["le"],
        doc: "Left-align line(s), setting leading indent to {n} (default 0) — vim :left.",
        fun: left_cmd,
        completer: CommandCompleter::none(),
        signature: Signature { positionals: (0, Some(1)), ..Signature::DEFAULT },
    },
    TypableCommand {
        name: "right",
        aliases: &["ri"],
        doc: "Right-align line(s) to width {n} (default 80) — vim :right.",
        fun: right_cmd,
        completer: CommandCompleter::none(),
        signature: Signature { positionals: (0, Some(1)), ..Signature::DEFAULT },
    },
    TypableCommand {
        name: "center",
        aliases: &["ce"],
        doc: "Center line(s) within width {n} (default 80) — vim :center.",
        fun: center_cmd,
        completer: CommandCompleter::none(),
        signature: Signature { positionals: (0, Some(1)), ..Signature::DEFAULT },
    },
    TypableCommand {
        name: "undo",
        aliases: &[],
        doc: "Undo the last change (vim :undo).",
        fun: undo_cmd,
        completer: CommandCompleter::none(),
        signature: Signature { positionals: (0, Some(0)), ..Signature::DEFAULT },
    },
    TypableCommand {
        name: "redo",
        aliases: &["red"],
        doc: "Redo the last undone change (vim :redo).",
        fun: redo_cmd,
        completer: CommandCompleter::none(),
        signature: Signature { positionals: (0, Some(0)), ..Signature::DEFAULT },
    },
    TypableCommand {
        name: "retab",
        aliases: &[],
        doc: "Replace tabs with spaces (tab-width per buffer) — vim :retab.",
        fun: retab,
        completer: CommandCompleter::none(),
        signature: Signature { positionals: (0, Some(0)), ..Signature::DEFAULT },
    },
    TypableCommand {
        name: "join",
        aliases: &["j"],
        doc: "Join the current line(s) with the next, separated by a space (vim :j).",
        fun: join_lines_cmd,
        completer: CommandCompleter::none(),
        signature: Signature { positionals: (0, Some(0)), ..Signature::DEFAULT },
    },
    TypableCommand {
        name: "join!",
        aliases: &["j!"],
        doc: "Join the current line(s) with the next, no separating space (vim :j!).",
        fun: join_lines_nospace_cmd,
        completer: CommandCompleter::none(),
        signature: Signature { positionals: (0, Some(0)), ..Signature::DEFAULT },
    },
    TypableCommand {
        name: "put",
        aliases: &["pu"],
        doc: "Put (paste) a register's contents as new line(s) below the cursor (vim :put).",
        fun: put_lines,
        completer: CommandCompleter::none(),
        signature: Signature { positionals: (0, Some(1)), ..Signature::DEFAULT },
    },
    TypableCommand {
        name: "put!",
        aliases: &["pu!"],
        doc: "Put (paste) a register's contents as new line(s) above the cursor (vim :put!).",
        fun: put_lines_above,
        completer: CommandCompleter::none(),
        signature: Signature { positionals: (0, Some(1)), ..Signature::DEFAULT },
    },
    TypableCommand {
        name: "delete-lines",
        aliases: &["d", "del", "delete"],
        doc: "Delete the current line(s) into the unnamed register (vim :d).",
        fun: delete_lines_cmd,
        completer: CommandCompleter::none(),
        signature: Signature { positionals: (0, Some(0)), ..Signature::DEFAULT },
    },
    TypableCommand {
        name: "yank-lines",
        aliases: &["y", "ya", "yank"],
        doc: "Yank the current line(s) into the unnamed register (vim :y).",
        fun: yank_lines_cmd,
        completer: CommandCompleter::none(),
        signature: Signature { positionals: (0, Some(0)), ..Signature::DEFAULT },
    },
    TypableCommand {
        name: "indent-lines",
        aliases: &[],
        doc: "Indent the current line(s) by one shiftwidth (vim :>).",
        fun: indent_cmd,
        completer: CommandCompleter::none(),
        signature: Signature { positionals: (0, Some(0)), ..Signature::DEFAULT },
    },
    TypableCommand {
        name: "dedent-lines",
        aliases: &[],
        doc: "Dedent the current line(s) by one shiftwidth (vim :<).",
        fun: dedent_cmd,
        completer: CommandCompleter::none(),
        signature: Signature { positionals: (0, Some(0)), ..Signature::DEFAULT },
    },
    TypableCommand {
        name: "move-lines",
        aliases: &["m"],
        doc: "Move the current line to after line {address}: :m{addr} (e.g. :m0, :m$, :m.+2).",
        fun: move_lines,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (1, Some(1)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "copy-lines",
        aliases: &["t", "co", "copy"],
        doc: "Copy the current line to after line {address}: :t{addr} (e.g. :t0, :t$).",
        fun: copy_lines,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (1, Some(1)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "global",
        aliases: &[],
        doc: "Run a command on matching lines: :g/pattern/d (delete). Also :g!/pat/d.",
        fun: global,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (1, Some(1)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "vglobal",
        aliases: &[],
        doc: "Run a command on non-matching lines: :v/pattern/d (delete).",
        fun: vglobal,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (1, Some(1)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "substitute",
        aliases: &["s"],
        doc: "Substitute: :s/pattern/replacement/[flags]. Also :%s/.../.../g for the whole file.",
        fun: substitute,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (1, Some(1)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "split-line",
        aliases: &[],
        doc: "Split the current line at the cursor, keeping the cursor in place.",
        fun: split_line,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "just-one-space",
        aliases: &[],
        doc: "Collapse spaces and tabs around the cursor to a single space.",
        fun: just_one_space,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "delete-blank-lines",
        aliases: &[],
        doc: "Collapse consecutive blank lines down to a single blank line.",
        fun: delete_blank_lines,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "uniquify-lines",
        aliases: &["uniq"],
        doc: "Delete duplicate lines, keeping the first occurrence.",
        fun: uniquify_lines,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "transpose-words",
        aliases: &[],
        doc: "Transpose the word before the cursor with the word after it.",
        fun: transpose_words,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "transpose-chars",
        aliases: &[],
        doc: "Transpose the two characters around the cursor.",
        fun: transpose_chars,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "duplicate-line",
        aliases: &["dup"],
        doc: "Duplicate the current line below.",
        fun: duplicate_line,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "delete-trailing-whitespace",
        aliases: &["dtw"],
        doc: "Delete trailing whitespace from every line in the buffer.",
        fun: delete_trailing_whitespace,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "sort",
        aliases: &[],
        doc: "Sort ranges in selection.",
        fun: sort,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            flags: &[
                Flag {
                    name: "insensitive",
                    alias: Some('i'),
                    doc: "sort the ranges case-insensitively",
                    ..Flag::DEFAULT
                },
                Flag {
                    name: "reverse",
                    alias: Some('r'),
                    doc: "sort ranges in reverse order",
                    ..Flag::DEFAULT
                },
            ],
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "reflow",
        aliases: &[],
        doc: "Hard-wrap the current selection of lines to a given width.",
        fun: reflow,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(1)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "tree-sitter-subtree",
        aliases: &["ts-subtree"],
        doc: "Display the smallest tree-sitter subtree that spans the primary selection, primarily for debugging queries.",
        fun: tree_sitter_subtree,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "config-reload",
        aliases: &[],
        doc: "Refresh user config.",
        fun: refresh_config,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "config-open",
        aliases: &[],
        doc: "Open the user config.toml file.",
        fun: open_config,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "config-open-workspace",
        aliases: &[],
        doc: "Open the workspace config.toml file.",
        fun: open_workspace_config,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "log-open",
        aliases: &[],
        doc: "Open the zemacs log file.",
        fun: open_log,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "insert-output",
        aliases: &[],
        doc: "Run shell command, inserting output before each selection.",
        fun: insert_output,
        completer: SHELL_COMPLETER,
        signature: SHELL_SIGNATURE,
    },
    TypableCommand {
        name: "append-output",
        aliases: &[],
        doc: "Run shell command, appending output after each selection.",
        fun: append_output,
        completer: SHELL_COMPLETER,
        signature: SHELL_SIGNATURE,
    },
    TypableCommand {
        name: "pipe",
        aliases: &["|"],
        doc: "Pipe each selection to the shell command.",
        fun: pipe,
        completer: SHELL_COMPLETER,
        signature: SHELL_SIGNATURE,
    },
    TypableCommand {
        name: "pipe-to",
        aliases: &[],
        doc: "Pipe each selection to the shell command, ignoring output.",
        fun: pipe_to,
        completer: SHELL_COMPLETER,
        signature: SHELL_SIGNATURE,
    },
    TypableCommand {
        name: "run-shell-command",
        aliases: &["sh", "!"],
        doc: "Run a shell command",
        fun: run_shell_command,
        completer: SHELL_COMPLETER,
        signature: SHELL_SIGNATURE,
    },
    TypableCommand {
        name: "reset-diff-change",
        aliases: &["diffget", "diffg"],
        doc: "Reset the diff change at the cursor position.",
        fun: reset_diff_change,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "clear-register",
        aliases: &[],
        doc: "Clear given register. If no argument is provided, clear all registers.",
        fun: clear_register,
        completer: CommandCompleter::all(completers::register),
        signature: Signature {
            positionals: (0, Some(1)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "set-register",
        aliases: &[],
        doc: "Set contents of the given register.",
        fun: set_register,
        completer: CommandCompleter::positional(&[completers::register, completers::none]),
        signature: Signature {
            positionals: (2, Some(2)),
            raw_after: Some(1),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "redraw",
        aliases: &[],
        doc: "Clear and re-render the whole UI",
        fun: redraw,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "move",
        aliases: &["mv"],
        doc: "Move the current buffer and its corresponding file to a different path",
        fun: move_buffer,
        completer: CommandCompleter::positional(&[completers::filename]),
        signature: Signature {
            positionals: (1, Some(1)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "move!",
        aliases: &["mv!"],
        doc: "Move the current buffer and its corresponding file to a different path creating necessary subdirectories",
        fun: force_move_buffer,
        completer: CommandCompleter::positional(&[completers::filename]),
        signature: Signature {
            positionals: (1, Some(1)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "yank-diagnostic",
        aliases: &[],
        doc: "Yank diagnostic(s) under primary cursor to register, or clipboard by default",
        fun: yank_diagnostic,
        completer: CommandCompleter::all(completers::register),
        signature: Signature {
            positionals: (0, Some(1)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "read",
        aliases: &["r"],
        doc: "Load a file into buffer",
        fun: read,
        completer: CommandCompleter::positional(&[completers::filename]),
        signature: Signature {
            positionals: (1, Some(1)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "echo",
        aliases: &[],
        doc: "Prints the given arguments to the statusline.",
        fun: echo,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (1, None),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "noop",
        aliases: &[],
        doc: "Does nothing.",
        fun: noop,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, None),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "workspace-trust",
        aliases: &[],
        doc: "Allow language servers and local config for the current workspace.",
        fun: trust_workspace,
        completer: CommandCompleter::none(),
        signature: Signature { positionals: (0, None), ..Signature::DEFAULT },
    },
    TypableCommand {
        name: "workspace-untrust",
        aliases: &[],
        doc: "Revoke the current workspace's trust grant or exclusion.",
        fun: untrust_workspace,
        completer: CommandCompleter::none(),
        signature: Signature { positionals: (0, None), ..Signature::DEFAULT },
    },
    TypableCommand {
        name: "workspace-exclude",
        aliases: &[],
        doc: "Mark the current workspace as never-prompt. Never prompts for trust again.",
        fun: exclude_workspace,
        completer: CommandCompleter::none(),
        signature: Signature { positionals: (0, None), ..Signature::DEFAULT },
    }
];

pub static TYPABLE_COMMAND_MAP: Lazy<HashMap<&'static str, &'static TypableCommand>> =
    Lazy::new(|| {
        TYPABLE_COMMAND_LIST
            .iter()
            .flat_map(|cmd| {
                std::iter::once((cmd.name, cmd))
                    .chain(cmd.aliases.iter().map(move |&alias| (alias, cmd)))
            })
            .collect()
    });

fn execute_command_line(
    cx: &mut compositor::Context,
    input: &str,
    event: PromptEvent,
) -> anyhow::Result<()> {
    let (command, rest, _) = command_line::split(input);
    if command.is_empty() {
        return Ok(());
    }

    // If command is numeric, interpret as line number and go there.
    if command.parse::<usize>().is_ok() && rest.trim().is_empty() {
        let cmd = TYPABLE_COMMAND_MAP.get("goto").unwrap();
        return execute_command(cx, cmd, command, event);
    }

    // vim-style substitute: `:s/pat/rep/flags`, `:%s/pat/rep/g` (no space after
    // the command name, so it does not reach the command map).
    if let Some((whole, pattern, replacement, flags)) = parse_vim_substitute(input) {
        if event != PromptEvent::Validate {
            return Ok(());
        }
        return do_substitute(cx.editor, whole, &pattern, &replacement, &flags);
    }

    // vim-style global: `:g/pat/d`, `:g!/pat/d`, `:v/pat/d`.
    if let Some((invert, pattern, gcommand)) = parse_vim_global(input) {
        if event != PromptEvent::Validate {
            return Ok(());
        }
        return do_global(cx, invert, &pattern, &gcommand);
    }

    // vim-style move/copy lines: `:m5`, `:t.`, `:co$`.
    if let Some((is_copy, addr)) = parse_vim_lineop(input) {
        if event != PromptEvent::Validate {
            return Ok(());
        }
        return do_move_copy(cx, is_copy, &addr);
    }

    // vim-style range indent/dedent: `:>`, `:<` (the command-line tokenizer
    // rejects `>`/`<` as command names, so handle them here).
    let trimmed = input.trim_start();
    if trimmed.starts_with('>') || trimmed.starts_with('<') {
        if event != PromptEvent::Validate {
            return Ok(());
        }
        return do_indent(cx, trimmed.starts_with('<'));
    }

    // vim-style shell escape: `:!cmd` with the bang directly followed by the command
    // (no space). `:! cmd` already works via the `!` alias, but the no-space form
    // tokenizes as a single unknown command (`!cmd`), so route any line starting with
    // `!` to run-shell-command with the remainder as the command.
    if let Some(shell_cmd) = trimmed.strip_prefix('!') {
        let cmd = TYPABLE_COMMAND_MAP.get("run-shell-command").unwrap();
        return execute_command(cx, cmd, shell_cmd, event);
    }

    match typed::TYPABLE_COMMAND_MAP.get(command) {
        Some(cmd) => execute_command(cx, cmd, rest, event),
        None if event == PromptEvent::Validate => Err(anyhow!("no such command: '{command}'")),
        None => Ok(()),
    }
}

pub(super) fn execute_command(
    cx: &mut compositor::Context,
    cmd: &TypableCommand,
    args: &str,
    event: PromptEvent,
) -> anyhow::Result<()> {
    let args = if event == PromptEvent::Validate {
        Args::parse(args, cmd.signature, true, |token| {
            expansion::expand(cx.editor, token).map_err(|err| err.into())
        })
        .map_err(|err| anyhow!("'{}': {err}", cmd.name))?
    } else {
        Args::parse(args, cmd.signature, false, |token| Ok(token.content))
            .expect("arg parsing cannot fail when validation is turned off")
    };

    (cmd.fun)(cx, args, event).map_err(|err| anyhow!("'{}': {err}", cmd.name))
}

#[allow(clippy::unnecessary_unwrap)]
/// vim `@:`: re-run the most recently executed command-line (`:` history).
/// Run a command line (without the leading `:`) from a picker/job callback that
/// already holds a `compositor::Context`. Used by `command_history_picker`.
pub(super) fn run_command_line(cx: &mut compositor::Context, line: &str) {
    if let Err(err) = execute_command_line(cx, line, PromptEvent::Validate) {
        cx.editor.set_error(err.to_string());
    }
}

pub(super) fn repeat_last_command_line(cx: &mut Context) {
    let last = cx.editor.registers.first(':', cx.editor).map(|c| c.into_owned());
    let Some(cmd) = last else {
        cx.editor.set_error("No previous command-line");
        return;
    };
    let mut cx = compositor::Context {
        editor: cx.editor,
        jobs: cx.jobs,
        scroll: None,
    };
    if let Err(err) = execute_command_line(&mut cx, &cmd, PromptEvent::Validate) {
        cx.editor.set_error(err.to_string());
    }
}

pub(super) fn command_mode(cx: &mut Context) {
    let mut prompt = Prompt::new(
        ":".into(),
        Some(':'),
        complete_command_line,
        move |cx: &mut compositor::Context, input: &str, event: PromptEvent| {
            if let Err(err) = execute_command_line(cx, input, event) {
                cx.editor.set_error(err.to_string());
            }
        },
    );
    prompt.doc_fn = Box::new(command_line_doc);

    // Calculate initial completion
    prompt.recalculate_completion(cx.editor);
    cx.push_layer(Box::new(prompt));
}

fn command_line_doc(input: &str) -> Option<Cow<'_, str>> {
    let (command, _, _) = command_line::split(input);
    let command = TYPABLE_COMMAND_MAP.get(command)?;

    if command.aliases.is_empty() && command.signature.flags.is_empty() {
        return Some(Cow::Borrowed(command.doc));
    }

    let mut doc = command.doc.to_string();

    if !command.aliases.is_empty() {
        write!(doc, "\nAliases: {}", command.aliases.join(", ")).unwrap();
    }

    if !command.signature.flags.is_empty() {
        const ARG_PLACEHOLDER: &str = " <arg>";

        fn flag_len(flag: &Flag) -> usize {
            let name_len = flag.name.len();
            let alias_len = if let Some(alias) = flag.alias {
                "/-".len() + alias.len_utf8()
            } else {
                0
            };
            let arg_len = if flag.completions.is_some() {
                ARG_PLACEHOLDER.len()
            } else {
                0
            };
            name_len + alias_len + arg_len
        }

        doc.push_str("\nFlags:");

        let max_flag_len = command.signature.flags.iter().map(flag_len).max().unwrap();

        for flag in command.signature.flags {
            let mut buf = [0u8; 4];
            let this_flag_len = flag_len(flag);
            write!(
                doc,
                "\n  --{flag_text}{spacer:spacing$}  {doc}",
                doc = flag.doc,
                // `fmt::Arguments` does not respect width controls so we must place the spacers
                // explicitly:
                spacer = "",
                spacing = max_flag_len - this_flag_len,
                flag_text = format_args!(
                    "{}{}{}{}",
                    flag.name,
                    // Ideally this would be written as a `format_args!` too but the borrow
                    // checker is not yet smart enough.
                    if flag.alias.is_some() { "/-" } else { "" },
                    if let Some(alias) = flag.alias {
                        alias.encode_utf8(&mut buf)
                    } else {
                        ""
                    },
                    if flag.completions.is_some() {
                        ARG_PLACEHOLDER
                    } else {
                        ""
                    }
                ),
            )
            .unwrap();
        }
    }

    Some(Cow::Owned(doc))
}

fn complete_command_line(editor: &Editor, input: &str) -> Vec<ui::prompt::Completion> {
    let (command, rest, complete_command) = command_line::split(input);

    if complete_command {
        fuzzy_match(
            input,
            TYPABLE_COMMAND_LIST.iter().map(|command| command.name),
            false,
        )
        .into_iter()
        .map(|(name, _)| (0.., name.into()))
        .collect()
    } else {
        TYPABLE_COMMAND_MAP
            .get(command)
            .map_or_else(Vec::new, |cmd| {
                let args_offset = command.len() + 1;
                complete_command_args(editor, cmd.signature, &cmd.completer, rest, args_offset)
            })
    }
}

pub fn complete_command_args(
    editor: &Editor,
    signature: Signature,
    completer: &CommandCompleter,
    input: &str,
    offset: usize,
) -> Vec<ui::prompt::Completion> {
    use command_line::{CompletionState, ExpansionKind, Tokenizer};

    // TODO: completion should depend on the location of the cursor instead of the end of the
    // string. This refactor is left for the future but the below completion code should respect
    // the cursor position if it becomes a parameter.
    let cursor = input.len();
    let prefix = &input[..cursor];
    let mut tokenizer = Tokenizer::new(prefix, false);
    let mut args = Args::new(signature, false);
    let mut final_token = None;
    let mut is_last_token = true;

    while let Some(token) = args
        .read_token(&mut tokenizer)
        .expect("arg parsing cannot fail when validation is turned off")
    {
        final_token = Some(token.clone());
        args.push(token.content)
            .expect("arg parsing cannot fail when validation is turned off");
        if tokenizer.pos() >= cursor {
            is_last_token = false;
        }
    }

    // Use a fake final token when the input is not terminated with a token. This simulates an
    // empty argument, causing completion on an empty value whenever you type space/tab. For
    // example if you say `":open README.md "` (with that trailing space) you should see the
    // files in the current dir - completing `""` rather than completions for `"README.md"` or
    // `"README.md "`.
    let token = if is_last_token {
        let token = Token::empty_at(prefix.len());
        args.push(token.content.clone()).unwrap();
        token
    } else {
        final_token.unwrap()
    };

    // Don't complete on closed tokens, for example after writing a closing double quote.
    if token.is_terminated {
        return Vec::new();
    }

    match token.kind {
        TokenKind::Unquoted | TokenKind::Quoted(_) => {
            match args.completion_state() {
                CompletionState::Positional => {
                    // If the completion state is positional there must be at least one positional
                    // in `args`.
                    let n = args
                        .len()
                        .checked_sub(1)
                        .expect("completion state to be positional");
                    let completer = completer.for_argument_number(n);

                    completer(editor, &token.content)
                        .into_iter()
                        .map(|(range, span)| quote_completion(&token, range, span, offset))
                        .collect()
                }
                CompletionState::Flag(_) => fuzzy_match(
                    token.content.trim_start_matches('-'),
                    signature.flags.iter().map(|flag| flag.name),
                    false,
                )
                .into_iter()
                .map(|(name, _)| ((offset + token.content_start).., format!("--{name}").into()))
                .collect(),
                CompletionState::FlagArgument(flag) => fuzzy_match(
                    &token.content,
                    flag.completions
                        .expect("flags in FlagArgument always have completions"),
                    false,
                )
                .into_iter()
                .map(|(value, _)| ((offset + token.content_start).., (*value).into()))
                .collect(),
            }
        }
        TokenKind::Expand | TokenKind::Expansion(ExpansionKind::Shell) => {
            // See the comment about the checked sub expect above.
            let arg_completer = matches!(args.completion_state(), CompletionState::Positional)
                .then(|| {
                    let n = args
                        .len()
                        .checked_sub(1)
                        .expect("completion state to be positional");
                    completer.for_argument_number(n)
                });
            complete_expand(editor, &token, arg_completer, offset + token.content_start)
        }
        TokenKind::Expansion(ExpansionKind::Variable) => {
            complete_variable_expansion(&token.content, offset + token.content_start)
        }
        TokenKind::Expansion(ExpansionKind::Unicode) => Vec::new(),
        TokenKind::Expansion(ExpansionKind::Register) => {
            complete_register_expansion(editor, &token.content, offset + token.content_start)
        }
        TokenKind::ExpansionKind => {
            complete_expansion_kind(&token.content, offset + token.content_start)
        }
    }
}

/// Replace the content and optionally update the range of a positional's completion to account
/// for quoting.
///
/// This is used to handle completions of file or directory names for example. When completing a
/// file with a space, tab or percent character in the name, the space should be escaped by
/// quoting the entire token. If the token being completed is already quoted, any quotes within
/// the completion text should be escaped by doubling them.
fn quote_completion<'a>(
    token: &Token,
    range: ops::RangeFrom<usize>,
    mut span: Span<'a>,
    offset: usize,
) -> (ops::RangeFrom<usize>, Span<'a>) {
    fn replace<'a>(text: Cow<'a, str>, from: char, to: &str) -> Cow<'a, str> {
        if text.contains(from) {
            Cow::Owned(text.replace(from, to))
        } else {
            text
        }
    }

    match token.kind {
        TokenKind::Unquoted if span.content.contains([' ', '\t', '%']) => {
            span.content = Cow::Owned(format!(
                "'{}{}'",
                // Escape any inner single quotes by doubling them.
                replace(token.content[..range.start].into(), '\'', "''"),
                replace(span.content, '\'', "''")
            ));
            // Ignore `range.start` here since we're replacing the entire token. We used
            // `range.start` above to emulate the replacement that using `range.start` would have
            // done.
            ((offset + token.content_start).., span)
        }
        TokenKind::Quoted(quote) => {
            span.content = replace(span.content, quote.char(), quote.escape());
            ((range.start + offset + token.content_start).., span)
        }
        TokenKind::Expand => {
            // NOTE: `token.content_start` is already accounted for in `offset` for `Expand`
            // tokens.
            span.content = replace(span.content, '"', "\"\"");
            ((range.start + offset).., span)
        }
        _ => ((range.start + offset + token.content_start).., span),
    }
}

fn complete_expand(
    editor: &Editor,
    token: &Token,
    completer: Option<&Completer>,
    offset: usize,
) -> Vec<ui::prompt::Completion> {
    use command_line::{ExpansionKind, Tokenizer};

    let mut start = 0;

    // If the expand token contains expansions, complete those.
    while let Some(idx) = token.content[start..].find('%') {
        let idx = start + idx;
        if token.content.as_bytes().get(idx + '%'.len_utf8()).copied() == Some(b'%') {
            // Two percents together are skipped.
            start = idx + ('%'.len_utf8() * 2);
        } else {
            let mut tokenizer = Tokenizer::new(&token.content[idx..], false);
            let token = tokenizer
                .parse_percent_token()
                .map(|token| token.expect("arg parser cannot fail when validation is disabled"));
            start = idx + tokenizer.pos();

            // Like closing quote characters in `complete_command_args` above, don't provide
            // completions if the token is already terminated. This also skips expansions
            // which have already been fully written, for example
            // `"%{cursor_line}:%{cursor_col` should complete `cursor_column` instead of
            // `cursor_line`.
            let Some(token) = token.filter(|t| !t.is_terminated) else {
                continue;
            };

            let local_offset = offset + idx + token.content_start;
            match token.kind {
                TokenKind::Expansion(ExpansionKind::Variable) => {
                    return complete_variable_expansion(&token.content, local_offset);
                }
                TokenKind::Expansion(ExpansionKind::Shell) => {
                    return complete_expand(editor, &token, None, local_offset);
                }
                TokenKind::ExpansionKind => {
                    return complete_expansion_kind(&token.content, local_offset);
                }
                _ => continue,
            }
        }
    }

    match completer {
        // If no expansions were found and an argument is being completed,
        Some(completer) if start == 0 => completer(editor, &token.content)
            .into_iter()
            .map(|(range, span)| quote_completion(token, range, span, offset))
            .collect(),
        _ => Vec::new(),
    }
}

fn complete_variable_expansion(content: &str, offset: usize) -> Vec<ui::prompt::Completion> {
    use expansion::Variable;

    fuzzy_match(
        content,
        Variable::VARIANTS.iter().map(Variable::as_str),
        false,
    )
    .into_iter()
    .map(|(name, _)| (offset.., (*name).into()))
    .collect()
}

fn complete_register_expansion(
    editor: &Editor,
    content: &str,
    offset: usize,
) -> Vec<ui::prompt::Completion> {
    let register_names: Vec<String> = editor
        .registers
        .iter_preview()
        .map(|(ch, _)| ch.to_string())
        .collect();
    fuzzy_match(content, register_names, false)
        .into_iter()
        .map(|(name, _)| (offset.., name.to_string().into()))
        .collect()
}

fn complete_expansion_kind(content: &str, offset: usize) -> Vec<ui::prompt::Completion> {
    use command_line::ExpansionKind;

    fuzzy_match(
        content,
        // Skip `ExpansionKind::Variable` since its kind string is empty.
        ExpansionKind::VARIANTS
            .iter()
            .skip(1)
            .map(ExpansionKind::as_str),
        false,
    )
    .into_iter()
    .map(|(name, _)| (offset.., (*name).into()))
    .collect()
}

fn current_workspace(cx: &compositor::Context) -> std::path::PathBuf {
    let (_, doc) = current_ref!(cx.editor);
    doc.workspace_root().to_path_buf()
}

/// Whether the currently focused document's workspace is trusted for git operations (gix
/// `Trust::Full`).
fn doc_trust_full(editor: &zemacs_view::Editor) -> bool {
    let (_, doc) = current_ref!(editor);
    editor
        .workspace_trust
        .query(
            doc.workspace_root(),
            zemacs_loader::workspace_trust::TrustQuery::Git,
        )
        .is_trusted()
}

fn trust_workspace(
    cx: &mut compositor::Context,
    args: Args<'_>,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    let workspace = current_workspace(cx);
    cx.editor.workspace_trust.trust(&workspace);

    cx.editor.config_events.0.send(ConfigEvent::Refresh)?;
    // Restart any LSPs that didn't start because trust was missing.
    lsp_restart(cx, args, event)
}

fn untrust_workspace(
    cx: &mut compositor::Context,
    _args: Args<'_>,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    let workspace = current_workspace(cx);
    cx.editor.workspace_trust.untrust(&workspace);
    // Drop any workspace overrides that were merged into the live editor config while trust was
    // granted. Running LSPs are not stopped here (use `:lsp-stop` for that); this only handles
    // in-memory config.
    cx.editor.config_events.0.send(ConfigEvent::Refresh)?;
    Ok(())
}

fn exclude_workspace(
    cx: &mut compositor::Context,
    _args: Args<'_>,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    let workspace = current_workspace(cx);
    cx.editor.workspace_trust.exclude(&workspace);
    cx.editor.config_events.0.send(ConfigEvent::Refresh)?;
    Ok(())
}

#[cfg(test)]
mod vim_set_tests {
    use super::*;

    #[test]
    fn parse_set_tokens() {
        assert_eq!(parse_set_token("nu"), (false, false, "nu", None));
        assert_eq!(parse_set_token("nonumber"), (true, false, "number", None));
        assert_eq!(parse_set_token("tw=80"), (false, false, "tw", Some("80")));
        assert_eq!(parse_set_token("wrap!"), (false, true, "wrap", None));
        assert_eq!(parse_set_token("invwrap"), (false, true, "wrap", None));
    }

    fn tr(tok: &str, cur: bool) -> (String, Value) {
        translate_vim_option(tok, |_| cur).unwrap().unwrap()
    }

    #[test]
    fn translate_options() {
        assert_eq!(tr("nu", false), ("line-number".into(), Value::String("absolute".into())));
        assert_eq!(tr("rnu", false), ("line-number".into(), Value::String("relative".into())));
        assert_eq!(
            tr("norelativenumber", false),
            ("line-number".into(), Value::String("absolute".into()))
        );
        assert_eq!(tr("tw=80", false), ("text-width".into(), Value::Number(80.into())));
        assert_eq!(tr("nowrap", false), ("soft-wrap.enable".into(), Value::Bool(false)));
        assert_eq!(tr("wrap", false), ("soft-wrap.enable".into(), Value::Bool(true)));
        // toggle from current=false -> true
        assert_eq!(tr("cursorline!", false), ("cursorline".into(), Value::Bool(true)));
        assert_eq!(tr("scrolloff=5", false), ("scrolloff".into(), Value::Number(5.into())));
        // unknown option -> None
        assert!(translate_vim_option("definitelynotanoption", |_| false).is_none());
    }
}
