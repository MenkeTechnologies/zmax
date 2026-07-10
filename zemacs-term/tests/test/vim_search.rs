use super::*;

use zemacs_term::config::Config;

// vim "magic" search patterns must be translated to the engine's syntax so vim
// muscle-memory works. Without translation `\(foo\)` searches for the literal
// text "(foo)" and `a+b` is a quantifier, not the literal "a+b" — both silently
// wrong. The harness default preset is `spacemacs` (vim base), so `vim_semantics`
// is on and translation applies. Each test picks a buffer where the vim reading
// and the raw-Rust reading select different text.
fn vim() -> AppBuilder {
    AppBuilder::new().with_config(Config {
        keys: zemacs_term::keymap::vim::default(),
        ..Default::default()
    })
}

/// The text of the primary selection after a search.
fn primary_fragment(app: &zemacs_term::application::Application) -> String {
    let (view, doc) = zemacs_view::current_ref!(app.editor);
    doc.selection(view.id)
        .primary()
        .fragment(doc.text().slice(..))
        .to_string()
}

/// The start offset of the primary selection.
fn primary_from(app: &zemacs_term::application::Application) -> usize {
    let (view, doc) = zemacs_view::current_ref!(app.editor);
    doc.selection(view.id).primary().from()
}

/// The whole buffer text.
fn buffer(app: &zemacs_term::application::Application) -> String {
    let (_, doc) = zemacs_view::current_ref!(app.editor);
    doc.text().to_string()
}

// Buffer "aa xx aa xx aa xx aa" — the four "aa" occurrences start at 0, 6, 12, 18.
const AA: &str = "aa xx aa xx aa xx aa";

