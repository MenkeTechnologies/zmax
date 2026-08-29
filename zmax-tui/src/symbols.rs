//! Common TUI symbols including blocks, bars, braille, lines

pub mod block {
    pub const FULL: &str = "█";
    pub const SEVEN_EIGHTHS: &str = "▉";
    pub const THREE_QUARTERS: &str = "▊";
    pub const FIVE_EIGHTHS: &str = "▋";
    pub const HALF: &str = "▌";
    pub const THREE_EIGHTHS: &str = "▍";
    pub const ONE_QUARTER: &str = "▎";
    pub const ONE_EIGHTH: &str = "▏";

    #[derive(Debug, Clone)]
    pub struct Set {
        pub full: &'static str,
        pub seven_eighths: &'static str,
        pub three_quarters: &'static str,
        pub five_eighths: &'static str,
        pub half: &'static str,
        pub three_eighths: &'static str,
        pub one_quarter: &'static str,
        pub one_eighth: &'static str,
        pub empty: &'static str,
    }

    pub const THREE_LEVELS: Set = Set {
        full: FULL,
        seven_eighths: FULL,
        three_quarters: HALF,
        five_eighths: HALF,
        half: HALF,
        three_eighths: HALF,
        one_quarter: HALF,
        one_eighth: " ",
        empty: " ",
    };

    pub const NINE_LEVELS: Set = Set {
        full: FULL,
        seven_eighths: SEVEN_EIGHTHS,
        three_quarters: THREE_QUARTERS,
        five_eighths: FIVE_EIGHTHS,
        half: HALF,
        three_eighths: THREE_EIGHTHS,
        one_quarter: ONE_QUARTER,
        one_eighth: ONE_EIGHTH,
        empty: " ",
    };
}

pub mod bar {
    pub const FULL: &str = "█";
    pub const SEVEN_EIGHTHS: &str = "▇";
    pub const THREE_QUARTERS: &str = "▆";
    pub const FIVE_EIGHTHS: &str = "▅";
    pub const HALF: &str = "▄";
    pub const THREE_EIGHTHS: &str = "▃";
    pub const ONE_QUARTER: &str = "▂";
    pub const ONE_EIGHTH: &str = "▁";

    #[derive(Debug, Clone)]
    pub struct Set {
        pub full: &'static str,
        pub seven_eighths: &'static str,
        pub three_quarters: &'static str,
        pub five_eighths: &'static str,
        pub half: &'static str,
        pub three_eighths: &'static str,
        pub one_quarter: &'static str,
        pub one_eighth: &'static str,
        pub empty: &'static str,
    }

    pub const THREE_LEVELS: Set = Set {
        full: FULL,
        seven_eighths: FULL,
        three_quarters: HALF,
        five_eighths: HALF,
        half: HALF,
        three_eighths: HALF,
        one_quarter: HALF,
        one_eighth: " ",
        empty: " ",
    };

    pub const NINE_LEVELS: Set = Set {
        full: FULL,
        seven_eighths: SEVEN_EIGHTHS,
        three_quarters: THREE_QUARTERS,
        five_eighths: FIVE_EIGHTHS,
        half: HALF,
        three_eighths: THREE_EIGHTHS,
        one_quarter: ONE_QUARTER,
        one_eighth: ONE_EIGHTH,
        empty: " ",
    };
}

pub mod line {
    pub const VERTICAL: &str = "│";
    pub const DOUBLE_VERTICAL: &str = "║";
    pub const THICK_VERTICAL: &str = "┃";

    pub const HORIZONTAL: &str = "─";
    pub const DOUBLE_HORIZONTAL: &str = "═";
    pub const THICK_HORIZONTAL: &str = "━";

    pub const TOP_RIGHT: &str = "┐";
    pub const ROUNDED_TOP_RIGHT: &str = "╮";
    pub const DOUBLE_TOP_RIGHT: &str = "╗";
    pub const THICK_TOP_RIGHT: &str = "┓";

    pub const TOP_LEFT: &str = "┌";
    pub const ROUNDED_TOP_LEFT: &str = "╭";
    pub const DOUBLE_TOP_LEFT: &str = "╔";
    pub const THICK_TOP_LEFT: &str = "┏";

    pub const BOTTOM_RIGHT: &str = "┘";
    pub const ROUNDED_BOTTOM_RIGHT: &str = "╯";
    pub const DOUBLE_BOTTOM_RIGHT: &str = "╝";
    pub const THICK_BOTTOM_RIGHT: &str = "┛";

