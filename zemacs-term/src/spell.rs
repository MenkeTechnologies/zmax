//! Minimal spell checker backing the vim `z=` / `zg` / `zw` / `[s` / `]s` family.
//!
//! The base word list is loaded once from the system dictionary
//! (`/usr/share/dict/words`, present on macOS and most Linux installs). User
//! additions made with `zg` (good) and `zw` (wrong) persist to
//! `<config-dir>/spell-good` and `<config-dir>/spell-bad`, mirroring vim's
//! `spellfile`. If no system dictionary is found, nothing is ever flagged
//! misspelled (so the feature degrades to a no-op rather than firing on every
//! word).

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{OnceLock, RwLock};

fn good_path() -> PathBuf {
    zemacs_loader::config_dir().join("spell-good")
}
fn bad_path() -> PathBuf {
    zemacs_loader::config_dir().join("spell-bad")
}

fn dict() -> &'static HashSet<String> {
    static DICT: OnceLock<HashSet<String>> = OnceLock::new();
    DICT.get_or_init(|| {
        let mut set = HashSet::new();
        for path in ["/usr/share/dict/words", "/usr/share/dict/web2"] {
            if let Ok(contents) = std::fs::read_to_string(path) {
                for line in contents.lines() {
                    let w = line.trim();
                    if !w.is_empty() {
                        set.insert(w.to_lowercase());
                    }
                }
                break;
            }
        }
        set
    })
}

fn load_words(path: PathBuf) -> HashSet<String> {
    std::fs::read_to_string(path)
        .map(|c| c.lines().map(|l| l.trim().to_lowercase()).filter(|w| !w.is_empty()).collect())
        .unwrap_or_default()
}

fn user_good() -> &'static RwLock<HashSet<String>> {
    static G: OnceLock<RwLock<HashSet<String>>> = OnceLock::new();
    G.get_or_init(|| RwLock::new(load_words(good_path())))
}
fn user_bad() -> &'static RwLock<HashSet<String>> {
    static B: OnceLock<RwLock<HashSet<String>>> = OnceLock::new();
    B.get_or_init(|| RwLock::new(load_words(bad_path())))
}

fn persist(path: PathBuf, set: &HashSet<String>) {
    let mut words: Vec<&String> = set.iter().collect();
    words.sort();
    let body = words.iter().map(|w| w.as_str()).collect::<Vec<_>>().join("\n");
    let _ = std::fs::write(path, body);
}

/// Is `word` worth flagging? Short tokens, anything non-alphabetic, and words in
/// the good list are never flagged; words in the bad list always are.
pub fn is_misspelled(word: &str) -> bool {
    if dict().is_empty() {
        return false;
    }
    let w = word.to_lowercase();
    if w.chars().count() < 2 || !w.chars().all(|c| c.is_alphabetic()) {
        return false;
    }
    if user_bad().read().unwrap().contains(&w) {
        return true;
    }
    if user_good().read().unwrap().contains(&w) {
        return false;
    }
    !dict().contains(&w)
}

/// `zg`: mark a word as correctly spelled (and forget any prior `zw`).
pub fn add_good(word: &str) {
    let w = word.to_lowercase();
    {
        let mut g = user_good().write().unwrap();
        g.insert(w.clone());
        persist(good_path(), &g);
    }
    let mut b = user_bad().write().unwrap();
    if b.remove(&w) {
        persist(bad_path(), &b);
    }
}

/// `zw`: mark a word as incorrectly spelled.
pub fn add_bad(word: &str) {
    let w = word.to_lowercase();
    {
        let mut b = user_bad().write().unwrap();
        b.insert(w.clone());
        persist(bad_path(), &b);
    }
    let mut g = user_good().write().unwrap();
    if g.remove(&w) {
        persist(good_path(), &g);
    }
}

/// `zug` / `zuw`: undo a previous `zg`/`zw` for the word.
pub fn remove_user(word: &str) {
    let w = word.to_lowercase();
    let mut g = user_good().write().unwrap();
    if g.remove(&w) {
        persist(good_path(), &g);
    }
    let mut b = user_bad().write().unwrap();
    if b.remove(&w) {
        persist(bad_path(), &b);
    }
}

/// `z=`: suggestions for `word` — dictionary words within edit distance 1,
/// preserving the original capitalization style.
pub fn suggest(word: &str) -> Vec<String> {
    let lower = word.to_lowercase();
    let dict = dict();
    if dict.is_empty() {
        return Vec::new();
    }
    let alphabet = "abcdefghijklmnopqrstuvwxyz";
    let chars: Vec<char> = lower.chars().collect();
    let mut cands: HashSet<String> = HashSet::new();

    // deletions
    for i in 0..chars.len() {
        let mut s: String = chars[..i].iter().collect();
        s.extend(&chars[i + 1..]);
        if dict.contains(&s) {
            cands.insert(s);
        }
    }
    // substitutions + insertions
    for i in 0..=chars.len() {
        for a in alphabet.chars() {
            // insertion at i
            let mut ins: String = chars[..i].iter().collect();
            ins.push(a);
            ins.extend(&chars[i..]);
            if dict.contains(&ins) {
                cands.insert(ins);
            }
            // substitution at i
            if i < chars.len() {
                let mut sub: String = chars[..i].iter().collect();
                sub.push(a);
                sub.extend(&chars[i + 1..]);
                if dict.contains(&sub) {
                    cands.insert(sub);
                }
            }
        }
    }
    // transpositions
    for i in 0..chars.len().saturating_sub(1) {
        let mut t = chars.clone();
        t.swap(i, i + 1);
        let s: String = t.into_iter().collect();
        if dict.contains(&s) {
            cands.insert(s);
        }
    }

    cands.remove(&lower);
    let mut out: Vec<String> = cands.into_iter().map(|s| match_case(word, &s)).collect();
    out.sort();
    out.truncate(25);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_and_suggests() {
        if dict().is_empty() {
            return; // no system dictionary on this box — feature degrades to no-op
        }
        assert!(is_misspelled("teh"), "'teh' should be flagged");
        assert!(!is_misspelled("the"), "'the' should be accepted");
        assert!(!is_misspelled("a"), "single letters are never flagged");
        assert!(!is_misspelled("x86"), "non-alphabetic tokens are skipped");
        let s = suggest("teh");
        assert!(
            s.contains(&"the".to_string()) || s.contains(&"ten".to_string()),
            "expected a plausible suggestion for 'teh', got {s:?}"
        );
        // capitalization is preserved
        assert_eq!(match_case("Teh", "the"), "The");
        assert_eq!(match_case("TEH", "the"), "THE");
    }
}

/// Apply `model`'s capitalization (all-caps or Title-case) to `candidate`.
fn match_case(model: &str, candidate: &str) -> String {
    if model.chars().all(|c| c.is_uppercase()) && model.chars().any(|c| c.is_alphabetic()) {
        candidate.to_uppercase()
    } else if model.chars().next().is_some_and(|c| c.is_uppercase()) {
        let mut cs = candidate.chars();
        match cs.next() {
            Some(f) => f.to_uppercase().collect::<String>() + cs.as_str(),
            None => candidate.to_string(),
        }
    } else {
        candidate.to_string()
    }
}
