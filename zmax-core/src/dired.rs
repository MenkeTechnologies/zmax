//! Pure, editor-type-free algorithms backing the Dired directory-editor mode
//! (`crate::ui::dired` in the term crate). Everything here is filesystem-free and
//! unit-tested in isolation: the term layer reads the directory into
//! [`DiredEntry`] values, calls these to sort / format / transform them, and
//! renders the result. Prior art: GNU Emacs Dired (sorting `s`, `% R`/`% u`/`% l`
//! name transforms, human-readable sizes).

use std::path::{Path, PathBuf};

/// One directory entry as Dired needs it. `mtime` is seconds since the Unix
/// epoch (only used as a sort key, so its absolute value is irrelevant).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiredEntry {
    pub name: String,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub size: u64,
    pub mtime: i64,
}

/// Dired sort orders (Emacs `s` cycles name/time; we add size/extension).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortKey {
    Name,
    Size,
    Time,
    Ext,
}

impl SortKey {
    /// The order Emacs-style `s` cycles through.
    pub fn next(self) -> SortKey {
        match self {
            SortKey::Name => SortKey::Time,
            SortKey::Time => SortKey::Size,
            SortKey::Size => SortKey::Ext,
            SortKey::Ext => SortKey::Name,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            SortKey::Name => "name",
            SortKey::Size => "size",
            SortKey::Time => "time",
            SortKey::Ext => "ext",
        }
    }
}

/// The extension (lowercased, without the dot) used for `SortKey::Ext`. A
/// leading dot (dotfile) is not treated as an extension separator.
pub fn extension(name: &str) -> String {
    match name.rfind('.') {
        Some(i) if i > 0 => name[i + 1..].to_ascii_lowercase(),
        _ => String::new(),
    }
}

/// Sort entries in place. Directories always sort before files (Emacs
/// `dired-listing-switches` "--group-directories-first" style, which zmax Dired
/// uses unconditionally); within each group the `key` decides, then name breaks
/// ties. `reverse` flips the within-group order but keeps dirs-first.
pub fn sort_entries(entries: &mut [DiredEntry], key: SortKey, reverse: bool) {
    entries.sort_by(|a, b| {
        // dirs first, regardless of key/reverse
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| {
                let ord = match key {
                    SortKey::Name => a
                        .name
                        .to_ascii_lowercase()
                        .cmp(&b.name.to_ascii_lowercase()),
                    SortKey::Size => a.size.cmp(&b.size),
                    // most-recent first is the useful default for time
                    SortKey::Time => b.mtime.cmp(&a.mtime),
                    SortKey::Ext => extension(&a.name).cmp(&extension(&b.name)).then_with(|| {
                        a.name
                            .to_ascii_lowercase()
                            .cmp(&b.name.to_ascii_lowercase())
                    }),
                };
                if reverse {
                    ord.reverse()
                } else {
                    ord
                }
            })
            .then_with(|| a.name.cmp(&b.name))
    });
}

/// Human-readable byte size like `ls -lh` (1024-based, `K`/`M`/`G`/…). Bytes
/// under 1024 render as the bare number; larger values carry one decimal unless
/// the value is >= 10 in its unit.
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["", "K", "M", "G", "T", "P"];
    if bytes < 1024 {
        return bytes.to_string();
    }
    let mut v = bytes as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if v >= 10.0 {
        format!("{:.0}{}", v, UNITS[u])
    } else {
        format!("{:.1}{}", v, UNITS[u])
    }
}

/// Emacs Dired name transforms applied by `% u` (upcase), `% l` (downcase) and
/// `% R`/`% C` regexp rename (here a literal find/replace of the first match,
/// keeping the transform dependency-free). Directories are transformed the same
/// as files.
#[derive(Clone, Copy)]
pub enum NameTransform<'a> {
    Upcase,
    Downcase,
    /// Replace the first occurrence of `from` with `to`.
    Replace {
        from: &'a str,
        to: &'a str,
    },
}

pub fn transform_name(name: &str, t: NameTransform) -> String {
    match t {
        NameTransform::Upcase => name.to_uppercase(),
        NameTransform::Downcase => name.to_lowercase(),
        NameTransform::Replace { from, to } => {
            if from.is_empty() {
                name.to_string()
            } else if let Some(i) = name.find(from) {
                let mut out = String::with_capacity(name.len());
                out.push_str(&name[..i]);
                out.push_str(to);
                out.push_str(&name[i + from.len()..]);
                out
            } else {
                name.to_string()
            }
        }
    }
}

