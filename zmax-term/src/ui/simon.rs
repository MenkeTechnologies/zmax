//! Simon — a memory game in the spirit of the classic electronic toy.
//!
//! Each round the machine appends one random pad to a growing sequence and
//! flashes the whole sequence back at you; then you must reproduce it by pressing
//! the matching pads with the arrows or `1`-`4`. A perfect round scores its length
//! and moves on to a longer one; a wrong press ends the game. Like the other
//! action games it animates itself via `zmax_event::request_redraw` only during
//! the flash playback and idles while it waits for your input. The pad/sequence
//! logic is pure and unit-tested.

use std::time::{Duration, Instant};

use tui::buffer::Buffer as Surface;
use zmax_view::graphics::Rect;

use crate::{
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

/// How long a pad stays lit, and the dark gap between pads, during playback.
const ON: Duration = Duration::from_millis(450);
const GAP: Duration = Duration::from_millis(180);

/// Pad block geometry (a 2x2 grid with a small gutter).
const PAD_W: u16 = 16;
const PAD_H: u16 = 6;
const GAP_XY: u16 = 2;

/// Which part of the game we're in — the pure model tracks this itself.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    /// The machine is flashing the sequence back.
    Showing,
    /// Waiting for the player to reproduce it.
    Input,
    /// A wrong press ended the game.
    Over,
}

/// The result of a single pad press.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Feedback {
    /// Right pad, but the sequence isn't finished yet.
    Correct,
    /// Wrong pad — the game is now over.
    Wrong,
    /// The last pad of the sequence was matched: the round is complete.
    RoundComplete,
}

/// The pure Simon model. No I/O, no timing — unit-tested.
#[derive(Clone)]
pub struct Game {
    /// The pads to reproduce, in order (each 0..4).
    seq: Vec<usize>,
    pub phase: Phase,
    /// How many pads the player has correctly pressed this round.
    pub input_idx: usize,
    rng: u64,
}

impl Game {
    pub fn new(seed: u64) -> Self {
        let mut g = Game {
            seq: Vec::new(),
            phase: Phase::Showing,
            input_idx: 0,
            rng: seed | 1,
        };
        g.add_step();
        g
    }

    fn rand(&mut self) -> u64 {
        self.rng = self
            .rng
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.rng >> 33
    }

    /// Append one random pad and re-arm the sequence for playback.
    pub fn add_step(&mut self) {
        let pad = (self.rand() % 4) as usize;
        self.seq.push(pad);
        self.input_idx = 0;
        self.phase = Phase::Showing;
    }

    /// Called by the wrapper when the flash playback finishes: hand control to the
    /// player.
    pub fn begin_input(&mut self) {
        if self.phase == Phase::Showing {
            self.phase = Phase::Input;
            self.input_idx = 0;
        }
    }

    /// Register a pad press. A matching pad returns `Correct` (or `RoundComplete`
    /// when it was the last one, resetting the input index for the next round); a
    /// mismatch returns `Wrong` and ends the game.
    pub fn press(&mut self, pad: usize) -> Feedback {
        if self.phase == Phase::Over {
            return Feedback::Wrong;
        }
        if pad != self.seq[self.input_idx] {
            self.phase = Phase::Over;
            return Feedback::Wrong;
        }
        self.input_idx += 1;
        if self.input_idx >= self.seq.len() {
            self.input_idx = 0;
            self.phase = Phase::Showing;
            Feedback::RoundComplete
        } else {
            Feedback::Correct
        }
    }

    /// The current sequence to reproduce.
    pub fn sequence(&self) -> &[usize] {
        &self.seq
    }

    /// The round number is just the sequence length.
    pub fn round(&self) -> usize {
        self.seq.len()
    }
}

impl Default for Game {
    fn default() -> Self {
        Self::new(1)
    }
}

/// The flash state within a `Showing` playback: pad lit, then a dark gap.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Flash {
    On,
    Gap,
}

/// The interactive Simon overlay.
pub struct Simon {
    game: Game,
    seed: u64,
    best: u32,
    last: Option<Instant>,
    interval: Duration,
    /// Which pad of the sequence the playback is currently on.
    step: usize,
    flash: Flash,
}

impl Simon {
    pub fn new() -> Self {
        Simon {
            game: Game::new(1),
            seed: 1,
            best: 0,
            last: None,
            interval: ON,
            step: 0,
            flash: Flash::On,
        }
    }

    fn restart(&mut self) {
        self.seed = self.seed.wrapping_add(1);
        self.game = Game::new(self.seed);
        self.start_playback();
    }

    /// (Re)arm the flash loop from the start of the sequence.
    fn start_playback(&mut self) {
        self.step = 0;
        self.flash = Flash::On;
        self.interval = ON;
        self.last = None;
    }

    /// Advance the flash animation one beat: On → Gap, then Gap → next pad (or hand
    /// off to the player once every pad has flashed).
    fn advance_flash(&mut self) {
        match self.flash {
            Flash::On => {
                self.flash = Flash::Gap;
                self.interval = GAP;
            }
            Flash::Gap => {
                self.step += 1;
                if self.step >= self.game.sequence().len() {
                    self.game.begin_input();
                } else {
                    self.flash = Flash::On;
                    self.interval = ON;
                }
            }
        }
    }

    /// Route a pad press during input; a completed round grows the sequence and
    /// replays it.
    fn press_pad(&mut self, pad: usize) {
        if self.game.phase != Phase::Input {
            return;
        }
        if let Feedback::RoundComplete = self.game.press(pad) {
            self.best = self.best.max(self.game.round() as u32);
            self.game.add_step();
            self.start_playback();
        }
    }
}

