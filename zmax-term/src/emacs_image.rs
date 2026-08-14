//! The parts of GNU Emacs's image surface that are not display transforms:
//! marking the visited image in Dired (`image-mode-mark-file` /
//! `image-mode-unmark-file`), the `thumbs-mode` contact sheet, and `yank-media`
//! (pasting an image off the system clipboard into the buffer).
//!
//! Everything here is process-level state plus external-tool plumbing; the
//! `commands/typed.rs` dispatchers do the editor work. Splitting it this way
//! keeps the shell-outs (ImageMagick `montage`, `pngpaste`/`osascript`,
//! `wl-paste`/`xclip`) in one testable place, the way `emacs_rect` holds the
//! rectangle geometry for the rectangle commands.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{anyhow, bail};
use once_cell::sync::Lazy;
use zmax_core::command_line::Args;
use zmax_core::Transaction;
use zmax_view::editor::Editor;

use crate::compositor::{Compositor, Context};
use crate::ui::PromptEvent;

// ---------------------------------------------------------------------------
// image-mode-mark-file / image-mode-unmark-file
// ---------------------------------------------------------------------------

/// Marks set on files from outside a Dired listing, keyed by the containing
/// directory. Emacs `image-mode--mark-file` marks the file in every Dired buffer
/// visiting its directory and opens one when none exists; zmax's Dired is a
/// modal overlay rather than a background buffer, so the mark is recorded here
/// and a listing picks it up when it is opened (and a listing that is already up
/// is updated in place by the dispatcher).
static EXTERNAL_MARKS: Lazy<Mutex<HashMap<PathBuf, HashSet<String>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// The externally-set marks for `dir`, for [`crate::ui::dired::Dired::new`] to
/// seed a fresh listing with.
pub fn external_marks(dir: &Path) -> HashSet<String> {
    EXTERNAL_MARKS
        .lock()
        .unwrap()
        .get(dir)
        .cloned()
        .unwrap_or_default()
}

/// Record (`mark` true) or clear (false) a mark on `dir/name`.
pub fn set_external_mark(dir: &Path, name: &str, mark: bool) {
    let mut table = EXTERNAL_MARKS.lock().unwrap();
    let entry = table.entry(dir.to_path_buf()).or_default();
    if mark {
        entry.insert(name.to_string());
    } else {
        entry.remove(name);
    }
    if entry.is_empty() {
        table.remove(dir);
    }
}

// ---------------------------------------------------------------------------
// thumbs-mode
// ---------------------------------------------------------------------------

/// The image files in `dir`, sorted by name — the set `thumbs-mode` builds its
/// contact sheet from.
pub fn images_in_dir(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.is_file() && crate::commands::is_image_path(p))
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

/// `thumbs-thumbsize` (default 100): the side of one thumbnail in pixels.
pub const THUMB_SIZE: u32 = 100;

/// Build the contact sheet `thumbs-mode` shows: every image scaled into a
/// `THUMB_SIZE` cell, laid out `cols` per row and labelled with its file name.
/// ImageMagick 7 spells the tool `magick montage`, 6 ships a `montage` binary.
pub fn build_contact_sheet(images: &[PathBuf], cols: usize, out: &Path) -> Result<(), String> {
    if images.is_empty() {
        return Err("thumbs: no image files here".into());
    }
    let mut args: Vec<String> = vec![
        "-label".into(),
        "%f".into(),
        "-tile".into(),
        format!("{cols}x"),
        "-geometry".into(),
        format!("{THUMB_SIZE}x{THUMB_SIZE}+4+4"),
    ];
    args.extend(images.iter().map(|p| p.to_string_lossy().into_owned()));
    args.push(out.to_string_lossy().into_owned());

    let mut last = String::new();
    for argv in [&["montage"][..], &["magick", "montage"][..]] {
        match std::process::Command::new(argv[0])
            .args(&argv[1..])
            .args(&args)
            .output()
        {
            Ok(o) if o.status.success() => return Ok(()),
            Ok(o) => last = String::from_utf8_lossy(&o.stderr).trim().to_string(),
            // Not installed under that name; try the next spelling.
            Err(_) => continue,
        }
    }
    if last.is_empty() {
        Err("thumbs: `montage' not found (install ImageMagick)".into())
    } else {
        Err(format!("thumbs: montage: {last}"))
    }
}

