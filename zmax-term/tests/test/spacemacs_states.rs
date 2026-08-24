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
