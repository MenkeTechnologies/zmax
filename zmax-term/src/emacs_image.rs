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

/// `thumbs-per-line` (default 4): thumbnails per row of the contact sheet.
pub const THUMBS_PER_LINE: usize = 4;

/// The index into the contact sheet's file list of the thumbnail in cell
/// (`row`, `col`) of a `cols`-wide sheet, or `None` when that cell is past the
/// last image. `montage -tile <cols>x` fills the sheet row-major in the order it
/// was given the files, so the arithmetic is the inverse of the layout: emacs
/// `thumbs-do-thumbs-insertion` inserts the thumbnails in list order and breaks
/// the line every `thumbs-per-line`, which is the same mapping.
pub fn thumb_index(row: usize, col: usize, cols: usize, len: usize) -> Option<usize> {
    if cols == 0 || col >= cols {
        return None;
    }
    let index = row.checked_mul(cols)?.checked_add(col)?;
    (index < len).then_some(index)
}

/// The cell holding image `index` — the inverse of [`thumb_index`], for placing
/// a cursor on the sheet.
pub fn thumb_cell(index: usize, cols: usize) -> (usize, usize) {
    if cols == 0 {
        return (0, index);
    }
    (index / cols, index % cols)
}

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

/// Everything a `thumbs-mode` buffer holds: the files behind the contact sheet,
/// the sheet itself, the grid geometry, point (`thumbs-current-image` is just
/// the image at point) and `thumbs-marked-list`. Emacs keeps this in buffer-local
/// variables over a buffer of thumbnail characters; zmax has no such buffer, so
/// the grid is described by [`thumb_index`] and the state lives here, ready for a
/// picker component to drive.
#[derive(Clone, Debug)]
pub struct ThumbsPicker {
    /// The directory the sheet was built from.
    pub dir: PathBuf,
    /// The images, in the order `montage` laid them out.
    pub images: Vec<PathBuf>,
    /// `thumbs-per-line` for this sheet.
    pub cols: usize,
    /// The rendered contact sheet.
    pub sheet: PathBuf,
    index: usize,
    marked: HashSet<PathBuf>,
}

impl ThumbsPicker {
    /// Emacs `thumbs-show-from-dir`: list `dir`'s images and render the sheet.
    /// Errors are the ones a caller reports verbatim (`thumbs: …`).
    pub fn new(dir: &Path) -> Result<Self, String> {
        let images = images_in_dir(dir);
        if images.is_empty() {
            return Err(format!("thumbs: no image files in {}", dir.display()));
        }
        let sheet = std::env::temp_dir().join(format!("zmax-thumbs-{}.png", std::process::id()));
        build_contact_sheet(&images, THUMBS_PER_LINE, &sheet)?;
        Ok(Self::from_images(dir, images, sheet))
    }

    /// Emacs `thumbs-show-thumbs-list`: the same picker over a list somebody else
    /// chose — the Dired entry points (`thumbs-dired-show-marked`) come in here.
    /// The sheet is the caller's; `montage` is not run again.
    pub fn from_images(dir: &Path, images: Vec<PathBuf>, sheet: PathBuf) -> Self {
        Self {
            dir: dir.to_path_buf(),
            images,
            cols: THUMBS_PER_LINE,
            sheet,
            index: 0,
            marked: HashSet::new(),
        }
    }

    /// The number of rows the sheet occupies.
    pub fn rows(&self) -> usize {
        self.images.len().div_ceil(self.cols.max(1))
    }

    /// Point, as an index into [`Self::images`].
    pub fn index(&self) -> usize {
        self.index
    }

    /// `thumbs-current-image`: "the name of the image file name at point".
    pub fn current(&self) -> Option<&Path> {
        self.images.get(self.index).map(PathBuf::as_path)
    }

    /// The image in cell (`row`, `col`) — what a mouse click resolves to, the way
    /// `thumbs-mouse-find-image` sets point from the event and then reads
    /// `thumbs-current-image`. `None` for a cell past the last thumbnail.
    pub fn at_cell(&self, row: usize, col: usize) -> Option<&Path> {
        thumb_index(row, col, self.cols, self.images.len())
            .and_then(|i| self.images.get(i).map(PathBuf::as_path))
    }

