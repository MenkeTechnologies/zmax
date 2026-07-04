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
//!
//! On top of the basic grid this module ports the higher-level `table.el`
//! operations that map faithfully to pure grid logic: per-cell/column
//! [justification](Justify), per-column minimum width and per-row minimum
//! height ([`Table::widen_cell`] / [`Table::heighten_cell`] and friends),
//! [recognition](recognize) of an ASCII grid back into a `Table`,
//! [capture](capture) of delimited plain text into a table, [release](Table::release)
//! back to plain text, [source generation](Table::generate_source) (HTML / LaTeX /
//! CALS), [`Table::query_dimension`], and [`Table::insert_sequence`].

use std::fmt::Write as _;

/// Per-cell text justification, mirroring the four modes `table.el` offers
/// (`table-justify` / `M-x table-justify-cell`). [`Justify::Left`] is the
/// default so a fresh table renders exactly as before this was added.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Justify {
    /// Content flush left, padding on the right.
    #[default]
    Left,
    /// Content flush right, padding on the left.
    Right,
    /// Content centred, any odd padding char to the right.
    Center,
    /// Words spread out so the line exactly fills the column width.
    Full,
}

impl Justify {
    /// Parse the Emacs justification symbol name (`"left"`, `"right"`,
    /// `"center"`, `"full"`); anything else falls back to [`Justify::Left`].
    pub fn parse(name: &str) -> Justify {
        match name.trim().to_ascii_lowercase().as_str() {
            "right" => Justify::Right,
            "center" | "centre" => Justify::Center,
            "full" => Justify::Full,
            _ => Justify::Left,
        }
    }

    /// The single LaTeX `tabular` column-spec letter for this justification
    /// (`l`/`r`/`c`; `full` maps to `l`).
    fn latex_spec(self) -> char {
        match self {
            Justify::Left | Justify::Full => 'l',
            Justify::Right => 'r',
            Justify::Center => 'c',
        }
    }
}

/// A rectangular grid of text cells.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Table {
    cells: Vec<Vec<String>>,
    justify: Vec<Vec<Justify>>,
    /// Per-column minimum display width (0 = no floor beyond the content).
    min_col_width: Vec<usize>,
    /// Per-row minimum display height in lines (0/1 = one line).
    min_row_height: Vec<usize>,
    rows: usize,
    cols: usize,
}

/// Display width of a (possibly multi-line) cell: the widest of its lines.
fn text_width(s: &str) -> usize {
    s.split('\n').map(|l| l.chars().count()).max().unwrap_or(0)
}

/// Number of visual lines a cell occupies (at least 1).
fn text_height(s: &str) -> usize {
    s.split('\n').count().max(1)
}

/// A run of `n` spaces.
fn spaces(n: usize) -> String {
    " ".repeat(n)
}

