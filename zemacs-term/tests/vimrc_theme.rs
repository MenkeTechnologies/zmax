//! End-to-end: a real `~/.vimrc` running `:colorscheme` (and trailing
//! `:highlight` overrides) repaints the live zemacs editor at startup.
//!
//! This drives the actual public startup entry point `Application::load_init_scripts`
//! — it discovers `~/.vimrc` on disk, sources it through the embedded vimlrs
//! interpreter (`colorscheme acme` → `colors/acme.vim` → `:highlight` commands),
//! and applies the synthesised theme to the live `Editor`. Runs in its own test
//! binary (own process) so pointing `HOME` at a temp tree cannot affect any other
//! test. Requires the `integration` feature (headless `TestBackend`) and, via the
//! default `scripting` feature, the real (non-stub) `load_init_scripts`.
#![cfg(all(feature = "integration", feature = "scripting", unix))]

// The shared test harness (only a small part is used here, so silence the
// dead-code/unused warnings its unused surface would otherwise raise).
#[allow(dead_code, unused_imports, clippy::all)]
mod helpers {
    include!("test/helpers.rs");
}

use helpers::{test_config, test_syntax_loader};
use zemacs_loader::workspace_trust::WorkspaceTrust;
use zemacs_term::{application::Application, args::Args};
use zemacs_view::theme::Color;

#[tokio::test(flavor = "multi_thread")]
async fn vimrc_colorscheme_repaints_live_editor() -> anyhow::Result<()> {
    // An isolated HOME with a real vim config tree, exercising the real-world
    // path: the colour scheme lives in a *pathogen bundle*
    // (`~/.vim/bundle/*/colors/acme.vim`), the `.vimrc` selects it with a
    // `silent!` command modifier, and — crucially — an earlier line is an
    // unparseable construct. A real vimrc aborts on that line unless sourcing is
    // error-tolerant; the options/colours after it must still take effect.
    let home = std::env::temp_dir().join(format!("zemacs-vimrc-e2e-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(home.join(".vim/bundle/Colors/colors"))?;
    std::fs::create_dir_all(home.join(".zemacs"))?;
    std::fs::write(
        home.join(".vim/bundle/Colors/colors/acme.vim"),
        "highlight Normal guifg=#c0ffee guibg=#0a0a0a\n\
         hi Comment guifg=#00ff00 gui=italic\n",
    )?;
    std::fs::write(
        home.join(".vimrc"),
        "set number\n\
         let broken = \"an unterminated string that fails to parse\n\
         silent! colorscheme acme\n\
         highlight Comment guifg=#00cc00\n",
    )?;

    // etcetera / std::env::home_dir read $HOME on unix, so this redirects both
    // zemacs's config dir and the `.vimrc` lookup into the temp tree. Isolated
    // to this process (own test binary).
    std::env::set_var("HOME", &home);

    // The user's personal `~/.vimrc` is sourced only when `source-vimrc` is
    // enabled (off by default); this feature test opts in.
    let mut config = test_config();
    config.editor.source_vimrc = true;

    let mut app = Application::new(
        Args::default(),
        config,
        test_syntax_loader(None),
        WorkspaceTrust::fully_trusted(),
    )?;

    // Baseline: the built-in theme, not the Vim scheme.
    assert_ne!(app.editor.theme.name(), "vim:acme");

    // The real startup path.
    app.load_init_scripts();

    // The live editor now wears the Vim colour scheme.
    assert_eq!(app.editor.theme.name(), "vim:acme");
    assert_eq!(
        app.editor.theme.get("ui.text").fg,
        Some(Color::Rgb(0xc0, 0xff, 0xee)),
        "Normal guifg → ui.text"
    );
    assert_eq!(
        app.editor.theme.get("ui.background").bg,
        Some(Color::Rgb(0x0a, 0x0a, 0x0a)),
        "Normal guibg → ui.background"
    );
    // The trailing `:highlight Comment` (after `:colorscheme`) wins over the
    // scheme file's Comment colour — proving the post-source override flush.
    assert_eq!(
        app.editor.theme.get("comment").fg,
        Some(Color::Rgb(0x00, 0xcc, 0x00)),
        "trailing vimrc :highlight override applies"
    );

    let _ = std::fs::remove_dir_all(&home);
    Ok(())
}
