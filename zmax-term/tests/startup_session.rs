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

