//! Bidirectional text: paragraph base direction and logical→visual reordering.
//!
//! Emacs stores bidirectional text in *logical* (reading) order and reorders it
//! into *visual* order at display time, following the Unicode Bidirectional
//! Algorithm (UAX #9). `left-char` / `right-char` are the commands that care:
//! in a right-to-left paragraph "left" means *forward* through the buffer, and
//! with `visual-order-cursor-movement` on they step to the character that is
//! physically to the left/right of the cursor, which can be far away in buffer
//! positions (Emacs manual, "Bidirectional Editing").
//!
//! Scope: the implicit part of the UBA — the P2/P3 base-direction rule, the
//! implicit level rules (L1–L2, N1–N2, W-series for numbers) and the L2 run
//! reversal. The explicit embedding controls (LRE/RLE/LRO/RLO/PDF) and the
//! isolate controls (LRI/RLI/FSI/PDI) are *not* resolved: they are treated as
//! neutrals, so text that steers direction with them reorders as if they were
//! not there. Everything that steers direction with the characters themselves —
//! Hebrew/Arabic runs, Latin runs inside them, and numbers in either — reorders
//! exactly as the UBA prescribes.

/// The two paragraph/base directions of UAX #9.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    LeftToRight,
    RightToLeft,
}

impl Direction {
    /// The embedding level a paragraph with this base direction starts at.
    fn base_level(self) -> u8 {
        match self {
            Direction::LeftToRight => 0,
            Direction::RightToLeft => 1,
        }
    }
}

/// The bidirectional character types this module distinguishes. The UBA has
/// more; the ones collapsed into `Neutral` are all resolved the same way here
/// (rules N1/N2), and `Removed` covers the marks that never occupy a position of
/// their own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    /// L — strong left-to-right.
    Left,
    /// R and AL — strong right-to-left.
    Right,
    /// EN — European digits.
    EuropeanNumber,
    /// AN — Arabic-Indic digits.
    ArabicNumber,
    /// B, S, WS, ON and the unresolved explicit/isolate controls.
    Neutral,
    /// NSM — a combining mark, which inherits the class of what it follows.
    Mark,
}

/// The code-point ranges whose characters have a strong right-to-left type (R or
/// AL). This is the same set Emacs gives the `R` character category in
/// `characters.el` ("Characters with strong right-to-left directionality").
const RTL_RANGES: &[(u32, u32)] = &[
    (0x0590, 0x05FF), // Hebrew
    (0x0600, 0x0605), // Arabic number signs (AN in the UBA, R for base direction)
    (0x0608, 0x0608),
    (0x060B, 0x060B),
    (0x060D, 0x060D),
    (0x061B, 0x064A), // Arabic letters
    (0x066D, 0x066F),
    (0x0671, 0x06D5),
    (0x06E5, 0x06E6),
    (0x06EE, 0x06EF),
    (0x06FA, 0x070D), // Arabic + Syriac
    (0x0710, 0x0710),
    (0x0712, 0x072F),
    (0x074D, 0x07A5), // Syriac, Thaana
    (0x07B1, 0x07BF),
    (0x07C0, 0x08FF),   // NKo, Samaritan, Mandaic, Arabic Extended-A
    (0xFB1D, 0xFB4F),   // Hebrew presentation forms
    (0xFB50, 0xFDFF),   // Arabic presentation forms A
    (0xFE70, 0xFEFC),   // Arabic presentation forms B
    (0x10800, 0x10FFF), // Cypriot … Old Hungarian and friends
    (0x1E800, 0x1EFFF), // Mende Kikakui, Adlam, Arabic Mathematical
];

/// The Arabic-Indic digit ranges (AN).
const ARABIC_NUMBER_RANGES: &[(u32, u32)] = &[
    (0x0600, 0x0605),
    (0x0660, 0x0669),
    (0x066B, 0x066C),
    (0x06DD, 0x06DD),
    (0x0890, 0x0891),
    (0x08E2, 0x08E2),
    (0x10E60, 0x10E7E),
];

fn in_ranges(c: char, ranges: &[(u32, u32)]) -> bool {
    let code = c as u32;
    ranges.iter().any(|&(lo, hi)| (lo..=hi).contains(&code))
}

/// True when `c` has strong right-to-left directionality (UBA class R or AL) —
/// Emacs's `R` character category.
pub fn is_strong_rtl(c: char) -> bool {
    in_ranges(c, RTL_RANGES) && !in_ranges(c, ARABIC_NUMBER_RANGES)
}

/// True when `c` has strong left-to-right directionality (UBA class L) — Emacs's
/// `L` character category. Letters that are not right-to-left are L; digits are
/// numbers, not strong types, so they are excluded.
pub fn is_strong_ltr(c: char) -> bool {
    c.is_alphabetic() && !is_strong_rtl(c)
}

