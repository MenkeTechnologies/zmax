//! Emacs kill-ring with `yank-pop` cycling.
//!
//! zmax stores the latest kill in a register, which overwrites on every
//! kill — so there is no *ring*. This module keeps a bounded ring of recent
//! kills (most-recent first) plus the state `yank-pop` needs to cycle.
//!
//! Population: [`record`] is called from the yank and delete paths in
//! `commands.rs`. Cycling: after `yank-from-kill-ring` selects the inserted
//! text, the selection is remembered; `yank-pop` only fires while that exact
//! selection is still in place (our stand-in for emacs's "last command was a
//! yank" check), replacing it with the next-older entry and re-remembering.

use std::sync::Mutex;

use once_cell::sync::Lazy;

const MAX_ENTRIES: usize = 60;

#[derive(Default)]
struct KillRing {
    /// Most-recent kill first.
    entries: Vec<String>,
    /// Index of the entry currently showing at the yank site while cycling.
    index: usize,
    /// Selection ranges (anchor, head) the last yank/yank-pop left behind.
    /// `yank-pop` only proceeds while the live selection still matches this.
    yank_sel: Vec<(usize, usize)>,
    /// Armed by `append-next-kill` (C-M-w): the *next* kill joins the most-recent
    /// ring entry instead of starting a new one. Consumed by the next kill.
    append_next: bool,
}

static RING: Lazy<Mutex<KillRing>> = Lazy::new(|| Mutex::new(KillRing::default()));

/// Push `text` onto the kill ring for a *forward* kill (called from every
/// kill/copy path). Empty kills and exact-duplicate consecutive kills are
/// ignored, matching emacs's `kill-ring` de-duplication of identical adjacent
/// entries. When `append-next-kill` is armed, the text is appended to the
/// most-recent entry instead of starting a new one.
pub fn record(text: String) {
    record_join(text, true);
}

/// Like [`record`] but for a *backward* kill: when `append-next-kill` is armed,
/// the text is prepended to the most-recent entry (emacs prepends the kill when
/// the command kills backward). Without the arm it is identical to [`record`].
pub fn record_prepend(text: String) {
    record_join(text, false);
}

/// Arm emacs `append-next-kill` (C-M-w): the next kill command joins its text to
/// the most-recent kill-ring entry rather than creating a new entry — appended
/// for a forward kill, prepended for a backward kill.
pub fn arm_append() {
    RING.lock().unwrap().append_next = true;
}

/// Shared implementation for [`record`]/[`record_prepend`]. `forward` selects
/// append-vs-prepend when the `append-next-kill` arm is consumed.
fn record_join(text: String, forward: bool) {
    if text.is_empty() {
        return;
    }
    let mut r = RING.lock().unwrap();
    if r.append_next {
        r.append_next = false;
        if let Some(top) = r.entries.first_mut() {
            if forward {
                top.push_str(&text);
            } else {
                top.insert_str(0, &text);
            }
            return;
        }
        // Ring empty: nothing to join to, fall through and insert as first entry.
    }
    if r.entries.first().map(|s| s == &text).unwrap_or(false) {
        return;
    }
    r.entries.insert(0, text);
    if r.entries.len() > MAX_ENTRIES {
        r.entries.truncate(MAX_ENTRIES);
    }
}

/// The most-recent kill, or `None` if the ring is empty.
pub fn top() -> Option<String> {
    RING.lock().unwrap().entries.first().cloned()
}

/// Begin a yank: index 0 is now showing, remember the selection it occupies.
pub fn begin_yank(sel: Vec<(usize, usize)>) {
    let mut r = RING.lock().unwrap();
    r.index = 0;
    r.yank_sel = sel;
}

/// Advance to the next-older entry for `yank-pop`, but only if `current_sel`
/// still matches what the previous yank left (i.e. nothing else has run since).
/// Returns the entry to swap in, or `None` to signal "previous command was not
/// a yank" / nothing to cycle.
pub fn next_entry(current_sel: &[(usize, usize)]) -> Option<String> {
    let mut r = RING.lock().unwrap();
    if r.entries.len() < 2 || r.yank_sel.is_empty() || r.yank_sel != current_sel {
        return None;
    }
    r.index = (r.index + 1) % r.entries.len();
    r.entries.get(r.index).cloned()
}

