//! Mpuz — the zmax port of GNU Emacs `mpuz`, the multiplication puzzle.
//!
//! A long multiplication is shown with every digit 0-9 replaced by a fixed but
//! secret letter A-J. Deduce the mapping: press a letter, then the digit you
//! think it stands for; a correct guess reveals that digit everywhere, a wrong
//! one counts as an error. Solve every digit that appears to win. `n` deals a
//! new puzzle, `q`/`Esc` quits. The cipher, the arithmetic and guess-checking
//! are pure and unit-tested (keys parse into an `mpuz` keymap mode by
//! `scripts/gen_port_report.py`).

use tui::buffer::Buffer as Surface;
use zmax_view::graphics::Rect;

use crate::{
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

/// The pure puzzle: the secret digit→letter cipher, the multiplication to solve,
/// which digits appear, and which have been revealed. No I/O — unit-tested.
#[derive(Clone)]
pub struct Puzzle {
    /// `digit_to_letter[d]` = the letter shown for digit d.
    pub cipher: [char; 10],
    pub m1: u32,
    pub m2: u32,
    /// Which digits occur anywhere in the shown numbers (must be solved).
    pub present: [bool; 10],
    /// Which digits the player has correctly identified.
    pub revealed: [bool; 10],
    pub errors: u32,
}

fn digits_of(mut n: u32, present: &mut [bool; 10]) {
    if n == 0 {
        present[0] = true;
        return;
    }
    while n > 0 {
        present[(n % 10) as usize] = true;
        n /= 10;
    }
}

impl Puzzle {
    /// Deterministically deal a puzzle from `seed`: a random 3-digit × 2-digit
    /// multiplication and a random digit→letter (A-J) bijection.
    pub fn from_seed(seed: u64) -> Self {
        let mut s = seed | 1;
        let mut next = || {
            s = s
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            s >> 33
        };
        let m1 = 100 + (next() % 900) as u32; // 100..=999
        let m2 = 10 + (next() % 90) as u32; //   10..=99

        // Fisher–Yates shuffle of 0..10 → a digit→letter bijection.
        let mut perm: [usize; 10] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9];
        for i in (1..10).rev() {
            let j = (next() as usize) % (i + 1);
            perm.swap(i, j);
        }
        let mut cipher = ['A'; 10];
        for (d, slot) in cipher.iter_mut().enumerate() {
            *slot = (b'A' + perm[d] as u8) as char;
        }

        let mut present = [false; 10];
        for n in [m1, m2, m1 * (m2 % 10), m1 * (m2 / 10), m1 * m2] {
            digits_of(n, &mut present);
        }
        Puzzle {
            cipher,
            m1,
            m2,
            present,
            revealed: [false; 10],
            errors: 0,
        }
    }

    /// The digit a shown `letter` stands for, if any.
    pub fn digit_for(&self, letter: char) -> Option<usize> {
        let up = letter.to_ascii_uppercase();
        self.cipher.iter().position(|&c| c == up)
    }

    /// Guess that `letter` is `digit`. Reveals the digit on success; otherwise
    /// records an error. Returns whether the guess was correct.
    pub fn guess(&mut self, letter: char, digit: usize) -> bool {
        match self.digit_for(letter) {
            Some(d) if d == digit && self.present[d] => {
                self.revealed[d] = true;
                true
            }
            _ => {
                self.errors += 1;
                false
            }
        }
    }

    /// Won when every present digit has been revealed.
    pub fn solved(&self) -> bool {
        (0..10).all(|d| !self.present[d] || self.revealed[d])
    }

    /// The glyph to show for digit `d`: the digit itself once revealed, else its
    /// cipher letter.
    fn glyph(&self, d: usize) -> char {
        if self.revealed[d] {
            (b'0' + d as u8) as char
        } else {
            self.cipher[d]
        }
    }

    /// Render number `n` using the current reveal state (right-aligned to `w`).
    fn show(&self, n: u32, w: usize) -> String {
        let s: String = n
            .to_string()
            .chars()
            .map(|c| self.glyph((c as u8 - b'0') as usize))
            .collect();
        format!("{s:>w$}")
    }
}

/// The interactive Mpuz overlay.
pub struct Mpuz {
    puzzle: Puzzle,
    seed: u64,
    pending: Option<char>,
    status: String,
}

impl Mpuz {
    pub fn new() -> Self {
        Mpuz {
            puzzle: Puzzle::from_seed(1),
            seed: 1,
            pending: None,
            status: "Press a letter, then the digit you think it is.".into(),
        }
    }

