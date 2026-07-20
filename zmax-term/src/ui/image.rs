//! Image — the zmax port of GNU Emacs `image-mode`'s transform keys.
//!
//! Like `doc-view-mode`, the picture is drawn straight to the terminal by
//! `commands::display_images_in_terminal` and the rotate/flip/scale state lives
//! in the `IMAGE_XFORM` the `image-*` typable commands already read and write.
//! This [`Component`] exists to own the *keymap*: without it the commands were
//! reachable only by name (`:image-rotate`), so none of Emacs's image keys
//! worked. Every key dispatches into the same helpers the typables use, so the
//! two paths cannot drift apart.
//!
//! Keys (parsed into an `image` keymap mode by `scripts/gen_port_report.py`).
//! Emacs splits them across two prefixes, and this follows the manual's Image
//! Mode node verbatim:
//!
//!   i + — `image-increase-size`      i - — `image-decrease-size`
//!   i r — `image-rotate` (90° cw)    i h — `image-flip-horizontally`
//!   i v — `image-flip-vertically`    i o — `image-save`
//!   i c — `image-crop`               i x — `image-cut`
//!   RET — `image-toggle-animation`  C-c C-c — `image-toggle-display`
//!   s w — `image-transform-fit-to-window`
//!   s o — `image-transform-reset-to-original`
//!   q / Esc — leave the viewer
//!
//! `i c` / `i x` / `i o` follow `image-crop.el` and `image.el` in keeping the
//! *file* untouched: the crop/cut result is a pending edit that only `i o`
//! (`image-save`, "Write image to file:") writes out, exactly like Emacs editing
//! the image in the buffer and leaving the save to you. The two ImageMagick
//! pipelines are `image-crop-crop-command` (`+repage -crop WxH+X+Y`) and
//! `image-crop-cut-command` (`-fill COLOR -draw "rectangle L,T R,B"`).
//!
//! Diverges from Emacs in how the *region* is named. Emacs superimposes an SVG
//! rectangle on the image and lets you drag it; zmax's picture is painted
//! straight into the terminal by an external viewer, with no cell zmax can draw
//! a rubber band into, so the region is typed as an ImageMagick `WxH+X+Y`
//! geometry in an in-mode minibuffer instead (Dired's pattern). `image-cut`'s
//! colour, a prefix argument in Emacs, is an optional second word.
//!
//! `C-c C-c` (`image-toggle-display`) is the same two-state toggle Emacs has, in
//! the terms this overlay has to work in. Emacs's image state is a *display*
//! property over the file's bytes and its text state is those bytes with the
//! property stripped (`image-toggle-display-text`); zmax's picture is painted by
//! the tty handoff over a buffer that already holds the bytes, so the text state
//! is just this overlay standing down — it keeps only `C-c C-c`, the way
//! `image-minor-mode` does, and every other key falls through to the buffer,
//! which is editable again. Toggling back repaints the image.
//!
//! `RET` likewise cannot drive the frames itself — the viewer owns the animation
//! — so stopping freezes the image on frame 0 rather than on the frame that was
//! showing, and `image-animate-loop` has no analogue.
//!
//! Deferred, each needing substrate that does not exist yet:
//!   s 0 (`image-transform-reset-to-initial`) — distinct from `-to-original`:
//!     it restores the *initial display* size (the auto-fit), which needs an
//!     auto-resize model zmax does not have.
//!   s p / s s (`image-transform-set-percent` / `-set-scale`) — both read a
//!     value, so they need a prompt rather than a bare chord; `:image-transform-
//!     set-percent 50` reaches them meanwhile.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context as _};
use tui::buffer::Buffer as Surface;
use zmax_view::graphics::Rect;
use zmax_view::input::KeyEvent;
use zmax_view::keyboard::{KeyCode, KeyModifiers};

