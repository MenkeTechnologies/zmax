//! Text properties — the persistent, per-region property store that Emacs keeps
//! on buffer text (`put-text-property`, `add-face-text-property`,
//! `remove-text-properties`).
//!
//! Emacs stores properties *on the characters themselves*, so they survive
//! editing, get saved through a format converter (`text/enriched`), and are read
//! by the redisplay engine. zmax had no such store: faces could only come from
//! tree-sitter or a theme scope, which is why `facemenu`, `enriched-mode` and
//! `hide-ifdef-mode` had nothing to write to.
//!
//! This module is the pure, filesystem-free core of that store:
//!
//! * [`Face`] — the face attributes Emacs' `facemenu` can put on a region: a
//!   named face plus the four attribute toggles and two colors.
//! * [`Props`] — everything carried by one run of characters. Today that is a
//!   face and the `invisible` property (Emacs hides text with a non-nil
//!   `invisible` property; zmax renders it with empty-grapheme overlays, the
//!   same mechanism `conceallevel` uses).
//! * [`TextProps`] — a sorted, non-overlapping run list with the add / remove /
//!   query operations, plus [`TextProps::positions_mut`] so `Document::apply`
//!   can map every run boundary through a `ChangeSet` and keep the properties
//!   stuck to their characters across edits.
//!
//! The two Emacs removal commands are genuinely different operations here, which
//! is why `Props` has more than one field: `facemenu-remove-face-props` clears
//! only the face and leaves `invisible` alone, while `facemenu-remove-all` drops
//! the whole run.

use std::ops::Range;

/// An RGB color, as stored in a face text property.
pub type Rgb = (u8, u8, u8);

/// The face attributes `facemenu` can put on a region of text.
///
/// A face is a *delta*: only the attributes that are set are applied, so
/// `facemenu-set-bold` over a region that is already red produces bold red text.
/// [`Face::is_default`] is the "nothing set" state, which is what
/// `facemenu-set-default` installs and what [`TextProps`] prunes away.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Face {
    /// A named face from `list-faces-display` (`facemenu-set-face`), resolved to
    /// a theme scope at render time. `None` when the run only carries attributes.
    pub name: Option<String>,
    /// `facemenu-set-bold`.
    pub bold: bool,
    /// `facemenu-set-italic`.
    pub italic: bool,
    /// `facemenu-set-underline`.
    pub underline: bool,
    /// `facemenu-set-foreground`.
    pub fg: Option<Rgb>,
    /// `facemenu-set-background`.
    pub bg: Option<Rgb>,
}

impl Face {
    /// A face with only the bold attribute — `facemenu-set-bold`.
    pub fn bold() -> Self {
        Face {
            bold: true,
            ..Face::default()
        }
    }

    /// A face with only the italic attribute — `facemenu-set-italic`.
    pub fn italic() -> Self {
        Face {
            italic: true,
            ..Face::default()
        }
    }

    /// `facemenu-set-bold-italic`.
    pub fn bold_italic() -> Self {
        Face {
            bold: true,
            italic: true,
            ..Face::default()
        }
    }

    /// A face with only the underline attribute — `facemenu-set-underline`.
    pub fn underline() -> Self {
        Face {
            underline: true,
            ..Face::default()
        }
    }

    /// A named face — `facemenu-set-face`.
    pub fn named(name: impl Into<String>) -> Self {
        Face {
            name: Some(name.into()),
            ..Face::default()
        }
    }

    /// `facemenu-set-foreground`.
    pub fn foreground(rgb: Rgb) -> Self {
        Face {
            fg: Some(rgb),
            ..Face::default()
        }
    }

    /// `facemenu-set-background`.
    pub fn background(rgb: Rgb) -> Self {
        Face {
            bg: Some(rgb),
            ..Face::default()
        }
    }

    /// True when no attribute is set, i.e. the text renders exactly as it would
    /// with no face property at all. `facemenu-set-default` puts this face on a
    /// region, which is the same as removing the face.
    pub fn is_default(&self) -> bool {
        self == &Face::default()
    }

    /// Layer `delta` on top of `self`, Emacs `add-face-text-property` style: an
    /// attribute the delta sets wins, an attribute it leaves unset is inherited.
    pub fn merge(&mut self, delta: &Face) {
        if delta.name.is_some() {
            self.name = delta.name.clone();
        }
        self.bold |= delta.bold;
        self.italic |= delta.italic;
        self.underline |= delta.underline;
        if delta.fg.is_some() {
            self.fg = delta.fg;
        }
        if delta.bg.is_some() {
            self.bg = delta.bg;
        }
    }
}

/// Every property carried by one run of characters.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Props {
    /// The `face` / `font-lock-face` text property.
    pub face: Face,
    /// The `invisible` text property: the run is not displayed at all.
    pub invisible: bool,
}