// ---------------------------------------------------------------------------
// yank-media
// ---------------------------------------------------------------------------

/// Write the clipboard's `image/png` flavour to `dest`. Uses the platform's own
/// clipboard reader: `pngpaste` (or AppleScript's `«class PNGf»` when it is not
/// installed) on macOS, `wl-paste` under Wayland and `xclip` under X11.
/// Returns the number of bytes written.
pub fn clipboard_png(dest: &Path) -> Result<u64, String> {
    for (prog, args) in png_readers(dest) {
        let out = match std::process::Command::new(&prog).args(&args).output() {
            Ok(o) => o,
            // Reader not installed; try the next one.
            Err(_) => continue,
        };
        if !out.status.success() {
            continue;
        }
        // `pngpaste FILE` and the AppleScript writer produce the file
        // themselves; the X11/Wayland readers write the bytes to stdout.
        if !out.stdout.is_empty() {
            std::fs::write(dest, &out.stdout).map_err(|e| format!("yank-media: {e}"))?;
        }
        let size = std::fs::metadata(dest).map(|m| m.len()).unwrap_or(0);
        if size > 0 {
            return Ok(size);
        }
    }
    Err("yank-media: no image on the clipboard".into())
}

/// The clipboard readers to try, in order, for the running platform.
fn png_readers(dest: &Path) -> Vec<(String, Vec<String>)> {
    let d = dest.to_string_lossy().into_owned();
    if cfg!(target_os = "macos") {
        vec![
            ("pngpaste".into(), vec![d.clone()]),
            (
                "osascript".into(),
                vec![
                    "-e".into(),
                    format!("set p to POSIX file \"{d}\""),
                    "-e".into(),
                    "set d to (the clipboard as «class PNGf»)".into(),
                    "-e".into(),
                    "set fh to open for access p with write permission".into(),
                    "-e".into(),
                    "set eof fh to 0".into(),
                    "-e".into(),
                    "write d to fh".into(),
                    "-e".into(),
                    "close access fh".into(),
                ],
            ),
        ]
    } else {
        vec![
            (
                "wl-paste".into(),
                vec!["--no-newline".into(), "--type".into(), "image/png".into()],
            ),
            (
                "xclip".into(),
                vec![
                    "-selection".into(),
                    "clipboard".into(),
                    "-t".into(),
                    "image/png".into(),
                    "-o".into(),
                ],
            ),
        ]
    }
}

/// The text `yank-media` inserts for a saved image, per the buffer's language —
/// zmax's stand-in for Emacs's `yank-media-handler` registry, which is what
/// decides how a mode represents pasted media. `None` means the language has no
/// registered handler.
pub fn media_link(language: Option<&str>, path: &str) -> Option<String> {
    Some(match language? {
        "markdown" | "markdown.inline" | "mdx" => format!("![]({path})"),
        "org" => format!("[[file:{path}]]"),
        "html" | "vue" | "svelte" | "astro" => format!("<img src=\"{path}\">"),
        "latex" | "tex" | "bibtex" => format!("\\includegraphics{{{path}}}"),
        "rst" => format!(".. image:: {path}"),
        "asciidoc" => format!("image::{path}[]"),
        "typst" => format!("#image(\"{path}\")"),
        _ => return None,
    })
}

/// A collision-free name for the image `yank-media` is about to save, derived
/// from the buffer's own name the way Emacs's org/markdown handlers derive
/// theirs: `<stem>-<n>.png` in `dir`, `n` the first free number.
pub fn media_file_name(dir: &Path, stem: &str) -> PathBuf {
    let stem = if stem.is_empty() { "yank" } else { stem };
    for n in 1..10_000 {
        let candidate = dir.join(format!("{stem}-{n}.png"));
        if !candidate.exists() {
            return candidate;
        }
    }
    dir.join(format!("{stem}.png"))
}

