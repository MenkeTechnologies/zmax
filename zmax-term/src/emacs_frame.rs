//! Emacs frame furniture: the window-manager side of a frame (fullscreen,
//! maximized, iconified) and the bars that surround the text — menu bar, tool
//! bar, modifier bar, window tool bar, scroll bars and window dividers.
//!
//! A terminal *is* a frame: the emulator owns a real window, and the xterm
//! window-manipulation sequences (`CSI … t`, XTWINOPS) drive it — `CSI 10;2t`
//! toggles full-screen, `CSI 9;1t` maximizes, `CSI 2t` iconifies. So
//! `toggle-frame-fullscreen` and friends are not bookkeeping here; they move the
//! terminal's own window. Emulators that do not implement XTWINOPS ignore the
//! sequence, which is why the state is also tracked: the toggle still round-trips
//! and `frame-parameters` style queries still answer.
//!
//! The bars are rows and columns taken out of the text area. The reservation
//! itself lives in `zmax_view::view` (that is where window geometry is computed,
//! so a reserved cell really stops being the text's); this module owns the state
//! the mode commands toggle and the button tables the renderer draws.

use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};
use std::sync::Mutex;

use zmax_view::input::KeyModifiers;
use zmax_view::view::ScrollBarSide;

// ── the frame's own window (XTWINOPS) ────────────────────────────────────────

/// Write one xterm window-manipulation sequence (`CSI <args> t`) to the
/// terminal. Emulators that do not implement XTWINOPS ignore it.
fn emit_winop(args: &str) {
    use std::io::Write;
    let mut out = std::io::stdout();
    let _ = write!(out, "\x1b[{args}t");
    let _ = out.flush();
}

static FULLSCREEN: AtomicBool = AtomicBool::new(false);
static MAXIMIZED: AtomicBool = AtomicBool::new(false);
static ICONIFIED: AtomicBool = AtomicBool::new(false);

/// emacs `toggle-frame-fullscreen`: flip the frame between `fullboth` and nil.
/// `CSI 10;2t` is XTWINOPS "toggle full-screen"; the returned bool is the new
/// state.
pub fn toggle_fullscreen() -> bool {
    let on = !FULLSCREEN.fetch_xor(true, Ordering::Relaxed);
    emit_winop("10;2");
    on
}

/// emacs `toggle-frame-maximized`. `CSI 9;1t` maximizes, `CSI 9;0t` restores.
pub fn toggle_maximized() -> bool {
    let on = !MAXIMIZED.fetch_xor(true, Ordering::Relaxed);
    emit_winop(if on { "9;1" } else { "9;0" });
    on
}

/// emacs `iconify-or-deiconify-frame` (`C-z` under a window system). `CSI 2t`
/// iconifies, `CSI 1t` de-iconifies.
pub fn toggle_iconified() -> bool {
    let on = !ICONIFIED.fetch_xor(true, Ordering::Relaxed);
    emit_winop(if on { "2" } else { "1" });
    on
}

// ── menu bar / tool bar / modifier bar ───────────────────────────────────────

/// emacs `menu-bar-mode`. Off until asked for: emacs -nw spends a row on it by
/// default, zmax does not, and `menu-bar-open` (`F10`) reaches the same menu
/// without one.
static MENU_BAR: AtomicBool = AtomicBool::new(false);
/// emacs `tool-bar-mode`.
static TOOL_BAR: AtomicBool = AtomicBool::new(false);
/// emacs `modifier-bar-mode`.
static MODIFIER_BAR: AtomicBool = AtomicBool::new(false);
/// emacs `window-divider-mode`.
static WINDOW_DIVIDER: AtomicBool = AtomicBool::new(true);
/// Modifiers latched by a click on the modifier bar, applied to the next key.
static STICKY_MODIFIERS: AtomicU8 = AtomicU8::new(0);
/// The `tab-bar-select-tab-modifiers` currently installed, for reporting.
static SELECT_TAB_MODIFIERS: Mutex<String> = Mutex::new(String::new());

/// Define the setter/getter pair for one of the boolean modes above.
macro_rules! flag {
    ($set:ident, $get:ident, $cell:ident) => {
        #[doc = concat!("Set the `", stringify!($cell), "` mode.")]
        pub fn $set(on: bool) {
            $cell.store(on, Ordering::Relaxed);
        }
        #[doc = concat!("Whether the `", stringify!($cell), "` mode is on.")]
        pub fn $get() -> bool {
            $cell.load(Ordering::Relaxed)
        }
    };
}

