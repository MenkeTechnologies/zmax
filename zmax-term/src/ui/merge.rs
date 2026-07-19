//! Interactive 3-pane merge viewer (slice 2 of a JetBrains-style diff/merge
//! tool, built on the slice-1 side-by-side alignment).
//!
//! A full-screen overlay [`Component`] that shows the focused buffer's git diff
//! as three vertically-aligned panes: the file's `HEAD` version on the
//! **left**, a live **Result** in the **center**, and the current working-tree
//! buffer on the **right**. Opened with the `:diff` typable command.
//!
//! The alignment is computed once up front from a line-level [`imara_diff`]
//! diff between the two texts (see [`align`]). Each [`DiffRow`] pairs an
//! optional left line with an optional right line; changed regions pair old
//! lines against new lines and pad the shorter side with blank rows so all
//! panes stay in lock-step as you scroll.
//!
//! Contiguous runs of changed rows become [`Block`]s, each with a
//! [`Resolution`] (`Left` = take HEAD, `Right` = keep working tree). The
//! center Result pane is recomputed every frame from the per-block
//! resolutions. `Enter` writes the resolved text back into the document as a
//! single undoable transaction.
//!
//! Keys: `j`/`k`/arrows scroll a row, PageUp/PageDown (`ctrl-d`/`ctrl-u`) a
//! screenful, `g`/`G` jump to top/bottom, `n`/`p` move the selected block,
//! `,`/`[`/`h` take HEAD, `.`/`]`/`l` take working, `L`/`R` resolve all,
//! `Enter`/`a` apply, `q`/`Esc` cancel. Mouse wheel scrolls too.

use std::ops::Range;
use std::path::PathBuf;

use imara_diff::{sources::lines, Algorithm, Diff, InternedInput};

use tui::buffer::Buffer as Surface;
use zmax_view::graphics::{Rect, Style};
use zmax_view::input::MouseEventKind;
use zmax_view::DocumentId;

use crate::{
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

/// What a single aligned row represents.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum RowKind {
    /// Identical on both sides.
    Unchanged,
    /// Present only on the right (working tree) — an inserted line.
    Added,
    /// Present only on the left (HEAD) — a deleted line.
    Removed,
    /// A modified line: old text on the left, new text on the right.
    Changed,
}

/// One vertically-aligned row of the side-by-side view. `left`/`right` index
/// into [`DiffView::base_lines`] / [`DiffView::doc_lines`]; `None` means that
/// side is a blank filler so the other side's change stays aligned.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct DiffRow {
    left: Option<usize>,
    right: Option<usize>,
    kind: RowKind,
}

/// Build the aligned row list for two texts.
///
/// Pure and unit-tested. Lines are tokenised exactly as `imara_diff` sees them
/// (see [`split_lines`]) so the line indices stored in each [`DiffRow`] line up
/// with the displayed line vectors.
fn align(base: &str, doc: &str) -> Vec<DiffRow> {
    let n_base = split_lines(base).len() as u32;
    let n_doc = split_lines(doc).len() as u32;

    // vim 'diffexpr': when the option names an expression, the hunks are the ones
    // *it* produced (run by the command that opened this view, where the vimlrs
    // context is live). Otherwise zmax diffs the two texts itself, honouring vim
    // 'diffopt': the flags that change what counts as a difference
    // (`iwhite`/`iwhiteall`/`iwhiteeol`, `icase`) replace each line by its
    // comparison key, so the line *count* — and every row index below — is
    // unchanged; the view still displays the original lines.
    let hunks: Vec<(std::ops::Range<u32>, std::ops::Range<u32>)> =
        match crate::commands::typed::diffexpr_hunks(base, doc) {
            Some(hunks) => hunks
                .into_iter()
                .map(|h| (h.before_start..h.before_end, h.after_start..h.after_end))
                .collect(),
            None => {
                let base_key = crate::commands::typed::diffopt_normalize(base);
                let doc_key = crate::commands::typed::diffopt_normalize(doc);
                let input = InternedInput::new(lines(&base_key), lines(&doc_key));
                let diff = Diff::compute(Algorithm::Histogram, &input);
                diff.hunks().map(|h| (h.before, h.after)).collect()
            }
        };

    let mut rows = Vec::new();
    let mut b = 0u32; // next un-emitted base (HEAD) line
    let mut d = 0u32; // next un-emitted doc (working) line

    for (before, after) in hunks {
        // Unchanged region between the previous hunk and this one: paired rows.
        while b < before.start {
            rows.push(DiffRow {
                left: Some(b as usize),
                right: Some(d as usize),
                kind: RowKind::Unchanged,
            });
            b += 1;
            d += 1;
        }

        // The hunk itself. Pair the overlapping span as `Changed`, then spill
        // the longer side into pure `Removed` / `Added` rows.
        let removed = before.end.saturating_sub(before.start);
        let added = after.end.saturating_sub(after.start);
        let common = removed.min(added);
        for _ in 0..common {
            rows.push(DiffRow {
                left: Some(b as usize),
                right: Some(d as usize),
                kind: RowKind::Changed,
            });
            b += 1;
            d += 1;
        }
        while b < before.end {
            rows.push(DiffRow {
                left: Some(b as usize),
                right: None,
                kind: RowKind::Removed,
            });
            b += 1;
        }
        while d < after.end {
            rows.push(DiffRow {
                left: None,
                right: Some(d as usize),
                kind: RowKind::Added,
            });
            d += 1;
        }
    }

    // Trailing unchanged tail. Both sides advance together.
    while b < n_base && d < n_doc {
        rows.push(DiffRow {
            left: Some(b as usize),
            right: Some(d as usize),
            kind: RowKind::Unchanged,
        });
        b += 1;
        d += 1;
    }

    rows
}

/// Split text into lines the same way `imara_diff::sources::lines` tokenises
/// it: one entry per line, trailing newline stripped, no phantom final entry.
fn split_lines(text: &str) -> Vec<String> {
    lines(text)
        .map(|l| l.strip_suffix('\n').unwrap_or(l))
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .map(str::to_string)
        .collect()
}

/// Row indices at which a contiguous run of changed/added/removed rows begins.
fn change_blocks(rows: &[DiffRow]) -> Vec<usize> {
    let mut blocks = Vec::new();
    let mut prev_changed = false;
    for (i, row) in rows.iter().enumerate() {
        let changed = row.kind != RowKind::Unchanged;
        if changed && !prev_changed {
            blocks.push(i);
        }
        prev_changed = changed;
    }
    blocks
}

/// Which side a change block resolves to in the Result.
///
/// In **diff** mode only `Left`/`Right` are used (slice 2). In **conflict**
/// mode all four apply: `Left` = take ours, `Right` = take theirs, `Both` =
/// ours then theirs, `None` = leave the region unresolved (markers preserved).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Resolution {
    /// Take the HEAD / ours (left) side — reverts the hunk.
    Left,
    /// Keep the working-tree / theirs (right) side — the diff-mode default.
    Right,
    /// Conflict mode: emit ours then theirs.
    Both,
    /// Conflict mode: leave the conflict unresolved (re-emit the markers). The
    /// default for a freshly-loaded conflict block.
    None,
}

/// Whether the view is showing a working-tree diff (slice 2) or resolving git
/// merge-conflict markers (slice 3). Controls labels, the default resolution,
/// the header text, the available keys and the Apply behaviour.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ViewKind {
    /// Working-tree diff against HEAD (`:diff`).
    Diff,
    /// Merge-conflict resolver over `<<<<<<< ======= >>>>>>>` markers (`:merge`).
    Conflict,
}

/// A parsed region of a conflicted file: either already-merged context lines or
/// an unresolved conflict with its ours/base/theirs sides (lines have their
/// trailing newline stripped). `base` is empty unless the file was produced
/// with `merge.conflictStyle=diff3`/`zdiff3` (the `|||||||` section).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Segment {
    /// Lines outside any conflict — already merged, shown in all three panes.
    Context(Vec<String>),
    /// One `<<<<<<< … >>>>>>>` region.
    Conflict {
        ours: Vec<String>,
        base: Vec<String>,
        theirs: Vec<String>,
    },
}

/// Parse git merge-conflict markers out of `text`.
///
/// Returns `None` when the text contains no `<<<<<<<` marker (so the caller can
/// report "no conflicts"). Otherwise returns the file split into ordered
/// [`Segment`]s. Pure and unit-tested.
///
/// Handles multiple conflicts, the optional `|||||||` base section, CRLF line
/// endings, a missing trailing newline, and markers that are exactly
/// `<<<<<<<`/`|||||||`/`=======`/`>>>>>>>` optionally followed by a label.
/// Nested conflicts are not handled (git never produces them).
pub fn parse_conflicts(text: &str) -> Option<Vec<Segment>> {
    // `split_lines` strips trailing newlines and `\r`, drops the phantom final
    // entry, and matches how the rest of the module tokenises text — so marker
    // detection works uniformly for LF/CRLF and for a missing final newline.
    let lines = split_lines(text);
    if !lines.iter().any(|l| l.starts_with("<<<<<<<")) {
        return None;
    }

    let mut segments = Vec::new();
    let mut context: Vec<String> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if !lines[i].starts_with("<<<<<<<") {
            context.push(lines[i].clone());
            i += 1;
            continue;
        }

        // Flush any pending context before starting a conflict.
        if !context.is_empty() {
            segments.push(Segment::Context(std::mem::take(&mut context)));
        }

        let mut ours = Vec::new();
        let mut base = Vec::new();
        let mut theirs = Vec::new();
        i += 1; // skip the `<<<<<<<` marker line

        // ours: up to `|||||||`, `=======` or (defensively) `>>>>>>>`.
        while i < lines.len()
            && !lines[i].starts_with("|||||||")
            && !lines[i].starts_with("=======")
            && !lines[i].starts_with(">>>>>>>")
        {
            ours.push(lines[i].clone());
            i += 1;
        }
        // optional diff3 base section.
        if i < lines.len() && lines[i].starts_with("|||||||") {
            i += 1;
            while i < lines.len()
                && !lines[i].starts_with("=======")
                && !lines[i].starts_with(">>>>>>>")
            {
                base.push(lines[i].clone());
                i += 1;
            }
        }
        // theirs: between `=======` and `>>>>>>>`.
        if i < lines.len() && lines[i].starts_with("=======") {
            i += 1;
            while i < lines.len() && !lines[i].starts_with(">>>>>>>") {
                theirs.push(lines[i].clone());
                i += 1;
            }
        }
        // closing marker.
        if i < lines.len() && lines[i].starts_with(">>>>>>>") {
            i += 1;
        }

        segments.push(Segment::Conflict { ours, base, theirs });
    }
    if !context.is_empty() {
        segments.push(Segment::Context(context));
    }
    Some(segments)
}