/// Record the selection a yank-pop left behind, so a subsequent pop can verify
/// it and keep cycling.
pub fn set_yank_sel(sel: Vec<(usize, usize)>) {
    RING.lock().unwrap().yank_sel = sel;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;

    // The ring is a process-global static; serialize tests so cargo's parallel
    // harness can't interleave one test's `reset` with another's assertions.
    static TEST_GUARD: StdMutex<()> = StdMutex::new(());

    fn reset() {
        let mut r = RING.lock().unwrap();
        *r = KillRing::default();
    }

    #[test]
    fn records_most_recent_first_and_dedupes() {
        let _g = TEST_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        reset();
        record("a".into());
        record("b".into());
        record("b".into()); // consecutive dup ignored
        record("c".into());
        let r = RING.lock().unwrap();
        assert_eq!(r.entries, vec!["c", "b", "a"]);
    }

    #[test]
    fn empty_kill_ignored() {
        let _g = TEST_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        reset();
        record(String::new());
        assert!(top().is_none());
    }

    #[test]
    fn yank_pop_cycles_only_while_selection_matches() {
        let _g = TEST_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        reset();
        record("first".into());
        record("second".into());
        record("third".into()); // ring: [third, second, first]
        begin_yank(vec![(0, 5)]); // "third" sits at 0..5
                                  // wrong selection -> refuses (previous command was not a yank)
        assert_eq!(next_entry(&[(2, 9)]), None);
        // matching selection -> next-older entry
        assert_eq!(next_entry(&[(0, 5)]).as_deref(), Some("second"));
        set_yank_sel(vec![(0, 6)]); // "second" now occupies 0..6
        assert_eq!(next_entry(&[(0, 6)]).as_deref(), Some("first"));
        set_yank_sel(vec![(0, 5)]);
        // wraps back to the most recent
        assert_eq!(next_entry(&[(0, 5)]).as_deref(), Some("third"));
    }

    #[test]
    fn single_entry_does_not_cycle() {
        let _g = TEST_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        reset();
        record("only".into());
        begin_yank(vec![(0, 4)]);
        assert_eq!(next_entry(&[(0, 4)]), None);
    }

    #[test]
    fn append_next_kill_joins_forward_kill_to_top_entry() {
        let _g = TEST_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        reset();
        record("foo".into()); // ring: [foo]
        arm_append();
        record("bar".into()); // forward kill joins -> [foobar], not two entries
        let r = RING.lock().unwrap();
        assert_eq!(r.entries, vec!["foobar"]);
    }

    #[test]
    fn append_next_kill_prepends_backward_kill() {
        let _g = TEST_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        reset();
        record("world".into()); // ring: [world]
        arm_append();
        record_prepend("hello".into()); // backward kill prepends -> [helloworld]
        let r = RING.lock().unwrap();
        assert_eq!(r.entries, vec!["helloworld"]);
    }

    #[test]
    fn arm_is_consumed_by_a_single_kill() {
        let _g = TEST_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        reset();
        record("a".into());
        arm_append();
        record("b".into()); // joins -> [ab]
        record("c".into()); // arm already spent -> new entry
        let r = RING.lock().unwrap();
        assert_eq!(r.entries, vec!["c", "ab"]);
    }

    #[test]
    fn arm_on_empty_ring_inserts_first_entry() {
        let _g = TEST_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        reset();
        arm_append();
        record("solo".into()); // nothing to join to -> plain first entry
        let r = RING.lock().unwrap();
        assert_eq!(r.entries, vec!["solo"]);
    }

    #[test]
    fn empty_kill_does_not_consume_the_arm() {
        let _g = TEST_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        reset();
        record("keep".into());
        arm_append();
        record(String::new()); // empty kill is a no-op, arm survives
        record("join".into()); // this real kill consumes it -> [keepjoin]
        let r = RING.lock().unwrap();
        assert_eq!(r.entries, vec!["keepjoin"]);
    }

    #[test]
    fn append_join_bypasses_adjacent_dedup() {
        let _g = TEST_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        reset();
        record("x".into());
        arm_append();
        record("x".into()); // would be a dup, but the arm joins instead -> [xx]
        let r = RING.lock().unwrap();
        assert_eq!(r.entries, vec!["xx"]);
    }
}