flag!(set_menu_bar, menu_bar, MENU_BAR);
flag!(set_tool_bar, tool_bar, TOOL_BAR);
flag!(set_modifier_bar, modifier_bar, MODIFIER_BAR);
flag!(set_window_divider, window_divider, WINDOW_DIVIDER);

/// Rows the frame-wide bars take off the top of the editor area: the menu bar,
/// the tool bar and the modifier bar, in emacs's own top-to-bottom order.
pub fn frame_bar_rows() -> u16 {
    u16::from(menu_bar()) + u16::from(tool_bar()) + u16::from(modifier_bar())
}

/// One tool-bar button: its label and the command it runs, spelled the way
/// `MappableCommand::from_str` reads it (a bare name is a static command, a
/// leading `:` a typable one).
pub type BarButton = (&'static str, &'static str);

/// emacs `tool-bar-map`'s default buttons, as labels plus the zmax command each
/// runs. Emacs draws icons; a terminal draws the same actions as words.
pub const TOOL_BAR_BUTTONS: &[BarButton] = &[
    ("New", ":new"),
    ("Open", "file_picker"),
    ("Save", ":write"),
    ("Undo", "undo"),
    ("Redo", "redo"),
    ("Cut", "delete_selection"),
    ("Copy", "copy_region_as_kill"),
    ("Paste", "yank_from_kill_ring"),
    ("Search", "search"),
    ("Replace", "query_replace"),
];

/// emacs `window-tool-bar-mode`: the per-window tool bar. Its buttons are the
/// buffer's own verbs rather than the frame-wide file commands.
pub const WINDOW_TOOL_BAR_BUTTONS: &[BarButton] = &[
    ("Save", ":write"),
    ("Undo", "undo"),
    ("Redo", "redo"),
    ("Format", "format_selections"),
    ("Def", "goto_definition"),
    ("Refs", "goto_reference"),
    ("Diag", "diagnostics_picker"),
];

/// emacs `modifier-bar-mode`: one button per modifier key. Clicking a button
/// applies that modifier to the next key zmax reads.
pub const MODIFIER_BAR_BUTTONS: &[(&str, KeyModifiers)] = &[
    ("Ctrl", KeyModifiers::CONTROL),
    ("Meta", KeyModifiers::ALT),
    ("Shift", KeyModifiers::SHIFT),
    ("Super", KeyModifiers::SUPER),
];

/// Latch (or un-latch) a modifier clicked on the modifier bar.
pub fn toggle_sticky_modifier(m: KeyModifiers) {
    let cur = KeyModifiers::from_bits_truncate(STICKY_MODIFIERS.load(Ordering::Relaxed));
    let next = cur ^ m;
    STICKY_MODIFIERS.store(next.bits(), Ordering::Relaxed);
}

/// The modifiers currently latched by the modifier bar (for drawing them lit).
pub fn sticky_modifiers() -> KeyModifiers {
    KeyModifiers::from_bits_truncate(STICKY_MODIFIERS.load(Ordering::Relaxed))
}

/// Take the latched modifiers, clearing them — emacs applies a modifier-bar
/// click to exactly one following key event.
pub fn take_sticky_modifiers() -> KeyModifiers {
    KeyModifiers::from_bits_truncate(STICKY_MODIFIERS.swap(0, Ordering::Relaxed))
}

// ── scroll bars ──────────────────────────────────────────────────────────────

/// emacs `scroll-bar-mode`: turn the vertical scroll bar on (at `side`) or off.
pub fn set_scroll_bar(side: Option<ScrollBarSide>) {
    zmax_view::view::set_scroll_bar(side);
}

/// The side the vertical scroll bar is on, or `None` when it is off.
pub fn scroll_bar_side() -> Option<ScrollBarSide> {
    zmax_view::view::scroll_bar_side()
}

/// emacs `horizontal-scroll-bar-mode`.
pub fn set_horizontal_scroll_bar(on: bool) {
    zmax_view::view::set_horizontal_scroll_bar(on);
}

/// Whether the horizontal scroll bar is on.
pub fn horizontal_scroll_bar() -> bool {
    zmax_view::view::horizontal_scroll_bar_rows() > 0
}

/// emacs `window-tool-bar-mode` / `global-window-tool-bar-mode`.
pub fn set_window_tool_bar(on: bool) {
    zmax_view::view::set_window_tool_bar(on);
}

/// Whether the per-window tool bar is on.
pub fn window_tool_bar() -> bool {
    zmax_view::view::window_tool_bar_rows() > 0
}

