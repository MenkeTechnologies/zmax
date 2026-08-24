//! Emacs `repeat-mode` (repeat.el): after a command bound to a sequence of two
//! or more keys has run, the *last* key of that sequence alone runs it again —
//! `C-x u u u…` to keep undoing, `C-x o o o…` to keep switching windows,
//! `C-x { } ^ v` to keep resizing.
//!
//! Emacs decides which keys continue a repetition from the `repeat-map` symbol
//! property of the command that just ran, which names a keymap of single-key
//! shortcuts. zmax has no per-command property list, but it has the very keymap
//! the chord came out of: after `C-x o`, the transient map is the `C-x` submap
//! itself. That reproduces the manual's examples exactly — `u`, `o`, `{`, `}`,
//! `^` and `v` are all leaves of the same `C-x` node — without inventing a
//! second table that could drift from the keymap.
//!
//! The state here is *what to replay*: the prefix keys of the chord that armed
//! the repetition. [`crate::ui::editor::EditorView`] feeds them back through the
//! keymap ahead of the repeat key, so the repetition runs through exactly the
//! same path as typing the whole chord again.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use zmax_view::input::{KeyCode, KeyEvent};

/// Emacs `repeat-mode`: off until turned on, as in Emacs.
static REPEAT_MODE: AtomicBool = AtomicBool::new(false);

/// Emacs's `repeat` command (`C-x z`) installs a transient map of its own — a
/// bare `z` repeats again — and it does that whether or not `repeat-mode` is on.
/// This flag is that one-shot repetition, so `C-x z z z` works with the mode
/// off, which is how Emacs ships.
static COMMAND_REPEAT: AtomicBool = AtomicBool::new(false);

/// The prefix keys of the chord that armed the current repetition, e.g. `[C-x]`
/// after `C-x o`. `None` when no repetition is in flight.
static ARMED: Mutex<Option<Vec<KeyEvent>>> = Mutex::new(None);

/// Whether `repeat-mode` is on.
pub fn enabled() -> bool {
    REPEAT_MODE.load(Ordering::Relaxed)
}

/// Toggle `repeat-mode`, returning the new state. Turning it off also ends any
/// repetition already in flight.
pub fn toggle() -> bool {
    let on = !REPEAT_MODE.fetch_xor(true, Ordering::Relaxed);
    if !on {
        disarm();
    }
    on
}

/// Emacs `repeat-exit-key`: the key that ends the transient repeating mode
/// *without* executing itself. Emacs leaves this unset and suggests `RET`; zmax
/// takes the suggestion, since a bare `RET` inside a repetition has no other
/// useful meaning.
pub fn is_exit_key(event: KeyEvent) -> bool {
    event.code == KeyCode::Enter && event.modifiers.is_empty()
}

/// Arm a repetition: `prefix` is the chord minus its final key. A single-key
/// chord arms nothing — there is no shorter way to type it.
pub fn arm(prefix: &[KeyEvent]) {
    if !enabled() || prefix.is_empty() {
        return;
    }
    if let Ok(mut armed) = ARMED.lock() {
        *armed = Some(prefix.to_vec());
    }
}

/// Arm the repetition `repeat` (`C-x z`) installs itself: same replay, but not
/// gated on `repeat-mode`. `prefix` is the chord minus its final key, so a bare
/// `z` after `C-x z` re-runs `C-x z`.
pub fn arm_command_repeat(prefix: &[KeyEvent]) {
    if prefix.is_empty() {
        return;
    }
    COMMAND_REPEAT.store(true, Ordering::Relaxed);
    if let Ok(mut armed) = ARMED.lock() {
        *armed = Some(prefix.to_vec());
    }
}

/// The prefix to replay before the repeat key, if a repetition is in flight.
pub fn armed() -> Option<Vec<KeyEvent>> {
    if !enabled() && !COMMAND_REPEAT.load(Ordering::Relaxed) {
        return None;
    }
    ARMED.lock().ok().and_then(|a| a.clone())
}

/// End the current repetition.
pub fn disarm() {
    COMMAND_REPEAT.store(false, Ordering::Relaxed);
    if let Ok(mut armed) = ARMED.lock() {
        *armed = None;
    }
}

/// The echo-area hint Emacs shows while a repetition is live ("the single-key
/// shortcuts are shown in the echo area"). `keys` are the single keys the
/// transient map accepts.
pub fn hint(keys: &[String]) -> String {
    format!("Repeat with {}", keys.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use zmax_view::input::KeyModifiers;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
        }
    }

    /// `REPEAT_MODE` and `ARMED` are process-global, so the arming rules are
    /// asserted in one test rather than in several that would race each other.
    #[test]
    fn arming_rules() {
        // Nothing arms while the mode is off.
        disarm();
        REPEAT_MODE.store(false, Ordering::Relaxed);
        arm(&[key(KeyCode::Char('x'))]);
        assert!(armed().is_none());

        // A single-key chord (an empty prefix) arms nothing either — there is no
        // shorter way to type it.
        REPEAT_MODE.store(true, Ordering::Relaxed);
        arm(&[]);
        assert!(armed().is_none());

        // A two-key chord arms its prefix...
        arm(&[key(KeyCode::Char('x'))]);
        assert_eq!(armed(), Some(vec![key(KeyCode::Char('x'))]));
        // ...and turning the mode off ends the repetition.
        toggle();
        assert!(armed().is_none());
        REPEAT_MODE.store(false, Ordering::Relaxed);
        disarm();

        // `repeat`'s own transient map is not gated on the mode: with it off,
        // arming through `arm_command_repeat` still replays, and disarming ends
        // it.
        arm_command_repeat(&[key(KeyCode::Char('x'))]);
        assert_eq!(armed(), Some(vec![key(KeyCode::Char('x'))]));
        disarm();
        assert!(armed().is_none());
        // An empty prefix arms nothing there either.
        arm_command_repeat(&[]);
        assert!(armed().is_none());
    }

    #[test]
    fn ret_is_the_exit_key() {
        assert!(is_exit_key(key(KeyCode::Enter)));
        assert!(!is_exit_key(key(KeyCode::Char('u'))));
        // A modified RET is not the exit key.
        assert!(!is_exit_key(KeyEvent {
            code: KeyCode::Enter,
            modifiers: KeyModifiers::CONTROL,
        }));
    }

    #[test]
    fn hint_lists_the_transient_keys() {
        assert_eq!(hint(&["u".into(), "o".into()]), "Repeat with u, o");
    }
}
