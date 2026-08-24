use super::*;

/// Spacemacs `SPC z x` is a *transient state*: the first `+` scales and the
/// state stays up, so bare `+`/`k` keep scaling until `q` (or any other key)
/// leaves it. Without the sticky node each key would have been a one-shot and
/// the second `+` would have hit Normal mode's `increment` instead.
#[tokio::test(flavor = "multi_thread")]
async fn font_scaling_transient_state_keeps_scaling() -> anyhow::Result<()> {
    let mut app = preset_app("spacemacs")
        .with_input_text("#[a|]#bc\n")
        .build()?;
    test_key_sequences(
        &mut app,
        vec![
            (
                // Enter with `+`, then stay: `+` again, then the `k` alias.
                Some("<space>zx++k"),
                Some(&|app: &zmax_term::application::Application| {
                    assert_eq!(app.editor.text_scale, 3, "three steps up in the state");
                }),
            ),
            (
                Some("0"),
                Some(&|app: &zmax_term::application::Application| {
                    assert_eq!(app.editor.text_scale, 0, "0 resets without leaving");
                }),
            ),
            (
                // `q` leaves the state; the buffer is untouched throughout.
                Some("q"),
                Some(&|app: &zmax_term::application::Application| {
                    let (_v, doc) = zmax_view::current_ref!(app.editor);
                    assert_eq!(doc.text().to_string(), "abc\n", "scaling never edits");
                }),
            ),
        ],
        false,
    )
    .await?;
    Ok(())
}

/// The same shape for `SPC z f` (`zoom-frm-in`/`out`/`unzoom`), which keeps its
/// own counter so `SPC z f 0` reports and resets the steps that family took.
#[tokio::test(flavor = "multi_thread")]
async fn frame_scaling_transient_state_keeps_zooming() -> anyhow::Result<()> {
    let mut app = preset_app("spacemacs")
        .with_input_text("#[a|]#bc\n")
        .build()?;
    test_key_sequences(
        &mut app,
        vec![
            (
                Some("<space>zf--j"),
                Some(&|app: &zmax_term::application::Application| {
                    assert_eq!(app.editor.frame_scale, -3, "three steps out in the state");
                }),
            ),
            (
                Some("0"),
                Some(&|app: &zmax_term::application::Application| {
                    assert_eq!(app.editor.frame_scale, 0, "0 resets without leaving");
                }),
            ),
        ],
        false,
    )
    .await?;
    Ok(())
}

/// Spacemacs binds the test runners under each language layer's `SPC m` map, so
/// `SPC m t b` is a prefix in a code buffer while org-mode keeps `SPC m t` for
/// `org-todo`. zmax gets the same split from the major-mode overlay: the chord
/// resolves in rust, and the base leaf still wins in org.
#[tokio::test(flavor = "multi_thread")]
async fn major_mode_test_chords_shadow_the_org_leaf() -> anyhow::Result<()> {
    use zmax_term::keymap::{major_mode, Keymaps};
    use zmax_view::document::Mode;

    let mut keymaps = Keymaps::default();
    let chord = |s: &str| -> Vec<zmax_view::input::KeyEvent> {
        s.split(' ')
            .map(|k| k.parse().expect("valid key"))
            .collect()
    };

    // The overlay exists for a code language and carries both chords.
    let rust = major_mode::overlay("rust", Mode::Normal).expect("rust overlay");
    for keys in ["space m t b", "space m t q"] {
        assert!(rust.search(&chord(keys)).is_some(), "rust binds {keys}");
    }
    // org does not: `SPC m t` there is the base map's org-todo leaf.
    let org = major_mode::overlay("org", Mode::Normal).expect("org overlay");
    assert!(
        org.search(&chord("space m t b")).is_none(),
        "org keeps SPC m t for org-todo"
    );

    // Driven through the resolver: in rust `space m t` is pending (the overlay
    // prefix shadows the leaf), in org it matches org_todo outright.
    for key in chord("space m") {
        keymaps.get_with_language(Mode::Normal, key, Some("rust"));
    }
    let pending = keymaps.get_with_language(Mode::Normal, chord("t")[0], Some("rust"));
    assert!(
        matches!(pending, zmax_term::keymap::KeymapResult::Pending(_)),
        "SPC m t opens a prefix in rust, got {pending:?}"
    );
    Ok(())
}

