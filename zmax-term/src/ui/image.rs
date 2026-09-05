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
//!   m   — `image-mode-mark-file`     u   — `image-mode-unmark-file`
//!   s w — `image-transform-fit-to-window`
//!   s o — `image-transform-reset-to-original`
//!   s 0 — `image-transform-reset-to-initial`
//!   s p — `image-transform-set-percent`
//!   s s — `image-transform-set-scale`
//!   q / Esc — leave the viewer
//!
//! `i c` / `i x` / `i o` follow `image-crop.el` and `image.el` in keeping the
//! *file* untouched: the crop/cut result is a pending edit that only `i o`
//! (`image-save`, "Write image to file:") writes out, exactly like Emacs editing
//! the image in the buffer and leaving the save to you.
//!
//! # `image-crop` / `image-cut`
//!
//! Emacs replaces the image with an SVG copy of it, draws a rectangle on that
//! SVG and runs a mouse-tracking loop over it (`image-crop--crop-image-1`,
//! image-crop.el:294-399) whose keys are `m` (move the rectangle), `s` (square
//! it, then move), `RET` (execute) and `q` (quit, changing nothing). zmax's
//! picture is painted straight into the terminal by an external viewer, with no
//! cell zmax can draw a rubber band into and no mouse position to read against
//! it, so the rectangle is *named* rather than dragged: `i c` / `i x` read a
//! `WxH+X+Y` geometry in an in-mode minibuffer and then open the same
//! move/square/execute/quit session over it, driven by `h`/`j`/`k`/`l` and the
//! arrows instead of a drag. Everything downstream of the rectangle is Emacs's:
//!
//! * the rectangle lives in *displayed* pixel space and is multiplied by
//!   `factor` — the image's own width over the width it is displayed at — before
//!   the command runs (`image-crop--crop-image-update`, image-crop.el:254-266),
//!   so a region named while the image is scaled by `i +` / `s p` still cuts the
//!   part of the picture it covers;
//! * the size is an absolute difference and the origin the smaller of the two
//!   corners (image-crop.el:259-266), so a rectangle grown up/left names the
//!   same region as one grown down/right;
//! * the display rotation is baked into the image data before the crop runs
//!   (`image-crop--possibly-rotate-buffer`, image-crop.el:422-455), so the
//!   region always means what it covers on screen;
//! * the two ImageMagick pipelines are `image-crop-crop-command`
//!   (`+repage -crop %wx%h+%l+%t`, image-crop.el:72-73) and
//!   `image-crop-cut-command` (`-draw "rectangle %l,%t %r,%b" -fill %c`,
//!   image-crop.el:57-59), argument order included — see [`cut_args`] for what
//!   that order costs.
//!
//! Not reproduced: the mouse (`down-mouse-1` to place a corner, drag to stretch,
//! click near a corner to adjust it — image-crop.el:330-375) has no terminal
//! analogue; and Emacs's "no SVG support" error (image-crop.el:161-162) is moot
//! because there is no SVG overlay to build. `image-cut`'s colour is a prefix
//! argument feeding `read-color` in Emacs (image-crop.el:134-135); with no
//! prefix keys in the viewer it is an optional second word on the geometry.
//!
//! # `RET` (`image-toggle-animation`)
//!
//! Emacs cancels the animation timer, leaving the image on the frame it had
//! reached, and restarts from that frame — from the beginning only when it is
//! already on the last one (image-mode.el:1124-1133). zmax keeps the same
//! `:index` (image.el:976-978) and hands the viewer either the still frame at
//! that index or an animation that starts there, so stopping no longer jumps
//! back to frame 0. What zmax cannot reproduce: the viewer owns playback during
//! a blocking tty handoff, so nothing here observes where a *looping* animation
//! was interrupted — the index is only known to advance when the animation was
//! played once through to its end, which is what `image-animate-loop` nil means
//! (image-mode.el:1105-1109, image.el:1072-1075).
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
//! `s 0` (`image-transform-reset-to-initial`) and `s o`
//! (`image-transform-reset-to-original`) differ in Emacs only in what resize
//! policy they restore — `image-auto-resize` versus none. The terminal viewer
//! always fits the picture to the window, so both land on "no rotation, no flip,
//! 100%" here and share a handler.
//!
//! `s p` / `s s` read their value in the same in-mode minibuffer `i c` / `i x`
//! use, since a bare chord cannot carry a number; `:image-transform-set-percent`
//! and `:image-transform-set-scale` are the same code by name.

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

/// `image-cut-color`: the fill for the rectangle `image-cut` removes
/// (image-crop.el:108-111).
const IMAGE_CUT_COLOR: &str = "black";

/// `image-animate-loop` (image-mode.el:1105-1109): "Non-nil means animated
/// images loop forever, rather than playing once." Its default is nil, and
/// Emacs only offers it through the menu bar (image-mode.el:600-608), which zmax
/// has no analogue of, so it is a constant here.
const IMAGE_ANIMATE_LOOP: bool = false;

/// The image's unsaved edit — `(original file, the cropped/cut bytes)`. Emacs
/// replaces the *buffer's* image and leaves the file alone until `image-save`;
/// zmax's buffer holds no image data, so the edited bytes live in a temp file
/// and `image-save` copies that out.
static IMAGE_EDIT: std::sync::Mutex<Option<(PathBuf, PathBuf)>> = std::sync::Mutex::new(None);

/// The animation state of the image on screen, or `None` when nothing has been
/// toggled yet. See [`Anim`].
static IMAGE_ANIM: std::sync::Mutex<Option<Anim>> = std::sync::Mutex::new(None);