/// For each line of `base`, the index of the corresponding line in `other` if
/// that base line is **unchanged** between `base` and `other`, else `None`.
///
/// Built from the same line-level [`align`] used everywhere else, so the
/// "unchanged" notion matches the rest of the module. The mapping is monotonic
/// (later base lines map to later `other` lines) because the diff is.
fn base_match_map(base: &str, other: &str) -> Vec<Option<usize>> {
    let n_base = split_lines(base).len();
    let mut map = vec![None; n_base];
    for row in align(base, other) {
        if row.kind == RowKind::Unchanged {
            if let (Some(b), Some(o)) = (row.left, row.right) {
                map[b] = Some(o);
            }
        }
    }
    map
}

/// Resolve one non-stable region of a 3-way merge into either auto-merged
/// context (appended to `ctx`) or a real [`Segment::Conflict`] (which first
/// flushes the pending context). The classic diff3 decision:
///
/// * ours == base → ours didn't touch it, take **theirs**.
/// * theirs == base → theirs didn't touch it, take **ours**.
/// * ours == theirs → both made the same edit, take it.
/// * otherwise → a genuine conflict carrying the real per-region `base`.
fn resolve_region(
    segments: &mut Vec<Segment>,
    ctx: &mut Vec<String>,
    ours: Vec<String>,
    base: Vec<String>,
    theirs: Vec<String>,
) {
    if ours == base {
        // ours didn't touch it → take theirs.
        ctx.extend(theirs);
    } else if theirs == base || ours == theirs {
        // theirs didn't touch it, or both made the same edit → take ours.
        ctx.extend(ours);
    } else {
        if !ctx.is_empty() {
            segments.push(Segment::Context(std::mem::take(ctx)));
        }
        segments.push(Segment::Conflict { ours, base, theirs });
    }
}

/// Three-way merge of `ours`/`theirs` against their common ancestor `base`,
/// producing ordered [`Segment`]s. Pure and unit-tested.
///
/// Uses two line-level 2-way diffs — base↔ours and base↔theirs (via
/// [`base_match_map`]) — and the classic diff3 algorithm: base lines that are
/// unchanged in **both** sides are *stable* and become context; the regions
/// between stable lines are resolved by [`resolve_region`]. Net effect:
/// non-conflicting edits from either side are auto-merged into [`Segment::Context`];
/// only genuinely overlapping edits become [`Segment::Conflict`], each carrying
/// the real common-ancestor `base` for that region. Adjacent context runs are
/// coalesced into a single segment.
pub fn diff3(base: &str, ours: &str, theirs: &str) -> Vec<Segment> {
    let base_lines = split_lines(base);
    let ours_lines = split_lines(ours);
    let theirs_lines = split_lines(theirs);
    let n_base = base_lines.len();
    let n_ours = ours_lines.len();
    let n_theirs = theirs_lines.len();

    let ours_match = base_match_map(base, ours);
    let theirs_match = base_match_map(base, theirs);

    let mut segments: Vec<Segment> = Vec::new();
    let mut ctx: Vec<String> = Vec::new();

    // Cursors into base / ours / theirs marking the start of the not-yet-emitted
    // region. A base line is *stable* when it is unchanged in both sides.
    let (mut b, mut o, mut t) = (0usize, 0usize, 0usize);
    let mut bi = 0usize;
    while bi < n_base {
        let (Some(oi), Some(ti)) = (ours_match[bi], theirs_match[bi]) else {
            bi += 1;
            continue;
        };
        // Resolve the region preceding this stable line.
        resolve_region(
            &mut segments,
            &mut ctx,
            ours_lines[o..oi].to_vec(),
            base_lines[b..bi].to_vec(),
            theirs_lines[t..ti].to_vec(),
        );
        // Emit the maximal run of stable lines that are also consecutive in all
        // three sequences (stable lines are identical across base/ours/theirs).
        let (mut cb, mut co, mut ct) = (bi, oi, ti);
        loop {
            ctx.push(base_lines[cb].clone());
            cb += 1;
            co += 1;
            ct += 1;
            if cb < n_base && ours_match[cb] == Some(co) && theirs_match[cb] == Some(ct) {
                continue;
            }
            break;
        }
        b = cb;
        o = co;
        t = ct;
        bi = cb;
    }
    // Trailing region after the last stable line.
    resolve_region(
        &mut segments,
        &mut ctx,
        ours_lines[o..n_ours].to_vec(),
        base_lines[b..n_base].to_vec(),
        theirs_lines[t..n_theirs].to_vec(),
    );
    if !ctx.is_empty() {
        segments.push(Segment::Context(ctx));
    }
    segments
}

/// Reconstruct the resolved text of a conflicted file from its parsed
/// [`Segment`]s and the per-conflict resolutions held in `blocks` (one block
/// per [`Segment::Conflict`], in order). Pure so it can be unit-tested.
///
/// `Left`→ours, `Right`→theirs, `Both`→ours then theirs, `None`→ the original
/// conflict region with its markers preserved (so a partial resolution leaves
/// the rest conflicted).
fn conflict_result_text(segments: &[Segment], blocks: &[Block]) -> String {
    fn emit(out: &mut String, lines: &[String]) {
        for line in lines {
            out.push_str(line);
            out.push('\n');
        }
    }

    let mut out = String::new();
    let mut conflicts = blocks.iter();
    for seg in segments {
        match seg {
            Segment::Context(lines) => emit(&mut out, lines),
            Segment::Conflict { ours, base, theirs } => {
                let res = conflicts
                    .next()
                    .map(|b| b.resolution)
                    .unwrap_or(Resolution::None);
                match res {
                    Resolution::Left => emit(&mut out, ours),
                    Resolution::Right => emit(&mut out, theirs),
                    Resolution::Both => {
                        emit(&mut out, ours);
                        emit(&mut out, theirs);
                    }
                    Resolution::None => {
                        out.push_str("<<<<<<<\n");
                        emit(&mut out, ours);
                        if !base.is_empty() {
                            out.push_str("|||||||\n");
                            emit(&mut out, base);
                        }
                        out.push_str("=======\n");
                        emit(&mut out, theirs);
                        out.push_str(">>>>>>>\n");
                    }
                }
            }
        }
    }
    out
}

/// A contiguous run of changed rows together with its chosen resolution.
#[derive(Clone, Debug)]
struct Block {
    /// Half-open range of row indices (into `DiffView::rows`) the block covers.
    rows: Range<usize>,
    /// Which side this block contributes to the Result.
    resolution: Resolution,
}

/// Turn the aligned rows into change blocks (contiguous runs of non-unchanged
/// rows), each defaulting to [`Resolution::Right`] so the Result initially
/// equals the working tree. Pure — built on [`change_blocks`].
fn compute_blocks(rows: &[DiffRow]) -> Vec<Block> {
    change_blocks(rows)
        .into_iter()
        .map(|start| {
            let mut end = start;
            while end < rows.len() && rows[end].kind != RowKind::Unchanged {
                end += 1;
            }
            Block {
                rows: start..end,
                resolution: Resolution::Right,
            }
        })
        .collect()
}

/// Compute the resolved Result text from the alignment + per-block
/// resolutions. Pure (no editor state) so it can be unit-tested.
///
/// Walks `rows` in order: unchanged rows emit their (identical) line; rows
/// inside a block emit the chosen side's *actual* line and skip padded blanks
/// (`None`). Each emitted line is newline-terminated.
fn result_text(
    rows: &[DiffRow],
    blocks: &[Block],
    base_lines: &[String],
    doc_lines: &[String],
) -> String {
    // Per-row resolution, `None` for unchanged rows outside any block.
    let mut row_res: Vec<Option<Resolution>> = vec![None; rows.len()];
    for block in blocks {
        for i in block.rows.clone() {
            row_res[i] = Some(block.resolution);
        }
    }

    let mut out = String::new();
    for (i, row) in rows.iter().enumerate() {
        match row_res[i] {
            // Unchanged: both sides hold the same line; use the working tree.
            None => {
                if let Some(r) = row.right.and_then(|r| doc_lines.get(r)) {
                    out.push_str(r);
                    out.push('\n');
                } else if let Some(l) = row.left.and_then(|l| base_lines.get(l)) {
                    out.push_str(l);
                    out.push('\n');
                }
            }
            Some(Resolution::Left) => {
                if let Some(l) = row.left.and_then(|l| base_lines.get(l)) {
                    out.push_str(l);
                    out.push('\n');
                }
            }
            // `Right` is the diff-mode default; `Both`/`None` never occur in
            // diff mode (conflict mode uses `conflict_result_text`) but keep the
            // match exhaustive by treating them as "keep the right side".
            Some(Resolution::Right) | Some(Resolution::Both) | Some(Resolution::None) => {
                if let Some(r) = row.right.and_then(|r| doc_lines.get(r)) {
                    out.push_str(r);
                    out.push('\n');
                }
            }
        }
    }
    out
}

/// The full-screen interactive 3-pane merge overlay.
pub struct DiffView {
    /// Whether this is a working-tree diff or a conflict resolver.
    kind: ViewKind,
    /// Display name of the file being diffed (shown in the header).
    file_name: String,
    /// Document the resolved Result is written back into on Apply.
    doc_id: DocumentId,
    /// Absolute path of the document, captured up front so Apply can write the
    /// resolved file to disk and `git add` it (conflict mode only).
    path: Option<PathBuf>,
    /// HEAD / ours lines (left pane), trailing newline stripped.
    base_lines: Vec<String>,
    /// Working-tree / theirs lines (right pane), trailing newline stripped.
    doc_lines: Vec<String>,
    /// Conflict mode only: the common-ancestor (diff3) **base** lines, shown in
    /// the optional Base pane. Indexed by [`DiffView::row_base`]. Empty in diff
    /// mode and for conflicts that have no recorded base.
    base_pane_lines: Vec<String>,
    /// Per-row index into [`DiffView::base_pane_lines`] for the Base pane, or
    /// `None` for a blank filler on that row. Always the same length as `rows`.
    row_base: Vec<Option<usize>>,
    /// Whether the Base pane is currently shown (toggled with `B`). Defaults to
    /// `true` when any conflict carries a non-empty base, else `false`.
    show_base: bool,
    /// True when at least one conflict has a non-empty base (so the Base pane is
    /// meaningful and the `B` toggle / 4-pane layout are offered).
    has_base: bool,
    rows: Vec<DiffRow>,
    /// Change blocks with their (mutable) per-block resolution.
    blocks: Vec<Block>,
    /// Conflict mode only: the parsed segments, used to rebuild the resolved
    /// text (with markers preserved for unresolved blocks). Empty in diff mode.
    segments: Vec<Segment>,
    /// Index into `blocks` of the currently-focused block.
    selected: usize,
    /// Index of the top visible row.
    scroll: usize,
    /// Horizontal scroll offset in display columns, applied to every pane's
    /// content (the line-number gutter stays fixed). Clamped to the longest
    /// line's width by [`DiffView::hscroll_by`].
    hscroll: usize,
    /// Number of body rows visible in the last render (for page scrolling).
    viewport: usize,
    /// When true (external-file / multi-file ediff comparison), `Apply` never
    /// writes back to any document or disk — the view is comparison-only, so it
    /// can safely diff arbitrary files without risking the current buffer.
    read_only: bool,
    /// Emerge `emerge-auto-advance`: choosing a side for a difference moves on to
    /// the next difference by itself.
    auto_advance: bool,
    /// Emerge `emerge-skip-prefers`: stepping between differences skips the ones
    /// that already have a side chosen.
    skip_prefers: bool,
    /// `ediff-regions-wordwise`: refine a changed row at emacs's word
    /// granularity instead of per character.
    word_refine: bool,
}

