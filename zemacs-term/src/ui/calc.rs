//! Calc — the zemacs port of GNU Emacs `calc-mode`, the RPN stack calculator.
//!
//! A full-screen [`Component`] over the pure, unit-tested [`zemacs_core::calc`]
//! stack machine. The bottom of the screen shows the stack (level `1:` = top,
//! Calc's convention), above it a scrolling trail of past results, and a live
//! entry line for the number being typed. Keys map to their `calc-mode`
//! counterparts (parsed into a `calc` keymap mode by `scripts/gen_port_report.py`):
//!
//!   digits `.` `e` `_`  — build a number in the entry line (`_` = leading minus)
//!   RET                 — push the entry, or duplicate level 1 (`calc-enter`)
//!   DEL/Backspace       — edit the entry, or drop level 1 (`calc-pop`)
//!   `+ - * / ^ %`       — binary ops on levels 2 and 1
//!   n & Q A !           — negate, reciprocal, sqrt, abs, factorial (level 1)
//!   S C T               — sin cos tan;  I S / I C / I T — inverse
//!   L E                 — ln, exp
//!   TAB / M-TAB         — roll the stack down / up
//!   U / D               — undo / redo
//!   '                   — algebraic entry (type an infix expression)
//!   r                   — toggle radians/degrees
//!   q / Esc             — quit (`calc-quit`)

use tui::buffer::Buffer as Surface;
use zemacs_core::calc::{format_value, Angle, BinOp, Calc as Engine, UnOp};
use zemacs_view::graphics::Rect;

