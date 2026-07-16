//! Blackjack — a small terminal blackjack for zmax.
//!
//! Beat the dealer without going over 21. `h` hits (draws a card), `s` stands
//! (the dealer then reveals and plays out), `n` deals a fresh hand and `q`/`Esc`
//! quits. Like Minesweeper this one is turn-based: nothing animates, so there is
//! no frame loop — the table only changes in response to a key. The card logic is
//! pure and unit-tested (the shoe is shuffled by a small LCG so a given seed is
//! reproducible).

use tui::buffer::Buffer as Surface;
use zmax_view::graphics::Rect;

use crate::{
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

/// Chips the player starts with.
const START_BALANCE: i32 = 100;
/// Chips wagered on every hand.
const BET: i32 = 10;
/// The dealer keeps hitting until the hand reaches at least this total.
const DEALER_STANDS: u8 = 17;
/// Reshuffle a fresh 52-card shoe once the shoe drops below this many cards.
const RESHUFFLE_AT: usize = 15;

/// A single playing card. `rank` is 2..=14 where 11=J, 12=Q, 13=K and 14=A;
/// `suit` is 0=♠, 1=♥, 2=♦, 3=♣.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Card {
    pub rank: u8,
    pub suit: u8,
}

/// Where a hand is: the player is still deciding, or it has been settled.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    Player,
    Over,
}

/// How a settled hand turned out.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outcome {
    /// The player won even money.
    PlayerWin,
    /// The player won with a two-card 21 (pays 3:2).
    PlayerBlackjack,
    /// The dealer won (also covers a player bust).
    DealerWin,
    /// A tie — the bet is returned.
    Push,
}

/// The blackjack value of a single card: pips at face value, J/Q/K as 10 and an
/// ace as 11 (downgraded to 1 later by [`hand_value`] when it would bust).
fn card_value(rank: u8) -> u8 {
    match rank {
        14 => 11,
        11..=13 => 10,
        r => r,
    }
}

/// The best total for a hand: sum the cards counting every ace as 11, then knock
/// each ace down to 1 while the total is over 21. So A+9+A is 21, A+K is 21 and
/// A+K+5 is 16.
pub fn hand_value(cards: &[Card]) -> u8 {
    let mut total: u32 = 0;
    let mut aces = 0u32;
    for c in cards {
        total += card_value(c.rank) as u32;
        if c.rank == 14 {
            aces += 1;
        }
    }
    while total > 21 && aces > 0 {
        total -= 10;
        aces -= 1;
    }
    total as u8
}

/// A natural: exactly two cards totalling 21.
pub fn is_blackjack(cards: &[Card]) -> bool {
    cards.len() == 2 && hand_value(cards) == 21
}

/// The pure blackjack table. No I/O, no timing — unit-tested. The shoe is
/// shuffled with the same LCG the other games use, so `Game::new(seed)` is
/// deterministic.
#[derive(Clone)]
pub struct Game {
    /// The shoe; cards are drawn from the end.
    deck: Vec<Card>,
    player: Vec<Card>,
    dealer: Vec<Card>,
    balance: i32,
    phase: Phase,
    outcome: Option<Outcome>,
    rng: u64,
}

impl Game {
    pub fn new(seed: u64) -> Self {
        let mut g = Game {
            deck: Vec::new(),
            player: Vec::new(),
            dealer: Vec::new(),
            balance: START_BALANCE,
            phase: Phase::Player,
            outcome: None,
            rng: seed | 1,
        };
        g.build_deck();
        g.deal();
        g
    }

    fn rand(&mut self) -> u64 {
        self.rng = self
            .rng
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.rng >> 33
    }

    /// Build a fresh, shuffled 52-card shoe with a Fisher–Yates pass driven by
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

    /// Draw the top card of the shoe. The shoe is refilled before it can empty,
    /// so the fallback is only a belt-and-braces guard.
    fn draw(&mut self) -> Card {
        self.deck.pop().unwrap_or(Card { rank: 14, suit: 0 })
    }