impl Table {
    /// A fresh `rows` x `cols` table of empty, left-justified cells.
    pub fn new(rows: usize, cols: usize) -> Self {
        Table {
            cells: vec![vec![String::new(); cols]; rows],
            justify: vec![vec![Justify::Left; cols]; rows],
            min_col_width: vec![0; cols],
            min_row_height: vec![1; rows],
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
        self.cells
            .get(r)
            .and_then(|row| row.get(c))
            .map(|s| s.as_str())
    }

    /// Replace the contents of cell `(r, c)`. Out-of-bounds writes are ignored.
    pub fn set(&mut self, r: usize, c: usize, val: impl Into<String>) {
        if let Some(cell) = self.cells.get_mut(r).and_then(|row| row.get_mut(c)) {
            *cell = val.into();
        }
    }

    /// The justification of cell `(r, c)` (default [`Justify::Left`]).
    pub fn get_justify(&self, r: usize, c: usize) -> Justify {
        self.justify
            .get(r)
            .and_then(|row| row.get(c))
            .copied()
            .unwrap_or_default()
    }

    /// Set the justification of a single cell (`table-justify-cell`).
    pub fn set_justify(&mut self, r: usize, c: usize, j: Justify) {
        if let Some(cell) = self.justify.get_mut(r).and_then(|row| row.get_mut(c)) {
            *cell = j;
        }
    }

    /// Justify every cell of column `c` (`table-justify-column`).
    pub fn justify_column(&mut self, c: usize, j: Justify) {
        for row in &mut self.justify {
            if let Some(cell) = row.get_mut(c) {
                *cell = j;
            }
        }
    }

    /// Justify every cell of row `r` (`table-justify-row`).
    pub fn justify_row(&mut self, r: usize, j: Justify) {
        if let Some(row) = self.justify.get_mut(r) {
            for cell in row.iter_mut() {
                *cell = j;
            }
        }
    }

    /// Justify the whole table (`table-justify` with a table target).
    pub fn justify_all(&mut self, j: Justify) {
        for row in &mut self.justify {
            for cell in row.iter_mut() {
                *cell = j;
            }
        }
    }

    /// Insert a new empty row at index `at` (clamped to `[0, rows]`).
    pub fn insert_row(&mut self, at: usize) {
        let at = at.min(self.rows);
        self.cells.insert(at, vec![String::new(); self.cols]);
        self.justify.insert(at, vec![Justify::Left; self.cols]);
        self.min_row_height.insert(at, 1);
        self.rows += 1;
    }

    /// Delete the row at index `at`. No-op if out of bounds.
    pub fn delete_row(&mut self, at: usize) {
        if at < self.rows {
            self.cells.remove(at);
            self.justify.remove(at);
            self.min_row_height.remove(at);
            self.rows -= 1;
        }
    }

    /// Insert a new empty column at index `at` (clamped to `[0, cols]`).
    pub fn insert_col(&mut self, at: usize) {
        let at = at.min(self.cols);
        for row in &mut self.cells {
            row.insert(at, String::new());
        }
        for row in &mut self.justify {
            row.insert(at, Justify::Left);
        }
        self.min_col_width.insert(at, 0);
        self.cols += 1;
    }

    /// Delete the column at index `at`. No-op if out of bounds.
    pub fn delete_col(&mut self, at: usize) {
        if at < self.cols {
            for row in &mut self.cells {
                row.remove(at);
            }
            for row in &mut self.justify {
                row.remove(at);
            }
            self.min_col_width.remove(at);
            self.cols -= 1;
        }
    }

    /// Display width of column `c`: the widest cell in that column, floored by
    /// the column's minimum width and never less than 1 so every column draws
    /// at least one space.
    pub fn col_width(&self, c: usize) -> usize {
        let mut w = 1;
        w = w.max(self.min_col_width.get(c).copied().unwrap_or(0));
        for row in &self.cells {
            if let Some(cell) = row.get(c) {
                w = w.max(text_width(cell));
            }
        }
        w
    }

    /// Display height of row `r`: the tallest cell in that row, floored by the
    /// row's minimum height and never less than 1.
    pub fn row_height(&self, r: usize) -> usize {
        let mut h = 1.max(self.min_row_height.get(r).copied().unwrap_or(1));
        if let Some(row) = self.cells.get(r) {
            for cell in row {
                h = h.max(text_height(cell));
            }
        }
        h
    }

    /// Widen column `c` by `by` characters (`table-widen-cell`). This raises the
    /// column's minimum width; it operates on the whole column, since a dense
    /// grid shares one width per column.
    pub fn widen_cell(&mut self, c: usize, by: usize) {
        if let Some(w) = self.min_col_width.get_mut(c) {
            *w = self
                .cells
                .iter()
                .filter_map(|row| row.get(c))
                .map(|s| text_width(s))
                .max()
                .unwrap_or(0)
                .max(*w)
                + by;
        }
    }

    /// Narrow column `c` by `by` characters (`table-narrow-cell`), never below
    /// the content's own width.
    pub fn narrow_cell(&mut self, c: usize, by: usize) {
        if let Some(w) = self.min_col_width.get_mut(c) {
            *w = w.saturating_sub(by);
        }
    }

    /// Heighten row `r` by `by` lines (`table-heighten-cell`); raises the row's
    /// minimum height. Operates on the whole row (dense grid).
    pub fn heighten_cell(&mut self, r: usize, by: usize) {
        let target = self.row_height(r) + by;
        if let Some(h) = self.min_row_height.get_mut(r) {
            *h = target;
        }
    }

    /// Shorten row `r` by `by` lines (`table-shorten-cell`), never below 1.
    pub fn shorten_cell(&mut self, r: usize, by: usize) {
        if let Some(h) = self.min_row_height.get_mut(r) {
            *h = h.saturating_sub(by).max(1);
        }
    }

    /// Render the grid as an ASCII box-drawing table: `+`/`-` borders, `|`
    /// column separators, and each cell justified to its column width with a
    /// one-space gutter on either side. Multi-line cells and tall rows draw
    /// across several lines. Ends with a trailing newline.
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
            let height = self.row_height(r);
            // Pre-split each cell of the row into its visual lines.
            let cell_lines: Vec<Vec<&str>> = (0..self.cols)
                .map(|c| self.get(r, c).unwrap_or("").split('\n').collect())
                .collect();
            for line_idx in 0..height {
                let mut line = String::from("|");
                for (c, &w) in widths.iter().enumerate() {
                    let content = cell_lines[c].get(line_idx).copied().unwrap_or("");
                    line.push(' ');
                    line.push_str(&justify_line(content, w, self.get_justify(r, c)));
                    line.push(' ');
                    line.push('|');
                }
                out.push_str(&line);
                out.push('\n');
            }
            out.push_str(&separator);
            out.push('\n');
        }
        out
    }

    /// `table-query-dimension`: a one-line report of the size of cell `(r, c)`,
    /// the whole rendered table, the grid dimension and the total cell count —
    /// mirroring the Emacs echo-area message shape.
    pub fn query_dimension(&self, r: usize, c: usize) -> String {
        let rendered = self.render();
        let t_h = rendered.lines().count();
        let t_w = rendered
            .lines()
            .map(|l| l.chars().count())
            .max()
            .unwrap_or(0);
        let cell_w = self.col_width(c);
        let cell_h = self.row_height(r);
        format!(
            "Cell: ({cell_w}w, {cell_h}h)  Table: ({t_w}w, {t_h}h)  Dim: ({}r, {}c)  Total Cells: {}",
            self.rows,
            self.cols,
            self.rows * self.cols
        )
    }

    /// `table-insert-sequence`: fill `count` cells, starting at `start` and
    /// stepping `interval` cells forward each time, with an incrementing
    /// sequence. The trailing run of digits in `pattern` seeds the number
    /// (default 0) and is replaced by seed + i*`increment`; the non-digit
    /// prefix is kept. E.g. pattern `"a1"`, count 3, increment 1 →
    /// `a1`, `a2`, `a3`; pattern `"9"`, increment 2 → `9`, `11`, `13`.
    pub fn insert_sequence(
        &mut self,
        start: (usize, usize),
        pattern: &str,
        count: usize,
        increment: i64,
        interval: usize,
    ) {
        if self.rows == 0 || self.cols == 0 {
            return;
        }
        let digits_start = pattern
            .trim_end_matches(|ch: char| ch.is_ascii_digit())
            .len();
        let prefix = &pattern[..digits_start];
        let seed: i64 = pattern[digits_start..].parse().unwrap_or(0);
        let step = interval.max(1);

        let (mut r, mut c) = start;
        for i in 0..count {
            let val = seed + (i as i64) * increment;
            self.set(r, c, format!("{prefix}{val}"));
            for _ in 0..step {
                (r, c) = forward_cell(r, c, self.rows, self.cols);
            }
        }
    }

    /// `table-span-cell` (rightward): merge cell `(r, c)` with its right
    /// neighbour by appending the neighbour's content (space-joined) and
    /// clearing it. On a dense grid this cannot draw a true multi-column
    /// spanned cell — only the text is merged.
    pub fn span_cell_right(&mut self, r: usize, c: usize) {
        if c + 1 >= self.cols {
            return;
        }
        let left = self.get(r, c).unwrap_or("").to_string();
        let right = self.get(r, c + 1).unwrap_or("").to_string();
        let merged = match (left.is_empty(), right.is_empty()) {
            (_, true) => left,
            (true, false) => right,
            (false, false) => format!("{left} {right}"),
        };
        self.set(r, c, merged);
        self.set(r, c + 1, "");
    }

    /// `table-split-cell-horizontally`: insert a fresh column after `c` and move
    /// the part of cell `(r, c)` at/after char index `at` into it. Because the
    /// grid is dense the new column is inserted across every row.
    pub fn split_cell_horizontally(&mut self, r: usize, c: usize, at: usize) {
        let content = self.get(r, c).unwrap_or("").to_string();
        let idx = at.min(content.chars().count());
        let head: String = content.chars().take(idx).collect();
        let tail: String = content.chars().skip(idx).collect();
        self.insert_col(c + 1);
        self.set(r, c, head);
        self.set(r, c + 1, tail);
    }

    /// `table-split-cell-vertically`: insert a fresh row below `r` and move the
    /// part of cell `(r, c)` at/after char index `at` into it. The new row is
    /// inserted across every column (dense grid).
    pub fn split_cell_vertically(&mut self, r: usize, c: usize, at: usize) {
        let content = self.get(r, c).unwrap_or("").to_string();
        let idx = at.min(content.chars().count());
        let head: String = content.chars().take(idx).collect();
        let tail: String = content.chars().skip(idx).collect();
        self.insert_row(r + 1);
        self.set(r, c, head);
        self.set(r + 1, c, tail);
    }

    /// `table-release`: turn the table back into plain text. Each row's cells are
    /// space-joined (trimmed) and rows are newline-joined, ending with a newline.
    pub fn release(&self) -> String {
        let mut out = String::new();
        for r in 0..self.rows {
            let row: Vec<String> = (0..self.cols)
                .map(|c| {
                    self.get(r, c)
                        .unwrap_or("")
                        .replace('\n', " ")
                        .trim()
                        .to_string()
                })
                .filter(|s| !s.is_empty())
                .collect();
            out.push_str(row.join(" ").trim_end());
            out.push('\n');
        }
        out
    }

    /// `table-generate-source`: emit the table as HTML, LaTeX or CALS source.
    pub fn generate_source(&self, lang: SourceLang) -> String {
        match lang {
            SourceLang::Html => self.generate_html(),
            SourceLang::Latex => self.generate_latex(),
            SourceLang::Cals => self.generate_cals(),
        }
    }

    fn generate_html(&self) -> String {
        let mut out = String::from("<table border=\"1\">\n");
        for r in 0..self.rows {
            out.push_str("  <tr>\n");
            for c in 0..self.cols {
                let cell = html_escape(self.get(r, c).unwrap_or("")).replace('\n', "<br />");
                let _ = writeln!(out, "    <td>{cell}</td>");
            }
            out.push_str("  </tr>\n");
        }
        out.push_str("</table>\n");
        out
    }

    fn generate_latex(&self) -> String {
        let mut spec = String::from("|");
        for c in 0..self.cols {
            // Column justification comes from the first row's cell.
            spec.push(self.get_justify(0, c).latex_spec());
            spec.push('|');
        }
        let mut out = format!("\\begin{{tabular}}{{{spec}}}\n\\hline\n");
        for r in 0..self.rows {
            let cells: Vec<String> = (0..self.cols)
                .map(|c| latex_escape(&self.get(r, c).unwrap_or("").replace('\n', " ")))
                .collect();
            let _ = writeln!(out, "{} \\\\", cells.join(" & "));
            out.push_str("\\hline\n");
        }
        out.push_str("\\end{tabular}\n");
        out
    }

    fn generate_cals(&self) -> String {
        let mut out = format!("<tgroup cols=\"{}\">\n  <tbody>\n", self.cols);
        for r in 0..self.rows {
            out.push_str("    <row>\n");
            for c in 0..self.cols {
                let cell = html_escape(self.get(r, c).unwrap_or("")).replace('\n', " ");
                let _ = writeln!(out, "      <entry>{cell}</entry>");
            }
            out.push_str("    </row>\n");
        }
        out.push_str("  </tbody>\n</tgroup>\n");
        out
    }
}

