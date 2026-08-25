use super::*;

use zmax_term::config::Config;

// The emacs preset is modeless: it starts in Insert mode (see keymap::default_mode)
// and the emacs C-/M- chords live there. Setting `keymap: "emacs"` makes
// Application::new pick Insert as the initial mode; `keys` supplies the bindings.
fn emacs() -> AppBuilder {
    AppBuilder::new().with_config(Config {
        keys: zmax_term::keymap::emacs::default(),
        keymap: "emacs".to_string(),
        ..Default::default()
    })
}

fn buffer(app: &zmax_term::application::Application) -> String {
    let (_, doc) = zmax_view::current_ref!(app.editor);
    doc.text().to_string()
}

// M-u (upcase-word), M-l (downcase-word), M-c (capitalize-word) each act on the
// word after point — verifies the new emacs case-op chords route to their
// commands and that the preset starts in Insert mode.
#[tokio::test(flavor = "multi_thread")]
async fn emacs_upcase_word() -> anyhow::Result<()> {
    let mut app = emacs().with_input_text("#[f|]#oo bar").build()?;
    test_key_sequence(
        &mut app,
        Some("<A-u>"),
        Some(&|app| {
            assert_eq!(buffer(app), "FOO bar", "M-u upcases the word after point");
        }),
        false,
    )
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn emacs_capitalize_word() -> anyhow::Result<()> {
    let mut app = emacs().with_input_text("#[f|]#oo bar").build()?;
    test_key_sequence(
        &mut app,
        Some("<A-c>"),
        Some(&|app| {
            assert_eq!(
                buffer(app),
                "Foo bar",
                "M-c capitalizes the word after point"
            );
        }),
        false,
    )
    .await?;
    Ok(())
}

// M-m (back-to-indentation) moves point to the first non-whitespace character.
#[tokio::test(flavor = "multi_thread")]
async fn emacs_back_to_indentation() -> anyhow::Result<()> {
    let mut app = emacs().with_input_text("    foo#[b|]#ar").build()?;
    test_key_sequence(
        &mut app,
        Some("<A-m>"),
        Some(&|app| {
            let (view, doc) = zmax_view::current_ref!(app.editor);
            assert_eq!(
                doc.selection(view.id).primary().from(),
                4,
                "M-m -> first non-blank col"
            );
        }),
        false,
    )
    .await?;
    Ok(())
}

// C-t (transpose-chars) swaps the two characters around point (typable command).
#[tokio::test(flavor = "multi_thread")]
async fn emacs_transpose_chars() -> anyhow::Result<()> {
    let mut app = emacs().with_input_text("ab#[c|]#d").build()?;
    test_key_sequence(
        &mut app,
        Some("<C-t>"),
        Some(&|app| {
            // emacs transpose-chars drags the char before point over the one at point.
            assert_ne!(buffer(app), "abcd", "C-t transposed characters");
        }),
        false,
    )
    .await?;
    Ok(())
}

/// `C-x w d` (`toggle-window-dedicated`) takes its FLAG from the prefix
/// argument: `(if (consp flag) t …)`, so only a *raw* `C-u` dedicates strongly.
/// A numeric prefix is a non-nil non-t FLAG, which
/// `toggle-window-dedicated-flag`'s own docstring says gives "the same kind of
/// non-strong dedication" as its default. zmax had this inverted — any leading
/// count dedicated strongly and a bare `C-u` weakly.
#[tokio::test(flavor = "multi_thread")]
async fn c_x_w_d_dedicates_strongly_only_for_a_raw_prefix() -> anyhow::Result<()> {
    let mut app = preset_app("emacs")
        .with_input_text("#[a|]#bc\n")
        .build()?;
    test_key_sequences(
        &mut app,
        vec![
            (
                Some("<C-u>4<C-x>wd"),
                Some(&|app: &zmax_term::application::Application| {
                    let status = app
                        .editor
                        .get_status()
                        .map(|(msg, _)| msg.to_string())
                        .unwrap_or_default();
                    assert!(
                        status.starts_with("Window is now dedicated"),
                        "a numeric prefix dedicates weakly: {status:?}"
                    );
                }),
            ),
            // Toggle back off, then strongly with the raw prefix.
            (Some("<C-x>wd"), None),
            (
                Some("<C-u><C-x>wd"),
                Some(&|app: &zmax_term::application::Application| {
                    let status = app
                        .editor
                        .get_status()
                        .map(|(msg, _)| msg.to_string())
                        .unwrap_or_default();
                    assert!(
                        status.starts_with("Window is now strongly dedicated"),
                        "a raw C-u dedicates strongly: {status:?}"
                    );
                }),
            ),
        ],
        false,
    )
    .await?;
    Ok(())
}
