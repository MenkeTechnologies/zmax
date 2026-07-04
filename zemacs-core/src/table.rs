//! Table — the pure, filesystem-free substrate behind the zemacs port of GNU
//! Emacs `table.el` (the text-based table editor).
//!
//! A [`Table`] is a dense grid of `String` cells with a fixed row/column count.
//! It knows how to grow and shrink (insert/delete a row or column), how wide
//! each column needs to be ([`Table::col_width`]), and how to draw itself as an
//! ASCII box-drawing table ([`Table::render`]) using `+`, `-` and `|`. Cell
//! navigation that wraps across the grid lives in the free functions
//! [`forward_cell`] / [`backward_cell`]. All of this is I/O-free and unit-tested
//! here; the interactive overlay in `zemacs-term/src/ui/table.rs` layers key
//! handling and rendering on top.

/// A rectangular grid of text cells.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Table {
    cells: Vec<Vec<String>>,
    rows: usize,
    cols: usize,
}

impl Table {
    /// A fresh `rows` x `cols` table of empty cells.
    pub fn new(rows: usize, cols: usize) -> Self {
        Table {
            cells: vec![vec![String::new(); cols]; rows],
            rows,
            cols,
        }
    }

    /// Number of rows.
    pub fn rows(&self) -> usize {
        self.rows
    }

    /// Number of columns.
    pub fn cols(&self) -> usize {
        self.cols
    }

    /// The contents of cell `(r, c)`, or `None` if out of bounds.
    pub fn get(&self, r: usize, c: usize) -> Option<&str> {
        self.cells.get(r).and_then(|row| row.get(c)).map(|s| s.as_str())
    }

    /// Replace the contents of cell `(r, c)`. Out-of-bounds writes are ignored.
    pub fn set(&mut self, r: usize, c: usize, val: impl Into<String>) {
        if let Some(cell) = self.cells.get_mut(r).and_then(|row| row.get_mut(c)) {
            *cell = val.into();
        }
    }

    /// Insert a new empty row at index `at` (clamped to `[0, rows]`).
    pub fn insert_row(&mut self, at: usize) {
        let at = at.min(self.rows);
        self.cells.insert(at, vec![String::new(); self.cols]);
        self.rows += 1;
    }

    /// Delete the row at index `at`. No-op if out of bounds.
    pub fn delete_row(&mut self, at: usize) {
        if at < self.rows {
            self.cells.remove(at);
            self.rows -= 1;
        }
    }

    /// Insert a new empty column at index `at` (clamped to `[0, cols]`).
    pub fn insert_col(&mut self, at: usize) {
        let at = at.min(self.cols);
        for row in &mut self.cells {
            row.insert(at, String::new());
        }
        self.cols += 1;
    }

    /// Delete the column at index `at`. No-op if out of bounds.
    pub fn delete_col(&mut self, at: usize) {
        if at < self.cols {
            for row in &mut self.cells {
                row.remove(at);
            }
            self.cols -= 1;
        }
    }

    /// Display width of column `c`: the widest cell in that column, never less
    /// than 1 so every column draws at least one space.
    pub fn col_width(&self, c: usize) -> usize {
        let mut w = 1;
        for row in &self.cells {
            if let Some(cell) = row.get(c) {
                w = w.max(cell.chars().count());
            }
        }
        w
    }

    /// Render the grid as an ASCII box-drawing table: `+`/`-` borders, `|`
    /// column separators, and each cell space-padded to its column width with a
    /// one-space gutter on either side. Ends with a trailing newline.
    pub fn render(&self) -> String {
        let widths: Vec<usize> = (0..self.cols).map(|c| self.col_width(c)).collect();

        let mut separator = String::from("+");
        for w in &widths {
            separator.push_str(&"-".repeat(w + 2));
            separator.push('+');
        }

        let mut out = String::new();
        out.push_str(&separator);
        out.push('\n');
        for r in 0..self.rows {
            let mut line = String::from("|");
            for (c, &w) in widths.iter().enumerate() {
                let content = self.get(r, c).unwrap_or("");
                let pad = w.saturating_sub(content.chars().count());
                line.push(' ');
                line.push_str(content);
                for _ in 0..pad {
                    line.push(' ');
                }
                line.push(' ');
                line.push('|');
            }
            out.push_str(&line);
            out.push('\n');
            out.push_str(&separator);
            out.push('\n');
        }
        out
    }
}

