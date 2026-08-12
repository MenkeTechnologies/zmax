//! A handful of Emacs global/buffer-local minor modes whose state is a flag the
//! rest of the editor reads, kept out of the 70k-line `commands.rs`.
//!
//! Each one here is a real behaviour switch, not a recorded value:
//!
//! * [`toggle_blink_cursor`] flips the DECSCUSR sequence the terminal backends
//!   emit between the steady and the blinking variant of the current shape.
//! * [`erase_translate`] is `normal-erase-is-backspace-mode`: with the mode off
//!   the <backspace> and <delete> key events swap before the keymap sees them.
//! * [`temp_buffer_resize`] is `temp-buffer-resize-mode`: a temporary display
//!   (`show_text_in_scratch`) opens in its own split sized to its contents.
//! * [`use_hard_newlines`] is `use-hard-newlines`: RET/`open-line` mark the
//!   newlines they insert `hard`, and the fill commands refuse to remove one.
//! * [`cua_mode_target`] is `cua-mode`: the keymap preset to switch to, and the
//!   one to come back to when the mode is turned off.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use zmax_view::input::{KeyCode, KeyEvent};
use zmax_view::DocumentId;

// ── blink-cursor-mode ───────────────────────────────────────────────────────

/// Emacs `blink-cursor-mode`: toggle cursor blinking, returning the new state.
/// The flag lives in `zmax-view` because the terminal backends are what turn a
/// `CursorKind` into a DECSCUSR value, and they only see that crate.
pub fn toggle_blink_cursor() -> bool {
    zmax_view::graphics::toggle_cursor_blink()
}

/// Emacs `blink-cursor-mode` with an explicit argument.
pub fn set_blink_cursor(on: bool) {
    zmax_view::graphics::set_cursor_blink(on);
}

/// Whether the cursor is currently blinking.
pub fn blink_cursor() -> bool {
    zmax_view::graphics::cursor_blink()
}

// ── tooltip-mode ────────────────────────────────────────────────────────────

/// Emacs `tooltip-mode`: help text is shown as a tooltip while the mode is on,
/// and "in the echo area" when it is off. zmax's tooltip is a popup and its echo
/// area is the status line, so the mode decides which of the two an LSP hover
/// lands in. Enabled by default, as in Emacs.
static TOOLTIP_MODE: AtomicBool = AtomicBool::new(true);

/// Whether `tooltip-mode` is on.
pub fn tooltip_mode() -> bool {
    TOOLTIP_MODE.load(Ordering::Relaxed)
}

/// Toggle `tooltip-mode`, returning the new state.
pub fn toggle_tooltip_mode() -> bool {
    !TOOLTIP_MODE.fetch_xor(true, Ordering::Relaxed)
}

// ── normal-erase-is-backspace-mode ──────────────────────────────────────────

/// Emacs `normal-erase-is-backspace`: `t` (the default) is the mode where
/// <backspace> erases the character *before* point and <delete> the one after
/// it. `nil` is the other mode the manual's "DEL Does Not Delete" node
/// describes, where <backspace> deletes forward instead.
static NORMAL_ERASE_IS_BACKSPACE: AtomicBool = AtomicBool::new(true);

/// Whether <backspace> erases backwards (Emacs `normal-erase-is-backspace`).
pub fn normal_erase_is_backspace() -> bool {
    NORMAL_ERASE_IS_BACKSPACE.load(Ordering::Relaxed)
}

/// Emacs `M-x normal-erase-is-backspace-mode`: toggle between the two modes,
/// returning the new state.
pub fn toggle_normal_erase_is_backspace() -> bool {
    !NORMAL_ERASE_IS_BACKSPACE.fetch_xor(true, Ordering::Relaxed)
}

/// `normal-erase-is-backspace-mode` with an explicit argument.
pub fn set_normal_erase_is_backspace(on: bool) {
    NORMAL_ERASE_IS_BACKSPACE.store(on, Ordering::Relaxed);
}

