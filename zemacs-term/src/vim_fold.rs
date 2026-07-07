//! vim `foldmethod` fold-range computation. Pure functions over the buffer's
//! lines / per-line indent levels, returning inclusive `(start_line, end_line)`
//! fold ranges to hand to `zemacs_core::fold::Folds::create`. The command layer
//! (`commands::apply_foldmethod`) reads the document, calls one of these, and
//! rebuilds the document's folds.

/// vim `foldmethod=marker`: fold ranges delimited by the open/close markers
/// (default `{{{` / `}}}`, from `foldmarker`). Markers may carry a level digit
/// (`{{{1`) which is ignored here — nesting is derived from marker pairing. A
/// close with no matching open is ignored; unclosed opens fold to the last line.
pub fn marker_fold_ranges(lines: &[&str], open: &str, close: &str) -> Vec<(usize, usize)> {
    let mut stack: Vec<usize> = Vec::new();
    let mut out: Vec<(usize, usize)> = Vec::new();
    let last = lines.len().saturating_sub(1);
    for (i, line) in lines.iter().enumerate() {
        // An open marker on this line begins a fold here.
        if line.contains(open) {
            stack.push(i);
        }
        // A close marker ends the innermost open fold on this line.
        if line.contains(close) {
            if let Some(start) = stack.pop() {
                if i > start {
                    out.push((start, i));
                }
            }
        }
    }
    // Unclosed markers fold to the end of the buffer (vim behavior).
    while let Some(start) = stack.pop() {
        if last > start {
            out.push((start, last));
        }
    }
    out.sort_unstable();
    out
}

/// vim `foldmethod=indent`: a fold begins wherever the next line is more deeply
/// indented and spans the run of following lines at least that deep, so each
/// increase in indent level opens a (possibly nested) fold. `levels[i]` is line
/// `i`'s fold level (indent columns / `shiftwidth`, blank lines carrying the
/// previous line's level — computed by the caller).
pub fn indent_fold_ranges(levels: &[usize]) -> Vec<(usize, usize)> {
    let n = levels.len();
    let mut out = Vec::new();
    for i in 0..n.saturating_sub(1) {
        if levels[i + 1] > levels[i] {
            let target = levels[i] + 1;
            let mut j = i + 1;
            while j < n && levels[j] >= target {
                j += 1;
            }
            // Header line `i` plus the deeper body `i+1..j`.
            out.push((i, j - 1));
        }
    }
    out
}

/// Fold levels for `foldmethod=indent`: each line's indent width in columns
/// (tabs counted as `tab_width`) divided by `shiftwidth`, with blank lines
/// inheriting the previous line's level so they stay inside the enclosing fold.
pub fn indent_levels(lines: &[&str], tab_width: usize, shiftwidth: usize) -> Vec<usize> {
    let sw = shiftwidth.max(1);
    let mut levels = Vec::with_capacity(lines.len());
    let mut prev = 0;
    for line in lines {
        if line.trim().is_empty() {
            levels.push(prev);
            continue;
        }
        let mut cols = 0;
        for ch in line.chars() {
            match ch {
                ' ' => cols += 1,
                '\t' => cols += tab_width.max(1),
                _ => break,
            }
        }
        let level = cols / sw;
        levels.push(level);
        prev = level;
    }
    levels
}

/// Apply vim `foldminlines` (drop folds that span fewer than `min_lines` lines)
/// and `foldnestmax` (drop folds nested deeper than `max_nest` levels). Ranges
/// are inclusive `(start, end)` line pairs; nesting depth counts how many other
/// ranges strictly contain a fold.
pub fn filter_folds(
    ranges: Vec<(usize, usize)>,
    min_lines: usize,
    max_nest: usize,
) -> Vec<(usize, usize)> {
    let min_lines = min_lines.max(1);
    let kept: Vec<(usize, usize)> = ranges
        .into_iter()
        .filter(|(s, e)| e - s + 1 >= min_lines)
        .collect();
    kept.iter()
        .filter(|r| {
            let depth = kept
                .iter()
                .filter(|o| o.0 <= r.0 && o.1 >= r.1 && (o.0 < r.0 || o.1 > r.1))
                .count();
            depth < max_nest.max(1)
        })
        .copied()
        .collect()
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn filters_by_min_lines_and_nesting() {
        // (0,5) depth0, (2,4) depth1, (3,3) depth2 single-line.
        let folds = vec![(0, 5), (2, 4), (3, 3)];
        // min_lines=2 drops the single-line (3,3).
        assert_eq!(filter_folds(folds.clone(), 2, 20), vec![(0, 5), (2, 4)]);
        // max_nest=2 keeps depths 0,1 and drops depth 2.
        assert_eq!(filter_folds(folds.clone(), 1, 2), vec![(0, 5), (2, 4)]);
        // max_nest=1 keeps only the outermost.
        assert_eq!(filter_folds(folds, 1, 1), vec![(0, 5)]);
    }

    #[test]
    fn markers_pair_and_nest() {
        let lines = vec![
            "fn a() { // {{{",
            "  inner // {{{",
            "  // }}}",
            "} // }}}",
            "top",
        ];
        // outer fold 0..3, inner fold 1..2.
        assert_eq!(
            marker_fold_ranges(&lines, "{{{", "}}}"),
            vec![(0, 3), (1, 2)]
        );
    }

    #[test]
    fn unclosed_marker_folds_to_end() {
        let lines = vec!["a // {{{", "b", "c"];
        assert_eq!(marker_fold_ranges(&lines, "{{{", "}}}"), vec![(0, 2)]);
    }

    #[test]
    fn indent_levels_and_ranges() {
        let lines = vec![
            "def f():",     // 0 -> level 0
            "    a = 1",    // 4 -> level 1
            "    if a:",    // 4 -> level 1
            "        b = 2", // 8 -> level 2
            "",             // blank -> inherits 2
            "    c = 3",    // 4 -> level 1
            "back",         // 0 -> level 0
        ];
        let levels = indent_levels(&lines, 8, 4);
        assert_eq!(levels, vec![0, 1, 1, 2, 2, 1, 0]);
        // level rises 0->1 at line 0 (body 1..5), and 1->2 at line 2 (body 3..4).
        assert_eq!(indent_fold_ranges(&levels), vec![(0, 5), (2, 4)]);
    }
}
