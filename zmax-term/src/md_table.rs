//! Markdown pipe-table editing — the JetBrains Markdown table actions.
//!
//! A table is the run of lines starting with `|` around the cursor. Edits
//! work on its cells as rows of strings; the result is re-aligned by the same
//! `format_markdown_table` that `format_table_selection` uses, so every edit
//! leaves an aligned table behind.

/// A table found in a buffer: the line it starts on and its rows of cells.
/// The separator row (`| --- | :-: |`) is a row like any other; the helpers
/// below know to keep it in place.
#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    pub first_line: usize,
    pub rows: Vec<Vec<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Align {
    Left,
    Center,
    Right,
}

fn is_table_line(line: &str) -> bool {
    line.trim_start().starts_with('|')
}

/// The cells of one table line, trimmed, without the empty edges the outer
/// pipes leave.
pub fn parse_row(line: &str) -> Vec<String> {
    let t = line.trim();
    let t = t.strip_prefix('|').unwrap_or(t);
    let t = t.strip_suffix('|').unwrap_or(t);
    t.split('|').map(|c| c.trim().to_string()).collect()
}

/// Is `row` a separator row (`---`, `:--`, `:-:`, `--:` in every cell)?
pub fn is_separator(row: &[String]) -> bool {
    !row.is_empty()
        && row.iter().all(|c| {
            let t = c.trim();
            !t.is_empty() && t.contains('-') && t.chars().all(|ch| ch == '-' || ch == ':')
        })
}

/// The table containing line `at` of `lines`, if that line is a table line.
pub fn find(lines: &[&str], at: usize) -> Option<Table> {
    if !lines.get(at).is_some_and(|l| is_table_line(l)) {
        return None;
    }
    let first = (0..=at)
        .rev()
        .take_while(|&i| is_table_line(lines[i]))
        .last()?;
    let last = (at..lines.len())
        .take_while(|&i| is_table_line(lines[i]))
        .last()?;
    Some(Table {
        first_line: first,
        rows: lines[first..=last].iter().map(|l| parse_row(l)).collect(),
    })
}

/// Which column character offset `col` of a table line falls in: the number
/// of pipes before it, less the leading one.
pub fn column_at(line: &str, col: usize) -> usize {
    line.chars()
        .take(col)
        .filter(|&c| c == '|')
        .count()
        .saturating_sub(1)
}

/// Character offset of the content of cell `column` in an aligned table line.
pub fn cell_offset(line: &str, column: usize) -> usize {
    line.char_indices()
        .filter(|&(_, c)| c == '|')
        .nth(column)
        .map_or(0, |(byte, _)| line[..byte].chars().count() + 2)
}

impl Table {
    pub fn columns(&self) -> usize {
        self.rows.iter().map(Vec::len).max().unwrap_or(0)
    }

    fn separator_row(&self) -> Option<usize> {
        self.rows.iter().position(|r| is_separator(r))
    }

    fn blank_row(&self) -> Vec<String> {
        vec![String::new(); self.columns()]
    }

    /// Insert an empty row before row `at`. A row asked for above the
    /// separator row goes below it instead: the separator row must stay
    /// directly under the header.
    pub fn insert_row(&mut self, at: usize) -> usize {
        let at = match self.separator_row() {
            Some(sep) if at == sep => sep + 1,
            _ => at,
        };
        let blank = self.blank_row();
        self.rows.insert(at.min(self.rows.len()), blank);
        at
    }

    /// Remove row `at`, unless it is the separator row.
    pub fn remove_row(&mut self, at: usize) -> bool {
        if at >= self.rows.len() || self.separator_row() == Some(at) {
            return false;
        }
        self.rows.remove(at);
        true
    }

    /// Swap row `at` with the next body row in `down`'s direction. The header
    /// and the separator row never move. Returns where the row went.
    pub fn move_row(&mut self, at: usize, down: bool) -> Option<usize> {
        let first_body = self.separator_row().map_or(0, |sep| sep + 1);
        if at < first_body {
            return None;
        }
        let to = if down {
            at.checked_add(1).filter(|&to| to < self.rows.len())?
        } else {
            at.checked_sub(1).filter(|&to| to >= first_body)?
        };
        self.rows.swap(at, to);
        Some(to)
    }

    /// Insert an empty column before column `at` (`at == columns()` appends).
    pub fn insert_column(&mut self, at: usize) {
        let columns = self.columns();
        let at = at.min(columns);
        for row in &mut self.rows {
            row.resize(columns, String::new());
            let cell = if is_separator(row) { "---".to_string() } else { String::new() };
            row.insert(at, cell);
        }
    }