fn class_of(c: char) -> Class {
    if in_ranges(c, ARABIC_NUMBER_RANGES) {
        return Class::ArabicNumber;
    }
    if is_strong_rtl(c) {
        return Class::Right;
    }
    if c.is_numeric() {
        return Class::EuropeanNumber;
    }
    if c.is_alphabetic() {
        return Class::Left;
    }
    // NSM: a combining mark takes the class of the character it is applied to.
    use unicode_general_category::{get_general_category, GeneralCategory};
    if get_general_category(c) == GeneralCategory::NonspacingMark {
        return Class::Mark;
    }
    Class::Neutral
}

/// UAX #9 rules P2/P3: the base direction of a paragraph is that of its first
/// strong character; a paragraph with no strong character is left-to-right.
pub fn paragraph_direction(text: impl Iterator<Item = char>) -> Direction {
    for c in text {
        match class_of(c) {
            Class::Left => return Direction::LeftToRight,
            Class::Right => return Direction::RightToLeft,
            _ => {}
        }
    }
    Direction::LeftToRight
}

/// The embedding level of every character of `line`, under `base`.
///
/// Numbers take an even level above the run they sit in (UBA W/I rules), which
/// is what makes `123` read left-to-right inside Hebrew text; neutrals take the
/// surrounding level when both sides agree and the base level when they do not
/// (rules N1/N2); a combining mark keeps the level of the character before it.
fn levels(line: &[char], base: Direction) -> Vec<u8> {
    let base_level = base.base_level();
    let mut classes: Vec<Class> = line.iter().copied().map(class_of).collect();
    // NSM (rule W1): a mark takes the class of the previous character, or the
    // base direction's class at the start of the line.
    for i in 0..classes.len() {
        if classes[i] == Class::Mark {
            classes[i] = if i == 0 {
                match base {
                    Direction::LeftToRight => Class::Left,
                    Direction::RightToLeft => Class::Right,
                }
            } else {
                classes[i - 1]
            };
        }
    }
    // Rule W2: a European number in an Arabic-letter context is an Arabic number.
    let mut last_strong = match base {
        Direction::LeftToRight => Class::Left,
        Direction::RightToLeft => Class::Right,
    };
    for cls in classes.iter_mut() {
        match *cls {
            Class::Left | Class::Right => last_strong = *cls,
            Class::EuropeanNumber if last_strong == Class::Right => *cls = Class::ArabicNumber,
            _ => {}
        }
    }

    // Strong types and numbers get their level directly (rules I1/I2).
    let mut levels: Vec<Option<u8>> = classes
        .iter()
        .map(|cls| match cls {
            Class::Left => Some(if base_level % 2 == 0 {
                base_level
            } else {
                base_level + 1
            }),
            Class::Right => Some(if base_level % 2 == 1 {
                base_level
            } else {
                base_level + 1
            }),
            // A number is always laid out left-to-right, one level above the
            // right-to-left run it may sit in.
            Class::EuropeanNumber | Class::ArabicNumber => Some(if base_level % 2 == 0 {
                base_level
            } else {
                base_level + 1
            }),
            _ => None,
        })
        .collect();
    // A number inside a right-to-left run: give it the next even level so it
    // reads left-to-right while the run around it reads right-to-left.
    for (i, cls) in classes.iter().enumerate() {
        if matches!(cls, Class::EuropeanNumber | Class::ArabicNumber) {
            let prev_strong = classes[..i]
                .iter()
                .rev()
                .find(|c| matches!(c, Class::Left | Class::Right));
            let rtl_context = match prev_strong {
                Some(Class::Right) => true,
                Some(_) => false,
                None => base == Direction::RightToLeft,
            };
            if rtl_context {
                let odd = if base_level % 2 == 1 {
                    base_level
                } else {
                    base_level + 1
                };
                levels[i] = Some(odd + 1);
            }
        }
    }

    // Rules N1/N2: a run of neutrals between two equal levels takes that level,
    // otherwise the base level.
    let mut out = vec![base_level; line.len()];
    let mut i = 0;
    while i < levels.len() {
        match levels[i] {
            Some(level) => {
                out[i] = level;
                i += 1;
            }
            None => {
                let start = i;
                while i < levels.len() && levels[i].is_none() {
                    i += 1;
                }
                let before = start.checked_sub(1).and_then(|j| levels[j]);
                let after = levels.get(i).copied().flatten();
                let level = match (before, after) {
                    (Some(a), Some(b)) if a == b => a,
                    _ => base_level,
                };
                for slot in out.iter_mut().take(i).skip(start) {
                    *slot = level;
                }
            }
        }
    }
    out
}

