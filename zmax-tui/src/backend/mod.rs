//! Provides interface for controlling the terminal

use std::io;

use crate::{buffer::Cell, terminal::Config};

use zmax_view::{
    graphics::{CursorKind, Rect},
    theme::Color,
};

#[cfg(all(feature = "termina", not(windows)))]
mod termina;
#[cfg(all(feature = "termina", not(windows)))]
pub use self::termina::TerminaBackend;

#[cfg(all(feature = "termina", windows))]
mod crossterm;
#[cfg(all(feature = "termina", windows))]
pub use self::crossterm::CrosstermBackend;

mod test;
pub use self::test::TestBackend;

/// Emacs `tty-suppress-bold-inverse-default-colors`: whether boldness is dropped
/// from faces drawn in inverse video with the default colours. Off until the
/// command of that name turns it on.
static SUPPRESS_BOLD_INVERSE_DEFAULT_COLORS: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Suppress or allow boldness of faces with inverse default colors.
///
/// On some terminals bold text with inverse video is unreadable, so with this
/// enabled such faces are drawn without the bold attribute. Emacs' command
/// treats a numeric prefix of zero as "off"; the caller does that decoding and
/// passes the resulting boolean here.
pub fn set_suppress_bold_inverse_default_colors(suppress: bool) {
    SUPPRESS_BOLD_INVERSE_DEFAULT_COLORS.store(suppress, std::sync::atomic::Ordering::Relaxed);
}

/// Whether boldness of faces with inverse default colors is currently suppressed.
pub fn suppress_bold_inverse_default_colors() -> bool {
    SUPPRESS_BOLD_INVERSE_DEFAULT_COLORS.load(std::sync::atomic::Ordering::Relaxed)
}

/// The modifier a cell is actually drawn with.
///
/// Port of the bold check at the end of `realize_tty_face` in Emacs'
/// `src/xfaces.c`, which clears `tty_bold_p` when the face is bold and its
/// background is the default foreground color while its foreground is the
/// default background color. A cell with both colors `Reset` and the
/// `REVERSED` modifier is how a backend spells that swap.
pub fn effective_modifier(cell: &Cell) -> zmax_view::graphics::Modifier {
    use zmax_view::graphics::Modifier;
    if suppress_bold_inverse_default_colors()
        && cell.modifier.contains(Modifier::BOLD | Modifier::REVERSED)
        && cell.fg == Color::Reset
        && cell.bg == Color::Reset
    {
        cell.modifier - Modifier::BOLD
    } else {
        cell.modifier
    }
}

/// Representation of a terminal backend.
pub trait Backend {
    /// Claims the terminal for TUI use.
    fn claim(&mut self) -> Result<(), io::Error>;
    /// Update terminal configuration.
    fn reconfigure(&mut self, config: Config) -> Result<(), io::Error>;
    /// Restores the terminal to a normal state, undoes `claim`
    fn restore(&mut self) -> Result<(), io::Error>;
    /// Draws styled text to the terminal
    fn draw<'a, I>(&mut self, content: I) -> Result<(), io::Error>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>;
    /// Hides the cursor
    fn hide_cursor(&mut self) -> Result<(), io::Error>;
    /// Sets the cursor to the given shape
    fn show_cursor(&mut self, kind: CursorKind) -> Result<(), io::Error>;
    /// Sets the cursor to the given position
    fn set_cursor(&mut self, x: u16, y: u16) -> Result<(), io::Error>;
    /// Clears the terminal
    fn clear(&mut self) -> Result<(), io::Error>;
    /// Begins a synchronized-output frame (if the terminal supports it), so the
    /// draw and cursor updates between `start_sync` and `end_sync` present as one
    /// frame instead of flickering.
    fn start_sync(&mut self) -> Result<(), io::Error>;
    /// Ends the synchronized-output frame opened by `start_sync`.
    fn end_sync(&mut self) -> Result<(), io::Error>;
    /// Gets the size of the terminal in cells
    fn size(&self) -> Result<Rect, io::Error>;
    /// Flushes the terminal buffer
    fn flush(&mut self) -> Result<(), io::Error>;
    fn supports_true_color(&self) -> bool;
    fn get_theme_mode(&self) -> Option<zmax_view::theme::Mode>;
    fn set_background_color(&mut self, color: Option<Color>) -> io::Result<()>;
    /// vim `title`: set the terminal window title (OSC 2). Default no-op.
    fn set_title(&mut self, _title: &str) -> io::Result<()> {
        Ok(())
    }
}
