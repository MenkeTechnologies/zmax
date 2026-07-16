//! Yahtzee — a solo terminal Yahtzee for zmax.
//!
//! Roll five dice up to three times a turn, holding the ones you like with the
//! number keys `1`..`5`, then assign the dice to one of the thirteen scoring
//! categories. Move the cursor over the scorecard with the arrows or `hjkl`,
//! `r`/`SPC` re-rolls the un-held dice, `RET` scores the current dice into the
//! highlighted category, `n` starts a new game and `q`/`Esc` quits. Like
//! Minesweeper this game is turn-based — nothing animates, the state only
//! changes in response to a key. The scoring is a pure function and unit-tested;
//! the dice are rolled by the same LCG the other games use, so `Game::new(seed)`
//! is deterministic.

use tui::buffer::Buffer as Surface;
use zmax_view::graphics::Rect;

use crate::{
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

/// The thirteen Yahtzee scoring categories.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Category {
    Ones,
    Twos,
    Threes,
    Fours,
    Fives,
    Sixes,
    ThreeKind,
    FourKind,
    FullHouse,
    SmallStraight,
    LargeStraight,
    Yahtzee,
    Chance,
}

/// The categories in scorecard order.
const CATS: [Category; 13] = [
    Category::Ones,
    Category::Twos,
    Category::Threes,
    Category::Fours,
    Category::Fives,
    Category::Sixes,
    Category::ThreeKind,
    Category::FourKind,
    Category::FullHouse,
    Category::SmallStraight,
    Category::LargeStraight,
    Category::Yahtzee,
    Category::Chance,
];

/// Human names for the scorecard, aligned with `CATS`.
const NAMES: [&str; 13] = [
    "Ones",
    "Twos",
    "Threes",
    "Fours",
    "Fives",
    "Sixes",
    "3 of a Kind",
    "4 of a Kind",
    "Full House",
    "Sm Straight",
    "Lg Straight",
    "Yahtzee",
    "Chance",
];

/// Bonus awarded when the upper section (Ones..Sixes) totals at least 63.
const UPPER_BONUS_THRESHOLD: u32 = 63;
const UPPER_BONUS: u32 = 35;
const MAX_ROLLS: u8 = 3;

/// Pure Yahtzee scoring. Given five dice and a category, returns the score that
/// assigning those dice to the category would earn (0 when the dice don't
/// qualify). No I/O, no state — unit-tested directly by the test module.
pub fn score_category(dice: &[u8; 5], cat: Category) -> u32 {
    let sum: u32 = dice.iter().map(|&d| d as u32).sum();
    // counts[face] = how many dice show that face (index 1..=6 used).
    let mut counts = [0u8; 7];
    for &d in dice {
        if (1..=6).contains(&d) {
            counts[d as usize] += 1;
        }
    }
    let present = |f: usize| counts[f] > 0;
    match cat {
        Category::Ones => counts[1] as u32,
        Category::Twos => counts[2] as u32 * 2,
        Category::Threes => counts[3] as u32 * 3,
        Category::Fours => counts[4] as u32 * 4,
        Category::Fives => counts[5] as u32 * 5,
        Category::Sixes => counts[6] as u32 * 6,
        Category::ThreeKind => {
            if counts.iter().any(|&c| c >= 3) {
                sum
            } else {
                0
            }
        }
        Category::FourKind => {
            if counts.iter().any(|&c| c >= 4) {
                sum
            } else {
                0
            }
        }
        Category::FullHouse => {
            let has3 = counts.contains(&3);
            let has2 = counts.contains(&2);
            if has3 && has2 {
                25
            } else {
                0
            }
        }
        Category::SmallStraight => {
            if (present(1) && present(2) && present(3) && present(4))
                || (present(2) && present(3) && present(4) && present(5))
                || (present(3) && present(4) && present(5) && present(6))
            {
                30
            } else {
                0
            }
        }
        Category::LargeStraight => {
            if (present(1) && present(2) && present(3) && present(4) && present(5))
                || (present(2) && present(3) && present(4) && present(5) && present(6))
            {
                40
            } else {
                0
            }
        }
        Category::Yahtzee => {
            if counts.contains(&5) {
                50
            } else {
                0
            }
        }
        Category::Chance => sum,
    }
}

/// The pure solo-Yahtzee game. No I/O, no timing — unit-tested. Dice are rolled
/// with the same LCG the other games use, so `Game::new(seed)` is deterministic.
#[derive(Clone)]
pub struct Game {
    /// The five dice, each 1..=6.
    dice: [u8; 5],
    /// Which dice are held between rolls.
    held: [bool; 5],
    /// Rolls used this turn (0..=`MAX_ROLLS`).
    rolls: u8,
    /// Locked category scores; `None` until the category is filled.
    scores: [Option<u32>; 13],
    /// Cursor index into `CATS` (the highlighted scorecard row).
    cursor: usize,
    rng: u64,
}

