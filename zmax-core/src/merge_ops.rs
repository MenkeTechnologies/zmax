//! Pure-Rust diff / three-way-merge / patch algorithms — the round-4 gap-fill
//! batch. `diff.rs` only exposes [`crate::diff::compare_ropes`], which produces a
//! ropey [`crate::Transaction`] for the undo engine; it has no *renderable* line
//! or word diff, no three-way merge, no unified-patch application, and no
//! conflict reconciliation. VCS-aware editors (GNU Emacs `smerge`/`ediff`, VS
//! Code, Neovim, Sublime, JetBrains, Zed and Helix) all ship these; this module
//! adds them as plain, editor-type-free functions over `&str`, so each is unit
//! tested in isolation and the command layer just wires them to the live buffer.
//!
//! Everything here shares one primitive — a Hunt–Szymanski / dynamic-programming
//! longest-common-subsequence over an arbitrary token type ([`lcs_pairs`]) — so
//! the line diff, word diff, three-way merge and the ⭐ token reconciler all rest
//! on the same, separately verified core.
//!
//! Line splitting is done with `split('\n')` and re-joined with `\n`, so a
//! merge / patch round-trips a buffer exactly (including a trailing newline,
//! which shows up as a final empty element).

use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Longest-common-subsequence core (shared by every algorithm below)
// ---------------------------------------------------------------------------

/// The matched `(a_index, b_index)` pairs of a longest common subsequence of
/// `a` and `b`, in increasing order on both indices. Runs in `O(n*m)` time and
/// space; the token type only needs [`PartialEq`], so the same routine backs the
/// line diff, the word diff and the merge.
pub fn lcs_pairs<T: PartialEq>(a: &[T], b: &[T]) -> Vec<(usize, usize)> {
    let (n, m) = (a.len(), b.len());
    // dp[i][j] = LCS length of a[i..] and b[j..].
    let mut dp = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i][j] = if a[i] == b[j] {
                dp[i + 1][j + 1] + 1
            } else {
                dp[i + 1][j].max(dp[i][j + 1])
            };
        }
    }
    let mut out = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if a[i] == b[j] {
            out.push((i, j));
            i += 1;
            j += 1;
        } else if dp[i + 1][j] >= dp[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    out
}

fn split_lines(s: &str) -> Vec<&str> {
    s.split('\n').collect()
}

// ---------------------------------------------------------------------------
// Line diff
// ---------------------------------------------------------------------------

/// One step of a line-granular diff (git `--word-diff=none`, VS Code inline
/// diff): a line that is unchanged, removed from `a`, or added in `b`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LineDiff {
    Equal(String),
    Delete(String),
    Insert(String),
}