impl DiffView {
    /// Construct a viewer from the HEAD text and the current buffer text.
    /// `doc_id` is the document the resolved Result is applied to.
    pub fn new(file_name: String, doc_id: DocumentId, base: &str, doc: &str) -> Self {
        let rows = align(base, doc);
        let blocks = compute_blocks(&rows);
        let row_base = vec![None; rows.len()];
        DiffView {
            kind: ViewKind::Diff,
            file_name,
            doc_id,
            path: None,
            base_lines: split_lines(base),
            doc_lines: split_lines(doc),
            base_pane_lines: Vec::new(),
            row_base,
            show_base: false,
            has_base: false,
            rows,
            blocks,
            segments: Vec::new(),
            selected: 0,
            scroll: 0,
            hscroll: 0,
            viewport: 1,
            read_only: false,
            auto_advance: false,
            skip_prefers: false,
            word_refine: false,
        }
    }

    /// Construct a conflict resolver from the parsed [`Segment`]s of a
    /// conflicted file. Context lines become `Unchanged` rows shown in all
    /// three panes; each conflict becomes a [`Block`] (one per conflict, in
    /// order) whose left side is the *ours* lines and right side the *theirs*
    /// lines, padded so the panes stay aligned — exactly like [`align`]. Blocks
    /// default to [`Resolution::None`] (unresolved). `path` is the document's
    /// absolute path, captured for the `git add` step on Apply.
    /// Mark this view comparison-only: `Apply` will never write back. Use for
    /// ediff of arbitrary external files so it can't clobber the current buffer.
    pub fn read_only(mut self) -> Self {
        self.read_only = true;
        self
    }

    /// `ediff-regions-wordwise`: keep showing the regions as they are written,
    /// but refine each changed row at emacs's word granularity so only the
    /// differing words are marked.
    pub fn wordwise(mut self) -> Self {
        self.word_refine = true;
        self
    }

    pub fn from_conflicts(
        file_name: String,
        doc_id: DocumentId,
        path: Option<PathBuf>,
        segments: Vec<Segment>,
    ) -> Self {
        let mut base_lines = Vec::new();
        let mut doc_lines = Vec::new();
        let mut base_pane_lines: Vec<String> = Vec::new();
        let mut row_base: Vec<Option<usize>> = Vec::new();
        let mut rows = Vec::new();
        let mut blocks = Vec::new();
        let mut has_base = false;

        for seg in &segments {
            match seg {
                Segment::Context(lines) => {
                    for line in lines {
                        let (bi, di) = (base_lines.len(), doc_lines.len());
                        base_lines.push(line.clone());
                        doc_lines.push(line.clone());
                        rows.push(DiffRow {
                            left: Some(bi),
                            right: Some(di),
                            kind: RowKind::Unchanged,
                        });
                        // Context is common to all sides, so the Base pane shows
                        // the same line.
                        let pbi = base_pane_lines.len();
                        base_pane_lines.push(line.clone());
                        row_base.push(Some(pbi));
                    }
                }
                Segment::Conflict { ours, base, theirs } => {
                    if !base.is_empty() {
                        has_base = true;
                    }
                    let start = rows.len();
                    let common = ours.len().min(theirs.len());
                    for k in 0..common {
                        let (bi, di) = (base_lines.len(), doc_lines.len());
                        base_lines.push(ours[k].clone());
                        doc_lines.push(theirs[k].clone());
                        rows.push(DiffRow {
                            left: Some(bi),
                            right: Some(di),
                            kind: RowKind::Changed,
                        });
                        row_base.push(None);
                    }
                    for line in &ours[common..] {
                        let bi = base_lines.len();
                        base_lines.push(line.clone());
                        rows.push(DiffRow {
                            left: Some(bi),
                            right: None,
                            kind: RowKind::Removed,
                        });
                        row_base.push(None);
                    }
                    for line in &theirs[common..] {
                        let di = doc_lines.len();
                        doc_lines.push(line.clone());
                        rows.push(DiffRow {
                            left: None,
                            right: Some(di),
                            kind: RowKind::Added,
                        });
                        row_base.push(None);
                    }
                    // Lay the conflict's base lines into the Base pane, aligned to
                    // the block's rows; if there are more base lines than rows,
                    // append blank-on-both-sides filler rows inside the block so
                    // every base line stays visible (and the panes stay in step).
                    for (k, line) in base.iter().enumerate() {
                        let pbi = base_pane_lines.len();
                        base_pane_lines.push(line.clone());
                        if start + k < rows.len() {
                            row_base[start + k] = Some(pbi);
                        } else {
                            rows.push(DiffRow {
                                left: None,
                                right: None,
                                kind: RowKind::Changed,
                            });
                            row_base.push(Some(pbi));
                        }
                    }
                    // One block per conflict, even if empty, so blocks stay 1:1
                    // and in order with the conflict segments.
                    blocks.push(Block {
                        rows: start..rows.len(),
                        resolution: Resolution::None,
                    });
                }
            }
        }

        DiffView {
            kind: ViewKind::Conflict,
            file_name,
            doc_id,
            path,
            base_lines,
            doc_lines,
            base_pane_lines,
            row_base,
            show_base: has_base,
            has_base,
            rows,
            blocks,
            segments,
            selected: 0,
            scroll: 0,
            hscroll: 0,
            viewport: 1,
            read_only: false,
            auto_advance: false,
            skip_prefers: false,
            word_refine: false,
        }
    }

    /// True when the two texts are identical (nothing to show).
    pub fn is_unchanged(&self) -> bool {
        self.blocks.is_empty()
    }

    /// Number of resolved blocks, for the header. In diff mode this counts
    /// blocks taken to HEAD (`Left`, i.e. changed from the `Right` default); in
    /// conflict mode it counts blocks resolved away from `None`.
    fn resolved_count(&self) -> usize {
        match self.kind {
            ViewKind::Diff => self
                .blocks
                .iter()
                .filter(|b| b.resolution == Resolution::Left)
                .count(),
            ViewKind::Conflict => self
                .blocks
                .iter()
                .filter(|b| b.resolution != Resolution::None)
                .count(),
        }
    }

    /// Conflict mode: number of still-unresolved conflict blocks.
    fn unresolved_count(&self) -> usize {
        self.blocks
            .iter()
            .filter(|b| b.resolution == Resolution::None)
            .count()
    }

    /// Conflict mode: true when every conflict has a chosen resolution.
    fn all_resolved(&self) -> bool {
        self.blocks.iter().all(|b| b.resolution != Resolution::None)
    }

    fn max_scroll(&self) -> usize {
        self.rows.len().saturating_sub(self.viewport)
    }

    fn scroll_by(&mut self, delta: isize) {
        let next = self.scroll as isize + delta;
        self.scroll = next.clamp(0, self.max_scroll() as isize) as usize;
    }

    /// Widest line (in display columns, tabs expanded) across all panes — the
    /// clamp ceiling for horizontal scrolling.
    fn max_line_width(&self) -> usize {
        self.base_lines
            .iter()
            .chain(self.doc_lines.iter())
            .chain(self.base_pane_lines.iter())
            .map(|s| line_width(s))
            .max()
            .unwrap_or(0)
    }

    /// Scroll horizontally by `delta` columns, clamped to `[0, max_line_width]`.
    fn hscroll_by(&mut self, delta: isize) {
        let max = self.max_line_width() as isize;
        let next = self.hscroll as isize + delta;
        self.hscroll = next.clamp(0, max.max(0)) as usize;
    }

    /// Scroll so the selected block is within the viewport.
    fn scroll_to_selected(&mut self) {
        if let Some(block) = self.blocks.get(self.selected) {
            let start = block.rows.start;
            if start < self.scroll {
                self.scroll = start;
            } else if start >= self.scroll + self.viewport {
                self.scroll = start.saturating_sub(self.viewport.saturating_sub(1));
            }
            self.scroll = self.scroll.min(self.max_scroll());
        }
    }

    /// Focus the next change block and scroll it into view.
    fn next_change(&mut self) {
        // Emerge `skip-prefers`: with it on, differences that already have a side
        // chosen are stepped over, so only what still needs a decision is visited.
        let next = if self.skip_prefers {
            ((self.selected + 1)..self.blocks.len())
                .find(|&i| self.blocks[i].resolution == Resolution::None)
        } else {
            (self.selected + 1 < self.blocks.len()).then_some(self.selected + 1)
        };
        if let Some(i) = next {
            self.selected = i;
        }
        self.scroll_to_selected();
    }

    /// Focus the previous change block and scroll it into view.
    fn prev_change(&mut self) {
        let prev = if self.skip_prefers {
            (0..self.selected)
                .rev()
                .find(|&i| self.blocks[i].resolution == Resolution::None)
        } else {
            self.selected.checked_sub(1)
        };
        if let Some(i) = prev {
            self.selected = i;
        }
        self.scroll_to_selected();
    }

    /// Emerge `emerge-auto-advance`: whether choosing a side moves on to the next
    /// difference by itself. Returns the new state.
    pub fn toggle_auto_advance(&mut self) -> bool {
        self.auto_advance = !self.auto_advance;
        self.auto_advance
    }

    /// Emerge `emerge-skip-prefers`: whether stepping between differences skips
    /// the ones that already have a side chosen. Returns the new state.
    pub fn toggle_skip_prefers(&mut self) -> bool {
        self.skip_prefers = !self.skip_prefers;
        self.skip_prefers
    }

    /// Set the selected block's resolution. With `emerge-auto-advance` on, the
    /// next difference is selected right after (Emerge's `auto-advance` submode).
    fn resolve_selected(&mut self, resolution: Resolution) {
        if let Some(block) = self.blocks.get_mut(self.selected) {
            block.resolution = resolution;
        }
        if self.auto_advance && resolution != Resolution::None {
            self.next_change();
        }
    }

    /// Set every block's resolution.
    fn resolve_all(&mut self, resolution: Resolution) {
        for block in &mut self.blocks {
            block.resolution = resolution;
        }
    }

    /// The block index owning row `i`, if any (for render highlighting).
    fn block_at(&self, i: usize) -> Option<usize> {
        self.blocks.iter().position(|b| b.rows.contains(&i))
    }

    /// Build the resolved Result text from the current resolutions.
    fn result_text(&self) -> String {
        match self.kind {
            ViewKind::Diff => {
                result_text(&self.rows, &self.blocks, &self.base_lines, &self.doc_lines)
            }
            ViewKind::Conflict => conflict_result_text(&self.segments, &self.blocks),
        }
    }
}

