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

/// Spacemacs `SPC D f p` is `ediff-patch-file`: it *asks* for the patch and for
/// the file to patch, then opens original ⇔ patched in the ediff view. The chord
/// ran `diff-ediff-patch`, which takes the patch from the current buffer and
/// never asks — a different emacs command.
#[tokio::test(flavor = "multi_thread")]
async fn spc_d_f_p_asks_for_the_patch_and_the_file() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let target = dir.path().join("hello.txt");
    std::fs::write(&target, "one\ntwo\n")?;
    let patch = dir.path().join("change.patch");
    std::fs::write(
        &patch,
        format!(
            "--- a/{name}\n+++ b/{name}\n@@ -1,2 +1,2 @@\n one\n-two\n+TWO\n",
            name = "hello.txt"
        ),
    )?;

    let mut app = preset_app("spacemacs")
        .with_file(target.clone(), None)
        .build()?;
    let keys = format!(
        "<space>Dfp{}<ret>{}<ret>",
        patch.display(),
        target.display()
    );
    test_key_sequence(
        &mut app,
        Some(&keys),
        Some(&|app: &zmax_term::application::Application| {
            assert!(
                !app.editor.is_err(),
                "the patch applied: {:?}",
                app.editor.get_status()
            );
            // The ediff view holds the patched text; the file on disk is
            // untouched until the review is applied.
            assert_eq!(
                std::fs::read_to_string(&target).unwrap(),
                "one\ntwo\n",
                "the file is only rewritten when the review is applied"
            );
        }),
        false,
    )
    .await?;
    Ok(())
}

/// Spacemacs `C-TAB` cycles *through* the buffers this window has visited; the
/// chord used to run `goto_last_accessed_file`, which only ever toggled between
/// the last two. Three buffers make the difference visible: two presses reach
/// the oldest, and the reverse chord walks back.
#[tokio::test(flavor = "multi_thread")]
async fn c_tab_cycles_the_visited_buffer_ring() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let (a, b, c) = (
        dir.path().join("a.txt"),
        dir.path().join("b.txt"),
        dir.path().join("c.txt"),
    );
    for (p, text) in [(&a, "aaa\n"), (&b, "bbb\n"), (&c, "ccc\n")] {
        std::fs::write(p, text)?;
    }
    let mut app = preset_app("spacemacs").with_file(a.clone(), None).build()?;
    let name = |app: &zmax_term::application::Application| {
        let (_v, doc) = zmax_view::current_ref!(app.editor);
        doc.path()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    let open_b = format!(":open {}<ret>", b.display());
    let open_c = format!(":open {}<ret>", c.display());
    test_key_sequences(
        &mut app,
        vec![
            // Visit a, b, c — c is on screen, the ring holds b then a.
            (Some(&open_b), None),
            (Some(&open_c), None),
            (
                Some("<C-tab>"),
                Some(&|app: &zmax_term::application::Application| {
                    assert_eq!(name(app), "b.txt", "one press: the previous buffer");
                }),
            ),
            (
                Some("<C-tab>"),
                Some(&|app: &zmax_term::application::Application| {
                    assert_eq!(name(app), "a.txt", "two presses: the one before that");
                }),
            ),
            (
                Some("<A-C-tab>"),
                Some(&|app: &zmax_term::application::Application| {
                    assert_eq!(name(app), "b.txt", "the reverse chord walks back");
                }),
            ),
        ],
        false,
    )
    .await?;
    Ok(())
}