/// Emacs `C-x t d` is dired-*other-tab*: the listing opens in a new tab rather
/// than taking over the current one. The chord prompts for the directory first
/// (dired-read-dir-and-switches), so the tab appears when that is answered.
#[tokio::test(flavor = "multi_thread")]
async fn dired_other_tab_opens_a_new_tab() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let mut app = preset_app("spacemacs")
        .with_input_text("#[a|]#bc\n")
        .build()?;
    let before = app.editor.tab_count();
    let keys = format!("<C-x>td<C-u>{}<ret>", dir.path().display());
    test_key_sequence(
        &mut app,
        Some(&keys),
        Some(&|app: &zmax_term::application::Application| {
            assert_eq!(
                app.editor.tab_count(),
                before + 1,
                "the dired listing went to a new tab"
            );
        }),
        false,
    )
    .await?;
    Ok(())
}

/// Emacs `C-x 4 m` is compose-mail-*other-window*: the draft opens in a split,
/// leaving the buffer it was called from on screen.
#[tokio::test(flavor = "multi_thread")]
async fn compose_mail_other_window_splits_first() -> anyhow::Result<()> {
    let mut app = preset_app("spacemacs")
        .with_input_text("#[a|]#bc\n")
        .build()?;
    let before = app.editor.tree.views().count();
    test_key_sequence(
        &mut app,
        Some("<C-x>4m"),
        Some(&|app: &zmax_term::application::Application| {
            assert!(
                app.editor.tree.views().count() > before,
                "the draft opened in another window, not this one"
            );
            let doc = zmax_view::doc!(app.editor);
            assert!(
                doc.text().to_string().contains("To:"),
                "the split holds a mail draft: {:?}",
                doc.text().to_string()
            );
        }),
        false,
    )
    .await?;
    Ok(())
}

/// Emacs `C-x z` is `repeat`: it runs the *last command* again, whatever that
/// was. The chord used to run `repeat_last_motion`, which only ever repeated a
/// motion — an edit before it was not repeatable at all.
#[tokio::test(flavor = "multi_thread")]
async fn c_x_z_repeats_the_last_command() -> anyhow::Result<()> {
    let mut app = preset_app("spacemacs")
        .with_input_text("#[a|]#bcdef\n")
        .build()?;
    test_key_sequences(
        &mut app,
        vec![
            // `x` deletes the character under the cursor …
            (
                Some("x"),
                Some(&|app: &zmax_term::application::Application| {
                    let (_v, doc) = zmax_view::current_ref!(app.editor);
                    assert_eq!(doc.text().to_string(), "bcdef\n");
                }),
            ),
            // … and `C-x z` deletes another one, which a motion-only repeat
            // could not do.
            (
                Some("<C-x>z"),
                Some(&|app: &zmax_term::application::Application| {
                    let (_v, doc) = zmax_view::current_ref!(app.editor);
                    assert_eq!(doc.text().to_string(), "cdef\n", "the edit repeated");
                }),
            ),
            // Emacs then lets a bare `z` keep repeating (repeat-mode's transient
            // map), which zmax arms from the chord's own prefix.
            (
                Some("z"),
                Some(&|app: &zmax_term::application::Application| {
                    let (_v, doc) = zmax_view::current_ref!(app.editor);
                    assert_eq!(doc.text().to_string(), "def\n", "bare z repeats again");
                }),
            ),
        ],
        false,
    )
    .await?;
    Ok(())
}