use crate::commands::typed::{
    current_image_path, image_set_scale, image_transform, image_transform_reset_all, image_xform_of,
};
use crate::{
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

/// `image-cut-color`: the fill for the rectangle `image-cut` removes.
const IMAGE_CUT_COLOR: &str = "black";

/// The image's unsaved edit — `(original file, the cropped/cut bytes)`. Emacs
/// replaces the *buffer's* image and leaves the file alone until `image-save`;
/// zmax's buffer holds no image data, so the edited bytes live in a temp file
/// and `image-save` copies that out.
static IMAGE_EDIT: std::sync::Mutex<Option<(PathBuf, PathBuf)>> = std::sync::Mutex::new(None);

/// The still frame shown while animation is stopped — `(original file, frame 0)`.
/// Absent means the viewer is playing the image, which is its default.
static IMAGE_STILL: std::sync::Mutex<Option<(PathBuf, PathBuf)>> = std::sync::Mutex::new(None);

/// An in-mode minibuffer read, opened by the keys that need an argument. Emacs
/// reads those interactively over the image; see the module comment for why the
/// region has to be typed here.
struct Input {
    prompt: &'static str,
    buffer: String,
    action: Pending,
}

/// What the [`Input`] currently being read is for.
enum Pending {
    /// `image-crop`: keep the named region.
    Crop,
    /// `image-cut`: fill the named region with a colour.
    Cut,
    /// `image-save`: write the image's current bytes to the named file.
    Save,
}

/// The viewer overlay. Holds no transform state of its own — `IMAGE_XFORM` stays
/// the single source of truth, so an `:image-rotate` typed while the overlay is
/// up stays in sync.
#[derive(Default)]
pub struct Image {
    /// `i` or `s` was typed and the next key names the transform.
    pending: Option<char>,
    /// `C-c` was typed and the next key decides whether it is `C-c C-c`.
    pending_ctrl_c: bool,
    /// The buffer is in `image-toggle-display`'s *text* state: the bytes are
    /// showing and only the toggle itself is still bound here.
    text: bool,
    /// Active minibuffer read, if any (see [`Input`]).
    input: Option<Input>,
}

impl Image {
    pub fn new() -> Self {
        Self::default()
    }

    /// `i` then a key: Emacs's image-at-point transform map.
    fn dispatch_i_key(&mut self, cx: &mut Context, key: KeyEvent) -> anyhow::Result<()> {
        match key {
            // `image-increase-size` / `-decrease-size`: emacs steps by 20%, zmax
            // by the same 5/4 and 4/5 the typables use, so key and command agree.
            key!('+') => {
                let sc = current_scale(cx);
                image_set_scale(cx, (sc * 5 / 4).max(sc + 1))
            }
            key!('-') => {
                let sc = current_scale(cx);
                image_set_scale(cx, (sc * 4 / 5).max(1))
            }
            key!('r') => image_transform(cx, 90, false, false),
            key!('h') => image_transform(cx, 0, true, false),
            key!('v') => image_transform(cx, 0, false, true),
            // The three keys that need an argument Emacs reads by dragging a
            // rectangle over the image / with `read-file-name`.
            key!('c') => {
                self.begin_input("Crop region (WxH+X+Y): ", Pending::Crop);
                Ok(())
            }
            key!('x') => {
                self.begin_input("Cut region (WxH+X+Y [colour]): ", Pending::Cut);
                Ok(())
            }
            key!('o') => {
                self.begin_input("Write image to file: ", Pending::Save);
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// `C-c` then a key: Emacs binds exactly one chord off it here. Kept in its
    /// own fn so the chord reads as the two-key sequence it is.
    fn dispatch_ctrl_c_key(&mut self, cx: &mut Context, key: KeyEvent) -> anyhow::Result<()> {
        match key {
            ctrl!('c') => self.toggle_display(cx),
            _ => Ok(()),
        }
    }

    /// `image-toggle-display`: swap the buffer between the image and the bytes
    /// behind it. Both messages are Emacs's own, from `image-toggle-display-text`
    /// and `image-toggle-display-image`.
    fn toggle_display(&mut self, cx: &mut Context) -> anyhow::Result<()> {
        let Some(orig) = current_image_path(cx) else {
            bail!("image-mode: current buffer is not an image file");
        };
        self.text = !self.text;
        if self.text {
            cx.editor
                .set_status("Repeat this command to go back to displaying the image");
        } else {
            redisplay(cx, &orig);
            cx.editor
                .set_status("Repeat this command to go back to displaying the file as text");
        }
        Ok(())
    }

    /// Open the in-mode minibuffer for `action`, showing `prompt`.
    fn begin_input(&mut self, prompt: &'static str, action: Pending) {
        self.input = Some(Input {
            prompt,
            buffer: String::new(),
            action,
        });
    }

    /// Run the read the user just committed.
    fn run_pending(&mut self, action: Pending, arg: &str, cx: &mut Context) {
        let done = match action {
            Pending::Crop => image_edit(cx, arg, false),
            Pending::Cut => image_edit(cx, arg, true),
            Pending::Save => image_save(cx, arg),
        };
        report(cx, done);
    }

    /// The minibuffer's own key handling: type, rub out, commit, abort.
    fn handle_input_key(&mut self, key: KeyEvent, cx: &mut Context) -> EventResult {
        match key {
            key!(Esc) => self.input = None,
            key!(Enter) => {
                if let Some(inp) = self.input.take() {
                    self.run_pending(inp.action, &inp.buffer, cx);
                }
            }
            key!(Backspace) => {
                if let Some(inp) = self.input.as_mut() {
                    inp.buffer.pop();
                }
            }
            KeyEvent {
                code: KeyCode::Char(c),
                modifiers,
            } if modifiers == KeyModifiers::NONE || modifiers == KeyModifiers::SHIFT => {
                if let Some(inp) = self.input.as_mut() {
                    inp.buffer.push(c);
                }
            }
            _ => {}
        }
        EventResult::Consumed(None)
    }

    /// `s` then a key: Emacs's image-mode scaling map.
    fn dispatch_s_key(&mut self, cx: &mut Context, key: KeyEvent) -> anyhow::Result<()> {
        match key {
            key!('w') => image_set_scale(cx, 100),
            key!('o') => image_transform_reset_all(cx),
            _ => Ok(()),
        }
    }
}

/// The current image's scale, or 100% when there is no image (the callers then
/// fail with the same "not an image file" error the typables give).
fn current_scale(cx: &Context) -> u32 {
    current_image_path(cx)
        .map(|p| image_xform_of(&p).3)
        .unwrap_or(100)
}

/// The entry of `slot` belonging to `orig`, if any.
fn slot_for(slot: &std::sync::Mutex<Option<(PathBuf, PathBuf)>>, orig: &Path) -> Option<PathBuf> {
    match &*slot.lock().unwrap() {
        Some((p, f)) if p == orig => Some(f.clone()),
        _ => None,
    }
}

/// Put `file` in `slot` for `orig`, discarding whatever temp it held.
fn set_slot(
    slot: &std::sync::Mutex<Option<(PathBuf, PathBuf)>>,
    orig: &Path,
    file: Option<PathBuf>,
) {
    let old = std::mem::replace(
        &mut *slot.lock().unwrap(),
        file.map(|f| (orig.to_path_buf(), f)),
    );
    if let Some((_, stale)) = old {
        let _ = std::fs::remove_file(stale);
    }
}

/// The image's current bytes: the pending crop/cut result if there is one, else
/// the file itself. This is what Emacs calls the image *in the buffer*.
fn image_data_path(orig: &Path) -> PathBuf {
    slot_for(&IMAGE_EDIT, orig).unwrap_or_else(|| orig.to_path_buf())
}

/// The file to hand the viewer: the frozen frame while animation is stopped,
/// else the image's current bytes.
fn shown_path(orig: &Path) -> PathBuf {
    slot_for(&IMAGE_STILL, orig).unwrap_or_else(|| image_data_path(orig))
}

/// Redisplay `orig` — the frozen/edited file when there is one — under the
/// rotate/flip/scale the transform keys have accumulated.
fn redisplay(cx: &mut Context, orig: &Path) {
    let (r, fh, fv, sc) = image_xform_of(orig);
    let shown = shown_path(orig);
    crate::commands::display_images_in_terminal(cx.editor, &[shown], r, fh, fv, sc);
}

/// A fresh temp file carrying `orig`'s extension, so ImageMagick keeps writing
/// the format the image already is (`%f:-` in the `image-crop-*-command`s).
fn edit_temp(orig: &Path) -> PathBuf {
    let ext = orig.extension().and_then(|e| e.to_str()).unwrap_or("png");
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("zmax-image-{}-{stamp}.{ext}", std::process::id()))
}

/// Run ImageMagick with `args`, probing `magick` then the older `convert` — the
/// same two names the display pipeline falls back through.
fn magick(args: &[String]) -> anyhow::Result<()> {
    for prog in ["magick", "convert"] {
        match std::process::Command::new(prog).args(args).output() {
            Ok(out) if out.status.success() => return Ok(()),
            Ok(out) => bail!("{prog}: {}", String::from_utf8_lossy(&out.stderr).trim()),
            // Not installed under that name; try the next one.
            Err(_) => continue,
        }
    }
    bail!("Couldn't find `magick' command to crop/cut the image")
}

/// Parse an ImageMagick region — `WxH+X+Y`, or `WxH` at the origin.
fn parse_geometry(spec: &str) -> Option<(u32, u32, u32, u32)> {
    let (size, offset) = match spec.find('+') {
        Some(i) => (&spec[..i], &spec[i + 1..]),
        None => (spec, ""),
    };
    let (w, h) = size.split_once(['x', 'X'])?;
    let (x, y) = match offset {
        "" => ("0", "0"),
        o => o.split_once('+')?,
    };
    Some((
        w.trim().parse().ok()?,
        h.trim().parse().ok()?,
        x.trim().parse().ok()?,
        y.trim().parse().ok()?,
    ))
}

/// `image-crop` (`cut` false) and `image-cut` (`cut` true): run the region
/// through the matching `image-crop-*-command` and make the result the image's
/// pending, unsaved bytes.
fn image_edit(cx: &mut Context, spec: &str, cut: bool) -> anyhow::Result<()> {
    let Some(orig) = current_image_path(cx) else {
        bail!("image-mode: current buffer is not an image file");
    };
    let name = if cut { "image-cut" } else { "image-crop" };
    // `image-cut` takes the fill colour from a prefix argument; with no prefix
    // keys in the viewer it is an optional second word, defaulting to
    // `image-cut-color`.
    let mut words = spec.split_whitespace();
    let (geom, color) = (
        words.next().unwrap_or_default(),
        words.next().unwrap_or(IMAGE_CUT_COLOR),
    );
    let (w, h, x, y) =
        parse_geometry(geom).with_context(|| format!("{name}: expected a WxH+X+Y region"))?;
    if w == 0 || h == 0 {
        bail!("{name}: region has no area");
    }

    let src = image_data_path(&orig);
    let out = edit_temp(&orig);
    let args: Vec<String> = if cut {
        // `image-crop-cut-command`. Emacs lists `-fill` after the `-draw` it
        // colours; ImageMagick only honours the setting when it comes first.
        vec![
            src.to_string_lossy().into_owned(),
            "-fill".into(),
            color.into(),
            "-draw".into(),
            format!("rectangle {x},{y} {},{}", x + w, y + h),
            out.to_string_lossy().into_owned(),
        ]
    } else {
        // `image-crop-crop-command`.
        vec![
            src.to_string_lossy().into_owned(),
            "+repage".into(),
            "-crop".into(),
            format!("{w}x{h}+{x}+{y}"),
            out.to_string_lossy().into_owned(),
        ]
    };
    magick(&args)?;

    set_slot(&IMAGE_EDIT, &orig, Some(out));
    // The frozen frame was cut from the pre-edit bytes, so it is stale now.
    set_slot(&IMAGE_STILL, &orig, None);
    redisplay(cx, &orig);
    cx.editor.set_status(format!(
        "{name}: {w}x{h}+{x}+{y} (unsaved; i o writes it out)"
    ));
    Ok(())
}

/// `image-save`: write the image's data to a file you name. Emacs saves the
/// original bytes — "Rotating or changing the displayed image size does not
/// affect the saved image" — so the rotate/flip/scale state is deliberately not
/// applied here; a pending crop/cut *is* the data and so is written.
fn image_save(cx: &mut Context, dest: &str) -> anyhow::Result<()> {
    let Some(orig) = current_image_path(cx) else {
        bail!("image-mode: current buffer is not an image file");
    };
    if dest.trim().is_empty() {
        bail!("image-save: no file name");
    }
    let src = image_data_path(&orig);
    if !src.exists() {
        bail!("File {} no longer exists", src.display());
    }
    let dest = zmax_stdx::path::expand_tilde(Path::new(dest.trim()));
    std::fs::copy(&src, &dest)
        .with_context(|| format!("image-save: cannot write {}", dest.display()))?;
    cx.editor
        .set_status(format!("image: wrote {}", dest.display()));
    Ok(())
}

/// The image's frame count (Emacs `image-multi-frame-p`), read with ImageMagick
/// `identify %n`. `None` when neither identify name is available.
fn frame_count(path: &Path) -> Option<u32> {
    for argv in [&["magick", "identify"][..], &["identify"][..]] {
        let out = std::process::Command::new(argv[0])
            .args(&argv[1..])
            .args(["-ping", "-format", "%n\n"])
            .arg(path)
            .output();
        match out {
            Ok(out) if out.status.success() => {
                return String::from_utf8_lossy(&out.stdout)
                    .lines()
                    .next()
                    .and_then(|n| n.trim().parse().ok());
            }
            _ => continue,
        }
    }
    None
}

/// `image-toggle-animation`: start or stop animating the current image. The
/// terminal viewer owns playback, so "start" is just handing it the animated
/// file again and "stop" is handing it frame 0 instead.
fn toggle_animation(cx: &mut Context) -> anyhow::Result<()> {
    let Some(orig) = current_image_path(cx) else {
        bail!("No image is present");
    };
    if slot_for(&IMAGE_STILL, &orig).is_some() {
        set_slot(&IMAGE_STILL, &orig, None);
        redisplay(cx, &orig);
        cx.editor.set_status("image: animating");
        return Ok(());
    }
    let src = image_data_path(&orig);
    if frame_count(&src).unwrap_or(1) < 2 {
        cx.editor.set_status("No image animation.");
        return Ok(());
    }
    let still = edit_temp(&orig);
    magick(&[
        format!("{}[0]", src.to_string_lossy()),
        still.to_string_lossy().into_owned(),
    ])?;
    set_slot(&IMAGE_STILL, &orig, Some(still));
    redisplay(cx, &orig);
    cx.editor.set_status("image: animation stopped");
    Ok(())
}

impl Component for Image {
    fn handle_event(&mut self, event: &Event, cx: &mut Context) -> EventResult {
        let key = match event {
            Event::Key(key) => *key,
            _ => return EventResult::Ignored(None),
        };
        let close: Callback = Box::new(|compositor: &mut Compositor, _cx| {
            compositor.pop();
        });

        // A minibuffer read owns every key until it is committed or aborted.
        if self.input.is_some() {
            return self.handle_input_key(key, cx);
        }

        // `C-c C-c` is a two-key chord; a Component has no keymap trie, so the
        // prefix is tracked by hand as doc-view's `C-x` pair is.
        if std::mem::take(&mut self.pending_ctrl_c) {
            let toggled = self.dispatch_ctrl_c_key(cx, key);
            report(cx, toggled);
            return EventResult::Consumed(None);
        }
        if key == ctrl!('c') {
            self.pending_ctrl_c = true;
            return EventResult::Consumed(None);
        }
        // Text state: the bytes are showing and the buffer is a normal, editable
        // buffer again, so every key but the toggle above belongs to it.
        if self.text {
            return EventResult::Ignored(None);
        }

        if let Some(prefix) = self.pending.take() {
            let done = match prefix {
                'i' => self.dispatch_i_key(cx, key),
                _ => self.dispatch_s_key(cx, key),
            };
            report(cx, done);
            // The transform helpers redisplay the *file*; when the image has a
            // pending crop/cut or is frozen, put the right one back on screen.
            if let Some(orig) = current_image_path(cx) {
                if shown_path(&orig) != orig {
                    redisplay(cx, &orig);
                }
            }
            return EventResult::Consumed(None);
        }

        match key {
            key!('q') | key!(Esc) => return EventResult::Consumed(Some(close)),
            key!('i') => self.pending = Some('i'),
            key!('s') => self.pending = Some('s'),
            key!(Enter) => {
                let done = toggle_animation(cx);
                report(cx, done);
            }
            _ => return EventResult::Ignored(None),
        }
        EventResult::Consumed(None)
    }

    /// The picture is painted straight to the terminal, so there is nothing to
    /// draw onto the `Surface` — clearing it would erase the image the terminal
    /// is already holding. The one exception is a minibuffer read, which has to
    /// show what has been typed so far.
    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        let Some(inp) = &self.input else { return };
        if area.height == 0 {
            return;
        }
        let style = ctx.editor.theme.get("ui.text");
        let line = format!("{}{}", inp.prompt, inp.buffer);
        surface.set_stringn(
            area.x,
            area.y + area.height - 1,
            &line,
            area.width as usize,
            style,
        );
    }

    fn id(&self) -> Option<&'static str> {
        Some("image")
    }
}

/// The helpers fail when the buffer stops being an image; say so on the status
/// line rather than dropping it, which is what the typable path does.
fn report(cx: &mut Context, result: anyhow::Result<()>) {
    if let Err(e) = result {
        cx.editor.set_error(e.to_string());
    }
}