/// Spacemacs's `SPC t k` family pins a which-key popup that *stays* — the
/// listing survives the next keystrokes and `SPC t k k` takes it down. The
/// chords used to run `describe_keymap`/`describe_bindings`, which dump the map
/// into a scratch buffer once.
#[tokio::test(flavor = "multi_thread")]
async fn spc_t_k_pins_a_persistent_which_key_popup() -> anyhow::Result<()> {
    let mut app = preset_app("spacemacs")
        .with_input_text("#[a|]#bc\n")
        .build()?;
    test_key_sequences(
        &mut app,
        vec![
            (
                Some("<space>tkt"),
                Some(&|app: &zmax_term::application::Application| {
                    let pinned = app.editor.persistent_autoinfo.as_ref();
                    assert_eq!(
                        pinned.map(|i| i.title.to_string()).as_deref(),
                        Some("Top-level keymap"),
                        "the top-level map is pinned"
                    );
                    assert!(app.editor.autoinfo.is_some(), "and it is showing");
                }),
            ),
            // An ordinary keystroke does not take it down.
            (
                Some("l"),
                Some(&|app: &zmax_term::application::Application| {
                    assert!(
                        app.editor.persistent_autoinfo.is_some(),
                        "the pin survives a keystroke"
                    );
                    assert!(app.editor.autoinfo.is_some(), "and stays on screen");
                }),
            ),
            // `SPC t k k` does.
            (
                Some("<space>tkk"),
                Some(&|app: &zmax_term::application::Application| {
                    assert!(app.editor.persistent_autoinfo.is_none(), "unpinned");
                    assert!(app.editor.autoinfo.is_none(), "and gone from the screen");
                }),
            ),
        ],
        false,
    )
    .await?;
    Ok(())
}

/// Spacemacs `SPC a k` launches paradox — the package listing. The chord was
/// zmax's AI shell-command generator, which has moved to `SPC a K`.
#[tokio::test(flavor = "multi_thread")]
async fn spc_a_k_opens_the_package_listing() -> anyhow::Result<()> {
    use zmax_term::keymap::{KeymapResult, Keymaps};
    use zmax_view::document::Mode;

    let mut keymaps = Keymaps::default();
    let chord = |s: &str| -> Vec<zmax_view::input::KeyEvent> {
        s.split(' ')
            .map(|k| k.parse().expect("valid key"))
            .collect()
    };
    let resolve = |keymaps: &mut Keymaps, keys: &str| -> Option<String> {
        let mut last = None;
        for key in chord(keys) {
            last = match keymaps.get(Mode::Normal, key) {
                KeymapResult::Matched(cmd) => Some(cmd.name().to_string()),
                _ => None,
            };
        }
        last
    };
    assert_eq!(
        resolve(&mut keymaps, "space a k").as_deref(),
        Some("list_packages"),
        "SPC a k is the package listing"
    );
    assert_eq!(
        resolve(&mut keymaps, "space a K").as_deref(),
        Some("ai_terminal_command"),
        "the shell-command generator kept a chord of its own"
    );
    Ok(())
}

/// Spacemacs `SPC D d r` (`ediff-directory-revisions`) opens a session *group*:
/// one working-copy-vs-revision entry per changed file, which you pick from.
/// The chord was unbound and the command wrote a static scratch listing.
#[tokio::test(flavor = "multi_thread")]
async fn spc_d_d_r_lists_the_changed_files_as_a_group() -> anyhow::Result<()> {
    use zmax_term::keymap::{KeymapResult, Keymaps};
    use zmax_view::document::Mode;

    let mut keymaps = Keymaps::default();
    let mut last = None;
    for key in "space D d r".split(' ') {
        last = match keymaps.get(Mode::Normal, key.parse().expect("valid key")) {
            KeymapResult::Matched(cmd) => Some(cmd.name().to_string()),
            _ => None,
        };
    }
    assert_eq!(
        last.as_deref(),
        Some("ediff_directory_revisions"),
        "the chord resolves in the shipped preset"
    );
    Ok(())
}

/// Spacemacs `SPC D m d 3` (`ediff-merge-directories-with-ancestor`) opens a
/// merge session group over the files two directories share, three-way against
/// a third directory's ancestors. The chord was unbound and the command wrote a
/// listing telling you to re-run a *different* chord per file.
#[tokio::test(flavor = "multi_thread")]
async fn spc_d_m_d_3_resolves_and_merges_common_files() -> anyhow::Result<()> {
    use zmax_term::keymap::{KeymapResult, Keymaps};
    use zmax_view::document::Mode;

    let mut keymaps = Keymaps::default();
    let mut last = None;
    for key in "space D m d 3".split(' ') {
        last = match keymaps.get(Mode::Normal, key.parse().expect("valid key")) {
            KeymapResult::Matched(cmd) => Some(cmd.name().to_string()),
            _ => None,
        };
    }
    assert_eq!(
        last.as_deref(),
        Some("ediff_merge_directories_with_ancestor"),
        "the chord resolves in the shipped preset"
    );
    Ok(())
}

