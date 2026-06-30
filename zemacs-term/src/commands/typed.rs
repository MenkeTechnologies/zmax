use std::fmt::Write;
use std::io::BufReader;
use std::ops::{self, Deref};

use crate::job::Job;

use super::*;

use serde_json::Value;
use ui::completers::{self, Completer};
use zemacs_core::command_line::{Args, Flag, Signature, Token, TokenKind};
use zemacs_core::fuzzy::fuzzy_match;
use zemacs_core::indent::MAX_INDENT;
use zemacs_core::line_ending;
use zemacs_stdx::path::home_dir;
use zemacs_view::document::{read_to_string, DEFAULT_LANGUAGE_NAME};
use zemacs_view::editor::{CloseError, ConfigEvent};
use zemacs_view::expansion;

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
            // Otherwise, just open the file — a binary file opens in the hex
            // editor instead of being rejected.
            match cx.editor.open(&path, action) {
                Ok(_) => {}
                Err(zemacs_view::DocumentOpenError::BinaryFile) => {
                    push_hex_view(cx, path.into_owned());
                    continue;
                }
                Err(e) => return Err(e.into()),
            }
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

/// `:nohlsearch` / `:noh` — clear the persistent search highlight (vim `:nohlsearch`). Mirrors the
/// `clear_search_highlight` command: drop the last-search register and clear the status line.
fn no_highlight_search(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let reg = cx.editor.registers.last_search_register;
    cx.editor.registers.remove(reg);
    cx.editor.clear_status();
    Ok(())
}

/// `:clearjumps` — empty the current view's jump list (vim `:clearjumps`).
fn clear_jumps(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    view_mut!(cx.editor).jumps.clear();
    cx.editor.set_status("jumps cleared");
    Ok(())
}

/// `:buffers` / `:ls` / `:files` — list open buffers (vim). zemacs surfaces the list as the
/// interactive buffer picker rather than a textual dump.
fn buffers_list(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let call: job::Callback = job::Callback::EditorCompositor(Box::new(
        move |editor: &mut Editor, compositor: &mut Compositor| {
            compositor.push(crate::commands::build_buffer_picker(editor, None, false));
        },
    ));
    cx.jobs.callback(async move { Ok(call) });
    Ok(())
}

/// `:jumps` — list the jump list in a picker (vim `:jumps`).
fn jumps_list(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let call: job::Callback = job::Callback::EditorCompositor(Box::new(
        move |editor: &mut Editor, compositor: &mut Compositor| {
            compositor.push(crate::commands::build_jumplist_picker(editor));
        },
    ));
    cx.jobs.callback(async move { Ok(call) });
    Ok(())
}

/// `:oldfiles` / `:browse oldfiles` — pick from recently edited files (vim `:oldfiles`).
fn oldfiles_list(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let call: job::Callback = job::Callback::EditorCompositor(Box::new(
        move |editor: &mut Editor, compositor: &mut Compositor| {
            match crate::commands::build_frecent_file_picker() {
                Some(picker) => compositor.push(picker),
                None => editor.set_status("No recent files yet"),
            }
        },
    ));
    cx.jobs.callback(async move { Ok(call) });
    Ok(())
}

/// `:marks` — list the buffer's marks in a picker (vim `:marks`).
fn marks_list(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let call: job::Callback = job::Callback::EditorCompositor(Box::new(
        move |editor: &mut Editor, compositor: &mut Compositor| {
            match crate::commands::build_marks_picker(editor) {
                Some(picker) => compositor.push(picker),
                None => editor.set_status("No marks set"),
            }
        },
    ));
    cx.jobs.callback(async move { Ok(call) });
    Ok(())
}

/// `:history` / `:history :` — pick from the command-line history (vim `:history`).
fn history_list(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let call: job::Callback = job::Callback::EditorCompositor(Box::new(
        move |editor: &mut Editor, compositor: &mut Compositor| {
            match crate::commands::build_command_history_picker(editor) {
                Some(picker) => compositor.push(picker),
                None => editor.set_status("No command-line history"),
            }
        },
    ));
    cx.jobs.callback(async move { Ok(call) });
    Ok(())
}

/// `:delmarks {marks}` — delete the listed named marks (vim `:delmarks`/`:delm`).
fn del_marks(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let spec: String = args.join("");
    let spec = spec.trim();
    ensure!(!spec.is_empty(), ":delmarks requires marks (or use :delmarks! for all)");
    let n = {
        let doc = doc_mut!(cx.editor);
        spec.chars()
            .filter(|c| !c.is_whitespace() && *c != ',')
            .filter(|&c| doc.remove_mark(c))
            .count()
    };
    cx.editor.set_status(format!("deleted {n} mark(s)"));
    Ok(())
}

/// `:project-replace {pattern} {replacement}` / `:replace-in-files` — JetBrains "Replace in Files"
/// (Cmd Shift R): regex-replace across every workspace file that matches `pattern` (ripgrep finds the
/// files; `$1`-style capture refs work in the replacement), writing changed files and reloading any
/// open buffers. Destructive across the working tree — recoverable via git.
fn project_replace(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let pattern = args[0].to_string();
    let replacement = args[1].to_string();
    let re = regex::Regex::new(&pattern).map_err(|e| anyhow!("invalid pattern: {e}"))?;
    let root = zemacs_loader::find_workspace().0;
    cx.editor.set_status("project-replace: searching…");
    cx.jobs.callback(async move {
        let res = tokio::task::spawn_blocking(
            move || -> (usize, usize, Vec<std::path::PathBuf>) {
                let out = std::process::Command::new("rg")
                    .arg("-l")
                    .arg("--null")
                    .arg("-e")
                    .arg(&pattern)
                    .current_dir(&root)
                    .output();
                let files: Vec<std::path::PathBuf> = match out {
                    Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
                        .split('\0')
                        .filter(|s| !s.is_empty())
                        .map(|s| root.join(s))
                        .collect(),
                    _ => Vec::new(),
                };
                let mut changed = Vec::new();
                let mut total = 0usize;
                for path in files {
                    let Ok(content) = std::fs::read_to_string(&path) else {
                        continue;
                    };
                    let count = re.find_iter(&content).count();
                    if count == 0 {
                        continue;
                    }
                    let new = re.replace_all(&content, replacement.as_str());
                    if new != content && std::fs::write(&path, new.as_bytes()).is_ok() {
                        total += count;
                        changed.push(path);
                    }
                }
                (changed.len(), total, changed)
            },
        )
        .await
        .map_err(|e| anyhow!("project-replace task: {e}"))?;
        let (nfiles, total, paths) = res;
        Ok(job::Callback::Editor(Box::new(move |editor: &mut Editor| {
            crate::commands::reload_docs_for_paths(editor, &paths);
            editor.set_status(format!(
                "project-replace: {total} replacement(s) in {nfiles} file(s)"
            ));
        })))
    });
    Ok(())
}

/// `:delmarks!` — delete all lowercase/uppercase letter marks (vim `:delmarks!`).
fn del_marks_all(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    doc_mut!(cx.editor).clear_letter_marks();
    cx.editor.set_status("deleted all letter marks");
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
        names.extend(zemacs_view::theme::Loader::read_names(
            &rt_dir.join("themes"),
        ));
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
    // Reset EVERY hunk intersecting the selection's line ranges — so a visual
    // selection across several hunks resets all of them, while a plain cursor
    // (a one-line range) resets just the hunk under it.
    let scrolloff = cx.editor.config().scrolloff;
    let (view, doc) = current!(cx.editor);
    let Some(handle) = doc.diff_handle() else {
        bail!("Diff is not available in the current buffer");
    };
    let diff = handle.load();
    let doc_text = doc.text().slice(..);
    let diff_base = diff.diff_base();
    let mut count = 0usize;
    let transaction = Transaction::change(
        doc.text(),
        diff.hunks_intersecting_line_ranges(doc.selection(view.id).line_ranges(doc_text))
            .map(|hunk| {
                count += 1;
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
    if count == 0 {
        bail!("No git hunk under the selection");
    }
    drop(diff); // release the immutable diff borrow before mutating the doc
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    view.ensure_cursor_in_view(doc, scrolloff);
    cx.editor.set_status(format!(
        "Reset {count} hunk{}",
        if count == 1 { "" } else { "s" }
    ));
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
    Some((
        text.line_to_char(start),
        text.line_to_char(end + 1),
        ours,
        theirs,
    ))
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
    cx.editor
        .set_status(format!("conflict resolved: kept {which}"));
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
fn theme_toggle(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
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
    let dark = args
        .first()
        .map(|s| s.to_string())
        .unwrap_or_else(|| "zgui-cyberpunk".to_string());
    let light = args
        .get(1)
        .map(|s| s.to_string())
        .unwrap_or_else(|| "catppuccin_latte".to_string());
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

/// `:ide` — enter IDE mode (the file-tree sidebar + tool panels you get from
/// `zemacs --ide`). Toggles the workbench like `F2` / `SPC z`: the first call
/// boots and shows it, the next hides it for distraction-free editing.
fn ide(cx: &mut compositor::Context, _args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let call: job::Callback = job::Callback::EditorCompositor(Box::new(
        move |_editor: &mut Editor, compositor: &mut Compositor| {
            if let Some(view) = compositor.find::<crate::ui::EditorView>() {
                view.toggle_ide();
            }
        },
    ));
    cx.jobs.callback(async move { Ok(call) });
    Ok(())
}

/// `:diff` — open a read-only, full-screen side-by-side diff viewer comparing
/// the focused buffer's git `HEAD` version (left) with the current working-tree
/// buffer (right). Changed/added/removed lines are aligned and highlighted with
/// synchronized scrolling and `n`/`p` change-to-change navigation. Requires a
/// git diff base for the file (otherwise a status message is shown).
fn diff(cx: &mut compositor::Context, _args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    open_diff(cx.editor, cx.jobs);
    Ok(())
}

/// Shared implementation behind `:diff` and the `git_diff` static command:
/// open a read-only side-by-side diff of the focused buffer vs. its git HEAD.
pub(crate) fn open_diff(editor: &mut Editor, jobs: &mut crate::job::Jobs) {
    // Pull the HEAD text + current buffer text on the main thread, then drop the
    // document borrow before touching `editor` mutably.
    let data = {
        let doc = doc!(editor);
        doc.diff_handle().map(|handle| {
            let base = handle.load().diff_base().to_string();
            (
                doc.id(),
                doc.display_name().into_owned(),
                base,
                doc.text().to_string(),
            )
        })
    };

    let Some((doc_id, name, base, current)) = data else {
        editor.set_status("no git diff base for this file");
        return;
    };
    if base.is_empty() {
        editor.set_status("no git diff base for this file");
        return;
    }

    let view = crate::ui::merge::DiffView::new(name, doc_id, &base, &current);
    if view.is_unchanged() {
        editor.set_status("no changes against git HEAD");
        return;
    }

    let call: job::Callback = job::Callback::EditorCompositor(Box::new(
        move |_editor: &mut Editor, compositor: &mut Compositor| {
            compositor.push(Box::new(view));
        },
    ));
    jobs.callback(async move { Ok(call) });
}

/// `:merge` / `:resolve` — open the 3-pane merge-conflict resolver over the
/// focused buffer's git conflict markers (`<<<<<<< ======= >>>>>>>`). Each
/// conflict is shown with ours on the left, theirs on the right and a live
/// resolved Result in the center; choosing ours/theirs/both/unresolved per
/// conflict and pressing `Enter` writes the result back and, once every
/// conflict is resolved, writes the file and `git add`s it. If the buffer has
/// no conflict markers a status message is shown instead.
fn merge(cx: &mut compositor::Context, _args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    open_merge(cx.editor, cx.jobs);
    Ok(())
}

/// Shared implementation behind `:merge` / `:resolve` and the `resolve_conflicts`
/// static command: open the 3-pane merge-conflict resolver over the focused
/// buffer's git conflicts.
pub(crate) fn open_merge(editor: &mut Editor, jobs: &mut crate::job::Jobs) {
    // Capture the focused doc's id, name, absolute path and text, then drop the
    // borrow before touching `editor` mutably.
    let (doc_id, name, path, text) = {
        let doc = doc!(editor);
        (
            doc.id(),
            doc.display_name().into_owned(),
            doc.path().map(|p| p.to_path_buf()),
            doc.text().to_string(),
        )
    };

    // Prefer the git **index** conflict stages: a real diff3 against the common
    // ancestor auto-merges the non-conflicting changes and gives every conflict
    // its true per-region base. Only fall back to scraping `<<<<<<<` markers out
    // of the buffer when the file isn't a git conflict (or git is unavailable).
    let staged_segments = path.as_deref().and_then(|p| {
        let trust_full = editor
            .workspace_trust
            .query(
                &zemacs_loader::find_workspace_in(p.parent().unwrap_or(p)).0,
                zemacs_loader::workspace_trust::TrustQuery::Git,
            )
            .is_trusted();
        let stages = editor.diff_providers.get_conflict_stages(p, trust_full)?;
        // Need at least ours + theirs to run a 3-way merge; missing base is fine
        // (add/add → "" base).
        let ours = stages.ours?;
        let theirs = stages.theirs?;
        let base = stages.base.unwrap_or_default();
        let decode = |b: Vec<u8>| String::from_utf8_lossy(&b).into_owned();
        Some(crate::ui::merge::diff3(
            &decode(base),
            &decode(ours),
            &decode(theirs),
        ))
    });

    let segments = match staged_segments {
        Some(segments) => segments,
        None => match crate::ui::merge::parse_conflicts(&text) {
            Some(segments) => segments,
            None => {
                editor.set_status("no conflicts to resolve");
                return;
            }
        },
    };

    let view = crate::ui::merge::DiffView::from_conflicts(name, doc_id, path, segments);
    let call: job::Callback = job::Callback::EditorCompositor(Box::new(
        move |_editor: &mut Editor, compositor: &mut Compositor| {
            compositor.push(Box::new(view));
        },
    ));
    jobs.callback(async move { Ok(call) });
}

/// `:magit` / `:git` / `:gst` — open the Magit-style git status porcelain: a
/// full-screen overlay listing the repo's untracked / unstaged / staged changes
/// and merge conflicts in sections, with stage/unstage/discard/commit and a
/// live refresh. The repo is derived from the focused file's directory, falling
/// back to the current working directory.
fn magit(cx: &mut compositor::Context, _args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    open_magit(cx.editor, cx.jobs);
    Ok(())
}

/// Shared implementation behind `:magit` / `:git` / `:gst` and the `git_status`
/// static command: open the Magit-style git status porcelain over the repo
/// derived from the focused file's directory (falling back to the cwd).
pub(crate) fn open_magit(editor: &mut Editor, jobs: &mut crate::job::Jobs) {
    // Derive a starting directory from the focused file, else the cwd.
    let start = doc!(editor)
        .path()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| {
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
        });
    match crate::ui::magit::MagitStatus::new(&start) {
        Some(view) => {
            let call: job::Callback = job::Callback::EditorCompositor(Box::new(
                move |_editor: &mut Editor, compositor: &mut Compositor| {
                    compositor.push(Box::new(view));
                },
            ));
            jobs.callback(async move { Ok(call) });
        }
        None => editor.set_status("not inside a git repository"),
    }
}

/// `:hex` — open a read-only, full-screen `xxd`-style hex viewer of a file's raw
/// bytes (offset gutter, 16 hex bytes per row grouped 8 + 8, and an ASCII
/// gutter). With no argument the focused document's file is shown; with an
/// argument that path is read instead. The bytes are read straight off disk
/// (not the text buffer) so arbitrary non-UTF-8 content is shown faithfully.
fn hex(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    // Resolve the target path: the positional argument, else the focused doc.
    let path = match args.first() {
        Some(arg) => Some(zemacs_stdx::path::expand_tilde(Path::new(arg)).into_owned()),
        None => doc!(cx.editor).path().map(|p| p.to_path_buf()),
    };

    let Some(path) = path else {
        cx.editor
            .set_status("no file to view; pass a path: :hex <file>");
        return Ok(());
    };

    push_hex_view(cx, path);
    Ok(())
}

/// Read `path`'s raw bytes and open them in the hex editor overlay. Shared by the
/// `:hex` command and by binary-file open routing (a binary file opens here
/// instead of being rejected). Sets a status message on read failure.
pub(crate) fn push_hex_view(cx: &mut compositor::Context, path: std::path::PathBuf) {
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(err) => {
            cx.editor
                .set_status(format!("can't read {}: {err}", path.display()));
            return;
        }
    };

    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    let view = crate::ui::hex::HexView::new(name, Some(path), bytes);

    let call: job::Callback = job::Callback::EditorCompositor(Box::new(
        move |_editor: &mut Editor, compositor: &mut Compositor| {
            compositor.push(Box::new(view));
        },
    ));
    cx.jobs.callback(async move { Ok(call) });
}

/// `:snippets` — open the user snippet library editor: a full-screen two-pane
/// TUI to create / edit / delete reusable snippets (trigger + scope + body in
/// LSP snippet syntax). The library persists to `<config-dir>/snippets.toml`.
/// Expansion-on-trigger is a later slice; this opens the manager only.
fn snippets(cx: &mut compositor::Context, _args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let view = crate::ui::snippets::SnippetPanel::new();
    let call: job::Callback = job::Callback::EditorCompositor(Box::new(
        move |_editor: &mut Editor, compositor: &mut Compositor| {
            compositor.push(Box::new(view));
        },
    ));
    cx.jobs.callback(async move { Ok(call) });
    Ok(())
}

// --- Org-mode (slice 1: outline folding + TODO cycling) ----------------------
// Pure logic lives in `super::org`; these typable commands wire it to the focused
// buffer. Folding reuses the document's `Folds` model exactly like the vim `z*`
// fold commands: a closed fold over `heading_line..=subtree_end` hides the body
// while keeping the heading visible.

/// Document line the primary cursor sits on, in the focused view.
fn org_cursor_line(editor: &mut Editor) -> usize {
    let (view, doc) = current!(editor);
    let text = doc.text().slice(..);
    let cursor = doc.selection(view.id).primary().cursor(text);
    text.char_to_line(cursor)
}

/// Content of document `line` with any trailing line ending stripped.
fn org_line_text(doc: &Document, line: usize) -> String {
    doc.text()
        .line(line)
        .chars()
        .collect::<String>()
        .trim_end_matches(['\n', '\r'])
        .to_string()
}

/// Every document line as an owned string (line endings stripped) — the form the
/// pure `org::subtree_end` helper consumes.
fn org_all_lines(doc: &Document) -> Vec<String> {
    (0..doc.text().len_lines())
        .map(|l| org_line_text(doc, l))
        .collect()
}

/// Replace document `line`'s content (excluding its line ending) with `new`.
fn org_replace_line(doc: &mut Document, view: &mut View, line: usize, new: String) {
    let line_start = doc.text().line_to_char(line);
    let line_end = line_start + org_line_text(doc, line).chars().count();
    let tx = Transaction::change(
        doc.text(),
        std::iter::once((line_start, line_end, Some(Tendril::from(new)))),
    );
    doc.apply(&tx, view.id);
    doc.append_changes_to_history(view);
}

/// `:org-cycle` / `:org-fold` — toggle a fold over the current heading's subtree.
/// If a closed fold already starts on the heading it is opened; otherwise a
/// closed fold over `heading..=subtree_end` is created (reusing `Folds`).
fn org_cycle(cx: &mut compositor::Context, _args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    org_toggle_fold(cx.editor);
    Ok(())
}

/// Shared implementation behind `:org-cycle` / `:org-fold` and the `org_cycle`
/// static command: toggle a fold over the current heading's subtree.
pub(crate) fn org_toggle_fold(editor: &mut Editor) {
    let line = org_cursor_line(editor);
    let (_view, doc) = current!(editor);
    let line_str = org_line_text(doc, line);
    if super::org::heading_level(&line_str).is_none() {
        editor.set_status("org-cycle: not on a heading");
        return;
    }
    let lines = org_all_lines(doc);
    let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
    let end = super::org::subtree_end(&refs, line);
    if end <= line {
        editor.set_status("org-cycle: heading has no subtree to fold");
        return;
    }
    if doc.folds().closed_fold_starting_at(line).is_some() {
        doc.folds_mut().open(line);
        editor.set_status("org-cycle: expanded subtree");
    } else {
        let last = doc.text().len_lines().saturating_sub(1);
        doc.folds_mut().create(line, end);
        doc.folds_mut().clamp(last);
        editor.set_status(format!("org-cycle: folded lines {}-{}", line + 1, end + 1));
    }
}

/// `:org-todo` — cycle the current heading's TODO keyword (none → TODO → DONE → none).
fn org_todo(cx: &mut compositor::Context, _args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    org_cycle_keyword(cx.editor);
    Ok(())
}

/// Shared implementation behind `:org-todo` and the `org_todo` static command:
/// cycle the current heading's TODO keyword (none → TODO → DONE → none).
pub(crate) fn org_cycle_keyword(editor: &mut Editor) {
    let line = org_cursor_line(editor);
    let (view, doc) = current!(editor);
    let line_str = org_line_text(doc, line);
    if super::org::heading_level(&line_str).is_none() {
        editor.set_status("org-todo: not on a heading");
        return;
    }
    let new = super::org::cycle_todo(&line_str);
    org_replace_line(doc, view, line, new);
}

/// `:org-promote` — promote the current heading one level (remove a leading `*`).
fn org_promote(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    org_promote_heading(cx.editor);
    Ok(())
}

/// Shared implementation behind `:org-promote` and the `org_promote` static
/// command: promote the current heading one level (remove a leading `*`).
pub(crate) fn org_promote_heading(editor: &mut Editor) {
    let line = org_cursor_line(editor);
    let (view, doc) = current!(editor);
    let line_str = org_line_text(doc, line);
    if super::org::heading_level(&line_str).is_none() {
        editor.set_status("org-promote: not on a heading");
        return;
    }
    let new = super::org::promote(&line_str);
    org_replace_line(doc, view, line, new);
}

/// `:org-demote` — demote the current heading one level (add a leading `*`).
fn org_demote(cx: &mut compositor::Context, _args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    org_demote_heading(cx.editor);
    Ok(())
}

/// Shared implementation behind `:org-demote` and the `org_demote` static
/// command: demote the current heading one level (add a leading `*`).
pub(crate) fn org_demote_heading(editor: &mut Editor) {
    let line = org_cursor_line(editor);
    let (view, doc) = current!(editor);
    let line_str = org_line_text(doc, line);
    if super::org::heading_level(&line_str).is_none() {
        editor.set_status("org-demote: not on a heading");
        return;
    }
    let new = super::org::demote(&line_str);
    org_replace_line(doc, view, line, new);
}

/// `:org-next-heading` — move the cursor to the next heading line, if any.
fn org_next_heading(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    org_goto_next_heading(cx.editor);
    Ok(())
}

/// Shared implementation behind `:org-next-heading` and the `org_next_heading`
/// static command: move the cursor to the next heading line, if any.
pub(crate) fn org_goto_next_heading(editor: &mut Editor) {
    let line = org_cursor_line(editor);
    let (view, doc) = current!(editor);
    let total = doc.text().len_lines();
    let target =
        (line + 1..total).find(|&l| super::org::heading_level(&org_line_text(doc, l)).is_some());
    match target {
        Some(l) => {
            let pos = doc.text().line_to_char(l);
            doc.set_selection(view.id, Selection::point(pos));
        }
        None => editor.set_status("org: no next heading"),
    }
}

/// `:org-prev-heading` — move the cursor to the previous heading line, if any.
fn org_prev_heading(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    org_goto_prev_heading(cx.editor);
    Ok(())
}

/// Shared implementation behind `:org-prev-heading` and the `org_prev_heading`
/// static command: move the cursor to the previous heading line, if any.
pub(crate) fn org_goto_prev_heading(editor: &mut Editor) {
    let line = org_cursor_line(editor);
    let (view, doc) = current!(editor);
    let target = (0..line)
        .rev()
        .find(|&l| super::org::heading_level(&org_line_text(doc, l)).is_some());
    match target {
        Some(l) => {
            let pos = doc.text().line_to_char(l);
            doc.set_selection(view.id, Selection::point(pos));
        }
        None => editor.set_status("org: no previous heading"),
    }
}

/// `:org-fold-all` — fold every heading subtree in the buffer.
fn org_fold_all(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    org_fold_all_headings(cx.editor);
    Ok(())
}

/// Shared implementation behind `:org-fold-all` and the `org_fold_all` static
/// command: fold every heading subtree in the buffer.
pub(crate) fn org_fold_all_headings(editor: &mut Editor) {
    let cursor = org_cursor_line(editor);
    let (view, doc) = current!(editor);
    let lines = org_all_lines(doc);
    let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
    let mut count = 0usize;
    for (line, l) in refs.iter().enumerate() {
        if super::org::heading_level(l).is_some() {
            let end = super::org::subtree_end(&refs, line);
            if end > line && doc.folds_mut().create(line, end) {
                count += 1;
            }
        }
    }
    let last = doc.text().len_lines().saturating_sub(1);
    doc.folds_mut().clamp(last);
    // Pull the cursor out of any region we just hid.
    let anchor = doc.folds().visible_anchor(cursor);
    if anchor != cursor {
        let pos = doc.text().line_to_char(anchor);
        doc.set_selection(view.id, Selection::point(pos));
    }
    editor.set_status(format!("org: folded {count} heading(s)"));
}

/// `:org-unfold-all` — open every fold in the buffer.
fn org_unfold_all(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    org_unfold_all_folds(cx.editor);
    Ok(())
}

/// Shared implementation behind `:org-unfold-all` and the `org_unfold_all`
/// static command: open every fold in the buffer.
pub(crate) fn org_unfold_all_folds(editor: &mut Editor) {
    let (_view, doc) = current!(editor);
    doc.folds_mut().open_all();
    editor.set_status("org: unfolded all");
}

/// `:org-agenda` / `:agenda` — open the org agenda overlay: TODO/DONE headings
/// gathered from every open `.org` buffer and every `*.org` file under the
/// working directory, grouped by scheduled/deadline date, undated TODOs and done
/// items. `Enter` jumps to an item; see [`crate::ui::org_agenda::OrgAgenda`].
fn org_agenda(cx: &mut compositor::Context, _args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    open_org_agenda(cx.editor, cx.jobs);
    Ok(())
}

/// Shared implementation behind `:org-agenda` / `:agenda` and the `org_agenda`
/// static command: open the org agenda overlay over the working tree.
pub(crate) fn open_org_agenda(editor: &mut Editor, jobs: &mut crate::job::Jobs) {
    // Scan the focused file's directory if it has one, else the cwd.
    let root = doc!(editor)
        .path()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| {
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
        });
    let agenda = crate::ui::org_agenda::OrgAgenda::new(editor, root);
    let call: job::Callback = job::Callback::EditorCompositor(Box::new(
        move |_editor: &mut Editor, compositor: &mut Compositor| {
            compositor.push(Box::new(agenda));
        },
    ));
    jobs.callback(async move { Ok(call) });
}

/// `:org-priority` — cycle the current heading's priority cookie: none → `[#A]` →
/// `[#B]` → `[#C]` → none, applied as a single undoable line replacement.
fn org_priority(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    org_cycle_priority(cx.editor);
    Ok(())
}

/// Shared implementation behind `:org-priority` and the `org_priority` static
/// command: cycle the current heading's priority cookie (none → `[#A]` → `[#B]`
/// → `[#C]` → none).
pub(crate) fn org_cycle_priority(editor: &mut Editor) {
    let line = org_cursor_line(editor);
    let (view, doc) = current!(editor);
    let line_str = org_line_text(doc, line);
    if super::org::heading_level(&line_str).is_none() {
        editor.set_status("org-priority: not on a heading");
        return;
    }
    let new = super::org::priority_cycle(&line_str);
    org_replace_line(doc, view, line, new);
}

/// `:org-capture` / `:capture` — prompt for a line of text and append it as a
/// `* TODO <text>` entry to an inbox org file. With an argument the inbox is that
/// path (relative paths resolve against the working dir); otherwise it defaults
/// to `<working-dir>/inbox.org`. The file (and any parent dirs) is created on
/// demand and only ever appended to. Pure parts live in [`super::org`].
fn org_capture(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    open_org_capture(cx.editor, cx.jobs, args.first());
    Ok(())
}

/// Shared implementation behind `:org-capture` / `:capture` and the `org_capture`
/// static command: prompt for a line of text and append it as a `* TODO <text>`
/// entry to an inbox org file. `arg` is the optional explicit inbox path.
pub(crate) fn open_org_capture(
    editor: &mut Editor,
    jobs: &mut crate::job::Jobs,
    arg: Option<&str>,
) {
    // Resolve the inbox path from the optional argument + working directory.
    let working_dir = doc!(editor)
        .path()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| {
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
        });
    let inbox = super::org::inbox_path(arg, &working_dir);

    let call: job::Callback = job::Callback::EditorCompositor(Box::new(
        move |_editor: &mut Editor, compositor: &mut Compositor| {
            let prompt = Prompt::new(
                "capture: ".into(),
                None,
                ui::completers::none,
                move |cx: &mut compositor::Context, input: &str, event: PromptEvent| {
                    if event != PromptEvent::Validate {
                        return;
                    }
                    if input.trim().is_empty() {
                        cx.editor.set_status("org-capture: nothing captured");
                        return;
                    }
                    match super::org::append_capture(&inbox, input) {
                        Ok(_) => cx
                            .editor
                            .set_status(format!("org-capture: appended to {}", inbox.display())),
                        Err(err) => cx.editor.set_error(format!(
                            "org-capture: failed to write {}: {err}",
                            inbox.display()
                        )),
                    }
                },
            );
            compositor.push(Box::new(prompt));
        },
    ));
    jobs.callback(async move { Ok(call) });
}

/// `:terminal` / `:term` — open an integrated terminal (PTY shell). The panel is
/// created inside the compositor callback so the PTY handle lives on the main
/// thread (it isn't `Send`).
fn terminal(cx: &mut compositor::Context, _args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let call: job::Callback = job::Callback::EditorCompositor(Box::new(
        move |editor: &mut Editor, compositor: &mut Compositor| {
            match crate::ui::terminal::TerminalPanel::new() {
                Ok(panel) => compositor.push(Box::new(panel)),
                Err(e) => editor.set_error(format!("terminal: {e}")),
            }
        },
    ));
    cx.jobs.callback(async move { Ok(call) });
    Ok(())
}

/// Wrap `s` in single quotes for safe inclusion in a `/bin/sh -c` command line,
/// escaping any embedded single quotes. Pure — unit tested.
fn shell_single_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        if c == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(c);
        }
    }
    out.push('\'');
    out
}

/// Build the shell command for a project search: ripgrep `--vimgrep` (jumpable
/// `file:line:col:text`) with a `grep -rnHIE` fallback when rg isn't installed.
/// Pure — unit tested.
fn grep_command(pattern: &str, whole_word: bool) -> String {
    let q = shell_single_quote(pattern);
    let w = if whole_word { "-w " } else { "" };
    format!("rg --vimgrep --color=never {w}-e {q} 2>/dev/null || grep -rnHIE {w}-e {q} .")
}

/// The word (alphanumeric + `_`) at or immediately before char column `col` in
/// `line`, used by `:grep-word`. Pure — unit tested.
fn word_at_col(line: &str, col: usize) -> Option<String> {
    let chars: Vec<char> = line.chars().collect();
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let mut i = col.min(chars.len());
    if i >= chars.len() || !is_word(chars[i]) {
        if i > 0 && is_word(chars[i - 1]) {
            i -= 1;
        } else {
            return None;
        }
    }
    let mut start = i;
    while start > 0 && is_word(chars[start - 1]) {
        start -= 1;
    }
    let mut end = i;
    while end < chars.len() && is_word(chars[end]) {
        end += 1;
    }
    Some(chars[start..end].iter().collect())
}

/// Spawn `cmd` in the project root and route its output to the jumpable Run console.
fn spawn_into_run_console(cx: &mut compositor::Context, cmd: String) {
    let path = doc!(cx.editor).path().map(|p| p.to_path_buf());
    let (_default, cwd) = crate::ui::run::smart_command(path.as_deref());
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
}

/// `:grep <pattern>` — search the project and stream jumpable results into the
/// Run console.
fn grep(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let pattern = args.join(" ");
    if pattern.trim().is_empty() {
        bail!("usage: :grep <pattern>");
    }
    spawn_into_run_console(cx, grep_command(&pattern, false));
    Ok(())
}

