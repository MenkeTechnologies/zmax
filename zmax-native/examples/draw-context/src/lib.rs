//! Example plugin: what a plugin needs to know before drawing anything.
//!
//! A plugin rendering an overlay, a ruler or a status fragment has to fit the
//! space it is given and match the colours around it. Three calls answer that:
//!
//! - [`Host::window_size`] — the text area in CELLS, gutters excluded. The
//!   budget an overlay has to fit inside.
//! - [`Host::window_index`] — which window is focused, so a plugin drawing per
//!   window knows which one it is being asked about.
//! - [`Host::bg_color`] — the theme's background, as `#rrggbb` or a colour
//!   name. `None` means the theme sets none and the TERMINAL's background shows
//!   through, which is the case where a plugin must not assume light or dark.
//!
//! ```text
//! :plugin load .../libzmax_native_draw_context.dylib
//! :drawctx   # → "window 1: 80x24 cells · bg #1e1e2e (dark) · overlay budget 76x22"
//! ```

use std::os::raw::c_int;

use zmax_native::{declare_plugin, Args, Host};

/// Margin an overlay leaves so it does not touch the window edges.
const MARGIN: usize = 2;

/// The space an overlay may occupy, given the window.
///
/// Saturating, so a window too small for the margins yields zero rather than
/// wrapping to an enormous size — the caller then knows not to draw at all.
fn overlay_budget(width: usize, height: usize) -> (usize, usize) {
    (
        width.saturating_sub(MARGIN * 2),
        height.saturating_sub(MARGIN),
    )
}

/// Whether a background colour is dark, for choosing readable foregrounds.
///
/// Only answers for `#rrggbb`, which is the form a true-colour theme gives.
/// A NAMED colour is left unanswered rather than guessed: "blue" could be
/// rendered light or dark by the terminal's palette, and the plugin cannot see
/// which.
fn is_dark(bg: &str) -> Option<bool> {
    let hex = bg.strip_prefix('#')?;
    if hex.len() != 6 {
        return None;
    }
    let channel = |at: usize| u8::from_str_radix(hex.get(at..at + 2)?, 16).ok();
    let (r, g, b) = (channel(0)?, channel(2)?, channel(4)?);
    // Rec. 601 luma, the usual quick test for perceived brightness.
    let luma = 0.299 * r as f64 + 0.587 * g as f64 + 0.114 * b as f64;
    Some(luma < 128.0)
}

/// How to describe the background.
fn bg_note(bg: Option<&str>) -> String {
    match bg {
        None => "bg from terminal (theme sets none — assume neither light nor dark)".to_string(),
        Some(colour) => match is_dark(colour) {
            Some(true) => format!("bg {colour} (dark)"),
            Some(false) => format!("bg {colour} (light)"),
            // A palette name, whose rendering only the terminal knows.
            None => format!("bg {colour} (named — brightness unknown)"),
        },
    }
}

/// `:drawctx` — the rendering context for the focused window.
fn drawctx(host: &Host, _args: &Args) -> c_int {
    let (width, height) = host.window_size();
    let (budget_w, budget_h) = overlay_budget(width, height);

    host.message(&format!(
        "window {}: {width}x{height} cells · {} · overlay budget {budget_w}x{budget_h}",
        host.window_index(),
        bg_note(host.bg_color().as_deref()),
    ));
    0
}

declare_plugin! {
    name: "draw-context",
    version: "0.1.0",
    commands: { "drawctx" => drawctx },
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An ordinary window leaves room for the margins.
    #[test]
    fn an_ordinary_window_has_a_budget() {
        assert_eq!(overlay_budget(80, 24), (76, 22));
    }

    /// A window too small for its margins yields zero, not a wrapped
    /// enormous number — which is what unchecked subtraction would give and
    /// what would then be used as a draw size.
    #[test]
    fn a_tiny_window_yields_no_budget() {
        assert_eq!(overlay_budget(2, 1), (0, 0));
        assert_eq!(overlay_budget(0, 0), (0, 0));
    }

    /// Brightness is judged by luma, so a saturated colour is classified by
    /// how bright it looks rather than by its largest channel.
    #[test]
    fn brightness_uses_luma_not_the_largest_channel() {
        assert_eq!(is_dark("#000000"), Some(true));
        assert_eq!(is_dark("#ffffff"), Some(false));
        assert_eq!(is_dark("#1e1e2e"), Some(true), "a typical dark theme");
        // Pure blue is dark despite a full channel; pure green is light.
        assert_eq!(is_dark("#0000ff"), Some(true));
        assert_eq!(is_dark("#00ff00"), Some(false));
    }

    /// A named colour is NOT guessed at: the terminal's palette decides how it
    /// renders, and the plugin cannot see that.
    #[test]
    fn a_named_colour_is_left_unanswered() {
        assert_eq!(is_dark("blue"), None);
        assert!(bg_note(Some("blue")).contains("brightness unknown"));
    }

    /// Malformed hex is unanswered rather than parsed partially.
    #[test]
    fn malformed_hex_is_unanswered() {
        assert_eq!(is_dark("#12345"), None, "too short");
        assert_eq!(is_dark("#zzzzzz"), None, "not hex");
    }

    /// No background at all is its own case — the terminal's shows through,
    /// and a plugin must not assume either polarity.
    #[test]
    fn no_background_forbids_assuming_a_polarity() {
        let note = bg_note(None);
        assert!(note.contains("from terminal"));
        assert!(note.contains("neither light nor dark"));
    }
}