impl Component for DiffView {
    fn handle_event(&mut self, event: &Event, _cx: &mut Context) -> EventResult {
        let close: crate::compositor::Callback = Box::new(|compositor: &mut Compositor, _cx| {
            compositor.pop();
        });

        let key = match event {
            Event::Key(key) => *key,
            Event::Mouse(ev) => {
                match ev.kind {
                    MouseEventKind::ScrollDown => self.scroll_by(3),
                    MouseEventKind::ScrollUp => self.scroll_by(-3),
                    _ => {}
                }
                return EventResult::Consumed(None);
            }
            _ => return EventResult::Ignored(None),
        };

        let page = self.viewport.max(1) as isize;
        match key {
            key!('q') | key!(Esc) | ctrl!('c') => return EventResult::Consumed(Some(close)),
            // Apply: write the resolved Result back into the document, then close.
            // In conflict mode, once every conflict is resolved, also write the
            // file to disk and `git add` it to mark the conflict resolved.
            key!(Enter) | key!('a') => {
                // Comparison-only view: never write back, just close.
                if self.read_only {
                    return EventResult::Consumed(Some(close));
                }
                // Compute everything the callback needs up front — it can't
                // borrow `self`.
                let result = self.result_text();
                let doc_id = self.doc_id;
                let is_conflict = self.kind == ViewKind::Conflict;
                let all_resolved = self.all_resolved();
                let remaining = self.unresolved_count();
                let path = self.path.clone();
                let apply: Callback = Box::new(move |compositor: &mut Compositor, cx| {
                    let (view, doc) = current!(cx.editor);
                    if doc.id() == doc_id {
                        let new_text = zmax_core::Rope::from(result.as_str());
                        let transaction =
                            zmax_core::diff::compare_ropes(&doc.text().clone(), &new_text);
                        doc.apply(&transaction, view.id);
                        doc.append_changes_to_history(view);
                    }
                    compositor.pop();

                    if !is_conflict {
                        return;
                    }
                    if !all_resolved {
                        cx.editor
                            .set_status(format!("{remaining} conflicts remaining"));
                        return;
                    }
                    let Some(path) = path else {
                        cx.editor
                            .set_status("conflicts resolved (no file path to stage)");
                        return;
                    };
                    // The buffer now holds the fully-resolved text; mirror it to
                    // disk so `git add` stages the resolution.
                    if let Err(err) = std::fs::write(&path, &result) {
                        cx.editor.set_status(format!("write failed: {err}"));
                        return;
                    }
                    match std::process::Command::new("git")
                        .args(["add", "--"])
                        .arg(&path)
                        .status()
                    {
                        Ok(status) if status.success() => cx
                            .editor
                            .set_status("conflict resolved and staged (git add)"),
                        Ok(_) | Err(_) => cx.editor.set_status("conflict resolved; git add failed"),
                    }
                });
                return EventResult::Consumed(Some(apply));
            }
            key!('j') | key!(Down) => self.scroll_by(1),
            key!('k') | key!(Up) => self.scroll_by(-1),
            key!(PageDown) | ctrl!('d') | ctrl!('f') => self.scroll_by(page),
            key!(PageUp) | ctrl!('u') | ctrl!('b') => self.scroll_by(-page),
            key!('g') | key!(Home) => self.scroll = 0,
            key!('G') | key!(End) => self.scroll = self.max_scroll(),
            // Horizontal scroll (arrows so `h`/`l` stay conflict-accept keys).
            // `0`/`$` jump to the start / end of the longest line.
            key!(Right) => self.hscroll_by(4),
            key!(Left) => self.hscroll_by(-4),
            key!('0') => self.hscroll = 0,
            key!('$') => self.hscroll = self.max_line_width(),
            key!('n') => self.next_change(),
            key!('p') => self.prev_change(),
            // Resolve the selected block. `,`/`[`/`h` take ours (HEAD/left),
            // `.`/`]`/`l` take theirs (working/right).
            key!(',') | key!('[') | key!('h') => self.resolve_selected(Resolution::Left),
            key!('.') | key!(']') | key!('l') => self.resolve_selected(Resolution::Right),
            // Resolve all blocks one way.
            key!('L') => self.resolve_all(Resolution::Left),
            key!('R') => self.resolve_all(Resolution::Right),
            // Conflict-mode only: take both sides / reset to unresolved.
            key!('b') if self.kind == ViewKind::Conflict => self.resolve_selected(Resolution::Both),
            key!('u') | key!('x') if self.kind == ViewKind::Conflict => {
                self.resolve_selected(Resolution::None)
            }
            // Toggle the Base (common-ancestor) pane. Only meaningful when a
            // conflict carries a base; on narrow terminals the renderer keeps it
            // 3-pane regardless.
            key!('B') if self.kind == ViewKind::Conflict && self.has_base => {
                self.show_base = !self.show_base;
            }
            _ => {}
        }
        // Stay modal: never let keys leak to the editor behind us.
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        use ratatui::text::Line;
        use ratatui::widgets::Paragraph;

        let theme = &ctx.editor.theme;
        let mut bg = theme.get("ui.background");
        // `transparent-background`: drop the panel fill so the terminal shows
        // through, matching the editor surface and the rest of the IDE.
        if ctx.editor.config().transparent_background {
            bg.bg = None;
        }
        let text_style = theme.get("ui.text");
        let linenr_style = theme.get("ui.linenr");
        let sep_style = theme.get("ui.background.separator");
        let plus_style = theme.get("diff.plus");
        let minus_style = theme.get("diff.minus");
        let delta_style = theme.get("diff.delta");

        surface.clear_with(area, bg);

        if area.width < 8 || area.height < 4 {
            return;
        }

        // ── Layout ──────────────────────────────────────────────────────────
        // Two header rows, then the body. The body is split into 3 panes
        // (Current/Result/Incoming) or — when a common-ancestor base is present
        // and the Base pane is toggled on and the terminal is wide enough — 4
        // panes (Base/Current/Result/Incoming), with 1-column separators.
        let header_h = 2u16;
        let body_y = area.y + header_h;
        let body_h = area.height.saturating_sub(header_h);
        self.viewport = body_h as usize;

        // Base pane: only in conflict mode, only when a base exists and the user
        // hasn't toggled it off, and only on terminals wide enough to fit a
        // readable 4th column (otherwise stay 3-pane).
        let show_base_pane =
            self.kind == ViewKind::Conflict && self.show_base && self.has_base && area.width >= 100;

        // Split `width - separators` into N equal panes, remainder to the earlier
        // panes so the columns exactly fill the area.
        let n_panes: u16 = if show_base_pane { 4 } else { 3 };
        let n_seps = n_panes - 1;
        let avail = area.width.saturating_sub(n_seps);
        let pane_base = avail / n_panes;
        let rem = avail % n_panes;
        let pane_w = |i: u16| pane_base + u16::from(i < rem);

        // Compute each pane's x and the separator x's, left to right.
        let mut xs = [0u16; 4];
        let mut seps = [0u16; 3];
        let mut cur = area.x;
        for i in 0..n_panes {
            xs[i as usize] = cur;
            cur += pane_w(i);
            if i + 1 < n_panes {
                seps[i as usize] = cur;
                cur += 1;
            }
        }

        // Map the generic panes onto named columns. Result is always the pane
        // just left of the Incoming pane; the Base pane (when shown) is first.
        let base_col: Option<(u16, u16)>;
        let (left_x, left_w, center_x, center_w, right_x, right_w);
        if show_base_pane {
            base_col = Some((xs[0], pane_w(0)));
            left_x = xs[1];
            left_w = pane_w(1);
            center_x = xs[2];
            center_w = pane_w(2);
            right_x = xs[3];
            right_w = pane_w(3);
        } else {
            base_col = None;
            left_x = xs[0];
            left_w = pane_w(0);
            center_x = xs[1];
            center_w = pane_w(1);
            right_x = xs[2];
            right_w = pane_w(2);
        }

        // Gutter width: enough digits for the largest line number, plus a space.
        let max_no = self
            .base_lines
            .len()
            .max(self.doc_lines.len())
            .max(self.base_pane_lines.len())
            .max(1);
        let digits = ((max_no as f64).log10().floor() as usize) + 1;
        let gutter = (digits + 1) as u16;
        // Center gutter is two wider: a select marker + a direction arrow.
        let center_gutter = gutter + 2;

        // ── Header ──────────────────────────────────────────────────────────
        let count = self.blocks.len();
        let resolved = self.resolved_count();
        let noun = match self.kind {
            ViewKind::Diff => "change",
            ViewKind::Conflict => "conflict",
        };
        let header = format!(
            " {}  —  {} {}{} · {} resolved",
            self.file_name,
            count,
            noun,
            if count == 1 { "" } else { "s" },
            resolved,
        );
        let title_style = theme.get("ui.text.focus");
        surface.set_stringn(
            area.x,
            area.y,
            &header,
            area.width as usize,
            to_zstyle_bold(title_style),
        );
        // Key hint + column labels on the second header row (mode-dependent).
        let (hint, left_label, right_label) = match self.kind {
            ViewKind::Diff => (
                ", take HEAD   . take working   n/p nav   Enter apply   q cancel",
                " HEAD",
                " Working tree",
            ),
            ViewKind::Conflict => (
                ", ours  . theirs  b both  u unresolve  B base  n/p nav  Enter apply  q cancel",
                " Current (ours)",
                " Incoming (theirs)",
            ),
        };
        if let Some((base_x, base_w)) = base_col {
            surface.set_stringn(base_x, area.y + 1, " Base", base_w as usize, linenr_style);
        }
        surface.set_stringn(
            left_x,
            area.y + 1,
            left_label,
            left_w as usize,
            linenr_style,
        );
        surface.set_stringn(
            center_x,
            area.y + 1,
            " Result",
            center_w as usize,
            linenr_style,
        );
        surface.set_stringn(
            right_x,
            area.y + 1,
            right_label,
            right_w as usize,
            linenr_style,
        );
        // Separators down the full height.
        for y in area.y..area.y + area.height {
            for sep_x in seps.iter().take(n_seps as usize) {
                surface.set_string(*sep_x, y, "\u{2502}", sep_style);
            }
        }
        // Overlay the key hint dimly on the right of the title row if it fits.
        if (header.len() + hint.len() + 3) < area.width as usize {
            surface.set_stringn(
                area.x + area.width - hint.len() as u16 - 1,
                area.y,
                hint,
                hint.len(),
                linenr_style,
            );
        }

        if body_h == 0 {
            return;
        }

        // ── Body: build a ratatui Paragraph per pane ─────────────────────────
        let style = PaneStyle {
            text: text_style,
            linenr: linenr_style,
            filler: sep_style,
            plus: plus_style,
            minus: minus_style,
            delta: delta_style,
        };
        let selected_style = theme.get("ui.selection");
        let left_inner = left_w.saturating_sub(gutter) as usize;
        let center_inner = center_w.saturating_sub(center_gutter) as usize;
        let right_inner = right_w.saturating_sub(gutter) as usize;
        let base_inner = base_col.map(|(_, w)| w.saturating_sub(gutter) as usize);

        let mut base_pane_lines_v = Vec::with_capacity(body_h as usize);
        let mut left_lines = Vec::with_capacity(body_h as usize);
        let mut center_lines = Vec::with_capacity(body_h as usize);
        let mut right_lines = Vec::with_capacity(body_h as usize);
        for (offset, row) in self
            .rows
            .iter()
            .enumerate()
            .skip(self.scroll)
            .take(body_h as usize)
        {
            // For a paired modification, compute the char-level diff between the
            // old (left) and new (right) line so only the differing spans get
            // emphasised, instead of styling the whole line uniformly.
            let inline = match (row.kind, row.left, row.right) {
                (RowKind::Changed, Some(li), Some(ri)) => {
                    let old = self.base_lines.get(li).map(String::as_str).unwrap_or("");
                    let new = self.doc_lines.get(ri).map(String::as_str).unwrap_or("");
                    Some(if self.word_refine {
                        inline_spans_words(old, new)
                    } else {
                        inline_spans(old, new)
                    })
                }
                _ => None,
            };
            let left_emph = inline.as_ref().map(|(l, _)| l.as_slice());
            let right_emph = inline.as_ref().map(|(_, r)| r.as_slice());

            if let Some(base_inner) = base_inner {
                // The Base pane shows the common ancestor as neutral text
                // (RowKind::Unchanged) regardless of the conflict colouring.
                base_pane_lines_v.push(pane_line(
                    self.row_base[offset],
                    &self.base_pane_lines,
                    RowKind::Unchanged,
                    Side::Left,
                    gutter as usize,
                    base_inner,
                    self.hscroll,
                    None,
                    &style,
                ));
            }
            left_lines.push(pane_line(
                row.left,
                &self.base_lines,
                row.kind,
                Side::Left,
                gutter as usize,
                left_inner,
                self.hscroll,
                left_emph,
                &style,
            ));
            right_lines.push(pane_line(
                row.right,
                &self.doc_lines,
                row.kind,
                Side::Right,
                gutter as usize,
                right_inner,
                self.hscroll,
                right_emph,
                &style,
            ));
            // Center Result line, recomputed live from the resolutions.
            let block = self.block_at(offset);
            let resolution = block.map(|b| self.blocks[b].resolution);
            let selected = block == Some(self.selected);
            center_lines.push(result_line(
                row,
                resolution,
                selected,
                &self.base_lines,
                &self.doc_lines,
                gutter as usize,
                center_inner,
                self.hscroll,
                &style,
                selected_style,
            ));
        }
        // Pad the tail so the background fills the whole body.
        while left_lines.len() < body_h as usize {
            left_lines.push(Line::default());
            center_lines.push(Line::default());
            right_lines.push(Line::default());
            if base_inner.is_some() {
                base_pane_lines_v.push(Line::default());
            }
        }

        if let Some((base_x, base_w)) = base_col {
            let base_rect = Rect::new(base_x, body_y, base_w, body_h);
            crate::ui::rat::render(Paragraph::new(base_pane_lines_v), base_rect, surface);
        }
        let left_rect = Rect::new(left_x, body_y, left_w, body_h);
        let center_rect = Rect::new(center_x, body_y, center_w, body_h);
        let right_rect = Rect::new(right_x, body_y, right_w, body_h);
        crate::ui::rat::render(Paragraph::new(left_lines), left_rect, surface);
        crate::ui::rat::render(Paragraph::new(center_lines), center_rect, surface);
        crate::ui::rat::render(Paragraph::new(right_lines), right_rect, surface);
    }

