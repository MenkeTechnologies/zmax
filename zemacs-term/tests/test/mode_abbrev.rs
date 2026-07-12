use super::*;

use zemacs_term::application::Application;

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
                    zemacs_term::emacs_abbrev::get_effective(Some("fundamental"), "mabtest")
                        .as_deref(),
                    Some("mode-expansion-ok"),
                    "define-mode-abbrev did not populate the fundamental mode table"
                );
                // Scoped to that mode: another major mode's table doesn't hold it.
                assert!(
                    zemacs_term::emacs_abbrev::get_mode("rust", "mabtest").is_none(),
                    "mode abbrev leaked into another mode's table"
                );
            }),
        )],
        false,
    )
    .await?;
    Ok(())
}
