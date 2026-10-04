//! Session parity with nvim: sessions written by nvim's `:mksession`
//! (`tests/fixtures/sessions/*.vim`) are loaded the way `zmax -S` loads them,
//! and the resulting editor state is compared with what nvim itself restores
//! from the same file (`*.nvim`, dumped by `fixtures/sessions/dump.vim`).
//! `fixtures/sessions/generate.sh` rebuilds both from nvim.
//!
//! Own test binary, one test: sessions `cd`, so they must not run in parallel
//! with each other or with other tests.
#![cfg(all(feature = "integration", feature = "scripting", unix))]

#[allow(dead_code, unused_imports, clippy::all)]
mod helpers {
    include!("test/helpers.rs");
}

use std::path::{Path, PathBuf};

use helpers::{test_config, test_syntax_loader};
use zmax_loader::workspace_trust::WorkspaceTrust;
use zmax_term::{application::Application, args::Args};
use zmax_view::tree::{Layout, TreeShape};
use zmax_view::Editor;

/// The workspace the fixture sessions were written in (see `generate.sh`).
fn write_workspace(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir.join("sub"))?;
    for name in ["a", "b", "c", "d"] {
        let text: String = (1..=200).map(|n| format!("{name} line {n}\n")).collect();
        std::fs::write(dir.join(format!("{name}.txt")), text)?;
    }
    let mut marked = String::new();
    for s in 1..=6 {
        marked.push_str(&format!("section {s} {{{{{{\n"));
        for n in 1..=8 {
            marked.push_str(&format!("  body {s}.{n}\n"));
        }
        marked.push_str("}}}\n");
    }
    std::fs::write(dir.join("m.txt"), marked)?;
    let sub: String = (1..=50).map(|n| format!("sub line {n}\n")).collect();
    std::fs::write(dir.join("sub/s.txt"), sub)
}