/// Diff `a` against `b` at line granularity, emitting deletions before
/// insertions within each changed region so the result reads like a unified
/// diff body. Unlike [`crate::diff::compare_ropes`] this yields owned,
/// directly-renderable ops rather than a ropey transaction.
pub fn line_diff(a: &str, b: &str) -> Vec<LineDiff> {
    let (la, lb) = (split_lines(a), split_lines(b));
    let pairs = lcs_pairs(&la, &lb);
    let mut out = Vec::new();
    let (mut i, mut j) = (0, 0);
    for (pi, pj) in pairs
        .iter()
        .copied()
        .chain(std::iter::once((la.len(), lb.len())))
    {
        while i < pi {
            out.push(LineDiff::Delete(la[i].to_string()));
            i += 1;
        }
        while j < pj {
            out.push(LineDiff::Insert(lb[j].to_string()));
            j += 1;
        }
        if pi < la.len() {
            out.push(LineDiff::Equal(la[pi].to_string()));
            i = pi + 1;
            j = pj + 1;
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Word / token diff
// ---------------------------------------------------------------------------

/// One step of a token-granular diff (git `--word-diff`, Emacs `ediff` refine).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WordDiff {
    Equal(String),
    Delete(String),
    Insert(String),
}

/// Split `s` into diff tokens: each maximal run of identifier characters
/// (alphanumeric or `_`) is one token, each maximal run of whitespace is one
/// token, and every other character stands alone. Concatenating the tokens
/// reproduces `s` exactly, which is what lets the reconciler rebuild text.
fn tokenize(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = s.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_alphanumeric() || c == '_' {
            let mut w = String::new();
            while let Some(&c) = chars.peek() {
                if c.is_alphanumeric() || c == '_' {
                    w.push(c);
                    chars.next();
                } else {
                    break;
                }
            }
            out.push(w);
        } else if c.is_whitespace() {
            let mut w = String::new();
            while let Some(&c) = chars.peek() {
                if c.is_whitespace() {
                    w.push(c);
                    chars.next();
                } else {
                    break;
                }
            }
            out.push(w);
        } else {
            out.push(c.to_string());
            chars.next();
        }
    }
    out
}

/// Diff `a` against `b` at word/token granularity (the "word diff" the TODO in
/// `diff.rs` gestures at). Adjacent ops of the same kind are coalesced so a
/// changed phrase reads as one `Delete`/`Insert` pair rather than a token storm.
pub fn word_diff(a: &str, b: &str) -> Vec<WordDiff> {
    let (ta, tb) = (tokenize(a), tokenize(b));
    let pairs = lcs_pairs(&ta, &tb);
    let mut raw = Vec::new();
    let (mut i, mut j) = (0, 0);
    for (pi, pj) in pairs
        .iter()
        .copied()
        .chain(std::iter::once((ta.len(), tb.len())))
    {
        while i < pi {
            raw.push(WordDiff::Delete(ta[i].clone()));
            i += 1;
        }
        while j < pj {
            raw.push(WordDiff::Insert(tb[j].clone()));
            j += 1;
        }
        if pi < ta.len() {
            raw.push(WordDiff::Equal(ta[pi].clone()));
            i = pi + 1;
            j = pj + 1;
        }
    }
    coalesce(raw)
}

/// Merge consecutive same-kind [`WordDiff`] ops by concatenating their text.
fn coalesce(ops: Vec<WordDiff>) -> Vec<WordDiff> {
    let mut out: Vec<WordDiff> = Vec::with_capacity(ops.len());
    for op in ops {
        match (out.last_mut(), &op) {
            (Some(WordDiff::Equal(prev)), WordDiff::Equal(s))
            | (Some(WordDiff::Delete(prev)), WordDiff::Delete(s))
            | (Some(WordDiff::Insert(prev)), WordDiff::Insert(s)) => prev.push_str(s),
            _ => out.push(op),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Three-way merge (diff3)
// ---------------------------------------------------------------------------

/// The outcome of a [`three_way_merge`]: the merged text (with git-style diff3
/// conflict markers around any region that could not be reconciled) and the
/// number of conflict regions emitted (`0` == clean merge).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeResult {
    pub text: String,
    pub conflicts: usize,
}

/// Three-way (diff3) line merge of `ours` and `theirs` against a common
/// `base`, the algorithm behind `git merge` / Emacs `smerge-mode`. Regions where
/// only one side changed are taken automatically; regions where both sides made
/// the *same* change collapse to that change; regions where they diverge are
/// wrapped in git diff3 conflict markers (`<<<<<<< OURS` / `||||||| BASE` /
/// `======= ` / `>>>>>>> THEIRS`).
pub fn three_way_merge(base: &str, ours: &str, theirs: &str) -> MergeResult {
    let (o, a, b) = (split_lines(base), split_lines(ours), split_lines(theirs));

    // Base-line index -> index in ours / theirs, for lines on each LCS.
    let map_a: HashMap<usize, usize> = lcs_pairs(&o, &a).into_iter().collect();
    let map_b: HashMap<usize, usize> = lcs_pairs(&o, &b).into_iter().collect();

    // Sync points: base lines matched in *both* sides. Because each LCS is
    // monotonic, the base indices common to both maps are monotonic in a and b
    // too, giving aligned (bi, ai, ti) triples to anchor the merge.
    let mut syncs: Vec<(usize, usize, usize)> = (0..o.len())
        .filter_map(|bi| Some((bi, *map_a.get(&bi)?, *map_b.get(&bi)?)))
        .collect();
    syncs.sort_unstable();

    let mut out: Vec<String> = Vec::new();
    let mut conflicts = 0usize;
    let (mut bo, mut ao, mut to) = (0usize, 0usize, 0usize);

    let mut resolve = |os: &[&str], as_: &[&str], bs: &[&str], out: &mut Vec<String>| {
        if as_ == os {
            // ours unchanged -> take theirs
            out.extend(bs.iter().map(|s| s.to_string()));
        } else if bs == os {
            // theirs unchanged -> take ours
            out.extend(as_.iter().map(|s| s.to_string()));
        } else if as_ == bs {
            // same change on both sides
            out.extend(as_.iter().map(|s| s.to_string()));
        } else {
            conflicts += 1;
            out.push("<<<<<<< OURS".to_string());
            out.extend(as_.iter().map(|s| s.to_string()));
            out.push("||||||| BASE".to_string());
            out.extend(os.iter().map(|s| s.to_string()));
            out.push("=======".to_string());
            out.extend(bs.iter().map(|s| s.to_string()));
            out.push(">>>>>>> THEIRS".to_string());
        }
    };

    for (bi, ai, ti) in syncs {
        if bi > bo || ai > ao || ti > to {
            resolve(&o[bo..bi], &a[ao..ai], &b[to..ti], &mut out);
        }
        out.push(o[bi].to_string()); // the shared line itself
        bo = bi + 1;
        ao = ai + 1;
        to = ti + 1;
    }
    // Trailing region after the last sync point.
    if bo < o.len() || ao < a.len() || to < b.len() {
        resolve(&o[bo..], &a[ao..], &b[to..], &mut out);
    }

    MergeResult {
        text: out.join("\n"),
        conflicts,
    }
}

// ---------------------------------------------------------------------------
// Unified-diff parse + apply
// ---------------------------------------------------------------------------

/// A single line inside a unified-diff [`Hunk`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HunkLine {
    Context(String),
    Delete(String),
    Insert(String),
}

/// One `@@ -old_start,old_len +new_start,new_len @@` hunk of a unified diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    pub old_start: usize,
    pub old_len: usize,
    pub new_start: usize,
    pub new_len: usize,
    pub lines: Vec<HunkLine>,
}

/// Why [`apply_unified_diff`] refused to apply a patch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatchError {
    /// A context/deletion line did not match the original at that position.
    ContextMismatch { line: usize },
    /// A hunk pointed past the end of the original text.
    OutOfBounds { line: usize },
    /// Hunks were not in increasing, non-overlapping order.
    Overlap,
}

