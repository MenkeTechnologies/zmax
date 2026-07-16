//! Wordle — a small terminal word game for zmax.
//!
//! Guess the hidden five-letter word in six tries. Type letters `a`..`z` to fill
//! the current row, `Backspace` deletes the last letter, `Enter` submits a full
//! five-letter guess and `q`/`Esc` quits. After each guess every letter is tinted:
//! green (`function`) for a letter in the right spot, yellow (`warning`) for a
//! letter that is in the word but misplaced, and gray (`ui.linenr`) for a letter
//! that isn't in the word at all — with the standard duplicate-letter rule (a
//! repeated guess letter only earns a colour for each copy the target actually
//! has). Once the game ends (`Solved!` or the target is revealed) any key starts
//! a fresh round. Like Minesweeper this is turn-based: nothing animates, the board
//! only changes in response to a key. The scoring is a pure function and the word
//! is chosen by the same LCG the other games use, so a given seed is reproducible.
//!
//! Note: because `q` always quits, the letter `q` can't be typed into a guess —
//! an accepted limitation for keeping the quit key consistent with the others.

use tui::buffer::Buffer as Surface;
use zmax_view::graphics::Rect;

use crate::{
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

/// The candidate targets. Any typed five-letter guess is accepted; the target is
/// always drawn from this list so a seed reproduces the same word.
const WORDS: &[&str] = &[
    "crane", "slate", "trace", "adieu", "audio", "raise", "roast", "ratio", "brave", "cloud",
    "dance", "eager", "flame", "grape", "house", "ivory", "joker", "knead", "lemon", "mango",
    "night", "ocean", "peach", "quiet", "river", "stone", "table", "unity", "vivid", "waltz",
    "yield", "zebra", "bloom", "charm", "drift", "ember", "frost", "glide", "haste", "index",
];

const ROWS: usize = 6;
const COLS: usize = 5;

/// The verdict for a single guessed letter.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mark {
    /// Right letter, right spot (green).
    Correct,
    /// Right letter, wrong spot (yellow).
    Present,
    /// Not in the word (gray).
    Absent,
}

/// Score `guess` against `target` with the standard Wordle duplicate rule: first
/// hand out `Correct` for every exact-position match (consuming that target
/// letter), then walk the rest left-to-right and award `Present` only while an
/// unconsumed matching target letter remains, otherwise `Absent`. Pure — the
/// tests call it directly. Both strings are assumed to be five ASCII letters.
pub fn score(target: &str, guess: &str) -> [Mark; COLS] {
    let t: Vec<char> = target.chars().collect();
    let g: Vec<char> = guess.chars().collect();
    let mut marks = [Mark::Absent; COLS];
    let mut used = [false; COLS];

    // Greens first: exact matches consume the target slot they land on.
    for i in 0..COLS {
        if g[i] == t[i] {
            marks[i] = Mark::Correct;
            used[i] = true;
        }
    }
    // Yellows: a misplaced letter is Present only if an unconsumed copy remains.
    for i in 0..COLS {
        if marks[i] == Mark::Correct {
            continue;
        }
        for j in 0..COLS {
            if !used[j] && g[i] == t[j] {
                marks[i] = Mark::Present;
                used[j] = true;
                break;
            }
        }
    }
    marks
}

/// Where the game is: still guessing, solved, or out of tries.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    Playing,
    Won,
    Lost,
}

/// The pure Wordle state. No I/O, no timing — unit-tested. The target is drawn
/// with the same LCG the other games use, so `Game::new(seed)` is deterministic.
#[derive(Clone)]
pub struct Game {
    target: String,
    /// Submitted five-letter guesses, oldest first.
    guesses: Vec<String>,
    /// The row currently being typed (up to `COLS` letters).
    current: String,
    state: State,
    rng: u64,
}

impl Game {
    pub fn new(seed: u64) -> Self {
        let mut g = Game {
            target: String::new(),
            guesses: Vec::new(),
            current: String::new(),
            state: State::Playing,
            rng: seed | 1,
        };
        let i = (g.rand() % WORDS.len() as u64) as usize;
        g.target = WORDS[i].to_string();
        g
    }