fn tail(path: Option<&Path>) -> String {
    path.and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// nvim's `winlayout()` rendering: `row(a,col(b,c))`.
fn layout(editor: &Editor, shape: &TreeShape) -> String {
    match shape {
        TreeShape::Leaf { doc, .. } => tail(editor.document(*doc).and_then(|d| d.path())),
        // zmax roots every tree in a container; vim's `winlayout()` does not wrap
        // a lone child.
        TreeShape::Split { children, .. } if children.len() == 1 => layout(editor, &children[0].1),
        TreeShape::Split { layout: kind, children } => {
            let name = match kind {
                Layout::Vertical => "row",
                Layout::Horizontal => "col",
            };
            let inner: Vec<String> = children.iter().map(|(_, c)| layout(editor, c)).collect();
            format!("{name}({})", inner.join(","))
        }
    }
}

/// The current tab's windows, in the format of `dump.vim`.
fn dump_tab(editor: &Editor, tab: usize, out: &mut Vec<String>) {
    // vim `getcwd(-1, {tab})`: the tab page's `:tcd` directory, else the global one.
    let tab_cwd = editor.tab_localdir.clone().unwrap_or_else(|| editor.global_cwd());
    out.push(format!(
        "TAB {tab} cwd {} layout {}",
        tail(Some(&tab_cwd)),
        layout(editor, &editor.tree.shape())
    ));
    for (n, (id, _)) in editor.tree.traverse().enumerate() {
        let view = editor.tree.get(id);
        let doc = editor.document(view.doc).unwrap();
        let text = doc.text().slice(..);
        let cursor = doc.selection(id).primary().cursor(text);
        let line = text.char_to_line(cursor);
        let col = cursor - text.line_to_char(line);
        let top = text.char_to_line(doc.view_offset(id).anchor);
        let folds = doc.folds();
        let mut closed = Vec::new();
        let mut l = 0;
        while l < text.len_lines() {
            match folds.closed_fold_starting_at(l).filter(|_| !folds.is_line_hidden(l)) {
                Some(f) => {
                    closed.push(format!("{}-{}", f.start + 1, f.end + 1));
                    l = f.end + 1;
                }
                None => l += 1,
            }
        }
        out.push(format!(
            "  win {} {} w={} h={} cur={}:{} top={} folds={} cwd={}",
            n + 1,
            tail(doc.path()),
            view.area.width,
            view.inner_height(),
            line + 1,
            col + 1,
            top + 1,
            closed.join(","),
            tail(Some(&editor.window_cwd(id))),
        ));
    }
}

fn dump(editor: &mut Editor) -> Vec<String> {
    let tabs = editor.tabs.len().max(1);
    let current = editor.current_tab;
    let win = editor
        .tree
        .traverse()
        .position(|(id, _)| id == editor.tree.focus)
        .map_or(0, |n| n + 1);
    // `:args`, each name as its file name, as dump.vim writes it.
    let args: Vec<String> = zmax_term::commands::arglist_display()
        .split_whitespace()
        .map(|word| {
            let name = word.trim_start_matches('[').trim_end_matches(']');
            let name = tail(Some(Path::new(name)));
            if word.starts_with('[') {
                format!("[{name}]")
            } else {
                name
            }
        })
        .collect();
    let mut out = vec![format!(
        "tab {}/{tabs} win {win} args {}",
        current + 1,
        args.join(" ")
    )];
    for t in 0..tabs {
        if tabs > 1 {
            editor.switch_tab(t);
        }
        dump_tab(editor, t + 1, &mut out);
    }
    if tabs > 1 {
        editor.switch_tab(current);
    }
    out
}

#[tokio::test(flavor = "multi_thread")]
async fn sessions_restore_as_nvim_restores_them() -> anyhow::Result<()> {
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sessions");
    let fixtures = std::env::var_os("ZMAX_SESSION_FIXTURES").map_or(fixtures, PathBuf::from);
    let root = std::env::temp_dir().join(format!("zmax-session-parity-{}", std::process::id()));
    let mut names: Vec<_> = std::fs::read_dir(&fixtures)?
        .filter_map(|e| e.ok()?.path().file_stem().map(|s| s.to_string_lossy().into_owned()))
        .filter(|n| fixtures.join(format!("{n}.vim")).is_file() && n != "dump" && n != "layouts")
        .collect();
    names.sort();
    names.dedup();
    let mut failures = Vec::new();
    for name in names {
        let dir = root.join(&name).join("w");
        let _ = std::fs::remove_dir_all(&dir);
        write_workspace(&dir)?;
        std::env::set_current_dir(&dir)?;
        let session = std::fs::read_to_string(fixtures.join(format!("{name}.vim")))?
            .replace("@DIR@", &dir.to_string_lossy());
        let script = root.join(&name).join("sess.vim");
        std::fs::write(&script, session)?;

        let mut args = Args::default();
        args.source_files.push(script);
        let source_files = args.source_files.clone();
        // A session's `normal!` keys are vim keys: run under the vim preset.
        let mut config = test_config();
        config.keymap = "vim".to_string();
        config.keys = zmax_term::keymap::preset("vim").expect("the vim preset");
        let mut app = Application::new(
            args,
            config,
            test_syntax_loader(None),
            WorkspaceTrust::fully_trusted(),
        )?;
        app.load_init_scripts();
        app.source_startup_files(&source_files);
        if app.editor.is_err() {
            failures.push(format!("{name}: {:?}", app.editor.get_status()));
        }
        let got = dump(&mut app.editor);
        let want = std::fs::read_to_string(fixtures.join(format!("{name}.nvim")))?;
        let want: Vec<&str> = want.lines().collect();
        if got != want {
            failures.push(format!(
                "{name}:\n  nvim:\n    {}\n  zmax:\n    {}",
                want.join("\n    "),
                got.join("\n    ")
            ));
        }
    }
    let _ = std::fs::remove_dir_all(&root);
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
    Ok(())
}