/// Parse the hunks of a unified diff. `---`/`+++`/`diff`/`index` file headers
/// and any other stray lines are ignored; only `@@` headers and the ` `/`-`/`+`
/// body lines are consumed. The leading marker character is stripped from each
/// body line. Tolerant of a bare `@@ -l +l @@` (length omitted, meaning `1`).
pub fn parse_unified_diff(diff: &str) -> Vec<Hunk> {
    let mut hunks = Vec::new();
    let mut cur: Option<Hunk> = None;
    for line in diff.split('\n') {
        if let Some(rest) = line.strip_prefix("@@") {
            if let Some(h) = cur.take() {
                hunks.push(h);
            }
            // rest looks like " -old[,len] +new[,len] @@ optional"
            let mut old = (0usize, 1usize);
            let mut new = (0usize, 1usize);
            for tok in rest.split_whitespace() {
                if let Some(t) = tok.strip_prefix('-') {
                    old = parse_range(t);
                } else if let Some(t) = tok.strip_prefix('+') {
                    new = parse_range(t);
                }
            }
            cur = Some(Hunk {
                old_start: old.0,
                old_len: old.1,
                new_start: new.0,
                new_len: new.1,
                lines: Vec::new(),
            });
        } else if let Some(h) = cur.as_mut() {
            if let Some(s) = line.strip_prefix(' ') {
                h.lines.push(HunkLine::Context(s.to_string()));
            } else if let Some(s) = line.strip_prefix('-') {
                h.lines.push(HunkLine::Delete(s.to_string()));
            } else if let Some(s) = line.strip_prefix('+') {
                h.lines.push(HunkLine::Insert(s.to_string()));
            } else if line.is_empty() {
                // A blank line in the body is a context line for an empty line.
                h.lines.push(HunkLine::Context(String::new()));
            }
            // anything else (e.g. "\ No newline at end of file") is ignored.
        }
    }
    if let Some(h) = cur.take() {
        hunks.push(h);
    }
    hunks
}