/// Emacs `C-x r N` is rectangle-number-lines: the numbers go in at the
/// rectangle's left edge, not at the start of each whole line — that is
/// `number-lines`, which the chord used to run.
#[tokio::test(flavor = "multi_thread")]
async fn c_x_r_n_numbers_at_the_rectangle_edge() -> anyhow::Result<()> {
    let mut app = preset_app("spacemacs")
        .with_input_text("#[a|]#aaa\nbbbb\ncccc\n")
        .build()?;
    // A rectangle two columns in, spanning the three lines.
    test_key_sequence(
        &mut app,
        Some("ll<C-space>jj<C-x>rN"),
        Some(&|app: &zmax_term::application::Application| {
            let (_v, doc) = zmax_view::current_ref!(app.editor);
            let text = doc.text().to_string();
            for line in text.lines().take(3) {
                assert!(
                    line.starts_with("aa") || line.starts_with("bb") || line.starts_with("cc"),
                    "the line keeps its first two columns: {line:?}"
                );
            }
            assert!(
                text.contains("aa 1") || text.contains("aa1"),
                "the number went in at the rectangle's edge: {text:?}"
            );
        }),
        false,
    )
    .await?;
    Ok(())
}

/// Emacs rebinds `C-x C-x` inside `rectangle-mark-mode`, so the one chord is
/// `exchange-point-and-mark` over an ordinary region and
/// `rectangle-exchange-point-and-mark` — the opposite *corner* — over a
/// rectangular one. It used to be the plain swap in both.
#[tokio::test(flavor = "multi_thread")]
async fn c_x_c_x_walks_the_rectangle_corners() -> anyhow::Result<()> {
    let mut app = preset_app("spacemacs")
        .with_input_text("#[a|]#bcd\nefgh\nijkl\n")
        .build()?;
    test_key_sequences(
        &mut app,
        vec![
            // A rectangle from (0,0) to (2,2): the cursor is at the bottom-right.
            (
                Some("<C-x><space>jjll"),
                Some(&|app: &zmax_term::application::Application| {
                    assert!(app.editor.block.is_some(), "rectangle-mark-mode is on");
                    let (view, doc) = zmax_view::current_ref!(app.editor);
                    let text = doc.text().slice(..);
                    let cursor = doc.selection(view.id).primary().cursor(text);
                    assert_eq!(text.char_to_line(cursor), 2, "cursor on the last row");
                }),
            ),
            // `C-x C-x` moves it to the opposite corner — the other row.
            (
                Some("<C-x><C-x>"),
                Some(&|app: &zmax_term::application::Application| {
                    let (view, doc) = zmax_view::current_ref!(app.editor);
                    let text = doc.text().slice(..);
                    let cursor = doc.selection(view.id).primary().cursor(text);
                    assert_eq!(
                        text.char_to_line(cursor),
                        0,
                        "the opposite corner is on the first row"
                    );
                }),
            ),
        ],
        false,
    )
    .await?;
    Ok(())
}

/// Emacs `C-x v !` is `vc-edit-next-command`: it arms the *next* VC command to
/// open in the minibuffer for editing before it runs. The chord ran
/// `git_status`, which is a different command entirely.
#[tokio::test(flavor = "multi_thread")]
async fn c_x_v_bang_arms_the_next_vc_command() -> anyhow::Result<()> {
    let mut app = preset_app("spacemacs")
        .with_input_text("#[a|]#bc\n")
        .build()?;
    test_key_sequences(
        &mut app,
        vec![
            (
                Some("<C-x>v!"),
                Some(&|app: &zmax_term::application::Application| {
                    let status = app.editor.get_status().map(|(msg, _)| msg.to_string());
                    assert_eq!(
                        status.as_deref(),
                        Some("Edit the next VC command before it runs"),
                        "the chord arms the edit rather than opening the status view"
                    );
                }),
            ),
            // The next VC command (`SPC g P`, push) hands its line to the prompt
            // instead of shelling out — the prompt is what has the keyboard now,
            // so the editor is not reporting a push in flight.
            (
                Some("<space>gP"),
                Some(&|app: &zmax_term::application::Application| {
                    let status = app.editor.get_status().map(|(msg, _)| msg.to_string());
                    assert_ne!(
                        status.as_deref(),
                        Some("git: pushing…"),
                        "the push was handed to the prompt, not run"
                    );
                }),
            ),
        ],
        false,
    )
    .await?;
    Ok(())
}
