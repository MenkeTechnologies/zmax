//! vim `-S {file}`: a session script is sourced after the command-line files
//! have loaded, through the real startup entry points (`load_init_scripts`,
//! then `source_startup_files`), as `zmax -S sess.vim + file` runs them.
//!
//! The script is a session as vim-session / `:mksession` write it: `cd`,
//! `badd`, `edit`, window options, then `exe {line}` + `normal! zt` to put the
//! cursor back. Own test binary so its `HOME` override doesn't race other tests.
//! Requires the `integration` + `scripting` features.
#![cfg(all(feature = "integration", feature = "scripting", unix))]

#[allow(dead_code, unused_imports, clippy::all)]
mod helpers {
    include!("test/helpers.rs");
}

use helpers::{test_config, test_syntax_loader};
use zmax_core::Position;
use zmax_loader::workspace_trust::WorkspaceTrust;
use zmax_term::{application::Application, args::Args};

const SESSION: &str = r#"" Vim session script.
if exists('g:syntax_on') != 1 | syntax on | endif
if exists('g:did_load_filetypes') != 1 | filetype on | endif
call setqflist([])
let SessionLoad = 1
let s:so_save = &so | let s:siso_save = &siso | set so=0 siso=0
let v:this_session=expand("<sfile>:p")
silent only
cd DIR
if expand('%') == '' && !&modified && line('$') <= 1 && getline(1) == ''
  let s:wipebuf = bufnr('%')
endif
set shortmess=aoO
badd +0 notes.txt
argglobal
%argdel
$argadd notes.txt
edit notes.txt
set splitbelow splitright
set nosplitbelow
wincmd t
set winminheight=0
set winheight=1
argglobal
setlocal fdm=marker
setlocal fen
let s:l = 30 - ((5 * winheight(0) + 10) / 20)
if s:l < 1 | let s:l = 1 | endif
exe s:l
normal! zt
30
normal! 0
tabnext 1
set winheight=1 winwidth=20 winminheight=1 winminwidth=1 shortmess=aIc
let s:sx = expand("<sfile>:p:r")."x.vim"
if file_readable(s:sx)
  exe "source " . fnameescape(s:sx)
endif
let &so = s:so_save | let &siso = s:siso_save
1wincmd w
tabnext 1
doautoall SessionLoadPost
unlet SessionLoad
"#;

