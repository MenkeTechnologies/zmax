//! Video Poker — a small terminal Jacks-or-Better machine for zemacs.
//!
//! Draw poker against the paytable. `SPC`/`Enter` deals the opening hand; then
//! `1`..`5` toggle a HOLD on each card and a second `SPC`/`Enter` draws
//! replacements for the un-held cards and pays out. `←`/`→` change the bet
//! between hands, `n` starts a fresh shuffle and `q`/`Esc` quits. Like
//! Minesweeper and Blackjack this one is turn-based: nothing animates, so there
//! is no frame loop — the machine only changes in response to a key. The deck
//! and the hand evaluator are pure and unit-tested (the deck is shuffled by a
//! small LCG so a given seed is reproducible).

use tui::buffer::Buffer as Surface;
use zemacs_view::graphics::Rect;

use crate::{
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

/// Credits the player starts with.
const START_CREDITS: u32 = 100;
/// Credits wagered by default (1..=MAX_BET).
const BET: u32 = 5;
/// The most credits that can be wagered on one hand.
const MAX_BET: u32 = 5;
/// Reshuffle a fresh 52-card deck once it drops below this many cards (a round
/// can draw up to ten cards, so this keeps a comfortable margin).
const RESHUFFLE_AT: usize = 15;

/// A single playing card. `rank` is 2..=14 where 11=J, 12=Q, 13=K and 14=A;
/// `suit` is 0=♠, 1=♥, 2=♦, 3=♣.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Card {
    pub rank: u8,
    pub suit: u8,
}

/// Where a round is: waiting to deal, choosing holds, or paid out.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    /// No hand on the table — the next deal opens a round.
    Deal,
    /// The opening hand is dealt; the player picks holds, then draws.
    Draw,
    /// The draw is done and the hand has been scored and paid.
    Over,
}

/// The category a five-card hand falls into, best first, matching the
/// Jacks-or-Better paytable.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HandRank {
    RoyalFlush,
    StraightFlush,
    FourOfAKind,
    FullHouse,
    Flush,
    Straight,
    ThreeOfAKind,
    TwoPair,
    /// A single pair of Jacks, Queens, Kings or Aces — the lowest paying hand.
    JacksOrBetter,
    /// Anything that does not pay.
    Nothing,
}

/// Score a completed five-card hand. Pure — the tests drive it with fixed hands.
/// Handles the ace-high straight/royal and the wheel straight A-2-3-4-5.
pub fn evaluate(cards: &[Card; 5]) -> HandRank {
    // Tally how many of each rank we hold (index by rank, 2..=14).
    let mut counts = [0u8; 15];
    for c in cards {
        counts[c.rank as usize] += 1;
    }
    // The multiplicities, largest first: e.g. a full house is [3, 2].
    let mut freq: Vec<u8> = counts.iter().filter(|&&x| x > 0).copied().collect();
    freq.sort_unstable_by(|a, b| b.cmp(a));

    let flush = cards.iter().all(|c| c.suit == cards[0].suit);

    let mut ranks: Vec<u8> = cards.iter().map(|c| c.rank).collect();
    ranks.sort_unstable();
    // The wheel: ace plays low, A-2-3-4-5.
    let wheel = ranks == [2, 3, 4, 5, 14];
    let distinct = {
        let mut r = ranks.clone();
        r.dedup();
        r.len() == 5
    };
    let straight = distinct && (wheel || ranks[4] - ranks[0] == 4);

    if straight && flush {
        // A non-wheel ace-high straight flush is 10-J-Q-K-A: a royal.
        if !wheel && ranks[4] == 14 {
            return HandRank::RoyalFlush;
        }
        return HandRank::StraightFlush;
    }
    if freq[0] == 4 {
        return HandRank::FourOfAKind;
    }
    if freq[0] == 3 && freq[1] == 2 {
        return HandRank::FullHouse;
    }
    if flush {
        return HandRank::Flush;
    }
    if straight {
        return HandRank::Straight;
    }
    if freq[0] == 3 {
        return HandRank::ThreeOfAKind;
    }
    if freq[0] == 2 && freq[1] == 2 {
        return HandRank::TwoPair;
    }
    if freq[0] == 2 {
        // The lone pair pays only when it is Jacks or better.
        let pair_rank = counts.iter().position(|&x| x == 2).unwrap() as u8;
        if pair_rank >= 11 {
            return HandRank::JacksOrBetter;
        }
        return HandRank::Nothing;
    }
    HandRank::Nothing
}