    pub const BOTTOM_LEFT: &str = "└";
    pub const ROUNDED_BOTTOM_LEFT: &str = "╰";
    pub const DOUBLE_BOTTOM_LEFT: &str = "╚";
    pub const THICK_BOTTOM_LEFT: &str = "┗";

    pub const VERTICAL_LEFT: &str = "┤";
    pub const DOUBLE_VERTICAL_LEFT: &str = "╣";
    pub const THICK_VERTICAL_LEFT: &str = "┫";

    pub const VERTICAL_RIGHT: &str = "├";
    pub const DOUBLE_VERTICAL_RIGHT: &str = "╠";
    pub const THICK_VERTICAL_RIGHT: &str = "┣";

    pub const HORIZONTAL_DOWN: &str = "┬";
    pub const DOUBLE_HORIZONTAL_DOWN: &str = "╦";
    pub const THICK_HORIZONTAL_DOWN: &str = "┳";

    pub const HORIZONTAL_UP: &str = "┴";
    pub const DOUBLE_HORIZONTAL_UP: &str = "╩";
    pub const THICK_HORIZONTAL_UP: &str = "┻";

    pub const CROSS: &str = "┼";
    pub const DOUBLE_CROSS: &str = "╬";
    pub const THICK_CROSS: &str = "╋";

    #[derive(Debug, Clone)]
    pub struct Set {
        pub vertical: &'static str,
        pub horizontal: &'static str,
        pub top_right: &'static str,
        pub top_left: &'static str,
        pub bottom_right: &'static str,
        pub bottom_left: &'static str,
        pub vertical_left: &'static str,
        pub vertical_right: &'static str,
        pub horizontal_down: &'static str,
        pub horizontal_up: &'static str,
        pub cross: &'static str,
    }

    pub const NORMAL: Set = Set {
        vertical: VERTICAL,
        horizontal: HORIZONTAL,
        top_right: TOP_RIGHT,
        top_left: TOP_LEFT,
        bottom_right: BOTTOM_RIGHT,
        bottom_left: BOTTOM_LEFT,
        vertical_left: VERTICAL_LEFT,
        vertical_right: VERTICAL_RIGHT,
        horizontal_down: HORIZONTAL_DOWN,
        horizontal_up: HORIZONTAL_UP,
        cross: CROSS,
    };

    pub const ROUNDED: Set = Set {
        top_right: ROUNDED_TOP_RIGHT,
        top_left: ROUNDED_TOP_LEFT,
        bottom_right: ROUNDED_BOTTOM_RIGHT,
        bottom_left: ROUNDED_BOTTOM_LEFT,
        ..NORMAL
    };

    pub const DOUBLE: Set = Set {
        vertical: DOUBLE_VERTICAL,
        horizontal: DOUBLE_HORIZONTAL,
        top_right: DOUBLE_TOP_RIGHT,
        top_left: DOUBLE_TOP_LEFT,
        bottom_right: DOUBLE_BOTTOM_RIGHT,
        bottom_left: DOUBLE_BOTTOM_LEFT,
        vertical_left: DOUBLE_VERTICAL_LEFT,
        vertical_right: DOUBLE_VERTICAL_RIGHT,
        horizontal_down: DOUBLE_HORIZONTAL_DOWN,
        horizontal_up: DOUBLE_HORIZONTAL_UP,
        cross: DOUBLE_CROSS,
    };

    pub const THICK: Set = Set {
        vertical: THICK_VERTICAL,
        horizontal: THICK_HORIZONTAL,
        top_right: THICK_TOP_RIGHT,
        top_left: THICK_TOP_LEFT,
        bottom_right: THICK_BOTTOM_RIGHT,
        bottom_left: THICK_BOTTOM_LEFT,
        vertical_left: THICK_VERTICAL_LEFT,
        vertical_right: THICK_VERTICAL_RIGHT,
        horizontal_down: THICK_HORIZONTAL_DOWN,
        horizontal_up: THICK_HORIZONTAL_UP,
        cross: THICK_CROSS,
    };
}

pub const DOT: &str = "•";

pub mod braille {
    pub const BLANK: u16 = 0x2800;
    pub const DOTS: [[u16; 2]; 4] = [
        [0x0001, 0x0008],
        [0x0002, 0x0010],
        [0x0004, 0x0020],
        [0x0040, 0x0080],
    ];
}