/// The source languages [`Table::generate_source`] can emit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceLang {
    /// HTML `<table>`.
    Html,
    /// LaTeX `tabular`.
    Latex,
    /// OASIS CALS (DocBook) `tgroup`.
    Cals,
}

impl SourceLang {
    /// Parse a language name (`"html"`, `"latex"`/`"tex"`, `"cals"`); unknown
    /// names default to [`SourceLang::Html`].
    pub fn parse(name: &str) -> SourceLang {
        match name.trim().to_ascii_lowercase().as_str() {
            "latex" | "tex" => SourceLang::Latex,
            "cals" | "docbook" => SourceLang::Cals,
            _ => SourceLang::Html,
        }
    }
}

/// Justify a single line to exactly `width` columns using `j`.
pub fn justify_line(s: &str, width: usize, j: Justify) -> String {
    let len = s.chars().count();
    if len >= width {
        return s.to_string();
    }
    let pad = width - len;
    match j {
        Justify::Left => format!("{s}{}", spaces(pad)),
        Justify::Right => format!("{}{s}", spaces(pad)),
        Justify::Center => {
            let left = pad / 2;
            format!("{}{s}{}", spaces(left), spaces(pad - left))
        }
        Justify::Full => full_justify(s, width),
    }
}

/// Full ("newspaper") justification: spread the extra space between words so
/// the line exactly fills `width`. A single word (or none) is left-justified.
fn full_justify(s: &str, width: usize) -> String {
    let words: Vec<&str> = s.split_whitespace().collect();
    if words.len() <= 1 {
        let len = s.chars().count();
        return format!("{s}{}", spaces(width.saturating_sub(len)));
    }
    let word_chars: usize = words.iter().map(|w| w.chars().count()).sum();
    let gaps = words.len() - 1;
    let space_total = width.saturating_sub(word_chars);
    let per = space_total / gaps;
    let rem = space_total % gaps;
    let mut out = String::new();
    for (i, word) in words.iter().enumerate() {
        out.push_str(word);
        if i < gaps {
            let extra = per + usize::from(i < rem);
            out.push_str(&spaces(extra));
        }
    }
    out
}