/// The credits paid for `rank` at a given `bet`, per the Jacks-or-Better table
/// (per-credit odds scaled by the wager).
pub fn payout(rank: HandRank, bet: u32) -> u32 {
    let per = match rank {
        HandRank::RoyalFlush => 250,
        HandRank::StraightFlush => 50,
        HandRank::FourOfAKind => 25,
        HandRank::FullHouse => 9,
        HandRank::Flush => 6,
        HandRank::Straight => 4,
        HandRank::ThreeOfAKind => 3,
        HandRank::TwoPair => 2,
        HandRank::JacksOrBetter => 1,
        HandRank::Nothing => 0,
    };
    per * bet
}

/// The pure video-poker machine. No I/O, no timing — unit-tested. The deck is
/// shuffled with the same LCG the other games use, so `Game::new(seed)` is
/// deterministic.
#[derive(Clone)]
pub struct Game {
    /// The stock; cards are drawn from the end.
    deck: Vec<Card>,
    /// The five cards on the table.
    hand: [Card; 5],
    /// Which of the five cards the player is holding across the draw.
    held: [bool; 5],
    credits: u32,
    bet: u32,
    phase: Phase,
    /// The rank the drawn hand scored, once the round is over.
    result: Option<HandRank>,
    /// Credits won on the last completed round.
    won: u32,
    rng: u64,
}

impl Game {
    pub fn new(seed: u64) -> Self {
        let mut g = Game {
            deck: Vec::new(),
            hand: [Card { rank: 2, suit: 0 }; 5],
            held: [false; 5],
            credits: START_CREDITS,
            bet: BET,
            phase: Phase::Deal,
            result: None,
            won: 0,
            rng: seed | 1,
        };
        g.build_deck();
        g
    }

    fn rand(&mut self) -> u64 {
        self.rng = self
            .rng
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.rng >> 33
    }

    /// Build a fresh, shuffled 52-card deck with a Fisher–Yates pass driven by
    /// the LCG.
    fn build_deck(&mut self) {
        let mut deck = Vec::with_capacity(52);
        for suit in 0..4u8 {
            for rank in 2..=14u8 {
                deck.push(Card { rank, suit });
            }
        }
        for i in (1..deck.len()).rev() {
            let j = (self.rand() % (i as u64 + 1)) as usize;
            deck.swap(i, j);
        }
        self.deck = deck;
    }

    /// Draw the top card of the deck. The deck is refilled before it can empty,
    /// so the fallback is only a belt-and-braces guard.
    fn draw_card(&mut self) -> Card {
        self.deck.pop().unwrap_or(Card { rank: 14, suit: 0 })
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn credits(&self) -> u32 {
        self.credits
    }

    pub fn bet(&self) -> u32 {
        self.bet
    }

    pub fn hand(&self) -> &[Card; 5] {
        &self.hand
    }

    pub fn held(&self) -> &[bool; 5] {
        &self.held
    }

    pub fn result(&self) -> Option<HandRank> {
        self.result
    }

    pub fn won(&self) -> u32 {
        self.won
    }

    /// Adjust the wager by `delta`, clamped to 1..=MAX_BET. The bet is locked
    /// once a hand is in play.
    pub fn set_bet(&mut self, delta: i32) {
        if self.phase == Phase::Draw {
            return;
        }
        let b = (self.bet as i32 + delta).clamp(1, MAX_BET as i32);
        self.bet = b as u32;
    }

    /// Open a round: take the wager, reshuffle if the deck is low, deal five
    /// fresh cards and hand control to the draw. Does nothing if the player
    /// cannot cover the bet.
    pub fn deal(&mut self) {
        if self.credits < self.bet {
            return;
        }
        if self.deck.len() < RESHUFFLE_AT {
            self.build_deck();
        }
        self.credits -= self.bet;
        for i in 0..5 {
            self.hand[i] = self.draw_card();
        }
        self.held = [false; 5];
        self.result = None;
        self.won = 0;
        self.phase = Phase::Draw;
    }

    /// Toggle the HOLD on card `i` (only while choosing holds).
    pub fn toggle_hold(&mut self, i: usize) {
        if self.phase == Phase::Draw && i < 5 {
            self.held[i] = !self.held[i];
        }
    }

    /// Replace every un-held card, score the final hand and pay out.
    pub fn draw(&mut self) {
        if self.phase != Phase::Draw {
            return;
        }
        for i in 0..5 {
            if !self.held[i] {
                self.hand[i] = self.draw_card();
            }
        }
        let rank = evaluate(&self.hand);
        let won = payout(rank, self.bet);
        self.credits += won;
        self.won = won;
        self.result = Some(rank);
        self.phase = Phase::Over;
    }
}

impl Default for Game {
    fn default() -> Self {
        Self::new(1)
    }
}

/// The suit glyph for a card.
fn suit_glyph(suit: u8) -> &'static str {
    match suit {
        0 => "♠",
        1 => "♥",
        2 => "♦",
        _ => "♣",
    }
}