/// Reorder one line from logical into visual order (UAX #9 rule L2) and return
/// the logical index of each visual position: `visual_order(line, base)[0]` is
/// the character drawn leftmost.
pub fn visual_order(line: &[char], base: Direction) -> Vec<usize> {
    let levels = levels(line, base);
    let mut order: Vec<usize> = (0..line.len()).collect();
    let Some(&highest) = levels.iter().max() else {
        return order;
    };
    let lowest_odd = levels
        .iter()
        .copied()
        .filter(|l| l % 2 == 1)
        .min()
        .unwrap_or(highest + 1);
    // "From the highest level found in the text to the lowest odd level, reverse
    // any contiguous sequence of characters that are at that level or higher."
    let mut level = highest;
    while level >= lowest_odd {
        let mut i = 0;
        while i < order.len() {
            if levels[order[i]] >= level {
                let start = i;
                while i < order.len() && levels[order[i]] >= level {
                    i += 1;
                }
                order[start..i].reverse();
            } else {
                i += 1;
            }
        }
        if level == 0 {
            break;
        }
        level -= 1;
    }
    order
}

/// The logical index of the character one screen position to the left (or right)
/// of the character at logical index `from` on `line`. `None` when there is no
/// such character on this line, i.e. the cursor is at the visual edge and the
/// caller has to move to the neighbouring screen line.
///
/// This is what `left-char` / `right-char` do when
/// `visual-order-cursor-movement` is non-nil: they move to the character that is
/// physically to the left or right, which in reordered text can be many buffer
/// positions away.
pub fn visual_neighbor(line: &[char], from: usize, base: Direction, right: bool) -> Option<usize> {
    if from >= line.len() {
        // Past the last character: the caller is at the end of the line; step
        // in from whichever visual edge that is.
        let order = visual_order(line, base);
        return match (right, base) {
            (false, Direction::LeftToRight) => order.last().copied(),
            (true, Direction::RightToLeft) => order.first().copied(),
            _ => None,
        };
    }
    let order = visual_order(line, base);
    let visual = order.iter().position(|&idx| idx == from)?;
    let next = if right {
        visual.checked_add(1).filter(|v| *v < order.len())
    } else {
        visual.checked_sub(1)
    }?;
    Some(order[next])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chars(s: &str) -> Vec<char> {
        s.chars().collect()
    }

    #[test]
    fn base_direction_follows_the_first_strong_character() {
        assert_eq!(paragraph_direction("hello".chars()), Direction::LeftToRight);
        // A Hebrew word: the paragraph is right-to-left.
        assert_eq!(paragraph_direction("שלום".chars()), Direction::RightToLeft);
        // Leading digits and punctuation are not strong, so the Hebrew still wins.
        assert_eq!(
            paragraph_direction("123 — שלום".chars()),
            Direction::RightToLeft
        );
        // No strong character at all falls back to left-to-right (rule P3).
        assert_eq!(paragraph_direction("123!".chars()), Direction::LeftToRight);
    }

    #[test]
    fn rtl_run_reverses_inside_ltr_text() {
        // "abc שלום" — the Hebrew word is drawn right-to-left, so its last
        // logical character (ם, index 7) is the leftmost of the run.
        let line = chars("abc שלום");
        let order = visual_order(&line, Direction::LeftToRight);
        assert_eq!(&order[..4], &[0, 1, 2, 3]);
        assert_eq!(&order[4..], &[7, 6, 5, 4]);
    }

    #[test]
    fn ltr_run_and_numbers_stay_readable_inside_rtl_text() {
        // A Hebrew line with an embedded number: the digits keep their
        // left-to-right order even though the line reads right-to-left.
        let line = chars("שלום 12");
        let order = visual_order(&line, Direction::RightToLeft);
        // Visually: "12" sits at the left end, in its logical order.
        assert_eq!(&order[..2], &[5, 6]);
        // …and the Hebrew is reversed after it.
        assert_eq!(order.last(), Some(&0));
    }

    #[test]
    fn visual_neighbor_crosses_a_reversed_run() {
        // In "abc שלום", moving right from the space (index 3) lands on the
        // *last* character of the Hebrew word, because that is what is drawn
        // immediately to the right of the space.
        let line = chars("abc שלום");
        assert_eq!(
            visual_neighbor(&line, 3, Direction::LeftToRight, true),
            Some(7)
        );
        // Moving left from the first Hebrew character (index 4, drawn rightmost)
        // has nothing further right… but leftward from it is index 5.
        assert_eq!(
            visual_neighbor(&line, 4, Direction::LeftToRight, false),
            Some(5)
        );
        // The rightmost character of the line has no right neighbour.
        assert_eq!(
            visual_neighbor(&line, 4, Direction::LeftToRight, true),
            None
        );
    }

    #[test]
    fn plain_ascii_is_untouched() {
        let line = chars("hello, world");
        let order = visual_order(&line, Direction::LeftToRight);
        assert_eq!(order, (0..line.len()).collect::<Vec<_>>());
        assert_eq!(
            visual_neighbor(&line, 0, Direction::LeftToRight, true),
            Some(1)
        );
        assert_eq!(
            visual_neighbor(&line, 0, Direction::LeftToRight, false),
            None
        );
    }
}
