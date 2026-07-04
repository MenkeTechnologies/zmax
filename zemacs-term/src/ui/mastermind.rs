//! Mastermind — a small terminal code-breaker for zemacs.
//!
//! A secret code of four colored pegs (each `1`..=`6`, repeats allowed) is hidden
//! at the start. Type four digits to build a guess, `Backspace` deletes the last,
//! `Enter` submits it, `n` starts a fresh game and `q`/`Esc` quits. Each guess is
//! scored with black pegs (`●`, right color *and* place) and white pegs (`○`,
//! right color, wrong place); you get ten tries to crack it. Like Minesweeper this
//! is turn-based: nothing animates, so there is no frame loop — the board only
//! changes in response to a key. The scoring is pure and unit-tested, and the code
//! is chosen by a small LCG so a given seed is reproducible.

use tui::buffer::Buffer as Surface;
use zemacs_view::graphics::Rect;

use crate::{
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

const PEGS: usize = 4;
const COLORS: u8 = 6;
const MAX_GUESSES: usize = 10;

/// Where the game is: still guessing, cracked, or out of tries.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    Playing,
    Won,
    Lost,
}

/// Score `guess` against `secret`, returning `(black, white)` pegs. Black counts
/// exact color-and-position matches; white counts correct colors in the wrong
/// place. Duplicates are handled properly: after removing the black matches, the
/// leftover colors on each side are tallied and the overlap (per-color `min`) is
/// the white count — no color is ever double-credited.
pub fn score(secret: &[u8; PEGS], guess: &[u8; PEGS]) -> (u8, u8) {
    let mut black = 0u8;
    // Per-color counts of the pegs that were *not* an exact match, indexed by the
    // color value itself (0 is unused so colors read naturally).
    let mut secret_left = [0u8; COLORS as usize + 1];
    let mut guess_left = [0u8; COLORS as usize + 1];
    for i in 0..PEGS {
        if secret[i] == guess[i] {
            black += 1;
        } else {
            secret_left[secret[i] as usize] += 1;
            guess_left[guess[i] as usize] += 1;
        }
    }
    let mut white = 0u8;
    for c in 1..=COLORS as usize {
        white += secret_left[c].min(guess_left[c]);
    }
    (black, white)
}

/// The pure Mastermind game. No I/O, no timing — unit-tested. The secret code is
/// drawn with the same LCG the other games use, so `Game::new(seed)` is
/// deterministic.
#[derive(Clone)]
pub struct Game {
    /// The hidden code, each peg a color `1..=COLORS`.
    secret: [u8; PEGS],
    /// Submitted guesses paired with their `(black, white)` score.
    guesses: Vec<([u8; PEGS], (u8, u8))>,
    /// The in-progress guess, 0..=PEGS digits entered so far.
    current: Vec<u8>,
    state: State,
    rng: u64,
}

impl Game {
    pub fn new(seed: u64) -> Self {
        let mut g = Game {
            secret: [0; PEGS],
            guesses: Vec::new(),
            current: Vec::new(),
            state: State::Playing,
            rng: seed | 1,
        };
        g.pick_secret();
        g
    }

    fn rand(&mut self) -> u64 {
        self.rng = self
            .rng
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.rng >> 33
    }

    fn pick_secret(&mut self) {
        for i in 0..PEGS {
            self.secret[i] = (self.rand() % COLORS as u64) as u8 + 1;
        }
    }

    pub fn state(&self) -> State {
        self.state
    }

    pub fn secret(&self) -> &[u8; PEGS] {
        &self.secret
    }

    pub fn guesses(&self) -> &[([u8; PEGS], (u8, u8))] {
        &self.guesses
    }

    pub fn current(&self) -> &[u8] {
        &self.current
    }

    /// Tries left before the game is lost.
    pub fn remaining(&self) -> usize {
        MAX_GUESSES - self.guesses.len()
    }

