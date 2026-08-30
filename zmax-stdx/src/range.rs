//! Provides [Range] type expanding on [RangeBounds].

use std::ops::{self, RangeBounds};

/// A range of `char`s within the text.
#[derive(Debug, Clone, Copy, PartialOrd, Ord, PartialEq, Eq)]
pub struct Range<T = usize> {
    pub start: T,
    pub end: T,
}

impl<T: PartialOrd> Range<T> {
    pub fn contains(&self, other: Self) -> bool {
        self.start <= other.start && other.end <= self.end
    }
    pub fn is_empty(&self) -> bool {
        self.end <= self.start
    }
}

impl<T> RangeBounds<T> for Range<T> {
    fn start_bound(&self) -> ops::Bound<&T> {
        ops::Bound::Included(&self.start)
    }

    fn end_bound(&self) -> ops::Bound<&T> {
        ops::Bound::Excluded(&self.end)
    }
}

/// Returns true if all ranges yielded by `sub_set` are contained by
/// `super_set`. This is essentially an optimized implementation of
/// `sub_set.all(|rb| super_set.any(|ra| ra.contains(rb)))` that runs in O(m+n)
/// instead of O(mn) (and in many cases faster).
///
/// Both iterators must uphold a the following invariants:
/// * ranges must not overlap (but they can be adjacent)
/// * ranges must be sorted
pub fn is_subset<const ALLOW_EMPTY: bool>(
    mut super_set: impl Iterator<Item = Range>,
    mut sub_set: impl Iterator<Item = Range>,
) -> bool {
    let (mut super_range, mut sub_range) = (super_set.next(), sub_set.next());
    loop {
        match (super_range, sub_range) {
            // skip over irrelevant ranges
            (Some(ra), Some(rb))
                if ra.end <= rb.start && (ra.start != rb.start || !ALLOW_EMPTY) =>
            {
                super_range = super_set.next();
            }
            (Some(ra), Some(rb)) => {
                if ra.contains(rb) {
                    sub_range = sub_set.next();
                } else {
                    return false;
                }
            }
            (None, Some(_)) => {
                // exhausted `super_set`, we can't match the reminder of `sub_set`
                return false;
            }
            (_, None) => {
                // no elements from `sub_sut` left to match, `super_set` contains `sub_set`
                return true;
            }
        }
    }
}

