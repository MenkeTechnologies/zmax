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

/// `C-x C-e` (`eval-last-sexp`) follows `eval-expression-get-print-arguments`
/// (simple.el:2087): a bare `-` (or `-1`) lifts
/// `eval-expression-print-maximum-character`, so the character form is printed
/// for any codepoint instead of stopping at 127. Under the spacemacs keymap the
/// argument is typed as `M--`, which is emacs's own `negative-argument` and is
/// what real spacemacs leaves reachable (evil binds neither `M--` nor `M-<digit>`).
#[tokio::test(flavor = "multi_thread")]
async fn c_x_c_e_negative_argument_lifts_the_character_limit() -> anyhow::Result<()> {
    let mut app = preset_app("spacemacs")
        .with_input_text("(+ 200 33)#[\n|]#")
        .build()?;
    test_key_sequences(
        &mut app,
        vec![
            (
                Some("<C-x><C-e>"),
                Some(&|app: &zmax_term::application::Application| {
                    let status = app
                        .editor
                        .get_status()
                        .map(|(msg, _)| msg.to_string())
                        .unwrap_or_default();
                    assert_eq!(
                        status, "233 (#o351, #xe9)",
                        "past eval-expression-print-maximum-character, no character form"
                    );
                }),
            ),
            (
                Some("<A-minus><C-x><C-e>"),
                Some(&|app: &zmax_term::application::Application| {
                    let status = app
                        .editor
                        .get_status()
                        .map(|(msg, _)| msg.to_string())
                        .unwrap_or_default();
                    assert_eq!(
                        status, "233 (#o351, #xe9, ?é)",
                        "M-- lifts the limit, so the character form is printed"
                    );
                }),
            ),
        ],
        false,
    )
    .await?;
    Ok(())
}

/// `M-- M-u` still upcases the *previous* word — but as emacs does it, with the
/// negative argument reaching `upcase-word` (`(interactive "p")`) rather than
/// through a keymap node standing in for the argument.
#[tokio::test(flavor = "multi_thread")]
async fn negative_argument_makes_the_word_case_commands_work_backwards() -> anyhow::Result<()> {
    let mut app = preset_app("spacemacs")
        .with_input_text("alpha beta#[ |]#gamma\n")
        .build()?;
    test_key_sequences(
        &mut app,
        vec![
            (
                Some("<A-minus><A-u>"),
                Some(&|app: &zmax_term::application::Application| {
                    let (_v, doc) = zmax_view::current_ref!(app.editor);
                    assert_eq!(
                        doc.text().to_string(),
                        "alpha BETA gamma\n",
                        "M-- M-u upcases the word before point"
                    );
                    let (view, doc) = zmax_view::current_ref!(app.editor);
                    assert_eq!(
                        doc.selection(view.id).primary().cursor(doc.text().slice(..)),
                        10,
                        "point stays where it was, as emacs leaves it"
                    );
                }),
            ),
            (
                // Without the argument, M-u takes the word *after* point.
                Some("<A-u>"),
                Some(&|app: &zmax_term::application::Application| {
                    let (_v, doc) = zmax_view::current_ref!(app.editor);
                    assert_eq!(doc.text().to_string(), "alpha BETA GAMMA\n");
                }),
            ),
        ],
        false,
    )
    .await?;
    Ok(())
}

/// `C-x C-e` truncates long output to `eval-expression-print-length` (12) and
/// `eval-expression-print-level` (4) — "With a prefix argument of zero, however,
/// there is no such truncation" (eval-last-sexp's docstring). Emacs prints
/// `(make-list 20 1)` as `(1 1 1 1 1 1 1 1 1 1 1 1 ...)` under those limits.
#[tokio::test(flavor = "multi_thread")]
async fn c_x_c_e_truncates_unless_the_argument_is_zero() -> anyhow::Result<()> {
    let mut app = preset_app("spacemacs")
        .with_input_text("(make-list 20 1)#[\n|]#")
        .build()?;
    test_key_sequence(
        &mut app,
        Some("<C-x><C-e>"),
        Some(&|app: &zmax_term::application::Application| {
            assert_eq!(
                app.editor.get_status().map(|(msg, _)| msg.to_string()),
                Some("(1 1 1 1 1 1 1 1 1 1 1 1 ...)".to_string()),
                "twelve elements, then the ellipsis emacs prints"
            );
        }),
        false,
    )
    .await?;
    // `M-0` is the zero argument: no truncation. It also inserts the value, as
    // any argument other than nil or a bare `-` does.
    test_key_sequence(&mut app, Some("<A-0><C-x><C-e>"), None, false).await?;
    let text = zmax_view::doc!(app.editor).text().to_string();
    assert!(
        text.contains(&format!("({})", vec!["1"; 20].join(" "))),
        "a zero argument prints the whole list: {text:?}"
    );
    Ok(())
}