/// What Emacs keeps on the image spec while animating it: the frame it is on
/// (`:index`, image.el:976-978), how many frames there are (the `car` of
/// `image-multi-frame-p`, image.el:912-925) and whether a timer is running
/// (image-mode.el:1124-1126 cancels it to stop).
///
/// `file` is the extra piece zmax needs: the viewer is handed a *file*, so the
/// still frame, or an animation re-encoded to start at `index`, has to exist on
/// disk. `None` means the image's own data file already shows what this state
/// describes.
struct Anim {
    orig: PathBuf,
    count: u32,
    index: u32,
    playing: bool,
    file: Option<PathBuf>,
}

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
    /// `image-transform-set-percent`: scale to a percentage of the original.
    SetPercent,
    /// `image-transform-set-scale`: scale by a multiplier of the original.
    SetScale,
}

/// Emacs's crop rectangle, in the *displayed* pixel space its SVG overlay works
/// in (`image-crop--crop-image-1`, image-crop.el:294-399). Signed and unordered:
/// `image-crop--width` / `--height` subtract without `abs` (image-crop.el:288-292)
/// because a drag may end above or left of where it started, and the ordering is
/// settled only when the region is handed to ImageMagick
/// (image-crop.el:259-266).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Area {
    left: i64,
    top: i64,
    right: i64,
    bottom: i64,
}

/// A region in the image's own pixel space — ImageMagick's `WxH+X+Y`, which is
/// what `image-crop-crop-command`'s `%wx%h+%l+%t` spells (image-crop.el:72-73).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Region {
    w: u32,
    h: u32,
    x: i64,
    y: i64,
}

