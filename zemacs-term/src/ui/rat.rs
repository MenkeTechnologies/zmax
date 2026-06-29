//! Render ratatui widgets onto zemacs's tui `Surface`.
//!
//! ratatui and zemacs-tui share the tui-rs lineage, so the `Color`/`Modifier` vocabularies line up.
//! We render a widget offscreen into a ratatui `Buffer` sized to the target rect, then blit the cells
//! across. This lets the IDE panels use real widgets — `List` with stateful selection, `Scrollbar`,
//! `Block`, `Table` — instead of hand-drawn `set_string`.

use ratatui::buffer::Buffer as RatBuffer;
use ratatui::layout::Rect as RatRect;
use ratatui::widgets::{StatefulWidget, Widget};

use tui::buffer::Buffer as Surface;
use zemacs_view::graphics::{Color, Modifier, Rect};

fn rat_rect(r: Rect) -> RatRect {
    RatRect::new(r.x, r.y, r.width, r.height)
}

fn color(c: ratatui::style::Color) -> Color {
    use ratatui::style::Color as R;
    match c {
        R::Reset => Color::Reset,
        R::Black => Color::Black,
        R::Red => Color::Red,
        R::Green => Color::Green,
        R::Yellow => Color::Yellow,
        R::Blue => Color::Blue,
        R::Magenta => Color::Magenta,
        R::Cyan => Color::Cyan,
        R::Gray => Color::Gray,
        R::DarkGray => Color::LightGray,
        R::LightRed => Color::LightRed,
        R::LightGreen => Color::LightGreen,
        R::LightYellow => Color::LightYellow,
        R::LightBlue => Color::LightBlue,
        R::LightMagenta => Color::LightMagenta,
        R::LightCyan => Color::LightCyan,
        R::White => Color::White,
        R::Rgb(r, g, b) => Color::Rgb(r, g, b),
        R::Indexed(i) => Color::Indexed(i),
    }
}

fn modifier(m: ratatui::style::Modifier) -> Modifier {
    use ratatui::style::Modifier as R;
    let mut out = Modifier::empty();
    for (from, to) in [
        (R::BOLD, Modifier::BOLD),
        (R::DIM, Modifier::DIM),
        (R::ITALIC, Modifier::ITALIC),
        (R::SLOW_BLINK, Modifier::SLOW_BLINK),
        (R::RAPID_BLINK, Modifier::RAPID_BLINK),
        (R::REVERSED, Modifier::REVERSED),
        (R::HIDDEN, Modifier::HIDDEN),
        (R::CROSSED_OUT, Modifier::CROSSED_OUT),
    ] {
        if m.contains(from) {
            out.insert(to);
        }
    }
    out
}

/// zemacs `Color` → ratatui `Color` (so widgets can be themed with editor colors).
pub fn to_rat_color(c: Color) -> ratatui::style::Color {
    use ratatui::style::Color as R;
    match c {
        Color::Reset => R::Reset,
        Color::Black => R::Black,
        Color::Red => R::Red,
        Color::Green => R::Green,
        Color::Yellow => R::Yellow,
        Color::Blue => R::Blue,
        Color::Magenta => R::Magenta,
        Color::Cyan => R::Cyan,
        Color::Gray => R::Gray,
        Color::LightRed => R::LightRed,
        Color::LightGreen => R::LightGreen,
        Color::LightYellow => R::LightYellow,
        Color::LightBlue => R::LightBlue,
        Color::LightMagenta => R::LightMagenta,
        Color::LightCyan => R::LightCyan,
        Color::LightGray => R::DarkGray,
        Color::White => R::White,
        Color::Rgb(r, g, b) => R::Rgb(r, g, b),
        Color::Indexed(i) => R::Indexed(i),
    }
}

/// zemacs `Style` → ratatui `Style`.
pub fn to_rat_style(s: zemacs_view::graphics::Style) -> ratatui::style::Style {
    let mut rs = ratatui::style::Style::default();
    if let Some(fg) = s.fg {
        rs = rs.fg(to_rat_color(fg));
    }
    if let Some(bg) = s.bg {
        rs = rs.bg(to_rat_color(bg));
    }
    let mut m = ratatui::style::Modifier::empty();
    for (z, r) in [
        (Modifier::BOLD, ratatui::style::Modifier::BOLD),
        (Modifier::DIM, ratatui::style::Modifier::DIM),
        (Modifier::ITALIC, ratatui::style::Modifier::ITALIC),
        (Modifier::REVERSED, ratatui::style::Modifier::REVERSED),
        (Modifier::CROSSED_OUT, ratatui::style::Modifier::CROSSED_OUT),
        (Modifier::HIDDEN, ratatui::style::Modifier::HIDDEN),
        (Modifier::SLOW_BLINK, ratatui::style::Modifier::SLOW_BLINK),
        (Modifier::RAPID_BLINK, ratatui::style::Modifier::RAPID_BLINK),
    ] {
        if s.add_modifier.contains(z) {
            m.insert(r);
        }
    }
    rs.add_modifier(m)
}

fn blit(buf: &RatBuffer, surface: &mut Surface) {
    use ratatui::style::Color as R;
    let area = buf.area;
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            let rc = &buf[(x, y)];
            if let Some(sc) = surface.get_mut(x, y) {
                sc.set_symbol(rc.symbol());
                sc.set_fg(color(rc.fg));
                // A `Reset` background is a widget's "transparent" cell (most
                // widgets leave their empty cells like this). Don't copy it onto
                // the surface — that would punch through the panel's already-
                // painted theme background to the terminal's default/transparent
                // background. Keep whatever bg is already there instead.
                if rc.bg != R::Reset {
                    sc.set_bg(color(rc.bg));
                }
                sc.modifier = modifier(rc.modifier);
            }
        }
    }
}

/// Render a ratatui widget into `area` of the zemacs `Surface`.
pub fn render<W: Widget>(widget: W, area: Rect, surface: &mut Surface) {
    let rr = rat_rect(area);
    if rr.width == 0 || rr.height == 0 {
        return;
    }
    let mut buf = RatBuffer::empty(rr);
    widget.render(rr, &mut buf);
    blit(&buf, surface);
}

/// Render a ratatui stateful widget (e.g. `List` with `ListState`).
pub fn render_stateful<W: StatefulWidget>(
    widget: W,
    area: Rect,
    surface: &mut Surface,
    state: &mut W::State,
) {
    let rr = rat_rect(area);
    if rr.width == 0 || rr.height == 0 {
        return;
    }
    let mut buf = RatBuffer::empty(rr);
    StatefulWidget::render(widget, rr, &mut buf, state);
    blit(&buf, surface);
}