// ---------------------------------------------------------------------------
// Command dispatchers (`:image-save`, `:image-mode-mark-file`, `:thumbs-mode`,
// `:yank-media`, `:doc-view-set-slice-using-mouse`).
// ---------------------------------------------------------------------------

/// The current buffer's file, or Emacs's own error when there is none.
fn visited_file(cx: &Context) -> anyhow::Result<PathBuf> {
    doc!(cx.editor)
        .path()
        .map(|p| p.to_path_buf())
        .ok_or_else(|| anyhow!("Current buffer is not visiting a file"))
}

/// Emacs `image-mode--mark-file`: mark (or unmark) the visited file in the Dired
/// listing of its directory. A listing that is already on screen is updated in
/// place; otherwise the mark is remembered for when that directory is listed.
pub fn mark_visited_file(cx: &mut Context, mark: bool) -> anyhow::Result<()> {
    let file = visited_file(cx)?;
    let dir = file
        .parent()
        .map(|d| std::fs::canonicalize(d).unwrap_or_else(|_| d.to_path_buf()))
        .ok_or_else(|| anyhow!("image-mode: {} has no directory", file.display()))?;
    let name = file
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or_else(|| anyhow!("image-mode: {} has no file name", file.display()))?;
    set_external_mark(&dir, &name, mark);

    let (cb_dir, cb_name) = (dir.clone(), name.clone());
    let call: crate::job::Callback = crate::job::Callback::EditorCompositor(Box::new(
        move |_editor: &mut Editor, compositor: &mut Compositor| {
            if let Some(dired) = compositor.find::<crate::ui::dired::Dired>() {
                dired.set_external_mark(&cb_dir, &cb_name, mark);
            }
        },
    ));
    cx.jobs.callback(async move { Ok(call) });

    let verb = if mark { "marked" } else { "unmarked" };
    cx.editor
        .set_status(format!("{name} {verb} in {}", dir.display()));
    Ok(())
}

/// `:image-mode-mark-file` — the typable behind Image mode's `m`.
pub fn ex_image_mode_mark_file(
    cx: &mut Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    mark_visited_file(cx, true)
}

/// `:image-mode-unmark-file` — the typable behind Image mode's `u`.
pub fn ex_image_mode_unmark_file(
    cx: &mut Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    mark_visited_file(cx, false)
}

/// `:image-save <file>` — Emacs `image-save`, which writes the image's *data*
/// (the file's bytes, or a pending `image-crop`/`image-cut` result) to a file you
/// name. Rotation and scale are display state and deliberately not applied.
pub fn ex_image_save(cx: &mut Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let dest = args.join(" ");
    if dest.trim().is_empty() {
        bail!("usage: :image-save <file>");
    }
    crate::ui::image::image_save(cx, &dest)
}

/// `thumbs-per-line` (default 4): thumbnails per row of the contact sheet.
const THUMBS_PER_LINE: usize = 4;

/// `:thumbs-mode [dir]` — Emacs `thumbs-mode`: show a directory's images as a
/// grid of labelled thumbnails. With no argument the current buffer's directory
/// is used, which is what `M-x thumbs` defaults to.
pub fn ex_thumbs_mode(cx: &mut Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let arg = args.join(" ");
    let dir = if arg.trim().is_empty() {
        doc!(cx.editor)
            .path()
            .and_then(|p| p.parent())
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| std::path::PathBuf::from("."))
    } else {
        zmax_stdx::path::expand_tilde(Path::new(arg.trim())).into_owned()
    };
    let images = images_in_dir(&dir);
    if images.is_empty() {
        bail!("thumbs: no image files in {}", dir.display());
    }
    let sheet = std::env::temp_dir().join(format!("zmax-thumbs-{}.png", std::process::id()));
    build_contact_sheet(&images, THUMBS_PER_LINE, &sheet).map_err(|e| anyhow!("{e}"))?;
    crate::commands::display_images_in_terminal(cx.editor, &[sheet], 0, false, false, 100);
    cx.editor.set_status(format!(
        "thumbs: {} image{} in {}",
        images.len(),
        if images.len() == 1 { "" } else { "s" },
        dir.display()
    ));
    Ok(())
}