/// The rectangle session `i c` / `i x` open over the image — zmax's stand-in for
/// the `track-mouse` loop in `image-crop--crop-image-1` (image-crop.el:294-399).
struct Crop {
    /// The image being cropped, and the data the crop will run on.
    orig: PathBuf,
    /// `image-crop`'s CUT argument (image-crop.el:157-159): the fill colour for
    /// `image-cut`, `None` for a plain crop.
    cut: Option<String>,
    /// The rectangle, in displayed pixel space.
    area: Area,
    /// Emacs's `move-unclick`/`move-click` states (image-crop.el:376-392), where
    /// the pointer moves the whole rectangle instead of a corner.
    moving: bool,
    /// The size the image is displayed at, which the rectangle cannot leave —
    /// image-crop.el:321-327 only tracks events whose position is over the SVG.
    /// `None` when ImageMagick's `identify` could not be run to measure it.
    size: Option<(u32, u32)>,
    /// `factor` in `image-crop--crop-image-update` (image-crop.el:257): the
    /// image's own width over the width it is displayed at.
    factor: f64,
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
    /// Active crop/cut rectangle session, if any (see [`Crop`]).
    crop: Option<Crop>,
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
            Pending::Crop => self.begin_crop(cx, arg, false),
            Pending::Cut => self.begin_crop(cx, arg, true),
            Pending::Save => image_save(cx, arg),
            // `image-transform-set-percent` reads a percentage of the original,
            // `-set-scale` a multiplier; both end at the same scale state.
            Pending::SetPercent => parse_number(arg, "image-transform-set-percent")
                .and_then(|n| image_set_scale(cx, scale_percent(n)?)),
            Pending::SetScale => parse_number(arg, "image-transform-set-scale")
                .and_then(|n| image_set_scale(cx, scale_percent(n * 100.0)?)),
        };
        report(cx, done);
    }

    /// Open the rectangle session `image-crop` / `image-cut` run over the image
    /// (image-crop.el:235-240). `spec` is the geometry that was typed, plus the
    /// fill colour for a cut; an empty geometry starts from the whole image,
    /// which is the largest rectangle Emacs's drag could have produced.
    fn begin_crop(&mut self, cx: &mut Context, spec: &str, cut: bool) -> anyhow::Result<()> {
        let Some(orig) = current_image_path(cx) else {
            // `image-crop`'s own user-error when point is not on an image
            // (image-crop.el:177-178).
            bail!("No image under point");
        };
        let name = if cut { "image-cut" } else { "image-crop" };
        // `image-cut` takes the fill colour from a prefix argument; with no
        // prefix keys in the viewer it is an optional second word, defaulting to
        // `image-cut-color` (image-crop.el:134-136).
        let mut words = spec.split_whitespace();
        let geom = words.next().unwrap_or_default();
        let color = words.next().unwrap_or(IMAGE_CUT_COLOR).to_string();

        let (rot, _, _, scale) = image_xform_of(&orig);
        let size = displayed_size(&image_data_path(&orig), rot, scale);
        let area = match (parse_geometry(geom), size) {
            (Some(r), _) => Area {
                left: r.x,
                top: r.y,
                right: r.x + r.w as i64,
                bottom: r.y + r.h as i64,
            },
            (None, Some((w, h))) if geom.is_empty() => Area {
                left: 0,
                top: 0,
                right: w as i64,
                bottom: h as i64,
            },
            _ => bail!("{name}: expected a WxH+X+Y region"),
        };

        let crop = Crop {
            orig,
            cut: cut.then_some(color),
            area: area.clamped(size),
            moving: false,
            size,
            factor: display_factor(scale),
        };
        cx.editor.set_status(crop.prompt());
        self.crop = Some(crop);
        Ok(())
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

    /// The crop session's keys — Emacs's `q` / `RET` / `m` / `s`
    /// (image-crop.el:126-131, 310-319, 397-399), with `h`/`j`/`k`/`l` and the
    /// arrows standing in for the mouse.
    ///
    /// Written as literal `KeyEvent` patterns rather than the `key!` macros the
    /// rest of this file uses: these keys are live only while a session is up,
    /// and `_parse_component_keymap` reads every `key!` in the file as a
    /// *top-level* image-mode binding (scripts/gen_port_report.py:512-518),
    /// which they are not.
    fn handle_crop_key(&mut self, key: KeyEvent, cx: &mut Context) -> EventResult {
        match key.code {
            // "q: Exit without changing anything" (image-crop.el:126). The loop
            // ends and `image-crop--crop-image-1` returns nil, so nothing runs
            // (image-crop.el:397-399).
            KeyCode::Esc | KeyCode::Char('q') => {
                self.crop = None;
                cx.editor.set_status("");
            }
            // "RET: Crop/cut the image" (image-crop.el:127).
            KeyCode::Enter => {
                if let Some(crop) = self.crop.take() {
                    let done = run_crop(cx, crop);
                    report(cx, done);
                }
            }
            code => {
                let Some(crop) = self.crop.as_mut() else {
                    return EventResult::Consumed(None);
                };
                match code {
                    // "m: Make mouse movements move the rectangle instead of
                    // altering the rectangle shape" (image-crop.el:128-129,
                    // 317-319).
                    KeyCode::Char('m') => crop.moving = true,
                    // "s: Same as `m', but make the rectangle into a square
                    // first" (image-crop.el:130-131, 310-315).
                    KeyCode::Char('s') => {
                        crop.area = crop.area.square();
                        crop.moving = true;
                    }
                    KeyCode::Left => crop.nudge(-1, 0),
                    KeyCode::Right => crop.nudge(1, 0),
                    KeyCode::Up => crop.nudge(0, -1),
                    KeyCode::Down => crop.nudge(0, 1),
                    // vi motions, with the shifted letter stepping by ten. A
                    // drag is continuous and a key press is not, so the coarse
                    // step has no Emacs counterpart; it is here so a large
                    // image is adjustable in a sane number of keystrokes.
                    KeyCode::Char(c @ ('h' | 'H' | 'j' | 'J' | 'k' | 'K' | 'l' | 'L')) => {
                        let step = if c.is_ascii_uppercase() { 10 } else { 1 };
                        let (dx, dy) = match c.to_ascii_lowercase() {
                            'h' => (-step, 0),
                            'l' => (step, 0),
                            'k' => (0, -step),
                            _ => (0, step),
                        };
                        crop.nudge(dx, dy);
                    }
                    _ => {}
                }
                let prompt = crop.prompt();
                cx.editor.set_status(prompt);
            }
        }
        EventResult::Consumed(None)
    }

    /// `s` then a key: Emacs's image-mode scaling map.
    fn dispatch_s_key(&mut self, cx: &mut Context, key: KeyEvent) -> anyhow::Result<()> {
        match key {
            key!('w') => image_set_scale(cx, 100),
            key!('o') => image_transform_reset_all(cx),
            // `image-transform-reset-to-initial`: back to the size, rotation and
            // scale the image was first displayed at. Emacs restores
            // `image-auto-resize` (fit-to-window by default) plus rotation 0 and
            // scale 1; the terminal viewer always fits the picture to the window,
            // so the initial state here is exactly "no rotation, no flip, 100%" —
            // which is why it coincides with `s o` (`-reset-to-original`).
            key!('0') => image_transform_reset_all(cx),
            key!('p') => {
                self.begin_input("Scale (% of original): ", Pending::SetPercent);
                Ok(())
            }
            key!('s') => {
                self.begin_input("Scale: ", Pending::SetScale);
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

impl Area {
    /// `image-crop--width` (image-crop.el:288-290).
    fn width(&self) -> i64 {
        self.right - self.left
    }

    /// `image-crop--height` (image-crop.el:291-292).
    fn height(&self) -> i64 {
        self.bottom - self.top
    }

    /// `s`: shrink to a square of the shorter side, keeping the top-left corner
    /// (image-crop.el:313-315).
    fn square(self) -> Self {
        let size = self.width().min(self.height());
        Self {
            right: self.left + size,
            bottom: self.top + size,
            ..self
        }
    }

    /// Confine the rectangle to an image `size` pixels across. Emacs's rectangle
    /// cannot leave the image either: the loop only acts on events whose
    /// position is over the SVG being tracked (image-crop.el:321-327).
    fn clamped(self, size: Option<(u32, u32)>) -> Self {
        let Some((w, h)) = size else { return self };
        let (w, h) = (w as i64, h as i64);
        Self {
            left: self.left.clamp(0, w),
            right: self.right.clamp(0, w),
            top: self.top.clamp(0, h),
            bottom: self.bottom.clamp(0, h),
        }
    }

    /// The region `image-crop--crop-image-update` hands the crop/cut command
    /// (image-crop.el:254-266): every coordinate scaled by `factor`, the size
    /// taken as an absolute difference and the origin as the smaller of the two
    /// corners, each truncated towards zero.
    fn to_original(self, factor: f64) -> Region {
        let trunc = |v: f64| v.trunc() as i64;
        Region {
            w: trunc(factor * (self.right - self.left) as f64).unsigned_abs() as u32,
            h: trunc(factor * (self.bottom - self.top) as f64).unsigned_abs() as u32,
            x: trunc(factor * self.left.min(self.right) as f64),
            y: trunc(factor * self.top.min(self.bottom) as f64),
        }
    }
}

impl Region {
    /// Confine the region to a `w`×`h` image. ImageMagick clips an out-of-bounds
    /// `-crop` to the canvas anyway, so this is what makes the geometry zmax
    /// reports the geometry it actually produced.
    fn clamped(self, w: u32, h: u32) -> Self {
        let x = self.x.clamp(0, w as i64);
        let y = self.y.clamp(0, h as i64);
        Self {
            w: self.w.min((w as i64 - x) as u32),
            h: self.h.min((h as i64 - y) as u32),
            x,
            y,
        }
    }

    /// The `WxH+X+Y` ImageMagick geometry, `image-crop-crop-command`'s
    /// `%wx%h+%l+%t` (image-crop.el:72-73).
    fn geometry(&self) -> String {
        format!("{}x{}+{}+{}", self.w, self.h, self.x, self.y)
    }
}

impl Crop {
    /// Move a corner, or the whole rectangle in `m`/`s` mode. Emacs's `stretch`
    /// state moves `:right`/`:bottom` with the pointer (image-crop.el:341-343)
    /// and its `move-click` state translates the rectangle, size kept
    /// (image-crop.el:381-389).
    fn nudge(&mut self, dx: i64, dy: i64) {
        let a = self.area;
        self.area = if self.moving {
            let (w, h) = (a.width(), a.height());
            Area {
                left: a.left + dx,
                top: a.top + dy,
                right: a.left + dx + w,
                bottom: a.top + dy + h,
            }
        } else {
            Area {
                right: a.right + dx,
                bottom: a.bottom + dy,
                ..a
            }
        }
        .clamped(self.size);
    }

    /// The line shown while the session is up — Emacs's own prompts, which name
    /// the operation and the key that ends it (image-crop.el:317-319, 346-351).
    fn prompt(&self) -> String {
        let op = if self.cut.is_some() { "cut" } else { "crop" };
        let geom = self.area.to_original(1.0).geometry();
        if self.moving {
            format!("Move for {op} [{geom}] (hjkl/arrows, RET, q)")
        } else {
            format!("Type RET to {op}, or hjkl/arrows to adjust corners [{geom}] (m, s, q)")
        }
    }
}

/// Parse the number one of the scaling prompts read, naming the command in the
/// error the way the typable of the same name does.
fn parse_number(arg: &str, cmd: &str) -> anyhow::Result<f64> {
    arg.trim()
        .parse::<f64>()
        .map_err(|_| anyhow::anyhow!("{cmd}: not a number: {}", arg.trim()))
}

/// A percentage from a prompt, rejecting the non-positive values Emacs rejects
/// ("Not a positive number: %s") and clamping to the range `image_set_scale`
/// accepts.
fn scale_percent(pct: f64) -> anyhow::Result<u32> {
    if !(pct.is_finite() && pct > 0.0) {
        bail!("Not a positive number: {pct}");
    }
    Ok((pct.round() as i64).clamp(1, 1000) as u32)
}

/// The current image's scale, or 100% when there is no image (the callers then
/// fail with the same "not an image file" error the typables give).
fn current_scale(cx: &Context) -> u32 {
    current_image_path(cx)
        .map(|p| image_xform_of(&p).3)
        .unwrap_or(100)
}

/// `factor` in `image-crop--crop-image-update` (image-crop.el:257): the image's
/// own width divided by the width it is displayed at. zmax's display scale is a
/// percentage of the original, so the ratio is its reciprocal.
fn display_factor(scale: u32) -> f64 {
    100.0 / scale.max(1) as f64
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

/// Replace the animation state, discarding whatever temp frame or re-encoded
/// animation the old one held.
fn set_anim(next: Option<Anim>) {
    let old = std::mem::replace(&mut *IMAGE_ANIM.lock().unwrap(), next);
    if let Some(stale) = old.and_then(|a| a.file) {
        let _ = std::fs::remove_file(stale);
    }
}

/// The image's current bytes: the pending crop/cut result if there is one, else
/// the file itself. This is what Emacs calls the image *in the buffer*.
fn image_data_path(orig: &Path) -> PathBuf {
    slot_for(&IMAGE_EDIT, orig).unwrap_or_else(|| orig.to_path_buf())
}

/// The file to hand the viewer: the still frame or resumed animation the last
/// `image-toggle-animation` left, else the image's current bytes.
fn shown_path(orig: &Path) -> PathBuf {
    let animated = match &*IMAGE_ANIM.lock().unwrap() {
        Some(a) if a.orig.as_path() == orig => a.file.clone(),
        _ => None,
    };
    animated.unwrap_or_else(|| image_data_path(orig))
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
/// same two names the display pipeline falls back through. Emacs's own error
/// when neither is installed is image-crop.el:163-166.
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

/// The first line of `identify -ping -format FORMAT PATH`, probing both
/// ImageMagick 7's `magick identify` and the standalone `identify` of 6.
/// Multi-frame files print one line per frame, so the first line is the answer
/// for the image as a whole.
fn identify_first(path: &Path, format: &str) -> Option<String> {
    for argv in [&["magick", "identify"][..], &["identify"][..]] {
        match std::process::Command::new(argv[0])
            .args(&argv[1..])
            .args(["-ping", "-format", format])
            .arg(path)
            .output()
        {
            Ok(out) if out.status.success() => {
                return String::from_utf8_lossy(&out.stdout)
                    .lines()
                    .next()
                    .map(|l| l.trim().to_string());
            }
            _ => continue,
        }
    }
    None
}

/// The image's frame count (Emacs `image-multi-frame-p`, image.el:912-925), read
/// with ImageMagick `identify %n`. `None` when neither identify name is
/// available.
fn frame_count(path: &Path) -> Option<u32> {
    identify_first(path, "%n\n")?.parse().ok()
}

/// The image's own pixel size — the page canvas (`%W`/`%H`), not the frame's own
/// extent, which on an optimised animation is only the part that changed.
fn image_size(path: &Path) -> Option<(u32, u32)> {
    let line = identify_first(path, "%W %H\n")?;
    let (w, h) = line.split_once(' ')?;
    Some((w.trim().parse().ok()?, h.trim().parse().ok()?))
}

/// The size an image of `size` is on screen: transposed by a quarter-turn
/// rotation and taken to `scale` percent. This is the space Emacs's crop
/// rectangle lives in — `(image-size image t)` returns the *transformed* size
/// (image-crop.el:194) — so it is the space a typed geometry is read in.
fn transform_size(size: (u32, u32), rotate: i32, scale: u32) -> (u32, u32) {
    let (w, h) = if rotate.rem_euclid(180) == 90 {
        (size.1, size.0)
    } else {
        size
    };
    let at = |v: u32| ((v as u64 * scale as u64) / 100).max(1) as u32;
    (at(w), at(h))
}

/// [`transform_size`] of the image in `path`, when `identify` can measure it.
fn displayed_size(path: &Path, rotate: i32, scale: u32) -> Option<(u32, u32)> {
    Some(transform_size(image_size(path)?, rotate, scale))
}

/// Parse an ImageMagick region — `WxH+X+Y`, or `WxH` at the origin.
fn parse_geometry(spec: &str) -> Option<Region> {
    let (size, offset) = match spec.find('+') {
        Some(i) => (&spec[..i], &spec[i + 1..]),
        None => (spec, ""),
    };
    let (w, h) = size.split_once(['x', 'X'])?;
    let (x, y) = match offset {
        "" => ("0", "0"),
        o => o.split_once('+')?,
    };
    Some(Region {
        w: w.trim().parse().ok()?,
        h: h.trim().parse().ok()?,
        x: x.trim().parse().ok()?,
        y: y.trim().parse().ok()?,
    })
}

/// `image-crop-crop-command` — `("convert" "+repage" "-crop" "%wx%h+%l+%t" "-"
/// "%f:-")` (image-crop.el:72-73). Emacs pipes the image through stdin and names
/// the output format with `%f:-`; zmax has the bytes in a file, so the `-` /
/// `%f:-` pair becomes the input and output paths and the format follows the
/// extension.
fn crop_args(src: &Path, region: Region, out: &Path) -> Vec<String> {
    vec![
        src.to_string_lossy().into_owned(),
        "+repage".into(),
        "-crop".into(),
        region.geometry(),
        out.to_string_lossy().into_owned(),
    ]
}

/// `image-crop-cut-command` — `("convert" "-draw" "rectangle %l,%t %r,%b"
/// "-fill" "%c" "-" "%f:-")` (image-crop.el:57-59), with `%r`/`%b` the far corner
/// (image-crop.el:275-276).
///
/// The argument order is Emacs's, and it has a consequence worth stating: an
/// ImageMagick setting applies to the operators that *follow* it, so a `-fill`
/// written after the `-draw` it is meant to colour arrives too late and the
/// rectangle is drawn in the default black. `image-cut`'s COLOR argument
/// therefore has no effect on the result in Emacs, and none here. Verified on
/// ImageMagick 7.1.2-30: `magick p0.png -draw 'rectangle 20,20 40,40' -fill blue
/// e.png` leaves pixel (30,30) `srgb(0,0,0)`, while the same command with
/// `-fill blue` moved in front of `-draw` leaves it `srgb(0,0,255)`.
fn cut_args(src: &Path, region: Region, color: &str, out: &Path) -> Vec<String> {
    let (l, t) = (region.x, region.y);
    let (r, b) = (l + region.w as i64, t + region.h as i64);
    vec![
        src.to_string_lossy().into_owned(),
        "-draw".into(),
        format!("rectangle {l},{t} {r},{b}"),
        "-fill".into(),
        color.into(),
        out.to_string_lossy().into_owned(),
    ]
}

/// `image-crop--possibly-rotate-buffer` (image-crop.el:422-455): the image's
/// display rotation is baked into the data before the region is cut out of it,
/// so the region means what it covers on screen. Returns the file to crop and
/// whether it is a temp this call created.
///
/// Emacs rotates only (its `image-crop-rotate-command` is `convert -rotate %r`,
/// image-crop.el:85); zmax's `image-flip-horizontally` / `-vertically` are
/// display state in exactly the same way, and leaving them out would make a
/// region named over a flipped picture mean the mirror image of itself, so they
/// are baked with the same call. Emacs has no flip to bake.
fn bake_transform(orig: &Path, rotate: i32, flip_h: bool, flip_v: bool) -> anyhow::Result<PathBuf> {
    let src = image_data_path(orig);
    let deg = rotate.rem_euclid(360);
    if deg == 0 && !flip_h && !flip_v {
        return Ok(src);
    }
    let out = edit_temp(orig);
    let mut args = vec![src.to_string_lossy().into_owned()];
    if deg != 0 {
        args.push("-rotate".into());
        args.push(deg.to_string());
    }
    if flip_h {
        args.push("-flop".into());
    }
    if flip_v {
        args.push("-flip".into());
    }
    args.push(out.to_string_lossy().into_owned());
    magick(&args)?;
    Ok(out)
}

/// Execute the session `i c` / `i x` opened: run the region through the matching
/// `image-crop-*-command` and make the result the image's pending, unsaved bytes
/// (`image-crop--crop-image-update`, image-crop.el:254-286).
fn run_crop(cx: &mut Context, crop: Crop) -> anyhow::Result<()> {
    let orig = crop.orig;
    if current_image_path(cx).as_deref() != Some(orig.as_path()) {
        bail!("No image under point");
    }
    let name = if crop.cut.is_some() {
        "image-cut"
    } else {
        "image-crop"
    };
    let (rot, flip_h, flip_v, _) = image_xform_of(&orig);
    let src = bake_transform(&orig, rot, flip_h, flip_v)?;
    let baked = src != image_data_path(&orig);

    let mut region = crop.area.to_original(crop.factor);
    if let Some((w, h)) = image_size(&src) {
        region = region.clamped(w, h);
    }
    let out = edit_temp(&orig);
    let done = if region.w == 0 || region.h == 0 {
        Err(anyhow::anyhow!("{name}: region has no area"))
    } else if let Some(color) = &crop.cut {
        magick(&cut_args(&src, region, color, &out))
    } else {
        magick(&crop_args(&src, region, &out))
    };
    if baked {
        let _ = std::fs::remove_file(&src);
    }
    done?;

    // The rotation and flips are in the data now, so the display must stop
    // applying them — Emacs's cropped image is inserted with no :rotation
    // either (image-crop.el:462-468). `image_transform` toggles the flips, so
    // passing the current ones back clears them.
    if rot != 0 || flip_h || flip_v {
        image_transform(cx, -rot, flip_h, flip_v)?;
    }
    set_slot(&IMAGE_EDIT, &orig, Some(out));
    // A frozen frame or resumed animation was built from the pre-edit bytes, so
    // it is stale now.
    set_anim(None);
    redisplay(cx, &orig);
    // "Type \\[image-save] to save %s image to file" (image-crop.el:241-243),
    // where `i o` is the key that runs `image-save` here.
    let what = if crop.cut.is_some() { "cut" } else { "cropped" };
    cx.editor.set_status(format!(
        "Type i o to save {what} image to file [{}]",
        region.geometry()
    ));
    Ok(())
}

/// `image-save`: write the image's data to a file you name. Emacs saves the
/// original bytes — "Rotating or changing the displayed image size does not
/// affect the saved image" — so the rotate/flip/scale state is deliberately not
/// applied here; a pending crop/cut *is* the data and so is written.
pub(crate) fn image_save(cx: &mut Context, dest: &str) -> anyhow::Result<()> {
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

/// The frame `image-toggle-animation` restarts from: the one the image is on,
/// unless that is the last one, in which case the animation starts over
/// (image-mode.el:1127-1131 — "If we're at the end, restart" clears the index,
/// and `image-animate` then takes `(or index 0)`, image.el:957).
fn resume_index(index: u32, count: u32) -> u32 {
    if index >= count.saturating_sub(1) {
        0
    } else {
        index
    }
}

/// `image-show-frame`'s range check (image.el:984-986): "if (< n 0) (setq n 0)"
/// else clamp to the last frame.
fn clamp_frame(n: i64, count: u32) -> u32 {
    n.clamp(0, count.saturating_sub(1) as i64) as u32
}

/// The GIF loop count for `image-animate`'s LIMIT: t (loop forever) is
/// ImageMagick's `-loop 0`, and nil — "play the animation until the end"
/// (image.el:939-941), which is `image-animate-loop`'s nil default — is one pass.
fn loop_count(loop_forever: bool) -> u32 {
    if loop_forever {
        0
    } else {
        1
    }
}

/// The ImageMagick arguments that select frame `index` of an animation.
///
/// `-coalesce` first, because the frames of an optimised animation are deltas:
/// reading `file.gif[2]` directly hands back only the part that changed. Checked
/// on ImageMagick 7.1.2-30 against a three-frame optimised GIF — `magick
/// 'opt.gif[2]' g.png` produced a 31x11 fragment, while the arguments below
/// produced the full 60x60 frame.
fn still_args(src: &Path, index: u32, out: &Path) -> Vec<String> {
    let mut args = vec![src.to_string_lossy().into_owned(), "-coalesce".into()];
    if index > 0 {
        args.push("-delete".into());
        args.push(format!("0-{}", index - 1));
    }
    // Everything after the frame that is now first.
    args.push("-delete".into());
    args.push("1--1".into());
    args.push(out.to_string_lossy().into_owned());
    args
}

/// The ImageMagick arguments for an animation that starts at frame `from` and
/// plays `loops` times (0 = forever) — `image-animate`'s INDEX and LIMIT
/// (image.el:934-958) expressed in the only terms an external viewer takes.
fn animate_args(src: &Path, from: u32, loops: u32, out: &Path) -> Vec<String> {
    let mut args = vec![src.to_string_lossy().into_owned(), "-coalesce".into()];
    if from > 0 {
        args.push("-delete".into());
        args.push(format!("0-{}", from - 1));
    }
    args.push("-loop".into());
    args.push(loops.to_string());
    args.push(out.to_string_lossy().into_owned());
    args
}

/// `image-toggle-animation`: "Start or stop animating the current image"
/// (image-mode.el:1111-1133).
///
/// Emacs stops by cancelling the timer, which leaves the image displaying the
/// frame it had reached (image-mode.el:1124-1126); it restarts from that frame,
/// or from the beginning if it was already on the last one. zmax keeps the same
/// index and hands the viewer a still of that frame, or an animation re-encoded
/// to start there.
///
/// What the index can be known to be is the limit here. The viewer owns playback
/// during a blocking tty handoff, so a *looping* animation's position when the
/// user dismissed it is unobservable and the index is left where it was; a
/// non-looping one (`image-animate-loop` nil, the default) ran to its last frame
/// and stopped there, which is what image.el:1072-1075 does with a nil LIMIT.
fn toggle_animation(cx: &mut Context) -> anyhow::Result<()> {
    let Some(orig) = current_image_path(cx) else {
        // image-mode.el:1119-1120.
        bail!("No image is present");
    };
    let src = image_data_path(&orig);
    // Nothing toggled yet: the viewer starts animated files playing, so that is
    // the state to toggle out of. (Emacs's own initial state is stopped on frame
    // 0 — `image-mode` displays a multi-frame image without starting a timer,
    // image-mode.el:733-755 — but the first display here is queued by
    // `commands::typed::ex_image_display`, which hands the viewer the raw file.)
    let (playing, index, known) = match &*IMAGE_ANIM.lock().unwrap() {
        Some(a) if a.orig == orig => (a.playing, a.index, Some(a.count)),
        _ => (true, 0, None),
    };
    // `image-multi-frame-p` returns nil for a single-frame image, and
    // image-mode.el:1121-1122 then just says so. The count was measured when the
    // state was recorded and the image's data has not changed since — a crop or
    // cut clears the state — so it is only probed for an image with none.
    let count = match known {
        Some(c) => c,
        None => frame_count(&src).unwrap_or(1),
    };
    if count < 2 {
        cx.editor.set_status("No image animation.");
        return Ok(());
    }
    let index = clamp_frame(index as i64, count);

    if playing {
        let still = edit_temp(&orig);
        magick(&still_args(&src, index, &still))?;
        set_anim(Some(Anim {
            orig: orig.clone(),
            count,
            index,
            playing: false,
            file: Some(still),
        }));
        redisplay(cx, &orig);
        // image-mode shows the frame in the mode line as `[N/count]`
        // (image-mode.el:744-746), counting from 1.
        cx.editor
            .set_status(format!("image: stopped on frame [{}/{count}]", index + 1));
    } else {
        let start = resume_index(index, count);
        let loops = loop_count(IMAGE_ANIMATE_LOOP);
        // Frame 0 played forever is what the viewer does with the file itself.
        let file = if start == 0 && loops == 0 {
            None
        } else {
            let anim = edit_temp(&orig);
            magick(&animate_args(&src, start, loops, &anim))?;
            Some(anim)
        };
        set_anim(Some(Anim {
            orig: orig.clone(),
            count,
            // Where the pass this starts will leave the image.
            index: if loops == 0 { start } else { count - 1 },
            playing: true,
            file,
        }));
        redisplay(cx, &orig);
        cx.editor
            .set_status(format!("image: animating from frame [{}/{count}]", start + 1));
    }
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
        // So does a crop/cut rectangle session: Emacs's is a `read-event` loop
        // that only `RET` and `q` leave (image-crop.el:307, 397-399).
        if self.crop.is_some() {
            return self.handle_crop_key(key, cx);
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
            // `image-mode-mark-file` / `image-mode-unmark-file`: (un)mark the
            // visited file in the Dired listing of its directory.
            key!('m') => {
                let done = crate::emacs_image::mark_visited_file(cx, true);
                report(cx, done);
            }
            key!('u') => {
                let done = crate::emacs_image::mark_visited_file(cx, false);
                report(cx, done);
            }
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
    /// is already holding. The exceptions are a minibuffer read and a crop
    /// session, which have to show what has been typed and where the rectangle
    /// is.
    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        if area.height == 0 {
            return;
        }
        let line = match (&self.input, &self.crop) {
            (Some(inp), _) => format!("{}{}", inp.prompt, inp.buffer),
            (None, Some(crop)) => crop.prompt(),
            (None, None) => return,
        };
        let style = ctx.editor.theme.get("ui.text");
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

#[cfg(test)]
mod tests {
    use super::*;

    /// `image-crop-crop-command`'s `%wx%h+%l+%t` (image-crop.el:72-73) is the
    /// spelling a region is both read and written in, so the two have to agree.
    #[test]
    fn geometry_round_trips() {
        let r = parse_geometry("120x80+10+5").unwrap();
        assert_eq!(
            r,
            Region {
                w: 120,
                h: 80,
                x: 10,
                y: 5
            }
        );
        assert_eq!(r.geometry(), "120x80+10+5");
        // `WxH` alone is the same region at the origin.
        assert_eq!(parse_geometry("64x64").unwrap().geometry(), "64x64+0+0");
        // Not a geometry at all.
        assert!(parse_geometry("").is_none());
        assert!(parse_geometry("120+10+5").is_none());
        assert!(parse_geometry("120x80+10").is_none());
    }

    /// `image-crop--crop-image-update` takes the size as an absolute difference
    /// and the origin as the *smaller* of the two corners
    /// (image-crop.el:259-266), so a rectangle grown up and to the left names
    /// the same region as one grown down and to the right.
    #[test]
    fn area_normalises_a_backwards_rectangle() {
        let forwards = Area {
            left: 10,
            top: 20,
            right: 110,
            bottom: 70,
        };
        let backwards = Area {
            left: 110,
            top: 70,
            right: 10,
            bottom: 20,
        };
        assert_eq!(forwards.to_original(1.0).geometry(), "100x50+10+20");
        assert_eq!(
            backwards.to_original(1.0),
            forwards.to_original(1.0),
            "the corners are unordered until the region is built"
        );
        // …and the signed width/height they are built from are not
        // (image-crop.el:288-292).
        assert_eq!(backwards.width(), -100);
        assert_eq!(backwards.height(), -50);
    }

    /// `factor` is the image's own width over the width it is displayed at, and
    /// every coordinate is multiplied by it (image-crop.el:257-266). Displayed
    /// at 50%, a rectangle covers twice as many of the image's own pixels.
    #[test]
    fn factor_scales_the_region_to_the_original() {
        let area = Area {
            left: 10,
            top: 20,
            right: 110,
            bottom: 70,
        };
        assert_eq!(display_factor(50), 2.0);
        assert_eq!(area.to_original(display_factor(50)).geometry(), "200x100+20+40");
        // 100% is the identity, which is what the geometry means with no
        // scaling in play.
        assert_eq!(display_factor(100), 1.0);
        assert_eq!(area.to_original(display_factor(100)).geometry(), "100x50+10+20");
        // Emacs truncates towards zero at every step (image-crop.el:259-266):
        // at 300% a 100px-wide rectangle is 33, not 34.
        assert_eq!(
            area.to_original(display_factor(300)).geometry(),
            "33x16+3+6"
        );
    }

    /// Emacs's rectangle cannot leave the image: the tracking loop ignores
    /// events whose position is not over the SVG (image-crop.el:321-327). A
    /// named one can, so it is clamped in both spaces it lives in.
    #[test]
    fn rectangles_are_confined_to_the_image() {
        let over = Area {
            left: -5,
            top: -5,
            right: 200,
            bottom: 200,
        };
        assert_eq!(
            over.clamped(Some((100, 80))),
            Area {
                left: 0,
                top: 0,
                right: 100,
                bottom: 80
            }
        );
        // With no measurement of the image, nothing is clamped rather than
        // guessed — ImageMagick clips the crop itself.
        assert_eq!(over.clamped(None), over);
        // In the image's own space, the origin is clamped first and the size
        // then trimmed to what is left of the image.
        let r = Region {
            w: 100,
            h: 100,
            x: 60,
            y: 70,
        };
        assert_eq!(r.clamped(100, 80).geometry(), "40x10+60+70");
        let off = Region {
            w: 50,
            h: 50,
            x: -20,
            y: -20,
        };
        assert_eq!(off.clamped(100, 80).geometry(), "50x50+0+0");
    }

    /// `s` squares the rectangle on its shorter side, keeping the top-left
    /// corner (image-crop.el:313-315).
    #[test]
    fn square_shrinks_to_the_shorter_side() {
        let a = Area {
            left: 10,
            top: 20,
            right: 110,
            bottom: 70,
        };
        assert_eq!(
            a.square(),
            Area {
                left: 10,
                top: 20,
                right: 60,
                bottom: 70
            }
        );
        assert_eq!(a.square().width(), a.square().height());
    }

    /// `image-crop-cut-command` lists `-fill %c` *after* the `-draw` it is meant
    /// to colour (image-crop.el:57-59), and the far corner is left+width,
    /// top+height (image-crop.el:275-276). The order is the reason `image-cut`'s
    /// COLOR never reaches the rectangle — see [`cut_args`] — so pinning it is
    /// pinning the port's fidelity, not an accident of formatting.
    #[test]
    fn cut_command_keeps_emacs_argument_order() {
        let region = Region {
            w: 100,
            h: 50,
            x: 10,
            y: 20,
        };
        let args = cut_args(Path::new("/in.png"), region, "red", Path::new("/out.png"));
        assert_eq!(
            args,
            vec![
                "/in.png",
                "-draw",
                "rectangle 10,20 110,70",
                "-fill",
                "red",
                "/out.png"
            ]
        );
        // `image-crop-crop-command` is `+repage -crop %wx%h+%l+%t`
        // (image-crop.el:72-73).
        assert_eq!(
            crop_args(Path::new("/in.png"), region, Path::new("/out.png")),
            vec!["/in.png", "+repage", "-crop", "100x50+10+20", "/out.png"]
        );
    }

    /// "If we're at the end, restart" (image-mode.el:1127-1131): restarting an
    /// animation resumes at the frame it stopped on unless that is the last one.
    #[test]
    fn restart_resumes_at_the_stopped_frame() {
        assert_eq!(resume_index(0, 5), 0);
        assert_eq!(resume_index(3, 5), 3);
        // The last frame is `(1- (car animation))`, and `>=` sends it back to
        // the start.
        assert_eq!(resume_index(4, 5), 0);
        assert_eq!(resume_index(9, 5), 0);
        // A single-frame image has no other frame to be on.
        assert_eq!(resume_index(0, 1), 0);
    }

    /// `image-show-frame` clamps N into the frames the image has
    /// (image.el:984-986).
    #[test]
    fn frame_numbers_are_clamped_to_the_animation() {
        assert_eq!(clamp_frame(-3, 5), 0);
        assert_eq!(clamp_frame(2, 5), 2);
        assert_eq!(clamp_frame(4, 5), 4);
        assert_eq!(clamp_frame(5, 5), 4);
        assert_eq!(clamp_frame(0, 0), 0);
    }

    /// `image-animate-loop` nil plays the animation once and stops
    /// (image-mode.el:1105-1109, image.el:1072-1075); t loops forever, which is
    /// ImageMagick's `-loop 0`.
    #[test]
    fn loop_count_follows_image_animate_loop() {
        assert_eq!(loop_count(false), 1);
        assert_eq!(loop_count(true), 0);
        assert_eq!(loop_count(IMAGE_ANIMATE_LOOP), 1, "the defcustom's default");
    }

    /// Frame selection has to composite the deltas of an optimised animation
    /// before picking a frame out of it, and resuming has to drop only the
    /// frames before the one being resumed at (image.el:934-958).
    #[test]
    fn frame_selection_coalesces_first() {
        let (src, out) = (Path::new("/a.gif"), Path::new("/o.gif"));
        assert_eq!(
            still_args(src, 0, out),
            vec!["/a.gif", "-coalesce", "-delete", "1--1", "/o.gif"],
            "frame 0 has nothing before it to drop"
        );
        assert_eq!(
            still_args(src, 2, out),
            vec![
                "/a.gif", "-coalesce", "-delete", "0-1", "-delete", "1--1", "/o.gif"
            ]
        );
        assert_eq!(
            animate_args(src, 0, 0, out),
            vec!["/a.gif", "-coalesce", "-loop", "0", "/o.gif"]
        );
        assert_eq!(
            animate_args(src, 3, 1, out),
            vec!["/a.gif", "-coalesce", "-delete", "0-2", "-loop", "1", "/o.gif"]
        );
    }

    /// The rectangle is named in the space the picture is shown in, which a
    /// quarter turn transposes and a scale multiplies — `(image-size image t)`
    /// is the *transformed* size (image-crop.el:194).
    #[test]
    fn displayed_size_follows_the_display_transform() {
        assert_eq!(transform_size((200, 100), 0, 100), (200, 100));
        assert_eq!(transform_size((200, 100), 90, 100), (100, 200));
        assert_eq!(transform_size((200, 100), 270, 100), (100, 200));
        assert_eq!(transform_size((200, 100), 180, 100), (200, 100));
        assert_eq!(transform_size((200, 100), 0, 50), (100, 50));
        // A quarter turn and a scale compose, and nothing ever measures zero.
        assert_eq!(transform_size((200, 100), 90, 25), (25, 50));
        assert_eq!(transform_size((3, 3), 0, 1), (1, 1));
    }

    /// Moving the rectangle keeps its size (image-crop.el:381-389); adjusting a
    /// corner moves `:right`/`:bottom` only (image-crop.el:341-343). Both stay
    /// inside the image.
    #[test]
    fn nudge_moves_a_corner_or_the_whole_rectangle() {
        let mut crop = Crop {
            orig: PathBuf::from("/a.png"),
            cut: None,
            area: Area {
                left: 10,
                top: 10,
                right: 30,
                bottom: 30,
            },
            moving: false,
            size: Some((100, 100)),
            factor: 1.0,
        };
        crop.nudge(5, 5);
        assert_eq!(crop.area.to_original(1.0).geometry(), "25x25+10+10");
        crop.moving = true;
        crop.nudge(5, 5);
        assert_eq!(
            crop.area.to_original(1.0).geometry(),
            "25x25+15+15",
            "moving keeps the size"
        );
        // The rectangle stops at the edge of the image.
        crop.nudge(1000, 1000);
        assert_eq!(crop.area.right, 100);
        assert_eq!(crop.area.bottom, 100);
    }
}