#[tokio::test(flavor = "multi_thread")]
async fn n_after_forward_search_continues_forward() -> anyhow::Result<()> {
    // `/aa` from the first "aa" jumps to offset 6; `n` continues forward to 12.
    let mut app = vim().with_input_text(&format!("#[a|]#{}", &AA[1..])).build()?;
    test_key_sequences(
        &mut app,
        vec![
            (
                Some("/aa<ret>"),
                Some(&|app| {
                    assert!(!app.editor.is_err(), "{:?}", app.editor.get_status());
                    assert_eq!(primary_from(app), 6, "forward search lands on 2nd aa");
                }),
            ),
            (
                Some("n"),
                Some(&|app| {
                    assert_eq!(primary_from(app), 12, "n continues forward");
                }),
            ),
        ],
        false,
    )
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn n_after_backward_search_continues_backward() -> anyhow::Result<()> {
    // The core fix: `?aa` from the last "aa" (offset 18) jumps back to 12, and a
    // vim `n` must continue BACKWARD to 6 (pre-fix it went forward). `N` reverses.
    let mut app = vim()
        .with_input_text(&format!("{}#[a|]#a", &AA[..AA.len() - 1]))
        .build()?;
    test_key_sequences(
        &mut app,
        vec![
            (
                Some("?aa<ret>"),
                Some(&|app| {
                    assert!(!app.editor.is_err(), "{:?}", app.editor.get_status());
                    assert_eq!(primary_from(app), 12, "backward search lands on 3rd aa");
                }),
            ),
            (
                Some("n"),
                Some(&|app| {
                    assert_eq!(primary_from(app), 6, "n continues backward after ?");
                }),
            ),
            (
                Some("N"),
                Some(&|app| {
                    assert_eq!(primary_from(app), 12, "N reverses direction (forward)");
                }),
            ),
        ],
        false,
    )
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn magic_group_and_alternation_matches() -> anyhow::Result<()> {
    // `\(bar\)` is a group in vim; untranslated it would hunt for the literal
    // "(bar)" which is absent. It must select "bar".
    let mut app = vim().with_input_text("#[f|]#oo bar baz").build()?;
    test_key_sequence(
        &mut app,
        Some(r"/\(ba\|qu\)r<ret>"),
        Some(&|app| {
            assert!(!app.editor.is_err(), "{:?}", app.editor.get_status());
            assert_eq!(primary_fragment(app), "bar", "group+alternation matched");
        }),
        false,
    )
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn bare_plus_is_literal() -> anyhow::Result<()> {
    // In vim magic `a+b` is the literal text "a+b" (the `+` is not a quantifier).
    // The buffer has no "ab", so a raw-Rust `a+b` would find nothing; the vim
    // reading selects the literal "a+b".
    let mut app = vim().with_input_text("#[x|]#x a+b yy").build()?;
    test_key_sequence(
        &mut app,
        Some("/a+b<ret>"),
        Some(&|app| {
            assert!(!app.editor.is_err(), "{:?}", app.editor.get_status());
            assert_eq!(primary_fragment(app), "a+b", "bare + treated as literal");
        }),
        false,
    )
    .await?;
    Ok(())
}

// Word boundaries (`\<`/`\>`) can't go through the key-sequence harness — its key
// parser treats `<`/`>` as key-notation delimiters. Their engine acceptance is
// covered by the `translated_patterns_compile` unit test in `src/vim_regex.rs`.

#[tokio::test(flavor = "multi_thread")]
async fn counted_quantifier_matches() -> anyhow::Result<()> {
    // vim `a\{3}` — exactly three a's.
    let mut app = vim().with_input_text("#[b|]#b aaaa cc").build()?;
    test_key_sequence(
        &mut app,
        Some(r"/a\{3}<ret>"),
        Some(&|app| {
            assert!(!app.editor.is_err(), "{:?}", app.editor.get_status());
            assert_eq!(primary_fragment(app), "aaa", "counted quantifier matched");
        }),
        false,
    )
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn cgn_changes_match_and_dot_repeats() -> anyhow::Result<()> {
    // /foo sets the pattern and lands on the first match; cgnX changes it, and `.`
    // walks to the next match and changes it too.
    let mut app = vim().with_input_text("#[a|]#a foo bb foo cc").build()?;
    test_key_sequences(
        &mut app,
        vec![
            (Some("/foo<ret>"), None),
            (
                Some("cgnX<esc>"),
                Some(&|app| {
                    assert_eq!(buffer(app), "aa X bb foo cc");
                }),
            ),
            (
                Some("."),
                Some(&|app| {
                    assert_eq!(buffer(app), "aa X bb X cc");
                }),
            ),
        ],
        false,
    )
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn dgn_deletes_match_at_cursor() -> anyhow::Result<()> {
    // /foo lands on the match; dgn deletes that match (the one at the cursor).
    let mut app = vim().with_input_text("#[a|]#a foo bb").build()?;
    test_key_sequences(
        &mut app,
        vec![
            (Some("/foo<ret>"), None),
            (
                Some("dgn"),
                Some(&|app| {
                    assert_eq!(buffer(app), "aa  bb");
                }),
            ),
        ],
        false,
    )
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn count_on_search_jumps_to_nth_match() -> anyhow::Result<()> {
    // Four "foo" at offsets 0,6,12,18. `3/foo` from the first jumps to the 4th
    // (three matches forward = offset 18).
    let mut app = vim().with_input_text("#[f|]#oo a foo b foo c foo").build()?;
    test_key_sequence(
        &mut app,
        Some("3/foo<ret>"),
        Some(&|app| {
            assert!(!app.editor.is_err(), "{:?}", app.editor.get_status());
            assert_eq!(primary_from(app), 18, "3/foo lands three matches forward");
        }),
        false,
    )
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn search_offset_end() -> anyhow::Result<()> {
    // `/foo/e` lands on the LAST char of the match ("foo" at 3..6 → offset 5).
    let mut app = vim().with_input_text("#[x|]#x foo yy").build()?;
    test_key_sequence(&mut app, Some("/foo/e<ret>"), Some(&|app| {
        assert!(!app.editor.is_err(), "{:?}", app.editor.get_status());
        assert_eq!(primary_from(app), 5, "/foo/e lands on match end");
    }), false).await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn search_offset_default_start() -> anyhow::Result<()> {
    // `/foo/` (no offset) lands on the match start (offset 3).
    let mut app = vim().with_input_text("#[x|]#x foo yy").build()?;
    test_key_sequence(&mut app, Some("/foo<ret>"), Some(&|app| {
        assert_eq!(primary_from(app), 3, "plain search lands on match start");
    }), false).await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn search_offset_line_below() -> anyhow::Result<()> {
    // `/foo/+1` moves one line below the match, to the first non-blank ('b' at 9).
    let mut app = vim().with_input_text("#[x|]#x foo\n  bar\n").build()?;
    test_key_sequence(&mut app, Some("/foo/+1<ret>"), Some(&|app| {
        assert_eq!(primary_from(app), 9, "/foo/+1 lands a line below at first non-blank");
    }), false).await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn incsearch_ctrl_g_cycles_to_next_match() -> anyhow::Result<()> {
    // /foo previews the first match (offset 6); C-g advances to the next (12);
    // Enter commits there.
    let mut app = vim().with_input_text("#[f|]#oo a foo b foo").build()?;
    test_key_sequence(&mut app, Some("/foo<C-g><ret>"), Some(&|app| {
        assert!(!app.editor.is_err(), "{:?}", app.editor.get_status());
        assert_eq!(primary_from(app), 12, "C-g advanced the incsearch preview, committed there");
    }), false).await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn incsearch_ctrl_g_ctrl_t_cycle_back() -> anyhow::Result<()> {
    // matches at 0,6,12,18. /foo -> 6, C-g -> 12, C-g -> 18, C-t -> 12; commit 12.
    let mut app = vim().with_input_text("#[f|]#oo a foo b foo c foo").build()?;
    test_key_sequence(&mut app, Some("/foo<C-g><C-g><C-t><ret>"), Some(&|app| {
        assert!(!app.editor.is_err(), "{:?}", app.editor.get_status());
        assert_eq!(primary_from(app), 12, "net one forward advance, no wrap");
    }), false).await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn incsearch_plain_search_still_first_match() -> anyhow::Result<()> {
    // No cycling: /foo + Enter still lands on the first forward match (6).
    let mut app = vim().with_input_text("#[f|]#oo a foo b foo").build()?;
    test_key_sequence(&mut app, Some("/foo<ret>"), Some(&|app| {
        assert!(!app.editor.is_err(), "{:?}", app.editor.get_status());
        assert_eq!(primary_from(app), 6, "plain search unchanged");
    }), false).await?;
    Ok(())
}

// vim `:s/pat/rep/c` — interactive per-match confirmation. `y` replaces, `n`
// skips, `a` replaces the rest, `l` replaces this then stops, `q` stops. The
// prompt is a modal layer pushed when the command validates, so each test runs
// the `:s...c` command first (draining the event loop so the layer appears),
// then feeds the confirm keys — matching how a human uses the prompt.
async fn confirm_case(confirm_keys: &str, expect: &str, why: &'static str) -> anyhow::Result<()> {
    let mut app = vim().with_input_text("#[f|]#oo\nfoo\nfoo").build()?;
    let check: &dyn Fn(&zemacs_term::application::Application) =
        &move |app| assert_eq!(buffer(app), expect, "{}", why);
    test_key_sequences(
        &mut app,
        vec![
            (Some(":%s/foo/bar/c<ret>"), None),
            (Some(confirm_keys), Some(check)),
        ],
        false,
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn substitute_confirm_yes_no_yes() -> anyhow::Result<()> {
    confirm_case("yny", "bar\nfoo\nbar", "y skip y").await
}

#[tokio::test(flavor = "multi_thread")]
async fn substitute_confirm_all() -> anyhow::Result<()> {
    confirm_case("a", "bar\nbar\nbar", "a replaces the rest").await
}

#[tokio::test(flavor = "multi_thread")]
async fn substitute_confirm_last() -> anyhow::Result<()> {
    confirm_case("yl", "bar\nbar\nfoo", "y then l (this + stop)").await
}

#[tokio::test(flavor = "multi_thread")]
async fn substitute_confirm_quit() -> anyhow::Result<()> {
    confirm_case("yq", "bar\nfoo\nfoo", "y then q (stop)").await
}

// vim visual-block (`<C-v>`): select a rectangle with free 2D motion, then an
// operator applies to the block. This is the proper block workflow (the forced
// operator form `d<C-v>motion` can only express a 1D block via static keys).
#[tokio::test(flavor = "multi_thread")]
async fn visual_block_delete_rectangle() -> anyhow::Result<()> {
    // 3x3 grid; block-select the left 2 columns over all 3 rows, delete -> "c" rows.
    let mut app = vim().with_input_text("#[a|]#bc\nabc\nabc").build()?;
    test_key_sequence(&mut app, Some("<C-v>jjld"), Some(&|app| {
        assert_eq!(buffer(app), "c\nc\nc", "block delete removed the 2-col rectangle");
    }), false).await?;
    Ok(())
}

// nvim `gc` comment operator (added to the vim `g` submap). Needs a language
// with comment tokens, so each test sets `:lang rust` first.
#[tokio::test(flavor = "multi_thread")]
async fn vim_gcc_comments_current_line() -> anyhow::Result<()> {
    let mut app = vim().with_input_text("#[f|]#oo").build()?;
    test_key_sequence(&mut app, Some(":lang rust<ret>gcc"), Some(&|app| {
        assert_eq!(buffer(app), "// foo", "gcc comments the current line");
    }), false).await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn vim_visual_gc_comments_selection() -> anyhow::Result<()> {
    let mut app = vim().with_input_text("#[f|]#oo\nbar").build()?;
    test_key_sequence(&mut app, Some(":lang rust<ret>Vjgc"), Some(&|app| {
        assert_eq!(buffer(app), "// foo\n// bar", "visual gc comments both lines");
    }), false).await?;
    Ok(())
}

// vim `:set commentstring=#%s` overrides the comment token used by the comment
// operator (the prefix before `%s`), even with no language comment token.
#[tokio::test(flavor = "multi_thread")]
async fn vim_commentstring_overrides_comment_token() -> anyhow::Result<()> {
    let mut app = vim().with_input_text("#[f|]#oo").build()?;
    test_key_sequence(&mut app, Some(":set commentstring=#%s<ret>gcc"), Some(&|app| {
        assert_eq!(buffer(app), "# foo", "commentstring prefix used as line-comment token");
    }), false).await?;
    Ok(())
}

// vim `:set nostartofline`: G keeps the cursor's column instead of jumping to the
// first non-blank of the target line.
#[tokio::test(flavor = "multi_thread")]
async fn vim_nostartofline_keeps_column() -> anyhow::Result<()> {
    // line1 "abcdefg" (cursor col 4 = 'e'); line2 "  xxxxxxx" (first non-blank col 2).
    let mut app = vim().with_input_text("abcd#[e|]#fg\n  xxxxxxx").build()?;
    test_key_sequence(&mut app, Some(":set nostartofline<ret>G"), Some(&|app| {
        // nostartofline -> column 4 on line 2 = index 8 + 4 = 12 (not the default 10).
        assert_eq!(primary_from(app), 12, "G keeps column 4 with nostartofline");
    }), false).await?;
    Ok(())
}

// nostartofline also applies to bare `gg` (keep column, not first non-blank).
#[tokio::test(flavor = "multi_thread")]
async fn vim_nostartofline_keeps_column_gg() -> anyhow::Result<()> {
    // line1 "  aaaaa" (first non-blank col 2); line2 "xyzw" cursor col 3 = 'w'.
    let mut app = vim().with_input_text("  aaaaa\nxyz#[w|]#").build()?;
    test_key_sequence(&mut app, Some(":set nostartofline<ret>gg"), Some(&|app| {
        // nostartofline -> column 3 on line 1 = index 3 (not the default first non-blank 2).
        assert_eq!(primary_from(app), 3, "gg keeps column 3 with nostartofline");
    }), false).await?;
    Ok(())
}

// vim `:set iskeyword`: adding a char (here `-`, code 45) makes word motions treat
// it as part of a word. `e` then moves to the end of "foo-bar", not "foo".
// Resets iskeyword afterward so the core thread-local doesn't leak into other tests.
#[tokio::test(flavor = "multi_thread")]
async fn vim_iskeyword_extends_word_motion() -> anyhow::Result<()> {
    let mut app = vim().with_input_text("#[f|]#oo-bar baz").build()?;
    test_key_sequence(&mut app, Some(":set iskeyword=@,48-57,_,45<ret>e<esc>:set iskeyword=<ret>"), Some(&|app| {
        assert_eq!(primary_from(app), 6, "e reaches end of foo-bar with `-` as keyword char");
    }), false).await?;
    Ok(())
}

// vim `:set foldmethod=indent` recomputes folds from indentation.
#[tokio::test(flavor = "multi_thread")]
async fn vim_foldmethod_indent_creates_folds() -> anyhow::Result<()> {
    let mut app = vim().with_input_text("#[f|]#n foo:\n    a\n    b\nbar").build()?;
    test_key_sequence(&mut app, Some(":set foldmethod=indent<ret>"), Some(&|app| {
        let (_v, doc) = zemacs_view::current_ref!(app.editor);
        assert!(doc.folds().len() >= 1, "indent foldmethod created a fold");
    }), false).await?;
    Ok(())
}

// vim `:set bomb` / `:set nobomb` toggle the document's byte-order-mark on write.
#[tokio::test(flavor = "multi_thread")]
async fn vim_bomb_toggles_document_bom() -> anyhow::Result<()> {
    let mut app = vim().with_input_text("#[x|]#").build()?;
    test_key_sequences(&mut app, vec![
        (Some(":set bomb<ret>"), Some(&|app: &zemacs_term::application::Application| {
            let (_v, doc) = zemacs_view::current_ref!(app.editor);
            assert!(doc.has_bom(), ":set bomb enables the BOM");
        })),
        (Some(":set nobomb<ret>"), Some(&|app: &zemacs_term::application::Application| {
            let (_v, doc) = zemacs_view::current_ref!(app.editor);
            assert!(!doc.has_bom(), ":set nobomb disables the BOM");
        })),
    ], false).await?;
    Ok(())
}

// vim `:set foldlevel`: 0 closes all folds, a high value opens them. Drives the
// folds created by foldmethod=indent.
#[tokio::test(flavor = "multi_thread")]
async fn vim_foldlevel_closes_and_opens_folds() -> anyhow::Result<()> {
    let mut app = vim().with_input_text("#[f|]#n foo:\n    a\n    b\nbar").build()?;
    test_key_sequences(&mut app, vec![
        (Some(":set foldmethod=indent<ret>:set foldlevel=0<ret>"), Some(&|app: &zemacs_term::application::Application| {
            let (_v, doc) = zemacs_view::current_ref!(app.editor);
            assert!(doc.folds().len() >= 1, "folds exist");
            assert!(doc.folds().iter().all(|f| f.closed), "foldlevel=0 closes all folds");
        })),
        (Some(":set foldlevel=99<ret>"), Some(&|app: &zemacs_term::application::Application| {
            let (_v, doc) = zemacs_view::current_ref!(app.editor);
            assert!(doc.folds().iter().all(|f| !f.closed), "foldlevel=99 opens all folds");
        })),
    ], false).await?;
    Ok(())
}

// vim `:set backupdir` / `:set backupskip` are recognized and their values feed
// the config schema. vim_set deserializes the updated config with
// `deny_unknown_fields` *before* applying it, so an unwired key would raise an
// error status here — a passing (no-error) run proves the `backup-dir` /
// `backup-skip` Config fields and `:set` arms line up. The path logic that
// consumes them is unit-tested by `backup_plan` in zemacs-view.
#[tokio::test(flavor = "multi_thread")]
async fn vim_backupdir_backupskip_recognized() -> anyhow::Result<()> {
    let mut app = vim().with_input_text("#[x|]#").build()?;
    test_key_sequence(&mut app, Some(":set backupdir=/tmp/zbak<ret>:set backupskip=/tmp/*<ret>"), Some(&|app| {
        assert!(!app.editor.is_err(), "{:?}", app.editor.get_status());
    }), false).await?;
    Ok(())
}

// vim `:set foldmethod=syntax` folds tree-sitter function/class regions.
#[tokio::test(flavor = "multi_thread")]
async fn vim_foldmethod_syntax_folds_functions() -> anyhow::Result<()> {
    let mut app = vim().with_input_text("#[f|]#n foo() {\n    let a = 1;\n    let b = 2;\n}\nfn bar() {\n    baz();\n}").build()?;
    test_key_sequences(&mut app, vec![
        (Some(":lang rust<ret>"), None),
        (Some(":set foldmethod=syntax<ret>"), Some(&|app: &zemacs_term::application::Application| {
            let (_v, doc) = zemacs_view::current_ref!(app.editor);
            assert!(doc.folds().len() >= 2, "syntax foldmethod folded both functions, got {}", doc.folds().len());
            // first fold starts at the `fn foo` line (0) and spans multiple lines.
            assert!(doc.folds().iter().any(|f| f.start == 0 && f.end >= 2), "fn foo folded");
        })),
    ], false).await?;
    Ok(())
}

// vim `K` with `:set keywordprg=<prog>` runs the program on the word under the
// cursor and shows its output in a scratch buffer (here echo, so the word itself).
#[tokio::test(flavor = "multi_thread")]
async fn vim_keywordprg_runs_on_word() -> anyhow::Result<()> {
    let mut app = vim().with_input_text("#[f|]#oobar baz").build()?;
    test_key_sequence(&mut app, Some(":set keywordprg=echo<ret>K"), Some(&|app| {
        assert!(buffer(app).contains("foobar"), "K ran keywordprg on the word: {:?}", buffer(app));
    }), false).await?;
    Ok(())
}

// vim `formatoptions` `r` flag gates comment-leader continuation after <Enter>.
// Default (unset) keeps zemacs's behaviour (continue); with fo set but no `r`,
// <Enter> in a comment starts a bare line. Resets the store afterward.
#[tokio::test(flavor = "multi_thread")]
async fn vim_formatoptions_r_default_continues() -> anyhow::Result<()> {
    let mut app = vim().with_input_text("// #[f|]#oo").build()?;
    test_key_sequence(&mut app, Some(":lang rust<ret>A<ret>x<esc>"), Some(&|app| {
        assert_eq!(buffer(app), "// foo\n// x", "default: <Enter> continues the comment");
    }), false).await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn vim_formatoptions_without_r_stops_continuation() -> anyhow::Result<()> {
    let mut app = vim().with_input_text("// #[f|]#oo").build()?;
    test_key_sequence(&mut app, Some(":lang rust<ret>:set formatoptions=q<ret>A<ret>x<esc>:set formatoptions&<ret>"), Some(&|app| {
        assert_eq!(buffer(app), "// foo\nx", "formatoptions without r: no continuation");
    }), false).await?;
    Ok(())
}

// vim formatoptions `j`: joining a comment line onto another drops the joined
// line's comment leader. Default (no j) keeps it. Resets the store afterward.
#[tokio::test(flavor = "multi_thread")]
async fn vim_formatoptions_j_strips_comment_leader_on_join() -> anyhow::Result<()> {
    let mut app = vim().with_input_text("// #[f|]#oo\n// bar").build()?;
    test_key_sequence(&mut app, Some(":lang rust<ret>:set formatoptions=j<ret>J:set formatoptions&<ret>"), Some(&|app| {
        assert_eq!(buffer(app), "// foo bar", "formatoptions j: join drops the second // leader");
    }), false).await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn vim_join_without_j_keeps_comment_leader() -> anyhow::Result<()> {
    let mut app = vim().with_input_text("// #[f|]#oo\n// bar").build()?;
    test_key_sequence(&mut app, Some(":lang rust<ret>J"), Some(&|app| {
        assert_eq!(buffer(app), "// foo // bar", "default: join keeps the second // leader");
    }), false).await?;
    Ok(())
}

// vim formatoptions `t`: typing past text_width (default 80) auto-wraps the line
// (via the shared auto-fill). Default (no t) leaves one long line.
#[tokio::test(flavor = "multi_thread")]
async fn vim_formatoptions_t_auto_wraps() -> anyhow::Result<()> {
    let text = "aa ".repeat(35); // 105 chars > 80
    let mut app = vim().with_input_text("#[\n|]#").build()?;
    let keys = format!(":set formatoptions=t<ret>i{}<esc>:set formatoptions&<ret>", text);
    test_key_sequence(&mut app, Some(&keys), Some(&|app| {
        let max = buffer(app).lines().map(|l| l.chars().count()).max().unwrap_or(0);
        assert!(max <= 82, "formatoptions=t wrapped the line, max line = {}", max);
    }), false).await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn vim_no_autowrap_by_default() -> anyhow::Result<()> {
    let text = "aa ".repeat(35);
    let mut app = vim().with_input_text("#[\n|]#").build()?;
    let keys = format!("i{}<esc>", text);
    test_key_sequence(&mut app, Some(&keys), Some(&|app| {
        let max = buffer(app).lines().map(|l| l.chars().count()).max().unwrap_or(0);
        assert!(max > 82, "no auto-wrap by default, max line = {}", max);
    }), false).await?;
    Ok(())
}

// vim `quoteescape` (default `\`): `di"` on a string containing escaped quotes
// spans the whole string rather than stopping at the first `\"`.
#[tokio::test(flavor = "multi_thread")]
async fn vim_quoteescape_di_quote_spans_escaped() -> anyhow::Result<()> {
    // text: "a \"b\" c"  (cursor on the first `a`)
    let mut app = vim().with_input_text("\"#[a|]# \\\"b\\\" c\"").build()?;
    test_key_sequence(&mut app, Some("di\""), Some(&|app| {
        assert_eq!(buffer(app), "\"\"", "di\" deletes the whole escaped-quote string");
    }), false).await?;
    Ok(())
}