/// Escape `&`, `<`, `>` for HTML/CALS output.
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Escape the LaTeX special characters so cell text survives `tabular`.
fn latex_escape(s: &str) -> String {
    let mut out = String::new();
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\textbackslash{}"),
            '&' | '%' | '$' | '#' | '_' | '{' | '}' => {
                out.push('\\');
                out.push(ch);
            }
            '~' => out.push_str("\\textasciitilde{}"),
            '^' => out.push_str("\\textasciicircum{}"),
            _ => out.push(ch),
        }
    }
    out
}

/// `table-capture`: build a table from delimited plain `text`. Rows are split on
/// `row_delim` (default: newlines) and cells on `col_delim` (default: runs of
/// whitespace). Short rows are padded to the widest row's column count.
pub fn capture(text: &str, col_delim: Option<&str>, row_delim: Option<&str>) -> Table {
    let row_strings: Vec<String> = match row_delim {
        Some(d) if !d.is_empty() => text.split(d).map(str::to_string).collect(),
        _ => text.lines().map(str::to_string).collect(),
    };
    // Drop a trailing empty row created by a final delimiter/newline.
    let mut row_strings = row_strings;
    if row_strings.len() > 1 && row_strings.last().is_some_and(|r| r.trim().is_empty()) {
        row_strings.pop();
    }

    let split_cols = |row: &str| -> Vec<String> {
        match col_delim {
            Some(d) if !d.is_empty() => row.split(d).map(|c| c.trim().to_string()).collect(),
            _ => row.split_whitespace().map(str::to_string).collect(),
        }
    };

    let parsed: Vec<Vec<String>> = row_strings.iter().map(|r| split_cols(r)).collect();
    let cols = parsed.iter().map(Vec::len).max().unwrap_or(0).max(1);
    let rows = parsed.len().max(1);

    let mut table = Table::new(rows, cols);
    for (r, cells) in parsed.iter().enumerate() {
        for (c, val) in cells.iter().enumerate() {
            table.set(r, c, val.clone());
        }
    }
    table
}