/// "This command handles `defvar', `defcustom' and `defface' the same way that
/// `eval-defun' does" — eval-last-sexp's docstring. Both refuse to do anything
/// on re-evaluation otherwise: a `defcustom` skips a variable that already has a
/// value, and a `defface` keeps the spec it first recorded. Emacs rewrites the
/// form (`elisp--eval-defun-1`) so a re-evaluated definition takes effect.
#[tokio::test(flavor = "multi_thread")]
async fn c_x_c_e_reevaluates_defcustom_and_defface() -> anyhow::Result<()> {
    let mut app = preset_app("spacemacs")
        .with_input_text("(defcustom cxe-opt 1 \"doc\")#[\n|]#")
        .build()?;
    test_key_sequence(&mut app, Some("<C-x><C-e>"), None, false).await?;
    test_key_sequence(&mut app, Some(":elisp (setq cxe-opt 99)<ret>"), None, false).await?;
    // Re-evaluating the declaration with a new value re-sets the option.
    test_key_sequence(&mut app, Some(":%d<ret>"), None, false).await?;
    test_key_sequence(
        &mut app,
        Some("i(defcustom cxe-opt 2 \"doc\")<ret><esc>"),
        None,
        false,
    )
    .await?;
    test_key_sequence(&mut app, Some("<C-x><C-e>"), None, false).await?;
    test_key_sequence(
        &mut app,
        Some(":elisp cxe-opt<ret>"),
        Some(&|app: &zmax_term::application::Application| {
            assert_eq!(
                app.editor.get_status().map(|(msg, _)| msg.to_string()),
                Some("=> 2".to_string()),
                "the re-evaluated defcustom re-set the option"
            );
        }),
        false,
    )
    .await?;

    // The same for a face's spec.
    test_key_sequence(&mut app, Some(":%d<ret>"), None, false).await?;
    test_key_sequence(
        &mut app,
        Some("i(defface cxe-face (quote ((t :weight bold))) \"doc\")<ret><esc>"),
        None,
        false,
    )
    .await?;
    test_key_sequence(&mut app, Some("<C-x><C-e>"), None, false).await?;
    test_key_sequence(&mut app, Some(":%d<ret>"), None, false).await?;
    test_key_sequence(
        &mut app,
        Some("i(defface cxe-face (quote ((t :weight normal))) \"doc\")<ret><esc>"),
        None,
        false,
    )
    .await?;
    test_key_sequence(&mut app, Some("<C-x><C-e>"), None, false).await?;
    test_key_sequence(
        &mut app,
        Some(":elisp (get (quote cxe-face) (quote face-defface-spec))<ret>"),
        Some(&|app: &zmax_term::application::Application| {
            assert_eq!(
                app.editor.get_status().map(|(msg, _)| msg.to_string()),
                Some("=> ((t :weight normal))".to_string()),
                "the re-evaluated defface replaced the spec"
            );
        }),
        false,
    )
    .await?;
    Ok(())
}

/// `C-h 4 i` (`info-other-window`) opens an Info-*mode* buffer, not a plain
/// scratch dump of the node: info.el's reading keys are live in it — `n`/`p`/`u`
/// walk the node's Next/Prev/Up pointers, `l` the history, `RET` follows the
/// menu item at point. Skipped where the `info` program or its directory node is
/// not installed, which is what the reader shells out to.
#[tokio::test(flavor = "multi_thread")]
async fn c_h_4_i_opens_an_info_mode_buffer() -> anyhow::Result<()> {
    let has_info = std::process::Command::new("info")
        .args(["-o", "-", "(dir)Top"])
        .output()
        .map(|out| out.status.success() && !out.stdout.is_empty())
        .unwrap_or(false);
    if !has_info {
        eprintln!("skipping: no `info` directory node on this machine");
        return Ok(());
    }
    let mut app = preset_app("spacemacs")
        .with_input_text("#[a|]#bc\n")
        .build()?;
    test_key_sequence(
        &mut app,
        Some("<C-h>4i"),
        Some(&|app: &zmax_term::application::Application| {
            let (_v, doc) = zmax_view::current_ref!(app.editor);
            assert_eq!(
                doc.major_mode(),
                Some("info"),
                "the node lands in an Info-mode buffer"
            );
            assert!(
                doc.text().to_string().starts_with("File:"),
                "showing an info node: {:?}",
                doc.text().to_string().chars().take(40).collect::<String>()
            );
        }),
        false,
    )
    .await?;
    // `n` on the directory node has no Next pointer, and Info says so rather than
    // doing nothing — the buffer is navigable, which a scratch dump was not.
    test_key_sequence(
        &mut app,
        Some("n"),
        Some(&|app: &zmax_term::application::Application| {
            let status = app
                .editor
                .get_status()
                .map(|(msg, _)| msg.to_string())
                .unwrap_or_default();
            assert!(
                status.starts_with("info ") || status.contains("\"Next\" pointer"),
                "n either moved or reported that there is nowhere to move: {status:?}"
            );
        }),
        false,
    )
    .await?;
    Ok(())
}
