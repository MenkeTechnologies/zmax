//! Numbered bookmarks — ne's `SetBookmark` / `GotoBookmark` / `UnsetBookmark`
//! and mcedit's bookmark keys.
//!
//! Both editors give a *document* ten numbered slots (`0`–`9`) holding a
//! position you jump back to, which is a different thing from emacs bookmarks
//! (named, global, persisted across files) and from the jumplist (a history, not
//! a set of slots). zmax had the other two and not this one, so the ne and
//! mcedit bookmark rows were mapped partial against the jumplist.
//!
//! A bookmark remembers the *line*, as ne's does: an edit above it should carry
//! it along rather than leave it pointing into the middle of some other line.

use std::collections::HashMap;
use std::sync::Mutex;

use zmax_view::DocumentId;

/// The slots, keyed by document. Ten per document, as both editors offer.
static SLOTS: Mutex<Option<HashMap<DocumentId, HashMap<u8, usize>>>> = Mutex::new(None);

/// The digits a bookmark can live under.
pub const SLOT_DIGITS: &str = "0123456789";

fn with<R>(f: impl FnOnce(&mut HashMap<DocumentId, HashMap<u8, usize>>) -> R) -> R {
    let mut guard = match SLOTS.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    f(guard.get_or_insert_with(HashMap::new))
}

/// Remember `line` under `slot` for `doc`. Returns the line it replaced, if any
/// — ne reports when a bookmark is moved rather than set.
pub fn set(doc: DocumentId, slot: u8, line: usize) -> Option<usize> {
    with(|slots| slots.entry(doc).or_default().insert(slot, line))
}

/// The line held under `slot`, if the document has one.
pub fn get(doc: DocumentId, slot: u8) -> Option<usize> {
    with(|slots| slots.get(&doc).and_then(|d| d.get(&slot).copied()))
}

/// ne `UnsetBookmark`: forget one slot. Returns whether there was one.
pub fn unset(doc: DocumentId, slot: u8) -> bool {
    with(|slots| {
        slots
            .get_mut(&doc)
            .and_then(|d| d.remove(&slot))
            .is_some()
    })
}

/// Every slot a document holds, lowest digit first — for listing them.
pub fn list(doc: DocumentId) -> Vec<(u8, usize)> {
    with(|slots| {
        let mut rows: Vec<(u8, usize)> = slots
            .get(&doc)
            .map(|d| d.iter().map(|(k, v)| (*k, *v)).collect())
            .unwrap_or_default();
        rows.sort_unstable();
        rows
    })
}

/// Drop a document's slots when it closes, so a reused id cannot inherit them.
pub fn forget(doc: DocumentId) {
    with(|slots| slots.remove(&doc));
}

/// Parse a bookmark digit from a typed key.
pub fn slot_of(ch: char) -> Option<u8> {
    SLOT_DIGITS
        .find(ch)
        .map(|index| u8::try_from(index).expect("ten slots fit in a u8"))
}

#[cfg(test)]
mod test {
    use super::*;

    /// `DocumentId` has no public constructor, so the tests drive the store
    /// through the ids the editor hands out; `Default` is one real id.
    fn doc() -> DocumentId {
        DocumentId::default()
    }

    #[test]
    fn a_slot_holds_a_line_until_it_is_replaced_or_unset() {
        let doc = doc();
        forget(doc);
        assert_eq!(get(doc, 3), None);
        assert_eq!(set(doc, 3, 42), None, "nothing was there before");
        assert_eq!(get(doc, 3), Some(42));
        // Setting again reports the line it displaced, which is what ne says.
        assert_eq!(set(doc, 3, 7), Some(42));
        assert_eq!(get(doc, 3), Some(7));
        assert!(unset(doc, 3));
        assert!(!unset(doc, 3), "already gone");
        assert_eq!(get(doc, 3), None);
    }

    #[test]
    fn slots_are_independent_and_listed_in_order() {
        let doc = doc();
        forget(doc);
        set(doc, 9, 90);
        set(doc, 1, 10);
        set(doc, 5, 50);
        assert_eq!(list(doc), vec![(1, 10), (5, 50), (9, 90)]);
        forget(doc);
        assert!(list(doc).is_empty());
    }

    #[test]
    fn only_the_ten_digits_are_slots() {
        assert_eq!(slot_of('0'), Some(0));
        assert_eq!(slot_of('9'), Some(9));
        assert_eq!(slot_of('a'), None);
        assert_eq!(slot_of(' '), None);
    }
}
