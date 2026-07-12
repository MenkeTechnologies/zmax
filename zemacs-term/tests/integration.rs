#[cfg(feature = "integration")]
mod test {
    mod helpers;

    use zemacs_core::{syntax::config::AutoPairConfig, Selection};
    use zemacs_term::config::Config;

    use indoc::indoc;

    use self::helpers::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn hello_world() -> anyhow::Result<()> {
        test(("#[\n|]#", "ihello world<esc>", "hello world#[|\n]#")).await?;
        Ok(())
    }

    mod auto_pairs;
    mod auto_reload;
    mod changelist;
    mod command_line;
    mod commands;
    mod dot_repeat;
    mod emacs_keys;
    mod hi_lock;
    mod injection;
    mod movement;
    mod operator_count;
    mod reflow;
    mod splits;
    mod abbrev_mode;
    mod ex_input;
    mod mode_abbrev;
    mod signs;
    mod tab_bar;
    mod undojoin;
    mod vim_search;
    mod vim_motions;
}
