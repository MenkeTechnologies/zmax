//! Emacs buffer-name uniquification, backing `rename-buffer` and
//! `rename-uniquely` (Emacs `Misc Buffer`). Emacs buffer names are unique by
//! construction: `generate-new-buffer-name` returns the requested name when it
//! is free, otherwise it appends `<2>`, `<3>`, … until a free name is found.
//! `rename-uniquely` first strips any existing `<N>` suffix, then generates a
//! fresh unique name.
//!
//! These functions are pure: name generation is decoupled from the editor's
//! document set via an `is_taken` predicate so they can be unit-tested in
//! isolation.

/// Strip a trailing `<N>` uniquify suffix (`N` = one or more ASCII digits) from
/// `name`, as Emacs `rename-uniquely` does before re-generating. Names without
/// such a suffix are returned unchanged. Only a well-formed `<digits>` tail is
/// removed — `foo<bar>` and `foo<>` keep their tail.
pub fn strip_uniquify_suffix(name: &str) -> &str {
    let Some(inner) = name.strip_suffix('>') else {
        return name;
    };
    let Some(open) = inner.rfind('<') else {
        return name;
    };
    let digits = &inner[open + 1..];
    if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
        &name[..open]
    } else {
        name
    }
}

/// Emacs `generate-new-buffer-name`: return `base` if `is_taken(base)` is false,
/// otherwise the first of `base<2>`, `base<3>`, … for which `is_taken` is false.
///
/// `is_taken` reports whether a candidate name is already in use by some buffer.
/// The suffix count starts at 2 to match Emacs (the un-suffixed name is the
/// implicit "1").
pub fn generate_unique_name(base: &str, is_taken: impl Fn(&str) -> bool) -> String {
    if !is_taken(base) {
        return base.to_string();
    }
    let mut n = 2u32;
    loop {
        let candidate = format!("{base}<{n}>");
        if !is_taken(&candidate) {
            return candidate;
        }
        n += 1;
    }
}

/// Emacs `rename-uniquely` on `current`: strip any `<N>` suffix, then generate a
/// unique name against `is_taken`. `is_taken` must report `true` for the current
/// buffer's own name so an already-unique name still gains a `<2>` suffix (the
/// buffer occupies its own name), matching Emacs behaviour.
pub fn rename_uniquely(current: &str, is_taken: impl Fn(&str) -> bool) -> String {
    generate_unique_name(strip_uniquify_suffix(current), is_taken)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_suffix_removes_only_well_formed_numeric_tail() {
        assert_eq!(strip_uniquify_suffix("foo<2>"), "foo");
        assert_eq!(strip_uniquify_suffix("foo<10>"), "foo");
        assert_eq!(strip_uniquify_suffix("a<b><3>"), "a<b>");
        // not a numeric suffix -> unchanged
        assert_eq!(strip_uniquify_suffix("foo<bar>"), "foo<bar>");
        assert_eq!(strip_uniquify_suffix("foo<>"), "foo<>");
        assert_eq!(strip_uniquify_suffix("foo"), "foo");
        assert_eq!(strip_uniquify_suffix("<2>"), "");
    }

    #[test]
    fn free_name_is_returned_unchanged() {
        assert_eq!(generate_unique_name("main.rs", |_| false), "main.rs");
    }

    #[test]
    fn taken_name_gets_lowest_free_numeric_suffix() {
        let taken = ["main.rs", "main.rs<2>", "main.rs<3>"];
        let got = generate_unique_name("main.rs", |c| taken.contains(&c));
        assert_eq!(got, "main.rs<4>");
    }

    #[test]
    fn suffix_search_skips_only_occupied_numbers() {
        // <2> free even though the base is taken.
        let taken = ["log"];
        assert_eq!(
            generate_unique_name("log", |c| taken.contains(&c)),
            "log<2>"
        );
    }

    #[test]
    fn rename_uniquely_strips_then_regenerates() {
        // "notes<2>" -> strip -> "notes"; "notes" still occupied by self -> "notes<2>".
        let taken = ["notes", "notes<2>"];
        assert_eq!(
            rename_uniquely("notes<2>", |c| taken.contains(&c)),
            "notes<3>"
        );
    }

    #[test]
    fn rename_uniquely_forces_suffix_when_already_unique() {
        // A buffer that occupies its own name (is_taken true for it) still gets a suffix.
        assert_eq!(rename_uniquely("scratch", |c| c == "scratch"), "scratch<2>");
    }
}