    fn rand(&mut self) -> u64 {
        self.rng = self
            .rng
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.rng >> 33
    }

    pub fn state(&self) -> State {
        self.state
    }

    pub fn target(&self) -> &str {
        &self.target
    }

    pub fn guesses(&self) -> &[String] {
        &self.guesses
    }

    pub fn current(&self) -> &str {
        &self.current
    }

    /// Append a letter to the in-progress row (lowercased, `a`..`z`), if the game
    /// is still on and the row isn't full.
    pub fn type_letter(&mut self, c: char) {
        if self.state != State::Playing || self.current.len() >= COLS {
            return;
        }
        if c.is_ascii_alphabetic() {
            self.current.push(c.to_ascii_lowercase());
        }
    }

    /// Delete the last letter of the in-progress row.
    pub fn backspace(&mut self) {
        if self.state != State::Playing {
            return;
        }
        self.current.pop();
    }

    /// Submit the in-progress row once it holds a full five letters: record it,
    /// then win on an exact match or lose after the sixth guess.
    pub fn submit(&mut self) {
        if self.state != State::Playing || self.current.len() != COLS {
            return;
        }
        let guess = std::mem::take(&mut self.current);
        let won = guess == self.target;
        self.guesses.push(guess);
        if won {
            self.state = State::Won;
        } else if self.guesses.len() >= ROWS {
            self.state = State::Lost;
        }
    }
}

impl Default for Game {
    fn default() -> Self {
        Self::new(1)
    }
}

/// The interactive Wordle overlay.
pub struct Wordle {
    game: Game,
    seed: u64,
}

impl Wordle {
    pub fn new() -> Self {
        Wordle {
            game: Game::new(1),
            seed: 1,
        }
    }

    fn restart(&mut self) {
        self.seed = self.seed.wrapping_add(1);
        self.game = Game::new(self.seed);
    }
}