    /// Append a color to the current guess. Out-of-range colors and a full row are
    /// ignored, as is any input once the game is over.
    pub fn push_digit(&mut self, color: u8) {
        if self.state != State::Playing {
            return;
        }
        if color < 1 || color > COLORS {
            return;
        }
        if self.current.len() < PEGS {
            self.current.push(color);
        }
    }

    /// Delete the last color of the current guess.
    pub fn backspace(&mut self) {
        self.current.pop();
    }

    /// Submit the current guess once it has all `PEGS` colors: score it, file it in
    /// the history and clear the row. Four black pegs wins; running out of tries
    /// loses.
    pub fn submit(&mut self) {
        if self.state != State::Playing || self.current.len() != PEGS {
            return;
        }
        let mut guess = [0u8; PEGS];
        guess.copy_from_slice(&self.current);
        let s = score(&self.secret, &guess);
        self.guesses.push((guess, s));
        self.current.clear();
        if s.0 == PEGS as u8 {
            self.state = State::Won;
        } else if self.guesses.len() >= MAX_GUESSES {
            self.state = State::Lost;
        }
    }
}

impl Default for Game {
    fn default() -> Self {
        Self::new(1)
    }
}

/// The theme style for a peg color, so the four colors read distinctly.
fn color_key(color: u8) -> &'static str {
    match color {
        1 => "warning",
        2 => "function",
        3 => "error",
        4 => "ui.text",
        5 => "ui.text.focus",
        _ => "ui.selection",
    }
}

/// The interactive Mastermind overlay.
pub struct Mastermind {
    game: Game,
    seed: u64,
}

impl Mastermind {
    pub fn new() -> Self {
        Mastermind {
            game: Game::new(1),
            seed: 1,
        }
    }

    fn restart(&mut self) {
        self.seed = self.seed.wrapping_add(1);
        self.game = Game::new(self.seed);
    }
}

impl Default for Mastermind {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for Mastermind {
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
            key!('1') => self.game.push_digit(1),
            key!('2') => self.game.push_digit(2),
            key!('3') => self.game.push_digit(3),
            key!('4') => self.game.push_digit(4),
            key!('5') => self.game.push_digit(5),
            key!('6') => self.game.push_digit(6),
            key!(Backspace) => self.game.backspace(),
            key!(Enter) => self.game.submit(),
            key!('n') => self.restart(),
            _ => {}
        }
        zemacs_event::request_redraw();
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        let theme = &ctx.editor.theme;
        let bg = theme.get("ui.background");
        let text_style = theme.get("ui.text");
        let header_style = theme.get("ui.text.focus");
        let empty_style = theme.get("ui.linenr");
        let cursor_style = theme.get("ui.selection");
        let black_style = theme.get("ui.text.focus");
        let white_style = theme.get("ui.linenr");

        surface.clear_with(area, bg);
        if area.width < 44 || area.height < 22 {
            return;
        }
        let ox = area.x + 2;
        let oy = area.y + 2;

        let status = match self.game.state() {
            State::Playing => "Playing",
            State::Won => "Cracked it!",
            State::Lost => "Out of tries",
        };
        surface.set_string(
            ox,
            area.y,
            &format!(
                "Mastermind   colors 1-6   tries left {}   [{}]",
                self.game.remaining(),
                status
            ),
            header_style,
        );

        // One row per possible guess: past guesses show their pegs and score, the
        // active row shows the digits typed so far, and the rest are placeholders.
        let guesses = self.game.guesses();
        for row in 0..MAX_GUESSES {
            let y = oy + row as u16;
            surface.set_string(ox, y, &format!("{:>2} ", row + 1), empty_style);
            let px = ox + 3;
            if row < guesses.len() {
                let (guess, (black, white)) = &guesses[row];
                for (i, &c) in guess.iter().enumerate() {
                    let x = px + (i as u16) * 2;
                    surface.set_string(x, y, &c.to_string(), theme.get(color_key(c)));
                }
                let sx = px + (PEGS as u16) * 2 + 1;
                let mut off = 0u16;
                for _ in 0..*black {
                    surface.set_string(sx + off, y, "●", black_style);
                    off += 1;
                }
                for _ in 0..*white {
                    surface.set_string(sx + off, y, "○", white_style);
                    off += 1;
                }
            } else if row == guesses.len() && self.game.state() == State::Playing {
                let cur = self.game.current();
                for i in 0..PEGS {
                    let x = px + (i as u16) * 2;
                    if i < cur.len() {
                        let c = cur[i];
                        surface.set_string(x, y, &c.to_string(), theme.get(color_key(c)));
                    } else {
                        surface.set_string(x, y, "_", cursor_style);
                    }
                }
            } else {
                for i in 0..PEGS {
                    let x = px + (i as u16) * 2;
                    surface.set_string(x, y, "·", empty_style);
                }
            }
        }

