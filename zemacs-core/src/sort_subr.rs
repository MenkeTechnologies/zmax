//! Faithful port of emacs `sort-subr` (sort.el) — the generic engine that
//! divides a region into records, sorts them by key, and rewrites the region.
//!
//! The key subtlety (`sort-reorder-buffer`): records are permuted into their
//! ORIGINAL slots while the inter-record gaps stay fixed in place. Emacs walks
//! the original record order, emitting for each slot the gap that precedes it
//! plus the sorted record's contents. This module ports that reorder plus the
//! `sort-pages` record builder (records = `^L`-delimited pages, gaps = the
//! newlines skipped between them). String keys compare by code point
//! (`string<`, `sort-fold-case` nil).

/// One sort record: its `[start, end)` char range in the region and its key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub start: usize,
    pub end: usize,
    pub key: String,
}

/// Reorder `text` by permuting the record contents into their original slots by
/// key, leaving the gaps between records unchanged (`sort-reorder-buffer`).
/// `reverse` sorts descending; equal keys keep their original order (stable).
pub fn reorder(text: &str, records: &[Record], reverse: bool) -> String {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let mut order: Vec<usize> = (0..records.len()).collect();
    if reverse {
        order.sort_by(|&a, &b| records[b].key.cmp(&records[a].key));
    } else {
        order.sort_by(|&a, &b| records[a].key.cmp(&records[b].key));
    }
    let mut out = String::with_capacity(text.len());
    let mut last = 0usize;
    for (i, rec) in records.iter().enumerate() {
        // The gap that precedes this original slot stays in place.
        out.extend(chars[last..rec.start.min(n)].iter());
        // The sorted record's contents go into the slot.
        let s = &records[order[i]];
        out.extend(chars[s.start.min(n)..s.end.min(n)].iter());
        last = rec.end.min(n);
    }
    out.extend(chars[last..n].iter());
    out
}

/// Build the records for `sort-pages`: each record is a `^L`-delimited page
/// (from a page start through the next form feed, inclusive), and the newlines
/// skipped between pages are the gaps. Mirrors `sort-build-lists` driven by
/// `nextrecfun = skip \n` and `endrecfun = forward-page`.
pub fn page_records(text: &str) -> Vec<Record> {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let mut recs = Vec::new();
    let mut point = 0usize;
    while point < n {
        let start = point;
        let end = crate::page::forward_page(text, start);
        let key: String = chars[start..end].iter().collect();
        recs.push(Record { start, end, key });
        // nextrecfun: skip the newlines between this page and the next.
        let mut p = end;
        while p < n && chars[p] == '\n' {
            p += 1;
        }
        point = p;
    }
    recs
}

/// `sort-pages`: sort the `^L`-delimited pages in `text` alphabetically (by page
/// text), `reverse` for descending. Convenience over [`page_records`]+[`reorder`].
pub fn sort_pages(text: &str, reverse: bool) -> String {
    let records = page_records(text);
    if records.len() < 2 {
        return text.to_string();
    }
    reorder(text, &records, reverse)
}

#[cfg(test)]
mod tests {
    use super::*;

    // All three outputs captured from GNU Emacs 30.2 `sort-pages` via `od -c`
    // (the form feed is \u{000C}).
    #[test]
    fn sort_pages_matches_emacs() {
        assert_eq!(
            sort_pages("banana\nbb\n\u{000C}apple\naa\n\u{000C}cherry\ncc\n", false),
            "apple\naa\n\u{000C}banana\nbb\n\u{000C}cherry\ncc\n"
        );
        assert_eq!(
            sort_pages("banana\n\u{000C}apple\n\u{000C}cherry\n", true),
            "cherry\nbanana\n\u{000C}apple\n\u{000C}"
        );
        // The pathological case that a naive split diverges on: a page led by
        // blank lines keeps them in place as a gap.
        assert_eq!(
            sort_pages("\u{000C}\n\nzebra\n\u{000C}apple\n", false),
            "\u{000C}\n\napple\nzebra\n\u{000C}"
        );
    }

    #[test]
    fn reorder_preserves_gaps_and_is_stable() {
        // Two records "b" and "a" separated by a gap " | "; sorting swaps their
        // contents but the gap stays between the slots.
        let text = "b | a";
        let records = vec![
            Record {
                start: 0,
                end: 1,
                key: "b".into(),
            },
            Record {
                start: 4,
                end: 5,
                key: "a".into(),
            },
        ];
        assert_eq!(reorder(text, &records, false), "a | b");
        assert_eq!(reorder(text, &records, true), "b | a");
    }
}
