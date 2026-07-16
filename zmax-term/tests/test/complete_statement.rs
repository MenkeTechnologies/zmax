use super::*;

use zmax_term::config::Config;

// Spacemacs preset: JetBrains "Complete Current Statement" is bound to `C-c ;`
// and must fire in insert mode — `while(1|)` with the caret inside the parens
// gets a `{ }` block, not a self-inserted semicolon (`while(1;)`).
//
// This guards the chord routing itself. The reported bug only surfaced with an
// LSP completion popup open (stryke-lsp), where `Popup::handle_event` used to
// *consume* `C-c` — stranding the prefix so `;` self-inserted. The harness
// disables LSP so no popup opens here; that consume→propagate fix lives in
// `ui/popup.rs` and can't be driven by this harness (no synchronous
// `Popup<Menu>` trigger without a live language server).
fn spacemacs() -> AppBuilder {
    AppBuilder::new().with_config(Config {
        keys: zmax_term::keymap::default(),
        keymap: "spacemacs".to_string(),
        ..Default::default()
    })
}

fn buffer(app: &zmax_term::application::Application) -> String {
    let (_, doc) = zmax_view::current_ref!(app.editor);
    doc.text().to_string()
}

#[tokio::test(flavor = "multi_thread")]
async fn cc_semicolon_completes_while_header_with_braces() -> anyhow::Result<()> {
    // Caret on the `)`; `i` enters insert *before* it → between `1` and `)`.
    let mut app = spacemacs().with_input_text("while(1#[|)]#").build()?;
    test_key_sequence(
        &mut app,
        Some("i<C-c>;"),
        Some(&|app| {
            let got = buffer(app);
            assert!(
                !got.contains("while(1;)"),
                "C-c ; self-inserted a semicolon instead of completing: {got:?}"
            );
            assert!(
                got.contains('{') && got.contains('}'),
                "C-c ; should open a brace block: {got:?}"
            );
        }),
        false,
    )
    .await?;
    Ok(())
}