impl Default for Wordle {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for Wordle {
    fn handle_event(&mut self, event: &Event, _cx: &mut Context) -> EventResult {
        let key = match event {
            Event::Key(key) => *key,
            _ => return EventResult::Ignored(None),
        };
        let close: Callback = Box::new(|compositor: &mut Compositor, _cx| {
            compositor.pop();
        });
        // Quit is consistent with the other games and takes precedence.
        if let key!('q') | key!(Esc) | ctrl!('c') = key {
            return EventResult::Consumed(Some(close));
        }
        if self.game.state() != State::Playing {
            // The round is over: any other key deals a fresh word.
            self.restart();
            zmax_event::request_redraw();
            return EventResult::Consumed(None);
        }
        match key {
            key!(Enter) => self.game.submit(),
            key!(Backspace) => self.game.backspace(),
            _ => {
                if let ::zmax_view::keyboard::KeyCode::Char(c) = key.code {
                    if key.modifiers == ::zmax_view::keyboard::KeyModifiers::NONE {
                        self.game.type_letter(c);
                    }
                }
            }
        }
        zmax_event::request_redraw();
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        let theme = &ctx.editor.theme;
        let bg = theme.get("ui.background");
        let text_style = theme.get("ui.text");
        let header_style = theme.get("ui.text.focus");
        let empty_style = theme.get("ui.linenr");
        let typing_style = theme.get("ui.text.focus");
        let correct_style = theme.get("function");
        let present_style = theme.get("warning");
        let absent_style = theme.get("ui.linenr");
        let reveal_style = theme.get("error");

        surface.clear_with(area, bg);
        // Each cell is drawn two columns wide; the board plus chrome fits ~40x20.
        if area.width < (COLS as u16) * 2 + 4 || area.height < (ROWS as u16) + 5 {
            return;
        }
        let ox = area.x + 2;
        let oy = area.y + 2;

        let status = match self.game.state() {
            State::Playing => format!("guess {}/{}", self.game.guesses().len() + 1, ROWS),
            State::Won => "Solved!".to_string(),
            State::Lost => format!("Lost — {}", self.game.target().to_uppercase()),
        };
        surface.set_string(ox, area.y, &format!("Wordle    {}", status), header_style);

        let guesses = self.game.guesses();
        for r in 0..ROWS {
            for c in 0..COLS {
                let (glyph, style): (String, _) = if r < guesses.len() {
                    let g = &guesses[r];
                    let marks = score(self.game.target(), g);
                    let ch = g.as_bytes()[c] as char;
                    let st = match marks[c] {
                        Mark::Correct => correct_style,
                        Mark::Present => present_style,
                        Mark::Absent => absent_style,
                    };
                    (ch.to_ascii_uppercase().to_string(), st)
                } else if r == guesses.len() && self.game.state() == State::Playing {
                    let cur = self.game.current();
                    if c < cur.len() {
                        (
                            (cur.as_bytes()[c] as char).to_ascii_uppercase().to_string(),
                            typing_style,
                        )
                    } else {
                        ("·".to_string(), empty_style)
                    }
                } else if self.game.state() == State::Lost && r == guesses.len() {
                    // Reveal the answer beneath the exhausted guesses.
                    let ch = self.game.target().as_bytes()[c] as char;
                    (ch.to_ascii_uppercase().to_string(), reveal_style)
                } else {
                    ("·".to_string(), empty_style)
                };
                let x = ox + (c as u16) * 2;
                let y = oy + r as u16;
                surface.set_string(x, y, &glyph, style);
            }
        }

        let sy = oy + ROWS as u16 + 1;
        let hint = match self.game.state() {
            State::Playing => "type letters · Enter guess · Backspace edit · q quit",
            _ => "any key new game · q quit",
        };
        surface.set_string(ox, sy, hint, text_style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A blank game with a hand-chosen target, so wins and losses are exact.
    fn with_target(t: &str) -> Game {
        Game {
            target: t.to_string(),
            guesses: Vec::new(),
            current: String::new(),
            state: State::Playing,
            rng: 1,
        }
    }

    fn type_word(g: &mut Game, w: &str) {
        for c in w.chars() {
            g.type_letter(c);
        }
        g.submit();
    }

    #[test]
    fn exact_match_scores_all_correct() {
        assert_eq!(score("crane", "crane"), [Mark::Correct; COLS]);
    }

    #[test]
    fn absent_letter_scored_absent() {
        // "boils" shares no letter with "crane", so every mark is Absent.
        assert_eq!(score("crane", "boils"), [Mark::Absent; COLS]);
    }

    #[test]
    fn misplaced_letter_scored_present() {
        // Every letter of "range" is in "crane"; only the final 'e' is in place.
        let m = score("crane", "range");
        assert_eq!(
            m,
            [
                Mark::Present,
                Mark::Present,
                Mark::Present,
                Mark::Absent,
                Mark::Correct
            ]
        );
    }

    #[test]
    fn duplicate_letters_only_earn_one_colour() {
        // "hello" has two 'l's but "leapt" has one: the first misplaced 'l' is
        // Present, the second is Absent (no target copy left to consume).
        let m = score("leapt", "hello");
        assert_eq!(
            m,
            [
                Mark::Absent,  // h
                Mark::Correct, // e in position
                Mark::Present, // first l — one target 'l' available
                Mark::Absent,  // second l — none left
                Mark::Absent   // o
            ]
        );
    }

    #[test]
    fn correct_guess_wins() {
        let mut g = with_target("crane");
        type_word(&mut g, "crane");
        assert_eq!(g.state(), State::Won);
        assert_eq!(g.guesses().len(), 1);
    }

    #[test]
    fn six_wrong_guesses_lose_and_keep_the_target() {
        let mut g = with_target("crane");
        for _ in 0..ROWS {
            type_word(&mut g, "beast");
        }
        assert_eq!(g.state(), State::Lost);
        assert_eq!(g.guesses().len(), ROWS);
        assert_eq!(
            g.target(),
            "crane",
            "the answer survives the loss for reveal"
        );
    }
}