/// The single mark character Dired shows in the left column for an entry given
/// its mark/flag state: `D` flagged for deletion (takes precedence), `*` marked,
/// else a space. Mirrors Emacs Dired's leftmost column.
pub fn mark_char(marked: bool, flagged: bool) -> char {
    if flagged {
        'D'
    } else if marked {
        '*'
    } else {
        ' '
    }
}

/// Emacs `dired-flag-backup-files` predicate: a GNU-style backup name, i.e. one
/// ending in `~` (both `foo~` and versioned `foo.~3~`).
pub fn is_backup_file(name: &str) -> bool {
    name.ends_with('~')
}

/// Emacs `dired-flag-auto-save-files` predicate: an auto-save file `#name#`.
pub fn is_auto_save_file(name: &str) -> bool {
    name.len() > 2 && name.starts_with('#') && name.ends_with('#')
}

/// Extensions matched by Emacs `dired-garbage-files-regexp` (the dired-x
/// default): intermediate build/tex droppings.
const GARBAGE_EXTENSIONS: &[&str] = &["aux", "bak", "dvi", "log", "orig", "rej", "toc", "out"];

/// Emacs `dired-flag-garbage-files` predicate: a backup/auto-save file, or a
/// name whose extension is one of `GARBAGE_EXTENSIONS`.
pub fn is_garbage_file(name: &str) -> bool {
    is_backup_file(name)
        || is_auto_save_file(name)
        || GARBAGE_EXTENSIONS.contains(&extension(name).as_str())
}

/// Emacs `dired-mark-executables` predicate on a Unix permission `mode`: true
/// when any of the owner/group/other execute bits (`0o111`) are set.
pub fn is_executable_mode(mode: u32) -> bool {
    mode & 0o111 != 0
}

/// Count and total byte size of the entries whose name satisfies `is_marked` —
/// backs Emacs `dired-number-of-marked-files` (the `* N` display).
pub fn marked_summary(entries: &[DiredEntry], is_marked: impl Fn(&str) -> bool) -> (usize, u64) {
    entries
        .iter()
        .filter(|e| is_marked(&e.name))
        .fold((0usize, 0u64), |(c, b), e| (c + 1, b + e.size))
}

/// Resolve the on-disk destination for copying/renaming/linking a file named
/// `src_name` to the user-supplied `dest`. When `dest` is an existing directory
/// the file keeps its basename inside it (Emacs Dired "Copy to: `<dir>`/"
/// behaviour); otherwise `dest` is taken as the literal target path.
pub fn destination_path(dest: &Path, dest_is_dir: bool, src_name: &str) -> PathBuf {
    if dest_is_dir {
        dest.join(src_name)
    } else {
        dest.to_path_buf()
    }
}

/// Parse an octal file mode as typed for Emacs `dired-do-chmod` (e.g. `"755"`,
/// `"0644"`). Returns `None` for empty input, non-octal digits, or overflow.
pub fn parse_octal_mode(s: &str) -> Option<u32> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    u32::from_str_radix(s, 8).ok()
}

/// Whether `name` is usable as a new single-component file name for the
/// create/rename prompts: non-empty, not the `.`/`..` self/parent entries, and
/// containing no path separator.
pub fn is_valid_filename(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains('/')
}

/// Index of the next (`forward`) or previous directory entry relative to `from`,
/// backing Emacs `dired-next-dirline` / `dired-prev-dirline`. Searches strictly
/// away from `from` without wrapping; `None` if there is no further directory in
/// that direction.
pub fn next_dir_index(entries: &[DiredEntry], from: usize, forward: bool) -> Option<usize> {
    if forward {
        ((from + 1)..entries.len()).find(|&i| entries[i].is_dir)
    } else {
        (0..from.min(entries.len()))
            .rev()
            .find(|&i| entries[i].is_dir)
    }
}