use crate::{
    alt,
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

/// The interactive Calc overlay.
pub struct Calc {
    engine: Engine,
    /// The number currently being typed (empty when not entering).
    entry: String,
    /// Pending inverse (`I`) prefix for the next trig key.
    inverse: bool,
    /// Last error, shown until the next keystroke.
    status: String,
}

impl Calc {
    pub fn new() -> Self {
        Calc {
            engine: Engine::new(),
            entry: String::new(),
            inverse: false,
            status: String::new(),
        }
    }

    /// Commit the entry line to the stack, if non-empty. Returns whether a value
    /// was pushed (so RET can fall back to `calc-enter` when the entry is empty).
    fn commit_entry(&mut self) -> bool {
        if self.entry.is_empty() {
            return false;
        }
        let text = self.entry.replace('_', "-");
        match text.parse::<f64>() {
            Ok(v) => {
                self.engine.push(v);
                self.entry.clear();
                true
            }
            Err(_) => {
                self.status = format!("bad number: {}", self.entry);
                self.entry.clear();
                true
            }
        }
    }

    /// Apply a unary op, committing any pending entry first.
    fn unary(&mut self, op: UnOp) {
        self.commit_entry();
        if let Err(e) = self.engine.unop(op) {
            self.status = e;
        }
    }

    /// Apply a binary op, committing any pending entry first.
    fn binary(&mut self, op: BinOp) {
        self.commit_entry();
        if let Err(e) = self.engine.binop(op) {
            self.status = e;
        }
    }

    /// Resolve a trig key against the pending inverse prefix.
    fn trig(&mut self, forward: UnOp, inverse: UnOp) {
        let op = if self.inverse { inverse } else { forward };
        self.inverse = false;
        self.unary(op);
    }
}

impl Default for Calc {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for Calc {
    fn handle_event(&mut self, event: &Event, _cx: &mut Context) -> EventResult {
        let key = match event {
            Event::Key(key) => *key,
            _ => return EventResult::Ignored(None),
        };
        self.status.clear();
        let close: Callback = Box::new(|compositor: &mut Compositor, _cx| {
            compositor.pop();
        });
        match key {
            key!('q') | key!(Esc) | ctrl!('c') => return EventResult::Consumed(Some(close)),

            // Number entry.
            key!(c @ '0'..='9') | key!(c @ '.') | key!(c @ '_') => self.entry.push(c),
            key!('e') if !self.entry.is_empty() => self.entry.push('e'),

            key!(Enter) => {
                if !self.commit_entry() {
                    self.engine.enter();
                }
            }
            key!(Backspace) | key!(Delete) => {
                if self.entry.pop().is_none() {
                    self.engine.pop();
                }
            }

            // Binary operators.
            key!('+') => self.binary(BinOp::Add),
            key!('-') => self.binary(BinOp::Sub),
            key!('*') => self.binary(BinOp::Mul),
            key!('/') => self.binary(BinOp::Div),
            key!('^') => self.binary(BinOp::Pow),
            key!('%') => self.binary(BinOp::Mod),

            // Unary operators.
            key!('n') => self.unary(UnOp::Neg),
            key!('&') => self.unary(UnOp::Recip),
            key!('Q') => self.unary(UnOp::Sqrt),
            key!('A') => self.unary(UnOp::Abs),
            key!('!') => self.unary(UnOp::Factorial),
            key!('L') => self.unary(UnOp::Ln),
            key!('E') => self.unary(UnOp::Exp),

            // Trig (with the `I` inverse prefix).
            key!('I') => self.inverse = true,
            key!('S') => self.trig(UnOp::Sin, UnOp::Asin),
            key!('C') => self.trig(UnOp::Cos, UnOp::Acos),
            key!('T') => self.trig(UnOp::Tan, UnOp::Atan),

            // Stack manipulation.
            key!(Tab) => {
                self.commit_entry();
                self.engine.roll_down();
            }
            alt!(Tab) => {
                self.commit_entry();
                self.engine.roll_up();
            }

            // Undo / redo.
            key!('U') => {
                if !self.engine.undo() {
                    self.status = "no further undo".into();
                }
            }
            key!('D') => {
                if !self.engine.redo() {
                    self.status = "no further redo".into();
                }
            }

            // Angle mode toggle.
            key!('r') => {
                self.engine.toggle_angle();
            }

            _ => {}
        }
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        let theme = &ctx.editor.theme;
        let bg = theme.get("ui.background");
        let header_style = theme.get("ui.text.focus");
        let text_style = theme.get("ui.text");
        let info_style = theme.get("ui.linenr");
        let level_style = theme.get("ui.selection");
        let err_style = theme.get("error");

        surface.clear_with(area, bg);
        if area.width < 16 || area.height < 4 {
            return;
        }

        // Title + hint.
        let angle = match self.engine.angle {
            Angle::Radians => "Rad",
            Angle::Degrees => "Deg",
        };
        let title = format!(" Calc  ({angle})");
        surface.set_stringn(area.x, area.y, &title, area.width as usize, header_style);
        let hint = "RET push  +-*/^  n&QA!  SCT  L E  TAB roll  U undo  q quit";
        if title.len() + hint.len() + 3 < area.width as usize {
            surface.set_stringn(
                area.x + area.width - hint.len() as u16 - 1,
                area.y,
                hint,
                hint.len(),
                info_style,
            );
        }

        // Reserve the bottom row for the entry line, the rows above it for the
        // stack (level 1 nearest the entry line, growing upward), and whatever
        // remains at the top for the trail.
        let entry_y = area.y + area.height - 1;
        let stack = self.engine.stack();
        let stack_rows = (area.height.saturating_sub(3)).min(stack.len() as u16);
        let stack_top = entry_y.saturating_sub(stack_rows);

        // Trail (past results), filling the gap between the header and the stack.
        let trail = self.engine.trail();
        let trail_top = area.y + 2;
        if stack_top > trail_top {
            let rows = (stack_top - trail_top) as usize;
            let start = trail.len().saturating_sub(rows);
            for (i, line) in trail[start..].iter().enumerate() {
                let s = format!("      {line}");
                surface.set_stringn(
                    area.x,
                    trail_top + i as u16,
                    &s,
                    area.width as usize,
                    info_style,
                );
            }
        }

        // Stack: level 1 (top of stack) closest to the entry line.
        for row in 0..stack_rows {
            let level = row + 1; // 1 = closest to entry line
            let idx = stack.len() - level as usize;
            let y = entry_y - level;
            let label = format!("{level}:");
            surface.set_stringn(area.x, y, &label, 4, level_style);
            let val = format_value(stack[idx]);
            surface.set_stringn(area.x + 4, y, &val, area.width as usize - 4, text_style);
        }

        // Entry / status line.
        if !self.status.is_empty() {
            surface.set_stringn(
                area.x,
                entry_y,
                &self.status,
                area.width as usize,
                err_style,
            );
        } else {
            let shown = if self.entry.is_empty() {
                "_".to_string()
            } else {
                self.entry.replace('_', "-")
            };
            let prompt = format!("Calc> {shown}");
            surface.set_stringn(area.x, entry_y, &prompt, area.width as usize, header_style);
        }
    }
}
