mod date_time;
mod integer;

pub fn integer(selected_text: &str, amount: i64) -> Option<String> {
    integer::increment(selected_text, amount)
}

pub fn date_time(selected_text: &str, amount: i64) -> Option<String> {
    date_time::increment(selected_text, amount)
}

/// Increment a single ASCII letter within its case, clamping at the boundary
/// (vim `nrformats+=alpha`): `a`→`b`, `Z`+1 stays `Z`, `a`-1 stays `a`. Only a
/// lone alphabetic character qualifies; anything else returns `None`.
pub fn alpha(selected_text: &str, amount: i64) -> Option<String> {
    let mut chars = selected_text.chars();
    let c = chars.next()?;
    if chars.next().is_some() || !c.is_ascii_alphabetic() {
        return None;
    }
    let base = if c.is_ascii_lowercase() { b'a' } else { b'A' };
    let idx = (c as u8 - base) as i64;
    let new = (idx + amount).clamp(0, 25) as u8;
    Some(((base + new) as char).to_string())
}

#[cfg(test)]
mod tests {
    use super::alpha;

    #[test]
    fn alpha_increments_within_case_and_clamps() {
        assert_eq!(alpha("a", 1).as_deref(), Some("b"));
        assert_eq!(alpha("A", 1).as_deref(), Some("B"));
        assert_eq!(alpha("y", 3).as_deref(), Some("z")); // clamps at z, no wrap
        assert_eq!(alpha("z", 1).as_deref(), Some("z"));
        assert_eq!(alpha("a", -5).as_deref(), Some("a")); // clamps at a
        assert_eq!(alpha("c", -1).as_deref(), Some("b"));
        // Not a lone letter.
        assert_eq!(alpha("ab", 1), None);
        assert_eq!(alpha("5", 1), None);
        assert_eq!(alpha("", 1), None);
    }
}