/// `table-recognize`: parse an ASCII box-drawing table back into a [`Table`].
/// Accepts the exact output of [`Table::render`] (plus any surrounding blank or
/// non-table lines, which are ignored). Cell text is trimmed of the drawing
/// gutters; justification and minimum sizes are not recovered (Emacs stores
/// those in text properties that plain text loses too). Returns `None` if no
/// table grid is found.
pub fn recognize(text: &str) -> Option<Table> {
    let tbl: Vec<&str> = text
        .lines()
        .filter(|l| {
            let t = l.trim_start();
            t.starts_with('+') || t.starts_with('|')
        })
        .collect();
    if tbl.is_empty() {
        return None;
    }

    // Group consecutive content (`|`) lines; each group is one table row.
    let mut groups: Vec<Vec<&str>> = Vec::new();
    let mut cur: Vec<&str> = Vec::new();
    for l in &tbl {
        if l.trim_start().starts_with('+') {
            if !cur.is_empty() {
                groups.push(std::mem::take(&mut cur));
            }
        } else {
            cur.push(l);
        }
    }
    if !cur.is_empty() {
        groups.push(cur);
    }
    if groups.is_empty() {
        return None;
    }

    // Column count from the first content line's pipe positions.
    let first_pipes = pipe_positions(groups[0][0]);
    if first_pipes.len() < 2 {
        return None;
    }
    let cols = first_pipes.len() - 1;
    let rows = groups.len();
    let mut table = Table::new(rows, cols);

    for (r, group) in groups.iter().enumerate() {
        // For each column, gather its slice from every visual line of the group.
        let mut col_lines: Vec<Vec<String>> = vec![Vec::new(); cols];
        for line in group {
            let Some(fields) = extract_fields(line, cols) else {
                continue;
            };
            for (c, f) in fields.into_iter().enumerate() {
                col_lines[c].push(f.trim().to_string());
            }
        }
        for (c, lines) in col_lines.iter().enumerate() {
            // Drop trailing blank visual lines, then join what remains.
            let mut lines = lines.clone();
            while lines.last().is_some_and(|l| l.is_empty()) {
                lines.pop();
            }
            table.set(r, c, lines.join("\n"));
        }
    }
    Some(table)
}