/// The `[start, end)` rows of a vertical scroll bar's thumb inside a bar of
/// `height` rows, for a window showing `visible` of `total` lines starting at
/// line `top`. Port of emacs's `set_vertical_scroll_bar` proportions: the thumb
/// is as tall a fraction of the bar as the window is of the buffer, at least one
/// row, and it sits at the same fraction down the bar as the window is down the
/// buffer.
pub fn thumb_range(height: usize, total: usize, top: usize, visible: usize) -> (usize, usize) {
    if height == 0 {
        return (0, 0);
    }
    let total = total.max(1);
    if visible >= total {
        return (0, height);
    }
    let len = (height * visible / total).clamp(1, height);
    let start = (height * top / total).min(height - len);
    (start, start + len)
}

/// Where in a scroll bar the last `mouse-1`/`C-mouse-2` press landed, as a
/// fraction of the bar (0.0 at the top). Emacs hands the click event to
/// `scroll-bar-drag` / `mouse-split-window-vertically`; this is that argument, so
/// the commands stay invokable as commands.
static SCROLL_BAR_CLICK: AtomicUsize = AtomicUsize::new(0);
/// The fixed-point denominator `SCROLL_BAR_CLICK` is stored against.
const CLICK_SCALE: usize = 1 << 20;

/// Record where in the scroll bar the pointer is, as a 0.0–1.0 fraction.
pub fn set_scroll_bar_click(frac: f64) {
    let f = (frac.clamp(0.0, 1.0) * CLICK_SCALE as f64) as usize;
    SCROLL_BAR_CLICK.store(f, Ordering::Relaxed);
}

/// The fraction of the scroll bar the last press landed at.
pub fn scroll_bar_click() -> f64 {
    SCROLL_BAR_CLICK.load(Ordering::Relaxed) as f64 / CLICK_SCALE as f64
}

// ── the wheel ────────────────────────────────────────────────────────────────

/// Which way the wheel last turned: 0 = never, 1 = up, 2 = down. Emacs passes
/// the wheel event to `mouse-wheel-text-scale`, which reads its direction off
/// it; the wheel handler records it here so the command is invokable as itself.
static LAST_WHEEL: AtomicU8 = AtomicU8::new(0);

/// Record the direction the wheel just turned (`true` = up).
pub fn set_last_wheel_up(up: bool) {
    LAST_WHEEL.store(if up { 1 } else { 2 }, Ordering::Relaxed);
}

/// The direction the wheel last turned, or `None` if it never has.
pub fn last_wheel_up() -> Option<bool> {
    match LAST_WHEEL.load(Ordering::Relaxed) {
        1 => Some(true),
        2 => Some(false),
        _ => None,
    }
}

// ── tab-bar-select-tab-modifiers ─────────────────────────────────────────────

/// Remember which modifier `tab-bar-select-tab-modifiers` last installed, so the
/// command can un-install it before installing another.
pub fn take_select_tab_modifier() -> String {
    let mut slot = SELECT_TAB_MODIFIERS
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    std::mem::take(&mut *slot)
}

/// Record the modifier `tab-bar-select-tab-modifiers` installed.
pub fn set_select_tab_modifier(m: &str) {
    let mut slot = SELECT_TAB_MODIFIERS
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    *slot = m.to_string();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thumb_fills_the_bar_when_the_buffer_fits() {
        // The whole buffer is on screen: emacs draws a full-length thumb.
        assert_eq!(thumb_range(10, 20, 0, 20), (0, 10));
        assert_eq!(thumb_range(10, 5, 0, 40), (0, 10));
    }

    #[test]
    fn thumb_tracks_the_window_down_the_buffer() {
        // 100-line buffer, 10 visible, a 10-row bar: a 1-row thumb that walks
        // down one row per ten lines scrolled, and never past the bar's end.
        assert_eq!(thumb_range(10, 100, 0, 10), (0, 1));
        assert_eq!(thumb_range(10, 100, 50, 10), (5, 6));
        assert_eq!(thumb_range(10, 100, 95, 10), (9, 10));
    }

    #[test]
    fn thumb_is_never_zero_rows() {
        // A huge buffer would round the proportional length to zero; emacs
        // clamps it to one row so the thumb stays visible.
        let (start, end) = thumb_range(10, 1_000_000, 0, 10);
        assert!(end > start);
    }

    #[test]
    fn degenerate_bar_has_no_thumb() {
        assert_eq!(thumb_range(0, 100, 0, 10), (0, 0));
    }
}
