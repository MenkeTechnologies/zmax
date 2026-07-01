use super::*;

use zemacs_term::config::Config;

// vim dot-repeat (`.`) must repeat the last *change* of any kind, not just the
// last insert session. These cover the normal-mode changes that previously did
// not repeat at all. They pin the vim keymap explicitly (the harness default is
// the selection-first keymap; see `helpers::test_config`).
fn vim() -> AppBuilder {
    AppBuilder::new().with_config(Config {
        keys: zemacs_term::keymap::vim::default(),
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

// NOTE: operator + insert + intermediate motion (e.g. `cwX<esc>w.`) does not yet
// repeat faithfully — the replayed operator interacts with the selection-model
// motions. Pure normal-mode changes (above) and insert sessions
// (`commands::vim_dot_repeat_insert`) repeat correctly; the `cw.`-with-motion edge
// is tracked for a follow-up.