fn parse_range(t: &str) -> (usize, usize) {
    let mut it = t.split(',');
    let start = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let len = it.next().and_then(|s| s.parse().ok()).unwrap_or(1);
    (start, len)
}

/// Apply parsed [`Hunk`]s to `original`, returning the patched text. Context and
/// deletion lines are verified against the original (git `patch` semantics): a
/// mismatch, an out-of-range hunk, or out-of-order hunks all error rather than
/// silently corrupting the buffer.
pub fn apply_unified_diff(original: &str, hunks: &[Hunk]) -> Result<String, PatchError> {
    let orig = split_lines(original);
    let mut out: Vec<String> = Vec::new();
    let mut cur = 0usize; // 0-based cursor into `orig`

    for h in hunks {
        // Hunk headers are 1-based; a pure-insertion hunk uses old_start of the
        // line it follows, so clamp to `cur` when old_len == 0.
        let start = h.old_start.saturating_sub(1);
        if start < cur {
            return Err(PatchError::Overlap);
        }
        if start > orig.len() {
            return Err(PatchError::OutOfBounds { line: h.old_start });
        }
        while cur < start {
            out.push(orig[cur].to_string());
            cur += 1;
        }
        for hl in &h.lines {
            match hl {
                HunkLine::Context(s) => {
                    if cur >= orig.len() {
                        return Err(PatchError::OutOfBounds { line: cur + 1 });
                    }
                    if orig[cur] != s {
                        return Err(PatchError::ContextMismatch { line: cur + 1 });
                    }
                    out.push(s.clone());
                    cur += 1;
                }
                HunkLine::Delete(s) => {
                    if cur >= orig.len() {
                        return Err(PatchError::OutOfBounds { line: cur + 1 });
                    }
                    if orig[cur] != s {
                        return Err(PatchError::ContextMismatch { line: cur + 1 });
                    }
                    cur += 1; // dropped
                }
                HunkLine::Insert(s) => out.push(s.clone()),
            }
        }
    }
    while cur < orig.len() {
        out.push(orig[cur].to_string());
        cur += 1;
    }
    Ok(out.join("\n"))
}

// ---------------------------------------------------------------------------
// ⭐ zmax original — token-level conflict reconciliation
// ---------------------------------------------------------------------------

/// The token edits `other` makes relative to `base`, as
/// `(base_start, base_end, replacement_tokens)` gaps (half-open on the base
/// token stream). Identity gaps (replacement equals the base slice) are dropped.
fn token_edits(base: &[String], other: &[String]) -> Vec<(usize, usize, Vec<String>)> {
    let pairs = lcs_pairs(base, other);
    let mut edits = Vec::new();
    let (mut bi, mut oi) = (0usize, 0usize);
    for (pb, po) in pairs
        .iter()
        .copied()
        .chain(std::iter::once((base.len(), other.len())))
    {
        if pb > bi || po > oi {
            let repl = other[oi..po].to_vec();
            if base[bi..pb] != repl[..] {
                edits.push((bi, pb, repl));
            }
        }
        bi = pb + 1;
        oi = po + 1;
    }
    edits
}