/// Hearts and diamonds are the red suits.
fn is_red(c: &Card) -> bool {
    c.suit == 1 || c.suit == 2
}

/// A card's short label, e.g. "A♠", "10♥", "K♣".
fn card_label(c: &Card) -> String {
    let rank = match c.rank {
        14 => "A".to_string(),
        13 => "K".to_string(),
        12 => "Q".to_string(),
        11 => "J".to_string(),
        r => r.to_string(),
    };
    format!("{}{}", rank, suit_glyph(c.suit))
}

/// A short name for the winning hand, for the result line.
fn rank_name(rank: HandRank) -> &'static str {
    match rank {
        HandRank::RoyalFlush => "Royal Flush",
        HandRank::StraightFlush => "Straight Flush",
        HandRank::FourOfAKind => "Four of a Kind",
        HandRank::FullHouse => "Full House",
        HandRank::Flush => "Flush",
        HandRank::Straight => "Straight",
        HandRank::ThreeOfAKind => "Three of a Kind",
        HandRank::TwoPair => "Two Pair",
        HandRank::JacksOrBetter => "Jacks or Better",
        HandRank::Nothing => "No win",
    }
}

/// The paytable rows, best first, as `(rank, label, per-credit payout)`.
const PAYTABLE: [(HandRank, &str, u32); 9] = [
    (HandRank::RoyalFlush, "Royal Flush", 250),
    (HandRank::StraightFlush, "Straight Flush", 50),
    (HandRank::FourOfAKind, "Four of a Kind", 25),
    (HandRank::FullHouse, "Full House", 9),
    (HandRank::Flush, "Flush", 6),
    (HandRank::Straight, "Straight", 4),
    (HandRank::ThreeOfAKind, "Three of a Kind", 3),
    (HandRank::TwoPair, "Two Pair", 2),
    (HandRank::JacksOrBetter, "Jacks or Better", 1),
];

/// The interactive Video Poker overlay.
pub struct VideoPoker {
    game: Game,
    seed: u64,
}

impl VideoPoker {
    pub fn new() -> Self {
        VideoPoker {
            game: Game::new(1),
            seed: 1,
        }
    }

    /// Start a fresh machine on a new shuffle, carrying credits and bet forward,
    /// and immediately deal the opening hand.
    fn restart(&mut self) {
        self.seed = self.seed.wrapping_add(1);
        let credits = self.game.credits();
        let bet = self.game.bet();
        self.game = Game::new(self.seed);
        self.game.credits = credits;
        self.game.bet = bet;
        self.game.deal();
    }

    /// The `SPC`/`Enter` action: deal an opening hand, or draw replacements once
    /// holds are chosen, or start the next round after a payout.
    fn advance(&mut self) {
        match self.game.phase() {
            Phase::Deal | Phase::Over => self.game.deal(),
            Phase::Draw => self.game.draw(),
        }
    }
}

impl Default for VideoPoker {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for VideoPoker {
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
            key!(Left) => self.game.set_bet(-1),
            key!(Right) => self.game.set_bet(1),
            key!(' ') | key!(Enter) => self.advance(),
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
        let dim_style = theme.get("ui.linenr");
        let win_row_style = theme.get("warning");
        let red_style = theme.get("error");
        let card_style = theme.get("ui.text");
        let hold_style = theme.get("function");

        surface.clear_with(area, bg);
        if area.width < 44 || area.height < 16 {
            return;
        }
        let ox = area.x + 2;
        let oy = area.y + 2;

        surface.set_string(
            ox,
            area.y,
            &format!("Video Poker  credits {}", self.game.credits()),
            header_style,
        );

        // Paytable down the left; the winning row lights up once a hand pays.
        let win = self.game.result();
        for (i, (rank, label, pay)) in PAYTABLE.iter().enumerate() {
            let style = if Some(*rank) == win {
                win_row_style
            } else {
                dim_style
            };
            surface.set_string(
                ox,
                oy + i as u16,
                &format!("{:<15}{:>3}", label, pay),
                style,
            );
        }