/// Apply an Emacs Dired regexp name transform (`% R`/`% C`/`% H`/`% S`) to
/// `name`: rewrite the first match of `re` using `replacement`, where the
/// replacement uses Emacs backslash syntax — `\&` = whole match, `\1`..`\9` =
/// capture group N, `\\` = a literal backslash. Text before and after the match
/// is preserved (the regexp need not anchor the whole name). Returns `None` when
/// `re` does not match `name`, mirroring Emacs, which only acts on matching files.
pub fn regexp_replace_name(name: &str, re: &regex::Regex, replacement: &str) -> Option<String> {
    let caps = re.captures(name)?;
    let whole = caps.get(0)?;
    let mut out = String::with_capacity(name.len());
    out.push_str(&name[..whole.start()]);
    let mut chars = replacement.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('&') => out.push_str(whole.as_str()),
                Some(d @ '0'..='9') => {
                    let idx = d as usize - '0' as usize;
                    if let Some(g) = caps.get(idx) {
                        out.push_str(g.as_str());
                    }
                }
                Some('\\') => out.push('\\'),
                Some(other) => out.push(other),
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out.push_str(&name[whole.end()..]);
    Some(out)
}

/// Parse a GNU-style *numbered* backup name `base.~N~` into its base and version
/// number, backing Emacs `dired-clean-directory`. A plain `foo~` (unnumbered) is
/// not a numbered backup and returns `None`.
pub fn parse_numbered_backup(name: &str) -> Option<(&str, u32)> {
    let inner = name.strip_suffix('~')?;
    let sep = inner.rfind(".~")?;
    let digits = &inner[sep + 2..];
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let version: u32 = digits.parse().ok()?;
    Some((&name[..sep], version))
}

/// Emacs `dired-clean-directory`: given a set of file names, return the numbered
/// backups that should be flagged for deletion — for each base name, all but the
/// `keep` highest-numbered versions (Emacs `dired-kept-versions`, default 2). The
/// result is the excess backup file names (order unspecified by Emacs; we return
/// them base-grouped, oldest-first within a base).
pub fn backups_to_clean(names: &[String], keep: usize) -> Vec<String> {
    use std::collections::BTreeMap;
    let mut groups: BTreeMap<&str, Vec<(u32, &str)>> = BTreeMap::new();
    for n in names {
        if let Some((base, version)) = parse_numbered_backup(n) {
            groups.entry(base).or_default().push((version, n.as_str()));
        }
    }
    let mut out = Vec::new();
    for versions in groups.values_mut() {
        // Highest version first; the `keep` newest survive, the rest are flagged.
        versions.sort_by_key(|v| std::cmp::Reverse(v.0));
        for (_, name) in versions.iter().skip(keep) {
            out.push((*name).to_string());
        }
    }
    out
}

/// Emacs `dired-compare-directories` default: the names in `here` that differ
/// from directory `there` — present here but missing there, or present in both
/// with a different size or file-vs-directory kind. (Emacs' default predicate
/// also weighs mtime; size+kind+presence is the portable, copy-stable subset.)
pub fn dirs_differ(here: &[DiredEntry], there: &[DiredEntry]) -> Vec<String> {
    here.iter()
        .filter(|a| match there.iter().find(|b| b.name == a.name) {
            None => true,
            Some(b) => b.size != a.size || b.is_dir != a.is_dir,
        })
        .map(|a| a.name.clone())
        .collect()
}

/// Extract the file-name token surrounding byte offset `pos` in `text`, backing
/// Emacs `dired-at-point` / ffap. A token is a maximal run of non-whitespace
/// characters excluding the shell/markup delimiters `"'`()<>[]{},;:` — enough to
/// pick a path out of surrounding prose or brackets. Returns `None` when point is
/// not on such a token.
pub fn filename_at_point(text: &str, pos: usize) -> Option<String> {
    let bytes = text.as_bytes();
    let is_fname = |b: u8| {
        !b.is_ascii_whitespace()
            && !matches!(
                b,
                b'"' | b'\''
                    | b'`'
                    | b'('
                    | b')'
                    | b'<'
                    | b'>'
                    | b'['
                    | b']'
                    | b'{'
                    | b'}'
                    | b','
                    | b';'
                    | b':'
            )
    };
    let pos = pos.min(bytes.len());
    let mut start = pos;
    while start > 0 && is_fname(bytes[start - 1]) {
        start -= 1;
    }
    let mut end = pos;
    while end < bytes.len() && is_fname(bytes[end]) {
        end += 1;
    }
    if start == end {
        None
    } else {
        Some(text[start..end].to_string())
    }
}