    fn deal(&mut self) {
        self.seed = self.seed.wrapping_add(1);
        self.puzzle = Puzzle::from_seed(self.seed);
        self.pending = None;
        self.status = "New puzzle.".into();
    }

    fn key_letter(&mut self, l: char) {
        self.pending = Some(l.to_ascii_uppercase());
        self.status = format!("{} = ?  (press a digit)", l.to_ascii_uppercase());
    }

    fn key_digit(&mut self, d: usize) {
        if let Some(l) = self.pending.take() {
            if self.puzzle.guess(l, d) {
                self.status = if self.puzzle.solved() {
                    format!("Solved with {} errors!  n: new puzzle", self.puzzle.errors)
                } else {
                    format!("{l} = {d}. Correct.")
                };
            } else {
                self.status = format!("{l} is not {d}. Errors: {}", self.puzzle.errors);
            }
        } else {
            self.status = "Press a letter first.".into();
        }
    }
}

impl Default for Mpuz {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for Mpuz {
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
            key!('n') => self.deal(),
            key!(c @ 'A'..='J') | key!(c @ 'a'..='j') => self.key_letter(c),
            key!(c @ '0'..='9') => self.key_digit((c as u8 - b'0') as usize),
            _ => {}
        }
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        let theme = &ctx.editor.theme;
        let bg = theme.get("ui.background");
        let text_style = theme.get("ui.text");
        let header_style = theme.get("ui.text.focus");
        let num_style = theme.get("ui.text.focus");
        let rule_style = theme.get("ui.linenr");

        surface.clear_with(area, bg);
        if area.width < 20 || area.height < 10 {
            return;
        }
        let ox = area.x + 3;
        let oy = area.y + 2;
        surface.set_string(ox, area.y, "Mpuz — crack the multiplication", header_style);

        let p = &self.puzzle;
        let w = (p.m1 * p.m2).to_string().len();
        let p1 = p.m1 * (p.m2 % 10);
        let p2 = p.m1 * (p.m2 / 10);
        let rows = [
            p.show(p.m1, w),
            format!("{}{}", "x", p.show(p.m2, w - 1)),
            p.show(p1, w),
            p.show(p2, w),
            p.show(p.m1 * p.m2, w),
        ];
        // Draw with a rule before the partials and before the total.
        surface.set_string(ox, oy, &rows[0], num_style);
        surface.set_string(ox, oy + 1, &rows[1], num_style);
        surface.set_string(ox, oy + 2, &"─".repeat(w), rule_style);
        surface.set_string(ox, oy + 3, &rows[2], num_style);
        surface.set_string(ox, oy + 4, &rows[3], num_style);
        surface.set_string(ox, oy + 5, &"─".repeat(w), rule_style);
        surface.set_string(ox, oy + 6, &rows[4], num_style);

        let sy = oy + 8;
        surface.set_string(ox, sy, &self.status, text_style);
        surface.set_string(
            ox,
            sy + 1,
            "letter then digit to guess · n new · q quit",
            rule_style,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cipher_is_a_bijection() {
        let p = Puzzle::from_seed(5);
        let mut seen = [false; 10];
        for &c in &p.cipher {
            let i = (c as u8 - b'A') as usize;
            assert!(i < 10 && !seen[i], "letter repeated: {c}");
            seen[i] = true;
        }
    }

    #[test]
    fn present_matches_the_actual_digits() {
        let p = Puzzle::from_seed(3);
        // Every digit flagged present must occur in one of the shown numbers.
        let mut want = [false; 10];
        for n in [
            p.m1,
            p.m2,
            p.m1 * (p.m2 % 10),
            p.m1 * (p.m2 / 10),
            p.m1 * p.m2,
        ] {
            digits_of(n, &mut want);
        }
        assert_eq!(p.present, want);
    }

    #[test]
    fn correct_guess_reveals_wrong_guess_errors() {
        let mut p = Puzzle::from_seed(9);
        // Find a present digit and its letter.
        let d = (0..10).find(|&d| p.present[d]).unwrap();
        let letter = p.cipher[d];
        assert!(p.guess(letter, d), "true mapping should be accepted");
        assert!(p.revealed[d]);
        // A wrong digit for the same letter is an error.
        let wrong = (0..10).find(|&x| x != d).unwrap();
        let before = p.errors;
        assert!(!p.guess(letter, wrong));
        assert_eq!(p.errors, before + 1);
    }

    #[test]
    fn solving_all_present_digits_wins() {
        let mut p = Puzzle::from_seed(2);
        assert!(!p.solved());
        for d in 0..10 {
            if p.present[d] {
                assert!(p.guess(p.cipher[d], d));
            }
        }
        assert!(p.solved());
    }
}