        // Cards and holds to the right of the paytable.
        let cards_x = ox + 20;
        surface.set_string(
            cards_x,
            oy,
            &format!("Bet {}", self.game.bet()),
            header_style,
        );
        let hand_y = oy + 2;
        let showing = self.game.phase() != Phase::Deal;
        for i in 0..5 {
            let x = cards_x + (i as u16) * 4;
            if showing {
                let c = &self.game.hand()[i];
                let st = if is_red(c) { red_style } else { card_style };
                surface.set_string(x, hand_y, &card_label(c), st);
                if self.game.held()[i] {
                    surface.set_string(x, hand_y + 1, "HOLD", hold_style);
                }
            } else {
                surface.set_string(x, hand_y, "░░", dim_style);
            }
        }

        // Result / prompt line under the cards.
        let (msg, msg_style) = match self.game.phase() {
            Phase::Deal => ("Press SPC to deal".to_string(), text_style),
            Phase::Draw => ("Hold cards, then draw".to_string(), text_style),
            Phase::Over => {
                let rank = self.game.result().unwrap_or(HandRank::Nothing);
                if self.game.won() > 0 {
                    (
                        format!("{}!  +{}", rank_name(rank), self.game.won()),
                        win_row_style,
                    )
                } else {
                    ("No win".to_string(), text_style)
                }
            }
        };
        surface.set_string(cards_x, hand_y + 3, &msg, msg_style);

        let sy = oy + PAYTABLE.len() as u16 + 1;
        let hint = if self.game.phase() == Phase::Draw {
            "1-5 hold · SPC draw · n new · q quit"
        } else {
            "SPC deal · ←/→ bet · n new · q quit"
        };
        surface.set_string(ox, sy, hint, text_style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(rank: u8, suit: u8) -> Card {
        Card { rank, suit }
    }

    #[test]
    fn four_of_a_kind_detected() {
        let hand = [card(9, 0), card(9, 1), card(9, 2), card(9, 3), card(2, 0)];
        assert_eq!(evaluate(&hand), HandRank::FourOfAKind);
    }

    #[test]
    fn full_house_detected_and_beats_a_flush() {
        let full = [card(5, 0), card(5, 1), card(5, 2), card(11, 0), card(11, 1)];
        assert_eq!(evaluate(&full), HandRank::FullHouse);
        // A five-card flush (all spades, no straight, no pairs).
        let flush = [card(2, 0), card(5, 0), card(8, 0), card(11, 0), card(13, 0)];
        assert_eq!(evaluate(&flush), HandRank::Flush);
        // The paytable ranks the full house above the flush.
        assert!(payout(HandRank::FullHouse, 1) > payout(HandRank::Flush, 1));
    }

    #[test]
    fn ace_low_straight_is_a_straight() {
        // A-2-3-4-5 across mixed suits (the wheel).
        let hand = [card(14, 0), card(2, 1), card(3, 2), card(4, 3), card(5, 0)];
        assert_eq!(evaluate(&hand), HandRank::Straight);
    }

    #[test]
    fn royal_flush_detected() {
        let hand = [
            card(10, 1),
            card(11, 1),
            card(12, 1),
            card(13, 1),
            card(14, 1),
        ];
        assert_eq!(evaluate(&hand), HandRank::RoyalFlush);
        // The wheel straight flush is not a royal.
        let wheel_sf = [card(14, 2), card(2, 2), card(3, 2), card(4, 2), card(5, 2)];
        assert_eq!(evaluate(&wheel_sf), HandRank::StraightFlush);
    }

    #[test]
    fn jacks_or_better_pays_but_a_low_pair_does_not() {
        let jacks = [card(11, 0), card(11, 1), card(3, 2), card(7, 3), card(9, 0)];
        assert_eq!(evaluate(&jacks), HandRank::JacksOrBetter);
        assert!(payout(HandRank::JacksOrBetter, 5) > 0);
        // A pair of fives is below the paying line.
        let fives = [card(5, 0), card(5, 1), card(3, 2), card(7, 3), card(9, 0)];
        assert_eq!(evaluate(&fives), HandRank::Nothing);
        assert_eq!(payout(HandRank::Nothing, 5), 0);
    }

    #[test]
    fn payout_multiplies_by_the_bet() {
        assert_eq!(payout(HandRank::FourOfAKind, 1), 25);
        assert_eq!(payout(HandRank::FourOfAKind, 3), 75);
        assert_eq!(payout(HandRank::RoyalFlush, 5), 1250);
    }
}