/// Char indices of every `|` in a line.
fn pipe_positions(line: &str) -> Vec<usize> {
    line.chars()
        .enumerate()
        .filter(|(_, ch)| *ch == '|')
        .map(|(i, _)| i)
        .collect()
}

/// Extract the `cols` cell fields between the `|` separators of a content line,
/// stripping the single gutter space on each side. Returns `None` if the line
/// does not have exactly `cols + 1` pipes.
fn extract_fields(line: &str, cols: usize) -> Option<Vec<String>> {
    let chars: Vec<char> = line.chars().collect();
    let pipes = pipe_positions(line);
    if pipes.len() != cols + 1 {
        return None;
    }
    let mut out = Vec::with_capacity(cols);
    for c in 0..cols {
        let start = pipes[c] + 1;
        let end = pipes[c + 1];
        let field: String = chars[start..end].iter().collect();
        // Strip one leading and one trailing gutter space if present.
        let field = field.strip_prefix(' ').unwrap_or(&field);
        let field = field.strip_suffix(' ').unwrap_or(field);
        out.push(field.to_string());
    }
    Some(out)
}

/// Map a caret position inside a rendered table `block` (its `line` within the
/// block, 0-based, and character `col` within that line) to the `(row, col)`
/// of the cell it falls in. Border lines and out-of-range positions clamp to
/// the nearest cell. Returns `None` if `block` has no grid.
pub fn cell_at_position(block: &str, line: usize, col: usize) -> Option<(usize, usize)> {
    let lines: Vec<&str> = block.lines().collect();
    if lines.is_empty() {
        return None;
    }
    // Column count from the first content line.
    let first_content = lines.iter().find(|l| l.trim_start().starts_with('|'))?;
    let ncols = pipe_positions(first_content).len().saturating_sub(1);
    if ncols == 0 {
        return None;
    }
    let nborders = lines
        .iter()
        .filter(|l| l.trim_start().starts_with('+'))
        .count();
    let nrows = nborders.saturating_sub(1).max(1);

    // Row = number of border lines strictly before `line`, minus 1.
    let line = line.min(lines.len().saturating_sub(1));
    let borders_before = lines[..line]
        .iter()
        .filter(|l| l.trim_start().starts_with('+'))
        .count();
    let row = borders_before.saturating_sub(1).min(nrows - 1);

    // Column = number of `|` at char-positions < col, minus 1.
    let target = lines[line];
    let pipes_before = target.chars().take(col).filter(|ch| *ch == '|').count();
    let c = pipes_before.saturating_sub(1).min(ncols - 1);
    Some((row, c))
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

    // ---- justification ---------------------------------------------------

    #[test]
    fn justify_line_modes_pad_exactly() {
        assert_eq!(justify_line("ab", 6, Justify::Left), "ab    ");
        assert_eq!(justify_line("ab", 6, Justify::Right), "    ab");
        assert_eq!(justify_line("ab", 6, Justify::Center), "  ab  ");
        // odd padding puts the extra char on the right
        assert_eq!(justify_line("ab", 5, Justify::Center), " ab  ");
        // no room: content returned verbatim
        assert_eq!(justify_line("abcdef", 4, Justify::Left), "abcdef");
    }

    #[test]
    fn full_justify_spreads_space_between_words() {
        // "a b c" in width 9: 3 words (3 chars), 6 spaces over 2 gaps = 3 each.
        assert_eq!(justify_line("a b c", 9, Justify::Full), "a   b   c");
        // width 8: 5 spaces over 2 gaps -> 3 then 2 (remainder on the left gap)
        assert_eq!(justify_line("a b c", 8, Justify::Full), "a   b  c");
        // single word behaves like left
        assert_eq!(justify_line("word", 8, Justify::Full), "word    ");
    }

    #[test]
    fn render_honours_per_cell_justification() {
        let mut t = Table::new(1, 1);
        t.set(0, 0, "x");
        t.set_justify(0, 0, Justify::Right);
        t.widen_cell(0, 4); // force a wide column so justification shows
        let lines: Vec<String> = t.render().lines().map(String::from).collect();
        // width 5 (1 content + 4 widen): right-justified "x" -> "    x"
        assert_eq!(lines[1], "|     x |");
    }

    // ---- widen / narrow / heighten / shorten -----------------------------

    #[test]
    fn widen_and_narrow_change_column_width() {
        let mut t = Table::new(1, 1);
        t.set(0, 0, "ab");
        assert_eq!(t.col_width(0), 2);
        t.widen_cell(0, 3);
        assert_eq!(t.col_width(0), 5);
        t.narrow_cell(0, 2);
        assert_eq!(t.col_width(0), 3);
        // never below the actual content width
        t.narrow_cell(0, 100);
        assert_eq!(t.col_width(0), 2);
    }

    #[test]
    fn heighten_pads_the_row_vertically() {
        let mut t = Table::new(1, 1);
        t.set(0, 0, "x");
        t.heighten_cell(0, 1);
        assert_eq!(t.row_height(0), 2);
        let lines: Vec<String> = t.render().lines().map(String::from).collect();
        // border, content line, blank padded line, border
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[1], "| x |");
        assert_eq!(lines[2], "|   |");
        t.shorten_cell(0, 1);
        assert_eq!(t.row_height(0), 1);
    }

    // ---- recognize / round-trip ------------------------------------------

    #[test]
    fn recognize_round_trips_render() {
        let mut t = Table::new(2, 2);
        t.set(0, 0, "name");
        t.set(0, 1, "age");
        t.set(1, 0, "sam");
        t.set(1, 1, "42");
        let rendered = t.render();
        let back = recognize(&rendered).expect("should recognize");
        assert_eq!(back.rows(), 2);
        assert_eq!(back.cols(), 2);
        assert_eq!(back.get(0, 0), Some("name"));
        assert_eq!(back.get(0, 1), Some("age"));
        assert_eq!(back.get(1, 0), Some("sam"));
        assert_eq!(back.get(1, 1), Some("42"));
    }

    #[test]
    fn recognize_handles_surrounding_text_and_multiline() {
        let mut t = Table::new(1, 2);
        t.set(0, 0, "top\nbottom");
        t.set(0, 1, "x");
        let rendered = t.render();
        let wrapped = format!("intro line\n{rendered}\ntrailing\n");
        let back = recognize(&wrapped).expect("recognize inside prose");
        assert_eq!(back.cols(), 2);
        assert_eq!(back.get(0, 0), Some("top\nbottom"));
        assert_eq!(back.get(0, 1), Some("x"));
    }

    #[test]
    fn recognize_rejects_non_tables() {
        assert!(recognize("just some words\nno grid here").is_none());
    }

    // ---- capture / release ------------------------------------------------

    #[test]
    fn capture_splits_on_whitespace_by_default() {
        let t = capture("a b c\nd e f\n", None, None);
        assert_eq!(t.rows(), 2);
        assert_eq!(t.cols(), 3);
        assert_eq!(t.get(0, 1), Some("b"));
        assert_eq!(t.get(1, 2), Some("f"));
    }

    #[test]
    fn capture_with_explicit_delimiters() {
        let t = capture("a,b,c\nd,e\n", Some(","), Some("\n"));
        assert_eq!(t.rows(), 2);
        assert_eq!(t.cols(), 3); // widest row wins
        assert_eq!(t.get(1, 0), Some("d"));
        assert_eq!(t.get(1, 1), Some("e"));
        assert_eq!(t.get(1, 2), Some("")); // short row padded
    }

    #[test]
    fn release_joins_cells_back_to_text() {
        let mut t = Table::new(2, 2);
        t.set(0, 0, "a");
        t.set(0, 1, "b");
        t.set(1, 0, "c");
        t.set(1, 1, "d");
        assert_eq!(t.release(), "a b\nc d\n");
    }

    #[test]
    fn capture_then_release_round_trips_whitespace_text() {
        let text = "one two\nthree four\n";
        let t = capture(text, None, None);
        assert_eq!(t.release(), text);
    }

    // ---- generate-source --------------------------------------------------

    #[test]
    fn generate_html_source() {
        let mut t = Table::new(1, 2);
        t.set(0, 0, "a&b");
        t.set(0, 1, "c");
        let html = t.generate_source(SourceLang::Html);
        assert_eq!(
            html,
            "<table border=\"1\">\n  <tr>\n    <td>a&amp;b</td>\n    <td>c</td>\n  </tr>\n</table>\n"
        );
    }

    #[test]
    fn generate_latex_source() {
        let mut t = Table::new(1, 2);
        t.set(0, 0, "a_b");
        t.set(0, 1, "c");
        let tex = t.generate_source(SourceLang::Latex);
        assert_eq!(
            tex,
            "\\begin{tabular}{|l|l|}\n\\hline\na\\_b & c \\\\\n\\hline\n\\end{tabular}\n"
        );
    }

    #[test]
    fn generate_cals_source() {
        let mut t = Table::new(1, 1);
        t.set(0, 0, "x");
        let cals = t.generate_source(SourceLang::Cals);
        assert_eq!(
            cals,
            "<tgroup cols=\"1\">\n  <tbody>\n    <row>\n      <entry>x</entry>\n    </row>\n  </tbody>\n</tgroup>\n"
        );
    }

    // ---- query-dimension / insert-sequence -------------------------------

    #[test]
    fn query_dimension_reports_sizes() {
        let mut t = Table::new(2, 2);
        t.set(0, 0, "ab");
        let msg = t.query_dimension(0, 0);
        // rendered table is 2 cols (widths 2 and 1) -> "+----+---+" = 10 wide
        assert!(msg.contains("Dim: (2r, 2c)"), "{msg}");
        assert!(msg.contains("Total Cells: 4"), "{msg}");
        assert!(msg.contains("Cell: (2w"), "{msg}");
    }

    #[test]
    fn insert_sequence_increments_trailing_number() {
        let mut t = Table::new(1, 3);
        t.insert_sequence((0, 0), "a1", 3, 1, 1);
        assert_eq!(t.get(0, 0), Some("a1"));
        assert_eq!(t.get(0, 1), Some("a2"));
        assert_eq!(t.get(0, 2), Some("a3"));
    }

    #[test]
    fn insert_sequence_carries_and_steps() {
        let mut t = Table::new(1, 3);
        t.insert_sequence((0, 0), "9", 3, 2, 1);
        assert_eq!(t.get(0, 0), Some("9"));
        assert_eq!(t.get(0, 1), Some("11"));
        assert_eq!(t.get(0, 2), Some("13"));
    }

    #[test]
    fn insert_sequence_respects_interval() {
        let mut t = Table::new(1, 4);
        t.insert_sequence((0, 0), "0", 2, 1, 2); // fill every 2nd cell
        assert_eq!(t.get(0, 0), Some("0"));
        assert_eq!(t.get(0, 1), Some(""));
        assert_eq!(t.get(0, 2), Some("1"));
    }

    // ---- span / split -----------------------------------------------------

    #[test]
    fn span_cell_right_merges_content() {
        let mut t = Table::new(1, 2);
        t.set(0, 0, "a");
        t.set(0, 1, "b");
        t.span_cell_right(0, 0);
        assert_eq!(t.get(0, 0), Some("a b"));
        assert_eq!(t.get(0, 1), Some(""));
    }

    #[test]
    fn split_cell_horizontally_adds_column() {
        let mut t = Table::new(1, 1);
        t.set(0, 0, "abcd");
        t.split_cell_horizontally(0, 0, 2);
        assert_eq!(t.cols(), 2);
        assert_eq!(t.get(0, 0), Some("ab"));
        assert_eq!(t.get(0, 1), Some("cd"));
    }

    #[test]
    fn cell_at_position_maps_caret_to_cell() {
        let mut t = Table::new(2, 2);
        t.set(0, 0, "name");
        t.set(0, 1, "age");
        t.set(1, 0, "sam");
        t.set(1, 1, "42");
        let block = t.render();
        // block lines: 0 sep, 1 row0, 2 sep, 3 row1, 4 sep
        assert_eq!(cell_at_position(&block, 1, 2), Some((0, 0))); // inside "name"
        assert_eq!(cell_at_position(&block, 1, 9), Some((0, 1))); // inside "age"
        assert_eq!(cell_at_position(&block, 3, 2), Some((1, 0))); // inside "sam"
        assert_eq!(cell_at_position(&block, 3, 9), Some((1, 1))); // inside "42"
                                                                  // a caret on a border line clamps to the row below/above
        assert_eq!(cell_at_position(&block, 0, 2), Some((0, 0)));
    }

    #[test]
    fn split_cell_vertically_adds_row() {
        let mut t = Table::new(1, 1);
        t.set(0, 0, "abcd");
        t.split_cell_vertically(0, 0, 2);
        assert_eq!(t.rows(), 2);
        assert_eq!(t.get(0, 0), Some("ab"));
        assert_eq!(t.get(1, 0), Some("cd"));
    }
}
