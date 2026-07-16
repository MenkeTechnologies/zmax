use super::*;

use zmax_term::config::Config;

// vim multiplies the operator count and the motion count: `2d3w` deletes
// `2 * 3 = 6` words, NOT `23` words (the pre-fix behavior concatenated the two
// digits into a single count via the shared `editor.count`). These pin that
// product semantics. The harness default keymap is `spacemacs`, which carries
// the vim base, so `vim_semantics` is on and the multiplication applies.
fn vim() -> AppBuilder {
    AppBuilder::new().with_config(Config {
        keys: zmax_term::keymap::vim::default(),
        ..Default::default()
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn operator_count_times_motion_count() -> anyhow::Result<()> {
    // `2d3w` deletes six words ("a b c d e f "), leaving "g h".
    test_with_config(vim(), ("#[a|]# b c d e f g h", "2d3w", "#[g|]# h")).await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn operator_count_alone() -> anyhow::Result<()> {
    // `3dd` deletes three whole lines (operator count with no motion count).
    test_with_config(
        vim(),
        (
            indoc! {"\
                #[o|]#ne
                two
                three
                four"},
            "3dd",
            "#[f|]#our",
        ),
    )
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn motion_count_alone() -> anyhow::Result<()> {
    // `d3w` deletes three words ("a b c "), leaving "d e".
    test_with_config(vim(), ("#[a|]# b c d e", "d3w", "#[d|]# e")).await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn plain_operator_count_no_second_count() -> anyhow::Result<()> {
    // `2dw` deletes two words ("a b "), leaving "c d". Guards against the
    // operator-count snapshot dropping the count when no second count follows.
    test_with_config(vim(), ("#[a|]# b c d", "2dw", "#[c|]# d")).await?;
    Ok(())
}
