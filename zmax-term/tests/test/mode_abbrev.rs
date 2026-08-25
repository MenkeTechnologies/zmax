use super::*;

use zmax_term::application::Application;

/// End-to-end emacs `define-mode-abbrev`: the command must store the abbrev in
/// the current buffer's major-mode table — the exact table `expand-abbrev` reads
/// (mode-local before global). The scratch buffer has no language, so it resolves
/// to the `fundamental` mode, and both the command and `expand-abbrev` agree on
/// that name. A unique abbrev keeps the process-global table from colliding with
/// other tests.
#[tokio::test(flavor = "multi_thread")]
async fn define_mode_abbrev_populates_the_mode_table() -> anyhow::Result<()> {
    test_key_sequences(
        &mut AppBuilder::new().build()?,
        vec![(
            Some(":define-mode-abbrev mabtest mode-expansion-ok<ret>"),
            Some(&|app: &Application| {
                assert!(
                    !app.editor.is_err(),
                    "define-mode-abbrev errored: {:?}",
                    app.editor.get_status()
                );
                // Resolvable via the exact lookup expand-abbrev uses for a
                // fundamental-mode buffer (mode table before global).
                assert_eq!(
                    zmax_term::emacs_abbrev::get_effective(Some("fundamental"), "mabtest")
                        .as_deref(),
                    Some("mode-expansion-ok"),
                    "define-mode-abbrev did not populate the fundamental mode table"
                );
                // Scoped to that mode: another major mode's table doesn't hold it.
                assert!(
                    zmax_term::emacs_abbrev::get_mode("rust", "mabtest").is_none(),
                    "mode abbrev leaked into another mode's table"
                );
            }),
        )],
        false,
    )
    .await?;
    Ok(())
}

/// Emacs `only-global-abbrevs` (abbrev.el's defcustom, nil by default): when it
/// is non-nil the mode-abbrev commands define into the *global* table instead of
/// the buffer's mode-local one. The routing was written but never observed; this
/// drives `C-x a l` with the variable set and reads the store back.
#[tokio::test(flavor = "multi_thread")]
async fn only_global_abbrevs_reroutes_add_mode_abbrev_to_the_global_table() -> anyhow::Result<()> {
    let mut app = preset_app("spacemacs")
        .with_input_text("ogaexpansion#[ |]#\n")
        .build()?;
    test_key_sequence(
        &mut app,
        Some(":elisp (setq only-global-abbrevs t)<ret>"),
        None,
        false,
    )
    .await?;
    // `C-x a l` takes the word before point as the expansion and prompts for the
    // name; with only-global-abbrevs set it defines globally.
    test_key_sequence(&mut app, Some("<C-x>al"), None, false).await?;
    test_key_sequence(&mut app, Some("oganame<ret>"), None, false).await?;

    let store = std::fs::read_to_string(
        std::env::var("ZMAX_ABBREV_FILE").expect("the harness points the store at a temp file"),
    )
    .unwrap_or_default();
    assert!(
        store
            .lines()
            .any(|line| line == "oganame\togaexpansion"),
        "the abbrev went into the global table: {store:?}"
    );
    assert!(
        !store.lines().any(|line| line.ends_with("\toganame\togaexpansion")),
        "and not into a mode table: {store:?}"
    );
    // Leave the variable as it was for the rest of the process.
    test_key_sequence(
        &mut app,
        Some(":elisp (setq only-global-abbrevs nil)<ret>"),
        None,
        false,
    )
    .await?;
    Ok(())
}

/// Mode-local abbrev tables used to live only in memory, so a mode abbrev was
/// lost on restart while a global one persisted. Emacs writes every table to
/// `abbrev-file-name` and reads them all back, and the store keeps them the same
/// way now: `mode\tname\texpansion` rows beside the global `name\texpansion`
/// ones, in the one file.
#[tokio::test(flavor = "multi_thread")]
async fn mode_abbrevs_are_written_to_the_store() -> anyhow::Result<()> {
    let mut app = preset_app("spacemacs").build()?;
    test_key_sequence(
        &mut app,
        Some(":define-mode-abbrev persistzz persisted-expansion<ret>"),
        None,
        false,
    )
    .await?;
    let store = std::fs::read_to_string(
        std::env::var("ZMAX_ABBREV_FILE").expect("the harness points the store at a temp file"),
    )
    .unwrap_or_default();
    assert!(
        store
            .lines()
            .any(|line| line.ends_with("\tpersistzz\tpersisted-expansion")
                && line.matches('\t').count() == 2),
        "the mode table is on disk with its mode: {store:?}"
    );
    Ok(())
}