impl Default for Simon {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for Simon {
    fn handle_event(&mut self, event: &Event, _cx: &mut Context) -> EventResult {
        let key = match event {
            Event::Key(key) => *key,
            _ => return EventResult::Ignored(None),
        };
        let close: Callback = Box::new(|compositor: &mut Compositor, _cx| {
            compositor.pop();
        });
        match key {
            key!('q') | key!(Esc) | ctrl!('c') => return EventResult::Consumed(Some(close)),
            key!(Up) | key!('1') => self.press_pad(0),
            key!(Right) | key!('2') => self.press_pad(1),
            key!(Left) | key!('3') => self.press_pad(2),
            key!(Down) | key!('4') => self.press_pad(3),
            key!('n') => self.restart(),
            _ => {}
        }
        // Only the flash playback needs the frame loop; input just idles.
        if self.game.phase == Phase::Showing {
            zmax_event::request_redraw();
        }
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        // Advance the flash on wall-clock delta, scheduling the next frame while a
        // playback is in progress.
        let now = Instant::now();
        if self.game.phase == Phase::Showing {
            match self.last {
                Some(t) if now.duration_since(t) >= self.interval => {
                    self.advance_flash();
                    self.last = Some(now);
                }
                None => self.last = Some(now),
                _ => {}
            }
            if self.game.phase == Phase::Showing {
                zmax_event::request_redraw();
            }
        }

        let theme = &ctx.editor.theme;
        let bg = theme.get("ui.background");
        let text_style = theme.get("ui.text");
        let header_style = theme.get("ui.text.focus");
        let dim_style = theme.get("ui.linenr");
        let sel_style = theme.get("ui.selection");
        let pad_styles = [
            theme.get("function"),      // 0 green
            theme.get("error"),         // 1 red
            theme.get("warning"),       // 2 yellow
            theme.get("ui.text.focus"), // 3 blue
        ];

        surface.clear_with(area, bg);
        if area.width < PAD_W * 2 + GAP_XY + 4 || area.height < PAD_H * 2 + GAP_XY + 6 {
            return;
        }
        let ox = area.x + 2;
        let oy = area.y + 2;
        surface.set_string(
            ox,
            area.y,
            &format!("Simon  round {}  best {}", self.game.round(), self.best),
            header_style,
        );

        // The lit pad, if any (only while a pad is flashing On).
        let lit = if self.game.phase == Phase::Showing && self.flash == Flash::On {
            self.game.sequence().get(self.step).copied()
        } else {
            None
        };

        let pads = [
            (ox, oy),
            (ox + PAD_W + GAP_XY, oy),
            (ox, oy + PAD_H + GAP_XY),
            (ox + PAD_W + GAP_XY, oy + PAD_H + GAP_XY),
        ];
        for (p, &(px, py)) in pads.iter().enumerate() {
            let style = if lit == Some(p) {
                pad_styles[p]
            } else {
                dim_style
            };
            for ry in 0..PAD_H {
                for rx in 0..PAD_W {
                    surface.set_string(px + rx, py + ry, "█", style);
                }
            }
            // The pad's number, centred, so keys and pads line up at a glance.
            let label = ((p as u8 + 1) + b'0') as char;
            surface.set_string(
                px + PAD_W / 2,
                py + PAD_H / 2,
                &label.to_string(),
                sel_style,
            );
        }

        let py = oy + PAD_H * 2 + GAP_XY;
        let prompt = match self.game.phase {
            Phase::Showing => "Watch…".to_string(),
            Phase::Input => "Your turn".to_string(),
            Phase::Over => format!("Game over at round {}", self.game.round()),
        };
        surface.set_string(ox, py, &prompt, header_style);
        surface.set_string(ox, py + 1, "↑↓←→ or 1-4 press · n new · q quit", text_style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_step_grows_the_sequence() {
        let mut g = Game::new(1); // starts with one pad already
        let len = g.sequence().len();
        g.add_step();
        assert_eq!(g.sequence().len(), len + 1);
    }

    #[test]
    fn correct_presses_advance_then_complete_the_round() {
        let mut g = Game::new(1);
        g.add_step(); // ensure a length of at least two
        let seq = g.sequence().to_vec();
        for &pad in &seq[..seq.len() - 1] {
            assert_eq!(g.press(pad), Feedback::Correct);
        }
        assert_eq!(g.press(*seq.last().unwrap()), Feedback::RoundComplete);
    }

    #[test]
    fn a_wrong_press_ends_the_game() {
        let mut g = Game::new(1);
        let right = g.sequence()[0];
        let wrong = (right + 1) % 4;
        assert_eq!(g.press(wrong), Feedback::Wrong);
        assert_eq!(g.phase, Phase::Over);
    }

    #[test]
    fn round_complete_resets_the_input_index() {
        let mut g = Game::new(1);
        let seq = g.sequence().to_vec();
        for &pad in &seq {
            g.press(pad);
        }
        assert_eq!(g.input_idx, 0, "the input index resets for the next round");
    }

    #[test]
    fn a_fixed_seed_is_deterministic() {
        let mut a = Game::new(7);
        let mut b = Game::new(7);
        for _ in 0..8 {
            a.add_step();
            b.add_step();
        }
        assert_eq!(a.sequence(), b.sequence());
    }
}
