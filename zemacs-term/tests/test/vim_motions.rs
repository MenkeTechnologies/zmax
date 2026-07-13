use super::*;

use zemacs_term::config::Config;

// New vim motions implemented as real commands (g_/gM/go). Pin the vim keymap
// (harness default is the selection-first keymap; see `helpers::test_config`).
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

// Word motions must land the caret ON the target char like vim, not one short of
// it (selection block-cursor semantics). Text "foo bar baz":
// f0 o1 o2 ' '3 b4 a5 r6 ' '7 b8 a9 z10.
#[tokio::test(flavor = "multi_thread")]
async fn w_lands_on_next_word_first_char() -> anyhow::Result<()> {
    // vim `w`: onto 'b' of "bar", not the space before it.
    test_with_config(vim(), ("#[f|]#oo bar baz", "w", "foo #[b|]#ar baz")).await?;
    Ok(())
}

// Regression: cursor ON the indentation/whitespace before the first token. The
// block cursor's head sits on the token's first char, so reading `range.head`
// (instead of the visual caret) skipped the token and landed on `=`. vim lands
// on the first token. Mirrors a `.gitmodules` line `\tpath = zshrs` with the
// caret on the leading tab.
#[tokio::test(flavor = "multi_thread")]
async fn w_from_leading_whitespace_lands_on_first_token() -> anyhow::Result<()> {
    // caret on the leading space -> 'p' of path, not '='.
    test_with_config(vim(), ("#[ |]#path = zshrs", "w", " #[p|]#ath = zshrs")).await?;
    // caret on the space directly before `=` -> `=`, not the token past it.
    test_with_config(vim(), ("foo#[ |]#= bar", "w", "foo #[=|]# bar")).await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn e_lands_on_word_last_char() -> anyhow::Result<()> {
    // vim `e`: onto last char of "foo".
    test_with_config(vim(), ("#[f|]#oo bar baz", "e", "fo#[o|]# bar baz")).await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn b_lands_on_prev_word_first_char() -> anyhow::Result<()> {
    // vim `b` from the start of "baz": onto 'b' of "bar".
    test_with_config(vim(), ("foo bar #[b|]#az", "b", "foo #[b|]#ar baz")).await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn ge_lands_on_prev_word_last_char() -> anyhow::Result<()> {
    // vim `ge` from the start of "baz": onto 'r', the last char of "bar", not the
    // space after it.
    test_with_config(vim(), ("foo bar #[b|]#az", "ge", "foo ba#[r|]# baz")).await?;
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
async fn emacs_meta_m_back_to_indentation() -> anyhow::Result<()> {
    // M-m moves to the first non-blank char of the line.
    test_with_config(vim(), ("#[ |]# ab", "<A-m>", "  #[a|]#b")).await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn indent_operator_double() -> anyhow::Result<()> {
    // >> indents the current line by one shiftwidth (4 spaces here).
    test_with_config(vim(), ("#[f|]#oo\nbar\n", "<gt><gt>", "\t#[f|]#oo\nbar\n")).await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn indent_operator_with_motion() -> anyhow::Result<()> {
    // >j indents the current line and the next.
    test_with_config(
        vim(),
        ("#[f|]#oo\nbar\nbaz\n", "<gt>j", "\t#[f|]#oo\n\tbar\nbaz\n"),
    )
    .await?;
    Ok(())
}

// NOTE: the classic vim `f`/`t`/`F`/`T` character-find (and therefore the `;`/`,`
// find-repeat this test exercised) was intentionally replaced by EasyMotion-style
// label jumps in `feat(easymotion): label-jump for f/t/F/T`. `fx` no longer walks
// to the next `x`; it prompts for a target and labels every match. The old
// `comma_repeats_find_reversed` case tested behavior that no longer exists, so it
// was removed rather than left asserting a dead code path.

#[tokio::test(flavor = "multi_thread")]
async fn bracket_next_lowercase_mark() -> anyhow::Result<()> {
    // set mark 'a' on the 'c', return to start, then ]` jumps forward to mark 'a'.
    test_with_config(vim(), ("ab#[c|]#de", "ma0]`", "ab#[c|]#de")).await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn bracket_next_unmatched_brace() -> anyhow::Result<()> {
    // cursor before the inner pair; ]} jumps to the enclosing unmatched '}'.
    test_with_config(vim(), ("{a#[b|]#{c}d}", "]}", "{ab{c}d#[}|]#")).await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn ctrl_v_builds_rectangular_block() -> anyhow::Result<()> {
    // From the top-left, CTRL-V then grow down two rows and right one column:
    // a 2-wide rectangle over all three lines, primary on the active (last) row.
    test_with_config(
        vim(),
        (
            "#[f|]#oo\nbar\nbaz",
            "<C-v>jjl",
            "#(fo|)#o\n#(ba|)#r\n#[ba|]#z",
        ),
    )
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn ctrl_v_o_jumps_to_opposite_corner() -> anyhow::Result<()> {
    // Build the same block, then `o` moves the cursor to the opposite (top-left)
    // corner: the rectangle is unchanged but the primary/cursor is now top-left.
    test_with_config(
        vim(),
        (
            "#[f|]#oo\nbar\nbaz",
            "<C-v>jjlo",
            "#[|fo]#o\n#(|ba)#r\n#(|ba)#z",
        ),
    )
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn ctrl_v_uppercase_o_swaps_column_edge() -> anyhow::Result<()> {
    // Build the block (cursor bottom-right), then `O` moves the cursor to the
    // other column edge on the same row: same rectangle, cursor now bottom-left.
    test_with_config(
        vim(),
        (
            "#[f|]#oo\nbar\nbaz",
            "<C-v>jjlO",
            "#(|fo)#o\n#(|ba)#r\n#[|ba]#z",
        ),
    )
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn ctrl_v_dollar_is_ragged_right() -> anyhow::Result<()> {
    // CTRL-V $ extends each row to its own line end (ragged right): the two rows
    // of differing length each select to their end, without swallowing the
    // newline (head stops at end-of-content, not past it).
    test_with_config(vim(), ("#[a|]#b\nabcd", "<C-v>j$", "#(ab|)#\n#[abcd|]#")).await?;
    Ok(())
}

/// Spacemacs subword-mode (SPC t c): `w` moves by sub-word, splitting CamelCase.
#[tokio::test(flavor = "multi_thread")]
async fn subword_w_splits_camelcase() -> anyhow::Result<()> {
    use std::io::Write;
    let mut file = tempfile::NamedTempFile::new()?;
    write!(file, "fooBarBaz")?;
    file.flush()?;
    let mut app = helpers::AppBuilder::new()
        .with_config(Config {
            keys: zemacs_term::keymap::vim::default(),
            ..Default::default()
        })
        .with_file(file.path(), None)
        .build()?;
    app.editor.subword = true; // as if SPC t c had been pressed

    test_key_sequences(
        &mut app,
        vec![(
            Some("w"),
            Some(&|app| {
                let view = app.editor.tree.get(app.editor.tree.focus);
                let doc = app.editor.documents().next().unwrap();
                let pos = doc
                    .selection(view.id)
                    .primary()
                    .cursor(doc.text().slice(..));
                assert_eq!(3, pos, "subword `w` should land on 'B' of Bar (col 3)");
            }),
        )],
        false,
    )
    .await?;
    Ok(())
}

/// With subword-mode on, `dw` deletes a single sub-word.
#[tokio::test(flavor = "multi_thread")]
async fn subword_dw_deletes_one_subword() -> anyhow::Result<()> {
    use std::io::Write;
    let mut file = tempfile::NamedTempFile::new()?;
    write!(file, "fooBarBaz")?;
    file.flush()?;
    let mut app = helpers::AppBuilder::new()
        .with_config(Config {
            keys: zemacs_term::keymap::vim::default(),
            ..Default::default()
        })
        .with_file(file.path(), None)
        .build()?;
    app.editor.subword = true;

    test_key_sequences(
        &mut app,
        vec![(
            Some("dw"),
            Some(&|app| {
                let doc = app.editor.documents().next().unwrap();
                assert_eq!(
                    "BarBaz",
                    doc.text().to_string(),
                    "subword `dw` deletes only 'foo'"
                );
            }),
        )],
        false,
    )
    .await?;
    Ok(())
}

/// Emacs superword-mode (SPC t C): `w` moves over a whole symbol, so the
/// punctuation-joined `foo-bar` is one word. Without superword, vim `w` would
/// stop on the `-` (index 3, a separate punctuation category); with superword on
/// it skips to the next super-word `baz` (index 8).
#[tokio::test(flavor = "multi_thread")]
async fn superword_w_moves_over_hyphenated_word() -> anyhow::Result<()> {
    use std::io::Write;
    let mut file = tempfile::NamedTempFile::new()?;
    write!(file, "foo-bar baz")?;
    file.flush()?;
    let mut app = helpers::AppBuilder::new()
        .with_config(Config {
            keys: zemacs_term::keymap::vim::default(),
            ..Default::default()
        })
        .with_file(file.path(), None)
        .build()?;
    app.editor.superword = true; // as if SPC t C had been pressed

    test_key_sequences(
        &mut app,
        vec![(
            Some("w"),
            Some(&|app| {
                let view = app.editor.tree.get(app.editor.tree.focus);
                let doc = app.editor.documents().next().unwrap();
                let pos = doc
                    .selection(view.id)
                    .primary()
                    .cursor(doc.text().slice(..));
                assert_eq!(
                    8, pos,
                    "superword `w` should skip `foo-bar` and land on 'b' of baz (col 8)"
                );
            }),
        )],
        false,
    )
    .await?;
    Ok(())
}

/// With superword-mode on, `dw` deletes the whole punctuation-joined symbol
/// (`foo-bar` plus its trailing space), leaving `baz`. Without superword, vim
/// `dw` would delete only `foo` up to the `-`.
#[tokio::test(flavor = "multi_thread")]
async fn superword_dw_deletes_whole_symbol() -> anyhow::Result<()> {
    use std::io::Write;
    let mut file = tempfile::NamedTempFile::new()?;
    write!(file, "foo-bar baz")?;
    file.flush()?;
    let mut app = helpers::AppBuilder::new()
        .with_config(Config {
            keys: zemacs_term::keymap::vim::default(),
            ..Default::default()
        })
        .with_file(file.path(), None)
        .build()?;
    app.editor.superword = true;

    test_key_sequences(
        &mut app,
        vec![(
            Some("dw"),
            Some(&|app| {
                let doc = app.editor.documents().next().unwrap();
                assert_eq!(
                    "baz",
                    doc.text().to_string(),
                    "superword `dw` deletes the whole `foo-bar ` symbol"
                );
            }),
        )],
        false,
    )
    .await?;
    Ok(())
}

/// vim `{count}dd`: `5dd` deletes 5 whole lines from the cursor line, not 1.
/// Regression guard for the count-blind `extend_to_line_bounds` that made every
/// `Ndd`/`Nyy`/`Ncc` collapse to a single line.
#[tokio::test(flavor = "multi_thread")]
async fn count_dd_deletes_count_lines() -> anyhow::Result<()> {
    use std::io::Write;
    let mut file = tempfile::NamedTempFile::new()?;
    write!(file, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\n")?;
    file.flush()?;
    let mut app = helpers::AppBuilder::new()
        .with_config(Config {
            keys: zemacs_term::keymap::vim::default(),
            ..Default::default()
        })
        .with_file(file.path(), None)
        .build()?;

    test_key_sequences(
        &mut app,
        vec![(
            Some("5dd"),
            Some(&|app| {
                let doc = app.editor.documents().next().unwrap();
                assert_eq!(
                    "l6\nl7\nl8\n",
                    doc.text().to_string(),
                    "`5dd` deletes lines l1..l5, leaving l6..l8"
                );
            }),
        )],
        false,
    )
    .await?;
    Ok(())
}

/// vim `dd` with no count still deletes exactly one line (pins the count=1 path
/// so the count fix can't regress the common case).
#[tokio::test(flavor = "multi_thread")]
async fn plain_dd_deletes_one_line() -> anyhow::Result<()> {
    use std::io::Write;
    let mut file = tempfile::NamedTempFile::new()?;
    write!(file, "l1\nl2\nl3\n")?;
    file.flush()?;
    let mut app = helpers::AppBuilder::new()
        .with_config(Config {
            keys: zemacs_term::keymap::vim::default(),
            ..Default::default()
        })
        .with_file(file.path(), None)
        .build()?;

    test_key_sequences(
        &mut app,
        vec![(
            Some("dd"),
            Some(&|app| {
                let doc = app.editor.documents().next().unwrap();
                assert_eq!(
                    "l2\nl3\n",
                    doc.text().to_string(),
                    "`dd` deletes only the current line"
                );
            }),
        )],
        false,
    )
    .await?;
    Ok(())
}

/// vim `{count}r{char}`: `3rx` replaces 3 characters and leaves the cursor on the
/// last replaced one (index 2). Guards the count-blind Helix `replace` that
/// replaced only the single block-cursor char.
#[tokio::test(flavor = "multi_thread")]
async fn count_r_replaces_count_chars() -> anyhow::Result<()> {
    use std::io::Write;
    let mut file = tempfile::NamedTempFile::new()?;
    write!(file, "abcdef\n")?;
    file.flush()?;
    let mut app = helpers::AppBuilder::new()
        .with_config(Config {
            keys: zemacs_term::keymap::vim::default(),
            ..Default::default()
        })
        .with_file(file.path(), None)
        .build()?;

    test_key_sequences(
        &mut app,
        vec![(
            Some("3rx"),
            Some(&|app| {
                let view = app.editor.tree.get(app.editor.tree.focus);
                let doc = app.editor.documents().next().unwrap();
                assert_eq!(
                    "xxxdef\n",
                    doc.text().to_string(),
                    "`3rx` replaces the first 3 chars with x"
                );
                let cursor = doc
                    .selection(view.id)
                    .primary()
                    .cursor(doc.text().slice(..));
                assert_eq!(2, cursor, "cursor rests on the last replaced char");
            }),
        )],
        false,
    )
    .await?;
    Ok(())
}

/// vim `r` aborts (no change) when the line has fewer characters than the count,
/// matching vim's bell-and-do-nothing.
#[tokio::test(flavor = "multi_thread")]
async fn count_r_aborts_when_line_too_short() -> anyhow::Result<()> {
    use std::io::Write;
    let mut file = tempfile::NamedTempFile::new()?;
    write!(file, "abc\n")?;
    file.flush()?;
    let mut app = helpers::AppBuilder::new()
        .with_config(Config {
            keys: zemacs_term::keymap::vim::default(),
            ..Default::default()
        })
        .with_file(file.path(), None)
        .build()?;

    test_key_sequences(
        &mut app,
        vec![(
            Some("5rx"),
            Some(&|app| {
                let doc = app.editor.documents().next().unwrap();
                assert_eq!(
                    "abc\n",
                    doc.text().to_string(),
                    "`5rx` on a 3-char line changes nothing"
                );
            }),
        )],
        false,
    )
    .await?;
    Ok(())
}

/// vim `{count}D`: `2D` deletes to end of line plus `count`-1 more lines
/// (count-aware `$`). Guards the count-blind `extend_to_line_end`.
#[tokio::test(flavor = "multi_thread")]
async fn count_d_capital_deletes_count_lines_to_eol() -> anyhow::Result<()> {
    use std::io::Write;
    let mut file = tempfile::NamedTempFile::new()?;
    write!(file, "l1\nl2\nl3\nl4\n")?;
    file.flush()?;
    let mut app = helpers::AppBuilder::new()
        .with_config(Config {
            keys: zemacs_term::keymap::vim::default(),
            ..Default::default()
        })
        .with_file(file.path(), None)
        .build()?;

    test_key_sequences(
        &mut app,
        vec![(
            Some("2D"),
            Some(&|app| {
                let doc = app.editor.documents().next().unwrap();
                assert_eq!(
                    "\nl3\nl4\n",
                    doc.text().to_string(),
                    "`2D` deletes l1 through end of l2"
                );
            }),
        )],
        false,
    )
    .await?;
    Ok(())
}

/// Spacemacs auto-fill (SPC t F): typing past text_width wraps the line at the
/// last whitespace.
#[tokio::test(flavor = "multi_thread")]
async fn auto_fill_wraps_at_text_width() -> anyhow::Result<()> {
    let mut cfg = Config {
        keys: zemacs_term::keymap::vim::default(),
        ..Default::default()
    };
    cfg.editor.text_width = 10; // narrow fill column so a short line triggers the wrap
    let mut app = helpers::AppBuilder::new().with_config(cfg).build()?;
    app.editor.auto_fill = true;

    // Type "aaa bbb ccc" (11 chars) in insert mode; the space-separated text
    // crosses column 10, so auto-fill breaks at the last space <= 10 (index 7).
    test_key_sequences(
        &mut app,
        vec![(
            Some("iaaa bbb ccc"),
            Some(&|app| {
                let doc = app.editor.documents().next().unwrap();
                assert_eq!(
                    "aaa bbb\nccc\n",
                    doc.text().to_string(),
                    "auto-fill wraps at the last space before col 10"
                );
            }),
        )],
        false,
    )
    .await?;
    Ok(())
}

// --- jumplist parity: which motions record a jump (vim `:help jump-motions`) ---
// Each test moves to a distinct origin with non-jump motions (`ll`), performs the
// jump motion, then `<C-o>` (jump_backward). If the motion recorded the jump the
// cursor returns to the origin; if it did not, `<C-o>` falls through to an older
// entry and lands elsewhere.

#[tokio::test(flavor = "multi_thread")]
async fn paragraph_motion_records_jump() -> anyhow::Result<()> {
    // `}` is a vim jump command: after `}`, `<C-o>` returns to the pre-jump 'c'.
    test_with_config(
        vim(),
        ("#[a|]#bcd\n\nefgh", "ll}<C-o>", "ab#[c|]#d\n\nefgh"),
    )
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn percent_records_jump() -> anyhow::Result<()> {
    // `%` (match bracket) is a vim jump command; `<C-o>` returns to the '('.
    test_with_config(vim(), ("#[x|]#y(abc)", "ll%<C-o>", "xy#[(|]#abc)")).await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn star_records_jump() -> anyhow::Result<()> {
    // `*` (search word under cursor) is a vim jump command; `<C-o>` returns to the
    // first "foo". Regression: `*`/`#`/`n`/`N` previously did not record a jump.
    test_with_config(vim(), ("#[f|]#oo bar foo", "*<C-o>", "#[f|]#oo bar foo")).await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn hash_search_records_jump() -> anyhow::Result<()> {
    // `#` (backward search of the word under cursor) shares the `n`/`N` search path,
    // itself a vim jump command; `<C-o>` returns to the last "foo".
    test_with_config(vim(), ("foo bar #[f|]#oo", "#<C-o>", "foo bar #[f|]#oo")).await?;
    Ok(())
}

/// vim `5@q`: a count typed before `@` replays the macro that many times.
/// Regression: the count was dropped because the register key (`q`) arrives in
/// a fresh key context whose count is `None`, so `5@q` replayed once, not five.
#[tokio::test(flavor = "multi_thread")]
async fn count_before_macro_replay_repeats() -> anyhow::Result<()> {
    use std::io::Write;
    let mut file = tempfile::NamedTempFile::new()?;
    write!(file, "0123456789")?;
    file.flush()?;
    let mut app = helpers::AppBuilder::new()
        .with_config(Config {
            keys: zemacs_term::keymap::vim::default(),
            ..Default::default()
        })
        .with_file(file.path(), None)
        .build()?;

    test_key_sequences(
        &mut app,
        vec![
            // Record register q = "x" (delete one char). Recording itself
            // deletes '0', leaving "123456789" with the cursor at '1'.
            (Some("qqxq"), None),
            (
                Some("5@q"),
                Some(&|app| {
                    let doc = app.editor.documents().next().unwrap();
                    assert_eq!(
                        "6789",
                        doc.text().to_string(),
                        "`5@q` must replay the delete-char macro five times"
                    );
                }),
            ),
        ],
        false,
    )
    .await?;
    Ok(())
}

/// vim `dd` on the last line of a file with no trailing newline removes the
/// preceding newline, so no empty line is left behind: "a\nb\nc" -> "a\nb".
/// Regression: it deleted only the line's own span, leaving "a\nb\n" (a
/// trailing empty line) that vim never produces.
#[tokio::test(flavor = "multi_thread")]
async fn dd_last_line_no_trailing_newline() -> anyhow::Result<()> {
    use std::io::Write;
    let mut file = tempfile::NamedTempFile::new()?;
    write!(file, "a\nb\nc")?; // no trailing newline
    file.flush()?;
    let mut app = helpers::AppBuilder::new()
        .with_config(Config {
            keys: zemacs_term::keymap::vim::default(),
            ..Default::default()
        })
        .with_file(file.path(), None)
        .build()?;

    test_key_sequences(
        &mut app,
        vec![(
            Some("Gdd"), // G to last line, dd deletes it
            Some(&|app| {
                let view = app.editor.tree.get(app.editor.tree.focus);
                let doc = app.editor.documents().next().unwrap();
                assert_eq!(
                    "a\nb",
                    doc.text().to_string(),
                    "`dd` on the final line (no trailing newline) must not leave an empty line"
                );
                // vim leaves the cursor on the first non-blank of the new last line.
                let pos = doc
                    .selection(view.id)
                    .primary()
                    .cursor(doc.text().slice(..));
                assert_eq!(2, pos, "cursor should land on 'b' (char 2)");
            }),
        )],
        false,
    )
    .await?;
    Ok(())
}

/// vim `dd` on the last line of a file WITH a trailing newline moves the cursor
/// UP to the new last line. In the rope model the trailing newline leaves a
/// phantom empty last line, so before the fix the cursor stayed at the deleted
/// line's position (the phantom line) instead of moving up to "b" like vim.
/// Repeating with `.` then keeps peeling off the last line, matching vim.
#[tokio::test(flavor = "multi_thread")]
async fn dd_last_line_trailing_newline_moves_cursor_up() -> anyhow::Result<()> {
    use std::io::Write;
    let mut file = tempfile::NamedTempFile::new()?;
    write!(file, "a\nb\nc\n")?; // WITH trailing newline
    file.flush()?;
    let mut app = helpers::AppBuilder::new()
        .with_config(Config {
            keys: zemacs_term::keymap::vim::default(),
            ..Default::default()
        })
        .with_file(file.path(), None)
        .build()?;

    test_key_sequences(
        &mut app,
        vec![
            (
                Some("Gdd"),
                Some(&|app| {
                    let view = app.editor.tree.get(app.editor.tree.focus);
                    let doc = app.editor.documents().next().unwrap();
                    assert_eq!(
                        "a\nb\n",
                        doc.text().to_string(),
                        "dd keeps trailing newline"
                    );
                    let pos = doc
                        .selection(view.id)
                        .primary()
                        .cursor(doc.text().slice(..));
                    assert_eq!(2, pos, "cursor moves up to 'b', not the phantom last line");
                }),
            ),
            (
                Some("."),
                Some(&|app| {
                    let view = app.editor.tree.get(app.editor.tree.focus);
                    let doc = app.editor.documents().next().unwrap();
                    assert_eq!(
                        "a\n",
                        doc.text().to_string(),
                        "`.` deletes the new last line"
                    );
                    let pos = doc
                        .selection(view.id)
                        .primary()
                        .cursor(doc.text().slice(..));
                    assert_eq!(0, pos, "cursor moves up to 'a'");
                }),
            ),
        ],
        false,
    )
    .await?;
    Ok(())
}

/// vim `dd` on the final line yanks the line *linewise*, so a following `p`
/// re-inserts it as a whole line below — even though that line had no trailing
/// newline. Regression: the last line yanked charwise (no newline), so `p`
/// pasted the text inline instead of opening a new line.
#[tokio::test(flavor = "multi_thread")]
async fn dd_last_line_yanks_linewise_for_paste() -> anyhow::Result<()> {
    use std::io::Write;
    let mut file = tempfile::NamedTempFile::new()?;
    write!(file, "a\nb\nc")?; // no trailing newline
    file.flush()?;
    let mut app = helpers::AppBuilder::new()
        .with_config(Config {
            keys: zemacs_term::keymap::vim::default(),
            ..Default::default()
        })
        .with_file(file.path(), None)
        .build()?;

    test_key_sequences(
        &mut app,
        vec![(
            Some("Gddp"), // delete last line "c", then paste it back linewise below "b"
            Some(&|app| {
                let doc = app.editor.documents().next().unwrap();
                assert_eq!(
                    "a\nb\nc",
                    doc.text().to_string(),
                    "`dd` then `p` must re-insert the line linewise (vim parity)"
                );
            }),
        )],
        false,
    )
    .await?;
    Ok(())
}

/// vim linewise paste-after (`p`) on the final line of a buffer with no
/// trailing newline lands on a NEW line below, not appended inline, and adds no
/// trailing empty line: `yy` on line 1, `G`, `p` -> "a\nb\nc\na". Regression:
/// the block was appended to the last line ("a\nb\nca\n").
#[tokio::test(flavor = "multi_thread")]
async fn linewise_paste_after_last_line_no_trailing_newline() -> anyhow::Result<()> {
    use std::io::Write;
    let mut file = tempfile::NamedTempFile::new()?;
    write!(file, "a\nb\nc")?; // no trailing newline
    file.flush()?;
    let mut app = helpers::AppBuilder::new()
        .with_config(Config {
            keys: zemacs_term::keymap::vim::default(),
            ..Default::default()
        })
        .with_file(file.path(), None)
        .build()?;

    test_key_sequences(
        &mut app,
        vec![(
            Some("yyGp"), // yank line "a" linewise, go to last line, paste after
            Some(&|app| {
                let doc = app.editor.documents().next().unwrap();
                assert_eq!(
                    "a\nb\nc\na",
                    doc.text().to_string(),
                    "linewise `p` after the last line must open a new line, not append inline"
                );
            }),
        )],
        false,
    )
    .await?;
    Ok(())
}

/// vim `dd` on a single-line file empties the buffer to one empty line.
#[tokio::test(flavor = "multi_thread")]
async fn dd_single_line_file_empties_buffer() -> anyhow::Result<()> {
    use std::io::Write;
    let mut file = tempfile::NamedTempFile::new()?;
    write!(file, "abc")?; // one line, no trailing newline
    file.flush()?;
    let mut app = helpers::AppBuilder::new()
        .with_config(Config {
            keys: zemacs_term::keymap::vim::default(),
            ..Default::default()
        })
        .with_file(file.path(), None)
        .build()?;

    test_key_sequences(
        &mut app,
        vec![(
            Some("dd"),
            Some(&|app| {
                let doc = app.editor.documents().next().unwrap();
                assert_eq!(
                    "",
                    doc.text().to_string(),
                    "`dd` on the only line leaves an empty buffer (no preceding newline to eat)"
                );
            }),
        )],
        false,
    )
    .await?;
    Ok(())
}

/// vim `cG`: linewise change from the current line to the last line. On l3 of a
/// five-line file it removes l3..l5 and drops into insert mode, leaving l1..l2.
/// Guards the missing change-submap `G` (was a fake port: `dG` existed, `cG` did
/// not fire the change operator at all).
#[tokio::test(flavor = "multi_thread")]
async fn c_capital_g_changes_to_last_line() -> anyhow::Result<()> {
    use std::io::Write;
    let mut file = tempfile::NamedTempFile::new()?;
    write!(file, "l1\nl2\nl3\nl4\nl5\n")?;
    file.flush()?;
    let mut app = helpers::AppBuilder::new()
        .with_config(Config {
            keys: zemacs_term::keymap::vim::default(),
            ..Default::default()
        })
        .with_file(file.path(), None)
        .build()?;

    test_key_sequences(
        &mut app,
        vec![(
            // 2j -> l3, then cG removes l3..l5 and enters insert.
            Some("2jcG"),
            Some(&|app| {
                let doc = app.editor.documents().next().unwrap();
                assert_eq!(
                    "l1\nl2\n\n",
                    doc.text().to_string(),
                    "`cG` on l3 changes l3..l5 away, leaving l1..l2 and an empty \
                     line to insert into (vim linewise change)"
                );
                assert_eq!(
                    zemacs_view::document::Mode::Insert,
                    app.editor.mode,
                    "`cG` drops into insert mode after removing the lines"
                );
            }),
        )],
        false,
    )
    .await?;
    Ok(())
}

/// vim `cgg`: linewise change from the current line up to the first line. On l3
/// it removes l1..l3 and enters insert, leaving l4..l5. Guards the missing
/// change-submap `gg` (parallel to `dgg`, which already existed).
#[tokio::test(flavor = "multi_thread")]
async fn c_gg_changes_to_first_line() -> anyhow::Result<()> {
    use std::io::Write;
    let mut file = tempfile::NamedTempFile::new()?;
    write!(file, "l1\nl2\nl3\nl4\nl5\n")?;
    file.flush()?;
    let mut app = helpers::AppBuilder::new()
        .with_config(Config {
            keys: zemacs_term::keymap::vim::default(),
            ..Default::default()
        })
        .with_file(file.path(), None)
        .build()?;

    test_key_sequences(
        &mut app,
        vec![(
            // 2j -> l3, then cgg removes l1..l3 and enters insert.
            Some("2jcgg"),
            Some(&|app| {
                let doc = app.editor.documents().next().unwrap();
                assert_eq!(
                    "\nl4\nl5\n",
                    doc.text().to_string(),
                    "`cgg` on l3 changes l1..l3 away, leaving an empty line to \
                     insert into above l4..l5 (vim linewise change)"
                );
                assert_eq!(
                    zemacs_view::document::Mode::Insert,
                    app.editor.mode,
                    "`cgg` drops into insert mode after removing the lines"
                );
            }),
        )],
        false,
    )
    .await?;
    Ok(())
}

/// Helper: build a vim-keymap app over `body` and run `keys`, asserting the
/// resulting buffer text. Keeps the operator-motion tests below terse.
async fn assert_after_keys(body: &str, keys: &str, expect: &str, msg: &str) -> anyhow::Result<()> {
    use std::io::Write;
    let mut file = tempfile::NamedTempFile::new()?;
    write!(file, "{body}")?;
    file.flush()?;
    let mut app = helpers::AppBuilder::new()
        .with_config(Config {
            keys: zemacs_term::keymap::vim::default(),
            ..Default::default()
        })
        .with_file(file.path(), None)
        .build()?;
    let expect = expect.to_string();
    let msg = msg.to_string();
    test_key_sequences(
        &mut app,
        vec![(
            Some(keys),
            Some(&move |app| {
                let doc = app.editor.documents().next().unwrap();
                assert_eq!(expect, doc.text().to_string(), "{msg}");
            }),
        )],
        false,
    )
    .await?;
    Ok(())
}

/// Regression: `cG` on (or moved to) the last line pushes a jumplist entry at
/// the old last-line caret, then deletes those lines — shrinking the buffer so
/// the stored jump caret is now past EOF. The marks gutter renders `view.jumps`
/// via `cursor(slice)`, which called `prev_grapheme_boundary` on the stale
/// position and panicked ropey inside the frame render (char index > len). The
/// buffer must survive `cG` from the last line and from a single-line file.
#[tokio::test(flavor = "multi_thread")]
async fn cg_from_last_line_no_gutter_panic() -> anyhow::Result<()> {
    // G moves to the last line (pushing a jump at the old caret), then cG
    // changes that line: the stale jump caret must not panic gutter render.
    assert_after_keys(
        "aaaaaaaaaa\nbbbbbbbbbb\ncccccccccc\ndddddddddd\neeeeeeeeee",
        "GcG",
        "aaaaaaaaaa\nbbbbbbbbbb\ncccccccccc\ndddddddddd\n",
        "`GcG` clears the last line, leaving an empty last line for insert",
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn cg_single_line_no_gutter_panic() -> anyhow::Result<()> {
    assert_after_keys(
        "aaaaaaaaaa",
        "cG",
        "",
        "`cG` on a one-line file clears it without panicking",
    )
    .await
}

/// vim `dj`: linewise, deletes the current line and the one below (2 lines).
#[tokio::test(flavor = "multi_thread")]
async fn dj_deletes_current_and_next_line() -> anyhow::Result<()> {
    assert_after_keys(
        "l1\nl2\nl3\nl4\n",
        "jdj",
        "l1\nl4\n",
        "`dj` on l2 removes l2+l3",
    )
    .await
}

/// vim `2dj`: count is faithful — deletes the current line plus 2 below (3
/// lines), NOT the doubled span a shared-count `extend_to_line_bounds` chain
/// would produce. This is the regression guard for the count fix.
#[tokio::test(flavor = "multi_thread")]
async fn count_dj_is_current_plus_count_lines() -> anyhow::Result<()> {
    assert_after_keys(
        "l1\nl2\nl3\nl4\nl5\nl6\n",
        "2dj",
        "l4\nl5\nl6\n",
        "`2dj` removes l1+l2+l3 (3 lines)",
    )
    .await
}

/// vim `dk`: linewise, deletes the current line and the one above.
#[tokio::test(flavor = "multi_thread")]
async fn dk_deletes_current_and_prev_line() -> anyhow::Result<()> {
    assert_after_keys(
        "l1\nl2\nl3\nl4\n",
        "2jdk",
        "l1\nl4\n",
        "`dk` on l3 removes l2+l3",
    )
    .await
}

/// vim `cj`: linewise change of two lines leaves one empty line for insert.
#[tokio::test(flavor = "multi_thread")]
async fn cj_changes_two_lines_leaving_empty() -> anyhow::Result<()> {
    assert_after_keys(
        "l1\nl2\nl3\nl4\n",
        "jcj",
        "l1\n\nl4\n",
        "`cj` on l2 changes l2+l3 to an empty line",
    )
    .await
}

/// vim `dl`: deletes the character under the cursor (charwise, like `x`).
#[tokio::test(flavor = "multi_thread")]
async fn dl_deletes_char_under_cursor() -> anyhow::Result<()> {
    assert_after_keys("abcdef\n", "dl", "bcdef\n", "`dl` deletes the 'a'").await
}

/// vim `3dl`: count is faithful for charwise motions — deletes 3 chars.
#[tokio::test(flavor = "multi_thread")]
async fn count_dl_deletes_count_chars() -> anyhow::Result<()> {
    assert_after_keys("abcdef\n", "3dl", "def\n", "`3dl` deletes 'abc'").await
}

/// vim `dh`: deletes the character to the left of the cursor.
#[tokio::test(flavor = "multi_thread")]
async fn dh_deletes_char_to_left() -> anyhow::Result<()> {
    assert_after_keys(
        "abcdef\n",
        "lldh",
        "acdef\n",
        "`dh` from col 2 deletes the 'b'",
    )
    .await
}

/// vim `c0`: change from the cursor back to the line start (parity fill — `d0`
/// existed but `c0` did not). `c0` must span the same text as the sibling `d0`:
/// build the operator behavior from the identical `extend_to_line_start` motion,
/// so the change and delete leave the same buffer (modulo insert mode).
#[tokio::test(flavor = "multi_thread")]
async fn c0_matches_d0_span() -> anyhow::Result<()> {
    use std::io::Write;
    let run = |keys: &'static str| async move {
        let mut file = tempfile::NamedTempFile::new()?;
        write!(file, "abcdef\n")?;
        file.flush()?;
        let mut app = helpers::AppBuilder::new()
            .with_config(Config {
                keys: zemacs_term::keymap::vim::default(),
                ..Default::default()
            })
            .with_file(file.path(), None)
            .build()?;
        let out = std::cell::RefCell::new(String::new());
        test_key_sequences(
            &mut app,
            vec![(
                Some(keys),
                Some(&|app| {
                    *out.borrow_mut() = app.editor.documents().next().unwrap().text().to_string();
                }),
            )],
            false,
        )
        .await?;
        anyhow::Ok(out.into_inner())
    };
    let d0 = run("llld0").await?;
    let c0 = run("lllc0").await?;
    assert_eq!(d0, c0, "`c0` must delete the same span as `d0`");
    Ok(())
}

/// vim `d}`: `}` is exclusive and promotes to linewise at column 0, so it deletes
/// the paragraph's lines but leaves the blank separator (matching real vim).
#[tokio::test(flavor = "multi_thread")]
async fn d_paragraph_forward_deletes_paragraph() -> anyhow::Result<()> {
    assert_after_keys(
        "a\nb\n\nc\nd\n",
        "d}",
        "\nc\nd\n",
        "`d}` deletes the paragraph lines, keeping the blank separator",
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn d_paragraph_linewise_from_col0() -> anyhow::Result<()> {
    // vim d} at col 0: whole lines up to (not incl) the blank separator.
    test_with_config(vim(), ("#[f|]#oo\nbar\n\nbaz\n", "d}", "#[\n|]#\nbaz\n")).await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn d_paragraph_charwise_from_midline() -> anyhow::Result<()> {
    // vim d} from mid-line (not at first non-blank): charwise to the blank line.
    test_with_config(vim(), ("f#[o|]#o\nbar\n\nbaz\n", "d}", "f#[\n|]#\nbaz\n")).await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn dvj_forces_charwise_down() -> anyhow::Result<()> {
    // vim `dvj`: force charwise on the normally-linewise `j` — delete from the
    // cursor to the same column on the next line (inclusive).
    test_with_config(vim(), ("ab#[c|]#d\nefgh\n", "dvj", "ab#[h|]#\n")).await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn dvk_forces_charwise_up() -> anyhow::Result<()> {
    // vim `dvk`: force charwise on `k` — delete from the same column on the line
    // above through the cursor.
    test_with_config(vim(), ("abcd\nef#[g|]#h\n", "dvk", "ab#[h|]#\n")).await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn block_append_pads_short_rows() -> anyhow::Result<()> {
    // 2-wide block over 3 rows (cols 0-1); `A` appends after col 1 on every row,
    // padding the short middle row "a" with a space so its append lands correctly.
    let mut app = vim().with_input_text("#[a|]#bc\na\nabc").build()?;
    test_key_sequence(
        &mut app,
        Some("<C-v>jjlAX<esc>"),
        Some(&|app| {
            let (_, doc) = zemacs_view::current_ref!(app.editor);
            assert_eq!(doc.text().to_string(), "abXc\na X\nabXc");
        }),
        false,
    )
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn block_insert_at_left_column() -> anyhow::Result<()> {
    // Build the block then move right; `I` inserts at the LEFT column (col 0) on
    // every row, not the active (right) cursor column.
    let mut app = vim().with_input_text("#[a|]#bc\nxyz\npqr").build()?;
    test_key_sequence(
        &mut app,
        Some("<C-v>jjlIZ<esc>"),
        Some(&|app| {
            let (_, doc) = zemacs_view::current_ref!(app.editor);
            assert_eq!(doc.text().to_string(), "Zabc\nZxyz\nZpqr");
        }),
        false,
    )
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn dV_paragraph_forces_linewise_from_midline() -> anyhow::Result<()> {
    // From mid-line `d}` is charwise, but `dV}` forces whole-line deletion of the
    // paragraph, stopping before the blank separator (like `d}` at column 0).
    let mut app = vim().with_input_text("f#[o|]#o\nbar\n\nbaz\n").build()?;
    test_key_sequence(
        &mut app,
        Some("dV}"),
        Some(&|app| {
            let (_, doc) = zemacs_view::current_ref!(app.editor);
            assert_eq!(doc.text().to_string(), "\nbaz\n");
        }),
        false,
    )
    .await?;
    Ok(())
}
