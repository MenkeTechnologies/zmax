//! Decipher — the zemacs port of GNU Emacs `decipher`, the cryptogram helper.
//!
//! A short quote is enciphered with a random simple substitution. Press a
//! CIPHER letter (upper-case, as shown) then the plain letter you think it is to
//! assign it; the decryption updates and the cipher-letter frequencies guide
//! you. `c` reveals the solution, `n` deals a new cryptogram, `q`/`Esc` quits.
//! The substitution, frequency count and solved-check are pure and unit-tested
//! (keys parse into a `decipher` keymap mode by `scripts/gen_port_report.py`).

use tui::buffer::Buffer as Surface;
use zemacs_view::graphics::Rect;

use crate::{
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

const QUOTES: &[&str] = &[
    "the quick brown fox jumps over the lazy dog",
    "to be or not to be that is the question",
    "all that glitters is not gold",
    "a journey of a thousand miles begins with a single step",
    "the only thing we have to fear is fear itself",
];

/// The pure cryptogram: the plaintext, the plain→cipher substitution, and the
/// player's cipher→plain guesses. No I/O — unit-tested.
#[derive(Clone)]
pub struct Puzzle {
    pub plain: String,
    /// enc[plain-letter index 0..26] = the upper-case cipher letter.
    pub enc: [char; 26],
    /// guess[cipher-letter index 0..26] = the player's plain letter, or None.
    pub guess: [Option<char>; 26],
}

impl Puzzle {
    /// Deterministically deal a cryptogram from `seed`.
    pub fn from_seed(seed: u64) -> Self {
        let plain = QUOTES[(seed as usize) % QUOTES.len()].to_string();
        // Fisher–Yates shuffle of A..Z for the substitution, with no letter
        // mapping to itself avoided-loosely (a self-map is legal in substitution).
        let mut perm: [u8; 26] = std::array::from_fn(|i| i as u8);
        let mut s = seed | 1;
        for i in (1..26).rev() {
            s = s
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let j = ((s >> 33) as usize) % (i + 1);
            perm.swap(i, j);
        }
        let enc = std::array::from_fn(|i| (b'A' + perm[i]) as char);
        Puzzle {
            plain,
            enc,
            guess: [None; 26],
        }
    }

    /// The enciphered text (letters upper-cased, everything else verbatim).
    pub fn ciphertext(&self) -> String {
        self.plain
            .chars()
            .map(|ch| {
                if ch.is_ascii_lowercase() {
                    self.enc[(ch as u8 - b'a') as usize]
                } else {
                    ch
                }
            })
            .collect()
    }

    /// The current decryption: a cipher letter with a guess shows the guessed
    /// plain letter (lower-case), otherwise the cipher letter (upper-case).
    pub fn worked(&self) -> String {
        self.ciphertext()
            .chars()
            .map(|ch| {
                if ch.is_ascii_uppercase() {
                    self.guess[(ch as u8 - b'A') as usize].unwrap_or(ch)
                } else {
                    ch
                }
            })
            .collect()
    }

    /// Assign `plain` as the guess for cipher letter `cipher`.
    pub fn assign(&mut self, cipher: char, plain: char) {
        if cipher.is_ascii_alphabetic() && plain.is_ascii_alphabetic() {
            self.guess[(cipher.to_ascii_uppercase() as u8 - b'A') as usize] =
                Some(plain.to_ascii_lowercase());
        }
    }

    /// Cipher-letter frequencies over the ciphertext (index 0..26).
    pub fn frequencies(&self) -> [u16; 26] {
        let mut f = [0u16; 26];
        for ch in self.ciphertext().chars() {
            if ch.is_ascii_uppercase() {
                f[(ch as u8 - b'A') as usize] += 1;
            }
        }
        f
    }

    /// Solved when every plain letter used maps back correctly.
    pub fn solved(&self) -> bool {
        self.plain
            .chars()
            .filter(|c| c.is_ascii_lowercase())
            .all(|c| {
                let cipher = self.enc[(c as u8 - b'a') as usize];
                self.guess[(cipher as u8 - b'A') as usize] == Some(c)
            })
    }
}

/// The interactive Decipher overlay.
pub struct Decipher {
    puzzle: Puzzle,
    seed: u64,
    pending: Option<char>,
    revealed: bool,
    status: String,
}

impl Decipher {
    pub fn new() -> Self {
        Decipher {
            puzzle: Puzzle::from_seed(1),
            seed: 1,
            pending: None,
            revealed: false,
            status: "Press a CIPHER letter, then the plain letter it stands for.".into(),
        }
    }

    fn deal(&mut self) {
        self.seed = self.seed.wrapping_add(1);
        self.puzzle = Puzzle::from_seed(self.seed);
        self.pending = None;
        self.revealed = false;
        self.status = "New cryptogram.".into();
    }

    fn key_letter(&mut self, ch: char) {
        match self.pending.take() {
            Some(cipher) => {
                self.puzzle.assign(cipher, ch);
                self.status = if self.puzzle.solved() {
                    "Solved!  n: new cryptogram".into()
                } else {
                    format!(
                        "{} = {}",
                        cipher.to_ascii_uppercase(),
                        ch.to_ascii_lowercase()
                    )
                };
            }
            None => {
                self.pending = Some(ch.to_ascii_uppercase());
                self.status = format!("{} = ?  (press the plain letter)", ch.to_ascii_uppercase());
            }
        }
    }
}

impl Default for Decipher {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for Decipher {
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
            key!('c') => {
                self.revealed = true;
                self.status = "Solution revealed.  n: new".into();
            }
            key!(c @ 'A'..='Z') | key!(c @ 'a'..='z') => self.key_letter(c),
            _ => {}
        }
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        let theme = &ctx.editor.theme;
        let bg = theme.get("ui.background");
        let text_style = theme.get("ui.text");
        let header_style = theme.get("ui.text.focus");
        let cipher_style = theme.get("ui.linenr");
        let plain_style = theme.get("ui.text.focus");
        let freq_style = theme.get("function");

        surface.clear_with(area, bg);
        if area.width < 30 || area.height < 10 {
            return;
        }
        let ox = area.x + 2;
        let oy = area.y + 2;
        surface.set_string(
            ox,
            area.y,
            "Decipher — crack the substitution",
            header_style,
        );

        // Cipher line above, current decryption (or solution) below.
        let cipher = self.puzzle.ciphertext();
        let worked = if self.revealed {
            self.puzzle.plain.clone()
        } else {
            self.puzzle.worked()
        };
        surface.set_string(ox, oy, &cipher, cipher_style);
        let wstyle = if self.revealed {
            plain_style
        } else {
            text_style
        };
        surface.set_string(ox, oy + 1, &worked, wstyle);

        // Frequency table (only letters that occur).
        let f = self.puzzle.frequencies();
        let mut fx = ox;
        let fy = oy + 3;
        surface.set_string(ox, fy, "freq:", freq_style);
        fx += 6;
        for (i, &n) in f.iter().enumerate() {
            if n > 0 {
                let letter = (b'A' + i as u8) as char;
                let cell = format!("{letter}{n} ");
                if fx + cell.len() as u16 >= area.x + area.width {
                    break;
                }
                surface.set_string(fx, fy, &cell, freq_style);
                fx += cell.len() as u16;
            }
        }

        let sy = oy + 5;
        surface.set_string(ox, sy, &self.status, text_style);
        surface.set_string(
            ox,
            sy + 1,
            "CIPHER then plain to guess · c reveal · n new · q quit",
            cipher_style,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ciphertext_is_a_consistent_substitution() {
        let p = Puzzle::from_seed(0);
        let ct = p.ciphertext();
        // Same plain letter always enciphers to the same cipher letter.
        let mut map = std::collections::HashMap::new();
        for (pc, cc) in p.plain.chars().zip(ct.chars()) {
            if pc.is_ascii_lowercase() {
                let e = map.entry(pc).or_insert(cc);
                assert_eq!(*e, cc, "inconsistent substitution for {pc}");
                assert!(cc.is_ascii_uppercase());
            } else {
                assert_eq!(pc, cc, "non-letters pass through");
            }
        }
    }

    #[test]
    fn assigning_the_true_mapping_solves_it() {
        let mut p = Puzzle::from_seed(3);
        assert!(!p.solved());
        for c in 'a'..='z' {
            let cipher = p.enc[(c as u8 - b'a') as usize];
            p.assign(cipher, c);
        }
        assert!(p.solved());
    }

    #[test]
    fn a_wrong_guess_does_not_solve() {
        let mut p = Puzzle::from_seed(2);
        // Assign everything shifted by one → wrong.
        for c in 'a'..='z' {
            let cipher = p.enc[(c as u8 - b'a') as usize];
            let wrong = if c == 'z' { 'a' } else { (c as u8 + 1) as char };
            p.assign(cipher, wrong);
        }
        assert!(!p.solved());
    }

    #[test]
    fn worked_text_shows_guesses_lowercase() {
        let mut p = Puzzle::from_seed(1);
        // Solve one letter and confirm it appears in lower case in the worked text.
        let plain = p.plain.chars().find(|c| c.is_ascii_lowercase()).unwrap();
        let cipher = p.enc[(plain as u8 - b'a') as usize];
        p.assign(cipher, plain);
        assert!(p.worked().contains(plain));
    }

    #[test]
    fn frequencies_sum_to_the_letter_count() {
        let p = Puzzle::from_seed(4);
        let total: u16 = p.frequencies().iter().sum();
        let letters = p.plain.chars().filter(|c| c.is_ascii_lowercase()).count() as u16;
        assert_eq!(total, letters);
    }
}
