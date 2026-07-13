use super::*;

use zemacs_term::application::Application;

/// End-to-end emacs `abbrev-mode`: with the minor mode on, typing a word
/// separator (space) after an abbrev auto-expands it — emacs's
/// `self-insert-command` runs `expand-abbrev` for non-word input. Drives the real
/// insert path, so a broken hook or a wrong expansion fails here. A unique abbrev
/// keeps the process-global mode table from colliding with other tests.
#[tokio::test(flavor = "multi_thread")]
async fn abbrev_mode_auto_expands_on_word_separator() -> anyhow::Result<()> {
    test_key_sequences(
        &mut AppBuilder::new().build()?,
        vec![
            (
                Some(":abbrev-mode on<ret>:define-mode-abbrev abzz abbrev-expanded-ok<ret>"),
                Some(&|app: &Application| {
                    assert!(
                        !app.editor.is_err(),
                        "setup errored: {:?}",
                        app.editor.get_status()
                    );
                    assert!(
                        app.editor.abbrev_mode,
                        ":abbrev-mode on did not enable the mode"
                    );
                }),
            ),
            (
                // Type the abbrev then a space; the space triggers expansion.
                Some("iabzz <esc>"),
                Some(&|app: &Application| {
                    let text = app.editor.documents().next().unwrap().text().to_string();
                    assert!(
                        text.contains("abbrev-expanded-ok"),
                        "abbrev did not auto-expand on space, buffer: {text:?}"
                    );
                    assert!(
                        !text.contains("abzz"),
                        "abbrev text should have been replaced, buffer: {text:?}"
                    );
                }),
            ),
        ],
        false,
    )
    .await?;
    Ok(())
}

/// With abbrev-mode off (the default), typing the same abbrev + space must NOT
/// expand — proving the hook is gated on the mode flag and off by default.
#[tokio::test(flavor = "multi_thread")]
async fn abbrev_mode_off_does_not_expand() -> anyhow::Result<()> {
    test_key_sequences(
        &mut AppBuilder::new().build()?,
        vec![(
            Some(":define-mode-abbrev abzz2 should-not-appear<ret>iabzz2 <esc>"),
            Some(&|app: &Application| {
                let text = app.editor.documents().next().unwrap().text().to_string();
                assert!(
                    text.contains("abzz2"),
                    "literal abbrev should remain with abbrev-mode off, buffer: {text:?}"
                );
                assert!(
                    !text.contains("should-not-appear"),
                    "abbrev must not expand with abbrev-mode off, buffer: {text:?}"
                );
            }),
        )],
        false,
    )
    .await?;
    Ok(())
}
