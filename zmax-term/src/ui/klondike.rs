//! Klondike — a small terminal Klondike solitaire (draw-1) for zmax.
//!
//! Clear the table by building the four foundations up from Ace to King, one
//! per suit. Seven tableau columns are dealt 1..7 cards with only the top of
//! each face-up; the rest of the deck is the face-down stock. Tableau columns
//! build *down* in alternating colours and only a King (or a valid descending
//! alternating-colour run) may move onto an empty column.
//!
//! Like Minesweeper and Blackjack this one is turn-based: nothing animates, so
//! there is no frame loop — the table only changes in response to a key. The
//! rules are pure and unit-tested (the deck is shuffled by a small LCG so a
//! given seed is reproducible).
//!
//! Controls — the cursor sits on one of nine spots: the stock, the waste, then
//! the seven tableau columns.
//!   * `←`/`h`, `→`/`l` move the cursor.
//!   * `SPC`
//!       - on the **stock**: draw a card to the waste (recycling the waste back
//!         into the stock when the stock is empty).
//!       - on the **waste**: pick up the waste's top card (press `SPC` again to
//!         cancel).
//!       - on a **tableau column**: with nothing held, pick up that column's
//!         movable face-up run; with a card/run held, drop it there if the move
//!         is legal (dropping back on the source column cancels).
//!   * `Enter` auto-sends a card to a matching foundation — the held card, or
//!     else the waste top / the current column's top card.
//!   * `n` deals a fresh game, `q`/`Esc` quits.

use tui::buffer::Buffer as Surface;
use zmax_view::graphics::Rect;

use crate::{
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

/// Number of tableau columns.
const COLS: usize = 7;
/// Cursor spot for the stock.
const CURSOR_STOCK: usize = 0;
/// Cursor spot for the waste.
const CURSOR_WASTE: usize = 1;
/// Cursor spot for the first tableau column (columns follow contiguously).
const CURSOR_TABLEAU0: usize = 2;

/// A single playing card. `rank` is 1..=13 where 1=A, 11=J, 12=Q, 13=K; `suit`
/// is 0=♠, 1=♥, 2=♦, 3=♣. Aces are low here (foundations build up from the
/// Ace, tableaux build down to it).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Card {
    pub rank: u8,
    pub suit: u8,
}

/// A card sitting in a tableau column, which may be face up or face down.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Slot {
    card: Card,
    up: bool,
}

/// What the player is currently holding (has picked up with `SPC`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Source {
    /// The top card of the waste.
    Waste,
    /// A run of a tableau column starting at `idx` (inclusive) to its end.
    Tableau { col: usize, idx: usize },
}

/// Hearts and diamonds are the red suits.
fn is_red(c: Card) -> bool {
    c.suit == 1 || c.suit == 2
}

/// Whether `moving` (the bottom card of a run) may be placed onto a tableau
/// column whose current top card is `onto`. An empty column (`None`) accepts a
/// King only; otherwise the move must be one rank lower and the opposite colour.
pub fn can_stack_tableau(moving: Card, onto: Option<Card>) -> bool {
    match onto {
        None => moving.rank == 13,
        Some(top) => moving.rank + 1 == top.rank && is_red(moving) != is_red(top),
    }
}

/// Whether `card` may be placed on a foundation whose current top is `top`. An
/// empty foundation (`None`) accepts an Ace only; otherwise the move must be the
/// same suit and exactly one rank higher.
pub fn can_move_to_foundation(card: Card, top: Option<Card>) -> bool {
    match top {
        None => card.rank == 1,
        Some(t) => card.suit == t.suit && card.rank == t.rank + 1,
    }
}