/// ⭐ zmax original — beyond GNU Emacs `smerge`, `git merge`, VS Code, Neovim,
/// Sublime, JetBrains, Zed and Helix, all of which conflict at *line* (or at best
/// hunk) granularity: attempt to auto-resolve a diff3 conflict at *token*
/// granularity. When `ours` and `theirs` each edit `base` only in
/// non-overlapping token spans (e.g. one side renames a variable, the other
/// tweaks a comment on the same line), their edits are woven together into a
/// single clean result. Returns `None` when the two sides touch overlapping
/// tokens (a genuine conflict) or both insert at the same point with different
/// text — never a silent wrong merge.
pub fn reconcile_conflict(base: &str, ours: &str, theirs: &str) -> Option<String> {
    let bt = tokenize(base);
    let mut edits = token_edits(&bt, &tokenize(ours));
    edits.extend(token_edits(&bt, &tokenize(theirs)));
    edits.sort_by_key(|x| (x.0, x.1));
    // Drop edits both sides made identically (same span + replacement).
    edits.dedup();

    let mut out: Vec<String> = Vec::new();
    let mut cur = 0usize;
    let mut prev_end: Option<usize> = None;
    let mut prev_zero_at: Option<usize> = None;
    for (s, e, repl) in &edits {
        if let Some(pe) = prev_end {
            if *s < pe {
                return None; // overlapping base spans
            }
            // Two distinct insertions at the exact same point are ambiguous.
            if prev_zero_at == Some(*s) && *s == *e {
                return None;
            }
        }
        out.extend(bt[cur..*s].iter().cloned());
        out.extend(repl.iter().cloned());
        cur = *e;
        prev_end = Some(*e);
        prev_zero_at = if *s == *e { Some(*s) } else { None };
    }
    out.extend(bt[cur..].iter().cloned());
    Some(out.concat())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lcs_matches_are_monotonic() {
        let a = ["a", "b", "c", "d"];
        let b = ["b", "d", "e"];
        let pairs = lcs_pairs(&a, &b);
        assert_eq!(pairs, vec![(1, 0), (3, 1)]);
    }

    #[test]
    fn line_diff_reports_del_ins_equal() {
        let d = line_diff("one\ntwo\nthree", "one\nTWO\nthree");
        assert_eq!(
            d,
            vec![
                LineDiff::Equal("one".into()),
                LineDiff::Delete("two".into()),
                LineDiff::Insert("TWO".into()),
                LineDiff::Equal("three".into()),
            ]
        );
    }

    #[test]
    fn word_diff_coalesces_runs() {
        let d = word_diff("the quick brown fox", "the slow brown fox");
        assert_eq!(
            d,
            vec![
                WordDiff::Equal("the ".into()),
                WordDiff::Delete("quick".into()),
                WordDiff::Insert("slow".into()),
                WordDiff::Equal(" brown fox".into()),
            ]
        );
        // Concatenating deletes+equals reproduces the original.
        let orig: String = d
            .iter()
            .filter_map(|o| match o {
                WordDiff::Equal(s) | WordDiff::Delete(s) => Some(s.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(orig, "the quick brown fox");
    }

    #[test]
    fn merge_takes_one_sided_change() {
        // ours changes line 2, theirs untouched -> clean, take ours.
        let r = three_way_merge("a\nb\nc", "a\nB\nc", "a\nb\nc");
        assert_eq!(r.conflicts, 0);
        assert_eq!(r.text, "a\nB\nc");

        // theirs changes, ours untouched -> take theirs.
        let r = three_way_merge("a\nb\nc", "a\nb\nc", "a\nb\nC");
        assert_eq!(r.conflicts, 0);
        assert_eq!(r.text, "a\nb\nC");
    }

    #[test]
    fn merge_same_change_both_sides() {
        let r = three_way_merge("a\nb\nc", "a\nX\nc", "a\nX\nc");
        assert_eq!(r.conflicts, 0);
        assert_eq!(r.text, "a\nX\nc");
    }

    #[test]
    fn merge_disjoint_changes_are_clean() {
        // ours edits line 1, theirs edits line 3 -> both applied, no conflict.
        let r = three_way_merge("a\nb\nc", "A\nb\nc", "a\nb\nC");
        assert_eq!(r.conflicts, 0);
        assert_eq!(r.text, "A\nb\nC");
    }

    #[test]
    fn merge_reports_conflict() {
        let r = three_way_merge("a\nb\nc", "a\nOURS\nc", "a\nTHEIRS\nc");
        assert_eq!(r.conflicts, 1);
        assert!(r.text.contains("<<<<<<< OURS"));
        assert!(r.text.contains("||||||| BASE"));
        assert!(r.text.contains("======="));
        assert!(r.text.contains(">>>>>>> THEIRS"));
        assert!(r.text.contains("OURS"));
        assert!(r.text.contains("THEIRS"));
    }

    #[test]
    fn parse_and_apply_unified_diff() {
        let original = "line1\nline2\nline3\nline4";
        let diff = "\
--- a
+++ b
@@ -2,2 +2,2 @@
 line2
-line3
+LINE3
 line4";
        let hunks = parse_unified_diff(diff);
        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].old_start, 2);
        let patched = apply_unified_diff(original, &hunks).unwrap();
        assert_eq!(patched, "line1\nline2\nLINE3\nline4");
    }

    #[test]
    fn apply_detects_context_mismatch() {
        let hunks = parse_unified_diff("@@ -1,1 +1,1 @@\n-nope\n+yes");
        let err = apply_unified_diff("actual\nrest", &hunks).unwrap_err();
        assert_eq!(err, PatchError::ContextMismatch { line: 1 });
    }

    #[test]
    fn apply_pure_insertion_hunk() {
        // Insert a line after line 1 (old_len 0 style header with context).
        let hunks = parse_unified_diff("@@ -1,1 +1,2 @@\n a\n+inserted");
        let patched = apply_unified_diff("a\nb", &hunks).unwrap();
        assert_eq!(patched, "a\ninserted\nb");
    }

    #[test]
    fn apply_two_hunks_in_order() {
        let original = "1\n2\n3\n4\n5\n6";
        let diff = "@@ -1,1 +1,1 @@\n-1\n+ONE\n@@ -5,1 +5,1 @@\n-5\n+FIVE";
        let hunks = parse_unified_diff(diff);
        assert_eq!(hunks.len(), 2);
        let patched = apply_unified_diff(original, &hunks).unwrap();
        assert_eq!(patched, "ONE\n2\n3\n4\nFIVE\n6");
    }

    #[test]
    fn reconcile_disjoint_token_edits() {
        // ours renames `foo`->`bar`; theirs renames `x`->`y`; same line, disjoint.
        let merged = reconcile_conflict("foo = x + 1", "bar = x + 1", "foo = y + 1");
        assert_eq!(merged.as_deref(), Some("bar = y + 1"));
    }

    #[test]
    fn reconcile_same_edit_both_sides() {
        let merged = reconcile_conflict("foo = 1", "bar = 1", "bar = 1");
        assert_eq!(merged.as_deref(), Some("bar = 1"));
    }

    #[test]
    fn reconcile_overlapping_edits_conflict() {
        // Both sides rewrite the same token differently -> None.
        let merged = reconcile_conflict("value = 1", "value = 2", "value = 3");
        assert_eq!(merged, None);
    }

    #[test]
    fn reconcile_no_change_returns_base() {
        let merged = reconcile_conflict("a b c", "a b c", "a b c");
        assert_eq!(merged.as_deref(), Some("a b c"));
    }
}