/// The key translation the mode performs, applied before the keymap is
/// consulted: with the mode off, <backspace> and <delete> trade places, which is
/// exactly what `(normal-erase-is-backspace-mode 0)` rebinds them to. Pure.
pub fn erase_translate(event: KeyEvent) -> KeyEvent {
    if normal_erase_is_backspace() {
        return event;
    }
    match event.code {
        KeyCode::Backspace => KeyEvent {
            code: KeyCode::Delete,
            ..event
        },
        KeyCode::Delete => KeyEvent {
            code: KeyCode::Backspace,
            ..event
        },
        _ => event,
    }
}

// ── temp-buffer-resize-mode ─────────────────────────────────────────────────

/// Emacs `temp-buffer-resize-mode`: windows showing a temporary display are
/// resized to fit their contents. Off by default, as in Emacs.
static TEMP_BUFFER_RESIZE: AtomicBool = AtomicBool::new(false);

/// Whether `temp-buffer-resize-mode` is on.
pub fn temp_buffer_resize() -> bool {
    TEMP_BUFFER_RESIZE.load(Ordering::Relaxed)
}

/// Toggle `temp-buffer-resize-mode`, returning the new state.
pub fn toggle_temp_buffer_resize() -> bool {
    !TEMP_BUFFER_RESIZE.fetch_xor(true, Ordering::Relaxed)
}

/// Emacs `temp-buffer-max-height` (window.el): the default is
/// `(/ (frame-height) 2)`, and the manual adds that the resized window "cannot
/// exceed the size of the containing frame". `lines` is how many lines the
/// temporary buffer holds, `frame_height` the height available to the window
/// tree. One extra row leaves the buffer's last line clear of the status line.
/// Pure — unit tested.
pub fn temp_buffer_height(lines: usize, frame_height: u16) -> u16 {
    let max = (frame_height / 2).max(1);
    let wanted = u16::try_from(lines.saturating_add(1)).unwrap_or(u16::MAX);
    wanted.clamp(1, max)
}

// ── use-hard-newlines ───────────────────────────────────────────────────────

/// The buffers `use-hard-newlines` is on for — it is a buffer-local mode.
fn hard_newline_docs() -> &'static Mutex<HashSet<DocumentId>> {
    static DOCS: std::sync::OnceLock<Mutex<HashSet<DocumentId>>> = std::sync::OnceLock::new();
    DOCS.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Whether `use-hard-newlines` is on for `doc`.
pub fn use_hard_newlines(doc: DocumentId) -> bool {
    hard_newline_docs()
        .lock()
        .map(|d| d.contains(&doc))
        .unwrap_or(false)
}

/// Emacs `use-hard-newlines`: toggle the mode for `doc`, returning the new state.
pub fn toggle_use_hard_newlines(doc: DocumentId) -> bool {
    let Ok(mut docs) = hard_newline_docs().lock() else {
        return false;
    };
    if docs.remove(&doc) {
        false
    } else {
        docs.insert(doc);
        true
    }
}

/// The char ranges to fill separately when `use-hard-newlines` is on: the
/// region `text` is cut at every hard newline, because "all the fill commands …
/// delete only soft newlines". `hard` holds the char offsets *within `text`* of
/// the newlines that carry the `hard` property.
///
/// Returns half-open ranges covering `text` with no gaps, so joining the filled
/// pieces back with the newlines that separated them reproduces the region.
/// Pure — unit tested.
pub fn hard_newline_segments(len: usize, hard: &[usize]) -> Vec<std::ops::Range<usize>> {
    let mut out = Vec::new();
    let mut start = 0;
    for &pos in hard {
        if pos >= len || pos < start {
            continue;
        }
        // The segment ends *before* the hard newline; the newline itself is the
        // separator and is never touched by the fill.
        out.push(start..pos);
        start = pos + 1;
    }
    out.push(start..len);
    out
}

// ── cua-mode ────────────────────────────────────────────────────────────────

/// The keymap preset that was in force when `cua-mode` was turned on, so
/// turning it off puts it back. `None` means the mode is off.
static CUA_PREVIOUS: Mutex<Option<String>> = Mutex::new(None);

/// Whether `cua-mode` is on.
pub fn cua_mode() -> bool {
    CUA_PREVIOUS.lock().map(|p| p.is_some()).unwrap_or(false)
}