impl Game {
    pub fn new(seed: u64) -> Self {
        let mut g = Game {
            dice: [1; 5],
            held: [false; 5],
            rolls: 0,
            scores: [None; 13],
            cursor: 0,
            rng: seed | 1,
        };
        // Open the first turn with a fresh roll so there are dice to look at.
        g.roll();
        g
    }

    fn rand(&mut self) -> u64 {
        self.rng = self
            .rng
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.rng >> 33
    }

    pub fn dice(&self) -> &[u8; 5] {
        &self.dice
    }

    pub fn held(&self, i: usize) -> bool {
        self.held[i]
    }

    pub fn rolls(&self) -> u8 {
        self.rolls
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// Locked score for a scorecard row, or `None` if still open.
    pub fn score_at(&self, i: usize) -> Option<u32> {
        self.scores[i]
    }

    /// Number of categories already scored.
    pub fn filled(&self) -> usize {
        self.scores.iter().filter(|s| s.is_some()).count()
    }

    /// The game is over once every category is filled.
    pub fn is_over(&self) -> bool {
        self.filled() == 13
    }

    /// Sum of the locked upper-section scores (Ones..Sixes).
    pub fn upper_total(&self) -> u32 {
        self.scores[0..6].iter().flatten().sum()
    }

    /// +35 once the upper section reaches the bonus threshold.
    pub fn upper_bonus(&self) -> u32 {
        if self.upper_total() >= UPPER_BONUS_THRESHOLD {
            UPPER_BONUS
        } else {
            0
        }
    }

    /// Grand total: every locked score plus the upper-section bonus.
    pub fn grand_total(&self) -> u32 {
        let base: u32 = self.scores.iter().flatten().sum();
        base + self.upper_bonus()
    }

    /// Toggle whether die `i` (0..5) is held. Only meaningful mid-turn, before
    /// the last roll — a held die is kept on the next re-roll.
    pub fn toggle_hold(&mut self, i: usize) {
        if self.is_over() || i >= 5 {
            return;
        }
        self.held[i] = !self.held[i];
    }

    /// Re-roll every un-held die, up to `MAX_ROLLS` times a turn. Does nothing
    /// once the turn's rolls are spent or the game is over.
    pub fn roll(&mut self) {
        if self.is_over() || self.rolls >= MAX_ROLLS {
            return;
        }
        for i in 0..5 {
            if !self.held[i] {
                self.dice[i] = (self.rand() % 6) as u8 + 1;
            }
        }
        self.rolls += 1;
    }

    /// Move the scorecard cursor by `d`, clamped to the thirteen rows.
    pub fn move_cursor(&mut self, d: i32) {
        let c = (self.cursor as i32 + d).clamp(0, 12) as usize;
        self.cursor = c;
    }

    /// The score the current dice would earn in category `i` if taken now.
    pub fn potential(&self, i: usize) -> u32 {
        score_category(&self.dice, CATS[i])
    }

    /// Assign the current dice to the highlighted category and advance to the
    /// next turn. Ignored if that category is already filled or the game is
    /// over. When categories remain, a fresh turn opens with a new roll.
    pub fn score_current(&mut self) {
        if self.is_over() || self.scores[self.cursor].is_some() {
            return;
        }
        self.scores[self.cursor] = Some(self.potential(self.cursor));
        if self.is_over() {
            return;
        }
        // Fresh turn: release the dice, reset the roll count, and roll once. Park
        // the cursor on the next still-open category so it's ready to score.
        self.held = [false; 5];
        self.rolls = 0;
        self.roll();
        for step in 1..=13 {
            let i = (self.cursor + step) % 13;
            if self.scores[i].is_none() {
                self.cursor = i;
                break;
            }
        }
    }
}

impl Default for Game {
    fn default() -> Self {
        Self::new(1)
    }
}

fn die_face(v: u8) -> &'static str {
    match v {
        1 => "⚀",
        2 => "⚁",
        3 => "⚂",
        4 => "⚃",
        5 => "⚄",
        6 => "⚅",
        _ => "·",
    }
}

/// The interactive Yahtzee overlay.
pub struct Yahtzee {
    game: Game,
    seed: u64,
}

impl Yahtzee {
    pub fn new() -> Self {
        Yahtzee {
            game: Game::new(1),
            seed: 1,
        }
    }

    fn restart(&mut self) {
        self.seed = self.seed.wrapping_add(1);
        self.game = Game::new(self.seed);
    }
}