/// The next cell after `(r, c)` in row-major order, wrapping from the end of a
/// row to the start of the next and from the last cell back to `(0, 0)`.
pub fn forward_cell(r: usize, c: usize, rows: usize, cols: usize) -> (usize, usize) {
    if rows == 0 || cols == 0 {
        return (0, 0);
    }
    if c + 1 < cols {
        (r, c + 1)
    } else if r + 1 < rows {
        (r + 1, 0)
    } else {
        (0, 0)
    }
}

/// The previous cell before `(r, c)` in row-major order, wrapping from the start
/// of a row to the end of the previous and from `(0, 0)` back to the last cell.
pub fn backward_cell(r: usize, c: usize, rows: usize, cols: usize) -> (usize, usize) {
    if rows == 0 || cols == 0 {
        return (0, 0);
    }
    if c > 0 {
        (r, c - 1)
    } else if r > 0 {
        (r - 1, cols - 1)
    } else {
        (rows - 1, cols - 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_and_get_roundtrips() {
        let mut t = Table::new(2, 2);
        t.set(1, 0, "hi");
        assert_eq!(t.get(1, 0), Some("hi"));
        assert_eq!(t.get(0, 0), Some(""));
        assert_eq!(t.get(5, 5), None);
    }

    #[test]
    fn insert_and_delete_row_adjust_dimensions() {
        let mut t = Table::new(2, 3);
        t.insert_row(1);
        assert_eq!(t.rows(), 3);
        assert_eq!(t.get(1, 0), Some("")); // fresh row is blank
        t.delete_row(0);
        assert_eq!(t.rows(), 2);
        assert_eq!(t.cols(), 3);
    }

    #[test]
    fn insert_and_delete_col_adjust_dimensions() {
        let mut t = Table::new(2, 2);
        t.set(0, 1, "x");
        t.insert_col(1);
        assert_eq!(t.cols(), 3);
        assert_eq!(t.get(0, 1), Some("")); // inserted blank column
        assert_eq!(t.get(0, 2), Some("x")); // old col shifted right
        t.delete_col(0);
        assert_eq!(t.cols(), 2);
        assert_eq!(t.rows(), 2);
    }

    #[test]
    fn col_width_reflects_the_widest_cell() {
        let mut t = Table::new(2, 1);
        assert_eq!(t.col_width(0), 1, "empty column still has width 1");
        t.set(0, 0, "a");
        t.set(1, 0, "wider");
        assert_eq!(t.col_width(0), 5);
    }

    #[test]
    fn render_draws_borders_and_pads_to_column_width() {
        let mut t = Table::new(1, 2);
        t.set(0, 0, "ab");
        t.set(0, 1, "c");
        let out = t.render();
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines[0], "+----+---+"); // widths 2 and 1, plus gutters
        assert_eq!(lines[1], "| ab | c |"); // second cell padded to width 1
        assert_eq!(lines[2], "+----+---+");
    }

    #[test]
    fn forward_cell_wraps_at_row_end() {
        // 2x2 grid: (0,1) -> (1,0) -> ... -> (1,1) -> (0,0)
        assert_eq!(forward_cell(0, 0, 2, 2), (0, 1));
        assert_eq!(forward_cell(0, 1, 2, 2), (1, 0));
        assert_eq!(forward_cell(1, 1, 2, 2), (0, 0));
    }

    #[test]
    fn backward_cell_wraps_at_row_start() {
        assert_eq!(backward_cell(1, 1, 2, 2), (1, 0));
        assert_eq!(backward_cell(1, 0, 2, 2), (0, 1));
        assert_eq!(backward_cell(0, 0, 2, 2), (1, 1));
    }

    #[test]
    fn navigation_is_a_noop_on_a_degenerate_grid() {
        assert_eq!(forward_cell(0, 0, 0, 0), (0, 0));
        assert_eq!(backward_cell(0, 0, 3, 0), (0, 0));
    }
}
