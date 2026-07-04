//! Hangman — a small terminal word game for zemacs.
//!
//! Guess the hidden word one letter at a time. Press any letter `a`–`z` to guess
//! it: a correct guess reveals every occurrence in the masked word, a wrong guess
//! adds a body part to the gallows. Six misses and the poor fellow is done for;
//! reveal the whole word and you've saved him. `n` picks a fresh word and
//! `q`/`Esc` quits. Like Minesweeper this one is turn-based: nothing animates, so
//! there is no frame loop — the board only changes in response to a key. The word
//! logic is pure and unit-tested (the word is chosen by a small LCG so a given
//! seed is reproducible).

use tui::buffer::Buffer as Surface;
use zemacs_view::graphics::Rect;

use crate::{
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

/// Most misses the gallows can hold: head, body, two arms, two legs.
const MAX_MISSES: usize = 6;

/// The word list. All lowercase, no spaces, varied length.
const WORDS: &[&str] = &[
    "rust", "emacs", "buffer", "keyboard", "compiler", "terminal", "cursor", "window", "syntax",
    "closure", "pointer", "lambda", "macro", "vector", "thread", "kernel", "socket", "gallows",
    "puzzle", "cipher", "parser", "lexer", "widget", "palette",
];

/// The pure hangman game. No I/O, no timing — unit-tested. The word is chosen
/// with the same LCG the other games use, so `Game::new(seed)` is deterministic.
#[derive(Clone)]
pub struct Game {
    /// The secret word the player is trying to uncover.
    word: String,
    /// Letters the player has already guessed (both hits and misses).
    guessed: Vec<char>,
    /// How many wrong guesses so far.
    misses: usize,
    rng: u64,
}

impl Game {
    pub fn new(seed: u64) -> Self {
        let mut g = Game {
            word: String::new(),
            guessed: Vec::new(),
            misses: 0,
            rng: seed | 1,
        };
        let i = (g.rand() % WORDS.len() as u64) as usize;
        g.word = WORDS[i].to_string();
        g
    }

    fn rand(&mut self) -> u64 {
        self.rng = self
            .rng
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.rng >> 33
    }

    /// Wrong guesses so far (0..=`MAX_MISSES`).
    pub fn misses(&self) -> usize {
        self.misses
    }

    /// The letters guessed so far, in guess order.
    pub fn guessed(&self) -> &[char] {
        &self.guessed
    }

    /// The secret word (used by the renderer to colour guessed letters).
    pub fn word(&self) -> &str {
        &self.word
    }

    /// Guess a letter. Returns whether it appears in the word. An already-guessed
    /// letter, or a guess made after the game is over, is a no-op (and does not
    /// change the miss count). A wrong new guess adds a miss.
    pub fn guess(&mut self, ch: char) -> bool {
        if self.won() || self.lost() {
            return false;
        }
        if self.guessed.contains(&ch) {
            return self.word.contains(ch);
        }
        self.guessed.push(ch);
        let hit = self.word.contains(ch);
        if !hit {
            self.misses += 1;
        }
        hit
    }

    /// The word with un-guessed letters hidden, spaced for legibility
    /// (e.g. `"_ a _ _ e"`). Once the game is lost the whole word is shown.
    pub fn masked(&self) -> String {
        let mut s = String::new();
        for (i, c) in self.word.chars().enumerate() {
            if i > 0 {
                s.push(' ');
            }
            if self.lost() || self.guessed.contains(&c) {
                s.push(c);
            } else {
                s.push('_');
            }
        }
        s
    }

    /// Every letter of the word has been guessed.
    pub fn won(&self) -> bool {
        self.word.chars().all(|c| self.guessed.contains(&c))
    }

    /// The gallows is full.
    pub fn lost(&self) -> bool {
        self.misses >= MAX_MISSES
    }
}

impl Default for Game {
    fn default() -> Self {
        Self::new(1)
    }
}

/// The six lines of the gallows drawing, growing with the miss count.
fn gallows(misses: usize) -> [String; 6] {
    let head = if misses >= 1 { "O" } else { " " };
    let body = if misses >= 2 { "|" } else { " " };
    let larm = if misses >= 3 { "/" } else { " " };
    let rarm = if misses >= 4 { "\\" } else { " " };
    let lleg = if misses >= 5 { "/" } else { " " };
    let rleg = if misses >= 6 { "\\" } else { " " };
    [
        " +---+".to_string(),
        " |   |".to_string(),
        format!(" |   {head}"),
        format!(" |  {larm}{body}{rarm}"),
        format!(" |  {lleg} {rleg}"),
        " |".to_string(),
    ]
}

/// The interactive Hangman overlay.
pub struct Hangman {
    game: Game,
    seed: u64,
}

impl Hangman {
    pub fn new() -> Self {
        Hangman {
            game: Game::new(1),
            seed: 1,
        }
    }

    fn restart(&mut self) {
        self.seed = self.seed.wrapping_add(1);
        self.game = Game::new(self.seed);
    }
}

impl Default for Hangman {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for Hangman {
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
            key!('n') => self.restart(),
            _ => {
                // Any plain a–z key is a letter guess.
                if let ::zemacs_view::keyboard::KeyCode::Char(ch) = key.code {
                    if ch.is_ascii_lowercase() {
                        self.game.guess(ch);
                    }
                }
            }
        }
        zemacs_event::request_redraw();
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        let theme = &ctx.editor.theme;
        let bg = theme.get("ui.background");
        let text_style = theme.get("ui.text");
        let header_style = theme.get("ui.text.focus");
        let dim_style = theme.get("ui.linenr");
        let word_style = theme.get("ui.text.focus");
        let good_style = theme.get("function");
        let bad_style = theme.get("error");

        surface.clear_with(area, bg);
        if area.width < 30 || area.height < 18 {
            return;
        }
        let ox = area.x + 2;
        let oy = area.y + 2;

        let status = if self.game.won() {
            "You saved him!"
        } else if self.game.lost() {
            "Game over"
        } else {
            "Playing"
        };
        surface.set_string(
            ox,
            area.y,
            &format!(
                "Hangman  misses {}/{}  [{}]",
                self.game.misses(),
                MAX_MISSES,
                status
            ),
            header_style,
        );

        // The gallows and its base.
        let lines = gallows(self.game.misses());
        for (i, line) in lines.iter().enumerate() {
            surface.set_string(ox, oy + i as u16, line, dim_style);
        }
        surface.set_string(ox, oy + lines.len() as u16, "======", dim_style);

        // The masked word.
        let wy = oy + lines.len() as u16 + 2;
        surface.set_string(ox, wy, &self.game.masked(), word_style);

        // The guessed-letters row: hits in "function", misses in "error".
        let gy = wy + 2;
        surface.set_string(ox, gy, "guessed:", text_style);
        let mut gx = ox + 9;
        for &c in self.game.guessed() {
            let style = if self.game.word().contains(c) {
                good_style
            } else {
                bad_style
            };
            surface.set_string(gx, gy, &c.to_string(), style);
            gx += 2;
        }

        let sy = gy + 2;
        surface.set_string(ox, sy, "a-z guess · n new · q quit", text_style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A game with a hand-picked word, ready for deterministic play.
    fn with_word(w: &str) -> Game {
        Game {
            word: w.to_string(),
            guessed: Vec::new(),
            misses: 0,
            rng: 1,
        }
    }

    #[test]
    fn correct_guess_reveals_all_occurrences() {
        let mut g = with_word("apple");
        assert!(g.guess('p'), "'p' is in the word");
        assert_eq!(g.masked(), "_ p p _ _", "both p's are revealed at once");
        assert_eq!(g.misses(), 0, "a correct guess is not a miss");
    }

    #[test]
    fn wrong_guess_increments_misses() {
        let mut g = with_word("apple");
        assert!(!g.guess('z'), "'z' is not in the word");
        assert_eq!(g.misses(), 1);
        assert_eq!(g.masked(), "_ _ _ _ _", "nothing is revealed");
    }

    #[test]
    fn repeated_guess_is_a_noop() {
        let mut g = with_word("apple");
        g.guess('z');
        g.guess('z');
        assert_eq!(
            g.misses(),
            1,
            "guessing the same wrong letter twice costs one miss"
        );
        assert_eq!(g.guessed().len(), 1, "the letter is recorded only once");
    }

    #[test]
    fn win_when_whole_word_revealed() {
        let mut g = with_word("cat");
        for c in "cat".chars() {
            g.guess(c);
        }
        assert!(g.won());
        assert!(!g.lost());
        assert_eq!(g.masked(), "c a t");
    }

    #[test]
    fn loss_after_six_wrong_guesses() {
        let mut g = with_word("cat");
        for c in ['b', 'd', 'e', 'f', 'g', 'h'] {
            g.guess(c);
        }
        assert_eq!(g.misses(), MAX_MISSES);
        assert!(g.lost());
        assert!(!g.won());
        assert_eq!(g.masked(), "c a t", "the word is revealed on a loss");
    }
}