/// The pure Klondike table. No I/O, no timing — unit-tested. The deck is
/// shuffled with the same LCG the other games use, so `Game::new(seed)` is
/// deterministic.
#[derive(Clone)]
pub struct Game {
    /// Face-down draw pile; cards are drawn from the end.
    stock: Vec<Card>,
    /// Face-up discard pile; the top is the last element.
    waste: Vec<Card>,
    /// One foundation per suit, indexed by `suit`.
    foundations: [Vec<Card>; 4],
    /// The seven tableau columns, bottom card first.
    tableau: [Vec<Slot>; 7],
    /// Cursor spot: `CURSOR_STOCK`, `CURSOR_WASTE`, or a tableau column.
    cursor: usize,
    /// The card/run currently held, if any.
    sel: Option<Source>,
    moves: u32,
    rng: u64,
}

impl Game {
    pub fn new(seed: u64) -> Self {
        let mut g = Game {
            stock: Vec::new(),
            waste: Vec::new(),
            foundations: [Vec::new(), Vec::new(), Vec::new(), Vec::new()],
            tableau: [
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
            ],
            cursor: CURSOR_STOCK,
            sel: None,
            moves: 0,
            rng: seed | 1,
        };
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

    /// Build a fresh, shuffled 52-card deck with a Fisher–Yates pass driven by
    /// the LCG.
    fn shuffled_deck(&mut self) -> Vec<Card> {
        let mut deck = Vec::with_capacity(52);
        for suit in 0..4u8 {
            for rank in 1..=13u8 {
                deck.push(Card { rank, suit });
            }
        }
        for i in (1..deck.len()).rev() {
            let j = (self.rand() % (i as u64 + 1)) as usize;
            deck.swap(i, j);
        }
        deck
    }

    /// The classic deal: column `c` gets `c + 1` cards, only its top face-up;
    /// the remaining 24 cards form the face-down stock.
    fn deal(&mut self) {
        let mut deck = self.shuffled_deck();
        for c in 0..COLS {
            for i in 0..=c {
                let card = deck.pop().unwrap_or(Card { rank: 1, suit: 0 });
                self.tableau[c].push(Slot { card, up: i == c });
            }
        }
        self.stock = deck;
    }

    pub fn moves(&self) -> u32 {
        self.moves
    }

    /// Win when every card has reached a foundation.
    pub fn won(&self) -> bool {
        self.foundations.iter().map(|f| f.len()).sum::<usize>() == 52
    }

    /// Draw one card from the stock to the waste, or — when the stock is empty —
    /// recycle the whole waste back into the stock (face down again).
    fn draw(&mut self) {
        if let Some(c) = self.stock.pop() {
            self.waste.push(c);
        } else {
            while let Some(c) = self.waste.pop() {
                self.stock.push(c);
            }
        }
        self.moves += 1;
    }

    /// After cards leave a tableau column, turn its newly exposed top card up.
    fn flip(&mut self, col: usize) {
        if let Some(top) = self.tableau[col].last_mut() {
            top.up = true;
        }
    }

    /// Pick up the movable face-up run of `col`: the longest properly-sequenced
    /// (descending, alternating-colour) tail of face-up cards.
    fn select_run(&mut self, col: usize) {
        let pile = &self.tableau[col];
        if pile.is_empty() || !pile[pile.len() - 1].up {
            self.sel = None;
            return;
        }
        let mut start = pile.len() - 1;
        while start > 0
            && pile[start - 1].up
            && can_stack_tableau(pile[start].card, Some(pile[start - 1].card))
        {
            start -= 1;
        }
        self.sel = Some(Source::Tableau { col, idx: start });
    }

    /// Move the held card/run onto tableau column `dest`. Returns whether the
    /// move happened; on success the hold is cleared.
    fn move_to_tableau(&mut self, dest: usize) -> bool {
        let src = match self.sel {
            Some(s) => s,
            None => return false,
        };
        let onto = self.tableau[dest].last().map(|s| s.card);
        match src {
            Source::Waste => {
                let card = match self.waste.last() {
                    Some(c) => *c,
                    None => return false,
                };
                if can_stack_tableau(card, onto) {
                    let c = self.waste.pop().unwrap();
                    self.tableau[dest].push(Slot { card: c, up: true });
                    self.moves += 1;
                    self.sel = None;
                    true
                } else {
                    false
                }
            }
            Source::Tableau { col, idx } => {
                if col == dest {
                    return false;
                }
                let moving = self.tableau[col][idx].card;
                if can_stack_tableau(moving, onto) {
                    let run = self.tableau[col].split_off(idx);
                    self.tableau[dest].extend(run);
                    self.flip(col);
                    self.moves += 1;
                    self.sel = None;
                    true
                } else {
                    false
                }
            }
        }
    }

    /// Auto-send a single card to its matching foundation. The candidate is the
    /// held card (a single-card hold), else the waste top / the current column's
    /// top card. Returns whether a card moved.
    fn auto_foundation(&mut self) -> bool {
        #[derive(Clone, Copy)]
        enum Pick {
            Waste,
            Tableau(usize),
        }
        let pick = if let Some(s) = self.sel {
            match s {
                Source::Waste => Some(Pick::Waste),
                Source::Tableau { col, idx } => {
                    if idx == self.tableau[col].len() - 1 {
                        Some(Pick::Tableau(col))
                    } else {
                        None
                    }
                }
            }
        } else {
            match self.cursor {
                CURSOR_WASTE => Some(Pick::Waste),
                c if c >= CURSOR_TABLEAU0 => Some(Pick::Tableau(c - CURSOR_TABLEAU0)),
                _ => None,
            }
        };
        let card = match pick {
            Some(Pick::Waste) => self.waste.last().copied(),
            Some(Pick::Tableau(col)) => self.tableau[col].last().filter(|s| s.up).map(|s| s.card),
            None => None,
        };
        let card = match card {
            Some(c) => c,
            None => return false,
        };
        let f = card.suit as usize;
        let top = self.foundations[f].last().copied();
        if can_move_to_foundation(card, top) {
            match pick.unwrap() {
                Pick::Waste => {
                    self.waste.pop();
                }
                Pick::Tableau(col) => {
                    self.tableau[col].pop();
                    self.flip(col);
                }
            }
            self.foundations[f].push(card);
            self.moves += 1;
            self.sel = None;
            true
        } else {
            false
        }
    }

    /// Move the cursor by `delta`, clamped to the nine spots.
    fn move_cursor(&mut self, delta: i32) {
        let max = (CURSOR_TABLEAU0 + COLS - 1) as i32;
        self.cursor = (self.cursor as i32 + delta).clamp(0, max) as usize;
    }

    /// The interactive `SPC` action, dispatched on the cursor spot.
    fn on_space(&mut self) {
        match self.cursor {
            CURSOR_STOCK => {
                self.draw();
                self.sel = None;
            }
            CURSOR_WASTE => {
                if self.sel.is_some() {
                    self.sel = None;
                } else if !self.waste.is_empty() {
                    self.sel = Some(Source::Waste);
                }
            }
            c => {
                let col = c - CURSOR_TABLEAU0;
                if self.sel.is_some() {
                    if !self.move_to_tableau(col) {
                        if matches!(self.sel, Some(Source::Tableau { col: sc, .. }) if sc == col) {
                            self.sel = None;
                        } else {
                            self.select_run(col);
                        }
                    }
                } else {
                    self.select_run(col);
                }
            }
        }
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

/// A card's short label, e.g. "A♠", "10♥", "K♣".
fn card_label(c: Card) -> String {
    let rank = match c.rank {
        1 => "A".to_string(),
        11 => "J".to_string(),
        12 => "Q".to_string(),
        13 => "K".to_string(),
        r => r.to_string(),
    };
    format!("{}{}", rank, suit_glyph(c.suit))
}

/// The interactive Klondike overlay.
pub struct Klondike {
    game: Game,
    seed: u64,
}

impl Klondike {
    pub fn new() -> Self {
        Klondike {
            game: Game::new(1),
            seed: 1,
        }
    }

    fn restart(&mut self) {
        self.seed = self.seed.wrapping_add(1);
        self.game = Game::new(self.seed);
    }
}

impl Default for Klondike {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for Klondike {
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
            key!(Left) | key!('h') => self.game.move_cursor(-1),
            key!(Right) | key!('l') => self.game.move_cursor(1),
            key!(' ') => self.game.on_space(),
            key!(Enter) => {
                self.game.auto_foundation();
            }
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
        let back_style = theme.get("ui.linenr");
        let sel_style = theme.get("ui.selection");
        let red_style = theme.get("error");
        let win_style = theme.get("function");

        surface.clear_with(area, bg);
        if area.width < 50 || area.height < 18 {
            return;
        }
        let ox = area.x + 2;
        let oy = area.y + 2;
        let g = &self.game;

        // Header.
        let status = if g.won() { "  — you win!" } else { "" };
        surface.set_string(
            ox,
            area.y,
            &format!("Klondike  moves {}{}", g.moves(), status),
            if g.won() { win_style } else { header_style },
        );

        // Stock (face-down draw pile).
        let stock_glyph = if g.stock.is_empty() { "[]" } else { "##" };
        let stock_style = if g.cursor == CURSOR_STOCK {
            sel_style
        } else {
            back_style
        };
        surface.set_string(ox, oy, stock_glyph, stock_style);

        // Waste (top card face-up).
        let (waste_text, mut waste_style) = match g.waste.last() {
            Some(c) => (
                card_label(*c),
                if is_red(*c) { red_style } else { text_style },
            ),
            None => ("--".to_string(), back_style),
        };
        if g.cursor == CURSOR_WASTE || g.sel == Some(Source::Waste) {
            waste_style = sel_style;
        }
        surface.set_string(ox + 4, oy, &waste_text, waste_style);

        // Foundations, one per suit, on the right.
        for j in 0..4usize {
            let x = ox + 20 + (j as u16) * 5;
            let (text, style) = match g.foundations[j].last() {
                Some(c) => (
                    card_label(*c),
                    if is_red(*c) { red_style } else { text_style },
                ),
                None => (suit_glyph(j as u8).to_string(), back_style),
            };
            surface.set_string(x, oy, &text, style);
        }

        // Tableau columns.
        let top = oy + 3;
        let bottom = area.y + area.height - 2;
        for c in 0..COLS {
            let x = ox + (c as u16) * 7;
            if g.cursor == CURSOR_TABLEAU0 + c {
                surface.set_string(x, top - 1, "▼", sel_style);
            }
            let pile = &g.tableau[c];
            if pile.is_empty() {
                let style = if g.cursor == CURSOR_TABLEAU0 + c {
                    sel_style
                } else {
                    back_style
                };
                surface.set_string(x, top, "[]", style);
                continue;
            }
            for (i, slot) in pile.iter().enumerate() {
                let y = top + i as u16;
                if y > bottom {
                    break;
                }
                let (text, mut style) = if slot.up {
                    (
                        card_label(slot.card),
                        if is_red(slot.card) {
                            red_style
                        } else {
                            text_style
                        },
                    )
                } else {
                    ("##".to_string(), back_style)
                };
                if let Some(Source::Tableau { col, idx }) = g.sel {
                    if col == c && i >= idx {
                        style = sel_style;
                    }
                }
                surface.set_string(x, y, &text, style);
            }
        }

        // Footer.
        let sy = area.y + area.height - 1;
        surface.set_string(
            ox,
            sy,
            "←/→ move  SPC draw/pick/drop  ⏎ foundation  n new  q quit",
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

    /// A blank table with empty piles, ready for hand-built setups.
    fn blank() -> Game {
        Game {
            stock: Vec::new(),
            waste: Vec::new(),
            foundations: [Vec::new(), Vec::new(), Vec::new(), Vec::new()],
            tableau: [
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
            ],
            cursor: CURSOR_STOCK,
            sel: None,
            moves: 0,
            rng: 1,
        }
    }

    #[test]
    fn tableau_stacks_down_in_alternating_colours() {
        // A red 6 (♥) drops onto a black 7 (♠).
        assert!(can_stack_tableau(card(6, 1), Some(card(7, 0))));
        // Same colour is rejected: red 6 (♥) onto red 7 (♦).
        assert!(!can_stack_tableau(card(6, 1), Some(card(7, 2))));
        // Not one lower is rejected: an 8 onto a 7.
        assert!(!can_stack_tableau(card(8, 1), Some(card(7, 0))));
    }

    #[test]
    fn only_a_king_moves_to_an_empty_column() {
        assert!(can_stack_tableau(card(13, 0), None));
        assert!(!can_stack_tableau(card(12, 0), None));
        assert!(!can_stack_tableau(card(1, 0), None));
    }

    #[test]
    fn foundations_build_up_by_suit_from_the_ace() {
        // An empty foundation only takes an Ace.
        assert!(can_move_to_foundation(card(1, 0), None));
        assert!(!can_move_to_foundation(card(2, 0), None));
        // Then the same suit ascends by one.
        assert!(can_move_to_foundation(card(2, 0), Some(card(1, 0))));
        // The wrong suit is rejected.
        assert!(!can_move_to_foundation(card(2, 1), Some(card(1, 0))));
        // Skipping a rank is rejected.
        assert!(!can_move_to_foundation(card(3, 0), Some(card(1, 0))));
    }

    #[test]
    fn drawing_moves_to_the_waste_and_recycling_refills_the_stock() {
        let mut g = blank();
        g.stock = vec![card(2, 0), card(3, 1), card(4, 2)]; // 4♦ on top
        g.draw();
        assert_eq!(
            g.waste.last(),
            Some(&card(4, 2)),
            "the drawn card is on the waste"
        );
        assert_eq!(g.stock.len(), 2);

        // With the stock empty, drawing recycles the waste back into the stock.
        let mut g2 = blank();
        g2.waste = vec![card(5, 0), card(6, 1)];
        g2.draw();
        assert_eq!(g2.stock.len(), 2, "the waste refills the stock");
        assert!(g2.waste.is_empty());
    }

    #[test]
    fn the_last_card_to_a_foundation_wins() {
        let mut g = blank();
        // Three suits complete, the fourth one card short of its King.
        for suit in 0..3u8 {
            g.foundations[suit as usize] = (1..=13).map(|r| card(r, suit)).collect();
        }
        g.foundations[3] = (1..=12).map(|r| card(r, 3)).collect();
        assert!(!g.won());
        // The final King waits on the waste; sending it home completes the game.
        g.waste = vec![card(13, 3)];
        g.cursor = CURSOR_WASTE;
        assert!(g.auto_foundation());
        assert!(g.won(), "the last card completes the foundations");
    }

    #[test]
    fn moving_a_run_flips_the_exposed_card() {
        let mut g = blank();
        // Column 0: a face-down 9♣ under a face-up 6♥.
        g.tableau[0] = vec![
            Slot {
                card: card(9, 3),
                up: false,
            },
            Slot {
                card: card(6, 1),
                up: true,
            },
        ];
        // Column 1: a face-up 7♠ to receive the red 6.
        g.tableau[1] = vec![Slot {
            card: card(7, 0),
            up: true,
        }];
        g.sel = Some(Source::Tableau { col: 0, idx: 1 });
        assert!(g.move_to_tableau(1));
        assert_eq!(g.tableau[1].len(), 2, "the 6 moved onto the 7");
        assert_eq!(g.tableau[0].len(), 1);
        assert!(g.tableau[0][0].up, "the exposed face-down card flips up");
    }
}