/// The command-log layer's `command-log-mode`: a live side buffer that fills as
/// commands run, rather than `view-lossage`'s after-the-fact dump.
#[tokio::test(flavor = "multi_thread")]
async fn command_log_mode_logs_commands_as_they_run() -> anyhow::Result<()> {
    let mut app = preset_app("spacemacs")
        .with_input_text("#[a|]#bcdef\n")
        .build()?;
    let log_text = |app: &zmax_term::application::Application| -> String {
        app.editor
            .documents()
            .find(|doc| doc.display_name() == "*command-log*")
            .map(|doc| doc.text().to_string())
            .unwrap_or_default()
    };
    test_key_sequences(
        &mut app,
        vec![
            (Some("<space>t<C-l>"), None),
            // Two ordinary commands land in the log with the keys that ran them.
            (
                Some("ll"),
                Some(&|app: &zmax_term::application::Application| {
                    let text = log_text(app);
                    assert!(
                        text.matches("move_char_right").count() >= 2,
                        "both presses were logged: {text:?}"
                    );
                    assert!(text.contains('l'), "the keys are logged too: {text:?}");
                }),
            ),
            // Toggling off stops the log growing.
            (
                Some("<space>t<C-l>"),
                Some(&|app: &zmax_term::application::Application| {
                    assert!(!app.editor.is_err(), "{:?}", app.editor.get_status());
                }),
            ),
        ],
        false,
    )
    .await?;
    Ok(())
}

/// The tmux layer's `tmux-navigate`: the control variants of the window map move
/// one split, and at the edge — where vim does nothing, there being no window
/// that way — hand the move to tmux. Both window maps carry them, since `SPC w`
/// mirrors `C-w`.
#[tokio::test(flavor = "multi_thread")]
async fn tmux_navigate_is_bound_on_both_window_maps() -> anyhow::Result<()> {
    use zmax_term::keymap::{KeymapResult, Keymaps};
    use zmax_view::document::Mode;

    let resolve = |keys: &str| -> Option<String> {
        let mut keymaps = Keymaps::default();
        let mut last = None;
        for key in keys.split(' ') {
            last = match keymaps.get(Mode::Normal, key.parse().expect("valid key")) {
                KeymapResult::Matched(cmd) => Some(cmd.name().to_string()),
                _ => None,
            };
        }
        last
    };
    for (keys, want) in [
        ("C-w C-h", "tmux_navigate_left"),
        ("C-w C-j", "tmux_navigate_down"),
        ("C-w C-k", "tmux_navigate_up"),
        ("C-w C-l", "tmux_navigate_right"),
        ("space w C-h", "tmux_navigate_left"),
        ("space w C-l", "tmux_navigate_right"),
    ] {
        assert_eq!(resolve(keys).as_deref(), Some(want), "{keys}");
    }
    // The bare keys stay pure vim window jumps.
    for (keys, want) in [
        ("C-w h", "jump_view_left"),
        ("space w l", "jump_view_right"),
    ] {
        assert_eq!(resolve(keys).as_deref(), Some(want), "{keys}");
    }
    Ok(())
}

/// The better-defaults layer's auto-indent-on-paste: `C-u C-y` yanks *and*
/// re-indents what it yanked to the line it lands on. Plain `C-y` does not.
#[tokio::test(flavor = "multi_thread")]
async fn c_u_c_y_reindents_what_it_yanks() -> anyhow::Result<()> {
    let mut app = preset_app("emacs")
        .with_input_text("#[i|]#f x:\n    pass\n")
        .build()?;
    test_key_sequences(
        &mut app,
        vec![
            // Kill the un-indented first line, then land inside the indented block.
            (Some("<C-space><C-e><C-w>"), None),
            (
                Some("<C-n><C-e><C-u><C-y>"),
                Some(&|app: &zmax_term::application::Application| {
                    let (_v, doc) = zmax_view::current_ref!(app.editor);
                    let text = doc.text().to_string();
                    assert!(
                        text.contains("    pass    if x:") || text.contains("    if x:"),
                        "the yank was re-indented to its new home: {text:?}"
                    );
                }),
            ),
        ],
        false,
    )
    .await?;
    Ok(())
}

