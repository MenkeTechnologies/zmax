use super::*;

use zmax_term::config::Config;

// vim keymap (the harness default is the selection-first keymap).
fn vim() -> AppBuilder {
    AppBuilder::new().with_config(Config {
        keys: zmax_term::keymap::vim::default(),
        ..Default::default()
    })
}

/// vim `u` after a charwise change puts the cursor on the first character of the
/// restored text, not on the first non-blank of the line: `u_undoredo` restores
/// the column saved before the change (`uh_cursor.col`) whenever the undo lands
/// on the line that cursor was on (undo.c: `curwin->w_cursor.col =
/// curhead->uh_cursor.col`). Only when the undo lands on some *other* line does
/// vim fall back to `beginline(BL_SOL | BL_FIX)`.
///
/// Ground truth, vim 9.2, `foo "bar" baz` with the cursor on `a` (col 7):
/// `ci"XY<Esc>u` leaves the cursor at 1:6 — the `b` of the restored `bar`.
#[tokio::test(flavor = "multi_thread")]
async fn undo_of_change_inside_puts_cursor_at_start_of_restored_text() -> anyhow::Result<()> {
    test_with_config(
        vim(),
        (
            "foo \"b#[a|]#r\" baz",
            "ci\"XY<esc>u",
            "foo \"#[b|]#ar\" baz",
        ),
    )
    .await?;
    Ok(())
}

/// Same rule on an indented line: the cursor goes to the change, not to the
/// line's first non-blank. vim 9.2, `    indented "bar" tail` with the cursor at
/// col 16: `ci"XY<Esc>u` leaves it at 1:15 (the `b`), not at col 5 (`i`).
#[tokio::test(flavor = "multi_thread")]
async fn undo_on_indented_line_does_not_jump_to_first_non_blank() -> anyhow::Result<()> {
    test_with_config(
        vim(),
        (
            "    indented \"b#[a|]#r\" tail",
            "ci\"XY<esc>u",
            "    indented \"#[b|]#ar\" tail",
        ),
    )
    .await?;
    Ok(())
}

/// `cw` is the same case as `ci"` — the restored word's first char. vim 9.2,
/// `foo bar baz` with the cursor on `b` (col 5): `cwXY<Esc>u` -> 1:5.
#[tokio::test(flavor = "multi_thread")]
async fn undo_of_cw_puts_cursor_at_word_start() -> anyhow::Result<()> {
    test_with_config(
        vim(),
        ("foo #[b|]#ar baz", "cwXY<esc>u", "foo #[b|]#ar baz"),
    )
    .await?;
    Ok(())
}

/// A charwise delete restores the saved column too. vim 9.2,
/// `        bbbbbbbb` with the cursor at col 12: `x u` -> 1:12.
///
/// The leading `i<esc>` is an undo boundary, not part of the case: the harness
/// installs the input text with a plain `Document::apply`, so without it the one
/// `u` reverts that setup edit along with the `x` and empties the buffer. `i<esc>`
/// commits the pending setup change and steps the cursor back one column, hence
/// the input caret one to the right of the character `x` removes.
#[tokio::test(flavor = "multi_thread")]
async fn undo_of_x_puts_cursor_back_on_the_deleted_char() -> anyhow::Result<()> {
    test_with_config(
        vim(),
        (
            "        bbbb#[b|]#bbb",
            "i<esc>xu",
            "        bbb#[b|]#bbbb",
        ),
    )
    .await?;
    Ok(())
}

/// The linewise case keeps the first-non-blank landing. A linewise operator
/// saves a cursor that never sits past the line's indent, so undoing `dd` from a
/// column inside the text lands on the first non-blank (vim 9.2, line indented
/// with 8 spaces: `2G12|ddu` -> 2:9), not on the line ending where the raw
/// inverse transaction would leave it.
#[tokio::test(flavor = "multi_thread")]
async fn undo_of_dd_puts_cursor_on_first_non_blank() -> anyhow::Result<()> {
    test_with_config(
        vim(),
        (
            "aaa\n        bbb#[b|]#bbbb\nccc\n",
            "ddu",
            "aaa\n        #[b|]#bbbbbbb\nccc\n",
        ),
    )
    .await?;
    Ok(())
}