        // On a finished game reveal the secret; otherwise show the controls.
        let sy = oy + MAX_GUESSES as u16 + 1;
        if self.game.state() == State::Playing {
            surface.set_string(
                ox,
                sy,
                "1-6 add · Backspace del · Enter guess · n new · q quit",
                text_style,
            );
        } else {
            surface.set_string(ox, sy, "code was ", text_style);
            let px = ox + 9;
            for (i, &c) in self.game.secret().iter().enumerate() {
                surface.set_string(
                    px + (i as u16) * 2,
                    sy,
                    &c.to_string(),
                    theme.get(color_key(c)),
                );
            }
            surface.set_string(ox, sy + 1, "n new · q quit", text_style);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a game with a hand-picked secret (bypassing the PRNG) so scoring and
    /// end-state tests are fully deterministic.
    fn game_with(secret: [u8; PEGS]) -> Game {
        Game {
            secret,
            guesses: Vec::new(),
            current: Vec::new(),
            state: State::Playing,
            rng: 1,
        }
    }

    /// Type a full guess and submit it.
    fn play(g: &mut Game, guess: [u8; PEGS]) {
        for &d in &guess {
            g.push_digit(d);
        }
        g.submit();
    }

    #[test]
    fn exact_match_scores_all_black_and_wins() {
        assert_eq!(score(&[1, 2, 3, 4], &[1, 2, 3, 4]), (4, 0));
        let mut g = game_with([1, 2, 3, 4]);
        play(&mut g, [1, 2, 3, 4]);
        assert_eq!(g.state(), State::Won);
    }

    #[test]
    fn no_shared_colors_scores_zero() {
        assert_eq!(score(&[1, 1, 1, 1], &[2, 2, 2, 2]), (0, 0));
    }

    #[test]
    fn right_colors_wrong_order_scores_all_white() {
        // A code with four distinct colors, guessed in reverse: nothing is in
        // place, but every color is present.
        assert_eq!(score(&[1, 2, 3, 4], &[4, 3, 2, 1]), (0, 4));
    }

    #[test]
    fn duplicates_are_counted_without_double_credit() {
        // secret 1 1 2 3 vs guess 1 2 1 1: one exact (slot 0), then the leftover
        // secret {1,2,3} overlaps guess {2,1,1} on a single 1 and a single 2.
        assert_eq!(score(&[1, 1, 2, 3], &[1, 2, 1, 1]), (1, 2));
    }

    #[test]
    fn wrong_guess_keeps_playing_and_records_score() {
        let mut g = game_with([6, 5, 4, 3]);
        play(&mut g, [1, 2, 3, 4]);
        assert_eq!(g.state(), State::Playing);
        assert_eq!(g.guesses().len(), 1);
        // 3 and 4 are present but misplaced.
        assert_eq!(g.guesses()[0].1, (0, 2));
        assert!(g.current().is_empty(), "the row clears after a submit");
    }

    #[test]
    fn ten_wrong_guesses_lose() {
        let mut g = game_with([1, 2, 3, 4]);
        for _ in 0..MAX_GUESSES {
            play(&mut g, [5, 5, 5, 5]);
        }
        assert_eq!(g.state(), State::Lost);
        assert_eq!(g.remaining(), 0);
        // Input is ignored once the game is over.
        g.push_digit(1);
        assert!(g.current().is_empty());
    }
}