impl Props {
    /// True when the run carries nothing and can be dropped from the store.
    pub fn is_empty(&self) -> bool {
        self.face.is_default() && !self.invisible
    }
}

/// One run of characters carrying [`Props`]. `start..end` is a char range into
/// the document, half-open, and runs never overlap.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    /// First char of the run.
    pub start: usize,
    /// One past the last char of the run.
    pub end: usize,
    /// What the run carries.
    pub props: Props,
}

/// A document's text properties: a sorted, non-overlapping list of runs.
///
/// The invariant enforced by every mutator is: runs are sorted by `start`, no
/// two runs overlap, no run is empty (`start < end`), no run carries empty
/// [`Props`], and no two *adjacent* runs carry equal props (they get coalesced).
/// [`TextProps::spans`] therefore is a canonical form: two `TextProps` describing
/// the same properties compare equal.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextProps {
    spans: Vec<Span>,
}

impl TextProps {
    /// An empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// True when no character carries any property.
    pub fn is_empty(&self) -> bool {
        self.spans.is_empty()
    }

    /// The runs, sorted and non-overlapping.
    pub fn spans(&self) -> &[Span] {
        &self.spans
    }

    /// Drop every property in the buffer.
    pub fn clear(&mut self) {
        self.spans.clear();
    }

    /// The props at char `pos`, or `None` when the char carries none.
    pub fn props_at(&self, pos: usize) -> Option<&Props> {
        self.spans
            .iter()
            .find(|s| s.start <= pos && pos < s.end)
            .map(|s| &s.props)
    }