    fn id(&self) -> Option<&'static str> {
        match self.kind {
            ViewKind::Diff => Some("diff"),
            ViewKind::Conflict => Some("merge"),
        }
    }
}

/// Which pane a line belongs to (selects deleted/added emphasis).
#[derive(Clone, Copy)]
enum Side {
    Left,
    Right,
}

/// Resolved theme styles for the panes.
struct PaneStyle {
    text: Style,
    linenr: Style,
    filler: Style,
    plus: Style,
    minus: Style,
    delta: Style,
}

/// Build one ratatui `Line` for a pane row: a right-aligned line-number gutter
/// followed by the line content, horizontally scrolled by `hscroll` columns and
/// padded to `inner` so the row background fills the pane width. When
/// `emphasis` is `Some` (a paired modification), it carries the per-side
/// char-level diff runs from [`inline_spans`] so only the differing spans are
/// rendered in the stronger (reversed + bold) style; otherwise the whole line
/// is styled uniformly. The gutter is never scrolled.
#[allow(clippy::too_many_arguments)]
fn pane_line<'a>(
    idx: Option<usize>,
    src: &[String],
    kind: RowKind,
    side: Side,
    gutter: usize,
    inner: usize,
    hscroll: usize,
    emphasis: Option<&[(String, bool)]>,
    style: &PaneStyle,
) -> ratatui::text::Line<'a> {
    use crate::ui::rat::to_rat_style;
    use ratatui::text::{Line, Span};
    use zmax_view::graphics::Modifier;

    let zstyle = match (kind, side) {
        (RowKind::Unchanged, _) => style.text,
        (RowKind::Changed, _) => style.delta,
        (RowKind::Removed, _) => style.minus,
        (RowKind::Added, _) => style.plus,
    };

    match idx {
        Some(i) => {
            let num = format!("{:>width$} ", i + 1, width = gutter.saturating_sub(1));
            // Char-level runs: the diff spans for a paired modification, else the
            // whole line as a single non-emphasised run.
            let runs: Vec<(String, bool)> = match emphasis {
                Some(e) => e.to_vec(),
                None => vec![(src.get(i).cloned().unwrap_or_default(), false)],
            };
            let emph = zstyle
                .add_modifier(Modifier::REVERSED)
                .add_modifier(Modifier::BOLD);
            let mut spans = vec![Span::styled(num, to_rat_style(style.linenr))];
            spans.extend(content_spans(&runs, hscroll, inner, zstyle, emph));
            Line::from(spans)
        }
        None => {
            // Blank filler on the side that has no counterpart line.
            let _ = side;
            let mut filler = String::new();
            truncate_pad(&mut filler, gutter + inner);
            Line::from(Span::styled(filler, to_rat_style(style.filler)))
        }
    }
}

/// Build the center **Result** pane line for one aligned row, live from its
/// block resolution. A two-column prefix (`▌` select marker + `◀`/`▶` direction
/// arrow) precedes the same number gutter + content layout as [`pane_line`].
/// Unchanged rows (`resolution == None`) show no marker/arrow.
#[allow(clippy::too_many_arguments)]
fn result_line<'a>(
    row: &DiffRow,
    resolution: Option<Resolution>,
    selected: bool,
    base_lines: &[String],
    doc_lines: &[String],
    gutter: usize,
    inner: usize,
    hscroll: usize,
    style: &PaneStyle,
    selected_style: Style,
) -> ratatui::text::Line<'a> {
    use crate::ui::rat::to_rat_style;
    use ratatui::text::{Line, Span};

    let marker = if selected { "\u{258C}" } else { " " }; // ▌
    let arrow = match resolution {
        None => " ",
        Some(Resolution::Left) => "\u{25C0}",  // ◀
        Some(Resolution::Right) => "\u{25B6}", // ▶
        Some(Resolution::Both) => "\u{25C6}",  // ◆
        Some(Resolution::None) => "?",         // unresolved conflict
    };
    // Which source line this row previews. `Left`/`Right` pick a side; `Both`,
    // unresolved (`Resolution::None`) and unchanged rows (outer `None`) preview
    // whichever side this row carries.
    let (idx, src): (Option<usize>, &[String]) = match resolution {
        Some(Resolution::Left) => (row.left, base_lines),
        Some(Resolution::Right) => (row.right, doc_lines),
        _ if row.right.is_some() => (row.right, doc_lines),
        _ => (row.left, base_lines),
    };

    let content_style = if selected { selected_style } else { style.text };
    let mut prefix = vec![
        Span::styled(marker.to_string(), to_rat_style(style.linenr)),
        Span::styled(arrow.to_string(), to_rat_style(style.delta)),
    ];

    match idx {
        Some(i) => {
            let num = format!("{:>width$} ", i + 1, width = gutter.saturating_sub(1));
            let runs = vec![(src.get(i).cloned().unwrap_or_default(), false)];
            prefix.push(Span::styled(num, to_rat_style(style.linenr)));
            prefix.extend(content_spans(
                &runs,
                hscroll,
                inner,
                content_style,
                content_style,
            ));
        }
        None => {
            // Resolved side contributes no line here (a blank filler row).
            let mut filler = String::new();
            truncate_pad(&mut filler, gutter + inner);
            prefix.push(Span::styled(filler, to_rat_style(style.filler)));
        }
    }
    Line::from(prefix)
}

/// Truncate `s` to `width` display columns (best-effort, char-based) or pad it
/// with spaces to exactly `width` columns.
fn truncate_pad(s: &mut String, width: usize) {
    let count = s.chars().count();
    if count > width {
        *s = s.chars().take(width).collect();
    } else {
        s.extend(std::iter::repeat_n(' ', width - count));
    }
}

/// Add BOLD to a zmax style.
fn to_zstyle_bold(style: Style) -> Style {
    style.add_modifier(zmax_view::graphics::Modifier::BOLD)
}

/// Display width of a line in columns, expanding each tab to 4 columns. Used to
/// clamp horizontal scrolling.
fn line_width(s: &str) -> usize {
    s.chars().map(|c| if c == '\t' { 4 } else { 1 }).sum()
}

/// Push the chars in `chars` as one run if non-empty.
fn push_run(out: &mut Vec<(String, bool)>, chars: &[char], emph: bool) {
    if !chars.is_empty() {
        out.push((chars.iter().collect(), emph));
    }
}

/// Char-level (intra-line) diff of two single lines.
///
/// Returns one run list per side; each run is `(text, emphasized)` where
/// `emphasized == true` marks the characters that differ between the two lines
/// (a deletion on the left / an insertion on the right). Common prefixes,
/// suffixes and interior matches come back as non-emphasised runs. Identical
/// inputs yield a single non-emphasised run on each side. Pure and unit-tested;
/// mirrors the char-diff approach in `zmax-core::diff` (Myers over `char`
/// tokens, since the histogram heuristic is poor for repeated characters).
#[allow(clippy::type_complexity)]
fn inline_spans(old: &str, new: &str) -> (Vec<(String, bool)>, Vec<(String, bool)>) {
    let old_chars: Vec<char> = old.chars().collect();
    let new_chars: Vec<char> = new.chars().collect();

    let mut input: InternedInput<char> = InternedInput::default();
    input.update_before(old_chars.iter().copied());
    input.update_after(new_chars.iter().copied());
    let diff = Diff::compute(Algorithm::Myers, &input);

    let mut left = Vec::new();
    let mut right = Vec::new();
    let (mut o, mut n) = (0usize, 0usize);
    for hunk in diff.hunks() {
        let (bs, be) = (hunk.before.start as usize, hunk.before.end as usize);
        let (as_, ae) = (hunk.after.start as usize, hunk.after.end as usize);
        // Common stretch before this hunk.
        push_run(&mut left, &old_chars[o..bs], false);
        push_run(&mut right, &new_chars[n..as_], false);
        // The differing stretch (emphasised).
        push_run(&mut left, &old_chars[bs..be], true);
        push_run(&mut right, &new_chars[as_..ae], true);
        o = be;
        n = ae;
    }
    // Common tail after the last hunk.
    push_run(&mut left, &old_chars[o..], false);
    push_run(&mut right, &new_chars[n..], false);
    (left, right)
}

