//! zmax can source an arbitrary user-named config file in EITHER language:
//! `source-elisp-file` (Emacs Lisp) and `source-viml-file` (Vimscript). Both are
//! off by default; when both are set the Emacs Lisp file is sourced first. This
//! drives the real startup entry point `Application::load_init_scripts`.
//!
//! Own test binary (own process) so its `HOME`/config overrides don't race the
//! other init tests. Requires the `integration` + `scripting` features.
#![cfg(all(feature = "integration", feature = "scripting", unix))]

#[allow(dead_code, unused_imports, clippy::all)]
mod helpers {
    include!("test/helpers.rs");
}

use helpers::{test_config, test_syntax_loader};
use zmax_loader::workspace_trust::WorkspaceTrust;
use zmax_term::{application::Application, args::Args};
use zmax_view::theme::Color;

fn make_app(elisp: Option<String>, viml: Option<String>) -> anyhow::Result<Application> {
    let mut config = test_config();
    config.editor.source_elisp_file = elisp;
    config.editor.source_viml_file = viml;
    Ok(Application::new(
        Args::default(),
        config,
        test_syntax_loader(None),
        WorkspaceTrust::fully_trusted(),
    )?)
}

#[tokio::test(flavor = "multi_thread")]
async fn arbitrary_elisp_and_viml_files_are_sourced() -> anyhow::Result<()> {
    let dir = std::env::temp_dir().join(format!("zmax-custom-src-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join(".zmax"))?;
    std::env::set_var("HOME", &dir);

    // An Emacs Lisp file that inserts a marker into the buffer.
    let el = dir.join("my-init.el");
    std::fs::write(&el, "(insert \"ELISP_SOURCED\")\n")?;
    // A Vimscript file that sets a highlight (no colorscheme/runtimepath needed).
    let vim = dir.join("my-init.vim");
    std::fs::write(&vim, "highlight Normal guifg=#abcdef\n")?;

    let mut app = make_app(
        Some(el.to_string_lossy().into_owned()),
        Some(vim.to_string_lossy().into_owned()),
    )?;

    app.load_init_scripts();

    // The Emacs Lisp file ran: its inserted marker is in the buffer.
    let buf = app.editor.documents().next().unwrap().text().to_string();
    assert!(
        buf.contains("ELISP_SOURCED"),
        "source-elisp-file must run the named .el; buffer = {buf:?}"
    );
    // The Vimscript file ran: its `:highlight Normal` reached the live theme.
    assert_eq!(
        app.editor.theme.get("ui.text").fg,
        Some(Color::Rgb(0xab, 0xcd, 0xef)),
        "source-viml-file must run the named .vim (Normal guifg -> ui.text)"
    );

    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn custom_source_files_off_by_default() -> anyhow::Result<()> {
    let dir = std::env::temp_dir().join(format!("zmax-custom-off-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join(".zmax"))?;
    std::env::set_var("HOME", &dir);

    // The same files exist on disk, but the settings are left at their defaults.
    std::fs::write(dir.join("my-init.el"), "(insert \"ELISP_SOURCED\")\n")?;
    std::fs::write(dir.join("my-init.vim"), "highlight Normal guifg=#abcdef\n")?;

    let config = test_config();
    assert!(config.editor.source_elisp_file.is_none());
    assert!(config.editor.source_viml_file.is_none());

    let mut app = make_app(None, None)?;
    let theme_before = app.editor.theme.get("ui.text").fg;
    app.load_init_scripts();

    let buf = app.editor.documents().next().unwrap().text().to_string();
    assert!(
        !buf.contains("ELISP_SOURCED"),
        "no elisp file must be sourced by default"
    );
    assert_eq!(
        app.editor.theme.get("ui.text").fg,
        theme_before,
        "no viml file must be sourced by default"
    );

    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

/// A sourced Vimscript file can drive the editor with its own `:` commands
/// (`:badd`/`:edit`/…): vimlrs would otherwise "handle" those against its
/// internal scratch buffer, so the ex-command host bridge must route them to
/// zmax. This is what makes `:mksession` / `:source Session.vim` restore work.
#[tokio::test(flavor = "multi_thread")]
async fn sourced_viml_editor_commands_reach_zmax() -> anyhow::Result<()> {
    let dir = std::env::temp_dir().join(format!("zmax-src-excmd-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join(".zmax"))?;
    std::env::set_var("HOME", &dir);

    // A real file for the session to add to the buffer list.
    let target = dir.join("added_by_session.txt");
    std::fs::write(&target, "hello from the session\n")?;

    // A "session" .vim that opens the file via the editor `:edit`.
    let vim = dir.join("session.vim");
    std::fs::write(&vim, format!("edit {}\n", target.to_string_lossy()))?;

    let mut app = make_app(None, Some(vim.to_string_lossy().into_owned()))?;
    app.load_init_scripts();

    // `:badd` reached zmax: the target file is now a known document.
    let has_target = app.editor.documents().any(|d| {
        d.path()
            .map(|p| p.ends_with("added_by_session.txt"))
            .unwrap_or(false)
    });
    assert!(
        has_target,
        "sourced `:badd` must add the buffer to zmax (ex-command bridge); open docs = {:?}",
        app.editor
            .documents()
            .filter_map(|d| d.path().map(|p| p.to_path_buf()))
            .collect::<Vec<_>>()
    );

    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}