/// Marker to use when plotting data points
#[derive(Debug, Clone, Copy)]
pub enum Marker {
    /// One point per cell in shape of dot
    Dot,
    /// One point per cell in shape of a block
    Block,
    /// Up to 8 points per cell
    Braille,
}

#[cfg(test)]
mod tests {
    use super::*;
    use zmax_core::unicode::width::UnicodeWidthStr;

    fn line_set_symbols(set: &line::Set) -> Vec<(&'static str, &'static str)> {
        vec![
            ("vertical", set.vertical),
            ("horizontal", set.horizontal),
            ("top_right", set.top_right),
            ("top_left", set.top_left),
            ("bottom_right", set.bottom_right),
            ("bottom_left", set.bottom_left),
            ("vertical_left", set.vertical_left),
            ("vertical_right", set.vertical_right),
            ("horizontal_down", set.horizontal_down),
            ("horizontal_up", set.horizontal_up),
            ("cross", set.cross),
        ]
    }

    fn block_set_symbols(set: &block::Set) -> Vec<&'static str> {
        vec![
            set.full,
            set.seven_eighths,
            set.three_quarters,
            set.five_eighths,
            set.half,
            set.three_eighths,
            set.one_quarter,
            set.one_eighth,
            set.empty,
        ]
    }

    /// Every border glyph must occupy exactly one column. A two-column symbol
    /// slipping into a set shifts every cell after it on that row, so the box
    /// stops closing and the corner lands in the wrong place -- and nothing else
    /// would report it, because the string is still a valid single character.
    #[test]
    fn every_border_symbol_is_one_column_wide() {
        for (name, set) in [
            ("NORMAL", &line::NORMAL),
            ("ROUNDED", &line::ROUNDED),
            ("DOUBLE", &line::DOUBLE),
            ("THICK", &line::THICK),
        ] {
            for (field, symbol) in line_set_symbols(set) {
                assert_eq!(
                    symbol.width(),
                    1,
                    "{name}.{field} is {} columns: {symbol:?}",
                    symbol.width()
                );
                assert_eq!(symbol.chars().count(), 1, "{name}.{field} is one char");
            }
        }

        assert_eq!(DOT.width(), 1);
    }

    /// The gauge and sparkline ladders are drawn one cell at a time, so their
    /// symbols are single-column too -- including `empty`, which is a space
    /// rather than an empty string. An empty string would draw nothing and
    /// collapse the bar's width.
    #[test]
    fn every_level_symbol_is_one_column_wide() {
        for set in [&block::THREE_LEVELS, &block::NINE_LEVELS] {
            for symbol in block_set_symbols(set) {
                assert_eq!(symbol.width(), 1, "{symbol:?}");
            }
            assert_eq!(set.empty, " ", "empty draws a blank cell, not nothing");
        }

        for symbol in [
            bar::FULL,
            bar::SEVEN_EIGHTHS,
            bar::THREE_QUARTERS,
            bar::FIVE_EIGHTHS,
            bar::HALF,
            bar::THREE_EIGHTHS,
            bar::ONE_QUARTER,
            bar::ONE_EIGHTH,
        ] {
            assert_eq!(symbol.width(), 1, "{symbol:?}");
        }
    }

    /// The two ladders differ in resolution, which is the whole reason both
    /// exist: nine distinct steps, or three for terminals whose font lacks the
    /// eighths blocks.
    #[test]
    fn the_level_sets_offer_nine_steps_and_three() {
        let distinct = |set: &block::Set| {
            let mut symbols = block_set_symbols(set);
            symbols.sort_unstable();
            symbols.dedup();
            symbols.len()
        };

        assert_eq!(distinct(&block::NINE_LEVELS), 9);
        assert_eq!(distinct(&block::THREE_LEVELS), 3, "full, half, blank");
    }

    /// `ROUNDED` is `NORMAL` with rounded corners and nothing else -- it is
    /// defined with `..NORMAL`, so a new field added to the set silently
    /// inherits, which is correct only for edges.
    #[test]
    fn rounded_changes_the_corners_and_nothing_else() {
        let corners = ["top_right", "top_left", "bottom_right", "bottom_left"];

        for ((field, rounded), (_, normal)) in line_set_symbols(&line::ROUNDED)
            .into_iter()
            .zip(line_set_symbols(&line::NORMAL))
        {
            if corners.contains(&field) {
                assert_ne!(rounded, normal, "{field} should be rounded");
            } else {
                assert_eq!(rounded, normal, "{field} should match NORMAL");
            }
        }
    }
}