/// `:yank-media` — Emacs `yank-media`: take the image on the system clipboard,
/// save it next to the buffer's file, and insert the reference the buffer's mode
/// uses for media (Emacs's `yank-media-handler` registry; [`media_link`] is
/// zmax's). Languages with no registered handler get the saved file's path.
pub fn ex_yank_media(cx: &mut Context, _args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (dir, stem, language) = {
        let doc = doc!(cx.editor);
        let path = doc.path().map(|p| p.to_path_buf());
        let dir = path
            .as_ref()
            .and_then(|p| p.parent())
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
        let stem = path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "yank".to_string());
        (dir, stem, doc.language_name().map(|s| s.to_string()))
    };
    let dest = media_file_name(&dir, &stem);
    let bytes = clipboard_png(&dest).map_err(|e| anyhow!("{e}"))?;

    // A relative reference keeps the buffer portable, the way Emacs's org and
    // markdown handlers insert one relative to the file.
    let shown = dest
        .strip_prefix(&dir)
        .unwrap_or(&dest)
        .to_string_lossy()
        .into_owned();
    let (text, note) = match media_link(language.as_deref(), &shown) {
        Some(link) => (link, String::new()),
        None => (
            shown.clone(),
            format!(
                " (no media handler for {}; inserted the path)",
                language.as_deref().unwrap_or("this buffer")
            ),
        ),
    };
    insert_at_cursors(cx, &text);
    cx.editor
        .set_status(format!("yank-media: saved {shown} ({bytes} bytes){note}"));
    Ok(())
}

/// Insert `text` at every cursor, replacing any selection.
fn insert_at_cursors(cx: &mut Context, text: &str) {
    let (view, doc) = current!(cx.editor);
    let selection = doc.selection(view.id).clone();
    let transaction = Transaction::change_by_selection(doc.text(), &selection, |range| {
        (range.from(), range.to(), Some(text.into()))
    });
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_link_covers_the_registered_modes_only() {
        assert_eq!(
            media_link(Some("markdown"), "a b.png").as_deref(),
            Some("![](a b.png)")
        );
        assert_eq!(
            media_link(Some("org"), "x.png").as_deref(),
            Some("[[file:x.png]]")
        );
        assert_eq!(
            media_link(Some("latex"), "x.png").as_deref(),
            Some("\\includegraphics{x.png}")
        );
        // No handler registered -> the caller falls back to the bare path.
        assert_eq!(media_link(Some("rust"), "x.png"), None);
        assert_eq!(media_link(None, "x.png"), None);
    }

    #[test]
    fn external_marks_round_trip_and_clear() {
        let dir = std::env::temp_dir().join("zmax-emacs-image-marks-test");
        set_external_mark(&dir, "a.png", true);
        set_external_mark(&dir, "b.png", true);
        assert_eq!(external_marks(&dir).len(), 2);
        set_external_mark(&dir, "a.png", false);
        assert_eq!(
            external_marks(&dir).into_iter().collect::<Vec<_>>(),
            vec!["b.png".to_string()]
        );
        // Clearing the last mark drops the directory's entry entirely.
        set_external_mark(&dir, "b.png", false);
        assert!(external_marks(&dir).is_empty());
    }

    #[test]
    fn media_file_name_skips_existing_files() {
        let dir = std::env::temp_dir().join("zmax-yank-media-test");
        let _ = std::fs::create_dir_all(&dir);
        let first = media_file_name(&dir, "note");
        assert_eq!(first.file_name().unwrap(), "note-1.png");
        std::fs::write(&first, b"x").unwrap();
        assert_eq!(
            media_file_name(&dir, "note").file_name().unwrap(),
            "note-2.png"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