    /// The runs that overlap `range`, clipped to it.
    pub fn spans_in(&self, range: Range<usize>) -> impl Iterator<Item = Span> + '_ {
        self.spans
            .iter()
            .filter(move |s| s.start < range.end && range.start < s.end)
            .map(move |s| Span {
                start: s.start.max(range.start),
                end: s.end.min(range.end),
                props: s.props.clone(),
            })
    }

    /// Every char index in an `invisible` run, ascending. This is what the view
    /// turns into empty-grapheme overlays.
    pub fn invisible_chars(&self) -> impl Iterator<Item = usize> + '_ {
        self.spans
            .iter()
            .filter(|s| s.props.invisible)
            .flat_map(|s| s.start..s.end)
    }

    /// Every run boundary, for `ChangeSet::update_positions`. The caller maps the
    /// positions through the change set and then calls [`TextProps::repair`].
    pub fn positions_mut(&mut self) -> impl Iterator<Item = &mut usize> {
        self.spans
            .iter_mut()
            .flat_map(|s| [&mut s.start, &mut s.end])
    }

    /// Restore the invariants after the run boundaries were moved externally
    /// (i.e. by `ChangeSet::update_positions`): clamp to `len`, drop runs that
    /// collapsed to nothing, re-sort and coalesce.
    pub fn repair(&mut self, len: usize) {
        for span in &mut self.spans {
            span.start = span.start.min(len);
            span.end = span.end.min(len);
        }
        self.spans
            .retain(|s| s.start < s.end && !s.props.is_empty());
        self.spans.sort_by_key(|s| (s.start, s.end));
        self.coalesce();
    }

    /// Apply `f` to the props of every char in `range`, splitting runs at the
    /// range boundaries and materialising runs for the uncovered gaps. This is
    /// the single primitive every public mutator is built from.
    fn update_range(&mut self, range: Range<usize>, f: impl Fn(&mut Props)) {
        if range.start >= range.end {
            return;
        }
        let mut out: Vec<Span> = Vec::with_capacity(self.spans.len() + 2);
        // `cursor` walks the range, emitting a fresh run for every gap between
        // the existing runs that overlap it.
        let mut cursor = range.start;
        for span in self.spans.drain(..) {
            if span.end <= range.start || span.start >= range.end {
                out.push(span);
                continue;
            }
            // The part of the run before the range keeps its props untouched.
            if span.start < range.start {
                out.push(Span {
                    start: span.start,
                    end: range.start,
                    props: span.props.clone(),
                });
            }
            // Any gap between the last run and this one gets a brand-new run.
            let overlap_start = span.start.max(range.start);
            if cursor < overlap_start {
                let mut props = Props::default();
                f(&mut props);
                out.push(Span {
                    start: cursor,
                    end: overlap_start,
                    props,
                });
            }
            let overlap_end = span.end.min(range.end);
            let mut props = span.props.clone();
            f(&mut props);
            out.push(Span {
                start: overlap_start,
                end: overlap_end,
                props,
            });
            cursor = overlap_end;
            // The part of the run after the range keeps its props untouched.
            if span.end > range.end {
                out.push(Span {
                    start: range.end,
                    end: span.end,
                    props: span.props,
                });
            }
        }
        if cursor < range.end {
            let mut props = Props::default();
            f(&mut props);
            out.push(Span {
                start: cursor,
                end: range.end,
                props,
            });
        }
        out.retain(|s| s.start < s.end && !s.props.is_empty());
        out.sort_by_key(|s| (s.start, s.end));
        self.spans = out;
        self.coalesce();
    }

    /// Merge adjacent runs that carry equal props. Keeps [`TextProps::spans`]
    /// canonical so equal property sets compare equal.
    fn coalesce(&mut self) {
        let mut out: Vec<Span> = Vec::with_capacity(self.spans.len());
        for span in self.spans.drain(..) {
            match out.last_mut() {
                Some(prev) if prev.end == span.start && prev.props == span.props => {
                    prev.end = span.end;
                }
                _ => out.push(span),
            }
        }
        self.spans = out;
    }

    /// Emacs `add-face-text-property`: layer `delta`'s attributes on top of
    /// whatever face the region already has. This is what every `facemenu-set-*`
    /// command does.
    pub fn add_face(&mut self, range: Range<usize>, delta: &Face) {
        if delta.is_default() {
            // `facemenu-set-default` — a face with nothing set means "no face".
            self.remove_face(range);
            return;
        }
        self.update_range(range, |props| props.face.merge(delta));
    }

    /// Replace the face on the region outright, discarding whatever was there.
    pub fn set_face(&mut self, range: Range<usize>, face: &Face) {
        self.update_range(range, |props| props.face = face.clone());
    }

    /// Emacs `facemenu-remove-face-props`: drop the `face` property from the
    /// region and leave every other property (notably `invisible`) alone.
    pub fn remove_face(&mut self, range: Range<usize>) {
        self.update_range(range, |props| props.face = Face::default());
    }

    /// Emacs `facemenu-remove-all`: drop *every* text property from the region.
    pub fn remove_all(&mut self, range: Range<usize>) {
        self.update_range(range, |props| *props = Props::default());
    }

    /// Emacs `put-text-property ... 'invisible`: hide (or reveal) the region.
    pub fn set_invisible(&mut self, range: Range<usize>, invisible: bool) {
        self.update_range(range, move |props| props.invisible = invisible);
    }

    /// Drop the `invisible` property everywhere (Emacs `show-ifdefs` /
    /// `sgml-tags-invisible` toggling back off), leaving faces intact.
    pub fn clear_invisible(&mut self) {
        for span in &mut self.spans {
            span.props.invisible = false;
        }
        self.spans.retain(|s| !s.props.is_empty());
        self.coalesce();
    }

    /// True when any char is hidden.
    pub fn has_invisible(&self) -> bool {
        self.spans.iter().any(|s| s.props.invisible)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn faces(tp: &TextProps) -> Vec<(usize, usize, Face)> {
        tp.spans()
            .iter()
            .map(|s| (s.start, s.end, s.props.face.clone()))
            .collect()
    }

    #[test]
    fn add_face_on_empty_store_creates_one_run() {
        let mut tp = TextProps::new();
        tp.add_face(2..5, &Face::bold());
        assert_eq!(faces(&tp), vec![(2, 5, Face::bold())]);
    }

    #[test]
    fn empty_or_inverted_range_is_a_no_op() {
        let mut tp = TextProps::new();
        tp.add_face(3..3, &Face::bold());
        // An inverted range (end < start) reaches these functions whenever a
        // caller hands over a reversed selection; it must not panic or corrupt
        // the run list. `#[allow]` because clippy is right that the literal range
        // is empty — that is what is being tested.
        #[allow(clippy::reversed_empty_ranges)]
        tp.add_face(5..2, &Face::bold());
        assert!(tp.is_empty());
    }

    #[test]
    fn overlapping_adds_merge_attributes_only_in_the_overlap() {
        let mut tp = TextProps::new();
        tp.add_face(0..6, &Face::bold());
        tp.add_face(4..10, &Face::italic());
        assert_eq!(
            faces(&tp),
            vec![
                (0, 4, Face::bold()),
                (4, 6, Face::bold_italic()),
                (6, 10, Face::italic()),
            ]
        );
    }

    #[test]
    fn a_face_applied_inside_a_run_splits_it_in_three() {
        let mut tp = TextProps::new();
        tp.add_face(0..10, &Face::bold());
        tp.add_face(4..6, &Face::underline());
        let mut both = Face::bold();
        both.underline = true;
        assert_eq!(
            faces(&tp),
            vec![(0, 4, Face::bold()), (4, 6, both), (6, 10, Face::bold())]
        );
    }

    #[test]
    fn colors_are_last_writer_wins_and_survive_attribute_adds() {
        let mut tp = TextProps::new();
        tp.add_face(0..4, &Face::foreground((255, 0, 0)));
        tp.add_face(0..4, &Face::foreground((0, 255, 0)));
        tp.add_face(0..4, &Face::bold());
        let expect = Face {
            bold: true,
            fg: Some((0, 255, 0)),
            ..Face::default()
        };
        assert_eq!(faces(&tp), vec![(0, 4, expect)]);
    }

    #[test]
    fn set_default_removes_the_face() {
        let mut tp = TextProps::new();
        tp.add_face(0..8, &Face::bold());
        tp.add_face(2..4, &Face::default());
        assert_eq!(faces(&tp), vec![(0, 2, Face::bold()), (4, 8, Face::bold())]);
    }

    #[test]
    fn adjacent_equal_runs_coalesce_into_one() {
        let mut tp = TextProps::new();
        tp.add_face(0..3, &Face::bold());
        tp.add_face(3..6, &Face::bold());
        assert_eq!(faces(&tp), vec![(0, 6, Face::bold())]);
    }

    #[test]
    fn runs_never_overlap_after_arbitrary_adds() {
        let mut tp = TextProps::new();
        for (a, b) in [(0, 9), (3, 5), (7, 12), (1, 2), (4, 11), (0, 20)] {
            tp.add_face(a..b, &Face::italic());
        }
        let spans = tp.spans();
        for pair in spans.windows(2) {
            assert!(
                pair[0].end <= pair[1].start,
                "runs overlap: {:?} {:?}",
                pair[0],
                pair[1]
            );
        }
        assert!(spans.iter().all(|s| s.start < s.end));
    }

    #[test]
    fn remove_face_keeps_invisible_but_remove_all_does_not() {
        let mut tp = TextProps::new();
        tp.add_face(0..6, &Face::bold());
        tp.set_invisible(0..6, true);

        let mut only_face_removed = tp.clone();
        only_face_removed.remove_face(2..4);
        assert_eq!(only_face_removed.props_at(3).unwrap().face, Face::default());
        assert!(
            only_face_removed.props_at(3).unwrap().invisible,
            "facemenu-remove-face-props must leave `invisible` alone"
        );

        let mut all_removed = tp;
        all_removed.remove_all(2..4);
        assert!(
            all_removed.props_at(3).is_none(),
            "facemenu-remove-all must drop every property"
        );
        assert_eq!(all_removed.props_at(1).unwrap().face, Face::bold());
    }

    #[test]
    fn invisible_chars_lists_every_hidden_index() {
        let mut tp = TextProps::new();
        tp.set_invisible(2..5, true);
        tp.set_invisible(8..9, true);
        assert_eq!(tp.invisible_chars().collect::<Vec<_>>(), vec![2, 3, 4, 8]);
        assert!(tp.has_invisible());
        tp.clear_invisible();
        assert!(!tp.has_invisible());
        assert!(tp.is_empty());
    }

    #[test]
    fn clear_invisible_preserves_faces() {
        let mut tp = TextProps::new();
        tp.add_face(0..4, &Face::bold());
        tp.set_invisible(0..4, true);
        tp.clear_invisible();
        assert_eq!(faces(&tp), vec![(0, 4, Face::bold())]);
    }

    #[test]
    fn repair_clamps_and_drops_collapsed_runs() {
        let mut tp = TextProps::new();
        tp.add_face(0..4, &Face::bold());
        tp.add_face(10..20, &Face::italic());
        // Simulate a deletion that collapsed the second run and truncated the buffer.
        for (i, pos) in tp.positions_mut().enumerate() {
            if i >= 2 {
                *pos = 6;
            }
        }
        tp.repair(6);
        assert_eq!(faces(&tp), vec![(0, 4, Face::bold())]);
    }

    #[test]
    fn spans_in_clips_to_the_query_range() {
        let mut tp = TextProps::new();
        tp.add_face(0..10, &Face::bold());
        let got: Vec<_> = tp.spans_in(4..7).collect();
        assert_eq!(got.len(), 1);
        assert_eq!((got[0].start, got[0].end), (4, 7));
    }

    #[test]
    fn named_face_overrides_a_previous_name() {
        let mut tp = TextProps::new();
        tp.add_face(0..4, &Face::named("font-lock-string-face"));
        tp.add_face(0..4, &Face::named("error"));
        assert_eq!(tp.props_at(1).unwrap().face.name.as_deref(), Some("error"));
    }

    #[test]
    fn set_face_replaces_rather_than_merges() {
        let mut tp = TextProps::new();
        tp.add_face(0..4, &Face::bold());
        tp.set_face(0..4, &Face::italic());
        assert_eq!(faces(&tp), vec![(0, 4, Face::italic())]);
    }
}