impl Default for Yahtzee {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for Yahtzee {
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
            key!('1') => self.game.toggle_hold(0),
            key!('2') => self.game.toggle_hold(1),
            key!('3') => self.game.toggle_hold(2),
            key!('4') => self.game.toggle_hold(3),
            key!('5') => self.game.toggle_hold(4),
            key!('r') | key!(' ') => self.game.roll(),
            key!(Left) | key!('h') | key!(Up) | key!('k') => self.game.move_cursor(-1),
            key!(Right) | key!('l') | key!(Down) | key!('j') => self.game.move_cursor(1),
            key!(Enter) => self.game.score_current(),
            key!('n') => self.restart(),
            _ => {}
        }
        zmax_event::request_redraw();
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        let theme = &ctx.editor.theme;
        let bg = theme.get("ui.background");
        let text_style = theme.get("ui.text");
        let header_style = theme.get("ui.text.focus");
        let dim_style = theme.get("ui.linenr");
        let cursor_style = theme.get("ui.selection");
        let held_style = theme.get("function");
        let locked_style = theme.get("function");
        let roll_style = theme.get("warning");
        let over_style = theme.get("error");

        surface.clear_with(area, bg);
        if area.width < 40 || area.height < 21 {
            return;
        }
        let ox = area.x + 2;
        let oy = area.y + 2;

        surface.set_string(
            ox,
            area.y,
            &format!(
                "Yahtzee  total {}  turn {}/13",
                self.game.grand_total(),
                (self.game.filled() + 1).min(13),
            ),
            header_style,
        );

        // Dice row, three columns apart, with a held marker beneath each die.
        for i in 0..5 {
            let x = ox + (i as u16) * 3;
            let face = die_face(self.game.dice()[i]);
            let style = if self.game.held(i) {
                held_style
            } else {
                text_style
            };
            surface.set_string(x, oy, face, style);
            let (mark, mstyle) = if self.game.held(i) {
                ("hld", held_style)
            } else {
                (["1", "2", "3", "4", "5"][i], dim_style)
            };
            surface.set_string(x, oy + 1, mark, mstyle);
        }
        surface.set_string(
            ox + 18,
            oy,
            &format!("roll {}/{}", self.game.rolls(), MAX_ROLLS),
            roll_style,
        );

        // Scorecard: one row per category, locked scores lit, open ones dim.
        let scy = oy + 3;
        for i in 0..13 {
            let y = scy + i as u16;
            let (val, mut style) = match self.game.score_at(i) {
                Some(v) => (v, locked_style),
                None => (self.game.potential(i), dim_style),
            };
            if i == self.game.cursor() {
                style = cursor_style;
            }
            surface.set_string(ox, y, &format!("{:<12}{:>3}", NAMES[i], val), style);
        }
        // Upper bonus line beneath the scorecard.
        surface.set_string(
            ox,
            scy + 13,
            &format!(
                "Upper {:>2}/{}  bonus {}",
                self.game.upper_total(),
                UPPER_BONUS_THRESHOLD,
                self.game.upper_bonus(),
            ),
            dim_style,
        );

        let fy = scy + 15;
        if self.game.is_over() {
            surface.set_string(
                ox,
                fy,
                &format!(
                    "GAME OVER  total {} · n new · q quit",
                    self.game.grand_total()
                ),
                over_style,
            );
        } else {
            surface.set_string(
                ox,
                fy,
                "1-5 hold · r/SPC roll · move · RET score · n new · q quit",
                text_style,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn threes_score_triple_the_count() {
        // Two 3s → 6, three 3s → 9; unrelated faces don't contribute.
        assert_eq!(score_category(&[3, 3, 1, 2, 5], Category::Threes), 6);
        assert_eq!(score_category(&[3, 3, 3, 1, 2], Category::Threes), 9);
    }

    #[test]
    fn full_house_pays_flat_and_busts_otherwise() {
        assert_eq!(score_category(&[2, 2, 3, 3, 3], Category::FullHouse), 25);
        assert_eq!(score_category(&[1, 2, 3, 4, 5], Category::FullHouse), 0);
    }

    #[test]
    fn small_straight_scores_thirty() {
        assert_eq!(
            score_category(&[1, 2, 3, 4, 6], Category::SmallStraight),
            30
        );
        assert_eq!(score_category(&[1, 1, 2, 2, 5], Category::SmallStraight), 0);
    }

    #[test]
    fn large_straight_scores_forty() {
        assert_eq!(
            score_category(&[2, 3, 4, 5, 6], Category::LargeStraight),
            40
        );
        // A small straight is not a large straight.
        assert_eq!(score_category(&[1, 2, 3, 4, 6], Category::LargeStraight), 0);
    }

    #[test]
    fn yahtzee_scores_fifty() {
        assert_eq!(score_category(&[5, 5, 5, 5, 5], Category::Yahtzee), 50);
        assert_eq!(score_category(&[5, 5, 5, 5, 4], Category::Yahtzee), 0);
    }

    #[test]
    fn chance_sums_all_and_four_of_a_kind_qualifies() {
        assert_eq!(score_category(&[1, 2, 3, 4, 5], Category::Chance), 15);
        // Four 4s and a 1 sum to 17 and satisfy four-of-a-kind.
        assert_eq!(score_category(&[4, 4, 4, 4, 1], Category::FourKind), 17);
        assert_eq!(score_category(&[4, 4, 4, 2, 1], Category::FourKind), 0);
    }
}