/// Emacs `M-x cua-mode`: toggle it. `current` is the keymap preset in force.
/// Returns the preset to switch to — `"cua"` when turning the mode on, and the
/// preset that was displaced when turning it off.
pub fn toggle_cua_mode(current: &str) -> String {
    let Ok(mut prev) = CUA_PREVIOUS.lock() else {
        return current.to_string();
    };
    match prev.take() {
        // Turning it off: back to whatever was displaced.
        Some(previous) => previous,
        // Turning it on: remember what to come back to. A `cua` preset already
        // in force means the mode was on before this process asked, so there is
        // nothing sensible to restore but the emacs map it is built from.
        None => {
            *prev = Some(if current == "cua" {
                "emacs".to_string()
            } else {
                current.to_string()
            });
            "cua".to_string()
        }
    }
}

// ── debbugs-browse-mode ─────────────────────────────────────────────────────

/// The GNU debbugs bug page, which is where `debbugs-browse-mode` sends a
/// `Bug#NNN` reference (`debbugs-browse-url` in debbugs.el).
pub const DEBBUGS_URL_FORMAT: &str = "https://debbugs.gnu.org/cgi/bugreport.cgi?bug=%s";

/// Whether `debbugs-browse-mode` is on.
static DEBBUGS_BROWSE: AtomicBool = AtomicBool::new(false);

/// Whether `debbugs-browse-mode` is on.
pub fn debbugs_browse() -> bool {
    DEBBUGS_BROWSE.load(Ordering::Relaxed)
}

/// Toggle `debbugs-browse-mode`, returning the new state.
pub fn toggle_debbugs_browse() -> bool {
    !DEBBUGS_BROWSE.fetch_xor(true, Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn erase_translate_swaps_only_when_the_mode_is_off() {
        let bs = KeyEvent {
            code: KeyCode::Backspace,
            modifiers: zmax_view::input::KeyModifiers::NONE,
        };
        let del = KeyEvent {
            code: KeyCode::Delete,
            modifiers: zmax_view::input::KeyModifiers::NONE,
        };
        set_normal_erase_is_backspace(true);
        assert_eq!(erase_translate(bs), bs);
        assert_eq!(erase_translate(del), del);
        set_normal_erase_is_backspace(false);
        assert_eq!(erase_translate(bs), del);
        assert_eq!(erase_translate(del), bs);
        // Any other key is untouched in either mode.
        let a = KeyEvent {
            code: KeyCode::Char('a'),
            modifiers: zmax_view::input::KeyModifiers::NONE,
        };
        assert_eq!(erase_translate(a), a);
        set_normal_erase_is_backspace(true);
    }

    #[test]
    fn temp_buffer_height_is_capped_at_half_the_frame() {
        // Content shorter than the cap gets its own height plus one row.
        assert_eq!(temp_buffer_height(3, 40), 4);
        // Content longer than `temp-buffer-max-height` is clamped.
        assert_eq!(temp_buffer_height(500, 40), 20);
        // A one-line frame still yields a usable window.
        assert_eq!(temp_buffer_height(0, 1), 1);
    }

    #[test]
    fn hard_newlines_cut_the_fill_region() {
        // "aaa\nbbb\nccc" with the newline at 7 hard: two fill regions.
        assert_eq!(hard_newline_segments(11, &[7]), vec![0..7, 8..11]);
        // No hard newline: the whole region is filled as one paragraph.
        assert_eq!(hard_newline_segments(11, &[]), vec![0..11]);
        // Two adjacent hard newlines leave an empty segment between them, which
        // preserves the blank line the fill must not swallow.
        assert_eq!(hard_newline_segments(5, &[1, 2]), vec![0..1, 2..2, 3..5]);
        // Offsets past the region are ignored.
        assert_eq!(hard_newline_segments(4, &[9]), vec![0..4]);
    }

    #[test]
    fn cua_mode_round_trips_the_previous_preset() {
        // Off -> on remembers the displaced preset...
        assert_eq!(toggle_cua_mode("spacemacs"), "cua");
        assert!(cua_mode());
        // ...and on -> off restores it.
        assert_eq!(toggle_cua_mode("cua"), "spacemacs");
        assert!(!cua_mode());
        // Turning it on while `cua` is already the preset falls back to emacs.
        assert_eq!(toggle_cua_mode("cua"), "cua");
        assert_eq!(toggle_cua_mode("cua"), "emacs");
    }
}
