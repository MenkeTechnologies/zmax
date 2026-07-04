//! Pure, editor-type-free algorithms backing the Project mode (`crate::ui::project`
//! in the term crate), the zemacs port of GNU Emacs `project.el`.
//!
//! Everything here is filesystem-free and unit-tested in isolation: the term layer
//! discovers the project root on disk, reads the file list, then calls these to
//! rank the list against a fuzzy query. Prior art: GNU Emacs `project.el`
//! (`project-find-file`, `project-root`, the VC/transient project backends that
//! locate a root by walking up to a marker file).

use std::path::Path;

/// Marker names that identify a directory as a project root, matching the set the
/// term layer probes on disk. Any one present in a directory makes it a root.
/// Ordered roughly by specificity; `.git` first (the common VC case).
pub const PROJECT_MARKERS: &[&str] = &[
    ".git",
    "Cargo.toml",
    "package.json",
    ".hg",
    "Makefile",
    ".project",
];

/// Is `name` one of the recognised project-root markers?
pub fn is_project_marker(name: &str) -> bool {
    PROJECT_MARKERS.contains(&name)
}

/// Locate the project root for `start` from a filesystem-free directory listing.
///
/// `dir_markers` maps directory paths to the names of the entries directly inside
/// them (the term layer builds this from `read_dir`, or in tests it is a fixture).
/// We walk up the ancestors of `start` and return the **nearest** one whose entry
/// list contains a project marker, or `None` if none of the ancestors qualify.
pub fn detect_root(dir_markers: &[(String, Vec<String>)], start: &str) -> Option<String> {
    for ancestor in Path::new(start).ancestors() {
        let anc = ancestor.to_string_lossy();
        if let Some((_, entries)) = dir_markers.iter().find(|(p, _)| p.as_str() == &*anc) {
            if entries.iter().any(|e| is_project_marker(e)) {
                return Some(anc.into_owned());
            }
        }
    }
    None
}

/// Fuzzy subsequence score of `query` against `candidate`.
///
/// Returns `None` when `query` is not a (case-insensitive) subsequence of
/// `candidate`. An empty query always matches with a neutral score of `0`. The
/// score rewards matches at the start / after a path or word boundary
/// (`/ _ - . space`) and rewards runs of consecutive matched characters, so
/// tighter, earlier matches rank above scattered ones. Shorter candidates get a
/// small tie-breaking nudge.
pub fn fuzzy_score(candidate: &str, query: &str) -> Option<i64> {
    if query.is_empty() {
        return Some(0);
    }
    let q: Vec<char> = query.chars().collect();
    let mut qi = 0;
    let mut score: i64 = 0;
    let mut prev_matched = false;
    let mut prev_char: Option<char> = None;
    for (ci, c) in candidate.chars().enumerate() {
        if qi < q.len() && c.eq_ignore_ascii_case(&q[qi]) {
            score += 1;
            if prev_matched {
                score += 10; // consecutive-run bonus (dominates a scattered match)
            }
            let boundary = ci == 0
                || matches!(
                    prev_char,
                    Some('/') | Some('_') | Some('-') | Some('.') | Some(' ')
                );
            if boundary {
                score += 8; // start / word-boundary bonus
            }
            qi += 1;
            prev_matched = true;
        } else {
            prev_matched = false;
            if qi < q.len() {
                score -= 3; // gap before the match is completed
            }
        }
        prev_char = Some(c);
    }
    if qi == q.len() {
        // Prefer shorter candidates on ties.
        score -= candidate.chars().count() as i64 / 10;
        Some(score)
    } else {
        None
    }
}

/// Rank `files` against `query`: keep only the ones that match (a subsequence of
/// the query), sorted by descending [`fuzzy_score`] and then lexically for ties.
/// An empty query keeps every file, in lexical order.
pub fn rank<'a>(files: &'a [String], query: &str) -> Vec<&'a str> {
    let mut scored: Vec<(i64, &str)> = files
        .iter()
        .filter_map(|f| fuzzy_score(f, query).map(|s| (s, f.as_str())))
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(b.1)));
    scored.into_iter().map(|(_, f)| f).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn markers(pairs: &[(&str, &[&str])]) -> Vec<(String, Vec<String>)> {
        pairs
            .iter()
            .map(|(p, es)| (p.to_string(), es.iter().map(|e| e.to_string()).collect()))
            .collect()
    }

    #[test]
    fn detect_root_walks_up_to_nearest_marker() {
        let dirs = markers(&[
            ("/home/u/proj", &["Cargo.toml", "src"]),
            ("/home/u/proj/src", &["main.rs"]),
        ]);
        assert_eq!(
            detect_root(&dirs, "/home/u/proj/src"),
            Some("/home/u/proj".to_string())
        );
    }

    #[test]
    fn detect_root_returns_none_without_a_marker() {
        let dirs = markers(&[
            ("/home/u/proj", &["notes.txt"]),
            ("/home/u/proj/src", &["main.rs"]),
        ]);
        assert_eq!(detect_root(&dirs, "/home/u/proj/src"), None);
    }

    #[test]
    fn detect_root_prefers_the_deepest_marked_ancestor() {
        // Both the outer and inner directories are marked; the nearer one wins.
        let dirs = markers(&[
            ("/w", &[".git"]),
            ("/w/inner", &["Cargo.toml"]),
            ("/w/inner/src", &["lib.rs"]),
        ]);
        assert_eq!(
            detect_root(&dirs, "/w/inner/src"),
            Some("/w/inner".to_string())
        );
    }

    #[test]
    fn fuzzy_score_requires_a_subsequence() {
        assert!(fuzzy_score("src/main.rs", "smain").is_some());
        assert!(fuzzy_score("src/main.rs", "xyz").is_none());
        // Right characters, wrong order -> not a subsequence.
        assert!(fuzzy_score("abc", "cba").is_none());
    }

    #[test]
    fn fuzzy_score_empty_query_is_neutral() {
        assert_eq!(fuzzy_score("anything", ""), Some(0));
    }

    #[test]
    fn fuzzy_score_rewards_consecutive_and_boundary_matches() {
        // A tight, boundary-anchored run beats a scattered subsequence.
        let tight = fuzzy_score("main.rs", "main").unwrap();
        let scattered = fuzzy_score("a_m_a_i_n", "main").unwrap();
        assert!(tight > scattered, "tight={tight} scattered={scattered}");
    }

    #[test]
    fn rank_orders_by_score_then_lexically() {
        let files = vec![
            "src/main.rs".to_string(),
            "lib/domain.rs".to_string(),
            "README.md".to_string(),
        ];
        let ranked = rank(&files, "main");
        // "main" is a subsequence of both paths; the clean `main` prefix in
        // src/main.rs scores higher than the mid-word run in do`main`.rs.
        // README has no `main` subsequence and is filtered out.
        assert_eq!(ranked, vec!["src/main.rs", "lib/domain.rs"]);
    }

    #[test]
    fn rank_empty_query_keeps_all_in_lexical_order() {
        let files = vec!["b.rs".to_string(), "a.rs".to_string(), "c.rs".to_string()];
        let ranked = rank(&files, "");
        assert_eq!(ranked, vec!["a.rs", "b.rs", "c.rs"]);
    }
}