/// Split one line into `ediff-forward-word` tokens and the white space between
/// them, as `(text, is_word)`. Concatenating every `text` reproduces the line
/// exactly, so the pane still renders the original characters — only the
/// comparison granularity changes.
fn ediff_word_segments(line: &str) -> Vec<(String, bool)> {
    use crate::commands::{ediff_in_word_class, ediff_is_whitespace, ediff_word_class_of};

    let mut segments = Vec::new();
    let mut chars = line.chars().peekable();
    while let Some(&first) = chars.peek() {
        let mut text = String::new();
        let is_word = !ediff_is_whitespace(first);
        if is_word {
            // `ediff-forward-word` takes the first class that matches the leading
            // character and runs to the end of *that* class.
            let class = ediff_word_class_of(first);
            while let Some(c) = chars.next_if(|c| ediff_in_word_class(*c, class)) {
                text.push(c);
            }
        } else {
            while let Some(c) = chars.next_if(|c| ediff_is_whitespace(*c)) {
                text.push(c);
            }
        }
        segments.push((text, is_word));
    }
    segments
}

/// [`inline_spans`] at emacs's *word* granularity — the refinement
/// `ediff-regions-wordwise` shows. The comparison runs over `ediff-forward-word`
/// tokens (white space is not a token in emacs, so it never differs on its own),
/// and the emphasis is painted back onto the original characters rather than the
/// wordified scratch text.
#[allow(clippy::type_complexity)]
fn inline_spans_words(old: &str, new: &str) -> (Vec<(String, bool)>, Vec<(String, bool)>) {
    let old_segments = ediff_word_segments(old);
    let new_segments = ediff_word_segments(new);

    let words = |segments: &[(String, bool)]| -> Vec<String> {
        segments
            .iter()
            .filter(|(_, is_word)| *is_word)
            .map(|(text, _)| text.clone())
            .collect()
    };
    let old_words = words(&old_segments);
    let new_words = words(&new_segments);

    let mut input: InternedInput<String> = InternedInput::default();
    input.update_before(old_words.iter().cloned());
    input.update_after(new_words.iter().cloned());
    let diff = Diff::compute(Algorithm::Myers, &input);

    // Which word *index* on each side falls inside a differing hunk.
    let mut old_changed = vec![false; old_words.len()];
    let mut new_changed = vec![false; new_words.len()];
    for hunk in diff.hunks() {
        old_changed[hunk.before.start as usize..hunk.before.end as usize].fill(true);
        new_changed[hunk.after.start as usize..hunk.after.end as usize].fill(true);
    }

    // Walk the segments back, emphasising the word segments the diff flagged and
    // leaving the white space between them neutral.
    let paint = |segments: Vec<(String, bool)>, changed: &[bool]| -> Vec<(String, bool)> {
        let mut runs: Vec<(String, bool)> = Vec::new();
        let mut word = 0usize;
        for (text, is_word) in segments {
            let emphasised = is_word && changed[word];
            if is_word {
                word += 1;
            }
            match runs.last_mut() {
                // Coalesce, matching `push_run`'s contract for `inline_spans`.
                Some((prev, prev_emph)) if *prev_emph == emphasised => prev.push_str(&text),
                _ => runs.push((text, emphasised)),
            }
        }
        runs
    };
    (
        paint(old_segments, &old_changed),
        paint(new_segments, &new_changed),
    )
}

/// Lay out content runs into exactly `width` display cells, applying the
/// horizontal scroll `hscroll` (in columns) before truncating/padding.
///
/// Tabs in each run expand to 4 spaces (inheriting the run's emphasis), then the
/// first `hscroll` columns are skipped, the remainder is truncated to `width`,
/// and the line is right-padded with spaces to fill `width`. Operates on `char`s
/// throughout so multibyte text never panics. Pure and unit-tested.
fn layout_cells(runs: &[(String, bool)], hscroll: usize, width: usize) -> Vec<(char, bool)> {
    let mut cells: Vec<(char, bool)> = Vec::new();
    for (text, emph) in runs {
        for ch in text.chars() {
            if ch == '\t' {
                cells.extend(std::iter::repeat_n((' ', *emph), 4));
            } else {
                cells.push((ch, *emph));
            }
        }
    }
    // Skip the leading scrolled-off columns.
    let mut visible: Vec<(char, bool)> = if hscroll < cells.len() {
        cells[hscroll..].to_vec()
    } else {
        Vec::new()
    };
    // Truncate or pad to exactly `width` cells.
    if visible.len() > width {
        visible.truncate(width);
    } else {
        visible.extend(std::iter::repeat_n((' ', false), width - visible.len()));
    }
    visible
}