/// The emoji layer's three chords: `SPC i e` inserts one from the picker,
/// `SPC a f e` browses them, and `SPC i E` is company-emoji's completion — the
/// `:name` being typed becomes the glyph.
#[tokio::test(flavor = "multi_thread")]
async fn emoji_layer_chords_resolve_and_complete() -> anyhow::Result<()> {
    use zmax_term::keymap::{KeymapResult, Keymaps};
    use zmax_view::document::Mode;

    let resolve = |keys: &str| -> Option<String> {
        let mut keymaps = Keymaps::default();
        let mut last = None;
        for key in keys.split(' ') {
            last = match keymaps.get(Mode::Normal, key.parse().expect("valid key")) {
                KeymapResult::Matched(cmd) => Some(cmd.name().to_string()),
                _ => None,
            };
        }
        last
    };
    assert_eq!(resolve("space i e").as_deref(), Some("emoji_list"));
    assert_eq!(resolve("space a f e").as_deref(), Some("emoji_list"));
    assert_eq!(resolve("space i E").as_deref(), Some("complete_emoji"));
    // The file tree kept a chord when `SPC a f` became a prefix.
    assert_eq!(resolve("space a f f").as_deref(), Some("file_explorer"));

    // The completion itself: `:catf` offers the cat faces, replacing the token.
    let mut app = preset_app("spacemacs").with_input_text("#[|]#").build()?;
    test_key_sequence(
        &mut app,
        Some("i:grinning cat<esc><space>iE<ret>"),
        Some(&|app: &zmax_term::application::Application| {
            let (_v, doc) = zmax_view::current_ref!(app.editor);
            let text = doc.text().to_string();
            assert!(
                !text.contains(':'),
                "the `:name` token was replaced by a glyph: {text:?}"
            );
            assert!(!text.trim().is_empty(), "something was inserted");
        }),
        false,
    )
    .await?;
    Ok(())
}

/// The readers layer's reflowable text mode (nov.el): an EPUB opens as text in
/// spine order, rather than only as page images.
#[tokio::test(flavor = "multi_thread")]
async fn epub_read_renders_the_spine_as_text() -> anyhow::Result<()> {
    use std::io::Write;
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("book.epub");
    {
        let file = std::fs::File::create(&path)?;
        let mut zip = zip::ZipWriter::new(file);
        let opts: zip::write::FileOptions<'_, ()> =
            zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Stored);
        zip.start_file("META-INF/container.xml", opts)?;
        zip.write_all(
            br#"<container><rootfiles><rootfile full-path="OEBPS/content.opf"/></rootfiles></container>"#,
        )?;
        zip.start_file("OEBPS/content.opf", opts)?;
        zip.write_all(
            br#"<package><manifest>
                  <item id="one" href="one.xhtml"/>
                  <item id="two" href="two.xhtml"/>
                </manifest><spine>
                  <itemref idref="two"/>
                  <itemref idref="one"/>
                </spine></package>"#,
        )?;
        zip.start_file("OEBPS/one.xhtml", opts)?;
        zip.write_all(b"<html><body><p>First chapter text.</p></body></html>")?;
        zip.start_file("OEBPS/two.xhtml", opts)?;
        zip.write_all(b"<html><body><p>Front matter.</p></body></html>")?;
        zip.finish()?;
    }

    // An EPUB is a zip, which the editor refuses to open as a buffer, so the
    // path is given — the same way nov.el is pointed at a file.
    let mut app = preset_app("spacemacs").build()?;
    let keys = format!(":epub-read {}<ret>", path.display());
    test_key_sequence(
        &mut app,
        Some(&keys),
        Some(&|app: &zmax_term::application::Application| {
            let (_v, doc) = zmax_view::current_ref!(app.editor);
            let text = doc.text().to_string();
            assert!(
                text.contains("First chapter text."),
                "chapter text: {text:?}"
            );
            assert!(text.contains("Front matter."), "front matter: {text:?}");
            // Spine order, not manifest order: "two" comes first.
            assert!(
                text.find("Front matter.") < text.find("First chapter text."),
                "documents are in spine order: {text:?}"
            );
        }),
        false,
    )
    .await?;
    Ok(())
}
