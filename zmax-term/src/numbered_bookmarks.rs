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

/// The slots, keyed by document. The ten ne/mcedit digits, plus the letters
/// IntelliJ's mnemonic bookmarks add on top of them.
static SLOTS: Mutex<Option<HashMap<DocumentId, HashMap<u8, usize>>>> = Mutex::new(None);

/// The characters a bookmark can live under: ne and mcedit's ten digits, then
/// the letters of IntelliJ's "Toggle Bookmark with Mnemonic" (`Ctrl-F11`).
///
/// The digits come FIRST and keep slot numbers 0–9, so every position ne and
/// mcedit could address still answers to the same key and the same slot.
pub const SLOT_DIGITS: &str = "0123456789abcdefghijklmnopqrstuvwxyz";

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
    with(|slots| slots.get_mut(&doc).and_then(|d| d.remove(&slot)).is_some())
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

/// The key a slot answers to — the inverse of [`slot_of`], for listings.
pub fn char_of(slot: u8) -> Option<char> {
    SLOT_DIGITS.chars().nth(usize::from(slot))
}

/// Parse a bookmark key (digit or letter) from a typed key. A capital is taken
/// as its lowercase, so Shift does not silently address a different slot.
pub fn slot_of(ch: char) -> Option<u8> {
    let ch = ch.to_ascii_lowercase();
    SLOT_DIGITS
        .find(ch)
        .map(|index| u8::try_from(index).expect("thirty-six slots fit in a u8"))
}

#[cfg(test)]
mod test {
    use super::*;

    /// The digits keep the slots ne and mcedit gave them, and the mnemonic
    /// letters continue from there — so an old binding addresses the old slot.
    #[test]
    fn digits_keep_their_slots_and_letters_follow() {
        assert_eq!(slot_of('0'), Some(0));
        assert_eq!(slot_of('9'), Some(9));
        assert_eq!(slot_of('a'), Some(10));
        assert_eq!(slot_of('z'), Some(35));
        // A capital addresses the same slot as its lowercase, so Shift cannot
        // silently land on a different bookmark.
        assert_eq!(slot_of('A'), slot_of('a'));
        assert_eq!(slot_of('-'), None);
        assert_eq!(slot_of(' '), None);
    }

    #[test]
    fn every_slot_reports_the_key_it_answers_to() {
        for ch in SLOT_DIGITS.chars() {
            let slot = slot_of(ch).expect("every slot char parses");
            assert_eq!(char_of(slot), Some(ch));
        }
        assert_eq!(char_of(36), None);
    }

    /// `DocumentId` has no public constructor, so every test here addresses the
    /// same id in one process-wide store — which means they must not run at the
    /// same time. `cargo test` runs a module's tests in parallel by default, and
    /// two of these did interfere before this guard existed.
    static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

    fn doc() -> DocumentId {
        DocumentId::default()
    }

    fn guard() -> std::sync::MutexGuard<'static, ()> {
        ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn a_slot_holds_a_line_until_it_is_replaced_or_unset() {
        let _serial = guard();
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
        let _serial = guard();
        let doc = doc();
        forget(doc);
        set(doc, 9, 90);
        set(doc, 1, 10);
        set(doc, 5, 50);
        assert_eq!(list(doc), vec![(1, 10), (5, 50), (9, 90)]);
        forget(doc);
        assert!(list(doc).is_empty());
    }

    /// Was `only_the_ten_digits_are_slots`, pinning `slot_of('a') == None`.
    /// The letters are slots now (IntelliJ's mnemonic bookmarks), so what this
    /// pins is the boundary that still holds: nothing OUTSIDE `0-9a-z` is one.
    #[test]
    fn only_digits_and_letters_are_slots() {
        assert_eq!(slot_of('0'), Some(0));
        assert_eq!(slot_of('9'), Some(9));
        assert_eq!(slot_of('a'), Some(10));
        assert_eq!(slot_of(' '), None);
        assert_eq!(slot_of('-'), None);
        assert_eq!(slot_of('\u{e9}'), None);
    }
}