    /// Move point to cell (`row`, `col`), reporting whether the cell holds a
    /// thumbnail; an empty cell leaves point where it was.
    pub fn select_cell(&mut self, row: usize, col: usize) -> bool {
        match thumb_index(row, col, self.cols, self.images.len()) {
            Some(i) => {
                self.index = i;
                true
            }
            None => false,
        }
    }

    /// The cell point is on.
    pub fn cursor_cell(&self) -> (usize, usize) {
        thumb_cell(self.index, self.cols)
    }

    /// `thumbs-forward-char` / `thumbs-backward-char`: "Move forward one image" —
    /// a `forward-char` that skips the newlines between rows, so it is a step of
    /// one through the file list and stops at either end.
    pub fn forward_char(&mut self, back: bool) {
        if back {
            self.index = self.index.saturating_sub(1);
        } else if self.index + 1 < self.images.len() {
            self.index += 1;
        }
    }

    /// `thumbs-forward-line` / `thumbs-backward-line`: a bare `forward-line`,
    /// which lands at the *start* of the next or previous row rather than keeping
    /// the column. Faithful to thumbs.el; a row that does not exist is no move.
    pub fn forward_line(&mut self, back: bool) -> bool {
        let (row, _) = self.cursor_cell();
        let target = if back {
            match row.checked_sub(1) {
                Some(r) => r,
                None => return false,
            }
        } else {
            row + 1
        };
        self.select_cell(target, 0)
    }

    /// `thumbs-mark` / `thumbs-unmark`: add or drop the image at point from
    /// `thumbs-marked-list`. Both signal "No image here" on an empty cell, which
    /// is what the `None` return stands for.
    pub fn mark(&mut self, mark: bool) -> Option<&Path> {
        let image = self.images.get(self.index)?.clone();
        if mark {
            self.marked.insert(image);
        } else {
            self.marked.remove(&image);
        }
        self.current()
    }

    /// Whether `image` is in `thumbs-marked-list` — the sheet draws those cells
    /// with `thumbs-relief` inverted.
    pub fn is_marked(&self, image: &Path) -> bool {
        self.marked.contains(image)
    }

    /// `thumbs-marked-list`, in sheet order — the set `thumbs-delete-images` and
    /// `thumbs-rename-images` act on when it is non-empty.
    pub fn marked_files(&self) -> Vec<PathBuf> {
        self.images
            .iter()
            .filter(|p| self.marked.contains(*p))
            .cloned()
            .collect()
    }
}

// ---------------------------------------------------------------------------
// yank-media
// ---------------------------------------------------------------------------

/// `yank-media-preferred-types`, "List of MIME types in the order of
/// preference": the LibreOffice TSV first ("Check first since LibreOffice also
/// puts a PNG image in the clipboard when a table cell is copied"), then PNG
/// ("Give PNG more priority"), then JPEG, and `text/html` last. Emacs's
/// `x-special/*-copied-files` entry is a function of the window system rather
/// than a type, so it has no place in a plain list. Nothing here is handled
/// until a language claims it in [`handled_types`], so the order can stay
/// emacs's whether or not zmax has a handler for a given entry yet.
const PREFERRED_TYPES: &[&str] = &[
    "application/x-libreoffice-tsvc",
    "image/png",
    "image/jpeg",
    "text/html",
];

/// The MIME types zmax has a `yank-media` handler for in a buffer of `language`
/// — the buffer half of emacs's `yank-media--registered-handlers`, which is what
/// decides whether `yank-media` has anything to do at all.
///
/// Emacs's org and Message handlers register the regexp `image/.*` and filter it
/// down to the image types Emacs can actually display
/// (`yank-media--find-matching-media`); the equivalent filter here is
/// [`media_link`], since an image is saved and referenced through it. `text/html`
/// is additionally handled where raw HTML is valid in the buffer, which is the
/// type emacs's list falls back to.
pub fn handled_types(language: Option<&str>) -> Vec<&'static str> {
    let mut out = Vec::new();
    if media_link(language, "x").is_some() {
        out.extend_from_slice(&[
            "image/png",
            "image/jpeg",
            "image/gif",
            "image/tiff",
            "image/webp",
        ]);
    }
    if html_is_literal(language) {
        out.push("text/html");
    }
    out
}

