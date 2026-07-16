//! Emacs mark-ring with `pop-to-mark-command`.
//!
//! Emacs keeps a ring of previous mark positions per buffer; `C-SPC` sets the
//! mark (and pushes the old one onto the ring) and `C-u C-SPC` pops point back
//! through it. zmax has the anchor/head selection (anchor = emacs mark) and a
//! jumplist, but no mark *ring*. This module adds one.
//!
//! Simplification: the ring is process-global rather than per-buffer (like the
//! kill-ring port). Positions are character offsets; the command layer clamps
//! to the current document length before jumping, so a stale offset from
//! another buffer lands safely rather than panicking.

use std::sync::Mutex;

use once_cell::sync::Lazy;

const MAX_MARKS: usize = 16;

static MARKS: Lazy<Mutex<Vec<usize>>> = Lazy::new(|| Mutex::new(Vec::new()));

/// Push a mark position (most-recent first), dropping a consecutive duplicate
/// and bounding the ring.
pub fn push(pos: usize) {
    let mut m = MARKS.lock().unwrap();
    if m.first() == Some(&pos) {
        return;
    }
    m.insert(0, pos);
    if m.len() > MAX_MARKS {
        m.truncate(MAX_MARKS);
    }
}

/// `pop-to-mark`: return the position to jump to and rotate the ring — the
/// front mark is removed and `current` is pushed to the back, so repeated pops
/// walk through the ring (and eventually back to where you started). `None`
/// when the ring is empty.
pub fn pop_to(current: usize) -> Option<usize> {
    let mut m = MARKS.lock().unwrap();
    if m.is_empty() {
        return None;
    }
    let target = m.remove(0);
    m.push(current);
    Some(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    static GUARD: Mutex<()> = Mutex::new(());

    fn reset() {
        MARKS.lock().unwrap().clear();
    }

    #[test]
    fn pop_to_empty_is_none() {
        let _g = GUARD.lock().unwrap_or_else(|e| e.into_inner());
        reset();
        assert_eq!(pop_to(5), None);
    }

    #[test]
    fn push_dedupes_consecutive_and_bounds() {
        let _g = GUARD.lock().unwrap_or_else(|e| e.into_inner());
        reset();
        push(10);
        push(10); // consecutive dup ignored
        push(20);
        assert_eq!(*MARKS.lock().unwrap(), vec![20, 10]);
    }

    #[test]
    fn pop_to_rotates_through_the_ring() {
        let _g = GUARD.lock().unwrap_or_else(|e| e.into_inner());
        reset();
        push(100); // ring (front-first): [200, 100] after next push
        push(200);
        // first pop: jump to 200, current (5) goes to the back
        assert_eq!(pop_to(5), Some(200));
        assert_eq!(*MARKS.lock().unwrap(), vec![100, 5]);
        // second pop: jump to 100
        assert_eq!(pop_to(7), Some(100));
        assert_eq!(*MARKS.lock().unwrap(), vec![5, 7]);
    }
}
