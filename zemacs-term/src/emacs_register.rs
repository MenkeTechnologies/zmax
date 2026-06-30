//! Emacs position and number registers (`C-x r`).
//!
//! zemacs already has *text* registers (the `"`/`y`/`p` system) and named marks,
//! but not emacs's `C-x r SPC` point registers or `C-x r n` number registers,
//! which store a buffer position or an integer under a single character. This
//! module is that store; the command layer (`commands.rs`) reads the register
//! character via `on_next_key` and jumps/inserts.
//!
//! Process-global, like the kill/mark rings — a simplification of emacs's
//! per-frame registers. Positions are clamped to the live buffer before use.

use std::collections::HashMap;
use std::sync::Mutex;

use once_cell::sync::Lazy;

#[derive(Clone, Copy)]
enum RegVal {
    Pos(usize),
    Num(i64),
}

static REGS: Lazy<Mutex<HashMap<char, RegVal>>> = Lazy::new(|| Mutex::new(HashMap::new()));

/// Store a buffer position (`point-to-register`).
pub fn set_pos(ch: char, pos: usize) {
    REGS.lock().unwrap().insert(ch, RegVal::Pos(pos));
}

/// Read a position register (`jump-to-register`); `None` if unset or non-position.
pub fn get_pos(ch: char) -> Option<usize> {
    match REGS.lock().unwrap().get(&ch) {
        Some(RegVal::Pos(p)) => Some(*p),
        _ => None,
    }
}

/// Store a number (`number-to-register`).
pub fn set_num(ch: char, n: i64) {
    REGS.lock().unwrap().insert(ch, RegVal::Num(n));
}

/// Read a number register; `None` if unset or non-number.
pub fn get_num(ch: char) -> Option<i64> {
    match REGS.lock().unwrap().get(&ch) {
        Some(RegVal::Num(n)) => Some(*n),
        _ => None,
    }
}

/// Increment a number register by `by` (`increment-register`), treating an unset
/// or non-number register as starting from 0. Returns the new value.
pub fn incr(ch: char, by: i64) -> i64 {
    let mut regs = REGS.lock().unwrap();
    let cur = match regs.get(&ch) {
        Some(RegVal::Num(n)) => *n,
        _ => 0,
    };
    let next = cur + by;
    regs.insert(ch, RegVal::Num(next));
    next
}

#[cfg(test)]
mod tests {
    use super::*;

    static GUARD: Mutex<()> = Mutex::new(());

    fn reset() {
        REGS.lock().unwrap().clear();
    }

    #[test]
    fn position_round_trips_and_is_typed() {
        let _g = GUARD.lock().unwrap_or_else(|e| e.into_inner());
        reset();
        set_pos('a', 42);
        assert_eq!(get_pos('a'), Some(42));
        assert_eq!(get_num('a'), None); // a position is not a number
        assert_eq!(get_pos('z'), None); // unset
    }

    #[test]
    fn number_round_trips_and_is_typed() {
        let _g = GUARD.lock().unwrap_or_else(|e| e.into_inner());
        reset();
        set_num('n', 7);
        assert_eq!(get_num('n'), Some(7));
        assert_eq!(get_pos('n'), None);
    }

    #[test]
    fn increment_starts_from_zero_and_accumulates() {
        let _g = GUARD.lock().unwrap_or_else(|e| e.into_inner());
        reset();
        assert_eq!(incr('c', 1), 1); // unset -> 0 + 1
        assert_eq!(incr('c', 5), 6);
        assert_eq!(get_num('c'), Some(6));
        // incrementing a position register overwrites from 0 (emacs treats it numerically)
        set_pos('p', 100);
        assert_eq!(incr('p', 2), 2);
    }
}