/// Whether HTML pasted into a buffer of this language is inserted as-is. Emacs
/// leaves `text/html` to a mode that can take it; the markup languages that
/// embed HTML verbatim are the ones that can.
fn html_is_literal(language: Option<&str>) -> bool {
    matches!(
        language,
        Some(
            "html"
                | "vue"
                | "svelte"
                | "astro"
                | "markdown"
                | "markdown.inline"
                | "mdx"
                | "org"
                | "xml"
        )
    )
}

/// `yank-media-autoselect-function`: of the types on the clipboard, the first
/// one in `yank-media-preferred-types` that the buffer also has a handler for.
///
/// Emacs stops there and tells you to retry with a prefix argument when nothing
/// preferred matched ("No preferred MIME type to yank"); the prefix argument then
/// offers the rest through `completing-read`. `:yank-media` is a typable with no
/// prefix argument, so the fallback is the first handled type in the order the
/// platform reported it — the same set that dialog would have offered.
pub fn preferred_type(available: &[String], language: Option<&str>) -> Option<String> {
    let handled = handled_types(language);
    let is_handled = |t: &str| handled.iter().any(|h| h.eq_ignore_ascii_case(t));
    for want in PREFERRED_TYPES {
        if is_handled(want) {
            if let Some(found) = available.iter().find(|t| t.eq_ignore_ascii_case(want)) {
                return Some(found.clone());
            }
        }
    }
    available.iter().find(|t| is_handled(t)).cloned()
}

/// The MIME types on the system clipboard — emacs's
/// `(gui-get-selection 'CLIPBOARD 'TARGETS)`. macOS has no MIME clipboard, so
/// AppleScript's `clipboard info` is read and its classes translated; Wayland and
/// X11 report MIME types directly.
pub fn clipboard_types() -> Vec<String> {
    for (prog, args) in type_listers() {
        let out = match std::process::Command::new(&prog).args(&args).output() {
            Ok(o) => o,
            // Lister not installed; try the next one.
            Err(_) => continue,
        };
        if !out.status.success() {
            continue;
        }
        let text = String::from_utf8_lossy(&out.stdout);
        let types = if prog == "osascript" {
            mac_clipboard_types(&text)
        } else {
            text.lines()
                .map(|l| l.trim().to_string())
                .filter(|l| l.contains('/'))
                .collect()
        };
        if !types.is_empty() {
            return types;
        }
    }
    Vec::new()
}

/// The clipboard-type listers to try, in order, for the running platform.
fn type_listers() -> Vec<(String, Vec<String>)> {
    if cfg!(target_os = "macos") {
        vec![(
            "osascript".into(),
            vec!["-e".into(), "clipboard info".into()],
        )]
    } else {
        vec![
            ("wl-paste".into(), vec!["--list-types".into()]),
            (
                "xclip".into(),
                vec![
                    "-selection".into(),
                    "clipboard".into(),
                    "-t".into(),
                    "TARGETS".into(),
                    "-o".into(),
                ],
            ),
        ]
    }
}

/// AppleScript clipboard classes and the MIME type each stands for. `clipboard
/// info` names the flavours by four-character class code, so this is the table
/// that turns a macOS pasteboard into the TARGETS list emacs works from.
const MAC_CLASSES: &[(&str, &str)] = &[
    ("PNGf", "image/png"),
    ("JPEG", "image/jpeg"),
    ("GIFf", "image/gif"),
    ("TIFF", "image/tiff"),
    ("HTML", "text/html"),
];

/// The MIME types in one `osascript -e 'clipboard info'` line, which reads
/// `«class PNGf», 8462, «class HTML», 214, string, 12`: class/size pairs
/// separated by commas.
fn mac_clipboard_types(info: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for field in info.split(',') {
        let field = field.trim();
        let class = field
            .trim_start_matches("«class ")
            .trim_end_matches('»')
            .trim();
        if let Some((_, mime)) = MAC_CLASSES.iter().find(|(c, _)| *c == class) {
            if !out.iter().any(|t| t == mime) {
                out.push((*mime).to_string());
            }
        }
    }
    out
}

