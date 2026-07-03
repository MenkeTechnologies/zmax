//! Regression test: zemacs must NOT source the user's personal `~/.vimrc` unless
//! the `source-vimrc` setting is explicitly enabled. The default is off — zemacs
//! is not Vim and silently inheriting a personal `.vimrc` (options, mappings,
//! colours) is a bug. This drives the real startup entry point
//! `Application::load_init_scripts` with a `.vimrc` present on disk but the
//! setting left at its default, and asserts the config is ignored.
//!
//! Own test binary (own process) so its `HOME` override cannot race the opt-in
//! `vimrc_theme` test. Requires the `integration` + `scripting` features.
#![cfg(all(feature = "integration", feature = "scripting", unix))]

#[allow(dead_code, unused_imports, clippy::all)]
mod helpers {
    include!("test/helpers.rs");
}

use helpers::{test_config, test_syntax_loader};
use zemacs_loader::workspace_trust::WorkspaceTrust;
use zemacs_term::{application::Application, args::Args};

#[tokio::test(flavor = "multi_thread")]
async fn vimrc_not_sourced_by_default() -> anyhow::Result<()> {
    let home = std::env::temp_dir().join(format!("zemacs-vimrc-off-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(home.join(".vim/bundle/Colors/colors"))?;
    std::fs::create_dir_all(home.join(".zemacs"))?;
    // The very same `.vimrc` that the opt-in test proves DOES repaint the editor.
    std::fs::write(
        home.join(".vim/bundle/Colors/colors/acme.vim"),
        "highlight Normal guifg=#c0ffee guibg=#0a0a0a\n",
    )?;
    std::fs::write(
        home.join(".vimrc"),
        "set number\n\
         silent! colorscheme acme\n",
    )?;

    std::env::set_var("HOME", &home);

    // Default config: `source_vimrc` is left false (the whole point of the fix).
    let config = test_config();
    assert!(
        !config.editor.source_vimrc,
        "source_vimrc must default to false"
    );

    let mut app = Application::new(
        Args::default(),
        config,
        test_syntax_loader(None),
        WorkspaceTrust::fully_trusted(),
    )?;

    let baseline = app.editor.theme.name().to_string();
    assert_ne!(baseline, "vim:acme");

    // Real startup path — with the setting off, the personal `.vimrc` is skipped.
    app.load_init_scripts();

    // The theme is unchanged: the `.vimrc`'s `:colorscheme acme` was NOT sourced.
    assert_eq!(
        app.editor.theme.name(),
        baseline,
        "personal ~/.vimrc must be ignored unless `source-vimrc` is enabled"
    );
    assert_ne!(app.editor.theme.name(), "vim:acme");

    let _ = std::fs::remove_dir_all(&home);
    Ok(())
}