/// Similar to is_subset but requires each element of `super_set` to be matched
pub fn is_exact_subset(
    mut super_set: impl Iterator<Item = Range>,
    mut sub_set: impl Iterator<Item = Range>,
) -> bool {
    let (mut super_range, mut sub_range) = (super_set.next(), sub_set.next());
    let mut super_range_matched = true;
    loop {
        match (super_range, sub_range) {
            // skip over irrelevant ranges
            (Some(ra), Some(rb)) if ra.end <= rb.start && ra.start < rb.start => {
                if !super_range_matched {
                    return false;
                }
                super_range_matched = false;
                super_range = super_set.next();
            }
            (Some(ra), Some(rb)) => {
                if ra.contains(rb) {
                    super_range_matched = true;
                    sub_range = sub_set.next();
                } else {
                    return false;
                }
            }
            (None, Some(_)) => {
                // exhausted `super_set`, we can't match the reminder of `sub_set`
                return false;
            }
            (_, None) => {
                // no elements from `sub_sut` left to match, `super_set` contains `sub_set`
                return super_set.next().is_none();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ranges(pairs: &[(usize, usize)]) -> Vec<Range> {
        pairs
            .iter()
            .map(|&(start, end)| Range { start, end })
            .collect()
    }

    fn subset<const ALLOW_EMPTY: bool>(
        super_set: &[(usize, usize)],
        sub_set: &[(usize, usize)],
    ) -> bool {
        is_subset::<ALLOW_EMPTY>(ranges(super_set).into_iter(), ranges(sub_set).into_iter())
    }

    #[test]
    fn contains_is_inclusive_of_both_ends_and_empty_is_end_at_or_before_start() {
        let range = Range { start: 4, end: 8 };

        assert!(range.contains(range), "a range contains itself");
        assert!(range.contains(Range { start: 5, end: 7 }));
        assert!(
            range.contains(Range { start: 4, end: 4 }),
            "empty at the start"
        );
        assert!(
            range.contains(Range { start: 8, end: 8 }),
            "empty at the end"
        );
        assert!(
            !range.contains(Range { start: 3, end: 8 }),
            "starts earlier"
        );
        assert!(!range.contains(Range { start: 4, end: 9 }), "ends later");

        assert!(Range { start: 4, end: 4 }.is_empty());
        assert!(
            Range { start: 5, end: 4 }.is_empty(),
            "inverted counts as empty"
        );
        assert!(!Range { start: 4, end: 5 }.is_empty());
    }

    /// The O(m+n) walk must agree with the naive `all`/`any` definition: every
    /// sub range has to sit inside *one* super range. A sub range that straddles
    /// two adjacent super ranges is contained by neither.
    #[test]
    fn subset_matches_each_sub_range_against_a_single_super_range() {
        assert!(subset::<false>(&[(0, 10), (20, 30)], &[(0, 5), (21, 22)]));
        assert!(subset::<false>(&[(0, 10)], &[(0, 10)]), "exact match");
        assert!(
            !subset::<false>(&[(0, 10), (10, 20)], &[(5, 15)]),
            "adjacent super ranges do not merge to contain a straddling sub range"
        );
        assert!(!subset::<false>(&[(0, 10)], &[(5, 11)]), "sub extends past");
        assert!(!subset::<false>(&[(20, 30)], &[(0, 5)]), "sub before super");
    }

    /// Exhausting the super set with sub ranges left over fails; exhausting the
    /// sub set first succeeds regardless of what is left in the super set.
    #[test]
    fn subset_handles_exhausted_iterators() {
        assert!(subset::<false>(&[], &[]));
        assert!(subset::<false>(&[(0, 10)], &[]), "nothing to contain");
        assert!(!subset::<false>(&[], &[(0, 1)]), "nothing can contain it");
        assert!(
            subset::<false>(&[(0, 10), (20, 30), (40, 50)], &[(1, 2)]),
            "unused super ranges are fine for is_subset"
        );
    }

    /// `ALLOW_EMPTY` is the whole difference between the two call sites: an empty
    /// sub range sitting exactly at an empty super range's start counts as
    /// contained only when it is set. `Selection::contains` passes `true` (a
    /// zero-width cursor is inside a zero-width selection); the snippet code
    /// passes `false`.
    #[test]
    fn allow_empty_decides_whether_a_zero_width_range_is_contained() {
        assert!(subset::<true>(&[(0, 0)], &[(0, 0)]));
        assert!(!subset::<false>(&[(0, 0)], &[(0, 0)]));

        // A zero-width sub range at the start of a non-empty super range is
        // contained either way -- the flag only matters when the *super* range is
        // itself empty and the walk would otherwise skip past it.
        assert!(subset::<true>(&[(4, 8)], &[(4, 4)]));
        assert!(subset::<false>(&[(4, 8)], &[(4, 4)]));
    }

    /// `is_exact_subset` additionally requires every super range to be matched by
    /// at least one sub range -- snippet tabstops must all still be covered, not
    /// merely not-exceeded.
    #[test]
    fn exact_subset_requires_every_super_range_to_be_matched() {
        let exact = |super_set: &[(usize, usize)], sub_set: &[(usize, usize)]| {
            is_exact_subset(ranges(super_set).into_iter(), ranges(sub_set).into_iter())
        };

        assert!(exact(&[(0, 10)], &[(0, 5)]));
        assert!(exact(&[(0, 10), (20, 30)], &[(0, 5), (21, 22)]));
        assert!(
            !exact(&[(0, 10), (20, 30)], &[(0, 5)]),
            "the trailing super range went unmatched"
        );
        assert!(
            !exact(&[(0, 10), (20, 30), (40, 50)], &[(0, 5), (41, 42)]),
            "the middle super range went unmatched"
        );
        assert!(!exact(&[(0, 10)], &[(0, 5), (20, 25)]), "sub outside super");
    }
}
