use super::*;

use zemacs_term::config::Config;

// New vim motions implemented as real commands (g_/gM/go). Pin the vim keymap
// (harness default is the Helix keymap; see `helpers::test_config`).
fn vim() -> AppBuilder {
    AppBuilder::new().with_config(Config {
        keys: zemacs_term::keymap::vim::default(),
        ..Default::default()
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn g_underscore_goes_to_last_nonblank() -> anyhow::Result<()> {
    // cursor at line start; g_ lands on the last non-whitespace char ('b').
    test_with_config(vim(), ("#[ |]# ab  ", "g_", "  a#[b|]#  ")).await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn gm_capital_goes_to_text_line_middle() -> anyhow::Result<()> {
    // 10-char line; gM lands on the middle column (index 5 = '5').
    test_with_config(vim(), ("#[0|]#123456789", "gM", "01234#[5|]#6789")).await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn go_goes_to_byte_count() -> anyhow::Result<()> {
    // 3go -> byte 3 (1-based) = char index 2 = 'c'.
    test_with_config(vim(), ("#[a|]#bcdef", "3go", "ab#[c|]#def")).await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn bracket_prev_unmatched_paren() -> anyhow::Result<()> {
    // cursor inside the inner pair; [( jumps to the enclosing unmatched '('.
    // text: ( a ( b ) c )  cursor on 'b'; nearest unmatched '(' to the left is
    // the outer one at index 0 (the inner '(' is matched by its ')').
    test_with_config(vim(), ("(a(#[b|]#)c)", "[(", "(a#[(|]#b)c)")).await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn backtick_visual_start_mark() -> anyhow::Result<()> {
    // select "cd" (v l), leave visual, go to line start, then `< jumps back to
    // the start of the last visual area (the 'c').
    test_with_config(vim(), ("ab#[c|]#de", "vl<esc>0`<lt>", "ab#[c|]#de")).await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn bracket_next_unmatched_brace() -> anyhow::Result<()> {
    // cursor before the inner pair; ]} jumps to the enclosing unmatched '}'.
    test_with_config(vim(), ("{a#[b|]#{c}d}", "]}", "{ab{c}d#[}|]#")).await?;
    Ok(())
}
