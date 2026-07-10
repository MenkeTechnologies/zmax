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