    /// Deal a new hand: two cards each, reshuffling first if the shoe is low. A
    /// natural on either side settles the hand immediately.
    fn deal(&mut self) {
        if self.deck.len() < RESHUFFLE_AT {
            self.build_deck();
        }
        self.player.clear();
        self.dealer.clear();
        self.outcome = None;
        self.phase = Phase::Player;
        for _ in 0..2 {
            let p = self.draw();
            self.player.push(p);
            let d = self.draw();
            self.dealer.push(d);
        }
        if is_blackjack(&self.player) || is_blackjack(&self.dealer) {
            self.settle();
        }
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn outcome(&self) -> Option<Outcome> {
        self.outcome
    }

    pub fn balance(&self) -> i32 {
        self.balance
    }

    /// Take another card. Busting (over 21) settles the hand as a loss.
    pub fn hit(&mut self) {
        if self.phase != Phase::Player {
            return;
        }
        let c = self.draw();
        self.player.push(c);
        if hand_value(&self.player) > 21 {
            self.settle();
        }
    }

    /// Stop drawing: the dealer reveals the hole card and hits until it reaches
    /// `DEALER_STANDS` (standing on all 17s), then the hand is settled.
    pub fn stand(&mut self) {
        if self.phase != Phase::Player {
            return;
        }
        while hand_value(&self.dealer) < DEALER_STANDS {
            let c = self.draw();
            self.dealer.push(c);
        }
        self.settle();
    }

    /// Compare the final hands, pay the balance and record the outcome.
    fn settle(&mut self) {
        let pv = hand_value(&self.player);
        let dv = hand_value(&self.dealer);
        let pbj = is_blackjack(&self.player);
        let dbj = is_blackjack(&self.dealer);
        let outcome = if pv > 21 {
            Outcome::DealerWin
        } else if dv > 21 {
            Outcome::PlayerWin
        } else if pbj && !dbj {
            Outcome::PlayerBlackjack
        } else if dbj && !pbj {
            Outcome::DealerWin
        } else if pv > dv {
            Outcome::PlayerWin
        } else if dv > pv {
            Outcome::DealerWin
        } else {
            Outcome::Push
        };
        match outcome {
            Outcome::PlayerWin => self.balance += BET,
            Outcome::PlayerBlackjack => self.balance += BET * 3 / 2,
            Outcome::DealerWin => self.balance -= BET,
            Outcome::Push => {}
        }
        self.outcome = Some(outcome);
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

/// The interactive Blackjack overlay.
pub struct Blackjack {
    game: Game,
    seed: u64,
}

impl Blackjack {
    pub fn new() -> Self {
        Blackjack {
            game: Game::new(1),
            seed: 1,
        }
    }

    /// Deal a fresh hand on a newly shuffled shoe, carrying the chip balance
    /// forward (including any settlement from an immediate natural).
    fn deal(&mut self) {
        self.seed = self.seed.wrapping_add(1);
        let balance = self.game.balance();
        let g = Game::new(self.seed);
        let delta = g.balance() - START_BALANCE;
        self.game = g;
        self.game.balance = balance + delta;
    }
}

impl Default for Blackjack {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for Blackjack {
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
            key!('h') => self.game.hit(),
            key!('s') => self.game.stand(),
            key!('n') => self.deal(),
            _ => {}
        }
        zmax_event::request_redraw();
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        let theme = &ctx.editor.theme;
        let bg = theme.get("ui.background");
        let text_style = theme.get("ui.text");
        let card_style = theme.get("ui.text");
        let red_style = theme.get("error");
        let header_style = theme.get("ui.text.focus");
        let label_style = theme.get("ui.text.focus");
        let hidden_style = theme.get("ui.linenr");
        let win_style = theme.get("function");
        let lose_style = theme.get("error");
        let push_style = theme.get("warning");

        surface.clear_with(area, bg);
        if area.width < 40 || area.height < 14 {
            return;
        }
        let ox = area.x + 2;
        let oy = area.y + 2;

        surface.set_string(
            ox,
            area.y,
            &format!("Blackjack  chips {}", self.game.balance()),
            header_style,
        );

        // While the player is still deciding, the dealer's hole card stays down.
        let hide = self.game.phase() == Phase::Player;

        // Dealer.
        let dealer_total = if hide {
            card_value(self.game.dealer[0].rank)
        } else {
            hand_value(&self.game.dealer)
        };
        surface.set_string(ox, oy, &format!("Dealer: {}", dealer_total), label_style);
        let mut dx = ox;
        for (i, c) in self.game.dealer.iter().enumerate() {
            if hide && i >= 1 {
                surface.set_string(dx, oy + 1, "??", hidden_style);
            } else {
                let st = if is_red(c) { red_style } else { card_style };
                surface.set_string(dx, oy + 1, &card_label(c), st);
            }
            dx += 4;
        }

        // Player.
        surface.set_string(
            ox,
            oy + 3,
            &format!("You:    {}", hand_value(&self.game.player)),
            label_style,
        );
        let mut px = ox;
        for c in self.game.player.iter() {
            let st = if is_red(c) { red_style } else { card_style };
            surface.set_string(px, oy + 4, &card_label(c), st);
            px += 4;
        }

        // Result line.
        let (result, result_style) = match self.game.outcome() {
            None => (format!("Your move — bet {}", BET), text_style),
            Some(Outcome::PlayerBlackjack) => {
                (format!("Blackjack! You win {}", BET * 3 / 2), win_style)
            }
            Some(Outcome::PlayerWin) => (format!("You win {}", BET), win_style),
            Some(Outcome::DealerWin) => {
                if hand_value(&self.game.player) > 21 {
                    (format!("Bust! You lose {}", BET), lose_style)
                } else {
                    (format!("Dealer wins — you lose {}", BET), lose_style)
                }
            }
            Some(Outcome::Push) => ("Push — bet returned".to_string(), push_style),
        };
        surface.set_string(ox, oy + 6, &result, result_style);

        surface.set_string(
            ox,
            oy + 8,
            "h hit · s stand · n new hand · q quit",
            text_style,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(rank: u8, suit: u8) -> Card {
        Card { rank, suit }
    }

    /// A table with hands and a shoe set by hand (cards are drawn from the end).
    fn table(player: Vec<Card>, dealer: Vec<Card>, deck: Vec<Card>) -> Game {
        Game {
            deck,
            player,
            dealer,
            balance: 100,
            phase: Phase::Player,
            outcome: None,
            rng: 1,
        }
    }

    #[test]
    fn ace_counts_as_eleven_or_one() {
        // A+9+A: 11 + 9 + 1 = 21 (one ace downgraded).
        assert_eq!(hand_value(&[card(14, 0), card(9, 1), card(14, 2)]), 21);
        // A+K: the ace stays soft at 11.
        assert_eq!(hand_value(&[card(14, 0), card(13, 1)]), 21);
        // A+K+5: the ace must drop to 1 to avoid busting → 16.
        assert_eq!(hand_value(&[card(14, 0), card(13, 1), card(5, 2)]), 16);
    }

    #[test]
    fn ace_king_is_a_blackjack() {
        assert!(is_blackjack(&[card(14, 0), card(13, 1)]));
        // 21 across three cards is not a natural.
        assert!(!is_blackjack(&[card(14, 0), card(5, 1), card(5, 2)]));
    }

    #[test]
    fn hitting_past_21_busts_and_loses() {
        // Player has K+9 = 19; the next card off the shoe is a 5 → 24.
        let mut g = table(
            vec![card(13, 0), card(9, 1)],
            vec![card(10, 2), card(7, 3)],
            vec![card(5, 0)],
        );
        g.hit();
        assert!(hand_value(&g.player) > 21, "the extra card busts the hand");
        assert_eq!(g.phase, Phase::Over);
        assert_eq!(g.outcome, Some(Outcome::DealerWin));
        assert_eq!(g.balance, 90, "the bet is lost on a bust");
    }

    #[test]
    fn dealer_draws_until_seventeen() {
        // Dealer starts on 12 and must draw: 12 + 3 = 15, then 15 + 2 = 17.
        let mut g = table(
            vec![card(10, 0), card(10, 1)],
            vec![card(10, 2), card(2, 3)],
            vec![card(2, 0), card(3, 0)],
        );
        g.stand();
        assert!(hand_value(&g.dealer) >= 17, "dealer keeps hitting below 17");
    }

    #[test]
    fn settle_pays_a_win() {
        // Player 20 beats the dealer's 19.
        let mut g = table(
            vec![card(13, 0), card(10, 1)],
            vec![card(13, 2), card(9, 3)],
            vec![],
        );
        g.settle();
        assert_eq!(g.outcome, Some(Outcome::PlayerWin));
        assert_eq!(g.balance, 110, "a win pays even money");
    }

    #[test]
    fn settle_charges_a_loss() {
        // Player 16 loses to the dealer's 19.
        let mut g = table(
            vec![card(10, 0), card(6, 1)],
            vec![card(10, 2), card(9, 3)],
            vec![],
        );
        g.settle();
        assert_eq!(g.outcome, Some(Outcome::DealerWin));
        assert_eq!(g.balance, 90, "a loss forfeits the bet");
    }
}