/// `:shell-quote` — wrap the selection in safe shell single-quotes (for pasting a
/// path or string into a shell command). Reuses [`shell_single_quote`].
fn shell_quote_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let quoted = shell_single_quote(&s);
    if quoted == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(quoted.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Wrap `content` in `<tag>…</tag>`. Pure — unit tested.
fn wrap_in_tag(content: &str, tag: &str) -> String {
    format!("<{tag}>{content}</{tag}>")
}

/// `:wrap-tag <tag>` — wrap each selection in `<tag>…</tag>` (HTML/JSX/XML).
fn wrap_tag_cmd(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let tag = args.join("").trim().to_string();
    if tag.is_empty() {
        bail!("usage: :wrap-tag <tag>");
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let selection = doc.selection(view.id).clone();
    let transaction = Transaction::change_by_selection(text, &selection, |range| {
        let content: String = text
            .slice(..)
            .slice(range.from()..range.to())
            .chunks()
            .collect();
        (
            range.from(),
            range.to(),
            Some(wrap_in_tag(&content, &tag).into()),
        )
    });
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Extract the 1-based `col`-th field from each line of a delimited table (tab if
/// any tab is present, else comma), trimmed, one per line. Pure — unit tested.
fn csv_column(s: &str, col: usize) -> String {
    let delim = if s.contains('\t') { '\t' } else { ',' };
    s.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            l.split(delim)
                .nth(col.saturating_sub(1))
                .unwrap_or("")
                .trim()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `:csv-column <n>` — replace the selected CSV/TSV with just its Nth column.
fn csv_column_cmd(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let col: usize = args
        .first()
        .and_then(|a| a.trim().parse().ok())
        .filter(|n| *n >= 1)
        .context("usage: :csv-column <n> (1-based)")?;
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = csv_column(&s, col);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Wrap `content` in a fenced Markdown code block with optional `lang`. Trailing
/// newlines on the body are trimmed so the closing fence sits flush. Pure — unit tested.
fn code_fence(content: &str, lang: &str) -> String {
    let body = content.trim_end_matches('\n');
    format!("```{lang}\n{body}\n```")
}

fn code_fence_cmd(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let lang = args.first().map(|a| a.trim()).unwrap_or("");
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = code_fence(&s, lang);
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Pretty-print a GitHub-flavored Markdown pipe table: trims cells, pads every
/// column to its widest cell, and regenerates the `---` separator row to match
/// (honoring `:` alignment markers — left/right/center). Returns the input
/// unchanged if the second line isn't a separator row. Pure — unit tested.
fn format_md_table(s: &str) -> String {
    let lines: Vec<&str> = s.lines().filter(|l| l.contains('|')).collect();
    if lines.len() < 2 {
        return s.to_string();
    }
    let split_row = |line: &str| -> Vec<String> {
        let t = line.trim();
        let t = t.strip_prefix('|').unwrap_or(t);
        let t = t.strip_suffix('|').unwrap_or(t);
        t.split('|').map(|c| c.trim().to_string()).collect()
    };
    let rows: Vec<Vec<String>> = lines.iter().map(|l| split_row(l)).collect();
    let is_sep_cell = |c: &str| {
        let c = c.trim();
        c.contains('-') && c.chars().all(|ch| ch == '-' || ch == ':')
    };
    let sep_idx = 1;
    if !rows[sep_idx].iter().all(|c| is_sep_cell(c)) {
        return s.to_string();
    }
    let ncols = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    // Alignment per column: 0 = left, 1 = right, 2 = center.
    let mut align = vec![0u8; ncols];
    for (i, c) in rows[sep_idx].iter().enumerate() {
        let left = c.starts_with(':');
        let right = c.ends_with(':');
        align[i] = match (left, right) {
            (true, true) => 2,
            (false, true) => 1,
            _ => 0,
        };
    }
    // Column widths from content rows (skip the separator); min 3 for "---".
    let mut widths = vec![3usize; ncols];
    for (ri, row) in rows.iter().enumerate() {
        if ri == sep_idx {
            continue;
        }
        for (ci, cell) in row.iter().enumerate() {
            let len = cell.chars().count();
            if len > widths[ci] {
                widths[ci] = len;
            }
        }
    }
    let pad = |cell: &str, w: usize, a: u8| -> String {
        let total = w.saturating_sub(cell.chars().count());
        match a {
            1 => format!("{}{}", " ".repeat(total), cell),
            2 => {
                let l = total / 2;
                format!("{}{}{}", " ".repeat(l), cell, " ".repeat(total - l))
            }
            _ => format!("{}{}", cell, " ".repeat(total)),
        }
    };
    let mut out = Vec::new();
    for (ri, row) in rows.iter().enumerate() {
        let cells: Vec<String> = (0..ncols)
            .map(|ci| {
                if ri == sep_idx {
                    let w = widths[ci];
                    match align[ci] {
                        1 => format!("{}:", "-".repeat(w.saturating_sub(1))),
                        2 => format!(":{}:", "-".repeat(w.saturating_sub(2))),
                        _ => "-".repeat(w),
                    }
                } else {
                    pad(
                        row.get(ci).map(|s| s.as_str()).unwrap_or(""),
                        widths[ci],
                        align[ci],
                    )
                }
            })
            .collect();
        out.push(format!("| {} |", cells.join(" | ")));
    }
    out.join("\n")
}

fn md_table_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = format_md_table(&s);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// jq-lite: navigate `input` (a JSON document) by a dot-separated `path` and return
/// the addressed sub-value, pretty-printed. Object keys index by name, arrays by
/// 0-based integer. Empty path segments are ignored, so a leading `.` is allowed.
/// Pure — unit tested.
fn json_query(input: &str, path: &str) -> anyhow::Result<String> {
    let root: Value = serde_json::from_str(input.trim()).context("selection is not valid JSON")?;
    let mut cur = &root;
    for seg in path.split('.').filter(|s| !s.is_empty()) {
        cur = match cur {
            Value::Object(map) => map.get(seg).with_context(|| format!("no key '{seg}'"))?,
            Value::Array(arr) => {
                let idx: usize = seg
                    .parse()
                    .with_context(|| format!("'{seg}' is not an array index"))?;
                arr.get(idx)
                    .with_context(|| format!("index {idx} out of range (len {})", arr.len()))?
            }
            _ => anyhow::bail!("cannot descend into a scalar with '{seg}'"),
        };
    }
    Ok(serde_json::to_string_pretty(cur)?)
}

fn json_query_cmd(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let path = args.join(" ");
    let path = path.trim();
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = json_query(&s, path)?;
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Flatten a JSON document into one `path = value` line per leaf, where `path` is
/// the dot-path (arrays indexed by integer) and `value` is the leaf rendered as
/// compact JSON. Empty objects/arrays emit `path = {}` / `path = []`. The result
/// round-trips through [`json_query`]. Pure — unit tested.
fn json_flatten(input: &str) -> anyhow::Result<String> {
    fn walk(prefix: &str, v: &Value, out: &mut Vec<String>) {
        match v {
            Value::Object(map) if !map.is_empty() => {
                for (k, val) in map {
                    let p = if prefix.is_empty() {
                        k.clone()
                    } else {
                        format!("{prefix}.{k}")
                    };
                    walk(&p, val, out);
                }
            }
            Value::Array(arr) if !arr.is_empty() => {
                for (i, val) in arr.iter().enumerate() {
                    let p = if prefix.is_empty() {
                        i.to_string()
                    } else {
                        format!("{prefix}.{i}")
                    };
                    walk(&p, val, out);
                }
            }
            // scalars, plus empty {} / [] which Display renders as `{}` / `[]`
            leaf => out.push(format!("{prefix} = {leaf}")),
        }
    }
    let root: Value = serde_json::from_str(input.trim()).context("selection is not valid JSON")?;
    let mut out = Vec::new();
    walk("", &root, &mut out);
    Ok(out.join("\n"))
}

fn json_flatten_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = json_flatten(&s)?;
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Convert a JSON array of objects into CSV. The header row is the sorted union of
/// every object's keys; missing fields become empty cells; string values are
/// unquoted unless they need RFC-4180 escaping (comma, quote, newline). The inverse
/// of `:csv-to-json`. Pure — unit tested.
fn json_to_csv(input: &str) -> anyhow::Result<String> {
    let root: Value = serde_json::from_str(input.trim()).context("selection is not valid JSON")?;
    let arr = root
        .as_array()
        .context("expected a JSON array of objects")?;
    let mut headers: Vec<String> = Vec::new();
    for item in arr {
        let obj = item
            .as_object()
            .context("every array element must be a JSON object")?;
        for k in obj.keys() {
            if !headers.iter().any(|h| h == k) {
                headers.push(k.clone());
            }
        }
    }
    headers.sort();
    fn esc(s: &str) -> String {
        if s.contains([',', '"', '\n']) {
            format!("\"{}\"", s.replace('"', "\"\""))
        } else {
            s.to_string()
        }
    }
    fn cell(v: &Value) -> String {
        match v {
            Value::String(s) => s.clone(),
            Value::Null => String::new(),
            other => other.to_string(),
        }
    }
    let mut lines = vec![headers.iter().map(|h| esc(h)).collect::<Vec<_>>().join(",")];
    for item in arr {
        let obj = item.as_object().expect("validated above");
        let row: Vec<String> = headers
            .iter()
            .map(|h| obj.get(h).map(|v| esc(&cell(v))).unwrap_or_default())
            .collect();
        lines.push(row.join(","));
    }
    Ok(lines.join("\n"))
}

fn json_to_csv_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = json_to_csv(&s)?;
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Insert `val` into `node` at the dot-path `segs`, growing objects/arrays as
/// needed. A segment that parses as an integer addresses an array index; anything
/// else is an object key. Errors if an existing node has a conflicting type.
fn json_insert_path(node: &mut Value, segs: &[&str], val: Value) -> anyhow::Result<()> {
    let Some((seg, rest)) = segs.split_first() else {
        *node = val;
        return Ok(());
    };
    if let Ok(idx) = seg.parse::<usize>() {
        if node.is_null() {
            *node = Value::Array(Vec::new());
        }
        let arr = node.as_array_mut().context("expected an array")?;
        if arr.len() <= idx {
            arr.resize(idx + 1, Value::Null);
        }
        json_insert_path(&mut arr[idx], rest, val)
    } else {
        if node.is_null() {
            *node = Value::Object(serde_json::Map::new());
        }
        let obj = node.as_object_mut().context("expected an object")?;
        let entry = obj.entry(seg.to_string()).or_insert(Value::Null);
        json_insert_path(entry, rest, val)
    }
}

/// Rebuild nested JSON from `path = value` lines (the inverse of `:json-flatten`).
/// Each value is parsed as JSON; integer path segments become array indices, all
/// other segments object keys. Blank lines are skipped. Pure — unit tested.
fn json_unflatten(input: &str) -> anyhow::Result<String> {
    let mut root = Value::Null;
    for (i, raw) in input.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let (path, valstr) = line
            .split_once(" = ")
            .with_context(|| format!("line {}: expected `path = value`", i + 1))?;
        let val: Value = serde_json::from_str(valstr.trim())
            .with_context(|| format!("line {}: value is not valid JSON", i + 1))?;
        let segs: Vec<&str> = path.trim().split('.').filter(|s| !s.is_empty()).collect();
        json_insert_path(&mut root, &segs, val)
            .with_context(|| format!("line {}: conflicting path '{}'", i + 1, path.trim()))?;
    }
    Ok(serde_json::to_string_pretty(&root)?)
}

fn json_unflatten_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = json_unflatten(&s)?;
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Convert a TOML document to pretty-printed JSON. Pure — unit tested.
fn toml_to_json(input: &str) -> anyhow::Result<String> {
    let v: toml::Value = toml::from_str(input.trim()).context("selection is not valid TOML")?;
    Ok(serde_json::to_string_pretty(&v)?)
}

/// Convert a JSON document to pretty-printed TOML. Errors when the value has no
/// TOML representation (e.g. a `null`, or a non-table value at the top level).
/// The inverse of [`toml_to_json`]. Pure — unit tested.
fn json_to_toml(input: &str) -> anyhow::Result<String> {
    let v: Value = serde_json::from_str(input.trim()).context("selection is not valid JSON")?;
    toml::to_string_pretty(&v)
        .context("value has no TOML representation (null, or non-table at top level)")
}

fn toml_to_json_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = toml_to_json(&s)?;
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

fn json_to_toml_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = json_to_toml(&s)?;
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Type-rank for ordering JSON values of differing types (null < bool < number <
/// string < array < object), used as a fallback when two values aren't the same
/// scalar type.
fn json_type_rank(v: Option<&Value>) -> u8 {
    match v {
        Some(Value::Null) | None => 0,
        Some(Value::Bool(_)) => 1,
        Some(Value::Number(_)) => 2,
        Some(Value::String(_)) => 3,
        Some(Value::Array(_)) => 4,
        Some(Value::Object(_)) => 5,
    }
}

/// Compare two optional JSON values: numbers numerically, strings/bools naturally,
/// otherwise by type rank. Total and panic-free (NaN compares Equal).
fn json_cmp(a: Option<&Value>, b: Option<&Value>) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (a, b) {
        (Some(Value::Number(x)), Some(Value::Number(y))) => x
            .as_f64()
            .partial_cmp(&y.as_f64())
            .unwrap_or(Ordering::Equal),
        (Some(Value::String(x)), Some(Value::String(y))) => x.cmp(y),
        (Some(Value::Bool(x)), Some(Value::Bool(y))) => x.cmp(y),
        _ => json_type_rank(a).cmp(&json_type_rank(b)),
    }
}

/// Sort a JSON array in place and pretty-print it. With `key`, elements are
/// expected to be objects and are ordered by that field; without, the elements
/// themselves are compared. Stable sort. Pure — unit tested.
fn json_sort_array(input: &str, key: Option<&str>) -> anyhow::Result<String> {
    let mut root: Value =
        serde_json::from_str(input.trim()).context("selection is not valid JSON")?;
    let arr = root.as_array_mut().context("expected a JSON array")?;
    arr.sort_by(|a, b| match key {
        Some(k) => json_cmp(a.get(k), b.get(k)),
        None => json_cmp(Some(a), Some(b)),
    });
    Ok(serde_json::to_string_pretty(&root)?)
}

fn json_sort_cmd(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let key = args.join(" ");
    let key = key.trim();
    let key = (!key.is_empty()).then_some(key);
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = json_sort_array(&s, key)?;
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Project a JSON value to only the named `keys` (SQL `SELECT`-style). An object
/// keeps just those keys (in the requested order, skipping absent ones); an array
/// projects each element; other values pass through unchanged. Pure — unit tested.
fn json_pick(input: &str, keys: &[&str]) -> anyhow::Result<String> {
    fn pick(v: &Value, keys: &[&str]) -> Value {
        match v {
            Value::Object(map) => {
                let mut out = serde_json::Map::new();
                for k in keys {
                    if let Some(val) = map.get(*k) {
                        out.insert((*k).to_string(), val.clone());
                    }
                }
                Value::Object(out)
            }
            other => other.clone(),
        }
    }
    let root: Value = serde_json::from_str(input.trim()).context("selection is not valid JSON")?;
    let result = match &root {
        Value::Array(arr) => Value::Array(arr.iter().map(|v| pick(v, keys)).collect()),
        other => pick(other, keys),
    };
    Ok(serde_json::to_string_pretty(&result)?)
}

fn json_pick_cmd(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let mut keys: Vec<String> = Vec::new();
    for a in args {
        let t = a.trim();
        if !t.is_empty() {
            keys.push(t.to_string());
        }
    }
    if keys.is_empty() {
        anyhow::bail!("usage: :json-pick <key>...");
    }
    let key_refs: Vec<&str> = keys.iter().map(String::as_str).collect();
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = json_pick(&s, &key_refs)?;
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Drop the named `keys` from a JSON value (the inverse of [`json_pick`]). An
/// object keeps every key except those listed; an array applies this to each
/// element; other values pass through unchanged. Pure — unit tested.
fn json_omit(input: &str, keys: &[&str]) -> anyhow::Result<String> {
    fn omit(v: &Value, keys: &[&str]) -> Value {
        match v {
            Value::Object(map) => {
                let kept: serde_json::Map<String, Value> = map
                    .iter()
                    .filter(|(k, _)| !keys.iter().any(|drop| drop == k))
                    .map(|(k, val)| (k.clone(), val.clone()))
                    .collect();
                Value::Object(kept)
            }
            other => other.clone(),
        }
    }
    let root: Value = serde_json::from_str(input.trim()).context("selection is not valid JSON")?;
    let result = match &root {
        Value::Array(arr) => Value::Array(arr.iter().map(|v| omit(v, keys)).collect()),
        other => omit(other, keys),
    };
    Ok(serde_json::to_string_pretty(&result)?)
}

fn json_omit_cmd(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let mut keys: Vec<String> = Vec::new();
    for a in args {
        let t = a.trim();
        if !t.is_empty() {
            keys.push(t.to_string());
        }
    }
    if keys.is_empty() {
        anyhow::bail!("usage: :json-omit <key>...");
    }
    let key_refs: Vec<&str> = keys.iter().map(String::as_str).collect();
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = json_omit(&s, &key_refs)?;
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Remove duplicate elements from a JSON array, preserving the first occurrence of
/// each. Without `key`, identity is the element's canonical JSON; with `key`,
/// elements are deduplicated by that object field. Pure — unit tested.
fn json_unique(input: &str, key: Option<&str>) -> anyhow::Result<String> {
    let mut root: Value =
        serde_json::from_str(input.trim()).context("selection is not valid JSON")?;
    let arr = root.as_array_mut().context("expected a JSON array")?;
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut out: Vec<Value> = Vec::new();
    for item in arr.drain(..) {
        let id = match key {
            Some(k) => item.get(k).map(|v| v.to_string()).unwrap_or_default(),
            None => item.to_string(),
        };
        if seen.insert(id) {
            out.push(item);
        }
    }
    Ok(serde_json::to_string_pretty(&Value::Array(out))?)
}

fn json_unique_cmd(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let key = args.join(" ");
    let key = key.trim();
    let key = (!key.is_empty()).then_some(key);
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = json_unique(&s, key)?;
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Group a JSON array of objects by `key` into an object mapping each distinct
/// field value to the array of matching elements. The grouping label is the raw
/// string for string fields, the canonical JSON otherwise, and `null` when the
/// field is absent. Pure — unit tested.
fn json_group_by(input: &str, key: &str) -> anyhow::Result<String> {
    let root: Value = serde_json::from_str(input.trim()).context("selection is not valid JSON")?;
    let arr = root.as_array().context("expected a JSON array")?;
    let mut groups: serde_json::Map<String, Value> = serde_json::Map::new();
    for item in arr {
        let label = match item.get(key) {
            Some(Value::String(s)) => s.clone(),
            Some(v) => v.to_string(),
            None => "null".to_string(),
        };
        groups
            .entry(label)
            .or_insert_with(|| Value::Array(Vec::new()))
            .as_array_mut()
            .expect("group entries are always arrays")
            .push(item.clone());
    }
    Ok(serde_json::to_string_pretty(&Value::Object(groups))?)
}

fn json_group_by_cmd(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let key = args.join(" ");
    let key = key.trim();
    if key.is_empty() {
        anyhow::bail!("usage: :json-group-by <key>");
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = json_group_by(&s, key)?;
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Extract every regex match in `input`, one per line (like `grep -o`). If the
/// pattern has a capture group, the first group's text is emitted instead of the
/// whole match; matches where that group didn't participate are skipped. Pure —
/// unit tested.
fn extract_matches(input: &str, pattern: &str) -> anyhow::Result<String> {
    let re = regex::Regex::new(pattern).map_err(|e| anyhow!("invalid pattern: {e}"))?;
    let has_group = re.captures_len() > 1;
    let mut out = Vec::new();
    for caps in re.captures_iter(input) {
        if has_group {
            if let Some(m) = caps.get(1) {
                out.push(m.as_str().to_string());
            }
        } else if let Some(m) = caps.get(0) {
            out.push(m.as_str().to_string());
        }
    }
    Ok(out.join("\n"))
}

fn extract_cmd(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let pattern = args.join(" ");
    let pattern = pattern.trim();
    if pattern.is_empty() {
        anyhow::bail!("usage: :extract <regex>");
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = extract_matches(&s, pattern)?;
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Keep (or, when `keep` is false, drop) the lines of `input` that match `pattern`
/// — the in-buffer equivalent of `grep` / `grep -v`. Pure — unit tested.
fn filter_lines(input: &str, pattern: &str, keep: bool) -> anyhow::Result<String> {
    let re = regex::Regex::new(pattern).map_err(|e| anyhow!("invalid pattern: {e}"))?;
    let out: Vec<&str> = input
        .lines()
        .filter(|line| re.is_match(line) == keep)
        .collect();
    Ok(out.join("\n"))
}

fn filter_reject_impl(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
    keep: bool,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let pattern = args.join(" ");
    let pattern = pattern.trim();
    if pattern.is_empty() {
        anyhow::bail!("usage: :{} <regex>", if keep { "filter" } else { "reject" });
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = filter_lines(&s, pattern, keep)?;
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

fn filter_cmd(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    filter_reject_impl(cx, args, event, true)
}

fn reject_cmd(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    filter_reject_impl(cx, args, event, false)
}

/// Count regex matches in `input`: returns (total matches, number of lines with at
/// least one match). Pure — unit tested.
fn count_matches(input: &str, pattern: &str) -> anyhow::Result<(usize, usize)> {
    let re = regex::Regex::new(pattern).map_err(|e| anyhow!("invalid pattern: {e}"))?;
    let total = re.find_iter(input).count();
    let lines = input.lines().filter(|l| re.is_match(l)).count();
    Ok((total, lines))
}

fn count_matches_cmd(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let pattern = args.join(" ");
    let pattern = pattern.trim();
    if pattern.is_empty() {
        anyhow::bail!("usage: :count-matches <regex>");
    }
    let (view, doc) = current!(cx.editor);
    let sel = doc.selection(view.id).primary();
    let s: String = doc
        .text()
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let (total, lines) = count_matches(&s, pattern)?;
    cx.editor.set_status(format!(
        "{total} match(es) on {lines} line(s) for /{pattern}/"
    ));
    Ok(())
}

/// Collapse `input` to `count line` rows — the `sort | uniq -c | sort -rn` idiom.
/// Rows are ordered by descending count, ties broken by first appearance. Pure —
/// unit tested.
fn uniq_count(input: &str) -> String {
    let mut counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    let mut order: Vec<&str> = Vec::new();
    for line in input.lines() {
        let n = counts.entry(line).or_insert(0);
        if *n == 0 {
            order.push(line);
        }
        *n += 1;
    }
    let mut items: Vec<(&str, usize)> = order.iter().map(|l| (*l, counts[*l])).collect();
    // stable sort by count desc keeps first-seen order for equal counts
    items.sort_by_key(|&(_, c)| std::cmp::Reverse(c));
    items
        .iter()
        .map(|(l, c)| format!("{c} {l}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn uniq_count_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = uniq_count(&s);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Format a float compactly: integers without a decimal point, otherwise to 4
/// decimal places with trailing zeros trimmed.
fn fmt_stat(x: f64) -> String {
    if x.is_finite() && x == x.trunc() {
        format!("{}", x as i64)
    } else {
        let s = format!("{x:.4}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

/// Summary statistics over `nums`: (count, sum, mean, min, max). `None` for an
/// empty slice. Pure — unit tested.
fn number_stats(nums: &[f64]) -> Option<(usize, f64, f64, f64, f64)> {
    if nums.is_empty() {
        return None;
    }
    let n = nums.len();
    let sum: f64 = nums.iter().sum();
    let mean = sum / n as f64;
    let min = nums.iter().copied().fold(f64::INFINITY, f64::min);
    let max = nums.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    Some((n, sum, mean, min, max))
}

fn stats_cmd(cx: &mut compositor::Context, _args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let sel = doc.selection(view.id).primary();
    let s: String = doc
        .text()
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let nums = extract_numbers(&s);
    match number_stats(&nums) {
        Some((n, sum, mean, min, max)) => cx.editor.set_status(format!(
            "n={} sum={} mean={} min={} max={}",
            n,
            fmt_stat(sum),
            fmt_stat(mean),
            fmt_stat(min),
            fmt_stat(max),
        )),
        None => cx.editor.set_status("no numbers in selection".to_string()),
    }
    Ok(())
}

/// Generate the inclusive integer sequence from `start` toward `end` stepping by
/// `step`, one value per line. Errors on a zero step or a range exceeding a sanity
/// cap. An empty range (direction disagrees with sign of step) yields "". Pure —
/// unit tested.
fn gen_seq(start: i64, end: i64, step: i64) -> anyhow::Result<String> {
    if step == 0 {
        anyhow::bail!("step must be non-zero");
    }
    let mut out: Vec<String> = Vec::new();
    let mut v = start;
    while (step > 0 && v <= end) || (step < 0 && v >= end) {
        out.push(v.to_string());
        if out.len() > 1_000_000 {
            anyhow::bail!("range too large (over 1,000,000 items)");
        }
        v += step;
    }
    Ok(out.join("\n"))
}

fn seq_cmd(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let argv: Vec<String> = args.into_iter().map(|a| a.trim().to_string()).collect();
    let start: i64 = argv
        .first()
        .and_then(|s| s.parse().ok())
        .context("usage: :seq <start> <end> [step]")?;
    let end: i64 = argv
        .get(1)
        .and_then(|s| s.parse().ok())
        .context("usage: :seq <start> <end> [step]")?;
    let step: i64 = match argv.get(2) {
        Some(s) => s.parse().context("step must be an integer")?,
        None => {
            if end >= start {
                1
            } else {
                -1
            }
        }
    };
    let new = gen_seq(start, end, step)?;
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Keep only the `n`th whitespace-delimited field (1-based) of each line, like
/// `awk '{print $n}'`. Runs of whitespace collapse; lines without an `n`th field
/// become empty. Pure — unit tested.
fn cut_field(input: &str, n: usize) -> String {
    let idx = n.saturating_sub(1);
    input
        .lines()
        .map(|line| line.split_whitespace().nth(idx).unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

fn field_cmd(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let n: usize = args
        .first()
        .and_then(|a| a.trim().parse().ok())
        .filter(|n| *n >= 1)
        .context("usage: :field <n> (1-based)")?;
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = cut_field(&s, n);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Replace each numeric line with the running (cumulative) total so far. Lines
/// that don't parse as a number pass through unchanged and don't affect the
/// accumulator. Pure — unit tested.
fn running_total(input: &str) -> String {
    let mut acc = 0.0_f64;
    input
        .lines()
        .map(|line| match line.trim().parse::<f64>() {
            Ok(n) => {
                acc += n;
                fmt_stat(acc)
            }
            Err(_) => line.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn running_total_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = running_total(&s);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Replace each numeric line with its difference from the previous numeric line
/// (the inverse of [`running_total`]). The first numeric value is kept as-is;
/// non-numeric lines pass through and don't update the reference. Pure — unit tested.
fn diff_lines(input: &str) -> String {
    let mut prev: Option<f64> = None;
    input
        .lines()
        .map(|line| match line.trim().parse::<f64>() {
            Ok(n) => {
                let out = match prev {
                    Some(p) => fmt_stat(n - p),
                    None => fmt_stat(n),
                };
                prev = Some(n);
                out
            }
            Err(_) => line.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn diff_lines_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = diff_lines(&s);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Sum the `n`th whitespace field (1-based) across every line, skipping lines
/// whose field is missing or non-numeric. Returns (sum, count of summed values).
/// Pure — unit tested.
fn sum_column(input: &str, n: usize) -> (f64, usize) {
    let idx = n.saturating_sub(1);
    let mut sum = 0.0_f64;
    let mut count = 0usize;
    for line in input.lines() {
        if let Some(Ok(v)) = line.split_whitespace().nth(idx).map(str::parse::<f64>) {
            sum += v;
            count += 1;
        }
    }
    (sum, count)
}

fn sum_column_cmd(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let n: usize = args
        .first()
        .and_then(|a| a.trim().parse().ok())
        .filter(|n| *n >= 1)
        .context("usage: :sum-column <n> (1-based)")?;
    let (view, doc) = current!(cx.editor);
    let sel = doc.selection(view.id).primary();
    let s: String = doc
        .text()
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let (sum, count) = sum_column(&s, n);
    cx.editor.set_status(format!(
        "sum of column {n} = {} (over {count} value(s))",
        fmt_stat(sum)
    ));
    Ok(())
}

/// Randomly reorder the lines of `input` using a Fisher-Yates shuffle driven by a
/// seeded xorshift64 RNG. Deterministic for a given `seed` (so it is unit-testable);
/// the command seeds from the wall clock. Pure — unit tested.
fn shuffle_lines(input: &str, seed: u64) -> String {
    let mut lines: Vec<&str> = input.lines().collect();
    let mut state = seed | 1; // avoid the all-zero state
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    for i in (1..lines.len()).rev() {
        let j = (next() % (i as u64 + 1)) as usize;
        lines.swap(i, j);
    }
    lines.join("\n")
}

fn shuffle_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x9E37_79B9_7F4A_7C15);
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = shuffle_lines(&s, seed);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Keep `n` randomly chosen lines from `input`, preserving their original relative
/// order (so the result is a subsequence). Returns the input unchanged when `n` is
/// at least the line count. Seeded for determinism. Pure — unit tested.
fn sample_lines(input: &str, n: usize, seed: u64) -> String {
    let lines: Vec<&str> = input.lines().collect();
    if n >= lines.len() {
        return input.to_string();
    }
    let mut idx: Vec<usize> = (0..lines.len()).collect();
    let mut state = seed | 1;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    // partial Fisher-Yates: pick the first `n` of a shuffle
    for i in 0..n {
        let j = i + (next() as usize) % (lines.len() - i);
        idx.swap(i, j);
    }
    let mut chosen: Vec<usize> = idx[..n].to_vec();
    chosen.sort_unstable();
    chosen
        .iter()
        .map(|&i| lines[i])
        .collect::<Vec<_>>()
        .join("\n")
}

fn sample_cmd(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let n: usize = args
        .first()
        .and_then(|a| a.trim().parse().ok())
        .filter(|n| *n >= 1)
        .context("usage: :sample <n> (number of lines to keep)")?;
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x9E37_79B9_7F4A_7C15);
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = sample_lines(&s, n, seed);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Convert JSONL/NDJSON (one JSON value per line) into a pretty JSON array. Blank
/// lines are skipped. Pure — unit tested.
fn jsonl_to_json(input: &str) -> anyhow::Result<String> {
    let mut arr: Vec<Value> = Vec::new();
    for (i, line) in input.lines().enumerate() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        let v: Value =
            serde_json::from_str(t).with_context(|| format!("line {}: invalid JSON", i + 1))?;
        arr.push(v);
    }
    Ok(serde_json::to_string_pretty(&Value::Array(arr))?)
}

/// Convert a JSON array into JSONL: each element serialized compactly on its own
/// line. The inverse of [`jsonl_to_json`]. Pure — unit tested.
fn json_to_jsonl(input: &str) -> anyhow::Result<String> {
    let root: Value = serde_json::from_str(input.trim()).context("selection is not valid JSON")?;
    let arr = root.as_array().context("expected a JSON array")?;
    let lines = arr
        .iter()
        .map(serde_json::to_string)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(lines.join("\n"))
}

fn jsonl_to_json_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = jsonl_to_json(&s)?;
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

fn json_to_jsonl_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = json_to_jsonl(&s)?;
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Keep the first `n` lines of `input` (like `head -n`). Pure — unit tested.
fn head_lines(input: &str, n: usize) -> String {
    input.lines().take(n).collect::<Vec<_>>().join("\n")
}

/// Keep the last `n` lines of `input` (like `tail -n`). Pure — unit tested.
fn tail_lines(input: &str, n: usize) -> String {
    let lines: Vec<&str> = input.lines().collect();
    let start = lines.len().saturating_sub(n);
    lines[start..].join("\n")
}

fn head_tail_impl(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
    head: bool,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let n: usize = args
        .first()
        .and_then(|a| a.trim().parse().ok())
        .filter(|n| *n >= 1)
        .with_context(|| format!("usage: :{} <n>", if head { "head" } else { "tail" }))?;
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = if head {
        head_lines(&s, n)
    } else {
        tail_lines(&s, n)
    };
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

fn head_cmd(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    head_tail_impl(cx, args, event, true)
}

fn tail_cmd(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    head_tail_impl(cx, args, event, false)
}

/// Reverse the characters of each line independently (Unix `rev`), preserving line
/// order. Distinct from reversing the whole selection or the line order. Operates
/// on Unicode scalar values. Pure — unit tested.
fn rev_each_line(input: &str) -> String {
    input
        .lines()
        .map(|l| l.chars().rev().collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

fn rev_cmd(cx: &mut compositor::Context, _args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = rev_each_line(&s);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Render a JSON array of objects as a column-aligned plain-text table for viewing
/// in-buffer. Header row is the sorted union of keys; each column is left-padded to
/// its widest cell; a `---` rule separates the header. Scalars render bare, strings
/// unquoted, null/missing as empty. Pure — unit tested.
fn json_to_table(input: &str) -> anyhow::Result<String> {
    let root: Value = serde_json::from_str(input.trim()).context("selection is not valid JSON")?;
    let arr = root
        .as_array()
        .context("expected a JSON array of objects")?;
    let mut headers: Vec<String> = Vec::new();
    for item in arr {
        let obj = item
            .as_object()
            .context("every array element must be a JSON object")?;
        for k in obj.keys() {
            if !headers.iter().any(|h| h == k) {
                headers.push(k.clone());
            }
        }
    }
    headers.sort();
    fn cell(v: &Value) -> String {
        match v {
            Value::String(s) => s.clone(),
            Value::Null => String::new(),
            other => other.to_string(),
        }
    }
    let rows: Vec<Vec<String>> = arr
        .iter()
        .map(|item| {
            let obj = item.as_object().expect("validated above");
            headers
                .iter()
                .map(|h| obj.get(h).map(cell).unwrap_or_default())
                .collect()
        })
        .collect();
    let mut widths: Vec<usize> = headers.iter().map(|h| h.chars().count()).collect();
    for row in &rows {
        for (i, c) in row.iter().enumerate() {
            widths[i] = widths[i].max(c.chars().count());
        }
    }
    let fmt_row = |cells: &[String]| {
        cells
            .iter()
            .enumerate()
            .map(|(i, c)| format!("{:<width$}", c, width = widths[i]))
            .collect::<Vec<_>>()
            .join("  ")
            .trim_end()
            .to_string()
    };
    let sep: Vec<String> = widths.iter().map(|w| "-".repeat(*w)).collect();
    let mut out = vec![fmt_row(&headers), fmt_row(&sep)];
    for row in &rows {
        out.push(fmt_row(row));
    }
    Ok(out.join("\n"))
}

fn json_table_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = json_to_table(&s)?;
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Render `input`'s UTF-8 bytes as an `xxd`-style hex dump: an 8-digit offset, 16
/// space-separated hex bytes per row (short rows padded so the gutter aligns), then
/// the ASCII gutter with non-printables shown as `.`. Pure — unit tested.
fn hexdump(input: &str) -> String {
    let bytes = input.as_bytes();
    if bytes.is_empty() {
        return String::new();
    }
    let mut out = Vec::new();
    for (i, chunk) in bytes.chunks(16).enumerate() {
        let mut hex = String::new();
        for j in 0..16 {
            match chunk.get(j) {
                Some(b) => hex.push_str(&format!("{b:02x} ")),
                None => hex.push_str("   "),
            }
        }
        let ascii: String = chunk
            .iter()
            .map(|&b| {
                if (0x20..0x7f).contains(&b) {
                    b as char
                } else {
                    '.'
                }
            })
            .collect();
        out.push(format!("{:08x}  {hex}|{ascii}|", i * 16));
    }
    out.join("\n")
}

fn hexdump_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = hexdump(&s);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Remove all duplicate lines globally, keeping the first occurrence and the
/// original order (unlike adjacent-only dedup, this needs no pre-sorting). Pure —
/// unit tested.
fn dedup_all_lines(input: &str) -> String {
    let mut seen = std::collections::HashSet::new();
    input
        .lines()
        .filter(|l| seen.insert(l.to_string()))
        .collect::<Vec<_>>()
        .join("\n")
}

fn dedup_all_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = dedup_all_lines(&s);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Caesar-shift the ASCII letters of `input` by `shift` (wrapping within each case,
/// non-letters untouched). Negative and large shifts are normalized mod 26, so this
/// generalizes ROT13 (`shift == 13`). Pure — unit tested.
fn caesar(input: &str, shift: i32) -> String {
    let s = shift.rem_euclid(26) as u8;
    input
        .chars()
        .map(|c| {
            if c.is_ascii_uppercase() {
                (((c as u8 - b'A' + s) % 26) + b'A') as char
            } else if c.is_ascii_lowercase() {
                (((c as u8 - b'a' + s) % 26) + b'a') as char
            } else {
                c
            }
        })
        .collect()
}

fn caesar_cmd(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let shift: i32 = args
        .first()
        .and_then(|a| a.trim().parse().ok())
        .context("usage: :caesar <n> (letter shift; n may be negative)")?;
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = caesar(&s, shift);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

const BASE32_ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

/// Base32-encode `input`'s UTF-8 bytes per RFC 4648 (standard alphabet, `=`
/// padding). Pure — unit tested.
fn base32_encode(input: &str) -> String {
    let mut out = String::new();
    let mut bits = 0u32;
    let mut nbits = 0u32;
    for &byte in input.as_bytes() {
        bits = (bits << 8) | byte as u32;
        nbits += 8;
        while nbits >= 5 {
            nbits -= 5;
            out.push(BASE32_ALPHABET[((bits >> nbits) & 0x1f) as usize] as char);
        }
    }
    if nbits > 0 {
        out.push(BASE32_ALPHABET[((bits << (5 - nbits)) & 0x1f) as usize] as char);
    }
    // pad to a multiple of 8 characters
    while !out.len().is_multiple_of(8) {
        out.push('=');
    }
    out
}

/// Base32-decode `input` per RFC 4648, ignoring padding and ASCII whitespace and
/// accepting either letter case. Errors on invalid characters or non-UTF-8 output.
/// The inverse of [`base32_encode`]. Pure — unit tested.
fn base32_decode(input: &str) -> anyhow::Result<String> {
    let mut bits = 0u32;
    let mut nbits = 0u32;
    let mut out: Vec<u8> = Vec::new();
    for b in input.bytes() {
        if b == b'=' || b.is_ascii_whitespace() {
            continue;
        }
        let v = BASE32_ALPHABET
            .iter()
            .position(|&x| x == b.to_ascii_uppercase())
            .with_context(|| format!("invalid base32 character '{}'", b as char))?
            as u32;
        bits = (bits << 5) | v;
        nbits += 5;
        if nbits >= 8 {
            nbits -= 8;
            out.push((bits >> nbits) as u8);
        }
    }
    String::from_utf8(out).context("decoded bytes are not valid UTF-8")
}

fn base32_encode_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = base32_encode(&s);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

fn base32_decode_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = base32_decode(&s)?;
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Compute the IEEE CRC32 checksum of `input`'s UTF-8 bytes (reflected, poly
/// 0xEDB88320, init/final 0xFFFFFFFF) — the zlib/PNG variant. Pure — unit tested.
fn crc32(input: &str) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &byte in input.as_bytes() {
        crc ^= byte as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

fn crc32_cmd(cx: &mut compositor::Context, _args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let sel = doc.selection(view.id).primary();
    let s: String = doc
        .text()
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let crc = crc32(&s);
    cx.editor.set_status(format!("CRC32: {crc:08x} ({crc})"));
    Ok(())
}

/// ROT47: rotate every printable-ASCII character (`!`..=`~`, 0x21–0x7e) by 47
/// within that 94-glyph range, leaving spaces and other bytes alone. Self-inverse.
/// Pure — unit tested.
fn rot47(input: &str) -> String {
    input
        .chars()
        .map(|c| {
            let b = c as u32;
            if (33..=126).contains(&b) {
                char::from_u32(33 + (b - 33 + 47) % 94).unwrap_or(c)
            } else {
                c
            }
        })
        .collect()
}

fn rot47_cmd(cx: &mut compositor::Context, _args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = rot47(&s);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// International Morse code for A–Z and 0–9.
const MORSE: &[(char, &str)] = &[
    ('A', ".-"),
    ('B', "-..."),
    ('C', "-.-."),
    ('D', "-.."),
    ('E', "."),
    ('F', "..-."),
    ('G', "--."),
    ('H', "...."),
    ('I', ".."),
    ('J', ".---"),
    ('K', "-.-"),
    ('L', ".-.."),
    ('M', "--"),
    ('N', "-."),
    ('O', "---"),
    ('P', ".--."),
    ('Q', "--.-"),
    ('R', ".-."),
    ('S', "..."),
    ('T', "-"),
    ('U', "..-"),
    ('V', "...-"),
    ('W', ".--"),
    ('X', "-..-"),
    ('Y', "-.--"),
    ('Z', "--.."),
    ('0', "-----"),
    ('1', ".----"),
    ('2', "..---"),
    ('3', "...--"),
    ('4', "....-"),
    ('5', "....."),
    ('6', "-...."),
    ('7', "--..."),
    ('8', "---.."),
    ('9', "----."),
];

/// Encode text to Morse: letters separated by spaces, words by ` / `. Unknown
/// characters are dropped; case is ignored. Pure — unit tested.
fn morse_encode(input: &str) -> String {
    input
        .split_whitespace()
        .map(|word| {
            word.chars()
                .filter_map(|c| {
                    let uc = c.to_ascii_uppercase();
                    MORSE
                        .iter()
                        .find(|(ch, _)| *ch == uc)
                        .map(|(_, code)| *code)
                })
                .collect::<Vec<_>>()
                .join(" ")
        })
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" / ")
}

/// Decode Morse (letters space-separated, words ` / `-separated) back to uppercase
/// text. Unknown codes are dropped. The inverse of [`morse_encode`] for A–Z/0–9.
/// Pure — unit tested.
fn morse_decode(input: &str) -> String {
    input
        .split(" / ")
        .map(|word| {
            word.split_whitespace()
                .filter_map(|code| MORSE.iter().find(|(_, c)| *c == code).map(|(ch, _)| *ch))
                .collect::<String>()
        })
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn morse_encode_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = morse_encode(&s);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

fn morse_decode_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = morse_decode(&s);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Format a byte count as a human-readable binary size (e.g. 1536 → "1.5 KiB").
/// Whole bytes show no decimal; larger units use one decimal place. Pure — unit tested.
fn human_bytes(n: f64) -> String {
    const UNITS: &[&str] = &["B", "KiB", "MiB", "GiB", "TiB", "PiB"];
    let sign = if n < 0.0 { "-" } else { "" };
    let mut v = n.abs();
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{sign}{} B", v as u64)
    } else {
        format!("{sign}{v:.1} {}", UNITS[i])
    }
}

/// Replace each line that is a bare number (byte count) with its human-readable
/// size; non-numeric lines pass through. Pure — unit tested.
fn humanize_lines(input: &str) -> String {
    input
        .lines()
        .map(|line| match line.trim().parse::<f64>() {
            Ok(n) => human_bytes(n),
            Err(_) => line.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn human_bytes_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = humanize_lines(&s);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// English ordinal suffix for `n` ("st"/"nd"/"rd"/"th"), with the 11–13 exception.
/// Pure — unit tested.
fn ordinal_suffix(n: i64) -> &'static str {
    let abs = (n % 100).abs();
    if (11..=13).contains(&abs) {
        "th"
    } else {
        match abs % 10 {
            1 => "st",
            2 => "nd",
            3 => "rd",
            _ => "th",
        }
    }
}

/// Replace each line that is a bare integer with its ordinal (e.g. 22 → "22nd");
/// non-integer lines pass through. Pure — unit tested.
fn ordinalize_lines(input: &str) -> String {
    input
        .lines()
        .map(|line| match line.trim().parse::<i64>() {
            Ok(n) => format!("{n}{}", ordinal_suffix(n)),
            Err(_) => line.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn ordinal_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = ordinalize_lines(&s);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Split an identifier into lowercased words, recognizing snake_case, kebab-case,
/// space, and camelCase/PascalCase boundaries (including acronym → word, e.g.
/// "HTTPSConnection" → ["https","connection"]). Pure — unit tested.
fn split_identifier_words(s: &str) -> Vec<String> {
    let chars: Vec<char> = s.chars().collect();
    let mut words: Vec<String> = Vec::new();
    let mut cur = String::new();
    for (i, &c) in chars.iter().enumerate() {
        if c == '_' || c == '-' || c == ' ' {
            if !cur.is_empty() {
                words.push(std::mem::take(&mut cur));
            }
            continue;
        }
        if c.is_uppercase() && !cur.is_empty() {
            let prev = chars[i - 1];
            let next_lower = chars.get(i + 1).is_some_and(|n| n.is_lowercase());
            if prev.is_lowercase() || prev.is_ascii_digit() || (prev.is_uppercase() && next_lower) {
                words.push(std::mem::take(&mut cur));
            }
        }
        cur.extend(c.to_lowercase());
    }
    if !cur.is_empty() {
        words.push(cur);
    }
    words
}

/// Uppercase the first character of `w`, leaving the rest unchanged.
fn ucfirst(w: &str) -> String {
    let mut it = w.chars();
    match it.next() {
        Some(f) => f.to_uppercase().collect::<String>() + it.as_str(),
        None => String::new(),
    }
}

fn to_snake(s: &str) -> String {
    split_identifier_words(s).join("_")
}

fn to_kebab(s: &str) -> String {
    split_identifier_words(s).join("-")
}

fn to_camel(s: &str) -> String {
    split_identifier_words(s)
        .iter()
        .enumerate()
        .map(|(i, w)| if i == 0 { w.clone() } else { ucfirst(w) })
        .collect()
}

fn to_pascal(s: &str) -> String {
    split_identifier_words(s)
        .iter()
        .map(|w| ucfirst(w))
        .collect()
}

fn to_constant(s: &str) -> String {
    split_identifier_words(s)
        .iter()
        .map(|w| w.to_uppercase())
        .collect::<Vec<_>>()
        .join("_")
}

fn convert_case_impl(
    cx: &mut compositor::Context,
    event: PromptEvent,
    f: fn(&str) -> String,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = f(&s);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

fn to_snake_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    convert_case_impl(cx, event, to_snake)
}

fn to_kebab_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    convert_case_impl(cx, event, to_kebab)
}

fn to_camel_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    convert_case_impl(cx, event, to_camel)
}

fn to_pascal_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    convert_case_impl(cx, event, to_pascal)
}

fn to_constant_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    convert_case_impl(cx, event, to_constant)
}

/// Render each UTF-8 byte of `input` as an 8-bit binary group, space-separated.
/// Pure — unit tested.
fn to_binary(input: &str) -> String {
    input
        .bytes()
        .map(|b| format!("{b:08b}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Parse whitespace-separated binary byte groups back into text. Errors on invalid
/// digits or non-UTF-8 output. The inverse of [`to_binary`]. Pure — unit tested.
fn from_binary(input: &str) -> anyhow::Result<String> {
    let bytes = input
        .split_whitespace()
        .map(|tok| u8::from_str_radix(tok, 2))
        .collect::<Result<Vec<u8>, _>>()
        .context("invalid binary (expected space-separated 8-bit groups)")?;
    String::from_utf8(bytes).context("decoded bytes are not valid UTF-8")
}

fn to_binary_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = to_binary(&s);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

fn from_binary_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = from_binary(&s)?;
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Compare two strings in "natural" order: contiguous digit runs are compared by
/// numeric value (leading zeros ignored, overflow-safe via length+lex), other
/// characters by code point. So "file2" < "file10". Pure — unit tested.
fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let mut ai = a.chars().peekable();
    let mut bi = b.chars().peekable();
    loop {
        match (ai.peek().copied(), bi.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(ca), Some(cb)) if ca.is_ascii_digit() && cb.is_ascii_digit() => {
                let mut na = String::new();
                while ai.peek().is_some_and(|c| c.is_ascii_digit()) {
                    na.push(ai.next().unwrap());
                }
                let mut nb = String::new();
                while bi.peek().is_some_and(|c| c.is_ascii_digit()) {
                    nb.push(bi.next().unwrap());
                }
                let va = na.trim_start_matches('0');
                let vb = nb.trim_start_matches('0');
                let ord = va
                    .len()
                    .cmp(&vb.len())
                    .then_with(|| va.cmp(vb))
                    .then_with(|| na.len().cmp(&nb.len()));
                if ord != Ordering::Equal {
                    return ord;
                }
            }
            (Some(ca), Some(cb)) => {
                if ca != cb {
                    return ca.cmp(&cb);
                }
                ai.next();
                bi.next();
            }
        }
    }
}

/// Sort the lines of `input` in natural order (see [`natural_cmp`]). Stable. Pure —
/// unit tested.
fn natural_sort_lines(input: &str) -> String {
    let mut lines: Vec<&str> = input.lines().collect();
    lines.sort_by(|a, b| natural_cmp(a, b));
    lines.join("\n")
}

fn natural_sort_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = natural_sort_lines(&s);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Pad each line to a minimum `width` (measured in characters). When `left` is
/// true the text is left-justified (padding on the right); otherwise it is
/// right-justified. Lines already at least `width` wide are untouched. Pure —
/// unit tested.
fn pad_lines(input: &str, width: usize, left: bool) -> String {
    input
        .lines()
        .map(|line| {
            let len = line.chars().count();
            if len >= width {
                line.to_string()
            } else {
                let pad = " ".repeat(width - len);
                if left {
                    format!("{line}{pad}")
                } else {
                    format!("{pad}{line}")
                }
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn pad_lines_impl(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
    left: bool,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let width: usize = args
        .first()
        .and_then(|a| a.trim().parse().ok())
        .with_context(|| {
            format!(
                "usage: :{} <width>",
                if left { "pad-right" } else { "pad-left" }
            )
        })?;
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = pad_lines(&s, width, left);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

fn pad_right_cmd(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    pad_lines_impl(cx, args, event, true)
}

fn pad_left_cmd(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    pad_lines_impl(cx, args, event, false)
}

/// Append the not-yet-seen keys of `map` to `keys`, preserving order.
fn push_keys(map: &serde_json::Map<String, Value>, keys: &mut Vec<String>) {
    for k in map.keys() {
        if !keys.iter().any(|e| e == k) {
            keys.push(k.clone());
        }
    }
}

/// List the keys of a JSON object — or the union of keys across a JSON array of
/// objects — one per line. Errors on a scalar value. Pure — unit tested.
fn json_keys(input: &str) -> anyhow::Result<String> {
    let root: Value = serde_json::from_str(input.trim()).context("selection is not valid JSON")?;
    let mut keys: Vec<String> = Vec::new();
    match &root {
        Value::Object(map) => push_keys(map, &mut keys),
        Value::Array(arr) => {
            for item in arr {
                if let Value::Object(map) = item {
                    push_keys(map, &mut keys);
                }
            }
        }
        _ => anyhow::bail!("expected a JSON object or array of objects"),
    }
    Ok(keys.join("\n"))
}

fn json_keys_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = json_keys(&s)?;
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Describe the top-level JSON type and size of `input` (e.g. "object (3 keys)",
/// "array (5 elements)"). Errors on invalid JSON. Pure — unit tested.
fn json_describe(input: &str) -> anyhow::Result<String> {
    let v: Value = serde_json::from_str(input.trim()).context("selection is not valid JSON")?;
    Ok(match &v {
        Value::Null => "null".to_string(),
        Value::Bool(b) => format!("boolean ({b})"),
        Value::Number(n) => format!("number ({n})"),
        Value::String(s) => format!("string (len {})", s.chars().count()),
        Value::Array(a) => format!("array ({} elements)", a.len()),
        Value::Object(o) => format!("object ({} keys)", o.len()),
    })
}

fn json_type_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let sel = doc.selection(view.id).primary();
    let s: String = doc
        .text()
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let desc = json_describe(&s)?;
    cx.editor.set_status(format!("JSON: {desc}"));
    Ok(())
}

/// For each line, keep the text after (or, when `keep_after` is false, before) the
/// first occurrence of `delim`. Lines without the delimiter are kept whole. Pure —
/// unit tested.
fn cut_lines(input: &str, delim: &str, keep_after: bool) -> String {
    input
        .lines()
        .map(|line| match line.split_once(delim) {
            Some((before, after)) => {
                if keep_after {
                    after
                } else {
                    before
                }
            }
            None => line,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn cut_lines_impl(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
    keep_after: bool,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let delim = args.join(" ");
    let delim = delim.trim();
    if delim.is_empty() {
        anyhow::bail!(
            "usage: :{} <delimiter>",
            if keep_after { "after" } else { "before" }
        );
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = cut_lines(&s, delim, keep_after);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

fn after_cmd(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    cut_lines_impl(cx, args, event, true)
}

fn before_cmd(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    cut_lines_impl(cx, args, event, false)
}

/// Invert the case of every character: uppercase ↔ lowercase, others unchanged.
/// Self-inverse. Pure — unit tested.
fn swapcase(input: &str) -> String {
    input
        .chars()
        .flat_map(|c| {
            if c.is_uppercase() {
                c.to_lowercase().collect::<Vec<_>>()
            } else if c.is_lowercase() {
                c.to_uppercase().collect::<Vec<_>>()
            } else {
                vec![c]
            }
        })
        .collect()
}

fn swapcase_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = swapcase(&s);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Strip zero-width / invisible Unicode characters often hidden in pasted text:
/// ZWSP, ZWNJ, ZWJ, word joiner, BOM/ZWNBSP, and the soft hyphen. Pure — unit tested.
fn strip_zero_width(input: &str) -> String {
    input
        .chars()
        .filter(|&c| {
            !matches!(
                c,
                '\u{200B}' | '\u{200C}' | '\u{200D}' | '\u{2060}' | '\u{FEFF}' | '\u{00AD}'
            )
        })
        .collect()
}

fn strip_invisible_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = strip_zero_width(&s);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Turn each line of `input` into a JSON string element of a pretty array. Pure —
/// unit tested.
fn lines_to_json_array(input: &str) -> String {
    let arr: Vec<Value> = input
        .lines()
        .map(|l| Value::String(l.to_string()))
        .collect();
    serde_json::to_string_pretty(&Value::Array(arr)).unwrap_or_else(|_| "[]".to_string())
}

/// Turn a JSON array into lines: string elements unquoted, others rendered as
/// compact JSON. The inverse of [`lines_to_json_array`] for string arrays. Pure —
/// unit tested.
fn json_array_to_lines(input: &str) -> anyhow::Result<String> {
    let root: Value = serde_json::from_str(input.trim()).context("selection is not valid JSON")?;
    let arr = root.as_array().context("expected a JSON array")?;
    let lines: Vec<String> = arr
        .iter()
        .map(|v| match v {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        })
        .collect();
    Ok(lines.join("\n"))
}

fn lines_to_json_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = lines_to_json_array(&s);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

fn json_to_lines_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = json_array_to_lines(&s)?;
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Turn each non-blank line into a Markdown task-list item (`- [ ] item`),
/// stripping any leading bullet so existing bullet lists convert cleanly. Pure —
/// unit tested.
fn checkbox_list(input: &str) -> String {
    input
        .lines()
        .map(|l| {
            if l.trim().is_empty() {
                l.to_string()
            } else {
                let content = l.trim_start();
                let content = content.strip_prefix("- ").unwrap_or(content);
                format!("- [ ] {content}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn checkbox_list_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = checkbox_list(&s);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Join hard-wrapped lines: consecutive non-blank lines (trimmed) are joined with a
/// single space, and blank lines are preserved as paragraph breaks. The inverse of
/// hard-wrapping. Pure — unit tested.
fn unwrap_paragraphs(input: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut para: Vec<&str> = Vec::new();
    for line in input.lines() {
        if line.trim().is_empty() {
            if !para.is_empty() {
                out.push(para.join(" "));
                para.clear();
            }
            out.push(String::new());
        } else {
            para.push(line.trim());
        }
    }
    if !para.is_empty() {
        out.push(para.join(" "));
    }
    out.join("\n")
}

fn unwrap_paragraphs_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = unwrap_paragraphs(&s);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Build a SQL `IN`-list — `('a', 'b', 'c')` — from the non-blank selected lines,
/// single-quoting each (doubling any embedded `'`). Pure — unit tested.
fn sql_in_list(input: &str) -> String {
    let items: Vec<String> = input
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(|l| format!("'{}'", l.replace('\'', "''")))
        .collect();
    format!("({})", items.join(", "))
}

fn sql_in_cmd(cx: &mut compositor::Context, _args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = sql_in_list(&s);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Convert each line that is a decimal integer to lowercase hex (sign preserved);
/// non-numeric lines pass through. Pure — unit tested.
fn dec_to_hex_lines(input: &str) -> String {
    input
        .lines()
        .map(|l| match l.trim().parse::<i64>() {
            Ok(n) if n < 0 => format!("-{:x}", -n),
            Ok(n) => format!("{n:x}"),
            Err(_) => l.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Convert each line that is a hex integer (optional `0x`/`-` prefix) to decimal;
/// non-hex lines pass through. The inverse of [`dec_to_hex_lines`]. Pure — unit tested.
fn hex_to_dec_lines(input: &str) -> String {
    input
        .lines()
        .map(|l| {
            let t = l.trim();
            let (sign, digits) = match t.strip_prefix('-') {
                Some(rest) => (-1i64, rest),
                None => (1, t),
            };
            let digits = digits
                .strip_prefix("0x")
                .or_else(|| digits.strip_prefix("0X"))
                .unwrap_or(digits);
            match i64::from_str_radix(digits, 16) {
                Ok(n) => (sign * n).to_string(),
                Err(_) => l.to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn dec_to_hex_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = dec_to_hex_lines(&s);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

fn hex_to_dec_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = hex_to_dec_lines(&s);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Replace each non-ASCII character with a `\u{XXXX}` escape; ASCII passes through.
/// Pure — unit tested.
fn unicode_escape(input: &str) -> String {
    input
        .chars()
        .map(|c| {
            if c.is_ascii() {
                c.to_string()
            } else {
                format!("\\u{{{:x}}}", c as u32)
            }
        })
        .collect()
}

/// Decode `\u{XXXX}` and 4-digit `\uXXXX` escapes back to characters; everything
/// else is copied verbatim. The inverse of [`unicode_escape`]. Pure — unit tested.
fn unicode_unescape(input: &str) -> String {
    let chars: Vec<char> = input.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\\' && chars.get(i + 1) == Some(&'u') {
            // \u{XXXX}
            if chars.get(i + 2) == Some(&'{') {
                if let Some(close) = chars[i + 3..].iter().position(|&c| c == '}') {
                    let hex: String = chars[i + 3..i + 3 + close].iter().collect();
                    if let Some(c) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                        out.push(c);
                        i += 3 + close + 1;
                        continue;
                    }
                }
            } else if i + 6 <= chars.len() {
                // \uXXXX (exactly four hex digits)
                let hex: String = chars[i + 2..i + 6].iter().collect();
                if hex.chars().all(|c| c.is_ascii_hexdigit()) {
                    if let Some(c) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                        out.push(c);
                        i += 6;
                        continue;
                    }
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn unicode_escape_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = unicode_escape(&s);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

fn unicode_unescape_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = unicode_unescape(&s);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Sort the lines of `input` by character length (shortest first), breaking ties
/// lexically for determinism. Pure — unit tested.
fn sort_by_length(input: &str) -> String {
    let mut lines: Vec<&str> = input.lines().collect();
    lines.sort_by(|a, b| {
        a.chars()
            .count()
            .cmp(&b.chars().count())
            .then_with(|| a.cmp(b))
    });
    lines.join("\n")
}

fn sort_by_length_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = sort_by_length(&s);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Count distinct vs total lines: returns (unique, total). Pure — unit tested.
fn count_unique(input: &str) -> (usize, usize) {
    let lines: Vec<&str> = input.lines().collect();
    let total = lines.len();
    let unique: std::collections::HashSet<&str> = lines.into_iter().collect();
    (unique.len(), total)
}

fn count_unique_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let sel = doc.selection(view.id).primary();
    let s: String = doc
        .text()
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let (unique, total) = count_unique(&s);
    cx.editor
        .set_status(format!("{unique} unique / {total} total line(s)"));
    Ok(())
}

/// Cyclically rotate the lines of `input` by `n`: positive moves the first `n`
/// lines to the bottom, negative rotates the other way. `n` is taken modulo the
/// line count. Pure — unit tested.
fn rotate_lines(input: &str, n: i64) -> String {
    let lines: Vec<&str> = input.lines().collect();
    if lines.is_empty() {
        return String::new();
    }
    let len = lines.len() as i64;
    let shift = (((n % len) + len) % len) as usize;
    let mut rotated: Vec<&str> = lines[shift..].to_vec();
    rotated.extend_from_slice(&lines[..shift]);
    rotated.join("\n")
}

fn rotate_lines_cmd(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let n: i64 = args
        .first()
        .and_then(|a| a.trim().parse().ok())
        .context("usage: :rotate-lines <n> (negative rotates the other way)")?;
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = rotate_lines(&s, n);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Remove a single pair of surrounding matching quotes (`"`, `'`, or `` ` ``) from
/// each line independently (unlike the whole-selection strip). Lines without
/// matched surrounding quotes are unchanged. Pure — unit tested.
fn unquote_each_line(input: &str) -> String {
    input
        .lines()
        .map(|line| {
            let first = line.chars().next();
            let last = line.chars().last();
            if line.chars().count() >= 2 && first == last && matches!(first, Some('"' | '\'' | '`'))
            {
                let q = first.unwrap().len_utf8();
                line[q..line.len() - q].to_string()
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn unquote_lines_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = unquote_each_line(&s);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Wrap each line in double quotes, backslash-escaping any `\` and `"` so the
/// result is a valid string literal. The inverse of [`unquote_each_line`] for
/// double-quoted input. Pure — unit tested.
fn quote_each_line(input: &str) -> String {
    input
        .lines()
        .map(|line| format!("\"{}\"", line.replace('\\', "\\\\").replace('"', "\\\"")))
        .collect::<Vec<_>>()
        .join("\n")
}

fn quote_lines_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = quote_each_line(&s);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

fn repeat_cmd(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let n: usize = args
        .first()
        .and_then(|a| a.trim().parse().ok())
        .context("usage: :repeat <n> (number of copies)")?;
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = s.repeat(n);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Uppercase the first alphabetic character of each line, leaving the rest (and any
/// leading indentation or bullet) untouched. Pure — unit tested.
fn capitalize_lines(input: &str) -> String {
    input
        .lines()
        .map(|line| {
            let mut done = false;
            line.chars()
                .map(|c| {
                    if !done && c.is_alphabetic() {
                        done = true;
                        c.to_uppercase().collect::<String>()
                    } else {
                        c.to_string()
                    }
                })
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn capitalize_lines_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = capitalize_lines(&s);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Drop every blank (whitespace-only) line, unlike the squeeze that collapses runs
/// to one. Pure — unit tested.
fn remove_blank_lines(input: &str) -> String {
    input
        .lines()
        .filter(|l| !l.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn remove_blank_lines_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = remove_blank_lines(&s);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Trim leading and trailing whitespace from each line, preserving internal spacing
/// (unlike normalize-whitespace, which collapses internal runs and keeps the
/// leading indent). Pure — unit tested.
fn trim_lines(input: &str) -> String {
    input.lines().map(str::trim).collect::<Vec<_>>().join("\n")
}

fn trim_lines_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = trim_lines(&s);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Convert `key=value` / `key: value` config or env lines into a JSON object, with
/// values kept as strings. Blank lines and `#` comments are skipped; the split uses
/// the first `=` or `:`. Errors on a line with neither. Pure — unit tested.
fn kv_to_json(input: &str) -> anyhow::Result<String> {
    let mut map = serde_json::Map::new();
    for line in input.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        match line.find(['=', ':']) {
            Some(p) => {
                let key = line[..p].trim().to_string();
                let val = line[p + 1..].trim().to_string();
                map.insert(key, Value::String(val));
            }
            None => anyhow::bail!("line has no '=' or ':': {line}"),
        }
    }
    Ok(serde_json::to_string_pretty(&Value::Object(map))?)
}

fn kv_to_json_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = kv_to_json(&s)?;
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Convert a JSON object into `key=value` lines (string values unquoted, others as
/// compact JSON). The inverse of [`kv_to_json`] for string values. Pure — unit tested.
fn json_to_kv(input: &str) -> anyhow::Result<String> {
    let root: Value = serde_json::from_str(input.trim()).context("selection is not valid JSON")?;
    let obj = root.as_object().context("expected a JSON object")?;
    let lines: Vec<String> = obj
        .iter()
        .map(|(k, v)| {
            let val = match v {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            format!("{k}={val}")
        })
        .collect();
    Ok(lines.join("\n"))
}

fn json_to_kv_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = json_to_kv(&s)?;
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Extract one field's value from every object in a JSON array, one per line (jq
/// `.[].key`): string values unquoted, others compact JSON, missing fields blank.
/// Pure — unit tested.
fn json_pluck(input: &str, key: &str) -> anyhow::Result<String> {
    let root: Value = serde_json::from_str(input.trim()).context("selection is not valid JSON")?;
    let arr = root.as_array().context("expected a JSON array")?;
    let vals: Vec<String> = arr
        .iter()
        .map(|item| match item.get(key) {
            Some(Value::String(s)) => s.clone(),
            Some(v) => v.to_string(),
            None => String::new(),
        })
        .collect();
    Ok(vals.join("\n"))
}

fn json_pluck_cmd(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let key = args.join(" ");
    let key = key.trim();
    if key.is_empty() {
        anyhow::bail!("usage: :json-pluck <key>");
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = json_pluck(&s, key)?;
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Convert the non-blank lines of `input` into an HTML `<ul>` list, one `<li>` per
/// line with `&`, `<`, `>` escaped. Pure — unit tested.
fn to_html_list(input: &str) -> String {
    let items: Vec<String> = input
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let esc = l
                .trim()
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;");
            format!("  <li>{esc}</li>")
        })
        .collect();
    format!("<ul>\n{}\n</ul>", items.join("\n"))
}

fn to_html_list_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = to_html_list(&s);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Extract the text of each `<li>…</li>` item from HTML, one per line, unescaping
/// `&lt;`/`&gt;`/`&amp;`. The inverse of [`to_html_list`]. Pure — unit tested.
fn from_html_list(input: &str) -> String {
    let re = regex::Regex::new(r"(?s)<li[^>]*>(.*?)</li>").expect("valid regex");
    re.captures_iter(input)
        .map(|c| {
            c[1].trim()
                .replace("&lt;", "<")
                .replace("&gt;", ">")
                .replace("&amp;", "&")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn from_html_list_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = from_html_list(&s);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Convert CSV/TSV (first row = headers) into an HTML `<table>`; cell text is
/// HTML-escaped. Tab is auto-detected as the delimiter when present. Pure — unit
/// tested.
fn csv_to_html_table(input: &str) -> String {
    let rows: Vec<&str> = input.lines().filter(|l| !l.trim().is_empty()).collect();
    if rows.is_empty() {
        return String::new();
    }
    let delim = if input.contains('\t') { '\t' } else { ',' };
    let esc = |s: &str| {
        s.trim()
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    };
    let mut out = String::from("<table>");
    for (i, row) in rows.iter().enumerate() {
        let tag = if i == 0 { "th" } else { "td" };
        let cells: String = row
            .split(delim)
            .map(|c| format!("<{tag}>{}</{tag}>", esc(c)))
            .collect();
        out.push_str(&format!("\n  <tr>{cells}</tr>"));
    }
    out.push_str("\n</table>");
    out
}

fn csv_to_html_table_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = csv_to_html_table(&s);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Slugify one line: lowercase, runs of non-alphanumeric characters become a single
/// hyphen, with leading/trailing hyphens trimmed. Pure — unit tested.
fn slugify_line(s: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for c in s.chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
            prev_dash = false;
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
    }
    out.trim_end_matches('-').to_string()
}

/// Slugify each line independently (unlike whole-selection slugify). Pure — unit
/// tested.
fn slugify_lines(input: &str) -> String {
    input
        .lines()
        .map(slugify_line)
        .collect::<Vec<_>>()
        .join("\n")
}

fn slugify_lines_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = slugify_lines(&s);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Join the lines of `input` into a single CSV row, RFC-4180 quoting any field that
/// contains a comma, quote, or newline (embedded quotes doubled). Pure — unit tested.
fn lines_to_csv_row(input: &str) -> String {
    input
        .lines()
        .map(|l| {
            if l.contains([',', '"', '\n']) {
                format!("\"{}\"", l.replace('"', "\"\""))
            } else {
                l.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn lines_to_csv_row_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = lines_to_csv_row(&s);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Split one CSV row into its fields, one per line, honoring RFC-4180 quoting
/// (quoted fields may contain commas; `""` is an escaped quote). The inverse of
/// [`lines_to_csv_row`]. Pure — unit tested.
fn csv_row_to_lines(input: &str) -> String {
    let mut fields: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    let mut chars = input.trim_end_matches(['\n', '\r']).chars().peekable();
    while let Some(c) = chars.next() {
        if in_quotes {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    cur.push('"');
                    chars.next();
                } else {
                    in_quotes = false;
                }
            } else {
                cur.push(c);
            }
        } else {
            match c {
                '"' => in_quotes = true,
                ',' => fields.push(std::mem::take(&mut cur)),
                _ => cur.push(c),
            }
        }
    }
    fields.push(cur);
    fields.join("\n")
}

fn csv_row_to_lines_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = csv_row_to_lines(&s);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Turn a slug into a readable title: split each line on `-`/`_`/space and
/// title-case the words. Pure — unit tested.
fn deslugify(input: &str) -> String {
    input
        .lines()
        .map(|line| {
            line.split(['-', '_', ' '])
                .filter(|w| !w.is_empty())
                .map(|w| {
                    let mut c = w.chars();
                    match c.next() {
                        Some(f) => format!(
                            "{}{}",
                            f.to_uppercase().collect::<String>(),
                            c.as_str().to_lowercase()
                        ),
                        None => String::new(),
                    }
                })
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn deslugify_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = deslugify(&s);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Convert CSV to TSV one row per line: each row is parsed quote-aware (so a quoted
/// `a,b` stays a single cell) and re-joined with tabs. Pure — unit tested.
fn csv_to_tsv(input: &str) -> String {
    input
        .lines()
        .map(|line| csv_row_to_lines(line).replace('\n', "\t"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn csv_to_tsv_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = csv_to_tsv(&s);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Convert TSV to CSV one row per line: split on tabs and RFC-4180-quote any cell
/// containing a comma, quote, or newline. The inverse of [`csv_to_tsv`]. Pure —
/// unit tested.
fn tsv_to_csv(input: &str) -> String {
    input
        .lines()
        .map(|line| {
            line.split('\t')
                .map(|cell| {
                    if cell.contains([',', '"', '\n']) {
                        format!("\"{}\"", cell.replace('"', "\"\""))
                    } else {
                        cell.to_string()
                    }
                })
                .collect::<Vec<_>>()
                .join(",")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn tsv_to_csv_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = tsv_to_csv(&s);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Strip a leading line number from each line: optional indent, digits, an optional
/// `.`/`:`/`)`/`|`/tab separator, then following spaces. Lines without a leading
/// number are unchanged. Pure — unit tested.
fn strip_line_numbers(input: &str) -> String {
    input
        .lines()
        .map(|line| {
            let trimmed = line.trim_start();
            let after_digits = trimmed.trim_start_matches(|c: char| c.is_ascii_digit());
            if after_digits.len() == trimmed.len() {
                return line.to_string();
            }
            let rest = after_digits
                .strip_prefix(['.', ':', ')', '|', '\t'])
                .unwrap_or(after_digits);
            rest.trim_start().to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn strip_line_numbers_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = strip_line_numbers(&s);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Wrap `text` as a Markdown link `[text](url)`. Pure — unit tested.
fn markdown_link(text: &str, url: &str) -> String {
    format!("[{text}]({url})")
}

fn markdown_link_cmd(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let url = args.join(" ");
    let url = url.trim();
    if url.is_empty() {
        anyhow::bail!("usage: :markdown-link <url>");
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = markdown_link(&s, url);
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Extract every http(s) URL from `input`, one per line, in order of appearance.
/// Pure — unit tested.
fn extract_urls(input: &str) -> String {
    let re = regex::Regex::new(r#"https?://[^\s<>()\[\]"]+"#).expect("valid regex");
    re.find_iter(input)
        .map(|m| m.as_str().to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

fn extract_urls_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = extract_urls(&s);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Extract every email-like address from `input`, one per line, in order. Pure —
/// unit tested.
fn extract_emails(input: &str) -> String {
    let re =
        regex::Regex::new(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}").expect("valid regex");
    re.find_iter(input)
        .map(|m| m.as_str().to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

fn extract_emails_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = extract_emails(&s);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Extract IPv4-looking addresses (`d.d.d.d`) from `input`, one per line, in order.
/// Octet ranges aren't validated. Pure — unit tested.
fn extract_ips(input: &str) -> String {
    let re = regex::Regex::new(r"\b(?:\d{1,3}\.){3}\d{1,3}\b").expect("valid regex");
    re.find_iter(input)
        .map(|m| m.as_str().to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

fn extract_ips_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = extract_ips(&s);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Extract the contents of double-quoted strings from `input`, one per line. Escaped
/// quotes inside a string are handled. Pure — unit tested.
fn extract_quoted(input: &str) -> String {
    let re = regex::Regex::new(r#""([^"\\]*(?:\\.[^"\\]*)*)""#).expect("valid regex");
    re.captures_iter(input)
        .map(|c| c[1].to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

fn extract_quoted_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = extract_quoted(&s);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Extract each substring found between the `start` and `end` delimiters, one per
/// line, scanning left to right without overlaps. Pure — unit tested.
fn extract_between(input: &str, start: &str, end: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut rest = input;
    while let Some(i) = rest.find(start) {
        let after = &rest[i + start.len()..];
        match after.find(end) {
            Some(j) => {
                out.push(after[..j].to_string());
                rest = &after[j + end.len()..];
            }
            None => break,
        }
    }
    out.join("\n")
}

fn extract_between_cmd(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let argv: Vec<String> = args.into_iter().map(|a| a.trim().to_string()).collect();
    let (Some(start), Some(end)) = (argv.first(), argv.get(1)) else {
        anyhow::bail!("usage: :extract-between <start> <end>");
    };
    if start.is_empty() || end.is_empty() {
        anyhow::bail!("usage: :extract-between <start> <end>");
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = extract_between(&s, start, end);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Wrap `text` with `wrapper` on both sides. Pure — unit tested.
fn wrap_with(text: &str, wrapper: &str) -> String {
    format!("{wrapper}{text}{wrapper}")
}

fn wrap_with_cmd(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let wrapper = args.join(" ");
    let wrapper = wrapper.trim();
    if wrapper.is_empty() {
        anyhow::bail!("usage: :wrap-with <text>");
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = wrap_with(&s, wrapper);
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Extract signed integers and decimals from `input`, one per line, in order. Pure
/// — unit tested.
fn extract_numbers_lines(input: &str) -> String {
    let re = regex::Regex::new(r"-?\d+(?:\.\d+)?").expect("valid regex");
    re.find_iter(input)
        .map(|m| m.as_str().to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

fn extract_numbers_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = extract_numbers_lines(&s);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Validate that `input` is well-formed JSON, returning the parse error message
/// (with line/column) on failure. Pure — unit tested.
fn json_validate(input: &str) -> Result<(), String> {
    serde_json::from_str::<Value>(input.trim())
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn json_validate_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let sel = doc.selection(view.id).primary();
    let s: String = doc
        .text()
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    match json_validate(&s) {
        Ok(()) => cx.editor.set_status("valid JSON"),
        Err(e) => cx.editor.set_status(format!("invalid JSON: {e}")),
    }
    Ok(())
}

/// Count CSV fields in one row (quote-aware: commas inside `"..."` don't split).
fn csv_field_count(row: &str) -> usize {
    let mut count = 1;
    let mut in_quotes = false;
    let mut chars = row.chars().peekable();
    while let Some(c) = chars.next() {
        if in_quotes {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                } else {
                    in_quotes = false;
                }
            }
        } else if c == '"' {
            in_quotes = true;
        } else if c == ',' {
            count += 1;
        }
    }
    count
}

/// Check that every non-blank CSV row has the same field count. Returns that count
/// on success, or a message naming the first offending line. Pure — unit tested.
fn csv_validate(input: &str) -> Result<usize, String> {
    let rows: Vec<(usize, &str)> = input
        .lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty())
        .collect();
    let Some((_, first)) = rows.first() else {
        return Err("no rows".to_string());
    };
    let expected = csv_field_count(first);
    for (i, row) in &rows {
        let n = csv_field_count(row);
        if n != expected {
            return Err(format!(
                "line {} has {n} fields, expected {expected}",
                i + 1
            ));
        }
    }
    Ok(expected)
}

fn csv_validate_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let sel = doc.selection(view.id).primary();
    let s: String = doc
        .text()
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    match csv_validate(&s) {
        Ok(n) => cx.editor.set_status(format!("valid CSV: {n} columns")),
        Err(e) => cx.editor.set_status(format!("CSV mismatch: {e}")),
    }
    Ok(())
}

/// Turn the non-blank lines into a Markdown ordered list (`1. item`, `2. item`…),
/// stripping a leading bullet so bullet lists convert cleanly. Blank lines are kept
/// and don't advance the counter. Pure — unit tested.
fn ordered_list(input: &str) -> String {
    let mut n = 0;
    input
        .lines()
        .map(|line| {
            if line.trim().is_empty() {
                line.to_string()
            } else {
                n += 1;
                let content = line.trim_start();
                let content = content.strip_prefix("- ").unwrap_or(content);
                format!("{n}. {content}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn ordered_list_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = ordered_list(&s);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Strip a leading Markdown list marker from each line: task checkbox
/// (`- [ ] `/`- [x] `), bullet (`- `/`* `/`+ `), or ordered (`N. `/`N) `). Lines
/// without a marker are unchanged. Pure — unit tested.
fn strip_list_markers(input: &str) -> String {
    input
        .lines()
        .map(|line| {
            let t = line.trim_start();
            if let Some(rest) = t
                .strip_prefix("- [ ] ")
                .or_else(|| t.strip_prefix("- [x] "))
                .or_else(|| t.strip_prefix("- [X] "))
            {
                rest.to_string()
            } else if let Some(rest) = t
                .strip_prefix("- ")
                .or_else(|| t.strip_prefix("* "))
                .or_else(|| t.strip_prefix("+ "))
            {
                rest.to_string()
            } else {
                let after_digits = t.trim_start_matches(|c: char| c.is_ascii_digit());
                if after_digits.len() != t.len() {
                    after_digits
                        .strip_prefix(". ")
                        .or_else(|| after_digits.strip_prefix(") "))
                        .unwrap_or(t)
                        .to_string()
                } else {
                    t.to_string()
                }
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn strip_list_markers_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = strip_list_markers(&s);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Sort the whitespace-separated words within each line (ascending), collapsing
/// runs of whitespace to single spaces. Pure — unit tested.
fn sort_words(input: &str) -> String {
    input
        .lines()
        .map(|line| {
            let mut words: Vec<&str> = line.split_whitespace().collect();
            words.sort_unstable();
            words.join(" ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn sort_words_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = sort_words(&s);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Remove duplicate whitespace-separated words within each line, keeping the first
/// occurrence and original order. Pure — unit tested.
fn unique_words(input: &str) -> String {
    input
        .lines()
        .map(|line| {
            let mut seen = std::collections::HashSet::new();
            line.split_whitespace()
                .filter(|w| seen.insert(*w))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn unique_words_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = unique_words(&s);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Replace each line with the sum of its numeric whitespace fields (a row total);
/// non-numeric fields are ignored. Pure — unit tested.
fn sum_fields(input: &str) -> String {
    input
        .lines()
        .map(|line| {
            let sum: f64 = line
                .split_whitespace()
                .filter_map(|w| w.parse::<f64>().ok())
                .sum();
            fmt_stat(sum)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn sum_fields_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = sum_fields(&s);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Replace each line with the mean of its numeric whitespace fields; lines with no
/// numbers pass through unchanged. Pure — unit tested.
fn avg_fields(input: &str) -> String {
    input
        .lines()
        .map(|line| {
            let nums: Vec<f64> = line
                .split_whitespace()
                .filter_map(|w| w.parse::<f64>().ok())
                .collect();
            if nums.is_empty() {
                line.to_string()
            } else {
                fmt_stat(nums.iter().sum::<f64>() / nums.len() as f64)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn avg_fields_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = avg_fields(&s);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Replace each line with the max (or, when `max` is false, min) of its numeric
/// whitespace fields; lines with no numbers pass through. Pure — unit tested.
fn reduce_fields(input: &str, max: bool) -> String {
    input
        .lines()
        .map(|line| {
            let nums: Vec<f64> = line
                .split_whitespace()
                .filter_map(|w| w.parse::<f64>().ok())
                .collect();
            if nums.is_empty() {
                return line.to_string();
            }
            let v = if max {
                nums.iter().copied().fold(f64::NEG_INFINITY, f64::max)
            } else {
                nums.iter().copied().fold(f64::INFINITY, f64::min)
            };
            fmt_stat(v)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn reduce_fields_impl(
    cx: &mut compositor::Context,
    event: PromptEvent,
    max: bool,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = reduce_fields(&s, max);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

fn max_fields_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    reduce_fields_impl(cx, event, true)
}

fn min_fields_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    reduce_fields_impl(cx, event, false)
}

/// Replace each line with the range (max − min) of its numeric whitespace fields;
/// lines with no numbers pass through. Pure — unit tested.
fn range_fields(input: &str) -> String {
    input
        .lines()
        .map(|line| {
            let nums: Vec<f64> = line
                .split_whitespace()
                .filter_map(|w| w.parse::<f64>().ok())
                .collect();
            if nums.is_empty() {
                return line.to_string();
            }
            let max = nums.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            let min = nums.iter().copied().fold(f64::INFINITY, f64::min);
            fmt_stat(max - min)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn range_fields_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = range_fields(&s);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Prefix each `KEY=value` line with `export ` (turning a `.env` into shell export
/// statements). Blank lines, `#` comments, and already-`export`ed lines are left
/// alone, as are lines without `=`. Pure — unit tested.
fn to_env_export(input: &str) -> String {
    input
        .lines()
        .map(|line| {
            let t = line.trim();
            if t.is_empty() || t.starts_with('#') || t.starts_with("export ") || !t.contains('=') {
                line.to_string()
            } else {
                format!("export {t}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn to_env_export_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = to_env_export(&s);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Remove a leading `export ` from each line (preserving indentation); the inverse
/// of [`to_env_export`]. Lines without the prefix are unchanged. Pure — unit tested.
fn strip_export(input: &str) -> String {
    input
        .lines()
        .map(|line| {
            let indent_len = line.len() - line.trim_start().len();
            let (indent, rest) = line.split_at(indent_len);
            match rest.strip_prefix("export ") {
                Some(after) => format!("{indent}{after}"),
                None => line.to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn strip_export_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = strip_export(&s);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Convert CRLF (and lone CR) line endings to LF. Pure — unit tested.
fn dos2unix(input: &str) -> String {
    input.replace("\r\n", "\n").replace('\r', "\n")
}

/// Convert LF line endings to CRLF (normalizing any existing CRLF first). Pure —
/// unit tested.
fn unix2dos(input: &str) -> String {
    input.replace("\r\n", "\n").replace('\n', "\r\n")
}

fn line_ending_impl(
    cx: &mut compositor::Context,
    event: PromptEvent,
    to_dos: bool,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = if to_dos { unix2dos(&s) } else { dos2unix(&s) };
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

fn dos2unix_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    line_ending_impl(cx, event, false)
}

fn unix2dos_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    line_ending_impl(cx, event, true)
}

/// Replace each numeric line with its percentage of the total of all numeric lines.
/// Non-numeric lines pass through; if the total is zero, nothing changes. Pure —
/// unit tested.
fn percent_of_total(input: &str) -> String {
    let nums: Vec<Option<f64>> = input
        .lines()
        .map(|l| l.trim().parse::<f64>().ok())
        .collect();
    let total: f64 = nums.iter().flatten().sum();
    input
        .lines()
        .zip(nums.iter())
        .map(|(line, n)| match n {
            Some(v) if total != 0.0 => format!("{}%", fmt_stat(v / total * 100.0)),
            _ => line.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn percent_of_total_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = percent_of_total(&s);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Replace each numeric line with the running maximum seen so far (a high-water
/// mark); non-numeric lines pass through and don't update the running value. Pure —
/// unit tested.
fn running_max(input: &str) -> String {
    let mut acc: Option<f64> = None;
    input
        .lines()
        .map(|line| match line.trim().parse::<f64>() {
            Ok(n) => {
                let m = acc.map_or(n, |a| a.max(n));
                acc = Some(m);
                fmt_stat(m)
            }
            Err(_) => line.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn running_max_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = running_max(&s);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Replace each numeric line with the running minimum seen so far (a low-water
/// mark); non-numeric lines pass through and don't update the running value. Pure —
/// unit tested.
fn running_min(input: &str) -> String {
    let mut acc: Option<f64> = None;
    input
        .lines()
        .map(|line| match line.trim().parse::<f64>() {
            Ok(n) => {
                let m = acc.map_or(n, |a| a.min(n));
                acc = Some(m);
                fmt_stat(m)
            }
            Err(_) => line.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn running_min_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = running_min(&s);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Format each numeric line to `n` decimal places; non-numeric lines pass through.
/// Pure — unit tested.
fn to_fixed(input: &str, n: usize) -> String {
    input
        .lines()
        .map(|line| match line.trim().parse::<f64>() {
            Ok(v) => format!("{v:.n$}"),
            Err(_) => line.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn to_fixed_cmd(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let n: usize = args
        .first()
        .and_then(|a| a.trim().parse().ok())
        .context("usage: :to-fixed <decimals>")?;
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = to_fixed(&s, n);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Clamp each numeric line to `[lo, hi]`; non-numeric lines pass through. Pure —
/// unit tested.
fn clamp_lines(input: &str, lo: f64, hi: f64) -> String {
    input
        .lines()
        .map(|line| match line.trim().parse::<f64>() {
            Ok(v) => fmt_stat(v.clamp(lo, hi)),
            Err(_) => line.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn clamp_cmd(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let argv: Vec<String> = args.into_iter().map(|a| a.trim().to_string()).collect();
    let lo: f64 = argv
        .first()
        .and_then(|a| a.parse().ok())
        .context("usage: :clamp <min> <max>")?;
    let hi: f64 = argv
        .get(1)
        .and_then(|a| a.parse().ok())
        .context("usage: :clamp <min> <max>")?;
    if lo > hi {
        anyhow::bail!("min ({lo}) must not exceed max ({hi})");
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = clamp_lines(&s, lo, hi);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Multiply each numeric line by `factor`; non-numeric lines pass through. Pure —
/// unit tested.
fn scale_lines(input: &str, factor: f64) -> String {
    input
        .lines()
        .map(|line| match line.trim().parse::<f64>() {
            Ok(v) => fmt_stat(v * factor),
            Err(_) => line.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn scale_cmd(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let factor: f64 = args
        .first()
        .and_then(|a| a.trim().parse().ok())
        .context("usage: :scale <factor>")?;
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = scale_lines(&s, factor);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Add `delta` to each numeric line; non-numeric lines pass through. Pure — unit
/// tested.
fn offset_lines(input: &str, delta: f64) -> String {
    input
        .lines()
        .map(|line| match line.trim().parse::<f64>() {
            Ok(v) => fmt_stat(v + delta),
            Err(_) => line.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn offset_cmd(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let delta: f64 = args
        .first()
        .and_then(|a| a.trim().parse().ok())
        .context("usage: :offset <n>")?;
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = offset_lines(&s, delta);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Replace each numeric line with its absolute value; non-numeric lines pass
/// through. Pure — unit tested.
fn abs_lines(input: &str) -> String {
    input
        .lines()
        .map(|line| match line.trim().parse::<f64>() {
            Ok(v) => fmt_stat(v.abs()),
            Err(_) => line.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn abs_cmd(cx: &mut compositor::Context, _args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = abs_lines(&s);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Wrap each bare http(s) URL in Markdown link syntax `[url](url)`. Pure — unit
/// tested.
fn linkify(input: &str) -> String {
    let re = regex::Regex::new(r#"https?://[^\s<>()\[\]"]+"#).expect("valid regex");
    re.replace_all(input, "[$0]($0)").to_string()
}

fn linkify_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = linkify(&s);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Replace each `[text](url)` Markdown link with just its `text` (markdown → plain).
/// Pure — unit tested.
fn strip_markdown_links(input: &str) -> String {
    let re = regex::Regex::new(r"\[([^\]]*)\]\([^)]*\)").expect("valid regex");
    re.replace_all(input, "$1").to_string()
}

fn strip_markdown_links_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = strip_markdown_links(&s);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Remove paired Markdown emphasis markers — bold (`**`/`__`), italic (`*`/`_`), and
/// inline code (`` ` ``) — leaving the inner text. Double markers are handled before
/// single ones. Pure — unit tested.
fn strip_emphasis(input: &str) -> String {
    let mut s = input.to_string();
    for pat in [
        r"\*\*([^*]+)\*\*",
        r"__([^_]+)__",
        r"\*([^*]+)\*",
        r"_([^_]+)_",
        r"`([^`]+)`",
    ] {
        let re = regex::Regex::new(pat).expect("valid regex");
        s = re.replace_all(&s, "$1").to_string();
    }
    s
}

fn strip_emphasis_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = strip_emphasis(&s);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Remove `<!-- ... -->` HTML/Markdown comments, including multi-line ones, from
/// `input`. Pure — unit tested.
fn strip_html_comments(input: &str) -> String {
    let re = regex::Regex::new(r"(?s)<!--.*?-->").expect("valid regex");
    re.replace_all(input, "").to_string()
}

fn strip_html_comments_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = strip_html_comments(&s);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Remove trailing commas that appear just before a closing `}` or `]` (turning
/// JSON5/JS into strict JSON). Pure — unit tested.
fn remove_trailing_commas(input: &str) -> String {
    let re = regex::Regex::new(r",(\s*[}\]])").expect("valid regex");
    re.replace_all(input, "$1").to_string()
}

fn remove_trailing_commas_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = remove_trailing_commas(&s);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Add a trailing comma before a closing `}` or `]` when the last value doesn't
/// already have one (cleaner git diffs in JS/JSON5). Empty `{}`/`[]` are left alone.
/// Idempotent. Pure — unit tested.
fn add_trailing_commas(input: &str) -> String {
    let re = regex::Regex::new(r"([^\s,{\[(])(\s*[}\]])").expect("valid regex");
    re.replace_all(input, "$1,$2").to_string()
}

fn add_trailing_commas_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = add_trailing_commas(&s);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Convert straight quotes to typographic curly quotes: a quote after whitespace or
/// an opening bracket becomes an opening quote, otherwise a closing quote (so
/// apostrophes become `’`). The inverse of `:straighten-quotes`. Pure — unit tested.
fn smart_quotes(input: &str) -> String {
    let chars: Vec<char> = input.chars().collect();
    let mut out = String::new();
    for (i, &c) in chars.iter().enumerate() {
        let opening =
            i == 0 || chars[i - 1].is_whitespace() || matches!(chars[i - 1], '(' | '[' | '{');
        match c {
            '"' => out.push(if opening { '\u{201C}' } else { '\u{201D}' }),
            '\'' => out.push(if opening { '\u{2018}' } else { '\u{2019}' }),
            _ => out.push(c),
        }
    }
    out
}

fn smart_quotes_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = smart_quotes(&s);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Apply typographic substitutions: `---` → em dash, `--` → en dash, `...` →
/// ellipsis (longest sequences first). Pure — unit tested.
fn typographic_dashes(input: &str) -> String {
    input
        .replace("---", "\u{2014}")
        .replace("--", "\u{2013}")
        .replace("...", "\u{2026}")
}

fn typographic_dashes_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = typographic_dashes(&s);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Normalize typographic punctuation back to ASCII: curly quotes → `"`/`'`, em dash
/// → `---`, en dash → `--`, ellipsis → `...`. Pure — unit tested.
fn de_typography(input: &str) -> String {
    input
        .replace(['\u{201C}', '\u{201D}'], "\"")
        .replace(['\u{2018}', '\u{2019}'], "'")
        .replace('\u{2014}', "---")
        .replace('\u{2013}', "--")
        .replace('\u{2026}', "...")
}

fn de_typography_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = de_typography(&s);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Transliterate common accented Latin characters to their ASCII equivalents (e.g.
/// `é`→`e`, `ß`→`ss`, `æ`→`ae`); other characters are left as-is. Pure — unit tested.
fn to_ascii(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for c in input.chars() {
        let repl: &str = match c {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' => "a",
            'è' | 'é' | 'ê' | 'ë' | 'ē' => "e",
            'ì' | 'í' | 'î' | 'ï' | 'ī' => "i",
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' => "o",
            'ù' | 'ú' | 'û' | 'ü' | 'ū' => "u",
            'ñ' => "n",
            'ç' => "c",
            'ý' | 'ÿ' => "y",
            'À' | 'Á' | 'Â' | 'Ã' | 'Ä' | 'Å' => "A",
            'È' | 'É' | 'Ê' | 'Ë' => "E",
            'Ì' | 'Í' | 'Î' | 'Ï' => "I",
            'Ò' | 'Ó' | 'Ô' | 'Õ' | 'Ö' | 'Ø' => "O",
            'Ù' | 'Ú' | 'Û' | 'Ü' => "U",
            'Ñ' => "N",
            'Ç' => "C",
            'ß' => "ss",
            'æ' => "ae",
            'Æ' => "AE",
            'œ' => "oe",
            'Œ' => "OE",
            other => {
                out.push(other);
                continue;
            }
        };
        out.push_str(repl);
    }
    out
}

fn to_ascii_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = to_ascii(&s);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// NATO phonetic alphabet for A–Z and 0–9.
const NATO: &[(char, &str)] = &[
    ('a', "Alfa"),
    ('b', "Bravo"),
    ('c', "Charlie"),
    ('d', "Delta"),
    ('e', "Echo"),
    ('f', "Foxtrot"),
    ('g', "Golf"),
    ('h', "Hotel"),
    ('i', "India"),
    ('j', "Juliett"),
    ('k', "Kilo"),
    ('l', "Lima"),
    ('m', "Mike"),
    ('n', "November"),
    ('o', "Oscar"),
    ('p', "Papa"),
    ('q', "Quebec"),
    ('r', "Romeo"),
    ('s', "Sierra"),
    ('t', "Tango"),
    ('u', "Uniform"),
    ('v', "Victor"),
    ('w', "Whiskey"),
    ('x', "Xray"),
    ('y', "Yankee"),
    ('z', "Zulu"),
    ('0', "Zero"),
    ('1', "One"),
    ('2', "Two"),
    ('3', "Three"),
    ('4', "Four"),
    ('5', "Five"),
    ('6', "Six"),
    ('7', "Seven"),
    ('8', "Eight"),
    ('9', "Nine"),
];

/// Spell `input` in the NATO phonetic alphabet (case-insensitive), space-joined.
/// Characters not in the alphabet are dropped. Pure — unit tested.
fn nato_spell(input: &str) -> String {
    input
        .chars()
        .filter_map(|c| {
            let lc = c.to_ascii_lowercase();
            NATO.iter().find(|(ch, _)| *ch == lc).map(|(_, w)| *w)
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn nato_cmd(cx: &mut compositor::Context, _args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = nato_spell(&s);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Transpose a whitespace-separated grid: row `i`, column `j` becomes row `j`,
/// column `i`. Short rows are padded with empty cells. Pure — unit tested.
fn transpose_grid(input: &str) -> String {
    let rows: Vec<Vec<&str>> = input
        .lines()
        .map(|l| l.split_whitespace().collect())
        .collect();
    let cols = rows.iter().map(Vec::len).max().unwrap_or(0);
    (0..cols)
        .map(|c| {
            rows.iter()
                .map(|r| r.get(c).copied().unwrap_or(""))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn transpose_grid_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = transpose_grid(&s);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Repeat each line `n` times consecutively. Pure — unit tested.
fn repeat_lines(input: &str, n: usize) -> String {
    input
        .lines()
        .flat_map(|l| (0..n).map(move |_| l))
        .collect::<Vec<_>>()
        .join("\n")
}

fn repeat_lines_cmd(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let n: usize = args
        .first()
        .and_then(|a| a.trim().parse().ok())
        .filter(|n| *n >= 1)
        .context("usage: :repeat-lines <n>")?;
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = repeat_lines(&s, n);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// `:grep-word` — search the project for the whole word under the cursor (a
/// lightweight "find references"), jumpable in the Run console.
fn grep_word(cx: &mut compositor::Context, _args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let word = {
        let (view, doc) = current!(cx.editor);
        let text = doc.text().slice(..);
        let cursor = doc.selection(view.id).primary().cursor(text);
        let line_idx = text.char_to_line(cursor);
        let col = cursor - text.line_to_char(line_idx);
        let line: String = text
            .line(line_idx)
            .chars()
            .filter(|c| *c != '\n' && *c != '\r')
            .collect();
        word_at_col(&line, col)
    };
    let Some(word) = word else {
        bail!("no word under cursor");
    };
    spawn_into_run_console(cx, grep_command(&word, true));
    Ok(())
}

/// Char-offset ranges of every whole-word occurrence of `word` in `haystack`
/// (a match must not be flanked by identifier characters). Pure — unit tested.
fn whole_word_ranges(haystack: &str, word: &str) -> Vec<(usize, usize)> {
    if word.is_empty() {
        return Vec::new();
    }
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let chars: Vec<char> = haystack.chars().collect();
    let wchars: Vec<char> = word.chars().collect();
    let wlen = wchars.len();
    let mut ranges = Vec::new();
    let mut i = 0;
    while i + wlen <= chars.len() {
        if chars[i..i + wlen] == wchars[..] {
            let before_ok = i == 0 || !is_word(chars[i - 1]);
            let after_ok = i + wlen >= chars.len() || !is_word(chars[i + wlen]);
            if before_ok && after_ok {
                ranges.push((i, i + wlen));
                i += wlen;
                continue;
            }
        }
        i += 1;
    }
    ranges
}

/// `:rename-word <new>` — rename every whole-word occurrence of the identifier
/// under the cursor within the current buffer (a textual rename that, unlike LSP
/// rename, needs no language server).
fn rename_word(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let new_name = args.join(" ");
    let new_name = new_name.trim();
    if new_name.is_empty() {
        bail!("usage: :rename-word <new-name>");
    }

    let (view, doc) = current!(cx.editor);
    let text = doc.text().slice(..);
    let cursor = doc.selection(view.id).primary().cursor(text);
    let line_idx = text.char_to_line(cursor);
    let col = cursor - text.line_to_char(line_idx);
    let line: String = text
        .line(line_idx)
        .chars()
        .filter(|c| *c != '\n' && *c != '\r')
        .collect();
    let Some(old) = word_at_col(&line, col) else {
        bail!("no symbol under cursor");
    };

    let haystack: String = text.chunks().collect();
    let ranges = whole_word_ranges(&haystack, &old);
    if ranges.is_empty() {
        return Ok(());
    }
    let count = ranges.len();
    let transaction = Transaction::change(
        doc.text(),
        ranges
            .into_iter()
            .map(|(s, e)| (s, e, Some(new_name.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    cx.editor.set_status(format!(
        "renamed {count} occurrence(s) of `{old}` → `{new_name}`"
    ));
    Ok(())
}

/// `:todos` — scan the whole project for TODO/FIXME-style markers, jumpable in
/// the Run console (the Todo tab only scans the current buffer).
fn project_todos(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    spawn_into_run_console(
        cx,
        grep_command(r"\b(TODO|FIXME|HACK|XXX|BUG|NOTE)\b", false),
    );
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

/// Run a `git stash …` subcommand synchronously (it's a fast local op) and
/// return the first line of output, or the first line of stderr on failure.
fn run_git_stash(args: &[&str]) -> Result<String, String> {
    let dir = std::env::current_dir().unwrap_or_default();
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(&dir)
        .args(args)
        .output()
        .map_err(|e| e.to_string())?;
    let first = |b: &[u8]| {
        String::from_utf8_lossy(b)
            .lines()
            .next()
            .unwrap_or("")
            .trim()
            .to_owned()
    };
    if out.status.success() {
        Ok(first(&out.stdout))
    } else {
        Err(first(&out.stderr))
    }
}

/// `:stash` — stash the working-tree changes, then reload open buffers so they
/// reflect the reverted (HEAD) state.
pub(crate) fn git_stash(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    match run_git_stash(&["stash", "push"]) {
        Ok(msg) => {
            let _ = reload_all(cx, args, PromptEvent::Validate);
            cx.editor.set_status(if msg.is_empty() {
                "git stash: done".to_string()
            } else {
                format!("git: {msg}")
            });
        }
        Err(e) => cx.editor.set_error(format!("git stash: {e}")),
    }
    Ok(())
}

/// `:stash-pop` — restore the most recent stash, then reload open buffers.
pub(crate) fn git_stash_pop(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    match run_git_stash(&["stash", "pop"]) {
        Ok(msg) => {
            let _ = reload_all(cx, args, PromptEvent::Validate);
            cx.editor.set_status(if msg.is_empty() {
                "git stash pop: done".to_string()
            } else {
                format!("git: {msg}")
            });
        }
        Err(e) => cx.editor.set_error(format!("git stash pop: {e}")),
    }
    Ok(())
}

/// Run `git <git_args> -- <current file>` in the file's directory and report
/// `<verb> <path>` (or the git error). Shared by `:git-stage`/`:git-unstage`.
pub(crate) fn git_on_current_file(
    cx: &mut compositor::Context,
    git_args: &[&str],
    verb: &str,
) -> anyhow::Result<()> {
    let path = doc!(cx.editor)
        .path()
        .map(ToOwned::to_owned)
        .context("buffer has no file")?;
    let dir = path
        .parent()
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let mut cmd = std::process::Command::new("git");
    cmd.arg("-C").arg(&dir);
    for a in git_args {
        cmd.arg(a);
    }
    cmd.arg("--").arg(&path);
    let out = cmd.output().map_err(|e| anyhow!("git: {e}"))?;
    if out.status.success() {
        cx.editor.set_status(format!("{verb} {}", path.display()));
        Ok(())
    } else {
        bail!(
            "git {verb}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
}

/// `:git-stage` — stage the current buffer's file (`git add`).
fn git_stage(cx: &mut compositor::Context, _args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    git_on_current_file(cx, &["add"], "staged")
}

/// `:git-unstage` — unstage the current buffer's file (`git reset HEAD`).
fn git_unstage(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    git_on_current_file(cx, &["reset", "-q", "HEAD"], "unstaged")
}

/// Reload all open documents from disk (after an external working-tree change
/// such as a branch checkout). Usable from non-typable callers.
pub(crate) fn reload_open_docs(cx: &mut compositor::Context) {
    let _ = reload_all(cx, Args::default(), PromptEvent::Validate);
}

/// Invoke stash / stash-pop from a non-typable caller (e.g. the Git tab key map).
pub(crate) fn git_stash_action(cx: &mut compositor::Context, pop: bool) {
    let res = if pop {
        git_stash_pop(cx, Args::default(), PromptEvent::Validate)
    } else {
        git_stash(cx, Args::default(), PromptEvent::Validate)
    };
    if let Err(e) = res {
        cx.editor.set_error(e.to_string());
    }
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
                    val.parse()
                        .map_err(|_| anyhow!("expected bool for {name}"))?
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
                let n: i64 = val
                    .parse()
                    .map_err(|_| anyhow!("expected number for {name}"))?;
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
    cx.editor
        .config_events
        .0
        .send(ConfigEvent::Update(config))?;
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

fn change_case(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let style = args[0].to_lowercase();
    transform_symbol_under_cursor(cx, |sym| {
        symbol_to_case(sym, &style)
            .ok_or_else(|| anyhow!("Unknown case style `{style}` (use camel|snake|kebab|pascal)"))
    })
}

fn cycle_case(cx: &mut compositor::Context, _args: Args, event: PromptEvent) -> anyhow::Result<()> {
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
            slice.char_to_line(sel.to().min(slice.len_chars().saturating_sub(1))),
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
    doc.set_selection(view.id, Selection::point(0));
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

    let transaction = Transaction::change(
        doc.text(),
        std::iter::once((insert_at, insert_at, Some(Tendril::from(text.as_str())))),
    );
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
fn put_lines_above(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
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

/// Align lines on the first occurrence of `delim` so the delimiters share a
/// column (Tabularize / easy-align style). Lines without the delimiter are left
/// untouched; trailing whitespace before the delimiter is collapsed to the
/// computed padding plus a single space. Pure — unit tested.
fn align_on_delim(lines: &[&str], delim: &str) -> Vec<String> {
    if delim.is_empty() {
        return lines.iter().map(|l| l.to_string()).collect();
    }
    // (byte index of delimiter, char width of the left part trimmed of trailing
    // whitespace) for each line that contains the delimiter.
    let metas: Vec<Option<(usize, usize)>> = lines
        .iter()
        .map(|l| {
            l.find(delim)
                .map(|bidx| (bidx, l[..bidx].trim_end().chars().count()))
        })
        .collect();
    let target = metas
        .iter()
        .filter_map(|m| m.map(|(_, len)| len))
        .max()
        .unwrap_or(0);
    lines
        .iter()
        .zip(&metas)
        .map(|(l, m)| match m {
            Some((bidx, len)) => {
                let left = l[..*bidx].trim_end();
                let rest = &l[*bidx..]; // delimiter + everything after it
                format!("{}{} {}", left, " ".repeat(target - len), rest)
            }
            None => l.to_string(),
        })
        .collect()
}

fn align_delimiter(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let delim = args
        .first()
        .map(|s| s.to_string())
        .unwrap_or_else(|| "=".into());
    if delim.is_empty() {
        return Ok(());
    }

    let (view, doc) = current!(cx.editor);
    let slice = doc.text().slice(..);
    let total = slice.len_lines();
    let (first, last) = primary_line_range(doc, view.id);
    let region_start = slice.line_to_char(first);
    let region_end = if last + 1 < total {
        slice.line_to_char(last + 1)
    } else {
        slice.len_chars()
    };
    if region_start >= region_end {
        return Ok(());
    }

    let block: String = slice.slice(region_start..region_end).chunks().collect();
    let had_trailing = block.ends_with('\n');
    let mut lines: Vec<&str> = block.split('\n').collect();
    if had_trailing {
        lines.pop();
    }
    let aligned = align_on_delim(&lines, &delim);
    let mut out = aligned.join("\n");
    if had_trailing {
        out.push('\n');
    }
    if out == block {
        return Ok(());
    }

    let transaction = Transaction::change(
        doc.text(),
        std::iter::once((region_start, region_end, Some(out.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
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

fn join_lines_cmd(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    join_lines(cx, args, event, true)
}
fn join_lines_nospace_cmd(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    join_lines(cx, args, event, false)
}

fn yank_lines_cmd(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    yank_lines(cx, args, event, false)
}
fn delete_lines_cmd(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    yank_lines(cx, args, event, true)
}

fn substitute(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
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

fn split_line(cx: &mut compositor::Context, _args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    let (view, doc) = current!(cx.editor);
    let line_ending = Tendril::from(doc.line_ending.as_str());
    let slice = doc.text().slice(..);
    let cursor = doc.selection(view.id).primary().cursor(slice);

    // Insert a line ending at the cursor, pushing the rest of the line down,
    // but keep the cursor where it was (emacs `split-line`).
    let transaction = Transaction::change(
        doc.text(),
        std::iter::once((cursor, cursor, Some(line_ending))),
    );
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
    doc.set_selection(
        view.id,
        Selection::point((start + 1).min(doc.text().len_chars())),
    );
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
        slice
            .slice(start..end)
            .chars()
            .all(|c| c == ' ' || c == '\t')
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

/// Reverse the order of the lines in `block`, preserving its trailing-newline
/// shape (a block that ended in a newline still does; one that didn't, doesn't).
fn reverse_line_order(block: &str) -> String {
    let had_trailing = block.ends_with('\n');
    let mut lines: Vec<&str> = block.split('\n').collect();
    if had_trailing {
        lines.pop(); // drop the empty element produced by the trailing newline
    }
    lines.reverse();
    let mut out = lines.join("\n");
    if had_trailing {
        out.push('\n');
    }
    out
}

fn reverse_lines(
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
    if total == 0 {
        return Ok(());
    }

    // Reverse the primary selection's line span, or the whole buffer when the
    // selection is confined to a single line.
    let range = doc.selection(view.id).primary();
    let start_line = slice.char_to_line(range.from());
    let last_char = range.to().saturating_sub(1).max(range.from());
    let end_line = slice.char_to_line(last_char);
    let (first, last) = if end_line > start_line {
        (start_line, end_line)
    } else {
        (0, total - 1)
    };

    let region_start = slice.line_to_char(first);
    let region_end = if last + 1 < total {
        slice.line_to_char(last + 1)
    } else {
        slice.len_chars()
    };
    if region_start >= region_end {
        return Ok(());
    }

    let block: String = slice.slice(region_start..region_end).chunks().collect();
    let reversed = reverse_line_order(&block);
    if reversed == block {
        return Ok(());
    }

    let transaction = Transaction::change(
        doc.text(),
        std::iter::once((region_start, region_end, Some(reversed.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Parse the leading numeric value of a line for `:sort-lines --numeric`, ignoring
/// leading whitespace. Lines without a leading number sort first.
fn leading_number(line: &str) -> f64 {
    let t = line.trim_start();
    let bytes = t.as_bytes();
    let mut end = 0;
    if end < bytes.len() && (bytes[end] == b'-' || bytes[end] == b'+') {
        end += 1;
    }
    let digits_start = end;
    while end < bytes.len() && (bytes[end].is_ascii_digit() || bytes[end] == b'.') {
        end += 1;
    }
    if end == digits_start {
        return f64::NEG_INFINITY; // no leading number → sort first
    }
    t[..end].parse::<f64>().unwrap_or(f64::NEG_INFINITY)
}

/// Sort the lines of `block`, preserving its trailing-newline shape. The sort is
/// stable on ties. `numeric` orders by each line's leading number; `insensitive`
/// folds case; `reverse` flips the result; `unique` drops duplicate lines.
/// Sort the lines of `block` by their `field`-th whitespace-separated field
/// (1-based), stably, ties broken by the whole line. Lines with fewer fields sort
/// by an empty key (first). Preserves the trailing-newline shape. Pure — unit tested.
fn sort_lines_by_field(block: &str, field: usize) -> String {
    let had_trailing = block.ends_with('\n');
    let mut lines: Vec<&str> = block.split('\n').collect();
    if had_trailing {
        lines.pop();
    }
    let key = |l: &str| -> String {
        l.split_whitespace()
            .nth(field.saturating_sub(1))
            .unwrap_or("")
            .to_string()
    };
    lines.sort_by(|a, b| key(a).cmp(&key(b)).then_with(|| a.cmp(b)));
    let out = lines.join("\n");
    if had_trailing {
        format!("{out}\n")
    } else {
        out
    }
}

fn sort_line_block(
    block: &str,
    reverse: bool,
    insensitive: bool,
    numeric: bool,
    unique: bool,
) -> String {
    let had_trailing = block.ends_with('\n');
    let mut lines: Vec<&str> = block.split('\n').collect();
    if had_trailing {
        lines.pop();
    }
    if numeric {
        lines.sort_by(|a, b| {
            leading_number(a)
                .partial_cmp(&leading_number(b))
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.cmp(b))
        });
    } else if insensitive {
        lines.sort_by(|a, b| {
            a.to_lowercase()
                .cmp(&b.to_lowercase())
                .then_with(|| a.cmp(b))
        });
    } else {
        lines.sort();
    }
    if reverse {
        lines.reverse();
    }
    if unique {
        lines.dedup();
    }
    let mut out = lines.join("\n");
    if had_trailing {
        out.push('\n');
    }
    out
}

/// `:sort-by-field <n>` — sort the selected lines by their Nth whitespace field.
fn sort_by_field(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let field: usize = args
        .first()
        .and_then(|a| a.trim().parse().ok())
        .unwrap_or(1)
        .max(1);

    let (view, doc) = current!(cx.editor);
    let slice = doc.text().slice(..);
    let total = slice.len_lines();
    let (first, last) = primary_line_range(doc, view.id);
    let region_start = slice.line_to_char(first);
    let region_end = if last + 1 < total {
        slice.line_to_char(last + 1)
    } else {
        slice.len_chars()
    };
    if region_start >= region_end {
        return Ok(());
    }
    let block: String = slice.slice(region_start..region_end).chunks().collect();
    let sorted = sort_lines_by_field(&block, field);
    if sorted == block {
        return Ok(());
    }
    let transaction = Transaction::change(
        doc.text(),
        std::iter::once((region_start, region_end, Some(sorted.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

fn sort_lines(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    let (view, doc) = current!(cx.editor);
    let slice = doc.text().slice(..);
    let total = slice.len_lines();
    if total == 0 {
        return Ok(());
    }

    // Sort the primary selection's line span, or the whole buffer when the
    // selection is confined to a single line.
    let range = doc.selection(view.id).primary();
    let start_line = slice.char_to_line(range.from());
    let last_char = range.to().saturating_sub(1).max(range.from());
    let end_line = slice.char_to_line(last_char);
    let (first, last) = if end_line > start_line {
        (start_line, end_line)
    } else {
        (0, total - 1)
    };

    let region_start = slice.line_to_char(first);
    let region_end = if last + 1 < total {
        slice.line_to_char(last + 1)
    } else {
        slice.len_chars()
    };
    if region_start >= region_end {
        return Ok(());
    }

    let block: String = slice.slice(region_start..region_end).chunks().collect();
    let sorted = sort_line_block(
        &block,
        args.has_flag("reverse"),
        args.has_flag("insensitive"),
        args.has_flag("numeric"),
        args.has_flag("unique"),
    );
    if sorted == block {
        return Ok(());
    }

    let transaction = Transaction::change(
        doc.text(),
        std::iter::once((region_start, region_end, Some(sorted.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Join the lines of `block` with `sep` into a single line, preserving a trailing
/// newline if the block had one. Pure — unit tested.
fn join_lines_with(block: &str, sep: &str) -> String {
    let had_trailing = block.ends_with('\n');
    let mut lines: Vec<&str> = block.split('\n').collect();
    if had_trailing {
        lines.pop();
    }
    let joined = lines.join(sep);
    if had_trailing {
        format!("{joined}\n")
    } else {
        joined
    }
}

/// `:join-with [sep]` — join the selected lines into one, separated by `sep`
/// (default `", "`). Turns a column of values into a CSV/IN-list.
fn join_with(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let sep = if args.is_empty() {
        ", ".to_string()
    } else {
        args.join(" ")
    };

    let (view, doc) = current!(cx.editor);
    let slice = doc.text().slice(..);
    let total = slice.len_lines();
    let (first, last) = primary_line_range(doc, view.id);
    let region_start = slice.line_to_char(first);
    let region_end = if last + 1 < total {
        slice.line_to_char(last + 1)
    } else {
        slice.len_chars()
    };
    if region_start >= region_end {
        return Ok(());
    }

    let block: String = slice.slice(region_start..region_end).chunks().collect();
    let joined = join_lines_with(&block, &sep);
    if joined == block {
        return Ok(());
    }
    let transaction = Transaction::change(
        doc.text(),
        std::iter::once((region_start, region_end, Some(joined.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Split `block` on every occurrence of `sep`, putting each piece on its own
/// line, preserving a trailing newline if the block had one. The inverse of
/// [`join_lines_with`]. Pure — unit tested.
fn split_on_sep(block: &str, sep: &str) -> String {
    if sep.is_empty() {
        return block.to_string();
    }
    let had_trailing = block.ends_with('\n');
    let core = block.strip_suffix('\n').unwrap_or(block);
    let joined = core.split(sep).collect::<Vec<_>>().join("\n");
    if had_trailing {
        format!("{joined}\n")
    } else {
        joined
    }
}

/// `:split-on [sep]` — split the selected line(s) on `sep` (default `,`) into one
/// item per line. The inverse of `:join-with`.
fn split_on(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let sep = if args.is_empty() {
        ",".to_string()
    } else {
        args.join(" ")
    };

    let (view, doc) = current!(cx.editor);
    let slice = doc.text().slice(..);
    let total = slice.len_lines();
    let (first, last) = primary_line_range(doc, view.id);
    let region_start = slice.line_to_char(first);
    let region_end = if last + 1 < total {
        slice.line_to_char(last + 1)
    } else {
        slice.len_chars()
    };
    if region_start >= region_end {
        return Ok(());
    }

    let block: String = slice.slice(region_start..region_end).chunks().collect();
    let split = split_on_sep(&block, &sep);
    if split == block {
        return Ok(());
    }
    let transaction = Transaction::change(
        doc.text(),
        std::iter::once((region_start, region_end, Some(split.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Remove consecutive duplicate lines from `block` (Unix `uniq`: only adjacent
/// repeats collapse; non-adjacent repeats are kept). Preserves the trailing
/// newline shape. Pure — unit tested.
/// Collapse runs of blank (whitespace-only) lines in `block` to a single blank
/// line (`cat -s`), preserving the trailing-newline shape. Pure — unit tested.
fn squeeze_blank_lines(block: &str) -> String {
    let had_trailing = block.ends_with('\n');
    let mut lines: Vec<&str> = block.split('\n').collect();
    if had_trailing {
        lines.pop();
    }
    let mut out: Vec<&str> = Vec::with_capacity(lines.len());
    let mut prev_blank = false;
    for l in lines {
        let blank = l.trim().is_empty();
        if blank && prev_blank {
            continue;
        }
        out.push(l);
        prev_blank = blank;
    }
    let joined = out.join("\n");
    if had_trailing {
        format!("{joined}\n")
    } else {
        joined
    }
}

/// `:squeeze-blank-lines` — collapse consecutive blank lines in the selection to
/// one (`cat -s`), unlike `:delete-blank-lines` which removes them all.
fn squeeze_blank_lines_cmd(
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
    let (first, last) = primary_line_range(doc, view.id);
    let region_start = slice.line_to_char(first);
    let region_end = if last + 1 < total {
        slice.line_to_char(last + 1)
    } else {
        slice.len_chars()
    };
    if region_start >= region_end {
        return Ok(());
    }
    let block: String = slice.slice(region_start..region_end).chunks().collect();
    let squeezed = squeeze_blank_lines(&block);
    if squeezed == block {
        return Ok(());
    }
    let transaction = Transaction::change(
        doc.text(),
        std::iter::once((region_start, region_end, Some(squeezed.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

fn dedup_adjacent_lines(block: &str) -> String {
    let had_trailing = block.ends_with('\n');
    let mut lines: Vec<&str> = block.split('\n').collect();
    if had_trailing {
        lines.pop();
    }
    let mut out_lines: Vec<&str> = Vec::with_capacity(lines.len());
    for l in lines {
        if out_lines.last() != Some(&l) {
            out_lines.push(l);
        }
    }
    let out = out_lines.join("\n");
    if had_trailing {
        format!("{out}\n")
    } else {
        out
    }
}

/// `:dedup-adjacent` — collapse consecutive duplicate lines in the selection
/// (Unix `uniq`), unlike `:uniquify-lines` which removes all duplicates.
fn dedup_adjacent(
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
    let (first, last) = primary_line_range(doc, view.id);
    let region_start = slice.line_to_char(first);
    let region_end = if last + 1 < total {
        slice.line_to_char(last + 1)
    } else {
        slice.len_chars()
    };
    if region_start >= region_end {
        return Ok(());
    }
    let block: String = slice.slice(region_start..region_end).chunks().collect();
    let deduped = dedup_adjacent_lines(&block);
    if deduped == block {
        return Ok(());
    }
    let transaction = Transaction::change(
        doc.text(),
        std::iter::once((region_start, region_end, Some(deduped.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Prepend `start`-based, right-aligned line numbers (`"{n}. "`) to each line of
/// `block`, preserving its trailing-newline shape. Pure — unit tested.
fn number_lines(block: &str, start: usize) -> String {
    let had_trailing = block.ends_with('\n');
    let mut lines: Vec<&str> = block.split('\n').collect();
    if had_trailing {
        lines.pop();
    }
    if lines.is_empty() {
        return block.to_string();
    }
    let width = (start + lines.len() - 1).to_string().len();
    let numbered: Vec<String> = lines
        .iter()
        .enumerate()
        .map(|(i, l)| format!("{:>width$}. {l}", start + i, width = width))
        .collect();
    let out = numbered.join("\n");
    if had_trailing {
        format!("{out}\n")
    } else {
        out
    }
}

/// `:number-lines [start]` — prepend line numbers (default starting at 1) to the
/// selected lines, like `nl`/`cat -n`.
fn number_lines_cmd(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let start: usize = args
        .first()
        .and_then(|a| a.trim().parse().ok())
        .unwrap_or(1);

    let (view, doc) = current!(cx.editor);
    let slice = doc.text().slice(..);
    let total = slice.len_lines();
    let (first, last) = primary_line_range(doc, view.id);
    let region_start = slice.line_to_char(first);
    let region_end = if last + 1 < total {
        slice.line_to_char(last + 1)
    } else {
        slice.len_chars()
    };
    if region_start >= region_end {
        return Ok(());
    }

    let block: String = slice.slice(region_start..region_end).chunks().collect();
    let numbered = number_lines(&block, start);
    if numbered == block {
        return Ok(());
    }
    let transaction = Transaction::change(
        doc.text(),
        std::iter::once((region_start, region_end, Some(numbered.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// A tiny recursive-descent evaluator for `:calc`. Grammar (lowest→highest):
/// expr = term (('+'|'-') term)*; term = power (('*'|'/'|'%') power)*;
/// power = factor ('^' power)?; factor = number | '(' expr ')' | ('+'|'-') factor.
/// Unary minus binds tighter than `^` (so `-2^2` == 4; use `-(2^2)` for -4).
struct Calc<'a> {
    s: &'a [u8],
    i: usize,
}

impl<'a> Calc<'a> {
    fn new(s: &'a str) -> Self {
        Calc {
            s: s.as_bytes(),
            i: 0,
        }
    }
    fn peek(&mut self) -> Option<u8> {
        while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
        self.s.get(self.i).copied()
    }
    fn eat(&mut self, c: u8) -> bool {
        if self.peek() == Some(c) {
            self.i += 1;
            true
        } else {
            false
        }
    }
    fn expr(&mut self) -> Result<f64, String> {
        let mut v = self.term()?;
        loop {
            match self.peek() {
                Some(b'+') => {
                    self.i += 1;
                    v += self.term()?;
                }
                Some(b'-') => {
                    self.i += 1;
                    v -= self.term()?;
                }
                _ => break,
            }
        }
        Ok(v)
    }
    fn term(&mut self) -> Result<f64, String> {
        let mut v = self.power()?;
        loop {
            match self.peek() {
                Some(b'*') => {
                    self.i += 1;
                    v *= self.power()?;
                }
                Some(b'/') => {
                    self.i += 1;
                    let d = self.power()?;
                    if d == 0.0 {
                        return Err("division by zero".into());
                    }
                    v /= d;
                }
                Some(b'%') => {
                    self.i += 1;
                    let d = self.power()?;
                    if d == 0.0 {
                        return Err("modulo by zero".into());
                    }
                    v %= d;
                }
                _ => break,
            }
        }
        Ok(v)
    }
    fn power(&mut self) -> Result<f64, String> {
        let base = self.factor()?;
        if self.eat(b'^') {
            Ok(base.powf(self.power()?)) // right-associative
        } else {
            Ok(base)
        }
    }
    fn factor(&mut self) -> Result<f64, String> {
        match self.peek() {
            Some(b'-') => {
                self.i += 1;
                Ok(-self.factor()?)
            }
            Some(b'+') => {
                self.i += 1;
                self.factor()
            }
            Some(b'(') => {
                self.i += 1;
                let v = self.expr()?;
                if !self.eat(b')') {
                    return Err("missing closing parenthesis".into());
                }
                Ok(v)
            }
            Some(c) if c.is_ascii_digit() || c == b'.' => self.number(),
            Some(c) => Err(format!("unexpected character '{}'", c as char)),
            None => Err("unexpected end of expression".into()),
        }
    }
    fn number(&mut self) -> Result<f64, String> {
        self.peek(); // skip whitespace
        let start = self.i;
        while self.i < self.s.len() {
            let c = self.s[self.i];
            if c.is_ascii_digit() || c == b'.' {
                self.i += 1;
            } else if (c == b'e' || c == b'E')
                && self.i + 1 < self.s.len()
                && (self.s[self.i + 1] == b'+' || self.s[self.i + 1] == b'-')
            {
                self.i += 2; // exponent with sign
            } else if c == b'e' || c == b'E' {
                self.i += 1;
            } else {
                break;
            }
        }
        let tok = std::str::from_utf8(&self.s[start..self.i]).unwrap_or("");
        tok.parse::<f64>()
            .map_err(|_| format!("invalid number '{tok}'"))
    }
}

/// Evaluate an arithmetic expression, erroring on trailing/garbage input.
fn eval_arith(input: &str) -> Result<f64, String> {
    let mut c = Calc::new(input);
    let v = c.expr()?;
    if c.peek().is_some() {
        return Err("unexpected trailing input".into());
    }
    if !v.is_finite() {
        return Err("result is not finite".into());
    }
    Ok(v)
}

/// Render a calc result: integers without a decimal point, otherwise trimmed.
fn format_calc(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        let s = format!("{v:.10}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

fn calc(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    let expr = args.join(" ");
    if !expr.trim().is_empty() {
        // `:calc <expr>` — show the result in the status line.
        match eval_arith(&expr) {
            Ok(v) => cx
                .editor
                .set_status(format!("{} = {}", expr.trim(), format_calc(v))),
            Err(e) => bail!("calc: {e}"),
        }
        return Ok(());
    }

    // No argument — evaluate each selection and replace it with the result.
    let (view, doc) = current!(cx.editor);
    let text = doc.text().slice(..);
    let selection = doc.selection(view.id);
    let mut first_err: Option<String> = None;
    let transaction = Transaction::change_by_selection(doc.text(), selection, |range| {
        let s: String = text.slice(range.from()..range.to()).chunks().collect();
        match eval_arith(&s) {
            Ok(v) => (range.from(), range.to(), Some(format_calc(v).into())),
            Err(e) => {
                if first_err.is_none() {
                    first_err = Some(e);
                }
                (range.from(), range.to(), None)
            }
        }
    });
    if let Some(e) = first_err {
        bail!("calc: {e}");
    }
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// Extract every numeric literal from `s` (integers, decimals, scientific, with
/// optional leading sign). A `+`/`-` counts as a sign only at a token boundary
/// (start, whitespace, or one of `([{,;:=`), so `1-2` reads as `[1, 2]` while a
/// column entry `-5` reads as `[-5]`. Pure — unit tested.
fn extract_numbers(s: &str) -> Vec<f64> {
    let b = s.as_bytes();
    let mut nums = Vec::new();
    let mut i = 0;
    let boundary = |c: u8| {
        matches!(
            c,
            b' ' | b'\t' | b'\n' | b'\r' | b'(' | b'[' | b'{' | b',' | b';' | b':' | b'='
        )
    };
    while i < b.len() {
        let c = b[i];
        let signed = (c == b'-' || c == b'+')
            && (i == 0 || boundary(b[i - 1]))
            && i + 1 < b.len()
            && (b[i + 1].is_ascii_digit()
                || (b[i + 1] == b'.' && i + 2 < b.len() && b[i + 2].is_ascii_digit()));
        let starts = c.is_ascii_digit()
            || (c == b'.' && i + 1 < b.len() && b[i + 1].is_ascii_digit())
            || signed;
        if !starts {
            i += 1;
            continue;
        }
        let start = i;
        if c == b'-' || c == b'+' {
            i += 1;
        }
        while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'.') {
            i += 1;
        }
        if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
            let mut j = i + 1;
            if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
                j += 1;
            }
            if j < b.len() && b[j].is_ascii_digit() {
                i = j;
                while i < b.len() && b[i].is_ascii_digit() {
                    i += 1;
                }
            }
        }
        if let Ok(v) = std::str::from_utf8(&b[start..i])
            .unwrap_or("")
            .parse::<f64>()
        {
            nums.push(v);
        }
    }
    nums
}

fn sum(cx: &mut compositor::Context, _args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    let (view, doc) = current!(cx.editor);
    let text = doc.text().slice(..);
    let sel = doc.selection(view.id).primary();
    let s: String = text.slice(sel.from()..sel.to()).chunks().collect();
    let nums = extract_numbers(&s);
    if nums.is_empty() {
        bail!("no numbers in selection");
    }

    let count = nums.len();
    let total: f64 = nums.iter().sum();
    let avg = total / count as f64;
    let min = nums.iter().cloned().fold(f64::INFINITY, f64::min);
    let max = nums.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    cx.editor.set_status(format!(
        "sum={} avg={} min={} max={} n={count}",
        format_calc(total),
        format_calc(avg),
        format_calc(min),
        format_calc(max),
    ));
    Ok(())
}

/// Format a Unix timestamp as a UTC `YYYY-MM-DD` date, optionally with a
/// ` HH:MM:SS` time. Pure — unit tested.
fn format_utc(secs: u64, with_time: bool) -> String {
    let days = (secs / 86_400) as i64;
    let tod = secs % 86_400;
    let (year, month, day) = crate::logging::civil_from_days(days);
    let date = format!("{year:04}-{month:02}-{day:02}");
    if with_time {
        format!(
            "{date} {:02}:{:02}:{:02}",
            tod / 3600,
            (tod % 3600) / 60,
            tod % 60
        )
    } else {
        date
    }
}

fn now_unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Insert `text` at every cursor (replacing any selection).
fn insert_at_cursors(cx: &mut compositor::Context, text: String) {
    let (view, doc) = current!(cx.editor);
    let selection = doc.selection(view.id).clone();
    let transaction = Transaction::change_by_selection(doc.text(), &selection, |range| {
        (range.from(), range.to(), Some(text.clone().into()))
    });
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
}

/// The classic lorem-ipsum word corpus (lowercase).
const LOREM_WORDS: &str = "lorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod \
tempor incididunt ut labore et dolore magna aliqua enim ad minim veniam quis nostrud exercitation \
ullamco laboris nisi aliquip ex ea commodo consequat duis aute irure in reprehenderit voluptate \
velit esse cillum eu fugiat nulla pariatur excepteur sint occaecat cupidatat non proident sunt \
culpa qui officia deserunt mollit anim id est laborum";

/// Generate `n` words of lorem ipsum (cycling the corpus if needed), as a single
/// sentence: first word capitalized, terminated with a period. Pure — unit tested.
fn lorem(n: usize) -> String {
    if n == 0 {
        return String::new();
    }
    let words: Vec<&str> = LOREM_WORDS.split_whitespace().collect();
    let mut out = String::new();
    for i in 0..n {
        if i > 0 {
            out.push(' ');
        }
        let w = words[i % words.len()];
        if i == 0 {
            let mut cs = w.chars();
            if let Some(first) = cs.next() {
                out.extend(first.to_uppercase());
                out.push_str(cs.as_str());
            }
        } else {
            out.push_str(w);
        }
    }
    out.push('.');
    out
}

/// `:lorem [n]` — insert `n` words (default 30) of lorem-ipsum placeholder text.
fn lorem_cmd(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let n: usize = args
        .first()
        .and_then(|a| a.trim().parse().ok())
        .unwrap_or(30);
    insert_at_cursors(cx, lorem(n));
    Ok(())
}

/// Clamp a target char offset to a valid cursor position in a buffer of `len`
/// chars (a cursor may sit on the final EOF position). Pure — unit tested.
fn clamp_offset(n: usize, len: usize) -> usize {
    n.min(len)
}

/// `:goto-offset <n>` — move the cursor to absolute character offset `n` (useful
/// when a tool reports a char offset). Clamped to the buffer.
fn goto_offset(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let n: usize = args
        .first()
        .and_then(|a| a.trim().parse().ok())
        .context("usage: :goto-offset <n>")?;
    let scrolloff = cx.editor.config().scrolloff;
    let (view, doc) = current!(cx.editor);
    let pos = clamp_offset(n, doc.text().len_chars());
    doc.set_selection(view.id, zemacs_core::Selection::point(pos));
    view.ensure_cursor_in_view(doc, scrolloff);
    Ok(())
}

/// Parse an integer in any common notation: `0x`/`0X` hex, `0b`/`0B` binary,
/// `0o`/`0O` octal, or plain decimal, with an optional leading `-`. Pure — unit tested.
fn parse_int_any(s: &str) -> Option<i64> {
    let t = s.trim();
    let (neg, t) = match t.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, t),
    };
    let v = if let Some(h) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        i64::from_str_radix(h, 16).ok()?
    } else if let Some(b) = t.strip_prefix("0b").or_else(|| t.strip_prefix("0B")) {
        i64::from_str_radix(b, 2).ok()?
    } else if let Some(o) = t.strip_prefix("0o").or_else(|| t.strip_prefix("0O")) {
        i64::from_str_radix(o, 8).ok()?
    } else {
        t.parse::<i64>().ok()?
    };
    Some(if neg { -v } else { v })
}

/// Format an integer's decimal/hex/octal/binary representations. Pure — unit tested.
fn format_bases(n: i64) -> String {
    format!("{n} = 0x{n:x} = 0o{n:o} = 0b{n:b}")
}

/// `:bases` — show the selected integer in decimal, hex, octal, and binary.
fn show_bases(cx: &mut compositor::Context, _args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let s = {
        let (view, doc) = current!(cx.editor);
        let text = doc.text().slice(..);
        let sel = doc.selection(view.id).primary();
        text.slice(sel.from()..sel.to())
            .chunks()
            .collect::<String>()
    };
    match parse_int_any(&s) {
        Some(n) => cx.editor.set_status(format_bases(n)),
        None => bail!("not an integer: {}", s.trim()),
    }
    Ok(())
}

/// Add `delta` to every unsigned-integer run in `s`, leaving the rest of the text
/// intact (e.g. `v1.2.3` +1 → `v2.3.4`). Pure — unit tested.
fn increment_numbers(s: &str, delta: i64) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_digit() {
            let start = i;
            while i < chars.len() && chars[i].is_ascii_digit() {
                i += 1;
            }
            let num: i64 = chars[start..i]
                .iter()
                .collect::<String>()
                .parse()
                .unwrap_or(0);
            out.push_str(&(num + delta).to_string());
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

/// Zero-pad every integer run in `s` to at least `width` digits (never truncates).
/// Pure — unit tested.
fn pad_numbers(s: &str, width: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_digit() {
            let start = i;
            while i < chars.len() && chars[i].is_ascii_digit() {
                i += 1;
            }
            let num: String = chars[start..i].iter().collect();
            out.push_str(&format!("{num:0>width$}"));
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

/// `:pad-numbers <width>` — zero-pad every integer in the selection to `width`
/// digits (so e.g. `1,2,10` sort correctly).
fn pad_numbers_cmd(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let width: usize = args
        .first()
        .and_then(|a| a.trim().parse().ok())
        .context("usage: :pad-numbers <width>")?;
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = pad_numbers(&s, width);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// `:increment-numbers [n]` — add `n` (default 1; negative to decrement) to every
/// integer in the selection.
fn increment_numbers_cmd(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let delta: i64 = args
        .first()
        .and_then(|a| a.trim().parse().ok())
        .unwrap_or(1);
    let (view, doc) = current!(cx.editor);
    let text = doc.text();
    let sel = doc.selection(view.id).primary();
    let s: String = text
        .slice(..)
        .slice(sel.from()..sel.to())
        .chunks()
        .collect();
    let new = increment_numbers(&s, delta);
    if new == s {
        return Ok(());
    }
    let transaction = Transaction::change(
        text,
        std::iter::once((sel.from(), sel.to(), Some(new.into()))),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    Ok(())
}

/// `:date` — insert the current UTC date (`YYYY-MM-DD`).
fn insert_date(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    insert_at_cursors(cx, format_utc(now_unix_secs(), false));
    Ok(())
}

/// `:datetime` — insert the current UTC date and time (`YYYY-MM-DD HH:MM:SS`).
fn insert_datetime(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    insert_at_cursors(cx, format_utc(now_unix_secs(), true));
    Ok(())
}

/// `:timestamp` — insert the current Unix epoch in seconds.
fn insert_timestamp(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    insert_at_cursors(cx, now_unix_secs().to_string());
    Ok(())
}

/// Format 16 random bytes as an RFC 4122 version-4 UUID string, setting the
/// version and variant bits. Pure — unit tested.
fn format_uuid_v4(mut b: [u8; 16]) -> String {
    b[6] = (b[6] & 0x0f) | 0x40; // version 4
    b[8] = (b[8] & 0x3f) | 0x80; // variant 10xx
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]
    )
}

/// 16 bytes from the OS RNG (`/dev/urandom`), with a time-seeded xorshift fallback.
fn random_bytes_16() -> [u8; 16] {
    use std::io::Read;
    let mut buf = [0u8; 16];
    if std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut buf))
        .is_ok()
    {
        return buf;
    }
    let mut x = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
        ^ (&buf as *const _ as u64)
        | 1;
    for byte in buf.iter_mut() {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *byte = (x >> 24) as u8;
    }
    buf
}

/// `:uuid` — insert a fresh random UUID v4 at each cursor (replacing any selection).
fn insert_uuid(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current!(cx.editor);
    let selection = doc.selection(view.id).clone();
    let transaction = Transaction::change_by_selection(doc.text(), &selection, |range| {
        let uuid = format_uuid_v4(random_bytes_16());
        (range.from(), range.to(), Some(uuid.into()))
    });
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

    let transaction = Transaction::change(
        doc.text(),
        std::iter::once((w1_start, w2_end, Some(swapped))),
    );
    doc.apply(&transaction, view.id);
    // Point lands after the transposed pair (emacs behaviour). Region length is
    // unchanged, so w2_end is still the end of the swapped span.
    doc.set_selection(
        view.id,
        Selection::point(w2_end.min(doc.text().len_chars())),
    );
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

    cx.editor
        .open(&zemacs_loader::log_file(), Action::Replace)?;
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

/// `:elisp <code>` / `:eval-expression` — evaluate Emacs Lisp against the live
/// editor via the embedded elisprs interpreter. The result is shown on the
/// status line.
fn elisp_eval(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let code = args.join(" ");
    if code.trim().is_empty() {
        return Ok(());
    }
    match crate::commands::scripting::eval_elisp(cx, &code) {
        Ok(result) => cx.editor.set_status(format!("=> {result}")),
        Err(err) => cx.editor.set_error(format!("elisp: {err}")),
    }
    Ok(())
}

/// `:vim <code>` / `:viml` — evaluate Vimscript against the editor via the
/// embedded vimlrs interpreter. Captured `:echo` output and the trailing
/// expression value are shown on the status line.
fn viml_eval(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let code = args.join(" ");
    if code.trim().is_empty() {
        return Ok(());
    }
    match crate::commands::scripting::eval_viml(cx, &code) {
        Ok(result) if result.trim().is_empty() => cx.editor.set_status("ok"),
        Ok(result) => cx.editor.set_status(result),
        Err(err) => cx.editor.set_error(format!("viml: {err}")),
    }
    Ok(())
}

/// `:awk <program>` — filter the selection (or whole buffer if no selection)
/// through an awk program via the embedded awkrs interpreter, replacing it with
/// the program's output.
fn awk_filter(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let program = args.join(" ");
    if program.trim().is_empty() {
        return Ok(());
    }
    match crate::commands::scripting::run_awk_filter(cx, &program) {
        Ok(msg) => cx.editor.set_status(msg),
        Err(err) => cx.editor.set_error(format!("awk: {err}")),
    }
    Ok(())
}

/// `:zsh <command>` — run a shell command in the embedded zshrs interpreter
/// (state persists across calls) and show its captured output in a popup.
fn zsh_run(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let cmd = args.join(" ");
    if cmd.trim().is_empty() {
        return Ok(());
    }
    match crate::commands::scripting::run_zsh(&cmd) {
        Ok((status, output)) if output.trim().is_empty() => {
            cx.editor.set_status(format!("zsh: exit {status}"));
        }
        Ok((status, output)) => {
            let callback = async move {
                let call: job::Callback = Callback::EditorCompositor(Box::new(
                    move |editor: &mut Editor, compositor: &mut Compositor| {
                        let contents = ui::Markdown::new(
                            format!("```\n{}\n```", output.trim_end()),
                            editor.syn_loader.clone(),
                        );
                        let popup =
                            Popup::new("zsh", contents).position(Some(zemacs_core::Position::new(
                                editor.cursor().0.unwrap_or_default().row,
                                2,
                            )));
                        compositor.replace_or_push("zsh", popup);
                        editor.set_status(format!("zsh: exit {status}"));
                    },
                ));
                Ok(call)
            };
            cx.jobs.callback(callback);
        }
        Err(err) => cx.editor.set_error(format!("zsh: {err}")),
    }
    Ok(())
}

/// `:stryke <code>` — evaluate stryke (strykelang) source via the embedded
/// interpreter. Shows captured `print` output or the last expression value on
/// the status line; state persists across calls.
fn stryke_eval(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let code = args.join(" ");
    if code.trim().is_empty() {
        return Ok(());
    }
    match crate::commands::scripting::eval_stryke(cx, &code) {
        Ok(result) if result.trim().is_empty() => cx.editor.set_status("ok"),
        Ok(result) => cx.editor.set_status(result),
        Err(err) => cx.editor.set_error(format!("stryke: {err}")),
    }
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

/// Delete the current buffer's file from disk and close the buffer
/// (vim-eunuch `:Delete`).
fn delete_file(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    let doc = doc!(cx.editor);
    let path = doc
        .path()
        .map(ToOwned::to_owned)
        .context("Scratch buffer has no file to delete")?;
    let doc_id = doc.id();

    if !path.exists() {
        bail!("file does not exist: {}", path.display());
    }
    std::fs::remove_file(&path).map_err(|err| anyhow!("could not delete file: {err}"))?;

    // Close the now-orphaned buffer (force: its backing file is gone).
    buffer_close_by_ids_impl(cx, &[doc_id], true)?;
    cx.editor.set_status(format!("Deleted {}", path.display()));
    Ok(())
}

/// Resolve the directory `:mkdir` should create: the explicit argument if given,
/// otherwise the parent directory of the current buffer's file. Pure — unit tested.
fn resolve_mkdir_target(
    arg: Option<&str>,
    current_file: Option<&std::path::Path>,
) -> Option<PathBuf> {
    match arg.map(str::trim).filter(|a| !a.is_empty()) {
        Some(path) => Some(PathBuf::from(path)),
        None => current_file
            .and_then(|f| f.parent())
            .filter(|p| !p.as_os_str().is_empty())
            .map(ToOwned::to_owned),
    }
}

/// Create a directory (and any missing parents), like vim-eunuch `:Mkdir`.
/// With no argument, creates the parent directory of the current buffer's file.
fn mkdir(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    let current = doc!(cx.editor).path().map(ToOwned::to_owned);
    let target = resolve_mkdir_target(args.first(), current.as_deref())
        .context("no path argument and current buffer has no file")?;

    std::fs::create_dir_all(&target).map_err(|err| anyhow!("could not create directory: {err}"))?;
    cx.editor
        .set_status(format!("Created {}", target.display()));
    Ok(())
}

/// Add the execute bit (`a+x`, i.e. `0o111`) to a Unix permission mode. Pure —
/// unit tested.
fn with_exec_bits(mode: u32) -> u32 {
    mode | 0o111
}

/// `:chmod-x` — make the current buffer's file executable (`chmod a+x`, vim-eunuch
/// `:Chmod +x`). Unix only.
fn chmod_x(cx: &mut compositor::Context, _args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let path = doc!(cx.editor)
        .path()
        .map(ToOwned::to_owned)
        .context("buffer has no file")?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&path)?.permissions();
        perms.set_mode(with_exec_bits(perms.mode()));
        std::fs::set_permissions(&path, perms)
            .map_err(|err| anyhow!("could not set permissions: {err}"))?;
        cx.editor
            .set_status(format!("made executable: {}", path.display()));
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        bail!("chmod is only supported on Unix");
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

// Script commands (`:elisp`, `:vim`) take the entire remainder verbatim (one
// raw positional) so string literals and whitespace inside the code survive.
pub const ELISP_SIGNATURE: Signature = Signature {
    positionals: (1, Some(1)),
    raw_after: Some(0),
    ..Signature::DEFAULT
};

pub const VIML_SIGNATURE: Signature = ELISP_SIGNATURE;
pub const AWK_SIGNATURE: Signature = ELISP_SIGNATURE;
pub const ZSH_SIGNATURE: Signature = ELISP_SIGNATURE;
pub const STRYKE_SIGNATURE: Signature = ELISP_SIGNATURE;

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
        name: "terminal",
        aliases: &["term"],
        doc: "Open an integrated terminal (PTY shell) running $SHELL.",
        fun: terminal,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "ide",
        aliases: &["workbench"],
        doc: "Enter IDE mode (file-tree sidebar + panels, like `--ide` / F2).",
        fun: ide,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "diff",
        aliases: &["gdiff"],
        doc: "Open a read-only side-by-side diff of the buffer vs. its git HEAD version.",
        fun: diff,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "merge",
        aliases: &["resolve"],
        doc: "Resolve the buffer's git merge conflicts in a 3-pane (ours/result/theirs) view.",
        fun: merge,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "magit",
        aliases: &["git", "gst"],
        doc: "Open the Magit-style git status (stage/unstage/discard/commit changes by section).",
        fun: magit,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "hex",
        aliases: &["hexview", "hexedit"],
        doc: "Open a read-only xxd-style hex viewer of a file's raw bytes (optional path; defaults to the buffer's file).",
        fun: hex,
        completer: CommandCompleter::positional(&[completers::filename]),
        signature: Signature {
            positionals: (0, Some(1)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "snippets",
        aliases: &["snip"],
        doc: "Open the user snippet library editor (create/edit/delete reusable snippets).",
        fun: snippets,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "org-cycle",
        aliases: &["org-fold"],
        doc: "Toggle a fold over the current org heading's subtree (TAB-style outline cycling).",
        fun: org_cycle,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "org-todo",
        aliases: &[],
        doc: "Cycle the current org heading's TODO keyword: none -> TODO -> DONE -> none.",
        fun: org_todo,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "org-promote",
        aliases: &[],
        doc: "Promote the current org heading one level (remove a leading star).",
        fun: org_promote,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "org-demote",
        aliases: &[],
        doc: "Demote the current org heading one level (add a leading star).",
        fun: org_demote,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "org-next-heading",
        aliases: &[],
        doc: "Move the cursor to the next org heading line.",
        fun: org_next_heading,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "org-prev-heading",
        aliases: &[],
        doc: "Move the cursor to the previous org heading line.",
        fun: org_prev_heading,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "org-fold-all",
        aliases: &[],
        doc: "Fold every org heading subtree in the buffer.",
        fun: org_fold_all,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "org-unfold-all",
        aliases: &[],
        doc: "Unfold every fold in the buffer.",
        fun: org_unfold_all,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "org-agenda",
        aliases: &["agenda"],
        doc: "Open the org agenda: TODO/DONE items across open .org buffers and *.org files under the working directory, grouped by scheduled/deadline date.",
        fun: org_agenda,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "org-priority",
        aliases: &[],
        doc: "Cycle the current org heading's priority cookie: none -> [#A] -> [#B] -> [#C] -> none.",
        fun: org_priority,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "org-capture",
        aliases: &["capture"],
        doc: "Prompt for a line of text and append it as a '* TODO <text>' entry to an inbox org file (default <working-dir>/inbox.org, or an explicit path argument).",
        fun: org_capture,
        completer: CommandCompleter::positional(&[completers::filename]),
        signature: Signature {
            positionals: (0, Some(1)),
            ..Signature::DEFAULT
        },
    },
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
        name: "help",
        aliases: &["h"],
        doc: "Open the inline Help browser (searchable: commands, keybindings, topics).",
        fun: open_help,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(1)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "wc",
        aliases: &["words", "count"],
        doc: "Show document line/word/char counts (and selection stats).",
        fun: word_count,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "blame",
        aliases: &[],
        doc: "Show git blame for the current line in the status bar.",
        fun: blame_line,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "reopen",
        aliases: &["reopen-closed"],
        doc: "Reopen the most recently closed file.",
        fun: reopen_closed,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "zen",
        aliases: &[],
        doc: "Toggle the IDE workbench (Zen / focus mode).",
        fun: zen_mode,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "emmet",
        aliases: &["zencode"],
        doc: "Expand the emmet/zen HTML abbreviation before the cursor.",
        fun: emmet_expand_cmd,
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
        name: "nohlsearch",
        aliases: &["noh", "nohl"],
        doc: "Clear the persistent search highlight (vim :nohlsearch).",
        fun: no_highlight_search,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "clearjumps",
        aliases: &[],
        doc: "Clear the current view's jump list (vim :clearjumps).",
        fun: clear_jumps,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "project-replace",
        aliases: &["replace-in-files"],
        doc: "Regex-replace across all matching workspace files (JetBrains Replace in Files).",
        fun: project_replace,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (2, Some(2)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "buffers",
        aliases: &["ls", "files"],
        doc: "List open buffers in the buffer picker (vim :buffers/:ls/:files).",
        fun: buffers_list,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "jumps",
        aliases: &[],
        doc: "List the jump list in a picker (vim :jumps).",
        fun: jumps_list,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "oldfiles",
        aliases: &[],
        doc: "Pick from recently edited files (vim :oldfiles).",
        fun: oldfiles_list,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "marks",
        aliases: &[],
        doc: "List the buffer's marks in a picker (vim :marks).",
        fun: marks_list,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "history",
        aliases: &[],
        doc: "Pick from the command-line history (vim :history).",
        fun: history_list,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(1)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "delmarks",
        aliases: &["delm"],
        doc: "Delete the listed named marks (vim :delmarks abc).",
        fun: del_marks,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (1, None),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "delmarks!",
        aliases: &["delm!"],
        doc: "Delete all letter marks (vim :delmarks!).",
        fun: del_marks_all,
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
        name: "grep",
        aliases: &["rg", "search-project"],
        doc: "Search the project (ripgrep) and show jumpable results in the Run console.",
        fun: grep,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, None),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "shell-quote",
        aliases: &["sh-quote"],
        doc: "Wrap the selection in safe shell single-quotes.",
        fun: shell_quote_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "wrap-tag",
        aliases: &["tag"],
        doc: "Wrap each selection in <tag>…</tag>.",
        fun: wrap_tag_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (1, Some(1)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "csv-column",
        aliases: &["csv-col"],
        doc: "Replace the selected CSV/TSV with just its Nth column (1-based).",
        fun: csv_column_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (1, Some(1)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "code-fence",
        aliases: &["fence"],
        doc: "Wrap the selection in a fenced Markdown code block with optional language.",
        fun: code_fence_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(1)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "md-table",
        aliases: &["table-fmt"],
        doc: "Align the selected Markdown pipe table (pad columns, rebuild separator row).",
        fun: md_table_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "json-query",
        aliases: &["json-get"],
        doc: "Replace the selected JSON with the value at a dot-path (e.g. users.0.name).",
        fun: json_query_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (1, Some(1)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "json-flatten",
        aliases: &["json-paths"],
        doc: "Flatten the selected JSON into greppable `path = value` lines.",
        fun: json_flatten_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "json-to-csv",
        aliases: &["json-csv"],
        doc: "Convert the selected JSON array of objects to CSV (sorted header union).",
        fun: json_to_csv_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "json-unflatten",
        aliases: &["json-unpaths"],
        doc: "Rebuild nested JSON from `path = value` lines (inverse of :json-flatten).",
        fun: json_unflatten_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "toml-to-json",
        aliases: &["toml-json"],
        doc: "Convert the selected TOML to pretty-printed JSON.",
        fun: toml_to_json_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "json-to-toml",
        aliases: &["json-toml"],
        doc: "Convert the selected JSON to pretty-printed TOML.",
        fun: json_to_toml_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "json-sort",
        aliases: &["json-sort-array"],
        doc: "Sort the selected JSON array (optionally by an object field: :json-sort name).",
        fun: json_sort_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(1)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "json-pick",
        aliases: &["json-select"],
        doc: "Keep only the named keys in the selected JSON object/array (e.g. :json-pick name age).",
        fun: json_pick_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (1, None),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "json-omit",
        aliases: &["json-drop"],
        doc: "Drop the named keys from the selected JSON object/array (e.g. :json-omit password).",
        fun: json_omit_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (1, None),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "json-unique",
        aliases: &["json-uniq"],
        doc: "Remove duplicate elements from the selected JSON array (optionally by a field).",
        fun: json_unique_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(1)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "json-group-by",
        aliases: &["json-group"],
        doc: "Group the selected JSON array of objects by a field (e.g. :json-group-by city).",
        fun: json_group_by_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (1, Some(1)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "extract",
        aliases: &["matches"],
        doc: "Replace the selection with every regex match, one per line (group 1 if present).",
        fun: extract_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (1, None),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "filter",
        aliases: &["keep-lines"],
        doc: "Keep only the selected lines matching a regex (in-buffer grep).",
        fun: filter_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (1, None),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "reject",
        aliases: &["remove-lines"],
        doc: "Drop the selected lines matching a regex (in-buffer grep -v).",
        fun: reject_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (1, None),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "count-matches",
        aliases: &["count-regex"],
        doc: "Report how many regex matches (and matching lines) are in the selection.",
        fun: count_matches_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (1, None),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "uniq-count",
        aliases: &["frequency"],
        doc: "Collapse the selected lines to `count line`, sorted by frequency (uniq -c | sort -rn).",
        fun: uniq_count_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "stats",
        aliases: &["describe"],
        doc: "Show count/sum/mean/min/max of the numbers in the selection (non-destructive).",
        fun: stats_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "seq",
        aliases: &["sequence"],
        doc: "Insert an integer sequence, one per line: :seq <start> <end> [step].",
        fun: seq_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (2, Some(3)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "field",
        aliases: &["cut"],
        doc: "Keep only the Nth whitespace field of each selected line (awk '{print $N}').",
        fun: field_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (1, Some(1)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "running-total",
        aliases: &["cumsum"],
        doc: "Replace each numeric line with the cumulative total so far.",
        fun: running_total_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "diff-lines",
        aliases: &["deltas"],
        doc: "Replace each numeric line with its delta from the previous (inverse of running-total).",
        fun: diff_lines_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "sum-column",
        aliases: &["sumcol"],
        doc: "Sum the Nth whitespace field across the selected lines (non-destructive).",
        fun: sum_column_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (1, Some(1)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "shuffle",
        aliases: &["shuf"],
        doc: "Randomly reorder the selected lines (Fisher-Yates).",
        fun: shuffle_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "sample",
        aliases: &["random-lines"],
        doc: "Keep N random lines from the selection, preserving order (:sample 10).",
        fun: sample_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (1, Some(1)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "jsonl-to-json",
        aliases: &["jsonl-json"],
        doc: "Convert the selected JSONL/NDJSON (one value per line) to a JSON array.",
        fun: jsonl_to_json_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "json-to-jsonl",
        aliases: &["json-jsonl"],
        doc: "Convert the selected JSON array to JSONL (one compact value per line).",
        fun: json_to_jsonl_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "head",
        aliases: &["first-lines"],
        doc: "Keep only the first N lines of the selection (:head 10).",
        fun: head_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (1, Some(1)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "tail",
        aliases: &["last-lines"],
        doc: "Keep only the last N lines of the selection (:tail 10).",
        fun: tail_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (1, Some(1)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "rev",
        aliases: &["reverse-each-line"],
        doc: "Reverse the characters of each selected line independently (Unix rev).",
        fun: rev_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "json-table",
        aliases: &["json-tbl"],
        doc: "Render the selected JSON array of objects as an aligned plain-text table.",
        fun: json_table_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "hexdump",
        aliases: &["xxd"],
        doc: "Render the selection as an xxd-style hex dump (offset, hex bytes, ASCII).",
        fun: hexdump_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "dedup",
        aliases: &["unique-lines"],
        doc: "Remove all duplicate lines globally, keeping first occurrence and order.",
        fun: dedup_all_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "caesar",
        aliases: &["shift-letters"],
        doc: "Caesar-shift the selection's letters by N (e.g. :caesar 13 = ROT13; N may be negative).",
        fun: caesar_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (1, Some(1)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "base32-encode",
        aliases: &["base32"],
        doc: "Base32-encode the selection (RFC 4648).",
        fun: base32_encode_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "base32-decode",
        aliases: &["unbase32"],
        doc: "Base32-decode the selection (RFC 4648).",
        fun: base32_decode_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "crc32",
        aliases: &["checksum"],
        doc: "Show the CRC32 (IEEE) checksum of the selection in hex and decimal (non-destructive).",
        fun: crc32_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "rot47",
        aliases: &["rot-47"],
        doc: "Apply ROT47 to the selection (rotates all printable ASCII; self-inverse).",
        fun: rot47_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "morse-encode",
        aliases: &["morse"],
        doc: "Encode the selection (A-Z, 0-9) to Morse code (words separated by /).",
        fun: morse_encode_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "morse-decode",
        aliases: &["unmorse"],
        doc: "Decode Morse code in the selection back to text.",
        fun: morse_decode_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "human-bytes",
        aliases: &["humanize-size"],
        doc: "Convert each numeric line (a byte count) to a human-readable size like 1.5 KiB.",
        fun: human_bytes_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "ordinal",
        aliases: &["ordinalize"],
        doc: "Convert each numeric line to its ordinal (1 → 1st, 22 → 22nd).",
        fun: ordinal_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "to-snake",
        aliases: &["snake-case"],
        doc: "Convert the selected identifier to snake_case.",
        fun: to_snake_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "to-kebab",
        aliases: &["kebab-case"],
        doc: "Convert the selected identifier to kebab-case.",
        fun: to_kebab_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "to-camel",
        aliases: &["camel-case"],
        doc: "Convert the selected identifier to camelCase.",
        fun: to_camel_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "to-pascal",
        aliases: &["pascal-case"],
        doc: "Convert the selected identifier to PascalCase.",
        fun: to_pascal_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "to-constant",
        aliases: &["screaming-snake", "upper-snake"],
        doc: "Convert the selected identifier to CONSTANT_CASE.",
        fun: to_constant_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "to-binary",
        aliases: &["text-to-binary"],
        doc: "Convert the selection to space-separated 8-bit binary.",
        fun: to_binary_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "from-binary",
        aliases: &["binary-to-text"],
        doc: "Convert space-separated binary in the selection back to text.",
        fun: from_binary_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "natural-sort",
        aliases: &["sort-natural"],
        doc: "Sort the selected lines in natural order (file2 before file10).",
        fun: natural_sort_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "pad-right",
        aliases: &["ljust"],
        doc: "Left-justify each selected line, padding with spaces to a minimum width.",
        fun: pad_right_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (1, Some(1)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "pad-left",
        aliases: &["rjust"],
        doc: "Right-justify each selected line, padding with spaces to a minimum width.",
        fun: pad_left_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (1, Some(1)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "json-keys",
        aliases: &["json-fields"],
        doc: "List the keys of the selected JSON object (or union across an array of objects).",
        fun: json_keys_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "json-type",
        aliases: &["json-describe"],
        doc: "Show the JSON type and size of the selection in the status line (non-destructive).",
        fun: json_type_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "after",
        aliases: &["cut-after"],
        doc: "Keep the text after the first <delimiter> on each selected line.",
        fun: after_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (1, None),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "before",
        aliases: &["cut-before"],
        doc: "Keep the text before the first <delimiter> on each selected line.",
        fun: before_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (1, None),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "swapcase",
        aliases: &["invert-case"],
        doc: "Invert the case of each character in the selection (Hello → hELLO).",
        fun: swapcase_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "strip-invisible",
        aliases: &["strip-zero-width"],
        doc: "Remove zero-width / invisible Unicode characters from the selection.",
        fun: strip_invisible_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "lines-to-json",
        aliases: &["lines-to-array"],
        doc: "Wrap the selected lines into a JSON array of strings.",
        fun: lines_to_json_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "json-to-lines",
        aliases: &["array-to-lines"],
        doc: "Unwrap a JSON array in the selection into one line per element.",
        fun: json_to_lines_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "checkbox-list",
        aliases: &["task-list"],
        doc: "Turn the selected lines into a Markdown task list (- [ ] item).",
        fun: checkbox_list_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "unwrap-paragraphs",
        aliases: &["unhardwrap"],
        doc: "Join hard-wrapped lines within each paragraph into single lines.",
        fun: unwrap_paragraphs_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "sql-in",
        aliases: &["sql-in-list"],
        doc: "Build a SQL IN-list ('a', 'b', 'c') from the selected lines.",
        fun: sql_in_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "dec-to-hex",
        aliases: &["to-hex-num"],
        doc: "Convert each decimal number line to hexadecimal.",
        fun: dec_to_hex_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "hex-to-dec",
        aliases: &["from-hex-num"],
        doc: "Convert each hexadecimal number line to decimal.",
        fun: hex_to_dec_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "unicode-escape",
        aliases: &["u-escape"],
        doc: "Escape non-ASCII characters in the selection as \\u{XXXX}.",
        fun: unicode_escape_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "unicode-unescape",
        aliases: &["u-unescape"],
        doc: "Decode \\u{XXXX} and \\uXXXX escapes in the selection back to characters.",
        fun: unicode_unescape_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "sort-by-length",
        aliases: &["sortlen"],
        doc: "Sort the selected lines by length (shortest first).",
        fun: sort_by_length_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "count-unique",
        aliases: &["distinct-count"],
        doc: "Report the number of distinct vs total selected lines (non-destructive).",
        fun: count_unique_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "rotate-lines",
        aliases: &["rotate"],
        doc: "Cyclically rotate the selected lines by N (negative rotates the other way).",
        fun: rotate_lines_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (1, Some(1)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "unquote-lines",
        aliases: &["strip-quotes-lines"],
        doc: "Remove surrounding quotes from each selected line independently.",
        fun: unquote_lines_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "quote-lines",
        aliases: &["quote-each"],
        doc: "Wrap each selected line in double quotes (escaping \\ and \").",
        fun: quote_lines_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "repeat",
        aliases: &["repeat-text"],
        doc: "Repeat the selected text N times (:repeat 3).",
        fun: repeat_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (1, Some(1)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "capitalize-lines",
        aliases: &["capitalize"],
        doc: "Uppercase the first letter of each selected line.",
        fun: capitalize_lines_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "remove-blank-lines",
        aliases: &["remove-empty"],
        doc: "Remove all blank (whitespace-only) lines from the selection.",
        fun: remove_blank_lines_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "trim-lines",
        aliases: &["trim"],
        doc: "Trim leading and trailing whitespace from each selected line.",
        fun: trim_lines_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "kv-to-json",
        aliases: &["env-to-json"],
        doc: "Convert key=value / key: value lines in the selection to a JSON object.",
        fun: kv_to_json_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "json-to-kv",
        aliases: &["json-to-env"],
        doc: "Convert the selected JSON object to key=value lines.",
        fun: json_to_kv_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "json-pluck",
        aliases: &["json-values-of"],
        doc: "Extract one field's value from each object in a JSON array, one per line.",
        fun: json_pluck_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (1, Some(1)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "to-html-list",
        aliases: &["html-list"],
        doc: "Convert the selected lines into an HTML <ul> list.",
        fun: to_html_list_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "from-html-list",
        aliases: &["html-list-to-lines"],
        doc: "Extract <li> item text from an HTML list in the selection, one per line.",
        fun: from_html_list_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "csv-to-html-table",
        aliases: &["csv-to-html"],
        doc: "Convert the selected CSV/TSV (first row = headers) to an HTML <table>.",
        fun: csv_to_html_table_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "slugify-lines",
        aliases: &["slug-lines"],
        doc: "Slugify each selected line independently (URL-friendly).",
        fun: slugify_lines_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "lines-to-csv-row",
        aliases: &["join-csv"],
        doc: "Join the selected lines into one CSV row (RFC-4180 quoting).",
        fun: lines_to_csv_row_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "csv-row-to-lines",
        aliases: &["split-csv"],
        doc: "Split a CSV row in the selection into one field per line (quote-aware).",
        fun: csv_row_to_lines_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "deslugify",
        aliases: &["unslugify"],
        doc: "Turn a slug back into a Title Cased phrase (hyphens/underscores to spaces).",
        fun: deslugify_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "csv-to-tsv",
        aliases: &["csv-tsv"],
        doc: "Convert the selected CSV to tab-separated values (quote-aware).",
        fun: csv_to_tsv_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "tsv-to-csv",
        aliases: &["tsv-csv"],
        doc: "Convert the selected TSV to CSV (RFC-4180 quoting).",
        fun: tsv_to_csv_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "strip-line-numbers",
        aliases: &["unnumber"],
        doc: "Remove a leading line number (and separator) from each selected line.",
        fun: strip_line_numbers_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "markdown-link",
        aliases: &["md-link"],
        doc: "Wrap the selected text as a Markdown link [text](url).",
        fun: markdown_link_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (1, None),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "extract-urls",
        aliases: &["urls"],
        doc: "Replace the selection with the http(s) URLs found in it, one per line.",
        fun: extract_urls_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "extract-emails",
        aliases: &["emails"],
        doc: "Replace the selection with the email addresses found in it, one per line.",
        fun: extract_emails_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "extract-ips",
        aliases: &["ips"],
        doc: "Replace the selection with the IPv4 addresses found in it, one per line.",
        fun: extract_ips_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "extract-quoted",
        aliases: &["quoted-strings"],
        doc: "Replace the selection with the contents of double-quoted strings, one per line.",
        fun: extract_quoted_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "extract-between",
        aliases: &["between"],
        doc: "Extract substrings between <start> and <end> delimiters, one per line.",
        fun: extract_between_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (2, Some(2)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "wrap-with",
        aliases: &["surround-with"],
        doc: "Wrap the selection with the given text on both sides (:wrap-with **).",
        fun: wrap_with_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (1, None),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "extract-numbers",
        aliases: &["numbers"],
        doc: "Replace the selection with the numbers found in it, one per line.",
        fun: extract_numbers_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "json-validate",
        aliases: &["json-check"],
        doc: "Report whether the selection is valid JSON (with error location) — non-destructive.",
        fun: json_validate_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "csv-validate",
        aliases: &["csv-check"],
        doc: "Check all CSV rows have the same field count (non-destructive).",
        fun: csv_validate_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "ordered-list",
        aliases: &["numbered-list"],
        doc: "Turn the selected lines into a Markdown ordered list (1. 2. 3.).",
        fun: ordered_list_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "strip-list-markers",
        aliases: &["unlist"],
        doc: "Strip leading bullet/number/checkbox list markers from each selected line.",
        fun: strip_list_markers_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "sort-words",
        aliases: &["sort-fields"],
        doc: "Sort the whitespace-separated words within each selected line.",
        fun: sort_words_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "unique-words",
        aliases: &["dedup-words"],
        doc: "Remove duplicate words within each selected line (first occurrence kept).",
        fun: unique_words_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "sum-fields",
        aliases: &["row-sum"],
        doc: "Replace each line with the sum of its numeric fields (row total).",
        fun: sum_fields_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "avg-fields",
        aliases: &["row-avg"],
        doc: "Replace each line with the mean of its numeric fields.",
        fun: avg_fields_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "max-fields",
        aliases: &["row-max"],
        doc: "Replace each line with the maximum of its numeric fields.",
        fun: max_fields_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "min-fields",
        aliases: &["row-min"],
        doc: "Replace each line with the minimum of its numeric fields.",
        fun: min_fields_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "range-fields",
        aliases: &["row-range"],
        doc: "Replace each line with the range (max - min) of its numeric fields.",
        fun: range_fields_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "to-env-export",
        aliases: &["export-vars"],
        doc: "Prefix each KEY=value line with `export ` (turn a .env into shell exports).",
        fun: to_env_export_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "strip-export",
        aliases: &["unexport"],
        doc: "Remove a leading `export ` from each selected line.",
        fun: strip_export_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "dos2unix",
        aliases: &["crlf-to-lf"],
        doc: "Convert CRLF/CR line endings in the selection to LF.",
        fun: dos2unix_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "unix2dos",
        aliases: &["lf-to-crlf"],
        doc: "Convert LF line endings in the selection to CRLF.",
        fun: unix2dos_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "percent-of-total",
        aliases: &["percentages"],
        doc: "Replace each numeric line with its percentage of the column total.",
        fun: percent_of_total_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "running-max",
        aliases: &["cummax"],
        doc: "Replace each numeric line with the running maximum so far.",
        fun: running_max_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "running-min",
        aliases: &["cummin"],
        doc: "Replace each numeric line with the running minimum so far.",
        fun: running_min_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "to-fixed",
        aliases: &["round-to"],
        doc: "Format each numeric line to N decimal places (:to-fixed 2).",
        fun: to_fixed_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (1, Some(1)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "clamp",
        aliases: &["clip"],
        doc: "Clamp each numeric line to the [min, max] range (:clamp 0 100).",
        fun: clamp_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (2, Some(2)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "scale",
        aliases: &["multiply-by"],
        doc: "Multiply each numeric line by a factor (:scale 1.5).",
        fun: scale_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (1, Some(1)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "offset",
        aliases: &["add-to-each"],
        doc: "Add N to each numeric line (:offset 10; negative subtracts).",
        fun: offset_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (1, Some(1)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "abs",
        aliases: &["absolute-value"],
        doc: "Replace each numeric line with its absolute value.",
        fun: abs_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "linkify",
        aliases: &["auto-link"],
        doc: "Wrap bare URLs in the selection with Markdown link syntax [url](url).",
        fun: linkify_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "strip-markdown-links",
        aliases: &["unlink"],
        doc: "Replace [text](url) Markdown links with just their text.",
        fun: strip_markdown_links_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "strip-emphasis",
        aliases: &["strip-md-emphasis"],
        doc: "Remove Markdown bold/italic/code emphasis markers from the selection.",
        fun: strip_emphasis_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "strip-html-comments",
        aliases: &["strip-comments-html"],
        doc: "Remove <!-- ... --> HTML/Markdown comments from the selection.",
        fun: strip_html_comments_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "remove-trailing-commas",
        aliases: &["fix-trailing-commas"],
        doc: "Remove trailing commas before } or ] (JSON5/JS to strict JSON).",
        fun: remove_trailing_commas_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "add-trailing-commas",
        aliases: &["trailing-commas"],
        doc: "Add trailing commas before } or ] (cleaner JS/JSON5 diffs).",
        fun: add_trailing_commas_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "smart-quotes",
        aliases: &["curly-quotes"],
        doc: "Convert straight quotes to typographic curly quotes (context-aware).",
        fun: smart_quotes_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "typographic-dashes",
        aliases: &["em-dash"],
        doc: "Convert --- to em dash, -- to en dash, ... to ellipsis.",
        fun: typographic_dashes_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "de-typography",
        aliases: &["ascii-punctuation"],
        doc: "Normalize curly quotes/dashes/ellipsis back to ASCII punctuation.",
        fun: de_typography_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "to-ascii",
        aliases: &["transliterate"],
        doc: "Transliterate accented Latin characters to ASCII (café → cafe).",
        fun: to_ascii_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "nato",
        aliases: &["phonetic"],
        doc: "Spell the selection in the NATO phonetic alphabet (A → Alfa).",
        fun: nato_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "transpose-grid",
        aliases: &["transpose-ws"],
        doc: "Transpose a whitespace-separated grid (rows become columns).",
        fun: transpose_grid_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "repeat-lines",
        aliases: &["duplicate-each"],
        doc: "Repeat each selected line N times (:repeat-lines 3).",
        fun: repeat_lines_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (1, Some(1)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "rename-word",
        aliases: &["rename-local"],
        doc: "Rename every whole-word occurrence of the symbol under the cursor in this buffer.",
        fun: rename_word,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, None),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "grep-word",
        aliases: &["gw", "find-references"],
        doc: "Search the project for the whole word under the cursor (jumpable in Run).",
        fun: grep_word,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "todos",
        aliases: &["project-todos", "fixme"],
        doc: "Scan the whole project for TODO/FIXME/HACK/XXX/BUG/NOTE markers (jumpable in Run).",
        fun: project_todos,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
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
        name: "git-stage",
        aliases: &["stage", "git-add"],
        doc: "Stage the current buffer's file (git add).",
        fun: git_stage,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "git-unstage",
        aliases: &["unstage"],
        doc: "Unstage the current buffer's file (git reset HEAD).",
        fun: git_unstage,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "stash",
        aliases: &["git-stash"],
        doc: "git stash the working-tree changes (then reload open buffers).",
        fun: git_stash,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "stash-pop",
        aliases: &["git-stash-pop"],
        doc: "git stash pop the most recent stash (then reload open buffers).",
        fun: git_stash_pop,
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
        name: "reverse",
        aliases: &["reverse-lines", "tac"],
        doc: "Reverse the order of the selected lines (or the whole buffer).",
        fun: reverse_lines,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "uuid",
        aliases: &["guid"],
        doc: "Insert a random UUID v4 at each cursor (replaces any selection).",
        fun: insert_uuid,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "goto-offset",
        aliases: &["goto-char"],
        doc: "Move the cursor to an absolute character offset.",
        fun: goto_offset,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (1, Some(1)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "pad-numbers",
        aliases: &["zero-pad"],
        doc: "Zero-pad every integer in the selection to <width> digits.",
        fun: pad_numbers_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (1, Some(1)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "increment-numbers",
        aliases: &["incr-numbers"],
        doc: "Add N (default 1; negative to decrement) to every integer in the selection.",
        fun: increment_numbers_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(1)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "bases",
        aliases: &["base-info"],
        doc: "Show the selected integer in decimal, hex, octal, and binary.",
        fun: show_bases,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "lorem",
        aliases: &["lipsum"],
        doc: "Insert N words (default 30) of lorem-ipsum placeholder text.",
        fun: lorem_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(1)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "date",
        aliases: &[],
        doc: "Insert the current UTC date (YYYY-MM-DD) at each cursor.",
        fun: insert_date,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "datetime",
        aliases: &["now"],
        doc: "Insert the current UTC date and time (YYYY-MM-DD HH:MM:SS) at each cursor.",
        fun: insert_datetime,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "timestamp",
        aliases: &["epoch"],
        doc: "Insert the current Unix epoch (seconds) at each cursor.",
        fun: insert_timestamp,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "sum",
        aliases: &["total"],
        doc: "Sum the numbers in the selection; reports sum/avg/min/max/count in the status line.",
        fun: sum,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "calc",
        aliases: &["eval-math"],
        doc: "Evaluate an arithmetic expression (+ - * / % ^), or each selection in place.",
        fun: calc,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, None),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "join-with",
        aliases: &["joinw"],
        doc: "Join the selected lines into one with a separator (default \", \").",
        fun: join_with,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, None),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "split-on",
        aliases: &["splito"],
        doc: "Split the selected line(s) on a separator (default \",\") into one item per line.",
        fun: split_on,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, None),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "squeeze-blank-lines",
        aliases: &["squeeze"],
        doc: "Collapse consecutive blank lines in the selection to one (cat -s).",
        fun: squeeze_blank_lines_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "dedup-adjacent",
        aliases: &["uniq-adjacent"],
        doc: "Collapse consecutive duplicate lines in the selection (Unix uniq).",
        fun: dedup_adjacent,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "number-lines",
        aliases: &["nl"],
        doc: "Prepend line numbers to the selected lines (optional start, default 1).",
        fun: number_lines_cmd,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(1)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "align",
        aliases: &["tabularize"],
        doc: "Align the selected lines on a delimiter (default `=`) so it shares a column.",
        fun: align_delimiter,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(1)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "sort-by-field",
        aliases: &["sortf"],
        doc: "Sort the selected lines by their Nth whitespace field (default 1).",
        fun: sort_by_field,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(1)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "sort-lines",
        aliases: &["sortl"],
        doc: "Sort the selected lines (or the whole buffer) — vim-style line sort.",
        fun: sort_lines,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            flags: &[
                Flag {
                    name: "reverse",
                    alias: Some('r'),
                    doc: "sort in reverse (descending) order",
                    ..Flag::DEFAULT
                },
                Flag {
                    name: "insensitive",
                    alias: Some('i'),
                    doc: "sort case-insensitively",
                    ..Flag::DEFAULT
                },
                Flag {
                    name: "numeric",
                    alias: Some('n'),
                    doc: "sort by each line's leading number",
                    ..Flag::DEFAULT
                },
                Flag {
                    name: "unique",
                    alias: Some('u'),
                    doc: "drop duplicate lines after sorting",
                    ..Flag::DEFAULT
                },
            ],
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
        name: "keymap",
        aliases: &[],
        doc: "Switch the active keymap preset: vim, helix, or emacs.",
        fun: set_keymap,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (1, Some(1)),
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
        name: "elisp",
        aliases: &["eval-expression", "el"],
        doc: "Evaluate an Emacs Lisp expression against the editor (embedded elisprs).",
        fun: elisp_eval,
        completer: CommandCompleter::none(),
        signature: ELISP_SIGNATURE,
    },
    TypableCommand {
        name: "vim",
        aliases: &["viml", "vimscript"],
        doc: "Evaluate a Vimscript (VimL) expression via the embedded vimlrs interpreter.",
        fun: viml_eval,
        completer: CommandCompleter::none(),
        signature: VIML_SIGNATURE,
    },
    TypableCommand {
        name: "awk",
        aliases: &["awk-filter"],
        doc: "Filter the selection (or whole buffer) through an awk program (embedded awkrs).",
        fun: awk_filter,
        completer: CommandCompleter::none(),
        signature: AWK_SIGNATURE,
    },
    TypableCommand {
        name: "zsh",
        aliases: &["zshell"],
        doc: "Run a command in the embedded zsh shell (state persists); output shown in a popup.",
        fun: zsh_run,
        completer: CommandCompleter::none(),
        signature: ZSH_SIGNATURE,
    },
    TypableCommand {
        name: "stryke",
        aliases: &["st"],
        doc: "Evaluate stryke (strykelang) source via the embedded interpreter (state persists).",
        fun: stryke_eval,
        completer: CommandCompleter::none(),
        signature: STRYKE_SIGNATURE,
    },
    TypableCommand {
        name: "repl",
        aliases: &[],
        doc: "Open the embedded-language REPL (elisp/viml/stryke/awk/zsh); optional starting language.",
        fun: repl_open,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(1)),
            ..Signature::DEFAULT
        },
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
        name: "delete-file",
        aliases: &["remove-file"],
        doc: "Delete the current buffer's file from disk and close the buffer (vim-eunuch :Delete).",
        fun: delete_file,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "chmod-x",
        aliases: &["chmodx", "make-executable"],
        doc: "Make the current file executable (chmod a+x). Unix only.",
        fun: chmod_x,
        completer: CommandCompleter::none(),
        signature: Signature {
            positionals: (0, Some(0)),
            ..Signature::DEFAULT
        },
    },
    TypableCommand {
        name: "mkdir",
        aliases: &[],
        doc: "Create a directory and any missing parents; with no arg, the current file's parent.",
        fun: mkdir,
        completer: CommandCompleter::positional(&[completers::directory]),
        signature: Signature {
            positionals: (0, Some(1)),
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
/// vim `:help` / `:h`: open the inline Help browser.
/// Build a `commands::Context` from a typable's compositor context, for calling
/// static commands that only touch the editor (no layer/callback side effects).
fn editor_context<'a>(cx: &'a mut compositor::Context) -> super::Context<'a> {
    super::Context {
        editor: cx.editor,
        count: None,
        register: None,
        callback: Vec::new(),
        on_next_key_callback: None,
        jobs: cx.jobs,
    }
}

/// `:emmet` — expand the emmet/zen abbreviation before the cursor.
fn emmet_expand_cmd(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let mut ecx = editor_context(cx);
    if !super::try_emmet_expand(&mut ecx) {
        ecx.editor
            .set_error("no emmet abbreviation before the cursor");
    }
    Ok(())
}

/// `:wc` — report document line/word/char counts (and selection stats).
fn word_count(cx: &mut compositor::Context, _args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    super::document_stats(&mut editor_context(cx));
    Ok(())
}

/// `:blame` — show git blame for the current line in the status bar.
fn blame_line(cx: &mut compositor::Context, _args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    super::git_blame_line(&mut editor_context(cx));
    Ok(())
}

/// `:reopen` — reopen the most recently closed file.
fn reopen_closed(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    super::reopen_last_closed(&mut editor_context(cx));
    Ok(())
}

/// `:zen` — toggle the IDE workbench (Zen / focus mode).
fn zen_mode(cx: &mut compositor::Context, _args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let call: job::Callback = job::Callback::EditorCompositor(Box::new(
        |_editor: &mut Editor, compositor: &mut Compositor| {
            if let Some(view) = compositor.find::<crate::ui::EditorView>() {
                view.toggle_ide();
            }
        },
    ));
    cx.jobs.callback(async move { Ok(call) });
    Ok(())
}

fn open_help(cx: &mut compositor::Context, _args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let callback = async move {
        let call: job::Callback = job::Callback::EditorCompositor(Box::new(
            |_editor: &mut Editor, compositor: &mut Compositor| {
                compositor.push(Box::new(crate::ui::preferences::PreferencesPanel::new(4)));
            },
        ));
        Ok(call)
    };
    cx.jobs.callback(callback);
    Ok(())
}

/// `:repl [lang]` — open the embedded-language REPL panel. With no argument it
/// starts on elisp; `:repl awk` (etc.) opens on that language. Tab switches in-panel.
fn repl_open(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    use crate::ui::repl::ReplLang;
    let arg = args.first().map(|s| s.to_string());
    let lang = match arg.as_deref() {
        None | Some("") => ReplLang::Elisp,
        Some(name) => match ReplLang::from_name(name) {
            Some(l) => l,
            None => bail!("repl: unknown language '{name}' (elisp/viml/stryke/awk/zsh)"),
        },
    };
    let callback = async move {
        let call: job::Callback = job::Callback::EditorCompositor(Box::new(
            move |_editor: &mut Editor, compositor: &mut Compositor| {
                compositor.push(Box::new(crate::ui::repl::ReplPanel::new(lang)));
            },
        ));
        Ok(call)
    };
    cx.jobs.callback(callback);
    Ok(())
}

/// vim `@:`: re-run the most recently executed command-line (`:` history).
/// Run a command line (without the leading `:`) from a picker/job callback that
/// already holds a `compositor::Context`. Used by `command_history_picker`.
pub(super) fn run_command_line(cx: &mut compositor::Context, line: &str) {
    if let Err(err) = execute_command_line(cx, line, PromptEvent::Validate) {
        cx.editor.set_error(err.to_string());
    }
}

pub(super) fn repeat_last_command_line(cx: &mut Context) {
    let last = cx
        .editor
        .registers
        .first(':', cx.editor)
        .map(|c| c.into_owned());
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

fn set_keymap(
    cx: &mut compositor::Context,
    args: Args<'_>,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    let name = args
        .first()
        .ok_or_else(|| anyhow::anyhow!("usage: :keymap <{}>", crate::keymap::PRESETS.join("|")))?;
    if !crate::keymap::PRESETS.contains(&name) {
        bail!(
            "unknown keymap `{name}` (expected one of: {})",
            crate::keymap::PRESETS.join(", ")
        );
    }
    // The keymap lives in the app-level config, which only the Application can
    // mutate; hand it the chosen preset to swap live (and set the matching mode).
    cx.editor
        .config_events
        .0
        .send(ConfigEvent::SetKeymap(name.to_string()))?;
    Ok(())
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
    fn csv_column_extracts() {
        use super::csv_column;
        assert_eq!(csv_column("a,b,c\n1,2,3", 2), "b\n2");
        assert_eq!(csv_column("a,b,c\n1,2,3", 1), "a\n1");
        // tab-separated auto-detected; cells trimmed
        assert_eq!(csv_column("x\ty\n1\t 2 ", 2), "y\n2");
        // out-of-range column → empty cells
        assert_eq!(csv_column("a,b\n1,2", 5), "\n");
    }

    #[test]
    fn code_fence_wraps() {
        use super::code_fence;
        assert_eq!(code_fence("let x = 1;", "rust"), "```rust\nlet x = 1;\n```");
        // no language → bare fence
        assert_eq!(code_fence("plain", ""), "```\nplain\n```");
        // trailing newlines on the body are trimmed so the closing fence is flush
        assert_eq!(code_fence("a\nb\n\n", "sh"), "```sh\na\nb\n```");
    }

    #[test]
    fn md_table_aligns() {
        use super::format_md_table;
        // ragged input → padded columns, regenerated separator
        assert_eq!(
            format_md_table("| a | b |\n|---|---|\n| 1 | 22 |"),
            "| a   | b   |\n| --- | --- |\n| 1   | 22  |"
        );
        // right-align marker preserved and applied
        assert_eq!(
            format_md_table("| x |\n| --: |\n| 1 |"),
            "|   x |\n| --: |\n|   1 |"
        );
        // not a table (no separator row) → unchanged
        assert_eq!(format_md_table("hello\nworld"), "hello\nworld");
    }

    #[test]
    fn json_query_navigates() {
        use super::json_query;
        let j = r#"{"users":[{"name":"Alice"},{"name":"Bob"}],"count":2}"#;
        // scalar leaf
        assert_eq!(json_query(j, "count").unwrap(), "2");
        // array index + nested key; string leaf keeps its JSON quotes
        assert_eq!(json_query(j, "users.1.name").unwrap(), "\"Bob\"");
        // leading dot tolerated; object leaf pretty-printed
        assert_eq!(
            json_query(j, ".users.0").unwrap(),
            "{\n  \"name\": \"Alice\"\n}"
        );
        // missing key, bad index, and descending into a scalar all error
        assert!(json_query(j, "nope").is_err());
        assert!(json_query(j, "users.9").is_err());
        assert!(json_query(j, "count.x").is_err());
        // invalid JSON input errors rather than panics
        assert!(json_query("not json", "a").is_err());
    }

    #[test]
    fn json_flatten_leaves() {
        use super::json_flatten;
        let j = r#"{"users":[{"name":"Alice"}],"count":2,"ok":true,"meta":{},"tags":[]}"#;
        // compare as a sorted set so the assertion is independent of key order
        let mut got: Vec<String> = json_flatten(j).unwrap().lines().map(String::from).collect();
        got.sort();
        let mut want = vec![
            "users.0.name = \"Alice\"".to_string(),
            "count = 2".to_string(),
            "ok = true".to_string(),
            "meta = {}".to_string(),
            "tags = []".to_string(),
        ];
        want.sort();
        assert_eq!(got, want);
        // invalid JSON errors rather than panicking
        assert!(json_flatten("nope").is_err());
    }

    #[test]
    fn json_to_csv_converts() {
        use super::json_to_csv;
        let j = r#"[{"name":"Alice","age":30},{"name":"Bob","city":"NYC"}]"#;
        // header = sorted union (age, city, name); missing fields → empty cells
        assert_eq!(
            json_to_csv(j).unwrap(),
            "age,city,name\n30,,Alice\n,NYC,Bob"
        );
        // values needing RFC-4180 escaping get quoted/doubled
        let q = r#"[{"note":"a,b","say":"he \"hi\""}]"#;
        assert_eq!(
            json_to_csv(q).unwrap(),
            "note,say\n\"a,b\",\"he \"\"hi\"\"\""
        );
        // non-array and non-object-element inputs error
        assert!(json_to_csv(r#"{"a":1}"#).is_err());
        assert!(json_to_csv(r#"[1,2]"#).is_err());
    }

    #[test]
    fn json_unflatten_rebuilds() {
        use super::{json_flatten, json_unflatten};
        let flat = "users.0.name = \"Alice\"\nusers.1.name = \"Bob\"\ncount = 2\nok = true";
        let json = json_unflatten(flat).unwrap();
        let v: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["count"], serde_json::json!(2));
        assert_eq!(v["ok"], serde_json::json!(true));
        assert_eq!(v["users"][0]["name"], serde_json::json!("Alice"));
        assert_eq!(v["users"][1]["name"], serde_json::json!("Bob"));
        // round-trips against json_flatten (compare as sorted line sets)
        let mut reflat: Vec<String> = json_flatten(&json)
            .unwrap()
            .lines()
            .map(String::from)
            .collect();
        let mut orig: Vec<String> = flat.lines().map(String::from).collect();
        reflat.sort();
        orig.sort();
        assert_eq!(reflat, orig);
        // malformed line, bad value, and type-conflicting paths all error
        assert!(json_unflatten("no equals here").is_err());
        assert!(json_unflatten("a = nope").is_err());
        assert!(json_unflatten("a = 1\na.b = 2").is_err());
    }

    #[test]
    fn toml_json_roundtrip() {
        use super::{json_to_toml, toml_to_json};
        let t = "name = \"zemacs\"\ncount = 3\n\n[server]\nport = 8080\n";
        let json = toml_to_json(t).unwrap();
        let v: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["name"], serde_json::json!("zemacs"));
        assert_eq!(v["count"], serde_json::json!(3));
        assert_eq!(v["server"]["port"], serde_json::json!(8080));
        // back to TOML, parse again → structure preserved
        let back = json_to_toml(&json).unwrap();
        let v2: toml::Value = toml::from_str(&back).unwrap();
        assert_eq!(v2["server"]["port"].as_integer(), Some(8080));
        // invalid inputs error
        assert!(toml_to_json("= = =").is_err());
        assert!(json_to_toml("not json").is_err());
        // JSON null has no TOML representation
        assert!(json_to_toml(r#"{"x": null}"#).is_err());
    }

    #[test]
    fn json_sort_orders() {
        use super::json_sort_array;
        // scalar numbers ascending
        assert_eq!(
            json_sort_array("[3,1,2]", None).unwrap(),
            "[\n  1,\n  2,\n  3\n]"
        );
        // strings lexically
        assert_eq!(
            json_sort_array(r#"["banana","apple"]"#, None).unwrap(),
            "[\n  \"apple\",\n  \"banana\"\n]"
        );
        // array of objects by field
        let j = r#"[{"n":"b","a":2},{"n":"a","a":1}]"#;
        let v: Value = serde_json::from_str(&json_sort_array(j, Some("n")).unwrap()).unwrap();
        assert_eq!(v[0]["n"], serde_json::json!("a"));
        assert_eq!(v[1]["n"], serde_json::json!("b"));
        // sort by a numeric field
        let v2: Value = serde_json::from_str(&json_sort_array(j, Some("a")).unwrap()).unwrap();
        assert_eq!(v2[0]["a"], serde_json::json!(1));
        // non-array and invalid inputs error
        assert!(json_sort_array("{}", None).is_err());
        assert!(json_sort_array("nope", None).is_err());
    }

    #[test]
    fn json_pick_projects() {
        use super::json_pick;
        let j = r#"[{"name":"Alice","age":30,"city":"NYC"},{"name":"Bob","age":5}]"#;
        let v: Value = serde_json::from_str(&json_pick(j, &["name", "age"]).unwrap()).unwrap();
        // array projected: only requested keys survive
        assert_eq!(v[0]["name"], serde_json::json!("Alice"));
        assert_eq!(v[0]["age"], serde_json::json!(30));
        assert!(v[0].get("city").is_none());
        assert_eq!(v[1]["name"], serde_json::json!("Bob"));
        // single object projected; absent key silently skipped
        let o = r#"{"a":1,"b":2,"c":3}"#;
        let v2: Value = serde_json::from_str(&json_pick(o, &["a", "c", "z"]).unwrap()).unwrap();
        assert_eq!(v2["a"], serde_json::json!(1));
        assert_eq!(v2["c"], serde_json::json!(3));
        assert!(v2.get("b").is_none());
        assert!(v2.get("z").is_none());
        // invalid JSON errors
        assert!(json_pick("nope", &["a"]).is_err());
    }

    #[test]
    fn json_omit_drops() {
        use super::{json_omit, json_pick};
        let j = r#"[{"name":"Alice","age":30,"pw":"x"},{"name":"Bob","age":5,"pw":"y"}]"#;
        let v: Value = serde_json::from_str(&json_omit(j, &["pw"]).unwrap()).unwrap();
        // dropped key gone, others retained
        assert!(v[0].get("pw").is_none());
        assert_eq!(v[0]["name"], serde_json::json!("Alice"));
        assert_eq!(v[1]["age"], serde_json::json!(5));
        // omit is the complement of pick: omitting all-but-one == picking that one
        let o = r#"{"a":1,"b":2,"c":3}"#;
        assert_eq!(
            json_omit(o, &["b", "c"]).unwrap(),
            json_pick(o, &["a"]).unwrap()
        );
        // invalid JSON errors
        assert!(json_omit("nope", &["a"]).is_err());
    }

    #[test]
    fn json_unique_dedups() {
        use super::json_unique;
        // scalar dedup preserves first-seen order
        assert_eq!(
            json_unique("[1,2,2,3,1]", None).unwrap(),
            "[\n  1,\n  2,\n  3\n]"
        );
        // dedup by field: first occurrence of each key wins
        let j = r#"[{"id":1,"v":"a"},{"id":1,"v":"b"},{"id":2,"v":"c"}]"#;
        let v: Value = serde_json::from_str(&json_unique(j, Some("id")).unwrap()).unwrap();
        assert_eq!(v.as_array().unwrap().len(), 2);
        assert_eq!(v[0]["v"], serde_json::json!("a"));
        assert_eq!(v[1]["id"], serde_json::json!(2));
        // non-array and invalid inputs error
        assert!(json_unique("{}", None).is_err());
        assert!(json_unique("nope", None).is_err());
    }

    #[test]
    fn json_group_by_buckets() {
        use super::json_group_by;
        let j = r#"[{"city":"NYC","n":"a"},{"city":"LA","n":"b"},{"city":"NYC","n":"c"}]"#;
        let v: Value = serde_json::from_str(&json_group_by(j, "city").unwrap()).unwrap();
        // two buckets; NYC keeps both members in input order
        assert_eq!(v["NYC"].as_array().unwrap().len(), 2);
        assert_eq!(v["LA"].as_array().unwrap().len(), 1);
        assert_eq!(v["NYC"][0]["n"], serde_json::json!("a"));
        assert_eq!(v["NYC"][1]["n"], serde_json::json!("c"));
        // missing field groups under "null"
        let v2: Value =
            serde_json::from_str(&json_group_by(r#"[{"x":1},{"y":2}]"#, "x").unwrap()).unwrap();
        assert_eq!(v2["1"].as_array().unwrap().len(), 1);
        assert_eq!(v2["null"].as_array().unwrap().len(), 1);
        // non-array and invalid inputs error
        assert!(json_group_by("{}", "x").is_err());
        assert!(json_group_by("nope", "x").is_err());
    }

    #[test]
    fn extract_matches_lists() {
        use super::extract_matches;
        // no capture group → whole matches, one per line
        assert_eq!(extract_matches("a1 b2 c3", r"\d").unwrap(), "1\n2\n3");
        // capture group → group 1 text
        assert_eq!(
            extract_matches("key=val; x=y", r"(\w+)=").unwrap(),
            "key\nx"
        );
        // no matches → empty
        assert_eq!(extract_matches("abc", r"\d").unwrap(), "");
        // invalid pattern errors rather than panicking
        assert!(extract_matches("x", r"(").is_err());
    }

    #[test]
    fn filter_lines_keeps_and_drops() {
        use super::filter_lines;
        let text = "apple\nbanana\ncherry\napricot";
        // keep matching (grep)
        assert_eq!(filter_lines(text, "^a", true).unwrap(), "apple\napricot");
        // drop matching (grep -v)
        assert_eq!(filter_lines(text, "^a", false).unwrap(), "banana\ncherry");
        // no keep-matches → empty
        assert_eq!(filter_lines(text, "zzz", true).unwrap(), "");
        // invalid pattern errors
        assert!(filter_lines(text, "(", true).is_err());
    }

    #[test]
    fn count_matches_counts() {
        use super::count_matches;
        // 4 digit matches across the first two of three lines
        let (m, l) = count_matches("a1 b2\nc3 d4\nxx", r"\d").unwrap();
        assert_eq!(m, 4);
        assert_eq!(l, 2);
        // no matches
        assert_eq!(count_matches("abc", r"\d").unwrap(), (0, 0));
        // invalid pattern errors
        assert!(count_matches("x", "(").is_err());
    }

    #[test]
    fn uniq_count_tallies() {
        use super::uniq_count;
        let t = "apple\nbanana\napple\ncherry\napple\nbanana";
        // sorted by count desc; ties (none here) would keep first-seen order
        assert_eq!(uniq_count(t), "3 apple\n2 banana\n1 cherry");
        // empty input → empty output
        assert_eq!(uniq_count(""), "");
        // tie keeps first-seen order (x before y)
        assert_eq!(uniq_count("x\ny\nx\ny"), "2 x\n2 y");
    }

    #[test]
    fn number_stats_summarizes() {
        use super::{fmt_stat, number_stats};
        let s = number_stats(&[1.0, 2.0, 3.0, 4.0, 5.0]).unwrap();
        assert_eq!(s, (5, 15.0, 3.0, 1.0, 5.0));
        // empty slice → None
        assert!(number_stats(&[]).is_none());
        // compact float formatting: integers bare, fractions trimmed
        assert_eq!(fmt_stat(3.0), "3");
        assert_eq!(fmt_stat(3.5), "3.5");
        assert_eq!(fmt_stat(10.0 / 3.0), "3.3333");
    }

    #[test]
    fn gen_seq_generates() {
        use super::gen_seq;
        assert_eq!(gen_seq(1, 5, 1).unwrap(), "1\n2\n3\n4\n5");
        assert_eq!(gen_seq(0, 10, 5).unwrap(), "0\n5\n10");
        // descending
        assert_eq!(gen_seq(5, 1, -2).unwrap(), "5\n3\n1");
        // single element when start == end
        assert_eq!(gen_seq(3, 3, 1).unwrap(), "3");
        // direction disagrees with step sign → empty
        assert_eq!(gen_seq(5, 1, 1).unwrap(), "");
        // zero step errors
        assert!(gen_seq(1, 5, 0).is_err());
    }

    #[test]
    fn cut_field_extracts() {
        use super::cut_field;
        assert_eq!(cut_field("a b c\n1 2 3", 2), "b\n2");
        // runs of whitespace collapse; leading space ignored
        assert_eq!(cut_field("  x   y  ", 1), "x");
        assert_eq!(cut_field("  x   y  ", 2), "y");
        // out-of-range field → empty line
        assert_eq!(cut_field("only\na b", 3), "\n");
    }

    #[test]
    fn running_total_accumulates() {
        use super::running_total;
        assert_eq!(running_total("1\n2\n3\n4"), "1\n3\n6\n10");
        // negatives and a fractional result
        assert_eq!(running_total("10\n-3\n5"), "10\n7\n12");
        assert_eq!(running_total("1.5\n2.5"), "1.5\n4");
        // non-numeric line passes through and doesn't affect the accumulator
        assert_eq!(running_total("1\nfoo\n2"), "1\nfoo\n3");
    }

    #[test]
    fn diff_lines_deltas() {
        use super::{diff_lines, running_total};
        // exact inverse of running_total
        assert_eq!(diff_lines("1\n3\n6\n10"), "1\n2\n3\n4");
        assert_eq!(diff_lines("10\n7\n12"), "10\n-3\n5");
        // round-trips: diff then cumulative-sum restores the original
        assert_eq!(running_total(&diff_lines("5\n9\n2\n8")), "5\n9\n2\n8");
        // non-numeric passes through; reference stays at previous numeric
        assert_eq!(diff_lines("1\nfoo\n3"), "1\nfoo\n2");
    }

    #[test]
    fn sum_column_totals() {
        use super::sum_column;
        assert_eq!(sum_column("a 1\nb 2\nc 3", 2), (6.0, 3));
        // non-numeric and missing cells are skipped, not counted
        assert_eq!(sum_column("x 10\ny notnum\nz 5", 2), (15.0, 2));
        assert_eq!(sum_column("a\nb", 2), (0.0, 0));
        // first column
        assert_eq!(sum_column("3 x\n4 y", 1), (7.0, 2));
    }

    #[test]
    fn shuffle_lines_permutes() {
        use super::shuffle_lines;
        let input = "a\nb\nc\nd\ne\nf\ng";
        let out = shuffle_lines(input, 12345);
        // result is a permutation: same multiset of lines
        let mut got: Vec<&str> = out.lines().collect();
        let mut orig: Vec<&str> = input.lines().collect();
        got.sort_unstable();
        orig.sort_unstable();
        assert_eq!(got, orig);
        // deterministic for a fixed seed
        assert_eq!(shuffle_lines(input, 12345), out);
        // different seed generally reorders differently (these two seeds differ here)
        assert_ne!(shuffle_lines(input, 999), shuffle_lines(input, 12345));
        // single line is unchanged
        assert_eq!(shuffle_lines("x", 7), "x");
    }

    #[test]
    fn sample_lines_subsets() {
        use super::sample_lines;
        let input = "a\nb\nc\nd\ne\nf";
        let out = sample_lines(input, 3, 42);
        // exactly n lines, all drawn from the input
        let orig: Vec<&str> = input.lines().collect();
        let got: Vec<&str> = out.lines().collect();
        assert_eq!(got.len(), 3);
        assert!(got.iter().all(|l| orig.contains(l)));
        // order is preserved (result is a subsequence): indices strictly increasing
        let positions: Vec<usize> = got
            .iter()
            .map(|l| orig.iter().position(|o| o == l).unwrap())
            .collect();
        assert!(positions.windows(2).all(|w| w[0] < w[1]));
        // deterministic for a fixed seed
        assert_eq!(sample_lines(input, 3, 42), out);
        // n >= line count → unchanged
        assert_eq!(sample_lines(input, 10, 1), input);
    }

    #[test]
    fn jsonl_json_roundtrip() {
        use super::{json_to_jsonl, jsonl_to_json};
        let jsonl = "{\"a\":1}\n{\"a\":2}\n\n{\"a\":3}";
        let json = jsonl_to_json(jsonl).unwrap();
        let v: Value = serde_json::from_str(&json).unwrap();
        // blank line skipped → 3 elements
        assert_eq!(v.as_array().unwrap().len(), 3);
        assert_eq!(v[1]["a"], serde_json::json!(2));
        // back to JSONL: compact, one per line (blank line not reproduced)
        assert_eq!(
            json_to_jsonl(&json).unwrap(),
            "{\"a\":1}\n{\"a\":2}\n{\"a\":3}"
        );
        // errors: bad JSON line, and non-array input to the inverse
        assert!(jsonl_to_json("{not json}").is_err());
        assert!(json_to_jsonl("{}").is_err());
    }

    #[test]
    fn head_tail_truncate() {
        use super::{head_lines, tail_lines};
        let t = "a\nb\nc\nd\ne";
        assert_eq!(head_lines(t, 2), "a\nb");
        assert_eq!(tail_lines(t, 2), "d\ne");
        // n exceeding the line count returns everything
        assert_eq!(head_lines("a\nb", 5), "a\nb");
        assert_eq!(tail_lines("a\nb", 5), "a\nb");
        // head n + tail (len-n) together reconstruct the whole
        assert_eq!(format!("{}\n{}", head_lines(t, 2), tail_lines(t, 3)), t);
    }

    #[test]
    fn rev_each_line_reverses() {
        use super::rev_each_line;
        // each line reversed independently; line order preserved
        assert_eq!(rev_each_line("abc\ndef"), "cba\nfed");
        // precomposed accent preserved
        assert_eq!(rev_each_line("héllo"), "olléh");
        // double application is the identity
        assert_eq!(
            rev_each_line(&rev_each_line("hello\nworld")),
            "hello\nworld"
        );
    }

    #[test]
    fn json_to_table_aligns() {
        use super::json_to_table;
        let j = r#"[{"name":"Alice","age":30},{"name":"Bob","age":5}]"#;
        // headers = sorted union (age, name); columns left-aligned to widest cell
        assert_eq!(
            json_to_table(j).unwrap(),
            "age  name\n---  -----\n30   Alice\n5    Bob"
        );
        // non-array and non-object-element inputs error
        assert!(json_to_table(r#"{"a":1}"#).is_err());
        assert!(json_to_table("[1,2]").is_err());
    }

    #[test]
    fn hexdump_formats() {
        use super::hexdump;
        let d = hexdump("hello");
        assert!(d.starts_with("00000000  68 65 6c 6c 6f"));
        assert!(d.ends_with("|hello|"));
        assert_eq!(d.lines().count(), 1);
        // non-printable bytes render as '.' in the gutter
        let d2 = hexdump("a\tb");
        assert!(d2.contains("61 09 62"));
        assert!(d2.ends_with("|a.b|"));
        // 17 bytes wrap to a second row at offset 00000010
        let d3 = hexdump("0123456789abcdefX");
        assert_eq!(d3.lines().count(), 2);
        assert!(d3.lines().nth(1).unwrap().starts_with("00000010"));
        assert!(d3.lines().nth(1).unwrap().ends_with("|X|"));
        // empty input → empty output
        assert_eq!(hexdump(""), "");
    }

    #[test]
    fn dedup_all_lines_global() {
        use super::dedup_all_lines;
        // non-adjacent duplicates removed; first occurrence + order kept
        assert_eq!(dedup_all_lines("a\nb\na\nc\nb"), "a\nb\nc");
        assert_eq!(dedup_all_lines("x\nx\nx"), "x");
        // already-unique input unchanged
        assert_eq!(dedup_all_lines("a\nb\nc"), "a\nb\nc");
    }

    #[test]
    fn caesar_shifts() {
        use super::caesar;
        assert_eq!(caesar("abc", 1), "bcd");
        assert_eq!(caesar("xyz", 3), "abc"); // wraps within case
                                             // shift 13 == ROT13, case + punctuation preserved
        assert_eq!(caesar("Hello, World!", 13), "Uryyb, Jbeyq!");
        // identity and negative shifts (normalized mod 26)
        assert_eq!(caesar("abc", 0), "abc");
        assert_eq!(caesar("bcd", -1), "abc");
        // shift then unshift is the identity, digits untouched
        assert_eq!(caesar(&caesar("Test123", 5), -5), "Test123");
    }

    #[test]
    fn base32_roundtrip() {
        use super::{base32_decode, base32_encode};
        // RFC 4648 test vector
        assert_eq!(base32_encode("foobar"), "MZXW6YTBOI======");
        assert_eq!(base32_decode("MZXW6YTBOI======").unwrap(), "foobar");
        // round-trips arbitrary text
        assert_eq!(
            base32_decode(&base32_encode("Hello, World!")).unwrap(),
            "Hello, World!"
        );
        // decode tolerates lowercase and whitespace/padding
        assert_eq!(base32_decode("mzxw6ytb oi======").unwrap(), "foobar");
        // empty, and invalid char errors
        assert_eq!(base32_encode(""), "");
        assert!(base32_decode("11111111").is_err());
    }

    #[test]
    fn crc32_known_vectors() {
        use super::crc32;
        assert_eq!(crc32(""), 0);
        // canonical CRC32 check value
        assert_eq!(crc32("123456789"), 0xCBF4_3926);
        // pangram vector
        assert_eq!(
            crc32("The quick brown fox jumps over the lazy dog"),
            0x414F_A339
        );
    }

    #[test]
    fn rot47_rotates() {
        use super::rot47;
        assert_eq!(rot47("Hello"), "w6==@");
        // self-inverse over mixed printable text
        assert_eq!(rot47(&rot47("Hello, World! 123")), "Hello, World! 123");
        // spaces are left untouched
        assert!(rot47("a b").contains(' '));
        assert_eq!(rot47(""), "");
    }

    #[test]
    fn morse_roundtrip() {
        use super::{morse_decode, morse_encode};
        assert_eq!(morse_encode("SOS"), "... --- ...");
        assert_eq!(morse_decode("... --- ..."), "SOS");
        // word separator ` / ` and case-insensitive encode
        assert_eq!(morse_encode("Hi there"), ".... .. / - .... . .-. .");
        // round-trips uppercase words and digits
        assert_eq!(
            morse_decode(&morse_encode("HELLO WORLD 42")),
            "HELLO WORLD 42"
        );
        assert_eq!(morse_encode(""), "");
    }

    #[test]
    fn human_bytes_formats() {
        use super::{human_bytes, humanize_lines};
        assert_eq!(human_bytes(0.0), "0 B");
        assert_eq!(human_bytes(512.0), "512 B");
        assert_eq!(human_bytes(1024.0), "1.0 KiB");
        assert_eq!(human_bytes(1536.0), "1.5 KiB");
        assert_eq!(human_bytes(1048576.0), "1.0 MiB");
        assert_eq!(human_bytes(1073741824.0), "1.0 GiB");
        // per-line: numbers converted, others passed through
        assert_eq!(
            humanize_lines("2048\nfoo\n1073741824"),
            "2.0 KiB\nfoo\n1.0 GiB"
        );
    }

    #[test]
    fn ordinal_suffixes() {
        use super::{ordinal_suffix, ordinalize_lines};
        assert_eq!(ordinal_suffix(1), "st");
        assert_eq!(ordinal_suffix(2), "nd");
        assert_eq!(ordinal_suffix(3), "rd");
        assert_eq!(ordinal_suffix(4), "th");
        // 11-13 are always "th" despite ending in 1/2/3
        assert_eq!(ordinal_suffix(11), "th");
        assert_eq!(ordinal_suffix(12), "th");
        assert_eq!(ordinal_suffix(13), "th");
        // ...but 21/22/23/111 follow the normal rule / exception correctly
        assert_eq!(ordinal_suffix(21), "st");
        assert_eq!(ordinal_suffix(112), "th");
        assert_eq!(ordinal_suffix(121), "st");
        // per-line, non-numeric passes through
        assert_eq!(ordinalize_lines("1\n2\n22\nfoo"), "1st\n2nd\n22nd\nfoo");
    }

    #[test]
    fn case_conversions() {
        use super::{split_identifier_words, to_camel, to_kebab, to_pascal, to_snake};
        // word splitting across all input conventions
        assert_eq!(
            split_identifier_words("myVariableName"),
            ["my", "variable", "name"]
        );
        assert_eq!(
            split_identifier_words("my_variable_name"),
            ["my", "variable", "name"]
        );
        assert_eq!(
            split_identifier_words("my-variable-name"),
            ["my", "variable", "name"]
        );
        assert_eq!(
            split_identifier_words("MyVariableName"),
            ["my", "variable", "name"]
        );
        assert_eq!(
            split_identifier_words("my variable name"),
            ["my", "variable", "name"]
        );
        // acronym followed by a word splits correctly
        assert_eq!(
            split_identifier_words("HTTPSConnection"),
            ["https", "connection"]
        );
        // rendering into each target case (from any source form)
        assert_eq!(to_snake("myVariableName"), "my_variable_name");
        assert_eq!(to_kebab("MyVariableName"), "my-variable-name");
        assert_eq!(to_camel("my_variable_name"), "myVariableName");
        assert_eq!(to_pascal("my-variable-name"), "MyVariableName");
        // round-trip snake -> camel -> snake
        assert_eq!(to_snake(&to_camel("foo_bar_baz")), "foo_bar_baz");
    }

    #[test]
    fn to_constant_case() {
        use super::to_constant;
        assert_eq!(to_constant("myVariableName"), "MY_VARIABLE_NAME");
        assert_eq!(to_constant("my-var"), "MY_VAR");
        assert_eq!(to_constant("already_snake"), "ALREADY_SNAKE");
    }

    #[test]
    fn binary_roundtrip() {
        use super::{from_binary, to_binary};
        assert_eq!(to_binary("A"), "01000001");
        assert_eq!(to_binary("AB"), "01000001 01000010");
        assert_eq!(from_binary("01000001 01000010").unwrap(), "AB");
        // round-trips arbitrary text (incl. punctuation)
        assert_eq!(from_binary(&to_binary("Hi!")).unwrap(), "Hi!");
        // invalid binary digit errors
        assert!(from_binary("01000002").is_err());
    }

    #[test]
    fn natural_sort_orders() {
        use super::natural_sort_lines;
        // numbers compared by value, not lexically
        assert_eq!(
            natural_sort_lines("file10\nfile2\nfile1"),
            "file1\nfile2\nfile10"
        );
        assert_eq!(
            natural_sort_lines("img12\nimg2\nimg1\nimg100"),
            "img1\nimg2\nimg12\nimg100"
        );
        // shorter prefix sorts first ("a" < "a2" < "b")
        assert_eq!(natural_sort_lines("b\na2\na"), "a\na2\nb");
        // leading zeros: equal value, fewer digits first
        assert_eq!(natural_sort_lines("x010\nx10\nx9"), "x9\nx10\nx010");
    }

    #[test]
    fn pad_lines_justifies() {
        use super::pad_lines;
        // left-justify (pad on the right)
        assert_eq!(pad_lines("a\nbb\nccc", 3, true), "a  \nbb \nccc");
        // right-justify (pad on the left)
        assert_eq!(pad_lines("a\nbb", 3, false), "  a\n bb");
        // lines already at/over width are untouched
        assert_eq!(pad_lines("toolong", 3, true), "toolong");
    }

    #[test]
    fn json_keys_lists() {
        use super::json_keys;
        // single object: keys (BTreeMap-sorted), one per line
        assert_eq!(json_keys(r#"{"name":"a","age":1}"#).unwrap(), "age\nname");
        // array of objects: union of keys, first-seen order across elements
        assert_eq!(json_keys(r#"[{"a":1},{"b":2},{"a":3}]"#).unwrap(), "a\nb");
        // array of non-objects yields no keys
        assert_eq!(json_keys("[1,2]").unwrap(), "");
        // scalar errors
        assert!(json_keys("42").is_err());
    }

    #[test]
    fn json_describe_types() {
        use super::json_describe;
        assert_eq!(json_describe("42").unwrap(), "number (42)");
        assert_eq!(json_describe("[1,2,3]").unwrap(), "array (3 elements)");
        assert_eq!(
            json_describe(r#"{"a":1,"b":2}"#).unwrap(),
            "object (2 keys)"
        );
        assert_eq!(json_describe(r#""hi""#).unwrap(), "string (len 2)");
        assert_eq!(json_describe("true").unwrap(), "boolean (true)");
        assert_eq!(json_describe("null").unwrap(), "null");
        assert!(json_describe("nope").is_err());
    }

    #[test]
    fn cut_lines_splits() {
        use super::cut_lines;
        // keep after the first delimiter
        assert_eq!(cut_lines("key=val\nfoo=bar", "=", true), "val\nbar");
        // keep before
        assert_eq!(cut_lines("key=val\nfoo=bar", "=", false), "key\nfoo");
        // only the FIRST occurrence splits
        assert_eq!(cut_lines("a:b:c", ":", true), "b:c");
        assert_eq!(cut_lines("a:b:c", ":", false), "a");
        // lines without the delimiter are kept whole
        assert_eq!(cut_lines("nodelim", "=", true), "nodelim");
        // multi-char delimiter
        assert_eq!(cut_lines("a -> b", " -> ", true), "b");
    }

    #[test]
    fn swapcase_inverts() {
        use super::swapcase;
        assert_eq!(swapcase("Hello World"), "hELLO wORLD");
        // digits and symbols pass through
        assert_eq!(swapcase("ABC abc 123!"), "abc ABC 123!");
        // self-inverse
        assert_eq!(swapcase(&swapcase("MixedCase42")), "MixedCase42");
    }

    #[test]
    fn strip_zero_width_cleans() {
        use super::strip_zero_width;
        // ZWSP and BOM removed, visible text intact
        assert_eq!(strip_zero_width("a\u{200B}b\u{FEFF}c"), "abc");
        // soft hyphen and zero-width joiner removed
        assert_eq!(strip_zero_width("co\u{00AD}de\u{200D}x"), "codex");
        // clean text untouched
        assert_eq!(strip_zero_width("normal text"), "normal text");
    }

    #[test]
    fn lines_json_array_roundtrip() {
        use super::{json_array_to_lines, lines_to_json_array};
        assert_eq!(lines_to_json_array("a\nb"), "[\n  \"a\",\n  \"b\"\n]");
        assert_eq!(json_array_to_lines(r#"["a","b"]"#).unwrap(), "a\nb");
        // non-string elements render as compact JSON
        assert_eq!(json_array_to_lines("[1,2,3]").unwrap(), "1\n2\n3");
        // string round-trip
        assert_eq!(
            json_array_to_lines(&lines_to_json_array("x\ny")).unwrap(),
            "x\ny"
        );
        // non-array errors
        assert!(json_array_to_lines("{}").is_err());
    }

    #[test]
    fn checkbox_list_builds() {
        use super::checkbox_list;
        assert_eq!(
            checkbox_list("buy milk\nwalk dog"),
            "- [ ] buy milk\n- [ ] walk dog"
        );
        // an existing bullet is stripped, not doubled
        assert_eq!(checkbox_list("- existing"), "- [ ] existing");
        // blank lines stay blank
        assert_eq!(checkbox_list("a\n\nb"), "- [ ] a\n\n- [ ] b");
    }

    #[test]
    fn unwrap_paragraphs_joins() {
        use super::unwrap_paragraphs;
        // lines within a paragraph join with a space; blank line separates paragraphs
        assert_eq!(unwrap_paragraphs("a\nb\n\nc\nd"), "a b\n\nc d");
        // single paragraph
        assert_eq!(unwrap_paragraphs("one\ntwo\nthree"), "one two three");
        // leading/trailing whitespace on wrapped lines is trimmed before joining
        assert_eq!(unwrap_paragraphs("a  \n  b"), "a b");
    }

    #[test]
    fn sql_in_list_builds() {
        use super::sql_in_list;
        assert_eq!(sql_in_list("a\nb\nc"), "('a', 'b', 'c')");
        // embedded single quote is doubled (SQL escaping)
        assert_eq!(sql_in_list("O'Brien"), "('O''Brien')");
        // blank lines are skipped, surrounding whitespace trimmed
        assert_eq!(sql_in_list("x\n\n  y  "), "('x', 'y')");
    }

    #[test]
    fn dec_hex_line_conversion() {
        use super::{dec_to_hex_lines, hex_to_dec_lines};
        assert_eq!(dec_to_hex_lines("255\n16\n10"), "ff\n10\na");
        assert_eq!(hex_to_dec_lines("ff\n10\n0xa"), "255\n16\n10");
        // negative and non-numeric handling
        assert_eq!(dec_to_hex_lines("-255\nfoo"), "-ff\nfoo");
        assert_eq!(hex_to_dec_lines("-ff\nfoo"), "-255\nfoo");
        // round-trip
        assert_eq!(hex_to_dec_lines(&dec_to_hex_lines("4096\n42")), "4096\n42");
    }

    #[test]
    fn unicode_escape_roundtrip() {
        use super::{unicode_escape, unicode_unescape};
        assert_eq!(unicode_escape("café"), "caf\\u{e9}");
        assert_eq!(unicode_unescape("caf\\u{e9}"), "café");
        // 4-digit \uXXXX form also decodes
        assert_eq!(unicode_unescape("caf\\u00e9"), "café");
        // ASCII passes through unchanged
        assert_eq!(unicode_escape("ascii 123"), "ascii 123");
        // round-trips mixed text incl. an astral character
        assert_eq!(unicode_unescape(&unicode_escape("héllo→🎉")), "héllo→🎉");
    }

    #[test]
    fn sort_by_length_orders() {
        use super::sort_by_length;
        assert_eq!(sort_by_length("ccc\na\nbb"), "a\nbb\nccc");
        // equal-length lines break ties lexically (aa before bb)
        assert_eq!(sort_by_length("bb\nc\naa"), "c\naa\nbb");
    }

    #[test]
    fn count_unique_counts() {
        use super::count_unique;
        assert_eq!(count_unique("a\nb\na\nc"), (3, 4));
        assert_eq!(count_unique("x\nx\nx"), (1, 3));
        assert_eq!(count_unique(""), (0, 0));
    }

    #[test]
    fn rotate_lines_cycles() {
        use super::rotate_lines;
        assert_eq!(rotate_lines("a\nb\nc\nd", 1), "b\nc\nd\na");
        // negative rotates the other way
        assert_eq!(rotate_lines("a\nb\nc\nd", -1), "d\na\nb\nc");
        // full rotation and zero are identities; large n wraps (mod len)
        assert_eq!(rotate_lines("a\nb\nc", 3), "a\nb\nc");
        assert_eq!(rotate_lines("a\nb\nc", 0), "a\nb\nc");
        assert_eq!(rotate_lines("a\nb\nc", 4), "b\nc\na");
    }

    #[test]
    fn unquote_each_line_strips() {
        use super::unquote_each_line;
        // each line independently: double, single, and backtick quotes
        assert_eq!(unquote_each_line("\"a\"\n'b'\n`c`"), "a\nb\nc");
        // unquoted and mismatched lines pass through
        assert_eq!(
            unquote_each_line("\"x\"\nplain\n\"unmatched"),
            "x\nplain\n\"unmatched"
        );
        // empty quotes collapse to empty
        assert_eq!(unquote_each_line("\"\""), "");
    }

    #[test]
    fn quote_each_line_wraps() {
        use super::{quote_each_line, unquote_each_line};
        assert_eq!(quote_each_line("a\nb"), "\"a\"\n\"b\"");
        // embedded quote and backslash are escaped
        assert_eq!(quote_each_line("say \"hi\""), "\"say \\\"hi\\\"\"");
        assert_eq!(quote_each_line("path\\x"), "\"path\\\\x\"");
        // plain text round-trips through unquote
        assert_eq!(unquote_each_line(&quote_each_line("plain")), "plain");
    }

    #[test]
    fn capitalize_lines_uppercases_first() {
        use super::capitalize_lines;
        assert_eq!(capitalize_lines("hello\nworld"), "Hello\nWorld");
        // leading indentation/bullet is skipped to the first letter
        assert_eq!(capitalize_lines("  indented"), "  Indented");
        assert_eq!(capitalize_lines("- item"), "- Item");
        // only the first letter changes; rest of the line is untouched
        assert_eq!(capitalize_lines("hello WORLD"), "Hello WORLD");
    }

    #[test]
    fn remove_blank_lines_drops_all() {
        use super::remove_blank_lines;
        // every blank/whitespace-only line removed (not just collapsed)
        assert_eq!(remove_blank_lines("a\n\nb\n  \nc"), "a\nb\nc");
        // no blanks → unchanged
        assert_eq!(remove_blank_lines("a\nb"), "a\nb");
    }

    #[test]
    fn trim_lines_both_ends() {
        use super::trim_lines;
        assert_eq!(trim_lines("  a  \n\tb\t"), "a\nb");
        // internal whitespace is preserved (unlike normalize-whitespace)
        assert_eq!(trim_lines("  x  y  "), "x  y");
    }

    #[test]
    fn kv_to_json_builds_object() {
        use super::kv_to_json;
        let v: Value =
            serde_json::from_str(&kv_to_json("a=1\nb: two\n# comment\n\nc = three").unwrap())
                .unwrap();
        assert_eq!(v["a"], serde_json::json!("1"));
        assert_eq!(v["b"], serde_json::json!("two"));
        assert_eq!(v["c"], serde_json::json!("three"));
        assert!(v.get("# comment").is_none());
        // value containing a colon keeps everything after the first '='
        let v2: Value = serde_json::from_str(&kv_to_json("url=http://x").unwrap()).unwrap();
        assert_eq!(v2["url"], serde_json::json!("http://x"));
        // a line with no separator errors
        assert!(kv_to_json("noseparator").is_err());
    }

    #[test]
    fn json_to_kv_pairs() {
        use super::{json_to_kv, kv_to_json};
        // BTreeMap-sorted keys; string values unquoted
        assert_eq!(json_to_kv(r#"{"a":"1","b":"two"}"#).unwrap(), "a=1\nb=two");
        // non-string values render as compact JSON
        assert_eq!(
            json_to_kv(r#"{"n":42,"ok":true}"#).unwrap(),
            "n=42\nok=true"
        );
        // round-trips string values through kv_to_json
        let v: Value =
            serde_json::from_str(&kv_to_json(&json_to_kv(r#"{"x":"y"}"#).unwrap()).unwrap())
                .unwrap();
        assert_eq!(v["x"], serde_json::json!("y"));
        // non-object errors
        assert!(json_to_kv("[1,2]").is_err());
    }

    #[test]
    fn json_pluck_extracts_field() {
        use super::json_pluck;
        let j = r#"[{"name":"Alice","age":30},{"name":"Bob","age":5}]"#;
        assert_eq!(json_pluck(j, "name").unwrap(), "Alice\nBob");
        // numeric field values rendered compactly
        assert_eq!(json_pluck(j, "age").unwrap(), "30\n5");
        // missing field → blank line for that element
        assert_eq!(json_pluck(r#"[{"a":1},{"b":2}]"#, "a").unwrap(), "1\n");
        // non-array errors
        assert!(json_pluck("{}", "x").is_err());
    }

    #[test]
    fn to_html_list_builds() {
        use super::to_html_list;
        assert_eq!(
            to_html_list("apple\nbanana"),
            "<ul>\n  <li>apple</li>\n  <li>banana</li>\n</ul>"
        );
        // content is HTML-escaped; blank lines skipped
        assert_eq!(
            to_html_list("a < b\n\nx & y"),
            "<ul>\n  <li>a &lt; b</li>\n  <li>x &amp; y</li>\n</ul>"
        );
    }

    #[test]
    fn from_html_list_extracts() {
        use super::{from_html_list, to_html_list};
        assert_eq!(
            from_html_list("<ul>\n  <li>apple</li>\n  <li>banana</li>\n</ul>"),
            "apple\nbanana"
        );
        // entities are unescaped
        assert_eq!(from_html_list("<li>a &lt; b</li>"), "a < b");
        // round-trips with to_html_list
        assert_eq!(from_html_list(&to_html_list("x\ny")), "x\ny");
    }

    #[test]
    fn csv_to_html_table_builds() {
        use super::csv_to_html_table;
        let html = csv_to_html_table("name,age\nAlice,30");
        assert!(html.starts_with("<table>"));
        assert!(html.ends_with("</table>"));
        // header row uses <th>, body rows use <td>
        assert!(html.contains("<tr><th>name</th><th>age</th></tr>"));
        assert!(html.contains("<tr><td>Alice</td><td>30</td></tr>"));
        // empty input → empty output
        assert_eq!(csv_to_html_table(""), "");
    }

    #[test]
    fn slugify_lines_each() {
        use super::slugify_lines;
        assert_eq!(
            slugify_lines("Hello World\nFoo Bar!"),
            "hello-world\nfoo-bar"
        );
        // runs of non-alphanumerics collapse; leading/trailing trimmed
        assert_eq!(slugify_lines("  Multiple   Spaces  "), "multiple-spaces");
        // numbers retained
        assert_eq!(slugify_lines("Top 10 Tips"), "top-10-tips");
    }

    #[test]
    fn lines_to_csv_row_joins() {
        use super::lines_to_csv_row;
        assert_eq!(lines_to_csv_row("a\nb\nc"), "a,b,c");
        // a field containing a comma is quoted; an embedded quote is doubled
        assert_eq!(
            lines_to_csv_row("a,b\nplain\nsay \"hi\""),
            "\"a,b\",plain,\"say \"\"hi\"\"\""
        );
    }

    #[test]
    fn csv_row_to_lines_splits() {
        use super::{csv_row_to_lines, lines_to_csv_row};
        assert_eq!(csv_row_to_lines("a,b,c"), "a\nb\nc");
        // quoted field with an embedded comma stays whole
        assert_eq!(csv_row_to_lines("\"a,b\",plain"), "a,b\nplain");
        // doubled quote decodes to a single quote
        assert_eq!(csv_row_to_lines("\"say \"\"hi\"\"\",x"), "say \"hi\"\nx");
        // round-trips with lines_to_csv_row
        assert_eq!(
            csv_row_to_lines(&lines_to_csv_row("p\nq,r\ns")),
            "p\nq,r\ns"
        );
    }

    #[test]
    fn deslugify_titleizes() {
        use super::deslugify;
        assert_eq!(deslugify("hello-world"), "Hello World");
        assert_eq!(deslugify("my_cool_post"), "My Cool Post");
        // numbers retained; mixed separators handled
        assert_eq!(deslugify("top-10-tips"), "Top 10 Tips");
        // per-line
        assert_eq!(deslugify("a-b\nc_d"), "A B\nC D");
    }

    #[test]
    fn csv_to_tsv_converts() {
        use super::csv_to_tsv;
        assert_eq!(csv_to_tsv("a,b,c"), "a\tb\tc");
        // quoted comma stays within one cell
        assert_eq!(csv_to_tsv("\"a,b\",c"), "a,b\tc");
        // multiple rows
        assert_eq!(csv_to_tsv("x,y\n1,2"), "x\ty\n1\t2");
    }

    #[test]
    fn tsv_to_csv_converts() {
        use super::{csv_to_tsv, tsv_to_csv};
        assert_eq!(tsv_to_csv("a\tb\tc"), "a,b,c");
        // a cell containing a comma gets quoted
        assert_eq!(tsv_to_csv("a,b\tc"), "\"a,b\",c");
        // round-trips with csv_to_tsv
        assert_eq!(tsv_to_csv(&csv_to_tsv("p,q\n\"r,s\",t")), "p,q\n\"r,s\",t");
    }

    #[test]
    fn strip_line_numbers_removes_prefix() {
        use super::strip_line_numbers;
        assert_eq!(strip_line_numbers("1. apple\n2. banana"), "apple\nbanana");
        // indented gutter numbers, and ':' / ')' separators
        assert_eq!(strip_line_numbers("  10  code"), "code");
        assert_eq!(strip_line_numbers("42: line\n3) item"), "line\nitem");
        // lines without a leading number are untouched
        assert_eq!(strip_line_numbers("noprefix"), "noprefix");
    }

    #[test]
    fn markdown_link_wraps() {
        use super::markdown_link;
        assert_eq!(
            markdown_link("Google", "https://google.com"),
            "[Google](https://google.com)"
        );
        assert_eq!(markdown_link("", "u"), "[](u)");
    }

    #[test]
    fn extract_urls_finds() {
        use super::extract_urls;
        assert_eq!(
            extract_urls("see https://a.com and http://b.org/x here"),
            "https://a.com\nhttp://b.org/x"
        );
        // no URLs → empty
        assert_eq!(extract_urls("no links here"), "");
    }

    #[test]
    fn extract_emails_finds() {
        use super::extract_emails;
        assert_eq!(
            extract_emails("contact a@b.com or c.d@e.org!"),
            "a@b.com\nc.d@e.org"
        );
        // no emails → empty
        assert_eq!(extract_emails("nothing here"), "");
    }

    #[test]
    fn extract_ips_finds() {
        use super::extract_ips;
        assert_eq!(
            extract_ips("from 192.168.1.1 to 10.0.0.255 done"),
            "192.168.1.1\n10.0.0.255"
        );
        // no IPs → empty
        assert_eq!(extract_ips("no ips here"), "");
    }

    #[test]
    fn extract_quoted_finds() {
        use super::extract_quoted;
        assert_eq!(extract_quoted(r#"a "hello" b "world""#), "hello\nworld");
        // escaped quote inside a string is kept within that string
        assert_eq!(extract_quoted(r#"x "a \"b\" c" y"#), r#"a \"b\" c"#);
        // no quotes → empty
        assert_eq!(extract_quoted("no quotes"), "");
    }

    #[test]
    fn extract_between_finds() {
        use super::extract_between;
        assert_eq!(extract_between("a(1)b(2)c", "(", ")"), "1\n2");
        // multi-char delimiters
        assert_eq!(
            extract_between("<x>foo</x><x>bar</x>", "<x>", "</x>"),
            "foo\nbar"
        );
        // unterminated start is ignored; no matches → empty
        assert_eq!(extract_between("a[1] b[", "[", "]"), "1");
        assert_eq!(extract_between("none", "[", "]"), "");
    }

    #[test]
    fn wrap_with_surrounds() {
        use super::wrap_with;
        assert_eq!(wrap_with("bold", "**"), "**bold**");
        assert_eq!(wrap_with("x", "~"), "~x~");
        assert_eq!(wrap_with("", "|"), "||");
    }

    #[test]
    fn extract_numbers_lines_finds() {
        use super::extract_numbers_lines;
        assert_eq!(extract_numbers_lines("a1 b2.5 c-3 d"), "1\n2.5\n-3");
        assert_eq!(extract_numbers_lines("price: $19.99!"), "19.99");
        // no numbers → empty
        assert_eq!(extract_numbers_lines("none"), "");
    }

    #[test]
    fn json_validate_checks() {
        use super::json_validate;
        assert!(json_validate(r#"{"a":1}"#).is_ok());
        assert!(json_validate("[1, 2, 3]").is_ok());
        assert!(json_validate("{bad}").is_err());
        assert!(json_validate("").is_err());
    }

    #[test]
    fn csv_validate_checks_columns() {
        use super::csv_validate;
        assert_eq!(csv_validate("a,b,c\n1,2,3\n4,5,6").unwrap(), 3);
        // a quoted comma does not inflate the field count
        assert_eq!(csv_validate("\"a,b\",c\n1,2").unwrap(), 2);
        // mismatched row count → error
        assert!(csv_validate("a,b\n1,2,3").is_err());
        assert!(csv_validate("").is_err());
    }

    #[test]
    fn ordered_list_numbers() {
        use super::ordered_list;
        assert_eq!(
            ordered_list("apple\nbanana\ncherry"),
            "1. apple\n2. banana\n3. cherry"
        );
        // an existing bullet is replaced, not doubled
        assert_eq!(ordered_list("- existing"), "1. existing");
        // blank lines kept and don't advance the counter
        assert_eq!(ordered_list("a\n\nb"), "1. a\n\n2. b");
    }

    #[test]
    fn strip_list_markers_cleans() {
        use super::strip_list_markers;
        assert_eq!(
            strip_list_markers("- apple\n* banana\n+ cherry"),
            "apple\nbanana\ncherry"
        );
        assert_eq!(strip_list_markers("1. one\n2) two"), "one\ntwo");
        assert_eq!(strip_list_markers("- [ ] todo\n- [x] done"), "todo\ndone");
        // lines without a marker are untouched
        assert_eq!(strip_list_markers("plain"), "plain");
    }

    #[test]
    fn sort_words_orders_within_line() {
        use super::sort_words;
        assert_eq!(sort_words("banana apple cherry"), "apple banana cherry");
        // per-line; whitespace runs collapse
        assert_eq!(sort_words("z  a  m\n3 1 2"), "a m z\n1 2 3");
    }

    #[test]
    fn unique_words_dedups_within_line() {
        use super::unique_words;
        assert_eq!(unique_words("a b a c b"), "a b c");
        assert_eq!(unique_words("x x x"), "x");
        // per-line, order preserved
        assert_eq!(unique_words("foo bar\nbaz baz qux"), "foo bar\nbaz qux");
    }

    #[test]
    fn sum_fields_totals_rows() {
        use super::sum_fields;
        assert_eq!(sum_fields("1 2 3\n10 20"), "6\n30");
        // non-numeric fields are ignored
        assert_eq!(sum_fields("a 5 b 3"), "8");
        // no numbers → 0
        assert_eq!(sum_fields("none"), "0");
    }

    #[test]
    fn avg_fields_averages_rows() {
        use super::avg_fields;
        assert_eq!(avg_fields("1 2 3\n10 20"), "2\n15");
        // non-numeric ignored
        assert_eq!(avg_fields("a 4 b 6"), "5");
        // no numbers → line unchanged
        assert_eq!(avg_fields("none"), "none");
    }

    #[test]
    fn reduce_fields_max_min() {
        use super::reduce_fields;
        assert_eq!(reduce_fields("1 5 3\n10 2", true), "5\n10");
        assert_eq!(reduce_fields("1 5 3\n10 2", false), "1\n2");
        // negatives handled; no-number line unchanged
        assert_eq!(reduce_fields("-4 -1 -9", true), "-1");
        assert_eq!(reduce_fields("none", false), "none");
    }

    #[test]
    fn range_fields_spread() {
        use super::range_fields;
        assert_eq!(range_fields("1 5 3\n10 2"), "4\n8");
        // single value → 0; no-number line unchanged
        assert_eq!(range_fields("7"), "0");
        assert_eq!(range_fields("none"), "none");
    }

    #[test]
    fn to_env_export_prefixes() {
        use super::to_env_export;
        assert_eq!(
            to_env_export("FOO=bar\nBAZ=qux"),
            "export FOO=bar\nexport BAZ=qux"
        );
        // comments, already-exported, and non-assignments are left alone
        assert_eq!(
            to_env_export("# c\nexport X=1\nplain"),
            "# c\nexport X=1\nplain"
        );
    }

    #[test]
    fn strip_export_removes_prefix() {
        use super::{strip_export, to_env_export};
        assert_eq!(strip_export("export FOO=bar\nexport X=1"), "FOO=bar\nX=1");
        // lines without the prefix are unchanged
        assert_eq!(strip_export("plain\nexport A=2"), "plain\nA=2");
        // round-trips with to_env_export
        assert_eq!(strip_export(&to_env_export("K=v")), "K=v");
    }

    #[test]
    fn line_ending_conversion() {
        use super::{dos2unix, unix2dos};
        assert_eq!(dos2unix("a\r\nb\r\nc"), "a\nb\nc");
        assert_eq!(unix2dos("a\nb"), "a\r\nb");
        // lone CR (old Mac) also normalized
        assert_eq!(dos2unix("x\ry"), "x\ny");
        // round-trip
        assert_eq!(dos2unix(&unix2dos("p\nq\nr")), "p\nq\nr");
    }

    #[test]
    fn percent_of_total_distributes() {
        use super::percent_of_total;
        assert_eq!(percent_of_total("25\n25\n50"), "25%\n25%\n50%");
        assert_eq!(percent_of_total("1\n3"), "25%\n75%");
        // non-numeric lines pass through; zero total leaves all unchanged
        assert_eq!(percent_of_total("a\nb"), "a\nb");
    }

    #[test]
    fn running_max_high_water() {
        use super::running_max;
        assert_eq!(running_max("1\n3\n2\n5\n4"), "1\n3\n3\n5\n5");
        assert_eq!(running_max("5\n2\n8"), "5\n5\n8");
        // non-numeric line passes through and doesn't affect the running max
        assert_eq!(running_max("3\nfoo\n2"), "3\nfoo\n3");
    }

    #[test]
    fn running_min_low_water() {
        use super::running_min;
        assert_eq!(running_min("5\n3\n4\n1\n2"), "5\n3\n3\n1\n1");
        assert_eq!(running_min("2\n8\n1"), "2\n2\n1");
        // non-numeric line passes through unchanged
        assert_eq!(running_min("4\nfoo\n6"), "4\nfoo\n4");
    }

    #[test]
    fn to_fixed_formats_decimals() {
        use super::to_fixed;
        assert_eq!(to_fixed("3.14159\n2", 2), "3.14\n2.00");
        // rounds; non-numeric passes through
        assert_eq!(to_fixed("1.7\nfoo", 0), "2\nfoo");
    }

    #[test]
    fn clamp_lines_bounds() {
        use super::clamp_lines;
        assert_eq!(clamp_lines("5\n-3\n10\n7", 0.0, 8.0), "5\n0\n8\n7");
        // non-numeric passes through
        assert_eq!(clamp_lines("foo\n2", 0.0, 1.0), "foo\n1");
    }

    #[test]
    fn scale_lines_multiplies() {
        use super::scale_lines;
        assert_eq!(scale_lines("1\n2\n3", 10.0), "10\n20\n30");
        // fractional factor; non-numeric passes through
        assert_eq!(scale_lines("5\nfoo", 0.5), "2.5\nfoo");
    }

    #[test]
    fn offset_lines_adds() {
        use super::offset_lines;
        assert_eq!(offset_lines("1\n2\n3", 10.0), "11\n12\n13");
        // negative offset subtracts; non-numeric passes through
        assert_eq!(offset_lines("5\nfoo", -2.0), "3\nfoo");
    }

    #[test]
    fn abs_lines_absolute() {
        use super::abs_lines;
        assert_eq!(abs_lines("-3\n5\n-2.5"), "3\n5\n2.5");
        // non-numeric passes through
        assert_eq!(abs_lines("foo\n-1"), "foo\n1");
    }

    #[test]
    fn linkify_wraps_urls() {
        use super::linkify;
        assert_eq!(
            linkify("see https://a.com here"),
            "see [https://a.com](https://a.com) here"
        );
        // multiple URLs; text without URLs unchanged
        assert_eq!(linkify("a http://x.io b"), "a [http://x.io](http://x.io) b");
        assert_eq!(linkify("no links"), "no links");
    }

    #[test]
    fn strip_markdown_links_to_text() {
        use super::{linkify, strip_markdown_links};
        assert_eq!(
            strip_markdown_links("see [Google](https://g.com) now"),
            "see Google now"
        );
        assert_eq!(strip_markdown_links("[a](u) and [b](v)"), "a and b");
        // text without links unchanged; round-trips a linkified bare URL back to bare
        assert_eq!(strip_markdown_links("plain"), "plain");
        assert_eq!(
            strip_markdown_links(&linkify("x https://a.com y")),
            "x https://a.com y"
        );
    }

    #[test]
    fn strip_emphasis_plain() {
        use super::strip_emphasis;
        assert_eq!(
            strip_emphasis("**bold** and *italic* and `code`"),
            "bold and italic and code"
        );
        assert_eq!(strip_emphasis("__b__ and _i_"), "b and i");
        // text without paired markers is unchanged (single underscore kept)
        assert_eq!(strip_emphasis("plain snake_case"), "plain snake_case");
    }

    #[test]
    fn strip_html_comments_removes() {
        use super::strip_html_comments;
        assert_eq!(strip_html_comments("a<!-- x -->b"), "ab");
        // multi-line comment removed
        assert_eq!(strip_html_comments("<!--\nmulti\n-->c"), "c");
        // no comments → unchanged
        assert_eq!(strip_html_comments("no comments"), "no comments");
    }

    #[test]
    fn remove_trailing_commas_strict() {
        use super::remove_trailing_commas;
        assert_eq!(remove_trailing_commas("[1, 2, 3,]"), "[1, 2, 3]");
        // trailing comma before a newline + brace
        assert_eq!(remove_trailing_commas("{\"a\": 1,\n}"), "{\"a\": 1\n}");
        // valid input unchanged
        assert_eq!(remove_trailing_commas("[1, 2]"), "[1, 2]");
    }

    #[test]
    fn add_trailing_commas_adds() {
        use super::{add_trailing_commas, remove_trailing_commas};
        assert_eq!(add_trailing_commas("[1, 2]"), "[1, 2,]");
        assert_eq!(add_trailing_commas("{\"a\": 1\n}"), "{\"a\": 1,\n}");
        // empty containers untouched; idempotent (already has comma → unchanged)
        assert_eq!(add_trailing_commas("[]"), "[]");
        assert_eq!(add_trailing_commas("[1, 2,]"), "[1, 2,]");
        // add then remove returns the original
        assert_eq!(
            remove_trailing_commas(&add_trailing_commas("[1, 2]")),
            "[1, 2]"
        );
    }

    #[test]
    fn smart_quotes_curlies() {
        use super::smart_quotes;
        assert_eq!(smart_quotes("\"hi\""), "\u{201C}hi\u{201D}");
        // apostrophe after a letter becomes a closing single quote
        assert_eq!(smart_quotes("it's"), "it\u{2019}s");
        assert_eq!(smart_quotes("'quoted'"), "\u{2018}quoted\u{2019}");
    }

    #[test]
    fn typographic_dashes_substitutes() {
        use super::typographic_dashes;
        assert_eq!(typographic_dashes("a--b"), "a\u{2013}b");
        // longest first: --- becomes an em dash, not en-dash + hyphen
        assert_eq!(typographic_dashes("a---b"), "a\u{2014}b");
        assert_eq!(typographic_dashes("wait..."), "wait\u{2026}");
    }

    #[test]
    fn de_typography_to_ascii() {
        use super::de_typography;
        assert_eq!(de_typography("\u{201C}hi\u{201D}"), "\"hi\"");
        assert_eq!(de_typography("it\u{2019}s"), "it's");
        assert_eq!(de_typography("a\u{2014}b and c\u{2013}d"), "a---b and c--d");
        assert_eq!(de_typography("wait\u{2026}"), "wait...");
    }

    #[test]
    fn to_ascii_transliterates() {
        use super::to_ascii;
        assert_eq!(to_ascii("café"), "cafe");
        assert_eq!(to_ascii("naïve Müller Señor"), "naive Muller Senor");
        // multi-char expansions
        assert_eq!(to_ascii("straße"), "strasse");
        assert_eq!(to_ascii("Æsop œuvre"), "AEsop oeuvre");
        // plain ASCII unchanged
        assert_eq!(to_ascii("plain"), "plain");
    }

    #[test]
    fn nato_spells() {
        use super::nato_spell;
        assert_eq!(nato_spell("AB1"), "Alfa Bravo One");
        // case-insensitive; unknown chars dropped
        assert_eq!(nato_spell("Hi!"), "Hotel India");
        assert_eq!(nato_spell(""), "");
    }

    #[test]
    fn transpose_grid_swaps() {
        use super::transpose_grid;
        assert_eq!(transpose_grid("1 2 3\n4 5 6"), "1 4\n2 5\n3 6");
        assert_eq!(transpose_grid("a b\nc d\ne f"), "a c e\nb d f");
        // double transpose is the identity for a full grid
        assert_eq!(transpose_grid(&transpose_grid("1 2\n3 4")), "1 2\n3 4");
    }

    #[test]
    fn repeat_lines_replicates() {
        use super::repeat_lines;
        assert_eq!(repeat_lines("a\nb", 2), "a\na\nb\nb");
        assert_eq!(repeat_lines("x", 3), "x\nx\nx");
        // n=1 leaves the input unchanged
        assert_eq!(repeat_lines("a\nb", 1), "a\nb");
    }

    #[test]
    fn wrap_in_tag_wraps() {
        use super::wrap_in_tag;
        assert_eq!(wrap_in_tag("text", "b"), "<b>text</b>");
        assert_eq!(wrap_in_tag("a & b", "div"), "<div>a & b</div>");
        assert_eq!(wrap_in_tag("", "br"), "<br></br>");
    }

    #[test]
    fn grep_command_builds() {
        use super::grep_command;
        // pattern is single-quoted, present in both the rg and grep fallback,
        // and the rg branch is tried first with a `||` fallback to grep.
        assert_eq!(
            grep_command("foo bar", false),
            "rg --vimgrep --color=never -e 'foo bar' 2>/dev/null || grep -rnHIE -e 'foo bar' ."
        );
        // whole-word search adds -w to both branches
        assert_eq!(
            grep_command("foo", true),
            "rg --vimgrep --color=never -w -e 'foo' 2>/dev/null || grep -rnHIE -w -e 'foo' ."
        );
        // an injection attempt stays inert inside the single quotes
        assert!(grep_command("$(rm -rf /)", false).contains("'$(rm -rf /)'"));
    }

    #[test]
    fn squeeze_blank_collapses() {
        use super::squeeze_blank_lines;
        assert_eq!(squeeze_blank_lines("a\n\n\n\nb\n"), "a\n\nb\n");
        // whitespace-only lines count as blank and collapse
        assert_eq!(squeeze_blank_lines("a\n  \n\t\nb\n"), "a\n  \nb\n");
        // nothing to collapse
        assert_eq!(squeeze_blank_lines("a\nb\n"), "a\nb\n");
        // leading run collapses to one
        assert_eq!(squeeze_blank_lines("\n\nx\n"), "\nx\n");
    }

    #[test]
    fn dedup_adjacent_collapses() {
        use super::dedup_adjacent_lines;
        // adjacent repeats collapse; the non-adjacent `a` at the end is kept
        assert_eq!(dedup_adjacent_lines("a\na\nb\nb\na\n"), "a\nb\na\n");
        // nothing to collapse
        assert_eq!(dedup_adjacent_lines("x\ny\nz\n"), "x\ny\nz\n");
        // no trailing newline preserved
        assert_eq!(dedup_adjacent_lines("p\np\nq"), "p\nq");
    }

    #[test]
    fn number_lines_prefixes() {
        use super::number_lines;
        assert_eq!(number_lines("a\nb\nc\n", 1), "1. a\n2. b\n3. c\n");
        // width aligns when the range crosses into two digits
        assert_eq!(number_lines("x\ny\n", 9), " 9. x\n10. y\n");
        // custom start, no trailing newline
        assert_eq!(number_lines("only", 5), "5. only");
    }

    #[test]
    fn split_on_sep_splits() {
        use super::split_on_sep;
        assert_eq!(split_on_sep("a, b, c\n", ", "), "a\nb\nc\n");
        assert_eq!(split_on_sep("x|y|z", "|"), "x\ny\nz");
        // no separator present → single line unchanged
        assert_eq!(split_on_sep("solo\n", ","), "solo\n");
        // empty separator is a no-op
        assert_eq!(split_on_sep("a,b", ""), "a,b");
        // round-trips with join_lines_with for a simple case
        assert_eq!(
            super::join_lines_with(&split_on_sep("a,b,c\n", ","), ","),
            "a,b,c\n"
        );
    }

    #[test]
    fn join_lines_with_sep() {
        use super::join_lines_with;
        // trailing newline preserved on the single joined line
        assert_eq!(join_lines_with("a\nb\nc\n", ", "), "a, b, c\n");
        // no trailing newline
        assert_eq!(join_lines_with("a\nb", "|"), "a|b");
        // empty separator concatenates
        assert_eq!(join_lines_with("1\n2\n3\n", ""), "123\n");
        // single line is unchanged
        assert_eq!(join_lines_with("solo\n", ", "), "solo\n");
    }

    #[test]
    fn whole_word_ranges_matches() {
        use super::whole_word_ranges;
        // only the standalone `foo`, not the one inside `foobar` or `_foo`
        assert_eq!(
            whole_word_ranges("foo foobar _foo foo", "foo"),
            vec![(0, 3), (16, 19)]
        );
        // adjacency via underscore/digits blocks a match
        assert_eq!(whole_word_ranges("x x2 x_ x", "x"), vec![(0, 1), (8, 9)]);
        // no matches / empty word
        assert!(whole_word_ranges("bar", "foo").is_empty());
        assert!(whole_word_ranges("foo", "").is_empty());
    }

    #[test]
    fn word_at_col_extracts() {
        use super::word_at_col;
        assert_eq!(word_at_col("foo bar", 0).as_deref(), Some("foo"));
        assert_eq!(word_at_col("foo bar", 4).as_deref(), Some("bar"));
        // on the space, fall back to the word just before
        assert_eq!(word_at_col("foo bar", 3).as_deref(), Some("foo"));
        // identifiers with underscores and digits
        assert_eq!(
            word_at_col("let my_var2 = 1", 8).as_deref(),
            Some("my_var2")
        );
        // cursor past the end falls back to the trailing word
        assert_eq!(word_at_col("end", 3).as_deref(), Some("end"));
        // no word here
        assert_eq!(word_at_col("  ", 0), None);
        assert_eq!(word_at_col("a.b", 1).as_deref(), Some("a"));
    }

    #[test]
    fn shell_single_quote_escapes() {
        use super::shell_single_quote as q;
        assert_eq!(q("foo"), "'foo'");
        assert_eq!(q("a b"), "'a b'");
        // embedded single quote is closed, escaped, and reopened
        assert_eq!(q("it's"), "'it'\\''s'");
        // shell metacharacters are inert inside single quotes
        assert_eq!(q("$(rm -rf /)"), "'$(rm -rf /)'");
        assert_eq!(q(""), "''");
    }

    #[test]
    fn extract_numbers_scans() {
        use super::extract_numbers;
        // a column of numbers including a negative and a decimal
        assert_eq!(extract_numbers("10\n-5\n3.5\n"), vec![10.0, -5.0, 3.5]);
        // comma/space separated, sign at boundary is attached
        assert_eq!(extract_numbers("a=1, b=-2, c=3"), vec![1.0, -2.0, 3.0]);
        // a minus between digits is subtraction context → two positives
        assert_eq!(extract_numbers("1-2"), vec![1.0, 2.0]);
        // scientific notation and leading-dot decimals
        assert_eq!(extract_numbers("1.5e3 and .25"), vec![1500.0, 0.25]);
        // no numbers
        assert!(extract_numbers("no digits here").is_empty());
    }

    #[test]
    fn clamp_offset_bounds() {
        use super::clamp_offset;
        assert_eq!(clamp_offset(5, 10), 5);
        assert_eq!(clamp_offset(10, 10), 10); // EOF position allowed
        assert_eq!(clamp_offset(99, 10), 10); // clamped to len
        assert_eq!(clamp_offset(0, 0), 0); // empty buffer
    }

    #[test]
    fn pad_numbers_zero_pads() {
        use super::pad_numbers;
        assert_eq!(pad_numbers("a1 b22 c333", 3), "a001 b022 c333");
        assert_eq!(pad_numbers("file9", 2), "file09");
        // never truncates a longer number
        assert_eq!(pad_numbers("1234", 2), "1234");
        assert_eq!(pad_numbers("no digits", 3), "no digits");
    }

    #[test]
    fn increment_numbers_in_text() {
        use super::increment_numbers;
        assert_eq!(increment_numbers("v1.2.3", 1), "v2.3.4");
        assert_eq!(increment_numbers("a 9 b 10", 1), "a 10 b 11");
        assert_eq!(increment_numbers("count: 5", -1), "count: 4");
        // no digits → unchanged
        assert_eq!(increment_numbers("none here", 1), "none here");
    }

    #[test]
    fn parse_int_and_bases() {
        use super::{format_bases, parse_int_any};
        assert_eq!(parse_int_any("255"), Some(255));
        assert_eq!(parse_int_any("0xff"), Some(255));
        assert_eq!(parse_int_any("0b11111111"), Some(255));
        assert_eq!(parse_int_any("0o377"), Some(255));
        assert_eq!(parse_int_any("-0x10"), Some(-16));
        assert_eq!(parse_int_any("notanumber"), None);
        assert_eq!(format_bases(255), "255 = 0xff = 0o377 = 0b11111111");
    }

    #[test]
    fn lorem_generates() {
        use super::lorem;
        assert_eq!(lorem(0), "");
        assert_eq!(lorem(1), "Lorem.");
        assert_eq!(lorem(3), "Lorem ipsum dolor.");
        // exactly N words (counted by spaces + 1) and ends with a period
        let text = lorem(50);
        assert_eq!(text.trim_end_matches('.').split(' ').count(), 50);
        assert!(text.ends_with('.'));
        // first word is capitalized
        assert!(text.starts_with("Lorem"));
    }

    #[test]
    fn format_utc_dates() {
        use super::format_utc;
        assert_eq!(format_utc(0, false), "1970-01-01");
        assert_eq!(format_utc(0, true), "1970-01-01 00:00:00");
        // a known instant: 1700000000 == 2023-11-14 22:13:20 UTC
        assert_eq!(format_utc(1_700_000_000, true), "2023-11-14 22:13:20");
        // a leap day: 2024-02-29 12:30:45 UTC == 1709209845
        assert_eq!(format_utc(1_709_209_845, true), "2024-02-29 12:30:45");
    }

    #[test]
    fn uuid_v4_format() {
        use super::{format_uuid_v4, random_bytes_16};
        // all-zero input: version nibble is 4, variant nibble is 8, dashes placed
        assert_eq!(
            format_uuid_v4([0u8; 16]),
            "00000000-0000-4000-8000-000000000000"
        );
        // all-ones input: version still 4, variant still 10xx (=> 'b')
        assert_eq!(
            format_uuid_v4([0xff; 16]),
            "ffffffff-ffff-4fff-bfff-ffffffffffff"
        );
        // a real generated UUID has the right shape: 8-4-4-4-12, version 4, variant 8/9/a/b
        let u = format_uuid_v4(random_bytes_16());
        let parts: Vec<&str> = u.split('-').collect();
        assert_eq!(
            parts.iter().map(|p| p.len()).collect::<Vec<_>>(),
            vec![8, 4, 4, 4, 12]
        );
        assert!(u.chars().all(|c| c.is_ascii_hexdigit() || c == '-'));
        assert_eq!(parts[2].as_bytes()[0], b'4');
        assert!(matches!(parts[3].as_bytes()[0], b'8' | b'9' | b'a' | b'b'));
    }

    #[test]
    fn calc_evaluates() {
        use super::{eval_arith, format_calc};
        assert_eq!(eval_arith("2+3*4").unwrap(), 14.0);
        assert_eq!(eval_arith("(2+3)*4").unwrap(), 20.0);
        assert_eq!(eval_arith("-5 + 3").unwrap(), -2.0);
        assert_eq!(eval_arith("2^10").unwrap(), 1024.0);
        assert_eq!(eval_arith("10 / 4").unwrap(), 2.5);
        assert_eq!(eval_arith("10 % 3").unwrap(), 1.0);
        assert_eq!(eval_arith("(-2)^2").unwrap(), 4.0);
        assert_eq!(eval_arith("1.5e2").unwrap(), 150.0);
        // errors
        assert!(eval_arith("2 +").is_err());
        assert!(eval_arith("1/0").is_err());
        assert!(eval_arith("2 3").is_err()); // trailing garbage
        assert!(eval_arith("(1+2").is_err()); // unbalanced
                                              // formatting: integers are clean, fractions trimmed
        assert_eq!(format_calc(14.0), "14");
        assert_eq!(format_calc(2.5), "2.5");
        assert_eq!(format_calc(7.0), "7");
    }

    #[test]
    fn exec_bits_added() {
        use super::with_exec_bits;
        // rw-r--r-- (0o644) → rwxr-xr-x (0o755)
        assert_eq!(with_exec_bits(0o644), 0o755);
        // already executable stays executable
        assert_eq!(with_exec_bits(0o755), 0o755);
        // rw------- (0o600) → rwx--x--x (0o711)
        assert_eq!(with_exec_bits(0o600), 0o711);
    }

    #[test]
    fn mkdir_target_resolution() {
        use std::path::{Path, PathBuf};
        // explicit arg wins
        assert_eq!(
            resolve_mkdir_target(Some("a/b/c"), Some(Path::new("/x/y/file.rs"))),
            Some(PathBuf::from("a/b/c"))
        );
        // blank/whitespace arg falls back to the current file's parent
        assert_eq!(
            resolve_mkdir_target(Some("  "), Some(Path::new("/x/y/file.rs"))),
            Some(PathBuf::from("/x/y"))
        );
        // no arg → current file's parent
        assert_eq!(
            resolve_mkdir_target(None, Some(Path::new("/x/y/file.rs"))),
            Some(PathBuf::from("/x/y"))
        );
        // no arg and no file → nothing to create
        assert_eq!(resolve_mkdir_target(None, None), None);
        // a bare filename with no parent dir yields nothing
        assert_eq!(resolve_mkdir_target(None, Some(Path::new("file.rs"))), None);
    }

    #[test]
    fn align_on_delim_columns() {
        // basic = alignment, single space before the delimiter
        assert_eq!(
            align_on_delim(&["foo = 1", "barbar = 2"], "="),
            vec!["foo    = 1", "barbar = 2"]
        );
        // collapses extra pre-delimiter whitespace, preserves indentation
        assert_eq!(
            align_on_delim(&["  a   = 1", "  bb = 2"], "="),
            vec!["  a  = 1", "  bb = 2"]
        );
        // lines without the delimiter are left untouched
        assert_eq!(
            align_on_delim(&["x = 1", "# comment", "yy = 2"], "="),
            vec!["x  = 1", "# comment", "yy = 2"]
        );
        // multi-char delimiter
        assert_eq!(
            align_on_delim(&["a => 1", "bbb => 2"], "=>"),
            vec!["a   => 1", "bbb => 2"]
        );
    }

    #[test]
    fn sort_by_field_orders() {
        use super::sort_lines_by_field;
        // sort by the 2nd field
        assert_eq!(sort_lines_by_field("b 2\na 3\nc 1\n", 2), "c 1\nb 2\na 3\n");
        // sort by the 1st field
        assert_eq!(sort_lines_by_field("b 2\na 3\nc 1\n", 1), "a 3\nb 2\nc 1\n");
        // no trailing newline preserved
        assert_eq!(sort_lines_by_field("x 9\ny 1", 2), "y 1\nx 9");
    }

    #[test]
    fn sort_line_block_variants() {
        // plain ascending sort, trailing newline preserved
        assert_eq!(
            sort_line_block("b\na\nc\n", false, false, false, false),
            "a\nb\nc\n"
        );
        // reverse (descending)
        assert_eq!(
            sort_line_block("b\na\nc\n", true, false, false, false),
            "c\nb\na\n"
        );
        // case-insensitive groups regardless of case
        assert_eq!(
            sort_line_block("B\na\nC\n", false, true, false, false),
            "a\nB\nC\n"
        );
        // numeric sorts by value, not lexically (10 after 2)
        assert_eq!(
            sort_line_block("10\n2\n1\n", false, false, true, false),
            "1\n2\n10\n"
        );
        // unique drops duplicate lines after sorting
        assert_eq!(
            sort_line_block("b\na\nb\na\n", false, false, false, true),
            "a\nb\n"
        );
        // no trailing newline is preserved
        assert_eq!(sort_line_block("b\na", false, false, false, false), "a\nb");
    }

    #[test]
    fn leading_number_parses() {
        assert_eq!(leading_number("42 foo"), 42.0);
        assert_eq!(leading_number("  -3.5 bar"), -3.5);
        assert_eq!(leading_number("no number"), f64::NEG_INFINITY);
        assert_eq!(leading_number("100"), 100.0);
    }

    #[test]
    fn reverse_line_order_preserves_eol() {
        // trailing newline preserved
        assert_eq!(reverse_line_order("a\nb\nc\n"), "c\nb\na\n");
        // no trailing newline preserved
        assert_eq!(reverse_line_order("a\nb\nc"), "c\nb\na");
        // single line is unchanged either way
        assert_eq!(reverse_line_order("only\n"), "only\n");
        assert_eq!(reverse_line_order("only"), "only");
        // blank interior lines keep their place when reversed
        assert_eq!(reverse_line_order("a\n\nb\n"), "b\n\na\n");
        // empty input
        assert_eq!(reverse_line_order(""), "");
    }

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
        assert_eq!(
            tr("nu", false),
            ("line-number".into(), Value::String("absolute".into()))
        );
        assert_eq!(
            tr("rnu", false),
            ("line-number".into(), Value::String("relative".into()))
        );
        assert_eq!(
            tr("norelativenumber", false),
            ("line-number".into(), Value::String("absolute".into()))
        );
        assert_eq!(
            tr("tw=80", false),
            ("text-width".into(), Value::Number(80.into()))
        );
        assert_eq!(
            tr("nowrap", false),
            ("soft-wrap.enable".into(), Value::Bool(false))
        );
        assert_eq!(
            tr("wrap", false),
            ("soft-wrap.enable".into(), Value::Bool(true))
        );
        // toggle from current=false -> true
        assert_eq!(
            tr("cursorline!", false),
            ("cursorline".into(), Value::Bool(true))
        );
        assert_eq!(
            tr("scrolloff=5", false),
            ("scrolloff".into(), Value::Number(5.into()))
        );
        // unknown option -> None
        assert!(translate_vim_option("definitelynotanoption", |_| false).is_none());
    }
}