/// Build the styled content spans for one pane row from its char-level runs,
/// scrolled and clamped via [`layout_cells`]. Adjacent cells with the same
/// emphasis are coalesced into a single `Span`: emphasised cells get `emph`, the
/// rest get `base`.
fn content_spans<'a>(
    runs: &[(String, bool)],
    hscroll: usize,
    width: usize,
    base: Style,
    emph: Style,
) -> Vec<ratatui::text::Span<'a>> {
    use crate::ui::rat::to_rat_style;
    use ratatui::text::Span;

    let cells = layout_cells(runs, hscroll, width);
    let mut spans = Vec::new();
    let mut cur = String::new();
    let mut cur_emph = false;
    for (ch, e) in cells {
        if !cur.is_empty() && e != cur_emph {
            let st = if cur_emph { emph } else { base };
            spans.push(Span::styled(std::mem::take(&mut cur), to_rat_style(st)));
        }
        cur_emph = e;
        cur.push(ch);
    }
    if !cur.is_empty() {
        let st = if cur_emph { emph } else { base };
        spans.push(Span::styled(cur, to_rat_style(st)));
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(rows: &[DiffRow]) -> Vec<RowKind> {
        rows.iter().map(|r| r.kind).collect()
    }

    #[test]
    fn identical_texts_pair_every_line() {
        let rows = align("a\nb\nc\n", "a\nb\nc\n");
        assert_eq!(rows.len(), 3);
        assert!(rows.iter().all(|r| r.kind == RowKind::Unchanged));
        for (i, r) in rows.iter().enumerate() {
            assert_eq!(r.left, Some(i));
            assert_eq!(r.right, Some(i));
        }
        assert!(change_blocks(&rows).is_empty());
    }

    #[test]
    fn pure_insertion_pads_left_side() {
        // "b" inserted between a and c.
        let rows = align("a\nc\n", "a\nb\nc\n");
        assert_eq!(
            kinds(&rows),
            vec![RowKind::Unchanged, RowKind::Added, RowKind::Unchanged]
        );
        let added = &rows[1];
        assert_eq!(added.left, None, "inserted line has no HEAD counterpart");
        assert_eq!(added.right, Some(1));
        assert_eq!(change_blocks(&rows), vec![1]);
    }

    #[test]
    fn pure_deletion_pads_right_side() {
        // "b" removed.
        let rows = align("a\nb\nc\n", "a\nc\n");
        assert_eq!(
            kinds(&rows),
            vec![RowKind::Unchanged, RowKind::Removed, RowKind::Unchanged]
        );
        let removed = &rows[1];
        assert_eq!(removed.left, Some(1));
        assert_eq!(
            removed.right, None,
            "deleted line has no working counterpart"
        );
    }

    #[test]
    fn modification_pairs_old_against_new() {
        // Single changed line: old "b" on the left, new "B" on the right.
        let rows = align("a\nb\nc\n", "a\nB\nc\n");
        assert_eq!(
            kinds(&rows),
            vec![RowKind::Unchanged, RowKind::Changed, RowKind::Unchanged]
        );
        let changed = &rows[1];
        assert_eq!(changed.left, Some(1));
        assert_eq!(changed.right, Some(1));
    }

    #[test]
    fn lopsided_change_pairs_then_pads() {
        // 1 old line replaced by 3 new lines: 1 Changed + 2 Added, panes aligned.
        let rows = align("a\nx\nc\n", "a\np\nq\nr\nc\n");
        assert_eq!(
            kinds(&rows),
            vec![
                RowKind::Unchanged,
                RowKind::Changed,
                RowKind::Added,
                RowKind::Added,
                RowKind::Unchanged,
            ]
        );
        // The two pure-Added rows have blank HEAD sides so the panes stay aligned.
        assert_eq!(rows[2].left, None);
        assert_eq!(rows[3].left, None);
        // One contiguous change block starting at row 1.
        assert_eq!(change_blocks(&rows), vec![1]);
    }

    #[test]
    fn split_lines_matches_diff_tokenisation() {
        assert_eq!(split_lines("a\nb\n"), vec!["a", "b"]);
        assert_eq!(split_lines("a\nb"), vec!["a", "b"]);
        assert_eq!(split_lines(""), Vec::<String>::new());
        assert_eq!(split_lines("a\r\nb\r\n"), vec!["a", "b"]);
    }

    // ── Result computation (slice 2) ─────────────────────────────────────────

    /// Build the full Result-text inputs from two texts, override each block's
    /// resolution with `resolutions[i]`, and return the merged text.
    fn merged(base: &str, doc: &str, resolutions: &[Resolution]) -> String {
        let rows = align(base, doc);
        let mut blocks = compute_blocks(&rows);
        assert_eq!(
            blocks.len(),
            resolutions.len(),
            "test gave the wrong number of resolutions"
        );
        for (b, &r) in blocks.iter_mut().zip(resolutions) {
            b.resolution = r;
        }
        let base_lines = split_lines(base);
        let doc_lines = split_lines(doc);
        result_text(&rows, &blocks, &base_lines, &doc_lines)
    }

    #[test]
    fn compute_blocks_default_to_right() {
        let rows = align("a\nb\nc\n", "a\nB\nc\n");
        let blocks = compute_blocks(&rows);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].rows, 1..2);
        assert_eq!(blocks[0].resolution, Resolution::Right);
    }

    #[test]
    fn unchanged_text_results_unchanged() {
        // No blocks: Result is just the text.
        let rows = align("a\nb\n", "a\nb\n");
        let blocks = compute_blocks(&rows);
        assert!(blocks.is_empty());
        let lines = split_lines("a\nb\n");
        assert_eq!(result_text(&rows, &blocks, &lines, &lines), "a\nb\n");
    }

    #[test]
    fn modification_left_is_head_right_is_working() {
        // "b" -> "B". Left keeps HEAD ("b"), Right keeps working ("B").
        assert_eq!(
            merged("a\nb\nc\n", "a\nB\nc\n", &[Resolution::Left]),
            "a\nb\nc\n"
        );
        assert_eq!(
            merged("a\nb\nc\n", "a\nB\nc\n", &[Resolution::Right]),
            "a\nB\nc\n"
        );
    }

    #[test]
    fn deletion_left_keeps_line_right_drops_it() {
        // "b" deleted in working. Left reverts (keeps "b"), Right drops it.
        assert_eq!(
            merged("a\nb\nc\n", "a\nc\n", &[Resolution::Left]),
            "a\nb\nc\n"
        );
        assert_eq!(
            merged("a\nb\nc\n", "a\nc\n", &[Resolution::Right]),
            "a\nc\n"
        );
    }

    #[test]
    fn insertion_left_drops_line_right_keeps_it() {
        // "b" inserted in working. Left drops it, Right keeps it.
        assert_eq!(merged("a\nc\n", "a\nb\nc\n", &[Resolution::Left]), "a\nc\n");
        assert_eq!(
            merged("a\nc\n", "a\nb\nc\n", &[Resolution::Right]),
            "a\nb\nc\n"
        );
    }

    #[test]
    fn lopsided_change_emits_actual_lines_not_blanks() {
        // 1 line -> 3 lines. Right emits all three working lines (no padding);
        // Left emits the single HEAD line.
        let base = "a\nx\nc\n";
        let doc = "a\np\nq\nr\nc\n";
        assert_eq!(merged(base, doc, &[Resolution::Right]), "a\np\nq\nr\nc\n");
        assert_eq!(merged(base, doc, &[Resolution::Left]), "a\nx\nc\n");
    }

    #[test]
    fn multiple_blocks_resolve_independently() {
        // Two separate changes: take HEAD for the first, working for the second.
        let base = "a\nb\nc\nd\ne\n";
        let doc = "a\nB\nc\nD\ne\n";
        assert_eq!(
            merged(base, doc, &[Resolution::Left, Resolution::Right]),
            "a\nb\nc\nD\ne\n"
        );
    }

    // ── Conflict parsing (slice 3) ───────────────────────────────────────────

    fn ctx(lines: &[&str]) -> Segment {
        Segment::Context(lines.iter().map(|s| s.to_string()).collect())
    }
    fn conflict(ours: &[&str], base: &[&str], theirs: &[&str]) -> Segment {
        Segment::Conflict {
            ours: ours.iter().map(|s| s.to_string()).collect(),
            base: base.iter().map(|s| s.to_string()).collect(),
            theirs: theirs.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn parse_no_markers_returns_none() {
        assert_eq!(parse_conflicts("a\nb\nc\n"), None);
        assert_eq!(parse_conflicts(""), None);
    }

    #[test]
    fn parse_single_conflict() {
        let text = "top\n\
                    <<<<<<< HEAD\n\
                    ours1\n\
                    ours2\n\
                    =======\n\
                    theirs1\n\
                    >>>>>>> branch\n\
                    bottom\n";
        assert_eq!(
            parse_conflicts(text),
            Some(vec![
                ctx(&["top"]),
                conflict(&["ours1", "ours2"], &[], &["theirs1"]),
                ctx(&["bottom"]),
            ])
        );
    }

    #[test]
    fn parse_multiple_conflicts() {
        let text = "<<<<<<<\n\
                    A\n\
                    =======\n\
                    B\n\
                    >>>>>>>\n\
                    mid\n\
                    <<<<<<<\n\
                    C\n\
                    =======\n\
                    D\n\
                    >>>>>>>\n";
        assert_eq!(
            parse_conflicts(text),
            Some(vec![
                conflict(&["A"], &[], &["B"]),
                ctx(&["mid"]),
                conflict(&["C"], &[], &["D"]),
            ])
        );
    }

    #[test]
    fn parse_diff3_base_section() {
        let text = "<<<<<<< ours\n\
                    o\n\
                    ||||||| base\n\
                    b1\n\
                    b2\n\
                    =======\n\
                    t\n\
                    >>>>>>> theirs\n";
        assert_eq!(
            parse_conflicts(text),
            Some(vec![conflict(&["o"], &["b1", "b2"], &["t"])])
        );
    }

    #[test]
    fn parse_crlf_line_endings() {
        let text = "top\r\n\
                    <<<<<<< HEAD\r\n\
                    ours\r\n\
                    =======\r\n\
                    theirs\r\n\
                    >>>>>>> branch\r\n";
        assert_eq!(
            parse_conflicts(text),
            Some(vec![ctx(&["top"]), conflict(&["ours"], &[], &["theirs"])])
        );
    }

    #[test]
    fn parse_missing_trailing_newline() {
        // No final newline after the closing marker.
        let text = "<<<<<<<\nours\n=======\ntheirs\n>>>>>>>";
        assert_eq!(
            parse_conflicts(text),
            Some(vec![conflict(&["ours"], &[], &["theirs"])])
        );
    }

    // ── Conflict result text (slice 3) ───────────────────────────────────────

    /// Parse `text`, build a conflict view, override each conflict block's
    /// resolution with `resolutions[i]`, and return the resolved text.
    fn resolved_conflict(text: &str, resolutions: &[Resolution]) -> String {
        let segments = parse_conflicts(text).expect("expected conflict markers");
        let view = DiffView::from_conflicts("f".to_string(), DocumentId::default(), None, segments);
        let mut blocks = view.blocks;
        assert_eq!(
            blocks.len(),
            resolutions.len(),
            "test gave the wrong number of resolutions"
        );
        for (b, &r) in blocks.iter_mut().zip(resolutions) {
            b.resolution = r;
        }
        conflict_result_text(&view.segments, &blocks)
    }

    #[test]
    fn conflict_ours_theirs_both() {
        let text = "x\n<<<<<<<\nours\n=======\ntheirs\n>>>>>>>\ny\n";
        assert_eq!(resolved_conflict(text, &[Resolution::Left]), "x\nours\ny\n");
        assert_eq!(
            resolved_conflict(text, &[Resolution::Right]),
            "x\ntheirs\ny\n"
        );
        assert_eq!(
            resolved_conflict(text, &[Resolution::Both]),
            "x\nours\ntheirs\ny\n"
        );
    }

    #[test]
    fn conflict_none_re_emits_markers() {
        // An unresolved conflict re-emits the markers (normalised, base dropped
        // when empty) so the file stays conflicted.
        let text = "x\n<<<<<<<\nours\n=======\ntheirs\n>>>>>>>\ny\n";
        assert_eq!(
            resolved_conflict(text, &[Resolution::None]),
            "x\n<<<<<<<\nours\n=======\ntheirs\n>>>>>>>\ny\n"
        );
    }

    #[test]
    fn conflict_none_preserves_diff3_base() {
        let text = "<<<<<<<\nours\n|||||||\nbase\n=======\ntheirs\n>>>>>>>\n";
        assert_eq!(
            resolved_conflict(text, &[Resolution::None]),
            "<<<<<<<\nours\n|||||||\nbase\n=======\ntheirs\n>>>>>>>\n"
        );
    }

    #[test]
    fn conflict_partial_resolution_leaves_rest_conflicted() {
        // First conflict resolved to ours, second left unresolved.
        let text = "<<<<<<<\nA\n=======\nB\n>>>>>>>\n\
                    mid\n\
                    <<<<<<<\nC\n=======\nD\n>>>>>>>\n";
        assert_eq!(
            resolved_conflict(text, &[Resolution::Left, Resolution::None]),
            "A\nmid\n<<<<<<<\nC\n=======\nD\n>>>>>>>\n"
        );
    }

    #[test]
    fn from_conflicts_blocks_default_to_unresolved() {
        let segments = parse_conflicts("<<<<<<<\nA\n=======\nB\n>>>>>>>\n").unwrap();
        let view = DiffView::from_conflicts("f".into(), DocumentId::default(), None, segments);
        assert_eq!(view.kind, ViewKind::Conflict);
        assert_eq!(view.blocks.len(), 1);
        assert_eq!(view.blocks[0].resolution, Resolution::None);
        assert!(!view.all_resolved());
    }

    // ── diff3 three-way merge (slice 4) ──────────────────────────────────────

    #[test]
    fn diff3_ours_only_change_auto_merges() {
        // theirs == base; only ours changed line 2 → take ours, no conflict.
        assert_eq!(
            diff3("a\nb\nc\n", "a\nB\nc\n", "a\nb\nc\n"),
            vec![ctx(&["a", "B", "c"])]
        );
    }

    #[test]
    fn diff3_theirs_only_change_auto_merges() {
        // ours == base; only theirs changed → take theirs, no conflict.
        assert_eq!(
            diff3("a\nb\nc\n", "a\nb\nc\n", "a\nb\nC\n"),
            vec![ctx(&["a", "b", "C"])]
        );
    }

    #[test]
    fn diff3_identical_change_on_both_sides() {
        // Both sides made the same edit → take it, no conflict.
        assert_eq!(
            diff3("a\nb\nc\n", "a\nX\nc\n", "a\nX\nc\n"),
            vec![ctx(&["a", "X", "c"])]
        );
    }

    #[test]
    fn diff3_genuine_conflict_carries_base() {
        // Overlapping edits → conflict, and it carries the real base line "b".
        assert_eq!(
            diff3("a\nb\nc\n", "a\nX\nc\n", "a\nY\nc\n"),
            vec![ctx(&["a"]), conflict(&["X"], &["b"], &["Y"]), ctx(&["c"])]
        );
    }

    #[test]
    fn diff3_disjoint_changes_both_apply() {
        // ours changes the first line, theirs the last — both auto-merge.
        assert_eq!(
            diff3("a\nb\nc\nd\ne\n", "A\nb\nc\nd\ne\n", "a\nb\nc\nd\nE\n"),
            vec![ctx(&["A", "b", "c", "d", "E"])]
        );
    }

    #[test]
    fn diff3_insertion_one_side_change_other_disjoint() {
        // ours inserts a line near the top, theirs changes the last line.
        assert_eq!(
            diff3("a\nb\nc\n", "a\nNEW\nb\nc\n", "a\nb\nC\n"),
            vec![ctx(&["a", "NEW", "b", "C"])]
        );
    }

    #[test]
    fn diff3_multiple_regions_mixed() {
        // Region 1: ours-only change (auto-merged). Region 2: genuine conflict.
        assert_eq!(
            diff3("a\nb\nc\nd\ne\n", "a\nB\nc\nX\ne\n", "a\nb\nc\nY\ne\n"),
            vec![
                ctx(&["a", "B", "c"]),
                conflict(&["X"], &["d"], &["Y"]),
                ctx(&["e"]),
            ]
        );
    }

    #[test]
    fn diff3_adjacent_conflict_regions() {
        // Two conflicting lines back-to-back with no stable line between them:
        // one conflict spanning both base lines.
        assert_eq!(
            diff3("a\nb\nc\n", "X1\nX2\nc\n", "Y1\nY2\nc\n"),
            vec![
                conflict(&["X1", "X2"], &["a", "b"], &["Y1", "Y2"]),
                ctx(&["c"]),
            ]
        );
    }

    #[test]
    fn diff3_empty_base_addadd_conflict() {
        // No common ancestor; both sides added different content → conflict with
        // an empty base.
        assert_eq!(diff3("", "a\n", "b\n"), vec![conflict(&["a"], &[], &["b"])]);
    }

    #[test]
    fn diff3_empty_base_addadd_identical() {
        // Both sides added the same content → auto-merge, no conflict.
        assert_eq!(diff3("", "a\n", "a\n"), vec![ctx(&["a"])]);
    }

    #[test]
    fn diff3_ours_deletes_everything() {
        // ours deleted all lines, theirs unchanged → take the deletion (empty).
        assert!(diff3("a\nb\n", "", "a\nb\n").is_empty());
    }

    #[test]
    fn diff3_theirs_deletes_everything() {
        assert!(diff3("a\nb\n", "a\nb\n", "").is_empty());
    }

    #[test]
    fn diff3_all_empty() {
        assert!(diff3("", "", "").is_empty());
    }

    // ── diff3 round-trips through the conflict view (slice 4) ─────────────────

    #[test]
    fn diff3_roundtrip_auto_merge_survives_apply() {
        // A fully auto-merged diff3 has no Conflict segments: the conflict view
        // has zero blocks and its resolved text is just the merged content.
        let segs = diff3("a\nb\nc\n", "a\nB\nc\n", "a\nb\nc\n");
        let view = DiffView::from_conflicts("f".into(), DocumentId::default(), None, segs);
        assert!(
            view.blocks.is_empty(),
            "auto-merge produces no conflict blocks"
        );
        assert_eq!(
            conflict_result_text(&view.segments, &view.blocks),
            "a\nB\nc\n"
        );
    }

    #[test]
    fn diff3_roundtrip_conflict_resolves_each_way() {
        // A genuine conflict from diff3 round-trips through the view: Left→ours,
        // Right→theirs, Both→ours then theirs.
        let segs = diff3("a\nb\nc\n", "a\nX\nc\n", "a\nY\nc\n");
        let view = DiffView::from_conflicts("f".into(), DocumentId::default(), None, segs);
        assert_eq!(view.blocks.len(), 1);
        let mut blocks = view.blocks.clone();

        blocks[0].resolution = Resolution::Left;
        assert_eq!(conflict_result_text(&view.segments, &blocks), "a\nX\nc\n");
        blocks[0].resolution = Resolution::Right;
        assert_eq!(conflict_result_text(&view.segments, &blocks), "a\nY\nc\n");
        blocks[0].resolution = Resolution::Both;
        assert_eq!(
            conflict_result_text(&view.segments, &blocks),
            "a\nX\nY\nc\n"
        );
    }

    #[test]
    fn diff3_roundtrip_conflict_records_base_for_pane() {
        // The conflict's base line is captured so the Base pane can show it, and
        // `has_base` is set.
        let segs = diff3("a\nb\nc\n", "a\nX\nc\n", "a\nY\nc\n");
        let view = DiffView::from_conflicts("f".into(), DocumentId::default(), None, segs);
        assert!(view.has_base);
        assert!(view.show_base);
        assert!(view.base_pane_lines.contains(&"b".to_string()));
    }

    // ── intra-line (char-level) highlighting ─────────────────────────────────

    /// Concatenate only the emphasised runs.
    fn emph_text(runs: &[(String, bool)]) -> String {
        runs.iter()
            .filter(|(_, e)| *e)
            .map(|(s, _)| s.as_str())
            .collect()
    }
    /// Concatenate every run (reconstructs the original line).
    fn full_text(runs: &[(String, bool)]) -> String {
        runs.iter().map(|(s, _)| s.as_str()).collect()
    }

    #[test]
    fn inline_identical_has_no_emphasis() {
        let (l, r) = inline_spans("hello world", "hello world");
        assert_eq!(emph_text(&l), "");
        assert_eq!(emph_text(&r), "");
        assert_eq!(full_text(&l), "hello world");
        assert_eq!(full_text(&r), "hello world");
        // No run is flagged emphasised.
        assert!(l.iter().all(|(_, e)| !e));
        assert!(r.iter().all(|(_, e)| !e));
    }

    #[test]
    fn inline_empty_inputs() {
        let (l, r) = inline_spans("", "");
        assert!(l.is_empty() && r.is_empty());
    }

    #[test]
    fn inline_one_word_change_emphasises_only_that_word() {
        // Only the middle word differs; the surrounding text is common.
        let (l, r) = inline_spans("the cat sat", "the dog sat");
        assert_eq!(emph_text(&l), "cat");
        assert_eq!(emph_text(&r), "dog");
        assert_eq!(full_text(&l), "the cat sat");
        assert_eq!(full_text(&r), "the dog sat");
    }

    #[test]
    fn wordwise_refines_only_the_differing_word_inside_punctuation() {
        // The whole point of `ediff-regions-wordwise`: `foo(bar)` against
        // `foo(qux)` is not one differing token. Emacs's tokenizer splits on four
        // character classes, so `foo`, `(`, `bar` and `)` are separate words and
        // only `bar`/`qux` is marked. A char-level refinement would also stop at
        // the parens, so the test that discriminates is the full-text check
        // below: the panes must still render the ORIGINAL text, not the
        // one-word-per-line wordified form.
        let (l, r) = inline_spans_words("foo(bar) baz", "foo(qux) baz");
        assert_eq!(emph_text(&l), "bar");
        assert_eq!(emph_text(&r), "qux");
        assert_eq!(full_text(&l), "foo(bar) baz");
        assert_eq!(full_text(&r), "foo(qux) baz");
    }

    #[test]
    fn wordwise_marks_whole_words_where_chars_would_mark_a_fragment() {
        // `12.5,` is two emacs tokens (`12` type-1, then `.5,` type-2). Changing
        // the digits marks the whole `12` token, not just the differing `2` a
        // character diff would emphasise.
        let (chars_l, _) = inline_spans("a 12.5, b", "a 13.5, b");
        assert_eq!(emph_text(&chars_l), "2");
        let (l, r) = inline_spans_words("a 12.5, b", "a 13.5, b");
        assert_eq!(emph_text(&l), "12");
        assert_eq!(emph_text(&r), "13");
        // White space is not a token in emacs, so it is never emphasised.
        assert_eq!(full_text(&l), "a 12.5, b");
        assert_eq!(full_text(&r), "a 13.5, b");
    }

    #[test]
    fn wordwise_identical_lines_have_no_emphasis() {
        let (l, r) = inline_spans_words("foo(bar) 12.5, baz", "foo(bar) 12.5, baz");
        assert!(l.iter().all(|(_, e)| !e));
        assert!(r.iter().all(|(_, e)| !e));
        assert_eq!(full_text(&l), "foo(bar) 12.5, baz");
    }

    #[test]
    fn inline_common_prefix_and_suffix() {
        // "ab" prefix and "ef" suffix are common; only the middle is emphasised.
        let (l, r) = inline_spans("abcdef", "abXYef");
        assert_eq!(emph_text(&l), "cd");
        assert_eq!(emph_text(&r), "XY");
        // The common prefix/suffix come back un-emphasised.
        assert_eq!(l.first().unwrap(), &("ab".to_string(), false));
        assert_eq!(l.last().unwrap(), &("ef".to_string(), false));
        assert_eq!(r.first().unwrap(), &("ab".to_string(), false));
        assert_eq!(r.last().unwrap(), &("ef".to_string(), false));
    }

    #[test]
    fn inline_pure_insertion_emphasises_added_only() {
        // Right adds a trailing word; left has nothing emphasised.
        let (l, r) = inline_spans("foo", "foo bar");
        assert_eq!(emph_text(&l), "");
        assert_eq!(emph_text(&r), " bar");
    }

    // ── horizontal-offset line slicing ───────────────────────────────────────

    fn cells_text(cells: &[(char, bool)]) -> String {
        cells.iter().map(|(c, _)| *c).collect()
    }
    fn run(s: &str) -> Vec<(String, bool)> {
        vec![(s.to_string(), false)]
    }

    #[test]
    fn layout_pads_short_line_to_width() {
        let cells = layout_cells(&run("abc"), 0, 6);
        assert_eq!(cells.len(), 6);
        assert_eq!(cells_text(&cells), "abc   ");
    }

    #[test]
    fn layout_truncates_long_line_to_width() {
        let cells = layout_cells(&run("abcdefgh"), 0, 4);
        assert_eq!(cells_text(&cells), "abcd");
    }

    #[test]
    fn layout_hscroll_skips_leading_columns() {
        // Skip 2 leading columns, then show 4 (padded).
        let cells = layout_cells(&run("hello"), 2, 4);
        assert_eq!(cells_text(&cells), "llo ");
    }

    #[test]
    fn layout_hscroll_past_end_is_all_blank() {
        // Offset beyond the content clamps to an all-blank, full-width line.
        let cells = layout_cells(&run("hi"), 99, 3);
        assert_eq!(cells_text(&cells), "   ");
        assert_eq!(cells.len(), 3);
    }

    #[test]
    fn layout_expands_tabs_to_four_columns() {
        let cells = layout_cells(&run("\tx"), 0, 6);
        assert_eq!(cells_text(&cells), "    x ");
    }

    #[test]
    fn layout_is_multibyte_safe() {
        // Scrolling over multibyte chars operates on chars, never bytes.
        let cells = layout_cells(&run("héllo"), 1, 4);
        assert_eq!(cells.len(), 4);
        assert_eq!(cells_text(&cells), "éllo");
        // Past-the-end on multibyte content also must not panic.
        let cells = layout_cells(&run("naïve"), 10, 2);
        assert_eq!(cells_text(&cells), "  ");
    }

    #[test]
    fn layout_preserves_emphasis_flags_through_scroll() {
        // Emphasis travels with the chars across hscroll/truncation.
        let runs = vec![("ab".to_string(), false), ("CD".to_string(), true)];
        let cells = layout_cells(&runs, 1, 3);
        // "bCD" — first char un-emphasised, next two emphasised.
        assert_eq!(cells_text(&cells), "bCD");
        assert_eq!(
            cells.iter().map(|(_, e)| *e).collect::<Vec<_>>(),
            vec![false, true, true]
        );
    }

    #[test]
    fn max_line_width_clamps_hscroll() {
        // Longest line is "abcdef" (6 cols); hscroll clamps there.
        let mut view = DiffView::new("f".into(), DocumentId::default(), "abcdef\n", "abcdef\nx\n");
        view.hscroll_by(100);
        assert_eq!(view.hscroll, 6);
        view.hscroll_by(-100);
        assert_eq!(view.hscroll, 0);
    }
}