#[tokio::test(flavor = "multi_thread")]
async fn dash_s_restores_a_session_over_the_command_line_file() -> anyhow::Result<()> {
    let dir = std::env::temp_dir().join(format!("zmax-dash-s-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join(".zmax"))?;
    std::env::set_var("HOME", &dir);

    let notes = dir.join("notes.txt");
    let text: String = (1..=50).map(|n| format!("line {n}\n")).collect();
    std::fs::write(&notes, text)?;
    let session = dir.join("sess.vim");
    std::fs::write(&session, SESSION.replace("DIR", &dir.to_string_lossy()))?;

    // `zmax -S sess.vim + notes.txt`: `+` puts the cursor on the last line,
    // then the session moves it to line 30.
    let mut args = Args::default();
    args.files
        .insert(notes.clone(), vec![Position::new(usize::MAX, 0)]);
    args.source_files.push(session.clone());
    let source_files = args.source_files.clone();

    let mut app = Application::new(
        args,
        test_config(),
        test_syntax_loader(None),
        WorkspaceTrust::fully_trusted(),
    )?;
    app.load_init_scripts();
    app.source_startup_files(&source_files);

    assert!(!app.editor.is_err(), "{:?}", app.editor.get_status());
    let (view, doc) = zmax_view::current_ref!(app.editor);
    assert_eq!(doc.path(), Some(notes.as_path()), "the session's `edit` buffer");
    let text = doc.text().slice(..);
    let cursor = doc.selection(view.id).primary().cursor(text);
    assert_eq!(text.char_to_line(cursor), 29, "the session's `exe 30`");

    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn dash_s_reports_a_missing_script() -> anyhow::Result<()> {
    let missing = std::env::temp_dir().join("zmax-dash-s-missing/Session.vim");
    let mut app = Application::new(
        Args::default(),
        test_config(),
        test_syntax_loader(None),
        WorkspaceTrust::fully_trusted(),
    )?;
    app.source_startup_files(std::slice::from_ref(&missing));

    let (status, _) = app.editor.get_status().expect("an error status");
    assert!(
        app.editor.is_err() && status.contains("Can't open file"),
        "{status}"
    );
    Ok(())
}


/// Boot `zmax -S {session}` over `files` in a fresh `HOME` holding `files`,
/// with `DIR` in the session replaced by that directory.
fn boot_session(name: &str, files: &[(&str, usize)], session: &str) -> anyhow::Result<(Application, std::path::PathBuf)> {
    let dir = std::env::temp_dir().join(format!("zmax-dash-s-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join(".zmax"))?;
    std::env::set_var("HOME", &dir);
    for (file, lines) in files {
        let text: String = (1..=*lines).map(|n| format!("line {n}\n")).collect();
        std::fs::write(dir.join(file), text)?;
    }
    let script = dir.join("sess.vim");
    std::fs::write(&script, session.replace("DIR", &dir.to_string_lossy()))?;

    let mut args = Args::default();
    args.files.insert(dir.join(files[0].0), vec![Position::default()]);
    args.source_files.push(script);
    let source_files = args.source_files.clone();
    let mut app = Application::new(
        args,
        test_config(),
        test_syntax_loader(None),
        WorkspaceTrust::fully_trusted(),
    )?;
    app.load_init_scripts();
    app.source_startup_files(&source_files);
    Ok((app, dir))
}

/// vim `:badd +lnum {fname}` — what a session writes for every buffer — lists
/// the file and lands on `lnum` when it is entered. The `+lnum` is not a file:
/// it must not become a buffer named `+30`.
#[tokio::test(flavor = "multi_thread")]
async fn session_badd_lnum_is_the_entry_line_not_a_buffer() -> anyhow::Result<()> {
    let (mut app, dir) = boot_session(
        "badd",
        &[("notes.txt", 50), ("other.txt", 50)],
        // Absolute paths: a `cd` would move the cwd under the parallel tests.
        "badd +30 DIR/notes.txt\nbadd +7 DIR/other.txt\nbadd +0 DIR/notes.txt\nbuffer other.txt\n",
    )?;

    assert!(!app.editor.is_err(), "{:?}", app.editor.get_status());
    let names: Vec<_> = app
        .editor
        .documents()
        .filter_map(|d| d.path().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()))
        .collect();
    assert!(
        names.iter().all(|n| !n.starts_with('+')),
        "a `+lnum` became a buffer: {names:?}"
    );
    let (view, doc) = zmax_view::current_ref!(app.editor);
    assert_eq!(doc.path(), Some(dir.join("other.txt").as_path()));
    let text = doc.text().slice(..);
    let cursor = doc.selection(view.id).primary().cursor(text);
    assert_eq!(text.char_to_line(cursor), 6, "`badd +7` enters on line 7");

    // The command-line file was showing when its `badd +30` ran: vim leaves
    // a displayed buffer's cursor alone.
    app.editor.switch(
        app.editor.document_by_path(dir.join("notes.txt")).unwrap().id(),
        zmax_view::editor::Action::Replace,
    );
    let (view, doc) = zmax_view::current_ref!(app.editor);
    let text = doc.text().slice(..);
    assert_eq!(text.char_to_line(doc.selection(view.id).primary().cursor(text)), 0);

    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

/// `winheight(0)` is the window's text height, so a session's
/// `let s:l = N - ((K * winheight(0) + …) / …)` scroll lands where vim's does.
/// Standalone vimlrs answers -1, which collapsed every `s:l` to `N`.
#[tokio::test(flavor = "multi_thread")]
async fn session_winheight_measures_the_window() -> anyhow::Result<()> {
    let (app, dir) = boot_session("winheight", &[("notes.txt", 400)], "exe winheight(0)\n")?;

    assert!(!app.editor.is_err(), "{:?}", app.editor.get_status());
    let (view, doc) = zmax_view::current_ref!(app.editor);
    let text = doc.text().slice(..);
    let line = text.char_to_line(doc.selection(view.id).primary().cursor(text));
    assert!(view.inner_height() > 1);
    assert_eq!(line + 1, view.inner_height(), "`exe winheight(0)` goes to that line");

    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

/// A session scrolls with `exe s:l | normal! zt` and then moves the cursor:
/// the `zt` must run in script order, leaving line `s:l` at the top of the
/// window with the cursor below it, as vim does.
#[tokio::test(flavor = "multi_thread")]
async fn session_normal_zt_scrolls_before_the_cursor_moves() -> anyhow::Result<()> {
    let (app, dir) = boot_session(
        "zt",
        &[("notes.txt", 400)],
        "set so=0\nexe 100\nnormal! zt\n110\nnormal! 0\n",
    )?;

    assert!(!app.editor.is_err(), "{:?}", app.editor.get_status());
    let (view, doc) = zmax_view::current_ref!(app.editor);
    let text = doc.text().slice(..);
    let cursor = doc.selection(view.id).primary().cursor(text);
    assert_eq!(text.char_to_line(cursor), 109, "the session's `110`");
    let top = text.char_to_line(doc.view_offset(view.id).anchor);
    assert_eq!(top, 99, "`normal! zt` on line 100 puts it at the top");

    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

/// A session lays out each tab page in turn and ends with `tabnext 1`: the
/// first tab's window must come back at the top line its `normal! zt` gave
/// it, not re-scrolled to its cursor, as vim restores each window's topline.
#[tokio::test(flavor = "multi_thread")]
async fn session_tabnext_restores_the_window_scroll() -> anyhow::Result<()> {
    let (app, dir) = boot_session(
        "tabs",
        &[("notes.txt", 400), ("other.txt", 50)],
        "set so=0\nedit DIR/notes.txt\nexe 100\nnormal! zt\n110\ntabedit DIR/other.txt\n5\ntabnext 1\n",
    )?;

    assert!(!app.editor.is_err(), "{:?}", app.editor.get_status());
    let (view, doc) = zmax_view::current_ref!(app.editor);
    assert_eq!(doc.path(), Some(dir.join("notes.txt").as_path()));
    let text = doc.text().slice(..);
    let cursor = doc.selection(view.id).primary().cursor(text);
    assert_eq!(text.char_to_line(cursor), 109);
    let top = text.char_to_line(doc.view_offset(view.id).anchor);
    assert_eq!(top, 99, "tab 1 keeps its `normal! zt` scroll across tabnext");

    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}
