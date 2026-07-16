use super::*;

use zmax_term::config::Config;

// vim dot-repeat (`.`) must repeat the last *change* of any kind, not just the
// last insert session. These cover the normal-mode changes that previously did
// not repeat at all. They pin the vim keymap explicitly (the harness default is
// the selection-first keymap; see `helpers::test_config`).
fn vim() -> AppBuilder {
    AppBuilder::new().with_config(Config {
        keys: zmax_term::keymap::vim::default(),
        ..Default::default()
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn dot_repeats_delete_char() -> anyhow::Result<()> {
    // x deletes under cursor; . repeats it.
    test_with_config(vim(), ("#[h|]#ello", "x.", "#[l|]#lo")).await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn dot_repeats_delete_line() -> anyhow::Result<()> {
    test_with_config(
        vim(),
        (
            indoc! {"\
                #[o|]#ne
                two
                three"},
            "dd.",
            "#[t|]#hree",
        ),
    )
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn dot_repeats_operator_motion() -> anyhow::Result<()> {
    // dw deletes a word; . repeats the operator+motion change.
    test_with_config(vim(), ("#[f|]#oo bar baz", "dw.", "#[b|]#az")).await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn dot_repeats_change_in_quotes_on_new_line() -> anyhow::Result<()> {
    // The reported bug: `ci"NEW<esc>` on one line, move to a *different* line,
    // then `.`. Dot-repeat must re-run the change on the new line — not walk the
    // quote text-object search back to the original line's quotes.
    test_with_config(
        vim(),
        (
            indoc! {r##"
                foo "#[a|]#aaaa" end
                bar "bbbbb" xyz"##},
            // change inside quotes -> NEW, leave insert, down a line to the
            // second string (cursor lands inside it), repeat.
            r#"ci"NEW<esc>j."#,
            indoc! {r##"
                foo "NEW" end
                bar "NEW#["|]# xyz"##},
        ),
    )
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn dot_repeats_change_in_quotes_cursor_before() -> anyhow::Result<()> {
    // Same change, but after `j` the cursor lands *before* the quotes on the new
    // line. Vim's `i"` grabs the next quoted string on that line, so `.` still
    // repeats there rather than reaching back to the previous line.
    test_with_config(
        vim(),
        (
            indoc! {r##"
                "#[a|]#" xxxxxxxx
                zzzz "bbbbb" yy"##},
            r#"ci"NEW<esc>j."#,
            indoc! {r##"
                "NEW" xxxxxxxx
                zzzz "NEW#["|]# yy"##},
        ),
    )
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn dot_repeats_delete_in_quotes() -> anyhow::Result<()> {
    // `di"` is an on_next_key text-object operator with no insert session; `.`
    // must repeat the delete on the next line's string.
    test_with_config(
        vim(),
        (
            indoc! {r##"
                foo "#[a|]#aaaa" end
                bar "bbbbb" xyz"##},
            r#"di"j."#,
            indoc! {r##"
                foo "" end
                bar "#["|]# xyz"##},
        ),
    )
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn dot_repeats_change_in_parens() -> anyhow::Result<()> {
    // `ci(` (bracket text object). Brackets legitimately span lines in vim, so
    // this is *not* line-restricted; the point is that `.` repeats the operator
    // at the new cursor rather than no-opping.
    test_with_config(
        vim(),
        (
            indoc! {r##"
                foo (#[a|]#aaaa) end
                bar (bbbbb) xyz"##},
            r#"ci(NEW<esc>j."#,
            indoc! {r##"
                foo (NEW) end
                bar (NEW#[)|]# xyz"##},
        ),
    )
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn dot_repeats_replace_char() -> anyhow::Result<()> {
    // `r<c>` reads its replacement via on_next_key too; `.` must repeat it.
    test_with_config(vim(), ("#[a|]#bc", "rXl.", "X#[X|]#c")).await?;
    Ok(())
}

// NOTE: operator + insert + intermediate motion (e.g. `cwX<esc>w.`) does not yet
// repeat faithfully — the replayed operator interacts with the selection-model
// motions. Pure normal-mode changes (above), text-object operators
// (`ci"`/`di"`/`ci(`), `r<c>`, and insert sessions
// (`commands::vim_dot_repeat_insert`) repeat correctly; the `cw.`-with-motion
// edge is tracked for a follow-up.

#[tokio::test(flavor = "multi_thread")]
async fn char_count_reuse() -> anyhow::Result<()> {
    // vim: `2x` then `.` deletes 2 again. abcdef -> (2x) cdef -> (.) ef
    test_with_config(vim(), ("#[a|]#bcdef", "2x.", "#[e|]#f")).await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn char_count_override() -> anyhow::Result<()> {
    // vim: `x` then `3.` deletes 3. abcdef -> (x) bcdef -> (3.) ef
    test_with_config(vim(), ("#[a|]#bcdef", "x3.", "#[e|]#f")).await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn dot_does_not_repeat_yank() -> anyhow::Result<()> {
    // vim: yank is not a "change"; after `dd` then `yy`, `.` repeats the delete.
    test_with_config(
        vim(),
        (
            indoc! {"\
            #[o|]#ne
            two
            three"},
            "ddyy.",
            "#[t|]#hree",
        ),
    )
    .await?;
    Ok(())
}