/// The file extension for a saved clipboard image, so the name `yank-media`
/// inserts matches what it actually wrote.
pub fn media_extension(mime: &str) -> &'static str {
    match mime {
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/tiff" => "tif",
        "image/webp" => "webp",
        _ => "png",
    }
}

/// Write the clipboard's `mime` flavour to `dest`, returning the bytes written.
/// Uses the platform's own clipboard reader: `pngpaste` (or AppleScript, which
/// can fetch any of the `MAC_CLASSES`) on macOS, `wl-paste` under Wayland and
/// `xclip` under X11.
pub fn clipboard_media(mime: &str, dest: &Path) -> Result<u64, String> {
    for (prog, args) in media_readers(mime, dest) {
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
    Err(format!("yank-media: no {mime} on the clipboard"))
}

/// Write the clipboard's `image/png` flavour to `dest` — the common case, and
/// the only one before a mode had a handler for anything else.
pub fn clipboard_png(dest: &Path) -> Result<u64, String> {
    clipboard_media("image/png", dest)
}

/// The clipboard's `text/html` flavour as text. Both platform readers hand back
/// bytes, so it goes through a file the way the image flavours do.
pub fn clipboard_html() -> Result<String, String> {
    let tmp = std::env::temp_dir().join(format!("zmax-yank-html-{}", std::process::id()));
    clipboard_media("text/html", &tmp)?;
    let text = std::fs::read(&tmp)
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .map_err(|e| format!("yank-media: {e}"));
    let _ = std::fs::remove_file(&tmp);
    // "Some programs add a nul character at the end of text/* selections.
    // Remove that." — `yank-media-types--format'.
    text.map(|t| t.trim_end_matches('\0').to_string())
}

/// The clipboard readers to try, in order, for `mime` on the running platform.
fn media_readers(mime: &str, dest: &Path) -> Vec<(String, Vec<String>)> {
    let d = dest.to_string_lossy().into_owned();
    if cfg!(target_os = "macos") {
        let class = MAC_CLASSES
            .iter()
            .find(|(_, m)| *m == mime)
            .map(|(c, _)| *c)
            .unwrap_or("PNGf");
        let mut readers = Vec::new();
        // `pngpaste' only knows PNG, and is the fastest path when it is there.
        if mime == "image/png" {
            readers.push(("pngpaste".into(), vec![d.clone()]));
        }
        readers.push((
            "osascript".into(),
            vec![
                "-e".into(),
                format!("set p to POSIX file \"{d}\""),
                "-e".into(),
                format!("set d to (the clipboard as «class {class}»)"),
                "-e".into(),
                "set fh to open for access p with write permission".into(),
                "-e".into(),
                "set eof fh to 0".into(),
                "-e".into(),
                "write d to fh".into(),
                "-e".into(),
                "close access fh".into(),
            ],
        ));
        readers
    } else {
        vec![
            (
                "wl-paste".into(),
                vec!["--no-newline".into(), "--type".into(), mime.to_string()],
            ),
            (
                "xclip".into(),
                vec![
                    "-selection".into(),
                    "clipboard".into(),
                    "-t".into(),
                    mime.to_string(),
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
/// theirs: `<stem>-<n>.<ext>` in `dir`, `n` the first free number. `ext` follows
/// the clipboard flavour that was chosen, so a JPEG is not saved as `.png`.
pub fn media_file_name(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let stem = if stem.is_empty() { "yank" } else { stem };
    for n in 1..10_000 {
        let candidate = dir.join(format!("{stem}-{n}.{ext}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    dir.join(format!("{stem}.{ext}"))
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
    let picker = ThumbsPicker::new(&dir).map_err(|e| anyhow!("{e}"))?;
    crate::commands::display_images_in_terminal(
        cx.editor,
        &[picker.sheet.clone()],
        0,
        false,
        false,
        100,
    );
    cx.editor.set_status(format!(
        "thumbs: {} image{} in {}",
        picker.images.len(),
        if picker.images.len() == 1 { "" } else { "s" },
        dir.display()
    ));
    Ok(())
}

/// `:yank-media` — Emacs `yank-media`: "Yank media (images, HTML and the like)
/// from the clipboard. This command depends on the current major mode having
/// support for accepting the media type." Every flavour the clipboard is
/// offering is enumerated, intersected with what the buffer's language handles
/// ([`handled_types`]) and narrowed to one by `yank-media-preferred-types`
/// ([`preferred_type`]). An image is saved next to the buffer's file and
/// referenced through [`media_link`]; `text/html` is inserted as it stands.
///
/// A language with no handler at all is emacs's `user-error`, not a fallback:
/// "The `%s' mode hasn't registered any handlers".
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
    let mode = language.as_deref().unwrap_or("fundamental");
    if handled_types(language.as_deref()).is_empty() {
        bail!("The `{mode}' mode hasn't registered any handlers");
    }
    let available = clipboard_types();
    let mime = match preferred_type(&available, language.as_deref()) {
        Some(mime) => mime,
        // Emacs's own wording for a clipboard nothing in the buffer can take.
        None => bail!("No handler in the current buffer for anything on the clipboard"),
    };

    if mime == "text/html" {
        let html = clipboard_html().map_err(|e| anyhow!("{e}"))?;
        insert_at_cursors(cx, &html);
        cx.editor
            .set_status(format!("yank-media: inserted {} bytes of HTML", html.len()));
        return Ok(());
    }

    let dest = media_file_name(&dir, &stem, media_extension(&mime));
    let bytes = clipboard_media(&mime, &dest).map_err(|e| anyhow!("{e}"))?;

    // A relative reference keeps the buffer portable, the way Emacs's org and
    // markdown handlers insert one relative to the file.
    let shown = dest
        .strip_prefix(&dir)
        .unwrap_or(&dest)
        .to_string_lossy()
        .into_owned();
    // `handled_types' only offers the image flavours when a link exists, so this
    // is the same handler the type was selected through.
    let text = media_link(language.as_deref(), &shown)
        .ok_or_else(|| anyhow!("The `{mode}' mode hasn't registered any handlers"))?;
    insert_at_cursors(cx, &text);
    cx.editor
        .set_status(format!("yank-media: saved {shown} ({bytes} bytes, {mime})"));
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
        let first = media_file_name(&dir, "note", "png");
        assert_eq!(first.file_name().unwrap(), "note-1.png");
        std::fs::write(&first, b"x").unwrap();
        assert_eq!(
            media_file_name(&dir, "note", "png").file_name().unwrap(),
            "note-2.png"
        );
        // The extension follows the flavour that was taken off the clipboard.
        assert_eq!(
            media_file_name(&dir, "note", media_extension("image/jpeg"))
                .file_name()
                .unwrap(),
            "note-1.jpg"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn preferred_type_follows_yank_media_preferred_types() {
        let all: Vec<String> = ["text/html", "image/tiff", "image/jpeg", "image/png"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        // "Give PNG more priority" — ahead of JPEG and of text/html, whatever
        // order the platform listed the flavours in.
        assert_eq!(
            preferred_type(&all, Some("markdown")).as_deref(),
            Some("image/png")
        );
        let no_png: Vec<String> = all.iter().filter(|t| *t != "image/png").cloned().collect();
        assert_eq!(
            preferred_type(&no_png, Some("markdown")).as_deref(),
            Some("image/jpeg")
        );
        // text/html is last in the preference list but still beats a type that
        // is not on it at all (image/tiff), which emacs would only reach via the
        // prefix-argument prompt.
        assert_eq!(
            preferred_type(&["image/tiff".into(), "text/html".into()], Some("markdown")).as_deref(),
            Some("text/html")
        );
        // Nothing preferred is on the clipboard: the first handled type wins,
        // standing in for emacs's `completing-read' over the rest.
        assert_eq!(
            preferred_type(&["image/tiff".into()], Some("markdown")).as_deref(),
            Some("image/tiff")
        );
        // A language with an image handler but no literal HTML takes no HTML.
        assert_eq!(preferred_type(&["text/html".into()], Some("latex")), None);
        // No handler at all -> nothing to yank, which the caller turns into
        // emacs's "hasn't registered any handlers".
        assert!(handled_types(Some("rust")).is_empty());
        assert_eq!(preferred_type(&all, Some("rust")), None);
    }

    #[test]
    fn mac_clipboard_info_becomes_a_targets_list() {
        // The shape `osascript -e 'clipboard info'` prints: class/size pairs.
        assert_eq!(
            mac_clipboard_types("«class PNGf», 8462, «class HTML», 214, string, 12"),
            vec!["image/png".to_string(), "text/html".to_string()]
        );
        // Text-only clipboards carry no media flavour at all.
        assert!(mac_clipboard_types("string, 12, «class utf8», 12").is_empty());
        assert!(mac_clipboard_types("").is_empty());
    }

    #[test]
    fn thumb_index_maps_cells_to_the_montage_order() {
        // `montage -tile 4x' fills row-major in the order the files were given.
        assert_eq!(thumb_index(0, 0, 4, 6), Some(0));
        assert_eq!(thumb_index(0, 3, 4, 6), Some(3));
        assert_eq!(thumb_index(1, 1, 4, 6), Some(5));
        // Past the last image, and past the last column of the sheet.
        assert_eq!(thumb_index(1, 2, 4, 6), None);
        assert_eq!(thumb_index(0, 4, 4, 6), None);
        assert_eq!(thumb_index(9, 0, 4, 6), None);
        // Round-trips with its inverse for every cell that holds an image.
        for i in 0..6 {
            let (row, col) = thumb_cell(i, 4);
            assert_eq!(thumb_index(row, col, 4, 6), Some(i));
        }
    }

    #[test]
    fn thumbs_picker_tracks_point_and_the_marked_list() {
        let images: Vec<PathBuf> = (1..=6)
            .map(|n| PathBuf::from(format!("/tmp/img{n}.png")))
            .collect();
        let mut p = ThumbsPicker::from_images(
            Path::new("/tmp"),
            images.clone(),
            PathBuf::from("/tmp/sheet.png"),
        );
        assert_eq!(p.rows(), 2, "6 images at 4 per line is two rows");
        assert_eq!(p.current(), Some(images[0].as_path()));
        assert_eq!(p.at_cell(1, 1), Some(images[5].as_path()));
        assert_eq!(p.at_cell(1, 2), None);

        // A click on an empty cell leaves point alone; on a filled one it moves.
        assert!(!p.select_cell(1, 3));
        assert_eq!(p.index(), 0);
        assert!(p.select_cell(1, 0));
        assert_eq!(p.current(), Some(images[4].as_path()));
        assert_eq!(p.cursor_cell(), (1, 0));

        // `thumbs-forward-char' steps one image and stops at the end.
        p.forward_char(false);
        assert_eq!(p.index(), 5);
        p.forward_char(false);
        assert_eq!(p.index(), 5, "no image after the last one");
        p.forward_char(true);
        assert_eq!(p.index(), 4);

        // `thumbs-forward-line' is a bare `forward-line': start of the row.
        assert!(p.forward_line(true));
        assert_eq!(p.cursor_cell(), (0, 0));
        assert!(!p.forward_line(true), "no row above the first");
        assert!(p.forward_line(false));
        assert_eq!(p.index(), 4);
        assert!(!p.forward_line(false), "no row below the last");

        // `thumbs-mark' / `thumbs-unmark' keep `thumbs-marked-list' in sheet
        // order, whatever order the cells were marked in.
        p.mark(true);
        p.select_cell(0, 1);
        p.mark(true);
        assert_eq!(p.marked_files(), vec![images[1].clone(), images[4].clone()]);
        assert!(p.is_marked(&images[1]));
        p.mark(false);
        assert_eq!(p.marked_files(), vec![images[4].clone()]);
        assert!(!p.is_marked(&images[1]));
    }
}