/// Compute the relative path from directory `from_dir` to `target` (both taken as
/// already-absolute, normalized paths), for Emacs `dired-do-relsymlink` — the
/// symlink stores `../foo` style targets rather than absolute ones. Falls back to
/// `.` when the two are identical.
pub fn relative_path(from_dir: &Path, target: &Path) -> PathBuf {
    let from: Vec<_> = from_dir.components().collect();
    let to: Vec<_> = target.components().collect();
    let mut common = 0;
    while common < from.len() && common < to.len() && from[common] == to[common] {
        common += 1;
    }
    let mut result = PathBuf::new();
    for _ in common..from.len() {
        result.push("..");
    }
    for c in &to[common..] {
        result.push(c.as_os_str());
    }
    if result.as_os_str().is_empty() {
        result.push(".");
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(name: &str, is_dir: bool, size: u64, mtime: i64) -> DiredEntry {
        DiredEntry {
            name: name.into(),
            is_dir,
            is_symlink: false,
            size,
            mtime,
        }
    }

    #[test]
    fn dirs_sort_before_files_then_by_name() {
        let mut v = vec![
            e("zebra.txt", false, 10, 1),
            e("alpha", true, 0, 5),
            e("beta.rs", false, 20, 2),
            e("Gamma", true, 0, 3),
        ];
        sort_entries(&mut v, SortKey::Name, false);
        let names: Vec<&str> = v.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(names, vec!["alpha", "Gamma", "beta.rs", "zebra.txt"]);
    }

    #[test]
    fn size_sort_keeps_dirs_first() {
        let mut v = vec![
            e("big.bin", false, 5000, 1),
            e("d", true, 0, 1),
            e("small.txt", false, 5, 1),
        ];
        sort_entries(&mut v, SortKey::Size, false);
        let names: Vec<&str> = v.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(names, vec!["d", "small.txt", "big.bin"]);
    }

    #[test]
    fn reverse_flips_within_group_not_dirs() {
        let mut v = vec![
            e("a.txt", false, 1, 1),
            e("dir", true, 0, 1),
            e("b.txt", false, 2, 1),
        ];
        sort_entries(&mut v, SortKey::Name, true);
        let names: Vec<&str> = v.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(names, vec!["dir", "b.txt", "a.txt"]);
    }

    #[test]
    fn ext_sort_groups_by_extension() {
        let mut v = vec![
            e("main.rs", false, 1, 1),
            e("readme.md", false, 1, 1),
            e("lib.rs", false, 1, 1),
        ];
        sort_entries(&mut v, SortKey::Ext, false);
        let names: Vec<&str> = v.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(names, vec!["readme.md", "lib.rs", "main.rs"]);
    }

    #[test]
    fn human_sizes() {
        assert_eq!(human_size(0), "0");
        assert_eq!(human_size(512), "512");
        assert_eq!(human_size(1024), "1.0K");
        assert_eq!(human_size(1536), "1.5K");
        assert_eq!(human_size(10 * 1024), "10K");
        assert_eq!(human_size(1024 * 1024), "1.0M");
        assert_eq!(human_size(5 * 1024 * 1024 * 1024), "5.0G");
    }

    #[test]
    fn extension_ignores_leading_dot() {
        assert_eq!(extension(".gitignore"), "");
        assert_eq!(extension("a.tar.gz"), "gz");
        assert_eq!(extension("noext"), "");
    }

    #[test]
    fn name_transforms() {
        assert_eq!(
            transform_name("Foo.TXT", NameTransform::Downcase),
            "foo.txt"
        );
        assert_eq!(transform_name("foo", NameTransform::Upcase), "FOO");
        assert_eq!(
            transform_name(
                "img_001.jpeg",
                NameTransform::Replace {
                    from: "jpeg",
                    to: "jpg"
                }
            ),
            "img_001.jpg"
        );
    }

    #[test]
    fn mark_chars() {
        assert_eq!(mark_char(false, false), ' ');
        assert_eq!(mark_char(true, false), '*');
        assert_eq!(mark_char(false, true), 'D');
        assert_eq!(mark_char(true, true), 'D'); // flag wins
    }

    #[test]
    fn garbage_predicates() {
        assert!(is_backup_file("foo.c~"));
        assert!(is_backup_file("foo.~3~"));
        assert!(!is_backup_file("foo.c"));

        assert!(is_auto_save_file("#foo.c#"));
        assert!(!is_auto_save_file("foo.c"));
        assert!(!is_auto_save_file("#")); // too short to be #name#

        assert!(is_garbage_file("paper.aux"));
        assert!(is_garbage_file("paper.log"));
        assert!(is_garbage_file("foo.c~")); // backups count as garbage
        assert!(is_garbage_file("#foo.c#")); // auto-saves count as garbage
        assert!(!is_garbage_file("paper.tex"));
        assert!(!is_garbage_file("main.rs"));
    }

    #[test]
    fn executable_mode_bits() {
        assert!(is_executable_mode(0o755));
        assert!(is_executable_mode(0o744)); // owner-only exec still counts
        assert!(is_executable_mode(0o111));
        assert!(!is_executable_mode(0o644));
        assert!(!is_executable_mode(0o000));
    }

    #[test]
    fn marked_summary_counts_and_sums() {
        let v = vec![
            e("a.txt", false, 100, 1),
            e("b.txt", false, 250, 1),
            e("dir", true, 0, 1),
            e("c.txt", false, 4, 1),
        ];
        // Mark a.txt and c.txt only.
        let marked = ["a.txt", "c.txt"];
        let (count, bytes) = marked_summary(&v, |n| marked.contains(&n));
        assert_eq!(count, 2);
        assert_eq!(bytes, 104);
        // Nothing marked → zero of both.
        assert_eq!(marked_summary(&v, |_| false), (0, 0));
    }

    #[test]
    fn destination_into_dir_or_literal() {
        let dir = Path::new("/tmp/dst");
        // Existing directory: keep the basename inside it.
        assert_eq!(
            destination_path(dir, true, "photo.png"),
            PathBuf::from("/tmp/dst/photo.png")
        );
        // Not a directory: the literal target path is used verbatim.
        assert_eq!(
            destination_path(dir, false, "photo.png"),
            PathBuf::from("/tmp/dst")
        );
    }

    #[test]
    fn octal_mode_parsing() {
        assert_eq!(parse_octal_mode("755"), Some(0o755));
        assert_eq!(parse_octal_mode("0644"), Some(0o644));
        assert_eq!(parse_octal_mode("  600 "), Some(0o600));
        assert_eq!(parse_octal_mode(""), None);
        assert_eq!(parse_octal_mode("8"), None); // 8 is not an octal digit
        assert_eq!(parse_octal_mode("nope"), None);
    }

    #[test]
    fn valid_filenames() {
        assert!(is_valid_filename("foo.txt"));
        assert!(is_valid_filename("New Name"));
        assert!(!is_valid_filename(""));
        assert!(!is_valid_filename("."));
        assert!(!is_valid_filename(".."));
        assert!(!is_valid_filename("a/b"));
    }

    #[test]
    fn dirline_navigation() {
        // Indices:      0(d)   1      2(d)   3      4(d)
        let v = vec![
            e("adir", true, 0, 1),
            e("a.txt", false, 1, 1),
            e("bdir", true, 0, 1),
            e("b.txt", false, 1, 1),
            e("cdir", true, 0, 1),
        ];
        assert_eq!(next_dir_index(&v, 0, true), Some(2));
        assert_eq!(next_dir_index(&v, 2, true), Some(4));
        assert_eq!(next_dir_index(&v, 4, true), None); // nothing past the last dir
        assert_eq!(next_dir_index(&v, 4, false), Some(2));
        assert_eq!(next_dir_index(&v, 2, false), Some(0));
        assert_eq!(next_dir_index(&v, 0, false), None); // nothing before the first dir
    }

    #[test]
    fn regexp_rename_first_match_and_backrefs() {
        let re = regex::Regex::new(r"\.jpeg$").unwrap();
        assert_eq!(
            regexp_replace_name("img.jpeg", &re, ".jpg").as_deref(),
            Some("img.jpg")
        );
        // No match -> None (Emacs skips non-matching files).
        assert_eq!(regexp_replace_name("img.png", &re, ".jpg"), None);

        // \& whole match and \1 capture group.
        let re = regex::Regex::new(r"^(\d+)-").unwrap();
        assert_eq!(
            regexp_replace_name("07-song.mp3", &re, "track\\1_").as_deref(),
            Some("track07_song.mp3")
        );
        let re = regex::Regex::new(r"foo").unwrap();
        assert_eq!(
            regexp_replace_name("foobar", &re, "[\\&]").as_deref(),
            Some("[foo]bar")
        );
        // \\ is a literal backslash.
        let re = regex::Regex::new(r"a").unwrap();
        assert_eq!(regexp_replace_name("a", &re, "\\\\").as_deref(), Some("\\"));
    }

    #[test]
    fn numbered_backup_parsing() {
        assert_eq!(parse_numbered_backup("foo.~3~"), Some(("foo", 3)));
        assert_eq!(parse_numbered_backup("foo.c.~12~"), Some(("foo.c", 12)));
        assert_eq!(parse_numbered_backup("foo~"), None); // unnumbered backup
        assert_eq!(parse_numbered_backup("foo.~x~"), None); // non-numeric
        assert_eq!(parse_numbered_backup("foo.txt"), None);
    }

    #[test]
    fn clean_directory_keeps_newest_versions() {
        let names: Vec<String> = [
            "foo.~1~", "foo.~2~", "foo.~3~", "foo.~4~", "bar.~1~", "bar.txt", "keep.rs",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        // Keep the 2 newest per base: foo.~4~,foo.~3~ survive; foo.~2~,foo.~1~ flagged.
        // bar has only one backup (<=2) so nothing flagged there.
        let mut flagged = backups_to_clean(&names, 2);
        flagged.sort();
        assert_eq!(flagged, vec!["foo.~1~".to_string(), "foo.~2~".to_string()]);
        // keep=0 flags every numbered backup.
        let mut all = backups_to_clean(&names, 0);
        all.sort();
        assert_eq!(
            all,
            vec![
                "bar.~1~".to_string(),
                "foo.~1~".to_string(),
                "foo.~2~".to_string(),
                "foo.~3~".to_string(),
                "foo.~4~".to_string(),
            ]
        );
    }

    #[test]
    fn compare_directories_flags_missing_and_changed() {
        let here = vec![
            e("same.txt", false, 100, 1),
            e("changed.txt", false, 200, 1),
            e("only-here.txt", false, 5, 1),
            e("d", true, 0, 1),
        ];
        let there = vec![
            e("same.txt", false, 100, 9),    // same size -> not flagged
            e("changed.txt", false, 250, 9), // different size -> flagged
            e("d", false, 0, 9),             // was dir here, file there -> flagged
        ];
        let mut diff = dirs_differ(&here, &there);
        diff.sort();
        assert_eq!(
            diff,
            vec![
                "changed.txt".to_string(),
                "d".to_string(),
                "only-here.txt".to_string()
            ]
        );
    }

    #[test]
    fn filename_token_at_point() {
        let text = "see /etc/hosts for details";
        // pos inside the path
        assert_eq!(filename_at_point(text, 6).as_deref(), Some("/etc/hosts"));
        // brackets and quotes bound the token
        assert_eq!(
            filename_at_point("open (src/main.rs)", 8).as_deref(),
            Some("src/main.rs")
        );
        // surrounded by whitespace on both sides -> no token
        assert_eq!(filename_at_point("a  b", 2), None);
        assert_eq!(filename_at_point("", 0), None);
    }

    #[test]
    fn relative_paths() {
        assert_eq!(
            relative_path(Path::new("/a/b/c"), Path::new("/a/b/target")),
            PathBuf::from("../target")
        );
        assert_eq!(
            relative_path(Path::new("/a/b"), Path::new("/a/b/sub/f")),
            PathBuf::from("sub/f")
        );
        assert_eq!(
            relative_path(Path::new("/a/x"), Path::new("/a/y/f")),
            PathBuf::from("../y/f")
        );
        assert_eq!(
            relative_path(Path::new("/a/b"), Path::new("/a/b")),
            PathBuf::from(".")
        );
    }
}
