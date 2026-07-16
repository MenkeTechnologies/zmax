use std::io::Write as _;

use zmax_term::application::Application;
use zmax_term::config::Config;

use super::*;

/// Rewrite `path` from "another process" and bump its mtime so the editor sees
/// an external change. The short sleep keeps the new mtime strictly later even on
/// filesystems with coarse timestamp resolution.
fn external_write(path: &std::path::Path, content: &[u8]) {
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(path, content).unwrap();
}

/// Current buffer text for `path` (via path lookup, so it doesn't depend on the
/// focused view — safe to call outside a key-sequence callback).
fn buffer_text(app: &Application, path: &std::path::Path) -> String {
    app.editor
        .document_by_path(path)
        .expect("document open")
        .text()
        .to_string()
}

#[tokio::test(flavor = "multi_thread")]
async fn auto_reload_clean_buffer() -> anyhow::Result<()> {
    let mut file = tempfile::NamedTempFile::new()?;
    file.as_file_mut().write_all(b"original\n")?;
    file.as_file_mut().flush()?;
    let path = file.path().to_path_buf();

    let mut app = helpers::AppBuilder::new().with_file(&path, None).build()?;
    helpers::run_event_loop_until_idle(&mut app).await;

    external_write(&path, b"changed on disk\n");
    assert!(
        app.editor.auto_reload_file(&path),
        "a clean buffer should auto-reload after an external change"
    );
    assert_eq!(buffer_text(&app, &path), "changed on disk\n");
    assert!(!app.editor.is_err());
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn auto_reload_keeps_unsaved_edits() -> anyhow::Result<()> {
    let mut file = tempfile::NamedTempFile::new()?;
    file.as_file_mut().write_all(b"original\n")?;
    file.as_file_mut().flush()?;
    let path = file.path().to_path_buf();

    let mut app = helpers::AppBuilder::new().with_file(&path, None).build()?;
    helpers::run_event_loop_until_idle(&mut app).await;

    // Make an unsaved edit, then have the file change underneath us.
    test_key_sequence(&mut app, Some("iEDIT<esc>"), None, false).await?;
    let edited = buffer_text(&app, &path);
    assert!(app.editor.document_by_path(&path).unwrap().is_modified());

    external_write(&path, b"changed on disk\n");
    assert!(
        !app.editor.auto_reload_file(&path),
        "a modified buffer must not be clobbered by auto-reload"
    );
    assert_eq!(
        buffer_text(&app, &path),
        edited,
        "unsaved edits must be preserved"
    );
    assert!(app.editor.is_err(), "a conflict warning should be shown");
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn auto_reload_disabled_by_config() -> anyhow::Result<()> {
    let mut file = tempfile::NamedTempFile::new()?;
    file.as_file_mut().write_all(b"original\n")?;
    file.as_file_mut().flush()?;
    let path = file.path().to_path_buf();

    let mut config = Config::default();
    config.editor.auto_reload = false;
    let mut app = helpers::AppBuilder::new()
        .with_config(config)
        .with_file(&path, None)
        .build()?;
    helpers::run_event_loop_until_idle(&mut app).await;

    external_write(&path, b"changed on disk\n");
    assert!(
        !app.editor.auto_reload_file(&path),
        "auto-reload must be a no-op when the setting is off"
    );
    assert_eq!(buffer_text(&app, &path), "original\n");
    Ok(())
}
