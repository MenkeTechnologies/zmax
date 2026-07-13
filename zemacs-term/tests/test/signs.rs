use super::*;

use zemacs_term::application::Application;

/// End-to-end vim `:sign`: define a sign, place it on a line of a real file, and
/// drive a render (the sign column becomes visible, exercising the `signs` gutter
/// — a render panic would fail here). Then `:sign unplace *` clears it. One test,
/// sequential steps, since the sign store is process-global.
#[tokio::test(flavor = "multi_thread")]
async fn define_place_and_unplace_sign() -> anyhow::Result<()> {
    zemacs_view::signs::unplace_all();

    let file = tempfile::NamedTempFile::new()?;
    std::fs::write(file.path(), "line one\nline two\nline three\n")?;
    let path = file.path().to_path_buf();

    let mut app = AppBuilder::new().with_file(path.clone(), None).build()?;

    test_key_sequences(
        &mut app,
        vec![
            (
                Some(":sign define warn text=WW texthl=WarningMsg<ret>"),
                Some(&|app: &Application| {
                    assert!(
                        !app.editor.is_err(),
                        "sign define errored: {:?}",
                        app.editor.get_status()
                    );
                    assert!(zemacs_view::signs::is_defined("warn"));
                }),
            ),
            (
                Some(":sign place 1 line=2 name=warn<ret>"),
                Some(&{
                    let path = path.clone();
                    move |app: &Application| {
                        assert!(
                            !app.editor.is_err(),
                            "sign place errored: {:?}",
                            app.editor.get_status()
                        );
                        assert!(
                            zemacs_view::signs::has_signs(&path),
                            "sign not placed for {path:?}"
                        );
                        // Highest-priority sign resolves to the defined glyph on
                        // the 0-based line (vim line 2 -> line index 1).
                        let ls = zemacs_view::signs::line_signs(&path);
                        assert_eq!(
                            ls,
                            vec![(1, "WW".to_string(), Some("WarningMsg".to_string()))]
                        );
                    }
                }),
            ),
            (
                Some(":sign unplace *<ret>"),
                Some(&{
                    let path = path.clone();
                    move |app: &Application| {
                        assert!(!app.editor.is_err());
                        assert!(!zemacs_view::signs::has_signs(&path), "signs not cleared");
                    }
                }),
            ),
        ],
        false,
    )
    .await?;

    zemacs_view::signs::unplace_all();
    zemacs_view::signs::undefine("warn");
    Ok(())
}