    /// Remove column `at`, keeping at least one column.
    pub fn remove_column(&mut self, at: usize) -> bool {
        let columns = self.columns();
        if columns <= 1 || at >= columns {
            return false;
        }
        for row in &mut self.rows {
            row.resize(columns, String::new());
            row.remove(at);
        }
        true
    }

    /// Swap column `at` with its neighbour. Returns where the column went.
    pub fn move_column(&mut self, at: usize, right: bool) -> Option<usize> {
        let columns = self.columns();
        let to = if right {
            at.checked_add(1).filter(|&to| to < columns)?
        } else {
            at.checked_sub(1)?
        };
        for row in &mut self.rows {
            row.resize(columns, String::new());
            row.swap(at, to);
        }
        Some(to)
    }

    /// Set the alignment of column `at` in the separator row.
    pub fn set_alignment(&mut self, at: usize, align: Align) -> bool {
        let columns = self.columns();
        let Some(sep) = self.separator_row() else {
            return false;
        };
        let row = &mut self.rows[sep];
        row.resize(columns, "---".to_string());
        row[at] = match align {
            Align::Left => ":--".to_string(),
            Align::Center => ":-:".to_string(),
            Align::Right => "--:".to_string(),
        };
        true
    }

    /// The table as `|`-joined lines, before alignment.
    pub fn to_text(&self) -> String {
        let columns = self.columns();
        self.rows
            .iter()
            .map(|row| {
                let cells: Vec<&str> = (0..columns)
                    .map(|i| row.get(i).map_or("", String::as_str))
                    .collect();
                format!("| {} |", cells.join(" | "))
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// An empty table with a header row, its separator row, and `body_rows` rows,
/// `columns` wide.
pub fn empty(columns: usize, body_rows: usize) -> Table {
    let columns = columns.max(1);
    let mut rows = vec![vec![String::new(); columns], vec!["---".to_string(); columns]];
    rows.extend((0..body_rows).map(|_| vec![String::new(); columns]));
    Table { first_line: 0, rows }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(text: &str) -> Table {
        let lines: Vec<&str> = text.lines().collect();
        find(&lines, 0).unwrap()
    }

    #[test]
    fn find_takes_the_whole_run_of_table_lines() {
        let lines = ["intro", "| a | b |", "| - | - |", "| 1 | 2 |", "after"];
        let t = find(&lines, 3).unwrap();
        assert_eq!(1, t.first_line);
        assert_eq!(3, t.rows.len());
        assert_eq!(None, find(&lines, 0));
    }

    #[test]
    fn column_at_counts_the_pipes_before_the_cursor() {
        let line = "| aa | bb | cc |";
        assert_eq!(0, column_at(line, 3));
        assert_eq!(1, column_at(line, 7));
        assert_eq!(2, column_at(line, 12));
        assert_eq!(7, cell_offset(line, 1));
    }

    #[test]
    fn a_row_asked_for_above_the_separator_goes_below_it() {
        let mut t = table("| a |\n| - |\n| 1 |");
        assert_eq!(2, t.insert_row(1));
        assert_eq!("| a |\n| - |\n|  |\n| 1 |", t.to_text());
    }

    #[test]
    fn the_separator_row_cannot_be_removed_or_moved_past() {
        let mut t = table("| a |\n| - |\n| 1 |\n| 2 |");
        assert!(!t.remove_row(1));
        assert_eq!(None, t.move_row(2, false));
        assert_eq!(Some(3), t.move_row(2, true));
        assert_eq!("| a |\n| - |\n| 2 |\n| 1 |", t.to_text());
    }

    #[test]
    fn a_new_column_gets_dashes_in_the_separator_row() {
        let mut t = table("| a |\n| - |\n| 1 |");
        t.insert_column(1);
        assert_eq!("| a |  |\n| - | --- |\n| 1 |  |", t.to_text());
    }

    #[test]
    fn columns_move_and_the_last_one_stays() {
        let mut t = table("| a | b |\n| - | - |\n| 1 | 2 |");
        assert_eq!(Some(1), t.move_column(0, true));
        assert_eq!("| b | a |\n| - | - |\n| 2 | 1 |", t.to_text());
        assert!(t.remove_column(0));
        assert!(!t.remove_column(0));
    }

    #[test]
    fn alignment_lives_in_the_separator_row() {
        let mut t = table("| a | b |\n| - | - |");
        assert!(t.set_alignment(1, Align::Right));
        assert_eq!("| a | b |\n| - | --: |", t.to_text());
    }
}
